//! The package manager page's pub: `pub deps --json` lists the packages
//! the project resolved, `pub outdated --json` what's newer and what
//! pub.dev has an advisory against, and `pub add`, `remove` and `upgrade`
//! change them. In a Flutter project they're `flutter pub ...`, which
//! resolves Flutter's own packages too.

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Advisory, Dependency, Finding, Output, PackageManager, Risk, Update};

/// pub, run as `tool pub ...`: `dart`, or `flutter`.
pub struct Pub {
    pub tool: Command,
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// The JSON object a pub command printed: pub may say something before
/// it (a lock it waits for, `Resolving dependencies...`).
fn object(out: &Output) -> Result<Value, String> {
    let Some(at) = out.stdout.find('{') else {
        return Err(if out.ok() { "pub printed nothing to read".into() } else { out.why() });
    };
    serde_json::from_str(out.stdout[at..].trim()).map_err(|e| format!("pub's JSON isn't what was expected: {e}"))
}

use fenix_lang_kit::serde_json;

/// `pub deps --json`: each package the project resolved, its direct
/// dependencies and dev dependencies marked as such, the transitive ones
/// not. SDK packages (`flutter`, `flutter_test`) have no version to
/// manage and are left out, and so is the project itself.
pub fn parse_list(out: &Output) -> Result<Vec<Dependency>, String> {
    let all = object(out)?;
    let packages = all.get("packages").and_then(Value::as_array).ok_or("pub deps printed no packages")?;
    Ok(packages
        .iter()
        .filter(|p| !matches!(text(p, "kind"), Some("root")) && text(p, "source") != Some("sdk"))
        .filter_map(|p| {
            let (name, version) = (text(p, "name")?, text(p, "version").unwrap_or_default());
            let d = Dependency::new(name, version);
            Some(match text(p, "kind") {
                Some("direct") => d,
                Some("dev") => d.group("dev"),
                _ => d.transitive(),
            })
        })
        .collect())
}

/// `pub outdated --json`: what `pub upgrade` would get (`wanted`) and
/// the newest there is (`latest`), for each package behind.
pub fn parse_outdated(out: &Output) -> Result<Vec<Update>, String> {
    let all = object(out)?;
    let packages = all.get("packages").and_then(Value::as_array).ok_or("pub outdated printed no packages")?;
    let version = |p: &Value, key: &str| p.get(key).and_then(|v| text(v, "version")).map(str::to_string);
    Ok(packages
        .iter()
        .filter_map(|p| {
            let current = version(p, "current");
            let (wanted, latest) = (version(p, "upgradable"), version(p, "latest"));
            if current == latest && current == wanted {
                return None;
            }
            let u = Update::new(text(p, "package")?).current(current.unwrap_or_default());
            let u = match wanted {
                Some(w) => u.wanted(w),
                None => u,
            };
            Some(match latest {
                Some(l) => u.latest(l),
                None => u,
            }
            .direct(text(p, "kind") != Some("transitive")))
        })
        .collect())
}

/// The same JSON read for what pub.dev has an advisory against
/// (`isCurrentAffectedByAdvisory`, which a newer pub writes: 3.14's does). pub
/// gives no score, so the risk is unknown; the advisory's page is
/// pub.dev's.
pub fn parse_audit(out: &Output) -> Result<Vec<Finding>, String> {
    let all = object(out)?;
    let packages = all.get("packages").and_then(Value::as_array).ok_or("pub outdated printed no packages")?;
    Ok(packages
        .iter()
        .filter(|p| p.get("isCurrentAffectedByAdvisory").and_then(Value::as_bool) == Some(true))
        .filter_map(|p| {
            let name = text(p, "package")?;
            let version = p.get("current").and_then(|c| text(c, "version"));
            let advisory = Advisory::new(format!("pub.dev/{name}"), Risk::Unknown).title("pub.dev lists a security advisory against this version").url(format!("https://pub.dev/packages/{name}/advisories"));
            Some(Finding::new(name, version, advisory))
        })
        .collect())
}

impl PackageManager for Pub {
    fn name(&self) -> &str {
        "pub"
    }

    fn list(&self) -> Command {
        self.tool.clone().args(["pub", "deps", "--json"])
    }

    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        parse_list(out)
    }

    fn outdated(&self) -> Option<Command> {
        Some(self.tool.clone().args(["pub", "outdated", "--json"]))
    }

    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_outdated(out)
    }

    fn audit(&self) -> Option<Command> {
        self.outdated()
    }

    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        parse_audit(out)
    }

    /// Past its constraint, to the newest the others allow: what a
    /// package's `latest` means. A transitive one stays inside what the
    /// packages that ask for it allow.
    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        let cmd = self.tool.clone().args(["pub", "upgrade"]);
        Some(if dep.direct { cmd.args(["--major-versions", &dep.name]) } else { cmd.arg(&dep.name) })
    }

    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        Some(self.tool.clone().args(["pub", "upgrade", "--major-versions"]))
    }

    /// `name`, or `name@^1.2.3`; `dev:name` for a dev dependency.
    fn add(&self, package: &str) -> Command {
        self.tool.clone().args(["pub", "add", package])
    }

    fn remove(&self, dep: &Dependency) -> Option<Command> {
        dep.direct.then(|| self.tool.clone().args(["pub", "remove", &dep.name]))
    }

    fn lockfiles(&self) -> Vec<String> {
        vec!["pubspec.lock".into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn pub_cmd() -> Pub {
        Pub { tool: Command::new("/sdk/bin/dart") }
    }

    #[test]
    fn the_resolved_packages_are_listed_and_the_ones_asked_for_marked() {
        let deps = parse_list(&said(include_str!("samples/pub-deps.json"))).unwrap();
        assert_eq!(deps.len(), 59, "everything but the project itself");
        let mut direct: Vec<(&str, &str, bool, Option<&str>)> = deps.iter().filter(|d| d.direct).map(|d| (d.name.as_str(), d.current.as_str(), d.direct, d.group.as_deref())).collect();
        direct.sort();
        assert_eq!(direct, [("http", "0.13.6", true, None), ("lints", "6.1.0", true, Some("dev")), ("mockito", "5.8.1", true, Some("dev")), ("path", "1.9.1", true, None), ("test", "1.33.0", true, Some("dev"))]);
        assert!(deps.iter().any(|d| d.name == "yaml" && !d.direct && d.current == "3.1.4"));
    }

    #[test]
    fn sdk_packages_are_not_managed() {
        let out = said(r#"{"root":"x","packages":[{"name":"x","version":"1.0.0","kind":"root","source":"root"},{"name":"flutter","version":"0.0.0","kind":"direct","source":"sdk"},{"name":"go_router","version":"14.2.0","kind":"direct","source":"hosted"}],"sdks":[],"executables":[]}"#);
        assert_eq!(parse_list(&out).unwrap().iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["go_router"]);
        assert!(parse_list(&said("Resolving dependencies...\n{\"packages\": []}")).unwrap().is_empty(), "past what pub said first");
        assert!(parse_list(&said("")).unwrap_err().contains("nothing"));
        let failed = Output { code: Some(65), stdout: String::new(), stderr: "Could not find a file named \"pubspec.yaml\"".into() };
        assert_eq!(parse_list(&failed).unwrap_err(), "Could not find a file named \"pubspec.yaml\"");
    }

    #[test]
    fn what_is_behind_has_what_pub_upgrade_gets_and_the_newest() {
        let updates = parse_outdated(&said(include_str!("samples/pub-outdated.json"))).unwrap();
        let rows: Vec<String> = updates.iter().map(|u| format!("{} {:?} {:?} {:?} {:?}", u.name, u.current, u.wanted, u.latest, u.direct)).collect();
        assert_eq!(rows, [r#"build Some("4.0.11") Some("4.0.11") Some("5.0.0") Some(false)"#, r#"built_collection Some("5.1.2") Some("5.1.2") Some("6.0.0") Some(false)"#, r#"http Some("0.13.6") Some("0.13.6") Some("1.6.0") Some(true)"#]);
        // A package that isn't resolved yet has no current version.
        let fresh = said(r#"{"packages":[{"package":"x","kind":"direct","current":null,"upgradable":{"version":"1.0.0"},"resolvable":{"version":"1.0.0"},"latest":{"version":"1.0.0"}}]}"#);
        assert_eq!(parse_outdated(&fresh).unwrap()[0].current, None);
        let current = said(r#"{"packages":[{"package":"x","kind":"direct","current":{"version":"1.0.0"},"upgradable":{"version":"1.0.0"},"resolvable":{"version":"1.0.0"},"latest":{"version":"1.0.0"}}]}"#);
        assert!(parse_outdated(&current).unwrap().is_empty());
    }

    /// pub's `isCurrentAffectedByAdvisory`: the recorded output has
    /// none, so one is written into it.
    #[test]
    fn an_advisory_pub_dev_has_is_a_finding() {
        assert!(parse_audit(&said(include_str!("samples/pub-outdated.json"))).unwrap().is_empty());
        let marked = include_str!("samples/pub-outdated.json").replacen("\"isCurrentAffectedByAdvisory\": false", "\"isCurrentAffectedByAdvisory\": true", 3);
        let found = parse_audit(&said(&marked)).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!((found[0].name.as_str(), found[0].version.as_deref()), ("build", Some("4.0.11")));
        assert_eq!(found[0].advisory.url.as_deref(), Some("https://pub.dev/packages/build/advisories"));
        assert_eq!(found[0].advisory.risk, Risk::Unknown);
    }

    #[test]
    fn each_action_is_a_pub_command() {
        let p = pub_cmd();
        let line = |c: Command| std::iter::once(c.program).chain(c.args).collect::<Vec<_>>().join(" ");
        assert_eq!(line(p.list()), "/sdk/bin/dart pub deps --json");
        assert_eq!(line(p.outdated().unwrap()), "/sdk/bin/dart pub outdated --json");
        let direct = Dependency::new("http", "0.13.6");
        let indirect = Dependency::new("build", "4.0.11").transitive();
        assert_eq!(line(p.upgrade(&direct).unwrap()), "/sdk/bin/dart pub upgrade --major-versions http");
        assert_eq!(line(p.upgrade(&indirect).unwrap()), "/sdk/bin/dart pub upgrade build");
        assert_eq!(line(p.upgrade_all(&[&direct]).unwrap()), "/sdk/bin/dart pub upgrade --major-versions");
        assert_eq!(line(p.add("collection@^1.18.0")), "/sdk/bin/dart pub add collection@^1.18.0");
        assert_eq!(line(p.remove(&direct).unwrap()), "/sdk/bin/dart pub remove http");
        assert!(p.remove(&indirect).is_none());
        assert_eq!(p.lockfiles(), ["pubspec.lock"]);
        let flutter = Pub { tool: Command::new("/f/bin/flutter") };
        assert_eq!(line(flutter.list()), "/f/bin/flutter pub deps --json");
    }
}
