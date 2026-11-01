//! Parquet files read in a pane, as a WebAssembly extension: a
//! `.parquet` opens in it however you open it (`contributes.opens`), its
//! rows as a table and its schema beside them.
//!
//! The footer is read once (`file`), and rows only as they're shown, so
//! a file of any size opens at once. One in the project is read where it
//! is, as far as each read needs; one elsewhere Fenix reads whole for
//! it, up to its 64 MiB.

pub mod file;
pub mod page;

use std::collections::HashMap;

use fenix_extension_api::{export, page as pages, Event, Extension, Key, KeyPress, MouseEvent, NamedKey, Page, SurfaceKey};

use file::Input;
use page::{Action, ParquetPage, Saved};

/// A file's page.
const PAGE: &str = "parquet";

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
    fenix_extension_api::read_file(path).map(|b| Input::Bytes(b.into())).map_err(|e| format!("{e} -- a file outside the project is read whole; one in it is read a part at a time, whatever its size"))
}

#[derive(Default)]
struct Parquet {
    pages: HashMap<u64, ParquetPage>,
}

impl Parquet {
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
        let mut p = ParquetPage::new(full, &path, input(&path));
        if let Some(s) = &kept {
            p.restore(s);
        }
        let id = pages::open(PAGE, &p.name());
        self.pages.insert(id, p);
    }

    fn act(&mut self, page: u64, action: Action) {
        match action {
            Action::Nothing => {}
            Action::Redraw => pages::refresh(page),
            Action::Copy(text, said) => {
                if !text.is_empty() {
                    fenix_extension_api::copy(&text);
                }
                fenix_extension_api::notify(&said);
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

impl Extension for Parquet {
    fn handle(&mut self, event: Event) {
        if let Event::Opened(path) = event {
            self.open(&path);
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

    /// PageUp and PageDown a screen.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = match key.key {
            SurfaceKey::Named(NamedKey::PageUp) => p.key(fenix_ui_kit::Key::PageUp),
            SurfaceKey::Named(NamedKey::PageDown) => p.key(fenix_ui_kit::Key::PageDown),
            _ => Action::Nothing,
        };
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
        let mut p = ParquetPage::new(&saved.full, &path, Ok(read));
        p.restore(&saved);
        self.pages.insert(page, p);
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }
}

export!(Parquet);

#[cfg(test)]
mod tests {
    use super::under;

    #[test]
    fn a_file_in_the_project_is_read_there() {
        assert_eq!(under("/home/me/p/data/a.parquet", "/home/me/p").as_deref(), Some("/project/data/a.parquet"));
        assert_eq!(under("C:\\Work\\P\\a.parquet", "c:/work/p/").as_deref(), Some("/project/a.parquet"));
        assert_eq!(under("/home/me/pq/a.parquet", "/home/me/p"), None, "a folder named alike isn't it");
        assert_eq!(under("/home/me/P/a.parquet", "/home/me/p"), None, "case counts on Linux");
    }
}
