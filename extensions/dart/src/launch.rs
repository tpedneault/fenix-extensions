//! The launch configurations the pack works out as it runs, offered by
//! `SPC u u` beside its launch files (`launch/`): a program for each of a
//! package's `bin/` files, and, in a Flutter app, `flutter run` -- the
//! device Flutter picks, the file you're in -- and one on each device
//! Flutter lists. They're the same adapter as the files': the SDK's, over
//! stdio.

use fenix_extension_api::LaunchConfig;

use crate::devices::{self, Device};
use crate::project::Project;

/// The configurations for `project`, with the devices Flutter listed.
pub fn launches(project: &Project, devices: &[Device]) -> Vec<LaunchConfig> {
    let mut out = Vec::new();
    if !project.is_dart() {
        return out;
    }
    if project.is_flutter() {
        if project.has_main {
            out.push(LaunchConfig::launch("Flutter: run", &["dart"]).program("/project/lib/main.dart"));
        }
        out.push(LaunchConfig::launch("Flutter: run this file", &["dart"]).program("${file}"));
        out.extend(devices::launches(devices));
    } else {
        for bin in &project.bins {
            out.push(LaunchConfig::launch(&format!("Dart: run bin/{bin}.dart"), &["dart"]).program(&format!("/project/bin/{bin}.dart")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, APP, CONSOLE};

    fn names(l: &[LaunchConfig]) -> Vec<&str> {
        l.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn a_dart_package_runs_each_of_its_programs() {
        let dir = temp("launch-dart");
        write(&dir, &[("pubspec.yaml", CONSOLE), ("bin/orbit.dart", ""), ("bin/seed.dart", "")]);
        let l = launches(&Project::read(&dir.to_string_lossy()), &[]);
        assert_eq!(names(&l), ["Dart: run bin/orbit.dart", "Dart: run bin/seed.dart"]);
        assert_eq!(l[1].program.as_deref(), Some("/project/bin/seed.dart"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flutter_app_runs_and_runs_on_each_device() {
        let dir = temp("launch-flutter");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", "")]);
        let devices = devices::parse(&fenix_lang_kit::Output { code: Some(0), stdout: include_str!("samples/flutter-devices.json").into(), stderr: String::new() }).unwrap();
        let l = launches(&Project::read(&dir.to_string_lossy()), &devices);
        assert_eq!(names(&l), ["Flutter: run", "Flutter: run this file", "Flutter: run on sdk gphone64 x86 64", "Flutter: run on Pixel 8", "Flutter: run on Linux (desktop)", "Flutter: run on Chrome (web)"]);
        assert_eq!(l[0].program.as_deref(), Some("/project/lib/main.dart"));
        assert_eq!(l[1].program.as_deref(), Some("${file}"));
        // Without devices, and with no folder of Dart at all.
        assert_eq!(launches(&Project::read(&dir.to_string_lossy()), &[]).len(), 2);
        assert!(launches(&Project::read(&temp("launch-none").to_string_lossy()), &devices).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
