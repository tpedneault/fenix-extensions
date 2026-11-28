//! What several tabs say the same way, written once so they say it alike:
//! where open work stands in the plan (`ready`, `waits on TASK-003`) and
//! who claimed it (`· ana`), as the Board and the Roadmap show it; and IDs
//! in their natural order (`MS-9` before `MS-10`), as the Roadmap and
//! Decisions sort them. (Statuses ordered by what they mean are
//! [`super::view::Meaning::rank`].)
//!
//! The mark is Kiln's GUI's (`plan_badge`): `ready`, or `waits on …`
//! (`kiln next`'s definition, ADR-018), and nothing for work in progress or
//! blocked -- the item's status, beside it or its heading, says that
//! already, and saying it twice reads as two facts.
//!
//! What it deliberately doesn't do: work out ready or waiting itself (it
//! reads `kiln status`'s groups, [`crate::project::Loaded::groups`]).

use std::collections::HashMap;

use super::view::{Cell, Look};
use crate::model::Waiting;
use crate::project::Loaded;

/// Where one open item stands in the plan, when there's something to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// Nothing it depends on is open, and it hasn't started.
    Ready,
    /// The open items it depends on.
    Waits(Vec<String>),
}

impl Mark {
    /// A waiting item's mark: what it waits on, or none when only its
    /// status (`blocked`) holds it.
    pub fn of(w: &Waiting) -> Option<Mark> {
        (!w.waits_on.is_empty())
            .then(|| Mark::Waits(w.waits_on.iter().map(|r| r.id.clone()).collect()))
    }

    /// Its cell: `ready` in the good look, `waits on A, B` in the warning one.
    pub fn cell(mark: Option<&Mark>) -> Cell {
        match mark {
            Some(Mark::Ready) => Cell::new("ready", Look::Good),
            Some(Mark::Waits(on)) => Cell::new(format!("waits on {}", on.join(", ")), Look::Warn),
            None => Cell::text(""),
        }
    }
}

/// Each open item's mark, by ID, from `kiln status`'s groups: built once a
/// read, so a tab looks each item up instead of searching. A work item is
/// in one group; if a duplicated ID puts it in two, the first says.
pub fn marks(loaded: &Loaded) -> HashMap<&str, Mark> {
    let mut marks = HashMap::new();
    for g in loaded.groups() {
        for w in &g.ready {
            marks.entry(w.id.as_str()).or_insert(Mark::Ready);
        }
        for w in &g.waiting {
            if let Some(m) = Mark::of(w) {
                marks.entry(w.id.as_str()).or_insert(m);
            }
        }
    }
    marks
}

/// Who claimed it: `· ana`, muted; an empty cell when no one has, so the
/// columns after it stay in line.
pub fn owner(owner: Option<&str>) -> Cell {
    match owner {
        Some(o) if !o.is_empty() => Cell::muted(format!("· {o}")),
        _ => Cell::text(""),
    }
}

/// An ID as (letters, number, whole), so `MS-9` comes before `MS-10`, and
/// IDs that only differ in their zeros (`MS-01`, `MS-1`) by the whole.
pub fn natural(id: &str) -> (String, u64, String) {
    let at = id.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
    let (head, tail) = id.split_at(at);
    (head.to_string(), tail.parse().unwrap_or(0), id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::frame::tests::demo;

    #[test]
    fn the_demos_marks_are_kilns_and_say_nothing_twice() {
        let d = demo();
        let m = marks(&d);
        assert_eq!(m.get("TASK-003"), Some(&Mark::Ready));
        assert_eq!(
            m.get("TASK-004"),
            Some(&Mark::Waits(vec!["TASK-003".into()]))
        );
        assert_eq!(m.get("TASK-002"), None, "in progress: its status says so");
        assert_eq!(m.get("TASK-005"), None, "blocked: its status says so");
        assert_eq!(Mark::cell(m.get("TASK-004")).text, "waits on TASK-003");
        assert_eq!(owner(Some("ana")).text, "· ana");
        assert_eq!(owner(Some("")).text, "");
    }

    #[test]
    fn natural_order_puts_9_before_10() {
        assert!(natural("MS-9") < natural("MS-10"));
        assert!(natural("ADR-010") > natural("ADR-9"));
        assert!(natural("X") < natural("X-1"));
        assert_eq!(
            natural("A-99999999999999999999999").1,
            0,
            "too long a number: no panic"
        );
    }
}
