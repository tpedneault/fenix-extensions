//! The project doctor's .NET checks: the SDK (the one `global.json` asks
//! for, or one new enough for the newest framework a project targets),
//! csharp-ls, netcoredbg, CSharpier where the project doesn't bring its
//! own, and whether the project to debug is built. Each that isn't right
//! has a fix row.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{find_version, Fix, Output, Program, Version};

use crate::project::{Project, Solution};
use crate::tools::{csharp_ls_for, hints, Tools};

/// The fix rows' commands, in the manifest.
pub const INSTALL_SDK: &str = "dotnet.install-sdk";
pub const INSTALL_SERVER: &str = "dotnet.install-server";
pub const INSTALL_DEBUGGER: &str = "dotnet.install-debugger";
pub const INSTALL_CSHARPIER: &str = "dotnet.install-csharpier";
pub const BUILD: &str = "dotnet.build";

/// `dotnet --version` in the project's folder, which `global.json`
/// steers: the SDK it picked, or that none fits.
pub fn sdk_for_global_json(out: &Output) -> (Health, String) {
    let text = out.text();
    if text.contains("compatible .NET SDK was not found") {
        let wanted = text.lines().find_map(|l| l.trim().strip_prefix("Requested SDK version:")).map(str::trim).unwrap_or("another");
        return (Health::Bad, format!("global.json asks for SDK {wanted}, and none installed fits it"));
    }
    match find_version(&text) {
        Some(v) if out.ok() => (Health::Ok, format!("{v}, as global.json asks")),
        _ => (Health::Warn, format!("dotnet --version failed: {}", out.why())),
    }
}

/// The download page for the SDK the project needs: `global.json`'s, or
/// the newest framework's.
pub fn sdk_page(solution: &Solution) -> String {
    let wanted = solution.global_json.as_ref().and_then(|g| g.version.as_deref()).and_then(Version::parse).map(|v| format!("{}.{}", v.major(), v.number(1))).or_else(|| solution.newest_framework());
    match wanted {
        Some(channel) => format!("https://dotnet.microsoft.com/download/dotnet/{channel}"),
        None => "https://dotnet.microsoft.com/download".into(),
    }
}

fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

/// The checks, with what's on this machine: `sdk` is the SDK's version
/// as last checked, which says the csharp-ls it can run; `debugger` is
/// whether netcoredbg can be installed here.
pub fn programs(solution: &Solution, tools: &Tools, sdk: Option<&Version>, debugger: bool) -> Vec<Program> {
    let mut out = Vec::new();
    let dotnet = program("dotnet", ".NET SDK");
    let dotnet = match (&solution.global_json, solution.newest_framework()) {
        (Some(_), _) => dotnet.judge(sdk_for_global_json),
        (None, Some(f)) => dotnet.at_least(&f),
        (None, None) => dotnet,
    };
    out.push(dotnet.fix("Download the SDK", INSTALL_SDK, Fix::Own));

    let server = program("csharp-ls", "Language server").optional();
    out.push(match &tools.dotnet {
        Some(_) => server.fix("Install csharp-ls", INSTALL_SERVER, Fix::Task(tools.install_tool("csharp-ls", csharp_ls_for(sdk)))),
        None => server,
    });

    let debug = program("netcoredbg", "Debugger").optional();
    out.push(if debugger { debug.fix("Install netcoredbg", INSTALL_DEBUGGER, Fix::Install("netcoredbg".into())) } else { debug });

    // CSharpier formats: the project's own, from its tools manifest, else
    // one installed for you.
    if solution.csharpier.is_none() {
        let c = program("csharpier", "CSharpier").optional();
        out.push(match &tools.dotnet {
            Some(_) => c.fix("Install CSharpier", INSTALL_CSHARPIER, Fix::Task(tools.install_tool("csharpier", None))),
            None => c,
        });
    }
    out
}

/// Whether `project`, the one to debug, is built: its dll there, under
/// `root`. The fix builds it.
pub fn built(root: &str, project: Option<&Project>) -> Option<Check> {
    let p = project?;
    let dll = p.dll();
    let check = if std::path::Path::new(root).join(&dll).is_file() {
        Check::new(Section::Project, "Debugging", Health::Ok, format!("{} ({dll})", p.name))
    } else {
        Check::new(Section::Project, "Debugging", Health::Warn, format!("{} isn't built yet: {dll}", p.name)).fix(&format!("Build {}", p.name), BUILD)
    };
    Some(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::GlobalJson;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(p: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        p.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn the_sdk_is_held_to_global_json() {
        let missing = Output { code: Some(145), ..out(0, include_str!("samples/version-globaljson.txt")) };
        assert_eq!(sdk_for_global_json(&missing), (Health::Bad, "global.json asks for SDK 9.0.100, and none installed fits it".into()));
        assert_eq!(sdk_for_global_json(&out(0, "8.0.132\n")), (Health::Ok, "8.0.132, as global.json asks".into()));
        assert_eq!(sdk_for_global_json(&Output { code: Some(1), stderr: "boom".into(), ..Output::default() }).0, Health::Warn);
    }

    #[test]
    fn each_check_has_its_fix() {
        let dir = crate::project::tests::orbit("doctor");
        let mut s = Solution::read(&dir.to_string_lossy(), None);
        let tools = Tools { dotnet: Some("/usr/bin/dotnet".into()), ..Tools::default() };
        let p = programs(&s, &tools, Version::parse("8.0.132").as_ref(), true);
        assert_eq!(
            rows(&p),
            [
                (".NET SDK".into(), "dotnet".into(), true, Some(INSTALL_SDK.into())),
                ("Language server".into(), "csharp-ls".into(), false, Some(INSTALL_SERVER.into())),
                ("Debugger".into(), "netcoredbg".into(), false, Some(INSTALL_DEBUGGER.into())),
                ("CSharpier".into(), "csharpier".into(), false, Some(INSTALL_CSHARPIER.into())),
            ]
        );
        // Held to the newest framework's SDK, without global.json.
        let c = p[0].evaluate(Some("/usr/bin/dotnet"), Some(&out(0, "7.0.410\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "7.0.410, but the project needs >=8.0 (/usr/bin/dotnet)"));
        match &p[1].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/usr/bin/dotnet tool update -g csharp-ls --version 0.16.0", "the one SDK 8 runs"),
            other => panic!("{other:?}"),
        }
        assert!(p[0].hints.contains(&"$HOME/.dotnet/tools".to_string()));
        assert_eq!(sdk_page(&s), "https://dotnet.microsoft.com/download/dotnet/8.0");

        // global.json, CSharpier the project's, no netcoredbg for this
        // machine, no dotnet to install with.
        s.global_json = Some(GlobalJson { version: Some("9.0.100".into()), roll_forward: None });
        s.csharpier = Some("1.3.0".into());
        let p = programs(&s, &Tools::default(), None, false);
        assert_eq!(rows(&p).iter().map(|r| (r.0.as_str(), r.3.is_some())).collect::<Vec<_>>(), [(".NET SDK", true), ("Language server", false), ("Debugger", false)]);
        assert!(p[0].judge.is_some());
        assert_eq!(sdk_page(&s), "https://dotnet.microsoft.com/download/dotnet/9.0");

        // The project to debug, built or not.
        let root = dir.to_string_lossy().into_owned();
        let app = s.by_name("Orbit.App");
        let c = built(&root, app).unwrap();
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "Orbit.App isn't built yet: src/Orbit.App/bin/Debug/net8.0/Orbit.App.dll"));
        crate::project::tests::write(&dir, "src/Orbit.App/bin/Debug/net8.0/Orbit.App.dll", "MZ");
        assert_eq!(built(&root, app).unwrap().health, Health::Ok);
        assert_eq!(built(&root, None), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
