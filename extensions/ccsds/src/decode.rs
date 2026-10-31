//! Bytes read as what they are: a packet through the MIB, a frame or a
//! CLTU with what they carry, a CLCW, an on-board time, a CFDP PDU --
//! the inspector's tree -- and the short reading `K` gives on hex or an
//! on-board time.

use fenix_ccsds::field::{Check, Field, Link};
use fenix_mib::mission::Mission;
use fenix_mib::{DefRef, MibSet};

use crate::packet_page::DecodeAs;

/// `K` on an on-board time: the word under the cursor, when it's the
/// mission's time code -- `814B878A.8000` as the converter writes a CUC,
/// or its octets in hex -- read from the mission's epoch.
pub(crate) fn time_hover(line: &str, col: usize, m: &Mission) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let part = |c: char| c.is_ascii_hexdigit() || c == '.' || c == 'x' || c == 'X';
    if !chars.get(col).is_some_and(|c| part(*c)) {
        return None;
    }
    let (mut a, mut b) = (col, col);
    while a > 0 && part(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && part(chars[b]) {
        b += 1;
    }
    let word: String = chars[a..b].iter().collect();
    let word = word.trim_start_matches("0x").trim_start_matches("0X");
    let hex = |s: &str| s.len().is_multiple_of(2).then(|| fenix_ccsds::hex::parse(s)).flatten();
    let bytes = match word.split_once('.') {
        Some((c, f)) => {
            let mut v = hex(c)?;
            v.extend(hex(f)?);
            v
        }
        None => hex(word)?,
    };
    let p = &m.profile;
    if bytes.len() != p.tm_time.len() {
        return None;
    }
    let d = fenix_ccsds::time::decode(&bytes, p.tm_time, p.epoch, &p.leap, 0)?;
    let utc = fenix_ccsds::time::tai_to_utc(d.tai, &p.leap);
    let correlated = if m.nominal_epoch.is_some() { " (correlated)" } else { "" };
    Some(format!(
        "{} · {}{correlated}\n{}\nSPC k T converts it",
        p.tm_time.label(),
        fenix_ccsds::time::format_utc(utc),
        fenix_ccsds::time::format_doy(utc)
    ))
}

/// Bytes decoded as `reading`, through the MIB when there is one: the
/// inspector's tree, and the definition it identified.
pub(crate) fn decode_bytes(bytes: &[u8], reading: DecodeAs, set: Option<&MibSet>, m: &Mission) -> (Field, Option<DefRef>) {
    use fenix_ccsds::{cfdp, coding, frames, time};
    let unreadable = |why: &str| (Field::new("not readable", 0, bytes.len() * 8, fenix_ccsds::field::hex(&bytes[..bytes.len().min(16)]), why).checked(Check::Bad(why.into())), None);
    let looks_like_packet = || fenix_ccsds::PrimaryHeader::decode(bytes).is_some_and(|h| h.plausible() && h.packet_len() <= bytes.len() + 2 && h.packet_len() + 64 >= bytes.len());
    let reading = match reading {
        DecodeAs::Auto if bytes.starts_with(&coding::ASM) => DecodeAs::Frame,
        DecodeAs::Auto if bytes.starts_with(&coding::CLTU_START) => DecodeAs::Cltu,
        DecodeAs::Auto if looks_like_packet() => DecodeAs::Packet,
        DecodeAs::Auto if bytes.len() == 4 => DecodeAs::Clcw,
        DecodeAs::Auto if bytes.len() == m.frames.length || bytes.len() == m.frames.length + 4 => DecodeAs::Frame,
        DecodeAs::Auto => DecodeAs::Packet,
        r => r,
    };
    match reading {
        DecodeAs::Packet | DecodeAs::Auto => match fenix_mib::packets::decode_packet(set, bytes, &m.profile, m.base) {
            Some(r) => r,
            None => unreadable("shorter than a packet header"),
        },
        DecodeAs::Clcw => match bytes.get(..4) {
            Some(b) => (frames::clcw_field(u32::from_be_bytes(b.try_into().unwrap_or([0; 4])), 0), None),
            None => unreadable("a CLCW is 4 octets"),
        },
        DecodeAs::Time => match time::decode(bytes, m.profile.tm_time, m.profile.epoch, &m.profile.leap, 0) {
            Some(d) => (d.field, None),
            None => unreadable("not a time in the project's format (ccsds.tm_time)"),
        },
        DecodeAs::Cfdp => match cfdp::decode(bytes) {
            Some(f) => (f, None),
            None => unreadable("shorter than a CFDP header"),
        },
        DecodeAs::Cltu => {
            let Some((cltu, sent)) = coding::cltu_decode(bytes, m.tc.bch_correct) else { return unreadable("no CLTU start sequence (EB90)") };
            let start = cltu.children.first().map(|f| f.bit / 8).unwrap_or(0);
            // Its layers side by side -- start sequence, TC frame, code
            // blocks, tail -- so the bytes take the frame's colour where
            // the frame is.
            let mut coding = cltu.children;
            let tail = coding.pop();
            let mut children = coding;
            // The randomizer ran over the frame before it was coded: it
            // comes off after the code blocks (the fill was never on it).
            let mut data = sent.clone();
            if m.tc.randomized {
                coding::derandomize(&mut data);
                children.insert(1, Field::new("de-randomized", (start + 2) * 8, 0, "", "pseudo-randomizer removed from the frame").linked(Link::Standard(coding::TC_STANDARD, "Pseudo-Randomizer")));
            }
            let mut def = None;
            if let Some((mut frame, info, bytes, problem)) = frames::tc_frame(&data, &m.tc) {
                if let Some(why) = problem {
                    frame.check = Some(Check::Bad(why));
                }
                let ctrl = bytes[0] & 0x10 != 0;
                if let Some(pkt) = bytes.get(info.data.clone()).filter(|_| !ctrl) {
                    if let Some((mut f, d)) = fenix_mib::packets::decode_packet(set, pkt, &m.profile, m.base) {
                        f.shift(info.data.start * 8);
                        frame.children.push(f);
                        def = d;
                    }
                }
                let fill = data.len() - bytes.len();
                if fill > 0 {
                    let all_fill = sent[bytes.len()..].iter().all(|b| *b == 0x55);
                    let f = Field::new("fill", bytes.len() * 8, fill * 8, format!("{fill} octets"), "after the frame, to fill the last code block")
                        .checked(if all_fill { Check::Ok("55".into()) } else { Check::Warn("not 0x55".into()) });
                    frame.children.push(f);
                }
                // The frame's octets are spread over the code blocks:
                // seven to a block, each block followed by its parity.
                frame.remap(&|o| coding::cltu_octet(start, o));
                let at = if m.tc.randomized { 2 } else { 1 };
                children.insert(at, frame);
            }
            children.extend(tail);
            (Field::group("CLTU", children), def)
        }
        DecodeAs::Frame => {
            let mut children = Vec::new();
            let mut at = 0;
            let mut frame = bytes.to_vec();
            if frame.starts_with(&coding::ASM) {
                children.push(Field::new("sync marker", 0, 32, "1ACFFC1D", "ASM").checked(Check::Ok("found".into())).linked(Link::Standard(coding::TM_STANDARD, "Attached Sync Marker")));
                frame.drain(..4);
                at = 4;
            }
            if m.frames.randomized {
                coding::derandomize(&mut frame);
                children.push(Field::new("de-randomized", at * 8, 0, "", "pseudo-randomizer removed").linked(Link::Standard(coding::TM_STANDARD, "Pseudo-Randomizer")));
            }
            if m.frames.rs_depth > 0 {
                if let Some((block, pad)) = fenix_ccsds::stream::rs_geometry(m.frames.length, m.frames.rs_depth) {
                    if frame.len() >= block {
                        frame.truncate(block);
                        let outcomes = fenix_ccsds::rs::decode_block(&mut frame, m.frames.rs_depth, pad);
                        let mut words = Vec::new();
                        for (w, o) in outcomes.iter().enumerate() {
                            let (text, check) = match o {
                                fenix_ccsds::rs::Outcome::Clean => ("no errors".to_string(), Check::Ok("clean".into())),
                                fenix_ccsds::rs::Outcome::Corrected(v) => (format!("{} symbol{} corrected", v.len(), if v.len() == 1 { "" } else { "s" }), Check::Warn(format!("corrected {}", v.len()))),
                                fenix_ccsds::rs::Outcome::Uncorrectable => ("beyond repair".to_string(), Check::Bad("more than 16 symbol errors".into())),
                            };
                            words.push(Field::new(format!("code word {}", w + 1), at * 8, 0, "", text).checked(check));
                        }
                        let mut rs = Field::group("Reed-Solomon (255,223)", words);
                        rs.bit = (at + m.frames.length) * 8;
                        rs.bits = 32 * m.frames.rs_depth * 8;
                        rs.link = Some(Link::Standard(fenix_ccsds::rs::STANDARD, "Reed-Solomon Coding"));
                        children.push(rs);
                        frame.truncate(m.frames.length);
                    }
                }
            }
            let Some((mut f, info)) = frames::decode(&frame, &m.frames) else { return unreadable("shorter than a frame header") };
            f.shift(at * 8);
            let mut def = None;
            if let (Some(fhp), Some(data)) = (info.first_header.filter(|h| *h != u16::MAX), frame.get(info.data.clone())) {
                let mut off = fhp as usize;
                while let Some(h) = data.get(off..).and_then(fenix_ccsds::PrimaryHeader::decode) {
                    let len = h.packet_len();
                    if !h.plausible() || off + len > data.len() {
                        break;
                    }
                    if let Some((mut p, d)) = fenix_mib::packets::decode_packet(set, &data[off..off + len], &m.profile, m.base) {
                        p.shift((at + info.data.start + off) * 8);
                        f.children.push(p);
                        def = def.or(d);
                    }
                    off += len;
                }
            }
            children.push(f);
            let title = children.last().map(|f| f.name.clone()).unwrap_or_default();
            (Field::group(title, children), def)
        }
    }
}

/// `K` on hex: a short reading of the packet or frame it spells -- or of
/// the on-board time it is, in the mission's format.
pub(crate) fn hover(line: &str, col: usize, set: Option<&MibSet>, mission: &Mission) -> Option<String> {
    if let Some(hover) = time_hover(line, col, mission) {
        return Some(hover);
    }
    let bytes = hex_at(line, col)?;
    if bytes.len() < 6 {
        return None;
    }
    let (root, def) = decode_bytes(&bytes, DecodeAs::Auto, set, mission);
    let walk = root.walk();
    let bad: Vec<String> = walk
        .iter()
        .filter_map(|(_, f)| match &f.check {
            Some(Check::Bad(m)) => Some(format!("{}: {m}", f.name)),
            _ => None,
        })
        .collect();
    let mut lines = vec![format!("{} · {} · {} bytes", root.name, root.value, bytes.len())];
    if let (Some(d), Some(set)) = (def, set) {
        let e = set.get(d);
        lines.push(format!("{} {} · {}", d.kind.tag(), e.name, if e.alias.is_empty() { &e.description } else { &e.alias }));
    }
    if let Some((_, t)) = walk.iter().find(|(_, f)| f.name.starts_with("time (")) {
        lines.push(t.value.clone());
    }
    lines.push(if bad.is_empty() { "every check passes".into() } else { bad.join("
") });
    lines.push(String::new());
    lines.push("SPC k d opens the decode".into());
    Some(lines.join("
"))
}

/// A word's hex digits, when it's all hex: `0x1B,` and `"1BF2"` are,
/// `send` isn't.
pub(crate) fn hex_word(w: &str) -> Option<String> {
    let w = w.trim_matches(|c: char| matches!(c, ',' | '"' | '\'' | '[' | ']' | '{' | '}'));
    let w = w.strip_prefix("0x").or_else(|| w.strip_prefix("0X")).unwrap_or(w);
    (!w.is_empty() && w.len().is_multiple_of(2) && w.chars().all(|c| c.is_ascii_hexdigit())).then(|| w.to_string())
}

/// The hex at column `col` of `line`: the run `SPC k d` reads there, else
/// the words around it that are all hex (`tc 1BF2C007`: `tc` ends in a
/// hex digit, so the run would take its `c`).
pub(crate) fn hex_at(line: &str, col: usize) -> Option<Vec<u8>> {
    if let Some((_, bytes)) = fenix_ccsds::hex::around(line, col) {
        return Some(bytes);
    }
    // Each word with the columns it spans.
    let mut words = Vec::new();
    let mut start = None;
    let chars: Vec<char> = line.chars().collect();
    for (i, c) in chars.iter().chain([&' ']).enumerate() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                words.push((s..i, chars[s..i].iter().collect::<String>()));
                start = None;
            }
            _ => {}
        }
    }
    let at = words.iter().position(|(cols, w)| cols.contains(&col) && hex_word(w).is_some())?;
    let first = (0..=at).rev().take_while(|&i| hex_word(&words[i].1).is_some()).last()?;
    let last = (at..words.len()).take_while(|&i| hex_word(&words[i].1).is_some()).last()?;
    let text: String = words[first..=last].iter().filter_map(|(_, w)| hex_word(w)).collect();
    fenix_ccsds::hex::parse(&text).filter(|b| b.len() >= 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hex_at_a_column_leaves_out_a_word_that_only_ends_in_hex() {
        let line = "tc 1BF2C007 0009 3FA3";
        assert_eq!(hex_at(line, 5), Some(vec![0x1B, 0xF2, 0xC0, 0x07, 0x00, 0x09, 0x3F, 0xA3]));
        assert_eq!(hex_at(line, 0), None, "tc isn't hex");
        assert_eq!(hex_at("  0BF2C123 ", 4), Some(vec![0x0B, 0xF2, 0xC1, 0x23]));
    }
}
