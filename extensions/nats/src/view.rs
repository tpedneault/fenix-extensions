//! The live page: a section a subject, opened to its messages newest
//! first, a message opened to its headers and payload.
//!
//! It's laid out from the store as it is when Fenix asks, and only what's
//! on screen is: rows are worked out as blocks (a subject's row, a run of
//! its messages, an opened message's lines) whose heights are known
//! without drawing them, and only the rows that show are drawn. A filter
//! that has to read payloads reads them here, a time budget at a time, for
//! the subjects being looked at first; what it found is kept, and only
//! messages newer than those it read are read next time.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use fenix_extension_api::grid::{self, Grid, Popup};
use fenix_extension_api::{Cells, Key, Page, Role, Shade};

use crate::core::Core;
use crate::filter::{Colour, Filter};
use crate::keys::Keys;
use crate::payload::{self, Line};
use crate::store::{Store, View};

/// A row of the page, by what it shows: it stays on the same message as
/// new ones come in above it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Focus {
    Subject(u32),
    Node(String),
    Msg(u32, u32),
    Line(u32, u32, usize),
    Note(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Rate,
    Count,
    Recent,
}

impl Sort {
    fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Rate,
            Sort::Rate => Sort::Count,
            Sort::Count => Sort::Recent,
            Sort::Recent => Sort::Name,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Rate => "rate",
            Sort::Count => "count",
            Sort::Recent => "last seen",
        }
    }
}

/// What a subject's filter has read of its messages.
#[derive(Debug, Default, Clone)]
struct Scan {
    gen: u64,
    /// The newest and oldest messages read.
    top: Option<u32>,
    bottom: Option<u32>,
    /// What matched, newest first.
    matches: VecDeque<u32>,
    /// Read down to its oldest.
    done: bool,
}

/// A time budget.
#[derive(Clone, Copy)]
pub struct Budget {
    until: Instant,
}

impl Budget {
    pub fn ms(ms: u64) -> Budget {
        Budget { until: Instant::now() + Duration::from_millis(ms) }
    }
    fn spent(&self) -> bool {
        Instant::now() >= self.until
    }
}

#[derive(Debug, Clone)]
enum Block {
    Node { path: String, depth: usize, label: String, count: u64, rate: f64, subjects: usize, open: bool },
    Subject { id: u32, depth: usize, label: String },
    /// Its messages from `from` to `to` (newest first).
    Msgs { id: u32, depth: usize, from: usize, to: usize },
    Detail { id: u32, seq: u32, depth: usize, lines: usize },
    Note { id: u32, depth: usize, text: String },
}

impl Block {
    fn height(&self) -> usize {
        match self {
            Block::Msgs { from, to, .. } => to - from,
            Block::Detail { lines, .. } => *lines,
            _ => 1,
        }
    }
}

/// What a key asks of the extension, beyond the page itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    /// The structured filter, to write.
    Filter,
    SavedFilters,
    Rules,
    Publish,
    Request,
    /// The message, as is, again.
    Republish(u32, u32),
    /// The message, in the publish buffer.
    Edit(u32, u32),
    Copy(u32, u32),
    CopyCommand(u32, u32),
    OpenWhole(u32, u32),
    ExportSubject(u32),
    ExportView,
    Subscribe,
    Unsubscribe,
    NextConnection,
    ToggleConnection,
    JetStream,
    Clear,
}

pub struct LiveView {
    pub conn: u16,
    pub tree: bool,
    pub sort: Sort,
    pub quick: grid::Filter,
    /// The structured filter (`f`).
    pub structured: Option<Filter>,
    effective: Option<Filter>,
    /// Bumped when the filter changes: what scans found goes.
    gen: u64,
    pub expanded: HashSet<u32>,
    pub open_nodes: HashSet<String>,
    pub open_msgs: HashSet<(u32, u32)>,
    details: HashMap<(u32, u32), Vec<Line>>,
    pub focus: Option<Focus>,
    /// The focused row's line on screen: kept as rows come and go above.
    screen_row: usize,
    scans: HashMap<u32, Scan>,
    list_height: usize,
    /// The rows drawn, by line: the mouse's.
    rows_at: Vec<(usize, Focus)>,
    /// A line said at the foot: what a key did.
    pub note: Option<(String, Role)>,
    /// Where you are, for times of day: seconds east of UTC.
    pub offset_s: i64,
    pub help: bool,
    /// Subjects still being checked by a filter that reads payloads.
    pending: usize,
    /// Where the background scan is, among the subjects.
    scan_cursor: usize,
    /// The keys its hints name, as the key scheme in use has them.
    pub keys: Keys,
}

impl Default for LiveView {
    fn default() -> LiveView {
        LiveView {
            conn: 0,
            tree: false,
            sort: Sort::Name,
            quick: grid::Filter::default(),
            structured: None,
            effective: None,
            gen: 1,
            expanded: HashSet::new(),
            open_nodes: HashSet::new(),
            open_msgs: HashSet::new(),
            details: HashMap::new(),
            focus: None,
            screen_row: 0,
            scans: HashMap::new(),
            list_height: 20,
            rows_at: Vec::new(),
            note: None,
            offset_s: 0,
            help: false,
            pending: 0,
            scan_cursor: 0,
            keys: Keys::default(),
        }
    }
}

/// Lines above the list, and below it.
const HEAD: usize = 7;
const FOOT: usize = 3;

impl LiveView {
    /// The filter as it stands, both parts.
    pub fn filter(&self) -> Option<&Filter> {
        self.effective.as_ref()
    }

    /// The quick or structured filter changed.
    pub fn refilter(&mut self) {
        let quick = Filter::parse(&self.quick.text).ok().filter(|f| !f.is_empty());
        self.effective = match (&quick, &self.structured) {
            (Some(q), Some(s)) => Some(s.and(q)),
            (Some(q), None) => Some(q.clone()),
            (None, Some(s)) => Some(s.clone()),
            (None, None) => None,
        };
        self.gen += 1;
        self.scans.clear();
        self.scan_cursor = 0;
    }

    /// Everything forgotten with the store's messages.
    pub fn forget(&mut self) {
        self.scans.clear();
        self.details.clear();
        self.open_msgs.clear();
        self.expanded.clear();
        self.focus = None;
    }

    /// Whether subject `id`'s messages must be read to tell which match.
    fn needs_scan(&self, store: &Store, id: u32) -> bool {
        let Some(f) = &self.effective else { return false };
        if !f.per_message() {
            return false;
        }
        let only_words = f.headers.is_empty() && f.regexes.is_empty() && f.json.is_empty() && f.min_size.is_none() && f.max_size.is_none();
        !(only_words && f.words_in_subject(&store.subjects[id as usize].name))
    }

    /// Reads more of subject `id`'s messages, until `want` have matched,
    /// it's read them all, or the budget's spent.
    fn advance(&mut self, store: &Store, id: u32, want: usize, budget: &Budget) {
        let Some(f) = &self.effective else { return };
        let gen = self.gen;
        let scan = self.scans.entry(id).or_default();
        if scan.gen != gen {
            *scan = Scan { gen, ..Scan::default() };
        }
        let s = &store.subjects[id as usize];
        let wis = f.words_in_subject(&s.name);
        let check = |e: &crate::store::Entry| f.size_ok(e.size) && store.view(id, e).is_some_and(|v| f.message_ok(&v, wis));
        while let Some(&b) = scan.matches.back() {
            if s.position(b).is_some() {
                break;
            }
            scan.matches.pop_back();
        }
        let mut n = 0;
        // What's come since.
        match scan.top.map(|t| s.position(t)) {
            Some(Some(p)) => {
                for e in s.msgs.range(p + 1..) {
                    if check(e) {
                        scan.matches.push_front(e.seq);
                    }
                    scan.top = Some(e.seq);
                    n += 1;
                    if n % 32 == 0 && budget.spent() {
                        return;
                    }
                }
            }
            Some(None) => {
                // Its newest read went: so did all it had read.
                *scan = Scan { gen, ..Scan::default() };
            }
            None => {}
        }
        if scan.top.is_none() {
            let Some(e) = s.msgs.back() else {
                scan.done = true;
                return;
            };
            scan.top = Some(e.seq);
            scan.bottom = Some(e.seq);
            scan.done = false;
            if check(e) {
                scan.matches.push_front(e.seq);
            }
        }
        // Further back.
        if scan.done || scan.matches.len() >= want {
            return;
        }
        let Some(start) = scan.bottom.and_then(|b| s.position(b)) else {
            scan.done = true;
            return;
        };
        for i in (0..start).rev() {
            let e = &s.msgs[i];
            if check(e) {
                scan.matches.push_back(e.seq);
            }
            scan.bottom = Some(e.seq);
            n += 1;
            if i == 0 {
                scan.done = true;
            }
            if scan.matches.len() >= want || (n % 32 == 0 && budget.spent()) {
                return;
            }
        }
        if start == 0 {
            scan.done = true;
        }
    }

    /// How many of subject `id`'s messages are listed, and whether more may
    /// be found.
    fn listed(&self, store: &Store, id: u32) -> (usize, bool) {
        if self.needs_scan(store, id) {
            self.scans.get(&id).filter(|s| s.gen == self.gen).map_or((0, true), |s| (s.matches.len(), !s.done))
        } else {
            (store.subjects[id as usize].msgs.len(), false)
        }
    }

    /// Subject `id`'s `i`th message listed, newest first.
    fn listed_seq(&self, store: &Store, id: u32, i: usize) -> Option<u32> {
        if self.needs_scan(store, id) {
            self.scans.get(&id)?.matches.get(i).copied()
        } else {
            let s = &store.subjects[id as usize];
            s.msgs.get(s.msgs.len().checked_sub(i + 1)?).map(|e| e.seq)
        }
    }

    fn listed_index(&self, store: &Store, id: u32, seq: u32) -> Option<usize> {
        if self.needs_scan(store, id) {
            self.scans.get(&id)?.matches.iter().position(|s| *s == seq)
        } else {
            let s = &store.subjects[id as usize];
            Some(s.msgs.len() - 1 - s.position(seq)?)
        }
    }

    /// Whether subject `id` is listed at all.
    fn shown(&self, store: &Store, id: u32) -> Option<bool> {
        let s = &store.subjects[id as usize];
        let Some(f) = &self.effective else { return Some(true) };
        if !s.overflow && !f.subject_ok(&s.name) {
            return Some(false);
        }
        if !self.needs_scan(store, id) {
            return Some(true);
        }
        match self.scans.get(&id).filter(|x| x.gen == self.gen) {
            Some(x) if !x.matches.is_empty() => Some(true),
            Some(x) if x.done => Some(false),
            _ => None,
        }
    }

    /// Between frames: a filter that reads payloads reads some more, so
    /// subjects not in view are found too.
    pub fn background(&mut self, core: &Core, ms: u64) -> bool {
        self.find_some(core, &Budget::ms(ms))
    }

    fn find_some(&mut self, core: &Core, budget: &Budget) -> bool {
        let store = &core.store;
        if self.effective.as_ref().is_none_or(|f| !f.per_message()) {
            return false;
        }
        let n = store.subjects.len();
        let mut any = false;
        for k in 0..n {
            let id = ((self.scan_cursor + k) % n) as u32;
            if store.subjects[id as usize].conn != self.conn || self.shown(store, id).is_some() {
                // Those found keep up with new messages when drawn.
                continue;
            }
            if !self.effective.as_ref().is_some_and(|f| f.subject_ok(&store.subjects[id as usize].name)) {
                continue;
            }
            self.advance(store, id, 1, budget);
            any = true;
            if budget.spent() {
                self.scan_cursor = (self.scan_cursor + k) % n.max(1);
                return true;
            }
        }
        any
    }

    fn subjects(&self, core: &Core, now_ms: u64) -> Vec<u32> {
        let store = &core.store;
        let now_sec = now_ms / 1000;
        let mut ids: Vec<u32> = (0..store.subjects.len() as u32).filter(|id| store.subjects[*id as usize].conn == self.conn).filter(|id| self.shown(store, *id) == Some(true)).collect();
        let s = |id: &u32| &store.subjects[*id as usize];
        match self.sort {
            _ if self.tree => ids.sort_by(|a, b| s(a).name.cmp(&s(b).name)),
            Sort::Name => ids.sort_by(|a, b| s(a).overflow.cmp(&s(b).overflow).then_with(|| s(a).name.cmp(&s(b).name))),
            Sort::Rate => ids.sort_by(|a, b| s(b).rate.per_sec(now_sec).total_cmp(&s(a).rate.per_sec(now_sec)).then_with(|| s(a).name.cmp(&s(b).name))),
            Sort::Count => ids.sort_by(|a, b| s(b).count.cmp(&s(a).count).then_with(|| s(a).name.cmp(&s(b).name))),
            Sort::Recent => ids.sort_by(|a, b| s(b).last_ms.cmp(&s(a).last_ms).then_with(|| s(a).name.cmp(&s(b).name))),
        }
        ids
    }

    fn pending_count(&self, core: &Core) -> usize {
        let store = &core.store;
        (0..store.subjects.len() as u32).filter(|id| store.subjects[*id as usize].conn == self.conn && self.shown(store, *id).is_none()).count()
    }

    /// The opened message's lines, made when it's first drawn.
    fn detail(&mut self, store: &Store, id: u32, seq: u32) -> usize {
        if let Some(d) = self.details.get(&(id, seq)) {
            return d.len();
        }
        let Some(v) = store.get(id, seq) else { return 0 };
        let lines = detail_lines(&v, self.offset_s);
        let n = lines.len();
        if self.details.len() > 64 {
            let open = &self.open_msgs;
            self.details.retain(|k, _| open.contains(k));
        }
        self.details.insert((id, seq), lines);
        n
    }

    #[allow(clippy::too_many_arguments)]
    fn subject_blocks(&mut self, core: &Core, id: u32, depth: usize, label: String, out: &mut Vec<Block>, want: usize, budget: &Budget) {
        out.push(Block::Subject { id, depth, label });
        if !self.expanded.contains(&id) {
            return;
        }
        let store = &core.store;
        if self.needs_scan(store, id) {
            self.advance(store, id, want, budget);
        }
        let (n, more) = self.listed(store, id);
        let mut open: Vec<(usize, u32)> = self.open_msgs.iter().filter(|(s, _)| *s == id).filter_map(|(_, seq)| Some((self.listed_index(store, id, *seq)?, *seq))).collect();
        open.sort_unstable();
        let mut from = 0;
        for (i, seq) in open {
            if i >= n {
                continue;
            }
            out.push(Block::Msgs { id, depth: depth + 1, from, to: i + 1 });
            let lines = self.detail(store, id, seq);
            out.push(Block::Detail { id, seq, depth: depth + 2, lines });
            from = i + 1;
        }
        if from < n {
            out.push(Block::Msgs { id, depth: depth + 1, from, to: n });
        }
        let s = &store.subjects[id as usize];
        if more {
            out.push(Block::Note { id, depth: depth + 1, text: "checking older messages…".into() });
        } else if n == 0 {
            let text = if s.msgs.is_empty() {
                if s.skipped > 0 { "none stored: sampling left them out".to_string() } else { "none stored".to_string() }
            } else {
                "none matches the filter".to_string()
            };
            out.push(Block::Note { id, depth: depth + 1, text });
        } else if s.evicted > 0 || s.skipped > 0 {
            let mut parts = Vec::new();
            if s.evicted > 0 {
                parts.push(format!("{} older let go", payload::count(s.evicted as f64)));
            }
            if s.skipped > 0 {
                parts.push(format!("{} not stored (sampling)", payload::count(s.skipped as f64)));
            }
            out.push(Block::Note { id, depth: depth + 1, text: parts.join(" · ") });
        }
    }

    fn blocks(&mut self, core: &Core, now_ms: u64, want: usize, budget: &Budget) -> Vec<Block> {
        let ids = self.subjects(core, now_ms);
        let store = &core.store;
        let mut out = Vec::new();
        if !self.tree {
            for id in ids {
                let label = store.subjects[id as usize].name.to_string();
                self.subject_blocks(core, id, 0, label, &mut out, want, budget);
            }
            return out;
        }
        // Totals for every prefix.
        let now_sec = now_ms / 1000;
        let mut totals: HashMap<String, (u64, f64, usize)> = HashMap::new();
        for id in &ids {
            let s = &store.subjects[*id as usize];
            let tokens: Vec<&str> = s.name.split('.').collect();
            let rate = s.rate.per_sec(now_sec);
            for j in 1..tokens.len() {
                let t = totals.entry(tokens[..j].join(".")).or_default();
                t.0 += s.count;
                t.1 += rate;
                t.2 += 1;
            }
        }
        let mut stack: Vec<String> = Vec::new();
        for id in ids {
            let name = store.subjects[id as usize].name.to_string();
            let tokens: Vec<&str> = name.split('.').collect();
            let prefixes: Vec<String> = (1..tokens.len()).map(|j| tokens[..j].join(".")).collect();
            let common = stack.iter().zip(&prefixes).take_while(|(a, b)| a == b).count();
            stack.truncate(common);
            for (j, p) in prefixes.iter().enumerate().skip(common) {
                let visible = prefixes[..j].iter().all(|q| self.open_nodes.contains(q));
                if visible {
                    let (count, rate, subjects) = totals.get(p).copied().unwrap_or_default();
                    out.push(Block::Node { path: p.clone(), depth: j, label: tokens[j].to_string(), count, rate, subjects, open: self.open_nodes.contains(p) });
                }
                stack.push(p.clone());
            }
            if prefixes.iter().all(|q| self.open_nodes.contains(q)) {
                let label = tokens.last().copied().unwrap_or("").to_string();
                self.subject_blocks(core, id, prefixes.len(), label, &mut out, want, budget);
            }
        }
        out
    }

    /// The focus of row `row`.
    fn focus_at(&self, core: &Core, blocks: &[Block], row: usize) -> Option<Focus> {
        let mut at = 0;
        for b in blocks {
            let h = b.height();
            if row < at + h {
                let k = row - at;
                return Some(match b {
                    Block::Node { path, .. } => Focus::Node(path.clone()),
                    Block::Subject { id, .. } => Focus::Subject(*id),
                    Block::Msgs { id, from, .. } => Focus::Msg(*id, self.listed_seq(&core.store, *id, from + k)?),
                    Block::Detail { id, seq, .. } => Focus::Line(*id, *seq, k),
                    Block::Note { id, .. } => Focus::Note(*id),
                });
            }
            at += h;
        }
        None
    }

    /// The row of `focus`.
    fn row_of(&self, core: &Core, blocks: &[Block], focus: &Focus) -> Option<usize> {
        let mut at = 0;
        for b in blocks {
            match (b, focus) {
                (Block::Node { path, .. }, Focus::Node(p)) if path == p => return Some(at),
                (Block::Subject { id, .. }, Focus::Subject(f)) if id == f => return Some(at),
                (Block::Msgs { id, from, to, .. }, Focus::Msg(f, seq)) if id == f => {
                    if let Some(i) = self.listed_index(&core.store, *id, *seq) {
                        if (*from..*to).contains(&i) {
                            return Some(at + i - from);
                        }
                    }
                }
                (Block::Detail { id, seq, lines, .. }, Focus::Line(f, s, l)) if id == f && seq == s => return Some(at + (*l).min(lines.saturating_sub(1))),
                (Block::Note { id, .. }, Focus::Note(f)) if id == f => return Some(at),
                _ => {}
            }
            at += b.height();
        }
        None
    }

    /// Where the focus falls back to when what it was on has gone: its
    /// subject, or its node.
    fn fallback(focus: &Focus) -> Option<Focus> {
        match focus {
            Focus::Line(id, seq, _) => Some(Focus::Msg(*id, *seq)),
            Focus::Msg(id, _) | Focus::Note(id) => Some(Focus::Subject(*id)),
            _ => None,
        }
    }

    fn resolve(&mut self, core: &Core, blocks: &[Block]) -> (usize, usize) {
        let total: usize = blocks.iter().map(Block::height).sum();
        let mut focus = self.focus.clone();
        let mut row = None;
        while let Some(f) = focus.clone() {
            if let Some(r) = self.row_of(core, blocks, &f) {
                row = Some(r);
                break;
            }
            focus = Self::fallback(&f);
        }
        let row = row.unwrap_or(0).min(total.saturating_sub(1));
        self.focus = self.focus_at(core, blocks, row);
        (row, total)
    }

    /// Moves the focus `delta` rows.
    fn step(&mut self, core: &Core, now_ms: u64, delta: isize) {
        let blocks = self.blocks(core, now_ms, self.want(), &Budget::ms(5));
        let (row, total) = self.resolve(core, &blocks);
        let new = (row as isize + delta).clamp(0, total.saturating_sub(1) as isize) as usize;
        self.screen_row = (self.screen_row as isize + (new as isize - row as isize)).clamp(0, self.list_height.saturating_sub(1) as isize) as usize;
        self.focus = self.focus_at(core, &blocks, new);
    }

    fn want(&self) -> usize {
        self.list_height * 2 + 50
    }

    /// The subject the focus is in.
    pub fn focused_subject(&self) -> Option<u32> {
        match self.focus {
            Some(Focus::Subject(id) | Focus::Msg(id, _) | Focus::Line(id, _, _) | Focus::Note(id)) => Some(id),
            _ => None,
        }
    }

    pub fn focused_message(&self) -> Option<(u32, u32)> {
        match self.focus {
            Some(Focus::Msg(id, seq) | Focus::Line(id, seq, _)) => Some((id, seq)),
            _ => None,
        }
    }

    /// Every message the filter lets through, oldest first, subject by
    /// subject: what an export of the view writes.
    pub fn matching(&self, core: &Core) -> Vec<(u32, u32)> {
        let store = &core.store;
        let mut out = Vec::new();
        for (id, s) in store.subjects.iter().enumerate() {
            if s.conn != self.conn {
                continue;
            }
            let id = id as u32;
            let f = self.effective.as_ref();
            if f.is_some_and(|f| !s.overflow && !f.subject_ok(&s.name)) {
                continue;
            }
            let wis = f.is_none_or(|f| f.words_in_subject(&s.name));
            for e in &s.msgs {
                let ok = match f {
                    None => true,
                    Some(f) => store.view(id, e).is_some_and(|v| f.message_ok(&v, wis)),
                };
                if ok {
                    out.push((id, e.seq));
                }
            }
        }
        out
    }

    /// The page: laid out from the store as it is now.
    pub fn render(&mut self, core: &Core, width: u32, height: u32, now_ms: u64) -> Page {
        let width = (width as usize).max(40);
        let height = (height as usize).max(HEAD + FOOT + 3);
        self.list_height = height - HEAD - FOOT;
        // Subjects not yet known to match: some read, a few milliseconds'
        // worth; then the rows, the rest of the budget.
        self.find_some(core, &Budget::ms(3));
        let budget = Budget::ms(5);
        let blocks = self.blocks(core, now_ms, self.want(), &budget);
        self.pending = self.pending_count(core);
        let (row, total) = self.resolve(core, &blocks);
        let lh = self.list_height;
        self.screen_row = self.screen_row.min(lh.saturating_sub(1)).min(row);
        let mut top = row - self.screen_row;
        if total > lh && top + lh > total {
            top = total - lh;
            self.screen_row = row - top;
        }
        let mut g = Grid::new();
        let mut shades: Vec<(Cells, Shade)> = Vec::new();
        self.header(&mut g, core, width, now_ms);
        self.rows_at.clear();
        // The rows on screen.
        let mut at = 0;
        let mut line = HEAD;
        for b in &blocks {
            let h = b.height();
            if at + h <= top {
                at += h;
                continue;
            }
            let first = top.saturating_sub(at);
            for k in first..h {
                if line >= HEAD + lh {
                    break;
                }
                let r = at + k;
                self.draw_row(&mut g, &mut shades, core, b, k, line, width, now_ms);
                if r == row {
                    g.focus(line, 0..width);
                }
                if let Some(f) = self.focus_at_block(core, b, k) {
                    self.rows_at.push((line, f));
                }
                line += 1;
            }
            at += h;
            if line >= HEAD + lh {
                break;
            }
        }
        if total == 0 {
            let text = if core.conns.is_empty() {
                format!("No servers: add one to nats.servers ({}), then {} connects.", self.keys.settings, self.keys.connect)
            } else if self.pending > 0 {
                format!("Checking {} subjects for the filter…", self.pending)
            } else if self.effective.is_some() {
                "Nothing matches the filter.".to_string()
            } else {
                "Nothing yet: messages show here as they come.".to_string()
            };
            g.put(HEAD, 2, &grid::fit(&text, width - 4), Role::Muted);
        }
        if self.pending > 0 && total > 0 && line < HEAD + lh {
            g.put(line, 2, &format!("checking {} more subjects for the filter…", self.pending), Role::Muted);
        }
        // The foot.
        let foot = HEAD + lh;
        g.rule(foot, 0..width);
        if let Some((text, role)) = &self.note {
            g.put(foot + 1, 1, &grid::fit(text, width - 2), role.clone());
        } else {
            let keys: &[(&str, &str)] = &[("Enter", "open"), ("/", "find"), ("f", "filter"), ("t", "tree"), ("P", "pause"), ("p", "publish"), ("r", "request"), ("J", "JetStream"), ("?", "keys")];
            let mut x = 1;
            for (k, label) in keys {
                if x + k.len() + label.len() + 2 > width {
                    break;
                }
                x = g.put(foot + 1, x, k, Role::Accent) + 1;
                x = g.put(foot + 1, x, label, Role::Muted) + 3;
            }
        }
        if self.help {
            g.popup = Some(Popup { line: HEAD, col: 2, rows: help_rows() });
        }
        let mut page = Page::canvas(g.finish()).typing(self.quick.typing).more_keys(true).mouse(true);
        for (cells, shade) in shades {
            page = page.background(cells, shade);
        }
        page
    }

    fn focus_at_block(&self, core: &Core, b: &Block, k: usize) -> Option<Focus> {
        Some(match b {
            Block::Node { path, .. } => Focus::Node(path.clone()),
            Block::Subject { id, .. } => Focus::Subject(*id),
            Block::Msgs { id, from, .. } => Focus::Msg(*id, self.listed_seq(&core.store, *id, from + k)?),
            Block::Detail { id, seq, .. } => Focus::Line(*id, *seq, k),
            Block::Note { id, .. } => Focus::Note(*id),
        })
    }

    fn header(&self, g: &mut Grid, core: &Core, width: usize, now_ms: u64) {
        let store = &core.store;
        let now_sec = now_ms / 1000;
        let conn = core.conn_index(self.conn).map(|i| &core.conns[i]);
        // Line 0: the connection.
        let mut x = g.put(0, 1, "NATS", Role::Title);
        match conn {
            Some(c) => {
                x = g.put(0, x + 2, &c.addr.key(), Role::Title);
                let (dot, role) = match c.state() {
                    "up" => ("● up", Role::Good),
                    "connecting" => ("◌ connecting", Role::Warn),
                    "retrying" => ("○ retrying", Role::Bad),
                    _ => ("○ off", Role::Muted),
                };
                x = g.put(0, x + 2, dot, role);
                if let Some(info) = &c.info {
                    let mut parts = Vec::new();
                    if !info.version.is_empty() {
                        parts.push(format!("v{}", info.version));
                    }
                    if !info.server_name.is_empty() {
                        parts.push(info.server_name.clone());
                    }
                    if let Some(cl) = info.cluster.as_deref().filter(|c| !c.is_empty()) {
                        parts.push(format!("cluster {cl}"));
                    }
                    parts.push(format!("JetStream {}", if info.jetstream { "on" } else { "off" }));
                    if let Some(rtt) = c.rtt_us {
                        parts.push(format!("RTT {:.1} ms", rtt as f64 / 1000.0));
                    }
                    if info.ldm {
                        parts.push("lame duck".into());
                    }
                    x = g.put(0, x + 2, &grid::fit(&parts.join(" · "), width.saturating_sub(x + 2)), Role::Muted);
                }
                if core.conns.len() > 1 {
                    let i = core.conn_index(self.conn).unwrap_or(0);
                    let tag = format!("{}/{}  c next", i + 1, core.conns.len());
                    if x + tag.len() + 4 < width {
                        g.put(0, width - tag.len() - 1, &tag, Role::Muted);
                    }
                }
            }
            None => {
                g.put(0, x + 2, "no server: nats.servers is empty", Role::Muted);
            }
        }
        // Line 1: what's coming. Line 2: what's wrong, or how it copes.
        let (msgs, rate) = store.totals.conns.get(self.conn as usize).map_or((0, 0.0), |c| (c.0, c.2.per_sec(now_sec)));
        let mut parts = vec![format!("{} msg/s", payload::count(rate)), format!("{} messages", payload::count(msgs as f64)), format!("{} subjects", store.subjects.iter().filter(|s| s.conn == self.conn).count())];
        parts.push(format!("stored {} / {}", payload::bytes(store.stored_bytes() as f64), payload::bytes(store.limits.budget as f64)));
        g.put(1, 1, &grid::fit(&parts.join(" · "), width - 2), Role::Text);
        if let Some(e) = conn.and_then(|c| c.error.as_ref().filter(|_| c.state() != "up")) {
            g.put(2, 1, &grid::fit(e, width - 2), Role::Bad);
        } else {
            let mut flags: Vec<(String, Role)> = Vec::new();
            if store.sampling > 1 {
                let s = if store.sampling >= crate::core::COUNT_ONLY { "counting only".to_string() } else { format!("sampling 1:{}", store.sampling) };
                flags.push((format!("{s} · {} not stored", payload::count(store.totals.skipped as f64)), Role::Warn));
            } else if store.totals.skipped > 0 && !store.paused {
                flags.push((format!("{} not stored", payload::count(store.totals.skipped as f64)), Role::Muted));
            }
            if store.paused {
                flags.push(("paused: counting, not storing (P resumes)".into(), Role::Warn));
            }
            let load = &core.load;
            if load.busy >= 0.05 || load.lag_ms >= 50 || store.sampling > 1 {
                flags.push((format!("busy {:.0}% · timer {} ms late · longest chunk {:.1} ms", load.busy * 100.0, load.lag_ms, load.chunk_max_recent_us as f64 / 1000.0), Role::Muted));
            }
            if let Some(w) = conn.and_then(|c| c.warning.as_ref()) {
                flags.push((w.clone(), Role::Warn));
            }
            let mut x = 1;
            for (t, r) in flags {
                if x + 4 >= width {
                    break;
                }
                x = g.put(2, x, &grid::fit(&t, width - x - 1), r) + 3;
            }
        }
        // Line 3: subscriptions.
        if let Some(c) = conn {
            let subs = if c.patterns.is_empty() { "none (a subscribes)".to_string() } else { c.patterns.join("  ") };
            let x = g.put(3, 1, "subscribed ", Role::Muted);
            let x = g.put(3, x, &grid::fit(&subs, width.saturating_sub(x + 2)), Role::Accent);
            let active = c.wanted_subjects();
            if !c.narrow.is_empty() && x + 12 < width {
                g.put(3, x + 2, &grid::fit(&format!("narrowed by the filter to {}", if active.is_empty() { "nothing".into() } else { active.join(" ") }), width - x - 3), Role::Muted);
            }
        }
        // Line 4: the filter.
        let (text, role) = self.quick.line("find in subjects and payloads", width.saturating_sub(2));
        g.put(4, 1, &text, role);
        if let Some(f) = &self.structured {
            let label = format!("filter: {}  (f changes it, F saved ones)", f.source);
            let start = (text.chars().count() + 4).max(width / 2);
            if start + 10 < width {
                g.put(4, start, &grid::fit(&label, width - start - 1), Role::Accent);
            }
        }
        // Line 5: headings.
        let cols = Cols::new(width);
        g.put(5, 2, if self.tree { "SUBJECT TREE" } else { "SUBJECT" }, Role::Muted);
        g.put(5, cols.count, &format!("{:>9}", "MESSAGES"), Role::Muted);
        g.put(5, cols.rate, &format!("{:>8}", "RATE/S"), Role::Muted);
        g.put(5, cols.last, &format!("{:>5}", "LAST"), Role::Muted);
        g.put(5, cols.avg, &format!("{:>9}", "AVG SIZE"), Role::Muted);
        if cols.spark_w > 0 {
            g.put(5, cols.spark, &format!("LAST {} S", cols.spark_w), Role::Muted);
        }
        if !self.tree && self.sort != Sort::Name {
            g.put(5, 10, &format!("by {}", self.sort.name()), Role::Muted);
        }
        g.rule(6, 0..width);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(&mut self, g: &mut Grid, shades: &mut Vec<(Cells, Shade)>, core: &Core, b: &Block, k: usize, line: usize, width: usize, now_ms: u64) {
        let store = &core.store;
        let now_sec = now_ms / 1000;
        let cols = Cols::new(width);
        match b {
            Block::Node { depth, label, count, rate, subjects, open, .. } => {
                let x = 1 + depth * 2;
                let x = g.put(line, x, if *open { "▾ " } else { "▸ " }, Role::Muted);
                let x = g.put(line, x, &grid::fit(&format!("{label}.*"), cols.count.saturating_sub(x + 10)), Role::Accent);
                g.put(line, x + 1, &format!("{subjects} subject{}", if *subjects == 1 { "" } else { "s" }), Role::Muted);
                g.put(line, cols.count, &format!("{:>9}", payload::count(*count as f64)), Role::Text);
                g.put(line, cols.rate, &format!("{:>8}", payload::count(*rate)), Role::Text);
            }
            Block::Subject { id, depth, label } => {
                let s = &store.subjects[*id as usize];
                let x = 1 + depth * 2;
                let open = self.expanded.contains(id);
                let x = g.put(line, x, if open { "▾ " } else { "▸ " }, Role::Muted);
                let name = if s.overflow { format!("… {} more subjects, counted together", payload::count(s.count as f64)) } else { label.clone() };
                let end = g.put(line, x, &grid::fit(&name, cols.count.saturating_sub(x + 1)), if s.overflow { Role::Muted } else { Role::Title });
                if self.needs_scan(store, *id) {
                    if let Some(scan) = self.scans.get(id).filter(|sc| sc.gen == self.gen) {
                        let found = format!("{}{} match", scan.matches.len(), if scan.done { "" } else { "+" });
                        if end + found.len() + 3 < cols.count {
                            g.put(line, end + 2, &found, Role::Accent);
                        }
                    }
                }
                g.put(line, cols.count, &format!("{:>9}", payload::count(s.count as f64)), Role::Text);
                let rate = s.rate.per_sec(now_sec);
                g.put(line, cols.rate, &format!("{:>8}", payload::count(rate)), if rate > 0.0 { Role::Text } else { Role::Muted });
                g.put(line, cols.last, &format!("{:>5}", payload::ago(now_ms.saturating_sub(s.last_ms))), Role::Muted);
                g.put(line, cols.avg, &format!("{:>9}", payload::bytes(s.average_size())), Role::Muted);
                if cols.spark_w > 0 {
                    g.put(line, cols.spark, &payload::sparkline(&s.rate.series(now_sec, cols.spark_w)), Role::Accent);
                }
                if let Some(c) = self.subject_colour(core, &s.name) {
                    shades.push((Cells { line: line as u32, start: 0, end: width as u32 }, shade(c)));
                }
            }
            Block::Msgs { id, depth, from, .. } => {
                let Some(seq) = self.listed_seq(store, *id, from + k) else { return };
                let Some(v) = store.get(*id, seq) else { return };
                let mut x = 1 + depth * 2;
                let open = self.open_msgs.contains(&(*id, seq));
                x = g.put(line, x, if open { "▾ " } else { "  " }, Role::Muted);
                x = g.put(line, x, &payload::clock(v.time_ms, self.offset_s), Role::Muted) + 2;
                x = g.put(line, x, &format!("{:>9}", payload::bytes(v.entry.size as f64)), Role::Text) + 2;
                if v.entry.header_count > 0 {
                    x = g.put(line, x, &format!("h{:<2}", v.entry.header_count), Role::Accent) + 1;
                } else {
                    x += 4;
                }
                if let Some(r) = v.reply {
                    let r = grid::fit(&format!("↩ {r}"), 24.min(width / 5));
                    x = g.put(line, x, &r, Role::Muted) + 2;
                }
                if width > x + 2 {
                    let (text, kind) = payload::preview(v.payload, width - x - 1);
                    let role = match kind {
                        payload::Kind::Binary => Role::Syntax("number".into()),
                        payload::Kind::Json => Role::Syntax("string".into()),
                        _ => Role::Text,
                    };
                    g.put(line, x, &text, role);
                }
                if let Some(c) = self.message_colour(core, &v) {
                    shades.push((Cells { line: line as u32, start: 0, end: width as u32 }, shade(c)));
                }
            }
            Block::Detail { id, seq, depth, .. } => {
                let Some(lines) = self.details.get(&(*id, *seq)) else { return };
                let Some(l) = lines.get(k) else { return };
                let mut x = 1 + depth * 2;
                for (text, role) in l {
                    if x >= width - 1 {
                        break;
                    }
                    let t = grid::fit(text, width - 1 - x);
                    x = g.put(line, x, &t, role.clone());
                }
            }
            Block::Note { depth, text, .. } => {
                g.put(line, 1 + depth * 2 + 2, &grid::fit(text, width.saturating_sub(4 + depth * 2)), Role::Muted);
            }
        }
    }

    fn subject_colour(&self, core: &Core, subject: &str) -> Option<Colour> {
        core.rules.list.iter().find(|r| !r.filter.per_message() && r.filter.subject_ok(subject)).map(|r| r.colour)
    }

    fn message_colour(&self, core: &Core, v: &View<'_>) -> Option<Colour> {
        core.rules.list.iter().find(|r| r.filter.per_message() && r.filter.matches(v)).map(|r| r.colour)
    }

    /// A key; what it asks of the extension.
    pub fn key(&mut self, core: &mut Core, key: Key, now_ms: u64) -> Action {
        self.note = None;
        if self.quick.typing {
            if self.quick.key(key) {
                self.refilter();
            }
            return Action::None;
        }
        if self.help {
            self.help = false;
            if matches!(key, Key::Escape | Key::Char('?')) {
                return Action::None;
            }
        }
        match key {
            Key::Char('j') | Key::Down => self.step(core, now_ms, 1),
            Key::Char('k') | Key::Up => self.step(core, now_ms, -1),
            Key::Char('g') | Key::Home => {
                self.focus = None;
                self.screen_row = 0;
            }
            Key::Char('G') | Key::End => self.step(core, now_ms, isize::MAX / 2),
            Key::Char('J') => return Action::JetStream,
            Key::Char('K') => self.step(core, now_ms, -(self.list_height as isize)),
            Key::Enter | Key::Tab | Key::Char('l') => self.toggle(core),
            Key::Char('h') | Key::Left => self.close(),
            Key::Char('/') => self.quick.start(),
            Key::Escape => {
                if !self.quick.is_empty() {
                    self.quick.clear();
                    self.refilter();
                }
            }
            Key::Char('f') => return Action::Filter,
            Key::Char('F') => return Action::SavedFilters,
            Key::Char('H') => return Action::Rules,
            Key::Char('t') => {
                self.tree = !self.tree;
                self.note = Some((if self.tree { "a tree by subject tokens" } else { "a subject a row" }.into(), Role::Muted));
            }
            Key::Char('o') => {
                if let Some((id, seq)) = self.focused_message() {
                    return Action::OpenWhole(id, seq);
                }
                self.sort = self.sort.next();
                self.note = Some((format!("sorted by {}", self.sort.name()), Role::Muted));
            }
            Key::Char('s') => {
                self.sort = self.sort.next();
                self.note = Some((format!("sorted by {}", self.sort.name()), Role::Muted));
            }
            Key::Char('P') => {
                core.store.paused = !core.store.paused;
                self.note = Some((if core.store.paused { "paused: still counting, storing nothing new" } else { "storing again" }.into(), Role::Muted));
            }
            Key::Char('p') => return Action::Publish,
            Key::Char('r') => return Action::Request,
            Key::Char('R') => {
                if let Some((id, seq)) = self.focused_message() {
                    return Action::Republish(id, seq);
                }
            }
            Key::Char('e') => {
                if let Some((id, seq)) = self.focused_message() {
                    return Action::Edit(id, seq);
                }
            }
            Key::Char('y') => {
                if let Some((id, seq)) = self.focused_message() {
                    return Action::Copy(id, seq);
                }
            }
            Key::Char('Y') => {
                if let Some((id, seq)) = self.focused_message() {
                    return Action::CopyCommand(id, seq);
                }
            }
            Key::Char('x') => {
                if let Some(id) = self.focused_subject() {
                    return Action::ExportSubject(id);
                }
            }
            Key::Char('X') => return Action::ExportView,
            Key::Char('a') => return Action::Subscribe,
            Key::Char('d') => return Action::Unsubscribe,
            Key::Char('c') => return Action::NextConnection,
            Key::Char('C') => return Action::ToggleConnection,
            Key::Char('D') => return Action::Clear,
            Key::Char('?') => self.help = true,
            Key::Char('z') => {
                // Every subject closed.
                self.expanded.clear();
                self.open_msgs.clear();
                self.open_nodes.clear();
                if let Some(id) = self.focused_subject() {
                    self.focus = Some(Focus::Subject(id));
                }
            }
            _ => {}
        }
        Action::None
    }

    /// PageUp, PageDown.
    pub fn page(&mut self, core: &Core, now_ms: u64, down: bool) {
        let n = self.list_height.saturating_sub(1).max(1) as isize;
        self.step(core, now_ms, if down { n } else { -n });
    }

    /// Opens or closes what's focused.
    fn toggle(&mut self, core: &Core) {
        match self.focus.clone() {
            Some(Focus::Node(p)) => {
                if !self.open_nodes.remove(&p) {
                    self.open_nodes.insert(p);
                }
            }
            Some(Focus::Subject(id)) => {
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
            }
            Some(Focus::Msg(id, seq)) => {
                if !self.open_msgs.remove(&(id, seq)) {
                    if core.store.get(id, seq).is_some() {
                        self.open_msgs.insert((id, seq));
                    }
                } else {
                    self.details.remove(&(id, seq));
                }
            }
            Some(Focus::Line(id, seq, _)) => {
                self.open_msgs.remove(&(id, seq));
                self.details.remove(&(id, seq));
                self.focus = Some(Focus::Msg(id, seq));
            }
            _ => {}
        }
    }

    /// `h`: closes what's open around the focus, and goes up.
    fn close(&mut self) {
        match self.focus.clone() {
            Some(Focus::Line(id, seq, _)) => {
                self.open_msgs.remove(&(id, seq));
                self.focus = Some(Focus::Msg(id, seq));
            }
            Some(Focus::Msg(id, seq)) if self.open_msgs.remove(&(id, seq)) => {}
            Some(Focus::Msg(id, _) | Focus::Note(id)) => self.focus = Some(Focus::Subject(id)),
            Some(Focus::Subject(id)) => {
                if !self.expanded.remove(&id) && self.tree {
                    // Up to its node.
                    self.focus = None;
                }
            }
            Some(Focus::Node(p)) if !self.open_nodes.remove(&p) => {
                if let Some((parent, _)) = p.rsplit_once('.') {
                    self.focus = Some(Focus::Node(parent.to_string()));
                }
            }
            Some(Focus::Node(_)) => {}
            None => {}
        }
    }

    /// The mouse: a click focuses a row, a double click opens it, the
    /// wheel moves.
    pub fn mouse(&mut self, core: &Core, now_ms: u64, line: usize, clicks: u8, wheel: i32) {
        if wheel != 0 {
            self.step(core, now_ms, -(wheel as isize) * 3);
            return;
        }
        if let Some((_, f)) = self.rows_at.iter().find(|(l, _)| *l == line).cloned() {
            self.screen_row = line.saturating_sub(HEAD);
            self.focus = Some(f);
            if clicks >= 2 {
                self.toggle(core);
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.quick.typing {
            self.quick.paste(text);
            self.refilter();
        }
    }
}

/// Where each column goes in a page `width` wide.
struct Cols {
    count: usize,
    rate: usize,
    last: usize,
    avg: usize,
    spark: usize,
    spark_w: usize,
}

impl Cols {
    fn new(width: usize) -> Cols {
        let spark_w = if width >= 120 { 30 } else if width >= 90 { 15 } else { 0 };
        let spark = width.saturating_sub(spark_w + 1);
        let avg = spark.saturating_sub(if spark_w > 0 { 11 } else { 10 });
        let last = avg.saturating_sub(7);
        let rate = last.saturating_sub(10);
        let count = rate.saturating_sub(11);
        Cols { count, rate, last, avg, spark, spark_w }
    }
}

fn shade(c: Colour) -> Shade {
    match c {
        Colour::Accent => Shade::Accent,
        Colour::Good => Shade::Good,
        Colour::Warn => Shade::Warn,
        Colour::Bad => Shade::Bad,
        Colour::Rgb(r, g, b) => Shade::Rgb((r, g, b)),
    }
}

/// An opened message: what it is, its headers, its payload.
pub fn detail_lines(v: &View<'_>, offset_s: i64) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let (body, kind, cut) = payload::detail(v.payload);
    let mut facts = vec![payload::clock(v.time_ms, offset_s), payload::bytes(v.entry.size as f64), kind.name().to_string(), format!("#{}", v.entry.seq)];
    if v.entry.truncated {
        facts.push(format!("kept the first {}", payload::bytes(v.payload.len() as f64)));
    }
    lines.push(vec![(v.subject.to_string(), Role::Title), ("  ".into(), Role::Text), (facts.join(" · "), Role::Muted)]);
    if let Some(r) = v.reply {
        lines.push(vec![("reply-to ".into(), Role::Muted), (r.to_string(), Role::Accent)]);
    }
    if let Some(h) = v.headers.and_then(|h| fenix_nats::headers::Headers::parse(h).ok()) {
        if let Some(status) = h.status {
            lines.push(vec![("status ".into(), Role::Muted), (format!("{status} {}", h.description), Role::Warn)]);
        }
        for (k, val) in h.iter() {
            lines.push(vec![(format!("{k}: "), Role::Syntax("property".into())), (val.to_string(), Role::Text)]);
        }
    }
    lines.push(vec![("─".repeat(8), Role::Muted)]);
    lines.extend(body);
    if cut {
        lines.push(vec![(format!("… cut at {} lines: o opens it whole", payload::MAX_LINES), Role::Muted)]);
    }
    lines.push(vec![("y copies it · Y as nats pub · e edits and sends · R sends it again".into(), Role::Muted)]);
    lines
}

fn help_rows() -> Vec<Vec<(String, Role)>> {
    let keys: &[(&str, &str)] = &[
        ("j k  PgUp PgDn  g G", "move"),
        ("Enter l  h", "open, close"),
        ("z", "close them all"),
        ("/", "find in subjects and payloads"),
        ("f  F", "filter: write one, saved ones"),
        ("H", "highlight rules"),
        ("t  s", "tree by tokens, sort"),
        ("P", "pause storing (counting goes on)"),
        ("a  d", "subscribe, unsubscribe"),
        ("p  r", "publish, request"),
        ("R  e", "send again, edit and send"),
        ("y  Y  o", "copy, as nats pub, open whole"),
        ("x  X", "export the subject, the view (JSONL)"),
        ("c  C", "next connection, connect or not"),
        ("J", "JetStream: streams, consumers, KV"),
        ("D", "forget what's stored"),
    ];
    keys.iter().map(|(k, w)| vec![(format!("{k:<22}"), Role::Accent), (w.to_string(), Role::Text)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::up;

    fn text(page: &Page) -> String {
        page.canvas.as_ref().unwrap().all_text()
    }

    fn feed(core: &mut Core, net: &mut crate::core::tests::FakeNet, s: u64, msgs: &[(&str, &str)], now_us: u64) {
        let mut wire = String::new();
        for (subject, payload) in msgs {
            wire.push_str(&format!("MSG {subject} 1 {}\r\n{payload}\r\n", payload.len()));
        }
        core.on_data(net, s, wire.as_bytes(), now_us);
    }

    #[test]
    fn with_no_servers_it_says_the_keys_of_the_scheme_in_use() {
        let core = Core::new(crate::store::Limits::default(), 1_000_000_000);
        let mut v = LiveView::default();
        assert!(text(&v.render(&core, 120, 30, 1_000_000)).contains("No servers: add one to nats.servers (SPC ,), then SPC o m c connects."));
        v.keys = crate::keys::tests::standard();
        assert!(text(&v.render(&core, 120, 30, 1_000_000)).contains("No servers: add one to nats.servers (C-,), then C-k o m c connects."));
    }

    #[test]
    fn subjects_are_listed_and_open_to_their_messages() {
        let (mut core, mut net, s) = up(&[">"]);
        feed(&mut core, &mut net, s, &[("orders.new", "{\"id\":1}"), ("orders.new", "{\"id\":2}"), ("billing.paid", "ok")], 1_000_002_000);
        let mut v = LiveView::default();
        let now = 1_000_003;
        let page = v.render(&core, 120, 30, now);
        let t = text(&page);
        assert!(t.contains("billing.paid") && t.contains("orders.new"), "{t}");
        assert!(t.find("billing.paid").unwrap() < t.find("orders.new").unwrap());
        assert_eq!(v.focus, Some(Focus::Subject(core.store.find(0, "billing.paid").unwrap())));
        v.key(&mut core, Key::Char('j'), now);
        v.key(&mut core, Key::Enter, now);
        let t = text(&v.render(&core, 120, 30, now));
        let id2 = t.find("{\"id\":2}").unwrap();
        assert!(id2 < t.find("{\"id\":1}").unwrap(), "newest first: {t}");
        v.key(&mut core, Key::Char('j'), now);
        v.key(&mut core, Key::Enter, now);
        let t = text(&v.render(&core, 120, 30, now));
        assert!(t.contains("\"id\": 2"), "pretty: {t}");
        assert!(v.focused_message().is_some());
    }

    #[test]
    fn the_focus_stays_on_its_message_as_more_come() {
        let (mut core, mut net, s) = up(&[">"]);
        feed(&mut core, &mut net, s, &[("a", "first"), ("a", "second")], 1_000_002_000);
        let mut v = LiveView::default();
        let now = 1_000_003;
        v.render(&core, 100, 30, now);
        v.key(&mut core, Key::Enter, now);
        v.key(&mut core, Key::Char('j'), now);
        v.key(&mut core, Key::Char('j'), now);
        let (id, seq) = v.focused_message().unwrap();
        assert_eq!(core.store.get(id, seq).unwrap().payload, b"first");
        feed(&mut core, &mut net, s, &[("a", "third"), ("a", "fourth")], 1_000_004_000);
        v.render(&core, 100, 30, now);
        assert_eq!(v.focused_message(), Some((id, seq)));
    }

    #[test]
    fn the_quick_filter_reads_payloads() {
        let (mut core, mut net, s) = up(&[">"]);
        feed(&mut core, &mut net, s, &[("a.one", "nothing"), ("a.two", "has the NEEDLE"), ("a.three", "x"), ("needle.subject", "y")], 1_000_002_000);
        let mut v = LiveView::default();
        let now = 1_000_003;
        v.key(&mut core, Key::Char('/'), now);
        for c in "needle".chars() {
            v.key(&mut core, Key::Char(c), now);
        }
        v.key(&mut core, Key::Enter, now);
        let t = text(&v.render(&core, 100, 30, now));
        assert!(t.contains("a.two") && t.contains("needle.subject"), "{t}");
        assert!(!t.contains("a.one") && !t.contains("a.three"), "{t}");
        assert_eq!(v.matching(&core).len(), 2);
    }

    #[test]
    fn a_tree_groups_by_token_with_totals() {
        let (mut core, mut net, s) = up(&[">"]);
        feed(&mut core, &mut net, s, &[("site1.temp", "1"), ("site1.hum", "2"), ("site1.hum", "3"), ("site2.temp", "4")], 1_000_002_000);
        let mut v = LiveView { tree: true, ..LiveView::default() };
        let now = 1_000_003;
        let t = text(&v.render(&core, 100, 30, now));
        assert!(t.contains("site1.*") && t.contains("2 subjects") && t.contains("site2.*"), "{t}");
        assert!(!t.contains("temp"), "closed: {t}");
        v.key(&mut core, Key::Enter, now);
        let t = text(&v.render(&core, 100, 30, now));
        assert!(t.contains("hum") && t.contains("temp"), "{t}");
    }

    #[test]
    fn only_rows_on_screen_are_drawn() {
        let (mut core, mut net, s) = up(&[">"]);
        let msgs: Vec<(String, String)> = (0..2000).map(|i| (format!("s.{i:04}"), format!("payload {i}"))).collect();
        let refs: Vec<(&str, &str)> = msgs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        feed(&mut core, &mut net, s, &refs, 1_000_002_000);
        let mut v = LiveView::default();
        let page = v.render(&core, 100, 30, 1_000_003);
        assert!(page.canvas.as_ref().unwrap().text.lines().count() <= 30);
        let start = Instant::now();
        for _ in 0..10 {
            v.render(&core, 100, 30, 1_000_003);
        }
        assert!(start.elapsed() < Duration::from_millis(500), "{:?}", start.elapsed());
    }
}
