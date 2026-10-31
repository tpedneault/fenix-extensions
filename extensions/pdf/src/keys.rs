//! The reader's keys, as Vim reads them: an optional count, then a key,
//! with `g`, `z`, `m`, `'` and `y` waiting for a second. Fenix 1.x's
//! reader's keys, kept; the pane's tab keys (`gt`, `gT`) are Fenix's, which
//! hears them before the reader does.

use fenix_extension_api::{KeyEvent, NamedKey, SurfaceKey};

/// A key as the reader reads it: a character or a named key, and whether
/// Ctrl was held (with Alt or the Windows key, it isn't the reader's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Press {
    pub key: SurfaceKey,
    pub ctrl: bool,
    pub other: bool,
}

impl Press {
    pub fn char(c: char) -> Press {
        Press { key: SurfaceKey::Char(c), ctrl: false, other: false }
    }

    pub fn named(n: NamedKey) -> Press {
        Press { key: SurfaceKey::Named(n), ctrl: false, other: false }
    }

    pub fn ctrl(c: char) -> Press {
        Press { key: SurfaceKey::Char(c), ctrl: true, other: false }
    }

    pub fn of(event: &KeyEvent) -> Press {
        Press { key: event.key, ctrl: event.mods.ctrl, other: event.mods.alt || event.mods.meta }
    }

    pub fn plain(&self) -> bool {
        !self.ctrl && !self.other
    }

    /// The character, when it's one typed with nothing held.
    pub fn plain_char(&self) -> Option<char> {
        match self.key {
            SurfaceKey::Char(c) if self.plain() => Some(c),
            _ => None,
        }
    }
}

/// What a key asks for. Counts are already folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// `j`/`k`: scroll by this many steps (negative is up).
    Scroll(i32),
    /// `Ctrl-d`/`Ctrl-u`: this many half screens.
    HalfScreen(i32),
    /// `Ctrl-f`/`Ctrl-b`, `PageDown`/`PageUp`: this many screens.
    Screen(i32),
    /// `h`/`l`: pan sideways by this many steps.
    Pan(i32),
    /// `J`/`K`: this many pages on, each landing at the page's top.
    Page(i32),
    /// `gg`, `{n}G`, `{n}gg`: page n, counting from 1.
    GotoPage(u32),
    /// `G` with no count.
    LastPage,
    ZoomIn,
    ZoomOut,
    /// `=`: fit width and fit page, in turn.
    ToggleFit,
    FitWidth,
    FitPage,
    /// `z0`: 100%.
    ActualSize,
    /// `o`: the outline.
    Outline,
    /// `/`: search.
    Search,
    /// `n`/`N`: the next or previous match of the last search.
    NextMatch,
    PrevMatch,
    /// `m{a}`: a mark here.
    SetMark(char),
    /// `'{a}` or `` `{a} ``: back to a mark.
    GotoMark(char),
    /// `Esc` with nothing typed: a selection cleared, the search's
    /// highlights hidden, or the outline closed.
    Escape,
    /// `yp`: copy a link to this page (`file.pdf#page=38`).
    CopyLink,
    /// `v`: select text with the keys.
    Visual,
    /// `f`: a label on every link in sight, to follow one by typing it.
    Hints,
    /// `Ctrl-o` and `Ctrl-i`: back where a jump came from, and on again.
    JumpBack,
    JumpForward,
}

/// What `Keys::key` made of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Taken, waiting for more (a count, or a second key).
    Pending,
    /// Taken and understood.
    Run(Cmd),
    /// Taken and dropped: a prefix followed by a key that means nothing
    /// after it, or `Esc` clearing a count.
    Dropped,
    /// Not the reader's.
    Pass,
}

#[derive(Debug, Default, Clone)]
pub struct Keys {
    count: Option<u32>,
    prefix: Option<char>,
}

impl Keys {
    /// Whether a count or a prefix is waiting for its next key.
    pub fn is_pending(&self) -> bool {
        self.count.is_some() || self.prefix.is_some()
    }

    pub fn reset(&mut self) {
        *self = Keys::default();
    }

    /// The count typed so far, for the modeline.
    pub fn pending_text(&self) -> String {
        let mut text = self.count.map(|c| c.to_string()).unwrap_or_default();
        if let Some(p) = self.prefix {
            text.push(p);
        }
        text
    }

    pub fn key(&mut self, key: Press) -> Outcome {
        let count = self.count;
        let n = count.unwrap_or(1).min(i32::MAX as u32) as i32;
        if let Some(prefix) = self.prefix.take() {
            self.count = None;
            let Some(c) = key.plain_char() else { return Outcome::Dropped };
            let cmd = match (prefix, c) {
                ('g', 'g') => Cmd::GotoPage(count.unwrap_or(1)),
                ('z', 'w') => Cmd::FitWidth,
                ('z', 'p') => Cmd::FitPage,
                ('z', '0') => Cmd::ActualSize,
                ('m', c) if c.is_ascii_alphabetic() => Cmd::SetMark(c),
                ('\'', c) if c.is_ascii_alphabetic() => Cmd::GotoMark(c),
                ('y', 'p') => Cmd::CopyLink,
                _ => return Outcome::Dropped,
            };
            return Outcome::Run(cmd);
        }

        if key.ctrl && !key.other {
            let cmd = match key.key {
                SurfaceKey::Char('d') => Cmd::HalfScreen(n),
                SurfaceKey::Char('u') => Cmd::HalfScreen(-n),
                SurfaceKey::Char('f') => Cmd::Screen(n),
                SurfaceKey::Char('b') => Cmd::Screen(-n),
                SurfaceKey::Char('o') => Cmd::JumpBack,
                SurfaceKey::Char('i') => Cmd::JumpForward,
                _ => {
                    self.count = None;
                    return Outcome::Pass;
                }
            };
            self.count = None;
            return Outcome::Run(cmd);
        }
        if !key.plain() {
            self.count = None;
            return Outcome::Pass;
        }

        let cmd = match key.key {
            SurfaceKey::Char(c @ '1'..='9') => return self.digit(c),
            SurfaceKey::Char('0') if count.is_some() => return self.digit('0'),
            SurfaceKey::Char(c @ ('g' | 'z' | 'm' | 'y' | '\'')) => {
                self.prefix = Some(c);
                return Outcome::Pending;
            }
            SurfaceKey::Char('`') => {
                self.prefix = Some('\'');
                return Outcome::Pending;
            }
            SurfaceKey::Char('j') | SurfaceKey::Named(NamedKey::Down) => Cmd::Scroll(n),
            SurfaceKey::Char('k') | SurfaceKey::Named(NamedKey::Up) => Cmd::Scroll(-n),
            SurfaceKey::Char('h') | SurfaceKey::Named(NamedKey::Left) => Cmd::Pan(-n),
            SurfaceKey::Char('l') | SurfaceKey::Named(NamedKey::Right) => Cmd::Pan(n),
            SurfaceKey::Named(NamedKey::PageDown) => Cmd::Screen(n),
            SurfaceKey::Named(NamedKey::PageUp) => Cmd::Screen(-n),
            SurfaceKey::Char('J') => Cmd::Page(n),
            SurfaceKey::Char('K') => Cmd::Page(-n),
            SurfaceKey::Char('G') => count.map(Cmd::GotoPage).unwrap_or(Cmd::LastPage),
            SurfaceKey::Named(NamedKey::Home) => Cmd::GotoPage(1),
            SurfaceKey::Named(NamedKey::End) => Cmd::LastPage,
            SurfaceKey::Char('+') => Cmd::ZoomIn,
            SurfaceKey::Char('-') => Cmd::ZoomOut,
            SurfaceKey::Char('=') => Cmd::ToggleFit,
            SurfaceKey::Char('o') => Cmd::Outline,
            SurfaceKey::Char('v') => Cmd::Visual,
            SurfaceKey::Char('f') => Cmd::Hints,
            SurfaceKey::Char('/') => Cmd::Search,
            SurfaceKey::Char('n') => Cmd::NextMatch,
            SurfaceKey::Char('N') => Cmd::PrevMatch,
            SurfaceKey::Named(NamedKey::Escape) if self.is_pending() => {
                self.reset();
                return Outcome::Dropped;
            }
            SurfaceKey::Named(NamedKey::Escape) => Cmd::Escape,
            _ => {
                self.count = None;
                return Outcome::Pass;
            }
        };
        self.count = None;
        Outcome::Run(cmd)
    }

    fn digit(&mut self, c: char) -> Outcome {
        let d = c.to_digit(10).unwrap_or(0);
        self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d).min(99_999));
        Outcome::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(keys: &mut Keys, text: &str) -> Vec<Outcome> {
        text.chars().map(|c| keys.key(Press::char(c))).collect()
    }

    fn last(text: &str) -> Outcome {
        *feed(&mut Keys::default(), text).last().unwrap()
    }

    #[test]
    fn counts_repeat_a_motion() {
        assert_eq!(last("j"), Outcome::Run(Cmd::Scroll(1)));
        assert_eq!(last("5j"), Outcome::Run(Cmd::Scroll(5)));
        assert_eq!(last("12k"), Outcome::Run(Cmd::Scroll(-12)));
        assert_eq!(last("3J"), Outcome::Run(Cmd::Page(3)));
        assert_eq!(last("K"), Outcome::Run(Cmd::Page(-1)));
    }

    #[test]
    fn g_and_capital_g_go_to_pages_as_in_vim() {
        assert_eq!(last("gg"), Outcome::Run(Cmd::GotoPage(1)));
        assert_eq!(last("G"), Outcome::Run(Cmd::LastPage));
        assert_eq!(last("38G"), Outcome::Run(Cmd::GotoPage(38)));
        assert_eq!(last("104G"), Outcome::Run(Cmd::GotoPage(104)), "0 continues a count");
        assert_eq!(last("7gg"), Outcome::Run(Cmd::GotoPage(7)));
        assert_eq!(last("gx"), Outcome::Dropped);
    }

    #[test]
    fn zoom_keys() {
        assert_eq!(last("+"), Outcome::Run(Cmd::ZoomIn));
        assert_eq!(last("-"), Outcome::Run(Cmd::ZoomOut));
        assert_eq!(last("="), Outcome::Run(Cmd::ToggleFit));
        assert_eq!(last("zw"), Outcome::Run(Cmd::FitWidth));
        assert_eq!(last("zp"), Outcome::Run(Cmd::FitPage));
        assert_eq!(last("z0"), Outcome::Run(Cmd::ActualSize));
    }

    #[test]
    fn yp_copies_a_link_and_marks_are_set_and_gone_to() {
        assert_eq!(last("yp"), Outcome::Run(Cmd::CopyLink));
        assert_eq!(last("yy"), Outcome::Dropped);
        assert_eq!(last("ma"), Outcome::Run(Cmd::SetMark('a')));
        assert_eq!(last("'a"), Outcome::Run(Cmd::GotoMark('a')));
        assert_eq!(last("`b"), Outcome::Run(Cmd::GotoMark('b')));
        assert_eq!(last("m1"), Outcome::Dropped);
    }

    #[test]
    fn the_old_single_keys_are_gone() {
        for key in ["p", "w", "0"] {
            assert_eq!(last(key), Outcome::Pass, "{key}");
        }
        assert_eq!(last("n"), Outcome::Run(Cmd::NextMatch), "n is the next match, not the next page");
    }

    #[test]
    fn ctrl_keys_scroll_by_screens_jump_and_the_rest_pass_on() {
        let mut keys = Keys::default();
        assert_eq!(keys.key(Press::ctrl('d')), Outcome::Run(Cmd::HalfScreen(1)));
        assert_eq!(keys.key(Press::ctrl('b')), Outcome::Run(Cmd::Screen(-1)));
        feed(&mut keys, "2");
        assert_eq!(keys.key(Press::ctrl('f')), Outcome::Run(Cmd::Screen(2)));
        assert_eq!(keys.key(Press::ctrl('o')), Outcome::Run(Cmd::JumpBack));
        assert_eq!(keys.key(Press::ctrl('i')), Outcome::Run(Cmd::JumpForward));
        assert_eq!(keys.key(Press::ctrl('x')), Outcome::Pass);
        assert_eq!(keys.key(Press::named(NamedKey::PageDown)), Outcome::Run(Cmd::Screen(1)));
    }

    #[test]
    fn esc_clears_a_count_then_is_the_readers() {
        let mut keys = Keys::default();
        feed(&mut keys, "4");
        assert_eq!(keys.pending_text(), "4");
        assert_eq!(keys.key(Press::named(NamedKey::Escape)), Outcome::Dropped, "Esc clears a count");
        assert_eq!(keys.key(Press::named(NamedKey::Escape)), Outcome::Run(Cmd::Escape));
        feed(&mut keys, "4");
        assert_eq!(keys.key(Press::char('x')), Outcome::Pass);
        assert!(!keys.is_pending(), "a key not the reader's clears the count");
    }
}
