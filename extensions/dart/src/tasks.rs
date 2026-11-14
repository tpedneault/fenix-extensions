//! The tasks (`SPC p t`): what you'd type in a terminal, worked out from
//! the project. A Dart package gets `dart pub get`, `dart run`, `dart
//! test`, `dart analyze`; a Flutter app, which a pubspec that depends on
//! Flutter says it is, gets Flutter's own (`flutter pub get`, `flutter
//! run`, `flutter test`, a build for each platform it has a folder for,
//! `flutter doctor`). `dart fix --apply` rewrites files all over the
//! project, so it's a task you pick by name, last but one, with no key.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Project, Tests};
use crate::tools::Tools;

/// Where `flutter run` writes its process id, so a hot reload (SIGUSR1)
/// or restart (SIGUSR2) can be sent to it. Under `/project`.
pub const PID_FILE: &str = ".dart_tool/fenix-flutter.pid";

/// The arguments `flutter run` takes for the pid file.
pub fn pid_args() -> [String; 2] {
    ["--pid-file".into(), PID_FILE.into()]
}

/// `flutter run`, as the task and the device picker start it: `device`
/// is a device id.
pub fn flutter_run(tools: &Tools, device: Option<&str>) -> Command {
    let mut cmd = tools.flutter_cmd().arg("run").args(pid_args());
    if let Some(device) = device {
        cmd = cmd.args(["-d", device]);
    }
    cmd
}

/// What `flutter build` makes for a folder, on `os`: iOS and macOS only
/// build on a Mac, Linux on Linux, Windows on Windows.
pub fn builds(platform: &str, os: &str) -> &'static [&'static str] {
    match (platform, os) {
        ("android", _) => &["apk", "appbundle"],
        ("web", _) => &["web"],
        ("ios", "macos") => &["ios"],
        ("macos", "macos") => &["macos"],
        ("linux", "linux") => &["linux"],
        ("windows", "windows") => &["windows"],
        _ => &[],
    }
}

/// The tasks for `project`, with the programs found, on `os`.
pub fn tasks(project: &Project, tools: &Tools, os: &str) -> Tasks {
    let mut tasks = Tasks::default();
    if !project.is_dart() {
        return tasks;
    }
    let flutter = project.is_flutter();
    let dart = tools.dart_cmd();
    let pub_tool = tools.pub_cmd(flutter);
    let member = project.pubspec.as_ref().is_some_and(|p| p.member);

    if !member {
        tasks.add("pub get", pub_tool.clone().args(["pub", "get"]));
    }
    if flutter {
        tasks.add("run", flutter_run(tools, None));
    } else if project.has_default_bin() {
        tasks.add("run", dart.clone().arg("run"));
        tasks.add("run (attachable)", dart.clone().args(["run", "--enable-vm-service", "--write-service-info=.dart_tool/fenix-vm-service.json"]));
    }
    if !flutter {
        for bin in project.bins.iter().filter(|b| **b != project.name()) {
            tasks.add(&format!("run {bin}"), dart.clone().args(["run", &format!("bin/{bin}.dart")]));
        }
        if project.bins.is_empty() && project.has_main {
            tasks.add("run main", dart.clone().args(["run", "lib/main.dart"]));
        }
    }
    match project.tests() {
        Some(Tests::Flutter) => tasks.add("test", tools.flutter_cmd().arg("test")),
        Some(Tests::Dart) => tasks.add("test", dart.clone().arg("test")),
        None => {}
    }
    tasks.add("analyze", if flutter { tools.flutter_cmd().arg("analyze") } else { dart.clone().arg("analyze") });
    tasks.add("format", dart.clone().args(["format", "."]));

    if flutter {
        for platform in project.platforms.iter() {
            for target in builds(platform, os) {
                tasks.add(&format!("build {target}"), tools.flutter_cmd().args(["build", target]));
            }
        }
        tasks.add("devices", tools.flutter_cmd().arg("devices"));
        tasks.add("doctor", tools.flutter_cmd().args(["doctor", "-v"]));
        tasks.add("clean", tools.flutter_cmd().arg("clean"));
    } else if let Some(bin) = project.bins.iter().find(|b| **b == project.name()).or(project.bins.first()) {
        tasks.add("compile exe", dart.clone().args(["compile", "exe", &format!("bin/{bin}.dart")]));
    }
    tasks.add("pub outdated", pub_tool.clone().args(["pub", "outdated"]));
    if !member {
        tasks.add("pub upgrade", pub_tool.args(["pub", "upgrade"]));
    }
    // Rewrites files across the project: after the rest, named for what it does.
    tasks.add("fix preview", dart.clone().args(["fix", "--dry-run"]));
    tasks.add("fix apply", dart.args(["fix", "--apply"]));
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, APP, CONSOLE};

    fn names(t: &Tasks) -> Vec<String> {
        t.resolved().iter().map(|t| t.name.clone()).collect()
    }

    fn line(t: &Tasks, name: &str) -> String {
        let task = t.resolved().into_iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no task {name}: {:?}", names(t)));
        std::iter::once(task.cmd.program.clone()).chain(task.cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found() -> Tools {
        Tools { dart: Some("/sdk/bin/dart".into()), flutter: Some("/f/bin/flutter".into()) }
    }

    #[test]
    fn a_dart_package_has_dart_commands() {
        let dir = temp("tasks-dart");
        write(&dir, &[("pubspec.yaml", CONSOLE), ("bin/orbit.dart", ""), ("bin/seed.dart", ""), ("test/a_test.dart", "")]);
        let t = tasks(&Project::read(&dir.to_string_lossy()), &found(), "linux");
        assert_eq!(names(&t), ["pub get", "run", "run (attachable)", "run seed", "test", "analyze", "format", "compile exe", "pub outdated", "pub upgrade", "fix preview", "fix apply"]);
        assert_eq!(line(&t, "pub get"), "/sdk/bin/dart pub get");
        assert_eq!(line(&t, "run"), "/sdk/bin/dart run");
        assert_eq!(line(&t, "run seed"), "/sdk/bin/dart run bin/seed.dart");
        assert_eq!(line(&t, "test"), "/sdk/bin/dart test");
        assert_eq!(line(&t, "analyze"), "/sdk/bin/dart analyze");
        assert_eq!(line(&t, "format"), "/sdk/bin/dart format .");
        assert_eq!(line(&t, "compile exe"), "/sdk/bin/dart compile exe bin/orbit.dart");
        assert_eq!(line(&t, "fix apply"), "/sdk/bin/dart fix --apply");
        assert_eq!(names(&t).last().map(String::as_str), Some("fix apply"), "after the rest");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flutter_app_has_flutter_commands_and_the_builds_it_can_make() {
        let dir = temp("tasks-flutter");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", ""), ("test/w_test.dart", ""), ("android/a", ""), ("ios/a", ""), ("web/a", ""), ("linux/a", ""), ("windows/a", "")]);
        let project = Project::read(&dir.to_string_lossy());
        let t = tasks(&project, &found(), "linux");
        assert_eq!(names(&t), ["pub get", "run", "test", "analyze", "format", "build apk", "build appbundle", "build web", "build linux", "devices", "doctor", "clean", "pub outdated", "pub upgrade", "fix preview", "fix apply"]);
        assert_eq!(line(&t, "pub get"), "/f/bin/flutter pub get");
        assert_eq!(line(&t, "run"), "/f/bin/flutter run --pid-file .dart_tool/fenix-flutter.pid");
        assert_eq!(line(&t, "test"), "/f/bin/flutter test");
        assert_eq!(line(&t, "analyze"), "/f/bin/flutter analyze");
        assert_eq!(line(&t, "build appbundle"), "/f/bin/flutter build appbundle");
        assert_eq!(line(&t, "doctor"), "/f/bin/flutter doctor -v");
        assert_eq!(line(&t, "fix apply"), "/sdk/bin/dart fix --apply");
        // On a Mac: the iOS build, not Linux's or Windows'.
        let mac = names(&tasks(&project, &found(), "macos"));
        assert!(mac.contains(&"build ios".to_string()) && !mac.contains(&"build linux".to_string()) && !mac.contains(&"build windows".to_string()), "{mac:?}");
        let win = names(&tasks(&project, &found(), "windows"));
        assert!(win.contains(&"build windows".to_string()) && !win.contains(&"build ios".to_string()), "{win:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_workspace_member_leaves_resolving_to_the_root_and_a_script_has_no_run() {
        let dir = temp("tasks-member");
        write(&dir, &[("pubspec.yaml", "name: lib_a\nresolution: workspace\nenvironment:\n  sdk: ^3.6.0\n"), ("lib/main.dart", "")]);
        let t = tasks(&Project::read(&dir.to_string_lossy()), &Tools::default(), "linux");
        assert_eq!(names(&t), ["run main", "analyze", "format", "pub outdated", "fix preview", "fix apply"]);
        assert_eq!(line(&t, "analyze"), "dart analyze", "named for Fenix to say it isn't found");
        assert!(tasks(&Project::read(&temp("nothing").to_string_lossy()), &found(), "linux").resolved().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_device_is_asked_for_by_its_id() {
        let t = Tools { dart: None, flutter: Some("/f/bin/flutter".into()) };
        let cmd = flutter_run(&t, Some("emulator-5554"));
        assert_eq!(cmd.args, ["run", "--pid-file", ".dart_tool/fenix-flutter.pid", "-d", "emulator-5554"]);
        assert_eq!(builds("android", "windows"), ["apk", "appbundle"]);
        assert!(builds("ios", "linux").is_empty());
    }
}
