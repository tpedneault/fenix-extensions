//! Fenix's model of a provider's tests, by its rules (Fenix's
//! `docs/plans/test-explorer.md`, §4): each node with a record per source
//! -- the scan of a file, the listing, a run -- merged field by field;
//! each source dropping its own; a result per test, the last `ended`
//! winning; a group's state worked out from its children.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use fenix_lang_kit::tests::api::{Found, Kind, Outcome};

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
        Record { parent: f.parent.clone(), label: f.label.clone(), kind: f.kind, path, line: f.line, end_line: f.end_line, skip: f.skip.clone(), template: f.template, data: f.data.clone() }
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
    pub records: [Option<Record>; 3],
    /// The files whose last scan gave it.
    pub scanned_by: BTreeSet<String>,
    pub state: State,
    pub result: Detail,
    /// What it printed in its runs, its last 64 KiB.
    pub printed: String,
    /// Added by an `ended` Fenix didn't know (Fenix §4, matching rule 3).
    pub found_in_run: bool,
}

impl Node {
    pub fn merged(&self) -> Option<Record> {
        let first = self.records.iter().flatten().next()?;
        let field = |f: fn(&Record) -> Option<String>| self.records.iter().flatten().find_map(f);
        let num = |f: fn(&Record) -> Option<u32>| self.records.iter().flatten().find_map(f);
        // `skip` is the scan's where the scan has it, `none` included.
        let skip = match &self.records[Source::Scan as usize] {
            Some(scan) => scan.skip.clone(),
            None => field(|r| r.skip.clone()),
        };
        Some(Record {
            parent: first.parent.clone(),
            label: first.label.clone(),
            kind: first.kind,
            path: field(|r| r.path.clone()),
            line: num(|r| r.line),
            end_line: num(|r| r.end_line),
            skip,
            template: first.template,
            data: field(|r| r.data.clone()),
        })
    }
}

/// Each node's children, in the page's order.
type Kids = HashMap<Option<String>, Vec<String>>;

/// One provider's tests.
#[derive(Debug, Default)]
pub struct Model {
    pub nodes: BTreeMap<String, Node>,
    /// Children by parent, worked out again after a record changes.
    kids: RefCell<Option<Kids>>,
    /// Each file's last scan: the ids it gave.
    pub scans: BTreeMap<String, Vec<String>>,
    /// What `test-provider-set` last said: runner, problem.
    pub runner: Option<(String, Option<String>)>,
}

impl Model {
    pub fn record(&self, id: &str) -> Option<Record> {
        self.nodes.get(id)?.merged()
    }

    pub fn has(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// Its children's ids, as the page lists them: by line, then label.
    pub fn children(&self, id: Option<&str>) -> Vec<String> {
        let mut cache = self.kids.borrow_mut();
        let kids = cache.get_or_insert_with(|| {
            let mut all: HashMap<Option<String>, Vec<(u32, String, String)>> = HashMap::new();
            for (k, n) in &self.nodes {
                if let Some(r) = n.merged() {
                    all.entry(r.parent).or_default().push((r.line.unwrap_or(u32::MAX), r.label, k.clone()));
                }
            }
            all.into_iter()
                .map(|(p, mut v)| {
                    v.sort();
                    (p, v.into_iter().map(|(_, _, k)| k).collect())
                })
                .collect()
        });
        kids.get(&id.map(String::from)).cloned().unwrap_or_default()
    }

    /// Everything under `id`, deepest last, in the page's order.
    pub fn descendants(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![id.to_string()];
        while let Some(at) = stack.pop() {
            let kids = self.children(Some(&at));
            for k in kids.iter().rev() {
                stack.push(k.clone());
            }
            if at != id {
                out.push(at);
            }
        }
        out
    }

    /// `id` and what's under it that runs: its tests and cases.
    pub fn tests_under(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.record(id).is_some_and(|r| matches!(r.kind, Kind::Test | Kind::Case)) {
            out.push(id.to_string());
        }
        out.extend(self.descendants(id).into_iter().filter(|d| self.record(d).is_some_and(|r| matches!(r.kind, Kind::Test | Kind::Case))));
        out
    }

    /// Every test and case.
    pub fn all_tests(&self) -> Vec<String> {
        self.nodes.iter().filter(|(_, n)| n.merged().is_some_and(|r| matches!(r.kind, Kind::Test | Kind::Case))).map(|(k, _)| k.clone()).collect()
    }

    /// The node's depth: how many are above it.
    pub fn depth(&self, id: &str) -> usize {
        let mut d = 0;
        let mut cur = self.record(id).and_then(|r| r.parent);
        while let Some(p) = cur {
            d += 1;
            cur = self.record(&p).and_then(|r| r.parent);
            if d > 10_000 {
                break;
            }
        }
        d
    }

    /// Source `source`'s record of `id` set.
    pub fn set(&mut self, id: &str, source: Source, record: Record) {
        self.kids.take();
        self.nodes.entry(id.to_string()).or_default().records[source as usize] = Some(record);
    }

    /// Source `source`'s record of `id` dropped, and the node with it when
    /// no source has it.
    pub fn drop_record(&mut self, id: &str, source: Source) {
        self.kids.take();
        if let Some(n) = self.nodes.get_mut(id) {
            n.records[source as usize] = None;
            if n.records.iter().all(Option::is_none) {
                self.nodes.remove(id);
            }
        }
    }

    /// A state worked out from children, for a group (and a node whose own
    /// state is unknown, a template with cases): failed if any is, then
    /// errored, running, passed if all that ran passed.
    pub fn state(&self, id: &str) -> State {
        let Some(n) = self.nodes.get(id) else { return State::Unknown };
        let kind = n.merged().map(|r| r.kind);
        if matches!(kind, Some(Kind::Test | Kind::Case)) {
            return n.state;
        }
        let states: Vec<State> = self.children(Some(id)).iter().map(|c| self.state(c)).collect();
        for s in [State::Failed, State::Errored, State::Running, State::Queued] {
            if states.contains(&s) {
                return s;
            }
        }
        if states.contains(&State::Passed) {
            State::Passed
        } else {
            State::Unknown
        }
    }
}
