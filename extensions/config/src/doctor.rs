//! The project doctor's checks: for each kind of file the project has
//! (and the pack's setting for it is on), its language server, and the
//! formatter and linters that go with it. Each that's missing has a fix
//! row: npm for the servers written in Node, the release for this machine
//! for the rest. None of them is needed, so none is worse than a warning.
//! The servers that would start a server to say a version are only
//! found.

use fenix_extension_api::Command;
use fenix_lang_kit::{Fix, Program};

use crate::project::{Kind, Project};
use crate::tools::{self, hints, Platform, Tools};
use crate::Settings;

/// The fix rows' commands, in the manifest.
pub const INSTALL_YAML: &str = "config.install-yaml-server";
pub const INSTALL_JSON: &str = "config.install-json-server";
pub const INSTALL_DOCKER: &str = "config.install-docker-server";
pub const INSTALL_BASH: &str = "config.install-bash-server";
pub const INSTALL_PRETTIER: &str = "config.install-prettier";

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint).optional()
}

fn npm_fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// Its fix row installs the release of that name, when there's one for
/// this machine (`config.install.<name>`).
fn release_fix(program: Program, label: &str, name: &str, platform: &Platform) -> Program {
    if platform.installs().iter().any(|i| i.name == name) {
        program.fix(label, &format!("config.install.{name}"), Fix::Install(name.into()))
    } else {
        program
    }
}

/// Whether the pack serves `kind` in this project.
pub fn serves(project: &Project, settings: &Settings, kind: Kind) -> bool {
    settings.on(kind) && project.has(kind)
}

/// The checks for `project`, with what's on this machine.
pub fn programs(project: &Project, tools: &Tools, settings: &Settings, platform: &Platform) -> Vec<Program> {
    let mut out = Vec::new();
    let serves = |kind| serves(project, settings, kind);

    // npm, when a server it would install is missing.
    let wanted = [(Kind::Yaml, &tools.yaml), (Kind::Json, &tools.json), (Kind::Dockerfile, &tools.docker), (Kind::Shell, &tools.bash)];
    if tools.npm.is_none() && wanted.iter().any(|(k, found)| serves(*k) && found.is_none()) {
        out.push(program("npm", "npm (installs the servers)"));
    }

    if serves(Kind::Yaml) {
        let p = program(tools::YAML_SERVER, "YAML server").version_args(Vec::<String>::new());
        out.push(npm_fix(p, "Install yaml-language-server", INSTALL_YAML, tools.install_global(&[tools::YAML_PACKAGE])));
    }
    if serves(Kind::Json) {
        let p = program(tools::JSON_SERVER, "JSON server").version_args(Vec::<String>::new());
        out.push(npm_fix(p, "Install vscode-langservers-extracted", INSTALL_JSON, tools.install_global(&[tools::JSON_PACKAGE])));
    }
    if serves(Kind::Toml) {
        out.push(release_fix(program("taplo", "Taplo (TOML)"), "Install Taplo", "taplo", platform));
    }
    if serves(Kind::Dockerfile) {
        let p = program(tools::DOCKER_SERVER, "Dockerfile server").version_args(Vec::<String>::new());
        out.push(npm_fix(p, "Install dockerfile-language-server-nodejs", INSTALL_DOCKER, tools.install_global(&[tools::DOCKER_PACKAGE])));
        if settings.hadolint {
            out.push(release_fix(program("hadolint", "hadolint"), "Install hadolint", "hadolint", platform));
        }
    }
    if serves(Kind::Shell) {
        let p = program(tools::BASH_SERVER, "Bash server");
        out.push(npm_fix(p, "Install bash-language-server", INSTALL_BASH, tools.install_global(&[tools::BASH_PACKAGE])));
        out.push(release_fix(program("shellcheck", "ShellCheck"), "Install ShellCheck", "shellcheck", platform));
        out.push(release_fix(program("shfmt", "shfmt"), "Install shfmt", "shfmt", platform));
    }
    if serves(Kind::Markdown) {
        out.push(release_fix(program("marksman", "Marksman (Markdown)"), "Install Marksman", "marksman", platform));
    }
    if settings.prettier && serves(Kind::Json) {
        let p = program("prettier", "Prettier (formats JSON)");
        out.push(npm_fix(p, "Install Prettier", INSTALL_PRETTIER, tools.install_global(&["prettier"])));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::Health;
    use fenix_lang_kit::Output;

    fn rows(programs: &[Program]) -> Vec<(String, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn project(counts: [usize; 6]) -> Project {
        Project { counts, ..Project::default() }
    }

    #[test]
    fn only_the_projects_kinds_are_checked() {
        let tools = Tools { npm: Some("/n/npm".into()), ..Tools::default() };
        let linux = Platform::new("linux", "x86_64");
        let p = programs(&project([1, 0, 1, 0, 0, 1]), &tools, &Settings::default(), &linux);
        assert_eq!(rows(&p), [("YAML server".into(), Some(INSTALL_YAML.into())), ("Taplo (TOML)".into(), Some("config.install.taplo".into()))], "a README alone isn't Markdown");
        assert!(p.iter().all(|p| !p.required), "none is needed");
        assert!(p[0].version_args.is_empty(), "a server only found");
        assert_eq!(p[1].hints[0], "/tools/taplo");

        let all = programs(&project([1, 1, 1, 1, 1, 2]), &tools, &Settings::default(), &linux);
        assert_eq!(
            rows(&all).into_iter().map(|r| r.0).collect::<Vec<_>>(),
            ["YAML server", "JSON server", "Taplo (TOML)", "Dockerfile server", "hadolint", "Bash server", "ShellCheck", "shfmt", "Marksman (Markdown)", "Prettier (formats JSON)"]
        );
        match &all[1].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/n/npm install -g vscode-langservers-extracted"),
            other => panic!("{other:?}"),
        }
        assert_eq!(all[6].fix.as_ref().map(|f| f.2.clone()), Some(Fix::Install("shellcheck".into())));
    }

    #[test]
    fn settings_turn_kinds_off_and_npm_is_asked_for_when_its_needed() {
        let settings = Settings { json: false, hadolint: false, prettier: false, ..Settings::default() };
        let p = programs(&project([0, 3, 0, 1, 0, 0]), &Tools::default(), &settings, &Platform::new("windows", "aarch64"));
        assert_eq!(rows(&p), [("npm (installs the servers)".into(), None), ("Dockerfile server".into(), None)], "no npm, no release for this machine: no fixes");
        let c = p[0].evaluate(None, None);
        assert_eq!(c.health, Health::Warn);
        let c = p[1].evaluate(Some("/n/docker-langserver"), None);
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "/n/docker-langserver"), "found, not asked");
        let said = Output { code: Some(0), stdout: "version: 0.11.0\nlicense: GNU General Public License, version 3\n".into(), stderr: String::new() };
        let shellcheck = program("shellcheck", "ShellCheck").evaluate(Some("/t/shellcheck"), Some(&said));
        assert_eq!(shellcheck.detail, "0.11.0 (/t/shellcheck)");
    }
}
