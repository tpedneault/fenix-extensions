//! CI status in Fenix: the open project's branch on GitHub Actions,
//! GitLab pipelines or Jenkins, on the modeline (`CI ✓`, `CI ● 3/7`,
//! `CI ✗ 2 failed`); a page of its runs, their jobs and steps, logs,
//! re-runs and cancels (`SPC g C`); a notification when a run finishes;
//! and a tool for coding agents that gives them the failing jobs and
//! their logs.
//!
//! GitHub and GitLab are found by the repository's remote; a Jenkins job
//! is named in the settings (`jenkins`), its builds the runs and their
//! stages the jobs.
//!
//! A timer looks at the repository every few seconds (`host::git_state`):
//! a new branch, a new `HEAD` or a push asks the forge at once; otherwise
//! it asks every 30 s while something runs and every 2 minutes when
//! nothing does (`poll`). Everything that talks to the forge runs as a
//! job (`run::spawn`) and leaves what it found in `DONE`, applied once
//! it lands.

mod client;
mod github;
mod gitlab;
mod jenkins;
mod keys;
mod log;
mod model;
mod notify;
mod page;
mod poll;
mod proc;
mod remote;
mod summary;
mod tool;

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;

use fenix_extension_api::{self as api, clock, export, host, page as pages, run, Command, Event, Extension, HttpOptions, HttpRequest, Key, Notification, Page, Urgency};
use fenix_forge::{Http, Response};

use client::{Client, Shared, Snapshot};
use model::{Job, Provider};
use page::{Action, CiPage, Data};
use poll::{Change, Intervals, Poller, Seen};
use remote::{Config, Repo};

const PAGE_KIND: &str = "runs";
const PROJECT: &str = "/project";
/// How often it looks at the repository (Fenix reads it at most every 2 s).
const TICK_MS: u32 = 3_000;

/// Fenix's HTTP client.
pub struct Fetch;

impl Http for Fetch {
    async fn request(&self, method: &str, url: &str, headers: &[(&str, String)], body: Option<String>) -> Result<Response, String> {
        let req = HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            body: body.map(String::into_bytes).unwrap_or_default(),
            options: HttpOptions::new().timeout(30_000),
        };
        let r = run::fetch(req).await?;
        Ok(Response { status: r.status, headers: r.headers, body: String::from_utf8_lossy(&r.body).into_owned() })
    }
}

fn now_ms() -> i64 {
    clock::now().timestamp_millis()
}

fn now_s() -> i64 {
    clock::now().timestamp()
}

/// What a job found, applied once it's done.
enum Done {
    Polled { generation: u64, branch: String, result: Result<Snapshot, String> },
    Jobs { generation: u64, run: u64, result: Result<Vec<Job>, String> },
    /// A re-run or a cancel went out (or didn't).
    Wrote { label: String, result: Result<(), String> },
    /// `origin`'s URL, from `git` (a worktree's `.git` is a file).
    Remote(Option<String>),
    Say(Result<String, String>),
}

thread_local! {
    static DONE: RefCell<Vec<Done>> = const { RefCell::new(Vec::new()) };
    /// The forge's token, by host: looked up once (`gh` is a program).
    static TOKENS: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

fn done(d: Done) {
    DONE.with(|q| q.borrow_mut().push(d));
}

fn job(f: impl Future<Output = Result<(), String>> + 'static) {
    run::spawn(async move {
        if let Err(e) = f.await {
            done(Done::Say(Err(e)));
        }
    });
}

fn setting(key: &str) -> Option<String> {
    api::setting(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn on(key: &str, default: bool) -> bool {
    setting(key).map(|v| v == "true").unwrap_or(default)
}

/// The settings, as `remote` reads them.
fn config() -> Config {
    Config {
        provider: match setting("ci.provider").as_deref() {
            Some("github") => Some(Provider::GitHub),
            Some("gitlab") => Some(Provider::GitLab),
            Some("jenkins") => Some(Provider::Jenkins),
            _ => None,
        },
        github_url: setting("ci.github_url"),
        gitlab_url: setting("ci.gitlab_url").or_else(|| setting("gitlab.base_url")),
        jenkins_url: setting("ci.jenkins_url"),
        jenkins_job: setting("ci.jenkins_job"),
        settings_keys: keys::settings(),
    }
}

fn intervals() -> Intervals {
    Intervals::from_seconds(setting("ci.poll_running").as_deref(), setting("ci.poll_idle").as_deref())
}

/// The forge's token: Fenix's own (`github.token`, `gitlab.token`), else
/// the GitHub CLI's for that host. None for a public repository. A
/// Jenkins's is this extension's own: a user and their API token, as
/// `user:token`.
async fn token(repo: &Repo) -> Option<String> {
    if let Some(known) = TOKENS.with(|t| t.borrow().get(&repo.host).cloned()) {
        return known;
    }
    let fenix = |key: &str| host::secret(key).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let found = match repo.provider {
        Provider::GitLab => fenix("gitlab.token"),
        Provider::Jenkins => setting("ci.jenkins_user").zip(fenix("ci.jenkins_token")).map(|(user, token)| format!("{user}:{token}")),
        Provider::GitHub if repo.host == "github.com" => match fenix("github.token") {
            Some(t) => Some(t),
            None => proc::stdout(Command::new("gh").args(["auth", "token"])).await,
        },
        Provider::GitHub => match proc::stdout(Command::new("gh").args(["auth", "token", "--hostname", &repo.host])).await {
            Some(t) => Some(t),
            None => fenix("github.token"),
        },
    };
    let found = found.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    TOKENS.with(|t| t.borrow_mut().insert(repo.host.clone(), found.clone()));
    found
}

async fn client(repo: Repo, memory: Shared) -> Client<Fetch> {
    let token = token(&repo).await;
    Client::new(Fetch, repo, token, memory, now_ms).settings_at(keys::settings())
}

/// `origin`'s URL, from the project's `.git/config`.
fn read_remote() -> Result<String, bool> {
    let git = format!("{PROJECT}/.git");
    match std::fs::metadata(&git) {
        Ok(m) if m.is_dir() => {
            let text = std::fs::read_to_string(format!("{git}/config")).map_err(|_| true)?;
            remote::remote_url(&text).ok_or(false)
        }
        // A worktree's `.git` file, or a project inside the repository:
        // `git` knows.
        _ => Err(true),
    }
}

#[derive(Default)]
pub struct Ci {
    data: Data,
    repo: Option<Repo>,
    memory: Shared,
    poller: Poller,
    watcher: notify::Watcher,
    pages: HashMap<u64, CiPage>,
    timer: Option<u64>,
    /// Bumped when the branch or the repository changes: what a job
    /// found for the one before is dropped.
    generation: u64,
    /// The failed job "Open the log" opens: (run, job).
    last_failed: Option<(u64, u64)>,
    /// `git` is being asked for the remote.
    asking_git: bool,
    remote: Option<String>,
}

impl Ci {
    fn refresh_pages(&self) {
        for id in self.pages.keys() {
            pages::refresh(*id);
        }
    }

    fn say_status(&self) {
        if self.repo.is_none() {
            api::status(None);
            return;
        }
        let text = match (&self.data.summary, &self.data.error) {
            (Some(s), Some(_)) => format!("{} ?", s.modeline()),
            (Some(s), None) => s.modeline(),
            (None, Some(_)) => "CI ?".to_string(),
            (None, None) => return,
        };
        api::status(Some(&text));
    }

    /// Works out where the CI is, from the remote's URL.
    fn resolve(&mut self) {
        let url = match read_remote() {
            Ok(url) => Some(url),
            Err(true) if !self.asking_git && self.remote.is_none() => {
                self.asking_git = true;
                run::spawn(async {
                    let url = proc::stdout(Command::new("git").args(["remote", "get-url", "origin"]).cwd(PROJECT).env("GIT_TERMINAL_PROMPT", "0")).await;
                    done(Done::Remote(url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty())));
                });
                return;
            }
            Err(true) => self.remote.clone(),
            Err(false) => None,
        };
        self.set_remote(url);
    }

    fn set_remote(&mut self, url: Option<String>) {
        self.remote = url.clone();
        match remote::locate(url.as_deref(), &config()) {
            Ok(repo) => {
                if self.repo.as_ref() != Some(&repo) {
                    *self.memory.borrow_mut() = client::Memory::default();
                    self.generation += 1;
                    self.data.runs.clear();
                    self.data.jobs.clear();
                    self.data.summary = None;
                    self.data.checked = None;
                    self.data.error = None;
                    self.data.project = Some(repo.project.clone());
                    self.data.provider = Some(repo.provider);
                    self.poller.now(now_ms());
                    self.repo = Some(repo);
                }
            }
            Err(e) => {
                self.repo = None;
                self.data.project = None;
                self.data.provider = None;
                self.data.error = Some(e);
            }
        }
        self.say_status();
        self.refresh_pages();
    }

    /// The timer: the repository looked at, and the forge asked when it's time.
    fn tick(&mut self) {
        let at = now_ms();
        let Some(git) = host::git_state() else {
            if self.repo.take().is_some() || self.data.error.is_none() {
                self.data = Data { error: Some("The project isn't a Git repository.".into()), ..Data::default() };
                api::status(None);
                self.refresh_pages();
            }
            return;
        };
        let change = self.poller.git(Seen { branch: git.branch.clone(), head: git.head.clone(), ahead: git.ahead }, at);
        if change != Change::None || self.repo.is_none() {
            if change == Change::Branch {
                self.generation += 1;
                self.data.runs.clear();
                self.data.jobs.clear();
                self.data.summary = None;
                self.data.checked = None;
            }
            self.data.branch = git.branch.clone();
            self.data.head = git.head.clone();
            self.resolve();
            self.refresh_pages();
        }
        if self.poller.due(at) {
            self.poll();
        }
    }

    /// Asks the forge for the branch's runs.
    fn poll(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let Some(branch) = self.data.branch.clone() else {
            self.data.error = Some("HEAD is detached: CI is shown for a branch.".into());
            self.poller.finished(true, false, false, None, intervals(), now_ms());
            self.refresh_pages();
            return;
        };
        self.poller.started();
        self.data.loading = true;
        let head = self.data.head.clone();
        // The jobs of runs open on a page that may still change.
        let also: Vec<u64> = self.pages.values().flat_map(|p| p.open_runs().collect::<Vec<_>>()).filter(|id| self.data.runs.iter().any(|r| r.id == *id && (!r.state.done() || !self.data.jobs.contains_key(id)))).collect();
        let (memory, generation) = (self.memory.clone(), self.generation);
        run::spawn(async move {
            let client = client(repo, memory).await;
            let result = client.snapshot(&branch, head.as_deref(), &also).await;
            done(Done::Polled { generation, branch, result });
        });
        self.refresh_pages();
    }

    fn apply(&mut self, d: Done) {
        match d {
            Done::Polled { generation, branch, result } => {
                let at = now_ms();
                self.data.loading = false;
                let blocked = self.memory.borrow().blocked_until;
                if generation != self.generation {
                    self.poller.finished(true, false, false, blocked, intervals(), at);
                    self.poller.now(at);
                    return;
                }
                match result {
                    Ok(snap) => {
                        self.data.error = None;
                        self.data.checked = Some(at / 1000);
                        let ids: Vec<u64> = snap.runs.iter().map(|r| r.id).collect();
                        self.data.jobs.retain(|id, _| ids.contains(id));
                        self.data.jobs.extend(snap.jobs);
                        self.data.runs = snap.runs;
                        let summary = page::summary_of(&self.data);
                        let (current, _) = summary::current(&self.data.runs, self.data.head.as_deref());
                        if self.watcher.observe(&branch, &current) {
                            let wanted = notify::Wanted { success: on("ci.notify_success", true), failure: on("ci.notify_failure", true), critical: setting("ci.failure_urgency").as_deref() != Some("normal") };
                            if let Some(note) = notify::note(&branch, &current, &self.data.jobs, wanted, at / 1000) {
                                self.last_failed = note.failed.or(self.last_failed);
                                show(&note);
                            }
                        }
                        let waiting = summary.earlier || self.data.runs.is_empty();
                        self.poller.finished(true, summary.active(), waiting, blocked, intervals(), at);
                        self.data.summary = Some(summary);
                    }
                    Err(e) => {
                        api::warn!("CI: {e}");
                        self.data.error = Some(e);
                        let active = self.data.summary.as_ref().is_some_and(|s| s.active());
                        self.poller.finished(false, active, false, blocked, intervals(), at);
                    }
                }
                for p in self.pages.values_mut() {
                    p.open_first(&self.data);
                }
                self.say_status();
                self.refresh_pages();
            }
            Done::Jobs { generation, run, result } => {
                self.data.reading.remove(&run);
                if generation == self.generation {
                    match result {
                        Ok(jobs) => {
                            self.data.jobs.insert(run, jobs);
                        }
                        Err(e) => self.say_on_pages(&format!("Couldn't read its jobs: {e}"), true),
                    }
                }
                self.refresh_pages();
            }
            Done::Wrote { label, result } => {
                match result {
                    Ok(()) => {
                        self.say_on_pages(&label, false);
                        api::notify(&label);
                        // Its new state, soon.
                        self.poller.next_at = now_ms() + 3_000;
                    }
                    Err(e) => self.say_on_pages(&e, true),
                }
                self.refresh_pages();
            }
            Done::Remote(url) => {
                self.asking_git = false;
                self.set_remote(url);
            }
            Done::Say(Ok(text)) => api::notify(&text),
            Done::Say(Err(e)) => {
                host::notify(api::Level::Error, &e);
                self.say_on_pages(&e, true);
            }
        }
    }

    fn say_on_pages(&mut self, text: &str, bad: bool) {
        for p in self.pages.values_mut() {
            p.message = Some((text.to_string(), bad));
        }
    }

    fn act(&mut self, action: Action) {
        let Some(repo) = self.repo.clone() else { return };
        let memory = self.memory.clone();
        match action {
            Action::None => {}
            Action::Refresh => {
                self.poller.now(now_ms());
                self.poll_if_due();
            }
            Action::Jobs(run_id) => {
                self.data.reading.insert(run_id);
                let generation = self.generation;
                run::spawn(async move {
                    let result = client(repo, memory).await.jobs(run_id).await;
                    done(Done::Jobs { generation, run: run_id, result });
                });
            }
            Action::Log { run: _, job: job_id } => {
                let name = self.data.jobs.values().flatten().find(|j| j.id == job_id).map(|j| j.name.clone()).unwrap_or_else(|| job_id.to_string());
                let url = self.data.jobs.values().flatten().find(|j| j.id == job_id).map(|j| j.url.clone()).unwrap_or_default();
                api::notify(&format!("Reading the log of {name}…"));
                job(async move {
                    let (text, partial) = client(repo, memory).await.log(job_id, &name).await.map_err(|e| format!("Couldn't read the log of {name}: {e}"))?;
                    let text = if partial { format!("… only the end of the log: the whole of it is at {url}\n\n{text}") } else { text };
                    api::open_text(&format!("CI log · {name}"), &text, Some("log"));
                    Ok(())
                });
            }
            Action::Rerun { run: run_id, job: job_id } => {
                let label = self.describe(run_id, job_id, "Re-running");
                run::spawn(async move {
                    let result = client(repo, memory).await.rerun(run_id, job_id).await;
                    done(Done::Wrote { label, result });
                });
            }
            Action::Cancel { run: run_id, job: job_id } => {
                let label = self.describe(run_id, job_id, "Cancelling");
                run::spawn(async move {
                    let result = client(repo, memory).await.cancel(run_id, job_id).await;
                    done(Done::Wrote { label, result });
                });
            }
            Action::Browser(url) => {
                if let Err(e) = api::open_url(&url) {
                    host::notify(api::Level::Error, &e);
                }
            }
        }
    }

    /// "Re-running the failed jobs of CI #482", "Cancelling the job test";
    /// on Jenkins "Building rocket again", "Stopping rocket #14".
    fn describe(&self, run: u64, job: Option<u64>, verb: &str) -> String {
        let found = self.data.runs.iter().find(|r| r.id == run);
        let run_name = found.map(|r| format!("{} #{}", r.name, r.number)).unwrap_or_else(|| format!("#{run}"));
        if self.data.provider == Some(Provider::Jenkins) {
            return match (verb, found) {
                ("Re-running", Some(r)) => format!("Building {} again", r.name),
                ("Re-running", None) => "Building it again".to_string(),
                _ => format!("Stopping {run_name}"),
            };
        }
        match job.and_then(|j| self.data.jobs.values().flatten().find(|x| x.id == j)) {
            Some(j) => format!("{verb} the job {} of {run_name}", j.name),
            None if verb == "Re-running" => format!("{verb} the failed jobs of {run_name}"),
            None => format!("{verb} {run_name}"),
        }
    }

    fn poll_if_due(&mut self) {
        if self.poller.due(now_ms()) {
            self.poll();
        } else if self.poller.blocked_until.is_some() {
            self.say_on_pages("Rate limited: it asks again once the limit lifts.", true);
        }
    }

    fn open_page(&mut self) {
        if let Some(id) = self.pages.keys().next().copied() {
            pages::focus(id);
        } else {
            let id = pages::open(PAGE_KIND, "CI");
            let mut p = CiPage::default();
            p.open_first(&self.data);
            self.pages.insert(id, p);
        }
        if self.data.checked.is_none_or(|c| now_s() - c > 10) {
            self.poller.now(now_ms());
            self.poll_if_due();
        }
    }

    /// The failed job to open: the one the last notification was about,
    /// else the first that failed of the runs that count.
    fn failed_job(&self) -> Option<(u64, u64)> {
        let known = |(r, j): (u64, u64)| self.data.jobs.get(&r).is_some_and(|l| l.iter().any(|x| x.id == j));
        self.last_failed.filter(|f| known(*f)).or_else(|| {
            let (current, _) = summary::current(&self.data.runs, self.data.head.as_deref());
            tool::failed(&current, &self.data.jobs).first().map(|(r, j)| (r.id, j.id))
        })
    }

    /// The agents' tool: the branch's CI asked afresh, its failed jobs'
    /// logs read, the report given back.
    fn failures(&mut self, call: u64, payload: &str) -> Option<Result<String, String>> {
        let Some(repo) = self.repo.clone() else {
            return Some(Err(self.data.error.clone().unwrap_or_else(|| "No CI to read: the project has no GitHub or GitLab remote, and no Jenkins job is set.".into())));
        };
        let Some(branch) = self.data.branch.clone() else { return Some(Err("HEAD is detached: CI is read for a branch.".into())) };
        let (head, memory, lines) = (self.data.head.clone(), self.memory.clone(), tool::lines_asked(payload));
        run::spawn(async move {
            api::progress(call, "asking the forge", Some(0.1));
            let client = client(repo.clone(), memory).await;
            let snap = match client.snapshot(&branch, head.as_deref(), &[]).await {
                Ok(s) => s,
                Err(e) => return api::answer(call, Err(e)),
            };
            let (current, earlier) = summary::current(&snap.runs, head.as_deref());
            let summary = summary::summarise(&current, &snap.jobs, earlier);
            let failed = tool::failed(&current, &snap.jobs);
            let mut logs = HashMap::new();
            for (i, (_, j)) in failed.iter().take(tool::MOST_LOGS).enumerate() {
                api::progress(call, &format!("reading the log of {}", j.name), Some(0.3 + 0.6 * i as f32 / failed.len().max(1) as f32));
                logs.insert(j.id, client.log(j.id, &j.name).await.map(|(text, _)| text));
            }
            let report = tool::Report { project: &repo.project, forge: repo.provider.label(), branch: &branch, head: head.as_deref(), summary: &summary, current: &current, jobs: &snap.jobs, logs: &logs, lines }.text();
            api::answer(call, Ok(report));
        });
        None
    }
}

fn show(note: &notify::Note) {
    let urgency = match note.loudness {
        notify::Loudness::Low => Urgency::Low,
        notify::Loudness::Normal => Urgency::Normal,
        notify::Loudness::Critical => Urgency::Critical,
    };
    let mut n = Notification::new(note.summary.clone()).body(note.body.clone()).urgency(urgency).tag(note.tag.clone());
    for (label, command) in &note.actions {
        n = n.action(label.clone(), command.clone());
    }
    n.show();
}

impl Extension for Ci {
    fn activate(&mut self) -> Result<(), String> {
        self.timer = Some(host::set_timer(TICK_MS, true));
        self.tick();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "ci.open" => self.open_page(),
            "ci.refresh" => {
                self.tick();
                self.poller.now(now_ms());
                self.poll_if_due();
            }
            "ci.failed_log" => match self.failed_job() {
                Some((run, job)) => self.act(Action::Log { run, job }),
                None => api::notify("Nothing has failed in this branch's CI."),
            },
            "ci.browser" => {
                let (current, _) = summary::current(&self.data.runs, self.data.head.as_deref());
                match current.first() {
                    Some(run) => self.act(Action::Browser(run.url.clone())),
                    None => api::notify("This branch has no CI runs yet."),
                }
            }
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let event = proc::deliver(event).and_then(run::deliver);
        match event {
            Some(Event::Timer(id)) if Some(id) == self.timer => self.tick(),
            Some(Event::SettingChanged(_)) => {
                // A token, a server or the provider may have changed: what
                // it knew of the forge goes, and it asks again.
                TOKENS.with(|t| t.borrow_mut().clear());
                self.remote = None;
                self.data.error = None;
                self.resolve();
                self.poller.now(now_ms());
            }
            Some(Event::PageShown((id, true))) if self.pages.contains_key(&id) && self.data.checked.is_none_or(|c| now_s() - c > 15) => {
                self.poller.now(now_ms());
                self.poll_if_due();
            }
            _ => {}
        }
        let landed: Vec<Done> = DONE.with(|q| std::mem::take(&mut *q.borrow_mut()));
        for d in landed {
            self.apply(d);
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let data = &self.data;
        let p = self.pages.entry(page).or_default();
        Page::canvas(p.layout(data, width as usize, height as usize, now_s()))
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key, &self.data);
        self.act(action);
        let landed: Vec<Done> = DONE.with(|q| std::mem::take(&mut *q.borrow_mut()));
        for d in landed {
            self.apply(d);
        }
        pages::refresh(page);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.pages.contains_key(&page).then(|| "{}".to_string())
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind != PAGE_KIND {
            return false;
        }
        self.pages.insert(page, CiPage::default());
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        match name {
            "ci.failures" => self.failures(call, payload),
            _ => Some(Err(format!("no service {name}"))),
        }
    }
}

export!(Ci);
