//! The plan page's frame: what every tab does the same, written once.
//!
//! One page kind, `plan`, with a tab per [`Tab`], in Kiln's GUI's order.
//! The frame keeps, for each tab, its [`View`] and what it built from it:
//! the rows, the rows the filter lets through, the cursor (by the row's
//! key, so it stays on the same item across reloads, or where it was when
//! that item went away), the scroll, the filter and `a`. It draws the top
//! line (the tabs, the project's name, the check's errors), the rows
//! (columns aligned, statuses coloured by what they mean, only the rows in
//! view), and the bottom line (the filter, a message, kiln's last error,
//! "reading…", or the keys that work here); and every state that isn't
//! "loaded", in the words the project gives, for every tab.
//!
//! # Keys the frame owns
//!
//! `j` `k` `↓` `↑`, `g g` `G` `Home` `End`, `C-d` `C-u` `PgDn` `PgUp` (half
//! a page), `Tab` `]` `[` `S-Tab` `1`–`9`, `/` (then everything until
//! `Enter` or `Esc`), `Enter`, `r`, `a`, `v`, `i`. A view gets every other
//! key. `q` and `Esc` close the page and Space is the leader: Fenix's.
//!
//! # When rows are built
//!
//! A tab's rows are built when it's shown and something they depend on
//! changed: a new read of the project ([`Now::loads`]), `a`, an answer to
//! one of its queries, a key its view says changed them. Not per frame:
//! drawing walks only the rows in view, and the filter only the rows'
//! text, lowercased once as they're built.
//!
//! # Asking the user, and editing
//!
//! A view can't call the host, so it replies to a key with what it wants
//! asked ([`Reply::Ask`], [`Reply::Pick`]) or run ([`Reply::Edit`]); the
//! frame turns that into an [`Effect`] naming the tab and the view's tag,
//! `lib.rs` asks Fenix (its prompt, its picker) or kiln, and the answer
//! comes back through [`Plan::typed`], [`Plan::picked`] or
//! [`Plan::edited`] to that tab's view, whose reply is handled the same
//! way -- so a flow is a chain of them. An edit's outcome is said on the
//! bottom line before the view hears it.
//!
//! What it deliberately doesn't do: call Fenix (it returns [`Effect`]s and
//! [`Ask`]s for `lib.rs` to perform, so it is tested natively as it is),
//! run kiln, or know what any tab shows.

use std::collections::HashMap;

use fenix_ui_kit::text::{fit, width};
use fenix_ui_kit::{
    scrollbar, Canvas, Key, KeyPress, Mouse, Outcome, Rect, Role, Style, Tab as KitTab, TabsState,
    TextField,
};
use serde_json::{json, Value};

use super::view::{stdout, Choice, Ctx, Kind, Looks, Place, Reply, Row, View};
use super::Tab;
use crate::kiln::{Failure, Ran};
use crate::model::{count, MIN_VERSION};
use crate::project::{Loaded, QueryId, State};

/// The project now, as the frame needs it: its state, and how many reads
/// have loaded ([`crate::project::Project::loads`]), so a new read is
/// told from the one the rows were built from.
#[derive(Clone, Copy)]
pub struct Now<'a> {
    pub state: State<'a>,
    pub loads: u64,
}

/// Keys of Fenix's own that the frame names, looked up as the page is
/// drawn (`key_for`): `None` when nothing runs them.
#[derive(Debug, Clone, Default)]
pub struct Hints {
    /// `project.doctor`.
    pub doctor: Option<String>,
    /// `settings.open`.
    pub settings: Option<String>,
}

/// What a key, a click or an answer asks of Fenix or kiln. The last three
/// carry the tab and the view's tag, for the answer to come back to
/// ([`Plan::typed`], [`Plan::picked`], [`Plan::edited`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A file opened at a line (relative to the project's top).
    Open(Place),
    /// `r`: the project read again.
    Reload,
    /// Fenix's prompt: a line of text ([`Reply::Ask`]).
    Ask {
        tab: Tab,
        tag: String,
        label: String,
        initial: String,
    },
    /// Fenix's picker: one of `items` ([`Reply::Pick`]).
    Pick {
        tab: Tab,
        tag: String,
        title: String,
        items: Vec<Choice>,
    },
    /// An editing command for kiln ([`Reply::Edit`]).
    Edit {
        tab: Tab,
        tag: String,
        args: Vec<String>,
        stdin: Option<String>,
    },
}

/// A query to put to kiln for a tab ([`super::view::Query`]): `lib.rs`
/// asks it and gives back its id ([`Plan::asked`]), then its answer
/// ([`Plan::answered`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub tab: Tab,
    pub tag: String,
    pub args: Vec<String>,
}

/// The page's state as saved: its shape's version.
const SAVED: u64 = 1;

/// Cells between columns.
const GAP: usize = 2;

/// The narrowest the giving column gets before the others are cut.
const MIN_FLEX: usize = 8;

/// A tab: its view and what was built from it.
struct Pane {
    view: Box<dyn View>,
    rows: Vec<Row>,
    /// Each row's text, lowercased, for the filter.
    hay: Vec<String>,
    /// Each column's width over the rows (headings aside).
    widths: Vec<usize>,
    /// The rows the filter lets through, as indices into `rows`.
    shown: Vec<usize>,
    /// The cursor, an index into `shown`.
    cursor: usize,
    /// The key of the row the cursor is on, which it follows.
    at: Option<String>,
    /// The first row in view, an index into `shown`.
    top: usize,
    filter: String,
    /// `a`.
    closed: bool,
    /// The read its rows were built from; `None` before the first build.
    built: Option<u64>,
    dirty: bool,
    /// Each query asked, by tag: its args and the read it was for.
    asked: HashMap<String, (Vec<String>, u64)>,
    /// Each query out, by tag: its args and id.
    out: HashMap<String, (Vec<String>, QueryId)>,
}

impl Pane {
    fn new(view: Box<dyn View>) -> Pane {
        Pane {
            view,
            rows: Vec::new(),
            hay: Vec::new(),
            widths: Vec::new(),
            shown: Vec::new(),
            cursor: 0,
            at: None,
            top: 0,
            filter: String::new(),
            closed: false,
            built: None,
            dirty: true,
            asked: HashMap::new(),
            out: HashMap::new(),
        }
    }

    /// The row under the cursor.
    fn row(&self) -> Option<&Row> {
        self.shown
            .get(self.cursor)
            .map(|&i| &self.rows[i])
            .filter(|r| r.selectable())
    }

    fn build(&mut self, ctx: &Ctx) {
        self.rows = self.view.rows(ctx);
        self.hay = self
            .rows
            .iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            })
            .collect();
        let mut widths: Vec<usize> = Vec::new();
        for row in self.rows.iter().filter(|r| r.in_columns()) {
            for (c, cell) in row.cells.iter().enumerate() {
                let w = width(&cell.text) + if c == 0 { row.depth * 2 } else { 0 };
                if widths.len() <= c {
                    widths.resize(c + 1, 0);
                }
                widths[c] = widths[c].max(w);
            }
        }
        self.widths = widths;
        self.dirty = false;
        self.refilter();
    }

    /// The rows the filter lets through: those whose text has it, with
    /// the rows they're nested under (shallower ones before them since
    /// their heading) and their heading. Then the cursor put back.
    fn refilter(&mut self) {
        let needle = self.filter.trim().to_lowercase();
        if needle.is_empty() {
            self.shown = (0..self.rows.len()).collect();
        } else {
            let mut shown = Vec::new();
            let mut heading: Option<(usize, bool)> = None;
            // The rows this one may be nested under: (depth, index, shown).
            let mut above: Vec<(usize, usize, bool)> = Vec::new();
            for (i, row) in self.rows.iter().enumerate() {
                if row.kind == Kind::Heading {
                    heading = Some((i, false));
                    above.clear();
                    continue;
                }
                while above.last().is_some_and(|a| a.0 >= row.depth) {
                    above.pop();
                }
                let hit = self.hay[i].contains(&needle);
                if hit {
                    if let Some((h, false)) = heading {
                        shown.push(h);
                        heading = Some((h, true));
                    }
                    for a in above.iter_mut().filter(|a| !a.2) {
                        shown.push(a.1);
                        a.2 = true;
                    }
                    shown.push(i);
                }
                above.push((row.depth, i, hit));
            }
            self.shown = shown;
        }
        self.place();
    }

    /// The cursor on the row it follows, else where it was, on a row it
    /// stops at.
    fn place(&mut self) {
        if let Some(at) = &self.at {
            if let Some(i) = self
                .shown
                .iter()
                .position(|&r| self.rows[r].selectable() && self.rows[r].key() == at)
            {
                self.cursor = i;
                return;
            }
        }
        let last = self.shown.len().saturating_sub(1);
        self.cursor = self.cursor.min(last);
        if let Some(i) = self
            .stop(self.cursor, true)
            .or_else(|| self.stop(self.cursor, false))
        {
            self.cursor = i;
        }
        // Nothing to stop at (a filter that matches nothing): the row it
        // follows stays, for when there's something again.
        if let Some(row) = self.row() {
            self.at = Some(row.key().to_string());
        }
    }

    /// The first row from `from` the cursor stops at, forward or back.
    fn stop(&self, from: usize, forward: bool) -> Option<usize> {
        let ok = |i: usize| self.rows[self.shown[i]].selectable();
        if forward {
            (from..self.shown.len()).find(|&i| ok(i))
        } else {
            (0..=from.min(self.shown.len().saturating_sub(1)))
                .rev()
                .find(|&i| self.shown.get(i).is_some() && ok(i))
        }
    }

    /// The cursor moved to `to` (or the nearest row it stops at that way).
    fn go(&mut self, to: usize, forward: bool) -> bool {
        if self.shown.is_empty() {
            return false;
        }
        let to = to.min(self.shown.len() - 1);
        let Some(i) = self.stop(to, forward).or_else(|| self.stop(to, !forward)) else {
            return false;
        };
        let moved = i != self.cursor;
        self.cursor = i;
        self.at = self.row().map(|r| r.key().to_string());
        moved
    }

    fn down(&mut self, by: usize) -> bool {
        let to = self.cursor.saturating_add(by);
        if to >= self.shown.len() {
            return self.go(self.shown.len().saturating_sub(1), false);
        }
        self.go(to, true)
    }

    fn up(&mut self, by: usize) -> bool {
        match self.cursor.checked_sub(by) {
            Some(to) => self.go(to, false),
            None => self.go(0, true),
        }
    }
}

/// What `pane`'s view builds from and is asked with.
fn ctx<'a>(loaded: &'a Loaded, pane: &Pane, looks: &'a Looks) -> Ctx<'a> {
    Ctx {
        loaded,
        closed: pane.closed,
        reading: !pane.out.is_empty(),
        looks,
    }
}

/// An edit's outcome for the bottom line: kiln's stdout (`REQ-014: status
/// draft → approved`), its lines run together, or its error line.
fn outcome(result: &Result<Ran, Failure>) -> (String, Role) {
    match stdout(result, &[0]) {
        Ok(out) => {
            let lines: Vec<&str> = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                ("kiln did it, and said nothing".into(), Role::Text)
            } else {
                (lines.join("; "), Role::Text)
            }
        }
        Err(line) => (line, Role::Bad),
    }
}

/// One plan page: its tabs and what each keeps.
pub struct Plan {
    tabs: TabsState,
    panes: Vec<Pane>,
    looks: Looks,
    /// The read `looks` is from.
    looked: Option<u64>,
    /// The filter while it's typed.
    typing: Option<TextField>,
    /// `g` pressed, waiting for the second.
    g: bool,
    /// Something to say on the bottom line until the next key.
    message: Option<(String, Role)>,
    /// The rows' rectangle when last drawn, for the mouse and half pages.
    body: Rect,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan::new()
    }
}

impl Plan {
    pub fn new() -> Plan {
        Plan {
            tabs: TabsState::new(),
            panes: Tab::ALL.iter().map(|t| Pane::new(t.view())).collect(),
            looks: Looks::default(),
            looked: None,
            typing: None,
            g: false,
            message: None,
            body: Rect::default(),
        }
    }

    /// The tab shown.
    pub fn tab(&self) -> Tab {
        Tab::ALL[self.tabs.active.min(Tab::ALL.len() - 1)]
    }

    /// Tab `tab` shown.
    pub fn show(&mut self, tab: Tab) {
        self.tabs.active = tab.index();
    }

    /// The ID under the cursor on the tab shown, as last built.
    pub fn id(&self) -> Option<&str> {
        self.panes[self.tabs.active].row()?.id.as_deref()
    }

    /// Tab `tab`'s view replaced: a test's own.
    #[cfg(test)]
    pub(crate) fn set_view(&mut self, tab: Tab, view: Box<dyn View>) {
        self.panes[tab.index()] = Pane::new(view);
    }

    /// Whether the filter is being typed: the page takes every key.
    pub fn typing(&self) -> bool {
        self.typing.is_some()
    }

    // -- The page's state ------------------------------------------------------

    /// What the page needs to open as it was: the tab, and each tab's
    /// cursor, filter, `a` and its view's own.
    pub fn save(&self) -> String {
        let tabs: serde_json::Map<String, Value> = Tab::ALL
            .iter()
            .zip(&self.panes)
            .map(|(t, p)| {
                let mut v = json!({ "at": p.at, "filter": p.filter, "closed": p.closed });
                let own = p.view.save();
                if !own.is_null() {
                    v["view"] = own;
                }
                (t.name().to_string(), v)
            })
            .collect();
        json!({ "v": SAVED, "tab": self.tab().name(), "tabs": tabs }).to_string()
    }

    /// A page as [`Plan::save`] left it. A state it can't read -- another
    /// version's, not JSON, a tab it doesn't know -- leaves that part as
    /// a new page has it: restoring never fails.
    pub fn restore(saved: &str) -> Plan {
        let mut plan = Plan::new();
        let Ok(v) = serde_json::from_str::<Value>(saved) else {
            return plan;
        };
        if v["v"].as_u64() != Some(SAVED) {
            return plan;
        }
        if let Some(tab) = v["tab"].as_str().and_then(Tab::by_name) {
            plan.show(tab);
        }
        for (t, pane) in Tab::ALL.iter().zip(plan.panes.iter_mut()) {
            let s = &v["tabs"][t.name()];
            pane.at = s["at"].as_str().map(str::to_string);
            pane.filter = s["filter"].as_str().unwrap_or("").to_string();
            pane.closed = s["closed"].as_bool().unwrap_or(false);
            if !s["view"].is_null() {
                pane.view.restore(&s["view"]);
            }
        }
        plan
    }

    // -- Building --------------------------------------------------------------

    /// The tab shown built, if something it depends on changed.
    fn ensure(&mut self, now: Now) {
        let State::Loaded { loaded, .. } = now.state else {
            return;
        };
        if self.looked != Some(now.loads) {
            self.looks = Looks::new(loaded);
            self.looked = Some(now.loads);
        }
        let pane = &mut self.panes[self.tabs.active];
        if pane.built != Some(now.loads) || pane.dirty {
            let ctx = ctx(loaded, pane, &self.looks);
            pane.build(&ctx);
            pane.built = Some(now.loads);
        }
    }

    /// The queries the tab shown wants asked now: those not asked for this
    /// read with these args, and not out already. Only the tab shown asks,
    /// so a page costs kiln nothing for tabs no one looks at.
    pub fn wants(&mut self, now: Now) -> Vec<Ask> {
        let State::Loaded { loaded, .. } = now.state else {
            return Vec::new();
        };
        self.ensure(now);
        let tab = self.tab();
        let pane = &self.panes[self.tabs.active];
        let ctx = ctx(loaded, pane, &self.looks);
        pane.view
            .queries(&ctx)
            .into_iter()
            .filter(|q| {
                pane.asked.get(&q.tag) != Some(&(q.args.clone(), now.loads))
                    && pane.out.get(&q.tag).is_none_or(|(args, _)| *args != q.args)
            })
            .map(|q| Ask {
                tab,
                tag: q.tag,
                args: q.args,
            })
            .collect()
    }

    /// `ask` was put to kiln as query `id`.
    pub fn asked(&mut self, ask: &Ask, id: QueryId, now: Now) {
        let pane = &mut self.panes[ask.tab.index()];
        pane.asked
            .insert(ask.tag.clone(), (ask.args.clone(), now.loads));
        pane.out.insert(ask.tag.clone(), (ask.args.clone(), id));
        pane.dirty = true;
    }

    /// The answer to query `id`, asked for `tab`'s `tag`: handed to its
    /// view, unless another has been asked for that tag since (its view
    /// wants that one's). Whether it was taken.
    pub fn answered(
        &mut self,
        tab: Tab,
        tag: &str,
        id: QueryId,
        result: &Result<Ran, Failure>,
    ) -> bool {
        let pane = &mut self.panes[tab.index()];
        match pane.out.get(tag) {
            Some((_, out)) if *out == id => {
                pane.out.remove(tag);
                pane.view.answer(tag, result);
                pane.dirty = true;
                true
            }
            _ => false,
        }
    }

    // -- Keys and the mouse ----------------------------------------------------

    /// A key, with the keys held with it.
    pub fn key(&mut self, press: KeyPress, now: Now) -> Vec<Effect> {
        self.message = None;
        if let Some(field) = self.typing.as_mut() {
            let outcome = field.handle_press(press);
            let pane = &mut self.panes[self.tabs.active];
            match outcome {
                Outcome::Edited => {
                    pane.filter = field.text().to_string();
                    pane.refilter();
                }
                Outcome::Submit => self.typing = None,
                Outcome::Cancel => {
                    self.typing = None;
                    pane.filter.clear();
                    pane.refilter();
                }
                _ => {}
            }
            return Vec::new();
        }
        self.ensure(now);
        let g = std::mem::take(&mut self.g);
        let half = (self.body.h / 2).max(1);
        let ctrl = press.mods.ctrl && !press.mods.alt;
        let tab_key = match press.key {
            Key::Char(']') => Some(Key::Tab),
            Key::Char('[') => Some(Key::BackTab),
            k @ (Key::Tab | Key::BackTab) => Some(k),
            k @ Key::Char('1'..='9') if !ctrl => Some(k),
            _ => None,
        };
        if let Some(k) = tab_key {
            self.tabs.handle_key(k, Tab::ALL.len());
            return Vec::new();
        }
        let pane = &mut self.panes[self.tabs.active];
        match press.key {
            Key::Char('d') if ctrl => {
                pane.down(half);
            }
            Key::Char('u') if ctrl => {
                pane.up(half);
            }
            _ if press.mods.chord() => return self.pass_to_view(press, now),
            Key::PageDown => {
                pane.down(half);
            }
            Key::PageUp => {
                pane.up(half);
            }
            Key::Char('j') | Key::Down => {
                pane.down(1);
            }
            Key::Char('k') | Key::Up => {
                pane.up(1);
            }
            Key::Char('g') if g => {
                pane.go(0, true);
            }
            Key::Char('g') => self.g = true,
            Key::Home => {
                pane.go(0, true);
            }
            Key::Char('G') | Key::End => {
                pane.go(usize::MAX, false);
            }
            Key::Char('/') => {
                let mut field = TextField::new().with_text(&pane.filter);
                field.placeholder = "type to narrow the rows".into();
                self.typing = Some(field);
            }
            Key::Enter => {
                return match pane.row() {
                    Some(Row { place: Some(p), .. }) => vec![Effect::Open(p.clone())],
                    Some(_) => {
                        self.say("this row has no file to open", Role::Muted);
                        Vec::new()
                    }
                    None => Vec::new(),
                }
            }
            Key::Char('r') => return vec![Effect::Reload],
            Key::Char('a') => {
                if pane.view.hides_closed() {
                    pane.closed = !pane.closed;
                    pane.dirty = true;
                    let said = if pane.closed {
                        "closed items shown"
                    } else {
                        "closed items hidden"
                    };
                    self.say(said, Role::Muted);
                } else {
                    self.say(
                        &format!("{} hides no closed items", self.tab().title()),
                        Role::Muted,
                    );
                }
            }
            Key::Char(c @ ('v' | 'i')) => {
                let to = if c == 'v' { Tab::Item } else { Tab::Impact };
                match pane.row().and_then(|r| r.id.clone()) {
                    Some(id) => self.hand(to, &id),
                    None => self.say("no item here", Role::Muted),
                }
            }
            _ => return self.pass_to_view(press, now),
        }
        Vec::new()
    }

    /// Item `id` handed to tab `to`'s view, and that tab shown.
    pub fn hand(&mut self, to: Tab, id: &str) {
        let pane = &mut self.panes[to.index()];
        pane.view.hand(id);
        pane.dirty = true;
        self.show(to);
    }

    /// A key the frame doesn't take, for the view.
    fn pass_to_view(&mut self, press: KeyPress, now: Now) -> Vec<Effect> {
        let State::Loaded { loaded, .. } = now.state else {
            return Vec::new();
        };
        let tab = self.tab();
        let pane = &mut self.panes[tab.index()];
        let row = pane.row().cloned();
        let ctx = ctx(loaded, pane, &self.looks);
        let reply = pane.view.key(press, row.as_ref(), &ctx);
        self.reply(tab, reply)
    }

    /// What tab `tab`'s view replied, done: its rows marked to be built
    /// again, a line said, or an [`Effect`] for `lib.rs` to perform.
    fn reply(&mut self, tab: Tab, reply: Reply) -> Vec<Effect> {
        match reply {
            Reply::Ignored => Vec::new(),
            Reply::Rebuild => {
                self.panes[tab.index()].dirty = true;
                Vec::new()
            }
            Reply::Say(text) => {
                self.say(&text, Role::Text);
                Vec::new()
            }
            Reply::Ask {
                tag,
                label,
                initial,
            } => vec![Effect::Ask {
                tab,
                tag,
                label,
                initial,
            }],
            Reply::Pick { tag, title, items } => vec![Effect::Pick {
                tab,
                tag,
                title,
                items,
            }],
            Reply::Edit { tag, args, stdin } => vec![Effect::Edit {
                tab,
                tag,
                args,
                stdin,
            }],
        }
    }

    // -- Answers to what a view asked -------------------------------------------

    /// What was typed into the prompt tab `tab`'s view asked for with
    /// `tag`, handed to it; what it replies, done. With the project not
    /// read (kiln gone since it asked), there's nothing to act on: it's
    /// said, and dropped.
    pub fn typed(&mut self, tab: Tab, tag: &str, text: &str, now: Now) -> Vec<Effect> {
        self.answer_view(tab, now, |view, ctx| view.typed(tag, text, ctx))
    }

    /// The value picked in the picker tab `tab`'s view asked for with
    /// `tag`, handed to it, as [`Plan::typed`] does.
    pub fn picked(&mut self, tab: Tab, tag: &str, value: &str, now: Now) -> Vec<Effect> {
        self.answer_view(tab, now, |view, ctx| view.picked(tag, value, ctx))
    }

    /// How the edit tab `tab`'s view asked for with `tag` ended: said on
    /// the bottom line (kiln's stdout, or its error line), then handed to
    /// the view, whose reply may go on (another prompt after a refusal).
    pub fn edited(
        &mut self,
        tab: Tab,
        tag: &str,
        result: &Result<Ran, Failure>,
        now: Now,
    ) -> Vec<Effect> {
        let (line, role) = outcome(result);
        self.say(&line, role);
        if !matches!(now.state, State::Loaded { .. }) {
            return Vec::new();
        }
        // A word of the view's own replaces the outcome on the bottom line.
        self.answer_view(tab, now, |view, ctx| view.edited(tag, result, ctx))
    }

    fn answer_view(
        &mut self,
        tab: Tab,
        now: Now,
        hear: impl FnOnce(&mut dyn View, &Ctx) -> Reply,
    ) -> Vec<Effect> {
        let State::Loaded { loaded, .. } = now.state else {
            self.say(
                "the project isn't read now, so the answer was dropped",
                Role::Muted,
            );
            return Vec::new();
        };
        self.ensure(now);
        let pane = &mut self.panes[tab.index()];
        let ctx = ctx(loaded, pane, &self.looks);
        let reply = hear(pane.view.as_mut(), &ctx);
        // An answer may change what the view shows whatever it replies (a
        // step of a flow kept, then the next asked): built again, which an
        // answer, rare, can afford.
        pane.dirty = true;
        self.reply(tab, reply)
    }

    /// Text pasted: into the filter while it's typed.
    pub fn paste(&mut self, text: &str) {
        if let Some(field) = self.typing.as_mut() {
            if field.paste(text) == Outcome::Edited {
                let pane = &mut self.panes[self.tabs.active];
                pane.filter = field.text().to_string();
                pane.refilter();
            }
        }
    }

    /// The mouse, where the page was last drawn: a click on a tab shows
    /// it, on a row puts the cursor there (a double click opens it), the
    /// wheel moves the cursor three rows a notch.
    pub fn mouse(&mut self, m: &Mouse, now: Now) -> Vec<Effect> {
        if self.tabs.mouse(m).handled() {
            return Vec::new();
        }
        if !self.body.contains(m.x, m.y) {
            return Vec::new();
        }
        self.ensure(now);
        let pane = &mut self.panes[self.tabs.active];
        if let Some(lines) = m.wheel() {
            let by = lines.unsigned_abs() as usize * 3;
            if lines > 0 {
                pane.up(by);
            } else {
                pane.down(by);
            }
            return Vec::new();
        }
        if m.left_clicks() == 0 {
            return Vec::new();
        }
        let at = pane.top + (m.y - self.body.y);
        if at >= pane.shown.len() || !pane.rows[pane.shown[at]].selectable() {
            return Vec::new();
        }
        pane.go(at, true);
        if m.left_clicks() >= 2 {
            if let Some(p) = pane.row().and_then(|r| r.place.clone()) {
                return vec![Effect::Open(p)];
            }
        }
        Vec::new()
    }

    fn say(&mut self, text: &str, role: Role) {
        self.message = Some((text.to_string(), role));
    }

    // -- Drawing ---------------------------------------------------------------

    /// The page, `w` by `h` cells.
    pub fn render(&mut self, now: Now, hints: &Hints, w: usize, h: usize) -> Canvas {
        self.ensure(now);
        let mut c = Canvas::new();
        c.set_mouse(true);
        c.set_more_keys(true);
        let page = Rect::new(0, 0, w, h);
        let (head, rest) = page.split_top(2.min(h));
        let (foot, body) = rest.split_bottom(2.min(rest.h));
        self.body = body;
        self.draw_head(now, head, &mut c);
        match now.state {
            State::Loaded { .. } => self.draw_rows(body, &mut c),
            state => draw_state(state, hints, body, &mut c),
        }
        if foot.h > 0 {
            c.rule(foot.y, 0..w);
            self.draw_foot(now, foot.row(foot.h - 1), &mut c);
        }
        c
    }

    fn draw_head(&self, now: Now, rect: Rect, c: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        let mut right: Vec<(String, Style)> = Vec::new();
        let name = match now.state {
            State::Loaded {
                loaded, reloading, ..
            } => {
                if reloading {
                    right.push(("reading…  ".into(), Style::new(Role::Muted)));
                }
                loaded.name().unwrap_or("Kiln").to_string()
            }
            _ => "Kiln".to_string(),
        };
        right.push((name, Style::new(Role::Title)));
        if let State::Loaded { loaded, .. } = now.state {
            if let Some(e) = loaded.counts().map(|c| c.errors).filter(|&e| e > 0) {
                right.push((
                    format!("  {}", count(e as usize, "error")),
                    Style::new(Role::Bad),
                ));
            }
        }
        let rw: usize = right.iter().map(|(t, _)| width(t)).sum();
        let room = rect.w.saturating_sub(rw + 2);
        let tabs: Vec<KitTab> = Tab::ALL.iter().map(|t| KitTab::new(t.title())).collect();
        self.tabs
            .render(&tabs, Rect::new(rect.x, rect.y, room, 1), c);
        if rect.h > 1 {
            c.rule(rect.y + 1, rect.x..rect.right());
        }
        let mut x = rect
            .right()
            .saturating_sub(rw)
            .max(rect.x + room.min(rect.w));
        for (text, style) in right {
            x = c.put_fit(x, rect.y, rect.right().saturating_sub(x), &text, style);
        }
    }

    fn draw_rows(&mut self, body: Rect, c: &mut Canvas) {
        let pane = &mut self.panes[self.tabs.active];
        if body.is_empty() {
            return;
        }
        if pane.shown.is_empty() {
            let said = if pane.filter.trim().is_empty() {
                pane.view.empty()
            } else {
                format!(
                    "no row has “{}”: / then Esc clears the filter",
                    pane.filter.trim()
                )
            };
            c.put_fit(body.x, body.y, body.w, &said, Role::Muted);
            return;
        }
        let n = pane.shown.len();
        let bar = n > body.h && body.w > 2;
        // A cell between the rows and the scrollbar, so a column ending at
        // the edge doesn't run into it.
        let w = body.w - if bar { 2 } else { 0 };
        // The first row in view: as little moved as keeps the cursor in it.
        let mut top = pane.top.min(n.saturating_sub(body.h));
        if pane.cursor < top {
            top = pane.cursor;
        } else if pane.cursor >= top + body.h {
            top = pane.cursor + 1 - body.h;
        }
        pane.top = top;
        let widths = fitted(&pane.widths, pane.view.flex(), w);
        for (line, &r) in pane.shown[top..n.min(top + body.h)].iter().enumerate() {
            let y = body.y + line;
            let row = &pane.rows[r];
            if row.in_columns() {
                let mut x = body.x;
                for (i, cell) in row.cells.iter().enumerate() {
                    let indent = if i == 0 { row.depth * 2 } else { 0 };
                    let cw = widths.get(i).copied().unwrap_or(0);
                    let room = (body.x + w).saturating_sub(x + indent);
                    let last = i + 1 == row.cells.len();
                    let shown = if last {
                        room
                    } else {
                        cw.saturating_sub(indent).min(room)
                    };
                    if shown > 0 {
                        c.put(x + indent, y, &fit(&cell.text, shown), cell.look.style());
                    }
                    x += cw + GAP;
                    if x >= body.x + w {
                        break;
                    }
                }
            } else {
                let mut x = body.x;
                for cell in &row.cells {
                    if x >= body.x + w {
                        break;
                    }
                    x = c.put_fit(x, y, body.x + w - x, &cell.text, cell.look.style()) + GAP;
                }
            }
            if top + line == pane.cursor && row.selectable() {
                c.focus(y, body.x..body.x + w);
            }
        }
        if bar {
            scrollbar(c, body.right() - 1, body.y, body.h, n, top);
        }
    }

    fn draw_foot(&self, now: Now, rect: Rect, c: &mut Canvas) {
        let pane = &self.panes[self.tabs.active];
        if let Some(field) = &self.typing {
            let x = c.put(rect.x, rect.y, "/", Role::Accent);
            field.render(
                Rect::new(x, rect.y, rect.w.saturating_sub(x - rect.x), 1),
                c,
            );
            return;
        }
        let mut x = rect.x;
        let end = rect.right();
        if !pane.filter.trim().is_empty() {
            x = c.put_fit(
                x,
                rect.y,
                end - x,
                &format!("/{}", pane.filter.trim()),
                Role::Accent,
            ) + GAP;
        }
        let said = if let Some((text, role)) = &self.message {
            Some((text.clone(), role.clone()))
        } else if let State::Loaded { error: Some(e), .. } = now.state {
            Some((format!("the last read failed: {e}"), Role::Bad))
        } else if matches!(now.state, State::Loaded { .. }) && !pane.out.is_empty() {
            Some(("reading…".to_string(), Role::Muted))
        } else {
            None
        };
        if let Some((text, role)) = said {
            c.put_fit(x, rect.y, end.saturating_sub(x), &text, role);
            return;
        }
        let mut keys: Vec<(&str, &str)> = Vec::new();
        if let State::Loaded { .. } = now.state {
            keys.extend(pane.view.keys());
            keys.extend([("Enter", "open"), ("/", "filter")]);
            if pane.view.hides_closed() {
                keys.push((
                    "a",
                    if pane.closed {
                        "hide closed"
                    } else {
                        "show closed"
                    },
                ));
            }
            keys.extend([
                ("v", "item"),
                ("i", "impact"),
                ("r", "reload"),
                ("Tab", "next tab"),
            ]);
        } else {
            keys.extend([("r", "try again"), ("Tab", "next tab")]);
        }
        for (k, what) in keys {
            let need = width(k) + 1 + width(what);
            if x + need > end {
                break;
            }
            let after = c.put(x, rect.y, k, Role::Accent);
            x = c.put(after + 1, rect.y, what, Role::Muted) + GAP;
        }
    }
}

/// The columns' widths in `w` cells: as wide as what they hold, the
/// `flex` one giving way (down to [`MIN_FLEX`]) when they don't all fit;
/// what's still too wide is cut as it's drawn.
fn fitted(widths: &[usize], flex: usize, w: usize) -> Vec<usize> {
    let mut out = widths.to_vec();
    let total: usize = out.iter().sum::<usize>() + GAP * out.len().saturating_sub(1);
    if total > w {
        if let Some(f) = out.get_mut(flex) {
            *f = f.saturating_sub(total - w).max(MIN_FLEX.min(*f));
        }
    }
    out
}

/// A state that isn't "loaded", in the project's words, on every tab.
fn draw_state(state: State, hints: &Hints, body: Rect, c: &mut Canvas) {
    let mut lines: Vec<(String, Role)> = Vec::new();
    let settings = |what: &str| match &hints.settings {
        Some(k) => format!("{what} ({k} opens the settings)"),
        None => what.to_string(),
    };
    match state {
        State::Idle | State::Probing => lines.push(("looking for kiln…".into(), Role::Muted)),
        State::NotFound(why) => {
            lines.push((why.to_string(), Role::Bad));
            lines.push(("Kiln's releases: https://github.com/tpedneault/kiln/releases".into(), Role::Muted));
            lines.push((settings("kiln.path is where kiln is, when it isn't on the PATH"), Role::Muted));
        }
        State::NotKiln(why) => {
            lines.push((format!("kiln can't be used: {why}"), Role::Bad));
            lines.push((settings("set kiln.path to Kiln's kiln"), Role::Muted));
        }
        State::TooOld(v) => lines.push((
            format!("kiln {v} is older than {MIN_VERSION}, the oldest this extension reads: update Kiln"),
            Role::Bad,
        )),
        State::NoProject(line) => lines.push((line.to_string(), Role::Warn)),
        State::Loading => lines.push(("reading the project…".into(), Role::Muted)),
        State::Failed(line) => lines.push((line.to_string(), Role::Bad)),
        State::Loaded { .. } => {}
    }
    if matches!(
        state,
        State::NotFound(_) | State::NotKiln(_) | State::TooOld(_)
    ) {
        if let Some(k) = &hints.doctor {
            lines.push((
                format!("{k}: the project doctor says what it found"),
                Role::Muted,
            ));
        }
    }
    for (i, (text, role)) in lines.into_iter().enumerate().take(body.h) {
        c.put_fit(body.x, body.y + i, body.w, &text, role);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{self, tests::fixture, Item, Version};
    use crate::pages::view::{Cell, Choice, Query};
    use crate::project::Loaded;
    use fenix_ui_kit::Mods;

    fn version() -> Version {
        Version::parse("kiln 1.0.0").unwrap()
    }

    pub(crate) fn demo() -> Loaded {
        Loaded::new(
            version(),
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            model::check(&fixture("demo/check.json")),
        )
    }

    fn loaded(l: &Loaded, loads: u64) -> Now<'_> {
        Now {
            state: State::Loaded {
                loaded: l,
                reloading: false,
                error: None,
            },
            loads,
        }
    }

    fn key(p: &mut Plan, k: Key, now: Now) -> Vec<Effect> {
        p.key(KeyPress::from(k), now)
    }

    fn keys(p: &mut Plan, s: &str, now: Now) {
        for k in fenix_test_kit::parse_keys(s) {
            p.key(KeyPress::from(fenix_ui_kit::Key::from(k)), now);
        }
    }

    fn screen(p: &mut Plan, now: Now, w: usize, h: usize) -> String {
        fenix_test_kit::render::canvas(&p.render(now, &Hints::default(), w, h))
    }

    #[test]
    fn tabs_go_round_by_tab_brackets_and_numbers() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        assert_eq!(p.tab(), Tab::Tree);
        key(&mut p, Key::Tab, now);
        assert_eq!(p.tab(), Tab::Trace);
        keys(&mut p, "]]", now);
        assert_eq!(p.tab(), Tab::Board);
        keys(&mut p, "[", now);
        assert_eq!(p.tab(), Tab::Impact);
        keys(&mut p, "9", now);
        assert_eq!(p.tab(), Tab::Check);
        key(&mut p, Key::Tab, now);
        assert_eq!(p.tab(), Tab::Tree, "round to the first");
        key(&mut p, Key::BackTab, now);
        assert_eq!(p.tab(), Tab::Check);
    }

    #[test]
    fn the_cursor_skips_headings_and_moves_by_halves() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        screen(&mut p, now, 80, 14);
        assert_eq!(p.id(), Some("REQ-001"), "the first item, under its heading");
        keys(&mut p, "jj", now);
        assert_eq!(p.id(), Some("REQ-003"));
        keys(&mut p, "j", now);
        assert_eq!(p.id(), Some("ADR-001"), "over the decision's heading");
        keys(&mut p, "k", now);
        assert_eq!(p.id(), Some("REQ-003"));
        keys(&mut p, "G", now);
        assert_eq!(p.id(), Some("RISK-001"));
        keys(&mut p, "gg", now);
        assert_eq!(p.id(), Some("REQ-001"));
        let ctrl = |c| KeyPress::new(Key::Char(c), Mods::CTRL);
        p.key(ctrl('d'), now);
        // The body is 10 rows: half is 5.
        assert_eq!(p.id(), Some("MS-001"));
        p.key(ctrl('u'), now);
        assert_eq!(
            p.id(),
            Some("REQ-002"),
            "5 rows up, where the heading isn't"
        );
    }

    #[test]
    fn the_filter_narrows_keeping_headings_and_parents() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        keys(&mut p, "/batch", now);
        assert!(p.typing());
        let s = screen(&mut p, now, 80, 10);
        assert!(s.contains("task  "), "the heading of the match: {s}");
        assert!(s.contains("TASK-002"), "its parent: {s}");
        assert!(s.contains("  TASK-007"), "the match, nested: {s}");
        assert!(!s.contains("REQ-001"), "{s}");
        assert!(s.ends_with("/batch▏\n") || s.contains("\n/batch"), "{s}");
        keys(&mut p, "<Enter>", now);
        assert!(!p.typing());
        assert_eq!(p.id(), Some("TASK-002"));
        // Kept: shown before the keys.
        assert!(screen(&mut p, now, 80, 10)
            .lines()
            .last()
            .unwrap()
            .starts_with("/batch  "));
        keys(&mut p, "/<Esc>", now);
        assert!(screen(&mut p, now, 80, 30).contains("REQ-001"));
        assert_eq!(p.id(), Some("TASK-002"), "cleared, still on its row");
        keys(&mut p, "/zzz", now);
        assert!(screen(&mut p, now, 80, 10).contains("no row has “zzz”"));
        assert_eq!(p.id(), None);
        keys(&mut p, "<Esc>", now);
        assert_eq!(p.id(), Some("TASK-002"), "back on it once there are rows");
    }

    #[test]
    fn the_cursor_follows_its_id_across_a_reload() {
        let d = demo();
        let mut p = Plan::new();
        keys(&mut p, "G", loaded(&d, 1));
        keys(&mut p, "k", loaded(&d, 1));
        assert_eq!(p.id(), Some("TEST-001"));
        // A read with an item before it gone: still on TEST-001.
        let mut items = d.items.clone();
        items.retain(|i| i.id != "REQ-003");
        let fewer = Loaded::new(version(), items, d.plan.clone(), d.findings.clone());
        screen(&mut p, loaded(&fewer, 2), 80, 30);
        assert_eq!(p.id(), Some("TEST-001"));
        // TEST-001 gone too: on the row where it was.
        let mut items = fewer.items.clone();
        items.retain(|i| i.id != "TEST-001");
        let gone = Loaded::new(version(), items, d.plan.clone(), d.findings.clone());
        screen(&mut p, loaded(&gone, 3), 80, 30);
        assert_eq!(p.id(), Some("RISK-001"));
    }

    #[test]
    fn enter_opens_the_item_at_its_line_and_a_and_v_i_hand_on() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        assert_eq!(
            key(&mut p, Key::Enter, now),
            [Effect::Open(Place {
                path: "design/requirements/REQ-001-readings-are-logged-every-second.md".into(),
                line: Some(2)
            })]
        );
        assert_eq!(key(&mut p, Key::Char('r'), now), [Effect::Reload]);
        key(&mut p, Key::Char('v'), now);
        assert_eq!(p.tab(), Tab::Item);
        keys(&mut p, "1i", now);
        assert_eq!(p.tab(), Tab::Impact);
        keys(&mut p, "3a", now);
        assert!(screen(&mut p, now, 80, 10).contains("Impact hides no closed items"));
    }

    #[test]
    fn the_state_round_trips_and_a_bad_one_is_the_default() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        keys(&mut p, "jja/req<Enter>", now);
        keys(&mut p, "4", now);
        // The Board drawn, so its cursor is on its first row, as the
        // restored page's will be once it's drawn.
        screen(&mut p, now, 80, 30);
        assert_eq!(p.id(), Some("TASK-003"));
        let saved = p.save();
        let mut q = Plan::restore(&saved);
        assert_eq!(q.tab(), Tab::Board);
        keys(&mut q, "1", now);
        screen(&mut q, now, 80, 30);
        assert_eq!(q.id(), p.panes[0].at.as_deref());
        assert_eq!(q.panes[0].filter, "req");
        assert!(q.panes[0].closed);
        assert_eq!(q.save(), {
            let mut p = p;
            p.show(Tab::Tree);
            p.save()
        });
        for bad in [
            "",
            "nope",
            "{\"v\": 2, \"tab\": \"board\"}",
            "{\"v\":1,\"tab\":7,\"tabs\":{\"tree\":{\"at\":3,\"closed\":\"x\"}}}",
            "[]",
        ] {
            let r = Plan::restore(bad);
            assert_eq!(r.tab(), Tab::Tree, "{bad}");
            assert_eq!(r.panes[0].at, None, "{bad}");
        }
    }

    #[test]
    fn columns_give_way_in_the_title() {
        assert_eq!(fitted(&[8, 40, 9], 1, 80), [8, 40, 9]);
        assert_eq!(fitted(&[8, 40, 9], 1, 40), [8, 19, 9]);
        assert_eq!(fitted(&[8, 40, 9], 1, 20), [8, 8, 9], "no narrower than 8");
    }

    /// A view that asks kiln something, for the frame's part in it.
    #[derive(Default)]
    pub(crate) struct Asking {
        about: Option<String>,
        said: Option<String>,
    }

    impl View for Asking {
        fn rows(&self, ctx: &Ctx) -> Vec<Row> {
            let note = match (&self.said, ctx.reading) {
                (Some(s), _) => s.clone(),
                (None, true) => "reading…".into(),
                (None, false) => "nothing".into(),
            };
            vec![Row::note(note), Row::heading(vec![Cell::title("x")])]
        }
        fn queries(&self, _: &Ctx) -> Vec<Query> {
            self.about
                .iter()
                .map(|id| Query::new("show", ["show", id.as_str()]))
                .collect()
        }
        fn answer(&mut self, _: &str, result: &Result<Ran, Failure>) {
            self.said = Some(
                crate::pages::view::stdout(result, &[0])
                    .map(str::to_string)
                    .unwrap_or_else(|e| e),
            );
        }
        fn hand(&mut self, id: &str) {
            self.about = Some(id.to_string());
        }
    }

    #[test]
    fn a_query_is_asked_once_a_read_and_its_answer_handed_back() {
        let d = demo();
        let mut p = Plan::new();
        p.set_view(Tab::Item, Box::<Asking>::default());
        let now = loaded(&d, 1);
        assert!(p.wants(now).is_empty(), "the Tree asks nothing");
        p.hand(Tab::Item, "REQ-001");
        let asks = p.wants(now);
        assert_eq!(
            asks,
            [Ask {
                tab: Tab::Item,
                tag: "show".into(),
                args: vec!["show".into(), "REQ-001".into()]
            }]
        );
        p.asked(&asks[0], 7, now);
        assert!(p.wants(now).is_empty(), "out already");
        let s = screen(&mut p, now, 60, 8);
        assert!(s.contains("\nreading…") && s.ends_with("reading…\n"), "{s}");
        let ran = Ran {
            code: Some(0),
            stdout: "shown\n".into(),
            stderr: vec![],
        };
        assert!(
            !p.answered(Tab::Item, "show", 6, &Ok(ran.clone())),
            "an older one's"
        );
        assert!(p.answered(Tab::Item, "show", 7, &Ok(ran)));
        assert!(screen(&mut p, now, 60, 8).contains("shown"));
        assert!(p.wants(now).is_empty(), "once a read");
        assert_eq!(p.wants(loaded(&d, 2)).len(), 1, "again after a reload");
    }

    /// A view whose `x` is an edit flow, as E2's will be: pick a role,
    /// type a target, then `kiln link`; asked again when kiln refuses.
    #[derive(Default)]
    pub(crate) struct Flow {
        role: Option<String>,
        pub(crate) heard: Vec<String>,
    }

    impl View for Flow {
        fn rows(&self, _: &Ctx) -> Vec<Row> {
            vec![Row::note(format!("heard: {}", self.heard.join(", ")))]
        }
        fn key(&mut self, key: KeyPress, _: Option<&Row>, _: &Ctx) -> Reply {
            if key.key != Key::Char('x') {
                return Reply::Ignored;
            }
            Reply::pick(
                "role",
                "Link REQ-001 as",
                vec![
                    Choice::new("verifies", "verifies").detail("a test"),
                    Choice::new("implements", "implements"),
                ],
            )
        }
        fn picked(&mut self, tag: &str, value: &str, _: &Ctx) -> Reply {
            self.heard.push(format!("{tag}={value}"));
            self.role = Some(value.to_string());
            Reply::ask("target", format!("REQ-001 {value} what"), "")
        }
        fn typed(&mut self, tag: &str, text: &str, _: &Ctx) -> Reply {
            self.heard.push(format!("{tag}={text}"));
            let role = self.role.clone().unwrap_or_default();
            Reply::edit("link", ["link", "REQ-001", role.as_str(), text])
        }
        fn edited(&mut self, tag: &str, result: &Result<Ran, Failure>, _: &Ctx) -> Reply {
            match crate::pages::view::stdout(result, &[0]) {
                Ok(_) => {
                    self.heard.push(format!("{tag} done"));
                    Reply::Rebuild
                }
                // Refused: the flow stays open, asked again.
                Err(_) => {
                    self.heard.push(format!("{tag} refused"));
                    Reply::ask("target", "REQ-001 again", "")
                }
            }
        }
    }

    fn foot_line(p: &mut Plan, now: Now) -> String {
        screen(p, now, 80, 8)
            .lines()
            .last()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn a_views_flow_of_picks_and_prompts_ends_in_one_edit() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        p.set_view(Tab::Check, Box::<Flow>::default());
        keys(&mut p, "9", now);
        let pick = key(&mut p, Key::Char('x'), now);
        assert_eq!(
            pick,
            [Effect::Pick {
                tab: Tab::Check,
                tag: "role".into(),
                title: "Link REQ-001 as".into(),
                items: vec![
                    Choice::new("verifies", "verifies").detail("a test"),
                    Choice::new("implements", "implements")
                ],
            }]
        );
        // The answer goes to the tab that asked, even with another shown.
        keys(&mut p, "1", now);
        let ask = p.picked(Tab::Check, "role", "verifies", now);
        assert_eq!(
            ask,
            [Effect::Ask {
                tab: Tab::Check,
                tag: "target".into(),
                label: "REQ-001 verifies what".into(),
                initial: String::new(),
            }]
        );
        let edit = p.typed(Tab::Check, "target", "TEST-001", now);
        assert_eq!(
            edit,
            [Effect::Edit {
                tab: Tab::Check,
                tag: "link".into(),
                args: vec![
                    "link".into(),
                    "REQ-001".into(),
                    "verifies".into(),
                    "TEST-001".into()
                ],
                stdin: None,
            }]
        );
        // Done: kiln's stdout on the bottom line, and the view told.
        let ran = Ran {
            code: Some(0),
            stdout: "REQ-001: + verifies TEST-001\n".into(),
            stderr: vec![],
        };
        assert!(p.edited(Tab::Check, "link", &Ok(ran), now).is_empty());
        assert_eq!(foot_line(&mut p, now), "REQ-001: + verifies TEST-001");
        keys(&mut p, "9", now);
        assert!(
            screen(&mut p, now, 80, 8).contains("heard: role=verifies, target=TEST-001, link done")
        );
    }

    #[test]
    fn a_refused_edit_says_kilns_line_and_the_view_may_ask_again() {
        let d = demo();
        let now = loaded(&d, 1);
        let mut p = Plan::new();
        p.set_view(Tab::Check, Box::<Flow>::default());
        let refused = Err(Failure::Exit {
            code: Some(1),
            line: "kiln: design/requirements/REQ-001.md changed on disk since it was read".into(),
        });
        assert_eq!(
            p.edited(Tab::Check, "link", &refused, now),
            [Effect::Ask {
                tab: Tab::Check,
                tag: "target".into(),
                label: "REQ-001 again".into(),
                initial: String::new(),
            }]
        );
        assert_eq!(
            foot_line(&mut p, now),
            "kiln: design/requirements/REQ-001.md changed on disk since it was read"
        );
        // An edit that printed nothing says so rather than nothing.
        let quiet = Ran {
            code: Some(0),
            stdout: "\n".into(),
            stderr: vec![],
        };
        p.edited(Tab::Check, "link", &Ok(quiet), now);
        assert_eq!(foot_line(&mut p, now), "kiln did it, and said nothing");
    }

    #[test]
    fn an_answer_while_the_project_isnt_read_is_dropped_and_said() {
        let d = demo();
        let mut p = Plan::new();
        p.set_view(Tab::Check, Box::<Flow>::default());
        let gone = Now {
            state: State::Failed("kiln: kiln.toml:4: unknown key `nmae`"),
            loads: 1,
        };
        assert!(p.typed(Tab::Check, "target", "TEST-001", gone).is_empty());
        assert_eq!(
            foot_line(&mut p, gone),
            "the project isn't read now, so the answer was dropped"
        );
        // An edit's outcome is still said; the view isn't asked to go on.
        let ran = Ran {
            code: Some(0),
            stdout: "REQ-001: + verifies TEST-001\n".into(),
            stderr: vec![],
        };
        assert!(p.edited(Tab::Check, "link", &Ok(ran), gone).is_empty());
        assert_eq!(foot_line(&mut p, gone), "REQ-001: + verifies TEST-001");
        let now = loaded(&d, 1);
        keys(&mut p, "9", now);
        let s = screen(&mut p, now, 80, 8);
        assert!(
            s.contains("\nheard:") && !s.contains("target"),
            "the view heard nothing: {s}"
        );
    }

    #[test]
    fn not_loaded_states_are_said_on_every_tab() {
        let mut p = Plan::new();
        let now = Now {
            state: State::Loading,
            loads: 0,
        };
        assert!(screen(&mut p, now, 60, 8).contains("reading the project…"));
        keys(&mut p, "5", now);
        let s = screen(&mut p, now, 60, 8);
        assert!(
            s.contains("reading the project…") && s.contains("r try again"),
            "{s}"
        );
    }

    /// Kiln's budget, 2,000 items, nested and in every type.
    pub(crate) fn big() -> Loaded {
        let kinds = [
            "requirement",
            "design",
            "decision",
            "milestone",
            "task",
            "test",
            "risk",
        ];
        let mut items = Vec::new();
        for n in 0..2000 {
            let kind = kinds[n % kinds.len()];
            let id = format!("{}-{n:04}", kind[..3].to_uppercase());
            // A chain under every tenth, so nesting goes deep too.
            let parent = (n >= kinds.len() && n % 10 != 0).then(|| {
                let p = n - kinds.len();
                format!("{}-{p:04}", kind[..3].to_uppercase())
            });
            items.push(Item {
                id,
                kind: kind.into(),
                title: format!("Item number {n} with a title of some length"),
                status: if n % 13 == 0 {
                    "dropped".into()
                } else {
                    "draft".into()
                },
                parent,
                tags: vec![],
                jira_keys: vec![],
                owner: None,
                due: None,
                done: false,
                closed: n % 13 == 0,
                links: Default::default(),
                path: format!("design/{kind}/{n}.md"),
                line: 2,
                body: None,
            });
        }
        items.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.id.cmp(&b.id)));
        Loaded::new(version(), items, Err("none".into()), Ok(vec![]))
    }

    #[test]
    fn two_thousand_items_build_and_draw_well_inside_a_call() {
        let l = big();
        let mut p = Plan::new();
        let started = std::time::Instant::now();
        for loads in 1..=5 {
            let now = loaded(&l, loads);
            let s = screen(&mut p, now, 120, 40);
            assert!(s.contains("Item number"));
            keys(&mut p, "G", now);
            keys(&mut p, "/number 1", now);
            keys(&mut p, "<Esc>", now);
            keys(&mut p, "a", now);
        }
        let each = started.elapsed() / 5;
        // A page call has 1 s; this catches what grows faster than the
        // project, not a slow machine. Natively it's a few ms.
        assert!(each.as_millis() < 250, "{each:?} a read");
    }
}
