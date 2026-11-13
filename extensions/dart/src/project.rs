//! What the pack reads of the project from its files: `pubspec.yaml`
//! (the package's name, the SDKs it asks for, what it depends on), which
//! platforms a Flutter app has folders for, and what `bin/` and `test/`
//! hold. No YAML library: a pubspec's top level is a few keys, and the
//! pack reads only those, a line at a time.

use std::path::Path;

/// What `pubspec.yaml` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pubspec {
    pub name: String,
    /// `environment: sdk:`, as written (`^3.5.0`).
    pub sdk: Option<String>,
    /// `environment: flutter:`, as written.
    pub flutter: Option<String>,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
    /// A top-level `flutter:` section: assets, fonts, a plugin's platforms.
    pub flutter_section: bool,
    /// `executables:` -- what `dart pub global activate` puts on the PATH.
    pub executables: Vec<String>,
    /// The root of a pub workspace (`workspace:`).
    pub workspace: bool,
    /// A member of one (`resolution: workspace`): its packages are
    /// resolved at the root.
    pub member: bool,
}

/// A value without its quotes and its trailing comment.
fn value(text: &str) -> String {
    let text = text.trim();
    let text = match text.chars().next() {
        Some(q @ ('"' | '\'')) => match text[1..].find(q) {
            Some(end) => return text[1..1 + end].to_string(),
            None => &text[1..],
        },
        _ => text.split(" #").next().unwrap_or(text),
    };
    text.trim().to_string()
}

/// `key: value`'s two halves; a line that isn't one is just a key.
fn key_value(line: &str) -> (String, String) {
    match line.split_once(':') {
        Some((key, rest)) => (value(key), value(rest)),
        None => (value(line), String::new()),
    }
}

impl Pubspec {
    pub fn parse(text: &str) -> Pubspec {
        let mut p = Pubspec::default();
        let mut section = String::new();
        // How far the section's own keys are indented: its first line's.
        let mut child: Option<usize> = None;
        for raw in text.lines() {
            let line = raw.trim_end();
            let t = line.trim_start();
            if t.is_empty() || t.starts_with('#') || t == "---" {
                continue;
            }
            let indent = line.len() - t.len();
            if indent == 0 {
                let (key, v) = key_value(t);
                child = None;
                match key.as_str() {
                    "name" => p.name = v,
                    "resolution" => p.member = v == "workspace",
                    "workspace" => p.workspace = true,
                    "flutter" => p.flutter_section = true,
                    _ => {}
                }
                section = key;
                continue;
            }
            if t.starts_with('-') || indent != *child.get_or_insert(indent) {
                continue;
            }
            let (key, v) = key_value(t);
            match section.as_str() {
                "environment" => match key.as_str() {
                    "sdk" => p.sdk = Some(v).filter(|v| !v.is_empty()),
                    "flutter" => p.flutter = Some(v).filter(|v| !v.is_empty()),
                    _ => {}
                },
                "dependencies" => p.dependencies.push(key),
                "dev_dependencies" => p.dev_dependencies.push(key),
                "executables" => p.executables.push(key),
                _ => {}
            }
        }
        p
    }

    /// A Flutter app or package: it depends on Flutter itself, or on what
    /// only Flutter has.
    pub fn is_flutter(&self) -> bool {
        let has = |name: &str| self.dependencies.iter().chain(&self.dev_dependencies).any(|d| d == name);
        has("flutter") || has("flutter_test") || has("flutter_web_plugins") || has("flutter_driver") || has("integration_test")
    }

    /// The test runner its dev dependencies name.
    pub fn tests(&self) -> Option<Tests> {
        let has = |name: &str| self.dev_dependencies.iter().chain(&self.dependencies).any(|d| d == name);
        if has("flutter_test") {
            Some(Tests::Flutter)
        } else if has("test") {
            Some(Tests::Dart)
        } else {
            None
        }
    }
}

/// Which command runs a package's tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tests {
    /// `dart test`, package:test.
    Dart,
    /// `flutter test`, flutter_test.
    Flutter,
}

/// The platforms a Flutter app has a folder for.
pub const PLATFORMS: &[&str] = &["android", "ios", "web", "linux", "macos", "windows"];

/// The project in `root`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub root: String,
    pub pubspec: Option<Pubspec>,
    /// Of `PLATFORMS`, those with a folder here.
    pub platforms: Vec<String>,
    /// The programs in `bin/`: `orbit` for `bin/orbit.dart`.
    pub bins: Vec<String>,
    pub has_main: bool,
    pub has_tests: bool,
    /// `pub get` has run: `.dart_tool/package_config.json` is there. (A
    /// workspace member's is at the workspace's root, so a member isn't
    /// judged by it.)
    pub resolved: bool,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let dir = Path::new(root);
        let pubspec = std::fs::read_to_string(dir.join("pubspec.yaml")).ok().map(|t| Pubspec::parse(&t));
        let mut bins: Vec<String> = std::fs::read_dir(dir.join("bin"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".dart").map(str::to_string))
            .collect();
        bins.sort();
        Project {
            root: root.into(),
            pubspec,
            platforms: PLATFORMS.iter().filter(|p| dir.join(p).is_dir()).map(|p| p.to_string()).collect(),
            bins,
            has_main: dir.join("lib/main.dart").is_file(),
            has_tests: dir.join("test").is_dir(),
            resolved: dir.join(".dart_tool/package_config.json").is_file(),
        }
    }

    /// A folder with a pubspec: a Dart package.
    pub fn is_dart(&self) -> bool {
        self.pubspec.is_some()
    }

    pub fn is_flutter(&self) -> bool {
        self.pubspec.as_ref().is_some_and(Pubspec::is_flutter)
    }

    /// The package's name; the folder's when there's no pubspec to say.
    pub fn name(&self) -> String {
        match &self.pubspec {
            Some(p) if !p.name.is_empty() => p.name.clone(),
            _ => Path::new(&self.root).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    /// Which command runs its tests: what the pubspec names; else, with a
    /// `test/` folder, by the kind of package.
    pub fn tests(&self) -> Option<Tests> {
        self.pubspec.as_ref().and_then(Pubspec::tests).or_else(|| {
            self.has_tests.then(|| if self.is_flutter() { Tests::Flutter } else { Tests::Dart })
        })
    }

    /// The package's own program, `bin/<name>.dart`, which `dart run`
    /// starts with no argument.
    pub fn has_default_bin(&self) -> bool {
        let name = self.name();
        self.bins.contains(&name)
    }

    /// The version `environment: sdk:` asks of Dart, as a requirement
    /// lang-kit reads (`^3.5.0`, `>=3.0.0 <4.0.0`).
    pub fn sdk(&self) -> Option<&str> {
        self.pubspec.as_ref().and_then(|p| p.sdk.as_deref())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const CONSOLE: &str = include_str!("samples/pubspec-console.yaml");
    pub const APP: &str = include_str!("samples/pubspec-flutter.yaml");

    /// A folder of its own under the system's temporary one.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-dart-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, files: &[(&str, &str)]) {
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    #[test]
    fn a_console_pubspec_is_read() {
        let p = Pubspec::parse(CONSOLE);
        assert_eq!((p.name.as_str(), p.sdk.as_deref()), ("orbit", Some("^3.14.0")));
        assert_eq!(p.dependencies, ["http", "path"]);
        assert_eq!(p.dev_dependencies, ["lints", "mockito", "test"]);
        assert!(!p.is_flutter() && p.executables == ["orbit"]);
        assert_eq!(p.tests(), Some(Tests::Dart));
    }

    #[test]
    fn a_flutter_pubspec_is_read() {
        let p = Pubspec::parse(APP);
        assert_eq!((p.name.as_str(), p.sdk.as_deref(), p.flutter.as_deref()), ("orbit_app", Some(">=3.5.0 <4.0.0"), Some(">=3.24.0")));
        assert_eq!(p.dependencies, ["flutter", "cupertino_icons", "go_router"], "a dependency's own lines aren't dependencies");
        assert_eq!(p.dev_dependencies, ["flutter_test", "flutter_lints"]);
        assert!(p.is_flutter() && p.flutter_section);
        assert_eq!(p.tests(), Some(Tests::Flutter));
    }

    #[test]
    fn quotes_comments_and_workspaces_are_read() {
        let p = Pubspec::parse("name: \"a_b\"  # the name\nresolution: workspace\nenvironment:\n    sdk: '^3.6.0'\ndependencies:\n    # nothing yet\n    path: ^1.9.0\n  \nworkspace:\n  - packages/a\n  - packages/b\n");
        assert_eq!(p.name, "a_b");
        assert_eq!(p.sdk.as_deref(), Some("^3.6.0"), "any indentation, quotes off");
        assert!(p.member && p.workspace);
        assert_eq!(p.dependencies, ["path"]);
        assert_eq!(Pubspec::parse("").name, "");
        assert!(!Pubspec::parse("name: x\nresolution: none\n").member);
    }

    #[test]
    fn the_folders_say_what_there_is_to_run() {
        let dir = temp("project");
        write(&dir, &[("pubspec.yaml", CONSOLE), ("bin/orbit.dart", "void main() {}\n"), ("bin/seed.dart", ""), ("lib/main.dart", ""), ("test/a_test.dart", ""), ("web/index.html", ""), ("android/x", ""), (".dart_tool/package_config.json", "{}")]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.is_dart() && !p.is_flutter());
        assert_eq!(p.bins, ["orbit", "seed"]);
        assert!(p.has_default_bin() && p.has_main && p.has_tests && p.resolved);
        assert_eq!(p.platforms, ["android", "web"]);
        assert_eq!(p.name(), "orbit");
        // Without a pubspec: a folder.
        let bare = temp("bare");
        let p = Project::read(&bare.to_string_lossy());
        assert!(!p.is_dart() && p.tests().is_none() && !p.resolved);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&bare).unwrap();
    }

    #[test]
    fn tests_follow_the_dev_dependencies_then_the_folder() {
        let dir = temp("tests");
        write(&dir, &[("pubspec.yaml", "name: a\ndependencies:\n  flutter:\n    sdk: flutter\n"), ("test/x_test.dart", "")]);
        assert_eq!(Project::read(&dir.to_string_lossy()).tests(), Some(Tests::Flutter), "a Flutter package with a test folder");
        write(&dir, &[("pubspec.yaml", "name: a\n")]);
        std::fs::write(dir.join("pubspec.yaml"), "name: a\n").unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()).tests(), Some(Tests::Dart));
        std::fs::remove_dir_all(dir.join("test")).unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()).tests(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
