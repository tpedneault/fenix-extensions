//! What a provider's scans and listings found, kept by lang-kit for the
//! answers that don't say it all: a listing's or a run's test given the
//! scan's groups, file and line; a case put under its template; an asked
//! group's tests; a group's kind and file when Fenix sends only its id.
//! The last scan of each file and the last listing, about 150 bytes a
//! test; lost when the pack starts again, when Fenix scans again
//! (Fenix §7.1).

use std::collections::{HashMap, HashSet};

use super::{child_id, cut_id, group_id, Found, Group, Key, Kind, GROUP};

/// A test or case as the memory has it.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    pub key: Key,
    pub label: String,
    pub kind: Kind,
    /// Its innermost group's id.
    pub group: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub skip: Option<String>,
    pub template: bool,
    pub of: Option<Key>,
    pub data: Option<String>,
    /// From the last scan of its file; from the last listing.
    pub scanned: bool,
    pub listed: bool,
}

#[derive(Default)]
pub(crate) struct Memory {
    tests: HashMap<String, Known>,
    /// Each group by id, with its parent's id.
    groups: HashMap<String, (Group, Option<String>)>,
    /// The ids each file's last scan gave.
    scans: HashMap<String, Vec<String>>,
    /// Templates by file and line, from 1.
    templates: HashMap<(String, u32), String>,
}

/// `path` below the project: `/project/` and `./` taken off.
pub(crate) fn below(path: &str) -> &str {
    let p = path.strip_prefix("/project/").unwrap_or(path);
    p.strip_prefix("./").unwrap_or(p)
}

impl Memory {
    pub fn known(&self, id: &str) -> Option<&Known> {
        self.tests.get(id)
    }

    fn remember_groups(&mut self, groups: &[Group]) -> Option<String> {
        let mut parent: Option<String> = None;
        for g in groups {
            let id = child_id(parent.as_deref().unwrap_or(""), &g.label);
            match self.groups.get(&id) {
                Some((had, p)) if had == g && *p == parent => {}
                _ => {
                    self.groups.insert(id.clone(), (g.clone(), parent.take()));
                }
            }
            parent = Some(id);
        }
        parent
    }

    fn record(&mut self, f: &Found, file: Option<String>) -> Known {
        let group = self.remember_groups(&f.groups);
        Known {
            key: f.key.clone(),
            label: f.label.clone(),
            kind: f.kind,
            group,
            file,
            line: f.line,
            end_line: f.end_line,
            skip: f.skip.clone(),
            template: f.template,
            of: f.of.clone(),
            data: f.data.clone(),
            scanned: false,
            listed: false,
        }
    }

    /// File `path`'s scan found `found`: its last scan's tests it no
    /// longer gives are forgotten, unless a listing has them.
    pub fn scanned(&mut self, path: &str, found: &[Found]) {
        let path = below(path).to_string();
        let mut ids = Vec::with_capacity(found.len());
        for f in found {
            let id = cut_id(&f.key.0).into_owned();
            let file = Some(f.file.as_deref().map(|p| below(p).to_string()).unwrap_or_else(|| path.clone()));
            let mut k = self.record(f, file);
            k.scanned = true;
            k.listed = self.tests.get(&id).is_some_and(|old| old.listed);
            if let (true, Some(file), Some(line)) = (k.template, &k.file, k.line) {
                self.templates.insert((file.clone(), line), id.clone());
            }
            self.tests.insert(id.clone(), k);
            ids.push(id);
        }
        let now: HashSet<&String> = ids.iter().collect();
        for old in self.scans.remove(&path).unwrap_or_default() {
            if now.contains(&old) {
                continue;
            }
            self.forget_scan(&old);
        }
        self.scans.insert(path, ids);
    }

    fn forget_scan(&mut self, id: &str) {
        let Some(k) = self.tests.get_mut(id) else { return };
        k.scanned = false;
        if let (true, Some(file), Some(line)) = (k.template, k.file.clone(), k.line) {
            if self.templates.get(&(file.clone(), line)).map(String::as_str) == Some(id) {
                self.templates.remove(&(file, line));
            }
        }
        if !k.listed {
            self.tests.remove(id);
        }
    }

    /// A listing gave `f`: kept beside what a scan has of it, which wins.
    pub fn listed(&mut self, f: &Found) -> String {
        let id = cut_id(&f.key.0).into_owned();
        match self.tests.get_mut(&id) {
            Some(k) if k.scanned => k.listed = true,
            _ => {
                let file = f.file.as_deref().map(|p| below(p).to_string());
                let mut k = self.record(f, file);
                k.listed = true;
                self.tests.insert(id.clone(), k);
            }
        }
        id
    }

    /// A complete listing ended: what it didn't give is the listing's no
    /// more.
    pub fn listing_ended(&mut self, seen: &HashSet<String>) {
        let gone: Vec<String> = self.tests.iter().filter(|(id, k)| k.listed && !seen.contains(*id)).map(|(id, _)| id.clone()).collect();
        for id in gone {
            let scanned = self.tests.get_mut(&id).map(|k| {
                k.listed = false;
                k.scanned
            });
            if scanned == Some(false) {
                self.tests.remove(&id);
            }
        }
    }

    /// Group `id` and those it's in, root first: as remembered, or, for
    /// one never seen, its labels read back from its id.
    pub fn chain(&self, id: &str) -> Vec<Group> {
        let mut out = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(id) = cur {
            if let Some((g, parent)) = self.groups.get(&id) {
                out.push(g.clone());
                cur = parent.clone();
                continue;
            }
            let labels: Vec<&str> = id.split(GROUP).skip(1).collect();
            for n in (1..=labels.len()).rev() {
                let g = self.groups.get(&group_id(&labels[..n])).map(|(g, _)| g.clone());
                out.push(g.unwrap_or_else(|| Group::new(Kind::Group, labels[n - 1])));
            }
            break;
        }
        out.reverse();
        out
    }

    /// Test `id` as a [`Found`], with its groups.
    pub fn found(&self, id: &str) -> Option<Found> {
        let k = self.tests.get(id)?;
        Some(Found {
            key: k.key.clone(),
            label: k.label.clone(),
            kind: k.kind,
            groups: k.group.as_deref().map(|g| self.chain(g)).unwrap_or_default(),
            file: k.file.clone(),
            line: k.line,
            end_line: k.end_line,
            skip: k.skip.clone(),
            template: k.template,
            of: k.of.clone(),
            data: k.data.clone(),
        })
    }

    /// `f`, from a listing or a run, filled in from a scan that found its
    /// key: the scan's groups, and its file and lines where `f` gives
    /// none. A listing that can't place a test (cargo's `--list`) then
    /// neither moves it nor adds groups of its guessing.
    pub fn fill(&self, f: &mut Found) {
        let Some(k) = self.tests.get(cut_id(&f.key.0).as_ref()).filter(|k| k.scanned) else { return };
        if let Some(g) = &k.group {
            f.groups = self.chain(g);
        }
        if f.file.is_none() {
            f.file = k.file.clone();
            f.line = f.line.or(k.line);
            f.end_line = f.end_line.or(k.end_line);
        }
        if f.of.is_none() {
            f.of = k.of.clone();
        }
    }

    /// The template `key` is a case of, when a known template's key is
    /// its start up to a separator (`tests::x` of `tests::x::case_1`,
    /// `…::test_cases` of `…::test_cases[negative]`, `TestTable` of
    /// `TestTable/negative_one`), or a template is at its file and line
    /// (`test.each`'s cases are at the template's line).
    pub fn template_of(&self, key: &str, file: Option<&str>, line: Option<u32>) -> Option<Key> {
        for (at, c) in key.char_indices().rev() {
            if at == 0 || c.is_alphanumeric() || c == '_' {
                continue;
            }
            if let Some(k) = self.tests.get(cut_id(&key[..at]).as_ref()).filter(|k| k.template) {
                return Some(k.key.clone());
            }
        }
        let (file, line) = (below(file?), line?);
        let id = self.templates.get(&(file.to_string(), line))?;
        self.tests.get(id).map(|k| k.key.clone())
    }

    /// The tests remembered under group `id`, in key order.
    pub fn under(&self, id: &str) -> Vec<Key> {
        let deeper = format!("{id}{GROUP}");
        let mut keys: Vec<Key> = self
            .tests
            .values()
            .filter(|k| k.group.as_deref().is_some_and(|g| g == id || g.starts_with(&deeper)))
            .map(|k| k.key.clone())
            .collect();
        keys.sort();
        keys
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.tests.len()
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    fn lib() -> Vec<Group> {
        vec![Group::package("orbit (lib)"), Group::file("src/lib.rs"), Group::suite("tests", "src/lib.rs", 9)]
    }

    #[test]
    fn a_scan_replaces_its_files_last_and_a_listing_keeps_its_own() {
        let mut m = Memory::default();
        m.scanned("/project/src/lib.rs", &[Found::test("lib tests::adds", "adds").under(lib()).line(10), Found::test("lib tests::gone", "gone").under(lib()).line(20)]);
        m.listed(&Found::test("lib tests::gone", "gone"));
        m.listed(&Found::test("lib tests::case_negative", "case_negative"));
        assert_eq!(m.len(), 3);
        m.scanned("/project/src/lib.rs", &[Found::test("lib tests::adds", "adds").under(lib()).line(11)]);
        assert!(m.known("lib tests::gone").is_some_and(|k| !k.scanned && k.listed), "the listing still has it");
        m.listing_ended(&["lib tests::case_negative".to_string()].into_iter().collect());
        assert!(m.known("lib tests::gone").is_none() && m.known("lib tests::case_negative").is_some());
        assert_eq!(m.found("lib tests::adds").unwrap().file.as_deref(), Some("src/lib.rs"));
        assert_eq!(m.under(&group_id(&["orbit (lib)", "src/lib.rs"])), [Key::from("lib tests::adds")]);
    }

    #[test]
    fn a_listings_test_takes_the_scans_place_and_a_case_finds_its_template() {
        let mut m = Memory::default();
        m.scanned("/project/src/lib.rs", &[Found::test("lib tests::adds", "adds").under(lib()).line(10), Found::test("lib tests::x", "x").under(lib()).line(30).template()]);
        let mut listed = Found::test("lib tests::adds", "adds").under([Group::package("orbit (lib)"), Group::file("src/tests.rs")]);
        m.fill(&mut listed);
        assert_eq!((listed.groups, listed.file.as_deref(), listed.line), (lib(), Some("src/lib.rs"), Some(10)));
        assert_eq!(m.template_of("lib tests::x::case_1", None, None), Some(Key::from("lib tests::x")));
        assert_eq!(m.template_of("lib tests::xy", None, None), None, "not at a separator");
        assert_eq!(m.template_of("lib other", Some("/project/src/lib.rs"), Some(30)), Some(Key::from("lib tests::x")), "at its line");
        let chain = m.chain(&group_id(&["orbit (lib)", "src/lib.rs", "unseen"]));
        assert_eq!(chain.iter().map(|g| (g.kind, g.label.as_str())).collect::<Vec<_>>(), [(Kind::Group, "orbit (lib)"), (Kind::File, "src/lib.rs"), (Kind::Group, "unseen")]);
    }
}
