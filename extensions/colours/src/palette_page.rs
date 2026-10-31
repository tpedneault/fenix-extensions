//! The palette page: the project's colours by hue, or the near-duplicates
//! among them, each opened to where it's used.

use std::cell::Cell;
use std::collections::HashSet;

use fenix_ui_kit::{Canvas, Key, Mouse, Outcome, Rect, Role, ScrollState, Style, Tab, TabsState};

use crate::draw;
use crate::palette::Palette;

/// A row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A family's heading, or a group of near-duplicates': its index.
    Header(usize),
    /// An entry of the palette.
    Entry(usize),
    /// An entry's use: the entry, the use.
    Use(usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    None,
    Redraw,
    /// The file below the project, at a line (from 1).
    Open(String, u32),
    Copy(String),
    Rescan,
}

#[derive(Debug, Default)]
pub struct PalettePage {
    pub tabs: TabsState,
    pub scroll: ScrollState,
    pub selected: usize,
    expanded: HashSet<usize>,
    list: Cell<Rect>,
}

impl PalettePage {
    pub fn new() -> PalettePage {
        PalettePage { tabs: TabsState::new(), ..PalettePage::default() }
    }

    /// The rows of the tab shown.
    pub fn rows(&self, p: &Palette) -> Vec<Row> {
        let mut out = Vec::new();
        let entry = |out: &mut Vec<Row>, i: usize| {
            out.push(Row::Entry(i));
            if self.expanded.contains(&i) {
                out.extend((0..p.entries[i].uses.len()).map(|u| Row::Use(i, u)));
            }
        };
        if self.tabs.active == 0 {
            for (k, (_, members)) in p.by_hue().iter().enumerate() {
                out.push(Row::Header(k));
                for &i in members {
                    entry(&mut out, i);
                }
            }
        } else {
            for (k, group) in p.near_duplicates().iter().enumerate() {
                out.push(Row::Header(k));
                for &i in group {
                    entry(&mut out, i);
                }
            }
        }
        out
    }

    /// The selected row, kept off headings.
    fn settle(&mut self, rows: &[Row], down: bool) {
        if rows.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.min(rows.len() - 1);
        let ok = |i: usize| !matches!(rows[i], Row::Header(_));
        if ok(self.selected) {
            return;
        }
        let forward = (self.selected..rows.len()).find(|&i| ok(i));
        let back = (0..self.selected).rev().find(|&i| ok(i));
        if let Some(i) = if down { forward.or(back) } else { back.or(forward) } {
            self.selected = i;
        }
    }

    fn activate(&mut self, rows: &[Row], p: &Palette) -> Act {
        match rows.get(self.selected) {
            Some(Row::Entry(i)) => {
                if !self.expanded.remove(i) {
                    self.expanded.insert(*i);
                }
                Act::Redraw
            }
            Some(Row::Use(i, u)) => {
                let usage = &p.entries[*i].uses[*u];
                Act::Open(usage.path.clone(), usage.line)
            }
            _ => Act::None,
        }
    }

    /// `j` `k` a row, `J` `K` a screen, `g` `G` the ends; Enter or `l`
    /// opens a colour to its uses, or goes to a use; `h` closes it; Tab,
    /// `1`, `2` the tabs; `y` copies the colour; `r` reads it all again.
    pub fn key(&mut self, key: Key, p: &Palette) -> Act {
        if self.tabs.handle_key(key, 2).handled() {
            self.selected = 0;
            self.scroll.scroll_to(0, 0);
            let rows = self.rows(p);
            self.settle(&rows, true);
            return Act::Redraw;
        }
        let rows = self.rows(p);
        // Off a heading first: the page opens on one.
        self.settle(&rows, true);
        match key {
            Key::Enter | Key::Char('l') | Key::Right => return self.activate(&rows, p),
            Key::Char('h') | Key::Left => match rows.get(self.selected) {
                Some(Row::Use(i, _)) | Some(Row::Entry(i)) => {
                    let i = *i;
                    self.expanded.remove(&i);
                    let rows = self.rows(p);
                    self.selected = rows.iter().position(|r| *r == Row::Entry(i)).unwrap_or(0);
                }
                _ => return Act::None,
            },
            Key::Char('y') => {
                return match rows.get(self.selected) {
                    Some(Row::Entry(i) | Row::Use(i, _)) => Act::Copy(p.entries[*i].hex()),
                    _ => Act::None,
                }
            }
            Key::Char('r') => return Act::Rescan,
            key => {
                let was = self.selected;
                let down = matches!(key, Key::Down | Key::Char('j' | 'J' | 'G') | Key::PageDown | Key::End);
                let step = match key {
                    Key::Down | Key::Char('j') => Some((self.selected + 1).min(rows.len().saturating_sub(1))),
                    Key::Up | Key::Char('k') => Some(self.selected.saturating_sub(1)),
                    Key::Char('J') | Key::PageDown => Some((self.selected + 20).min(rows.len().saturating_sub(1))),
                    Key::Char('K') | Key::PageUp => Some(self.selected.saturating_sub(20)),
                    Key::Char('g') | Key::Home => Some(0),
                    Key::Char('G') | Key::End => Some(rows.len().saturating_sub(1)),
                    _ => None,
                };
                let Some(to) = step else { return Act::None };
                self.selected = to;
                self.settle(&rows, down);
                // A heading at the top stays in view above its first row.
                if !down && self.selected == was {
                    return Act::None;
                }
            }
        }
        let rows = self.rows(p);
        let at = match self.selected.checked_sub(1).map(|i| rows[i]) {
            Some(Row::Header(_)) => self.selected - 1,
            _ => self.selected,
        };
        self.scroll.show(at, rows.len());
        self.scroll.show(self.selected, rows.len());
        Act::Redraw
    }

    /// A click on a tab, or a row (a double click as Enter); the wheel
    /// scrolls.
    pub fn mouse(&mut self, m: &Mouse, p: &Palette) -> Act {
        if self.tabs.mouse(m) == Outcome::Moved {
            self.selected = 0;
            self.scroll.scroll_to(0, 0);
            let rows = self.rows(p);
            self.settle(&rows, true);
            return Act::Redraw;
        }
        let rows = self.rows(p);
        if self.scroll.mouse(m, rows.len()) == Outcome::Moved {
            return Act::Redraw;
        }
        let list = self.list.get();
        if m.left_clicks() == 0 || !list.contains(m.x, m.y) {
            return Act::None;
        }
        let i = self.scroll.offset() + (m.y - list.y);
        if i >= rows.len() || matches!(rows[i], Row::Header(_)) {
            return Act::None;
        }
        self.selected = i;
        if m.left_clicks() >= 2 {
            return self.activate(&rows, p);
        }
        Act::Redraw
    }

    /// The page, `w` by `h`; `status` says how the reading's going.
    pub fn render(&self, p: &Palette, status: &str, w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        let page = Rect::new(0, 0, w, h.max(8));
        let body = page.inset_x(2);
        c.put(body.x, 0, "Palette", Role::Title);
        c.put_fit(body.x + 9, 0, body.w.saturating_sub(9), status, Role::Muted);
        let near = p.near_duplicates();
        let tabs = [Tab::new("By hue").badge(p.entries.len().to_string()), Tab::new("Near-duplicates").badge(near.len().to_string())];
        self.tabs.render(&tabs, Rect::new(body.x, 1, body.w, 2), &mut c);
        let (foot, list) = Rect::new(body.x, 3, body.w, page.h.saturating_sub(3)).split_bottom(1);
        self.list.set(list);
        let rows = self.rows(p);
        let groups = p.by_hue();
        self.scroll.render(rows.len(), list, &mut c, |i, r, c| match rows[i] {
            Row::Header(k) => {
                let (title, detail) = if self.tabs.active == 0 {
                    let (family, members) = &groups[k];
                    (family.name().to_string(), members.len().to_string())
                } else {
                    (format!("{} alike", near[k].len()), format!("ΔEOK {:.3}", p.spread(&near[k])))
                };
                let x = c.put(r.x, r.y, &title, Role::Title);
                c.put(x + 2, r.y, &detail, Role::Muted);
            }
            Row::Entry(e) => {
                let entry = &p.entries[e];
                let open = self.expanded.contains(&e);
                c.put(r.x, r.y, if open { "▾" } else { "▸" }, Role::Muted);
                draw::block(c, Rect::new(r.x + 2, r.y, 4, 1), &entry.colour);
                let x = c.put(r.x + 7, r.y, &format!("{:<10}", entry.hex()), Style::new(Role::Text));
                let files = entry.files();
                let uses = format!("{} use{} in {} file{}", entry.uses.len(), if entry.uses.len() == 1 { "" } else { "s" }, files, if files == 1 { "" } else { "s" });
                c.put_fit(x + 1, r.y, r.right().saturating_sub(x + 1), &uses, Role::Muted);
                if i == self.selected {
                    c.focus(r.y, r.x..r.right());
                }
            }
            Row::Use(e, u) => {
                let usage = &p.entries[e].uses[u];
                let place = format!("{}:{}", usage.path, usage.line);
                let x = c.put_fit(r.x + 7, r.y, r.w.saturating_sub(7 + 2), &place, Role::Accent);
                c.put_fit(x + 2, r.y, r.right().saturating_sub(x + 2), &usage.text, Role::Muted);
                if i == self.selected {
                    c.focus(r.y, r.x..r.right());
                }
            }
        });
        if rows.is_empty() {
            let empty = if self.tabs.active == 0 { "No colours found" } else { "No colours near another" };
            c.put(list.x, list.y, empty, Role::Muted);
        }
        c.put_fit(foot.x, foot.y, foot.w, "j k move  Enter open, go to  h close  Tab near-duplicates  y copy  r reread", Role::Muted);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::tests::project;
    use crate::palette::Walk;

    fn palette() -> Palette {
        let fs = project();
        let mut walk = Walk::new();
        let mut p = Palette::new();
        while walk.step(&fs, &mut p) {}
        p
    }

    #[test]
    fn by_hue_opened_to_its_uses() {
        let p = palette();
        let mut page = PalettePage::new();
        let rows = page.rows(&p);
        assert!(matches!(rows[0], Row::Header(_)));
        page.settle(&rows, true);
        assert_eq!(page.selected, 1, "off the heading");
        let text = page.render(&p, "4 files", 70, 20).text();
        assert!(text.contains("Reds  1"), "{text}");
        assert!(text.contains("#ff0000    1 use in 1 file"), "{text}");
        assert_eq!(page.key(Key::Enter, &p), Act::Redraw);
        let text = page.render(&p, "", 70, 20).text();
        assert!(text.contains("src/main.rs:2  #ff0000"), "{text}");
        assert_eq!(page.key(Key::Char('j'), &p), Act::Redraw);
        assert_eq!(page.key(Key::Enter, &p), Act::Open("src/main.rs".into(), 2));
        assert_eq!(page.key(Key::Char('y'), &p), Act::Copy("#ff0000".into()));
        page.key(Key::Char('h'), &p);
        assert_eq!(page.rows(&p)[page.selected], Row::Entry(page.rows(&p).iter().find_map(|r| if let Row::Entry(i) = r { Some(*i) } else { None }).unwrap()));
        page.key(Key::Char('j'), &p);
        assert!(matches!(page.rows(&p)[page.selected], Row::Entry(_)), "past the next heading");
        assert_eq!(page.key(Key::Char('r'), &p), Act::Rescan);
    }

    #[test]
    fn near_duplicates_and_the_mouse() {
        let p = palette();
        let mut page = PalettePage::new();
        page.render(&p, "", 70, 20);
        assert_eq!(page.key(Key::Tab, &p), Act::Redraw);
        let text = page.render(&p, "", 70, 20).text();
        assert!(text.contains("2 alike  ΔEOK 0.00"), "{text}");
        assert!(text.contains("#3b82f6") && text.contains("#3b83f6"));
        // Line 3 the heading, 4 and 5 the two.
        let double = Mouse { action: fenix_ui_kit::MouseAction::Press { button: fenix_ui_kit::Button::Left, clicks: 2 }, ..Mouse::click(10, 5) };
        assert_eq!(page.mouse(&double, &p), Act::Redraw);
        assert_eq!(page.rows(&p)[page.selected], page.rows(&p)[2]);
        assert!(page.render(&p, "", 70, 20).text().contains("src/app.css:3"));
        assert_eq!(page.mouse(&Mouse::click(10, 3), &p), Act::None, "a heading");
        assert_eq!(page.mouse(&Mouse::click(4, 1), &p), Act::Redraw, "the first tab");
        assert_eq!(page.tabs.active, 0);
    }
}
