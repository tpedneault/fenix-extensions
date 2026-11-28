//! The tasks Kiln adds to Fenix's task picker (`SPC t t`, `project::tasks`):
//! `kiln: check`, `kiln: export (file targets only)` and `kiln: export
//! --check (file targets only)`, each running the kiln this extension
//! runs, in the project's folder.
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
//! # File targets only, named
//!
//! `kiln: export` writes the file targets (Markdown, HTML, Word), and
//! names them: `kiln export -- NAME…`, from the project's `kiln.toml`
//! ([`crate::config::Config::file_targets`], the same list the Exports
//! tab's `W` names). A bare `kiln export` is wrong in a project with a
//! Confluence target twice over: kiln then plans that target against the
//! live space, so with no token the whole run fails before it writes any
//! file (recorded: tests/fixtures/out/exports/bare-no-token.*), and in a
//! task pane that is a terminal kiln would ASK whether to publish it,
//! which is a way to publish from a picker. Naming the file targets
//! writes exactly them and can't reach Confluence; `--` before the names
//! so none is read as an option. `kiln: export --check` is the same with
//! `--check`: whether the committed files are current. The tasks never
//! pass `--yes`, `--force`, `--adopt` or `--prune-removed` either.
//! Publishing is for the Exports page, with its plan and its
//! confirmation.
//!
//! The configuration is read by the extension, not kiln, so when it can't
//! be read, or names no file target, neither export task is offered (what
//! to name isn't known, and a task that named nothing would be the bare
//! command); `kiln: check` still is. The names are part of what is offered:
//! a new target in `kiln.toml` brings the tasks again.
//!
//! What it deliberately doesn't do: decide when a project is usable (that
//! is [`crate::lsp::usability`]), look for kiln, or set the tasks.

use crate::api::{Command, Task};
use crate::lsp::Usability;

/// The tasks' names, as the picker shows them: the export ones say they
/// write files only, and which.
pub const CHECK: &str = "kiln: check";
pub const EXPORT_CHECK: &str = "kiln: export --check (file targets only)";
pub const EXPORT: &str = "kiln: export (file targets only)";

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
    /// `kiln: check`, and the export tasks naming these file targets (when
    /// there are any), running this kiln program.
    With { program: String, files: Vec<String> },
}

/// What to offer in a project that is `usability`, for the kiln program
/// (`Err` when there's none) and the file targets `kiln.toml` names (none
/// when it can't be read). It is a function of those alone, so reloads
/// that find the same kiln and targets offer the same tasks and set
/// nothing.
pub fn offer(usability: &Usability, program: Result<String, String>, files: Vec<String>) -> Offer {
    match usability {
        Usability::Unknown => Offer::Keep,
        Usability::Unusable(_) => Offer::Clear,
        Usability::Usable => match program {
            Ok(program) => Offer::With { program, files },
            Err(_) => Offer::Clear,
        },
    }
}

/// The tasks, running `program`: `kiln: check`, and the two export tasks
/// naming `files` -- none of them when there are none.
pub fn tasks(program: &str, files: &[String]) -> Vec<Task> {
    let task = |name: &str, args: Vec<&str>| Task {
        name: name.to_string(),
        cmd: Command::new(program).args(args).cwd(PROJECT),
    };
    let mut out = vec![task(CHECK, vec!["check"])];
    if !files.is_empty() {
        let named = |first: &[&'static str]| -> Vec<&str> {
            first
                .iter()
                .copied()
                .chain(std::iter::once("--"))
                .chain(files.iter().map(String::as_str))
                .collect()
        };
        out.push(task(EXPORT_CHECK, named(&["export", "--check"])));
        out.push(task(EXPORT, named(&["export"])));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &Task) -> String {
        fenix_test_kit::line(&t.cmd)
    }

    fn files() -> Vec<String> {
        vec!["design-md".into(), "site".into()]
    }

    #[test]
    fn the_export_tasks_name_the_file_targets_and_run_in_the_project() {
        let t = tasks("/usr/bin/kiln", &files());
        let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, [CHECK, EXPORT_CHECK, EXPORT]);
        assert!(
            EXPORT.contains("file targets only") && EXPORT_CHECK.contains("file targets only"),
            "titled so it's clear they write files only"
        );
        assert_eq!(
            t.iter().map(line).collect::<Vec<_>>(),
            [
                "/usr/bin/kiln check",
                "/usr/bin/kiln export --check -- design-md site",
                "/usr/bin/kiln export -- design-md site"
            ]
        );
        assert!(t.iter().all(|t| t.cmd.cwd.as_deref() == Some(PROJECT)));
    }

    #[test]
    fn with_no_file_target_only_the_check_is_offered() {
        let t = tasks("kiln", &[]);
        assert_eq!(
            t.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            [CHECK]
        );
        assert!(
            !t.iter().any(|t| t.cmd.args == ["export"]),
            "never the bare export"
        );
    }

    #[test]
    fn no_task_passes_a_flag_that_publishes_or_overrides() {
        for files in [files(), vec![]] {
            for t in tasks("kiln", &files) {
                for flag in [
                    "--yes",
                    "--force",
                    "--adopt",
                    "--prune-removed",
                    "--allow-shallow",
                ] {
                    assert!(
                        !t.cmd.args.iter().any(|a| a == flag),
                        "{} has {flag}",
                        t.name
                    );
                }
            }
        }
    }

    #[test]
    fn no_task_can_name_a_confluence_target() {
        // What `kiln.toml` holds: two file targets and two Confluence ones.
        let c = crate::config::parse(&crate::model::tests::fixture("exports/kiln.toml")).unwrap();
        let files: Vec<String> = c.file_targets().into_iter().map(String::from).collect();
        assert!(c.has_confluence_target(), "the fixture has some");
        let confluence: Vec<&str> = c
            .exports
            .iter()
            .filter(|e| e.is_confluence())
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(confluence, ["design-doc-confluence", "adopted"]);
        for t in tasks("kiln", &files) {
            // After `--`, only file targets: never a Confluence one, and
            // never a bare `export` that would take every target.
            let after: Vec<&String> = match t.cmd.args.iter().position(|a| a == "--") {
                Some(at) => t.cmd.args[at + 1..].iter().collect(),
                None => Vec::new(),
            };
            if t.cmd.args.first().map(String::as_str) == Some("export") {
                assert!(!after.is_empty(), "{}: a bare export", t.name);
            }
            for name in after {
                assert!(
                    c.file_targets().contains(&name.as_str())
                        && !confluence.contains(&name.as_str()),
                    "{} names {name}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn tasks_follow_the_project_the_program_and_the_targets() {
        let found = || Ok("/usr/bin/kiln".to_string());
        let none = || Err("kiln isn't on the PATH".to_string());
        assert_eq!(offer(&Usability::Unknown, found(), files()), Offer::Keep);
        assert_eq!(
            offer(
                &Usability::Unusable("no kiln.toml".into()),
                found(),
                files()
            ),
            Offer::Clear
        );
        assert_eq!(
            offer(&Usability::Usable, found(), files()),
            Offer::With {
                program: "/usr/bin/kiln".into(),
                files: files()
            }
        );
        assert_eq!(offer(&Usability::Usable, none(), files()), Offer::Clear);
    }
}
