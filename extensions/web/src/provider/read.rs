//! The reader of Fenix's two reporters' lines, `FENIX-TEST {json}` on
//! stdout ([`super::vitest`]'s and [`super::jest`]'s), and what they share:
//! a reporter's paths made the project's, a failure's place from its stack,
//! a reported test named as the scan named it, placed where the scan has no
//! such test, and how a step that printed none of them ended.
//!
//! The reporters print every path as Node has it (`/home/ann/orbit/src/
//! orbit.test.js`, `C:\src\orbit\src\orbit.test.js`, `file:///C:/...`) and,
//! on each line, the folder the runner runs in: the step's package's
//! folder. Below the project is what that folder's path has before the
//! package's own (`packages/app`), so a path is made the project's without
//! knowing where the project is.

use std::collections::{HashMap, HashSet};

use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::tests::{At, Ended, Found, Group, Kind, Line, Outcome, Reader, Report, StepEnd};

use super::index::{made_by, Shared};
use super::layout::{key, names_data, nth, parse_key, Layout, Runner};

/// What a step's reader reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `-t FENIX_NOTHING`: the tests there are, none run.
    Listing,
    Run,
}

/// The tests a step named with `-t`: a result of one it left out (both
/// runners report those skipped) isn't one.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub exact: HashSet<String>,
    /// Keys' starts: a describe's (`src/orbit.test.js > outer > `), a
    /// file's.
    pub prefixes: Vec<String>,
    /// Templates: their file and names.
    pub templates: Vec<(String, Vec<String>)>,
}

impl Selection {
    pub fn wants(&self, key: &str, file: &str, names: &[String]) -> bool {
        self.exact.contains(key)
            || self.prefixes.iter().any(|p| key.starts_with(p.as_str()))
            || self.templates.iter().any(|(f, t)| {
                f == file && t.len() == names.len() && t[..t.len() - 1] == names[..names.len() - 1] && made_by(&t[t.len() - 1], &names[names.len() - 1])
            })
    }
}

/// What a reader knows of its step.
pub struct Ctx {
    pub layout: Layout,
    /// The step's package, by its folder; none after a restart (then
    /// found from the reporter's folder).
    pub package: Option<usize>,
    pub index: Shared,
    pub purpose: Purpose,
    /// None: every result counts (the step names no tests).
    pub select: Option<Selection>,
}

impl Ctx {
    /// The package of a line whose runner runs in `cwd`.
    fn package(&self, cwd: &str) -> Option<usize> {
        if self.package.is_some() {
            return self.package;
        }
        let cwd = slashes(cwd);
        let cwd = cwd.trim_end_matches('/');
        self.layout.packages.iter().enumerate().filter(|(_, p)| p.dir.is_empty() || cwd.ends_with(&format!("/{}", p.dir))).max_by_key(|(_, p)| p.dir.len()).map(|(i, _)| i)
    }

    /// `path`, as a reporter running in `cwd` printed it, below the
    /// project: none for one outside it.
    pub fn below(&self, cwd: &str, path: &str) -> Option<String> {
        let dir = self.package(cwd).and_then(|p| self.layout.packages.get(p)).map(|p| p.dir.as_str()).unwrap_or("");
        let cwd = slashes(cwd);
        let cwd = cwd.trim_end_matches('/');
        let root = if dir.is_empty() { cwd } else { cwd.strip_suffix(&format!("/{dir}")).unwrap_or(cwd) };
        let path = slashes(&from_url(path));
        let rest = strip_prefix_ci(&path, root)?;
        let rest = rest.strip_prefix('/')?;
        (!rest.is_empty()).then(|| rest.to_string())
    }

    /// The groups above a test of `file`: its package's, then its file's.
    pub fn base(&self, cwd: &str, file: &str) -> Vec<Group> {
        let mut groups = self.package(cwd).map(|p| self.layout.base(p)).unwrap_or_default();
        groups.push(Group::file(file));
        groups
    }

    pub fn runner(&self, cwd: &str) -> Option<Runner> {
        self.package(cwd).and_then(|p| self.layout.packages.get(p)).map(|p| p.runner)
    }
}

/// `\` as `/`.
pub fn slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// A `file://` URL as a path: `file:///C:/x%20y/a.js` is `C:/x y/a.js`.
pub fn from_url(path: &str) -> String {
    let Some(rest) = path.strip_prefix("file://") else { return path.to_string() };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let rest = if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() { &rest[1..] } else { rest };
    let mut out = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `path` without `prefix`; a drive letter's path (`C:/`) is compared
/// without case, as Windows does.
fn strip_prefix_ci<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if let Some(rest) = path.strip_prefix(prefix) {
        return Some(rest);
    }
    let windows = prefix.as_bytes().get(1) == Some(&b':');
    let head = path.get(..prefix.len())?;
    (windows && head.eq_ignore_ascii_case(prefix)).then(|| &path[prefix.len()..])
}

/// Text without its terminal colours: the reporters' JSON carries them
/// escaped (`\u001b[31m`), past lang-kit's stripping of the line.
pub fn plain(text: &str) -> String {
    if !text.contains('\u{1b}') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

/// A message's first line that says something.
pub fn first_line(text: &str) -> Option<String> {
    plain(text).lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// `…/src/orbit.test.js:10:21` as its path, line and column: read from
/// the end, so `C:\src\x.js:9:21`'s drive keeps its colon.
pub fn split_place(text: &str) -> Option<(&str, u32, Option<u32>)> {
    let mut parts = text.trim().rsplitn(3, ':');
    let (last, middle) = (parts.next()?, parts.next()?);
    match parts.next() {
        Some(file) if middle.parse::<u32>().is_ok() => Some((file, middle.parse().ok()?, last.parse().ok())),
        _ => Some((middle, last.parse().ok()?, None)),
    }
}

/// The first frame of `stack` (`    at Object.toBe (/…/src/orbit.test.js:9:21)`,
/// `    at /…/src/orbit.test.js:10:21`) in the project, not in a
/// `node_modules`.
pub fn first_frame(ctx: &Ctx, cwd: &str, stack: &str) -> Option<At> {
    for l in stack.lines() {
        let Some(rest) = l.trim().strip_prefix("at ") else { continue };
        let place = match rest.strip_suffix(')') {
            Some(r) => r.rsplit_once('(').map_or(r, |(_, p)| p),
            None => rest,
        };
        let Some((path, line, column)) = split_place(place) else { continue };
        let Some(file) = ctx.below(cwd, path) else { continue };
        if file.split('/').any(|s| s == "node_modules") {
            continue;
        }
        return Some(At::new(file, line, column));
    }
    None
}

/// A reporter's line, parsed.
pub struct Said<'a> {
    pub v: &'a Value,
    pub cwd: &'a str,
}

impl Said<'_> {
    pub fn str(&self, field: &str) -> Option<&str> {
        self.v[field].as_str()
    }

    pub fn names(&self) -> Option<Vec<String>> {
        let n: Vec<String> = self.v["names"].as_array()?.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<_>>()?;
        (!n.is_empty()).then_some(n)
    }

    pub fn line(&self) -> Option<u32> {
        self.v["line"].as_u64().and_then(|l| u32::try_from(l).ok())
    }
}

/// What one reporter's lines mean.
pub trait Format {
    /// A line of `kind`: the reports it makes.
    fn line(&mut self, r: &mut Read, said: &Said, kind: &str, out: &mut Vec<Report>);
}

/// What a reader keeps between lines, whichever the reporter.
pub struct Read {
    pub ctx: Ctx,
    /// How many tests of each file and names each kind of line gave, for
    /// a repeated name's ` (2)`.
    seen: HashMap<(&'static str, String, Vec<String>), usize>,
    found: HashSet<String>,
    pub ended: HashSet<String>,
    /// What each test printed, until its result.
    pub printed: HashMap<String, String>,
    /// Whether any reporter line came, and the last lines of the rest.
    pub any: bool,
    tail: Vec<String>,
}

/// A test as a line names it.
pub struct Named {
    pub key: String,
    pub file: String,
    pub names: Vec<String>,
    pub line: Option<u32>,
}

impl Read {
    pub fn new(ctx: Ctx) -> Read {
        Read { ctx, seen: HashMap::new(), found: HashSet::new(), ended: HashSet::new(), printed: HashMap::new(), any: false, tail: Vec::new() }
    }

    /// The test a line of `kind` names, keyed as the scan keyed it: the
    /// scanned one with these names (the one at its line, when several
    /// share them), else its names with ` (k)` for the `k`-th.
    pub fn named(&mut self, said: &Said, kind: &'static str) -> Option<Named> {
        let file = self.ctx.below(said.cwd, said.str("file")?)?;
        let names = said.names()?;
        let line = said.line();
        let k = {
            let n = self.seen.entry((kind, file.clone(), names.clone())).or_insert(0);
            *n += 1;
            *n
        };
        let scanned = self.ctx.index.borrow().key_for(&file, &names, line, k);
        let key = scanned.unwrap_or_else(|| {
            let mut n = names.clone();
            if let Some(last) = n.last_mut() {
                *last = nth(last, k);
            }
            key(&file, &n)
        });
        Some(Named { key, file, names, line })
    }

    /// `t`, named from a line of `kind`, not counted: a test's logs are
    /// many lines (so a repeated name's logs go to its first).
    pub fn forget(&mut self, t: &Named, kind: &'static str) {
        if let Some(n) = self.seen.get_mut(&(kind, t.file.clone(), t.names.clone())) {
            *n = n.saturating_sub(1);
        }
    }

    /// Whether a result for `t` counts in this step.
    pub fn wants(&self, t: &Named) -> bool {
        self.ctx.select.as_ref().is_none_or(|s| s.wants(&t.key, &t.file, &t.names))
    }

    /// A `Found` for a test no scan found, once a step: under its template
    /// (a `test.each` case, by its line or title), else under the deepest
    /// describe the scan found it in and the rest of its describes.
    pub fn found(&mut self, said: &Said, t: &Named, out: &mut Vec<Report>) {
        let listing = self.ctx.purpose == Purpose::Listing;
        self.ctx.index.borrow_mut().reported(&t.key, &t.names, listing);
        if self.ctx.index.borrow().is_scanned(&t.key) || !self.found.insert(t.key.clone()) {
            return;
        }
        let label = parse_key(&t.key).and_then(|(_, n)| n.last().cloned()).unwrap_or_else(|| t.names.last().cloned().unwrap_or_default());
        let index = self.ctx.index.borrow();
        let mut f = match index.template_of(&t.file, &t.names, t.line) {
            Some(template) => Found::case(t.key.clone(), label, template.key.clone()).under(template.groups.clone()),
            None => {
                let (mut groups, from) = index.groups_of(&t.file, &t.names).unwrap_or_else(|| (self.ctx.base(said.cwd, &t.file), 0));
                for name in &t.names[from..t.names.len() - 1] {
                    groups.push(Group::new(Kind::Suite, name.clone()).in_file(t.file.clone()));
                }
                Found::test(t.key.clone(), label).under(groups)
            }
        };
        drop(index);
        match t.line {
            Some(line) => f = f.at(t.file.clone(), line),
            None => f.file = Some(t.file.clone()),
        }
        if t.names.iter().any(|n| n.contains(" > ")) || parse_key(&t.key).is_none_or(|(_, n)| n != t.names) {
            f = f.data(names_data(&t.names));
        }
        out.push(Report::Found(f));
    }

    /// A result: what the test printed, then it.
    pub fn ended(&mut self, e: Ended, out: &mut Vec<Report>) {
        if let Some(text) = self.printed.remove(&e.key.0) {
            let text = text.trim_end().to_string();
            if !text.is_empty() {
                out.push(Report::Output { key: Some(e.key.clone()), text });
            }
        }
        self.ended.insert(e.key.0.clone());
        out.push(Report::Ended(e));
    }

    /// The labels of file `file`'s groups, for a `broken` or `crashed`.
    pub fn under(&self, cwd: &str, file: &str) -> Vec<String> {
        self.ctx.base(cwd, file).into_iter().map(|g| g.label).collect()
    }

    fn other(&mut self, text: &str) {
        let t = text.trim();
        if !t.is_empty() {
            if self.tail.len() == 40 {
                self.tail.remove(0);
            }
            self.tail.push(t.to_string());
        }
    }

    /// The step ended: a runner killed by a signal crashed; one that
    /// exited with an error before any reporter line didn't load
    /// (a config that doesn't, no test file found): `broken`, with the
    /// first line of its error.
    pub fn end(&mut self, runner: &str, end: &StepEnd, out: &mut Vec<Report>) {
        match end {
            StepEnd::Exited(None) => out.push(Report::Crashed { under: None, message: format!("{runner} was killed by a signal") }),
            StepEnd::Exited(Some(code)) if *code != 0 && !self.any => {
                let why = self.tail.iter().find(|l| l.contains("Error") || l.contains("error") || l.starts_with("No test")).or(self.tail.last()).cloned();
                let message = match why {
                    Some(w) => format!("{runner} ran nothing: {w}"),
                    None => format!("{runner} ran nothing: it exited with {code}"),
                };
                out.push(Report::Broken { under: None, message, at: None });
            }
            _ => {}
        }
    }
}

/// An outcome as either runner says it.
pub fn outcome(state: &str) -> Option<Outcome> {
    match state {
        "pass" | "passed" => Some(Outcome::Passed),
        "fail" | "failed" => Some(Outcome::Failed),
        "skip" | "skipped" | "pending" | "todo" | "disabled" => Some(Outcome::Skipped),
        _ => None,
    }
}

/// The reader of a step: the reporter's lines by their `kind`, the rest
/// kept only for a step that printed none.
pub struct ReporterReader<F: Format> {
    pub read: Read,
    pub format: F,
    pub runner: &'static str,
}

impl<F: Format> Reader for ReporterReader<F> {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        let Some(json) = line.text.trim_start().strip_prefix("FENIX-TEST ").filter(|_| !line.stderr) else {
            self.read.other(line.text);
            return;
        };
        let Ok(v) = serde_json::from_str::<Value>(json) else { return };
        self.read.any = true;
        let (Some(kind), cwd) = (v["kind"].as_str(), v["cwd"].as_str().unwrap_or("")) else { return };
        let said = Said { v: &v, cwd };
        self.format.line(&mut self.read, &said, kind, out);
    }

    fn end(&mut self, end: &StepEnd, out: &mut Vec<Report>) {
        self.read.end(self.runner, end, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::layout::Package;

    fn ctx(dirs: &[&str]) -> Ctx {
        let packages = dirs.iter().map(|d| Package { dir: d.to_string(), runner: Runner::Vitest, installed: None }).collect();
        Ctx { layout: Layout { packages, pnp: false }, package: None, index: Default::default(), purpose: Purpose::Run, select: None }
    }

    #[test]
    fn a_reporters_paths_are_made_the_projects() {
        let c = ctx(&["", "packages/app"]);
        assert_eq!(c.below("/home/ann/orbit", "/home/ann/orbit/src/a.test.js").as_deref(), Some("src/a.test.js"));
        assert_eq!(c.below("/home/ann/orbit/packages/app", "/home/ann/orbit/packages/app/src/a.test.js").as_deref(), Some("packages/app/src/a.test.js"));
        assert_eq!(c.below("/home/ann/orbit/packages/app", "/home/ann/orbit/shared/util.js").as_deref(), Some("shared/util.js"), "outside its package, inside the project");
        assert_eq!(c.below("/home/ann/orbit", "/usr/lib/node.js"), None);
        assert_eq!(c.below("C:\\src\\orbit", "C:\\src\\orbit\\src\\a.test.js").as_deref(), Some("src/a.test.js"), "Windows' paths, by hand");
        assert_eq!(c.below("C:\\src\\orbit", "file:///c:/src/orbit/src/a%20b.test.js").as_deref(), Some("src/a b.test.js"));
        assert_eq!(split_place("C:\\src\\x.test.js:9:21"), Some(("C:\\src\\x.test.js", 9, Some(21))));
        assert_eq!(split_place("/a/x.js:9"), Some(("/a/x.js", 9, None)));
        let stack = "Error: no\n    at Object.<anonymous> (/home/ann/orbit/node_modules/jest-each/build/index.js:80:140)\n    at Object.toBe (C:\\src\\orbit\\src\\orbit.test.js:9:21)\n";
        assert_eq!(first_frame(&ctx(&[""]), "C:\\src\\orbit", stack), Some(At::new("src/orbit.test.js", 9, Some(21))));
        assert_eq!(plain("\u{1b}[31mexpected\u{1b}[39m 4"), "expected 4");
    }
}
