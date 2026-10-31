//! The package manager page's JavaScript: npm, pnpm, Yarn 1, Yarn 2 and
//! later (berry), and bun, the one the lockfile says. Each lists what's
//! installed, its outdated and its audit as JSON where it can (bun's
//! `outdated` and `pm ls` print tables, read as such), and adds, removes
//! and upgrades with its own commands. Which dependencies the project
//! asks for, and in which group, is read from `package.json`.
//!
//! Berry has no `outdated`: its page shows what's installed and
//! vulnerable, and `U` upgrades nothing.

use std::collections::BTreeSet;

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{Advisory, Dependency, Finding, Found, Output, PackageManager, Risk, Update};

use crate::project::{Manager, Project};

/// The first version a range like `<4.17.21` or `>=4.0.0 <4.17.21` (or
/// `patched_versions`' `>=4.17.21`) leaves out, or takes.
pub fn fixed_in(range: &str) -> Option<String> {
    let words: Vec<&str> = range.split_whitespace().collect();
    if let [only] = words.as_slice() {
        if let Some(v) = only.strip_prefix(">=") {
            return Some(v.to_string());
        }
    }
    words.iter().rev().find_map(|w| w.strip_prefix('<').filter(|v| !v.starts_with('='))).map(str::to_string)
}

/// Each dependency marked as the project asks for it: direct, and its
/// group.
fn mark(d: Dependency, project: &Project) -> Dependency {
    match project.group_of(&d.name) {
        None => d.transitive(),
        Some(None) => d,
        Some(Some(g)) => d.group(g),
    }
}

/// `name@1.2.3`, `@scope/name@1.2.3`, `name@npm:1.2.3`: its name and
/// version.
pub fn name_version(text: &str) -> Option<(String, String)> {
    let (name, version) = text.rsplit_once('@').filter(|(n, _)| !n.is_empty())?;
    let version = version.strip_prefix("npm:").unwrap_or(version);
    Some((name.to_string(), version.to_string()))
}

/// Rows once each, by name and version, in order.
fn once(rows: impl IntoIterator<Item = (String, String)>, project: &Project) -> Vec<Dependency> {
    let mut seen = BTreeSet::new();
    rows.into_iter().filter(|r| seen.insert(r.clone())).map(|(n, v)| mark(Dependency::new(n, v), project)).collect()
}

/// npm's and pnpm's tree (`npm ls --all --json`, `pnpm list --json
/// --depth Infinity`), every package in it once.
fn walk(node: &Value, out: &mut Vec<(String, String)>) {
    for key in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(deps) = node[key].as_object() {
            for (name, d) in deps {
                if let Some(v) = d["version"].as_str() {
                    // A linked workspace's is a path.
                    if !v.starts_with("link:") && !v.starts_with("file:") {
                        out.push((name.clone(), v.to_string()));
                    }
                }
                walk(d, out);
            }
        }
    }
}

pub fn parse_npm_tree(out: &Output, project: &Project) -> Result<Vec<Dependency>, String> {
    let doc: Value = out.json()?;
    let mut rows = Vec::new();
    match &doc {
        // pnpm: one tree per project.
        Value::Array(projects) => projects.iter().for_each(|p| walk(p, &mut rows)),
        root => walk(root, &mut rows),
    }
    Ok(once(rows, project))
}

/// `npm outdated --json` and `pnpm outdated --format json`: by name,
/// its current, wanted and latest. Both exit 1 when they found some.
pub fn parse_outdated_map(out: &Output) -> Result<Vec<Update>, String> {
    if out.stdout.trim().is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let doc: serde_json::Map<String, Value> = out.json()?;
    let mut rows = Vec::new();
    for (name, v) in doc {
        // npm gives an array when the same package is outdated in two
        // places (a workspace's).
        let each: Vec<Value> = match v {
            Value::Array(a) => a,
            v => vec![v],
        };
        for v in each {
            let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
            rows.push(Update::new(name.clone()).current(s("current")).wanted(s("wanted")).latest(s("latest")));
        }
    }
    Ok(rows)
}

/// `npm audit --json` (its report version 2): each advisory a
/// vulnerable package's `via` gives; a `via` that's only another
/// package's name is that one's.
pub fn parse_npm_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let doc: Value = out.json()?;
    if let Some(e) = doc["error"]["summary"].as_str() {
        return Err(e.to_string());
    }
    let Some(vulns) = doc["vulnerabilities"].as_object() else { return Ok(Vec::new()) };
    let mut found = Vec::new();
    for (name, v) in vulns {
        for via in v["via"].as_array().into_iter().flatten().filter(|via| via.is_object()) {
            let url = via["url"].as_str().unwrap_or("");
            let id = url.rsplit('/').next().filter(|s| s.starts_with("GHSA-")).map(str::to_string).unwrap_or_else(|| via["source"].to_string());
            let range = via["range"].as_str().unwrap_or("");
            let mut a = Advisory::new(id, Risk::parse(via["severity"].as_str().unwrap_or(""))).title(via["title"].as_str().unwrap_or("")).url(url);
            if let Some(f) = fixed_in(range) {
                a = a.fixed_in(f);
            }
            found.push(Finding::new(via["name"].as_str().unwrap_or(name), None, a));
        }
    }
    Ok(found)
}

/// An advisory as npm's old audit format has it -- `pnpm audit --json`'s
/// `advisories`, `yarn audit --json`'s `auditAdvisory` lines: one
/// finding for each version it was found in.
fn v1_advisory(a: &Value) -> Vec<Finding> {
    let id = a["github_advisory_id"].as_str().map(str::to_string).unwrap_or_else(|| a["id"].to_string());
    let mut adv = Advisory::new(id, Risk::parse(a["severity"].as_str().unwrap_or(""))).title(a["title"].as_str().unwrap_or("")).url(a["url"].as_str().unwrap_or(""));
    if let Some(f) = a["patched_versions"].as_str().and_then(fixed_in).or_else(|| a["vulnerable_versions"].as_str().and_then(fixed_in)) {
        adv = adv.fixed_in(f);
    }
    let name = a["module_name"].as_str().unwrap_or("");
    let versions: BTreeSet<&str> = a["findings"].as_array().into_iter().flatten().filter_map(|f| f["version"].as_str()).collect();
    if versions.is_empty() {
        return vec![Finding::new(name, None, adv)];
    }
    versions.into_iter().map(|v| Finding::new(name, Some(v), adv.clone())).collect()
}

pub fn parse_pnpm_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let doc: Value = out.json()?;
    if let Some(e) = doc["error"]["message"].as_str() {
        return Err(e.to_string());
    }
    Ok(doc["advisories"].as_object().into_iter().flat_map(|m| m.values()).flat_map(v1_advisory).collect())
}

/// Yarn's JSON lines that read.
fn lines(out: &Output) -> Vec<Value> {
    out.stdout.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// `yarn list --json`: its tree, every package in it at the top (Yarn 1
/// hoists them there), a child that's only a range left out.
pub fn parse_yarn_list(out: &Output, project: &Project) -> Result<Vec<Dependency>, String> {
    let tree = lines(out).into_iter().find(|l| l["type"] == "tree").ok_or_else(|| format!("yarn list said no tree: {}", out.why()))?;
    let rows = tree["data"]["trees"].as_array().into_iter().flatten().filter_map(|t| name_version(t["name"].as_str()?)).filter(|(_, v)| v.starts_with(|c: char| c.is_ascii_digit()));
    Ok(once(rows, project))
}

/// `yarn outdated --json`: a table line, its head saying the columns.
pub fn parse_yarn_outdated(out: &Output) -> Result<Vec<Update>, String> {
    let Some(table) = lines(out).into_iter().find(|l| l["type"] == "table") else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let head: Vec<&str> = table["data"]["head"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let col = |row: &Value, name: &str| head.iter().position(|h| *h == name).and_then(|i| row[i].as_str()).unwrap_or("").to_string();
    Ok(table["data"]["body"].as_array().into_iter().flatten().map(|r| Update::new(col(r, "Package")).current(col(r, "Current")).wanted(col(r, "Wanted")).latest(col(r, "Latest"))).collect())
}

/// `yarn audit --json`: its `auditAdvisory` lines.
pub fn parse_yarn_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let all = lines(out);
    if all.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(all.iter().filter(|l| l["type"] == "auditAdvisory").flat_map(|l| v1_advisory(&l["data"]["advisory"])).collect())
}

/// `yarn info --all --recursive --json` (berry): a line a package, the
/// project's own workspaces left out.
pub fn parse_berry_info(out: &Output, project: &Project) -> Result<Vec<Dependency>, String> {
    let all = lines(out);
    if all.is_empty() && !out.ok() {
        return Err(out.why());
    }
    let rows = all.iter().filter_map(|l| {
        let (name, _) = name_version(l["value"].as_str()?)?;
        let version = l["children"]["Version"].as_str()?;
        (!l["value"].as_str()?.contains("@workspace:")).then(|| (name, version.to_string()))
    });
    Ok(once(rows, project))
}

/// `yarn npm audit --all --recursive --json` (berry): a line an
/// advisory, for each version in the tree.
pub fn parse_berry_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let all = lines(out);
    if all.is_empty() && !out.ok() {
        return Err(out.why());
    }
    let mut found = Vec::new();
    for l in &all {
        let (Some(name), c) = (l["value"].as_str(), &l["children"]) else { continue };
        let url = c["URL"].as_str().unwrap_or("");
        let id = url.rsplit('/').next().filter(|s| s.starts_with("GHSA-")).map(str::to_string).unwrap_or_else(|| c["ID"].to_string());
        let mut a = Advisory::new(id, Risk::parse(c["Severity"].as_str().unwrap_or(""))).title(c["Issue"].as_str().unwrap_or("")).url(url);
        if let Some(f) = c["Vulnerable Versions"].as_str().and_then(fixed_in) {
            a = a.fixed_in(f);
        }
        let versions: Vec<&str> = c["Tree Versions"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if versions.is_empty() {
            found.push(Finding::new(name, None, a));
            continue;
        }
        for v in versions {
            found.push(Finding::new(name, Some(v), a.clone()));
        }
    }
    Ok(found)
}

/// `bun pm ls --all`: a tree of `name@version` lines.
pub fn parse_bun_ls(out: &Output, project: &Project) -> Result<Vec<Dependency>, String> {
    if !out.ok() {
        return Err(out.why());
    }
    let rows = out.stdout.lines().filter_map(|l| {
        let item = l.trim_start_matches(|c: char| c.is_whitespace() || "├└│─┬".contains(c));
        (item.len() < l.len()).then_some(())?;
        name_version(item.split_whitespace().next()?)
    });
    Ok(once(rows, project))
}

/// `bun outdated`'s table: `| name (dev) | current | update | latest |`.
pub fn parse_bun_outdated(out: &Output) -> Result<Vec<Update>, String> {
    if !out.ok() && out.stdout.trim().is_empty() {
        return Err(out.why());
    }
    Ok(out
        .stdout
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.trim().strip_prefix('|')?.strip_suffix('|')?.split('|').map(str::trim).collect();
            let [name, current, update, latest] = cells.as_slice() else { return None };
            if *name == "Package" || name.starts_with('-') {
                return None;
            }
            let name = name.split(" (").next().unwrap_or(name);
            Some(Update::new(name).current(*current).wanted(*update).latest(*latest))
        })
        .collect())
}

/// `bun audit --json`: by package, its advisories (npm's bulk format,
/// no version said).
pub fn parse_bun_audit(out: &Output) -> Result<Vec<Finding>, String> {
    if out.stdout.trim().is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let doc: serde_json::Map<String, Value> = out.json()?;
    let mut found = Vec::new();
    for (name, advisories) in doc {
        for a in advisories.as_array().into_iter().flatten() {
            let url = a["url"].as_str().unwrap_or("");
            let id = url.rsplit('/').next().filter(|s| s.starts_with("GHSA-")).map(str::to_string).unwrap_or_else(|| a["id"].to_string());
            let mut adv = Advisory::new(id, Risk::parse(a["severity"].as_str().unwrap_or(""))).title(a["title"].as_str().unwrap_or("")).url(url);
            if let Some(f) = a["vulnerable_versions"].as_str().and_then(fixed_in) {
                adv = adv.fixed_in(f);
            }
            found.push(Finding::new(name.clone(), None, adv));
        }
    }
    Ok(found)
}

/// `npm search --json`: name, version, description.
pub fn parse_search(out: &Output) -> Result<Vec<Found>, String> {
    let rows: Vec<Value> = out.json()?;
    Ok(rows.iter().map(|r| Found { name: r["name"].as_str().unwrap_or("").into(), version: r["version"].as_str().unwrap_or("").into(), description: r["description"].as_str().unwrap_or("").into() }).collect())
}

/// The project's package manager, as the page uses it.
pub struct Js {
    pub manager: Manager,
    /// The manager's program.
    pub program: Command,
    /// npm, for searching the registry: every manager installs from it.
    pub npm: Option<Command>,
    pub project: Project,
    pub root: String,
}

impl Js {
    fn cmd(&self, line: &[&str]) -> Command {
        self.program.clone().args(line.iter().copied())
    }

    /// The flag that keeps one in its group when it's added again.
    fn dev_flag(&self, dep: &Dependency) -> Option<&'static str> {
        (dep.group.as_deref() == Some("dev")).then_some(match self.manager {
            Manager::Bun => "--dev",
            Manager::Npm => "--save-dev",
            _ => "--dev",
        })
    }

    fn add_cmd(&self) -> &'static str {
        match self.manager {
            Manager::Npm => "install",
            _ => "add",
        }
    }
}

impl PackageManager for Js {
    fn name(&self) -> &str {
        self.manager.name()
    }

    fn list(&self) -> Command {
        match self.manager {
            Manager::Npm => self.cmd(&["ls", "--all", "--json"]),
            Manager::Pnpm => self.cmd(&["list", "--json", "--depth", "Infinity"]),
            Manager::Yarn => self.cmd(&["list", "--json", "--no-progress"]),
            Manager::Berry => self.cmd(&["info", "--all", "--recursive", "--json"]),
            Manager::Bun => self.cmd(&["pm", "ls", "--all"]),
        }
    }

    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        match self.manager {
            // npm ls exits 1 with what's missing or extraneous, and still lists.
            Manager::Npm | Manager::Pnpm => parse_npm_tree(out, &self.project),
            Manager::Yarn => parse_yarn_list(out, &self.project),
            Manager::Berry => parse_berry_info(out, &self.project),
            Manager::Bun => parse_bun_ls(out, &self.project),
        }
    }

    fn outdated(&self) -> Option<Command> {
        Some(match self.manager {
            Manager::Npm => self.cmd(&["outdated", "--json"]),
            Manager::Pnpm => self.cmd(&["outdated", "--format", "json"]),
            Manager::Yarn => self.cmd(&["outdated", "--json"]),
            Manager::Berry => return None,
            Manager::Bun => self.cmd(&["outdated"]),
        })
    }

    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        match self.manager {
            Manager::Npm | Manager::Pnpm => parse_outdated_map(out),
            Manager::Yarn => parse_yarn_outdated(out),
            Manager::Berry => Ok(Vec::new()),
            Manager::Bun => parse_bun_outdated(out),
        }
    }

    fn audit(&self) -> Option<Command> {
        Some(match self.manager {
            Manager::Npm | Manager::Pnpm | Manager::Yarn => self.cmd(&["audit", "--json"]),
            Manager::Berry => self.cmd(&["npm", "audit", "--all", "--recursive", "--json"]),
            Manager::Bun => self.cmd(&["audit", "--json"]),
        })
    }

    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        match self.manager {
            Manager::Npm => parse_npm_audit(out),
            Manager::Pnpm => parse_pnpm_audit(out),
            Manager::Yarn => parse_yarn_audit(out),
            Manager::Berry => parse_berry_audit(out),
            Manager::Bun => parse_bun_audit(out),
        }
    }

    /// One the project asks for: its latest, in its group (the range in
    /// `package.json` moves with it). One it doesn't: within the ranges
    /// that bring it.
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        if dep.direct {
            let latest = format!("{}@latest", dep.name);
            let cmd = match self.manager {
                Manager::Berry => self.cmd(&["up", &latest]),
                _ => self.cmd(&[self.add_cmd(), &latest]),
            };
            return Some(cmd.args(self.dev_flag(dep).filter(|_| self.manager != Manager::Berry)));
        }
        Some(match self.manager {
            Manager::Berry => self.cmd(&["up", "-R", &dep.name]),
            Manager::Yarn => self.cmd(&["upgrade", &dep.name]),
            _ => self.cmd(&["update", &dep.name]),
        })
    }

    /// The outdated, within the ranges `package.json` gives.
    fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
        Some(match self.manager {
            Manager::Yarn => self.cmd(&["upgrade"]),
            Manager::Berry if outdated.is_empty() => return None,
            Manager::Berry => self.cmd(&["up", "-R"]).args(outdated.iter().map(|d| d.name.clone())),
            _ => self.cmd(&["update"]),
        })
    }

    /// What's typed, as the manager takes it: `zod`, `zod@3`, `vitest
    /// -D`.
    fn add(&self, package: &str) -> Command {
        self.cmd(&[self.add_cmd()]).args(fenix_lang_kit::split_words(package))
    }

    fn remove(&self, dep: &Dependency) -> Option<Command> {
        let verb = match self.manager {
            Manager::Npm => "uninstall",
            _ => "remove",
        };
        dep.direct.then(|| self.cmd(&[verb, &dep.name]))
    }

    fn search(&self, query: &str) -> Option<Command> {
        self.npm.clone().map(|npm| npm.args(["search", "--json", "--searchlimit", "20", query]))
    }

    fn parse_search(&self, out: &Output) -> Result<Vec<Found>, String> {
        parse_search(out)
    }

    fn lockfiles(&self) -> Vec<String> {
        let lock = self.project.lockfile.iter().cloned();
        std::iter::once("package.json".to_string()).chain(lock).map(|f| format!("{}/{f}", self.root)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::PACKAGE;
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    /// The samples' project: lodash 4.17.20 and chalk 4 asked for,
    /// semver ~7.5 for development.
    fn sample() -> Project {
        Project::parse(r#"{"name": "orbit-web", "dependencies": {"lodash": "4.17.20", "chalk": "^4.1.0"}, "devDependencies": {"semver": "~7.5.0"}}"#, &["package-lock.json"]).unwrap()
    }

    fn js(manager: Manager) -> Js {
        Js { manager, program: Command::new(manager.program()), npm: Some(Command::new("npm")), project: sample(), root: "/project".into() }
    }

    /// What the page shows, as name, current, latest, direct, group and
    /// advisories.
    type Row = (String, String, Option<String>, bool, Option<String>, usize);

    fn rows(m: &Js, list: &str, list_code: i32, outdated: Option<(&str, i32)>, audit: (&str, i32)) -> Vec<Row> {
        let deps = m.parse_list(&out(list_code, list)).unwrap();
        let updates = outdated.map(|(o, c)| m.parse_outdated(&out(c, o)).unwrap()).unwrap_or_default();
        let findings = m.parse_audit(&out(audit.1, audit.0)).unwrap();
        let mut all = merge(m, deps, &updates, &findings);
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all.into_iter().map(|d| (d.name.clone(), d.current.clone(), d.latest.clone(), d.direct, d.group.clone(), d.advisories.len())).collect()
    }

    fn picked<'a>(rows: &'a [Row], name: &str) -> &'a Row {
        rows.iter().find(|r| r.0 == name).unwrap()
    }

    #[test]
    fn npm_lists_its_tree_with_outdated_and_audit() {
        let m = js(Manager::Npm);
        let r = rows(&m, include_str!("samples/npm-ls.json"), 0, Some((include_str!("samples/npm-outdated.json"), 1)), (include_str!("samples/npm-audit.json"), 1));
        assert_eq!(r.len(), 10, "every package once: {r:?}");
        assert_eq!(*picked(&r, "lodash"), ("lodash".into(), "4.17.20".into(), Some("4.18.1".into()), true, None, 5));
        assert_eq!(*picked(&r, "semver"), ("semver".into(), "7.5.4".into(), Some("7.8.5".into()), true, Some("dev".into()), 0));
        assert!(!picked(&r, "color-name").3, "brought by chalk");
        let found = parse_npm_audit(&out(1, include_str!("samples/npm-audit.json"))).unwrap();
        let a = &found.iter().find(|f| f.advisory.id == "GHSA-35jh-r3h4-6jhm").unwrap().advisory;
        assert_eq!((a.risk, a.title.as_str(), a.fixed_in.as_deref()), (Risk::High, "Command Injection in lodash", Some("4.17.21")));
        assert_eq!(found.iter().find(|f| f.advisory.id == "GHSA-r5fr-rjxr-66jc").unwrap().advisory.fixed_in, None, "<=4.17.23 says no fix");
        assert!(parse_npm_audit(&out(1, r#"{"error": {"code": "ENOLOCK", "summary": "This command requires an existing lockfile."}}"#)).unwrap_err().contains("lockfile"));
    }

    #[test]
    fn pnpm_reads_npms_tree_and_the_old_audit_format() {
        let m = js(Manager::Pnpm);
        let r = rows(&m, include_str!("samples/pnpm-list.json"), 0, Some((include_str!("samples/pnpm-outdated.json"), 1)), (include_str!("samples/pnpm-audit.json"), 1));
        assert_eq!(r.len(), 10, "{r:?}");
        assert_eq!(*picked(&r, "chalk"), ("chalk".into(), "4.1.2".into(), Some("6.0.1".into()), true, None, 0));
        assert_eq!(picked(&r, "lodash").5, 5);
        let found = parse_pnpm_audit(&out(1, include_str!("samples/pnpm-audit.json"))).unwrap();
        let f = found.iter().find(|f| f.advisory.id == "GHSA-35jh-r3h4-6jhm").unwrap();
        assert_eq!((f.version.as_deref(), f.advisory.fixed_in.as_deref()), (Some("4.17.20"), Some("4.17.21")), "patched_versions' >=");
    }

    #[test]
    fn yarn_reads_its_json_lines() {
        let m = js(Manager::Yarn);
        let r = rows(&m, include_str!("samples/yarn1-list.json"), 0, Some((include_str!("samples/yarn1-outdated.json"), 1)), (include_str!("samples/yarn1-audit.json"), 12));
        assert_eq!(r.len(), 10, "{r:?}");
        assert_eq!(*picked(&r, "semver"), ("semver".into(), "7.5.4".into(), Some("7.8.5".into()), true, Some("dev".into()), 0));
        assert_eq!(picked(&r, "lodash").5, 5);
        assert!(parse_yarn_outdated(&out(0, "")).unwrap().is_empty());
    }

    #[test]
    fn berry_lists_and_audits_but_has_no_outdated() {
        let m = js(Manager::Berry);
        assert!(m.outdated().is_none());
        let r = rows(&m, include_str!("samples/berry-info.json"), 0, None, (include_str!("samples/berry-audit.json"), 1));
        assert_eq!(r.len(), 10, "the workspace left out: {r:?}");
        assert_eq!(*picked(&r, "lodash"), ("lodash".into(), "4.17.20".into(), None, true, None, 5));
        assert_eq!(m.upgrade_all(&[]), None);
    }

    #[test]
    fn bun_reads_its_tables_and_its_audit() {
        let m = js(Manager::Bun);
        let r = rows(&m, include_str!("samples/bun-ls.txt"), 0, Some((include_str!("samples/bun-outdated.txt"), 0)), (include_str!("samples/bun-audit.json"), 1));
        assert_eq!(r.len(), 10, "{r:?}");
        assert_eq!(*picked(&r, "semver"), ("semver".into(), "7.5.4".into(), Some("7.8.5".into()), true, Some("dev".into()), 0), "`semver (dev)` is semver");
        assert_eq!(picked(&r, "lodash").5, 5);
        assert_eq!(parse_bun_outdated(&out(0, "bun outdated v1.4.4\n")).unwrap(), Vec::new());
    }

    #[test]
    fn each_manager_adds_removes_and_upgrades_its_way() {
        let lodash = Dependency::new("lodash", "4.17.20");
        let semver = Dependency::new("semver", "7.5.4").group("dev");
        let color = Dependency::new("color-name", "1.1.4").transitive();
        let l = |c: Option<Command>| c.map(|c| line(&c)).unwrap_or_default();
        let npm = js(Manager::Npm);
        assert_eq!(l(npm.upgrade(&lodash)), "npm install lodash@latest");
        assert_eq!(l(npm.upgrade(&semver)), "npm install semver@latest --save-dev", "kept in its group");
        assert_eq!(l(npm.upgrade(&color)), "npm update color-name");
        assert_eq!(l(npm.upgrade_all(&[&lodash])), "npm update");
        assert_eq!(line(&npm.add("vitest -D")), "npm install vitest -D");
        assert_eq!(l(npm.remove(&lodash)), "npm uninstall lodash");
        assert_eq!(npm.remove(&color), None, "the project doesn't ask for it");
        assert_eq!(l(npm.search("zod")), "npm search --json --searchlimit 20 zod");
        let pnpm = js(Manager::Pnpm);
        assert_eq!(l(pnpm.upgrade(&semver)), "pnpm add semver@latest --dev");
        assert_eq!(line(&pnpm.add("zod@3")), "pnpm add zod@3");
        assert_eq!(l(pnpm.remove(&lodash)), "pnpm remove lodash");
        let yarn = js(Manager::Yarn);
        assert_eq!(l(yarn.upgrade(&color)), "yarn upgrade color-name");
        assert_eq!(l(yarn.upgrade_all(&[])), "yarn upgrade");
        let berry = js(Manager::Berry);
        assert_eq!(l(berry.upgrade(&semver)), "yarn up semver@latest", "yarn up keeps its group");
        assert_eq!(l(berry.upgrade(&color)), "yarn up -R color-name");
        assert_eq!(l(berry.upgrade_all(&[&lodash, &semver])), "yarn up -R lodash semver");
        let bun = js(Manager::Bun);
        assert_eq!(l(bun.upgrade(&semver)), "bun add semver@latest --dev");
        assert_eq!(l(bun.upgrade_all(&[])), "bun update");
        assert_eq!(npm.lockfiles(), ["/project/package.json", "/project/package-lock.json"]);
        let found = parse_search(&out(0, include_str!("samples/npm-search.json"))).unwrap();
        assert_eq!((found[0].name.as_str(), found[0].version.as_str()), ("zod", "4.6.7"));
    }

    #[test]
    fn ranges_say_their_fix_and_names_their_version() {
        assert_eq!(fixed_in("<4.17.21").as_deref(), Some("4.17.21"));
        assert_eq!(fixed_in(">=4.0.0 <4.17.21").as_deref(), Some("4.17.21"));
        assert_eq!(fixed_in(">=4.17.23").as_deref(), Some("4.17.23"));
        assert_eq!(fixed_in("<=4.17.23"), None);
        assert_eq!(name_version("@types/node@22.10.1"), Some(("@types/node".into(), "22.10.1".into())));
        assert_eq!(name_version("lodash@npm:4.17.20"), Some(("lodash".into(), "4.17.20".into())));
        assert_eq!(name_version("@types"), None);
        let p = Project::parse(PACKAGE, &[]).unwrap();
        assert_eq!(mark(Dependency::new("react-dom", "19.0.0"), &p).group.as_deref(), Some("peer"));
    }
}
