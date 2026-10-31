//! The contrast checker: text in one colour on another, their WCAG 2.x
//! ratio, what it passes for normal and large text and for a control's
//! parts, a preview, and the nearest colour that passes.

use fenix_ui_kit::{Canvas, Fill, Key, KeyPress, Mouse, Outcome, Rect, Role, Style, TextField};

use crate::colour::{contrast, nearest_passing, passes, Colour};
use crate::draw;
use crate::literal::Format;
use crate::picker::{parse_any, Place};

/// One of the two: its colour, the literal it came from (to write it
/// back), and its field.
#[derive(Debug, Clone)]
pub struct Side {
    pub colour: Colour,
    pub from: Option<(Place, Format)>,
    pub field: TextField,
}

impl Side {
    pub fn new(colour: Colour, from: Option<(Place, Format)>) -> Side {
        let mut field = TextField::new();
        field.focused = false;
        let mut s = Side { colour, from, field };
        s.sync();
        s
    }

    fn sync(&mut self) {
        if !self.field.focused {
            self.field.set_text(&Format::Hex { digits: 6, upper: false }.write(&self.colour));
        }
    }

    /// Changed from what its literal says.
    pub fn changed(&self) -> bool {
        self.from.as_ref().is_some_and(|(p, f)| f.write(&self.colour) != p.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    None,
    Redraw,
    /// Write the changed colours back into their literals.
    Write,
}

#[derive(Debug, Clone)]
pub struct Checker {
    pub text: Side,
    pub back: Side,
    /// What a see-through background is over: the editor's.
    pub under: Colour,
    /// 0 the text's colour, 1 the background's.
    pub focus: usize,
    /// What `f` last reached for, to say so.
    pub note: Option<String>,
    fields: std::cell::Cell<[Rect; 2]>,
}

impl Checker {
    pub fn new(text: Side, back: Side, under: Colour) -> Checker {
        Checker { text, back, under, focus: 0, note: None, fields: std::cell::Cell::new([Rect::default(); 2]) }
    }

    pub fn ratio(&self) -> f64 {
        contrast(&self.text.colour, &self.back.colour, &self.under)
    }

    fn side(&mut self) -> &mut Side {
        if self.focus == 0 {
            &mut self.text
        } else {
            &mut self.back
        }
    }

    pub fn typing(&self) -> bool {
        self.text.field.focused || self.back.field.focused
    }

    /// The focused colour lighter or darker, by `by` of OKLCH's lightness.
    fn lighten(&mut self, by: f64) {
        let side = self.side();
        let [l, c, h] = side.colour.oklch();
        side.colour = Colour::from_oklch((l + by).clamp(0.0, 1.0), c, h, side.colour.a).gamut_map();
        side.sync();
    }

    /// The focused colour moved as little as it takes to reach `target`.
    fn fix(&mut self, target: f64, name: &str) {
        let (fg, bg, under) = (self.text.colour, self.back.colour, self.under);
        let found = if self.focus == 0 {
            nearest_passing(&fg, &bg, &under, target)
        } else {
            // The background moved instead: the same, the other way round
            // (contrast doesn't care which is which, alpha aside).
            nearest_passing(&bg.over(&under), &fg.over(&bg.over(&under)), &under, target).map(|c| c.with_alpha(bg.a))
        };
        self.note = Some(match found {
            Some(c) => {
                let side = self.side();
                side.colour = c;
                side.sync();
                format!("moved to pass {name}")
            }
            None => format!("no lightness of it passes {name}"),
        });
    }

    fn field_outcome(&mut self, outcome: Outcome) -> Act {
        let side = self.side();
        match outcome {
            Outcome::Edited => {
                if let Some(c) = parse_any(side.field.text()) {
                    side.colour = c;
                }
            }
            Outcome::Submit | Outcome::Cancel => {
                if outcome == Outcome::Submit {
                    if let Some(c) = parse_any(side.field.text()) {
                        side.colour = c;
                    }
                }
                side.field.focused = false;
                side.sync();
            }
            Outcome::Ignored => return Act::None,
            _ => {}
        }
        Act::Redraw
    }

    /// `Tab` or `j` `k` the other colour; `h` `l` darker and lighter (`H`
    /// `L` more); `s` swaps them; `f` the nearest that passes AA, `F` AAA;
    /// `x` or `e` types one in; Enter writes what changed.
    pub fn key(&mut self, key: Key) -> Act {
        if self.typing() {
            if matches!(key, Key::Tab | Key::BackTab) {
                return self.field_outcome(Outcome::Submit);
            }
            let outcome = self.side().field.handle_key(key);
            return self.field_outcome(outcome);
        }
        self.note = None;
        match key {
            Key::Tab | Key::BackTab | Key::Char('j' | 'k') | Key::Up | Key::Down => self.focus = 1 - self.focus,
            Key::Char('h') | Key::Left => self.lighten(-0.01),
            Key::Char('l') | Key::Right => self.lighten(0.01),
            Key::Char('H') => self.lighten(-0.05),
            Key::Char('L') => self.lighten(0.05),
            Key::Char('s') => {
                std::mem::swap(&mut self.text.colour, &mut self.back.colour);
                self.text.sync();
                self.back.sync();
            }
            Key::Char('f') => self.fix(4.5, "AA"),
            Key::Char('F') => self.fix(7.0, "AAA"),
            Key::Char('x' | 'e' | '#') => {
                let side = self.side();
                side.field.focused = true;
                side.field.select_all();
            }
            Key::Enter if self.text.changed() || self.back.changed() => return Act::Write,
            _ => return Act::None,
        }
        Act::Redraw
    }

    pub fn press(&mut self, press: KeyPress) -> Act {
        if self.typing() {
            let outcome = self.side().field.handle_press(press);
            return self.field_outcome(outcome);
        }
        match (press.key, press.mods.shift) {
            (Key::Left, true) => self.lighten(-0.05),
            (Key::Right, true) => self.lighten(0.05),
            _ => return Act::None,
        }
        Act::Redraw
    }

    pub fn paste(&mut self, text: &str) -> Act {
        let side = self.side();
        side.field.focused = true;
        let outcome = side.field.paste(text);
        self.field_outcome(outcome)
    }

    /// A click on a colour's row picks it, on its field types in it; the
    /// wheel there makes it lighter or darker.
    pub fn mouse(&mut self, m: &Mouse) -> Act {
        let fields = self.fields.get();
        let Some(i) = fields.iter().position(|r| r.y == m.y) else { return Act::None };
        if let Some(lines) = m.wheel() {
            self.focus = i;
            self.lighten(0.01 * f64::from(lines.signum()));
            return Act::Redraw;
        }
        if m.left_clicks() == 0 {
            return Act::None;
        }
        if self.typing() && self.focus != i {
            self.field_outcome(Outcome::Submit);
        }
        self.focus = i;
        if fields[i].contains(m.x, m.y) {
            let side = self.side();
            side.field.focused = true;
            side.field.mouse(m);
        }
        Act::Redraw
    }

    pub fn render(&self, w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        c.set_more_keys(true);
        let page = Rect::new(0, 0, w, h.max(26));
        let body = page.inset_x(2);
        let mut y = 0;
        c.put(body.x, y, "Contrast", Role::Title);
        c.put(body.x + 10, y, "WCAG 2.x", Role::Muted);
        y += 1;
        c.rule(y, page.x..page.right());
        y += 2;
        let mut fields = [Rect::default(); 2];
        for (i, (label, side)) in [("Text", &self.text), ("Background", &self.back)].into_iter().enumerate() {
            let focused = self.focus == i;
            c.put(body.x, y, label, if focused { Role::Accent } else { Role::Muted });
            draw::block(&mut c, Rect::new(body.x + 12, y, 4, 1), &side.colour);
            let field = Rect::new(body.x + 18, y, 12, 1);
            side.field.render(field, &mut c);
            fields[i] = field;
            if let Some((place, _)) = &side.from {
                let note = if side.changed() { format!("{}:{}  changed", crate::picker::short_path(&place.path), place.line + 1) } else { format!("{}:{}", crate::picker::short_path(&place.path), place.line + 1) };
                c.put_fit(body.x + 32, y, body.w.saturating_sub(32), &note, Role::Muted);
            }
            if focused && !self.typing() {
                c.focus(y, page.x..page.right());
            }
            y += 1;
        }
        self.fields.set(fields);
        y += 1;
        let ratio = self.ratio();
        // Two decimals, cut rather than rounded, so 4.499 doesn't read as
        // the 4.5 it fails.
        let shown = (ratio * 100.0).floor() / 100.0;
        c.put(body.x, y, &format!("{shown:.2}:1"), Style::new(Role::Title));
        if let Some(note) = &self.note {
            c.put_fit(body.x + 10, y, body.w.saturating_sub(10), note, Role::Muted);
        }
        y += 2;
        let p = passes(ratio);
        let verdict = |c: &mut Canvas, x: usize, y: usize, name: &str, ok: bool, need: &str| {
            let x = c.put(x, y, name, Role::Text) + 1;
            let x = c.put(x, y, if ok { " pass " } else { " fail " }, Style::new(if ok { Role::Good } else { Role::Bad }).bg(if ok { Fill::Good } else { Fill::Bad }));
            c.put(x + 1, y, need, Role::Muted);
        };
        c.put(body.x, y, "Normal text", Role::Muted);
        verdict(&mut c, body.x + 14, y, "AA", p.aa, "4.5");
        verdict(&mut c, body.x + 30, y, "AAA", p.aaa, "7");
        y += 1;
        c.put(body.x, y, "Large text", Role::Muted);
        verdict(&mut c, body.x + 14, y, "AA", p.aa_large, "3");
        verdict(&mut c, body.x + 30, y, "AAA", p.aaa_large, "4.5");
        y += 1;
        c.put(body.x, y, "UI parts", Role::Muted);
        verdict(&mut c, body.x + 14, y, "AA", p.ui, "3");
        y += 2;

        // The preview: big letters of the text's colour on the background.
        let back = self.back.colour.over(&self.under);
        let fg = self.text.colour.gamut_map();
        let sample = "Aa1g";
        let pw = draw::big_width(sample) + 4;
        let rect = Rect::new(body.x, y, pw.min(body.w), 7);
        c.fill_rect(rect, draw::fill(&back));
        draw::big(&mut c, body.x + 2, y + 1, sample, &fg, &back);
        // And as a control: a line of it, a border round a box.
        let ui = Rect::new(body.x + pw + 2, y, 10, 7);
        if ui.right() <= body.right() {
            c.fill_rect(ui, draw::fill(&back));
            let ink = draw::fill(&fg.over(&back));
            for x in ui.x + 1..ui.right() - 1 {
                c.fill(ui.y + 1, x..x + 1, ink);
                c.fill(ui.bottom() - 2, x..x + 1, ink);
            }
            for yy in ui.y + 1..ui.bottom() - 1 {
                c.fill(yy, ui.x + 1..ui.x + 2, ink);
                c.fill(yy, ui.right() - 2..ui.right() - 1, ink);
            }
        }
        y += 8;
        let keys = if self.typing() { "Enter take it   Esc leave it   Tab the other" } else { "Tab other  h l darker, lighter  s swap  f F fix AA, AAA  Enter write" };
        let foot = page.bottom().max(y + 1) - 1;
        c.put_fit(body.x, foot, body.w, keys, Role::Muted);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> Checker {
        let place = Place { path: "/p/a.css".into(), line: 0, start: 7, end: 11, text: "#999".into() };
        let text = Side::new(Colour::from_u8(0x99, 0x99, 0x99), Some((place, Format::Hex { digits: 3, upper: false })));
        Checker::new(text, Side::new(Colour::WHITE, None), Colour::WHITE)
    }

    #[test]
    fn the_ratio_and_what_it_passes() {
        let mut k = checker();
        assert!((k.ratio() - 2.85).abs() < 0.01);
        let text = k.render(70, 30).text();
        assert!(text.contains("2.84:1") || text.contains("2.85:1"), "{text}");
        assert!(text.contains("Normal text   AA  fail  4.5"), "{text}");
        assert!(!k.text.changed());
        assert_eq!(k.key(Key::Enter), Act::None, "nothing to write");
        assert_eq!(k.key(Key::Char('f')), Act::Redraw);
        assert!(k.ratio() >= 4.5 && k.ratio() < 4.8, "{}", k.ratio());
        assert!(k.text.changed());
        assert_eq!(k.note.as_deref(), Some("moved to pass AA"));
        let text = k.render(70, 30).text();
        assert!(text.contains("AA  pass  4.5"), "{text}");
        assert_eq!(k.key(Key::Enter), Act::Write);
    }

    #[test]
    fn swapping_moving_typing() {
        let mut k = checker();
        k.key(Key::Char('s'));
        assert_eq!(k.text.colour, Colour::WHITE);
        k.key(Key::Tab);
        assert_eq!(k.focus, 1);
        k.key(Key::Char('x'));
        assert!(k.typing());
        k.paste("#000");
        k.key(Key::Enter);
        assert!(!k.typing());
        assert!((k.ratio() - 21.0).abs() < 1e-6);
        k.key(Key::Char('l'));
        assert!(k.back.colour.luminance() > 0.0, "lighter");
        // Under AAA the background moves instead, when it's focused.
        let mut k = checker();
        k.focus = 1;
        k.key(Key::Char('F'));
        assert!(k.ratio() >= 7.0, "{}", k.ratio());
        assert_eq!(k.text.colour.rgb(), (0x99, 0x99, 0x99), "the text kept");
    }

    #[test]
    fn the_mouse_picks_a_colour() {
        let mut k = checker();
        k.render(70, 30);
        let fields = k.fields.get();
        assert_eq!(k.mouse(&Mouse::click(0, fields[1].y)), Act::Redraw);
        assert_eq!((k.focus, k.typing()), (1, false));
        k.mouse(&Mouse::click(fields[0].x + 1, fields[0].y));
        assert_eq!((k.focus, k.typing()), (0, true));
        assert_eq!(k.mouse(&Mouse::click(0, 0)), Act::None);
    }

    #[test]
    fn the_preview_is_drawn_in_the_colours() {
        let k = checker();
        let c = k.render(70, 30);
        let bgs = c.backgrounds();
        assert!(bgs.iter().any(|(_, _, f)| *f == Fill::Rgb(255, 255, 255)));
        assert!(bgs.iter().any(|(_, _, f)| *f == Fill::Rgb(0x99, 0x99, 0x99)), "the letters");
    }
}
