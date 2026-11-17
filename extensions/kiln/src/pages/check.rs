//! Check: `kiln check`'s findings under a heading per file, one row each --
//! `12:3  K102  error  reference REQ-099 doesn't exist` -- with `Enter` at
//! the line and `e` for errors only.
//!
//! The findings are already in the loaded project (every read runs `kiln
//! check`, and the page's head counts them), so this tab asks nothing. When
//! `check` itself failed, its line is shown as a note: there is nothing
//! to group.
//!
//! Within a file the findings are in line order, as an editor's problem
//! list has them, not errors before warnings: you read a file top down and
//! fix it so. The files come in the order kiln first names them. Findings
//! without a file (a project-wide one) go under a heading of their own,
//! and have no place for `Enter`.
//!
//! What it deliberately doesn't do: explain a code, offer fixes (the
//! language server's code actions do), or re-run `check` on its own: `r`
//! reads the project again, `check` included.

use std::collections::HashMap;

use super::view::{Cell, Ctx, Look, Reply, Row, View};
use crate::model::{count, Finding, Severity};
use fenix_ui_kit::{Key, KeyPress};
use serde_json::{json, Value};

/// What the heading of findings with no file says.
const PROJECT: &str = "the project";

/// The Check tab. Its one state is `e`.
#[derive(Debug, Default)]
pub struct Check {
    /// `e`: errors only.
    errors_only: bool,
}

impl View for Check {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let findings = match &ctx.loaded.findings {
            Ok(f) => f,
            Err(why) => return vec![Row::note(why.clone())],
        };
        // Files in the order kiln first names them, each with its
        // findings: an index built once, so this is linear in them.
        let mut files: Vec<(&str, Vec<&Finding>)> = Vec::new();
        let mut at: HashMap<&str, usize> = HashMap::new();
        for f in findings
            .iter()
            .filter(|f| !self.errors_only || f.severity == Severity::Error)
        {
            let n = *at.entry(f.path.as_str()).or_insert_with(|| {
                files.push((f.path.as_str(), Vec::new()));
                files.len() - 1
            });
            files[n].1.push(f);
        }
        let mut rows = Vec::new();
        for (path, mut group) in files {
            // Stable: findings on one line stay in kiln's order.
            group.sort_by_key(|f| (f.line, f.column));
            rows.push(Row::heading(vec![
                Cell::title(if path.is_empty() { PROJECT } else { path }),
                Cell::muted(count(group.len(), "finding")),
            ]));
            rows.extend(group.into_iter().map(finding_row));
        }
        rows
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![(
            "e",
            if self.errors_only {
                "all"
            } else {
                "errors only"
            },
        )]
    }

    fn key(&mut self, key: KeyPress, _row: Option<&Row>, _ctx: &Ctx) -> Reply {
        match key.key {
            Key::Char('e') if !key.mods.chord() => {
                self.errors_only = !self.errors_only;
                Reply::Rebuild
            }
            _ => Reply::Ignored,
        }
    }

    fn flex(&self) -> usize {
        3
    }

    fn empty(&self) -> String {
        if self.errors_only {
            "no errors (e shows the warnings too)".into()
        } else {
            "no findings: kiln check is clean".into()
        }
    }

    fn save(&self) -> Value {
        json!({ "errors_only": self.errors_only })
    }

    fn restore(&mut self, saved: &Value) {
        if let Some(on) = saved["errors_only"].as_bool() {
            self.errors_only = on;
        }
    }
}

/// One finding: `12:3  K102  error  message`, the severity coloured.
fn finding_row(f: &Finding) -> Row {
    let place = match (f.line, f.column) {
        (0, _) => String::new(),
        (l, 0) => l.to_string(),
        (l, c) => format!("{l}:{c}"),
    };
    let (severity, look) = match f.severity {
        Severity::Error => ("error", Look::Bad),
        Severity::Warning => ("warning", Look::Warn),
        Severity::Other => ("note", Look::Muted),
    };
    let mut row = Row::finding(
        &f.path,
        f.line,
        vec![
            Cell::muted(place),
            Cell::new(&f.code, look.clone()),
            Cell::new(severity, look),
            Cell::text(&f.message),
        ],
    );
    // No file, nowhere for `Enter` to go (it would say so).
    if f.path.is_empty() {
        row.place = None;
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture};
    use crate::pages::view::{Kind, Looks, Place};
    use crate::project::Loaded;
    use crate::tests::{demo, foot, opened, own};
    use fenix_test_kit::assert_snapshot;

    fn loaded(findings: Result<Vec<Finding>, String>) -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            findings,
        )
    }

    fn rows(l: &Loaded, v: &Check) -> Vec<Row> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        v.rows(&ctx)
    }

    fn lines(l: &Loaded, v: &Check) -> Vec<String> {
        rows(l, v)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}", text.join(" | "))
            })
            .collect()
    }

    fn finding(code: &str, sev: &str, path: &str, line: u32, column: u32) -> Finding {
        serde_json::from_value(json!({
            "code": code, "severity": sev, "message": "m", "path": path,
            "line": line, "column": column
        }))
        .unwrap()
    }

    #[test]
    fn the_demo_has_an_error_and_two_warnings_under_their_files() {
        let l = loaded(model::check(&fixture("demo/check.json")));
        assert_eq!(
            lines(&l, &Check::default()),
            [
                "# design/requirements/REQ-003-the-log-exports-as-csv.md | 1 finding",
                "2:7 | K201 | warning | REQ-003 is approved but nothing verifies it",
                "# design/tasks/TASK-006-draft-the-user-guide.md | 1 finding",
                "11:23 | K102 | error | reference REQ-099 doesn't exist",
                "# design/tasks/TASK-008-tidy-the-build-scripts.md | 1 finding",
                "2:7 | K213 | warning | TASK-008 is open work in no milestone; `kiln link TASK-008 part-of MS-…` places it",
            ]
        );
        let r = rows(&l, &Check::default());
        assert_eq!(r[1].cells[2].look, Look::Warn);
        assert_eq!(r[3].cells[2].look, Look::Bad);
        assert_eq!(r[3].kind, Kind::Finding);
        assert_eq!(
            r[3].place,
            Some(Place {
                path: "design/tasks/TASK-006-draft-the-user-guide.md".into(),
                line: Some(11)
            })
        );
    }

    #[test]
    fn e_keeps_the_errors_and_drops_the_headings_of_files_without_any() {
        let l = loaded(model::check(&fixture("demo/check.json")));
        let mut v = Check::default();
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        assert_eq!(v.keys(), [("e", "errors only")]);
        let e = KeyPress::new(Key::Char('e'), fenix_ui_kit::Mods::NONE);
        assert_eq!(v.key(e, None, &ctx), Reply::Rebuild);
        assert_eq!(
            lines(&l, &v),
            [
                "# design/tasks/TASK-006-draft-the-user-guide.md | 1 finding",
                "11:23 | K102 | error | reference REQ-099 doesn't exist",
            ]
        );
        assert_eq!(v.keys(), [("e", "all")]);
        let ctrl_e = KeyPress::new(Key::Char('e'), fenix_ui_kit::Mods::CTRL);
        assert_eq!(v.key(ctrl_e, None, &ctx), Reply::Ignored);
        let x = KeyPress::new(Key::Char('x'), fenix_ui_kit::Mods::NONE);
        assert_eq!(v.key(x, None, &ctx), Reply::Ignored);
        assert_eq!(v.key(e, None, &ctx), Reply::Rebuild);
        assert_eq!(lines(&l, &v).len(), 6);
        // Only warnings: errors-only is empty, and says so.
        let warned = loaded(Ok(vec![finding("K201", "warning", "a.md", 1, 1)]));
        v.errors_only = true;
        assert!(rows(&warned, &v).is_empty());
        assert_eq!(v.empty(), "no errors (e shows the warnings too)");
    }

    #[test]
    fn findings_keep_line_order_and_files_their_first_appearance() {
        let l = loaded(Ok(vec![
            finding("K201", "warning", "b.md", 9, 1),
            finding("K102", "error", "a.md", 7, 3),
            finding("K101", "error", "b.md", 2, 0),
            finding("K000", "error", "", 0, 0),
            finding("K300", "info", "a.md", 1, 5),
            finding("K103", "error", "b.md", 2, 0),
        ]));
        let v = Check::default();
        assert_eq!(
            lines(&l, &v)
                .iter()
                .map(|s| s.split(" | ").take(2).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>(),
            [
                "# b.md 3 findings",
                "2 K101",
                "2 K103",
                "9:1 K201",
                "# a.md 2 findings",
                "1:5 K300",
                "7:3 K102",
                "# the project 1 finding",
                " K000"
            ]
        );
        let r = rows(&l, &v);
        assert_eq!(r[3].cells[2].text, "warning", "line order, not severity");
        assert_eq!(
            r[5].cells[2].look,
            Look::Muted,
            "a severity it doesn't know"
        );
        assert_eq!(
            r[8].place, None,
            "a project-wide finding has no file to open"
        );
        assert_eq!(r[8].kind, Kind::Finding);
    }

    #[test]
    fn a_check_that_failed_is_its_line_and_none_is_empty() {
        let l = loaded(Err("kiln check: kiln's JSON can't be read: eof".into()));
        assert_eq!(
            lines(&l, &Check::default()),
            ["kiln check: kiln's JSON can't be read: eof"]
        );
        let clean = loaded(Ok(vec![]));
        assert!(rows(&clean, &Check::default()).is_empty());
        assert_eq!(Check::default().empty(), "no findings: kiln check is clean");
    }

    #[test]
    fn the_state_round_trips_and_a_bad_one_is_the_default() {
        let v = Check { errors_only: true };
        let mut w = Check::default();
        w.restore(&v.save());
        assert!(w.errors_only);
        for bad in [
            json!(null),
            json!("x"),
            json!({"errors_only": 1}),
            json!([]),
        ] {
            let mut w = Check::default();
            w.restore(&bad);
            assert!(!w.errors_only, "{bad}");
        }
    }

    #[test]
    fn the_demo_page_lists_its_findings_and_e_narrows_to_the_error() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "9");
        assert_snapshot!("check-demo", h.screen(page));
        h.keys(page, "e");
        assert_snapshot!("check-demo-errors", h.screen(page));
        let foot = foot(&mut h, page);
        assert!(foot.starts_with("e all"), "{foot}");
        fake.take();
        h.keys(page, "<Enter>");
        assert_eq!(
            fake.take(),
            ["open /project/design/tasks/TASK-006-draft-the-user-guide.md:11"]
        );
        h.keys(page, "e");
        assert!(h.screen(page).contains("K213"));
    }

    #[test]
    fn kilns_own_project_has_no_findings() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.keys(page, "9");
        assert_snapshot!("check-own", h.screen(page));
    }
}
