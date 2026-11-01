//! Binary files as a hex dump in a pane, as a WebAssembly extension: a
//! `.bin`, `.exe`, `.so` (`contributes.opens`) opens in it however you
//! open it, and any file at all with `hex.open`.
//!
//! One in the project is read where it is, a block at a time as it's
//! shown (`data`), so a file of any size opens at once; one elsewhere
//! Fenix reads whole for it, up to its 64 MiB.

pub mod data;
pub mod inspect;
pub mod page;

use std::collections::HashMap;

use fenix_extension_api::{export, page as pages, Event, Extension, Key, KeyPress, MouseEvent, Page};

use data::Input;
use page::{Action, Ask, HexPage, Saved};

/// A file's page.
const PAGE: &str = "hex";

/// Where a full path is under `/project`, if it is: read there, a part
/// at a time.
fn in_project(full: &str) -> Option<String> {
    let root = fenix_extension_api::here()?.project?.root;
    under(full, &root)
}

/// `full` under `root`, as `/project/...`: a drive's (`C:`) without case,
/// and `\\` as `/`.
fn under(full: &str, root: &str) -> Option<String> {
    let (f, r) = (full.replace('\\', "/"), root.replace('\\', "/"));
    let r = r.trim_end_matches('/');
    let same = |a: &str, b: &str| if a.as_bytes().get(1) == Some(&b':') { a.eq_ignore_ascii_case(b) } else { a == b };
    (f.len() > r.len() && f.is_char_boundary(r.len()) && same(&f[..r.len()], r) && f.as_bytes()[r.len()] == b'/').then(|| format!("/project{}", &f[r.len()..]))
}

/// What reads `path`: a file under `/project` where it is, any other
/// whole.
fn input(path: &str) -> Result<Input, String> {
    if path.starts_with("/project/") {
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        return Ok(Input::Path(path.to_string(), len));
    }
    fenix_extension_api::read_file(path).map(Input::Bytes).map_err(|e| format!("{e} (a file outside the project is read whole, up to 64 MiB)"))
}

/// `hex.bytesPerRow`: 0 (or unset) fits the pane.
fn per_row() -> Option<usize> {
    match fenix_extension_api::setting("hex.bytesPerRow").and_then(|s| s.trim().parse::<usize>().ok()) {
        Some(0) => None,
        Some(n) => Some(n.min(64)),
        None => Some(16),
    }
}

#[derive(Default)]
struct Hex {
    pages: HashMap<u64, HexPage>,
    /// Prompts asked: for which page, and what.
    asks: HashMap<u64, (u64, Ask)>,
}

impl Hex {
    fn open(&mut self, full: &str) {
        // One open already goes, its place coming here.
        let mut kept: Option<Saved> = None;
        let open: Vec<u64> = self.pages.iter().filter(|(_, p)| p.full == full).map(|(&id, _)| id).collect();
        for id in open {
            if let Some(p) = self.pages.remove(&id) {
                kept = Some(p.saved());
            }
            pages::close(id);
        }
        let path = in_project(full).unwrap_or_else(|| full.to_string());
        let mut p = HexPage::new(full, &path, input(&path));
        p.per_row = per_row();
        if let Some(s) = &kept {
            p.restore(s);
        }
        let id = pages::open(PAGE, &format!("{} (hex)", p.name()));
        self.pages.insert(id, p);
    }

    fn act(&mut self, page: u64, action: Action) {
        match action {
            Action::Nothing => {}
            Action::Redraw => pages::refresh(page),
            Action::Copy(text, said) => {
                fenix_extension_api::copy(&text);
                fenix_extension_api::notify(&said);
                pages::refresh(page);
            }
            Action::Say(said) => {
                fenix_extension_api::notify(&said);
                pages::refresh(page);
            }
            Action::Ask(ask, label, initial) => {
                let id = fenix_extension_api::prompt(&label, &initial);
                self.asks.insert(id, (page, ask));
            }
            Action::Reload => {
                if let Some(p) = self.pages.get_mut(&page) {
                    let read = input(&p.path);
                    p.reload(read);
                    pages::refresh(page);
                }
            }
        }
    }
}

impl Extension for Hex {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "hex.open" => {
                let full = fenix_extension_api::here().and_then(|h| h.path).ok_or("No file here to show in hex")?;
                self.open(&full);
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Opened(path) => self.open(&path),
            Event::Prompted((id, typed)) => {
                let Some((page, ask)) = self.asks.remove(&id) else { return };
                let (Some(typed), Some(p)) = (typed, self.pages.get_mut(&page)) else { return };
                let action = p.answer(ask, &typed);
                self.act(page, action);
            }
            Event::SettingChanged(_) => {
                let n = per_row();
                for (&id, p) in &mut self.pages {
                    p.per_row = n;
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
        p.render(width as usize, height as usize).to_page().more_keys(true).mouse(true)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key.into());
        self.act(page, action);
    }

    /// PageUp and PageDown, Ctrl+D, U, F and B, and Shift with a move.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let (Some(p), Some(press)) = (self.pages.get_mut(&page), fenix_ui_kit::KeyPress::from_api(&key)) else { return };
        let action = p.press(press);
        self.act(page, action);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.mouse(&fenix_ui_kit::Mouse::from(&event));
        self.act(page, action);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        serde_json::to_string(&self.pages.get(&page)?.saved()).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != PAGE {
            return false;
        }
        let Ok(saved) = serde_json::from_str::<Saved>(state) else { return false };
        // One in the project it reads from there, if it's the same file;
        // one elsewhere Fenix gives it again with its page.
        let path = if saved.path.starts_with("/project/") {
            if in_project(&saved.full).as_deref() != Some(saved.path.as_str()) {
                return false;
            }
            saved.path.clone()
        } else {
            saved.full.clone()
        };
        let Ok(read) = input(&path) else { return false };
        let mut p = HexPage::new(&saved.full, &path, Ok(read));
        p.per_row = per_row();
        p.restore(&saved);
        self.pages.insert(page, p);
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
        self.asks.retain(|_, (p, _)| *p != page);
    }
}

export!(Hex);

#[cfg(test)]
mod tests {
    use super::under;

    #[test]
    fn a_file_in_the_project_is_read_there() {
        assert_eq!(under("/home/me/p/fw/a.bin", "/home/me/p").as_deref(), Some("/project/fw/a.bin"));
        assert_eq!(under("C:\\Work\\P\\a.bin", "c:/work/p/").as_deref(), Some("/project/a.bin"));
        assert_eq!(under("/home/me/pq/a.bin", "/home/me/p"), None, "a folder named alike isn't it");
        assert_eq!(under("/home/me/P/a.bin", "/home/me/p"), None, "case counts on Linux");
    }
}
