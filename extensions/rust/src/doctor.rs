//! The project doctor's Rust checks: the toolchain `rust-toolchain.toml`
//! pins, installed; rustc, held to `rust-version`; cargo; rust-analyzer,
//! clippy and rustfmt, rustup's components; lldb-dap or CodeLLDB;
//! cargo-audit and cargo-outdated. Each that isn't right has a fix row: `rustup
//! toolchain install`, `rustup component add`, `cargo install`.

use fenix_extension_api::{Command, Health};
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::{Metadata, Project};
use crate::tools::{hints, Debugger, Tools, CODELLDB};

/// The fix rows' commands, in the manifest.
pub const INSTALL_TOOLCHAIN: &str = "rust.install-toolchain";
pub const UPDATE_TOOLCHAIN: &str = "rust.update-toolchain";
pub const INSTALL_RUST_ANALYZER: &str = "rust.install-rust-analyzer";
pub const INSTALL_CLIPPY: &str = "rust.install-clippy";
pub const INSTALL_RUSTFMT: &str = "rust.install-rustfmt";
pub const INSTALL_LLDB_DAP: &str = "rust.install-lldb-dap";
pub const INSTALL_CARGO_AUDIT: &str = "rust.install-cargo-audit";
pub const INSTALL_CARGO_OUTDATED: &str = "rust.install-cargo-outdated";

/// What a failed run said, in a line: its first `error:` line, since
/// rustup's proxies may print a backtrace after it.
pub fn error_line(out: &Output) -> String {
    let text = out.text();
    text.lines().map(str::trim).find(|l| l.starts_with("error")).map(str::to_string).unwrap_or_else(|| out.why())
}

/// A rustup component's program (`rust-analyzer --version`, `cargo clippy
/// --version`): rustup's proxy is there whether or not the component is.
/// `channel` is the toolchain the project pins, if it does.
pub fn component(channel: Option<String>) -> impl Fn(&Output) -> (Health, String) {
    move |out| {
        let text = out.text();
        if out.ok() {
            let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            return (Health::Ok, first.to_string());
        }
        if text.contains("Unknown binary") || text.contains("is not installed") {
            let toolchain = channel.as_deref().map(|c| format!("the project's {c}")).unwrap_or_else(|| "this toolchain".into());
            return (Health::Warn, format!("not installed for {toolchain}"));
        }
        (Health::Warn, format!("asking its version failed: {}", error_line(out)))
    }
}

/// `rustup run <channel> rustc --version`: the toolchain the project pins
/// is installed.
pub fn toolchain(channel: &str) -> impl Fn(&Output) -> (Health, String) {
    let channel = channel.to_string();
    move |out| {
        if out.ok() {
            return (Health::Ok, format!("{channel}: {}", out.text().trim()));
        }
        let line = error_line(out);
        if line.contains("is not installed") {
            return (Health::Bad, format!("{channel} isn't installed ({line})"));
        }
        (Health::Bad, line)
    }
}

/// A cargo subcommand of its own (`cargo audit --version`).
pub fn subcommand(out: &Output) -> (Health, String) {
    let text = out.text();
    if out.ok() {
        return (Health::Ok, fenix_lang_kit::find_version(&text).map(|v| v.text).unwrap_or_else(|| text.trim().to_string()));
    }
    if text.contains("no such command") {
        return (Health::Warn, "isn't installed".into());
    }
    (Health::Warn, format!("asking its version failed: {}", error_line(out)))
}

/// `name`, looked for where rustup and LLVM put theirs too.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// `rustup toolchain install` for what the toolchain file says.
pub fn install_toolchain(project: &Project, tools: &Tools) -> Option<Command> {
    let t = project.toolchain.as_ref()?;
    tools.rustup.as_ref()?;
    let mut cmd = tools.rustup().args(["toolchain", "install", &t.channel]);
    if !t.components.is_empty() {
        cmd = cmd.args(["--component".to_string(), t.components.join(",")]);
    }
    if !t.targets.is_empty() {
        cmd = cmd.args(["--target".to_string(), t.targets.join(",")]);
    }
    Some(cmd)
}

/// The checks for `project`, with what's on this machine; `metadata`'s
/// `rust-version`s once cargo has said them.
pub fn programs(project: &Project, metadata: Option<&Metadata>, tools: &Tools, debugger: Debugger) -> Vec<Program> {
    let mut out = Vec::new();
    let pinned = project.toolchain.as_ref();
    let channel = pinned.map(|t| t.channel.clone());

    if let Some(t) = pinned {
        let p = program("rustup", "Toolchain").version_args(["run", t.channel.as_str(), "rustc", "--version"]).judge(toolchain(&t.channel));
        out.push(fix(p, &format!("Install {} (rustup)", t.channel), INSTALL_TOOLCHAIN, install_toolchain(project, tools)));
    }

    // rustc, in the project: the toolchain it pins, else the default.
    let wants = metadata.and_then(Metadata::rust_version).map(|v| format!(">={v}")).or_else(|| project.wants_rustc());
    let rustc = program("rustc", "Rust");
    let rustc = match wants {
        Some(w) => rustc.wants(&w),
        None => rustc,
    };
    let update = if pinned.is_none() { tools.rustup.as_ref().map(|_| tools.rustup().arg("update")) } else { None };
    out.push(fix(rustc, "Update Rust (rustup update)", UPDATE_TOOLCHAIN, update));

    out.push(program("cargo", "cargo"));

    // rust-analyzer's fix is the pack's own: the server is started again
    // once it's there.
    let ra = program("rust-analyzer", "rust-analyzer").optional().judge(component(channel.clone()));
    out.push(match tools.add_component("rust-analyzer") {
        Some(_) => ra.fix("Install rust-analyzer (rustup)", INSTALL_RUST_ANALYZER, Fix::Own),
        None => ra,
    });
    let clippy = program("cargo", "clippy").version_args(["clippy", "--version"]).optional().judge(component(channel.clone()));
    out.push(fix(clippy, "Install clippy (rustup)", INSTALL_CLIPPY, tools.add_component("clippy")));
    let rustfmt = program("rustfmt", "rustfmt").optional().judge(component(channel));
    out.push(fix(rustfmt, "Install rustfmt (rustup)", INSTALL_RUSTFMT, tools.add_component("rustfmt")));

    // The debugger is LLVM's or CodeLLDB's, not rustup's: its fix row says
    // how to get it. The one used, else the one asked for.
    let debugger = tools.debugger(debugger).map(|(d, _)| d).unwrap_or(debugger);
    let adapter = match debugger {
        Debugger::LldbDap => program(tools.lldb_dap_name(), "lldb-dap"),
        Debugger::CodeLldb => std::iter::once(CODELLDB.to_string()).chain(hints()).fold(Program::new("codelldb", "CodeLLDB"), Program::hint),
    };
    out.push(adapter.version_args(Vec::<String>::new()).optional().fix("How to install the debugger", INSTALL_LLDB_DAP, Fix::Own));

    for (name, command) in [("cargo-audit", INSTALL_CARGO_AUDIT), ("cargo-outdated", INSTALL_CARGO_OUTDATED)] {
        let sub = name.trim_start_matches("cargo-");
        let p = program("cargo", name).version_args([sub, "--version"]).optional().judge(subcommand);
        out.push(fix(p, &format!("Install {name}"), command, tools.install_crate(name)));
    }
    out
}

/// What the lldb-dap fix row opens: where each system has it.
pub const LLDB_DAP_HELP: &str = "\
lldb-dap is LLVM's debug adapter; rustup doesn't ship it. Install LLVM's
lldb, then check the project again (SPC m d):

  Debian, Ubuntu   sudo apt install lldb        (lldb-dap-18 and later)
  Fedora           sudo dnf install lldb
  Arch             sudo pacman -S lldb
  macOS            Xcode 16's command line tools have it (xcrun lldb-dap),
                   or: brew install llvm
  Windows          winget install LLVM.LLVM

The launch files build with cargo through lldb's Python, which these
packages include.

Or CodeLLDB (rust.debugger = codelldb): unpack the .vsix for your system
from github.com/vadimcn/codelldb/releases (it's a zip) and set
rust.codelldb to its extension/adapter/codelldb; VS Code's copy, under
~/.vscode/extensions/vadimcn.vscode-lldb-*/adapter/, does too.
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Toolchain;
    use crate::tests::line;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    /// rustup 1.28's proxy for a component that isn't installed, with
    /// `RUST_BACKTRACE` set.
    const NO_RUST_ANALYZER: &str = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable-x86_64-unknown-linux-gnu'.\n\nStack backtrace:\n   0: anyhow::kind::Adhoc::new\n  13: __libc_start_main_impl\n             at ./csu/../csu/libc-start.c:360:3\n  14: <unknown>";
    const NO_TOOLCHAIN: &str = "error: toolchain '1.70.0-x86_64-unknown-linux-gnu' is not installed\nhelp: run `rustup toolchain install 1.70.0-x86_64-unknown-linux-gnu` to install it\n\nStack backtrace:\n   0: anyhow::error::<impl core::convert::From<E> for anyhow::Error>::from\n  11: <unknown>";

    #[test]
    fn judges_read_what_rustup_and_cargo_said() {
        let component = component(None);
        assert_eq!(component(&out(0, "rust-analyzer 1.97.0 (2d8144b 2026-07-07)\n", "")), (Health::Ok, "rust-analyzer 1.97.0 (2d8144b 2026-07-07)".into()));
        assert_eq!(component(&out(1, "", NO_RUST_ANALYZER)), (Health::Warn, "not installed for this toolchain".into()));
        assert_eq!(component(&out(1, "", "error: 'cargo-clippy' is not installed for the toolchain 'stable-x86_64-unknown-linux-gnu'.")).1, "not installed for this toolchain");
        assert_eq!(component(&out(1, "", "boom\n")).1, "asking its version failed: boom");
        assert_eq!(super::component(Some("1.82.0".into()))(&out(1, "", NO_RUST_ANALYZER)).1, "not installed for the project's 1.82.0", "the judge knows the pin");
        let toolchain = toolchain("1.70.0");
        assert_eq!(toolchain(&out(1, "", NO_TOOLCHAIN)), (Health::Bad, "1.70.0 isn't installed (error: toolchain '1.70.0-x86_64-unknown-linux-gnu' is not installed)".into()), "not the backtrace's last line");
        assert_eq!(toolchain(&out(0, "rustc 1.97.0 (2d8144b78 2026-07-07)\n", "")), (Health::Ok, "1.70.0: rustc 1.97.0 (2d8144b78 2026-07-07)".into()));
        assert_eq!(subcommand(&out(0, "cargo-audit-audit 0.22.2\n", "")), (Health::Ok, "0.22.2".into()));
        assert_eq!(subcommand(&out(0, "cargo-outdated-outdated 0.19.0\n", "")).1, "0.19.0");
        let missing = out(101, "", "error: no such command: `outdated`\n\nhelp: view all installed commands with `cargo --list`\nhelp: find a package to install `outdated` with `cargo search cargo-outdated`");
        assert_eq!(subcommand(&missing), (Health::Warn, "isn't installed".into()));
    }

    #[test]
    fn a_pinned_toolchain_is_checked_and_installed_with_its_components() {
        let project = Project { cargo: true, rust_version: Some("1.80".into()), toolchain: Some(Toolchain { channel: "1.82.0".into(), components: vec!["clippy".into(), "rustfmt".into()], targets: vec!["wasm32-wasip2".into()] }), ..Project::default() };
        let tools = Tools { cargo: Some("/c/cargo".into()), rustup: Some("/c/rustup".into()), lldb_dap: Some("/usr/bin/lldb-dap-18".into()), ..Tools::default() };
        let p = programs(&project, None, &tools, Debugger::LldbDap);
        assert_eq!(
            rows(&p),
            [
                ("Toolchain".into(), "rustup".into(), true, Some(INSTALL_TOOLCHAIN.into())),
                ("Rust".into(), "rustc".into(), true, None),
                ("cargo".into(), "cargo".into(), true, None),
                ("rust-analyzer".into(), "rust-analyzer".into(), false, Some(INSTALL_RUST_ANALYZER.into())),
                ("clippy".into(), "cargo".into(), false, Some(INSTALL_CLIPPY.into())),
                ("rustfmt".into(), "rustfmt".into(), false, Some(INSTALL_RUSTFMT.into())),
                ("lldb-dap".into(), "lldb-dap-18".into(), false, Some(INSTALL_LLDB_DAP.into())),
                ("cargo-audit".into(), "cargo".into(), false, Some(INSTALL_CARGO_AUDIT.into())),
                ("cargo-outdated".into(), "cargo".into(), false, Some(INSTALL_CARGO_OUTDATED.into())),
            ],
            "pinned: rustup update isn't the fix"
        );
        assert_eq!(p[0].version_args, ["run", "1.82.0", "rustc", "--version"]);
        match &p[0].fix {
            Some((label, _, Fix::Task(cmd))) => assert_eq!((label.as_str(), line(cmd).as_str()), ("Install 1.82.0 (rustup)", "/c/rustup toolchain install 1.82.0 --component clippy,rustfmt --target wasm32-wasip2")),
            other => panic!("{other:?}"),
        }
        let c = p[0].evaluate(Some("/c/rustup"), Some(&out(1, "", NO_TOOLCHAIN)));
        assert_eq!((c.health, c.fix.map(|f| f.1)), (Health::Bad, Some(INSTALL_TOOLCHAIN.into())));
        // rustc held to rust-version.
        let old = p[1].evaluate(Some("/c/rustc"), Some(&out(0, "rustc 1.79.0 (129f3b996 2024-06-10)\n", "")));
        assert_eq!((old.health, old.detail.as_str()), (Health::Bad, "1.79.0, but the project needs >=1.80 (/c/rustc)"));
        assert!(p[6].version_args.is_empty(), "lldb-dap is only found");
        match &p[7].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(line(cmd), "/c/cargo install --locked cargo-audit"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_a_pin_rust_is_updated_and_metadata_says_the_rust_version() {
        let project = Project { cargo: true, rust_version: Some("1.60".into()), ..Project::default() };
        let tools = Tools { rustup: Some("/c/rustup".into()), ..Tools::default() };
        let metadata = crate::project::Metadata::parse(&crate::project::tests::ok(crate::project::tests::METADATA)).unwrap();
        let p = programs(&project, Some(&metadata), &tools, Debugger::LldbDap);
        assert_eq!(p[0].label, "Rust");
        assert_eq!(p[0].wants.as_ref().map(|w| w.to_string()).as_deref(), Some(">=1.70"), "orbit's rust-version, cargo's answer");
        match &p[0].fix {
            Some((_, command, Fix::Task(cmd))) => assert_eq!((command.as_str(), line(cmd).as_str()), (UPDATE_TOOLCHAIN, "/c/rustup update")),
            other => panic!("{other:?}"),
        }
        let p = programs(&project, None, &Tools::default(), Debugger::LldbDap);
        assert_eq!(p[0].wants.as_ref().map(|w| w.to_string()).as_deref(), Some(">=1.60"));
        assert!(p.iter().filter(|p| p.label != "lldb-dap").all(|p| p.fix.is_none()), "no rustup, no cargo: nothing to install with");
        assert_eq!(p[5].name, "lldb-dap", "its own name, when not found");
        // CodeLLDB asked for: it's the one checked, where you put it first.
        let p = programs(&project, None, &Tools::default(), Debugger::CodeLldb);
        assert_eq!((p[5].label.as_str(), p[5].name.as_str(), p[5].hints[0].as_str()), ("CodeLLDB", "codelldb", CODELLDB));
        // lldb-dap asked for, but only CodeLLDB is here: CodeLLDB's checked.
        let only = Tools { codelldb: Some("/opt/codelldb".into()), ..Tools::default() };
        assert_eq!(programs(&project, None, &only, Debugger::LldbDap)[5].label, "CodeLLDB");
    }
}
