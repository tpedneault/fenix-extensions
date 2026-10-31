//! The programs the pack runs, found on this machine: CMake and CTest,
//! Ninja, a compiler, clangd, clang-format and clang-tidy, lldb-dap or
//! GDB, vcpkg and Conan; and uv, pipx or pip, which install the ones
//! PyPI has. LLVM's folders are looked in too: Debian's and Ubuntu's
//! `/usr/lib/llvm-N/bin` (where `lldb-dap` has no version on its name),
//! Homebrew's and Windows'.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look.
pub const HINTS: &[&str] = &[
    "$HOME/.local/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/opt/homebrew/opt/llvm/bin",
    "/usr/local/opt/llvm/bin",
    "/usr/lib/llvm-21/bin",
    "/usr/lib/llvm-20/bin",
    "/usr/lib/llvm-19/bin",
    "/usr/lib/llvm-18/bin",
    "/usr/lib/llvm-17/bin",
    "/usr/lib/llvm-16/bin",
    "/usr/lib/llvm-15/bin",
    "/usr/lib/llvm-14/bin",
    "C:\\Program Files\\LLVM\\bin",
    "C:\\Program Files\\CMake\\bin",
    "%USERPROFILE%\\.local\\bin",
    "%APPDATA%\\Python\\Scripts",
];

/// Where vcpkg's clone usually is, when it isn't on `PATH`.
pub const VCPKG_HINTS: &[&str] = &["$VCPKG_ROOT", "%VCPKG_ROOT%", "$HOME/vcpkg", "%USERPROFILE%\\vcpkg", "C:\\vcpkg", "C:\\src\\vcpkg", "/opt/vcpkg"];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

pub fn vcpkg_hints() -> Vec<String> {
    VCPKG_HINTS.iter().chain(HINTS).map(|h| h.to_string()).collect()
}

/// Compilers, in the order looked for: the system's, GCC's, Clang's,
/// MSVC's.
pub const COMPILERS: &[&str] = &["c++", "cc", "g++", "gcc", "clang++", "clang", "cl"];

/// The debug adapter to prefer (`cpp.debugger`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Debugger {
    #[default]
    LldbDap,
    Gdb,
}

impl Debugger {
    pub fn parse(text: &str) -> Debugger {
        if text.trim().eq_ignore_ascii_case("gdb") {
            Debugger::Gdb
        } else {
            Debugger::LldbDap
        }
    }

    pub fn other(self) -> Debugger {
        match self {
            Debugger::LldbDap => Debugger::Gdb,
            Debugger::Gdb => Debugger::LldbDap,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub cmake: Option<String>,
    pub ctest: Option<String>,
    pub ninja: Option<String>,
    /// The compiler's name, and where it is.
    pub compiler: Option<(String, String)>,
    pub clangd: Option<String>,
    pub clang_format: Option<String>,
    pub clang_tidy: Option<String>,
    /// `lldb-dap`, or `lldb-vscode` as it was called before LLVM 18.
    pub lldb_dap: Option<String>,
    pub gdb: Option<String>,
    pub vcpkg: Option<String>,
    pub conan: Option<String>,
    pub uv: Option<String>,
    pub pipx: Option<String>,
    pub python: Option<String>,
    /// Homebrew, winget and Apple's `xcode-select`, which install the
    /// compiler and LLVM's tools where PyPI has none.
    pub brew: Option<String>,
    pub winget: Option<String>,
    pub xcode_select: Option<String>,
    /// `platform()`'s `os`: `linux`, `macos`, `windows`.
    pub os: String,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut find = |name: &str| host.find_program(name, &hints);
        let compiler = COMPILERS.iter().find_map(|c| find(c).map(|p| (c.to_string(), p)));
        let mut t = Tools {
            cmake: find("cmake"),
            ctest: find("ctest"),
            ninja: find("ninja"),
            compiler,
            clangd: find("clangd"),
            clang_format: find("clang-format"),
            clang_tidy: find("clang-tidy"),
            lldb_dap: find("lldb-dap").or_else(|| find("lldb-vscode")),
            gdb: find("gdb"),
            vcpkg: None,
            conan: find("conan"),
            uv: find("uv"),
            pipx: find("pipx"),
            python: find("python3").or_else(|| find("python")),
            brew: find("brew"),
            winget: find("winget"),
            xcode_select: find("xcode-select"),
            os: String::new(),
        };
        t.vcpkg = host.find_program("vcpkg", &vcpkg_hints());
        t
    }

    /// A program by its path when found, else by its name.
    pub fn run(path: Option<&str>, name: &str) -> Command {
        Command::new(path.unwrap_or(name))
    }

    pub fn cmake(&self) -> Command {
        Tools::run(self.cmake.as_deref(), "cmake")
    }

    pub fn ctest(&self) -> Command {
        Tools::run(self.ctest.as_deref(), "ctest")
    }

    pub fn vcpkg(&self) -> Command {
        Tools::run(self.vcpkg.as_deref(), "vcpkg")
    }

    pub fn conan(&self) -> Command {
        Tools::run(self.conan.as_deref(), "conan")
    }

    /// vcpkg's folder (`VCPKG_ROOT`): where its program is.
    pub fn vcpkg_root(&self) -> Option<String> {
        let path = self.vcpkg.as_deref()?.replace('\\', "/");
        let (dir, _) = path.rsplit_once('/')?;
        Some(dir.to_string())
    }

    /// The debug adapter: the one asked for, else the other. GDB speaks
    /// DAP itself from 14 on (`--interpreter=dap`); the doctor says when
    /// it's older.
    pub fn adapter(&self, wanted: Debugger) -> Option<(Debugger, Command)> {
        [wanted, wanted.other()].into_iter().find_map(|d| match d {
            Debugger::LldbDap => self.lldb_dap.as_ref().map(|p| (d, Command::new(p))),
            Debugger::Gdb => self.gdb.as_ref().map(|p| (d, Command::new(p).arg("--interpreter=dap"))),
        })
    }

    /// Installing a tool PyPI has (`cmake`, `ninja`, `clangd`,
    /// `clang-format`, `clang-tidy`, `conan`) for you: `uv tool install`,
    /// `pipx install`, or pip's `--user`.
    pub fn install_tool(&self, package: &str) -> Option<Command> {
        if let Some(uv) = &self.uv {
            Some(Command::new(uv).args(["tool", "install", package]))
        } else if let Some(pipx) = &self.pipx {
            Some(Command::new(pipx).args(["install", package]))
        } else {
            self.python.as_ref().map(|p| Command::new(p).args(["-m", "pip", "install", "--user", package]))
        }
    }
}

impl Tools {
    /// A compiler: Apple's command line tools on macOS, Visual Studio's
    /// build tools on Windows (winget). Linux's come with its packages,
    /// which need root.
    pub fn install_compiler(&self) -> Option<Command> {
        match self.os.as_str() {
            "macos" => self.xcode_select.as_ref().map(|x| Command::new(x).arg("--install")),
            "windows" => self.winget.as_ref().map(|w| {
                Command::new(w).args(["install", "-e", "--id", "Microsoft.VisualStudio.2022.BuildTools", "--override", "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"])
            }),
            _ => None,
        }
    }

    /// LLVM, for lldb-dap (and clangd, clang-format, clang-tidy with it):
    /// Homebrew's on macOS, LLVM's own on Windows.
    pub fn install_llvm(&self) -> Option<Command> {
        match self.os.as_str() {
            "macos" => self.brew.as_ref().map(|b| Command::new(b).args(["install", "llvm"])),
            "windows" => self.winget.as_ref().map(|w| Command::new(w).args(["install", "-e", "--id", "LLVM.LLVM"])),
            _ => None,
        }
    }
}

/// A path's last part, without `.exe`: what a program's called.
pub fn program_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".exe").unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::host::mock::Mock;

    #[test]
    fn whats_there_is_found_where_llvm_puts_it() {
        let mut host = Mock::default();
        for (name, path) in [("g++", "/usr/bin/g++"), ("lldb-dap", "/usr/lib/llvm-18/bin/lldb-dap"), ("gdb", "/usr/bin/gdb"), ("vcpkg", "/home/u/vcpkg/vcpkg"), ("cmake", "/usr/bin/cmake")] {
            host.programs.insert(name.into(), path.into());
        }
        let t = Tools::find(&mut host);
        assert_eq!(t.compiler, Some(("g++".to_string(), "/usr/bin/g++".to_string())), "c++ and cc aren't there");
        assert_eq!(t.vcpkg_root().as_deref(), Some("/home/u/vcpkg"));
        assert_eq!(t.adapter(Debugger::LldbDap).map(|(d, c)| (d, line(&c))), Some((Debugger::LldbDap, "/usr/lib/llvm-18/bin/lldb-dap".to_string())));
        assert_eq!(t.adapter(Debugger::Gdb).map(|(d, c)| (d, line(&c))), Some((Debugger::Gdb, "/usr/bin/gdb --interpreter=dap".to_string())));
        let no_gdb = Tools { gdb: None, ..t.clone() };
        assert_eq!(no_gdb.adapter(Debugger::Gdb).map(|(d, _)| d), Some(Debugger::LldbDap), "else the other");
        assert_eq!(Tools::default().adapter(Debugger::Gdb), None);
        assert!(hints().contains(&"/usr/lib/llvm-18/bin".to_string()) && vcpkg_hints()[0] == "$VCPKG_ROOT");
        assert_eq!(line(&t.cmake()), "/usr/bin/cmake");
        assert_eq!(line(&t.ctest()), "ctest");
        assert_eq!(Debugger::parse(" GDB"), Debugger::Gdb);
        assert_eq!(Debugger::parse(""), Debugger::LldbDap);
        assert_eq!(program_name("C:\\LLVM\\bin\\clangd.exe"), "clangd");
    }

    #[test]
    fn a_tool_is_installed_with_uv_then_pipx_then_pip() {
        let mut t = Tools { uv: Some("/u/uv".into()), pipx: Some("/u/pipx".into()), python: Some("/usr/bin/python3".into()), ..Tools::default() };
        assert_eq!(t.install_tool("clangd").map(|c| line(&c)).as_deref(), Some("/u/uv tool install clangd"));
        t.uv = None;
        assert_eq!(t.install_tool("cmake").map(|c| line(&c)).as_deref(), Some("/u/pipx install cmake"));
        t.pipx = None;
        assert_eq!(t.install_tool("ninja").map(|c| line(&c)).as_deref(), Some("/usr/bin/python3 -m pip install --user ninja"));
        t.python = None;
        assert_eq!(t.install_tool("ninja"), None);
    }

    #[test]
    fn the_compiler_and_llvm_are_installed_as_the_system_does() {
        let mac = Tools { os: "macos".into(), brew: Some("/opt/homebrew/bin/brew".into()), xcode_select: Some("/usr/bin/xcode-select".into()), ..Tools::default() };
        assert_eq!(mac.install_compiler().map(|c| line(&c)).as_deref(), Some("/usr/bin/xcode-select --install"));
        assert_eq!(mac.install_llvm().map(|c| line(&c)).as_deref(), Some("/opt/homebrew/bin/brew install llvm"));
        let win = Tools { os: "windows".into(), winget: Some("winget".into()), ..Tools::default() };
        assert!(win.install_compiler().map(|c| line(&c)).unwrap().starts_with("winget install -e --id Microsoft.VisualStudio.2022.BuildTools"));
        assert_eq!(win.install_llvm().map(|c| line(&c)).as_deref(), Some("winget install -e --id LLVM.LLVM"));
        let linux = Tools { os: "linux".into(), brew: Some("/home/linuxbrew/bin/brew".into()), ..Tools::default() };
        assert_eq!((linux.install_compiler(), linux.install_llvm()), (None, None), "root's packages");
    }
}
