//! A regex playground (`SPC s x`): a pattern, a test text and a
//! replacement, matched live as you type -- every group in its own
//! colour, the matches listed with their groups, the replacement's
//! result -- with the pattern explained piece by piece, its errors
//! pointed at, how JavaScript, Python and PCRE read it otherwise, and
//! copies of it for six languages. Rust's `regex` matches, or
//! `fancy-regex` for lookaround and backreferences. A cheatsheet tab has
//! the syntax and recipes, each loading into the playground.
//!
//! It opens on the regex under the cursor, or with the selection as the
//! text; open, it's brought forward with them. While it's open its
//! matches are marked in the file you're in too (`decorations`), as you
//! type there or in it. Patterns are saved by name, for the project or
//! for you, in `/data/patterns.json`.

mod cheatsheet;
mod engine;
mod escape;
mod explain;
mod field;
mod notes;
mod playground;
mod seed;
mod store;
mod syntax;

use std::collections::HashMap;
use std::path::Path;

use fenix_extension_api::{self as api, decorations, export, host, Cells, Changed, Decoration, Event, Extension, Focus, Key, KeyPress, MouseAction, MouseButton, MouseEvent, NamedKey, Page, PickerItem, Rgb, Role, Shade, SurfaceKey};

use engine::{Flags, Flavour};
use playground::{Action, Back, Playground, State, Tab};
use seed::{Cursor, Seed};
use store::{Saved, Scope, Store};

/// The kind of page it opens.
const KIND: &str = "regex";
const TITLE: &str = "*regex*";

/// What a picker or prompt it opened is for, and for which page.
enum Ask {
    Copy(u64),
    SaveAs(u64, Scope),
    Open(u64),
    Forget(u64),
}

/// The most decorations put in a buffer: Fenix takes 10,000.
const MOST_MARKS: usize = 10_000;
/// How much of a group's colour is in its background.
const TINT: f32 = 0.35;

/// The file marked with the playground's matches, and what they were
/// of: its text then, and the pattern.
struct Marked {
    path: String,
    text: Option<String>,
    of: Option<(String, Flags, Flavour)>,
}

#[derive(Default)]
pub struct RegexExtension {
    pages: HashMap<u64, Playground>,
    asks: HashMap<u64, Ask>,
    /// The file you're in, whose buffer the matches are marked in.
    source: Option<String>,
    marked: Option<Marked>,
}

/// `a` over `b`, `amount` of it.
fn blend(a: Rgb, b: Rgb, amount: f32) -> Rgb {
    let mix = |x: u8, y: u8| (x as f32 * amount + y as f32 * (1.0 - amount)).round() as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The backgrounds groups are on: the whole match the selection's, group
/// `n` the theme palette's `n`th colour, faint on the theme's background.
struct Shades {
    groups: Vec<Rgb>,
}

impl Shades {
    fn now() -> Shades {
        let bg = api::theme().background;
        Shades { groups: api::theme_palette().into_iter().map(|c| blend(c, bg, TINT)).collect() }
    }

    fn group(&self, n: usize) -> Shade {
        match n {
            0 => Shade::Selection,
            _ if self.groups.is_empty() => Shade::Accent,
            n => Shade::Rgb(self.groups[(n - 1) % self.groups.len()]),
        }
    }
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

/// The playground's matches in `text`, as decorations: each match's runs
/// tinted, a group on its colour; an empty match underlines the
/// character after it. At most [`MOST_MARKS`].
fn marks(text: &str, pattern: &str, flags: Flags, flavour: Flavour, shades: &Shades) -> Vec<Decoration> {
    let outcome = engine::run(pattern, flags, flavour, text, None);
    let starts = field::line_starts(text);
    let at = |c: usize| {
        let (l, col) = field::line_col(&starts, c);
        (l as u32, col as u32)
    };
    let n = text.chars().count();
    let mut out = Vec::new();
    for m in &outcome.matches {
        let whole = m.at();
        if whole.is_empty() {
            // Not at a line's end, where there's no character after it.
            if whole.start < n && !starts.contains(&(whole.start + 1)) {
                out.push(Decoration::underline(at(whole.start), at(whole.start + 1), Role::Accent));
            }
        }
        for (r, g) in playground::runs(m) {
            out.push(Decoration::tint(at(r.start), at(r.end), shades.group(g)));
        }
        if out.len() >= MOST_MARKS {
            out.truncate(MOST_MARKS);
            break;
        }
    }
    out
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

/// Where you are: the project (its folder and name), and what to open
/// with -- the regex under the cursor, or the selection.
fn from_here() -> (Option<(String, String)>, Option<Seed>) {
    let Some(here) = api::here() else { return (None, None) };
    let project = here.project.as_ref().map(|p| (p.root.clone(), p.name.clone()));
    let cursor = api::cursor().map(|c| Cursor { column: c.at.1 as usize, selected: c.selected });
    (project, Some(seed::seed(&here.text, &here.language, cursor)))
}

/// A new playground's state, from where you are.
fn opening(project: Option<(String, String)>, seed: Option<Seed>, tab: Tab) -> State {
    let mut state = State { project, tab, ..State::default() };
    match seed {
        Some(Seed::Pattern(found)) => {
            state.pattern = found.pattern;
            if let Some(flags) = found.flags {
                state.flags = flags;
            }
        }
        Some(Seed::Selection(text) | Seed::Line(text)) => state.text = text,
        None => {}
    }
    state
}

impl RegexExtension {
    /// The playground, on where you are: the one open brought forward,
    /// else a new one.
    fn open(&mut self, tab: Tab) {
        let (project, seed) = from_here();
        if let Some((&id, p)) = self.pages.iter_mut().max_by_key(|(&id, _)| id) {
            if let Some(seed) = &seed {
                p.reseed(seed, tab);
            } else {
                p.state.tab = tab;
            }
            if project.is_some() {
                p.state.project = project;
            }
            api::page::refresh(id);
            api::page::focus(id);
            if let Some(path) = api::here().and_then(|h| h.path) {
                self.source = Some(path);
            }
            return self.mark(true);
        }
        // Beside the file you're in, whose matches are marked as you type.
        let id = match api::here().and_then(|h| h.path) {
            Some(path) => {
                self.source = Some(path);
                let id = api::page::open_beside(KIND, TITLE, 0.5);
                api::page::focus(id);
                id
            }
            None => api::page::open(KIND, TITLE),
        };
        self.pages.insert(id, Playground::new(opening(project, seed, tab)));
        self.mark(false);
    }

    /// The newest playground: the one whose matches the file shows.
    fn newest(&self) -> Option<&Playground> {
        self.pages.iter().max_by_key(|(&id, _)| id).map(|(_, p)| p)
    }

    /// The file you're in marked with the playground's matches -- again
    /// when its text changed (`reread`), or the pattern -- and one marked
    /// before, that you've left, cleared. None while no playground's open.
    fn mark(&mut self, reread: bool) {
        let want = self.newest().map(|p| (p.state.pattern.clone(), p.state.flags, p.state.flavour)).zip(self.source.clone());
        if let Some(m) = self.marked.take_if(|m| want.as_ref().is_none_or(|(_, path)| !same_file(&m.path, path))) {
            let _ = decorations::set(&decorations::file(m.path), &[]);
        }
        let Some((of, path)) = want else { return };
        let m = self.marked.get_or_insert_with(|| Marked { path: path.clone(), text: None, of: None });
        if reread || m.text.is_none() {
            m.text = api::buffer::text(&path);
        } else if m.of.as_ref() == Some(&of) {
            return;
        }
        let Some(text) = &m.text else { return };
        let decs = if of.0.is_empty() { Vec::new() } else { marks(text, &of.0, of.1, of.2, &Shades::now()) };
        m.of = Some(of);
        if let Err(e) = decorations::set(&decorations::file(path), &decs) {
            api::warn!("marking the matches: {e}");
        }
    }

    /// The page to lay out: `height` lines of it from where it's
    /// scrolled, and its backgrounds.
    fn styled(&mut self, page: u64, width: u32, height: u32) -> Option<Page> {
        let p = self.pages.get_mut(&page)?;
        let laid = playground::laid(p, width as usize);
        let view = p.view(laid, height as usize);
        let shades = Shades::now();
        let mut extra = Page::canvas(view.canvas).typing(p.typing()).more_keys(true).mouse(true);
        for (line, cols, back) in view.backs {
            let cells = Cells { line: line as u32, start: cols.start as u32, end: cols.end as u32 };
            extra = match back {
                Back::Group(n) => extra.background(cells, shades.group(n)),
                Back::Empty => extra.underline(cells, Role::Accent),
            };
        }
        Some(extra)
    }

    fn act(&mut self, page: u64, action: Action) {
        let Some(p) = self.pages.get(&page) else { return };
        match action {
            Action::None => {}
            Action::Copy(text, what) => {
                api::copy(&text);
                api::notify(&format!("copied {what}"));
            }
            Action::PickCopy => {
                if p.state.pattern.is_empty() {
                    return error("there's no pattern to copy yet");
                }
                let mut items: Vec<PickerItem> = escape::LANGS.iter().map(|&l| PickerItem::new(format!("the pattern for {}", l.name()), format!("lang:{}", l.id())).detail(escape::copy(l, &p.state.pattern, p.state.flags).code)).collect();
                if let Some(result) = &p.outcome.replaced {
                    items.push(PickerItem::new("the replacement's result", "result").detail(result.lines().next().unwrap_or("").to_string()));
                }
                if !p.outcome.matches.is_empty() {
                    items.push(PickerItem::new("the matches, a line each", "matches").detail(format!("{} of them", p.outcome.matches.len())));
                }
                let id = api::pick("Copy", &items);
                self.asks.insert(id, Ask::Copy(page));
            }
            Action::Save { global } => {
                if p.state.pattern.is_empty() {
                    return error("there's no pattern to save yet");
                }
                let scope = match (&p.state.project, global) {
                    (Some((root, _)), false) => Scope::Project(root.clone()),
                    _ => Scope::Global,
                };
                let label = match (&scope, &p.state.project) {
                    (Scope::Project(_), Some((_, name))) => format!("Save the pattern for {name} as"),
                    _ => "Save the pattern, for every project, as".to_string(),
                };
                let id = api::prompt(&label, "");
                self.asks.insert(id, Ask::SaveAs(page, scope));
            }
            Action::Open | Action::Forget => {
                let store = Store::load(Path::new(store::KEPT));
                let project = p.state.project.as_ref().map(|(root, _)| root.as_str());
                let items: Vec<PickerItem> = store
                    .visible(project)
                    .iter()
                    .map(|(scope, s)| {
                        let (key, whose) = match scope {
                            Scope::Global => ("g", "yours".to_string()),
                            Scope::Project(_) => ("p", p.state.project.as_ref().map(|(_, n)| n.clone()).unwrap_or_default()),
                        };
                        PickerItem::new(s.name.clone(), format!("{key}\t{}", s.name)).detail(format!("{whose} · {}", s.pattern))
                    })
                    .collect();
                if items.is_empty() {
                    return api::notify("no saved patterns yet: w saves one");
                }
                let forget = action == Action::Forget;
                let id = api::pick(if forget { "Forget a saved pattern" } else { "Open a saved pattern" }, &items);
                self.asks.insert(id, if forget { Ask::Forget(page) } else { Ask::Open(page) });
            }
        }
    }

    /// The saved pattern a picker's value names.
    fn scope_of(&self, page: u64, value: &str) -> Option<(Scope, String)> {
        let (key, name) = value.split_once('\t')?;
        let scope = match key {
            "p" => Scope::Project(self.pages.get(&page)?.state.project.as_ref()?.0.clone()),
            _ => Scope::Global,
        };
        Some((scope, name.to_string()))
    }

    fn picked(&mut self, id: u64, value: Option<String>) {
        let (Some(ask), Some(value)) = (self.asks.remove(&id), value) else { return };
        match ask {
            Ask::Copy(page) => {
                let Some(p) = self.pages.get(&page) else { return };
                let (text, what) = match value.as_str() {
                    "result" => (p.outcome.replaced.clone().unwrap_or_default(), "the result".to_string()),
                    "matches" => (p.matches_text(), format!("{} matches", p.outcome.matches.len())),
                    v => {
                        let Some(lang) = v.strip_prefix("lang:").and_then(escape::Lang::from_id) else { return };
                        let copied = escape::copy(lang, &p.state.pattern, p.state.flags);
                        let lost = if copied.lost.is_empty() { String::new() } else { format!(" (without {}: {} hasn't it)", copied.lost.iter().collect::<String>(), lang.name()) };
                        (copied.code, format!("the pattern for {}{lost}", lang.name()))
                    }
                };
                api::copy(&text);
                api::notify(&format!("copied {what}"));
            }
            Ask::Open(page) => {
                let Some((scope, name)) = self.scope_of(page, &value) else { return };
                let store = Store::load(Path::new(store::KEPT));
                let project = match &scope {
                    Scope::Project(root) => Some(root.as_str()),
                    Scope::Global => None,
                };
                let found = store.visible(project).into_iter().find(|(s, saved)| *s == scope && saved.name == name).map(|(_, s)| s.clone());
                if let (Some(saved), Some(p)) = (found, self.pages.get_mut(&page)) {
                    p.open_saved(&saved);
                    p.note = Some((format!("opened {}", saved.name), false));
                    api::page::refresh(page);
                }
            }
            Ask::Forget(page) => {
                let Some((scope, name)) = self.scope_of(page, &value) else { return };
                let mut store = Store::load(Path::new(store::KEPT));
                if store.remove(&scope, &name) {
                    match store.save(Path::new(store::KEPT)) {
                        Ok(()) => api::notify(&format!("forgot {name}")),
                        Err(e) => error(&e),
                    }
                }
            }
            Ask::SaveAs(..) => {}
        }
    }

    fn prompted(&mut self, id: u64, text: Option<String>) {
        let Some(Ask::SaveAs(page, scope)) = self.asks.remove(&id) else { return };
        let Some(name) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else { return };
        let Some(p) = self.pages.get_mut(&page) else { return };
        let saved = Saved { name: name.clone(), pattern: p.state.pattern.clone(), flags: p.state.flags, flavour: p.state.flavour, replacement: p.state.replacement.clone().unwrap_or_default() };
        let mut store = Store::load(Path::new(store::KEPT));
        store.put(&scope, saved);
        let (said, bad) = match store.save(Path::new(store::KEPT)) {
            Ok(()) => (format!("saved {name}{}", if scope == Scope::Global { ", for every project" } else { "" }), false),
            Err(e) => (e, true),
        };
        p.note = Some((said, bad));
        api::page::refresh(page);
    }
}

impl Extension for RegexExtension {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "regex.open" => self.open(Tab::Playground),
            "regex.cheatsheet" => self.open(Tab::Cheatsheet),
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Picked((id, value)) => self.picked(id, value),
            Event::Prompted((id, text)) => self.prompted(id, text),
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        self.styled(page, width, height).unwrap_or_default()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key);
        api::page::refresh(page);
        self.act(page, action);
        self.mark(false);
    }

    /// PageUp and PageDown; Ctrl+Z and Ctrl+Shift+Z (or Ctrl+Y) the
    /// fields' undo and redo.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        match key.key {
            SurfaceKey::Named(NamedKey::PageUp) => p.page_key(false),
            SurfaceKey::Named(NamedKey::PageDown) => p.page_key(true),
            SurfaceKey::Char(c) if key.mods.ctrl && !key.mods.alt => match c.to_ascii_lowercase() {
                'z' => p.undo(key.mods.shift),
                'y' => p.undo(true),
                _ => return,
            },
            _ => return,
        }
        api::page::refresh(page);
        self.mark(false);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = match event.action {
            MouseAction::Press(press) if press.button == MouseButton::Left => p.click(event.line as usize, event.cell as usize, press.clicks),
            MouseAction::Wheel(lines) => {
                p.scroll(-(lines as i64));
                Action::None
            }
            _ => return,
        };
        api::page::refresh(page);
        self.act(page, action);
        self.mark(false);
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.paste(text);
            api::page::refresh(page);
        }
        self.mark(false);
    }

    /// The file you're in: its matches marked, the last one's cleared.
    fn focus_moved(&mut self, focus: Focus) {
        if let Focus::File(Some(path)) = focus {
            if self.source.as_deref().is_none_or(|s| !same_file(s, &path)) {
                self.source = Some(path);
                self.mark(true);
            }
        }
    }

    /// The file marked was typed in: marked again.
    fn buffer_changed(&mut self, what: Changed) {
        if let (Changed::File(path), Some(m)) = (what, &self.marked) {
            if same_file(&m.path, &path) {
                self.mark(true);
            }
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.pages.get(&page).and_then(|p| serde_json::to_string(&p.kept()).ok())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        let Ok(state) = serde_json::from_str::<State>(state) else { return false };
        self.pages.insert(page, Playground::new(state));
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
        self.asks.retain(|_, ask| !matches!(ask, Ask::Copy(p) | Ask::SaveAs(p, _) | Ask::Open(p) | Ask::Forget(p) if *p == page));
        // The last one closed: the file's marks go with it.
        self.mark(false);
    }
}

export!(RegexExtension);

#[cfg(test)]
mod tests {
    use super::*;

    fn shades() -> Shades {
        Shades { groups: vec![(1, 1, 1), (2, 2, 2)] }
    }

    #[test]
    fn matches_in_a_buffer_are_tinted_a_group_its_colour() {
        let d = marks("ab 12\ncd 345", r"(\d)\d", Flags::default(), Flavour::Rust, &shades());
        assert_eq!(
            d,
            [
                Decoration::tint((0, 3), (0, 4), Shade::Rgb((1, 1, 1))),
                Decoration::tint((0, 4), (0, 5), Shade::Selection),
                Decoration::tint((1, 3), (1, 4), Shade::Rgb((1, 1, 1))),
                Decoration::tint((1, 4), (1, 5), Shade::Selection),
            ]
        );
        assert_eq!(shades().group(3), Shade::Rgb((1, 1, 1)), "the colours go round");
    }

    #[test]
    fn an_empty_match_underlines_the_character_after_it() {
        let d = marks("ab cd", r"\b", Flags::default(), Flavour::Rust, &shades());
        let at: Vec<(u32, u32)> = d.iter().map(|d| d.start).collect();
        assert_eq!(at, [(0, 0), (0, 2), (0, 3)], "none at the text's end");
        assert_eq!(d[0], Decoration::underline((0, 0), (0, 1), Role::Accent));
        let m = Flags { m: true, ..Flags::default() };
        assert!(marks("ab\ncd", "$", m, Flavour::Rust, &shades()).is_empty());
        assert!(marks("ab", "(", Flags::default(), Flavour::Rust, &shades()).is_empty(), "an error marks nothing");
    }

    #[test]
    fn a_file_is_the_same_however_fenix_spells_it() {
        assert!(same_file("C:\\Work\\a.rs", "c:/work/a.rs"));
        assert!(!same_file("/a/b.rs", "/a/B.rs"));
    }
}
