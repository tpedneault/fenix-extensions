//! What the MIB's pages are laid out with: the extension API's grid,
//! under the names Fenix's own pages used, and their keys -- Fenix's
//! page keys, Space among them, read from what a page is sent.

pub use fenix_extension_api::grid::{fit, fit_tail, frame, insert_at, with_caret, wrap, Grid, Popup};
pub use fenix_extension_api::{Canvas as Page, Role};

/// A key on one of the MIB's pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    BackTab,
    Escape,
    Space,
    Backspace,
    Char(char),
    /// While a field is typed: the caret to the start, the end.
    Home,
    End,
    /// While a field is typed: the character after the caret.
    Delete,
    /// While a field is typed (`Ctrl-←`, `Ctrl-→`): a word back, forward.
    WordLeft,
    WordRight,
    /// While a field is typed (`Ctrl-Backspace`): the word before the caret.
    DeleteWordBack,
}

impl From<fenix_extension_api::Key> for Key {
    fn from(key: fenix_extension_api::Key) -> Key {
        use fenix_extension_api::Key as K;
        match key {
            K::Up => Key::Up,
            K::Down => Key::Down,
            K::Left => Key::Left,
            K::Right => Key::Right,
            K::Enter => Key::Enter,
            K::Tab => Key::Tab,
            K::BackTab => Key::BackTab,
            K::Escape => Key::Escape,
            K::Backspace => Key::Backspace,
            K::Delete => Key::Delete,
            K::Home => Key::Home,
            K::End => Key::End,
            K::WordLeft => Key::WordLeft,
            K::WordRight => Key::WordRight,
            K::DeleteWordBack => Key::DeleteWordBack,
            K::Char(' ') => Key::Space,
            K::Char(c) => Key::Char(c),
        }
    }
}

impl From<Key> for fenix_extension_api::Key {
    fn from(key: Key) -> fenix_extension_api::Key {
        use fenix_extension_api::Key as K;
        match key {
            Key::Up => K::Up,
            Key::Down => K::Down,
            Key::Left => K::Left,
            Key::Right => K::Right,
            Key::Enter => K::Enter,
            Key::Tab => K::Tab,
            Key::BackTab => K::BackTab,
            Key::Escape => K::Escape,
            Key::Space => K::Char(' '),
            Key::Backspace => K::Backspace,
            Key::Char(c) => K::Char(c),
            Key::Home => K::Home,
            Key::End => K::End,
            Key::Delete => K::Delete,
            Key::WordLeft => K::WordLeft,
            Key::WordRight => K::WordRight,
            Key::DeleteWordBack => K::DeleteWordBack,
        }
    }
}

/// A filter typed after `/`, as the extension API keeps one, taking the
/// pages' keys.
#[derive(Debug, Clone, Default)]
pub struct Filter(fenix_extension_api::grid::Filter);

impl std::ops::Deref for Filter {
    type Target = fenix_extension_api::grid::Filter;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Filter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Filter {
    /// A key while it's typed: whether it took it.
    pub fn key(&mut self, key: Key) -> bool {
        self.0.key(key.into())
    }
}

/// A key typed into a one-line field: whether it changed it or moved the
/// caret.
pub fn edit_line(text: &mut String, caret: &mut usize, key: Key) -> bool {
    fenix_extension_api::grid::edit_line(text, caret, key.into())
}
