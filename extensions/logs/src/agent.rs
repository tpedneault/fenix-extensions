//! What coding agents are given (`logs_query`, `logs_summary`): a log in
//! the project read as the viewer reads it -- the same formats, the same
//! entries (a stack trace with the line it follows), the same filters --
//! and only read.
//!
//! A log can be gigabytes, and a call has milliseconds, so a tool's work
//! is a job done in steps (`Job::step`, a few hundred KiB each) on the
//! viewer's timer, the agent answered once it's done. Each call reads at
//! most [`SCAN_CAP`] of the file, and gives back at most `limit` entries
//! and `max_bytes` of their text: past any of them, the answer says which
//! (`truncated`) and gives a cursor to read on from.
//!
//! A tool reads a file in the open project, named from its root: no `..`,
//! no absolute path, and no link on the way, so nothing outside the
//! project is reached through one.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::detect::{self, journal_summary, Level, Rules, LEVELS};
use crate::entry::{starts_entry, Entry, Grouper, Grouping};
use crate::filter::{Field, Filter, Matcher, Range};
use crate::hist::Histogram;
use crate::index::grouping_of;
use crate::source::{Lines, Next, Source};
use crate::time;

/// The services the tools call.
pub const QUERY: &str = "logs.query";
pub const SUMMARY: &str = "logs.summary";

/// Entries an answer gives: by default, and at most.
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 500;
/// Bytes of entries' text an answer gives: by default, at least, at most.
pub const DEFAULT_BYTES: usize = 16 * 1024;
pub const MIN_BYTES: usize = 1024;
pub const MAX_BYTES: usize = 256 * 1024;
/// What of one entry's text is given: a stack trace's first lines.
pub const ENTRY_BYTES: usize = 4096;
/// The most of the file a call reads.
pub const SCAN_CAP: u64 = 128 * 1024 * 1024;
/// What a step reads, about: a few milliseconds' work.
pub const STEP: u64 = 256 * 1024;
/// Newest first, the file is read back from its end a window at a time.
const WINDOW: u64 = 4 * 1024 * 1024;
/// How much of its end is read for its last time (`-15m`, `12:00`).
const TAIL: u64 = 1024 * 1024;
/// How many entries' first lines the summary's formats are told from.
const FORMAT_SAMPLE: u64 = 1000;
/// The summary's spans of time: by default, and at most.
pub const DEFAULT_SPANS: usize = 24;
pub const MAX_SPANS: usize = 100;

/// `typed`, a path from the project's root, as a file under `root`: its
/// name as given back (`logs/app.log`) and its full path. Refused: an
/// absolute path, a `..`, a link anywhere on the way, a folder.
pub fn project_file(root: &str, typed: &str) -> Result<(String, String), String> {
    let t = typed.trim().replace('\\', "/");
    if t.is_empty() {
        return Err("which log? \"path\": a file's path from the project's root, logs/app.log".into());
    }
    if t.contains('\0') || t.starts_with('/') || t.starts_with('~') || t.as_bytes().get(1) == Some(&b':') {
        return Err(format!("{typed}: a path from the project's root, logs/app.log -- not an absolute one"));
    }
    let mut parts = Vec::new();
    for p in t.split('/') {
        match p {
            "" | "." => {}
            ".." => return Err(format!("{typed}: a path in the project, without ..")),
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(format!("{typed}: a file's path, not the project's root"));
    }
    if std::fs::symlink_metadata(root).is_err() {
        return Err("no project is open in Fenix: a log is read from the project's files".into());
    }
    let rel = parts.join("/");
    let mut at = PathBuf::from(root);
    for (i, p) in parts.iter().enumerate() {
        at.push(p);
        let shown = parts[..=i].join("/");
        let meta = std::fs::symlink_metadata(&at).map_err(|e| format!("{shown}: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{shown} is a link: only the project's own files are read"));
        }
        let last = i + 1 == parts.len();
        if !last && !meta.is_dir() {
            return Err(format!("{shown} isn't a folder"));
        }
        if last && !meta.is_file() {
            return Err(format!("{rel} isn't a file"));
        }
    }
    Ok((rel, at.to_string_lossy().into_owned()))
}

/// An entry found.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub offset: u64,
    /// Its first line's number, from 1, when it's known (oldest first).
    pub line: Option<u64>,
    pub lines: u32,
    pub level: Level,
    pub time: Option<i64>,
    pub text: String,
    /// Its text was cut short.
    pub cut: bool,
}

impl Hit {
    fn new(e: &Entry, text: &[u8], line: Option<u64>) -> Hit {
        let summary = if text.starts_with(b"__CURSOR=") { journal_summary(text) } else { None };
        let whole = match &summary {
            Some(s) => s.clone(),
            None => String::from_utf8_lossy(text).into_owned(),
        };
        let (text, cut_here) = cut(&whole, ENTRY_BYTES);
        // The reader keeps a long line's start only: its lines and their
        // newlines are fewer bytes than the entry.
        let short = summary.is_none() && (e.len as usize) > whole.len() + 2 * e.lines as usize;
        Hit { offset: e.offset, line: line.map(|l| l + 1), lines: e.lines, level: e.level, time: e.time, text, cut: cut_here || short }
    }

    fn json(&self) -> Value {
        let mut v = json!({ "offset": self.offset, "level": self.level.name(), "lines": self.lines, "text": self.text });
        if let Some(l) = self.line {
            v["line"] = json!(l);
        }
        if let Some(t) = self.time {
            v["time"] = json!(time::format(t));
        }
        if self.cut {
            v["cut"] = json!(true);
        }
        v
    }
}

/// `s` in at most `max` bytes, cut at a character's edge; whether it was.
fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// A log's entries from one place to another, a step at a time: each
/// closed one given with its text (its lines' starts, as the filters see
/// them) and its first line's number from where it started.
struct Walker {
    lines: Lines,
    g: Grouper,
    buf: Vec<u8>,
    line: u64,
    open_line: u64,
}

impl Walker {
    fn new(from: u64, to: u64, grouping: Grouping, first_line: u64) -> Walker {
        Walker { lines: Lines::new(from, to), g: Grouper::new(grouping), buf: Vec::new(), line: first_line, open_line: first_line }
    }

    /// Up to `budget` bytes more read; whether there's more. At the end,
    /// the entry still open is given too.
    fn step(&mut self, src: &Source, rules: &Rules, budget: u64, each: &mut dyn FnMut(&Entry, &[u8], u64)) -> bool {
        let start = self.lines.offset();
        loop {
            if self.lines.offset() - start >= budget {
                return true;
            }
            match self.lines.next(src, true) {
                Next::Line(l) => {
                    if let Some(done) = self.g.line(l.offset, l.text, l.len, rules) {
                        each(&done, &self.buf, self.open_line);
                        self.buf.clear();
                        self.open_line = self.line;
                    } else if !self.buf.is_empty() {
                        self.buf.push(b'\n');
                    }
                    self.buf.extend_from_slice(l.text);
                    self.line += 1;
                }
                Next::End => {
                    if let Some(done) = self.g.finish() {
                        each(&done, &self.buf, self.open_line);
                        self.buf.clear();
                    }
                    return false;
                }
            }
        }
    }

    /// Where the entry still open starts, and its first line: where to
    /// read on from.
    fn resume(&self) -> (u64, u64) {
        match self.g.open() {
            Some(e) => (e.offset, self.open_line),
            None => (self.lines.offset(), self.line),
        }
    }
}

/// The first entry that starts at `at` or after it, before `to`: past the
/// rest of the line `at` is in, and past lines that go with an entry
/// before. A stretch with none is read from its first whole line.
fn entry_start(src: &Source, at: u64, to: u64, grouping: Grouping, rules: &Rules) -> u64 {
    if at == 0 {
        return 0;
    }
    let mut lines = Lines::new(at, to);
    let mut whole = src.read(at - 1, 1) == b"\n";
    let mut first_whole = None;
    while let Next::Line(l) = lines.next(src, true) {
        if !whole {
            whole = true;
            continue;
        }
        first_whole.get_or_insert(l.offset);
        if starts_entry(grouping, l.text, rules) {
            return l.offset;
        }
    }
    first_whole.unwrap_or(at)
}

/// The last time among the entries of the last [`TAIL`] of a log.
fn last_time(src: &Source, grouping: Grouping, rules: &Rules) -> Option<i64> {
    let len = src.len();
    let from = entry_start(src, len.saturating_sub(TAIL), len, grouping, rules);
    let mut w = Walker::new(from, len, grouping, 0);
    let mut last: Option<i64> = None;
    while w.step(src, rules, u64::MAX, &mut |e, _, _| {
        if let Some(t) = e.time {
            last = Some(last.map_or(t, |l| l.max(t)));
        }
    }) {}
    last
}

fn text_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn int_arg(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("\"{key}\" is a whole number, 1 or more")),
    }
}

/// Why a query stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// It read all there was to read.
    End,
    /// It found one more than `limit`.
    Limit,
    /// It read [`SCAN_CAP`]: what's left starts (or ends) here, and the
    /// line it's on.
    Cap(u64, u64),
}

/// A window of the file, newest first: read whole before its entries,
/// newest first, are taken.
struct Window {
    start: u64,
    walker: Walker,
    found: VecDeque<Hit>,
}

enum Stage {
    /// Its time range still to be read: the log's last time first.
    Start,
    Forward(Walker),
    Back { end: u64, window: Option<Window> },
    Done(Stop),
    Failed(String),
}

/// `logs_query`: the entries of a log a filter keeps, newest or oldest
/// first, a bounded number and size of them.
pub struct Query {
    rel: String,
    src: Source,
    rules: Rc<Rules>,
    grouping: Grouping,
    filter: Filter,
    min_level: Option<Level>,
    range: Option<String>,
    newest: bool,
    limit: usize,
    max_bytes: usize,
    /// Where it reads from (oldest first: an entry's start and its line)
    /// or to (newest first).
    from: (u64, u64),
    stage: Stage,
    hits: Vec<Hit>,
    scanned: u64,
    /// The most it reads: [`SCAN_CAP`].
    cap: u64,
    /// The bytes read: from, to.
    covered: (u64, u64),
}

impl Query {
    /// A query of the log `rel` (its bytes `src`), as the agent asked.
    pub fn new(rel: &str, src: Source, rules: Rc<Rules>, args: &Value) -> Result<Query, String> {
        Query::with_cap(rel, src, rules, args, SCAN_CAP)
    }

    /// One that reads at most `cap` bytes.
    pub fn with_cap(rel: &str, src: Source, rules: Rc<Rules>, args: &Value, cap: u64) -> Result<Query, String> {
        let min_level = match text_arg(args, "level") {
            None => None,
            Some(w) => Some(Level::word(&w).ok_or_else(|| format!("level {w:?}: fatal, error, warn, info, debug or trace"))?),
        };
        let text = text_arg(args, "text").map(|t| Matcher::new(&t)).transpose()?;
        let exclude = text_arg(args, "exclude").map(|t| Matcher::new(&t)).transpose()?.into_iter().collect();
        let fields: Vec<String> = match args.get("fields") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).ok_or("\"fields\" are texts: user=bob, status!=200")).collect::<Result<_, _>>()?,
            Some(_) => return Err("\"fields\" are texts: user=bob, status!=200".into()),
        };
        let fields = fields.iter().map(|f| Field::new(f)).collect::<Result<_, _>>()?;
        let filter = Filter { text, exclude, fields, ..Filter::default() };
        let newest = match text_arg(args, "order").as_deref() {
            None | Some("newest") => true,
            Some("oldest") => false,
            Some(o) => return Err(format!("order {o:?}: newest or oldest")),
        };
        let limit = int_arg(args, "limit")?.map_or(DEFAULT_LIMIT, |n| (n as usize).clamp(1, MAX_LIMIT));
        let max_bytes = int_arg(args, "max_bytes")?.map_or(DEFAULT_BYTES, |n| (n as usize).clamp(MIN_BYTES, MAX_BYTES));
        let len = src.len();
        let from = match text_arg(args, "cursor") {
            None => (if newest { len } else { 0 }, 0),
            Some(c) => cursor(&c, newest, len)?,
        };
        let grouping = grouping_of(&src, &rules);
        Ok(Query {
            rel: rel.to_string(),
            src,
            rules,
            grouping,
            filter,
            min_level,
            range: text_arg(args, "time"),
            newest,
            limit,
            max_bytes,
            from,
            stage: Stage::Start,
            hits: Vec::new(),
            scanned: 0,
            cap,
            covered: (from.0, from.0),
        })
    }

    pub fn done(&self) -> bool {
        matches!(self.stage, Stage::Done(_) | Stage::Failed(_))
    }

    /// About `budget` bytes more read; whether there's more to do.
    pub fn step(&mut self, budget: u64) -> bool {
        let Query { src, rules, grouping, filter, min_level, newest, limit, from, stage, hits, scanned, covered, range, cap, .. } = self;
        let (src, rules, grouping, limit, cap) = (&*src, &**rules, *grouping, *limit, *cap);
        let keeps = |e: &Entry, text: &[u8]| min_level.is_none_or(|m| e.level <= m) && filter.keeps(e.level, e.time, text);
        match stage {
            Stage::Start => {
                if let Some(typed) = range.as_deref() {
                    match Range::new(typed, last_time(src, grouping, rules), rules.year) {
                        Ok(r) => filter.range = Some(r),
                        Err(e) => {
                            *stage = Stage::Failed(format!("time: {e}"));
                            return false;
                        }
                    }
                }
                *stage = if *newest { Stage::Back { end: from.0, window: None } } else { Stage::Forward(Walker::new(from.0, src.len(), grouping, from.1)) };
                true
            }
            Stage::Forward(w) => {
                let more = w.step(src, rules, budget, &mut |e, text, line| {
                    if hits.len() <= limit && keeps(e, text) {
                        hits.push(Hit::new(e, text, Some(line)));
                    }
                });
                let at = w.resume();
                *scanned = at.0 - from.0;
                covered.1 = if more { at.0 } else { src.len() };
                *stage = if hits.len() > limit {
                    Stage::Done(Stop::Limit)
                } else if !more {
                    Stage::Done(Stop::End)
                } else if *scanned >= cap {
                    Stage::Done(Stop::Cap(at.0, at.1))
                } else {
                    return true;
                };
                false
            }
            Stage::Back { end, window } => {
                if window.is_none() {
                    if *end == 0 {
                        *stage = Stage::Done(Stop::End);
                        return false;
                    }
                    if *scanned >= cap {
                        *stage = Stage::Done(Stop::Cap(*end, 0));
                        return false;
                    }
                    let raw = end.saturating_sub(WINDOW);
                    let mut start = entry_start(src, raw, *end, grouping, rules);
                    if start >= *end {
                        start = raw;
                    }
                    *window = Some(Window { start, walker: Walker::new(start, *end, grouping, 0), found: VecDeque::new() });
                }
                let Some(Window { start, walker, found }) = window.as_mut() else { return false };
                let need = limit + 1 - hits.len().min(limit);
                let more = walker.step(src, rules, budget, &mut |e, text, _| {
                    if keeps(e, text) {
                        found.push_back(Hit::new(e, text, None));
                        if found.len() > need {
                            found.pop_front();
                        }
                    }
                });
                if more {
                    return true;
                }
                *scanned += *end - *start;
                hits.extend(found.drain(..).rev());
                *end = *start;
                covered.0 = *start;
                *window = None;
                if hits.len() > limit {
                    *stage = Stage::Done(Stop::Limit);
                    return false;
                }
                true
            }
            Stage::Done(_) | Stage::Failed(_) => false,
        }
    }

    /// The answer, once it's done.
    pub fn answer(&self) -> Result<Value, String> {
        let stop = match &self.stage {
            Stage::Failed(e) => return Err(e.clone()),
            Stage::Done(s) => *s,
            _ => return Err("not done yet".into()),
        };
        let mut given: Vec<&Hit> = Vec::new();
        let mut used = 0;
        let mut by_bytes = false;
        let mut text_cut = None;
        for h in self.hits.iter().take(self.limit) {
            if used + h.text.len() > self.max_bytes {
                if given.is_empty() {
                    // The first is given whatever it takes, cut to fit.
                    text_cut = Some(cut(&h.text, self.max_bytes).0);
                    given.push(h);
                }
                by_bytes = true;
                break;
            }
            used += h.text.len();
            given.push(h);
        }
        let left = given.len() < self.hits.len();
        let (truncated, next) = if left {
            let next = if self.newest {
                format!("b{}", given.last().map_or(self.from.0, |h| h.offset))
            } else {
                let h = &self.hits[given.len()];
                format!("a{}:{}", h.offset, h.line.unwrap_or(1) - 1)
            };
            (Some(if by_bytes { "bytes" } else { "limit" }), Some(next))
        } else {
            match stop {
                Stop::Cap(at, line) => (Some("scan"), Some(if self.newest { format!("b{at}") } else { format!("a{at}:{line}") })),
                _ => (None, None),
            }
        };
        let entries: Vec<Value> = given
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let mut v = h.json();
                if let (0, Some(t)) = (i, &text_cut) {
                    v["text"] = json!(t);
                    v["cut"] = json!(true);
                }
                v
            })
            .collect();
        let (a, z) = if self.newest { (self.covered.0, self.from.0) } else { (self.from.0, self.covered.1) };
        Ok(json!({
            "path": self.rel,
            "bytes": self.src.len(),
            "order": if self.newest { "newest first" } else { "oldest first" },
            "returned": entries.len(),
            "entries": entries,
            "truncated": truncated,
            "next_cursor": next,
            "read": { "from": a, "to": z },
            "entries_by": grouping_name(self.grouping),
        }))
    }
}

/// A cursor as an answer gave it: `a<offset>:<line>` for oldest first,
/// the entries from there; `b<offset>` newest first, those before.
fn cursor(c: &str, newest: bool, len: u64) -> Result<(u64, u64), String> {
    let bad = || format!("cursor {c:?}: give next_cursor as the last answer had it, with the same order");
    let (order, rest) = c.split_at(c.chars().next().map_or(0, char::len_utf8));
    let (offset, line) = match rest.split_once(':') {
        Some((o, l)) => (o.parse::<u64>().map_err(|_| bad())?, l.parse::<u64>().map_err(|_| bad())?),
        None => (rest.parse::<u64>().map_err(|_| bad())?, 0),
    };
    if (order == "b") != newest || !matches!(order, "a" | "b") {
        return Err(bad());
    }
    if offset > len {
        return Err(format!("cursor {c:?}: the log is shorter than it was -- truncated or rotated: start again without a cursor"));
    }
    Ok((offset, line))
}

fn grouping_name(g: Grouping) -> &'static str {
    match g {
        Grouping::Heads => "a line with a time or a level starts one; the lines after go with it",
        Grouping::Indent => "each line, and indented lines after it go with it",
    }
}

/// `logs_summary`: a log at a glance -- its formats, its entries by
/// level, its times, and counts over time -- over all of it, or its
/// newest [`SCAN_CAP`].
pub struct Summary {
    rel: String,
    src: Source,
    rules: Rc<Rules>,
    grouping: Grouping,
    from: u64,
    walker: Walker,
    spans: usize,
    counts: [u64; LEVELS],
    entries: u64,
    hist: Histogram,
    formats: Vec<(&'static str, u64)>,
    done: bool,
}

impl Summary {
    pub fn new(rel: &str, src: Source, rules: Rc<Rules>, args: &Value) -> Result<Summary, String> {
        Summary::with_cap(rel, src, rules, args, SCAN_CAP)
    }

    /// One that reads at most about `cap` bytes: the newest of the log.
    pub fn with_cap(rel: &str, src: Source, rules: Rc<Rules>, args: &Value, cap: u64) -> Result<Summary, String> {
        let spans = int_arg(args, "buckets")?.map_or(DEFAULT_SPANS, |n| (n as usize).clamp(1, MAX_SPANS));
        let grouping = grouping_of(&src, &rules);
        let len = src.len();
        let from = if len > cap { entry_start(&src, len - cap, len, grouping, &rules) } else { 0 };
        Ok(Summary {
            rel: rel.to_string(),
            walker: Walker::new(from, len, grouping, 0),
            src,
            rules,
            grouping,
            from,
            spans,
            counts: [0; LEVELS],
            entries: 0,
            hist: Histogram::default(),
            formats: Vec::new(),
            done: false,
        })
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn step(&mut self, budget: u64) -> bool {
        let Summary { src, rules, walker, counts, entries, hist, formats, done, .. } = self;
        let rules = &**rules;
        let more = walker.step(src, rules, budget, &mut |e, text, _| {
            counts[e.level.index()] += 1;
            *entries += 1;
            if let Some(t) = e.time {
                hist.add(t, e.level);
            }
            if *entries <= FORMAT_SAMPLE {
                let head = &text[..memchr::memchr(b'\n', text).unwrap_or(text.len())];
                let f = detect::format(head, rules);
                match formats.iter_mut().find(|(n, _)| *n == f) {
                    Some((_, k)) => *k += 1,
                    None => formats.push((f, 1)),
                }
            }
        });
        *done = !more;
        more
    }

    pub fn answer(&self) -> Result<Value, String> {
        let mut formats = self.formats.clone();
        formats.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let levels: serde_json::Map<String, Value> = Level::ALL.iter().map(|l| (l.name().to_string(), json!(self.counts[l.index()]))).collect();
        let spans: Vec<Value> = self
            .hist
            .spans(self.spans)
            .into_iter()
            .map(|(a, z, c)| json!({ "from": time::format(a), "to": time::format(z), "entries": c[0], "warn": c[1], "error": c[2] }))
            .collect();
        let len = self.src.len();
        let mut out = json!({
            "path": self.rel,
            "bytes": len,
            "read": { "from": self.from, "to": len, "whole": self.from == 0 },
            "entries": self.entries,
            "lines": self.walker.line,
            "levels": levels,
            "formats": formats.iter().map(|(f, n)| json!({ "format": f, "entries": n })).collect::<Vec<_>>(),
            "formats_of": format!("its first {} entries read", self.entries.min(FORMAT_SAMPLE)),
            "entries_by": grouping_name(self.grouping),
            "over_time": spans,
        });
        if let Some((a, z)) = self.hist.range {
            out["first_time"] = json!(time::format(a));
            out["last_time"] = json!(time::format(z));
        }
        if self.from > 0 {
            out["note"] = json!(format!("only its newest {} of {} bytes: its last entries, from byte {}", len - self.from, len, self.from));
        }
        Ok(out)
    }
}

/// A tool call under way.
pub enum Job {
    Query(Box<Query>),
    Summary(Box<Summary>),
}

impl Job {
    /// The job service `name` asks for, on log `rel`.
    pub fn new(name: &str, rel: &str, src: Source, rules: Rc<Rules>, args: &Value) -> Result<Job, String> {
        match name {
            QUERY => Query::new(rel, src, rules, args).map(|q| Job::Query(Box::new(q))),
            SUMMARY => Summary::new(rel, src, rules, args).map(|s| Job::Summary(Box::new(s))),
            other => Err(format!("no service {other}")),
        }
    }

    pub fn step(&mut self, budget: u64) -> bool {
        match self {
            Job::Query(q) => q.step(budget),
            Job::Summary(s) => s.step(budget),
        }
    }

    pub fn done(&self) -> bool {
        match self {
            Job::Query(q) => q.done(),
            Job::Summary(s) => s.done(),
        }
    }

    pub fn answer(&self) -> Result<Value, String> {
        match self {
            Job::Query(q) => q.answer(),
            Job::Summary(s) => s.answer(),
        }
    }

    /// Run to its end: for tests, and a small log answered at once.
    pub fn finish(&mut self, budget: u64) -> Result<Value, String> {
        while !self.done() {
            self.step(budget);
        }
        self.answer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::scan;
    use crate::index::Index;

    fn rules() -> Rc<Rules> {
        Rc::new(Rules { custom: Vec::new(), year: 2024 })
    }

    /// A fresh folder of its own under the system's.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-logs-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn src(text: &str) -> Source {
        Source::bytes(text.as_bytes().to_vec())
    }

    fn run(text: &str, args: Value) -> Result<Value, String> {
        Job::new(QUERY, "app.log", src(text), rules(), &args)?.finish(STEP)
    }

    fn texts(answer: &Value) -> Vec<String> {
        answer["entries"].as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap().lines().next().unwrap().to_string()).collect()
    }

    /// `n` entries at a second each, every fifth an error with a stack
    /// trace of two lines.
    fn log(n: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            let level = ["INFO", "WARN", "INFO", "DEBUG", "ERROR"][i % 5];
            s.push_str(&format!("2024-03-01 12:{:02}:{:02} {level} job={i} user={} request {i}\n", i / 60 % 60, i % 60, ["ada", "bob"][i % 2]));
            if i % 5 == 4 {
                s.push_str("    at app.Worker.run(Worker.java:42)\n    at java.lang.Thread.run(Thread.java:833)\n");
            }
        }
        s
    }

    #[test]
    fn a_path_is_the_projects_own_file() {
        let dir = folder("paths");
        let root = dir.join("p");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/app.log"), "x\n").unwrap();
        std::fs::write(dir.join("secret.txt"), "outside\n").unwrap();
        let root = root.to_str().unwrap();
        let ok = |t: &str| project_file(root, t).map(|(rel, _)| rel);
        assert_eq!(ok("logs/app.log").as_deref(), Ok("logs/app.log"));
        assert_eq!(ok("./logs//app.log").as_deref(), Ok("logs/app.log"));
        assert_eq!(ok("logs\\app.log").as_deref(), Ok("logs/app.log"));
        assert!(project_file(root, "logs/app.log").unwrap().1.ends_with("p/logs/app.log"));
        for (bad, why) in [
            ("", "which log"),
            ("../secret.txt", ".."),
            ("logs/../../secret.txt", ".."),
            ("/etc/passwd", "absolute"),
            ("C:\\Windows\\win.ini", "absolute"),
            ("~/x.log", "absolute"),
            ("logs", "isn't a file"),
            (".", "root"),
            ("logs/missing.log", "missing.log"),
            ("logs/app.log/x", "isn't a folder"),
        ] {
            let e = project_file(root, bad).unwrap_err();
            assert!(e.contains(why), "{bad:?}: {e}");
        }
        assert!(project_file(dir.join("none").to_str().unwrap(), "a.log").unwrap_err().contains("no project"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_project_is_refused() {
        let dir = folder("links");
        let root = dir.join("p");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(dir.join("secret.log"), "outside\n").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.log"), root.join("logs/app.log")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        let root = root.to_str().unwrap();
        assert!(project_file(root, "logs/app.log").unwrap_err().contains("is a link"));
        assert!(project_file(root, "up/secret.log").unwrap_err().contains("up is a link"), "a linked folder too");
    }

    #[test]
    fn newest_first_by_default_and_the_viewers_entries() {
        let text = log(40);
        let a = run(&text, json!({})).unwrap();
        assert_eq!(a["order"], "newest first");
        assert_eq!(a["returned"], 40);
        assert!(a["truncated"].is_null() && a["next_cursor"].is_null(), "{a}");
        let first = &a["entries"][0];
        assert!(first["text"].as_str().unwrap().starts_with("2024-03-01 12:00:39 ERROR job=39"), "{first}");
        assert_eq!((first["level"].as_str(), first["lines"].as_u64(), first["time"].as_str()), (Some("error"), Some(3), Some("2024-03-01 12:00:39")));
        assert!(first["text"].as_str().unwrap().ends_with("Thread.java:833)"), "the stack trace is the entry's");
        assert!(first.get("line").is_none(), "newest first, lines aren't counted");

        // What the viewer's index finds, the tool finds.
        let s = src(&text);
        let mut i = Index::new(rules(), s.len());
        while i.step(&s, 1 << 20) {}
        let mut viewer = Vec::new();
        for n in 0..i.chunk_count() {
            let v = i.chunk(n).unwrap();
            scan(&s, v.chunk.offset..v.chunk.end, v.eof, i.grouping(), &i.rules, false, |e, _| viewer.push((e.offset, e.level.name(), e.lines)));
        }
        let oldest = run(&text, json!({"order": "oldest"})).unwrap();
        let tool: Vec<(u64, &str, u64)> = oldest["entries"].as_array().unwrap().iter().map(|e| (e["offset"].as_u64().unwrap(), e["level"].as_str().unwrap(), e["lines"].as_u64().unwrap())).collect();
        assert_eq!(tool, viewer.iter().map(|(o, l, n)| (*o, *l, *n as u64)).collect::<Vec<_>>());
        assert_eq!(oldest["entries"][5]["line"], 8, "oldest first: the line, from 1, past the stack trace before it");
    }

    #[test]
    fn filtered_by_level_text_field_and_time() {
        let text = log(200);
        let errors = run(&text, json!({"level": "error"})).unwrap();
        assert_eq!(errors["returned"], 40);
        assert!(errors["entries"].as_array().unwrap().iter().all(|e| e["level"] == "error"));
        let warn = run(&text, json!({"level": "warning", "order": "oldest", "limit": 500})).unwrap();
        assert_eq!(warn["returned"], 80, "warn or worse");
        let stack = run(&text, json!({"text": "worker.java", "limit": 500})).unwrap();
        assert_eq!(stack["returned"], 40, "the stack trace's lines are searched, any case");
        assert_eq!(run(&text, json!({"text": "Worker.JAVA"})).unwrap()["returned"], 0, "a capital: its own case");
        let re = run(&text, json!({"text": "re:job=1\\d\\b", "exclude": "DEBUG", "order": "oldest"})).unwrap();
        assert_eq!(texts(&re).len(), 8, "{re}");
        let bob = run(&text, json!({"fields": ["user=bob"], "level": "error", "limit": 500})).unwrap();
        assert_eq!(bob["returned"], 20);
        assert!(texts(&bob).iter().all(|t| t.contains("user=bob")));
        let minute = run(&text, json!({"time": "12:01..12:01:59", "limit": 500})).unwrap();
        assert_eq!(minute["returned"], 60);
        let last = run(&text, json!({"time": "-9s", "limit": 500})).unwrap();
        assert_eq!(last["returned"], 10, "the log's last ten seconds, by its own last time");
        for (args, said) in [
            (json!({"level": "loud"}), "level"),
            (json!({"time": "soon"}), "time"),
            (json!({"text": "re:("}), "re:("),
            (json!({"fields": ["user"]}), "key=value"),
            (json!({"order": "sideways"}), "order"),
            (json!({"limit": -3}), "whole number"),
            (json!({"cursor": "a10:0"}), "same order"),
        ] {
            let e = run(&text, json!(args.clone())).map(|_| ()).unwrap_err();
            assert!(e.contains(said), "{args}: {e}");
        }
        let fine = run(&text, json!({"order": "oldest", "cursor": "zzz"})).unwrap_err();
        assert!(fine.contains("next_cursor"), "{fine}");
    }

    #[test]
    fn a_limit_and_a_byte_cap_say_so_and_a_cursor_reads_on() {
        let text = log(300);
        for order in ["newest", "oldest"] {
            let mut seen: Vec<u64> = Vec::new();
            let mut cursor: Option<String> = None;
            let mut calls = 0;
            loop {
                let mut args = json!({"order": order, "limit": 7, "level": "warn"});
                if let Some(c) = &cursor {
                    args["cursor"] = json!(c);
                }
                let a = run(&text, args).unwrap();
                seen.extend(a["entries"].as_array().unwrap().iter().map(|e| e["offset"].as_u64().unwrap()));
                calls += 1;
                match a["next_cursor"].as_str() {
                    Some(c) => {
                        assert_eq!(a["truncated"], "limit");
                        cursor = Some(c.to_string());
                    }
                    None => break,
                }
            }
            assert_eq!(seen.len(), 120, "{order}: every warning and error once");
            let mut sorted = seen.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 120, "{order}: none twice");
            if order == "newest" {
                sorted.reverse();
            }
            assert_eq!(seen, sorted, "{order}: in order");
            assert_eq!(calls, 18);
        }
        let small = run(&text, json!({"max_bytes": 1024})).unwrap();
        assert_eq!(small["truncated"], "bytes");
        let given: usize = small["entries"].as_array().unwrap().iter().map(|e| e["text"].as_str().unwrap().len()).sum();
        assert!(given <= 1024 && small["returned"].as_u64().unwrap() > 3, "{given}");
        let next = small["next_cursor"].as_str().unwrap();
        let on = run(&text, json!({"max_bytes": 1024, "cursor": next})).unwrap();
        assert!(on["entries"][0]["offset"].as_u64() < small["entries"][small["returned"].as_u64().unwrap() as usize - 1]["offset"].as_u64());
    }

    #[test]
    fn a_long_entry_is_cut_and_marked() {
        let mut text = String::from("2024-03-01 12:00:00 ERROR huge\n");
        for i in 0..400 {
            text.push_str(&format!("    at frame{i}(X.java:{i})\n"));
        }
        text.push_str("2024-03-01 12:00:01 INFO after\n");
        let a = run(&text, json!({"level": "error"})).unwrap();
        let e = &a["entries"][0];
        assert_eq!((e["lines"].as_u64(), e["cut"].as_bool()), (Some(401), Some(true)));
        assert!(e["text"].as_str().unwrap().len() <= ENTRY_BYTES);
        let tiny = run(&text, json!({"level": "error", "max_bytes": 1})).unwrap();
        assert_eq!(tiny["entries"][0]["text"].as_str().unwrap().len(), MIN_BYTES, "the first is given, cut to fit");
    }

    /// A log over several of the windows newest first reads by: its
    /// entries, a stack trace astride a window's edge too, each found
    /// once and whole.
    #[test]
    fn a_big_log_is_read_back_a_window_at_a_time() {
        let text = log(150_000);
        assert!(text.len() as u64 > 2 * WINDOW, "{}", text.len());
        let all = Job::new(QUERY, "big.log", src(&text), rules(), &json!({"level": "error", "limit": 500, "max_bytes": MAX_BYTES})).unwrap();
        let mut job = all;
        let mut steps = 0;
        while !job.done() {
            job.step(STEP);
            steps += 1;
        }
        assert!(steps > 2, "in steps: {steps}");
        let a = job.answer().unwrap();
        assert_eq!(a["returned"], 500);
        assert_eq!(a["truncated"], "limit");
        let entries = a["entries"].as_array().unwrap();
        assert!(entries.iter().all(|e| e["lines"] == 3), "each error whole, its stack trace with it");
        assert!(entries[0]["text"].as_str().unwrap().contains("job=149999 "));
        assert!(entries[499]["text"].as_str().unwrap().contains(&format!("job={} ", 149_999 - 5 * 499)));
        // Every entry from the end back, against reading it forward.
        let mut cursor: Option<String> = None;
        let mut back = Vec::new();
        loop {
            let mut args = json!({"limit": 500, "max_bytes": MAX_BYTES, "text": "job=1499"});
            if let Some(c) = &cursor {
                args["cursor"] = json!(c);
            }
            let a = run(&text, args).unwrap();
            back.extend(texts(&a));
            match a["next_cursor"].as_str() {
                Some(c) => cursor = Some(c.to_string()),
                None => break,
            }
        }
        let forward = texts(&run(&text, json!({"order": "oldest", "limit": 500, "max_bytes": MAX_BYTES, "text": "job=1499"})).unwrap());
        back.reverse();
        assert_eq!(back, forward);
        assert_eq!(forward.len(), 111, "job=1499, 14990..14999, 149900..149999");
    }

    #[test]
    fn a_call_reads_no_more_than_its_cap() {
        let text = log(300_000);
        let len = text.len() as u64;
        let cap = len / 3;
        assert!(cap > WINDOW, "{cap}");
        for (order, args) in [("newest", json!({"text": "nothing like this"})), ("oldest", json!({"text": "nothing like this", "order": "oldest"}))] {
            let mut read = 0;
            let mut cursor: Option<String> = None;
            let mut calls = 0;
            loop {
                let mut args = args.clone();
                if let Some(c) = &cursor {
                    args["cursor"] = json!(c);
                }
                let mut q = Query::with_cap("big.log", src(&text), rules(), &args, cap).unwrap();
                while q.step(STEP) {}
                let a = q.answer().unwrap();
                let (from, to) = (a["read"]["from"].as_u64().unwrap(), a["read"]["to"].as_u64().unwrap());
                assert!(to - from <= cap + WINDOW, "{order}: {from}..{to}");
                read += to - from;
                calls += 1;
                assert_eq!(a["returned"], 0);
                match a["next_cursor"].as_str() {
                    Some(c) => {
                        assert_eq!(a["truncated"], "scan");
                        cursor = Some(c.to_string());
                    }
                    None => break,
                }
            }
            assert_eq!(read, len, "{order}: all of it, over the calls");
            assert!((3..=4).contains(&calls), "{order}: {calls}");
        }
    }

    #[test]
    fn a_summary_counts_as_the_viewer_does() {
        let text = log(500);
        let a = Job::new(SUMMARY, "app.log", src(&text), rules(), &json!({"buckets": 5})).unwrap().finish(STEP).unwrap();
        assert_eq!((a["entries"].as_u64(), a["lines"].as_u64()), (Some(500), Some(700)));
        assert_eq!((a["levels"]["error"].as_u64(), a["levels"]["warn"].as_u64(), a["levels"]["info"].as_u64()), (Some(100), Some(100), Some(200)));
        assert_eq!((a["first_time"].as_str(), a["last_time"].as_str()), (Some("2024-03-01 12:00:00"), Some("2024-03-01 12:08:19")));
        let spans = a["over_time"].as_array().unwrap();
        assert!(spans.len() <= 5 && !spans.is_empty());
        assert_eq!(spans.iter().map(|s| s["entries"].as_u64().unwrap()).sum::<u64>(), 500);
        assert_eq!(spans.iter().map(|s| s["error"].as_u64().unwrap()).sum::<u64>(), 100);
        assert_eq!(a["formats"][0]["format"], "text with a time and a level");
        assert_eq!(a["read"]["whole"], true);

        let json_lines = Job::new(SUMMARY, "a.jsonl", src(include_str!("samples/app.jsonl")), rules(), &json!({})).unwrap().finish(STEP).unwrap();
        assert_eq!(json_lines["formats"][0]["format"], "JSON lines", "{json_lines}");
        let logfmt = Job::new(SUMMARY, "a.log", src(include_str!("samples/logfmt.log")), rules(), &json!({})).unwrap().finish(STEP).unwrap();
        assert_eq!(logfmt["formats"][0]["format"], "logfmt");
        let syslog = Job::new(SUMMARY, "s.log", src(include_str!("samples/syslog.log")), rules(), &json!({})).unwrap().finish(STEP).unwrap();
        assert!(syslog["formats"].as_array().unwrap().iter().any(|f| f["format"] == "syslog"), "{syslog}");
        let export = Job::new(SUMMARY, "j.export", src(include_str!("samples/journal.export")), rules(), &json!({})).unwrap().finish(STEP).unwrap();
        assert_eq!((export["formats"][0]["format"].as_str(), export["entries"].as_u64()), (Some("journalctl export"), Some(2)));

        let big = log(20_000);
        let mut s = Summary::with_cap("big.log", src(&big), rules(), &json!({}), 100_000).unwrap();
        while s.step(STEP) {}
        let a = s.answer().unwrap();
        assert_eq!(a["read"]["whole"], false);
        assert!(a["note"].as_str().unwrap().contains("newest"));
        assert!(a["entries"].as_u64().unwrap() < 20_000 && a["last_time"].as_str() == Some("2024-03-01 12:33:19"), "{a}");
    }

    #[test]
    fn a_journal_exports_entries_are_summed_up() {
        let a = run(include_str!("samples/journal.export"), json!({"order": "oldest"})).unwrap();
        assert_eq!(a["returned"], 2);
        assert!(!a["entries"][0]["text"].as_str().unwrap().starts_with("__CURSOR"), "{a}");
    }
}
