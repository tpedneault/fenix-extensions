//! Whether Git would commit an env file: one it tracks, or one
//! `.gitignore` doesn't ignore. Asked of `git ls-files` and `git
//! check-ignore`, in the project.

use std::collections::HashSet;

use crate::files;

/// What Git said of the project's env files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitState {
    /// The project is in a repository.
    pub repo: bool,
    /// The files asked about.
    pub asked: Vec<String>,
    pub tracked: HashSet<String>,
    pub ignored: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Committed already: ignoring it now doesn't take it out.
    Tracked,
    /// Not ignored: `git add .` takes it.
    NotIgnored,
}

impl Risk {
    pub fn said(self, rel: &str) -> String {
        match self {
            Risk::Tracked => format!("{rel} is tracked by Git: its secrets are in the repository's history"),
            Risk::NotIgnored => format!("{rel} isn't in .gitignore: Git would commit it"),
        }
    }
}

/// The files worth asking about: env files that aren't examples.
pub fn worth_asking(all: &[String]) -> Vec<String> {
    all.iter().filter(|f| !files::is_example(files::name(f))).cloned().collect()
}

pub fn ls_files(rels: &[String]) -> Vec<String> {
    ["ls-files", "--"].into_iter().map(String::from).chain(rels.iter().cloned()).collect()
}

/// Ignored by a pattern, tracked or not (`--no-index`).
pub fn check_ignore(rels: &[String]) -> Vec<String> {
    ["check-ignore", "--no-index", "--"].into_iter().map(String::from).chain(rels.iter().cloned()).collect()
}

/// The paths Git wrote, a line each; a quoted one (an odd character in
/// it) unquoted as far as `"` and `\\` go.
pub fn paths(lines: &[String]) -> HashSet<String> {
    lines
        .iter()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_prefix('"').and_then(|l| l.strip_suffix('"')) {
            Some(q) => q.replace("\\\"", "\"").replace("\\\\", "\\"),
            None => l.to_string(),
        })
        .collect()
}

impl GitState {
    /// What's wrong with `rel`, if Git would commit it.
    pub fn risk(&self, rel: &str) -> Option<Risk> {
        if !self.repo || !self.asked.iter().any(|a| a == rel) {
            return None;
        }
        if self.tracked.contains(rel) {
            Some(Risk::Tracked)
        } else if !self.ignored.contains(rel) {
            Some(Risk::NotIgnored)
        } else {
            None
        }
    }

    /// Every file asked about that Git would commit, with why.
    pub fn risks(&self) -> Vec<(String, Risk)> {
        self.asked.iter().filter_map(|f| self.risk(f).map(|r| (f.clone(), r))).collect()
    }
}

/// The lines `.gitignore` (its text) needs for `rels` to be ignored: each
/// path, but not one it already has.
pub fn ignore_lines(gitignore: &str, rels: &[String]) -> Vec<String> {
    let has: HashSet<&str> = gitignore.lines().map(|l| l.trim().trim_start_matches('/')).collect();
    let mut out: Vec<String> = Vec::new();
    for rel in rels {
        if !has.contains(rel.as_str()) && !out.contains(rel) {
            out.push(rel.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_git_said() {
        let asked = worth_asking(&[".env".into(), ".env.example".into(), ".env.local".into(), "api/.env".into()]);
        assert_eq!(asked, [".env", ".env.local", "api/.env"]);
        assert_eq!(ls_files(&asked), ["ls-files", "--", ".env", ".env.local", "api/.env"]);
        assert_eq!(check_ignore(&asked)[..3], ["check-ignore", "--no-index", "--"]);
        let state = GitState { repo: true, asked, tracked: paths(&[".env".into()]), ignored: paths(&[".env".into(), "\"api/.env\"".into(), String::new()]) };
        assert_eq!(state.risk(".env"), Some(Risk::Tracked), "ignored, but committed already");
        assert_eq!(state.risk(".env.local"), Some(Risk::NotIgnored));
        assert_eq!(state.risk("api/.env"), None);
        assert_eq!(state.risk(".env.example"), None, "not asked: an example is meant to be committed");
        assert_eq!(state.risks().len(), 2);
        assert_eq!(GitState { repo: false, ..state }.risks(), []);
    }

    #[test]
    fn gitignore_gets_what_it_lacks() {
        let rels: Vec<String> = [".env", ".env.local", "api/.env", ".env.local"].map(String::from).to_vec();
        assert_eq!(ignore_lines("node_modules\n/.env\n", &rels), [".env.local", "api/.env"]);
        assert!(ignore_lines(".env\n.env.local\napi/.env\n", &rels).is_empty());
    }
}
