//! The project doctor's Swift checks, in a Swift project: the Swift that
//! builds it, held to `Package.swift`'s tools version; sourcekit-lsp (or
//! Xcode's, through `xcrun`); the formatter in use, swift-format or
//! SwiftFormat; and SwiftLint where it lints. Swift is a toolchain the
//! pack doesn't install: its fix row opens swift.org's page for this OS,
//! which says how (swiftly, Xcode, the installer). SwiftLint's installs
//! its release; on macOS Homebrew installs the formatters.

use fenix_extension_api::{Command, Health};
use fenix_lang_kit::{find_version, Fix, Output, Program, Requirement, Version};

use crate::project::Project;
use crate::tools::{hints, Platform, Tools};

/// The fix rows' commands, in the manifest.
pub const GET_SWIFT: &str = "swift.get-swift";
pub const INSTALL_SWIFTLINT: &str = "swift.install.swiftlint";
pub const INSTALL_SWIFT_FORMAT: &str = "swift.install-swift-format";
pub const INSTALL_SWIFTFORMAT: &str = "swift.install-swiftformat";

/// Swift's own version in what `swift --version` said: Apple's starts
/// with the driver's (`swift-driver version: 1.127.14.1 Apple Swift
/// version 6.2.1`), so the one after `Swift version` is it.
pub fn swift_version(text: &str) -> Option<Version> {
    let (_, after) = text.split_once("Swift version")?;
    find_version(after)
}

/// What `swift --version` said, held to `wants` (`Package.swift`'s
/// tools version, or later).
pub fn judge_swift(wants: Option<Requirement>) -> impl Fn(&Output) -> (Health, String) {
    move |out| match (swift_version(&out.text()), &wants) {
        (Some(v), Some(r)) if !r.matches(&v) => (Health::Bad, format!("{v}, but Package.swift needs {r}")),
        (Some(v), _) => (Health::Ok, v.to_string()),
        (None, _) if out.ok() => (Health::Warn, "its version couldn't be read".into()),
        (None, _) => (Health::Warn, format!("asking its version failed: {}", out.why())),
    }
}

/// What `xcrun --find sourcekit-lsp` said: Xcode's server, or none.
pub fn judge_xcrun(out: &Output) -> (Health, String) {
    match out.stdout.lines().map(str::trim).find(|l| l.ends_with("sourcekit-lsp")) {
        Some(path) if out.ok() => (Health::Ok, format!("Xcode's ({path})")),
        _ => (Health::Warn, "Xcode's isn't there: install Xcode or a Swift toolchain".into()),
    }
}

/// `name`, looked for where the toolchains and the pack put it.
fn program(name: &str, label: &str, tools: &Tools) -> Program {
    hints(name, tools.swift.as_deref()).into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine; `apple` when
/// swift-format formats it, `lockwood` SwiftFormat, `lint` when SwiftLint
/// lints it.
pub fn programs(project: &Project, tools: &Tools, platform: &Platform, (apple, lockwood, lint): (bool, bool, bool)) -> Vec<Program> {
    let mut out = Vec::new();
    if !project.is_swift() {
        return out;
    }

    // Swift, held to the tools version: needed to build a package.
    let wants = project.tools().and_then(|v| Requirement::parse(&format!(">={v}")));
    let swift = program("swift", "Swift", tools).judge(judge_swift(wants));
    let swift = if project.package.is_some() { swift } else { swift.optional() };
    out.push(swift.fix("Get Swift (swift.org)", GET_SWIFT, Fix::Own));

    // The server: on macOS without one found, Xcode's.
    let server = match (&tools.sourcekit_lsp, &tools.xcrun) {
        (None, Some(_)) => Program::new("xcrun", "sourcekit-lsp").version_args(["--find", "sourcekit-lsp"]).judge(judge_xcrun),
        _ => program("sourcekit-lsp", "sourcekit-lsp", tools).version_args(Vec::<String>::new()),
    };
    out.push(server.optional().fix("Get Swift, which has it (swift.org)", GET_SWIFT, Fix::Own));

    if apple {
        let p = match (&tools.swift_format, &tools.swift) {
            (None, Some(_)) => program("swift", "swift-format", tools).version_args(["format", "--version"]),
            _ => program("swift-format", "swift-format", tools),
        };
        out.push(fix(p.optional(), "Install swift-format (Homebrew)", INSTALL_SWIFT_FORMAT, tools.brew_install("swift-format")));
    }
    if lockwood {
        let p = program("swiftformat", "SwiftFormat", tools).optional();
        out.push(fix(p, "Install SwiftFormat (Homebrew)", INSTALL_SWIFTFORMAT, tools.brew_install("swiftformat")));
    }
    if lint {
        let p = program("swiftlint", "SwiftLint", tools).version_args(["version"]).optional();
        out.push(match platform.swiftlint() {
            Some(_) => p.fix(&format!("Install SwiftLint {}", crate::tools::SWIFTLINT), INSTALL_SWIFTLINT, Fix::Install("swiftlint".into())),
            None => p,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Package;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn package(tools: &str) -> Project {
        Project { package: Some(Package { tools: Some(tools.into()), executables: Vec::new() }), ..Project::default() }
    }

    const LINUX: &str = "Swift version 6.2.1 (swift-6.2.1-RELEASE)\nTarget: x86_64-unknown-linux-gnu\n";
    const APPLE: &str = "swift-driver version: 1.127.14.1 Apple Swift version 6.2.1 (swiftlang-6.2.1.4.8 clang-1700.4.4.1)\nTarget: arm64-apple-macosx15.0\n";

    #[test]
    fn swifts_version_is_read_past_the_drivers() {
        assert_eq!(swift_version(LINUX).map(|v| v.to_string()).as_deref(), Some("6.2.1"));
        assert_eq!(swift_version(APPLE).map(|v| v.to_string()).as_deref(), Some("6.2.1"), "not the driver's 1.127.14.1");
        assert_eq!(swift_version("Swift version 5.10 (swift-5.10-RELEASE)").map(|v| v.to_string()).as_deref(), Some("5.10"));
        assert_eq!(swift_version("swift-driver version: 1.0"), None);
    }

    #[test]
    fn swift_is_held_to_the_tools_version() {
        let tools = Tools { swift: Some("/usr/bin/swift".into()), sourcekit_lsp: Some("/usr/bin/sourcekit-lsp".into()), ..Tools::default() };
        let linux = Platform::new("linux", "x86_64");
        let p = programs(&package("6.3"), &tools, &linux, (true, false, true));
        assert_eq!(
            rows(&p),
            [
                ("Swift".into(), "swift".into(), true, Some(GET_SWIFT.into())),
                ("sourcekit-lsp".into(), "sourcekit-lsp".into(), false, Some(GET_SWIFT.into())),
                ("swift-format".into(), "swift".into(), false, None),
                ("SwiftLint".into(), "swiftlint".into(), false, Some(INSTALL_SWIFTLINT.into())),
            ],
            "swift-format is Swift 6's; no Homebrew on Linux"
        );
        let c = p[0].evaluate(Some("/usr/bin/swift"), Some(&out(0, LINUX, "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "6.2.1, but Package.swift needs >=6.3"));
        let older = programs(&package("5.9"), &tools, &linux, (false, false, false));
        assert_eq!(older[0].evaluate(Some("/usr/bin/swift"), Some(&out(0, APPLE, ""))).health, Health::Ok);
        assert_eq!(older.len(), 2, "neither formatter nor linter in use");
        assert_eq!(p[2].version_args, ["format", "--version"]);
        assert_eq!(p[3].fix.as_ref().map(|f| f.2.clone()), Some(Fix::Install("swiftlint".into())));
        assert!(p[1].version_args.is_empty(), "a server only found");
        assert_eq!(p[1].hints[1], "/usr/bin", "beside swift");
        let failed = p[0].evaluate(Some("/usr/bin/swift"), Some(&out(127, "", "swift: error while loading shared libraries: libncurses.so.6")));
        assert_eq!(failed.health, Health::Warn);
        assert!(failed.detail.contains("libncurses"), "{}", failed.detail);
    }

    #[test]
    fn on_macos_xcode_has_the_server_and_homebrew_the_formatters() {
        let tools = Tools { xcrun: Some("/usr/bin/xcrun".into()), brew: Some("/opt/homebrew/bin/brew".into()), swiftformat: Some("/opt/homebrew/bin/swiftformat".into()), os: "macos".into(), ..Tools::default() };
        let project = Project { xcode: true, swiftformat: true, ..Project::default() };
        let p = programs(&project, &tools, &Platform::new("macos", "aarch64"), (false, true, false));
        assert_eq!(
            rows(&p),
            [
                ("Swift".into(), "swift".into(), false, Some(GET_SWIFT.into())),
                ("sourcekit-lsp".into(), "xcrun".into(), false, Some(GET_SWIFT.into())),
                ("SwiftFormat".into(), "swiftformat".into(), false, Some(INSTALL_SWIFTFORMAT.into())),
            ],
            "no Package.swift: Swift isn't needed to build"
        );
        let found = p[1].evaluate(Some("/usr/bin/xcrun"), Some(&out(0, "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/sourcekit-lsp\n", "")));
        assert_eq!((found.health, found.detail.as_str()), (Health::Ok, "Xcode's (/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/sourcekit-lsp)"));
        let none = p[1].evaluate(Some("/usr/bin/xcrun"), Some(&out(72, "", "xcrun: error: unable to find utility \"sourcekit-lsp\", not a developer tool or in PATH")));
        assert_eq!(none.health, Health::Warn);
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/opt/homebrew/bin/brew install swiftformat"),
            other => panic!("{other:?}"),
        }
        assert!(programs(&Project::default(), &tools, &Platform::default(), (true, true, true)).is_empty(), "not a Swift project");
    }
}
