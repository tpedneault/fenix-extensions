//! What the pack knows of the project, from its files: its packages and
//! their components -- each `.cabal` file's (the one at the top, or those
//! `cabal.project` and `stack.yaml` list), or hpack's `package.yaml` where
//! there's no `.cabal` beside it -- whether Stack builds it, the
//! formatters' and hlint's configs, and, with no package at all, the
//! Haskell files at its top, each run on its own.
//!
//! A `.cabal` file is read lightly: its `name:` and the sections at its
//! left margin (`executable orbit`, `test-suite spec`), never its
//! conditionals. `package.yaml` is YAML the pack reads as far as its
//! top-level keys and the names under `executables:`, `tests:` and
//! `benchmarks:`.

use std::path::Path;

/// How many lone files get tasks, how much of each is read to see whether
/// it has a `main`, and how many packages a project's list is read for.
pub const LONE_FILES: usize = 20;
const READ_BYTES: usize = 256 << 10;
const PACKAGES: usize = 50;

/// What a component is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Library,
    Executable,
    Test,
    Benchmark,
}

impl Kind {
    /// Its prefix in a Cabal target (`exe:orbit`) and a Stack one
    /// (`orbit:test:spec`).
    pub fn target(self) -> &'static str {
        match self {
            Kind::Library => "lib",
            Kind::Executable => "exe",
            Kind::Test => "test",
            Kind::Benchmark => "bench",
        }
    }
}

/// A library, an executable, a test suite or a benchmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub kind: Kind,
    pub name: String,
}

impl Component {
    pub fn new(kind: Kind, name: &str) -> Component {
        Component { kind, name: name.into() }
    }
}

/// A package: its name and components, and its folder under the project
/// (empty at the top).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    pub name: Option<String>,
    pub dir: String,
    pub components: Vec<Component>,
    /// Read from `package.yaml`: hpack makes its `.cabal` file, which Stack
    /// runs it for.
    pub hpack: bool,
}

impl Package {
    pub fn of(&self, kind: Kind) -> impl Iterator<Item = &Component> {
        self.components.iter().filter(move |c| c.kind == kind)
    }
}

/// A `.cabal` file's name and components. A library without a name of its
/// own is the package's.
pub fn parse_cabal(text: &str) -> Package {
    let mut p = Package::default();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) || line.trim_start().starts_with("--") {
            continue;
        }
        let line = line.trim_end();
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("name:") {
            let at = line.len() - rest.len();
            p.name = Some(line[at..].trim().to_string()).filter(|n| !n.is_empty());
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(section), name) = (words.next(), words.next()) else { continue };
        let kind = match section.to_ascii_lowercase().as_str() {
            "library" => Kind::Library,
            "executable" => Kind::Executable,
            "test-suite" => Kind::Test,
            "benchmark" => Kind::Benchmark,
            _ => continue,
        };
        match name {
            Some(name) => p.components.push(Component::new(kind, name)),
            // The package's own library: named once its name is known.
            None if kind == Kind::Library => p.components.push(Component::new(kind, "")),
            None => {}
        }
    }
    let name = p.name.clone().unwrap_or_default();
    for c in p.components.iter_mut().filter(|c| c.name.is_empty()) {
        c.name = name.clone();
    }
    p.components.retain(|c| !c.name.is_empty());
    p
}

/// The value of a YAML line's key, unquoted.
fn yaml_value(value: &str) -> String {
    let value = value.split(" #").next().unwrap_or_default().trim();
    value.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// hpack's `package.yaml`: its `name`, a `library`, and the names under
/// `executables`, `tests` and `benchmarks` (or one `executable`, the
/// package's).
pub fn parse_package_yaml(text: &str) -> Package {
    let mut p = Package::default();
    let mut section: Option<Kind> = None;
    let mut child_indent: Option<usize> = None;
    for line in text.lines() {
        let code = line.trim_end();
        if code.trim().is_empty() || code.trim_start().starts_with('#') {
            continue;
        }
        let indent = code.len() - code.trim_start().len();
        if indent == 0 {
            child_indent = None;
            let Some((key, value)) = code.split_once(':') else {
                section = None;
                continue;
            };
            section = match key.trim() {
                "name" => {
                    p.name = Some(yaml_value(value)).filter(|n| !n.is_empty());
                    None
                }
                "library" => {
                    p.components.push(Component::new(Kind::Library, ""));
                    None
                }
                "executable" => {
                    p.components.push(Component::new(Kind::Executable, ""));
                    None
                }
                "executables" => Some(Kind::Executable),
                "tests" => Some(Kind::Test),
                "benchmarks" => Some(Kind::Benchmark),
                _ => None,
            };
            continue;
        }
        let Some(kind) = section else { continue };
        let depth = *child_indent.get_or_insert(indent);
        if indent != depth {
            continue;
        }
        if let Some((name, _)) = code.trim().split_once(':') {
            p.components.push(Component::new(kind, &yaml_value(name)));
        }
    }
    let name = p.name.clone().unwrap_or_default();
    for c in p.components.iter_mut().filter(|c| c.name.is_empty()) {
        c.name = name.clone();
    }
    p.components.retain(|c| !c.name.is_empty());
    p
}

/// The entries of a field that may go on over the lines after it, more
/// indented: `packages:` in `cabal.project` and `stack.yaml`. YAML's
/// `- ` and commas between them taken off.
fn field_entries(text: &str, field: &str) -> Option<Vec<String>> {
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some(rest) = line.strip_prefix(field).and_then(|r| r.trim_start().strip_prefix(':')) else { continue };
        let mut words: Vec<String> = Vec::new();
        let mut take = |text: &str| {
            let text = text.split("--").next().unwrap_or_default().split(" #").next().unwrap_or_default();
            words.extend(text.split([',', ' ', '\t']).map(|w| w.trim_matches(|c| c == '"' || c == '\'' || c == '-')).filter(|w| !w.is_empty() && !w.starts_with('#')).map(str::to_string));
        };
        take(rest);
        while let Some(next) = lines.next_if(|l| l.starts_with([' ', '\t']) || l.starts_with("- ") || l.trim().is_empty()) {
            take(next.trim().trim_start_matches("- "));
        }
        return Some(words);
    }
    None
}

/// The `.cabal` files in folder `dir` (under `root`), sorted.
fn cabal_files(root: &Path, dir: &str) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(root.join(dir)).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".cabal") && !n.starts_with('.')).collect();
    files.sort();
    files.into_iter().map(|f| if dir.is_empty() { f } else { format!("{dir}/{f}") }).collect()
}

/// A package's folder as written (`./orbit/`, `.`): as a path under the
/// project, empty at the top.
fn clean_dir(dir: &str) -> String {
    let d = dir.replace('\\', "/");
    let d = d.trim_start_matches("./").trim_end_matches('/');
    if d == "." { String::new() } else { d.to_string() }
}

/// The package in folder `dir`: its `.cabal` file, else its
/// `package.yaml`.
fn package_in(root: &Path, dir: &str) -> Option<Package> {
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let mut p = match cabal_files(root, dir).first() {
        Some(file) => parse_cabal(&read(file)?),
        None => Package { hpack: true, ..parse_package_yaml(&read(&if dir.is_empty() { "package.yaml".into() } else { format!("{dir}/package.yaml") })?) },
    };
    p.dir = dir.to_string();
    Some(p)
}

/// The folders an entry of `packages:` names: itself, the folder of a
/// `.cabal` file, or each folder a `*` at its end matches (`*/`,
/// `libs/*/`, `*/*.cabal`). Other globs aren't read.
fn entry_dirs(root: &Path, entry: &str) -> Vec<String> {
    let entry = entry.replace('\\', "/");
    let entry = match entry.strip_suffix(".cabal") {
        Some(file) => file.rsplit_once('/').map_or("", |(dir, _)| dir).to_string(),
        None => entry,
    };
    let entry = entry.trim_end_matches('/');
    let (base, last) = entry.rsplit_once('/').unwrap_or(("", entry));
    if last != "*" {
        return if entry.contains('*') { Vec::new() } else { vec![clean_dir(entry)] };
    }
    let base = clean_dir(base);
    let mut out: Vec<String> = std::fs::read_dir(root.join(&base))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && n != "dist-newstyle")
        .map(|n| if base.is_empty() { n } else { format!("{base}/{n}") })
        .collect();
    out.sort();
    out
}

/// A Haskell file at the project's top, in a project without a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lone {
    pub file: String,
    /// It has a `main`: it runs.
    pub main: bool,
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `cabal.project` at the top.
    pub cabal_project: bool,
    /// `stack.yaml` at the top: Stack builds it.
    pub stack: bool,
    pub packages: Vec<Package>,
    /// fourmolu's config, `fourmolu.yaml`, at the top.
    pub fourmolu_yaml: bool,
    pub lone: Vec<Lone>,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let cabal_project = text("cabal.project");
        let stack = text("stack.yaml");
        // The package folders the project lists; the top when it lists
        // none.
        let dirs: Vec<String> = match (&stack, &cabal_project) {
            (Some(s), _) => field_entries(s, "packages").map(|e| e.iter().flat_map(|e| entry_dirs(root, e)).collect()),
            (None, Some(c)) => field_entries(c, "packages").map(|e| e.iter().flat_map(|e| entry_dirs(root, e)).collect()),
            _ => None,
        }
        .unwrap_or_else(|| vec![String::new()]);
        let mut packages: Vec<Package> = Vec::new();
        for dir in dirs.into_iter().take(PACKAGES) {
            if let Some(p) = package_in(root, &dir).filter(|p| !packages.iter().any(|q| q.dir == p.dir)) {
                packages.push(p);
            }
        }
        let mut lone = Vec::new();
        if packages.is_empty() {
            let mut files: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".hs") && root.join(n).is_file()).collect();
            files.sort();
            for file in files.into_iter().take(LONE_FILES) {
                let text = std::fs::read(root.join(&file)).map(|b| String::from_utf8_lossy(&b[..b.len().min(READ_BYTES)]).into_owned()).unwrap_or_default();
                lone.push(Lone { main: text.lines().any(|l| l.starts_with("main ") || l.starts_with("main=")), file });
            }
        }
        Project { cabal_project: cabal_project.is_some(), stack: stack.is_some(), packages, fourmolu_yaml: root.join("fourmolu.yaml").is_file(), lone }
    }

    /// Whether it's a Haskell project at all: a package, or a Haskell file.
    pub fn is_haskell(&self) -> bool {
        self.cabal_project || self.stack || !self.packages.is_empty() || !self.lone.is_empty()
    }

    /// Whether Stack is its tool, as far as its files say: a `stack.yaml`,
    /// or packages hpack's `package.yaml` says, with no `.cabal` file for
    /// Cabal to read.
    pub fn stack_project(&self) -> bool {
        self.stack || (!self.packages.is_empty() && self.packages.iter().all(|p| p.hpack))
    }

    /// Every component of `kind`, with its package, across the packages.
    pub fn components(&self, kind: Kind) -> Vec<(&Package, &Component)> {
        self.packages.iter().flat_map(|p| p.of(kind).map(move |c| (p, c))).collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-haskell-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A Cabal package as `cabal init` makes it, and more: a library, an
    /// executable, a test suite and a benchmark.
    pub fn cabal_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("orbit.cabal"), include_str!("samples/orbit.cabal")).unwrap();
        dir
    }

    fn names(p: &Package) -> Vec<(Kind, &str)> {
        p.components.iter().map(|c| (c.kind, c.name.as_str())).collect()
    }

    #[test]
    fn a_cabal_file_says_its_components() {
        let p = parse_cabal(include_str!("samples/orbit.cabal"));
        assert_eq!(p.name.as_deref(), Some("orbit"));
        assert_eq!(names(&p), [(Kind::Library, "orbit"), (Kind::Library, "orbit-internal"), (Kind::Executable, "orbit"), (Kind::Executable, "orbit-sim"), (Kind::Test, "orbit-test"), (Kind::Benchmark, "orbit-bench")], "the common stanza and the conditionals left out");
        let late = parse_cabal("cabal-version: 3.0\nlibrary\n  exposed-modules: A\nName:    late\n");
        assert_eq!(names(&late), [(Kind::Library, "late")], "Name: after the library, any case");
    }

    #[test]
    fn hpacks_package_yaml_says_its_components() {
        let p = parse_package_yaml(include_str!("samples/package.yaml"));
        assert_eq!(p.name.as_deref(), Some("orbit"));
        assert_eq!(names(&p), [(Kind::Library, "orbit"), (Kind::Executable, "orbit-exe"), (Kind::Executable, "orbit-sim"), (Kind::Test, "orbit-test")], "their fields left alone");
        let one = parse_package_yaml("name: \"tiny\"\nexecutable:\n  main: Main.hs\n");
        assert_eq!(names(&one), [(Kind::Executable, "tiny")]);
    }

    #[test]
    fn a_cabal_project_lists_its_packages() {
        let dir = temp("cabal-project");
        for pkg in ["orbit-core", "orbit-cli"] {
            std::fs::create_dir_all(dir.join("libs").join(pkg)).unwrap();
            std::fs::write(dir.join("libs").join(pkg).join(format!("{pkg}.cabal")), format!("name: {pkg}\nlibrary\nexecutable {pkg}-exe\n")).unwrap();
        }
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        std::fs::write(dir.join("tools/tool.cabal"), "name: tool\nexecutable tool\n").unwrap();
        std::fs::write(dir.join("cabal.project"), "-- the packages\npackages: libs/*/\n          ./tools/tool.cabal\n\ntests: True\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.cabal_project && !p.stack && p.is_haskell());
        let dirs: Vec<&str> = p.packages.iter().map(|p| p.dir.as_str()).collect();
        assert_eq!(dirs, ["libs/orbit-cli", "libs/orbit-core", "tools"]);
        let exes: Vec<&str> = p.components(Kind::Executable).iter().map(|(_, c)| c.name.as_str()).collect();
        assert_eq!(exes, ["orbit-cli-exe", "orbit-core-exe", "tool"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stack_project_reads_package_yaml_where_theres_no_cabal_file() {
        let dir = temp("stack");
        std::fs::write(dir.join("stack.yaml"), "snapshot: lts-24.11\npackages:\n- .\n").unwrap();
        std::fs::write(dir.join("package.yaml"), include_str!("samples/package.yaml")).unwrap();
        std::fs::write(dir.join("fourmolu.yaml"), "indentation: 2\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.stack && p.fourmolu_yaml);
        assert_eq!(p.packages.len(), 1);
        assert_eq!((p.packages[0].name.as_deref(), p.packages[0].dir.as_str(), p.packages[0].hpack), (Some("orbit"), "", true));
        assert!(p.stack_project());
        // hpack wrote the .cabal file: it's read instead.
        std::fs::write(dir.join("orbit.cabal"), include_str!("samples/orbit.cabal")).unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.packages[0].components.len(), p.packages[0].hpack), (6, false));
        std::fs::remove_file(dir.join("stack.yaml")).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).stack_project(), "Cabal reads it now");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_of_scripts_has_its_files_run_alone() {
        let dir = temp("lone");
        std::fs::write(dir.join("Hello.hs"), "main :: IO ()\nmain = putStrLn \"hello\"\n").unwrap();
        std::fs::write(dir.join("Orbit.hs"), "module Orbit where\n\nperiod :: Double\nperiod = 5520\n").unwrap();
        std::fs::write(dir.join("notes.md"), "").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.lone, [Lone { file: "Hello.hs".into(), main: true }, Lone { file: "Orbit.hs".into(), main: false }]);
        assert!(p.packages.is_empty() && p.is_haskell());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).is_haskell());
    }
}
