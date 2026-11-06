//! A log's entries: a line that starts one, and the lines after it that
//! go with it -- a stack trace, a traceback, a message on several lines
//! -- found the same way however far into the file it starts.

use crate::detect::{detect, Level, Rules, Seen};
use crate::source::{Lines, Next, Source};

/// The most lines an entry takes: past them, the next line starts
/// another, so an entry is never more than a page can show.
pub const MAX_LINES: u32 = 2000;
/// The most bytes an entry takes, the same way (a line longer than this
/// is still one line).
pub const MAX_BYTES: u64 = 1024 * 1024;

/// How a log's lines make entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Grouping {
    /// Most lines start with a time or a level: any that doesn't goes
    /// with the entry before it.
    Heads,
    /// Few do: only a line that's indented (or `Caused by:`, `... 12
    /// more`) goes with the entry before it.
    Indent,
}

impl Grouping {
    /// How the lines of `sample` make entries: by their heads when at
    /// least one in five non-empty lines has one.
    pub fn guess(sample: &[Seen]) -> Grouping {
        let heads = sample.iter().filter(|s| s.head).count();
        if heads > 0 && heads * 5 >= sample.len() {
            Grouping::Heads
        } else {
            Grouping::Indent
        }
    }
}

/// An entry: where it is, its lines, and what its first line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub offset: u64,
    /// Its bytes, the last newline among them.
    pub len: u64,
    pub lines: u32,
    pub level: Level,
    pub time: Option<i64>,
}

impl Entry {
    pub fn end(&self) -> u64 {
        self.offset + self.len
    }
}

/// Lines in, entries out: each entry comes out once the line after it
/// starts another (or `finish`).
#[derive(Debug, Clone)]
pub struct Grouper {
    grouping: Grouping,
    open: Option<Entry>,
}

impl Grouper {
    pub fn new(grouping: Grouping) -> Grouper {
        Grouper { grouping, open: None }
    }

    /// The entry still taking lines.
    pub fn open(&self) -> Option<&Entry> {
        self.open.as_ref()
    }

    /// A line, `text` its start without its newline, `len` its bytes:
    /// the entry it closed, if it starts one.
    pub fn line(&mut self, offset: u64, text: &[u8], len: u64, rules: &Rules) -> Option<Entry> {
        let grouping = self.grouping;
        let indented = text.first().is_some_and(|b| *b == b' ' || *b == b'\t');
        if let Some(open) = &mut self.open {
            if open.lines < MAX_LINES && open.len + len <= MAX_BYTES {
                // An indented or empty line goes with it without being read.
                if indented || text.is_empty() {
                    open.lines += 1;
                    open.len += len;
                    if open.level == Level::Other && indented {
                        open.level = detect(text, rules).level;
                    }
                    return None;
                }
                let seen = detect(text, rules);
                if continues(grouping, text, &seen) {
                    open.lines += 1;
                    open.len += len;
                    if open.level == Level::Other {
                        open.level = seen.level;
                    }
                    // A journal export's time is a field after its head.
                    if open.time.is_none() {
                        open.time = seen.time;
                    }
                    return None;
                }
                return self.open.replace(Entry { offset, len, lines: 1, level: seen.level, time: seen.time });
            }
        }
        let seen = detect(text, rules);
        self.open.replace(Entry { offset, len, lines: 1, level: seen.level, time: seen.time })
    }

    /// The entry still open, closed.
    pub fn finish(&mut self) -> Option<Entry> {
        self.open.take()
    }
}

/// Whether a line that isn't indented goes with the entry before it.
fn continues(grouping: Grouping, text: &[u8], seen: &Seen) -> bool {
    match grouping {
        Grouping::Heads => !seen.head,
        Grouping::Indent => text.starts_with(b"Caused by: ") || (text.starts_with(b"...") && text.ends_with(b"more")),
    }
}

/// The entries from `from` to `to`, the first starting at `from` (an
/// entry's start); `eof`: a last line without its newline counts. `each`
/// gets each entry and, with `text`, its lines' starts joined by
/// newlines -- what the filters look at.
pub fn scan(src: &Source, span: std::ops::Range<u64>, eof: bool, grouping: Grouping, rules: &Rules, text: bool, mut each: impl FnMut(&Entry, &[u8])) {
    let mut lines = Lines::new(span.start, span.end);
    let mut g = Grouper::new(grouping);
    let mut buf: Vec<u8> = Vec::new();
    while let Next::Line(l) = lines.next(src, eof) {
        if let Some(done) = g.line(l.offset, l.text, l.len, rules) {
            each(&done, &buf);
            buf.clear();
        } else if text && !buf.is_empty() {
            buf.push(b'\n');
        }
        if text {
            buf.extend_from_slice(l.text);
        }
    }
    if let Some(done) = g.finish() {
        each(&done, &buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str, grouping: Grouping) -> Vec<(u32, Level, String)> {
        let src = Source::bytes(text.as_bytes().to_vec());
        let mut out = Vec::new();
        scan(&src, 0..src.len(), true, grouping, &Rules::default(), true, |e, t| {
            out.push((e.lines, e.level, String::from_utf8_lossy(t).lines().next().unwrap_or("").to_string()));
        });
        out
    }

    #[test]
    fn a_stack_trace_goes_with_the_entry_it_follows() {
        let log = include_str!("samples/java.log");
        let e = entries(log, Grouping::Heads);
        assert_eq!(e.len(), 4, "{e:#?}");
        assert_eq!((e[1].0, e[1].1), (7, Level::Error));
        assert!(e[1].2.contains("Request failed"));
        assert_eq!(e[2].1, Level::Warn);
    }

    #[test]
    fn a_python_traceback_with_its_exception_line() {
        let e = entries(include_str!("samples/python.log"), Grouping::Heads);
        let tb = e.iter().find(|e| e.2.contains("could not load")).unwrap();
        assert_eq!((tb.0, tb.1), (6, Level::Error), "{e:#?}");
    }

    #[test]
    fn without_heads_only_indented_lines_go_together() {
        let text = "plain one\nplain two\n    indented\nCaused by: x\n\tat y\n... 3 more\nplain three\n";
        let e = entries(text, Grouping::Indent);
        assert_eq!(e.iter().map(|e| e.0).collect::<Vec<_>>(), [1, 5, 1]);
        assert_eq!(e[1].1, Level::Error, "Caused by says it went wrong");
    }

    #[test]
    fn an_entry_without_a_level_takes_its_trouble_from_the_lines_after() {
        let text = "2024-03-01 12:00:03 worker 3 stopped\nTraceback (most recent call last):\n  File \"w.py\", line 1\nValueError: boom\nthread 'main' panicked at src/main.rs:2:5:\nboom\n2024-03-01 12:00:04 INFO next\n";
        let e = entries(text, Grouping::Heads);
        assert_eq!((e[0].0, e[0].1), (4, Level::Error));
        assert_eq!((e[1].0, e[1].1), (2, Level::Fatal), "a panic is an entry of its own");
        assert_eq!(e[2].1, Level::Info);
    }

    #[test]
    fn an_entry_never_grows_past_its_limits() {
        let mut text = String::from("2024-03-01 12:00:03 ERROR huge\n");
        for _ in 0..(MAX_LINES + 10) {
            text.push_str("  at x\n");
        }
        let e = entries(&text, Grouping::Heads);
        assert_eq!(e.iter().map(|e| e.0).collect::<Vec<_>>(), [MAX_LINES, 11]);
    }

    #[test]
    fn grouping_is_guessed_from_how_many_lines_have_heads() {
        let r = Rules::default();
        let lines = ["2024-03-01 12:00:03 a", "  b", "c", "d", "e"].map(|l| detect(l.as_bytes(), &r));
        assert_eq!(Grouping::guess(&lines), Grouping::Heads);
        let lines = ["2024-03-01 12:00:03 a", "b", "c", "d", "e", "f"].map(|l| detect(l.as_bytes(), &r));
        assert_eq!(Grouping::guess(&lines), Grouping::Indent);
    }
}
