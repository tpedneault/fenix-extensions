//! The package manager page's Gradle and Maven.
//!
//! Gradle: the versions plugin's report (`dependencyUpdates`) -- the
//! build's own plugin, or the one the pack's init script applies -- as
//! JSON, which lists each declared dependency and the newer release of
//! one that's outdated. Gradle has no command that edits a build file, so
//! the init script's `fenixEdit` adds, removes and upgrades: the module's
//! `dependencies { }` block, or the version catalog.
//!
//! Maven: `dependency:list`, `versions:display-dependency-updates`, and
//! `versions:use-dep-version` / `update-property` to upgrade. Maven has
//! no command to add or remove one: `a` fetches it (`dependency:get`),
//! for pom.xml, and `d` isn't there.
//!
//! Both: OWASP dependency-check's JSON report for the vulnerable ones,
//! when the build has the plugin or `java.dependency_check` is on -- its
//! first run downloads NVD's database, which takes a while.

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Advisory, Dependency, Finding, Output, PackageManager, Risk, Update};

use crate::project::{Build, Declared, Tool};

/// The init scripts, written into the tools folder, where `${tools}`
/// names them for Gradle.
pub const DEPENDENCIES_INIT: &str = include_str!("gradle/dependencies.init.gradle");
pub const DEPENDENCY_CHECK_INIT: &str = include_str!("gradle/dependency-check.init.gradle");
pub const DEPENDENCIES_INIT_PATH: &str = "/tools/gradle/dependencies.init.gradle";
pub const DEPENDENCY_CHECK_INIT_PATH: &str = "/tools/gradle/dependency-check.init.gradle";

/// dependency-check's plugin, when the build doesn't have its own.
pub const DEPENDENCY_CHECK: &str = "12.2.2";

/// The versions a pre-release has, for Maven's versions plugin.
const PRE_RELEASES: &str = "(?i).*[.-](alpha|beta|rc|cr|m|milestone|preview|snapshot|ea)[.0-9-]*";

/// The versions plugin's report, between the init script's `FENIX-JSON`
/// lines: each dependency as `group:name`, the latest release of one
/// outdated. `declared` says which module asks for each.
pub fn parse_gradle(out: &Output, declared: &[(Declared, String)]) -> Result<Vec<Dependency>, String> {
    let json = out.stdout.split("FENIX-JSON").nth(1).map(str::trim).filter(|j| !j.is_empty()).ok_or_else(|| out.why())?;
    let report: Value = fenix_lang_kit::serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut deps = Vec::new();
    for (kind, outdated) in [("current", false), ("outdated", true), ("exceeded", false), ("unresolved", false)] {
        for d in report.get(kind).and_then(Value::as_array).into_iter().flatten() {
            let (Some(group), Some(name)) = (d.get("group").and_then(Value::as_str), d.get("name").and_then(Value::as_str)) else { continue };
            let version = d.get("version").and_then(Value::as_str).unwrap_or("");
            let coords = format!("{group}:{name}");
            let module = if name.ends_with(".gradle.plugin") { Some("plugins".to_string()) } else { declared.iter().find(|(x, _)| x.coordinates() == coords).map(|(_, m)| if m.is_empty() { "root".into() } else { m.clone() }) };
            let mut dep = Dependency::new(coords, version).direct(true);
            if let Some(m) = module {
                dep = dep.group(m);
            }
            if outdated {
                if let Some(latest) = d.get("latest").and_then(Value::as_str) {
                    dep = dep.latest(latest);
                }
            }
            deps.push(dep);
        }
    }
    Ok(deps)
}

/// `mvn dependency:list`: each module's (`Building orbit-core`), as
/// `group:artifact:type[:classifier]:version:scope`. A module that
/// failed (one needing another not installed yet) lists nothing.
pub fn parse_maven_list(out: &Output, declared: &[(Declared, String)]) -> Result<Vec<Dependency>, String> {
    let mut deps = Vec::new();
    let mut module = String::new();
    let mut listing = false;
    for line in out.stdout.lines() {
        let text = line.trim_start_matches("[INFO]").trim();
        if let Some(rest) = text.strip_prefix("Building ") {
            module = rest.split_whitespace().next().unwrap_or("").to_string();
            listing = false;
            continue;
        }
        if text.starts_with("The following files have been resolved") {
            listing = true;
            continue;
        }
        if !listing {
            continue;
        }
        if text.is_empty() {
            listing = false;
            continue;
        }
        let coords = text.split(" -- ").next().unwrap_or(text).trim();
        let parts: Vec<&str> = coords.split(':').collect();
        let (group, artifact, version) = match parts.len() {
            5 => (parts[0], parts[1], parts[3]),
            6 => (parts[0], parts[1], parts[4]),
            _ => continue,
        };
        let name = format!("{group}:{artifact}");
        let direct = declared.iter().any(|(d, m)| d.coordinates() == name && (m == &module || m.is_empty()));
        deps.push(Dependency::new(name, version).direct(direct).group(module.clone()));
    }
    if deps.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(deps)
}

/// `mvn versions:display-dependency-updates`: `group:artifact ... old ->
/// new`, the dots on one line or the next.
pub fn parse_maven_updates(out: &Output) -> Result<Vec<Update>, String> {
    if !out.ok() && !out.stdout.contains("display-dependency-updates") {
        return Err(out.why());
    }
    let mut updates = Vec::new();
    let mut pending: Option<String> = None;
    for line in out.stdout.lines() {
        let text = line.trim_start_matches("[INFO]").trim();
        let (name, rest) = match text.split_once(" ...") {
            Some((n, r)) if n.contains(':') && !n.contains(' ') => (Some(n.trim().to_string()), r),
            _ => (None, text),
        };
        let name = name.or_else(|| pending.take());
        match (name, rest.trim_start_matches('.').trim().split_once(" -> ")) {
            (Some(n), Some((old, new))) => updates.push(Update::new(n).current(old.trim()).latest(new.trim())),
            (Some(n), None) if text.ends_with("...") => pending = Some(n),
            _ => {}
        }
    }
    Ok(updates)
}

/// `pkg:maven/org.apache.logging.log4j/log4j-core@2.14.1` as
/// `org.apache.logging.log4j:log4j-core` and `2.14.1`.
fn purl(id: &str) -> Option<(String, String)> {
    let rest = id.strip_prefix("pkg:maven/")?;
    let (path, version) = rest.split_once('@')?;
    let (group, name) = path.split_once('/')?;
    Some((format!("{group}:{name}"), version.split(['?', '#']).next().unwrap_or(version).to_string()))
}

/// OWASP dependency-check's JSON report: each vulnerable dependency's
/// vulnerabilities, by their CVE (or the source's id).
pub fn parse_dependency_check(text: &str) -> Result<Vec<Finding>, String> {
    let report: Value = fenix_lang_kit::serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for d in report.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
        let Some(vulns) = d.get("vulnerabilities").and_then(Value::as_array) else { continue };
        let package = d.get("packages").and_then(Value::as_array).into_iter().flatten().filter_map(|p| p.get("id").and_then(Value::as_str)).find_map(purl);
        let (name, version) = match package {
            Some((n, v)) => (n, Some(v)),
            None => (d.get("fileName").and_then(Value::as_str).unwrap_or("?").to_string(), None),
        };
        for v in vulns {
            let id = v.get("name").and_then(Value::as_str).unwrap_or("?");
            let severity = v.get("severity").and_then(Value::as_str).or_else(|| v.pointer("/cvssv3/baseSeverity").and_then(Value::as_str)).unwrap_or("");
            let mut risk = Risk::parse(severity);
            if risk == Risk::Unknown {
                if let Some(score) = v.pointer("/cvssv3/baseScore").and_then(Value::as_f64) {
                    risk = Risk::parse(&score.to_string());
                }
            }
            let description = v.get("description").and_then(Value::as_str).unwrap_or("");
            let title = description.split(". ").next().unwrap_or(description).chars().take(160).collect::<String>();
            let url = v.get("references").and_then(Value::as_array).into_iter().flatten().filter_map(|r| r.get("url").and_then(Value::as_str)).next().map(str::to_string).unwrap_or_else(|| format!("https://nvd.nist.gov/vuln/detail/{id}"));
            let fixed = v.get("vulnerableSoftware").and_then(Value::as_array).into_iter().flatten().filter_map(|s| s.get("software")).find(|s| s.get("vulnerabilityIdMatched").and_then(Value::as_str) == Some("true")).and_then(|s| s.get("versionEndExcluding")).and_then(Value::as_str);
            let mut advisory = Advisory::new(id, risk).title(if title.is_empty() { id.to_string() } else { title }).url(url);
            if let Some(f) = fixed {
                advisory = advisory.fixed_in(f);
            }
            found.push(Finding::new(name.as_str(), version.as_deref(), advisory));
        }
    }
    Ok(found)
}

/// The report dependency-check writes: Gradle's under `build/reports`,
/// Maven's under `target`.
pub fn report_path(root: &str, tool: Tool) -> String {
    match tool {
        Tool::Maven => format!("{root}/target/dependency-check-report.json"),
        _ => format!("{root}/build/reports/dependency-check-report.json"),
    }
}

/// What's typed to add: `group:artifact` or `group:artifact:version`, a
/// space or `@` for the version too.
pub fn coordinates(typed: &str) -> Option<(String, String, Option<String>)> {
    let typed = typed.trim().replace(['@', ' '], ":");
    let parts: Vec<&str> = typed.split(':').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [g, a] => Some((g.to_string(), a.to_string(), None)),
        [g, a, v, ..] => Some((g.to_string(), a.to_string(), Some(v.to_string()))),
        _ => None,
    }
}

/// The page's manager for the build.
pub struct JavaPackages {
    pub tool: Tool,
    /// The wrapper, or Gradle's or Maven's own.
    pub runner: Command,
    /// Under `/project`.
    pub root: String,
    /// Each declared dependency, and its module's name.
    pub declared: Vec<(Declared, String)>,
    /// The build file a package is added to: the module you're in's.
    pub add_to: Option<String>,
    /// dependency-check, run.
    pub audit: bool,
    /// The build has dependency-check of its own.
    pub own_check: bool,
    /// Its build files, for the page's cache.
    pub files: Vec<String>,
    /// Each module's name and build file.
    pub modules: Vec<(String, String)>,
}

impl JavaPackages {
    pub fn new(build: &Build, runner: Command, root: &str, audit: bool) -> JavaPackages {
        let declared = build.modules.iter().flat_map(|m| m.declared.iter().map(|d| (d.clone(), m.name.clone()))).collect();
        let mut files: Vec<String> = build.file.iter().chain(build.modules.iter().map(|m| &m.file)).filter(|f| !f.is_empty()).map(|f| format!("{root}/{f}")).collect();
        files.dedup();
        if build.catalog {
            files.push(format!("{root}/gradle/libs.versions.toml"));
        }
        JavaPackages { tool: build.tool, runner, root: root.into(), declared, add_to: None, audit: audit || build.dependency_check, own_check: build.dependency_check, files, modules: build.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect() }
    }

    fn gradle(&self, args: &[&str]) -> Command {
        self.runner.clone().args(["-q", "--console=plain", "--init-script", "${tools}/gradle/dependencies.init.gradle"].iter().chain(args).map(|a| a.to_string()))
    }

    fn maven(&self, args: &[&str]) -> Command {
        self.runner.clone().args(std::iter::once("-B").chain(args.iter().copied()).map(str::to_string))
    }

    /// The version a dependency's pom says, when it's a property
    /// (`${junit.version}`): that property's name.
    fn property_of(&self, name: &str) -> Option<String> {
        let (d, _) = self.declared.iter().find(|(d, _)| d.coordinates() == name)?;
        let v = d.version.as_deref()?;
        v.strip_prefix("${").and_then(|v| v.strip_suffix('}')).map(str::to_string)
    }
}

impl PackageManager for JavaPackages {
    fn name(&self) -> &str {
        match self.tool {
            Tool::Maven => "Maven",
            _ => "Gradle",
        }
    }

    fn list(&self) -> Command {
        match self.tool {
            Tool::Maven => self.maven(&["-fae", "dependency:list", "-DoutputAbsoluteArtifactFilename=false", "-Dsort=true"]),
            _ => self.gradle(&["dependencyUpdates"]),
        }
    }

    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        match self.tool {
            Tool::Maven => parse_maven_list(out, &self.declared),
            _ => parse_gradle(out, &self.declared),
        }
    }

    /// Gradle's list has them already.
    fn outdated(&self) -> Option<Command> {
        let ignore = format!("-Dmaven.version.ignore={PRE_RELEASES}");
        (self.tool == Tool::Maven).then(|| self.maven(&["versions:display-dependency-updates", "-DprocessDependencyManagement=false", &ignore, "-Dversions.outputLineWidth=200"]))
    }

    fn parse_outdated(&self, out: &Output) -> Result<Vec<Update>, String> {
        parse_maven_updates(out)
    }

    fn audit(&self) -> Option<Command> {
        if !self.audit {
            return None;
        }
        Some(match self.tool {
            Tool::Maven if self.own_check => self.maven(&["dependency-check:aggregate", "-Dformat=JSON"]),
            Tool::Maven => self.maven(&[&format!("org.owasp:dependency-check-maven:{DEPENDENCY_CHECK}:aggregate"), "-Dformat=JSON"]),
            _ => self.runner.clone().args(["-q", "--console=plain", "--init-script", "${tools}/gradle/dependency-check.init.gradle", "dependencyCheckAggregate"]),
        })
    }

    /// The report it wrote, read.
    fn parse_audit(&self, out: &Output) -> Result<Vec<Finding>, String> {
        let path = report_path(&self.root, self.tool);
        match std::fs::read_to_string(&path) {
            Ok(text) => parse_dependency_check(&text),
            Err(_) if !out.ok() => Err(out.why()),
            Err(e) => Err(format!("{path}: {e}")),
        }
    }

    fn upgrade(&self, dep: &Dependency) -> Option<Command> {
        let latest = dep.latest.as_deref()?;
        Some(match self.tool {
            Tool::Maven => match self.property_of(&dep.name) {
                Some(p) => self.maven(&["versions:update-property", &format!("-Dproperty={p}"), &format!("-DnewVersion=[{latest}]"), "-DgenerateBackupPoms=false"]),
                None => self.maven(&["versions:use-dep-version", &format!("-Dincludes={}", dep.name), &format!("-DdepVersion={latest}"), "-DforceVersion=true", "-DgenerateBackupPoms=false"]),
            },
            _ => self.gradle(&["fenixEdit", "-Pfenix.op=set", &format!("-Pfenix.coords={}:{latest}", dep.name)]),
        })
    }

    fn upgrade_all(&self, outdated: &[&Dependency]) -> Option<Command> {
        match self.tool {
            Tool::Maven => {
                let ignore = format!("-Dmaven.version.ignore={PRE_RELEASES}");
                Some(self.maven(&["versions:use-latest-releases", "versions:update-properties", &ignore, "-DgenerateBackupPoms=false"]))
            }
            _ => {
                let coords: Vec<String> = outdated.iter().filter_map(|d| Some(format!("{}:{}", d.name, d.latest.as_deref()?))).collect();
                (!coords.is_empty()).then(|| self.gradle(&["fenixEdit", "-Pfenix.op=set", &format!("-Pfenix.coords={}", coords.join(","))]))
            }
        }
    }

    fn add(&self, package: &str) -> Command {
        let (g, a, v) = coordinates(package).unwrap_or_else(|| (package.trim().to_string(), String::new(), None));
        match self.tool {
            Tool::Maven => {
                let artifact = format!("-Dartifact={g}:{a}:{}", v.as_deref().unwrap_or("LATEST"));
                self.maven(&["dependency:get", &artifact])
            }
            _ => {
                let coords = match &v {
                    Some(v) => format!("{g}:{a}:{v}"),
                    None => format!("{g}:{a}"),
                };
                let file = self.add_to.clone().unwrap_or_else(|| "build.gradle.kts".into());
                self.gradle(&["fenixEdit", "-Pfenix.op=add", &format!("-Pfenix.file={file}"), &format!("-Pfenix.coords={coords}")])
            }
        }
    }

    /// Gradle's: out of the build file of the module that declares it.
    fn remove(&self, dep: &Dependency) -> Option<Command> {
        if self.tool == Tool::Maven || !dep.direct {
            return None;
        }
        let module = self.declared.iter().find(|(d, _)| d.coordinates() == dep.name).map(|(_, m)| m.clone())?;
        let rel = self.modules.iter().find(|(m, f)| *m == module && !f.is_empty()).map(|(_, f)| f.clone())?;
        Some(self.gradle(&["fenixEdit", "-Pfenix.op=remove", &format!("-Pfenix.file={rel}"), &format!("-Pfenix.coords={}", dep.name)]))
    }

    fn lockfiles(&self) -> Vec<String> {
        self.files.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{maven, orbit};
    use crate::tests::line;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    type Row = (String, String, Option<String>, bool, Option<String>);

    fn rows(deps: &[Dependency]) -> Vec<Row> {
        deps.iter().map(|d| (d.name.clone(), d.current.clone(), d.latest.clone(), d.direct, d.group.clone())).collect()
    }

    #[test]
    fn gradle_versions_report_is_the_list_with_its_latest() {
        let dir = orbit("pkg-gradle");
        let b = Build::read(&dir.to_string_lossy());
        let p = JavaPackages::new(&b, b.runner(Some(crate::tests::wrapper(&b)), None, None).unwrap(), "/project", false);
        assert_eq!(line(&p.list()), "./gradlew -q --console=plain --init-script ${tools}/gradle/dependencies.init.gradle dependencyUpdates");
        let deps = p.parse_list(&out(0, include_str!("samples/gradle-dependency-updates.txt"))).unwrap();
        let r = rows(&deps);
        assert!(r.contains(&("com.google.guava:guava".into(), "33.0.0-jre".into(), Some("33.7.2-jre".into()), true, Some("app".into()))), "{r:?}");
        assert!(r.contains(&("org.junit.jupiter:junit-jupiter".into(), "6.1.3".into(), None, true, Some("app".into()))));
        assert!(r.iter().any(|d| d.0 == "com.github.ben-manes.versions:com.github.ben-manes.versions.gradle.plugin" && d.4.as_deref() == Some("plugins")));
        assert_eq!(p.outdated(), None);
        assert_eq!(p.audit(), None, "no dependency-check unless asked");
        assert!(p.parse_list(&Output { code: Some(1), stderr: "BUILD FAILED".into(), ..Output::default() }).unwrap_err().contains("BUILD FAILED"));

        let guava = deps.iter().find(|d| d.name == "com.google.guava:guava").unwrap();
        assert_eq!(line(&p.upgrade(guava).unwrap()), "./gradlew -q --console=plain --init-script ${tools}/gradle/dependencies.init.gradle fenixEdit -Pfenix.op=set -Pfenix.coords=com.google.guava:guava:33.7.2-jre");
        let outdated: Vec<&Dependency> = deps.iter().filter(|d| d.is_outdated()).collect();
        assert!(line(&p.upgrade_all(&outdated).unwrap()).ends_with("-Pfenix.coords=com.google.guava:guava:33.7.2-jre,org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0"));
        assert_eq!(line(&p.remove(guava).unwrap()), "./gradlew -q --console=plain --init-script ${tools}/gradle/dependencies.init.gradle fenixEdit -Pfenix.op=remove -Pfenix.file=app/build.gradle.kts -Pfenix.coords=com.google.guava:guava");
        let mut p = p;
        p.add_to = Some("core/build.gradle.kts".into());
        assert!(line(&p.add("org.slf4j:slf4j-api@2.0.17")).ends_with("fenixEdit -Pfenix.op=add -Pfenix.file=core/build.gradle.kts -Pfenix.coords=org.slf4j:slf4j-api:2.0.17"));
        assert_eq!(p.lockfiles(), ["/project/settings.gradle.kts", "/project/build.gradle.kts", "/project/app/build.gradle.kts", "/project/core/build.gradle.kts", "/project/gradle/libs.versions.toml"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn maven_list_updates_and_upgrades() {
        let dir = maven("pkg-maven");
        let b = Build::read(&dir.to_string_lossy());
        let p = JavaPackages::new(&b, b.runner(Some(crate::tests::wrapper(&b)), None, None).unwrap(), "/project", false);
        assert_eq!(line(&p.list()), "./mvnw -B -fae dependency:list -DoutputAbsoluteArtifactFilename=false -Dsort=true");
        let deps = p.parse_list(&out(1, include_str!("samples/mvn-dependency-list.txt"))).unwrap();
        let r = rows(&deps);
        assert!(r.contains(&("com.google.guava:guava".into(), "33.0.0-jre".into(), None, true, Some("orbit-core".into()))), "{r:?}");
        assert!(r.contains(&("com.google.guava:failureaccess".into(), "1.0.2".into(), None, false, Some("orbit-core".into()))));
        assert_eq!(deps.len(), 15, "orbit-api failed: orbit-core isn't installed");
        assert!(p.parse_list(&Output { code: Some(1), stdout: "[ERROR] boom".into(), ..Output::default() }).is_err());

        assert!(line(&p.outdated().unwrap()).starts_with("./mvnw -B versions:display-dependency-updates -DprocessDependencyManagement=false -Dmaven.version.ignore="));
        let updates = p.parse_outdated(&out(0, include_str!("samples/mvn-display-dependency-updates.txt"))).unwrap();
        let u: Vec<(String, Option<String>, Option<String>)> = updates.iter().map(|u| (u.name.clone(), u.current.clone(), u.latest.clone())).collect();
        assert_eq!(
            u,
            [
                ("com.google.guava:guava".into(), Some("33.0.0-jre".into()), Some("33.7.2-jre".into())),
                ("org.junit.jupiter:junit-jupiter".into(), Some("5.10.0".into()), Some("6.1.3".into())),
                ("org.springframework.boot:spring-boot-starter-web".into(), Some("3.4.0".into()), Some("4.1.2".into())),
            ]
        );
        // Maven's own wrapping, the version on the next line.
        let wrapped = parse_maven_updates(&out(0, "[INFO]   org.springframework.boot:spring-boot-starter-web ...\n[INFO]                                                       3.4.0 -> 4.2.0-RC1\n")).unwrap();
        assert_eq!((wrapped[0].name.as_str(), wrapped[0].latest.as_deref()), ("org.springframework.boot:spring-boot-starter-web", Some("4.2.0-RC1")));

        let junit = Dependency::new("org.junit.jupiter:junit-jupiter", "5.10.0").latest("6.1.3");
        assert_eq!(line(&p.upgrade(&junit).unwrap()), "./mvnw -B versions:update-property -Dproperty=junit.version -DnewVersion=[6.1.3] -DgenerateBackupPoms=false", "its version is a property");
        let guava = Dependency::new("com.google.guava:guava", "33.0.0-jre").latest("33.7.2-jre");
        assert_eq!(line(&p.upgrade(&guava).unwrap()), "./mvnw -B versions:use-dep-version -Dincludes=com.google.guava:guava -DdepVersion=33.7.2-jre -DforceVersion=true -DgenerateBackupPoms=false");
        assert!(line(&p.upgrade_all(&[]).unwrap()).starts_with("./mvnw -B versions:use-latest-releases versions:update-properties"));
        assert_eq!(line(&p.add("org.slf4j:slf4j-api")), "./mvnw -B dependency:get -Dartifact=org.slf4j:slf4j-api:LATEST");
        assert_eq!(p.remove(&guava), None, "Maven can't");
        // The build has dependency-check: its own goal.
        assert_eq!(line(&p.audit().unwrap()), "./mvnw -B dependency-check:aggregate -Dformat=JSON");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dependency_checks_report_is_each_vulnerability() {
        let found = parse_dependency_check(include_str!("samples/dependency-check-report.json")).unwrap();
        type Found = (String, Option<String>, String, Risk, Option<String>);
        let rows: Vec<Found> = found.iter().map(|f| (f.name.clone(), f.version.clone(), f.advisory.id.clone(), f.advisory.risk, f.advisory.fixed_in.clone())).collect();
        assert_eq!(
            rows,
            [
                ("org.apache.logging.log4j:log4j-core".into(), Some("2.14.1".into()), "CVE-2021-44228".into(), Risk::Critical, Some("2.15.0".into())),
                ("org.apache.logging.log4j:log4j-core".into(), Some("2.14.1".into()), "CVE-2021-45046".into(), Risk::Critical, Some("2.16.0".into())),
                ("org.apache.logging.log4j:log4j-core".into(), Some("2.14.1".into()), "CVE-2021-44832".into(), Risk::Moderate, None),
            ]
        );
        assert_eq!(found[0].advisory.url.as_deref(), Some("https://logging.apache.org/log4j/2.x/security.html"));
        assert_eq!(found[1].advisory.url.as_deref(), Some("https://nvd.nist.gov/vuln/detail/CVE-2021-45046"), "no reference: NVD's page");
        assert!(found[0].advisory.title.starts_with("Apache Log4j2 2.0-beta9 through 2.15.0"));

        // Read from where the run wrote it.
        let dir = orbit("pkg-audit");
        let b = Build::read(&dir.to_string_lossy());
        let root = dir.to_string_lossy().into_owned();
        let p = JavaPackages::new(&b, b.runner(Some(crate::tests::wrapper(&b)), None, None).unwrap(), &root, true);
        assert_eq!(line(&p.audit().unwrap()), "./gradlew -q --console=plain --init-script ${tools}/gradle/dependency-check.init.gradle dependencyCheckAggregate");
        assert!(p.parse_audit(&out(1, "")).is_err());
        crate::project::tests::write(&dir, "build/reports/dependency-check-report.json", include_str!("samples/dependency-check-report.json"));
        assert_eq!(p.parse_audit(&out(0, "")).unwrap().len(), 3);
        assert_eq!(report_path("/p", Tool::Maven), "/p/target/dependency-check-report.json");
        assert_eq!(coordinates("a:b 1.0"), Some(("a".into(), "b".into(), Some("1.0".into()))));
        assert_eq!(coordinates("guava"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
