//! Which MIBs there are in a project, and which of them are in use: the
//! one rule the MIB and CCSDS extensions both follow.
//!
//! A MIB is *known* in a project when it's
//!
//! - **found in it**: its `mib/` folder or its top, looked at directly as
//!   ever (links followed); and any other folder of `.dat` tables, by a
//!   bounded walk (`walk`);
//! - **named by the settings**: `mib.roots`, the project's own list in
//!   place of yours, as Fenix gives it -- each folder at
//!   `/folders/mib/<name>`, or an XTCE file in the project;
//! - **picked** in it: a folder of the project the walk didn't reach,
//!   opened by its path.
//!
//! One a setting names and the walk finds too is known once, by the
//! setting's name.
//!
//! The MIBs *in use* (`in_use`) are the one picked in the project, if
//! any; else every MIB the settings name, together; else a found one,
//! when there's no doubt which: `mib/`, else the top, else the only one
//! found outside folders of tests, fixtures, samples, examples, demos,
//! mocks and vendored code. Otherwise none, until one is picked.
//!
//! Pure but for reading folders, so it's tested natively.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::MibRoot;

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
/// is looked through in well under a second. `mib/` and the top are
/// looked at besides, whatever the walk meets.
pub const BOUNDS: Bounds = Bounds { depth: 6, folders: 3_000, entries: 60_000 };

/// A folder of MIB tables.
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
    /// What the pickers say of it.
    pub fn what(&self) -> String {
        tables_text(self.tables, self.tc, self.tm)
    }

    /// Whether it's in a folder of tests, fixtures, samples, examples,
    /// demos, mocks or vendored code: never used without being picked.
    pub fn incidental(&self) -> bool {
        incidental(&self.rel)
    }
}

fn incidental(rel: &str) -> bool {
    rel.split('/').any(|c| {
        let c = c.to_ascii_lowercase();
        ["test", "fixture", "sample", "example", "demo", "mock"].iter().any(|p| c.starts_with(p)) || ["vendor", "vendored", "third_party", "thirdparty", "3rdparty"].contains(&c.as_str())
    })
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

/// `dir` as a MIB, if it's one: its tables looked at directly, links
/// followed, as `rel` in the project.
pub fn probe(dir: &Path, rel: &str) -> Option<Found> {
    if !MIB_TABLES.iter().any(|t| dir.join(t).is_file()) {
        return None;
    }
    let tables = std::fs::read_dir(dir).map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".dat")).count()).unwrap_or(0);
    Some(Found { rel: rel.to_string(), tables, tc: dir.join("ccf.dat").is_file(), tm: dir.join("pid.dat").is_file() })
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
/// without wildcards, and `*` wildcards within one name. A file that
/// takes something back (`!`) isn't read at all -- `*` then `!*/` would
/// hide every folder -- nor is a bare `*`.
#[derive(Debug, Default)]
struct Ignore {
    /// Any folder of this name (`build`, `*.egg-info`).
    names: Vec<String>,
    /// This folder, relative to the project (`/out`, `docs/build`).
    paths: Vec<String>,
}

impl Ignore {
    fn read(root: &Path) -> Ignore {
        let Ok(text) = std::fs::read_to_string(root.join(".gitignore")) else { return Ignore::default() };
        Ignore::parse(&text)
    }

    fn parse(text: &str) -> Ignore {
        let mut out = Ignore::default();
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        if lines.iter().any(|l| l.starts_with('!')) {
            return out;
        }
        for line in lines {
            if line.contains("**") {
                continue;
            }
            let line = line.trim_end_matches('/');
            if line.trim_start_matches('/').chars().all(|c| c == '*') {
                continue;
            }
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
    if name.len() < first.len() + last.len() || !name.starts_with(first) || !name.ends_with(last) {
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

/// The folders of MIB tables in the project at `root`: its `mib/` and
/// its top looked at directly, links followed and whatever the bounds;
/// then the rest, breadth first within `bounds` -- hidden folders, build
/// output and dependencies (`SKIPPED`) and what the project's
/// `.gitignore` names left out, a linked table counted, a linked folder
/// not followed. Ordered as they're offered: `mib/`, the top, then by
/// depth and name.
pub fn walk(root: &Path, bounds: Bounds) -> Walk {
    let mut out = Walk::default();
    out.found.extend(probe(&root.join("mib"), "mib"));
    out.found.extend(probe(root, ""));
    let ignore = Ignore::read(root);
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
                // A linked table is one; a linked folder isn't followed:
                // it may lead out, or round.
                Ok(t) if t.is_symlink() && entry.path().is_file() => names.push(name.to_lowercase()),
                _ => {}
            }
            if out.entries >= bounds.entries {
                out.cut = true;
                break;
            }
        }
        if rel.is_empty() || rel == "mib" {
            // Looked at already.
        } else if let Some(found) = found_in(rel.clone(), &names) {
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
    /// In the project, at this path relative to it ("" its top): found,
    /// or picked by its path.
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
    /// What it's called: the setting's name; a folder of the project by
    /// its path in it -- `mib/` and the top by the project's name, as
    /// they always were, so settings by that name still apply.
    pub label: String,
    pub origin: Origin,
    /// Where it's read from; none when the extension can't reach it (yet).
    pub path: Option<PathBuf>,
    /// Where it is, as the settings say or the project has it.
    pub place: String,
    /// What's in it, as far as is cheap to say.
    pub what: String,
    /// In a folder of tests, samples and the like.
    pub incidental: bool,
}

impl Known {
    pub fn root(&self) -> Option<MibRoot> {
        self.path.clone().map(|path| MibRoot { label: self.label.clone(), path })
    }

    /// Found in the project, or named by its own settings: the doctor's.
    pub fn of_project(&self) -> bool {
        !matches!(self.origin, Origin::Yours)
    }

    pub fn named(&self) -> bool {
        !matches!(self.origin, Origin::Found(_))
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

/// A path relative to the project, when `value` (a setting's folder, a
/// typed path) is in it.
pub fn in_project(value: &str, project_path: Option<&str>) -> Option<String> {
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
/// those found and not named, and `picked`, a folder of the project
/// picked by its path that the walk didn't reach -- ordered for the
/// picker: the project's first, then its settings', then yours.
pub fn known(found: &[Found], settings: &[(String, String)], project_lists: bool, picked: Option<&str>, places: &Places) -> Vec<Known> {
    let mut found = found.to_vec();
    if let Some(rel) = picked.filter(|r| !found.iter().any(|f| f.rel == *r)) {
        let dir = if rel.is_empty() { places.project.to_path_buf() } else { places.project.join(rel) };
        found.extend(probe(&dir, rel));
    }
    let mut named = HashSet::new();
    let mut out = Vec::new();
    for (label, value) in settings {
        let rel = in_project(value, places.project_path);
        let found_here = rel.as_ref().and_then(|r| found.iter().find(|f| f.rel == *r));
        if let Some(f) = found_here {
            named.insert(f.rel.clone());
        }
        // Where Fenix put it; else, for one in the project, where it is.
        let path = crate::root_path(label, value, places.folders, places.project).or_else(|| found_here.map(|f| places.project.join(&f.rel)));
        let what = match (&path, found_here) {
            (_, Some(f)) => f.what(),
            (Some(p), None) if crate::is_xtce_file(p) => "an XTCE file".to_string(),
            (Some(p), None) => probe(p, "").map(|f| f.what()).unwrap_or_else(|| "no MIB tables in it".to_string()),
            (None, None) => "not given to the extension yet -- Fenix gives it the next time it starts the extension".to_string(),
        };
        out.push(Known { id: format!("set:{label}"), label: label.clone(), origin: if project_lists { Origin::Project } else { Origin::Yours }, path, place: value.clone(), what, incidental: false });
    }
    for f in found.iter().filter(|f| !named.contains(&f.rel)) {
        let label = if f.rel.is_empty() || f.rel == "mib" { places.project_name.to_string() } else { f.rel.clone() };
        let place = if f.rel.is_empty() { "the project's top".to_string() } else { format!("{}/", f.rel) };
        let path = if f.rel.is_empty() { places.project.to_path_buf() } else { places.project.join(&f.rel) };
        out.push(Known { id: format!("found:{}", f.rel), label, origin: Origin::Found(f.rel.clone()), path: Some(path), place, what: f.what(), incidental: f.incidental() });
    }
    out.sort_by_key(|k| match k.origin {
        Origin::Found(_) => 0,
        Origin::Project => 1,
        Origin::Yours => 2,
    });
    out
}

/// What's picked in a project.
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

    /// The folder of the project it's picked by, if it's one.
    pub fn found(&self) -> Option<&str> {
        match self {
            Choice::One(id) => id.strip_prefix("found:"),
            Choice::Settings => None,
        }
    }
}

/// The MIBs in use, and what that comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InUse<'a> {
    pub used: Vec<&'a Known>,
    /// What's picked, as it stands: the pick, or what the rule took.
    pub choice: Option<Choice>,
    /// None in use because several were found and none is sure: how many.
    pub undecided: usize,
}

/// The MIBs in use: what's picked (if it's still known and reachable);
/// else what the settings name; else a found one when there's no doubt
/// which -- `mib/`, the top, or the only one outside folders of tests,
/// samples and the like; else none.
pub fn in_use<'a>(known: &'a [Known], choice: Option<&Choice>) -> InUse<'a> {
    let reachable = |k: &&Known| k.path.is_some();
    let named: Vec<&Known> = known.iter().filter(|k| k.named()).filter(reachable).collect();
    let one = |k: &'a Known| InUse { used: vec![k], choice: Some(Choice::One(k.id.clone())), undecided: 0 };
    match choice {
        Some(Choice::One(id)) => {
            if let Some(k) = known.iter().filter(reachable).find(|k| k.id == *id) {
                return InUse { used: vec![k], choice: choice.cloned(), undecided: 0 };
            }
        }
        Some(Choice::Settings) if !named.is_empty() => return InUse { used: named, choice: choice.cloned(), undecided: 0 },
        _ => {}
    }
    if !named.is_empty() {
        return InUse { used: named, choice: Some(Choice::Settings), undecided: 0 };
    }
    let found: Vec<&Known> = known.iter().filter(|k| !k.named()).collect();
    for sure in ["mib", ""] {
        if let Some(k) = found.iter().find(|k| k.origin == Origin::Found(sure.to_string())) {
            return one(k);
        }
    }
    let likely: Vec<&&Known> = found.iter().filter(|k| !k.incidental).collect();
    if let [k] = likely.as_slice() {
        return one(k);
    }
    InUse { used: Vec::new(), choice: None, undecided: found.len() }
}

/// Where the MIBs in use come from, as the MIB page's header says it, and
/// the key that picks another when there are others; or that several
/// were found and none is in use.
pub fn source(in_use: &InUse, project_name: &str, known: usize) -> String {
    let from = match in_use.used.as_slice() {
        [] if in_use.undecided > 0 => return undecided_text(in_use.undecided, "M"),
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
    if known > 1 {
        format!("{from} · M: {known} MIBs")
    } else {
        from
    }
}

/// What it says when several MIBs were found and none is sure.
pub fn undecided_text(n: usize, keys: &str) -> String {
    if n == 1 {
        format!("1 MIB found here, among tests or samples -- {keys} picks it")
    } else {
        format!("{n} MIBs found here -- {keys} picks one")
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

/// A typed or pasted folder, as it goes in `mib.roots`: quotes and a
/// `file://` taken off, `.`, `..` and doubled or trailing separators
/// resolved. `~/…` stays so: Fenix makes it your home folder's, on every
/// machine you use. What can't be a MIB folder is refused, and why: not
/// a full path, the top of a disk, a drive's current folder (`C:mibs`),
/// a home folder.
pub fn typed_folder(text: &str) -> Result<String, String> {
    let t = text.trim().trim_matches(['"', '\'']).trim();
    let t = t.strip_prefix("file://").unwrap_or(t);
    let bytes = t.as_bytes();
    let (prefix, rest, sep) = if t == "~" || t.starts_with("~/") || t.starts_with("~\\") {
        ("~/".to_string(), t.get(2..).unwrap_or(""), '/')
    } else if t.starts_with("\\\\") || t.starts_with("//") && t.len() > 2 && !t[2..].starts_with('/') && cfg!(windows) {
        ("\\\\".to_string(), &t[2..], '\\')
    } else if t.starts_with('/') || t.starts_with('\\') {
        ("/".to_string(), &t[1..], '/')
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        match bytes.get(2) {
            Some(b'/' | b'\\') => (format!("{}\\", &t[..2]), &t[3..], '\\'),
            _ => return Err(format!("{t} is relative to a drive's current folder -- give its full path, {}\\…", &t[..2])),
        }
    } else {
        return Err(format!("{t} isn't a full path -- a MIB folder elsewhere is /…, ~/… or C:\\…"));
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in rest.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let shown = format!("{prefix}{}", parts.join(&sep.to_string()));
    let home = prefix == "~/" && parts.is_empty()
        || prefix == "/" && (matches!(parts.as_slice(), ["root"]) || matches!(parts.as_slice(), ["home" | "Users", _]))
        || prefix.ends_with(":\\") && matches!(parts.as_slice(), [u, _] if u.eq_ignore_ascii_case("users"));
    if home {
        return Err(format!("{} is a home folder -- a MIB folder is the one that holds its .dat tables", shown.trim_end_matches(['/', '\\'])));
    }
    if parts.is_empty() || prefix == "\\\\" && parts.len() < 2 {
        return Err(format!("{} is the top of a disk -- a MIB folder is the one that holds its .dat tables", shown));
    }
    Ok(shown)
}

/// Your `mib.roots` with `path` (as `typed_folder` makes it) in it: the
/// list to write, and the name it has there -- the one it had, when it
/// was there already (`None` then for the list: nothing to write).
pub fn with_folder(yours: &[(String, String)], path: &str, others: &[String]) -> (Option<Vec<(String, String)>>, String) {
    let same = |v: &str| typed_folder(v).is_ok_and(|v| v == path);
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

    fn rels(w: &Walk) -> Vec<&str> {
        w.found.iter().map(|f| f.rel.as_str()).collect()
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
        assert_eq!(rels(&walk(t.path(), BOUNDS)), ["mib", "", "a/old"]);
    }

    #[test]
    fn mib_and_the_top_are_found_whatever_the_walk_meets() {
        // More entries at the top than the walk looks at.
        let t = tree();
        mib(&t.path().join("mib"));
        let w = walk(t.path(), Bounds { depth: 6, folders: 3_000, entries: 2 });
        assert!(w.cut);
        assert_eq!(rels(&w), ["mib"]);
        // More folders than it reads, and a .gitignore that hides them all.
        let t = tree();
        mib(t.path());
        mib(&t.path().join("mib"));
        std::fs::write(t.path().join(".gitignore"), "mib*\n").unwrap();
        assert_eq!(rels(&walk(t.path(), Bounds { depth: 6, folders: 0, entries: 0 })), ["mib", ""]);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_mib_folder_and_linked_tables_are_found() {
        let t = tree();
        mib(&t.path().join("data/ops"));
        std::os::unix::fs::symlink(t.path().join("data/ops"), t.path().join("mib")).unwrap();
        std::fs::create_dir_all(t.path().join("db")).unwrap();
        for table in ["ccf.dat", "pid.dat"] {
            std::os::unix::fs::symlink(t.path().join("data/ops").join(table), t.path().join("db").join(table)).unwrap();
        }
        // A link round doesn't go round.
        std::os::unix::fs::symlink(t.path(), t.path().join("data/back")).unwrap();
        let w = walk(t.path(), BOUNDS);
        assert_eq!(rels(&w), ["mib", "db", "data/ops"]);
        assert_eq!(w.found[1].what(), "2 tables · TC and TM");
    }

    #[test]
    fn hidden_build_and_ignored_folders_are_left_out() {
        let t = tree();
        for d in [".git/x", "target/debug", "node_modules/m", "generated", "out/mib", "docs/build/mib", "eggs/x.egg-info"] {
            mib(&t.path().join(d));
        }
        mib(&t.path().join("keep"));
        std::fs::write(t.path().join(".gitignore"), "# built\n/out/\ndocs/build\n*.egg-info\n*\n").unwrap();
        assert_eq!(rels(&walk(t.path(), BOUNDS)), ["keep"]);
    }

    #[test]
    fn a_gitignore_that_takes_back_is_not_read() {
        let t = tree();
        mib(&t.path().join("db/ops"));
        std::fs::write(t.path().join(".gitignore"), "*\n!*/\n!*.dat\n").unwrap();
        assert_eq!(rels(&walk(t.path(), BOUNDS)), ["db/ops"]);
    }

    #[test]
    fn the_walk_stops_at_its_bounds() {
        let t = tree();
        mib(&t.path().join("a/b/c/d/e/f/g/h"));
        mib(&t.path().join("a/b/c"));
        let w = walk(t.path(), BOUNDS);
        assert_eq!(rels(&w), ["a/b/c"], "8 deep is past the 6 it goes");
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
            let k = known(&found, &[("OPS".into(), value.into())], true, None, &p);
            let ids: Vec<&str> = k.iter().map(|k| k.id.as_str()).collect();
            assert_eq!(ids, ["found:db/sim", "set:OPS"], "{value}");
            assert_eq!(k[1].what, "4 tables · TC and TM");
            assert_eq!(k[1].origin, Origin::Project);
        }
        // Not given to it where Fenix puts them: where it is.
        let k = known(&found, &[("db/x".into(), "db/sim".into())], true, None, &p);
        assert_eq!(k[1].path.as_deref(), Some(t.path().join("db/sim").as_path()));
        // Elsewhere: both.
        let k = known(&found, &[("OPS".into(), "/srv/mibs/ops".into())], false, None, &p);
        assert_eq!(k.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(), ["found:db/ops", "found:db/sim", "set:OPS"]);
        assert_eq!(k[2].origin, Origin::Yours);
    }

    #[test]
    fn mib_and_the_top_keep_the_projects_name() {
        let t = tree();
        mib(t.path());
        mib(&t.path().join("mib"));
        mib(&t.path().join("db/ops"));
        let found = walk(t.path(), BOUNDS).found;
        let k = known(&found, &[], false, None, &places(t.path(), Path::new("/nowhere")));
        let labels: Vec<(&str, &str)> = k.iter().map(|k| (k.label.as_str(), k.place.as_str())).collect();
        assert_eq!(labels, [("mission", "mib/"), ("mission", "the project's top"), ("db/ops", "db/ops/")]);
        assert_eq!(k[1].root().unwrap().path, t.path());
    }

    #[test]
    fn a_folder_of_the_project_picked_by_its_path_is_known() {
        let t = tree();
        mib(&t.path().join("deep/a/b/c/d/e/f/ops"));
        let found = walk(t.path(), BOUNDS).found;
        assert!(found.is_empty());
        let p = places(t.path(), Path::new("/nowhere"));
        let k = known(&found, &[], false, Some("deep/a/b/c/d/e/f/ops"), &p);
        assert_eq!(k[0].id, "found:deep/a/b/c/d/e/f/ops");
        let use_ = in_use(&k, Some(&Choice::One(k[0].id.clone())));
        assert_eq!(use_.used.len(), 1);
        assert!(known(&found, &[], false, Some("nothing/here"), &p).is_empty());
    }

    fn k(id: &str, origin: Origin, reachable: bool) -> Known {
        let incidental = matches!(&origin, Origin::Found(rel) if incidental(rel));
        Known { id: id.into(), label: id.into(), origin, path: reachable.then(|| PathBuf::from("/x")), place: String::new(), what: String::new(), incidental }
    }

    fn ids(u: InUse) -> Vec<String> {
        u.used.iter().map(|k| k.id.clone()).collect()
    }

    #[test]
    fn what_is_in_use() {
        let found = k("found:mib", Origin::Found("mib".into()), true);
        let other = k("found:db/old", Origin::Found("db/old".into()), true);
        let ops = k("set:OPS", Origin::Yours, true);
        let sim = k("set:SIM", Origin::Yours, true);
        let gone = k("set:NEW", Origin::Yours, false);
        let all = vec![found.clone(), other.clone(), ops.clone(), sim.clone(), gone.clone()];
        // Nothing picked: the settings', together; else mib/.
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
    fn several_found_and_nothing_sure_is_none_until_picked() {
        let top = k("found:", Origin::Found(String::new()), true);
        let a = k("found:db/ops", Origin::Found("db/ops".into()), true);
        let b = k("found:db/sim", Origin::Found("db/sim".into()), true);
        let fixture = k("found:crates/mib/tests/fixtures/ops", Origin::Found("crates/mib/tests/fixtures/ops".into()), true);
        let sample = k("found:examples/demo", Origin::Found("examples/demo".into()), true);
        // The top: sure.
        assert_eq!(ids(in_use(&[top.clone(), a.clone(), b.clone()], None)), ["found:"]);
        // One, not among tests or samples: sure.
        assert_eq!(ids(in_use(&[a.clone(), fixture.clone(), sample.clone()], None)), ["found:db/ops"]);
        // Two: none, and how many.
        let two = [a.clone(), b.clone()];
        let u = in_use(&two, None);
        assert!(u.used.is_empty() && u.choice.is_none());
        assert_eq!(u.undecided, 2);
        assert_eq!(source(&u, "mission", 2), "2 MIBs found here -- M picks one");
        // A fixture alone is never used unpicked.
        let alone = [fixture.clone()];
        let u = in_use(&alone, None);
        assert!(u.used.is_empty());
        assert_eq!(u.undecided, 1);
        assert_eq!(ids(in_use(&alone, Some(&Choice::One(fixture.id.clone())))), std::slice::from_ref(&fixture.id));
    }

    #[test]
    fn the_header_says_where_they_come_from_and_that_there_are_others() {
        let mib = k("found:mib", Origin::Found("mib".into()), true);
        let top = k("found:", Origin::Found(String::new()), true);
        let (p1, p2, y) = (k("a", Origin::Project, true), k("b", Origin::Project, true), k("c", Origin::Yours, true));
        fn u(used: Vec<&Known>) -> InUse<'_> {
            InUse { used, choice: None, undecided: 0 }
        }
        assert_eq!(source(&u(vec![&mib]), "mission", 1), "found in mib/");
        assert_eq!(source(&u(vec![&top]), "mission", 3), "found at the project's top · M: 3 MIBs");
        assert_eq!(source(&u(vec![&p1, &p2]), "mission", 2), "from mission's settings · M: 2 MIBs");
        assert_eq!(source(&u(vec![&y]), "mission", 1), "from your settings");
        assert_eq!(source(&u(vec![]), "mission", 2), "");
    }

    #[test]
    fn another_folder_gets_a_name_of_its_own() {
        assert_eq!(label_for("/srv/mibs/OPS/", &[]), "OPS");
        assert_eq!(label_for("/srv/mibs/OPS", &["ops".into(), "OPS-2".into()]), "OPS-3");
        assert_eq!(label_for("C:\\MIBs\\ops 2.1", &[]), "ops 2.1");
    }

    #[test]
    fn a_typed_folder_is_made_plain_or_refused() {
        assert_eq!(typed_folder(" '/srv/mibs/ops/' "), Ok("/srv/mibs/ops".into()));
        assert_eq!(typed_folder("/srv//mibs/./x/../ops"), Ok("/srv/mibs/ops".into()));
        assert_eq!(typed_folder("file:///srv/x"), Ok("/srv/x".into()));
        assert_eq!(typed_folder("~/mibs/"), Ok("~/mibs".into()));
        assert_eq!(typed_folder("C:\\MIBs\\OPS\\"), Ok("C:\\MIBs\\OPS".into()));
        assert_eq!(typed_folder("c:/MIBs/OPS"), Ok("c:\\MIBs\\OPS".into()));
        for bad in ["~", "~/", "/", "//", "/home/me", "/Users/me/", "/root", "C:", "C:\\", "C:\\Users\\me", "C:mibs", "mibs/ops", "", "/srv/.."] {
            assert!(typed_folder(bad).is_err(), "{bad}: {:?}", typed_folder(bad));
        }
        assert!(typed_folder("/home/me").unwrap_err().contains("home folder"));
        assert!(typed_folder("/").unwrap_err().contains("top of a disk"));
        assert!(typed_folder("C:mibs").unwrap_err().contains("drive's current folder"));
    }

    #[test]
    fn a_folder_is_added_to_your_list_once() {
        let yours = vec![("OPS".to_string(), "/srv/a/OPS".to_string())];
        let (list, label) = with_folder(&yours, "/srv/b/OPS", &["mib".into()]);
        assert_eq!(label, "OPS-2");
        assert_eq!(list.unwrap(), vec![("OPS".into(), "/srv/a/OPS".into()), ("OPS-2".into(), "/srv/b/OPS".into())]);
        let yours = vec![("OPS".to_string(), "/srv/a//OPS/".to_string())];
        let (list, label) = with_folder(&yours, "/srv/a/OPS", &[]);
        assert_eq!((list, label.as_str()), (None, "OPS"), "there already, however it was spelt");
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
    fn incidental_folders() {
        for rel in ["crates/mib/tests/fixtures/ops", "test/mib", "examples/ops", "samples", "vendor/x/mib", "third_party/mib", "demo"] {
            assert!(incidental(rel), "{rel}");
        }
        for rel in ["db/ops", "mission/mib", "contest/ops"] {
            assert!(!incidental(rel), "{rel}");
        }
    }
}
