//! Linear, as a WebAssembly extension: its page (`SPC o w`) -- your issues, a
//! team's cycle and backlog, an issue with its comments, a new-issue form,
//! a branch started from an issue -- Linear for the agenda through the
//! tracker seam (`linear.tracker.*`, `fenix_linear_client::seam`), and two
//! read-only tools for coding agents.
//!
//! It holds your API key: Fenix's secret storage, `linear.token`, read when
//! a request is made and written into that request's `Authorization`
//! header and nowhere else. Nothing here logs, notifies, answers or stores
//! it, and an error that carries it is scrubbed first.

pub mod page;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{host, run, Command, Event, Extension, HttpRequest, Key, Page, PickerItem};
use fenix_linear_client::branch::{self, Outcome, Ran};
use fenix_linear_client::linear::{Listed, NewIssue, TeamInfo, TeamMeta};
use fenix_linear_client::model::Detail;
use fenix_linear_client::seam::{self, Keys, Seam};
use fenix_linear_client::{filter, refs, tools, Client, Linear, Reply, Sent, Transport, View, ENDPOINT};
use fenix_tracker::{code, SeamError};

use page::{Action as A, Ctx, LinearPage, Purpose, ViewId};

/// The page's kind, for session restore.
const KIND: &str = "linear";
/// The secret that holds the API key.
const TOKEN: &str = "linear.token";
/// The project's folder, where `git` runs.
const PROJECT: &str = "/project";

/// Seam service `call` of this tracker: the one place its names are made.
fn service(call: &str) -> String {
    fenix_tracker::service(seam::ID, call)
}

/// The tools' services.
const TOOL_ISSUE: &str = "linear.tool.read_issue";
const TOOL_MINE: &str = "linear.tool.list_mine";

// -- The key and the transport ---------------------------------------------------------

/// The API key, or why there's none. A key with a line break or a control
/// character in it can't be put in a header: it's refused, not sent.
fn token() -> Result<String, String> {
    let key = host::secret(TOKEN).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    match key {
        Some(k) if k.chars().any(|c| c.is_control()) => Err(format!("the API key in {TOKEN} has a line break or control character in it -- set it again")),
        Some(k) => Ok(k),
        None => Err(seam::not_ready(&keys())),
    }
}

/// Linear itself: your key in the `Authorization` header, as Linear's
/// personal API keys are sent (no `Bearer`).
struct Direct {
    key: String,
}

impl Transport for Direct {
    fn send(&self, body: String) -> Sent {
        let req = HttpRequest::post(ENDPOINT, body.into_bytes()).header("Authorization", &self.key).header("Content-Type", "application/json").header("Accept", "application/json");
        let key = self.key.clone();
        Box::pin(async move {
            io::fetch(req).await.map(|r| Reply { status: r.status, headers: r.headers.clone(), body: r.text() }).map_err(|e| e.replace(&key, "***"))
        })
    }
}

fn now_secs() -> i64 {
    api::clock::now().timestamp()
}

fn linear() -> Result<Linear, String> {
    let key = token()?;
    Ok(Linear::new(Client::new(Rc::new(Direct { key }), Rc::new(now_secs))))
}

fn keys() -> Keys {
    Keys { settings: settings_keys(), page: page_keys() }
}

fn settings_keys() -> String {
    api::key_for("settings.open").unwrap_or_else(|| "SPC ,".into())
}

fn page_keys() -> String {
    api::key_for("linear.open").unwrap_or_else(|| "SPC o w w".into())
}

fn seam_settings() -> seam::Settings {
    seam::Settings {
        ready: token().is_ok(),
        sync_minutes: api::setting("linear.sync_minutes").and_then(|m| m.trim().parse().ok()).unwrap_or(10),
        keys: keys(),
        team: api::setting("linear.team").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
    }
}

fn teams_setting() -> Vec<String> {
    api::setting("linear.teams").map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

fn branch_pattern() -> String {
    api::setting("linear.branch_pattern").unwrap_or_default()
}


/// What waits on Fenix: HTTP, services, timers. In the WebAssembly, the
/// API's `run`; in tests, the fake host answering at once (the API's own
/// `run` calls the real host, which panics natively).
#[cfg(not(test))]
mod io {
    use super::*;

    pub async fn fetch(req: HttpRequest) -> Result<api::HttpResponse, String> {
        run::fetch(req).await
    }

    pub async fn call(name: &str, payload: &str) -> Result<String, String> {
        run::call(name, payload).await
    }

    pub async fn sleep(ms: u32) {
        run::sleep(ms).await
    }
}

#[cfg(test)]
mod io {
    use super::*;
    use fenix_test_kit::Fake;

    pub async fn fetch(req: HttpRequest) -> Result<api::HttpResponse, String> {
        let id = host::http(&req)?;
        while let Some(event) = Fake::current().next_event() {
            if let Event::HttpResponse((got, answer)) = event {
                if got == id {
                    return answer;
                }
            }
        }
        Err(format!("the fake had no answer for {}", req.url))
    }

    pub async fn call(name: &str, payload: &str) -> Result<String, String> {
        let id = host::call_service(name, payload);
        while let Some(event) = Fake::current().next_event() {
            if let Event::ServiceReply((got, answer)) = event {
                if got == id {
                    return answer;
                }
            }
        }
        Err(format!("nothing offers {name}"))
    }

    /// Programs' events come as the fake sends them: taken in here.
    pub async fn sleep(_ms: u32) {
        while let Some(event) = Fake::current().next_event() {
            let _ = deliver_process(event);
        }
    }
}

// -- git, run in the project -------------------------------------------------------------

#[derive(Default)]
struct Running {
    out: Vec<String>,
    err: Vec<String>,
    exit: Option<Option<i32>>,
}

thread_local! {
    static RUNNING: RefCell<HashMap<u64, Running>> = RefCell::new(HashMap::new());
    static DONE: RefCell<Vec<Done>> = const { RefCell::new(Vec::new()) };
}

/// An event about a program run here is kept for the job waiting on it.
fn deliver_process(event: Event) -> Option<Event> {
    let id = match &event {
        Event::ProcessOutput(o) => o.process,
        Event::ProcessExit((id, _)) => *id,
        _ => return Some(event),
    };
    RUNNING.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.get_mut(&id) else { return Some(event) };
        match event {
            Event::ProcessOutput(o) if o.stderr => run.err.push(o.line),
            Event::ProcessOutput(o) => run.out.push(o.line),
            Event::ProcessExit((_, code)) => run.exit = Some(code),
            _ => {}
        }
        None
    })
}

/// `git args` in the project, never waiting on an editor or a prompt.
fn git(args: Vec<String>) -> Pin<Box<dyn Future<Output = Result<Ran, String>>>> {
    Box::pin(async move {
        let cmd = Command::new("git").args(args.iter().cloned()).cwd(PROJECT).env("GIT_EDITOR", "true").env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never");
        let id = host::spawn(&cmd)?;
        RUNNING.with(|r| r.borrow_mut().insert(id, Running::default()));
        loop {
            let done = RUNNING.with(|r| {
                let mut r = r.borrow_mut();
                if r.get(&id).is_some_and(|run| run.exit.is_some()) {
                    r.remove(&id)
                } else {
                    None
                }
            });
            if let Some(r) = done {
                return Ok(Ran { ok: r.exit == Some(Some(0)), stdout: r.out.join("\n"), stderr: r.err.join("\n") });
            }
            io::sleep(15).await;
        }
    })
}

// -- Jobs ---------------------------------------------------------------------------------

enum Done {
    List { view: ViewId, after: Option<String>, result: Result<Listed, String> },
    Teams(Result<Vec<TeamInfo>, String>),
    Detail { key: String, result: Result<Box<Detail>, String> },
    Meta { team: String, result: Result<TeamMeta, String> },
    Menu { purpose: Purpose, title: String, items: Vec<(String, String)>, current: Option<String>, key: String, error: Option<String> },
    /// A write to `key`, said when done, and the page brought up to date.
    Wrote { key: String, result: Result<String, String> },
    Created { result: Result<fenix_linear_client::linear::Created, String> },
    /// What a branch came to: said, asked about, or done.
    Branch { key: String, result: Result<Outcome, String>, name: Option<String> },
    InAgenda(HashSet<String>),
}

fn spawn(job: impl Future<Output = Done> + 'static) {
    run::spawn(async move {
        let done = job.await;
        DONE.with(|d| d.borrow_mut().push(done));
    });
}

/// What a picker or prompt is for.
enum Asked {
    Title(String),
    Due(String),
    Description(String),
    Comment(String),
    /// Whether to carry a dirty tree's changes to a new branch.
    Dirty(String),
    /// The write gate's question (`fenix_tracker::gate`).
    Gate,
}

#[derive(Default)]
pub struct LinearExt {
    page: Option<(u64, LinearPage)>,
    asked: HashMap<u64, Asked>,
    /// Whether the seam may write this session, and what waits on it.
    gate: fenix_tracker::gate::Gate,
    held: Vec<Held>,
    /// The seam's writes done this session, by `op`.
    done_ops: Rc<RefCell<fenix_tracker::Dedupe>>,
}

/// A seam write held while you're asked whether to allow it.
struct Held {
    call: u64,
    seam_call: &'static str,
    payload: String,
}

fn say(message: &str) {
    api::notify(message);
}

fn complain(message: &str) {
    host::notify(api::Level::Error, message);
}

/// A failure as a person is told it: the seam's words, with a refused key
/// pointing at the settings.
fn told(e: SeamError) -> String {
    seam::refit(e, &keys()).message
}

impl LinearExt {
    fn ctx(&self) -> Ctx {
        Ctx { today: api::clock::now().format("%Y-%m-%d").to_string(), settings_keys: settings_keys(), ready: token().is_ok() }
    }

    fn refresh(&self) {
        if let Some((id, _)) = &self.page {
            api::page::refresh(*id);
        }
    }

    fn open_page(&mut self) -> u64 {
        let id = match &self.page {
            Some((id, _)) => *id,
            None => {
                let id = api::page::open(KIND, "linear");
                let mut p = LinearPage::new(teams_setting());
                if token().is_err() {
                    p.teams = Some(Vec::new());
                }
                self.page = Some((id, p));
                id
            }
        };
        if token().is_ok() {
            self.ask_teams();
            self.ask_agenda();
            self.fetch_wanted();
        }
        self.refresh();
        id
    }

    fn ask_agenda(&self) {
        spawn(async {
            let payload = serde_json::json!({ "tracker": "linear" }).to_string();
            let keys = io::call("agenda.linked", &payload).await.ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default();
            Done::InAgenda(keys.into_iter().collect())
        });
    }

    fn ask_teams(&self) {
        if self.page.as_ref().is_some_and(|(_, p)| p.teams.is_some()) {
            return;
        }
        match linear() {
            Ok(l) => spawn(async move { Done::Teams(l.teams().await.map_err(told)) }),
            Err(why) => complain(&why),
        }
    }

    fn fetch_wanted(&mut self) {
        let want = self.page.as_mut().and_then(|(_, p)| p.wants_fetch());
        if let Some(view) = want {
            self.fetch(view, None);
        }
    }

    fn fetch(&mut self, view: ViewId, after: Option<String>) {
        let l = match linear() {
            Ok(l) => l,
            Err(why) => {
                if let Some((_, p)) = &mut self.page {
                    p.list_loaded(&view, None, Err(why));
                }
                return;
            }
        };
        let as_view = match &view {
            ViewId::Mine => View::Assigned,
            ViewId::Created => View::Created,
            ViewId::Cycle(t) => View::Cycle { team: t.clone() },
            ViewId::Backlog(t) => View::Backlog { team: t.clone() },
            ViewId::Typed(text) => match filter::parse(text) {
                Ok(f) => View::Filter(f),
                Err(why) => {
                    if let Some((_, p)) = &mut self.page {
                        p.list_loaded(&view, None, Err(why));
                    }
                    return;
                }
            },
        };
        spawn(async move {
            let result = l.list(&as_view, 50, after.as_deref()).await.map_err(told);
            Done::List { view, after, result }
        });
    }

    fn fetch_detail(&mut self, key: String) {
        let l = match linear() {
            Ok(l) => l,
            Err(why) => return complain(&why),
        };
        spawn(async move {
            let result = match refs::parse(&key) {
                Some(n) => l.issue(&n).await.map(Box::new).map_err(told),
                None => Err(format!("{key} isn't an issue")),
            };
            Done::Detail { key, result }
        });
    }

    /// A write to `key`: the one place the page changes Linear. Said when
    /// done; the page brought up to date. Not retried.
    fn write<F, Fut>(&mut self, key: String, job: F)
    where
        F: FnOnce(Linear) -> Fut + 'static,
        Fut: Future<Output = Result<String, SeamError>> + 'static,
    {
        let l = match linear() {
            Ok(l) => l,
            Err(why) => return complain(&why),
        };
        spawn(async move { Done::Wrote { key, result: job(l).await.map_err(told) } });
    }

    fn url_of(&self, key: &str) -> String {
        self.page.as_ref().and_then(|(_, p)| p.known(key)).and_then(|d| d.issue.url.clone()).unwrap_or_else(|| format!("https://linear.app/issue/{key}"))
    }

    fn action(&mut self, action: A) {
        match action {
            A::None => {}
            A::Close => {
                if let Some((id, _)) = self.page.take() {
                    api::page::close(id);
                }
            }
            A::Refresh => self.fetch_wanted(),
            A::Fetch { view, after } => self.fetch(view, after),
            A::Detail(key) => self.fetch_detail(key),
            A::Teams => self.ask_teams(),
            A::TeamMeta(team) => {
                let l = match linear() {
                    Ok(l) => l,
                    Err(why) => return complain(&why),
                };
                spawn(async move {
                    let result = l.team_meta(&team).await.map_err(told);
                    Done::Meta { team, result }
                });
            }
            A::States(key) => {
                let l = match linear() {
                    Ok(l) => l,
                    Err(why) => return complain(&why),
                };
                spawn(async move {
                    let r = l.states(&key).await.map_err(told);
                    let (items, current, error) = match r {
                        Ok((states, current)) => (states.into_iter().map(|s| (format!("{}  ({})", s.name, s.kind), s.id)).collect(), current, None),
                        Err(e) => (Vec::new(), None, Some(e)),
                    };
                    Done::Menu { purpose: Purpose::State(key.clone()), title: format!("Move {key}"), items, current, key, error }
                });
            }
            A::People(key) => {
                let l = match linear() {
                    Ok(l) => l,
                    Err(why) => return complain(&why),
                };
                spawn(async move {
                    let r = l.people(&key).await.map_err(told);
                    let (items, error): (Vec<(String, String)>, _) = match r {
                        Ok(people) => (std::iter::once(("Unassigned".to_string(), String::new())).chain(people.into_iter().map(|p| (p.name, p.id))).collect(), None),
                        Err(e) => (Vec::new(), Some(e)),
                    };
                    Done::Menu { purpose: Purpose::Assign(key.clone()), title: format!("Assign {key}"), items, current: None, key, error }
                });
            }
            A::SetState { key, id, name } => {
                let k = key.clone();
                self.write(key, move |l| async move { l.set_state(&k, &id).await.map(|_| format!("{k} moved to {name}")) });
            }
            A::SetPriority { key, number, name } => {
                let k = key.clone();
                self.write(key, move |l| async move { l.set_priority(&k, number).await.map(|_| format!("{k} is {name} now")) });
            }
            A::Assign { key, person } => {
                let k = key.clone();
                self.write(key, move |l| async move {
                    let said = match &person {
                        Some(p) => format!("{k} assigned to {}", p.name),
                        None => format!("{k} is unassigned now"),
                    };
                    l.assign(&k, person.as_ref().map(|p| p.id.as_str())).await.map(|_| said)
                });
            }
            A::SetDue { key, due } => {
                let k = key.clone();
                self.write(key, move |l| async move {
                    let said = match &due {
                        Some(d) => format!("{k} due {d}"),
                        None => format!("{k} has no due date now"),
                    };
                    l.change(&k, &fenix_tracker::Change::Due(due)).await.map(|_| said)
                });
            }
            A::SetTitle { key, title } => {
                let k = key.clone();
                self.write(key, move |l| async move { l.change(&k, &fenix_tracker::Change::Title(title)).await.map(|_| format!("{k} renamed")) });
            }
            A::EditTitle { key, current } => {
                let id = api::prompt(&format!("{key} title"), &current);
                self.asked.insert(id, Asked::Title(key));
            }
            A::EditDue { key, current } => {
                let id = api::prompt(&format!("{key} due (YYYY-MM-DD, empty clears)"), &current);
                self.asked.insert(id, Asked::Due(key));
            }
            A::EditDescription(key) => {
                let seed = self.page.as_ref().and_then(|(_, p)| p.known(&key)).map(|d| d.issue.body.clone()).unwrap_or_default();
                let cut = self.page.as_ref().and_then(|(_, p)| p.known(&key)).is_some_and(|d| d.issue.body_cut);
                if cut {
                    return complain(&format!("{key}'s description is longer than this shows, so it isn't edited here: that would cut it in Linear"));
                }
                let id = api::compose(&format!("{key} description"), "save", &seed, true);
                self.asked.insert(id, Asked::Description(key));
            }
            A::Comment(key) => {
                let id = api::compose(&format!("Comment on {key}"), "post", "", false);
                self.asked.insert(id, Asked::Comment(key));
            }
            A::Create(new) => self.create(new),
            A::Branch { key, confirmed } => self.start_branch(key, confirmed),
            A::CopyUrl(key) => {
                let url = self.url_of(&key);
                api::copy(&url);
                say(&format!("Copied {url}"));
            }
            A::CopyIdentifier(key) => {
                api::copy(&key);
                say(&format!("Copied {key}"));
            }
            A::CopyBranch(key) => {
                let known = self.page.as_ref().and_then(|(_, p)| p.known(&key)).map(|d| (d.branch.clone(), d.issue.title.clone()));
                match known.map(|(b, t)| branch::name_for(&b, &branch_pattern(), &key, &t)) {
                    Some(Ok(name)) => {
                        api::copy(&name);
                        say(&format!("Copied {name}"));
                    }
                    Some(Err(why)) => complain(&why),
                    None => complain("open the issue first: its branch name comes with it"),
                }
            }
            A::Browser(key) => {
                let _ = api::open_url(&self.url_of(&key));
            }
            A::AddToAgenda { key, clock } => {
                let payload = serde_json::json!({ "tracker": "linear", "ref": key, "clock": clock }).to_string();
                spawn(async move {
                    if let Err(why) = io::call("agenda.add_issue", &payload).await {
                        complain(&format!("couldn't add it to the agenda: {why}"));
                    }
                    Done::InAgenda(HashSet::new())
                });
                if let Some((_, p)) = &mut self.page {
                    p.in_agenda.insert(key);
                }
            }
            A::OpenTask(key) => {
                let payload = serde_json::json!({ "tracker": "linear", "ref": key }).to_string();
                spawn(async move {
                    let _ = io::call("agenda.open_task", &payload).await;
                    Done::InAgenda(HashSet::new())
                });
            }
        }
    }

    /// A new issue from the form, made once it's sent: the page's own
    /// act, not gated.
    fn create(&mut self, new: NewIssue) {
        let l = match linear() {
            Ok(l) => l,
            Err(why) => return complain(&why),
        };
        say("Creating the issue...");
        spawn(async move { Done::Created { result: l.create(&new).await.map_err(told) } });
    }

    /// The branch for `key` in the project; Linear's own `branchName`
    /// unless `linear.branch_pattern` says otherwise.
    fn start_branch(&mut self, key: String, confirmed: bool) {
        let known = self.page.as_ref().and_then(|(_, p)| p.known(&key)).map(|d| (d.branch.clone(), d.issue.title.clone()));
        let l = match linear() {
            Ok(l) => l,
            Err(why) => return complain(&why),
        };
        let pattern = branch_pattern();
        spawn(async move {
            let (suggested, title) = match known {
                Some(k) => k,
                None => match refs::parse(&key) {
                    Some(n) => match l.issue(&n).await {
                        Ok(d) => (d.branch, d.issue.title),
                        Err(e) => return Done::Branch { key, result: Err(told(e)), name: None },
                    },
                    None => return Done::Branch { key: key.clone(), result: Err(format!("{key} isn't an issue")), name: None },
                },
            };
            let name = branch::name_for(&suggested, &pattern, &key, &title).ok();
            let result = branch::start(git, &suggested, &pattern, &key, &title, confirmed).await;
            Done::Branch { key, result, name }
        });
    }

    fn take_done(&mut self) {
        let done = DONE.with(|d| std::mem::take(&mut *d.borrow_mut()));
        for done in done {
            match done {
                Done::List { view, after, result } => {
                    if let Some((_, p)) = &mut self.page {
                        p.list_loaded(&view, after.as_deref(), result);
                    }
                }
                Done::Teams(result) => {
                    let action = self.page.as_mut().map(|(_, p)| {
                        p.set_teams(result);
                        p.teams_for_form()
                    });
                    if let Some(a) = action {
                        self.action(a);
                    }
                    self.fetch_wanted();
                }
                Done::Detail { key, result } => {
                    if let Some((_, p)) = &mut self.page {
                        p.detail_loaded(&key, result.map(|d| *d));
                    }
                }
                Done::Meta { team, result } => {
                    if let Some((_, p)) = &mut self.page {
                        p.meta_loaded(&team, result);
                    }
                }
                Done::Menu { purpose, title, items, current, key, error } => match error {
                    Some(why) => complain(&format!("{key}: {why}")),
                    None => {
                        if let Some((_, p)) = &mut self.page {
                            p.offer(purpose, &title, items, current.as_deref());
                        }
                    }
                },
                Done::Wrote { key, result } => {
                    match result {
                        Ok(message) => say(&message),
                        Err(err) => complain(&format!("{key}: {err}")),
                    }
                    if let Some((_, p)) = &mut self.page {
                        p.details.remove(&key);
                        p.lists.clear();
                    }
                    self.fetch_wanted();
                    if self.page.as_ref().is_some_and(|(_, p)| p.screen == page::Screen::Issue(key.clone())) {
                        self.fetch_detail(key.clone());
                    }
                    if self.page.as_ref().is_some_and(|(_, p)| p.in_agenda.contains(&key)) {
                        spawn(async {
                            let _ = io::call("agenda.sync_quietly", "null").await;
                            Done::InAgenda(HashSet::new())
                        });
                    }
                }
                Done::Created { result } => match result {
                    Ok(made) => {
                        say(&format!("Created {}", made.identifier));
                        if let Some((_, p)) = &mut self.page {
                            p.form = None;
                            p.lists.clear();
                        }
                        self.fetch_wanted();
                    }
                    // The form stays, to send again.
                    Err(why) => complain(&format!("couldn't create the issue: {why}")),
                },
                Done::Branch { key, result, name } => match result {
                    Ok(Outcome::Switched { name, created }) => say(&if created { format!("Created {name} and switched to it") } else { format!("Switched to {name}") }),
                    Ok(Outcome::Confirm { changed }) => {
                        let items = vec![PickerItem::new(format!("Make the branch here, with the {changed} changed file(s) coming along"), "yes"), PickerItem::new("Leave it", "no")];
                        let id = api::pick(&format!("{key}: the tree has uncommitted changes"), &items);
                        self.asked.insert(id, Asked::Dirty(key));
                    }
                    Ok(Outcome::Said(message)) => {
                        if message.starts_with("this project isn't a git repository") {
                            if let Some(n) = &name {
                                api::copy(n);
                            }
                        }
                        say(&message);
                    }
                    Err(why) => complain(&format!("{key}: {why}")),
                },
                Done::InAgenda(keys) => {
                    if !keys.is_empty() {
                        if let Some((_, p)) = &mut self.page {
                            p.in_agenda = keys;
                        }
                    }
                }
            }
        }
        self.refresh();
    }

    /// The tracker's seam and the tools: answered once Linear has. Every
    /// write that comes through the seam passes the gate first
    /// (`fenix_tracker::gate`: asked once a session, `linear.seam_writes`),
    /// and one already done this session (its `op`) is answered as it was.
    /// The page's own writes (`write`, `create`) don't come this way.
    fn serve_ours(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        use fenix_tracker::gate::{self, Verdict};
        if name == TOOL_ISSUE || name == TOOL_MINE {
            let l = match linear() {
                Ok(l) => l,
                Err(why) => return Some(Err(why)),
            };
            let (name, payload) = (name.to_string(), payload.to_string());
            run::spawn(async move {
                let answer = if name == TOOL_ISSUE { tools::read_issue(&l, &payload).await } else { tools::list_mine(&l, &payload).await };
                api::answer(call, answer.map_err(told));
            });
            return None;
        }
        let Some(seam_call) = fenix_tracker::call::ALL.iter().find(|c| service(c) == name).copied() else {
            return Some(Err(format!("no service {name}")));
        };
        if seam_call == fenix_tracker::call::ABOUT {
            return Some(serde_json::to_string(&seam::about(&seam_settings())).map_err(|e| e.to_string()));
        }
        if !fenix_tracker::call::writes(seam_call) {
            return self.serve_now(call, seam_call, payload);
        }
        let settings = seam_settings();
        if !settings.ready {
            return Some(Err(SeamError::new(code::NOT_SET_UP, seam::not_ready(&settings.keys)).to_err()));
        }
        // A write Linear has no way to do is `unsupported`, nothing asked.
        if ![fenix_tracker::call::TRANSITION, fenix_tracker::call::UPDATE, fenix_tracker::call::COMMENT, fenix_tracker::call::CREATE].contains(&seam_call) {
            return Some(Err(SeamError::new(code::UNSUPPORTED, format!("no service {name}")).to_err()));
        }
        let op = op_of(payload);
        if let Some(answer) = self.done_ops.borrow().get(op.as_deref()) {
            return Some(answer);
        }
        let held = Held { call, seam_call, payload: payload.to_string() };
        match self.gate.check(api::setting("linear.seam_writes").as_deref(), "Linear") {
            Verdict::Go => self.serve_now(call, seam_call, payload),
            Verdict::Refuse(why) => Some(Err(why.to_err())),
            Verdict::Wait => {
                self.held.push(held);
                None
            }
            Verdict::Ask => {
                self.held.push(held);
                let rows = [PickerItem::new(gate::ALLOW, "allow"), PickerItem::new(gate::DENY, "deny")];
                let id = api::pick(&gate::question("Linear"), &rows);
                self.asked.insert(id, Asked::Gate);
                None
            }
        }
    }

    /// You answered the gate: the writes held go through, or are refused.
    fn gate_answered(&mut self, allow: bool) {
        self.gate.answered(allow);
        for Held { call, seam_call, payload } in std::mem::take(&mut self.held) {
            let answer = if allow { self.serve_now(call, seam_call, &payload) } else { Some(Err(fenix_tracker::gate::Gate::denied("Linear").to_err())) };
            if let Some(answer) = answer {
                api::answer(call, answer);
            }
        }
    }

    /// A seam call, served now (past the gate, if it's a write): `Some`
    /// answered at once, `None` later. Never retried here.
    fn serve_now(&mut self, call: u64, seam_call: &'static str, payload: &str) -> Option<Result<String, String>> {
        let settings = seam_settings();
        let op = op_of(payload);
        if seam_call == fenix_tracker::call::CREATE {
            // Linear's form is its own page: opened, filled in, and said at
            // once; the call isn't held while you fill it in.
            let new: fenix_tracker::Create = match serde_json::from_str(payload) {
                Ok(n) => n,
                Err(e) => return Some(Err(SeamError::new(code::BAD_REQUEST, format!("a Linear call: {e}")).to_err())),
            };
            self.open_page();
            let action = self.page.as_mut().map(|(_, p)| p.start_form(&new.title, &new.body));
            if let Some(a) = action {
                self.action(a);
            }
            self.take_done();
            let answer = Ok(serde_json::json!({ "ref": null, "opened": format!("Opened a new-issue form on Linear's page ({}) -- add the issue to the agenda once it's made", page_keys()) }).to_string());
            self.done_ops.borrow_mut().put(op.as_deref(), &answer);
            return Some(answer);
        }
        let l = match linear() {
            Ok(l) => l,
            Err(_) => return Some(Err(SeamError::new(code::NOT_SET_UP, seam::not_ready(&settings.keys)).to_err())),
        };
        let tracker = Seam::new(l, settings);
        let payload = payload.to_string();
        let done_ops = self.done_ops.clone();
        run::spawn(async move {
            let answer = tracker.serve(seam_call, &payload).await;
            if fenix_tracker::call::writes(seam_call) {
                done_ops.borrow_mut().put(op.as_deref(), &answer);
            }
            api::answer(call, answer);
        });
        None
    }
}

/// A write's `op`, if it has one.
fn op_of(payload: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload).ok().and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_string))
}

impl Extension for LinearExt {
    fn key_scheme_changed(&mut self, _scheme: api::KeyScheme) {
        self.refresh();
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "linear.open" => {
                self.open_page();
            }
            "linear.refresh" => {
                self.open_page();
                if let Some((_, p)) = &mut self.page {
                    p.lists.clear();
                }
                self.fetch_wanted();
            }
            "linear.new_issue" => {
                self.open_page();
                let action = self.page.as_mut().map(|(_, p)| p.start_form("", ""));
                if let Some(a) = action {
                    self.action(a);
                }
            }
            "linear.goto_issue" => {
                self.open_page();
                let id = api::prompt("Linear issue (ENG-123, or its address)", "");
                self.asked.insert(id, Asked::Title(String::new()));
            }
            other => return Err(format!("no command {other}")),
        }
        self.refresh();
        self.take_done();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let event = run::deliver(event).and_then(deliver_process);
        self.take_done();
        let Some(event) = event else { return };
        match event {
            Event::SettingChanged(_) => {
                // The agenda asks again: the key, the team or the sync may have changed.
                api::call_service("agenda.tracker_changed", &serde_json::json!({ "tracker": seam::ID }).to_string());
                if let Some((_, p)) = &mut self.page {
                    p.lists.clear();
                    p.teams = None;
                }
                self.ask_teams();
                self.fetch_wanted();
            }
            Event::Prompted((id, Some(text))) => match self.asked.remove(&id) {
                // The go-to prompt shares the asked table with an empty key.
                Some(Asked::Title(key)) if key.is_empty() => match refs::parse(&text) {
                    Some(n) => {
                        let key = n.text();
                        if let Some((_, p)) = &mut self.page {
                            p.screen = page::Screen::Issue(key.clone());
                            p.scroll = 0;
                        }
                        self.fetch_detail(key);
                    }
                    None => complain(&format!("\"{}\" isn't an issue's identifier (ENG-123) or address", text.chars().take(60).collect::<String>())),
                },
                Some(Asked::Title(key)) => {
                    let title = text.trim().to_string();
                    if title.is_empty() {
                        complain("a title can't be empty");
                    } else {
                        self.action(A::SetTitle { key, title });
                    }
                }
                Some(Asked::Due(key)) => match page::parse_due(&text) {
                    Ok(due) => self.action(A::SetDue { key, due }),
                    Err(why) => complain(&why),
                },
                _ => {}
            },
            Event::Composed((id, Some(body))) => match self.asked.remove(&id) {
                Some(Asked::Description(key)) => {
                    let k = key.clone();
                    self.write(key, move |l| async move { l.change(&k, &fenix_tracker::Change::Body(body)).await.map(|_| format!("Description updated for {k}")) });
                }
                Some(Asked::Comment(key)) => {
                    let k = key.clone();
                    self.write(key, move |l| async move { l.comment(&k, &body).await.map(|_| format!("Comment added to {k}")) });
                }
                _ => {}
            },
            Event::Picked((id, choice)) => {
                match self.asked.remove(&id) {
                    Some(Asked::Dirty(key)) if choice.as_deref() == Some("yes") => self.start_branch(key, true),
                    Some(Asked::Gate) => self.gate_answered(choice.as_deref() == Some("allow")),
                    _ => {}
                }
            }
            Event::Prompted((id, None)) | Event::Composed((id, None)) => {
                self.asked.remove(&id);
            }
            _ => {}
        }
        self.take_done();
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let ctx = self.ctx();
        match &self.page {
            Some((id, p)) if *id == page => p.layout(&ctx, width as usize, height as usize).to_page(),
            _ => Page::new(),
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let ctx = self.ctx();
        let Some((id, mut p)) = self.page.take() else { return };
        if id != page {
            self.page = Some((id, p));
            return;
        }
        let action = p.key(key.into(), &ctx);
        self.page = Some((id, p));
        self.action(action);
        self.refresh();
        self.take_done();
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some((id, p)) = &mut self.page {
            if *id == page {
                p.paste(text);
                api::page::refresh(page);
            }
        }
    }

    fn page_closed(&mut self, page: u64) {
        if self.page.as_ref().is_some_and(|(id, _)| *id == page) {
            self.page = None;
        }
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        self.page = Some((page, LinearPage::new(teams_setting())));
        if token().is_ok() {
            self.ask_teams();
            self.ask_agenda();
            self.fetch_wanted();
        }
        true
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        self.serve_ours(call, name, payload)
    }
}

api::export!(LinearExt);

#[cfg(test)]
mod tests {
    use super::*;
    use api::HttpResponse;
    use fenix_test_kit::{Fake, Harness, Ran};
    use fenix_tracker::{About, Found, Page as SeamPage};
    use serde_json::{json, Value};

    const KEY: &str = "lin_api_NjQ1MzIxOTg3NjU0OlRPS0VO_secret";

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/linear/tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// One answer for every request (the fake tells requests by their URL,
    /// and Linear has one): the fixtures' `data` merged, each operation
    /// reading its own key of it.
    fn workspace_answer() -> HttpResponse {
        let mut data = serde_json::Map::new();
        for f in ["viewer.json", "assigned-0.json", "teams.json", "issue-ENG-12.json", "team-meta.json", "people-ENG-12.json", "states-ENG-12.json", "update-issue.json", "comment-created.json", "issue-created.json"] {
            for (k, v) in fixture(f)["data"].as_object().unwrap() {
                match (data.get_mut(k), v) {
                    // `viewer` is in two: who you are, and your issues.
                    (Some(Value::Object(have)), Value::Object(more)) => have.extend(more.clone()),
                    (Some(_), _) => {}
                    (None, v) => {
                        data.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        HttpResponse::new(200, json!({ "data": data }).to_string())
    }

    fn linear_with(key: bool) -> Harness<LinearExt> {
        let fake = Fake::new();
        fake.setting("linear.sync_minutes", "5").setting("linear.branch_pattern", "");
        fake.project("proj", "/project");
        if key {
            fake.secret(TOKEN, KEY);
        }
        fake.on_http(ENDPOINT, workspace_answer());
        Harness::with_fake(fake)
    }

    /// A service call and its answer, whether it came at once or later.
    fn ask(h: &mut Harness<LinearExt>, name: &str, payload: Value) -> Result<String, String> {
        let mut payload = payload;
        payload["seam"] = json!(1);
        let before = h.fake.state().answers.len();
        match h.serve(name, &payload.to_string()) {
            Some(answer) => answer,
            None => {
                h.settle();
                let state = h.fake.state();
                assert!(state.answers.len() > before, "no answer to {name}");
                state.answers.iter().last().map(|(_, a)| a.clone()).unwrap()
            }
        }
    }

    /// Nothing the extension said, asked, logged or answered has the key.
    fn no_key_anywhere(h: &Harness<LinearExt>) {
        let fake = &h.fake;
        let mut seen: Vec<String> = fake.calls();
        seen.extend(fake.notifications());
        seen.extend(fake.toasts().iter().flat_map(|t| [t.summary.clone(), t.body.clone()]));
        seen.extend(fake.services().iter().map(|s| format!("{} {}", s.name, s.payload)));
        seen.extend(fake.state().answers.values().map(|a| format!("{a:?}")));
        seen.extend(fake.state().copied.clone());
        for s in &seen {
            assert!(!s.contains(KEY), "the key is in {s:?}");
        }
        // And in no request but the one header it belongs in.
        for (_, r) in fake.requests() {
            assert!(!String::from_utf8_lossy(&r.body).contains(KEY), "the key is in a request body");
            assert!(!r.url.contains(KEY));
            for (n, v) in &r.headers {
                assert_eq!(v.contains(KEY), n.eq_ignore_ascii_case("authorization"), "{n}");
            }
        }
    }

    #[test]
    fn it_serves_what_its_manifest_says_and_names_every_service_in_one_place() {
        let manifest = include_str!("../extension.toml");
        for c in fenix_tracker::call::ALL {
            let name = service(c);
            let served = ["about", "assigned", "issues", "transitions", "transition", "update", "priorities", "people", "comment", "create", "project"].contains(&c);
            assert!(name.starts_with("fenix.linear.tracker."), "{name}");
            assert_eq!(manifest.contains(&format!("\"{name}\"")), served, "{name}");
        }
        for tool in [TOOL_ISSUE, TOOL_MINE] {
            assert!(manifest.contains(&format!("\"{tool}\"")));
        }
        // Every tool is read-only.
        assert_eq!(manifest.matches("class = \"read\"").count(), manifest.matches("[[contributes.agent_tools]]").count());
        assert!(!manifest.contains("writes = true") && !manifest.contains("class = \"external\""));
        assert!(manifest.contains("network = [\"api.linear.app\"]") && manifest.contains("secrets = [\"linear.token\"]"));
        let mut h = linear_with(true);
        // A call it doesn't serve is `unsupported`; a name that isn't a call, no service.
        let err = SeamError::from_err(&ask(&mut h, &service("worklog"), json!({})).unwrap_err());
        assert_eq!(err.code, "unsupported");
        assert_eq!(h.serve("linear.tracker.nonsense", "{}"), Some(Err("no service linear.tracker.nonsense".into())));
    }

    #[test]
    fn about_says_ready_with_a_key_and_what_to_do_without() {
        let mut h = linear_with(true);
        let about: About = serde_json::from_str(&ask(&mut h, &service("about"), json!({})).unwrap()).unwrap();
        assert_eq!((about.name.as_str(), about.ready, about.sync_minutes), ("Linear", true, 5));
        let mut h = linear_with(false);
        let about: About = serde_json::from_str(&ask(&mut h, &service("about"), json!({})).unwrap()).unwrap();
        assert_eq!((about.ready, about.not_ready.as_deref()), (false, Some("Linear isn't set up yet -- add a personal API key in SPC , (Linear)")));
        let err = SeamError::from_err(&ask(&mut h, &service("assigned"), json!({})).unwrap_err());
        assert_eq!(err.code, "not_set_up");
        assert!(h.fake.requests().is_empty(), "no request without a key");
    }

    #[test]
    fn the_key_goes_in_the_authorization_header_and_nowhere_else() {
        let mut h = linear_with(true);
        let page: SeamPage = serde_json::from_str(&ask(&mut h, &service("assigned"), json!({ "limit": 3 })).unwrap()).unwrap();
        assert_eq!(page.issues[0].key, "ENG-12");
        let reqs = h.fake.requests();
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0].1;
        assert_eq!((r.method.as_str(), r.url.as_str()), ("POST", "https://api.linear.app/graphql"));
        let header = |n: &str| r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.clone());
        assert_eq!(header("Authorization").as_deref(), Some(KEY), "the key as it is: no Bearer");
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(body["variables"]["first"], 3);
        // Other calls, a failure that echoes the key, a refused one.
        let _ = ask(&mut h, &service("issues"), json!({ "refs": ["ENG-12"] })).unwrap();
        h.fake.on_http_error(ENDPOINT, &format!("tls handshake failed (sent {KEY})"));
        let err = SeamError::from_err(&ask(&mut h, &service("assigned"), json!({})).unwrap_err());
        assert_eq!(err.code, "offline");
        assert!(err.message.contains("***") && !err.message.contains(KEY), "{}", err.message);
        h.fake.on_http(ENDPOINT, HttpResponse::new(200, json!({ "errors": [{ "message": "Authentication required", "extensions": { "code": "AUTHENTICATION_ERROR" } }] }).to_string()));
        let err = SeamError::from_err(&ask(&mut h, &service("assigned"), json!({})).unwrap_err());
        assert_eq!(err.code, "signed_out");
        assert!(err.message.contains("settings page (SPC ,)"));
        no_key_anywhere(&h);
    }

    #[test]
    fn a_key_that_cannot_be_a_header_is_refused_not_sent() {
        let fake = Fake::new();
        fake.secret(TOKEN, "abc\r\nX-Evil: 1");
        fake.on_http(ENDPOINT, workspace_answer());
        let mut h: Harness<LinearExt> = Harness::with_fake(fake);
        let err = ask(&mut h, &service("assigned"), json!({})).unwrap_err();
        assert!(SeamError::from_err(&err).code == "not_set_up" || err.contains("control character") || err.contains("set up"), "{err}");
        assert!(h.fake.requests().is_empty());
    }

    #[test]
    fn the_agents_tools_read_and_say_the_text_is_data() {
        let mut h = linear_with(true);
        let issue: Value = serde_json::from_str(&ask(&mut h, TOOL_ISSUE, json!({ "ref": "ENG-12" })).unwrap()).unwrap();
        assert_eq!(issue["issue"]["ref"], "ENG-12");
        assert_eq!(issue["note"], tools::NOTE);
        let mine: Value = serde_json::from_str(&ask(&mut h, TOOL_MINE, json!({ "limit": 2 })).unwrap()).unwrap();
        assert_eq!(mine["issues"].as_array().unwrap().len(), 2);
        // Only queries were sent: nothing that writes.
        for (_, r) in h.fake.requests() {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            assert!(body["query"].as_str().unwrap().starts_with("query "), "{}", body["query"]);
        }
        let err = ask(&mut h, TOOL_ISSUE, json!({ "ref": "nonsense" })).unwrap_err();
        assert!(err.contains("bad_request") || err.contains("isn't an issue"), "{err}");
        no_key_anywhere(&h);
    }

    #[test]
    fn the_agenda_can_be_served_found_issues() {
        let mut h = linear_with(true);
        let found: Found = serde_json::from_str(&ask(&mut h, &service("issues"), json!({ "refs": ["ENG-12"] })).unwrap()).unwrap();
        assert_eq!(found.issues[0].title, "Fix the login timeout");
        assert!(found.issues[0].labels.contains(&"bug".to_string()));
    }

    #[test]
    fn the_page_lists_opens_and_writes_only_when_asked() {
        let mut h = linear_with(true);
        h.command("linear.open").unwrap();
        let page = h.page(KIND);
        let screen = h.screen(page);
        for want in ["My issues", "Fix the login timeout", "Write the session docs", "ENG  ", "Active cycle"] {
            assert!(screen.contains(want), "{want:?} in\n{screen}");
        }
        let writes = |h: &Harness<LinearExt>| h.fake.requests().iter().filter(|(_, r)| String::from_utf8_lossy(&r.body).contains("mutation")).count();
        assert_eq!(writes(&h), 0, "opening and reading write nothing");
        h.keys(page, "l");
        h.keys(page, "l");
        h.keys(page, "G");
        let screen = h.screen(page);
        assert!(screen.contains("Comments (3)") && screen.contains("Reproduced on staging."), "{screen}");
        h.keys(page, "g");
        // A priority is picked from a menu, then sent: one write.
        h.keys(page, "p");
        assert!(h.screen(page).contains("No priority"));
        assert_eq!(writes(&h), 0, "a menu opened is nothing sent");
        h.keys(page, "1");
        assert_eq!(writes(&h), 1);
        assert!(h.fake.notifications().iter().any(|n| n.contains("ENG-12 is Urgent now")), "{:?}", h.fake.notifications());
        // A comment is composed, and sent.
        h.fake.on_prompt("Comment on ENG-12", "Looks **good**");
        h.keys(page, "C");
        assert_eq!(writes(&h), 2);
        assert!(h.fake.notifications().iter().any(|n| n == "Comment added to ENG-12"));
        // Copying and the browser are not writes.
        h.keys(page, "yYBo");
        assert_eq!(h.fake.state().urls.last().map(String::as_str), Some("https://linear.app/acme/issue/ENG-12/fix-the-login-timeout"));
        assert_eq!(h.fake.copied().as_deref(), Some("jo/eng-12-fix-the-login-timeout"));
        assert_eq!(writes(&h), 2);
        no_key_anywhere(&h);
    }

    #[test]
    fn without_a_key_the_page_says_so_and_asks_nothing() {
        let mut h = linear_with(false);
        h.command("linear.open").unwrap();
        let screen = h.screen(h.page(KIND));
        assert!(screen.contains("personal API key") && screen.contains("linear.token"), "{screen}");
        assert!(h.fake.requests().is_empty());
    }

    #[test]
    fn create_opens_the_form_filled_in_and_answers_at_once() {
        let mut h = linear_with(true);
        h.fake.setting("linear.seam_writes", "always");
        h.command("linear.open").unwrap();
        let answer: Value = serde_json::from_str(&ask(&mut h, &service("create"), json!({ "title": "From a task", "body": "its notes", "op": "agenda:1:1" })).unwrap()).unwrap();
        assert_eq!(answer["ref"], Value::Null);
        assert!(answer["opened"].as_str().unwrap().contains("add the issue to the agenda"), "{answer}");
        let page = h.page(KIND);
        assert!(h.screen(page).contains("From a task"), "{}", h.screen(page));
        // Sent from the page with S: that's your own act, and writes.
        h.keys(page, "S");
        assert!(h.fake.notifications().iter().any(|n| n == "Created ENG-21"), "{:?}", h.fake.notifications());
    }

    #[test]
    fn project_is_the_team_a_project_sets() {
        let mut h = linear_with(true);
        let none: fenix_tracker::Project = serde_json::from_str(&ask(&mut h, &service("project"), json!({})).unwrap()).unwrap();
        assert_eq!(none.project, None);
        h.fake.setting("linear.team", " ENG ");
        let p: fenix_tracker::Project = serde_json::from_str(&ask(&mut h, &service("project"), json!({})).unwrap()).unwrap();
        assert_eq!(p.project.as_deref(), Some("ENG"));
        let about: About = serde_json::from_str(&ask(&mut h, &service("about"), json!({ "seam": 7 })).unwrap_or_default()).unwrap();
        assert!(about.can("project") && !about.blocked && about.id == "fenix.linear", "about answers whatever seam it's asked with");
    }

    fn comment(h: &mut Harness<LinearExt>, op: &str) -> Option<Result<String, String>> {
        let call = Fake::with(|s| s.id());
        let payload = json!({ "seam": 1, "ref": "ENG-12", "body": "hi", "op": op }).to_string();
        let first = h.ext.serve(call, &service("comment"), &payload);
        h.settle();
        first.or_else(|| h.fake.state().answers.get(&call).cloned())
    }

    fn mutations(h: &Harness<LinearExt>) -> usize {
        h.fake.requests().iter().filter(|(_, r)| String::from_utf8_lossy(&r.body).contains("mutation")).count()
    }

    #[test]
    fn the_gate_asks_once_a_session_and_holds_writes_until_answered() {
        let mut h = linear_with(true);
        h.fake.on_pick("Agenda wants to change issues in Linear", fenix_tracker::gate::ALLOW);
        assert!(comment(&mut h, "op-1").unwrap().is_ok());
        assert_eq!(h.fake.pickers().len(), 1, "asked");
        assert_eq!(mutations(&h), 1);
        // Allowed for the session: the next is not asked.
        assert!(comment(&mut h, "op-2").unwrap().is_ok());
        assert_eq!((h.fake.pickers().len(), mutations(&h)), (1, 2));
        // The same op again is answered as it was, and not done twice.
        assert!(comment(&mut h, "op-2").unwrap().is_ok());
        assert_eq!(mutations(&h), 2, "a comment is the write that isn't idempotent: sent once");
        no_key_anywhere(&h);
    }

    #[test]
    fn not_now_refuses_and_asks_again_next_time() {
        let mut h = linear_with(true);
        h.fake.on_pick("Agenda wants to change issues in Linear", fenix_tracker::gate::DENY);
        let err = SeamError::from_err(&comment(&mut h, "op-1").unwrap().unwrap_err());
        assert_eq!(err.code, "refused");
        assert!(err.message.contains("you didn't allow the agenda"));
        assert_eq!(mutations(&h), 0, "nothing was written");
        let _ = comment(&mut h, "op-2");
        assert_eq!(h.fake.pickers().len(), 2, "asked again");
    }

    #[test]
    fn never_refuses_always_does_and_neither_asks() {
        let mut h = linear_with(true);
        h.fake.setting("linear.seam_writes", "never");
        let err = SeamError::from_err(&comment(&mut h, "op-1").unwrap().unwrap_err());
        assert_eq!(err.code, "refused");
        assert!(err.message.contains("turned off"));
        for call in ["transition", "update", "create"] {
            let call_id = Fake::with(|s| s.id());
            let payload = json!({ "seam": 1, "ref": "ENG-12", "id": "s", "set": { "title": "x" }, "title": "x", "op": call }).to_string();
            let got = h.ext.serve(call_id, &service(call), &payload);
            assert_eq!(SeamError::from_err(&got.unwrap().unwrap_err()).code, "refused", "{call} is gated too");
        }
        assert!(h.fake.pickers().is_empty() && mutations(&h) == 0);
        h.fake.setting("linear.seam_writes", "always");
        assert!(comment(&mut h, "op-2").unwrap().is_ok());
        assert!(h.fake.pickers().is_empty(), "always doesn't ask");
        assert_eq!(mutations(&h), 1);
    }

    #[test]
    fn the_pages_own_writes_are_not_gated() {
        let mut h = linear_with(true);
        h.command("linear.open").unwrap();
        let page = h.page(KIND);
        h.keys(page, "ll");
        h.keys(page, "p");
        h.keys(page, "1");
        assert_eq!(mutations(&h), 1, "your own pick wrote at once");
        assert!(h.fake.pickers().iter().all(|p| !p.title.contains("Agenda wants")), "no gate question");
    }

    #[test]
    fn a_write_that_isnt_linears_to_do_is_unsupported_and_unasked() {
        let mut h = linear_with(true);
        for call in ["worklog", "remember"] {
            let id = Fake::with(|s| s.id());
            let got = h.ext.serve(id, &service(call), &json!({ "seam": 1, "ref": "ENG-12", "seconds": 1, "started": "x", "remember": "x" }).to_string());
            assert_eq!(SeamError::from_err(&got.unwrap().unwrap_err()).code, "unsupported", "{call}");
        }
        assert!(h.fake.pickers().is_empty());
    }

    #[test]
    fn changing_a_setting_tells_the_agenda_to_ask_again() {
        let mut h = linear_with(true);
        h.set_setting("linear.team", Some("OPS"));
        let told = h.fake.services().into_iter().find(|c| c.name == "agenda.tracker_changed").expect("told");
        assert_eq!(serde_json::from_str::<Value>(&told.payload).unwrap()["tracker"], "fenix.linear");
    }

    fn branch_world(repo: bool, dirty: &str) -> Harness<LinearExt> {
        let h = linear_with(true);
        let f = &h.fake;
        f.on_spawn("git rev-parse --show-toplevel", if repo { Ran::ok("/project\n") } else { Ran::fails(128, "fatal: not a git repository") });
        f.on_spawn("git check-ref-format", Ran::ok(""));
        f.on_spawn("git status --porcelain", Ran::ok(dirty));
        f.on_spawn("git symbolic-ref", Ran::ok("main\n"));
        f.on_spawn("git show-ref", Ran::fails(1, ""));
        f.on_spawn("git switch", Ran::ok(""));
        h
    }

    fn page_on_issue(h: &mut Harness<LinearExt>) -> u64 {
        h.command("linear.open").unwrap();
        let page = h.page(KIND);
        h.keys(page, "ll");
        page
    }

    #[test]
    fn a_branch_is_made_from_linears_name_in_a_clean_repository() {
        let mut h = branch_world(true, "");
        let page = page_on_issue(&mut h);
        h.keys(page, "b");
        let line = h.fake.spawned().into_iter().map(|(_, l)| l).collect::<Vec<_>>();
        assert!(line.iter().any(|l| l == "git switch -c jo/eng-12-fix-the-login-timeout"), "{line:?}");
        assert!(h.fake.notifications().iter().any(|n| n == "Created jo/eng-12-fix-the-login-timeout and switched to it"), "{:?}", h.fake.notifications());
    }

    #[test]
    fn a_dirty_tree_is_asked_about_and_only_a_yes_goes_on() {
        let mut h = branch_world(true, " M a.txt\n?? b.txt\n");
        let page = page_on_issue(&mut h);
        h.fake.on_pick_nothing("ENG-12: the tree has uncommitted changes");
        h.keys(page, "b");
        assert!(!h.fake.spawned().iter().any(|(_, l)| l.starts_with("git switch")), "nothing touched without a yes");
        assert_eq!(h.fake.pickers().len(), 1);
        assert!(h.fake.pickers()[0].items[0].label.contains("2 changed file"));
        let mut h = branch_world(true, " M a.txt\n");
        let page = page_on_issue(&mut h);
        h.fake.on_pick("ENG-12: the tree has uncommitted changes", "Make the branch here, with the 1 changed file(s) coming along");
        h.keys(page, "b");
        assert!(h.fake.spawned().iter().any(|(_, l)| l == "git switch -c jo/eng-12-fix-the-login-timeout"));
    }

    #[test]
    fn outside_a_repository_it_says_so_and_copies_the_name() {
        let mut h = branch_world(false, "");
        let page = page_on_issue(&mut h);
        h.keys(page, "b");
        assert!(h.fake.notifications().iter().any(|n| n.contains("isn't a git repository")), "{:?}", h.fake.notifications());
        assert_eq!(h.fake.copied().as_deref(), Some("jo/eng-12-fix-the-login-timeout"));
        assert!(!h.fake.spawned().iter().any(|(_, l)| l.starts_with("git switch")));
    }

    #[test]
    fn its_own_pattern_names_the_branch_instead() {
        let mut h = branch_world(true, "");
        h.fake.setting("linear.branch_pattern", "feature/{id}-{slug}");
        let page = page_on_issue(&mut h);
        h.keys(page, "b");
        assert!(h.fake.spawned().iter().any(|(_, l)| l == "git switch -c feature/eng-12-fix-the-login-timeout"), "{:?}", h.fake.spawned());
    }
}
