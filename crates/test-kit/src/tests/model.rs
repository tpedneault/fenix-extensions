//! Fenix's model of a provider's tests, by its rules (Fenix's
//! `docs/plans/test-explorer.md` §4, and `fenix-tests`' numbered
//! Decisions, cited as *Decisions n*): each node with a record per
//! source -- each file's scan, the listing, a run -- merged; each source
//! dropping its own; a result per test, the last `ended` winning; a
//! group's state worked out from the tests counted under it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use fenix_lang_kit::tests::api::{Found, Kind, Outcome};
use fenix_lang_kit::tests::is_cut_id;

/// How deep the tree goes: a node deeper is put at its provider's top
/// (Decisions 6).
const MAX_DEPTH: usize = 256;

/// All the `data` kept for one provider: past it a node's isn't kept
/// (Fenix §6.5).
pub const DATA_PER_PROVIDER: usize = 16 << 20;

/// A test's state on the page (Fenix §4, "States").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Unknown,
    Queued,
    Running,
    Passed,
    Failed,
    Skipped,
    Errored,
}

impl State {
    pub fn of(o: Outcome) -> State {
        match o {
            Outcome::Passed => State::Passed,
            Outcome::Failed => State::Failed,
            Outcome::Skipped => State::Skipped,
            Outcome::Errored => State::Errored,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            State::Unknown => "unknown",
            State::Queued => "queued",
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Skipped => "skipped",
            State::Errored => "errored",
        }
    }
}

/// Which source said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Scan = 0,
    Listing = 1,
    Run = 2,
}

/// What one source said of a node; paths below the project (`src/lib.rs`)
/// or full, as Fenix maps them.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub parent: Option<String>,
    pub label: String,
    pub kind: Kind,
    pub path: Option<String>,
    /// From 0, as the API's.
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub skip: Option<String>,
    pub template: bool,
    pub data: Option<String>,
}

impl Record {
    pub fn from(f: &Found, path: Option<String>) -> Record {
        // A last line before the first is left out (Decisions 5).
        let end_line = match (f.line, f.end_line) {
            (Some(l), Some(e)) if e < l => None,
            (_, e) => e,
        };
        Record { parent: f.parent.clone(), label: f.label.clone(), kind: f.kind, path, line: f.line, end_line, skip: f.skip.clone(), template: f.template, data: f.data.clone() }
    }
}

/// A test's last result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Detail {
    pub message: Option<String>,
    /// Below the project or full; line and column from 0.
    pub at: Option<(String, u32, Option<u32>)>,
    pub text: Option<String>,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Each file's last scan that gave it: when, and what it said.
    pub scans: BTreeMap<String, (u64, Record)>,
    /// The latest of `scans`, the listing's, the last run's.
    pub records: [Option<Record>; 3],
    /// When it was first found: the page's order among equals.
    pub seq: u64,
    /// Its own state: a result, or queued or running in a run.
    pub state: State,
    pub result: Detail,
    /// What it printed in its runs, its last 64 KiB.
    pub printed: String,
    /// Added by an `ended` Fenix didn't know (Fenix §4, matching rule 3).
    pub found_in_run: bool,
}

impl Node {
    /// Its fields from its sources (Fenix §4; Decisions 30): field by
    /// field the scan's, else the listing's, else the run's; its place
    /// (`path`, `line`, `end_line`) as one, from the first that gives a
    /// path or a line; `label`, `kind`, `parent` and `template` from the
    /// first that has it; `skip` the scan's as it is wherever a scan has
    /// it.
    pub fn merged(&self) -> Option<Record> {
        let sources: Vec<&Record> = self.records.iter().flatten().collect();
        let first = sources.first()?;
        let place = sources.iter().find(|r| r.path.is_some() || r.line.is_some());
        let skip = match &self.records[Source::Scan as usize] {
            Some(scan) => scan.skip.clone(),
            None => sources.iter().find_map(|r| r.skip.clone()),
        };
        Some(Record {
            parent: first.parent.clone(),
            label: first.label.clone(),
            kind: first.kind,
            path: place.and_then(|r| r.path.clone()),
            line: place.and_then(|r| r.line),
            end_line: place.and_then(|r| r.end_line),
            skip,
            template: first.template,
            data: sources.iter().find_map(|r| r.data.clone()),
        })
    }

    fn data_bytes(&self) -> usize {
        let len = |r: &Record| r.data.as_ref().map_or(0, String::len);
        self.scans.values().map(|(_, r)| len(r)).sum::<usize>() + self.records[1..].iter().flatten().map(len).sum::<usize>()
    }
}

/// The tree's shape, worked out from the merged records when one changes
/// (Decisions 6): parents, children in the page's order, the page's
/// order, and what's counted.
#[derive(Debug, Default)]
struct Shape {
    parent: HashMap<String, Option<String>>,
    kids: HashMap<Option<String>, Vec<String>>,
    order: HashMap<String, usize>,
    leaf: HashSet<String>,
}

/// One provider's tests.
#[derive(Debug, Default)]
pub struct Model {
    pub nodes: BTreeMap<String, Node>,
    shape: RefCell<Option<Shape>>,
    /// Each file's last scan: the ids it gave.
    pub scans: BTreeMap<String, Vec<String>>,
    /// Results of nodes that left every source, kept for them should they
    /// come back (Decisions 29).
    pub restored: HashMap<String, (State, Detail)>,
    /// What `test-provider-set` last said: runner, problem.
    pub runner: Option<(String, Option<String>)>,
    /// `data` kept, in bytes, against [`DATA_PER_PROVIDER`].
    data_bytes: usize,
    seq: u64,
}

impl Model {
    pub fn record(&self, id: &str) -> Option<Record> {
        self.nodes.get(id)?.merged()
    }

    pub fn has(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    fn shape(&self) -> std::cell::RefMut<'_, Shape> {
        let mut cache = self.shape.borrow_mut();
        if cache.is_none() {
            *cache = Some(self.build());
        }
        std::cell::RefMut::map(cache, |c| c.as_mut().expect("built"))
    }

    fn build(&self) -> Shape {
        let merged: HashMap<&String, Record> = self.nodes.iter().filter_map(|(k, n)| n.merged().map(|r| (k, r))).collect();
        // A parent it hasn't given, itself, a loop or a chain past 256:
        // at the provider's top (Decisions 6).
        let mut parent: HashMap<String, Option<String>> = HashMap::new();
        for (k, r) in &merged {
            let p = r.parent.clone().filter(|p| p != *k && merged.contains_key(p));
            parent.insert((*k).clone(), p);
        }
        let ids: Vec<String> = parent.keys().cloned().collect();
        for id in &ids {
            let mut at = parent[id].clone();
            let mut steps = 0;
            while let Some(up) = at {
                steps += 1;
                if up == *id || steps > MAX_DEPTH {
                    parent.insert(id.clone(), None);
                    break;
                }
                at = parent.get(&up).cloned().flatten();
            }
        }
        // The page's order: by path (none last), line (none last), then
        // when found (Fenix's `build`).
        let mut kids: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for (k, p) in &parent {
            kids.entry(p.clone()).or_default().push(k.clone());
        }
        let key = |k: &String| {
            let r = &merged[k];
            (r.path.is_none(), r.path.clone(), r.line.unwrap_or(u32::MAX), self.nodes[k].seq)
        };
        for v in kids.values_mut() {
            v.sort_by_key(|k| key(k));
        }
        let mut order = HashMap::new();
        let mut leaf = HashSet::new();
        let mut stack: Vec<String> = kids.get(&None).cloned().unwrap_or_default().into_iter().rev().collect();
        while let Some(k) = stack.pop() {
            order.insert(k.clone(), order.len());
            let children = kids.get(&Some(k.clone())).cloned().unwrap_or_default();
            // Counted: a test or case with no test or case under it
            // (Decisions 7): a template once its cases are found isn't.
            if is_test(merged[&k].kind) && !children.iter().any(|c| is_test(merged[c].kind)) {
                leaf.insert(k.clone());
            }
            stack.extend(children.into_iter().rev());
        }
        Shape { parent, kids, order, leaf }
    }

    /// Where Fenix put `id`: its parent if it has one, else none (Decisions 6).
    pub fn parent(&self, id: &str) -> Option<String> {
        self.shape().parent.get(id).cloned().flatten()
    }

    /// Its children's ids, as the page lists them.
    pub fn children(&self, id: Option<&str>) -> Vec<String> {
        self.shape().kids.get(&id.map(String::from)).cloned().unwrap_or_default()
    }

    /// Its place in the page's order.
    pub fn order(&self, id: &str) -> usize {
        self.shape().order.get(id).copied().unwrap_or(usize::MAX)
    }

    /// Whether `id` is counted: a test or case with none under it.
    pub fn is_leaf(&self, id: &str) -> bool {
        self.shape().leaf.contains(id)
    }

    /// Everything under `id`, in the page's order.
    pub fn descendants(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack: Vec<String> = self.children(Some(id)).into_iter().rev().collect();
        while let Some(at) = stack.pop() {
            stack.extend(self.children(Some(&at)).into_iter().rev());
            out.push(at);
        }
        out
    }

    /// What's counted under `id`, itself included (Decisions 7).
    pub fn tests_under(&self, id: &str) -> Vec<String> {
        std::iter::once(id.to_string()).chain(self.descendants(id)).filter(|d| self.is_leaf(d)).collect()
    }

    /// Every test counted.
    pub fn all_tests(&self) -> Vec<String> {
        self.nodes.keys().filter(|k| self.is_leaf(k)).cloned().collect()
    }

    /// `id` and those it's in, nearest first.
    pub fn ancestry(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_string()];
        let mut at = self.parent(id);
        while let Some(p) = at {
            at = self.parent(&p);
            out.push(p);
        }
        out
    }

    /// The node's depth: how many are above it.
    pub fn depth(&self, id: &str) -> usize {
        self.ancestry(id).len() - 1
    }

    /// Whether its provider can name `id` to its runner: its id isn't a
    /// cut one, or its `data` is kept (Decisions 32).
    pub fn can_be_named(&self, id: &str) -> bool {
        !is_cut_id(id) || self.record(id).is_some_and(|r| r.data.is_some())
    }

    /// Source `source`'s record of `id` set (for a scan, `file`'s). Its
    /// `data` isn't kept past the provider's 16 MiB; it says so.
    pub fn set(&mut self, id: &str, source: Source, file: Option<&str>, mut record: Record) -> Option<&'static str> {
        self.shape.take();
        self.seq += 1;
        let seq = self.seq;
        let new = !self.nodes.contains_key(id);
        let node = self.nodes.entry(id.to_string()).or_default();
        if new {
            node.seq = seq;
            if let Some((state, detail)) = self.restored.remove(id) {
                node.state = state;
                node.result = detail;
            }
        }
        let before = node.data_bytes();
        let replaced = match source {
            Source::Scan => node.scans.get(file.unwrap_or_default()).and_then(|(_, r)| r.data.as_ref()).map_or(0, String::len),
            _ => node.records[source as usize].as_ref().and_then(|r| r.data.as_ref()).map_or(0, String::len),
        };
        let incoming = record.data.as_ref().map_or(0, String::len);
        let mut said = None;
        if incoming > 0 && self.data_bytes.saturating_sub(replaced) + incoming > DATA_PER_PROVIDER {
            record.data = None;
            said = Some("past 16 MiB of tests' data, a test's data isn't kept");
        }
        match source {
            Source::Scan => {
                node.scans.insert(file.unwrap_or_default().to_string(), (seq, record));
                node.records[0] = node.scans.values().max_by_key(|(s, _)| *s).map(|(_, r)| r.clone());
            }
            _ => node.records[source as usize] = Some(record),
        }
        let after = node.data_bytes();
        self.data_bytes = self.data_bytes.saturating_sub(before) + after;
        said
    }

    /// Source `source`'s record of `id` dropped (for a scan, `file`'s),
    /// and the node with it when no source has it. Its result is kept for
    /// it should it come back (Decisions 29), unless `with_result`.
    pub fn drop_record(&mut self, id: &str, source: Source, file: Option<&str>, with_result: bool) {
        self.shape.take();
        let Some(n) = self.nodes.get_mut(id) else { return };
        let before = n.data_bytes();
        match source {
            Source::Scan => {
                n.scans.remove(file.unwrap_or_default());
                n.records[0] = n.scans.values().max_by_key(|(s, _)| *s).map(|(_, r)| r.clone());
            }
            _ => n.records[source as usize] = None,
        }
        let after = n.data_bytes();
        self.data_bytes = self.data_bytes.saturating_sub(before) + after;
        if n.records.iter().all(Option::is_none) {
            let n = self.nodes.remove(id).expect("held");
            if !with_result && matches!(n.state, State::Passed | State::Failed | State::Skipped | State::Errored) {
                self.restored.insert(id.to_string(), (n.state, n.result));
            }
        }
    }

    /// Every node's `data` gone, as after Fenix starts again, until a
    /// scan, listing or run gives it again (Fenix §19, finding 2).
    pub fn forget_data(&mut self) {
        for n in self.nodes.values_mut() {
            for (_, r) in n.scans.values_mut() {
                r.data = None;
            }
            for r in n.records.iter_mut().flatten() {
                r.data = None;
            }
        }
        self.data_bytes = 0;
    }

    /// Its state: a test's own; a group's -- and a template's once its
    /// cases are found -- from what's counted under it, and its own result
    /// if it has one (Fenix §4; Decisions 8): failed, errored, running,
    /// queued, passed, else unknown.
    pub fn state(&self, id: &str) -> State {
        let Some(n) = self.nodes.get(id) else { return State::Unknown };
        if self.is_leaf(id) {
            return n.state;
        }
        let kind = n.merged().map(|r| r.kind);
        let own = (kind.is_some_and(is_test) && n.state != State::Unknown).then_some(n.state);
        let states: Vec<State> = self.tests_under(id).iter().map(|t| self.nodes[t].state).chain(own).collect();
        [State::Failed, State::Errored, State::Running, State::Queued, State::Passed].into_iter().find(|s| states.contains(s)).unwrap_or(State::Unknown)
    }
}

pub(crate) fn is_test(kind: Kind) -> bool {
    matches!(kind, Kind::Test | Kind::Case)
}
