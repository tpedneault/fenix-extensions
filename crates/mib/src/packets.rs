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
    if row.source().file.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml") || e.eq_ignore_ascii_case("xtce")) {
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

/// One value a TM packet was read for, before calibration.
enum TmRead {
    /// A parameter the packet places that the MIB doesn't define.
    Missing(String),
    /// Past the end of the packet.
    Short { name: String, at: usize, param: String },
    Value { name: String, at: usize, width: usize, raw: Raw, param: DefRef },
}

/// How deep packets carried in packets (`VPD_PIDREF`) are followed.
const MAX_EMBEDDED: usize = 4;

/// Reads a variable packet's `VPD` entries one after another.
struct VpdReader<'a> {
    set: &'a MibSet,
    root: usize,
    bytes: &'a [u8],
    cursor: usize,
    reads: Vec<TmRead>,
    /// The last value of an entry whose value names a parameter (`VPD_CHOICE`).
    choice: Option<Raw>,
    failed: bool,
    depth: usize,
}

fn indexed(name: &str, path: &[usize]) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{name}[{}]", path.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("."))
    }
}

/// A deduced (PTC 11) parameter's type: the TM parameter's that `named`
/// names, by name or `PCF_PID`.
fn deduced_tm_type(set: &MibSet, root: usize, named: &Raw) -> Option<(String, String)> {
    let index = set.index();
    let row = match named {
        Raw::Text(name) => index.first_row_by_field("pcf", "PCF_NAME", name.trim(), Some(root)),
        Raw::Unsigned(n) => index.first_row_by_field("pcf", "PCF_PID", &n.to_string(), Some(root)).or_else(|| index.first_row_by_field("pcf", "PCF_NAME", &n.to_string(), Some(root))),
        _ => None,
    }?;
    Some((row.clean("PCF_PTC").to_string(), row.clean("PCF_PFC").to_string()))
}

impl VpdReader<'_> {
    fn read(&mut self, slots: &[detail::PacketSlot], start: usize, end: usize, path: &[usize]) {
        let mut i = start;
        while i < end && !self.failed {
            let s = &slots[i];
            if s.group > 0 {
                let body_end = (i + 1 + s.group).min(end);
                let times = if s.repeat > 0 {
                    // A fixed repetition: the counter isn't in the packet,
                    // but the gap before it is.
                    self.cursor = (self.cursor as i64 + s.gap).max(0) as usize;
                    s.repeat
                } else {
                    match self.entry(s, path) {
                        Some(raw) => raw.as_f64().unwrap_or(0.0).max(0.0) as usize,
                        None => return,
                    }
                };
                for rep in 1..=times.min(layout::MAX_REPEAT) {
                    let mut p = path.to_vec();
                    p.push(rep);
                    self.read(slots, i + 1, body_end, &p);
                    if self.failed {
                        return;
                    }
                }
                i = body_end;
            } else {
                if s.repeat > 1 {
                    for rep in 1..=s.repeat.min(layout::MAX_REPEAT) {
                        let mut p = path.to_vec();
                        p.push(rep);
                        if self.entry(s, &p).is_none() {
                            return;
                        }
                    }
                } else if self.entry(s, path).is_none() {
                    return;
                }
                i += 1;
            }
        }
    }

    /// Reads one entry at the cursor; `None` when it can't be.
    fn entry(&mut self, s: &detail::PacketSlot, path: &[usize]) -> Option<Raw> {
        let name = indexed(&s.name, path);
        let Some(param) = s.param else {
            self.reads.push(TmRead::Missing(name));
            // Its width is unknown: nothing after it can be placed.
            self.failed = true;
            return None;
        };
        let pcf = &self.set.get(param).row;
        self.cursor = (self.cursor as i64 + s.gap).max(0) as usize;
        let at = self.cursor;
        let (mut ptc, mut pfc) = (pcf.clean("PCF_PTC").to_string(), pcf.clean("PCF_PFC").to_string());
        if ptc == "11" {
            if let Some((p, f)) = self.choice.as_ref().and_then(|c| deduced_tm_type(self.set, self.root, c)) {
                (ptc, pfc) = (p, f);
            }
        }
        let hint = parse_int(pcf.clean("PCF_WIDTH")).filter(|w| *w > 0).map(|w| w as usize);
        let read = if matches!(ptc.as_str(), "7" | "8") && pfc == "0" && hint.is_none() {
            // Variable length: a length octet, then the octets.
            bits::get(self.bytes, at, 8).and_then(|n| {
                let n = n as usize;
                let b = self.bytes.get((at + 8) / 8..(at + 8) / 8 + n).filter(|_| at.is_multiple_of(8))?.to_vec();
                Some((if ptc == "8" { Raw::Text(String::from_utf8_lossy(&b).to_string()) } else { Raw::Bytes(b) }, 8 + n * 8))
            })
        } else {
            read_raw(self.bytes, at, &ptc, &pfc, hint)
        };
        let Some((raw, width)) = read else {
            self.reads.push(TmRead::Short { name, at, param: s.name.clone() });
            self.failed = true;
            return None;
        };
        self.cursor = at + width;
        if s.choice {
            self.choice = Some(raw.clone());
        }
        self.reads.push(TmRead::Value { name, at, width, raw: raw.clone(), param });
        if s.pidref {
            self.embedded(&raw);
        }
        Some(raw)
    }

    /// The packet `value` identifies, read where the cursor is: a
    /// variable one by its `VPD` entries, a fixed one by its `PLF`
    /// positions from here.
    fn embedded(&mut self, value: &Raw) {
        if self.depth >= MAX_EMBEDDED {
            return;
        }
        let Some(id) = value.as_f64().map(|v| v as i64) else { return };
        let set = self.set;
        let found = set.entries(Kind::TmPacket).iter().enumerate().find(|(_, p)| p.root == self.root && parse_int(&p.name) == Some(id)).or_else(|| {
            set.entries(Kind::TmPacket).iter().enumerate().find(|(_, p)| p.root == self.root && parse_int(p.row.clean("PID_TPSD")) == Some(id))
        });
        let Some((index, _)) = found else { return };
        let def = DefRef { kind: Kind::TmPacket, index };
        let slots = detail::packet_slots(set, def);
        self.depth += 1;
        if detail::tpsd(set, def).is_some() {
            self.read(&slots, 0, slots.len(), &[]);
        } else {
            let base = self.cursor;
            let mut end = base;
            for s in &slots {
                let Some(param) = s.param else {
                    self.reads.push(TmRead::Missing(s.name.clone()));
                    continue;
                };
                let pcf = &set.get(param).row;
                let at = base + s.byte.unwrap_or(0).max(0) as usize * 8 + s.bit.unwrap_or(0).max(0) as usize;
                let hint = parse_int(pcf.clean("PCF_WIDTH")).filter(|w| *w > 0).map(|w| w as usize);
                match read_raw(self.bytes, at, pcf.clean("PCF_PTC"), pcf.clean("PCF_PFC"), hint) {
                    Some((raw, width)) => {
                        end = end.max(at + width);
                        self.reads.push(TmRead::Value { name: s.name.clone(), at, width, raw, param });
                    }
                    None => self.reads.push(TmRead::Short { name: s.name.clone(), at, param: s.name.clone() }),
                }
            }
            self.cursor = end;
        }
        self.depth -= 1;
    }
}

/// A fixed packet's values, at their `PLF` positions.
fn read_fixed(set: &MibSet, spid: DefRef, bytes: &[u8], start: usize) -> Vec<TmRead> {
    let e = set.get(spid);
    let mut reads = Vec::new();
    for slot in detail::packet_slots(set, spid) {
        let Some(param) = slot.param else {
            reads.push(TmRead::Missing(slot.name.clone()));
            continue;
        };
        let pcf = &set.get(param).row;
        let occurrences = parse_int(&slot.occurrences).filter(|n| *n > 1).unwrap_or(1) as usize;
        let spacing = set.index().rows_by_field("plf", "PLF_NAME", &slot.name, Some(e.root)).into_iter().find(|r| r.clean("PLF_SPID") == e.name).and_then(|r| parse_int(r.clean("PLF_LGOCC"))).unwrap_or(0) as usize;
        for occ in 0..occurrences {
            let at = (start + slot.byte.unwrap_or(0).max(0) as usize) * 8 + slot.bit.unwrap_or(0).max(0) as usize + occ * spacing;
            let width_hint = parse_int(pcf.clean("PCF_WIDTH")).filter(|w| *w > 0).map(|w| w as usize);
            let name = if occurrences > 1 { format!("{} [{}]", slot.name, occ + 1) } else { slot.name.clone() };
            match read_raw(bytes, at, pcf.clean("PCF_PTC"), pcf.clean("PCF_PFC"), width_hint) {
                Some((raw, width)) => reads.push(TmRead::Value { name, at, width, raw, param }),
                None => reads.push(TmRead::Short { name, at, param: slot.name.clone() }),
            }
        }
    }
    reads
}

/// The raw value of parameter `name` in a packet's values: its own, or
/// its first occurrence's.
fn raw_of<'a>(reads: &'a [TmRead], name: &str) -> Option<&'a Raw> {
    let first = |exact: bool| {
        reads.iter().find_map(|r| match r {
            TmRead::Value { name: n, raw, .. } if (exact && n == name) || (!exact && (n.starts_with(&format!("{name}[")) || n.starts_with(&format!("{name} [")))) => Some(raw),
            _ => None,
        })
    };
    first(true).or_else(|| first(false))
}

/// The calibration `CUR` selects for `param` from the packet's other
/// values: the first row, by `CUR_POS`, whose `CUR_RLCHK` parameter has
/// the raw value `CUR_VALPAR`. `None`: no row applies -- `PCF_CURTX` does.
pub(crate) fn conditional_calibration(set: &MibSet, param: DefRef, value_of: impl Fn(&str) -> Option<Raw>) -> Option<DefRef> {
    let e = set.get(param);
    let mut rows = set.index().rows_by_field("cur", "CUR_PNAME", &e.name, Some(e.root));
    if rows.is_empty() {
        return None;
    }
    rows.sort_by_key(|r| parse_int(r.clean("CUR_POS")).unwrap_or(i64::MAX));
    let text = e.row.clean("PCF_CATEG") == "S";
    for r in rows {
        let Some(raw) = value_of(r.clean("CUR_RLCHK")) else { continue };
        let want = r.clean("CUR_VALPAR");
        let matches = match (raw.as_f64(), values::parse_number(want)) {
            (Some(have), Some(want)) => have == want,
            _ => raw.display().trim_matches('"') == want,
        };
        if !matches {
            continue;
        }
        let id = r.clean("CUR_SELECT");
        let kinds: &[CalKind] = if text { &[CalKind::Text, CalKind::Numeric, CalKind::Polynomial, CalKind::Logarithmic] } else { &[CalKind::Numeric, CalKind::Polynomial, CalKind::Logarithmic, CalKind::Text] };
        if let Some(c) = kinds.iter().find_map(|k| set.find_cal(e.root, *k, id)) {
            return Some(c);
        }
    }
    None
}

/// The parameters of TM packet `spid` in `pkt`, read, calibrated and
/// checked. A variable packet (`PID_TPSD`, `VPD`) is read entry by entry,
/// its groups repeated as their counters say; a parameter with `CUR`
/// rows is calibrated as the packet's other values select.
pub fn decode_tm(set: &MibSet, spid: DefRef, pkt: &Packet, profile: &Profile, base: OffsetBase) -> Field {
    let e = set.get(spid);
    let start = params_start(set, spid, profile, base);
    let reads = match detail::tpsd(set, spid) {
        Some(_) => {
            let slots = detail::packet_slots(set, spid);
            let mut r = VpdReader { set, root: e.root, bytes: &pkt.bytes, cursor: start * 8, reads: Vec::new(), choice: None, failed: false, depth: 0 };
            r.read(&slots, 0, slots.len(), &[]);
            r.reads
        }
        None => read_fixed(set, spid, &pkt.bytes, start),
    };
    let mut children = Vec::new();
    // (field, parameter, engineering value) of each limit to check.
    let mut checks = Vec::new();
    for read in &reads {
        match read {
            TmRead::Missing(name) => children.push(Field::new(name, 0, 0, "", "not in the MIB").checked(Check::Warn("no PCF row".into()))),
            TmRead::Short { name, at, param } => {
                children.push(Field::new(name, *at, 0, "", "past the end of the packet").checked(Check::Bad("packet too short".into())).linked(Link::Parameter(param.clone())));
            }
            TmRead::Value { name, at, width, raw, param } => {
                let pcf = &set.get(*param).row;
                let pname = pcf.clean("PCF_NAME").to_string();
                let unit = pcf.clean("PCF_UNIT");
                let cal = conditional_calibration(set, *param, |n| raw_of(&reads, n).cloned()).or_else(|| detail::tm_calibration(set, *param));
                let (value, eng) = match cal.and_then(|c| calibrate(set, c, raw)) {
                    Some((text, n)) => (text, n),
                    None => (raw.display(), raw.as_f64()),
                };
                let shown = if unit.is_empty() || eng.is_none() { value } else { format!("{value} {unit}") };
                if let Some(y) = eng {
                    checks.push((children.len(), *param, y));
                }
                children.push(Field::new(name, *at, *width, raw.display(), shown).linked(Link::Parameter(pname)));
            }
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
                    Some(t) => values::time_text(t, &placed.raw, profile).unwrap_or_else(|| values::to_engineering(set, t, &placed.raw)),
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

    /// SPID 500: a variable packet (structure 7) whose counter N repeats
    /// A, a nested counter M and the B it repeats, then C, calibrated as
    /// N selects (`CUR`). SPID 501 (structure 8): a group repeated a
    /// fixed twice, its counter not in the packet.
    fn variable() -> MibSet {
        let r = temp_root("packets-vpd");
        write(&r, "pid", &[line(&["3", "25", "100", "0", "0", "500", "Variable", "", "7"], 16), line(&["3", "26", "100", "0", "0", "501", "Fixed repeat", "", "8"], 16)].join("\n"));
        write(
            &r,
            "vpd",
            &[
                line(&["7", "1", "N", "3", "0", "N", "N", "", "", "", "", "", "", "48"], 14),
                line(&["7", "2", "A"], 14),
                line(&["7", "3", "M", "1"], 14),
                line(&["7", "4", "B"], 14),
                line(&["7", "5", "C"], 14),
                line(&["8", "1", "D", "1", "2", "", "", "", "", "", "", "", "", "48"], 14),
                line(&["8", "2", "A"], 14),
            ]
            .join("\n"),
        );
        let u8p = |n: &str| line(&[n, n, "", "", "3", "4"], 24);
        write(&r, "pcf", &[u8p("N"), u8p("A"), u8p("M"), u8p("B"), u8p("D"), line(&["C", "c", "", "V", "3", "12", "", "", "", "N", "R", "CAF1"], 24)].join("\n"));
        write(&r, "caf", &[line(&["CAF1", "one", "R", "U", "D", "", "2"], 8), line(&["CAF2", "two", "R", "U", "D", "", "2"], 8)].join("\n"));
        write(&r, "cap", "CAF1\t0\t0\nCAF1\t100\t100\nCAF2\t0\t0\nCAF2\t100\t200\n");
        write(&r, "cur", "C\t2\tN\t1\tCAF1\nC\t1\tN\t2\tCAF2\n");
        MibSet::load(vec![r], None)
    }

    fn decoded(set: &MibSet, spid: &str, data: &[u8]) -> Field {
        let mut bytes = vec![0x08, 0x64, 0xC0, 0x00, 0x00, 0x00];
        bytes.extend_from_slice(data);
        let len = (bytes.len() - 7) as u16;
        bytes[4..6].copy_from_slice(&len.to_be_bytes());
        let (pkt, _) = Packet::parse(&bytes).unwrap();
        decode_tm(set, set.find(Kind::TmPacket, 0, spid).unwrap(), &pkt, &Profile::default(), OffsetBase::PacketStart)
    }

    #[test]
    fn a_variable_packet_is_read_entry_by_entry_its_groups_repeated() {
        let set = variable();
        let f = decoded(&set, "500", &[2, 10, 1, 20, 11, 2, 21, 22, 0, 100]);
        let got: Vec<String> = f.children.iter().map(|c| format!("{}={}", c.name, c.raw)).collect();
        assert_eq!(got, vec!["N=2", "A[1]=10", "M[1]=1", "B[1.1]=20", "A[2]=11", "M[2]=2", "B[2.1]=21", "B[2.2]=22", "C=100"]);
        assert_eq!(f.find("C").unwrap().bit, 8 * 8 + 48, "after the six octets VPD_OFFSET skips and the groups");
        let fixed = decoded(&set, "501", &[1, 2]);
        assert_eq!(fixed.children.iter().map(|c| format!("{}={}", c.name, c.raw)).collect::<Vec<_>>(), vec!["A[1]=1", "A[2]=2"]);
        let short = decoded(&set, "500", &[3, 1]);
        assert!(short.children.iter().any(|c| c.check == Some(Check::Bad("packet too short".into()))));
        // Its entries, as the packet's page lists them.
        let slots = detail::packet_slots(&set, set.find(Kind::TmPacket, 0, "500").unwrap());
        assert_eq!(slots.iter().map(|s| (s.name.as_str(), s.group, s.within.clone())).collect::<Vec<_>>(), vec![("N", 3, vec![]), ("A", 0, vec![0]), ("M", 1, vec![0]), ("B", 0, vec![2, 0]), ("C", 0, vec![])]);
        assert_eq!((slots[0].byte, slots[1].byte), (Some(6), None), "only what comes before a group has a certain place");
        let a = set.find(Kind::TmParam, 0, "A").unwrap();
        assert_eq!(detail::carried_by(&set, a).len(), 2, "both variable packets carry A");
    }

    #[test]
    fn cur_selects_the_calibration_from_the_packets_values() {
        let set = variable();
        let c = |n: u8| {
            let mut data = vec![n];
            for _ in 0..n {
                data.extend([0, 0]);
            }
            data.extend([0, 50]);
            decoded(&set, "500", &data).find("C").unwrap().value.clone()
        };
        assert_eq!(c(2), "100.0 V", "N = 2: CAF2 doubles");
        assert_eq!(c(1), "50.0 V", "N = 1: CAF1");
        assert_eq!(c(0), "50.0 V", "no CUR row applies: PCF_CURTX");
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
