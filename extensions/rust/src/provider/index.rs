//! What the provider's own scans and listings found, by target: each
//! test's path, which are templates, and each module's groups. A reader
//! uses it to place a test a listing or a run finds that no scan did
//! (`tests::case_negative`, made by a macro, under `tests`; `tests::sums::
//! case_1` under its `#[rstest]` template), and to tell which binary a
//! run's lines are from when cargo's headers alone can't (§6.1 of the
//! plan: a `harness = false` binary prints no `running N tests`).
//!
//! lang-kit remembers scans too, but only for keys it's given; this is
//! the pack's, shared with its readers (an `Rc`), so a reader made for a
//! step sees the scans made while it reads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fenix_lang_kit::tests::Group;

#[derive(Debug, Default)]
struct FileEntry {
    target: String,
    tests: Vec<String>,
    modules: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Index {
    files: HashMap<String, FileEntry>,
    /// By target id (`orbit/lib`): each test's path, whether it's a
    /// template, and the file that gave it (none: a listing).
    tests: HashMap<String, HashMap<String, (bool, Option<String>)>>,
    /// By target id: each module path's groups, root first.
    modules: HashMap<String, HashMap<String, Vec<Group>>>,
}

pub type Shared = Rc<RefCell<Index>>;

impl Index {
    /// File `file`'s scan: its target, its tests' paths (and whether each
    /// is a template) and its modules' groups. Replaces its last.
    pub fn scanned(&mut self, file: &str, target: Option<&str>, tests: Vec<(String, bool)>, modules: Vec<(String, Vec<Group>)>) {
        if let Some(old) = self.files.remove(file) {
            if let Some(t) = self.tests.get_mut(&old.target) {
                for p in &old.tests {
                    if t.get(p).is_some_and(|(_, f)| f.as_deref() == Some(file)) {
                        t.remove(p);
                    }
                }
            }
            if let Some(m) = self.modules.get_mut(&old.target) {
                for p in &old.modules {
                    m.remove(p);
                }
            }
        }
        let Some(target) = target else { return };
        let entry = FileEntry { target: target.into(), tests: tests.iter().map(|(p, _)| p.clone()).collect(), modules: modules.iter().map(|(p, _)| p.clone()).collect() };
        let t = self.tests.entry(target.into()).or_default();
        for (path, template) in tests {
            t.insert(path, (template, Some(file.into())));
        }
        let m = self.modules.entry(target.into()).or_default();
        for (path, groups) in modules {
            m.insert(path, groups);
        }
        self.files.insert(file.into(), entry);
    }

    /// A listing gave `path` in `target`.
    pub fn listed(&mut self, target: &str, path: &str) {
        self.tests.entry(target.into()).or_default().entry(path.into()).or_insert((false, None));
    }

    /// Whether a scan or a listing has `path` in `target`, or a template
    /// whose cases it can be.
    pub fn knows(&self, target: &str, path: &str) -> bool {
        self.tests.get(target).is_some_and(|t| t.contains_key(path)) || self.template_of(target, path).is_some()
    }

    /// Whether a scan found `path` itself (not a listing).
    pub fn scanned_test(&self, target: &str, path: &str) -> bool {
        self.tests.get(target).and_then(|t| t.get(path)).is_some_and(|(_, file)| file.is_some())
    }

    /// The template `path` is a case of: the longest known template whose
    /// path is its start, up to a `::` (`tests::sums` of
    /// `tests::sums::case_1`).
    pub fn template_of(&self, target: &str, path: &str) -> Option<String> {
        let t = self.tests.get(target)?;
        let mut at = path.len();
        while let Some(i) = path[..at].rfind("::") {
            if t.get(&path[..i]).is_some_and(|(template, _)| *template) {
                return Some(path[..i].to_string());
            }
            at = i;
        }
        None
    }

    /// The groups of the deepest module a scan found that `path` is in
    /// (`tests` of `tests::case_negative`).
    pub fn groups_of(&self, target: &str, path: &str) -> Option<Vec<Group>> {
        let m = self.modules.get(target)?;
        let mut at = path.len();
        while let Some(i) = path[..at].rfind("::") {
            if let Some(g) = m.get(&path[..i]) {
                return Some(g.clone());
            }
            at = i;
        }
        m.get("").cloned()
    }

    /// The groups a template was found under.
    pub fn template_groups(&self, target: &str, template: &str) -> Option<Vec<Group>> {
        self.groups_of(target, &format!("{template}::x"))
    }

    pub fn is_template(&self, target: &str, path: &str) -> bool {
        self.tests.get(target).and_then(|t| t.get(path)).is_some_and(|(template, _)| *template)
    }

    /// A listing starts: what the last one gave is forgotten.
    pub fn forget_listed(&mut self) {
        for t in self.tests.values_mut() {
            t.retain(|_, (_, file)| file.is_some());
        }
    }
}
