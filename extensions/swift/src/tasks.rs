//! The tasks (`SPC p t`) of a Swift package: build, a release build, run
//! (each executable), test, resolve and update its dependencies, and
//! SwiftLint and the formatter on its sources, as each is installed and
//! wanted. The project's `.env` goes to each; a task of the same name in
//! `.fenix/tools.json` wins. An Xcode project has none: xcodebuild's
//! schemes are Xcode's.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The tasks for `project` in folder `root`, with what's on this machine:
/// `format` is the command that formats its files in place, when one
/// does; `lint`, whether SwiftLint lints it.
pub fn tasks(project: &Project, tools: &Tools, root: &str, format: Option<Command>, lint: bool) -> Tasks {
    let mut t = Tasks::default();
    let Some(package) = &project.package else { return t };
    let swift = Command::new(tools.swift.as_deref().unwrap_or("swift"));
    t.add("build", swift.clone().arg("build"));
    t.add("build release", swift.clone().args(["build", "-c", "release"]));
    for exe in &package.executables {
        let name = if package.executables.len() == 1 { "run".to_string() } else { format!("run {exe}") };
        t.add(&name, swift.clone().args(["run", exe.as_str()]));
    }
    t.add("test", swift.clone().arg("test"));
    t.add("resolve", swift.clone().args(["package", "resolve"]));
    t.add("update", swift.clone().args(["package", "update"]));
    if let (true, Some(swiftlint)) = (lint, &tools.swiftlint) {
        t.add("lint", Command::new(swiftlint).arg("lint"));
    }
    if let Some(format) = format.filter(|_| !project.dirs.is_empty()) {
        t.add("format", format.args(project.dirs.iter().cloned()));
    }
    t.env_file(&format!("{root}/.env"))
}

/// The formatter's command that writes the files it's given: swift-format
/// over folders (`-r`), SwiftFormat as it is.
pub fn in_place(cmd: &Command, apple: bool) -> Command {
    if apple {
        let mut cmd = cmd.clone();
        cmd.args.truncate(cmd.args.iter().rposition(|a| a == "format").map_or(0, |i| i + 1));
        cmd.args(["--in-place", "--recursive"])
    } else {
        Command::new(&cmd.program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, Package};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_package_with_its_tools_gets_them_all() {
        let package = Package { tools: Some("5.9".into()), executables: vec!["orbitctl".into(), "orbit-server".into()] };
        let project = Project { package: Some(package), dirs: vec!["Package.swift".into(), "Sources".into(), "Tests".into()], ..Project::default() };
        let tools = Tools { swift: Some("/usr/bin/swift".into()), swiftlint: Some("/t/swiftlint".into()), ..Tools::default() };
        let format = in_place(&Command::new("/usr/bin/swift").args(["format", "format", "--assume-filename", "${file}"]), true);
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere", Some(format), true)),
            rows(&[
                ("build", "/usr/bin/swift build"),
                ("build release", "/usr/bin/swift build -c release"),
                ("run orbitctl", "/usr/bin/swift run orbitctl"),
                ("run orbit-server", "/usr/bin/swift run orbit-server"),
                ("test", "/usr/bin/swift test"),
                ("resolve", "/usr/bin/swift package resolve"),
                ("update", "/usr/bin/swift package update"),
                ("lint", "/t/swiftlint lint"),
                ("format", "/usr/bin/swift format format --in-place --recursive Package.swift Sources Tests"),
            ])
        );
        assert_eq!(line(&in_place(&Command::new("/b/swift-format").args(["format", "--assume-filename", "${file}"]), true)), "/b/swift-format format --in-place --recursive");
        assert_eq!(line(&in_place(&Command::new("/h/swiftformat").args(["--stdinpath", "${file}", "--quiet"]), false)), "/h/swiftformat");
    }

    #[test]
    fn one_executable_is_run_and_the_env_goes_to_each() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let project = Project { package: Some(Package { tools: None, executables: vec!["Tool".into()] }), dirs: vec!["Sources".into()], ..Project::default() };
        let t = tasks(&project, &Tools::default(), &dir.to_string_lossy(), Some(Command::new("/h/swiftformat")), false);
        let names: Vec<String> = table(&t).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["build", "build release", "run", "test", "resolve", "update", "format"], "no SwiftLint");
        assert_eq!(line(&t.resolved()[2].cmd), "swift run Tool");
        assert_eq!(line(&t.resolved()[6].cmd), "/h/swiftformat Sources");
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        assert!(tasks(&Project { xcode: true, ..Project::default() }, &Tools::default(), "/nowhere", None, true).tasks.is_empty(), "Xcode's own");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
