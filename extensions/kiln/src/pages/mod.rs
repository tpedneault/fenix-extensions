//! The plan page: one page kind, `plan`, with a tab for each of Kiln's
//! views, in the order Kiln's GUI has them.
//!
//! - [`frame`]: what every tab does the same -- tabs, cursor, scroll, the
//!   `/` filter, `Enter`, `r`, `a`, `v`, `i`, the page's state, the states
//!   that aren't "loaded" -- written once.
//! - [`view`]: the [`view::View`] each tab implements, and its rows.
//! - one module per tab, each its own file: [`tree`] is the worked
//!   example; `README.md` here says how to write one.
//! - [`shared`]: what several tabs say alike (the plan's marks, the
//!   owner, natural ID order).
//!
//! [`Tab`] is the one list of tabs. Every tab is registered here already,
//! so a tab's module is all its author edits. Eleven: `1`–`9` reach the
//! first nine and `0` the tenth, Exports; Comments is reached with `Tab`
//! (the frame's header says how the bar copes).
//!
//! What it deliberately doesn't do: call Fenix or kiln. `lib.rs` hands
//! the frame Fenix's keys and kiln's answers and does what it returns.

pub mod frame;
pub mod view;

pub mod board;
pub mod changes;
pub mod check;
pub mod comments;
pub mod decisions;
pub mod exports;
pub mod impact;
pub mod item;
pub mod roadmap;
pub mod shared;
pub mod trace;
pub mod tree;

use view::View;

/// A tab of the plan page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Tree,
    Trace,
    Impact,
    Board,
    Roadmap,
    Decisions,
    Changes,
    Item,
    Check,
    Exports,
    Comments,
}

impl Tab {
    /// Every tab, in the order they're shown and numbered (`1`–`9`, then
    /// `0`).
    pub const ALL: [Tab; 11] = [
        Tab::Tree,
        Tab::Trace,
        Tab::Impact,
        Tab::Board,
        Tab::Roadmap,
        Tab::Decisions,
        Tab::Changes,
        Tab::Item,
        Tab::Check,
        Tab::Exports,
        Tab::Comments,
    ];

    /// Its place in [`Tab::ALL`].
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    /// Its title on the tab bar.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Tree => "Tree",
            Tab::Trace => "Trace",
            Tab::Impact => "Impact",
            Tab::Board => "Board",
            Tab::Roadmap => "Roadmap",
            Tab::Decisions => "Decisions",
            Tab::Changes => "Changes",
            Tab::Item => "Item",
            Tab::Check => "Check",
            Tab::Exports => "Exports",
            Tab::Comments => "Comments",
        }
    }

    /// Its name in the page's saved state: never changed once released.
    pub fn name(self) -> &'static str {
        match self {
            Tab::Tree => "tree",
            Tab::Trace => "trace",
            Tab::Impact => "impact",
            Tab::Board => "board",
            Tab::Roadmap => "roadmap",
            Tab::Decisions => "decisions",
            Tab::Changes => "changes",
            Tab::Item => "item",
            Tab::Check => "check",
            Tab::Exports => "exports",
            Tab::Comments => "comments",
        }
    }

    pub fn by_name(name: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.name() == name)
    }

    /// A new view for it, as a page opens.
    pub fn view(self) -> Box<dyn View> {
        match self {
            Tab::Tree => Box::<tree::Tree>::default(),
            Tab::Trace => Box::<trace::Trace>::default(),
            Tab::Impact => Box::<impact::Impact>::default(),
            Tab::Board => Box::<board::Board>::default(),
            Tab::Roadmap => Box::<roadmap::Roadmap>::default(),
            Tab::Decisions => Box::<decisions::Decisions>::default(),
            Tab::Changes => Box::<changes::Changes>::default(),
            Tab::Item => Box::<item::ItemView>::default(),
            Tab::Check => Box::<check::Check>::default(),
            Tab::Exports => Box::<exports::Exports>::default(),
            Tab::Comments => Box::<comments::Comments>::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_has_a_name_it_is_found_by() {
        for t in Tab::ALL {
            assert_eq!(Tab::by_name(t.name()), Some(t));
            assert_eq!(Tab::ALL[t.index()], t);
        }
        assert_eq!(Tab::by_name("exports"), Some(Tab::Exports));
        assert_eq!(Tab::by_name("jira"), None);
    }
}
