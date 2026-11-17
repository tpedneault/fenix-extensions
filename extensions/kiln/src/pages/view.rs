//! What a tab of the plan page is: the [`View`] each tab's module
//! implements, the [`Row`]s it gives, and what it builds them with
//! ([`Ctx`], [`Cell`], [`Look`], [`Query`]).
//!
//! A view is the part of a tab that knows Kiln: given the loaded project
//! (and its own state), its rows. Everything a tab does the same as every
//! other -- the cursor, scrolling, the filter, `Enter`, the tabs, the
//! states that aren't "loaded", the page's state -- is the frame's
//! ([`super::frame`]), written once, so a view stays small: the Tree
//! ([`super::tree`]) is the worked example.
//!
//! What it deliberately doesn't do: draw (a row says what its cells are
//! and what they mean, [`Look`]; the frame lays them out and colours them),
//! run kiln (a view says what it wants asked, [`View::queries`], and is
//! handed the answer), or write anything.

use std::collections::HashMap;

use fenix_ui_kit::{KeyPress, Role, Style};

use crate::kiln::{Failure, Ran};
use crate::model::Item;
use crate::project::Loaded;

/// What a status means, so it's coloured the same in every project: by
/// what its type says of it, not by its name (Kiln's GUI, "Looks").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meaning {
    /// One of its type's `done` statuses.
    Done,
    /// Past its first status, not done: approved, doing, active.
    InProgress,
    /// Its type's first status: a draft, a to-do.
    Initial,
    /// `blocked`.
    Blocked,
    /// One of its type's `closed` statuses: dropped, obsolete.
    Closed,
}

impl Meaning {
    /// Done green, blocked red, closed muted, initial in the accent (the
    /// headings' blue in Kiln's GUI), and what's between in the theme's
    /// `keyword` colour -- its purple in most themes, as Kiln's pills are.
    pub fn role(self) -> Role {
        match self {
            Meaning::Done => Role::Good,
            Meaning::InProgress => Role::Syntax("keyword".into()),
            Meaning::Initial => Role::Accent,
            Meaning::Blocked => Role::Bad,
            Meaning::Closed => Role::Muted,
        }
    }
}

/// How a cell is drawn: by what it is, the frame choosing the colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    Text,
    Muted,
    /// An item's ID.
    Id,
    /// A heading's name.
    Title,
    Accent,
    Good,
    Warn,
    Bad,
    Status(Meaning),
}

impl Look {
    pub fn style(&self) -> Style {
        Style::new(match self {
            Look::Text => Role::Text,
            Look::Muted => Role::Muted,
            Look::Id | Look::Accent => Role::Accent,
            Look::Title => Role::Title,
            Look::Good => Role::Good,
            Look::Warn => Role::Warn,
            Look::Bad => Role::Bad,
            Look::Status(m) => m.role(),
        })
    }
}

/// One column of a row: its text, one line, and how it's drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub look: Look,
}

impl Cell {
    pub fn new(text: impl Into<String>, look: Look) -> Cell {
        Cell {
            text: text.into(),
            look,
        }
    }

    pub fn text(text: impl Into<String>) -> Cell {
        Cell::new(text, Look::Text)
    }

    pub fn muted(text: impl Into<String>) -> Cell {
        Cell::new(text, Look::Muted)
    }

    pub fn id(text: impl Into<String>) -> Cell {
        Cell::new(text, Look::Id)
    }

    pub fn title(text: impl Into<String>) -> Cell {
        Cell::new(text, Look::Title)
    }
}

/// What a row is: the frame skips headings with the cursor and keeps one
/// while the filter lets a row under it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Groups the rows under it, up to the next heading. Its cells are
    /// drawn one after another, not in the columns.
    Heading,
    /// An item: the cursor's ID is its.
    Item,
    /// A place in code (`src/a.rs:4`).
    Code,
    /// A finding of `kiln check`'s.
    Finding,
    /// A line that says something: kiln's error, "reading…". The cursor
    /// passes over it; the filter treats it as any row.
    Note,
}

/// A file and a line, from 1, relative to the project's top: where
/// `Enter` goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: String,
    pub line: Option<u32>,
}

/// One line of a tab. Columns line up across a tab's rows (headings
/// aside); `depth` indents the first one, two cells a level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub depth: usize,
    pub cells: Vec<Cell>,
    /// The item it's about, for `v`, `i` and the edits to come.
    pub id: Option<String>,
    /// Where `Enter` opens.
    pub place: Option<Place>,
    /// What the cursor follows across reloads: the ID, else the place,
    /// else the text.
    key: String,
}

impl Row {
    fn new(kind: Kind, cells: Vec<Cell>) -> Row {
        let key = cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Row {
            kind,
            depth: 0,
            cells,
            id: None,
            place: None,
            key,
        }
    }

    /// A heading over the rows that follow it.
    pub fn heading(cells: Vec<Cell>) -> Row {
        Row::new(Kind::Heading, cells)
    }

    /// `item`'s row: its ID, and `Enter` at its file and line.
    pub fn item(item: &Item, cells: Vec<Cell>) -> Row {
        let mut row = Row::new(Kind::Item, cells);
        row.key = item.id.clone();
        row.id = Some(item.id.clone());
        row.place = (!item.path.is_empty()).then(|| Place {
            path: item.path.clone(),
            line: (item.line > 0).then_some(item.line),
        });
        row
    }

    /// A row about item `id` that `list` may not have (another answer
    /// names it: `show`, `impact`, `diff`): its place when it has, none
    /// when it hasn't ([`Ctx::row`] looks it up).
    pub fn about(id: &str, place: Option<Place>, cells: Vec<Cell>) -> Row {
        let mut row = Row::new(Kind::Item, cells);
        row.key = id.to_string();
        row.id = Some(id.to_string());
        row.place = place;
        row
    }

    /// A place in code: `Enter` opens it.
    pub fn code(path: &str, line: u32, cells: Vec<Cell>) -> Row {
        let mut row = Row::new(Kind::Code, cells);
        row.key = format!("{path}:{line}");
        row.place = Some(Place {
            path: path.to_string(),
            line: (line > 0).then_some(line),
        });
        row
    }

    /// A finding at `path`, `line` (0: none): `Enter` opens it there.
    pub fn finding(path: &str, line: u32, cells: Vec<Cell>) -> Row {
        let mut row = Row::code(path, line, cells);
        row.kind = Kind::Finding;
        row.key = format!("{path}:{line}:{}", row.key_text());
        row
    }

    /// A muted line that says something.
    pub fn note(text: impl Into<String>) -> Row {
        Row::new(Kind::Note, vec![Cell::muted(text)])
    }

    /// Indented `depth` levels.
    pub fn depth(mut self, depth: usize) -> Row {
        self.depth = depth;
        self
    }

    /// Followed by `key` across reloads instead.
    pub fn keyed(mut self, key: impl Into<String>) -> Row {
        self.key = key.into();
        self
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Whether the cursor stops on it: an item, a place in code, a
    /// finding -- not a heading or a note.
    pub fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Heading | Kind::Note)
    }

    /// Whether its cells line up in the tab's columns: as the rows the
    /// cursor stops on. A heading's and a note's run on, one after another.
    pub fn in_columns(&self) -> bool {
        self.selectable()
    }

    fn key_text(&self) -> String {
        self.cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Something a view wants kiln asked: `args` after `kiln` (`["impact",
/// "REQ-001", "--format", "json"]`), its answer handed back with `tag`
/// ([`View::answer`]). The frame asks it once for each read of the project
/// -- again after a reload -- and again when its `args` change; a view
/// that wants it no more leaves it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub tag: String,
    pub args: Vec<String>,
}

impl Query {
    pub fn new<S: Into<String>>(tag: &str, args: impl IntoIterator<Item = S>) -> Query {
        Query {
            tag: tag.to_string(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// What a view did with a key ([`View::key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Not its key.
    Ignored,
    /// Its state changed in a way its rows show: they're built again.
    Rebuild,
    /// Something to say on the bottom line.
    Say(String),
}

/// What a view builds its rows from: the project as kiln last read it,
/// and the frame's toggles for this tab.
pub struct Ctx<'a> {
    pub loaded: &'a Loaded,
    /// `a`: closed items are shown too.
    pub closed: bool,
    /// One of this tab's queries is out: its last answer is still what the
    /// view has.
    pub reading: bool,
    pub looks: &'a Looks,
}

impl Ctx<'_> {
    /// `item`'s status, coloured by what it means.
    pub fn status(&self, item: &Item) -> Cell {
        let m = self
            .looks
            .meaning(&item.kind, &item.status, item.done, item.closed);
        Cell::new(item.status.clone(), Look::Status(m))
    }

    /// A status another answer gives with only its ID (`status`, `show`):
    /// coloured as the item `list` has, else by its name alone.
    pub fn status_of(&self, id: &str, status: &str) -> Cell {
        match self.loaded.item(id) {
            Some(item) if item.status == status => self.status(item),
            Some(item) => Cell::new(
                status,
                Look::Status(self.looks.meaning(&item.kind, status, false, false)),
            ),
            None => Cell::new(
                status,
                Look::Status(self.looks.meaning("", status, false, false)),
            ),
        }
    }

    /// A row about item `id`, `Enter` at its file when `list` has it.
    pub fn row(&self, id: &str, cells: Vec<Cell>) -> Row {
        match self.loaded.item(id) {
            Some(item) => Row::item(item, cells),
            None => Row::about(id, None, cells),
        }
    }
}

/// Status names that are a first status when nothing better says so.
const INITIAL_NAMES: &[&str] = &[
    "draft", "todo", "to-do", "new", "open", "proposed", "planned", "idea", "backlog",
];

/// What each type's first status is, as far as kiln says: `status` lists
/// work that's ready, which is work at its first status, so a ready item's
/// status is its type's. Built once a read ([`Looks::new`]); a type with
/// no ready work falls back on the name (`draft`, `todo`...), since
/// `list` doesn't give a type's statuses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looks {
    initial: HashMap<String, String>,
}

impl Looks {
    pub fn new(loaded: &Loaded) -> Looks {
        let mut initial = HashMap::new();
        for group in loaded.groups() {
            for work in &group.ready {
                if let Some(item) = loaded.item(&work.id) {
                    initial
                        .entry(item.kind.clone())
                        .or_insert_with(|| work.status.clone());
                }
            }
        }
        Looks { initial }
    }

    /// What `status` means for an item of type `kind`.
    pub fn meaning(&self, kind: &str, status: &str, done: bool, closed: bool) -> Meaning {
        if closed {
            Meaning::Closed
        } else if done {
            Meaning::Done
        } else if status == "blocked" {
            Meaning::Blocked
        } else if match self.initial.get(kind) {
            Some(first) => first == status,
            None => INITIAL_NAMES.contains(&status),
        } {
            Meaning::Initial
        } else {
            Meaning::InProgress
        }
    }
}

/// kiln's answer as text when it exited with one of `fine`; else its
/// line (`kiln: no item REQ-099`), for a view's note.
pub fn stdout<'a>(result: &'a Result<Ran, Failure>, fine: &[i32]) -> Result<&'a str, String> {
    match result {
        Ok(ran) => ran.expect(fine).map_err(|f| f.line()),
        Err(f) => Err(f.line()),
    }
}

/// A tab of the plan page. One value per tab per page, kept while the page
/// is open; its own state (a toggle, the item it was handed) is its
/// fields. Every method but [`View::rows`] has a default.
///
/// The frame calls [`View::rows`] only when something it depends on
/// changed -- a new read, a key it said changed its rows, an answer, `a`
/// -- never per frame, so it may walk the whole project; it should stay
/// linear in the project's size (2,000 items is Kiln's budget).
pub trait View {
    /// Its rows, from `ctx` and its own state.
    fn rows(&self, ctx: &Ctx) -> Vec<Row>;

    /// The keys of its own for the bottom line, before the frame's:
    /// `("m", "only gaps")`.
    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }

    /// A key the frame doesn't take (`super::frame`'s list), with the row
    /// under the cursor.
    fn key(&mut self, _key: KeyPress, _row: Option<&Row>, _ctx: &Ctx) -> Reply {
        Reply::Ignored
    }

    /// Whether it hides closed items until `a` ([`Ctx::closed`]).
    fn hides_closed(&self) -> bool {
        false
    }

    /// The column that gives way when a row is wider than the page: the
    /// title, after the ID.
    fn flex(&self) -> usize {
        1
    }

    /// What it says when it has no rows.
    fn empty(&self) -> String {
        "nothing here".into()
    }

    /// What it wants kiln asked now ([`Query`]).
    fn queries(&self, _ctx: &Ctx) -> Vec<Query> {
        Vec::new()
    }

    /// The answer to its query `tag`, kiln's output or why there's none
    /// ([`stdout`]). Its rows are built again after.
    fn answer(&mut self, _tag: &str, _result: &Result<Ran, Failure>) {}

    /// An item handed to it from another tab: `v` for Item, `i` for
    /// Impact. The frame then shows this tab.
    fn hand(&mut self, _id: &str) {}

    /// Its own state for the page's, beside what the frame keeps (the
    /// cursor, the filter, `a`).
    fn save(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// That state again; one it can't read leaves it as it starts.
    fn restore(&mut self, _saved: &serde_json::Value) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture};

    fn demo() -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            model::check(&fixture("demo/check.json")),
        )
    }

    #[test]
    fn statuses_mean_what_their_type_says() {
        let d = demo();
        let looks = Looks::new(&d);
        let m = |id: &str| {
            let i = d.item(id).unwrap();
            looks.meaning(&i.kind, &i.status, i.done, i.closed)
        };
        assert_eq!(m("TASK-001"), Meaning::Done);
        assert_eq!(m("TASK-002"), Meaning::InProgress);
        assert_eq!(m("TASK-003"), Meaning::Initial, "ready work's status");
        assert_eq!(m("TASK-005"), Meaning::Blocked);
        assert_eq!(m("REQ-002"), Meaning::Initial, "draft, by its name");
        assert_eq!(m("REQ-001"), Meaning::InProgress);
        assert_eq!(
            looks.meaning("task", "dropped", false, true),
            Meaning::Closed
        );
    }

    #[test]
    fn rows_know_their_item_and_place() {
        let d = demo();
        let r = Row::item(d.item("TASK-002").unwrap(), vec![Cell::id("TASK-002")]);
        assert_eq!((r.key(), r.id.as_deref()), ("TASK-002", Some("TASK-002")));
        assert_eq!(
            r.place,
            Some(Place {
                path: "design/tasks/TASK-002-write-readings-to-sqlite.md".into(),
                line: Some(2)
            })
        );
        assert!(!Row::heading(vec![Cell::title("task")]).selectable());
        assert_eq!(Row::code("src/a.rs", 4, vec![]).key(), "src/a.rs:4");
    }
}
