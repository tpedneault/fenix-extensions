//! Context for coding agents, as tools they call through Fenix's MCP
//! server (`agent_context_*`):
//!
//! - `dependencies`: the project's direct dependencies per ecosystem, with
//!   the versions its lockfiles hold (`deps`).
//! - `dependency_docs`: a dependency's documentation for exactly the
//!   version locked -- its README, or an item's page -- from the
//!   ecosystem's registry (`registry`), made readable and cut to a size
//!   (`text`), and kept under `/data/cache`: a version's documentation
//!   doesn't change.
//!
//! Both are MCP resources too, `fenix://ext/agent_context/dependencies`
//! and `fenix://ext/agent_context/docs/{package}`.

pub mod deps;
pub mod registry;
pub mod text;

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use fenix_extension_api::{self as api, export, run, Event, Extension, HttpOptions, HttpRequest};
use serde::{Deserialize, Serialize};

use deps::Ecosystem;
use registry::Docs;

/// The project, as the extension is given it.
const PROJECT: &str = "/project";
/// Documentation fetched, by ecosystem, package, version and item.
const CACHE: &str = "/data/cache";
/// What it tells the registries it is (crates.io turns away a request
/// that doesn't say).
const USER_AGENT: &str = "fenix-agent-context/1.0 (https://github.com/tpedneault/fenix-extensions)";
/// The most text an answer has, unless `agent-context.max_chars` or the
/// call says.
const MAX_CHARS: usize = 20_000;

#[derive(Default)]
pub struct AgentContext;

/// A call's arguments: a tool's, or a resource's (`{"params": ...}` too).
fn arguments(payload: &str) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    match value.get("params") {
        Some(p) if p.is_object() => p.clone(),
        _ => value,
    }
}

/// `%xx` undone: a package's name in a resource's URI (`@scope%2Fname`).
fn percent_decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Default, Deserialize)]
struct DepsArgs {
    #[serde(default)]
    ecosystem: Option<String>,
    /// Dev, build, optional and peer dependencies too (the default).
    #[serde(default)]
    include_dev: Option<bool>,
}

/// `dependencies`: what the project at `root` depends on.
pub fn dependencies(root: &Path, payload: &str) -> Result<String, String> {
    let args: DepsArgs = serde_json::from_value(arguments(payload)).map_err(|e| format!("the arguments: {e}"))?;
    let ecosystem = match args.ecosystem.as_deref().filter(|e| !e.trim().is_empty()) {
        Some(e) => Some(Ecosystem::parse(e).ok_or_else(|| format!("{e}: one of cargo, npm, python, go"))?),
        None => None,
    };
    if !root.is_dir() {
        return Err("no project is open".into());
    }
    let mut found: Vec<deps::Found> = deps::scan(root).into_iter().filter(|f| ecosystem.is_none_or(|e| e == f.ecosystem)).collect();
    if args.include_dev == Some(false) {
        for f in &mut found {
            f.dependencies.retain(|d| d.kinds.iter().any(|k| k == "normal"));
        }
    }
    if found.is_empty() {
        return Err(format!(
            "no {}manifest in the project's top folder or the folders under it: Cargo.toml, package.json, pyproject.toml, requirements*.txt, go.mod",
            ecosystem.map(|e| format!("{} ", e.name())).unwrap_or_default()
        ));
    }
    serde_json::to_string(&serde_json::json!({ "projects": found })).map_err(|e| e.to_string())
}

#[derive(Debug, Default, Deserialize)]
struct DocsArgs {
    #[serde(default, alias = "name")]
    package: String,
    #[serde(default)]
    item: Option<String>,
    #[serde(default)]
    ecosystem: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
}

/// A `dependency_docs` call, its package found in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub ecosystem: Ecosystem,
    pub package: String,
    pub version: String,
    pub item: Option<String>,
    /// Where the version comes from: a lockfile, or the call.
    pub locked_in: Option<String>,
    pub requirement: Option<String>,
    pub max_chars: usize,
}

/// What a `dependency_docs` call asks for, its version from the lockfile
/// -- a direct dependency's, else one a dependency pulls in -- unless the
/// call gives one.
pub fn docs_request(root: &Path, payload: &str, max_default: usize) -> Result<Request, String> {
    let args: DocsArgs = serde_json::from_value(arguments(payload)).map_err(|e| format!("the arguments: {e}"))?;
    let package = percent_decode(args.package.trim());
    if package.is_empty() {
        return Err("which package? {\"package\": \"serde\"}".into());
    }
    let ecosystem = match args.ecosystem.as_deref().filter(|e| !e.trim().is_empty()) {
        Some(e) => Some(Ecosystem::parse(e).ok_or_else(|| format!("{e}: one of cargo, npm, python, go"))?),
        None => None,
    };
    let item = args.item.map(|i| i.trim().to_string()).filter(|i| !i.is_empty());
    let version_given = args.version.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let max_chars = args.max_chars.unwrap_or(max_default).clamp(1_000, 200_000);
    let found = if root.is_dir() { deps::scan(root) } else { Vec::new() };
    let request = |ecosystem, package: String, version, locked_in, requirement| Request { ecosystem, package, version, item: item.clone(), locked_in, requirement, max_chars };
    if let Some((f, d)) = deps::find(&found, &package, ecosystem) {
        // The lockfile, unless the call asks for another version.
        let locked_in = d.version.as_ref().filter(|v| version_given.as_ref().is_none_or(|g| g == *v)).and(f.lockfile.clone());
        return match version_given.or_else(|| d.version.clone()) {
            Some(v) => Ok(request(f.ecosystem, d.name.clone(), v, locked_in, Some(d.requirement.clone()))),
            None => Err(format!("{package} ({}) isn't locked -- {} -- so there's no exact version: give one as \"version\"", d.requirement, f.notes.first().cloned().unwrap_or_else(|| "no lockfile has it".into()))),
        };
    }
    if let Some((eco, v, lockfile)) = deps::locked(root, &found, &package, ecosystem) {
        let version = version_given.unwrap_or(v);
        return Ok(request(eco, package, version, Some(lockfile), None));
    }
    match (ecosystem, version_given) {
        (Some(e), Some(v)) => Ok(request(e, package, v, None, None)),
        _ => {
            let have: Vec<&str> = found.iter().map(|f| f.ecosystem.name()).collect();
            let have = if have.is_empty() { "no dependencies it can read".to_string() } else { format!("its {}", have.join(", ")) };
            Err(format!("{package} isn't a dependency of this project, nor locked by it ({have}); dependencies lists them -- or give \"ecosystem\" and \"version\" to read another package's"))
        }
    }
}

/// A file's name for a cache entry: what's not a letter, digit or
/// `.-_@` as `_`.
fn safe(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@') { c } else { '_' }).collect()
}

impl Request {
    /// Where its documentation is kept once fetched.
    pub fn cache_file(&self) -> String {
        let item = self.item.as_deref().map(|i| format!("#{}", safe(i))).unwrap_or_default();
        format!("{CACHE}/{}/{}@{}{item}.json", self.ecosystem.name(), safe(&self.package), safe(&self.version))
    }
}

/// What's kept of documentation fetched.
#[derive(Debug, Serialize, Deserialize)]
struct Kept {
    source: String,
    text: String,
    about_item: bool,
    note: Option<String>,
}

/// The answer an agent gets.
#[derive(Debug, Serialize)]
struct Answer<'a> {
    package: &'a str,
    ecosystem: Ecosystem,
    version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    locked_in: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requirement: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<&'a str>,
    source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    cached: bool,
    /// It's cut: `max_chars` (or `item`) gets the rest.
    truncated: bool,
    text: String,
}

/// The answer to `req` from documentation `docs`: about its item when it
/// names one, cut to its size.
pub fn answer_text(req: &Request, docs: &Docs, cached: bool) -> String {
    let mut note = docs.note.clone();
    let mut body = docs.text.clone();
    if let (Some(item), false) = (&req.item, docs.about_item) {
        match text::focus(&body, item) {
            Some(part) => body = part,
            None => note = Some(note.map_or(String::new(), |n| format!("{n}; ")) + &format!("nothing in it names {item}: all of it")),
        }
    }
    let (text, truncated) = text::cut(&body, req.max_chars);
    let answer = Answer {
        package: &req.package,
        ecosystem: req.ecosystem,
        version: &req.version,
        locked_in: req.locked_in.as_deref(),
        requirement: req.requirement.as_deref().filter(|r| !r.is_empty()),
        item: req.item.as_deref(),
        source: &docs.source,
        note,
        cached,
        truncated,
        text,
    };
    serde_json::to_string(&answer).unwrap_or_default()
}

fn read_cache(req: &Request) -> Option<Docs> {
    let kept: Kept = serde_json::from_str(&std::fs::read_to_string(req.cache_file()).ok()?).ok()?;
    Some(Docs { source: kept.source, text: kept.text, about_item: kept.about_item, note: kept.note })
}

fn write_cache(req: &Request, docs: &Docs) {
    let file = req.cache_file();
    if let Some(dir) = Path::new(&file).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let kept = Kept { source: docs.source.clone(), text: docs.text.clone(), about_item: docs.about_item, note: docs.note.clone() };
    if let Ok(json) = serde_json::to_string(&kept) {
        let _ = std::fs::write(file, json);
    }
}

/// A URL fetched through Fenix: its status and body.
fn fenix_get(url: String) -> Pin<Box<dyn Future<Output = registry::Body>>> {
    Box::pin(async move {
        let req = HttpRequest::get(&url).header("User-Agent", USER_AGENT).options(HttpOptions::new().timeout(20_000));
        run::fetch(req).await.map(|r| (r.status, r.text()))
    })
}

fn max_chars() -> usize {
    api::setting("agent-context.max_chars").and_then(|s| s.trim().parse().ok()).filter(|n| *n > 0).unwrap_or(MAX_CHARS)
}

impl Extension for AgentContext {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "agent-context.clear_cache" => {
                let _ = std::fs::remove_dir_all(CACHE);
                api::notify("Agent context: the documentation kept is cleared; it's fetched again when asked for");
                Ok(())
            }
            other => Err(format!("no command {other}")),
        }
    }

    fn handle(&mut self, event: Event) {
        // An HTTP answer a job waits for.
        let _ = run::deliver(event);
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        match name {
            "agent-context.dependencies" => Some(dependencies(Path::new(PROJECT), payload)),
            "agent-context.docs" => {
                let req = match docs_request(Path::new(PROJECT), payload, max_chars()) {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
                if let Some(docs) = read_cache(&req) {
                    return Some(Ok(answer_text(&req, &docs, true)));
                }
                run::spawn(async move {
                    api::progress(call, &format!("reading {} {}'s documentation", req.package, req.version), Some(0.1));
                    let get = fenix_get;
                    match registry::fetch(&get, req.ecosystem, &req.package, &req.version, req.item.as_deref()).await {
                        Ok(docs) => {
                            write_cache(&req, &docs);
                            api::answer(call, Ok(answer_text(&req, &docs, false)));
                        }
                        Err(e) => api::answer(call, Err(e)),
                    }
                });
                None
            }
            other => Some(Err(format!("no service {other}"))),
        }
    }
}

export!(AgentContext);

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    const LOCK: &str = "version = 3\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.195\"\n\n[[package]]\nname = \"serde_derive\"\nversion = \"1.0.195\"\n";

    #[test]
    fn the_dependencies_tool_answers_each_ecosystem() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\ninsta = \"1\"\n"), ("Cargo.lock", LOCK), ("go.mod", "module m\n\nrequire golang.org/x/sync v0.6.0\n")]);
        let answer: serde_json::Value = serde_json::from_str(&dependencies(dir.path(), "{}").unwrap()).unwrap();
        let projects = answer["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0]["ecosystem"], "cargo");
        assert_eq!(projects[0]["lockfile"], "Cargo.lock");
        assert_eq!(projects[0]["dependencies"][0], serde_json::json!({"name": "serde", "requirement": "1", "version": "1.0.195", "kinds": ["normal"]}));
        assert_eq!(projects[0]["dependencies"][1]["version"], serde_json::Value::Null, "insta isn't locked");
        let only: serde_json::Value = serde_json::from_str(&dependencies(dir.path(), r#"{"ecosystem": "golang", "include_dev": false}"#).unwrap()).unwrap();
        assert_eq!(only["projects"].as_array().unwrap().len(), 1);
        let normal: serde_json::Value = serde_json::from_str(&dependencies(dir.path(), r#"{"ecosystem": "rust", "include_dev": false}"#).unwrap()).unwrap();
        assert_eq!(normal["projects"][0]["dependencies"].as_array().unwrap().len(), 1, "insta is dev");
        assert!(dependencies(dir.path(), r#"{"ecosystem": "npm"}"#).unwrap_err().contains("no npm manifest"));
        assert!(dependencies(dir.path(), r#"{"ecosystem": "maven"}"#).unwrap_err().contains("one of cargo"));
    }

    #[test]
    fn a_docs_call_takes_the_locked_version() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\nloose = \"0.1\"\n"), ("Cargo.lock", LOCK)]);
        let req = docs_request(dir.path(), r#"{"package": "serde", "item": "de::Deserialize"}"#, 5000).unwrap();
        assert_eq!((req.ecosystem, req.version.as_str(), req.locked_in.as_deref(), req.item.as_deref(), req.max_chars), (Ecosystem::Cargo, "1.0.195", Some("Cargo.lock"), Some("de::Deserialize"), 5000));
        assert_eq!(req.cache_file(), "/data/cache/cargo/serde@1.0.195#de__Deserialize.json");
        let pulled_in = docs_request(dir.path(), r#"{"params": {"package": "serde_derive"}}"#, 5000).unwrap();
        assert_eq!((pulled_in.version.as_str(), pulled_in.requirement), ("1.0.195", None), "not direct, but locked: a resource's arguments");
        let given = docs_request(dir.path(), r#"{"package": "serde", "version": "1.0.100", "max_chars": 10}"#, 5000).unwrap();
        assert_eq!((given.version.as_str(), given.max_chars, given.locked_in), ("1.0.100", 1000, None), "the call's version, not the lockfile's; a size no smaller than 1000");
        assert!(docs_request(dir.path(), r#"{"package": "loose"}"#, 5000).unwrap_err().contains("isn't locked"));
        assert!(docs_request(dir.path(), r#"{"package": "tokio"}"#, 5000).unwrap_err().contains("isn't a dependency of this project"));
        let other = docs_request(dir.path(), r#"{"package": "%40types%2Fnode", "ecosystem": "npm", "version": "20.10.0"}"#, 5000).unwrap();
        assert_eq!((other.package.as_str(), other.locked_in), ("@types/node", None), "another package's, by name and version");
        assert!(docs_request(dir.path(), "{}", 5000).unwrap_err().contains("which package"));
    }

    #[test]
    fn the_answer_is_about_the_item_and_cut_to_size() {
        let req = Request { ecosystem: Ecosystem::Npm, package: "lib".into(), version: "1.0.0".into(), item: Some("Client".into()), locked_in: Some("package-lock.json".into()), requirement: Some("^1".into()), max_chars: 1000 };
        let docs = Docs { source: "https://x".into(), text: format!("# lib\n\n## Install\n\nnpm i lib\n\n## Client\n\n{}", "word ".repeat(400)), about_item: false, note: None };
        let answer: serde_json::Value = serde_json::from_str(&answer_text(&req, &docs, true)).unwrap();
        assert!(answer["text"].as_str().unwrap().starts_with("## Client"));
        assert_eq!((answer["truncated"].as_bool(), answer["cached"].as_bool(), answer["locked_in"].as_str()), (Some(true), Some(true), Some("package-lock.json")));
        assert!(answer["text"].as_str().unwrap().chars().count() <= 1000);
        let missing = Request { item: Some("Nothing".into()), ..req };
        let answer: serde_json::Value = serde_json::from_str(&answer_text(&missing, &docs, false)).unwrap();
        assert!(answer["note"].as_str().unwrap().contains("nothing in it names Nothing"));
        assert!(answer["text"].as_str().unwrap().starts_with("# lib"));
    }

    #[test]
    fn names_from_uris() {
        assert_eq!(percent_decode("%40scope%2Fname"), "@scope/name");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(safe("github.com/a/b"), "github.com_a_b");
    }
}
