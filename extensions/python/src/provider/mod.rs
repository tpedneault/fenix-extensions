//! The test provider for Fenix 1.6: pytest, run through the plugin the
//! pack ships, as lang-kit's [`TestRunner`] (`plans/test-explorer.md`, §6.2).
//!
//! | | |
//! |---|---|
//! | Ids | pytest's node ids below the project: `tests/test_basic.py::test_cases[strings [odd] id]`, `tests/test_basic.py::TestOuter::TestInner::test_deep` |
//! | Groups | the file (`tests/test_basic.py`), each class (`TestOuter`) |
//! | Scan ([`scan`]) | `test*` functions and `Test*` or `unittest.TestCase` classes at their `def`'s line, `parametrize` as templates, `skip` marks as skips; nothing run, nothing imported |
//! | Listing | `pytest --collect-only` through the plugin: the cases of a `parametrize`, and what the scan can't see (it imports every test module) |
//! | Plans ([`plan`]) | a step for each rootdir: node ids as arguments, or in a file of ids |
//! | Reader ([`pytest`]) | the plugin's lines, one result for each test once its setup, call and teardown are in |
//! | Launch | debugpy's `pytest` module, `-q <node id>`, from the rootdir |
//!
//! Built and tested on `main`, against lang-kit's copy of API 1.6's
//! types; Fenix calls it once the pack is on API 1.6 (the `api-1.6`
//! branch adds its manifest section and the five exports).

pub mod index;
pub mod layout;
pub mod plan;
pub mod pytest;
pub mod read;
pub mod scan;

#[cfg(test)]
mod drive;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fenix_extension_api::serde_json::json;
use fenix_extension_api::LaunchConfig;
use fenix_lang_kit::tests::{is_cut_id, Ask, Found, Group, Launching, Reader, Step, TestRunner};
use fenix_lang_kit::Version;

use index::{Entry, Index, Shared};
use layout::{key, split_key, Layout};
use read::{Ctx, Purpose};

/// The manifest's `[[contributes.test_providers]].id`.
pub const PROVIDER: &str = "python";

/// What the pack knows that its provider needs, kept up to date by the
/// pack ([`crate::Python`]) and read by the provider when Fenix asks.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// uv, by its full path, when it runs the tests (`uv run ... python`).
    pub uv: Option<String>,
    /// uv brings pytest for the run: the project doesn't ask for it.
    pub with_pytest: bool,
    /// The environment's Python, by its full path when found.
    pub python: Option<String>,
    /// pytest's version: from `python -m pytest --version`, or the lock.
    pub pytest: Option<String>,
    /// pytest isn't in the Python that runs the tests: `python -m pytest`
    /// or a step said so.
    pub missing: bool,
    /// The project's folder on this machine: `/project`, a test's own.
    pub root: String,
    /// Counted up each time a file that says what pytest is changes.
    pub generation: u64,
}

pub type SharedFacts = Rc<RefCell<Facts>>;

/// `pytest 9.2.0`'s version, from what `pytest --version` printed (on
/// stdout since pytest 7, before it on stderr: `This is pytest version
/// 6.2.5, imported from ...`).
pub fn pytest_version(out: &str) -> Option<String> {
    for line in out.lines().map(str::trim) {
        let word = line.strip_prefix("pytest ").or_else(|| line.strip_prefix("This is pytest version ")).map(|r| r.split([' ', ',']).next().unwrap_or(r));
        if let Some(v) = word.filter(|v| Version::parse(v).is_some()) {
            return Some(v.to_string());
        }
    }
    None
}

/// pytest's version in a `uv.lock`: `[[package]]` with `name = "pytest"`.
pub fn locked_version(lock: &str) -> Option<String> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"pytest\"" {
            let v = lines.next()?.trim().strip_prefix("version = \"")?.strip_suffix('"')?;
            return Version::parse(v).map(|_| v.to_string());
        }
    }
    None
}

/// pytest, as a test provider.
pub struct Provider {
    facts: SharedFacts,
    index: Shared,
    /// What it read of the project's files, for the facts' generation.
    layout: Layout,
    generation: u64,
}

impl Provider {
    pub fn new(facts: SharedFacts) -> Provider {
        let layout = Layout::new(&facts.borrow().root);
        Provider { facts, index: Rc::new(RefCell::new(Index::default())), layout, generation: 0 }
    }

    /// The layout as the files say now: read again after one changed.
    fn refresh(&mut self) {
        let generation = self.facts.borrow().generation;
        if generation != self.generation {
            self.generation = generation;
            self.layout.forget();
        }
    }

    fn planner<'a>(&'a self, facts: &'a Facts, files: Vec<String>) -> plan::Planner<'a> {
        plan::Planner { facts, layout: &self.layout, files, env: plan::dotenv(&facts.root) }
    }

    /// A key's file's folder: the rootdir the key's node id is relative to.
    fn root_of(&self, k: &str) -> String {
        self.layout.root_of(split_key(k).0)
    }
}

impl TestRunner for Provider {
    fn provider(&self) -> &str {
        PROVIDER
    }

    fn runner(&mut self) -> (String, Option<String>) {
        let f = self.facts.borrow();
        let name = match &f.pytest {
            Some(v) => format!("pytest {v}"),
            None => "pytest".to_string(),
        };
        if f.uv.is_none() && f.python.is_none() {
            return (name, Some("Python isn't installed: pytest runs on it (python.org, or uv: docs.astral.sh/uv)".into()));
        }
        if f.missing && f.uv.is_none() {
            return (name, Some("pytest isn't installed in this environment: `pip install pytest`, or add it to the project's dependencies".into()));
        }
        (name, None)
    }

    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        self.refresh();
        let file = path.strip_prefix("/project/").unwrap_or(path).replace('\\', "/");
        let root = self.layout.root_of(&file);
        let rules = self.layout.rules(&root);
        let below = if root.is_empty() { file.as_str() } else { file.strip_prefix(&format!("{root}/")).unwrap_or(&file) };
        if !rules.file(below) {
            self.index.borrow_mut().scanned(&file, Vec::new(), Vec::new());
            return Vec::new();
        }
        let found = scan::scan(text, &rules);
        let file_group = Group::file(file.clone());
        // Each class's groups, root first: the file, then each class. The
        // tests of a class share them.
        let mut suites: Vec<(Vec<String>, Rc<Vec<Group>>)> = Vec::with_capacity(found.classes.len());
        let mut class_groups = Vec::with_capacity(found.classes.len());
        let top = Rc::new(vec![file_group]);
        for c in &found.classes {
            let parent = &c.path[..c.path.len() - 1];
            let mut groups: Vec<Group> = suites.iter().find(|(p, _)| p[..] == *parent).map(|(_, g)| g.to_vec()).unwrap_or_else(|| top.to_vec());
            let name = c.path.last().cloned().unwrap_or_default();
            let group = Group::suite(name, file.clone(), c.line).lines(c.line, Some(c.end_line));
            class_groups.push((format!("{file}::{}", c.path.join("::")), group.clone()));
            groups.push(group);
            suites.push((c.path.clone(), Rc::new(groups)));
        }
        let mut out: Vec<Option<Found>> = Vec::with_capacity(found.tests.len());
        let mut entries: Vec<Option<(String, Entry)>> = Vec::with_capacity(found.tests.len());
        let mut at: HashMap<String, usize> = HashMap::with_capacity(found.tests.len());
        for t in &found.tests {
            let parent = &t.path[..t.path.len() - 1];
            let groups = if parent.is_empty() { top.clone() } else { suites.iter().find(|(p, _)| p[..] == *parent).map(|(_, g)| g.clone()).unwrap_or_else(|| top.clone()) };
            let name = t.path.last().cloned().unwrap_or_default();
            let k = format!("{file}::{}", t.path.join("::"));
            let mut f = Found::test(k.clone(), name.clone()).under(groups.iter().cloned()).line(t.line).end_line(t.end_line);
            if let Some(why) = &t.skip {
                f = f.skip(why.clone());
            }
            if t.template {
                f = f.template();
            }
            // A test defined twice is the later one.
            if let Some(i) = at.insert(k.clone(), out.len()) {
                out[i] = None;
                entries[i] = None;
            }
            entries.push(Some((k, Entry { groups, line: t.line, label: name, template: t.template })));
            out.push(Some(f));
        }
        self.index.borrow_mut().scanned(&file, entries.into_iter().flatten().collect(), class_groups);
        out.into_iter().flatten().collect()
    }

    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        self.refresh();
        if let (_, Some(problem)) = self.runner() {
            return Err(problem);
        }
        if *ask == Ask::Discover {
            self.index.borrow_mut().forget_listed();
        }
        let facts = self.facts.borrow();
        let files: Vec<String> = self.index.borrow().files().map(str::to_string).collect();
        Ok(plan::plan(&self.planner(&facts, files), ask))
    }

    fn reader(&mut self, ask: &Ask, step: &Step) -> Box<dyn Reader> {
        let purpose = match ask {
            Ask::Discover => Purpose::Listing,
            _ => Purpose::Run,
        };
        // The folder the step ran in: below `/project`.
        let prefix = step.command.cwd.as_deref().and_then(|c| c.strip_prefix("/project")).map(|c| c.trim_matches('/')).filter(|c| !c.is_empty()).map(|c| format!("{c}/")).unwrap_or_default();
        let ctx = Rc::new(Ctx { index: self.index.clone(), purpose, prefix, facts: Some(self.facts.clone()) });
        Box::new(pytest::PytestReader::new(ctx))
    }

    fn launch(&mut self, test: &Found, _kept: &[(String, String)]) -> Result<Launching, String> {
        self.refresh();
        if test.key.0.starts_with(fenix_lang_kit::tests::GROUP) {
            return Err("only a test can be debugged, not a file or a class".into());
        }
        // A key over 1 KiB is cut: its full one is in `data`.
        let full = match (&test.data, is_cut_id(&test.key.0)) {
            (Some(d), _) => d.as_str(),
            (None, false) => test.key.0.as_str(),
            (None, true) => return Err("its id is too long for Fenix to keep: debug it from its own file".into()),
        };
        let root = self.root_of(full);
        let node = if root.is_empty() { full } else { full.strip_prefix(&format!("{root}/")).unwrap_or(full) };
        let cwd = if root.is_empty() { "${project}".to_string() } else { key("${project}", &root) };
        // As `launch/pytest-file.toml`: debugpy's pytest module, not just
        // my code. A template's id without its brackets runs all its cases.
        let config = LaunchConfig::launch(&format!("pytest: {full}"), &["python"]).arg("-q").arg(node).cwd(&cwd).attributes(&json!({ "module": "pytest", "justMyCode": false }));
        Ok(Launching::Now(config))
    }

    fn assets(&self) -> Vec<(&str, &[u8])> {
        vec![(plan::PLUGIN, include_bytes!("../tests/fenix_tests.py").as_slice())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pytests_version_is_read_from_either_stream_as_old_and_new_say_it() {
        assert_eq!(pytest_version("pytest 9.2.0\n").as_deref(), Some("9.2.0"));
        assert_eq!(pytest_version("This is pytest version 6.2.5, imported from /x/pytest/__init__.py\n").as_deref(), Some("6.2.5"));
        assert_eq!(pytest_version("/usr/bin/python3: No module named pytest"), None);
        assert_eq!(pytest_version(""), None);
    }

    #[test]
    fn the_lock_says_pytests_version() {
        let lock = "version = 1\n\n[[package]]\nname = \"iniconfig\"\nversion = \"2.0.0\"\n\n[[package]]\nname = \"pytest\"\nversion = \"9.2.0\"\nsource = { registry = \"https://pypi.org/simple\" }\n\n[[package]]\nname = \"pytest-cov\"\nversion = \"7.0.0\"\n";
        assert_eq!(locked_version(lock).as_deref(), Some("9.2.0"));
        assert_eq!(locked_version("[[package]]\nname = \"pytest-cov\"\nversion = \"7.0.0\"\n"), None);
    }
}
