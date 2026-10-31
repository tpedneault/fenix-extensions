//! The package manager page's C and C++: vcpkg in manifest mode
//! (`vcpkg.json`) and Conan 2 (`conanfile.txt` or `conanfile.py`), each
//! as far as its command line goes.
//!
//! - vcpkg lists what's installed (`vcpkg list --x-json`) and what its
//!   ports tree has (`vcpkg search --x-json`, the newest it knows of);
//!   `U` moves the manifest's baseline up (`vcpkg x-update-baseline`),
//!   and `a` is `vcpkg add port`. It has no command to take a port out of
//!   `vcpkg.json`, nor to move one up alone, nor an audit.
//! - Conan lists the graph (`conan graph info --format json`), what's
//!   newer in its remotes (`conan graph outdated`) and what's vulnerable
//!   (`conan audit scan`, which needs a token from conan.io). Upgrading
//!   moves within the conanfile's version ranges (`conan lock upgrade`
//!   with a `conan.lock`, else `conan install --update`); a pinned
//!   version can only be changed in the file. `a` brings a package into
//!   Conan's cache (`conan install --requires`) -- Conan has no command
//!   that adds it to the conanfile -- and nothing removes one.

use std::path::Path;

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::{Map, Value};
use fenix_lang_kit::{Advisory, Dependency, Finding, Found, Output, PackageManager, Risk, Update, Version};

use crate::project::{conan_txt, vcpkg_deps, Dep};

/// Where vcpkg installed the manifest's ports: the build's own (vcpkg's
/// CMake toolchain puts them there), else vcpkg's default.
pub fn install_root(root: &str, build: Option<&str>) -> Option<String> {
    let dir = format!("{}/vcpkg_installed", build?);
    Path::new(root).join(&dir).is_dir().then_some(dir)
}

/// The ports installed under `install`: `vcpkg/info/<port>_<version>_<triplet>.list`.
pub fn installed_ports(root: &str, install: &str) -> Vec<String> {
    let dir = Path::new(root).join(install).join("vcpkg/info");
    let mut out: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_string_lossy().split('_').next().map(str::to_string)).collect();
    out.sort();
    out.dedup();
    out
}

fn object(out: &Output) -> Result<Map<String, Value>, String> {
    out.json::<Map<String, Value>>()
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// `vcpkg list --x-json`: each port once (the target triplet's and the
/// host's are the same port), marked with whether the manifest asks for
/// it. A manifest that names none: all are its.
pub fn parse_vcpkg_list(out: &Output, manifest: &[Dep]) -> Result<Vec<Dependency>, String> {
    let rows = object(out)?;
    let mut deps: Vec<Dependency> = Vec::new();
    for v in rows.values() {
        let name = text(v, "package_name");
        if name.is_empty() || deps.iter().any(|d| d.name == name) {
            continue;
        }
        let d = Dependency::new(name.as_str(), text(v, "version"));
        deps.push(match manifest.iter().find(|m| m.name == name) {
            _ if manifest.is_empty() => d,
            None => d.transitive(),
            Some(Dep { group: Some(g), .. }) => d.group(g.as_str()),
            Some(_) => d,
        });
    }
    Ok(deps)
}

/// `vcpkg search --x-json`: the newest of each port in vcpkg's tree.
/// Only `ports` are kept -- the tree has thousands.
pub fn parse_vcpkg_search(out: &Output, ports: &[String]) -> Result<Vec<Update>, String> {
    let rows = object(out)?;
    Ok(rows.iter().filter(|(name, _)| ports.contains(name)).map(|(name, v)| Update::new(name.as_str()).latest(text(v, "version"))).collect())
}

pub fn parse_vcpkg_found(out: &Output) -> Result<Vec<Found>, String> {
    let rows = object(out)?;
    Ok(rows
        .iter()
        .map(|(name, v)| Found { name: name.clone(), version: text(v, "version"), description: v.get("description").and_then(Value::as_array).and_then(|d| d.first()).and_then(Value::as_str).unwrap_or_default().to_string() })
        .collect())
}

/// A port as `vcpkg add port` takes it: `fmt@11` is `fmt` (the
/// baseline says the version).
pub fn port(typed: &str) -> String {
    typed.trim().split('@').next().unwrap_or("").trim().to_string()
}

/// vcpkg, in manifest mode.
pub struct Vcpkg {
    pub vcpkg: Command,
    /// The project's folder, `/project`.
    pub root: String,
    /// The build folder, where CMake's vcpkg installed into.
    pub build: Option<String>,
}

impl Vcpkg {
    fn manifest(&self) -> Vec<Dep> {
        std::fs::read_to_string(Path::new(&self.root).join("vcpkg.json")).ok().and_then(|t| vcpkg_deps(&t).ok()).unwrap_or_default()
    }

    fn install_root(&self) -> Option<String> {
        install_root(&self.root, self.build.as_deref())
    }
}

impl PackageManager for Vcpkg {
    fn name(&self) -> &str {
        "vcpkg"
    }
    fn list(&self) -> Command {
        let cmd = self.vcpkg.clone().args(["list", "--x-json"]);
        match self.install_root() {
            Some(dir) => cmd.arg(format!("--x-install-root={dir}")),
            None => cmd,
        }
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_vcpkg_list(out, &self.manifest())
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.vcpkg.clone().args(["search", "--x-json"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        let mut ports: Vec<String> = self.manifest().into_iter().map(|d| d.name).collect();
        ports.extend(installed_ports(&self.root, &self.install_root().unwrap_or_else(|| "vcpkg_installed".into())));
        parse_vcpkg_search(out, &ports)
    }
    /// vcpkg takes the versions the manifest's baseline says, so one
    /// can't be moved up alone (an `overrides` entry would, by hand).
    fn upgrade(&self, _dep: &Dependency) -> Option<Command> {
        None
    }
    /// The baseline moved to vcpkg's tree's newest; the next configure
    /// installs those.
    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        Some(self.vcpkg.clone().arg("x-update-baseline"))
    }
    fn add(&self, package: &str) -> Command {
        self.vcpkg.clone().args(["add".to_string(), "port".to_string(), port(package)])
    }
    fn remove(&self, _dep: &Dependency) -> Option<Command> {
        None
    }
    fn search(&self, query: &str) -> Option<Command> {
        Some(self.vcpkg.clone().args(["search".to_string(), query.trim().to_string(), "--x-json".to_string()]))
    }
    fn parse_search(&self, out: &Output) -> Result<Vec<Found>, String> {
        parse_vcpkg_found(out)
    }
    fn lockfiles(&self) -> Vec<String> {
        ["vcpkg.json", "vcpkg-configuration.json"].iter().map(|f| format!("{}/{f}", self.root)).collect()
    }
}

/// `conan graph info --format json`: every package in the graph but the
/// project, with whether the project requires it itself; tool requires
/// are `build`'s, test requires `test`'s.
pub fn parse_conan_graph(out: &Output) -> Result<Vec<Dependency>, String> {
    let doc: Value = out.json()?;
    let nodes = doc.pointer("/graph/nodes").and_then(Value::as_object).ok_or("there's no graph in what conan said")?;
    let root = nodes.iter().find(|(_, n)| n.get("recipe").and_then(Value::as_str) == Some("Consumer")).map(|(k, _)| k.as_str()).unwrap_or("0");
    let edges = nodes.get(root).and_then(|n| n.get("dependencies")).and_then(Value::as_object).cloned().unwrap_or_default();
    let mut deps: Vec<Dependency> = Vec::new();
    for (id, n) in nodes.iter().filter(|(k, _)| k.as_str() != root) {
        let name = text(n, "name");
        if name.is_empty() || deps.iter().any(|d| d.name == name) {
            continue;
        }
        let edge = edges.get(id).filter(|e| e.get("direct").and_then(Value::as_bool).unwrap_or(false));
        let mut d = Dependency::new(name.as_str(), text(n, "version")).direct(edge.is_some());
        let flag = |k: &str| edge.and_then(|e| e.get(k)).and_then(Value::as_bool).unwrap_or(false);
        if flag("test") || n.get("test").and_then(Value::as_bool).unwrap_or(false) {
            d = d.group("test");
        } else if flag("build") || n.get("context").and_then(Value::as_str) == Some("build") {
            d = d.group("build");
        }
        deps.push(d);
    }
    Ok(deps)
}

/// A reference's version: `hello/1.2#rev` is `1.2`.
fn version_of(reference: &str) -> String {
    reference.split('#').next().unwrap_or("").split('@').next().unwrap_or("").split_once('/').map(|(_, v)| v.to_string()).unwrap_or_default()
}

/// `conan graph outdated --format json`: the newest each remote has.
pub fn parse_conan_outdated(out: &Output) -> Result<Vec<Update>, String> {
    let rows = object(out)?;
    let mut updates = Vec::new();
    for (name, v) in &rows {
        let latest = v.pointer("/latest_remote/ref").and_then(Value::as_str).map(version_of).unwrap_or_default();
        let current: Vec<String> = v.get("current_versions").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(version_of).collect();
        if current.is_empty() {
            updates.push(Update::new(name.as_str()).latest(latest.as_str()));
        }
        for c in current {
            updates.push(Update::new(name.as_str()).current(c).latest(latest.as_str()));
        }
    }
    Ok(updates)
}

/// A description's first sentence, short enough for a row.
fn title(description: &str) -> String {
    let first = description.trim().lines().next().unwrap_or("").split(". ").next().unwrap_or("").trim_end_matches('.').trim();
    if first.chars().count() > 100 {
        format!("{}…", first.chars().take(99).collect::<String>().trim_end())
    } else {
        first.to_string()
    }
}

/// `conan audit scan --format json`: an advisory for each vulnerability
/// of each package, its risk its severity (else its CVSS score). One the
/// audit doesn't know says so, and has none. Without a token it says
/// how to get one, on stderr, and that's the page's error.
pub fn parse_conan_audit(out: &Output) -> Result<Vec<Finding>, String> {
    if out.stdout.trim().is_empty() {
        return Err(out.why());
    }
    let doc: Value = out.json()?;
    let mut findings = Vec::new();
    for (reference, pkg) in doc.get("data").and_then(Value::as_object).into_iter().flatten() {
        let (name, version) = reference.split_once('/').map(|(n, _)| (n.to_string(), version_of(reference))).unwrap_or((reference.clone(), String::new()));
        for edge in pkg.pointer("/vulnerabilities/edges").and_then(Value::as_array).into_iter().flatten() {
            let Some(node) = edge.get("node") else { continue };
            let id = text(node, "name");
            let risk = match node.get("severity").and_then(Value::as_str).map(Risk::parse) {
                Some(r) if r != Risk::Unknown => r,
                _ => node.pointer("/cvss/preferredBaseScore").and_then(Value::as_f64).map(|s| Risk::parse(&s.to_string())).unwrap_or_default(),
            };
            let mut advisory = Advisory::new(id.as_str(), risk).title(title(&text(node, "description")));
            if id.starts_with("CVE-") {
                advisory = advisory.url(format!("https://nvd.nist.gov/vuln/detail/{id}"));
            }
            findings.push(Finding::new(name.as_str(), Some(version.as_str()), advisory));
        }
    }
    Ok(findings)
}

/// `conan search --format json`: the newest of each package found, by
/// its reference, which is what `a` adds.
pub fn parse_conan_search(out: &Output) -> Result<Vec<Found>, String> {
    let remotes = object(out)?;
    let mut found: Vec<Found> = Vec::new();
    for (remote, refs) in &remotes {
        for reference in refs.as_object().into_iter().flatten().map(|(r, _)| r).filter(|r| r.as_str() != "error") {
            let Some((name, version)) = reference.split_once('/') else { continue };
            let newer = |f: &Found| Version::parse(version) > Version::parse(&f.version);
            match found.iter_mut().find(|f| f.name.split('/').next() == Some(name)) {
                Some(f) if newer(f) => *f = Found { name: reference.clone(), version: version.into(), description: format!("in {remote}") },
                Some(_) => {}
                None => found.push(Found { name: reference.clone(), version: version.into(), description: format!("in {remote}") }),
            }
        }
    }
    Ok(found)
}

/// What's typed to add, as a Conan reference: `fmt@11.1.4` is
/// `fmt/11.1.4`.
pub fn reference(typed: &str) -> String {
    let typed = typed.trim();
    match typed.split_once('@') {
        Some((name, version)) if !name.contains('/') && !version.contains('/') => format!("{name}/{version}"),
        _ => typed.to_string(),
    }
}

/// Conan 2, in the project's folder.
pub struct Conan {
    pub conan: Command,
    pub root: String,
    /// `conanfile.txt` or `conanfile.py`.
    pub file: String,
}

impl Conan {
    fn locked(&self) -> bool {
        Path::new(&self.root).join("conan.lock").is_file()
    }

    /// `name`, pinned in conanfile.txt (`hello/1.0`, not a range).
    fn pinned(&self, name: &str) -> bool {
        let Ok(text) = std::fs::read_to_string(Path::new(&self.root).join(&self.file)) else { return false };
        self.file == "conanfile.txt" && conan_txt(&text).0.iter().any(|d| d.name == name && d.version.as_deref().is_some_and(|v| !v.starts_with('[')))
    }

    /// Moving `pattern` up as far as the ranges let it.
    fn update(&self, name: &str) -> Command {
        if self.locked() {
            self.conan.clone().args(["lock", "upgrade", "."]).arg(format!("--update-requires={name}")).args(["--update", "--lockfile-out=conan.lock"])
        } else {
            let all = name == "*";
            let cmd = self.conan.clone().args(["install", "."]);
            let cmd = if all { cmd.arg("--update") } else { cmd.arg(format!("--update={}", name.trim_end_matches("/*"))) };
            cmd.arg("--build=missing")
        }
    }
}

impl PackageManager for Conan {
    fn name(&self) -> &str {
        "conan"
    }
    fn list(&self) -> Command {
        self.conan.clone().args(["graph", "info", ".", "--format=json"])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_conan_graph(out)
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.conan.clone().args(["graph", "outdated", ".", "--format=json"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_conan_outdated(out)
    }
    fn audit(&self) -> Option<Command> {
        Some(self.conan.clone().args(["audit", "scan", ".", "--format=json"]))
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_conan_audit(out)
    }
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        (!self.pinned(&dep.name)).then(|| self.update(&format!("{}/*", dep.name)))
    }
    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        Some(self.update("*"))
    }
    fn add(&self, package: &str) -> Command {
        self.conan.clone().args(["install".to_string(), format!("--requires={}", reference(package)), "--build=missing".to_string()])
    }
    fn remove(&self, _dep: &Dependency) -> Option<Command> {
        None
    }
    fn search(&self, query: &str) -> Option<Command> {
        Some(self.conan.clone().args(["search".to_string(), format!("*{}*", query.trim()), "--format=json".to_string()]))
    }
    fn parse_search(&self, out: &Output) -> Result<Vec<Found>, String> {
        parse_conan_search(out)
    }
    fn lockfiles(&self) -> Vec<String> {
        [self.file.as_str(), "conan.lock"].iter().map(|f| format!("{}/{f}", self.root)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn conans_graph_says_what_the_project_requires() {
        let deps = parse_conan_graph(&ok(include_str!("samples/conan-graph-info.json"))).unwrap();
        let rows: Vec<(&str, &str, bool)> = deps.iter().map(|d| (d.name.as_str(), d.current.as_str(), d.direct)).collect();
        assert_eq!(rows, [("hello", "1.0", true), ("greet", "2.0", true)], "the conanfile itself isn't one");
        let tool = r#"{"graph": {"nodes": {
            "0": {"ref": "conanfile", "recipe": "Consumer", "dependencies": {"1": {"ref": "cmake/3.30.1", "direct": true, "build": true}, "2": {"ref": "gtest/1.15.0", "direct": true, "test": true}}},
            "1": {"ref": "cmake/3.30.1#a", "name": "cmake", "version": "3.30.1", "context": "build", "dependencies": {}},
            "2": {"ref": "gtest/1.15.0#b", "name": "gtest", "version": "1.15.0", "context": "host", "test": true, "dependencies": {}},
            "3": {"ref": "zlib/1.3.1#c", "name": "zlib", "version": "1.3.1", "context": "host", "dependencies": {}}
        }}}"#;
        let deps = parse_conan_graph(&ok(tool)).unwrap();
        let rows: Vec<(&str, bool, Option<&str>)> = deps.iter().map(|d| (d.name.as_str(), d.direct, d.group.as_deref())).collect();
        assert_eq!(rows, [("cmake", true, Some("build")), ("gtest", true, Some("test")), ("zlib", false, None)]);
        assert!(parse_conan_graph(&ok("{}")).unwrap_err().contains("no graph"));
    }

    #[test]
    fn conans_outdated_audit_and_search() {
        let updates = parse_conan_outdated(&ok(include_str!("samples/conan-graph-outdated.json"))).unwrap();
        assert_eq!(updates, [Update::new("hello").current("1.0").latest("1.2")]);
        let findings = parse_conan_audit(&Output { code: Some(0), ..ok(include_str!("samples/conan-audit.json")) }).unwrap();
        assert_eq!(findings.len(), 2, "hello has none, and greet isn't known to the audit");
        let f = &findings[0];
        assert_eq!((f.name.as_str(), f.version.as_deref(), f.advisory.id.as_str(), f.advisory.risk), ("zlib", Some("1.2.11"), "CVE-2022-37434", Risk::Critical));
        assert_eq!(f.advisory.title, "zlib through 1.2.12 has a heap-based buffer over-read or buffer overflow in inflate in inflate.c vi…");
        assert_eq!(f.advisory.url.as_deref(), Some("https://nvd.nist.gov/vuln/detail/CVE-2022-37434"));
        assert_eq!(findings[1].advisory.risk, Risk::High);
        // Without a token: what it said.
        let no_token = Output { code: Some(1), stdout: String::new(), stderr: "Authentication required for the CVE provider: 'conancenter'\n\nERROR: Missing authentication token. Please authenticate and retry.\n".into() };
        assert_eq!(parse_conan_audit(&no_token).unwrap_err(), "ERROR: Missing authentication token. Please authenticate and retry.");
        let found = parse_conan_search(&ok(include_str!("samples/conan-search.json"))).unwrap();
        assert_eq!(found, [Found { name: "hello/1.2".into(), version: "1.2".into(), description: "in local".into() }], "the newest of each");
        assert_eq!(parse_conan_search(&ok(r#"{"conancenter": {"error": "Unable to connect"}}"#)).unwrap(), []);
    }

    #[test]
    fn conan_moves_within_ranges_and_adds_to_its_cache() {
        let dir = temp("conan");
        let root = dir.to_string_lossy().into_owned();
        std::fs::write(dir.join("conanfile.txt"), "[requires]\nhello/1.0\ngreet/[>=2 <3]\n").unwrap();
        let c = Conan { conan: Command::new("conan"), root: root.clone(), file: "conanfile.txt".into() };
        assert_eq!(line(&c.list()), "conan graph info . --format=json");
        assert_eq!(c.upgrade(&Dependency::new("hello", "1.0").latest("1.2")), None, "pinned in the file");
        assert_eq!(line(&c.upgrade(&Dependency::new("greet", "2.0")).unwrap()), "conan install . --update=greet --build=missing");
        assert_eq!(line(&c.upgrade_all(&[]).unwrap()), "conan install . --update --build=missing");
        std::fs::write(dir.join("conan.lock"), "{}").unwrap();
        assert_eq!(line(&c.upgrade(&Dependency::new("zlib", "1.3").transitive()).unwrap()), "conan lock upgrade . --update-requires=zlib/* --update --lockfile-out=conan.lock");
        assert_eq!(line(&c.upgrade_all(&[]).unwrap()), "conan lock upgrade . --update-requires=* --update --lockfile-out=conan.lock");
        assert_eq!(line(&c.add("fmt@11.1.4")), "conan install --requires=fmt/11.1.4 --build=missing");
        assert_eq!(line(&c.add("hello/1.2")), "conan install --requires=hello/1.2 --build=missing");
        assert_eq!(c.remove(&Dependency::new("hello", "1.0")), None);
        assert_eq!(line(&c.search("hel").unwrap()), "conan search *hel* --format=json");
        assert_eq!(c.lockfiles(), [format!("{root}/conanfile.txt"), format!("{root}/conan.lock")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vcpkg_lists_what_it_installed_and_its_trees_newest() {
        let dir = temp("vcpkg");
        let root = dir.to_string_lossy().into_owned();
        std::fs::write(dir.join("vcpkg.json"), r#"{"dependencies": ["fmt", "spdlog", {"name": "vcpkg-cmake", "host": true}]}"#).unwrap();
        std::fs::create_dir_all(dir.join("build/debug/vcpkg_installed/vcpkg/info")).unwrap();
        for f in ["fmt_10.2.1_x64-linux.list", "spdlog_1.13.0_x64-linux.list", "zlib_1.3.1_x64-linux.list", "vcpkg-cmake_2024-04-23_x64-linux.list"] {
            std::fs::write(dir.join("build/debug/vcpkg_installed/vcpkg/info").join(f), "").unwrap();
        }
        let v = Vcpkg { vcpkg: Command::new("vcpkg"), root: root.clone(), build: Some("build/debug".into()) };
        assert_eq!(line(&v.list()), "vcpkg list --x-json --x-install-root=build/debug/vcpkg_installed");
        let deps = v.parse_list(&ok(include_str!("samples/vcpkg-list.json"))).unwrap();
        let rows: Vec<(&str, &str, bool, Option<&str>)> = deps.iter().map(|d| (d.name.as_str(), d.current.as_str(), d.direct, d.group.as_deref())).collect();
        assert_eq!(rows, [("fmt", "10.2.1", true, None), ("spdlog", "1.13.0", true, None), ("vcpkg-cmake", "2024-04-23", true, Some("host")), ("zlib", "1.3.1", false, None)]);
        let updates = v.parse_outdated(&ok(include_str!("samples/vcpkg-search.json"))).unwrap();
        assert_eq!(updates.iter().map(|u| (u.name.as_str(), u.latest.as_deref())).collect::<Vec<_>>(), [("fmt", Some("11.1.4")), ("spdlog", Some("1.15.1")), ("zlib", Some("1.3.1"))], "not fmtlog, which it doesn't have");
        let all = merge(&v, deps, &updates, &[]);
        assert_eq!(all.iter().filter(|d| d.is_outdated()).map(|d| d.name.as_str()).collect::<Vec<_>>(), ["fmt", "spdlog"]);
        assert_eq!(v.upgrade(&all[0]), None);
        assert_eq!(line(&v.upgrade_all(&[]).unwrap()), "vcpkg x-update-baseline");
        assert_eq!(line(&v.add("catch2@3.8")), "vcpkg add port catch2");
        assert_eq!(v.remove(&all[0]), None);
        assert_eq!(line(&v.search("fmt").unwrap()), "vcpkg search fmt --x-json");
        let found = v.parse_search(&ok(include_str!("samples/vcpkg-search.json"))).unwrap();
        assert_eq!((found[0].name.as_str(), found[0].version.as_str()), ("fmt", "11.1.4"));
        assert!(found[0].description.starts_with("{fmt} is"));
        let bare = Vcpkg { build: Some("build/release".into()), ..v };
        assert_eq!(line(&bare.list()), "vcpkg list --x-json", "nothing installed there: vcpkg's own");
        assert!(parse_vcpkg_list(&ok("[]"), &[]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
