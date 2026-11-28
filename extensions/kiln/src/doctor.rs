//! The project doctor's Kiln rows (`project::doctor`): kiln found and its
//! version, `kiln.toml` read, the language server, and what the last `kiln
//! check` found -- each with what to do when it isn't right.
//!
//! A pure function from what the extension knows to the rows, so every
//! wording and health is a plain test; `lib.rs` sets the rows when they
//! differ from the ones it last set.
//!
//! What it deliberately doesn't do: look for kiln or read a setting (what
//! it needs is handed in), download kiln (the extension has no network
//! permission: the fix row for a missing kiln opens Kiln's install
//! instructions), or draw anything.
//!
//! # Rows by state
//!
//! | The project ([`State`]) | Rows |
//! |---|---|
//! | not asked yet, kiln being looked for, first read running | none yet: [`rows`] says `None`, so what was set stays |
//! | kiln missing, not Kiln's, too old | `kiln` alone, with its fix |
//! | no `kiln.toml` | none: not a Kiln project, so nothing here is its business |
//! | read | `kiln`, `kiln.toml`, `language server`, `kiln check` |
//! | first read failed | `kiln`, `kiln.toml` (kiln's line), `language server` |
//!
//! With kiln unusable the language server's row would only repeat the
//! `kiln` row's reason (`lsp` is unregistered for the same cause), so it
//! is left out there.

use crate::api::{Check, Health, Section};
use crate::lsp;
use crate::model::{self, count, MIN_VERSION};
use crate::project::{Loaded, State};

/// The fix row's command for a missing, foreign or old kiln: opens Kiln's
/// install instructions (`kiln.install_help`, in the manifest).
pub const INSTALL_HELP: &str = "kiln.install_help";

/// The command that opens the plan on its Check tab, which lists what
/// `kiln check` found.
pub const OPEN: &str = "kiln.findings";

/// Where Kiln's install instructions are: a page of Kiln's wiki. Opening
/// it is all the extension does about a missing kiln.
pub const INSTALL_URL: &str = "https://github.com/tpedneault/kiln/wiki/Installing";

/// What the rows are worked out from.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub state: State<'a>,
    /// The language server's status ([`lsp::Lsp::status`]).
    pub lsp: &'a lsp::Status,
    /// `kiln.path`, when it isn't empty.
    pub kiln_path: Option<&'a str>,
    /// The kiln program in use, once it is known to run.
    pub program: Option<&'a str>,
    /// kiln's version, once known ([`crate::project::Project::version`]).
    pub version: Option<model::Version>,
}

/// The rows, in the order the doctor shows them; `None` while it isn't
/// known yet whether this is a usable Kiln project, and no rows (`Some`
/// of none) where it isn't one.
pub fn rows(input: &Input) -> Option<Vec<Check>> {
    let kiln = |health, detail: String| Check::new(Section::Toolchain, "kiln", health, detail);
    match input.state {
        State::Idle | State::Probing | State::Loading => None,
        State::NoProject(_) => Some(Vec::new()),
        State::NotFound(_) => {
            let detail = match input.kiln_path {
                None => "isn't on the PATH, and kiln.path is empty".to_string(),
                Some(path) => format!(
                    "kiln.path is {path}, which can't be run: set it to where kiln is, or empty it to use the PATH"
                ),
            };
            Some(vec![
                kiln(Health::Bad, detail).fix("how to install Kiln", INSTALL_HELP)
            ])
        }
        State::NotKiln(why) => Some(vec![kiln(Health::Bad, format!("can't be used: {why}"))
            .fix("how to install Kiln", INSTALL_HELP)]),
        State::TooOld(v) => Some(vec![kiln(
            Health::Bad,
            format!("kiln {v} is older than {MIN_VERSION}, the oldest this extension reads"),
        )
        .fix("how to update Kiln", INSTALL_HELP)]),
        State::Loaded { loaded, error, .. } => {
            let mut out = vec![kiln_found(input, Some(loaded.version))];
            out.push(match error {
                // The last reload failed: kiln's line, and the rows after
                // are from the read before it, as the pages show it.
                Some(line) => toml(Health::Bad, line.to_string()),
                None => toml(
                    Health::Ok,
                    format!("read, {}", count(loaded.items.len(), "item")),
                ),
            });
            out.push(server(input.lsp));
            out.push(check(loaded));
            Some(out)
        }
        State::Failed(line) => Some(vec![
            kiln_found(input, input.version),
            toml(Health::Bad, line.to_string()),
            server(input.lsp),
        ]),
    }
}

fn kiln_found(input: &Input, version: Option<model::Version>) -> Check {
    let mut detail = match version {
        Some(v) => format!("kiln {v}"),
        None => "found".to_string(),
    };
    if let Some(program) = input.program {
        detail.push_str(&format!(", {program}"));
    }
    Check::new(Section::Toolchain, "kiln", Health::Ok, detail)
}

fn toml(health: Health, detail: String) -> Check {
    Check::new(Section::Project, "kiln.toml", health, detail)
}

/// The language server's row, from what `lsp` decided.
fn server(status: &lsp::Status) -> Check {
    let row =
        |health, detail: String| Check::new(Section::Toolchain, "language server", health, detail);
    match status {
        lsp::Status::Pending => row(
            Health::Info,
            "waiting for kiln to say which languages it serves".into(),
        ),
        lsp::Status::Registered { server, note } => match note {
            None => row(
                Health::Ok,
                format!("kiln lsp, for {}", server.languages.join(", ")),
            ),
            // Markdown only is working, less than it could: said, not failed.
            Some(note) => row(
                Health::Info,
                format!("kiln lsp, for {} only: {note}", server.languages.join(", ")),
            ),
        },
        lsp::Status::ToolsJson(_) => row(
            Health::Ok,
            "left to .fenix/tools.json, which runs kiln lsp".into(),
        ),
        lsp::Status::NotRegistered(why) => row(Health::Warn, format!("not registered: {why}")),
    }
}

/// `kiln check`'s row: what the last check found, or why there's no
/// answer. A row with findings has a fix that opens the plan's Check tab,
/// which lists them.
fn check(loaded: &Loaded) -> Check {
    let row = |health, detail: String| Check::new(Section::Project, "kiln check", health, detail);
    if let Err(why) = &loaded.findings {
        return row(Health::Warn, format!("not checked: {why}"));
    }
    let counts = loaded.counts().unwrap_or_default();
    match counts.said() {
        None => row(Health::Ok, "no findings".into()),
        Some(said) => {
            let health = if counts.errors > 0 {
                Health::Bad
            } else {
                Health::Warn
            };
            row(health, said).fix("open the Check tab", OPEN)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use crate::model::Version;

    const V1: Version = Version {
        major: 1,
        minor: 0,
        patch: 0,
    };

    fn loaded(check: &str) -> Loaded {
        Loaded::new(
            V1,
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            model::check(check),
        )
    }

    fn server(note: Option<&str>) -> lsp::Status {
        lsp::Status::Registered {
            server: lsp::Server {
                languages: vec!["markdown".into(), "rust".into()],
                program: "/usr/bin/kiln".into(),
            },
            note: note.map(str::to_string),
        }
    }

    fn input<'a>(state: State<'a>, lsp: &'a lsp::Status) -> Input<'a> {
        Input {
            state,
            lsp,
            kiln_path: None,
            program: Some("/usr/bin/kiln"),
            version: Some(V1),
        }
    }

    /// One row as a line: `label health detail`, with `-> fix` when it has one.
    fn said(rows: Option<Vec<Check>>) -> Vec<String> {
        rows.expect("rows").iter().map(row).collect()
    }

    fn row(c: &Check) -> String {
        let fix = c
            .fix
            .as_ref()
            .map(|(label, command)| format!(" -> {label} ({command})"))
            .unwrap_or_default();
        let health = format!("{:?}", c.health);
        let health = health.strip_prefix("Health::").unwrap_or(&health);
        format!("{} {health} {}{fix}", c.label, c.detail)
    }

    fn state(l: &Loaded) -> State<'_> {
        State::Loaded {
            loaded: l,
            reloading: false,
            error: None,
        }
    }

    #[test]
    fn a_project_with_findings_says_each_row() {
        let l = loaded(&fixture("demo/check.json"));
        let status = server(None);
        assert_eq!(
            said(rows(&input(state(&l), &status))),
            [
                "kiln Ok kiln 1.0.0, /usr/bin/kiln",
                "kiln.toml Ok read, 16 items",
                "language server Ok kiln lsp, for markdown, rust",
                "kiln check Bad 1 error, 2 warnings -> open the Check tab (kiln.findings)",
            ]
        );
    }

    #[test]
    fn the_rows_sit_in_the_sections_where_they_belong() {
        let l = loaded("[]");
        let status = server(None);
        let got = rows(&input(state(&l), &status)).unwrap();
        let sections: Vec<Section> = got.iter().map(|c| c.section).collect();
        assert_eq!(
            sections,
            [
                Section::Toolchain,
                Section::Project,
                Section::Toolchain,
                Section::Project
            ]
        );
    }

    #[test]
    fn warnings_alone_warn_and_no_findings_is_ok_without_a_fix() {
        let status = server(None);
        let warnings = r#"[{"code":"K201","severity":"warning","message":"m","path":"a.md","line":1,"column":1}]"#;
        let l = loaded(warnings);
        let got = rows(&input(state(&l), &status)).unwrap();
        assert_eq!(
            row(&got[3]),
            "kiln check Warn 1 warning -> open the Check tab (kiln.findings)"
        );
        let l = loaded("[]");
        let got = rows(&input(state(&l), &status)).unwrap();
        assert_eq!(row(&got[3]), "kiln check Ok no findings");
    }

    #[test]
    fn a_check_that_gave_no_answer_says_so() {
        let status = server(None);
        let l = Loaded::new(
            V1,
            Vec::new(),
            Err("x".into()),
            Err("kiln: check broke".into()),
        );
        let rows = rows(&input(state(&l), &status)).unwrap();
        assert_eq!(
            row(&rows[3]),
            "kiln check Warn not checked: kiln: check broke"
        );
    }

    #[test]
    fn the_language_server_row_follows_its_status() {
        let l = loaded("[]");
        let line = |status: lsp::Status| {
            let rows = rows(&input(state(&l), &status)).unwrap();
            row(&rows[2])
        };
        assert_eq!(
            line(lsp::Status::Pending),
            "language server Info waiting for kiln to say which languages it serves"
        );
        assert_eq!(
            line(server(Some("kiln 1.0.0 has no `kiln lsp --languages`"))),
            "language server Info kiln lsp, for markdown, rust only: kiln 1.0.0 has no `kiln lsp --languages`"
        );
        assert_eq!(
            line(lsp::Status::ToolsJson(lsp::Form::Additional)),
            "language server Ok left to .fenix/tools.json, which runs kiln lsp"
        );
        assert_eq!(
            line(lsp::Status::NotRegistered(
                "Fenix refused kiln's language server: x".into()
            )),
            "language server Warn not registered: Fenix refused kiln's language server: x"
        );
    }

    #[test]
    fn a_project_kiln_cant_read_says_kilns_line_as_it_wrote_it() {
        let status = server(None);
        let line = "kiln: kiln.toml:4: unknown key `nmae`";
        // The first read failed.
        assert_eq!(
            said(rows(&input(State::Failed(line), &status))),
            [
                "kiln Ok kiln 1.0.0, /usr/bin/kiln".to_string(),
                format!("kiln.toml Bad {line}"),
                "language server Ok kiln lsp, for markdown, rust".to_string(),
            ]
        );
        // A later read failed: the read before it is what the rest says.
        let l = loaded("[]");
        let failed = State::Loaded {
            loaded: &l,
            reloading: false,
            error: Some(line),
        };
        let rows = rows(&input(failed, &status)).unwrap();
        assert_eq!(row(&rows[1]), format!("kiln.toml Bad {line}"));
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn a_missing_kiln_says_how_to_install_it_and_kiln_path() {
        let status = lsp::Status::NotRegistered("x".into());
        let mut i = input(State::NotFound("kiln isn't on the PATH: …"), &status);
        i.program = None;
        i.version = None;
        assert_eq!(
            said(rows(&i)),
            ["kiln Bad isn't on the PATH, and kiln.path is empty -> how to install Kiln (kiln.install_help)"]
        );
        i.kiln_path = Some("/opt/kiln/kiln");
        assert_eq!(
            said(rows(&i)),
            ["kiln Bad kiln.path is /opt/kiln/kiln, which can't be run: set it to where kiln is, or empty it to use the PATH -> how to install Kiln (kiln.install_help)"]
        );
    }

    #[test]
    fn a_kiln_that_isnt_kilns_or_is_too_old_says_so() {
        let status = lsp::Status::NotRegistered("x".into());
        let i = input(
            State::NotKiln("`kiln --version` said \"hi\", not `kiln <version>`"),
            &status,
        );
        assert_eq!(
            said(rows(&i)),
            ["kiln Bad can't be used: `kiln --version` said \"hi\", not `kiln <version>` -> how to install Kiln (kiln.install_help)"]
        );
        let old = Version {
            major: 0,
            minor: 9,
            patch: 2,
        };
        assert_eq!(
            said(rows(&input(State::TooOld(old), &status))),
            ["kiln Bad kiln 0.9.2 is older than 1.0.0, the oldest this extension reads -> how to update Kiln (kiln.install_help)"]
        );
    }

    #[test]
    fn not_a_kiln_project_has_no_rows_and_unknown_has_none_to_say() {
        let status = lsp::Status::Pending;
        let none = State::NoProject("kiln: no kiln.toml in …");
        assert_eq!(rows(&input(none, &status)).map(|r| r.len()), Some(0));
        for s in [State::Idle, State::Probing, State::Loading] {
            assert!(rows(&input(s, &status)).is_none());
        }
    }

    #[test]
    fn a_kiln_without_a_known_path_or_version_still_reads() {
        let l = loaded("[]");
        let status = server(None);
        let mut i = input(state(&l), &status);
        i.program = None;
        assert_eq!(row(&rows(&i).unwrap()[0]), "kiln Ok kiln 1.0.0");
        let mut i = input(State::Failed("kiln: x"), &status);
        i.program = None;
        i.version = None;
        assert_eq!(row(&rows(&i).unwrap()[0]), "kiln Ok found");
    }

    #[test]
    fn the_install_page_is_kilns_wiki() {
        assert_eq!(
            INSTALL_URL,
            "https://github.com/tpedneault/kiln/wiki/Installing"
        );
    }
}
