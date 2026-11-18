//! The Kiln project in the open Fenix project: whether kiln is there and
//! new enough, and the project as kiln last read it -- its items (`kiln
//! list`), its plan (`kiln status`) and its findings (`kiln check`) --
//! read again when something changes.
//!
//! # States
//!
//! ```text
//! idle --start/reload--> probing (kiln --version, at least 1.0.0)
//!   probing --> not found | not kiln | too old           (r probes again)
//!   probing --> loading (list, status, check) --> loaded | no kiln.toml | failed
//!   loaded --reload--> loaded, reloading --> loaded (new) | loaded with the error
//! ```
//!
//! [`Project::state`] says which, for a page to show. While a reload runs,
//! the last loaded project stays what's shown ([`State::Loaded`] with
//! `reloading`); a reload that fails keeps it too, with its error beside
//! it, so a typo in an item file mid-edit doesn't blank the pages.
//! `list` failing fails the reload; `status` or `check` failing on its own
//! leaves its part an `Err` with kiln's line (a project without a
//! `part-of` role has no plan, and `status` exits 2 there) and the rest
//! loaded. A `status` in a `kiln_json` shape this doesn't read is another
//! kiln's: the reload fails with that reason and what was loaded goes, so
//! no page shows one kiln's items beside another's guesses.
//!
//! # Reloads
//!
//! At most one runs, and at most one more waits: a request while one runs
//! marks it to run once more when it ends, however many come (they
//! coalesce). What asks for one ([`Why`]): `kiln.reload` or `r`
//! ([`Project::reload`]), a file saved ([`Project::saved`]), a new git
//! head or branch ([`Project::timer`], polling `git_state` every
//! [`GIT_POLL_MS`] as the API allows), a kiln edit ending
//! ([`Project::edit`], or [`Project::edited`] for an edit run elsewhere),
//! and the refresh timer, every `kiln.refresh_seconds` while one of the
//! extension's pages is in view ([`Project::set_shown`]). A timer's
//! reload is skipped, not queued, when one runs: it would only repeat it.
//!
//! Nothing is asked of kiln until something wants the project
//! ([`Project::start`]: the extension loading, since Kiln's language
//! server is decided from it, or a page opening): an idle project ignores
//! saves, git and timers.
//!
//! # Where kiln runs
//!
//! With no `cwd`, which Fenix takes as the open project's folder: its top,
//! where the `kiln.toml` marker that activated the extension is. kiln
//! walks up from there to the nearest `kiln.toml`, so a project with none
//! at its top but one above reads that one, as kiln in a terminal there
//! would. "No kiln.toml" is kiln's own word for it (exit 2, `kiln: no
//! kiln.toml in …`), read from its line, not from the filesystem.
//!
//! # For later pages
//!
//! - [`Project::query`]: any other reading command (`show`, `trace`,
//!   `impact`, `diff`, `export --dry-run`), its answer a [`Notice::Answer`]
//!   with the id it returned;
//! - [`Project::edit`]: an editing command (`set`, `link`, `new`...), its
//!   answer the same way, then a reload;
//! - [`Project::take_notices`] after every call into it: what changed.
//!
//! What it deliberately doesn't do: draw anything, read files itself (kiln
//! does), or run two kilns at once ([`crate::kiln`]).

use std::collections::HashMap;

use crate::api;
use crate::kiln::{Failure, Finished, Job, Kiln, Ran};
use crate::model::{self, Counts, Finding, Group, Item, Status, Version};

/// How often git's head is looked at: every 2 s, as often as Fenix reads
/// it again (`git_state`). A host call, no process -- cheap enough to run
/// while the project is in use, so a checkout or a commit made in a
/// terminal reloads even with no page in view.
pub const GIT_POLL_MS: u32 = 2_000;

/// `kiln.refresh_seconds` when it's unset or not a number.
pub const DEFAULT_REFRESH_SECONDS: u32 = 5;

/// What asked for a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// `kiln.reload`, `r`: also probes kiln again when it wasn't usable,
    /// and starts an idle project.
    Asked,
    /// A file in the project was saved.
    Saved,
    /// The repository's head or branch moved.
    Git,
    /// A kiln edit ended.
    Edit,
    /// The refresh timer, or a page coming into view: skipped when a
    /// reload runs.
    Timer,
}

/// An id for a query's or an edit's answer.
pub type QueryId = u64;

/// What happened, for `lib.rs` to pass on.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// [`Project::state`] changed: the pages showing it are drawn again.
    Changed,
    /// A query's or an edit's answer: kiln's output, or why there's none.
    /// What an exit code means is the caller's: `kiln set` refusing a file
    /// changed on disk exits 1 with its line ([`Ran::expect`]).
    Answer {
        id: QueryId,
        line: String,
        result: Result<Ran, Failure>,
    },
}

/// The project as kiln last read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The kiln that read it.
    pub version: Version,
    /// Every item, in kiln's order (type order, then ID).
    pub items: Vec<Item>,
    /// The plan by milestone, or why `kiln status` couldn't give it.
    pub plan: Result<Status, String>,
    /// `kiln check`'s findings, or why it couldn't give them.
    pub findings: Result<Vec<Finding>, String>,
    by_id: HashMap<String, usize>,
}

impl Loaded {
    pub fn new(
        version: Version,
        items: Vec<Item>,
        plan: Result<Status, String>,
        findings: Result<Vec<Finding>, String>,
    ) -> Loaded {
        let mut by_id = HashMap::new();
        for (at, item) in items.iter().enumerate() {
            // A duplicated ID (K101) finds its first file; check names both.
            by_id.entry(item.id.clone()).or_insert(at);
        }
        Loaded {
            version,
            items,
            plan,
            findings,
            by_id,
        }
    }

    /// The item with this ID (its first file, when it's duplicated).
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.by_id.get(id).map(|&at| &self.items[at])
    }

    /// Items whose `parent` is `id`, in kiln's order.
    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items
            .iter()
            .filter(move |i| i.parent.as_deref() == Some(id))
    }

    /// The project's name (`kiln.toml`'s `name`), from `status`.
    pub fn name(&self) -> Option<&str> {
        self.plan.as_ref().ok().map(|s| s.project.as_str())
    }

    /// The plan's groups: each open milestone (or one with open work) with
    /// its progress and its work in progress, ready and waiting; the work
    /// in no milestone last. Empty without a plan.
    pub fn groups(&self) -> &[Group] {
        self.plan
            .as_ref()
            .map(|s| s.milestones.as_slice())
            .unwrap_or(&[])
    }

    /// One milestone's group by its ID, or the work in no milestone for
    /// `None`.
    pub fn group(&self, milestone: Option<&str>) -> Option<&Group> {
        self.groups()
            .iter()
            .find(|g| g.milestone.as_ref().map(|m| m.id.as_str()) == milestone)
    }

    /// Errors and warnings: `check`'s, else `status`'s count of them, else
    /// none known.
    pub fn counts(&self) -> Option<Counts> {
        match (&self.findings, &self.plan) {
            (Ok(f), _) => Some(model::counts(f)),
            (Err(_), Ok(s)) => Some(s.findings),
            _ => None,
        }
    }
}

/// What a page shows: one of these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State<'a> {
    /// Nothing asked of kiln yet.
    Idle,
    /// `kiln --version` runs.
    Probing,
    /// No kiln to run: the line says why and how to give one (`kiln.path`).
    NotFound(&'a str),
    /// `kiln --version` failed, or what it said isn't kiln's version.
    NotKiln(&'a str),
    /// A kiln older than [`model::MIN_VERSION`]: the pages are off.
    TooOld(Version),
    /// kiln found no `kiln.toml`: its line.
    NoProject(&'a str),
    /// The first read runs; nothing to show yet.
    Loading,
    Loaded {
        loaded: &'a Loaded,
        /// A reload runs (or waits to).
        reloading: bool,
        /// The last reload failed: kiln's line. What's shown is from
        /// before it.
        error: Option<&'a str>,
    },
    /// The first read failed: kiln's line.
    Failed(&'a str),
}

#[derive(Debug, Clone, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    Probing,
    NotFound(String),
    NotKiln(String),
    TooOld(Version),
    Ready(Version),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    List,
    Status,
    Check,
}

/// What a kiln job is for. `generation` drops answers from before another
/// kiln was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Version { generation: u64 },
    Load { generation: u64, step: Step },
    Query { id: QueryId, edit: bool },
}

/// A reload under way: what it has read so far.
#[derive(Debug, Default)]
struct Reading {
    items: Option<Vec<Item>>,
    plan: Option<Result<Status, String>>,
    /// How many edits had finished when it began: a read that loads
    /// shows them all.
    covers: u64,
}

#[derive(Debug, Default)]
pub struct Project {
    kiln: Kiln<Tag>,
    generation: u64,
    phase: Phase,
    loaded: Option<Loaded>,
    /// Why the last reload failed, while it's the latest word.
    error: Option<String>,
    /// kiln's "no kiln.toml" line, while it's the latest word.
    no_project: Option<String>,
    reading: Option<Reading>,
    /// Another reload asked for while one runs.
    again: bool,
    /// Queries asked before kiln was known to be usable.
    held: Vec<(QueryId, Job<Tag>)>,
    next_id: QueryId,
    shown: bool,
    refresh_timer: Option<u64>,
    git_timer: Option<u64>,
    /// The repository's (branch, head) when last looked at; `None` before
    /// the first look, `Some(None)` outside a repository.
    git: Option<Option<(Option<String>, Option<String>)>>,
    notices: Vec<Notice>,
    /// How many reads have loaded ([`Project::loads`]).
    loads: u64,
    /// How many edits have finished, and how many of those the last read
    /// that loaded shows ([`Project::settled`]).
    edits_done: u64,
    edits_read: u64,
}

impl Project {
    // -- What it is now ------------------------------------------------------

    pub fn state(&self) -> State<'_> {
        match &self.phase {
            Phase::Idle => State::Idle,
            Phase::Probing => State::Probing,
            Phase::NotFound(why) => State::NotFound(why),
            Phase::NotKiln(why) => State::NotKiln(why),
            Phase::TooOld(v) => State::TooOld(*v),
            Phase::Ready(_) => {
                if let Some(line) = &self.no_project {
                    return State::NoProject(line);
                }
                match (&self.loaded, &self.error) {
                    (Some(loaded), error) => State::Loaded {
                        loaded,
                        reloading: self.reading.is_some() || self.again,
                        error: error.as_deref(),
                    },
                    (None, Some(error)) if self.reading.is_none() => State::Failed(error),
                    (None, _) => State::Loading,
                }
            }
        }
    }

    /// The last project loaded, whatever's happened since: for what keeps
    /// working on the last good read (decorations, completions).
    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    /// The kiln in use, once it's known to be usable.
    pub fn version(&self) -> Option<Version> {
        match self.phase {
            Phase::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// How many reads have loaded a project so far: a page tells a new
    /// read from the one it built its rows from by this, so it builds them
    /// again only when there's a new one -- not on every [`Notice::Changed`],
    /// which a reload starting or failing brings too.
    pub fn loads(&self) -> u64 {
        self.loads
    }

    /// Whether what's loaded shows every edit asked so far: none waits
    /// or runs, and the last read that loaded began after the last one
    /// ended. An edit started while it isn't would be chosen from a read
    /// already known to be out of date (the item just moved, renamed or
    /// renumbered), so the edits wait for it. A reload for any other
    /// reason (the refresh timer, a save) doesn't unsettle it: what's
    /// shown is the latest known, and kiln checks every edit anyway.
    pub fn settled(&self) -> bool {
        let is_edit = |t: &Tag| matches!(t, Tag::Query { edit: true, .. });
        !self.kiln.any(is_edit)
            && !self.held.iter().any(|(_, j)| is_edit(&j.tag))
            && self.edits_read >= self.edits_done
    }

    /// Whether a reload runs or waits.
    pub fn reloading(&self) -> bool {
        self.reading.is_some() || self.again
    }

    /// What happened since last asked. Call after each call into it.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    // -- What asks it to act -------------------------------------------------

    /// Something wants the project (a page opened): kiln is looked for and
    /// the project read, if that hasn't happened; nothing, if it has.
    pub fn start(&mut self) {
        match self.phase {
            Phase::Idle => self.probe(),
            Phase::Ready(_)
                if self.loaded.is_none()
                    && self.reading.is_none()
                    && self.no_project.is_none()
                    && self.error.is_none() =>
            {
                self.begin_reading()
            }
            _ => {}
        }
        self.pump();
    }

    /// Read the project again, for `why`.
    pub fn reload(&mut self, why: Why) {
        match self.phase {
            Phase::Idle | Phase::NotFound(_) | Phase::NotKiln(_) | Phase::TooOld(_) => {
                // Only you start it, or try again: kiln may be installed
                // now. A save or a timer doesn't.
                if why == Why::Asked {
                    self.probe();
                }
            }
            // A read follows the probe anyway.
            Phase::Probing => {}
            Phase::Ready(_) => {
                if self.reading.is_some() {
                    if why != Why::Timer && !self.again {
                        self.again = true;
                        self.notices.push(Notice::Changed);
                    }
                } else {
                    self.begin_reading();
                }
            }
        }
        self.pump();
    }

    /// A file was saved (`file_saved`): the project is read again, once
    /// it's been read at all. Fenix tells an extension only of saves in
    /// its own project, so any is one of this project's; kiln reads code
    /// too (`[code] paths`), so a source file counts as much as an item's.
    pub fn saved(&mut self, _path: &str) {
        self.reload(Why::Saved);
    }

    /// A kiln edit ran some other way than [`Project::edit`] (a terminal,
    /// a task): read the project again.
    pub fn edited(&mut self) {
        self.reload(Why::Edit);
    }

    /// Whether one of the extension's pages is in view (`PageShown`): the
    /// refresh timer runs only while one is, and a page coming into view
    /// is read again (unless a reload runs).
    pub fn set_shown(&mut self, shown: bool) {
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        self.restart_refresh();
        if shown {
            self.reload(Why::Timer);
        }
    }

    /// `kiln.refresh_seconds` changed.
    pub fn refresh_changed(&mut self) {
        self.restart_refresh();
    }

    /// `kiln.path` changed: another kiln. What runs is stopped, waiting
    /// queries are answered that it changed, and kiln is looked for again
    /// if it had been; what was loaded goes, since another kiln may read
    /// the project differently.
    pub fn kiln_changed(&mut self) {
        let started = self.phase != Phase::Idle;
        self.generation += 1;
        for tag in self.kiln.clear() {
            if let Tag::Query { id, .. } = tag {
                self.answer(
                    id,
                    String::new(),
                    Err(Failure::Unusable(
                        "another kiln was chosen (kiln.path) before it answered".into(),
                    )),
                );
            }
        }
        self.reading = None;
        self.again = false;
        self.loaded = None;
        self.error = None;
        self.no_project = None;
        self.phase = Phase::Idle;
        self.stop_timers();
        self.git = None;
        if started {
            self.probe();
        } else {
            self.notices.push(Notice::Changed);
        }
        self.pump();
    }

    /// A reading command for a later page: `args` after `kiln` (`["show",
    /// "REQ-001", "--format", "json"]`). Its answer comes as a
    /// [`Notice::Answer`] with the id returned, even when kiln can't run
    /// (then its `Failure` says why). It waits behind what's queued, a
    /// reload included. Asked before kiln has been looked for, it starts
    /// that and runs once kiln is known to be usable.
    pub fn query<S: Into<String>>(&mut self, args: impl IntoIterator<Item = S>) -> QueryId {
        self.ask(Job::new(args, Tag::Query { id: 0, edit: false }))
    }

    /// An editing command (`["set", "TASK-004", "status=doing"]`), with
    /// text for its stdin when it reads one (`new … --body-file -`). Its
    /// answer comes as [`Notice::Answer`], and the project is then read
    /// again, whether kiln changed anything or refused (a refusal means
    /// what's shown may be stale).
    pub fn edit<S: Into<String>>(
        &mut self,
        args: impl IntoIterator<Item = S>,
        stdin: Option<String>,
    ) -> QueryId {
        let mut job = Job::new(args, Tag::Query { id: 0, edit: true });
        job.stdin = stdin;
        self.ask(job)
    }

    // -- Events, from lib.rs -------------------------------------------------

    /// A line of a process's output.
    pub fn output(&mut self, process: u64, stderr: bool, line: &str) {
        self.kiln.output(process, stderr, line);
        self.pump();
    }

    /// A process ended.
    pub fn exited(&mut self, process: u64, code: Option<i32>) {
        self.kiln.exited(process, code);
        self.pump();
    }

    /// A timer went off: kiln's watchdog, the refresh, or the git poll.
    /// Whether it was one of the project's.
    pub fn timer(&mut self, timer: u64) -> bool {
        let ours = if self.kiln.timer(timer) {
            true
        } else if Some(timer) == self.refresh_timer {
            self.reload(Why::Timer);
            true
        } else if Some(timer) == self.git_timer {
            self.look_at_git();
            true
        } else {
            false
        };
        self.pump();
        ours
    }

    // -- Inside --------------------------------------------------------------

    fn ask(&mut self, mut job: Job<Tag>) -> QueryId {
        self.next_id += 1;
        let id = self.next_id;
        if let Tag::Query { id: at, .. } = &mut job.tag {
            *at = id;
        }
        match &self.phase {
            Phase::Ready(_) => match &self.no_project {
                Some(line) => {
                    let line = line.clone();
                    self.answer(id, job.line(), Err(Failure::Unusable(line)));
                }
                None => self.kiln.push(job),
            },
            Phase::Idle | Phase::Probing => {
                self.held.push((id, job));
                if self.phase == Phase::Idle {
                    self.probe();
                }
            }
            Phase::NotFound(why) | Phase::NotKiln(why) => {
                let why = why.clone();
                self.answer(id, job.line(), Err(Failure::Unusable(why)));
            }
            Phase::TooOld(v) => {
                let why = too_old(*v);
                self.answer(id, job.line(), Err(Failure::Unusable(why)));
            }
        }
        self.pump();
        id
    }

    fn answer(&mut self, id: QueryId, line: String, result: Result<Ran, Failure>) {
        self.notices.push(Notice::Answer { id, line, result });
    }

    fn probe(&mut self) {
        self.phase = Phase::Probing;
        self.no_project = None;
        self.notices.push(Notice::Changed);
        self.kiln.push(Job::new(
            ["--version"],
            Tag::Version {
                generation: self.generation,
            },
        ));
    }

    fn begin_reading(&mut self) {
        self.reading = Some(Reading {
            covers: self.edits_done,
            ..Reading::default()
        });
        self.notices.push(Notice::Changed);
        self.kiln.push(Job::new(
            ["list", "--format", "json"],
            Tag::Load {
                generation: self.generation,
                step: Step::List,
            },
        ));
    }

    /// What kiln finished, handled until nothing more has.
    fn pump(&mut self) {
        while let Some(done) = self.kiln.take_finished() {
            self.finished(done);
        }
    }

    fn finished(&mut self, done: Finished<Tag>) {
        match done.tag {
            Tag::Query { id, edit } => {
                self.answer(id, done.line, done.result);
                if edit {
                    self.edits_done += 1;
                    self.reload(Why::Edit);
                }
            }
            Tag::Version { generation } if generation == self.generation => {
                self.probed(done.result)
            }
            Tag::Load { generation, step } if generation == self.generation => {
                self.step_done(step, &done.line, done.result)
            }
            // From before another kiln was chosen.
            Tag::Version { .. } | Tag::Load { .. } => {}
        }
    }

    fn probed(&mut self, result: Result<Ran, Failure>) {
        self.phase = match result {
            Err(Failure::NotFound(why)) => Phase::NotFound(why),
            Err(f) => Phase::NotKiln(f.line()),
            Ok(ran) => match ran
                .expect(&[0])
                .map_err(|f| f.line())
                .and_then(Version::parse)
            {
                Err(why) => Phase::NotKiln(why),
                Ok(v) if !v.supported() => Phase::TooOld(v),
                Ok(v) => Phase::Ready(v),
            },
        };
        self.notices.push(Notice::Changed);
        let held = std::mem::take(&mut self.held);
        if let Phase::Ready(_) = self.phase {
            self.begin_reading();
            self.start_git();
            self.restart_refresh();
            for (_, job) in held {
                self.kiln.push(job);
            }
        } else {
            let why = match &self.phase {
                Phase::TooOld(v) => too_old(*v),
                Phase::NotFound(w) | Phase::NotKiln(w) => w.clone(),
                _ => String::new(),
            };
            for (id, job) in held {
                self.answer(id, job.line(), Err(Failure::Unusable(why.clone())));
            }
        }
    }

    fn step_done(&mut self, step: Step, line: &str, result: Result<Ran, Failure>) {
        let Phase::Ready(version) = self.phase else {
            return;
        };
        let Some(reading) = self.reading.as_mut() else {
            return;
        };
        let generation = self.generation;
        // `check` exits 1 when it finds an error, with its JSON all the same.
        let fine: &[i32] = if step == Step::Check { &[0, 1] } else { &[0] };
        let text = result
            .as_ref()
            .map_err(Failure::clone)
            .and_then(|r| r.expect(fine).map(str::to_string));
        match step {
            Step::List => match text.map_err(|f| f.line()).and_then(|t| model::list(&t)) {
                Ok(items) => {
                    reading.items = Some(items);
                    self.kiln.push(Job::new(
                        ["status", "--format", "json"],
                        Tag::Load {
                            generation,
                            step: Step::Status,
                        },
                    ));
                }
                Err(why) => {
                    if is_no_project(&why) {
                        self.no_project = Some(why);
                        self.loaded = None;
                        self.error = None;
                    } else {
                        self.error = Some(failed(line, &why));
                    }
                    self.end_reading();
                }
            },
            Step::Status => {
                // Another kiln's shape turns the pages off with the reason,
                // as a failed reload; any other failure of `status` is its
                // part's alone.
                if let Some(why) = text
                    .as_deref()
                    .ok()
                    .and_then(|t| model::other_shape("kiln status", t))
                {
                    self.error = Some(why);
                    self.loaded = None;
                    self.end_reading();
                    return;
                }
                reading.plan = Some(text.map_err(|f| f.line()).and_then(|t| model::status(&t)));
                self.kiln.push(Job::new(
                    ["check", "--format", "json"],
                    Tag::Load {
                        generation,
                        step: Step::Check,
                    },
                ));
            }
            Step::Check => {
                let findings = text.map_err(|f| f.line()).and_then(|t| model::check(&t));
                let reading = self.reading.take().unwrap_or_default();
                let items = reading.items.unwrap_or_default();
                let plan = reading
                    .plan
                    .unwrap_or_else(|| Err("kiln status gave no answer".into()));
                self.loaded = Some(Loaded::new(version, items, plan, findings));
                self.loads += 1;
                self.edits_read = reading.covers;
                self.error = None;
                self.no_project = None;
                self.end_reading();
            }
        }
    }

    fn end_reading(&mut self) {
        self.reading = None;
        self.notices.push(Notice::Changed);
        if std::mem::take(&mut self.again) {
            self.begin_reading();
        }
    }

    fn look_at_git(&mut self) {
        let now = api::host::git_state().map(|g| (g.branch, g.head));
        let before = self.git.replace(now.clone());
        if before.is_some_and(|b| b != now) {
            self.reload(Why::Git);
        }
    }

    fn start_git(&mut self) {
        if self.git_timer.is_none() {
            self.git_timer = Some(api::host::set_timer(GIT_POLL_MS, true));
            self.look_at_git();
        }
    }

    fn restart_refresh(&mut self) {
        if let Some(t) = self.refresh_timer.take() {
            api::host::cancel_timer(t);
        }
        let seconds = refresh_seconds();
        if self.shown && seconds > 0 && matches!(self.phase, Phase::Ready(_)) {
            self.refresh_timer = Some(api::host::set_timer(seconds.saturating_mul(1000), true));
        }
    }

    fn stop_timers(&mut self) {
        for t in [self.refresh_timer.take(), self.git_timer.take()]
            .into_iter()
            .flatten()
        {
            api::host::cancel_timer(t);
        }
    }
}

/// `kiln.refresh_seconds`: 0 for none; unset or not a number, the
/// default; at most an hour.
pub fn refresh_seconds() -> u32 {
    api::setting("kiln.refresh_seconds")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_REFRESH_SECONDS)
        .min(3600)
}

fn too_old(v: Version) -> String {
    format!(
        "kiln {v} is older than {}, the oldest this extension reads: update Kiln",
        model::MIN_VERSION
    )
}

/// kiln's line for a folder with no `kiln.toml` in it or above it (exit 2):
/// `kiln: no kiln.toml in <dir> or any folder above it (…)`.
fn is_no_project(line: &str) -> bool {
    line.starts_with("kiln: no kiln.toml")
}

/// A reload's failure as one line: kiln's own line as it is, and one that
/// already names its command (`kiln list: kiln's JSON can't be read…`);
/// any other (a cap, a timeout) after the command it was.
fn failed(line: &str, why: &str) -> String {
    let command = line.split(' ').take(2).collect::<Vec<_>>().join(" ");
    if why.starts_with("kiln: ") || why.starts_with(&format!("{command}:")) {
        why.to_string()
    } else {
        format!("{line}: {why}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use fenix_test_kit::{Fake, Ran as Run};

    /// The fake's events, handed in as lib.rs does, until there are none.
    fn settle(fake: &Fake, p: &mut Project) {
        while let Some(e) = fake.next_event() {
            match e {
                api::Event::ProcessOutput(o) => p.output(o.process, o.stderr, &o.line),
                api::Event::ProcessExit((id, code)) => p.exited(id, code),
                api::Event::Timer(t) => {
                    p.timer(t);
                }
                _ => {}
            }
        }
    }

    /// A fake whose kiln answers as the demo project's did.
    fn demo() -> Fake {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Run::ok(fixture("version.txt")))
            .on_spawn(
                "kiln list --format json",
                Run::ok(fixture("demo/list.json")),
            )
            .on_spawn(
                "kiln status --format json",
                Run::ok(fixture("demo/status.json")),
            )
            .on_spawn(
                "kiln check --format json",
                Run::exits(1)
                    .stdout(fixture("demo/check.json"))
                    .stderr("kiln: 1 error, 2 warnings"),
            );
        fake
    }

    fn lines(fake: &Fake) -> Vec<String> {
        fake.take()
            .into_iter()
            .filter(|l| l.starts_with("spawn"))
            .map(|l| l.split_whitespace().skip(2).collect::<Vec<_>>().join(" "))
            .collect()
    }

    #[test]
    fn idle_until_something_wants_it() {
        let fake = demo();
        let mut p = Project::default();
        p.saved("/p/design/a.md");
        p.set_shown(true);
        p.timer(99);
        assert_eq!(p.state(), State::Idle);
        assert!(lines(&fake).is_empty());
    }

    #[test]
    fn start_probes_then_reads_list_status_check() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        assert_eq!(p.state(), State::Probing);
        settle(&fake, &mut p);
        assert_eq!(
            lines(&fake),
            [
                "/usr/bin/kiln --version",
                "/usr/bin/kiln list --format json",
                "/usr/bin/kiln status --format json",
                "/usr/bin/kiln check --format json"
            ]
        );
        let State::Loaded {
            loaded,
            reloading: false,
            error: None,
        } = p.state()
        else {
            panic!("{:?}", p.state())
        };
        assert_eq!(loaded.name(), Some("Demo"));
        assert_eq!(loaded.items.len(), 16);
        assert_eq!(
            loaded.item("TASK-004").map(|i| i.title.as_str()),
            Some("Add the export button")
        );
        assert_eq!(
            loaded
                .children("TASK-002")
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            ["TASK-007"]
        );
        // check exited 1, with its findings.
        assert_eq!(
            loaded.counts(),
            Some(Counts {
                errors: 1,
                warnings: 2
            })
        );
        let ms1 = loaded.group(Some("MS-001")).unwrap();
        assert_eq!(
            (
                ms1.done,
                ms1.total,
                ms1.ready.len(),
                ms1.waiting.len(),
                ms1.in_progress.len()
            ),
            (1, 5, 2, 1, 1)
        );
        assert_eq!(loaded.group(None).unwrap().ready[0].id, "TASK-008");
        assert_eq!(
            p.version(),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert!(p.take_notices().contains(&Notice::Changed));
    }

    #[test]
    fn kiln_missing_says_how_to_give_one() {
        let fake = Fake::new();
        fake.install();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        assert_eq!(
            p.state(),
            State::NotFound(
                "kiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is"
            )
        );
        // A save doesn't try again; asking does.
        p.saved("x");
        assert!(matches!(p.state(), State::NotFound(_)));
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Run::ok("kiln 1.0.0\n"));
        p.reload(Why::Asked);
        assert_eq!(p.state(), State::Probing);
    }

    #[test]
    fn an_old_kiln_turns_the_pages_off() {
        let fake = demo();
        fake.on_spawn("kiln --version", Run::ok("kiln 0.9.4\n"));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        assert_eq!(
            p.state(),
            State::TooOld(Version {
                major: 0,
                minor: 9,
                patch: 4
            })
        );
        assert_eq!(lines(&fake), ["/usr/bin/kiln --version"]);
        let id = p.query(["show", "REQ-001", "--format", "json"]);
        let n = p.take_notices();
        assert!(
            n.contains(&Notice::Answer {
                id,
                line: "kiln show REQ-001 --format json".into(),
                result: Err(Failure::Unusable(
                    "kiln 0.9.4 is older than 1.0.0, the oldest this extension reads: update Kiln"
                        .into()
                ))
            }),
            "{n:?}"
        );
    }

    #[test]
    fn another_program_called_kiln_is_said() {
        let fake = demo();
        fake.on_spawn(
            "kiln --version",
            Run::fails(2, "error: unknown flag --version"),
        );
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        assert_eq!(p.state(), State::NotKiln("error: unknown flag --version"));
    }

    #[test]
    fn no_kiln_toml_is_kilns_line() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Run::fails(2, fixture("errors/no-kiln-toml.stderr")),
        );
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        assert_eq!(p.state(), State::NoProject("kiln: no kiln.toml in <dir> or any folder above it (run `kiln init` to start a project)"));
        let id = p.query(["show", "REQ-001"]);
        assert!(
            matches!(&p.take_notices()[..], [.., Notice::Answer { id: i, result: Err(Failure::Unusable(_)), .. }] if *i == id)
        );
    }

    #[test]
    fn the_previous_project_stays_while_a_reload_runs() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.on_spawn("kiln list", Run::running(""));
        p.reload(Why::Asked);
        let State::Loaded {
            loaded,
            reloading: true,
            error: None,
        } = p.state()
        else {
            panic!("{:?}", p.state())
        };
        assert_eq!(loaded.items.len(), 16);
    }

    #[test]
    fn two_reloads_while_one_runs_make_exactly_one_more() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.take();
        fake.on_spawn("kiln list", Run::running(fixture("demo/list.json")));
        p.reload(Why::Asked);
        p.reload(Why::Saved);
        p.reload(Why::Git);
        p.reload(Why::Timer);
        settle(&fake, &mut p);
        let first = fake.process("kiln list").unwrap().id;
        fake.on_spawn("kiln list", Run::ok(fixture("demo/list.json")));
        p.exited(first, Some(0));
        settle(&fake, &mut p);
        let ran = lines(&fake);
        assert_eq!(
            ran.iter().filter(|l| l.contains(" list ")).count(),
            2,
            "{ran:?}"
        );
        assert_eq!(
            ran.iter().filter(|l| l.contains(" check ")).count(),
            2,
            "{ran:?}"
        );
        assert!(!p.reloading());
    }

    #[test]
    fn a_timer_doesnt_queue_behind_a_reload() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.on_spawn("kiln list", Run::running(""));
        p.reload(Why::Asked);
        p.reload(Why::Timer);
        assert!(!p.again);
    }

    #[test]
    fn a_failed_reload_keeps_what_was_loaded_with_the_error() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.on_spawn(
            "kiln list",
            Run::fails(
                2,
                "kiln: design/tasks/TASK-001.md:3: front matter can't be parsed",
            ),
        );
        p.reload(Why::Saved);
        settle(&fake, &mut p);
        let State::Loaded {
            loaded,
            reloading: false,
            error: Some(e),
        } = p.state()
        else {
            panic!("{:?}", p.state())
        };
        assert_eq!(
            (loaded.items.len(), e),
            (
                16,
                "kiln: design/tasks/TASK-001.md:3: front matter can't be parsed"
            )
        );
        // The next good read clears it.
        fake.on_spawn("kiln list", Run::ok(fixture("demo/list.json")));
        p.reload(Why::Asked);
        settle(&fake, &mut p);
        assert!(matches!(p.state(), State::Loaded { error: None, .. }));
    }

    #[test]
    fn a_first_read_that_fails_is_failed() {
        let fake = demo();
        fake.on_spawn("kiln list", Run::ok("[{\"id\": \"REQ-0"));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let State::Failed(line) = p.state() else {
            panic!("{:?}", p.state())
        };
        assert!(
            line.starts_with("kiln list: kiln's JSON can't be read: "),
            "{line}"
        );
    }

    #[test]
    fn status_or_check_failing_leaves_the_rest() {
        let fake = demo();
        fake.on_spawn(
            "kiln status",
            Run::fails(
                2,
                "kiln: kiln.toml has no `part-of` link role, so nothing is work in a milestone",
            ),
        );
        fake.on_spawn("kiln check", Run::ok("{\"kiln_json\": 1}"));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let State::Loaded { loaded, .. } = p.state() else {
            panic!("{:?}", p.state())
        };
        assert_eq!(loaded.items.len(), 16);
        assert_eq!(
            loaded.plan,
            Err(
                "kiln: kiln.toml has no `part-of` link role, so nothing is work in a milestone"
                    .into()
            )
        );
        assert_eq!(
            loaded.findings,
            Err("kiln check: expected a JSON array, got an object".into())
        );
        assert_eq!(
            (loaded.groups().len(), loaded.counts(), loaded.name()),
            (0, None, None)
        );
    }

    #[test]
    fn another_kilns_shape_turns_the_pages_off() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.on_spawn(
            "kiln status",
            Run::ok("{\"kiln_json\": 2, \"project\": \"Demo\"}"),
        );
        p.reload(Why::Asked);
        settle(&fake, &mut p);
        assert_eq!(p.state(), State::Failed("kiln status: its JSON is shape 2 (kiln_json), and this extension reads shape 1: update the extension"));
        assert!(p.loaded().is_none());
    }

    #[test]
    fn check_exiting_2_is_its_line() {
        let fake = demo();
        fake.on_spawn(
            "kiln check",
            Run::fails(2, "kiln: --results x.xml can't be read"),
        );
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let State::Loaded { loaded, .. } = p.state() else {
            panic!()
        };
        assert_eq!(
            loaded.findings,
            Err("kiln: --results x.xml can't be read".into())
        );
        // status still counts them.
        assert_eq!(
            loaded.counts(),
            Some(Counts {
                errors: 1,
                warnings: 2
            })
        );
    }

    #[test]
    fn a_query_waits_for_kiln_to_be_known() {
        let fake = demo();
        fake.on_spawn("kiln show", Run::ok(fixture("demo/show-REQ-001.json")));
        let mut p = Project::default();
        let id = p.query(["show", "REQ-001", "--format", "json"]);
        settle(&fake, &mut p);
        let answer = p.take_notices().into_iter().find_map(|n| match n {
            Notice::Answer { id: i, result, .. } if i == id => Some(result),
            _ => None,
        });
        let shown = model::show(&answer.unwrap().unwrap().stdout).unwrap();
        assert_eq!(shown.id, "REQ-001");
        // It ran after the probe, behind the first read's list.
        assert_eq!(
            lines(&fake)[..3],
            [
                "/usr/bin/kiln --version".to_string(),
                "/usr/bin/kiln list --format json".into(),
                "/usr/bin/kiln show REQ-001 --format json".into()
            ]
        );
    }

    #[test]
    fn an_edit_answers_then_reloads() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.take();
        p.take_notices();
        fake.on_spawn("kiln set", Run::fails(1, "kiln: design/tasks/TASK-004.md changed on disk since it was read; run the command again"));
        let id = p.edit(["set", "TASK-004", "status=doing"], None);
        settle(&fake, &mut p);
        let n = p.take_notices();
        let Some(Notice::Answer {
            result: Ok(ran), ..
        }) = n
            .iter()
            .find(|n| matches!(n, Notice::Answer { id: i, .. } if *i == id))
        else {
            panic!("{n:?}")
        };
        // kiln ran and refused: exit 1, its line.
        let f = ran.expect(&[0]).unwrap_err();
        assert_eq!((f.code(), f.line()), (Some(1), "kiln: design/tasks/TASK-004.md changed on disk since it was read; run the command again".to_string()));
        assert_eq!(
            lines(&fake)[1..],
            [
                "/usr/bin/kiln list --format json".to_string(),
                "/usr/bin/kiln status --format json".into(),
                "/usr/bin/kiln check --format json".into()
            ]
        );
    }

    #[test]
    fn a_new_git_head_reloads() {
        let fake = demo();
        fake.git(Some(api::GitSummary {
            branch: Some("main".into()),
            head: Some("aaa".into()),
            upstream: None,
            ahead: 0,
            behind: 0,
            conflicts: 0,
        }));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let git = fake
            .timers()
            .iter()
            .find(|t| t.millis == GIT_POLL_MS && t.repeat)
            .unwrap()
            .id;
        fake.take();
        p.timer(git);
        settle(&fake, &mut p);
        assert!(lines(&fake).is_empty(), "the same head reloads nothing");
        fake.git(Some(api::GitSummary {
            branch: Some("main".into()),
            head: Some("bbb".into()),
            upstream: None,
            ahead: 1,
            behind: 0,
            conflicts: 0,
        }));
        p.timer(git);
        settle(&fake, &mut p);
        assert_eq!(lines(&fake).len(), 3);
    }

    #[test]
    fn the_refresh_timer_runs_only_while_shown() {
        let fake = demo();
        fake.setting("kiln.refresh_seconds", "7");
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        assert!(!fake.timers().iter().any(|t| t.millis == 7000));
        p.set_shown(true);
        settle(&fake, &mut p);
        let refresh = fake
            .timers()
            .iter()
            .find(|t| t.millis == 7000 && t.repeat)
            .unwrap()
            .id;
        fake.take();
        p.timer(refresh);
        settle(&fake, &mut p);
        assert_eq!(lines(&fake).len(), 3);
        p.set_shown(false);
        assert!(!fake.timers().iter().any(|t| t.id == refresh));
        fake.setting("kiln.refresh_seconds", "0");
        p.set_shown(true);
        assert!(!fake.timers().iter().any(|t| t.millis == 0));
    }

    #[test]
    fn another_kiln_starts_again_and_answers_what_waited() {
        let fake = demo();
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        fake.on_spawn("kiln show", Run::running(""));
        let id = p.query(["show", "REQ-001"]);
        p.take_notices();
        fake.setting("kiln.path", "/opt/kiln");
        fake.on_spawn("/opt/kiln --version", Run::ok("kiln 1.1.0\n"));
        p.kiln_changed();
        let n = p.take_notices();
        assert!(n.iter().any(|n| matches!(n, Notice::Answer { id: i, result: Err(Failure::Unusable(_)), .. } if *i == id)), "{n:?}");
        settle(&fake, &mut p);
        assert_eq!(
            p.version(),
            Some(Version {
                major: 1,
                minor: 1,
                patch: 0
            })
        );
    }

    #[test]
    fn a_slow_kiln_is_stopped_and_said() {
        let fake = demo();
        fake.on_spawn("kiln list", Run::running("["));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let watchdog = fake
            .timers()
            .iter()
            .find(|t| t.millis == crate::kiln::TIMEOUT_MS)
            .unwrap()
            .id;
        assert!(p.timer(watchdog));
        assert_eq!(
            p.state(),
            State::Failed(
                "kiln list --format json: kiln gave no answer in 60 s, so it was stopped"
            )
        );
    }

    #[test]
    fn kilns_own_project_loads() {
        let fake = demo();
        fake.on_spawn("kiln list", Run::ok(fixture("own/list.json")))
            .on_spawn("kiln status", Run::ok(fixture("own/status.json")))
            .on_spawn("kiln check", Run::ok(fixture("own/check.json")));
        let mut p = Project::default();
        p.start();
        settle(&fake, &mut p);
        let State::Loaded { loaded, .. } = p.state() else {
            panic!()
        };
        assert_eq!(
            (loaded.name(), loaded.groups().len(), loaded.counts()),
            (Some("Kiln"), 0, Some(Counts::default()))
        );
    }
}
