//! The gallery's sections, a widget each, and the code each is drawn by.
//!
//! Each section is a file here: a `State` the page keeps, `render(&self,
//! rect, canvas)`, `key(&mut self, press)` and `mouse(&mut self, m)`.
//! What `c` shows is that file's own text between its `// gallery:begin`
//! and `// gallery:end` lines, built in with `include_str!`, so the code
//! shown is the code that runs.

pub mod field;
pub mod lists;
pub mod menu;
pub mod mouse;
pub mod scroll;
pub mod splits;
pub mod styles;
pub mod table;
pub mod tabs;
pub mod tree;

use serde::{Deserialize, Serialize};

/// What a section did with a key or the mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Not its key: the page may use it.
    Ignored,
    /// Taken: draw again.
    Done,
    /// Taken, and something to say in the page's message line.
    Say(String),
    /// `text` to the clipboard, and what to say about it.
    Copy { text: String, say: String },
}

impl Reply {
    pub fn handled(&self) -> bool {
        *self != Reply::Ignored
    }

    /// A widget's outcome as a reply.
    pub fn from(outcome: fenix_ui_kit::Outcome) -> Reply {
        if outcome.handled() {
            Reply::Done
        } else {
            Reply::Ignored
        }
    }
}

/// The sections, in the sidebar's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Id {
    Table,
    Tree,
    Tabs,
    Field,
    Lists,
    Menu,
    Splits,
    Scroll,
    Styles,
    Mouse,
}

impl Id {
    pub const ALL: [Id; 10] = [Id::Table, Id::Tree, Id::Tabs, Id::Field, Id::Lists, Id::Menu, Id::Splits, Id::Scroll, Id::Styles, Id::Mouse];

    pub fn index(self) -> usize {
        Id::ALL.iter().position(|&i| i == self).unwrap_or(0)
    }

    /// As the sidebar has it.
    pub fn title(self) -> &'static str {
        match self {
            Id::Table => "Table",
            Id::Tree => "Tree",
            Id::Tabs => "Tabs",
            Id::Field => "Text field",
            Id::Lists => "List",
            Id::Menu => "Popup menu",
            Id::Splits => "Splits",
            Id::Scroll => "Scrolling",
            Id::Styles => "Styles",
            Id::Mouse => "Mouse",
        }
    }

    /// A line under the title: what ui-kit gives it.
    pub fn about(self) -> &'static str {
        match self {
            Id::Table => "TableState: 400 rows asked for as shown; sort, select",
            Id::Tree => "TreeState: folders opened, closed, one read when opened",
            Id::Tabs => "TabsState: Tab, Shift+Tab, 1 to 9 or a click",
            Id::Field => "TextField: a caret, selections, undo, a / filter",
            Id::Lists => "A list is a tree of leaves: styled, detailed, or empty",
            Id::Menu => "Canvas::popup: Fenix draws it, the page has the keys",
            Id::Splits => "Split: a sidebar, one over the other, cells or percent",
            Id::Scroll => "ScrollState: only the lines shown are drawn",
            Id::Styles => "Role, Fill, underlines, the palette, text in cells",
            Id::Mouse => "Mouse::from(&event): clicks, drags, the wheel, mods",
        }
    }

    /// Its file under `src/sections/`.
    pub fn file(self) -> &'static str {
        match self {
            Id::Table => "table.rs",
            Id::Tree => "tree.rs",
            Id::Tabs => "tabs.rs",
            Id::Field => "field.rs",
            Id::Lists => "lists.rs",
            Id::Menu => "menu.rs",
            Id::Splits => "splits.rs",
            Id::Scroll => "scroll.rs",
            Id::Styles => "styles.rs",
            Id::Mouse => "mouse.rs",
        }
    }

    /// Its file's whole text, as built.
    pub fn source(self) -> &'static str {
        match self {
            Id::Table => include_str!("table.rs"),
            Id::Tree => include_str!("tree.rs"),
            Id::Tabs => include_str!("tabs.rs"),
            Id::Field => include_str!("field.rs"),
            Id::Lists => include_str!("lists.rs"),
            Id::Menu => include_str!("menu.rs"),
            Id::Splits => include_str!("splits.rs"),
            Id::Scroll => include_str!("scroll.rs"),
            Id::Styles => include_str!("styles.rs"),
            Id::Mouse => include_str!("mouse.rs"),
        }
    }

    /// What `c` shows: its file between the markers, with a line saying
    /// where it's from.
    pub fn code(self) -> String {
        let body = markers(self.source()).unwrap_or("// (its markers are missing)\n");
        format!("// extensions/gallery/src/sections/{} -- the {} section, as the gallery runs it.\n\n{}", self.file(), self.title(), body.trim_matches('\n'))
    }
}

pub const BEGIN: &str = "// gallery:begin";
pub const END: &str = "// gallery:end";

/// The text between a file's one `// gallery:begin` line and its one
/// `// gallery:end` line after it; none unless there's exactly one of
/// each, in that order.
pub fn markers(source: &str) -> Option<&str> {
    let lines = |m: &str| source.match_indices(m).filter(|(at, _)| source[..*at].ends_with('\n') || *at == 0).map(|(at, _)| at).collect::<Vec<_>>();
    let (begins, ends) = (lines(BEGIN), lines(END));
    let ([b], [e]) = (begins.as_slice(), ends.as_slice()) else { return None };
    let start = source[*b..].find('\n').map(|n| b + n + 1)?;
    (start <= *e).then(|| &source[start..*e])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_take_one_region() {
        let src = "//! doc\nuse x;\n// gallery:begin\nfn a() {}\n// gallery:end\nfn tests() {}\n";
        assert_eq!(markers(src), Some("fn a() {}\n"));
        assert_eq!(markers("// gallery:begin\nx\n"), None, "no end");
        assert_eq!(markers("// gallery:end\n// gallery:begin\n"), None, "the wrong way round");
        assert_eq!(markers("// gallery:begin\n// gallery:begin\nx\n// gallery:end\n"), None, "two");
        assert_eq!(markers("let s = \"// gallery:begin\";\n"), None, "only at a line's start");
    }
}
