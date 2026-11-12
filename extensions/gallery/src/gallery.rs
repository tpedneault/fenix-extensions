//! The gallery's page: the sections in a sidebar, the one picked beside
//! it, a message line and the key strip at its foot. Plain state drawn
//! into a [`Canvas`], with nothing of Fenix's, so it's tested natively;
//! `lib.rs` is the glue.

use std::cell::Cell;

use fenix_ui_kit::{Canvas, Fill, Key, KeyPress, Mouse, Rect, Role, Split, Style, TreeNode, TreeState};
use serde::{Deserialize, Serialize};

use crate::draw;
use crate::sections::{self, Id, Reply};

/// What the page asks of Fenix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Text to the clipboard.
    Copy(String),
    /// A section's code, read-only, beside the page.
    Code { title: String, text: String },
}

/// The sidebar's width.
const SIDEBAR: usize = 13;
/// Narrower than this, the page has no sidebar.
const WITH_SIDEBAR: usize = 50;

pub struct Gallery {
    pub section: Id,
    pub table: sections::table::State,
    pub tree: sections::tree::State,
    pub tabs: sections::tabs::State,
    pub field: sections::field::State,
    pub lists: sections::lists::State,
    pub menu: sections::menu::State,
    pub splits: sections::splits::State,
    pub scroll: sections::scroll::State,
    pub styles: sections::styles::State,
    pub mouse: sections::mouse::State,
    sidebar: TreeState,
    /// The last thing a section said; gone at the next key.
    pub message: Option<String>,
    /// What the foot says when nothing else is: the keys that open the
    /// gallery from anywhere, as the scheme in use has them (`lib.rs`
    /// asks Fenix, `key_for`). None when no key does.
    pub hint: Option<String>,
    /// Where the sidebar was drawn, for clicks.
    side: Cell<Rect>,
}

impl Default for Gallery {
    fn default() -> Gallery {
        Gallery::new()
    }
}

/// What the page keeps with the session: the section, and where you
/// were in each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub section: Option<Id>,
    pub table: (usize, usize),
    pub sort: Option<(usize, bool)>,
    pub tree: usize,
    pub open: Vec<String>,
    pub fixtures: bool,
    pub tab: usize,
    pub numbers: Option<bool>,
    pub name: Option<String>,
    pub filter: String,
    pub element: usize,
    pub task: usize,
    pub done: Vec<bool>,
    pub file: usize,
    pub splits: Option<(usize, u8, bool)>,
    pub line: usize,
    pub width: Option<usize>,
}

impl Gallery {
    pub fn new() -> Gallery {
        let mut sidebar = TreeState::new();
        sidebar.focused = false;
        Gallery {
            section: Id::Table,
            table: Default::default(),
            tree: Default::default(),
            tabs: Default::default(),
            field: Default::default(),
            lists: Default::default(),
            menu: Default::default(),
            splits: Default::default(),
            scroll: Default::default(),
            styles: Default::default(),
            mouse: Default::default(),
            sidebar,
            message: None,
            hint: None,
            side: Cell::new(Rect::default()),
        }
    }

    /// A field has the keyboard: the page is sent as typing.
    pub fn typing(&self) -> bool {
        self.section == Id::Field && self.field.typing()
    }

    /// The sections, as the sidebar lists them: the one shown marked.
    fn entries(&self) -> Vec<TreeNode> {
        Id::ALL
            .iter()
            .map(|&id| {
                if id == self.section {
                    TreeNode::leaf(id.title(), format!("› {}", id.title())).style(Style::new(Role::Title).bg(Fill::Panel))
                } else {
                    TreeNode::leaf(id.title(), format!("  {}", id.title())).style(Role::Muted)
                }
            })
            .collect()
    }

    /// The keys for the strip: the section's, then the page's.
    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = match self.section {
            Id::Table => self.table.keys(),
            Id::Tree => self.tree.keys(),
            Id::Tabs => self.tabs.keys(),
            Id::Field => self.field.keys(),
            Id::Lists => self.lists.keys(),
            Id::Menu => self.menu.keys(),
            Id::Splits => self.splits.keys(),
            Id::Scroll => self.scroll.keys(),
            Id::Styles => self.styles.keys(),
            Id::Mouse => self.mouse.keys(),
        };
        if !self.typing() {
            keys.extend([("[ ]", "section"), ("c", "its code")]);
        }
        keys
    }

    /// A section drawn into `rect`, as the page draws it.
    pub fn render_section(&self, id: Id, rect: Rect, c: &mut Canvas) {
        match id {
            Id::Table => self.table.render(rect, c),
            Id::Tree => self.tree.render(rect, c),
            Id::Tabs => self.tabs.render(rect, c),
            Id::Field => self.field.render(rect, c),
            Id::Lists => self.lists.render(rect, c),
            Id::Menu => self.menu.render(rect, c),
            Id::Splits => self.splits.render(rect, c),
            Id::Scroll => self.scroll.render(rect, c),
            Id::Styles => self.styles.render(rect, c),
            Id::Mouse => self.mouse.render(rect, c),
        }
    }

    /// The page, `w` cells wide and `h` lines tall.
    pub fn render(&self, w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        c.set_more_keys(true);
        let page = Rect::new(0, 0, w, h);
        if page.is_empty() {
            return c;
        }
        // The title, and a hairline under it.
        let (head, rest) = page.split_top(if h >= 8 { 2 } else { 0 });
        let x = draw::text(&mut c, head, 0, 0, "Widget gallery", Role::Title);
        draw::text(&mut c, head, x + 2, 0, "ui-kit's widgets, live -- c shows the code that draws each", Role::Muted);
        if head.h == 2 {
            c.rule(1, 0..w);
        }

        // The foot: a hairline, the message, the keys.
        let keys = self.keys();
        let strip = draw::strip_height(&keys, w);
        let foot_h = if rest.h >= strip + 8 { strip + 2 } else { 0 };
        let (foot, body) = rest.split_bottom(foot_h);
        if foot_h > 0 {
            c.rule(foot.y, 0..w);
            if let Some(m) = &self.message {
                draw::text(&mut c, foot, 0, 1, m, Role::Accent);
            } else if let Some(keys) = &self.hint {
                draw::text(&mut c, foot, 0, 1, &format!("{keys} opens the gallery from anywhere"), Role::Muted);
            }
            draw::draw_strip(&mut c, &keys, Rect::new(0, foot.y + 2, w, strip));
        }

        // The sidebar, when there's room for it.
        let body = if w >= WITH_SIDEBAR {
            let (side, main) = Split::sidebar(SIDEBAR).render(body, &mut c);
            self.side.set(side);
            self.sidebar.render(&self.entries(), side, &mut c);
            main.inset_x(1)
        } else {
            self.side.set(Rect::default());
            body
        };

        // The section: its title, what it shows, then it.
        let (title, main) = body.split_top(if body.h >= 6 { 2 } else { 0 });
        let x = draw::text(&mut c, title, 0, 0, self.section.title(), Role::Title);
        draw::text(&mut c, title, x - title.x + 2, 0, self.section.about(), Role::Muted);
        self.render_section(self.section, main, &mut c);
        c
    }

    fn go(&mut self, to: Id) {
        self.section = to;
        self.sidebar.selected = to.index();
    }

    /// A key, with what's held: a typing field's first, then the page's
    /// own -- `[` `]` and `c` -- then the section's.
    pub fn key(&mut self, press: KeyPress) -> Option<Effect> {
        self.message = None;
        if !self.typing() && !press.mods.chord() {
            let n = Id::ALL.len();
            let at = self.section.index();
            match press.key {
                Key::Char('[') => {
                    self.go(Id::ALL[(at + n - 1) % n]);
                    return None;
                }
                Key::Char(']') => {
                    self.go(Id::ALL[(at + 1) % n]);
                    return None;
                }
                Key::Char('c') => return Some(Effect::Code { title: format!("Gallery: {}", self.section.title()), text: self.section.code() }),
                _ => {}
            }
        }
        let reply = match self.section {
            Id::Table => self.table.key(press),
            Id::Tree => self.tree.key(press),
            Id::Tabs => self.tabs.key(press),
            Id::Field => self.field.key(press),
            Id::Lists => self.lists.key(press),
            Id::Menu => self.menu.key(press),
            Id::Splits => self.splits.key(press),
            Id::Scroll => self.scroll.key(press),
            Id::Styles => self.styles.key(press),
            Id::Mouse => self.mouse.key(press),
        };
        self.reply(reply)
    }

    fn reply(&mut self, reply: Reply) -> Option<Effect> {
        match reply {
            Reply::Say(text) => {
                self.message = Some(text);
                None
            }
            Reply::Copy { text, say } => {
                self.message = Some(say);
                Some(Effect::Copy(text))
            }
            Reply::Ignored | Reply::Done => None,
        }
    }

    /// The mouse: on the sidebar, a section; else the section's.
    pub fn mouse(&mut self, m: &Mouse) -> Option<Effect> {
        let side = self.side.get();
        if side.contains(m.x, m.y) {
            if m.left_clicks() > 0 {
                let i = self.sidebar.selected;
                self.sidebar.mouse(m, &self.entries());
                let picked = self.sidebar.selected;
                self.sidebar.selected = i;
                if let Some(&id) = Id::ALL.get(picked) {
                    self.go(id);
                }
            }
            return None;
        }
        let reply = match self.section {
            Id::Table => self.table.mouse(m),
            Id::Tree => self.tree.mouse(m),
            Id::Tabs => self.tabs.mouse(m),
            Id::Field => self.field.mouse(m),
            Id::Lists => self.lists.mouse(m),
            Id::Menu => self.menu.mouse(m),
            Id::Splits => self.splits.mouse(m),
            Id::Scroll => self.scroll.mouse(m),
            Id::Styles => self.styles.mouse(m),
            Id::Mouse => self.mouse.mouse(m),
        };
        self.reply(reply)
    }

    /// Text pasted while a field has the keyboard.
    pub fn paste(&mut self, text: &str) {
        if self.typing() {
            self.field.paste(text);
        }
    }

    pub fn saved(&self) -> Saved {
        let roots = self.tree.roots();
        let mut open = Vec::new();
        fn walk(s: &TreeState, nodes: &[TreeNode], out: &mut Vec<String>) {
            for n in nodes {
                if s.is_expanded(&n.id) {
                    out.push(n.id.clone());
                }
                walk(s, &n.children, out);
            }
        }
        walk(&self.tree.tree, &roots, &mut open);
        Saved {
            section: Some(self.section),
            table: (self.table.grid.row, self.table.grid.col),
            sort: self.table.sort,
            tree: self.tree.tree.selected,
            open,
            fixtures: self.tree.loaded,
            tab: self.tabs.tabs.active,
            numbers: Some(self.tabs.tabs.numbers),
            name: Some(self.field.name.text().to_string()),
            filter: self.field.filter.text().to_string(),
            element: self.field.list.selected,
            task: self.lists.list.selected,
            done: self.lists.done.clone(),
            file: self.menu.list.selected,
            splits: Some((self.splits.side, self.splits.top, self.splits.dividers)),
            line: self.scroll.scroll.offset(),
            width: Some(self.styles.width),
        }
    }

    pub fn restore(&mut self, s: &Saved) {
        self.go(s.section.unwrap_or(Id::Table));
        if let Some((col, desc)) = s.sort {
            if col < 7 {
                self.table.sort_by(col);
                if desc {
                    self.table.sort_by(col);
                }
            }
        }
        (self.table.grid.row, self.table.grid.col) = s.table;
        self.tree.loaded = s.fixtures;
        for id in &s.open {
            self.tree.tree.expand(id);
        }
        self.tree.tree.selected = s.tree;
        self.tabs.tabs.active = s.tab;
        self.tabs.tabs.numbers = s.numbers.unwrap_or(true);
        if let Some(name) = &s.name {
            self.field.name = fenix_ui_kit::TextField::new().with_text(name).with_placeholder(self.field.name.placeholder.clone());
            self.field.name.focused = false;
        }
        self.field.filter.set_text(&s.filter);
        self.field.list.selected = s.element;
        self.lists.list.selected = s.task;
        if s.done.len() == self.lists.done.len() {
            self.lists.done = s.done.clone();
        }
        self.menu.list.selected = s.file;
        if let Some((side, top, dividers)) = s.splits {
            (self.splits.side, self.splits.top, self.splits.dividers) = (side.clamp(4, 60), top.clamp(10, 90), dividers);
        }
        self.scroll.scroll.scroll_to(s.line, self.scroll.count());
        if let Some(w) = s.width {
            self.styles.width = w.clamp(2, 40);
        }
    }
}
