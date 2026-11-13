//! A step's output as Fenix gives it to a provider (Fenix §6.4): each
//! stream's bytes in lines, lossy UTF-8, a trailing `\r` dropped, colour
//! codes taken out, a line past 64 KiB in pieces; the two streams
//! interleaved, each in its own order; batches of at most 512 lines and
//! 256 KiB.

use fenix_lang_kit::tests::api::Line;

/// Where Fenix cuts a line: `TaskRunner`'s `MAX_LINE`, 64 KiB of bytes
/// (`fenix-tasks/src/supervisor.rs:17`, `436-438`).
pub const CUT_AT: usize = 64 * 1024;
/// A `tests-read`'s most lines and bytes (Fenix §6.4).
pub const BATCH_LINES: usize = 512;
pub const BATCH_BYTES: usize = 256 * 1024;

/// One stream's bytes as Fenix gives them (Fenix §6.4 "Lines", §19 K3
/// and K4): a line at each `\n`, its last `\r` dropped; a line longer
/// than 64 KiB (`cut_at`) in pieces of at most that, each cut at a
/// character's start and marked `cut` but the last -- a line of 64 KiB
/// or less, at the stream's end too, is never cut; each read as lossy
/// UTF-8 with its colour codes taken out. What's after the last `\n` is
/// a line of its own.
pub fn lines(bytes: &[u8], stderr: bool, cut_at: usize) -> Vec<Line> {
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let emit = |piece: &[u8], cut: bool, out: &mut Vec<Line>| {
        let piece = if !cut && piece.last() == Some(&b'\r') { &piece[..piece.len() - 1] } else { piece };
        out.push(Line { text: strip_ansi(&String::from_utf8_lossy(piece)), stderr, cut });
    };
    let continuation = |b: u8| b & 0xC0 == 0x80;
    for &b in bytes {
        if b == b'\n' {
            emit(&pending, false, &mut out);
            pending.clear();
            continue;
        }
        if pending.len() >= cut_at {
            // More than the limit: a piece, cut where a character starts
            // (back over at most three bytes of one).
            let mut at = pending.len();
            let byte_at = |i: usize| if i == pending.len() { b } else { pending[i] };
            while at > 0 && pending.len() - at < 3 && continuation(byte_at(at)) {
                at -= 1;
            }
            if at == 0 || continuation(byte_at(at)) {
                at = pending.len();
            }
            emit(&pending[..at], true, &mut out);
            pending.drain(..at);
        }
        pending.push(b);
    }
    if !pending.is_empty() {
        emit(&pending, false, &mut out);
    }
    out
}

/// `text` without ANSI CSI (`ESC [ … final`) and OSC (`ESC ] … BEL` or
/// `ESC \`) sequences.
pub fn strip_ansi(text: &str) -> String {
    if !text.contains('\u{1b}') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// How a step's two streams are interleaved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    /// As recorded: a `<name>.order` file of `o` and `e`, one a line; else
    /// stdout first.
    Recorded,
    StdoutFirst,
    StderrFirst,
    /// As Windows gives them, per 10 ms tick: runs of a few lines from one
    /// stream then the other, chosen from this seed. Each stream keeps its
    /// own order (and a cut line's pieces may have the other stream's
    /// lines between them).
    Shuffled(u64),
}

/// The two streams' lines as one sequence, in `order`; `recorded` is the
/// `.order` file's, when there was one.
pub fn interleave(out: Vec<Line>, err: Vec<Line>, order: &Order, recorded: Option<&[bool]>) -> Vec<Line> {
    let mut all = Vec::with_capacity(out.len() + err.len());
    let (mut o, mut e) = (out.into_iter().peekable(), err.into_iter().peekable());
    match order {
        Order::StdoutFirst => all.extend(o.chain(e)),
        Order::StderrFirst => all.extend(e.chain(o)),
        Order::Recorded => {
            for &stderr in recorded.unwrap_or(&[]) {
                let next = if stderr { e.next() } else { o.next() };
                all.extend(next);
            }
            all.extend(o.chain(e));
        }
        Order::Shuffled(seed) => {
            let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1;
            while o.peek().is_some() || e.peek().is_some() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let take_err = if o.peek().is_none() { true } else if e.peek().is_none() { false } else { x & 1 == 1 };
                let run = 1 + (x >> 8) as usize % 4;
                let stream = if take_err { &mut e } else { &mut o };
                all.extend(stream.take(run));
            }
        }
    }
    all
}

/// Lines in calls: at most `most` lines (512) and 256 KiB a call.
pub fn batches(lines: Vec<Line>, most: usize) -> Vec<Vec<Line>> {
    let mut out = Vec::new();
    let mut batch: Vec<Line> = Vec::new();
    let mut bytes = 0;
    for l in lines {
        if !batch.is_empty() && (batch.len() == most || bytes + l.text.len() > BATCH_BYTES) {
            out.push(std::mem::take(&mut batch));
            bytes = 0;
        }
        bytes += l.text.len();
        batch.push(l);
    }
    if !batch.is_empty() {
        out.push(batch);
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn lines_are_cut_as_fenixs_task_runner_cuts_them() {
        let got = lines(b"one\r\ntwo\n\x1b[31mred\x1b[0m\x1b]8;;http://x\x07link\x1b]8;;\x1b\\\nlast", false, 64 * 1024);
        let texts: Vec<&str> = got.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["one", "two", "redlink", "last"]);
        // Only a line longer than the limit is cut (Fenix §19, K4): one
        // of exactly the limit isn't, at a stream's end either.
        let got = lines(b"abcdefg\nabc\nxyz", true, 3);
        let pieces: Vec<(&str, bool)> = got.iter().map(|l| (l.text.as_str(), l.cut)).collect();
        assert_eq!(pieces, [("abc", true), ("def", true), ("g", false), ("abc", false), ("xyz", false)]);
        assert!(got.iter().all(|l| l.stderr));
        // A piece ends where a character starts: never one split.
        let got = lines("aéb".as_bytes(), false, 2);
        let pieces: Vec<(&str, bool)> = got.iter().map(|l| (l.text.as_str(), l.cut)).collect();
        assert_eq!(pieces, [("a", true), ("é", true), ("b", false)]);
    }

    #[test]
    fn streams_keep_their_order_and_batches_their_limits() {
        let mk = |n: usize, stderr| (0..n).map(|i| Line { text: format!("{}{i}", if stderr { 'e' } else { 'o' }), stderr, cut: false }).collect::<Vec<_>>();
        let all = interleave(mk(50, false), mk(50, true), &Order::Shuffled(7), None);
        let o: Vec<&str> = all.iter().filter(|l| !l.stderr).map(|l| l.text.as_str()).collect();
        assert_eq!(o, mk(50, false).iter().map(|l| l.text.as_str()).collect::<Vec<_>>());
        assert!(all.windows(2).any(|w| w[0].stderr != w[1].stderr) && all[..10].iter().any(|l| l.stderr), "mixed");
        let rec = interleave(mk(2, false), mk(1, true), &Order::Recorded, Some(&[true, false]));
        assert_eq!(rec.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["e0", "o0", "o1"]);
        let b = batches(mk(1100, false), 512);
        assert_eq!(b.iter().map(Vec::len).collect::<Vec<_>>(), [512, 512, 76]);
        let big = (0..5).map(|_| Line { text: "x".repeat(64 * 1024), stderr: false, cut: true }).collect();
        assert_eq!(batches(big, 512).iter().map(Vec::len).collect::<Vec<_>>(), [4, 1]);
    }
}
