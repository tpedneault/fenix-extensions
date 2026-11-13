//! `cargo nextest run --message-format libtest-json-plus`'s output
//! (`NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`; streamed as each test ends
//! since nextest 0.9.103): one JSON line per event on stdout, naming
//! each test's binary (`orbit::it$integration_fails`), its time, and a
//! failure's output -- no pairing of streams, nothing held. Its human
//! lines on stderr (`PASS [   0.005s] (1/10) orbit tests::adds`) give a
//! result before its JSON line comes, the signal a test died of
//! (`SIGABRT [ 0.056s] ... aborts`), and everything when the JSON doesn't
//! parse. A retry's attempt is its own `Ended` (`...#2`): the last wins.
//!
//! Before it runs, nextest builds with cargo, whose errors are on stderr,
//! rendered (`error[E0308]: mismatched types`, ` --> src/lib.rs:19:27`,
//! ``error: could not compile `orbit` (lib test)``): `Broken`, as cargo's
//! reader says them. A binary that can't list its tests (a `harness =
//! false` one that panics) stops the run: `Broken` too.

use std::collections::HashMap;
use std::rc::Rc;

use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::tests::{At, Ended, Line, Outcome, Reader, Report, StepEnd};

use super::layout::TargetKey;
use super::read::{self, Ctx};

pub struct NextestReader {
    ctx: Rc<Ctx>,
    /// Each binary (`orbit`, `orbit::it`) by its package and name: its target.
    binaries: HashMap<(String, String), TargetKey>,
    /// A JSON line was read: the human lines only add to it.
    json: bool,
    /// What the human lines said of each key: a signal it died of.
    signals: HashMap<String, String>,
    /// What JSON said of each key: whether it gave text.
    texts: HashMap<String, bool>,
    /// The first build error and its place, waiting for the target cargo
    /// says couldn't compile.
    error: Option<(String, Option<At>)>,
    failed_targets: Vec<(TargetKey, String, Option<At>)>,
    /// `creating test list failed`: what couldn't list, and why.
    listing: Option<(Option<TargetKey>, Option<String>)>,
    cargo_error: Option<String>,
}

/// nextest's binary id: `orbit` (the library), `orbit::it` (a test
/// target), `orbit::bin/orbit`.
fn binary_id(id: &str) -> TargetKey {
    match id.split_once("::") {
        None => TargetKey::new(id, "lib"),
        Some((package, binary)) => match binary.split_once('/') {
            Some((kind, name)) => TargetKey::new(package, &format!("{kind}:{name}")),
            None => TargetKey::new(package, &format!("test:{binary}")),
        },
    }
}

impl NextestReader {
    pub fn new(ctx: Rc<Ctx>) -> NextestReader {
        NextestReader { ctx, binaries: HashMap::new(), json: false, signals: HashMap::new(), texts: HashMap::new(), error: None, failed_targets: Vec::new(), listing: None, cargo_error: None }
    }

    /// `orbit::orbit$tests::adds#2`: its target and path.
    fn name(&self, name: &str) -> Option<(TargetKey, String)> {
        let (binary, path) = name.split_once('$')?;
        let path = match path.rsplit_once('#') {
            Some((p, n)) if n.chars().all(|c| c.is_ascii_digit()) => p,
            _ => path,
        };
        let (package, bin) = binary.split_once("::")?;
        let target = match self.binaries.get(&(package.to_string(), bin.to_string())) {
            Some(t) => t.clone(),
            None if self.ctx.layout.package(package).and_then(|p| p.targets.iter().find(|t| t.key.target == "lib")).is_some_and(|t| t.crate_name == bin) => TargetKey::new(package, "lib"),
            None => TargetKey::new(package, &format!("test:{bin}")),
        };
        Some((target, path.to_string()))
    }

    /// An ignored test the step wasn't asked for: nextest reports the
    /// binary's ignored tests when a filter picked others.
    fn asked(&self, target: &TargetKey, path: &str) -> bool {
        match &self.ctx.exact {
            None => true,
            Some(names) => names.iter().any(|(t, p)| *t == target.id() && p == path),
        }
    }

    fn json_line(&mut self, v: &Value, out: &mut Vec<Report>) {
        let nextest = &v["nextest"];
        if let (Some(package), Some(binary), Some(kind)) = (nextest["crate"].as_str(), nextest["test_binary"].as_str(), nextest["kind"].as_str()) {
            if let Some(t) = TargetKey::of_kind(package, &[kind.to_string()], binary) {
                self.binaries.insert((package.into(), binary.into()), t);
            }
        }
        if v["type"].as_str() != Some("test") {
            return;
        }
        let Some((target, path)) = v["name"].as_str().and_then(|n| self.name(n)) else { return };
        let key = target.key(&path);
        let ms = v["exec_time"].as_f64().map(|s| s * 1000.0);
        let event = v["event"].as_str().unwrap_or("");
        // nextest starts and reports a binary's ignored tests that a
        // filter didn't pick: not this step's.
        if matches!(event, "started" | "ignored") && !self.asked(&target, &path) {
            return;
        }
        read::found_report(&self.ctx, &target, &path, out);
        match event {
            "started" => out.push(Report::Started(key.into())),
            "ok" => {
                let e = Ended::new(key, Outcome::Passed);
                out.push(Report::Ended(match ms {
                    Some(ms) => e.ms(ms),
                    None => e,
                }));
            }
            "ignored" => {
                let e = Ended::new(key, Outcome::Skipped);
                out.push(Report::Ended(match v["message"].as_str() {
                    Some(m) => e.message(m),
                    None => e,
                }));
            }
            "failed" | "timeout" => {
                let stdout = v["stdout"].as_str().unwrap_or("");
                let lines: Vec<&str> = stdout.lines().collect();
                let (mut e, printed) = read::failure(&key, &lines, &self.ctx.layout);
                if e.message.is_none() {
                    e.message = self.signals.get(&key).cloned().or_else(|| (event == "timeout").then(|| "timed out".to_string()));
                }
                self.texts.insert(key.clone(), e.text.is_some());
                if let Some(ms) = ms {
                    e.ms = Some(ms);
                }
                read::ended(&key, printed, e, out);
            }
            _ => {}
        }
    }

    /// `        PASS [   0.005s] ( 1/10) orbit tests::adds`.
    fn human(&mut self, t: &str, out: &mut Vec<Report>) {
        let Some((status, rest)) = t.split_once(" [") else { return };
        let status = status.trim();
        let Some((time, rest)) = rest.split_once("s]") else { return };
        let rest = rest.trim_start();
        let rest = match rest.strip_prefix('(') {
            Some(r) => r.split_once(')').map(|(_, r)| r.trim_start()).unwrap_or(r),
            None => rest,
        };
        let Some((binary, path)) = rest.split_once(' ') else { return };
        let target = binary_id(binary);
        let key = target.key(path);
        let ms = time.trim().parse::<f64>().ok().map(|s| s * 1000.0);
        let outcome = match status {
            "PASS" => Outcome::Passed,
            "FAIL" | "TIMEOUT" => Outcome::Failed,
            "SKIP" => Outcome::Skipped,
            s if s.starts_with("SIG") || s == "ABORT" => {
                self.signals.insert(key.clone(), format!("it crashed: {s}"));
                Outcome::Failed
            }
            _ => return,
        };
        // Once JSON has come, a human line adds only a crash's signal to a
        // result JSON gave no text for.
        if self.json && !(outcome == Outcome::Failed && self.texts.get(&key) == Some(&false)) {
            return;
        }
        read::found_report(&self.ctx, &target, path, out);
        let mut e = Ended::new(key.clone(), outcome);
        e.ms = ms;
        e.message = self.signals.get(&key).cloned();
        out.push(Report::Ended(e));
    }

    fn stderr(&mut self, text: &str, out: &mut Vec<Report>) {
        let t = text.trim();
        if t.starts_with("PASS [") || t.starts_with("FAIL [") || t.starts_with("SKIP [") || t.starts_with("TIMEOUT [") || (t.starts_with("SIG") && t.contains(" [")) || t.starts_with("TRY ") {
            let t = t.strip_prefix("TRY ").map(|r| r.split_once(' ').map(|(_, r)| r).unwrap_or(r)).unwrap_or(t);
            return self.human(t, out);
        }
        if t == "error: creating test list failed" {
            self.listing = Some((None, None));
            return;
        }
        if let Some((what, why)) = &mut self.listing {
            if let Some(rest) = t.strip_prefix("for `") {
                *what = rest.split('`').next().map(binary_id);
            } else if let Some(("main", _)) = read::panic_line(t) {
                *why = Some(String::new());
            } else if why.as_deref() == Some("") {
                *why = Some(t.to_string());
            }
            return;
        }
        if t.starts_with("error[") || (t.starts_with("error: ") && self.error.is_none() && !t.starts_with("error: could not compile")) {
            if self.error.is_none() && !t.starts_with("error: test run failed") && !t.starts_with("error: command") {
                let message = t.split_once("]: ").map(|(_, m)| m).or_else(|| t.strip_prefix("error: ")).unwrap_or(t);
                self.error = Some((message.to_string(), None));
            }
            return;
        }
        if let (Some(place), Some((_, at @ None))) = (t.strip_prefix("--> "), &mut self.error) {
            *at = read::place(place, &self.ctx.layout);
            return;
        }
        if let Some(rest) = t.strip_prefix("error: could not compile `") {
            // ``orbit` (lib test) due to 1 previous error``
            let (package, rest) = rest.split_once('`').unwrap_or((rest, ""));
            let inner = rest.split_once('(').and_then(|(_, r)| r.split_once(')')).map(|(i, _)| i).unwrap_or("lib");
            let mut words = inner.split(' ').filter(|w| *w != "test");
            let kind = words.next().unwrap_or("lib");
            let name = words.next().map(|n| n.trim_matches('"')).unwrap_or(package);
            let target = TargetKey::of_kind(package, &[kind.to_string()], name).unwrap_or_else(|| TargetKey::new(package, "lib"));
            let (message, at) = self.error.clone().unwrap_or_else(|| ("error".into(), None));
            self.failed_targets.push((target, message, at));
            return;
        }
        if t.starts_with("error") && self.cargo_error.is_none() && !t.starts_with("error: test run failed") {
            self.cargo_error = Some(t.to_string());
        }
    }
}

impl Reader for NextestReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        if line.stderr {
            return self.stderr(line.text, out);
        }
        let Ok(v) = serde_json::from_str::<Value>(line.text) else { return };
        self.json = true;
        self.json_line(&v, out);
    }

    fn end(&mut self, _end: &StepEnd, out: &mut Vec<Report>) {
        let mut first = None;
        for (target, message, at) in &self.failed_targets {
            let label = target.label();
            let place = at.as_ref().map(|a| format!("{}:{} ", a.file, a.line)).unwrap_or_default();
            let message = format!("{label} didn't build: {place}{message}");
            first.get_or_insert_with(|| (message.clone(), at.clone()));
            out.push(Report::Broken { under: Some(vec![label]), message, at: at.clone() });
        }
        if let Some((what, why)) = self.listing.take() {
            let label = what.as_ref().map(TargetKey::label);
            let message = format!("{} couldn't list its tests: {}", label.clone().unwrap_or_else(|| "a test binary".into()), why.filter(|w| !w.is_empty()).unwrap_or_else(|| "see the output".into()));
            first.get_or_insert_with(|| (message.clone(), None));
            out.push(Report::Broken { under: label.map(|l| vec![l]), message, at: None });
        }
        // Nothing ran: the rest of the step didn't either.
        if let Some((message, at)) = first {
            out.push(Report::Broken { under: None, message, at });
        }
    }
}
