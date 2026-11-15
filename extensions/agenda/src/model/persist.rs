#[cfg(test)]
use std::path::PathBuf;

use super::store::AgendaStore;
use super::task::TaskId;

/// Beside `agenda.json`: every link that isn't Jira's, as of this
/// agenda's last save. Agenda 1.2 reads only Jira's links, and drops the
/// others when it saves `agenda.json`; this file it never touches, so a
/// link 1.2 dropped is found here again (`reconcile`).
pub const LINKS: &str = "agenda-links.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Links {
    links: Vec<Kept>,
}

/// A link, and the task it's on: by its id and when it was made, so a
/// task 1.2 deleted (and another took its id) isn't given it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Kept {
    task: TaskId,
    created_at: chrono::DateTime<crate::clock::Local>,
    link: super::link::IssueLink,
}

fn links_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_file_name(LINKS)
}

/// Links the side file has that `agenda.json` lost (1.2 saved it) put
/// back, and changes left waiting for a task with no link dropped, with
/// a note.
fn reconcile(path: &std::path::Path, store: &mut AgendaStore) {
    let kept: Links = std::fs::read_to_string(links_path(path)).ok().and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default();
    for k in kept.links {
        if let Some(task) = store.tasks.iter_mut().find(|t| t.id == k.task && t.created_at == k.created_at && t.link.is_none()) {
            task.link = Some(k.link);
        }
    }
    store.drop_orphans();
}

/// Loads the store from `path`. A missing or corrupt file loads as an
/// empty store rather than failing -- the agenda is a convenience, not
/// critical data that should refuse to open Fenix over, same posture
/// `fenix-config`'s `load_or_default` already has for `config.ini`.
pub fn load(path: &std::path::Path) -> AgendaStore {
    let mut store: AgendaStore = std::fs::read_to_string(path).ok().and_then(|contents| serde_json::from_str(&contents).ok()).unwrap_or_default();
    reconcile(path, &mut store);
    store
}

/// Reads the store from `path` for a reload: `None` when the file isn't
/// there, an error when it can't be read or isn't an agenda (a sync
/// tool's half-written copy, a hand edit gone wrong). Unlike `load`, a
/// bad file isn't taken for an empty agenda, so the caller can keep the
/// one it has rather than lose it at the next save.
pub fn reload(path: &std::path::Path) -> std::io::Result<Option<AgendaStore>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut store: AgendaStore = serde_json::from_str(&contents).map_err(std::io::Error::other)?;
    reconcile(path, &mut store);
    Ok(Some(store))
}

pub fn save(path: &std::path::Path, store: &AgendaStore) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // The links 1.2 would drop, first: a crash between the two leaves a
    // link kept twice, never none.
    let links = Links {
        links: store
            .tasks
            .iter()
            .filter_map(|t| Some(Kept { task: t.id, created_at: t.created_at, link: t.link.clone().filter(|l| l.tracker != crate::legacy::ID)? }))
            .collect(),
    };
    let side = links_path(path);
    if links.links.is_empty() {
        match std::fs::remove_file(&side) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    } else {
        write(&side, &serde_json::to_vec_pretty(&links).map_err(std::io::Error::other)?)?;
    }
    write(path, &serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?)
}

/// Written beside it, then put in its place: a crash mid-write leaves
/// the last good one.
fn write(path: &std::path::Path, json: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, json)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("fenix-agenda-test-{name}-{}-{n}.json", std::process::id()))
    }

    #[test]
    fn loading_a_missing_file_yields_an_empty_store_not_an_error() {
        let store = load(&temp_path("missing"));
        assert!(store.tasks.is_empty());
        assert!(store.active_timer.is_none());
    }

    #[test]
    fn loading_a_corrupt_file_yields_an_empty_store_not_a_panic() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"not json").unwrap();
        let store = load(&path);
        assert!(store.tasks.is_empty());
    }

    #[test]
    fn reload_tells_a_missing_file_from_a_bad_one() {
        assert!(reload(&temp_path("reload-missing")).unwrap().is_none());
        let path = temp_path("reload-bad");
        std::fs::write(&path, b"{\"tasks\": [").unwrap();
        assert!(reload(&path).is_err());
        let mut store = AgendaStore::default();
        store.create_task("Ship it".to_string(), String::new(), Priority::Medium, None);
        save(&path, &store).unwrap();
        assert_eq!(reload(&path).unwrap().unwrap().tasks[0].title, "Ship it");
    }

    #[test]
    fn a_store_round_trips_through_save_and_load() {
        let path = temp_path("round_trip");
        let mut store = AgendaStore::default();
        let a = store.create_task("Write the plan".to_string(), "details".to_string(), Priority::High, Some("Fenix".to_string()));
        let b = store.create_task("Review it".to_string(), "".to_string(), Priority::Low, None);
        store.add_dependency(b, a);
        store.add_subtask(a, "Draft outline".to_string());
        store.add_note(a, "started".to_string());
        store.log_manual_time(a, chrono::Duration::minutes(30));
        store.clock_in(b);

        save(&path, &store).unwrap();
        let reloaded = load(&path);

        assert_eq!(reloaded.tasks.len(), 2);
        assert_eq!(reloaded.task(a).unwrap().title, "Write the plan");
        assert_eq!(reloaded.task(b).unwrap().depends_on, vec![a]);
        assert_eq!(reloaded.task(a).unwrap().subtasks.len(), 1);
        assert_eq!(reloaded.task(a).unwrap().notes.len(), 1);
        assert_eq!(reloaded.task(a).unwrap().time_entries.len(), 1);
        assert_eq!(reloaded.active_timer.as_ref().map(|t| t.task_id), Some(b));
    }
}
