//! The tasks Kiln adds to Fenix's task picker (`SPC t t`, `project::tasks`):
//! `kiln: check`, `kiln: export --check` and `kiln: export`, each running
//! the kiln this extension runs, in the project's folder.
//!
//! The tasks run in the task pane, which feeds lines that look like
//! `path:line:col: message` to the quickfix list. `kiln check`'s text
//! format is exactly that (`design/tasks/TASK-006-….md:11:23: K102 error:
//! reference REQ-099 doesn't exist`: Fenix's generic reading, in
//! `fenix-tasks`' `output::parse_line`, takes the first `:line:col:` and
//! the rest as the message), so the task uses the default text format;
//! `--format github` prints `::error file=…` workflow commands, which no
//! reading of Fenix's matches, and `--format gitlab` is one JSON document.
//!
//! `kiln: export` writes the file targets (Markdown, HTML, Word). It never
//! publishes to Confluence unasked: kiln publishes only with `--yes` when
//! it isn't on a terminal (CLI.md, "kiln export"), and these tasks never
//! pass `--yes`, `--force`, `--adopt` or `--prune-removed`. A task pane
//! that is a terminal would make kiln ask first, which is the person's
//! own answer. Publishing is for the Exports page (E3), with its plan and
//! its confirmation.
//!
//! What it deliberately doesn't do: decide when a project is usable (that
//! is [`crate::lsp::usability`]), look for kiln, or set the tasks.

use crate::api::{Command, Task};
use crate::lsp::Usability;

/// The tasks' names, as the picker shows them.
pub const CHECK: &str = "kiln: check";
pub const EXPORT_CHECK: &str = "kiln: export --check";
pub const EXPORT: &str = "kiln: export";

/// The project's folder, where Fenix runs a task (`filesystem` is `/project`
/// to an extension): kiln walks up from there to the `kiln.toml`.
const PROJECT: &str = "/project";

/// What the tasks should be now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Not known yet (kiln being looked for, the first read running):
    /// what was set stays.
    Keep,
    /// None: not a usable Kiln project, or no kiln to run.
    Clear,
    /// These three, running this kiln program.
    With(String),
}

/// What to offer in a project that is `usability`, for the kiln program
/// (`Err` when there's none). It is a function of the program alone, so
/// reloads that find the same kiln offer the same tasks and set nothing.
pub fn offer(usability: &Usability, program: Result<String, String>) -> Offer {
    match usability {
        Usability::Unknown => Offer::Keep,
        Usability::Unusable(_) => Offer::Clear,
        Usability::Usable => match program {
            Ok(program) => Offer::With(program),
            Err(_) => Offer::Clear,
        },
    }
}

/// The three tasks, running `program`.
pub fn tasks(program: &str) -> Vec<Task> {
    [
        (CHECK, &["check"][..]),
        (EXPORT_CHECK, &["export", "--check"][..]),
        (EXPORT, &["export"][..]),
    ]
    .into_iter()
    .map(|(name, args)| Task {
        name: name.to_string(),
        cmd: Command::new(program)
            .args(args.iter().copied())
            .cwd(PROJECT),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &Task) -> String {
        fenix_test_kit::line(&t.cmd)
    }

    #[test]
    fn three_tasks_run_the_given_kiln_in_the_project() {
        let t = tasks("/usr/bin/kiln");
        let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, [CHECK, EXPORT_CHECK, EXPORT]);
        assert_eq!(
            t.iter().map(line).collect::<Vec<_>>(),
            [
                "/usr/bin/kiln check",
                "/usr/bin/kiln export --check",
                "/usr/bin/kiln export"
            ]
        );
        assert!(t.iter().all(|t| t.cmd.cwd.as_deref() == Some(PROJECT)));
    }

    #[test]
    fn no_task_can_publish_to_confluence() {
        for t in tasks("kiln") {
            for flag in ["--yes", "--force", "--adopt", "--prune-removed"] {
                assert!(
                    !t.cmd.args.iter().any(|a| a == flag),
                    "{} has {flag}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn tasks_follow_the_project_and_the_program() {
        let found = || Ok("/usr/bin/kiln".to_string());
        let none = || Err("kiln isn't on the PATH".to_string());
        assert_eq!(offer(&Usability::Unknown, found()), Offer::Keep);
        assert_eq!(
            offer(&Usability::Unusable("no kiln.toml".into()), found()),
            Offer::Clear
        );
        assert_eq!(
            offer(&Usability::Usable, found()),
            Offer::With("/usr/bin/kiln".into())
        );
        assert_eq!(offer(&Usability::Usable, none()), Offer::Clear);
    }
}
