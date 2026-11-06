//! The programs the pack runs, found on this machine: opam, OCaml, dune,
//! ocaml-lsp-server (`ocamllsp`), ocamlformat and earlybird. They live in
//! an opam switch -- the project's own (`_opam`), or the one opam has
//! chosen -- which an editor started from the desktop rarely has on its
//! `PATH`. One found directly runs as it is; one that isn't, but opam
//! is, runs through `opam exec --`, in the switch opam picks for the
//! project, once the doctor has seen it answer there.

use std::collections::BTreeSet;

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

/// Where else to look: the project's own switch, opam's default one,
/// Homebrew's and Nix's, and opam's on Windows.
pub const HINTS: &[&str] = &[
    "/project/_opam/bin", "$HOME/.opam/default/bin", "$HOME/.local/bin", "/opt/homebrew/bin", "/usr/local/bin", "$HOME/.nix-profile/bin",
    "/nix/var/nix/profiles/default/bin", "%LOCALAPPDATA%\\opam\\default\\bin",
];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// Where opam comes from: its page says how, for each OS.
pub const OPAM_PAGE: &str = "https://opam.ocaml.org/doc/Install.html";

/// A program the pack runs, its opam package, and what the doctor calls
/// it.
pub struct Tool {
    pub program: &'static str,
    pub package: &'static str,
    pub label: &'static str,
}

pub const OCAML: Tool = Tool { program: "ocaml", package: "ocaml", label: "OCaml" };
pub const DUNE: Tool = Tool { program: "dune", package: "dune", label: "dune" };
pub const OCAMLLSP: Tool = Tool { program: "ocamllsp", package: "ocaml-lsp-server", label: "ocaml-lsp-server" };
pub const OCAMLFORMAT: Tool = Tool { program: "ocamlformat", package: "ocamlformat", label: "ocamlformat" };
pub const EARLYBIRD: Tool = Tool { program: "ocamlearlybird", package: "earlybird", label: "earlybird" };

/// Each, as the doctor checks them.
pub const TOOLS: &[&Tool] = &[&OCAML, &DUNE, &OCAMLLSP, &OCAMLFORMAT, &EARLYBIRD];

/// The tool the doctor calls `label`.
pub fn by_label(label: &str) -> Option<&'static Tool> {
    TOOLS.iter().copied().find(|t| t.label == label)
}

/// What was found, by full path; and which answered through opam.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub opam: Option<String>,
    pub ocaml: Option<String>,
    pub dune: Option<String>,
    pub ocamllsp: Option<String>,
    pub ocamlformat: Option<String>,
    pub ocamlearlybird: Option<String>,
    /// The programs `opam exec --` ran, as the doctor last saw.
    pub in_switch: BTreeSet<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut t = Tools::default();
        for name in ["opam", "ocaml", "dune", "ocamllsp", "ocamlformat", "ocamlearlybird"] {
            let found = host.find_program(name, &hints);
            if let Some(slot) = t.slot(name) {
                *slot = found;
            }
        }
        t
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "opam" => &mut self.opam,
            "ocaml" => &mut self.ocaml,
            "dune" => &mut self.dune,
            "ocamllsp" => &mut self.ocamllsp,
            "ocamlformat" => &mut self.ocamlformat,
            "ocamlearlybird" => &mut self.ocamlearlybird,
            _ => return None,
        })
    }

    /// Where `program` was found.
    pub fn found(&self, program: &str) -> Option<&str> {
        match program {
            "opam" => self.opam.as_deref(),
            "ocaml" => self.ocaml.as_deref(),
            "dune" => self.dune.as_deref(),
            "ocamllsp" => self.ocamllsp.as_deref(),
            "ocamlformat" => self.ocamlformat.as_deref(),
            "ocamlearlybird" => self.ocamlearlybird.as_deref(),
            _ => None,
        }
    }

    /// `program` through opam: `opam exec -- <program>`.
    pub fn via_opam(&self, program: &str) -> Option<Command> {
        self.opam.as_deref().map(|opam| Command::new(opam).args(["exec", "--", program]))
    }

    /// `program`, when it's known to be there: by its path, else through
    /// opam once it answered there.
    pub fn command(&self, program: &str) -> Option<Command> {
        match self.found(program) {
            Some(path) => Some(Command::new(path)),
            None if self.in_switch.contains(program) => self.via_opam(program),
            None => None,
        }
    }

    /// `program` as a task runs it: as [`Tools::command`], else through
    /// opam if there's opam, else by its name -- a task says what's wrong
    /// itself.
    pub fn run(&self, program: &str) -> Command {
        self.command(program).or_else(|| self.via_opam(program)).unwrap_or_else(|| Command::new(program))
    }

    /// `opam install <packages> --yes`: what a fix row runs.
    pub fn opam_install(&self, packages: &[&str]) -> Option<Command> {
        self.opam.as_deref().map(|opam| Command::new(opam).arg("install").args(packages.iter().copied()).arg("--yes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn a_tool_runs_by_its_path_else_through_opam() {
        let mut t = Tools { opam: Some("/o/opam".into()), dune: Some("/s/dune".into()), ..Tools::default() };
        assert_eq!(t.command("dune").map(|c| line(&c)).as_deref(), Some("/s/dune"));
        assert!(t.command("ocamllsp").is_none(), "not yet seen in the switch");
        assert_eq!(line(&t.run("ocamllsp")), "/o/opam exec -- ocamllsp", "a task tries");
        t.in_switch.insert("ocamllsp".into());
        assert_eq!(t.command("ocamllsp").map(|c| line(&c)).as_deref(), Some("/o/opam exec -- ocamllsp"));
        assert_eq!(line(&Tools::default().run("dune")), "dune");
        assert_eq!(t.opam_install(&["ocamlformat.0.27.0"]).map(|c| line(&c)).as_deref(), Some("/o/opam install ocamlformat.0.27.0 --yes"));
        assert!(Tools::default().opam_install(&["dune"]).is_none());
        assert!(t.slot("go").is_none() && t.found("go").is_none());
        assert_eq!(by_label("ocaml-lsp-server").map(|t| t.program), Some("ocamllsp"));
        assert!(hints().contains(&"/project/_opam/bin".to_string()), "the project's own switch first");
    }
}
