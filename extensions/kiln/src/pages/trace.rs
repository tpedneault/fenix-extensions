//! Trace: per open item of the traced type (`requirement` unless `T` says
//! another), its ID and title, then a column for each coverage role: what
//! covers it -- items and code locations, the first few and `+N` -- or
//! `gap` in the error look. `m` keeps only the items with a gap; `T`
//! traces the next type the project has (`kiln trace`).
//!
//! The one tab whose rows come from a kiln answer rather than from
//! `list`: coverage is kiln's to compute (links, children, code
//! references), so this reads `kiln trace --format json` and shows it
//! as it is. The traced type is part of the query's args, so `T` makes the
//! frame ask again.
//!
//! What it deliberately doesn't do: guess which types a coverage role
//! reaches (kiln says so in the answer, `roles`, and for a type none
//! reaches the tab says that, one `T` from the next), run `--levels` (its
//! JSON isn't recorded for a project with levels, and Kiln 1.0.0 refuses
//! it without them: nothing here reads a shape it hasn't seen), or pass
//! `--results` (what a code reference's test did is for the Check tab and
//! CI).

use std::collections::BTreeMap;

use fenix_ui_kit::Key;
use serde::Deserialize;
use serde_json::{json, Value};

use super::view::{stdout, Cell, Ctx, Look, Query, Reply, Row, View};
use crate::kiln::{Failure, Ran};
use crate::model::{self, count};
use fenix_ui_kit::KeyPress;

/// The type kiln traces when none is named, as Kiln's GUI starts.
const DEFAULT: &str = "requirement";

/// How many of what covers an item a row names before `+N`: the row stays
/// one line, and `Enter` on it opens the item, where all of it is.
const SHOWN: usize = 2;

/// `kiln trace --format json`, the parts this reads. Unknown fields are
/// ignored; what's missing is empty.
#[derive(Debug, Clone, Deserialize)]
struct Traced {
    /// The type traced, as kiln names it.
    #[serde(rename = "type", default)]
    kind: String,
    /// The coverage roles that reach it, in kiln's order: the columns.
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    items: Vec<Traces>,
}

/// One item's coverage.
#[derive(Debug, Clone, Deserialize)]
struct Traces {
    id: String,
    #[serde(default)]
    title: String,
    /// Role -> what covers it: item IDs and `path:line` code locations.
    #[serde(default)]
    coverage: BTreeMap<String, Vec<String>>,
    /// The roles nothing covers.
    #[serde(default)]
    gaps: Vec<String>,
}

/// The Trace tab. Its state is what it traces, whether it shows only
/// gaps, and kiln's last answer.
#[derive(Debug, Default)]
pub struct Trace {
    /// The type asked for: none is kiln's default (`requirement`).
    kind: Option<String>,
    /// `m`: only the items with a gap.
    gaps_only: bool,
    /// What kiln last said: its trace, or its line.
    said: Option<Result<Traced, String>>,
}

impl Trace {
    /// What `kind` traces when none is chosen: `requirement`, else the
    /// project's first type, as the default isn't there to ask for.
    fn default_kind(kinds: &[&str]) -> Option<String> {
        if kinds.contains(&DEFAULT) {
            Some(DEFAULT.to_string())
        } else {
            kinds.first().map(|k| k.to_string())
        }
    }
}

/// The types the project has items of, in the order kiln lists them: those
/// `T` cycles. Kiln traces any type it has and says, in `roles`, whether a
/// coverage role reaches it; asking each is how the tab finds out.
fn kinds<'a>(ctx: &Ctx<'a>) -> Vec<&'a str> {
    let mut kinds: Vec<&str> = Vec::new();
    for item in &ctx.loaded.items {
        if !kinds.contains(&item.kind.as_str()) {
            kinds.push(item.kind.as_str());
        }
    }
    kinds
}

/// A coverage's text: `TASK-001, TASK-002 +1`, a code location as its file
/// and line (`docx.rs:131`, the folders being in the item).
fn covers(by: &[String]) -> String {
    let short = |s: &String| match s.rsplit_once('/') {
        Some((_, tail)) => tail.to_string(),
        None => s.clone(),
    };
    let text: Vec<String> = by.iter().take(SHOWN).map(short).collect();
    match by.len().checked_sub(SHOWN) {
        Some(more) if more > 0 => format!("{} +{more}", text.join(", ")),
        _ => text.join(", "),
    }
}

impl View for Trace {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let traced = match &self.said {
            None => return Vec::new(),
            Some(Err(line)) => return vec![Row::note(line.clone())],
            Some(Ok(t)) => t,
        };
        let gap = |t: &Traces| !t.gaps.is_empty();
        let gaps = traced.items.iter().filter(|t| gap(t)).count();
        let mut rows = vec![Row::heading(vec![
            Cell::title(&traced.kind),
            Cell::muted(count(traced.items.len(), "item")),
        ])];
        if !traced.roles.is_empty() {
            rows[0].cells.push(if gaps == 0 {
                Cell::new("no gaps", Look::Good)
            } else {
                Cell::new(count(gaps, "gap"), Look::Bad)
            });
            rows[0].cells.push(Cell::muted(format!(
                "columns: {}",
                traced.roles.join(" | ")
            )));
        }
        // The note under it says why there are no columns, once.
        if traced.roles.is_empty() {
            rows.push(Row::note(format!(
                "no coverage role reaches {}: nothing traces to it (T for the next type)",
                traced.kind
            )));
            return rows;
        }
        let before = rows.len();
        for t in traced.items.iter().filter(|t| !self.gaps_only || gap(t)) {
            let mut cells = vec![Cell::id(&t.id), Cell::text(&t.title)];
            for role in &traced.roles {
                let by = t.coverage.get(role).map_or(&[][..], Vec::as_slice);
                cells.push(if t.gaps.contains(role) {
                    Cell::new("gap", Look::Bad)
                } else if by.is_empty() {
                    // Not a gap and nothing of its own: kiln counts it
                    // covered by its open children (CLI.md, `kiln trace`).
                    Cell::muted("through children")
                } else {
                    Cell::text(covers(by))
                });
            }
            rows.push(ctx.row(&t.id, cells));
        }
        if rows.len() == before {
            // Every item is covered: the heading says so; the tab says why
            // there's nothing under it.
            rows.push(Row::note(if traced.items.is_empty() {
                format!("no open {} to trace", traced.kind)
            } else {
                "every item is covered (m shows them all)".to_string()
            }));
        }
        rows
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "m",
                if self.gaps_only {
                    "all items"
                } else {
                    "only gaps"
                },
            ),
            ("T", "next type"),
        ]
    }

    fn key(&mut self, key: KeyPress, _row: Option<&Row>, ctx: &Ctx) -> Reply {
        if key.mods.ctrl || key.mods.alt {
            return Reply::Ignored;
        }
        match key.key {
            Key::Char('m') => {
                self.gaps_only = !self.gaps_only;
                Reply::Rebuild
            }
            Key::Char('T') => {
                let kinds = kinds(ctx);
                let Some(default) = Trace::default_kind(&kinds) else {
                    return Reply::Say("no items, so no type to trace".into());
                };
                if kinds.len() < 2 {
                    return Reply::Say(format!("{default} is the only type here"));
                }
                let now = self.kind.as_deref().unwrap_or(&default);
                // A type that's gone from the project (a reload) starts
                // the cycle again from the default.
                let next = match kinds.iter().position(|k| *k == now) {
                    Some(at) => kinds[(at + 1) % kinds.len()],
                    None => default.as_str(),
                };
                self.kind = (next != default).then(|| next.to_string());
                Reply::Rebuild
            }
            _ => Reply::Ignored,
        }
    }

    fn empty(&self) -> String {
        match &self.said {
            None => "reading the coverage…".into(),
            Some(_) => "nothing to trace".into(),
        }
    }

    fn queries(&self, _ctx: &Ctx) -> Vec<Query> {
        let mut args = vec!["trace".to_string()];
        if let Some(kind) = &self.kind {
            args.extend(["--type".to_string(), kind.clone()]);
        }
        args.extend(["--format".to_string(), "json".to_string()]);
        vec![Query::new("trace", args)]
    }

    fn answer(&mut self, _tag: &str, result: &Result<Ran, Failure>) {
        self.said =
            Some(stdout(result, &[0]).and_then(|text| model::object::<Traced>("kiln trace", text)));
    }

    fn save(&self) -> Value {
        json!({ "kind": self.kind, "gaps_only": self.gaps_only })
    }

    fn restore(&mut self, saved: &Value) {
        self.kind = saved["kind"]
            .as_str()
            .filter(|k| !k.is_empty())
            .map(str::to_string);
        self.gaps_only = saved["gaps_only"].as_bool().unwrap_or(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;
    use crate::tests::{at, demo, foot, opened, own};
    use fenix_test_kit::{assert_snapshot, Fake, Ran as Says};

    fn loaded(dir: &str) -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture(&format!("{dir}/list.json"))).unwrap(),
            model::status(&fixture(&format!("{dir}/status.json"))),
            model::check(&fixture(&format!("{dir}/check.json"))),
        )
    }

    fn ran(code: i32, stdout: &str, stderr: &str) -> Result<Ran, Failure> {
        Ok(Ran {
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.lines().map(str::to_string).collect(),
        })
    }

    fn lines(view: &Trace, l: &Loaded) -> Vec<String> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        view.rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}", text.join(" | "))
            })
            .collect()
    }

    fn press(view: &mut Trace, l: &Loaded, c: char) -> Reply {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        view.key(KeyPress::from(Key::Char(c)), None, &ctx)
    }

    fn args(view: &Trace, l: &Loaded) -> Vec<String> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        view.queries(&ctx).remove(0).args
    }

    fn with(text: &str) -> Trace {
        let mut t = Trace::default();
        t.answer("trace", &ran(0, text, ""));
        t
    }

    #[test]
    fn the_demo_has_a_column_for_each_role_and_says_gap() {
        let l = loaded("demo");
        let t = with(&fixture("demo/trace.json"));
        assert_eq!(
            lines(&t, &l),
            [
                "# requirement | 3 items | 2 gaps | columns: implements | verifies",
                "REQ-001 | Readings are logged every second | TASK-001, TASK-002 | TEST-001",
                "REQ-002 | Logged readings survive a power cut | gap | gap",
                "REQ-003 | The log exports as CSV | TASK-003 | gap",
            ]
        );
    }

    #[test]
    fn gap_cells_are_in_the_error_look_and_rows_open_their_item() {
        let l = loaded("demo");
        let t = with(&fixture("demo/trace.json"));
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        let rows = t.rows(&ctx);
        assert_eq!(rows[3].cells[3], Cell::new("gap", Look::Bad));
        assert_eq!(rows[3].cells[2], Cell::text("TASK-003"));
        assert_eq!(rows[3].id.as_deref(), Some("REQ-003"));
        assert!(rows[3].place.is_some(), "Enter opens its file");
    }

    #[test]
    fn m_keeps_only_the_items_with_a_gap_and_says_when_there_are_none() {
        let l = loaded("demo");
        let mut t = with(&fixture("demo/trace.json"));
        assert_eq!(press(&mut t, &l, 'm'), Reply::Rebuild);
        assert_eq!(lines(&t, &l).len(), 3, "{:?}", lines(&t, &l));
        assert!(!lines(&t, &l).join("\n").contains("REQ-001"));
        press(&mut t, &l, 'm');
        assert_eq!(lines(&t, &l).len(), 4);
        // Kiln's own project has none, and the tab says so.
        let o = loaded("own");
        let mut own = with(&fixture("own/trace.json"));
        press(&mut own, &o, 'm');
        assert_eq!(
            lines(&own, &o),
            [
                "# requirement | 25 items | no gaps | columns: implements | verifies",
                "every item is covered (m shows them all)"
            ]
        );
    }

    #[test]
    fn kilns_own_rows_keep_to_one_line_with_a_plus_n_and_children_noted() {
        let o = loaded("own");
        let t = with(&fixture("own/trace.json"));
        let rows = lines(&t, &o);
        assert_eq!(rows.len(), 26);
        assert_eq!(
            rows[1],
            "REQ-001 | The repository is the source of truth | through children | cli.rs:957"
        );
        assert_eq!(
            rows[2],
            "REQ-002 | Readable without Kiln | apply.rs:82, docx.rs:131 +3 | tests.rs:301, tests.rs:326 +3"
        );
        assert_eq!(
            rows[3],
            "REQ-003 | Checkable in CI | through children | through children"
        );
    }

    #[test]
    fn t_cycles_the_types_kiln_has_and_the_query_follows() {
        let l = loaded("demo");
        let mut t = Trace::default();
        assert_eq!(args(&t, &l), ["trace", "--format", "json"]);
        let mut seen = Vec::new();
        for _ in 0..6 {
            assert_eq!(press(&mut t, &l, 'T'), Reply::Rebuild);
            seen.push(args(&t, &l).join(" "));
        }
        assert_eq!(
            seen,
            [
                "trace --type decision --format json",
                "trace --type milestone --format json",
                "trace --type task --format json",
                "trace --type test --format json",
                "trace --type risk --format json",
                "trace --format json"
            ],
            "the default is the cycle's start and end"
        );
        // A type the project lost starts the cycle again.
        t.restore(&json!({"kind": "design"}));
        assert_eq!(press(&mut t, &l, 'T'), Reply::Rebuild);
        assert_eq!(args(&t, &l), ["trace", "--format", "json"]);
    }

    #[test]
    fn a_type_no_role_reaches_is_said_and_t_goes_on() {
        let l = loaded("demo");
        let t = with(&fixture("demo/trace-risk.json"));
        assert_eq!(
            lines(&t, &l),
            [
                "# risk | 1 item",
                "no coverage role reaches risk: nothing traces to it (T for the next type)"
            ]
        );
        let task = with(&fixture("demo/trace-task.json"));
        assert_eq!(lines(&task, &l).len(), 2);
    }

    #[test]
    fn a_project_with_one_type_says_so_and_a_missing_one_says_none() {
        let l = Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(r#"[{"id":"REQ-001","type":"requirement","title":"t","status":"draft"}]"#)
                .unwrap(),
            Err("none".into()),
            Ok(vec![]),
        );
        let mut t = Trace::default();
        assert_eq!(
            press(&mut t, &l, 'T'),
            Reply::Say("requirement is the only type here".into())
        );
        let none = Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            vec![],
            Err("none".into()),
            Ok(vec![]),
        );
        assert!(matches!(press(&mut t, &none, 'T'), Reply::Say(_)));
        assert_eq!(press(&mut t, &l, 'x'), Reply::Ignored);
    }

    #[test]
    fn what_kiln_cant_say_is_one_note_and_unknown_fields_are_fine() {
        let l = loaded("demo");
        let mut t = Trace::default();
        assert!(lines(&t, &l).is_empty(), "before any answer");
        t.answer("trace", &ran(2, "", "kiln: `nosuch` isn't a type\n"));
        assert_eq!(lines(&t, &l), ["kiln: `nosuch` isn't a type"]);
        t.answer("trace", &ran(0, "not json", ""));
        let said = lines(&t, &l);
        assert_eq!(said.len(), 1);
        assert!(
            said[0].starts_with("kiln trace: kiln's JSON can't be read"),
            "{said:?}"
        );
        t.answer("trace", &ran(0, r#"{"kiln_json": 2, "items": []}"#, ""));
        assert!(lines(&t, &l)[0].contains("shape 2"));
        t.answer("trace", &Err(Failure::TimedOut));
        assert!(lines(&t, &l)[0].contains("no answer"));
        t.answer(
            "trace",
            &ran(
                0,
                r#"{"kiln_json":1,"type":"requirement","roles":["verifies"],"new":1,
                    "items":[{"id":"REQ-9","title":"x","status":"draft","more":2,
                              "coverage":{"verifies":["a/b.rs:3"]},"gaps":[]}]}"#,
                "",
            ),
        );
        assert_eq!(lines(&t, &l)[1], "REQ-9 | x | b.rs:3");
    }

    #[test]
    fn state_is_saved_and_a_bad_one_leaves_the_default() {
        let mut t = Trace::default();
        t.restore(&json!({"kind": "task", "gaps_only": true}));
        assert_eq!(t.save(), json!({"kind": "task", "gaps_only": true}));
        t.restore(&json!("junk"));
        assert_eq!(t.save(), json!({"kind": null, "gaps_only": false}));
    }

    #[test]
    fn two_thousand_items_stay_linear() {
        let l = crate::pages::frame::tests::big();
        let items: Vec<Value> = (0..2000)
            .map(|n| {
                json!({"id": format!("REQ-{n:04}"), "title": "t", "status": "draft",
                       "coverage": {"verifies": ["a.rs:1"]}, "gaps": ["implements"]})
            })
            .collect();
        let t = with(
            &json!({"kiln_json": 1, "type": "requirement", "roles": ["implements", "verifies"],
                    "items": items})
            .to_string(),
        );
        let start = std::time::Instant::now();
        assert_eq!(lines(&t, &l).len(), 2001);
        assert!(start.elapsed().as_millis() < 500);
    }

    /// The fake's answers for the Trace tab beside the demo's.
    fn trace_demo() -> Fake {
        let fake = demo();
        fake.on_spawn(
            "kiln trace --format json",
            Says::ok(fixture("demo/trace.json")),
        )
        .on_spawn(
            "kiln trace --type decision --format json",
            Says::ok(
                r#"{"kiln_json":1,"type":"decision","roles":[],"items":[{"coverage":{},"gaps":[],"id":"ADR-001","status":"accepted","title":"Store readings in SQLite"}]}"#,
            ),
        )
        .on_spawn(
            "kiln trace --type milestone --format json",
            Says::fails(2, "kiln: `milestone` isn't a type\n"),
        )
        .on_spawn(
            "kiln trace --type task --format json",
            Says::ok("not json at all"),
        )
        .on_spawn(
            "kiln trace --type test --format json",
            Says::ok(r#"{"kiln_json":1,"type":"test","roles":[],"items":[]}"#),
        )
        .on_spawn(
            "kiln trace --type risk --format json",
            Says::ok(fixture("demo/trace-risk.json")),
        );
        fake
    }

    #[test]
    fn the_tab_shows_the_demos_coverage_and_m_and_t_ask_again() {
        let fake = trace_demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "2");
        assert_snapshot!("trace-demo", h.screen(page));
        assert_eq!(at(&h, page).as_deref(), Some("REQ-001"));
        h.keys(page, "m");
        assert_snapshot!("trace-demo-gaps", h.screen(page));
        assert!(
            foot(&mut h, page).contains("m all items"),
            "{}",
            foot(&mut h, page)
        );
        h.keys(page, "m");
        // A type no role reaches, kiln's refusal, an answer that isn't JSON.
        h.keys(page, "T");
        assert_snapshot!("trace-demo-decision", h.screen(page));
        h.keys(page, "T");
        assert!(
            h.screen(page).contains("kiln: `milestone` isn't a type"),
            "{}",
            h.screen(page)
        );
        assert_snapshot!("trace-demo-error", h.screen(page));
        h.keys(page, "T");
        assert!(
            h.screen(page)
                .contains("kiln trace: kiln's JSON can't be read"),
            "{}",
            h.screen(page)
        );
        h.keys(page, "TTT");
        assert!(
            h.screen(page).contains("REQ-002"),
            "back to the default: {}",
            h.screen(page)
        );
    }

    #[test]
    fn kilns_own_trace_fits_the_page() {
        let fake = own();
        fake.on_spawn(
            "kiln trace --format json",
            Says::ok(fixture("own/trace.json")),
        );
        let (mut h, page) = opened(&fake);
        h.size = (110, 40);
        h.keys(page, "2");
        assert_snapshot!("trace-own", h.screen(page));
    }

    #[test]
    fn enter_opens_the_item_and_i_hands_it_to_impact() {
        let fake = trace_demo();
        fake.on_spawn(
            "kiln impact REQ-001 --format json",
            Says::ok(fixture("demo/impact-REQ-001.json")),
        );
        let (mut h, page) = opened(&fake);
        h.keys(page, "2i");
        assert!(h.screen(page).contains("REQ-001"), "{}", h.screen(page));
    }
}
