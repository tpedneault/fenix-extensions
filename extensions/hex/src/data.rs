//! The bytes a page shows: a file under `/project` read where it is, a
//! block at a time as it's shown, or one elsewhere that Fenix read whole
//! for it -- and searches through them.

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// What's read at once for the dump, and kept.
const BLOCK: u64 = 64 * 1024;
/// How many blocks are kept: 2 MiB.
const KEPT: usize = 32;
/// What a search reads at once.
const CHUNK: u64 = 1024 * 1024;

/// Where the bytes are.
pub enum Input {
    /// A file under `/project`, and its length.
    Path(String, u64),
    /// A file read whole.
    Bytes(Vec<u8>),
}

/// A search's pattern: a byte, or `None` for any (`??`).
pub type Pattern = [Option<u8>];

pub struct Data {
    input: Input,
    cache: RefCell<Cache>,
}

#[derive(Default)]
struct Cache {
    file: Option<File>,
    /// Blocks by their index, the one used last at the end.
    blocks: Vec<(u64, Vec<u8>)>,
    /// Why the last read came short.
    error: Option<String>,
}

impl Data {
    pub fn new(input: Input) -> Data {
        Data { input, cache: RefCell::default() }
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

    /// Why a read came short, if one did.
    pub fn error(&self) -> Option<String> {
        self.cache.borrow().error.clone()
    }

    /// `len` bytes from `at`, or as many as the file has there.
    pub fn read(&self, at: u64, len: usize) -> Vec<u8> {
        let end = at.saturating_add(len as u64).min(self.len());
        if at >= end {
            return Vec::new();
        }
        match &self.input {
            Input::Bytes(b) => b[at as usize..end as usize].to_vec(),
            Input::Path(path, _) if end - at > BLOCK => self.read_file(path, at, (end - at) as usize),
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

    /// The byte at `at`.
    pub fn byte(&self, at: u64) -> Option<u8> {
        self.read(at, 1).first().copied()
    }

    /// Block `index` of the file, read if it isn't kept.
    fn with_block<T>(&self, path: &str, index: u64, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.blocks.iter().position(|(n, _)| *n == index) {
            let kept = cache.blocks.remove(i);
            cache.blocks.push(kept);
        } else {
            let len = BLOCK.min(self.len().saturating_sub(index * BLOCK)) as usize;
            let block = read_at(&mut cache, path, index * BLOCK, len)?;
            if cache.blocks.len() >= KEPT {
                cache.blocks.remove(0);
            }
            cache.blocks.push((index, block));
        }
        cache.blocks.last().map(|(_, b)| f(b))
    }

    /// A long read, past the blocks kept.
    fn read_file(&self, path: &str, at: u64, len: usize) -> Vec<u8> {
        read_at(&mut self.cache.borrow_mut(), path, at, len).unwrap_or_default()
    }

    /// Where `pattern` is first found from `from` on (`forward`), or
    /// last found before `from` -- going round the file's end.
    pub fn find(&self, pattern: &Pattern, from: u64, forward: bool) -> Option<u64> {
        let n = pattern.len() as u64;
        if n == 0 || n > self.len() {
            return None;
        }
        // The starts a match can have.
        let last = self.len() - n + 1;
        let from = from.min(last);
        if forward {
            self.scan(pattern, from, last, true).or_else(|| self.scan(pattern, 0, from, true))
        } else {
            self.scan(pattern, 0, from, false).or_else(|| self.scan(pattern, from, last, false))
        }
    }

    /// The first (or last) start in `lo..hi` where `pattern` is.
    fn scan(&self, pattern: &Pattern, lo: u64, hi: u64, forward: bool) -> Option<u64> {
        let over = pattern.len() as u64 - 1;
        let mut chunks: Vec<u64> = (lo..hi).step_by(CHUNK as usize).collect();
        if !forward {
            chunks.reverse();
        }
        for start in chunks {
            let starts = CHUNK.min(hi - start) as usize;
            let buf = self.read(start, starts + over as usize);
            let at = |i: &usize| matches(&buf[*i..], pattern);
            let found = if forward { (0..starts).find(at) } else { (0..starts).rev().find(at) };
            if let Some(i) = found {
                return Some(start + i as u64);
            }
            if buf.len() < starts {
                // The file is shorter than it was.
                return None;
            }
        }
        None
    }
}

/// Whether `bytes` start with `pattern`.
fn matches(bytes: &[u8], pattern: &Pattern) -> bool {
    bytes.len() >= pattern.len() && pattern.iter().zip(bytes).all(|(p, b)| p.is_none_or(|p| p == *b))
}

/// `len` bytes of `path` from `at`, the file opened once.
fn read_at(cache: &mut Cache, path: &str, at: u64, len: usize) -> Option<Vec<u8>> {
    let read = |file: &mut File| -> std::io::Result<Vec<u8>> {
        file.seek(SeekFrom::Start(at))?;
        let mut buf = Vec::with_capacity(len);
        file.take(len as u64).read_to_end(&mut buf)?;
        Ok(buf)
    };
    if cache.file.is_none() {
        match File::open(path) {
            Ok(f) => cache.file = Some(f),
            Err(e) => {
                cache.error = Some(e.to_string());
                return None;
            }
        }
    }
    match read(cache.file.as_mut()?) {
        Ok(buf) => {
            cache.error = (buf.len() < len).then(|| "the file is shorter than it was: R reads it again".into());
            Some(buf)
        }
        Err(e) => {
            cache.error = Some(e.to_string());
            cache.file = None;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> Data {
        let dir = std::env::temp_dir().join("fenix-hex-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        Data::new(Input::Path(path.to_string_lossy().into_owned(), bytes.len() as u64))
    }

    fn big() -> Vec<u8> {
        (0..300_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_go_across_blocks_and_stop_at_the_end() {
        let bytes = big();
        let d = file("blocks.bin", &bytes);
        assert_eq!(d.read(BLOCK - 3, 6), bytes[BLOCK as usize - 3..BLOCK as usize + 3]);
        assert_eq!(d.read(299_998, 10), bytes[299_998..]);
        assert_eq!(d.read(300_000, 10), Vec::<u8>::new());
        assert_eq!(d.read(10, 200_000), bytes[10..200_010], "a long read goes past the blocks");
        assert_eq!(d.byte(BLOCK * 2), Some(bytes[BLOCK as usize * 2]));
        assert_eq!(d.error(), None);
        let m = Data::new(Input::Bytes(bytes.clone()));
        assert_eq!(m.read(5, 3), bytes[5..8]);
    }

    #[test]
    fn more_blocks_than_are_kept_are_read_again() {
        let bytes: Vec<u8> = (0..(BLOCK as usize * (KEPT + 4))).map(|i| (i / 7) as u8).collect();
        let d = file("many.bin", &bytes);
        for i in 0..(KEPT as u64 + 4) {
            assert_eq!(d.byte(i * BLOCK + 9), Some(bytes[(i * BLOCK + 9) as usize]));
        }
        assert_eq!(d.cache.borrow().blocks.len(), KEPT);
        assert_eq!(d.byte(9), Some(bytes[9]));
    }

    #[test]
    fn a_search_goes_round_the_end_and_across_chunks() {
        let mut bytes = vec![0u8; CHUNK as usize + 100];
        bytes[CHUNK as usize - 2..CHUNK as usize + 2].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        bytes[10..14].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let d = file("search.bin", &bytes);
        let p = [Some(0xde), Some(0xad), Some(0xbe), Some(0xef)];
        assert_eq!(d.find(&p, 0, true), Some(10));
        assert_eq!(d.find(&p, 11, true), Some(CHUNK - 2), "across the chunk's edge");
        assert_eq!(d.find(&p, CHUNK, true), Some(10), "round the end");
        assert_eq!(d.find(&p, CHUNK, false), Some(CHUNK - 2));
        assert_eq!(d.find(&p, 10, false), Some(CHUNK - 2), "round the start");
        assert_eq!(d.find(&[Some(0xad), None, Some(0xef)], 0, true), Some(11), "?? is any byte");
        assert_eq!(d.find(&[Some(0x42)], 0, true), None);
    }
}
