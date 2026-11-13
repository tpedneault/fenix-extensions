//! Where a test is, as `go test` names it: its package's import path, from
//! the module's path in the nearest `go.mod` and the file's folder, and
//! the keys and groups built from it, which a scan, a listing and a run
//! must all build alike or Fenix won't know the id.
//!
//! A project is one module (`go.mod` at its top), several of them in a
//! `go.work`, or a folder of modules with no `go.work` (each its own
//! `go test` step, in its own folder). A scan reads the `go.mod` it needs
//! itself, with `std::fs` under the project's folder, before Go has said
//! a word.

use std::collections::HashMap;

use fenix_lang_kit::tests::Group;

use crate::project::GoMod;

/// A module: its path and where its `go.mod` is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Module {
    /// `example.com/orbit`.
    pub path: String,
    /// The folder of its `go.mod`, below the project: `""` for the top,
    /// `lib` or `services/api`.
    pub dir: String,
}

/// A package's key: `example.com/orbit TestTable/negative_one`.
pub fn key(package: &str, name: &str) -> String {
    format!("{package} {name}")
}

/// A key's package and test name (subtests included): `None` for what
/// isn't one (a group's id starts with U+001F, and no import path holds a
/// space).
pub fn parse_key(key: &str) -> Option<(&str, &str)> {
    let (package, name) = key.split_once(' ')?;
    (!package.is_empty() && !name.is_empty() && !package.starts_with('\u{1f}')).then_some((package, name))
}

/// A package's group, the root of each of its tests' groups.
pub fn package_group(package: &str) -> Group {
    Group::package(package)
}

/// `name` for a `-run` pattern: Go's `regexp.QuoteMeta`.
pub fn quote_meta(name: &str) -> String {
    let special = |c: char| matches!(c, '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$');
    if !name.chars().any(special) {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len() + 4);
    for c in name.chars() {
        if special(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A folder `go` leaves out of `./...`: `testdata`, `vendor`, and what
/// starts with `_` or `.`, at any depth of `dir` (below its module).
pub fn ignored(dir: &str) -> bool {
    dir.split('/').any(|part| part == "testdata" || part == "vendor" || part.starts_with('_') || (part.starts_with('.') && part.len() > 1))
}

/// `a/b` and `c` as `a/b/c`; either may be empty.
pub fn join(dir: &str, name: &str) -> String {
    match (dir.is_empty(), name.is_empty()) {
        (true, _) => name.into(),
        (_, true) => dir.into(),
        _ => format!("{dir}/{name}"),
    }
}

/// `path` with `.` and `..` resolved, `/`-separated; `None` when it climbs
/// out of where it started.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// Whether `path` is a full one: `/home/u/x`, `C:/x`, `//server/share`.
pub fn is_full(path: &str) -> bool {
    path.starts_with('/') || (path.len() > 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[2] == b'/')
}

impl Module {
    /// The package a folder below the project is, when it's in this
    /// module: `lib/util` of module `example.com/lib` at `lib` is
    /// `example.com/lib/util`.
    pub fn package_of(&self, dir: &str) -> Option<String> {
        let rest = if self.dir.is_empty() { dir } else { dir.strip_prefix(&self.dir)?.strip_prefix('/').or_else(|| (dir == self.dir).then_some(""))? };
        Some(join(&self.path, rest))
    }

    /// A package's folder below the project, when it's in this module.
    pub fn dir_of(&self, package: &str) -> Option<String> {
        let rest = if package == self.path { "" } else { package.strip_prefix(&self.path)?.strip_prefix('/')? };
        Some(join(&self.dir, rest))
    }
}

/// The `use`d folders of a `go.work`, `./`-less, `.` as `""`: `use ./app`,
/// `use ( ./lib "./other dir" )`. Folders outside the project (`..`) are
/// left out.
pub fn parse_work(text: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut block: Option<String> = None;
    for raw in text.lines() {
        let code = raw.split("//").next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        if code == ")" {
            block = None;
            continue;
        }
        let (verb, rest) = match &block {
            Some(v) => (v.clone(), code),
            None => {
                let (verb, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
                if rest.trim() == "(" {
                    block = Some(verb.to_string());
                    continue;
                }
                (verb.to_string(), rest.trim())
            }
        };
        if verb != "use" {
            continue;
        }
        let word = rest.trim().trim_matches(|c| c == '"' || c == '`');
        if let Some(dir) = normalize(&word.replace('\\', "/")) {
            if !word.starts_with("..") && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// The modules of the project, found as they're needed and kept until
/// `go.mod` or `go.work` changes ([`Modules::forget`]).
#[derive(Debug, Default)]
pub struct Modules {
    /// The project's folder on this machine (`/project`).
    pub root: String,
    /// By folder below the project: the nearest module at or above it.
    nearest: HashMap<String, Option<Module>>,
    /// The `go.work`'s folders, once read: `None` for no `go.work`.
    work: Option<Option<Vec<String>>>,
}

impl Modules {
    pub fn new(root: &str) -> Modules {
        Modules { root: root.trim_end_matches('/').into(), ..Modules::default() }
    }

    pub fn forget(&mut self) {
        self.nearest.clear();
        self.work = None;
    }

    fn read(&self, dir: &str, name: &str) -> Option<String> {
        std::fs::read_to_string(format!("{}/{}", self.root, join(dir, name))).ok()
    }

    /// The module `dir` (below the project) is in: the nearest `go.mod`
    /// at or above it, no higher than the project. `None` outside any.
    pub fn of_dir(&mut self, dir: &str) -> Option<Module> {
        if let Some(m) = self.nearest.get(dir) {
            return m.clone();
        }
        let found = match self.read(dir, "go.mod").and_then(|t| GoMod::parse(&t).module) {
            Some(path) => Some(Module { path, dir: dir.into() }),
            None if dir.is_empty() => None,
            None => self.of_dir(dir.rsplit_once('/').map_or("", |(parent, _)| parent)),
        };
        self.nearest.insert(dir.into(), found.clone());
        found
    }

    /// The `go.work`'s `use`d folders: `None` when there's no `go.work`.
    pub fn work(&mut self) -> Option<Vec<String>> {
        if self.work.is_none() {
            self.work = Some(self.read("", "go.work").map(|t| parse_work(&t)));
        }
        self.work.clone().flatten()
    }

    /// A package's folder, from the modules known: the project's own, and
    /// a `go.work`'s.
    pub fn package_dir(&mut self, package: &str) -> Option<String> {
        let mut modules = self.work_modules();
        if let Some(top) = self.of_dir("") {
            modules.push(top);
        }
        modules.iter().filter_map(|m| m.dir_of(package).map(|d| (m.path.len(), d))).max_by_key(|(len, _)| *len).map(|(_, d)| d)
    }

    /// The modules of a `go.work`, as its folders say.
    pub fn work_modules(&mut self) -> Vec<Module> {
        let dirs = self.work().unwrap_or_default();
        dirs.iter().filter_map(|d| self.of_dir(d).filter(|m| m.dir == *d)).collect()
    }
}

/// What the planner needs of the project's modules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// The project's modules: the top one's, a `go.work`'s, and where the
    /// scans found tests, sorted.
    pub modules: Vec<Module>,
    /// A `go.work`'s folders: Go runs them all from the project's folder,
    /// and a package of one is named by its import path from anywhere.
    pub work: Option<Vec<String>>,
}

impl Layout {
    pub fn of(modules: &mut Modules, seen: Vec<Module>) -> Layout {
        let work = modules.work();
        let mut all = if work.is_some() { modules.work_modules() } else { Vec::new() };
        all.extend(modules.of_dir("").filter(|m| m.dir.is_empty()));
        all.extend(seen);
        all.sort();
        all.dedup();
        Layout { modules: all, work }
    }

    /// The module a package is in: the one whose path is the longest
    /// prefix of its import path.
    pub fn module_of(&self, package: &str) -> Option<&Module> {
        self.modules.iter().filter(|m| m.dir_of(package).is_some()).max_by_key(|m| m.path.len())
    }

    /// The folder `go` runs a package's tests from, below the project: the
    /// project's own with a `go.work`, else its module's.
    pub fn cwd_of(&self, package: &str) -> String {
        if self.work.is_some() {
            return String::new();
        }
        self.module_of(package).map(|m| m.dir.clone()).unwrap_or_default()
    }

    /// A package's folder below the project.
    pub fn dir_of(&self, package: &str) -> Option<String> {
        self.module_of(package)?.dir_of(package)
    }
}

/// The folder a step runs in, as its command says it: `/project/lib`.
pub fn project_dir(dir: &str) -> String {
    if dir.is_empty() {
        "/project".into()
    } else {
        format!("/project/{dir}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_the_import_path_then_the_name() {
        assert_eq!(key("example.com/orbit", "TestTable/negative_one"), "example.com/orbit TestTable/negative_one");
        assert_eq!(parse_key("example.com/orbit TestTable/negative_one"), Some(("example.com/orbit", "TestTable/negative_one")));
        assert_eq!(parse_key("\u{1f}example.com/orbit"), None);
        assert_eq!(parse_key("nospace"), None);
    }

    #[test]
    fn a_name_is_quoted_as_regexp_quote_meta_does() {
        assert_eq!(quote_meta("a+b_(c)"), "a\\+b_\\(c\\)");
        assert_eq!(quote_meta("x|y"), "x\\|y");
        assert_eq!(quote_meta("[z]"), "\\[z\\]");
        assert_eq!(quote_meta("naïve"), "naïve");
        assert_eq!(quote_meta("seed#0"), "seed#0", "# is no metacharacter");
        assert_eq!(quote_meta("a\\b.c^d$e{1}f*g?h"), "a\\\\b\\.c\\^d\\$e\\{1\\}f\\*g\\?h");
    }

    #[test]
    fn go_leaves_some_folders_out() {
        assert!(ignored("testdata") && ignored("a/testdata/b") && ignored("vendor/x") && ignored("_old") && ignored("x/.hidden"));
        assert!(!ignored("") && !ignored("geo") && !ignored("a/b/c") && !ignored("my.pkg"));
    }

    #[test]
    fn a_module_names_its_packages_and_their_folders() {
        let top = Module { path: "example.com/orbit".into(), dir: String::new() };
        assert_eq!(top.package_of(""), Some("example.com/orbit".into()));
        assert_eq!(top.package_of("geo"), Some("example.com/orbit/geo".into()));
        assert_eq!(top.dir_of("example.com/orbit/geo"), Some("geo".into()));
        assert_eq!(top.dir_of("example.com/orbit"), Some(String::new()));
        assert_eq!(top.dir_of("example.com/orbital"), None, "a prefix isn't a parent");
        let lib = Module { path: "example.com/lib".into(), dir: "lib".into() };
        assert_eq!(lib.package_of("lib/util"), Some("example.com/lib/util".into()));
        assert_eq!(lib.package_of("lib"), Some("example.com/lib".into()));
        assert_eq!(lib.package_of("library"), None);
        assert_eq!(lib.package_of("app"), None);
        assert_eq!(lib.dir_of("example.com/lib/util"), Some("lib/util".into()));
    }

    #[test]
    fn paths_are_normalised_and_full_ones_told() {
        assert_eq!(normalize("./a/../b//c"), Some("b/c".into()));
        assert_eq!(normalize("../x"), None);
        assert!(is_full("/home/u/x.go") && is_full("C:/Users/u/x.go") && !is_full("geo/x.go") && !is_full("./x.go") && !is_full("x.go"));
    }

    #[test]
    fn a_go_work_says_its_folders() {
        let text = "go 1.22\n\ntoolchain go1.24.7\n\nuse (\n\t./app\n\t\"./other dir\" // two words\n\t.\n\t../outside\n)\nuse ./lib\nreplace a => b\n";
        assert_eq!(parse_work(text), ["app", "other dir", "", "lib"]);
        assert_eq!(parse_work(""), Vec::<String>::new());
    }
}
