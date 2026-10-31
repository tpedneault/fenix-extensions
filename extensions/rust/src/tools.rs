//! The programs the pack runs, found on this machine: cargo, rustup,
//! rustc, rustfmt, rust-analyzer, lldb-dap or CodeLLDB, cargo-audit,
//! cargo-outdated and cargo-binstall. rustup's and cargo's go into `~/.cargo/bin`, which
//! an editor started from the desktop may not have on its `PATH`, and
//! LLVM's lldb-dap is under a name of its version in Linux's packages and
//! in no folder on `PATH` in Homebrew's or Xcode's, so those are looked
//! in too.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: cargo's bin folder, then LLVM's.
pub const HINTS: &[&str] = &[
    "$HOME/.cargo/bin",
    "%USERPROFILE%\\.cargo\\bin",
    "/opt/homebrew/opt/llvm/bin",
    "/usr/local/opt/llvm/bin",
    "/Applications/Xcode.app/Contents/Developer/usr/bin",
    "/Library/Developer/CommandLineTools/usr/bin",
    "/usr/lib/llvm-22/bin",
    "/usr/lib/llvm-21/bin",
    "/usr/lib/llvm-20/bin",
    "/usr/lib/llvm-19/bin",
    "/usr/lib/llvm-18/bin",
    "C:\\Program Files\\LLVM\\bin",
];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// lldb-dap's names, newest first: LLVM 18 renamed lldb-vscode, and
/// Debian's and Ubuntu's packages add the version.
pub const LLDB_DAP: &[&str] = &["lldb-dap", "lldb-dap-22", "lldb-dap-21", "lldb-dap-20", "lldb-dap-19", "lldb-dap-18", "lldb-vscode", "lldb-vscode-17", "lldb-vscode-16", "lldb-vscode-15"];

/// CodeLLDB's adapter where you put it (`rust.codelldb`), looked at
/// first: the `adapter/codelldb` of its release, or VS Code's copy.
pub const CODELLDB: &str = "${setting:rust.codelldb}";

/// The debug adapter to prefer (`rust.debugger`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Debugger {
    #[default]
    LldbDap,
    CodeLldb,
}

impl Debugger {
    pub fn parse(text: &str) -> Debugger {
        if text.trim().eq_ignore_ascii_case("codelldb") {
            Debugger::CodeLldb
        } else {
            Debugger::LldbDap
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub cargo: Option<String>,
    pub rustup: Option<String>,
    pub rustfmt: Option<String>,
    pub rust_analyzer: Option<String>,
    /// lldb-dap, by whichever name it has here.
    pub lldb_dap: Option<String>,
    /// CodeLLDB's `codelldb`, which speaks DAP over a socket.
    pub codelldb: Option<String>,
    pub cargo_audit: Option<String>,
    pub cargo_outdated: Option<String>,
    pub cargo_binstall: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let codelldb_hints: Vec<String> = std::iter::once(CODELLDB.to_string()).chain(hints.iter().cloned()).collect();
        let codelldb = host.find_program("codelldb", &codelldb_hints);
        let mut find = |name: &str| host.find_program(name, &hints);
        Tools {
            cargo: find("cargo"),
            rustup: find("rustup"),
            rustfmt: find("rustfmt"),
            rust_analyzer: find("rust-analyzer"),
            lldb_dap: LLDB_DAP.iter().find_map(|n| find(n)),
            codelldb,
            cargo_audit: find("cargo-audit"),
            cargo_outdated: find("cargo-outdated"),
            cargo_binstall: find("cargo-binstall"),
        }
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn cargo(&self) -> Command {
        Tools::run(self.cargo.as_deref(), "cargo")
    }

    pub fn rustup(&self) -> Command {
        Tools::run(self.rustup.as_deref(), "rustup")
    }

    /// The name lldb-dap was found by: its program's file name.
    pub fn lldb_dap_name(&self) -> &str {
        let path = self.lldb_dap.as_deref().unwrap_or(LLDB_DAP[0]);
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        file.strip_suffix(".exe").unwrap_or(file)
    }

    /// The adapter to use: the one asked for when it's found, else the
    /// other; none when neither is.
    pub fn debugger(&self, wanted: Debugger) -> Option<(Debugger, &str)> {
        let path = |d: Debugger| match d {
            Debugger::LldbDap => self.lldb_dap.as_deref(),
            Debugger::CodeLldb => self.codelldb.as_deref(),
        };
        let other = if wanted == Debugger::LldbDap { Debugger::CodeLldb } else { Debugger::LldbDap };
        [wanted, other].into_iter().find_map(|d| path(d).map(|p| (d, p)))
    }

    /// A rustup component added to the toolchain the project uses.
    pub fn add_component(&self, component: &str) -> Option<Command> {
        self.rustup.as_ref().map(|_| self.rustup().args(["component", "add", component]))
    }

    /// A cargo subcommand installed: cargo-binstall's prebuilt binary when
    /// there's cargo-binstall, else built by `cargo install --locked`.
    pub fn install_crate(&self, name: &str) -> Option<Command> {
        if let Some(binstall) = &self.cargo_binstall {
            return Some(Command::new(binstall).args(["--no-confirm", name]));
        }
        self.cargo.as_ref().map(|_| self.cargo().args(["install", "--locked", name]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::host::mock::Mock;

    #[test]
    fn lldb_dap_is_found_by_any_of_its_names() {
        let mut host = Mock::default();
        host.programs.insert("lldb-dap-18".into(), "/usr/bin/lldb-dap-18".into());
        host.programs.insert("lldb-vscode".into(), "/usr/bin/lldb-vscode".into());
        let t = Tools::find(&mut host);
        assert_eq!((t.lldb_dap.as_deref(), t.lldb_dap_name()), (Some("/usr/bin/lldb-dap-18"), "lldb-dap-18"), "the newest name first");
        let windows = Tools { lldb_dap: Some("C:\\Program Files\\LLVM\\bin\\lldb-dap.exe".into()), ..Tools::default() };
        assert_eq!(windows.lldb_dap_name(), "lldb-dap");
        assert!(hints().contains(&"$HOME/.cargo/bin".to_string()));
    }

    #[test]
    fn the_debugger_asked_for_is_used_or_else_the_other() {
        let mut host = Mock::default();
        host.programs.insert("codelldb".into(), "/opt/codelldb/adapter/codelldb".into());
        let t = Tools::find(&mut host);
        assert_eq!(t.debugger(Debugger::LldbDap), Some((Debugger::CodeLldb, "/opt/codelldb/adapter/codelldb")), "no lldb-dap here");
        let both = Tools { lldb_dap: Some("/usr/bin/lldb-dap".into()), ..t };
        assert_eq!(both.debugger(Debugger::LldbDap).map(|d| d.0), Some(Debugger::LldbDap));
        assert_eq!(both.debugger(Debugger::parse("CodeLLDB")).map(|d| d.0), Some(Debugger::CodeLldb));
        assert_eq!(Tools::default().debugger(Debugger::CodeLldb), None);
    }

    #[test]
    fn components_come_from_rustup_and_tools_from_cargo() {
        let mut t = Tools { cargo: Some("/c/cargo".into()), rustup: Some("/c/rustup".into()), ..Tools::default() };
        assert_eq!(t.add_component("clippy").map(|c| line(&c)).as_deref(), Some("/c/rustup component add clippy"));
        assert_eq!(t.install_crate("cargo-audit").map(|c| line(&c)).as_deref(), Some("/c/cargo install --locked cargo-audit"));
        t.cargo_binstall = Some("/c/cargo-binstall".into());
        assert_eq!(t.install_crate("cargo-audit").map(|c| line(&c)).as_deref(), Some("/c/cargo-binstall --no-confirm cargo-audit"), "prebuilt");
        assert_eq!(Tools::default().add_component("rustfmt"), None, "no rustup, nothing to add it with");
        assert_eq!(Tools::default().install_crate("cargo-audit"), None);
    }
}
