//! The playground's page: the pattern, the test text and the replacement
//! as fields, matched live -- each group in its own colour -- with the
//! result, the pattern explained, how other flavours differ, and every
//! match with its groups. Its second tab is the cheatsheet, whose
//! examples load into the first.

use std::ops::Range;

use fenix_extension_api::grid::{edit_line, fit, frame, insert_at, wrap, Filter, Grid};
use fenix_extension_api::{Canvas, CellSpan, Cells, Key, Role};
use serde::{Deserialize, Serialize};

use crate::cheatsheet::{self, Entry};
use crate::engine::{self, Flags, Flavour, Outcome};
use crate::explain::{self, Explained};
use crate::field;
use crate::notes;
use crate::seed::Seed;

/// The labels' column.
const LABEL: usize = 10;
/// The most rows of the test text, and of the result, drawn.
const MOST_ROWS: usize = 400;
/// The most matches listed on the page.
const MOST_LISTED: usize = 200;
/// The most of the test text kept with the session.
const KEPT_TEXT: usize = 64 * 1024;

/// The most steps Ctrl+Z goes back.
const MOST_UNDOS: usize = 100;

/// A background on the page: a group's colour (group 0 the whole
/// match's), or the place of an empty match, the character after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    Group(usize),
    Empty,
}

/// What's under a cell, for a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A row of a field's text: the cell each char from `start` is
    /// drawn at, and the one after the last.
    Field { field: Field, start: usize, xs: Vec<usize> },
    /// A field's label.
    Label(Field),
    /// A match in the list: its row (after the fields).
    Row(usize),
    Flag(char),
    Flavour,
    Tab(Tab),
    Cheat(usize),
}

/// A page laid out: its canvas, the backgrounds on it, and what a click
/// on it finds; lines from the page's first.
#[derive(Debug)]
pub struct Laid {
    pub canvas: Canvas,
    pub backs: Vec<(usize, Range<usize>, Back)>,
    pub hits: Vec<(usize, Range<usize>, Hit)>,
}

/// The fields as they were, for Ctrl+Z.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snap {
    pattern: String,
    text: String,
    replacement: Option<String>,
    carets: [usize; 3],
}

/// Within match `m`, the runs of chars and the group each is drawn in:
/// the innermost there (group 0, the match, where none is).
pub fn runs(m: &engine::Match) -> Vec<(Range<usize>, usize)> {
    let at = m.at();
    let mut by = vec![0; at.len()];
    for (g, r) in m.groups.iter().enumerate().skip(1) {
        if let Some(r) = r {
            for k in r.start.max(at.start)..r.end.min(at.end) {
                by[k - at.start] = g;
            }
        }
    }
    let mut out: Vec<(Range<usize>, usize)> = Vec::new();
    for (k, &g) in by.iter().enumerate() {
        match out.last_mut() {
            Some((r, last)) if *last == g && r.end == at.start + k => r.end += 1,
            _ => out.push((at.start + k..at.start + k + 1, g)),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Playground,
    Cheatsheet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Pattern,
    Text,
    Replacement,
}

const FIELDS: [Field; 3] = [Field::Pattern, Field::Text, Field::Replacement];

/// What a key asks the extension to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    /// Onto the clipboard: the text, and what it is.
    Copy(String, String),
    /// The pattern copied for a language: the picker of them.
    PickCopy,
    /// Saved, under a name asked for: in the project (false) or yours.
    Save { global: bool },
    /// The saved patterns, to open one.
    Open,
    /// The saved patterns, to forget one.
    Forget,
}

/// What a page keeps with the session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub pattern: String,
    pub text: String,
    pub replacement: Option<String>,
    pub flags: Flags,
    pub flavour: Flavour,
    pub tab: Tab,
    /// The project it was opened in, for its saved patterns: its folder
    /// and name.
    pub project: Option<(String, String)>,
}

pub struct Playground {
    pub state: State,
    /// The carets, in chars, of the pattern, the text and the replacement.
    carets: [usize; 3],
    /// The row picked: a field (0-2), then the matches.
    sel: usize,
    editing: Option<Field>,
    pub note: Option<(String, bool)>,
    pub outcome: Outcome,
    pub explained: Option<Explained>,
    pub notes: Vec<String>,
    cheat_sel: usize,
    pub filter: Filter,
    /// The first line shown, when the page is taller than its pane; kept
    /// on the focus after a key.
    pub top: usize,
    follow: bool,
    /// The pane's height, last laid out (0 not known).
    pub height: usize,
    /// What the last layout's cells are, for a click.
    hits: Vec<(usize, Range<usize>, Hit)>,
    history: Vec<Snap>,
    future: Vec<Snap>,
    /// The field last typed in: typing on in it is the same undo step.
    typed: Option<Field>,
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

impl Playground {
    pub fn new(state: State) -> Playground {
        let carets = [chars(&state.pattern), 0, state.replacement.as_deref().map_or(0, chars)];
        let mut p = Playground {
            state,
            carets,
            sel: 0,
            editing: None,
            note: None,
            outcome: Outcome::default(),
            explained: None,
            notes: Vec::new(),
            cheat_sel: 0,
            filter: Filter::default(),
            top: 0,
            follow: true,
            height: 0,
            hits: Vec::new(),
            history: Vec::new(),
            future: Vec::new(),
            typed: None,
        };
        p.rerun();
        p
    }

    /// What's kept with the session: the text cut to [`KEPT_TEXT`].
    pub fn kept(&self) -> State {
        let mut s = self.state.clone();
        if s.text.len() > KEPT_TEXT {
            let mut end = KEPT_TEXT;
            while !s.text.is_char_boundary(end) {
                end -= 1;
            }
            s.text.truncate(end);
        }
        s
    }

    /// Matched again, explained again: after anything changed.
    pub fn rerun(&mut self) {
        let s = &self.state;
        self.outcome = engine::run(&s.pattern, s.flags, s.flavour, &s.text, s.replacement.as_deref());
        self.explained = if s.pattern.is_empty() { None } else { explain::explain(&s.pattern, s.flags, s.flavour) };
        self.notes = self.explained.as_ref().map(|e| notes::notes(&e.features, s.flags, s.flavour)).unwrap_or_default();
        let rows = FIELDS.len() + self.outcome.matches.len().min(MOST_LISTED);
        self.sel = self.sel.min(rows - 1);
    }

    pub fn typing(&self) -> bool {
        self.editing.is_some() || self.filter.typing
    }

    fn snap(&self) -> Snap {
        Snap { pattern: self.state.pattern.clone(), text: self.state.text.clone(), replacement: self.state.replacement.clone(), carets: self.carets }
    }

    fn put_back(&mut self, s: Snap) {
        self.state.pattern = s.pattern;
        self.state.text = s.text;
        self.state.replacement = s.replacement;
        self.carets = s.carets;
        self.rerun();
    }

    /// `before`, the fields as they were before a change, kept to undo it.
    fn changed_from(&mut self, before: Snap) {
        if self.history.last() != Some(&before) {
            self.history.push(before);
            if self.history.len() > MOST_UNDOS {
                self.history.remove(0);
            }
        }
        self.future.clear();
    }

    /// Ctrl+Z: the fields' last change undone; Ctrl+Shift+Z done again.
    pub fn undo(&mut self, redo: bool) {
        self.note = None;
        self.typed = None;
        let now = self.snap();
        let (from, to) = if redo { (&mut self.future, &mut self.history) } else { (&mut self.history, &mut self.future) };
        let Some(s) = from.pop() else {
            self.note = Some((if redo { "nothing to redo" } else { "nothing to undo" }.into(), false));
            return;
        };
        to.push(now);
        self.put_back(s);
        self.follow = true;
    }

    /// An example from the cheatsheet, to try.
    pub fn load(&mut self, e: &Entry) {
        self.changed_from(self.snap());
        self.state.pattern = e.pattern.to_string();
        self.state.text = e.text.to_string();
        self.state.flags = e.flags();
        self.state.flavour = e.flavour();
        self.state.replacement = (e.syntax == "$1 ${name}").then(|| "${first}: $2".to_string());
        self.carets = [chars(e.pattern), 0, self.state.replacement.as_deref().map_or(0, chars)];
        self.state.tab = Tab::Playground;
        self.sel = 0;
        self.editing = None;
        self.rerun();
    }

    /// A saved pattern, opened: the text stays.
    pub fn open_saved(&mut self, s: &crate::store::Saved) {
        self.changed_from(self.snap());
        self.state.pattern = s.pattern.clone();
        self.state.flags = s.flags;
        self.state.flavour = s.flavour;
        self.state.replacement = (!s.replacement.is_empty()).then(|| s.replacement.clone());
        self.carets = [chars(&s.pattern), 0, s.replacement.chars().count()];
        self.state.tab = Tab::Playground;
        self.rerun();
    }

    /// Where you are, brought to it as it's opened again: a regex as the
    /// pattern, a selection as the text. A line with no regex leaves the
    /// text you had.
    pub fn reseed(&mut self, seed: &Seed, tab: Tab) {
        let before = self.snap();
        match seed {
            Seed::Pattern(found) => {
                self.state.pattern = found.pattern.clone();
                if let Some(flags) = found.flags {
                    self.state.flags = flags;
                }
                self.carets[0] = chars(&found.pattern);
            }
            Seed::Selection(text) => {
                self.state.text = text.clone();
                self.carets[1] = 0;
            }
            Seed::Line(_) => {}
        }
        self.state.tab = tab;
        self.editing = None;
        self.typed = None;
        if self.snap() != before {
            self.changed_from(before);
        }
        self.rerun();
    }

    fn field_text(&mut self, f: Field) -> &mut String {
        match f {
            Field::Pattern => &mut self.state.pattern,
            Field::Text => &mut self.state.text,
            Field::Replacement => self.state.replacement.get_or_insert_with(String::new),
        }
    }

    fn index(f: Field) -> usize {
        FIELDS.iter().position(|x| *x == f).unwrap_or(0)
    }

    fn edit(&mut self, f: Field) {
        // Writing a replacement turns it on.
        self.field_text(f);
        self.editing = Some(f);
        self.sel = Self::index(f);
    }

    pub fn paste(&mut self, text: &str) {
        if self.filter.typing {
            self.filter.paste(text);
            return;
        }
        let Some(f) = self.editing else { return };
        self.changed_from(self.snap());
        self.typed = None;
        let i = Self::index(f);
        let mut caret = self.carets[i];
        match f {
            Field::Text => field::insert(&mut self.state.text, &mut caret, text),
            // A pattern of one line: lines pasted into it join.
            _ => insert_at(self.field_text(f), &mut caret, &text.replace(['\n', '\r'], "")),
        }
        self.carets[i] = caret;
        self.rerun();
    }

    pub fn key(&mut self, key: Key) -> Action {
        self.note = None;
        self.follow = true;
        match self.state.tab {
            Tab::Cheatsheet => self.cheat_key(key),
            Tab::Playground if self.editing.is_some() => {
                self.typing_key(key);
                Action::None
            }
            Tab::Playground => self.play_key(key),
        }
    }

    fn typing_key(&mut self, key: Key) {
        let Some(f) = self.editing else { return };
        let i = Self::index(f);
        let next = |by: isize| FIELDS[(i as isize + by).rem_euclid(FIELDS.len() as isize) as usize];
        match (f, key) {
            (_, Key::Escape) | (Field::Pattern | Field::Replacement, Key::Enter) => self.editing = None,
            (_, Key::Tab) | (Field::Pattern | Field::Replacement, Key::Down) => self.edit(next(1)),
            (_, Key::BackTab) | (Field::Replacement, Key::Up) => self.edit(next(-1)),
            (Field::Pattern, Key::Up) => {}
            (f, key) => {
                let snap = self.snap();
                let mut caret = self.carets[i];
                let text = self.field_text(f);
                // Every edit changes the length; a move doesn't, and needn't
                // match again.
                let before = text.len();
                let took = if f == Field::Text { field::edit_text(text, &mut caret, key) } else { edit_line(text, &mut caret, key) };
                let changed = text.len() != before;
                if took {
                    self.carets[i] = caret;
                }
                if changed {
                    // Characters typed on in one field are one step.
                    let typing = matches!(key, Key::Char(_));
                    if !(typing && self.typed == Some(f)) {
                        self.changed_from(snap);
                    }
                    self.typed = typing.then_some(f);
                    self.rerun();
                } else {
                    self.typed = None;
                }
            }
        }
    }

    /// PageUp, PageDown: in the text being typed, its caret a screen's
    /// lines; else the page a screen.
    pub fn page_key(&mut self, down: bool) {
        let screen = self.height.saturating_sub(4).max(1);
        if self.editing == Some(Field::Text) {
            let mut caret = self.carets[1];
            for _ in 0..screen {
                field::edit_text(&mut self.state.text, &mut caret, if down { Key::Down } else { Key::Up });
            }
            self.carets[1] = caret;
            self.typed = None;
            self.follow = true;
        } else {
            self.scroll(if down { screen as i64 } else { -(screen as i64) });
        }
    }

    /// The page scrolled `by` lines, down when more than 0.
    pub fn scroll(&mut self, by: i64) {
        self.top = (self.top as i64 + by).max(0) as usize;
        self.follow = false;
    }

    /// A click at (`line`, `cell`) of the page shown: in a field, typing
    /// there at the character clicked; on a match, it's picked (a double
    /// click copies it); on a flag, a tab or a cheatsheet entry (a double
    /// click tries it), that.
    pub fn click(&mut self, line: usize, cell: usize, clicks: u8) -> Action {
        let line = line + self.top;
        let Some(hit) = self.hits.iter().find(|(l, xs, _)| *l == line && xs.contains(&cell)).map(|h| h.2.clone()) else { return Action::None };
        self.note = None;
        self.typed = None;
        self.follow = true;
        match hit {
            Hit::Field { field, start, xs } => {
                self.edit(field);
                let k = xs.iter().position(|&x| x >= cell).unwrap_or(xs.len().saturating_sub(1));
                self.carets[Self::index(field)] = start + k;
            }
            Hit::Label(f) => {
                self.edit(f);
                self.carets[Self::index(f)] = chars(self.field_text(f));
            }
            Hit::Row(row) => {
                self.editing = None;
                self.sel = row.min(self.rows() - 1);
                if clicks >= 2 {
                    return self.copy_match();
                }
            }
            Hit::Flag(c) => {
                self.state.flags.toggle(c);
                self.rerun();
            }
            Hit::Flavour => {
                self.state.flavour = self.state.flavour.other();
                self.rerun();
            }
            Hit::Tab(tab) => {
                self.editing = None;
                self.filter.typing = false;
                self.state.tab = tab;
            }
            Hit::Cheat(k) => {
                self.filter.typing = false;
                self.cheat_sel = k;
                if clicks >= 2 {
                    if let Some(&(_, e)) = self.cheats().get(k) {
                        self.load(e);
                    }
                }
            }
        }
        Action::None
    }

    /// `laid`, the whole page, cut to the `height` lines it's shown in (0:
    /// all of it), from [`Playground::top`] -- moved to keep the focus in
    /// view after a key. What a click finds is kept.
    pub fn view(&mut self, laid: Laid, height: usize) -> Laid {
        self.height = height;
        let Laid { canvas, backs, hits } = laid;
        self.hits = hits;
        let total = canvas.text.lines().count();
        if height == 0 || total <= height {
            self.top = 0;
            return Laid { canvas, backs, hits: Vec::new() };
        }
        if self.follow {
            if let Some(f) = canvas.focus.as_ref().map(|f| f.line as usize) {
                if f < self.top {
                    self.top = f.saturating_sub(1);
                } else if f >= self.top + height {
                    self.top = f + 2 - height;
                }
            }
            self.follow = false;
        }
        self.top = self.top.min(total - height);
        let (top, end) = (self.top as u32, (self.top + height) as u32);
        let keep = |c: &Cells| (top..end).contains(&c.line);
        let shift = |mut c: Cells| {
            c.line -= top;
            c
        };
        let canvas = Canvas {
            text: canvas.text.lines().skip(self.top).take(height).collect::<Vec<_>>().join("\n"),
            spans: canvas.spans.into_iter().filter(|s| keep(&s.cells)).map(|s| CellSpan { cells: shift(s.cells), role: s.role }).collect(),
            rules: canvas.rules.into_iter().filter(keep).map(shift).collect(),
            focus: canvas.focus.filter(keep).map(shift),
            panels: canvas.panels.into_iter().filter(keep).map(shift).collect(),
            pictures: Vec::new(),
            popup: None,
        };
        let backs = backs.into_iter().filter(|b| (self.top..self.top + height).contains(&b.0)).map(|(l, r, b)| (l - self.top, r, b)).collect();
        Laid { canvas, backs, hits: Vec::new() }
    }

    fn rows(&self) -> usize {
        FIELDS.len() + self.outcome.matches.len().min(MOST_LISTED)
    }

    fn play_key(&mut self, key: Key) -> Action {
        let on_field = FIELDS.get(self.sel).copied();
        match key {
            Key::Down | Key::Char('j') => self.sel = (self.sel + 1).min(self.rows() - 1),
            Key::Up | Key::Char('k') => self.sel = self.sel.saturating_sub(1),
            Key::Home | Key::Char('g') => self.sel = 0,
            Key::End | Key::Char('G') => self.sel = self.rows() - 1,
            Key::Enter | Key::Char('e') | Key::Char('a') => match on_field {
                Some(f) => {
                    self.edit(f);
                    let i = Self::index(f);
                    if f != Field::Text || key == Key::Char('a') {
                        self.carets[i] = chars(self.field_text(f));
                    }
                }
                None => return self.copy_match(),
            },
            Key::Char('c') => {
                if let Some(f) = on_field {
                    self.changed_from(self.snap());
                    self.field_text(f).clear();
                    self.carets[Self::index(f)] = 0;
                    self.edit(f);
                    self.rerun();
                }
            }
            Key::Delete | Key::Char('d') => {
                if on_field.is_some() {
                    self.changed_from(self.snap());
                }
                match on_field {
                    Some(Field::Replacement) => self.state.replacement = None,
                    Some(f) => self.field_text(f).clear(),
                    None => return Action::None,
                }
                if let Some(f) = on_field {
                    self.carets[Self::index(f)] = 0;
                }
                self.rerun();
            }
            Key::Char(c @ ('i' | 'm' | 's' | 'x' | 'u')) => {
                self.state.flags.toggle(c);
                self.rerun();
            }
            Key::Char('f') => {
                self.state.flavour = self.state.flavour.other();
                self.rerun();
            }
            Key::Tab | Key::BackTab | Key::Char('?') => self.state.tab = Tab::Cheatsheet,
            Key::Char('y') => return Action::PickCopy,
            Key::Char('Y') => return self.copy_match(),
            Key::Char('w') => return Action::Save { global: self.state.project.is_none() },
            Key::Char('W') => return Action::Save { global: true },
            Key::Char('o') => return Action::Open,
            Key::Char('D') => return Action::Forget,
            _ => {}
        }
        Action::None
    }

    /// The match picked, onto the clipboard.
    fn copy_match(&self) -> Action {
        let Some(m) = self.sel.checked_sub(FIELDS.len()).and_then(|i| self.outcome.matches.get(i)) else {
            return Action::None;
        };
        Action::Copy(slice(&self.state.text, m.at()), "the match".into())
    }

    /// The matches, a line each.
    pub fn matches_text(&self) -> String {
        self.outcome.matches.iter().map(|m| slice(&self.state.text, m.at())).collect::<Vec<_>>().join("\n")
    }

    fn cheat_key(&mut self, key: Key) -> Action {
        if self.filter.typing {
            if self.filter.key(key) {
                self.cheat_sel = 0;
            }
            return Action::None;
        }
        let shown = self.cheats().len();
        match key {
            Key::Down | Key::Char('j') => self.cheat_sel = (self.cheat_sel + 1).min(shown.saturating_sub(1)),
            Key::Up | Key::Char('k') => self.cheat_sel = self.cheat_sel.saturating_sub(1),
            Key::Home | Key::Char('g') => self.cheat_sel = 0,
            Key::End | Key::Char('G') => self.cheat_sel = shown.saturating_sub(1),
            Key::Char('/') => self.filter.start(),
            Key::Enter => {
                if let Some((_, e)) = self.cheats().get(self.cheat_sel) {
                    let e: &Entry = e;
                    self.load(e);
                }
            }
            Key::Tab | Key::BackTab | Key::Char('?') => self.state.tab = Tab::Playground,
            _ => {}
        }
        Action::None
    }

    /// The cheatsheet's entries the filter leaves.
    fn cheats(&self) -> Vec<(&'static str, &'static Entry)> {
        cheatsheet::all().filter(|(section, e)| self.filter.matches(&format!("{section} {} {} {}", e.syntax, e.meaning, e.pattern))).collect()
    }
}

/// Chars `r` of `text`.
fn slice(text: &str, r: Range<usize>) -> String {
    text.chars().skip(r.start).take(r.len()).collect()
}

/// A field's text shown in `width` columns: the slice of it that fits
/// around the caret, and where that slice starts.
fn window(len: usize, caret: Option<usize>, width: usize) -> usize {
    let width = width.max(2);
    match caret {
        Some(c) if c + 1 >= width => (c + 2).saturating_sub(width).min(len),
        _ => 0,
    }
}

/// One row: chars `cols` of `cs` at (`y`, `x`), each in its role, with
/// the caret drawn before char `caret` when it's in the row (or after
/// the last, at `cols.end`). Runs of a role are put as one span. The
/// cell each char went to, and the one after the last.
fn draw_row(g: &mut Grid, y: usize, x: usize, cs: &[char], roles: &[Role], cols: Range<usize>, caret: Option<usize>) -> Vec<usize> {
    let mut xs = Vec::with_capacity(cols.len() + 1);
    let mut col = x;
    let mut run = String::new();
    let mut run_len = 0;
    let mut role = Role::Text;
    let flush = |g: &mut Grid, run: &mut String, run_len: &mut usize, role: &Role, col: &mut usize| {
        if !run.is_empty() {
            *col = g.put(y, *col, run, role.clone());
            run.clear();
            *run_len = 0;
        }
    };
    for (i, &c) in cs.iter().enumerate().take(cols.end).skip(cols.start) {
        if caret == Some(i) {
            flush(g, &mut run, &mut run_len, &role, &mut col);
            col = g.put(y, col, "▏", Role::Accent);
        }
        let r = roles.get(i).cloned().unwrap_or(Role::Text);
        if r != role {
            flush(g, &mut run, &mut run_len, &role, &mut col);
            role = r;
        }
        xs.push(col + run_len);
        run_len += 1;
        run.push(match c {
            '\t' | '\r' => ' ',
            c if c.is_control() => '·',
            c => c,
        });
    }
    flush(g, &mut run, &mut run_len, &role, &mut col);
    xs.push(col);
    if caret == Some(cols.end) {
        g.put(y, col, "▏", Role::Accent);
    }
    xs
}

/// The group each char of the test text is drawn in, if it's in a
/// match; and where empty matches are.
fn text_marks(len: usize, outcome: &Outcome) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut marks = vec![None; len];
    let mut empty = Vec::new();
    for m in &outcome.matches {
        if m.at().is_empty() {
            empty.push(m.at().start);
        }
        for (r, g) in runs(m) {
            for k in marks.iter_mut().take(r.end.min(len)).skip(r.start) {
                *k = Some(g);
            }
        }
    }
    (marks, empty)
}

/// Backgrounds for a row drawn at `xs` (from [`draw_row`]), chars
/// `start..` of a text marked `marks`: a run a group.
fn mark_row(out: &mut Laid, y: usize, xs: &[usize], start: usize, marks: &[Option<usize>]) {
    let mut k = 0;
    while k + 1 < xs.len() {
        let Some(g) = marks.get(start + k).copied().flatten() else {
            k += 1;
            continue;
        };
        let from = k;
        while k + 1 < xs.len() && marks.get(start + k).copied().flatten() == Some(g) && (k == from || xs[k] == xs[k - 1] + 1) {
            k += 1;
        }
        out.backs.push((y, xs[from]..xs[k - 1] + 1, Back::Group(g)));
    }
}

#[cfg(test)]
fn layout(p: &Playground, cols: usize) -> Canvas {
    laid(p, cols).canvas
}

/// The page laid out `cols` wide, with its backgrounds and what clicks
/// find.
pub fn laid(p: &Playground, cols: usize) -> Laid {
    let mut out = Laid { canvas: Grid::new().finish(), backs: Vec::new(), hits: Vec::new() };
    let (left, width) = frame(cols, 120);
    let mut g = Grid::new();
    g.put(1, left, "Regex", Role::Title);
    let tabs = [("playground", Tab::Playground), ("cheatsheet", Tab::Cheatsheet)];
    let all: usize = tabs.iter().map(|(t, _)| t.len() + 3).sum();
    let mut x = (left + width).saturating_sub(all);
    for (name, tab) in tabs {
        let end = g.put(1, x, name, if p.state.tab == tab { Role::Title } else { Role::Muted });
        out.hits.push((1, x..end, Hit::Tab(tab)));
        x = end + 3;
    }
    if let Some((text, bad)) = &p.note {
        g.put(2, left, &fit(text, width), if *bad { Role::Bad } else { Role::Good });
    }
    match p.state.tab {
        Tab::Playground => playground(p, &mut g, &mut out, left, width),
        Tab::Cheatsheet => cheats(p, &mut g, &mut out, left, width),
    }
    out.canvas = g.finish();
    out
}

fn playground(p: &Playground, g: &mut Grid, out: &mut Laid, left: usize, width: usize) {
    let s = &p.state;
    let o = &p.outcome;
    let x = left + LABEL;
    let fw = width.saturating_sub(LABEL).max(10);
    let label = |g: &mut Grid, out: &mut Laid, y: usize, text: &str, row: usize| {
        g.put(y, left, text, if p.sel == row && p.editing.is_none() { Role::Title } else { Role::Muted });
        out.hits.push((y, left..x, Hit::Label(FIELDS[row])));
        if p.sel == row && p.editing.is_none() {
            g.focus(y, left..left + width);
        }
    };

    // The pattern: its groups in their colours, the error in red.
    let mut y = 3;
    label(g, out, y, "pattern", 0);
    let pc: Vec<char> = s.pattern.chars().collect();
    let mut roles = vec![Role::Text; pc.len()];
    // Each group on its colour, as its matches are in the text.
    let mut marks = vec![None; pc.len()];
    if let Some(e) = &p.explained {
        for piece in e.pieces.iter().filter(|piece| piece.group.is_some()) {
            for m in marks.iter_mut().take(piece.at.end.min(pc.len())).skip(piece.at.start) {
                *m = piece.group.map(|n| n as usize);
            }
        }
    }
    let error_at = o.error.as_ref().and_then(|e| e.at.clone());
    if let Some(at) = &error_at {
        for r in roles.iter_mut().take(at.end.min(pc.len())).skip(at.start) {
            *r = Role::Bad;
        }
    }
    let editing = p.editing == Some(Field::Pattern);
    let caret = editing.then_some(p.carets[0].min(pc.len()));
    let from = window(pc.len(), caret, fw);
    let to = (from + fw - usize::from(editing)).min(pc.len());
    if pc.is_empty() && !editing {
        g.put(y, x, "Enter to type a pattern", Role::Muted);
        out.hits.push((y, x..x + fw, Hit::Label(Field::Pattern)));
    } else {
        let xs = draw_row(g, y, x, &pc, &roles, from..to, caret);
        mark_row(out, y, &xs, from, &marks);
        out.hits.push((y, x..x + fw, Hit::Field { field: Field::Pattern, start: from, xs }));
    }
    if editing {
        g.panel(y, x..x + fw);
    }

    // Under it: where the error is, or how it went.
    y += 1;
    match &o.error {
        Some(e) => {
            let mut msg_x = x;
            if let Some(at) = &error_at {
                let shift = |i: usize| i.saturating_sub(from) + usize::from(caret.is_some_and(|c| i >= c));
                if at.start >= from && at.start <= to {
                    let start = x + shift(at.start);
                    let end = x + shift(at.end.min(to).max(at.start + 1));
                    let marks = "^".repeat(end.saturating_sub(start).max(1));
                    msg_x = g.put(y, start, &marks, Role::Bad) + 1;
                }
            }
            // Too little room after the marks: the message under them.
            if msg_x + 20 > x + fw {
                y += 1;
                msg_x = x;
            }
            g.put(y, msg_x, &fit(&e.message, (x + fw).saturating_sub(msg_x)), Role::Bad);
        }
        None if s.pattern.is_empty() => {}
        None => {
            let n = o.matches.len();
            let groups = o.names.len().saturating_sub(1);
            let mut said = format!("{}{} match{}", n, if o.more { "+" } else { "" }, if n == 1 { "" } else { "es" });
            if groups > 0 {
                said.push_str(&format!(" · {groups} group{}", if groups == 1 { "" } else { "s" }));
            }
            if let Some(stop) = &o.stopped {
                said.push_str(&format!(" · stopped: {stop}"));
            }
            g.put(y, x, &fit(&said, fw), if o.stopped.is_some() { Role::Warn } else if n == 0 { Role::Muted } else { Role::Good });
        }
    }

    // The flags, and the flavour.
    y += 1;
    g.put(y, left, "flags", Role::Muted);
    let mut fx = x;
    for (c, on, name) in [('i', s.flags.i, "ignore case"), ('m', s.flags.m, "multi-line"), ('s', s.flags.s, ". matches \\n"), ('x', s.flags.x, "extended"), ('u', s.flags.u, "Unicode")] {
        if fx + name.len() + 4 > x + fw {
            break;
        }
        let start = fx;
        fx = g.put(y, fx, &c.to_string(), if on { Role::Accent } else { Role::Muted }) + 1;
        fx = g.put(y, fx, name, if on { Role::Text } else { Role::Muted });
        out.hits.push((y, start..fx, Hit::Flag(c)));
        fx += 3;
    }
    y += 1;
    g.put(y, left, "flavour", Role::Muted);
    let fx = g.put(y, x, s.flavour.name(), Role::Text);
    out.hits.push((y, x..fx, Hit::Flavour));
    let other = format!("  f: {}{}", s.flavour.other().name(), if s.flavour == Flavour::Rust { " (lookaround, backreferences)" } else { "" });
    g.put(y, fx, &fit(&other, (x + fw).saturating_sub(fx)), Role::Muted);

    // The test text, its matches coloured.
    y += 2;
    label(g, out, y, "text", 1);
    let tc: Vec<char> = s.text.chars().collect();
    let editing = p.editing == Some(Field::Text);
    if tc.is_empty() && !editing {
        g.put(y, x, "Enter to type the text to match", Role::Muted);
        out.hits.push((y, x..x + fw, Hit::Label(Field::Text)));
        y += 1;
    } else {
        let (marks, empty) = text_marks(tc.len(), o);
        let rows = field::rows(&s.text, fw - 1);
        let caret = editing.then_some(p.carets[1].min(tc.len()));
        let caret_row = caret.map(|c| field::caret_row(&rows, c));
        let first = caret_row.map_or(0, |r| r.saturating_sub(MOST_ROWS - 1));
        for (r, (start, len)) in rows.iter().enumerate().skip(first).take(MOST_ROWS) {
            let here = caret.filter(|_| caret_row == Some(r));
            let xs = draw_row(g, y, x, &tc, &[], *start..start + len, here);
            mark_row(out, y, &xs, *start, &marks);
            // An empty match: the cell after it underlined, at a line's
            // end the one past it.
            let line_end = tc.get(start + len).is_none_or(|&c| c == '\n');
            for &e in empty.iter().filter(|&&e| e >= *start && (e < start + len || (e == start + len && line_end))) {
                let cx = xs[e - start];
                out.backs.push((y, cx..cx + 1, Back::Empty));
            }
            out.hits.push((y, x..x + fw, Hit::Field { field: Field::Text, start: *start, xs }));
            if editing {
                g.panel(y, x..x + fw);
            }
            if caret_row == Some(r) {
                g.focus(y, x..x + fw);
            }
            y += 1;
        }
        if rows.len() > first + MOST_ROWS {
            g.put(y, x, &format!("… {} more rows", rows.len() - first - MOST_ROWS), Role::Muted);
            y += 1;
        }
    }

    // The replacement, and what it makes of the text.
    y += 1;
    label(g, out, y, "replace", 2);
    let editing = p.editing == Some(Field::Replacement);
    match &s.replacement {
        None if !editing => {
            g.put(y, x, &fit("none · Enter to write one: $1, ${name}, $$ for $", fw), Role::Muted);
            out.hits.push((y, x..x + fw, Hit::Label(Field::Replacement)));
        }
        rep => {
            let rc: Vec<char> = rep.as_deref().unwrap_or("").chars().collect();
            let caret = editing.then_some(p.carets[2].min(rc.len()));
            let from = window(rc.len(), caret, fw);
            let to = (from + fw - usize::from(editing)).min(rc.len());
            // `$1`, `${name}`, `$$` in the accent.
            let mut roles = vec![Role::Text; rc.len()];
            let mut i = 0;
            while i < rc.len() {
                let end = match (rc[i], rc.get(i + 1)) {
                    ('$', Some('$')) => i + 2,
                    ('$', Some('{')) => rc[i..].iter().position(|&c| c == '}').map_or(rc.len(), |e| i + e + 1),
                    ('$', _) => i + 1 + rc[i + 1..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').count(),
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                for r in roles.iter_mut().take(end).skip(i) {
                    *r = Role::Accent;
                }
                i = end;
            }
            if rc.is_empty() && !editing {
                g.put(y, x, "(empty: the matches are taken out)", Role::Muted);
                out.hits.push((y, x..x + fw, Hit::Label(Field::Replacement)));
            } else {
                let xs = draw_row(g, y, x, &rc, &roles, from..to, caret);
                out.hits.push((y, x..x + fw, Hit::Field { field: Field::Replacement, start: from, xs }));
            }
            if editing {
                g.panel(y, x..x + fw);
            }
        }
    }
    if let Some(result) = &o.replaced {
        y += 1;
        g.put(y, left, "result", Role::Muted);
        let rows = field::rows(result, fw);
        let rc: Vec<char> = result.chars().collect();
        for (start, len) in rows.iter().take(MOST_ROWS) {
            draw_row(g, y, x, &rc, &[], *start..start + len, None);
            y += 1;
        }
        if rows.len() > MOST_ROWS {
            g.put(y, x, &format!("… {} more rows", rows.len() - MOST_ROWS), Role::Muted);
            y += 1;
        }
        y -= 1;
    }

    // The pattern, explained.
    if let Some(e) = &p.explained {
        y += 2;
        g.heading(y, left, width, "explained");
        let code_w = 24.min(width / 3);
        for piece in &e.pieces {
            y += 1;
            let indent = (piece.depth * 2).min(code_w / 2);
            let code = fit(&piece.code.replace('\n', " "), code_w - indent);
            let end = g.put(y, left + indent, &code, if piece.group.is_some() { Role::Text } else { Role::Accent });
            if let Some(n) = piece.group {
                out.backs.push((y, left + indent..end, Back::Group(n as usize)));
            }
            for (k, line) in wrap(&piece.what, width - code_w - 2).iter().enumerate() {
                if k > 0 {
                    y += 1;
                }
                g.put(y, left + code_w + 2, line, Role::Text);
            }
        }
    } else if !s.pattern.is_empty() && o.error.is_none() {
        y += 2;
        g.heading(y, left, width, "explained");
        y += 1;
        g.put(y, left, &fit("it can't be explained piece by piece: regex-syntax can't read part of it", width), Role::Muted);
    }

    // Where other flavours differ.
    if !p.notes.is_empty() {
        y += 2;
        g.heading(y, left, width, "elsewhere");
        for note in &p.notes {
            for (k, line) in wrap(note, width - 2).iter().enumerate() {
                y += 1;
                g.put(y, left + if k == 0 { 0 } else { 2 }, line, Role::Muted);
            }
        }
    }

    // Every match, its groups under it.
    if !o.matches.is_empty() {
        y += 2;
        let more = if o.more { format!("first {}", engine::MOST) } else { o.matches.len().to_string() };
        g.heading(y, left, width, &format!("matches · {more}"));
        let starts = field::line_starts(&s.text);
        let shown = &o.matches[..o.matches.len().min(MOST_LISTED)];
        let name_w = o.names.iter().flatten().map(|n| n.chars().count()).max().unwrap_or(0).min(16);
        for (k, m) in shown.iter().enumerate() {
            y += 1;
            let at = m.at();
            let (line, col) = field::line_col(&starts, at.start);
            let row = FIELDS.len() + k;
            let on = p.sel == row && p.editing.is_none();
            if on {
                g.focus(y, left..left + width);
            }
            let row_y = y;
            let mx = g.put(y, left, &format!("{:>4}", k + 1), if on { Role::Title } else { Role::Muted }) + 2;
            let mx = g.put(y, mx, &format!("{line}:{col}", line = line + 1, col = col + 1), Role::Muted) + 2;
            let mx = g.put(y, mx.max(left + 18), &format!("{}-{}", at.start, at.end), Role::Muted) + 2;
            g.put(y, mx.max(left + 30), &fit(&format!("{:?}", slice(&s.text, at)), (left + width).saturating_sub(mx.max(left + 30))), Role::Accent);
            for (n, group) in m.groups.iter().enumerate().skip(1) {
                y += 1;
                let name = o.names.get(n).cloned().flatten().unwrap_or_default();
                let gx = g.put(y, left + 6, &format!("{n:>2}"), Role::Text);
                out.backs.push((y, left + 6..gx, Back::Group(n)));
                let gx = gx + 1;
                g.put(y, gx, &fit(&name, name_w), Role::Text);
                let gx = gx + name_w + 2;
                match group {
                    Some(at) => {
                        let gx = g.put(y, gx.max(left + 18), &format!("{}-{}", at.start, at.end), Role::Muted) + 2;
                        g.put(y, gx.max(left + 30), &fit(&format!("{:?}", slice(&s.text, at.clone())), (left + width).saturating_sub(gx.max(left + 30))), Role::Text);
                    }
                    None => {
                        g.put(y, gx.max(left + 18), "didn't take part", Role::Muted);
                    }
                }
            }
            for line in row_y..=y {
                out.hits.push((line, left..left + width, Hit::Row(row)));
            }
        }
        if o.matches.len() > MOST_LISTED {
            y += 1;
            g.put(y, left, &format!("     … {} more (y copies them all)", o.matches.len() - MOST_LISTED), Role::Muted);
        }
    }

    let keys: &[(&str, &str)] = match p.editing {
        Some(Field::Text) => &[("Esc", "done"), ("Tab", "next field"), ("Enter", "new line")],
        Some(_) => &[("Esc", "done"), ("Tab", "next field"), ("Enter", "done")],
        None if p.sel >= FIELDS.len() => &[("Enter", "copy the match"), ("i m s x u", "flags"), ("f", "flavour"), ("y", "copy as code"), ("Tab", "cheatsheet")],
        None => &[("Enter", "edit"), ("c", "clear and edit"), ("i m s x u", "flags"), ("f", "flavour"), ("y", "copy as code"), ("w", "save"), ("o", "open saved"), ("Tab", "cheatsheet")],
    };
    g.keys(left, width, keys);
}

fn cheats(p: &Playground, g: &mut Grid, out: &mut Laid, left: usize, width: usize) {
    let (line, role) = p.filter.line("filter the cheatsheet", width);
    g.put(3, left, &line, role);
    let shown = p.cheats();
    let syntax_w = 20;
    let meaning_w = (width.saturating_sub(syntax_w + 2)) / 2;
    let mut y = 4;
    let mut section = "";
    for (k, (title, e)) in shown.iter().enumerate() {
        if *title != section {
            section = title;
            y += 1;
            g.heading(y, left, width, title);
        }
        y += 1;
        let on = k == p.cheat_sel;
        if on {
            g.focus(y, left..left + width);
        }
        out.hits.push((y, left..left + width, Hit::Cheat(k)));
        g.put(y, left + 1, &fit(e.syntax, syntax_w - 1), Role::Accent);
        g.put(y, left + syntax_w + 1, &fit(e.meaning, meaning_w), if on { Role::Title } else { Role::Text });
        let ex = left + syntax_w + 3 + meaning_w;
        let mut tags = String::new();
        if e.fancy {
            tags.push_str(" · fancy");
        }
        let flags = e.flags();
        if flags != Flags::default() {
            tags.push_str(&format!(" · {}", if flags.u { flags.letters().replace('u', "") } else { "u off".into() }));
        }
        let end = g.put(y, ex, &fit(e.pattern, (left + width).saturating_sub(ex + tags.len())), Role::Syntax("string".into()));
        g.put(y, end, &tags, Role::Muted);
    }
    if shown.is_empty() {
        g.put(5, left, "nothing matches the filter", Role::Muted);
    }
    let keys: &[(&str, &str)] = if p.filter.typing { &[("Enter", "keep it"), ("Esc", "clear it")] } else { &[("Enter", "try it in the playground"), ("/", "filter"), ("Tab", "playground")] };
    g.keys(left, width, keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(pattern: &str, text: &str) -> Playground {
        Playground::new(State { pattern: pattern.into(), text: text.into(), ..State::default() })
    }

    fn type_in(p: &mut Playground, s: &str) {
        for c in s.chars() {
            p.key(Key::Char(c));
        }
    }

    #[test]
    fn opened_again_it_takes_the_regex_or_the_selection() {
        use crate::seed::Found;
        let mut p = page(r"\d+", "a 1 b 22");
        p.state.replacement = Some("#".into());
        p.reseed(&Seed::Pattern(Found { pattern: "[a-z]".into(), flags: None }), Tab::Cheatsheet);
        assert_eq!((p.state.pattern.as_str(), p.state.text.as_str(), p.state.tab), ("[a-z]", "a 1 b 22", Tab::Cheatsheet));
        assert_eq!(p.outcome.matches.len(), 2, "matched again");
        p.reseed(&Seed::Selection("x y z".into()), Tab::Playground);
        assert_eq!((p.state.pattern.as_str(), p.state.text.as_str()), ("[a-z]", "x y z"));
        assert_eq!(p.outcome.matches.len(), 3);
        p.reseed(&Seed::Line("let x = 1;".into()), Tab::Playground);
        assert_eq!(p.state.text, "x y z", "a line with no regex leaves the text");
        assert_eq!(p.state.replacement.as_deref(), Some("#"), "the rest stays");
    }

    #[test]
    fn typing_a_pattern_matches_live() {
        let mut p = page("", "on 2026-10-29");
        p.key(Key::Enter);
        assert!(p.typing());
        type_in(&mut p, r"(\d+)-");
        assert_eq!(p.outcome.matches.len(), 2);
        let c = layout(&p, 100);
        assert!(c.text.contains("2 matches · 1 group"), "{}", c.text);
        assert!(c.panels.iter().any(|r| r.line == 3), "the pattern is a panel while typed");
        p.key(Key::Escape);
        assert!(!p.typing());
    }

    fn back_at(l: &Laid, line: usize, col: usize) -> Option<Back> {
        l.backs.iter().rev().find(|(y, xs, _)| *y == line && xs.contains(&col)).map(|b| b.2)
    }

    #[test]
    fn each_group_is_on_its_colour_in_the_text_and_the_pattern() {
        let p = page(r"(\w)(\d)x", "a1x b");
        let l = laid(&p, 100);
        let c = &l.canvas;
        let line = c.text.lines().position(|l| l.contains("a1x b")).unwrap();
        let col = c.text.lines().nth(line).unwrap().find("a1x").unwrap();
        assert_eq!(back_at(&l, line, col), Some(Back::Group(1)));
        assert_eq!(back_at(&l, line, col + 1), Some(Back::Group(2)));
        assert_eq!(back_at(&l, line, col + 2), Some(Back::Group(0)), "the match outside its groups");
        assert_eq!(back_at(&l, line, col + 3), None);
        let pat = c.text.lines().nth(3).unwrap().find("(\\w)").unwrap();
        assert_eq!(back_at(&l, 3, pat + 1), Some(Back::Group(1)), "{:?}", l.backs);
        assert_eq!(back_at(&l, 3, pat + 4), Some(Back::Group(2)));
    }

    #[test]
    fn an_empty_match_is_marked_where_it_is() {
        let mut p = page("$", "ab\ncd");
        p.state.flags.m = true;
        p.rerun();
        let l = laid(&p, 100);
        let line = l.canvas.text.lines().position(|t| t.ends_with("ab")).unwrap();
        let col = l.canvas.text.lines().nth(line).unwrap().find("ab").unwrap();
        assert_eq!(back_at(&l, line, col + 2), Some(Back::Empty), "after ab: {:?}", l.backs);
        assert_eq!(back_at(&l, line + 1, col + 2), Some(Back::Empty), "after cd");
        assert_eq!(back_at(&l, line, col), None);
    }

    #[test]
    fn runs_take_the_innermost_group() {
        let p = page(r"((a)b)c", "abc");
        assert_eq!(runs(&p.outcome.matches[0]), [(0..1, 2), (1..2, 1), (2..3, 0)]);
    }

    #[test]
    fn ctrl_z_undoes_a_field_a_typed_run_at_a_time() {
        let mut p = page("", "");
        p.key(Key::Enter);
        type_in(&mut p, "ab");
        p.key(Key::Left);
        type_in(&mut p, "x");
        assert_eq!(p.state.pattern, "axb");
        p.undo(false);
        assert_eq!(p.state.pattern, "ab");
        p.undo(false);
        assert_eq!(p.state.pattern, "");
        p.undo(false);
        assert_eq!(p.note.as_ref().map(|n| n.0.as_str()), Some("nothing to undo"));
        p.undo(true);
        p.undo(true);
        assert_eq!((p.state.pattern.as_str(), p.outcome.matches.len()), ("axb", 0));
        p.key(Key::Tab);
        p.paste("axb axb");
        assert_eq!(p.outcome.matches.len(), 2);
        p.undo(false);
        assert_eq!((p.state.text.as_str(), p.outcome.matches.len()), ("", 0), "matched again");
    }

    #[test]
    fn a_click_types_in_a_field_where_it_is_and_picks_a_match() {
        let mut p = page(r"\d+", "a 12 b 345");
        let l = laid(&p, 100);
        p.view(l, 0);
        let c = layout(&p, 100);
        let lines: Vec<&str> = c.text.lines().collect();
        let line = lines.iter().position(|t| t.contains("a 12 b 345")).unwrap();
        let col = lines[line].find("12").unwrap();
        assert_eq!(p.click(line, col + 1, 1), Action::None);
        assert!(p.typing());
        type_in(&mut p, "0");
        assert_eq!(p.state.text, "a 102 b 345", "typed where it was clicked");
        let l = laid(&p, 100);
        p.view(l, 0);
        let c = layout(&p, 100);
        let lines: Vec<&str> = c.text.lines().collect();
        let row = lines.iter().position(|t| t.contains("\"345\"")).unwrap();
        assert_eq!(p.click(row, 10, 1), Action::None);
        assert!(!p.typing());
        assert_eq!(p.click(row, 10, 2), Action::Copy("345".into(), "the match".into()));
        let flags = lines.iter().position(|t| t.starts_with("flags") || t.contains("ignore case")).unwrap();
        let i = lines[flags].find("i ignore").unwrap();
        p.click(flags, i + 3, 1);
        assert!(p.state.flags.i);
    }

    #[test]
    fn a_page_taller_than_its_pane_is_cut_and_scrolled() {
        let text: String = (0..40).map(|i| format!("line {i}\n")).collect();
        let mut p = page(r"\d+", &text);
        let l = laid(&p, 100);
        let whole = l.canvas.text.lines().count();
        let sixth = l.canvas.text.lines().nth(5).unwrap().to_string();
        let v = p.view(l, 20);
        assert_eq!((p.top, v.canvas.text.lines().count()), (0, 20));
        p.scroll(5);
        let v = p.view(laid(&p, 100), 20);
        assert_eq!(p.top, 5);
        assert_eq!(v.canvas.text.lines().next(), Some(sixth.as_str()));
        assert!(v.backs.iter().all(|b| b.0 < 20));
        p.scroll(1000);
        p.view(laid(&p, 100), 20);
        assert_eq!(p.top, whole - 20, "no further than its end");
        // A key brings the focus back into view.
        p.key(Key::Char('g'));
        let v = p.view(laid(&p, 100), 20);
        assert_eq!(p.top, 2);
        assert!(v.canvas.focus.is_some_and(|f| f.line < 20));
        p.page_key(true);
        p.view(laid(&p, 100), 20);
        assert_eq!(p.top, 18);
    }

    #[test]
    fn an_error_is_pointed_at() {
        let p = page("ab(c", "");
        let c = layout(&p, 100);
        let lines: Vec<&str> = c.text.lines().collect();
        let pat = lines[3].find("ab(c").unwrap();
        assert_eq!(lines[4].find('^'), Some(pat + 2), "{}", c.text);
        assert!(lines[4].contains("unclosed group"));
    }

    #[test]
    fn the_text_takes_lines_and_the_replacement_shows_its_result() {
        let mut p = page(r"(\w+)@(\w+)", "");
        p.key(Key::Char('j'));
        p.key(Key::Enter);
        type_in(&mut p, "me@home");
        p.key(Key::Enter);
        type_in(&mut p, "you@work");
        p.key(Key::Tab);
        type_in(&mut p, "$2:$1");
        p.key(Key::Escape);
        assert_eq!(p.outcome.matches.len(), 2);
        assert_eq!(p.outcome.replaced.as_deref(), Some("home:me\nwork:you"));
        let c = layout(&p, 100);
        assert!(c.text.contains("home:me") && c.text.contains("MATCHES · 2"), "{}", c.text);
        assert!(c.text.contains("2:1") && c.text.contains("\"you@work\""), "a match's line and column: {}", c.text);
    }

    #[test]
    fn flags_and_flavour_toggle() {
        let mut p = page("a(?=b)", "ab Ab");
        assert!(p.outcome.error.is_some());
        p.key(Key::Char('f'));
        assert_eq!(p.outcome.matches.len(), 1);
        p.key(Key::Char('i'));
        assert_eq!(p.outcome.matches.len(), 2);
        let c = layout(&p, 100);
        assert!(c.text.contains("EXPLAINED") && c.text.contains("followed by"), "{}", c.text);
        assert!(c.text.contains("ELSEWHERE"), "{}", c.text);
    }

    #[test]
    fn the_cheatsheet_loads_an_example() {
        let mut p = page("", "");
        p.key(Key::Tab);
        assert_eq!(p.state.tab, Tab::Cheatsheet);
        p.key(Key::Char('/'));
        type_in(&mut p, "lookbehind");
        p.key(Key::Enter);
        let c = layout(&p, 100);
        assert!(c.text.contains("(?<=") && c.text.contains("LOOKAROUND"), "{}", c.text);
        p.key(Key::Enter);
        assert_eq!(p.state.tab, Tab::Playground);
        assert_eq!(p.state.flavour, Flavour::Fancy);
        assert!(!p.outcome.matches.is_empty());
    }

    #[test]
    fn a_match_is_copied_and_the_rest_asked_of_the_extension() {
        let mut p = page(r"\d+", "a 12 b 345");
        p.key(Key::Char('G'));
        assert_eq!(p.key(Key::Enter), Action::Copy("345".into(), "the match".into()));
        assert_eq!(p.key(Key::Char('y')), Action::PickCopy);
        assert_eq!(p.key(Key::Char('w')), Action::Save { global: true }, "no project: yours");
        p.state.project = Some(("/w/x".into(), "x".into()));
        assert_eq!(p.key(Key::Char('w')), Action::Save { global: false });
        assert_eq!(p.matches_text(), "12\n345");
    }

    #[test]
    fn a_paste_into_the_pattern_is_one_line() {
        let mut p = page("", "");
        p.key(Key::Enter);
        p.paste("a\nb");
        assert_eq!(p.state.pattern, "ab");
        p.key(Key::Tab);
        p.paste("x\ny");
        assert_eq!(p.state.text, "x\ny");
    }

    #[test]
    fn the_session_keeps_it_and_a_long_text_is_cut() {
        let p = page("a", &"é".repeat(KEPT_TEXT));
        let kept = p.kept();
        assert!(kept.text.len() <= KEPT_TEXT && kept.pattern == "a");
        let back: State = serde_json::from_str(&serde_json::to_string(&kept).unwrap()).unwrap();
        assert_eq!(back.pattern, "a");
    }

    #[test]
    fn a_long_pattern_scrolls_with_its_caret() {
        assert_eq!(window(100, None, 20), 0);
        assert_eq!(window(100, Some(5), 20), 0);
        assert_eq!(window(100, Some(50), 20), 32);
        let mut p = page(&"a".repeat(200), "");
        p.key(Key::Enter);
        let c = layout(&p, 80);
        assert!(c.text.lines().nth(3).unwrap().ends_with("a▏"), "{}", c.text);
    }
}
