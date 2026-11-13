//! What the provider's own scans found, by package: each test's name,
//! which are templates, which files hold them, and where each package's
//! folder is. A reader uses it to place a test a run finds that no scan
//! did (a test in a file Fenix never opened, a suite's method), to know
//! which results are of a package it has a group for (a `broken` naming
//! any other fails the step), and to turn the full paths Go prints into
//! paths below the project.
//!
//! lang-kit remembers scans too, but only for keys it's given; this is
//! the pack's, shared with its readers (an `Rc`), so a reader made for a
//! step sees the scans made while it reads.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use super::layout::{is_full, join, normalize, Module, Modules};

#[derive(Debug, Default)]
struct FileEntry {
    package: String,
    tests: Vec<String>,
}

/// A test a scan found.
#[derive(Debug, Clone)]
pub struct Known {
    pub template: bool,
    /// The file it's in, below the project, and its line there.
    pub file: Rc<str>,
    pub line: u32,
    /// Why `go test` doesn't build its file here, when it surely doesn't.
    pub excluded: Option<Rc<str>>,
}

/// A package a scan found tests in.
#[derive(Debug, Default)]
pub struct Package {
    /// Its folder below the project.
    pub dir: String,
    /// Its test files, below the project.
    pub files: BTreeSet<String>,
    /// Each test by name.
    pub tests: HashMap<String, Known>,
}

#[derive(Debug, Default)]
pub struct Index {
    /// The project's modules, read as they're needed.
    pub modules: Modules,
    files: HashMap<String, FileEntry>,
    packages: HashMap<String, Package>,
    /// The folders of the project on the machine Go ran on (`/home/u/orbit`),
    /// learned from a path Go printed for a file the scan knows.
    roots: Vec<String>,
    /// The modules of the files scanned.
    seen: BTreeSet<Module>,
    /// `platform()`'s `os` (`linux`, `macos`, `windows`), for the files
    /// Go doesn't build here; empty before it's known.
    pub os: String,
}

pub type Shared = Rc<RefCell<Index>>;

impl Index {
    pub fn new(root: &str) -> Index {
        Index { modules: Modules::new(root), ..Index::default() }
    }

    /// The project's files changed: what was read of them is read again.
    pub fn forget(&mut self) {
        self.modules.forget();
    }

    /// File `file`'s scan: its package, that package's folder, its module
    /// and its tests' names (and whether each is a template). Replaces its
    /// last.
    pub fn scanned(&mut self, file: &str, module: &Module, package: &str, dir: &str, tests: Vec<(String, Known)>) {
        self.unscan(file);
        let entry = FileEntry { package: package.into(), tests: tests.iter().map(|(n, _)| n.clone()).collect() };
        let p = self.packages.entry(package.into()).or_default();
        p.dir = dir.into();
        p.files.insert(file.into());
        for (name, known) in tests {
            p.tests.insert(name, known);
        }
        self.files.insert(file.into(), entry);
        self.seen.insert(module.clone());
    }

    /// `file` holds no tests any more (or isn't one `go test` reads).
    pub fn unscan(&mut self, file: &str) {
        let Some(old) = self.files.remove(file) else { return };
        let Some(p) = self.packages.get_mut(&old.package) else { return };
        for name in &old.tests {
            if p.tests.get(name).is_some_and(|k| &*k.file == file) {
                p.tests.remove(name);
            }
        }
        p.files.remove(file);
        if p.files.is_empty() {
            self.packages.remove(&old.package);
        }
    }

    pub fn is_scanned(&self, file: &str) -> bool {
        self.files.contains_key(file)
    }

    /// Whether a scan found a test file in `package`.
    pub fn knows(&self, package: &str) -> bool {
        self.packages.contains_key(package)
    }

    pub fn package(&self, package: &str) -> Option<&Package> {
        self.packages.get(package)
    }

    /// A test a scan found.
    pub fn test(&self, package: &str, name: &str) -> Option<&Known> {
        self.packages.get(package)?.tests.get(name)
    }

    /// The tests of `package` whose files `go test` doesn't build here, by
    /// name with the reason, in name order.
    pub fn excluded_in(&self, package: &str) -> Vec<(String, Rc<str>)> {
        let mut out: Vec<(String, Rc<str>)> = self.packages.get(package).map(|p| p.tests.iter().filter_map(|(n, k)| k.excluded.clone().map(|why| (n.clone(), why))).collect()).unwrap_or_default();
        out.sort();
        out
    }

    /// The modules the scans were in: the top one's, `go.work`'s and the
    /// nested ones' tests.
    pub fn modules_seen(&self) -> Vec<Module> {
        self.seen.iter().cloned().collect()
    }

    /// A path Go printed, as a path below the project. `package_dir` is
    /// the folder of the package whose test printed it: `-fullpath` prints
    /// `/home/u/orbit/geo/geo_test.go`, a compiler error
    /// `geo/geo_test.go` or `./geo_test.go` (from the folder the step ran
    /// in, `cwd`), a panic's frame the full path of the file. Where the
    /// file is one the scan found in that package, the folder tells where
    /// the project is on Go's machine, and later paths there are made
    /// relative to it. A full path of a file nothing places stays as it
    /// is: Fenix takes a full path of the project's machine.
    pub fn place(&mut self, path: &str, package_dir: Option<&str>, cwd: &str) -> String {
        let path = path.replace('\\', "/");
        if !is_full(&path) {
            let bare = !path.contains('/');
            return match (bare, package_dir) {
                (true, Some(dir)) => join(dir, &path),
                _ => normalize(&join(cwd, &path)).unwrap_or(path),
            };
        }
        let (dir, base) = path.rsplit_once('/').unwrap_or(("", &path));
        if let Some(pdir) = package_dir {
            let in_package = self.packages.values().any(|p| p.dir == pdir && p.files.iter().any(|f| f.rsplit('/').next() == Some(base)));
            if in_package {
                let root = if pdir.is_empty() { Some(dir) } else { dir.strip_suffix(pdir).and_then(|r| r.strip_suffix('/')) };
                if let Some(root) = root {
                    if !self.roots.iter().any(|r| r == root) {
                        self.roots.push(root.into());
                    }
                    return join(pdir, base);
                }
            }
        }
        for root in &self.roots {
            if let Some(rest) = path.strip_prefix(root.as_str()).and_then(|r| r.strip_prefix('/')) {
                return rest.into();
            }
        }
        path
    }

    /// The place of a frame in package `package`'s own files: its folder
    /// and the file's name.
    pub fn place_in_package(&mut self, path: &str, package: &str) -> String {
        let dir = self.packages.get(package).map(|p| p.dir.clone()).or_else(|| self.modules.package_dir(package));
        self.place(path, dir.as_deref(), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(name: &str, template: bool, line: u32, file: &str) -> (String, Known) {
        (name.into(), Known { template, file: file.into(), line, excluded: None })
    }

    fn module() -> Module {
        Module { path: "example.com/orbit".into(), dir: String::new() }
    }

    #[test]
    fn a_scan_replaces_its_files_last() {
        let mut i = Index::new("/nowhere");
        i.scanned("a_test.go", &module(), "example.com/orbit", "", vec![known("TestA", false, 3, "a_test.go"), known("TestB", true, 7, "a_test.go")]);
        i.scanned("b_test.go", &module(), "example.com/orbit", "", vec![known("TestC", false, 3, "b_test.go")]);
        assert_eq!(i.test("example.com/orbit", "TestB").map(|k| (k.template, &*k.file, k.line)), Some((true, "a_test.go", 7)));
        i.scanned("a_test.go", &module(), "example.com/orbit", "", vec![known("TestA", false, 3, "a_test.go")]);
        assert!(i.test("example.com/orbit", "TestB").is_none());
        assert!(i.test("example.com/orbit", "TestC").is_some());
        i.unscan("b_test.go");
        i.unscan("a_test.go");
        assert!(!i.knows("example.com/orbit"), "no file, no package");
    }

    #[test]
    fn go_s_paths_are_placed_below_the_project() {
        let mut i = Index::new("/nowhere");
        i.scanned("geo/geo_test.go", &module(), "example.com/orbit/geo", "geo", vec![known("TestDist", false, 5, "geo/geo_test.go")]);
        i.scanned("orbit_test.go", &module(), "example.com/orbit", "", vec![known("TestAdds", false, 8, "orbit_test.go")]);
        // A bare name, as Go prints it without -fullpath; a compiler's path from the step's folder.
        assert_eq!(i.place("geo_test.go", Some("geo"), ""), "geo/geo_test.go");
        assert_eq!(i.place("geo/geo_test.go", None, ""), "geo/geo_test.go");
        assert_eq!(i.place("./x_test.go", None, "lib"), "lib/x_test.go");
        assert_eq!(i.place("../util/u.go", None, "lib/app"), "lib/util/u.go");
        // A full path of a file the scan knows says where the project is.
        assert_eq!(i.place("/home/u/orbit/geo/geo_test.go", Some("geo"), ""), "geo/geo_test.go");
        assert_eq!(i.place("/home/u/orbit/internal/h/helper.go", Some("geo"), ""), "internal/h/helper.go", "then so for a file it doesn't");
        assert_eq!(i.place("/usr/local/go/src/testing/testing.go", None, ""), "/usr/local/go/src/testing/testing.go", "outside it, as it is");
        // Windows.
        let mut w = Index::new("/nowhere");
        w.scanned("geo/geo_test.go", &module(), "example.com/orbit/geo", "geo", vec![]);
        assert_eq!(w.place("C:\\Users\\u\\orbit\\geo\\geo_test.go", Some("geo"), ""), "geo/geo_test.go");
        assert_eq!(w.place("C:\\Users\\u\\orbit\\util\\u.go", None, ""), "util/u.go");
        // The top package's file is in the project's folder itself.
        let mut t = Index::new("/nowhere");
        t.scanned("orbit_test.go", &module(), "example.com/orbit", "", vec![]);
        assert_eq!(t.place("/home/u/orbit/orbit_test.go", Some(""), ""), "orbit_test.go");
        assert_eq!(t.place("/home/u/orbit/sub/x.go", Some(""), ""), "sub/x.go");
    }
}
