//! Decisions: the decision log in ID order -- `ADR-013  Title  superseded
//! superseded by ADR-024` -- with what each supersedes or is superseded by.
//! Kiln's GUI has the same page ("Decisions").
//!
//! Which items are decisions: the type named `decision` when the project has
//! items of it; otherwise the types whose items are the source of a
//! `supersedes` link (an `adr` type that says `supersedes` is a decision log
//! whatever it is called). A project with neither has no decisions, and the
//! tab says so. `kiln list` doesn't give the types, so "has the type" is
//! "has an item of it": a type with no items has nothing to log anyway.
//!
//! `supersedes` comes from the item's own outgoing `links`; `superseded by`
//! is those links read from the other end, across every loaded item (the
//! source of a `supersedes` link needn't be a decision), indexed once so
//! the tab stays linear. A target no item has (`supersedes ADR-099`) is
//! shown as written: `kiln check` says why it is wrong, not this tab.
//!
//! Closed decisions (superseded, rejected) stay: the log is the point, and
//! `a` does nothing here (Kiln's GUI doesn't hide them either).
//!
//! What it deliberately doesn't do: ask kiln anything, order by anything but
//! the ID (the log reads in the order decisions were taken), or show the
//! body or the other links (Item does).

use std::collections::{HashMap, HashSet};

use super::shared::natural;
use super::view::{Cell, Ctx, Row, View};
use crate::model::Item;

/// The link role that makes one decision replace another.
const SUPERSEDES: &str = "supersedes";

/// The type named this is the decision log.
const DECISION: &str = "decision";

/// The Decisions tab. No state: what it shows is the project.
#[derive(Debug, Default)]
pub struct Decisions;

impl View for Decisions {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let items = &ctx.loaded.items;

        // Every `supersedes` link once: target -> who supersedes it, in
        // kiln's order, and which types are the source of one.
        let mut by: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut source_types: HashSet<&str> = HashSet::new();
        for item in items {
            let Some(targets) = item.links.get(SUPERSEDES).filter(|t| !t.is_empty()) else {
                continue;
            };
            source_types.insert(item.kind.as_str());
            for target in targets {
                let list = by.entry(target.as_str()).or_default();
                if !list.contains(&item.id.as_str()) {
                    list.push(&item.id);
                }
            }
        }

        let named = items.iter().any(|i| i.kind == DECISION);
        let is_decision = |i: &&Item| {
            if named {
                i.kind == DECISION
            } else {
                source_types.contains(i.kind.as_str())
            }
        };
        let mut log: Vec<&Item> = items.iter().filter(is_decision).collect();
        log.sort_by_cached_key(|i| natural(&i.id));

        log.into_iter()
            .map(|item| {
                let mut said: Vec<String> = Vec::new();
                if let Some(targets) = item.links.get(SUPERSEDES).filter(|t| !t.is_empty()) {
                    said.push(format!("supersedes {}", targets.join(", ")));
                }
                if let Some(sources) = by.get(item.id.as_str()) {
                    said.push(format!("superseded by {}", sources.join(", ")));
                }
                let mut cells = vec![
                    Cell::id(&item.id),
                    Cell::text(&item.title),
                    ctx.status(item),
                ];
                if !said.is_empty() {
                    cells.push(Cell::muted(said.join("; ")));
                }
                Row::item(item, cells)
            })
            .collect()
    }

    fn empty(&self) -> String {
        "no decisions: no type named decision, and no item supersedes another".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture};
    use crate::pages::view::Looks;
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
            changes: 0,
            config: Err("not read in this test"),
            publishing: None,
            looks: &looks,
        };
        Decisions
            .rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                text.join(" | ")
            })
            .collect()
    }

    fn item(id: &str, kind: &str, status: &str, supersedes: &[&str]) -> Item {
        let links: Vec<String> = supersedes.iter().map(|s| format!("\"{s}\"")).collect();
        model::list(&format!(
            r#"[{{"id":"{id}","type":"{kind}","title":"t {id}","status":"{status}","links":{{"supersedes":[{}]}}}}]"#,
            links.join(",")
        ))
        .unwrap()
        .remove(0)
    }

    fn of(items: Vec<Item>) -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            items,
            Err("none".into()),
            Ok(vec![]),
        )
    }

    #[test]
    fn the_demo_has_one_decision_that_supersedes_nothing() {
        let l = loaded("demo");
        assert_eq!(
            rows(&l, false),
            ["ADR-001 | Store readings in SQLite | accepted"]
        );
        assert_eq!(rows(&l, true), rows(&l, false), "`a` changes nothing");
    }

    #[test]
    fn kilns_own_log_has_a_supersedes_chain_and_keeps_closed_decisions() {
        let l = loaded("own");
        let r = rows(&l, false);
        assert_eq!(r.len(), 24, "ADR-001..ADR-024, closed one included");
        assert_eq!(r[0].split(" | ").next(), Some("ADR-001"));
        assert!(r[11].starts_with("ADR-012 | "), "{}", r[11]);
        // ADR-013 is superseded, and says by what; ADR-024 says the inverse.
        assert!(
            r[12].starts_with("ADR-013 | ")
                && r[12].ends_with(" | superseded | superseded by ADR-024"),
            "{}",
            r[12]
        );
        assert!(
            r[23].ends_with(" | accepted | supersedes ADR-013"),
            "{}",
            r[23]
        );
        // No other decision has a relation.
        assert_eq!(r.iter().filter(|s| s.contains("supersede")).count(), 2);
    }

    #[test]
    fn a_decision_can_be_both_and_supersedes_many() {
        let l = of(vec![
            item("ADR-010", "decision", "accepted", &["ADR-9", "ADR-2"]),
            item("ADR-9", "decision", "superseded", &["ADR-1"]),
            item("ADR-1", "decision", "superseded", &[]),
            item("ADR-2", "decision", "superseded", &[]),
            item("NOTE-1", "note", "open", &["ADR-1"]),
        ]);
        assert_eq!(
            rows(&l, false),
            [
                "ADR-1 | t ADR-1 | superseded | superseded by ADR-9, NOTE-1",
                "ADR-2 | t ADR-2 | superseded | superseded by ADR-010",
                "ADR-9 | t ADR-9 | superseded | supersedes ADR-1; superseded by ADR-010",
                "ADR-010 | t ADR-010 | accepted | supersedes ADR-9, ADR-2",
            ],
            "IDs in natural order; a link from a non-decision counts"
        );
    }

    #[test]
    fn a_target_nothing_has_is_shown_as_written() {
        let l = of(vec![item("ADR-1", "decision", "accepted", &["ADR-99"])]);
        assert_eq!(
            rows(&l, false),
            ["ADR-1 | t ADR-1 | accepted | supersedes ADR-99"]
        );
    }

    #[test]
    fn without_a_decision_type_the_sources_of_supersedes_are_the_log() {
        let l = of(vec![
            item("DEC-1", "adr", "superseded", &[]),
            item("DEC-2", "adr", "accepted", &["DEC-1"]),
            item("T-1", "task", "todo", &[]),
        ]);
        assert_eq!(
            rows(&l, false),
            [
                "DEC-1 | t DEC-1 | superseded | superseded by DEC-2",
                "DEC-2 | t DEC-2 | accepted | supersedes DEC-1",
            ],
            "the target's type is a decision too; the task is not"
        );
        // A type named decision wins over the guess.
        let l = of(vec![
            item("DEC-1", "adr", "accepted", &["DEC-0"]),
            item("D-1", "decision", "accepted", &[]),
        ]);
        assert_eq!(rows(&l, false), ["D-1 | t D-1 | accepted"]);
    }

    #[test]
    fn no_decisions_says_why() {
        let l = of(vec![item("T-1", "task", "todo", &[])]);
        assert!(rows(&l, false).is_empty());
        assert!(Decisions.empty().starts_with("no decisions"));
        assert!(!Decisions.hides_closed());
        assert!(Decisions.keys().is_empty());
    }

    #[test]
    fn two_thousand_decisions_stay_linear() {
        let items: Vec<Item> = (1..=2_000)
            .map(|n| {
                let before = format!("ADR-{}", n - 1);
                let before = [before.as_str()];
                let before: &[&str] = if n > 1 { &before } else { &[] };
                item(&format!("ADR-{n}"), "decision", "accepted", before)
            })
            .collect();
        let started = std::time::Instant::now();
        assert_eq!(rows(&of(items), false).len(), 2_000);
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn the_decisions_page_of_the_demo() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "6");
        assert_snapshot!("decisions-demo", h.screen(page));
        assert_eq!(at(&h, page).as_deref(), Some("ADR-001"));
        // `a` is the frame's and does nothing here.
        h.keys(page, "a");
        assert_eq!(foot(&mut h, page), "Decisions hides no closed items");
        fake.take();
        h.keys(page, "<Enter>");
        assert_eq!(
            fake.take(),
            ["open /project/design/decisions/ADR-001-store-readings-in-sqlite.md:2"]
        );
    }

    #[test]
    fn the_decisions_page_of_kilns_own_project() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.size = (110, 30);
        h.keys(page, "6");
        h.keys(page, "G");
        assert_eq!(at(&h, page).as_deref(), Some("ADR-024"));
        assert_snapshot!("decisions-own-end", h.screen(page));
        h.keys(page, "/013<Enter>");
        assert_snapshot!("decisions-own-filter", h.screen(page));
        // The filter matches the decision and the one that supersedes it;
        // the cursor stayed on the latter.
        assert_eq!(at(&h, page).as_deref(), Some("ADR-024"));
        h.keys(page, "k");
        assert_eq!(at(&h, page).as_deref(), Some("ADR-013"));
    }
}
