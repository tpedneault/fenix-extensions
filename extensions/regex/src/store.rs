//! Saved patterns, kept in `/data/patterns.json`: yours, and each
//! project's, by its folder (the project `here` names).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::{Flags, Flavour};

pub const KEPT: &str = "/data/patterns.json";

/// A pattern saved by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub name: String,
    pub pattern: String,
    #[serde(default)]
    pub flags: Flags,
    #[serde(default)]
    pub flavour: Flavour,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub replacement: String,
}

/// Where a saved pattern is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    /// A project, by its folder.
    Project(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub global: Vec<Saved>,
    #[serde(default)]
    pub projects: BTreeMap<String, Vec<Saved>>,
}

impl Store {
    /// What `path` holds; nothing when it isn't there or can't be read.
    pub fn load(path: &Path) -> Store {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("couldn't keep the saved patterns: {e}"))
    }

    fn list_mut(&mut self, scope: &Scope) -> &mut Vec<Saved> {
        match scope {
            Scope::Global => &mut self.global,
            Scope::Project(key) => self.projects.entry(key.clone()).or_default(),
        }
    }

    /// Kept under `scope`, in place of one there by the same name.
    pub fn put(&mut self, scope: &Scope, saved: Saved) {
        let list = self.list_mut(scope);
        match list.iter_mut().find(|s| s.name == saved.name) {
            Some(old) => *old = saved,
            None => list.push(saved),
        }
        list.sort_by_key(|s| s.name.to_lowercase());
    }

    pub fn remove(&mut self, scope: &Scope, name: &str) -> bool {
        let list = self.list_mut(scope);
        let before = list.len();
        list.retain(|s| s.name != name);
        let gone = list.len() != before;
        if let Scope::Project(key) = scope {
            if self.projects.get(key).is_some_and(Vec::is_empty) {
                self.projects.remove(key);
            }
        }
        gone
    }

    /// What can be opened from `project`: its own first, then yours.
    pub fn visible(&self, project: Option<&str>) -> Vec<(Scope, &Saved)> {
        let own = project.and_then(|p| self.projects.get(p).map(|l| (p, l)));
        let own = own.into_iter().flat_map(|(p, l)| l.iter().map(move |s| (Scope::Project(p.to_string()), s)));
        own.chain(self.global.iter().map(|s| (Scope::Global, s))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(name: &str, pattern: &str) -> Saved {
        Saved { name: name.into(), pattern: pattern.into(), flags: Flags::default(), flavour: Flavour::Rust, replacement: String::new() }
    }

    #[test]
    fn a_project_sees_its_own_and_yours() {
        let mut s = Store::default();
        let p = Scope::Project("/w/fenix".into());
        s.put(&Scope::Global, saved("date", r"\d{4}-\d\d"));
        s.put(&p, saved("ticket", r"FX-\d+"));
        s.put(&Scope::Project("/w/other".into()), saved("other", "x"));
        let names: Vec<&str> = s.visible(Some("/w/fenix")).iter().map(|(_, s)| s.name.as_str()).collect();
        assert_eq!(names, ["ticket", "date"]);
        assert_eq!(s.visible(None).len(), 1);
    }

    #[test]
    fn the_same_name_replaces_and_remove_tidies() {
        let mut s = Store::default();
        let p = Scope::Project("/w".into());
        s.put(&p, saved("a", "1"));
        s.put(&p, saved("a", "2"));
        assert_eq!(s.projects["/w"].len(), 1);
        assert_eq!(s.projects["/w"][0].pattern, "2");
        assert!(s.remove(&p, "a"));
        assert!(s.projects.is_empty());
        assert!(!s.remove(&Scope::Global, "a"));
    }

    #[test]
    fn kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("fenix-regex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("patterns.json");
        let mut s = Store::default();
        let mut one = saved("words", r"\w+");
        one.flags.i = true;
        one.flavour = Flavour::Fancy;
        one.replacement = "<$0>".into();
        s.put(&Scope::Global, one);
        s.save(&path).unwrap();
        assert_eq!(Store::load(&path), s);
        assert_eq!(Store::load(&dir.join("none.json")), Store::default());
        std::fs::remove_dir_all(dir).ok();
    }
}
