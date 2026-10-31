//! Jira, as a WebAssembly extension: the Jira page (`SPC j j`) -- your
//! searches, their issues, a preview, and an issue's own page -- and
//! Jira for the agenda. It holds your server, your token and Jira's
//! settings; the agenda reaches Jira through its services:
//!
//! - `jira.request`: a request, sent as you (`{method, path, body}` ->
//!   `{status, body}`)
//! - `jira.config`: Jira's settings, as the agenda needs them
//! - `jira.set_blocked`: what Blocked means in a project, kept
//! - `jira.new_issue`: the new-issue form, filled from a task, answered
//!   with the key it made

pub mod dates;
pub mod page;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use fenix_extension_api::{self as api, host, run, Event, Extension, HttpRequest, Key, Page, PickerItem};
use fenix_jira_client::{IssueDetail, IssueSummary, JiraClient, Reply, Transport};

use page::{Action as A, Choice, JiraPage, Loaded, QueryKind};

/// A service call it answers later: Fenix's number for it.
pub type Waiting = u64;

/// How many issues a search shows.
const SEARCH_LIMIT: u32 = 100;

/// The page's kind, for session restore.
const KIND: &str = "jira";

/// Jira itself: your server, as you.
struct Direct {
    base: String,
    token: String,
}

impl Transport for Direct {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
        let mut req = HttpRequest { method: method.into(), url: format!("{}{path}", self.base), headers: Vec::new(), body: body.map(String::into_bytes).unwrap_or_default(), options: Default::default() }
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/json");
        if !req.body.is_empty() {
            req = req.header("Content-Type", "application/json");
        }
        Box::pin(async move { run::fetch(req).await.map(|r| (r.status, r.text())).map_err(|e| format!("request failed: {e}")) })
    }
}

/// One of its map settings (`jira.projects`): pairs, kept as JSON.
fn map(key: &str) -> Vec<(String, String)> {
    api::setting(key).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn set_map(key: &str, entries: &[(String, String)]) {
    let value = (!entries.is_empty()).then(|| serde_json::to_string(entries).unwrap_or_default());
    if let Err(err) = api::set_setting(key, value.as_deref()) {
        host::notify(api::Level::Error, &format!("couldn't save {key}: {err}"));
    }
}

/// Where Jira is, when it's set up.
fn base_url() -> Option<String> {
    api::setting("jira.base_url").map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
}

/// A client, or why there's none.
fn client() -> Result<JiraClient, String> {
    match (base_url(), host::secret("jira.token").filter(|t| !t.trim().is_empty())) {
        (Some(base), Some(token)) => Ok(JiraClient::new(Rc::new(Direct { base, token }))),
        _ => Err("Jira isn't set up yet -- set its server and token in SPC , (Jira)".into()),
    }
}

/// A job's end, taken in by `Jira::take_done`.
enum Done {
    Issues { jql: String, result: Result<Vec<IssueSummary>, String> },
    Detail { key: String, result: Result<IssueDetail, String> },
    Offer { title: String, key: String, result: Result<Vec<(String, Choice)>, String> },
    Types { project: String, types: Result<Vec<fenix_jira_client::IssueType>, String>, priorities: Vec<String> },
    Fields { type_id: String, result: Result<Vec<fenix_jira_client::CreateField>, String> },
    /// A write to issue `key`: what to say, or why it failed.
    Wrote { key: String, result: Result<String, String> },
    Created { result: Result<String, String>, agenda: bool, clock: bool, task: Option<Waiting> },
    /// What Blocked means in `project`: its statuses to pick from.
    Statuses { project: String, result: Result<Vec<fenix_jira_client::StatusInfo>, String> },
    /// The issues in the agenda.
    InAgenda(HashSet<String>),
}

thread_local! {
    static DONE: std::cell::RefCell<Vec<Done>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn spawn(job: impl Future<Output = Done> + 'static) {
    run::spawn(async move {
        let done = job.await;
        DONE.with(|d| d.borrow_mut().push(done));
    });
}

/// What a picker, prompt or compose pane is for.
enum Asked {
    Description(String),
    Comment(String),
    /// What Blocked means in a project: the meanings offered.
    Blocked(String, Vec<String>),
}

#[derive(Default)]
pub struct Jira {
    page: Option<(u64, JiraPage)>,
    /// Keys of the issues in the agenda.
    in_agenda: HashSet<String>,
    /// The token owner's username.
    me: Option<String>,
    asked: HashMap<u64, Asked>,
}

impl Jira {
    fn queries(&self, here: Option<&str>) -> Vec<page::Query> {
        page::queries(&map("jira.projects"), &map("jira.users"), &map("jira.queries"), here)
    }

    fn ctx_parts(&self) -> (Vec<(String, String)>, String, chrono::NaiveDate) {
        (map("jira.users"), base_url().unwrap_or_default(), fenix_extension_api::clock::now().date_naive())
    }

    fn refresh(&self) {
        if let Some((id, _)) = &self.page {
            api::page::refresh(*id);
        }
    }

    /// Which issues are in the agenda, from the agenda.
    fn ask_agenda(&self) {
        spawn(async {
            let keys = run::call("agenda.linked", "null").await.ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default();
            Done::InAgenda(keys.into_iter().collect())
        });
    }

    /// `SPC j j`: the page, on the searches for the project you're in.
    fn open_page(&mut self) -> u64 {
        let here = api::here().and_then(|h| h.project).and_then(|p| p.meta.into_iter().find(|(k, _)| k == "jira").map(|(_, v)| v));
        self.ask_agenda();
        let id = match &mut self.page {
            Some((id, page)) => {
                if here.is_some() {
                    page.here = here.clone();
                }
                page.note = None;
                *id
            }
            None => {
                let id = api::page::open(KIND, "jira");
                self.page = Some((id, JiraPage::new(self.queries(here.as_deref()), here.clone())));
                id
            }
        };
        let here = self.page.as_ref().and_then(|(_, p)| p.here.clone());
        let queries = self.queries(here.as_deref());
        if let Some((_, page)) = &mut self.page {
            page.set_queries(queries);
        }
        self.fetch_wanted();
        self.refresh();
        id
    }

    fn fetch_wanted(&mut self) {
        let want = self.page.as_mut().and_then(|(_, p)| p.wants_fetch());
        if let Some(jql) = want {
            self.fetch(jql);
        }
    }

    fn fetch(&mut self, jql: String) {
        let client = match client() {
            Ok(client) => client,
            Err(why) => {
                if let Some((_, page)) = &mut self.page {
                    page.results.insert(jql, Loaded::Failed(why));
                }
                return self.refresh();
            }
        };
        if let Some((_, page)) = &mut self.page {
            page.results.insert(jql.clone(), Loaded::Loading);
        }
        spawn(async move {
            let result = client.search_issues(&jql, SEARCH_LIMIT).await;
            Done::Issues { jql, result }
        });
    }

    fn fetch_detail(&mut self, key: String) {
        let Ok(client) = client() else { return };
        spawn(async move {
            let result = client.get_issue(&key).await;
            Done::Detail { key, result }
        });
    }

    /// A write to issue `key`, said when done, and the page brought up
    /// to date.
    fn write<F, Fut>(&mut self, key: String, job: F)
    where
        F: FnOnce(JiraClient) -> Fut + 'static,
        Fut: Future<Output = Result<String, String>> + 'static,
    {
        let client = match client() {
            Ok(client) => client,
            Err(why) => return host::notify(api::Level::Error, &why),
        };
        spawn(async move { Done::Wrote { key, result: job(client).await } });
    }

    /// The search showing, and its issue, again.
    fn refresh_page(&mut self) {
        let Some((_, page)) = &mut self.page else { return };
        let jql = page.query().map(|q| q.jql.clone());
        let keys: Vec<String> = page.details.keys().cloned().collect();
        page.details.clear();
        if let Some(jql) = jql {
            page.results.insert(jql.clone(), Loaded::Loading);
            self.fetch(jql);
        }
        if let Some(key) = keys.into_iter().next() {
            self.fetch_detail(key);
        }
        self.refresh();
    }

    fn issue_url(&self, key: &str) -> Option<String> {
        match base_url() {
            Some(base) => Some(format!("{base}/browse/{key}")),
            None => {
                host::notify(api::Level::Error, "Jira isn't set up -- its server goes on the settings page (SPC ,)");
                None
            }
        }
    }

    /// The projects an issue can be made in, the open one first.
    fn form_projects(&self) -> Vec<String> {
        let mut projects: Vec<String> = self.page.as_ref().and_then(|(_, p)| p.here.clone()).into_iter().collect();
        for (key, _) in map("jira.projects") {
            if !projects.iter().any(|p| p.eq_ignore_ascii_case(&key)) {
                projects.push(key);
            }
        }
        projects
    }

    fn edit_map(&self, key: &str, edit: impl FnOnce(&mut Vec<(String, String)>)) {
        let mut entries = map(key);
        edit(&mut entries);
        set_map(key, &entries);
    }

    /// The searches again, after the settings changed.
    fn settings_changed(&mut self) {
        let here = self.page.as_ref().and_then(|(_, p)| p.here.clone());
        let queries = self.queries(here.as_deref());
        if let Some((_, page)) = &mut self.page {
            page.set_queries(queries);
        }
        self.fetch_wanted();
        self.refresh();
    }

    fn action(&mut self, action: A) {
        match action {
            A::None => {}
            A::Close => {
                if let Some((id, _)) = self.page.take() {
                    api::page::close(id);
                }
            }
            A::Fetch(jql) => self.fetch(jql),
            A::Detail(key) => self.fetch_detail(key),
            A::Refresh => self.refresh_page(),
            A::OpenTask(key) => {
                let payload = serde_json::json!({ "key": key }).to_string();
                spawn(async move {
                    let _ = run::call("agenda.open_task", &payload).await;
                    Done::InAgenda(HashSet::new())
                });
            }
            A::Transitions(key) => {
                let Ok(client) = client() else { return };
                spawn(async move {
                    let result = client.list_transitions(&key).await.map(|ts| {
                        ts.into_iter().map(|t| (format!("{}  →  {}", t.name, if t.to_name.is_empty() { t.name.clone() } else { t.to_name.clone() }), Choice::Transition { id: t.id, name: t.name })).collect()
                    });
                    Done::Offer { title: format!("Move {key}"), key, result }
                });
            }
            A::Priorities(key) => {
                let Ok(client) = client() else { return };
                spawn(async move {
                    let result = client.list_priorities().await.map(|ps| ps.into_iter().map(|p| (p.name.clone(), Choice::Priority(p.name))).collect());
                    Done::Offer { title: format!("{key} priority"), key, result }
                });
            }
            A::Transition { key, id: transition, name } => {
                let k = key.clone();
                self.write(key, move |c| async move { c.apply_transition(&k, &transition).await.map(|_| format!("{k}: {name}")) });
            }
            A::SetPriority { key, name } => {
                let k = key.clone();
                self.write(key, move |c| async move { c.update_priority(&k, &name).await.map(|_| format!("{k} is {name} now")) });
            }
            A::Assign { key, id: user, name } => {
                let k = key.clone();
                let me = self.me.clone();
                self.write(key, move |c| async move {
                    let user = if user.is_empty() {
                        match me {
                            Some(me) => me,
                            None => c.myself().await?,
                        }
                    } else {
                        user
                    };
                    c.update_assignee(&k, &user).await.map(|_| format!("{k} assigned to {}", if name.is_empty() { user.as_str() } else { name.as_str() }))
                });
            }
            A::SetDue { key, due } => {
                let k = key.clone();
                let due = due.map(|d| d.format("%Y-%m-%d").to_string());
                self.write(key, move |c| async move {
                    c.update_due(&k, due.as_deref()).await.map(|_| match &due {
                        Some(d) => format!("{k} due {d}"),
                        None => format!("{k} has no due date now"),
                    })
                });
            }
            A::SetTitle { key, title } => {
                let k = key.clone();
                self.write(key, move |c| async move { c.update_summary(&k, &title).await.map(|_| format!("{k} renamed")) });
            }
            A::LogTime { key, time } => {
                let k = key.clone();
                self.write(key, move |c| async move { c.add_worklog(&k, &time).await.map(|_| format!("Logged {time} on {k}")) });
            }
            A::EditDescription(key) => {
                let seed = self.page.as_ref().and_then(|(_, p)| p.details.get(&key).and_then(|d| d.as_ref().ok()).and_then(|d| d.description.clone())).unwrap_or_default();
                let id = api::compose(&format!("{key} description"), "save", &seed, true);
                self.asked.insert(id, Asked::Description(key));
            }
            A::Comment(key) => {
                let id = api::compose(&format!("Comment on {key}"), "post", "", false);
                self.asked.insert(id, Asked::Comment(key));
            }
            A::AddToAgenda { key, clock } => {
                let payload = serde_json::json!({ "key": key, "clock": clock }).to_string();
                spawn(async move {
                    if let Err(why) = run::call("agenda.add_issue", &payload).await {
                        host::notify(api::Level::Error, &format!("couldn't add it to the agenda: {why}"));
                    }
                    Done::InAgenda(HashSet::new())
                });
                self.in_agenda.insert(key);
                self.refresh();
            }
            A::CopyLink(key) => {
                if let Some(url) = self.issue_url(&key) {
                    api::copy(&url);
                    api::notify(&format!("Copied {url}"));
                }
            }
            A::Browser(key) => {
                if let Some(url) = self.issue_url(&key) {
                    let _ = api::open_url(&url);
                }
            }
            A::AddProject { key, name } => {
                self.edit_map("jira.projects", |projects| {
                    if !projects.iter().any(|(k, _)| k.eq_ignore_ascii_case(&key)) {
                        projects.push((key.clone(), name));
                    }
                });
                api::notify(&format!("{key}'s open issues and sprint are on the left now"));
                self.settings_changed();
            }
            A::AddPerson { id: user, name } => {
                self.edit_map("jira.users", |users| {
                    if !users.iter().any(|(u, _)| *u == user) {
                        users.push((user, name));
                    }
                });
                self.settings_changed();
            }
            A::SaveQuery { name, jql } => {
                self.edit_map("jira.queries", |queries| match queries.iter_mut().find(|(n, _)| *n == name) {
                    Some(q) => q.1 = jql,
                    None => queries.push((name.clone(), jql)),
                });
                api::notify(&format!("Saved \"{name}\""));
                self.settings_changed();
            }
            A::RemoveQuery(q) => {
                match &q.kind {
                    QueryKind::Project(key) => self.edit_map("jira.projects", |projects| projects.retain(|(k, _)| k != key)),
                    QueryKind::Person(user) => self.edit_map("jira.users", |users| users.retain(|(u, _)| u != user)),
                    QueryKind::Saved => self.edit_map("jira.queries", |queries| queries.retain(|(n, _)| *n != q.name)),
                    QueryKind::Mine | QueryKind::Search => return,
                }
                self.settings_changed();
            }
            A::Blocked(project) => {
                let Ok(client) = client() else { return };
                spawn(async move {
                    let result = client.list_project_statuses(&project).await;
                    Done::Statuses { project, result }
                });
            }
            A::IssueTypes(project) => {
                let Ok(client) = client() else { return };
                spawn(async move {
                    let types = client.issue_types(&project).await;
                    let priorities = client.list_priorities().await.map(|ps| ps.into_iter().map(|p| p.name).collect()).unwrap_or_default();
                    Done::Types { project, types, priorities }
                });
            }
            A::CreateFields { project, type_id } => {
                let Ok(client) = client() else { return };
                spawn(async move {
                    let result = client.create_fields(&project, &type_id).await;
                    Done::Fields { type_id, result }
                });
            }
            A::Create(create) => {
                let page::Create { issue, agenda, clock, task } = *create;
                let client = match client() {
                    Ok(client) => client,
                    Err(why) => return host::notify(api::Level::Error, &why),
                };
                api::notify(&format!("Creating a {} issue...", issue.project));
                spawn(async move { Done::Created { result: client.create(&issue).await, agenda, clock, task } });
            }
        }
    }

    /// What Blocked means in `project`: its statuses, the Flagged field,
    /// or the agenda only.
    fn blocked_picker(&mut self, project: String, statuses: Vec<fenix_jira_client::StatusInfo>) {
        let current = map("jira.blocked").into_iter().find(|(p, _)| *p == project).map(|(_, m)| m);
        let mark = |meaning: &str| if current.as_deref() == Some(meaning) { "  (current)" } else { "" };
        let mut rows: Vec<(String, String)> = statuses.into_iter().map(|s| (format!("{}{}", s.name, mark(&format!("{}: {}", s.id, s.name))), format!("{}: {}", s.id, s.name))).collect();
        rows.push((format!("Flag as impediment (keep the status){}", mark("flag")), "flag".into()));
        rows.push((format!("Keep Blocked local{}", mark("local")), "local".into()));
        let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
        let id = api::pick(&format!("What does Blocked mean in {project}?"), &items);
        self.asked.insert(id, Asked::Blocked(project, rows.into_iter().map(|(_, m)| m).collect()));
    }

    fn set_blocked(&self, project: &str, meaning: &str) {
        self.edit_map("jira.blocked", |entries| match entries.iter_mut().find(|(p, _)| p == project) {
            Some(entry) => entry.1 = meaning.to_string(),
            None => entries.push((project.to_string(), meaning.to_string())),
        });
    }

    fn take_done(&mut self) {
        let done = DONE.with(|d| std::mem::take(&mut *d.borrow_mut()));
        for done in done {
            match done {
                Done::Issues { jql, result } => {
                    let first = result.as_ref().ok().and_then(|i| i.first().map(|i| i.key.clone()));
                    let mut want = None;
                    if let Some((_, page)) = &mut self.page {
                        let showing = page.query().is_some_and(|q| q.jql == jql);
                        page.results.insert(jql, result.map(Loaded::Issues).unwrap_or_else(Loaded::Failed));
                        // The first issue's preview, once there are issues.
                        want = first.filter(|k| showing && !page.details.contains_key(k));
                    }
                    if let Some(key) = want {
                        self.fetch_detail(key);
                    }
                }
                Done::Detail { key, result } => {
                    if let Some((_, page)) = &mut self.page {
                        page.details.insert(key, result);
                    }
                }
                Done::Offer { title, key, result } => match result {
                    Ok(items) => {
                        if let Some((_, page)) = &mut self.page {
                            page.offer(title, key, items);
                        }
                    }
                    Err(err) => host::notify(api::Level::Error, &format!("{key}: {err}")),
                },
                Done::Types { project, types, priorities } => {
                    let action = self.page.as_mut().map(|(_, p)| p.types_loaded(&project, types, priorities));
                    if let Some(action) = action {
                        self.action(action);
                    }
                }
                Done::Fields { type_id, result } => {
                    if let Some((_, page)) = &mut self.page {
                        page.fields_loaded(&type_id, result);
                    }
                }
                Done::Wrote { key, result } => {
                    match result {
                        Ok(message) => api::notify(&message),
                        Err(err) => host::notify(api::Level::Error, &format!("{key}: {err}")),
                    }
                    if let Some((_, page)) = &mut self.page {
                        page.details.remove(&key);
                        if let Some(jql) = page.query().map(|q| q.jql.clone()) {
                            page.results.remove(&jql);
                        }
                    }
                    self.fetch_wanted();
                    self.fetch_detail(key.clone());
                    // The agenda's task, when the issue is in it, follows.
                    if self.in_agenda.contains(&key) {
                        spawn(async {
                            let _ = run::call("agenda.sync_quietly", "null").await;
                            Done::InAgenda(HashSet::new())
                        });
                    }
                }
                Done::Created { result, agenda, clock, task } => match result {
                    Ok(key) => {
                        match task {
                            // The agenda's task it was made from: linked there.
                            Some(call) => api::answer(call, Ok(key.clone())),
                            None if agenda => {
                                let payload = serde_json::json!({ "key": key, "clock": clock }).to_string();
                                spawn(async move {
                                    let _ = run::call("agenda.add_issue", &payload).await;
                                    Done::InAgenda(HashSet::new())
                                });
                            }
                            None => {}
                        }
                        api::notify(&format!("Created {key}"));
                        self.refresh_page();
                    }
                    Err(err) => {
                        if let Some(call) = task {
                            api::answer(call, Err(err.clone()));
                        }
                        host::notify(api::Level::Error, &format!("couldn't create the issue: {err}"));
                    }
                },
                Done::Statuses { project, result } => match result {
                    Ok(statuses) => self.blocked_picker(project, statuses),
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't list {project}'s statuses: {err}")),
                },
                Done::InAgenda(keys) => {
                    if !keys.is_empty() {
                        self.in_agenda = keys;
                    }
                }
            }
        }
        self.refresh();
    }

    /// `jira.config`: Jira's settings, as the agenda needs them.
    fn config(&self) -> String {
        let minutes: u32 = api::setting("jira.sync_minutes").and_then(|m| m.trim().parse().ok()).unwrap_or(10);
        serde_json::json!({
            "base_url": base_url(),
            "projects": map("jira.projects").into_iter().map(|(k, _)| k).collect::<Vec<_>>(),
            "users": map("jira.users"),
            "priorities": map("jira.priorities"),
            "blocked": map("jira.blocked"),
            "sync_minutes": minutes,
        })
        .to_string()
    }
}

impl Extension for Jira {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "jira.open" => {
                self.open_page();
            }
            "jira.refresh" => self.refresh_page(),
            "jira.goto_issue" => {
                self.open_page();
                if let Some((_, page)) = &mut self.page {
                    page.start_goto();
                }
            }
            "jira.search" => {
                self.open_page();
                if let Some((_, page)) = &mut self.page {
                    page.start_search();
                }
            }
            "jira.create_issue" => {
                self.open_page();
                let projects = self.form_projects();
                let action = self.page.as_mut().map(|(_, p)| p.start_issue(projects, None));
                if let Some(action) = action {
                    self.action(action);
                }
            }
            other => return Err(format!("no command {other}")),
        }
        self.refresh();
        self.take_done();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let event = run::deliver(event);
        self.take_done();
        let Some(event) = event else { return };
        match event {
            Event::SettingChanged(_) => self.settings_changed(),
            Event::Composed((id, Some(body))) => match self.asked.remove(&id) {
                Some(Asked::Description(key)) => {
                    let k = key.clone();
                    self.write(key, move |c| async move { c.update_description(&k, &body).await.map(|_| format!("Description updated for {k}")) });
                }
                Some(Asked::Comment(key)) => {
                    let k = key.clone();
                    self.write(key, move |c| async move { c.add_comment(&k, &body).await.map(|_| format!("Comment added to {k}")) });
                }
                _ => {}
            },
            Event::Picked((id, choice)) => {
                if let Some(Asked::Blocked(project, meanings)) = self.asked.remove(&id) {
                    if let Some(meaning) = choice.and_then(|c| c.parse::<usize>().ok()).and_then(|i| meanings.get(i)) {
                        self.set_blocked(&project, meaning);
                        api::notify(&format!("Blocked in {project} now means {meaning}"));
                    }
                }
            }
            Event::Composed((id, None)) => {
                self.asked.remove(&id);
            }
            _ => {}
        }
        self.take_done();
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        let Some((id, p)) = &self.page else { return Page::new() };
        if *id != page {
            return Page::new();
        }
        let (people, server, today) = self.ctx_parts();
        let ctx = page::Ctx { in_agenda: &self.in_agenda, people: &people, server, today };
        Page::canvas(page::layout(p, &ctx, width as usize)).typing(p.typing()).space(p.claims_space())
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some((id, mut p)) = self.page.take() else { return };
        if id != page {
            self.page = Some((id, p));
            return;
        }
        let (people, server, today) = self.ctx_parts();
        let ctx = page::Ctx { in_agenda: &self.in_agenda, people: &people, server, today };
        let action = p.key(key, &ctx);
        self.page = Some((id, p));
        self.action(action);
        self.refresh();
        self.take_done();
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some((id, p)) = &mut self.page {
            if *id == page {
                p.paste(text);
            }
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        let (id, p) = self.page.as_ref()?;
        (*id == page).then(|| p.here.clone().unwrap_or_default())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        let here = (!state.is_empty()).then(|| state.to_string());
        self.page = Some((page, JiraPage::new(self.queries(here.as_deref()), here)));
        self.ask_agenda();
        self.fetch_wanted();
        true
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = match name {
            "jira.request" => {
                #[derive(serde::Deserialize)]
                struct Request {
                    method: String,
                    path: String,
                    body: Option<String>,
                }
                let req: Request = match serde_json::from_str(payload) {
                    Ok(req) => req,
                    Err(e) => return Some(Err(format!("a Jira request: {e}"))),
                };
                let (base, token) = match (base_url(), host::secret("jira.token").filter(|t| !t.trim().is_empty())) {
                    (Some(base), Some(token)) => (base, token),
                    _ => return Some(Err("Jira isn't set up yet -- set its server and token in SPC , (Jira)".into())),
                };
                let direct = Direct { base, token };
                let sent = direct.send(&req.method, &req.path, req.body);
                run::spawn(async move {
                    let answer = sent.await.map(|(status, body)| serde_json::json!({ "status": status, "body": body }).to_string());
                    api::answer(call, answer);
                });
                return None;
            }
            "jira.config" => Ok(self.config()),
            "jira.set_blocked" => {
                #[derive(serde::Deserialize)]
                struct Set {
                    project: String,
                    blocked: String,
                }
                match serde_json::from_str::<Set>(payload) {
                    Ok(set) => {
                        self.set_blocked(&set.project, &set.blocked);
                        Ok(String::new())
                    }
                    Err(e) => Err(format!("a Jira call: {e}")),
                }
            }
            // The form, filled from the task; answered once it's made.
            "jira.new_issue" => {
                #[derive(serde::Deserialize)]
                struct New {
                    summary: String,
                    #[serde(default)]
                    description: String,
                }
                let new: New = match serde_json::from_str(payload) {
                    Ok(new) => new,
                    Err(e) => return Some(Err(format!("a Jira call: {e}"))),
                };
                self.open_page();
                let projects = self.form_projects();
                let action = self.page.as_mut().map(|(_, p)| p.start_issue(projects, Some((call, new.summary, new.description))));
                if let Some(action) = action {
                    self.action(action);
                }
                self.take_done();
                return None;
            }
            other => Err(format!("no service {other}")),
        };
        Some(answer)
    }
}

api::export!(Jira);
