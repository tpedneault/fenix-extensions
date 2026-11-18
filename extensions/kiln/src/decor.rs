//! Titles after IDs: in a buffer of the project, after an item ID the
//! loaded project lists, its title and status, muted, so `[[REQ-014]]` or
//! `// implements: REQ-014` reads as `REQ-014 · Export as CSV · approved`
//! without hovering. `kiln lsp`'s hover still gives the rest (type,
//! parent, links, coverage); this is what hover can't: seeing without
//! asking. Kiln's language server gives no inlay hints (nor does Fenix's
//! client ask for them), so nothing is shown twice.
//!
//! **Which IDs.** A word is an ID only when `kiln list` has an item by
//! it, the rule [`crate::spot`] follows, with its words: so any prefix a
//! project configures works and `UTF-8` is none. Then, to keep what isn't
//! a mention out:
//!
//! - the whole word only: not inside `TASK-002-write-readings.md` or
//!   `X-REQ-014` (the same as Kiln's bare references);
//! - not in a path or a URL: not where the run of text around the word
//!   has a `/`, `\`, `@`, `?`, `&` or `%`, nor a file's extension after it
//!   (`REQ-014.md`), nor an anchor's `#` inside a longer run
//!   (`design.md#REQ-014`);
//! - not on an `id = "…"` line -- an item's own ID, its title two lines
//!   below -- nor on a line that already has that item's title (a heading,
//!   a generated document's row);
//! - once per line per ID.
//!
//! **Which files.** Every file of the project Fenix shows, not only item
//! files and `[code] paths`: an ID in a README, a commit message or a
//! script is as worth reading, the rules above keep accidents out, and
//! reading `[code]`'s globs would add a second, approximate copy of Kiln's
//! scanner for nothing a person would miss. "Of the project": its path is
//! under the project's folder (or `/project`), or ends with an item's
//! path ([`spot::relative`], [`spot::ends_with_path`]). A file over
//! [`MAX_TEXT`] isn't decorated.
//!
//! **Which lines.** Only those a pane shows ([`Decor::viewed`]), with
//! [`PAD`] lines either side so a short scroll doesn't show them bare for
//! the 100 ms before Fenix says the view moved; at most [`MOST`]
//! decorations a buffer, Fenix's own bound. Fenix gives a buffer's text
//! whole (`buffer::text`), so that is read once per view moved or change
//! (never per keystroke: Fenix tells a change 300 ms after typing stops),
//! and only the shown lines of it are looked at, each to [`MAX_LINE`]
//! characters. Items are indexed once per project read ([`Index`]).
//!
//! What it deliberately doesn't do: call the host (`lib.rs` hands it the
//! views, the texts and the project, and sets what it returns), change a
//! buffer, or mark an ID the project doesn't list -- that is `kiln check`'s
//! and the language server's (K102).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::api::{Decoration, Role, TextBuffer, TextView};
use crate::config::Config;
use crate::pages::view::Looks;
use crate::project::Loaded;
use crate::spot;

/// The setting that turns them off (on by default).
pub const SETTING: &str = "kiln.decorations";

/// The most characters of a title shown; past it, cut with `…`.
pub const TITLE_CHARS: usize = 40;

/// A buffer larger than this (in bytes) isn't decorated: the bound of
/// Fenix's own decoration rules, past any item or source file Kiln scans
/// (2 MiB).
pub const MAX_TEXT: usize = 4 * 1024 * 1024;

/// The characters of a line looked at: a minified line isn't read whole.
pub const MAX_LINE: usize = 4_000;

/// The most decorations given for a buffer: Fenix's bound.
pub const MOST: usize = 10_000;

/// Lines decorated above and below what a pane shows, at most (and at
/// most as many as it shows).
pub const PAD: u32 = 100;

/// Whether the setting's value says on: anything but `false`, so a
/// missing or garbled value keeps the default.
pub fn enabled(value: Option<&str>) -> bool {
    value.map(str::trim) != Some("false")
}

/// What is drawn after one item's ID.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    /// Its title, whole: a line that has it shows it already.
    title: String,
    /// ` · Title · `, the title cut to [`TITLE_CHARS`].
    lead: String,
    status: String,
    role: Role,
}

/// The loaded project's items by ID, with what's drawn for each, made once
/// per read so a line costs a lookup a word.
#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    items: HashMap<String, Entry>,
    /// Every item's path from the project's top, `/` between folders.
    paths: Vec<String>,
}

/// `text` without control characters, cut to `most` characters with `…`.
fn cut(text: &str, most: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() <= most {
        return clean.to_string();
    }
    let mut out: String = clean.chars().take(most.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

impl Index {
    pub fn new(loaded: &Loaded, looks: &Looks) -> Index {
        let mut items = HashMap::with_capacity(loaded.items.len());
        for item in &loaded.items {
            // A duplicated ID (K101): its first file's, as `Loaded::item`.
            items.entry(item.id.clone()).or_insert_with(|| {
                let meaning = looks.meaning(&item.kind, &item.status, item.done, item.closed);
                Entry {
                    title: item.title.trim().to_string(),
                    lead: format!(" · {} · ", cut(&item.title, TITLE_CHARS)),
                    status: cut(&item.status, TITLE_CHARS),
                    role: Role::from(&meaning.role()),
                }
            });
        }
        let paths = loaded
            .items
            .iter()
            .filter(|i| !i.path.is_empty())
            .map(|i| spot::slashes(&i.path))
            .collect();
        Index { items, paths }
    }

    /// Whether `path` (in full, as Fenix gives it) is a file of the
    /// project in `root`: under it or `/project`, or an item's file below
    /// another folder Fenix opened.
    pub fn in_project(&self, path: &str, root: Option<&str>) -> bool {
        if spot::relative(path, root).is_some() {
            return true;
        }
        let path = spot::slashes(path);
        self.paths.iter().any(|p| spot::ends_with_path(&path, p))
    }
}

/// Characters that end the run of text around a word: space, brackets,
/// quotes, and what separates a list.
fn ends_run(c: char) -> bool {
    c.is_whitespace() || "[](){}<>\"'`,;|*!".contains(c)
}

/// Whether the word at `start..end` of `chars` is part of a path or a URL
/// (the module header says what that is).
fn in_path(chars: &[char], start: usize, end: usize) -> bool {
    let mut a = start;
    while a > 0 && !ends_run(chars[a - 1]) {
        a -= 1;
    }
    let mut b = end;
    while b < chars.len() && !ends_run(chars[b]) {
        b += 1;
    }
    let around = chars[a..start].iter().chain(&chars[end..b]);
    if around
        .clone()
        .any(|c| matches!(c, '/' | '\\' | '@' | '?' | '&' | '%'))
    {
        return true;
    }
    let alnum = |at: usize| chars.get(at).is_some_and(|c| c.is_ascii_alphanumeric());
    // A file's extension, `REQ-014.md`; a dotted name, `x.REQ-014`.
    if chars.get(end) == Some(&'.') && alnum(end + 1) {
        return true;
    }
    if start >= 2 && chars[start - 1] == '.' && alnum(start - 2) {
        return true;
    }
    // An anchor in a link: `design.md#REQ-014`, not a lone `#REQ-014`.
    start > a + 1 && chars[start - 1] == '#'
}

/// The column the text goes after, for the ID at `start..end`: after a
/// `[[ID]]` or `[[ID|text]]` mention's `]]`, a quoted ID's closing quote,
/// a `[ID](…)` link's `)`, else the ID's last character.
fn place(chars: &[char], start: usize, end: usize) -> usize {
    let find = |from: usize, what: &[char]| {
        (from..chars.len().saturating_sub(what.len() - 1))
            .find(|&k| chars[k..k + what.len()] == *what)
    };
    if start >= 2 && chars[start - 2..start] == ['[', '['] {
        if let Some(k) = find(end, &[']', ']']) {
            return k + 1;
        }
    }
    if start >= 1
        && chars.get(end) == Some(&chars[start - 1])
        && matches!(chars[end], '"' | '\'' | '`')
    {
        return end;
    }
    if start >= 1
        && chars[start - 1] == '['
        && chars.get(end) == Some(&']')
        && chars.get(end + 1) == Some(&'(')
    {
        return find(end + 2, &[')']).unwrap_or(end);
    }
    end - 1
}

/// Whether `line` is front matter's `id = …`: an item's own ID.
fn is_id_line(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("id")
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// Where the IDs on `line` get their text: each column it goes after,
/// with the ID, left to right.
fn marks<'a>(index: &'a Index, line: &str) -> Vec<(usize, &'a str, &'a Entry)> {
    if is_id_line(line) {
        return Vec::new();
    }
    let chars: Vec<char> = line.chars().take(MAX_LINE).collect();
    let text: String;
    let line = if chars.len() < line.chars().count() {
        text = chars.iter().collect();
        text.as_str()
    } else {
        line
    };
    let mut out: Vec<(usize, &str, &Entry)> = Vec::new();
    for (start, end, word) in spot::words(line) {
        let Some((id, entry)) = index.items.get_key_value(word.as_str()) else {
            continue;
        };
        if out.iter().any(|(_, seen, _)| *seen == id)
            || in_path(&chars, start, end)
            || (!entry.title.is_empty() && line.contains(entry.title.as_str()))
        {
            continue;
        }
        out.push((place(&chars, start, end), id.as_str(), entry));
    }
    out
}

/// The decorations of line `n` (from 0), `line`'s text: for each mark, the
/// muted ` · Title · ` and the status in its meaning's colour, two texts
/// at one place, which Fenix draws in the order given.
pub fn line(index: &Index, n: u32, line: &str) -> Vec<Decoration> {
    let mut out = Vec::new();
    for (at, _, entry) in marks(index, line) {
        let at = (n, at as u32);
        out.push(Decoration::after(at, entry.lead.clone(), Role::Muted));
        out.push(Decoration::after(
            at,
            entry.status.clone(),
            entry.role.clone(),
        ));
    }
    out
}

/// `ranges` (first, last, both shown) widened by [`PAD`], sorted and
/// merged.
fn padded(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut wide: Vec<(u32, u32)> = ranges
        .iter()
        .map(|&(first, last)| {
            let (first, last) = (first.min(last), first.max(last));
            let pad = (last - first + 1).min(PAD);
            (first.saturating_sub(pad), last.saturating_add(pad))
        })
        .collect();
    wide.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (a, b) in wide {
        match out.last_mut() {
            Some((_, end)) if a <= end.saturating_add(1) => *end = (*end).max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// The decorations of `text`'s lines in `ranges`, at most [`MOST`].
pub fn text(index: &Index, text: &str, ranges: &[(u32, u32)]) -> Vec<Decoration> {
    let ranges = padded(ranges);
    let Some(&(_, end)) = ranges.last() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut range = ranges.iter().peekable();
    for (n, l) in text.lines().enumerate() {
        let n = n as u32;
        if n > end {
            break;
        }
        while range.peek().is_some_and(|&&(_, last)| last < n) {
            range.next();
        }
        if !range.peek().is_some_and(|&&(first, _)| first <= n) {
            continue;
        }
        let more = line(index, n, l);
        if out.len() + more.len() > MOST {
            break;
        }
        out.extend(more);
    }
    out
}

/// What the decorations are about now: the views, the project's index,
/// what was given each buffer, and what to do again.
#[derive(Debug, Default)]
pub struct Decor {
    /// The setting is off.
    off: bool,
    /// The lines the panes show, by file (full path): one range a pane.
    views: BTreeMap<String, Vec<(u32, u32)>>,
    index: Option<Index>,
    /// What the index was made from: the project's read
    /// (`Project::loads`) and `kiln.toml`'s.
    built: Option<(u64, u64)>,
    /// What each buffer was given last, and not taken away since.
    sent: HashMap<String, Vec<Decoration>>,
    /// Buffers to decorate again.
    dirty: HashSet<String>,
}

impl Decor {
    /// The setting turned on or off: off, every decoration goes at once
    /// and nothing more is worked out until it's on again.
    pub fn switch(&mut self, on: bool) {
        if self.off == !on {
            return;
        }
        self.off = !on;
        self.index = None;
        self.built = None;
        self.dirty.extend(self.views.keys().cloned());
    }

    pub fn on(&self) -> bool {
        !self.off
    }

    /// The project as it is: loaded and usable, with its read's count and
    /// `kiln.toml`'s (a reload that loads moves the first, so titles and
    /// statuses are read again; one that fails keeps the last), or not
    /// usable -- kiln gone, no `kiln.toml` -- when stale titles would
    /// mislead and an empty margin doesn't, so they go.
    pub fn project(&mut self, usable: Option<(&Loaded, u64, Option<&Config>)>, config: u64) {
        let key = usable.as_ref().map(|(_, loads, _)| (*loads, config));
        if self.off || key == self.built {
            return;
        }
        self.built = key;
        self.index =
            usable.map(|(loaded, _, config)| Index::new(loaded, &Looks::with(loaded, config)));
        self.dirty.extend(self.views.keys().cloned());
        self.dirty.extend(self.sent.keys().cloned());
    }

    /// The lines each pane shows now, every pane's (as Fenix tells them):
    /// a buffer whose lines moved is done again; one no pane shows any
    /// more loses its decorations, done again when it's back.
    pub fn viewed(&mut self, views: &[TextView]) {
        let mut now: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
        for v in views {
            if let TextBuffer::File(path) = &v.buffer {
                now.entry(path.clone()).or_default().push((v.first, v.last));
            }
        }
        for ranges in now.values_mut() {
            ranges.sort_unstable();
        }
        for (path, ranges) in &now {
            if self.views.get(path) != Some(ranges) {
                self.dirty.insert(path.clone());
            }
        }
        for path in self.views.keys() {
            if !now.contains_key(path) {
                self.dirty.insert(path.clone());
            }
        }
        self.views = now;
    }

    /// File `path`'s text changed: done again if a pane shows it, given
    /// again even if it comes out the same (Fenix moved what it had with
    /// the edits).
    pub fn changed(&mut self, path: &str) {
        if self.views.contains_key(path) {
            self.sent.remove(path);
            self.dirty.insert(path.to_string());
        }
    }

    /// Whether [`Decor::take`] has anything to do.
    pub fn pending(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// What to give each buffer that needs it, in path order: its
    /// decorations, or none to take them away. `root` is the project's
    /// folder as Fenix names it; `read` gives a buffer's text.
    pub fn take(
        &mut self,
        root: Option<&str>,
        mut read: impl FnMut(&str) -> Option<String>,
    ) -> Vec<(String, Vec<Decoration>)> {
        let mut dirty: Vec<String> = self.dirty.drain().collect();
        dirty.sort_unstable();
        let mut out = Vec::new();
        for path in dirty {
            let wanted = match (&self.index, self.views.get(&path)) {
                (Some(index), Some(ranges)) if !self.off && index.in_project(&path, root) => {
                    match read(&path) {
                        Some(t) if t.len() <= MAX_TEXT => text(index, &t, ranges),
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            };
            match self.sent.get(&path) {
                Some(was) if *was == wanted => continue,
                None if wanted.is_empty() => continue,
                _ => {}
            }
            if wanted.is_empty() {
                self.sent.remove(&path);
            } else {
                self.sent.insert(path.clone(), wanted.clone());
            }
            out.push((path, wanted));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::DecorationLook;
    use crate::model;
    use crate::pages::frame::tests::demo;

    fn index() -> Index {
        let d = demo();
        Index::new(&d, &Looks::new(&d))
    }

    /// One line's decorations as text: `col:ID status` -- the column the
    /// text goes after and what the status is drawn in -- or the whole of
    /// what's drawn with `full`.
    fn drawn(index: &Index, line: &str) -> Vec<String> {
        super::line(index, 0, line)
            .chunks(2)
            .map(|pair| {
                let text = |d: &Decoration| match &d.look {
                    DecorationLook::Text(t) => (t.text.clone(), t.role.clone(), t.after),
                    other => panic!("not text: {other:?}"),
                };
                let (lead, lead_role, a1) = text(&pair[0]);
                let (status, role, a2) = text(&pair[1]);
                assert!(a1 && a2, "drawn after");
                assert_eq!(lead_role, Role::Muted);
                assert_eq!(pair[0].start, pair[1].start, "one place");
                let role = format!("{role:?}");
                let role = role.trim_start_matches("Role::");
                format!("{}:{}{}[{role}]", pair[0].start.1, lead, status)
            })
            .collect()
    }

    #[test]
    fn a_table_of_lines_and_what_is_drawn_after_their_ids() {
        let i = index();
        let lead = |t: &str| format!(" · {t} · ");
        let req1 = lead("Readings are logged every second");
        let cases: Vec<(&str, Vec<String>)> = vec![
            // Front matter: parent, after the closing quote.
            (
                r#"parent = "REQ-001""#,
                vec![format!("17:{req1}approved[Syntax(\"keyword\")]")],
            ),
            // A [links] array: each ID after its quote.
            (
                r#"implements = ["REQ-001", "REQ-003"]"#,
                vec![
                    format!("22:{req1}approved[Syntax(\"keyword\")]"),
                    format!(
                        "33:{}approved[Syntax(\"keyword\")]",
                        lead("The log exports as CSV")
                    ),
                ],
            ),
            // A mention in prose: after the `]]`.
            (
                "Anything slower, see [[ADR-001]].",
                vec![format!(
                    "31:{}accepted[Syntax(\"keyword\")]",
                    lead("Store readings in SQLite")
                )],
            ),
            (
                "As [[TASK-001|the logger]] did.",
                vec![format!("25:{}done[Good]", lead("Set up the logger"))],
            ),
            // A code comment with several IDs; one blocked, one first.
            (
                "// verifies: TASK-005, TASK-004",
                vec![
                    format!("20:{}blocked[Bad]", lead("Ask about retention rules")),
                    format!("30:{}todo[Accent]", lead("Add the export button")),
                ],
            ),
            // In a file name, a URL, a longer word: nothing.
            (
                "see design/tasks/TASK-002-write-readings-to-sqlite.md",
                vec![],
            ),
            ("open TASK-002.md or REQ-001-x", vec![]),
            (
                "https://jira.example.com/browse/REQ-001 and x@REQ-001",
                vec![],
            ),
            ("design.md#REQ-001 and X-REQ-001 and REQ-0011", vec![]),
            // The item's own id line; a line with its title already.
            (r#"id = "REQ-001""#, vec![]),
            ("# REQ-001 Readings are logged every second", vec![]),
            // An ID the project doesn't list, and a lookalike.
            ("// implements: REQ-099, UTF-8", vec![]),
            // Once per line.
            (
                "REQ-003 and again REQ-003",
                vec![format!(
                    "6:{}approved[Syntax(\"keyword\")]",
                    lead("The log exports as CSV")
                )],
            ),
            // A lone anchor and a Markdown link's text.
            (
                "#RISK-001",
                vec![format!("8:{}open[Accent]", lead("The SD card wears out"))],
            ),
            (
                "[MS-001](../milestones/MS-001-first-release.md) next",
                vec![format!(
                    "46:{}active[Syntax(\"keyword\")]",
                    lead("First release")
                )],
            ),
        ];
        for (text, want) in cases {
            assert_eq!(drawn(&i, text), want, "{text}");
        }
    }

    #[test]
    fn columns_count_characters_past_wide_ones_and_tabs() {
        let i = index();
        // `参见` and an emoji before the ID, a tab before another.
        assert_eq!(
            drawn(&i, "参见REQ-002。🔥 \tTASK-001")
                .iter()
                .map(|d| d.split(':').next().unwrap().to_string())
                .collect::<Vec<_>>(),
            ["8", "20"]
        );
        let chars: Vec<char> = "参见REQ-002。🔥 \tTASK-001".chars().collect();
        assert_eq!(chars[8], '2');
        assert_eq!(chars[20], '1');
        assert_eq!(drawn(&i, "\t\tparent = \"MS-002\"").len(), 1);
    }

    #[test]
    fn a_long_title_is_cut_on_a_character_with_an_ellipsis() {
        assert_eq!(cut("short", 40), "short");
        let long = "Ünïcödé ".repeat(10);
        let c = cut(&long, 40);
        assert_eq!(c.chars().count(), 40);
        assert!(c.ends_with('…'));
        assert_eq!(cut("a\u{7}b\nc", 40), "abc");
        // Cut where a space was: no space before the ellipsis.
        assert_eq!(cut("abcd efgh", 6), "abcd…");
    }

    #[test]
    fn every_meaning_has_its_role() {
        let list = r#"[
          {"id":"A-001","type":"t","title":"one","status":"new","path":"a.md","line":2},
          {"id":"A-002","type":"t","title":"two","status":"going","path":"b.md","line":2},
          {"id":"A-003","type":"t","title":"three","status":"blocked","path":"c.md","line":2},
          {"id":"A-004","type":"t","title":"four","status":"shipped","done":true,"path":"d.md","line":2},
          {"id":"A-005","type":"t","title":"five","status":"dropped","closed":true,"path":"e.md","line":2}
        ]"#;
        let items = model::list(list).unwrap();
        let d = Loaded::new(demo().version, items, Err("-".into()), Ok(vec![]));
        let i = Index::new(&d, &Looks::new(&d));
        let roles: Vec<String> = drawn(&i, "A-001 A-002 A-003 A-004 A-005")
            .iter()
            .map(|d| {
                d.rsplit('[')
                    .next()
                    .unwrap()
                    .trim_end_matches(']')
                    .to_string()
            })
            .collect();
        assert_eq!(
            roles,
            ["Accent", "Syntax(\"keyword\")", "Bad", "Good", "Muted"]
        );
    }

    #[test]
    fn only_the_lines_in_view_and_a_margin_are_decorated() {
        let i = index();
        let text: String = (0..1000).map(|n| format!("{n} REQ-001\n")).collect();
        let lines = |ds: Vec<Decoration>| {
            let mut l: Vec<u32> = ds.iter().map(|d| d.start.0).collect();
            l.dedup();
            l
        };
        // Lines 500 to 509 shown: 10 more either side.
        let l = lines(super::text(&i, &text, &[(500, 509)]));
        assert_eq!((l[0], *l.last().unwrap(), l.len()), (490, 519, 30));
        // Two panes: both, merged where they meet.
        let l = lines(super::text(&i, &text, &[(0, 4), (8, 9)]));
        assert_eq!(l, (0..=11).collect::<Vec<_>>());
        // Past the end, nothing more; nothing shown, nothing.
        assert_eq!(lines(super::text(&i, &text, &[(995, 1200)])).len(), 5 + 100);
        assert!(super::text(&i, &text, &[]).is_empty());
    }

    #[test]
    fn a_buffer_gets_at_most_mosts_decorations() {
        let i = index();
        let text = "REQ-001 REQ-002 REQ-003 TASK-001 TASK-002\n".repeat(3000);
        assert_eq!(super::text(&i, &text, &[(0, 2999)]).len(), MOST);
    }

    #[test]
    fn the_setting_is_on_unless_it_says_false() {
        assert!(enabled(None));
        assert!(enabled(Some("true")));
        assert!(enabled(Some("yes?")));
        assert!(!enabled(Some(" false ")));
    }

    fn view(path: &str, pane: u64, first: u32, last: u32) -> TextView {
        TextView {
            buffer: TextBuffer::File(path.into()),
            pane,
            first,
            last,
        }
    }

    #[test]
    fn what_is_given_follows_the_views_the_project_and_the_setting() {
        let d = demo();
        let mut decor = Decor::default();
        let root = Some("/home/me/demo");
        let read = |_: &str| Some("// implements: REQ-001\n".to_string());
        decor.viewed(&[
            view("/home/me/demo/src/log.rs", 1, 0, 20),
            view("/elsewhere/notes.md", 2, 0, 20),
        ]);
        // No project yet: nothing to give.
        assert!(decor.take(root, read).is_empty());
        decor.project(Some((&d, 1, None)), 0);
        let given = decor.take(root, read);
        assert_eq!(given.len(), 1, "the file outside the project gets none");
        assert_eq!(given[0].0, "/home/me/demo/src/log.rs");
        assert_eq!(given[0].1.len(), 2);
        // The same read again, the same views: nothing.
        decor.project(Some((&d, 1, None)), 0);
        decor.viewed(&[
            view("/home/me/demo/src/log.rs", 1, 0, 20),
            view("/elsewhere/notes.md", 2, 0, 20),
        ]);
        assert!(decor.take(root, read).is_empty());
        // A change gives it again even when it's the same.
        decor.changed("/home/me/demo/src/log.rs");
        assert_eq!(decor.take(root, read).len(), 1);
        // Off: taken away, and nothing read.
        decor.switch(false);
        let given = decor.take(root, |_| panic!("read while off"));
        assert_eq!(given, [("/home/me/demo/src/log.rs".to_string(), vec![])]);
        decor.project(Some((&d, 2, None)), 0);
        decor.changed("/home/me/demo/src/log.rs");
        assert!(decor.take(root, |_| panic!("read while off")).is_empty());
        // On again: given again.
        decor.switch(true);
        decor.project(Some((&d, 2, None)), 0);
        assert_eq!(decor.take(root, read).len(), 1);
        // Out of view: taken away.
        decor.viewed(&[]);
        assert_eq!(
            decor.take(root, read),
            [("/home/me/demo/src/log.rs".to_string(), vec![])]
        );
        // The project lost: what's given goes.
        decor.viewed(&[view("/home/me/demo/src/log.rs", 1, 0, 20)]);
        assert_eq!(decor.take(root, read).len(), 1);
        decor.project(None, 0);
        assert_eq!(
            decor.take(root, read),
            [("/home/me/demo/src/log.rs".to_string(), vec![])]
        );
    }

    #[test]
    fn an_items_file_below_another_folder_and_a_large_file() {
        let d = demo();
        let mut decor = Decor::default();
        decor.project(Some((&d, 1, None)), 0);
        let item = "/work/repo/plan/design/tasks/TASK-004-add-the-export-button.md";
        let read = |p: &str| {
            Some(if p.ends_with("big.md") {
                "REQ-001\n".repeat(MAX_TEXT / 8 + 1)
            } else {
                "parent = \"MS-001\"\n".into()
            })
        };
        // Fenix's folder isn't the Kiln project's: the item's file is
        // known by its path's end, any other file isn't the project's.
        decor.viewed(&[
            view(item, 1, 0, 5),
            view("/work/repo/plan/notes.md", 2, 0, 5),
        ]);
        let given = decor.take(Some("/somewhere/else"), read);
        assert_eq!(given.len(), 1);
        assert_eq!(given[0].0, item);
        // In the project, but over the size: none.
        decor.viewed(&[view("/work/repo/big.md", 1, 0, 5)]);
        let given = decor.take(Some("/work/repo"), read);
        assert_eq!(
            given,
            [(item.to_string(), vec![])],
            "only the item's taken away"
        );
    }
}
