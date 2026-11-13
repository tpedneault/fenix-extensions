//! `cargo test`'s output, read a line at a time: cargo's own JSON lines
//! on stdout (`--message-format=json`: each binary built, each compiler
//! error, whether the build finished), libtest's human lines on stdout
//! (`test NAME ... ok`, the failures' sections at each binary's end, its
//! `test result:`), and cargo's headers on stderr (`Running unittests
//! src/lib.rs (target/debug/deps/orbit-…)`, `Doc-tests orbit`), which say
//! which binary is which.
//!
//! **Which binary a line is from.** The binary's name is on stderr, its
//! tests' lines on stdout, and Fenix keeps each stream's order but not
//! the order between them (Windows gives them per tick). So each binary's
//! stdout is a *segment* (`running N tests` to `test result:`; in a
//! listing, its `name: test` lines to `N tests, M benchmarks`), and the
//! k-th segment is the k-th header's -- except a header whose binary
//! printed no segment: a `harness = false` target, which prints what it
//! likes, or says `thread 'main' panicked` on stderr before its header's
//! next. So a segment is paired with a header when the header is
//! *settled* (the next header has come, or the step ended: everything
//! stderr says of it is in), or sooner when the segment's first test is
//! one the provider's scans or listings know in that header's target --
//! or in a later header's, which tells that the headers between printed
//! nothing. A segment whose header hasn't come is held. Each stream's own
//! order is all it needs, so stdout and stderr shuffled per tick end the
//! same (the tests interleave them so).
//!
//! **One whole result per test.** libtest says `FAILED` as a test ends
//! and its output and panic only at its binary's end, under `failures:`;
//! a failed test is held until its `---- name stdout ----` section has
//! come, then reported once, with the panic's place as `at`, the line
//! after it as its message, the section as its text and what it printed
//! as its output. One whose section never comes (its binary crashed) is
//! reported at its segment's end without them.
//!
//! **Didn't build, crashed.** A compiler error is never a test's failure:
//! when `build-finished` says it failed, each target with an error is
//! `Broken` with its first, and the rest of the step (cargo ran nothing)
//! `Broken` with the same. `process didn't exit successfully: ...
//! (signal: 6, SIGABRT ...)` is `Crashed` under its binary's target
//! alone: with `--no-fail-fast` cargo goes on to the next binary.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::tests::{At, Ended, Found, Line, Outcome, Reader, Report, StepEnd};

use super::layout::{doc_place, TargetKey};
use super::read::{self, Ctx, Purpose};

#[derive(Debug)]
enum Head {
    /// `Running <desc> (<exe>)`: the binary's file name, and what cargo
    /// calls it (`unittests src/lib.rs`, `tests/it.rs`).
    Binary { file: String, desc: String },
    /// `Doc-tests <crate>`.
    Doc(String),
}

#[derive(Debug)]
struct Header {
    head: Head,
    /// It printed no segment: a `harness = false` binary that panicked.
    no_segment: bool,
}

#[derive(Debug, Default)]
struct Segment {
    /// Its lines not read yet.
    lines: Vec<String>,
    closed: bool,
    header: Option<usize>,
    /// Its first test's name, which says whose it is.
    first: Option<String>,
}

/// The front segment's reading: its target, its failures held until their
/// sections come, the section being read.
struct Reading {
    target: TargetKey,
    held: Vec<(String, Ended)>,
    section: Option<(String, Vec<String>)>,
}

pub struct CargoReader {
    ctx: Rc<Ctx>,
    /// Each test binary by its file's name: its target.
    binaries: HashMap<String, TargetKey>,
    /// Each library's crate name: its package (`Doc-tests orbit_core`).
    libraries: HashMap<String, String>,
    built: bool,
    build_failed: bool,
    /// Each target that didn't build, with its first error.
    errors: Vec<(TargetKey, String, Option<At>)>,
    /// cargo's first `error:` on stderr, for a build that failed with no
    /// compiler message (a build script, a manifest).
    cargo_error: Option<String>,
    headers: Vec<Header>,
    /// A `thread 'main' panicked at <place>:` whose message is the next
    /// stderr line, and its header.
    main_panic: Option<(usize, String)>,
    /// Crashes waiting for their binary's target.
    crashes: Vec<(usize, String)>,
    segments: VecDeque<Segment>,
    next_header: usize,
    reading: Option<Reading>,
    ended: bool,
}

/// A path's file name.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `running 9 tests`, `running 1 test`.
fn is_running(line: &str) -> bool {
    line.strip_prefix("running ").is_some_and(|r| r.ends_with(" tests") || r.ends_with(" test"))
}

/// `9 tests, 0 benchmarks`: a listing's binary ended.
fn is_count(line: &str) -> bool {
    let mut w = line.split(' ');
    matches!((w.next().map(|n| n.parse::<u64>().is_ok()), w.next(), w.next().map(|n| n.parse::<u64>().is_ok()), w.next()), (Some(true), Some("tests," | "test,"), Some(true), Some("benchmarks" | "benchmark")))
}

/// `test NAME ... ok`: the name and what follows ` ... `.
fn result_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("test ")?;
    let (name, status) = rest.rsplit_once(" ... ")?;
    Some((name.strip_suffix(" - should panic").unwrap_or(name), status))
}

/// A listing's `NAME: test`.
fn listed(line: &str) -> Option<&str> {
    line.strip_suffix(": test")
}

impl CargoReader {
    pub fn new(ctx: Rc<Ctx>) -> CargoReader {
        CargoReader {
            ctx,
            binaries: HashMap::new(),
            libraries: HashMap::new(),
            built: false,
            build_failed: false,
            errors: Vec::new(),
            cargo_error: None,
            headers: Vec::new(),
            main_panic: None,
            crashes: Vec::new(),
            segments: VecDeque::new(),
            next_header: 0,
            reading: None,
            ended: false,
        }
    }

    /// A compiler-artifact's or compiler-message's package and target.
    fn target_of(&self, v: &Value) -> Option<TargetKey> {
        let t = &v["target"];
        let kinds: Vec<String> = t["kind"].as_array()?.iter().filter_map(|k| k.as_str().map(str::to_string)).collect();
        let name = t["name"].as_str()?;
        let manifest = v["manifest_path"].as_str().unwrap_or("").replace('\\', "/");
        let dir = manifest.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let package = match self.ctx.layout.packages.iter().find(|p| !p.abs.is_empty() && p.abs == dir) {
            Some(p) => p.name.clone(),
            // `path+file:///x/orbit#0.1.0` (the folder's name is the
            // package's) or `...#orbit@0.1.0`.
            None => {
                let id = v["package_id"].as_str()?;
                let (url, version) = id.rsplit_once('#')?;
                match version.split_once('@') {
                    Some((name, _)) => name.to_string(),
                    None => file_name(url).to_string(),
                }
            }
        };
        TargetKey::of_kind(&package, &kinds, name)
    }

    fn json(&mut self, text: &str, out: &mut Vec<Report>) {
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        match v["reason"].as_str() {
            Some("compiler-artifact") => {
                let Some(target) = self.target_of(&v) else { return };
                if target.target == "lib" {
                    if let Some(name) = v["target"]["name"].as_str() {
                        self.libraries.insert(name.to_string(), target.package.clone());
                    }
                }
                // A test harness's binary; not a `[[bin]]` built for an
                // integration test to run.
                let (Some(exe), true) = (v["executable"].as_str(), v["profile"]["test"].as_bool() == Some(true)) else { return };
                if self.ctx.purpose == Purpose::Build {
                    out.push(Report::Keep { name: format!("exe {}", target.id()), value: exe.to_string() });
                }
                self.binaries.insert(file_name(exe).to_string(), target);
            }
            Some("compiler-message") if v["message"]["level"].as_str() == Some("error") => {
                let Some(target) = self.target_of(&v) else { return };
                if let Some(rendered) = v["message"]["rendered"].as_str() {
                    out.push(Report::Output { key: None, text: rendered.trim_end().to_string() });
                }
                if self.errors.iter().any(|(t, _, _)| *t == target) {
                    return;
                }
                let message = v["message"]["message"].as_str().unwrap_or("error").to_string();
                let spans = v["message"]["spans"].as_array();
                let span = spans.and_then(|s| s.iter().find(|s| s["is_primary"].as_bool() == Some(true)).or_else(|| s.first()));
                let at = span.and_then(|s| Some(At::new(self.ctx.layout.place(s["file_name"].as_str()?), s["line_start"].as_u64()? as u32, s["column_start"].as_u64().map(|c| c as u32))));
                self.errors.push((target, message, at));
            }
            Some("build-finished") => {
                self.built = true;
                self.build_failed = v["success"].as_bool() == Some(false);
            }
            _ => {}
        }
    }

    fn stdout(&mut self, text: &str) {
        let listing = self.ctx.purpose == Purpose::Listing;
        let open = self.segments.back().is_some_and(|s| !s.closed);
        if listing {
            if is_count(text) {
                match self.segments.back_mut().filter(|s| !s.closed) {
                    Some(s) => s.closed = true,
                    None => self.segments.push_back(Segment { closed: true, ..Segment::default() }),
                }
                return;
            }
            let Some(name) = listed(text) else { return };
            if !open {
                self.segments.push_back(Segment::default());
            }
            let s = self.segments.back_mut().expect("open");
            s.first.get_or_insert_with(|| name.to_string());
            s.lines.push(text.to_string());
            return;
        }
        if is_running(text) {
            // A segment still open ended without its `test result:`: its
            // binary died.
            if let Some(s) = self.segments.back_mut() {
                s.closed = true;
            }
            self.segments.push_back(Segment::default());
            return;
        }
        let Some(s) = self.segments.back_mut().filter(|s| !s.closed) else { return };
        if s.first.is_none() {
            if let Some((name, _)) = result_line(text) {
                s.first = Some(name.to_string());
            }
        }
        s.closed = text.starts_with("test result:");
        s.lines.push(text.to_string());
    }

    fn stderr(&mut self, text: &str) {
        let t = text.trim();
        if let Some((header, place)) = self.main_panic.take() {
            self.crashes.push((header, format!("its test binary panicked outside any test: {t} ({place})")));
            return;
        }
        if let Some(rest) = t.strip_prefix("Running ") {
            if let Some((desc, exe)) = rest.rsplit_once(" (") {
                let exe = exe.strip_suffix(')').unwrap_or(exe);
                self.headers.push(Header { head: Head::Binary { file: file_name(exe).to_string(), desc: desc.to_string() }, no_segment: false });
            }
            return;
        }
        if let Some(name) = t.strip_prefix("Doc-tests ") {
            self.headers.push(Header { head: Head::Doc(name.to_string()), no_segment: false });
            return;
        }
        if let Some(("main", place)) = read::panic_line(t) {
            if let Some(last) = self.headers.len().checked_sub(1) {
                self.headers[last].no_segment = true;
                self.main_panic = Some((last, place.to_string()));
            }
            return;
        }
        if let Some(rest) = t.strip_prefix("process didn't exit successfully: `") {
            let Some(message) = read::crash_message(t) else { return };
            let exe = rest.split(['`', ' ']).next().unwrap_or("");
            let file = file_name(exe);
            let at = self.headers.iter().rposition(|h| matches!(&h.head, Head::Binary { file: f, .. } if f == file)).or(self.headers.len().checked_sub(1));
            if let Some(h) = at {
                self.crashes.push((h, message));
            }
            return;
        }
        if t.starts_with("error") && !t.starts_with("error: test failed") && !t.contains("targets failed") && self.cargo_error.is_none() {
            self.cargo_error = Some(t.to_string());
        }
    }

    /// Header `h`'s target, once cargo has said its binaries (or the
    /// step's ended): by its binary's file, else guessed from what cargo
    /// calls it.
    fn header_target(&self, h: usize) -> Option<TargetKey> {
        match &self.headers[h].head {
            Head::Doc(name) => self.libraries.get(name).map(|p| TargetKey::new(p, "doc")).or_else(|| self.ctx.layout.doc_target(name)).or_else(|| self.ctx.package.as_ref().map(|p| TargetKey::new(p, "doc"))),
            Head::Binary { file, desc } => {
                if let Some(t) = self.binaries.get(file) {
                    return Some(t.clone());
                }
                if !self.built && !self.ended {
                    return None;
                }
                self.guess(desc, file)
            }
        }
    }

    /// `unittests src/lib.rs`, `tests/it.rs`: a target, when cargo's JSON
    /// didn't say (the step's package, or the only one).
    fn guess(&self, desc: &str, file: &str) -> Option<TargetKey> {
        let package = self.ctx.package.clone().or_else(|| (self.ctx.layout.packages.len() == 1).then(|| self.ctx.layout.packages[0].name.clone()))?;
        let src = desc.strip_prefix("unittests ").unwrap_or(desc);
        if let Some(t) = self.ctx.layout.package(&package).and_then(|p| p.targets.iter().find(|t| t.src == src)) {
            return Some(t.key.clone());
        }
        let stem = file.rsplit_once('-').map(|(s, _)| s).unwrap_or(file);
        Some(match src {
            "src/lib.rs" => TargetKey::new(&package, "lib"),
            "src/main.rs" => TargetKey::new(&package, &format!("bin:{package}")),
            _ if src.starts_with("tests/") => TargetKey::new(&package, &format!("test:{stem}")),
            _ => TargetKey::new(&package, &format!("bin:{stem}")),
        })
    }

    /// Whether header `h`'s binary has test `name`, as the provider knows.
    fn knows(&self, h: usize, name: &str) -> bool {
        let doc = doc_place(name).is_some();
        match &self.headers[h].head {
            Head::Doc(_) => doc,
            Head::Binary { .. } => !doc && self.header_target(h).is_some_and(|t| self.ctx.index.borrow().knows(&t.id(), name)),
        }
    }

    /// The header the front segment is, once that can be told.
    fn pair(&mut self) -> Option<usize> {
        let s = self.segments.front()?;
        let open: Vec<usize> = (self.next_header..self.headers.len()).filter(|&h| !self.headers[h].no_segment).collect();
        if let Some(name) = &s.first {
            if let Some(&h) = open.iter().find(|&&h| self.knows(h, name)) {
                return Some(h);
            }
        } else if !s.closed && !self.ended {
            return None;
        }
        let &k = open.first()?;
        (k + 1 < self.headers.len() || self.ended).then_some(k)
    }

    /// Reads what can be read: each segment once its header is told, in
    /// order; then the crashes whose binaries are known.
    fn pump(&mut self, out: &mut Vec<Report>) {
        loop {
            if self.reading.is_none() {
                let Some(h) = self.pair() else { break };
                let Some(target) = self.header_target(h) else { break };
                self.next_header = h + 1;
                if let Some(s) = self.segments.front_mut() {
                    s.header = Some(h);
                }
                self.reading = Some(Reading { target, held: Vec::new(), section: None });
            }
            let Some(s) = self.segments.front_mut() else { break };
            let lines = std::mem::take(&mut s.lines);
            let closed = s.closed;
            for l in &lines {
                self.segment_line(l, out);
            }
            if !closed {
                break;
            }
            self.segment_ended(out);
            self.segments.pop_front();
        }
        let mut waiting = Vec::new();
        for (h, message) in std::mem::take(&mut self.crashes) {
            match self.header_target(h) {
                Some(t) => out.push(Report::Crashed { under: Some(vec![t.label()]), message }),
                None => waiting.push((h, message)),
            }
        }
        self.crashes = waiting;
    }

    fn segment_line(&mut self, text: &str, out: &mut Vec<Report>) {
        let ctx = self.ctx.clone();
        let Some(r) = self.reading.as_mut() else { return };
        if ctx.purpose == Purpose::Listing {
            if let Some(name) = listed(text) {
                ctx.index.borrow_mut().listed(&r.target.id(), name);
                let f = read::found(&ctx, &r.target, name).unwrap_or_else(|| Found::test(r.target.key(name), name.rsplit("::").next().unwrap_or(name)).under([r.target.group()]));
                out.push(Report::Found(f));
            }
            return;
        }
        if let Some((name, lines)) = &mut r.section {
            let ends = text.starts_with("---- ") && text.ends_with(" stdout ----");
            if !ends && text != "failures:" && !text.starts_with("test result:") {
                lines.push(text.to_string());
                return;
            }
            let (name, lines) = (std::mem::take(name), std::mem::take(lines));
            r.section = None;
            Self::section(&ctx, r, &name, &lines, out);
        }
        if let Some(name) = text.strip_prefix("---- ").and_then(|t| t.strip_suffix(" stdout ----")) {
            r.section = Some((name.to_string(), Vec::new()));
            return;
        }
        let Some((name, status)) = result_line(text) else { return };
        let key = r.target.key(name);
        if status.starts_with("bench:") {
            return;
        }
        read::found_report(&ctx, &r.target, name, out);
        match status {
            "ok" => out.push(Report::Ended(Ended::new(key, Outcome::Passed))),
            "FAILED" => r.held.push((name.to_string(), Ended::new(key, Outcome::Failed))),
            _ if status.starts_with("ignored") => {
                let why = status.strip_prefix("ignored, ").unwrap_or("");
                let e = Ended::new(key, Outcome::Skipped);
                out.push(Report::Ended(if why.is_empty() { e } else { e.message(why) }));
            }
            _ => {}
        }
    }

    /// A failure's section ended: its held result, whole.
    fn section(ctx: &Ctx, r: &mut Reading, name: &str, lines: &[String], out: &mut Vec<Report>) {
        let key = r.target.key(name);
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (e, printed) = read::failure(&key, &lines, &ctx.layout);
        r.held.retain(|(n, _)| n != name);
        read::ended(&key, printed, e, out);
    }

    /// A segment ended (`test result:`, or its binary died): a section
    /// still open, then the failures no section came for.
    fn segment_ended(&mut self, out: &mut Vec<Report>) {
        let ctx = self.ctx.clone();
        let Some(mut r) = self.reading.take() else { return };
        if let Some((name, lines)) = r.section.take() {
            Self::section(&ctx, &mut r, &name, &lines, out);
        }
        out.extend(r.held.drain(..).map(|(_, e)| Report::Ended(e)));
    }

    fn broken(&mut self, out: &mut Vec<Report>) {
        if !self.build_failed {
            return;
        }
        let mut first: Option<(String, Option<At>)> = None;
        for (target, message, at) in &self.errors {
            let label = target.label();
            let place = at.as_ref().map(|a| format!("{}:{} ", a.file, a.line)).unwrap_or_default();
            let message = format!("{label} didn't build: {place}{message}");
            first.get_or_insert_with(|| (message.clone(), at.clone()));
            out.push(Report::Broken { under: Some(vec![label]), message, at: at.clone() });
        }
        // cargo ran nothing: the rest of the step didn't run either.
        let (message, at) = first.unwrap_or_else(|| (format!("the tests didn't build: {}", self.cargo_error.clone().unwrap_or_else(|| "see the output".into())), None));
        out.push(Report::Broken { under: None, message, at });
    }
}

impl Reader for CargoReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        if line.stderr {
            self.stderr(line.text);
        } else if line.text.starts_with("{\"reason\":") {
            self.json(line.text, out);
        } else {
            self.stdout(line.text);
        }
        self.pump(out);
    }

    fn end(&mut self, _end: &StepEnd, out: &mut Vec<Report>) {
        self.ended = true;
        if let Some(s) = self.segments.back_mut() {
            s.closed = true;
        }
        self.pump(out);
        // Segments with no header left (stderr wasn't kept): read as the
        // step's package's library's, the best guess there is.
        let package = self.ctx.package.clone().or_else(|| self.ctx.layout.packages.first().map(|p| p.name.clone()));
        while let (false, Some(p)) = (self.segments.is_empty(), &package) {
            if self.reading.is_none() {
                self.reading = Some(Reading { target: TargetKey::new(p, "lib"), held: Vec::new(), section: None });
            }
            self.pump(out);
        }
        self.broken(out);
    }
}
