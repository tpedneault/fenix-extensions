//! A mission's profile: how its packets, frames and times are laid out,
//! from its `ccsds.*` settings -- the project's over yours over Fenix's
//! defaults, however the caller reads them. The MIB extension reads and
//! builds packets with it; the CCSDS extension decodes with it.

use fenix_ccsds::frames::{FrameKind, FrameProfile};
use fenix_ccsds::pus::{Pec, Profile, PusEdition};
use fenix_ccsds::time::{Epoch, LeapTable, TimeFormat};

use crate::packets::OffsetBase;

/// Everything a project says about how its bytes are laid out.
#[derive(Debug, Clone)]
pub struct Mission {
    pub profile: Profile,
    pub base: OffsetBase,
    pub frames: FrameProfile,
    /// Telecommand frames, as CLTUs carry them.
    pub tc: FrameProfile,
    /// The nominal epoch, when a time correlation moved the profile's.
    pub nominal_epoch: Option<Epoch>,
    pub checks_off: Vec<String>,
    /// What didn't parse, to say once.
    pub problems: Vec<String>,
}

impl Default for Mission {
    fn default() -> Mission {
        Mission::read(|_| None, None)
    }
}

/// Whether a setting's text means on.
fn on(text: Option<String>, default: bool) -> bool {
    match text.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("true" | "on" | "yes" | "1") => true,
        Some("false" | "off" | "no" | "0") => false,
        _ => default,
    }
}

/// A map setting's entries, as Fenix gives an extension one: JSON,
/// `[["key", "value"], ...]`.
pub fn map_entries(text: &str) -> Vec<(String, String)> {
    serde_json::from_str(text).unwrap_or_default()
}

/// A list setting's items, as Fenix gives an extension one: separated by
/// commas.
pub fn list_items(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

impl Mission {
    /// The profile `setting` says (a setting's text by its key, `None`
    /// when it isn't set), with `leap_seconds` the text of the leap
    /// seconds file `ccsds.leap_seconds` names, when there's one.
    pub fn read(setting: impl Fn(&str) -> Option<String>, leap_seconds: Option<Result<String, String>>) -> Mission {
        let text = |key: &str| setting(key).filter(|t| !t.trim().is_empty());
        let int = |key: &str| text(key).and_then(|t| t.trim().parse::<i64>().ok());
        let flag = |key: &str, default: bool| on(text(key), default);
        let mut problems = Vec::new();
        let leap = match leap_seconds {
            Some(Ok(t)) => LeapTable::parse(&t).unwrap_or_else(|e| {
                problems.push(format!("ccsds.leap_seconds: {e}"));
                LeapTable::default()
            }),
            Some(Err(e)) => {
                problems.push(format!("ccsds.leap_seconds: {e}"));
                LeapTable::default()
            }
            None => LeapTable::default(),
        };
        let pus = match text("ccsds.pus").as_deref().map(str::trim) {
            Some("a") => PusEdition::A,
            Some("none") => PusEdition::None,
            _ => PusEdition::C,
        };
        let cuc = TimeFormat::Cuc { coarse: 4, fine: 2, pfield: false };
        let tm_time = TimeFormat::parse(&text("ccsds.tm_time").unwrap_or_else(|| "cuc 4.2".into())).unwrap_or_else(|e| {
            problems.push(format!("ccsds.tm_time: {e}"));
            cuc
        });
        let epoch = match text("ccsds.epoch") {
            Some(e) => Epoch::parse(&e, &leap).unwrap_or_else(|why| {
                problems.push(format!("ccsds.epoch: {why}"));
                Epoch::default()
            }),
            None => Epoch::default(),
        };
        // A correlation says where the on-board clock really counts from.
        let (epoch, nominal_epoch) = match text("ccsds.time_correlation") {
            Some(t) => match fenix_ccsds::time::correlate(&t, tm_time, &leap) {
                Ok(correlated) => (correlated, Some(epoch)),
                Err(why) => {
                    problems.push(format!("ccsds.time_correlation: {why}"));
                    (epoch, None)
                }
            },
            None => (epoch, None),
        };
        let pec = text("ccsds.crc").and_then(|c| Pec::parse(&c).ok()).unwrap_or(Pec::Ccitt16);
        let base = OffsetBase::parse(&text("ccsds.plf_offset").unwrap_or_default()).unwrap_or_default();
        let profile = Profile { pus, tm_time, epoch, leap, pec, tc_source_id: int("ccsds.tc_source_id").unwrap_or(0).clamp(0, 65535) as u16, ..Default::default() };
        let vc_names = text("ccsds.vc_names").map(|t| map_entries(&t)).unwrap_or_default();
        let frames = FrameProfile {
            kind: FrameKind::parse(&text("ccsds.frame_type").unwrap_or_else(|| "tm".into())).unwrap_or_default(),
            length: int("ccsds.frame_length").unwrap_or(1115).max(7) as usize,
            asm: flag("ccsds.frame_asm", true),
            randomized: flag("ccsds.frame_randomized", false),
            rs_depth: int("ccsds.frame_rs_depth").unwrap_or(0).clamp(0, 8) as usize,
            ocf: flag("ccsds.frame_ocf", true),
            fecf: flag("ccsds.frame_fecf", false),
            vc_names: vc_names.into_iter().filter_map(|(k, v)| Some((k.trim().parse().ok()?, v))).collect(),
            ..Default::default()
        };
        let tc = FrameProfile {
            kind: FrameKind::Tc,
            asm: false,
            ocf: false,
            fecf: flag("ccsds.tc_fecf", true),
            tc_segment_header: flag("ccsds.tc_segment_header", true),
            randomized: flag("ccsds.tc_randomized", false),
            bch_correct: text("ccsds.tc_bch").as_deref().map(str::trim) != Some("detect"),
            vc_names: frames.vc_names.clone(),
            ..Default::default()
        };
        let checks_off = text("ccsds.checks_off").map(|t| list_items(&t)).unwrap_or_default();
        Mission { profile, base, frames, tc, nominal_epoch, checks_off, problems }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_read_from_settings_and_says_what_didnt_parse() {
        let settings = |key: &str| -> Option<String> {
            match key {
                "ccsds.pus" => Some("a".into()),
                "ccsds.tm_time" => Some("cds 16".into()),
                "ccsds.crc" => Some("iso".into()),
                "ccsds.frame_length" => Some("223".into()),
                "ccsds.frame_asm" => Some("false".into()),
                "ccsds.tc_bch" => Some("detect".into()),
                "ccsds.vc_names" => Some(r#"[["0","HK"],["x","no"]]"#.into()),
                "ccsds.checks_off" => Some("apid, size".into()),
                "ccsds.epoch" => Some("not a date".into()),
                _ => None,
            }
        };
        let m = Mission::read(settings, None);
        assert_eq!(m.profile.pus, PusEdition::A);
        assert_eq!(m.profile.pec, Pec::Iso);
        assert_eq!((m.frames.length, m.frames.asm), (223, false));
        assert!(!m.tc.bch_correct);
        assert_eq!(m.frames.vc_names, vec![(0, "HK".to_string())]);
        assert_eq!(m.checks_off, vec!["apid".to_string(), "size".to_string()]);
        assert!(m.problems.iter().any(|p| p.starts_with("ccsds.epoch")), "{:?}", m.problems);
        let d = Mission::default();
        assert_eq!((d.profile.pus, d.profile.pec, d.frames.length, d.frames.asm), (PusEdition::C, Pec::Ccitt16, 1115, true));
        assert!(d.problems.is_empty());
    }
}
