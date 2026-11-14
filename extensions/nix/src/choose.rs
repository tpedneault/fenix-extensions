//! What the pack's settings, the project's files and what's installed say
//! it should run: which language server, and how; which formatter. Pure
//! functions, so each rule is tested on its own.

use fenix_extension_api::{Command, Health};

use crate::project::{Formatter, Hint, Project};
use crate::tools::Tools;

/// `nix.server`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pref {
    /// nixd when it's there, else nil.
    Auto,
    Nixd,
    Nil,
}

/// `nix.server_runner`: where the language server comes from when it isn't
/// on `PATH` or in a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    /// Only the programs found.
    Path,
    /// Through the project's dev shell: `nix develop -c nixd`.
    DevShell,
    /// From nixpkgs: `nix shell nixpkgs#nixd -c nixd`, which downloads it
    /// the first time.
    Nixpkgs,
}

/// `nix.formatter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Auto,
    Tool(Formatter),
    Off,
}

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub server: Pref,
    pub runner: Runner,
    pub formatter: Pick,
    pub statix: bool,
    pub deadnix: bool,
    /// deadnix reports a function's unused arguments (`{ pkgs, lib, ... }:`
    /// with no `lib` used).
    pub deadnix_arguments: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { server: Pref::Auto, runner: Runner::Path, formatter: Pick::Auto, statix: true, deadnix: true, deadnix_arguments: true }
    }
}

fn flag(key: &str, default: bool) -> bool {
    match crate::calls::setting(key).map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "false" => false,
        Some(v) if v == "true" => true,
        _ => default,
    }
}

impl Settings {
    /// From Fenix: what a setting doesn't say is its default.
    pub fn read() -> Settings {
        let text = |key: &str| crate::calls::setting(key).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
        Settings {
            server: match text("nix.server").as_str() {
                "nixd" => Pref::Nixd,
                "nil" => Pref::Nil,
                _ => Pref::Auto,
            },
            runner: match text("nix.server_runner").as_str() {
                "dev-shell" => Runner::DevShell,
                "nixpkgs" => Runner::Nixpkgs,
                _ => Runner::Path,
            },
            formatter: match text("nix.formatter").as_str() {
                "off" => Pick::Off,
                name => Formatter::from_name(name).map_or(Pick::Auto, Pick::Tool),
            },
            statix: flag("nix.statix", true),
            deadnix: flag("nix.deadnix", true),
            deadnix_arguments: flag("nix.deadnix_arguments", true),
        }
    }
}

/// The language server to use.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    /// `nixd` or `nil`.
    pub name: &'static str,
    pub command: Command,
    /// How it's started, in words: where it was found, or the command Nix
    /// runs.
    pub how: String,
}

/// The servers `pref` allows, in the order they're tried.
pub fn candidates(pref: Pref) -> &'static [&'static str] {
    match pref {
        Pref::Auto => &["nixd", "nil"],
        Pref::Nixd => &["nixd"],
        Pref::Nil => &["nil"],
    }
}

/// The server to set: the first the setting allows that's found; else, when
/// the runner says so and Nix is there, the first allowed, run through
/// Nix. `None`: nothing to start (and the doctor says why).
pub fn server(settings: &Settings, project: &Project, tools: &Tools) -> Option<Server> {
    let allowed = candidates(settings.server);
    for name in allowed {
        if let Some(path) = tools.get(name) {
            return Some(Server { name, command: Command::new(path), how: path.to_string() });
        }
    }
    let name = allowed[0];
    let nix = tools.nix.as_deref()?;
    match settings.runner {
        Runner::Path => None,
        Runner::DevShell if project.flake => {
            let command = Command::new(nix).args(["develop", "--no-write-lock-file", "-c", name]);
            Some(Server { name, command, how: format!("nix develop -c {name}") })
        }
        Runner::DevShell if project.shell_nix => {
            let command = tools.command("nix-shell").args(["--run", name]);
            Some(Server { name, command, how: format!("nix-shell --run {name}") })
        }
        Runner::DevShell => None,
        Runner::Nixpkgs => {
            let command = Command::new(nix).args(["shell".to_string(), format!("nixpkgs#{name}"), "-c".to_string(), name.to_string()]);
            Some(Server { name, command, how: format!("nix shell nixpkgs#{name} -c {name}") })
        }
    }
}

/// The command that tries a server through Nix, to see that it's there:
/// run as a task, and when it works the runner is remembered.
pub fn try_server(runner: Runner, name: &str, project: &Project, tools: &Tools) -> Option<Command> {
    let nix = tools.nix.as_deref()?;
    match runner {
        Runner::DevShell if project.flake => Some(Command::new(nix).args(["develop", "--no-write-lock-file", "-c", name, "--version"])),
        Runner::DevShell if project.shell_nix => Some(tools.command("nix-shell").args(["--run".to_string(), format!("{name} --version")])),
        Runner::Nixpkgs => Some(Command::new(nix).args(["shell".to_string(), format!("nixpkgs#{name}"), "-c".to_string(), name.to_string(), "--version".to_string()])),
        _ => None,
    }
}

/// Which formatter to use, and what to tell about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The one to use: found or not (the doctor shows a missing one).
    pub want: Option<Formatter>,
    /// What the doctor says besides, when there's something to say.
    pub note: Option<(Health, String)>,
}

/// The rule: `nix.formatter` names one, or is `off`. On `auto`, the one the
/// flake's `formatter` output names -- the project's own choice -- even when
/// it isn't installed (a different one would reformat the project in
/// another style); with no such output, the first of nixfmt, alejandra and
/// nixpkgs-fmt that's installed. `nix fmt` itself isn't used on save: it
/// evaluates the flake each time, which takes longer than the three seconds
/// Fenix gives a formatter, and it doesn't read a buffer's text from stdin.
pub fn formatter(settings: &Settings, project: &Project, tools: &Tools) -> Decision {
    match settings.formatter {
        Pick::Off => Decision { want: None, note: None },
        Pick::Tool(f) => Decision { want: Some(f), note: None },
        Pick::Auto => match project.hint {
            Some(Hint::Tool(f)) => Decision { want: Some(f), note: None },
            Some(Hint::Treefmt) => {
                let note = (
                    Health::Info,
                    "the project formats with treefmt (nix fmt, the fmt task): saving doesn't run it, since a direct call would use another style; set nix.formatter to format with one tool".to_string(),
                );
                Decision { want: None, note: Some(note) }
            }
            None => match Formatter::ALL.into_iter().find(|f| tools.get(f.name()).is_some()) {
                Some(f) => Decision { want: Some(f), note: None },
                None => Decision {
                    want: None,
                    note: Some((Health::Warn, "none of nixfmt, alejandra or nixpkgs-fmt is on PATH or in your profile: add one to the dev shell or install it (nix profile install nixpkgs#nixfmt-rfc-style)".into())),
                },
            },
        },
    }
}

/// The formatter's command, when it's found.
pub fn formatter_command(want: Option<Formatter>, tools: &Tools) -> Option<Command> {
    let f = want?;
    let path = tools.get(f.name())?;
    Some(Command::new(path).args(f.args().iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn flake() -> Project {
        Project { flake: true, ..Project::default() }
    }

    fn found(names: &[&str]) -> Tools {
        let mut t = Tools::default();
        for n in names {
            *t.slot(n).unwrap() = Some(format!("/n/{n}"));
        }
        t
    }

    fn line(c: &Command) -> String {
        std::iter::once(c.program.as_str()).chain(c.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn the_server_found_is_used_nixd_first() {
        let s = Settings::default();
        let got = server(&s, &flake(), &found(&["nixd", "nil", "nix"])).unwrap();
        assert_eq!((got.name, line(&got.command)), ("nixd", "/n/nixd".to_string()));
        let got = server(&s, &flake(), &found(&["nil"])).unwrap();
        assert_eq!((got.name, got.how.as_str()), ("nil", "/n/nil"));
        let only_nil = Settings { server: Pref::Nil, ..Settings::default() };
        assert_eq!(server(&only_nil, &flake(), &found(&["nixd", "nil"])).unwrap().name, "nil");
        let only_nixd = Settings { server: Pref::Nixd, ..Settings::default() };
        assert!(server(&only_nixd, &flake(), &found(&["nil"])).is_none(), "nil isn't nixd");
    }

    #[test]
    fn nothing_is_run_through_nix_unless_the_setting_says_so() {
        let tools = found(&["nix"]);
        assert!(server(&Settings::default(), &flake(), &tools).is_none(), "a project with a flake and no server: nothing starts silently");
        let dev = Settings { runner: Runner::DevShell, ..Settings::default() };
        let got = server(&dev, &flake(), &tools).unwrap();
        assert_eq!(line(&got.command), "/n/nix develop --no-write-lock-file -c nixd");
        assert_eq!(got.how, "nix develop -c nixd");
        let shell = Project { shell_nix: true, ..Project::default() };
        assert_eq!(line(&server(&dev, &shell, &found(&["nix", "nix-shell"])).unwrap().command), "/n/nix-shell --run nixd");
        assert!(server(&dev, &Project::default(), &tools).is_none(), "no dev shell here");
        assert!(server(&dev, &flake(), &Tools::default()).is_none(), "no Nix");
        let pkgs = Settings { runner: Runner::Nixpkgs, server: Pref::Nil, ..Settings::default() };
        assert_eq!(line(&server(&pkgs, &Project::default(), &tools).unwrap().command), "/n/nix shell nixpkgs#nil -c nil");
        // A server on PATH wins over any runner.
        assert_eq!(line(&server(&dev, &flake(), &found(&["nix", "nixd"])).unwrap().command), "/n/nixd");
    }

    #[test]
    fn trying_a_server_through_nix_asks_its_version() {
        let tools = found(&["nix"]);
        assert_eq!(line(&try_server(Runner::DevShell, "nixd", &flake(), &tools).unwrap()), "/n/nix develop --no-write-lock-file -c nixd --version");
        assert_eq!(line(&try_server(Runner::Nixpkgs, "nil", &Project::default(), &tools).unwrap()), "/n/nix shell nixpkgs#nil -c nil --version");
        assert!(try_server(Runner::Path, "nixd", &flake(), &tools).is_none());
        assert!(try_server(Runner::DevShell, "nixd", &Project::default(), &tools).is_none());
        assert!(try_server(Runner::Nixpkgs, "nixd", &flake(), &Tools::default()).is_none());
    }

    #[test]
    fn the_flakes_formatter_is_the_projects_choice_installed_or_not() {
        let named = Project { flake: true, flake_formatter: true, hint: Some(Hint::Tool(Formatter::Alejandra)), ..Project::default() };
        let d = formatter(&Settings::default(), &named, &found(&["nixfmt"]));
        assert_eq!(d.want, Some(Formatter::Alejandra), "not nixfmt, though it's the one installed");
        assert_eq!(formatter_command(d.want, &found(&["nixfmt"])), None, "not installed: nothing is set");
        assert_eq!(line(&formatter_command(d.want, &found(&["alejandra"])).unwrap()), "/n/alejandra --quiet -");
    }

    #[test]
    fn without_a_hint_the_first_installed_is_used_in_the_official_ones_order() {
        let s = Settings::default();
        let p = flake();
        assert_eq!(formatter(&s, &p, &found(&["nixpkgs-fmt", "alejandra"])).want, Some(Formatter::Alejandra));
        assert_eq!(formatter(&s, &p, &found(&["nixfmt", "alejandra", "nixpkgs-fmt"])).want, Some(Formatter::Nixfmt));
        let none = formatter(&s, &p, &Tools::default());
        assert_eq!(none.want, None);
        assert_eq!(none.note.as_ref().map(|n| n.0), Some(Health::Warn));
        assert!(none.note.unwrap().1.contains("nixfmt, alejandra or nixpkgs-fmt"));
    }

    #[test]
    fn a_setting_picks_explicitly_and_off_means_off() {
        let named = Project { hint: Some(Hint::Tool(Formatter::Alejandra)), ..Project::default() };
        let pick = Settings { formatter: Pick::Tool(Formatter::NixpkgsFmt), ..Settings::default() };
        assert_eq!(formatter(&pick, &named, &Tools::default()).want, Some(Formatter::NixpkgsFmt));
        let off = Settings { formatter: Pick::Off, ..Settings::default() };
        assert_eq!(formatter(&off, &named, &found(&["alejandra"])), Decision { want: None, note: None });
    }

    #[test]
    fn treefmt_is_run_by_nix_fmt_not_on_save() {
        let p = Project { hint: Some(Hint::Treefmt), ..Project::default() };
        let d = formatter(&Settings::default(), &p, &found(&["nixfmt"]));
        assert_eq!(d.want, None);
        let (health, why) = d.note.unwrap();
        assert_eq!(health, Health::Info);
        assert!(why.contains("treefmt") && why.contains("nix.formatter"), "{why}");
        // Picking one by hand formats with it.
        let pick = Settings { formatter: Pick::Tool(Formatter::Nixfmt), ..Settings::default() };
        assert_eq!(formatter(&pick, &p, &Tools::default()).want, Some(Formatter::Nixfmt));
    }
}
