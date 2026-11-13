//! The project doctor's checks (`SPC p h`): the Dart SDK -- which one is
//! in use, its version, held to what `pubspec.yaml` asks -- Flutter in a
//! Flutter project, and whether `pub get` has resolved the packages.
//! Neither SDK is installed by the pack: Dart comes with Flutter, and
//! both are big installs with their own installers, so the fix rows open
//! the page that says how for this machine.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{find_version, Fix, Output, Program, Requirement, Version};

use crate::project::Project;
use crate::tools::{dart_hints, flutter_hints, folder_of, Tools};

/// The fix rows' commands, in the manifest.
pub const GET_DART: &str = "dart.get-dart";
pub const GET_FLUTTER: &str = "dart.get-flutter";
pub const PUB_GET: &str = "dart.pub-get";

/// Where each SDK says how to install it.
pub const DART_PAGE: &str = "https://dart.dev/get-dart";
pub const FLUTTER_PAGE: &str = "https://docs.flutter.dev/get-started/install";

/// What to tell someone without Dart, for `os`: the shortest way there,
/// and the page.
pub fn how_to_get_dart(os: &str) -> String {
    let how = match os {
        "macos" => "brew tap dart-lang/dart && brew install dart",
        "windows" => "choco install dart-sdk",
        _ => "apt: see the page, or install Flutter, which has Dart in it",
    };
    format!("Dart ({how}): {DART_PAGE}")
}

pub fn how_to_get_flutter() -> String {
    format!("Flutter (it brings its own Dart): {FLUTTER_PAGE}")
}

/// What `dart --version` said: `Dart SDK version: 3.14.0 (stable) (Wed Nov
/// 4 ...) on "linux_x64"` -- its version and its channel.
pub fn dart_version(text: &str) -> Option<(Version, Option<String>)> {
    let (_, after) = text.split_once("version:")?;
    let version = find_version(after)?;
    let channel = after.split_once('(').and_then(|(_, rest)| rest.split_once(')')).map(|(c, _)| c.trim().to_string()).filter(|c| !c.is_empty() && !c.contains(' '));
    Some((version, channel))
}

/// What `dart --version` said, held to `wants` (`environment: sdk:`), and
/// which Dart it is: `flutters` when it's in Flutter's own folder.
/// `elsewhere` is Flutter's folder when this Dart isn't in it and the
/// project is a Flutter one: its own Dart is what its packages are held
/// to.
pub fn judge_dart(wants: Option<Requirement>, path: Option<String>, flutters: bool, elsewhere: Option<String>) -> impl Fn(&Output) -> (Health, String) {
    move |out| {
        let Some((v, channel)) = dart_version(&out.text()).or_else(|| find_version(&out.text()).map(|v| (v, None))) else {
            return if out.ok() { (Health::Warn, "its version couldn't be read".into()) } else { (Health::Warn, format!("asking its version failed: {}", out.why())) };
        };
        let shown = match channel {
            Some(c) => format!("{v} {c}"),
            None => v.to_string(),
        };
        let which = match (&path, flutters) {
            (Some(p), true) => format!("Flutter's, {p}"),
            (Some(p), false) => format!("standalone, {p}"),
            (None, _) => "found".into(),
        };
        match (&wants, &elsewhere) {
            (Some(r), _) if !r.matches(&v) => (Health::Bad, format!("{shown}, but pubspec.yaml needs {r} ({which})")),
            (_, Some(folder)) => (Health::Warn, format!("{shown} ({which}), not Flutter's, in {folder}: the server and debugger use this one; put Flutter's bin first on PATH, or set dart.sdk")),
            _ => (Health::Ok, format!("{shown} ({which}): the analysis server and debugger come from it")),
        }
    }
}

/// What `flutter --version --machine` said: its JSON (`frameworkVersion`,
/// `channel`, `dartSdkVersion`), or, where it printed words, the version
/// in them.
pub fn judge_flutter(wants: Option<Requirement>) -> impl Fn(&Output) -> (Health, String) {
    move |out| {
        let text = out.stdout.find('{').and_then(|at| fenix_lang_kit::serde_json::from_str::<Value>(out.stdout[at..].trim()).ok());
        let say = |key: &str| text.as_ref().and_then(|j| j.get(key)).and_then(Value::as_str).map(str::to_string);
        let version = say("frameworkVersion").or_else(|| say("flutterVersion")).and_then(|v| find_version(&v)).or_else(|| find_version(&out.text()));
        let Some(v) = version else {
            return if out.ok() { (Health::Warn, "its version couldn't be read".into()) } else { (Health::Warn, format!("asking its version failed: {}", out.why())) };
        };
        let mut shown = v.to_string();
        if let Some(channel) = say("channel") {
            shown = format!("{shown} {channel}");
        }
        if let Some(dart) = say("dartSdkVersion") {
            shown = format!("{shown}, with Dart {dart}");
        }
        match (&wants, say("flutterRoot")) {
            (Some(r), _) if !r.matches(&v) => (Health::Bad, format!("{shown}, but pubspec.yaml needs {r}")),
            (_, Some(root)) => (Health::Ok, format!("{shown} ({root})")),
            _ => (Health::Ok, shown),
        }
    }
}

/// The program checks for `project`, with what's on this machine.
pub fn programs(project: &Project, tools: &Tools) -> Vec<Program> {
    let mut out = Vec::new();
    let flutter_project = project.is_flutter();
    let wants = project.sdk().and_then(Requirement::parse);
    let elsewhere = (flutter_project && tools.dart.is_some() && !tools.dart_is_flutters()).then(|| tools.flutter.as_deref().and_then(folder_of)).flatten();
    let dart = dart_hints(tools.flutter.as_deref()).into_iter().fold(Program::new("dart", "Dart SDK"), Program::hint).judge(judge_dart(wants, tools.dart.clone(), tools.dart_is_flutters(), elsewhere)).fix("Get Dart (dart.dev)", GET_DART, Fix::Own);
    out.push(if project.is_dart() { dart } else { dart.optional() });
    if flutter_project {
        let wants = project.pubspec.as_ref().and_then(|p| p.flutter.as_deref()).and_then(Requirement::parse);
        // Flutter's first run may download its tools: give it time.
        let flutter = flutter_hints().into_iter().fold(Program::new("flutter", "Flutter"), Program::hint).version_args(["--version", "--machine"]).timeout(60_000).judge(judge_flutter(wants)).fix("Get Flutter (flutter.dev)", GET_FLUTTER, Fix::Own);
        out.push(flutter);
    }
    out
}

/// The pack's own checks: whether `pub get` has run. Without it the
/// analysis server can't find a package, and says so on every import.
pub fn extra(project: &Project) -> Vec<Check> {
    match &project.pubspec {
        Some(p) if !p.member && !project.resolved => {
            vec![Check::new(Section::Project, "Packages", Health::Warn, "not resolved: .dart_tool/package_config.json isn't there, so imports of packages can't be found").fix("Run pub get", PUB_GET)]
        }
        Some(p) if p.member => Vec::new(),
        Some(_) => vec![Check::new(Section::Project, "Packages", Health::Ok, "resolved")],
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, APP, CONSOLE};

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn named<'a>(programs: &'a [Program], name: &str) -> &'a Program {
        programs.iter().find(|p| p.name == name).unwrap()
    }

    #[test]
    fn dart_says_its_version_and_channel() {
        let (v, channel) = dart_version(include_str!("samples/dart-version.txt")).unwrap();
        assert_eq!((v.to_string(), channel.as_deref()), ("3.14.0".to_string(), Some("stable")));
        let (v, channel) = dart_version("Dart SDK version: 3.15.0-259.0.dev (dev) (Tue Nov 3 12:00:00 2026 +0000) on \"macos_arm64\"").unwrap();
        assert_eq!((v.to_string(), channel.as_deref()), ("3.15.0-259.0.dev".to_string(), Some("dev")));
        assert_eq!(dart_version("Dart SDK version: 3.5.4 (stable) (Tue Oct 15 2024) on \"windows_x64\"").unwrap().0.to_string(), "3.5.4");
        assert!(dart_version("command not found").is_none());
    }

    #[test]
    fn it_is_held_to_what_the_pubspec_asks_and_says_which_dart_it_is() {
        let text = include_str!("samples/dart-version.txt");
        let judge = judge_dart(Requirement::parse("^3.5.0"), Some("/usr/lib/dart/bin/dart".into()), false, None);
        assert_eq!(judge(&said(text)), (Health::Ok, "3.14.0 stable (standalone, /usr/lib/dart/bin/dart): the analysis server and debugger come from it".into()));
        let judge = judge_dart(Requirement::parse("^3.5.0"), Some("/f/bin/dart".into()), true, None);
        assert!(judge(&said(text)).1.contains("(Flutter's, /f/bin/dart)"));
        let judge = judge_dart(Requirement::parse("^3.20.0"), Some("/usr/bin/dart".into()), false, None);
        assert_eq!(judge(&said(text)), (Health::Bad, "3.14.0 stable, but pubspec.yaml needs ^3.20.0 (standalone, /usr/bin/dart)".into()));
        // A Flutter project, with a Dart that isn't Flutter's own.
        let judge = judge_dart(None, Some("/usr/bin/dart".into()), false, Some("/f/bin".into()));
        let (health, detail) = judge(&said(text));
        assert_eq!(health, Health::Warn);
        assert!(detail.contains("not Flutter's, in /f/bin") && detail.contains("dart.sdk"), "{detail}");
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "boom".into() };
        assert_eq!(judge(&failed), (Health::Warn, "asking its version failed: boom".into()));
    }

    #[test]
    fn flutter_says_its_version_channel_and_dart() {
        let judge = judge_flutter(Requirement::parse(">=3.24.0"));
        assert_eq!(judge(&said(include_str!("samples/flutter-version.json"))), (Health::Ok, "3.27.1 stable, with Dart 3.6.0 (/home/dev/flutter)".into()));
        let judge = judge_flutter(Requirement::parse(">=3.30.0"));
        assert_eq!(judge(&said(include_str!("samples/flutter-version.json"))).0, Health::Bad);
        // Words from an older Flutter, with a notice before them.
        let judge = judge_flutter(None);
        assert_eq!(judge(&said("Flutter 3.10.6 • channel stable • https://github.com/flutter/flutter.git\nEngine • revision 18a827f393\nTools • Dart 3.0.6 • DevTools 2.23.1\n")), (Health::Ok, "3.10.6".into()));
        assert_eq!(judge(&said("")).1, "its version couldn't be read");
    }

    #[test]
    fn a_dart_package_checks_dart_and_a_flutter_app_checks_flutter_too() {
        let dir = temp("doctor");
        write(&dir, &[("pubspec.yaml", CONSOLE)]);
        let tools = Tools { dart: Some("/usr/bin/dart".into()), flutter: None };
        let p = programs(&Project::read(&dir.to_string_lossy()), &tools);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.required)).collect::<Vec<_>>(), [("dart", true)]);
        assert_eq!(p[0].fix.as_ref().map(|f| (f.1.clone(), f.2.clone())), Some((GET_DART.to_string(), Fix::Own)));
        assert_eq!(p[0].hints[..2], ["${setting:dart.sdk}", "./.fvm/flutter_sdk/bin"]);

        write(&dir, &[("pubspec.yaml", APP)]);
        let tools = Tools { dart: Some("/f/bin/dart".into()), flutter: Some("/f/bin/flutter".into()) };
        let p = programs(&Project::read(&dir.to_string_lossy()), &tools);
        assert_eq!(p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["dart", "flutter"]);
        assert_eq!(named(&p, "flutter").version_args, ["--version", "--machine"]);
        assert_eq!(named(&p, "flutter").millis(), 60_000);
        assert!(named(&p, "dart").hints.contains(&"/f/bin".to_string()), "its own Dart is beside it");
        assert_eq!(named(&p, "dart").wants, None, "the requirement is the pubspec's, in the judge");
        // Lone files: a warning, not a failure.
        let lone = temp("doctor-lone");
        let p = programs(&Project::read(&lone.to_string_lossy()), &tools);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.required)).collect::<Vec<_>>(), [("dart", false)]);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&lone).unwrap();
    }

    #[test]
    fn missing_packages_are_a_check_with_a_fix() {
        let dir = temp("doctor-packages");
        write(&dir, &[("pubspec.yaml", CONSOLE)]);
        let checks = extra(&Project::read(&dir.to_string_lossy()));
        assert_eq!((checks[0].label.as_str(), checks[0].health), ("Packages", Health::Warn));
        assert_eq!(checks[0].fix.as_ref().map(|f| f.1.as_str()), Some(PUB_GET));
        write(&dir, &[(".dart_tool/package_config.json", "{}")]);
        assert_eq!(extra(&Project::read(&dir.to_string_lossy()))[0].health, Health::Ok);
        write(&dir, &[("pubspec.yaml", "name: m\nresolution: workspace\n")]);
        assert!(extra(&Project::read(&dir.to_string_lossy())).is_empty(), "the workspace's root resolves it");
        assert!(extra(&Project::read(&temp("doctor-none").to_string_lossy())).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn how_to_get_each_says_it_for_the_machine() {
        assert!(how_to_get_dart("macos").contains("brew install dart") && how_to_get_dart("windows").contains("choco install dart-sdk"));
        assert!(how_to_get_dart("linux").ends_with(DART_PAGE));
        assert!(how_to_get_flutter().contains("its own Dart"));
    }
}
