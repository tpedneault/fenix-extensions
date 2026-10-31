//! A project's dependencies as its package manager tells them: what each
//! pack implements ([`PackageManager`]), and what lang-kit makes of the
//! answers -- one list, the outdated and vulnerable first.

use std::cmp::Reverse;

use fenix_extension_api::Command;
use serde::{Deserialize, Serialize};

use crate::exec::Output;
use crate::version::{newer, Bump, Version};

/// How bad an advisory says it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Risk {
    #[default]
    Unknown,
    Low,
    Moderate,
    High,
    Critical,
}

impl Risk {
    /// As the tools say it: `low`, `moderate` or `medium`, `high`,
    /// `critical`, a CVSS score (`7.5`), or a CVSS 2 or 3 vector's score
    /// (`CVSS:3.1/AV:N/AC:L/...`, [`crate::cvss`]). A CVSS 4 vector is
    /// unknown: it's scored from tables, not a formula.
    pub fn parse(text: &str) -> Risk {
        let t = text.trim().to_ascii_lowercase();
        match t.as_str() {
            "low" | "info" | "informational" | "minor" => Risk::Low,
            "moderate" | "medium" | "mod" => Risk::Moderate,
            "high" | "important" | "major" => Risk::High,
            "critical" | "crit" => Risk::Critical,
            _ => match t.parse::<f64>().ok().or_else(|| crate::cvss::score(text.trim())) {
                Some(s) => Risk::from_score(s),
                None => Risk::Unknown,
            },
        }
    }

    /// A CVSS score's: 9 and up critical, 7 high, 4 moderate, above 0 low.
    pub fn from_score(score: f64) -> Risk {
        match score {
            s if s >= 9.0 => Risk::Critical,
            s if s >= 7.0 => Risk::High,
            s if s >= 4.0 => Risk::Moderate,
            s if s > 0.0 => Risk::Low,
            _ => Risk::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Risk::Unknown => "unknown",
            Risk::Low => "low",
            Risk::Moderate => "moderate",
            Risk::High => "high",
            Risk::Critical => "critical",
        }
    }
}

/// A known vulnerability in a package.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advisory {
    /// `GHSA-...`, `CVE-...`, `PYSEC-...`, `RUSTSEC-...`, `GO-...`.
    pub id: String,
    pub risk: Risk,
    pub title: String,
    /// The first version without it, when known.
    pub fixed_in: Option<String>,
    pub url: Option<String>,
}

impl Advisory {
    pub fn new(id: impl Into<String>, risk: Risk) -> Advisory {
        Advisory { id: id.into(), risk, ..Advisory::default() }
    }

    pub fn title(mut self, title: impl Into<String>) -> Advisory {
        self.title = title.into();
        self
    }

    pub fn fixed_in(mut self, version: impl Into<String>) -> Advisory {
        self.fixed_in = Some(version.into()).filter(|v: &String| !v.is_empty());
        self
    }

    pub fn url(mut self, url: impl Into<String>) -> Advisory {
        self.url = Some(url.into()).filter(|v: &String| !v.is_empty());
        self
    }
}

/// One dependency of the project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    /// What's installed or locked.
    pub current: String,
    /// The newest its requirement takes (npm's `wanted`), when known.
    pub wanted: Option<String>,
    /// The newest there is, when known.
    pub latest: Option<String>,
    /// Asked for by the project itself, not by another dependency.
    pub direct: bool,
    /// `dev`, `test`, `build`: when the manager says.
    pub group: Option<String>,
    pub advisories: Vec<Advisory>,
}

impl Dependency {
    pub fn new(name: impl Into<String>, current: impl Into<String>) -> Dependency {
        Dependency { name: name.into(), current: current.into(), direct: true, ..Dependency::default() }
    }

    pub fn transitive(mut self) -> Dependency {
        self.direct = false;
        self
    }

    pub fn direct(mut self, direct: bool) -> Dependency {
        self.direct = direct;
        self
    }

    pub fn wanted(mut self, v: impl Into<String>) -> Dependency {
        self.wanted = Some(v.into()).filter(|v: &String| !v.is_empty());
        self
    }

    pub fn latest(mut self, v: impl Into<String>) -> Dependency {
        self.latest = Some(v.into()).filter(|v: &String| !v.is_empty());
        self
    }

    pub fn group(mut self, g: impl Into<String>) -> Dependency {
        self.group = Some(g.into());
        self
    }

    /// Something newer than what it has: its wanted or its latest.
    pub fn is_outdated(&self) -> bool {
        [&self.wanted, &self.latest].into_iter().flatten().any(|v| newer(v, &self.current))
    }

    pub fn is_vulnerable(&self) -> bool {
        !self.advisories.is_empty()
    }

    /// Its worst advisory's risk.
    pub fn risk(&self) -> Option<Risk> {
        self.advisories.iter().map(|a| a.risk).max()
    }

    /// The step to its latest: a major is drawn as a warning.
    pub fn bump(&self) -> Bump {
        match (Version::parse(&self.current), self.latest.as_deref().and_then(Version::parse)) {
            (Some(c), Some(l)) => c.bump_to(&l),
            _ => Bump::None,
        }
    }
}

/// A newer version a manager knows of (`outdated`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub name: String,
    /// The version it's about, when there could be several.
    pub current: Option<String>,
    pub wanted: Option<String>,
    pub latest: Option<String>,
    /// Whether the project asks for it, when the manager says.
    pub direct: Option<bool>,
}

impl Update {
    pub fn new(name: impl Into<String>) -> Update {
        Update { name: name.into(), ..Update::default() }
    }
    pub fn current(mut self, v: impl Into<String>) -> Update {
        self.current = Some(v.into()).filter(|v: &String| !v.is_empty());
        self
    }
    pub fn wanted(mut self, v: impl Into<String>) -> Update {
        self.wanted = Some(v.into()).filter(|v: &String| !v.is_empty());
        self
    }
    pub fn latest(mut self, v: impl Into<String>) -> Update {
        self.latest = Some(v.into()).filter(|v: &String| !v.is_empty());
        self
    }
    pub fn direct(mut self, direct: bool) -> Update {
        self.direct = Some(direct);
        self
    }
}

/// An advisory against a package (`audit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub name: String,
    /// The version it's in, when the tool says; else every version of it
    /// the project has.
    pub version: Option<String>,
    pub advisory: Advisory,
}

impl Finding {
    pub fn new(name: impl Into<String>, version: Option<&str>, advisory: Advisory) -> Finding {
        Finding { name: name.into(), version: version.map(str::to_string).filter(|v| !v.is_empty()), advisory }
    }
}

/// A package a search found, to add.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub version: String,
    pub description: String,
}

/// An action's commands, in order, or why there are none.
pub type Steps = Result<Vec<Command>, String>;

/// A project's package manager, as a pack teaches lang-kit it: the
/// command for each thing, and how to read what it prints. Each command
/// runs with its stdin closed; its exit code is in the [`Output`], and a
/// parser may take a non-zero one (`npm outdated` and most audits exit 1
/// when they found something). An error from a parser is shown on the
/// page, with what the program said when it failed.
///
/// `list` may fill in `wanted` and `latest` itself (`go list -m -u`,
/// `cargo outdated`) and leave `outdated` out; `audit` is left out where
/// there's no tool.
pub trait PackageManager {
    /// Its name, shown on the page: `uv`, `npm`, `cargo`.
    fn name(&self) -> &str;

    /// The dependencies.
    fn list(&self) -> Command;
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String>;

    /// Newer versions.
    fn outdated(&self) -> Option<Command> {
        None
    }
    fn parse_outdated(&self, _out: &Output) -> Result<Vec<Update>, String> {
        Ok(Vec::new())
    }

    /// Known vulnerabilities.
    fn audit(&self) -> Option<Command> {
        None
    }
    fn parse_audit(&self, _out: &Output) -> Result<Vec<Finding>, String> {
        Ok(Vec::new())
    }

    /// Upgrading one (`u`), run as a task; `None` when this manager can't
    /// (a transitive one it doesn't lock).
    fn upgrade(&self, _dep: &Dependency) -> Option<Command> {
        None
    }
    /// Upgrading those that are outdated (`U`), as one task.
    fn upgrade_all(&self, _outdated: &[&Dependency]) -> Option<Command> {
        None
    }
    /// Adding `package` (`a`): a name, or `name@version` as typed or
    /// picked.
    fn add(&self, package: &str) -> Command;
    /// Removing one the project asks for (`d`).
    fn remove(&self, _dep: &Dependency) -> Option<Command> {
        None
    }

    // What the page runs: each a list of commands, run as tasks one after
    // another (`go get`, then `go mod tidy`), stopping at one that fails;
    // or why it can't, said on the page. By default, the one command
    // above.

    fn upgrade_steps(&self, dep: &Dependency) -> Steps {
        self.upgrade(dep).map(|c| vec![c]).ok_or_else(|| format!("{} can't upgrade {} on its own", self.name(), dep.name))
    }
    /// An empty list: nothing to do.
    fn upgrade_all_steps(&self, outdated: &[&Dependency]) -> Steps {
        Ok(self.upgrade_all(outdated).into_iter().collect())
    }
    fn add_steps(&self, package: &str) -> Steps {
        Ok(vec![self.add(package)])
    }
    fn remove_steps(&self, dep: &Dependency) -> Steps {
        self.remove(dep).map(|c| vec![c]).ok_or_else(|| format!("{} can't remove {}", self.name(), dep.name))
    }

    /// A search for packages to add; without one, what's typed is added
    /// as it is.
    fn search(&self, _query: &str) -> Option<Command> {
        None
    }
    fn parse_search(&self, _out: &Output) -> Result<Vec<Found>, String> {
        Ok(Vec::new())
    }

    /// The same package's name, two ways written: Python's `Foo_Bar` is
    /// `foo-bar`. As it is, by default.
    fn normalize(&self, name: &str) -> String {
        name.to_string()
    }

    /// The files whose contents say what's installed (`uv.lock`,
    /// `package-lock.json`), under `/project`: the cache is kept for what
    /// they hold, so a list is shown at once until they change.
    fn lockfiles(&self) -> Vec<String> {
        Vec::new()
    }
}

/// PEP 503's normal form of a Python package's name.
pub fn python_name(name: &str) -> String {
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

/// One list from what `list`, `outdated` and `audit` said: updates and
/// advisories on the dependencies they're about (by name and, when they
/// say, version), and a row for any about one `list` didn't give.
pub fn merge(manager: &dyn PackageManager, mut deps: Vec<Dependency>, updates: &[Update], findings: &[Finding]) -> Vec<Dependency> {
    let key = |name: &str| manager.normalize(name);
    for u in updates {
        let k = key(&u.name);
        let mut hit = false;
        for d in deps.iter_mut().filter(|d| key(&d.name) == k && u.current.as_ref().is_none_or(|c| *c == d.current)) {
            if u.wanted.is_some() {
                d.wanted.clone_from(&u.wanted);
            }
            if u.latest.is_some() {
                d.latest.clone_from(&u.latest);
            }
            if let Some(direct) = u.direct {
                d.direct = direct;
            }
            hit = true;
        }
        if !hit {
            deps.push(Dependency {
                name: u.name.clone(),
                current: u.current.clone().unwrap_or_default(),
                wanted: u.wanted.clone(),
                latest: u.latest.clone(),
                direct: u.direct.unwrap_or(true),
                ..Dependency::default()
            });
        }
    }
    for f in findings {
        let k = key(&f.name);
        let mut hit = false;
        for d in deps.iter_mut().filter(|d| key(&d.name) == k && f.version.as_ref().is_none_or(|v| *v == d.current)) {
            if !d.advisories.iter().any(|a| a.id == f.advisory.id) {
                d.advisories.push(f.advisory.clone());
            }
            hit = true;
        }
        if !hit {
            deps.push(Dependency { name: f.name.clone(), current: f.version.clone().unwrap_or_default(), advisories: vec![f.advisory.clone()], ..Dependency::default() });
        }
    }
    for d in &mut deps {
        d.advisories.sort_by_key(|a| Reverse(a.risk));
    }
    sort(&mut deps);
    deps
}

/// Vulnerable first, the worst first; then outdated, a major first; then
/// the rest. The project's own before the others', then by name.
pub fn sort(deps: &mut [Dependency]) {
    deps.sort_by(|a, b| {
        let rank = |d: &Dependency| (Reverse(d.risk().map(|r| r as u8 + 1).unwrap_or(0)), Reverse(d.is_outdated()), Reverse(d.direct), Reverse(d.bump()));
        rank(a).cmp(&rank(b)).then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase())).then_with(|| a.current.cmp(&b.current))
    });
}

/// The cache: what the page showed last for a project in a state, kept
/// in `/data` so it shows at once next time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cached {
    /// The manager, and a hash of what its lockfiles held.
    pub key: String,
    /// When it was listed, and audited, in seconds since 1970.
    pub listed: u64,
    pub audited: Option<u64>,
    pub deps: Vec<Dependency>,
    pub updates: Vec<Update>,
    pub findings: Vec<Finding>,
}

/// FNV-1a: a hash that's the same in every build.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The cache's key for `manager` with its lockfiles as they are now.
pub fn cache_key(manager: &dyn PackageManager) -> String {
    let mut bytes = manager.name().as_bytes().to_vec();
    for f in manager.lockfiles() {
        bytes.push(0);
        bytes.extend(f.as_bytes());
        bytes.push(0);
        bytes.extend(std::fs::read(&f).unwrap_or_default());
    }
    format!("{}-{:016x}", manager.name(), hash(&bytes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde::Deserialize;

    /// A package manager for tests, much as npm's `--json`s read.
    #[derive(Default)]
    pub struct Mock {
        pub audit: bool,
        pub search: bool,
        pub lockfile: Option<String>,
    }

    #[derive(Deserialize)]
    struct Listed {
        name: String,
        version: String,
        #[serde(default)]
        dev: bool,
        #[serde(default)]
        direct: Option<bool>,
    }

    #[derive(Deserialize)]
    struct Outdated {
        current: Option<String>,
        wanted: Option<String>,
        latest: Option<String>,
    }

    #[derive(Deserialize)]
    struct Vuln {
        name: String,
        id: String,
        severity: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        fixed: Option<String>,
    }

    impl PackageManager for Mock {
        fn name(&self) -> &str {
            "mock"
        }
        fn list(&self) -> Command {
            Command::new("mock").args(["ls", "--json"])
        }
        fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
            let listed: Vec<Listed> = out.json()?;
            Ok(listed
                .into_iter()
                .map(|l| {
                    let d = Dependency::new(l.name, l.version).direct(l.direct.unwrap_or(true));
                    if l.dev {
                        d.group("dev")
                    } else {
                        d
                    }
                })
                .collect())
        }
        fn outdated(&self) -> Option<Command> {
            Some(Command::new("mock").args(["outdated", "--json"]))
        }
        fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
            // Exit 1 with JSON: it found some.
            let map: std::collections::BTreeMap<String, Outdated> = out.json()?;
            Ok(map
                .into_iter()
                .map(|(name, o)| {
                    let mut u = Update::new(name);
                    u.current = o.current;
                    u.wanted = o.wanted;
                    u.latest = o.latest;
                    u
                })
                .collect())
        }
        fn audit(&self) -> Option<Command> {
            self.audit.then(|| Command::new("mock").args(["audit", "--json"]))
        }
        fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
            let vulns: Vec<Vuln> = out.json()?;
            Ok(vulns
                .into_iter()
                .map(|v| {
                    let mut a = Advisory::new(v.id, Risk::parse(&v.severity));
                    if let Some(f) = v.fixed {
                        a = a.fixed_in(f);
                    }
                    Finding::new(v.name, v.version.as_deref(), a)
                })
                .collect())
        }
        fn upgrade(&self, dep: &Dependency) -> Option<Command> {
            dep.direct.then(|| Command::new("mock").args(["install".to_string(), format!("{}@latest", dep.name)]))
        }
        fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
            (!outdated.is_empty()).then(|| Command::new("mock").arg("update").args(outdated.iter().map(|d| d.name.clone())))
        }
        fn add(&self, package: &str) -> Command {
            Command::new("mock").args(["install", package])
        }
        fn remove(&self, dep: &Dependency) -> Option<Command> {
            dep.direct.then(|| Command::new("mock").args(["uninstall", &dep.name]))
        }
        fn search(&self, query: &str) -> Option<Command> {
            self.search.then(|| Command::new("mock").args(["search", "--json", query]))
        }
        fn parse_search(&self, out: &Output) -> Result<Vec<Found>, String> {
            Ok(out.stdout.lines().filter_map(|l| l.split_once(' ')).map(|(n, v)| Found { name: n.into(), version: v.into(), description: String::new() }).collect())
        }
        fn normalize(&self, name: &str) -> String {
            python_name(name)
        }
        fn lockfiles(&self) -> Vec<String> {
            self.lockfile.iter().cloned().collect()
        }
    }

    pub fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    pub const LIST: &str = r#"[{"name":"requests","version":"2.31.0"},{"name":"urllib3","version":"1.26.5","direct":false},{"name":"pytest","version":"8.0.0","dev":true},{"name":"Flask","version":"2.3.0"},{"name":"idna","version":"3.6","direct":false}]"#;
    pub const OUTDATED: &str = r#"{"requests":{"current":"2.31.0","wanted":"2.31.0","latest":"2.32.3"},"flask":{"current":"2.3.0","wanted":"2.3.3","latest":"3.0.3"},"rich":{"wanted":"13.7.1","latest":"13.7.1"}}"#;
    pub const AUDIT: &str = r#"[{"name":"urllib3","version":"1.26.5","id":"GHSA-v845-jxx5-vc9f","severity":"moderate","fixed":"1.26.18"},{"name":"urllib3","id":"PYSEC-2023-212","severity":"8.1"},{"name":"jinja2","version":"3.1.2","id":"GHSA-h5c8-rqwp-cp95","severity":"low"}]"#;

    #[test]
    fn risks_read_as_tools_say_them() {
        assert_eq!(Risk::parse("MODERATE"), Risk::Moderate);
        assert_eq!(Risk::parse("medium"), Risk::Moderate);
        assert_eq!(Risk::parse("9.8"), Risk::Critical);
        assert_eq!(Risk::parse("7.5"), Risk::High);
        assert_eq!(Risk::parse("5"), Risk::Moderate);
        assert_eq!(Risk::parse("0.1"), Risk::Low);
        assert_eq!(Risk::parse("?"), Risk::Unknown);
        assert_eq!(Risk::parse("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"), Risk::Critical);
        assert_eq!(Risk::parse("CVSS:3.1/AV:L/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H"), Risk::Moderate);
        assert_eq!(Risk::parse("AV:N/AC:L/Au:N/C:P/I:P/A:P"), Risk::High, "CVSS 2: 7.5");
        assert_eq!(Risk::parse("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H/SC:N/SI:N/SA:N"), Risk::Unknown);
        assert!(Risk::Critical > Risk::High && Risk::Low > Risk::Unknown);
    }

    #[test]
    fn python_names_are_one_form() {
        assert_eq!(python_name("Foo_Bar.baz"), "foo-bar-baz");
        assert_eq!(python_name("a--b"), "a-b");
    }

    #[test]
    fn list_outdated_and_audit_make_one_list_worst_first() {
        let m = Mock::default();
        let deps = m.parse_list(&ok(LIST)).unwrap();
        let updates = m.parse_outdated(&Output { code: Some(1), ..ok(OUTDATED) }).unwrap();
        let findings = m.parse_audit(&ok(AUDIT)).unwrap();
        let all = merge(&m, deps, &updates, &findings);
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        // urllib3: high (8.1) and moderate; jinja2: low, not listed; then
        // the outdated, Flask's a major; then the rest.
        assert_eq!(names, ["urllib3", "jinja2", "Flask", "requests", "rich", "pytest", "idna"]);
        let urllib3 = &all[0];
        assert_eq!(urllib3.advisories.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["PYSEC-2023-212", "GHSA-v845-jxx5-vc9f"]);
        assert_eq!(urllib3.risk(), Some(Risk::High));
        assert!(!urllib3.direct);
        assert_eq!(all[1].current, "3.1.2");
        let flask = &all[2];
        assert_eq!((flask.wanted.as_deref(), flask.latest.as_deref(), flask.bump()), (Some("2.3.3"), Some("3.0.3"), Bump::Major));
        assert!(flask.is_outdated());
        assert_eq!(all[4].current, "", "an update for one not listed is a row of its own");
        assert!(!all[5].is_outdated() && all[5].group.as_deref() == Some("dev"));
    }

    #[test]
    fn an_update_for_another_version_is_left_on_it() {
        let m = Mock::default();
        let deps = vec![Dependency::new("a", "1.0.0"), Dependency::new("a", "2.0.0").transitive()];
        let all = merge(&m, deps, &[Update::new("a").current("1.0.0").latest("1.5.0")], &[Finding::new("a", Some("2.0.0"), Advisory::new("X-1", Risk::Low))]);
        assert_eq!(all.len(), 2);
        let one = all.iter().find(|d| d.current == "1.0.0").unwrap();
        let two = all.iter().find(|d| d.current == "2.0.0").unwrap();
        assert!(one.is_outdated() && !one.is_vulnerable());
        assert!(!two.is_outdated() && two.is_vulnerable());
    }

    #[test]
    fn the_cache_key_follows_the_lockfile() {
        let dir = std::env::temp_dir().join(format!("lang-kit-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("mock.lock");
        std::fs::write(&lock, "a 1").unwrap();
        let m = Mock { lockfile: Some(lock.to_string_lossy().into_owned()), ..Mock::default() };
        let one = cache_key(&m);
        assert_eq!(one, cache_key(&m));
        assert!(one.starts_with("mock-"));
        std::fs::write(&lock, "a 2").unwrap();
        assert_ne!(one, cache_key(&m));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(hash(b""), 0xcbf2_9ce4_8422_2325);
    }
}
