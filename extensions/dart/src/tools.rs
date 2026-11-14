//! The programs the pack runs, found on this machine: `dart`, which has
//! the analysis server, the formatter and the debug adapter, and
//! `flutter`, for Flutter projects. Flutter ships its own Dart beside it
//! (`flutter/bin/dart`), so a machine with only Flutter has both.
//!
//! Fenix looks for a program in its settings first (`dart.sdk`,
//! `dart.flutter`), then the project's own pinned SDK (FVM's
//! `.fvm/flutter_sdk`), then `PATH`, then the hints here.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where the pack's settings say a program is.
pub const DART_SETTING: &str = "${setting:dart.sdk}";
pub const FLUTTER_SETTING: &str = "${setting:dart.flutter}";

/// A Flutter SDK pinned in the project by FVM: its own, before `PATH`.
pub const FVM: &str = "./.fvm/flutter_sdk/bin";

/// Where Flutter is kept when it isn't on `PATH`: where its install
/// guide unpacks it, the package managers' folders, a version manager's.
pub const FLUTTER_HINTS: &[&str] = &[
    "$HOME/flutter/bin",
    "$HOME/development/flutter/bin",
    "$HOME/dev/flutter/bin",
    "$HOME/sdk/flutter/bin",
    "$HOME/snap/flutter/common/flutter/bin",
    "$HOME/fvm/default/bin",
    "$HOME/.puro/envs/stable/flutter/bin",
    "/snap/bin",
    "/opt/flutter/bin",
    "/usr/local/flutter/bin",
    "/opt/homebrew/bin",
    "%USERPROFILE%\\flutter\\bin",
    "%USERPROFILE%\\development\\flutter\\bin",
    "%LOCALAPPDATA%\\flutter\\bin",
    "C:\\src\\flutter\\bin",
    "C:\\flutter\\bin",
];

/// Where a standalone Dart SDK is kept when it isn't on `PATH`.
pub const DART_HINTS: &[&str] = &[
    "/usr/lib/dart/bin",
    "/opt/homebrew/opt/dart/libexec/bin",
    "/usr/local/opt/dart/libexec/bin",
    "$HOME/dart-sdk/bin",
    "$HOME/development/dart-sdk/bin",
    "C:\\tools\\dart-sdk\\bin",
    "%LOCALAPPDATA%\\Programs\\dart-sdk\\bin",
];

/// Where to look for `flutter`.
pub fn flutter_hints() -> Vec<String> {
    [FLUTTER_SETTING, FVM].iter().chain(FLUTTER_HINTS).map(|h| h.to_string()).collect()
}

/// Where to look for `dart`: the setting, the project's pinned Flutter,
/// then beside the `flutter` found, then where SDKs are kept.
pub fn dart_hints(flutter: Option<&str>) -> Vec<String> {
    let beside = flutter.and_then(folder_of).into_iter();
    [DART_SETTING.to_string(), FVM.to_string()].into_iter().chain(beside).chain(DART_HINTS.iter().map(|h| h.to_string())).collect()
}

/// The folder a program is in: `/home/u/flutter/bin` for
/// `/home/u/flutter/bin/flutter`, `C:\src\flutter\bin` for its `.bat`.
pub fn folder_of(path: &str) -> Option<String> {
    let at = path.rfind(['/', '\\'])?;
    Some(path[..at].to_string())
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub dart: Option<String>,
    pub flutter: Option<String>,
}

impl Tools {
    /// Both, looked for as above. Flutter first: where it is says where
    /// its Dart might be.
    pub fn find(host: &mut dyn Host) -> Tools {
        let flutter = host.find_program("flutter", &flutter_hints());
        let dart = host.find_program("dart", &dart_hints(flutter.as_deref()));
        Tools { dart, flutter }
    }

    /// Whether the Dart found is the one in Flutter's own folder.
    pub fn dart_is_flutters(&self) -> bool {
        match (&self.dart, &self.flutter) {
            (Some(d), Some(f)) => folder_of(d) == folder_of(f),
            _ => false,
        }
    }

    /// `dart`, as found, or by its name for the run to say it isn't there.
    pub fn dart_cmd(&self) -> Command {
        Command::new(self.dart.as_deref().unwrap_or("dart"))
    }

    pub fn flutter_cmd(&self) -> Command {
        Command::new(self.flutter.as_deref().unwrap_or("flutter"))
    }

    /// What a project runs `pub`, builds and tests with: Flutter in a
    /// Flutter project (it resolves Flutter's packages), else Dart.
    pub fn pub_cmd(&self, flutter_project: bool) -> Command {
        if flutter_project {
            self.flutter_cmd()
        } else {
            self.dart_cmd()
        }
    }

    /// The analysis server: `dart language-server`. None without Dart.
    pub fn server(&self) -> Option<Command> {
        self.dart.as_ref().map(|d| Command::new(d).args(["language-server", "--client-id", "fenix"]))
    }

    /// The debug adapter, over stdio: Flutter's in a Flutter project it
    /// can run, else Dart's.
    pub fn adapter(&self, flutter_project: bool) -> Option<Command> {
        match (&self.flutter, &self.dart, flutter_project) {
            (Some(f), _, true) => Some(Command::new(f).arg("debug_adapter")),
            (_, Some(d), _) => Some(Command::new(d).arg("debug_adapter")),
            _ => None,
        }
    }

    /// `dart format`, reading the buffer on its stdin. `${file}` is
    /// where it came from: its package's language version and
    /// `analysis_options.yaml`'s `formatter:` rules are found from there.
    pub fn formatter(&self) -> Option<Command> {
        self.dart.as_ref().map(|d| Command::new(d).args(["format", "--stdin-name", "${file}"]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dart_is_looked_for_beside_flutter() {
        let hints = dart_hints(Some("/opt/sdk/flutter/bin/flutter"));
        assert_eq!(hints[..3], [DART_SETTING.to_string(), FVM.to_string(), "/opt/sdk/flutter/bin".to_string()], "the setting, the project's, Flutter's own folder");
        assert!(hints.contains(&"/usr/lib/dart/bin".to_string()));
        assert_eq!(dart_hints(None)[..2], [DART_SETTING.to_string(), FVM.to_string()]);
        assert_eq!(folder_of("C:\\src\\flutter\\bin\\flutter.bat").as_deref(), Some("C:\\src\\flutter\\bin"));
        assert_eq!(folder_of("flutter"), None);
        assert_eq!(flutter_hints()[0], FLUTTER_SETTING);
    }

    #[test]
    fn each_command_is_the_one_found() {
        let both = Tools { dart: Some("/f/bin/dart".into()), flutter: Some("/f/bin/flutter".into()) };
        assert!(both.dart_is_flutters());
        assert_eq!(both.server().unwrap().args, ["language-server", "--client-id", "fenix"]);
        assert_eq!(both.adapter(true).unwrap().program, "/f/bin/flutter", "Flutter's, in a Flutter project");
        assert_eq!(both.adapter(false).unwrap().args, ["debug_adapter"]);
        assert_eq!(both.formatter().unwrap().args, ["format", "--stdin-name", "${file}"]);
        assert_eq!(both.pub_cmd(true).program, "/f/bin/flutter");
        assert_eq!(both.pub_cmd(false).program, "/f/bin/dart");

        let apart = Tools { dart: Some("/usr/bin/dart".into()), flutter: Some("/f/bin/flutter".into()) };
        assert!(!apart.dart_is_flutters());
        let dart_only = Tools { dart: Some("/usr/bin/dart".into()), flutter: None };
        assert_eq!(dart_only.adapter(true).unwrap().program, "/usr/bin/dart", "no Flutter to debug with");
        let none = Tools::default();
        assert!(none.server().is_none() && none.adapter(false).is_none() && none.formatter().is_none() && !none.dart_is_flutters());
        assert_eq!((none.dart_cmd().program.as_str(), none.flutter_cmd().program.as_str()), ("dart", "flutter"), "by name, for Fenix to say it isn't there");
    }
}
