//! The test provider for Fenix 1.6: `go test`, as lang-kit's
//! [`TestRunner`] (`plans/test-explorer.md`, §6.3).
//!
//! | | |
//! |---|---|
//! | Ids | `<import path> <name>`: `example.com/orbit TestAdds`, `example.com/orbit TestTable/negative_one` (a subtest as `go test` names it, spaces as `_`) |
//! | Groups | the package (`example.com/orbit`), its file (`orbit_test.go`, below the project) |
//! | Scan ([`scan`]) | `TestXxx`, `FuzzXxx` and `ExampleXxx` (with an `// Output:`) at their `func`'s line, a test that calls `t.Run` a template; nothing run |
//! | Listing ([`golist`]) | `go list -e`: the test files Go would build, without building; those the scan hasn't seen are read and scanned |
//! | Plans ([`plan`]) | `go test -json -fullpath`, by import path and anchored `-run` regexes, a level to each `/`; per module, or from a `go.work`'s root |
//! | Reader ([`gotest`]) | the `-json` events by package and test: results, subtests found as they run, a build failure `Broken`, a crash `Crashed` |
//! | Launch | Delve's test mode on the package, `-test.run` the test, at once |
//!
//! Built and tested on `main`, against lang-kit's copy of API 1.6's
//! types; Fenix calls it once the pack is on API 1.6 (the `api-1.6`
//! branch adds its manifest section and the five exports).

pub mod golist;
pub mod gotest;
pub mod index;
pub mod layout;
pub mod plan;
pub mod read;
pub mod scan;

#[cfg(test)]
mod drive;

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::LaunchConfig;
use fenix_lang_kit::serde_json::json;
use fenix_lang_kit::tests::{Ask, Found, Group, Launching, Reader, Step, TestRunner};

use index::{Index, Known, Shared};
use layout::{ignored, package_group, parse_key, project_dir, quote_meta, Layout};
use read::Ctx;

/// The manifest's `[[contributes.test_providers]].id`.
pub const PROVIDER: &str = "go";

/// What the pack knows that its provider needs, kept up to date by the
/// pack ([`crate::Go`]) and read by the provider when Fenix asks.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The project's folder on this machine: `/project`, a test's own.
    pub root: String,
    /// `go`, by its full path when found.
    pub go: Option<String>,
    /// Go's version, once the doctor has asked (`1.24.7`).
    pub version: Option<String>,
    /// A `go.mod` at the project's top.
    pub module: bool,
    /// A `go.work` at the project's top.
    pub work: bool,
    /// `platform()`'s `os`: `linux`, `macos`, `windows`.
    pub os: String,
    /// Counted up each time `go.mod` or `go.work` changes.
    pub generation: u64,
}

pub type SharedFacts = Rc<RefCell<Facts>>;

/// `go test`, as a test provider.
pub struct Provider {
    facts: SharedFacts,
    index: Shared,
    /// The facts' generation the index's reading of the project's files is
    /// from.
    generation: u64,
}

impl Provider {
    pub fn new(facts: SharedFacts) -> Provider {
        let f = facts.borrow();
        let index = Rc::new(RefCell::new(Index::new(&f.root)));
        let generation = f.generation;
        drop(f);
        Provider { facts, index, generation }
    }

    /// What was read of `go.mod` and `go.work` is read again after either
    /// changed.
    fn fresh(&mut self) {
        let generation = self.facts.borrow().generation;
        let os = self.facts.borrow().os.clone();
        let mut index = self.index.borrow_mut();
        if generation != self.generation {
            self.generation = generation;
            index.forget();
        }
        if index.os != os {
            index.os = os;
        }
    }

    fn layout(&mut self) -> Layout {
        self.fresh();
        let mut index = self.index.borrow_mut();
        let seen = index.modules_seen();
        Layout::of(&mut index.modules, seen)
    }

    fn context(&mut self, step: &Step) -> Rc<Ctx> {
        self.fresh();
        let cwd = step.command.cwd.as_deref().unwrap_or("/project");
        let cwd = cwd.strip_prefix("/project").unwrap_or(cwd).trim_matches('/').to_string();
        Rc::new(Ctx { index: self.index.clone(), cwd, covers: step.covers.clone() })
    }
}

/// File `file`'s tests (below the project), as lang-kit's `Found`s, and
/// remembered in the index. Empty for a file `go test` doesn't read.
pub fn scan_file(index: &Shared, file: &str, text: &str) -> Vec<Found> {
    let mut index = index.borrow_mut();
    let (dir, base) = file.rsplit_once('/').unwrap_or(("", file));
    let module = (base.ends_with("_test.go") && !base.starts_with(['_', '.'])).then(|| index.modules.of_dir(dir)).flatten();
    let Some(module) = module else {
        index.unscan(file);
        return Vec::new();
    };
    let inside = if module.dir.is_empty() { dir } else { dir.strip_prefix(module.dir.as_str()).map_or(dir, |d| d.trim_start_matches('/')) };
    let Some(package) = module.package_of(dir).filter(|_| !ignored(inside)) else {
        index.unscan(file);
        return Vec::new();
    };
    let items = scan::scan(text);
    let why = scan::excluded(base, text, &index.os);
    let groups = vec![package_group(&package), Group::file(file)];
    let shared: Rc<str> = file.into();
    let excluded: Option<Rc<str>> = why.as_deref().map(Into::into);
    let mut found = Vec::with_capacity(items.len());
    let mut tests = Vec::with_capacity(items.len());
    for it in &items {
        let mut f = Found::test(layout::key(&package, &it.name), it.name.as_str()).under(groups.clone()).at(file, it.line);
        if let Some(end) = it.end_line {
            f = f.end_line(end);
        }
        if it.template {
            f = f.template();
        }
        if let Some(why) = &why {
            f = f.skip(why.as_str());
        }
        tests.push((it.name.clone(), Known { template: it.template, file: shared.clone(), line: it.line, excluded: excluded.clone() }));
        found.push(f);
    }
    index.scanned(file, &module, &package, dir, tests);
    found
}

impl TestRunner for Provider {
    fn provider(&self) -> &str {
        PROVIDER
    }

    fn runner(&mut self) -> (String, Option<String>) {
        let f = self.facts.borrow();
        if !f.module && !f.work {
            return (String::new(), None);
        }
        if f.go.is_none() {
            return ("go test".into(), Some("Go isn't installed: the doctor's fix row installs it with Homebrew or winget, else go.dev/dl".into()));
        }
        match &f.version {
            Some(v) => (format!("go test (go{v})"), None),
            None => ("go test".into(), None),
        }
    }

    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        self.fresh();
        scan_file(&self.index, path.strip_prefix("/project/").unwrap_or(path), text)
    }

    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        if let (_, Some(problem)) = self.runner() {
            return Err(problem);
        }
        let layout = self.layout();
        let go = self.facts.borrow().go.clone();
        let planner = plan::Planner { go: go.as_deref().unwrap_or("go"), layout: &layout };
        Ok(plan::plan(&planner, ask))
    }

    fn reader(&mut self, ask: &Ask, step: &Step) -> Box<dyn Reader> {
        let listing = *ask == Ask::Discover || step.command.args.first().map(String::as_str) == Some("list");
        if listing {
            Box::new(golist::ListReader::new(self.context(step)))
        } else {
            Box::new(gotest::GoTestReader::new(self.context(step)))
        }
    }

    fn launch(&mut self, test: &Found, _kept: &[(String, String)]) -> Result<Launching, String> {
        let (package, name) = parse_key(&test.key.0).ok_or_else(|| format!("{} isn't a test go can name", test.key))?;
        let layout = self.layout();
        let dir = layout.dir_of(package).or_else(|| self.index.borrow().package(package).map(|p| p.dir.clone())).ok_or_else(|| format!("{package} isn't in a module of this project"))?;
        let levels: Vec<String> = name.split('/').map(|l| format!("^{}$", quote_meta(l))).collect();
        let folder = project_dir(&dir);
        // Delve builds the package's test binary and runs it in its folder,
        // as `go test` does.
        let config = LaunchConfig::launch(&format!("Go: {name}"), &["go"]).program(&folder).arg("-test.run").arg(levels.join("/")).arg("-test.v").cwd(&folder).attributes(&json!({ "mode": "test" }));
        Ok(Launching::Now(config))
    }
}
