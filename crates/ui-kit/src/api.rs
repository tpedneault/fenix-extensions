//! The extension API's types: a canvas as `page.canvas` takes it (and
//! 0.8.2's `page-render-styled`), the keys `page-key` and `page-key-press`
//! give, and the mouse `page-mouse` gives.

use fenix_extension_api as api;

use crate::{Button, Canvas, Fill, Key, KeyPress, Mods, Mouse, MouseAction, Role, Style};

impl From<api::Key> for Key {
    fn from(key: api::Key) -> Key {
        use api::Key as K;
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
            K::Char(c) => Key::Char(c),
        }
    }
}

impl From<&Role> for api::Role {
    fn from(role: &Role) -> api::Role {
        match role {
            Role::Title => api::Role::Title,
            Role::Text => api::Role::Text,
            Role::Muted => api::Role::Muted,
            Role::Accent => api::Role::Accent,
            Role::Good => api::Role::Good,
            Role::Warn => api::Role::Warn,
            Role::Bad => api::Role::Bad,
            Role::Syntax(name) => api::Role::Syntax(name.clone()),
        }
    }
}

impl From<&api::Role> for Role {
    fn from(role: &api::Role) -> Role {
        match role {
            api::Role::Title => Role::Title,
            api::Role::Text => Role::Text,
            api::Role::Muted => Role::Muted,
            api::Role::Accent => Role::Accent,
            api::Role::Good => Role::Good,
            api::Role::Warn => Role::Warn,
            api::Role::Bad => Role::Bad,
            api::Role::Syntax(name) => Role::Syntax(name.clone()),
        }
    }
}

impl Canvas {
    /// As `page.canvas` takes it (API 0.8.0): columns as character
    /// offsets; a style's colour as its span, panel fills as panels. Other
    /// backgrounds and underlines go with it, in [`Canvas::to_page`].
    pub fn to_api(&self) -> api::Canvas {
        let cells = |y: usize, cols: &std::ops::Range<usize>| {
            let r = self.char_range(y, cols);
            api::Cells { line: y as u32, start: r.start as u32, end: r.end as u32 }
        };
        let spans = self.spans().into_iter().filter(|(_, _, s)| s.fg != Role::Text).map(|(y, cols, s): (usize, _, Style)| api::CellSpan { cells: cells(y, &cols), role: (&s.fg).into() }).collect();
        api::Canvas {
            text: self.text(),
            spans,
            rules: self.rules().iter().map(|(y, cols)| cells(*y, cols)).collect(),
            focus: self.focused().map(|(y, cols)| cells(*y, cols)),
            panels: self.panels().iter().map(|(y, cols)| cells(*y, cols)).collect(),
            pictures: self
                .pictures()
                .iter()
                .map(|p| api::Picture {
                    cells: cells(p.rect.y, &(p.rect.x..p.rect.right())),
                    rows: p.rect.h as u32,
                    image: p.image,
                    view: p.view,
                    marks: p.marks.iter().map(|m| api::Mark { x: m.x, y: m.y, width: m.width, height: m.height, selected: m.selected }).collect(),
                })
                .collect(),
            popup: self.popup_ref().map(|p| api::CanvasPopup {
                line: p.line as u32,
                column: self.char_offset(p.line, p.col) as u32,
                rows: p.rows.iter().map(|row| row.iter().map(|(text, s)| api::Span { text: text.clone(), role: (&s.fg).into() }).collect()).collect(),
            }),
        }
    }
}

impl From<Fill> for api::Shade {
    fn from(fill: Fill) -> api::Shade {
        match fill {
            Fill::Panel => api::Shade::Panel,
            Fill::Focus => api::Shade::Focus,
            Fill::Selection => api::Shade::Selection,
            Fill::Accent => api::Shade::Accent,
            Fill::Good => api::Shade::Good,
            Fill::Warn => api::Shade::Warn,
            Fill::Bad => api::Shade::Bad,
            Fill::Rgb(r, g, b) => api::Shade::Rgb((r, g, b)),
        }
    }
}

impl Canvas {
    /// The page for `page-render`: the canvas, typing when a field has the
    /// keyboard, every background (panels are `panels` already) and
    /// underline, the vertical rules, and whether the page takes more keys
    /// and the mouse.
    pub fn to_page(&self) -> api::Page {
        let cells = |y: usize, cols: &std::ops::Range<usize>| {
            let r = self.char_range(y, cols);
            api::Cells { line: y as u32, start: r.start as u32, end: r.end as u32 }
        };
        let mut page = api::Page::canvas(self.to_api()).typing(self.typing()).more_keys(self.more_keys()).mouse(self.mouse());
        for (y, cols, fill) in self.backgrounds() {
            if fill != Fill::Panel {
                page = page.background(cells(y, &cols), fill.into());
            }
        }
        for (y, cols, role) in self.underlines() {
            page = page.underline(cells(y, &cols), (&role).into());
        }
        for (x, ys) in self.vrules() {
            page = page.vertical_rule(*x as u32, ys.start as u32, ys.end as u32);
        }
        page
    }
}

impl From<api::MouseButton> for Button {
    fn from(b: api::MouseButton) -> Button {
        match b {
            api::MouseButton::Left => Button::Left,
            api::MouseButton::Middle => Button::Middle,
            api::MouseButton::Right => Button::Right,
        }
    }
}

impl From<api::KeyMods> for Mods {
    fn from(m: api::KeyMods) -> Mods {
        Mods { shift: m.shift, ctrl: m.ctrl, alt: m.alt, meta: m.meta }
    }
}

impl From<&api::MouseEvent> for Mouse {
    /// At the event's cell: a wide character is two, as the canvas
    /// counts.
    fn from(e: &api::MouseEvent) -> Mouse {
        let action = match e.action {
            api::MouseAction::Press(p) => MouseAction::Press { button: p.button.into(), clicks: p.clicks },
            api::MouseAction::Release(b) => MouseAction::Release(b.into()),
            api::MouseAction::Drag(b) => MouseAction::Drag(b.into()),
            api::MouseAction::Wheel(lines) => MouseAction::Wheel(lines),
        };
        Mouse { x: e.cell as usize, y: e.line as usize, action, mods: e.mods.into() }
    }
}

impl KeyPress {
    /// A key from `page-key-press`, when it's one a widget knows (Caps
    /// Lock isn't). Shift+Tab is `BackTab`.
    pub fn from_api(press: &api::KeyPress) -> Option<KeyPress> {
        use api::NamedKey as N;
        let mods: Mods = press.mods.into();
        let key = match press.key {
            api::SurfaceKey::Char(c) => Key::Char(c),
            api::SurfaceKey::Named(n) => match n {
                N::Escape => Key::Escape,
                N::Enter => Key::Enter,
                N::Tab if mods.shift => Key::BackTab,
                N::Tab => Key::Tab,
                N::Backspace => Key::Backspace,
                N::Delete => Key::Delete,
                N::Left => Key::Left,
                N::Right => Key::Right,
                N::Up => Key::Up,
                N::Down => Key::Down,
                N::Home => Key::Home,
                N::End => Key::End,
                N::PageUp => Key::PageUp,
                N::PageDown => Key::PageDown,
                N::CapsLock => return None,
            },
        };
        Some(KeyPress { key, mods })
    }
}

impl From<&Canvas> for api::Canvas {
    fn from(canvas: &Canvas) -> api::Canvas {
        canvas.to_api()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fill, Popup, Rect, TextField};

    #[test]
    fn a_canvas_goes_to_the_api_in_characters() {
        let mut c = Canvas::new();
        c.put(0, 0, "日本", Role::Title);
        c.put(4, 0, " ok", Style::new(Role::Good).bg(Fill::Selection).underline());
        c.rule(1, 0..6);
        c.focus(0, 0..10);
        c.popup(Popup { line: 0, col: 4, rows: vec![vec![("Open".into(), Role::Accent.into())]] });
        let a = c.to_api();
        assert_eq!(a.text, "日本 ok\n");
        assert_eq!(
            a.spans,
            [api::CellSpan { cells: api::Cells { line: 0, start: 0, end: 2 }, role: api::Role::Title }, api::CellSpan { cells: api::Cells { line: 0, start: 2, end: 5 }, role: api::Role::Good },]
        );
        assert_eq!(a.focus, Some(api::Cells { line: 0, start: 0, end: 8 }));
        assert_eq!(a.popup.map(|p| p.column), Some(2));
        let mut f = TextField::new().with_text("x");
        f.focused = true;
        let mut c = Canvas::new();
        f.render(Rect::new(2, 0, 10, 1), &mut c);
        assert_eq!(c.to_api().panels, [api::Cells { line: 0, start: 2, end: 12 }]);
        assert!(c.typing());
    }

    #[test]
    fn a_styled_page_has_its_backgrounds_and_underlines() {
        let mut c = Canvas::new();
        c.put(0, 0, "日本", Style::new(Role::Title).bg(Fill::Rgb(1, 2, 3)));
        c.put(4, 0, "ok", Style::new(Role::Good).underline());
        c.put(0, 1, "tab", Style::new(Role::Title).bg(Fill::Panel));
        c.fill(2, 0..4, Fill::Bad);
        c.vrule(6, 0..3);
        c.set_mouse(true);
        let p = c.to_page();
        let cells = |line, start, end| api::Cells { line, start, end };
        assert_eq!(
            p.styles,
            [
                api::CellStyle { cells: cells(0, 0, 2), background: Some(api::Shade::Rgb((1, 2, 3))), underline: None },
                api::CellStyle { cells: cells(2, 0, 4), background: Some(api::Shade::Bad), underline: None },
                api::CellStyle { cells: cells(0, 2, 4), background: None, underline: Some(api::Role::Good) },
            ],
            "in characters; the panel left to `panels`"
        );
        assert_eq!(p.vertical_rules, [api::VerticalRule { column: 6, top: 0, bottom: 3 }]);
        assert!(p.mouse && !p.more_keys);
        let canvas = p.canvas.expect("a canvas");
        assert_eq!(canvas.text, "日本ok\ntab\n    ", "the filled line's blanks kept");
        assert_eq!(canvas.panels, [cells(1, 0, 3)]);
    }

    #[test]
    fn the_mouse_and_more_keys_come_from_the_api() {
        let mods = api::KeyMods { shift: true, ctrl: false, alt: false, meta: false };
        let e = api::MouseEvent { line: 2, column: 1, cell: 2, action: api::MouseAction::Press(api::MousePress { button: api::MouseButton::Left, clicks: 2 }), mods };
        let m = Mouse::from(&e);
        assert_eq!((m.x, m.y, m.left_clicks(), m.mods.shift), (2, 2, 2, true), "by the cell, not the character");
        let e = api::MouseEvent { action: api::MouseAction::Wheel(-3), ..e };
        assert_eq!(Mouse::from(&e).wheel(), Some(-3));
        let press = |key, mods| KeyPress::from_api(&api::KeyPress { key, mods });
        assert_eq!(press(api::SurfaceKey::Named(api::NamedKey::Tab), mods), Some(KeyPress::new(Key::BackTab, Mods::SHIFT)));
        assert_eq!(press(api::SurfaceKey::Named(api::NamedKey::PageDown), mods).map(|p| p.key), Some(Key::PageDown));
        assert_eq!(press(api::SurfaceKey::Named(api::NamedKey::CapsLock), mods), None);
        let ctrl = api::KeyMods { shift: false, ctrl: true, alt: false, meta: false };
        assert!(press(api::SurfaceKey::Char('z'), ctrl).is_some_and(|p| p.is_ctrl('z')));
    }

    #[test]
    fn keys_come_from_the_api() {
        assert_eq!(Key::from(api::Key::WordLeft), Key::WordLeft);
        assert_eq!(Key::from(api::Key::Char(' ')), Key::Char(' '));
        assert_eq!(Role::from(&api::Role::Syntax("keyword".into())), Role::Syntax("keyword".into()));
    }
}
