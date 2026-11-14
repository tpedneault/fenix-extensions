//! The project doctor's Elixir checks (`SPC p h`): Elixir, held to the
//! version `mix.exs` asks for; Hex, which `mix deps.get` needs; and
//! Expert, the language server, which Fenix works without. Hex and
//! Expert each have a fix row.

use fenix_extension_api::Health;
use fenix_lang_kit::{find_version, Fix, Output, Program};

use crate::project::Project;
use crate::tools::{hints, Platform, Tools, EXPERT};

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// What `mix hex.info` said: Hex's version, or how to get it.
pub fn hex(out: &Output) -> (Health, String) {
    let text = out.text();
    let line = text.lines().find_map(|l| l.trim().strip_prefix("Hex:"));
    match (out.ok(), line.and_then(find_version)) {
        (true, Some(version)) => (Health::Ok, version.to_string()),
        _ => (Health::Warn, "not installed: mix deps.get needs it, and the fix installs it".into()),
    }
}

/// Expert's own check: the version it prints.
pub fn expert(out: &Output) -> (Health, String) {
    match find_version(&out.text()) {
        Some(version) => (Health::Ok, version.to_string()),
        None => (Health::Warn, format!("its version couldn't be read: {}", out.why())),
    }
}

/// The checks for `project` on `platform`, Hex's fix run with the Mix
/// `tools` found.
pub fn programs(project: &Project, platform: &Platform, tools: &Tools) -> Vec<Program> {
    // `elixir --short-version` is just the number; `--version` starts with
    // Erlang's.
    let elixir = program("elixir", "Elixir").version_args(["--short-version"]);
    let elixir = match &project.elixir {
        Some(min) => elixir.at_least(min),
        None => elixir,
    };
    let hex = program("mix", "Hex").version_args(["hex.info"]).optional().judge(hex).fix("Install Hex", "elixir.install-hex", Fix::Task(tools.mix().args(["local.hex", "--force"])));
    // Expert starts by unpacking itself the first time: it's slow once.
    let expert = program("expert", "Expert").version_args(["--version"]).optional().timeout(30_000).judge(expert);
    let expert = if platform.expert().is_some() { expert.fix(&format!("Install Expert {EXPERT}"), "elixir.install.expert", Fix::Install("expert".into())) } else { expert };
    vec![elixir, hex, expert]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn elixir_is_held_to_mix_exs() {
        let project = Project { mix: true, elixir: Some("1.15".into()), ..Project::default() };
        let p = programs(&project, &Platform::new("linux", "x86_64"), &Tools::default());
        assert_eq!(rows(&p), [("Elixir".into(), true, None), ("Hex".into(), false, Some("elixir.install-hex".into())), ("Expert".into(), false, Some("elixir.install.expert".into()))]);
        assert_eq!(p[0].version_args, ["--short-version"]);
        assert_eq!(p[0].hints[0], "/tools/elixir");
        let c = p[0].evaluate(Some("/t/elixir"), Some(&out(0, "1.14.5\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "1.14.5, but the project needs >=1.15 (/t/elixir)"));
        assert_eq!(p[0].evaluate(Some("/t/elixir"), Some(&out(0, "1.20.5\n"))).health, Health::Ok);
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad, "no Elixir, nothing builds");
        assert!(programs(&Project::default(), &Platform::default(), &Tools::default())[0].wants.is_none(), "no requirement: any Elixir");
        assert_eq!(rows(&programs(&Project::default(), &Platform::new("linux", "riscv64"), &Tools::default()))[2], ("Expert".into(), false, None), "no release for it");
    }

    #[test]
    fn hex_and_expert_say_what_they_found() {
        let p = programs(&Project::default(), &Platform::new("linux", "x86_64"), &Tools::default());
        let c = p[1].evaluate(Some("/t/mix"), Some(&out(0, include_str!("samples/mix-hex-info.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "2.2.1"));
        let c = p[1].evaluate(Some("/t/mix"), Some(&Output { code: Some(1), stdout: String::new(), stderr: "** (Mix) The task \"hex.info\" could not be found\n".into() }));
        assert_eq!(c.health, Health::Warn);
        assert_eq!(c.fix.map(|f| f.1), Some("elixir.install-hex".into()));
        assert_eq!(p[1].evaluate(None, None).health, Health::Warn, "Fenix works without Hex until deps.get");

        let c = p[2].evaluate(Some("/t/expert"), Some(&out(0, "0.2.3\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "0.2.3"));
        assert_eq!(p[2].millis(), 30_000);
        assert_eq!(p[2].evaluate(Some("/t/expert"), Some(&out(0, ""))).health, Health::Warn, "no version to read");
        assert_eq!(p[2].evaluate(None, None).health, Health::Warn);
    }
}
