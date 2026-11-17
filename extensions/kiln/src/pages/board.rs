//! Board: the work items grouped under a heading per status, each heading
//! with its count; open work says `ready`, `waits on TASK-003` or
//! `blocked`, and `· owner` when claimed. Done and closed statuses are
//! hidden until `a`.
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
//! The order of the statuses. `list` doesn't give a type's statuses in
//! their order, and nothing in the loaded project does, so the order is by
//! meaning ([`Meaning`]: initial, in progress, blocked, done, closed), then
//! by name within a meaning. Two in-progress statuses (`doing`, `review`)
//! therefore come in alphabetical order, not the type's. A type's
//! statuses in order (`kiln list` or `show` carrying them) would fix it:
//! reported as needed from kiln.
//!
//! What it deliberately doesn't do: recompute ready or waiting (it takes
//! them from `kiln status`'s groups, [`crate::project::Loaded::groups`],
//! the definition `kiln next` shares), ask kiln anything, or move an item
//! between statuses (`H`/`L` are editing, a later task).

use std::collections::{HashMap, HashSet};

use super::view::{Cell, Ctx, Look, Meaning, Row, View};
use crate::model::{count, Item};

/// The Board tab. No state of its own: what it shows is the project and `a`.
#[derive(Debug, Default)]
pub struct Board;

/// What the plan says of one open item: how it's waiting, if it is.
enum Mark {
    Ready,
    /// In progress: its status says so, the Board adds nothing.
    Going,
    Waiting {
        blocked: bool,
        on: Vec<String>,
    },
}

/// One status of one type, with its items in kiln's order.
struct Bucket<'a> {
    kind: &'a str,
    status: &'a str,
    items: Vec<&'a Item>,
}

impl View for Board {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let marks = marks(ctx);
        let kinds = work_kinds(ctx, &marks);
        let many = kinds.len() > 1;

        // Work, by (type, status): built in one pass over the items, in
        // kiln's order, so each bucket keeps it and nothing is searched.
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut at: HashMap<(&str, &str), usize> = HashMap::new();
        for item in &ctx.loaded.items {
            if !kinds.contains(item.kind.as_str()) || (!ctx.closed && (item.done || item.closed)) {
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
        // statuses by meaning, then name.
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
            (first_of_kind[b.kind], rank(meaning(b)), b.status)
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
        "no open work: a shows what's done, kiln new adds a task".into()
    }
}

/// Where a status comes among its type's: the order of a task's life.
fn rank(m: Meaning) -> u8 {
    match m {
        Meaning::Initial => 0,
        Meaning::InProgress => 1,
        Meaning::Blocked => 2,
        Meaning::Done => 3,
        Meaning::Closed => 4,
    }
}

/// What `kiln status` says of each open item, by ID. A work item is in one
/// group; if a duplicated ID puts it in two, the first says.
fn marks<'a>(ctx: &'a Ctx) -> HashMap<&'a str, Mark> {
    let mut marks = HashMap::new();
    for g in ctx.loaded.groups() {
        for w in &g.in_progress {
            marks.entry(w.id.as_str()).or_insert(Mark::Going);
        }
        for w in &g.ready {
            marks.entry(w.id.as_str()).or_insert(Mark::Ready);
        }
        for w in &g.waiting {
            marks.entry(w.id.as_str()).or_insert(Mark::Waiting {
                blocked: w.blocked,
                on: w.waits_on.iter().map(|r| r.id.clone()).collect(),
            });
        }
    }
    marks
}

/// The types that are work (see the module header).
fn work_kinds<'a>(ctx: &'a Ctx, marks: &HashMap<&str, Mark>) -> HashSet<&'a str> {
    let items = &ctx.loaded.items;
    if items.iter().any(|i| i.kind == "task") {
        return HashSet::from(["task"]);
    }
    items
        .iter()
        .filter(|i| i.kind != "milestone" && (i.done || marks.contains_key(i.id.as_str())))
        .map(|i| i.kind.as_str())
        .collect()
}

/// An item's row: ID, title, how it waits, its owner. The status is its
/// heading's, so it isn't said again.
fn row(item: &Item, mark: Option<&Mark>) -> Row {
    let state = match mark {
        Some(Mark::Ready) => Cell::new("ready", Look::Good),
        Some(Mark::Waiting { blocked, on }) => {
            let mut parts = Vec::new();
            if *blocked {
                parts.push("blocked".to_string());
            }
            if !on.is_empty() {
                parts.push(format!("waits on {}", on.join(", ")));
            }
            let look = if *blocked { Look::Bad } else { Look::Warn };
            Cell::new(parts.join(", "), look)
        }
        Some(Mark::Going) | None => Cell::text(""),
    };
    let owner = match item.owner.as_deref() {
        Some(o) if !o.is_empty() => Cell::muted(format!("· {o}")),
        _ => Cell::text(""),
    };
    Row::item(
        item,
        vec![Cell::id(&item.id), Cell::text(&item.title), state, owner],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture};
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;
    use crate::tests::{demo, foot, opened, own};
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
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed,
            reading: false,
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
                "TASK-005 | Ask about retention rules | blocked | ",
            ]
        );
    }

    #[test]
    fn a_shows_what_is_done_last() {
        let l = load("demo");
        let all = rows(&l, true);
        assert_eq!(all.len(), 12);
        assert_eq!(
            all[10..],
            ["# done | 1 item", "TASK-001 | Set up the logger |  | "]
        );
    }

    #[test]
    fn kilns_own_project_has_no_open_work() {
        let l = load("own");
        assert!(rows(&l, false).is_empty());
        assert_eq!(
            Board.empty(),
            "no open work: a shows what's done, kiln new adds a task"
        );
        let all = rows(&l, true);
        assert_eq!(all.len(), 22);
        assert_eq!(all[0], "# done | 21 items");
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
    fn a_plan_that_failed_marks_nothing() {
        let mut l = load("demo");
        l.plan = Err("kiln: no".into());
        let r = rows(&l, false);
        assert!(r
            .iter()
            .all(|r| !r.contains("ready") && !r.contains("waits")));
        assert_eq!(r.len(), 10);
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
        h.keys(page, "a");
        assert_snapshot!("board-demo-closed", h.screen(page));
    }

    #[test]
    fn kilns_own_page_says_there_is_no_open_work() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.size = (110, 26);
        h.keys(page, "4");
        assert_snapshot!("board-own", h.screen(page));
        h.keys(page, "a");
        assert_snapshot!("board-own-closed", h.screen(page));
        assert!(!foot(&mut h, page).is_empty());
    }
}
