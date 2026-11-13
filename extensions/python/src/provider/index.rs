//! What the provider's own scans found, by key: each test's groups, line
//! and whether it's a template, and each class's group. A reader uses it to
//! place what pytest lists and runs that no scan found -- the cases of a
//! `parametrize` under their template, a test in a file nobody scanned
//! under its file and classes -- and to leave alone what a scan has.
//!
//! lang-kit remembers scans too, but not for a reader to ask; this is the
//! pack's, shared with its readers (an `Rc`), so a reader made for a step
//! sees the scans made while it reads.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fenix_lang_kit::tests::Group;

#[derive(Debug, Clone)]
pub struct Entry {
    /// The groups it's in, root first: the tests of one class share them.
    pub groups: Rc<Vec<Group>>,
    pub line: u32,
    pub label: String,
    pub template: bool,
}

#[derive(Debug, Default)]
pub struct Index {
    tests: HashMap<String, Entry>,
    /// A class's key (`tests/test_a.py::TestA`) and its group.
    classes: HashMap<String, Group>,
    /// Each scanned file's keys, so the next scan replaces them.
    files: HashMap<String, (Vec<String>, Vec<String>)>,
    /// What a listing gave that no scan had: the cases, mostly.
    listed: HashSet<String>,
}

pub type Shared = Rc<RefCell<Index>>;

impl Index {
    /// `file`'s scan: its tests and its classes' groups, by key. Replaces
    /// its last.
    pub fn scanned(&mut self, file: &str, tests: Vec<(String, Entry)>, classes: Vec<(String, Group)>) {
        if let Some((old_tests, old_classes)) = self.files.remove(file) {
            for k in old_tests {
                self.tests.remove(&k);
            }
            for k in old_classes {
                self.classes.remove(&k);
            }
        }
        if tests.is_empty() && classes.is_empty() {
            return;
        }
        let keys = tests.iter().map(|(k, _)| k.clone()).collect();
        let class_keys = classes.iter().map(|(k, _)| k.clone()).collect();
        self.tests.extend(tests);
        self.classes.extend(classes);
        self.files.insert(file.into(), (keys, class_keys));
    }

    pub fn test(&self, key: &str) -> Option<&Entry> {
        self.tests.get(key)
    }

    pub fn class(&self, key: &str) -> Option<&Group> {
        self.classes.get(key)
    }

    /// The folders (below the project) of the files scanned, for a run of
    /// all or a listing to start in each.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// A listing gave `key`.
    pub fn listed(&mut self, key: &str) -> bool {
        self.listed.insert(key.to_string())
    }

    pub fn was_listed(&self, key: &str) -> bool {
        self.listed.contains(key)
    }

    /// A listing starts: what the last one gave is forgotten.
    pub fn forget_listed(&mut self) {
        self.listed.clear();
    }
}
