//! The package manager page's Python: uv for a project uv manages (`uv
//! add`, `uv remove`, `uv sync --upgrade`), and for any other, the venv's
//! pip -- `uv pip` when there's uv, else Python's own. Both list with
//! `pip list --format json` and its `--outdated`; `pip-audit` audits.
//!
//! Which dependencies the project asks for, and in which group, is read
//! from `pyproject.toml` (or its requirements files) each time: `pip
//! list` doesn't say.

use std::path::Path;

use fenix_extension_api::Command;
use fenix_lang_kit::packages::python_name;
use fenix_lang_kit::{Advisory, Dependency, Finding, Output, PackageManager, Risk, Update, Version};
use serde::Deserialize;

use crate::project::Project;

#[derive(Deserialize)]
struct Listed {
    name: String,
    version: String,
    #[serde(default)]
    latest_version: Option<String>,
}

/// `pip list --format json` (or uv's): what's installed, the project's
/// own package left out, each marked with whether the project asks for
/// it and in which group. When the project names none, all are its.
pub fn parse_list(out: &Output, project: &Project) -> Result<Vec<Dependency>, String> {
    let rows: Vec<Listed> = out.json()?;
    let own = project.own_name();
    Ok(rows
        .into_iter()
        .filter(|r| own.as_deref() != Some(python_name(&r.name).as_str()))
        .map(|r| {
            let d = Dependency::new(r.name.as_str(), r.version.as_str());
            match project.group_of(&r.name) {
                _ if project.deps.is_empty() => d,
                None => d.transitive(),
                Some(None) => d,
                Some(Some(g)) => d.group(g),
            }
        })
        .collect())
}

/// `pip list --outdated --format json`: each with its latest.
pub fn parse_outdated(out: &Output) -> Result<Vec<Update>, String> {
    let rows: Vec<Listed> = out.json()?;
    Ok(rows.into_iter().map(|r| Update::new(r.name).current(r.version).latest(r.latest_version.unwrap_or_default())).collect())
}

#[derive(Deserialize)]
struct Audit {
    dependencies: Vec<Audited>,
}

#[derive(Deserialize)]
struct Audited {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    vulns: Vec<Vuln>,
}

#[derive(Deserialize)]
struct Vuln {
    id: String,
    #[serde(default)]
    fix_versions: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    description: String,
}

/// A description's first sentence, or line, short enough for a row.
fn title(description: &str) -> String {
    let text = description.trim().trim_start_matches('#').trim();
    let first = text.lines().next().unwrap_or("").trim();
    let first = first.split(". ").next().unwrap_or(first).trim_end_matches('.');
    if first.chars().count() > 100 {
        format!("{}…", first.chars().take(99).collect::<String>().trim_end())
    } else {
        first.to_string()
    }
}

/// `pip-audit --format json`: an advisory for each of a package's
/// vulnerabilities. It exits 1 when it found some; it says no severity,
/// so each is of unknown risk. A package it skipped (the project's own,
/// not on PyPI) has none.
pub fn parse_audit(out: &Output) -> Result<Vec<Finding>, String> {
    if out.stdout.trim().is_empty() {
        return Err(out.why());
    }
    let audit: Audit = out.json()?;
    let mut findings = Vec::new();
    for dep in audit.dependencies {
        for v in dep.vulns {
            // The lowest version that fixes it; the CVE, if it has one,
            // is what most people know it by.
            let fixed = v.fix_versions.iter().filter_map(|f| Version::parse(f).map(|v| (v, f))).min().map(|(_, f)| f.clone());
            let mut title = title(&v.description);
            if let Some(cve) = v.aliases.iter().find(|a| a.starts_with("CVE-")) {
                title = if title.is_empty() { cve.clone() } else { format!("{cve}: {title}") };
            }
            if title.is_empty() {
                title.clone_from(&v.id);
            }
            let advisory = Advisory::new(v.id.as_str(), Risk::Unknown).title(title).fixed_in(fixed.unwrap_or_default()).url(format!("https://osv.dev/vulnerability/{}", v.id));
            findings.push(Finding::new(dep.name.as_str(), dep.version.as_deref(), advisory));
        }
    }
    Ok(findings)
}

/// The venv's `site-packages` under `root`, relative to it: what
/// pip-audit audits (`--path`), so it needs no pip in the venv (uv's have
/// none).
pub fn site_packages(root: &str) -> Option<String> {
    let venv = Path::new(root).join(".venv");
    if venv.join("Lib/site-packages").is_dir() {
        return Some(".venv/Lib/site-packages".into());
    }
    let mut libs: Vec<String> = std::fs::read_dir(venv.join("lib")).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("python") && venv.join("lib").join(n).join("site-packages").is_dir()).collect();
    libs.sort();
    libs.pop().map(|n| format!(".venv/lib/{n}/site-packages"))
}

/// pip-audit run by uv on the venv in `root`, when there is one.
fn uv_audit(uv: &Command, root: &str) -> Option<Command> {
    let site = site_packages(root)?;
    Some(uv.clone().args(["tool", "run", "pip-audit", "--path", &site, "--format", "json", "--progress-spinner", "off"]))
}

/// What's typed to add, as a requirement: `name@1.2` is `name==1.2`.
pub fn requirement(typed: &str) -> String {
    let typed = typed.trim();
    match typed.split_once('@') {
        Some((name, version)) if !name.contains(' ') && !version.trim().is_empty() && !version.contains('/') => format!("{}=={}", name.trim(), version.trim()),
        _ => typed.to_string(),
    }
}

/// `uv add`'s and `uv remove`'s flags for a dependency's group.
pub fn group_flags(group: Option<&str>) -> Vec<String> {
    match group {
        None => Vec::new(),
        Some("dev") => vec!["--dev".into()],
        Some(g) => match g.strip_prefix('[').and_then(|g| g.strip_suffix(']')) {
            Some(extra) => vec!["--optional".into(), extra.into()],
            None => vec!["--group".into(), g.into()],
        },
    }
}

/// A project uv manages: its lockfile changed by `uv add`, `uv remove`
/// and `uv sync`.
pub struct Uv {
    pub uv: Command,
    /// The project's folder, `/project`.
    pub root: String,
}

impl PackageManager for Uv {
    fn name(&self) -> &str {
        "uv"
    }
    fn list(&self) -> Command {
        self.uv.clone().args(["pip", "list", "--format", "json"])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out, &Project::read(&self.root))
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.uv.clone().args(["pip", "list", "--outdated", "--format", "json"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_outdated(out)
    }
    fn audit(&self) -> Option<Command> {
        uv_audit(&self.uv, &self.root)
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }
    /// One the project asks for is asked for at its latest (`uv add
    /// 'name>=latest'`, in its group); another is moved up in the lock as
    /// far as what asks for it lets it.
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        Some(match (&dep.latest, dep.direct) {
            (Some(latest), true) => self.uv.clone().arg("add").args(group_flags(dep.group.as_deref())).arg(format!("{}>={latest}", dep.name)),
            _ => self.uv.clone().args(["sync", "--upgrade-package", &dep.name]),
        })
    }
    /// All of them, as far as the project's requirements let them.
    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        Some(self.uv.clone().args(["sync", "--upgrade"]))
    }
    fn add(&self, package: &str) -> Command {
        self.uv.clone().args(["add".to_string(), requirement(package)])
    }
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        dep.direct.then(|| self.uv.clone().arg("remove").args(group_flags(dep.group.as_deref())).arg(&dep.name))
    }
    fn normalize(&self, name: &str) -> String {
        python_name(name)
    }
    fn lockfiles(&self) -> Vec<String> {
        vec![format!("{}/uv.lock", self.root), format!("{}/pyproject.toml", self.root)]
    }
}

/// A project uv doesn't manage: its venv's packages, installed and
/// removed with `uv pip` when there's uv, else with Python's pip (the
/// Python on `PATH`: the venv's when it's active). Requirements files
/// are left as they are.
pub struct Pip {
    /// `uv pip`, or `python3 -m pip`.
    pub pip: Command,
    pub uv: Option<Command>,
    pub python: Command,
    pub root: String,
}

impl Pip {
    pub fn new(uv: Option<Command>, python: Command, root: &str) -> Pip {
        let pip = match &uv {
            Some(uv) => uv.clone().arg("pip"),
            None => python.clone().args(["-m", "pip"]),
        };
        Pip { pip, uv, python, root: root.into() }
    }
}

impl PackageManager for Pip {
    fn name(&self) -> &str {
        if self.uv.is_some() {
            "uv pip"
        } else {
            "pip"
        }
    }
    fn list(&self) -> Command {
        self.pip.clone().args(["list", "--format", "json"])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out, &Project::read(&self.root))
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.pip.clone().args(["list", "--outdated", "--format", "json"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_outdated(out)
    }
    /// uv's pip-audit on the venv; without uv, the one installed in that
    /// Python (`pip install pip-audit`).
    fn audit(&self) -> Option<Command> {
        match &self.uv {
            Some(uv) => uv_audit(uv, &self.root),
            None => Some(self.python.clone().args(["-m", "pip_audit", "--format", "json", "--progress-spinner", "off"])),
        }
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        Some(self.pip.clone().args(["install", "--upgrade", &dep.name]))
    }
    fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
        (!outdated.is_empty()).then(|| self.pip.clone().args(["install", "--upgrade"]).args(outdated.iter().map(|d| d.name.clone())))
    }
    fn add(&self, package: &str) -> Command {
        self.pip.clone().args(["install".to_string(), requirement(package)])
    }
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        // pip asks first; uv doesn't.
        let yes = if self.uv.is_some() { None } else { Some("-y") };
        Some(self.pip.clone().arg("uninstall").args(yes).arg(&dep.name))
    }
    fn normalize(&self, name: &str) -> String {
        python_name(name)
    }
    fn lockfiles(&self) -> Vec<String> {
        ["requirements.txt", "requirements-dev.txt", "pyproject.toml"].iter().map(|f| format!("{}/{f}", self.root)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, PYPROJECT};
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    const LIST: &str = include_str!("samples/uv-pip-list.json");
    const OUTDATED: &str = include_str!("samples/uv-pip-list-outdated.json");
    const AUDIT: &str = include_str!("samples/pip-audit.json");

    fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn uv(root: &str) -> Uv {
        Uv { uv: Command::new("uv"), root: root.into() }
    }

    #[test]
    fn the_list_marks_what_the_project_asks_for() {
        let project = Project::parse(PYPROJECT).unwrap();
        let deps = parse_list(&ok(LIST), &project).unwrap();
        let get = |n: &str| deps.iter().find(|d| d.name == n).unwrap();
        assert!(!deps.iter().any(|d| d.name == "orbit-api"), "the project's own package isn't one");
        assert!(get("fastapi").direct && get("fastapi").group.is_none());
        assert_eq!(get("pytest").group.as_deref(), Some("dev"));
        assert_eq!(get("httpx").group.as_deref(), Some("[client]"));
        assert!(get("Pydantic-Settings").direct, "names compared in PEP 503's form");
        assert!(!get("starlette").direct);
        // A project that names none: everything is its.
        let none = parse_list(&ok(LIST), &Project::default()).unwrap();
        assert!(none.iter().all(|d| d.direct) && none.len() == deps.len() + 1);
        assert!(parse_list(&ok("not json"), &project).unwrap_err().contains("isn't what was expected"));
    }

    #[test]
    fn outdated_and_audit_make_one_list_with_the_vulnerable_first() {
        let project = Project::parse(PYPROJECT).unwrap();
        let updates = parse_outdated(&ok(OUTDATED)).unwrap();
        assert_eq!((updates[0].name.as_str(), updates[0].current.as_deref(), updates[0].latest.as_deref()), ("fastapi", Some("0.115.0"), Some("0.121.2")));
        let audit = Output { code: Some(1), ..ok(AUDIT) };
        let findings = parse_audit(&audit).unwrap();
        assert_eq!(findings.len(), 4);
        let jinja = &findings[0];
        assert_eq!((jinja.name.as_str(), jinja.version.as_deref(), jinja.advisory.id.as_str()), ("jinja2", Some("3.1.2"), "GHSA-h5c8-rqwp-cp95"));
        assert_eq!(jinja.advisory.fixed_in.as_deref(), Some("3.1.3"));
        assert_eq!(jinja.advisory.title, "CVE-2024-22195: The `xmlattr` filter in affected versions of Jinja accepts keys containing spaces");
        assert_eq!(jinja.advisory.url.as_deref(), Some("https://osv.dev/vulnerability/GHSA-h5c8-rqwp-cp95"));
        assert_eq!(findings[2].advisory.fixed_in.as_deref(), Some("1.26.17"), "the lowest that fixes it");
        assert_eq!(findings[3].advisory.title, "PYSEC-2099-1", "no description, no CVE: its id");

        let all = merge(&uv("/nowhere"), parse_list(&ok(LIST), &project).unwrap(), &updates, &findings);
        let top: Vec<&str> = all.iter().take(4).map(|d| d.name.as_str()).collect();
        assert_eq!(top, ["jinja2", "urllib3", "fastapi", "Pydantic-Settings"], "the vulnerable, then the outdated");
        assert_eq!(all[0].advisories.len(), 2);
        assert!(parse_audit(&Output { code: Some(2), stdout: String::new(), stderr: "error: no network\n".into() }).unwrap_err().contains("no network"));
    }

    #[test]
    fn titles_are_a_short_first_sentence() {
        assert_eq!(title("### Impact A specially crafted argument. More."), "Impact A specially crafted argument");
        assert_eq!(title(&"x".repeat(150)).chars().count(), 100);
        assert_eq!(title(""), "");
    }

    #[test]
    fn uv_changes_the_project_and_its_lock() {
        let m = uv("/project");
        let dev = Dependency::new("pytest", "8.0.0").latest("8.3.4").group("dev");
        assert_eq!(line(&m.upgrade(&dev).unwrap()), "uv add --dev pytest>=8.3.4");
        let extra = Dependency::new("httpx", "0.27.0").latest("0.28.1").group("[client]");
        assert_eq!(line(&m.upgrade(&extra).unwrap()), "uv add --optional client httpx>=0.28.1");
        let docs = Dependency::new("mkdocs", "1.5").group("docs");
        assert_eq!(line(&m.remove(&docs).unwrap()), "uv remove --group docs mkdocs");
        let deep = Dependency::new("idna", "3.6").latest("3.10").transitive();
        assert_eq!(line(&m.upgrade(&deep).unwrap()), "uv sync --upgrade-package idna");
        assert_eq!(m.remove(&deep), None, "one the project doesn't ask for isn't removed");
        assert_eq!(line(&m.upgrade_all(&[]).unwrap()), "uv sync --upgrade");
        assert_eq!(line(&m.add("rich@13.9.4")), "uv add rich==13.9.4");
        assert_eq!(line(&m.add("httpx[http2]>=0.27")), "uv add httpx[http2]>=0.27");
        assert_eq!(line(&m.add("pkg @ https://x/p.whl")), "uv add pkg @ https://x/p.whl");
        assert_eq!(m.lockfiles(), ["/project/uv.lock", "/project/pyproject.toml"]);
        assert_eq!(m.normalize("Typing_Extensions"), "typing-extensions");
    }

    #[test]
    fn pip_is_uvs_or_pythons() {
        let with_uv = Pip::new(Some(Command::new("uv")), Command::new("python3"), "/project");
        let bare = Pip::new(None, Command::new("python3"), "/project");
        assert_eq!((with_uv.name(), bare.name()), ("uv pip", "pip"));
        assert_eq!(line(&with_uv.list()), "uv pip list --format json");
        assert_eq!(line(&bare.outdated().unwrap()), "python3 -m pip list --outdated --format json");
        let d = Dependency::new("flask", "2.0").transitive();
        assert_eq!(line(&bare.remove(&d).unwrap()), "python3 -m pip uninstall -y flask");
        assert_eq!(line(&with_uv.remove(&d).unwrap()), "uv pip uninstall flask");
        let a = Dependency::new("a", "1");
        assert_eq!(line(&bare.upgrade_all(&[&a, &d]).unwrap()), "python3 -m pip install --upgrade a flask");
        assert_eq!(bare.upgrade_all(&[]), None);
        assert_eq!(line(&bare.audit().unwrap()), "python3 -m pip_audit --format json --progress-spinner off");
        assert_eq!(with_uv.audit(), None, "no venv to audit");
    }

    #[test]
    fn pip_audit_is_given_the_venvs_site_packages() {
        let dir = temp("site");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(site_packages(&root), None);
        std::fs::create_dir_all(dir.join(".venv/lib/python3.12/site-packages")).unwrap();
        std::fs::create_dir_all(dir.join(".venv/lib/python3.13/site-packages")).unwrap();
        assert_eq!(site_packages(&root).as_deref(), Some(".venv/lib/python3.13/site-packages"));
        assert_eq!(line(&uv(&root).audit().unwrap()), "uv tool run pip-audit --path .venv/lib/python3.13/site-packages --format json --progress-spinner off");
        std::fs::create_dir_all(dir.join(".venv/Lib/site-packages")).unwrap();
        assert_eq!(site_packages(&root).as_deref(), Some(".venv/Lib/site-packages"), "Windows' venv");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
