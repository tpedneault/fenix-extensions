//! Telecommand calls in a script, checked against the MIB: the calls are
//! found and read back through the project's templates (the same reading
//! `SPC k e` does), and what the MIB disagrees with becomes a diagnostic
//! -- an unknown mnemonic (with the nearest known ones), a missing
//! argument, a value outside its range or status texts. A call built at
//! run time (a variable mnemonic) isn't guessed at.

use std::collections::HashMap;

use fenix_ccsds::pus::Profile;
use fenix_mib::commanding;
use fenix_mib::layout::{self, Layout};

use fenix_mib::{DefRef, Kind, MibSet};

use crate::mib_form::{read_arguments, Templates};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// One finding: its line, the columns it covers, how bad, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub cols: std::ops::Range<usize>,
    pub severity: Severity,
    pub message: String,
}

/// The template's text right before `{mnemo}`: what a call's mnemonic
/// follows. `None` when the template doesn't name the mnemonic or has
/// nothing fixed before it.
fn mnemo_marker(template: &str) -> Option<String> {
    let before = template.split("{mnemo}").next()?;
    if before.len() == template.len() {
        return None;
    }
    let marker = before.rsplit('}').next().unwrap_or(before);
    (!marker.trim().is_empty()).then(|| marker.to_string())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + (ca != *cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The known telecommands closest to `name`.
fn nearest(set: &MibSet, name: &str) -> Vec<String> {
    let mut all: Vec<(usize, &str)> = set.entries(Kind::Telecommand).iter().map(|e| (levenshtein(name, &e.name), e.name.as_str())).collect();
    all.sort();
    all.into_iter().take_while(|(d, _)| *d <= 3).take(2).map(|(_, n)| n.to_string()).collect()
}

fn char_col(line: &str, byte: usize) -> usize {
    line[..byte.min(line.len())].chars().count()
}

/// Every finding in `text`.
pub fn check(text: &str, set: &MibSet, t: &Templates) -> Vec<Finding> {
    let Some(marker) = mnemo_marker(&t.command) else { return Vec::new() };
    let mut out = Vec::new();
    // A script calls the same few telecommands over and over, and each
    // one's parameters are a scan of the whole `cdf` table: once each.
    let mut known: HashMap<DefRef, (Layout, Vec<String>)> = HashMap::new();
    let mut unknown: HashMap<String, Vec<String>> = HashMap::new();
    for (ln, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with("--") {
            continue;
        }
        let Some(at) = line.find(&marker) else { continue };
        let start = at + marker.len();
        let word: String = line[start..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        let next = line[start + word.len()..].chars().next();
        if word.is_empty() || matches!(next, Some('$' | '[' | '(' | '{')) || line[start..].starts_with('$') {
            continue;
        }
        let cols = char_col(line, start)..char_col(line, start + word.len());
        let Some(tc) = set.resolve(&word).filter(|d| d.kind == Kind::Telecommand) else {
            let near = unknown.entry(word.clone()).or_insert_with(|| nearest(set, &word));
            let hint = if near.is_empty() { String::new() } else { format!(" -- did you mean {}?", near.join(" or ")) };
            out.push(Finding { line: ln, cols, severity: Severity::Error, message: format!("unknown telecommand {word}{hint}") });
            continue;
        };
        let e = set.get(tc);
        if e.row.clean("CCF_CRITICAL") == "Y" {
            out.push(Finding { line: ln, cols: cols.clone(), severity: Severity::Info, message: format!("{word} is a critical telecommand") });
        }
        let (layout, pre) = known.entry(tc).or_insert_with(|| (Layout::of(set, tc), commanding::preconditions(set, tc).iter().map(ToString::to_string).collect()));
        if !pre.is_empty() {
            out.push(Finding { line: ln, cols: cols.clone(), severity: Severity::Info, message: format!("{word} is sent only while {}", pre.join(" and ")) });
        }
        let mut names: Vec<String> = layout.asked().map(|el| el.name.clone()).collect();
        names.dedup();
        let args = read_arguments(line, &word, &names, t);
        // Each field takes the next value given for its parameter; a
        // counter's value says how many repetitions follow.
        let used = std::cell::RefCell::new(vec![false; args.len()]);
        let take = |name: &str| -> Option<usize> {
            let mut used = used.borrow_mut();
            let i = (0..args.len()).find(|&i| !used[i] && args[i].0 == name)?;
            used[i] = true;
            Some(i)
        };
        let mut given: HashMap<layout::Field, usize> = HashMap::new();
        let fields = layout.fields_by(|f| {
            let el = &layout.elements[f.element];
            match take(&el.name) {
                Some(i) => {
                    given.insert(f.clone(), i);
                    layout::count_of(el, &args[i].1)
                }
                None => 0,
            }
        });
        let value_at = |name: &str, value: &str| -> std::ops::Range<usize> {
            let name_at = line.find(name).unwrap_or(start);
            let vat = line[name_at..].find(value).map(|i| name_at + i).unwrap_or(name_at);
            char_col(line, vat)..char_col(line, vat + value.len().max(1))
        };
        for f in &fields {
            let el = &layout.elements[f.element];
            let at = match given.get(f) {
                Some(&i) => Some(i),
                None if el.is_repeater() => None,
                None => take(&el.name),
            };
            let Some(i) = at else {
                out.push(Finding { line: ln, cols: cols.clone(), severity: Severity::Warning, message: format!("{} is missing", f.label(layout)) });
                continue;
            };
            let value = &args[i].1;
            if let Some(w) = el.domain.as_ref().map(|d| d.check(value, &Profile::default())).and_then(|w| w.into_iter().next()) {
                out.push(Finding { line: ln, cols: value_at(&el.name, value), severity: Severity::Warning, message: w });
            }
        }
        for (i, (name, value)) in args.iter().enumerate() {
            if used.borrow()[i] {
                continue;
            }
            let repeated = layout.asked().any(|el| &el.name == name && !layout.enclosing(layout.elements.iter().position(|e| std::ptr::eq(e, el)).unwrap_or(0)).is_empty());
            let message = if repeated { format!("one more {name} than its counter says") } else { format!("{name} is given more than once") };
            out.push(Finding { line: ln, cols: value_at(name, value), severity: Severity::Warning, message });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn templates() -> Templates {
        Templates { command: "tc::send {mnemo} {arguments}".into(), argument: "-{name} {value}".into(), separator: ", ".into() }
    }

    #[test]
    fn unknown_mnemonics_missing_arguments_and_bad_values() {
        let root = crate::mib_page::tests::fixture();
        let set = MibSet::load(vec![root.clone()], None);
        let text = "proc checkout {} {\n    tc::send ZTC08101 -PTH00101 9, -PTH00102 AUTO\n    # tc::send NOPE\n    tc::send ZTC08109 -PTH00101 3\n    tc::send ZTC08101 -PTH00101 3\n    tc::send $cmd\n}\n";
        let found = check(text, &set, &templates());
        let msgs: Vec<(usize, &str)> = found.iter().map(|f| (f.line, f.message.as_str())).collect();
        assert!(found.iter().any(|f| f.line == 1 && f.message.contains("'9' is outside") && f.cols == (32..33)), "{msgs:?}");
        assert!(found.iter().any(|f| f.line == 3 && f.severity == Severity::Error && f.message.contains("did you mean ZTC08101")), "{msgs:?}");
        assert!(found.iter().any(|f| f.line == 4 && f.message == "PTH00102 is missing"), "{msgs:?}");
        assert!(!found.iter().any(|f| f.line == 2 || f.line == 5), "comments and variables are left alone: {msgs:?}");
        assert_eq!(mnemo_marker("telecommand_send PUS_T={type} PUS_ST={stype} APID={apid} MNEMO={mnemo} ARGUMENTS=[{arguments}]").as_deref(), Some(" MNEMO="));
        assert_eq!(mnemo_marker("{mnemo}({arguments})"), None);
        std::fs::remove_dir_all(&root.path).ok();
    }

    #[test]
    fn a_template_whose_arguments_are_just_values_is_checked_by_position() {
        // `{value}` alone used to hang the check -- and the editor with it --
        // as soon as a script had a call in it.
        let root = crate::mib_page::tests::fixture();
        let set = MibSet::load(vec![root.clone()], None);
        let t = Templates { command: "tc::send {mnemo} {arguments}".into(), argument: "{value}".into(), separator: " ".into() };
        let found = check("tc::send ZTC08101 9 AUTO\ntc::send ZTC08101 3\n", &set, &t);
        let msgs: Vec<(usize, &str)> = found.iter().map(|f| (f.line, f.message.as_str())).collect();
        assert!(found.iter().any(|f| f.line == 0 && f.message.contains("'9' is outside")), "{msgs:?}");
        assert!(found.iter().any(|f| f.line == 1 && f.message == "PTH00102 is missing"), "{msgs:?}");
        std::fs::remove_dir_all(&root.path).ok();
    }

    #[test]
    fn grouped_calls_are_checked_against_their_counters() {
        let root = crate::mib_page::tests::fixture();
        std::fs::write(root.path.join("ccf.dat"), "ZTC11004\tInsert\t\t\tN\t\t11\t4\t1008\n").unwrap();
        std::fs::write(root.path.join("cdf.dat"), "ZTC11004\tE\tCount\t8\t0\t1\tPN\t\t\t\nZTC11004\tE\tTime\t8\t8\t0\tPT\t\t\t\nZTC11004\tE\tAfter\t8\t16\t0\tPA\t\t\t\n").unwrap();
        std::fs::write(root.path.join("cpc.dat"), "PN\tCount\t3\t4\nPT\tTime\t3\t4\nPA\tAfter\t3\t4\n").unwrap();
        std::fs::write(root.path.join("ptv.dat"), "ZTC11004\tNMODE\tE\tNOMINAL\n").unwrap();
        let set = MibSet::load(vec![root.clone()], None);
        let text = "tc::send ZTC11004 -PN 2, -PT 1, -PA 3\ntc::send ZTC11004 -PN 1, -PT 1, -PT 2, -PA 3\ntc::send ZTC11004 -PN 1, -PT 1, -PA 3\n";
        let found = check(text, &set, &templates());
        let msgs: Vec<(usize, &str)> = found.iter().map(|f| (f.line, f.message.as_str())).collect();
        assert!(found.iter().any(|f| f.line == 0 && f.message == "PT[2] is missing"), "{msgs:?}");
        assert!(found.iter().any(|f| f.line == 1 && f.message == "one more PT than its counter says"), "{msgs:?}");
        assert!(!found.iter().any(|f| f.line == 2 && f.severity == Severity::Warning), "{msgs:?}");
        assert!(found.iter().any(|f| f.line == 2 && f.severity == Severity::Info && f.message.contains("NMODE = NOMINAL")), "{msgs:?}");
        std::fs::remove_dir_all(&root.path).ok();
    }
}
