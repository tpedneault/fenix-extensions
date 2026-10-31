//! The project doctor's Zig checks: the Zig that builds the project, held
//! to `build.zig.zon`'s `minimum_zig_version`, and ZLS, held to that Zig:
//! a ZLS knows the Zig of its own version (0.17 for 0.17), and reads
//! another's standard library and build wrongly. Each that's missing has
//! a fix row installing the release for this machine.

use fenix_extension_api::Health;
use fenix_lang_kit::{find_version, Fix, Output, Program, Version};

use crate::project::Project;
use crate::tools::{hints, Platform};

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// Its fix row installs the release of that name, when there's one for
/// this machine (`zig.install.<name>`).
fn release_fix(program: Program, label: &str, name: &str, platform: &Platform) -> Program {
    if platform.installs().iter().any(|i| i.name == name) {
        program.fix(label, &format!("zig.install.{name}"), Fix::Install(name.into()))
    } else {
        program
    }
}

/// Whether ZLS's `version` is for Zig's `zig`: the same major and minor.
pub fn matches(zls: &Version, zig: &Version) -> bool {
    zls.major() == zig.major() && zls.number(1) == zig.number(1)
}

/// What `zls --version` said, against the Zig found (`None` until it's
/// said its own).
pub fn zls(out: &Output, zig: Option<&Version>) -> (Health, String) {
    let Some(version) = find_version(&out.text()) else {
        return (Health::Warn, format!("its version couldn't be read: {}", out.why()));
    };
    match zig {
        Some(zig) if !matches(&version, zig) => (Health::Warn, format!("{version}, but Zig is {zig}: ZLS goes with the Zig of its own version")),
        Some(zig) => (Health::Ok, format!("{version}, for Zig {zig}")),
        None => (Health::Ok, version.to_string()),
    }
}

/// The checks for `project`, with Zig's version as it was last found.
pub fn programs(project: &Project, zig: Option<&Version>, platform: &Platform) -> Vec<Program> {
    let mut out = Vec::new();
    let zig_check = program("zig", "Zig").version_args(["version"]);
    let zig_check = match project.minimum_zig() {
        Some(min) => zig_check.at_least(min),
        None => zig_check,
    };
    out.push(release_fix(zig_check, "Install Zig", "zig", platform));
    let held = zig.cloned();
    let zls_check = program("zls", "ZLS").optional().judge(move |out| zls(out, held.as_ref()));
    out.push(release_fix(zls_check, "Install ZLS", "zls", platform));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Zon;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn zig_is_held_to_build_zig_zon() {
        let project = Project { build: true, zon: Some(Zon::parse(include_str!("samples/build.zig.zon"))), ..Project::default() };
        let p = programs(&project, None, &Platform::new("linux", "x86_64"));
        assert_eq!(rows(&p), [("Zig".into(), true, Some("zig.install.zig".into())), ("ZLS".into(), false, Some("zig.install.zls".into()))]);
        assert_eq!(p[0].hints[0], "/tools/zig");
        let c = p[0].evaluate(Some("/t/zig"), Some(&out(0, "0.16.0\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "0.16.0, but the project needs >=0.17.0 (/t/zig)"));
        assert_eq!(c.fix.map(|f| f.1), Some("zig.install.zig".into()));
        assert_eq!(p[0].evaluate(Some("/t/zig"), Some(&out(0, "0.17.0\n"))).health, Health::Ok);
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad, "no Zig, nothing builds");

        let windows = programs(&Project::default(), None, &Platform::new("windows", "x86_64"));
        assert_eq!(rows(&windows), [("Zig".into(), true, None), ("ZLS".into(), false, Some("zig.install.zls".into()))], "no Zig release for Windows");
        assert!(windows[0].wants.is_none(), "no build.zig.zon: any Zig");
    }

    #[test]
    fn zls_is_held_to_the_zig_found() {
        let zig = Version::parse("0.17.0").unwrap();
        let p = programs(&Project::default(), Some(&zig), &Platform::new("linux", "x86_64"));
        let c = p[1].evaluate(Some("/t/zls"), Some(&out(0, "0.17.0\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "0.17.0, for Zig 0.17.0"));
        let c = p[1].evaluate(Some("/t/zls"), Some(&out(0, "0.16.0\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "0.16.0, but Zig is 0.17.0: ZLS goes with the Zig of its own version"));
        assert!(c.fix.is_some(), "the fix row installs the ZLS for 0.17");
        let dev = Version::parse("0.18.0-dev.1290+0e9a1d8d4").unwrap();
        assert!(matches(&Version::parse("0.18.0-dev.312+4f4f2b5ba").unwrap(), &dev), "a nightly for a nightly");
        assert_eq!(zls(&out(0, "0.17.0\n"), None), (Health::Ok, "0.17.0".into()), "Zig not yet said");
        assert_eq!(zls(&out(1, ""), Some(&zig)).0, Health::Warn);
        assert_eq!(p[1].evaluate(None, None).health, Health::Warn, "Fenix works without it");
    }
}
