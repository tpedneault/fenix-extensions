//! What the bytes under the cursor are as numbers and as UTF-8, and
//! what's typed at the prompts: a search's bytes, an offset to go to.

/// A row of the inspector: its type, then the value little-endian and
/// big-endian -- one value only, for a byte.
pub struct Row {
    pub kind: &'static str,
    pub le: Option<String>,
    pub be: Option<String>,
}

/// The inspector's rows for the bytes from the cursor (as many as there
/// are, up to 8): a type the bytes don't reach has no value.
pub fn rows(bytes: &[u8]) -> Vec<Row> {
    fn take<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
        bytes.get(..N)?.try_into().ok()
    }
    macro_rules! both {
        ($kind:literal, $t:ty, $fmt:expr) => {{
            let b = take::<{ std::mem::size_of::<$t>() }>(bytes);
            Row { kind: $kind, le: b.map(|b| $fmt(<$t>::from_le_bytes(b))), be: b.map(|b| $fmt(<$t>::from_be_bytes(b))) }
        }};
    }
    let byte = bytes.first().copied();
    vec![
        Row { kind: "u8", le: byte.map(|b| b.to_string()), be: None },
        Row { kind: "i8", le: byte.map(|b| (b as i8).to_string()), be: None },
        Row { kind: "bits", le: byte.map(|b| format!("{:04b} {:04b}", b >> 4, b & 0xf)), be: None },
        both!("u16", u16, |v: u16| v.to_string()),
        both!("i16", i16, |v: i16| v.to_string()),
        both!("u32", u32, |v: u32| v.to_string()),
        both!("i32", i32, |v: i32| v.to_string()),
        both!("u64", u64, |v: u64| v.to_string()),
        both!("i64", i64, |v: i64| v.to_string()),
        both!("f32", f32, |v: f32| float(v, (v as f64).abs())),
        both!("f64", f64, |v: f64| float(v, v.abs())),
    ]
}

/// A float as short as it reads back: plainly when it's neither tiny nor
/// huge, else with an exponent.
fn float<F: std::fmt::Display + std::fmt::LowerExp>(v: F, magnitude: f64) -> String {
    if magnitude == 0.0 || !magnitude.is_finite() || (1e-4..1e15).contains(&magnitude) {
        v.to_string()
    } else {
        format!("{v:e}")
    }
}

/// The character the bytes from the cursor start, in UTF-8: it, its code
/// point and its length; or why they don't.
pub fn utf8(bytes: &[u8]) -> String {
    let Some(&first) = bytes.first() else { return String::new() };
    let len = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        0x80..=0xbf => return "a continuation byte".into(),
        _ => return "not UTF-8".into(),
    };
    let Some(c) = bytes.get(..len).and_then(|b| std::str::from_utf8(b).ok()).and_then(|s| s.chars().next()) else {
        return format!("not UTF-8 (a {len}-byte start)");
    };
    let shown = match c {
        c if c.is_control() => String::new(),
        c => format!("'{c}' "),
    };
    let bytes = if len == 1 { "1 byte".to_string() } else { format!("{len} bytes") };
    format!("{shown}U+{:04X} · {bytes}", c as u32)
}

/// What a search looks for, and how it's said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub pattern: Vec<Option<u8>>,
    pub said: String,
}

/// What's typed at the search prompt: `"text"` quoted is text; hex bytes
/// (`de ad be ef`, `0xdeadbeef`, `?? ` for any byte) are bytes, unless
/// `text` (the text column has the cursor); anything else is text.
pub fn search(typed: &str, text: bool) -> Result<Search, String> {
    let t = typed.trim();
    if t.is_empty() {
        return Err("nothing to search for".into());
    }
    let quoted = t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')));
    if quoted {
        return Ok(as_text(&t[1..t.len() - 1]));
    }
    if !text {
        if let Some(pattern) = hex(t) {
            let said = pattern.iter().map(|b| b.map_or("??".into(), |b| format!("{b:02x}"))).collect::<Vec<_>>().join(" ");
            return Ok(Search { pattern, said });
        }
    }
    Ok(as_text(typed))
}

fn as_text(text: &str) -> Search {
    Search { pattern: text.bytes().map(Some).collect(), said: format!("\"{text}\"") }
}

/// Hex bytes, in pairs: spaces, commas and `0x` between them.
pub fn hex(t: &str) -> Option<Vec<Option<u8>>> {
    let mut digits = String::new();
    for word in t.split([' ', ',', '\t']).filter(|w| !w.is_empty()) {
        let w = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")).unwrap_or(word);
        if w.is_empty() || w.len() % 2 == 1 || !w.chars().all(|c| c.is_ascii_hexdigit() || c == '?') {
            return None;
        }
        digits += w;
    }
    let pairs: Vec<&str> = (0..digits.len()).step_by(2).map(|i| &digits[i..i + 2]).collect();
    if pairs.is_empty() {
        return None;
    }
    pairs.iter().map(|p| if *p == "??" { Some(None) } else { u8::from_str_radix(p, 16).ok().map(Some) }).collect()
}

/// The offset typed at the go-to prompt, from the cursor's `at` in a
/// file of `len` bytes: `0x1f0` or `1f0h` (or with a hex letter) is hex,
/// `496` decimal, `50%` of the way in, and `+` or `-` from the cursor.
pub fn offset(typed: &str, at: u64, len: u64) -> Result<u64, String> {
    let t: String = typed.trim().chars().filter(|c| *c != '_' && *c != ' ').collect();
    let (sign, t) = match t.strip_prefix('+') {
        Some(rest) => (1, rest.to_string()),
        None => match t.strip_prefix('-') {
            Some(rest) => (-1, rest.to_string()),
            None => (0, t),
        },
    };
    if t.is_empty() {
        return Err("no offset".into());
    }
    let bad = || format!("{} isn't an offset: 0x1f0, 496, 50%, +16", typed.trim());
    let n: u64 = if let Some(p) = t.strip_suffix('%') {
        let p: f64 = p.parse().map_err(|_| bad())?;
        (len as f64 * p.clamp(0.0, 100.0) / 100.0) as u64
    } else if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).or_else(|| t.strip_suffix(['h', 'H'])) {
        u64::from_str_radix(h, 16).map_err(|_| bad())?
    } else if t.chars().all(|c| c.is_ascii_digit()) {
        t.parse().map_err(|_| bad())?
    } else {
        u64::from_str_radix(&t, 16).map_err(|_| bad())?
    };
    Ok(match sign {
        1 => at.saturating_add(n),
        -1 => at.saturating_sub(n),
        _ => n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inspector_reads_both_ways() {
        let r = rows(&[0xde, 0xad, 0xbe, 0xef, 0, 0, 0x80, 0x3f]);
        let get = |kind: &str| r.iter().find(|r| r.kind == kind).map(|r| (r.le.clone(), r.be.clone())).unwrap();
        assert_eq!(get("u8"), (Some("222".into()), None));
        assert_eq!(get("i8"), (Some("-34".into()), None));
        assert_eq!(get("bits"), (Some("1101 1110".into()), None));
        assert_eq!(get("u16"), (Some("44510".into()), Some("57005".into())));
        assert_eq!(get("u32"), (Some("4022250974".into()), Some("3735928559".into())));
        assert_eq!(get("i32").0, Some("-272716322".into()));
        let r = rows(&[0, 0, 0x80, 0x3f]);
        let f32 = r.iter().find(|r| r.kind == "f32").unwrap();
        assert_eq!(f32.le.as_deref(), Some("1"));
        assert!(r.iter().find(|r| r.kind == "u64").unwrap().le.is_none(), "past the end");
        let r = rows(&1.5e300f64.to_le_bytes());
        assert_eq!(r.iter().find(|r| r.kind == "f64").unwrap().le.as_deref(), Some("1.5e300"));
        let r = rows(&(-0.25f64).to_le_bytes());
        assert_eq!(r.iter().find(|r| r.kind == "f64").unwrap().le.as_deref(), Some("-0.25"));
    }

    #[test]
    fn utf8_is_read_from_the_cursor() {
        assert_eq!(utf8("é!".as_bytes()), "'é' U+00E9 · 2 bytes");
        assert_eq!(utf8(b"A"), "'A' U+0041 · 1 byte");
        assert_eq!(utf8(b"\n"), "U+000A · 1 byte");
        assert_eq!(utf8(&[0xa9]), "a continuation byte");
        assert_eq!(utf8(&[0xe2, 0x28]), "not UTF-8 (a 3-byte start)");
        assert_eq!(utf8(&[0xff]), "not UTF-8");
    }

    #[test]
    fn a_search_is_hex_or_text() {
        assert_eq!(search("de ad 0xbe,EF", false).unwrap().pattern, vec![Some(0xde), Some(0xad), Some(0xbe), Some(0xef)]);
        assert_eq!(search("cafe??", false).unwrap().said, "ca fe ??");
        assert_eq!(search("\"cafe\"", false).unwrap().pattern, b"cafe".iter().copied().map(Some).collect::<Vec<_>>());
        assert_eq!(search("cafe", true).unwrap().said, "\"cafe\"", "in the text column it's text");
        assert_eq!(search("PNG!", false).unwrap().said, "\"PNG!\"");
        assert_eq!(search("abc", false).unwrap().said, "\"abc\"", "an odd count isn't bytes");
        assert!(search("  ", false).is_err());
    }

    #[test]
    fn offsets_are_hex_decimal_percent_or_relative() {
        assert_eq!(offset("0x1f0", 0, 1000), Ok(0x1f0));
        assert_eq!(offset("1f0h", 0, 1000), Ok(0x1f0));
        assert_eq!(offset("ff", 0, 1000), Ok(0xff));
        assert_eq!(offset("496", 0, 1000), Ok(496));
        assert_eq!(offset("1_000", 0, 2000), Ok(1000));
        assert_eq!(offset("50%", 0, 1000), Ok(500));
        assert_eq!(offset("+0x10", 4, 1000), Ok(20));
        assert_eq!(offset("-10", 4, 1000), Ok(0));
        assert!(offset("zz", 0, 10).is_err());
        assert!(offset("", 0, 10).is_err());
    }
}
