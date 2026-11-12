//! The mouse: a pad to click and drag on, and what each event was.

// gallery:begin
use std::cell::Cell;

use fenix_ui_kit::{Button, Canvas, Key, KeyPress, Mouse, MouseAction, Rect, Role};

use crate::draw;
use crate::sections::Reply;

/// The brushes the wheel goes round.
const BRUSHES: [&str; 4] = ["●", "○", "◆", "✚"];
/// The events kept, newest first.
const KEPT: usize = 8;

pub struct State {
    /// What's on the pad: a cell from its corner, and the brush.
    pub marks: Vec<(usize, usize, usize)>,
    pub brush: usize,
    /// The last events, said in words.
    pub events: Vec<String>,
    pad: Cell<Rect>,
}

/// An event in words: what, which button, where, what was held.
fn said(m: &Mouse) -> String {
    let button = |b: Button| format!("{b:?}").to_lowercase();
    let what = match m.action {
        MouseAction::Press { button: b, clicks: 1 } => format!("{} press", button(b)),
        MouseAction::Press { button: b, clicks: 2 } => format!("{} double click", button(b)),
        MouseAction::Press { button: b, clicks } => format!("{} press ×{clicks}", button(b)),
        MouseAction::Release(b) => format!("{} release", button(b)),
        MouseAction::Drag(b) => format!("{} drag", button(b)),
        MouseAction::Wheel(lines) if lines > 0 => format!("wheel up {lines}"),
        MouseAction::Wheel(lines) => format!("wheel down {}", -lines),
    };
    let held: Vec<&str> = [(m.mods.shift, "Shift"), (m.mods.ctrl, "Ctrl"), (m.mods.alt, "Alt"), (m.mods.meta, "Meta")].iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
    let held = if held.is_empty() { String::new() } else { format!(" with {}", held.join("+")) };
    format!("{what} at cell {}, line {}{held}", m.x, m.y)
}

impl State {
    pub fn new() -> State {
        State { marks: Vec::new(), brush: 0, events: Vec::new(), pad: Cell::new(Rect::default()) }
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("click drag", "draw"), ("right click", "rub out"), ("wheel", "the brush"), ("x", "clear")]
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        // The pad on the left, framed; what happened on the right.
        let (pad, log) = rect.split_left((rect.w / 2).clamp(1, 44));
        draw::text(c, pad, 0, 0, &format!("The pad · brush {}", BRUSHES[self.brush]), Role::Muted);
        let (_, area) = pad.split_top(1);
        let framed = area.h >= 3 && area.w >= 3;
        let inside = if framed { area.inset(1) } else { area };
        self.pad.set(inside);
        if framed {
            for y in [inside.y - 1, inside.bottom()] {
                c.rule(y, inside.x..inside.right());
            }
            for x in [inside.x - 1, inside.right()] {
                c.vline(x, inside.y..inside.bottom(), Role::Muted);
            }
        }
        if self.marks.is_empty() {
            draw::text(c, inside, 1, 0, "click or drag here", Role::Muted);
        }
        for &(x, y, b) in &self.marks {
            if x < inside.w && y < inside.h {
                c.put(inside.x + x, inside.y + y, BRUSHES[b], Role::Accent);
            }
        }
        let log = log.inset_x(1);
        draw::text(c, log, 0, 0, "Events, the last first", Role::Muted);
        if self.events.is_empty() {
            draw::text(c, log, 0, 2, "Nothing yet", Role::Muted);
        }
        for (i, e) in self.events.iter().enumerate() {
            draw::text(c, log, 0, 2 + i, e, if i == 0 { Role::Text } else { Role::Muted });
        }
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        if press.key == Key::Char('x') {
            self.marks.clear();
            self.events.clear();
            return Reply::Done;
        }
        Reply::Ignored
    }

    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        self.events.insert(0, said(m));
        self.events.truncate(KEPT);
        let pad = self.pad.get();
        if let Some(lines) = m.wheel() {
            let n = BRUSHES.len();
            self.brush = (self.brush + if lines > 0 { n - 1 } else { 1 }) % n;
        } else if pad.contains(m.x, m.y) {
            let at = (m.x - pad.x, m.y - pad.y);
            match m.action {
                MouseAction::Press { button: Button::Left, .. } | MouseAction::Drag(Button::Left) => {
                    self.marks.retain(|&(x, y, _)| (x, y) != at);
                    self.marks.push((at.0, at.1, self.brush));
                }
                MouseAction::Press { button: Button::Right, .. } => self.marks.retain(|&(x, y, _)| (x, y) != at),
                _ => {}
            }
        }
        Reply::Done
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
