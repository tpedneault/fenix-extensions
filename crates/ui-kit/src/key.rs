//! The keys widgets answer.

/// A key on a page, as the API's `key` has them, and `PageUp` and
/// `PageDown`, which come with `page-key-press` (0.8.2; `J` and `K` page
/// too).
///
/// Space is `Char(' ')`: a page gets it while typing, or with `space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    BackTab,
    Escape,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// Ctrl+Left, Ctrl+Right, Ctrl+Backspace: a word at a time.
    WordLeft,
    WordRight,
    DeleteWordBack,
    Char(char),
}

/// A key with the keys held with it, from a page that takes more keys
/// (0.8.2's `page-key-press`): Shift with the arrows, Ctrl with letters,
/// PageUp and PageDown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: Key,
    pub mods: crate::Mods,
}

impl KeyPress {
    pub fn new(key: Key, mods: crate::Mods) -> KeyPress {
        KeyPress { key, mods }
    }

    /// Ctrl with letter `c`, Shift or not.
    pub fn is_ctrl(&self, c: char) -> bool {
        self.mods.ctrl && !self.mods.alt && matches!(self.key, Key::Char(k) if k.eq_ignore_ascii_case(&c))
    }
}

impl From<Key> for KeyPress {
    fn from(key: Key) -> KeyPress {
        KeyPress { key, mods: crate::Mods::NONE }
    }
}
