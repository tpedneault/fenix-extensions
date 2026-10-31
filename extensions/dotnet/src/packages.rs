//! The package manager page's NuGet: `dotnet list package --format json`
//! for the solution, with `--include-transitive` (so a vulnerable package
//! one of yours brings in shows too), `--outdated` and `--vulnerable`;
//! `dotnet add package` and `dotnet remove package` on a project.
//!
//! A package is listed once for each project that has it, its project as
//! its group: an upgrade or a removal is that project's.

use fenix_extension_api::Command;
use fenix_lang_kit::{Advisory, Dependency, Finding, Output, PackageManager, Risk, Update};
use serde::Deserialize;

#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    projects: Vec<ListedProject>,
    #[serde(default)]
    problems: Vec<ListProblem>,
}

#[derive(Deserialize)]
struct ListProblem {
    #[serde(default)]
    level: String,
    text: String,
}

#[derive(Deserialize)]
struct ListedProject {
    path: String,
    #[serde(default)]
    frameworks: Vec<Framework>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Framework {
    #[serde(default)]
    top_level_packages: Vec<Listed>,
    #[serde(default)]
    transitive_packages: Vec<Listed>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    id: String,
    #[serde(default)]
    resolved_version: String,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    vulnerabilities: Vec<Vulnerability>,
}

#[derive(Deserialize)]
struct Vulnerability {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    advisoryurl: String,
}

/// A project's name from its path: `src/App/App.csproj` is `App`.
pub fn project_name(path: &str) -> String {
    let file = path.replace('\\', "/");
    let file = file.rsplit('/').next().unwrap_or(&file).to_string();
    file.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or(file)
}

/// The listing, or why there's none: a project not restored yet is
/// `dotnet list package`'s error, in its JSON.
fn listing(out: &Output) -> Result<Listing, String> {
    if out.stdout.trim().is_empty() {
        return Err(out.why());
    }
    let l: Listing = out.json()?;
    if let Some(p) = l.problems.iter().find(|p| p.level.eq_ignore_ascii_case("error")) {
        return Err(p.text.clone());
    }
    Ok(l)
}

/// Each (project, framework's package, direct) once: a package a project
/// has for two frameworks at one version is one row.
fn rows(l: Listing) -> Vec<(String, Listed, bool)> {
    let mut out: Vec<(String, Listed, bool)> = Vec::new();
    for p in l.projects {
        let project = project_name(&p.path);
        for f in p.frameworks {
            for (direct, packages) in [(true, f.top_level_packages), (false, f.transitive_packages)] {
                for pkg in packages {
                    if !out.iter().any(|(pr, q, _)| *pr == project && q.id.eq_ignore_ascii_case(&pkg.id) && q.resolved_version == pkg.resolved_version) {
                        out.push((project.clone(), pkg, direct));
                    }
                }
            }
        }
    }
    out
}

/// `dotnet list package --include-transitive --format json`.
pub fn parse_list(out: &Output) -> Result<Vec<Dependency>, String> {
    Ok(rows(listing(out)?).into_iter().map(|(project, p, direct)| Dependency::new(p.id, p.resolved_version).direct(direct).group(project)).collect())
}

/// `... --outdated`: each with its latest.
pub fn parse_outdated(out: &Output) -> Result<Vec<Update>, String> {
    Ok(rows(listing(out)?).into_iter().filter_map(|(_, p, _)| p.latest_version.filter(|l| !l.is_empty() && *l != p.resolved_version).map(|l| Update::new(p.id).current(p.resolved_version).latest(l))).collect())
}

/// `... --vulnerable`: an advisory for each, by its GitHub advisory's id,
/// with the severity NuGet gives it.
pub fn parse_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let mut found = Vec::new();
    for (_, p, _) in rows(listing(out)?) {
        for v in &p.vulnerabilities {
            let id = v.advisoryurl.trim_end_matches('/').rsplit('/').next().unwrap_or(&v.advisoryurl).to_string();
            let advisory = Advisory::new(id.as_str(), Risk::parse(&v.severity)).title(format!("{} severity vulnerability in {} {}", v.severity, p.id, p.resolved_version)).url(v.advisoryurl.as_str());
            found.push(Finding::new(p.id.as_str(), Some(&p.resolved_version), advisory));
        }
    }
    Ok(found)
}

/// What's typed to add: `Name`, `Name@1.2.3` or `Name 1.2.3`, as
/// `dotnet add package`'s arguments.
pub fn package_args(typed: &str) -> Vec<String> {
    let typed = typed.trim();
    let (name, version) = match typed.split_once(['@', ' ']) {
        Some((n, v)) if !v.trim().is_empty() => (n.trim(), Some(v.trim())),
        _ => (typed, None),
    };
    let mut args = vec!["package".to_string(), name.to_string()];
    if let Some(v) = version {
        args.extend(["--version".to_string(), v.to_string()]);
    }
    args
}

/// NuGet for the solution.
pub struct Nuget {
    pub dotnet: Command,
    /// What's listed: the solution, or the one project (`None`: the
    /// folder's).
    pub target: Option<String>,
    /// Each project's name and file.
    pub projects: Vec<(String, String)>,
    /// The project a package is added to: the one you're in, else the
    /// first that isn't a test project.
    pub add_to: Option<String>,
    /// Under `/project`: the files whose change makes the list stale.
    pub root: String,
    /// `Directory.Packages.props`, where a solution that manages its
    /// versions centrally keeps them.
    pub central: bool,
}

impl Nuget {
    fn list_with(&self, flags: &[&str]) -> Command {
        let mut cmd = self.dotnet.clone().arg("list");
        if let Some(t) = &self.target {
            cmd = cmd.arg(t);
        }
        cmd = cmd.args(["package", "--include-transitive"]).args(flags.iter().copied()).args(["--format", "json"]);
        cmd.env.push(("DOTNET_NOLOGO".into(), "1".into()));
        cmd
    }

    fn project(&self, name: Option<&str>) -> Option<&str> {
        let name = name?;
        self.projects.iter().find(|(n, _)| n == name).map(|(_, p)| p.as_str())
    }
}

impl PackageManager for Nuget {
    fn name(&self) -> &str {
        "NuGet"
    }
    fn list(&self) -> Command {
        self.list_with(&[])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out)
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.list_with(&["--outdated"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_outdated(out)
    }
    fn audit(&self) -> Option<Command> {
        Some(self.list_with(&["--vulnerable"]))
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }
    /// Its project asks for its latest, or for the version that fixes its
    /// advisories when there's no newer one known: a transitive one too,
    /// which NuGet's way of moving one up is to ask for it.
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        let project = self.project(dep.group.as_deref())?;
        let version = dep.latest.clone().or_else(|| dep.advisories.iter().filter_map(|a| a.fixed_in.clone()).next());
        let mut cmd = self.dotnet.clone().args(["add", project, "package", &dep.name]);
        if let Some(v) = version {
            cmd = cmd.args(["--version", &v]);
        }
        Some(cmd)
    }
    /// One package in one project: there's no command for several (`dotnet
    /// add package` takes one), so `U` is for when there's one to do.
    fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
        match outdated {
            [one] => self.upgrade(one),
            _ => None,
        }
    }
    fn add(&self, package: &str) -> Command {
        let mut cmd = self.dotnet.clone().arg("add");
        if let Some(p) = &self.add_to {
            cmd = cmd.arg(p);
        }
        cmd.args(package_args(package))
    }
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        let project = self.project(dep.group.as_deref()).filter(|_| dep.direct)?;
        Some(self.dotnet.clone().args(["remove", project, "package", &dep.name]))
    }
    /// NuGet's ids aren't case-sensitive.
    fn normalize(&self, name: &str) -> String {
        name.to_ascii_lowercase()
    }
    fn lockfiles(&self) -> Vec<String> {
        let mut files: Vec<String> = self.projects.iter().map(|(_, p)| format!("{}/{p}", self.root)).collect();
        files.extend(self.projects.iter().map(|(_, p)| format!("{}/{}packages.lock.json", self.root, p.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default())));
        files.push(format!("{}/Directory.Build.props", self.root));
        if self.central {
            files.push(format!("{}/Directory.Packages.props", self.root));
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    const LIST: &str = include_str!("samples/list-package.json");
    const OUTDATED: &str = include_str!("samples/list-package-outdated.json");
    const VULNERABLE: &str = include_str!("samples/list-package-vulnerable.json");
    const NO_ASSETS: &str = include_str!("samples/list-package-no-assets.json");

    fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn nuget() -> Nuget {
        Nuget {
            dotnet: Command::new("dotnet"),
            target: Some("Orbit.sln".into()),
            projects: vec![("Orbit.App".into(), "src/Orbit.App/Orbit.App.csproj".into()), ("Orbit.Tests".into(), "tests/Orbit.Tests/Orbit.Tests.csproj".into())],
            add_to: Some("src/Orbit.App/Orbit.App.csproj".into()),
            root: "/project".into(),
            central: false,
        }
    }

    #[test]
    fn the_list_is_each_projects_packages_and_what_they_bring() {
        let deps = parse_list(&ok(LIST)).unwrap();
        let get = |n: &str, g: &str| deps.iter().find(|d| d.name == n && d.group.as_deref() == Some(g)).unwrap_or_else(|| panic!("{n} in {g}"));
        assert_eq!(get("Newtonsoft.Json", "Orbit.App").current, "12.0.1");
        assert!(get("Newtonsoft.Json", "Orbit.App").direct);
        assert!(get("xunit", "Orbit.Tests").direct);
        assert!(!get("System.Net.Http", "Orbit.Tests").direct, "brought by the test SDK");
        assert!(deps.iter().all(|d| d.group.is_some()));
        assert!(!deps.iter().any(|d| d.group.as_deref() == Some("Orbit.Core")), "a project with none has no rows");
        let err = parse_list(&ok(NO_ASSETS)).unwrap_err();
        assert!(err.starts_with("No assets file was found for `/home/u/norestore/norestore.csproj`. Please run restore"), "{err}");
        assert!(parse_list(&Output { code: Some(1), stderr: "error: MSB1011".into(), ..Output::default() }).unwrap_err().contains("MSB1011"));
    }

    #[test]
    fn outdated_and_vulnerable_make_one_list() {
        let updates = parse_outdated(&ok(OUTDATED)).unwrap();
        let json = updates.iter().find(|u| u.name == "Newtonsoft.Json").unwrap();
        assert_eq!((json.current.as_deref(), json.latest.as_deref()), (Some("12.0.1"), Some("13.0.4")));
        let findings = parse_audit(&ok(VULNERABLE)).unwrap();
        let f = &findings[0];
        assert_eq!((f.name.as_str(), f.version.as_deref(), f.advisory.id.as_str(), f.advisory.risk), ("Newtonsoft.Json", Some("12.0.1"), "GHSA-5crp-9r3c-p9vr", Risk::High));
        assert_eq!(f.advisory.url.as_deref(), Some("https://github.com/advisories/GHSA-5crp-9r3c-p9vr"));
        assert!(findings.iter().any(|f| f.name == "System.Net.Http"), "a transitive one too");
        let all = merge(&nuget(), parse_list(&ok(LIST)).unwrap(), &updates, &findings);
        assert_eq!(all[0].name, "Newtonsoft.Json", "the vulnerable first");
        assert!(all[0].is_outdated() && all[0].is_vulnerable());
    }

    #[test]
    fn upgrades_adds_and_removes_are_a_projects() {
        let n = nuget();
        assert_eq!(line(&n.list()), "dotnet list Orbit.sln package --include-transitive --format json");
        assert_eq!(line(&n.audit().unwrap()), "dotnet list Orbit.sln package --include-transitive --vulnerable --format json");
        let json = Dependency::new("Newtonsoft.Json", "12.0.1").latest("13.0.4").group("Orbit.App");
        assert_eq!(line(&n.upgrade(&json).unwrap()), "dotnet add src/Orbit.App/Orbit.App.csproj package Newtonsoft.Json --version 13.0.4");
        assert_eq!(line(&n.upgrade_all(&[&json]).unwrap()), line(&n.upgrade(&json).unwrap()));
        let http = Dependency { advisories: vec![Advisory::new("GHSA-7jgj-8wvc-jh57", Risk::High).fixed_in("4.3.4")], ..Dependency::new("System.Net.Http", "4.3.0").transitive().group("Orbit.Tests") };
        assert_eq!(line(&n.upgrade(&http).unwrap()), "dotnet add tests/Orbit.Tests/Orbit.Tests.csproj package System.Net.Http --version 4.3.4", "asked for, at its fix");
        assert_eq!(n.upgrade_all(&[&json, &http]), None, "one at a time");
        assert_eq!(n.remove(&http), None, "brought by another");
        assert_eq!(line(&n.remove(&json).unwrap()), "dotnet remove src/Orbit.App/Orbit.App.csproj package Newtonsoft.Json");
        assert_eq!(line(&n.add("Serilog@4.1.0")), "dotnet add src/Orbit.App/Orbit.App.csproj package Serilog --version 4.1.0");
        assert_eq!(line(&n.add(" Polly ")), "dotnet add src/Orbit.App/Orbit.App.csproj package Polly");
        assert_eq!(n.normalize("XUnit"), "xunit");
        assert!(n.lockfiles().contains(&"/project/tests/Orbit.Tests/packages.lock.json".to_string()));
        assert_eq!(project_name("src\\A\\A.fsproj"), "A");
    }
}
