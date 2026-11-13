//! The test provider for Fenix 1.6: `cargo test`, or `cargo nextest
//! run` when cargo-nextest 0.9.103 or later is installed, as lang-kit's
//! [`TestRunner`] (`plans/test-explorer.md`, §6.1).
//!
//! | | |
//! |---|---|
//! | Ids | `<package>/<target> <path>`: `orbit/lib tests::adds`, `orbit/test:it integration_fails`, `orbit-cli/bin:orbit-cli tests::sums::case_1`, `orbit/doc src/lib.rs - add (line 6)` |
//! | Groups | the target (`orbit (lib)`, `orbit (test it)`, `orbit (doc)`), its file (`src/lib.rs`), each inline `mod` (`tests`) |
//! | Scan ([`scan`]) | `#[test]` and the like at their `fn`'s line, a line at a time; nothing run |
//! | Listing | `cargo test --workspace -- --list` after a build: what a macro made, rstest's cases, doc tests at their line |
//! | Plans ([`plan`]) | a step per package: exact names when Fenix sends a file's tests, the module path otherwise, doc tests apart |
//! | Readers ([`cargo`], [`nextest`]) | libtest's lines and cargo's JSON, paired by binary; nextest's libtest JSON |
//! | Launch | a build-first plan (`cargo test --no-run`), then the test binary under lldb-dap or CodeLLDB with `<path> --exact --nocapture` |
//!
//! Built and tested on `main`, against lang-kit's copy of API 1.6's
//! types; Fenix calls it once the pack is on API 1.6 (the `api-1.6`
//! branch adds its manifest section and the five exports).

pub mod cargo;
pub mod index;
pub mod layout;
pub mod nextest;
pub mod plan;
pub mod read;
pub mod scan;

#[cfg(test)]
mod drive;

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::LaunchConfig;
use fenix_lang_kit::tests::{Ask, Covered, Found, Group, Launching, Reader, Step, TestRunner};
use fenix_lang_kit::Version;

use crate::project::Metadata;
use index::{Index, Shared};
use layout::{parse_key, place_in, Layout, Manifests};
use read::{Ctx, Purpose};

/// The manifest's `[[contributes.test_providers]].id`.
pub const PROVIDER: &str = "rust";

/// The first cargo-nextest whose `libtest-json-plus` streams each result
/// as it ends (its CHANGELOG, 0.9.103); before it, `cargo test` is used.
pub const NEXTEST_STREAMS: &str = "0.9.103";

/// What the pack knows that its provider needs, kept up to date by the
/// pack ([`crate::Rust`]) and read by the provider when Fenix asks.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// A Cargo.toml at the project's top.
    pub cargo_project: bool,
    /// cargo, by its full path when found.
    pub cargo: Option<String>,
    /// cargo-nextest's version, once `cargo nextest --version` said it.
    pub nextest: Option<String>,
    pub metadata: Option<Metadata>,
    /// The project's folder as cargo has it (the package the project is,
    /// else the workspace's root).
    pub project_dir: Option<String>,
    /// The project's folder on this machine: `/project`, a test's own.
    pub root: String,
    /// `rustc --print sysroot`, for lldb's Rust formatters.
    pub sysroot: Option<String>,
    /// Counted up each time `metadata` or `project_dir` changes.
    pub generation: u64,
}

pub type SharedFacts = Rc<RefCell<Facts>>;

impl Facts {
    /// Whether cargo-nextest runs the tests: installed, and new enough.
    pub fn nextest(&self) -> Option<&str> {
        let v = self.nextest.as_deref()?;
        (Version::parse(v)? >= Version::parse(NEXTEST_STREAMS)?).then_some(v)
    }
}

/// `cargo-nextest 0.9.151 (6b3f99457 2026-11-02)`'s version.
pub fn nextest_version(out: &str) -> Option<String> {
    let first = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let v = first.strip_prefix("cargo-nextest ")?.split(' ').next()?;
    Version::parse(v).map(|_| v.to_string())
}

/// cargo test, as a test provider.
pub struct Provider {
    facts: SharedFacts,
    index: Shared,
    /// The layout, for the facts' generation it was made from.
    layout: Option<(u64, Rc<Layout>)>,
    /// Cargo.toml files read before cargo answered, and the facts'
    /// generation they were read in: read again after it changes.
    manifests: Manifests,
    manifests_generation: u64,
}

impl Provider {
    pub fn new(facts: SharedFacts) -> Provider {
        Provider { facts, index: Rc::new(RefCell::new(Index::default())), layout: None, manifests: Manifests::default(), manifests_generation: 0 }
    }

    /// The project's packages: cargo's, else what its Cargo.toml files say.
    fn layout(&mut self) -> Rc<Layout> {
        let facts = self.facts.borrow();
        let generation = facts.generation;
        if let (Some((g, l)), true) = (&self.layout, facts.metadata.is_some()) {
            if *g == generation {
                return l.clone();
            }
        }
        let layout = match (&facts.metadata, &facts.project_dir) {
            (Some(m), Some(dir)) => Rc::new(Layout::from_metadata(m, dir)),
            _ => {
                Rc::new(Layout { packages: self.manifests.known(), ..Layout::default() })
            }
        };
        if facts.metadata.is_some() {
            self.layout = Some((generation, layout.clone()));
        }
        layout
    }

    fn planner<'a>(&'a self, facts: &'a Facts, layout: &'a Layout, index: &'a Index) -> plan::Planner<'a> {
        plan::Planner { cargo: facts.cargo.as_deref().unwrap_or("cargo"), layout, index, nextest: facts.nextest().is_some() }
    }
}

/// The package a step names with `-p`.
fn step_package(step: &Step) -> Option<String> {
    let args = &step.command.args;
    args.iter().position(|a| a == "-p").and_then(|i| args.get(i + 1)).cloned()
}

impl TestRunner for Provider {
    fn provider(&self) -> &str {
        PROVIDER
    }

    fn runner(&mut self) -> (String, Option<String>) {
        let f = self.facts.borrow();
        if !f.cargo_project {
            return (String::new(), None);
        }
        if f.cargo.is_none() {
            return ("cargo test".into(), Some("cargo isn't installed: rustup installs it with Rust (rustup.rs)".into()));
        }
        match f.nextest() {
            Some(v) => (format!("cargo nextest {v}"), None),
            None => ("cargo test".into(), None),
        }
    }

    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path).to_string();
        let layout = self.layout();
        let package = match layout.package_of(&file) {
            Some(p) if !layout.project.is_empty() => Some(p.clone()),
            _ => {
                let (root, generation) = {
                    let f = self.facts.borrow();
                    (f.root.clone(), f.generation)
                };
                if generation != self.manifests_generation {
                    self.manifests.forget();
                    self.manifests_generation = generation;
                }
                self.manifests.package_of(&root, &file)
            }
        };
        let place = package.as_ref().and_then(|p| place_in(p, if p.dir.is_empty() { &file } else { file.strip_prefix(&format!("{}/", p.dir)).unwrap_or(&file) }));
        let Some(place) = place else {
            self.index.borrow_mut().scanned(&file, None, Vec::new(), Vec::new());
            return Vec::new();
        };
        let found = scan::scan(text);
        let target = place.target;
        let base = vec![target.group(), Group::file(file.clone())];
        let prefix = place.module.join("::");
        let full = |p: &[String]| -> String {
            let inner = p.join("::");
            match (prefix.is_empty(), inner.is_empty()) {
                (true, _) => inner,
                (false, true) => prefix.clone(),
                (false, false) => format!("{prefix}::{inner}"),
            }
        };
        // Each module's groups: the file's, then its inline modules'.
        let mut modules: Vec<(Vec<String>, Vec<Group>)> = vec![(Vec::new(), base.clone())];
        for m in &found.modules {
            let parent = modules.iter().find(|(p, _)| p[..] == m.path[..m.path.len() - 1]).map(|(_, g)| g.clone()).unwrap_or_else(|| base.clone());
            let mut groups = parent;
            groups.push(Group::suite(m.path.last().cloned().unwrap_or_default(), file.clone(), m.line).lines(m.line, m.end_line));
            modules.push((m.path.clone(), groups));
        }
        let mut out = Vec::with_capacity(found.tests.len());
        let mut tests = Vec::with_capacity(found.tests.len());
        for t in &found.tests {
            let path = full(&t.path);
            let parent = &t.path[..t.path.len() - 1];
            let groups = modules.iter().find(|(p, _)| p[..] == *parent).map(|(_, g)| g.clone()).unwrap_or_else(|| base.clone());
            let mut f = Found::test(target.key(&path), t.path.last().cloned().unwrap_or_default()).under(groups).line(t.line);
            if let Some(end) = t.end_line {
                f = f.end_line(end);
            }
            if let Some(why) = &t.skip {
                f = f.skip(why.clone());
            }
            if t.template {
                f = f.template();
            }
            tests.push((path, t.template));
            out.push(f);
        }
        let modules = modules.into_iter().map(|(p, g)| (full(&p), g)).collect();
        self.index.borrow_mut().scanned(&file, Some(&target.id()), tests, modules);
        out
    }

    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        if let (_, Some(problem)) = self.runner() {
            return Err(problem);
        }
        let layout = self.layout();
        if *ask == Ask::Discover {
            self.index.borrow_mut().forget_listed();
        }
        let facts = self.facts.borrow();
        let index = self.index.borrow();
        Ok(plan::plan(&self.planner(&facts, &layout, &index), ask))
    }

    fn list_after(&mut self, ask: &Ask) -> bool {
        *ask == Ask::RunAll
    }

    fn reader(&mut self, ask: &Ask, step: &Step) -> Box<dyn Reader> {
        let purpose = match ask {
            Ask::Discover => Purpose::Listing,
            Ask::Debug(_) => Purpose::Build,
            _ => Purpose::Run,
        };
        // The tests a step named exactly: all it covers, when it names
        // no group.
        let exact = (!step.covers.is_empty() && step.covers.iter().all(|c| matches!(c, Covered::Test(_)))).then(|| {
            step.covers
                .iter()
                .filter_map(|c| match c {
                    Covered::Test(k) => parse_key(&k.0).map(|(t, p)| (t.id(), p.to_string())),
                    Covered::Group(_) => None,
                })
                .collect()
        });
        let ctx = Rc::new(Ctx { layout: (*self.layout()).clone(), index: self.index.clone(), purpose, package: step_package(step), exact });
        if step.command.args.first().map(String::as_str) == Some("nextest") {
            Box::new(nextest::NextestReader::new(ctx))
        } else {
            Box::new(cargo::CargoReader::new(ctx))
        }
    }

    fn launch(&mut self, test: &Found, kept: &[(String, String)]) -> Result<Launching, String> {
        let (target, path) = parse_key(&test.key.0).ok_or_else(|| format!("{} isn't a test cargo can name", test.key))?;
        if target.is_doc() {
            return Err("a doc test can't be debugged on its own: rustdoc builds and runs it in one go".into());
        }
        let layout = self.layout();
        if kept.is_empty() {
            let facts = self.facts.borrow();
            let index = self.index.borrow();
            return Ok(Launching::BuildFirst(vec![self.planner(&facts, &layout, &index).build(&target)]));
        }
        let want = format!("exe {}", target.id());
        let exe = kept.iter().find(|(name, _)| *name == want).map(|(_, v)| v.clone()).ok_or_else(|| format!("cargo built no test binary for {}", target.label()))?;
        let template = test.template || self.index.borrow().is_template(&target.id(), path);
        // cargo runs a test binary in its package's folder, with
        // CARGO_MANIFEST_DIR set: a test reading its fixtures by either
        // finds them under the debugger too.
        let dir = layout.package(&target.package).map(|p| p.abs.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| "${project}".into());
        let sysroot = self.facts.borrow().sysroot.clone();
        let mut config = LaunchConfig::launch(&format!("Rust: {path}"), &["rust"]).program(&exe).arg(path);
        // A template's cases are `<path>::case_1`: by name, not exactly.
        if !template {
            config = config.arg("--exact");
        }
        config = config.arg("--nocapture").cwd(&dir).env("CARGO_MANIFEST_DIR", &dir).attributes(&crate::debug::attributes(sysroot.as_deref()));
        Ok(Launching::Now(config))
    }
}
