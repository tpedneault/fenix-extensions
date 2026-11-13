//! What to run for an ask: `go test -json` and `go list` commands, split
//! under the command line's limit.
//!
//! | Ask | Steps |
//! |---|---|
//! | discover | `go list -e -f <package, folder, test files> ./...`: the test files Go would build, without building |
//! | run all | `go test -json -fullpath ./...`, claiming nothing |
//! | tests | per module, per set of names: `-run '^(TestA\|TestB)$' <packages>` by import path, so a name in another package isn't run there |
//! | subtests | a step per parent: `-run '^TestTable$/^(negative_one\|small)$'`, a level per `/` |
//! | a package, or a file with no fill-in | its import path, all of it (Go can't run a file) |
//! | a file's fill-in | its top-level tests by name; a case whose parent is named is left to it |
//!
//! A project with a `go.work` runs from its own folder, each `use`d
//! folder as `./app/...` (`./...` fails there when the top isn't a module
//! of the work); a folder of modules with no `go.work` has a step for
//! each module, run in its own folder. A test's own `Covered::Test`, a
//! group's `Covered::Group` on the last step of its package, so a group is
//! judged once all of it has run. Go's test cache is kept (`-count=1` is
//! left out, D-G1): a package that's cached says so, with its tests'
//! events and times of 0.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use fenix_extension_api::Command;
use fenix_lang_kit::tests::{Ask, Asked, Covered, Found, Kind, Step, DIRECT};

use super::layout::{key, parse_key, project_dir, quote_meta, Layout};

/// `go list`'s format: tabs between fields, files joined by commas (a
/// space in a folder's name doesn't break it).
pub const LIST_FORMAT: &str = r#"{{.ImportPath}}{{"\t"}}{{.Dir}}{{"\t"}}{{join .TestGoFiles ","}}{{"\t"}}{{join .XTestGoFiles ","}}"#;

/// More steps than this run whole packages instead: coarser, never wrong
/// (Fenix runs 256 at most).
const STEPS_MAX: usize = 200;

pub struct Planner<'a> {
    /// `go`, by its full path when found.
    pub go: &'a str,
    pub layout: &'a Layout,
}

/// What one package is asked for.
#[derive(Debug, Default)]
struct Work {
    /// All of it.
    whole: bool,
    /// Top-level tests by name.
    names: BTreeSet<String>,
    /// Subtests by their parent's levels: `["TestTable"]` → `negative_one`.
    subs: BTreeMap<Vec<String>, BTreeSet<String>>,
    tests: Vec<Covered>,
    groups: Vec<Covered>,
}

type Asks = BTreeMap<String, BTreeMap<String, Work>>;

/// `names` quoted for a `-run` pattern, once, so measuring a command made
/// of some of them again and again doesn't quote them again.
fn quoted(names: &[String]) -> Vec<String> {
    names.iter().map(|n| quote_meta(n)).collect()
}

/// A `-run` alternation of names already quoted: `^TestA$`, `^(TestA|TestB)$`.
pub fn alternation(quoted: &[String]) -> String {
    match quoted {
        [one] => format!("^{one}$"),
        _ => format!("^({})$", quoted.join("|")),
    }
}

/// A `-run` pattern for a test or subtest by its levels: `^TestTable$/^small$`.
pub fn run_pattern(levels: &[&str]) -> String {
    levels.iter().map(|l| format!("^{}$", quote_meta(l))).collect::<Vec<_>>().join("/")
}

/// The batches `items` go in: up to 64 or 2 KiB each, so a command that
/// takes them is measured a few dozen times, not 2,000.
fn batch_ends(items: &[String]) -> Vec<usize> {
    let mut ends = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (i, item) in items.iter().enumerate() {
        if i > start && (bytes + item.len() > 2048 || i - start == 64) {
            ends.push(i);
            start = i;
            bytes = 0;
        }
        bytes += item.len() + 1;
    }
    if !items.is_empty() {
        ends.push(items.len());
    }
    ends
}

/// `items` in as few commands as fit Windows' limit ([`DIRECT`]), each
/// with the range of items it holds. lang-kit's `split` measures a whole
/// command for each item, which for 2,000 names is too slow for a call's
/// few milliseconds; this measures once for each batch.
fn pack(items: &[String], make: impl Fn(&[String]) -> Command) -> Vec<(Command, Range<usize>)> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let (mut start, mut prev) = (0, 0);
    for end in batch_ends(items) {
        if prev > start && !DIRECT.fits(&make(&items[start..end])) {
            out.push((make(&items[start..prev]), start..prev));
            start = prev;
        }
        prev = end;
    }
    out.push((make(&items[start..prev]), start..prev));
    out
}

impl Planner<'_> {
    fn go(&self, cwd: &str) -> Command {
        Command::new(self.go).cwd(&project_dir(cwd))
    }

    fn test(&self, cwd: &str) -> Command {
        self.go(cwd).args(["test", "-json", "-fullpath"])
    }

    /// The patterns that name every package of the project, by the folder
    /// each runs in.
    fn everything(&self) -> Vec<(String, Vec<String>)> {
        if let Some(dirs) = &self.layout.work {
            if dirs.is_empty() {
                return vec![(String::new(), vec!["./...".into()])];
            }
            let patterns = dirs.iter().map(|d| if d.is_empty() { "./...".to_string() } else { format!("./{d}/...") }).collect();
            return vec![(String::new(), patterns)];
        }
        let mut dirs: Vec<&str> = self.layout.modules.iter().map(|m| m.dir.as_str()).collect();
        if dirs.is_empty() {
            dirs.push("");
        }
        dirs.sort();
        dirs.dedup();
        dirs.into_iter().map(|d| (d.to_string(), vec!["./...".to_string()])).collect()
    }

    pub fn discover(&self) -> Vec<Step> {
        self.everything()
            .into_iter()
            .map(|(cwd, patterns)| {
                let label = if cwd.is_empty() { "the packages' test files".to_string() } else { format!("{cwd}'s packages' test files") };
                Step::new(self.go(&cwd).args(["list", "-e", "-f", LIST_FORMAT]).args(patterns), label)
            })
            .collect()
    }

    pub fn run_all(&self) -> Vec<Step> {
        self.everything()
            .into_iter()
            .map(|(cwd, patterns)| {
                let label = if cwd.is_empty() { "every test".to_string() } else { format!("every test of {cwd}") };
                Step::new(self.test(&cwd).args(patterns), label)
            })
            .collect()
    }

    /// What's asked of `package`, made when it's first named.
    fn work<'a>(&self, asks: &'a mut Asks, package: &str) -> &'a mut Work {
        let cwd = self.layout.cwd_of(package);
        if !asks.contains_key(&cwd) {
            asks.insert(cwd.clone(), BTreeMap::new());
        }
        let packages = asks.get_mut(&cwd).expect("made above");
        if !packages.contains_key(package) {
            packages.insert(package.to_string(), Work::default());
        }
        packages.get_mut(package).expect("made above")
    }

    fn add_test(&self, asks: &mut Asks, f: &Found) {
        let Some((package, name)) = parse_key(&f.key.0) else { return };
        let w = self.work(asks, package);
        match name.rsplit_once('/') {
            None => {
                w.names.insert(name.to_string());
            }
            Some((parents, last)) => {
                w.subs.entry(parents.split('/').map(String::from).collect()).or_default().insert(last.to_string());
            }
        }
        w.tests.push(Covered::Test(f.key.clone()));
    }

    fn add(&self, asks: &mut Asks, item: &Asked) {
        match item {
            Asked::Test(f) => self.add_test(asks, f),
            Asked::Group { groups, tests, .. } => {
                let Some(package) = groups.first().map(|g| g.label.clone()) else { return };
                let w = self.work(asks, &package);
                w.groups.push(Covered::Group(groups.iter().map(|g| g.label.clone()).collect()));
                match tests {
                    // A file's or a suite's fill-in, whole: each top-level
                    // test by name, and a case left to its parent, which
                    // runs all of its cases.
                    Some(tests) if groups.len() > 1 => {
                        let keys: BTreeSet<&str> = tests.iter().map(|t| t.key.0.as_str()).collect();
                        for t in tests {
                            if t.kind == Kind::Case && t.of.as_ref().is_some_and(|of| keys.contains(of.0.as_str())) {
                                continue;
                            }
                            self.add_test(asks, t);
                        }
                    }
                    // A package; or a file Fenix sent no fill-in for, which
                    // Go has no filter for: its package, all of it.
                    _ => {
                        let package = groups[0].label.as_str();
                        if let Some(w) = asks.get_mut(&self.layout.cwd_of(package)).and_then(|m| m.get_mut(package)) {
                            w.whole = true;
                        }
                    }
                }
            }
        }
    }

    /// Steps for what `asks` names: whole packages, then top-level names,
    /// then subtests, in each folder.
    fn steps(&self, asks: &Asks) -> Vec<Step> {
        let mut steps: Vec<Step> = Vec::new();
        for (cwd, packages) in asks {
            let first = steps.len();
            // Whole packages.
            let whole: Vec<&String> = packages.iter().filter(|(_, w)| w.whole).map(|(p, _)| p).collect();
            let names: Vec<String> = whole.iter().map(|p| p.to_string()).collect();
            for (cmd, range) in pack(&names, |some| self.test(cwd).args(some.iter().cloned())) {
                let covers: Vec<Covered> = names[range.clone()].iter().flat_map(|p| packages[p].tests.iter().chain(packages[p].groups.iter()).cloned()).collect();
                steps.push(Step::new(cmd, names[range].join(", ")).covers(covers));
            }
            // Top-level names: packages asking for the same ones share a step.
            let mut sets: BTreeMap<Vec<String>, Vec<&String>> = BTreeMap::new();
            for (p, w) in packages.iter().filter(|(_, w)| !w.whole && !w.names.is_empty()) {
                sets.entry(w.names.iter().cloned().collect()).or_default().push(p);
            }
            for (set, pkgs) in &sets {
                let q = quoted(set);
                for chunk in pkgs.chunks(40) {
                    for (cmd, range) in pack(&q, |some| self.test(cwd).args(["-run".to_string(), alternation(some)]).args(chunk.iter().map(|p| p.to_string()))) {
                        let covers: Vec<Covered> = chunk.iter().flat_map(|p| set[range.clone()].iter().map(move |n| Covered::Test(key(p, n).into()))).collect();
                        let label = chunk.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                        steps.push(Step::new(cmd, label).covers(covers));
                    }
                }
            }
            // Subtests, a step for each parent.
            for (p, w) in packages.iter().filter(|(_, w)| !w.whole) {
                for (parents, lasts) in &w.subs {
                    let lasts: Vec<String> = lasts.iter().cloned().collect();
                    let q = quoted(&lasts);
                    let head: Vec<&str> = parents.iter().map(String::as_str).collect();
                    for (cmd, range) in pack(&q, |some| self.test(cwd).args(["-run".to_string(), format!("{}/{}", run_pattern(&head), alternation(some)), p.to_string()])) {
                        let covers: Vec<Covered> = lasts[range.clone()].iter().map(|l| Covered::Test(key(p, &format!("{}/{l}", parents.join("/"))).into())).collect();
                        steps.push(Step::new(cmd, format!("{p} {}", parents.join("/"))).covers(covers));
                    }
                }
            }
            // A group is judged on its package's last step.
            for (p, w) in packages.iter().filter(|(_, w)| !w.whole && !w.groups.is_empty()) {
                let mine = |s: &Step| s.command.args.iter().any(|a| a == p);
                if let Some(last) = steps[first..].iter_mut().rev().find(|s| mine(s)) {
                    last.covers.extend(w.groups.iter().cloned());
                }
            }
        }
        steps
    }

    pub fn run(&self, items: &[Asked]) -> Vec<Step> {
        let mut asks = Asks::new();
        for item in items {
            self.add(&mut asks, item);
        }
        let steps = self.steps(&asks);
        if steps.len() <= STEPS_MAX {
            return steps;
        }
        for packages in asks.values_mut() {
            for w in packages.values_mut() {
                w.whole = true;
            }
        }
        self.steps(&asks)
    }
}

/// The steps for `ask`.
pub fn plan(p: &Planner, ask: &Ask) -> Vec<Step> {
    match ask {
        Ask::Discover => p.discover(),
        Ask::RunAll => p.run_all(),
        Ask::Run(items) => p.run(items),
        Ask::Debug(_) => Vec::new(),
    }
}
