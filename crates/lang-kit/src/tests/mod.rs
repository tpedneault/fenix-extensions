//! Test providers (extension API 1.6): what a pack knows of its test
//! runner, as a [`TestRunner`], and the adapter that puts it on the five
//! exports Fenix calls -- `tests-scan`, `tests-plan`, `tests-read`,
//! `tests-step-ended`, `tests-launch` -- through [`Kit`]'s methods of the
//! same names.
//!
//! Fenix runs every command and owns the page, the model and the results
//! (Fenix's `docs/plans/test-explorer.md`); a pack says how to find tests
//! in a file's text, what commands list and run them, how to read what
//! they print and how to debug one. The adapter does the rest: a reader
//! per run and step, a line cut at 64 KiB joined again, groups' ids, a
//! memory of what scans and listings found, one result per test, long
//! selections split, reporters written to `/data` and named in `needs`,
//! and the runner in use told to Fenix only when it changes.
//!
//! API 1.6 isn't released yet: [`api`] is a copy of its types, and
//! nothing here calls a host function 1.5 hasn't. `plans/test-explorer.md`
//! says how the copy is swapped for the real module.

pub mod api;

mod adapter;
mod line;
mod memory;
#[cfg(test)]
mod unit;

use fenix_extension_api::{Command, LaunchConfig};

pub use adapter::Tests;
pub use api::{Kind, Outcome, StepEnd};
pub use line::{command_line_len, cut_id, cut_label, split, BATCH_FILE, BATCH_FILE_MAX, DIRECT, HEADROOM, ID_MAX, LABEL_MAX, WINDOWS_MAX};

use crate::Kit;

/// What precedes each label in a group's id: a group's id is its labels,
/// root first, each after one of these (`\u{1f}orbit (lib)\u{1f}src/lib.rs`),
/// so it's never a test's key (Fenix §6.6).
pub const GROUP: char = '\u{1f}';

/// A group's id in Fenix: its labels, root first, each after [`GROUP`];
/// cut to [`ID_MAX`] as Fenix cuts an id.
pub fn group_id<S: AsRef<str>>(labels: &[S]) -> String {
    labels.iter().fold(String::new(), |id, l| child_id(&id, l.as_ref()))
}

/// The id of group `label` in the group whose id is `parent` ("" for
/// none).
pub(crate) fn child_id(parent: &str, label: &str) -> String {
    let mut id = String::with_capacity(parent.len() + 1 + label.len());
    id.push_str(parent);
    id.push(GROUP);
    id.push_str(&cut_label(label));
    match cut_id(&id) {
        std::borrow::Cow::Borrowed(_) => id,
        std::borrow::Cow::Owned(cut) => cut,
    }
}

/// A runner's own name for a test, the same in what the pack scans, lists
/// and reads: `orbit/lib geo::tests::unwraps_none`,
/// `tests/test_basic.py::test_cases[negative]`, `example.com/orbit
/// TestTable/negative_one`. It's the test's id in Fenix, cut to 1 KiB
/// ([`cut_id`]) where it's longer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Key(pub String);

impl Key {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Key {
    fn from(s: &str) -> Key {
        Key(s.into())
    }
}

impl From<String> for Key {
    fn from(s: String) -> Key {
        Key(s)
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A group a test is in: a target or a package ([`Kind::Group`]), a file
/// ([`Kind::File`]), a suite -- a `mod tests`, a class, a `describe`
/// ([`Kind::Suite`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub kind: Kind,
    /// What the page calls it, and its part of the group's id: `orbit
    /// (lib)`, `src/lib.rs`, `tests`.
    pub label: String,
    /// Its file, relative to the project (or `/project/...`, or a full
    /// path of the project's machine).
    pub file: Option<String>,
    /// Its first and last line, from 1.
    pub line: Option<u32>,
    pub end_line: Option<u32>,
}

impl Group {
    pub fn new(kind: Kind, label: impl Into<String>) -> Group {
        Group { kind, label: label.into(), file: None, line: None, end_line: None }
    }

    /// A target or a package ([`Kind::Group`]): `orbit (lib)`,
    /// `example.com/orbit`.
    pub fn package(label: impl Into<String>) -> Group {
        Group::new(Kind::Group, label)
    }

    /// A file, labelled and placed by its path below the project.
    pub fn file(path: impl Into<String>) -> Group {
        let path = path.into();
        Group { file: Some(path.clone()), ..Group::new(Kind::File, path) }
    }

    /// A suite at `line` (from 1) of `file`.
    pub fn suite(label: impl Into<String>, file: impl Into<String>, line: u32) -> Group {
        Group { file: Some(file.into()), line: Some(line), ..Group::new(Kind::Suite, label) }
    }

    pub fn in_file(mut self, file: impl Into<String>) -> Group {
        self.file = Some(file.into());
        self
    }

    pub fn lines(mut self, line: u32, end_line: Option<u32>) -> Group {
        self.line = Some(line);
        self.end_line = end_line;
        self
    }
}

/// A test or a case, as a scan, a listing or a run finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub key: Key,
    /// What the page calls it: `adds`, `negative`, `add(-1, -1) is -3`.
    pub label: String,
    /// [`Kind::Test`] or [`Kind::Case`].
    pub kind: Kind,
    /// The groups it's in, root first. A [`Found`] from a listing or a run
    /// whose key a scan found is given under the scan's groups.
    pub groups: Vec<Group>,
    /// Its file, relative to the project (or `/project/...`, or a full
    /// path); none in a scan's answer is the file scanned.
    pub file: Option<String>,
    /// Its first and last line, from 1, as runners write them.
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    /// Left out by its source (`#[ignore = "slow"]`, `@skip`, `test.skip`):
    /// why, or "" when it doesn't say.
    pub skip: Option<String>,
    /// Its cases come only from a run: `test.each`, a `parametrize` the
    /// scan saw, a Go test that calls `t.Run`, an `#[rstest]`.
    pub template: bool,
    /// A case's template, when the pack knows it: pytest's `…::test_cases`
    /// of `…::test_cases[negative]`, Go's `TestTable`.
    pub of: Option<Key>,
    /// The pack's own, at most 4 KiB: never shown, given back as it was in
    /// an ask and to [`TestRunner::launch`] (what its runner calls it when
    /// the key can't say). Not kept across a restart of Fenix.
    pub data: Option<String>,
}

impl Found {
    /// A test called `label`, keyed `key`.
    pub fn test(key: impl Into<Key>, label: impl Into<String>) -> Found {
        Found {
            key: key.into(),
            label: label.into(),
            kind: Kind::Test,
            groups: Vec::new(),
            file: None,
            line: None,
            end_line: None,
            skip: None,
            template: false,
            of: None,
            data: None,
        }
    }

    /// A case of `template`.
    pub fn case(key: impl Into<Key>, label: impl Into<String>, template: impl Into<Key>) -> Found {
        Found { kind: Kind::Case, of: Some(template.into()), ..Found::test(key, label) }
    }

    /// Under these groups, root first.
    pub fn under(mut self, groups: impl IntoIterator<Item = Group>) -> Found {
        self.groups = groups.into_iter().collect();
        self
    }

    /// At `line` (from 1) of `file`.
    pub fn at(mut self, file: impl Into<String>, line: u32) -> Found {
        self.file = Some(file.into());
        self.line = Some(line);
        self
    }

    /// At `line` (from 1) of the file scanned.
    pub fn line(mut self, line: u32) -> Found {
        self.line = Some(line);
        self
    }

    pub fn end_line(mut self, end: u32) -> Found {
        self.end_line = Some(end);
        self
    }

    pub fn skip(mut self, why: impl Into<String>) -> Found {
        self.skip = Some(why.into());
        self
    }

    pub fn template(mut self) -> Found {
        self.template = true;
        self
    }

    pub fn data(mut self, data: impl Into<String>) -> Found {
        self.data = Some(data.into());
        self
    }
}

/// One thing Fenix asked to run.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    /// A test or a case, with what Fenix has of it now (its `data` too).
    Test(Found),
    /// A group, file or suite: everything under it. `groups` is it and
    /// those it's in, root first (it's the last).
    Group {
        groups: Vec<Group>,
        /// What Fenix sent after it: for a file or a suite, the tests and
        /// cases Fenix has under it, so a runner whose filter is a
        /// substring (libtest's `geo::`) can name them exactly. Never for
        /// a target or package, which is named to its runner as itself;
        /// and none when the ask would have been over 10,000 records. Empty:
        /// plan it with the runner's own filter, which runs at least those.
        tests: Vec<Found>,
        /// When `tests` is empty: the tests lang-kit remembers under it
        /// from scans and listings, possibly not all of them.
        known: Vec<Key>,
    },
}

/// What Fenix asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// The runner's listing of every test it can find in the project.
    Discover,
    /// These.
    Run(Vec<Asked>),
    /// Every test.
    RunAll,
    /// The steps of a [`Launching::BuildFirst`], before debugging this
    /// test: what its reader reads is a build's.
    Debug(Found),
}

/// What a step runs, for Fenix to judge as it ends: a test, or a group
/// by its labels root first, which stands for everything under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Covered {
    Test(Key),
    Group(Vec<String>),
}

/// One command of a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The API's: program, args, cwd, env. `${data}` and `${tools}` are
    /// filled in by Fenix; `${step_files}` by lang-kit (see `files`).
    pub command: Command,
    /// What the page says while it runs: `orbit (lib)`, `tests/`.
    pub label: String,
    /// What it runs, when it knows: Fenix judges those as the step ends.
    /// Empty claims nothing: what of the ask no step names is judged when
    /// the plan's last step ends.
    pub covers: Vec<Covered>,
    /// Files under `/data` its command reads that lang-kit doesn't name
    /// itself. A [`TestRunner::assets`] file its args or env name
    /// (`${data}/lang-kit/tests/fenix_tests.py`) and its `files` are
    /// named for it.
    pub needs: Vec<String>,
    /// Files written for this step only (pytest's file of ids): each
    /// `(name, bytes)` is written under
    /// `/data/lang-kit/tests/runs/<run>-<step>/`, which its args and env
    /// call `${step_files}`, named in `needs`, and deleted when it ends.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Default for Step {
    /// A step with no program: what a reader after a restart is given.
    fn default() -> Step {
        Step { command: Command::new(""), label: String::new(), covers: Vec::new(), needs: Vec::new(), files: Vec::new() }
    }
}

impl Step {
    pub fn new(command: Command, label: impl Into<String>) -> Step {
        Step { command, label: label.into(), ..Step::default() }
    }

    pub fn covers(mut self, covers: impl IntoIterator<Item = Covered>) -> Step {
        self.covers = covers.into_iter().collect();
        self
    }

    /// Covering these keys.
    pub fn covering<K: Into<Key>>(self, keys: impl IntoIterator<Item = K>) -> Step {
        self.covers(keys.into_iter().map(|k| Covered::Test(k.into())))
    }

    pub fn needs(mut self, path: impl Into<String>) -> Step {
        self.needs.push(path.into());
        self
    }

    pub fn file(mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Step {
        self.files.push((name.into(), bytes.into()));
        self
    }
}

/// A whole line of a step's output: lang-kit has joined a cut line's
/// pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub text: &'a str,
    pub stderr: bool,
}

/// A place in a file: from 1, as runners write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At {
    /// Relative to the project, `/project/...`, or a full path of the
    /// project's machine (`C:\src\orbit\src\lib.rs`).
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
}

impl At {
    pub fn new(file: impl Into<String>, line: u32, column: Option<u32>) -> At {
        At { file: file.into(), line, column }
    }
}

/// A test that ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    pub key: Key,
    pub outcome: Outcome,
    /// How long it took, when its runner says.
    pub ms: Option<f64>,
    /// One line: a failure's message, a skip's reason, why it errored.
    pub message: Option<String>,
    /// Where it failed: the assertion, the panic, the deepest frame in the
    /// project.
    pub at: Option<At>,
    /// All of it: the diff, the traceback, what it printed.
    pub text: Option<String>,
}

impl Ended {
    pub fn new(key: impl Into<Key>, outcome: Outcome) -> Ended {
        Ended { key: key.into(), outcome, ms: None, message: None, at: None, text: None }
    }

    pub fn ms(mut self, ms: f64) -> Ended {
        self.ms = Some(ms);
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Ended {
        self.message = Some(message.into());
        self
    }

    pub fn at(mut self, at: At) -> Ended {
        self.at = Some(at);
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Ended {
        self.text = Some(text.into());
        self
    }
}

/// What a reader read.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// A test or a case: from a listing, or found in a run.
    Found(Found),
    Started(Key),
    /// A second `Ended` for a key in a run is merged into the first, the
    /// worse outcome winning, so what Fenix keeps (the last) is the whole.
    Ended(Ended),
    /// What a test printed, or the run did (`key` none).
    Output { key: Option<Key>, text: String },
    /// Nothing of it ran: a target didn't build, a file didn't load. Never
    /// a test's failure. `under` is the group's labels, root first; none:
    /// all the step covers.
    Broken { under: Option<Vec<String>>, message: String, at: Option<At> },
    /// A process died: what's under the group `under` (the binary's
    /// target, the package; none: all the step covers) that hasn't ended
    /// in this step is errored with `message`.
    Crashed { under: Option<Vec<String>>, message: String },
    /// Something to keep for this run, not for Fenix: a test binary's path
    /// from a build-first plan, given to [`TestRunner::launch`] when Fenix
    /// asks again.
    Keep { name: String, value: String },
}

/// Reads one step's output, a line at a time: a new one for each step.
/// A line it can't read it passes over; it never panics on its input.
pub trait Reader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>);
    /// The step ended: what it still held.
    fn end(&mut self, _end: &StepEnd, _out: &mut Vec<Report>) {}
}

/// How to debug a test.
#[derive(Debug, Clone, PartialEq)]
pub enum Launching {
    Now(LaunchConfig),
    /// These steps first (a build that says where the test's binary is),
    /// read as a run's, then [`TestRunner::launch`] again with what their
    /// readers kept.
    BuildFirst(Vec<Step>),
}

/// What a pack implements for its runner: one per
/// `[[contributes.test_providers]]` of its manifest, given to its kit with
/// [`Kit::tests`].
pub trait TestRunner {
    /// The manifest's `[[contributes.test_providers]].id` it answers for.
    fn provider(&self) -> &str;

    /// The runner it uses in this project, and why it can't run, when it
    /// can't: `("cargo nextest", None)`, `("pytest", Some("pytest isn't
    /// installed in .venv"))`; `("", None)` when there's nothing to run
    /// here, which hides it in this project. Asked when the pack starts,
    /// when a file is saved and on [`Kit::tests_runner_changed`], so it
    /// should be quick; Fenix is told only what changed.
    fn runner(&mut self) -> (String, Option<String>);

    /// The tests in file `path` (`/project/<path>`), whose text is `text`:
    /// read, nothing run, in a few milliseconds.
    fn scan(&mut self, path: &str, text: &str) -> Vec<Found>;

    /// The steps that list or run `ask`, in order. A selection too long
    /// for one command line goes in several ([`split`]); lang-kit names
    /// the step's reporters in `needs`.
    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String>;

    /// Whether listing again costs little once `ask`'s plan has run (it
    /// built what the listing builds: cargo's `--list` after `cargo
    /// test`), so Fenix lists after it. By default it doesn't.
    fn list_after(&mut self, _ask: &Ask) -> bool {
        false
    }

    /// A reader for `step` of the plan it made for `ask`.
    fn reader(&mut self, ask: &Ask, step: &Step) -> Box<dyn Reader>;

    /// A reader for a step of a run this start of the pack didn't plan:
    /// the pack started again while it ran. Fenix's plan says it never
    /// asks so (its §6.4, "One project"); lang-kit doesn't trust it. By
    /// default [`TestRunner::reader`] for [`Ask::RunAll`] and an empty step.
    fn reader_after_restart(&mut self) -> Box<dyn Reader> {
        self.reader(&Ask::RunAll, &Step::default())
    }

    /// Whether `reported`, a key a run found that neither the pack nor
    /// lang-kit placed, is one of `template`'s cases, when the line alone
    /// doesn't say. By default none is.
    fn is_case(&mut self, _template: &Found, _reported: &Key) -> bool {
        false
    }

    /// How to debug `test`; `kept` is what a build-first plan's readers
    /// kept (empty the first time).
    fn launch(&mut self, _test: &Found, _kept: &[(String, String)]) -> Result<Launching, String> {
        Err("no debugging for tests here".into())
    }

    /// Files its commands read, written under `/data/lang-kit/tests/` when
    /// the pack starts, where they differ: `("fenix_tests.py", bytes)`.
    fn assets(&self) -> Vec<(&str, &[u8])> {
        Vec::new()
    }
}

impl Kit {
    /// A test provider: `runner`, for its `provider()`. A pack with two
    /// calls it twice.
    pub fn tests(mut self, runner: Box<dyn TestRunner>) -> Kit {
        self.tests.add(runner);
        self
    }

    /// `tests-scan`: the tests in file `path`, whose text is `text`.
    pub fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<api::Found> {
        self.tests.scan(self.host.as_mut(), provider, path, text)
    }

    /// `tests-plan`: what to run for `ask`, as run `run`.
    pub fn tests_plan(&mut self, provider: &str, run: u64, ask: api::Ask) -> Result<api::Plan, String> {
        self.tests.plan(self.host.as_mut(), provider, run, ask)
    }

    /// `tests-read`: lines of step `step` of run `run`.
    pub fn tests_read(&mut self, provider: &str, run: u64, step: u32, lines: Vec<api::Line>) -> Vec<api::Report> {
        self.tests.read(self.host.as_mut(), provider, run, step, lines)
    }

    /// `tests-step-ended`: step `step` of run `run` ended.
    pub fn tests_step_ended(&mut self, provider: &str, run: u64, step: u32, end: api::StepEnd) -> Vec<api::Report> {
        self.tests.step_ended(self.host.as_mut(), provider, run, step, end)
    }

    /// `tests-launch`: how to debug `test`, for run `run`.
    pub fn tests_launch(&mut self, provider: &str, run: u64, test: api::Found) -> Result<api::Launch, String> {
        self.tests.launch(self.host.as_mut(), provider, run, test)
    }

    /// Something its runners depend on changed: each is asked again which
    /// runner it uses, and Fenix told where that changed.
    pub fn tests_runner_changed(&mut self) {
        self.tests.tell(self.host.as_mut());
    }
}
