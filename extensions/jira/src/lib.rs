//! Jira, as a WebAssembly extension: the Jira page (`SPC j j`) -- your
//! searches, their issues, a preview, and an issue's own page -- and
//! Jira for the agenda: Server and Data Center, or Jira Cloud, told
//! apart by the site's address and settled by its `serverInfo` on first
//! contact (`fenix_jira_client::cloud`). It holds your site, your token
//! (and, for Cloud, your email) and Jira's settings; the agenda reaches Jira through the tracker seam
//! (`plans/tracker-seam.md`), `jira.tracker.*`, served by
//! `fenix_jira_client::tracker` over its own HTTP.
//!
//! The agenda 1.2, from before the seam, reaches it through these, kept
//! for it until both are 2.0:
//!
//! - `jira.request`: a request, sent as you (`{method, path, body}` ->
//!   `{status, body}`)
//! - `jira.config`: Jira's settings, as the agenda needs them
//! - `jira.set_blocked`: what Blocked means in a project, kept
//! - `jira.new_issue`: the new-issue form, filled from a task, answered
//!   with the key it made

pub mod dates;
pub mod page;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{host, run, Event, Extension, HttpRequest, Key, Page, PickerItem};
use fenix_jira_client::tracker::{self as seam, Learned};
use fenix_jira_client::cloud::{cloud_id, gateway, host_of};
use fenix_jira_client::{Auth, Deployment, IssueDetail, IssueSummary, JiraClient, Reply, Transport};
use fenix_tracker::{code, SeamError};

use page::{Action as A, Choice, JiraPage, Loaded, QueryKind};

/// A service call it answers later: Fenix's number for it.
pub type Waiting = u64;

/// How many issues a search shows.
const SEARCH_LIMIT: u32 = 100;

/// The page's kind, for session restore.
const KIND: &str = "jira";

/// Jira itself: your site (or, for a scoped API token, Atlassian's
/// gateway to it), as you -- or, with no `auth`, as anyone, for what a
/// site says before you sign in (`serverInfo`, `tenant_info`).
struct Direct {
    base: String,
    auth: Option<Auth>,
    /// What the last answer's `Retry-After` said, when it was a 429.
    waited: Rc<Cell<Option<u32>>>,
}

impl Direct {
    fn new(base: &str, auth: Option<Auth>) -> Direct {
        Direct { base: base.to_string(), auth, waited: Rc::default() }
    }
}

/// An HTTP request through Fenix's client.
#[cfg(not(test))]
use run::fetch;

/// In tests, `run::fetch` as the test kit can answer it: `run::fetch`
/// calls Fenix's own host, which isn't there natively, so a request goes
/// to the kit's (`api::host::http`) and its answer is taken from the
/// events before `run::deliver` sees them.
#[cfg(test)]
mod kit_fetch {
    use super::api::{self, run, Event, HttpRequest, HttpResponse};
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static CAME: RefCell<HashMap<u64, Option<Result<HttpResponse, String>>>> = RefCell::new(HashMap::new());
    }

    pub async fn fetch(req: HttpRequest) -> Result<HttpResponse, String> {
        let id = api::host::http(&req)?;
        CAME.with(|c| c.borrow_mut().insert(id, None));
        std::future::poll_fn(|_| match CAME.with(|c| c.borrow_mut().get_mut(&id).and_then(Option::take)) {
            Some(answer) => {
                CAME.with(|c| c.borrow_mut().remove(&id));
                std::task::Poll::Ready(answer)
            }
            None => std::task::Poll::Pending,
        })
        .await
    }

    /// `event`, unless it answers a request: then the jobs run on.
    pub fn deliver(event: Event) -> Option<Event> {
        if let Event::HttpResponse((id, answer)) = &event {
            if CAME.with(|c| c.borrow().contains_key(id)) {
                CAME.with(|c| c.borrow_mut().insert(*id, Some(answer.clone())));
                // Starting a job polls every job waiting.
                run::spawn(async {});
                return None;
            }
        }
        Some(event)
    }
}
#[cfg(test)]
use kit_fetch::fetch;

/// A 429's `Retry-After`, in seconds (an HTTP date isn't read).
fn retry_after(headers: &[(String, String)]) -> Option<u32> {
    headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("retry-after")).and_then(|(_, v)| v.trim().parse().ok())
}

impl Transport for Direct {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
        let mut req = HttpRequest { method: method.into(), url: format!("{}{path}", self.base), headers: Vec::new(), body: body.map(String::into_bytes).unwrap_or_default(), options: Default::default() };
        if let Some(auth) = &self.auth {
            req = req.header("Authorization", &auth.header());
        }
        req = req.header("Accept", "application/json");
        if !req.body.is_empty() {
            req = req.header("Content-Type", "application/json");
        }
        let waited = self.waited.clone();
        Box::pin(async move {
            let r = fetch(req).await.map_err(|e| format!("request failed: {e}"))?;
            waited.set(if r.status == 429 { retry_after(&r.headers) } else { None });
            Ok((r.status, r.text()))
        })
    }

    fn retry_after(&self) -> Option<u32> {
        self.waited.get()
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

/// The keys that open the settings page now, as the key scheme in use
/// writes them (`SPC ,`, `C-,` with standard keys, or yours); its Vim
/// keys when none does, and natively, where there's no Fenix to ask.
fn settings_keys() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        api::key_for("settings.open").unwrap_or_else(|| "SPC ,".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "SPC ,".to_string()
    }
}

/// Why it can't reach Jira yet: no server or token. `keys` open the
/// settings page.
fn not_set_up(keys: &str) -> String {
    format!("Jira isn't set up yet -- set its server and token on the settings page ({keys}), under Jira")
}

/// Why it can't reach Jira Cloud yet: no email for the API token.
fn cloud_not_set_up(keys: &str) -> String {
    format!("Jira Cloud isn't set up yet -- set your Atlassian account's email (Jira email) beside the site and API token on the settings page ({keys}), under Jira")
}

/// The keys that open the Jira page now (`jira.open`).
fn page_keys() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        api::key_for("jira.open").unwrap_or_else(|| "SPC j j".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "SPC j j".to_string()
    }
}

/// Jira's settings, as the tracker seam reads them.
fn tracker_settings() -> seam::Settings {
    seam::Settings {
        base_url: base_url(),
        projects: map("jira.projects").into_iter().map(|(k, _)| k).collect(),
        users: map("jira.users"),
        priorities: map("jira.priorities"),
        blocked: map("jira.blocked"),
        sync_minutes: api::setting("jira.sync_minutes").and_then(|m| m.trim().parse().ok()).unwrap_or(10),
        keys: seam::Keys { settings: settings_keys(), page: page_keys() },
    }
}

/// The agenda's payload for an issue of this tracker's: `tracker` and
/// `ref` for the agenda 1.3, `key` for 1.2.
fn agenda_payload(key: &str, clock: Option<bool>) -> String {
    let mut payload = serde_json::json!({ "tracker": "jira", "ref": key, "key": key });
    if let Some(clock) = clock {
        payload["clock"] = clock.into();
    }
    payload.to_string()
}

/// Where Jira is, when it's set up.
fn base_url() -> Option<String> {
    api::setting("jira.base_url").map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
}

fn token() -> Option<String> {
    host::secret("jira.token").filter(|t| !t.trim().is_empty())
}

/// `jira.email`: your Atlassian account's, for Jira Cloud.
fn email() -> Option<String> {
    api::setting("jira.email").map(|e| e.trim().to_string()).filter(|e| !e.is_empty())
}

/// What it has learned of the site it talks to, forgotten when the
/// address changes: which Jira it is and how it knows, a scoped token's
/// cloud id, and what the seam learned (who you are, the Flagged field).
#[derive(Default)]
struct Site {
    base: String,
    /// What `serverInfo` said (or, when it wouldn't, the address).
    said: Option<(Deployment, String)>,
    cloud_id: Option<String>,
    learned: Rc<RefCell<Learned>>,
}

type Known = Rc<RefCell<Site>>;

/// `site`, for `base`: anew when the address changed.
fn site_for<'a>(site: &'a Known, base: &str) -> std::cell::RefMut<'a, Site> {
    let mut s = site.borrow_mut();
    if s.base != base {
        *s = Site { base: base.to_string(), ..Default::default() };
    }
    s
}

/// Which Jira `base` is, as far as it knows now: as `jira.kind` forces,
/// as the site said, or as its address says.
fn deployment_now(site: &Known, base: &str) -> Deployment {
    Deployment::forced(api::setting("jira.kind").as_deref()).or_else(|| site_for(site, base).said.as_ref().map(|(d, _)| *d)).unwrap_or_else(|| Deployment::guess(base))
}

/// Everything a request needs, read from the settings now; its client
/// once the site has said which Jira it is.
#[derive(Clone)]
struct Connect {
    base: String,
    token: String,
    email: Option<String>,
    forced: Option<Deployment>,
    scoped: bool,
    site: Known,
}

/// A connection, or why there's none.
fn connect(site: &Known) -> Result<Connect, String> {
    let (Some(base), Some(token)) = (base_url(), token()) else { return Err(not_set_up(&settings_keys())) };
    let email = email();
    if deployment_now(site, &base).is_cloud() && email.is_none() {
        return Err(cloud_not_set_up(&settings_keys()));
    }
    let forced = Deployment::forced(api::setting("jira.kind").as_deref());
    let scoped = api::setting("jira.scoped_token").is_some_and(|v| v.trim() == "true");
    Ok(Connect { base, token, email, forced, scoped, site: site.clone() })
}

impl Connect {
    /// Which Jira it is: as forced or known, else asked of the site once
    /// (`serverInfo`, as anyone) -- what it says is kept; when it won't
    /// say, its address decides, and that's kept too; when it can't be
    /// reached, the address decides for now and it's asked again next
    /// time.
    async fn settle(&self) -> Deployment {
        if let Some(d) = self.forced {
            return d;
        }
        if let Some((d, _)) = site_for(&self.site, &self.base).said.clone() {
            return d;
        }
        let guess = Deployment::guess(&self.base);
        let by_address = (guess, format!("{}, by its address", if guess.is_cloud() { "Cloud" } else { "Server or Data Center" }));
        let said = match Direct::new(&self.base, None).send("GET", "/rest/api/2/serverInfo", None).await {
            Ok((200..=299, body)) => Deployment::from_server_info(&body).unwrap_or(by_address),
            Ok(_) => by_address,
            Err(_) => return guess,
        };
        api::info!("{} is Jira {}", host_of(&self.base), said.1);
        let mut site = site_for(&self.site, &self.base);
        if said.0 != guess {
            // A Cloud site on its own domain: who you are is an account id.
            *site.learned.borrow_mut() = Learned::default();
        }
        site.said = Some(said.clone());
        said.0
    }

    /// The seam's memory of this site.
    fn learned(&self) -> Rc<RefCell<Learned>> {
        site_for(&self.site, &self.base).learned.clone()
    }

    /// A scoped API token's base: Atlassian's gateway to the site, by the
    /// cloud id its `/_edge/tenant_info` says (asked once).
    async fn gateway(&self) -> Result<String, String> {
        if let Some(id) = site_for(&self.site, &self.base).cloud_id.clone() {
            return Ok(gateway(&id));
        }
        let (status, body) = Direct::new(&self.base, None).send("GET", "/_edge/tenant_info", None).await?;
        let id = (200..300).contains(&status).then(|| cloud_id(&body)).flatten().ok_or_else(|| {
            format!("couldn't find {}'s cloud id (its /_edge/tenant_info answered HTTP {status}) -- a scoped API token is taken only through api.atlassian.com, which needs it", host_of(&self.base))
        })?;
        site_for(&self.site, &self.base).cloud_id = Some(id.clone());
        Ok(gateway(&id))
    }

    /// A client for the site, signed in as it takes it.
    async fn client(self) -> Result<JiraClient, String> {
        let deployment = self.settle().await;
        if !deployment.is_cloud() {
            return Ok(JiraClient::new(Rc::new(Direct::new(&self.base, Some(Auth::Bearer(self.token))))));
        }
        let email = self.email.clone().ok_or_else(|| cloud_not_set_up(&settings_keys()))?;
        let base = if self.scoped { self.gateway().await? } else { self.base.clone() };
        Ok(JiraClient::cloud(Rc::new(Direct::new(&base, Some(Auth::Basic { email, token: self.token })))))
    }
}

/// `job` on a client for `connect`, or why there's none.
async fn with<T, F, Fut>(connect: Connect, job: F) -> Result<T, String>
where
    F: FnOnce(JiraClient) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    job(connect.client().await?).await
}

/// A job's end, taken in by `Jira::take_done`.
enum Done {
    Issues { jql: String, result: Result<Vec<IssueSummary>, String> },
    /// Boxed: an issue's detail is far bigger than the others.
    Detail { key: String, result: Result<Box<IssueDetail>, String> },
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
    /// What it has learned of the site: which Jira it is, and what the
    /// tracker seam learned (who you are, its Flagged field).
    site: Known,
    /// New-issue forms the seam's `create` opened, answered as the seam
    /// answers (`{"ref": ...}`) rather than with a bare key.
    seam_creates: HashSet<Waiting>,
}

impl Jira {
    fn queries(&self, here: Option<&str>) -> Vec<page::Query> {
        page::queries(&map("jira.projects"), &map("jira.users"), &map("jira.queries"), here)
    }

    fn ctx_parts(&self) -> (Vec<(String, String)>, String, chrono::NaiveDate, String) {
        (map("jira.users"), self.server_label(), api::clock::now().date_naive(), settings_keys())
    }

    /// The page's word on where it is: the site, and which Jira once it's
    /// known (`jira.example.com  ·  Jira Data Center 9.12.4`).
    fn server_label(&self) -> String {
        let Some(base) = base_url() else { return String::new() };
        let said = site_for(&self.site, &base).said.clone();
        match (Deployment::forced(api::setting("jira.kind").as_deref()), said) {
            (Some(d), _) => format!("{base}  ·  {}", d.name()),
            (None, Some((Deployment::Cloud, _))) => format!("{base}  ·  Jira Cloud (descriptions read-only here)"),
            (None, Some((_, words))) => format!("{base}  ·  Jira {words}"),
            (None, None) => base,
        }
    }

    fn connect(&self) -> Result<Connect, String> {
        connect(&self.site)
    }

    fn is_cloud(&self) -> bool {
        base_url().is_some_and(|b| deployment_now(&self.site, &b).is_cloud())
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
        let connect = match self.connect() {
            Ok(connect) => connect,
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
            let q = jql.clone();
            let result = with(connect, |c| async move { c.search_issues(&q, SEARCH_LIMIT).await }).await;
            Done::Issues { jql, result }
        });
    }

    fn fetch_detail(&mut self, key: String) {
        let Ok(connect) = self.connect() else { return };
        spawn(async move {
            let k = key.clone();
            let result = with(connect, |c| async move { c.get_issue(&k).await }).await;
            Done::Detail { key, result: result.map(Box::new) }
        });
    }

    /// A write to issue `key`, said when done, and the page brought up
    /// to date.
    fn write<F, Fut>(&mut self, key: String, job: F)
    where
        F: FnOnce(JiraClient) -> Fut + 'static,
        Fut: Future<Output = Result<String, String>> + 'static,
    {
        let connect = match self.connect() {
            Ok(connect) => connect,
            Err(why) => return host::notify(api::Level::Error, &why),
        };
        spawn(async move { Done::Wrote { key, result: with(connect, job).await } });
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
                host::notify(api::Level::Error, &format!("Jira isn't set up -- its server goes on the settings page ({})", settings_keys()));
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
                let payload = agenda_payload(&key, None);
                spawn(async move {
                    let _ = run::call("agenda.open_task", &payload).await;
                    Done::InAgenda(HashSet::new())
                });
            }
            A::Transitions(key) => {
                let Ok(connect) = self.connect() else { return };
                spawn(async move {
                    let k = key.clone();
                    let result = with(connect, |c| async move { c.list_transitions(&k).await }).await.map(|ts| {
                        ts.into_iter().map(|t| (format!("{}  →  {}", t.name, if t.to_name.is_empty() { t.name.clone() } else { t.to_name.clone() }), Choice::Transition { id: t.id, name: t.name })).collect()
                    });
                    Done::Offer { title: format!("Move {key}"), key, result }
                });
            }
            A::Priorities(key) => {
                let Ok(connect) = self.connect() else { return };
                spawn(async move {
                    let result = with(connect, |c| async move { c.list_priorities().await }).await.map(|ps| ps.into_iter().map(|p| (p.name.clone(), Choice::Priority(p.name))).collect());
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
            A::EditDescription(_) if self.is_cloud() => host::notify(api::Level::Error, fenix_jira_client::CLOUD_DESCRIPTION),
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
                let payload = agenda_payload(&key, Some(clock));
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
                let Ok(connect) = self.connect() else { return };
                spawn(async move {
                    let p = project.clone();
                    let result = with(connect, |c| async move { c.list_project_statuses(&p).await }).await;
                    Done::Statuses { project, result }
                });
            }
            A::IssueTypes(project) => {
                let Ok(connect) = self.connect() else { return };
                spawn(async move {
                    let (types, priorities) = match connect.client().await {
                        Ok(client) => (client.issue_types(&project).await, client.list_priorities().await.map(|ps| ps.into_iter().map(|p| p.name).collect()).unwrap_or_default()),
                        Err(why) => (Err(why), Vec::new()),
                    };
                    Done::Types { project, types, priorities }
                });
            }
            A::CreateFields { project, type_id } => {
                let Ok(connect) = self.connect() else { return };
                spawn(async move {
                    let t = type_id.clone();
                    let result = with(connect, |c| async move { c.create_fields(&project, &t).await }).await;
                    Done::Fields { type_id, result }
                });
            }
            A::Create(create) => {
                let page::Create { issue, agenda, clock, task } = *create;
                let connect = match self.connect() {
                    Ok(connect) => connect,
                    Err(why) => return host::notify(api::Level::Error, &why),
                };
                api::notify(&format!("Creating a {} issue...", issue.project));
                spawn(async move { Done::Created { result: with(connect, |c| async move { c.create(&issue).await }).await, agenda, clock, task } });
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
                        page.details.insert(key, result.map(|d| *d));
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
                            Some(call) if self.seam_creates.remove(&call) => api::answer(call, Ok(serde_json::json!({ "ref": key }).to_string())),
                            Some(call) => api::answer(call, Ok(key.clone())),
                            None if agenda => {
                                let payload = agenda_payload(&key, Some(clock));
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
                            let answer = if self.seam_creates.remove(&call) { seam::classify(err.clone()).to_err() } else { err.clone() };
                            api::answer(call, Err(answer));
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

    /// The new-issue form, filled in from an agenda task; `call` is
    /// answered once the issue is made.
    fn new_issue_form(&mut self, call: Waiting, summary: String, description: String) {
        self.open_page();
        let projects = self.form_projects();
        let action = self.page.as_mut().map(|(_, p)| p.start_issue(projects, Some((call, summary, description))));
        if let Some(action) = action {
            self.action(action);
        }
        self.take_done();
    }

    /// `jira.tracker.<call>`: the tracker seam. `about`, `create` and
    /// `remember` are this extension's own (its settings, its form); the
    /// rest are `fenix_jira_client::tracker`'s, over its own HTTP,
    /// answered once Jira has.
    fn serve_seam(&mut self, call: Waiting, name: &str, payload: &str) -> Option<Result<String, String>> {
        let settings = tracker_settings();
        let refuse = |code: &str, why: String| Some(Err(SeamError::new(code, why).to_err()));
        match name {
            fenix_tracker::call::ABOUT => {
                let connect = self.connect();
                let deployment = base_url().map_or(Deployment::Server, |b| deployment_now(&self.site, &b));
                // Not known yet: asked now, so the agenda's next look sees it.
                if let Ok(connect) = &connect {
                    let connect = connect.clone();
                    run::spawn(async move {
                        connect.settle().await;
                    });
                }
                Some(serde_json::to_string(&seam::about_for(&settings, connect.is_ok(), deployment)).map_err(|e| e.to_string()))
            }
            fenix_tracker::call::REMEMBER => match serde_json::from_str::<fenix_tracker::Remember>(payload).ok().and_then(|r| seam::parse_remember(&r.remember)) {
                Some((project, blocked)) => {
                    self.set_blocked(&project, &blocked.text());
                    Some(Ok("{}".into()))
                }
                None => refuse(code::BAD_REQUEST, format!("a Jira call: what to remember isn't Jira's: {payload}")),
            },
            fenix_tracker::call::CREATE => {
                let new: fenix_tracker::Create = match serde_json::from_str(payload) {
                    Ok(new) => new,
                    Err(e) => return refuse(code::BAD_REQUEST, format!("a Jira call: {e}")),
                };
                if let Err(why) = self.connect() {
                    return refuse(code::NOT_SET_UP, why);
                }
                self.seam_creates.insert(call);
                self.new_issue_form(call, new.title, new.body);
                None
            }
            _ => {
                let deployment = base_url().map_or(Deployment::Server, |b| deployment_now(&self.site, &b));
                let connect = match self.connect() {
                    Ok(connect) => connect,
                    Err(_) => return refuse(code::NOT_SET_UP, seam::about_for(&settings, false, deployment).not_ready.unwrap_or_default()),
                };
                let (name, payload) = (name.to_string(), payload.to_string());
                run::spawn(async move {
                    let learned = connect.learned();
                    let answer = match connect.client().await {
                        Ok(client) => seam::Seam::new(client, settings, learned).serve(&name, &payload).await,
                        Err(why) if why.contains("isn't set up") => Err(SeamError::new(code::NOT_SET_UP, why).to_err()),
                        Err(why) => Err(seam::classify(why).to_err()),
                    };
                    api::answer(call, answer);
                });
                None
            }
        }
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
    /// You switched between Vim keys and standard keys: the page names
    /// the settings page's keys again.
    fn key_scheme_changed(&mut self, _scheme: api::KeyScheme) {
        self.refresh();
    }

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
        #[cfg(test)]
        let Some(event) = kit_fetch::deliver(event) else { return self.take_done() };
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
        let (people, server, today, settings_keys) = self.ctx_parts();
        let ctx = page::Ctx { in_agenda: &self.in_agenda, people: &people, server, today, settings_keys };
        Page::canvas(page::layout(p, &ctx, width as usize)).typing(p.typing()).space(p.claims_space())
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some((id, mut p)) = self.page.take() else { return };
        if id != page {
            self.page = Some((id, p));
            return;
        }
        let (people, server, today, settings_keys) = self.ctx_parts();
        let ctx = page::Ctx { in_agenda: &self.in_agenda, people: &people, server, today, settings_keys };
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
        if let Some(seam_call) = fenix_tracker::call_of("jira", name) {
            return self.serve_seam(call, seam_call, payload);
        }
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
                // As before for Server; Cloud signed in as it takes it.
                let connect = match self.connect() {
                    Ok(connect) => connect,
                    Err(why) => return Some(Err(why)),
                };
                run::spawn(async move {
                    let direct = match deployment_now(&connect.site, &connect.base) {
                        Deployment::Cloud => Direct::new(&connect.base, connect.email.clone().map(|email| Auth::Basic { email, token: connect.token.clone() })),
                        Deployment::Server => Direct::new(&connect.base, Some(Auth::Bearer(connect.token.clone()))),
                    };
                    let answer = direct.send(&req.method, &req.path, req.body).await.map(|(status, body)| serde_json::json!({ "status": status, "body": body }).to_string());
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
                self.new_issue_form(call, new.summary, new.description);
                return None;
            }
            other => Err(format!("no service {other}")),
        };
        Some(answer)
    }
}

api::export!(Jira);

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::{Fake, Harness};
    use fenix_tracker::SeamError;

    const TOKEN: &str = "NjQ1MzIxOTg3NjU0OlRPS0VO-secret";

    fn jira(token: bool) -> Harness<Jira> {
        let fake = Fake::new();
        fake.setting("jira.base_url", "https://jira.example.com/")
            .setting("jira.projects", r#"[["PROJ", "Project"]]"#)
            .setting("jira.users", r#"[["jo1", "Jo Bloggs"]]"#)
            .setting("jira.blocked", r#"[["PROJ", "flag"]]"#)
            .setting("jira.sync_minutes", "5");
        if token {
            fake.secret("jira.token", TOKEN);
        }
        Harness::with_fake(fake)
    }

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    /// The agenda 1.2 calls these, and Jira 1.3 answers them as 1.2 did.
    #[test]
    fn the_agenda_1_2s_services_answer_as_they_did() {
        let mut h = jira(true);
        let config = h.serve("jira.config", "null").unwrap().unwrap();
        assert_eq!(
            json(&config),
            serde_json::json!({ "base_url": "https://jira.example.com", "projects": ["PROJ"], "users": [["jo1", "Jo Bloggs"]], "priorities": [], "blocked": [["PROJ", "flag"]], "sync_minutes": 5 })
        );
        assert_eq!(h.serve("jira.set_blocked", r#"{"project": "OPS", "blocked": "local"}"#), Some(Ok(String::new())));
        assert_eq!(h.fake.setting_value("jira.blocked").as_deref(), Some(r#"[["PROJ","flag"],["OPS","local"]]"#));
        assert!(!config.contains(TOKEN));

        let mut h = jira(false);
        let refused = h.serve("jira.request", r#"{"method": "GET", "path": "/rest/api/2/myself", "body": null}"#);
        assert_eq!(refused, Some(Err(not_set_up("SPC ,"))));

        let manifest: toml_lite::Services = toml_lite::services(include_str!("../extension.toml"));
        for old in ["jira.request", "jira.config", "jira.set_blocked", "jira.new_issue"] {
            assert!(manifest.0.iter().any(|s| s == old), "{old} still served");
        }
        for call in fenix_tracker::call::ALL {
            assert!(manifest.0.contains(&fenix_tracker::service("jira", call)), "jira.tracker.{call}");
        }
    }

    #[test]
    fn the_seam_says_who_it_is_and_keeps_what_blocked_means() {
        let mut h = jira(true);
        let about: fenix_tracker::About = serde_json::from_str(&h.serve("jira.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
        assert_eq!((about.name.as_str(), about.ready, about.sync_minutes), ("Jira", true, 5));
        assert_eq!(about.browse.as_deref(), Some("https://jira.example.com/browse/{ref}"));
        assert_eq!(about.can.len(), fenix_tracker::can::ALL.len());

        let remember = serde_json::json!({ "seam": 1, "ref": "OPS-1", "remember": serde_json::json!({ "project": "OPS", "blocked": "10103: On Hold" }).to_string() }).to_string();
        assert_eq!(h.serve("jira.tracker.remember", &remember), Some(Ok("{}".into())));
        assert_eq!(h.fake.setting_value("jira.blocked").as_deref(), Some(r#"[["PROJ","flag"],["OPS","10103: On Hold"]]"#));
        let odd = SeamError::from_err(&h.serve("jira.tracker.remember", r#"{"seam": 1, "remember": "not Jira's"}"#).unwrap().unwrap_err());
        assert_eq!(odd.code, fenix_tracker::code::BAD_REQUEST);

        let mut h = jira(false);
        let about: fenix_tracker::About = serde_json::from_str(&h.serve("jira.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
        assert_eq!((about.ready, about.not_ready.as_deref()), (false, Some("Jira isn't set up yet -- set its server and token in SPC , (Jira)")));
        let err = SeamError::from_err(&h.serve("jira.tracker.issues", r#"{"seam": 1, "refs": ["PROJ-1"]}"#).unwrap().unwrap_err());
        assert_eq!((err.code.as_str(), err.message.as_str()), ("not_set_up", "Jira isn't set up yet -- set its server and token in SPC , (Jira)"));
        let err = SeamError::from_err(&h.serve("jira.tracker.create", r#"{"seam": 1, "title": "T"}"#).unwrap().unwrap_err());
        assert_eq!(err.code, "not_set_up");
    }

    #[test]
    fn its_page_tells_the_agenda_which_tracker_an_issue_is_from() {
        assert_eq!(json(&agenda_payload("PROJ-1", Some(true))), serde_json::json!({ "tracker": "jira", "ref": "PROJ-1", "key": "PROJ-1", "clock": true }));
        assert_eq!(json(&agenda_payload("PROJ-1", None)), serde_json::json!({ "tracker": "jira", "ref": "PROJ-1", "key": "PROJ-1" }));
    }

    /// A Jira Cloud site, answering from the client's Cloud fixtures
    /// (written from Atlassian's documented shapes, not recordings).
    fn cloud(email: bool) -> Harness<Jira> {
        const SITE: &str = "https://acme.atlassian.net";
        let fixture = |name: &str| match name {
            "server-info" => include_str!("../../../crates/jira/tests/fixtures/cloud/server-info.json"),
            "myself" => include_str!("../../../crates/jira/tests/fixtures/cloud/myself.json"),
            "issue" => include_str!("../../../crates/jira/tests/fixtures/cloud/issue-PROJ-1.json"),
            "search" => include_str!("../../../crates/jira/tests/fixtures/cloud/search-jql-linked.json"),
            _ => unreachable!(),
        };
        let fake = Fake::new();
        fake.setting("jira.base_url", "https://acme.atlassian.net/").setting("jira.projects", r#"[["PROJ", "Project"]]"#).secret("jira.token", CLOUD_TOKEN);
        if email {
            fake.setting("jira.email", "jo@example.com");
        }
        fake.on_http(&format!("GET {SITE}/rest/api/2/serverInfo"), api::HttpResponse::new(200, fixture("server-info")))
            .on_http(&format!("GET {SITE}/rest/api/2/myself"), api::HttpResponse::new(200, fixture("myself")))
            .on_http(&format!("GET {SITE}/rest/api/3/issue/PROJ-1?*"), api::HttpResponse::new(200, fixture("issue")))
            .on_http(&format!("GET {SITE}/rest/api/3/search/jql?*"), api::HttpResponse::new(200, fixture("search")))
            .on_http(&format!("POST {SITE}/rest/api/3/issue/PROJ-1/comment"), api::HttpResponse::new(201, "{}"));
        Harness::with_fake(fake)
    }

    const CLOUD_TOKEN: &str = "ATATT3xFfGF0-cloud-secret";

    /// What the seam call it answered later answered.
    fn answered(h: &Harness<Jira>) -> Result<String, String> {
        h.fake.state().answers.values().last().cloned().expect("an answer")
    }

    #[test]
    fn cloud_is_told_by_its_address_and_signed_in_to_with_email_and_token() {
        let mut h = cloud(true);
        let about: fenix_tracker::About = serde_json::from_str(&h.serve("jira.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
        assert!(about.ready && !about.can(fenix_tracker::can::EDIT), "{about:?}");
        assert_eq!(about.browse.as_deref(), Some("https://acme.atlassian.net/browse/{ref}"));

        assert_eq!(h.serve("jira.tracker.issues", r#"{"seam": 1, "refs": ["PROJ-1"]}"#), None);
        h.settle();
        let found: fenix_tracker::Found = serde_json::from_str(&answered(&h).unwrap()).unwrap();
        assert_eq!(found.issues[0].assignee.as_ref().unwrap().id, "5b10ac8d82e05b22cc7d4ef5");
        assert!(found.issues[0].body.starts_with("Users are logged out after 5 minutes.\n\nSteps\n\n1. Log in"), "{}", found.issues[0].body);

        let requests = h.fake.requests();
        let auth = |r: &api::HttpRequest| r.headers.iter().find(|(n, _)| n == "Authorization").map(|(_, v)| v.clone());
        // serverInfo is asked as anyone; everything else as you, by Basic.
        let info = requests.iter().find(|(_, r)| r.url.ends_with("/rest/api/2/serverInfo")).unwrap();
        assert_eq!(auth(&info.1), None);
        let basic = format!("Basic {}", fenix_jira_client::cloud::base64(format!("jo@example.com:{CLOUD_TOKEN}").as_bytes()));
        let issue = requests.iter().find(|(_, r)| r.url.contains("/rest/api/3/issue/PROJ-1?")).unwrap();
        assert_eq!(auth(&issue.1), Some(basic.clone()));

        // The token is in no log, no notification, no answer.
        let said = [h.fake.logs().join("\n"), h.fake.calls().join("\n"), h.fake.notifications().join("\n")].join("\n");
        assert!(said.contains("acme.atlassian.net is Jira Cloud"), "{said}");
        for secret in [CLOUD_TOKEN, &basic[6..]] {
            assert!(!said.contains(secret), "the token in what it said: {said}");
        }

        // The page says which Jira it is.
        assert_eq!(h.ext.server_label(), "https://acme.atlassian.net  ·  Jira Cloud (descriptions read-only here)");
    }

    #[test]
    fn cloud_without_an_email_isnt_ready_and_says_what_to_set() {
        let mut h = cloud(false);
        let about: fenix_tracker::About = serde_json::from_str(&h.serve("jira.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
        assert_eq!((about.ready, about.not_ready.as_deref()), (false, Some("Jira Cloud isn't set up yet -- set its site, your email and an API token in SPC , (Jira)")));
        let err = SeamError::from_err(&h.serve("jira.tracker.issues", r#"{"seam": 1, "refs": ["PROJ-1"]}"#).unwrap().unwrap_err());
        assert_eq!(err.code, "not_set_up");
        assert!(h.fake.requests().iter().all(|(_, r)| r.headers.iter().all(|(n, _)| n != "Authorization")), "nothing sent as you");
    }

    #[test]
    fn a_comment_from_the_page_goes_to_cloud_as_rich_text_and_its_description_isnt_edited() {
        let mut h = cloud(true);
        h.serve("jira.tracker.comment", r#"{"seam": 1, "ref": "PROJ-1", "body": "Done.\nShipped."}"#);
        h.settle();
        assert_eq!(answered(&h), Ok("{}".to_string()));
        let (_, post) = h.fake.requests().into_iter().find(|(_, r)| r.method == "POST").unwrap();
        let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        assert_eq!(body["body"]["content"][0]["content"][1], serde_json::json!({"type": "hardBreak"}));

        h.ext.action(A::EditDescription("PROJ-1".into()));
        assert!(h.fake.notifications().iter().any(|n| n.contains("Jira Cloud keeps a description as rich text")), "{:?}", h.fake.notifications());
    }

    #[test]
    fn a_429s_retry_after_reaches_the_seam() {
        let mut h = cloud(true);
        let mut slow = api::HttpResponse::new(429, "");
        slow.headers.push(("Retry-After".into(), "42".into()));
        h.fake.on_http("GET https://acme.atlassian.net/rest/api/3/issue/PROJ-1?*", slow);
        h.serve("jira.tracker.issues", r#"{"seam": 1, "refs": ["PROJ-1"]}"#);
        h.settle();
        let err = SeamError::from_err(&answered(&h).unwrap_err());
        assert_eq!((err.code.as_str(), err.retry_after), ("rate_limited", Some(42)));
    }

    #[test]
    fn a_server_is_still_signed_in_to_with_its_bearer_token_and_says_so() {
        let mut h = jira(true);
        h.fake
            .on_http("GET https://jira.example.com/rest/api/2/serverInfo", api::HttpResponse::new(200, r#"{"version":"9.12.4","deploymentType":"DataCenter"}"#))
            .on_http("GET https://jira.example.com/rest/api/2/issue/PROJ-1/transitions", api::HttpResponse::new(200, r#"{"transitions":[]}"#));
        h.serve("jira.tracker.transitions", r#"{"seam": 1, "ref": "PROJ-1"}"#);
        h.settle();
        assert_eq!(answered(&h), Ok(r#"{"transitions":[],"blocked":"Blocked  (flag as impediment)"}"#.to_string()));
        let (_, asked) = h.fake.requests().into_iter().find(|(_, r)| r.url.ends_with("/transitions")).unwrap();
        assert_eq!(asked.headers, [("Authorization".to_string(), format!("Bearer {TOKEN}")), ("Accept".to_string(), "application/json".to_string())], "as before");
        assert_eq!(h.ext.server_label(), "https://jira.example.com  ·  Jira Data Center 9.12.4");
    }

    /// Just enough of the manifest: its services.
    mod toml_lite {
        pub struct Services(pub Vec<String>);

        pub fn services(manifest: &str) -> Services {
            let start = manifest.find("services = [").expect("services") + "services = [".len();
            let end = start + manifest[start..].find(']').expect("]");
            Services(manifest[start..end].split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect())
        }
    }

    #[test]
    fn not_set_up_names_the_settings_keys_of_the_scheme_in_use() {
        assert_eq!(not_set_up("SPC ,"), "Jira isn't set up yet -- set its server and token on the settings page (SPC ,), under Jira");
        assert_eq!(not_set_up("C-,"), "Jira isn't set up yet -- set its server and token on the settings page (C-,), under Jira");
        // Natively there's no Fenix to ask: its Vim keys.
        assert_eq!(settings_keys(), "SPC ,");
    }
}
