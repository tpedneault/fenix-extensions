//! What to run for an ask: a step per package folder (its `cwd`), the
//! runner run by Node as the launch files run it, `node
//! node_modules/vitest/vitest.mjs` and `node node_modules/jest/bin/jest.js`
//! (a workspace's hoisted one by its path from the package), never `npx`
//! or a `node_modules/.bin` shim: on Windows those are `.cmd` files, run
//! through `cmd.exe` and held to its 8,191 characters ([`BATCH_FILE`]).
//! With Node, Windows' 32,766 ([`DIRECT`]). Yarn Plug'n'Play has no
//! `node_modules` to run from: there `yarn vitest`, `yarn jest`, a shim,
//! split under [`BATCH_FILE`].
//!
//! | Ask | Steps, per package |
//! |---|---|
//! | discover | Vitest `run --includeTaskLocation --reporter=<ours> -t FENIX_NOTHING`; Jest `--ci --testLocationInResults --reporters=<ours> -t FENIX_NOTHING`: every test, none run |
//! | run all | Vitest `run --includeTaskLocation --reporter=default --reporter=<ours>`; Jest `--ci --watchAll=false --testLocationInResults --reporters=default --reporters=<ours>` |
//! | a package | the same, covering its group |
//! | files | the same with the files: Vitest's as paths below the package, Jest's as path regexes after `--` (`/src/orbit\.test\.js$`) |
//! | tests, a describe's fill-in | the files and `-t '^(?:outer > inner > deep\|adds)$'`: names joined by ` > ` (Vitest) or spaces (Jest), regex characters escaped; a template's (`add(%i, %i) is %i`) placeholders as `.*` |
//! | a describe with no fill-in | its names as the start: `-t '^(?:outer > .*)$'` |
//!
//! By name, never Vitest's `<file>:<line>`: the scan's line may be an
//! unsaved buffer's, and Vitest reads the disk (plan §11, F18). A file
//! asked runs whole, and the tests asked beside it in its package run in
//! another step (`-t` would filter it). A describe's or a package's group
//! is covered on its package's last step.

use std::cell::RefCell;
use std::collections::BTreeMap;

use fenix_extension_api::Command;
use fenix_lang_kit::tests::{fnv1a_64, split, Ask, Asked, Covered, Found, Group, Kind, Limit, Step, BATCH_FILE, DIRECT};

use super::index::{literal_parts, Index};
use super::layout::{data_names, parse_key, Layout, Package, Runner};
use super::Facts;

/// `text` for a JavaScript regex: its special characters escaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if matches!(c, '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A test's names as one of `-t`'s alternatives: exactly, or, for a
/// template, its title's placeholders as `.*`.
pub fn alternative(names: &[String], template: bool, runner: Runner) -> String {
    let joiner = escape(runner.joiner());
    let Some((title, parents)) = names.split_last() else { return String::new() };
    let mut out: Vec<String> = parents.iter().map(|n| escape(n)).collect();
    if template {
        out.push(literal_parts(title).iter().map(|p| escape(p)).collect::<Vec<_>>().join(".*"));
    } else {
        out.push(escape(title));
    }
    out.join(&joiner)
}

/// `-t`'s regex: one of these, whole.
pub fn whole(alternatives: &[&str]) -> String {
    format!("^(?:{})$", alternatives.join("|"))
}

/// What Fenix gave of a test: its file, names, and whether it's a
/// template -- from its `data`, else the scan's, else its key.
pub fn test_of(f: &Found, index: &Index) -> Option<(String, Vec<String>, bool)> {
    let file = f.key.0.split_once(" > ")?.0;
    let scanned = index.entry(&f.key.0);
    let names = match f.data.as_deref().and_then(data_names).or_else(|| scanned.map(|e| e.names.clone())).or_else(|| index.names_of(&f.key.0)) {
        Some(n) => n,
        None => parse_key(&f.key.0)?.1,
    };
    Some((file.to_string(), names, f.template || scanned.is_some_and(|e| e.template)))
}

pub struct Planner<'a> {
    pub facts: &'a Facts,
    pub index: &'a Index,
}

/// A test (or a describe with no fill-in) to name with `-t`.
#[derive(Debug, Clone)]
struct Named {
    file: String,
    alternative: String,
    cover: Option<Covered>,
}

#[derive(Debug, Default)]
struct Work {
    /// The package asked whole.
    whole: Vec<Covered>,
    /// Files asked whole, below the project.
    files: Vec<(String, Covered)>,
    named: Vec<Named>,
    /// Describes asked: on the package's last step.
    groups: Vec<Covered>,
}

impl Planner<'_> {
    fn layout(&self) -> &Layout {
        &self.facts.layout
    }

    /// The runner of `p`, by Node or Yarn, in its folder; its line's limit.
    pub fn base(&self, p: &Package) -> (Command, Limit) {
        let (cmd, limit) = if self.layout().pnp {
            (Command::new(self.facts.yarn.as_deref().unwrap_or("yarn")).arg(p.runner.id()), BATCH_FILE)
        } else {
            let script = p.installed.as_ref().map(|i| i.script.clone()).unwrap_or_else(|| format!("node_modules/{}", p.runner.script()));
            (Command::new(self.facts.node.as_deref().unwrap_or("node")).arg(script), DIRECT)
        };
        (cmd.cwd(&p.cwd()), limit)
    }

    fn reporter(r: Runner) -> String {
        format!("${{data}}/lang-kit/tests/{}", r.reporter())
    }

    /// Every test of `p`, none run.
    pub fn listing(&self, p: &Package) -> Command {
        let (cmd, _) = self.base(p);
        match p.runner {
            Runner::Vitest => cmd.args(["run".into(), "--includeTaskLocation".into(), format!("--reporter={}", Self::reporter(p.runner)), "-t".into(), "FENIX_NOTHING".into()]),
            Runner::Jest => cmd.args(["--ci".into(), "--testLocationInResults".into(), format!("--reporters={}", Self::reporter(p.runner)), "-t".into(), "FENIX_NOTHING".into()]),
        }
    }

    /// A run of `p`'s tests, before what it names.
    pub fn running(&self, p: &Package) -> Command {
        let (cmd, _) = self.base(p);
        match p.runner {
            Runner::Vitest => cmd.args(["run".into(), "--includeTaskLocation".into(), "--reporter=default".into(), format!("--reporter={}", Self::reporter(p.runner))]),
            Runner::Jest => cmd.args(["--ci".into(), "--watchAll=false".into(), "--testLocationInResults".into(), "--reporters=default".into(), format!("--reporters={}", Self::reporter(p.runner))]),
        }
    }

    /// `cmd` with `files` (below the project) and `-t regex`.
    pub fn naming(p: &Package, cmd: Command, files: &[&str], regex: Option<&str>) -> Command {
        let mut unique = std::collections::HashSet::new();
        let seen: Vec<&str> = files.iter().copied().filter(|f| unique.insert(*f)).collect();
        match p.runner {
            Runner::Vitest => {
                let cmd = cmd.args(seen.iter().map(|f| p.below(f).to_string()));
                match regex {
                    Some(re) => cmd.args(["-t", re]),
                    None => cmd,
                }
            }
            Runner::Jest => {
                let cmd = match regex {
                    Some(re) => cmd.args(["-t", re]),
                    None => cmd,
                };
                if seen.is_empty() {
                    cmd
                } else {
                    cmd.arg("--").args(seen.iter().map(|f| format!("/{}$", escape(p.below(f)))))
                }
            }
        }
    }

    fn label(&self, p: &Package) -> String {
        if self.layout().grouped() {
            p.label()
        } else {
            format!("{} tests", p.runner.name())
        }
    }

    pub fn discover(&self) -> Vec<Step> {
        self.layout().packages.iter().map(|p| Step::new(self.listing(p), format!("{}: listing", self.label(p)))).collect()
    }

    pub fn run_all(&self) -> Vec<Step> {
        let grouped = self.layout().grouped();
        self.layout()
            .packages
            .iter()
            .map(|p| {
                let step = Step::new(self.running(p), self.label(p));
                if grouped {
                    step.covers([Covered::Group(vec![p.label()])])
                } else {
                    step
                }
            })
            .collect()
    }

    fn test(&self, work: &mut BTreeMap<usize, Work>, f: &Found, cover: Option<Covered>) {
        let Some((file, names, template)) = test_of(f, self.index) else { return };
        let Some(pkg) = self.layout().package_of(&file) else { return };
        let runner = self.layout().packages[pkg].runner;
        work.entry(pkg).or_default().named.push(Named { file, alternative: alternative(&names, template, runner), cover });
    }

    fn add(&self, work: &mut BTreeMap<usize, Work>, item: &Asked) {
        match item {
            Asked::Test(f) => self.test(work, f, Some(Covered::Test(f.key.clone()))),
            Asked::Group { groups, tests, .. } => {
                let labels: Vec<String> = groups.iter().map(|g| g.label.clone()).collect();
                let Some(at) = groups.iter().position(|g| g.kind == Kind::File) else {
                    // A package.
                    if let Some(pkg) = groups.first().and_then(|g| self.layout().packages.iter().position(|p| p.label() == g.label)) {
                        work.entry(pkg).or_default().whole.push(Covered::Group(labels));
                    }
                    return;
                };
                let file = file_of(&groups[at]);
                let Some(pkg) = self.layout().package_of(&file) else { return };
                if at + 1 == groups.len() {
                    work.entry(pkg).or_default().files.push((file, Covered::Group(labels)));
                    return;
                }
                work.entry(pkg).or_default().groups.push(Covered::Group(labels));
                match tests {
                    // Its fill-in, whole: each test by its names, a template
                    // by its title (its cases), a case of one asked left to it.
                    Some(tests) => {
                        let templates: Vec<&str> = tests.iter().filter(|t| t.template).map(|t| t.key.0.as_str()).collect();
                        for t in tests {
                            if t.kind == Kind::Case && t.of.as_ref().is_some_and(|of| templates.contains(&of.0.as_str())) {
                                continue;
                            }
                            self.test(work, t, Some(Covered::Test(t.key.clone())));
                        }
                    }
                    // None: everything whose names start with its.
                    None => {
                        let runner = self.layout().packages[pkg].runner;
                        let names: Vec<String> = groups[at + 1..].iter().map(|g| g.label.clone()).collect();
                        let start = alternative(&names, false, runner);
                        work.entry(pkg).or_default().named.push(Named { file, alternative: format!("{start}{}.*", escape(runner.joiner())), cover: None });
                    }
                }
            }
        }
    }

    pub fn run(&self, items: &[Asked]) -> Vec<Step> {
        let mut work: BTreeMap<usize, Work> = BTreeMap::new();
        for item in items {
            self.add(&mut work, item);
        }
        let mut steps = Vec::new();
        for (pkg, mut w) in work {
            let p = &self.layout().packages[pkg];
            let label = self.label(p);
            let first = steps.len();
            if !w.whole.is_empty() {
                // All of it: what else was asked in it is covered too.
                let mut covers = w.whole;
                covers.extend(w.files.into_iter().map(|(_, c)| c));
                covers.extend(w.named.into_iter().filter_map(|n| n.cover));
                steps.push(Step::new(self.running(p), label.clone()).covers(covers));
            } else {
                let whole: Vec<String> = w.files.iter().map(|(f, _)| f.clone()).collect();
                let (inside, rest): (Vec<Named>, Vec<Named>) = std::mem::take(&mut w.named).into_iter().partition(|n| whole.contains(&n.file));
                if !w.files.is_empty() {
                    let mut covers: Vec<Covered> = w.files.iter().map(|(_, c)| c.clone()).collect();
                    covers.extend(inside.into_iter().filter_map(|n| n.cover));
                    let files: Vec<&str> = whole.iter().map(String::as_str).collect();
                    steps.push(Step::new(Self::naming(p, self.running(p), &files, None), label.clone()).covers(covers));
                }
                steps.extend(self.named(p, &rest, &label));
            }
            if steps.len() > first {
                steps.last_mut().expect("one").covers.extend(w.groups);
            }
        }
        steps
    }

    /// Steps naming `named` with `-t`, split under the limit: lang-kit's
    /// `split`, given them a batch at a time (256, or 8 KiB of names; 2
    /// KiB under `cmd.exe`'s 8,191), so it prices a few dozen commands, not
    /// one per name.
    fn named(&self, p: &Package, named: &[Named], label: &str) -> Vec<Step> {
        if named.is_empty() {
            return Vec::new();
        }
        let (_, limit) = self.base(p);
        // A batch well under the limit: `split` never cuts one.
        let most = if limit.batch_file { 2048 } else { 8192 };
        let mut batches: Vec<&[Named]> = Vec::new();
        let (mut start, mut bytes) = (0, 0);
        for (i, n) in named.iter().enumerate() {
            if i > start && (bytes + n.alternative.len() > most || i - start == 256) {
                batches.push(&named[start..i]);
                start = i;
                bytes = 0;
            }
            bytes += n.alternative.len() + n.file.len() + 2;
        }
        batches.push(&named[start..]);
        let make = |some: &[&[Named]]| {
            let all: Vec<&Named> = some.iter().flat_map(|b| b.iter()).collect();
            let files: Vec<&str> = all.iter().map(|n| n.file.as_str()).collect();
            let mut seen = std::collections::HashSet::new();
            let alternatives: Vec<&str> = all.iter().map(|n| n.alternative.as_str()).filter(|a| seen.insert(*a)).collect();
            Self::naming(p, self.running(p), &files, Some(&whole(&alternatives)))
        };
        // Which batches each command holds: the slice `split` made it of,
        // known by where the slice starts in `batches` and the command's
        // fingerprint (making each again to compare would be quadratic).
        let made: RefCell<Vec<(usize, usize, u64)>> = RefCell::new(Vec::new());
        let base = batches.as_ptr() as usize;
        let size = std::mem::size_of::<&[Named]>().max(1);
        let commands = split(&batches, limit, |some| {
            let cmd = make(some);
            if !some.is_empty() {
                made.borrow_mut().push(((some.as_ptr() as usize).wrapping_sub(base) / size, some.len(), fingerprint(&cmd)));
            }
            cmd
        });
        let made = made.into_inner();
        let mut out = Vec::new();
        let mut at = 0;
        for cmd in commands {
            let print = fingerprint(&cmd);
            let n = made.iter().rev().find(|(start, _, f)| *start == at && *f == print).map_or(batches.len() - at, |(_, n, _)| *n);
            let covers: Vec<Covered> = batches[at..(at + n).min(batches.len())].iter().flat_map(|b| b.iter()).filter_map(|x| x.cover.clone()).collect();
            at += n;
            out.push(Step::new(cmd, label).covers(covers));
        }
        out
    }
}

/// A command's args, hashed.
fn fingerprint(cmd: &Command) -> u64 {
    let mut bytes = Vec::with_capacity(cmd.args.iter().map(|a| a.len() + 1).sum());
    for a in &cmd.args {
        bytes.extend_from_slice(a.as_bytes());
        bytes.push(0);
    }
    fnv1a_64(&bytes)
}

/// A file group's path below the project.
fn file_of(g: &Group) -> String {
    g.file.clone().unwrap_or_else(|| g.label.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_regexes_their_characters_escaped() {
        let n = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(alternative(&n(&["outer", "inner", "deep"]), false, Runner::Vitest), "outer > inner > deep");
        assert_eq!(alternative(&n(&["outer", "inner", "deep"]), false, Runner::Jest), "outer inner deep");
        assert_eq!(alternative(&n(&["add(-1, -1) [regex] $x"]), false, Runner::Jest), "add\\(-1, -1\\) \\[regex\\] \\$x");
        assert_eq!(alternative(&n(&["add(%i, %i) is %i"]), true, Runner::Vitest), "add\\(.*, .*\\) is .*");
        assert_eq!(alternative(&n(&["names", "adds ${n}"]), true, Runner::Vitest), "names > adds .*");
        assert_eq!(whole(&["a", "b"]), "^(?:a|b)$");
    }
}
