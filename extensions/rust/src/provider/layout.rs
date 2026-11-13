//! Where a test is, as cargo would name it: its package, its target, and
//! its module path, from `cargo metadata`'s packages and targets -- or,
//! before cargo has answered, from the nearest `Cargo.toml` and cargo's
//! own conventions (`src/lib.rs`, `src/main.rs`, `src/bin/*.rs`,
//! `tests/*.rs`), so a scan's keys are the same either way. And the keys
//! and groups built from that, which a scan, a listing and a run must
//! all build alike or Fenix won't know the id.

use std::collections::HashMap;
use std::path::Path;

use fenix_lang_kit::tests::Group;

use crate::project::Metadata;

/// A target of a package, as a key says it: `lib`, `bin:<name>`,
/// `test:<name>`, `bench:<name>`, `example:<name>`, or `doc` (its doc
/// tests).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetKey {
    pub package: String,
    pub target: String,
}

impl TargetKey {
    pub fn new(package: &str, target: &str) -> TargetKey {
        TargetKey { package: package.into(), target: target.into() }
    }

    /// From cargo's kinds and name (`compiler-artifact`, metadata): `None`
    /// for a build script.
    pub fn of_kind(package: &str, kinds: &[String], name: &str) -> Option<TargetKey> {
        let kind = kinds.first().map(String::as_str).unwrap_or("");
        let target = match kind {
            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro" => "lib".to_string(),
            "bin" | "test" | "bench" | "example" => format!("{kind}:{name}"),
            _ => return None,
        };
        Some(TargetKey::new(package, &target))
    }

    /// `orbit/lib`, the start of its tests' keys.
    pub fn id(&self) -> String {
        format!("{}/{}", self.package, self.target)
    }

    /// A test's key: `orbit/lib tests::adds`, `orbit/doc src/lib.rs - add (line 6)`.
    pub fn key(&self, path: &str) -> String {
        format!("{}/{} {path}", self.package, self.target)
    }

    /// Its group's label: `orbit (lib)`, `orbit (test it)`, `orbit (doc)`.
    pub fn label(&self) -> String {
        match self.target.split_once(':') {
            Some((kind, name)) => format!("{} ({kind} {name})", self.package),
            None => format!("{} ({})", self.package, self.target),
        }
    }

    /// Its group, the root of each of its tests' groups.
    pub fn group(&self) -> Group {
        Group::package(self.label())
    }

    /// Back from a group's label.
    pub fn from_label(label: &str) -> Option<TargetKey> {
        let (package, rest) = label.split_once(" (")?;
        let inner = rest.strip_suffix(')')?;
        let target = match inner.split_once(' ') {
            Some((kind, name)) => format!("{kind}:{name}"),
            None => inner.to_string(),
        };
        Some(TargetKey::new(package, &target))
    }

    pub fn is_doc(&self) -> bool {
        self.target == "doc"
    }

    /// cargo's flags that pick it: `--lib`, `--test it`, `--doc`.
    pub fn flags(&self) -> Vec<String> {
        match self.target.split_once(':') {
            Some((kind, name)) => vec![format!("--{kind}"), name.to_string()],
            None => vec![format!("--{}", self.target)],
        }
    }
}

/// A key back as its target and path: `orbit/lib tests::adds`.
pub fn parse_key(key: &str) -> Option<(TargetKey, &str)> {
    let (target, path) = key.split_once(' ')?;
    let (package, target) = target.split_once('/')?;
    (!package.is_empty() && !target.is_empty() && !path.is_empty()).then(|| (TargetKey::new(package, target), path))
}

/// A doc test's name, `src/lib.rs - add (line 6)`: its file and line.
pub fn doc_place(name: &str) -> Option<(&str, u32)> {
    let (file, rest) = name.split_once(" - ")?;
    let line = rest.rsplit_once("(line ")?.1.split(')').next()?.parse().ok()?;
    Some((file, line))
}

/// A target as the layout has it: its source, relative to its package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetLayout {
    pub key: TargetKey,
    /// `src/lib.rs`, `tests/it.rs`, below its package's folder.
    pub src: String,
    /// `cargo test` runs its tests.
    pub tested: bool,
    /// A library with doc tests.
    pub doctest: bool,
    /// The crate's name (`orbit_core`), which `Doc-tests` says.
    pub crate_name: String,
    pub required_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLayout {
    pub name: String,
    /// Its folder below the project: "" for the project's own, `orbit-core`.
    pub dir: String,
    /// Its folder as cargo has it, a full path of the machine.
    pub abs: String,
    pub targets: Vec<TargetLayout>,
    /// Read from its Cargo.toml, not cargo's: `src/bin/*.rs` and
    /// `tests/*.rs` are targets by cargo's conventions.
    pub conventions: bool,
}

/// The project's packages, below it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub packages: Vec<PackageLayout>,
    /// The project's folder as cargo has it (`/home/u/orbit`); "" before
    /// cargo has said.
    pub project: String,
    /// The workspace's root as cargo has it.
    pub workspace_root: String,
    /// The project's folder below the workspace's root, with a `/`
    /// (`crates/orbit/`), when the project is one member of a larger
    /// workspace: rustc's paths (a panic's place, a doc test's name) are
    /// the workspace root's.
    pub below_root: String,
    /// The package the project is, when it's a member of a larger
    /// workspace: its runs say `-p` it, not `--workspace`.
    pub member: Option<String>,
}

fn slashes(p: &str) -> String {
    p.replace('\\', "/")
}

/// `path` below `dir`, when it is.
fn below<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    if dir.is_empty() {
        return Some(path);
    }
    match path.strip_prefix(dir)? {
        "" => Some(""),
        rest => rest.strip_prefix('/'),
    }
}

impl Layout {
    /// From `cargo metadata --no-deps`, with `project` the project's folder
    /// as cargo has it (the folder holding its Cargo.toml).
    pub fn from_metadata(m: &Metadata, project: &str) -> Layout {
        let project = slashes(project).trim_end_matches('/').to_string();
        let root = slashes(&m.workspace_root).trim_end_matches('/').to_string();
        let mut packages = Vec::new();
        for p in &m.packages {
            let manifest = slashes(&p.manifest_path);
            let abs = manifest.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
            let Some(dir) = below(&abs, &project).map(str::to_string) else { continue };
            let targets = p
                .targets
                .iter()
                .filter_map(|t| {
                    let key = TargetKey::of_kind(&p.name, &t.kind, &t.name)?;
                    let src = below(&slashes(&t.src_path), &abs)?.to_string();
                    Some(TargetLayout { tested: t.test, doctest: t.doctest && key.target == "lib", crate_name: t.name.replace('-', "_"), required_features: t.required_features.clone(), key, src })
                })
                .collect();
            packages.push(PackageLayout { name: p.name.clone(), dir, abs, targets, conventions: false });
        }
        let below_root = match below(&project, &root) {
            Some("") | None => String::new(),
            Some(rest) => format!("{rest}/"),
        };
        let member = (!below_root.is_empty()).then(|| packages.iter().find(|p| p.dir.is_empty()).map(|p| p.name.clone())).flatten();
        Layout { packages, project, workspace_root: root, below_root, member }
    }

    /// A package from its `Cargo.toml`'s text, for before cargo has said:
    /// its explicit targets, then cargo's conventions for the file asked.
    /// `has` says whether a file below its folder is there.
    pub fn package_from_toml(text: &str, dir: &str, has: &dyn Fn(&str) -> bool) -> Option<PackageLayout> {
        let doc: toml::Table = toml::from_str(text).ok()?;
        let name = doc.get("package")?.get("name")?.as_str()?.to_string();
        let mut targets = Vec::new();
        let tested = |t: &toml::Value, default: bool| t.get("test").and_then(toml::Value::as_bool).unwrap_or(default);
        let lib = doc.get("lib");
        let lib_path = lib.and_then(|l| l.get("path")).and_then(toml::Value::as_str).unwrap_or("src/lib.rs").to_string();
        let crate_name = lib.and_then(|l| l.get("name")).and_then(toml::Value::as_str).unwrap_or(&name).replace('-', "_");
        if lib.is_some() || has(&lib_path) {
            targets.push(TargetLayout { key: TargetKey::new(&name, "lib"), src: lib_path, tested: lib.is_none_or(|l| tested(l, true)), doctest: lib.and_then(|l| l.get("doctest")).and_then(toml::Value::as_bool).unwrap_or(true), crate_name, required_features: Vec::new() });
        }
        let bins = doc.get("bin").and_then(toml::Value::as_array);
        if has("src/main.rs") && !bins.into_iter().flatten().any(|b| b.get("path").and_then(toml::Value::as_str) == Some("src/main.rs")) {
            targets.push(TargetLayout { key: TargetKey::new(&name, &format!("bin:{name}")), src: "src/main.rs".into(), tested: true, doctest: false, crate_name: name.replace('-', "_"), required_features: Vec::new() });
        }
        for (table, kind, folder, default) in [("bin", "bin", "src/bin", true), ("test", "test", "tests", true), ("bench", "bench", "benches", false), ("example", "example", "examples", false)] {
            for t in doc.get(table).and_then(toml::Value::as_array).into_iter().flatten() {
                let Some(tname) = t.get("name").and_then(toml::Value::as_str) else { continue };
                let src = t.get("path").and_then(toml::Value::as_str).map(str::to_string).unwrap_or_else(|| if kind == "bin" && tname == name { "src/main.rs".into() } else { format!("{folder}/{tname}.rs") });
                targets.push(TargetLayout { key: TargetKey::new(&name, &format!("{kind}:{tname}")), src, tested: tested(t, default), doctest: false, crate_name: tname.replace('-', "_"), required_features: Vec::new() });
            }
        }
        Some(PackageLayout { name, dir: dir.into(), abs: String::new(), targets, conventions: true })
    }

    /// The package holding `file` (below the project): the deepest whose
    /// folder it's in.
    pub fn package_of(&self, file: &str) -> Option<&PackageLayout> {
        self.packages.iter().filter(|p| below(file, &p.dir).is_some()).max_by_key(|p| p.dir.len())
    }

    pub fn package(&self, name: &str) -> Option<&PackageLayout> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// The library whose crate is `crate_name`: what `Doc-tests orbit_core` says.
    pub fn doc_target(&self, crate_name: &str) -> Option<TargetKey> {
        self.packages.iter().find_map(|p| p.targets.iter().find(|t| t.key.target == "lib" && t.crate_name == crate_name).map(|_| TargetKey::new(&p.name, "doc")))
    }

    pub fn target(&self, key: &TargetKey) -> Option<&TargetLayout> {
        let lookup = if key.is_doc() { TargetKey::new(&key.package, "lib") } else { key.clone() };
        self.package(&key.package)?.targets.iter().find(|t| t.key == lookup)
    }

    /// rustc's path (relative to the workspace's root, or full) as one
    /// below the project, or a full path when it isn't in the project.
    pub fn place(&self, path: &str) -> String {
        let path = slashes(path);
        let full = path.starts_with('/') || (path.len() > 1 && path.as_bytes()[1] == b':');
        if full {
            return match below(&path, &self.project) {
                Some(rest) if !self.project.is_empty() => rest.to_string(),
                _ => path,
            };
        }
        let path = path.strip_prefix("./").unwrap_or(&path).to_string();
        if self.below_root.is_empty() {
            return path;
        }
        match path.strip_prefix(&self.below_root) {
            Some(rest) => rest.to_string(),
            None => format!("{}/{path}", self.workspace_root),
        }
    }
}

/// Where a file's tests are: its target and the module path it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub target: TargetKey,
    /// `geo` for `src/geo/mod.rs`, empty for a target's root.
    pub module: Vec<String>,
}

/// The module path of `rel`, a file below its target's root folder:
/// `geo/mod.rs` and `geo.rs` are `geo`, `a/b.rs` is `a::b`.
fn module_path(rel: &str) -> Option<Vec<String>> {
    let stem = rel.strip_suffix(".rs")?;
    let mut parts: Vec<String> = stem.split('/').map(str::to_string).collect();
    if parts.last().is_some_and(|p| p == "mod") {
        parts.pop();
    }
    (!parts.is_empty() && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_'))).then_some(parts)
}

/// Where `rel` (below its package's folder) is: a target's root; a module
/// under `src/bin/<name>/` or `tests/<name>/` of that target; else one
/// under the library's folder, or the binary's when there's no library.
/// Examples, benches and build scripts aren't tested by `cargo test`,
/// nor a target that says `test = false`: none.
pub fn place_in(package: &PackageLayout, rel: &str) -> Option<Place> {
    if let Some(t) = package.targets.iter().find(|t| t.src == rel) {
        return t.tested.then(|| Place { target: t.key.clone(), module: Vec::new() });
    }
    if package.conventions {
        // cargo's auto-discovered targets: `src/bin/x.rs`, `tests/x.rs`.
        for (folder, kind) in [("src/bin", "bin"), ("tests", "test")] {
            if let Some(stem) = below(rel, folder).and_then(|r| r.strip_suffix(".rs")).filter(|s| !s.contains('/')) {
                return Some(Place { target: TargetKey::new(&package.name, &format!("{kind}:{stem}")), module: Vec::new() });
            }
        }
    }
    // A target whose root is `<dir>/main.rs` holds the files in `<dir>/`.
    for t in package.targets.iter().filter(|t| t.tested) {
        let Some(dir) = t.src.strip_suffix("/main.rs").filter(|d| *d != "src") else { continue };
        if let Some(inner) = below(rel, dir) {
            return module_path(inner).map(|module| Place { target: t.key.clone(), module });
        }
    }
    if ["src/bin", "tests", "examples", "benches"].iter().any(|f| below(rel, f).is_some()) {
        return None;
    }
    let crate_root = package.targets.iter().find(|t| t.key.target == "lib").or_else(|| package.targets.iter().find(|t| t.src == "src/main.rs"))?;
    if !crate_root.tested {
        return None;
    }
    let dir = crate_root.src.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    module_path(below(rel, dir)?).map(|module| Place { target: crate_root.key.clone(), module })
}

/// The packages read from their `Cargo.toml`s, for before cargo has said:
/// each folder's read once.
#[derive(Debug, Default)]
pub struct Manifests {
    read: HashMap<String, Option<PackageLayout>>,
}

impl Manifests {
    /// The package holding `file` (below the project, whose folder is
    /// `root` on this machine): the nearest folder up from it with a
    /// `Cargo.toml` that has a `[package]`.
    pub fn package_of(&mut self, root: &str, file: &str) -> Option<PackageLayout> {
        let mut dir = file.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        loop {
            let found = self.read.entry(dir.clone()).or_insert_with(|| {
                let path = if dir.is_empty() { format!("{root}/Cargo.toml") } else { format!("{root}/{dir}/Cargo.toml") };
                let folder = if dir.is_empty() { root.to_string() } else { format!("{root}/{dir}") };
                let has = |rel: &str| Path::new(&folder).join(rel).is_file();
                std::fs::read_to_string(path).ok().and_then(|text| Layout::package_from_toml(&text, &dir, &has))
            });
            if let Some(p) = found {
                return Some(p.clone());
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        }
    }

    /// Every package read so far.
    pub fn known(&self) -> Vec<PackageLayout> {
        let mut out: Vec<PackageLayout> = self.read.values().flatten().cloned().collect();
        out.sort_by(|a, b| a.dir.cmp(&b.dir));
        out.dedup_by(|a, b| a.dir == b.dir);
        out
    }

    /// A Cargo.toml was saved: read them again.
    pub fn forget(&mut self) {
        self.read.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::ok;

    const ORBIT: &str = include_str!("../samples/tests/cargo/metadata-orbit.json");
    const WS: &str = include_str!("../samples/tests/cargo/metadata-ws.json");

    #[test]
    fn keys_and_labels_go_both_ways() {
        let t = TargetKey::new("orbit", "test:it");
        assert_eq!((t.key("integration_fails"), t.label(), t.flags()), ("orbit/test:it integration_fails".to_string(), "orbit (test it)".to_string(), vec!["--test".to_string(), "it".into()]));
        assert_eq!(TargetKey::from_label("orbit (test it)"), Some(t));
        assert_eq!(TargetKey::from_label("orbit-core (lib)"), Some(TargetKey::new("orbit-core", "lib")));
        assert_eq!(parse_key("orbit/doc src/lib.rs - add (line 6)"), Some((TargetKey::new("orbit", "doc"), "src/lib.rs - add (line 6)")));
        assert_eq!(parse_key("orbit tests::adds"), None);
        assert_eq!(doc_place("orbit-core/src/lib.rs - double (line 3)"), Some(("orbit-core/src/lib.rs", 3)));
        assert_eq!(doc_place("src/lib.rs - (line 1)"), Some(("src/lib.rs", 1)));
    }

    #[test]
    fn a_files_target_and_module_come_from_cargos_layout() {
        let m = Metadata::parse(&ok(ORBIT)).unwrap();
        let l = Layout::from_metadata(&m, "/project");
        let p = l.package_of("src/geo/mod.rs").unwrap();
        let at = |rel: &str| place_in(p, rel).map(|pl| (pl.target.id(), pl.module.join("::")));
        assert_eq!(at("src/lib.rs"), Some(("orbit/lib".into(), String::new())));
        assert_eq!(at("src/geo/mod.rs"), Some(("orbit/lib".into(), "geo".into())));
        assert_eq!(at("src/geo/deep.rs"), Some(("orbit/lib".into(), "geo::deep".into())));
        assert_eq!(at("tests/it.rs"), Some(("orbit/test:it".into(), String::new())));
        assert_eq!(at("tests/common/mod.rs"), None, "not a target of its own");
        assert_eq!(at("examples/demo.rs"), None, "cargo test doesn't run an example's tests");
        let ws = Layout::from_metadata(&Metadata::parse(&ok(WS)).unwrap(), "/project");
        assert_eq!(ws.packages.iter().map(|p| (p.name.as_str(), p.dir.as_str())).collect::<Vec<_>>(), [("orbit-core", "orbit-core"), ("orbit-cli", "orbit-cli")]);
        let cli = ws.package_of("orbit-cli/src/main.rs").unwrap();
        assert_eq!(place_in(cli, "src/main.rs").map(|p| p.target.id()).as_deref(), Some("orbit-cli/bin:orbit-cli"));
        assert_eq!(place_in(cli, "src/args.rs").map(|p| p.target.id()).as_deref(), Some("orbit-cli/bin:orbit-cli"), "a binary crate's module");
        assert_eq!(ws.doc_target("orbit_core"), Some(TargetKey::new("orbit-core", "doc")));
        assert_eq!((ws.place("orbit-core/src/lib.rs"), ws.member.as_deref()), ("orbit-core/src/lib.rs".to_string(), None));
    }

    #[test]
    fn a_member_opened_alone_takes_rustcs_paths_from_the_workspaces_root() {
        let m = Metadata::parse(&ok(WS)).unwrap();
        let l = Layout::from_metadata(&m, "/project/orbit-core");
        assert_eq!((l.below_root.as_str(), l.member.as_deref()), ("orbit-core/", Some("orbit-core")));
        assert_eq!(l.packages.len(), 1, "its sibling isn't in the project");
        assert_eq!(l.place("orbit-core/src/lib.rs"), "src/lib.rs");
        assert_eq!(l.place("orbit-cli/src/main.rs"), "/project/orbit-cli/src/main.rs", "outside it: a full path");
        assert_eq!(l.place("/project/orbit-core/tests/it_works.rs"), "tests/it_works.rs");
    }

    #[test]
    fn before_cargo_says_cargo_toml_and_its_conventions_say_the_same() {
        let p = Layout::package_from_toml("[package]\nname = \"orbit\"\n\n[[test]]\nname = \"custom\"\nharness = false\n\n[[bin]]\nname = \"probe\"\npath = \"tools/probe.rs\"\n", "", &|f| f == "src/lib.rs").unwrap();
        let at = |rel: &str| place_in(&p, rel).map(|pl| (pl.target.id(), pl.module.join("::")));
        assert_eq!(at("src/geo/mod.rs"), Some(("orbit/lib".into(), "geo".into())));
        assert_eq!(at("tools/probe.rs"), Some(("orbit/bin:probe".into(), String::new())));
        assert_eq!(at("tests/custom.rs"), Some(("orbit/test:custom".into(), String::new())));
        assert_eq!(at("tests/it.rs"), Some(("orbit/test:it".into(), String::new())), "by cargo's conventions");
        assert_eq!(at("src/bin/tool.rs"), Some(("orbit/bin:tool".into(), String::new())));
        let bin_only = Layout::package_from_toml("[package]\nname = \"cli\"\n", "", &|f| f == "src/main.rs").unwrap();
        assert_eq!(place_in(&bin_only, "src/args.rs").map(|p| p.target.id()).as_deref(), Some("cli/bin:cli"));
        let dir = crate::project::tests::temp("manifests");
        std::fs::create_dir_all(dir.join("crates/a/src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/a\"]\n").unwrap();
        std::fs::write(dir.join("crates/a/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        let mut m = Manifests::default();
        let a = m.package_of(&dir.to_string_lossy(), "crates/a/src/x/y.rs").unwrap();
        assert_eq!((a.name.as_str(), a.dir.as_str()), ("a", "crates/a"));
        assert!(m.package_of(&dir.to_string_lossy(), "README.rs").is_none(), "the workspace's own Cargo.toml has no package");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
