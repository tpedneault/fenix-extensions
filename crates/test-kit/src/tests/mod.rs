//! A stand-in for Fenix's side of a test provider (extension API 1.6),
//! so a pack's provider is tested natively on its runner's recorded
//! output: [`Fenix`] scans a file's text, asks for a plan, runs each step
//! by giving the provider recorded lines as Fenix would, ends it, and
//! keeps what Fenix would then hold -- the tests found, each one's state,
//! what didn't build -- by Fenix's rules as its plan states them
//! (Fenix's `docs/plans/test-explorer.md`, cited as *Fenix §n*). Where
//! that plan doesn't say, it takes the strictest reading, and the README
//! lists each under "Not yet specified by Fenix".
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
use fenix_lang_kit::tests::{command_line_len, cut_id, BATCH_FILE_MAX, ID_MAX, LABEL_MAX, WINDOWS_MAX};
use fenix_lang_kit::Kit;

use api::{Ask, Found, Kind, Line, Plan, Report, Step, StepEnd};
pub use feed::{batches, interleave, lines, strip_ansi, Order, BATCH_BYTES, BATCH_LINES, CUT_AT};
pub use model::{Detail, Model, Node, Record, Source, State};

use crate::Fake;

/// What `${data}` and `${tools}` count as in a step's command line: a
/// data folder's length on Windows, since Fenix fills them in before it
/// starts the step (and its plan doesn't say whether its limit is
/// counted before).
pub const DATA_ON_WINDOWS: &str = r"C:\Users\someone\AppData\Roaming\Fenix\data\extensions\fenix.pack";

/// Fenix's limits (Fenix §6.5).
const FOUND_MAX: usize = 10_000;
const REPORTS_MAX: usize = 10_000;
const STEPS_MAX: usize = 256;
const COVERS_MAX: usize = 10_000;
const NEEDS_MAX: usize = 32;
const ENV_MAX: usize = 64 * 1024;
const PATH_MAX: usize = 4 * 1024;
const DATA_MAX: usize = 4 * 1024;
const ASK_MAX: usize = 10_000;
const PRINTED_MAX: usize = 64 * 1024;

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
    /// A `build-first` plan before a launch: its `ended`s are ignored.
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
    pub list_after: bool,
    /// Each `broken` and `crashed`: its id and message.
    pub broken: Vec<(Option<String>, String)>,
    pub crashed: Vec<(Option<String>, String)>,
    /// What the page's head says when it was stopped.
    pub stopped: Option<String>,
    /// Every step exited and nothing was broken or crashed (Fenix §4).
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

/// Run state Fenix keeps while a plan runs.
struct Cx {
    provider: String,
    purpose: Purpose,
    queued: Vec<String>,
    /// Each test's state before the run, to go back to on a stop.
    before: HashMap<String, (State, Detail)>,
    ended: HashSet<String>,
    found: HashSet<String>,
    listed: HashSet<String>,
    /// The step's covers and what's ended in it.
    covers: Vec<String>,
    step_ended: HashSet<String>,
    /// Every step's covered tests so far.
    covered: HashSet<String>,
    cwd: Option<String>,
    run: Run,
}

/// `path` as Fenix maps it: `/project/x` and a relative one (to `base`,
/// below the project) as `x`; a full path as it is.
fn place(path: &str, base: Option<&str>) -> String {
    if let Some(rest) = path.strip_prefix("/project/") {
        return rest.to_string();
    }
    let full = path.starts_with(['/', '\\']) || (path.len() > 1 && path.as_bytes()[1] == b':');
    if full {
        return path.to_string();
    }
    let rel = path.replace('\\', "/");
    let rel = rel.strip_prefix("./").unwrap_or(&rel);
    match base.map(|b| b.strip_prefix("/project").unwrap_or(b).trim_matches('/')) {
        Some(b) if !b.is_empty() => format!("{b}/{rel}"),
        _ => rel.to_string(),
    }
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

    /// A program that's a `.cmd` or `.bat` on Windows: its command line is
    /// held to 8,191.
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

    /// Breaking Fenix's rules is said (as on the provider's output page,
    /// [`Fenix::said`]) and handled as Fenix does, instead of failing the
    /// test.
    pub fn lenient(&mut self) -> &mut Self {
        self.strict = false;
        self
    }

    /// What Fenix said on the provider's output page.
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

    fn violate(&mut self, what: String) {
        if self.strict {
            panic!("against Fenix's rules: {what}");
        }
        self.said.push(what);
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

    /// Test `id`'s state.
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

    /// Asserts each test's state.
    #[track_caller]
    pub fn assert_states(&self, provider: &str, expected: &[(&str, State)]) {
        let got: Vec<(&str, State)> = expected.iter().map(|(id, _)| (*id, self.state(provider, id))).collect();
        assert_eq!(got, expected, "states as Fenix holds them:\n{}", self.tree(provider));
    }

    /// What the pack told Fenix it runs, last: runner and problem.
    pub fn told(&self, provider: &str) -> Option<(String, Option<String>)> {
        self.fake.as_ref()?.test_providers().into_iter().rev().find(|(p, _, _)| p == provider).map(|(_, r, p)| (r, p))
    }

    /// `tests-scan` of `path` (`/project/<path>`), whose text is `text`;
    /// its answer replaces the file's last scan (Fenix §4, §6.4).
    pub fn scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<Found> {
        assert!(path.starts_with("/project/"), "Fenix scans files as /project/<path>: {path}");
        assert!(text.len() <= 1 << 20, "Fenix doesn't scan a file over 1 MiB");
        let mut answer = self.timed("tests-scan", |e| e.tests_scan(provider, path, text));
        if answer.len() > FOUND_MAX {
            self.violate(format!("the scan of {path} gave {} tests; Fenix keeps {FOUND_MAX}", answer.len()));
            answer.truncate(FOUND_MAX);
        }
        let file = place(path, None);
        let mut ids = Vec::new();
        let mut earlier = HashSet::new();
        for f in &answer {
            let Some(id) = self.check_found(provider, f, &earlier) else { continue };
            earlier.insert(id.clone());
            let path = match (&f.path, f.kind) {
                (Some(p), _) => Some(place(p, None)),
                (None, Kind::Test | Kind::Case) => Some(file.clone()),
                (None, _) => None,
            };
            let model = self.model_mut(provider);
            model.set(&id, Source::Scan, Record::from(f, path));
            model.nodes.get_mut(&id).expect("set").scanned_by.insert(file.clone());
            ids.push(id);
        }
        let model = self.model_mut(provider);
        let now: HashSet<&String> = ids.iter().collect();
        for old in model.scans.remove(&file).unwrap_or_default() {
            if now.contains(&old) {
                continue;
            }
            if let Some(n) = model.nodes.get_mut(&old) {
                n.scanned_by.remove(&file);
                if n.scanned_by.is_empty() {
                    model.drop_record(&old, Source::Scan);
                }
            }
        }
        model.scans.insert(file, ids);
        answer
    }

    /// A `found-test` held to Fenix's limits: its id, as Fenix keeps it.
    fn check_found(&mut self, provider: &str, f: &Found, earlier: &HashSet<String>) -> Option<String> {
        if f.id.len() > ID_MAX {
            self.violate(format!("an id of {} bytes; Fenix cuts it to {ID_MAX}: {}…", f.id.len(), &cut_id(&f.id)[..40.min(f.id.len())]));
        }
        if f.label.chars().count() > LABEL_MAX {
            self.violate(format!("{}'s label is over {LABEL_MAX} characters", f.id));
        }
        if f.path.as_ref().is_some_and(|p| p.len() > PATH_MAX) || f.parent.as_ref().is_some_and(|p| p.len() > ID_MAX) || f.skip.as_ref().is_some_and(|s| s.len() > ID_MAX) {
            self.violate(format!("{}'s path, parent or skip is over Fenix's limit", f.id));
        }
        if f.data.as_ref().is_some_and(|d| d.len() > DATA_MAX) {
            self.violate(format!("{}'s data is over 4 KiB", f.id));
        }
        let id = cut_id(&f.id).into_owned();
        if earlier.contains(&id) {
            self.violate(format!("{id} twice in one scan"));
            return None;
        }
        if let Some(p) = &f.parent {
            let p = cut_id(p);
            if !self.model(provider).has(&p) && !earlier.contains(p.as_ref()) {
                self.violate(format!("{id} came before its parent {p}: a group comes before its children (Fenix §6.5)"));
            }
        }
        if f.id.starts_with('\u{1f}') != matches!(f.kind, Kind::Group | Kind::File | Kind::Suite) {
            self.violate(format!("{id} is a {:?}, but lang-kit gives groups' ids a U+001F before each label and tests' none", f.kind));
        }
        Some(id)
    }

    /// The ask Fenix makes to run `ids` (Fenix §4, "What the provider is
    /// asked"): each node as it has it now, each file or suite followed by
    /// the tests and cases under it while the ask stays within 10,000
    /// records; more than 10,000 asked as their parents.
    pub fn ask_for(&self, provider: &str, ids: &[&str]) -> Ask {
        let model = self.model(provider);
        let mut sel: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        while sel.len() > ASK_MAX {
            let mut parents: Vec<String> = Vec::new();
            for id in &sel {
                match model.record(id).and_then(|r| r.parent) {
                    Some(p) if !parents.contains(&p) => parents.push(p),
                    Some(_) => {}
                    None => return Ask::RunAll,
                }
            }
            sel = parents;
        }
        let mut records = Vec::new();
        for id in &sel {
            let Some(r) = model.record(id) else { continue };
            let kind = r.kind;
            records.push(api_found(id, &r));
            if matches!(kind, Kind::File | Kind::Suite) {
                let under = model.tests_under(id);
                if records.len() + under.len() <= ASK_MAX {
                    records.extend(under.iter().filter_map(|t| model.record(t).map(|r| api_found(t, &r))));
                }
            }
        }
        Ask::Run(records)
    }

    /// A listing (`discover`).
    pub fn discover(&mut self, provider: &str) -> Run {
        self.go(provider, Ask::Discover)
    }

    /// Every test.
    pub fn run_all(&mut self, provider: &str) -> Run {
        self.go(provider, Ask::RunAll)
    }

    /// These tests, files or groups.
    pub fn run(&mut self, provider: &str, ids: &[&str]) -> Run {
        let ask = self.ask_for(provider, ids);
        self.go(provider, ask)
    }

    /// The failed again (Fenix §8.4): every test failed or errored.
    pub fn run_failed(&mut self, provider: &str) -> Run {
        let failed: Vec<String> = self.model(provider).nodes.iter().filter(|(_, n)| matches!(n.state, State::Failed | State::Errored)).map(|(k, _)| k.clone()).collect();
        let ids: Vec<&str> = failed.iter().map(String::as_str).collect();
        self.run(provider, &ids)
    }

    /// The ids failed or errored now.
    pub fn failed(&self, provider: &str) -> Vec<String> {
        self.model(provider).nodes.iter().filter(|(_, n)| matches!(n.state, State::Failed | State::Errored)).map(|(k, _)| k.clone()).collect()
    }

    fn go(&mut self, provider: &str, ask: Ask) -> Run {
        let id = self.next_run;
        self.next_run += 1;
        let model = self.model(provider);
        let queued: Vec<String> = match &ask {
            Ask::Discover => Vec::new(),
            Ask::RunAll => model.all_tests(),
            Ask::Run(records) => {
                let mut q: Vec<String> = Vec::new();
                let mut seen = HashSet::new();
                for r in records {
                    for t in model.tests_under(&r.id) {
                        if seen.insert(t.clone()) {
                            q.push(t);
                        }
                    }
                }
                q
            }
        };
        let purpose = if ask == Ask::Discover { Purpose::Listing } else { Purpose::Tests };
        let mut cx = self.cx(provider, id, purpose, queued);
        for t in cx.queued.clone() {
            self.set_state(&mut cx, &t, State::Queued);
        }
        let planned = self.timed("tests-plan", |e| e.tests_plan(provider, id, ask.clone()));
        match planned {
            Err(e) => {
                let queued = cx.queued.clone();
                self.back(&mut cx, &queued);
                cx.run.error = Some(e);
                cx.run.complete = false;
                cx.run
            }
            Ok(plan) => {
                cx.run.list_after = plan.list_after;
                self.execute(&mut cx, plan.steps);
                cx.run
            }
        }
    }

    fn cx(&self, provider: &str, id: u64, purpose: Purpose, queued: Vec<String>) -> Cx {
        Cx {
            provider: provider.into(),
            purpose,
            queued,
            before: HashMap::new(),
            ended: HashSet::new(),
            found: HashSet::new(),
            listed: HashSet::new(),
            covers: Vec::new(),
            step_ended: HashSet::new(),
            covered: HashSet::new(),
            cwd: None,
            run: Run { id, error: None, steps: Vec::new(), list_after: false, broken: Vec::new(), crashed: Vec::new(), stopped: None, complete: true },
        }
    }

    fn set_state(&mut self, cx: &mut Cx, id: &str, state: State) {
        let model = self.models.entry(cx.provider.clone()).or_default();
        let Some(n) = model.nodes.get_mut(id) else { return };
        cx.before.entry(id.to_string()).or_insert_with(|| (n.state, n.result.clone()));
        n.state = state;
    }

    /// Back to what they were before the run (a stop).
    fn back(&mut self, cx: &mut Cx, ids: &[String]) {
        let model = self.models.entry(cx.provider.clone()).or_default();
        for id in ids {
            if cx.ended.contains(id) {
                continue;
            }
            if let (Some(n), Some((s, d))) = (model.nodes.get_mut(id), cx.before.get(id)) {
                if matches!(n.state, State::Queued | State::Running) {
                    n.state = *s;
                    n.result = d.clone();
                }
            }
        }
    }

    fn errored(&mut self, cx: &mut Cx, id: &str, message: &str) {
        self.set_state(cx, id, State::Errored);
        let model = self.models.entry(cx.provider.clone()).or_default();
        if let Some(n) = model.nodes.get_mut(id) {
            n.result = Detail { message: Some(message.into()), at: None, text: None, duration_ms: None };
        }
        cx.ended.insert(id.to_string());
        cx.step_ended.insert(id.to_string());
    }

    /// Tests a step covered (or the plan's rest) that ended without a
    /// result, judged by how it ended (Fenix §8.5, §8.3, §8.4).
    fn judge(&mut self, cx: &mut Cx, ids: Vec<String>, end: &StepEnd) {
        let open: Vec<String> = ids.into_iter().filter(|t| !cx.ended.contains(t)).collect();
        for t in open {
            match end {
                StepEnd::Stopped => self.back(cx, std::slice::from_ref(&t)),
                StepEnd::TimedOut => self.errored(cx, &t, "timed out"),
                StepEnd::Failed(why) => self.errored(cx, &t, why),
                StepEnd::Exited(_) => {
                    let file = self.record(&cx.provider, &t).and_then(|r| r.path);
                    if file.is_some_and(|f| self.unsaved.contains(&f)) {
                        self.back(cx, std::slice::from_ref(&t));
                    } else {
                        self.errored(cx, &t, NO_RESULT);
                    }
                }
            }
        }
    }

    /// What `covers` stands for now: each id and what's under it.
    fn covered(&self, cx: &Cx, covers: &[String]) -> Vec<String> {
        let model = self.model(&cx.provider);
        let mut out = Vec::new();
        for c in covers {
            if model.has(c) {
                out.extend(model.tests_under(c));
            }
        }
        out
    }

    fn rest(&self, cx: &Cx) -> Vec<String> {
        cx.queued.iter().filter(|t| !cx.covered.contains(*t)).cloned().collect()
    }

    /// A plan's steps run in turn, as Fenix runs them (Fenix §6.4, §8.5).
    fn execute(&mut self, cx: &mut Cx, mut steps: Vec<Step>) {
        if steps.len() > STEPS_MAX {
            self.violate(format!("a plan of {} steps; Fenix runs {STEPS_MAX}", steps.len()));
            steps.truncate(STEPS_MAX);
        }
        let mut covers_total = 0;
        for s in &mut steps {
            covers_total += s.covers.len();
            if covers_total > COVERS_MAX {
                self.violate(format!("more than {COVERS_MAX} ids in the plan's covers; Fenix leaves the rest out"));
                s.covers.clear();
            }
        }
        let mut rest_running = false;
        let mut halted: Option<StepEnd> = None;
        let mut last_end = StepEnd::Exited(Some(0));
        for (i, step) in steps.into_iter().enumerate() {
            let provider = cx.provider.clone();
            let run = cx.run.id;
            if let Some(end) = &halted {
                // Steps after a stop never start: each is told `stopped`.
                let reports = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, StepEnd::Stopped));
                self.apply(cx, reports);
                cx.run.steps.push(Ran { command: step.command, label: step.label, covers: step.covers, needs: step.needs, end: StepEnd::Stopped, lines: 0, calls: 0 });
                last_end = end.clone();
                continue;
            }
            cx.covers = step.covers.iter().map(|c| cut_id(c).into_owned()).collect();
            cx.step_ended.clear();
            cx.cwd = step.command.cwd.clone();
            let mut ran = Ran { command: step.command.clone(), label: step.label.clone(), covers: step.covers.clone(), needs: step.needs.clone(), end: StepEnd::Exited(Some(0)), lines: 0, calls: 0 };
            if let Some(why) = self.check_step(&step) {
                self.violate(why.clone());
                let end = StepEnd::Failed(why);
                let reports = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, end.clone()));
                self.apply(cx, reports);
                self.end_step(cx, &end);
                ran.end = end.clone();
                cx.run.steps.push(ran);
                cx.run.complete = false;
                last_end = end;
                continue;
            }
            let Some(recorded) = self.rules.iter().rev().find_map(|r| r(&step)) else {
                panic!("no recording for step {i} of run {run}: {} (give one with on_step, on_command or on_recorded)", command_text(&step.command));
            };
            // Running: what it covers, or, the first step that claims
            // nothing, the plan's rest.
            if cx.purpose == Purpose::Tests {
                let mut running = self.covered(cx, &cx.covers.clone());
                if cx.covers.is_empty() && !rest_running {
                    rest_running = true;
                    running.extend(self.rest(cx));
                }
                for t in running {
                    if self.model(&provider).nodes.get(&t).is_some_and(|n| n.state == State::Queued) {
                        self.set_state(cx, &t, State::Running);
                    }
                }
            }
            let all = interleave(lines(&recorded.stdout, false, self.cut_at), lines(&recorded.stderr, true, self.cut_at), &self.order, recorded.order.as_deref());
            let (stop_at, stop_end) = recorded.stop.clone().map_or((usize::MAX, None), |(n, e)| (n, Some(e)));
            let restart_at = recorded.restart.unwrap_or(usize::MAX);
            let given: Vec<Line> = all.into_iter().take(stop_at.min(restart_at)).collect();
            for batch in batches(given, self.batch) {
                ran.lines += batch.len();
                ran.calls += 1;
                let n = batch.len();
                let reports = self.timed(&format!("tests-read of {n} lines"), |e| e.tests_read(&provider, run, i as u32, batch));
                self.apply(cx, reports);
            }
            if recorded.restart.is_some() {
                // As after a trap: the run is stopped, its tests back to
                // what they were, and no call goes to the new instance
                // about it (Fenix §6.4, "One project", "After a trap").
                let mut make = self.restart.take().expect("a recording restarts the pack: give Fenix::on_restart");
                self.ext = make();
                self.restart = Some(make);
                let data = self.data.to_string_lossy().into_owned();
                self.ext.tests_started(&data);
                let open: Vec<String> = cx.queued.clone();
                self.back(cx, &open);
                cx.run.stopped = Some(format!("stopped: {provider} started again"));
                cx.run.complete = false;
                ran.end = StepEnd::Stopped;
                cx.run.steps.push(ran);
                return;
            }
            let end = stop_end.unwrap_or(recorded.end.clone());
            let reports = self.timed("tests-step-ended", |e| e.tests_step_ended(&provider, run, i as u32, end.clone()));
            self.apply(cx, reports);
            self.end_step(cx, &end);
            if !matches!(end, StepEnd::Exited(_)) {
                cx.run.complete = false;
            }
            if matches!(end, StepEnd::Stopped | StepEnd::TimedOut) {
                cx.run.stopped = Some(if end == StepEnd::Stopped { "stopped".into() } else { "timed out".into() });
                halted = Some(end.clone());
            }
            ran.end = end.clone();
            cx.run.steps.push(ran);
            last_end = end;
        }
        // What of the ask no step named, judged as the plan's last step
        // ended.
        if cx.purpose == Purpose::Tests {
            let rest = self.rest(cx);
            self.judge(cx, rest, &last_end);
        }
        self.finish(cx);
    }

    /// A step ended: what it covered without a result judged.
    fn end_step(&mut self, cx: &mut Cx, end: &StepEnd) {
        if cx.purpose != Purpose::Tests {
            return;
        }
        let covered = self.covered(cx, &cx.covers.clone());
        cx.covered.extend(covered.iter().cloned());
        self.judge(cx, covered, end);
    }

    /// A run's end: a complete listing drops what it didn't give, a
    /// complete run the records of runs before it didn't name (Fenix §4,
    /// "What's dropped").
    fn finish(&mut self, cx: &mut Cx) {
        let model = self.models.entry(cx.provider.clone()).or_default();
        let complete = cx.run.complete && cx.run.broken.is_empty() && cx.run.crashed.is_empty();
        cx.run.complete = complete;
        if !complete {
            return;
        }
        match cx.purpose {
            Purpose::Listing => {
                let gone: Vec<String> = model.nodes.iter().filter(|(k, n)| n.records[Source::Listing as usize].is_some() && !cx.listed.contains(*k)).map(|(k, _)| k.clone()).collect();
                for g in gone {
                    model.drop_record(&g, Source::Listing);
                }
            }
            Purpose::Tests => {
                let asked: HashSet<&String> = cx.queued.iter().collect();
                let gone: Vec<String> = model
                    .nodes
                    .iter()
                    .filter(|(k, n)| n.records[Source::Run as usize].is_some() && !cx.found.contains(*k) && !cx.ended.contains(*k))
                    .filter(|(k, _)| {
                        let mut cur = model.record(k).and_then(|r| r.parent);
                        while let Some(p) = cur {
                            if asked.contains(&p) || cx.queued.is_empty() {
                                return true;
                            }
                            cur = model.record(&p).and_then(|r| r.parent);
                        }
                        false
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                for g in gone {
                    model.drop_record(&g, Source::Run);
                }
            }
            Purpose::Build => {}
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
        let filled = fill_data(cmd);
        let len = command_line_len(&filled);
        let limit = if self.batch_files.iter().any(|b| b == program) { BATCH_FILE_MAX } else { WINDOWS_MAX };
        if len > limit {
            return Some(format!("its command line is {len} characters; Windows takes {limit}"));
        }
        let env: usize = cmd.env.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
        if env > ENV_MAX {
            return Some(format!("its env is {env} bytes; Fenix takes 64 KiB"));
        }
        if step.label.chars().count() > LABEL_MAX {
            return Some(format!("its label is over {LABEL_MAX} characters"));
        }
        if step.needs.len() > NEEDS_MAX {
            return Some(format!("it needs {} files; Fenix copies {NEEDS_MAX}", step.needs.len()));
        }
        for need in &step.needs {
            let file = need.strip_prefix("/data/").filter(|_| need.len() <= PATH_MAX).map(|rest| self.data.join(rest));
            if !file.is_some_and(|f| f.is_file()) {
                return Some(format!("needs {need}, which isn't there"));
            }
        }
        None
    }

    /// A call's reports applied in order, as Fenix applies them (Fenix §4,
    /// §8.5).
    fn apply(&mut self, cx: &mut Cx, mut reports: Vec<Report>) {
        if reports.len() > REPORTS_MAX {
            self.violate(format!("a call gave {} reports; Fenix takes {REPORTS_MAX}", reports.len()));
            reports.truncate(REPORTS_MAX);
        }
        let mut answer: HashSet<String> = HashSet::new();
        for report in reports {
            match report {
                Report::Found(f) => {
                    let Some(id) = self.check_found(&cx.provider, &f, &answer) else { continue };
                    answer.insert(id.clone());
                    let path = f.path.as_deref().map(|p| place(p, cx.cwd.as_deref()));
                    let source = if cx.purpose == Purpose::Listing { Source::Listing } else { Source::Run };
                    self.model_mut(&cx.provider).set(&id, source, Record::from(&f, path));
                    if cx.purpose == Purpose::Listing {
                        cx.listed.insert(id);
                    } else {
                        cx.found.insert(id);
                    }
                }
                Report::Started(id) => {
                    let id = cut_id(&id).into_owned();
                    if cx.purpose == Purpose::Tests && self.model(&cx.provider).has(&id) {
                        self.set_state(cx, &id, State::Running);
                    }
                }
                Report::Ended(e) => {
                    // Ignored in a listing and a build-first plan (Fenix §4,
                    // §11).
                    if cx.purpose != Purpose::Tests {
                        continue;
                    }
                    let id = cut_id(&e.id).into_owned();
                    if e.message.as_ref().is_some_and(|m| m.len() > 4096) || e.at.as_ref().is_some_and(|a| a.path.len() > PATH_MAX) {
                        self.violate(format!("{id}'s message or place is over Fenix's limit"));
                    }
                    let at = e.at.as_ref().map(|a| (place(&a.path, cx.cwd.as_deref()), a.line, a.column));
                    if !self.model(&cx.provider).has(&id) {
                        // Matching rule 3: under the deepest node of the
                        // file its `at` names, marked found in a run.
                        let parent = at.as_ref().and_then(|(p, l, _)| self.deepest(&cx.provider, p, *l));
                        let record = Record { parent, label: id.clone(), kind: Kind::Test, path: None, line: None, end_line: None, skip: None, template: false, data: None };
                        let model = self.model_mut(&cx.provider);
                        model.set(&id, Source::Run, record);
                        model.nodes.get_mut(&id).expect("set").found_in_run = true;
                        cx.found.insert(id.clone());
                    }
                    self.set_state(cx, &id, State::of(e.outcome));
                    let n = self.model_mut(&cx.provider).nodes.get_mut(&id).expect("known");
                    n.result = Detail { message: e.message.clone(), at, text: e.text.clone(), duration_ms: e.duration_ms };
                    cx.ended.insert(id.clone());
                    cx.step_ended.insert(id);
                }
                Report::Output { id, text } => {
                    let Some(id) = id.map(|i| cut_id(&i).into_owned()) else { continue };
                    if let Some(n) = self.model_mut(&cx.provider).nodes.get_mut(&id) {
                        n.printed.push_str(&text);
                        n.printed.push('\n');
                        if n.printed.len() > PRINTED_MAX {
                            let mut cut = n.printed.len() - PRINTED_MAX;
                            while !n.printed.is_char_boundary(cut) {
                                cut += 1;
                            }
                            n.printed.drain(..cut);
                        }
                    }
                }
                Report::Broken { id, message, at: _ } => {
                    cx.run.broken.push((id.clone(), message.clone()));
                    if cx.purpose != Purpose::Tests {
                        continue;
                    }
                    // The tests under it are errored (Fenix §8.5).
                    let targets = self.scope(cx, id.as_deref(), "broken", false);
                    for t in targets {
                        self.errored(cx, &t, &message);
                    }
                }
                Report::Crashed { id, message } => {
                    cx.run.crashed.push((id.clone(), message.clone()));
                    if cx.purpose != Purpose::Tests {
                        continue;
                    }
                    // What's under it and hasn't ended in this step.
                    let targets = self.scope(cx, id.as_deref(), "crashed", true);
                    for t in targets {
                        self.errored(cx, &t, &message);
                    }
                }
            }
        }
    }

    /// What a `broken` or `crashed` reaches: what's under its id; with no
    /// id (or one Fenix doesn't know, which is said), all the step covers,
    /// or, for a step that claims nothing, the plan's rest.
    fn scope(&mut self, cx: &Cx, id: Option<&str>, what: &str, open_only: bool) -> Vec<String> {
        let id = id.map(|i| cut_id(i).into_owned());
        let known = id.as_ref().filter(|i| self.model(&cx.provider).has(i));
        if let (Some(i), None) = (&id, known) {
            self.violate(format!("{what} names {i}, which Fenix doesn't have: taken as all the step covers"));
        }
        let mut targets = match known {
            Some(i) => self.model(&cx.provider).tests_under(i),
            None if cx.covers.is_empty() => self.rest(cx),
            None => self.covered(cx, &cx.covers),
        };
        if open_only {
            targets.retain(|t| !cx.step_ended.contains(t));
        }
        targets
    }

    /// The deepest node of file `path` holding line `line` (from 0), else
    /// its file's node.
    fn deepest(&self, provider: &str, path: &str, line: u32) -> Option<String> {
        let model = self.model(provider);
        model
            .nodes
            .iter()
            .filter_map(|(k, n)| {
                let r = n.merged()?;
                let fits = matches!(r.kind, Kind::Group | Kind::File | Kind::Suite) && r.path.as_deref() == Some(path) && (r.kind == Kind::File || (r.line.is_none_or(|l| l <= line) && r.end_line.is_none_or(|e| e >= line)));
                fits.then(|| (model.depth(k), k.clone()))
            })
            .max()
            .map(|(_, k)| k)
    }

    /// How to debug `id` (Fenix §11): `tests-launch`, and a `build-first`
    /// plan run as a run's, then asked again once, only if every step
    /// exited 0 and nothing was broken or crashed.
    pub fn launch(&mut self, provider: &str, id: &str) -> Launched {
        let record = self.record(provider, id).unwrap_or_else(|| panic!("Fenix has no {id} to debug"));
        let test = api_found(id, &record);
        let run = self.next_run;
        self.next_run += 1;
        let first = self.timed("tests-launch", |e| e.tests_launch(provider, run, test.clone()));
        let plan = match first {
            Err(e) => return Launched { config: Err(e), built: None },
            Ok(api::Launch::Launch(c)) => return Launched { config: Ok(c), built: None },
            Ok(api::Launch::BuildFirst(plan)) => plan,
        };
        let mut cx = self.cx(provider, run, Purpose::Build, Vec::new());
        self.execute(&mut cx, plan.steps);
        let built = cx.run;
        let ok = built.steps.iter().all(|s| s.end == StepEnd::Exited(Some(0))) && built.broken.is_empty() && built.crashed.is_empty();
        if !ok {
            let why = built.broken.first().map(|(_, m)| m.clone()).or_else(|| built.steps.iter().find(|s| s.end != StepEnd::Exited(Some(0))).map(|s| format!("{} ended {:?}", s.label, s.end)));
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

    /// The tree Fenix holds for `provider`, as text: a node a line, its
    /// children by line then label, each with its kind, its first line
    /// (from 1), its state and its result, and which sources have it.
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
            if let Some(why) = r.skip.as_ref().filter(|_| matches!(r.kind, Kind::Test | Kind::Case)) {
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

/// A node as Fenix gives it back in an ask or `tests-launch`.
fn api_found(id: &str, r: &Record) -> Found {
    let path = r.path.as_ref().map(|p| if p.starts_with('/') || p.contains(':') { p.clone() } else { format!("/project/{p}") });
    Found { id: id.into(), parent: r.parent.clone(), label: r.label.clone(), kind: r.kind, path, line: r.line, end_line: r.end_line, skip: r.skip.clone(), template: r.template, data: r.data.clone() }
}

/// The program and args, joined by spaces.
pub fn command_text(cmd: &Command) -> String {
    std::iter::once(cmd.program.as_str()).chain(cmd.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
}

fn fill_data(cmd: &Command) -> Command {
    let mut c = cmd.clone();
    for a in &mut c.args {
        *a = a.replace("${data}", DATA_ON_WINDOWS).replace("${tools}", DATA_ON_WINDOWS);
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
    }
}
