//! GitLab Issues in Fenix: its page (`SPC g I`) -- your searches, an
//! issue with its comments, a new-issue form, a branch started from an
//! issue -- the tracker seam the agenda calls (`gitlab-issues.tracker.*`),
//! and two read-only agent tools. On gitlab.com or your own instance,
//! nested groups and all.
//!
//! The forge's API is `fenix-forge`'s (`issues`, and the seam over it,
//! `tracker`); the page and the rules for a branch are `fenix-issues`';
//! this is the glue to Fenix. Everything that waits -- a request, `git`
//! -- is a job (`jobs`), and what it found is applied to the extension
//! when it's done ([`done`]).
//!
//! **Sign-in.** The server is Fenix's own `gitlab.base_url` and the token
//! Fenix's own `gitlab.token` (Settings, Forges): the two Review reads,
//! read where they live each time they're needed, never copied, the
//! token never sent anywhere but the server's API, never in a seam
//! answer, a notification, a log line or an agent's answer.

mod jobs;
mod keys;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{export, page as pages, Command, Event, Extension, HttpRequest, Key, Page as ApiPage, PickerItem};
use fenix_forge::issues::gitlab::GitLabIssues;
use fenix_forge::issues::{Change, Host, Issue, IssueRef, Issues, NewIssue, Paging, Search};
use fenix_forge::tracker::{self as forge_seam, Learned, Seam};
use fenix_forge::{Http, Response};
use fenix_issues::branch;
use fenix_issues::page::{Action, Detail, Field, Page};
use fenix_tracker::{self as seam, SeamError};
use fenix_ui_kit::Role;

/// The extension's full id: the tracker seam's services start with it
/// (`fenix_tracker::service`).
pub const ID: &str = "fenix.gitlab-issues";
/// The setting that says whether the seam may write (`fenix_tracker::gate`).
const SEAM_WRITES: &str = "gitlab-issues.seam_writes";
const HOST: Host = Host::GitLab;
const KIND: &str = "issues";
/// Fenix's own: the server and the token Review reads.
const TOKEN: &str = "gitlab.token";
const SERVER: &str = "gitlab.base_url";

/// Its settings.
mod setting {
    pub const REPOSITORIES: &str = "gitlab-issues.projects";
    pub const QUERIES: &str = "gitlab-issues.queries";
    pub const BRANCH: &str = "gitlab-issues.branch_pattern";
    pub const WORKFLOW: &str = "gitlab-issues.workflow_scope";
    pub const TIME: &str = "gitlab-issues.time_tracking";
    pub const SYNC: &str = "gitlab-issues.sync_minutes";
}

/// Its agent tools' services.
const READ_ISSUE: &str = "gitlab-issues.read_issue";
const MY_ISSUES: &str = "gitlab-issues.my_issues";

/// The project, as the extension is given it: where `git` runs.
const PROJECT: &str = "/project";

fn setting(key: &str) -> Option<String> {
    api::setting(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// A list setting's items, commas or lines between.
fn list(key: &str) -> Vec<String> {
    setting(key).unwrap_or_default().split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// A map setting's pairs (JSON, `[["key", "value"], ...]`).
fn map(key: &str) -> Vec<(String, String)> {
    setting(key).and_then(|v| serde_json::from_str::<Vec<(String, String)>>(&v).ok()).unwrap_or_default()
}

/// Your GitLab's address, as Fenix's Forges settings hold it.
fn server() -> Option<String> {
    setting(SERVER).filter(|s| s.starts_with("https://") || s.starts_with("http://"))
}

/// What to do to sign in, with the keys of the scheme in use.
fn not_ready() -> String {
    format!("Set your GitLab server and token in {} (Forges) -- the ones Review uses; a personal access token with the api scope", keys::settings())
}

// -- The token, and a client ------------------------------------------------

/// Fenix's `gitlab.token`.
async fn token() -> Option<String> {
    api::secret(TOKEN).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Signed in: Fenix's server and token are both set. `about` says this.
fn ready_now() -> bool {
    server().is_some() && api::secret(TOKEN).is_some_and(|t| !t.trim().is_empty())
}

/// Fenix's HTTP client, for the forge's.
pub struct Fetch;

impl Http for Fetch {
    async fn request(&self, method: &str, url: &str, headers: &[(&str, String)], body: Option<String>) -> Result<Response, String> {
        // A write never follows a redirect: Fenix would follow a 301 or
        // 302 of a PUT as a GET with no body, and the change would seem
        // made. The forge's client takes the 3xx as the error it is.
        let options = if method == "GET" { Default::default() } else { api::HttpOptions::new().no_redirects() };
        let req = HttpRequest { method: method.into(), url: url.into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), body: body.map(String::into_bytes).unwrap_or_default(), options };
        let r = jobs::fetch(req).await?;
        Ok(Response { status: r.status, headers: r.headers, body: String::from_utf8_lossy(&r.body).into_owned() })
    }
}

type Client = GitLabIssues<Fetch>;

/// A client signed in, or what to do to sign in.
async fn client() -> Result<Client, String> {
    let base = server().ok_or_else(not_ready)?;
    let token = token().await.ok_or_else(not_ready)?;
    Ok(GitLabIssues::new(Fetch, &base, token))
}

fn tracker_settings(ready: bool) -> forge_seam::Settings {
    forge_seam::Settings {
        ready,
        not_ready: not_ready(),
        sync_minutes: setting(setting::SYNC).and_then(|m| m.parse().ok()).unwrap_or(10),
        workflow_scope: api::setting(setting::WORKFLOW).unwrap_or_else(|| "workflow".into()).trim().to_string(),
        worklog: setting(setting::TIME).is_some_and(|t| t == "true"),
    }
}

/// The seam, signed in or not.
async fn tracker(learned: Rc<RefCell<Learned>>) -> Seam<Client> {
    match client().await {
        Ok(c) => Seam::new(c, tracker_settings(true), learned),
        Err(_) => Seam::new(GitLabIssues::new(Fetch, &server().unwrap_or_default(), ""), tracker_settings(false), learned),
    }
}

fn web() -> String {
    server().unwrap_or_default().trim_end_matches('/').to_string()
}

/// Tells the agenda to ask this tracker again (fire and forget).
fn tracker_changed() {
    api::call_service("agenda.tracker_changed", &serde_json::json!({ "tracker": ID }).to_string());
}

// -- What jobs leave to be done ------------------------------------------

type Then = Box<dyn FnOnce(&mut IssuesExt)>;

thread_local! {
    static DONE: RefCell<Vec<Then>> = const { RefCell::new(Vec::new()) };
}

/// What a job found, applied to the extension once it's back.
fn done(f: impl FnOnce(&mut IssuesExt) + 'static) {
    DONE.with(|d| d.borrow_mut().push(Box::new(f)));
}

/// `git args` in the project: what it printed, or why it failed.
async fn git(args: &[&str]) -> Result<String, String> {
    // Never a prompt, an editor, or a protocol that runs something or
    // reads this machine (`ext::`, `file://`), whatever the project's git
    // config says its remotes are.
    let cmd = Command::new("git")
        .args(args.iter().copied())
        .cwd(PROJECT)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_ALLOW_PROTOCOL", branch::ALLOWED_PROTOCOLS)
        .env("GIT_PROTOCOL_FROM_USER", "0");
    let out = jobs::run(cmd).await?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(out.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed").trim().to_string())
    }
}

/// The project the one you're in is, from its remotes: one on your
/// GitLab's host first, else `origin`'s path (GitLab is often cloned by
/// another name, as Review takes it).
async fn here_repository() -> Option<String> {
    let remotes = branch::remotes(&git(&["remote", "-v"]).await.ok()?);
    let web = web();
    let host = web.split_once("://").map_or(web.as_str(), |(_, h)| h).split(['/', ':']).next().unwrap_or_default().to_ascii_lowercase();
    let parts: Vec<(String, String, String)> = remotes.iter().filter_map(|(n, u)| branch::remote_parts(u).map(|(h, p)| (n.clone(), h, p))).filter(|(_, _, p)| IssueRef::parse(&format!("{p}#1"), HOST, &web).is_some()).collect();
    parts.iter().find(|(_, h, _)| *h == host).or_else(|| parts.iter().find(|(n, _, _)| n == "origin")).map(|(_, _, p)| p.clone())
}

/// What a prompt, a pane or a picker is for.
enum Ask {
    Query { page: u64 },
    Comment { page: u64, key: IssueRef },
    Title { page: u64, key: IssueRef, was: String },
    Body { page: u64, key: IssueRef, was: String },
    Labels { page: u64, key: IssueRef, was: Vec<String> },
    Assignees { page: u64, key: IssueRef, was: Vec<String> },
    Move { page: u64, key: IssueRef },
    Branch { page: u64, plan: branch::Plan },
    Form { page: u64, field: Field },
    Template { page: u64 },
    /// The write gate's question (`fenix_tracker::gate`).
    Gate,
}

/// A seam write held while you're asked whether to allow it.
struct Held {
    call: u64,
    name: String,
    payload: String,
}

/// A write's `op`, when it carries one.
fn op_of(payload: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload).ok().and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_string))
}

#[derive(Default)]
pub struct IssuesExt {
    pages: HashMap<u64, Page>,
    asks: HashMap<u64, Ask>,
    /// The repository the project is, once worked out.
    here: Option<String>,
    learned: Rc<RefCell<Learned>>,
    /// A form opened by the seam's `create`: the page, and the call to
    /// answer once it's sent or closed, with its `op`.
    creating: HashMap<u64, (u64, Option<String>)>,
    /// The gate on the seam's writes, this session's, and the writes
    /// held while it asks.
    gate: fenix_tracker::gate::Gate,
    held: Vec<Held>,
    /// The seam's writes done this session, by `op`.
    done_ops: Rc<RefCell<fenix_tracker::Dedupe>>,
    /// Whether a sign-in found after `about` said not ready was looked for.
    looked: bool,
}

impl IssuesExt {
    fn apply(&mut self) {
        loop {
            let next: Vec<Then> = DONE.with(|d| std::mem::take(&mut *d.borrow_mut()));
            if next.is_empty() {
                return;
            }
            for f in next {
                f(self);
            }
        }
    }

    fn refresh(page: u64) {
        pages::refresh(page);
    }

    fn page_mut(&mut self, page: u64) -> Option<&mut Page> {
        self.pages.get_mut(&page)
    }

    fn say(&mut self, page: u64, text: &str, role: Role) {
        if let Some(p) = self.page_mut(page) {
            p.say(text, role);
            Self::refresh(page);
        }
    }

    /// The page, opened, or the one open brought forward.
    fn open_page(&mut self) -> u64 {
        if let Some(&page) = self.pages.keys().next() {
            pages::focus(page);
            return page;
        }
        let projects = list(setting::REPOSITORIES);
        let p = Page::new(HOST, fenix_issues::page::searches(HOST, &projects, &map(setting::QUERIES)));
        let page = pages::open(KIND, &p.title());
        self.pages.insert(page, p);
        self.start(page);
        page
    }

    /// The page's first search, once it's known whether you're signed in
    /// and which repository you're in.
    fn start(&mut self, page: u64) {
        let ask = self.page_mut(page).map(|p| p.load());
        jobs::spawn(async move {
            let ready = token().await.is_some();
            let here = if ready { here_repository().await } else { None };
            done(move |ext| {
                ext.here = here.clone();
                let Some(p) = ext.page_mut(page) else { return };
                if !ready {
                    p.loading = false;
                    p.not_ready = Some(not_ready());
                    return Self::refresh(page);
                }
                p.not_ready = None;
                // A form opened before the repository was known: it's this one.
                let mut fill = None;
                if let (Some(f), Some(here)) = (&mut p.form, &here) {
                    if f.new.project.is_empty() {
                        f.new.project = here.clone();
                        fill = Some(here.clone());
                    }
                }
                if let Some(here) = here {
                    let at = p.searches.iter().position(|s| matches!(&s.search, Search::Project(r) if r.eq_ignore_ascii_case(&here)));
                    let saved = match at {
                        Some(i) => p.searches.remove(i),
                        None => fenix_issues::page::Saved { label: here.clone(), search: Search::Project(here.clone()) },
                    };
                    p.searches.insert(3.min(p.searches.len()), saved);
                }
                if let Some(Action::Load { search, more }) = ask {
                    ext.load(page, search, more);
                }
                if let Some(project) = fill {
                    ext.templates(page, project);
                }
            });
        });
    }

    fn load(&mut self, page: u64, search: Search, more: Option<String>) {
        Self::refresh(page);
        jobs::spawn(async move {
            let result = match client().await {
                Ok(c) => c.search(&search, &Paging { cursor: more.clone(), per_page: 50, since: None }).await.map(|l| (l.issues, l.next)).map_err(|e| e.message),
                Err(e) => Err(e),
            };
            done(move |ext| {
                if let Some(p) = ext.page_mut(page) {
                    p.listed(&search, more.is_some(), result);
                    Self::refresh(page);
                }
            });
        });
    }

    fn open_issue(&mut self, page: u64, key: IssueRef) {
        self.say(page, &format!("asking for {key}..."), Role::Muted);
        jobs::spawn(async move {
            let result = async {
                let c = client().await?;
                let issue = c.issue(&key).await.map_err(|e| e.message)?;
                let notes = if issue.comments > 0 { c.notes(&key).await.map_err(|e| e.message)? } else { Vec::new() };
                let linked = c.linked(&key).await.unwrap_or_default();
                Ok::<_, String>(Detail { issue, notes, linked, scroll: 0 })
            }
            .await;
            done(move |ext| {
                if let Some(p) = ext.page_mut(page) {
                    p.status = None;
                    p.opened(result);
                    Self::refresh(page);
                }
            });
        });
    }

    /// A change made from the page: sent, said, and the issue read again.
    fn change(&mut self, page: u64, key: IssueRef, change: Change, said: String) {
        self.say(page, "sending...", Role::Muted);
        jobs::spawn(async move {
            let result = match client().await {
                Ok(c) => c.change(&key, &change).await.map_err(|e| e.message),
                Err(e) => Err(e),
            };
            done(move |ext| match result {
                Ok(()) => {
                    ext.say(page, &format!("{key}: {said}"), Role::Good);
                    ext.open_issue(page, key);
                }
                Err(e) => ext.say(page, &format!("{key}: {e}"), Role::Bad),
            });
        });
    }

    fn comment(&mut self, page: u64, key: IssueRef, body: String) {
        self.say(page, "sending...", Role::Muted);
        jobs::spawn(async move {
            let result = match client().await {
                Ok(c) => c.comment(&key, &body).await.map_err(|e| e.message),
                Err(e) => Err(e),
            };
            done(move |ext| match result {
                Ok(()) => {
                    ext.say(page, &format!("{key}: commented"), Role::Good);
                    ext.open_issue(page, key);
                }
                Err(e) => ext.say(page, &format!("{key}: {e}"), Role::Bad),
            });
        });
    }

    /// `s`: the issue's moves, the same the agenda's offered, to pick one.
    fn moves(&mut self, page: u64, issue: Issue) {
        let learned = self.learned.clone();
        jobs::spawn(async move {
            let t = tracker(learned).await;
            let moves = t.moves(&issue.key, None).await;
            done(move |ext| match moves {
                Ok(m) if !m.transitions.is_empty() => {
                    let items: Vec<PickerItem> = m.transitions.iter().filter(|m| !m.local).map(|m| PickerItem::new(m.row(), m.id.clone())).collect();
                    let id = api::pick(&format!("Move {}", issue.key), &items);
                    ext.asks.insert(id, Ask::Move { page, key: issue.key });
                }
                Ok(_) => ext.say(page, "nothing to do next", Role::Muted),
                Err(e) => ext.say(page, &e.message, Role::Bad),
            });
        });
    }

    fn do_move(&mut self, page: u64, key: IssueRef, id: String) {
        let learned = self.learned.clone();
        jobs::spawn(async move {
            let t = tracker(learned).await;
            let what = t.change_for_move(&key, &id);
            done(move |ext| match what {
                Ok(Some((change, said))) => ext.change(page, key, change, said),
                Ok(None) => {}
                Err(e) => ext.say(page, &e.message, Role::Bad),
            });
        });
    }

    /// `b`: whether a branch can be made here, then its name asked.
    fn branch(&mut self, page: u64, issue: Issue) {
        self.say(page, "looking at the repository...", Role::Muted);
        jobs::spawn(async move {
            let mut seen = branch::Seen { remotes: git(&["remote", "-v"]).await.unwrap_or_default(), status: git(&["status", "--porcelain"]).await.unwrap_or_default(), branches: git(&["branch", "--format=%(refname:short)"]).await.unwrap_or_default(), heads: Vec::new() };
            for (name, _) in branch::remotes(&seen.remotes).into_iter().filter(|(n, _)| branch::remote_name_ok(n)) {
                if let Ok(head) = git(&["symbolic-ref", "--quiet", "--end-of-options", &format!("refs/remotes/{name}/HEAD")]).await {
                    seen.heads.push((name, head));
                }
            }
            let pattern = api::setting(setting::BRANCH).unwrap_or_default();
            let plan = branch::plan(HOST, &web(), &issue, &pattern, &seen);
            done(move |ext| match plan {
                Ok(plan) => {
                    if let Some(p) = ext.page_mut(page) {
                        p.status = None;
                    }
                    let label = match &plan.warning {
                        Some(w) => format!("Branch for {} ({w}):", issue.key),
                        None => format!("Branch for {}, from {}:", issue.key, plan.base.as_deref().unwrap_or("what's checked out")),
                    };
                    let id = api::prompt(&label, &plan.name);
                    ext.asks.insert(id, Ask::Branch { page, plan });
                }
                Err(why) => ext.say(page, &why, Role::Warn),
            });
        });
    }

    fn make_branch(&mut self, page: u64, plan: branch::Plan, typed: String) {
        let name = branch::clean_name(&typed);
        if name.is_empty() {
            return self.say(page, "no branch name -- nothing made", Role::Warn);
        }
        let commands = match branch::commands(&plan, &name) {
            Ok(c) => c,
            Err(why) => return self.say(page, &format!("{why} -- nothing made"), Role::Warn),
        };
        jobs::spawn(async move {
            let result = async {
                git(&["check-ref-format", "--branch", &name]).await.map_err(|_| format!("git won't take \"{name}\" as a branch name"))?;
                for args in commands {
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    // A fetch that fails (offline) leaves the branch made
                    // from what's known of the remote.
                    match git(&args).await {
                        Err(_) if args[0] == "fetch" => {}
                        Err(e) => return Err(e),
                        Ok(_) => {}
                    }
                }
                Ok::<_, String>(())
            }
            .await;
            done(move |ext| match result {
                Ok(()) => ext.say(page, &format!("on {name} now"), Role::Good),
                Err(e) => ext.say(page, &e, Role::Bad),
            });
        });
    }

    /// `n`, or the seam's `create`: the form, its repository the one
    /// you're in (else the first you listed), its templates asked for.
    fn new_issue(&mut self, page: u64, title: String, body: String) {
        let project = self.here.clone().or_else(|| list(setting::REPOSITORIES).into_iter().next()).unwrap_or_default();
        if let Some(p) = self.page_mut(page) {
            p.open_form(NewIssue { project: project.clone(), title, body, ..Default::default() });
            p.status = None;
        }
        Self::refresh(page);
        self.templates(page, project);
    }

    fn templates(&mut self, page: u64, project: String) {
        if IssueRef::parse(&format!("{project}#1"), HOST, "").is_none() {
            return;
        }
        jobs::spawn(async move {
            let found = match client().await {
                Ok(c) => c.templates(&project).await.unwrap_or_default(),
                Err(_) => Vec::new(),
            };
            done(move |ext| {
                if let Some(p) = ext.page_mut(page) {
                    p.set_templates(found);
                    Self::refresh(page);
                }
            });
        });
    }

    fn submit(&mut self, page: u64, new: NewIssue) {
        Self::refresh(page);
        jobs::spawn(async move {
            let result = match client().await {
                Ok(c) => c.create(&new).await.map_err(|e| e.message),
                Err(e) => Err(e),
            };
            done(move |ext| {
                if let (Ok(issue), Some((call, op))) = (&result, ext.creating.remove(&page)) {
                    let answer = Ok(serde_json::json!({ "ref": issue.key.key() }).to_string());
                    ext.done_ops.borrow_mut().put(op.as_deref(), &answer);
                    api::answer(call, answer);
                }
                if let Some(p) = ext.page_mut(page) {
                    p.submitted(result);
                    Self::refresh(page);
                }
            });
        });
    }

    /// The seam's `create` answered: nothing made.
    fn create_closed(&mut self, page: u64) {
        if let Some((call, _)) = self.creating.remove(&page) {
            api::answer(call, Ok(r#"{"ref":null}"#.into()));
        }
    }

    fn action(&mut self, page: u64, action: Action) {
        match action {
            Action::None => {}
            Action::Redraw => Self::refresh(page),
            Action::Close => {
                self.create_closed(page);
                self.pages.remove(&page);
                pages::close(page);
            }
            Action::Load { search, more } => {
                if self.pages.get(&page).is_some_and(|p| p.not_ready.is_some()) {
                    return self.start(page);
                }
                self.load(page, search, more)
            }
            Action::Open(key) => self.open_issue(page, key),
            Action::Browser(url) => {
                if let Some(url) = fenix_tracker::clean::url(&url) {
                    let _ = api::open_url(&url);
                }
            }
            Action::Copy(text, what) => {
                api::copy(&text);
                self.say(page, &format!("copied the {what}: {text}"), Role::Muted);
            }
            Action::Comment(issue) => {
                let id = api::compose(&format!("Comment on {}", issue.key), "Comment", "", false);
                self.asks.insert(id, Ask::Comment { page, key: issue.key });
            }
            Action::EditTitle(issue) => {
                let id = api::prompt(&format!("Title of {}:", issue.key), &issue.title);
                self.asks.insert(id, Ask::Title { page, key: issue.key, was: issue.title });
            }
            Action::EditBody(issue) => {
                let id = api::compose(&format!("Description of {}", issue.key), "Save", &issue.body, true);
                self.asks.insert(id, Ask::Body { page, key: issue.key, was: issue.body });
            }
            Action::EditLabels(issue) => {
                let id = api::prompt(&format!("Labels of {} (commas between):", issue.key), &issue.labels.join(", "));
                self.asks.insert(id, Ask::Labels { page, key: issue.key, was: issue.labels });
            }
            Action::EditAssignees(issue) => {
                let was: Vec<String> = issue.assignees.iter().map(|a| a.username.clone()).collect();
                let id = api::prompt(&format!("Assignees of {} (usernames, commas between):", issue.key), &was.join(", "));
                self.asks.insert(id, Ask::Assignees { page, key: issue.key, was });
            }
            Action::Moves(issue) => self.moves(page, issue),
            Action::Branch(issue) => self.branch(page, issue),
            Action::Agenda(issue) => {
                api::call_service("agenda.add_issue", &serde_json::json!({ "tracker": ID, "ref": issue.key.key() }).to_string());
                self.say(page, &format!("{} sent to the agenda", issue.key), Role::Muted);
            }
            Action::Query => {
                let id = api::prompt("GitLab filter (label:bug assignee:@me project:group/project, and words):", "");
                self.asks.insert(id, Ask::Query { page });
            }
            Action::NewIssue => self.new_issue(page, String::new(), String::new()),
            Action::FormEdit(Field::Body, now) => {
                let id = api::compose("The new issue's description", "Done", &now, true);
                self.asks.insert(id, Ask::Form { page, field: Field::Body });
            }
            Action::FormEdit(field, now) => {
                let label = match field {
                    Field::Project => "Project (group/project):",
                    Field::Title => "Title:",
                    Field::Labels => "Labels (commas between):",
                    Field::Assignees => "Assignees (usernames, commas between):",
                    _ => "Value:",
                };
                let id = api::prompt(label, &now);
                self.asks.insert(id, Ask::Form { page, field });
            }
            Action::FormTemplate => {
                let items: Vec<PickerItem> = self.pages.get(&page).and_then(|p| p.form.as_ref()).map(|f| f.templates.iter().enumerate().map(|(i, t)| PickerItem::new(fenix_tracker::clean::name(&t.name), i.to_string())).collect()).unwrap_or_default();
                if items.is_empty() {
                    return self.say(page, "this project has no issue templates", Role::Muted);
                }
                let id = api::pick("Issue template", &items);
                self.asks.insert(id, Ask::Template { page });
            }
            Action::Submit(new) => self.submit(page, new),
            Action::CancelForm => {
                self.create_closed(page);
                Self::refresh(page);
            }
        }
    }

    /// A prompt, pane or picker came back.
    fn answered(&mut self, id: u64, text: Option<String>) {
        let Some(ask) = self.asks.remove(&id) else { return };
        if let Ask::Gate = ask {
            return self.gate_answered(text.as_deref() == Some("allow"));
        }
        let Some(text) = text else { return };
        let split = |t: &str| t.split(',').map(|s| s.trim().trim_start_matches('@').to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        match ask {
            Ask::Query { page } => {
                if let Some(Action::Load { search, more }) = self.page_mut(page).map(|p| p.query(&text)) {
                    self.load(page, search, more);
                }
            }
            Ask::Comment { page, key } if !text.trim().is_empty() => self.comment(page, key, text),
            Ask::Title { page, key, was } if text.trim() != was.trim() && !text.trim().is_empty() => self.change(page, key, Change::Title(text.trim().into()), "title changed".into()),
            Ask::Body { page, key, was } if text != was => self.change(page, key, Change::Body(text), "description changed".into()),
            Ask::Labels { page, key, was } if split(&text) != was => self.change(page, key, Change::Labels(split(&text)), "labels changed".into()),
            Ask::Assignees { page, key, was } if split(&text) != was => self.change(page, key, Change::Assignees(split(&text)), "assignees changed".into()),
            Ask::Move { page, key } => self.do_move(page, key, text),
            Ask::Branch { page, plan } => self.make_branch(page, plan, text),
            Ask::Form { page, field } => {
                if let Some(p) = self.page_mut(page) {
                    p.set_field(field, &text);
                }
                Self::refresh(page);
                if field == Field::Project {
                    self.templates(page, text.trim().to_string());
                }
            }
            Ask::Template { page } => {
                if let (Some(p), Ok(i)) = (self.page_mut(page), text.parse()) {
                    p.use_template(i);
                }
                Self::refresh(page);
            }
            _ => {}
        }
    }

    // -- Services ---------------------------------------------------------

    /// `fenix.gitlab-issues.tracker.<call>`: `about` at once, from what's known
    /// here; `project` from the git remote; reads over the forge; and every
    /// write -- `create` too -- through the gate (`fenix_tracker::gate`)
    /// and the session's `op` memory first. The page's own writes never
    /// come this way.
    fn serve_seam(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        use fenix_tracker::gate::{self, Verdict};
        if name == seam::call::ABOUT {
            let ready = ready_now();
            if !ready {
                self.look_for_sign_in();
            }
            return Some(serde_json::to_string(&forge_seam::about(HOST, &tracker_settings(ready))).map_err(|e| e.to_string()));
        }
        if name == seam::call::PROJECT {
            jobs::spawn(async move {
                let project = here_repository().await;
                api::answer(call, Ok(serde_json::json!({ "project": project }).to_string()));
            });
            return None;
        }
        if !seam::call::writes(name) {
            return self.serve_seam_now(call, name, payload);
        }
        // Not set up: said at once, nothing asked.
        if !ready_now() {
            return Some(Err(SeamError::new(seam::code::NOT_SET_UP, not_ready()).to_err()));
        }
        if let Some(answer) = self.done_ops.borrow().get(op_of(payload).as_deref()) {
            return Some(answer);
        }
        let held = Held { call, name: name.to_string(), payload: payload.to_string() };
        match self.gate.check(api::setting(SEAM_WRITES).as_deref(), HOST.name()) {
            Verdict::Go => self.serve_seam_now(call, name, payload),
            Verdict::Refuse(why) => Some(Err(why.to_err())),
            Verdict::Wait => {
                self.held.push(held);
                None
            }
            Verdict::Ask => {
                self.held.push(held);
                let rows = [PickerItem::new(gate::ALLOW, "allow"), PickerItem::new(gate::DENY, "deny")];
                let id = api::pick(&gate::question(HOST.name()), &rows);
                self.asks.insert(id, Ask::Gate);
                None
            }
        }
    }

    /// You answered the gate: the writes held go through, or are refused.
    fn gate_answered(&mut self, allow: bool) {
        self.gate.answered(allow);
        for Held { call, name, payload } in std::mem::take(&mut self.held) {
            let answer = if allow { self.serve_seam_now(call, &name, &payload) } else { Some(Err(fenix_tracker::gate::Gate::denied(HOST.name()).to_err())) };
            if let Some(answer) = answer {
                api::answer(call, answer);
            }
        }
    }

    /// A seam call, past the gate: answered later, once the forge has.
    fn serve_seam_now(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let op = op_of(payload);
        if name == seam::call::CREATE {
            let req: seam::Create = match serde_json::from_str(payload) {
                Ok(r) => r,
                Err(e) => return Some(Err(SeamError::new(seam::code::BAD_REQUEST, e.to_string()).to_err())),
            };
            let page = self.open_page();
            self.create_closed(page);
            self.new_issue(page, fenix_tracker::clean::line(&req.title, fenix_tracker::clean::TITLE), fenix_tracker::clean::text(&req.body, fenix_tracker::clean::BODY_BYTES).0);
            self.creating.insert(page, (call, op));
            return None;
        }
        let (name, payload, learned, done_ops) = (name.to_string(), payload.to_string(), self.learned.clone(), self.done_ops.clone());
        jobs::spawn(async move {
            let answer = tracker(learned).await.serve(&name, &payload).await;
            if seam::call::writes(&name) {
                done_ops.borrow_mut().put(op.as_deref(), &answer);
            }
            api::answer(call, answer);
        });
        None
    }

    /// The agenda's told when a sign-in it was told isn't there turns
    /// up (the GitHub CLI's, found by asking `gh`), so it asks again.
    fn look_for_sign_in(&mut self) {
        if std::mem::replace(&mut self.looked, true) {
            return;
        }
        jobs::spawn(async move {
            if token().await.is_some() && ready_now() {
                tracker_changed();
            }
        });
    }

    /// The agent tools: an issue with its comments, and yours. They read.
    fn serve_tool(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let args: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
        let read = name == READ_ISSUE;
        let key = args.get("ref").and_then(|r| r.as_str()).unwrap_or_default().to_string();
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(30).clamp(1, fenix_issues::agent::ISSUES as u64) as u32;
        jobs::spawn(async move {
            let answer = async {
                let c = client().await?;
                if read {
                    let key = IssueRef::parse(&key, HOST, c.web()).ok_or_else(|| format!("\"{}\" isn't an issue -- group/project#12, or its address", fenix_tracker::clean::line(&key, 200)))?;
                    let issue = c.issue(&key).await.map_err(|e| e.message)?;
                    let notes = if issue.comments > 0 { c.notes(&key).await.map_err(|e| e.message)? } else { Vec::new() };
                    let linked = c.linked(&key).await.unwrap_or_default();
                    Ok(fenix_issues::agent::issue(&issue, &notes, &linked))
                } else {
                    let listing = c.search(&Search::Assigned, &Paging::first(limit)).await.map_err(|e| e.message)?;
                    Ok(fenix_issues::agent::mine(&listing.issues, listing.next.is_some()))
                }
            }
            .await;
            api::answer(call, answer.map_err(|e: String| fenix_tracker::clean::message(&e)));
        });
        None
    }
}

impl Extension for IssuesExt {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "gitlab-issues.open" => {
                self.open_page();
            }
            "gitlab-issues.new" => {
                let page = self.open_page();
                self.new_issue(page, String::new(), String::new());
            }
            "gitlab-issues.close" => {
                for page in self.pages.keys().copied().collect::<Vec<_>>() {
                    self.action(page, Action::Close);
                }
            }
            _ => return Err(format!("no command {id}")),
        }
        self.apply();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let event = jobs::deliver(event);
        match event {
            Some(Event::Prompted((id, text)) | Event::Composed((id, text)) | Event::Picked((id, text))) => self.answered(id, text),
            Some(Event::SettingChanged(_)) => {
                // What was learned of the old server goes; the agenda asks
                // again.
                *self.learned.borrow_mut() = Learned::default();
                tracker_changed();
                for (&page, p) in &mut self.pages {
                    let current = p.search().cloned();
                    let mut searches = fenix_issues::page::searches(HOST, &list(setting::REPOSITORIES), &map(setting::QUERIES));
                    if let Some(here) = &self.here {
                        searches.insert(3, fenix_issues::page::Saved { label: here.clone(), search: Search::Project(here.clone()) });
                        searches.dedup_by(|a, b| a.search == b.search);
                    }
                    p.searches = searches;
                    p.current = current.and_then(|c| p.searches.iter().position(|s| s.search == c)).unwrap_or(0);
                    Self::refresh(page);
                }
            }
            _ => {}
        }
        self.apply();
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> ApiPage {
        match self.pages.get_mut(&page) {
            Some(p) => p.render(width as usize, height as usize).to_page(),
            None => ApiPage::new().line([pages::muted("Nothing here: the page is closed")]),
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key.into());
        self.action(page, action);
        self.apply();
    }

    fn page_closed(&mut self, page: u64) {
        self.create_closed(page);
        self.pages.remove(&page);
    }

    fn page_state(&mut self, _page: u64) -> Option<String> {
        Some(String::new())
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        let p = Page::new(HOST, fenix_issues::page::searches(HOST, &list(setting::REPOSITORIES), &map(setting::QUERIES)));
        self.pages.insert(page, p);
        self.start(page);
        self.apply();
        true
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = match seam::call_of(ID, name) {
            Some(c) => self.serve_seam(call, c, payload),
            None if name == READ_ISSUE || name == MY_ISSUES => self.serve_tool(call, name, payload),
            None => Some(Err(format!("no service {name}"))),
        };
        self.apply();
        answer
    }
}

export!(IssuesExt);

#[cfg(test)]
mod tests;
