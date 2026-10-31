//! The env files page: every env file in the project side by side, a
//! row a key and a column a file, values hidden until `r` shows them.

use std::borrow::Cow;
use std::collections::HashMap;

use fenix_ui_kit::{Canvas, Column, Key, Outcome, Rect, Role, Style, TableSource, TableState, Width};

use crate::files;
use crate::parse::{EnvFile, MASK};

/// One file's column.
pub struct FileColumn {
    pub rel: String,
    pub file: EnvFile,
    /// Its values as they're loaded, by key.
    pub values: HashMap<String, String>,
    pub example: bool,
    /// The keys its example has, when it has one.
    pub expected: Option<Vec<String>>,
}

impl FileColumn {
    pub fn new(rel: String, text: &str) -> FileColumn {
        let file = EnvFile::parse(text);
        let values = file.map();
        let example = files::is_example(files::name(&rel));
        FileColumn { rel, file, values, example, expected: None }
    }
}

/// What a key on the page asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    /// The file, at a line (from 0).
    Open(String, Option<u32>),
    Copy(String),
    AddMissing(String),
    Profile,
    Ignore,
    Reload,
}

pub struct EnvPage {
    pub columns: Vec<FileColumn>,
    pub keys: Vec<String>,
    pub revealed: bool,
    /// Whether examples' values are hidden too.
    pub mask_examples: bool,
    /// What's said above the table: what Git would commit, the profile.
    pub notes: Vec<(String, Role)>,
    pub table: TableState,
}

impl EnvPage {
    pub fn new(files: Vec<(String, String)>) -> EnvPage {
        let mut page = EnvPage { columns: Vec::new(), keys: Vec::new(), revealed: false, mask_examples: false, notes: Vec::new(), table: TableState::new() };
        page.load(files);
        page
    }

    /// Its files, read again: the focus stays on the same key and file.
    pub fn load(&mut self, files: Vec<(String, String)>) {
        let was = self.focused();
        let rels: Vec<String> = files.iter().map(|(r, _)| r.clone()).collect();
        let mut columns: Vec<FileColumn> = files.into_iter().map(|(rel, text)| FileColumn::new(rel, &text)).collect();
        let keys_of: HashMap<String, Vec<String>> = columns.iter().map(|c| (c.rel.clone(), c.file.keys().into_iter().map(String::from).collect())).collect();
        for c in &mut columns {
            c.expected = files::example_for(&c.rel, &rels).and_then(|e| keys_of.get(e)).cloned();
        }
        let mut keys: Vec<String> = Vec::new();
        for c in &columns {
            for k in c.file.keys() {
                if !keys.iter().any(|x| x == k) {
                    keys.push(k.to_string());
                }
            }
        }
        self.columns = columns;
        self.keys = keys;
        self.table.refit();
        if let Some((key, rel)) = was {
            if let Some(r) = self.keys.iter().position(|k| *k == key) {
                self.table.row = r;
            }
            if let Some(c) = self.columns.iter().position(|c| c.rel == rel) {
                self.table.col = c + 1;
            }
        }
    }

    /// The focused row's key, and the focused column's file (the first
    /// when it's the key's column).
    pub fn focused(&self) -> Option<(String, String)> {
        let key = self.keys.get(self.table.row)?.clone();
        let col = self.columns.get(self.table.col.saturating_sub(1))?;
        Some((key, col.rel.clone()))
    }

    /// What cell (`key`, column `c`) shows, and how.
    fn cell(&self, key: &str, c: &FileColumn) -> (String, Role) {
        match c.values.get(key) {
            None if c.expected.as_ref().is_some_and(|e| e.iter().any(|k| k == key)) => ("missing".into(), Role::Warn),
            None => ("—".into(), Role::Muted),
            Some(v) if v.is_empty() => ("\"\"".into(), Role::Muted),
            Some(_) if !self.revealed && (!c.example || self.mask_examples) => (MASK.into(), Role::Muted),
            Some(v) => (v.replace("\r\n", "⏎").replace('\n', "⏎"), Role::Text),
        }
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let w = width.max(20);
        let x = c.put(0, 0, "Env files", Role::Title);
        let about = format!(
            "  {} files · {} keys · values {}",
            self.columns.len(),
            self.keys.len(),
            if self.revealed { "shown" } else { "hidden" }
        );
        c.put_fit(x, 0, w.saturating_sub(x), &about, Role::Muted);
        let mut y = 1;
        for (note, role) in self.notes.iter().take(4) {
            c.put_fit(0, y, w, note, role.clone());
            y += 1;
        }
        y += 1;
        let keys_y = height.max(y + 4) - 1;
        if self.columns.is_empty() {
            c.put_fit(0, y, w, "No env files in this project: .env, .env.<profile>, <name>.env", Role::Muted);
        } else {
            let mut columns = vec![Column::new("Key", Width::Fit { min: 3, max: 40 })];
            columns.extend(self.columns.iter().map(|f| Column::new(f.rel.clone(), Width::Flex(1))));
            self.table.render(&columns, &Rows(self), Rect::new(0, y, w, keys_y.saturating_sub(y + 1)), &mut c);
        }
        let mut x = 0;
        for (k, what) in [("r", if self.revealed { "hide values" } else { "show values" }), ("Enter", "open"), ("y", "copy"), ("m", "add missing"), ("p", "profile"), ("i", ".gitignore"), ("R", "reload")] {
            x = c.put(x, keys_y, k, Role::Accent);
            x = c.put(x + 1, keys_y, what, Role::Muted) + 2;
        }
        c
    }

    pub fn key(&mut self, key: Key) -> Action {
        let cols = self.columns.len() + 1;
        match key {
            Key::Char('r') => {
                self.revealed = !self.revealed;
                Action::Redraw
            }
            Key::Char('R') => Action::Reload,
            Key::Char('p') => Action::Profile,
            Key::Char('i') => Action::Ignore,
            Key::Char('m') => match self.focused() {
                Some((_, rel)) => Action::AddMissing(rel),
                None => Action::Nothing,
            },
            Key::Char('y') => {
                let Some((key, rel)) = self.focused() else { return Action::Nothing };
                if self.table.col == 0 {
                    return Action::Copy(key);
                }
                match self.columns.iter().find(|c| c.rel == rel).and_then(|c| c.values.get(&key)) {
                    Some(v) => Action::Copy(v.clone()),
                    None => Action::Nothing,
                }
            }
            Key::Enter => {
                let Some((key, rel)) = self.focused() else { return Action::Nothing };
                let line = self.columns.iter().find(|c| c.rel == rel).and_then(|c| c.file.get(&key)).map(|e| e.line());
                Action::Open(rel, line)
            }
            key => match self.table.handle_key(key, self.keys.len(), cols) {
                Outcome::Ignored => Action::Nothing,
                _ => Action::Redraw,
            },
        }
    }
}

struct Rows<'a>(&'a EnvPage);

impl TableSource for Rows<'_> {
    fn len(&self) -> usize {
        self.0.keys.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        let key = &self.0.keys[index];
        std::iter::once(Cow::Borrowed(key.as_str())).chain(self.0.columns.iter().map(|c| Cow::Owned(self.0.cell(key, c).0))).collect()
    }

    fn style(&self, row: usize, col: usize) -> Style {
        if col == 0 {
            return Style::new(Role::Syntax("variable".into()));
        }
        let key = &self.0.keys[row];
        Style::new(self.0.columns.get(col - 1).map_or(Role::Text, |c| self.0.cell(key, c).1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> EnvPage {
        EnvPage::new(vec![
            (".env".into(), "HOST=db\nURL=postgres://${HOST}/app\nSECRET=s3\n".into()),
            (".env.example".into(), "HOST=localhost\nURL=\nSECRET=\nPORT=5432\n".into()),
            (".env.staging".into(), "HOST=stage\nEXTRA=1\n".into()),
        ])
    }

    #[test]
    fn keys_side_by_side_values_hidden() {
        let p = page();
        assert_eq!(p.keys, ["HOST", "URL", "SECRET", "PORT", "EXTRA"]);
        let text = p.render(90, 14).text();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("Env files  3 files · 5 keys · values hidden"), "{text}");
        let host = lines.iter().find(|l| l.starts_with("HOST")).unwrap();
        assert!(host.contains(MASK) && host.contains("localhost"), "an example's values are shown: {host}");
        assert!(!host.contains("db"));
        let port = lines.iter().find(|l| l.starts_with("PORT")).unwrap();
        assert!(port.contains("missing"), "{port}");
        let extra = lines.iter().find(|l| l.starts_with("EXTRA")).unwrap();
        assert!(extra.contains('—'), "not in .env, nor in its example: {extra}");
        assert!(lines.last().unwrap().starts_with("r show values"));
    }

    #[test]
    fn r_shows_them_interpolated() {
        let mut p = page();
        assert_eq!(p.key(Key::Char('r')), Action::Redraw);
        let text = p.render(100, 14).text();
        assert!(text.contains("postgres://db/app"), "{text}");
        assert!(!text.contains(MASK));
    }

    #[test]
    fn keys_open_copy_and_add() {
        let mut p = page();
        assert_eq!(p.key(Key::Char('j')), Action::Redraw);
        assert_eq!(p.key(Key::Char('l')), Action::Redraw);
        assert_eq!(p.focused(), Some(("URL".into(), ".env".into())));
        assert_eq!(p.key(Key::Enter), Action::Open(".env".into(), Some(1)));
        assert_eq!(p.key(Key::Char('y')), Action::Copy("postgres://db/app".into()));
        assert_eq!(p.key(Key::Char('m')), Action::AddMissing(".env".into()));
        // Read again, the focus stays.
        p.load(vec![(".env.example".into(), "URL=\n".into()), (".env".into(), "A=1\nURL=x\n".into())]);
        assert_eq!(p.focused(), Some(("URL".into(), ".env".into())));
    }

    #[test]
    fn no_files() {
        let p = EnvPage::new(Vec::new());
        assert!(p.render(80, 10).text().contains("No env files in this project"));
    }
}
