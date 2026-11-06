//! What's shown of a log: the levels on, a text or a regular expression,
//! patterns an entry must or mustn't have, a field's value (JSON or
//! logfmt), a time range -- and a search, which marks what it finds.
//!
//! A filter is worked out a chunk at a time (`Pass::step`), as the index
//! is, into a bit for each entry: shown or not, found or not. Levels
//! alone need nothing read again; the rest read each chunk's entries.

use std::fmt::Write as _;

use regex::bytes::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::detect::{logfmt_pairs, Level, LEVELS};
use crate::entry::scan;
use crate::hist::Histogram;
use crate::index::{Chunk, Index};
use crate::source::Source;
use crate::time;

/// A text, or a regular expression after `re:`: a text in lower case
/// matches any case, one with a capital its own.
#[derive(Debug, Clone)]
pub struct Matcher {
    pub said: String,
    re: Regex,
}

impl Matcher {
    pub fn new(typed: &str) -> Result<Matcher, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err("Nothing to look for".into());
        }
        let (pattern, regex) = match typed.strip_prefix("re:") {
            Some(p) => (p.trim().to_string(), true),
            None => (regex::escape(typed), false),
        };
        let caseless = !typed.chars().any(char::is_uppercase) || (regex && pattern.starts_with("(?i)"));
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(caseless)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| format!("{typed}: {}", e.to_string().lines().last().unwrap_or("not a regular expression")))?;
        Ok(Matcher { said: typed.to_string(), re })
    }

    pub fn is_match(&self, hay: &[u8]) -> bool {
        self.re.is_match(hay)
    }

    /// Where it matches in `hay`, as byte ranges.
    pub fn find(&self, hay: &[u8]) -> Vec<std::ops::Range<usize>> {
        self.re.find_iter(hay).filter(|m| !m.is_empty()).map(|m| m.range()).take(64).collect()
    }
}

/// How a field is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// `key=value`: the same, any case; `*` at its end, starting so.
    Is,
    /// `key!=value`
    IsNot,
    /// `key~value`: having it in it.
    Has,
}

/// A field of a JSON or logfmt line compared with a value: `user=bob`,
/// `status!=200`, `msg~timeout`, `user.id=7` (a JSON object's field's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    pub op: Op,
    pub value: String,
}

impl Field {
    pub fn new(typed: &str) -> Result<Field, String> {
        let typed = typed.trim();
        let (at, op, n) = [("!=", Op::IsNot), ("=", Op::Is), ("~", Op::Has)]
            .iter()
            .filter_map(|(s, op)| typed.find(s).map(|at| (at, *op, s.len())))
            .min_by_key(|(at, _, _)| *at)
            .ok_or_else(|| format!("{typed}: a field is key=value, key!=value or key~value"))?;
        let key = typed[..at].trim();
        if key.is_empty() {
            return Err(format!("{typed}: which field?"));
        }
        Ok(Field { key: key.to_string(), op, value: typed[at + n..].trim().trim_matches('"').to_lowercase() })
    }

    pub fn said(&self) -> String {
        let op = match self.op {
            Op::Is => "=",
            Op::IsNot => "!=",
            Op::Has => "~",
        };
        format!("{}{op}{}", self.key, self.value)
    }

    /// Whether `line` (an entry's first line) has the field so; a line
    /// without it is kept only by `!=`.
    pub fn keeps(&self, line: &[u8]) -> bool {
        let value = field(line, &self.key).map(|v| v.to_lowercase());
        match (self.op, value) {
            (Op::IsNot, None) => true,
            (_, None) => false,
            (Op::Is, Some(v)) => match self.value.strip_suffix('*') {
                Some(start) => v.starts_with(start),
                None => v == self.value,
            },
            (Op::IsNot, Some(v)) => v != self.value,
            (Op::Has, Some(v)) => v.contains(&self.value),
        }
    }
}

/// A field's value in a JSON line (`a.b` its object's field's) or a
/// logfmt one, as text.
pub fn field(line: &[u8], key: &str) -> Option<String> {
    if line.first() == Some(&b'{') {
        let v: serde_json::Value = serde_json::from_slice(line).ok()?;
        let mut at = &v;
        match at.get(key) {
            Some(direct) => at = direct,
            None => {
                for part in key.split('.') {
                    at = at.get(part)?;
                }
            }
        }
        return Some(match at {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    }
    logfmt_pairs(line).into_iter().find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes())).map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
}

/// A time range: from, to (both in), as typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub said: String,
}

impl Range {
    /// `12:00..12:30`, `2024-03-01 12:00..`, `..2024-03-02`, `-15m` (the
    /// last 15 minutes of the log: `s`, `m`, `h`, `d`), a single time (from
    /// it on). A time of day is on the day of `latest`, the log's last
    /// time; a time `to` as far as it's written goes (`12:30` to its
    /// minute's end, a date to the day's).
    pub fn new(typed: &str, latest: Option<i64>, year: i64) -> Result<Range, String> {
        let t = typed.trim();
        let said = t.to_string();
        if let Some(ago) = t.strip_prefix('-').or_else(|| t.strip_prefix("last ")) {
            let ago = ago.trim();
            let (n, unit) = ago.split_at(ago.find(|c: char| !c.is_ascii_digit()).unwrap_or(ago.len()));
            let n: i64 = n.parse().map_err(|_| format!("{t}: -15m is the last 15 minutes"))?;
            let unit = match unit.trim() {
                "s" | "sec" => 1000,
                "" | "m" | "min" => 60_000,
                "h" => 3_600_000,
                "d" => time::DAY,
                u => return Err(format!("{t}: {u:?} -- s, m, h or d")),
            };
            let last = latest.ok_or("The log has no times yet")?;
            return Ok(Range { from: Some(last - n * unit), to: None, said });
        }
        let day = latest.map_or(0, |l| l.div_euclid(time::DAY));
        let (a, b) = match t.split_once("..") {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (t, None),
        };
        let from = if a.is_empty() { None } else { Some(when(a, day, year, false)?) };
        let to = match b {
            Some(b) if !b.is_empty() => Some(when(b, day, year, true)?),
            _ => None,
        };
        if from.is_none() && to.is_none() {
            return Err("A time range is from..to: 12:00..12:30, -15m".into());
        }
        Ok(Range { from, to, said })
    }

    pub fn keeps(&self, t: Option<i64>) -> bool {
        // An entry with no time can't be placed: it stays.
        let Some(t) = t else { return true };
        self.from.is_none_or(|f| t >= f) && self.to.is_none_or(|z| t <= z)
    }
}

/// A time as typed, in milliseconds; `end`: the last of what it names.
fn when(s: &str, day: i64, year: i64, end: bool) -> Result<i64, String> {
    let b = s.as_bytes();
    if let Some(st) = time::parse(b, year).filter(|st| st.len == b.len()) {
        let to_end = if end { precision(s) - 1 } else { 0 };
        // A bare time of day: on the log's day.
        let ms = if st.ms < time::DAY && st.ms >= 0 && !s.contains('-') && !s.contains('/') { day * time::DAY + st.ms } else { st.ms };
        return Ok(ms + to_end);
    }
    // A date alone.
    if b.len() == 10 && (b[4] == b'-' || b[4] == b'/') {
        if let Some(st) = time::parse(format!("{s} 00:00:00").as_bytes(), year) {
            return Ok(st.ms + if end { time::DAY - 1 } else { 0 });
        }
    }
    // A time of day without seconds: 12:30.
    if let Some(st) = time::parse(format!("{s}:00").as_bytes(), year).filter(|st| st.ms < time::DAY) {
        return Ok(day * time::DAY + st.ms + if end { 59_999 } else { 0 });
    }
    Err(format!("{s}: not a time -- 12:00, 2024-03-01 12:00:03, -15m"))
}

/// What the last digit of a time as typed counts: a second, or less
/// after a dot.
fn precision(s: &str) -> i64 {
    match s.rsplit_once(['.', ',']) {
        Some((_, frac)) if frac.chars().all(|c| c.is_ascii_digit()) => 10i64.pow(3u32.saturating_sub(frac.len() as u32)),
        _ => 1000,
    }
}

/// Which levels are shown: the toggles `e w i d t o`, fatal with error.
pub fn toggle_index(level: Level) -> usize {
    match level {
        Level::Fatal | Level::Error => 0,
        Level::Warn => 1,
        Level::Info => 2,
        Level::Debug => 3,
        Level::Trace => 4,
        Level::Other => 5,
    }
}

/// The level toggles' keys, in order.
pub const TOGGLES: [char; 6] = ['e', 'w', 'i', 'd', 't', 'o'];

/// What's shown.
#[derive(Debug, Clone)]
pub struct Filter {
    /// By toggle: error (and fatal), warn, info, debug, trace, other.
    pub levels: [bool; 6],
    /// `/`: what every entry shown has.
    pub text: Option<Matcher>,
    /// `+`: an entry shown has one of them.
    pub include: Vec<Matcher>,
    /// `-`: and none of these.
    pub exclude: Vec<Matcher>,
    /// `=`: and its fields so.
    pub fields: Vec<Field>,
    /// `@`: and its time in this.
    pub range: Option<Range>,
}

impl Default for Filter {
    fn default() -> Filter {
        Filter { levels: [true; 6], text: None, include: Vec::new(), exclude: Vec::new(), fields: Vec::new(), range: None }
    }
}

impl Filter {
    /// Whether it shows every entry.
    pub fn all(&self) -> bool {
        self.levels.iter().all(|&l| l) && self.only_levels()
    }

    /// Whether it goes by levels alone, so nothing's read again.
    pub fn only_levels(&self) -> bool {
        self.text.is_none() && self.include.is_empty() && self.exclude.is_empty() && self.fields.is_empty() && self.range.is_none()
    }

    pub fn shows_level(&self, level: Level) -> bool {
        self.levels[toggle_index(level)]
    }

    /// Whether an entry is shown: its level, its time, and its text (its
    /// lines' starts, joined).
    pub fn keeps(&self, level: Level, t: Option<i64>, text: &[u8]) -> bool {
        if !self.shows_level(level) || !self.range.as_ref().is_none_or(|r| r.keeps(t)) {
            return false;
        }
        if self.text.as_ref().is_some_and(|m| !m.is_match(text)) {
            return false;
        }
        if !self.include.is_empty() && !self.include.iter().any(|m| m.is_match(text)) {
            return false;
        }
        if self.exclude.iter().any(|m| m.is_match(text)) {
            return false;
        }
        if !self.fields.is_empty() {
            let first = &text[..memchr::memchr(b'\n', text).unwrap_or(text.len())];
            return self.fields.iter().all(|f| f.keeps(first));
        }
        true
    }

    /// The filter as the page says it: `/timeout/ +db -health user=bob
    /// @12:00..12:30`, or nothing.
    pub fn said(&self) -> String {
        let mut s = String::new();
        if let Some(m) = &self.text {
            let _ = write!(s, "/{}/ ", m.said);
        }
        for m in &self.include {
            let _ = write!(s, "+{} ", m.said);
        }
        for m in &self.exclude {
            let _ = write!(s, "-{} ", m.said);
        }
        for f in &self.fields {
            let _ = write!(s, "{} ", f.said());
        }
        if let Some(r) = &self.range {
            let _ = write!(s, "@{} ", r.said);
        }
        s.trim_end().to_string()
    }
}

/// A bit for each entry.
#[derive(Debug, Clone, Default)]
pub struct Bits {
    words: Vec<u64>,
    len: u64,
}

impl Bits {
    fn push(&mut self, on: bool) {
        if self.len.is_multiple_of(64) {
            self.words.push(0);
        }
        if on {
            if let Some(w) = self.words.last_mut() {
                *w |= 1 << (self.len % 64);
            }
        }
        self.len += 1;
    }

    pub fn get(&self, i: u64) -> bool {
        i < self.len && self.words[(i / 64) as usize] >> (i % 64) & 1 == 1
    }

    /// How many are on in `from..to`.
    fn rank(&self, from: u64, to: u64) -> u64 {
        let to = to.min(self.len);
        let mut n = 0;
        let mut i = from;
        while i < to {
            if i.is_multiple_of(64) && to - i >= 64 {
                n += self.words[(i / 64) as usize].count_ones() as u64;
                i += 64;
            } else {
                n += u64::from(self.get(i));
                i += 1;
            }
        }
        n
    }

    /// The `k`th on (from 0) in `from..to`.
    fn select(&self, from: u64, to: u64, mut k: u64) -> Option<u64> {
        let to = to.min(self.len);
        let mut i = from;
        while i < to {
            if i.is_multiple_of(64) && to - i >= 64 {
                let w = self.words[(i / 64) as usize];
                let c = w.count_ones() as u64;
                if k >= c {
                    k -= c;
                    i += 64;
                    continue;
                }
            }
            if self.get(i) {
                if k == 0 {
                    return Some(i);
                }
                k -= 1;
            }
            i += 1;
        }
        None
    }

    /// The first on in `from..to`.
    fn next(&self, from: u64, to: u64) -> Option<u64> {
        self.select(from, to, 0)
    }

    /// The last on in `from..to`.
    fn prev(&self, from: u64, to: u64) -> Option<u64> {
        let mut i = to.min(self.len);
        while i > from {
            if i.is_multiple_of(64) && i - from >= 64 && self.words[(i / 64 - 1) as usize] == 0 {
                i -= 64;
                continue;
            }
            i -= 1;
            if self.get(i) {
                return Some(i);
            }
        }
        None
    }

    pub fn memory(&self) -> usize {
        self.words.capacity() * 8
    }
}

/// A filter and a search worked out over an index, a chunk at a time.
pub struct Pass {
    pub filter: Filter,
    pub search: Option<Matcher>,
    generation: u64,
    shown: Bits,
    found: Bits,
    /// Shown and found before each closed chunk worked out, and after the
    /// last: one more than `done`.
    shown_before: Vec<u64>,
    found_before: Vec<u64>,
    /// The closed chunks worked out.
    done: usize,
    /// The open chunk's entries, worked out for the chunk as it was.
    tail: Vec<(bool, bool)>,
    tail_of: Option<(u64, u64, u64)>,
    tail_first: u64,
    /// When the entries shown are.
    pub hist: Histogram,
}

impl Pass {
    pub fn new(filter: Filter, search: Option<Matcher>, index: &Index) -> Pass {
        Pass {
            filter,
            search,
            generation: index.generation,
            shown: Bits::default(),
            found: Bits::default(),
            shown_before: vec![0],
            found_before: vec![0],
            done: 0,
            tail: Vec::new(),
            tail_of: None,
            tail_first: 0,
            hist: Histogram::default(),
        }
    }

    /// Whether everything is shown and nothing searched: nothing to work
    /// out.
    pub fn trivial(&self) -> bool {
        self.filter.all() && self.search.is_none()
    }

    /// Whether it reads entries again, not only their levels.
    pub fn reads(&self) -> bool {
        !self.filter.only_levels() || self.search.is_some()
    }

    /// Works out up to `budget` bytes' worth more; whether there's more.
    pub fn step(&mut self, index: &Index, src: &Source, budget: u64) -> bool {
        if self.generation != index.generation {
            *self = Pass::new(self.filter.clone(), self.search.clone(), index);
        }
        if self.trivial() {
            return false;
        }
        let mut spent = 0;
        let closed = index.closed_chunks();
        while self.done < closed.len() {
            if spent >= budget {
                return true;
            }
            let c = closed[self.done];
            let (mut shown, mut found) = (0, 0);
            let mut bits = Vec::with_capacity(c.entries as usize);
            spent += self.work(index, src, &c, false, &mut |s, f| bits.push((s, f)));
            for (s, f) in bits {
                self.shown.push(s);
                self.found.push(f);
                shown += u64::from(s);
                found += u64::from(f);
            }
            debug_assert_eq!(self.shown.len, c.first_entry + c.entries);
            self.shown_before.push(self.shown_before[self.done] + shown);
            self.found_before.push(self.found_before[self.done] + found);
            self.done += 1;
        }
        // The chunk still growing, worked out again as it changes.
        match index.chunk(closed.len()) {
            Some(v) => {
                let key = (v.chunk.first_entry, v.chunk.entries, v.chunk.end);
                if self.tail_of != Some(key) {
                    let mut tail = Vec::new();
                    self.work(index, src, &v.chunk, v.eof, &mut |s, f| tail.push((s, f)));
                    self.tail = tail;
                    self.tail_first = v.chunk.first_entry;
                    self.tail_of = Some(key);
                }
            }
            None => {
                self.tail.clear();
                self.tail_of = None;
            }
        }
        false
    }

    /// Each entry of chunk `c`, shown and found: what it cost, in bytes.
    fn work(&mut self, index: &Index, src: &Source, c: &Chunk, eof: bool, each: &mut dyn FnMut(bool, bool)) -> u64 {
        let in_hist = !eof;
        if !self.reads() {
            for e in c.first_entry..c.first_entry + c.entries {
                each(self.filter.shows_level(index.level(e)), false);
            }
            return c.entries * 4;
        }
        let (filter, search, hist) = (&self.filter, &self.search, &mut self.hist);
        scan(src, c.offset..c.end, eof, index.grouping(), &index.rules, true, |e, text| {
            let shown = filter.keeps(e.level, e.time, text);
            let found = shown && search.as_ref().is_some_and(|m| m.is_match(text));
            if let (true, true, Some(t)) = (shown, in_hist, e.time) {
                hist.add(t, e.level);
            }
            each(shown, found);
        });
        c.end - c.offset
    }

    /// How far it has got, from 0 to 1.
    pub fn progress(&self, index: &Index) -> f64 {
        let n = index.closed_chunks().len();
        if self.trivial() || n == 0 {
            1.0
        } else {
            self.done as f64 / n as f64
        }
    }

    /// Whether it has worked out all the index has.
    pub fn finished(&self, index: &Index) -> bool {
        self.trivial() || (self.done == index.closed_chunks().len() && (index.chunk_count() == self.done || self.tail_of.is_some()))
    }

    fn shown_done(&self) -> u64 {
        self.shown_before[self.done]
    }

    /// The rows shown.
    pub fn rows(&self, index: &Index) -> u64 {
        if self.trivial() {
            return index.entries();
        }
        self.shown_done() + self.tail.iter().filter(|t| t.0).count() as u64
    }

    /// The entries found by the search, among those shown.
    pub fn found(&self) -> u64 {
        self.found_before[self.done] + self.tail.iter().filter(|t| t.1).count() as u64
    }

    pub fn is_shown(&self, entry: u64) -> bool {
        if self.trivial() {
            return true;
        }
        if entry < self.shown.len {
            return self.shown.get(entry);
        }
        entry.checked_sub(self.tail_first).and_then(|i| self.tail.get(i as usize)).is_some_and(|t| t.0)
    }

    pub fn is_found(&self, entry: u64) -> bool {
        if entry < self.found.len {
            return self.found.get(entry);
        }
        entry.checked_sub(self.tail_first).and_then(|i| self.tail.get(i as usize)).is_some_and(|t| t.1)
    }

    /// The entry on row `row`.
    pub fn entry(&self, index: &Index, row: u64) -> Option<u64> {
        if self.trivial() {
            return (row < index.entries()).then_some(row);
        }
        if row < self.shown_done() {
            let i = self.shown_before.partition_point(|&n| n <= row) - 1;
            let c = index.closed_chunks()[i];
            return self.shown.select(c.first_entry, c.first_entry + c.entries, row - self.shown_before[i]);
        }
        let k = row - self.shown_done();
        self.tail.iter().enumerate().filter(|(_, t)| t.0).nth(k as usize).map(|(i, _)| self.tail_first + i as u64)
    }

    /// The row of `entry`, or of the first shown after it: the rows' end
    /// when none is (yet).
    pub fn row_of(&self, index: &Index, entry: u64) -> u64 {
        if self.trivial() {
            return entry.min(index.entries().saturating_sub(1));
        }
        if entry < self.shown.len {
            let Some(i) = index.chunk_of(entry) else { return self.rows(index) };
            let c = index.closed_chunks()[i];
            return self.shown_before[i] + self.shown.rank(c.first_entry, entry);
        }
        let k = entry.saturating_sub(self.tail_first) as usize;
        self.shown_done() + self.tail.iter().take(k).filter(|t| t.0).count() as u64
    }

    /// The next entry found after `entry` (`forward`), or the one before;
    /// round the end.
    pub fn next_found(&self, index: &Index, entry: u64, forward: bool) -> Option<u64> {
        let (total, closed) = (index.entries(), self.found.len);
        let first = |from: u64, to: u64| self.found.next(from, to.min(closed)).or_else(|| (from.max(closed)..to).find(|&e| self.is_found(e)));
        let last = |from: u64, to: u64| (from.max(closed)..to).rev().find(|&e| self.is_found(e)).or_else(|| self.found.prev(from, to.min(closed)));
        if forward {
            first(entry + 1, total).or_else(|| first(0, (entry + 1).min(total)))
        } else {
            last(0, entry.min(total)).or_else(|| last(entry, total))
        }
    }

    pub fn memory(&self) -> usize {
        self.shown.memory() + self.found.memory() + self.shown_before.capacity() * 16
    }
}

/// The next shown entry after `entry` (`forward`), or before it, whose
/// level is an error or worse: chunks without one are skipped by their
/// counts.
pub fn next_error(index: &Index, pass: &Pass, entry: u64, forward: bool) -> Option<u64> {
    let total = index.entries();
    let mut e = entry;
    loop {
        e = if forward {
            e += 1;
            if e >= total {
                return None;
            }
            e
        } else {
            e.checked_sub(1)?
        };
        if let Some(c) = index.chunk_of(e).and_then(|i| index.closed_chunks().get(i)) {
            if c.errors == 0 {
                e = if forward { c.first_entry + c.entries - 1 } else { c.first_entry };
                continue;
            }
        }
        if index.level(e).is_error() && pass.is_shown(e) {
            return Some(e);
        }
    }
}

/// The level counts as the page says them: `e 12 error`.
pub fn counted(counts: &[u64; LEVELS]) -> [u64; 6] {
    let mut out = [0; 6];
    for (i, n) in counts.iter().enumerate() {
        out[toggle_index(Level::from_index(i))] += n;
    }
    out
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::detect::Rules;
    use std::rc::Rc;

    fn log() -> Source {
        let mut s = String::new();
        for i in 0..5000 {
            let level = ["INFO", "WARN", "ERROR", "DEBUG", "TRACE"][i % 5];
            let user = ["alice", "bob"][i % 2];
            s.push_str(&format!("2024-03-01T12:{:02}:{:02}Z {level} job={i} user={user} msg=\"tick {i}\"\n", i / 60 % 60, i % 60));
            if i % 50 == 0 {
                s.push_str("    at deep.stack(timeout.rs:1)\n");
            }
        }
        Source::bytes(s.into_bytes())
    }

    fn built(src: &Source) -> Index {
        let mut i = Index::new(Rc::new(Rules { custom: Vec::new(), year: 2024 }), src.len());
        while i.step(src, 1 << 20) {}
        i
    }

    fn pass(i: &Index, src: &Source, f: Filter, search: Option<&str>) -> Pass {
        let mut p = Pass::new(f, search.map(|s| Matcher::new(s).unwrap()), i);
        let mut steps = 0;
        while p.step(i, src, 64 * 1024) {
            steps += 1;
        }
        assert!(p.finished(i));
        assert!(steps > 0 || !p.reads() || i.closed_chunks().len() < 2);
        p
    }

    #[test]
    fn nothing_filtered_is_every_entry() {
        let src = log();
        let i = built(&src);
        let p = pass(&i, &src, Filter::default(), None);
        assert!(p.trivial());
        assert_eq!(p.rows(&i), 5000);
        assert_eq!(p.entry(&i, 4999), Some(4999));
    }

    #[test]
    fn levels_text_patterns_fields_and_times() {
        let src = log();
        let i = built(&src);
        let mut f = Filter::default();
        f.levels = [true, false, false, false, false, false];
        let p = pass(&i, &src, f.clone(), None);
        assert_eq!(p.rows(&i), 1000, "errors only");
        assert_eq!(p.entry(&i, 0), Some(2));
        assert_eq!(p.entry(&i, 999), Some(4997));
        assert_eq!(p.row_of(&i, 2), 0);
        assert_eq!(p.row_of(&i, 3), 1, "the row of the next one shown");

        let mut f = Filter::default();
        f.text = Some(Matcher::new("TIMEOUT").unwrap());
        assert_eq!(pass(&i, &src, f, None).rows(&i), 0, "a capital: its own case");
        let mut f = Filter::default();
        f.text = Some(Matcher::new("timeout").unwrap());
        assert_eq!(pass(&i, &src, f, None).rows(&i), 100, "a stack trace's lines count");

        let mut f = Filter::default();
        f.text = Some(Matcher::new(r"re:job=4\d{3} ").unwrap());
        f.exclude.push(Matcher::new("user=bob").unwrap());
        f.include.push(Matcher::new("ERROR").unwrap());
        f.include.push(Matcher::new("WARN").unwrap());
        let p = pass(&i, &src, f, None);
        // 4000..5000, alice's (even), ERROR (i%5==2) or WARN (i%5==1).
        let want = (4000..5000).filter(|n| n % 2 == 0 && (n % 5 == 2 || n % 5 == 1)).count() as u64;
        assert_eq!(p.rows(&i), want);

        let mut f = Filter::default();
        f.fields.push(Field::new("user=alice").unwrap());
        f.fields.push(Field::new("msg~tick 1").unwrap());
        let p = pass(&i, &src, f, None);
        assert_eq!(p.rows(&i), (0..5000).filter(|n| n % 2 == 0 && format!("tick {n}").starts_with("tick 1")).count() as u64);

        let mut f = Filter::default();
        f.range = Some(Range::new("12:01..12:01:59", i.time_range().map(|r| r.1), 2024).unwrap());
        let p = pass(&i, &src, f, None);
        assert_eq!(p.rows(&i), (0..5000).filter(|n| n / 60 % 60 == 1).count() as u64);
        assert!(p.hist.range.is_some());
    }

    #[test]
    fn a_search_marks_and_goes_round() {
        let src = log();
        let i = built(&src);
        let p = pass(&i, &src, Filter::default(), Some(r"re:job=4999\b"));
        assert!(!p.trivial());
        assert_eq!(p.found(), 1);
        assert_eq!(p.next_found(&i, 0, true), Some(4999));
        assert_eq!(p.next_found(&i, 4999, true), Some(4999), "round the end to itself");
        assert_eq!(p.next_found(&i, 10, false), Some(4999), "round the start");
        let p = pass(&i, &src, Filter::default(), Some(r"re:job=1\b"));
        assert_eq!(p.next_found(&i, 4000, true), Some(1));
        assert_eq!(p.next_found(&i, 4000, false), Some(1));
        assert_eq!(next_error(&i, &p, 0, true), Some(2));
        assert_eq!(next_error(&i, &p, 2, true), Some(7));
        assert_eq!(next_error(&i, &p, 7, false), Some(2));
        assert_eq!(next_error(&i, &p, 2, false), None);
    }

    #[test]
    fn a_filter_follows_a_growing_index() {
        let all = log();
        let bytes = all.read(0, all.len() as usize);
        let half = bytes.len() / 2;
        let mut src = Source::bytes(bytes[..half].to_vec());
        let mut i = built(&src);
        let mut f = Filter::default();
        f.levels = [true, false, false, false, false, false];
        let mut p = pass(&i, &src, f, None);
        let before = p.rows(&i);
        src.replace(bytes.clone());
        i.grow(src.len());
        while i.step(&src, 1 << 20) {}
        while p.step(&i, &src, 1 << 20) {}
        assert!(p.rows(&i) > before);
        assert_eq!(p.rows(&i), 1000);
        i.restart(src.len());
        while i.step(&src, 1 << 20) {}
        while p.step(&i, &src, 1 << 20) {}
        assert_eq!(p.rows(&i), 1000, "worked out again from the start");
    }

    #[test]
    fn fields_and_ranges_as_typed() {
        assert_eq!(Field::new("user = Bob").unwrap(), Field { key: "user".into(), op: Op::Is, value: "bob".into() });
        assert_eq!(Field::new("status!=200").unwrap().op, Op::IsNot);
        assert!(Field::new("nothing").is_err());
        let json = br#"{"level":"error","user":{"id":7,"name":"Bob"},"status":500}"#;
        assert!(Field::new("user.name=bob").unwrap().keeps(json));
        assert!(Field::new("user.id=7").unwrap().keeps(json));
        assert!(Field::new("status!=200").unwrap().keeps(json));
        assert!(Field::new("user.name=b*").unwrap().keeps(json));
        assert!(!Field::new("missing=x").unwrap().keeps(json));
        assert!(Field::new("missing!=x").unwrap().keeps(json));
        assert!(Field::new("err~refused").unwrap().keeps(br#"time=x level=error err="connection refused""#));

        let latest = Some(time::days(2024, 3, 1) * time::DAY + 13 * 3_600_000);
        let r = Range::new("12:00..12:30", latest, 2024).unwrap();
        assert_eq!(r.from.map(time::format).as_deref(), Some("2024-03-01 12:00:00"));
        assert_eq!(r.to.map(time::format).as_deref(), Some("2024-03-01 12:30:59.999"));
        let r = Range::new("-15m", latest, 2024).unwrap();
        assert_eq!(r.from.map(time::format).as_deref(), Some("2024-03-01 12:45:00"));
        let r = Range::new("..2024-02-29", latest, 2024).unwrap();
        assert_eq!(r.to.map(time::format).as_deref(), Some("2024-02-29 23:59:59.999"));
        assert!(r.keeps(None), "an entry with no time stays");
        assert!(Range::new("soon", latest, 2024).is_err());
        assert!(Matcher::new("re:(").is_err());
    }
}
