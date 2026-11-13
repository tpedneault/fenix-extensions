//! The test provider for Fenix 1.6: Vitest or Jest, whichever each of the
//! project's packages uses, as lang-kit's [`TestRunner`]
//! (`plans/test-explorer.md`, §6.4: one provider, `web`, D-W1).
//!
//! | | |
//! |---|---|
//! | Runner | per package folder (the project's, each workspace package's), from its `package.json` and config files ([`layout`]); Fenix is told `Vitest 5.1.0`, `Jest 30.5.2`, `Vitest 5.1.0 and Jest 30.5.2` |
//! | Ids | `<file below the project> > <names joined by " > ">`: `src/orbit.test.js > outer > inner > deep` |
//! | Groups | the package (with two or more), the file, each `describe` |
//! | Scan ([`scan`]) | `describe`, `it`, `test` and their `.skip`, `.todo`, `.each` at their lines, token by token; nothing run |
//! | Listing | a run whose `-t` matches nothing, through Fenix's reporter: every test, `test.each`'s cases, titles that aren't literals |
//! | Plans ([`plan`]) | a step per package, by Node: files, and names as a `-t` regex |
//! | Readers ([`vitest`], [`jest`]) | the reporters' `FENIX-TEST {json}` lines ([`assets`](Provider::assets)) |
//! | Launch | at once: the runner's script under js-debug with the test's file and `-t`, as the pack's launch files run it |
//!
//! Built and tested on `main`, against lang-kit's copy of API 1.6's
//! types; Fenix calls it once the pack is on API 1.6 (the `api-1.6`
//! branch adds its manifest section and the five exports).

pub mod index;
pub mod jest;
pub mod layout;
pub mod plan;
pub mod read;
pub mod scan;
pub mod vitest;

#[cfg(test)]
mod drive;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fenix_extension_api::serde_json::json;
use fenix_extension_api::LaunchConfig;
use fenix_lang_kit::tests::{Ask, Covered, Found, Group, Launching, Reader, Step, TestRunner};
use fenix_lang_kit::Version;

use index::{Entry, Index, Shared};
use layout::{is_test_file, key, names_data, nth, Layout, Runner};
use read::{Ctx, Purpose, ReporterReader, Selection};

/// The manifest's `[[contributes.test_providers]].id`.
pub const PROVIDER: &str = "web";

/// The first Vitest whose reporter API Fenix's reporter uses
/// (`onTestCaseResult`, `onTestModuleCollected`).
pub const VITEST_REPORTERS: &str = "3.0.0";

/// The reporters, written to `/data/lang-kit/tests/` as the pack starts.
pub const VITEST_REPORTER: &[u8] = include_bytes!("../tests/fenix-vitest.mjs");
pub const JEST_REPORTER: &[u8] = include_bytes!("../tests/fenix-jest.js");

/// What the pack knows that its provider needs, kept up to date by the
/// pack ([`crate::Web`]) and read by the provider when Fenix asks.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub layout: Layout,
    /// Node and Yarn, by their full paths when found.
    pub node: Option<String>,
    pub yarn: Option<String>,
}

pub type SharedFacts = Rc<RefCell<Facts>>;

/// Vitest or Jest, as a test provider.
pub struct Provider {
    facts: SharedFacts,
    index: Shared,
}

impl Provider {
    pub fn new(facts: SharedFacts) -> Provider {
        Provider { facts, index: Rc::new(RefCell::new(Index::default())) }
    }
}

/// Why the project's tests can't run, when they can't.
fn problem(f: &Facts) -> Option<String> {
    let l = &f.layout;
    if l.pnp {
        return f.yarn.is_none().then(|| "Yarn isn't installed: a Yarn Plug'n'Play project's tests run with `yarn vitest` or `yarn jest`".to_string());
    }
    if let Some(p) = l.packages.iter().find(|p| p.installed.is_none()) {
        let place = if p.dir.is_empty() { String::new() } else { format!(" in {}", p.dir) };
        return Some(format!("{} isn't installed{place}: install the project's packages (Web: install the project's packages)", p.runner.name()));
    }
    let old = l.packages.iter().find_map(|p| {
        let v = p.installed.as_ref()?.version.as_deref()?;
        (p.runner == Runner::Vitest && Version::parse(v)? < Version::parse(VITEST_REPORTERS)?).then_some(v)
    });
    if let Some(v) = old {
        return Some(format!("Vitest {v} is older than {VITEST_REPORTERS}, whose reporter API Fenix's reporter needs: upgrade it to run its tests here"));
    }
    f.node.is_none().then(|| "Node isn't installed, or isn't where Fenix looks: install it, or set web.node".to_string())
}

/// A step's selection: what it names with `-t`, from what it covers.
fn selection(step: &Step, index: &Index, layout: &Layout) -> Option<Selection> {
    if !step.command.args.iter().any(|a| a == "-t") {
        return None;
    }
    let mut s = Selection::default();
    // A group's labels: its package's (when grouped), its file's, its
    // describes'.
    let file_at = usize::from(layout.grouped());
    for c in &step.covers {
        match c {
            Covered::Test(k) => {
                if index.is_template(&k.0) {
                    if let Some((file, names)) = layout::parse_key(&k.0) {
                        s.templates.push((file.to_string(), index.names_of(&k.0).unwrap_or(names)));
                    }
                }
                s.exact.insert(k.0.clone());
            }
            Covered::Group(labels) => {
                if let Some(file) = labels.get(file_at) {
                    s.prefixes.push(format!("{} > ", key(file, &labels[file_at + 1..])));
                }
            }
        }
    }
    Some(s)
}

/// `dir` and `rel` (which may climb, `../../node_modules/..`) as one path.
fn joined(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

impl TestRunner for Provider {
    fn provider(&self) -> &str {
        PROVIDER
    }

    fn runner(&mut self) -> (String, Option<String>) {
        let f = self.facts.borrow();
        if f.layout.packages.is_empty() {
            return (String::new(), None);
        }
        let mut name = f.layout.runners();
        if f.layout.pnp {
            name.push_str(" (Yarn Plug'n'Play)");
        }
        (name, problem(&f))
    }

    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path).to_string();
        let facts = self.facts.borrow();
        let layout = &facts.layout;
        let Some(pkg) = layout.package_of(&file).filter(|&p| is_test_file(&file, layout.packages[p].runner)) else {
            self.index.borrow_mut().scanned(&file, Vec::new(), HashMap::new());
            return Vec::new();
        };
        let scanned = scan::scan(text);
        let mut base = layout.base(pkg);
        base.push(Group::file(file.clone()));
        let mut suites: HashMap<Vec<String>, Vec<Group>> = HashMap::new();
        for s in &scanned.suites {
            let parent = suites.get(&s.names[..s.names.len() - 1]).cloned().unwrap_or_else(|| base.clone());
            let mut groups = parent;
            groups.push(Group::suite(s.names.last().cloned().unwrap_or_default(), file.clone(), s.line).lines(s.line, s.end_line));
            suites.entry(s.names.clone()).or_insert(groups);
        }
        let mut seen: HashMap<&[String], usize> = HashMap::new();
        let mut out = Vec::with_capacity(scanned.tests.len());
        let mut entries = Vec::with_capacity(scanned.tests.len());
        for t in &scanned.tests {
            let k = {
                let n = seen.entry(&t.names).or_insert(0);
                *n += 1;
                *n
            };
            let (title, parents) = t.names.split_last().expect("named");
            let label = nth(title, k);
            let mut key = key(&file, parents);
            key.push_str(" > ");
            key.push_str(&label);
            let groups = if parents.is_empty() { base.clone() } else { suites.get(parents).cloned().unwrap_or_else(|| base.clone()) };
            let mut f = Found::test(key.clone(), label).under(groups).line(t.line);
            if let Some(end) = t.end_line {
                f = f.end_line(end);
            }
            if let Some(why) = &t.skip {
                f = f.skip(why.clone());
            }
            if t.template {
                f = f.template();
            }
            if k > 1 || t.names.iter().any(|n| n.contains(" > ")) {
                f = f.data(names_data(&t.names));
            }
            // A template's groups, for its cases.
            let groups = if t.template { f.groups.clone() } else { Vec::new() };
            entries.push(Entry { key, names: t.names.clone(), line: t.line, end_line: t.end_line, template: t.template, groups });
            out.push(f);
        }
        drop(facts);
        self.index.borrow_mut().scanned(&file, entries, suites);
        out
    }

    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        if let (_, Some(problem)) = self.runner() {
            return Err(problem);
        }
        if *ask == Ask::Discover {
            self.index.borrow_mut().forget_listed();
        }
        let facts = self.facts.borrow();
        let index = self.index.borrow();
        Ok(plan::plan(&plan::Planner { facts: &facts, index: &index }, ask))
    }

    fn reader(&mut self, ask: &Ask, step: &Step) -> Box<dyn Reader> {
        let facts = self.facts.borrow();
        let package = step.command.cwd.as_deref().and_then(|c| facts.layout.package_at(c));
        let purpose = if *ask == Ask::Discover { Purpose::Listing } else { Purpose::Run };
        let select = selection(step, &self.index.borrow(), &facts.layout);
        let runner = match package.map(|p| facts.layout.packages[p].runner) {
            Some(r) => r,
            // A step this start didn't plan: by its command.
            None if step.command.args.iter().any(|a| a.contains("jest")) => Runner::Jest,
            None => Runner::Vitest,
        };
        let ctx = Ctx { layout: facts.layout.clone(), package, index: self.index.clone(), purpose, select };
        let read = read::Read::new(ctx);
        match runner {
            Runner::Vitest => Box::new(ReporterReader { read, format: vitest::Vitest::default(), runner: "Vitest" }),
            Runner::Jest => Box::new(ReporterReader { read, format: jest::Jest, runner: "Jest" }),
        }
    }

    fn launch(&mut self, test: &Found, _kept: &[(String, String)]) -> Result<Launching, String> {
        let facts = self.facts.borrow();
        let index = self.index.borrow();
        let (file, names, template) = plan::test_of(test, &index).ok_or_else(|| format!("{} isn't a test Vitest or Jest can name", test.key))?;
        let pkg = facts.layout.package_of(&file).ok_or_else(|| format!("{file} isn't in a package that runs Vitest or Jest"))?;
        let p = &facts.layout.packages[pkg];
        let regex = plan::whole(&[&plan::alternative(&names, template, p.runner)]);
        let rel = p.below(&file).to_string();
        let args: Vec<String> = match p.runner {
            Runner::Vitest => vec!["run".into(), rel, "-t".into(), regex],
            Runner::Jest => vec!["--runInBand".into(), "--watchAll=false".into(), "-t".into(), regex, "--".into(), format!("/{}$", plan::escape(&rel))],
        };
        let cwd = if p.dir.is_empty() { "${project}".to_string() } else { format!("${{project}}/{}", p.dir) };
        let mut attributes = json!({ "type": "pwa-node", "skipFiles": ["<node_internals>/**", "**/node_modules/**"] });
        if p.runner == Runner::Vitest {
            attributes["autoAttachChildProcesses"] = json!(true);
            attributes["smartStep"] = json!(true);
        }
        let mut config = LaunchConfig::launch(&format!("{}: {}", p.runner.name(), names.join(" > ")), crate::debug::LANGUAGES);
        if facts.layout.pnp {
            // No node_modules: Yarn runs it.
            attributes["runtimeExecutable"] = json!(facts.yarn.as_deref().unwrap_or("yarn"));
            attributes["runtimeArgs"] = json!(std::iter::once(p.runner.id().to_string()).chain(args).collect::<Vec<_>>());
        } else {
            let script = p.installed.as_ref().map(|i| i.script.clone()).unwrap_or_else(|| format!("node_modules/{}", p.runner.script()));
            config = config.program(&format!("${{project}}/{}", joined(&p.dir, &script)));
            for a in args {
                config = config.arg(a);
            }
            if let Some(node) = &facts.node {
                attributes["runtimeExecutable"] = json!(node);
            }
        }
        Ok(Launching::Now(config.cwd(&cwd).attributes(&attributes)))
    }

    fn assets(&self) -> Vec<(&str, &[u8])> {
        vec![("fenix-vitest.mjs", VITEST_REPORTER), ("fenix-jest.js", JEST_REPORTER)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launchs_program_climbs_to_a_hoisted_script() {
        assert_eq!(joined("packages/app", "../../node_modules/vitest/vitest.mjs"), "node_modules/vitest/vitest.mjs");
        assert_eq!(joined("", "node_modules/jest/bin/jest.js"), "node_modules/jest/bin/jest.js");
    }
}
