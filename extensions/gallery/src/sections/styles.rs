//! Styles: the theme's roles and tints, underlines, the palette, and text
//! measured in cells.

// gallery:begin
use fenix_ui_kit::text::{fit, fit_tail, slice, width, wrap};
use fenix_ui_kit::{Canvas, Fill, Key, KeyPress, Mouse, Rect, Role, Style};

use crate::draw;
use crate::sections::Reply;

pub struct State {
    /// The theme's eight colours, `theme_palette()`'s.
    pub palette: Vec<(u8, u8, u8)>,
    /// How wide the text helpers cut to.
    pub width: usize,
}

/// A line with wide characters (東京 takes four cells) and a combining
/// one.
const SAMPLE: &str = "Brest → 東京 → Sa\u{303}o Paulo, by sea";

/// Where each label ends, and the swatches or samples start.
const LABEL: usize = 11;
/// Where the text helpers' results start, after their calls.
const CALL: usize = 18;

impl State {
    pub fn new() -> State {
        State { palette: Vec::new(), width: 16 }
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("+ -", "the text helpers wider, narrower")]
    }

    /// `items` along line `row` after its label, a space between.
    fn row(c: &mut Canvas, rect: Rect, row: usize, label: &str, items: &[(&str, Style)]) {
        draw::text(c, rect, 0, row, label, Role::Muted);
        let mut x = LABEL;
        for (text, style) in items {
            x = draw::text(c, rect, x, row, text, style) - rect.x + 2;
        }
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let roles = [("Title", Role::Title), ("Text", Role::Text), ("Muted", Role::Muted), ("Accent", Role::Accent), ("Good", Role::Good), ("Warn", Role::Warn), ("Bad", Role::Bad)];
        let items: Vec<(&str, Style)> = roles.iter().map(|(n, r)| (*n, Style::new(r.clone()))).collect();
        Self::row(c, rect, 0, "Role", &items);
        let syntax = ["keyword", "string", "comment", "function", "type", "number", "constant.builtin"];
        let items: Vec<(&str, Style)> = syntax.iter().map(|n| (*n, Style::new(Role::Syntax(n.to_string())))).collect();
        Self::row(c, rect, 1, "Syntax", &items);
        let fills = [(" Panel ", Fill::Panel), (" Focus ", Fill::Focus), (" Selection ", Fill::Selection), (" Accent ", Fill::Accent), (" Good ", Fill::Good), (" Warn ", Fill::Warn), (" Bad ", Fill::Bad)];
        let items: Vec<(&str, Style)> = fills.iter().map(|(n, f)| (*n, Style::new(Role::Text).bg(*f))).collect();
        Self::row(c, rect, 3, "Fill", &items);
        let lines = [("underlined", Style::new(Role::Text).underline()), ("in the accent", Style::new(Role::Accent).underline()), ("a problem", Style::new(Role::Bad).underline())];
        Self::row(c, rect, 4, "Underline", &lines);

        // The palette: a swatch of each colour and its hex, four a line.
        draw::text(c, rect, 0, 6, "Palette", Role::Muted);
        if self.palette.is_empty() {
            draw::text(c, rect, LABEL, 6, "(the theme gives none)", Role::Muted);
        }
        for (i, &(r, g, b)) in self.palette.iter().enumerate() {
            let (row, x) = (6 + i / 4, LABEL + (i % 4) * 13);
            if row < rect.h && x + 3 <= rect.w {
                c.fill_rect(Rect::new(rect.x + x, rect.y + row, 3, 1), Fill::Rgb(r, g, b));
                draw::text(c, rect, x + 4, row, &format!("#{r:02x}{g:02x}{b:02x}"), Role::Text);
            }
        }

        // Text in cells: cut, cut from the front, a slice, wrapped.
        let w = self.width;
        draw::text(c, rect, 0, 9, "Text", Role::Muted);
        draw::text(c, rect, LABEL, 9, &format!("s = \"{SAMPLE}\": {} chars in {} cells; w = {w}", SAMPLE.chars().count(), width(SAMPLE)), Role::Text);
        let ruler = format!("├{}┤", "─".repeat(w.saturating_sub(2)));
        draw::text(c, rect, CALL, 10, &ruler, Role::Muted);
        let shown = [("fit(s, w)", fit(SAMPLE, w)), ("fit_tail(s, w)", fit_tail(SAMPLE, w)), ("slice(s, 6, w)", slice(SAMPLE, 6, w))];
        for (i, (call, text)) in shown.iter().enumerate() {
            draw::text(c, rect, 2, 11 + i, call, Role::Syntax("function".into()));
            draw::text(c, rect, CALL, 11 + i, text, Role::Text);
        }
        draw::text(c, rect, 2, 14, "wrap(s, w)", Role::Syntax("function".into()));
        for (i, line) in wrap(SAMPLE, w).iter().enumerate() {
            draw::text(c, rect, CALL, 14 + i, line, Role::Text);
        }
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        match press.key {
            Key::Char('+') => self.width = (self.width + 2).min(40),
            Key::Char('-') => self.width = self.width.saturating_sub(2).max(2),
            _ => return Reply::Ignored,
        }
        Reply::Done
    }

    pub fn mouse(&mut self, _m: &Mouse) -> Reply {
        Reply::Ignored
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
