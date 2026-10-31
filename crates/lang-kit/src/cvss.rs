//! A CVSS vector's base score: version 3.0 and 3.1's formula
//! (`CVSS:3.1/AV:N/AC:L/...` is 9.8), which is how RustSec's and OSV's
//! advisories say how bad they are, and version 2's. CVSS 4's vectors
//! are scored from FIRST's tables, not a formula, and aren't scored here.

/// The base score of a CVSS 2, 3.0 or 3.1 vector; `None` for another, or
/// one that's missing a base metric.
pub fn score(vector: &str) -> Option<f64> {
    let vector = vector.trim().trim_start_matches('(').trim_end_matches(')');
    let mut parts = vector.split('/');
    match parts.next()? {
        "CVSS:3.0" | "CVSS:3.1" => {}
        "CVSS:2.0" => return score_2(vector),
        first if first.starts_with("AV:") => return score_2(vector),
        _ => return None,
    }
    let metric = |key: &str| vector.split('/').find_map(|p| p.strip_prefix(key)?.strip_prefix(':'));
    let changed = match metric("S")? {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    let av = match metric("AV")? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let ac = match metric("AC")? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let pr = match (metric("PR")?, changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let ui = match metric("UI")? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let cia = |key: &str| match metric(key) {
        Some("H") => Some(0.56),
        Some("L") => Some(0.22),
        Some("N") => Some(0.0),
        _ => None,
    };
    let iss = 1.0 - (1.0 - cia("C")?) * (1.0 - cia("I")?) * (1.0 - cia("A")?);
    let impact = if changed { 7.52 * (iss - 0.029) - 3.25 * (iss - 0.02f64).powi(15) } else { 6.42 * iss };
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 8.22 * av * ac * pr * ui;
    let base = if changed { (1.08 * (impact + exploitability)).min(10.0) } else { (impact + exploitability).min(10.0) };
    Some(round_up(base))
}

/// A CVSS 2 vector's base score (`AV:N/AC:L/Au:N/C:P/I:P/A:P` is 7.5),
/// as OSV still gives some.
fn score_2(vector: &str) -> Option<f64> {
    let metric = |key: &str| vector.split('/').find_map(|p| p.strip_prefix(key)?.strip_prefix(':'));
    let av = match metric("AV")? {
        "L" => 0.395,
        "A" => 0.646,
        "N" => 1.0,
        _ => return None,
    };
    let ac = match metric("AC")? {
        "H" => 0.35,
        "M" => 0.61,
        "L" => 0.71,
        _ => return None,
    };
    let au = match metric("Au")? {
        "M" => 0.45,
        "S" => 0.56,
        "N" => 0.704,
        _ => return None,
    };
    let cia = |key: &str| match metric(key) {
        Some("N") => Some(0.0),
        Some("P") => Some(0.275),
        Some("C") => Some(0.660),
        _ => None,
    };
    let impact = 10.41 * (1.0 - (1.0 - cia("C")?) * (1.0 - cia("I")?) * (1.0 - cia("A")?));
    let exploitability = 20.0 * av * ac * au;
    let f = if impact == 0.0 { 0.0 } else { 1.176 };
    let base: f64 = (0.6 * impact + 0.4 * exploitability - 1.5) * f;
    Some((base.max(0.0) * 10.0).round() / 10.0)
}

/// CVSS 3.1's Roundup: up to one decimal, without float noise.
fn round_up(x: f64) -> f64 {
    let n = (x * 100_000.0).round() as i64;
    if n % 10_000 == 0 {
        n as f64 / 100_000.0
    } else {
        ((n / 10_000) + 1) as f64 / 10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_score_as_nvd_scores_them() {
        // RUSTSEC-2021-0003 (smallvec) and RUSTSEC-2020-0071 (time), with
        // NVD's scores for their CVEs.
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"), Some(9.8));
        assert_eq!(score("CVSS:3.1/AV:L/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H"), Some(6.2));
        assert_eq!(score("CVSS:3.0/AV:N/AC:L/PR:L/UI:N/S:C/C:L/I:L/A:N"), Some(6.4), "scope changed");
        assert_eq!(score("CVSS:3.1/AV:N/AC:H/PR:H/UI:R/S:U/C:N/I:N/A:N"), Some(0.0));
        assert_eq!(score("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H/SC:N/SI:N/SA:N"), None);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L"), None);
        assert_eq!(score("9.8"), None);
        // CVSS 2, as NVD scored CVE-2002-0392 and CVE-2019-0001's own.
        assert_eq!(score("AV:N/AC:L/Au:N/C:P/I:P/A:P"), Some(7.5));
        assert_eq!(score("(AV:N/AC:L/Au:N/C:C/I:C/A:C)"), Some(10.0));
        assert_eq!(score("CVSS:2.0/AV:L/AC:H/Au:N/C:N/I:N/A:P"), Some(1.2));
        assert_eq!(score("AV:N/AC:L/Au:N/C:N/I:N/A:N"), Some(0.0));
    }
}
