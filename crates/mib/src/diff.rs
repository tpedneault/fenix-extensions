//! Two versions of a MIB set side by side: every definition added,
//! removed or changed between them, field by field -- a delivery checked
//! before it's taken. Each set's default MIB is compared; a definition
//! is the same one when it has the same kind and name (a calibration's
//! table too). A telecommand's changes include its `CDF` elements, a
//! TM packet's its `PLF` or `VPD` entries, a calibration's its points,
//! texts or ranges.

use std::collections::BTreeMap;

use crate::row::Row;
use crate::set::{CalKind, Entry, Kind, MibSet};
use crate::types::parse_int;

/// What happened to a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    Added,
    Removed,
    /// Each field that differs: (field, old value, new value). A list's
    /// items are `cdf #2`, `plf #1`...: empty on the side without it.
    Changed(Vec<(String, String, String)>),
}

/// One definition that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: Kind,
    pub name: String,
    /// A calibration's kind: two of them can share an id.
    pub cal: Option<CalKind>,
    pub what: What,
}

/// The definitions of a set's default MIB, by kind, calibration table
/// and name.
fn defined(set: &MibSet) -> BTreeMap<(Kind, &'static str, String), &Entry> {
    let root = set.default_root();
    let mut out = BTreeMap::new();
    for kind in Kind::ALL {
        for e in set.entries(kind).iter().filter(|e| e.root == root) {
            out.entry((kind, e.cal.map(|c| c.table()).unwrap_or(""), e.name.clone())).or_insert(e);
        }
    }
    out
}

fn fields(row: &Row) -> BTreeMap<String, String> {
    row.fields().map(|(k, v)| (k.to_string(), v.trim().to_string())).collect()
}

/// A related table's rows for `e`, each as one line, in order.
fn related(set: &MibSet, e: &Entry) -> Vec<(&'static str, Vec<String>)> {
    let index = set.index();
    let rows = |table: &str, field: &str, key: &str| -> Vec<&Row> { index.rows_by_field(table, field, key, Some(e.root)) };
    let line = |r: &Row, cols: &[&str]| -> String { cols.iter().map(|c| r.clean(c)).collect::<Vec<_>>().join(" ") };
    match e.kind {
        Kind::Telecommand => {
            let mut cdf = rows("cdf", "CDF_CNAME", &e.name);
            if cdf.iter().all(|r| parse_int(r.clean("CDF_BIT")).is_some()) {
                cdf.sort_by_key(|r| parse_int(r.clean("CDF_BIT")));
            }
            let cols = ["CDF_ELTYPE", "CDF_PNAME", "CDF_BIT", "CDF_ELLEN", "CDF_GRPSIZE", "CDF_INTER", "CDF_VALUE"];
            let summary = cdf
                .iter()
                .map(|r| {
                    let mut s = format!("{} {} bit {} len {}", r.clean(cols[0]), if r.clean(cols[1]).is_empty() { "spare" } else { r.clean(cols[1]) }, r.clean(cols[2]), r.clean(cols[3]));
                    if parse_int(r.clean(cols[4])).unwrap_or(0) > 0 {
                        s.push_str(&format!(" repeats {}", r.clean(cols[4])));
                    }
                    if !r.clean(cols[6]).is_empty() {
                        s.push_str(&format!(" = {}{}", r.clean(cols[6]), if r.clean(cols[5]).is_empty() { String::new() } else { format!(" ({})", r.clean(cols[5])) }));
                    }
                    s
                })
                .collect();
            vec![("cdf", summary)]
        }
        Kind::TmPacket => {
            let mut plf = rows("plf", "PLF_SPID", &e.name);
            plf.sort_by_key(|r| (parse_int(r.clean("PLF_OFFBY")).unwrap_or(i64::MAX), parse_int(r.clean("PLF_OFFBI")).unwrap_or(0), r.clean("PLF_NAME").to_string()));
            let mut out = vec![("plf", plf.iter().map(|r| line(r, &["PLF_NAME", "PLF_OFFBY", "PLF_OFFBI", "PLF_NBOCC", "PLF_LGOCC"])).collect())];
            let tpsd = e.row.clean("PID_TPSD");
            if parse_int(tpsd).unwrap_or(-1) > 0 {
                let mut vpd = rows("vpd", "VPD_TPSD", tpsd);
                vpd.sort_by_key(|r| parse_int(r.clean("VPD_POS")).unwrap_or(i64::MAX));
                out.push(("vpd", vpd.iter().map(|r| line(r, &["VPD_POS", "VPD_NAME", "VPD_GRPSIZE", "VPD_FIXREP", "VPD_CHOICE", "VPD_PIDREF", "VPD_OFFSET"])).collect()));
            }
            out
        }
        Kind::Calibration => {
            let (table, field, cols): (&'static str, &str, &[&str]) = match e.cal {
                Some(CalKind::Numeric) => ("cap", "CAP_NUMBR", &["CAP_XVALS", "CAP_YVALS"]),
                Some(CalKind::TcNumeric) => ("ccs", "CCS_NUMBR", &["CCS_XVALS", "CCS_YVALS"]),
                Some(CalKind::Text) => ("txp", "TXP_NUMBR", &["TXP_FROM", "TXP_TO", "TXP_ALTXT"]),
                Some(CalKind::Status) => ("pas", "PAS_NUMBR", &["PAS_ALTXT", "PAS_ALVAL"]),
                Some(CalKind::Range) => ("prv", "PRV_NUMBR", &["PRV_MINVAL", "PRV_MAXVAL"]),
                _ => return Vec::new(),
            };
            vec![(table, rows(table, field, &e.name).iter().map(|r| line(r, cols)).collect())]
        }
        Kind::TcParam | Kind::TmParam => Vec::new(),
    }
}

fn changes(set_a: &MibSet, a: &Entry, set_b: &MibSet, b: &Entry) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let (fa, fb) = (fields(&a.row), fields(&b.row));
    let mut names: Vec<&String> = fa.keys().chain(fb.keys()).collect();
    // In the table's column order where there is one.
    let order: Vec<String> = a.row.fields().chain(b.row.fields()).map(|(k, _)| k.to_string()).collect();
    names.sort_by_key(|n| order.iter().position(|o| o == *n).unwrap_or(usize::MAX));
    names.dedup();
    for n in names {
        let (va, vb) = (fa.get(n).cloned().unwrap_or_default(), fb.get(n).cloned().unwrap_or_default());
        if va != vb {
            out.push((n.clone(), va, vb));
        }
    }
    let (ra, rb) = (related(set_a, a), related(set_b, b));
    for (table, la) in &ra {
        let lb = rb.iter().find(|(t, _)| t == table).map(|(_, l)| l.clone()).unwrap_or_default();
        for i in 0..la.len().max(lb.len()) {
            let (va, vb) = (la.get(i).cloned().unwrap_or_default(), lb.get(i).cloned().unwrap_or_default());
            if va != vb {
                out.push((format!("{table} #{}", i + 1), va, vb));
            }
        }
    }
    for (table, lb) in &rb {
        if ra.iter().all(|(t, _)| t != table) {
            for (i, vb) in lb.iter().enumerate() {
                out.push((format!("{table} #{}", i + 1), String::new(), vb.clone()));
            }
        }
    }
    out
}

/// Every definition that differs from `old` to `new`, by kind, then
/// calibration table, then name.
pub fn compare(old: &MibSet, new: &MibSet) -> Vec<Change> {
    let (a, b) = (defined(old), defined(new));
    let mut keys: Vec<&(Kind, &'static str, String)> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = Vec::new();
    for key in keys {
        let (kind, _, name) = key;
        let what = match (a.get(key), b.get(key)) {
            (Some(_), None) => What::Removed,
            (None, Some(_)) => What::Added,
            (Some(x), Some(y)) => {
                let c = changes(old, x, new, y);
                if c.is_empty() {
                    continue;
                }
                What::Changed(c)
            }
            (None, None) => continue,
        };
        let cal = a.get(key).or(b.get(key)).and_then(|e| e.cal);
        out.push(Change { kind: *kind, name: name.clone(), cal, what });
    }
    out
}

fn cell(s: &str) -> String {
    if s.is_empty() {
        "--".into()
    } else {
        format!("`{}`", s.replace('`', "'").replace('|', "\\|"))
    }
}

/// `changes` as Markdown: how many of each kind were added, removed and
/// changed, then each kind's.
pub fn report(changes: &[Change], old_label: &str, new_label: &str) -> String {
    let mut out = format!("# MIB changes: {old_label} → {new_label}\n\n");
    if changes.is_empty() {
        out.push_str("No differences.\n");
        return out;
    }
    out.push_str("| Kind | Added | Removed | Changed |\n|---|---:|---:|---:|\n");
    for kind in Kind::ALL {
        let of = |f: fn(&What) -> bool| changes.iter().filter(|c| c.kind == kind && f(&c.what)).count();
        let (add, rem, chg) = (of(|w| *w == What::Added), of(|w| *w == What::Removed), of(|w| matches!(w, What::Changed(_))));
        if add + rem + chg > 0 {
            out.push_str(&format!("| {} | {add} | {rem} | {chg} |\n", kind.plural()));
        }
    }
    for kind in Kind::ALL {
        let mine: Vec<&Change> = changes.iter().filter(|c| c.kind == kind).collect();
        if mine.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n", kind.plural()));
        let label = |c: &Change| match c.cal {
            Some(k) => format!("{} ({})", c.name, k.table()),
            None => c.name.clone(),
        };
        let list = |what: fn(&What) -> bool| -> Vec<String> { mine.iter().filter(|c| what(&c.what)).map(|c| label(c)).collect() };
        let added = list(|w| *w == What::Added);
        if !added.is_empty() {
            out.push_str(&format!("\n**Added:** {}\n", added.join(", ")));
        }
        let removed = list(|w| *w == What::Removed);
        if !removed.is_empty() {
            out.push_str(&format!("\n**Removed:** {}\n", removed.join(", ")));
        }
        for c in &mine {
            if let What::Changed(fields) = &c.what {
                out.push_str(&format!("\n### {}\n\n| Field | {old_label} | {new_label} |\n|---|---|---|\n", label(c)));
                for (f, a, b) in fields {
                    out.push_str(&format!("| {f} | {} | {} |\n", cell(a), cell(b)));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::set::tests::{temp_root, write};

    fn set(ccf_descr: &str, mode_bit: &str, extra_tc: bool, pas: &str) -> MibSet {
        let r = temp_root("diff");
        let mut ccf = format!("ZTC1\t{ccf_descr}\t\t\tN\t\t8\t1\t100\t1\n");
        if extra_tc {
            ccf.push_str("ZTC2\tNew one\t\t\tN\t\t8\t2\t100\t0\n");
        }
        write(&r, "ccf", &ccf);
        write(&r, "cdf", &format!("ZTC1\tE\tmode\t8\t{mode_bit}\t0\tPMODE\t\t\t\n"));
        write(&r, "cpc", "PMODE\tMode\t3\t4\t\t\t\t\t\t\tPAF1\n");
        write(&r, "paf", "PAF1\tModes\tU\t2\n");
        write(&r, "pas", pas);
        write(&r, "pcf", "NOLD\tGone soon\t\t\t3\t4\n");
        MibSet::load(vec![r], None)
    }

    #[test]
    fn additions_removals_and_field_changes_are_found() {
        let old = set("Set mode", "0", false, "PAF1\tOFF\t0\nPAF1\tON\t1\n");
        let new = {
            let s = set("Set the mode", "8", true, "PAF1\tOFF\t0\nPAF1\tON\t1\nPAF1\tAUTO\t2\n");
            std::fs::remove_file(s.roots()[0].path.join("pcf.dat")).unwrap();
            MibSet::load(s.roots().to_vec(), None)
        };
        let c = compare(&old, &new);
        let summary: Vec<(Kind, &str, &str)> = c
            .iter()
            .map(|c| {
                (
                    c.kind,
                    c.name.as_str(),
                    match c.what {
                        What::Added => "added",
                        What::Removed => "removed",
                        What::Changed(_) => "changed",
                    },
                )
            })
            .collect();
        assert_eq!(summary, vec![(Kind::Telecommand, "ZTC1", "changed"), (Kind::Telecommand, "ZTC2", "added"), (Kind::TmParam, "NOLD", "removed"), (Kind::Calibration, "PAF1", "changed")]);
        let What::Changed(f) = &c[0].what else { panic!() };
        assert_eq!(f[0], ("CCF_DESCR".to_string(), "Set mode".to_string(), "Set the mode".to_string()));
        assert_eq!(f[1], ("cdf #1".to_string(), "E PMODE bit 0 len 8".to_string(), "E PMODE bit 8 len 8".to_string()));
        let What::Changed(f) = &c[3].what else { panic!() };
        assert_eq!(f, &vec![("pas #3".to_string(), String::new(), "AUTO 2".to_string())]);
        let md = report(&c, "v1", "v2");
        assert!(md.contains("| Telecommands | 1 | 0 | 1 |"), "{md}");
        assert!(md.contains("**Removed:** NOLD"), "{md}");
        assert!(md.contains("| cdf #1 | `E PMODE bit 0 len 8` | `E PMODE bit 8 len 8` |"), "{md}");
        assert!(compare(&old, &old).is_empty());
        assert!(report(&[], "a", "b").contains("No differences"));
    }
}
