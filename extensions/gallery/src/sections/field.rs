//! Text fields: one to edit, and one that filters a list as you type.

// gallery:begin
use fenix_ui_kit::{Canvas, KeyPress, Mouse, Outcome, Rect, Role, TextField, TreeNode, TreeState};

use crate::data::ELEMENTS;
use crate::draw;
use crate::sections::Reply;

/// Which field has the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typing {
    No,
    /// The name, and what it was before, for Esc.
    Name { was: String },
    Filter,
}

pub struct State {
    pub name: TextField,
    pub filter: TextField,
    pub list: TreeState,
    pub typing: Typing,
}

/// The label's width, before the name field.
const LABEL: usize = 14;

impl State {
    pub fn new() -> State {
        let mut name = TextField::new().with_text("Brest harbour gauge").with_placeholder("a name for the station");
        name.focused = false;
        let mut filter = TextField::new().with_placeholder("letters of a name, or a symbol");
        filter.focused = false;
        let mut list = TreeState::new();
        list.empty = "No element matches: Esc on the filter shows them all".into();
        State { name, filter, list, typing: Typing::No }
    }

    /// The elements the filter lets through.
    pub fn items(&self) -> Vec<TreeNode> {
        let f = self.filter.text().trim().to_lowercase();
        ELEMENTS
            .iter()
            .filter(|(name, symbol, _)| f.is_empty() || name.to_lowercase().contains(&f) || symbol.eq_ignore_ascii_case(&f))
            .map(|(name, symbol, n)| TreeNode::leaf(*symbol, *name).detail(format!("{symbol:<2} {n:>2}")))
            .collect()
    }

    pub fn typing(&self) -> bool {
        self.typing != Typing::No
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        match self.typing {
            Typing::Name { .. } => vec![("Enter", "keep it"), ("Esc", "put it back"), ("S-←→", "select"), ("C-←→", "a word"), ("C-z", "undo"), ("C-a", "all")],
            Typing::Filter => vec![("Enter", "keep the filter"), ("Esc", "clear it"), ("C-Backspace", "a word")],
            Typing::No => vec![("e", "edit the name"), ("/", "filter"), ("j k", "move"), ("Enter", "pick")],
        }
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        // The name: a label, the field beside it.
        draw::text(c, rect, 0, 0, "Station name", Role::Muted);
        if rect.w > LABEL && rect.h > 0 {
            let w = (rect.w - LABEL).min(40);
            self.name.render(Rect::new(rect.x + LABEL, rect.y, w, 1), c);
        }
        let field = if self.typing == Typing::Filter { &self.filter } else { &self.name };
        let about = match field.selection() {
            Some(s) => format!("caret at {} · {} to {} selected · {} chars", field.caret(), s.start, s.end, field.text().chars().count()),
            None => format!("caret at {} · {} chars", field.caret(), field.text().chars().count()),
        };
        draw::text(c, rect, LABEL, 1, &about, Role::Muted);

        // The filter: `/` and its field while typing; what it is after.
        let items = self.items();
        if self.typing == Typing::Filter {
            draw::text(c, rect, 0, 3, "/", Role::Accent);
            if rect.w > 2 && rect.h > 3 {
                self.filter.render(Rect::new(rect.x + 2, rect.y + 3, (rect.w - 2).min(40), 1), c);
            }
        } else if !self.filter.text().is_empty() {
            let x = draw::text(c, rect, 0, 3, "/ ", Role::Muted);
            let x = draw::text(c, rect, x - rect.x, 3, self.filter.text(), Role::Accent);
            draw::text(c, rect, x - rect.x, 3, &format!("  {} of {}", items.len(), ELEMENTS.len()), Role::Muted);
        } else {
            draw::text(c, rect, 0, 3, &format!("{} elements: / filters them", ELEMENTS.len()), Role::Muted);
        }
        let (_, below) = rect.split_top(5);
        let (list, _) = below.split_left(below.w.min(36));
        self.list.render(&items, list, c);
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        match self.typing.clone() {
            Typing::Name { was } => match self.name.handle_press(press) {
                Outcome::Submit => {
                    self.done();
                    Reply::Say(format!("Kept the name: {}", self.name.text()))
                }
                Outcome::Cancel => {
                    self.name.set_text(&was);
                    self.done();
                    Reply::Done
                }
                // Typing takes every key, though some do nothing.
                _ => Reply::Done,
            },
            Typing::Filter => {
                match self.filter.handle_press(press) {
                    Outcome::Edited => self.list.selected = 0,
                    Outcome::Submit => self.done(),
                    Outcome::Cancel => {
                        self.filter.set_text("");
                        self.done();
                    }
                    _ => {}
                }
                Reply::Done
            }
            Typing::No => match press.key {
                fenix_ui_kit::Key::Char('e') => {
                    self.start(Typing::Name { was: self.name.text().to_string() });
                    Reply::Done
                }
                fenix_ui_kit::Key::Char('/') => {
                    self.start(Typing::Filter);
                    Reply::Done
                }
                key => {
                    let items = self.items();
                    match self.list.handle_key(key, &items) {
                        Outcome::Submit => match self.list.selected_node(&items) {
                            Some(n) => Reply::Say(format!("{}: {}", n.label, n.detail.clone().unwrap_or_default())),
                            None => Reply::Done,
                        },
                        outcome => Reply::from(outcome),
                    }
                }
            },
        }
    }

    /// A field takes the keyboard: drawn as a panel with its caret, and
    /// the page sent as typing, so Space, q and Esc are its.
    fn start(&mut self, typing: Typing) {
        self.name.focused = matches!(typing, Typing::Name { .. });
        self.filter.focused = typing == Typing::Filter;
        self.typing = typing;
    }

    fn done(&mut self) {
        self.name.focused = false;
        self.filter.focused = false;
        self.typing = Typing::No;
    }

    /// A paste goes into the field typed in.
    pub fn paste(&mut self, text: &str) {
        match self.typing {
            Typing::Name { .. } => {
                self.name.paste(text);
            }
            Typing::Filter => {
                if self.filter.paste(text) == Outcome::Edited {
                    self.list.selected = 0;
                }
            }
            Typing::No => {}
        }
    }

    /// A click on the name starts editing it there; on the list, picks a
    /// row.
    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        if self.name.char_at(m.x, m.y).is_some() {
            if !matches!(self.typing, Typing::Name { .. }) {
                self.start(Typing::Name { was: self.name.text().to_string() });
            }
            self.name.mouse(m);
            return Reply::Done;
        }
        if self.typing == Typing::Filter {
            return Reply::from(self.filter.mouse(m));
        }
        let items = self.items();
        Reply::from(self.list.mouse(m, &items))
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
