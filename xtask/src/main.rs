//! The repository's tasks:
//!
//! ```text
//! cargo xtask validate [index.json]          every extension's manifest, checked; dependencies
//!                                            found here or in the index, their versions there
//! cargo xtask build <extension-dir>          a code extension's extension.wasm, from its source
//! cargo xtask pack <extension-dir> [dist]    a .fenixext (built first); prints its id, version, sha256, size
//! cargo xtask for-tag <name>-v<version>      the extension folder a release tag names
//! cargo xtask index-add <index.json> <extension-dir> <url> <sha256> <size>
//! cargo xtask external-check [--offline] [--index index.json]   external.toml and publishers.toml
//! cargo xtask index-add-external <index.json> [--only <id>@<version>]
//! cargo xtask screenshots [name] [--dry-run]   each extension's screenshots.toml, run in Fenix and captured
//! cargo xtask site <index.json> <out>        the catalogue, for GitHub Pages
//! cargo xtask new <kind> <name | publisher.name> [--name "Display name"]   a new extension from a template (theme, keymap, snippets, language-pack, tool)
//! cargo xtask install <extension-dir> [--fenix <path>] [--watch]   pack it and install it into Fenix; --watch, again whenever a file changes
//! cargo xtask theme-import <theme.json> [--out <dir>] [--name "..."] [--fenix <version>] [--source "..."]   a VS Code colour theme as a Fenix theme file, and what it couldn't map
//! cargo xtask snippets-import <snippets.json> --language <id> [--into <extension-dir>] [--approximate] [--indent <n>]   VS Code snippets as Fenix's, and what its format can't say
//! ```
//!
//! The manifest is read the way Fenix reads it (`fenix-extensions`'
//! `manifest.rs`): what's refused here would be refused there.

mod external;
mod fenix_theme;
mod install;
mod jsonc;
mod lessons;
mod new;
mod snippets_import;
mod theme_import;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

mod screenshots;
mod site;

#[derive(Debug, Deserialize)]
struct Manifest {
    extension: Info,
    engine: Engine,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    /// What a code extension may do: copied into the index as it is, so
    /// Fenix shows it before installing.
    #[serde(default)]
    permissions: Option<toml::Table>,
}

#[derive(Debug, Deserialize)]
struct Info {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    repository: Option<String>,
    runtime: String,
}

#[derive(Debug, Deserialize)]
struct Engine {
    fenix: String,
    /// The extension API a code extension was built against.
    #[serde(default)]
    api: Option<String>,
}

/// What runs an extension: nothing (data), its code, or -- a bundle --
/// nothing of its own: it's the extensions it depends on.
const RUNTIMES: &[&str] = &["none", "wasm", "lua", "bundle"];

/// What an extension may be filed under: the Extensions page has a place
/// for each.
const CATEGORIES: &[&str] = &["languages", "themes", "snippets", "keymaps", "templates", "tools", "data", "web", "mission", "fun"];

/// The pictures `screenshots/` may hold, as Fenix reads them, and the
/// most one may be.
const SCREENSHOTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const MAX_SCREENSHOT: u64 = 2 * 1024 * 1024;

/// What a bundle's folder may carry besides its manifest.
const BUNDLE_FILES: &[&str] = &["README.md", "CHANGELOG.md", "screenshots"];

fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
}

fn manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("extension.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
    let at = path.display();
    if !valid_id(&m.extension.id) {
        return Err(format!("{at}: id {:?} isn't publisher.name", m.extension.id));
    }
    if m.extension.name.trim().is_empty() {
        return Err(format!("{at}: name is empty"));
    }
    semver::Version::parse(&m.extension.version).map_err(|e| format!("{at}: version: {e}"))?;
    semver::VersionReq::parse(&m.engine.fenix).map_err(|e| format!("{at}: engine.fenix: {e}"))?;
    if !RUNTIMES.contains(&m.extension.runtime.as_str()) {
        return Err(format!("{at}: runtime {:?} -- one of {}", m.extension.runtime, RUNTIMES.join(", ")));
    }
    if let Some(c) = m.extension.categories.iter().find(|c| !CATEGORIES.contains(&c.as_str())) {
        return Err(format!("{at}: category {c:?} -- one of {}", CATEGORIES.join(", ")));
    }
    // Fenix 2.1.0 and before read a setting's default as text only: a bare
    // `default = true` made fenix.java 1.1.0 refuse to install.
    let raw: toml::Table = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
    let settings = raw.get("contributes").and_then(|c| c.get("settings")).and_then(|s| s.as_array()).cloned().unwrap_or_default();
    for setting in &settings {
        if let Some(default) = setting.get("default").filter(|d| !d.is_str()) {
            let key = setting.get("key").and_then(|k| k.as_str()).unwrap_or("?");
            return Err(format!("{at}: setting {key}: default = {default} -- write it as text (\"{}\"): Fenix 2.1.0 and before read no other", default.to_string().trim_matches('"')));
        }
    }
    if m.extension.runtime == "bundle" {
        if m.dependencies.is_empty() {
            return Err(format!("{at}: a bundle is the extensions it depends on, and it has none"));
        }
        if m.engine.api.is_some() || m.permissions.as_ref().is_some_and(|p| !p.is_empty()) {
            return Err(format!("{at}: a bundle runs nothing: no engine.api, no permissions"));
        }
        let first = |rel: &Path| rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(extra) = files(dir).iter().map(|rel| first(rel)).find(|f| f != "extension.toml" && !BUNDLE_FILES.contains(&f.as_str())) {
            return Err(format!("{at}: a bundle carries only its manifest, {} -- not {extra}", BUNDLE_FILES.join(", ")));
        }
    }
    if m.extension.runtime == "wasm" {
        let api = m.engine.api.as_deref().ok_or(format!("{at}: a wasm extension says its API (engine.api)"))?;
        let full = match api.matches('.').count() { 0 => format!("{api}.0.0"), 1 => format!("{api}.0"), _ => api.to_string() };
        semver::Version::parse(&full).map_err(|e| format!("{at}: engine.api: {e}"))?;
        if !dir.join("extension.wasm").is_file() && !dir.join("Cargo.toml").is_file() {
            return Err(format!("{at}: a wasm extension needs its extension.wasm, or a Cargo.toml to build it from"));
        }
    }
    for (dep, range) in &m.dependencies {
        if !valid_id(dep) {
            return Err(format!("{at}: dependency {dep:?} isn't an id"));
        }
        semver::VersionReq::parse(range).map_err(|e| format!("{at}: dependency {dep}: {e}"))?;
    }
    Ok(m)
}

/// What a code extension's source leaves out of its package: the
/// crate, and what building it makes besides `extension.wasm`.
fn source_only(rel: &Path) -> bool {
    let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    ["src", "target", "tests", "Cargo.toml", "Cargo.lock", ".gitignore"].contains(&first.as_str())
}

/// A code extension with a `Cargo.toml`: built for `wasm32-wasip2`, its
/// component copied to `extension.wasm`. One without has nothing to build.
fn build(dir: &Path) -> Result<(), String> {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&cargo_toml).map_err(|e| e.to_string())?;
    let crate_name = toml::from_str::<toml::Table>(&text)
        .map_err(|e| format!("{}: {}", cargo_toml.display(), e.message()))?
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or(format!("{}: no package name", cargo_toml.display()))?
        .replace('-', "_");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command.args(["build", "--release", "--locked", "--target", "wasm32-wasip2"]).current_dir(dir);
    // What `cargo run` set for xtask is xtask's, not the extension's build.
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("CARGO_") && k != "CARGO_HOME") {
        command.env_remove(key);
    }
    let status = command
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err(format!("{}: the build failed", dir.display()));
    }
    let built = dir.join("target/wasm32-wasip2/release").join(format!("{crate_name}.wasm"));
    std::fs::copy(&built, dir.join("extension.wasm")).map_err(|e| format!("{}: {e}", built.display()))?;
    // stderr: `pack` keeps stdout for its JSON, which the release reads.
    eprintln!("built {}", dir.join("extension.wasm").display());
    Ok(())
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                // A code extension's build, never shipped.
                if !(d == dir && entry.file_name() == "target" && dir.join("Cargo.toml").is_file()) {
                    stack.push(p);
                }
            } else if let Ok(rel) = p.strip_prefix(dir) {
                out.push(rel.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// An extension as the dependency checks know it: its versions (the one
/// here, or those the index has) and what it depends on.
struct Known {
    versions: Vec<semver::Version>,
    dependencies: BTreeMap<String, String>,
}

/// What the index (`index.json`, from the registry branch) has: each
/// extension and bundle with its versions, and what the newest depends
/// on.
fn indexed(index: &serde_json::Value) -> BTreeMap<String, Known> {
    let mut out = BTreeMap::new();
    let lists = ["extensions", "bundles"].map(|k| index[k].as_array().cloned().unwrap_or_default());
    for listing in lists.iter().flatten() {
        let Some(id) = listing["id"].as_str() else { continue };
        let releases = listing["versions"].as_array().cloned().unwrap_or_default();
        let mut versions: Vec<(semver::Version, &serde_json::Value)> = releases.iter().filter_map(|r| Some((semver::Version::parse(r["version"].as_str()?).ok()?, r))).collect();
        versions.sort_by(|a, b| a.0.cmp(&b.0));
        let dependencies = versions.last().and_then(|(_, r)| r["dependencies"].as_object()).map(|d| d.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
        out.insert(id.to_string(), Known { versions: versions.into_iter().map(|(v, _)| v).collect(), dependencies });
    }
    out
}

/// Every dependency (a bundle's members with them) is an extension here
/// or in the index, in a version its range takes, and nothing depends on
/// itself however far round.
fn check_dependencies(known: &BTreeMap<String, Known>, here: &BTreeSet<String>) -> Result<(), String> {
    for id in here {
        for (dep, range) in &known[id].dependencies {
            let req = semver::VersionReq::parse(range).map_err(|e| format!("{id}: dependency {dep}: {e}"))?;
            let Some(k) = known.get(dep) else {
                return Err(format!("{id} depends on {dep}, which is neither in extensions/ nor in the index"));
            };
            if !k.versions.iter().any(|v| req.matches(v)) {
                let have: Vec<String> = k.versions.iter().map(|v| v.to_string()).collect();
                return Err(format!("{id} depends on {dep} {range}, and there's only {}", have.join(", ")));
            }
        }
    }
    // Depth first: one met again on the way down is a circle.
    fn visit<'a>(id: &'a str, known: &'a BTreeMap<String, Known>, path: &mut Vec<&'a str>, done: &mut BTreeSet<&'a str>) -> Result<(), String> {
        if let Some(at) = path.iter().position(|p| *p == id) {
            let mut circle = path[at..].to_vec();
            circle.push(id);
            return Err(format!("dependencies go round in a circle: {}", circle.join(" -> ")));
        }
        if !done.insert(id) {
            return Ok(());
        }
        path.push(id);
        for dep in known.get(id).map(|k| k.dependencies.keys()).into_iter().flatten() {
            visit(dep, known, path, done)?;
        }
        path.pop();
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in here {
        visit(id, known, &mut Vec::new(), &mut done)?;
    }
    Ok(())
}

fn validate(root: &Path, index: Option<&Path>) -> Result<(), String> {
    let mut known = match index {
        Some(path) => indexed(&serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?),
        None => BTreeMap::new(),
    };
    let mut here = BTreeSet::new();
    let mut ids = BTreeMap::new();
    let dirs = std::fs::read_dir(root.join("extensions")).map_err(|e| format!("extensions/: {e}"))?;
    let mut count = 0;
    for dir in dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let m = manifest(&dir)?;
        if let Some(other) = ids.insert(m.extension.id.clone(), dir.clone()) {
            return Err(format!("{} and {} have the same id {}", other.display(), dir.display(), m.extension.id));
        }
        // A theme is one Fenix reads -- every Fenix the extension is for.
        let floor = fenix_theme::floor(&semver::VersionReq::parse(&m.engine.fenix).map_err(|e| e.to_string())?);
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("themes") && f.extension().is_some_and(|x| x == "toml")) {
            let text = std::fs::read_to_string(dir.join(&rel)).map_err(|e| e.to_string())?;
            fenix_theme::check(&text, Some(&floor)).map_err(|e| format!("{}: {e}", dir.join(&rel).display()))?;
        }
        // A snippet has its header -- a key, then `# --` -- and a body.
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("snippets") && f.extension().is_some_and(|x| x == "snippet")) {
            let text = std::fs::read_to_string(dir.join(&rel)).map_err(|e| e.to_string())?;
            let (header, body) = text.split_once("# --\n").ok_or_else(|| format!("{}: no `# --` line ending the header", dir.join(&rel).display()))?;
            if !header.lines().any(|l| l.strip_prefix("# key:").is_some_and(|k| !k.trim().is_empty())) {
                return Err(format!("{}: a snippet needs a `# key:`", dir.join(&rel).display()));
            }
            if body.trim().is_empty() {
                return Err(format!("{}: the snippet has no body", dir.join(&rel).display()));
            }
        }
        // A screenshot is a picture Fenix reads, and not a huge one.
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("screenshots")) {
            let kind = rel.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !SCREENSHOTS.contains(&kind.as_str()) {
                return Err(format!("{}: a screenshot is one of {}", dir.join(&rel).display(), SCREENSHOTS.join(", ")));
            }
            let size = std::fs::metadata(dir.join(&rel)).map_err(|e| e.to_string())?.len();
            if size > MAX_SCREENSHOT {
                return Err(format!("{}: a screenshot is at most {} MB", dir.join(&rel).display(), MAX_SCREENSHOT / 1024 / 1024));
            }
        }
        check_agent_tools(&dir)?;
        check_chords(&dir)?;
        let version = semver::Version::parse(&m.extension.version).map_err(|e| e.to_string())?;
        let entry = known.entry(m.extension.id.clone()).or_insert(Known { versions: Vec::new(), dependencies: BTreeMap::new() });
        entry.versions.push(version);
        entry.dependencies = m.dependencies.clone();
        here.insert(m.extension.id.clone());
        let kind = if m.extension.runtime == "bundle" { "bundle" } else { "" };
        println!("ok  {} {} ({}){}", m.extension.id, m.extension.version, dir.display(), if kind.is_empty() { String::new() } else { format!(" {kind} of {}", m.dependencies.keys().cloned().collect::<Vec<_>>().join(", ")) });
        count += 1;
    }
    check_dependencies(&known, &here)?;
    screenshots::validate_all(root)?;
    lessons::validate_all(root)?;
    println!("{count} extension(s)");
    Ok(())
}

/// The chords Fenix keeps for itself: standard keys' leader and palette,
/// the terminal's way out, Alt+X.
const RESERVED_CHORDS: &[&str] = &["C-k", "C-S-p", "C-\\", "M-x"];
/// Standard keys' own chords an extension's would hide (a warning, not an
/// error: in a context of its own it may mean it).
const STANDARD_CHORDS: &[&str] = &["C-c", "C-x", "C-v", "C-z", "C-y", "C-s", "C-a", "C-f", "C-h", "C-p", "C-o", "C-w", "C-/", "F12", "F2", "C-.", "C-g", "C-b", "C-j", "C-n", "C-e"];

/// `[[keys]]` in its keymaps (API 1.5): one key with its modifiers, a
/// scheme and a place Fenix knows, a context it has, short of Fenix's own
/// chords and of a terminal -- and an API that reads them.
fn check_chords(dir: &Path) -> Result<(), String> {
    let manifest_text = std::fs::read_to_string(dir.join("extension.toml")).map_err(|e| e.to_string())?;
    let manifest: toml::Table = toml::from_str(&manifest_text).map_err(|e| e.message().to_string())?;
    let contexts: Vec<String> = manifest
        .get("contributes")
        .and_then(|c| c.get("contexts"))
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect())
        .unwrap_or_default();
    // A keymap flavour is there to take standard keys' chords, and has no
    // context to give them: the warning below is for the others.
    let flavour = manifest.get("extension").and_then(|e| e.get("categories")).and_then(|c| c.as_array()).is_some_and(|c| c.iter().any(|c| c.as_str() == Some("keymaps")));
    let api = manifest.get("engine").and_then(|e| e.get("api")).and_then(|a| a.as_str()).map(|a| semver::Version::parse(&match a.matches('.').count() { 0 => format!("{a}.0.0"), 1 => format!("{a}.0"), _ => a.to_string() }).ok());
    for rel in files(dir).into_iter().filter(|f| f.starts_with("keymaps") && f.extension().is_some_and(|x| x == "toml")) {
        let path = dir.join(&rel);
        let at = path.display();
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
        let Some(chords) = table.get("keys").and_then(|k| k.as_array()) else { continue };
        if api.as_ref().is_some_and(|v| v.as_ref().is_none_or(|v| *v < semver::Version::new(1, 5, 0))) {
            return Err(format!("{at}: [[keys]] needs engine.api 1.5 -- an older Fenix can't read the keymap"));
        }
        for chord in chords {
            let field = |key: &str| chord.get(key).and_then(|v| v.as_str());
            let keys = field("keys").unwrap_or_default();
            if keys.is_empty() || keys.split_whitespace().count() != 1 {
                return Err(format!("{at}: [[keys]] {keys:?} -- one key with its modifiers (C-Enter, F5)"));
            }
            if RESERVED_CHORDS.contains(&keys) {
                return Err(format!("{at}: {keys} is Fenix's own"));
            }
            if !matches!(field("scheme").unwrap_or("any"), "vim" | "standard" | "any") {
                return Err(format!("{at}: {keys}: scheme is vim, standard or any"));
            }
            match field("when").unwrap_or("anywhere") {
                "anywhere" | "text" | "typing" | "not_typing" | "page" => {}
                "terminal" => return Err(format!("{at}: {keys}: only Fenix's chords and yours reach past a terminal")),
                other => return Err(format!("{at}: {keys}: when {other:?} -- anywhere, text, typing, not_typing or page")),
            }
            if let Some(context) = field("context") {
                if !contexts.iter().any(|c| c == context) {
                    return Err(format!("{at}: {keys}: context {context:?} isn't one of its [[contributes.contexts]]"));
                }
            }
            if field("command").is_none() && chord.get("unbind").and_then(|u| u.as_bool()) != Some(true) {
                return Err(format!("{at}: {keys}: a command, or unbind = true"));
            }
            if !flavour && STANDARD_CHORDS.contains(&keys) && field("context").is_none() {
                println!("warn {at}: {keys} is one of standard keys' own, and this takes it everywhere -- give it a context");
            }
        }
    }
    Ok(())
}

/// The classes an agent tool may say it is.
const TOOL_CLASSES: &[&str] = &["read", "show", "edit", "run", "external"];
/// What agents are given of a tool: its description, in characters, and
/// its input schema, in bytes (Fenix's `MAX_TOOL_DESCRIPTION` and
/// `MAX_TOOL_SCHEMA`).
const MAX_TOOL_DESCRIPTION: usize = 300;
const MAX_TOOL_SCHEMA: usize = 2048;
const MAX_TOOL_TIMEOUT_MS: i64 = 600_000;

/// `[[contributes.agent_tools]]`, held to Fenix 1.2's rules whatever API
/// the extension is built for: a name agents can call, one of its
/// services, a class its permissions fit, and sizes that fit what agents
/// are given.
fn check_agent_tools(dir: &Path) -> Result<(), String> {
    let path = dir.join("extension.toml");
    let at = path.display();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{at}: {e}"))?;
    let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
    let contributes = table.get("contributes").and_then(|c| c.as_table());
    let Some(tools) = contributes.and_then(|c| c.get("agent_tools")).and_then(|t| t.as_array()) else { return Ok(()) };
    let services: Vec<&str> = contributes.and_then(|c| c.get("services")).and_then(|s| s.as_array()).map(|s| s.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let permissions = table.get("permissions").and_then(|p| p.as_table());
    let permission = |key: &str| permissions.and_then(|p| p.get(key));
    let listed = |key: &str| permission(key).and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty());
    let mut names = BTreeSet::new();
    for tool in tools {
        let field = |key: &str| tool.get(key).and_then(|v| v.as_str());
        let name = field("name").unwrap_or_default();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(format!("{at}: agent tool {name:?} -- lower case letters, digits and _"));
        }
        if !names.insert(name.to_string()) {
            return Err(format!("{at}: agent tool {name:?} is there twice"));
        }
        let service = field("service").unwrap_or_default();
        if !services.contains(&service) {
            return Err(format!("{at}: agent tool {name:?} -- {service:?} isn't one of its services"));
        }
        let description = field("description").unwrap_or_default();
        if description.trim().is_empty() {
            return Err(format!("{at}: agent tool {name:?} has no description: it's how an agent decides to call it"));
        }
        if description.chars().count() > MAX_TOOL_DESCRIPTION {
            return Err(format!("{at}: agent tool {name:?} -- its description is {} characters, and agents are given at most {MAX_TOOL_DESCRIPTION}", description.chars().count()));
        }
        if let Some(input) = field("input") {
            if !serde_json::from_str::<serde_json::Value>(input).is_ok_and(|v| v.is_object()) {
                return Err(format!("{at}: agent tool {name:?} -- input isn't a JSON object"));
            }
            if input.len() > MAX_TOOL_SCHEMA {
                return Err(format!("{at}: agent tool {name:?} -- its input schema is {} bytes, and agents are given at most {MAX_TOOL_SCHEMA}", input.len()));
            }
        }
        if let Some(ms) = tool.get("timeout_ms").and_then(|v| v.as_integer()).filter(|ms| !(1..=MAX_TOOL_TIMEOUT_MS).contains(ms)) {
            return Err(format!("{at}: agent tool {name:?} -- timeout_ms is {ms}: from 1 to {MAX_TOOL_TIMEOUT_MS} (10 minutes)"));
        }
        let writes = tool.get("writes").and_then(|v| v.as_bool()).unwrap_or(false);
        let class = match field("class") {
            Some(class) if TOOL_CLASSES.contains(&class) => class,
            Some(class) => return Err(format!("{at}: agent tool {name:?} -- class {class:?}: one of {}", TOOL_CLASSES.join(", "))),
            None => return Err(format!("{at}: agent tool {name:?} has no class -- one of {}, so agents' rules can tell what it does", TOOL_CLASSES.join(", "))),
        };
        match class {
            "external" if !listed("network") && !listed("sockets") => return Err(format!("{at}: agent tool {name:?} is external -- it needs network or sockets in its permissions")),
            "edit" if permission("editor").and_then(|v| v.as_str()) != Some("write") && permission("filesystem").and_then(|v| v.as_str()) != Some("project-write") => {
                return Err(format!("{at}: agent tool {name:?} edits -- it needs editor = \"write\" or filesystem = \"project-write\" in its permissions"));
            }
            "read" | "show" if writes => return Err(format!("{at}: agent tool {name:?} says writes = true and class = {class:?}: one of them is wrong")),
            _ => {}
        }
    }
    Ok(())
}

fn pack(dir: &Path, dist: &Path) -> Result<(), String> {
    build(dir)?;
    let m = manifest(dir)?;
    let code = dir.join("Cargo.toml").is_file();
    std::fs::create_dir_all(dist).map_err(|e| e.to_string())?;
    let name = format!("{}-{}.fenixext", m.extension.id, m.extension.version);
    let out = dist.join(&name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out).map_err(|e| e.to_string())?);
    for rel in files(dir).into_iter().filter(|rel| !(code && source_only(rel)) && !screenshots::is_script(rel)) {
        let entry = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        zip.start_file(entry, zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        zip.write_all(&std::fs::read(dir.join(&rel)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    println!("{}", serde_json::json!({ "id": m.extension.id, "version": m.extension.version, "file": out, "sha256": sha, "size": bytes.len() }));
    Ok(())
}

fn for_tag(root: &Path, tag: &str) -> Result<(), String> {
    let (name, version) = tag.rsplit_once("-v").ok_or_else(|| format!("{tag}: a release tag is <name>-v<version>"))?;
    let dir = root.join("extensions").join(name);
    let m = manifest(&dir)?;
    if m.extension.version != version {
        return Err(format!("{tag}: extensions/{name} is version {}", m.extension.version));
    }
    println!("{}", dir.display());
    Ok(())
}

fn index_add(index: &Path, dir: &Path, url: &str, sha256: &str, size: u64) -> Result<(), String> {
    index_write(index, &manifest(dir)?, url, sha256, size)
}

/// A release in the index, from its manifest: one read here
/// (`index_add`), or from inside a package built elsewhere (`external`).
fn index_write(index: &Path, m: &Manifest, url: &str, sha256: &str, size: u64) -> Result<(), String> {
    let mut doc: serde_json::Value = match std::fs::read_to_string(index) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))?,
        Err(_) => serde_json::json!({ "format": 1, "extensions": [] }),
    };
    // A bundle is listed apart: a Fenix that doesn't know bundles reads
    // past `bundles`, as it reads past any key it doesn't know.
    let bundle = m.extension.runtime == "bundle";
    let key = if bundle { "bundles" } else { "extensions" };
    if doc[key].is_null() {
        doc[key] = serde_json::json!([]);
    }
    let mut release = serde_json::json!({
        "version": m.extension.version,
        "fenix": m.engine.fenix,
        "runtime": m.extension.runtime,
        "url": url,
        "sha256": sha256,
        "size": size,
        "dependencies": m.dependencies,
        "categories": m.extension.categories,
    });
    if bundle {
        release["bundle"] = serde_json::json!(m.dependencies.keys().collect::<Vec<_>>());
    }
    if let Some(api) = &m.engine.api {
        release["api"] = api.clone().into();
    }
    if let Some(permissions) = &m.permissions {
        release["permissions"] = serde_json::to_value(permissions).map_err(|e| e.to_string())?;
    }
    let list = doc[key].as_array_mut().ok_or(format!("the index's {key} isn't a list"))?;
    let at = match list.iter().position(|e| e["id"] == m.extension.id.as_str()) {
        Some(i) => i,
        None => {
            list.push(serde_json::json!({ "id": m.extension.id, "versions": [] }));
            list.len() - 1
        }
    };
    let listing = &mut list[at];
    listing["name"] = m.extension.name.clone().into();
    listing["description"] = m.extension.description.clone().into();
    listing["categories"] = serde_json::json!(m.extension.categories);
    listing["repository"] = serde_json::json!(m.extension.repository);
    let versions = listing["versions"].as_array_mut().ok_or("versions isn't a list")?;
    versions.retain(|v| v["version"] != m.extension.version.as_str());
    versions.push(release);
    versions.sort_by_key(|v| semver::Version::parse(v["version"].as_str().unwrap_or("0.0.0")).unwrap_or(semver::Version::new(0, 0, 0)));
    list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    std::fs::write(index, serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    println!("{} {} in {}", m.extension.id, m.extension.version, index.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("the repository").to_path_buf();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("validate") => validate(&root, args.get(1).map(Path::new)),
        Some("build") => build(Path::new(&arg(1))),
        Some("pack") => pack(Path::new(&arg(1)), Path::new(args.get(2).map(String::as_str).unwrap_or("dist"))),
        Some("for-tag") => for_tag(&root, &arg(1)),
        Some("external-check") => external::check_command(&root, &args[1..]),
        Some("index-add-external") => external::add_command(&root, &args[1..]),
        Some("screenshots") => screenshots::run(&root, &args[1..]),
        Some("site") => site::site(&root, Path::new(&arg(1)), Path::new(&arg(2))),
        Some("new") => new::run(&root, &args[1..]),
        Some("install") => install::run(&args[1..]),
        Some("theme-import") => theme_import::run(&args[1..]),
        Some("snippets-import") => snippets_import::run(&args[1..]),
        Some("index-add") => arg(5).parse::<u64>().map_err(|e| format!("size: {e}")).and_then(|size| index_add(Path::new(&arg(1)), Path::new(&arg(2)), &arg(3), &arg(4), size)),
        _ => Err("cargo xtask validate [index.json] | build <dir> | pack <dir> [dist] | for-tag <tag> | index-add <index.json> <dir> <url> <sha256> <size> | external-check [--offline] [--index <index.json>] | index-add-external <index.json> [--only <id>@<version>] | screenshots [name] [--fenix <path>] [--theme <name>] [--size WxH] [--dry-run] | site <index.json> <out> | new <kind> <name | publisher.name> [--name \"Display name\"] | install <extension-dir> [--fenix <path>] [--watch] | theme-import <theme.json> [--out <dir>] [--name \"...\"] [--fenix <version>] [--source \"...\"] | snippets-import <snippets.json> --language <id> [--into <extension-dir>] [--approximate] [--indent <n>]".into()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temp folder, gone when
    /// dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `extensions/<name>` under `root`, with this manifest's `[extension]`
    /// lines and what follows `[engine]`.
    fn extension(root: &Path, name: &str, version: &str, runtime: &str, rest: &str) -> PathBuf {
        let dir = root.join("extensions").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extension.toml"), format!("[extension]\nid = \"acme.{name}\"\nname = \"{name}\"\nversion = \"{version}\"\nruntime = \"{runtime}\"\n\n[engine]\nfenix = \">=2.0, <3\"\n{rest}")).unwrap();
        dir
    }

    #[test]
    fn chords_are_checked() {
        let t = Temp::new("chords");
        let keymap = |dir: &Path, text: &str| {
            std::fs::create_dir_all(dir.join("keymaps")).unwrap();
            std::fs::write(dir.join("keymaps/k.toml"), text).unwrap();
        };
        let ctx = "\n[engine]\napi = \"1.5\"\n\n[[contributes.contexts]]\nname = \"sql\"\nlanguages = [\"sql\"]\n";
        let dir = t.0.join("x");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extension.toml"), format!("[extension]\nid = \"acme.x\"{ctx}")).unwrap();
        keymap(&dir, "[[keys]]\nkeys = \"C-Enter\"\ncommand = \"x.run\"\ncontext = \"sql\"\nwhen = \"text\"\nscheme = \"standard\"\n");
        assert_eq!(check_chords(&dir), Ok(()));
        for (bad, says) in [("keys = \"C-k\"\ncommand = \"x.run\"", "Fenix's own"), ("keys = \"C-x C-s\"\ncommand = \"x.run\"", "one key"), ("keys = \"F5\"\ncommand = \"x.run\"\nwhen = \"terminal\"", "past a terminal"), ("keys = \"F5\"\ncommand = \"x.run\"\ncontext = \"tcl\"", "isn't one of"), ("keys = \"F5\"", "a command")] {
            keymap(&dir, &format!("[[keys]]\n{bad}\n"));
            let err = check_chords(&dir).unwrap_err();
            assert!(err.contains(says), "{bad}: {err}");
        }
        std::fs::write(dir.join("extension.toml"), "[extension]\nid = \"acme.x\"\n[engine]\napi = \"1.4\"\n").unwrap();
        keymap(&dir, "[[keys]]\nkeys = \"F5\"\ncommand = \"x.run\"\n");
        assert!(check_chords(&dir).unwrap_err().contains("1.5"));
    }

    #[test]
    fn a_bundle_is_its_members_and_nothing_else() {
        let t = Temp::new("bundle");
        extension(&t.0, "notes", "2.0.1", "none", "");
        let bundle = extension(&t.0, "personal", "1.0.0", "bundle", "\n[dependencies]\n\"acme.notes\" = \"^2\"\n");
        std::fs::write(bundle.join("README.md"), "# Personal").unwrap();
        std::fs::create_dir_all(bundle.join("screenshots")).unwrap();
        std::fs::write(bundle.join("screenshots/board.png"), "png").unwrap();
        validate(&t.0, None).unwrap();
        let m = manifest(&bundle).unwrap();
        assert_eq!(m.extension.runtime, "bundle");

        std::fs::write(bundle.join("screenshots/board.bmp"), "bmp").unwrap();
        assert!(validate(&t.0, None).unwrap_err().contains("a screenshot is one of"));
        std::fs::remove_file(bundle.join("screenshots/board.bmp")).unwrap();
        std::fs::create_dir_all(bundle.join("themes")).unwrap();
        std::fs::write(bundle.join("themes/x.toml"), "name = \"x\"").unwrap();
        assert!(manifest(&bundle).unwrap_err().contains("a bundle carries only its manifest"));
        std::fs::remove_dir_all(bundle.join("themes")).unwrap();

        let empty = extension(&t.0, "empty", "1.0.0", "bundle", "");
        assert!(manifest(&empty).unwrap_err().contains("has none"));
        let code = extension(&t.0, "empty", "1.0.0", "bundle", "api = \"0.8\"\n\n[dependencies]\n\"acme.notes\" = \"^2\"\n");
        assert!(manifest(&code).unwrap_err().contains("runs nothing"));
    }

    #[test]
    fn agent_tools_have_a_class_their_permissions_fit_and_sizes_agents_take() {
        let t = Temp::new("tools");
        let with = |tool: &str, permissions: &str| {
            let rest = format!("api = \"1.2\"\n\n[permissions]\n{permissions}\n\n[contributes]\nservices = [\"jira.issue\"]\n\n[[contributes.agent_tools]]\nname = \"issue\"\nservice = \"jira.issue\"\n{tool}\n");
            let dir = extension(&t.0, "jira", "1.0.0", "wasm", &rest);
            check_agent_tools(&dir)
        };
        with("description = \"An issue\"\nclass = \"read\"", "").unwrap();
        with("description = \"Moves it\"\nclass = \"external\"\ntimeout_ms = 120000", "network = [\"example.atlassian.net\"]").unwrap();
        assert!(with("description = \"An issue\"", "").unwrap_err().contains("has no class"));
        assert!(with("description = \"An issue\"\nclass = \"web\"", "").unwrap_err().contains("one of read, show, edit, run, external"));
        assert!(with("description = \"Moves it\"\nclass = \"external\"", "").unwrap_err().contains("needs network or sockets"));
        assert!(with("description = \"Edits\"\nclass = \"edit\"", "editor = \"read\"").unwrap_err().contains("editor = \"write\""));
        assert!(with("description = \"An issue\"\nclass = \"read\"\nwrites = true", "").unwrap_err().contains("one of them is wrong"));
        assert!(with(&format!("description = \"{}\"\nclass = \"read\"", "x".repeat(301)), "").unwrap_err().contains("301 characters"));
        let schema = format!("{{\"type\":\"object\",\"description\":\"{}\"}}", "x".repeat(2100));
        assert!(with(&format!("description = \"An issue\"\nclass = \"read\"\ninput = '{schema}'"), "").unwrap_err().contains("at most 2048"));
        assert!(with("description = \"An issue\"\nclass = \"read\"\ntimeout_ms = 0", "").unwrap_err().contains("timeout_ms is 0"));
        assert!(with("description = \"An issue\"\nclass = \"read\"\nservice = \"jira.other\"", "").is_err(), "a service it hasn't");
    }

    #[test]
    fn a_category_is_one_of_the_pages() {
        let t = Temp::new("category");
        let dir = extension(&t.0, "dusk", "1.0.0", "none", "");
        let with = |categories: &str| {
            let text = std::fs::read_to_string(dir.join("extension.toml")).unwrap().replace("runtime = ", &format!("categories = [{categories}]\nruntime = "));
            std::fs::write(dir.join("extension.toml"), text).unwrap();
            manifest(&dir)
        };
        assert!(with("\"themes\", \"fun\"").is_ok());
        extension(&t.0, "dusk", "1.0.0", "none", "");
        assert!(with("\"colours\"").unwrap_err().contains("category \"colours\" -- one of languages"));
    }

    /// An id, its versions, and what it depends on.
    type Entry<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

    fn known(list: &[Entry]) -> BTreeMap<String, Known> {
        list.iter()
            .map(|(id, versions, deps)| {
                let versions = versions.iter().map(|v| semver::Version::parse(v).unwrap()).collect();
                (id.to_string(), Known { versions, dependencies: deps.iter().map(|(d, r)| (d.to_string(), r.to_string())).collect() })
            })
            .collect()
    }

    #[test]
    fn members_are_there_in_a_version_that_fits_and_never_in_a_circle() {
        let here: BTreeSet<String> = ["acme.pack".to_string()].into();
        let fine = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2"), ("acme.b", "^1")]), ("acme.a", &["2.1.0"], &[]), ("acme.b", &["1.0.0", "1.3.0"], &[])]);
        check_dependencies(&fine, &here).unwrap();
        let missing = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2")])]);
        assert!(check_dependencies(&missing, &here).unwrap_err().contains("neither in extensions/ nor in the index"));
        let old = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2")]), ("acme.a", &["1.0.0", "1.4.0"], &[])]);
        assert!(check_dependencies(&old, &here).unwrap_err().contains("acme.a ^2, and there's only 1.0.0, 1.4.0"));
        let round = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "*")]), ("acme.a", &["1.0.0"], &[("acme.b", "*")]), ("acme.b", &["1.0.0"], &[("acme.pack", "*")])]);
        assert_eq!(check_dependencies(&round, &here).unwrap_err(), "dependencies go round in a circle: acme.pack -> acme.a -> acme.b -> acme.pack");
        let shared = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "*"), ("acme.b", "*")]), ("acme.a", &["1.0.0"], &[("acme.b", "*")]), ("acme.b", &["1.0.0"], &[])]);
        check_dependencies(&shared, &here).unwrap();
    }

    /// A member only the registry has is found there, from the index.
    #[test]
    fn the_index_has_what_extensions_left() {
        let index = serde_json::json!({ "format": 1,
            "extensions": [{ "id": "acme.a", "name": "A", "versions": [{ "version": "2.0.0" }, { "version": "2.2.0", "dependencies": { "acme.b": "^1" } }] }],
            "bundles": [{ "id": "acme.old", "name": "Old", "versions": [{ "version": "1.0.0", "dependencies": { "acme.a": "^2" } }] }] });
        let known = indexed(&index);
        assert_eq!(known["acme.a"].versions.len(), 2);
        assert_eq!(known["acme.a"].dependencies["acme.b"], "^1", "the newest's");
        assert!(known.contains_key("acme.old"));
    }

    /// Every theme in the store has a name of its own -- none of Fenix's
    /// (it would take that one's place), none another extension's -- and
    /// the theme packs made from VS Code answer to the names VS Code gives
    /// them (their `package.json`), as Fenix's settings import matches
    /// `workbench.colorTheme` (`fenix_theme::import_match`).
    #[test]
    fn the_store_s_themes_answer_to_their_vs_code_names() {
        const VS_CODE: &[(&str, &str)] = &[
            ("Vitesse Light", "Vitesse Light"),
            ("Vitesse Light Soft", "Vitesse Light Soft"),
            ("Vitesse Dark", "Vitesse Dark"),
            ("Vitesse Dark Soft", "Vitesse Dark Soft"),
            ("Vitesse Black", "Vitesse Black"),
            ("Min Light", "Min Light"),
            ("Min Dark", "Min Dark"),
            ("Winter is Coming (Dark Blue)", "Winter is Coming (Dark Blue)"),
            ("Winter is Coming (Dark Black)", "Winter is Coming (Dark Black)"),
            ("Winter is Coming (Light)", "Winter is Coming (Light)"),
        ];
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut names: Vec<String> = Vec::new();
        for dir in std::fs::read_dir(root.join("extensions")).unwrap().flatten().map(|e| e.path()) {
            for rel in files(&dir).into_iter().filter(|f| f.starts_with("themes") && f.extension().is_some_and(|x| x == "toml")) {
                let table: toml::Table = toml::from_str(&std::fs::read_to_string(dir.join(&rel)).unwrap()).unwrap();
                names.push(table["name"].as_str().unwrap().to_string());
            }
        }
        for name in &names {
            assert!(!fenix_theme::BUILT_IN_NAMES.iter().any(|b| fenix_theme::norm(b) == fenix_theme::norm(name)), "{name} is one of Fenix's own");
            assert_eq!(names.iter().filter(|n| fenix_theme::norm(n) == fenix_theme::norm(name)).count(), 1, "{name} is there twice");
        }
        let catalog: Vec<&str> = fenix_theme::BUILT_IN_NAMES.iter().copied().chain(names.iter().map(String::as_str)).collect();
        for (vs_code, ours) in VS_CODE {
            assert_eq!(fenix_theme::import_match(vs_code, &catalog), Some((ours.to_string(), true)), "workbench.colorTheme {vs_code:?}");
        }
        // VS Code's italic-free twins come to the theme their name begins with.
        assert_eq!(fenix_theme::import_match("Winter is Coming (Dark Blue - No Italics)", &catalog), Some(("Winter is Coming (Dark Blue)".into(), false)));
    }

    #[test]
    fn a_bundle_goes_in_the_index_apart_with_its_members() {
        let t = Temp::new("index");
        let theme = extension(&t.0, "dusk", "1.0.0", "none", "");
        let text = std::fs::read_to_string(theme.join("extension.toml")).unwrap().replace("runtime = ", "categories = [\"themes\"]\nruntime = ");
        std::fs::write(theme.join("extension.toml"), text).unwrap();
        let bundle = extension(&t.0, "evening", "1.0.0", "bundle", "\n[dependencies]\n\"acme.dusk\" = \"^1\"\n");
        let index = t.0.join("index.json");
        index_add(&index, &theme, "u1", "aa", 1).unwrap();
        index_add(&index, &bundle, "u2", "bb", 2).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
        assert_eq!(doc["extensions"].as_array().unwrap().len(), 1, "an index an older Fenix reads as it did");
        assert_eq!(doc["extensions"][0]["versions"][0]["categories"], serde_json::json!(["themes"]));
        assert_eq!(doc["bundles"][0]["id"], "acme.evening");
        let release = &doc["bundles"][0]["versions"][0];
        assert_eq!((release["runtime"].as_str(), release["bundle"].clone()), (Some("bundle"), serde_json::json!(["acme.dusk"])));
        assert_eq!(release["dependencies"]["acme.dusk"], "^1");
    }
}
