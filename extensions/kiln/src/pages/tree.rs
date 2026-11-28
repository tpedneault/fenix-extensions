//! Tree: every open item, nested by `parent` under a heading per type, in
//! the order kiln lists the types -- `REQ-014  Title  approved`, children
//! indented under their parent. Closed items are hidden until `a`.
//!
//! The worked example for a tab: a [`View`] whose state is nothing of its
//! own (the frame keeps the cursor, the filter and `a`), building its rows
//! from the loaded project alone. Copy its shape: rows in one pass, an
//! index built once instead of a search per item, no host or kiln calls.
//!
//! What it deliberately doesn't do: ask kiln anything (`list` has it all),
//! or show links (Trace, Impact and Item do).

use std::collections::HashMap;

use super::view::{Cell, Ctx, Row, View};
use crate::model::{count, Item};

/// The Tree tab. No state: what it shows is the project and `a`.
#[derive(Debug, Default)]
pub struct Tree;

impl View for Tree {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let items = &ctx.loaded.items;
        // `a` off: closed items aren't shown, and neither is their place
        // as a parent (an open child of a closed item goes to the top).
        let shown = |i: &Item| ctx.closed || !i.closed;

        // Each shown item's index by ID (its first file when an ID is
        // duplicated: kiln check names both), and each one's children:
        // built once, so nesting is linear in the project, not quadratic.
        let mut at: HashMap<&str, usize> = HashMap::new();
        for (n, item) in items.iter().enumerate().filter(|(_, i)| shown(i)) {
            at.entry(item.id.as_str()).or_insert(n);
        }
        let parent_of = |item: &Item| {
            item.parent
                .as_deref()
                .filter(|p| *p != item.id)
                .and_then(|p| at.get(p).copied())
        };
        let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
        // The types in kiln's order (it lists them type by type), each
        // with its top-level items.
        let mut kinds: Vec<(&str, Vec<usize>)> = Vec::new();
        for (n, item) in items.iter().enumerate().filter(|(_, i)| shown(i)) {
            match parent_of(item) {
                Some(p) => children.entry(p).or_default().push(n),
                None => match kinds.iter_mut().find(|(k, _)| *k == item.kind) {
                    Some((_, roots)) => roots.push(n),
                    None => kinds.push((item.kind.as_str(), vec![n])),
                },
            }
        }

        let mut rows = Vec::new();
        // Whether an item has a row yet: a `parent` loop (a K-finding of
        // kiln check's) can't nest forever.
        let mut placed = vec![false; items.len()];
        for (kind, roots) in &kinds {
            let heading = rows.len();
            rows.push(Row::heading(vec![]));
            // Depth first, without recursion: a long chain of parents
            // mustn't run out of stack.
            let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&n| (n, 0)).collect();
            while let Some((n, depth)) = stack.pop() {
                if std::mem::replace(&mut placed[n], true) {
                    continue;
                }
                rows.push(row(ctx, &items[n]).depth(depth));
                if let Some(kids) = children.get(&n) {
                    stack.extend(kids.iter().rev().map(|&k| (k, depth + 1)));
                }
            }
            // The heading says how many rows it has, nested ones included.
            let under = rows.len() - heading - 1;
            rows[heading] =
                Row::heading(vec![Cell::title(*kind), Cell::muted(count(under, "item"))]);
        }
        // Items only a loop of parents reaches: at the top of their type's
        // group would be a guess, so after everything, under their own
        // heading, where kiln check's finding about them says why.
        let looped: Vec<usize> = (0..items.len())
            .filter(|&n| shown(&items[n]) && !placed[n])
            .collect();
        if !looped.is_empty() {
            rows.push(Row::heading(vec![
                Cell::title("in a loop of parents"),
                Cell::muted(count(looped.len(), "item")),
            ]));
            rows.extend(looped.iter().map(|&n| row(ctx, &items[n])));
        }
        rows
    }

    fn hides_closed(&self) -> bool {
        true
    }

    fn empty(&self) -> String {
        "no items: kiln new adds one".into()
    }
}

/// An item's row: its ID, title and status.
fn row(ctx: &Ctx, item: &Item) -> Row {
    Row::item(
        item,
        vec![
            Cell::id(&item.id),
            Cell::text(&item.title),
            ctx.status(item),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;

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
        Tree.rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}{}", "  ".repeat(r.depth), text.join(" | "))
            })
            .collect()
    }

    fn item(id: &str, parent: Option<&str>, closed: bool) -> Item {
        crate::model::list(&format!(
            r#"[{{"id":"{id}","type":"task","title":"t","status":"{}","parent":{},"closed":{closed}}}]"#,
            if closed { "dropped" } else { "todo" },
            parent.map_or("null".into(), |p| format!("\"{p}\""))
        ))
        .unwrap()
        .remove(0)
    }

    fn of(items: Vec<Item>) -> Loaded {
        Loaded::new(
            crate::model::Version::parse("kiln 1.0.0").unwrap(),
            items,
            Err("none".into()),
            Ok(vec![]),
        )
    }

    #[test]
    fn children_nest_under_parents_and_closed_ones_wait_for_a() {
        let l = of(vec![
            item("T-1", None, false),
            item("T-2", Some("T-1"), false),
            item("T-3", Some("T-2"), false),
            item("T-4", None, true),
            item("T-5", Some("T-4"), false),
        ]);
        assert_eq!(
            rows(&l, false),
            [
                "# task | 4 items",
                "T-1 | t | todo",
                "  T-2 | t | todo",
                "    T-3 | t | todo",
                "T-5 | t | todo"
            ]
        );
        assert_eq!(
            rows(&l, true)[4..],
            ["T-4 | t | dropped", "  T-5 | t | todo"]
        );
    }

    #[test]
    fn a_loop_of_parents_is_shown_once_apart() {
        let l = of(vec![
            item("T-1", Some("T-2"), false),
            item("T-2", Some("T-1"), false),
            item("T-3", Some("T-3"), false),
        ]);
        assert_eq!(
            rows(&l, false),
            [
                "# task | 1 item",
                "T-3 | t | todo",
                "# in a loop of parents | 2 items",
                "T-1 | t | todo",
                "T-2 | t | todo"
            ]
        );
    }
}
