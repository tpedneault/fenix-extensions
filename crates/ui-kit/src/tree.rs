//! A list, or a tree of them: branches opened and closed, a row
//! selected and kept in view. A list is a tree of leaves.

use std::cell::Cell;
use std::collections::HashSet;

use crate::motion::{page, step, top_for};
use crate::scroll::scrollbar;
use crate::text::{fit, width};
use crate::mouse::wheel_step;
use crate::{Canvas, Key, Mouse, Outcome, Rect, Role, Style};

/// A row of a tree and what's under it. The extension owns its nodes;
/// ids are unique in the tree (a path), and are how it remembers which
/// branches are open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub style: Style,
    /// Drawn muted on the right: a count, a type.
    pub detail: Option<String>,
    pub children: Vec<TreeNode>,
    /// It may have children not loaded yet: it opens, and the extension
    /// loads them when it does (`Outcome::Expand`).
    pub lazy: bool,
}

impl TreeNode {
    pub fn leaf(id: impl Into<String>, label: impl Into<String>) -> TreeNode {
        TreeNode { id: id.into(), label: label.into(), ..TreeNode::default() }
    }

    pub fn branch(id: impl Into<String>, label: impl Into<String>, children: Vec<TreeNode>) -> TreeNode {
        TreeNode { children, ..TreeNode::leaf(id, label) }
    }

    /// A branch whose children come later.
    pub fn lazy(id: impl Into<String>, label: impl Into<String>) -> TreeNode {
        TreeNode { lazy: true, ..TreeNode::leaf(id, label) }
    }

    pub fn style(mut self, style: impl Into<Style>) -> TreeNode {
        self.style = style.into();
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> TreeNode {
        self.detail = Some(detail.into());
        self
    }

    /// Whether it opens.
    pub fn expandable(&self) -> bool {
        self.lazy || !self.children.is_empty()
    }
}

/// A row as shown: its node, how deep, and the row of its parent.
#[derive(Debug, Clone, Copy)]
pub struct TreeRow<'a> {
    pub node: &'a TreeNode,
    pub depth: usize,
    pub parent: Option<usize>,
}

/// Which row is selected and which branches are open.
#[derive(Debug, Clone)]
pub struct TreeState {
    /// The selected row, of those shown.
    pub selected: usize,
    /// Whether it has the keyboard: the selected row is the canvas's
    /// focus.
    pub focused: bool,
    /// What's drawn when there are no rows.
    pub empty: String,
    expanded: HashSet<String>,
    top: Cell<usize>,
    view: Cell<usize>,
    area: Cell<Rect>,
}

impl Default for TreeState {
    fn default() -> TreeState {
        TreeState { selected: 0, focused: true, empty: "Nothing here".into(), expanded: HashSet::new(), top: Cell::new(0), view: Cell::new(0), area: Cell::new(Rect::default()) }
    }
}

impl TreeState {
    pub fn new() -> TreeState {
        TreeState::default()
    }

    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    pub fn expand(&mut self, id: &str) {
        self.expanded.insert(id.to_string());
    }

    pub fn collapse(&mut self, id: &str) {
        self.expanded.remove(id);
    }

    /// The rows shown: each open branch's children under it.
    pub fn rows<'a>(&self, roots: &'a [TreeNode]) -> Vec<TreeRow<'a>> {
        fn walk<'a>(s: &TreeState, nodes: &'a [TreeNode], depth: usize, parent: Option<usize>, out: &mut Vec<TreeRow<'a>>) {
            for node in nodes {
                out.push(TreeRow { node, depth, parent });
                if s.is_expanded(&node.id) {
                    let me = out.len() - 1;
                    walk(s, &node.children, depth + 1, Some(me), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, roots, 0, None, &mut out);
        out
    }

    pub fn selected_node<'a>(&self, roots: &'a [TreeNode]) -> Option<&'a TreeNode> {
        let rows = self.rows(roots);
        rows.get(self.selected.min(rows.len().saturating_sub(1))).map(|r| r.node)
    }

    /// Selects the row of node `id`, when it's shown.
    pub fn select_id(&mut self, roots: &[TreeNode], id: &str) -> bool {
        match self.rows(roots).iter().position(|r| r.node.id == id) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    /// `j` `k` and the arrows a row, `J` `K` (`PageDown`, `PageUp`) a
    /// screen, `g` `G` (`Home`, `End`) the first and last; `l` (`Right`)
    /// opens a branch, then goes into it; `h` (`Left`) closes it, or goes
    /// to its parent; `Enter` submits the row, branch or leaf (the
    /// extension opens it, or [`TreeState::expand`]s it).
    pub fn handle_key(&mut self, key: Key, roots: &[TreeNode]) -> Outcome {
        let rows = self.rows(roots);
        if rows.is_empty() {
            return Outcome::Ignored;
        }
        let at = self.selected.min(rows.len() - 1);
        let row = rows[at];
        let id = row.node.id.clone();
        match key {
            Key::Enter => Outcome::Submit,
            Key::Right | Key::Char('l') if row.node.expandable() && !self.is_expanded(&id) => {
                self.expand(&id);
                Outcome::Expand
            }
            Key::Right | Key::Char('l') => {
                if self.is_expanded(&id) && rows.get(at + 1).is_some_and(|r| r.parent == Some(at)) {
                    self.selected = at + 1;
                    Outcome::Moved
                } else {
                    Outcome::Ignored
                }
            }
            Key::Left | Key::Char('h') if self.is_expanded(&id) => {
                self.collapse(&id);
                Outcome::Collapse
            }
            Key::Left | Key::Char('h') => match row.parent {
                Some(p) => {
                    self.selected = p;
                    Outcome::Moved
                }
                None => Outcome::Ignored,
            },
            key => match step(key, at, rows.len(), page(self.view.get())) {
                Some(to) if to != self.selected => {
                    self.selected = to;
                    Outcome::Moved
                }
                _ => Outcome::Ignored,
            },
        }
    }

    /// Draws the rows shown into `rect`: indented two cells a level, `▸`
    /// on a closed branch and `▾` on an open one, details on the right, a
    /// scrollbar when they don't all fit.
    pub fn render(&self, roots: &[TreeNode], rect: Rect, canvas: &mut Canvas) {
        self.area.set(rect);
        self.view.set(rect.h);
        if rect.is_empty() {
            return;
        }
        let rows = self.rows(roots);
        if rows.is_empty() {
            canvas.put_fit(rect.x, rect.y, rect.w, &self.empty, Role::Muted);
            return;
        }
        let bar = rows.len() > rect.h && rect.w > 1;
        let w = rect.w - usize::from(bar);
        let at = self.selected.min(rows.len() - 1);
        let top = top_for(self.top.get(), at, rect.h, rows.len());
        self.top.set(top);
        // A list (no branches at all) has no room kept for the markers.
        let markers = rows.iter().any(|r| r.node.expandable() || r.depth > 0);
        for (i, row) in rows.iter().enumerate().skip(top).take(rect.h) {
            let y = rect.y + i - top;
            let mut x = rect.x + 2 * row.depth;
            if markers {
                let mark = match (row.node.expandable(), self.is_expanded(&row.node.id)) {
                    (false, _) => "  ",
                    (true, false) => "▸ ",
                    (true, true) => "▾ ",
                };
                x = canvas.put(x, y, mark, Role::Muted);
            }
            let room = (rect.x + w).saturating_sub(x);
            let detail = row.node.detail.as_deref().unwrap_or("");
            // The detail when there's room for it and some of the label.
            let dw = width(detail);
            let (label_room, detail_at) = if !detail.is_empty() && dw + 2 + 4 <= room { (room - dw - 2, Some(rect.x + w - dw)) } else { (room, None) };
            canvas.put(x, y, &fit(&row.node.label, label_room), &row.node.style);
            if let Some(dx) = detail_at {
                canvas.put(dx, y, detail, Role::Muted);
            }
            if self.focused && i == at {
                canvas.focus(y, rect.x..rect.x + w);
            }
        }
        if bar {
            scrollbar(canvas, rect.x + w, rect.y, rect.h, rows.len(), top);
        }
    }

    /// A left click at (`x`, `y`) on the rows of `roots`.
    pub fn click(&mut self, x: usize, y: usize, roots: &[TreeNode]) -> Outcome {
        self.mouse(&Mouse::click(x, y), roots)
    }

    /// The mouse on it, as last drawn: a click selects the row, or opens
    /// or closes a branch clicked on its marker; a double click submits
    /// the row (as `Enter`); the wheel moves the selection three rows a
    /// notch.
    pub fn mouse(&mut self, m: &Mouse, roots: &[TreeNode]) -> Outcome {
        let rect = self.area.get();
        if !rect.contains(m.x, m.y) {
            return Outcome::Ignored;
        }
        let rows = self.rows(roots);
        if let Some(lines) = m.wheel() {
            return match wheel_step(self.selected.min(rows.len().saturating_sub(1)), rows.len(), lines) {
                Some(to) => {
                    self.selected = to;
                    Outcome::Moved
                }
                None => Outcome::Ignored,
            };
        }
        let clicks = m.left_clicks();
        let i = self.top.get() + (m.y - rect.y);
        let Some(row) = rows.get(i) else { return Outcome::Ignored };
        if clicks == 0 {
            return Outcome::Ignored;
        }
        let was = self.selected;
        self.selected = i;
        let marker = rect.x + 2 * row.depth;
        if clicks == 1 && row.node.expandable() && (marker..marker + 2).contains(&m.x) {
            let id = row.node.id.clone();
            return if self.is_expanded(&id) {
                self.collapse(&id);
                Outcome::Collapse
            } else {
                self.expand(&id);
                Outcome::Expand
            };
        }
        if clicks >= 2 {
            Outcome::Submit
        } else if was != i {
            Outcome::Moved
        } else {
            Outcome::Ignored
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Vec<TreeNode> {
        vec![
            TreeNode::branch(
                "main",
                "main",
                vec![TreeNode::branch("main/users", "users", vec![TreeNode::leaf("main/users/id", "id").detail("integer")]).detail("3 rows"), TreeNode::leaf("main/posts", "posts")],
            ),
            TreeNode::lazy("temp", "temp"),
        ]
    }

    fn drawn(s: &TreeState, roots: &[TreeNode], w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        s.render(roots, Rect::new(0, 0, w, h), &mut c);
        c
    }

    #[test]
    fn branches_open_and_close() {
        let roots = db();
        let mut s = TreeState::new();
        assert_eq!(drawn(&s, &roots, 30, 5).text(), "▸ main\n▸ temp");
        assert_eq!(s.handle_key(Key::Char('l'), &roots), Outcome::Expand);
        assert_eq!(drawn(&s, &roots, 30, 5).text(), "▾ main\n  ▸ users               3 rows\n    posts\n▸ temp");
        assert_eq!(s.handle_key(Key::Char('l'), &roots), Outcome::Moved, "into it");
        assert_eq!(s.selected_node(&roots).map(|n| n.id.as_str()), Some("main/users"));
        s.handle_key(Key::Right, &roots);
        s.handle_key(Key::Right, &roots);
        assert_eq!(s.selected_node(&roots).map(|n| n.id.as_str()), Some("main/users/id"));
        assert_eq!(s.handle_key(Key::Right, &roots), Outcome::Ignored, "a leaf");
        let c = drawn(&s, &roots, 30, 6);
        assert_eq!(c.line(2), "      id               integer");
        assert_eq!(c.focused(), Some(&(2, 0..30)));
        assert_eq!(s.handle_key(Key::Char('h'), &roots), Outcome::Moved, "to its parent");
        assert_eq!(s.handle_key(Key::Char('h'), &roots), Outcome::Collapse);
        assert_eq!(s.selected, 1);
        assert_eq!(s.handle_key(Key::Enter, &roots), Outcome::Submit);
        s.handle_key(Key::Char('G'), &roots);
        assert_eq!(s.selected_node(&roots).map(|n| n.id.as_str()), Some("temp"));
        assert_eq!(s.handle_key(Key::Char('l'), &roots), Outcome::Expand, "a lazy branch opens");
        assert!(s.is_expanded("temp"));
        assert_eq!(s.handle_key(Key::Char('j'), &roots), Outcome::Ignored, "the last row");
    }

    #[test]
    fn a_list_is_a_tree_of_leaves() {
        let items: Vec<TreeNode> = (0..50).map(|i| TreeNode::leaf(i.to_string(), format!("item {i}"))).collect();
        let mut s = TreeState::new();
        s.handle_key(Key::Char('G'), &items);
        let c = drawn(&s, &items, 12, 4);
        assert_eq!(c.text(), "item 46    │\nitem 47    │\nitem 48    │\nitem 49    ┃");
        assert_eq!(c.focused(), Some(&(3, 0..11)));
        assert!(s.select_id(&items, "3"));
        assert_eq!(drawn(&s, &items, 12, 4).line(0), "item 3     ┃");
        let none: Vec<TreeNode> = Vec::new();
        assert_eq!(drawn(&s, &none, 20, 2).text(), "Nothing here");
        assert_eq!(s.handle_key(Key::Down, &none), Outcome::Ignored);
    }

    #[test]
    fn clicks_select_and_open() {
        let roots = db();
        let mut s = TreeState::new();
        drawn(&s, &roots, 30, 5);
        assert_eq!(s.click(0, 0, &roots), Outcome::Expand, "on the marker");
        drawn(&s, &roots, 30, 5);
        assert_eq!(s.click(10, 2, &roots), Outcome::Moved);
        assert_eq!(s.selected_node(&roots).map(|n| n.id.as_str()), Some("main/posts"));
        assert_eq!(s.click(10, 2, &roots), Outcome::Ignored, "already selected");
        let double = Mouse { action: crate::MouseAction::Press { button: crate::Button::Left, clicks: 2 }, ..Mouse::click(5, 1) };
        assert_eq!(s.mouse(&double, &roots), Outcome::Submit);
        assert_eq!(s.selected, 1);
        assert_eq!(s.click(5, 4, &roots), Outcome::Ignored, "below the rows");
        assert_eq!(s.click(40, 0, &roots), Outcome::Ignored, "outside");
        let wheel = Mouse { action: crate::MouseAction::Wheel(-1), ..Mouse::click(5, 0) };
        assert_eq!(s.mouse(&wheel, &roots), Outcome::Moved);
        assert_eq!(s.selected, 3, "the last row");
    }
}
