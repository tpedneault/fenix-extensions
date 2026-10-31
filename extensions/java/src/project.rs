//! What the build is, as its files say: Gradle's (`settings.gradle(.kts)`,
//! each project's `build.gradle(.kts)`, the conventions in `buildSrc`, the
//! wrapper) or Maven's (`pom.xml` and its modules, the wrapper). For each
//! module: whether it's an application, its main class, Kotlin, Spring
//! Boot; for the build: the Java release it asks for (a toolchain,
//! `jvmToolchain`, `sourceCompatibility`, `maven.compiler.release`), and
//! the plugins the package page uses.
//!
//! Gradle's files are code, so they're read for the forms people write,
//! not run. Paths are relative to the project's folder, with `/` between.

use std::path::Path;

/// Which build it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    None,
    Gradle,
    Maven,
}

/// The wrapper: `gradlew` and `gradle/wrapper/`, or `mvnw` and
/// `.mvn/wrapper/`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wrapper {
    /// `gradlew` (`mvnw`) is there.
    pub script: bool,
    /// Gradle's `gradle-wrapper.jar`, without which `gradlew` can't run;
    /// Maven's script-only wrapper has none, and needs none.
    pub jar: bool,
    /// The version its `distributionUrl` downloads.
    pub version: Option<String>,
}

/// A dependency a build file declares: `group:name`, and its version
/// when it says one there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub group: String,
    pub name: String,
    pub version: Option<String>,
    /// `test`, `provided`; `testImplementation` for Gradle.
    pub scope: Option<String>,
}

impl Declared {
    pub fn coordinates(&self) -> String {
        format!("{}:{}", self.group, self.name)
    }
}

/// One project of the build: Gradle's subproject, Maven's module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    /// Gradle's path without its first `:` (`app`, `libs:core`); Maven's
    /// artifactId. "" is the root.
    pub name: String,
    /// Its folder: "" for the root.
    pub dir: String,
    /// Its build file.
    pub file: String,
    pub application: bool,
    pub main_class: Option<String>,
    pub kotlin: bool,
    pub spring_boot: bool,
    /// It has tests: `src/test`.
    pub tests: bool,
    pub declared: Vec<Declared>,
}

impl Module {
    /// Gradle's task `task` of it: `:app:test`, or `test` for the root.
    pub fn task(&self, task: &str) -> String {
        if self.name.is_empty() {
            task.to_string()
        } else {
            format!(":{}:{task}", self.name)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Build {
    pub tool: Tool,
    /// The root build file (or settings file): `build.gradle.kts`,
    /// `pom.xml`.
    pub file: Option<String>,
    pub wrapper: Wrapper,
    pub modules: Vec<Module>,
    /// The Java release it asks for: the highest a module asks.
    pub release: Option<u32>,
    /// `com.github.ben-manes.versions`, applied by the build.
    pub versions_plugin: bool,
    /// `org.owasp.dependencycheck` (`dependency-check-maven`), there.
    pub dependency_check: bool,
    /// Gradle's version catalog, `gradle/libs.versions.toml`.
    pub catalog: bool,
}

/// Files whose saving changes what the build is.
pub const BUILD_FILES: &[&str] = &["settings.gradle", "settings.gradle.kts", "build.gradle", "build.gradle.kts", "gradle.properties", "libs.versions.toml", "gradle-wrapper.properties", "pom.xml", "maven-wrapper.properties"];

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

/// `text` without `//` and `/* */` comments, and `<!-- -->` in XML: a
/// plugin commented out isn't applied.
fn uncommented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let next = [rest.find("//"), rest.find("/*"), rest.find('"')].into_iter().flatten().min();
        let Some(at) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(string) = tail.strip_prefix('"') {
            // A string, kept whole: `"https://..."` has `//` in it.
            let end = string.find('"').map(|e| e + 2).unwrap_or(tail.len());
            out.push_str(&tail[..end]);
            rest = &tail[end..];
        } else if tail.starts_with("//") {
            rest = tail.find('\n').map(|e| &tail[e..]).unwrap_or("");
        } else {
            rest = tail.find("*/").map(|e| &tail[e + 2..]).unwrap_or("");
        }
    }
    out
}

/// The quoted strings in `text`, `'...'` or `"..."`.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '"' || c == '\'' {
            if let Some(end) = text[i + 1..].find(c) {
                out.push(text[i + 1..i + 1 + end].to_string());
                for _ in 0..=end {
                    chars.next();
                }
            }
        }
    }
    out
}

/// The number after `pattern` in `text` (`JavaLanguageVersion.of(` is
/// followed by `21)`), skipping spaces, quotes and `VERSION_` / `1_`.
fn number_after(text: &str, pattern: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices(pattern) {
        let rest = text[at + pattern.len()..].trim_start_matches([' ', '"', '\'', '(', '=']);
        let rest = rest.strip_prefix("JavaVersion.").unwrap_or(rest);
        let rest = rest.strip_prefix("VERSION_").unwrap_or(rest);
        // Java 8 is `1.8` / `VERSION_1_8`.
        let rest = rest.strip_prefix("1_").or_else(|| rest.strip_prefix("1.")).filter(|r| r.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(rest);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse() {
            out.push(n);
        }
    }
    out
}

/// The Java release a Gradle file asks for.
fn gradle_release(text: &str) -> Option<u32> {
    ["JavaLanguageVersion.of", "jvmToolchain", "sourceCompatibility", "targetCompatibility", "options.release", "release.set"]
        .iter()
        .flat_map(|p| number_after(text, p))
        .filter(|n| (5..100).contains(n))
        .max()
}

/// The string set by `main` in `text`: `mainClass.set("x")`, `mainClass
/// = "x"`, `mainClassName = 'x'`.
fn gradle_main(text: &str) -> Option<String> {
    for key in ["mainClass.set(", "mainClass =", "mainClassName =", "mainClass.value("] {
        if let Some(at) = text.find(key) {
            let rest = &text[at + key.len()..];
            let line = rest.lines().next().unwrap_or("");
            if let Some(s) = quoted(line).into_iter().next() {
                return Some(s);
            }
        }
    }
    None
}

/// The plugins a Gradle file applies: ids in `plugins { }`, `kotlin("jvm")`
/// as `org.jetbrains.kotlin.jvm`, and `apply plugin:`.
fn gradle_plugins(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(at) = text.find("plugins") {
        let block = &text[at..];
        let start = block.find('{').map(|i| i + 1).unwrap_or(0);
        let end = block.find('}').unwrap_or(block.len()).max(start);
        let block = &block[start..end];
        for line in block.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("kotlin(") {
                if let Some(k) = quoted(rest).first() {
                    out.push(format!("org.jetbrains.kotlin.{k}"));
                }
            } else if let Some(rest) = line.strip_prefix("id") {
                if let Some(id) = quoted(rest).first() {
                    out.push(id.clone());
                }
            } else if line.starts_with("alias(") {
                // A catalog's plugin: `alias(libs.plugins.spring.boot)`.
                out.push(line.trim_start_matches("alias(").trim_end_matches(')').to_string());
            } else if let Some(word) = line.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '`').find(|w| !w.is_empty()) {
                // `application`, `java-library`, `` `java-library` ``.
                out.push(word.trim_matches('`').to_string());
            }
        }
    }
    for (at, _) in text.match_indices("apply plugin:") {
        if let Some(id) = quoted(text[at..].lines().next().unwrap_or("")).first() {
            out.push(id.clone());
        }
    }
    out
}

/// What a Gradle file declares in `dependencies { }`: `"g:a:v"` strings
/// and `group = "g", name = "a"` maps, by configuration.
fn gradle_declared(text: &str) -> Vec<Declared> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("dependencies") {
        let block = &text[at..];
        let Some(open) = block.find('{') else { continue };
        let mut depth = 0;
        let mut end = block.len();
        for (i, c) in block[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        for line in block[open + 1..end].lines() {
            let line = line.trim();
            let config: String = line.chars().take_while(|c| c.is_alphanumeric()).collect();
            if config.is_empty() || config == "platform" {
                continue;
            }
            for s in quoted(line) {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 2 && parts.iter().take(2).all(|p| !p.is_empty() && !p.contains(' ')) {
                    out.push(Declared { group: parts[0].into(), name: parts[1].into(), version: parts.get(2).map(|v| v.to_string()), scope: Some(config.clone()) });
                }
            }
        }
    }
    out
}

/// The projects `settings.gradle(.kts)` includes: `include("app",
/// ":libs:core")`, `include 'app'`. Each one's folder is its path with
/// `/`.
fn gradle_includes(settings: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in settings.match_indices("include") {
        let after = &settings[at + "include".len()..];
        if !after.starts_with(['(', ' ', '\t']) {
            continue;
        }
        // `include(` on one line, its names on the next.
        let end = if after.starts_with('(') { after.find(')') } else { after.find('\n') }.unwrap_or(after.len());
        for name in quoted(&after[..end]) {
            let name = name.trim_start_matches(':').to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// The first of `names` there is in `dir`.
fn first(root: &Path, dir: &str, names: &[&str]) -> Option<String> {
    names.iter().map(|n| if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") }).find(|p| root.join(p).is_file())
}

fn is_dir(root: &Path, rel: &str) -> bool {
    root.join(rel).is_dir()
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

/// The version in a wrapper's `distributionUrl`: `gradle-8.14.3-bin.zip`,
/// `apache-maven-3.9.11-bin.zip`.
pub fn distribution_version(properties: &str) -> Option<String> {
    let url = properties.lines().find_map(|l| l.trim().strip_prefix("distributionUrl"))?;
    let file = url.rsplit('/').next()?;
    let name = file.trim_end_matches(".zip").trim_end_matches("-bin").trim_end_matches("-all");
    let version = name.rsplit_once('-').map(|(_, v)| v)?;
    version.starts_with(|c: char| c.is_ascii_digit()).then(|| version.to_string())
}

impl Build {
    /// The build in `root`: Gradle's when it has a settings or build
    /// file, else Maven's when it has a `pom.xml`.
    pub fn read(root: &str) -> Build {
        let root = Path::new(root);
        if let Some(b) = Build::gradle(root) {
            return b;
        }
        Build::maven(root).unwrap_or_default()
    }

    fn gradle(root: &Path) -> Option<Build> {
        let settings = first(root, "", &["settings.gradle.kts", "settings.gradle"]);
        let file = first(root, "", &["build.gradle.kts", "build.gradle"]);
        let top = settings.clone().or(file.clone())?;
        let includes = settings.as_deref().and_then(|s| read(root, s)).map(|t| gradle_includes(&uncommented(&t))).unwrap_or_default();
        let mut b = Build { tool: Tool::Gradle, file: Some(top), ..Build::default() };
        let props = read(root, "gradle/wrapper/gradle-wrapper.properties");
        b.wrapper = Wrapper { script: root.join("gradlew").is_file(), jar: root.join("gradle/wrapper/gradle-wrapper.jar").is_file(), version: props.as_deref().and_then(distribution_version) };
        b.catalog = root.join("gradle/libs.versions.toml").is_file();
        // Conventions plugins: what's in buildSrc applies to each project
        // that uses it.
        let mut conventions = String::new();
        for dir in ["buildSrc/src/main/kotlin", "buildSrc/src/main/groovy", "build-logic/src/main/kotlin", "build-logic/convention/src/main/kotlin"] {
            if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
                let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".gradle.kts") || p.to_string_lossy().ends_with(".gradle")).collect();
                files.sort();
                for f in files {
                    conventions.push_str(&uncommented(&std::fs::read_to_string(f).unwrap_or_default()));
                    conventions.push('\n');
                }
            }
        }
        let catalog = read(root, "gradle/libs.versions.toml").unwrap_or_default();
        let mut releases = vec![gradle_release(&conventions)];
        let names = std::iter::once(String::new()).chain(includes);
        for name in names {
            let dir = name.replace(':', "/");
            let Some(file) = first(root, &dir, &["build.gradle.kts", "build.gradle"]) else {
                if name.is_empty() {
                    continue;
                }
                // An included project with no build file of its own.
                b.modules.push(Module { name: name.clone(), dir: dir.clone(), ..Module::default() });
                continue;
            };
            let text = uncommented(&read(root, &file).unwrap_or_default());
            let plugins = gradle_plugins(&text);
            let has = |p: &str| plugins.iter().any(|x| x == p || x.ends_with(p));
            let uses_conventions = plugins.iter().any(|p| conventions.contains(p.as_str()) || p.starts_with("buildlogic") || p.contains("conventions"));
            let applied_by_conventions = |word: &str| uses_conventions && gradle_plugins(&conventions).iter().any(|p| p == word);
            let kotlin = has("org.jetbrains.kotlin.jvm") || has("kotlin.jvm") || has("kotlin-jvm") || is_dir(root, &join(&dir, "src/main/kotlin")) || (uses_conventions && conventions.contains("org.jetbrains.kotlin"));
            let spring_boot = has("org.springframework.boot") || has("spring.boot") || text.contains("springframework.boot");
            let application = has("application") || spring_boot || applied_by_conventions("application") || plugins.iter().any(|p| p.contains("application-conventions"));
            releases.push(gradle_release(&text));
            b.versions_plugin |= has("com.github.ben-manes.versions") || has("ben.manes.versions") || (has("versions") && catalog.contains("ben-manes"));
            b.dependency_check |= has("org.owasp.dependencycheck") || has("dependencycheck") || has("dependency.check");
            let mut declared = gradle_declared(&text);
            if uses_conventions {
                declared.extend(gradle_declared(&conventions));
            }
            b.modules.push(Module {
                name: name.clone(),
                dir: dir.clone(),
                file,
                application,
                main_class: gradle_main(&text).or_else(|| uses_conventions.then(|| gradle_main(&conventions)).flatten()),
                kotlin,
                spring_boot,
                tests: is_dir(root, &join(&dir, "src/test")),
                declared,
            });
        }
        b.release = releases.into_iter().flatten().max();
        Some(b)
    }

    fn maven(root: &Path) -> Option<Build> {
        if !root.join("pom.xml").is_file() {
            return None;
        }
        let props = read(root, ".mvn/wrapper/maven-wrapper.properties");
        let mut b = Build {
            tool: Tool::Maven,
            file: Some("pom.xml".into()),
            wrapper: Wrapper { script: root.join("mvnw").is_file(), jar: true, version: props.as_deref().and_then(distribution_version) },
            ..Build::default()
        };
        let mut queue = std::collections::VecDeque::from([String::new()]);
        let mut releases = Vec::new();
        while let Some(dir) = queue.pop_front() {
            let file = join(&dir, "pom.xml");
            let Some(text) = read(root, &file) else { continue };
            let Some(pom) = Pom::parse(&text) else { continue };
            for m in &pom.modules {
                let sub = join(&dir, m.trim_end_matches("/pom.xml"));
                if !queue.contains(&sub) && !b.modules.iter().any(|x| x.dir == sub) {
                    queue.push_back(sub);
                }
            }
            releases.push(pom.release);
            b.dependency_check |= pom.plugins.iter().any(|p| p == "dependency-check-maven");
            b.versions_plugin |= pom.plugins.iter().any(|p| p == "versions-maven-plugin");
            let spring_boot = pom.spring_boot;
            b.modules.push(Module {
                name: if dir.is_empty() { String::new() } else { pom.artifact.clone() },
                dir: dir.clone(),
                file,
                application: spring_boot || pom.main_class.is_some(),
                main_class: pom.main_class.clone(),
                kotlin: pom.plugins.iter().any(|p| p == "kotlin-maven-plugin") || is_dir(root, &join(&dir, "src/main/kotlin")),
                spring_boot,
                tests: is_dir(root, &join(&dir, "src/test")),
                declared: pom.declared.clone(),
            });
        }
        b.release = releases.into_iter().flatten().max();
        Some(b)
    }

    /// Whether any module is Kotlin.
    pub fn kotlin(&self) -> bool {
        self.modules.iter().any(|m| m.kotlin)
    }

    pub fn spring_boot(&self) -> bool {
        self.modules.iter().any(|m| m.spring_boot)
    }

    /// The module whose folder holds `rel` (a file's path): the deepest.
    pub fn module_of(&self, rel: &str) -> Option<&Module> {
        self.modules.iter().filter(|m| m.dir.is_empty() || rel == m.dir || rel.starts_with(&format!("{}/", m.dir))).max_by_key(|m| m.dir.len())
    }

    /// The build program: the wrapper (`wrapper`, `gradlew` or `mvnw` as
    /// found in the project) when it can run, else Gradle's or Maven's
    /// own, `gradle` / `mvn`.
    pub fn runner(&self, wrapper: Option<&str>, gradle: Option<&str>, mvn: Option<&str>) -> Option<fenix_extension_api::Command> {
        use fenix_extension_api::Command;
        let wrapper = wrapper.filter(|_| self.wrapper.script && self.wrapper.jar);
        match self.tool {
            Tool::Gradle => wrapper.or(gradle).map(Command::new),
            Tool::Maven => wrapper.or(mvn).map(Command::new),
            Tool::None => None,
        }
    }

    /// The wrapper's script's name.
    pub fn wrapper_name(&self) -> Option<&'static str> {
        match self.tool {
            Tool::Gradle => Some("gradlew"),
            Tool::Maven => Some("mvnw"),
            Tool::None => None,
        }
    }
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

fn text_of(n: Option<roxmltree::Node<'_, '_>>) -> Option<String> {
    n.and_then(|n| n.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// What a `pom.xml` says that the pack uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pom {
    pub artifact: String,
    pub packaging: String,
    pub modules: Vec<String>,
    pub release: Option<u32>,
    /// Its plugins' artifactIds.
    pub plugins: Vec<String>,
    pub main_class: Option<String>,
    pub spring_boot: bool,
    pub declared: Vec<Declared>,
}

impl Pom {
    pub fn parse(text: &str) -> Option<Pom> {
        let doc = roxmltree::Document::parse(text).ok()?;
        let project = doc.root_element();
        let mut properties: Vec<(String, String)> = Vec::new();
        if let Some(p) = child(project, "properties") {
            for e in p.children().filter(|c| c.is_element()) {
                properties.push((e.tag_name().name().to_string(), e.text().unwrap_or("").trim().to_string()));
            }
        }
        let fill = |value: String| -> String {
            let mut v = value;
            for _ in 0..4 {
                let Some(start) = v.find("${") else { break };
                let Some(len) = v[start..].find('}') else { break };
                let key = &v[start + 2..start + len];
                let Some((_, with)) = properties.iter().find(|(k, _)| k == key) else { break };
                v = format!("{}{with}{}", &v[..start], &v[start + len + 1..]);
            }
            v
        };
        let prop = |key: &str| properties.iter().find(|(k, _)| k == key).map(|(_, v)| fill(v.clone()));
        let parent_artifact = child(project, "parent").and_then(|p| text_of(child(p, "artifactId")));
        let mut pom = Pom {
            artifact: text_of(child(project, "artifactId")).unwrap_or_default(),
            packaging: text_of(child(project, "packaging")).unwrap_or_else(|| "jar".into()),
            modules: child(project, "modules").map(|m| m.children().filter(|c| c.tag_name().name() == "module").filter_map(|c| c.text()).map(|t| t.trim().to_string()).collect()).unwrap_or_default(),
            spring_boot: parent_artifact.as_deref() == Some("spring-boot-starter-parent"),
            ..Pom::default()
        };
        let mut releases: Vec<Option<String>> = vec![prop("maven.compiler.release"), prop("maven.compiler.source"), prop("maven.compiler.target"), prop("java.version")];
        let build = child(project, "build");
        let plugins = build.into_iter().flat_map(|b| {
            let direct = child(b, "plugins");
            let managed = child(b, "pluginManagement").and_then(|m| child(m, "plugins"));
            direct.into_iter().chain(managed).flat_map(|p| p.children().filter(|c| c.tag_name().name() == "plugin"))
        });
        for plugin in plugins {
            let Some(id) = text_of(child(plugin, "artifactId")) else { continue };
            let config = child(plugin, "configuration");
            match id.as_str() {
                "maven-compiler-plugin" => {
                    releases.push(text_of(config.and_then(|c| child(c, "release"))).map(&fill));
                    releases.push(text_of(config.and_then(|c| child(c, "source"))).map(&fill));
                }
                "exec-maven-plugin" | "maven-jar-plugin" | "maven-shade-plugin" => {
                    let main = config.and_then(|c| c.descendants().find(|d| d.tag_name().name() == "mainClass")).and_then(|n| n.text()).map(|t| fill(t.trim().to_string()));
                    pom.main_class = pom.main_class.or(main);
                }
                "spring-boot-maven-plugin" => pom.spring_boot = true,
                _ => {}
            }
            pom.plugins.push(id);
        }
        if pom.main_class.is_none() {
            pom.main_class = prop("start-class").or_else(|| prop("exec.mainClass"));
        }
        pom.release = releases.into_iter().flatten().filter_map(|r| {
            let r = r.trim();
            let r = r.strip_prefix("1.").unwrap_or(r);
            r.parse::<u32>().ok()
        }).max();
        if let Some(deps) = child(project, "dependencies") {
            for d in deps.children().filter(|c| c.tag_name().name() == "dependency") {
                let (Some(group), Some(name)) = (text_of(child(d, "groupId")), text_of(child(d, "artifactId"))) else { continue };
                pom.declared.push(Declared { group: fill(group), name: fill(name), version: text_of(child(d, "version")).map(&fill), scope: text_of(child(d, "scope")) });
            }
        }
        Some(pom)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    pub fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-java-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The Gradle sample: `src/samples/gradle`, Orbit's app (Java) and
    /// core (Kotlin), copied to a folder of its own.
    pub fn orbit(name: &str) -> PathBuf {
        copy("gradle", name)
    }

    /// The Maven sample, `src/samples/maven`.
    pub fn maven(name: &str) -> PathBuf {
        copy("maven", name)
    }

    fn copy(sample: &str, name: &str) -> PathBuf {
        let dir = temp(name);
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/samples").join(sample);
        fn walk(from: &Path, to: &Path) {
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let target = to.join(e.file_name());
                if e.path().is_dir() {
                    std::fs::create_dir_all(&target).unwrap();
                    walk(&e.path(), &target);
                } else {
                    std::fs::copy(e.path(), target).unwrap();
                }
            }
        }
        walk(&from, &dir);
        // The wrapper's jar is binary: one that's there is enough.
        if dir.join("gradlew").is_file() {
            write(&dir, "gradle/wrapper/gradle-wrapper.jar", "PK");
        }
        dir
    }

    #[test]
    fn a_gradle_build_its_projects_release_and_plugins() {
        let dir = orbit("gradle-read");
        let b = Build::read(&dir.to_string_lossy());
        assert_eq!(b.tool, Tool::Gradle);
        assert_eq!(b.file.as_deref(), Some("settings.gradle.kts"));
        assert_eq!(b.wrapper, Wrapper { script: true, jar: true, version: Some("8.14.3".into()) });
        let names: Vec<&str> = b.modules.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["", "app", "core"]);
        let app = &b.modules[1];
        assert!(app.application && !app.kotlin && app.tests, "{app:?}");
        assert_eq!(app.main_class.as_deref(), Some("orbit.app.App"));
        assert_eq!(app.task("test"), ":app:test");
        let core = &b.modules[2];
        assert!(core.kotlin && !core.application);
        assert_eq!(b.release, Some(21));
        assert!(b.versions_plugin && !b.dependency_check && b.catalog);
        assert!(app.declared.iter().any(|d| d.coordinates() == "com.google.guava:guava" && d.version.as_deref() == Some("33.0.0-jre")), "{:?}", app.declared);
        assert_eq!(b.module_of("app/src/main/java/orbit/app/App.java").map(|m| m.name.as_str()), Some("app"));
        assert_eq!(b.module_of("README.md").map(|m| m.name.as_str()), Some(""));
        assert_eq!(crate::tests::line(&b.runner(Some("/home/u/orbit/gradlew"), Some("/opt/gradle/bin/gradle"), None).unwrap()), "/home/u/orbit/gradlew");
        assert_eq!(crate::tests::line(&b.runner(None, Some("/opt/gradle/bin/gradle"), None).unwrap()), "/opt/gradle/bin/gradle", "no wrapper found");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gradle_files_are_read_for_the_forms_people_write() {
        assert_eq!(gradle_release("java { toolchain { languageVersion = JavaLanguageVersion.of(17) } }"), Some(17));
        assert_eq!(gradle_release("kotlin { jvmToolchain(21) }"), Some(21));
        assert_eq!(gradle_release("sourceCompatibility = JavaVersion.VERSION_1_8"), Some(8));
        assert_eq!(gradle_release("sourceCompatibility = '11'"), Some(11));
        assert_eq!(gradle_release("tasks.withType<JavaCompile> { options.release.set(25) }"), Some(25));
        assert_eq!(gradle_release("version = '1.0'"), None);
        assert_eq!(gradle_main("application {\n    mainClassName = 'a.B'\n}"), Some("a.B".into()));
        assert_eq!(gradle_plugins("plugins {\n    `java-library`\n    kotlin(\"jvm\") version \"2.4.21\"\n    id(\"org.springframework.boot\") version \"4.1.2\"\n}\n"), ["java-library", "org.jetbrains.kotlin.jvm", "org.springframework.boot"]);
        assert_eq!(gradle_includes("rootProject.name = \"x\"\ninclude(\"app\", \":libs:core\")\ninclude 'tools'\n"), ["app", "libs:core", "tools"]);
        let declared = gradle_declared("dependencies {\n    implementation(\"org.slf4j:slf4j-api:2.0.17\")\n    testImplementation(platform(\"org.junit:junit-bom:6.1.3\"))\n    testImplementation 'org.junit.jupiter:junit-jupiter'\n    implementation(libs.guava)\n}\n");
        let rows: Vec<(String, Option<String>, Option<String>)> = declared.iter().map(|d| (d.coordinates(), d.version.clone(), d.scope.clone())).collect();
        assert_eq!(
            rows,
            [
                ("org.slf4j:slf4j-api".into(), Some("2.0.17".into()), Some("implementation".into())),
                ("org.junit:junit-bom".into(), Some("6.1.3".into()), Some("testImplementation".into())),
                ("org.junit.jupiter:junit-jupiter".into(), None, Some("testImplementation".into())),
            ]
        );
        // A plugin commented out isn't applied; a URL keeps its `//`.
        assert_eq!(uncommented("// id(\"x\")\nurl = \"https://a/b\" /* c */ d"), "\nurl = \"https://a/b\"  d");
        assert_eq!(distribution_version("distributionUrl=https\\://services.gradle.org/distributions/gradle-9.8.2-all.zip\n"), Some("9.8.2".into()));
        assert_eq!(distribution_version("distributionUrl=https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.9.11/apache-maven-3.9.11-bin.zip"), Some("3.9.11".into()));
    }

    #[test]
    fn a_maven_build_and_its_modules() {
        let dir = maven("maven-read");
        let b = Build::read(&dir.to_string_lossy());
        assert_eq!(b.tool, Tool::Maven);
        assert_eq!(b.wrapper, Wrapper { script: true, jar: true, version: Some("3.9.11".into()) });
        let names: Vec<(&str, &str)> = b.modules.iter().map(|m| (m.name.as_str(), m.dir.as_str())).collect();
        assert_eq!(names, [("", ""), ("orbit-core", "core"), ("orbit-api", "api")]);
        assert_eq!(b.release, Some(21));
        let api = &b.modules[2];
        assert!(api.spring_boot && api.application, "{api:?}");
        assert!(b.versions_plugin && b.dependency_check);
        assert!(api.declared.iter().any(|d| d.coordinates() == "org.springframework.boot:spring-boot-starter-web"));
        assert!(b.modules[1].declared.iter().any(|d| d.coordinates() == "org.junit.jupiter:junit-jupiter" && d.scope.as_deref() == Some("test")));
        assert_eq!(crate::tests::line(&b.runner(Some("./mvnw"), None, None).unwrap()), "./mvnw");
        assert_eq!(b.runner(None, Some("gradle"), None), None, "no mvn");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pom_says_its_release_and_main() {
        let p = Pom::parse("<project><artifactId>x</artifactId><properties><jdk>17</jdk><maven.compiler.release>${jdk}</maven.compiler.release><exec.mainClass>a.Main</exec.mainClass></properties></project>").unwrap();
        assert_eq!((p.release, p.main_class.as_deref(), p.packaging.as_str()), (Some(17), Some("a.Main"), "jar"));
        let p = Pom::parse("<project><build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><configuration><source>1.8</source></configuration></plugin></plugins></build></project>").unwrap();
        assert_eq!(p.release, Some(8));
        assert!(Pom::parse("<project").is_none());
        let none = temp("none");
        assert_eq!(Build::read(&none.to_string_lossy()), Build::default());
        std::fs::remove_dir_all(&none).unwrap();
    }
}
