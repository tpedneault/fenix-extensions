//! Roadmap: milestones in `due` order (undated last), each with its status,
//! due date and own progress (`3/5 ██████░░░░`), and the open work that is
//! part of it under it, with the Board's mark: `ready`, `waits on TASK-003`,
//! `blocked`, `in progress`, and `· owner` when someone has claimed it. Work
//! in no milestone comes last, under its own heading. Kiln's GUI has the
//! same page ("Roadmap").
//!
//! Everything about the plan is read from `kiln status`
//! (`ctx.loaded.groups()`), never recomputed here: the progress, and which
//! work is in progress, ready or waiting, is the very number and the very
//! lists `kiln status` and `kiln next` print (ADR-018, ADR-020). Only the
//! order and the rows' text are this tab's own.
//!
//! A milestone's row is an item row, not a heading: `Enter` opens its file,
//! `v` and `i` work on it, and the filter keeps it when work under it
//! matches. The work under it is one level in, in the groups' order: in
//! progress, then ready, then waiting.
//!
//! Which milestones: `kiln status` lists those that are open or still have
//! open work, and so does this tab. `a` adds the others -- done or closed
//! milestones, from `kiln list` -- for the history. No group gives their
//! numbers (`kiln status` doesn't list them) and working them out here would
//! be a second answer to a question kiln already answers, so they have no
//! progress, only their status and due date. An open milestone no group
//! names (only when `kiln status` failed, and then a note says why) is
//! shown the same way.
//!
//! Done work is not listed: the groups' lists hold only what is still to do,
//! and the progress already says how much is done (`3/5`: three done). The
//! Tree and the Board have the done items themselves. A sub-task stands
//! where kiln puts it, under its parent's milestone, not nested.
//!
//! What it deliberately doesn't do: ask kiln anything (`list` and `status`
//! have it all), or edit (the keys that set a status come with E2).

use std::collections::HashSet;

use super::view::{Cell, Ctx, Look, Meaning, Row, View};
use crate::model::{count, Group};

/// How wide a progress bar is: Kiln's own GUI draws ten.
const BAR: usize = 10;

/// The Roadmap tab. No state: what it shows is the project and `a`.
#[derive(Debug, Default)]
pub struct Roadmap;

/// A milestone to show: what its row says, and its group when `kiln status`
/// has one.
struct Entry<'a> {
    id: &'a str,
    title: &'a str,
    status: &'a str,
    due: Option<&'a str>,
    group: Option<&'a Group>,
}

impl View for Roadmap {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Err(why) = &ctx.loaded.plan {
            rows.push(Row::note(format!(
                "no plan: {why} (milestones are shown without progress or work)"
            )));
        }

        // The milestones kiln's plan lists, then the others from `list`
        // (a set built once): open ones always, closed ones on `a`.
        let mut entries: Vec<Entry> = Vec::new();
        let mut listed: HashSet<&str> = HashSet::new();
        for group in ctx.loaded.groups() {
            if let Some(m) = &group.milestone {
                listed.insert(m.id.as_str());
                entries.push(Entry {
                    id: &m.id,
                    title: &m.title,
                    status: &m.status,
                    due: m.due.as_deref(),
                    group: Some(group),
                });
            }
        }
        for item in ctx.loaded.items.iter().filter(|i| i.kind == "milestone") {
            let over = item.done || item.closed;
            if (over && !ctx.closed) || !listed.insert(item.id.as_str()) {
                continue;
            }
            entries.push(Entry {
                id: &item.id,
                title: &item.title,
                status: &item.status,
                due: item.due.as_deref(),
                group: None,
            });
        }
        // Dated first, by date, then the undated; ties by ID, so the order
        // never depends on how kiln happened to list them.
        entries.sort_by_cached_key(|e| (e.due.is_none(), e.due.map(str::to_string), natural(e.id)));

        for e in &entries {
            rows.push(milestone_row(ctx, e));
            if let Some(group) = e.group {
                rows.extend(work_rows(ctx, group, 1));
            }
        }

        // Work in no milestone, last, as `kiln status` lists it.
        if let Some(group) = ctx.loaded.group(None) {
            let work = work_rows(ctx, group, 0);
            if !work.is_empty() {
                rows.push(Row::heading(vec![
                    Cell::title("no milestone"),
                    Cell::muted(count(work.len(), "item")),
                ]));
                rows.extend(work);
            }
        }
        rows
    }

    fn hides_closed(&self) -> bool {
        true
    }

    fn empty(&self) -> String {
        "no open milestones: a shows the done ones, kiln new milestone adds one".into()
    }
}

/// A milestone's row: ID, title, status, its due date, and its progress
/// when a group gives it.
fn milestone_row(ctx: &Ctx, e: &Entry) -> Row {
    let mut cells = vec![
        Cell::id(e.id),
        Cell::text(e.title),
        ctx.status_of(e.id, e.status),
        match e.due {
            Some(due) => Cell::text(format!("due {due}")),
            None => Cell::muted("undated"),
        },
    ];
    if let Some(g) = e.group {
        cells.push(progress(g));
    }
    ctx.row(e.id, cells)
}

/// `3/5 ███░░░░░░░`, as `kiln status` counts it; `no work` for a milestone
/// nothing is part of.
fn progress(g: &Group) -> Cell {
    if g.total == 0 {
        return Cell::muted("no work");
    }
    let look = if g.done >= g.total {
        Look::Good
    } else {
        Look::Text
    };
    Cell::new(
        format!("{}/{} {}", g.done, g.total, bar(g.done, g.total)),
        look,
    )
}

/// A bar [`BAR`] cells wide, rounded to the nearest cell, never past full.
fn bar(done: u32, total: u32) -> String {
    let (done, total, width) = (u64::from(done), u64::from(total), BAR as u64);
    let full = ((done * width + total / 2) / total.max(1)).min(width) as usize;
    format!("{}{}", "█".repeat(full), "░".repeat(BAR - full))
}

/// The open work of `group`, one row each at `depth`: in progress, ready,
/// then waiting.
fn work_rows(ctx: &Ctx, group: &Group, depth: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut work = |id: &str, title: &str, status: &str, owner: &Option<String>, mark: Cell| {
        let mut cells = vec![
            Cell::id(id),
            Cell::text(title),
            ctx.status_of(id, status),
            mark,
        ];
        if let Some(owner) = owner {
            cells.push(Cell::muted(format!("· {owner}")));
        }
        rows.push(ctx.row(id, cells).depth(depth));
    };
    for w in &group.in_progress {
        let mark = Cell::new("in progress", Look::Status(Meaning::InProgress));
        work(&w.id, &w.title, &w.status, &w.owner, mark);
    }
    for w in &group.ready {
        let mark = Cell::new("ready", Look::Good);
        work(&w.id, &w.title, &w.status, &w.owner, mark);
    }
    for w in &group.waiting {
        let waits: Vec<&str> = w.waits_on.iter().map(|r| r.id.as_str()).collect();
        let mark = match (w.blocked, waits.is_empty()) {
            (true, true) => Cell::new("blocked", Look::Bad),
            (true, false) => {
                Cell::new(format!("blocked, waits on {}", waits.join(", ")), Look::Bad)
            }
            (false, _) => Cell::new(format!("waits on {}", waits.join(", ")), Look::Warn),
        };
        work(&w.id, &w.title, &w.status, &w.owner, mark);
    }
    rows
}

/// An ID as (letters, number, whole), so `MS-9` comes before `MS-10`.
fn natural(id: &str) -> (String, u64, String) {
    let at = id.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
    let (head, tail) = id.split_at(at);
    (head.to_string(), tail.parse().unwrap_or(0), id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture};
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;
    use crate::tests::{at, demo, foot, opened, own};
    use fenix_test_kit::assert_snapshot;

    fn loaded(dir: &str) -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture(&format!("{dir}/list.json"))).unwrap(),
            model::status(&fixture(&format!("{dir}/status.json"))),
            model::check(&fixture(&format!("{dir}/check.json"))),
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
        Roadmap
            .rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = match r.kind {
                    Kind::Heading => "# ",
                    Kind::Note => "note: ",
                    _ => "",
                };
                format!("{mark}{}{}", "  ".repeat(r.depth), text.join(" | "))
            })
            .collect()
    }

    #[test]
    fn the_demo_has_its_milestones_in_due_order_with_kilns_numbers() {
        let l = loaded("demo");
        assert_eq!(
            rows(&l, false),
            [
                "MS-001 | First release | active | due 2030-06-30 | 1/5 ██░░░░░░░░",
                "  TASK-002 | Write readings to SQLite | doing | in progress | · ana",
                "  TASK-003 | Export the log as CSV | todo | ready | · bo",
                "  TASK-007 | Batch the writes | todo | ready",
                "  TASK-004 | Add the export button | todo | waits on TASK-003",
                "MS-002 | Second release | planned | undated | 0/2 ░░░░░░░░░░",
                "  TASK-006 | Draft the user guide | todo | ready",
                "  TASK-005 | Ask about retention rules | blocked | blocked",
                "# no milestone | 1 item",
                "TASK-008 | Tidy the build scripts | todo | ready",
            ]
        );
        // Nothing closed to show: `a` changes nothing.
        assert_eq!(rows(&l, true), rows(&l, false));
        // The progress is kiln's, not counted again.
        let g = l.group(Some("MS-001")).unwrap();
        assert_eq!((g.done, g.total), (1, 5));
    }

    #[test]
    fn a_milestone_row_is_an_item_and_the_work_under_it_too() {
        let l = loaded("demo");
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        let rows = Roadmap.rows(&ctx);
        assert_eq!(rows[0].kind, Kind::Item);
        assert_eq!(rows[0].id.as_deref(), Some("MS-001"));
        assert!(rows[0].place.is_some(), "Enter opens the milestone's file");
        assert_eq!(rows[1].id.as_deref(), Some("TASK-002"));
        assert_eq!(rows[1].depth, 1);
        assert!(rows[1].selectable());
        // The heading of the work in no milestone is passed over.
        let heading = rows.iter().position(|r| r.kind == Kind::Heading).unwrap();
        assert_eq!(rows[heading + 1].depth, 0);
    }

    #[test]
    fn kilns_own_project_has_no_open_milestones_until_a() {
        let l = loaded("own");
        assert!(rows(&l, false).is_empty(), "{:?}", rows(&l, false));
        let all = rows(&l, true);
        assert_eq!(all.len(), 21, "MS-001..MS-021, all done");
        assert!(all[0].starts_with("MS-001 | "), "{}", all[0]);
        assert!(all[0].ends_with("| done | undated"), "{}", all[0]);
        assert!(all[8].starts_with("MS-009 | "), "natural order: {}", all[8]);
        assert!(all[20].starts_with("MS-021 | "), "{}", all[20]);
    }

    #[test]
    fn a_milestone_with_no_work_says_so_and_a_full_bar_is_good() {
        let mut l = loaded("demo");
        let mut status = l.plan.clone().unwrap();
        status.milestones[0].done = 5;
        status.milestones[1].total = 0;
        l.plan = Ok(status);
        let r = rows(&l, false);
        assert!(r[0].ends_with("5/5 ██████████"), "{}", r[0]);
        assert_eq!(bar(1, 3), "███░░░░░░░");
        assert_eq!(bar(2, 3), "███████░░░");
        assert_eq!(bar(9, 3), "██████████", "past full stays full");
        assert_eq!(bar(0, 0), "░░░░░░░░░░");
        let at = r.iter().position(|s| s.starts_with("MS-002")).unwrap();
        assert!(r[at].ends_with("undated | no work"), "{}", r[at]);
    }

    #[test]
    fn without_a_plan_the_milestones_show_with_the_reason() {
        let mut l = loaded("demo");
        l.plan = Err("kiln status: it broke".into());
        assert_eq!(
            rows(&l, false),
            [
                "note: no plan: kiln status: it broke (milestones are shown without progress or work)",
                "MS-001 | First release | active | due 2030-06-30",
                "MS-002 | Second release | planned | undated",
            ]
        );
    }

    #[test]
    fn dated_milestones_come_first_whatever_their_ids() {
        let l = loaded("demo");
        let mut items = l.items.clone();
        for i in &mut items {
            match i.id.as_str() {
                "MS-001" => i.due = None,
                "MS-002" => i.due = Some("2031-01-01".into()),
                _ => {}
            }
        }
        let mut status = l.plan.clone().unwrap();
        for g in &mut status.milestones {
            if let Some(m) = &mut g.milestone {
                if m.id == "MS-001" {
                    m.due = None;
                } else {
                    m.due = Some("2031-01-01".into());
                }
            }
        }
        let l = Loaded::new(l.version, items, Ok(status), Ok(vec![]));
        let r = rows(&l, false);
        assert!(r[0].starts_with("MS-002 | "), "{}", r[0]);
        assert!(r.iter().any(|s| s.starts_with("MS-001 | ")));
        assert!(natural("MS-9") < natural("MS-10"));
    }

    #[test]
    fn a_project_with_no_milestones_says_so() {
        let l = Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            vec![],
            Err("none".into()),
            Ok(vec![]),
        );
        assert_eq!(rows(&l, false).len(), 1, "only the note about the plan");
        assert!(Roadmap.empty().contains("kiln new milestone"));
        assert!(Roadmap.hides_closed());
        assert!(Roadmap.keys().is_empty());
    }

    #[test]
    fn two_thousand_milestones_stay_linear() {
        let l = loaded("demo");
        let mut status = l.plan.clone().unwrap();
        let g = status.milestones[0].clone();
        let mut items = l.items.clone();
        for n in 0..2_000 {
            let mut g = g.clone();
            let m = g.milestone.as_mut().unwrap();
            m.id = format!("MS-{}", 100 + n);
            status.milestones.push(g);
            let mut item = items.iter().find(|i| i.id == "MS-001").unwrap().clone();
            item.id = format!("MS-{}", 100 + n);
            items.push(item);
        }
        let l = Loaded::new(l.version, items, Ok(status), Ok(vec![]));
        let started = std::time::Instant::now();
        assert!(rows(&l, true).len() > 10_000);
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn the_roadmap_page_of_the_demo() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "5");
        assert_snapshot!("roadmap-demo", h.screen(page));
        assert_snapshot!("roadmap-demo-marked", h.marked(page));
        // The cursor starts on the milestone: it's selectable.
        assert_eq!(at(&h, page).as_deref(), Some("MS-001"));
        h.keys(page, "j");
        assert_eq!(at(&h, page).as_deref(), Some("TASK-002"));
        // `a` has no closed milestone to add here.
        h.keys(page, "a");
        assert!(!h.screen(page).contains("MS-003"));
        assert!(foot(&mut h, page).contains("closed items shown"));
        // Enter on the milestone opens its file.
        h.keys(page, "gg");
        h.keys(page, "<Enter>");
        let opened = fake.take();
        assert!(
            opened
                .iter()
                .any(|l| l.starts_with("open /project/design/milestones/MS-001")),
            "{opened:?}"
        );
    }

    #[test]
    fn the_roadmap_page_of_kilns_own_project_shows_closed_milestones_on_a() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.size = (110, 30);
        h.keys(page, "5");
        assert_snapshot!("roadmap-own", h.screen(page));
        h.keys(page, "a");
        assert_snapshot!("roadmap-own-all", h.screen(page));
        assert!(h.screen(page).contains("MS-021"));
        h.keys(page, "a");
        assert!(!h.screen(page).contains("MS-021"));
    }
}
