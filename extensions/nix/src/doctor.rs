//! The project doctor's Nix checks (`SPC p h`): Nix itself and whether
//! flakes are on, the language server, the formatter, the linters, and the
//! rebuild tools of a NixOS or home-manager configuration.
//!
//! Nothing here installs anything: Nix's tools come from Nix (a profile, a
//! dev shell, nixpkgs), and a release the pack could download and check
//! doesn't exist for them. What a fix row does is a command of the pack's
//! own, each one visible: how to install Nix is text beside the editor; the
//! two for the language server are tasks, in the task pane, that run the
//! server's `--version` through the dev shell or `nix shell nixpkgs#...` --
//! which build or download it, so they're only ever started by you -- and,
//! when one works, remember the runner in `nix.server_runner`.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{Fix, Output, Program};

use crate::choose::{self, Decision, Settings};
use crate::project::Project;
use crate::tools::{hints, Tools};

/// The command of the fix row that shows how to install Nix.
pub const INSTALL_HELP: &str = "nix.install-help";
/// The two that try the language server through Nix, as tasks.
pub const SERVER_DEV_SHELL: &str = "nix.server.dev-shell";
pub const SERVER_NIXPKGS: &str = "nix.server.nixpkgs";

/// The oldest Nix with flakes.
pub const FLAKES_FROM: &str = "2.4";

/// `name`, looked for where Nix's tools are put. Nothing is required: a
/// pack that works without them (highlighting, snippets) says what it
/// couldn't do instead of failing a project.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint).optional()
}

/// What to say, beside the editor, to someone without Nix.
pub fn install_help() -> String {
    "Installing Nix\n\
     \n\
     Nix isn't installed (or isn't on PATH). Fenix's Nix pack still highlights,\n\
     folds, indents and completes snippets for .nix files, and uses a\n\
     formatter, linters and a language server when they're installed. What needs\n\
     Nix itself is the tasks, the flake output pickers and running the language\n\
     server through a dev shell.\n\
     \n\
     To install Nix, see https://nixos.org/download/ -- the official installer\n\
     (multi-user, with the nix-daemon), or Determinate Systems' installer, which\n\
     turns flakes on -- or your package manager's. On NixOS and with Home\n\
     Manager it's already there. Flakes are an experimental feature: for the\n\
     official installer, add\n\
     \n\
     experimental-features = nix-command flakes\n\
     \n\
     to ~/.config/nix/nix.conf. Then SPC m d checks the project again.\n\
     \n\
     The tools the pack uses -- nixd or nil, nixfmt, alejandra or nixpkgs-fmt,\n\
     statix, deadnix -- come from Nix too: nix profile install nixpkgs#nixd,\n\
     or put them in the project's dev shell. The pack never installs them.\n"
        .to_string()
}

/// What a server said to `--version`: its first line, whatever it is --
/// nixd says `nixd 2.6.4`, nil dates its releases (`nil 2025-06-13`).
fn said_version(out: &Output) -> (Health, String) {
    let first = out.text().lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
    match (out.ok(), first) {
        (true, Some(line)) => (Health::Ok, line),
        (true, None) => (Health::Ok, "found, and it printed no version".into()),
        (false, _) => (Health::Warn, format!("asking its version failed: {}", out.why())),
    }
}

/// `nix config show experimental-features`, as the flakes check reads it.
fn flakes_judge(project_is_flake: bool) -> impl Fn(&Output) -> (Health, String) {
    move |out| {
        if !out.ok() {
            return (Health::Info, format!("couldn't tell (`nix config show` is Nix 2.20 and later): {}", out.why()));
        }
        let features = out.stdout.trim();
        if features.split_whitespace().any(|f| f == "flakes") {
            return (Health::Ok, "enabled".into());
        }
        let how = "add `experimental-features = nix-command flakes` to ~/.config/nix/nix.conf";
        if project_is_flake {
            (Health::Warn, format!("not enabled, and this project is a flake: {how}"))
        } else {
            (Health::Info, format!("not enabled (this project isn't a flake): {how}"))
        }
    }
}

/// The checks that are programs, for the project, the tools found and the
/// settings; and the pack's own other rows (what runs through Nix, what
/// isn't there and why).
pub fn checks(project: &Project, tools: &Tools, settings: &Settings, decision: &Decision) -> (Vec<Program>, Vec<Check>) {
    let mut programs = Vec::new();
    let mut extra = Vec::new();

    // Nix.
    let mut nix = program("nix", "Nix").fix("How to install Nix", INSTALL_HELP, Fix::Own);
    if project.flake {
        nix = nix.at_least(FLAKES_FROM);
    }
    programs.push(nix);
    if tools.nix.is_some() {
        programs.push(program("nix", "Flakes").version_args(["config", "show", "experimental-features"]).judge(flakes_judge(project.flake)));
    } else {
        extra.push(Check::new(
            Section::Toolchain,
            "Without Nix",
            Health::Info,
            "highlighting, folds, snippets, and the formatter, linters and language server you have still work; the tasks, the flake pickers and a dev shell need Nix",
        ));
    }

    // The language server.
    let allowed = choose::candidates(settings.server);
    let server = choose::server(settings, project, tools);
    let name = server.as_ref().map_or(allowed[0], |s| s.name);
    let label = format!("Language server ({name})");
    match &server {
        Some(s) if tools.get(s.name).is_some() => programs.push(program(s.name, &label).judge(said_version)),
        Some(s) => extra.push(Check::new(Section::Toolchain, &label, Health::Ok, format!("not on PATH: started through Nix, `{}` (nix.server_runner)", s.how))),
        None => {
            let mut p = program(name, &label);
            if tools.nix.is_some() {
                // The two ways to get it through Nix, each a task you start.
                if project.has_shell() {
                    p = p.fix("Try it in the project's dev shell", SERVER_DEV_SHELL, Fix::Own);
                    extra.push(
                        Check::new(Section::Toolchain, "Server from nixpkgs", Health::Info, format!("or `nix shell nixpkgs#{name}`, which downloads it the first time"))
                            .fix("Fetch it with nix shell", SERVER_NIXPKGS),
                    );
                } else {
                    p = p.fix("Fetch it with nix shell", SERVER_NIXPKGS, Fix::Own);
                }
            }
            programs.push(p);
        }
    }

    // The formatter.
    if let Some(f) = decision.want {
        programs.push(program(f.name(), &format!("Formatter ({})", f.name())));
    }
    if let Some((health, why)) = &decision.note {
        extra.push(Check::new(Section::Toolchain, "Formatter", *health, why));
    }

    // The linters.
    if settings.statix {
        programs.push(program("statix", "statix"));
    }
    if settings.deadnix {
        programs.push(program("deadnix", "deadnix"));
    }

    // A configuration's rebuild tools.
    if project.nixos {
        programs.push(program("nixos-rebuild", "nixos-rebuild"));
    }
    if project.home_manager {
        programs.push(program("home-manager", "home-manager"));
    }
    (programs, extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Formatter, Hint};

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn tools(names: &[&str]) -> Tools {
        let mut t = Tools::default();
        for n in names {
            *t.slot(n).unwrap() = Some(format!("/n/{n}"));
        }
        t
    }

    fn rows(project: &Project, tools: &Tools, settings: &Settings) -> (Vec<Program>, Vec<Check>) {
        let decision = choose::formatter(settings, project, tools);
        checks(project, tools, settings, &decision)
    }

    fn flake() -> Project {
        Project { flake: true, ..Project::default() }
    }

    #[test]
    fn nix_is_asked_its_version_and_flakes_need_2_4() {
        let (p, _) = rows(&flake(), &tools(&["nix"]), &Settings::default());
        assert_eq!((p[0].name.as_str(), p[0].label.as_str(), p[0].required), ("nix", "Nix", false), "the pack works without it");
        assert_eq!(p[0].version_args, ["--version"]);
        assert!(p[0].hints.contains(&"$HOME/.nix-profile/bin".to_string()));
        assert_eq!(p[0].fix.as_ref().map(|f| (f.0.as_str(), f.1.as_str())), Some(("How to install Nix", INSTALL_HELP)));
        let c = p[0].evaluate(Some("/n/nix"), Some(&said("nix (Nix) 2.24.9\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "2.24.9 (/n/nix)"));
        let old = p[0].evaluate(Some("/n/nix"), Some(&said("nix (Nix) 2.3.18\n")));
        assert_eq!(old.health, Health::Warn, "{}", old.detail);
        let det = p[0].evaluate(Some("/n/nix"), Some(&said("nix (Determinate Nix 3.1.0) 2.28.3\n")));
        assert_eq!(det.health, Health::Ok);
        // An old Nix is fine for a project that isn't a flake.
        let (plain, _) = rows(&Project { default_nix: true, ..Project::default() }, &tools(&["nix"]), &Settings::default());
        assert_eq!(plain[0].evaluate(Some("/n/nix"), Some(&said("nix (Nix) 2.3.18\n"))).health, Health::Ok);
    }

    #[test]
    fn without_nix_it_says_how_to_install_it_and_what_still_works() {
        let (p, extra) = rows(&flake(), &Tools::default(), &Settings::default());
        let missing = p[0].evaluate(None, None);
        assert_eq!((missing.health, missing.fix.map(|f| f.1)), (Health::Warn, Some(INSTALL_HELP.into())));
        assert!(!p.iter().any(|p| p.label == "Flakes"), "nothing to ask");
        let note = extra.iter().find(|c| c.label == "Without Nix").unwrap();
        assert!(note.detail.contains("highlighting") && note.detail.contains("still work"), "{}", note.detail);
        let text = install_help();
        assert!(text.contains("https://nixos.org/download/") && text.contains("experimental-features = nix-command flakes") && text.contains("never installs"), "{text}");
        // With it there's no such note.
        assert!(!rows(&flake(), &tools(&["nix"]), &Settings::default()).1.iter().any(|c| c.label == "Without Nix"));
    }

    #[test]
    fn flakes_are_checked_from_nixs_own_setting() {
        let (p, _) = rows(&flake(), &tools(&["nix"]), &Settings::default());
        let flakes = p.iter().find(|p| p.label == "Flakes").unwrap();
        assert_eq!(flakes.version_args, ["config", "show", "experimental-features"]);
        let on = flakes.evaluate(Some("/n/nix"), Some(&said("nix-command flakes\n")));
        assert_eq!((on.health, on.detail.as_str()), (Health::Ok, "enabled"));
        let off = flakes.evaluate(Some("/n/nix"), Some(&said("nix-command\n")));
        assert_eq!(off.health, Health::Warn, "this project is a flake");
        assert!(off.detail.contains("nix.conf"), "{}", off.detail);
        let (p, _) = rows(&Project { default_nix: true, ..Project::default() }, &tools(&["nix"]), &Settings::default());
        let off = p.iter().find(|p| p.label == "Flakes").unwrap().evaluate(Some("/n/nix"), Some(&said("")));
        assert_eq!(off.health, Health::Info, "not needed here");
        let old = Output { code: Some(1), stdout: String::new(), stderr: "error: 'config' is not a recognised command\n".into() };
        assert_eq!(flakes.evaluate(Some("/n/nix"), Some(&old)).health, Health::Info);
    }

    #[test]
    fn a_found_server_is_a_row_with_its_version() {
        let (p, extra) = rows(&flake(), &tools(&["nix", "nixd"]), &Settings::default());
        let row = p.iter().find(|p| p.label == "Language server (nixd)").unwrap();
        assert!(row.fix.is_none() && !row.required);
        assert_eq!(row.evaluate(Some("/n/nixd"), Some(&said("nixd 2.6.4\n"))).detail, "nixd 2.6.4");
        assert!(!extra.iter().any(|c| c.label.starts_with("Server")));
        let (p, _) = rows(&flake(), &tools(&["nil"]), &Settings::default());
        let nil = p.iter().find(|p| p.label == "Language server (nil)").expect("nil, when it's the one there is");
        let c = nil.evaluate(Some("/n/nil"), Some(&said("nil 2025-06-13\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "nil 2025-06-13"), "a date is its version");
        let broken = Output { code: Some(1), stdout: String::new(), stderr: "error while loading shared libraries: libnix.so\n".into() };
        assert_eq!(nil.evaluate(Some("/n/nil"), Some(&broken)).health, Health::Warn);
    }

    #[test]
    fn a_missing_server_offers_the_dev_shell_and_nixpkgs_each_as_a_task_you_start() {
        let (p, extra) = rows(&flake(), &tools(&["nix"]), &Settings::default());
        let row = p.iter().find(|p| p.label == "Language server (nixd)").unwrap();
        let missing = row.evaluate(None, None);
        assert_eq!(missing.health, Health::Warn);
        assert_eq!(missing.fix, Some(("Try it in the project's dev shell".into(), SERVER_DEV_SHELL.into())));
        let other = extra.iter().find(|c| c.label == "Server from nixpkgs").unwrap();
        assert_eq!(other.fix, Some(("Fetch it with nix shell".into(), SERVER_NIXPKGS.into())));
        assert!(other.detail.contains("downloads"), "{}", other.detail);

        // No dev shell: only nixpkgs.
        let (p, extra) = rows(&Project { default_nix: true, ..Project::default() }, &tools(&["nix"]), &Settings::default());
        let row = p.iter().find(|p| p.label == "Language server (nixd)").unwrap();
        assert_eq!(row.evaluate(None, None).fix.map(|f| f.1), Some(SERVER_NIXPKGS.into()));
        assert!(!extra.iter().any(|c| c.label == "Server from nixpkgs"));

        // No Nix: no fix rows for it.
        let (p, _) = rows(&flake(), &Tools::default(), &Settings::default());
        let row = p.iter().find(|p| p.label == "Language server (nixd)").unwrap();
        assert!(row.evaluate(None, None).fix.is_none());
    }

    #[test]
    fn a_server_run_through_nix_is_said_so() {
        let dev = Settings { runner: choose::Runner::DevShell, ..Settings::default() };
        let (p, extra) = rows(&flake(), &tools(&["nix"]), &dev);
        assert!(!p.iter().any(|p| p.label.starts_with("Language server")), "no program to ask");
        let c = extra.iter().find(|c| c.label == "Language server (nixd)").unwrap();
        assert_eq!(c.health, Health::Ok);
        assert!(c.detail.contains("nix develop -c nixd"), "{}", c.detail);
    }

    #[test]
    fn the_formatter_row_is_the_one_in_use_and_a_note_says_why_not() {
        let named = Project { flake: true, flake_formatter: true, hint: Some(Hint::Tool(Formatter::Alejandra)), ..Project::default() };
        let (p, extra) = rows(&named, &tools(&["nix", "nixfmt"]), &Settings::default());
        assert!(p.iter().any(|p| p.label == "Formatter (alejandra)"), "the project's, though it isn't installed");
        assert!(!p.iter().any(|p| p.label == "Formatter (nixfmt)"));
        assert!(!extra.iter().any(|c| c.label == "Formatter"));
        let (_, extra) = rows(&flake(), &tools(&["nix"]), &Settings::default());
        assert_eq!(extra.iter().find(|c| c.label == "Formatter").map(|c| c.health), Some(Health::Warn));
        let tree = Project { hint: Some(Hint::Treefmt), ..flake() };
        let (p, extra) = rows(&tree, &tools(&["nix", "nixfmt"]), &Settings::default());
        assert!(!p.iter().any(|p| p.label.starts_with("Formatter")));
        assert_eq!(extra.iter().find(|c| c.label == "Formatter").map(|c| c.health), Some(Health::Info));
    }

    #[test]
    fn the_linters_follow_their_settings_and_a_configurations_tools_its_kind() {
        let (p, _) = rows(&flake(), &tools(&["nix"]), &Settings::default());
        let labels: Vec<&str> = p.iter().map(|p| p.label.as_str()).collect();
        assert!(labels.contains(&"statix") && labels.contains(&"deadnix") && !labels.contains(&"nixos-rebuild"), "{labels:?}");
        let off = Settings { statix: false, deadnix: false, ..Settings::default() };
        let (p, _) = rows(&flake(), &tools(&["nix"]), &off);
        assert!(!p.iter().any(|p| p.label == "statix" || p.label == "deadnix"));
        let host = Project { flake: true, nixos: true, home_manager: true, ..Project::default() };
        let (p, _) = rows(&host, &tools(&["nix"]), &Settings::default());
        assert!(p.iter().any(|p| p.label == "nixos-rebuild") && p.iter().any(|p| p.label == "home-manager"));
        assert!(p.iter().all(|p| !p.required), "nothing Nix's is required");
        let c = p.iter().find(|p| p.label == "statix").unwrap().evaluate(Some("/n/statix"), Some(&said("statix 0.5.8\n")));
        assert_eq!(c.detail, "0.5.8 (/n/statix)");
    }
}
