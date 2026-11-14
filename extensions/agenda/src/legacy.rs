//! Before the tracker seam, the agenda knew one tracker, Jira, and what
//! it kept and was sent was Jira's. This is all that's left of that, in
//! one place, until the agenda's 2.0 drops it (`plans/tracker-seam.md`
//! §7):
//!
//! - **The Jira extension 1.2**, which doesn't serve the seam: asked
//!   `jira.tracker.about`, Fenix says nothing offers it, so the agenda
//!   asks `jira.config` and, answered, serves the seam to itself with
//!   the same code the Jira extension 1.3 serves it with
//!   (`fenix_jira_client::tracker`), over `jira.request`.
//! - **`agenda.json` from 1.2**: a link without a tracker is Jira's, its
//!   project is its key's, its stage is in Jira's words, and an error
//!   that began "request failed" is worth retrying.
//! - **Calls without `tracker`** (`agenda.add_issue`, `agenda.open_task`
//!   from Jira 1.2 and the notebook) are for Jira.
//! - **An outbox from 1.2** may hold Jira's Flagged field set or cleared
//!   (`OpKind::SetFlag`): sent as Jira's own moves for it.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use fenix_jira_client::tracker::{self as jira, Keys, Learned, Seam, Settings};
use fenix_jira_client::{JiraClient, Reply, Transport};
use fenix_tracker::{self as seam, code, SeamError};

use crate::calls;
use crate::model::AgendaStore;

/// The tracker a link, or a call, without one is: the only one there was.
pub const ID: &str = "jira";

/// Jira's move for its Flagged field set (`true`) or cleared.
pub fn flag_move(on: bool) -> &'static str {
    if on {
        "flag"
    } else {
        "unflag"
    }
}

/// `agenda.json` as 1.2 wrote it, read as this one keeps it.
pub fn migrate(store: &mut AgendaStore) {
    let stage = |category: &mut String| {
        let words = match category.as_str() {
            "new" => Some("todo"),
            "indeterminate" => Some("doing"),
            _ => None,
        };
        if let Some(w) = words {
            *category = w.to_string();
        }
    };
    for task in &mut store.tasks {
        let Some(link) = task.link.as_mut() else { continue };
        if link.tracker == ID && link.base.project.is_none() {
            link.base.project = Some(fenix_jira_client::project_of(&link.key).to_string());
        }
        stage(&mut link.base.status_category);
        for conflict in &mut link.conflicts {
            stage(&mut conflict.theirs.snapshot.status_category);
        }
        if link.error.as_deref().is_some_and(|e| e.starts_with("request failed")) {
            link.transient = true;
        }
    }
    for op in &mut store.outbox {
        if let crate::model::OpKind::Transition { to_category, .. } = &mut op.kind {
            stage(to_category);
        }
    }
}

/// Jira, through the Jira extension 1.2's `jira.request`.
struct ViaJira;

impl Transport for ViaJira {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
        let payload = serde_json::json!({ "method": method, "path": path, "body": body }).to_string();
        let reply = calls::call("jira.request", &payload);
        Box::pin(async move {
            let answer = reply.await?;
            let v: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
            let status = v.get("status").and_then(|s| s.as_u64()).unwrap_or(0) as u16;
            Ok((status, v.get("body").and_then(|b| b.as_str()).unwrap_or_default().to_string()))
        })
    }
}

/// The seam, served by the agenda itself for the Jira extension 1.2.
pub struct Legacy {
    seam: RefCell<Seam>,
}

/// The keys Jira's messages name.
fn keys() -> Keys {
    Keys { settings: crate::keys::keys_for("settings.open", "SPC ,"), page: crate::keys::keys_for("jira.open", "SPC j j") }
}

/// The Jira extension 1.2, when it's there: its settings, from
/// `jira.config`.
pub async fn probe() -> Option<Rc<Legacy>> {
    let config = calls::call("jira.config", "null").await.ok()?;
    let settings = Settings::from_config(&config, keys()).ok()?;
    let seam = Seam::new(JiraClient::new(Rc::new(ViaJira)), settings, Rc::new(RefCell::new(Learned::default())));
    Some(Rc::new(Legacy { seam: RefCell::new(seam) }))
}

impl Legacy {
    pub fn about(&self) -> seam::About {
        let settings = &self.seam.borrow().settings;
        jira::about(settings, settings.base().is_some())
    }

    /// Seam call `call`, as the Jira extension 1.3 would answer it.
    pub async fn serve(&self, call: &str, payload: &str) -> Result<String, SeamError> {
        match call {
            seam::call::ABOUT => serde_json::to_string(&self.about()).map_err(|e| SeamError::new(code::FAILED, e.to_string())),
            seam::call::REMEMBER => {
                let req: seam::Remember = serde_json::from_str(payload).map_err(|e| SeamError::new(code::BAD_REQUEST, e.to_string()))?;
                let Some((project, blocked)) = jira::parse_remember(&req.remember) else {
                    return Err(SeamError::new(code::BAD_REQUEST, "what to remember isn't Jira's"));
                };
                {
                    let blocked_now = &mut self.seam.borrow_mut().settings.blocked;
                    match blocked_now.iter_mut().find(|(p, _)| *p == project) {
                        Some(entry) => entry.1 = blocked.text(),
                        None => blocked_now.push((project.clone(), blocked.text())),
                    }
                }
                let payload = serde_json::json!({ "project": project, "blocked": blocked.text() }).to_string();
                calls::call("jira.set_blocked", &payload).await.map_err(|e| SeamError::from_err(&e))?;
                Ok("{}".into())
            }
            seam::call::CREATE => {
                let req: seam::Create = serde_json::from_str(payload).map_err(|e| SeamError::new(code::BAD_REQUEST, e.to_string()))?;
                let payload = serde_json::json!({ "summary": req.title, "description": req.body }).to_string();
                let key = calls::call_unbounded("jira.new_issue", &payload).await.map_err(|e| SeamError::new(code::FAILED, e))?;
                Ok(serde_json::json!({ "ref": (!key.is_empty()).then_some(key) }).to_string())
            }
            _ => {
                let seam = self.seam.borrow().clone();
                seam.serve(call, payload).await.map_err(|e| SeamError::from_err(&e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OpKind, Priority, RemoteSnapshot, RemoteUpdate, Status};

    #[test]
    fn a_link_from_1_2_is_jiras_with_its_project_and_stage() {
        let json = r#"{"tasks":[{"id":0,"title":"Old","description":"","status":"InProgress","priority":"High","category":null,
            "notes":[],"time_entries":[],"subtasks":[],"depends_on":[],"created_at":"2026-01-01T10:00:00+00:00","updated_at":"2026-01-01T10:00:00+00:00",
            "done_at":null,"archived":false,"order":0,
            "jira":{"key":"OPS-12","base":{"summary":"Old","description":"","status_id":"3","status_name":"In Progress","status_category":"indeterminate",
              "priority":"Major","assignee":null,"assignee_id":null,"flagged":false,"updated":"x","comments":[]},
              "last_synced":null,"error":"request failed: offline","conflicts":[],"not_mine":false}}],
            "next_id":1,"active_timer":null,
            "outbox":[{"id":1,"task":0,"kind":{"Transition":{"id":"31","to_id":"1","to_name":"Open","to_category":"new"}}},{"id":2,"task":0,"kind":{"SetFlag":true}}]}"#;
        let mut store: AgendaStore = serde_json::from_str(json).unwrap();
        migrate(&mut store);
        let link = store.tasks[0].link.as_ref().unwrap();
        assert_eq!((link.tracker.as_str(), link.project(), link.base.status_category.as_str(), link.transient), ("jira", Some("OPS"), "doing", true));
        assert_eq!(link.column(), None, "not known until the first sync");
        assert!(matches!(&store.outbox[0].kind, OpKind::Transition { to_category, .. } if to_category == "todo"));
        assert_eq!(store.outbox[1].kind, OpKind::SetFlag(true));
        assert_eq!(flag_move(true), "flag");
    }

    #[test]
    fn this_agendas_file_still_reads_in_1_2s_shape() {
        let mut store = AgendaStore::default();
        let id = store.create_task("T".into(), String::new(), Priority::Low, None);
        let update = RemoteUpdate { snapshot: RemoteSnapshot { summary: "T".into(), column: Some("doing".into()), ..Default::default() }, status: Status::InProgress, priority: Priority::Low, mine: None };
        store.link(id, "github-issues", "o/r#1".into(), update);
        let json: serde_json::Value = serde_json::to_value(&store).unwrap();
        let link = &json["tasks"][0]["jira"];
        assert_eq!(link["key"], "o/r#1");
        assert_eq!(link["tracker"], "github-issues");
        // What 1.2 requires is all there, by the names it had.
        for field in ["summary", "description", "status_id", "status_name", "status_category", "priority", "assignee", "assignee_id", "flagged", "updated", "comments"] {
            assert!(link["base"].get(field).is_some(), "{field}");
        }
    }
}
