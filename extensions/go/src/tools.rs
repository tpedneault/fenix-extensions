//! The programs the pack runs, found on this machine: Go and gofmt, and
//! what `go install` puts in `~/go/bin` (gopls, Delve, govulncheck,
//! goimports, golangci-lint), which an editor started from the desktop
//! may not have on its `PATH`, so it's looked in too. Homebrew and winget
//! install Go itself.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: `go install`'s folder (`$GOPATH/bin`, by default
/// `~/go/bin`), Go's installers' and Homebrew's.
pub const HINTS: &[&str] = &[
    "$HOME/go/bin",
    "%USERPROFILE%\\go\\bin",
    "/usr/local/go/bin",
    "/usr/lib/go/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "C:\\Program Files\\Go\\bin",
];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// What installs each tool: `go install <package>@latest`.
pub const GOPLS: &str = "golang.org/x/tools/gopls";
pub const DELVE: &str = "github.com/go-delve/delve/cmd/dlv";
pub const GOVULNCHECK: &str = "golang.org/x/vuln/cmd/govulncheck";
pub const GOLANGCI_LINT: &str = "github.com/golangci/golangci-lint/v2/cmd/golangci-lint";

/// The formatter to prefer (`go.formatter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Formatter {
    #[default]
    Goimports,
    Gofmt,
}

impl Formatter {
    pub fn parse(text: &str) -> Formatter {
        if text.trim().eq_ignore_ascii_case("gofmt") {
            Formatter::Gofmt
        } else {
            Formatter::Goimports
        }
    }
}

/// What lints a saved file (`go.linter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LintWith {
    #[default]
    GolangciLint,
    Vet,
    Off,
}

impl LintWith {
    pub fn parse(text: &str) -> LintWith {
        match text.trim().to_ascii_lowercase().as_str() {
            "go vet" | "vet" => LintWith::Vet,
            "off" | "none" | "false" => LintWith::Off,
            _ => LintWith::GolangciLint,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub go: Option<String>,
    pub gofmt: Option<String>,
    pub goimports: Option<String>,
    pub gopls: Option<String>,
    pub dlv: Option<String>,
    pub govulncheck: Option<String>,
    pub golangci_lint: Option<String>,
    /// golangci-lint before 2.0, whose flags differ.
    pub golangci_v1: bool,
    pub brew: Option<String>,
    pub winget: Option<String>,
    /// `platform()`'s `os`: `linux`, `macos`, `windows`.
    pub os: String,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut find = |name: &str| host.find_program(name, &hints);
        Tools {
            go: find("go"),
            gofmt: find("gofmt"),
            goimports: find("goimports"),
            gopls: find("gopls"),
            dlv: find("dlv"),
            govulncheck: find("govulncheck"),
            golangci_lint: find("golangci-lint"),
            golangci_v1: false,
            brew: find("brew"),
            winget: find("winget"),
            os: String::new(),
        }
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn go(&self) -> Command {
        Tools::run(self.go.as_deref(), "go")
    }

    /// `go install <package>@latest`, when there's Go to do it.
    pub fn install(&self, package: &str) -> Option<Command> {
        self.go.as_ref().map(|_| self.go().args(["install".to_string(), format!("{package}@latest")]))
    }

    /// Installing Go: Homebrew on macOS, winget on Windows. Linux's comes
    /// from its packages or go.dev, which need more than a task.
    pub fn install_go(&self) -> Option<Command> {
        match self.os.as_str() {
            "macos" => self.brew.as_ref().map(|b| Command::new(b).args(["install", "go"])),
            "windows" => self.winget.as_ref().map(|w| Command::new(w).args(["install", "--id", "GoLang.Go", "-e", "--accept-source-agreements", "--accept-package-agreements"])),
            _ => None,
        }
    }

    /// The formatter Fenix runs on the buffer: goimports, when it's
    /// wanted and found (`-srcdir` the file, so the package's own imports
    /// are known), else gofmt.
    pub fn formatter(&self, wanted: Formatter) -> Option<Command> {
        match (&self.goimports, &self.gofmt) {
            (Some(goimports), _) if wanted == Formatter::Goimports => Some(Command::new(goimports).args(["-srcdir", "${file}"])),
            (_, Some(gofmt)) => Some(Command::new(gofmt)),
            (Some(goimports), None) => Some(Command::new(goimports).args(["-srcdir", "${file}"])),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn tools_are_installed_with_go_install() {
        let t = Tools { go: Some("/usr/local/go/bin/go".into()), ..Tools::default() };
        assert_eq!(t.install(GOPLS).map(|c| line(&c)).as_deref(), Some("/usr/local/go/bin/go install golang.org/x/tools/gopls@latest"));
        assert_eq!(Tools::default().install(DELVE), None, "no Go to install with");
    }

    #[test]
    fn go_comes_from_homebrew_or_winget() {
        let mut t = Tools { brew: Some("/opt/homebrew/bin/brew".into()), winget: Some("winget".into()), os: "macos".into(), ..Tools::default() };
        assert_eq!(t.install_go().map(|c| line(&c)).as_deref(), Some("/opt/homebrew/bin/brew install go"));
        t.os = "windows".into();
        assert!(t.install_go().map(|c| line(&c)).is_some_and(|l| l.starts_with("winget install --id GoLang.Go -e")));
        t.os = "linux".into();
        assert_eq!(t.install_go(), None);
    }

    #[test]
    fn goimports_formats_when_found_and_wanted() {
        let mut t = Tools { gofmt: Some("/g/gofmt".into()), goimports: Some("/h/goimports".into()), ..Tools::default() };
        assert_eq!(t.formatter(Formatter::Goimports).map(|c| line(&c)).as_deref(), Some("/h/goimports -srcdir ${file}"));
        assert_eq!(t.formatter(Formatter::Gofmt).map(|c| line(&c)).as_deref(), Some("/g/gofmt"));
        t.gofmt = None;
        assert_eq!(t.formatter(Formatter::Gofmt).map(|c| line(&c)).as_deref(), Some("/h/goimports -srcdir ${file}"), "the one there is");
        assert_eq!(Tools::default().formatter(Formatter::Goimports), None);
        assert_eq!(Formatter::parse(" GoFmt"), Formatter::Gofmt);
        assert_eq!((LintWith::parse("go vet"), LintWith::parse("off"), LintWith::parse("")), (LintWith::Vet, LintWith::Off, LintWith::GolangciLint));
    }
}
