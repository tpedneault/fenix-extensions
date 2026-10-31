//! A page laid out cell by cell: its text and each cell's style, the
//! hairlines, panels and focused row drawn over it, its pictures and its
//! popup. What a page's widgets render into, and what goes to Fenix.

use std::ops::Range;

use crate::text::char_width;
use crate::{Fill, Rect, Role, Style};

/// What's in a cell.
#[derive(Debug, Clone, PartialEq)]
enum Glyph {
    Blank,
    /// A character, and the combining marks on it.
    Text(String),
    /// The right half of the wide character left of it.
    Wide,
}

#[derive(Debug, Clone, PartialEq)]
struct Cell {
    glyph: Glyph,
    style: Style,
}

const BLANK: Cell = Cell { glyph: Glyph::Blank, style: Style { fg: Role::Text, bg: None, underline: false } };

/// A picture over the canvas (`image-load`, `image-file`), in the cells
/// of `rect`.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub rect: Rect,
    pub image: u32,
    /// Zoomed and panned: (zoom, the centre's x and y as fractions).
    pub view: Option<(f32, f32, f32)>,
    pub marks: Vec<Mark>,
}

/// A box outlined over a picture, in fractions of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub selected: bool,
}

/// A menu or question in Fenix's popup box, beside line `line`, its text
/// lined up with column `col`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Popup {
    pub line: usize,
    pub col: usize,
    pub rows: Vec<Vec<(String, Style)>>,
}

/// A page being laid out. Lines and columns grow as they're written to;
/// columns are display cells.
#[derive(Debug, Clone, Default)]
pub struct Canvas {
    rows: Vec<Vec<Cell>>,
    rules: Vec<(usize, Range<usize>)>,
    panels: Vec<(usize, Range<usize>)>,
    focus: Option<(usize, Range<usize>)>,
    pictures: Vec<Picture>,
    popup: Option<Popup>,
    typing: bool,
    vrules: Vec<(usize, Range<usize>)>,
    more_keys: bool,
    mouse: bool,
}

impl Canvas {
    pub fn new() -> Canvas {
        Canvas::default()
    }

    fn row_mut(&mut self, y: usize, width: usize) -> &mut Vec<Cell> {
        if self.rows.len() <= y {
            self.rows.resize(y + 1, Vec::new());
        }
        let row = &mut self.rows[y];
        if row.len() < width {
            row.resize(width, BLANK);
        }
        row
    }

    /// Cell `x` no longer half of a wide character: its other half goes
    /// blank.
    fn unhook(row: &mut [Cell], x: usize) {
        match &row[x].glyph {
            Glyph::Wide if x > 0 => row[x - 1].glyph = Glyph::Blank,
            Glyph::Text(_) if row.get(x + 1).is_some_and(|c| c.glyph == Glyph::Wide) => row[x + 1].glyph = Glyph::Blank,
            _ => {}
        }
    }

    /// Writes `text` at (`x`, `y`), over what was there; returns the
    /// column after it. Tabs are spaces, other controls are left out.
    pub fn put(&mut self, x: usize, y: usize, text: &str, style: impl Into<Style>) -> usize {
        let style = style.into();
        let mut col = x;
        let row = self.row_mut(y, 0);
        for c in text.chars() {
            let c = if c == '\t' { ' ' } else { c };
            if c.is_control() {
                continue;
            }
            let w = char_width(c);
            if w == 0 {
                // A combining mark: on the character before it.
                let mut at = col;
                while at > x {
                    at -= 1;
                    if let Glyph::Text(s) = &mut row[at].glyph {
                        s.push(c);
                        break;
                    }
                }
                continue;
            }
            if row.len() < col + w {
                row.resize(col + w, BLANK);
            }
            Self::unhook(row, col);
            if w == 2 {
                Self::unhook(row, col + 1);
                row[col + 1] = Cell { glyph: Glyph::Wide, style: style.clone() };
            }
            row[col] = Cell { glyph: Glyph::Text(c.to_string()), style: style.clone() };
            col += w;
        }
        col
    }

    /// `text` cut to `w` cells (`…` where it's cut), at (`x`, `y`).
    pub fn put_fit(&mut self, x: usize, y: usize, w: usize, text: &str, style: impl Into<Style>) -> usize {
        self.put(x, y, &crate::text::fit(text, w), style)
    }

    /// Sets the style of cells `cols` of line `y`, keeping their text.
    pub fn style(&mut self, y: usize, cols: Range<usize>, style: impl Into<Style>) {
        let style = style.into();
        let row = self.row_mut(y, cols.end);
        for cell in &mut row[cols] {
            cell.style = style.clone();
        }
    }

    /// Blanks `rect`.
    pub fn clear(&mut self, rect: Rect) {
        for y in rect.y..rect.bottom() {
            let row = self.row_mut(y, rect.right());
            Self::unhook(row, rect.x);
            if rect.right() < row.len() {
                Self::unhook(row, rect.right());
            }
            for cell in &mut row[rect.x..rect.right()] {
                *cell = BLANK;
            }
        }
    }

    /// A hairline over cells `cols` of line `y`.
    pub fn rule(&mut self, y: usize, cols: Range<usize>) {
        self.row_mut(y, 0);
        self.rules.push((y, cols));
    }

    /// A hairline down column `x`, lines `ys`, over blank cells (0.8.2's
    /// `vertical-rules`; a page sent as 0.8.0 leaves it out -- draw a
    /// [`Canvas::vline`] for that).
    pub fn vrule(&mut self, x: usize, ys: Range<usize>) {
        if ys.end > 0 {
            self.row_mut(ys.end - 1, 0);
        }
        self.vrules.push((x, ys));
    }

    pub fn vrules(&self) -> &[(usize, Range<usize>)] {
        &self.vrules
    }

    /// Cells `cols` of line `y` on `fill`, keeping their text and colour;
    /// blank cells are made where there were none, so the background
    /// reaches past the text.
    pub fn fill(&mut self, y: usize, cols: Range<usize>, fill: Fill) {
        let row = self.row_mut(y, cols.end);
        for cell in &mut row[cols] {
            cell.style.bg = Some(fill);
        }
    }

    /// Every cell of `rect` on `fill`: a block of colour.
    pub fn fill_rect(&mut self, rect: Rect, fill: Fill) {
        for y in rect.y..rect.bottom() {
            self.fill(y, rect.x..rect.right(), fill);
        }
    }

    /// The page takes the keys Fenix doesn't give `page-key` (0.8.2's
    /// `more-keys`): PageUp, Shift with the arrows, Ctrl with letters.
    pub fn set_more_keys(&mut self, more: bool) {
        self.more_keys = more;
    }

    pub fn more_keys(&self) -> bool {
        self.more_keys
    }

    /// Clicks and the wheel on the page come to it (0.8.2's `mouse`).
    pub fn set_mouse(&mut self, mouse: bool) {
        self.mouse = mouse;
    }

    pub fn mouse(&self) -> bool {
        self.mouse
    }

    /// A line of `│` down column `x`, lines `ys`: a divider.
    pub fn vline(&mut self, x: usize, ys: Range<usize>, style: impl Into<Style>) {
        let style = style.into();
        for y in ys {
            self.put(x, y, "│", style.clone());
        }
    }

    /// Cells `cols` of line `y` tinted as a panel: a field being typed in.
    pub fn panel(&mut self, y: usize, cols: Range<usize>) {
        self.row_mut(y, 0);
        self.panels.push((y, cols));
    }

    /// Line `y` across `cols` as the focused row: tinted, kept in view.
    /// A canvas has one; the last set wins.
    pub fn focus(&mut self, y: usize, cols: Range<usize>) {
        self.row_mut(y, 0);
        self.focus = Some((y, cols));
    }

    /// Image `image` in the cells of `rect`, over cells left blank.
    pub fn picture(&mut self, rect: Rect, image: u32, view: Option<(f32, f32, f32)>, marks: Vec<Mark>) {
        if rect.h > 0 {
            self.row_mut(rect.bottom() - 1, 0);
        }
        self.pictures.push(Picture { rect, image, view, marks });
    }

    pub fn popup(&mut self, popup: Popup) {
        self.popup = Some(popup);
    }

    /// A field has the keyboard: the page is to be sent with `typing`.
    pub fn set_typing(&mut self, typing: bool) {
        self.typing = typing;
    }

    pub fn typing(&self) -> bool {
        self.typing
    }

    /// How many lines have been written.
    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// Line `y`'s text, trailing blanks dropped -- but not those on a
    /// background, which is drawn behind them.
    pub fn line(&self, y: usize) -> String {
        let Some(row) = self.rows.get(y) else { return String::new() };
        let mut s = String::new();
        // Up to the last cell on a background (a panel's is drawn anyway).
        let mut keep = 0;
        for cell in row {
            match &cell.glyph {
                Glyph::Blank => s.push(' '),
                Glyph::Text(t) => s.push_str(t),
                Glyph::Wide => {}
            }
            if cell.style.bg.is_some_and(|f| f != Fill::Panel) {
                keep = s.len();
            }
        }
        s.truncate(s.trim_end().len().max(keep));
        s
    }

    /// Its text: its lines, a newline between them.
    pub fn text(&self) -> String {
        (0..self.rows.len()).map(|y| self.line(y)).collect::<Vec<_>>().join("\n")
    }

    /// Cell (`x`, `y`)'s style; plain past what's written.
    pub fn style_at(&self, x: usize, y: usize) -> Style {
        self.rows.get(y).and_then(|r| r.get(x)).map(|c| c.style.clone()).unwrap_or_default()
    }

    /// Runs of cells in one style, plain text left out: (line, columns,
    /// style).
    pub fn spans(&self) -> Vec<(usize, Range<usize>, Style)> {
        let mut out = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            let mut x = 0;
            while x < row.len() {
                let style = &row[x].style;
                let mut end = x + 1;
                while end < row.len() && row[end].style == *style {
                    end += 1;
                }
                if !style.is_plain() {
                    out.push((y, x..end, style.clone()));
                }
                x = end;
            }
        }
        out
    }

    pub fn rules(&self) -> &[(usize, Range<usize>)] {
        &self.rules
    }

    /// Its panels: those asked for, and the cells filled as one.
    pub fn panels(&self) -> Vec<(usize, Range<usize>)> {
        let mut out = self.panels.clone();
        for (y, row) in self.rows.iter().enumerate() {
            let mut x = 0;
            while x < row.len() {
                if row[x].style.bg == Some(Fill::Panel) {
                    let start = x;
                    while x < row.len() && row[x].style.bg == Some(Fill::Panel) {
                        x += 1;
                    }
                    out.push((y, start..x));
                } else {
                    x += 1;
                }
            }
        }
        out
    }

    /// Runs of cells on a background, panels among them: (line, columns,
    /// fill).
    pub fn backgrounds(&self) -> Vec<(usize, Range<usize>, Fill)> {
        self.runs(|s| s.bg)
    }

    /// Runs of underlined cells, by the colour of the line: (line,
    /// columns, role).
    pub fn underlines(&self) -> Vec<(usize, Range<usize>, Role)> {
        self.runs(|s| s.underline.then(|| s.fg.clone()))
    }

    /// Runs of cells that `key` gives the same value, none left out.
    fn runs<T: PartialEq>(&self, key: impl Fn(&Style) -> Option<T>) -> Vec<(usize, Range<usize>, T)> {
        let mut out = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            let mut x = 0;
            while x < row.len() {
                let Some(k) = key(&row[x].style) else {
                    x += 1;
                    continue;
                };
                let start = x;
                x += 1;
                while x < row.len() && key(&row[x].style).as_ref() == Some(&k) {
                    x += 1;
                }
                out.push((y, start..x, k));
            }
        }
        out
    }

    pub fn focused(&self) -> Option<&(usize, Range<usize>)> {
        self.focus.as_ref()
    }

    pub fn pictures(&self) -> &[Picture] {
        &self.pictures
    }

    pub fn popup_ref(&self) -> Option<&Popup> {
        self.popup.as_ref()
    }

    /// Column `x` of line `y` as a character offset into the line's text:
    /// what the API's `cells` count.
    pub fn char_offset(&self, y: usize, x: usize) -> usize {
        let row = self.rows.get(y).map(Vec::as_slice).unwrap_or(&[]);
        let within: usize = row[..x.min(row.len())]
            .iter()
            .map(|c| match &c.glyph {
                Glyph::Blank => 1,
                Glyph::Text(t) => t.chars().count(),
                Glyph::Wide => 0,
            })
            .sum();
        within + x.saturating_sub(row.len())
    }

    /// Cells `cols` of line `y` as character offsets.
    pub fn char_range(&self, y: usize, cols: &Range<usize>) -> Range<usize> {
        self.char_offset(y, cols.start)..self.char_offset(y, cols.end.max(cols.start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_written_over_what_was_there() {
        let mut c = Canvas::new();
        assert_eq!(c.put(2, 1, "hello", Role::Title), 7);
        c.put(4, 1, "LL", Role::Accent);
        assert_eq!(c.text(), "\n  heLLo");
        assert_eq!(c.style_at(4, 1), Style::new(Role::Accent));
        assert_eq!(c.spans(), [(1, 2..4, Style::new(Role::Title)), (1, 4..6, Style::new(Role::Accent)), (1, 6..7, Style::new(Role::Title))]);
        c.put(0, 3, "plain", Role::Text);
        assert!(c.spans().iter().all(|s| s.0 != 3), "plain text has no span");
    }

    #[test]
    fn a_wide_character_takes_two_cells() {
        let mut c = Canvas::new();
        assert_eq!(c.put(0, 0, "日本", Role::Text), 4);
        c.put(4, 0, "x", Role::Text);
        assert_eq!(c.line(0), "日本x");
        // Writing over half of one blanks the other half.
        c.put(1, 0, "a", Role::Text);
        assert_eq!(c.line(0), " a本x");
        c.put(2, 0, "b", Role::Text);
        assert_eq!(c.line(0), " ab x");
        let mut c = Canvas::new();
        c.put(0, 0, "e\u{301}t\u{7}\tz", Role::Text);
        assert_eq!(c.line(0), "e\u{301}t z", "the mark on its letter, the bell left out, the tab a space");
    }

    #[test]
    fn columns_become_character_offsets() {
        let mut c = Canvas::new();
        c.put(0, 0, "日本", Role::Text);
        c.put(4, 0, "ok", Role::Good);
        // "日本ok": the span is at chars 2..4, though cells 4..6.
        assert_eq!(c.spans(), [(0, 4..6, Style::new(Role::Good))]);
        assert_eq!(c.char_range(0, &(4..6)), 2..4);
        assert_eq!(c.char_offset(0, 10), 8, "past the text, a cell a char");
    }

    #[test]
    fn rules_panels_and_focus_are_kept() {
        let mut c = Canvas::new();
        c.rule(2, 0..10);
        c.panel(3, 1..5);
        c.put(0, 4, "tab", Style::new(Role::Title).bg(Fill::Panel));
        c.focus(5, 0..10);
        assert_eq!(c.height(), 6);
        assert_eq!(c.rules(), [(2, 0..10)]);
        assert_eq!(c.panels(), [(3, 1..5), (4, 0..3)]);
        assert_eq!(c.focused(), Some(&(5, 0..10)));
        c.clear(Rect::new(0, 4, 2, 1));
        assert_eq!(c.line(4), "  b");
    }

    #[test]
    fn backgrounds_and_underlines_come_in_runs() {
        let mut c = Canvas::new();
        c.put(0, 0, "ab", Style::new(Role::Title).bg(Fill::Rgb(255, 0, 0)));
        c.put(2, 0, "cd", Style::new(Role::Muted).bg(Fill::Rgb(255, 0, 0)).underline());
        c.put(4, 0, "e", Style::new(Role::Muted).underline());
        assert_eq!(c.backgrounds(), [(0, 0..4, Fill::Rgb(255, 0, 0))], "one run, whatever the colour of the text");
        assert_eq!(c.underlines(), [(0, 2..5, Role::Muted)]);
        c.fill_rect(Rect::new(6, 1, 3, 2), Fill::Good);
        assert_eq!(c.backgrounds()[1..], [(1, 6..9, Fill::Good), (2, 6..9, Fill::Good)]);
        assert_eq!(c.line(1), "         ", "blanks on a background are kept, for it to be drawn behind");
        c.put(0, 2, "x", Role::Text);
        c.fill(2, 1..2, Fill::Panel);
        assert_eq!(c.line(2), "x        ");
        let mut c = Canvas::new();
        c.put(0, 0, "x", Role::Text);
        c.fill(0, 1..3, Fill::Panel);
        assert_eq!(c.line(0), "x", "a panel is drawn whatever the text");
    }

    #[test]
    fn vertical_rules_and_the_flags_are_kept() {
        let mut c = Canvas::new();
        c.vrule(4, 0..3);
        c.set_more_keys(true);
        c.set_mouse(true);
        assert_eq!((c.vrules(), c.height()), (&[(4, 0..3)][..], 3));
        assert!(c.more_keys() && c.mouse());
    }
}
