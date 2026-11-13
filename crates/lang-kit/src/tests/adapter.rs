//! The adapter: a pack's [`TestRunner`]s on the five exports.

use std::collections::{BTreeMap, HashMap, HashSet};

use fenix_extension_api::Command;

use super::memory::{below, Memory};
use super::{api, cut_id, cut_label, group_id, At, Ask, Asked, Covered, Found, Group, Key, Kind, Launching, Line, Outcome, Reader, Report, Step, StepEnd, TestRunner, GROUP};
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
    /// Each test's result so far, as sent.
    ended: HashMap<String, api::Ended>,
    /// The groups given to Fenix in this run.
    sent: HashSet<String>,
    /// Cases found in it, by id: their template's id.
    case_of: HashMap<String, String>,
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
                    Asked::Group { tests, .. } => tests.as_slice(),
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
            ended: HashMap::new(),
            sent: HashSet::new(),
            case_of: HashMap::new(),
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
    /// Cases that ended in this step, by their template's id: the worst
    /// outcome, how many, how many didn't pass.
    cases: BTreeMap<String, (Outcome, usize, usize)>,
}

impl Reading {
    fn new(reader: Box<dyn Reader>) -> Reading {
        Reading { reader, pieces: [None, None], over: [false, false], cases: BTreeMap::new() }
    }

    /// A line as Fenix gave it: a cut one's pieces joined, the whole given
    /// to the reader.
    fn feed(&mut self, line: api::Line, out: &mut Vec<Report>) {
        let s = usize::from(line.stderr);
        if line.cut || self.pieces[s].is_some() {
            let joined = self.pieces[s].get_or_insert_with(String::new);
            if !self.over[s] {
                let room = JOINED_MAX - joined.len();
                if line.text.len() <= room {
                    joined.push_str(&line.text);
                } else {
                    joined.push_str(&super::line::cut_bytes(&line.text, room));
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

/// How bad an outcome is, for one result of several.
fn rank(o: Outcome) -> u8 {
    match o {
        Outcome::Skipped => 0,
        Outcome::Passed => 1,
        Outcome::Failed => 2,
        Outcome::Errored => 3,
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
    let mut parent: Option<String> = None;
    let mut labels: Vec<&str> = Vec::with_capacity(f.groups.len());
    for g in &f.groups {
        labels.push(&g.label);
        let id = group_id(&labels);
        if sent.insert(id.clone()) {
            out.push(api::Report::Found(group_found(g, id.clone(), parent.clone())));
        }
        parent = Some(id);
    }
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

/// Two results for one test in a run, as one: the worse outcome and its
/// message and place, the times added, the texts one after the other.
fn merge(old: &api::Ended, new: api::Ended) -> api::Ended {
    let (worse, better) = if rank(new.outcome) >= rank(old.outcome) { (&new, old) } else { (old, &new) };
    let text = match (&old.text, &new.text) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (a, b) => a.clone().or(b.clone()),
    };
    let duration_ms = match (old.duration_ms, new.duration_ms) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    };
    api::Ended {
        id: new.id.clone(),
        outcome: worse.outcome,
        duration_ms,
        message: worse.message.clone().or(better.message.clone()),
        at: worse.at.clone().or(better.at.clone()),
        text,
    }
}

/// Fenix's records, as a pack's [`Found`]: what the memory has of each
/// filled in (its full key, its groups).
fn found_from(r: &api::Found, memory: &Memory) -> Found {
    let known = memory.known(&r.id);
    let (groups, of) = match &r.parent {
        Some(p) if p.starts_with(GROUP) => (memory.chain(p), None),
        Some(t) => {
            let template = memory.found(t);
            let key = template.as_ref().map(|f| f.key.clone()).unwrap_or_else(|| Key(t.clone()));
            (template.map(|f| f.groups).unwrap_or_default(), Some(key))
        }
        None => (Vec::new(), None),
    };
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

fn group_from(r: &api::Found, memory: &Memory) -> Vec<Group> {
    let mut groups = r.parent.as_deref().map(|p| memory.chain(p)).unwrap_or_default();
    groups.push(Group {
        kind: r.kind,
        label: r.label.clone(),
        file: r.path.as_deref().map(|p| below(p).to_string()),
        line: r.line.map(|l| l + 1),
        end_line: r.end_line.map(|l| l + 1),
    });
    groups
}

/// Fenix's ask as the pack's: each group, file or suite with the tests
/// Fenix sent after it.
fn ask_from(ask: api::Ask, memory: &Memory) -> Ask {
    let records = match ask {
        api::Ask::Discover => return Ask::Discover,
        api::Ask::RunAll => return Ask::RunAll,
        api::Ask::Run(records) => records,
    };
    let mut items = Vec::new();
    let mut i = 0;
    while i < records.len() {
        let r = &records[i];
        i += 1;
        if matches!(r.kind, Kind::Test | Kind::Case) && !r.id.starts_with(GROUP) {
            items.push(Asked::Test(found_from(r, memory)));
            continue;
        }
        let mut tests = Vec::new();
        if matches!(r.kind, Kind::File | Kind::Suite) {
            let deeper = format!("{}{GROUP}", r.id);
            let mut taken: HashSet<&str> = HashSet::new();
            while let Some(t) = records.get(i).filter(|t| matches!(t.kind, Kind::Test | Kind::Case)) {
                let under = t.parent.as_deref().is_some_and(|p| {
                    p == r.id || p.starts_with(&deeper) || taken.contains(p) || memory.known(p).and_then(|k| k.group.as_deref()).is_some_and(|g| g == r.id || g.starts_with(&deeper))
                });
                if !under {
                    break;
                }
                taken.insert(&t.id);
                tests.push(found_from(t, memory));
                i += 1;
            }
        }
        let known = if tests.is_empty() { memory.under(&r.id) } else { Vec::new() };
        items.push(Asked::Group { groups: group_from(r, memory), tests, known });
    }
    Ask::Run(items)
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
        let mut found = p.runner.scan(path, text);
        let mut ids = HashSet::new();
        found.retain(|f| {
            let fresh = ids.insert(cut_id(&f.key.0).into_owned());
            if !fresh {
                host.log(&format!("tests: {provider}: the scan of {path} found {} twice; the second is left out", f.key));
            }
            fresh
        });
        if found.len() > FOUND_MAX {
            host.log(&format!("tests: {provider}: the scan of {path} found {} tests; Fenix keeps {FOUND_MAX}", found.len()));
        }
        p.memory.scanned(path, &found);
        let mut out = Vec::new();
        let mut sent = HashSet::new();
        for f in &found {
            push_found(&mut out, f, &mut sent);
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
        convert(&mut out, reports, runner.as_mut(), memory, r, Some(&mut reading));
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
            convert(&mut out, reports, runner.as_mut(), memory, r, Some(&mut reading));
            // A template whose cases ended in this step and that has no
            // result of its own gets theirs, so Fenix doesn't judge it
            // "ended without a result".
            if r.purpose == Purpose::Tests || unplanned {
                for (template, (worst, n, bad)) in reading.cases {
                    if r.ended.contains_key(&template) {
                        continue;
                    }
                    let message = (bad > 0).then(|| format!("{bad} of its {n} cases didn't pass"));
                    let e = api::Ended { id: template.clone(), outcome: worst, duration_ms: None, message, at: None, text: None };
                    r.ended.insert(template, e.clone());
                    out.push(api::Report::Ended(e));
                }
            }
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
        let found = found_from(&test, &p.memory);
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

/// A reader's reports as Fenix's, onto `out`.
fn convert(out: &mut Vec<api::Report>, reports: Vec<Report>, runner: &mut dyn TestRunner, memory: &mut Memory, run: &mut Run, mut reading: Option<&mut Reading>) {
    for report in reports {
        match report {
            Report::Found(mut f) => {
                memory.fill(&mut f);
                if run.purpose == Purpose::Listing {
                    let id = memory.listed(&f);
                    run.seen.insert(id);
                } else {
                    place(&mut f, runner, memory, run);
                }
                push_found(out, &f, &mut run.sent);
            }
            Report::Started(k) => out.push(api::Report::Started(cut_id(&k.0).into_owned())),
            Report::Ended(e) => {
                let id = cut_id(&e.key.0).into_owned();
                let ended = api::Ended { id: id.clone(), outcome: e.outcome, duration_ms: e.ms, message: e.message, at: e.at.as_ref().map(location), text: e.text };
                let ended = match run.ended.get(&id) {
                    Some(old) => merge(old, ended),
                    None => ended,
                };
                let template = run.case_of.get(&id).cloned().or_else(|| memory.known(&id).and_then(|k| k.of.as_ref()).map(|t| cut_id(&t.0).into_owned()));
                if let (Some(t), Some(reading)) = (template, reading.as_deref_mut()) {
                    let c = reading.cases.entry(t).or_insert((ended.outcome, 0, 0));
                    if rank(ended.outcome) > rank(c.0) {
                        c.0 = ended.outcome;
                    }
                    c.1 += 1;
                    c.2 += usize::from(matches!(ended.outcome, Outcome::Failed | Outcome::Errored));
                }
                run.ended.insert(id, ended.clone());
                out.push(api::Report::Ended(ended));
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
