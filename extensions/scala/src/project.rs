//! What the pack knows of the project, from its files at the top: which
//! build tools it uses (sbt, Mill, scala-cli, Gradle, Maven), the projects
//! and modules they name, the scalafmt version `.scalafmt.conf` pins and
//! whether scalafix is set up. Nothing is run to find out.

use std::path::Path;

/// A build tool a Scala project is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Sbt,
    Mill,
    ScalaCli,
    Gradle,
    Maven,
}

impl Tool {
    /// As the doctor and the tasks say it.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Sbt => "sbt",
            Tool::Mill => "Mill",
            Tool::ScalaCli => "scala-cli",
            Tool::Gradle => "Gradle",
            Tool::Maven => "Maven",
        }
    }
}

/// The files that make a folder Mill's: its build file, in each of the
/// names Mill has had.
pub const MILL_BUILDS: &[&str] = &["build.mill", "build.mill.scala", "build.sc"];

/// The files that make a folder scala-cli's.
pub const SCALA_CLI_MARKERS: &[&str] = &["project.scala", ".scala-build", ".bsp/scala-cli.json"];

/// What `.scalafmt.conf` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scalafmt {
    /// `version = "3.8.3"`: the scalafmt Metals runs for this project.
    pub version: Option<String>,
    /// `runner.dialect = scala3`.
    pub dialect: Option<String>,
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The build tools it uses, in this order of preference.
    pub tools: Vec<Tool>,
    /// sbt's projects, as `build.sbt` names them (`lazy val core = project`).
    pub sbt_projects: Vec<String>,
    /// `project/build.properties`'s `sbt.version`.
    pub sbt_version: Option<String>,
    /// Mill's modules, as its build file names them, and whether the build
    /// has a root module of its own (`object foo extends RootModule`).
    pub mill_modules: Vec<String>,
    pub mill_root: bool,
    pub scalafmt: Option<Scalafmt>,
    /// sbt-scalafix in `project/plugins.sbt`, Mill's `ScalafixModule`, or a
    /// `.scalafix.conf` for scala-cli.
    pub scalafix: Option<Tool>,
    /// A `.scala` or `.sc` file at the top, or under `src/`: Scala, even
    /// with no build file.
    pub sources: bool,
    /// The `.sc` and `.scala` files at the top.
    pub scripts: Vec<String>,
    /// Gradle's `application` plugin, which has `run`.
    pub gradle_application: bool,
}

fn read(root: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(root.join(file)).ok()
}

/// The identifiers `lazy val`/`val` bind to a `project`: sbt's projects.
pub fn parse_sbt_projects(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let rest = line.strip_prefix("lazy val ").or_else(|| line.strip_prefix("val "));
        let Some((name, value)) = rest.and_then(|r| r.split_once('=')) else { continue };
        let name = name.trim().split([':', ' ']).next().unwrap_or("").trim_matches('`');
        let value = value.trim().trim_start_matches('(').trim_start();
        let is_project = ["project", "crossProject", "sbt.project"].iter().any(|p| value.strip_prefix(p).is_some_and(|r| !r.starts_with(|c: char| c.is_alphanumeric() || c == '_')));
        if is_project && !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// `sbt.version=1.10.7` in `project/build.properties`.
pub fn parse_sbt_version(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix("sbt.version").and_then(|r| r.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string())).filter(|v| !v.is_empty())
}

/// What a Mill build file's top level declares: its modules (`object core
/// extends ScalaModule`, in the traits that mean Scala), and whether one
/// is the root's. Test objects are inside a module, indented, so not
/// counted.
pub fn parse_mill_modules(text: &str) -> (Vec<String>, bool) {
    let mut modules = Vec::new();
    let mut root = false;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("object ") else { continue };
        let Some((name, extends)) = rest.split_once(" extends ") else { continue };
        let name = name.trim().trim_matches('`');
        let extends = extends.split('{').next().unwrap_or("");
        let scala = ["ScalaModule", "SbtModule", "MavenModule", "ScalaJSModule", "ScalaNativeModule", "CrossScalaModule", "CrossSbtModule", "ScalaCliModule"];
        if !scala.iter().any(|m| extends.contains(m)) && !extends.contains("RootModule") {
            continue;
        }
        if extends.contains("RootModule") {
            root = true;
        } else if !name.is_empty() && !modules.iter().any(|m| m == name) {
            modules.push(name.to_string());
        }
    }
    (modules, root)
}

/// `.scalafmt.conf`'s `version` and `runner.dialect`: HOCON, `key = value` or
/// `key=value`, quoted or not.
pub fn parse_scalafmt(text: &str) -> Scalafmt {
    let value = |key: &str| {
        text.lines().find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?;
            let v = rest.split('#').next().unwrap_or("").trim().trim_matches('"');
            (!v.is_empty()).then(|| v.to_string())
        })
    };
    Scalafmt { version: value("version"), dialect: value("runner.dialect").or_else(|| value("runner.dialect ")) }
}

/// Whether a Gradle build file or a `pom.xml` says Scala.
pub fn mentions_scala(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["'scala'", "\"scala\"", "scala-library", "scala3-library", "scala-lang", "scala-maven-plugin", "net.alchim31", "org.gradle.scala", "scala-plugin"].iter().any(|k| t.contains(k))
}

/// A top-level `.scala` or `.sc` file's names, sorted.
fn scripts(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".scala") || n.ends_with(".sc"))
        .collect();
    found.sort();
    found
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let has = |f: &str| root.join(f).exists();
        let mut p = Project { scripts: scripts(root), ..Project::default() };

        p.sbt_version = read(root, "project/build.properties").and_then(|t| parse_sbt_version(&t));
        if let Some(text) = read(root, "build.sbt") {
            p.sbt_projects = parse_sbt_projects(&text);
        }
        if has("build.sbt") || p.sbt_version.is_some() {
            p.tools.push(Tool::Sbt);
        }
        if let Some(text) = MILL_BUILDS.iter().find_map(|b| read(root, b)) {
            (p.mill_modules, p.mill_root) = parse_mill_modules(&text);
            p.tools.push(Tool::Mill);
            if text.contains("ScalafixModule") {
                p.scalafix = p.scalafix.or(Some(Tool::Mill));
            }
        }
        // scala-cli: its own files, or -- with no other build tool --
        // scripts and sources at the top.
        if SCALA_CLI_MARKERS.iter().any(|m| has(m)) || (p.tools.is_empty() && !p.scripts.is_empty()) {
            p.tools.push(Tool::ScalaCli);
        }
        let gradle = ["build.gradle", "build.gradle.kts"].iter().filter_map(|f| read(root, f)).any(|t| mentions_scala(&t));
        if gradle || has_one_down(root, &["build.gradle", "build.gradle.kts"]) {
            p.tools.push(Tool::Gradle);
        }
        let maven = read(root, "pom.xml").is_some_and(|t| mentions_scala(&t)) || has_one_down(root, &["pom.xml"]);
        if maven {
            p.tools.push(Tool::Maven);
        }

        if read(root, "project/plugins.sbt").is_some_and(|t| t.contains("sbt-scalafix")) {
            p.scalafix = Some(Tool::Sbt);
        }
        if p.scalafix.is_none() && has(".scalafix.conf") && p.tools.contains(&Tool::ScalaCli) {
            p.scalafix = Some(Tool::ScalaCli);
        }
        p.scalafmt = read(root, ".scalafmt.conf").map(|t| parse_scalafmt(&t));
        p.sources = !p.scripts.is_empty() || root.join("src/main/scala").is_dir() || root.join("src/test/scala").is_dir();
        p.gradle_application = ["build.gradle", "build.gradle.kts"].iter().filter_map(|f| read(root, f)).any(|t| t.contains("application"));
        p
    }

    /// Whether it's a Scala project: a build tool, a `.scalafmt.conf`, or
    /// sources.
    pub fn is_scala(&self) -> bool {
        !self.tools.is_empty() || self.scalafmt.is_some() || self.sources
    }

    /// The tool its tasks are for.
    pub fn primary(&self) -> Option<Tool> {
        self.tools.first().copied()
    }

    pub fn uses(&self, tool: Tool) -> bool {
        self.tools.contains(&tool)
    }
}

/// A Gradle or Maven Scala project's build file one folder down (`app/`,
/// `core/`), which says Scala there: a multi-project build's.
fn has_one_down(root: &Path, files: &[&str]) -> bool {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .any(|e| files.iter().any(|f| std::fs::read_to_string(e.path().join(f)).is_ok_and(|t| mentions_scala(&t))))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test: its name is the test's own, so no two
    /// share one.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-scala-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    const SBT: &str = include_str!("samples/build.sbt");
    const MILL: &str = include_str!("samples/build.mill");

    #[test]
    fn sbts_projects_are_the_vals_bound_to_a_project() {
        assert_eq!(parse_sbt_projects(SBT), ["root", "core", "cli", "docs"]);
        assert_eq!(parse_sbt_projects("lazy val a = project\nval notAProject = projection\nlazy val b = (project.in(file(\"b\")))\nlazy val c = crossProject(JVMPlatform)\nval x = Seq(project)\n"), ["a", "b", "c"]);
        assert_eq!(parse_sbt_version("sbt.version=1.10.7\n"), Some("1.10.7".into()));
        assert_eq!(parse_sbt_version("# pinned\nsbt.version = 2.0.12\n"), Some("2.0.12".into()));
        assert_eq!(parse_sbt_version("nothing"), None);
    }

    #[test]
    fn mills_modules_are_the_objects_that_extend_a_scala_module() {
        assert_eq!(parse_mill_modules(MILL), (vec!["core".to_string(), "app".to_string()], false));
        assert_eq!(parse_mill_modules("object `package` extends RootModule with ScalaModule {\n  def scalaVersion = \"3.7.3\"\n}\n"), (Vec::new(), true));
        assert_eq!(parse_mill_modules("object util extends Module {\n}\n"), (Vec::new(), false), "not Scala");
    }

    #[test]
    fn scalafmts_version_and_dialect_are_read() {
        let s = parse_scalafmt("# formatting\nversion = \"3.8.3\"\nrunner.dialect = scala3\nmaxColumn = 100\n");
        assert_eq!((s.version.as_deref(), s.dialect.as_deref()), (Some("3.8.3"), Some("scala3")));
        let s = parse_scalafmt("version=3.9.1 # pinned\nrunner.dialect=scala213source3\n");
        assert_eq!((s.version.as_deref(), s.dialect.as_deref()), (Some("3.9.1"), Some("scala213source3")));
        assert_eq!(parse_scalafmt("maxColumn = 80\n"), Scalafmt::default(), "no version: Metals' default");
    }

    #[test]
    fn each_build_tool_is_found_by_its_files() {
        let dir = temp("detect");
        let root = dir.to_string_lossy().into_owned();
        assert!(!Project::read(&root).is_scala());

        write(&dir, "build.sbt", SBT);
        write(&dir, "project/build.properties", "sbt.version=1.10.7\n");
        write(&dir, "project/plugins.sbt", "addSbtPlugin(\"ch.epfl.scala\" % \"sbt-scalafix\" % \"0.14.2\")\n");
        write(&dir, ".scalafmt.conf", "version = \"3.8.3\"\n");
        let p = Project::read(&root);
        assert_eq!(p.tools, [Tool::Sbt]);
        assert_eq!((p.sbt_version.as_deref(), p.scalafix, p.scalafmt.as_ref().and_then(|s| s.version.as_deref())), (Some("1.10.7"), Some(Tool::Sbt), Some("3.8.3")));
        assert_eq!(p.primary(), Some(Tool::Sbt));

        write(&dir, "build.mill", MILL);
        write(&dir, "project.scala", "//> using scala 3.7.3\n");
        let p = Project::read(&root);
        assert_eq!(p.tools, [Tool::Sbt, Tool::Mill, Tool::ScalaCli], "the order they're offered in");
        assert_eq!(p.mill_modules, ["core", "app"]);
        std::fs::remove_dir_all(&dir).unwrap();

        // Scripts with no build file: scala-cli's.
        let dir = temp("detect-scripts");
        let root = dir.to_string_lossy().into_owned();
        write(&dir, "hello.sc", "println(\"hi\")\n");
        let p = Project::read(&root);
        assert_eq!((p.tools.as_slice(), p.scripts.as_slice(), p.is_scala()), ([Tool::ScalaCli].as_slice(), ["hello.sc".to_string()].as_slice(), true));
        std::fs::remove_dir_all(&dir).unwrap();

        // Gradle and Maven only when their build says Scala.
        let dir = temp("detect-jvm");
        let root = dir.to_string_lossy().into_owned();
        write(&dir, "build.gradle.kts", "plugins {\n    java\n    application\n}\n");
        write(&dir, "pom.xml", "<project><dependencies><dependency><artifactId>guava</artifactId></dependency></dependencies></project>");
        assert!(Project::read(&root).tools.is_empty(), "a Java project stays the Java pack's");
        write(&dir, "build.gradle.kts", "plugins {\n    scala\n    id(\"scala\")\n}\ndependencies {\n    implementation(\"org.scala-lang:scala3-library_3:3.7.3\")\n}\n");
        write(&dir, "pom.xml", "<project><build><plugins><plugin><groupId>net.alchim31.maven</groupId><artifactId>scala-maven-plugin</artifactId></plugin></plugins></build></project>");
        assert_eq!(Project::read(&root).tools, [Tool::Gradle, Tool::Maven]);
        std::fs::remove_dir_all(&dir).unwrap();

        // A multi-project Gradle build: the Scala plugin one folder down.
        let dir = temp("detect-gradle-sub");
        let root = dir.to_string_lossy().into_owned();
        write(&dir, "settings.gradle", "include 'app'\n");
        write(&dir, "app/build.gradle", "plugins { id 'scala' }\n");
        assert_eq!(Project::read(&root).tools, [Tool::Gradle]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_scalafmt_conf_or_scala_sources_alone_make_it_scala() {
        let dir = temp("detect-fmt");
        write(&dir, ".scalafmt.conf", "maxColumn = 100\n");
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.tools.is_empty() && p.is_scala() && p.primary().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
        let dir = temp("detect-src");
        write(&dir, "src/main/scala/Main.scala", "object Main\n");
        assert!(Project::read(&dir.to_string_lossy()).sources);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
