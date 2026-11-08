//! What the MIB says about sending a telecommand beyond its layout: the
//! named sets of values it may be sent with (`CCF_DEFSET`, `PST`/`PSV`/
//! `PVS`/`PSM`), the TM it waits for before it goes (`PTV`), the stages
//! it's verified through (`CVP`/`CVS`/`CVE`), and its packet header as
//! `TCP`/`PCDF`/`PCPC` lay it out.

use crate::row::Row;
use crate::set::{DefRef, MibSet};
use crate::types::parse_int;
use crate::values::{self, Interp, Radix};

/// One value of a value set.
#[derive(Debug, Clone, PartialEq)]
pub struct SetValue {
    pub param: String,
    pub inter: Interp,
    pub value: String,
    /// `PVS_BIT`: which occurrence of a repeated parameter, when given.
    pub bit: Option<i64>,
}

/// A named set of values a telecommand can be sent with.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueSet {
    /// `PVS_ID`.
    pub id: String,
    /// `PSV_NAME`, else the id.
    pub name: String,
    pub description: String,
    /// Its parameter set (`PVS_PSID`).
    pub parameter_set: String,
    /// The telecommand's `CCF_DEFSET`.
    pub default: bool,
    pub values: Vec<SetValue>,
}

impl ValueSet {
    /// Its values as someone types them, by parameter.
    pub fn engineering(&self, set: &MibSet, tc: DefRef) -> Vec<(String, String)> {
        let root = set.get(tc).root;
        self.values
            .iter()
            .map(|v| {
                let shown = match set.find(crate::Kind::TcParam, root, &v.param) {
                    Some(p) => values::mib_value_as_engineering(set, &values::ParamType::of(set, p), &v.value, &v.inter),
                    None => v.value.clone(),
                };
                (v.param.clone(), shown)
            })
            .collect()
    }
}

/// The value sets `tc` can be sent with: those of the parameter sets
/// `PSM` maps it to, and its `CCF_DEFSET`; the default first.
pub fn value_sets(set: &MibSet, tc: DefRef) -> Vec<ValueSet> {
    let e = set.get(tc);
    let index = set.index();
    let root = Some(e.root);
    let defset = e.row.clean("CCF_DEFSET");
    let mut parsets: Vec<String> = index
        .rows_by_field("psm", "PSM_NAME", &e.name, root)
        .into_iter()
        .filter(|r| matches!(r.clean("PSM_TYPE"), "" | "C"))
        .map(|r| r.clean("PSM_PARSET").to_string())
        .collect();
    parsets.dedup();
    let mut ids: Vec<String> = Vec::new();
    for ps in &parsets {
        for r in index.rows_by_field("pvs", "PVS_PSID", ps, root) {
            let id = r.clean("PVS_ID").to_string();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    // The default set, by its id or its name.
    if !defset.is_empty() {
        let id = index.first_row_by_field("psv", "PSV_NAME", defset, root).map(|r| r.clean("PSV_PVSID").to_string()).filter(|i| !i.is_empty()).unwrap_or_else(|| defset.to_string());
        if !ids.contains(&id) {
            ids.insert(0, id);
        }
    }
    let mut out: Vec<ValueSet> = ids
        .into_iter()
        .filter_map(|id| {
            let rows: Vec<&Row> = index.rows_by_field("pvs", "PVS_ID", &id, root);
            let psv = index.first_row_by_field("psv", "PSV_PVSID", &id, root).or_else(|| index.first_row_by_field("psv", "PSV_NAME", &id, root));
            let name = psv.map(|r| r.clean("PSV_NAME").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
            if rows.is_empty() && psv.is_none() {
                return None;
            }
            Some(ValueSet {
                default: !defset.is_empty() && (defset == name || defset == id),
                description: psv.map(|r| r.clean("PSV_DESCR").to_string()).unwrap_or_default(),
                parameter_set: rows.first().map(|r| r.clean("PVS_PSID").to_string()).unwrap_or_default(),
                values: rows
                    .iter()
                    .map(|r| SetValue { param: r.clean("PVS_PNAME").to_string(), inter: Interp::of(r.clean("PVS_INTER")), value: r.clean("PVS_VALS").to_string(), bit: parse_int(r.clean("PVS_BIT")) })
                    .collect(),
                name,
                id,
            })
        })
        .collect();
    out.sort_by_key(|v| !v.default);
    out
}

/// A condition the TM must meet before `tc` is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Precondition {
    pub param: String,
    pub inter: Interp,
    pub value: String,
}

impl std::fmt::Display for Precondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} = {}{}", self.param, self.value, if self.inter == Interp::Raw { " (raw)" } else { "" })
    }
}

/// `tc`'s pre-transmission validation (`PTV`).
pub fn preconditions(set: &MibSet, tc: DefRef) -> Vec<Precondition> {
    let e = set.get(tc);
    set.index()
        .rows_by_field("ptv", "PTV_CNAME", &e.name, Some(e.root))
        .into_iter()
        .map(|r| Precondition { param: r.clean("PTV_PARNAM").to_string(), inter: Interp::of(r.clean("PTV_INTER")), value: r.clean("PTV_VAL").to_string() })
        .collect()
}

/// A check a verification stage makes on the TM.
#[derive(Debug, Clone, PartialEq)]
pub struct StageCheck {
    pub param: String,
    pub value: String,
    pub tolerance: String,
    pub inter: Interp,
    /// `CVE_CHECK`: `B` (before) / `A` (after), as the MIB writes it.
    pub when: String,
}

/// One stage a telecommand is verified through.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub id: String,
    /// "acceptance", "start", "progress 3", "completion".
    pub kind: String,
    /// "report" (a PUS service 1 report) or "parameters" (its checks).
    pub source: String,
    /// Seconds after release it opens, and how long it stays open.
    pub start: String,
    pub interval: String,
    /// The report packet it waits for, when it's one.
    pub spid: String,
    pub uncertainty: String,
    pub checks: Vec<StageCheck>,
}

fn stage_kind(t: &str) -> String {
    match t {
        "A" => "acceptance".into(),
        "S" => "start".into(),
        "C" => "completion".into(),
        "P" => "progress".into(),
        d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => format!("progress {d}"),
        other => other.to_string(),
    }
}

/// `tc`'s verification stages (`CVP` -> `CVS`, with `CVE`'s checks), in
/// the order a command goes through them.
pub fn verification(set: &MibSet, tc: DefRef) -> Vec<Stage> {
    let e = set.get(tc);
    let index = set.index();
    let root = Some(e.root);
    let mut out: Vec<Stage> = index
        .rows_by_field("cvp", "CVP_TASK", &e.name, root)
        .into_iter()
        .filter(|r| matches!(r.clean("CVP_TYPE"), "" | "C"))
        .filter_map(|r| {
            let id = r.clean("CVP_CVSID");
            let cvs = index.first_row_by_field("cvs", "CVS_ID", id, root)?;
            Some(Stage {
                id: id.to_string(),
                kind: stage_kind(cvs.clean("CVS_TYPE")),
                source: match cvs.clean("CVS_SOURCE") {
                    "R" => "report".into(),
                    "V" => "parameters".into(),
                    other => other.to_string(),
                },
                start: cvs.clean("CVS_START").to_string(),
                interval: cvs.clean("CVS_INTERVAL").to_string(),
                spid: cvs.clean("CVS_SPID").to_string(),
                uncertainty: cvs.clean("CVS_UNCERTAINTY").to_string(),
                checks: index
                    .rows_by_field("cve", "CVE_CVSID", id, root)
                    .into_iter()
                    .map(|c| StageCheck {
                        param: c.clean("CVE_PARNAM").to_string(),
                        value: c.clean("CVE_VAL").to_string(),
                        tolerance: c.clean("CVE_TOL").to_string(),
                        inter: Interp::of(c.clean("CVE_INTER")),
                        when: c.clean("CVE_CHECK").to_string(),
                    })
                    .collect(),
            })
        })
        .collect();
    let rank = |k: &str| match k {
        "acceptance" => 0,
        "start" => 1,
        "completion" => 3,
        _ => 2,
    };
    out.sort_by(|a, b| rank(&a.kind).cmp(&rank(&b.kind)).then(a.kind.cmp(&b.kind)));
    out
}

/// One field of a telecommand's packet header (`PCDF`).
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderField {
    pub name: String,
    pub description: String,
    /// `PCDF_TYPE`, as the MIB writes it.
    pub kind: String,
    pub bit: Option<i64>,
    pub bits: Option<i64>,
    /// The value it's fixed to, raw, when it is.
    pub value: Option<u64>,
    /// `PCPC_CODE`: what fills it when it isn't fixed.
    pub code: String,
}

/// `tc`'s packet header as `CCF_PKTID` -> `TCP`/`PCDF` lays it out:
/// the header's name and description, and its fields in order. `None`
/// when the MIB doesn't say (the mission profile's PUS header is used).
pub fn header(set: &MibSet, tc: DefRef) -> Option<(String, String, Vec<HeaderField>)> {
    let e = set.get(tc);
    let index = set.index();
    let root = Some(e.root);
    let id = e.row.clean("CCF_PKTID");
    if id.is_empty() {
        return None;
    }
    let mut rows: Vec<&Row> = index.rows_by_field("pcdf", "PCDF_TCNAME", id, root);
    if rows.is_empty() {
        return None;
    }
    rows.sort_by_key(|r| parse_int(r.clean("PCDF_BIT")));
    let descr = index.first_row_by_field("tcp", "TCP_ID", id, root).map(|r| r.clean("TCP_DESC").to_string()).unwrap_or_default();
    let fields = rows
        .into_iter()
        .map(|r| {
            let pname = r.clean("PCDF_PNAME");
            let pcpc = (!pname.is_empty()).then(|| index.first_row_by_field("pcpc", "PCPC_PNAME", pname, root)).flatten();
            let value = r.clean("PCDF_VALUE");
            HeaderField {
                name: if pname.is_empty() { r.clean("PCDF_DESC").to_string() } else { pname.to_string() },
                description: pcpc.map(|p| p.clean("PCPC_DESC").to_string()).filter(|d| !d.is_empty()).unwrap_or_else(|| r.clean("PCDF_DESC").to_string()),
                kind: r.clean("PCDF_TYPE").to_string(),
                bit: parse_int(r.clean("PCDF_BIT")),
                bits: parse_int(r.clean("PCDF_LEN")),
                value: (!value.is_empty()).then(|| values::parse_raw_int(value, Radix::of(r.clean("PCDF_RADIX")))).flatten().map(|n| n as u64),
                code: pcpc.map(|p| p.clean("PCPC_CODE").to_string()).unwrap_or_default(),
            }
        })
        .collect();
    Some((id.to_string(), descr, fields))
}

/// Where a built packet disagrees with the fixed values of the MIB's
/// header for it: (field, what the MIB fixes, what the packet has).
pub fn header_mismatches(set: &MibSet, tc: DefRef, packet: &[u8]) -> Vec<(String, u64, u64)> {
    let Some((_, _, fields)) = header(set, tc) else { return Vec::new() };
    fields
        .into_iter()
        .filter_map(|f| {
            let (Some(want), Some(bit), Some(bits)) = (f.value, f.bit, f.bits) else { return None };
            let have = fenix_ccsds::bits::get(packet, bit.max(0) as usize, (bits.max(0) as usize).min(64))?;
            (have != want).then_some((f.name, want, have))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::set::tests::{temp_root, write};
    use crate::Kind;

    fn line(fields: &[&str], n: usize) -> String {
        let mut v: Vec<&str> = fields.to_vec();
        v.resize(n.max(fields.len()), "");
        v.join("\t")
    }

    fn mib() -> MibSet {
        let r = temp_root("commanding");
        write(&r, "ccf", &line(&["ZTC1", "Heater", "", "", "N", "PUSTC", "8", "1", "100", "2", "", "", "", "", "", "", "", "HOT"], 21));
        write(&r, "cdf", &[line(&["ZTC1", "E", "", "8", "0", "0", "P1"], 10), line(&["ZTC1", "E", "", "8", "8", "0", "P2"], 10)].join("\n"));
        write(&r, "cpc", &[line(&["P1", "line", "3", "4"], 17), line(&["P2", "mode", "3", "4", "", "", "", "", "", "", "PAF1"], 17)].join("\n"));
        write(&r, "paf", "PAF1\tmode\tU\t2\n");
        write(&r, "pas", "PAF1\tOFF\t0\nPAF1\tON\t1\n");
        write(&r, "pst", "PS1\tHeater settings\n");
        write(&r, "psm", "ZTC1\tC\tPS1\n");
        write(&r, "psv", "HOT\tVS1\tAll on\nCOLD\tVS2\tAll off\n");
        write(&r, "pvs", "VS1\tPS1\tP1\tE\t3\t\nVS1\tPS1\tP2\tR\t1\t\nVS2\tPS1\tP1\tE\t1\t\nVS2\tPS1\tP2\tE\tOFF\t\n");
        write(&r, "ptv", "ZTC1\tNPWR\tE\tON\n");
        write(&r, "cvp", "ZTC1\tC\t10\nZTC1\tC\t11\n");
        write(&r, "cvs", "11\tC\tV\t2\t10\t\t1\n10\tA\tR\t0\t5\t10001\t\n");
        write(&r, "cve", "11\tNHTR\tE\tON\t\tA\n");
        write(&r, "tcp", "PUSTC\tPUS TC header\n");
        write(&r, "pcdf", &[line(&["PUSTC", "version", "K", "3", "0", "", "0", "D"], 8), line(&["PUSTC", "type", "K", "1", "3", "", "1", "D"], 8), line(&["PUSTC", "apid", "F", "11", "5", "APID"], 8)].join("\n"));
        write(&r, "pcpc", "APID\tApplication id\tA\n");
        MibSet::load(vec![r], None)
    }

    #[test]
    fn value_sets_preconditions_and_stages_come_from_their_tables() {
        let set = mib();
        let tc = set.find(Kind::Telecommand, 0, "ZTC1").unwrap();
        let sets = value_sets(&set, tc);
        assert_eq!(sets.iter().map(|s| (s.name.as_str(), s.default)).collect::<Vec<_>>(), vec![("HOT", true), ("COLD", false)]);
        assert_eq!(sets[0].engineering(&set, tc), vec![("P1".to_string(), "3".to_string()), ("P2".into(), "ON".into())], "raw 1 is ON");
        assert_eq!(preconditions(&set, tc)[0].to_string(), "NPWR = ON");
        let stages = verification(&set, tc);
        assert_eq!(stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), vec!["acceptance", "completion"]);
        assert_eq!(stages[1].checks[0].param, "NHTR");
        let (id, descr, fields) = header(&set, tc).unwrap();
        assert_eq!((id.as_str(), descr.as_str(), fields.len()), ("PUSTC", "PUS TC header", 3));
        assert_eq!(fields[2].code, "A");
        // Version 0, type 1 (TC): what a built TC has.
        let pkt = crate::packets::encode_tc(&set, tc, &[("P1".into(), "1".into()), ("P2".into(), "ON".into())], &fenix_ccsds::pus::Profile::default(), 0).unwrap();
        assert!(header_mismatches(&set, tc, &pkt.bytes).is_empty());
        assert_eq!(header_mismatches(&set, tc, &[0xFF, 0xFF]), vec![("version".to_string(), 0, 7)]);
    }
}
