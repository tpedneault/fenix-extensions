//! A stand-in for Fenix's side of a test provider (extension API 1.6),
//! so a pack's provider is tested natively on its runner's recorded
//! output: [`Fenix`] scans a file's text, asks for a plan, runs each step
//! by giving the provider recorded lines as Fenix would, ends it, and
//! keeps what Fenix would then hold -- the tests found, each one's state,
//! what didn't build -- by Fenix's rules: its plan (Fenix's
//! `docs/plans/test-explorer.md`, cited as *Fenix §n*, its review of steps
//! 1 and 2 as *Fenix §19*) and its model, `fenix-tests`, whose numbered
//! Decisions (`fenix-tests/src/lib.rs`) are cited as *Decisions n*.
//!
//! It is as strict as Fenix, no more: what Fenix refuses or loses --
//! a step it wouldn't start, a plan, answer or `data` past its limits --
//! fails the test (or, [`Fenix::lenient`], is handled as Fenix handles
//! it); what Fenix accepts and says once on the provider's output page --
//! an id over 1 KiB cut, a label, path, skip, message or text cut, a
//! `broken` naming a node it never found -- it does too, in
//! [`Fenix::said`].
//!
//! On API 1.5 it calls the five methods of lang-kit's `Kit` ([`Exports`]);
//! once the kits are on 1.6 the pack's `impl Extension` is one too.

mod feed;
mod model;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fenix_extension_api::{Command, LaunchConfig};
pub use fenix_lang_kit::tests::api;
use fenix_lang_kit::tests::{batch_file_line_len, command_line_len, cut_id, BATCH_FILE_MAX, ID_MAX, LABEL_MAX, WINDOWS_MAX};
use fenix_lang_kit::Kit;

use api::{Ask, Found, Kind, Line, Plan, Report, Step, StepEnd};
pub use feed::{batches, interleave, lines, strip_ansi, Order, BATCH_BYTES, BATCH_LINES, CUT_AT};
pub use model::{Detail, Model, Node, Record, Source, State, DATA_PER_PROVIDER};

use crate::Fake;

/// What `${data}` is filled in with when a step's command line is
/// counted (Fenix §19, K2): a data folder as long as Windows' would be.
pub const DATA_ON_WINDOWS: &str = r"C:\Users\someone\AppData\Roaming\Fenix\data\extensions\fenix.pack";

/// What `${tools}` is filled in with, the same way.
pub const TOOLS_ON_WINDOWS: &str = r"C:\Users\someone\AppData\Roaming\Fenix\tools\extensions\fenix.pack";

/// Where a program named without a path is taken to resolve to when a
/// command line is counted: Fenix counts the program's full path (Fenix
/// §19, K2), which depends on the machine.
pub const PROGRAMS_ON_WINDOWS: &str = r"C:\Users\someone\AppData\Local\Programs";

/// Fenix's limits (Fenix §6.5).
const FOUND_MAX: usize = 10_000;
const REPORTS_MAX: usize = 10_000;
const STEPS_MAX: usize = 256;
const COVERS_MAX: usize = 10_000;
const NEEDS_MAX: usize = 32;
const ENV_MAX: usize = 64 * 1024;
const PATH_MAX: usize = 4 * 1024;
const SKIP_MAX: usize = 1024;
const DATA_MAX: usize = 4 * 1024;
const MESSAGE_MAX: usize = 4 * 1024;
const TEXT_MAX: usize = 64 * 1024;
const ASK_MAX: usize = 10_000;

/// What runs without a result end as when their step exited (Fenix §8.5).
pub const NO_RESULT: &str = "the run ended without a result for it";

/// A provider's five exports, as Fenix calls them.
pub trait Exports {
    fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<Found>;
    fn tests_plan(&mut self, provider: &str, run: u64, ask: Ask) -> Result<Plan, String>;
    fn tests_read(&mut self, provider: &str, run: u64, step: u32, lines: Vec<Line>) -> Vec<Report>;
    fn tests_step_ended(&mut self, provider: &str, run: u64, step: u32, end: StepEnd) -> Vec<Report>;
    fn tests_launch(&mut self, provider: &str, run: u64, test: Found) -> Result<api::Launch, String>;
    /// The pack started, with `/data` at `data`: its reporters written and
    /// its runners told, as its `activate` does.
    fn tests_started(&mut self, _data: &str) {}
}

impl Exports for Kit {
    fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<Found> {
        Kit::tests_scan(self, provider, path, text)
    }
    fn tests_plan(&mut self, provider: &str, run: u64, ask: Ask) -> Result<Plan, String> {
        Kit::tests_plan(self, provider, run, ask)
    }
    fn tests_read(&mut self, provider: &str, run: u64, step: u32, lines: Vec<Line>) -> Vec<Report> {
        Kit::tests_read(self, provider, run, step, lines)
    }
    fn tests_step_ended(&mut self, provider: &str, run: u64, step: u32, end: StepEnd) -> Vec<Report> {
        Kit::tests_step_ended(self, provider, run, step, end)
    }
    fn tests_launch(&mut self, provider: &str, run: u64, test: Found) -> Result<api::Launch, String> {
        Kit::tests_launch(self, provider, run, test)
    }
    fn tests_started(&mut self, data: &str) {
        self.tests.data = data.to_string();
        self.tests.activate(self.host.as_mut());
    }
}

impl<E: Exports + ?Sized> Exports for &mut E {
    fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<Found> {
        (**self).tests_scan(provider, path, text)
    }
    fn tests_plan(&mut self, provider: &str, run: u64, ask: Ask) -> Result<Plan, String> {
        (**self).tests_plan(provider, run, ask)
    }
    fn tests_read(&mut self, provider: &str, run: u64, step: u32, lines: Vec<Line>) -> Vec<Report> {
        (**self).tests_read(provider, run, step, lines)
    }
    fn tests_step_ended(&mut self, provider: &str, run: u64, step: u32, end: StepEnd) -> Vec<Report> {
        (**self).tests_step_ended(provider, run, step, end)
    }
    fn tests_launch(&mut self, provider: &str, run: u64, test: Found) -> Result<api::Launch, String> {
        (**self).tests_launch(provider, run, test)
    }
    fn tests_started(&mut self, data: &str) {
        (**self).tests_started(data)
    }
}

/// What a step printed, recorded: its stdout and stderr as Fenix keeps
/// them apart, how the two were interleaved, and how it ended.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// From a `.order` file: for each line, whether it was stderr's.
    pub order: Option<Vec<bool>>,
    pub end: StepEnd,
    /// From an `.args` file: the command line that printed it.
    pub args: Option<String>,
    /// Stopped (or timed out) after this many lines were given.
    stop: Option<(usize, StepEnd)>,
    /// The pack started again after this many lines were given.
    restart: Option<usize>,
}

impl Default for Recorded {
    fn default() -> Recorded {
        Recorded { stdout: Vec::new(), stderr: Vec::new(), order: None, end: StepEnd::Exited(Some(0)), args: None, stop: None, restart: None }
    }
}

impl Recorded {
    /// The fixture at `path` (relative to the crate, where `cargo test`
    /// runs): `<path>.stdout`, `<path>.stderr`, and when they're there
    /// `<path>.exit` (a code, or `signal`), `<path>.order` (`o` and `e`, a
    /// line each) and `<path>.args`. Exits 0 unless its `.exit` says.
    pub fn from(path: impl AsRef<Path>) -> Recorded {
        let path = path.as_ref();
        let read = |ext: &str| std::fs::read(path.with_extension(ext)).ok();
        let (stdout, stderr) = (read("stdout"), read("stderr"));
        assert!(stdout.is_some() || stderr.is_some(), "no recording at {}: neither its .stdout nor its .stderr is there", path.display());
        let text = |ext: &str| read(ext).map(|b| String::from_utf8_lossy(&b).trim().to_string());
        let end = match text("exit").as_deref() {
            None => StepEnd::Exited(Some(0)),
            Some("signal") => StepEnd::Exited(None),
            Some(code) => StepEnd::Exited(Some(code.parse().unwrap_or_else(|_| panic!("{}.exit: {code} isn't a code", path.display())))),
        };
        let order = text("order").map(|o| o.chars().filter(|c| matches!(c, 'o' | 'e')).map(|c| c == 'e').collect());
        Recorded { stdout: stdout.unwrap_or_default(), stderr: stderr.unwrap_or_default(), order, end, args: text("args"), ..Recorded::default() }
    }

    /// Printed `text` on stdout.
    pub fn stdout(text: impl Into<Vec<u8>>) -> Recorded {
        Recorded { stdout: text.into(), ..Recorded::default() }
    }

    pub fn and_stderr(mut self, text: impl Into<Vec<u8>>) -> Recorded {
        self.stderr = text.into();
        self
    }

    pub fn exits(mut self, code: i32) -> Recorded {
        self.end = StepEnd::Exited(Some(code));
        self
    }

    /// Ended by a signal: no code.
    pub fn signal(mut self) -> Recorded {
        self.end = StepEnd::Exited(None);
        self
    }

    /// Couldn't start, or its output couldn't be read.
    pub fn fails(mut self, why: impl Into<String>) -> Recorded {
        self.end = StepEnd::Failed(why.into());
        self
    }

    /// Stopped (`x`, a replacing run) once `lines` lines were given.
    pub fn stopped_after(mut self, lines: usize) -> Recorded {
        self.stop = Some((lines, StepEnd::Stopped));
        self
    }

    /// Past `tests.timeout_minutes` once `lines` lines were given.
    pub fn timed_out_after(mut self, lines: usize) -> Recorded {
        self.stop = Some((lines, StepEnd::TimedOut));
        self
    }

    /// The pack started again once `lines` lines were given (Fenix's
    /// [`Fenix::on_restart`] makes it).
    pub fn restarts_after(mut self, lines: usize) -> Recorded {
        self.restart = Some(lines);
        self
    }
}

/// What the run's plan was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Listing,
    Tests,
    /// A `build-first` plan before a launch: its `found`s and `ended`s are
    /// ignored (Decisions 17).
    Build,
}

/// A step as it ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub command: Command,
    pub label: String,
    pub covers: Vec<String>,
    pub needs: Vec<String>,
    pub end: StepEnd,
    /// Lines given, and `tests-read` calls.
    pub lines: usize,
    pub calls: usize,
}

/// A run, as it went.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: u64,
    /// `tests-plan`'s error.
    pub error: Option<String>,
    pub steps: Vec<Ran>,
    /// What the plan said.
    pub list_after: bool,
    /// Whether Fenix lists after it (Decisions 23): `list_after`, every
    /// step exited and nothing was `broken` (a `crashed` doesn't stop it).
    /// The driver doesn't run that listing itself: `discover` does.
    pub lists_after: bool,
    /// Each `broken` and `crashed`: its id and message.
    pub broken: Vec<(Option<String>, String)>,
    pub crashed: Vec<(Option<String>, String)>,
    /// What the page's head says when it was stopped.
    pub stopped: Option<String>,
    /// Planned, with at least one step, every step exited, nothing broken
    /// or crashed, not stopped (Fenix §4).
    pub complete: bool,
}

/// How a debug launch went (Fenix §11).
#[derive(Debug, Clone, PartialEq)]
pub struct Launched {
    /// The launch, or why there's none: an error, or what the head says.
    pub config: Result<LaunchConfig, String>,
    /// The `build-first` plan's run, when there was one.
    pub built: Option<Run>,
}

type Rule = Box<dyn Fn(&Step) -> Option<Recorded>>;

/// Fenix, for a provider's tests: see the module's doc.
pub struct Fenix<E: Exports> {
    pub ext: E,
    data: PathBuf,
    programs: Option<Vec<String>>,
    settings: BTreeMap<String, String>,
    found: Vec<String>,
    batch_files: Vec<String>,
    rules: Vec<Rule>,
    restart: Option<Box<dyn FnMut() -> E>>,
    fake: Option<Fake>,
    unsaved: HashSet<String>,
    next_run: u64,
    models: BTreeMap<String, Model>,
    empty: Model,
    /// Lines a `tests-read` (512), where Fenix cuts a line (64 KiB), and
    /// how the streams interleave (stdout first, or as recorded).
    pub batch: usize,
    pub cut_at: usize,
    pub order: Order,
    /// The longest a call may take natively: 5 ms in a release build,
    /// Fenix's "a few milliseconds" (§6.5); 50 ms in a debug build, whose
    /// code is about ten times slower.
    pub budget: Duration,
    /// `tests.timeout_minutes`, for what a timed-out test's message says
    /// (`timed out after 30 min`).
    pub timeout_minutes: u32,
    strict: bool,
    said: Vec<String>,
    slowest: Duration,
}

static FOLDERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl<E: Exports> Drop for Fenix<E> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

/// What Fenix keeps while a provider's plan runs (`fenix-tests`' `Part`
/// and `Run`).
struct Cx {
    provider: String,
    purpose: Purpose,
    /// The nodes selected, topmost; none: all of the provider's. What a
    /// step judges is what's counted under them (Decisions 10).
    sel: Option<Vec<String>>,
    /// Each test's own state before the run, to go back to.
    before: HashMap<String, State>,
    /// The tests with a result in this run.
    results: HashSet<String>,
    /// The tests this run made queued or running.
    overlaid: HashSet<String>,
    /// What a report named: found, started, ended, printed.
    named: HashSet<String>,
    /// What a test printed before its result came.
    printed: HashMap<String, String>,
    /// Each step's covers, and every id some step's covers names.
    covers: Vec<HashSet<String>>,
    named_by_steps: HashSet<String>,
    started_seen: bool,
    rest_running: bool,
    all_exited: bool,
    all_zero: bool,
    broken: bool,
    crashed: bool,
    halted: Option<StepEnd>,
    cwd: Option<String>,
    run: Run,
}

/// `path` as Fenix maps it (Fenix §6.4 "Paths"; Decisions 1): `/project/x`
/// and a relative one (to `base`, a step's folder, else the project) as
/// `x`, with `/`; a full path as it is.
fn place(path: &str, base: Option<&str>) -> String {
    if let Some(rest) = path.strip_prefix("/project/") {
        return rest.replace('\\', "/");
    }
    if is_full(path) {
        return path.to_string();
    }
    let rel = path.replace('\\', "/");
    let rel = rel.strip_prefix("./").unwrap_or(&rel);
    match base.map(|b| b.strip_prefix("/project").unwrap_or(b).trim_matches('/')) {
        Some(b) if !b.is_empty() && !is_full(b) => format!("{b}/{rel}"),
        _ => rel.to_string(),
    }
}

fn is_full(path: &str) -> bool {
    path.starts_with(['/', '\\']) || (path.len() > 1 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic())
}

/// `s` to at most `max` bytes, its start kept, at a character's edge.
fn cut_start(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `s` to at most `max` bytes, its end kept.
fn cut_end(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

fn cut_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((at, _)) => &s[..at],
        None => s,
    }
}

/// `n` with a comma each three digits, as Fenix writes its numbers.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl<E: Exports> Fenix<E> {
    /// The pack `ext`, started: a folder of its own stands for `/data`.
    pub fn new(mut ext: E) -> Fenix<E> {
        let n = FOLDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data = std::env::temp_dir().join(format!("fenix-test-kit-data-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap_or_else(|e| panic!("{}: {e}", data.display()));
        ext.tests_started(&data.to_string_lossy());
        Fenix {
            ext,
            data,
            programs: None,
            settings: BTreeMap::new(),
            found: Vec::new(),
            batch_files: Vec::new(),
            rules: Vec::new(),
            restart: None,
            fake: None,
            unsaved: HashSet::new(),
            next_run: 1,
            models: BTreeMap::new(),
            empty: Model::default(),
            batch: BATCH_LINES,
            cut_at: CUT_AT,
            order: Order::Recorded,
            budget: if cfg!(debug_assertions) { Duration::from_millis(50) } else { Duration::from_millis(5) },
            timeout_minutes: 30,
            strict: true,
            said: Vec::new(),
            slowest: Duration::ZERO,
        }
    }

    /// Where `/data` is for this pack.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The programs its manifest's `permissions.process` lists, read from
    /// the manifest at `path`.
    pub fn manifest(&mut self, path: impl AsRef<Path>) -> &mut Self {
        let text = std::fs::read_to_string(path.as_ref()).unwrap_or_else(|e| panic!("{}: {e}", path.as_ref().display()));
        self.programs = Some(process_list(&text));
        self
    }

    /// The programs `permissions.process` lists.
    pub fn programs(&mut self, programs: &[&str]) -> &mut Self {
        self.programs = Some(programs.iter().map(|p| p.to_string()).collect());
        self
    }

    /// A setting's value, for a program by `${setting:key}`.
    pub fn setting(&mut self, key: &str, value: &str) -> &mut Self {
        self.settings.insert(key.into(), value.into());
        self
    }

    /// A path `find-program` gave the pack, which may then run it.
    pub fn found_program(&mut self, path: &str) -> &mut Self {
        self.found.push(path.into());
        self
    }

    /// A program that resolves to a `.cmd` or `.bat` on Windows: its
    /// command line is counted as Rust gives it to `cmd.exe`, and held to
    /// 8,191.
    pub fn batch_file(&mut self, program: &str) -> &mut Self {
        self.batch_files.push(program.into());
        self
    }

    /// The fake the pack's kit was given, to read what it told Fenix
    /// (`test-provider-set`).
    pub fn with_fake(&mut self, fake: Fake) -> &mut Self {
        self.fake = Some(fake);
        self
    }

    /// How the pack is made again when a recording says it started again.
    pub fn on_restart(&mut self, make: impl FnMut() -> E + 'static) -> &mut Self {
        self.restart = Some(Box::new(make));
        self
    }

    /// A file of the project with unsaved edits (Fenix §8.4).
    pub fn unsaved(&mut self, path: &str) -> &mut Self {
        self.unsaved.insert(place(path, None));
        self
    }

    /// Every node of `provider` without its `data`, as after Fenix starts
    /// again, until a scan, listing or run gives it again (Fenix §19,
    /// finding 2): a pack's asks and launches then come with `data`
    /// `None`, and it plans from the id alone. A node whose id was cut
    /// isn't asked for itself then, but as its nearest ancestor Fenix can
    /// name (Decisions 32).
    pub fn forget_data(&mut self, provider: &str) -> &mut Self {
        self.model_mut(provider).forget_data();
        self
    }

    /// Breaking Fenix's rules is said (as on the provider's output page,
    /// [`Fenix::said`]) and handled as Fenix does, instead of failing the
    /// test.
    pub fn lenient(&mut self) -> &mut Self {
        self.strict = false;
        self
    }

    /// What Fenix said on the provider's output page: what it cut, and,
    /// leniently, the rules broken.
    pub fn said(&self) -> &[String] {
        &self.said
    }

    /// The longest call so far.
    pub fn slowest(&self) -> Duration {
        self.slowest
    }

    /// What step `matches` prints, recorded. The last rule given that
    /// matches a step is its.
    pub fn on_step(&mut self, matches: impl Fn(&Step) -> bool + 'static, recorded: Recorded) -> &mut Self {
        self.on_step_with(move |s| matches(s).then(|| recorded.clone()))
    }

    /// What a step prints, worked out from the step: none for a step it
    /// isn't for.
    pub fn on_step_with(&mut self, answer: impl Fn(&Step) -> Option<Recorded> + 'static) -> &mut Self {
        self.rules.push(Box::new(answer));
        self
    }

    /// What a step whose command line (program and args joined by spaces)
    /// contains `pattern` prints.
    pub fn on_command(&mut self, pattern: &str, recorded: Recorded) -> &mut Self {
        let pattern = pattern.to_string();
        self.on_step(move |s| command_text(&s.command).contains(&pattern), recorded)
    }

    /// A recording for the step whose command line is its `.args`.
    pub fn on_recorded(&mut self, recorded: Recorded) -> &mut Self {
        let args = recorded.args.clone().expect("a recording without an .args file: give it with on_step or on_command");
        self.on_step(move |s| command_text(&s.command) == args, recorded)
    }

    /// A rule Fenix enforces by refusing or losing something: fails the
    /// test, or, leniently, is said.
    fn violate(&mut self, what: String) {
        if self.strict {
            panic!("against Fenix's rules: {what}");
        }
        self.said.push(what);
    }

    /// What Fenix accepts and says once on the provider's output page.
    fn note(&mut self, what: impl Into<String>) {
        let what = what.into();
        if !self.said.contains(&what) {
            self.said.push(what);
        }
    }

    fn timed<R>(&mut self, what: &str, f: impl FnOnce(&mut E) -> R) -> R {
        let start = Instant::now();
        let r = f(&mut self.ext);
        let took = start.elapsed();
        self.slowest = self.slowest.max(took);
        if took > self.budget {
            self.violate(format!("{what} took {took:?}; a call may take {:?} here (Fenix §6.5: a few milliseconds)", self.budget));
        }
        r
    }

    /// The model of `provider`'s tests.
    pub fn model(&self, provider: &str) -> &Model {
        self.models.get(provider).unwrap_or(&self.empty)
    }

    fn model_mut(&mut self, provider: &str) -> &mut Model {
        self.models.entry(provider.to_string()).or_default()
    }

    /// Node `id`'s state: a test's own, a group's from what's under it.
    pub fn state(&self, provider: &str, id: &str) -> State {
        self.model(provider).state(id)
    }

    /// Test `id`'s last result: message, place, text, duration.
    pub fn detail(&self, provider: &str, id: &str) -> Detail {
        self.model(provider).nodes.get(id).map(|n| n.result.clone()).unwrap_or_default()
    }

    /// Where test `id` failed: path, line and column from 1.
    pub fn at(&self, provider: &str, id: &str) -> Option<(String, u32, Option<u32>)> {
        self.detail(provider, id).at.map(|(p, l, c)| (p, l + 1, c.map(|c| c + 1)))
    }

    /// What Fenix has of `id`, its sources merged.
    pub fn record(&self, provider: &str, id: &str) -> Option<Record> {
        self.model(provider).record(id)
    }

    /// Every id Fenix has for `provider`, in order.
    pub fn ids(&self, provider: &str) -> Vec<String> {
        self.model(provider).nodes.keys().cloned().collect()
    }

    /// Asserts each node's state.
    #[track_caller]
    pub fn assert_states(&self, provider: &str, expected: &[(&str, State)]) {
        let got: Vec<(&str, State)> = expected.iter().map(|(id, _)| (*id, self.state(provider, id))).collect();
        assert_eq!(got, expected, "states as Fenix holds them:\n{}", self.tree(provider));
    }

    /// What the pack told Fenix it runs, last: runner and problem. Fenix
    /// keeps it per project across the pack starting again (Fenix §19,
    /// K13).
    pub fn told(&self, provider: &str) -> Option<(String, Option<String>)> {
        self.fake.as_ref()?.test_providers().into_iter().rev().find(|(p, _, _)| p == provider).map(|(_, r, p)| (r, p))
    }

    /// Node `id` as Fenix gives it back, in an ask or to `tests-launch`
    /// (Fenix §6.4 "What Fenix gives back"): merged; its path
    /// `/project/<path>` (a full path outside the project); its `parent`
    /// the one Fenix put it under; its `data` as kept, or none.
    pub fn given_back(&self, provider: &str, id: &str) -> Option<Found> {
        let model = self.model(provider);
        let r = model.record(id)?;
        let path = r.path.as_ref().map(|p| if is_full(p) { p.clone() } else { format!("/project/{p}") });
        Some(Found { id: id.into(), parent: model.parent(id), label: r.label, kind: r.kind, path, line: r.line, end_line: r.end_line, skip: r.skip, template: r.template, data: r.data })
    }

    /// `tests-scan` of `path` (`/project/<path>`), whose text is `text`;
    /// its answer replaces the file's last scan (Fenix §4, §6.4): what it
    /// no longer gives loses that file's record, and an empty answer drops
    /// the file's tests (their results kept should they come back,
    /// Decisions 29).
    pub fn scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<Found> {
        assert!(path.starts_with("/project/"), "Fenix scans files as /project/<path>: {path}");
        assert!(text.len() <= 1 << 20, "Fenix doesn't scan a file over 1 MiB");
        let answer = self.timed("tests-scan", |e| e.tests_scan(provider, path, text));
        let mut found = answer.clone();
        if found.len() > FOUND_MAX {
            self.violate(format!("the scan of {path} gave {} tests; Fenix keeps {FOUND_MAX}", found.len()));
            found.truncate(FOUND_MAX);
        }
        let file = place(path, None);
        let mut given = Vec::new();
        for f in found {
            // A scan's missing path is the file scanned, for every kind
            // but a group (Decisions 2).
            let path = match (&f.path, f.kind) {
                (Some(p), _) => Some(place(p, None)),
                (None, Kind::Group) => None,
                (None, _) => Some(file.clone()),
            };
            // The same id twice: the later record replaces the earlier
            // (Decisions 3).
            let Some(id) = self.set_found(provider, f, path, Source::Scan, Some(&file)) else { continue };
            given.push(id);
        }
        let model = self.model_mut(provider);
        let now: HashSet<&String> = given.iter().collect();
        for old in model.scans.remove(&file).unwrap_or_default() {
            if !now.contains(&old) {
                model.drop_record(&old, Source::Scan, Some(&file), false);
            }
        }
        model.scans.insert(file, given);
        answer
    }

    /// A `found-test` held to Fenix's sizes (Fenix §6.5) and kept as
    /// `source`'s record: its id, as Fenix keys it.
    fn set_found(&mut self, provider: &str, mut f: Found, path: Option<String>, source: Source, file: Option<&str>) -> Option<String> {
        if f.id.len() > ID_MAX {
            self.note("an id over 1 KiB was cut to 1 KiB, ending in a hash of the whole");
            f.id = cut_id(&f.id).into_owned();
        }
        if let Some(p) = &mut f.parent {
            if p.len() > ID_MAX {
                *p = cut_id(p).into_owned();
                self.note("an id over 1 KiB was cut to 1 KiB, ending in a hash of the whole");
            }
        }
        if f.label.chars().count() > LABEL_MAX {
            f.label = cut_chars(&f.label, LABEL_MAX).to_string();
            self.note("a label over 256 characters was cut");
        }
        let mut path = path;
        if let Some(p) = &mut path {
            if p.len() > PATH_MAX {
                *p = cut_start(p, PATH_MAX).to_string();
                self.note("a path over 4 KiB was cut");
            }
        }
        if let Some(skip) = &mut f.skip {
            if skip.len() > SKIP_MAX {
                *skip = cut_start(skip, SKIP_MAX).to_string();
                self.note("a skip's reason over 1 KiB was cut");
            }
        }
        if f.data.as_ref().is_some_and(|d| d.len() > DATA_MAX) {
            // Not kept, so not given back: the pack would plan without it.
            self.violate(format!("{}'s data is over 4 KiB, which Fenix doesn't keep", f.id));
            f.data = None;
        }
        let id = f.id.clone();
        if let Some(said) = self.model_mut(provider).set(&id, source, file, Record::from(&f, path)) {
            self.note(said);
        }
        Some(id)
    }

    /// The ask Fenix makes to run `ids` (Fenix §4 "What the provider is
    /// asked", §19 findings 2 and 3; Decisions 20 and 32), as
    /// `fenix-tests`' `ask_records` makes it: the topmost of them, each
    /// one its provider can't name (a cut id whose `data` isn't kept) as
    /// its nearest ancestor it can; past 10,000, the deepest as their
    /// parents until they fit (at the top, `run-all`); in the page's
    /// order, each file or suite followed by its fill-in -- every node
    /// under it, suites included, in the page's order -- whole, while it
    /// fits in what's left of the 10,000 and holds nothing that can't be
    /// named. Each record is the node as [`Fenix::given_back`] gives it.
    pub fn ask_for(&self, provider: &str, ids: &[&str]) -> Ask {
        let model = self.model(provider);
        let topmost = |nodes: Vec<String>| -> Vec<String> {
            let set: HashSet<&String> = nodes.iter().collect();
            let mut top: Vec<String> = nodes.iter().filter(|n| !model.ancestry(n).iter().skip(1).any(|a| set.contains(a))).cloned().collect();
            top.sort();
            top.dedup();
            top
        };
        let nameable = |nodes: Vec<String>| -> Option<Vec<String>> {
            let mut out = Vec::new();
            for n in nodes {
                out.push(model.ancestry(&n).into_iter().find(|a| model.can_be_named(a))?);
            }
            Some(topmost(out))
        };
        let sel = ids.iter().map(|i| cut_id(i).into_owned()).filter(|i| model.has(i)).collect();
        let Some(mut nodes) = nameable(topmost(sel)) else { return Ask::RunAll };
        while nodes.len() > ASK_MAX {
            let deepest = nodes.iter().map(|n| model.depth(n)).max().unwrap_or(0);
            if deepest == 0 {
                return Ask::RunAll;
            }
            let coarser = nodes.iter().map(|n| if model.depth(n) == deepest { model.parent(n).unwrap_or_else(|| n.clone()) } else { n.clone() }).collect();
            match nameable(coarser) {
                Some(n) => nodes = n,
                None => return Ask::RunAll,
            }
        }
        nodes.sort_by_key(|n| model.order(n));
        let mut room = ASK_MAX - nodes.len();
        let mut records = Vec::new();
        for n in nodes {
            records.extend(self.given_back(provider, &n));
            if !model.record(&n).is_some_and(|r| matches!(r.kind, Kind::File | Kind::Suite)) {
                continue;
            }
            let under = model.descendants(&n);
            if under.len() <= room && under.iter().all(|u| model.can_be_named(u)) {
                room -= under.len();
                records.extend(under.iter().filter_map(|u| self.given_back(provider, u)));
            }
        }
        Ask::Run(records)
    }

    /// A listing (`discover`).
    pub fn discover(&mut self, provider: &str) -> Run {
        self.go(provider, Ask::Discover, None)
    }

    /// Every test.
    pub fn run_all(&mut self, provider: &str) -> Run {
        self.go(provider, Ask::RunAll, None)
    }

    /// These tests, files or groups: asked as [`Fenix::ask_for`] asks.
    pub fn run(&mut self, provider: &str, ids: &[&str]) -> Run {
        let ask = self.ask_for(provider, ids);
        let model = self.model(provider);
        let sel: Vec<String> = ids.iter().map(|i| cut_id(i).into_owned()).filter(|i| model.has(i)).collect();
        let set: HashSet<&String> = sel.iter().collect();
        let top: Vec<String> = sel.iter().filter(|n| !model.ancestry(n).iter().skip(1).any(|a| set.contains(a))).cloned().collect();
        let sel = if ask == Ask::RunAll { None } else { Some(top) };
        self.go(provider, ask, sel)
    }

    /// A file's tests (`SPC T f`, Decisions 27): every node but a group
    /// whose file is `path` (`/project/<path>`), its own or the one it's
    /// in.
    pub fn run_file(&mut self, provider: &str, path: &str) -> Run {
        let file = place(path, None);
        let ids: Vec<String> = self.model(provider).nodes.keys().filter(|k| self.record(provider, k).is_some_and(|r| r.kind != Kind::Group) && self.file_of(provider, k).as_deref() == Some(file.as_str())).cloned().collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.run(provider, &ids)
    }

    /// Node `id`'s file as Fenix keeps it: its own path, else the one of
    /// what it's in, up to a group (`fenix-tests`' `build_node`).
    fn file_of(&self, provider: &str, id: &str) -> Option<String> {
        let model = self.model(provider);
        for a in model.ancestry(id) {
            let r = model.record(&a)?;
            if r.path.is_some() {
                return r.path;
            }
            if r.kind == Kind::Group {
                return None;
            }
        }
        None
    }

    /// The failed again (Fenix §8.4): every test failed or errored.
    pub fn run_failed(&mut self, provider: &str) -> Run {
        let failed = self.failed(provider);
        let ids: Vec<&str> = failed.iter().map(String::as_str).collect();
        self.run(provider, &ids)
    }

    /// The tests failed or errored now.
    pub fn failed(&self, provider: &str) -> Vec<String> {
        let model = self.model(provider);
        model.nodes.iter().filter(|(k, n)| matches!(n.state, State::Failed | State::Errored) && model.record(k).is_some_and(|r| model::is_test(r.kind))).map(|(k, _)| k.clone()).collect()
    }

    fn cx(&self, provider: &str, id: u64, purpose: Purpose, sel: Option<Vec<String>>) -> Cx {
        Cx {
            provider: provider.into(),
            purpose,
            sel,
            before: HashMap::new(),
            results: HashSet::new(),
            overlaid: HashSet::new(),
            named: HashSet::new(),
            printed: HashMap::new(),
            covers: Vec::new(),
            named_by_steps: HashSet::new(),
            started_seen: false,
            rest_running: false,
            all_exited: true,
            all_zero: true,
            broken: false,
            crashed: false,
            halted: None,
            cwd: None,
            run: Run { id, error: None, steps: Vec::new(), list_after: false, lists_after: false, broken: Vec::new(), crashed: Vec::new(), stopped: None, complete: false },
        }
    }

    /// What's counted under the selection now, cases found in the run
    /// included (Decisions 10).
    fn universe(&self, cx: &Cx) -> Vec<String> {
        let model = self.model(&cx.provider);
        match &cx.sel {
            None => model.all_tests(),
            Some(nodes) => {
                let mut out: Vec<String> = nodes.iter().flat_map(|n| model.tests_under(n)).collect();
                out.sort();
                out.dedup();
                out
            }
        }
    }

    fn in_universe(&self, cx: &Cx, leaf: &str) -> bool {
        match &cx.sel {
            None => true,
            Some(nodes) => self.model(&cx.provider).ancestry(leaf).iter().any(|a| nodes.contains(a)),
        }
    }

    fn in_covers(&self, cx: &Cx, id: &str, covers: &HashSet<String>) -> bool {
        !covers.is_empty() && self.model(&cx.provider).ancestry(id).iter().any(|a| covers.contains(a))
    }

    /// Step `step`'s own tests -- a covered id stands for what's under it
    /// -- or, for a step whose `covers` is empty, the plan's rest: what
    /// of the ask no step names (Fenix §8.5; Decisions 11).
    fn step_scope(&self, cx: &Cx, step: usize) -> Vec<String> {
        let covers = &cx.covers[step];
        self.universe(cx).into_iter().filter(|l| if covers.is_empty() { !self.in_covers(cx, l, &cx.named_by_steps) } else { self.in_covers(cx, l, covers) }).collect()
    }

    fn set_live(&mut self, cx: &mut Cx, id: &str, state: State) {
        let model = self.models.entry(cx.provider.clone()).or_default();
        let Some(n) = model.nodes.get_mut(id) else { return };
        cx.before.entry(id.to_string()).or_insert(n.state);
        cx.overlaid.insert(id.to_string());
        n.state = state;
    }

    /// Back to what it was before the run: its last result, or none.
    fn back(&mut self, cx: &Cx, id: &str) {
        let model = self.models.entry(cx.provider.clone()).or_default();
        if let (Some(n), Some(s)) = (model.nodes.get_mut(id), cx.before.get(id)) {
            if matches!(n.state, State::Queued | State::Running) {
                n.state = *s;
            }
        }
    }

    /// Test `id`'s result in this run: the last `ended` wins (Fenix §4 "A
    /// result"); what it printed before it, kept with it.
    fn give_result(&mut self, cx: &mut Cx, id: &str, outcome: api::Outcome, detail: Detail) {
        let again = !cx.results.insert(id.to_string());
        let pending = cx.printed.remove(id).unwrap_or_default();
        let model = self.models.entry(cx.provider.clone()).or_default();
        let Some(n) = model.nodes.get_mut(id) else { return };
        cx.before.entry(id.to_string()).or_insert(n.state);
        if !again {
            n.printed.clear();
        }
        n.printed.push_str(&pending);
        n.printed = cut_end(&n.printed, TEXT_MAX).to_string();
        n.state = State::of(outcome);
        n.result = detail;
    }

    fn errored(&mut self, cx: &mut Cx, id: &str, message: &str, at: Option<(String, u32, Option<u32>)>) {
        self.give_result(cx, id, api::Outcome::Errored, Detail { message: Some(message.into()), at, text: None, duration_ms: None });
    }

    fn timed_out(&self) -> String {
        format!("timed out after {} min", self.timeout_minutes)
    }

    /// Tests without a result in the run, judged by how their step ended
    /// (Fenix §8.3, §8.4, §8.5): stopped, back to what they were; exited,
    /// errored "the run ended without a result for it", unless their file
    /// has unsaved edits (back); timed out or failed, errored with why.
    fn judge(&mut self, cx: &mut Cx, leaves: Vec<String>, end: &StepEnd) {
        let timed_out = self.timed_out();
        for t in leaves {
            if cx.results.contains(&t) || !self.model(&cx.provider).has(&t) {
                continue;
            }
            let unsaved = self.file_of(&cx.provider, &t).is_some_and(|f| self.unsaved.contains(&f));
            match end {
                StepEnd::Stopped => self.back(cx, &t),
                StepEnd::Exited(_) if unsaved => self.back(cx, &t),
                StepEnd::Exited(_) => self.errored(cx, &t, NO_RESULT, None),
                StepEnd::TimedOut => self.errored(cx, &t, &timed_out, None),
                StepEnd::Failed(why) => self.errored(cx, &t, cut_start(why, MESSAGE_MAX), None),
            }
        }
    }

    fn go(&mut self, provider: &str, ask: Ask, sel: Option<Vec<String>>) -> Run {
        let id = self.next_run;
        self.next_run += 1;
        let purpose = if ask == Ask::Discover { Purpose::Listing } else { Purpose::Tests };
        let mut cx = self.cx(provider, id, purpose, sel);
        if self.told(provider) == Some((String::new(), None)) {
            // It said it has nothing to run here: Fenix doesn't show it or
            // ask it (Fenix §6.3 `test-provider-set`).
            self.note(format!("{provider} has nothing to run in this project: not asked"));
            return cx.run;
        }
        if purpose == Purpose::Tests {
            // Every test the run has is queued (Fenix §4 "States").
            for t in self.universe(&cx) {
                self.set_live(&mut cx, &t, State::Queued);
            }
        }
        let planned = self.timed("tests-plan", |e| e.tests_plan(provider, id, ask.clone()));
        match planned {
            Err(e) => {
                // Its tests in the run are errored with it (Decisions 12).
                let e = cut_start(&e, MESSAGE_MAX).to_string();
                if purpose == Purpose::Tests {
                    for t in self.universe(&cx) {
                        if !cx.results.contains(&t) {
                            self.errored(&mut cx, &t, &e, None);
                        }
                    }
                }
                cx.run.error = Some(e);
                self.end_run(&mut cx);
                cx.run
            }
            Ok(plan) => {
                cx.run.list_after = plan.list_after;
                let steps = self.bound_plan(plan);
                self.execute(&mut cx, steps);
                cx.run
            }
        }
    }

    /// A plan held to Fenix's limits (Fenix §6.5): steps past 256 and
    /// covers past 10,000 ids left out, ids cut, labels cut.
    fn bound_plan(&mut self, plan: Plan) -> Vec<Step> {
        let mut steps = plan.steps;
        if steps.len() > STEPS_MAX {
            self.violate(format!("a plan of {} steps; Fenix runs {STEPS_MAX}, the rest left out", steps.len()));
            steps.truncate(STEPS_MAX);
        }
        let mut ids = 0usize;
        for s in &mut steps {
            let room = COVERS_MAX.saturating_sub(ids);
            if s.covers.len() > room {
                self.violate(format!("more than {COVERS_MAX} ids in the plan's covers; Fenix leaves the rest out"));
                s.covers.truncate(room);
            }
            ids += s.covers.len();
            for c in &mut s.covers {
                if c.len() > ID_MAX {
                    *c = cut_id(c).into_owned();
                    self.note("an id over 1 KiB was cut to 1 KiB, ending in a hash of the whole");
                }
            }
            if s.label.chars().count() > LABEL_MAX {
                s.label = cut_chars(&s.label, LABEL_MAX).to_string();
                self.note("a step's label over 256 characters was cut");
            }
            if s.needs.len() > NEEDS_MAX {
                self.violate(format!("a step needs {} files; Fenix copies {NEEDS_MAX}, the rest left out", s.needs.len()));
                s.needs.truncate(NEEDS_MAX);
            }
            let mut env = 0;
            let before = s.command.env.len();
            s.command.env.retain(|(k, v)| {
                env += k.len() + v.len();
                env <= ENV_MAX
            });
            if s.command.env.len() < before {
                self.violate("a step's env over 64 KiB: Fenix leaves the rest out".to_string());
            }
        }
        steps
    }

    /// A plan's steps run in turn, as Fenix runs them (Fenix §6.4, §8.5).
    fn execute(&mut self, cx: &mut Cx, steps: Vec<Step>) {
        cx.covers = steps.iter().map(|s| s.covers.iter().cloned().collect()).collect();
        cx.named_by_steps = cx.covers.iter().flatten().cloned().collect();
        if steps.is_empty() {
            // A plan of no steps: its ask is judged at once as if it
            // exited (Decisions 13); a listing of none isn't complete.
            if cx.purpose == Purpose::Tests {
                let all = self.universe(cx);
                self.judge(cx, all, &StepEnd::Exited(Some(0)));
            }
            self.end_part(cx, false);
            self.end_run(cx);
            return;
        }
        let last = steps.len() - 1;
        for (i, step) in steps.into_iter().enumerate() {
            let provider = cx.provider.clone();
            let run = cx.run.id;
            if cx.halted.is_some() {
                // Steps after a stop never start: each is told `stopped`,
                // and what it says is dropped (Fenix §6.4 "Stopping").
                let _ = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, StepEnd::Stopped));
                cx.run.steps.push(Ran { command: step.command, label: step.label, covers: step.covers, needs: step.needs, end: StepEnd::Stopped, lines: 0, calls: 0 });
                continue;
            }
            self.start_step(cx, i);
            cx.cwd = step.command.cwd.clone();
            let mut ran = Ran { command: step.command.clone(), label: step.label.clone(), covers: step.covers.clone(), needs: step.needs.clone(), end: StepEnd::Exited(Some(0)), lines: 0, calls: 0 };
            if let Some(why) = self.check_step(&step) {
                // Fenix wouldn't start it: it ends `failed`, its tests
                // errored with why, and the plan goes on (Decisions 13).
                self.violate(why.clone());
                let end = StepEnd::Failed(why);
                let reports = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, end.clone()));
                self.apply(cx, i, reports);
                self.step_ended(cx, i, i == last, &end);
                ran.end = end;
                cx.run.steps.push(ran);
                continue;
            }
            let Some(recorded) = self.rules.iter().rev().find_map(|r| r(&step)) else {
                panic!("no recording for step {i} of run {run}: {} (give one with on_step, on_command or on_recorded)", command_text(&step.command));
            };
            let all = interleave(lines(&recorded.stdout, false, self.cut_at), lines(&recorded.stderr, true, self.cut_at), &self.order, recorded.order.as_deref());
            let (stop_at, stop_end) = recorded.stop.clone().map_or((usize::MAX, None), |(n, e)| (n, Some(e)));
            let restart_at = recorded.restart.unwrap_or(usize::MAX);
            let given: Vec<Line> = all.into_iter().take(stop_at.min(restart_at)).collect();
            for batch in batches(given, self.batch) {
                ran.lines += batch.len();
                ran.calls += 1;
                let n = batch.len();
                let reports = self.timed(&format!("tests-read of {n} lines"), |e| e.tests_read(&provider, run, i as u32, batch));
                self.apply(cx, i, reports);
            }
            if recorded.restart.is_some() {
                // As after a trap: the run is stopped, its tests back to
                // what they were, and no call goes to the new instance
                // about it (Fenix §6.4, "One project", "After a trap";
                // Decisions 15).
                let mut make = self.restart.take().expect("a recording restarts the pack: give Fenix::on_restart");
                self.ext = make();
                self.restart = Some(make);
                let data = self.data.to_string_lossy().into_owned();
                self.ext.tests_started(&data);
                cx.halted = Some(StepEnd::Stopped);
                cx.run.stopped = Some(format!("stopped: {provider} started again"));
                ran.end = StepEnd::Stopped;
                cx.run.steps.push(ran);
                self.end_run(cx);
                return;
            }
            let end = stop_end.unwrap_or(recorded.end.clone());
            let reports = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, end.clone()));
            self.apply(cx, i, reports);
            self.step_ended(cx, i, i == last, &end);
            if matches!(end, StepEnd::Stopped | StepEnd::TimedOut) {
                cx.run.stopped = Some(if end == StepEnd::Stopped { "stopped".into() } else { self.timed_out() });
                cx.halted = Some(end.clone());
            }
            ran.end = end;
            cx.run.steps.push(ran);
        }
        self.end_part(cx, true);
        self.end_run(cx);
    }

    /// A step starts: its tests running -- what it covers, or, at the
    /// first step that claims nothing, the plan's rest (Fenix §8.5) --
    /// until the runner says when each starts (Decisions 9).
    fn start_step(&mut self, cx: &mut Cx, step: usize) {
        cx.started_seen = false;
        if cx.purpose != Purpose::Tests {
            return;
        }
        let empty = cx.covers[step].is_empty();
        if empty && cx.rest_running {
            return;
        }
        cx.rest_running |= empty;
        for t in self.step_scope(cx, step) {
            if !cx.results.contains(&t) {
                self.set_live(cx, &t, State::Running);
            }
        }
    }

    /// Step `step` ended: what it covered without a result judged, and,
    /// at the plan's last step, the plan's rest (Fenix §8.5).
    fn step_ended(&mut self, cx: &mut Cx, step: usize, last: bool, end: &StepEnd) {
        cx.all_exited &= matches!(end, StepEnd::Exited(_));
        cx.all_zero &= *end == StepEnd::Exited(Some(0));
        if cx.purpose != Purpose::Tests {
            return;
        }
        let mut judged = Vec::new();
        if !cx.covers[step].is_empty() {
            judged.extend(self.step_scope(cx, step));
        }
        if last {
            let rest: Vec<String> = self.universe(cx).into_iter().filter(|l| !self.in_covers(cx, l, &cx.named_by_steps)).collect();
            judged.extend(rest);
        }
        self.judge(cx, judged, end);
    }

    /// The plan is done: its records dropped as their sources' rules say
    /// (Fenix §4 "What's dropped").
    fn end_part(&mut self, cx: &mut Cx, stepped: bool) {
        let complete = cx.halted.is_none() && cx.run.error.is_none() && stepped && cx.all_exited && !cx.broken && !cx.crashed;
        cx.run.complete = complete;
        cx.run.lists_after = cx.run.list_after && cx.purpose == Purpose::Tests && cx.halted.is_none() && cx.run.error.is_none() && cx.all_exited && !cx.broken;
        if !complete {
            return;
        }
        let model = self.models.entry(cx.provider.clone()).or_default();
        match cx.purpose {
            Purpose::Listing => {
                let gone: Vec<String> = model.nodes.iter().filter(|(k, n)| n.records[Source::Listing as usize].is_some() && !cx.named.contains(*k)).map(|(k, _)| k.clone()).collect();
                for g in gone {
                    model.drop_record(&g, Source::Listing, None, false);
                }
            }
            Purpose::Tests => {
                // A run's records go for what's strictly under a node the
                // run asked for, or all after `run-all` (Decisions 24).
                let gone: Vec<String> = model
                    .nodes
                    .iter()
                    .filter(|(k, n)| n.records[Source::Run as usize].is_some() && !cx.named.contains(*k))
                    .filter(|(k, _)| match &cx.sel {
                        None => true,
                        Some(sel) => model.ancestry(k).iter().skip(1).any(|a| sel.contains(a)),
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                for g in gone {
                    // A case its template's run dropped goes with the
                    // result this run judged it (Decisions 31).
                    let only_run = model.nodes.get(&g).is_some_and(|n| n.scans.is_empty() && n.records[Source::Listing as usize].is_none());
                    if only_run {
                        cx.results.remove(&g);
                    }
                    model.drop_record(&g, Source::Run, None, only_run);
                }
            }
            Purpose::Build => {}
        }
    }

    /// The run's end: what hadn't ended back to what it was, or, after a
    /// timeout, errored (Fenix §8.3; Decisions 14).
    fn end_run(&mut self, cx: &mut Cx) {
        let timed_out = self.timed_out();
        let mut overlaid: Vec<String> = cx.overlaid.iter().cloned().collect();
        overlaid.sort();
        for t in overlaid {
            let live = self.model(&cx.provider).nodes.get(&t).is_some_and(|n| matches!(n.state, State::Queued | State::Running));
            if !live {
                continue;
            }
            if cx.halted == Some(StepEnd::TimedOut) && self.model(&cx.provider).is_leaf(&t) && !cx.results.contains(&t) {
                self.errored(cx, &t, &timed_out, None);
            } else {
                self.back(cx, &t);
            }
        }
    }

    /// Why Fenix wouldn't start `step`, if it wouldn't (Fenix §5, §6.5).
    fn check_step(&mut self, step: &Step) -> Option<String> {
        let Some(programs) = &self.programs else {
            panic!("tell the driver the programs the manifest allows: Fenix::manifest or Fenix::programs");
        };
        let cmd = &step.command;
        let program = cmd.program.trim();
        let allowed = if program.is_empty() {
            false
        } else if !program.contains(['/', '\\']) {
            let stem = program.strip_suffix(".exe").unwrap_or(program);
            programs.iter().any(|p| p.eq_ignore_ascii_case(stem))
        } else {
            let slashed = program.replace('\\', "/");
            self.found.iter().any(|f| f.replace('\\', "/") == slashed)
                || (["/data/", "/tools/"].iter().any(|r| slashed.starts_with(r))
                    && programs.iter().any(|p| match p.strip_suffix('*') {
                        Some(prefix) => slashed.starts_with(prefix),
                        None => p.replace('\\', "/") == slashed,
                    }))
                || programs.iter().filter_map(|p| p.strip_prefix("${setting:").and_then(|k| k.strip_suffix('}'))).filter_map(|k| self.settings.get(k)).any(|v| v.trim() == program)
        };
        if !allowed {
            return Some(format!("{program} isn't a program this extension may run (permissions.process)"));
        }
        let batch = self.batch_files.iter().any(|b| b == program);
        let resolved = resolved(cmd, batch);
        let (len, limit) = if batch {
            match batch_file_line_len(&resolved) {
                Some(len) => (len, BATCH_FILE_MAX),
                None => return Some("Rust can't give one of its arguments to a batch file (a line break, or the script's path)".to_string()),
            }
        } else {
            (command_line_len(&resolved), WINDOWS_MAX)
        };
        if len > limit {
            return Some(format!("its command line is {} characters; Windows takes {}", thousands(len), thousands(limit)));
        }
        for need in &step.needs {
            // A `/data` file's path with `/`, there when it starts
            // (`fenix_wasm::test_limits::need`).
            if need.len() > PATH_MAX {
                return Some(format!("needs a path of {} bytes; a need is at most 4 KiB", need.len()));
            }
            if need.contains('\\') {
                return Some(format!("needs {need}, which isn't a /data path with /"));
            }
            match need.strip_prefix("/data/") {
                Some(rest) if !rest.is_empty() && !rest.split('/').any(|p| p.is_empty() || p == "." || p == "..") => {
                    if !self.data.join(rest).is_file() {
                        return Some(format!("needs {need}, which isn't there"));
                    }
                }
                _ => return Some(format!("needs {need}, which isn't a file's path under /data")),
            }
        }
        None
    }

    /// A call's reports applied in order, as Fenix applies them (Fenix
    /// §4, §8.5; `fenix-tests`' `apply`).
    fn apply(&mut self, cx: &mut Cx, step: usize, mut reports: Vec<Report>) {
        if reports.len() > REPORTS_MAX {
            self.violate(format!("a call gave {} reports; Fenix takes {REPORTS_MAX}", reports.len()));
            reports.truncate(REPORTS_MAX);
        }
        let provider = cx.provider.clone();
        for report in reports {
            match report {
                Report::Found(f) => {
                    let source = match cx.purpose {
                        Purpose::Tests => Source::Run,
                        Purpose::Listing => Source::Listing,
                        // Ignored in a build (Decisions 17).
                        Purpose::Build => continue,
                    };
                    let path = f.path.as_deref().map(|p| place(p, cx.cwd.as_deref()));
                    if let Some(id) = self.set_found(&provider, f, path, source, None) {
                        cx.named.insert(id);
                    }
                }
                Report::Started(id) => {
                    let id = cut_id(&id).into_owned();
                    if cx.purpose != Purpose::Tests || !self.model(&provider).has(&id) {
                        continue;
                    }
                    cx.named.insert(id.clone());
                    if !cx.started_seen {
                        // The runner says when each starts: the rest of the
                        // step's go back to queued (Decisions 9).
                        cx.started_seen = true;
                        let running: Vec<String> = cx.overlaid.iter().filter(|t| self.model(&provider).nodes.get(*t).is_some_and(|n| n.state == State::Running)).cloned().collect();
                        for t in running {
                            self.set_live(cx, &t, State::Queued);
                        }
                    }
                    self.set_live(cx, &id, State::Running);
                }
                Report::Ended(e) => {
                    if cx.purpose != Purpose::Tests {
                        continue;
                    }
                    self.ended(cx, e);
                }
                Report::Output { id: Some(id), text } => {
                    let id = cut_id(&id).into_owned();
                    if cx.purpose != Purpose::Tests || !self.model(&provider).has(&id) {
                        continue;
                    }
                    cx.named.insert(id.clone());
                    let text = cut_end(&text, TEXT_MAX);
                    if cx.results.contains(&id) {
                        let n = self.model_mut(&provider).nodes.get_mut(&id).expect("known");
                        n.printed.push_str(text);
                        n.printed = cut_end(&n.printed, TEXT_MAX).to_string();
                    } else {
                        let printed = cx.printed.entry(id).or_default();
                        printed.push_str(text);
                        *printed = cut_end(printed, TEXT_MAX).to_string();
                    }
                }
                Report::Output { id: None, .. } => {}
                Report::Broken { id, message, at } => {
                    let message = cut_start(&message, MESSAGE_MAX).to_string();
                    cx.run.broken.push((id.clone(), message.clone()));
                    cx.broken = true;
                    if cx.purpose == Purpose::Tests {
                        let at = at.map(|a| (place(&a.path, cx.cwd.as_deref()), a.line, a.column));
                        self.errored_under(cx, step, id, &message, at, "broken");
                    }
                }
                Report::Crashed { id, message } => {
                    let message = cut_start(&message, MESSAGE_MAX).to_string();
                    cx.run.crashed.push((id.clone(), message.clone()));
                    cx.crashed = true;
                    if cx.purpose == Purpose::Tests {
                        self.errored_under(cx, step, id, &message, None, "crashed");
                    }
                }
            }
        }
    }

    /// `ended` (Fenix §4 "Ids, and matching"): a known id's result; an
    /// unknown one added as a test labelled by its id, found in a run,
    /// under the deepest node of the file its `at` names, else at its
    /// provider's top (Decisions 21).
    fn ended(&mut self, cx: &mut Cx, e: api::Ended) {
        let provider = cx.provider.clone();
        let id = cut_id(&e.id).into_owned();
        if id.len() < e.id.len() {
            self.note("an id over 1 KiB was cut to 1 KiB, ending in a hash of the whole");
        }
        let mut message = e.message;
        if let Some(m) = &mut message {
            if m.len() > MESSAGE_MAX {
                *m = cut_start(m, MESSAGE_MAX).to_string();
                self.note("a message over 4 KiB was cut");
            }
        }
        let mut text = e.text;
        if let Some(t) = &mut text {
            if t.len() > TEXT_MAX {
                *t = cut_end(t, TEXT_MAX).to_string();
                self.note("a failure's text over 64 KiB was cut");
            }
        }
        let at = e.at.as_ref().map(|a| (cut_start(&place(&a.path, cx.cwd.as_deref()), PATH_MAX).to_string(), a.line, a.column));
        if !self.model(&provider).has(&id) {
            let parent = at.as_ref().and_then(|(p, l, _)| self.deepest(&provider, p, *l));
            let record = Record { parent, label: cut_chars(&id, LABEL_MAX).to_string(), kind: Kind::Test, path: None, line: None, end_line: None, skip: None, template: false, data: None };
            let model = self.model_mut(&provider);
            model.set(&id, Source::Run, None, record);
            model.nodes.get_mut(&id).expect("set").found_in_run = true;
        }
        cx.named.insert(id.clone());
        let duration_ms = e.duration_ms.filter(|d| d.is_finite() && *d >= 0.0);
        self.give_result(cx, &id, e.outcome, Detail { message, at, text, duration_ms });
    }

    /// A `broken` or `crashed` (Fenix §8.5, §19 K5 and K6; Decisions 11):
    /// what's under its id and has no result in this run yet is errored;
    /// with no id, or one Fenix doesn't have (said), the step's scope --
    /// for a step that claims nothing, the plan's rest -- that has none.
    fn errored_under(&mut self, cx: &mut Cx, step: usize, id: Option<String>, message: &str, at: Option<(String, u32, Option<u32>)>, what: &str) {
        let id = id.map(|i| cut_id(&i).into_owned());
        let known = id.as_ref().filter(|i| self.model(&cx.provider).has(i)).cloned();
        if let (Some(i), None) = (&id, &known) {
            self.note(format!("a `{what}` named {}, which it hasn't found: taken as all the step covers", cut_chars(i, 200)));
        }
        let targets: Vec<String> = match known {
            Some(node) => self.model(&cx.provider).tests_under(&node).into_iter().filter(|l| self.in_universe(cx, l)).collect(),
            None => self.step_scope(cx, step),
        };
        for t in targets {
            if !cx.results.contains(&t) {
                self.errored(cx, &t, message, at.clone());
            }
        }
    }

    /// The deepest group, file or suite of file `path`, preferring one
    /// whose lines hold `line` (from 0); between equals, the one that
    /// starts first, then the id that sorts first (Decisions 21).
    fn deepest(&self, provider: &str, path: &str, line: u32) -> Option<String> {
        let model = self.model(provider);
        model
            .nodes
            .keys()
            .filter_map(|k| {
                let r = model.record(k)?;
                if model::is_test(r.kind) || r.path.as_deref() != Some(path) {
                    return None;
                }
                let holds = match (r.line, r.end_line) {
                    (Some(s), Some(e)) => s <= line && line <= e,
                    (Some(s), None) => s <= line,
                    _ => false,
                };
                Some(((holds, model.depth(k), std::cmp::Reverse((r.line.unwrap_or(u32::MAX), k.clone()))), k.clone()))
            })
            .max()
            .map(|(_, k)| k)
    }

    /// How to debug `id` (Fenix §11): `tests-launch` with the node as
    /// Fenix gives it back, and a `build-first` plan run as the same run
    /// (its `found`s and `ended`s ignored), then asked again once, with
    /// that run, only if every step exited 0 and nothing was broken or
    /// crashed. A node whose id was cut and whose `data` isn't kept isn't
    /// debugged (Decisions 32).
    pub fn launch(&mut self, provider: &str, id: &str) -> Launched {
        let test = self.given_back(provider, id).unwrap_or_else(|| panic!("Fenix has no {id} to debug"));
        if !self.model(provider).can_be_named(id) {
            return Launched { config: Err(format!("{provider} can't name {id:.40}… without its data")), built: None };
        }
        let run = self.next_run;
        self.next_run += 1;
        let first = self.timed("tests-launch", |e| e.tests_launch(provider, run, test.clone()));
        let plan = match first {
            Err(e) => return Launched { config: Err(e), built: None },
            Ok(api::Launch::Launch(c)) => return Launched { config: Ok(c), built: None },
            Ok(api::Launch::BuildFirst(plan)) => plan,
        };
        let mut cx = self.cx(provider, run, Purpose::Build, Some(Vec::new()));
        let steps = self.bound_plan(plan);
        self.execute(&mut cx, steps);
        let ok = cx.halted.is_none() && cx.all_zero && !cx.broken && !cx.crashed && !cx.run.steps.is_empty();
        let built = cx.run;
        if !ok {
            let why = built.broken.first().map(|(_, m)| m.clone()).or_else(|| built.crashed.first().map(|(_, m)| m.clone())).or_else(|| built.steps.iter().find(|s| s.end != StepEnd::Exited(Some(0))).map(|s| format!("{} ended {:?}", s.label, s.end)));
            return Launched { config: Err(why.unwrap_or_default()), built: Some(built) };
        }
        let again = self.timed("tests-launch", |e| e.tests_launch(provider, run, test));
        let config = match again {
            Ok(api::Launch::Launch(c)) => Ok(c),
            Ok(api::Launch::BuildFirst(_)) => Err("it asked to build twice".to_string()),
            Err(e) => Err(e),
        };
        Launched { config, built: Some(built) }
    }

    /// The tree Fenix holds for `provider`, as text: a node a line, in the
    /// page's order, each with its kind, its first line (from 1), its
    /// state and its result, and which sources have it.
    pub fn tree(&self, provider: &str) -> String {
        let model = self.model(provider);
        let mut out = String::new();
        let mut stack: Vec<(String, usize)> = model.children(None).into_iter().rev().map(|k| (k, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            let n = &model.nodes[&id];
            let r = n.merged().expect("a node has a record");
            out.push_str(&"  ".repeat(depth));
            out.push_str(&r.label);
            match (r.kind, r.template) {
                (Kind::Group, _) => out.push_str(" [group]"),
                (Kind::File, _) => out.push_str(" [file]"),
                (Kind::Suite, _) => out.push_str(" [suite]"),
                (Kind::Case, _) => out.push_str(" [case]"),
                (Kind::Test, true) => out.push_str(" [template]"),
                (Kind::Test, false) => {}
            }
            if let Some(l) = r.line {
                out.push_str(&format!(" :{}", l + 1));
            }
            out.push_str(&format!(" -- {}", model.state(&id).name()));
            if let Some(ms) = n.result.duration_ms {
                out.push_str(&format!(" {ms}ms"));
            }
            if let Some(why) = r.skip.as_ref().filter(|_| model::is_test(r.kind)) {
                out.push_str(&format!(" (skip: {why})"));
            }
            if let Some(m) = &n.result.message {
                out.push_str(&format!(": {}", m.lines().next().unwrap_or("")));
            }
            if let Some((p, l, c)) = &n.result.at {
                out.push_str(&format!(" at {p}:{}", l + 1));
                if let Some(c) = c {
                    out.push_str(&format!(":{}", c + 1));
                }
            }
            if n.found_in_run {
                out.push_str(" (found in a run)");
            }
            let sources: Vec<&str> = ["scan", "listing", "run"].iter().zip(&n.records).filter(|(_, r)| r.is_some()).map(|(s, _)| *s).collect();
            out.push_str(&format!("  <{}>\n", sources.join("+")));
            for k in model.children(Some(&id)).into_iter().rev() {
                stack.push((k, depth + 1));
            }
        }
        out
    }
}

/// The program and args, joined by spaces.
pub fn command_text(cmd: &Command) -> String {
    std::iter::once(cmd.program.as_str()).chain(cmd.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
}

/// `cmd` as Fenix counts its command line (Fenix §19, K2): `${data}` and
/// `${tools}` filled in, the program as a full path of a Windows machine
/// -- a name under [`PROGRAMS_ON_WINDOWS`] (a `.cmd` for a batch file, an
/// `.exe` else), `/data/...` and `/tools/...` under the folders above, a
/// full path as it is.
fn resolved(cmd: &Command, batch: bool) -> Command {
    let fill = |s: &str| s.replace("${data}", DATA_ON_WINDOWS).replace("${tools}", TOOLS_ON_WINDOWS);
    let mut c = cmd.clone();
    let program = fill(cmd.program.trim());
    c.program = if let Some(rest) = program.strip_prefix("/data/") {
        format!("{DATA_ON_WINDOWS}\\{}", rest.replace('/', "\\"))
    } else if let Some(rest) = program.strip_prefix("/tools/") {
        format!("{TOOLS_ON_WINDOWS}\\{}", rest.replace('/', "\\"))
    } else if program.contains(['/', '\\']) {
        program
    } else {
        let stem = program.strip_suffix(".exe").unwrap_or(&program);
        format!("{PROGRAMS_ON_WINDOWS}\\{stem}\\{stem}.{}", if batch { "cmd" } else { "exe" })
    };
    for a in &mut c.args {
        *a = fill(a);
    }
    c
}

/// `permissions.process`'s strings, from a manifest's text.
fn process_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section = "";
    let mut inside = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && !inside {
            section = if line == "[permissions]" { "permissions" } else { "" };
            continue;
        }
        let rest = if inside {
            line
        } else if section == "permissions" && line.starts_with("process") && line.contains('=') {
            inside = true;
            line.split_once('=').map(|(_, r)| r.trim()).unwrap_or("")
        } else {
            continue;
        };
        for part in rest.split('"').skip(1).step_by(2) {
            out.push(part.to_string());
        }
        if rest.contains(']') {
            inside = false;
        }
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn a_manifests_programs_are_read() {
        let text = "[permissions]\n# what it runs\nprocess = [\n    \"cargo\", \"rustup\", # tools\n    \"${setting:rust.codelldb}\",\n]\nnetwork = [\"x\"]\n[contributes]\nprocess = [\"no\"]\n";
        assert_eq!(process_list(text), ["cargo", "rustup", "${setting:rust.codelldb}"]);
    }

    #[test]
    fn paths_are_mapped_as_fenix_maps_them() {
        assert_eq!(place("/project/src/a.rs", Some("/project/sub")), "src/a.rs");
        assert_eq!(place("src/a.rs", Some("/project/crates/orbit")), "crates/orbit/src/a.rs");
        assert_eq!(place("./src\\a.rs", None), "src/a.rs");
        assert_eq!(place(r"C:\src\a.rs", None), r"C:\src\a.rs");
        assert_eq!(place(r"/project/src\a.rs", None), "src/a.rs");
    }

    /// Fenix §19, K2: the line counted after `${data}` is filled in, with
    /// the program's full path.
    #[test]
    fn a_command_line_is_counted_with_its_program_resolved() {
        let cmd = Command::new("pytest").args(["-p", "${data}/r.py"]);
        let r = resolved(&cmd, false);
        assert_eq!(r.program, format!(r"{PROGRAMS_ON_WINDOWS}\pytest\pytest.exe"));
        assert_eq!(r.args[1], format!("{DATA_ON_WINDOWS}/r.py"));
        assert!(resolved(&Command::new("npx"), true).program.ends_with("npx.cmd"));
        assert_eq!(resolved(&Command::new("/tools/zls/zls"), false).program, format!(r"{TOOLS_ON_WINDOWS}\zls\zls"));
        assert_eq!((thousands(32_766), thousands(41_203), thousands(999)), ("32,766".into(), "41,203".into(), "999".into()));
    }
}
