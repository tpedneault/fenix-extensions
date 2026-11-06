//! A project's direct dependencies, and the versions its lockfiles hold:
//! Cargo (`Cargo.toml`, `Cargo.lock`), npm (`package.json` with
//! `package-lock.json`, `pnpm-lock.yaml` or `yarn.lock`), Python
//! (`pyproject.toml` or `requirements*.txt`, with `uv.lock`,
//! `poetry.lock`, `pdm.lock` or `Pipfile.lock`) and Go (`go.mod`,
//! confirmed by `go.sum`).
//!
//! The project's top folder is read, and the folders right under it (a
//! `web/` beside a Rust workspace), but not what a package manager keeps
//! (`node_modules`, `target`, `vendor`, `.venv`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

/// The package managers it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Cargo,
    Npm,
    Python,
    Go,
}

impl Ecosystem {
    pub const ALL: [Ecosystem; 4] = [Ecosystem::Cargo, Ecosystem::Npm, Ecosystem::Python, Ecosystem::Go];

    pub fn name(self) -> &'static str {
        match self {
            Ecosystem::Cargo => "cargo",
            Ecosystem::Npm => "npm",
            Ecosystem::Python => "python",
            Ecosystem::Go => "go",
        }
    }

    /// What an agent may call it: `rust`, `crates`, `pypi`, `node`.
    pub fn parse(text: &str) -> Option<Ecosystem> {
        match text.trim().to_ascii_lowercase().as_str() {
            "cargo" | "rust" | "crates" | "crates.io" | "crate" => Some(Ecosystem::Cargo),
            "npm" | "node" | "js" | "javascript" | "typescript" | "ts" | "pnpm" | "yarn" => Some(Ecosystem::Npm),
            "python" | "py" | "pypi" | "pip" | "uv" | "poetry" => Some(Ecosystem::Python),
            "go" | "golang" => Some(Ecosystem::Go),
            _ => None,
        }
    }
}

/// One direct dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dependency {
    pub name: String,
    /// What the manifest asks for: `^1.2`, `>=2,<3`, `v1.4.0`.
    pub requirement: String,
    /// The version the lockfile holds; `None` when nothing locks it.
    pub version: Option<String>,
    /// `normal`, `dev`, `build`, `optional`, `peer`, or a group's name.
    pub kinds: Vec<String>,
}

/// One ecosystem's manifests in the project, and what they depend on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub ecosystem: Ecosystem,
    /// The folder, relative to the project (`""` for its top).
    pub folder: String,
    pub manifests: Vec<String>,
    /// The lockfile the versions come from.
    pub lockfile: Option<String>,
    pub dependencies: Vec<Dependency>,
    /// What it couldn't read, or left out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Folders never looked in for manifests.
const SKIP: &[&str] = &["node_modules", "target", "vendor", "dist", "build", "venv", "__pycache__"];

fn read(dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(file)).ok()
}

/// Every ecosystem's dependencies under `root`: its top folder, then
/// the folders right under it, by name.
pub fn scan(root: &Path) -> Vec<Found> {
    let mut out = scan_folder(root, "");
    let mut subs: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !SKIP.contains(&n.as_str()))
        .collect();
    subs.sort();
    for sub in subs {
        for found in scan_folder(&root.join(&sub), &sub) {
            // A workspace member already read from the top.
            if !out.iter().any(|f| f.ecosystem == found.ecosystem && f.manifests.iter().any(|m| found.manifests.contains(m))) {
                out.push(found);
            }
        }
    }
    out
}

fn scan_folder(dir: &Path, folder: &str) -> Vec<Found> {
    let rel = |file: &str| if folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") };
    let mut out = Vec::new();
    for found in [cargo(dir), npm(dir), python(dir), go(dir)].into_iter().flatten() {
        out.push(Found {
            folder: folder.to_string(),
            manifests: found.manifests.iter().map(|m| rel(m)).collect(),
            lockfile: found.lockfile.as_deref().map(rel),
            ..found
        });
    }
    out
}

/// Adds `dep`, or its kind to the one of that name and version already
/// there.
fn add(deps: &mut Vec<Dependency>, dep: Dependency) {
    match deps.iter_mut().find(|d| d.name == dep.name && d.version == dep.version) {
        Some(d) => {
            for k in dep.kinds {
                if !d.kinds.contains(&k) {
                    d.kinds.push(k);
                }
            }
        }
        None => deps.push(dep),
    }
}

fn found(ecosystem: Ecosystem, manifests: Vec<String>, lockfile: Option<&str>, dependencies: Vec<Dependency>, notes: Vec<String>) -> Found {
    Found { ecosystem, folder: String::new(), manifests, lockfile: lockfile.map(str::to_string), dependencies, notes }
}

// -- Cargo --------------------------------------------------------------------

/// A `Cargo.lock`'s packages: each name's versions.
pub fn cargo_lock(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let Ok(table) = toml::from_str::<toml::Table>(text) else { return out };
    for p in table.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        if let (Some(name), Some(version)) = (p.get("name").and_then(|v| v.as_str()), p.get("version").and_then(|v| v.as_str())) {
            out.entry(name.to_string()).or_default().push(version.to_string());
        }
    }
    out
}

/// Of the versions locked, the one `requirement` takes (the newest that
/// does), else the newest.
fn pick(versions: &[String], requirement: &str) -> Option<String> {
    let mut parsed: Vec<(semver::Version, &String)> = versions.iter().filter_map(|v| Some((semver::Version::parse(v).ok()?, v))).collect();
    parsed.sort_by(|a, b| b.0.cmp(&a.0));
    if let Ok(req) = semver::VersionReq::parse(requirement.trim()) {
        if let Some((_, v)) = parsed.iter().find(|(p, _)| req.matches(p)) {
            return Some((*v).clone());
        }
    }
    parsed.first().map(|(_, v)| (*v).clone()).or_else(|| versions.first().cloned())
}

/// The dependencies a `Cargo.toml` table names: (name it's published
/// as, requirement, kind). Path-only ones are the workspace's own.
fn cargo_table(deps: Option<&toml::Value>, kind: &str, workspace: &toml::Table, out: &mut Vec<(String, String, String)>) {
    let Some(deps) = deps.and_then(|d| d.as_table()) else { return };
    for (key, spec) in deps {
        let (mut name, mut requirement, mut local) = (key.clone(), String::new(), false);
        let mut read_spec = |spec: &toml::Value| match spec {
            toml::Value::String(v) => requirement = v.clone(),
            toml::Value::Table(t) => {
                if let Some(p) = t.get("package").and_then(|v| v.as_str()) {
                    name = p.to_string();
                }
                if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                    requirement = v.to_string();
                }
                if let Some(g) = t.get("git").and_then(|v| v.as_str()) {
                    if requirement.is_empty() {
                        requirement = format!("git {g}");
                    }
                }
                if t.contains_key("path") && !t.contains_key("version") && !t.contains_key("git") {
                    local = true;
                }
            }
            _ => {}
        };
        read_spec(spec);
        if spec.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(w) = workspace.get(key) {
                read_spec(w);
            }
        }
        if !local {
            out.push((name, requirement, kind.to_string()));
        }
    }
}

fn cargo_manifest(table: &toml::Table, workspace: &toml::Table, out: &mut Vec<(String, String, String)>) {
    for (section, kind) in [("dependencies", "normal"), ("dev-dependencies", "dev"), ("build-dependencies", "build")] {
        cargo_table(table.get(section), kind, workspace, out);
        for target in table.get("target").and_then(|t| t.as_table()).into_iter().flat_map(|t| t.values()) {
            cargo_table(target.get(section), kind, workspace, out);
        }
    }
}

fn cargo(dir: &Path) -> Option<Found> {
    let text = read(dir, "Cargo.toml")?;
    let mut notes = Vec::new();
    let table: toml::Table = match toml::from_str(&text) {
        Ok(t) => t,
        Err(e) => return Some(found(Ecosystem::Cargo, vec!["Cargo.toml".into()], None, Vec::new(), vec![format!("Cargo.toml: {}", e.message())])),
    };
    let empty = toml::Table::new();
    let workspace = table.get("workspace").and_then(|w| w.as_table());
    let shared = workspace.and_then(|w| w.get("dependencies")).and_then(|d| d.as_table()).unwrap_or(&empty);
    let mut manifests = vec!["Cargo.toml".to_string()];
    let mut wanted = Vec::new();
    cargo_manifest(&table, shared, &mut wanted);
    // A workspace's members: their paths, or a folder of them (`crates/*`).
    let members: Vec<String> = workspace.and_then(|w| w.get("members")).and_then(|m| m.as_array()).into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
    let mut member_dirs = Vec::new();
    for m in members {
        match m.strip_suffix("/*") {
            Some(parent) => {
                let mut subs: Vec<String> = std::fs::read_dir(dir.join(parent)).into_iter().flatten().flatten().filter(|e| e.path().join("Cargo.toml").is_file()).map(|e| format!("{parent}/{}", e.file_name().to_string_lossy())).collect();
                subs.sort();
                member_dirs.extend(subs);
            }
            None if m.contains('*') => notes.push(format!("workspace member {m:?}: only a folder's /* is read")),
            None => member_dirs.push(m),
        }
    }
    for m in member_dirs {
        let Some(text) = read(&dir.join(&m), "Cargo.toml") else { continue };
        match toml::from_str::<toml::Table>(&text) {
            Ok(t) => cargo_manifest(&t, shared, &mut wanted),
            Err(e) => notes.push(format!("{m}/Cargo.toml: {}", e.message())),
        }
        manifests.push(format!("{m}/Cargo.toml"));
    }
    // A member's lockfile is its workspace's, above it.
    let lock_text = read(dir, "Cargo.lock");
    let locked = lock_text.as_deref().map(cargo_lock).unwrap_or_default();
    let mut deps = Vec::new();
    for (name, requirement, kind) in wanted {
        let version = locked.get(&name).and_then(|v| pick(v, &requirement));
        add(&mut deps, Dependency { name, requirement, version, kinds: vec![kind] });
    }
    if lock_text.is_none() && !deps.is_empty() {
        notes.push("no Cargo.lock here (cargo generate-lockfile writes one; a workspace member's is its workspace's)".into());
    }
    Some(found(Ecosystem::Cargo, manifests, lock_text.is_some().then_some("Cargo.lock"), deps, notes))
}

// -- npm ----------------------------------------------------------------------

/// `package-lock.json` (or `npm-shrinkwrap.json`): each top-level
/// package's version.
pub fn npm_lock(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else { return out };
    // Lockfile v2 and v3: by where it's installed.
    if let Some(packages) = json.get("packages").and_then(|p| p.as_object()) {
        for (path, p) in packages {
            let Some(name) = path.strip_prefix("node_modules/").filter(|n| !n.contains("/node_modules/")) else { continue };
            if let Some(v) = p.get("version").and_then(|v| v.as_str()) {
                out.insert(name.to_string(), v.to_string());
            }
        }
    }
    // v1.
    if let Some(deps) = json.get("dependencies").and_then(|p| p.as_object()) {
        for (name, p) in deps {
            if let Some(v) = p.get("version").and_then(|v| v.as_str()) {
                out.entry(name.clone()).or_insert_with(|| v.to_string());
            }
        }
    }
    out
}

/// A YAML key, its quotes off.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).or_else(|| s.strip_prefix('"').and_then(|s| s.strip_suffix('"'))).unwrap_or(s)
}

/// A pnpm version without its peers' suffix, `1.2.3(react@18.2.0)`; none
/// for a link to a folder.
fn pnpm_version(v: &str) -> Option<String> {
    let v = unquote(v);
    let v = v.split('(').next().unwrap_or(v).trim();
    (!v.is_empty() && !v.starts_with("link:") && !v.starts_with("file:")).then(|| v.to_string())
}

/// `pnpm-lock.yaml`: the project's own dependencies' versions -- from
/// `importers: .:` (lockfile 6 and 9), else the top-level sections
/// (lockfile 5, and 6 without workspaces).
pub fn pnpm_lock(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let lines: Vec<(usize, &str)> = text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).map(|l| (l.len() - l.trim_start().len(), l.trim())).collect();
    // Where the sections are: `importers: .:` at 2 then 4, else the top.
    let mut base = None;
    for (i, (indent, line)) in lines.iter().enumerate() {
        if *indent == 0 && *line == "importers:" {
            base = lines[i + 1..].iter().take_while(|(n, _)| *n > 0).position(|(n, l)| *n == 2 && unquote(l.trim_end_matches(':')) == ".").map(|p| (i + 1 + p + 1, 4));
            break;
        }
    }
    let (start, section_indent) = base.unwrap_or((0, 0));
    let mut i = start;
    while i < lines.len() {
        let (indent, line) = lines[i];
        if indent < section_indent {
            break;
        }
        let is_section = indent == section_indent && matches!(line, "dependencies:" | "devDependencies:" | "optionalDependencies:");
        i += 1;
        if !is_section {
            continue;
        }
        while i < lines.len() && lines[i].0 > section_indent {
            let (n, entry) = lines[i];
            i += 1;
            if n != section_indent + 2 {
                continue;
            }
            let Some((key, value)) = entry.split_once(':').map(|(k, v)| (unquote(k), v.trim())) else { continue };
            // Lockfile 5: `name: 1.2.3`. 6 and 9: `version:` under it.
            let version = if !value.is_empty() {
                pnpm_version(value)
            } else {
                let mut v = None;
                while i < lines.len() && lines[i].0 > n {
                    if let Some(rest) = lines[i].1.strip_prefix("version:") {
                        v = pnpm_version(rest);
                    }
                    i += 1;
                }
                v
            };
            if let Some(v) = version {
                out.insert(key.to_string(), v);
            }
        }
    }
    out
}

/// A `name@range` spec's name: `@babel/core@^7` is `@babel/core`.
fn spec_name(spec: &str) -> Option<(&str, &str)> {
    let spec = unquote(spec);
    let at = spec.get(1..)?.find('@')? + 1;
    Some((&spec[..at], &spec[at + 1..]))
}

/// `yarn.lock`, classic or Berry: each `name@range` an entry resolves,
/// and the version it resolved to.
pub fn yarn_lock(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut specs: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            specs = line.trim_end_matches(':').split(", ").filter_map(spec_name).map(|(n, r)| (n.to_string(), r.trim_start_matches("npm:").to_string())).collect();
            continue;
        }
        let t = line.trim();
        // Classic: `version "1.2.3"`; Berry: `version: 1.2.3`.
        if let Some(rest) = t.strip_prefix("version") {
            let v = unquote(rest.trim_start_matches(':')).to_string();
            for (name, range) in specs.drain(..) {
                out.push((name, range, v.clone()));
            }
        }
    }
    out
}

/// The version a lockfile holds of a package, by its name and range.
type VersionOf = Box<dyn Fn(&str, &str) -> Option<String>>;

fn npm(dir: &Path) -> Option<Found> {
    let text = read(dir, "package.json")?;
    let mut notes = Vec::new();
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(j) => j,
        Err(e) => return Some(found(Ecosystem::Npm, vec!["package.json".into()], None, Vec::new(), vec![format!("package.json: {e}")])),
    };
    let mut wanted = Vec::new();
    for (section, kind) in [("dependencies", "normal"), ("devDependencies", "dev"), ("optionalDependencies", "optional"), ("peerDependencies", "peer")] {
        for (name, req) in json.get(section).and_then(|d| d.as_object()).into_iter().flatten() {
            wanted.push((name.clone(), req.as_str().unwrap_or("").to_string(), kind));
        }
    }
    if json.get("workspaces").is_some() {
        notes.push("a workspace: only the top package.json's dependencies are listed".into());
    }
    let (lockfile, version_of): (Option<&str>, VersionOf) = if let Some(t) = read(dir, "package-lock.json").or_else(|| read(dir, "npm-shrinkwrap.json")) {
        let name = if dir.join("package-lock.json").is_file() { "package-lock.json" } else { "npm-shrinkwrap.json" };
        let locked = npm_lock(&t);
        (Some(name), Box::new(move |n, _| locked.get(n).cloned()))
    } else if let Some(t) = read(dir, "pnpm-lock.yaml") {
        let locked = pnpm_lock(&t);
        (Some("pnpm-lock.yaml"), Box::new(move |n, _| locked.get(n).cloned()))
    } else if let Some(t) = read(dir, "yarn.lock") {
        let locked = yarn_lock(&t);
        (
            Some("yarn.lock"),
            Box::new(move |n, req| {
                let of_name: Vec<&(String, String, String)> = locked.iter().filter(|(name, _, _)| name == n).collect();
                of_name.iter().find(|(_, range, _)| range == req).or(of_name.first()).map(|(_, _, v)| v.clone())
            }),
        )
    } else {
        if !wanted.is_empty() {
            notes.push("no package-lock.json, pnpm-lock.yaml or yarn.lock here: versions aren't locked".into());
        }
        (None, Box::new(|_, _| None))
    };
    let mut deps = Vec::new();
    for (name, requirement, kind) in wanted {
        let version = version_of(&name, &requirement);
        add(&mut deps, Dependency { name, requirement, version, kinds: vec![kind.to_string()] });
    }
    Some(found(Ecosystem::Npm, vec!["package.json".into()], lockfile, deps, notes))
}

// -- Python -------------------------------------------------------------------

/// A package name as PyPI compares them (PEP 503): `Foo_Bar` is
/// `foo-bar`.
pub fn normalize(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// A PEP 508 requirement's name and the rest: `requests[socks]>=2 ;
/// python_version>"3"` is `requests`, `>=2`.
pub fn pep508(spec: &str) -> Option<(String, String)> {
    let spec = spec.split('#').next().unwrap_or("").trim();
    let end = spec.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(spec.len());
    let name = &spec[..end];
    if name.is_empty() {
        return None;
    }
    let mut rest = spec[end..].trim();
    if rest.starts_with('[') {
        rest = rest.find(']').map_or("", |i| &rest[i + 1..]).trim();
    }
    let rest = rest.split(';').next().unwrap_or("").trim().trim_start_matches('(').trim_end_matches(')').trim();
    Some((name.to_string(), rest.to_string()))
}

/// The `[[package]]`s of a TOML lockfile (`uv.lock`, `poetry.lock`,
/// `pdm.lock`), by normalized name.
pub fn toml_packages(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(table) = toml::from_str::<toml::Table>(text) else { return out };
    for p in table.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        if let (Some(name), Some(version)) = (p.get("name").and_then(|v| v.as_str()), p.get("version").and_then(|v| v.as_str())) {
            out.insert(normalize(name), version.to_string());
        }
    }
    out
}

/// `Pipfile.lock`'s packages, by normalized name.
fn pipfile_lock(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else { return out };
    for section in ["default", "develop"] {
        for (name, p) in json.get(section).and_then(|s| s.as_object()).into_iter().flatten() {
            if let Some(v) = p.get("version").and_then(|v| v.as_str()) {
                out.insert(normalize(name), v.trim_start_matches("==").to_string());
            }
        }
    }
    out
}

/// A requirements file's lines: (name, specifier). `-r`, `-e`, options
/// and URLs are left out.
pub fn requirements(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.split(" #").next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-') && !l.contains("://"))
        .filter_map(pep508)
        .collect()
}

/// An exact pin's version: `==1.2.3`.
fn pinned(spec: &str) -> Option<String> {
    let v = spec.strip_prefix("==")?.trim();
    (!v.contains(['*', ',', ' '])).then(|| v.to_string())
}

fn python(dir: &Path) -> Option<Found> {
    let pyproject = read(dir, "pyproject.toml");
    let mut req_files: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("requirements") && n.ends_with(".txt"))
        .collect();
    req_files.sort();
    if pyproject.is_none() && req_files.is_empty() {
        return None;
    }
    let mut notes = Vec::new();
    let mut manifests = Vec::new();
    let mut wanted: Vec<(String, String, String)> = Vec::new();
    if let Some(text) = &pyproject {
        manifests.push("pyproject.toml".to_string());
        match toml::from_str::<toml::Table>(text) {
            Ok(t) => {
                let list = |v: Option<&toml::Value>, kind: &str, wanted: &mut Vec<(String, String, String)>| {
                    for spec in v.and_then(|v| v.as_array()).into_iter().flatten().filter_map(|s| s.as_str()) {
                        if let Some((n, r)) = pep508(spec) {
                            wanted.push((n, r, kind.to_string()));
                        }
                    }
                };
                let project = t.get("project");
                list(project.and_then(|p| p.get("dependencies")), "normal", &mut wanted);
                for (group, v) in project.and_then(|p| p.get("optional-dependencies")).and_then(|o| o.as_table()).into_iter().flatten() {
                    list(Some(v), &format!("optional ({group})"), &mut wanted);
                }
                for (group, v) in t.get("dependency-groups").and_then(|o| o.as_table()).into_iter().flatten() {
                    list(Some(v), if group == "dev" { "dev" } else { group }, &mut wanted);
                }
                // Poetry's tables: `name = "^1.2"` or `{ version = ... }`.
                let poetry = t.get("tool").and_then(|t| t.get("poetry"));
                let table = |v: Option<&toml::Value>, kind: &str, wanted: &mut Vec<(String, String, String)>| {
                    for (name, spec) in v.and_then(|v| v.as_table()).into_iter().flatten() {
                        if name.eq_ignore_ascii_case("python") {
                            continue;
                        }
                        let req = spec.as_str().or_else(|| spec.get("version").and_then(|v| v.as_str())).unwrap_or("").to_string();
                        wanted.push((name.clone(), req, kind.to_string()));
                    }
                };
                table(poetry.and_then(|p| p.get("dependencies")), "normal", &mut wanted);
                table(poetry.and_then(|p| p.get("dev-dependencies")), "dev", &mut wanted);
                for (group, g) in poetry.and_then(|p| p.get("group")).and_then(|g| g.as_table()).into_iter().flatten() {
                    table(g.get("dependencies"), group, &mut wanted);
                }
            }
            Err(e) => notes.push(format!("pyproject.toml: {}", e.message())),
        }
    }
    let mut pins = BTreeMap::new();
    for file in &req_files {
        let Some(text) = read(dir, file) else { continue };
        manifests.push(file.clone());
        let kind = if file.contains("dev") || file.contains("test") { "dev" } else { "normal" };
        for (name, spec) in requirements(&text) {
            if let Some(v) = pinned(&spec) {
                pins.insert(normalize(&name), v);
            }
            // With a pyproject.toml, a requirements file is usually its
            // export: its pins lock what the pyproject asks for.
            if pyproject.is_none() {
                wanted.push((name, spec, kind.to_string()));
            }
        }
    }
    let lock = ["uv.lock", "poetry.lock", "pdm.lock", "Pipfile.lock"].into_iter().find_map(|f| read(dir, f).map(|t| (f, t)));
    let locked = match &lock {
        Some(("Pipfile.lock", t)) => pipfile_lock(t),
        Some((_, t)) => toml_packages(t),
        None => BTreeMap::new(),
    };
    let lockfile = lock.as_ref().map(|(f, _)| *f).or_else(|| (!pins.is_empty()).then_some(req_files.first().map(String::as_str).unwrap_or("requirements.txt")));
    let lockfile = lockfile.map(str::to_string);
    let mut deps = Vec::new();
    for (name, requirement, kind) in wanted {
        let key = normalize(&name);
        let version = locked.get(&key).or_else(|| pins.get(&key)).cloned();
        add(&mut deps, Dependency { name, requirement, version, kinds: vec![kind] });
    }
    if lock.is_none() && pins.is_empty() && !deps.is_empty() {
        notes.push("no uv.lock, poetry.lock, pdm.lock, Pipfile.lock or == pins here: versions aren't locked".into());
    }
    Some(Found { ecosystem: Ecosystem::Python, folder: String::new(), manifests, lockfile, dependencies: deps, notes })
}

// -- Go -----------------------------------------------------------------------

/// `go.mod`'s requirements: (module, version, indirect).
pub fn go_mod(text: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let (code, comment) = match line.split_once("//") {
            Some((c, m)) => (c.trim(), m.trim()),
            None => (line.trim(), ""),
        };
        let entry = if in_block {
            if code == ")" {
                in_block = false;
                continue;
            }
            code
        } else if code == "require (" || code == "require(" {
            in_block = true;
            continue;
        } else if let Some(rest) = code.strip_prefix("require ") {
            rest.trim()
        } else {
            continue;
        };
        let mut parts = entry.split_whitespace();
        if let (Some(module), Some(version)) = (parts.next(), parts.next()) {
            out.push((unquote(module).to_string(), version.to_string(), comment.starts_with("indirect")));
        }
    }
    out
}

fn go(dir: &Path) -> Option<Found> {
    let text = read(dir, "go.mod")?;
    let sum = read(dir, "go.sum");
    let mut notes = Vec::new();
    let mut deps = Vec::new();
    for (module, version, indirect) in go_mod(&text) {
        if indirect {
            continue;
        }
        let in_sum = sum.as_deref().is_some_and(|s| s.lines().any(|l| l.split_whitespace().take(2).eq([module.as_str(), version.as_str()])));
        if sum.is_some() && !in_sum {
            notes.push(format!("{module} {version} isn't in go.sum (go mod tidy)"));
        }
        add(&mut deps, Dependency { requirement: version.clone(), version: Some(version), name: module, kinds: vec!["normal".into()] });
    }
    if text.lines().any(|l| l.trim_start().starts_with("replace")) {
        notes.push("go.mod replaces some modules: the docs are the original module's".into());
    }
    Some(Found { ecosystem: Ecosystem::Go, folder: String::new(), manifests: vec!["go.mod".into()], lockfile: sum.is_some().then(|| "go.sum".to_string()), dependencies: deps, notes })
}

/// A dependency by name, in the ecosystems found (the one named, when
/// it is): its ecosystem and lockfile with it. The top folder's first.
pub fn find<'a>(all: &'a [Found], name: &str, ecosystem: Option<Ecosystem>) -> Option<(&'a Found, &'a Dependency)> {
    let wanted = normalize(name);
    all.iter().filter(|f| ecosystem.is_none_or(|e| e == f.ecosystem)).find_map(|f| {
        let same = |d: &&Dependency| if f.ecosystem == Ecosystem::Python { normalize(&d.name) == wanted } else { d.name == name || (f.ecosystem == Ecosystem::Cargo && d.name.replace('_', "-") == name.replace('_', "-")) };
        f.dependencies.iter().find(same).map(|d| (f, d))
    })
}

/// A package the lockfiles hold that isn't a direct dependency -- one a
/// dependency pulls in: its version, from the first lockfile of `found`
/// (each folder's, under `root`) that has it.
pub fn locked(root: &Path, found: &[Found], name: &str, ecosystem: Option<Ecosystem>) -> Option<(Ecosystem, String, String)> {
    for f in found.iter().filter(|f| ecosystem.is_none_or(|e| e == f.ecosystem)) {
        let Some(lockfile) = &f.lockfile else { continue };
        let Ok(text) = std::fs::read_to_string(root.join(lockfile)) else { continue };
        let file = lockfile.rsplit('/').next().unwrap_or(lockfile);
        let version = match (f.ecosystem, file) {
            (Ecosystem::Cargo, _) => {
                let all = cargo_lock(&text);
                all.get(name).or_else(|| all.get(&name.replace('_', "-"))).and_then(|v| pick(v, ""))
            }
            (Ecosystem::Npm, "pnpm-lock.yaml") => pnpm_packages(&text).remove(name),
            (Ecosystem::Npm, "yarn.lock") => yarn_lock(&text).into_iter().find(|(n, _, _)| n == name).map(|(_, _, v)| v),
            (Ecosystem::Npm, _) => npm_lock(&text).remove(name),
            (Ecosystem::Python, "Pipfile.lock") => pipfile_lock(&text).remove(&normalize(name)),
            (Ecosystem::Python, f) if f.ends_with(".txt") => requirements(&text).into_iter().find(|(n, _)| normalize(n) == normalize(name)).and_then(|(_, s)| pinned(&s)),
            (Ecosystem::Python, _) => toml_packages(&text).remove(&normalize(name)),
            // go.sum: the go.mod beside it names every module needed.
            (Ecosystem::Go, _) => std::fs::read_to_string(root.join(f.manifests.first()?)).ok().and_then(|m| go_mod(&m).into_iter().find(|(n, _, _)| n == name).map(|(_, v, _)| v)),
        };
        if let Some(v) = version {
            return Some((f.ecosystem, v, lockfile.clone()));
        }
    }
    None
}

/// `pnpm-lock.yaml`'s `packages:` keys, each name's version: `/name@1.2.3`
/// (6), `name@1.2.3` (9) or `/name/1.2.3` (5).
fn pnpm_packages(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            inside = line.trim() == "packages:";
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if !inside || indent != 2 {
            continue;
        }
        let key = unquote(line.trim().trim_end_matches(':'));
        let key = key.strip_prefix('/').unwrap_or(key);
        let key = key.split('(').next().unwrap_or(key);
        let split = spec_name(key).or_else(|| key.rsplit_once('/').filter(|(_, v)| v.starts_with(|c: char| c.is_ascii_digit())));
        if let Some((name, version)) = split {
            out.entry(name.to_string()).or_insert_with(|| version.split('_').next().unwrap_or(version).to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata(name: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata").join(name)).unwrap()
    }

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn versions(f: &Found) -> Vec<(String, Option<String>)> {
        f.dependencies.iter().map(|d| (d.name.clone(), d.version.clone())).collect()
    }

    fn pair(n: &str, v: &str) -> (String, Option<String>) {
        (n.to_string(), Some(v.to_string()))
    }

    #[test]
    fn a_cargo_workspace_its_members_and_the_versions_locked() {
        let dir = project(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"app\"]\n\n[workspace.dependencies]\nserde = { version = \"1.0\", features = [\"derive\"] }\n"),
            ("app/Cargo.toml", "[package]\nname = \"app\"\n\n[dependencies]\nserde.workspace = true\nlib = { path = \"../crates/lib\" }\nrand = \"0.8\"\n\n[dev-dependencies]\ntempfile = \"3\"\n"),
            ("crates/lib/Cargo.toml", "[package]\nname = \"lib\"\n\n[dependencies]\nrand_old = { package = \"rand\", version = \"0.7\" }\n\n[target.'cfg(unix)'.build-dependencies]\ncc = \"1\"\n"),
            ("Cargo.lock", &testdata("Cargo.lock")),
        ]);
        let all = scan(dir.path());
        assert_eq!(all.len(), 1, "the members are the workspace's: {all:?}");
        let f = &all[0];
        assert_eq!(f.lockfile.as_deref(), Some("Cargo.lock"));
        assert_eq!(f.manifests, ["Cargo.toml", "crates/lib/Cargo.toml", "app/Cargo.toml"]);
        assert_eq!(versions(f), [pair("rand", "0.7.3"), pair("cc", "1.0.83"), pair("rand", "0.8.5"), pair("serde", "1.0.195"), pair("tempfile", "3.9.0")], "lib's first, then app's; a path dependency left out");
        let rand = f.dependencies.iter().find(|d| d.name == "rand").unwrap();
        assert_eq!(rand.kinds, ["normal"]);
        assert_eq!(find(&all, "serde", None).unwrap().1.requirement, "1.0", "the workspace's requirement");
        assert_eq!(find(&all, "rand", Some(Ecosystem::Cargo)).unwrap().1.version.as_deref(), Some("0.7.3"));
        assert!(find(&all, "rand", Some(Ecosystem::Npm)).is_none());
    }

    #[test]
    fn of_two_versions_locked_the_one_the_requirement_takes() {
        let locked = cargo_lock(&testdata("Cargo.lock"));
        assert_eq!(locked["rand"], ["0.7.3", "0.8.5"]);
        assert_eq!(pick(&locked["rand"], "0.8").as_deref(), Some("0.8.5"));
        assert_eq!(pick(&locked["rand"], "0.7").as_deref(), Some("0.7.3"));
        assert_eq!(pick(&locked["rand"], "git https://x").as_deref(), Some("0.8.5"), "can't tell: the newest");
    }

    #[test]
    fn npm_with_each_lockfile() {
        let manifest = r#"{"dependencies": {"react": "^18.2.0", "@babel/core": "^7.23.0"}, "devDependencies": {"typescript": "~5.3.0"}}"#;
        for lock in ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "yarn-berry.lock"] {
            let file = if lock == "yarn-berry.lock" { "yarn.lock" } else { lock };
            let dir = project(&[("package.json", manifest), (file, &testdata(lock))]);
            let all = scan(dir.path());
            assert_eq!(all[0].lockfile.as_deref(), Some(file));
            assert_eq!(versions(&all[0]), [pair("@babel/core", "7.23.7"), pair("react", "18.2.0"), pair("typescript", "5.3.3")], "{lock}");
            assert_eq!(all[0].dependencies[2].kinds, ["dev"]);
        }
        let dir = project(&[("package.json", manifest)]);
        let all = scan(dir.path());
        assert_eq!(all[0].dependencies[0].version, None);
        assert!(all[0].notes[0].contains("aren't locked"));
    }

    #[test]
    fn pnpm_5_has_no_importers() {
        let lock = "lockfileVersion: 5.4\n\nspecifiers:\n  lodash: ^4.17.0\n\ndependencies:\n  lodash: 4.17.21\n\ndevDependencies:\n  vitest: 1.1.0_@types+node@20.10.0\n\npackages:\n\n  /lodash/4.17.21:\n    resolution: {integrity: sha512-x}\n";
        let locked = pnpm_lock(lock);
        assert_eq!(locked["lodash"], "4.17.21");
        assert_eq!(locked.len(), 2);
    }

    #[test]
    fn python_with_uv_poetry_and_pins() {
        let pyproject = "[project]\nname = \"x\"\ndependencies = [\"Requests[socks]>=2.31 ; python_version > '3.8'\", \"pydantic_core\"]\n\n[project.optional-dependencies]\ndocs = [\"mkdocs\"]\n\n[dependency-groups]\ndev = [\"pytest>=7\"]\n";
        let dir = project(&[("pyproject.toml", pyproject), ("uv.lock", &testdata("uv.lock"))]);
        let all = scan(dir.path());
        let f = &all[0];
        assert_eq!(f.lockfile.as_deref(), Some("uv.lock"));
        assert_eq!(versions(f), [pair("Requests", "2.31.0"), pair("pydantic_core", "2.14.6"), pair("mkdocs", "1.5.3"), pair("pytest", "7.4.4")]);
        assert_eq!(f.dependencies[0].requirement, ">=2.31");
        assert_eq!(f.dependencies[2].kinds, ["optional (docs)"]);
        assert_eq!(find(&all, "pydantic-core", None).unwrap().1.name, "pydantic_core", "names compared as PyPI does");

        let poetry = "[tool.poetry.dependencies]\npython = \"^3.11\"\nflask = \"^3.0\"\n\n[tool.poetry.group.test.dependencies]\npytest = { version = \"^7.4\" }\n";
        let dir = project(&[("pyproject.toml", poetry), ("poetry.lock", "[[package]]\nname = \"Flask\"\nversion = \"3.0.0\"\n\n[[package]]\nname = \"pytest\"\nversion = \"7.4.3\"\n")]);
        let f = &scan(dir.path())[0];
        assert_eq!(versions(f), [pair("flask", "3.0.0"), pair("pytest", "7.4.3")]);
        assert_eq!(f.dependencies[1].kinds, ["test"]);

        let dir = project(&[("requirements.txt", "# app\n-r base.txt\nDjango==4.2.9  # LTS\nnumpy>=1.26\nhttps://example.com/x.whl\n"), ("requirements-dev.txt", "black==23.12.1\n")]);
        let f = &scan(dir.path())[0];
        assert_eq!(f.manifests, ["requirements-dev.txt", "requirements.txt"]);
        assert_eq!(versions(f), [pair("black", "23.12.1"), pair("Django", "4.2.9"), ("numpy".into(), None)]);
        assert_eq!(f.dependencies[0].kinds, ["dev"]);
    }

    #[test]
    fn go_modules_direct_ones_only() {
        let dir = project(&[("go.mod", &testdata("go.mod")), ("go.sum", &testdata("go.sum"))]);
        let all = scan(dir.path());
        let f = &all[0];
        assert_eq!(f.lockfile.as_deref(), Some("go.sum"));
        assert_eq!(versions(f), [pair("github.com/spf13/cobra", "v1.8.0"), pair("golang.org/x/sync", "v0.6.0"), pair("github.com/google/uuid", "v1.5.0")]);
        assert!(f.notes.iter().any(|n| n.contains("github.com/google/uuid v1.5.0 isn't in go.sum")), "{:?}", f.notes);
    }

    #[test]
    fn folders_under_the_top_are_read_but_not_what_package_managers_keep() {
        let dir = project(&[
            ("go.mod", "module x\n\nrequire golang.org/x/sync v0.6.0\n"),
            ("web/package.json", r#"{"dependencies": {"vue": "^3.4.0"}}"#),
            ("node_modules/x/package.json", r#"{"dependencies": {"nope": "1"}}"#),
            (".hidden/Cargo.toml", "[dependencies]\nnope = \"1\"\n"),
        ]);
        let all = scan(dir.path());
        assert_eq!(all.iter().map(|f| (f.ecosystem, f.folder.as_str())).collect::<Vec<_>>(), [(Ecosystem::Go, ""), (Ecosystem::Npm, "web")]);
        assert_eq!(all[1].manifests, ["web/package.json"]);
    }

    #[test]
    fn a_package_pulled_in_is_found_in_the_lockfile() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"app\"\n\n[dependencies]\nserde = \"1\"\n"), ("Cargo.lock", &testdata("Cargo.lock")), ("web/package.json", r#"{"dependencies": {"react": "^18.2.0"}}"#), ("web/pnpm-lock.yaml", &testdata("pnpm-lock.yaml"))]);
        let all = scan(dir.path());
        assert_eq!(locked(dir.path(), &all, "rand", None), Some((Ecosystem::Cargo, "0.8.5".into(), "Cargo.lock".into())), "the newest of two");
        assert_eq!(locked(dir.path(), &all, "@babel/core", None), Some((Ecosystem::Npm, "7.23.7".into(), "web/pnpm-lock.yaml".into())));
        assert_eq!(locked(dir.path(), &all, "rand", Some(Ecosystem::Npm)), None);
        let dir = project(&[("go.mod", &testdata("go.mod")), ("go.sum", &testdata("go.sum")), ("requirements.txt", "urllib3==2.1.0\n")]);
        let all = scan(dir.path());
        assert_eq!(locked(dir.path(), &all, "github.com/inconshreveable/mousetrap", None).map(|l| l.1), Some("v1.1.0".into()), "an indirect module");
        assert_eq!(locked(dir.path(), &all, "URLLib3", None).map(|l| l.1), Some("2.1.0".into()));
        assert_eq!(pnpm_packages("packages:\n\n  /lodash/4.17.21:\n    resolution: {}\n  /@types/node@20.10.0:\n    x: y\n  'react@18.2.0(x@1)':\n"), BTreeMap::from([("lodash".into(), "4.17.21".into()), ("@types/node".into(), "20.10.0".into()), ("react".into(), "18.2.0".into())]));
    }

    #[test]
    fn requirement_names() {
        assert_eq!(pep508("requests[socks,security] (>=2.0) ; extra == 'x'"), Some(("requests".into(), ">=2.0".into())));
        assert_eq!(pep508("zope.interface"), Some(("zope.interface".into(), String::new())));
        assert_eq!(normalize("Zope.Interface__x"), "zope-interface-x");
        assert_eq!(spec_name("\"@babel/core@npm:^7.0.0\""), Some(("@babel/core", "npm:^7.0.0")));
        assert_eq!(Ecosystem::parse("PyPI"), Some(Ecosystem::Python));
        assert_eq!(Ecosystem::parse("maven"), None);
    }
}
