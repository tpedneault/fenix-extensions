//! The mouse on a page (0.8.2's `page-mouse`), and the keys held with it
//! or with a key (`page-key-press`).

/// The keys held with a key, a click or the wheel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// The Windows key, or Command.
    pub meta: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, meta: false };
    pub const SHIFT: Mods = Mods { shift: true, ..Mods::NONE };
    pub const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };

    /// Ctrl, Alt or the Windows key: a chord, not typing.
    pub fn chord(&self) -> bool {
        self.ctrl || self.alt || self.meta
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseAction {
    /// A button went down: `clicks` 2 for a double click, 3 a triple.
    Press { button: Button, clicks: u8 },
    Release(Button),
    /// Moved to another cell with a button held.
    Drag(Button),
    /// The wheel turned, in lines: up is more than 0.
    Wheel(i32),
}

/// The mouse at a cell of the page: `x` in display cells (a wide
/// character two), as the canvas counts them, and `y` its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mouse {
    pub x: usize,
    pub y: usize,
    pub action: MouseAction,
    pub mods: Mods,
}

impl Mouse {
    /// A left click at (`x`, `y`).
    pub fn click(x: usize, y: usize) -> Mouse {
        Mouse { x, y, action: MouseAction::Press { button: Button::Left, clicks: 1 }, mods: Mods::NONE }
    }

    /// How many clicks of the left button this is (2 a double click),
    /// or 0 when it's something else.
    pub fn left_clicks(&self) -> u8 {
        match self.action {
            MouseAction::Press { button: Button::Left, clicks } => clicks.max(1),
            _ => 0,
        }
    }

    /// The lines the wheel turned (up is more than 0), when it did.
    pub fn wheel(&self) -> Option<i32> {
        match self.action {
            MouseAction::Wheel(lines) => Some(lines),
            _ => None,
        }
    }

    /// Dragged with the left button.
    pub fn is_drag(&self) -> bool {
        self.action == MouseAction::Drag(Button::Left)
    }
}

/// Where the wheel takes `at` of `len` rows: three a notch, up for more
/// than 0. None when it stays.
pub(crate) fn wheel_step(at: usize, len: usize, lines: i32) -> Option<usize> {
    let by = lines.unsigned_abs() as usize * 3;
    let to = if lines > 0 { at.saturating_sub(by) } else { (at + by).min(len.saturating_sub(1)) };
    (to != at).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_and_the_wheel() {
        let m = Mouse::click(3, 4);
        assert_eq!((m.left_clicks(), m.wheel(), m.is_drag()), (1, None, false));
        let m = Mouse { action: MouseAction::Wheel(-1), ..m };
        assert_eq!((m.left_clicks(), m.wheel()), (0, Some(-1)));
        assert_eq!(wheel_step(0, 10, -1), Some(3));
        assert_eq!(wheel_step(8, 10, -1), Some(9));
        assert_eq!(wheel_step(9, 10, -2), None, "at the end");
        assert_eq!(wheel_step(2, 10, 1), Some(0));
        assert!(Mods::CTRL.chord() && !Mods::SHIFT.chord());
    }
}
