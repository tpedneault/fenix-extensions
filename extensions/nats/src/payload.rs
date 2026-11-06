//! Payloads shown: a line's preview, and the whole when a message is
//! opened -- JSON pretty-printed and coloured, text as it is, anything else
//! as a hex dump.

use fenix_extension_api::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Empty,
    Json,
    Text,
    Binary,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Empty => "empty",
            Kind::Json => "json",
            Kind::Text => "text",
            Kind::Binary => "binary",
        }
    }
}

/// Whether it reads as text: UTF-8, with few control characters.
pub fn is_text(bytes: &[u8]) -> bool {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        // Cut in the middle of a character: what's before it.
        Err(e) if e.error_len().is_none() && e.valid_up_to() + 4 > bytes.len() => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
        Err(_) => return false,
    };
    let control = s.chars().filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')).count();
    control * 20 <= s.chars().count()
}

/// What a payload is, from its first bytes: JSON only when it starts like
/// JSON (parsing it whole is for when it's opened).
pub fn sniff(payload: &[u8]) -> Kind {
    let start = payload.iter().position(|b| !b.is_ascii_whitespace());
    match start {
        None => Kind::Empty,
        Some(i) if matches!(payload[i], b'{' | b'[') && is_text(&payload[..payload.len().min(512)]) => Kind::Json,
        Some(_) if is_text(&payload[..payload.len().min(512)]) => Kind::Text,
        Some(_) => Kind::Binary,
    }
}

/// One line of it, at most `width` characters: text with its line ends
/// and runs of spaces as one space, or hex.
pub fn preview(payload: &[u8], width: usize) -> (String, Kind) {
    let kind = sniff(payload);
    let mut out = String::new();
    match kind {
        Kind::Empty => {}
        Kind::Binary => {
            for (i, b) in payload.iter().take(width / 3 + 1).enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&format!("{b:02x}"));
            }
        }
        Kind::Json | Kind::Text => {
            let head = &payload[..payload.len().min(width * 4 + 8)];
            let text = String::from_utf8_lossy(head);
            let mut space = false;
            for c in text.chars() {
                if c.is_whitespace() || c.is_control() {
                    space = !out.is_empty();
                    continue;
                }
                if space {
                    out.push(' ');
                    space = false;
                }
                out.push(c);
                if out.len() > width * 4 {
                    break;
                }
            }
        }
    }
    let n = out.chars().count();
    if n > width {
        out = out.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
    }
    (out, kind)
}

/// A line of the opened payload: its runs.
pub type Line = Vec<(String, Role)>;

/// The most lines an opened payload shows; `o` opens it whole.
pub const MAX_LINES: usize = 400;

fn syntax(s: &str) -> Role {
    Role::Syntax(s.into())
}

/// A JSON text's line, coloured: keys, strings, numbers, constants.
fn json_line(line: &str) -> Line {
    let mut out: Line = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let push = |out: &mut Line, text: String, role: Role| {
        if let Some(last) = out.last_mut() {
            if last.1 == role {
                last.0.push_str(&text);
                return;
            }
        }
        out.push((text, role));
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            let text: String = chars[start..i].iter().collect();
            let key = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
            push(&mut out, text, syntax(if key { "property" } else { "string" }));
        } else if c == '-' || c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || "+-.eE".contains(chars[i])) {
                i += 1;
            }
            push(&mut out, chars[start..i].iter().collect(), syntax("number"));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            push(&mut out, chars[start..i].iter().collect(), syntax("constant"));
        } else {
            let start = i;
            while i < chars.len() && !(chars[i] == '"' || chars[i] == '-' || chars[i].is_ascii_alphanumeric()) {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let role = if text.trim().is_empty() { Role::Text } else { syntax("punctuation") };
            push(&mut out, text, role);
        }
    }
    out
}

/// The payload, pretty: JSON indented, text a line at a time, else a hex
/// dump. At most [`MAX_LINES`]; whether it was cut.
pub fn detail(payload: &[u8]) -> (Vec<Line>, Kind, bool) {
    let kind = sniff(payload);
    let mut lines: Vec<Line> = Vec::new();
    let mut cut = false;
    let mut add = |lines: &mut Vec<Line>, line: Line| {
        if lines.len() >= MAX_LINES {
            cut = true;
            false
        } else {
            lines.push(line);
            true
        }
    };
    match kind {
        Kind::Empty => {
            add(&mut lines, vec![("(empty payload)".into(), Role::Muted)]);
        }
        Kind::Json => match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(v) => {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or_default();
                for l in pretty.lines() {
                    if !add(&mut lines, json_line(l)) {
                        break;
                    }
                }
            }
            Err(e) => {
                add(&mut lines, vec![(format!("(not valid JSON: {e})"), Role::Warn)]);
                for l in String::from_utf8_lossy(payload).lines() {
                    if !add(&mut lines, vec![(l.replace('\t', "    "), Role::Text)]) {
                        break;
                    }
                }
            }
        },
        Kind::Text => {
            for l in String::from_utf8_lossy(payload).lines() {
                let clean: String = l.replace('\t', "    ").chars().map(|c| if c.is_control() { '·' } else { c }).collect();
                if !add(&mut lines, vec![(clean, Role::Text)]) {
                    break;
                }
            }
        }
        Kind::Binary => {
            for (row, chunk) in payload.chunks(16).enumerate() {
                let mut hex = String::new();
                for (i, b) in chunk.iter().enumerate() {
                    hex.push_str(&format!("{b:02x} "));
                    if i == 7 {
                        hex.push(' ');
                    }
                }
                let ascii: String = chunk.iter().map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' }).collect();
                let line = vec![(format!("{:08x}  ", row * 16), syntax("comment")), (format!("{hex:<50}"), syntax("number")), (ascii, Role::Muted)];
                if !add(&mut lines, line) {
                    break;
                }
            }
        }
    }
    (lines, kind, cut)
}

/// The payload as text to open in a buffer: JSON pretty, text as it is, a
/// hex dump whole.
pub fn full_text(payload: &[u8]) -> (String, Option<&'static str>) {
    match sniff(payload) {
        Kind::Json => match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), Some("json")),
            Err(_) => (String::from_utf8_lossy(payload).into_owned(), None),
        },
        Kind::Text | Kind::Empty => (String::from_utf8_lossy(payload).into_owned(), None),
        Kind::Binary => {
            let mut out = String::new();
            for (row, chunk) in payload.chunks(16).enumerate() {
                let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                let ascii: String = chunk.iter().map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' }).collect();
                out.push_str(&format!("{:08x}  {:<48}  {ascii}\n", row * 16, hex.join(" ")));
            }
            (out, None)
        }
    }
}

/// A size, shortly: `512 B`, `1.5 KiB`, `12 MiB`.
pub fn bytes(n: f64) -> String {
    if n < 1024.0 {
        format!("{} B", n.round() as u64)
    } else if n < 1024.0 * 1024.0 {
        trim(n / 1024.0, "KiB")
    } else if n < 1024.0 * 1024.0 * 1024.0 {
        trim(n / 1024.0 / 1024.0, "MiB")
    } else {
        trim(n / 1024.0 / 1024.0 / 1024.0, "GiB")
    }
}

fn trim(v: f64, unit: &str) -> String {
    if v >= 100.0 {
        format!("{} {unit}", v.round() as u64)
    } else {
        format!("{v:.1} {unit}")
    }
}

/// A count, shortly: `950`, `12.3k`, `4.1M`.
pub fn count(n: f64) -> String {
    if n < 1000.0 {
        if n.fract() == 0.0 || n >= 100.0 {
            format!("{}", n.round() as u64)
        } else {
            format!("{n:.1}")
        }
    } else if n < 1_000_000.0 {
        format!("{:.1}k", n / 1000.0)
    } else if n < 1_000_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else {
        format!("{:.1}G", n / 1_000_000_000.0)
    }
}

/// Eight levels of a sparkline, by the largest.
pub fn sparkline(values: &[u32]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|&v| if max == 0 { ' ' } else { BARS[((v as f64 / max as f64) * 7.0).round() as usize] })
        .collect()
}

/// How long ago, shortly: `now`, `3s`, `4m`, `2h`, `5d`.
pub fn ago(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0 => "now".into(),
        1..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// A time of day, from Unix milliseconds and an offset: `14:03:22.517`.
pub fn clock(ms: u64, offset_s: i64) -> String {
    let local = (ms as i64 + offset_s * 1000).rem_euclid(86_400_000);
    let (h, m, s, milli) = (local / 3_600_000, local / 60_000 % 60, local / 1000 % 60, local % 1000);
    format!("{h:02}:{m:02}:{s:02}.{milli:03}")
}

/// Unix milliseconds as RFC 3339, UTC.
pub fn rfc3339(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let rem = ms % 86_400_000;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_are_told_apart() {
        assert_eq!(sniff(b""), Kind::Empty);
        assert_eq!(sniff(b"  {\"a\":1}"), Kind::Json);
        assert_eq!(sniff(b"[1,2]"), Kind::Json);
        assert_eq!(sniff(b"hello there"), Kind::Text);
        assert_eq!(sniff(&[0, 1, 2, 3, 0xff]), Kind::Binary);
        assert_eq!(sniff("héllo".as_bytes()), Kind::Text);
        let mut cut = "héllo".as_bytes().to_vec();
        cut.truncate(2);
        assert!(is_text(&cut));
    }

    #[test]
    fn previews_fit() {
        assert_eq!(preview(b"{\n  \"a\": 1,\n  \"b\": [1, 2]\n}", 80).0, "{ \"a\": 1, \"b\": [1, 2] }");
        assert_eq!(preview(b"abcdefghij", 5).0, "abcd…");
        assert_eq!(preview(&[0, 1, 255], 20).0, "00 01 ff");
        assert_eq!(preview(b"", 5), (String::new(), Kind::Empty));
    }

    #[test]
    fn json_is_pretty_and_coloured() {
        let (lines, kind, cut) = detail(br#"{"name":"x","n":-1.5,"ok":true,"list":[null]}"#);
        assert_eq!(kind, Kind::Json);
        assert!(!cut);
        let text: Vec<String> = lines.iter().map(|l| l.iter().map(|(t, _)| t.as_str()).collect()).collect();
        assert_eq!(text[0], "{");
        assert_eq!(text[1], "  \"name\": \"x\",");
        let name = &lines[1];
        assert!(name.iter().any(|(t, r)| t == "\"name\"" && *r == Role::Syntax("property".into())));
        assert!(name.iter().any(|(t, r)| t == "\"x\"" && *r == Role::Syntax("string".into())));
        assert!(lines[2].iter().any(|(t, r)| t == "-1.5" && *r == Role::Syntax("number".into())));
        assert!(lines[3].iter().any(|(t, r)| t == "true" && *r == Role::Syntax("constant".into())));
    }

    #[test]
    fn binary_is_a_hex_dump_and_long_ones_are_cut() {
        let (lines, kind, _) = detail(&(0..40u8).collect::<Vec<_>>());
        assert_eq!(kind, Kind::Binary);
        assert_eq!(lines.len(), 3);
        assert!(lines[0][0].0.starts_with("00000000"));
        let long = "x\n".repeat(1000);
        let (lines, _, cut) = detail(long.as_bytes());
        assert_eq!((lines.len(), cut), (MAX_LINES, true));
        assert!(full_text(&[0, 1]).0.starts_with("00000000  00 01"));
    }

    #[test]
    fn numbers_are_said_shortly() {
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(1536.0), "1.5 KiB");
        assert_eq!(bytes(64.0 * 1024.0 * 1024.0), "64.0 MiB");
        assert_eq!(count(950.0), "950");
        assert_eq!(count(2.5), "2.5");
        assert_eq!(count(12_345.0), "12.3k");
        assert_eq!(sparkline(&[0, 1, 4, 8]), "▁▂▅█");
        assert_eq!(sparkline(&[0, 0]), "  ");
        assert_eq!(ago(65_000), "1m");
        assert_eq!(clock(3_723_004, 0), "01:02:03.004");
        assert_eq!(clock(0, -3600), "23:00:00.000");
        assert_eq!(rfc3339(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
    }
}
