//! A log's index, built a step at a time: its entries found as the file
//! is read a few MiB at a step, kept as chunks of about 64 KiB -- where
//! each starts, its first entry and line, its levels and times -- and
//! each entry's level in half a byte. That's all that's kept: an entry's
//! place and text are read again from its chunk when they're shown, so a
//! log of gigabytes takes a few MiB.
//!
//! The index grows with the file (`grow`, as `F` follows it), and is
//! started again when the file was truncated or replaced (`changed`).

use std::rc::Rc;

use crate::detect::{detect, Level, Rules, Seen, CAP, LEVELS};
use crate::entry::{Entry, Grouper, Grouping};
use crate::hist::Histogram;
use crate::source::{Lines, Next, Source};

/// A chunk closes at the first entry's end past this many bytes.
pub const CHUNK: u64 = 64 * 1024;
/// What a step reads, about: a few milliseconds' work, in the sandbox
/// too.
pub const STEP: u64 = 256 * 1024;
/// How much of the file's start the grouping is guessed from.
const SAMPLE: u64 = 256 * 1024;
/// How many of its first bytes tell a file from the one it replaced.
const HEAD: usize = 256;

/// A run of entries, a chunk of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Chunk {
    pub offset: u64,
    /// Where its last entry ends.
    pub end: u64,
    pub first_entry: u64,
    pub entries: u64,
    pub first_line: u64,
    pub lines: u64,
    /// Its errors and worse, so a jump to the next skips it when it has none.
    pub errors: u64,
    /// When its first and last entries with a time say.
    pub times: Option<(i64, i64)>,
}

/// A chunk as the page reads it: a closed one, or the one still growing
/// (`eof`: read to the file's end, a last line without its newline in
/// it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkView {
    pub chunk: Chunk,
    pub eof: bool,
}

/// Levels, half a byte each.
#[derive(Debug, Clone, Default)]
struct Levels {
    bytes: Vec<u8>,
    len: u64,
}

impl Levels {
    fn push(&mut self, l: Level) {
        if self.len.is_multiple_of(2) {
            self.bytes.push(l as u8);
        } else if let Some(b) = self.bytes.last_mut() {
            *b |= (l as u8) << 4;
        }
        self.len += 1;
    }

    fn get(&self, i: u64) -> Level {
        let b = self.bytes[(i / 2) as usize];
        Level::from_index(if i.is_multiple_of(2) { b & 15 } else { b >> 4 } as usize)
    }
}

/// What's built from the entries closed so far.
#[derive(Debug, Clone, Default)]
struct Built {
    chunks: Vec<Chunk>,
    /// The chunk being filled.
    cur: Chunk,
    levels: Levels,
    counts: [u64; LEVELS],
    lines: u64,
    hist: Histogram,
}

impl Built {
    fn close(&mut self, e: Entry) {
        self.levels.push(e.level);
        self.counts[e.level.index()] += 1;
        self.lines += e.lines as u64;
        let c = &mut self.cur;
        c.entries += 1;
        c.lines += e.lines as u64;
        c.errors += u64::from(e.level.is_error());
        c.end = e.end();
        if let Some(t) = e.time {
            c.times = Some(c.times.map_or((t, t), |(a, z)| (a.min(t), z.max(t))));
            self.hist.add(t, e.level);
        }
        if c.end - c.offset >= CHUNK {
            let done = *c;
            self.chunks.push(done);
            self.cur = Chunk {
                offset: done.end,
                end: done.end,
                first_entry: done.first_entry + done.entries,
                first_line: done.first_line + done.lines,
                ..Chunk::default()
            };
        }
    }
}

pub struct Index {
    pub rules: Rc<Rules>,
    pub grouping: Option<Grouping>,
    built: Built,
    reader: Lines,
    grouper: Grouper,
    /// The file's length it reads to.
    pub len: u64,
    head: Vec<u8>,
    /// Whether it has read all there is.
    pub done: bool,
    /// Once done: the entry still open, and a last line without its
    /// newline -- shown, but read again as the file grows.
    tail: Vec<Entry>,
    /// Bumped each time it starts again, so what's built on it does too.
    pub generation: u64,
}

impl Index {
    pub fn new(rules: Rc<Rules>, len: u64) -> Index {
        Index {
            rules,
            grouping: None,
            built: Built::default(),
            reader: Lines::new(0, len),
            grouper: Grouper::new(Grouping::Heads),
            len,
            head: Vec::new(),
            done: len == 0,
            tail: Vec::new(),
            generation: 0,
        }
    }

    /// Started again, on a file of `len` bytes.
    pub fn restart(&mut self, len: u64) {
        let generation = self.generation + 1;
        *self = Index::new(self.rules.clone(), len);
        self.generation = generation;
    }

    /// Whether the file isn't the one indexed any more: shorter than it
    /// was, or starting otherwise -- truncated, or rotated and written
    /// again.
    pub fn changed(&self, src: &Source) -> bool {
        src.len() < self.len || (!self.head.is_empty() && src.read(0, self.head.len()) != self.head)
    }

    /// The file grew to `len`: there's more to read.
    pub fn grow(&mut self, len: u64) {
        if len > self.len {
            self.len = len;
            self.reader.limit = len;
            self.done = false;
            // Read again from where the open entry is, with what came.
            self.tail.clear();
        }
    }

    /// How far it has read, from 0 to 1.
    pub fn progress(&self) -> f64 {
        if self.len == 0 || self.done {
            1.0
        } else {
            self.reader.offset() as f64 / self.len as f64
        }
    }

    /// Reads up to `budget` bytes more; whether there's more to read.
    pub fn step(&mut self, src: &Source, budget: u64) -> bool {
        if self.done {
            return false;
        }
        if self.grouping.is_none() {
            let sample = self.sample(src);
            if sample.is_empty() && self.len < SAMPLE.min(CAP as u64) {
                // No line yet: what's there is shown as it is, the grouping
                // left to when there are lines.
                self.finish(src);
                return false;
            }
            // A journal export's entries are a field a line, one in twenty
            // or so its head.
            let g = if src.read(0, 9) == b"__CURSOR=" { Grouping::Heads } else { Grouping::guess(&sample) };
            self.grouping = Some(g);
            self.grouper = Grouper::new(g);
            self.head = src.read(0, HEAD);
        }
        let start = self.reader.offset();
        loop {
            if self.reader.offset() - start >= budget {
                return true;
            }
            match self.reader.next(src, false) {
                Next::Line(l) => {
                    if let Some(e) = self.grouper.line(l.offset, l.text, l.len, &self.rules) {
                        self.built.close(e);
                    }
                }
                Next::End => break,
            }
        }
        self.finish(src);
        false
    }

    /// The first lines' heads, for the grouping.
    fn sample(&self, src: &Source) -> Vec<Seen> {
        let mut lines = Lines::new(0, self.len.min(SAMPLE));
        let mut out = Vec::new();
        while let Next::Line(l) = lines.next(src, false) {
            if !l.text.is_empty() {
                out.push(detect(l.text, &self.rules));
            }
            if out.len() >= 2000 {
                break;
            }
        }
        out
    }

    /// At the end of what's there: the open entry, and a last line
    /// without its newline, as they'd be if the file ended here.
    fn finish(&mut self, src: &Source) {
        self.done = true;
        self.tail.clear();
        let mut g = self.grouper.clone();
        let from = self.reader.offset();
        if self.len > from {
            let text = src.read(from, (self.len - from).min(CAP as u64) as usize);
            let text = text.strip_suffix(b"\r").unwrap_or(&text);
            if let Some(e) = g.line(from, text, self.len - from, &self.rules) {
                self.tail.push(e);
            }
        }
        self.tail.extend(g.finish());
    }

    /// How the entries are found: by their heads until it's guessed.
    pub fn grouping(&self) -> Grouping {
        self.grouping.unwrap_or(Grouping::Heads)
    }

    /// The entries found.
    pub fn entries(&self) -> u64 {
        self.built.levels.len + self.tail.len() as u64
    }

    /// The entries that won't change: those of the closed chunks.
    pub fn closed_entries(&self) -> u64 {
        self.built.cur.first_entry
    }

    pub fn lines(&self) -> u64 {
        self.built.lines + self.tail.iter().map(|e| e.lines as u64).sum::<u64>()
    }

    pub fn level(&self, entry: u64) -> Level {
        let closed = self.built.levels.len;
        if entry < closed {
            self.built.levels.get(entry)
        } else {
            self.tail.get((entry - closed) as usize).map_or(Level::Other, |e| e.level)
        }
    }

    /// Entries at each level.
    pub fn counts(&self) -> [u64; LEVELS] {
        let mut c = self.built.counts;
        for e in &self.tail {
            c[e.level.index()] += 1;
        }
        c
    }

    /// When the entries are, the open ones' too.
    pub fn hist(&self) -> Histogram {
        let mut h = self.built.hist.clone();
        for e in &self.tail {
            if let Some(t) = e.time {
                h.add(t, e.level);
            }
        }
        h
    }

    /// The first and last time of the entries, as far as they're read.
    pub fn time_range(&self) -> Option<(i64, i64)> {
        let mut r = self.built.hist.range;
        for t in self.tail.iter().filter_map(|e| e.time) {
            r = Some(r.map_or((t, t), |(a, z)| (a.min(t), z.max(t))));
        }
        r
    }

    /// The closed chunks.
    pub fn closed_chunks(&self) -> &[Chunk] {
        &self.built.chunks
    }

    pub fn chunk_count(&self) -> usize {
        let c = &self.built.cur;
        self.built.chunks.len() + usize::from(c.entries > 0 || !self.tail.is_empty())
    }

    pub fn chunk(&self, i: usize) -> Option<ChunkView> {
        if let Some(c) = self.built.chunks.get(i) {
            return Some(ChunkView { chunk: *c, eof: false });
        }
        if i != self.built.chunks.len() || self.chunk_count() == self.built.chunks.len() {
            return None;
        }
        let mut c = self.built.cur;
        c.entries += self.tail.len() as u64;
        c.lines += self.tail.iter().map(|e| e.lines as u64).sum::<u64>();
        if self.done {
            c.end = self.len;
        }
        Some(ChunkView { chunk: c, eof: self.done })
    }

    /// The chunk entry `entry` is in.
    pub fn chunk_of(&self, entry: u64) -> Option<usize> {
        if entry >= self.entries() {
            return None;
        }
        if entry >= self.built.cur.first_entry {
            return Some(self.built.chunks.len());
        }
        Some(self.built.chunks.partition_point(|c| c.first_entry + c.entries <= entry))
    }

    /// About how much memory it holds, in bytes.
    pub fn memory(&self) -> usize {
        self.built.chunks.capacity() * std::mem::size_of::<Chunk>() + self.built.levels.bytes.capacity() + crate::source::PIECE + CAP + 512 * 12
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::scan;

    fn index(src: &Source) -> Index {
        let mut i = Index::new(Rc::new(Rules::default()), src.len());
        while i.step(src, 4096) {}
        i
    }

    /// Every entry, read again chunk by chunk as the page reads them.
    fn rescan(i: &Index, src: &Source) -> Vec<Entry> {
        let mut out = Vec::new();
        for n in 0..i.chunk_count() {
            let v = i.chunk(n).unwrap();
            let before = out.len() as u64;
            scan(src, v.chunk.offset..v.chunk.end, v.eof, i.grouping(), &i.rules, false, |e, _| out.push(*e));
            assert_eq!(out.len() as u64 - before, v.chunk.entries, "chunk {n}");
            assert_eq!(before, v.chunk.first_entry);
        }
        out
    }

    fn log(n: usize) -> Vec<u8> {
        let mut s = String::new();
        for i in 0..n {
            let level = ["INFO", "WARN", "ERROR", "DEBUG"][i % 4];
            s.push_str(&format!("2024-03-01 12:{:02}:{:02} {level} line {i}\n", i / 60 % 60, i % 60));
            if i % 7 == 0 {
                s.push_str("    at stack.frame(x.rs:1)\n    at stack.frame(y.rs:2)\n");
            }
        }
        s.into_bytes()
    }

    #[test]
    fn steps_find_every_entry_and_chunks_find_them_again() {
        let src = Source::bytes(log(20_000));
        let i = index(&src);
        assert!(i.done);
        assert_eq!(i.entries(), 20_000);
        assert_eq!(i.lines(), 20_000 + 2 * 2858);
        assert!(i.chunk_count() > 10, "{}", i.chunk_count());
        let again = rescan(&i, &src);
        assert_eq!(again.len(), 20_000);
        for (n, e) in again.iter().enumerate() {
            assert_eq!(e.level, i.level(n as u64), "entry {n}");
        }
        assert_eq!(again[7].lines, 3);
        assert_eq!(i.counts()[Level::Error.index()], 5000);
        assert_eq!(i.chunk_of(0), Some(0));
        assert_eq!(i.chunk_of(19_999), Some(i.chunk_count() - 1));
        let c = i.chunk(3).unwrap().chunk;
        assert_eq!(i.chunk_of(c.first_entry), Some(3));
        assert_eq!(i.chunk_of(c.first_entry - 1), Some(2));
    }

    #[test]
    fn a_step_reads_no_more_than_its_budget() {
        let src = Source::bytes(log(100_000));
        let mut i = Index::new(Rc::new(Rules::default()), src.len());
        let mut steps = 0;
        let mut last = 0.0;
        while i.step(&src, 256 * 1024) {
            steps += 1;
            assert!(i.progress() > last);
            last = i.progress();
        }
        assert!(steps >= (src.len() / (256 * 1024)) as usize - 1, "{steps}");
        assert_eq!(i.entries(), 100_000);
    }

    #[test]
    fn a_file_appended_to_is_read_on_from_where_it_was() {
        let all = log(3000);
        let cut = all.len() / 2 + 7; // in the middle of a line
        let mut src = Source::bytes(all[..cut].to_vec());
        let mut i = index(&src);
        let partial = i.entries();
        assert!(i.done);
        let shown = rescan(&i, &src);
        assert_eq!(shown.len() as u64, partial, "the half line is shown as an entry");
        src.replace(all.clone());
        assert!(!i.changed(&src));
        i.grow(src.len());
        while i.step(&src, 4096) {}
        assert_eq!(i.entries(), 3000);
        assert_eq!(rescan(&i, &src).len(), 3000);
        let fresh = index(&src);
        assert_eq!(i.counts(), fresh.counts());
        assert_eq!(i.lines(), fresh.lines());
    }

    #[test]
    fn a_truncated_or_rotated_file_is_read_again() {
        let mut src = Source::bytes(log(500));
        let i = index(&src);
        src.replace(log(10));
        assert!(i.changed(&src), "shorter");
        let mut other = b"2024-03-02 00:00:00 INFO a new file\n".to_vec();
        other.extend(log(1000));
        src.replace(other);
        assert!(i.changed(&src), "longer, but another file");
        let mut i = i;
        i.restart(src.len());
        assert_eq!(i.generation, 1);
        while i.step(&src, 1 << 20) {}
        assert_eq!(i.entries(), 1001);
    }

    #[test]
    fn a_rotated_file_on_disk_is_noticed() {
        let dir = std::env::temp_dir().join("fenix-logs-index");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        std::fs::write(&path, log(2000)).unwrap();
        let mut src = Source::path(path.to_str().unwrap()).unwrap();
        let mut i = index(&src);
        assert_eq!(i.entries(), 2000);
        // Appended to.
        let mut more = log(2000);
        more.extend_from_slice(b"2024-03-01 13:00:00 ERROR appended\n");
        std::fs::write(&path, &more).unwrap();
        src.refresh().unwrap();
        assert!(!i.changed(&src));
        i.grow(src.len());
        while i.step(&src, 1 << 20) {}
        assert_eq!((i.entries(), i.level(2000)), (2001, Level::Error));
        // Rotated: moved away, and a new one started under its name.
        std::fs::rename(&path, dir.join("app.log.1")).unwrap();
        std::fs::write(&path, b"2024-03-02 00:00:00 WARN fresh\n").unwrap();
        src.refresh().unwrap();
        assert!(i.changed(&src));
        i.restart(src.len());
        while i.step(&src, 1 << 20) {}
        assert_eq!((i.entries(), i.level(0)), (1, Level::Warn));
    }

    #[test]
    fn an_empty_file_and_one_without_a_newline() {
        let src = Source::bytes(Vec::new());
        let i = index(&src);
        assert_eq!((i.entries(), i.chunk_count()), (0, 0));
        let src = Source::bytes(b"2024-03-01 12:00:00 ERROR half".to_vec());
        let i = index(&src);
        assert_eq!(i.entries(), 1);
        assert_eq!(i.level(0), Level::Error);
        assert_eq!(rescan(&i, &src).len(), 1);
    }

    #[test]
    fn levels_take_half_a_byte() {
        let mut l = Levels::default();
        for i in 0..11 {
            l.push(Level::from_index(i % LEVELS));
        }
        assert_eq!(l.bytes.len(), 6);
        assert_eq!((0..11).map(|i| l.get(i).index()).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3]);
    }
}
