//! `go test -json`: one stream on stdout, an event a line.
//!
//! ```text
//! {"Action":"run","Package":"example.com/orbit","Test":"TestTable/negative_one"}
//! {"Action":"output","Package":"example.com/orbit","Test":"TestTable/negative_one","Output":"    /p/orbit_test.go:37: got -2, want -3\n"}
//! {"Action":"fail","Package":"example.com/orbit","Test":"TestTable/negative_one","Elapsed":0}
//! ```
//!
//! Packages run side by side and their events interleave, so everything
//! is kept by `(Package, Test)`. A test's output lines come as they
//! happen, each with its test's name (Go 1.20 and later also name the
//! right one while `t.Parallel` tests are paused and continued, so
//! `pause` and `cont` say nothing the output doesn't), and its result
//! when it ends, with what it printed held to make a message, a place
//! (the last `file_test.go:37:` line, or a panic's first frame in the
//! project) and a text.
//!
//! What isn't a test's result:
//!
//! - A package's `fail` with `FailedBuild`, and the `build-output` lines
//!   of that import path before it (Go 1.24): `Broken`, the first error
//!   and its place. Before 1.24 the errors are on stderr and the
//!   package's `fail` is a plain one whose output says `[build failed]`
//!   (or `[setup failed]`): held until the step ends, when stderr is
//!   whole, and read from there.
//! - A package's `fail` while a test hasn't ended (`os.Exit` in a test, a
//!   timeout's panic, a crash): `Crashed`, with the panic if one was
//!   printed. After a test's panic Go ends the binary: its later tests
//!   never ran, so `Crashed` too.
//! - A package's panic before any test (a `TestMain` or an `init`): its
//!   lines have no test, then `fail`: `Crashed`.

use std::collections::{HashMap, VecDeque};

use fenix_lang_kit::serde_json;
use fenix_lang_kit::tests::{At, Ended, Line, Outcome, Reader, Report, StepEnd};
use serde::Deserialize;

use super::layout::{key, package_group};
use super::read::{case, file_line, first_frame, frame_package, is_marker, panic_message, unscanned, Context};

/// One line of `go test -json`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Event {
    action: String,
    package: Option<String>,
    test: Option<String>,
    output: Option<String>,
    elapsed: Option<f64>,
    failed_build: Option<String>,
    import_path: Option<String>,
}

/// The lines of a test, kept within bounds: a test that prints a million
/// lines keeps its first [`HEAD`] and last [`TAIL`] (a panic and its
/// trace are at the end, what a test says first is at the start).
#[derive(Debug, Default)]
struct Lines {
    head: Vec<String>,
    tail: VecDeque<String>,
    left_out: usize,
}

const HEAD: usize = 100;
const TAIL: usize = 900;

impl Lines {
    fn push(&mut self, line: String) {
        if self.head.len() < HEAD {
            self.head.push(line);
            return;
        }
        if self.tail.len() == TAIL {
            self.tail.pop_front();
            self.left_out += 1;
        }
        self.tail.push_back(line);
    }

    fn into_vec(self) -> Vec<String> {
        let mut all = self.head;
        if self.left_out > 0 {
            all.push(format!("... {} lines left out ...", self.left_out));
        }
        all.extend(self.tail);
        all
    }

    /// What `panic_message` and the like look in: all of it, in order.
    fn iter(&self) -> impl Iterator<Item = &String> {
        self.head.iter().chain(self.tail.iter())
    }
}

/// A test this step has begun.
#[derive(Debug, Default)]
struct Test {
    /// What it printed, without Go's own `=== RUN` and `--- FAIL` lines.
    lines: Lines,
    /// A line not yet ended by its newline.
    partial: String,
    /// One of its subtests ran.
    children: bool,
}

#[derive(Debug, Default)]
struct Pkg {
    /// Tests begun and not ended, in the order they began.
    running: Vec<String>,
    /// What it printed outside any test.
    outputs: Vec<String>,
    partial: String,
    /// A test that panicked: its name and message.
    panicked: Option<(String, String)>,
    saw_tests: bool,
}

pub struct GoTestReader {
    ctx: Context,
    tests: HashMap<(String, String), Test>,
    packages: HashMap<String, Pkg>,
    /// A package that didn't build, by import path: the compiler's lines.
    builds: HashMap<String, Vec<String>>,
    /// Packages whose build failed the way Go before 1.24 says it (their
    /// errors are on stderr): told at the end.
    deferred: Vec<String>,
    /// What stderr, and stdout lines that aren't JSON, said.
    noise: Vec<String>,
    /// Package-level events read: none means Go didn't get to run.
    events: usize,
}

/// `Output`'s lines: `buf` and what's in `chunk`, whole lines only; the
/// rest stays in `buf`.
fn lines_of(buf: &mut String, chunk: &str) -> Vec<String> {
    buf.push_str(chunk);
    let mut out = Vec::new();
    while let Some(at) = buf.find('\n') {
        out.push(buf[..at].trim_end_matches('\r').to_string());
        buf.drain(..=at);
    }
    // A line that never ends isn't kept whole: what's past 64 KiB is a line.
    if buf.len() > LINE_MAX {
        let mut cut = LINE_MAX;
        while !buf.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(buf[..cut].to_string());
        buf.drain(..cut);
    }
    out
}

const LINE_MAX: usize = 64 * 1024;

/// The compiler's lines kept for a package: the first errors are the ones said.
const BUILD_LINES: usize = 200;

/// What a package printed outside its tests, kept to read a panic or a
/// failed build from.
const PACKAGE_LINES: usize = 500;

/// A line is Go's summary of a package, not a test's or the program's.
fn is_summary(line: &str) -> bool {
    line == "PASS" || line == "FAIL" || line.starts_with("FAIL\t") || line.starts_with("ok  \t") || line.starts_with("?   \t") || line.starts_with("ok \t")
}

/// An import path as `build-output` and `FailedBuild` say it: `example.com/orbit/geo
/// [example.com/orbit/geo.test]` is `example.com/orbit/geo`.
fn base(import_path: &str) -> &str {
    import_path.split(" [").next().unwrap_or(import_path)
}

impl GoTestReader {
    pub fn new(ctx: Context) -> GoTestReader {
        GoTestReader { ctx, tests: HashMap::new(), packages: HashMap::new(), builds: HashMap::new(), deferred: Vec::new(), noise: Vec::new(), events: 0 }
    }

    /// The groups a `Broken` or `Crashed` of `package` names: its own when
    /// a scan gave it one, else the whole step (any other id would fail
    /// the step in Fenix).
    fn under(&self, package: &str) -> Option<Vec<String>> {
        self.ctx.index.borrow().knows(package).then(|| vec![package_group(package).label])
    }

    fn package_dir(&self, package: &str) -> Option<String> {
        let mut index = self.ctx.index.borrow_mut();
        index.package(package).map(|p| p.dir.clone()).or_else(|| index.modules.package_dir(package))
    }

    fn place(&self, file: &str, package: &str) -> String {
        let dir = self.package_dir(package);
        self.ctx.index.borrow_mut().place(file, dir.as_deref(), &self.ctx.cwd)
    }

    fn event(&mut self, e: Event, out: &mut Vec<Report>) {
        let Some(package) = e.package.clone() else {
            if e.action == "build-output" {
                if let (Some(path), Some(text)) = (&e.import_path, &e.output) {
                    let lines = self.builds.entry(base(path).to_string()).or_default();
                    if lines.len() < BUILD_LINES {
                        lines.extend(text.lines().map(|l| l.trim_end().to_string()));
                    }
                }
            }
            return;
        };
        match (e.action.as_str(), e.test.as_deref()) {
            ("start", None) => {
                self.packages.entry(package).or_default();
            }
            ("run", Some(name)) => self.run(&package, name, out),
            ("output", Some(name)) => self.test_output(&package, name, e.output.as_deref().unwrap_or(""), out),
            ("output", None) => self.package_output(&package, e.output.as_deref().unwrap_or(""), out),
            ("pass" | "fail" | "skip", Some(name)) => self.ended(&package, name, &e.action, e.elapsed, out),
            ("pass" | "fail" | "skip", None) => {
                self.events += 1;
                if e.action == "fail" {
                    self.package_failed(&package, e.failed_build.as_deref(), out);
                }
                if e.failed_build.is_none() {
                    self.not_built(&package, out);
                }
            }
            // `pause`, `cont`, `bench`, `attr`, and what Go adds.
            _ => {}
        }
    }

    /// A test begins: its `Found` when no scan saw it or it's a subtest,
    /// then `Started`.
    fn run(&mut self, package: &str, name: &str, out: &mut Vec<Report>) {
        self.events += 1;
        let k = (package.to_string(), name.to_string());
        if self.tests.contains_key(&k) {
            return;
        }
        self.tests.insert(k, Test::default());
        let p = self.packages.entry(package.to_string()).or_default();
        p.saw_tests = true;
        p.running.push(name.to_string());
        if let Some(first) = name.find('/') {
            // The nearest ancestor that ran is the parent; with none, the
            // top-level test, and `a/b` is one name.
            let mut parent = &name[..first];
            for (at, _) in name.rmatch_indices('/') {
                if self.tests.contains_key(&(package.to_string(), name[..at].to_string())) {
                    parent = &name[..at];
                    break;
                }
            }
            for (at, _) in name.match_indices('/') {
                if let Some(t) = self.tests.get_mut(&(package.to_string(), name[..at].to_string())) {
                    t.children = true;
                }
            }
            let top = self.ctx.index.borrow().test(package, &name[..first]).map(|k| (k.file.to_string(), k.line));
            out.push(Report::Found(case(package, name, parent, top.as_ref().map(|(f, l)| (f.as_str(), *l)))));
        } else if self.ctx.index.borrow().test(package, name).is_none() {
            out.push(Report::Found(unscanned(package, name)));
        }
        out.push(Report::Started(key(package, name).into()));
    }

    fn test_output(&mut self, package: &str, name: &str, chunk: &str, out: &mut Vec<Report>) {
        let t = self.tests.entry((package.to_string(), name.to_string())).or_default();
        for line in lines_of(&mut t.partial, chunk) {
            if is_marker(&line) {
                continue;
            }
            t.lines.push(line.clone());
            out.push(Report::Output { key: Some(key(package, name).into()), text: line });
        }
    }

    /// What a package printed outside any test (a `TestMain`'s, a panic
    /// before a test ran) is the run's output, apart from Go's own summary
    /// lines.
    fn package_output(&mut self, package: &str, chunk: &str, out: &mut Vec<Report>) {
        let p = self.packages.entry(package.to_string()).or_default();
        let lines = lines_of(&mut p.partial, chunk);
        for l in &lines {
            if !is_summary(l) && !l.trim().is_empty() && !l.contains("[build failed]") && !l.contains("[setup failed]") {
                out.push(Report::Output { key: None, text: l.clone() });
            }
        }
        if p.outputs.len() < PACKAGE_LINES {
            p.outputs.extend(lines);
        }
    }

    /// A test ended: its result, whole.
    fn ended(&mut self, package: &str, name: &str, action: &str, elapsed: Option<f64>, out: &mut Vec<Report>) {
        self.events += 1;
        let k = key(package, name);
        let t = self.tests.entry((package.to_string(), name.to_string())).or_default();
        if !t.partial.is_empty() {
            let rest = std::mem::take(&mut t.partial);
            t.lines.push(rest);
        }
        let (lines, children) = (std::mem::take(&mut t.lines).into_vec(), t.children);
        let p = self.packages.entry(package.to_string()).or_default();
        p.running.retain(|r| r != name);
        let outcome = match action {
            "pass" => Outcome::Passed,
            "skip" => Outcome::Skipped,
            _ => Outcome::Failed,
        };
        let mut e = Ended::new(k.clone(), outcome);
        if let Some(s) = elapsed {
            e = e.ms(s * 1000.0);
        }
        let mut last_place: Option<(String, u32, Option<u32>)> = None;
        let mut last_text: Option<String> = None;
        for l in &lines {
            if let Some((file, line, col, text)) = file_line(l) {
                last_place = Some((file.to_string(), line, col));
                last_text = Some(text.to_string());
            }
        }
        match outcome {
            Outcome::Skipped => {
                if let Some(text) = last_text.filter(|t| !t.is_empty()) {
                    e = e.message(text);
                }
            }
            Outcome::Failed => {
                if let Some(message) = panic_message(&lines) {
                    if let Some(at) = self.panic_place(&lines) {
                        e = e.at(at);
                    }
                    e = e.message(message.clone());
                    self.packages.entry(package.to_string()).or_default().panicked = Some((name.to_string(), message));
                } else if let Some(text) = last_text.filter(|t| !t.is_empty()) {
                    e = e.message(text);
                    if let Some((file, line, col)) = last_place {
                        e = e.at(At::new(self.place(&file, package), line, col));
                    }
                } else if let Some(message) = got_want(&lines) {
                    e = e.message(message);
                } else if let Some(first) = lines.iter().map(|l| l.trim()).find(|l| !l.is_empty()) {
                    e = e.message(first);
                } else if children {
                    e = e.message("a subtest failed");
                }
                let text = lines.join("\n");
                let text = text.trim_matches('\n');
                if !text.is_empty() {
                    e = e.text(text);
                }
            }
            _ => {}
        }
        out.push(Report::Ended(e));
    }

    /// A panic's first frame in the project's packages.
    fn panic_place(&self, lines: &[String]) -> Option<At> {
        let known = |function: &str| -> Option<String> {
            let package = frame_package(function)?;
            let mut index = self.ctx.index.borrow_mut();
            let is_project = index.knows(package) || index.modules.package_dir(package).is_some();
            is_project.then(|| package.to_string())
        };
        let (package, file, line) = first_frame(lines, &known)?;
        let place = self.ctx.index.borrow_mut().place_in_package(file, &package);
        Some(At::new(place, line, None))
    }

    /// A package ended, and Go said nothing of a test whose file it doesn't
    /// build here (a `//go:build integration` one): skipped, for the
    /// reason, where the step runs it. Without, Fenix would error it as
    /// a test the run gave no result for.
    fn not_built(&mut self, package: &str, out: &mut Vec<Report>) {
        let excluded = self.ctx.index.borrow().excluded_in(package);
        for (name, why) in excluded {
            let file = self.ctx.index.borrow().test(package, &name).map(|k| k.file.to_string()).unwrap_or_default();
            if !self.tests.contains_key(&(package.to_string(), name.clone())) && self.ctx.runs(package, &name, &file) {
                out.push(Report::Ended(Ended::new(key(package, &name), Outcome::Skipped).message(&*why)));
            }
        }
    }

    /// A package ended `fail`.
    fn package_failed(&mut self, package: &str, failed_build: Option<&str>, out: &mut Vec<Report>) {
        let under = self.under(package);
        if let Some(built) = failed_build {
            let lines = self.builds.get(base(built)).cloned().unwrap_or_default();
            let (message, at) = self.build_error(package, &lines);
            out.push(Report::Broken { under, message, at });
            return;
        }
        let p = self.packages.entry(package.to_string()).or_default();
        let outputs = std::mem::take(&mut p.outputs);
        let running = std::mem::take(&mut p.running);
        let (panicked, saw_tests) = (p.panicked.take(), p.saw_tests);
        if outputs.iter().any(|l| l.contains("[build failed]") || l.contains("[setup failed]")) {
            self.deferred.push(package.to_string());
            return;
        }
        if !running.is_empty() {
            // The binary ended with tests running: a timeout's panic is on
            // the test it caught, an `os.Exit` or a crash says nothing.
            let why = running
                .iter()
                .find_map(|name| self.tests.get(&(package.to_string(), name.clone())).and_then(|t| panic_line(t.lines.iter())))
                .map(|m| format!("{m} (running: {})", running.join(", ")));
            let message = why.unwrap_or_else(|| format!("its test binary ended while {} was running (os.Exit, or a crash)", running.join(", ")));
            out.push(Report::Crashed { under, message });
        } else if let Some((name, message)) = panicked {
            out.push(Report::Crashed { under, message: format!("{name} panicked, which ends the test binary: {message}") });
        } else if let Some(panic) = panic_message(&outputs) {
            out.push(Report::Crashed { under, message: format!("{panic} (before any test ran)") });
        } else if !saw_tests {
            let last = outputs.iter().rev().find(|l| !l.trim().is_empty() && !is_summary(l)).map(|l| l.trim().to_string());
            let message = match last {
                Some(l) => format!("its test binary failed before running a test: {l}"),
                None => "its test binary failed before running a test".to_string(),
            };
            out.push(Report::Crashed { under, message });
        }
    }

    /// The first compiler error of `lines` as `<package> didn't build:
    /// geo/geo_test.go:6 cannot use ...`, and where it is.
    fn build_error(&self, package: &str, lines: &[String]) -> (String, Option<At>) {
        let mut first_other = None;
        for l in lines {
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if let Some((file, line, col, text)) = file_line(l) {
                let place = self.place(file, package);
                return (format!("{package} didn't build: {place}:{line} {text}"), Some(At::new(place, line, col)));
            }
            first_other.get_or_insert(l.trim());
        }
        match first_other {
            Some(l) => (l.to_string(), None),
            None => (format!("{package} didn't build"), None),
        }
    }
}

/// A panic's message among a test's lines, in what's kept of them.
fn panic_line<'a>(lines: impl Iterator<Item = &'a String>) -> Option<String> {
    let kept: Vec<String> = lines.cloned().collect();
    panic_message(&kept)
}

/// An example's `got:` and `want:` blocks as one line.
fn got_want(lines: &[String]) -> Option<String> {
    let got = lines.iter().position(|l| l.trim() == "got:")?;
    let want = lines.iter().position(|l| l.trim() == "want:")?;
    if want <= got {
        return None;
    }
    let join = |a: &[String]| a.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    Some(format!("got: {}, want: {}", join(&lines[got + 1..want]), join(&lines[want + 1..])))
}

impl Reader for GoTestReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        if !line.stderr && line.text.starts_with('{') {
            if let Ok(e) = serde_json::from_str::<Event>(line.text) {
                self.event(e, out);
                return;
            }
        }
        if self.noise.len() < 200 && !line.text.trim().is_empty() {
            self.noise.push(line.text.to_string());
        }
    }

    fn end(&mut self, end: &StepEnd, out: &mut Vec<Report>) {
        // Before Go 1.24: a package that didn't build, said on stderr.
        for package in std::mem::take(&mut self.deferred) {
            let mut block: Vec<String> = Vec::new();
            let mut inside = false;
            for l in &self.noise {
                if let Some(header) = l.strip_prefix("# ") {
                    inside = base(header) == package;
                    continue;
                }
                if inside {
                    block.push(l.clone());
                }
            }
            if block.is_empty() {
                block = self.noise.iter().filter(|l| !l.starts_with("# ")).cloned().collect();
            }
            let under = self.under(&package);
            let (message, at) = self.build_error(&package, &block);
            out.push(Report::Broken { under, message, at });
        }
        // Go never got to run: its own error on stderr (no module, a
        // package that isn't there, a flag it doesn't know).
        if self.events == 0 && matches!(end, StepEnd::Exited(Some(c)) if *c != 0) {
            let noise: Vec<&String> = self.noise.iter().filter(|l| !l.starts_with("go: downloading ") && !l.starts_with("go: finding ") && !l.starts_with("go: extracting ") && !l.starts_with('#')).collect();
            if let Some(first) = noise.first() {
                let (message, at) = match file_line(first) {
                    Some((file, line, col, text)) => {
                        let place = self.ctx.index.borrow_mut().place(file, None, &self.ctx.cwd);
                        (format!("{place}:{line} {text}"), Some(At::new(place, line, col)))
                    }
                    None => (first.trim().to_string(), None),
                };
                out.push(Report::Broken { under: None, message, at });
            }
        }
    }
}
