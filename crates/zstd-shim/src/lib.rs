//! The `zstd` crate's [`Decoder`] and [`Encoder`], as much of them as the
//! `parquet` crate uses, in pure Rust: zstd-sys's C needs a C compiler
//! for `wasm32-wasip2`, which an extension's build doesn't have.
//!
//! Decoding is ruzstd's. Encoding isn't here: [`Encoder::new`] says so,
//! and nothing that only reads calls it.

use std::io::{self, Cursor, Read, Write};

/// A zstd stream's bytes, decoded: every frame of it, one after another.
pub struct Decoder(Cursor<Vec<u8>>);

impl Decoder {
    pub fn new(mut input: &[u8]) -> io::Result<Decoder> {
        let mut out = Vec::new();
        while !input.is_empty() {
            let mut frame = ruzstd::decoding::StreamingDecoder::new(&mut input).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            frame.read_to_end(&mut out)?;
        }
        Ok(Decoder(Cursor::new(out)))
    }
}

impl Read for Decoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

/// Never made: compressing isn't built in.
pub struct Encoder<W>(W);

impl<W: Write> Encoder<W> {
    pub fn new(_out: W, _level: i32) -> io::Result<Encoder<W>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "zstd compression isn't built in, only decompression"))
    }

    pub fn finish(self) -> io::Result<W> {
        Ok(self.0)
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "hello, hello, hello", compressed by libzstd (through pyarrow).
    const FRAME: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd, 0x20, 0x13, 0x6d, 0x00, 0x00, 0x38, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x01, 0x00, 0xe2, 0x8a, 0x11];

    #[test]
    fn every_frame_is_decoded() {
        let mut two = FRAME.to_vec();
        two.extend_from_slice(FRAME);
        let mut out = String::new();
        Decoder::new(&two).unwrap().read_to_string(&mut out).unwrap();
        assert_eq!(out, "hello, hello, hellohello, hello, hello");
    }

    #[test]
    fn what_isnt_zstd_is_an_error() {
        assert!(Decoder::new(b"not zstd at all").is_err());
    }

    #[test]
    fn compressing_says_it_isnt_there() {
        assert_eq!(Encoder::new(Vec::new(), 3).err().unwrap().kind(), io::ErrorKind::Unsupported);
    }
}
