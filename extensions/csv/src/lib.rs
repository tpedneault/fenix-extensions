//! CSV and TSV files as a table, as a WebAssembly extension: a `.csv` or
//! `.tsv` opens in it however you open it (`contributes.opens`), its
//! columns lined up, sorted and filtered in the view, its cells, rows and
//! columns changed through the file's buffer -- so saving, and Fenix's
//! undo, are the buffer's.
//!
//! The file's records are found once (`table`), and a row's cells read
//! only when it's shown, sorted on or filtered, so a million rows open
//! and scroll as a hundred do.

pub mod copy;
pub mod dialect;
pub mod edit;
pub mod page;
pub mod table;
pub mod view;

use std::collections::{BTreeMap, HashMap};

use fenix_extension_api::{buffer, export, host, page as pages, Cells, Changed, Event, Extension, Key, KeyPress, MouseEvent, NamedKey, Page, Rgb, Shade, SurfaceKey};

use dialect::Dialect;
use page::{Saved, TablePage, Tint, World};

/// A file's table page.
const TABLE: &str = "table";
/// The dialects chosen for files, by full path.
const CHOSEN: &str = "/data/dialects.json";

/// Fenix, as the pages ask things of it.
struct Fenix;

fn chosen() -> BTreeMap<String, Dialect> {
    std::fs::read_to_string(CHOSEN).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

impl World for Fenix {
    fn buffer_text(&mut self, path: &str) -> Option<String> {
        buffer::text(path)
    }

    fn edit(&mut self, path: &str, edits: Vec<edit::TextEdit>) -> Result<(), String> {
        let edits: Vec<fenix_extension_api::TextEdit> = edits.into_iter().map(|e| fenix_extension_api::TextEdit::new(e.start, e.end, e.text)).collect();
        buffer::edit(path, &edits)
    }

    fn save(&mut self, path: &str) -> Result<(), String> {
        buffer::save(path)
    }

    fn copy(&mut self, text: &str) {
        fenix_extension_api::copy(text)
    }

    fn say(&mut self, text: &str) {
        fenix_extension_api::notify(text)
    }

    fn text(&mut self, path: &str) {
        // A file it opens itself, asked for: Fenix opens its text. `SPC f
        // o` there comes back.
        host::open_file(path, None);
    }

    fn remember(&mut self, full: &str, dialect: Option<Dialect>) {
        let mut all = chosen();
        match dialect {
            Some(d) => all.insert(full.to_string(), d),
            None => all.remove(full),
        };
        if let Err(e) = serde_json::to_string_pretty(&all).map_err(|e| e.to_string()).and_then(|s| std::fs::write(CHOSEN, s).map_err(|e| e.to_string())) {
            fenix_extension_api::warn!("{CHOSEN}: {e}");
        }
    }

    fn buffer_undo(&mut self, path: &str, redo: bool) -> Result<(), String> {
        if redo {
            buffer::redo(path)
        } else {
            buffer::undo(path)
        }
    }
}

/// `a` over `b`, `amount` of it.
fn blend(a: Rgb, b: Rgb, amount: f32) -> Rgb {
    let mix = |x: u8, y: u8| (x as f32 * amount + y as f32 * (1.0 - amount)).round() as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The focused cell's background: the accent, faint on the theme's.
fn focus_shade() -> Shade {
    let t = fenix_extension_api::theme();
    Shade::Rgb(blend(t.accent, t.background, 0.3))
}

/// `csv.rows`, when it's set: the rows shown, whatever the pane's height.
fn rows_setting() -> Option<usize> {
    fenix_extension_api::setting("csv.rows").and_then(|s| s.trim().parse().ok()).filter(|&n: &usize| n > 0).map(|n| n.min(500))
}

/// Where a full path is under `/project`, if it is: for the session,
/// which reads it there.
fn in_project(full: &str) -> Option<String> {
    let root = fenix_extension_api::here()?.project?.root;
    let (f, r) = (full.replace('\\', "/"), root.replace('\\', "/"));
    let r = r.trim_end_matches('/');
    let same = |a: &str, b: &str| if a.as_bytes().get(1) == Some(&b':') { a.eq_ignore_ascii_case(b) } else { a == b };
    (f.len() > r.len() && same(&f[..r.len()], r) && f.as_bytes()[r.len()] == b'/').then(|| format!("/project{}", &f[r.len()..]))
}

/// Whether full paths `a` and `b` are the same file: a drive's (`C:`)
/// without case, and `\\` as `/`.
fn same_file(a: &str, b: &str) -> bool {
    let (a, b) = (a.replace('\\', "/"), b.replace('\\', "/"));
    if a.as_bytes().get(1) == Some(&b':') {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// The file's text: its buffer's, if one's open (changed and not saved),
/// else what's on disk.
fn read(path: &str) -> Result<Vec<u8>, String> {
    match buffer::text(path) {
        Some(t) => Ok(t.into_bytes()),
        None => fenix_extension_api::read_file(path),
    }
}

#[derive(Default)]
struct Csv {
    pages: HashMap<u64, TablePage>,
}

impl Csv {
    fn open(&mut self, full: &str) {
        let bytes = match read(full) {
            Ok(b) => b,
            Err(e) => return fenix_extension_api::notify(&format!("{full}: {e}")),
        };
        // One open already goes, its place coming here.
        let mut kept = None;
        let open: Vec<u64> = self.pages.iter().filter(|(_, p)| p.full == full).map(|(&id, _)| id).collect();
        for id in open {
            if let Some(p) = self.pages.remove(&id) {
                kept = Some(p.saved());
            }
            pages::close(id);
        }
        let mut p = TablePage::new(full, full, bytes, chosen().get(full).copied());
        p.rows_shown = rows_setting();
        if let Some(s) = &kept {
            p.restore(s);
        }
        let id = pages::open(TABLE, p.name());
        self.pages.insert(id, p);
    }
}

impl Extension for Csv {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Opened(path) => self.open(&path),
            Event::SettingChanged(_) => {
                let n = rows_setting();
                for (&id, p) in &mut self.pages {
                    p.rows_shown = n;
                    pages::refresh(id);
                }
            }
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let Some(p) = self.pages.get(&page) else {
            return Page::new().line([pages::muted("Nothing here: the file is closed")]);
        };
        let c = p.render_in(width as usize, height as usize);
        let mut out = Page::canvas(c.to_api()).typing(p.typing()).more_keys(true).mouse(true);
        let tints = p.tints(&c);
        let focus = if tints.iter().any(|t| t.2 == Tint::Focus) { Some(focus_shade()) } else { None };
        for (line, cols, tint) in tints {
            let cells = Cells { line: line as u32, start: cols.start as u32, end: cols.end as u32 };
            out = out.background(cells, if tint == Tint::Focus { focus.unwrap_or(Shade::Focus) } else { Shade::Selection });
        }
        out
    }

    fn page_key(&mut self, page: u64, key: Key) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.key(key.into(), &mut Fenix);
        }
    }

    /// PageUp and PageDown a screen; Ctrl+Z and Ctrl+Shift+Z the
    /// buffer's undo and redo, or a field's.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        match key.key {
            SurfaceKey::Named(NamedKey::PageUp) => p.key(fenix_ui_kit::Key::PageUp, &mut Fenix),
            SurfaceKey::Named(NamedKey::PageDown) => p.key(fenix_ui_kit::Key::PageDown, &mut Fenix),
            SurfaceKey::Char(c) if key.mods.ctrl && !key.mods.alt => p.ctrl_key(c, key.mods.shift, &mut Fenix),
            _ => {}
        }
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        p.mouse(&fenix_ui_kit::Mouse::from(&event), &mut Fenix);
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.paste(text);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        let p = self.pages.get(&page)?;
        let mut s = p.saved();
        if let Some(path) = in_project(&p.full) {
            s.path = path;
        }
        serde_json::to_string(&s).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != TABLE {
            return false;
        }
        let Ok(saved) = serde_json::from_str::<Saved>(state) else { return false };
        // One in the project it reads from there, if it's the same file;
        // one elsewhere Fenix gives it again with its page.
        let path = if saved.path.starts_with("/project/") {
            let root = fenix_extension_api::here().and_then(|h| h.project).map(|p| p.root);
            if root.is_some() && in_project(&saved.full).as_deref() != Some(saved.path.as_str()) {
                return false;
            }
            saved.path.clone()
        } else {
            saved.full.clone()
        };
        let Ok(bytes) = read(&path) else { return false };
        let full = if saved.full.is_empty() { path.clone() } else { saved.full.clone() };
        let mut p = TablePage::new(&full, &path, bytes, chosen().get(&full).copied());
        p.rows_shown = rows_setting();
        p.restore(&saved);
        self.pages.insert(page, p);
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }

    /// A file's buffer changed: one of its tables', typed in its text or
    /// replaced across files, is read again.
    fn buffer_changed(&mut self, what: Changed) {
        let Changed::File(path) = what else { return };
        for (&id, p) in self.pages.iter_mut().filter(|(_, p)| same_file(&p.full, &path)) {
            if p.changed(buffer::text(&p.path)) {
                pages::refresh(id);
            }
        }
    }
}

export!(Csv);

#[cfg(test)]
mod tests {
    use super::same_file;

    #[test]
    fn a_file_is_the_same_however_fenix_spells_it() {
        assert!(same_file("/home/me/a.csv", "/home/me/a.csv"));
        assert!(!same_file("/home/me/a.csv", "/home/me/A.csv"), "case counts on Linux");
        assert!(same_file("C:\\Work\\a.csv", "c:/work/a.csv"));
        assert!(!same_file("/home/me/a.csv", "/home/me/b.csv"));
    }
}
