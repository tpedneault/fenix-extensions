//! The package manager page's Rust: cargo. The crates are `cargo
//! metadata`'s, the workspace's own left out, each marked with whether a
//! package of the workspace asks for it, and as what (`dev`, `build`).
//! Newer versions are cargo-outdated's when it's installed; else `cargo
//! update --dry-run --verbose`'s (what the requirements take), with the
//! newest of each crate the project asks for read from crates.io's
//! sparse index. cargo-audit audits. `cargo add`, `cargo remove` and
//! `cargo update` change them.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fenix_extension_api::Command;
use fenix_lang_kit::exec::split_words;
use fenix_lang_kit::packages::Found;
use fenix_lang_kit::version::Bump;
use fenix_lang_kit::{serde_json, Advisory, Dependency, Finding, Output, PackageManager, Risk, Update, Version};
use serde::Deserialize;


/// The newest version of each crate, as crates.io's index says, by name:
/// filled by the pack as its answers come.
pub type Latest = Rc<RefCell<BTreeMap<String, String>>>;

#[derive(Deserialize)]
struct Full {
    packages: Vec<Pkg>,
    workspace_members: Vec<String>,
    #[serde(default)]
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct Pkg {
    name: String,
    version: String,
    id: String,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    deps: Vec<Edge>,
}

#[derive(Deserialize)]
struct Edge {
    pkg: String,
    #[serde(default)]
    dep_kinds: Vec<Kind>,
}

#[derive(Deserialize)]
struct Kind {
    #[serde(default)]
    kind: Option<String>,
}

/// A group as the page shows it: the packages that ask for it, when the
/// workspace has several, then `dev` or `build` when that's all it's
/// asked as.
pub fn group(members: &[String], kinds: &[Option<String>], several: bool) -> Option<String> {
    let only = |k: &str| !kinds.is_empty() && kinds.iter().all(|x| x.as_deref() == Some(k));
    let kind = if only("dev") {
        Some("dev")
    } else if only("build") {
        Some("build")
    } else {
        None
    };
    let mut words = Vec::new();
    if several {
        words.push(members.join(","));
    }
    words.extend(kind.map(str::to_string));
    (!words.is_empty()).then(|| words.join(" "))
}

/// The other way: the package (`-p`) to change and `cargo add`'s flag for
/// the kind.
pub fn ungroup(group: Option<&str>) -> (Option<String>, Option<&'static str>) {
    let mut package = None;
    let mut flag = None;
    for word in group.unwrap_or("").split_whitespace() {
        match word {
            "dev" => flag = Some("--dev"),
            "build" => flag = Some("--build"),
            members => package = members.split(',').next().map(str::to_string),
        }
    }
    (package, flag)
}

/// `cargo metadata --format-version 1`: every crate the lock has, but the
/// workspace's own packages.
pub fn parse_list(out: &Output) -> Result<Vec<Dependency>, String> {
    if !out.ok() {
        return Err(crate::doctor::error_line(out));
    }
    let full: Full = out.json()?;
    let by_id: BTreeMap<&str, &Pkg> = full.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes = full.resolve.as_ref().map(|r| r.nodes.as_slice()).unwrap_or_default();
    let several = full.workspace_members.len() > 1;
    // Who asks for each, and as what.
    let mut asked: BTreeMap<&str, (Vec<String>, Vec<Option<String>>)> = BTreeMap::new();
    for node in nodes.iter().filter(|n| full.workspace_members.contains(&n.id)) {
        let member = by_id.get(node.id.as_str()).map(|p| p.name.clone()).unwrap_or_default();
        for edge in &node.deps {
            let (members, kinds) = asked.entry(edge.pkg.as_str()).or_default();
            if !members.contains(&member) {
                members.push(member.clone());
            }
            kinds.extend(edge.dep_kinds.iter().map(|k| k.kind.clone()));
        }
    }
    let mut out = Vec::new();
    for node in nodes.iter().filter(|n| !full.workspace_members.contains(&n.id)) {
        let Some(p) = by_id.get(node.id.as_str()) else { continue };
        let d = Dependency::new(p.name.as_str(), p.version.as_str());
        out.push(match asked.get(node.id.as_str()) {
            Some((members, kinds)) => match group(members, kinds, several) {
                Some(g) => d.group(g),
                None => d,
            },
            None => d.transitive(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.current.cmp(&b.current)));
    Ok(out)
}

/// A version cargo-outdated gives, or nothing for its `---` and
/// `Removed`.
fn version(text: &str) -> &str {
    if Version::parse(text).is_some() && text.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        text
    } else {
        ""
    }
}

#[derive(Deserialize)]
struct Outdated {
    dependencies: Vec<OutdatedDep>,
}

#[derive(Deserialize)]
struct OutdatedDep {
    name: String,
    project: String,
    #[serde(default)]
    compat: String,
    #[serde(default)]
    latest: String,
}

/// `cargo outdated --format json --workspace`: a line for each package
/// of the workspace.
pub fn parse_outdated(out: &Output) -> Result<Vec<Update>, String> {
    if !out.ok() {
        return Err(crate::doctor::error_line(out));
    }
    let mut updates = Vec::new();
    for value in out.json_stream() {
        let o: Outdated = serde_json::from_value(value).map_err(|e| format!("its output isn't what was expected: {e}"))?;
        for d in o.dependencies {
            let u = Update::new(d.name).current(d.project.as_str()).wanted(version(&d.compat)).latest(version(&d.latest));
            if (u.wanted.is_some() || u.latest.is_some()) && !updates.contains(&u) {
                updates.push(u);
            }
        }
    }
    Ok(updates)
}

/// `cargo update --dry-run --verbose`: `Updating serde v1.0.100 ->
/// v1.0.210` is what its requirement takes; `Unchanged clap v3.2.0
/// (available: v3.2.25)` a newer one it doesn't.
pub fn parse_update(out: &Output) -> Result<Vec<Update>, String> {
    if !out.ok() {
        return Err(crate::doctor::error_line(out));
    }
    let v = |s: &str| s.trim_start_matches('v').to_string();
    let mut updates = Vec::new();
    for line in out.text().lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["Updating", name, from, "->", to, ..] => updates.push(Update::new(*name).current(v(from)).wanted(v(to))),
            ["Unchanged", name, from, "(available:", to] => updates.push(Update::new(*name).current(v(from)).latest(v(to.trim_end_matches(')')))),
            _ => {}
        }
    }
    Ok(updates)
}

#[derive(Deserialize)]
struct Audit {
    vulnerabilities: Vulnerabilities,
}

#[derive(Deserialize)]
struct Vulnerabilities {
    #[serde(default)]
    list: Vec<Vulnerability>,
}

#[derive(Deserialize)]
struct Vulnerability {
    advisory: RustSec,
    #[serde(default)]
    versions: Versions,
    package: Locked,
}

#[derive(Deserialize)]
struct RustSec {
    id: String,
    title: String,
    #[serde(default)]
    cvss: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Deserialize, Default)]
struct Versions {
    #[serde(default)]
    patched: Vec<String>,
}

#[derive(Deserialize)]
struct Locked {
    name: String,
    version: String,
}

/// The first version without it, for one at `current`: the lowest of the
/// patched ranges' lower bounds above it (`>=0.6.14, <1.0.0`, `>=1.6.1`
/// for 1.6.0 is 1.6.1).
pub fn fixed_in(patched: &[String], current: &str) -> Option<String> {
    let now = Version::parse(current);
    let mut bounds: Vec<(Version, String)> = patched
        .iter()
        .flat_map(|range| range.split(','))
        .filter_map(|c| c.trim().strip_prefix(">=").or_else(|| c.trim().strip_prefix('^')).map(str::trim))
        .filter_map(|b| Version::parse(b).map(|v| (v, b.to_string())))
        .collect();
    bounds.sort_by(|a, b| a.0.cmp(&b.0));
    let above = bounds.iter().find(|(v, _)| now.as_ref().is_none_or(|n| v > n));
    above.or(bounds.last()).map(|(_, b)| b.clone())
}

/// `cargo audit --json`: an advisory for each vulnerability, its risk
/// from its CVSS vector. It exits 1 when it found some.
pub fn parse_audit(out: &Output) -> Result<Vec<Finding>, String> {
    if out.stdout.trim().is_empty() {
        return Err(crate::doctor::error_line(out));
    }
    let audit: Audit = out.json()?;
    Ok(audit
        .vulnerabilities
        .list
        .into_iter()
        .map(|v| {
            let a = v.advisory;
            // A CVSS vector, scored by lang-kit.
            let risk = a.cvss.as_deref().map(Risk::parse).unwrap_or(Risk::Unknown);
            let title = match a.aliases.iter().find(|x| x.starts_with("CVE-")) {
                Some(cve) => format!("{cve}: {}", a.title),
                None => a.title,
            };
            let advisory = Advisory::new(a.id.as_str(), risk).title(title).fixed_in(fixed_in(&v.versions.patched, &v.package.version).unwrap_or_default()).url(format!("https://rustsec.org/advisories/{}.html", a.id));
            Finding::new(v.package.name, Some(&v.package.version), advisory)
        })
        .collect())
}

/// `cargo search`'s lines: `axum = "0.8.9"    # what it is`.
pub fn parse_search(out: &Output) -> Result<Vec<Found>, String> {
    if !out.ok() {
        return Err(crate::doctor::error_line(out));
    }
    Ok(out
        .stdout
        .lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(" = \"")?;
            let (version, rest) = rest.split_once('"')?;
            let description = rest.split_once('#').map(|(_, d)| d.trim()).unwrap_or("");
            Some(Found { name: name.trim().into(), version: version.into(), description: description.into() })
        })
        .collect())
}

/// Where crate `name`'s entry is in the sparse index.
pub fn index_url(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    let path = match n.len() {
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    };
    format!("https://index.crates.io/{path}")
}

#[derive(Deserialize)]
struct Entry {
    vers: String,
    #[serde(default)]
    yanked: bool,
}

/// The newest release in an index entry (a line of JSON for each
/// version): one that isn't yanked, and not a pre-release unless that's
/// all there is.
pub fn newest(entry: &str) -> Option<String> {
    let mut versions: Vec<(Version, String)> = entry
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .filter(|e| !e.yanked)
        .filter_map(|e| Version::parse(&e.vers).map(|v| (v, e.vers)))
        .collect();
    versions.sort_by(|a, b| a.0.cmp(&b.0));
    let stable = versions.iter().rev().find(|(v, _)| !v.is_prerelease());
    stable.or(versions.last()).map(|(_, s)| s.clone())
}

/// cargo, for a project in `root` (`/project`).
pub struct Cargo {
    pub cargo: Command,
    pub root: String,
    /// cargo-outdated is installed.
    pub outdated_tool: bool,
    /// cargo-audit is installed.
    pub audit_tool: bool,
    pub latest: Latest,
    /// The package `cargo add` adds to when what's typed doesn't say
    /// (`-p`): a workspace with no package of its own needs one.
    pub add_to: Option<String>,
}

impl Cargo {
    pub fn new(cargo: Command, root: &str) -> Cargo {
        Cargo { cargo, root: root.into(), outdated_tool: false, audit_tool: false, latest: Latest::default(), add_to: None }
    }

    fn cargo(&self, args: &[&str]) -> Command {
        self.cargo.clone().args(args.iter().copied())
    }

    /// `-p package` and `--dev`/`--build`, for changing `dep`'s entry.
    fn entry_flags(dep: &Dependency) -> Vec<String> {
        let (package, flag) = ungroup(dep.group.as_deref());
        let mut args = Vec::new();
        if let Some(p) = package {
            args.extend(["-p".to_string(), p]);
        }
        args.extend(flag.map(str::to_string));
        args
    }
}

impl PackageManager for Cargo {
    fn name(&self) -> &str {
        "cargo"
    }
    fn list(&self) -> Command {
        self.cargo(&["metadata", "--format-version", "1"])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out)
    }
    fn outdated(&self) -> Option<Command> {
        Some(if self.outdated_tool { self.cargo(&["outdated", "--format", "json", "--workspace"]) } else { self.cargo(&["update", "--dry-run", "--verbose"]) })
    }
    /// What the tool said, then the index's newest for what it didn't.
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        if self.outdated_tool {
            return parse_outdated(out);
        }
        let mut updates = parse_update(out)?;
        updates.extend(self.latest.borrow().iter().map(|(name, v)| Update::new(name.as_str()).latest(v.as_str())));
        Ok(updates)
    }
    fn audit(&self) -> Option<Command> {
        self.audit_tool.then(|| self.cargo(&["audit", "--json"]))
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }
    /// Its requirement moved to the latest when that's a major step
    /// (`cargo add name@2.0.1`); else the lock moved as far as the
    /// requirement takes it (`cargo update -p name@1.0.0`).
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        match &dep.latest {
            Some(latest) if dep.direct && dep.bump() == Bump::Major => Some(self.cargo(&["add"]).args(Cargo::entry_flags(dep)).arg(format!("{}@{latest}", dep.name))),
            _ => Some(self.cargo(&["update", "-p"]).arg(format!("{}@{}", dep.name, dep.current))),
        }
    }
    /// The lock, as far as every requirement takes it.
    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        Some(self.cargo(&["update"]))
    }
    /// `cargo add` with what's typed (`serde --features derive`,
    /// `tokio@1 -F full`, `-p orbit rand`).
    fn add(&self, package: &str) -> Command {
        let words = split_words(package);
        let says = words.iter().any(|w| w == "-p" || w == "--package" || w.starts_with("--package="));
        let cmd = self.cargo(&["add"]);
        let cmd = match (&self.add_to, says) {
            (Some(p), false) => cmd.args(["-p", p.as_str()]),
            _ => cmd,
        };
        cmd.args(words)
    }
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        dep.direct.then(|| self.cargo(&["remove"]).args(Cargo::entry_flags(dep)).arg(&dep.name))
    }
    fn search(&self, query: &str) -> Option<Command> {
        Some(self.cargo(&["search", "--limit", "20"]).arg(query))
    }
    fn parse_search(&self, out: &Output) -> Result<Vec<Found>, String> {
        parse_search(out)
    }
    /// crates.io's names: `-` and `_` are the same crate.
    fn normalize(&self, name: &str) -> String {
        name.replace('_', "-").to_ascii_lowercase()
    }
    fn lockfiles(&self) -> Vec<String> {
        vec![format!("{}/Cargo.lock", self.root), format!("{}/Cargo.toml", self.root)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::ok;
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    const METADATA: &str = include_str!("samples/cargo-metadata.json");
    const OUTDATED: &str = include_str!("samples/cargo-outdated.json");
    const UPDATE: &str = include_str!("samples/cargo-update-dry-run.txt");
    const AUDIT: &str = include_str!("samples/cargo-audit.json");
    const SEARCH: &str = include_str!("samples/cargo-search.txt");
    const INDEX: &str = include_str!("samples/index-smallvec.jsonl");

    fn cargo() -> Cargo {
        Cargo::new(Command::new("cargo"), "/project")
    }

    #[test]
    fn the_list_is_every_locked_crate_marked_with_who_asks() {
        let deps = parse_list(&ok(METADATA)).unwrap();
        let get = |n: &str| deps.iter().find(|d| d.name == n).unwrap_or_else(|| panic!("{n}"));
        assert!(!deps.iter().any(|d| d.name == "orbit" || d.name == "orbit-core"), "the workspace's own aren't");
        assert_eq!((get("clap").current.as_str(), get("clap").group.as_deref(), get("clap").direct), ("3.2.0", Some("orbit"), true));
        assert_eq!(get("smallvec").group.as_deref(), Some("orbit dev"));
        assert_eq!(get("cc").group.as_deref(), Some("orbit build"));
        assert_eq!(get("time").group.as_deref(), Some("orbit-core"));
        assert!(!get("atty").direct, "clap's");
        assert!(parse_list(&Output { code: Some(101), stdout: String::new(), stderr: "error: failed to parse manifest".into() }).unwrap_err().contains("failed to parse"));
    }

    #[test]
    fn groups_go_both_ways() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(group(&s(&["orbit"]), &[None], false), None);
        assert_eq!(group(&s(&["orbit"]), &[Some("dev".into())], false).as_deref(), Some("dev"));
        assert_eq!(group(&s(&["a", "b"]), &[Some("dev".into()), None], true).as_deref(), Some("a,b"), "asked for as more than dev");
        assert_eq!(ungroup(Some("a,b build")), (Some("a".into()), Some("--build")));
        assert_eq!(ungroup(Some("dev")), (None, Some("--dev")));
        assert_eq!(ungroup(None), (None, None));
    }

    #[test]
    fn cargo_outdated_says_what_requirements_take_and_the_newest() {
        let updates = parse_outdated(&ok(OUTDATED)).unwrap();
        let get = |n: &str| updates.iter().find(|u| u.name == n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!((get("clap").current.as_deref(), get("clap").wanted.as_deref(), get("clap").latest.as_deref()), (Some("3.2.0"), None, Some("4.6.7")));
        assert_eq!(get("time").latest.as_deref(), Some("0.3.55"));
        assert!(!updates.iter().any(|u| u.name == "atty"), "Removed is no version");
        assert_eq!(updates.iter().filter(|u| u.name == "smallvec").count(), 1);
    }

    #[test]
    fn without_it_cargo_update_and_the_index_say_them() {
        let out = Output { code: Some(0), stdout: String::new(), stderr: UPDATE.into() };
        let updates = parse_update(&out).unwrap();
        let rows: Vec<_> = updates.iter().map(|u| (u.name.as_str(), u.current.as_deref(), u.wanted.as_deref(), u.latest.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("cc", Some("1.0.83"), Some("1.10.0"), None),
                ("clap", Some("3.2.0"), None, Some("3.2.25")),
                ("libc", Some("0.2.150"), Some("0.2.191"), None),
                ("smallvec", Some("1.6.0"), None, Some("1.16.4")),
                ("time", Some("0.1.43"), None, Some("0.1.45")),
            ]
        );
        assert_eq!(newest(INDEX).as_deref(), Some("1.16.4"), "not 2.0.0-beta.3, a pre-release");
        assert_eq!(newest("{\"vers\":\"0.1.0-alpha\",\"yanked\":false}\n{\"vers\":\"0.1.0\",\"yanked\":true}\n").as_deref(), Some("0.1.0-alpha"), "a pre-release when that's all");
        assert_eq!(index_url("Serde"), "https://index.crates.io/se/rd/serde");
        assert_eq!(index_url("cc"), "https://index.crates.io/2/cc");
        assert_eq!(index_url("syn"), "https://index.crates.io/3/s/syn");
        assert_eq!(index_url("a"), "https://index.crates.io/1/a");

        // The index's newest after cargo's.
        let m = cargo();
        m.latest.borrow_mut().insert("clap".into(), "4.6.7".into());
        let all = merge(&m, parse_list(&ok(METADATA)).unwrap(), &m.parse_outdated(&out).unwrap(), &[]);
        let clap = all.iter().find(|d| d.name == "clap").unwrap();
        assert_eq!((clap.latest.as_deref(), clap.bump()), (Some("4.6.7"), Bump::Major));
        assert_eq!(line(&m.outdated().unwrap()), "cargo update --dry-run --verbose");
    }

    #[test]
    fn cargo_audit_says_the_vulnerable_with_their_risk() {
        let findings = parse_audit(&Output { code: Some(1), ..ok(AUDIT) }).unwrap();
        let rows: Vec<_> = findings.iter().map(|f| (f.name.as_str(), f.version.as_deref(), f.advisory.id.as_str(), f.advisory.risk, f.advisory.fixed_in.as_deref())).collect();
        assert_eq!(rows, [("smallvec", Some("1.6.0"), "RUSTSEC-2021-0003", Risk::Critical, Some("1.6.1")), ("time", Some("0.1.43"), "RUSTSEC-2020-0071", Risk::Moderate, Some("0.2.23"))]);
        assert_eq!(findings[0].advisory.title, "CVE-2021-25900: Buffer overflow in SmallVec::insert_many");
        assert_eq!(findings[0].advisory.url.as_deref(), Some("https://rustsec.org/advisories/RUSTSEC-2021-0003.html"));
        assert!(parse_audit(&Output { code: Some(1), stdout: String::new(), stderr: "error: couldn't fetch advisory database".into() }).unwrap_err().contains("advisory database"));
        assert_eq!(fixed_in(&[">=0.6.14, <1.0.0".into(), ">=1.6.1".into()], "0.6.10").as_deref(), Some("0.6.14"));
        assert_eq!(fixed_in(&["^0.2.9".into()], "0.3.0").as_deref(), Some("0.2.9"), "none above: the newest bound");
        assert_eq!(fixed_in(&[], "1.0.0"), None);
    }

    #[test]
    fn cargo_changes_the_requirements_and_the_lock() {
        let m = cargo();
        let major = Dependency::new("clap", "3.2.0").latest("4.6.7").group("orbit");
        assert_eq!(line(&m.upgrade(&major).unwrap()), "cargo add -p orbit clap@4.6.7");
        let dev = Dependency::new("smallvec", "1.6.0").latest("1.16.4").group("dev");
        assert_eq!(line(&m.upgrade(&dev).unwrap()), "cargo update -p smallvec@1.6.0", "a minor step is the lock's");
        let deep = Dependency::new("libc", "0.2.150").wanted("0.2.191").transitive();
        assert_eq!(line(&m.upgrade(&deep).unwrap()), "cargo update -p libc@0.2.150");
        assert_eq!(m.remove(&deep), None, "one nothing of the project's asks for isn't removed");
        let build = Dependency::new("cc", "1.0.83").group("orbit build");
        assert_eq!(line(&m.remove(&build).unwrap()), "cargo remove -p orbit --build cc");
        assert_eq!(line(&m.upgrade_all(&[]).unwrap()), "cargo update");
        assert_eq!(line(&m.add("serde --features derive")), "cargo add serde --features derive");
        let ws = Cargo { add_to: Some("orbit".into()), ..cargo() };
        assert_eq!(line(&ws.add("tokio@1 -F full")), "cargo add -p orbit tokio@1 -F full");
        assert_eq!(line(&ws.add("-p orbit-core rand")), "cargo add -p orbit-core rand", "what's typed says which");
        assert_eq!(m.audit(), None, "no cargo-audit");
        let tools = Cargo { audit_tool: true, outdated_tool: true, ..cargo() };
        assert_eq!(line(&tools.audit().unwrap()), "cargo audit --json");
        assert_eq!(line(&tools.outdated().unwrap()), "cargo outdated --format json --workspace");
        assert_eq!(m.lockfiles(), ["/project/Cargo.lock", "/project/Cargo.toml"]);
        assert_eq!(m.normalize("Serde_JSON"), "serde-json");
    }

    #[test]
    fn a_search_finds_crates_to_add() {
        let found = parse_search(&ok(SEARCH)).unwrap();
        assert_eq!(found.len(), 5, "not its `... and 6332 crates more`");
        assert_eq!((found[0].name.as_str(), found[0].version.as_str()), ("axum", "0.8.9"));
        assert_eq!(found[0].description, "HTTP routing and request handling library that focuses on ergonomics and modularity");
        assert_eq!(line(&cargo().search("axum").unwrap()), "cargo search --limit 20 axum");
    }
}
