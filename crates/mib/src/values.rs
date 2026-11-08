//! A TC parameter's values, both ways: what someone types (its
//! engineering value -- a status text, a number in engineering units, a
//! time, a string) to the raw value the packet carries, and back. The
//! `CPC` row says how: its type (`CPC_PTC`/`CPC_PFC`), the radix raw
//! values are written in (`CPC_RADIX`), and its calibrations -- status
//! texts (`PAF`/`PAS`), a numeric curve (`CCA`/`CCS`) and the range
//! check (`PRF`/`PRV`, in raw or engineering units as `PRF_INTER` says).

use fenix_ccsds::pus::Profile;
use fenix_ccsds::time::{self, Tai, TimeFormat};

use crate::detail::{self, CalBody};
use crate::packets::Raw;
use crate::row::Row;
use crate::set::{CalKind, DefRef, MibSet};
use crate::types;

/// How the MIB writes a raw value: `CPC_RADIX`, `PRF_RADIX`, `CCA_RADIX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Radix {
    #[default]
    Decimal,
    Hex,
    Octal,
}

impl Radix {
    pub fn of(flag: &str) -> Radix {
        match flag.trim() {
            "H" | "h" => Radix::Hex,
            "O" | "o" => Radix::Octal,
            _ => Radix::Decimal,
        }
    }
}

/// An integer as the MIB writes a raw value in `radix`: `0x`/`0o`/`0b`
/// prefixes always mean what they say, bare digits are in `radix`.
pub fn parse_raw_int(text: &str, radix: Radix) -> Option<i128> {
    let t = text.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let lower = t.to_ascii_lowercase();
    let (digits, base) = if let Some(h) = lower.strip_prefix("0x") {
        (h.to_string(), 16)
    } else if let Some(o) = lower.strip_prefix("0o") {
        (o.to_string(), 8)
    } else if let Some(b) = lower.strip_prefix("0b") {
        (b.to_string(), 2)
    } else {
        (
            lower.clone(),
            match radix {
                Radix::Decimal => 10,
                Radix::Hex => 16,
                Radix::Octal => 8,
            },
        )
    };
    let digits = digits.replace('_', "");
    if digits.is_empty() {
        return None;
    }
    let v = i128::from_str_radix(&digits, base).ok()?;
    Some(if neg { -v } else { v })
}

/// A number as someone types an engineering value: an integer in any of
/// `parse_raw_int`'s forms (decimal unless prefixed), or a decimal with
/// a fraction or exponent.
pub fn parse_number(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Some(v) = parse_raw_int(t, Radix::Decimal) {
        return Some(v as f64);
    }
    let v: f64 = t.parse().ok()?;
    v.is_finite().then_some(v)
}

/// How a value in the MIB is to be read: `CDF_INTER`, `CPC_INTER`,
/// `PRF_INTER`, `PVS_INTER`, `PTV_INTER`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interp {
    Raw,
    Engineering,
    /// The value is a TM parameter's, at the time the command is sent
    /// (`CDF_INTER` `D`/`T`, with `CDF_TMID`).
    Dynamic,
}

impl Interp {
    /// The flag as the MIB writes it; the ICD's default is raw.
    pub fn of(flag: &str) -> Interp {
        match flag.trim() {
            "E" | "e" => Interp::Engineering,
            "D" | "d" | "T" | "t" => Interp::Dynamic,
            _ => Interp::Raw,
        }
    }
}

/// A TC parameter's type and calibrations, from its `CPC` row.
#[derive(Debug, Clone)]
pub struct ParamType {
    pub name: String,
    pub ptc: u32,
    pub pfc: u32,
    pub radix: Radix,
    pub categ: String,
    pub unit: String,
    pub status: Option<DefRef>,
    pub numeric: Option<DefRef>,
    pub range: Option<DefRef>,
    pub root: usize,
}

impl ParamType {
    pub fn of(set: &MibSet, param: DefRef) -> ParamType {
        let e = set.get(param);
        Self::from_row(set, e.root, &e.row)
    }

    pub fn from_row(set: &MibSet, root: usize, cpc: &Row) -> ParamType {
        ParamType {
            name: cpc.clean("CPC_NAME").to_string(),
            ptc: cpc.clean("CPC_PTC").parse().unwrap_or(0),
            pfc: cpc.clean("CPC_PFC").parse().unwrap_or(0),
            radix: Radix::of(cpc.clean("CPC_RADIX")),
            categ: cpc.clean("CPC_CATEG").to_string(),
            unit: cpc.clean("CPC_UNIT").to_string(),
            status: set.find_cal(root, CalKind::Status, cpc.clean("CPC_PAFREF")),
            numeric: set.find_cal(root, CalKind::TcNumeric, cpc.clean("CPC_CCAREF")),
            range: set.find_cal(root, CalKind::Range, cpc.clean("CPC_PRFREF")),
            root,
        }
    }

    /// A type of its own, for a deduced parameter's value (its type is
    /// the TM parameter's an earlier argument names).
    pub fn plain(name: &str, ptc: u32, pfc: u32, root: usize) -> ParamType {
        ParamType { name: name.to_string(), ptc, pfc, radix: Radix::Decimal, categ: String::new(), unit: String::new(), status: None, numeric: None, range: None, root }
    }

    pub fn type_name(&self) -> String {
        types::decode(&self.ptc.to_string(), &self.pfc.to_string()).name
    }

    /// Its width in bits, when the type fixes one.
    pub fn bits(&self) -> Option<usize> {
        types::decode(&self.ptc.to_string(), &self.pfc.to_string()).bits.map(|b| b as usize)
    }

    /// Whether its value is a number someone types.
    pub fn is_numeric(&self) -> bool {
        matches!(self.ptc, 1..=6 | 10)
    }

    /// Variable-length octets or characters: a length octet, then them.
    pub fn variable_length(&self) -> bool {
        matches!(self.ptc, 6..=8) && self.pfc == 0
    }

    pub fn deduced(&self) -> bool {
        self.ptc == 11
    }

    /// `CPC_CATEG` `P`: its value names a parameter (the one a deduced
    /// parameter after it takes its type from).
    pub fn names_a_parameter(&self) -> bool {
        self.categ == "P"
    }

    fn texts(&self, set: &MibSet) -> Vec<(String, String)> {
        match self.status.map(|c| detail::calibration(set, c)) {
            Some(CalBody::Texts(t)) => t,
            _ => Vec::new(),
        }
    }

    /// The status texts it may be.
    pub fn aliases(&self, set: &MibSet) -> Vec<String> {
        self.texts(set).into_iter().map(|(_, t)| t).filter(|t| !t.is_empty()).collect()
    }

    /// The numeric curve, as (raw, engineering) points.
    fn curve(&self, set: &MibSet) -> Vec<(f64, f64)> {
        match self.numeric.map(|c| detail::calibration(set, c)) {
            Some(CalBody::Points(p)) => p.iter().filter_map(|(raw, eng)| Some((parse_number(raw)?, parse_number(eng)?))).collect(),
            _ => Vec::new(),
        }
    }
}

/// A TC parameter's type with its calibrations read out: all that's
/// needed to turn a typed value into raw and back, and to check it,
/// without the set.
#[derive(Debug, Clone)]
pub struct Domain {
    pub ty: ParamType,
    /// (raw, text) of its status calibration.
    pub texts: Vec<(String, String)>,
    /// (raw, engineering) points of its numeric curve.
    pub curve: Vec<(f64, f64)>,
    pub allowed: Vec<Allowed>,
}

impl Domain {
    pub fn of(set: &MibSet, ty: &ParamType) -> Domain {
        Domain { texts: ty.texts(set), curve: ty.curve(set), allowed: allowed(set, ty), ty: ty.clone() }
    }

    /// A type with no calibration.
    pub fn plain(ty: ParamType) -> Domain {
        Domain { ty, texts: Vec::new(), curve: Vec::new(), allowed: Vec::new() }
    }

    pub fn param(set: &MibSet, param: DefRef) -> Domain {
        Domain::of(set, &ParamType::of(set, param))
    }

    pub fn aliases(&self) -> Vec<String> {
        self.texts.iter().map(|(_, t)| t.clone()).filter(|t| !t.is_empty()).collect()
    }

    pub fn to_raw(&self, text: &str, profile: &Profile) -> Result<Raw, String> {
        raw_of(self, text, profile)
    }

    pub fn to_engineering(&self, raw: &Raw) -> String {
        engineering_of(self, raw)
    }

    pub fn check(&self, value: &str, profile: &Profile) -> Vec<String> {
        check_of(self, value, profile)
    }

    /// What it may be, for a person: its texts, else its ranges.
    pub fn allowed_text(&self) -> String {
        let aliases = self.aliases();
        if !aliases.is_empty() {
            return aliases.join(" ");
        }
        self.allowed.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    }

    /// A value the MIB gives, as someone types it (see
    /// `mib_value_as_engineering`).
    pub fn mib_value(&self, value: &str, inter: &Interp) -> String {
        let t = &self.ty;
        let v = value.trim();
        if v.is_empty() || *inter != Interp::Raw || !t.is_numeric() {
            return v.to_string();
        }
        match parse_raw_int(v, t.radix) {
            Some(n) if self.texts.is_empty() && self.curve.is_empty() => n.to_string(),
            Some(n) => self.to_engineering(&if n < 0 { Raw::Signed(n as i64) } else { Raw::Unsigned(n as u64) }),
            None => v.to_string(),
        }
    }

    /// A value the MIB gives, as raw.
    pub fn mib_raw(&self, value: &str, inter: &Interp, profile: &Profile) -> Result<Raw, String> {
        let t = &self.ty;
        if *inter == Interp::Raw && t.is_numeric() {
            if let Some(n) = parse_raw_int(value, t.radix) {
                return Ok(if n < 0 { Raw::Signed(n as i64) } else { Raw::Unsigned(n as u64) });
            }
            if t.ptc == 5 {
                if let Some(f) = parse_number(value) {
                    return Ok(Raw::Real(f));
                }
            }
        }
        self.to_raw(value, profile)
    }
}

/// Linear interpolation through `points` (x, y), extrapolating the end
/// segments.
pub fn interpolate(points: &[(f64, f64)], x: f64) -> Option<f64> {
    if points.len() < 2 {
        return points.first().map(|p| p.1);
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let seg = pts.windows(2).find(|w| x <= w[1].0).unwrap_or(&pts[pts.len() - 2..]);
    let (a, b) = (seg[0], seg[1]);
    if b.0 == a.0 {
        return Some(a.1);
    }
    Some(a.1 + (x - a.0) * (b.1 - a.1) / (b.0 - a.0))
}

fn mask(bits: usize) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// The time format of an absolute (`PTC 9`) or relative (`PTC 10`) time
/// of format `pfc`: CDS for PFC 1 and 2, CUC for 3 to 18, the mission's
/// TM time (with its P-field) for 0.
pub fn time_format(ptc: u32, pfc: u32, profile: &Profile) -> Option<TimeFormat> {
    match (ptc, pfc) {
        (9, 0) => Some(match profile.tm_time {
            TimeFormat::Cuc { coarse, fine, .. } => TimeFormat::Cuc { coarse, fine, pfield: true },
            TimeFormat::Cds { days24, submilli, .. } => TimeFormat::Cds { days24, submilli, pfield: true },
            TimeFormat::None => TimeFormat::Cuc { coarse: 4, fine: 2, pfield: true },
        }),
        (9, 1) => Some(TimeFormat::Cds { days24: false, submilli: 0, pfield: false }),
        (9, 2) => Some(TimeFormat::Cds { days24: false, submilli: 2, pfield: false }),
        (9 | 10, 3..=18) => Some(TimeFormat::Cuc { coarse: ((pfc - 3) / 4 + 1) as u8, fine: ((pfc - 3) % 4) as u8, pfield: false }),
        _ => None,
    }
}

/// An engineering value typed for `t`, as the raw value the packet
/// carries -- `bits` wide when the element fixes it.
pub fn to_raw(set: &MibSet, t: &ParamType, text: &str, profile: &Profile) -> Result<Raw, String> {
    Domain::of(set, t).to_raw(text, profile)
}

fn raw_of(d: &Domain, text: &str, profile: &Profile) -> Result<Raw, String> {
    let t = &d.ty;
    let v = text.trim();
    let name = &t.name;
    // A status text.
    if let Some((raw, _)) = d.texts.iter().find(|(_, s)| s == v).cloned() {
        let n = parse_raw_int(&raw, t.radix).ok_or_else(|| format!("{name}: {v}'s raw value {raw} isn't a number"))?;
        return Ok(if n < 0 { Raw::Signed(n as i64) } else { Raw::Unsigned(n as u64) });
    }
    match t.ptc {
        7 => {
            let hex: String = v.trim_start_matches("0x").trim_start_matches("0X").chars().filter(|c| !c.is_whitespace() && *c != ':' && *c != '-').collect();
            if !hex.len().is_multiple_of(2) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("{name} takes octets in hex, not {v}"));
            }
            let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            Ok(Raw::Bytes(bytes))
        }
        8 => {
            let s = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
            Ok(Raw::Text(s.to_string()))
        }
        9 | 10 => {
            let fmt = time_format(t.ptc, t.pfc, profile).ok_or_else(|| format!("{name}: PTC {}/{} isn't a time this knows", t.ptc, t.pfc))?;
            let (tai, epoch) = if t.ptc == 10 {
                let secs = parse_number(v).ok_or_else(|| format!("{name} takes a duration in seconds, not {v}"))?;
                (Tai(secs), time::Epoch { tai: Tai(0.0), level1: false })
            } else if let Some(utc) = time::parse_utc(v) {
                (time::utc_to_tai(utc, &profile.leap), profile.epoch)
            } else {
                let secs = parse_number(v).ok_or_else(|| format!("{name} takes a UTC time (2026-01-31T12:00:00) or seconds since the epoch, not {v}"))?;
                (Tai(profile.epoch.tai.0 + secs), profile.epoch)
            };
            Ok(Raw::Bytes(time::encode(tai, fmt, epoch)))
        }
        _ => {
            let number = parse_number(v).ok_or_else(|| {
                let aliases = d.aliases();
                if aliases.is_empty() {
                    format!("{name} takes a number, not {v}")
                } else {
                    format!("{name}: {v} isn't one of {}", aliases.join(", "))
                }
            })?;
            let curve = &d.curve;
            // The curve's points are (raw, engineering): back the other way.
            let number = if !curve.is_empty() {
                let back: Vec<(f64, f64)> = curve.iter().map(|(r, e)| (*e, *r)).collect();
                interpolate(&back, number).unwrap_or(number)
            } else {
                number
            };
            let bits = t.bits().unwrap_or(64);
            match t.ptc {
                // Deduced: its type is only known from the argument naming
                // a parameter; any number for now.
                11 if number.fract() != 0.0 => Ok(Raw::Real(number)),
                11 if number < 0.0 => Ok(Raw::Signed(number as i64)),
                11 => Ok(Raw::Unsigned(number as u64)),
                5 => Ok(Raw::Real(number)),
                4 => {
                    let n = number.round() as i64;
                    let (lo, hi) = if bits >= 64 { (i64::MIN, i64::MAX) } else { (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1) };
                    if n < lo || n > hi {
                        return Err(format!("{name}: {v} doesn't fit in {bits} signed bits"));
                    }
                    Ok(Raw::Signed(n))
                }
                _ => {
                    if number < 0.0 {
                        return Err(format!("{name} can't be negative"));
                    }
                    let n = number.round() as u64;
                    if bits < 64 && n > mask(bits) {
                        return Err(format!("{name}: {v} doesn't fit in {bits} bits"));
                    }
                    Ok(Raw::Unsigned(n))
                }
            }
        }
    }
}

/// `raw` for `t` as a person reads it: its status text, its engineering
/// value through the numeric curve, else the raw value.
pub fn to_engineering(set: &MibSet, t: &ParamType, raw: &Raw) -> String {
    Domain::of(set, t).to_engineering(raw)
}

fn engineering_of(d: &Domain, raw: &Raw) -> String {
    let t = &d.ty;
    if let Some(x) = raw.as_f64() {
        if let Some((_, text)) = d.texts.iter().find(|(r, _)| parse_raw_int(r, t.radix).map(|n| n as f64) == Some(x)) {
            return text.clone();
        }
        if !d.curve.is_empty() {
            if let Some(y) = interpolate(&d.curve, x) {
                return trim_float(y);
            }
        }
    }
    raw.display()
}

fn trim_float(y: f64) -> String {
    let s = format!("{y:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// A value the MIB gives (a default, a fixed value, a value set's) as
/// someone types it: read as raw (`inter`) when it's a raw number, else
/// as the engineering value it already is.
pub fn mib_value_as_engineering(set: &MibSet, t: &ParamType, value: &str, inter: &Interp) -> String {
    Domain::of(set, t).mib_value(value, inter)
}

/// A value the MIB gives, as raw.
pub fn mib_value_to_raw(set: &MibSet, t: &ParamType, value: &str, inter: &Interp, profile: &Profile) -> Result<Raw, String> {
    Domain::of(set, t).mib_raw(value, inter, profile)
}

/// One allowed range or value of a TC parameter (`PRV`), in raw or
/// engineering units.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    pub min: f64,
    /// `None`: just `min` (a `PRV` row with no maximum).
    pub max: Option<f64>,
    pub raw: bool,
}

impl Allowed {
    pub fn contains(&self, x: f64) -> bool {
        match self.max {
            Some(max) => self.min <= x && x <= max,
            None => x == self.min,
        }
    }
}

impl std::fmt::Display for Allowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.max {
            Some(max) => write!(f, "{}..{}", trim_float(self.min), trim_float(max))?,
            None => write!(f, "{}", trim_float(self.min))?,
        }
        if self.raw {
            write!(f, " (raw)")?;
        }
        Ok(())
    }
}

/// `t`'s range check: each `PRV` row, read in `PRF_INTER`'s units and
/// `PRF_RADIX`'s radix.
pub fn allowed(set: &MibSet, t: &ParamType) -> Vec<Allowed> {
    let Some(range) = t.range else { return Vec::new() };
    let prf = &set.get(range).row;
    let raw = Interp::of(prf.clean("PRF_INTER")) == Interp::Raw;
    let radix = Radix::of(prf.clean("PRF_RADIX"));
    let read = |s: &str| -> Option<f64> {
        if s.trim().is_empty() {
            return None;
        }
        if raw {
            parse_raw_int(s, radix).map(|n| n as f64).or_else(|| parse_number(s))
        } else {
            parse_number(s)
        }
    };
    match detail::calibration(set, range) {
        CalBody::Ranges(r) => r
            .iter()
            .filter_map(|(lo, hi)| {
                let min = read(lo)?;
                Some(Allowed { min, max: read(hi), raw })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// What's wrong with `value` for `t`, for a person: never stops the
/// command being written, only says. Empty when nothing is.
pub fn check(set: &MibSet, t: &ParamType, value: &str, profile: &Profile) -> Vec<String> {
    Domain::of(set, t).check(value, profile)
}

fn check_of(d: &Domain, value: &str, profile: &Profile) -> Vec<String> {
    let v = value.trim();
    let name = &d.ty.name;
    if v.is_empty() {
        return vec![format!("{name}: no value")];
    }
    let raw = match d.to_raw(v, profile) {
        Ok(r) => r,
        Err(e) => return vec![e],
    };
    let mut out = Vec::new();
    if !d.allowed.is_empty() {
        let eng = if d.texts.iter().any(|(_, s)| s == v) { None } else { parse_number(v) };
        let fits = d.allowed.iter().any(|r| match r.raw {
            true => raw.as_f64().is_some_and(|x| r.contains(x)),
            false => eng.or_else(|| raw.as_f64()).is_some_and(|x| r.contains(x)),
        });
        if !fits {
            let list: Vec<String> = d.allowed.iter().map(ToString::to_string).collect();
            out.push(format!("{name}: '{v}' is outside the allowed {}", list.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_read_in_their_radix() {
        assert_eq!(parse_raw_int("1F", Radix::Hex), Some(31));
        assert_eq!(parse_raw_int("0x1F", Radix::Decimal), Some(31));
        assert_eq!(parse_raw_int("17", Radix::Octal), Some(15));
        assert_eq!(parse_raw_int("-12", Radix::Decimal), Some(-12));
        assert_eq!(parse_raw_int("0b101", Radix::Decimal), Some(5));
        assert_eq!(parse_raw_int("1F", Radix::Decimal), None);
        assert_eq!(parse_number("2.5e3"), Some(2500.0));
        assert_eq!(parse_number("0x10"), Some(16.0));
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn interpretation_flags_default_to_raw() {
        assert_eq!(Interp::of(""), Interp::Raw);
        assert_eq!(Interp::of("E"), Interp::Engineering);
        assert_eq!(Interp::of("D"), Interp::Dynamic);
    }

    #[test]
    fn single_values_and_ranges() {
        assert!(Allowed { min: 3.0, max: None, raw: false }.contains(3.0));
        assert!(!Allowed { min: 3.0, max: None, raw: false }.contains(4.0));
        assert_eq!(Allowed { min: 0.0, max: Some(1.5), raw: true }.to_string(), "0..1.5 (raw)");
    }

    #[test]
    fn times_map_to_their_codes() {
        let p = Profile::default();
        assert_eq!(time_format(9, 1, &p), Some(TimeFormat::Cds { days24: false, submilli: 0, pfield: false }));
        assert_eq!(time_format(9, 17, &p), Some(TimeFormat::Cuc { coarse: 4, fine: 2, pfield: false }));
        assert_eq!(time_format(10, 7, &p), Some(TimeFormat::Cuc { coarse: 2, fine: 0, pfield: false }));
    }
}
