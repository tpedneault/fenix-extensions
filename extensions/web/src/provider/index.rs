//! What the provider's own scans found, by file: each test's key, names,
//! lines and whether it's a template, and each describe's group. A reader
//! uses it to name what a runner reports (`names > twice` the second time
//! is `names > twice (2)`), to place what no scan found (a `test.each`
//! case under its template, by its line or its title; a test whose title
//! isn't a literal under the describes the scan saw) and to tell which
//! keys were scanned, whose `Found` it needn't send again. Shared with the
//! readers (an `Rc`), so a reader made for a step sees scans made while
//! it reads.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fenix_lang_kit::tests::Group;

/// A scanned test.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    /// Its names as its runner has them: no ` (2)`.
    pub names: Vec<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub template: bool,
    /// The groups it's under, root first: a template's (its cases'), none
    /// for the rest.
    pub groups: Vec<Group>,
}

#[derive(Debug, Default)]
struct File {
    tests: Vec<Entry>,
    /// Each describe's groups, root first, by its names.
    suites: HashMap<Vec<String>, Vec<Group>>,
}

#[derive(Debug, Default)]
pub struct Index {
    files: HashMap<String, File>,
    /// Each scanned key's file, and its place in the file's tests.
    keys: HashMap<String, (String, usize)>,
    /// Names a listing or a run gave, by key, for those the scan didn't.
    reported: HashMap<String, Vec<String>>,
    listed: HashSet<String>,
}

pub type Shared = Rc<RefCell<Index>>;

/// Whether `title` is what template `pattern` (`add(%i, %i) is %i`,
/// `adds ${n}`, `table $a`) makes: its placeholders as anything.
pub fn made_by(pattern: &str, title: &str) -> bool {
    let parts = literal_parts(pattern);
    let Some((first, rest)) = parts.split_first() else { return true };
    let Some(mut left) = title.strip_prefix(first.as_str()) else { return false };
    if rest.is_empty() {
        return left.is_empty();
    }
    let (last, middle) = rest.split_last().expect("one");
    for p in middle {
        match left.find(p.as_str()) {
            Some(i) => left = &left[i + p.len()..],
            None => return false,
        }
    }
    left.len() >= last.len() && left.ends_with(last.as_str())
}

/// A template's title in the pieces between its placeholders: `%s`, `%d`,
/// `%i`, `%f`, `%j`, `%o`, `%O`, `%p`, `%c`, `%#`, `%$` (`%%` is a `%`),
/// `$name.path`, `${..}`. Always one more than its placeholders.
pub fn literal_parts(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut parts = vec![String::new()];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '%' && next == Some('%') {
            parts.last_mut().expect("one").push('%');
            i += 2;
        } else if c == '%' && next.is_some_and(|n| "sdifjoOpc#$".contains(n)) {
            parts.push(String::new());
            i += 2;
        } else if c == '$' && next == Some('{') {
            let end = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p + 1);
            parts.push(String::new());
            i = end;
        } else if c == '$' && next.is_some_and(|n| n.is_alphabetic() || n == '_') {
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || (chars[i] == '.' && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric()))) {
                i += 1;
            }
            parts.push(String::new());
        } else {
            parts.last_mut().expect("one").push(c);
            i += 1;
        }
    }
    parts
}

impl Index {
    /// File `file`'s scan, replacing its last.
    pub fn scanned(&mut self, file: &str, tests: Vec<Entry>, suites: HashMap<Vec<String>, Vec<Group>>) {
        if let Some(old) = self.files.remove(file) {
            for t in old.tests {
                self.keys.remove(&t.key);
            }
        }
        for (i, t) in tests.iter().enumerate() {
            self.keys.insert(t.key.clone(), (file.to_string(), i));
        }
        self.files.insert(file.to_string(), File { tests, suites });
    }

    pub fn is_scanned(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }

    /// The scanned test keyed `key`.
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        let (file, at) = self.keys.get(key)?;
        self.files.get(file)?.tests.get(*at).filter(|t| t.key == key)
    }

    pub fn is_template(&self, key: &str) -> bool {
        self.entry(key).is_some_and(|e| e.template)
    }

    /// A test's names as its runner has them, when a scan, a listing or a
    /// run gave them.
    pub fn names_of(&self, key: &str) -> Option<Vec<String>> {
        self.entry(key).map(|e| e.names.clone()).or_else(|| self.reported.get(key).cloned())
    }

    /// A listing or a run gave `key` with `names`.
    pub fn reported(&mut self, key: &str, names: &[String], listing: bool) {
        if !self.keys.contains_key(key) {
            self.reported.insert(key.to_string(), names.to_vec());
        }
        if listing {
            self.listed.insert(key.to_string());
        }
    }

    /// A listing starts: what the last gave is forgotten.
    pub fn forget_listed(&mut self) {
        let listed = std::mem::take(&mut self.listed);
        self.reported.retain(|k, _| !listed.contains(k));
    }

    /// The key of the scanned test of `file` with `names` that a runner
    /// reports at `line`: the one at that line when several share them,
    /// else the `k`-th (from 1); none when the scan has none of them, or
    /// fewer than `k`.
    pub fn key_for(&self, file: &str, names: &[String], line: Option<u32>, k: usize) -> Option<String> {
        let f = self.files.get(file)?;
        let same: Vec<&Entry> = f.tests.iter().filter(|t| t.names == names).collect();
        if same.len() > 1 {
            if let Some(e) = line.and_then(|l| same.iter().find(|t| t.line == l)) {
                return Some(e.key.clone());
            }
        }
        same.get(k.saturating_sub(1)).map(|e| e.key.clone())
    }

    /// The template of `file` a test named `names`, reported at `line`, is
    /// a case of: one under the same describes whose call spans the line,
    /// else one whose title makes its own.
    pub fn template_of(&self, file: &str, names: &[String], line: Option<u32>) -> Option<&Entry> {
        let f = self.files.get(file)?;
        let (title, parents) = names.split_last()?;
        let candidates = f.tests.iter().filter(|t| t.template && t.names.len() == names.len() && t.names[..t.names.len() - 1] == *parents);
        let mut by_title = None;
        for t in candidates {
            if let Some(l) = line {
                if t.line <= l && l <= t.end_line.unwrap_or(t.line) {
                    return Some(t);
                }
            }
            if by_title.is_none() && made_by(t.names.last().expect("named"), title) {
                by_title = Some(t);
            }
        }
        by_title
    }

    /// The groups of the deepest describe the scan found that `names` (a
    /// test's, its own last) are in, and how many of its names they hold.
    pub fn groups_of(&self, file: &str, names: &[String]) -> Option<(Vec<Group>, usize)> {
        let f = self.files.get(file)?;
        (1..names.len()).rev().find_map(|n| f.suites.get(&names[..n]).map(|g| (g.clone(), n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_templates_title_makes_its_cases() {
        assert!(made_by("add(%i, %i) is %i", "add(-1, -1) is -3"));
        assert!(made_by("adds ${n}", "adds 2") && made_by("table $a", "table 1") && made_by("$a + $b.c", "1 + 2"));
        assert!(made_by("100%% of %s", "100% of x") && made_by("%s", "anything"));
        assert!(!made_by("add(%i, %i) is %i", "adds") && !made_by("adds ${n}", "subtracts 2") && !made_by("x %s y", "x 1"));
        assert_eq!(literal_parts("add(%i, %i) is %i"), ["add(", ", ", ") is ", ""]);
    }
}
