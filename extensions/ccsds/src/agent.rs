//! `ccsds_decode`, for coding agents: bytes given as hex decoded as the
//! packet inspector (`SPC k d`) decodes them -- a space packet through
//! the MIB, a transfer frame or a CLTU with what it carries, a CLCW, an
//! on-board time, a CFDP PDU -- answered as JSON: the inspector's tree,
//! each field's place, raw value, meaning and check, and the definition
//! the MIB identified.

use fenix_ccsds::field::{Check, Field, Link};
use fenix_mib::mission::Mission;
use fenix_mib::{Kind, MibSet};
use serde_json::{json, Value};

use crate::decode;
use crate::packet_page::DecodeAs;

/// The most bytes an agent's call decodes, and the most fields answered.
pub const MAX_BYTES: usize = 65_536;
const MAX_FIELDS: usize = 1_500;

/// How an agent says to read the bytes.
pub fn reading(name: &str) -> Option<DecodeAs> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => DecodeAs::Auto,
        "packet" | "space_packet" | "tm" | "tc" => DecodeAs::Packet,
        "frame" | "transfer_frame" | "cadu" => DecodeAs::Frame,
        "cltu" => DecodeAs::Cltu,
        "cfdp" | "pdu" => DecodeAs::Cfdp,
        "clcw" => DecodeAs::Clcw,
        "time" | "cuc" | "cds" => DecodeAs::Time,
        _ => return None,
    })
}

fn link_text(link: &Link) -> String {
    match link {
        Link::Spid(s) => format!("TM packet {s}"),
        Link::Parameter(p) => format!("parameter {p}"),
        Link::Telecommand(t) => format!("telecommand {t}"),
        Link::Standard(id, heading) => format!("{id}, {heading}"),
    }
}

/// A field and what's under it, while `budget` lasts.
fn field_json(f: &Field, budget: &mut usize) -> Value {
    *budget = budget.saturating_sub(1);
    let mut out = json!({ "name": f.name, "byte": f.bit / 8 });
    if !f.bit.is_multiple_of(8) || !f.bits.is_multiple_of(8) {
        out["bit"] = json!(f.bit % 8);
        out["bits"] = json!(f.bits);
    } else if f.bits > 0 {
        out["bytes"] = json!(f.bits / 8);
    }
    if !f.raw.is_empty() {
        out["raw"] = json!(f.raw);
    }
    if !f.value.is_empty() {
        out["value"] = json!(f.value);
    }
    match &f.check {
        Some(Check::Ok(m)) => out["ok"] = json!(m),
        Some(Check::Warn(m)) => out["warning"] = json!(m),
        Some(Check::Bad(m)) => out["bad"] = json!(m),
        None => {}
    }
    if let Some(l) = &f.link {
        out["see"] = json!(link_text(l));
    }
    if !f.children.is_empty() {
        let mut children = Vec::new();
        for c in &f.children {
            if *budget == 0 {
                out["children_left_out"] = json!(f.children.len() - children.len());
                break;
            }
            children.push(field_json(c, budget));
        }
        out["children"] = Value::Array(children);
    }
    out
}

/// `ccsds_decode`'s answer for `bytes` read as `reading`, through the
/// MIB when there's one, against the mission's profile.
pub fn decode_answer(bytes: &[u8], reading: DecodeAs, set: Option<&MibSet>, m: &Mission) -> Value {
    let (root, def) = decode::decode_bytes(bytes, reading, set, m);
    let walk = root.walk();
    let checks = |bad: bool| -> Vec<String> {
        walk.iter()
            .filter_map(|(_, f)| match &f.check {
                Some(Check::Bad(msg)) if bad => Some(format!("{}: {msg}", f.name)),
                Some(Check::Warn(msg)) if !bad => Some(format!("{}: {msg}", f.name)),
                _ => None,
            })
            .collect()
    };
    let mut budget = MAX_FIELDS;
    let mut out = json!({
        "bytes": bytes.len(),
        "read_as": reading.label(),
        "decoded": root.name,
        "mib": set.is_some_and(|s| !s.is_empty()),
        "fields": field_json(&root, &mut budget),
    });
    if !root.value.is_empty() {
        out["summary"] = json!(root.value);
    }
    if let (Some(d), Some(set)) = (def, set) {
        let e = set.get(d);
        let kind = match d.kind {
            Kind::Telecommand => "telecommand",
            Kind::TmPacket => "tm_packet",
            Kind::TcParam => "tc_parameter",
            Kind::TmParam => "tm_parameter",
            Kind::Calibration => "calibration",
        };
        out["identified"] = json!({ "kind": kind, "name": e.name, "alias": e.alias, "description": e.description, "mib": set.root_label(e.root) });
    }
    let (bad, warnings) = (checks(true), checks(false));
    out["checks_pass"] = json!(bad.is_empty());
    if !bad.is_empty() {
        out["problems"] = json!(bad);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    if !m.problems.is_empty() {
        out["profile_problems"] = json!(m.problems);
    }
    if budget == 0 {
        out["note"] = json!(format!("only the first {MAX_FIELDS} fields are given"));
    }
    out
}

/// The bytes a call's `hex` spells: `1B F2 C0 07`, `0x1B,0xF2`, a hex
/// dump with offsets.
pub fn bytes_of(hex: &str) -> Result<Vec<u8>, String> {
    let bytes = fenix_ccsds::hex::parse(hex).filter(|b| !b.is_empty()).ok_or("hex isn't hex: 1BF2C00700 or 1B F2 C0 07 00, 0x prefixes and a dump's offsets are fine")?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("{} bytes: at most {MAX_BYTES} at a time", bytes.len()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_mib::{DefRef, MibRoot};
    use std::path::PathBuf;

    fn ops() -> MibSet {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/mib/tests/fixtures/ops");
        MibSet::load(vec![MibRoot { label: "OPS".into(), path }], None)
    }

    fn find<'a>(f: &'a Value, name: &str) -> Option<&'a Value> {
        if f["name"].as_str().is_some_and(|n| n.contains(name)) {
            return Some(f);
        }
        f["children"].as_array()?.iter().find_map(|c| find(c, name))
    }

    #[test]
    fn a_telecommand_built_from_the_mib_is_decoded_through_it() {
        let set = ops();
        let m = Mission::default();
        let tc: DefRef = set.find(Kind::Telecommand, 0, "ZTC08101").unwrap();
        let args = vec![("PTH00101".to_string(), "3".to_string()), ("PTH00102".to_string(), "ON".to_string())];
        let packet = fenix_mib::packets::encode_tc(&set, tc, &args, &m.profile, 7).unwrap().bytes;
        let hex: String = packet.iter().map(|b| format!("{b:02X} ")).collect();
        let bytes = bytes_of(&hex).unwrap();
        assert_eq!(bytes, packet);
        let answer = decode_answer(&bytes, reading("auto").unwrap(), Some(&set), &m);
        assert_eq!(answer["identified"]["name"], "ZTC08101", "{answer:#}");
        assert_eq!(answer["identified"]["kind"], "telecommand");
        assert_eq!(answer["bytes"], packet.len());
        assert_eq!(answer["mib"], true);
        assert!(find(&answer["fields"], "APID").is_some_and(|f| f["raw"].as_str().is_some() || f["value"].as_str().is_some()), "{answer:#}");
        assert!(answer.to_string().contains("PTH00102"), "its arguments: {answer:#}");
    }

    #[test]
    fn without_the_mib_and_when_it_doesnt_read() {
        let m = Mission::default();
        // A TM packet's primary header: version 0, TM, no secondary header,
        // APID 0x3F2, unsegmented, count 1, 2 octets of data.
        let bytes = bytes_of("0x03,0xF2,0xC0,0x01,0x00,0x01,0xAB,0xCD").unwrap();
        let answer = decode_answer(&bytes, DecodeAs::Packet, None, &m);
        assert_eq!(answer["mib"], false);
        assert!(answer.get("identified").is_none());
        assert!(find(&answer["fields"], "APID").is_some(), "{answer:#}");
        let short = decode_answer(&[0x1B], DecodeAs::Packet, None, &m);
        assert_eq!(short["checks_pass"], false);
        assert!(short["problems"][0].as_str().unwrap().contains("shorter than a packet header"), "{short:#}");
        let clcw = decode_answer(&[0x01, 0x00, 0x00, 0x05], DecodeAs::Clcw, None, &m);
        assert_eq!(clcw["read_as"], "a CLCW");
        assert!(bytes_of("not hex").is_err());
        assert!(bytes_of(&"00".repeat(MAX_BYTES + 1)).unwrap_err().contains("at most"));
        assert_eq!(reading("CLTU"), Some(DecodeAs::Cltu));
        assert_eq!(reading("zip"), None);
    }

    #[test]
    fn a_tree_is_cut_to_a_size() {
        let leaves: Vec<Field> = (0..2000).map(|i| Field::new(format!("f{i}"), i * 8, 8, "00", "")).collect();
        let mut budget = MAX_FIELDS;
        let tree = field_json(&Field::group("all", leaves), &mut budget);
        assert_eq!(tree["children"].as_array().unwrap().len(), MAX_FIELDS - 1);
        assert_eq!(tree["children_left_out"], 2000 - (MAX_FIELDS - 1));
        assert_eq!(tree["children"][1], json!({"name": "f1", "byte": 1, "bytes": 1, "raw": "00"}));
    }
}
