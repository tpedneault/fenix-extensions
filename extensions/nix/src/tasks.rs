//! The tasks (`SPC p t`), each run in the project's folder. They depend on
//! what the project is:
//!
//! | Project | Tasks |
//! |---|---|
//! | a flake | `check` (`nix flake check`), `build`, `run`, `fmt` (when the flake has a `formatter` output), `flake show`, and last `flake update (rewrites flake.lock)` |
//! | `default.nix` or `shell.nix` | `build` (`nix-build`, with a `default.nix`), `shell (build only)` (`nix-shell --run true`: builds the environment and leaves) |
//! | a NixOS configuration, with `nixos-rebuild` installed | `nixos build`, `nixos dry-build` |
//! | a home-manager one, with `home-manager` installed | `home-manager build` |
//!
//! There's no `switch` -- of NixOS or home-manager -- and no `test`, `boot`,
//! `dry-activate`, garbage collection or anything else that changes the
//! running system or deletes from the store: `build` and `dry-build` write
//! nothing but `./result`. `nix flake update` rewrites `flake.lock`, so it's
//! last and its name says so: no key or default is named `update`. A task's
//! stdin is closed: a dev shell isn't a task (it's a terminal, `SPC m s`).

use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The update task's name, which says what it does.
pub const UPDATE: &str = "flake update (rewrites flake.lock)";

/// The tasks for `project`; none when it's nothing of Nix's.
pub fn tasks(project: &Project, tools: &Tools) -> Tasks {
    let mut t = Tasks::default();
    let nix = || tools.command("nix");
    if project.flake {
        t.add("check", nix().args(["flake", "check"]));
        t.add("build", nix().arg("build"));
        t.add("run", nix().arg("run"));
        if project.flake_formatter {
            t.add("fmt", nix().arg("fmt"));
        }
        t.add("flake show", nix().args(["flake", "show"]));
    } else {
        if project.default_nix {
            t.add("build", tools.command("nix-build"));
        }
        if project.shell_nix || project.default_nix {
            t.add("shell (build only)", tools.command("nix-shell").args(["--run", "true"]));
        }
    }
    if project.nixos && tools.nixos_rebuild.is_some() {
        let rebuild = |verb: &str| {
            let cmd = tools.command("nixos-rebuild").arg(verb);
            if project.flake {
                cmd.args(["--flake", "."])
            } else {
                cmd.args(["-I", "nixos-config=./configuration.nix"])
            }
        };
        t.add("nixos build", rebuild("build"));
        t.add("nixos dry-build", rebuild("dry-build"));
    }
    if project.home_manager && tools.home_manager.is_some() {
        let build = tools.command("home-manager").arg("build");
        t.add("home-manager build", if project.flake { build.args(["--flake", "."]) } else { build.args(["-f", "./home.nix"]) });
    }
    if project.flake {
        t.add(UPDATE, nix().args(["flake", "update"]));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), fenix_test_kit::line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn tools(names: &[&str]) -> Tools {
        let mut t = Tools::default();
        for n in names {
            *t.slot(n).unwrap() = Some(format!("/n/{n}"));
        }
        t
    }

    #[test]
    fn a_flake_has_check_build_run_fmt_show_and_last_update() {
        let project = Project { flake: true, flake_formatter: true, ..Project::default() };
        assert_eq!(
            table(&tasks(&project, &tools(&["nix"]))),
            rows(&[
                ("check", "/n/nix flake check"),
                ("build", "/n/nix build"),
                ("run", "/n/nix run"),
                ("fmt", "/n/nix fmt"),
                ("flake show", "/n/nix flake show"),
                (UPDATE, "/n/nix flake update"),
            ])
        );
        let no_fmt = Project { flake: true, ..Project::default() };
        assert!(!table(&tasks(&no_fmt, &Tools::default())).iter().any(|(n, _)| n == "fmt"), "no formatter output, no nix fmt");
        assert_eq!(table(&tasks(&no_fmt, &Tools::default()))[0].1, "nix flake check", "not found: by name");
    }

    #[test]
    fn update_is_last_says_what_it_does_and_nothing_switches() {
        let project = Project { flake: true, flake_formatter: true, nixos: true, home_manager: true, ..Project::default() };
        let t = tasks(&project, &tools(&["nix", "nixos-rebuild", "home-manager"]));
        let names: Vec<&str> = t.tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.last(), Some(&UPDATE), "last, so it's never what a first key lands on");
        assert!(!names.contains(&"update") && !names.contains(&"test"), "no name a key or a default would take");
        for task in &t.tasks {
            let args = task.cmd.args.join(" ");
            for banned in ["switch", "boot", "dry-activate", "test", "gc", "collect-garbage", "delete", "--upgrade", "install", "profile"] {
                assert!(!args.split_whitespace().any(|a| a == banned), "{}: {args}", task.name);
            }
            assert!(!task.name.contains("switch"));
        }
    }

    #[test]
    fn a_plain_project_builds_and_checks_its_shell() {
        let default = Project { default_nix: true, ..Project::default() };
        assert_eq!(table(&tasks(&default, &tools(&["nix-build", "nix-shell"]))), rows(&[("build", "/n/nix-build"), ("shell (build only)", "/n/nix-shell --run true")]));
        let shell = Project { shell_nix: true, ..Project::default() };
        assert_eq!(table(&tasks(&shell, &Tools::default())), rows(&[("shell (build only)", "nix-shell --run true")]), "no default.nix, nothing to build");
        assert!(tasks(&Project::default(), &tools(&["nix"])).tasks.is_empty(), "not a Nix project");
    }

    #[test]
    fn nixos_and_home_manager_need_their_tools() {
        let flake = Project { flake: true, nixos: true, home_manager: true, ..Project::default() };
        let without = table(&tasks(&flake, &tools(&["nix"])));
        assert!(!without.iter().any(|(n, _)| n.starts_with("nixos") || n.starts_with("home")), "{without:?}");
        let with = table(&tasks(&flake, &tools(&["nix", "nixos-rebuild", "home-manager"])));
        for want in [("nixos build", "/n/nixos-rebuild build --flake ."), ("nixos dry-build", "/n/nixos-rebuild dry-build --flake ."), ("home-manager build", "/n/home-manager build --flake .")] {
            assert!(with.contains(&(want.0.to_string(), want.1.to_string())), "{want:?}: {with:?}");
        }
        let at = |name: &str| with.iter().position(|(n, _)| n == name).unwrap();
        assert!(at("nixos build") > at("flake show") && at("home-manager build") < at(UPDATE));

        let classic = Project { nixos: true, configuration_nix: true, home_manager: true, home_nix: true, ..Project::default() };
        assert_eq!(
            table(&tasks(&classic, &tools(&["nixos-rebuild", "home-manager"]))),
            rows(&[
                ("nixos build", "/n/nixos-rebuild build -I nixos-config=./configuration.nix"),
                ("nixos dry-build", "/n/nixos-rebuild dry-build -I nixos-config=./configuration.nix"),
                ("home-manager build", "/n/home-manager build -f ./home.nix"),
            ]),
            "a configuration alone is not a Nix project, but it has its tasks"
        );
    }
}
