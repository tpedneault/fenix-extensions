//! `mib_lookup`, for coding agents: a telecommand, a TC or TM parameter,
//! a TM packet or a calibration, found by name -- or by the words of its
//! description, as `/` on the MIB page finds them -- with what its page
//! shows: a telecommand's arguments (bits, types, defaults, what each may
//! be), a parameter's calibration, limits and packets, a packet's
//! parameters, a calibration's points or texts.

use fenix_mib::detail::{self, CalBody};
use fenix_mib::{DefRef, Kind, MibSet, Query};
use serde_json::{json, Map, Value};

/// The most other matches listed.
const MAX_MATCHES: usize = 20;

/// A kind as an agent names it.
pub fn kind(name: &str) -> Option<Kind> {
    match name.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "telecommand" | "tc" | "command" => Some(Kind::Telecommand),
        "tc_parameter" | "tc_param" | "argument" => Some(Kind::TcParam),
        "tm_packet" | "packet" | "pkt" => Some(Kind::TmPacket),
        "tm_parameter" | "tm_param" | "parameter" | "tm" => Some(Kind::TmParam),
        "calibration" | "cal" => Some(Kind::Calibration),
        _ => None,
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Telecommand => "telecommand",
        Kind::TcParam => "tc_parameter",
        Kind::TmPacket => "tm_packet",
        Kind::TmParam => "tm_parameter",
        Kind::Calibration => "calibration",
    }
}

/// The definition `name` is, exactly: by name (a TM packet by its SPID
/// or its name, a calibration by its id), from the default MIB first.
fn exact(set: &MibSet, name: &str, kinds: &[Kind]) -> Option<DefRef> {
    let name = name.trim();
    let roots = std::iter::once(set.default_root()).chain((0..set.roots().len()).filter(|&r| r != set.default_root()));
    for root in roots {
        for &k in kinds {
            if let Some(d) = set.find(k, root, name).or_else(|| set.find(k, root, &name.to_ascii_uppercase())) {
                return Some(d);
            }
        }
    }
    set.resolve(name).filter(|d| kinds.contains(&d.kind)).or_else(|| {
        // A calibration's id, a packet's name, in any case.
        kinds.iter().find_map(|&k| set.entries(k).iter().position(|e| e.name.eq_ignore_ascii_case(name) || (!e.alias.is_empty() && e.alias.eq_ignore_ascii_case(name))).map(|index| DefRef { kind: k, index }))
    })
}

/// A definition in a line: its kind, name, description and MIB.
fn brief(set: &MibSet, def: DefRef) -> Value {
    let e = set.get(def);
    let mut out = json!({ "kind": kind_name(def.kind), "name": e.name, "description": e.description, "mib": set.root_label(e.root) });
    if !e.alias.is_empty() {
        out["alias"] = json!(e.alias);
    }
    out
}

fn calibration_json(set: &MibSet, cal: DefRef) -> Value {
    let e = set.get(cal);
    let mut out = json!({ "id": e.name, "description": e.description, "kind": e.cal.map(|k| k.label()).unwrap_or("") });
    if let Some(unit) = e.cols.get(2).filter(|u| !u.is_empty()) {
        out["unit"] = json!(unit);
    }
    let pairs = |p: Vec<(String, String)>, a: &str, b: &str| p.into_iter().map(|(x, y)| json!({ a: x, b: y })).collect::<Vec<_>>();
    match detail::calibration(set, cal) {
        CalBody::Points(p) => out["points"] = json!(pairs(p, "raw", "engineering")),
        CalBody::Texts(t) => out["texts"] = json!(pairs(t, "raw", "text")),
        CalBody::Ranges(r) => out["ranges"] = json!(pairs(r, "min", "max")),
        CalBody::Coefficients(c) => out["coefficients"] = json!(c),
    }
    out
}

/// Everything its page shows about `def`.
pub fn detail(set: &MibSet, def: DefRef, apid_hex: bool) -> Value {
    let mut out = brief(set, def);
    let facts: Map<String, Value> = crate::mib_def::facts(set, def, apid_hex).into_iter().map(|(k, v)| (k.to_string(), Value::String(v))).collect();
    out["facts"] = Value::Object(facts);
    let names = |defs: &[DefRef]| defs.iter().map(|&d| set.get(d).name.clone()).collect::<Vec<_>>();
    match def.kind {
        Kind::Telecommand => {
            let args: Vec<Value> = detail::tc_elements(set, def)
                .into_iter()
                .map(|el| {
                    let mut a = json!({ "name": el.name, "description": el.description, "bit": el.bit, "bits": el.bits, "type": el.type_name });
                    for (key, v) in [("unit", &el.unit), ("default", &el.default), ("allowed", &el.allowed)] {
                        if !v.is_empty() {
                            a[key] = json!(v);
                        }
                    }
                    if let Some(f) = &el.fixed {
                        a["fixed"] = json!(f);
                    }
                    if el.name.is_empty() {
                        a["spare"] = json!(true);
                    }
                    if el.group > 0 {
                        a["repeats_next"] = json!(el.group);
                    }
                    if let Some(c) = el.cal {
                        a["calibration"] = calibration_json(set, c);
                    }
                    a
                })
                .collect();
            out["arguments"] = json!(args);
            let sequences: Vec<String> = set.sequences_using(def).into_iter().map(|(name, _, _)| name).collect();
            if !sequences.is_empty() {
                out["sequences"] = json!(sequences);
            }
        }
        Kind::TcParam => {
            out["calibrations"] = json!(detail::tc_param_calibrations(set, def).into_iter().map(|c| calibration_json(set, c)).collect::<Vec<_>>());
            out["used_by"] = json!(names(set.used_by(def)));
        }
        Kind::TmParam => {
            if let Some(c) = detail::tm_calibration(set, def) {
                out["calibration"] = calibration_json(set, c);
            }
            let limits: Vec<Value> = detail::limits(set, def)
                .into_iter()
                .map(|l| {
                    let mut v = json!({ "kind": l.kind, "low": l.low, "high": l.high });
                    if let Some((param, value)) = l.when {
                        v["when"] = json!({ "parameter": param, "value": value });
                    }
                    v
                })
                .collect();
            if !limits.is_empty() {
                out["limits"] = json!(limits);
            }
            out["carried_by"] = json!(detail::carried_by(set, def)
                .into_iter()
                .map(|(p, slot)| json!({ "spid": set.get(p).name, "packet": set.get(p).alias, "byte": slot.byte, "bit": slot.bit, "bits": slot.bits }))
                .collect::<Vec<_>>());
        }
        Kind::TmPacket => {
            out["parameters"] = json!(detail::packet_slots(set, def)
                .into_iter()
                .map(|s| {
                    let mut v = json!({ "name": s.name, "description": s.description, "byte": s.byte, "bit": s.bit, "bits": s.bits });
                    if !s.occurrences.is_empty() {
                        v["occurrences"] = json!(s.occurrences);
                    }
                    v
                })
                .collect::<Vec<_>>());
        }
        Kind::Calibration => {
            // Its body beside its name: what `calibration_json` says but
            // the id and description, already there.
            if let Value::Object(cal) = calibration_json(set, def) {
                for (k, v) in cal.into_iter().filter(|(k, _)| k != "id" && k != "description") {
                    let key = if k == "kind" { "calibration_kind".to_string() } else { k };
                    out[key.as_str()] = v;
                }
            }
            out["used_by"] = json!(names(set.used_by(def)));
        }
    }
    out
}

/// `mib_lookup`'s answer: the definition `query` names, or the best of
/// those its words find, with the others listed.
pub fn lookup(set: &MibSet, query: &str, kinds: &[Kind], apid_hex: bool) -> Result<Value, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("what to look up? {\"query\": \"ZTC08101\"} -- a name, or words of a description".into());
    }
    if let Some(def) = exact(set, query, kinds) {
        return Ok(json!({ "found": detail(set, def, apid_hex) }));
    }
    let q = Query::parse(query);
    let mut hits: Vec<_> = kinds.iter().flat_map(|&k| q.search(set, k)).collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| set.get(a.def).name.cmp(&set.get(b.def).name)));
    let Some(best) = hits.first() else {
        return Err(format!("nothing in the MIB matches {query:?}"));
    };
    let others: Vec<Value> = hits
        .iter()
        .skip(1)
        .take(MAX_MATCHES)
        .map(|h| {
            let mut b = brief(set, h.def);
            if let Some(on) = &h.matched_on {
                b["matched_on"] = json!(on);
            }
            b
        })
        .collect();
    let mut out = json!({ "found": detail(set, best.def, apid_hex), "matches": hits.len() });
    if !others.is_empty() {
        out["others"] = json!(others);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_mib::MibRoot;
    use std::path::PathBuf;

    fn ops() -> MibSet {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/mib/tests/fixtures/ops");
        MibSet::load(vec![MibRoot { label: "OPS".into(), path }], None)
    }

    #[test]
    fn a_telecommand_by_name_with_its_arguments() {
        let set = ops();
        let answer = lookup(&set, "ZTC08101", &Kind::ALL, true).unwrap();
        let tc = &answer["found"];
        assert_eq!((tc["kind"].as_str(), tc["name"].as_str(), tc["mib"].as_str()), (Some("telecommand"), Some("ZTC08101"), Some("OPS")));
        assert_eq!(tc["facts"]["PUS"], "8,1");
        assert_eq!(tc["facts"]["APID"], "0x3F2 · 1010");
        let args = tc["arguments"].as_array().unwrap();
        let mode = args.iter().find(|a| a["name"] == "PTH00102").expect("its mode argument");
        assert_eq!(mode["allowed"], "OFF ON AUTO");
        assert_eq!(mode["default"], "AUTO");
        assert_eq!(mode["calibration"]["texts"][2], json!({"raw": "2", "text": "AUTO"}));
        assert!(answer.get("others").is_none(), "an exact name: no list");
        assert_eq!(lookup(&set, "ztc08101", &Kind::ALL, true).unwrap()["found"]["name"], "ZTC08101", "any case");
    }

    #[test]
    fn a_parameter_by_its_description_with_its_calibration_limits_and_packets() {
        let set = ops();
        let answer = lookup(&set, "heater 3 temperature", &[Kind::TmParam], false).unwrap();
        let p = &answer["found"];
        assert_eq!(p["name"], "NTH00123");
        assert_eq!(p["facts"]["unit"], "degC");
        assert_eq!(p["calibration"]["id"], "CAF00310");
        assert!(p["calibration"]["points"].as_array().is_some_and(|pts| !pts.is_empty()));
        assert_eq!(p["limits"][0], json!({"kind": "soft", "low": "-10", "high": "50", "when": {"parameter": "NTH00201", "value": "2"}}));
        assert_eq!(p["carried_by"][0]["spid"], "30211");
        assert!(answer["matches"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn a_packet_by_spid_and_a_calibration_by_id() {
        let set = ops();
        let pkt = &lookup(&set, "30211", &Kind::ALL, true).unwrap()["found"];
        assert_eq!((pkt["kind"].as_str(), pkt["alias"].is_string()), (Some("tm_packet"), true));
        let names: Vec<&str> = pkt["parameters"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
        assert_eq!(&names[..3], ["NTH00100", "NTH00123", "NTH00124"], "by their place in the packet");
        let cal = &lookup(&set, "PAF00042", &[Kind::Calibration], true).unwrap()["found"];
        assert_eq!(cal["calibration_kind"], "status");
        assert_eq!(cal["texts"].as_array().unwrap().len(), 3);
        assert_eq!(cal["used_by"], json!(["PTH00102"]));
    }

    #[test]
    fn words_that_match_several_list_the_others() {
        let set = ops();
        let answer = lookup(&set, "heater", &[Kind::Telecommand], true).unwrap();
        assert!(answer["others"].as_array().is_some_and(|o| !o.is_empty()), "{answer}");
        assert_eq!(answer["found"]["kind"], "telecommand");
        assert!(lookup(&set, "zzzz nothing", &Kind::ALL, true).unwrap_err().contains("nothing in the MIB matches"));
        assert!(lookup(&set, " ", &Kind::ALL, true).is_err());
        assert_eq!(kind("TM parameter"), Some(Kind::TmParam));
        assert_eq!(kind("frame"), None);
    }
}
