//! The package manager page's Go modules: `go list -m -json all` lists
//! the build list and `-u` its updates, govulncheck finds the
//! vulnerable, and `go get` upgrades (`pkg@version`), adds and removes
//! (`pkg@none`).
//!
//! Which modules the project's own code asks for is read from `go.mod`
//! each time: a `require` without `// indirect`. `go list` says
//! `Indirect` too, but only for what's in go.mod.

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Advisory, Dependency, Finding, Output, PackageManager, Risk, Update, Version};

use crate::project::GoMod;

/// govulncheck's name for Go's own packages.
pub const STDLIB: &str = "stdlib";

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// The modules `go list -m -json` printed, the main ones left out.
fn modules(out: &Output) -> Result<Vec<Value>, String> {
    let all = out.json_stream();
    if all.is_empty() {
        return Err(if out.ok() { "go list printed nothing".into() } else { out.why() });
    }
    Ok(all.into_iter().filter(|m| m.get("Main").and_then(Value::as_bool) != Some(true) && text(m, "Path").is_some()).collect())
}

/// The version a `replace` puts in place of a module's; none for one
/// replaced by a folder.
fn replaced(m: &Value) -> Option<&str> {
    m.get("Replace").and_then(|r| text(r, "Version"))
}

/// `go list -m -json all`: the build list, each marked with whether the
/// module's own code asks for it (go.mod, else what go list says). A
/// replaced one is in the group `replaced`, at the version that replaces
/// it.
pub fn parse_list(out: &Output, go_mod: Option<&GoMod>) -> Result<Vec<Dependency>, String> {
    Ok(modules(out)?
        .iter()
        .map(|m| {
            let path = text(m, "Path").unwrap_or_default();
            let version = replaced(m).or(text(m, "Version")).unwrap_or_default();
            let direct = go_mod.and_then(|g| g.direct(path)).unwrap_or(m.get("Indirect").and_then(Value::as_bool) != Some(true));
            let d = Dependency::new(path, version).direct(direct);
            if m.get("Replace").is_some() {
                d.group("replaced")
            } else {
                d
            }
        })
        .collect())
}

/// `go list -m -u -json all`: each with an update, the newest. A replaced
/// one is left out: go get doesn't move a `replace`.
pub fn parse_outdated(out: &Output) -> Result<Vec<Update>, String> {
    Ok(modules(out)?
        .iter()
        .filter(|m| m.get("Replace").is_none())
        .filter_map(|m| {
            let latest = m.get("Update").and_then(|u| text(u, "Version"))?;
            Some(Update::new(text(m, "Path")?).current(text(m, "Version").unwrap_or_default()).latest(latest))
        })
        .collect())
}

/// How far a finding reaches into the code, as govulncheck traces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reach {
    Required,
    Imported,
    Called,
}

/// One vulnerability found in one module.
struct Hit {
    osv: String,
    module: String,
    version: Option<String>,
    fixed: Option<String>,
    reach: Reach,
    /// Where the code calls it: `main.go:14`.
    at: Option<String>,
}

/// `govulncheck -json ./...`: an advisory for each vulnerability in each
/// module, saying how far it reaches -- a function the code calls (and
/// where), a package it imports, or only a module it requires. Go's
/// database gives no severity, so each is of unknown risk. Go's own
/// packages are `stdlib`, at the Go that built it.
pub fn parse_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let all = out.json_stream();
    let mut osv: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut found: Vec<Hit> = Vec::new();
    for v in &all {
        if let Some(entry) = v.get("osv") {
            let id = text(entry, "id").unwrap_or_default().to_string();
            let summary = text(entry, "summary").unwrap_or_default().to_string();
            let aliases = entry.get("aliases").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            osv.push((id, summary, aliases));
        }
        let Some(f) = v.get("finding") else { continue };
        let Some(trace) = f.get("trace").and_then(Value::as_array).filter(|t| !t.is_empty()) else { continue };
        let (Some(id), Some(module)) = (text(f, "osv"), text(&trace[0], "module")) else { continue };
        let reach = if trace[0].get("function").is_some() {
            Reach::Called
        } else if trace[0].get("package").is_some() {
            Reach::Imported
        } else {
            Reach::Required
        };
        // Where the module's own code calls in: the trace's last frame.
        let at = (reach == Reach::Called).then(|| trace.last().and_then(|t| t.get("position"))).flatten().and_then(|p| Some(format!("{}:{}", text(p, "filename")?, p.get("line")?.as_u64()?)));
        let version = text(&trace[0], "version").map(str::to_string);
        let fixed = text(f, "fixed_version").map(str::to_string);
        match found.iter_mut().find(|h| h.osv == id && h.module == module) {
            Some(known) if known.reach < reach => {
                known.reach = reach;
                known.at = at;
            }
            Some(_) => {}
            None => found.push(Hit { osv: id.into(), module: module.into(), version, fixed, reach, at }),
        }
    }
    if found.is_empty() && !out.ok() {
        return Err(out.why());
    }
    if all.is_empty() {
        return Err(if out.ok() { "govulncheck printed nothing".into() } else { out.why() });
    }
    Ok(found
        .into_iter()
        .map(|Hit { osv: id, module, version, fixed, reach, at }| {
            let (summary, aliases) = osv.iter().find(|(i, ..)| *i == id).map(|(_, s, a)| (s.clone(), a.clone())).unwrap_or_default();
            let mut title = match aliases.iter().find(|a| a.starts_with("CVE-")) {
                Some(cve) if !summary.is_empty() => format!("{cve}: {summary}"),
                Some(cve) => cve.clone(),
                None if !summary.is_empty() => summary,
                None => id.clone(),
            };
            title.push_str(&match (reach, at) {
                (Reach::Called, Some(at)) => format!(" (called at {at})"),
                (Reach::Called, None) => " (called)".into(),
                (Reach::Imported, _) => " (imported, not called)".into(),
                (Reach::Required, _) => " (required, not imported)".into(),
            });
            let mut advisory = Advisory::new(id.as_str(), Risk::Unknown).title(title).url(format!("https://pkg.go.dev/vuln/{id}"));
            if let Some(fixed) = fixed {
                advisory = advisory.fixed_in(fixed);
            }
            Finding::new(module.as_str(), version.as_deref(), advisory)
        })
        .collect())
}

/// The version to move `dep` to: its latest, else the newest that fixes
/// its advisories, else go get's `latest`.
pub fn target(dep: &Dependency) -> String {
    if let Some(latest) = &dep.latest {
        return latest.clone();
    }
    dep.advisories.iter().filter_map(|a| a.fixed_in.as_deref()).filter_map(|f| Version::parse(f).map(|v| (v, f))).max().map(|(_, f)| f.to_string()).unwrap_or_else(|| "latest".into())
}

/// A module, as Go's `go.mod` and `go get` see it.
pub struct GoModules {
    pub go: Command,
    pub govulncheck: Option<Command>,
    /// The project's folder, `/project`.
    pub root: String,
}

impl GoModules {
    fn go_mod(&self) -> Option<GoMod> {
        std::fs::read_to_string(format!("{}/go.mod", self.root)).ok().map(|t| GoMod::parse(&t))
    }
}

impl PackageManager for GoModules {
    fn name(&self) -> &str {
        "go"
    }
    fn list(&self) -> Command {
        self.go.clone().args(["list", "-m", "-json", "all"])
    }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out, self.go_mod().as_ref())
    }
    fn outdated(&self) -> Option<Command> {
        Some(self.go.clone().args(["list", "-m", "-u", "-json", "all"]))
    }
    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_outdated(out)
    }
    fn audit(&self) -> Option<Command> {
        self.govulncheck.as_ref().map(|g| g.clone().args(["-json", "./..."]))
    }
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }
    /// `go get path@version`; Go itself (`stdlib`) by the toolchain line
    /// (`go get toolchain@go1.24.8`). A replaced one stays as its
    /// `replace` says.
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        if dep.group.as_deref() == Some("replaced") {
            return None;
        }
        let to = target(dep);
        if dep.name == STDLIB {
            return (to != "latest").then(|| self.go.clone().args(["get".to_string(), format!("toolchain@go{}", to.trim_start_matches('v'))]));
        }
        Some(self.go.clone().args(["get".to_string(), format!("{}@{to}", dep.name)]))
    }
    /// Each outdated one to its latest, in one `go get`.
    fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
        let each: Vec<String> = outdated.iter().filter(|d| d.name != STDLIB && d.group.as_deref() != Some("replaced")).filter_map(|d| d.latest.as_ref().map(|l| format!("{}@{l}", d.name))).collect();
        (!each.is_empty()).then(|| self.go.clone().arg("get").args(each))
    }
    /// `go get path`, or `path@version` as typed.
    fn add(&self, package: &str) -> Command {
        self.go.clone().args(["get", package.trim()])
    }
    /// One the module's code asks for, taken out of go.mod (`@none`).
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        (dep.direct && dep.name != STDLIB).then(|| self.go.clone().args(["get".to_string(), format!("{}@none", dep.name)]))
    }
    fn lockfiles(&self) -> Vec<String> {
        vec![format!("{}/go.mod", self.root), format!("{}/go.sum", self.root)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::GO_MOD;
    use crate::tests::line;
    use fenix_lang_kit::packages::merge;

    const LIST: &str = include_str!("samples/go-list-m.json");
    const UPDATES: &str = include_str!("samples/go-list-m-u.json");
    const AUDIT: &str = include_str!("samples/govulncheck.json");

    fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn manager() -> GoModules {
        GoModules { go: Command::new("go"), govulncheck: Some(Command::new("govulncheck")), root: "/nowhere".into() }
    }

    #[test]
    fn the_build_list_marks_what_go_mod_asks_for() {
        let go_mod = GoMod::parse(GO_MOD);
        let deps = parse_list(&ok(LIST), Some(&go_mod)).unwrap();
        let rows: Vec<(&str, &str, bool, Option<&str>)> = deps.iter().map(|d| (d.name.as_str(), d.current.as_str(), d.direct, d.group.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("github.com/google/uuid", "v1.3.0", false, None),
                ("github.com/spf13/pflag", "v1.0.3", true, Some("replaced")),
                ("golang.org/x/text", "v0.3.5", true, None),
                ("golang.org/x/tools", "v0.0.0-20180917221912-90fa682c2a6e", false, None),
                ("gopkg.in/check.v1", "v0.0.0-20161208181325-20d25e280405", false, None),
                ("gopkg.in/yaml.v2", "v2.2.2", true, None),
            ],
            "the main module left out"
        );
        // Without go.mod, go list's Indirect.
        let bare = parse_list(&ok(LIST), None).unwrap();
        assert!(bare.iter().find(|d| d.name == "golang.org/x/tools").is_some_and(|d| !d.direct));
        assert!(bare.iter().find(|d| d.name == "gopkg.in/yaml.v2").is_some_and(|d| d.direct));
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "go: updates to go.mod needed; to update it:\n\tgo mod tidy\n".into() };
        assert_eq!(parse_list(&failed, None).unwrap_err(), "go mod tidy");
    }

    #[test]
    fn updates_leave_a_replaced_module_alone() {
        let updates = parse_outdated(&ok(UPDATES)).unwrap();
        let rows: Vec<(&str, Option<&str>, Option<&str>)> = updates.iter().map(|u| (u.name.as_str(), u.current.as_deref(), u.latest.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("github.com/google/uuid", Some("v1.3.0"), Some("v1.6.0")),
                ("golang.org/x/text", Some("v0.3.5"), Some("v0.43.0")),
                ("golang.org/x/tools", Some("v0.0.0-20180917221912-90fa682c2a6e"), Some("v0.52.0")),
                ("gopkg.in/check.v1", Some("v0.0.0-20161208181325-20d25e280405"), Some("v1.0.0-20201130134442-10cb98267c6c")),
                ("gopkg.in/yaml.v2", Some("v2.2.2"), Some("v2.4.0")),
            ]
        );
    }

    #[test]
    fn govulncheck_says_how_far_each_reaches() {
        let findings = parse_audit(&ok(AUDIT)).unwrap();
        let rows: Vec<(&str, Option<&str>, &str, Option<&str>)> = findings.iter().map(|f| (f.name.as_str(), f.version.as_deref(), f.advisory.id.as_str(), f.advisory.fixed_in.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("gopkg.in/yaml.v2", Some("v2.2.2"), "GO-2020-0036", Some("v2.2.8")),
                ("gopkg.in/yaml.v2", Some("v2.2.2"), "GO-2021-0061", Some("v2.2.3")),
                ("gopkg.in/yaml.v2", Some("v2.2.2"), "GO-2022-0956", Some("v2.2.4")),
                ("golang.org/x/text", Some("v0.3.5"), "GO-2021-0113", Some("v0.3.7")),
                ("golang.org/x/text", Some("v0.3.5"), "GO-2022-1059", Some("v0.3.8")),
            ],
            "one for each vulnerability in each module; GO-2020-0015 was fixed before v0.3.5"
        );
        let title = |id: &str| findings.iter().find(|f| f.advisory.id == id).map(|f| f.advisory.title.clone()).unwrap_or_default();
        assert_eq!(title("GO-2021-0113"), "CVE-2021-38561: Out-of-bounds read in golang.org/x/text/language (called at main.go:14)");
        assert_eq!(title("GO-2022-1059"), "CVE-2022-32149: Denial of service via crafted Accept-Language header in golang.org/x/text/language (imported, not called)");
        assert_eq!(title("GO-2020-0036"), "CVE-2019-11254: Excessive resource consumption in YAML parsing in gopkg.in/yaml.v2 (called at main.go:19)");
        assert_eq!(findings[0].advisory.url.as_deref(), Some("https://pkg.go.dev/vuln/GO-2020-0036"));
        assert!(findings.iter().all(|f| f.advisory.risk == Risk::Unknown));

        // It couldn't load the packages: its stderr says why.
        let broken = Output { code: Some(1), stdout: "{\"config\":{\"protocol_version\":\"v1.0.0\"}}\n".into(), stderr: "govulncheck: loading packages: \n/home/u/orbit/internal/broken/broken.go:4:9: undefined: missing\n\nFor details on package patterns, see https://pkg.go.dev/cmd/go#hdr-Package_lists_and_patterns.\n".into() };
        assert!(parse_audit(&broken).unwrap_err().starts_with("For details on package patterns"));
        let stdlib = "{\"osv\":{\"id\":\"GO-2099-0001\",\"summary\":\"A flaw in net/http\",\"aliases\":[]}}\n{\"finding\":{\"osv\":\"GO-2099-0001\",\"fixed_version\":\"v1.24.8\",\"trace\":[{\"module\":\"stdlib\",\"version\":\"v1.24.7\",\"package\":\"net/http\"}]}}\n";
        let f = parse_audit(&ok(stdlib)).unwrap();
        assert_eq!((f[0].name.as_str(), f[0].advisory.title.as_str()), (STDLIB, "A flaw in net/http (imported, not called)"));
    }

    #[test]
    fn one_list_has_the_vulnerable_first() {
        let go_mod = GoMod::parse(GO_MOD);
        let m = manager();
        let all = merge(&m, parse_list(&ok(LIST), Some(&go_mod)).unwrap(), &parse_outdated(&ok(UPDATES)).unwrap(), &parse_audit(&ok(AUDIT)).unwrap());
        let top: Vec<&str> = all.iter().take(3).map(|d| d.name.as_str()).collect();
        assert_eq!(top, ["golang.org/x/text", "gopkg.in/yaml.v2", "golang.org/x/tools"], "the vulnerable, a major first; then the outdated");
        assert_eq!(all[1].advisories.len(), 3);
        assert_eq!(line(&m.upgrade(&all[1]).unwrap()), "go get gopkg.in/yaml.v2@v2.4.0");
    }

    #[test]
    fn go_get_upgrades_adds_and_removes() {
        let m = manager();
        let text = Dependency::new("golang.org/x/text", "v0.3.5").latest("v0.43.0");
        assert_eq!(line(&m.upgrade(&text).unwrap()), "go get golang.org/x/text@v0.43.0");
        let mut fixed = Dependency::new("golang.org/x/net", "v0.1.0");
        fixed.advisories = vec![Advisory::new("GO-1", Risk::Unknown).fixed_in("v0.7.0"), Advisory::new("GO-2", Risk::Unknown).fixed_in("v0.17.0")];
        assert_eq!(line(&m.upgrade(&fixed).unwrap()), "go get golang.org/x/net@v0.17.0", "the newest that fixes them");
        assert_eq!(line(&m.upgrade(&Dependency::new("x.io/y", "v1.0.0")).unwrap()), "go get x.io/y@latest");
        let mut go = Dependency::new(STDLIB, "v1.24.7");
        go.advisories = vec![Advisory::new("GO-3", Risk::Unknown).fixed_in("v1.24.8")];
        assert_eq!(line(&m.upgrade(&go).unwrap()), "go get toolchain@go1.24.8");
        assert_eq!(m.remove(&go), None);
        let pflag = Dependency::new("github.com/spf13/pflag", "v1.0.3").group("replaced").latest("v1.0.10");
        assert_eq!(m.upgrade(&pflag), None, "its replace says");
        let uuid = Dependency::new("github.com/google/uuid", "v1.3.0").latest("v1.6.0").transitive();
        assert_eq!(line(&m.upgrade_all(&[&text, &pflag, &uuid]).unwrap()), "go get golang.org/x/text@v0.43.0 github.com/google/uuid@v1.6.0");
        assert_eq!(m.upgrade_all(&[&pflag]), None);
        assert_eq!(line(&m.add(" github.com/spf13/cobra@v1.8.1")), "go get github.com/spf13/cobra@v1.8.1");
        assert_eq!(line(&m.remove(&text).unwrap()), "go get golang.org/x/text@none");
        assert_eq!(m.remove(&uuid), None, "an indirect one comes back with what needs it");
        assert_eq!(line(&m.list()), "go list -m -json all");
        assert_eq!(line(&m.outdated().unwrap()), "go list -m -u -json all");
        assert_eq!(line(&m.audit().unwrap()), "govulncheck -json ./...");
        assert_eq!(GoModules { govulncheck: None, ..manager() }.audit(), None);
        assert_eq!(m.lockfiles(), ["/nowhere/go.mod", "/nowhere/go.sum"]);
        assert_eq!(m.normalize("github.com/BurntSushi/toml"), "github.com/BurntSushi/toml", "module paths are case-sensitive");
    }
}
