//! Widgets for an extension's canvas pages (`Page::canvas`), and their
//! keys, so an extension doesn't lay out its own tables, tab bars and
//! fields cell by cell.
//!
//! Each widget is a state struct the extension keeps (what's focused,
//! where it's scrolled to, what's typed) and the data it borrows to draw:
//! `render(&state, rect, &mut canvas)` draws it into a [`Rect`] of a
//! [`Canvas`], `handle_key(&mut state, key)` answers a key with an
//! [`Outcome`]. A page is drawn by splitting its rectangle ([`Split`],
//! [`Rect::split_top`]) and rendering a widget into each piece; the
//! canvas then goes to Fenix with [`Canvas::to_api`].
//!
//! Columns are display cells: a wide character (CJK, most emoji) takes
//! two, measured with `unicode-width`. `to_api` turns them into the
//! character offsets the API's `cells` count.
//!
//! The crate has its own [`Key`], [`Role`] and [`Canvas`], so it builds
//! and tests on the host with nothing of Fenix's; the `api` feature (on by
//! default) converts them to the extension API's.

mod canvas;
mod key;
mod motion;
mod mouse;
mod rect;
mod style;

pub mod field;
pub mod scroll;
pub mod split;
pub mod table;
pub mod tabs;
pub mod text;
pub mod tree;

#[cfg(feature = "api")]
mod api;

pub use canvas::{Canvas, Mark, Picture, Popup};
pub use field::TextField;
pub use key::{Key, KeyPress};
pub use mouse::{Button, Mods, Mouse, MouseAction};
pub use rect::Rect;
pub use scroll::{scrollbar, ScrollState};
pub use split::{Direction, Size, Split};
pub use style::{Fill, Role, Style};
pub use table::{Align, Column, Selecting, TableSource, TableState, Width};
pub use tabs::{Tab, TabsState};
pub use tree::{TreeNode, TreeRow, TreeState};

/// What a widget did with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing: not its key, or it's at an edge (`Up` on a table's first
    /// row, `Left` at a field's start). The page can use it.
    Ignored,
    /// What's focused, selected or shown moved: draw it again.
    Moved,
    /// A field's text changed.
    Edited,
    /// `Enter`: open the row, run the query, take the field.
    Submit,
    /// `Esc` in a field with nothing selected.
    Cancel,
    /// A tree's branch opened: its children may want loading.
    Expand,
    /// A tree's branch closed.
    Collapse,
}

impl Outcome {
    /// Whether the widget took the key.
    pub fn handled(self) -> bool {
        self != Outcome::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database browser's page: the schema in a sidebar, and beside it
    /// a query field, the result's tabs and its table.
    #[test]
    fn widgets_compose_into_a_page() {
        let schema = vec![TreeNode::branch("main", "main", vec![TreeNode::leaf("main/users", "users")])];
        let mut tree = TreeState::new();
        tree.expand("main");
        tree.focused = false;
        let query = TextField::new().with_text("select * from users");
        let tabs = [Tab::new("Rows").badge("2"), Tab::new("Plan")];
        let tab_state = TabsState::new();
        let columns = [Column::fit("id", 4).right(), Column::new("name", Width::Flex(1))];
        let rows = vec![vec!["1", "ada"], vec!["2", "grace"]];
        let mut table = TableState::new();
        table.focused = false;

        let mut c = Canvas::new();
        let page = Rect::new(0, 0, 50, 8);
        let (side, body) = Split::sidebar(14).render(page, &mut c);
        tree.render(&schema, side, &mut c);
        let (field, rest) = body.inset_x(1).split_top(1);
        query.render(field, &mut c);
        let (_, rest) = rest.split_top(1);
        let (bar, rest) = rest.split_top(2);
        tab_state.render(&tabs, bar, &mut c);
        table.render(&columns, rows.as_slice(), rest, &mut c);
        assert_eq!(
            c.text(),
            [
                "▾ main        │ select * from users▏",
                "    users     │",
                "              │ 1 Rows 2   2 Plan",
                "              │",
                "              │ id  name",
                "              │",
                "              │  1  ada",
                "              │  2  grace",
            ]
            .join("\n")
        );
        assert!(c.typing(), "the field has the keyboard");
        assert_eq!(c.focused(), None);
    }
}
