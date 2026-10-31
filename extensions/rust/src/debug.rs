//! The debug adapter, and the launches cargo's targets make. lldb-dap
//! speaks DAP over stdio; CodeLLDB's `codelldb` over a socket
//! (`debug-adapter-connect`, 0.8.4), Fenix picking the port.
//!
//! Each binary and example is a launch of its own (`launches-set`, 0.8.6):
//! its program where cargo puts it, built first by its task
//! (`preLaunchTask`), and rustc's lldb formatters loaded, as rust-lldb
//! loads them, so `String` and `Vec` show what they hold. The tests in a
//! file are the launch file's, since a test binary's name has a hash.

use fenix_extension_api::serde_json::{json, Value};
use fenix_extension_api::{Command, LaunchConfig};

use crate::project::{Metadata, Package, Target};
use crate::tools::Debugger;

/// Fenix's name for what's debugged.
const RUST: &[&str] = &["rust"];

/// The adapter's command, and whether Fenix connects to it over a socket.
pub fn adapter(debugger: Debugger, path: &str) -> (Command, bool) {
    match debugger {
        Debugger::LldbDap => (Command::new(path), false),
        Debugger::CodeLldb => (Command::new(path).args(["--port", "${port}"]), true),
    }
}

/// The task that builds `target`: what a launch runs first.
pub fn build_task(target: &Target) -> String {
    if target.is("example") {
        format!("build example {}", target.name)
    } else {
        format!("build {}", target.name)
    }
}

/// `cargo build`'s arguments for `target` of `package` alone.
pub fn build_args(m: &Metadata, package: &Package, target: &Target) -> Vec<String> {
    let mut args = vec!["build".to_string()];
    if m.several() {
        args.extend(["-p".to_string(), package.name.clone()]);
    }
    let flag = if target.is("example") { "--example" } else { "--bin" };
    args.extend([flag.to_string(), target.name.clone()]);
    if !target.required_features.is_empty() {
        args.extend(["--features".to_string(), target.required_features.join(",")]);
    }
    args
}

/// lldb's commands that load rustc's formatters from `sysroot`
/// (`rustc --print sysroot`): each only where its file is. A Windows MSVC
/// toolchain has `lldb_lookup.py` but no `lldb_commands`, and CodeLLDB
/// refused the whole launch over it ("Error reading commands from file
/// lldb_commands - file not found"), `-s 1` or not. The pack can't look in
/// the sysroot itself, so lldb's Python does.
pub fn formatters(sysroot: &str) -> Vec<String> {
    let etc = format!("{}/lib/rustlib/etc", sysroot.trim_end_matches(['/', '\\']));
    // Raw strings: a Windows sysroot's backslashes (`C:\Program Files`) are
    // no escapes. `_ =`: the script's value isn't printed to the console.
    vec![format!(
        "script import os; _ = [lldb.debugger.HandleCommand(c) for f, c in ((r'''{etc}/lldb_lookup.py''', r'''command script import \"{etc}/lldb_lookup.py\"'''), (r'''{etc}/lldb_commands''', r'''command source -s 1 \"{etc}/lldb_commands\"''')) if os.path.isfile(f)]"
    )]
}

/// A launch for each binary and example: built by its task, then run
/// from cargo's target folder (`.exe` on Windows).
pub fn launches(m: &Metadata, windows: bool, sysroot: Option<&str>) -> Vec<LaunchConfig> {
    let exe = if windows { ".exe" } else { "" };
    let dir = m.target_directory.replace('\\', "/");
    let dir = dir.trim_end_matches('/');
    let mut attributes = json!({ "sourceLanguages": ["rust"] });
    if let Some(sysroot) = sysroot {
        attributes["initCommands"] = Value::from(formatters(sysroot));
    }
    let mut out = Vec::new();
    for kind in ["bin", "example"] {
        for (_, t) in m.targets(kind) {
            let (name, program) = if kind == "bin" { (format!("Rust: debug {}", t.name), format!("{dir}/debug/{}{exe}", t.name)) } else { (format!("Rust: debug example {}", t.name), format!("{dir}/debug/examples/{}{exe}", t.name)) };
            out.push(LaunchConfig::launch(&name, RUST).program(&program).cwd("${project}").attributes(&attributes).pre_launch_task(&build_task(t)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{ok, METADATA};
    use crate::tests::line;

    #[test]
    fn each_binary_and_example_is_a_launch_built_first() {
        let m = Metadata::parse(&ok(METADATA)).unwrap();
        let l = launches(&m, false, Some("/home/u/.rustup/toolchains/stable"));
        let rows: Vec<(&str, Option<&str>, Option<&str>)> = l.iter().map(|l| (l.name.as_str(), l.program.as_deref(), l.pre_launch_task.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("Rust: debug orbit", Some("/home/u/orbit/target/debug/orbit"), Some("build orbit")),
                ("Rust: debug probe", Some("/home/u/orbit/target/debug/probe"), Some("build probe")),
                ("Rust: debug example demo", Some("/home/u/orbit/target/debug/examples/demo"), Some("build example demo")),
            ]
        );
        let attrs: Value = fenix_extension_api::serde_json::from_str(&l[0].attributes).unwrap();
        let init = attrs["initCommands"][0].as_str().unwrap();
        assert!(init.starts_with("script import os; _ = [") && init.contains("os.path.isfile(f)"), "each only where its file is: {init}");
        assert!(!init.contains(", 'command"), "raw strings, for a Windows sysroot's backslashes: {init}");
        assert!(init.contains("command script import \"/home/u/.rustup/toolchains/stable/lib/rustlib/etc/lldb_lookup.py\""), "{init}");
        assert!(init.contains("command source -s 1 \"/home/u/.rustup/toolchains/stable/lib/rustlib/etc/lldb_commands\""), "{init}");
        assert_eq!(attrs["initCommands"].as_array().unwrap().len(), 1);
        assert_eq!(attrs["sourceLanguages"][0], "rust", "CodeLLDB's Rust");
        let windows = launches(&Metadata { target_directory: "C:\\work\\orbit\\target".into(), ..m.clone() }, true, None);
        assert_eq!(windows[0].program.as_deref(), Some("C:/work/orbit/target/debug/orbit.exe"));
        assert!(!windows[0].attributes.contains("initCommands"), "no sysroot yet");
        let (_, probe) = m.targets("bin")[1];
        assert_eq!(build_args(&m, &m.packages[0], probe), ["build", "-p", "orbit", "--bin", "probe"]);
    }

    #[test]
    fn codelldb_is_connected_to_and_lldb_dap_spoken_to() {
        assert!(!adapter(Debugger::LldbDap, "/usr/bin/lldb-dap").1, "stdio");
        let (cmd, socket) = adapter(Debugger::CodeLldb, "/opt/codelldb/adapter/codelldb");
        assert_eq!((line(&cmd).as_str(), socket), ("/opt/codelldb/adapter/codelldb --port ${port}", true));
    }
}
