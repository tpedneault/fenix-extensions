//! Impact: for the item handed to it with `i` on any tab, everything that
//! reaches it by a link or a parent edge, transitively, nearest first,
//! each with its path (`verifies REQ-014`, `child of REQ-002`); code
//! locations included, and `Enter` opens them (`kiln impact ID`).
//!
//! Its first row names the item it's about, so the rest reads as "what
//! reaches this". The order is kiln's (breadth first: the nearest first),
//! kept as it is rather than re-sorted, since distance is what the order
//! says. The query's tag carries the ID, so an answer for an item handed
//! over since is never shown under the new one's name.
//!
//! What it deliberately doesn't do: pick an item itself (`i` on another
//! tab does, or `i` on one of its own rows to follow the chain), show
//! status or the whole path as rows of their own, or ask kiln anything
//! while nothing is handed to it.

use serde::Deserialize;
use serde_json::{json, Value};

use super::view::{stdout, Cell, Ctx, Look, Query, Row, View};
use crate::kiln::{Failure, Ran};
use crate::model;

/// What's in front of the tag's ID: `impact:REQ-001`.
const TAG: &str = "impact:";

/// One thing that reaches the item: where it is (an item's ID or a
/// `path:line`), and the edges from there to the item.
#[derive(Debug, Clone, Deserialize)]
struct Reach {
    source: String,
    #[serde(default)]
    path: Vec<Step>,
}

/// One edge: `from` has `relation` to `to` (`TASK-001 implements
/// REQ-001`; `parent` is a child's edge to its parent).
#[derive(Debug, Clone, Deserialize)]
struct Step {
    #[serde(default)]
    relation: String,
    #[serde(default)]
    to: String,
}

/// The Impact tab. Its state is the ID it was handed and kiln's answer
/// about one.
#[derive(Debug, Default)]
pub struct Impact {
    /// The item handed to it last.
    id: Option<String>,
    /// What kiln said about an ID: what reaches it, or its line.
    said: Option<(String, Result<Vec<Reach>, String>)>,
}

/// The path as the CLI prints it: `verifies REQ-013, child of REQ-001`.
fn path_text(path: &[Step]) -> String {
    path.iter()
        .map(|s| match s.relation.as_str() {
            "parent" => format!("child of {}", s.to),
            relation => format!("{relation} {}", s.to),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `path:line` (`crates/a/src/b.rs:58`) split, when `source` is one: an
/// item's ID never has a `:`.
fn code(source: &str) -> Option<(&str, u32)> {
    let (path, line) = source.rsplit_once(':')?;
    Some((path, line.parse().ok()?))
}

impl View for Impact {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let Some(id) = &self.id else {
            return vec![Row::note(
                "nothing picked yet: press i on an item in any tab to see what reaches it",
            )];
        };
        let heading = |tail: Cell| {
            let title = ctx.loaded.item(id).map_or("", |i| i.title.as_str());
            Row::heading(vec![Cell::title(id), Cell::text(title), tail])
        };
        let reaches = match &self.said {
            Some((of, answer)) if of == id => match answer {
                Ok(reaches) => reaches,
                Err(line) => return vec![heading(Cell::muted("")), Row::note(line.clone())],
            },
            // Handed on, and kiln hasn't answered about this one yet.
            _ => return vec![heading(Cell::muted("")), Row::note("asking kiln…")],
        };
        let mut rows = vec![heading(Cell::muted(match reaches.len() {
            0 => "nothing reaches it".to_string(),
            1 => "1 reaches it".to_string(),
            n => format!("{n} reach it"),
        }))];
        if reaches.is_empty() {
            rows.push(Row::note(format!(
                "nothing reaches {id}: no link or child points at it"
            )));
        }
        for r in reaches {
            let path = Cell::text(path_text(&r.path));
            rows.push(match code(&r.source) {
                Some((file, line)) if ctx.loaded.item(&r.source).is_none() => Row::code(
                    file,
                    line,
                    vec![Cell::new(&r.source, Look::Accent), path, Cell::muted("")],
                ),
                _ => {
                    let title = ctx.loaded.item(&r.source).map_or("", |i| i.title.as_str());
                    ctx.row(
                        &r.source,
                        vec![Cell::id(&r.source), path, Cell::muted(title)],
                    )
                }
            });
        }
        rows
    }

    /// The path gives way before the ID, and the title before the path:
    /// the last column.
    fn flex(&self) -> usize {
        2
    }

    fn empty(&self) -> String {
        "press i on an item in any tab to see what reaches it".into()
    }

    fn queries(&self, _ctx: &Ctx) -> Vec<Query> {
        self.id
            .iter()
            .map(|id| {
                Query::new(
                    &format!("{TAG}{id}"),
                    ["impact", id.as_str(), "--format", "json"],
                )
            })
            .collect()
    }

    fn answer(&mut self, tag: &str, result: &Result<Ran, Failure>) {
        let Some(of) = tag.strip_prefix(TAG) else {
            return;
        };
        let answer = stdout(result, &[0]).and_then(|t| model::array::<Reach>("kiln impact", t));
        self.said = Some((of.to_string(), answer));
    }

    fn hand(&mut self, id: &str) {
        self.id = Some(id.to_string());
    }

    fn save(&self) -> Value {
        json!({ "id": self.id })
    }

    fn restore(&mut self, saved: &Value) {
        self.id = saved["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;
    use crate::tests::{at, demo, foot, opened, own};
    use fenix_test_kit::api::Extension;
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

    fn rows(view: &Impact, l: &Loaded) -> Vec<Row> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        view.rows(&ctx)
    }

    fn lines(view: &Impact, l: &Loaded) -> Vec<String> {
        rows(view, l)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}", text.join(" | ").trim_end_matches(" | "))
            })
            .collect()
    }

    /// Handed `id`, and kiln's answer in `file`.
    fn about(id: &str, file: &str) -> Impact {
        let mut v = Impact::default();
        v.hand(id);
        v.answer(&format!("impact:{id}"), &ran(0, &fixture(file), ""));
        v
    }

    #[test]
    fn nothing_handed_says_which_key_to_press() {
        let l = loaded("demo");
        let v = Impact::default();
        assert_eq!(
            lines(&v, &l),
            ["nothing picked yet: press i on an item in any tab to see what reaches it"]
        );
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        assert!(v.queries(&ctx).is_empty(), "nothing to ask");
    }

    #[test]
    fn the_demos_req_001_is_reached_nearest_first_each_with_its_path() {
        let l = loaded("demo");
        let v = about("REQ-001", "demo/impact-REQ-001.json");
        assert_eq!(
            lines(&v, &l),
            [
                "# REQ-001 | Readings are logged every second | 5 reach it",
                "REQ-002 | child of REQ-001 | Logged readings survive a power cut",
                "TASK-001 | implements REQ-001 | Set up the logger",
                "TASK-002 | implements REQ-001 | Write readings to SQLite",
                "TEST-001 | verifies REQ-001 | Readings arrive every second",
                "TASK-007 | child of TASK-002, implements REQ-001 | Batch the writes",
            ]
        );
        let r = rows(&v, &l);
        assert_eq!(
            r[5].id.as_deref(),
            Some("TASK-007"),
            "`i` follows the chain"
        );
        assert!(r[5].place.is_some(), "Enter opens its file");
    }

    #[test]
    fn kilns_own_code_locations_open_where_they_are() {
        let l = loaded("own");
        let v = about("REQ-001", "own/impact-REQ-001.json");
        let r = rows(&v, &l);
        assert_eq!(r[1].kind, Kind::Item);
        assert_eq!(r[2].kind, Kind::Code);
        let place = r[2].place.as_ref().unwrap();
        assert_eq!(
            (place.path.as_str(), place.line),
            ("crates/kiln/tests/cli.rs", Some(957))
        );
        assert_eq!(r[2].id, None, "no item behind a place in code");
        assert_eq!(
            lines(&v, &l)[3],
            "crates/kiln-gui/src/tests.rs:723 | verifies REQ-013, child of REQ-001"
        );
    }

    #[test]
    fn an_item_nothing_reaches_says_so_and_one_through_a_parent_shows_the_path() {
        let l = loaded("demo");
        let v = about("TASK-006", "demo/impact-TASK-006.json");
        assert_eq!(
            lines(&v, &l),
            [
                "# TASK-006 | Draft the user guide | nothing reaches it",
                "nothing reaches TASK-006: no link or child points at it"
            ]
        );
        let v = about("TASK-002", "demo/impact-TASK-002.json");
        assert_eq!(
            lines(&v, &l),
            [
                "# TASK-002 | Write readings to SQLite | 1 reaches it",
                "TASK-007 | child of TASK-002 | Batch the writes"
            ]
        );
    }

    #[test]
    fn a_reload_that_lost_the_item_shows_kilns_line_and_a_stale_answer_is_not_shown() {
        let l = loaded("demo");
        let mut v = Impact::default();
        v.hand("TASK-099");
        assert_eq!(lines(&v, &l)[1], "asking kiln…");
        v.answer(
            "impact:TASK-099",
            &ran(2, "", &fixture("errors/impact-unknown-id.stderr")),
        );
        assert_eq!(
            lines(&v, &l),
            ["# TASK-099", "kiln: TASK-099 doesn't exist"]
        );
        // Handed another while the first's answer is out: under its own name.
        let mut v = about("REQ-001", "demo/impact-REQ-001.json");
        v.hand("TASK-006");
        assert_eq!(lines(&v, &l)[1], "asking kiln…");
        v.answer("impact:REQ-001", &ran(0, "[]", ""));
        assert_eq!(lines(&v, &l)[1], "asking kiln…", "not the old one's");
    }

    #[test]
    fn an_unreadable_answer_is_one_note_and_unknown_fields_are_fine() {
        let l = loaded("demo");
        let mut v = Impact::default();
        v.hand("REQ-001");
        v.answer("impact:REQ-001", &ran(0, "not json", ""));
        assert!(lines(&v, &l)[1].starts_with("kiln impact: kiln's JSON can't be read"));
        v.answer("impact:REQ-001", &ran(0, r#"{"a": 1}"#, ""));
        assert_eq!(
            lines(&v, &l)[1],
            "kiln impact: expected a JSON array, got an object"
        );
        v.answer("impact:REQ-001", &Err(Failure::TooLarge));
        assert!(lines(&v, &l)[1].contains("over"));
        v.answer(
            "impact:REQ-001",
            &ran(
                0,
                r#"[{"source":"TASK-001","extra":1,"path":[{"from":"TASK-001","relation":"implements","to":"REQ-001","x":0}]},{"source":"src/a.rs:3"}]"#,
                "",
            ),
        );
        assert_eq!(
            lines(&v, &l)[1..],
            [
                "TASK-001 | implements REQ-001 | Set up the logger",
                "src/a.rs:3"
            ]
        );
        v.answer("other", &ran(0, "[]", ""));
        assert_eq!(
            lines(&v, &l).len(),
            3,
            "a tag that isn't its own is ignored"
        );
    }

    #[test]
    fn the_handed_id_is_saved_and_a_bad_state_leaves_none() {
        let mut v = Impact::default();
        v.hand("REQ-014");
        assert_eq!(v.save(), json!({"id": "REQ-014"}));
        let mut w = Impact::default();
        w.restore(&v.save());
        assert_eq!(w.id.as_deref(), Some("REQ-014"));
        w.restore(&json!([1]));
        assert_eq!(w.id, None);
    }

    fn impact_demo() -> Fake {
        let fake = demo();
        fake.on_spawn(
            "kiln impact REQ-001 --format json",
            Says::ok(fixture("demo/impact-REQ-001.json")),
        )
        .on_spawn("kiln impact REQ-002 --format json", Says::ok("[]"))
        .on_spawn(
            "kiln impact TASK-001 --format json",
            // As recorded, for an item the project has lost since.
            Says::fails(
                2,
                fixture("errors/impact-unknown-id.stderr").replace("TASK-099", "TASK-001"),
            ),
        )
        .on_spawn("kiln impact TASK-002 --format json", Says::ok("<html>"));
        fake
    }

    #[test]
    fn i_on_a_tree_row_shows_that_items_impact() {
        let fake = impact_demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "3");
        assert_snapshot!("impact-none", h.screen(page));
        h.keys(page, "1");
        assert_eq!(at(&h, page).as_deref(), Some("REQ-001"));
        h.keys(page, "i");
        assert_snapshot!("impact-demo", h.screen(page));
        // Another item, picked from the Tree: its answer is `[]`, an item
        // nothing reaches.
        h.keys(page, "1ji");
        assert_eq!(at(&h, page), None, "{}", h.screen(page));
        assert_snapshot!("impact-demo-nothing", h.screen(page));
    }

    #[test]
    fn kiln_errors_and_unreadable_answers_are_notes_on_the_page() {
        let fake = impact_demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "1/TASK-001<Enter>i");
        assert!(h.screen(page).contains("TASK-001"), "{}", h.screen(page));
        assert!(h.screen(page).contains("kiln: TASK-001 doesn't exist"));
        assert_snapshot!("impact-demo-error", h.screen(page));
        h.keys(page, "1/<Esc>/TASK-002<Enter>i");
        assert!(
            h.screen(page)
                .contains("kiln impact: kiln's JSON can't be read"),
            "{}",
            h.screen(page)
        );
        assert!(
            foot(&mut h, page).contains("Enter"),
            "{}",
            foot(&mut h, page)
        );
    }

    #[test]
    fn kilns_own_code_locations_are_rows_you_can_open() {
        let fake = own();
        fake.on_spawn(
            "kiln impact REQ-001 --format json",
            Says::ok(fixture("own/impact-REQ-001.json")),
        );
        let (mut h, page) = opened(&fake);
        h.size = (110, 40);
        h.keys(page, "/REQ-001<Enter>i");
        assert_snapshot!("impact-own", h.screen(page));
    }

    #[test]
    fn the_handed_item_survives_the_page_being_saved_and_restored() {
        let fake = impact_demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "1i");
        let state = h.ext.page_state(page).unwrap();
        assert!(state.contains("REQ-001"), "{state}");
        assert!(h.ext.page_restore(77, "plan", &state));
        h.settle();
        assert!(
            h.screen(77).contains("# REQ-001") || h.screen(77).contains("REQ-001  Readings"),
            "{}",
            h.screen(77)
        );
    }
}
