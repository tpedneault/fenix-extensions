//! The MIB applied to bytes: which TM packet (SPID) a packet is, from
//! `PID` and the identification fields `PIC` places; its parameters at
//! their `PLF` positions, read by type and calibrated, checked against
//! their limits; which telecommand a TC packet is and what its arguments
//! were; and, the other way, a telecommand's packet built from its
//! arguments.

use fenix_ccsds::field::{hex, Check, Field, Link};
use fenix_ccsds::pus::{self, Profile, Secondary};
use fenix_ccsds::{bits, Packet};

use crate::detail::{self, CalBody};
use crate::layout::{self, ElementKind, Layout};
use crate::values::{self, interpolate};
use crate::set::{CalKind, DefRef, Kind, MibSet};
use crate::types::{self, parse_int};

/// Where `PLF_OFFBY` counts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OffsetBase {
    /// After the primary header and the data field header (`PID_DFHSIZE`).
    #[default]
    AfterHeaders,
    /// From the packet's first octet.
    PacketStart,
}

impl OffsetBase {
    pub fn parse(s: &str) -> Result<OffsetBase, String> {
        match s.trim().to_ascii_lowercase().replace(['-', '_'], " ").as_str() {
            "after headers" | "headers" | "" => Ok(OffsetBase::AfterHeaders),
            "packet start" | "packet" | "start" => Ok(OffsetBase::PacketStart),
            other => Err(format!("{other}: after-headers or packet-start")),
        }
    }
}

/// The TM packet `pkt` is: the `PID` row whose type, subtype, APID and
/// identification values match, with PI1/PI2 read where `PIC` says.
pub fn identify_tm(set: &MibSet, pkt: &Packet, sec: Option<&Secondary>) -> Option<DefRef> {
    let (service, subtype) = sec.map(|s| (s.service as i64, s.subtype as i64)).unwrap_or((0, 0));
    let apid = pkt.header.apid as i64;
    let index = set.index();
    let pic = index.rows("pic").iter().find(|r| {
        parse_int(r.clean("PIC_TYPE")) == Some(service)
            && parse_int(r.clean("PIC_STYPE")) == Some(subtype)
            && r.get("PIC_APID").map(str::trim).filter(|a| !a.is_empty()).is_none_or(|a| parse_int(a) == Some(apid))
    });
    let read = |off: &str, wid: &str| -> Option<i64> {
        let off = parse_int(off)?;
        let wid = parse_int(wid)?;
        if off < 0 || wid <= 0 {
            return None;
        }
        bits::get(&pkt.bytes, off as usize * 8, wid as usize).map(|v| v as i64)
    };
    let pi1 = pic.and_then(|r| read(r.clean("PIC_PI1_OFF"), r.clean("PIC_PI1_WID")));
    let pi2 = pic.and_then(|r| read(r.clean("PIC_PI2_OFF"), r.clean("PIC_PI2_WID")));
    let mut best = None;
    for (i, e) in set.entries(Kind::TmPacket).iter().enumerate() {
        let r = &e.row;
        if parse_int(r.clean("PID_TYPE")) != Some(service) || parse_int(r.clean("PID_STYPE")) != Some(subtype) || parse_int(r.clean("PID_APID")) != Some(apid) {
            continue;
        }
        let want = |f: &str| parse_int(r.clean(f)).unwrap_or(0);
        let ok1 = pi1.is_none_or(|v| v == want("PID_PI1_VAL"));
        let ok2 = pi2.is_none_or(|v| v == want("PID_PI2_VAL"));
        if ok1 && ok2 {
            let def = DefRef { kind: Kind::TmPacket, index: i };
            // The default MIB wins a tie.
            if e.root == set.default_root() {
                return Some(def);
            }
            best.get_or_insert(def);
        }
    }
    best
}

/// The octet `PLF_OFFBY` counts from in `pkt`, for `spid`.
/// The octet a packet's `PLF_OFFBY` counts from: per the project's
/// setting, except that a packet read from XTCE counts from its start
/// (XTCE locations are from the container's start).
pub fn params_start(set: &MibSet, spid: DefRef, profile: &Profile, base: OffsetBase) -> usize {
    let row = &set.get(spid).row;
    if row.source.file.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml") || e.eq_ignore_ascii_case("xtce")) {
        return 0;
    }
    match base {
        OffsetBase::PacketStart => 0,
        OffsetBase::AfterHeaders => {
            let dfh = parse_int(set.get(spid).row.clean("PID_DFHSIZE")).filter(|n| *n > 0).map(|n| n as usize);
            6 + dfh.unwrap_or_else(|| pus::tm_header_len(profile))
        }
    }
}

/// A raw value read by type: the number (or text) and its display.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Unsigned(u64),
    Signed(i64),
    Real(f64),
    Bytes(Vec<u8>),
    Text(String),
}

impl Raw {
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Raw::Unsigned(v) => Some(v as f64),
            Raw::Signed(v) => Some(v as f64),
            Raw::Real(v) => Some(v),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Raw::Unsigned(v) => v.to_string(),
            Raw::Signed(v) => v.to_string(),
            Raw::Real(v) => format!("{v}"),
            Raw::Bytes(b) => hex(b),
            Raw::Text(t) => format!("\"{t}\""),
        }
    }
}

/// Reads a value of type `ptc`/`pfc`, `bits` wide, at bit `at`.
pub fn read_raw(bytes: &[u8], at: usize, ptc: &str, pfc: &str, bits_hint: Option<usize>) -> Option<(Raw, usize)> {
    let t = types::decode(ptc, pfc);
    let ptc_n = parse_int(ptc).unwrap_or(0);
    let width = t.bits.map(|b| b as usize).or(bits_hint)?;
    let raw = match ptc_n {
        1..=3 => Raw::Unsigned(bits::get(bytes, at, width)?),
        4 => Raw::Signed(bits::signed(bits::get(bytes, at, width)?, width)),
        5 if width == 32 => Raw::Real(f32::from_bits(bits::get(bytes, at, 32)? as u32) as f64),
        5 if width == 64 => Raw::Real(f64::from_bits(bits::get(bytes, at, 64)?)),
        8 => {
            let b = bytes.get(at / 8..(at + width) / 8)?;
            Raw::Text(String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
        }
        _ => Raw::Bytes(bytes.get(at / 8..(at + width).div_ceil(8))?.to_vec()),
    };
    Some((raw, width))
}

/// `raw` through calibration `cal`: engineering text, and the number
/// when there is one.
pub fn calibrate(set: &MibSet, cal: DefRef, raw: &Raw) -> Option<(String, Option<f64>)> {
    let x = raw.as_f64();
    let num = |pairs: &[(String, String)]| -> Vec<(f64, f64)> { pairs.iter().filter_map(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?))).collect() };
    match detail::calibration(set, cal) {
        CalBody::Points(p) => {
            let y = interpolate(&num(&p), x?)?;
            Some((trim_float(y), Some(y)))
        }
        CalBody::Texts(texts) => {
            let x = x?;
            texts.iter().find_map(|(range, text)| {
                let (lo, hi) = range.split_once("..").unwrap_or((range, range));
                let (lo, hi) = (parse_int(lo)? as f64, parse_int(hi)? as f64);
                (lo <= x && x <= hi).then(|| (text.clone(), None))
            })
        }
        CalBody::Coefficients(k) => {
            let x = x?;
            let a: Vec<f64> = k.iter().filter_map(|c| c.trim().parse().ok()).collect();
            let y = if set.get(cal).cal == Some(CalKind::Logarithmic) {
                let l = x.ln();
                let d: f64 = a.iter().enumerate().map(|(i, c)| c * l.powi(i as i32)).sum();
                1.0 / d
            } else {
                a.iter().enumerate().map(|(i, c)| c * x.powi(i as i32)).sum()
            };
            Some((trim_float(y), Some(y)))
        }
        CalBody::Ranges(_) => None,
    }
}

fn trim_float(y: f64) -> String {
    let s = format!("{y:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// The parameters of TM packet `spid` in `pkt`, read, calibrated and
/// checked.
pub fn decode_tm(set: &MibSet, spid: DefRef, pkt: &Packet, profile: &Profile, base: OffsetBase) -> Field {
    let e = set.get(spid);
    let start = params_start(set, spid, profile, base);
    let mut children = Vec::new();
    // (field, parameter, engineering value) of each limit to check.
    let mut checks = Vec::new();
    for slot in detail::packet_slots(set, spid) {
        let Some(param) = slot.param else {
            children.push(Field::new(&slot.name, 0, 0, "", "not in the MIB").checked(Check::Warn("no PCF row".into())));
            continue;
        };
        let pcf = &set.get(param).row;
        let occurrences = parse_int(&slot.occurrences).filter(|n| *n > 1).unwrap_or(1) as usize;
        let spacing = set.index().rows_by_field("plf", "PLF_NAME", &slot.name, Some(e.root)).into_iter().find(|r| r.clean("PLF_SPID") == e.name).and_then(|r| parse_int(r.clean("PLF_LGOCC"))).unwrap_or(0) as usize;
        for occ in 0..occurrences {
            let at = (start + slot.byte.unwrap_or(0).max(0) as usize) * 8 + slot.bit.unwrap_or(0).max(0) as usize + occ * spacing;
            let width_hint = parse_int(pcf.clean("PCF_WIDTH")).filter(|w| *w > 0).map(|w| w as usize);
            let name = if occurrences > 1 { format!("{} [{}]", slot.name, occ + 1) } else { slot.name.clone() };
            let Some((raw, width)) = read_raw(&pkt.bytes, at, pcf.clean("PCF_PTC"), pcf.clean("PCF_PFC"), width_hint) else {
                children.push(Field::new(name, at, 0, "", "past the end of the packet").checked(Check::Bad("packet too short".into())).linked(Link::Parameter(slot.name.clone())));
                continue;
            };
            let unit = pcf.clean("PCF_UNIT");
            let (value, eng) = match detail::tm_calibration(set, param).and_then(|c| calibrate(set, c, &raw)) {
                Some((text, n)) => (text, n),
                None => (raw.display(), raw.as_f64()),
            };
            let shown = if unit.is_empty() || eng.is_none() { value } else { format!("{value} {unit}") };
            if let Some(y) = eng {
                checks.push((children.len(), param, y));
            }
            children.push(Field::new(name, at, width, raw.display(), shown).linked(Link::Parameter(slot.name.clone())));
        }
    }
    // Limits, once every parameter is read: one that applies only while
    // another parameter has a value needs that value, wherever it is in
    // the packet.
    for (i, param, y) in checks {
        let mut check = None;
        // Limits that couldn't be checked: their parameter isn't here.
        let mut unknown: Vec<String> = Vec::new();
        for l in detail::limits(set, param) {
            let (Some(lo), Some(hi)) = (l.low.trim().parse::<f64>().ok(), l.high.trim().parse::<f64>().ok()) else { continue };
            if let Some((p, v)) = &l.when {
                // Skipped unless that parameter is in this packet with that value.
                let other = children.iter().find(|c| &c.name == p).map(|c| c.raw.clone());
                if other.is_none() && !unknown.contains(p) {
                    unknown.push(p.clone());
                }
                if other.as_deref() != Some(v.trim()) {
                    continue;
                }
            }
            if y < lo || y > hi {
                let c = format!("{} limit {lo}..{hi}", l.kind);
                check = Some(if l.kind == "hard" { Check::Bad(c) } else { Check::Warn(c) });
                break;
            }
            check.get_or_insert(Check::Ok(format!("within {} {lo}..{hi}", l.kind)));
        }
        if let (Some(Check::Ok(m)), false) = (&mut check, unknown.is_empty()) {
            m.push_str(&format!(" -- {} {NOT_HERE}", unknown.join(", ")));
        }
        children[i].check = check;
    }
    let mut g = Field::group(format!("SPID {} · {}", e.name, if e.alias.is_empty() { &e.description } else { &e.alias }), children);
    g.link = Some(Link::Spid(e.name.clone()));
    g.value = e.description.clone();
    g
}

/// Ends a limit check's message when a limit that depends on another
/// parameter couldn't be checked: that parameter isn't in the packet.
pub const NOT_HERE: &str = "isn't in this packet, so its limit isn't checked";

/// Which telecommand a TC packet is: same type, subtype and APID, and
/// every fixed argument where the packet has it.
pub fn identify_tc(set: &MibSet, pkt: &Packet, sec: Option<&Secondary>, profile: &Profile) -> Option<DefRef> {
    let (service, subtype) = sec.map(|s| (s.service as i64, s.subtype as i64))?;
    let app = pkt.bytes.get(6 + pus::tc_header_len(profile)..)?;
    let mut best: Option<(usize, DefRef)> = None;
    for (i, e) in set.entries(Kind::Telecommand).iter().enumerate() {
        let r = &e.row;
        if parse_int(r.clean("CCF_TYPE")) != Some(service) || parse_int(r.clean("CCF_STYPE")) != Some(subtype) || parse_int(r.clean("CCF_APID")) != Some(pkt.header.apid as i64) {
            continue;
        }
        let def = DefRef { kind: Kind::Telecommand, index: i };
        let mut matched = 0;
        let mut ok = true;
        let layout = Layout::of(set, def);
        for i in layout.static_prefix() {
            let el = &layout.elements[i];
            let (Some(want), Some(bit)) = (layout.fixed_raw_value(set, i), el.bit) else { continue };
            match bits::get(app, bit, el.bits.min(64)) {
                Some(have) if have == want => matched += 1,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && best.is_none_or(|(m, _)| matched > m) {
            best = Some((matched, def));
        }
    }
    best.map(|(_, d)| d)
}

/// A TC's application data, element by element, its groups repeated as
/// their counters in the packet say.
pub fn decode_tc(set: &MibSet, tc: DefRef, pkt: &Packet, profile: &Profile) -> Field {
    let e = set.get(tc);
    let head = 6 + pus::tc_header_len(profile);
    let app = pkt.bytes.get(head..).unwrap_or(&[]);
    let layout = Layout::of(set, tc);
    let mut children = Vec::new();
    for read in layout.decode(set, app, profile) {
        match read {
            Ok(placed) => {
                let el = &layout.elements[placed.element];
                let name = layout::Field { element: placed.element, path: placed.path.clone() }.label(&layout);
                let value = match &el.ty {
                    Some(t) => values::to_engineering(set, t, &placed.raw),
                    None => placed.raw.display(),
                };
                let mut f = Field::new(name, head * 8 + placed.bit, placed.bits, placed.raw.display(), value);
                if el.kind != ElementKind::Editable && !(el.kind == ElementKind::Area && el.value.is_empty()) {
                    let want = layout.fixed_raw_value(set, placed.element);
                    let have = match placed.raw {
                        Raw::Unsigned(v) => Some(v),
                        Raw::Signed(v) => Some(v as u64 & if placed.bits >= 64 { u64::MAX } else { (1 << placed.bits) - 1 }),
                        _ => None,
                    };
                    f.check = Some(if want.is_some() && want == have { Check::Ok(format!("fixed {}", el.value)) } else { Check::Bad(format!("fixed at {}", el.value)) });
                }
                if !el.name.is_empty() {
                    f.link = Some(Link::Parameter(el.name.clone()));
                }
                children.push(f);
            }
            Err((element, path, why)) => {
                let name = layout::Field { element, path }.label(&layout);
                children.push(Field::new(name, head * 8 + app.len() * 8, 0, "", "past the end").checked(Check::Bad(why)));
            }
        }
    }
    let mut g = Field::group(format!("{} · {}", e.name, e.description), children);
    g.link = Some(Link::Telecommand(e.name.clone()));
    g
}

/// A value typed for a TC parameter, as the raw number the packet takes:
/// a status text through its `PAS` table, an engineering number back
/// through its numeric calibration, else the number itself.
pub fn raw_argument(set: &MibSet, param: DefRef, value: &str) -> Result<u64, String> {
    let t = values::ParamType::of(set, param);
    match values::to_raw(set, &t, value, &Profile::default())? {
        Raw::Unsigned(v) => Ok(v),
        Raw::Signed(v) => Ok(v as u64 & t.bits().filter(|b| *b < 64).map_or(u64::MAX, |b| (1 << b) - 1)),
        Raw::Real(f) if t.bits() == Some(32) => Ok((f as f32).to_bits() as u64),
        Raw::Real(f) => Ok(f.to_bits()),
        _ => Err(format!("{} isn't a number", t.name)),
    }
}

/// Telecommand `tc`'s packet for `args` (`(parameter, value)` in the
/// form's order), with sequence count `seq`.
pub fn encode_tc(set: &MibSet, tc: DefRef, args: &[(String, String)], profile: &Profile, seq: u16) -> Result<Packet, String> {
    let e = set.get(tc);
    let r = &e.row;
    let (app, _) = Layout::of(set, tc).encode(set, args, profile)?;
    let apid = parse_int(r.clean("CCF_APID")).ok_or("no APID")? as u16;
    let service = parse_int(r.clean("CCF_TYPE")).ok_or("no type")? as u8;
    let subtype = parse_int(r.clean("CCF_STYPE")).ok_or("no subtype")? as u8;
    let ack = parse_int(r.clean("CCF_ACK")).unwrap_or(0b1001) as u8 & 0xF;
    Ok(pus::build_tc(profile, apid, seq, service, subtype, ack, &app))
}

/// Everything about a packet: PUS layers, then what the MIB says, as one
/// tree -- the inspector's.
pub fn decode_packet(set: Option<&MibSet>, bytes: &[u8], profile: &Profile, base: OffsetBase) -> Option<(Field, Option<DefRef>)> {
    let (pkt, sec, mut root) = pus::decode(bytes, profile)?;
    let Some(set) = set else { return Some((root, None)) };
    let (def, mib) = if pkt.header.is_tc {
        match identify_tc(set, &pkt, sec.as_ref(), profile) {
            Some(tc) => (Some(tc), Some(decode_tc(set, tc, &pkt, profile))),
            None => (None, None),
        }
    } else {
        match identify_tm(set, &pkt, sec.as_ref()) {
            Some(spid) => (Some(spid), Some(decode_tm(set, spid, &pkt, profile, base))),
            None => (None, None),
        }
    };
    match mib {
        Some(f) => {
            // The MIB's reading replaces the raw user data.
            root.children.retain(|c| c.name != "user data");
            let at = root.children.iter().position(|c| c.name == "packet error control").unwrap_or(root.children.len());
            root.children.insert(at, f);
        }
        None => {
            if let Some(u) = root.children.iter_mut().find(|c| c.name == "user data") {
                u.check = Some(Check::Warn("not identified in the MIB".into()));
            }
        }
    }
    Some((root, def))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::set::tests::{temp_root, write};

    const TM: [u8; 29] = [
        0x0B, 0xF2, 0xC1, 0x23, 0x00, 0x16, 0x20, 0x03, 0x19, 0x00, 0x42, 0x00, 0x00, 0x81, 0x4B, 0x87, 0x8A, 0x80, 0x00, 0x00, 0x01, 0x01, 0x0B, 0xB8, 0x0A, 0x28,
        0x02, 0x60, 0xE5,
    ];

    fn line(fields: &[&str], n: usize) -> String {
        let mut v: Vec<&str> = fields.to_vec();
        v.resize(n.max(fields.len()), "");
        v.join("\t")
    }

    /// The design document's mission: TM(3,25) SPID 30211 with SID 1 at
    /// octet 19, four parameters after it; ZTC08101 with a fixed function
    /// id, a line and a mode.
    pub(crate) fn mission() -> MibSet {
        let r = temp_root("packets");
        write(&r, "pic", &line(&["3", "25", "19", "16", "-1", "0"], 7));
        write(&r, "pid", &line(&["3", "25", "1010", "1", "0", "30211", "TCS fast housekeeping", "", "", "13", "Y", "", "", "1"], 16));
        write(&r, "tpcf", "30211\tTCS_HK_FAST\t29\n");
        write(
            &r,
            "pcf",
            &[
                line(&["NTH00100", "Thermistor bank powered", "", "", "3", "4", "", "", "", "S", "R", "TXF00006"], 24),
                line(&["NTH00123", "Heater 3 temperature", "", "degC", "3", "12", "", "", "", "N", "R", "CAF00310"], 24),
                line(&["NTH00124", "Heater 4 temperature", "", "degC", "3", "12", "", "", "", "N", "R", "CAF00310"], 24),
                line(&["NTH00201", "Heater 1 mode", "", "", "3", "4", "", "", "", "S", "R", "TXF00005"], 24),
            ]
            .join("\n"),
        );
        write(&r, "plf", "NTH00100\t30211\t2\t0\nNTH00123\t30211\t3\t0\nNTH00124\t30211\t5\t0\nNTH00201\t30211\t7\t0\n");
        write(&r, "caf", &line(&["CAF00310", "Thermistor curve", "R", "U", "D", "degC", "7"], 8));
        write(&r, "cap", "CAF00310\t0\t-40\nCAF00310\t600\t-20\nCAF00310\t1400\t5\nCAF00310\t2200\t25\nCAF00310\t3000\t45\nCAF00310\t3600\t65\nCAF00310\t4095\t85\n");
        write(&r, "txf", "TXF00005\tHeater mode\tU\t3\nTXF00006\tOn/off\tU\t2\n");
        write(&r, "txp", "TXF00005\t0\t0\tOFF\nTXF00005\t1\t1\tON\nTXF00005\t2\t2\tAUTO\nTXF00006\t0\t0\tOFF\nTXF00006\t1\t1\tON\n");
        write(&r, "ocp", "NTH00123\t1\tS\t-10\t40\t\t\nNTH00123\t2\tH\t-20\t60\t\t\n");
        write(&r, "ccf", &[line(&["ZTC08101", "Set heater control mode", "", "", "N", "", "8", "1", "1010", "3", "", "", "", "", "TCS", "", "", "", "", "9"], 21), line(&["ZTC08102", "Other function", "", "", "N", "", "8", "1", "1010", "1", "", "", "", "", "TCS", "", "", "", "", "9"], 21)].join("\n"));
        write(
            &r,
            "cdf",
            &[
                line(&["ZTC08101", "F", "Function id", "8", "0", "0", "PTC00001", "", "12"], 10),
                line(&["ZTC08101", "E", "Heater line", "8", "8", "0", "PTH00101"], 10),
                line(&["ZTC08101", "E", "Mode", "8", "16", "0", "PTH00102"], 10),
                line(&["ZTC08102", "F", "Function id", "8", "0", "0", "PTC00001", "", "13"], 10),
            ]
            .join("\n"),
        );
        write(&r, "cpc", &[line(&["PTC00001", "Function id", "3", "4"], 17), line(&["PTH00101", "Heater line", "3", "4"], 17), line(&["PTH00102", "Mode", "2", "8", "", "", "", "", "", "", "PAF00042"], 17)].join("\n"));
        write(&r, "paf", "PAF00042\tHeater mode\tU\t3\n");
        write(&r, "pas", "PAF00042\tOFF\t0\nPAF00042\tON\t1\nPAF00042\tAUTO\t2\n");
        MibSet::load(vec![r], None)
    }

    #[test]
    fn the_example_packet_is_identified_decoded_calibrated_and_limit_checked() {
        let set = mission();
        let (root, def) = decode_packet(Some(&set), &TM, &Profile::default(), OffsetBase::AfterHeaders).unwrap();
        assert_eq!(set.get(def.unwrap()).name, "30211");
        let f = |n: &str| root.find(n).unwrap().clone();
        assert_eq!(f("NTH00100").value, "ON");
        assert_eq!((f("NTH00123").raw.as_str(), f("NTH00123").value.as_str()), ("3000", "45.0 degC"));
        assert!(matches!(f("NTH00123").check, Some(Check::Warn(_))), "45 is over the soft 40");
        assert_eq!(f("NTH00124").value, "35.0 degC");
        assert_eq!(f("NTH00201").value, "AUTO");
        assert_eq!(f("NTH00123").bit, 22 * 8);
        assert!(root.find("user data").is_none());
        assert!(root.find("packet error control").unwrap().check == Some(Check::Ok("matches".into())));
    }

    #[test]
    fn a_limit_that_depends_on_a_later_parameter_still_applies() {
        // NTH00123's soft limit (-10..50) holds while NTH00201 is 2, and
        // NTH00201 comes after it in the packet.
        let ops = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ops");
        let set = MibSet::load(vec![crate::MibRoot { label: "OPS".into(), path: ops }], None);
        let packet = |raw: u16, mode: u8| {
            let mut b = vec![0x0B, 0xF2, 0xC1, 0x23, 0x00, 0x16, 0x20, 0x03, 0x19, 0x00, 0x42, 0x00, 0x00, 0x81, 0x4B, 0x87, 0x8A, 0x80, 0x00, 0x00, 0x01, 0x01];
            b.extend(raw.to_be_bytes());
            b.extend([0x0A, 0x28, mode]);
            let crc = fenix_ccsds::crc::ccitt16(&b);
            b.extend(crc.to_be_bytes());
            b
        };
        let check = |raw, mode| {
            let (root, _) = decode_packet(Some(&set), &packet(raw, mode), &Profile::default(), OffsetBase::AfterHeaders).unwrap();
            root.find("NTH00123").unwrap().check.clone()
        };
        assert_eq!(check(3153, 2), Some(Check::Warn("soft limit -10..50".into())), "AUTO: the soft limit");
        assert_eq!(check(3153, 1), Some(Check::Ok("within hard -20..60".into())), "ON: only the hard one");
        assert_eq!(check(2000, 2), Some(Check::Ok("within soft -10..50".into())));
        // The diagnostic packet (SID 2) doesn't carry NTH00201: its soft
        // limit can't be checked there, and says so.
        let mut diag = vec![0x0B, 0xF2, 0xC1, 0x24, 0x00, 0x1B, 0x20, 0x03, 0x19, 0x00, 0x43, 0x00, 0x00, 0x81, 0x4B, 0x87, 0x8A, 0x80, 0x00, 0x00, 0x02, 0x00];
        diag.extend(3153u16.to_be_bytes());
        diag.extend([0u8; 8]);
        let crc = fenix_ccsds::crc::ccitt16(&diag);
        diag.extend(crc.to_be_bytes());
        let (root, _) = decode_packet(Some(&set), &diag, &Profile::default(), OffsetBase::AfterHeaders).unwrap();
        assert_eq!(root.find("NTH00123").unwrap().check, Some(Check::Ok(format!("within hard -20..60 -- NTH00201 {NOT_HERE}"))));
    }

    #[test]
    fn a_tc_is_built_from_arguments_and_identified_back_by_its_fixed_values() {
        let set = mission();
        let tc = set.find(Kind::Telecommand, 0, "ZTC08101").unwrap();
        let args = vec![("PTH00101".to_string(), "3".to_string()), ("PTH00102".to_string(), "AUTO".to_string())];
        let pkt = encode_tc(&set, tc, &args, &Profile::default(), 7).unwrap();
        assert_eq!(pkt.bytes, vec![0x1B, 0xF2, 0xC0, 0x07, 0x00, 0x09, 0x29, 0x08, 0x01, 0x00, 0x00, 0x0C, 0x03, 0x02, 0x3F, 0xA3]);
        let (root, def) = decode_packet(Some(&set), &pkt.bytes, &Profile::default(), OffsetBase::AfterHeaders).unwrap();
        assert_eq!(set.get(def.unwrap()).name, "ZTC08101", "the fixed function id tells it from ZTC08102");
        assert_eq!(root.find("PTH00102").unwrap().value, "AUTO");
        assert!(encode_tc(&set, tc, &args[..1], &Profile::default(), 0).is_err());
    }

    #[test]
    fn a_grouped_tc_round_trips_and_is_identified_whatever_its_editable_defaults() {
        let set = crate::layout::tests::groups();
        let p = Profile::default();
        let tc = set.find(Kind::Telecommand, 0, "TC1").unwrap();
        let a: Vec<(String, String)> = [("N", "2"), ("A", "1"), ("A", "2"), ("C", "9")].iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        let pkt = encode_tc(&set, tc, &a, &p, 3).unwrap();
        let (root, def) = decode_packet(Some(&set), &pkt.bytes, &p, OffsetBase::AfterHeaders).unwrap();
        assert_eq!(def, Some(tc));
        assert_eq!(root.find("A[2]").unwrap().raw, "2");
        assert_eq!(root.find("B[2]").unwrap().check, Some(Check::Ok("fixed 7".into())));
        assert_eq!(root.find("C").unwrap().raw, "9");
        // TC2's first argument has a default in CDF_VALUE: another value
        // is still TC2, and its value is what was given.
        let tc2 = set.find(Kind::Telecommand, 0, "TC2").unwrap();
        let a: Vec<(String, String)> = [("A", "5"), ("M", "OFF")].iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        let pkt = encode_tc(&set, tc2, &a, &p, 0).unwrap();
        let (root, def) = decode_packet(Some(&set), &pkt.bytes, &p, OffsetBase::AfterHeaders).unwrap();
        assert_eq!(def, Some(tc2));
        assert_eq!((root.find("A").unwrap().raw.as_str(), root.find("M").unwrap().value.as_str()), ("5", "OFF"));
    }

    #[test]
    fn strings_octets_and_deduced_values_are_built() {
        let r = temp_root("packets-types");
        write(&r, "ccf", &line(&["TCS", "Types", "", "", "N", "", "20", "1", "100", "4"], 21));
        write(
            &r,
            "cdf",
            &[
                line(&["TCS", "E", "label", "8", "0", "0", "LBL"], 10),
                line(&["TCS", "E", "blob", "16", "8", "0", "BLB"], 10),
                line(&["TCS", "E", "param id", "16", "24", "0", "PID"], 10),
                line(&["TCS", "E", "value", "16", "40", "0", "VAL"], 10),
            ]
            .join("\n"),
        );
        write(&r, "cpc", &[line(&["LBL", "label", "8", "0"], 17), line(&["BLB", "blob", "7", "2"], 17), line(&["PID", "param", "3", "12", "", "", "", "P"], 17), line(&["VAL", "value", "11", "0"], 17)].join("\n"));
        write(&r, "pcf", &line(&["NTEMP", "temp", "7", "", "4", "12"], 24));
        let set = MibSet::load(vec![r], None);
        let tc = set.find(Kind::Telecommand, 0, "TCS").unwrap();
        let p = Profile::default();
        let a: Vec<(String, String)> = [("LBL", "\"AB\""), ("BLB", "0xCAFE"), ("PID", "7"), ("VAL", "-2")].iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        let (app, _) = Layout::of(&set, tc).encode(&set, &a, &p).unwrap();
        // A length octet and "AB", the two octets, PCF_PID 7, then -2 as NTEMP's int16.
        assert_eq!(app, vec![2, b'A', b'B', 0xCA, 0xFE, 0x00, 0x07, 0xFF, 0xFE]);
        let back = Layout::of(&set, tc).decode(&set, &app, &p);
        let raws: Vec<String> = back.into_iter().map(|r| r.unwrap().raw.display()).collect();
        assert_eq!(raws, vec!["\"AB\"", "CA FE", "7", "-2"]);
    }

    #[test]
    fn calibrations_interpolate_and_polynomials_evaluate() {
        assert_eq!(interpolate(&[(0.0, 0.0), (10.0, 100.0)], 2.5), Some(25.0));
        assert_eq!(interpolate(&[(0.0, 0.0), (10.0, 100.0)], 20.0), Some(200.0), "extrapolates the last segment");
        assert_eq!(trim_float(45.0), "45.0");
        assert_eq!(trim_float(35.25), "35.25");
        assert_eq!(OffsetBase::parse("packet-start"), Ok(OffsetBase::PacketStart));
    }
}
