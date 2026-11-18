//! Board: the work items grouped under a heading per status, each heading
//! with its count; open work says `ready` or `waits on TASK-003`, and
//! `· owner` when claimed ([`super::shared`], the Roadmap's marks too).
//! Closed statuses (`dropped`) are hidden until `a`; done ones are shown,
//! as Kiln's GUI shows them (GUI.md, "Pages").
//!
//! Which items are work. Kiln's GUI shows type `task`, else every type
//! with `done` statuses except milestones. The loaded project doesn't hold
//! the types' definitions, so this decides from what it does hold: when
//! any item is of type `task`, those; otherwise the types (not
//! `milestone`) that have an item whose status is `done`, or whose items
//! `kiln status` names as work (ready, in progress or waiting). What it
//! can't know: a type that has `done` statuses but no item at one yet and
//! no work in the plan is left out, until one is.
//!
//! The order of the statuses: the type's own, from `kiln.toml`, when the
//! extension could read it ([`crate::config`], through
//! [`super::view::Looks::order`]). Without it -- `list` doesn't give a
//! type's statuses in order -- by meaning ([`Meaning`]: initial, in
//! progress, blocked, done, closed), then by name within a meaning, so two
//! in-progress statuses (`doing`, `review`) come in alphabetical order. A
//! status `kiln.toml` doesn't list (kiln's check reports it) comes after
//! the type's, by meaning.
//!
//! What it deliberately doesn't do: recompute ready or waiting (it takes
//! them from `kiln status`'s groups, [`crate::project::Loaded::groups`],
//! the definition `kiln next` shares), ask kiln anything, or move an item
//! between statuses (`H`/`L` are the frame's, [`crate::edit`]).

use std::collections::{HashMap, HashSet};

use super::shared::{self, Mark};
use super::view::{Cell, Ctx, Look, Row, View};
use crate::model::{count, Item};

/// The Board tab. No state of its own: what it shows is the project and `a`.
#[derive(Debug, Default)]
pub struct Board;

/// One status of one type, with its items in kiln's order.
struct Bucket<'a> {
    kind: &'a str,
    status: &'a str,
    items: Vec<&'a Item>,
}

impl View for Board {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let marks = shared::marks(ctx.loaded);
        let kinds = work_kinds(ctx);
        let many = kinds.len() > 1;

        // Work, by (type, status): built in one pass over the items, in
        // kiln's order, so each bucket keeps it and nothing is searched.
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut at: HashMap<(&str, &str), usize> = HashMap::new();
        for item in &ctx.loaded.items {
            if !kinds.contains(item.kind.as_str()) || (!ctx.closed && item.closed) {
                continue;
            }
            let n = *at
                .entry((item.kind.as_str(), item.status.as_str()))
                .or_insert_with(|| {
                    buckets.push(Bucket {
                        kind: item.kind.as_str(),
                        status: item.status.as_str(),
                        items: Vec::new(),
                    });
                    buckets.len() - 1
                });
            buckets[n].items.push(item);
        }

        // Types in kiln's order (as the items first show them), then
        // statuses in the type's order when it's known, else by meaning,
        // then name.
        let mut first_of_kind: HashMap<&str, usize> = HashMap::new();
        for (n, b) in buckets.iter().enumerate() {
            first_of_kind.entry(b.kind).or_insert(n);
        }
        let meaning = |b: &Bucket| {
            let item = b.items[0];
            ctx.looks
                .meaning(&item.kind, &item.status, item.done, item.closed)
        };
        let mut order: Vec<usize> = (0..buckets.len()).collect();
        order.sort_by_key(|&n| {
            let b = &buckets[n];
            (
                first_of_kind[b.kind],
                ctx.looks.order(b.kind, b.status).unwrap_or(usize::MAX),
                meaning(b).rank(),
                b.status,
            )
        });

        let mut rows = Vec::new();
        for n in order {
            let b = &buckets[n];
            let name = if many {
                format!("{} {}", b.kind, b.status)
            } else {
                b.status.to_string()
            };
            rows.push(Row::heading(vec![
                Cell::new(name, Look::Status(meaning(b))),
                Cell::muted(count(b.items.len(), "item")),
            ]));
            rows.extend(b.items.iter().map(|i| row(i, marks.get(i.id.as_str()))));
        }
        rows
    }

    fn hides_closed(&self) -> bool {
        true
    }

    fn empty(&self) -> String {
        "no work items: a shows closed ones, kiln new adds one".into()
    }
}

/// The types that are work (see the module header): with no `task`, the
/// types with an item done, or one `kiln status` lists as work.
fn work_kinds<'a>(ctx: &'a Ctx) -> HashSet<&'a str> {
    let items = &ctx.loaded.items;
    if items.iter().any(|i| i.kind == "task") {
        return HashSet::from(["task"]);
    }
    let planned: HashSet<&str> = ctx
        .loaded
        .groups()
        .iter()
        .flat_map(|g| {
            g.in_progress
                .iter()
                .chain(&g.ready)
                .map(|w| w.id.as_str())
                .chain(g.waiting.iter().map(|w| w.id.as_str()))
        })
        .collect();
    items
        .iter()
        .filter(|i| i.kind != "milestone" && (i.done || planned.contains(i.id.as_str())))
        .map(|i| i.kind.as_str())
        .collect()
}

/// An item's row: ID, title, where it stands in the plan, its owner. The
/// status is its heading's, so it isn't said again.
fn row(item: &Item, mark: Option<&Mark>) -> Row {
    Row::item(
        item,
        vec![
            Cell::id(&item.id),
            Cell::text(&item.title),
            Mark::cell(mark),
            shared::owner(item.owner.as_deref()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, Config};
    use crate::model::{self, tests::fixture};
    use crate::pages::view::{Kind, Looks, Meaning};
    use crate::project::Loaded;
    use crate::tests::{at, demo, foot, opened, own};
    use fenix_test_kit::assert_snapshot;

    fn load(project: &str) -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture(&format!("{project}/list.json"))).unwrap(),
            model::status(&fixture(&format!("{project}/status.json"))),
            model::check(&fixture(&format!("{project}/check.json"))),
        )
    }

    fn rows(l: &Loaded, closed: bool) -> Vec<String> {
        rows_with(l, closed, None)
    }

    fn rows_with(l: &Loaded, closed: bool, config: Option<&Config>) -> Vec<String> {
        let looks = Looks::with(l, config);
        let ctx = Ctx {
            loaded: l,
            closed,
            reading: false,
            changes: 0,
            config: Err("not read in this test"),
            publishing: None,
            looks: &looks,
        };
        Board
            .rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}", text.join(" | "))
            })
            .collect()
    }

    #[test]
    fn the_demo_by_status_with_what_each_waits_on() {
        let l = load("demo");
        assert_eq!(
            rows(&l, false),
            [
                "# todo | 5 items",
                "TASK-003 | Export the log as CSV | ready | · bo",
                "TASK-004 | Add the export button | waits on TASK-003 | ",
                "TASK-006 | Draft the user guide | ready | ",
                "TASK-007 | Batch the writes | ready | ",
                "TASK-008 | Tidy the build scripts | ready | ",
                "# doing | 1 item",
                "TASK-002 | Write readings to SQLite |  | · ana",
                "# blocked | 1 item",
                "TASK-005 | Ask about retention rules |  | ",
                "# done | 1 item",
                "TASK-001 | Set up the logger |  | ",
            ],
            "blocked and in progress say nothing their heading says"
        );
        // Nothing closed in the demo: `a` adds nothing.
        assert_eq!(rows(&l, true), rows(&l, false));
    }

    #[test]
    fn a_shows_closed_statuses_last() {
        let mut l = load("demo");
        let mut items = l.items.clone();
        for i in &mut items {
            if i.id == "TASK-008" {
                i.status = "dropped".into();
                i.closed = true;
            }
        }
        l = Loaded::new(l.version, items, l.plan, l.findings);
        assert!(!rows(&l, false).iter().any(|r| r.contains("TASK-008")));
        let all = rows(&l, true);
        assert_eq!(
            all[all.len() - 2..],
            [
                "# dropped | 1 item",
                "TASK-008 | Tidy the build scripts | ready | "
            ],
        );
    }

    #[test]
    fn kilns_own_project_has_only_done_work() {
        let l = load("own");
        let r = rows(&l, false);
        assert_eq!(r.len(), 22);
        assert_eq!(r[0], "# done | 21 items");
        assert_eq!(rows(&l, true), r, "nothing closed");
        assert_eq!(
            Board.empty(),
            "no work items: a shows closed ones, kiln new adds one"
        );
    }

    #[test]
    fn without_tasks_the_types_with_done_statuses_are_the_work() {
        let list = r#"[
          {"id":"S-1","type":"story","title":"a","status":"new","done":false,"closed":false},
          {"id":"S-2","type":"story","title":"b","status":"shipped","done":true,"closed":false},
          {"id":"S-3","type":"story","title":"c","status":"building","owner":"zed","done":false,"closed":false},
          {"id":"R-1","type":"risk","title":"r","status":"open","done":false,"closed":false},
          {"id":"M-1","type":"milestone","title":"m","status":"done","done":true,"closed":false}
        ]"#;
        let l = Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(list).unwrap(),
            Err("none".into()),
            Ok(vec![]),
        );
        assert_eq!(
            rows(&l, true),
            [
                "# new | 1 item",
                "S-1 | a |  | ",
                "# building | 1 item",
                "S-3 | c |  | · zed",
                "# shipped | 1 item",
                "S-2 | b |  | "
            ],
            "no risks (no done status seen), no milestones"
        );
    }

    #[test]
    fn with_kiln_toml_statuses_come_in_the_types_order() {
        let list = r#"[
          {"id":"S-1","type":"story","title":"a","status":"building","done":false,"closed":false},
          {"id":"S-2","type":"story","title":"b","status":"review","done":false,"closed":false},
          {"id":"S-3","type":"story","title":"c","status":"shipped","done":true,"closed":false},
          {"id":"S-4","type":"story","title":"d","status":"sketch","done":false,"closed":false}
        ]"#;
        let l = Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(list).unwrap(),
            Err("none".into()),
            Ok(vec![]),
        );
        let headings = |r: Vec<String>| -> Vec<String> {
            r.into_iter().filter(|r| r.starts_with("# ")).collect()
        };
        // Without it: by meaning, then name -- `building` before `review`,
        // and `sketch` isn't known to be first.
        assert_eq!(
            headings(rows(&l, false)),
            [
                "# building | 1 item",
                "# review | 1 item",
                "# sketch | 1 item",
                "# shipped | 1 item"
            ]
        );
        let c = config::parse(
            "format = 1\n[types.story]\nprefix = \"S\"\nstatuses = [\"sketch\", \"review\", \"building\", \"shipped\"]\ndone = [\"shipped\"]\n",
        )
        .unwrap();
        assert_eq!(
            headings(rows_with(&l, false, Some(&c))),
            [
                "# sketch | 1 item",
                "# review | 1 item",
                "# building | 1 item",
                "# shipped | 1 item"
            ]
        );
        // And `sketch` means first: kiln.toml's initial.
        let looks = Looks::with(&l, Some(&c));
        assert_eq!(
            looks.meaning("story", "sketch", false, false),
            Meaning::Initial
        );
        assert_eq!(
            looks.meaning("story", "shipped", false, false),
            Meaning::Done
        );
        assert_eq!(
            Looks::new(&l).meaning("story", "sketch", false, false),
            Meaning::InProgress
        );
    }

    #[test]
    fn a_plan_that_failed_marks_nothing() {
        let mut l = load("demo");
        l.plan = Err("kiln: no".into());
        let r = rows(&l, false);
        assert!(r
            .iter()
            .all(|r| !r.contains("ready") && !r.contains("waits")));
        assert_eq!(r.len(), 12);
    }

    #[test]
    fn it_hides_closed_and_has_no_keys_of_its_own() {
        assert!(Board.hides_closed());
        assert!(Board.keys().is_empty());
    }

    #[test]
    fn the_demo_page() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.size = (110, 26);
        h.keys(page, "4");
        assert_snapshot!("board-demo", h.screen(page));
        // Nothing closed in the demo: `a` says so and the rows stay.
        let body = |s: String| s.lines().take(20).collect::<Vec<_>>().join("\n");
        let before = body(h.screen(page));
        h.keys(page, "a");
        assert_eq!(body(h.screen(page)), before);
        assert_eq!(foot(&mut h, page), "closed items shown");
    }

    #[test]
    fn kilns_own_page_has_its_done_tasks() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.size = (110, 26);
        h.keys(page, "4");
        assert_snapshot!("board-own", h.screen(page));
        assert_eq!(at(&h, page).as_deref(), Some("TASK-001"));
    }
}
