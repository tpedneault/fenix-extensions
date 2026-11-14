//! Which MIBs there are here, and which of them are in use.
//!
//! A MIB is *known* here when it's
//!
//! - **found in the project**: a folder of `.dat` tables anywhere under
//!   it, by a bounded walk (`walk`) -- its top and its `mib/` among them,
//!   as before;
//! - **named by the settings**: `mib.roots`, the project's own list in
//!   place of yours, as Fenix gives it -- each folder at
//!   `/folders/mib/<name>`, or an XTCE file in the project.
//!
//! One that a setting names and the walk finds too is known once, by the
//! setting's name.
//!
//! The MIBs *in use* -- the MIB page's, `K`, `gd`, completion, the checks
//! of scripts, the insert form, the pickers, the notebook's and agents'
//! lookups -- are the one picked with `mib.open` in this project (kept in
//! the extension's own folder, never in the project's settings); else
//! every MIB the settings name, together, as before; else the first
//! found: `mib/`, the project's top, then the shallowest.
//!
//! Pure but for reading folders, so it's tested natively.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fenix_mib::MibRoot;

/// The tables that say a folder is a MIB: any one is enough -- a TM-only
/// database has no `ccf.dat`.
const MIB_TABLES: &[&str] = &["vdf.dat", "ccf.dat", "pcf.dat", "pid.dat", "tpcf.dat", "caf.dat", "txf.dat"];

/// Folders the walk never goes into, besides hidden ones (`.git`) and
/// those the project's `.gitignore` names: build output, dependencies,
/// what's generated from the MIB.
const SKIPPED: &[&str] = &["target", "node_modules", "generated", "dist", "vendor", "venv", "__pycache__", "bower_components", "site-packages"];

/// How far the walk goes.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    /// Folders below the project's top: 1 is its own folders.
    pub depth: usize,
    /// Folders read.
    pub folders: usize,
    /// Entries looked at, files and folders.
    pub entries: usize,
}

/// A project's own bounds: a MIB is rarely deep, and a large repository
/// is looked through in well under a second.
pub const BOUNDS: Bounds = Bounds { depth: 6, folders: 3_000, entries: 60_000 };

/// A folder of MIB tables the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Relative to the project, with `/`; "" for its top.
    pub rel: String,
    /// Its `.dat` files.
    pub tables: usize,
    /// It has telecommands (`ccf.dat`), TM packets (`pid.dat`).
    pub tc: bool,
    pub tm: bool,
}

impl Found {
    /// What the `mib.open` picker says of it.
    pub fn what(&self) -> String {
        tables_text(self.tables, self.tc, self.tm)
    }
}

/// `21 tables · TC and TM`.
pub fn tables_text(tables: usize, tc: bool, tm: bool) -> String {
    let kinds = match (tc, tm) {
        (true, true) => " · TC and TM",
        (true, false) => " · TC only",
        (false, true) => " · TM only",
        (false, false) => "",
    };
    format!("{tables} table{}{kinds}", if tables == 1 { "" } else { "s" })
}

/// What a folder's listing says of it as a MIB, if it's one.
pub fn tables_in(dir: &Path) -> Option<Found> {
    let names: Vec<String> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
    found_in(String::new(), &names)
}

fn found_in(rel: String, names: &[String]) -> Option<Found> {
    if !names.iter().any(|n| MIB_TABLES.contains(&n.as_str())) {
        return None;
    }
    let tables = names.iter().filter(|n| n.ends_with(".dat")).count();
    Some(Found { rel, tables, tc: names.iter().any(|n| n == "ccf.dat"), tm: names.iter().any(|n| n == "pid.dat") })
}

/// What a walk found, and whether a bound stopped it before the end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    pub found: Vec<Found>,
    pub cut: bool,
    pub folders: usize,
    pub entries: usize,
}

/// The project's `.gitignore`, as far as folders go: its names and paths
/// without wildcards, and `*` wildcards within one name.
#[derive(Debug, Default)]
struct Ignore {
    /// Any folder of this name (`build`, `*.egg-info`).
    names: Vec<String>,
    /// This folder, relative to the project (`/out`, `docs/build`).
    paths: Vec<String>,
}

impl Ignore {
    fn read(root: &Path) -> Ignore {
        let mut out = Ignore::default();
        let Ok(text) = std::fs::read_to_string(root.join(".gitignore")) else { return out };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') || line.contains("**") {
                continue;
            }
            let line = line.trim_end_matches('/');
            match line.strip_prefix('/') {
                Some(path) if !path.contains('*') => out.paths.push(path.to_string()),
                Some(_) => {}
                None if line.contains('/') => {
                    if !line.contains('*') {
                        out.paths.push(line.to_string());
                    }
                }
                None => out.names.push(line.to_string()),
            }
        }
        out
    }

    fn skips(&self, name: &str, rel: &str) -> bool {
        self.names.iter().any(|p| glob(p, name)) || self.paths.iter().any(|p| p == rel)
    }
}

/// Whether `name` matches `pattern`, whose `*` is any run of characters.
fn glob(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || !name[first.len()..].ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// The folders of MIB tables under `root`, breadth first, within
/// `bounds`: hidden folders, build output and dependencies (`SKIPPED`),
/// and what the project's `.gitignore` names are left out. Ordered as
/// they're offered: `mib/`, the top, then by depth and name.
pub fn walk(root: &Path, bounds: Bounds) -> Walk {
    let ignore = Ignore::read(root);
    let mut out = Walk::default();
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), String::new(), 0usize)]);
    while let Some((dir, rel, depth)) = queue.pop_front() {
        if out.folders >= bounds.folders || out.entries >= bounds.entries {
            out.cut = true;
            break;
        }
        out.folders += 1;
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut names = Vec::new();
        let mut subs = Vec::new();
        for entry in read.flatten() {
            out.entries += 1;
            let name = entry.file_name().to_string_lossy().into_owned();
            // A link isn't followed: it may lead out, or round.
            match entry.file_type() {
                Ok(t) if t.is_dir() => {
                    let sub = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                    if name.starts_with('.') || SKIPPED.contains(&name.as_str()) || ignore.skips(&name, &sub) {
                        continue;
                    }
                    if depth < bounds.depth {
                        subs.push((name, sub));
                    } else {
                        out.cut = true;
                    }
                }
                Ok(t) if t.is_file() => names.push(name.to_lowercase()),
                _ => {}
            }
            if out.entries >= bounds.entries {
                out.cut = true;
                break;
            }
        }
        if let Some(found) = found_in(rel.clone(), &names) {
            out.found.push(found);
        }
        subs.sort();
        for (name, sub) in subs {
            queue.push_back((dir.join(name), sub, depth + 1));
        }
    }
    out.found.sort_by_key(|f| (rank(&f.rel), f.rel.matches('/').count(), f.rel.clone()));
    out
}

/// `mib/` first, as a project that was a MIB had it, then the top.
fn rank(rel: &str) -> usize {
    match rel {
        "mib" => 0,
        "" => 1,
        _ => 2,
    }
}

// -- Known and in use -----------------------------------------------------------

/// Where a known MIB comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Found in the project, at this path relative to it ("" its top).
    Found(String),
    /// Named by the project's own settings.
    Project,
    /// Named by yours.
    Yours,
}

/// A MIB known here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// How it's picked and remembered: `found:<path>`, `set:<name>`.
    pub id: String,
    /// What it's called: the setting's name; a found folder's path in
    /// the project, its top by the project's name.
    pub label: String,
    pub origin: Origin,
    /// Where it's read from; none when the extension can't reach it (yet).
    pub path: Option<PathBuf>,
    /// Where it is, as the settings say or the project has it.
    pub place: String,
    /// What's in it, as far as is cheap to say.
    pub what: String,
}

impl Known {
    pub fn root(&self) -> Option<MibRoot> {
        self.path.clone().map(|path| MibRoot { label: self.label.clone(), path })
    }

    /// Found in the project, or named by its own settings: the doctor's.
    pub fn of_project(&self) -> bool {
        !matches!(self.origin, Origin::Yours)
    }
}

/// Where everything is, for `known`.
pub struct Places<'a> {
    /// The project as the extension reaches it (`/project`).
    pub project: &'a Path,
    /// Where Fenix puts the folders `mib.roots` names (`/folders/mib`).
    pub folders: &'a Path,
    /// The project's folder as you know it, a full path: a setting naming
    /// a folder in it by that path is the found one.
    pub project_path: Option<&'a str>,
    pub project_name: &'a str,
}

/// A setting's folder, relative to the project when it's in it.
fn in_project(value: &str, project_path: Option<&str>) -> Option<String> {
    let value = value.trim().replace('\\', "/");
    let clean = |s: &str| s.trim_start_matches("./").trim_end_matches('/').to_string();
    if !value.starts_with('/') && !value.starts_with('~') && !value.contains(':') {
        let rel = clean(&value);
        return Some(if rel == "." { String::new() } else { rel });
    }
    let base = project_path?.replace('\\', "/");
    let base = base.trim_end_matches('/');
    let value = value.trim_end_matches('/');
    if value == base {
        return Some(String::new());
    }
    value.strip_prefix(base).and_then(|r| r.strip_prefix('/')).map(clean)
}

/// The MIBs known here: those the settings name (`settings`, as Fenix
/// gives `mib.roots`; `project_lists` when they're the project's own),
/// then those found and not named -- found first in the picker, as
/// `ordered` lays them out.
pub fn known(found: &[Found], settings: &[(String, String)], project_lists: bool, places: &Places) -> Vec<Known> {
    let mut named = HashSet::new();
    let mut out = Vec::new();
    for (label, value) in settings {
        let rel = in_project(value, places.project_path);
        let found_here = rel.as_ref().and_then(|r| found.iter().find(|f| f.rel == *r));
        // Where Fenix put it; else, for one in the project, where it is.
        let path = fenix_mib::root_path(label, value, places.folders, places.project).or_else(|| found_here.map(|f| places.project.join(&f.rel)));
        if let Some(f) = found_here {
            named.insert(f.rel.clone());
        }
        let what = match (&path, found_here) {
            (_, Some(f)) => f.what(),
            (Some(p), None) if fenix_mib::is_xtce_file(p) => "an XTCE file".to_string(),
            (Some(p), None) => tables_in(p).map(|f| f.what()).unwrap_or_else(|| "no MIB tables in it".to_string()),
            (None, None) => "not reachable yet -- Fenix gives it to the extension when it next starts it".to_string(),
        };
        out.push(Known { id: format!("set:{label}"), label: label.clone(), origin: if project_lists { Origin::Project } else { Origin::Yours }, path, place: value.clone(), what });
    }
    for f in found.iter().filter(|f| !named.contains(&f.rel)) {
        let label = if f.rel.is_empty() { places.project_name.to_string() } else { f.rel.clone() };
        let place = if f.rel.is_empty() { "the project's top".to_string() } else { format!("{}/", f.rel) };
        let path = if f.rel.is_empty() { places.project.to_path_buf() } else { places.project.join(&f.rel) };
        out.push(Known { id: format!("found:{}", f.rel), label, origin: Origin::Found(f.rel.clone()), path: Some(path), place, what: f.what() });
    }
    ordered(out)
}

/// Found in the project first, then the project's settings', then yours.
fn ordered(mut known: Vec<Known>) -> Vec<Known> {
    known.sort_by_key(|k| match k.origin {
        Origin::Found(_) => 0,
        Origin::Project => 1,
        Origin::Yours => 2,
    });
    known
}

/// What's picked in a project with `mib.open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Every MIB the settings name, together.
    Settings,
    /// One MIB, by its id.
    One(String),
}

impl Choice {
    pub fn parse(text: &str) -> Choice {
        match text {
            "settings" => Choice::Settings,
            id => Choice::One(id.to_string()),
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Choice::Settings => "settings",
            Choice::One(id) => id,
        }
    }
}

/// The MIBs in use, by what was picked here (if it's still known and
/// reachable), else by the rule: what the settings name, else the first
/// found. With what's picked as it stands.
pub fn in_use<'a>(known: &'a [Known], choice: Option<&Choice>) -> (Vec<&'a Known>, Option<Choice>) {
    let reachable = |k: &&Known| k.path.is_some();
    let named: Vec<&Known> = known.iter().filter(|k| !matches!(k.origin, Origin::Found(_))).filter(reachable).collect();
    match choice {
        Some(Choice::One(id)) => {
            if let Some(k) = known.iter().filter(reachable).find(|k| k.id == *id) {
                return (vec![k], choice.cloned());
            }
        }
        Some(Choice::Settings) if !named.is_empty() => return (named, choice.cloned()),
        _ => {}
    }
    if !named.is_empty() {
        return (named, Some(Choice::Settings));
    }
    match known.iter().find(|k| matches!(k.origin, Origin::Found(_))) {
        Some(k) => (vec![k], Some(Choice::One(k.id.clone()))),
        None => (Vec::new(), None),
    }
}

/// Where the MIBs in use come from, as the MIB page's header says it.
pub fn source(used: &[&Known], project_name: &str, known: usize) -> String {
    let from = match used {
        [] => return String::new(),
        [one] => match &one.origin {
            Origin::Found(rel) if rel.is_empty() => "found at the project's top".to_string(),
            Origin::Found(rel) => format!("found in {rel}/"),
            Origin::Project => format!("from {project_name}'s settings"),
            Origin::Yours => "from your settings".to_string(),
        },
        several if several.iter().any(|k| k.origin == Origin::Project) => format!("from {project_name}'s settings"),
        _ => "from your settings".to_string(),
    };
    // Something else to switch to: another MIB, or one of the settings'
    // alone.
    if known > 1 {
        format!("{from} · M: {known} MIBs")
    } else {
        from
    }
}

// -- Another folder -----------------------------------------------------------

/// A name for the folder at `path` in `mib.roots`: its own name, made
/// one no other MIB has (`OPS`, `OPS-2`).
pub fn label_for(path: &str, taken: &[String]) -> String {
    let path = path.trim().trim_end_matches(['/', '\\']);
    let base: String = path.rsplit(['/', '\\']).next().unwrap_or("").chars().map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' }).collect();
    let base = match base.trim().trim_matches('.') {
        "" => "MIB".to_string(),
        b => b.to_string(),
    };
    let free = |l: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(l));
    if free(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|l| free(l)).unwrap_or(base)
}

/// What a typed or pasted folder is, cleaned: quotes and a `file://`
/// taken off. None when it isn't a full path (`/…`, `~/…`, `C:\…`).
pub fn typed_folder(text: &str) -> Option<String> {
    let t = text.trim().trim_matches(['"', '\'']).trim();
    let t = t.strip_prefix("file://").unwrap_or(t);
    let t = if t.len() > 1 { t.trim_end_matches(['/', '\\']) } else { t };
    let full = t.starts_with('/') || t.starts_with("~/") || t == "~" || t.starts_with('\\') || t.as_bytes().get(1) == Some(&b':');
    full.then(|| t.to_string())
}

/// Your `mib.roots` with `path` in it: the list to write, and the name
/// it has there -- the one it had, when it was there already (`None`
/// then for the list: nothing to write).
pub fn with_folder(yours: &[(String, String)], path: &str, others: &[String]) -> (Option<Vec<(String, String)>>, String) {
    let same = |v: &str| v.trim().trim_end_matches(['/', '\\']) == path.trim_end_matches(['/', '\\']);
    if let Some((label, _)) = yours.iter().find(|(_, v)| same(v)) {
        return (None, label.clone());
    }
    let taken: Vec<String> = yours.iter().map(|(l, _)| l.clone()).chain(others.iter().cloned()).collect();
    let label = label_for(path, &taken);
    let mut list = yours.to_vec();
    list.push((label.clone(), path.to_string()));
    (Some(list), label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn mib(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for t in ["ccf.dat", "cdf.dat", "pid.dat", "pcf.dat"] {
            std::fs::write(dir.join(t), "").unwrap();
        }
    }

    fn places<'a>(project: &'a Path, folders: &'a Path) -> Places<'a> {
        Places { project, folders, project_path: Some("/home/me/mission"), project_name: "mission" }
    }

    #[test]
    fn a_mib_anywhere_in_the_project_is_found() {
        let t = tree();
        mib(&t.path().join("db/ops"));
        std::fs::write(t.path().join("README.md"), "").unwrap();
        let w = walk(t.path(), BOUNDS);
        assert_eq!(w.found, vec![Found { rel: "db/ops".into(), tables: 4, tc: true, tm: true }]);
        assert!(!w.cut);
        assert_eq!(w.found[0].what(), "4 tables · TC and TM");
    }

    #[test]
    fn a_mib_at_the_top_and_in_mib_are_still_found_mib_first() {
        let t = tree();
        mib(t.path());
        mib(&t.path().join("mib"));
        mib(&t.path().join("a/old"));
        let rels: Vec<String> = walk(t.path(), BOUNDS).found.into_iter().map(|f| f.rel).collect();
        assert_eq!(rels, ["mib", "", "a/old"]);
    }

    #[test]
    fn hidden_build_and_ignored_folders_are_left_out() {
        let t = tree();
        for d in [".git/x", "target/debug", "node_modules/m", "generated", "out/mib", "docs/build/mib", "eggs/x.egg-info"] {
            mib(&t.path().join(d));
        }
        mib(&t.path().join("keep"));
        std::fs::write(t.path().join(".gitignore"), "# built\n/out/\ndocs/build\n*.egg-info\n!keep\n").unwrap();
        let rels: Vec<String> = walk(t.path(), BOUNDS).found.into_iter().map(|f| f.rel).collect();
        assert_eq!(rels, ["keep"]);
    }

    #[test]
    fn the_walk_stops_at_its_bounds() {
        let t = tree();
        let deep = t.path().join("a/b/c/d/e/f/g/h");
        mib(&deep);
        mib(&t.path().join("a/b/c"));
        let w = walk(t.path(), BOUNDS);
        assert_eq!(w.found.iter().map(|f| f.rel.as_str()).collect::<Vec<_>>(), ["a/b/c"], "8 deep is past the 6 it goes");
        assert!(w.cut);
        let w = walk(t.path(), Bounds { depth: 20, folders: 3, entries: 1000 });
        assert!(w.cut && w.folders == 3);
        let w = walk(t.path(), Bounds { depth: 20, folders: 100, entries: 2 });
        assert!(w.cut && w.entries == 2);
    }

    #[test]
    fn thousands_of_files_are_looked_through_quickly() {
        let t = tree();
        // A repository: 60 packages of 10 folders of 20 files, and a MIB.
        for p in 0..60 {
            for d in 0..10 {
                let dir = t.path().join(format!("pkg{p}/src{d}"));
                std::fs::create_dir_all(&dir).unwrap();
                for f in 0..20 {
                    std::fs::write(dir.join(format!("f{f}.rs")), "").unwrap();
                }
            }
        }
        mib(&t.path().join("pkg59/src9/mib"));
        let start = std::time::Instant::now();
        let w = walk(t.path(), BOUNDS);
        let took = start.elapsed();
        eprintln!("walked {} folders, {} entries in {took:?}", w.folders, w.entries);
        assert_eq!(w.found.len(), 1);
        assert!(!w.cut, "{} folders, {} entries", w.folders, w.entries);
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn a_setting_naming_a_found_folder_is_known_once_by_its_name() {
        let t = tree();
        let elsewhere = tree();
        let folders = elsewhere.path().join("folders");
        mib(&folders.join("OPS"));
        mib(&t.path().join("db/ops"));
        mib(&t.path().join("db/sim"));
        let found = walk(t.path(), BOUNDS).found;
        let p = places(t.path(), &folders);
        // By a path relative to the project, and by its full path.
        for value in ["db/ops", "./db/ops/", "/home/me/mission/db/ops"] {
            let k = known(&found, &[("OPS".into(), value.into())], true, &p);
            let ids: Vec<&str> = k.iter().map(|k| k.id.as_str()).collect();
            assert_eq!(ids, ["found:db/sim", "set:OPS"], "{value}");
            assert_eq!(k[1].what, "4 tables · TC and TM");
            assert_eq!(k[1].origin, Origin::Project);
        }
        // Not given to it where Fenix puts them: where it is.
        let k = known(&found, &[("db/x".into(), "db/sim".into())], true, &p);
        assert_eq!(k[1].path.as_deref(), Some(t.path().join("db/sim").as_path()));
        // Elsewhere: both.
        let k = known(&found, &[("OPS".into(), "/srv/mibs/ops".into())], false, &p);
        assert_eq!(k.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(), ["found:db/ops", "found:db/sim", "set:OPS"]);
        assert_eq!(k[2].origin, Origin::Yours);
    }

    #[test]
    fn the_top_is_known_by_the_projects_name() {
        let t = tree();
        mib(t.path());
        let found = walk(t.path(), BOUNDS).found;
        let k = known(&found, &[], false, &places(t.path(), Path::new("/nowhere")));
        assert_eq!((k[0].label.as_str(), k[0].place.as_str()), ("mission", "the project's top"));
        assert_eq!(k[0].root().unwrap().path, t.path());
    }

    #[test]
    fn what_is_in_use() {
        let k = |id: &str, origin: Origin, reachable: bool| Known { id: id.into(), label: id.into(), origin, path: reachable.then(|| PathBuf::from("/x")), place: String::new(), what: String::new() };
        let found = k("found:mib", Origin::Found("mib".into()), true);
        let other = k("found:db/old", Origin::Found("db/old".into()), true);
        let ops = k("set:OPS", Origin::Yours, true);
        let sim = k("set:SIM", Origin::Yours, true);
        let gone = k("set:NEW", Origin::Yours, false);
        let ids = |(used, _): (Vec<&Known>, Option<Choice>)| used.iter().map(|k| k.id.clone()).collect::<Vec<_>>();
        // Nothing picked: the settings', together; else the first found.
        let all = vec![found.clone(), other.clone(), ops.clone(), sim.clone(), gone.clone()];
        assert_eq!(ids(in_use(&all, None)), ["set:OPS", "set:SIM"]);
        assert_eq!(ids(in_use(&all[..2], None)), ["found:mib"]);
        assert_eq!(ids(in_use(&[], None)), Vec::<String>::new());
        // Picked: that one; one that's gone or not reachable yet, the rule.
        assert_eq!(ids(in_use(&all, Some(&Choice::One("found:db/old".into())))), ["found:db/old"]);
        assert_eq!(ids(in_use(&all, Some(&Choice::One("set:SIM".into())))), ["set:SIM"]);
        assert_eq!(ids(in_use(&all, Some(&Choice::One("set:NEW".into())))), ["set:OPS", "set:SIM"]);
        assert_eq!(ids(in_use(&all[..2], Some(&Choice::One("found:gone".into())))), ["found:mib"]);
        assert_eq!(ids(in_use(&all, Some(&Choice::Settings))), ["set:OPS", "set:SIM"]);
        assert_eq!(ids(in_use(&all[..2], Some(&Choice::Settings))), ["found:mib"]);
    }

    #[test]
    fn the_header_says_where_they_come_from_and_that_there_are_others() {
        let k = |origin: Origin| Known { id: String::new(), label: String::new(), origin, path: None, place: String::new(), what: String::new() };
        let mib = k(Origin::Found("mib".into()));
        let top = k(Origin::Found(String::new()));
        assert_eq!(source(&[&mib], "mission", 1), "found in mib/");
        assert_eq!(source(&[&top], "mission", 3), "found at the project's top · M: 3 MIBs");
        assert_eq!(source(&[&k(Origin::Project), &k(Origin::Project)], "mission", 2), "from mission's settings · M: 2 MIBs");
        assert_eq!(source(&[&k(Origin::Yours)], "mission", 1), "from your settings");
        assert_eq!(source(&[], "mission", 2), "");
    }

    #[test]
    fn another_folder_gets_a_name_of_its_own() {
        assert_eq!(label_for("/srv/mibs/OPS/", &[]), "OPS");
        assert_eq!(label_for("/srv/mibs/OPS", &["ops".into(), "OPS-2".into()]), "OPS-3");
        assert_eq!(label_for("C:\\MIBs\\ops 2.1", &[]), "ops 2.1");
        assert_eq!(label_for("/", &[]), "MIB");
        assert_eq!(typed_folder(" '/srv/mibs/ops/' "), Some("/srv/mibs/ops".into()));
        assert_eq!(typed_folder("file:///srv/x"), Some("/srv/x".into()));
        assert_eq!(typed_folder("~/mibs"), Some("~/mibs".into()));
        assert_eq!(typed_folder("C:\\MIBs\\OPS"), Some("C:\\MIBs\\OPS".into()));
        assert_eq!(typed_folder("mibs/ops"), None);
        assert_eq!(typed_folder(""), None);
    }

    #[test]
    fn a_folder_is_added_to_your_list_once() {
        let yours = vec![("OPS".to_string(), "/srv/a/OPS".to_string())];
        let (list, label) = with_folder(&yours, "/srv/b/OPS", &["mib".into()]);
        assert_eq!(label, "OPS-2");
        assert_eq!(list.unwrap(), vec![("OPS".into(), "/srv/a/OPS".into()), ("OPS-2".into(), "/srv/b/OPS".into())]);
        let (list, label) = with_folder(&yours, "/srv/a/OPS/", &[]);
        assert_eq!((list, label.as_str()), (None, "OPS"), "there already");
    }

    #[test]
    fn globs() {
        assert!(glob("*.egg-info", "x.egg-info"));
        assert!(glob("build*", "build-x"));
        assert!(glob("a*b*c", "aXbYc"));
        assert!(!glob("a*b*c", "aXc"));
        assert!(!glob("*.egg-info", "egg-info"));
    }

    #[test]
    fn a_folder_with_any_mib_table_is_a_mib() {
        let dir = tree();
        std::fs::create_dir_all(dir.path().join("tm")).unwrap();
        std::fs::write(dir.path().join("tm/notes.dat"), "").unwrap();
        assert!(walk(dir.path(), BOUNDS).found.is_empty(), "any .dat isn't a MIB");
        std::fs::write(dir.path().join("tm/PID.dat"), "").unwrap();
        let found = walk(dir.path(), BOUNDS).found;
        assert_eq!(found, vec![Found { rel: "tm".into(), tables: 2, tc: false, tm: true }], "a TM-only MIB has no ccf.dat");
        assert_eq!(found[0].what(), "2 tables · TM only");
    }
}
