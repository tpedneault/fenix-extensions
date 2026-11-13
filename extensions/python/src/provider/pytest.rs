//! Reads pytest's output: the lines of `fenix_tests.py`, the plugin the
//! pack ships (`FENIX-TEST {json}`, one a line, each starting a line of its
//! own); every other line is pytest's own, which is read only for what it
//! says when the plugin couldn't.
//!
//! | `kind` | It reports |
//! |---|---|
//! | `test` | a test pytest collected that no scan has (or a case, under its template) as a `Found`, with its file and line |
//! | `start` | `Started`, and the test is the one running if the process then dies |
//! | `result` | one `Ended` for the test, when it's whole: the call's, or its setup's failure (errored) or its teardown's (errored) |
//! | `collect-error` | `Broken`: a file that didn't load; pytest then runs nothing |
//! | `done` | the session's exit status: no `done` means the process died |
//!
//! A test's phases are held until the next test starts or the step ends:
//! the plugin says nothing of a setup or a teardown that passed, so a
//! passed `call` is whole only then. Under `pytest-xdist` the workers'
//! reports interleave, and a teardown's failure may come after another
//! test began: a result already sent that wasn't a pass is kept to be
//! joined with the later phase and sent again (Fenix keeps the last).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use fenix_extension_api::serde_json::{self, Value};
use fenix_lang_kit::tests::{Ended, Line, Outcome, Reader, Report, StepEnd};
use serde::Deserialize;

use super::read::{found, Ctx, Purpose};

/// What starts each line of the plugin's.
pub const MARK: &str = "FENIX-TEST ";

/// A line of the plugin's, whatever its kind.
#[derive(Debug, Deserialize, Default)]
struct Message {
    #[serde(default)]
    kind: String,
    id: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    when: Option<String>,
    outcome: Option<String>,
    duration: Option<f64>,
    xfail: Option<Value>,
    message: Option<String>,
    at: Option<(String, u32)>,
    text: Option<String>,
    output: Option<std::collections::BTreeMap<String, String>>,
    status: Option<i64>,
}

/// The first line of `text`, trimmed: a message is one line.
fn first_line(text: &str) -> &str {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

/// `Skipped: slow` as "slow": pytest's own prefix off a skip's reason.
fn skip_reason(message: &str) -> &str {
    let m = first_line(message);
    m.strip_prefix("Skipped: ").or_else(|| m.strip_prefix("Skipped")).map(str::trim).unwrap_or(m)
}

/// xdist's `[gw0] linux -- Python 3.13.5 /path/python` and the blank line
/// after it, off a traceback's start.
fn without_worker(text: &str) -> &str {
    match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("[gw") && first.contains("] ") && first.contains(" -- Python ") => rest.trim_start_matches('\n'),
        _ => text,
    }
}

/// pytest's last line of a run: `3 failed, 6 passed, 1 skipped in 0.03s`,
/// with its colour and rules gone (`-v` puts `=====` round it).
pub fn is_summary(line: &str) -> bool {
    let line = line.trim().trim_matches('=').trim();
    let Some((counts, time)) = line.rsplit_once(" in ") else { return false };
    let time = time.split(" (").next().unwrap_or(time);
    time.strip_suffix('s').is_some_and(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.')) && !counts.is_empty()
}

pub struct PytestReader {
    ctx: Rc<Ctx>,
    /// The test whose phases are being held.
    pending: Option<(String, Ended)>,
    /// Results sent that weren't a pass or a skip, to be joined with a
    /// phase that comes late.
    sent: HashMap<String, Ended>,
    /// What's been found this step.
    announced: HashSet<String>,
    /// Tests started and not yet reported, in the order they started.
    running: BTreeSet<(u64, String)>,
    started: u64,
    /// The session's exit status, when the plugin said it.
    done: Option<i64>,
    /// A `collect-error` was read.
    broken: bool,
    no_pytest: bool,
    fatal: Option<String>,
    internal: Option<String>,
    /// An `ERROR: ...` line pytest prints for a usage error.
    usage: Option<String>,
    /// pytest's stderr: its last line, and the first that names a place
    /// (`ImportError while loading conftest ...`).
    last_error: Option<String>,
    loading: Option<String>,
    place: Option<(String, u32)>,
    /// The project's folder on the machine pytest ran on, learned from a
    /// result's full path to a test's own file.
    machine: Option<String>,
}

impl PytestReader {
    pub fn new(ctx: Rc<Ctx>) -> PytestReader {
        PytestReader {
            ctx,
            pending: None,
            sent: HashMap::new(),
            announced: HashSet::new(),
            running: BTreeSet::new(),
            started: 0,
            done: None,
            broken: false,
            no_pytest: false,
            fatal: None,
            internal: None,
            usage: None,
            last_error: None,
            loading: None,
            place: None,
            machine: None,
        }
    }

    fn json(&mut self, text: &str, out: &mut Vec<Report>) {
        let Ok(m) = serde_json::from_str::<Message>(text) else { return };
        match m.kind.as_str() {
            "test" => self.test(m, out),
            "start" => {
                let Some(id) = m.id else { return };
                self.flush(out);
                self.find(&id, None, out);
                self.started += 1;
                self.running.insert((self.started, id.clone()));
                out.push(Report::Started(self.ctx.key(&id).into()));
            }
            "result" => self.result(m, out),
            "collect-error" => self.collect_error(m, out),
            "done" => self.done = m.status,
            _ => {}
        }
    }

    /// What pytest names, found when no scan has it.
    fn find(&mut self, id: &str, at: Option<(&str, u32)>, out: &mut Vec<Report>) {
        let key = self.ctx.key(id);
        if self.ctx.purpose == Purpose::Run && self.ctx.index.borrow().was_listed(&key) {
            return;
        }
        for f in found(&self.ctx, id, at, &mut self.announced) {
            out.push(Report::Found(f));
        }
    }

    fn test(&mut self, m: Message, out: &mut Vec<Report>) {
        let Some(id) = m.id else { return };
        let at = m.file.as_deref().zip(m.line);
        self.find(&id, at, out);
        if self.ctx.purpose == Purpose::Listing {
            let key = self.ctx.key(&id);
            self.ctx.index.borrow_mut().listed(&key);
        }
    }

    /// The held result goes now: it's whole.
    fn flush(&mut self, out: &mut Vec<Report>) {
        let Some((key, mut ended)) = self.pending.take() else { return };
        // Phases' times added: a float's noise off.
        ended.ms = ended.ms.map(|ms| (ms * 1000.0).round() / 1000.0);
        if !matches!(ended.outcome, Outcome::Passed | Outcome::Skipped) {
            self.sent.insert(key, ended.clone());
        }
        out.push(Report::Ended(ended));
    }

    fn result(&mut self, m: Message, out: &mut Vec<Report>) {
        let Some(id) = m.id.clone() else { return };
        // pytest-rerunfailures' attempt that will be run again: no result yet.
        if m.outcome.as_deref() == Some("rerun") {
            return;
        }
        let key = self.ctx.key(&id);
        self.running.retain(|(_, r)| *r != id);
        self.find(&id, None, out);
        // What the test printed: each section's text.
        if let Some(sections) = &m.output {
            for text in sections.values() {
                if !text.trim().is_empty() {
                    out.push(Report::Output { key: Some(key.clone().into()), text: text.trim_end().to_string() });
                }
            }
        }
        // Learn where the project is on the machine pytest ran on, from a
        // full path of a test's own file.
        if let (Some(file), None) = (m.file.as_deref(), &self.machine) {
            let file = file.replace('\\', "/");
            let own = super::read::parts(&id).file.to_string();
            if let Some(rest) = file.strip_suffix(&own).and_then(|r| r.strip_suffix('/')) {
                self.machine = Some(rest.to_string());
            }
        }
        let ended = self.ended(&key, &m);
        match self.pending.take() {
            Some((k, held)) if k == key => self.pending = Some((k, held.and_then(ended))),
            held => {
                self.pending = held;
                self.flush(out);
                // A phase that came after its test was sent.
                self.pending = Some(match self.sent.remove(&key) {
                    Some(old) => (key, old.and_then(ended)),
                    None => (key, ended),
                });
            }
        }
    }

    /// One phase's result as an `Ended`.
    fn ended(&self, key: &str, m: &Message) -> Ended {
        let when = m.when.as_deref().unwrap_or("call");
        let mut e = Ended::new(key, Outcome::Passed);
        if let Some(d) = m.duration {
            e = e.ms((d * 1000.0 * 1000.0).round() / 1000.0);
        }
        let text = m.text.as_deref().map(without_worker).unwrap_or("");
        let message = m.message.as_deref().map(first_line).filter(|l| !l.is_empty()).or_else(|| Some(first_line(text)).filter(|l| !l.is_empty()));
        let xfail = m.xfail.as_ref().map(|x| match x {
            Value::String(s) if !s.is_empty() => format!("xfail: {s}"),
            _ => "xfail".to_string(),
        });
        let at = m
            .at
            .as_ref()
            .map(|(f, l)| self.ctx.at(f, *l, self.machine.as_deref()))
            .or_else(|| m.file.as_deref().zip(m.line).filter(|(f, _)| self.is_in_project(f)).map(|(f, l)| self.ctx.at(f, l, self.machine.as_deref())));
        match m.outcome.as_deref() {
            Some("failed") => {
                let what = message.unwrap_or("it failed");
                e.outcome = match when {
                    "call" => Outcome::Failed,
                    _ => Outcome::Errored,
                };
                e.message = Some(match when {
                    "setup" => format!("setup: {what}"),
                    "teardown" => format!("teardown: {what}"),
                    "call" => what.to_string(),
                    // xdist's `worker 'gw0' crashed while running ...`.
                    _ => what.to_string(),
                });
                e.at = at;
                if !text.trim().is_empty() {
                    e.text = Some(text.trim_end().to_string());
                }
            }
            Some("skipped") => {
                e.outcome = Outcome::Skipped;
                e.message = Some(xfail.unwrap_or_else(|| skip_reason(m.message.as_deref().unwrap_or("")).to_string()));
            }
            _ => {
                if let Some(x) = xfail {
                    e.message = Some(format!("{x}, but it passed"));
                }
            }
        }
        e
    }

    /// Whether a full path is in the project: under the folder a result
    /// has told, or `/project` itself.
    fn is_in_project(&self, file: &str) -> bool {
        let file = file.replace('\\', "/");
        !(file.starts_with('/') || file.as_bytes().get(1) == Some(&b':')) || self.machine.as_deref().is_some_and(|m| file.starts_with(&format!("{m}/")))
    }

    fn collect_error(&mut self, m: Message, out: &mut Vec<Report>) {
        let (Some(id), Some(message)) = (m.id, m.message) else { return };
        self.broken = true;
        // `E   SyntaxError: invalid syntax`: pytest's last `E` line, else its last.
        let why = message.lines().rev().find_map(|l| l.strip_prefix("E ").map(str::trim)).filter(|l| !l.is_empty()).unwrap_or_else(|| first_line(message.lines().next_back().unwrap_or("")));
        // Where: `tests/test_x.py:3: in <module>`, or a SyntaxError's `File "...", line 1`.
        let line = message
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{id}:")).and_then(|r| r.split(':').next()).and_then(|n| n.parse::<u32>().ok()))
            .or_else(|| message.lines().find_map(|l| l.trim_start_matches("E ").trim().strip_prefix("File \"").filter(|r| r.contains(&id)).and_then(|r| r.rsplit_once("line ")).and_then(|(_, n)| n.trim().parse::<u32>().ok())));
        let key = self.ctx.key(&id);
        out.push(Report::Broken { under: None, message: format!("{key} didn't load: {why}"), at: line.map(|l| self.ctx.at(&id, l, None)) });
    }

    /// A line pytest or Python wrote itself.
    fn text(&mut self, line: &str, stderr: bool) {
        let t = line.trim();
        if t.is_empty() {
            return;
        }
        if t.contains("No module named 'pytest'") || t.contains("No module named pytest") {
            self.no_pytest = true;
        }
        if t.starts_with("Fatal Python error:") && self.fatal.is_none() {
            self.fatal = Some(t.to_string());
        }
        if let Some(rest) = t.strip_prefix("INTERNALERROR> ") {
            if !rest.trim().is_empty() {
                self.internal = Some(rest.trim().to_string());
            }
        }
        if let Some(rest) = t.strip_prefix("ERROR: ") {
            if self.usage.is_none() {
                self.usage = Some(rest.to_string());
            }
        }
        if stderr {
            if t.contains("while loading conftest") && self.loading.is_none() {
                self.loading = Some(t.trim_end_matches('.').to_string());
            }
            // `tests/conftest.py:1: in <module>`
            if self.place.is_none() {
                if let Some((file, rest)) = t.split_once(".py:") {
                    if let Some(n) = rest.split(':').next().and_then(|n| n.parse::<u32>().ok()) {
                        self.place = Some((format!("{file}.py"), n));
                    }
                }
            }
            self.last_error = Some(t.strip_prefix("E ").map(str::trim).unwrap_or(t).to_string());
        }
    }
}

impl Reader for PytestReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        match line.text.strip_prefix(MARK) {
            Some(json) => self.json(json, out),
            None => self.text(line.text, line.stderr),
        }
    }

    fn end(&mut self, end: &StepEnd, out: &mut Vec<Report>) {
        self.flush(out);
        let StepEnd::Exited(code) = end else { return };
        if self.no_pytest {
            if let Some(f) = &self.ctx.facts {
                f.borrow_mut().missing = true;
            }
            out.push(Report::Broken { under: None, message: "pytest isn't installed in this environment".into(), at: None });
            return;
        }
        let at = self.place.as_ref().map(|(f, l)| self.ctx.at(f, *l, None));
        match (self.done, *code) {
            // pytest's own end: usage errors (a path that's gone), then what
            // ran, ran.
            (Some(4), _) | (None, Some(4)) => {
                let why = self.usage.clone().or_else(|| self.last_error.clone()).unwrap_or_else(|| "usage error".into());
                out.push(Report::Broken { under: None, message: format!("pytest couldn't start: {}{}", self.loading.as_ref().map(|l| format!("{l}: ")).unwrap_or_default(), why), at });
            }
            (Some(3), _) => {
                let why = self.internal.clone().unwrap_or_else(|| "internal error".into());
                out.push(Report::Crashed { under: None, message: format!("pytest had an internal error: {why}") });
            }
            (Some(_), _) => {}
            // No `done`: the process died, or the plugin never loaded.
            (None, code) => {
                if self.broken {
                    return;
                }
                let died = match (&self.fatal, code) {
                    (Some(f), _) => f.clone(),
                    (None, None) => "pytest was killed by a signal".to_string(),
                    (None, Some(c)) => format!("pytest exited with code {c} before the run ended{}", self.last_error.as_ref().map(|e| format!(": {e}")).unwrap_or_default()),
                };
                let message = match self.running.iter().next() {
                    Some((_, id)) => format!("{died} while running {}", self.ctx.key(id)),
                    None => died,
                };
                out.push(Report::Crashed { under: None, message });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_pytests_last_line_whatever_its_verbosity() {
        for l in ["5 failed, 16 passed, 4 skipped, 1 xfailed in 0.04s", "1 passed in 0.01s", "no tests ran in 0.00s", "28 tests collected in 0.04s", "========================= 1 failed, 1 passed in 0.02s ==========================", "2 failed in 1.50s (0:00:01)", "1/28 tests collected (27 deselected) in 0.01s"] {
            assert!(is_summary(l), "{l}");
        }
        for l in ["", "FAILED tests/x.py::t - assert 1 == 2", "tests/test_basic.py::test_adds", "something in time", "x in s"] {
            assert!(!is_summary(l), "{l}");
        }
    }

    #[test]
    fn a_skips_reason_has_pytests_prefix_off_and_a_worker_header_goes() {
        assert_eq!(skip_reason("Skipped: slow"), "slow");
        assert_eq!(skip_reason("Skipped"), "");
        assert_eq!(skip_reason("assert False"), "assert False");
        assert_eq!(without_worker("[gw1] linux -- Python 3.13.18 /venv/bin/python\n\ndef t():\n    pass"), "def t():\n    pass");
        assert_eq!(without_worker("[gw is a thing]\nmore"), "[gw is a thing]\nmore");
    }
}
