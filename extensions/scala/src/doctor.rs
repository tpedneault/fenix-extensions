//! The project doctor's checks (`SPC p h`): the JDK -- which one, and a
//! warning below what Metals runs on -- Coursier and Metals with their fix
//! rows, the build tools the project uses, and what `.scalafmt.conf` says
//! of formatting. Metals, which is optional, is found where Coursier puts
//! it; where there's none, the rows say how to get one: Coursier first,
//! then Metals through it.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{find_version, Fix, Output, Program, Version};

use crate::project::{Project, Tool};
use crate::tools::{coursier_hints, hints, hints_with, Platform, Tools, JDK_HINTS, METALS, METALS_JAVA, SCALAFMT};

/// The fix rows' commands, in the manifest.
pub const INSTALL_COURSIER: &str = "scala.install.coursier";
pub const INSTALL_METALS: &str = "scala.install-metals";
pub const INSTALL_SCALAFMT: &str = "scala.install.scalafmt";

/// `javac -version`, read: its version, from the line that's `javac <version>`
/// (a JVM may print a `Picked up JAVA_TOOL_OPTIONS` line first).
pub fn javac_version(out: &Output) -> Option<Version> {
    out.text().lines().find_map(|l| l.trim().strip_prefix("javac ").and_then(find_version))
}

/// The JDK, held to what Metals runs on: `javac`, since a runtime has none.
/// It's needed in a Scala project (the build tools and Metals run on it) and
/// only noticed in any other. `source` says where it was looked for, so the
/// row says which JDK it is.
pub fn jdk(project: &Project, tools: &Tools, chosen: Option<&str>) -> Program {
    let found = tools.javac.clone().unwrap_or_else(|| "javac".into());
    let source = match chosen {
        Some(c) => format!(" (scala.jdk: {c})"),
        None => String::new(),
    };
    let program = JDK_HINTS.iter().fold(Program::new("javac", "JDK"), |p, h| p.hint(*h)).version_args(["-version"]).judge(move |out| match javac_version(out) {
        None if out.ok() => (Health::Warn, format!("{found}: its version couldn't be read")),
        None => (Health::Warn, format!("{found}: asking its version failed: {}", out.why())),
        Some(v) if v.major() < u64::from(METALS_JAVA) => (Health::Warn, format!("{v} ({found}){source}: Metals {METALS} runs on Java {METALS_JAVA} or later")),
        Some(v) => (Health::Ok, format!("{v} ({found}){source}")),
    });
    if project.is_scala() {
        program
    } else {
        program.optional()
    }
}

/// Coursier, when there's no Metals yet, with a fix row that installs its
/// release for this machine.
fn coursier(tools: &Tools, platform: &Platform) -> Program {
    let name = if tools.coursier.as_deref().is_some_and(|c| c.rsplit(['/', '\\']).next().is_some_and(|n| n.starts_with("coursier"))) { "coursier" } else { "cs" };
    let mut p = coursier_hints().into_iter().fold(Program::new(name, "Coursier"), Program::hint).version_args(["version"]).optional();
    if platform.installs().iter().any(|i| i.name == "coursier") {
        p = p.fix(&format!("Install Coursier {}", crate::tools::COURSIER), INSTALL_COURSIER, Fix::Install("coursier".into()));
    }
    p
}

/// Metals: asked its version, which takes a moment (it checks Maven Central
/// for the Scala versions it supports too). Its fix row runs Coursier's `cs
/// install` as a task, so it's offered once there's a Coursier.
fn metals(tools: &Tools) -> Program {
    let mut p = hints("metals").into_iter().fold(Program::new("metals", "Metals"), Program::hint).version_args(["--version"]).timeout(30_000).optional();
    if tools.coursier.is_some() {
        p = p.fix(&format!("Install Metals {METALS}"), INSTALL_METALS, Fix::Own);
    }
    p
}

/// A build tool the project uses, asked its version: needed, since its
/// tasks run it. Gradle and Maven aren't asked: the Java pack checks their
/// wrappers, and their tasks only run what's there.
fn build_tool(tool: Tool) -> Option<Program> {
    let (name, label, args, first): (&str, &str, &[&str], &[&str]) = match tool {
        Tool::Sbt => ("sbt", "sbt", &["--version"], &[]),
        Tool::Mill => ("mill", "Mill", &["--version"], &["./mill"]),
        Tool::ScalaCli => ("scala-cli", "scala-cli", &["version"], &[]),
        Tool::Gradle | Tool::Maven => return None,
    };
    Some(hints_with(first, name).into_iter().fold(Program::new(name, label), Program::hint).version_args(args.iter().copied()).timeout(30_000))
}

/// The checks that run a program, for `project`: the JDK, Coursier and
/// Metals, and each build tool.
pub fn programs(project: &Project, tools: &Tools, platform: &Platform, chosen: Option<&str>) -> Vec<Program> {
    let mut out = vec![jdk(project, tools, chosen)];
    if tools.metals.is_none() {
        out.push(coursier(tools, platform));
    }
    out.push(metals(tools));
    for tool in &project.tools {
        out.extend(build_tool(*tool));
    }
    out
}

/// What the pack says of formatting: Metals formats (the format command) with the
/// scalafmt `.scalafmt.conf` pins; on save, only an extension's formatter runs, and
/// the pack has one where it can -- a native scalafmt, which formats only with
/// its own version -- so the row says which it is, and has a fix that installs
/// it when the project pins that version. `native` is whether it's installed.
pub fn formatting(project: &Project, native: bool, can_install: bool) -> Option<Check> {
    if !project.is_scala() {
        return None;
    }
    let check = |health, detail: String| Check::new(Section::Project, "scalafmt", health, detail);
    Some(match &project.scalafmt {
        Some(s) => match (&s.version, &s.dialect) {
            (Some(v), dialect) if v == SCALAFMT => {
                let said = match dialect {
                    Some(d) => format!("{v}, dialect {d} (.scalafmt.conf)"),
                    None => format!("{v} (.scalafmt.conf)"),
                };
                match (native, can_install) {
                    (true, _) => check(Health::Ok, format!("{said}: formatted by Metals, and on save by the native scalafmt")),
                    (false, true) => check(Health::Warn, format!("{said}: formatted by Metals; the native scalafmt, which formats on save, isn't installed")).fix(&format!("Install scalafmt {SCALAFMT}"), INSTALL_SCALAFMT),
                    (false, false) => check(Health::Ok, format!("{said}: formatted by Metals")),
                }
            }
            (Some(v), dialect) => {
                let said = match dialect {
                    Some(d) => format!("{v}, dialect {d} (.scalafmt.conf)"),
                    None => format!("{v} (.scalafmt.conf)"),
                };
                check(Health::Ok, format!("{said}: formatted by Metals, which runs that version; on save only {SCALAFMT}'s native scalafmt can"))
            }
            (None, _) => check(Health::Warn, ".scalafmt.conf has no version: Metals runs the one it's built with".into()),
        },
        None => check(Health::Warn, "no .scalafmt.conf: Metals formats with scalafmt's defaults".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::Output;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn scala() -> Project {
        Project { tools: vec![Tool::Sbt], ..Project::default() }
    }

    fn linux() -> Platform {
        Platform::new("linux", "x86_64")
    }

    fn labels(p: &[Program]) -> Vec<(&str, bool)> {
        p.iter().map(|p| (p.label.as_str(), p.required)).collect()
    }

    #[test]
    fn the_jdk_says_which_it_is_and_warns_below_what_metals_runs_on() {
        let tools = Tools { javac: Some("/usr/lib/jvm/java-21/bin/javac".into()), ..Tools::default() };
        let p = jdk(&scala(), &tools, None);
        assert_eq!((p.name.as_str(), p.label.as_str(), p.version_args.as_slice()), ("javac", "JDK", ["-version".to_string()].as_slice()));
        assert!(p.hints.contains(&"${setting:scala.jdk}".to_string()) && p.hints.contains(&"$JAVA_HOME/bin".to_string()));

        // As javac 21 printed it, a JVM's own first line before.
        let c = p.evaluate(Some("/usr/lib/jvm/java-21/bin/javac"), Some(&said(include_str!("samples/javac-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "21.0.12.1 (/usr/lib/jvm/java-21/bin/javac)"));
        assert_eq!(javac_version(&said("javac 17.0.9\n")).map(|v| v.major()), Some(17));
        assert_eq!(javac_version(&said("Picked up JAVA_TOOL_OPTIONS: -Xmx1g\njavac 11.0.2\n")).map(|v| v.major()), Some(11), "not the option's number");

        let old = jdk(&scala(), &Tools { javac: Some("/opt/jdk11/bin/javac".into()), ..Tools::default() }, None).evaluate(Some("/opt/jdk11/bin/javac"), Some(&said("javac 11.0.2\n")));
        assert_eq!((old.health, old.detail.as_str()), (Health::Warn, "11.0.2 (/opt/jdk11/bin/javac): Metals 1.6.9 runs on Java 17 or later"));
        let chosen = jdk(&scala(), &Tools { javac: Some("/opt/jdk21/bin/javac".into()), ..Tools::default() }, Some("/opt/jdk21")).evaluate(Some("/opt/jdk21/bin/javac"), Some(&said("javac 21.0.1\n")));
        assert_eq!(chosen.detail, "21.0.1 (/opt/jdk21/bin/javac) (scala.jdk: /opt/jdk21)");
        let broken = p.evaluate(Some("/x/javac"), Some(&Output { code: Some(1), stdout: String::new(), stderr: "boom".into() }));
        assert_eq!(broken.health, Health::Warn);
        // Needed in a Scala project, a warning in any other.
        assert_eq!(p.evaluate(None, None).health, Health::Bad);
        assert_eq!(jdk(&Project::default(), &Tools::default(), None).evaluate(None, None).health, Health::Warn);
    }

    #[test]
    fn coursier_and_metals_have_fix_rows_where_there_is_something_to_install() {
        let p = programs(&scala(), &Tools::default(), &linux(), None);
        assert_eq!(labels(&p), [("JDK", true), ("Coursier", false), ("Metals", false), ("sbt", true)]);
        assert_eq!(p[1].fix.as_ref().map(|f| (f.0.as_str(), f.1.as_str(), f.2.clone())), Some(("Install Coursier 2.1.26", "scala.install.coursier", Fix::Install("coursier".into()))));
        assert_eq!(p[2].fix, None, "no Coursier to install Metals with yet");
        assert_eq!(p[1].hints[0], "/tools/coursier");
        assert_eq!(p[1].version_args, ["version"]);

        let cs = Tools { coursier: Some("/t/cs".into()), ..Tools::default() };
        let p = programs(&scala(), &cs, &linux(), None);
        assert_eq!(p[2].fix.as_ref().map(|f| (f.0.as_str(), f.1.as_str(), f.2.clone())), Some(("Install Metals 1.6.9", "scala.install-metals", Fix::Own)));
        assert_eq!(p[2].timeout, Some(30_000));

        // Metals found: Coursier isn't asked about.
        let found = Tools { metals: Some("/t/metals".into()), coursier: Some("/t/cs".into()), ..Tools::default() };
        assert_eq!(labels(&programs(&scala(), &found, &linux(), None)), [("JDK", true), ("Metals", false), ("sbt", true)]);
        // Coursier under its other name.
        let named = Tools { coursier: Some("/usr/local/bin/coursier".into()), ..Tools::default() };
        assert_eq!(programs(&scala(), &named, &linux(), None)[1].name, "coursier");

        // No release for this machine: no row to install Coursier.
        let none = programs(&scala(), &Tools::default(), &Platform::new("windows", "aarch64"), None);
        assert_eq!(none[1].fix, None);
    }

    #[test]
    fn each_tool_is_asked_its_version_as_it_printed_it() {
        let p = programs(&Project { tools: vec![Tool::Sbt, Tool::Mill, Tool::ScalaCli, Tool::Gradle, Tool::Maven], ..Project::default() }, &Tools::default(), &linux(), None);
        assert_eq!(labels(&p), [("JDK", true), ("Coursier", false), ("Metals", false), ("sbt", true), ("Mill", true), ("scala-cli", true)], "Gradle and Maven are the Java pack's");
        let version = |label: &str, text: &str| {
            let program = p.iter().find(|p| p.label == label).unwrap();
            let c = program.evaluate(Some(&format!("/t/{}", program.name)), Some(&said(text)));
            (c.health, c.detail)
        };
        // What each printed, recorded (src/samples/README.md).
        assert_eq!(version("sbt", include_str!("samples/sbt-version.txt")), (Health::Ok, "1.12.15 (/t/sbt)".into()), "the project's, not the runner's");
        assert_eq!(version("Mill", include_str!("samples/mill-version.txt")), (Health::Ok, "0.11.13 (/t/mill)".into()));
        assert_eq!(version("scala-cli", include_str!("samples/scala-cli-version.txt")), (Health::Ok, "1.19.0 (/t/scala-cli)".into()));
        assert_eq!(version("Metals", include_str!("samples/metals-version.txt")), (Health::Ok, "1.6.9 (/t/metals)".into()));
        assert_eq!(version("Coursier", include_str!("samples/cs-version.txt")), (Health::Ok, "2.1.26 (/t/cs)".into()));
        let args = |label: &str| p.iter().find(|p| p.label == label).unwrap().version_args.clone();
        assert_eq!((args("sbt"), args("Mill"), args("scala-cli"), args("Metals")), (vec!["--version".to_string()], vec!["--version".to_string()], vec!["version".to_string()], vec!["--version".to_string()]));
        assert_eq!(p.iter().find(|p| p.label == "Mill").unwrap().hints[0], "./mill", "the project's wrapper first");
        // A tool the project needs and doesn't have fails; Metals warns.
        assert_eq!(p[3].evaluate(None, None).health, Health::Bad);
        assert_eq!(p[2].evaluate(None, None).health, Health::Warn);
    }

    #[test]
    fn formatting_says_what_scalafmt_conf_pins_and_which_formatter_runs_on_save() {
        use crate::project::Scalafmt;
        let with = |version: Option<&str>, dialect: Option<&str>| Project { scalafmt: Some(Scalafmt { version: version.map(str::to_string), dialect: dialect.map(str::to_string) }), ..scala() };
        // The version the native scalafmt is: formatted on save too, once it's installed.
        let c = formatting(&with(Some("3.11.5"), Some("scala3")), true, true).unwrap();
        assert_eq!((c.health, c.detail.as_str(), c.section, c.fix), (Health::Ok, "3.11.5, dialect scala3 (.scalafmt.conf): formatted by Metals, and on save by the native scalafmt", Section::Project, None));
        let c = formatting(&with(Some("3.11.5"), None), false, true).unwrap();
        assert_eq!((c.health, c.fix.as_ref().map(|f| f.1.as_str())), (Health::Warn, Some("scala.install.scalafmt")));
        assert_eq!(c.detail, "3.11.5 (.scalafmt.conf): formatted by Metals; the native scalafmt, which formats on save, isn't installed");
        let c = formatting(&with(Some("3.11.5"), None), false, false).unwrap();
        assert_eq!((c.health, c.fix), (Health::Ok, None), "no release for this machine: nothing to install");
        // Another version: Metals runs it, and on save nothing does.
        let c = formatting(&with(Some("3.8.3"), None), true, true).unwrap();
        assert_eq!((c.health, c.fix), (Health::Ok, None));
        assert_eq!(c.detail, "3.8.3 (.scalafmt.conf): formatted by Metals, which runs that version; on save only 3.11.5's native scalafmt can");
        assert_eq!(formatting(&with(None, None), true, true).unwrap().health, Health::Warn);
        let none = Project { scalafmt: None, ..scala() };
        let c = formatting(&none, false, true).unwrap();
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "no .scalafmt.conf: Metals formats with scalafmt's defaults"));
        assert!(formatting(&Project::default(), false, true).is_none(), "not a Scala project");
    }
}
