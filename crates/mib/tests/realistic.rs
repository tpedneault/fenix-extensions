//! A mission-like MIB, end to end: PUS service 3's nested groups, a
//! deduced value in service 20, an editable default in CDF_VALUE, raw
//! values in hex, a raw range behind a numeric calibration, a float, a
//! time, a variable-length string and octets, value sets, PTV,
//! verification stages, a TCP/PCDF header, a sequence, and a cdf.dat
//! written in Latin-1. Every telecommand is filled as the insert form
//! fills it, built, read back, identified, checked, exported to XTCE
//! and read again.

use std::collections::HashMap;

use fenix_ccsds::pus::Profile;
use fenix_mib::layout::{self, Layout};
use fenix_mib::packets::{self, OffsetBase};
use fenix_mib::{checks, commanding, diff, generate, sequence, Kind, MibRoot, MibSet};

fn line(fields: &[&str], n: usize) -> String {
    let mut v: Vec<&str> = fields.to_vec();
    v.resize(n.max(fields.len()), "");
    v.join("\t")
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fenix-mib-realistic-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(d: &std::path::Path, table: &str, rows: &[String]) {
    std::fs::write(d.join(format!("{table}.dat")), rows.join("\n") + "\n").unwrap();
}

fn mission(d: &std::path::Path) {
    write(
        d,
        "ccf",
        &[
            line(&["ZHK00301", "Create HK report", "", "", "N", "", "3", "1", "100", "8", "", "", "", "", "DHS", "", "", "HK_DEF", "", "9"], 21),
            line(&["ZPS02003", "Set parameter values", "", "", "Y", "", "20", "3", "100", "3", "", "", "", "", "DHS", "", "", "", "", "9"], 21),
            line(&["ZHT08001", "Heater control", "", "", "N", "PUSTC", "8", "1", "101", "5", "", "", "", "", "TCS", "", "", "", "", "9"], 21),
            line(&["ZTM09001", "Time and labels", "", "", "N", "", "9", "1", "102", "4", "", "", "", "", "DHS", "", "", "", "", "9"], 21),
        ],
    );
    let mut cdf = vec![
        line(&["ZHK00301", "E", "SID", "16", "0", "0", "PSID"], 10),
        line(&["ZHK00301", "E", "Interval", "16", "16", "0", "PINT"], 10),
        line(&["ZHK00301", "E", "N1", "8", "32", "1", "PN1"], 10),
        line(&["ZHK00301", "E", "Parameter", "16", "40", "0", "PPAR"], 10),
        line(&["ZHK00301", "E", "NFA", "8", "56", "3", "PNFA"], 10),
        line(&["ZHK00301", "E", "Super-commutation", "8", "64", "0", "PREP"], 10),
        line(&["ZHK00301", "E", "N2", "8", "72", "1", "PN2"], 10),
        line(&["ZHK00301", "E", "Parameter", "16", "80", "0", "PPAR"], 10),
        line(&["ZPS02003", "E", "N", "8", "0", "2", "PN"], 10),
        line(&["ZPS02003", "E", "Parameter id", "16", "8", "0", "PPID"], 10),
        line(&["ZPS02003", "E", "Value", "16", "24", "0", "PVAL"], 10),
        line(&["ZHT08001", "F", "Function", "8", "0", "0", "PFID", "R", "0C"], 10),
        line(&["ZHT08001", "E", "Line", "8", "8", "0", "PLINE", "R", "3"], 10),
        line(&["ZHT08001", "E", "Mode", "8", "16", "0", "PMODE"], 10),
        line(&["ZHT08001", "E", "Gain", "32", "24", "0", "PGAIN"], 10),
        line(&["ZHT08001", "E", "Set point", "16", "56", "0", "PSET"], 10),
        line(&["ZTM09001", "E", "Release", "48", "0", "0", "PTIME"], 10),
        line(&["ZTM09001", "E", "Label", "8", "48", "0", "PLBL"], 10),
        line(&["ZTM09001", "E", "Key", "32", "56", "0", "POCT"], 10),
        line(&["ZTM09001", "E", "Flag", "8", "88", "0", "PFLAG"], 10),
    ]
    .join("\n")
    .into_bytes();
    // A description in Latin-1: a degree sign.
    cdf.extend(b"\nZTM09001\tA\tR\xe9serve \xb0\t8\t96\t0\t\t\t0\t\n");
    std::fs::write(d.join("cdf.dat"), cdf).unwrap();
    write(
        d,
        "cpc",
        &[
            line(&["PSID", "Report SID", "3", "12", "", "", "", "", "", "", "", "", "1"], 17),
            line(&["PINT", "Collection interval", "3", "12", "", "", "s", "", "", "", "", "", "8"], 17),
            line(&["PN1", "Count", "3", "4", "", "", "", "", "", "", "", "", "1"], 17),
            line(&["PPAR", "Parameter id", "3", "12", "", "", "", "", "", "", "", "", "0"], 17),
            line(&["PNFA", "Fixed-length arrays", "3", "4", "", "", "", "", "", "", "", "", "0"], 17),
            line(&["PREP", "Super-commutation", "3", "4", "", "", "", "", "", "", "", "", "1"], 17),
            line(&["PN2", "Count", "3", "4", "", "", "", "", "", "", "", "", "1"], 17),
            line(&["PN", "Count", "3", "4", "", "", "", "", "", "", "", "", "1"], 17),
            line(&["PPID", "Parameter id", "3", "12", "", "", "", "P"], 17),
            line(&["PVAL", "Value", "11", "0"], 17),
            line(&["PFID", "Function id", "3", "4", "", "H"], 17),
            line(&["PLINE", "Heater line", "3", "4", "", "", "", "", "PRF01"], 17),
            line(&["PMODE", "Heater mode", "2", "8", "", "H", "", "", "", "", "PAF01", "E", "AUTO"], 17),
            line(&["PGAIN", "Loop gain", "5", "1", "", "", "", "", "PRF02", "", "", "E", "1.5"], 17),
            line(&["PSET", "Set point", "3", "12", "", "", "degC", "", "PRF03", "CCA01", "", "E", "20"], 17),
            line(&["PTIME", "Release time", "9", "17"], 17),
            line(&["PLBL", "Label", "8", "0"], 17),
            line(&["POCT", "Key", "7", "4"], 17),
            line(&["PFLAG", "Flag", "3", "4", "", "H", "", "", "", "", "", "", "1F"], 17),
        ],
    );
    write(d, "paf", &[line(&["PAF01", "Heater mode", "U", "3"], 4)]);
    write(d, "pas", &["PAF01\tOFF\t0".into(), "PAF01\tON\t1".into(), "PAF01\tAUTO\t0A".into()]);
    write(d, "prf", &[line(&["PRF01", "Lines", "E", "", "", "1"], 7), line(&["PRF02", "Gain", "E", "R", "", "1"], 7), line(&["PRF03", "Set point raw", "R", "", "H", "1"], 7)]);
    write(d, "prv", &["PRF01\t1\t8".into(), "PRF02\t0\t10.5".into(), "PRF03\t0\tFFF".into()]);
    write(d, "cca", &[line(&["CCA01", "Set point", "R", "U", "D", "degC", "2"], 7)]);
    // (engineering, raw): -40 degC is raw 0, 85 degC raw 4000.
    write(d, "ccs", &["CCA01\t-40\t0".into(), "CCA01\t85\t4000".into()]);
    write(d, "pcf", &[line(&["NTEMP", "Temperature", "1001", "degC", "4", "12"], 24), line(&["NMODE", "Mode", "1002", "", "3", "4"], 24), line(&["NPWR", "Power", "1003", "", "3", "4"], 24), line(&["NSID", "SID", "", "", "3", "4"], 24)]);
    write(d, "pst", &["PS_HK\tHK settings".into()]);
    write(d, "psm", &["ZHK00301\tC\tPS_HK".into()]);
    write(d, "psv", &["HK_DEF\tVS1\tNominal".into(), "HK_FAST\tVS2\tFast".into()]);
    write(d, "pvs", &["VS1\tPS_HK\tPSID\tE\t1\t".into(), "VS1\tPS_HK\tPINT\tE\t8\t".into(), "VS2\tPS_HK\tPSID\tE\t2\t".into(), "VS2\tPS_HK\tPINT\tE\t1\t".into()]);
    write(d, "ptv", &["ZHT08001\tNPWR\tE\tON".into()]);
    write(d, "cvp", &["ZHT08001\tC\tACC".into(), "ZHT08001\tC\tCOMP".into()]);
    write(d, "cvs", &["ACC\tA\tR\t0\t5\t10001\t".into(), "COMP\tC\tV\t1\t30\t\t2".into()]);
    write(d, "cve", &["COMP\tNMODE\tE\tAUTO\t\tA".into()]);
    write(d, "tcp", &["PUSTC\tPUS-C TC".into()]);
    write(d, "pcdf", &[line(&["PUSTC", "version", "K", "3", "0", "", "0", "D"], 8), line(&["PUSTC", "type", "K", "1", "3", "", "1", "D"], 8), line(&["PUSTC", "apid", "F", "11", "5", "APID"], 8)]);
    write(d, "pcpc", &["APID\tApplication id\tA".into()]);
    write(d, "csf", &[line(&["SQHT0001", "Heater on", "", "", "1"], 16)]);
    write(d, "csp", &[line(&["SQHT0001", "FLINE", "1", "Line", "3", "4", "", "", "", "E", "2"], 16)]);
    write(d, "css", &[line(&["SQHT0001", "Switch the heater", "1", "C", "ZHT08001", "2"], 18)]);
    write(d, "sdf", &[line(&["SQHT0001", "1", "ZHT08001", "1", "PLINE", "", "F", "FLINE"], 10), line(&["SQHT0001", "1", "ZHT08001", "2", "PMODE", "", "E", "ON"], 10)]);
    // Two HK packets told apart by their SID (PI1) alone.
    write(d, "pid", &[line(&["3", "25", "100", "1", "0", "5001", "HK one", "", "", "13"], 16), line(&["3", "25", "100", "2", "0", "5002", "HK two", "", "", "13"], 16)]);
    write(d, "pic", &["3\t25\t19\t8\t-1\t0\t".into()]);
    write(d, "tpcf", &["5001\tHK1\t".into(), "5002\tHK2\t".into()]);
    write(d, "plf", &["NSID\t5001\t0\t0".into(), "NTEMP\t5001\t1\t0".into(), "NSID\t5002\t0\t0".into(), "NMODE\t5002\t1\t0".into()]);
}

fn args(a: &[(&str, &str)]) -> Vec<(String, String)> {
    a.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
}

/// The fields the insert form would show for `values`: counters read
/// from the values in order, the way the form expands them.
fn form_fields(l: &Layout, values: &[(String, String)]) -> Vec<String> {
    let mut next: HashMap<String, usize> = HashMap::new();
    let fields = l.fields_by(|f| {
        let el = &l.elements[f.element];
        let k = next.entry(el.name.clone()).or_default();
        let v = values.iter().filter(|(n, _)| *n == el.name).nth(*k).map(|(_, v)| v.clone()).unwrap_or_default();
        *k += 1;
        layout::count_of(el, &v)
    });
    fields.iter().map(|f| f.label(l)).collect()
}

#[test]
fn a_mission_mib_round_trips_end_to_end() {
    let d = dir("ops");
    mission(&d);
    let set = MibSet::load(vec![MibRoot { label: "OPS".into(), path: d.clone() }], None);
    let problems: Vec<&str> = set.problems().iter().map(|p| p.message.as_str()).collect();
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("Latin-1"), "{problems:#?}");
    let p = Profile::default();

    // The standards checks find nothing in this MIB's telecommands.
    let off: Vec<String> = ["apid", "pus", "size", "identification", "checksum", "time"].iter().map(|s| s.to_string()).collect();
    let found: Vec<String> = checks::run(&set, &checks::Options { disabled: &off, base: OffsetBase::AfterHeaders, profile: &p }).into_iter().map(|p| p.message).collect();
    assert!(found.is_empty(), "{found:#?}");

    type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<&'a str>, Vec<(&'a str, &'a str)>);
    let cases: Vec<Case> = vec![
        (
            "ZHK00301",
            vec![("PSID", "5"), ("PINT", "8"), ("PN1", "2"), ("PPAR", "1001"), ("PPAR", "1002"), ("PNFA", "1"), ("PREP", "4"), ("PN2", "2"), ("PPAR", "1003"), ("PPAR", "1001")],
            vec!["PSID", "PINT", "PN1", "PPAR[1]", "PPAR[2]", "PNFA", "PREP[1]", "PN2[1]", "PPAR[1.1]", "PPAR[1.2]"],
            vec![("PPAR[1.2]", "1001"), ("PN2[1]", "2")],
        ),
        ("ZPS02003", vec![("PN", "2"), ("PPID", "1001"), ("PVAL", "-5"), ("PPID", "1002"), ("PVAL", "7")], vec!["PN", "PPID[1]", "PVAL[1]", "PPID[2]", "PVAL[2]"], vec![("PVAL[1]", "-5"), ("PVAL[2]", "7")]),
        ("ZHT08001", vec![("PLINE", "4"), ("PMODE", "AUTO"), ("PGAIN", "2.5"), ("PSET", "22.5")], vec!["PLINE", "PMODE", "PGAIN", "PSET"], vec![("PFID", "12"), ("PMODE", "10"), ("PSET", "2000")]),
        ("ZTM09001", vec![("PTIME", "2026-01-01T00:00:00"), ("PLBL", "\"HEATER\""), ("POCT", "DEADBEEF"), ("PFLAG", "1")], vec!["PTIME", "PLBL", "POCT", "PFLAG"], vec![("PLBL", "\"HEATER\""), ("POCT", "DE AD BE EF"), ("PFLAG", "1")]),
    ];
    // Test vectors build every telecommand: an argument with no default
    // gets a value of its type (octets as zeros, not "0").
    let v = generate::generate("vectors", &set, &p, OffsetBase::AfterHeaders).unwrap();
    assert!(v.lines().skip(1).all(|l| !l.contains('#')), "{v}");
    let x = generate::generate("xtce", &set, &p, OffsetBase::AfterHeaders).unwrap();
    let xdir = dir("xtce");
    let xfile = xdir.join("ops.xml");
    std::fs::write(&xfile, &x).unwrap();
    let back = MibSet::load(vec![MibRoot { label: "X".into(), path: xfile }], None);
    // Read back, it finds what the MIB itself does: no packets that can't
    // be told apart (PI1 kept), no defaults that don't fit (initialValue
    // isn't read in CPC_RADIX).
    let again: Vec<String> = checks::run(&back, &checks::Options { disabled: &off, base: OffsetBase::AfterHeaders, profile: &p }).into_iter().map(|p| p.message).collect();
    assert!(again.is_empty(), "{again:#?}");
    let pkt5002 = [0x08, 0x64, 0xC0, 0x00, 0x00, 0x10, 0x20, 0x03, 0x19, 0, 1, 0, 0, 0, 0, 0x03, 0xE8, 0, 0, 2, 1, 0, 0];
    let id = |s: &MibSet| packets::decode_packet(Some(s), &pkt5002, &p, OffsetBase::AfterHeaders).and_then(|(_, d)| d).map(|d| s.get(d).name.clone());
    assert_eq!((id(&set), id(&back)), (Some("5002".to_string()), Some("5002".to_string())), "SID 2 is SPID 5002, from the XTCE too");
    for (i, e) in set.entries(Kind::Telecommand).iter().enumerate() {
        let ours = Layout::of(&set, fenix_mib::DefRef { kind: Kind::Telecommand, index: i });
        let theirs = Layout::of(&back, back.find(Kind::Telecommand, 0, &e.name).unwrap());
        let offered = |l: &Layout| l.asked().map(|el| (el.name.clone(), el.offered.clone())).collect::<Vec<_>>();
        assert_eq!(offered(&theirs), offered(&ours), "{}: the defaults the form offers, from the XTCE", e.name);
    }
    for (name, values, labels, raws) in cases {
        let tc = set.find(Kind::Telecommand, 0, name).unwrap();
        let l = Layout::of(&set, tc);
        assert!(l.problems.is_empty(), "{name}: {:?}", l.problems);
        let a = args(&values);
        assert_eq!(form_fields(&l, &a), labels, "{name}: the form's fields");
        for (el, v) in l.elements.iter().filter(|e| e.asked()).map(|e| (e, a.iter().find(|(n, _)| *n == e.name).map(|(_, v)| v.as_str()).unwrap_or(""))) {
            let w = el.domain.as_ref().unwrap().check(v, &p);
            assert!(w.is_empty(), "{name}: {} = {v}: {w:?}", el.name);
        }
        let pkt = packets::encode_tc(&set, tc, &a, &p, 1).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (root, def) = packets::decode_packet(Some(&set), &pkt.bytes, &p, OffsetBase::AfterHeaders).unwrap();
        assert_eq!(def, Some(tc), "{name} is identified back");
        for (field, raw) in raws {
            let f = root.find(field).unwrap_or_else(|| panic!("{name}: {field} in {root:#?}"));
            assert_eq!(f.raw, raw, "{name}: {field}");
        }
        assert!(commanding::header_mismatches(&set, tc, &pkt.bytes).is_empty(), "{name}");
        // The same packet from the MIB read back from its XTCE.
        let theirs = back.find(Kind::Telecommand, 0, name).unwrap();
        let again = packets::encode_tc(&back, theirs, &a, &p, 1).unwrap_or_else(|e| panic!("{name} from XTCE: {e}"));
        assert_eq!(again.bytes, pkt.bytes, "{name} from XTCE");
        if name == "ZTM09001" {
            // A time argument decodes as the time typed, not its octets.
            let t = root.find("PTIME").unwrap();
            assert_eq!((t.raw.as_str(), t.value.as_str()), ("7F E8 17 A5 00 00", "2026-01-01 00:00:00.000 UTC"));
        }
    }

    // The form's defaults: CDF_VALUE raw 3, a status default, a raw
    // engineering curve default, and value set HK_DEF first.
    let heater = set.find(Kind::Telecommand, 0, "ZHT08001").unwrap();
    let l = Layout::of(&set, heater);
    let offered: Vec<(String, String)> = l.asked().map(|e| (e.name.clone(), e.offered.clone())).collect();
    assert_eq!(offered, args(&[("PLINE", "3"), ("PMODE", "AUTO"), ("PGAIN", "1.5"), ("PSET", "20")]));
    let hk = set.find(Kind::Telecommand, 0, "ZHK00301").unwrap();
    let sets = commanding::value_sets(&set, hk);
    assert_eq!((sets[0].name.as_str(), sets[0].default, sets[0].engineering(&set, hk)), ("HK_DEF", true, args(&[("PSID", "1"), ("PINT", "8")])));
    assert_eq!(commanding::preconditions(&set, heater)[0].to_string(), "NPWR = ON");
    assert_eq!(commanding::verification(&set, heater).iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), vec!["acceptance", "completion"]);
    // Out of range: raw 0..FFF behind the curve, engineering 1..8.
    let pset = l.elements.iter().find(|e| e.name == "PSET").unwrap().domain.clone().unwrap();
    assert!(pset.check("84", &p).is_empty());
    assert!(!pset.check("90", &p).is_empty(), "raw 4160 is past FFF");
    assert!(!l.elements.iter().find(|e| e.name == "PLINE").unwrap().domain.as_ref().unwrap().check("9", &p).is_empty());

    // The sequence sends the heater command with its formal's default.
    let seq = sequence::sequence(&set, 0, "SQHT0001").unwrap();
    let expanded = sequence::expand(&set, &seq, &HashMap::new());
    let lines = sequence::render(&set, &expanded, "tc::send {mnemo} {arguments}", "-{name} {value}", ", ", "#");
    assert!(lines.iter().any(|l| l.contains("tc::send ZHT08001 -PLINE 2, -PMODE ON, -PGAIN 1.5, -PSET 20")), "{lines:#?}");

    // A changed default is a change between two MIB versions.
    let d2 = dir("ops2");
    mission(&d2);
    let cpc = std::fs::read_to_string(d2.join("cpc.dat")).unwrap().replace("Loop gain\t5\t1\t\t\t\t\tPRF02\t\t\tE\t1.5", "Loop gain\t5\t1\t\t\t\t\tPRF02\t\t\tE\t2.0");
    std::fs::write(d2.join("cpc.dat"), cpc).unwrap();
    let set2 = MibSet::load(vec![MibRoot { label: "OPS2".into(), path: d2.clone() }], None);
    let changes = diff::compare(&set, &set2);
    assert_eq!(changes.len(), 1, "{changes:#?}");
    assert_eq!(changes[0].name, "PGAIN");
    for p in [d, xdir, d2] {
        std::fs::remove_dir_all(p).ok();
    }
}
