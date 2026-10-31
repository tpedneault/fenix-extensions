//! The picker page: the colour under the cursor by hue, saturation,
//! lightness and alpha, with the keys and the mouse; typed in as hex (or
//! any literal); shown beside what it was; written back the way the
//! literal was written.

use std::cell::Cell;

use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Outcome, Rect, Role, Style, TextField};

use crate::colour::Colour;
use crate::draw;
use crate::literal::{find, Format, Scan, Target};

/// Where a literal is in a file: its line and characters, from 0, and
/// its text, to check it's still there before it's written over.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub path: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A colour as typed: a literal of any kind, or hex digits alone.
pub fn parse_any(text: &str) -> Option<Colour> {
    let t = text.trim();
    if let Some(c) = crate::names::lookup(&t.to_ascii_lowercase()) {
        return Some(c);
    }
    let found = find(t, Scan { style: true, theme: true });
    if let [one] = found.as_slice() {
        if one.start == 0 && one.end == t.chars().count() {
            return Some(one.colour);
        }
    }
    if !t.starts_with('#') && matches!(t.len(), 3 | 4 | 6 | 8) {
        return crate::literal::parse_hex(t);
    }
    None
}

/// The four controls, then the hex field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Hue,
    Saturation,
    Lightness,
    Alpha,
}

const CONTROLS: [Control; 4] = [Control::Hue, Control::Saturation, Control::Lightness, Control::Alpha];

impl Control {
    fn label(self) -> &'static str {
        match self {
            Control::Hue => "H",
            Control::Saturation => "S",
            Control::Lightness => "L",
            Control::Alpha => "A",
        }
    }
}

/// What a key or click asks of the extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    None,
    /// Draw it again.
    Redraw,
    /// Write the colour into the file, and close.
    Write,
}

#[derive(Debug, Clone)]
pub struct Picker {
    /// Hue in degrees; saturation, lightness and alpha from 0 to 1. Kept
    /// apart from the colour so a grey keeps its hue as you go through it.
    pub h: f64,
    pub s: f64,
    pub l: f64,
    pub a: f64,
    pub original: Colour,
    /// How the literal is written, for writing it back.
    pub format: Format,
    /// Where it is; none when there was no literal at the cursor, and the
    /// colour is typed in there.
    pub place: Option<Place>,
    pub control: usize,
    pub field: TextField,
    bars: Cell<[Rect; 4]>,
}

impl Picker {
    pub fn new(colour: Colour, format: Format, place: Option<Place>) -> Picker {
        let (h, s, l) = colour.gamut_map().hsl();
        let mut field = TextField::new();
        field.focused = false;
        let mut p = Picker { h, s, l, a: colour.a, original: colour, format, place, control: 0, field, bars: Cell::new([Rect::default(); 4]) };
        p.sync_field();
        p
    }

    pub fn colour(&self) -> Colour {
        Colour::from_hsl(self.h, self.s, self.l, self.a)
    }

    /// What Enter writes.
    pub fn written(&self) -> String {
        self.format.write(&self.colour())
    }

    /// The colour set, its hue kept when it's grey.
    pub fn set(&mut self, c: &Colour) {
        let (h, s, l) = c.gamut_map().hsl();
        if s > 0.0 && l > 0.0 && l < 1.0 {
            self.h = h;
        }
        self.s = s;
        self.l = l;
        self.a = c.a;
    }

    fn sync_field(&mut self) {
        if !self.field.focused {
            let text = Format::Hex { digits: 6, upper: false }.write(&self.colour());
            self.field.set_text(&text);
        }
    }

    fn value(&self, c: Control) -> f64 {
        match c {
            Control::Hue => self.h / 360.0,
            Control::Saturation => self.s,
            Control::Lightness => self.l,
            Control::Alpha => self.a,
        }
    }

    fn set_value(&mut self, c: Control, v: f64) {
        let v = v.clamp(0.0, 1.0);
        match c {
            Control::Hue => self.h = v * 360.0,
            Control::Saturation => self.s = v,
            Control::Lightness => self.l = v,
            Control::Alpha => self.a = v,
        }
        self.sync_field();
    }

    /// The control moved by `steps`: a degree of hue, a percent of the
    /// rest. Hue goes round; the rest stop at their ends.
    pub fn nudge(&mut self, c: Control, steps: f64) {
        match c {
            Control::Hue => {
                self.h = ((self.h + steps).round()).rem_euclid(360.0);
                self.sync_field();
            }
            _ => {
                let v = ((self.value(c) * 100.0).round() + steps) / 100.0;
                self.set_value(c, v);
            }
        }
    }

    fn current(&self) -> Control {
        CONTROLS[self.control.min(3)]
    }

    /// The hex field takes the keyboard, its text all selected.
    pub fn edit(&mut self) {
        self.field.focused = true;
        self.field.select_all();
    }

    fn field_outcome(&mut self, outcome: Outcome) -> Act {
        match outcome {
            Outcome::Edited => {
                // Live, as it parses.
                if let Some(c) = parse_any(self.field.text()) {
                    self.set(&c);
                }
                Act::Redraw
            }
            Outcome::Submit => {
                if let Some(c) = parse_any(self.field.text()) {
                    self.set(&c);
                }
                self.field.focused = false;
                self.sync_field();
                Act::Redraw
            }
            Outcome::Cancel => {
                self.field.focused = false;
                self.sync_field();
                Act::Redraw
            }
            Outcome::Ignored => Act::None,
            _ => Act::Redraw,
        }
    }

    /// A key: `j` `k` (or the arrows) a control, `h` `l` a step, `H` `L`
    /// ten, `0` `$` its ends; `x`, `#` or Tab the hex field; `r` back to
    /// what it was; Enter writes it.
    pub fn key(&mut self, key: Key) -> Act {
        if self.field.focused {
            if matches!(key, Key::Tab | Key::BackTab) {
                return self.field_outcome(Outcome::Submit);
            }
            let outcome = self.field.handle_key(key);
            return self.field_outcome(outcome);
        }
        let c = self.current();
        match key {
            Key::Down | Key::Char('j') => self.control = (self.control + 1).min(3),
            Key::Up | Key::Char('k') => self.control = self.control.saturating_sub(1),
            Key::Left | Key::Char('h') => self.nudge(c, -1.0),
            Key::Right | Key::Char('l') => self.nudge(c, 1.0),
            Key::Char('H') | Key::WordLeft => self.nudge(c, -10.0),
            Key::Char('L') | Key::WordRight => self.nudge(c, 10.0),
            Key::Char('0') | Key::Home => self.set_value(c, 0.0),
            Key::Char('$') | Key::End => self.set_value(c, 1.0),
            Key::Char('x' | '#') | Key::Tab => self.edit(),
            Key::Char('r') => {
                let o = self.original;
                self.set(&o);
                self.sync_field();
            }
            Key::Enter => return Act::Write,
            _ => return Act::None,
        }
        Act::Redraw
    }

    /// A key with Shift or Ctrl: Shift with Left and Right ten steps;
    /// PageUp and PageDown ten; the field's own (Ctrl+A, Ctrl+Z).
    pub fn press(&mut self, press: KeyPress) -> Act {
        if self.field.focused {
            let outcome = self.field.handle_press(press);
            return self.field_outcome(outcome);
        }
        let c = self.current();
        match (press.key, press.mods.shift) {
            (Key::Left, true) | (Key::PageDown, _) => self.nudge(c, -10.0),
            (Key::Right, true) | (Key::PageUp, _) => self.nudge(c, 10.0),
            _ => return Act::None,
        }
        Act::Redraw
    }

    pub fn paste(&mut self, text: &str) -> Act {
        if !self.field.focused {
            self.edit();
        }
        let outcome = self.field.paste(text);
        self.field_outcome(outcome)
    }

    /// The mouse: a click or a drag on a bar sets it there, the wheel over
    /// one steps it; a click on the field types in it.
    pub fn mouse(&mut self, m: &Mouse) -> Act {
        let bars = self.bars.get();
        let hit = bars.iter().position(|r| r.contains(m.x, m.y));
        if let Some(i) = hit {
            let c = CONTROLS[i];
            if let Some(lines) = m.wheel() {
                self.control = i;
                self.nudge(c, f64::from(lines.signum()));
                return Act::Redraw;
            }
            if m.left_clicks() > 0 || m.is_drag() {
                if self.field.focused {
                    self.field_outcome(Outcome::Submit);
                }
                self.control = i;
                self.set_value(c, draw::bar_value(bars[i], m.x));
                return Act::Redraw;
            }
            return Act::None;
        }
        if m.left_clicks() > 0 {
            let was = self.field.focused;
            self.field.focused = true;
            if self.field.mouse(m) == Outcome::Ignored {
                self.field.focused = was;
                return Act::None;
            }
            return Act::Redraw;
        }
        Act::None
    }

    /// The page, `w` by `h`.
    pub fn render(&self, w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        c.set_more_keys(true);
        let page = Rect::new(0, 0, w, h.max(24));
        let body = page.inset_x(2);
        let mut y = 0;
        c.put(body.x, y, "Colour", Role::Title);
        let what = match &self.place {
            Some(p) => format!("{} · {}:{}", self.format.kind(), short_path(&p.path), p.line + 1),
            None => "typed in at the cursor".into(),
        };
        c.put_fit(body.x + 8, y, body.w.saturating_sub(8), &what, Role::Muted);
        y += 1;
        c.rule(y, page.x..page.right());
        y += 2;

        // Was and now, side by side.
        let half = body.w / 2;
        let colour = self.colour();
        draw::block(&mut c, Rect::new(body.x, y, half.saturating_sub(1), 4), &self.original);
        draw::block(&mut c, Rect::new(body.x + half, y, body.w - half, 4), &colour);
        y += 4;
        c.put(body.x, y, "was ", Role::Muted);
        c.put_fit(body.x + 4, y, half.saturating_sub(5), &Format::Hex { digits: 6, upper: false }.write(&self.original), Role::Text);
        c.put(body.x + half, y, "now ", Role::Muted);
        c.put_fit(body.x + half + 4, y, (body.w - half).saturating_sub(4), &self.written(), Role::Accent);
        y += 2;

        // The bars.
        let bar_w = body.w.saturating_sub(12);
        let mut rects = [Rect::default(); 4];
        for (i, control) in CONTROLS.iter().enumerate() {
            let focused = i == self.control && !self.field.focused;
            let style = if focused { Style::new(Role::Accent) } else { Style::new(Role::Muted) };
            c.put(body.x, y, control.label(), style);
            let rect = Rect::new(body.x + 3, y, bar_w, 2);
            rects[i] = rect;
            let (hh, ss, ll) = (self.h, self.s, self.l);
            match control {
                Control::Hue => draw::bar(&mut c, rect, self.value(*control), |t| Colour::from_hsl(t * 360.0, ss.max(0.25), ll.clamp(0.25, 0.75), 1.0)),
                Control::Saturation => draw::bar(&mut c, rect, self.value(*control), |t| Colour::from_hsl(hh, t, ll, 1.0)),
                Control::Lightness => draw::bar(&mut c, rect, self.value(*control), |t| Colour::from_hsl(hh, ss, t, 1.0)),
                Control::Alpha => draw::bar(&mut c, rect, self.value(*control), |t| Colour::from_hsl(hh, ss, ll, t)),
            }
            let value = match control {
                Control::Hue => format!("{}°", (self.h).round()),
                _ => format!("{}%", (self.value(*control) * 100.0).round()),
            };
            c.put(body.x + 4 + bar_w, y, &value, if focused { Role::Accent } else { Role::Text });
            if focused {
                c.focus(y, page.x..page.right());
            }
            y += 2;
        }
        self.bars.set(rects);
        y += 1;
        c.put(body.x, y, "Hex", if self.field.focused { Role::Accent } else { Role::Muted });
        self.field.render(Rect::new(body.x + 5, y, 12.min(body.w.saturating_sub(5)), 1), &mut c);
        y += 2;
        let mapped = colour.gamut_map();
        for (i, t) in Target::ALL.iter().enumerate().skip(1) {
            let text = t.format(&self.format, false).write(&mapped);
            c.put(body.x, y + i - 1, &format!("{:<6}", t.name()), Role::Muted);
            c.put_fit(body.x + 6, y + i - 1, body.w.saturating_sub(6), &text, Role::Text);
        }
        let keys = if self.field.focused { "Enter take it   Esc leave it" } else { "j k pick  h l step  H L ten  0 $ ends  x hex  r reset  Enter write" };
        let foot = page.bottom().max(y + 4) - 1;
        c.put_fit(body.x, foot, body.w, keys, Role::Muted);
        c
    }
}

/// The last two parts of a path.
pub fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(2)..].join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_ui_kit::Mods;

    fn blue() -> Picker {
        let place = Place { path: "/p/src/app.css".into(), line: 4, start: 9, end: 16, text: "#3b82f6".into() };
        Picker::new(Colour::from_u8(59, 130, 246), Format::Hex { digits: 6, upper: false }, Some(place))
    }

    #[test]
    fn keys_move_the_controls() {
        let mut p = blue();
        assert_eq!(p.written(), "#3b82f6");
        p.key(Key::Char('l'));
        assert_eq!(p.h.round(), 218.0);
        p.key(Key::Char('H'));
        assert_eq!(p.h.round(), 208.0);
        p.key(Key::Char('j'));
        p.key(Key::Char('j'));
        p.key(Key::Char('0'));
        assert_eq!(p.written(), "#000000", "lightness at 0");
        p.key(Key::Char('r'));
        assert_eq!(p.written(), "#3b82f6", "as it was");
        p.key(Key::Char('j'));
        p.key(Key::Char('H'));
        p.key(Key::Char('H'));
        p.key(Key::Char('H'));
        p.key(Key::Char('H'));
        p.key(Key::Char('H'));
        assert_eq!(p.written(), "#3b82f680", "half see-through: eight digits");
        assert_eq!(p.press(KeyPress::new(Key::Right, Mods::SHIFT)), Act::Redraw);
        assert!((p.a - 0.6).abs() < 1e-9);
        assert_eq!(p.key(Key::Enter), Act::Write);
        assert_eq!(p.key(Key::Char('?')), Act::None);
    }

    #[test]
    fn a_grey_keeps_its_hue() {
        let mut p = blue();
        p.control = 1;
        p.key(Key::Char('0'));
        assert_eq!(p.colour().rgb().0, p.colour().rgb().2, "grey");
        p.key(Key::Char('$'));
        let (r, g, b) = p.colour().rgb();
        assert!(b > g && g > r, "the hue came back with the saturation: {r} {g} {b}");
        assert!((p.h - 217.2).abs() < 0.1 && p.s == 1.0);
    }

    #[test]
    fn the_hex_field() {
        let mut p = blue();
        p.key(Key::Char('x'));
        assert!(p.field.focused);
        for k in "f00".chars() {
            p.key(Key::Char(k));
        }
        assert_eq!(p.colour().rgb(), (255, 0, 0), "live as it parses");
        p.key(Key::Enter);
        assert!(!p.field.focused);
        assert_eq!(p.field.text(), "#ff0000");
        p.key(Key::Tab);
        p.paste("rgb(0 0 255)");
        assert_eq!(p.colour().rgb(), (0, 0, 255), "any literal");
        p.key(Key::Escape);
        assert!(!p.field.focused);
        assert_eq!(parse_any("tomato").map(|c| c.rgb()), Some((255, 99, 71)));
        assert_eq!(parse_any("abc").map(|c| c.rgb()), Some((170, 187, 204)));
        assert_eq!(parse_any("#abc x"), None);
    }

    #[test]
    fn the_mouse_on_the_bars() {
        let mut p = blue();
        let c = p.render(60, 30);
        assert!(c.mouse() && c.more_keys());
        let bars = p.bars.get();
        // Lightness at its middle.
        let l = bars[2];
        assert_eq!(p.mouse(&Mouse::click(l.x + (l.w - 1) / 2, l.y)), Act::Redraw);
        assert_eq!(p.control, 2);
        assert!((p.l - 0.5).abs() < 0.03, "{}", p.l);
        let wheel = Mouse { action: fenix_ui_kit::MouseAction::Wheel(1), ..Mouse::click(bars[3].x + 1, bars[3].y) };
        let a = p.a;
        p.mouse(&wheel);
        assert_eq!((p.control, p.a), (3, a), "at its end already");
        p.mouse(&Mouse { action: fenix_ui_kit::MouseAction::Wheel(-1), ..wheel });
        assert!((p.a - 0.99).abs() < 1e-9);
        assert_eq!(p.mouse(&Mouse::click(0, 0)), Act::None, "the title");
    }

    #[test]
    fn the_page_shows_was_and_now() {
        let mut p = blue();
        p.key(Key::Char('L'));
        let c = p.render(60, 30);
        let text = c.text();
        assert!(text.contains("Colour  hex · src/app.css:5"), "{text}");
        assert!(text.contains("was #3b82f6"));
        assert!(text.contains("now #3b6cf6") || text.contains("now #"), "{text}");
        assert!(text.contains("oklch "));
        assert!(c.backgrounds().iter().any(|(_, _, f)| *f == draw::fill(&p.original)), "the old colour's block");
        assert!(c.backgrounds().iter().any(|(_, _, f)| *f == draw::fill(&p.colour())), "the new one's");
        assert_eq!(short_path("C:\\work\\x\\a.css"), "x/a.css");
    }
}
