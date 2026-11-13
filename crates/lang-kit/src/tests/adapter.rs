//! The adapter: a pack's [`TestRunner`]s on the five exports.

use std::collections::{BTreeMap, HashMap, HashSet};

use fenix_extension_api::Command;

use super::memory::{below, Memory};
use super::{api, child_id, cut_id, cut_label, group_id, At, Ask, Asked, Covered, Found, Group, Key, Kind, Launching, Line, Reader, Report, Step, StepEnd, TestRunner, GROUP};
use crate::host::Host;

/// The runs a provider keeps at most: a build-first plan Fenix never
/// asks about again (its build failed) is forgotten past these.
const RUNS_KEPT: usize = 8;
/// A cut line's pieces are joined up to this; the rest is dropped.
pub(crate) const JOINED_MAX: usize = 1 << 20;
/// Fenix's limits (its §6.5) the adapter keeps to itself.
const STEPS_MAX: usize = 256;
const COVERS_MAX: usize = 10_000;
const FOUND_MAX: usize = 10_000;
const OUTPUT_MAX: usize = 64 * 1024;
/// Where a pack's reporters go, below `/data`.
const ASSETS: &str = "lang-kit/tests";
/// What a step's args and env call the folder of its `files`.
const STEP_FILES: &str = "${step_files}";

/// A pack's test providers, and what the adapter keeps for each.
pub struct Tests {
    /// Where `/data` is: Fenix's in the sandbox, a folder of a test's on
    /// the host. `needs` always say `/data`.
    pub data: String,
    providers: Vec<Provider>,
}

impl Default for Tests {
    fn default() -> Tests {
        Tests { data: "/data".into(), providers: Vec::new() }
    }
}

struct Provider {
    runner: Box<dyn TestRunner>,
    /// Its assets' names, for `needs`.
    assets: Vec<String>,
    memory: Memory,
    runs: BTreeMap<u64, Run>,
    /// What Fenix was last told: the runner and the problem.
    told: Option<(String, Option<String>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Listing,
    Tests,
    /// A build-first plan's, kept until Fenix asks for the launch again.
    Build,
    /// A run this start of the pack didn't plan.
    Unplanned,
}

struct Run {
    ask: Ask,
    steps: Vec<Step>,
    purpose: Purpose,
    readers: HashMap<u32, Reading>,
    /// What readers kept (`Report::Keep`).
    kept: Vec<(String, String)>,
    /// The groups given to Fenix in this run.
    sent: HashSet<String>,
    /// Cases found in it, by id: their template's id.
    case_of: HashMap<String, String>,
    /// The ids found in it.
    found: HashSet<String>,
    /// The templates asked, for `is_case`.
    templates: Vec<Found>,
    /// A listing: the ids it gave, and whether it's complete so far.
    seen: HashSet<String>,
    complete: bool,
}

impl Run {
    fn new(ask: Ask, steps: Vec<Step>, purpose: Purpose) -> Run {
        let templates = match &ask {
            Ask::Run(items) => items
                .iter()
                .flat_map(|a| match a {
                    Asked::Test(f) => std::slice::from_ref(f),
                    Asked::Group { tests, .. } => tests.as_deref().unwrap_or_default(),
                })
                .filter(|f| f.template)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        Run {
            ask,
            steps,
            purpose,
            readers: HashMap::new(),
            kept: Vec::new(),
            sent: HashSet::new(),
            case_of: HashMap::new(),
            found: HashSet::new(),
            templates,
            seen: HashSet::new(),
            complete: true,
        }
    }
}

/// A step being read.
struct Reading {
    reader: Box<dyn Reader>,
    /// A cut line's pieces so far, per stream (stdout, stderr), and
    /// whether it went past [`JOINED_MAX`].
    pieces: [Option<String>; 2],
    over: [bool; 2],
}

impl Reading {
    fn new(reader: Box<dyn Reader>) -> Reading {
        Reading { reader, pieces: [None, None], over: [false, false] }
    }

    /// A line as Fenix gave it: a cut one's pieces joined, the whole given
    /// to the reader. A piece's rest is the next line of the same stream
    /// (Fenix §19, K3): the other stream's lines, and a call's end, may
    /// come between, so each stream keeps its own pending pieces.
    fn feed(&mut self, line: api::Line, out: &mut Vec<Report>) {
        let s = usize::from(line.stderr);
        if line.cut || self.pieces[s].is_some() {
            let joined = self.pieces[s].get_or_insert_with(String::new);
            if !self.over[s] {
                let room = JOINED_MAX - joined.len();
                if line.text.len() <= room {
                    joined.push_str(&line.text);
                } else {
                    joined.push_str(super::line::cut_bytes(&line.text, room));
                    self.over[s] = true;
                }
            }
            if line.cut {
                return;
            }
            let joined = self.pieces[s].take().unwrap_or_default();
            self.over[s] = false;
            self.reader.line(Line { text: &joined, stderr: line.stderr }, out);
        } else {
            self.reader.line(Line { text: &line.text, stderr: line.stderr }, out);
        }
    }

    /// The step ended with a cut line unfinished: what came of it.
    fn flush(&mut self, out: &mut Vec<Report>) {
        for s in 0..2 {
            if let Some(joined) = self.pieces[s].take() {
                self.reader.line(Line { text: &joined, stderr: s == 1 }, out);
            }
        }
    }
}

/// A path as Fenix takes it: below the project as `/project/<path>`, a
/// full path as it is.
fn project_path(p: &str) -> String {
    let full = p.starts_with(['/', '\\']) || (p.len() > 1 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic());
    if full {
        p.to_string()
    } else {
        format!("/project/{}", below(p))
    }
}

fn from_1(line: u32) -> u32 {
    line.saturating_sub(1)
}

fn location(at: &At) -> api::Location {
    api::Location { path: project_path(&at.file), line: from_1(at.line), column: at.column.map(from_1) }
}

/// `f`'s groups not yet given, then `f`, onto `out`. A case's parent is
/// its template.
fn push_found(out: &mut Vec<api::Report>, f: &Found, sent: &mut HashSet<String>) {
    push_found_after(out, f, sent, &mut None);
}

/// [`push_found`], with the groups of the `Found` given before and their
/// innermost id in `last`: consecutive tests of a scan share them, so
/// their ids are made once (a scan of 2,000 tests must take a few
/// milliseconds).
fn push_found_after(out: &mut Vec<api::Report>, f: &Found, sent: &mut HashSet<String>, last: &mut Option<(Vec<Group>, Option<String>)>) {
    let mut parent: Option<String> = match last {
        Some((groups, id)) if *groups == f.groups => id.clone(),
        _ => {
            let mut parent: Option<String> = None;
            for g in &f.groups {
                let id = child_id(parent.as_deref().unwrap_or(""), &g.label);
                if !sent.contains(&id) {
                    sent.insert(id.clone());
                    out.push(api::Report::Found(group_found(g, id.clone(), parent.clone())));
                }
                parent = Some(id);
            }
            *last = Some((f.groups.clone(), parent.clone()));
            parent
        }
    };
    if let Some(t) = &f.of {
        parent = Some(cut_id(&t.0).into_owned());
    }
    // In a scan's answer a test's file left out is the file scanned.
    let path = f.file.as_deref().map(project_path);
    out.push(api::Report::Found(api::Found {
        id: cut_id(&f.key.0).into_owned(),
        parent,
        label: cut_label(&f.label).into_owned(),
        kind: f.kind,
        path,
        line: f.line.map(from_1),
        end_line: f.end_line.map(from_1),
        skip: f.skip.clone(),
        template: f.template,
        data: f.data.clone(),
    }));
}

fn group_found(g: &Group, id: String, parent: Option<String>) -> api::Found {
    api::Found {
        id,
        parent,
        label: cut_label(&g.label).into_owned(),
        kind: g.kind,
        path: g.file.as_deref().map(project_path),
        line: g.line.map(from_1),
        end_line: g.end_line.map(from_1),
        skip: None,
        template: false,
        data: None,
    }
}

/// A record Fenix gave back as a pack's [`Found`], under `groups`, a case
/// of `of`: its full key where this start's memory has it (a key over 1
/// KiB comes back cut, and its `data` names it then), its place from its
/// `/project/<path>` and lines from 0.
fn found_from(r: &api::Found, memory: &Memory, groups: Vec<Group>, of: Option<Key>) -> Found {
    let known = memory.known(&r.id);
    Found {
        key: known.map(|k| k.key.clone()).unwrap_or_else(|| Key(r.id.clone())),
        label: r.label.clone(),
        kind: r.kind,
        groups,
        file: r.path.as_deref().map(|p| below(p).to_string()),
        line: r.line.map(|l| l + 1),
        end_line: r.end_line.map(|l| l + 1),
        skip: r.skip.clone(),
        template: r.template,
        of: if r.kind == Kind::Case { of } else { None },
        data: r.data.clone(),
    }
}

/// A test or case Fenix gave back for itself (selected, or to
/// `tests-launch`): its groups from this start's memory, by its `parent`
/// -- where Fenix put it, a template for a case, none at the top.
/// `chains` keeps each group's chain for the next record under it.
fn given_test(r: &api::Found, memory: &Memory, chains: &mut HashMap<String, Vec<Group>>) -> Found {
    let (groups, of) = match r.parent.as_deref() {
        Some(p) if !p.starts_with(GROUP) => {
            // A case's parent is its template.
            let template = memory.found(p);
            (template.as_ref().map(|t| t.groups.clone()).unwrap_or_default(), Some(template.map(|t| t.key).unwrap_or_else(|| Key(p.to_string()))))
        }
        Some(p) => (chains.entry(p.to_string()).or_insert_with(|| memory.chain(p)).clone(), None),
        None => (memory.found(&r.id).map(|f| f.groups).unwrap_or_default(), None),
    };
    found_from(r, memory, groups, of)
}

fn is_group(kind: Kind) -> bool {
    matches!(kind, Kind::Group | Kind::File | Kind::Suite)
}

fn group_of(r: &api::Found) -> Group {
    Group {
        kind: r.kind,
        label: r.label.clone(),
        file: r.path.as_deref().map(|p| below(p).to_string()),
        line: r.line.map(|l| l + 1),
        end_line: r.end_line.map(|l| l + 1),
    }
}

/// Fenix's ask as the pack's (Fenix §4 "What the provider is asked", §19
/// finding 3, Decisions 20): a record whose `parent` is the id of another
/// record in the ask is part of a fill-in, one whose `parent` isn't was
/// selected. No id is read. A selected file or suite gets the tests and
/// cases of the records that fill it in, whole, or `None`.
fn ask_from(ask: api::Ask, memory: &Memory) -> Ask {
    let records = match ask {
        api::Ask::Discover => return Ask::Discover,
        api::Ask::RunAll => return Ask::RunAll,
        api::Ask::Run(records) => records,
    };
    let ids: HashSet<&str> = records.iter().map(|r| r.id.as_str()).collect();
    let mut items: Vec<Asked> = Vec::new();
    // Each record of a fill-in (and each selected group), by id: the
    // item it fills in, the groups a test under it is in, and, for a
    // template, its key.
    let mut owner: HashMap<&str, (usize, Vec<Group>, Option<Key>)> = HashMap::new();
    let mut chains: HashMap<String, Vec<Group>> = HashMap::new();
    for r in &records {
        let filled = r.parent.as_deref().filter(|p| *p != r.id && ids.contains(p)).and_then(|p| owner.get(p).cloned());
        match filled {
            Some((item, groups, template)) => {
                if is_group(r.kind) {
                    let mut inner = groups;
                    inner.push(group_of(r));
                    owner.insert(&r.id, (item, inner, None));
                    continue;
                }
                let f = found_from(r, memory, groups.clone(), template);
                // What may have records of its own under it: a template, and
                // a case (Go's `t.Run` in a `t.Run`), whose cases are of it.
                // Left out, they'd be told as selected on their own.
                if f.template || f.kind == Kind::Case {
                    owner.insert(&r.id, (item, groups, Some(f.key.clone())));
                }
                if let Some(Asked::Group { tests, .. }) = items.get_mut(item) {
                    tests.get_or_insert_with(Vec::new).push(f);
                }
            }
            None => {
                // Selected: what it's in from this start's memory (Fenix
                // sends what's above a selected node only as its `parent`).
                let above = r.parent.as_deref().map(|p| chains.entry(p.to_string()).or_insert_with(|| memory.chain(p)).clone()).unwrap_or_default();
                if is_group(r.kind) {
                    let mut groups = above;
                    groups.push(group_of(r));
                    owner.insert(&r.id, (items.len(), groups.clone(), None));
                    items.push(Asked::Group { groups, tests: None, known: Vec::new() });
                    continue;
                }
                items.push(Asked::Test(given_test(r, memory, &mut chains)));
            }
        }
    }
    for item in &mut items {
        if let Asked::Group { groups, tests: None, known } = item {
            let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
            *known = memory.under(&group_id(&labels));
        }
    }
    Ask::Run(items)
}

/// A scanner's last answer it could read, per file: for a scanner that
/// parses, and so can't read half-typed text, which Fenix scans 500 ms
/// after typing pauses. An empty answer drops every test of the file
/// (Fenix §19, finding 8), so answer what was last read well instead:
///
/// ```
/// use fenix_lang_kit::tests::{Found, LastGood};
///
/// // A parser: `None` when the braces don't match.
/// fn parse(text: &str) -> Option<Vec<Found>> {
///     (text.matches('{').count() == text.matches('}').count())
///         .then(|| text.match_indices("fn ").map(|(at, _)| Found::test(format!("t {at}"), "t")).collect())
/// }
///
/// let mut last = LastGood::default();
/// assert_eq!(last.answer("/project/a.rs", parse("fn adds() {}")).len(), 1);
/// assert_eq!(last.answer("/project/a.rs", parse("fn adds() {")).len(), 1, "half-typed: the last good answer");
/// assert!(last.answer("/project/a.rs", parse("")).is_empty(), "read well, and empty: every test deleted");
/// ```
///
/// `answer(path, Some(found))` keeps `found` (even empty: a file whose
/// tests were all deleted) and gives it back; `answer(path, None)` -- the
/// text couldn't be read -- gives the last kept, or nothing. A scanner
/// that reads line by line always reads something, and needn't use it.
/// About a scan's size a file; `forget` a file that's gone.
#[derive(Debug, Default)]
pub struct LastGood {
    files: HashMap<String, Vec<Found>>,
}

impl LastGood {
    pub fn answer(&mut self, path: &str, read: Option<Vec<Found>>) -> Vec<Found> {
        match read {
            Some(found) => {
                self.files.insert(path.to_string(), found.clone());
                found
            }
            None => self.files.get(path).cloned().unwrap_or_default(),
        }
    }

    pub fn forget(&mut self, path: &str) {
        self.files.remove(path);
    }
}

impl Tests {
    pub(crate) fn add(&mut self, runner: Box<dyn TestRunner>) {
        let assets = runner.assets().iter().map(|(name, _)| name.to_string()).collect();
        self.providers.push(Provider { runner, assets, memory: Memory::default(), runs: BTreeMap::new(), told: None });
    }

    /// Its providers' ids.
    pub fn providers(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.runner.provider()).collect()
    }

    fn find(&mut self, provider: &str) -> Option<&mut Provider> {
        self.providers.iter_mut().find(|p| p.runner.provider() == provider)
    }

    /// When the pack starts: each provider's assets written where they
    /// differ, and its runner told.
    pub fn activate(&mut self, host: &mut dyn Host) {
        for p in &self.providers {
            for (name, bytes) in p.runner.assets() {
                let path = format!("{}/{ASSETS}/{name}", self.data);
                if std::fs::read(&path).ok().as_deref() == Some(bytes) {
                    continue;
                }
                let written = std::path::Path::new(&path).parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, bytes));
                if let Err(e) = written {
                    host.log(&format!("tests: couldn't write {path}: {e}"));
                }
            }
        }
        self.tell(host);
    }

    /// Each provider's runner asked again, and Fenix told where it changed.
    pub fn tell(&mut self, host: &mut dyn Host) {
        for p in &mut self.providers {
            let now = p.runner.runner();
            if p.told.as_ref() == Some(&now) {
                continue;
            }
            match host.test_provider_set(p.runner.provider(), &now.0, now.1.as_deref()) {
                Ok(()) => p.told = Some(now),
                Err(e) => host.log(&format!("tests: {}: {e}", p.runner.provider())),
            }
        }
    }

    pub fn scan(&mut self, host: &mut dyn Host, provider: &str, path: &str, text: &str) -> Vec<api::Found> {
        let Some(p) = self.find(provider) else { return Vec::new() };
        let found = p.runner.scan(path, text);
        // The same key twice: the later is kept, as Fenix keeps it (Fenix
        // §19, K10; Decisions 3), and the memory with it.
        let mut ids = HashSet::new();
        let mut later: Vec<Found> = Vec::with_capacity(found.len());
        for f in found.into_iter().rev() {
            if ids.insert(cut_id(&f.key.0).into_owned()) {
                later.push(f);
            } else {
                host.log(&format!("tests: {provider}: the scan of {path} found {} twice; the earlier is left out", f.key));
            }
        }
        later.reverse();
        let found = later;
        if found.len() > FOUND_MAX {
            host.log(&format!("tests: {provider}: the scan of {path} found {} tests; Fenix keeps {FOUND_MAX}", found.len()));
        }
        p.memory.scanned(path, &found);
        let mut out = Vec::new();
        let mut sent = HashSet::new();
        let mut last = None;
        for f in &found {
            push_found_after(&mut out, f, &mut sent, &mut last);
        }
        out.into_iter()
            .map(|r| match r {
                api::Report::Found(f) => f,
                _ => unreachable!("push_found gives found-tests"),
            })
            .collect()
    }

    pub fn plan(&mut self, host: &mut dyn Host, provider: &str, run: u64, ask: api::Ask) -> Result<api::Plan, String> {
        let data = self.data.clone();
        let p = self.find(provider).ok_or_else(|| format!("no test provider {provider} here"))?;
        let ask = ask_from(ask, &p.memory);
        let steps = p.runner.plan(&ask)?;
        let purpose = if ask == Ask::Discover { Purpose::Listing } else { Purpose::Tests };
        let list_after = purpose == Purpose::Tests && p.runner.list_after(&ask);
        let plan = p.steps(host, &data, run, &steps)?;
        p.keep(run, Run::new(ask, steps, purpose));
        Ok(api::Plan { steps: plan, list_after })
    }

    pub fn read(&mut self, host: &mut dyn Host, provider: &str, run: u64, step: u32, lines: Vec<api::Line>) -> Vec<api::Report> {
        let Some(p) = self.find(provider) else { return Vec::new() };
        let r = p.run(host, run);
        if !r.readers.contains_key(&step) {
            let reader = p.reader(run, step);
            p.runs.get_mut(&run).expect("kept above").readers.insert(step, Reading::new(reader));
        }
        let Provider { runner, memory, runs, .. } = p;
        let r = runs.get_mut(&run).expect("kept above");
        let mut reading = r.readers.remove(&step).expect("made above");
        let mut reports = Vec::new();
        for line in lines {
            reading.feed(line, &mut reports);
        }
        let mut out = Vec::new();
        convert(&mut out, reports, runner.as_mut(), memory, r);
        r.readers.insert(step, reading);
        out
    }

    pub fn step_ended(&mut self, host: &mut dyn Host, provider: &str, run: u64, step: u32, end: StepEnd) -> Vec<api::Report> {
        let data = self.data.clone();
        let Some(p) = self.find(provider) else { return Vec::new() };
        let r = p.run(host, run);
        let unplanned = r.purpose == Purpose::Unplanned;
        // A step that never started has no reader; one that did gives what
        // it held.
        let reading = r.readers.remove(&step);
        let Provider { runner, memory, runs, .. } = p;
        let r = runs.get_mut(&run).expect("kept above");
        let mut out = Vec::new();
        if let Some(mut reading) = reading {
            let mut reports = Vec::new();
            reading.flush(&mut reports);
            reading.reader.end(&end, &mut reports);
            // A template whose cases have results gets no result of its
            // own: Fenix doesn't count it, and its state is its cases'
            // (Fenix §19, K7; Decisions 7).
            convert(&mut out, reports, runner.as_mut(), memory, r);
        }
        if !matches!(end, StepEnd::Exited(_)) {
            r.complete = false;
        }
        let _ = std::fs::remove_dir_all(format!("{data}/{ASSETS}/runs/{run}-{step}"));
        let last = step as usize + 1 >= r.steps.len();
        if last && r.purpose == Purpose::Listing && r.complete {
            memory.listing_ended(&r.seen);
        }
        if unplanned || (last && r.purpose != Purpose::Build) {
            runs.remove(&run);
        }
        out
    }

    pub fn launch(&mut self, host: &mut dyn Host, provider: &str, run: u64, test: api::Found) -> Result<api::Launch, String> {
        let data = self.data.clone();
        let p = self.find(provider).ok_or_else(|| format!("no test provider {provider} here"))?;
        let found = given_test(&test, &p.memory, &mut HashMap::new());
        if let Some(built) = p.runs.remove(&run).filter(|r| r.purpose == Purpose::Build) {
            return match p.runner.launch(&found, &built.kept)? {
                Launching::Now(config) => Ok(api::Launch::Launch(config)),
                Launching::BuildFirst(_) => Err(format!("{provider} asked to build twice to debug {}", found.key)),
            };
        }
        match p.runner.launch(&found, &[])? {
            Launching::Now(config) => Ok(api::Launch::Launch(config)),
            Launching::BuildFirst(steps) => {
                let plan = p.steps(host, &data, run, &steps)?;
                p.keep(run, Run::new(Ask::Debug(found), steps, Purpose::Build));
                Ok(api::Launch::BuildFirst(api::Plan { steps: plan, list_after: false }))
            }
        }
    }
}

impl Provider {
    fn keep(&mut self, id: u64, run: Run) {
        self.runs.insert(id, run);
        while self.runs.len() > RUNS_KEPT {
            self.runs.pop_first();
        }
    }

    /// Run `id`, or one made for it when this start of the pack didn't
    /// plan it.
    fn run(&mut self, host: &mut dyn Host, id: u64) -> &mut Run {
        if !self.runs.contains_key(&id) {
            host.log(&format!("tests: {}: run {id} wasn't planned since the pack started; its lines are read afresh", self.runner.provider()));
            self.keep(id, Run::new(Ask::RunAll, Vec::new(), Purpose::Unplanned));
        }
        self.runs.get_mut(&id).expect("kept")
    }

    fn reader(&mut self, run: u64, step: u32) -> Box<dyn Reader> {
        let r = &self.runs[&run];
        match r.steps.get(step as usize) {
            Some(s) if r.purpose != Purpose::Unplanned => self.runner.reader(&r.ask, s),
            _ => self.runner.reader_after_restart(),
        }
    }

    /// The pack's steps as Fenix's: `needs` named, `files` written,
    /// `covers` as ids.
    fn steps(&mut self, host: &mut dyn Host, data: &str, run: u64, steps: &[Step]) -> Result<Vec<api::Step>, String> {
        if steps.len() > STEPS_MAX {
            return Err(format!("{} planned {} steps; Fenix runs {STEPS_MAX} at most, so ask for less or plan by groups", self.runner.provider(), steps.len()));
        }
        let mut covered = 0;
        let mut out = Vec::with_capacity(steps.len());
        for (i, s) in steps.iter().enumerate() {
            let mut command = s.command.clone();
            let mut needs = s.needs.clone();
            if !s.files.is_empty() {
                let dir = format!("{ASSETS}/runs/{run}-{i}");
                std::fs::create_dir_all(format!("{data}/{dir}")).map_err(|e| format!("tests: couldn't write {data}/{dir}: {e}"))?;
                for (name, bytes) in &s.files {
                    std::fs::write(format!("{data}/{dir}/{name}"), bytes).map_err(|e| format!("tests: couldn't write {data}/{dir}/{name}: {e}"))?;
                    needs.push(format!("/data/{dir}/{name}"));
                }
                fill(&mut command, STEP_FILES, &format!("${{data}}/{dir}"));
            }
            for name in &self.assets {
                let needle = format!("${{data}}/{ASSETS}/{name}");
                let named = command.args.iter().chain(command.env.iter().map(|(_, v)| v)).any(|a| a.contains(&needle));
                let need = format!("/data/{ASSETS}/{name}");
                if named && !needs.contains(&need) {
                    needs.push(need);
                }
            }
            let mut covers: Vec<String> = s
                .covers
                .iter()
                .map(|c| match c {
                    Covered::Test(k) => cut_id(&k.0).into_owned(),
                    Covered::Group(labels) => group_id(labels),
                })
                .collect();
            covered += covers.len();
            if covered > COVERS_MAX {
                // Fenix keeps 10,000 ids of covers a plan: a step past them
                // claims nothing, so its tests are judged at the plan's end.
                host.log(&format!("tests: {}: step {i} covers past Fenix's {COVERS_MAX} ids; judged at the plan's end", self.runner.provider()));
                covers.clear();
            }
            out.push(api::Step { command, label: cut_label(&s.label).into_owned(), covers, needs });
        }
        Ok(out)
    }
}

/// `name` in `cmd`'s args and env values, as `value`.
fn fill(cmd: &mut Command, name: &str, value: &str) {
    for a in cmd.args.iter_mut().chain(cmd.env.iter_mut().map(|(_, v)| v)) {
        if a.contains(name) {
            *a = a.replace(name, value);
        }
    }
}

/// A reader's reports as Fenix's, onto `out`. Each `Ended` goes as it
/// came: Fenix keeps a test's last in the run (Fenix §4 "A result").
fn convert(out: &mut Vec<api::Report>, reports: Vec<Report>, runner: &mut dyn TestRunner, memory: &mut Memory, run: &mut Run) {
    for report in reports {
        match report {
            // Ignored in a build-first plan (Fenix §19, K9; Decisions 17):
            // not sent.
            Report::Found(_) if run.purpose == Purpose::Build => {}
            Report::Found(mut f) => {
                memory.fill(&mut f);
                if run.purpose == Purpose::Listing {
                    let id = memory.listed(&f);
                    run.seen.insert(id);
                } else {
                    place(&mut f, runner, memory, run);
                }
                run.found.insert(cut_id(&f.key.0).into_owned());
                push_found(out, &f, &mut run.sent);
            }
            Report::Started(k) => {
                unfound_case(out, &k, runner, memory, run);
                out.push(api::Report::Started(cut_id(&k.0).into_owned()));
            }
            Report::Ended(e) => {
                unfound_case(out, &e.key, runner, memory, run);
                let id = cut_id(&e.key.0).into_owned();
                out.push(api::Report::Ended(api::Ended { id, outcome: e.outcome, duration_ms: e.ms, message: e.message, at: e.at.as_ref().map(location), text: e.text }));
            }
            Report::Output { key, text } => {
                let id = key.map(|k| cut_id(&k.0).into_owned());
                if let Some(api::Report::Output { id: last, text: so_far }) = out.last_mut() {
                    if *last == id && so_far.len() + 1 + text.len() <= OUTPUT_MAX {
                        so_far.push('\n');
                        so_far.push_str(&text);
                        continue;
                    }
                }
                out.push(api::Report::Output { id, text });
            }
            Report::Broken { under, message, at } => {
                run.complete = false;
                out.push(api::Report::Broken { id: under.map(|l| group_id(&l)), message, at: at.as_ref().map(location) });
            }
            Report::Crashed { under, message } => {
                run.complete = false;
                out.push(api::Report::Crashed { id: under.map(|l| group_id(&l)), message });
            }
            Report::Keep { name, value } => run.kept.push((name, value)),
        }
    }
}

/// A result for a key nothing found, which is a template's case by its
/// key ([`place`]): its `found` first, so Fenix puts it under its template
/// (Fenix §4, matching rule 2), labelled by what its key adds to the
/// template's (`negative_one` of `TestTable/negative_one`).
fn unfound_case(out: &mut Vec<api::Report>, key: &Key, runner: &mut dyn TestRunner, memory: &Memory, run: &mut Run) {
    let id = cut_id(&key.0).into_owned();
    if matches!(run.purpose, Purpose::Listing | Purpose::Build) || run.found.contains(&id) || run.case_of.contains_key(&id) || memory.known(&id).is_some() {
        return;
    }
    let mut f = Found::test(key.clone(), key.0.clone());
    place(&mut f, runner, memory, run);
    let Some(t) = f.of.clone() else { return };
    let rest = key.0.strip_prefix(t.0.as_str()).unwrap_or(&key.0);
    let label = rest.trim_start_matches([':', '/', '[', '.', ' ', '-', '#']).trim_end_matches(']');
    f.label = if label.is_empty() { key.0.clone() } else { label.to_string() };
    run.found.insert(id);
    push_found(out, &f, &mut run.sent);
}

/// A test a run found that the pack didn't say is a case: a case of a
/// template whose key starts it, or at whose line it is, or that
/// [`TestRunner::is_case`] claims; given its template's groups and place.
fn place(f: &mut Found, runner: &mut dyn TestRunner, memory: &Memory, run: &mut Run) {
    let id = cut_id(&f.key.0).into_owned();
    if f.of.is_none() && memory.known(&id).is_none() {
        f.of = memory.template_of(&f.key.0, f.file.as_deref(), f.line).or_else(|| run.templates.iter().find(|t| t.key != f.key && runner.is_case(t, &f.key)).map(|t| t.key.clone()));
    }
    let Some(t) = f.of.clone() else { return };
    f.kind = Kind::Case;
    let tid = cut_id(&t.0).into_owned();
    if let Some(template) = memory.found(&tid).or_else(|| run.templates.iter().find(|x| x.key == t).cloned()) {
        if f.groups.is_empty() {
            f.groups = template.groups;
        }
        if f.file.is_none() {
            f.file = template.file;
            f.line = f.line.or(template.line);
        }
    }
    run.case_of.insert(id, tid);
}
