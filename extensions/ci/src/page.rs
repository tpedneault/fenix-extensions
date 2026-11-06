//! The CI page (`SPC g C`): the branch's latest runs, each opening up to
//! its jobs, and a job to its steps -- with their state and how long they
//! took. `Enter` on a job opens its log; `r` re-runs what failed, `c`
//! cancels, `o` opens it in the browser, `R` asks again.

use std::collections::{BTreeSet, HashMap};

use fenix_extension_api::grid::{fit, frame, Grid, Popup};
use fenix_extension_api::{Canvas, Key, Role};

use crate::model::{ago, duration, short, took, Job, Provider, Run, State};
use crate::summary::{summarise, Summary};

/// What the page shows: the extension's, read when it's laid out.
#[derive(Debug, Clone, Default)]
pub struct Data {
    /// `acme/rocket`, or why there's no CI to show.
    pub project: Option<String>,
    pub provider: Option<Provider>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub runs: Vec<Run>,
    pub jobs: HashMap<u64, Vec<Job>>,
    /// Runs whose jobs are being read.
    pub reading: BTreeSet<u64>,
    /// Why the last ask failed.
    pub error: Option<String>,
    /// When it last heard from the forge (s since the epoch).
    pub checked: Option<i64>,
    /// An ask is out.
    pub loading: bool,
    /// The modeline's summary.
    pub summary: Option<Summary>,
}

/// A row, by what it shows: kept across a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKey {
    Run(u64),
    Job(u64, u64),
    Step(u64, u64, u32),
    /// A run's "reading its jobs…" or "no jobs".
    Note(u64),
}

/// What a key asks the extension to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Refresh,
    /// Read a run's jobs.
    Jobs(u64),
    Log { run: u64, job: u64 },
    /// Re-run a run's failed jobs, or one job.
    Rerun { run: u64, job: Option<u64> },
    Cancel { run: u64, job: Option<u64> },
    Browser(String),
}

/// A question the page is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Confirm {
    question: String,
    then: Action,
}

#[derive(Debug, Clone, Default)]
pub struct CiPage {
    focus: Option<RowKey>,
    open_runs: BTreeSet<u64>,
    open_jobs: BTreeSet<u64>,
    scroll: usize,
    confirm: Option<Confirm>,
    /// Something to say: (text, whether it's an error).
    pub message: Option<(String, bool)>,
}

pub fn state_role(state: State) -> Role {
    match state {
        State::Success => Role::Good,
        State::Failed => Role::Bad,
        State::Running | State::Manual => Role::Accent,
        State::Queued => Role::Warn,
        State::Cancelled | State::Skipped | State::Neutral => Role::Muted,
    }
}

impl CiPage {
    /// The runs whose jobs it shows.
    pub fn open_runs(&self) -> impl Iterator<Item = u64> + '_ {
        self.open_runs.iter().copied()
    }

    /// Opens the runs that count, the first time there are some: their
    /// failed jobs are what you came for.
    pub fn open_first(&mut self, data: &Data) {
        if self.focus.is_none() && self.open_runs.is_empty() {
            if let Some(run) = data.runs.first() {
                self.open_runs.insert(run.id);
            }
        }
    }

    fn rows(&self, data: &Data) -> Vec<RowKey> {
        let mut rows = Vec::new();
        for run in &data.runs {
            rows.push(RowKey::Run(run.id));
            if !self.open_runs.contains(&run.id) {
                continue;
            }
            match data.jobs.get(&run.id).filter(|j| !j.is_empty()) {
                Some(jobs) => {
                    for job in jobs {
                        rows.push(RowKey::Job(run.id, job.id));
                        if self.open_jobs.contains(&job.id) {
                            rows.extend(job.steps.iter().map(|s| RowKey::Step(run.id, job.id, s.number)));
                        }
                    }
                }
                None => rows.push(RowKey::Note(run.id)),
            }
        }
        rows
    }

    fn cursor(&self, rows: &[RowKey]) -> usize {
        let Some(focus) = self.focus else { return 0 };
        if let Some(i) = rows.iter().position(|r| *r == focus) {
            return i;
        }
        // Gone (a step's job closed, a run aged off): its parent.
        let parent = match focus {
            RowKey::Step(r, j, _) => Some(RowKey::Job(r, j)),
            RowKey::Job(r, _) | RowKey::Note(r) => Some(RowKey::Run(r)),
            RowKey::Run(_) => None,
        };
        parent.and_then(|p| rows.iter().position(|r| *r == p).or_else(|| match p {
            RowKey::Job(r, _) => rows.iter().position(|x| *x == RowKey::Run(r)),
            _ => None,
        })).unwrap_or(0)
    }

    /// The row under the cursor.
    #[cfg(test)]
    pub fn focused(&self, data: &Data) -> Option<RowKey> {
        let rows = self.rows(data);
        rows.get(self.cursor(&rows)).copied()
    }

    pub fn key(&mut self, key: Key, data: &Data) -> Action {
        if let Some(confirm) = self.confirm.take() {
            return if key == Key::Char('y') { confirm.then } else { Action::None };
        }
        self.message = None;
        let rows = self.rows(data);
        let at = self.cursor(&rows);
        let row = rows.get(at).copied();
        let mut go = |i: usize| self.focus = rows.get(i.min(rows.len().saturating_sub(1))).copied();
        let run = |id: u64| data.runs.iter().find(|r| r.id == id);
        let job = |run: u64, id: u64| data.jobs.get(&run).and_then(|l| l.iter().find(|j| j.id == id));
        match key {
            Key::Down | Key::Char('j') => go(at + 1),
            Key::Up | Key::Char('k') => go(at.saturating_sub(1)),
            Key::Char('g') => go(0),
            Key::Char('G') => go(usize::MAX),
            Key::Char('R') | Key::Char('u') => return Action::Refresh,
            Key::Enter | Key::Right | Key::Tab | Key::Char('l') => match row {
                Some(RowKey::Run(id)) => {
                    self.focus = row;
                    if key == Key::Enter && self.open_runs.contains(&id) {
                        self.open_runs.remove(&id);
                    } else {
                        self.open_runs.insert(id);
                        if !data.jobs.contains_key(&id) {
                            return Action::Jobs(id);
                        }
                    }
                }
                Some(RowKey::Job(r, j)) | Some(RowKey::Step(r, j, _)) => {
                    self.focus = row;
                    let has_steps = job(r, j).is_some_and(|j| !j.steps.is_empty());
                    if key == Key::Enter || !has_steps {
                        return Action::Log { run: r, job: j };
                    }
                    self.open_jobs.insert(j);
                }
                Some(RowKey::Note(id)) => return Action::Jobs(id),
                None => {}
            },
            Key::Left | Key::Char('h') | Key::BackTab => match row {
                Some(RowKey::Run(id)) => {
                    self.open_runs.remove(&id);
                }
                Some(RowKey::Job(r, j)) => {
                    if !self.open_jobs.remove(&j) {
                        self.open_runs.remove(&r);
                        self.focus = Some(RowKey::Run(r));
                    }
                }
                Some(RowKey::Step(_, j, _)) => {
                    self.open_jobs.remove(&j);
                    self.focus = row.map(|r| match r {
                        RowKey::Step(r, j, _) => RowKey::Job(r, j),
                        other => other,
                    });
                }
                Some(RowKey::Note(r)) => {
                    self.open_runs.remove(&r);
                    self.focus = Some(RowKey::Run(r));
                }
                None => {}
            },
            Key::Char('r') => match row {
                Some(RowKey::Run(id)) | Some(RowKey::Note(id)) => return Action::Rerun { run: id, job: None },
                Some(RowKey::Job(r, j)) | Some(RowKey::Step(r, j, _)) => return Action::Rerun { run: r, job: Some(j) },
                None => {}
            },
            Key::Char('c') => {
                let (run_id, job_id) = match row {
                    Some(RowKey::Run(id)) | Some(RowKey::Note(id)) => (id, None),
                    // GitHub cancels a whole run; GitLab a job.
                    Some(RowKey::Job(r, j)) | Some(RowKey::Step(r, j, _)) => (r, (data.provider == Some(Provider::GitLab)).then_some(j)),
                    None => return Action::None,
                };
                let Some(r) = run(run_id) else { return Action::None };
                if r.state.done() && job_id.is_none_or(|j| job(run_id, j).is_some_and(|j| j.state.done())) {
                    self.message = Some(("It has finished: there's nothing to cancel.".into(), false));
                    return Action::None;
                }
                let question = match job_id.and_then(|j| job(run_id, j)) {
                    Some(j) => format!("Cancel the job {}? ", j.name),
                    None => format!("Cancel {} #{}? ", r.name, r.number),
                };
                self.confirm = Some(Confirm { question, then: Action::Cancel { run: run_id, job: job_id } });
            }
            Key::Char('o') => {
                let url = match row {
                    Some(RowKey::Job(r, j)) | Some(RowKey::Step(r, j, _)) => job(r, j).map(|j| j.url.clone()),
                    Some(RowKey::Run(r)) | Some(RowKey::Note(r)) => run(r).map(|r| r.url.clone()),
                    None => None,
                };
                if let Some(url) = url.filter(|u| !u.is_empty()) {
                    return Action::Browser(url);
                }
            }
            _ => {}
        }
        Action::None
    }

    /// The page, `cols` wide and `height` tall, at `now` (s since the epoch).
    pub fn layout(&mut self, data: &Data, cols: usize, height: usize, now: i64) -> Canvas {
        let (left, width) = frame(cols, 140);
        let right = left + width;
        let mut g = Grid::new();
        let mut y = 1;
        let title = match &data.project {
            Some(p) => format!("CI · {p}"),
            None => "CI".to_string(),
        };
        g.put(y, left, &fit(&title, width.saturating_sub(20)), Role::Title);
        if let Some(p) = data.provider {
            g.put(y, right.saturating_sub(p.label().chars().count()), p.label(), Role::Muted);
        }
        y += 1;
        let mut x = left;
        if let Some(b) = &data.branch {
            x = g.put(y, x, b, Role::Accent);
        }
        if let Some(h) = &data.head {
            x = g.put(y, x, " · HEAD ", Role::Muted);
            x = g.put(y, x, short(h), Role::Text);
        }
        if let Some(s) = &data.summary {
            x = g.put(y, x, " · ", Role::Muted);
            let role = match s.overall {
                crate::summary::Overall::Success => Role::Good,
                crate::summary::Overall::Failed => Role::Bad,
                crate::summary::Overall::Running | crate::summary::Overall::Queued => Role::Accent,
                _ => Role::Muted,
            };
            g.put(y, x, &fit(&s.sentence(), right.saturating_sub(x + 18)), role);
        }
        let when = if data.loading {
            "checking…".to_string()
        } else {
            data.checked.map(|c| format!("checked {}", ago(c, now))).unwrap_or_default()
        };
        g.put(y, right.saturating_sub(when.chars().count()), &when, Role::Muted);
        y += 1;
        let said = self.message.clone().or_else(|| data.error.clone().map(|e| (e, true)));
        if let Some((text, bad)) = said {
            for line in fenix_extension_api::grid::wrap(&text, width).into_iter().take(3) {
                g.put(y, left, &line, if bad { Role::Bad } else { Role::Muted });
                y += 1;
            }
        }
        y += 1;
        if data.project.is_none() || (data.runs.is_empty() && data.checked.is_none()) {
            let text = if data.project.is_none() { "" } else { "Reading the runs…" };
            g.put(y, left, text, Role::Muted);
            g.keys(left, width, &[("R", "refresh"), ("q", "close")]);
            return g.finish();
        }
        let word = data.provider.map(Provider::run_word).unwrap_or("run");
        g.heading(y, left, width, &format!("{}s · {}", word, data.runs.len()));
        y += 1;
        if data.runs.is_empty() {
            g.put(y, left, &format!("No {word}s for {} yet: push it, and they show here.", data.branch.as_deref().unwrap_or("this branch")), Role::Muted);
            y += 1;
        }
        let rows = self.rows(data);
        let at = self.cursor(&rows);
        // What fits between the header and the key strip.
        let room = height.saturating_sub(y + 3).max(3);
        if at < self.scroll {
            self.scroll = at;
        } else if at >= self.scroll + room {
            self.scroll = at + 1 - room;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(room));
        let head = data.head.as_deref().unwrap_or("");
        for (i, row) in rows.iter().enumerate().skip(self.scroll).take(room) {
            match *row {
                RowKey::Run(id) => {
                    let Some(run) = data.runs.iter().find(|r| r.id == id) else { continue };
                    let open = self.open_runs.contains(&id);
                    let x = g.put(y, left, if open { "▾ " } else { "▸ " }, Role::Muted);
                    let x = g.put(y, x, run.state.icon(), state_role(run.state)) + 1;
                    let mut name = format!("{} #{}", run.name, run.number);
                    if run.attempt > 1 {
                        name.push_str(&format!(" (attempt {})", run.attempt));
                    }
                    let x = g.put(y, x, &name, Role::Text) + 2;
                    let mut tail: Vec<String> = Vec::new();
                    if !run.event.is_empty() {
                        tail.push(run.event.clone());
                    }
                    tail.push(short(&run.sha).to_string());
                    if let Some(t) = took(run.started, run.finished, now) {
                        tail.push(duration(t));
                    }
                    if let Some(at) = run.created {
                        tail.push(ago(at, now));
                    }
                    let tail = tail.join(" · ");
                    let tail_x = right.saturating_sub(tail.chars().count());
                    g.put(y, x, &fit(&run.title, tail_x.saturating_sub(x + 2)), Role::Muted);
                    let is_head = !head.is_empty() && run.sha.starts_with(head);
                    g.put(y, tail_x, &tail, if is_head { Role::Text } else { Role::Muted });
                }
                RowKey::Job(r, j) => {
                    let Some(job) = data.jobs.get(&r).and_then(|l| l.iter().find(|x| x.id == j)) else { continue };
                    let open = self.open_jobs.contains(&j);
                    let marker = if job.steps.is_empty() { "  " } else if open { "▾ " } else { "▸ " };
                    let x = g.put(y, left + 4, marker, Role::Muted);
                    let x = g.put(y, x, job.state.icon(), state_role(job.state)) + 1;
                    let mut tail = String::new();
                    if !job.stage.is_empty() {
                        tail.push_str(&job.stage);
                    }
                    if job.allow_failure && job.state == State::Failed {
                        tail.push_str(if tail.is_empty() { "allowed to fail" } else { " · allowed to fail" });
                    }
                    if let Some(t) = took(job.started, job.finished, now) {
                        if !tail.is_empty() {
                            tail.push_str(" · ");
                        }
                        tail.push_str(&duration(t));
                    } else if !job.state.done() {
                        if !tail.is_empty() {
                            tail.push_str(" · ");
                        }
                        tail.push_str(job.state.label());
                    }
                    let tail_x = right.saturating_sub(tail.chars().count());
                    g.put(y, x, &fit(&job.name, tail_x.saturating_sub(x + 2)), if job.failed() { Role::Bad } else { Role::Text });
                    g.put(y, tail_x, &tail, Role::Muted);
                }
                RowKey::Step(r, j, n) => {
                    let Some(step) = data.jobs.get(&r).and_then(|l| l.iter().find(|x| x.id == j)).and_then(|job| job.steps.iter().find(|s| s.number == n)) else { continue };
                    let x = g.put(y, left + 10, step.state.icon(), state_role(step.state)) + 1;
                    let x = g.put(y, x, &format!("{:>2} ", step.number), Role::Muted);
                    let tail = took(step.started, step.finished, now).filter(|_| step.state != State::Skipped).map(duration).unwrap_or_else(|| if step.state.done() { String::new() } else { step.state.label().to_string() });
                    let tail_x = right.saturating_sub(tail.chars().count());
                    g.put(y, x, &fit(&step.name, tail_x.saturating_sub(x + 2)), if step.state == State::Failed { Role::Bad } else { Role::Text });
                    g.put(y, tail_x, &tail, Role::Muted);
                }
                RowKey::Note(r) => {
                    let text = if data.reading.contains(&r) || !data.jobs.contains_key(&r) { "reading its jobs…" } else { "no jobs" };
                    g.put(y, left + 6, text, Role::Muted);
                }
            }
            if i == at {
                g.focus(y, left..right);
                if let Some(c) = &self.confirm {
                    g.popup = Some(Popup { line: y, col: left + 4, rows: vec![vec![(c.question.clone(), Role::Text), ("y".into(), Role::Accent), (" yes   ".into(), Role::Muted), ("n".into(), Role::Accent), (" no".into(), Role::Muted)]] });
                }
            }
            y += 1;
        }
        if rows.len() > self.scroll + room {
            g.put(y, left, &format!("… {} more", rows.len() - self.scroll - room), Role::Muted);
        }
        let on_job = matches!(rows.get(at), Some(RowKey::Job(..) | RowKey::Step(..)));
        let keys: &[(&str, &str)] = if on_job {
            &[("Enter", "log"), ("l", "steps"), ("h", "close"), ("r", "re-run job"), ("c", "cancel"), ("o", "browser"), ("R", "refresh"), ("q", "close")]
        } else {
            &[("Enter", "jobs"), ("j k", "move"), ("r", "re-run failed"), ("c", "cancel"), ("o", "browser"), ("R", "refresh"), ("q", "close")]
        };
        g.keys(left, width, keys);
        g.finish()
    }
}

/// The summary of what `data` holds, for the modeline and the header.
pub fn summary_of(data: &Data) -> Summary {
    let (current, earlier) = crate::summary::current(&data.runs, data.head.as_deref());
    summarise(&current, &data.jobs, earlier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{github, gitlab, model::parse_time};

    const HEAD: &str = "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10";

    fn now() -> i64 {
        parse_time("2026-11-06T09:44:31Z").unwrap()
    }

    fn data() -> Data {
        let runs = github::runs(&serde_json::from_str(include_str!("samples/github_runs.json")).unwrap());
        let mut jobs = HashMap::new();
        jobs.insert(runs[0].id, github::jobs(&serde_json::from_str(include_str!("samples/github_jobs.json")).unwrap()));
        let mut d = Data { project: Some("acme/rocket".into()), provider: Some(Provider::GitHub), branch: Some("feature/telemetry".into()), head: Some(HEAD.into()), runs, jobs, checked: Some(now() - 12), ..Data::default() };
        d.summary = Some(summary_of(&d));
        d
    }

    #[test]
    fn the_runs_are_listed_and_the_first_opens_on_its_jobs() {
        let d = data();
        let mut p = CiPage::default();
        p.open_first(&d);
        let text = p.layout(&d, 120, 40, now()).text;
        assert!(text.contains("CI · acme/rocket") && text.contains("GitHub Actions"), "{text}");
        assert!(text.contains("feature/telemetry · HEAD 9f1c2e7 · Running: 3 of 5 jobs done, 1 failed") && text.contains("checked just now"), "{text}");
        assert!(text.contains("RUNS · 3"), "{text}");
        assert!(text.contains("▾ ● CI #482  Decode PUS-17 replies") && text.contains("push · 9f1c2e7 · 3m 24s · 3m ago"), "{text}");
        assert!(text.contains("▸ ✓ build") && text.contains("1m 48s"), "{text}");
        assert!(text.contains("▸ ✗ test (ubuntu-latest)") && text.contains("3m 12s"), "{text}");
        assert!(text.contains("▸ ● test (windows-latest)") && text.contains("2m 51s"), "{text}");
        assert!(text.contains("○ docs") && text.contains("queued"), "{text}");
        assert!(text.contains("▸ ✓ Lint #301") && text.contains("▸ ✗ CI #479 (attempt 2)  Read the MIB's PCF table"), "{text}");
        assert!(text.contains("Enter jobs") && text.contains("r re-run failed"), "{text}");
    }

    #[test]
    fn a_job_opens_on_its_steps_and_enter_asks_for_its_log() {
        let d = data();
        let mut p = CiPage::default();
        p.open_first(&d);
        assert_eq!(p.key(Key::Char('j'), &d), Action::None);
        assert_eq!(p.key(Key::Char('j'), &d), Action::None);
        assert_eq!(p.focused(&d), Some(RowKey::Job(11807430219, 32897410321)));
        assert_eq!(p.key(Key::Char('l'), &d), Action::None);
        let text = p.layout(&d, 120, 40, now()).text;
        assert!(text.contains("▾ ✗ test (ubuntu-latest)"), "{text}");
        assert!(text.contains("✗  3 Run cargo test --workspace") && text.contains("3m 06s"), "{text}");
        assert!(text.contains("–  4 Upload the test report"), "{text}");
        assert!(text.contains("Enter log") && text.contains("r re-run job"), "{text}");
        assert_eq!(p.key(Key::Enter, &d), Action::Log { run: 11807430219, job: 32897410321 });
        p.key(Key::Char('j'), &d);
        assert_eq!(p.focused(&d), Some(RowKey::Step(11807430219, 32897410321, 1)));
        assert_eq!(p.key(Key::Enter, &d), Action::Log { run: 11807430219, job: 32897410321 }, "a step's log is its job's");
        assert_eq!(p.key(Key::Char('h'), &d), Action::None);
        assert_eq!(p.focused(&d), Some(RowKey::Job(11807430219, 32897410321)), "back to the job, closed");
        assert!(!p.layout(&d, 120, 40, now()).text.contains("Upload the test report"));
    }

    #[test]
    fn a_closed_run_asks_for_its_jobs_when_opened() {
        let d = data();
        let mut p = CiPage::default();
        p.open_first(&d);
        p.key(Key::Char('G'), &d);
        assert_eq!(p.focused(&d), Some(RowKey::Run(11799214410)));
        assert_eq!(p.key(Key::Enter, &d), Action::Jobs(11799214410));
        assert!(p.layout(&d, 120, 40, now()).text.contains("reading its jobs…"));
        assert_eq!(p.key(Key::Char('r'), &d), Action::Rerun { run: 11799214410, job: None });
        assert_eq!(p.key(Key::Char('o'), &d), Action::Browser("https://github.com/acme/rocket/actions/runs/11799214410".into()));
        assert_eq!(p.key(Key::Char('R'), &d), Action::Refresh);
    }

    #[test]
    fn cancelling_asks_first() {
        let d = data();
        let mut p = CiPage::default();
        p.open_first(&d);
        assert_eq!(p.key(Key::Char('c'), &d), Action::None);
        let page = p.layout(&d, 120, 40, now());
        assert_eq!(page.popup.as_ref().map(|p| p.text()), Some("Cancel CI #482? y yes   n no".to_string()));
        assert_eq!(p.key(Key::Char('y'), &d), Action::Cancel { run: 11807430219, job: None });
        p.key(Key::Char('c'), &d);
        assert_eq!(p.key(Key::Char('n'), &d), Action::None);
        assert!(p.layout(&d, 120, 40, now()).popup.is_none());
        // A finished run has nothing to cancel.
        p.key(Key::Char('G'), &d);
        assert_eq!(p.key(Key::Char('c'), &d), Action::None);
        assert!(p.layout(&d, 120, 40, now()).text.contains("It has finished: there's nothing to cancel."));
    }

    #[test]
    fn gitlab_cancels_and_retries_one_job_and_shows_its_stages() {
        let runs = gitlab::pipelines(&serde_json::from_str(include_str!("samples/gitlab_pipelines.json")).unwrap());
        let mut jobs = HashMap::new();
        jobs.insert(runs[0].id, gitlab::jobs(&serde_json::from_str(include_str!("samples/gitlab_jobs.json")).unwrap()));
        let mut d = Data { project: Some("space/ground/rocket".into()), provider: Some(Provider::GitLab), branch: Some("feature/telemetry".into()), head: Some("c0ffee1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b".into()), runs, jobs, checked: Some(now()), ..Data::default() };
        d.summary = Some(summary_of(&d));
        let mut p = CiPage::default();
        p.open_first(&d);
        let text = p.layout(&d, 120, 40, now()).text;
        assert!(text.contains("PIPELINES · 3") && text.contains("GitLab pipelines"), "{text}");
        assert!(text.contains("Failed: 1 job"), "{text}");
        assert!(text.contains("✗ Pipeline #2214") && text.contains("✗ lint: markdown") && text.contains("test · allowed to fail · 21s"), "{text}");
        assert!(text.contains("✗ unit tests") && text.contains("test · 6m 53s"), "{text}");
        assert!(text.contains("▷ deploy: staging") && text.contains("deploy"), "{text}");
        p.key(Key::Char('j'), &d);
        p.key(Key::Char('j'), &d);
        p.key(Key::Char('j'), &d);
        assert_eq!(p.focused(&d), Some(RowKey::Job(1468213077, 8312904417)));
        assert_eq!(p.key(Key::Char('l'), &d), Action::Log { run: 1468213077, job: 8312904417 }, "no steps: its log");
        assert_eq!(p.key(Key::Char('r'), &d), Action::Rerun { run: 1468213077, job: Some(8312904417) });
    }

    #[test]
    fn a_long_list_scrolls_with_the_cursor() {
        let mut d = data();
        let template = d.runs[2].clone();
        for i in 0..30 {
            let mut r = template.clone();
            r.id = 100 + i;
            r.number = 400 - i;
            d.runs.push(r);
        }
        let mut p = CiPage::default();
        let text = p.layout(&d, 120, 20, now()).text;
        assert!(text.contains("… ") && text.contains("more"), "{text}");
        p.key(Key::Char('G'), &d);
        let text = p.layout(&d, 120, 20, now()).text;
        assert!(text.contains("CI #371") && !text.contains("CI #482"), "{text}");
    }

    #[test]
    fn with_no_runs_it_says_so() {
        let d = Data { project: Some("acme/rocket".into()), provider: Some(Provider::GitHub), branch: Some("new".into()), checked: Some(now()), summary: Some(summary_of(&Data::default())), ..Data::default() };
        let text = CiPage::default().layout(&d, 100, 30, now()).text;
        assert!(text.contains("No runs for new yet: push it, and they show here."), "{text}");
        assert!(text.contains("No runs for this branch yet"), "{text}");
        let none = Data { error: Some("This project has no remote to find its CI by.".into()), ..Data::default() };
        assert!(CiPage::default().layout(&none, 100, 30, now()).text.contains("no remote"));
    }
}
