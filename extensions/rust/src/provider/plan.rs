//! What to run for an ask: `cargo test` commands (or `cargo nextest
//! run`'s), a step per package and kind of filter, split under the
//! command line's limit.
//!
//! | Ask | Steps |
//! |---|---|
//! | discover | `cargo test --workspace --no-fail-fast --message-format=json -- --list`: doc tests too |
//! | run all | `cargo test --workspace --no-fail-fast --message-format=json`, claiming nothing; with nextest, `cargo nextest run --workspace ...` and `cargo test --doc` for the doc tests nextest doesn't run |
//! | tests, a file's or a suite's fill-in | per package: `-p <pkg> <target flags> -- --exact <paths>`; with nextest `-E '(binary_id(orbit) & test(=tests::adds)) \| ...'` |
//! | a template (`#[rstest]` with cases) | its path as a substring, `-- tests::sums`: its cases are `tests::sums::case_1`; nextest `test(/^tests::sums(::\|$)/)` |
//! | a target | its flag, no names (`-p orbit --test it`) |
//! | a file or suite with no fill-in | its module path as a substring, `-- geo::` (which also runs `other::geo::`: libtest's filter is a substring); nextest `test(/^geo::/)`, exactly |
//! | doc tests | a step of their own, `--doc` (cargo takes it beside no other target flag), by name as a substring: `--exact` matches no doc test |
//!
//! `--no-fail-fast` everywhere, or cargo stops at the first binary that
//! fails. `-p` names the package even when there's one; a project that's
//! one member of a larger workspace runs `-p <it>` where others run
//! `--workspace`. A test's own `Covered::Test`, a group's `Covered::Group`
//! on the last step of its package, so a group is judged once all of it
//! has run.

use std::collections::BTreeMap;

use fenix_extension_api::Command;
use fenix_lang_kit::tests::{split, Ask, Asked, Covered, Found, Kind, Step, DIRECT};

use super::index::Index;
use super::layout::{parse_key, place_in, Layout, TargetKey};
use crate::PROJECT;

/// The variable that turns on nextest's libtest JSON.
pub const NEXTEST_JSON: &str = "NEXTEST_EXPERIMENTAL_LIBTEST_JSON";

/// What the planner needs of the project.
pub struct Planner<'a> {
    /// cargo, by its full path when found.
    pub cargo: &'a str,
    pub layout: &'a Layout,
    pub index: &'a Index,
    /// cargo-nextest runs the tests (the doc tests are cargo's).
    pub nextest: bool,
}

/// What one entry runs.
#[derive(Debug, Clone)]
struct Entry {
    target: TargetKey,
    /// A path, exactly; or a filter (a template's path, `geo::`).
    name: String,
    cover: Option<Covered>,
}

#[derive(Debug, Default)]
struct Work {
    whole: Vec<(TargetKey, Option<Covered>)>,
    exact: Vec<Entry>,
    prefix: Vec<Entry>,
    doc_all: bool,
    doc: Vec<Entry>,
    /// Groups asked: on the package's last step.
    groups: Vec<Covered>,
}

/// `name` for a nextest filterset's `/regex/`.
fn regex(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if "\\.+*?()|[]{}^$/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// nextest's id of a target's binary: `orbit`, `orbit::it`, `orbit::bin/orbit`.
fn binary_id(t: &TargetKey) -> String {
    match t.target.split_once(':') {
        None => t.package.clone(),
        Some(("test", name)) => format!("{}::{name}", t.package),
        Some((kind, name)) => format!("{}::{kind}/{name}", t.package),
    }
}

/// How many items each of `commands` holds: those after its fixed part.
fn counts(commands: &[Command], fixed: usize, nextest: bool) -> Vec<usize> {
    commands.iter().map(|c| if nextest { c.args.last().map_or(0, |e| e.split(" | ").count()) } else { c.args.len().saturating_sub(fixed) }).collect()
}

impl Planner<'_> {
    fn cargo(&self) -> Command {
        Command::new(self.cargo).cwd(PROJECT)
    }

    /// `--workspace`, or `-p <the project>` for one member of a larger one.
    fn scope(&self) -> Vec<String> {
        match &self.layout.member {
            Some(p) => vec!["-p".into(), p.clone()],
            None => vec!["--workspace".into()],
        }
    }

    fn cargo_test(&self) -> Command {
        self.cargo().arg("test")
    }

    fn nextest_run(&self) -> Command {
        self.cargo().args(["nextest", "run"]).env(NEXTEST_JSON, "1")
    }

    pub fn discover(&self) -> Vec<Step> {
        vec![Step::new(self.cargo_test().args(self.scope()).args(["--no-fail-fast", "--message-format=json", "--", "--list"]), "building the tests to list them")]
    }

    fn has_doc_tests(&self) -> bool {
        self.layout.packages.is_empty() || self.layout.packages.iter().any(|p| p.targets.iter().any(|t| t.doctest))
    }

    pub fn run_all(&self) -> Vec<Step> {
        if !self.nextest {
            return vec![Step::new(self.cargo_test().args(self.scope()).args(["--no-fail-fast", "--message-format=json"]), "every test")];
        }
        let mut steps = vec![Step::new(self.nextest_run().args(self.scope()).args(["--no-fail-fast", "--message-format", "libtest-json-plus"]), "every test (cargo nextest)")];
        if self.has_doc_tests() {
            steps.push(Step::new(self.cargo_test().args(self.scope()).args(["--doc", "--no-fail-fast", "--message-format=json"]), "the doc tests"));
        }
        steps
    }

    /// The module path a file or suite group is, below its target's root:
    /// `geo` for `src/geo/mod.rs`, `tests` for its `mod tests`.
    fn module_of(&self, target: &TargetKey, groups: &[fenix_lang_kit::tests::Group]) -> Option<Vec<String>> {
        let file = groups.get(1)?;
        let path = file.file.as_deref().unwrap_or(&file.label);
        let package = self.layout.package(&target.package);
        let mut module = match package {
            Some(p) => place_in(p, path.strip_prefix(&format!("{}/", p.dir)).filter(|_| !p.dir.is_empty()).unwrap_or(path))?.module,
            None => Vec::new(),
        };
        module.extend(groups[2..].iter().map(|g| g.label.clone()));
        Some(module)
    }

    fn add_test(&self, work: &mut BTreeMap<String, Work>, f: &Found, cover: Option<Covered>) {
        let Some((target, path)) = parse_key(&f.key.0) else { return };
        let w = work.entry(target.package.clone()).or_default();
        let e = Entry { target: target.clone(), name: path.to_string(), cover };
        if target.is_doc() {
            w.doc.push(e);
        } else if f.template || self.index.is_template(&target.id(), path) {
            w.prefix.push(e);
        } else {
            w.exact.push(e);
        }
    }

    fn add(&self, work: &mut BTreeMap<String, Work>, item: &Asked) {
        match item {
            Asked::Test(f) => self.add_test(work, f, Some(Covered::Test(f.key.clone()))),
            Asked::Group { groups, tests, .. } => {
                let Some(target) = groups.first().and_then(|g| TargetKey::from_label(&g.label)) else { return };
                let labels: Vec<String> = groups.iter().map(|g| g.label.clone()).collect();
                if groups.len() == 1 {
                    let w = work.entry(target.package.clone()).or_default();
                    if target.is_doc() {
                        w.doc_all = true;
                        w.groups.push(Covered::Group(labels));
                    } else {
                        w.whole.push((target, Some(Covered::Group(labels))));
                    }
                    return;
                }
                work.entry(target.package.clone()).or_default().groups.push(Covered::Group(labels));
                match tests {
                    // Its fill-in, whole: each test by name, a template by
                    // its path (all its cases), a case of one asked left to it.
                    Some(tests) => {
                        let templates: Vec<&str> = tests.iter().filter(|t| t.template).map(|t| t.key.0.as_str()).collect();
                        for t in tests {
                            if t.kind == Kind::Case && t.of.as_ref().is_some_and(|of| templates.contains(&of.0.as_str())) {
                                continue;
                            }
                            self.add_test(work, t, Some(Covered::Test(t.key.clone())));
                        }
                    }
                    // None: the runner's own filter for it.
                    None if target.is_doc() => {
                        let file = groups[1].file.as_deref().unwrap_or(&groups[1].label);
                        let name = format!("{}{file} - ", self.layout.below_root);
                        work.entry(target.package.clone()).or_default().doc.push(Entry { target, name, cover: None });
                    }
                    None => {
                        let w = work.entry(target.package.clone()).or_default();
                        match self.module_of(&target, groups).filter(|m| !m.is_empty()) {
                            Some(module) => w.prefix.push(Entry { target, name: format!("{}::", module.join("::")), cover: None }),
                            // The target's root file: all of it.
                            None => w.whole.push((target, None)),
                        }
                    }
                }
            }
        }
    }

    /// `--features` for what the targets need.
    fn features(&self, targets: &[&TargetKey]) -> Vec<String> {
        let mut features: Vec<String> = targets.iter().filter_map(|t| self.layout.target(t)).flat_map(|t| t.required_features.iter().cloned()).collect();
        features.sort();
        features.dedup();
        if features.is_empty() {
            Vec::new()
        } else {
            vec!["--features".into(), features.join(",")]
        }
    }

    fn flags(&self, targets: &[&TargetKey]) -> Vec<String> {
        let mut seen: Vec<&TargetKey> = Vec::new();
        for t in targets {
            if !seen.contains(t) {
                seen.push(t);
            }
        }
        let mut args: Vec<String> = seen.iter().flat_map(|t| t.flags()).collect();
        args.extend(self.features(&seen));
        args
    }

    /// Steps for `entries` of `package`, by name after `--` (`--exact`
    /// when `exact`), split under the limit, each covering its entries.
    fn named(&self, package: &str, entries: &[Entry], exact: bool, label: &str) -> Vec<Step> {
        if entries.is_empty() {
            return Vec::new();
        }
        let targets: Vec<&TargetKey> = entries.iter().map(|e| &e.target).collect();
        let base = self.cargo_test().args(["--no-fail-fast", "--message-format=json", "-p", package]).args(self.flags(&targets)).arg("--");
        let base = if exact { base.arg("--exact") } else { base };
        let fixed = base.args.len();
        let mut names: Vec<&str> = Vec::new();
        for e in entries {
            if !names.contains(&e.name.as_str()) {
                names.push(&e.name);
            }
        }
        let commands = split(&names, DIRECT, |some| base.clone().args(some.iter().map(|s| s.to_string())));
        self.steps(commands, counts_of(&names, entries), fixed, false, label)
    }

    /// Commands as steps, each covering the entries whose names it holds.
    fn steps(&self, commands: Vec<Command>, by_name: Vec<Vec<Covered>>, fixed: usize, nextest: bool, label: &str) -> Vec<Step> {
        let mut at = 0;
        let mut out = Vec::new();
        for (cmd, n) in commands.iter().zip(counts(&commands, fixed, nextest)) {
            let covers: Vec<Covered> = by_name[at..(at + n).min(by_name.len())].iter().flatten().cloned().collect();
            at += n;
            out.push(Step::new(cmd.clone(), label).covers(covers));
        }
        out
    }

    fn nextest_steps(&self, package: &str, w: &Work) -> Vec<Step> {
        let mut terms: Vec<(String, Option<Covered>)> = Vec::new();
        let mut targets: Vec<&TargetKey> = Vec::new();
        for (t, cover) in &w.whole {
            terms.push((format!("binary_id({})", binary_id(t)), cover.clone()));
            targets.push(t);
        }
        for e in &w.exact {
            terms.push((format!("(binary_id({}) & test(={}))", binary_id(&e.target), e.name), e.cover.clone()));
            targets.push(&e.target);
        }
        for e in &w.prefix {
            let re = match e.name.strip_suffix("::") {
                Some(module) => format!("^{}::", regex(module)),
                None => format!("^{}(::|$)", regex(&e.name)),
            };
            terms.push((format!("(binary_id({}) & test(/{re}/))", binary_id(&e.target)), e.cover.clone()));
            targets.push(&e.target);
        }
        if terms.is_empty() {
            return Vec::new();
        }
        let base = self.nextest_run().args(["--no-fail-fast", "--message-format", "libtest-json-plus", "-p", package]).args(self.flags(&targets)).arg("-E");
        let exprs: Vec<String> = terms.iter().map(|(t, _)| t.clone()).collect();
        let commands = split(&exprs, DIRECT, |some| base.clone().arg(some.join(" | ")));
        let covers: Vec<Vec<Covered>> = terms.into_iter().map(|(_, c)| c.into_iter().collect()).collect();
        self.steps(commands, covers, 0, true, &format!("{package} (cargo nextest)"))
    }

    pub fn run(&self, items: &[Asked]) -> Vec<Step> {
        let mut work: BTreeMap<String, Work> = BTreeMap::new();
        for item in items {
            self.add(&mut work, item);
        }
        let mut steps = Vec::new();
        for (package, mut w) in work {
            // A target run whole runs its names too.
            let whole: Vec<TargetKey> = w.whole.iter().map(|(t, _)| t.clone()).collect();
            for list in [&mut w.exact, &mut w.prefix] {
                let (inside, rest): (Vec<Entry>, Vec<Entry>) = std::mem::take(list).into_iter().partition(|e| whole.contains(&e.target));
                *list = rest;
                w.whole.extend(inside.into_iter().filter_map(|e| e.cover.map(|c| (e.target, Some(c)))));
            }
            let first = steps.len();
            if self.nextest {
                steps.extend(self.nextest_steps(&package, &w));
            } else {
                if !w.whole.is_empty() {
                    let targets: Vec<&TargetKey> = w.whole.iter().map(|(t, _)| t).collect();
                    let labels: Vec<String> = whole.iter().map(TargetKey::label).collect();
                    let cmd = self.cargo_test().args(["--no-fail-fast", "--message-format=json", "-p", &package]).args(self.flags(&targets));
                    steps.push(Step::new(cmd, labels.join(", ")).covers(w.whole.iter().filter_map(|(_, c)| c.clone())));
                }
                steps.extend(self.named(&package, &w.exact, true, &package));
                steps.extend(self.named(&package, &w.prefix, false, &package));
            }
            if w.doc_all {
                steps.push(Step::new(self.cargo_test().args(["--no-fail-fast", "--message-format=json", "-p", &package, "--doc"]), format!("{package}: doc tests")));
            } else if !w.doc.is_empty() {
                let base = self.cargo_test().args(["--no-fail-fast", "--message-format=json", "-p", &package, "--doc", "--"]);
                let fixed = base.args.len();
                let names: Vec<&str> = w.doc.iter().map(|e| e.name.as_str()).collect();
                let commands = split(&names, DIRECT, |some| base.clone().args(some.iter().map(|s| s.to_string())));
                steps.extend(self.steps(commands, counts_of(&names, &w.doc), fixed, false, &format!("{package}: doc tests")));
            }
            if steps.len() > first {
                let last = steps.last_mut().expect("one");
                last.covers.extend(w.groups);
            }
        }
        steps
    }

    /// The build before debugging `target`: its test binary, named by
    /// cargo's JSON.
    pub fn build(&self, target: &TargetKey) -> Step {
        let cmd = self.cargo_test().args(["--no-run", "--message-format=json", "-p", &target.package]).args(self.flags(&[target]));
        Step::new(cmd, format!("building {} to debug it", target.label()))
    }
}

/// Each name's covers, in the names' order.
fn counts_of(names: &[&str], entries: &[Entry]) -> Vec<Vec<Covered>> {
    names.iter().map(|n| entries.iter().filter(|e| e.name == *n).filter_map(|e| e.cover.clone()).collect()).collect()
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
