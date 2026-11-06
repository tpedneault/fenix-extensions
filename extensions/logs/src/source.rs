//! Where a log's bytes are: a file under `/project` read where it is, a
//! block at a time, or one elsewhere that Fenix read whole for it -- and
//! its lines, read a piece at a time however long they are.

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::detect::CAP;

/// What's read at once for the page, and kept.
const BLOCK: u64 = 64 * 1024;
/// How many blocks are kept: 2 MiB.
const KEPT: usize = 32;
/// What the line reader reads at once.
pub const PIECE: usize = 1024 * 1024;

/// The bytes of a log.
pub struct Source {
    input: Input,
    cache: RefCell<Cache>,
}

enum Input {
    /// A file under `/project`, and its length when last looked at.
    Path(String, u64),
    /// A file read whole.
    Bytes(Vec<u8>),
}

#[derive(Default)]
struct Cache {
    file: Option<File>,
    /// Blocks by their index, the one used last at the end.
    blocks: Vec<(u64, Vec<u8>)>,
    error: Option<String>,
}

impl Source {
    /// A file under `/project`, read as it's needed.
    pub fn path(path: &str) -> Result<Source, String> {
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        Ok(Source { input: Input::Path(path.to_string(), len), cache: RefCell::default() })
    }

    /// Bytes read whole.
    pub fn bytes(bytes: Vec<u8>) -> Source {
        Source { input: Input::Bytes(bytes), cache: RefCell::default() }
    }

    pub fn len(&self) -> u64 {
        match &self.input {
            Input::Path(_, len) => *len,
            Input::Bytes(b) => b.len() as u64,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether it's read where it is, so it can be read again cheaply.
    pub fn in_place(&self) -> bool {
        matches!(self.input, Input::Path(..))
    }

    /// Why the last read failed, if one did.
    pub fn error(&self) -> Option<String> {
        self.cache.borrow().error.clone()
    }

    /// The file's length looked at again (`F`'s poll): what's kept is
    /// let go of when it changed, so a file rotated under the same name
    /// is read afresh.
    pub fn refresh(&mut self) -> Result<u64, String> {
        let Input::Path(path, len) = &mut self.input else { return Ok(self.len()) };
        let now = std::fs::metadata(&*path).map_err(|e| e.to_string())?.len();
        if now != *len {
            *len = now;
            *self.cache.borrow_mut() = Cache::default();
        } else {
            // The same length can still be another file: read through a
            // fresh handle from here on.
            self.cache.borrow_mut().file = None;
        }
        Ok(now)
    }

    /// Bytes read whole replaced by `bytes`, read again.
    pub fn replace(&mut self, bytes: Vec<u8>) {
        self.input = Input::Bytes(bytes);
        *self.cache.borrow_mut() = Cache::default();
    }

    /// `len` bytes from `at`, or as many as there are there.
    pub fn read(&self, at: u64, len: usize) -> Vec<u8> {
        let end = at.saturating_add(len as u64).min(self.len());
        if at >= end {
            return Vec::new();
        }
        match &self.input {
            Input::Bytes(b) => b[at as usize..end as usize].to_vec(),
            Input::Path(path, _) if end - at > BLOCK => {
                let mut out = Vec::new();
                self.read_into(path, at, (end - at) as usize, &mut out);
                out
            }
            Input::Path(path, _) => {
                let mut out = Vec::with_capacity((end - at) as usize);
                let mut pos = at;
                while pos < end {
                    let index = pos / BLOCK;
                    let Some(n) = self.with_block(path, index, |b| {
                        let from = (pos - index * BLOCK) as usize;
                        let to = ((end - index * BLOCK) as usize).min(b.len());
                        out.extend_from_slice(b.get(from..to).unwrap_or_default());
                        to.saturating_sub(from)
                    }) else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    pos += n as u64;
                }
                out
            }
        }
    }

    /// Up to `len` bytes from `at` added to `out`, past the blocks kept:
    /// what the index and the filters read.
    pub fn read_onto(&self, at: u64, len: usize, out: &mut Vec<u8>) -> usize {
        let end = at.saturating_add(len as u64).min(self.len());
        if at >= end {
            return 0;
        }
        match &self.input {
            Input::Bytes(b) => {
                out.extend_from_slice(&b[at as usize..end as usize]);
                (end - at) as usize
            }
            Input::Path(path, _) => self.read_into(path, at, (end - at) as usize, out),
        }
    }

    fn with_block<T>(&self, path: &str, index: u64, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.blocks.iter().position(|(n, _)| *n == index) {
            let kept = cache.blocks.remove(i);
            cache.blocks.push(kept);
        } else {
            let len = BLOCK.min(self.len().saturating_sub(index * BLOCK)) as usize;
            let mut block = Vec::with_capacity(len);
            read_at(&mut cache, path, index * BLOCK, len, &mut block)?;
            if cache.blocks.len() >= KEPT {
                cache.blocks.remove(0);
            }
            cache.blocks.push((index, block));
        }
        cache.blocks.last().map(|(_, b)| f(b))
    }

    fn read_into(&self, path: &str, at: u64, len: usize, out: &mut Vec<u8>) -> usize {
        read_at(&mut self.cache.borrow_mut(), path, at, len, out).unwrap_or(0)
    }
}

/// `len` bytes of `path` from `at` onto `out`, the file opened once.
fn read_at(cache: &mut Cache, path: &str, at: u64, len: usize, out: &mut Vec<u8>) -> Option<usize> {
    if cache.file.is_none() {
        match File::open(path) {
            Ok(f) => cache.file = Some(f),
            Err(e) => {
                cache.error = Some(e.to_string());
                return None;
            }
        }
    }
    let file = cache.file.as_mut()?;
    let before = out.len();
    let read = file.seek(SeekFrom::Start(at)).and_then(|_| file.take(len as u64).read_to_end(out));
    match read {
        Ok(_) => {
            let n = out.len() - before;
            cache.error = (n < len).then(|| "the file is shorter than it was: R reads it again".into());
            Some(n)
        }
        Err(e) => {
            cache.error = Some(e.to_string());
            cache.file = None;
            None
        }
    }
}

/// A line as the reader gives it: where it starts, its first [`CAP`]
/// bytes without the newline (or `\r\n`), and its length with them.
pub struct Line<'a> {
    pub offset: u64,
    pub text: &'a [u8],
    pub len: u64,
}

/// A source's lines from one place to another, read [`PIECE`] at a time:
/// a line longer than [`CAP`] is skipped through to its end, only its
/// start kept. It can stop and go on later (the index's steps), and a
/// last line with no newline is a line only if it's asked to be (`eof`).
pub struct Lines {
    /// Where `buf[0]` is in the file.
    pos: u64,
    buf: Vec<u8>,
    /// What of `buf` is read already.
    at: usize,
    /// How far it may read.
    pub limit: u64,
    /// A long line being skipped through: where it starts, and its start.
    long: Option<u64>,
    long_head: Vec<u8>,
    /// How many bytes it has read from the source.
    pub read: u64,
}

/// What the reader has next.
pub enum Next<'a> {
    Line(Line<'a>),
    /// Nothing more, short of `limit`: or a last line without its
    /// newline, which is left where it is.
    End,
}

impl Lines {
    pub fn new(from: u64, limit: u64) -> Lines {
        Lines { pos: from, buf: Vec::new(), at: 0, limit, long: None, long_head: Vec::new(), read: 0 }
    }

    /// Where the next line starts: the first byte not given yet.
    pub fn offset(&self) -> u64 {
        self.long.unwrap_or(self.pos + self.at as u64)
    }

    /// Whether there are bytes after the last line given (a line with no
    /// newline yet).
    pub fn pending(&self) -> u64 {
        self.limit.saturating_sub(self.offset())
    }

    /// The next line. `eof`: the last line counts though it has no
    /// newline.
    pub fn next(&mut self, src: &Source, eof: bool) -> Next<'_> {
        loop {
            if let Some(start) = self.long {
                if let Some(i) = memchr::memchr(b'\n', &self.buf[self.at..]) {
                    let end = self.pos + (self.at + i + 1) as u64;
                    self.at += i + 1;
                    self.long = None;
                    return Next::Line(Line { offset: start, text: strip_cr(&self.long_head), len: end - start });
                }
                self.at = self.buf.len();
                if !self.fill(src) {
                    if eof {
                        self.long = None;
                        let end = self.pos + self.buf.len() as u64;
                        return Next::Line(Line { offset: start, text: &self.long_head, len: end - start });
                    }
                    return Next::End;
                }
                continue;
            }
            if let Some(i) = memchr::memchr(b'\n', &self.buf[self.at..]) {
                let s = self.at;
                self.at += i + 1;
                let text = &self.buf[s..s + i.min(CAP)];
                return Next::Line(Line { offset: self.pos + s as u64, text: if i <= CAP { strip_cr(text) } else { text }, len: i as u64 + 1 });
            }
            let rest = self.buf.len() - self.at;
            if rest > CAP {
                // A long line: its start kept, the rest read through.
                self.long = Some(self.pos + self.at as u64);
                self.long_head.clear();
                self.long_head.extend_from_slice(&self.buf[self.at..self.at + CAP]);
                self.at = self.buf.len();
                continue;
            }
            if !self.fill(src) {
                if eof && rest > 0 {
                    let s = self.at;
                    self.at = self.buf.len();
                    return Next::Line(Line { offset: self.pos + s as u64, text: &self.buf[s..], len: rest as u64 });
                }
                return Next::End;
            }
        }
    }

    /// More read after what's left: false when there's nothing more.
    fn fill(&mut self, src: &Source) -> bool {
        self.buf.drain(..self.at);
        self.pos += self.at as u64;
        self.at = 0;
        let from = self.pos + self.buf.len() as u64;
        let want = (self.limit.saturating_sub(from)).min(PIECE as u64) as usize;
        if want == 0 {
            return false;
        }
        let n = src.read_onto(from, want, &mut self.buf);
        self.read += n as u64;
        n > 0
    }
}

fn strip_cr(s: &[u8]) -> &[u8] {
    s.strip_suffix(b"\r").unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(src: &Source, eof: bool) -> Vec<(u64, String, u64)> {
        let mut lines = Lines::new(0, src.len());
        let mut out = Vec::new();
        while let Next::Line(l) = lines.next(src, eof) {
            out.push((l.offset, String::from_utf8_lossy(&l.text[..l.text.len().min(10)]).into_owned(), l.len));
        }
        out
    }

    #[test]
    fn lines_with_and_without_a_last_newline() {
        let src = Source::bytes(b"one\r\ntwo\n\nthree".to_vec());
        assert_eq!(all(&src, false), [(0, "one".into(), 5), (5, "two".into(), 4), (9, "".into(), 1)]);
        assert_eq!(all(&src, true).last(), Some(&(10, "three".into(), 5)));
    }

    #[test]
    fn a_long_line_keeps_its_start_and_is_read_through() {
        let mut bytes = vec![b'x'; CAP * 3 + 5];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"after\n");
        let src = Source::bytes(bytes);
        let mut lines = Lines::new(0, src.len());
        let Next::Line(l) = lines.next(&src, false) else { panic!() };
        assert_eq!((l.offset, l.text.len(), l.len), (0, CAP, CAP as u64 * 3 + 6));
        let Next::Line(l) = lines.next(&src, false) else { panic!() };
        assert_eq!((l.offset, l.text), (CAP as u64 * 3 + 6, &b"after"[..]));
        assert!(matches!(lines.next(&src, false), Next::End));
    }

    #[test]
    fn a_file_is_read_in_blocks_and_again_when_it_changes() {
        let dir = std::env::temp_dir().join("fenix-logs-source");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.log");
        let text: Vec<u8> = (0..200_000u32).flat_map(|i| format!("{i}\n").into_bytes()).collect();
        std::fs::write(&path, &text).unwrap();
        let mut src = Source::path(path.to_str().unwrap()).unwrap();
        assert_eq!(src.len(), text.len() as u64);
        assert_eq!(src.read(BLOCK - 2, 4), text[BLOCK as usize - 2..BLOCK as usize + 2]);
        assert_eq!(src.read(10, 300_000), text[10..300_010]);
        std::fs::write(&path, b"short\n").unwrap();
        assert_eq!(src.refresh().unwrap(), 6);
        assert_eq!(src.read(0, 100), b"short\n");
    }
}
