//! A cell's outputs as the page shows them: text (a stream, a result's
//! `text/plain`, a traceback with its colours taken out), and pictures
//! (`image/png`, `image/jpeg`), decoded from the notebook's base64.

use base64::Engine;
use serde_json::Value;

use crate::nb::text_of;

/// How a run of text is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    /// stderr.
    Warn,
    /// An error's traceback.
    Bad,
    /// Something about the output, not the output: what it is when
    /// there's nothing here to show of it.
    Muted,
}

/// A piece of an output.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Text { text: String, tone: Tone },
    /// A picture: its bytes, and its size in pixels.
    Picture { bytes: Vec<u8>, width: u32, height: u32 },
}

/// What output `out` shows: its text, or its picture.
pub fn shown(out: &Value) -> Vec<Shown> {
    let text = |t: String, tone| Shown::Text { text: t, tone };
    match out["output_type"].as_str() {
        Some("stream") => {
            let tone = if out["name"] == "stderr" { Tone::Warn } else { Tone::Text };
            vec![text(terminal(&text_of(&out["text"])), tone)]
        }
        Some("error") => {
            let tb: Vec<String> = out["traceback"].as_array().map(|l| l.iter().map(text_of).collect()).unwrap_or_default();
            let body = if tb.is_empty() { format!("{}: {}", out["ename"].as_str().unwrap_or("Error"), out["evalue"].as_str().unwrap_or("")) } else { tb.join("\n") };
            vec![text(terminal(&body), Tone::Bad)]
        }
        Some("execute_result") | Some("display_data") | Some("update_display_data") => data(&out["data"]),
        Some(other) => vec![text(format!("({other} output)"), Tone::Muted)],
        None => Vec::new(),
    }
}

/// A result's or a display's best kind of data, as this page can show
/// it: a picture, else its text.
fn data(data: &Value) -> Vec<Shown> {
    for kind in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        if let Some(b64) = data.get(kind) {
            if let Some(p) = picture(&text_of(b64)) {
                return vec![p];
            }
        }
    }
    if let Some(plain) = data.get("text/plain") {
        return vec![Shown::Text { text: terminal(&text_of(plain)), tone: Tone::Text }];
    }
    if let Some(md) = data.get("text/markdown") {
        return vec![Shown::Text { text: text_of(md), tone: Tone::Text }];
    }
    let kinds: Vec<&str> = data.as_object().map(|m| m.keys().map(String::as_str).collect()).unwrap_or_default();
    if kinds.is_empty() {
        return Vec::new();
    }
    vec![Shown::Text { text: format!("({} -- open the notebook in Jupyter to see it)", kinds.join(", ")), tone: Tone::Muted }]
}

/// A picture from base64, with its size, if it's one Fenix can show.
pub fn picture(b64: &str) -> Option<Shown> {
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()).ok()?;
    let (width, height) = size(&bytes)?;
    Some(Shown::Picture { bytes, width, height })
}

/// A PNG's, a JPEG's, a GIF's or a WebP's size, from its header, so a
/// picture's room is known without decoding it.
pub fn size(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let le16 = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    let be16 = |i: usize| Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return match b.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let v = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                Some(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => {
                let w = u32::from_le_bytes([*b.get(24)?, *b.get(25)?, *b.get(26)?, 0]) + 1;
                let h = u32::from_le_bytes([*b.get(27)?, *b.get(28)?, *b.get(29)?, 0]) + 1;
                Some((w, h))
            }
            _ => None,
        };
    }
    if b.starts_with(&[0xff, 0xd8]) {
        // Through the segments to a frame's header (SOF0..SOF15 but the
        // DHT, JPG and DAC markers).
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                return None;
            }
            let marker = b[i + 1];
            if marker == 0xff {
                i += 1;
                continue;
            }
            let len = be16(i + 2)? as usize;
            if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + len;
        }
    }
    None
}

/// Text a terminal would show: its colours (ANSI escapes) taken out,
/// and a line a `\r` went back over (a progress bar) as it ended up.
pub fn terminal(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // CSI: parameters, then a final letter.
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: to BEL or ST.
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            if c == '\u{1b}' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
            continue;
        }
        plain.push(c);
    }
    plain.split('\n').map(|line| line.trim_end_matches('\r').rsplit('\r').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_traceback_loses_its_colours() {
        let out = json!({"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero", "traceback": ["\u{1b}[31mZeroDivisionError\u{1b}[39m: division by zero"]});
        assert_eq!(shown(&out), vec![Shown::Text { text: "ZeroDivisionError: division by zero".into(), tone: Tone::Bad }]);
    }

    #[test]
    fn a_progress_bar_shows_where_it_got_to() {
        assert_eq!(terminal("10%\r50%\r100%\ndone\r\n"), "100%\ndone\n");
        assert_eq!(terminal("\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{1b}\\ ok"), "link ok");
    }

    #[test]
    fn stderr_is_a_warning() {
        let out = json!({"output_type": "stream", "name": "stderr", "text": ["careful\n"]});
        assert_eq!(shown(&out), vec![Shown::Text { text: "careful\n".into(), tone: Tone::Warn }]);
    }

    #[test]
    fn a_plot_is_a_picture() {
        let nb: Value = serde_json::from_str(include_str!("../tests/fixtures/assignment.ipynb")).unwrap();
        let out = &nb["cells"][3]["outputs"][0];
        match &shown(out)[..] {
            [Shown::Picture { width, height, bytes }] => {
                assert!(*width > 0 && *height > 0 && bytes.starts_with(b"\x89PNG"), "{width} x {height}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn html_alone_says_what_it_is() {
        let out = json!({"output_type": "display_data", "data": {"text/html": "<b>x</b>"}, "metadata": {}});
        assert!(matches!(&shown(&out)[..], [Shown::Text { tone: Tone::Muted, text }] if text.contains("text/html")));
        let both = json!({"output_type": "execute_result", "data": {"text/html": "<table/>", "text/plain": "   a\n0  1"}, "metadata": {}});
        assert_eq!(shown(&both), vec![Shown::Text { text: "   a\n0  1".into(), tone: Tone::Text }]);
    }

    #[test]
    fn sizes_from_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(size(&png), Some((640, 480)));
        assert_eq!(size(b"GIF89a\x10\x00\x20\x00"), Some((16, 32)));
        // A JPEG: SOI, an APP0 of 16 bytes, then SOF0 of 300 x 200.
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        jpg.extend([0u8; 14]);
        jpg.extend([0xff, 0xc0, 0, 17, 8, 0, 200, 1, 44, 3, 0, 0, 0, 0, 0, 0]);
        assert_eq!(size(&jpg), Some((300, 200)));
        assert_eq!(size(b"nothing"), None);
    }
}
