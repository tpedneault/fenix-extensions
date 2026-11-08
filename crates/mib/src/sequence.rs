//! Command sequences, as the ICD's sequence tables give them: a sequence
//! (`CSF`), its formal parameters (`CSP`), its steps in entry order
//! (`CSS`: a telecommand, another sequence, or a line of text), and the
//! values each step's parameters take (`SDF`: a raw or engineering value,
//! or one of the sequence's formal parameters). `expand` turns a sequence
//! and values for its formal parameters into the telecommands it sends,
//! each with its arguments in the order `Layout::fields` asks for them --
//! groups repeated as the step's counter values say -- and `render`
//! writes them out with the project's templates.

use std::collections::HashMap;

use crate::layout::{Field, Layout};
use crate::row::Row;
use crate::set::{CalKind, DefRef, Kind, MibSet};
use crate::telecommand;
use crate::types::parse_int;
use crate::values::{self, Interp, ParamType, Radix};

/// How deep sequences calling sequences are followed.
pub const MAX_DEPTH: usize = 16;

/// One of a sequence's formal parameters (`CSP`).
#[derive(Debug, Clone)]
pub struct Formal {
    pub name: String,
    /// `CSP_FPNUM`: its place in the sequence's list.
    pub number: i64,
    pub description: String,
    pub ty: ParamType,
    pub unit: String,
    /// `CSP_DEFVAL`, as someone types it.
    pub default: String,
}

/// One value a step gives a parameter (`SDF`).
#[derive(Debug, Clone)]
pub struct StepValue {
    /// `SDF_PNAME`: a telecommand's parameter, or a called sequence's
    /// formal parameter.
    pub param: String,
    /// `SDF_POS`.
    pub pos: i64,
    /// `SDF_REPPOS`: which occurrence of the parameter, in a repeated
    /// group (1-based); 1 when the MIB leaves it empty.
    pub rep: usize,
    /// `SDF_VTYPE`: `R` raw, `E` engineering, `F` the formal parameter
    /// `value` names.
    pub vtype: String,
    pub value: String,
    /// `SDF_VALSET`: a parameter value set named instead of a value.
    pub valset: String,
}

/// What a step does (`CSS_TYPE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// `C`: sends a telecommand.
    Command,
    /// `S`: runs another sequence.
    Sequence,
    /// `T`: a line of text.
    Text,
    /// A type this doesn't know, as the MIB writes it.
    Other(String),
}

/// One step of a sequence (`CSS`).
#[derive(Debug, Clone)]
pub struct Step {
    /// `CSS_ENTRY`.
    pub entry: String,
    pub kind: StepKind,
    /// `CSS_ELEMID`: the telecommand or sequence.
    pub element: String,
    /// `CSS_COMM`.
    pub comment: String,
    /// When it's released, for a person: `CSS_RELTYPE`, `CSS_RELTIME`,
    /// `CSS_EXTIME`.
    pub timing: String,
    pub values: Vec<StepValue>,
}

/// A sequence (`CSF`) with its formal parameters and steps.
#[derive(Debug, Clone)]
pub struct Sequence {
    pub name: String,
    pub description: String,
    pub root: usize,
    pub formals: Vec<Formal>,
    pub steps: Vec<Step>,
}

/// One step of an expanded sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandedKind {
    /// A telecommand, with its arguments in `Layout::fields`' order.
    Command { tc: DefRef, args: Vec<(String, String)> },
    /// A line of text: a textual step, or where a called sequence starts.
    Text(String),
}

/// One step as `expand` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    /// The sequence the step is in: the one expanded, or one it calls.
    pub sequence: String,
    pub entry: String,
    /// How many sequence calls deep it is: 0 for the sequence's own steps.
    pub depth: usize,
    pub kind: ExpandedKind,
    pub timing: String,
    pub comment: String,
    pub problems: Vec<String>,
}

fn entry_order(e: &str) -> (i64, String) {
    (parse_int(e).unwrap_or(i64::MAX), e.to_string())
}

fn param_type(set: &MibSet, root: usize, row: &Row, prefix: &str) -> ParamType {
    let f = |c: &str| row.clean(&format!("{prefix}_{c}")).to_string();
    ParamType {
        name: f("FPNAME"),
        ptc: f("PTC").parse().unwrap_or(0),
        pfc: f("PFC").parse().unwrap_or(0),
        radix: Radix::of(&f("RADIX")),
        categ: f("CATEG"),
        unit: f("UNIT"),
        status: set.find_cal(root, CalKind::Status, &f("PAFREF")),
        numeric: set.find_cal(root, CalKind::TcNumeric, &f("CCAREF")),
        range: set.find_cal(root, CalKind::Range, &f("PRFREF")),
        root,
    }
}

/// Every sequence in the set: (its MIB, name, description).
pub fn sequences(set: &MibSet) -> Vec<(usize, String, String)> {
    set.index().rows("csf").iter().map(|r| (r.source.root_index, r.clean("CSF_NAME").to_string(), r.clean("CSF_DESC").to_string())).collect()
}

/// Sequence `name` of MIB `root`.
pub fn sequence(set: &MibSet, root: usize, name: &str) -> Option<Sequence> {
    let index = set.index();
    let csf = index.first_row_by_field("csf", "CSF_NAME", name, Some(root))?;
    let name = csf.clean("CSF_NAME").to_string();
    let mut formals: Vec<Formal> = index
        .rows_by_field("csp", "CSP_SQNAME", &name, Some(root))
        .into_iter()
        .map(|r| {
            let ty = param_type(set, root, r, "CSP");
            let default = values::mib_value_as_engineering(set, &ty, r.clean("CSP_DEFVAL"), &Interp::of(r.clean("CSP_VTYPE")));
            Formal {
                name: r.clean("CSP_FPNAME").to_string(),
                number: parse_int(r.clean("CSP_FPNUM")).unwrap_or(i64::MAX),
                description: r.clean("CSP_DESCR").to_string(),
                unit: ty.unit.clone(),
                ty,
                default,
            }
        })
        .collect();
    formals.sort_by_key(|f| f.number);
    let sdf = index.rows_by_field("sdf", "SDF_SQNAME", &name, Some(root));
    let mut steps: Vec<Step> = index
        .rows_by_field("css", "CSS_SQNAME", &name, Some(root))
        .into_iter()
        .map(|r| {
            let entry = r.clean("CSS_ENTRY").to_string();
            let element = r.clean("CSS_ELEMID").to_string();
            let kind = match r.clean("CSS_TYPE") {
                "C" | "c" => StepKind::Command,
                "S" | "s" => StepKind::Sequence,
                "T" | "t" => StepKind::Text,
                other => StepKind::Other(other.to_string()),
            };
            let mut timing = Vec::new();
            let (reltype, reltime, extime) = (r.clean("CSS_RELTYPE"), r.clean("CSS_RELTIME"), r.clean("CSS_EXTIME"));
            if !reltime.is_empty() {
                let how = match reltype {
                    "R" | "r" => "after the previous step",
                    "A" | "a" => "absolute",
                    "" => "",
                    other => other,
                };
                timing.push(if how.is_empty() { format!("release {reltime}") } else { format!("release {reltime} {how}") });
            }
            if !extime.is_empty() {
                timing.push(format!("execute {extime}"));
            }
            let mut values: Vec<StepValue> = sdf
                .iter()
                .filter(|s| s.clean("SDF_ENTRY") == entry && (s.clean("SDF_ELEMID").is_empty() || s.clean("SDF_ELEMID") == element))
                .map(|s| StepValue {
                    param: s.clean("SDF_PNAME").to_string(),
                    pos: parse_int(s.clean("SDF_POS")).unwrap_or(i64::MAX),
                    rep: parse_int(s.clean("SDF_REPPOS")).filter(|n| *n > 0).unwrap_or(1) as usize,
                    vtype: s.clean("SDF_VTYPE").to_string(),
                    value: s.clean("SDF_VALUE").to_string(),
                    valset: s.clean("SDF_VALSET").to_string(),
                })
                .collect();
            values.sort_by_key(|v| (v.pos, v.rep));
            Step { entry, kind, element, comment: r.clean("CSS_COMM").to_string(), timing: timing.join(", "), values }
        })
        .collect();
    steps.sort_by_key(|s| entry_order(&s.entry));
    Some(Sequence { name, description: csf.clean("CSF_DESC").to_string(), root, formals, steps })
}

/// A step value as someone types it, its formal parameters substituted:
/// `None` (and a problem) when it can't be had.
fn step_value(set: &MibSet, v: &StepValue, ty: Option<&ParamType>, formals: &HashMap<String, String>, seq: &Sequence, problems: &mut Vec<String>) -> Option<String> {
    if !v.valset.is_empty() && v.value.is_empty() {
        problems.push(format!("{}: takes value set {}, which isn't applied here", v.param, v.valset));
        return None;
    }
    match v.vtype.as_str() {
        "F" | "f" => {
            let formal = v.value.trim();
            if let Some(given) = formals.get(formal).filter(|g| !g.trim().is_empty()) {
                return Some(given.clone());
            }
            match seq.formals.iter().find(|f| f.name == formal) {
                Some(f) if !f.default.is_empty() => Some(f.default.clone()),
                Some(_) => {
                    problems.push(format!("{}: formal parameter {formal} has no value and no default", v.param));
                    None
                }
                None => {
                    problems.push(format!("{}: {formal} isn't one of {}'s formal parameters", v.param, seq.name));
                    None
                }
            }
        }
        "R" | "r" | "" => Some(match ty {
            Some(t) => values::mib_value_as_engineering(set, t, &v.value, &Interp::Raw),
            None => v.value.clone(),
        }),
        "E" | "e" => Some(v.value.clone()),
        other => {
            problems.push(format!("{}: value type {other} is taken as written", v.param));
            Some(v.value.clone())
        }
    }
}

/// `seq` as the telecommands it sends, `formals` giving its formal
/// parameters' values (by name; the defaults otherwise).
pub fn expand(set: &MibSet, seq: &Sequence, formals: &HashMap<String, String>) -> Vec<Expanded> {
    let mut out = Vec::new();
    let mut stack = vec![seq.name.clone()];
    expand_into(set, seq, formals, 0, &mut stack, &mut out);
    out
}

fn expand_into(set: &MibSet, seq: &Sequence, formals: &HashMap<String, String>, depth: usize, stack: &mut Vec<String>, out: &mut Vec<Expanded>) {
    for step in &seq.steps {
        let mut problems = Vec::new();
        let base = |kind: ExpandedKind, problems: Vec<String>| Expanded {
            sequence: seq.name.clone(),
            entry: step.entry.clone(),
            depth,
            kind,
            timing: step.timing.clone(),
            comment: step.comment.clone(),
            problems,
        };
        match &step.kind {
            StepKind::Command => {
                let Some(tc) = set.find(Kind::Telecommand, seq.root, &step.element) else {
                    problems.push(format!("{} isn't in ccf.dat", step.element));
                    out.push(base(ExpandedKind::Text(format!("{} (unknown telecommand)", step.element)), problems));
                    continue;
                };
                let args = command_args(set, tc, step, seq, formals, &mut problems);
                out.push(base(ExpandedKind::Command { tc, args }, problems));
            }
            StepKind::Sequence => {
                if stack.contains(&step.element) {
                    problems.push(format!("{} calls itself through {}", step.element, stack.join(" -> ")));
                    out.push(base(ExpandedKind::Text(format!("sequence {} (not expanded)", step.element)), problems));
                    continue;
                }
                if depth + 1 >= MAX_DEPTH {
                    problems.push(format!("sequences call sequences more than {MAX_DEPTH} deep"));
                    out.push(base(ExpandedKind::Text(format!("sequence {} (not expanded)", step.element)), problems));
                    continue;
                }
                let Some(sub) = sequence(set, seq.root, &step.element) else {
                    problems.push(format!("{} isn't in csf.dat", step.element));
                    out.push(base(ExpandedKind::Text(format!("sequence {} (unknown)", step.element)), problems));
                    continue;
                };
                let mut given = HashMap::new();
                for v in &step.values {
                    let ty = sub.formals.iter().find(|f| f.name == v.param).map(|f| &f.ty);
                    if ty.is_none() {
                        problems.push(format!("{} isn't one of {}'s formal parameters", v.param, sub.name));
                    }
                    if let Some(value) = step_value(set, v, ty, formals, seq, &mut problems) {
                        given.insert(v.param.clone(), value);
                    }
                }
                out.push(base(ExpandedKind::Text(format!("sequence {}{}", sub.name, if sub.description.is_empty() { String::new() } else { format!(": {}", sub.description) })), problems));
                stack.push(sub.name.clone());
                expand_into(set, &sub, &given, depth + 1, stack, out);
                stack.pop();
            }
            StepKind::Text => {
                let text = if step.comment.is_empty() { step.element.clone() } else { step.comment.clone() };
                out.push(base(ExpandedKind::Text(text), problems));
            }
            StepKind::Other(t) => {
                problems.push(format!("step type {t} isn't one this expands"));
                out.push(base(ExpandedKind::Text(format!("{} {}", t, step.element).trim().to_string()), problems));
            }
        }
    }
}

/// A command step's arguments: each field `Layout::fields` asks for,
/// in its order, with the step's value for that occurrence of the
/// parameter, else the parameter's default.
fn command_args(set: &MibSet, tc: DefRef, step: &Step, seq: &Sequence, formals: &HashMap<String, String>, problems: &mut Vec<String>) -> Vec<(String, String)> {
    let layout = Layout::of(set, tc);
    let tc_name = &set.get(tc).name;
    // (parameter, occurrence) -> value.
    let mut given: HashMap<(String, usize), String> = HashMap::new();
    for v in &step.values {
        let el = layout.elements.iter().find(|e| e.asked() && e.name == v.param);
        if el.is_none() {
            if layout.elements.iter().any(|e| e.name == v.param) {
                problems.push(format!("{}: {} is fixed in {tc_name}, its value isn't used", v.param, v.param));
            } else {
                problems.push(format!("{} isn't a parameter of {tc_name}", v.param));
            }
            continue;
        }
        if let Some(value) = step_value(set, v, el.and_then(|e| e.ty.as_ref()), formals, seq, problems) {
            given.insert((v.param.clone(), v.rep), value);
        }
    }
    let value_of = |field: &Field, occurrence: usize| -> String {
        let name = &layout.elements[field.element].name;
        given.get(&(name.clone(), occurrence)).cloned().unwrap_or_else(|| layout.default(set, field.element))
    };
    // Repeaters' counts, read as fields expand: the nth time a repeater
    // is met is its nth occurrence.
    let mut met: HashMap<usize, usize> = HashMap::new();
    let fields = layout.fields(set, |f| {
        let n = met.entry(f.element).or_insert(0);
        *n += 1;
        let v = value_of(f, *n);
        values::parse_number(&v).map(|c| c.max(0.0) as usize).unwrap_or(0)
    });
    let mut seen: HashMap<usize, usize> = HashMap::new();
    let mut args = Vec::with_capacity(fields.len());
    let mut used: Vec<(String, usize)> = Vec::new();
    for f in &fields {
        let n = seen.entry(f.element).or_insert(0);
        *n += 1;
        let name = layout.elements[f.element].name.clone();
        let value = value_of(f, *n);
        if value.trim().is_empty() {
            problems.push(format!("{}: no value and no default", f.label(&layout)));
        }
        used.push((name.clone(), *n));
        args.push((name, value));
    }
    let mut unused: Vec<&(String, usize)> = given.keys().filter(|k| !used.contains(k)).collect();
    unused.sort();
    for (p, rep) in unused {
        problems.push(format!("{p} occurrence {rep} isn't in {tc_name} as its counters repeat it"));
    }
    args
}

/// `expanded` written out: each telecommand through the templates
/// (`render_telecommand`'s), each line of text and problem after
/// `comment` (a script's comment marker), indented a level per call.
pub fn render(set: &MibSet, expanded: &[Expanded], template: &str, arg_template: &str, separator: &str, comment: &str) -> Vec<String> {
    let mut out = Vec::new();
    for e in expanded {
        let indent = "    ".repeat(e.depth);
        match &e.kind {
            ExpandedKind::Command { tc, args } => {
                if !e.comment.is_empty() {
                    out.push(format!("{indent}{comment} {}", e.comment));
                }
                if !e.timing.is_empty() {
                    out.push(format!("{indent}{comment} {}", e.timing));
                }
                let entry = set.get(*tc);
                out.push(format!("{indent}{}", telecommand::render_telecommand(template, arg_template, separator, &entry.row, set.root_label(entry.root), args)));
            }
            ExpandedKind::Text(text) => out.push(format!("{indent}{comment} {text}")),
        }
        for p in &e.problems {
            out.push(format!("{indent}{comment} ! {p}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::set::tests::{temp_root, write};

    fn line(fields: &[&str], n: usize) -> String {
        let mut v: Vec<&str> = fields.to_vec();
        v.resize(n.max(fields.len()), "");
        v.join("\t")
    }

    /// SQ1(LINE, MODE): a comment, TC_SET with a raw mode (hex) and the
    /// formal line, TC_GRP repeating A twice with values by REPPOS, and
    /// SQ2 called with MODE passed through; SQ2 sends TC_SET with its own
    /// formal. SQLOOP calls itself.
    fn fixture() -> MibSet {
        let r = temp_root("sequence");
        write(&r, "ccf", &[line(&["TC_SET", "Set", "", "", "N", "", "8", "1", "100", "2"], 21), line(&["TC_GRP", "Group", "", "", "N", "", "8", "2", "100", "3"], 21)].join("\n"));
        write(
            &r,
            "cdf",
            &[
                line(&["TC_SET", "E", "line", "8", "0", "0", "PLINE"], 10),
                line(&["TC_SET", "E", "mode", "8", "8", "0", "PMODE", "R", "1"], 10),
                line(&["TC_GRP", "E", "count", "8", "0", "1", "PN"], 10),
                line(&["TC_GRP", "E", "a", "8", "8", "0", "PA"], 10),
                line(&["TC_GRP", "E", "tail", "8", "16", "0", "PT", "R", "3"], 10),
            ]
            .join("\n"),
        );
        write(
            &r,
            "cpc",
            &[
                line(&["PLINE", "line", "3", "4"], 17),
                line(&["PMODE", "mode", "3", "4", "", "H", "", "", "", "", "PAF1"], 17),
                line(&["PN", "count", "3", "4"], 17),
                line(&["PA", "a", "3", "4"], 17),
                line(&["PT", "tail", "3", "4"], 17),
            ]
            .join("\n"),
        );
        write(&r, "paf", "PAF1\tmode\tU\t3\n");
        write(&r, "pas", "PAF1\tOFF\t0\nPAF1\tON\t1\nPAF1\tAUTO\tA\n");
        write(&r, "csf", &[line(&["SQ1", "Checkout"], 16), line(&["SQ2", "Inner"], 16), line(&["SQLOOP", "Loop"], 16)].join("\n"));
        write(
            &r,
            "csp",
            &[
                line(&["SQ1", "LINE", "1", "Heater line", "3", "4", "", "", "", "E", "2"], 16),
                line(&["SQ1", "MODE", "2", "Mode", "3", "4", "", "", "", "E", "ON"], 16),
                line(&["SQ2", "M2", "1", "Mode", "3", "4", "", "", "", "E", "OFF"], 16),
            ]
            .join("\n"),
        );
        write(
            &r,
            "css",
            &[
                // Out of entry order in the file.
                line(&["SQ1", "set the mode", "20", "C", "TC_SET", "2", "", "R", "00.00.01.000"], 18),
                line(&["SQ1", "Start of checkout", "10", "T", ""], 18),
                line(&["SQ1", "", "30", "C", "TC_GRP", "4"], 18),
                line(&["SQ1", "", "40", "S", "SQ2", "1"], 18),
                line(&["SQ1", "", "50", "C", "NOPE"], 18),
                line(&["SQ2", "", "10", "C", "TC_SET", "2"], 18),
                line(&["SQLOOP", "", "10", "S", "SQLOOP"], 18),
            ]
            .join("\n"),
        );
        write(
            &r,
            "sdf",
            &[
                line(&["SQ1", "20", "TC_SET", "1", "PLINE", "E", "F", "LINE"], 10),
                line(&["SQ1", "20", "TC_SET", "2", "PMODE", "E", "R", "A"], 10),
                line(&["SQ1", "30", "TC_GRP", "1", "PN", "E", "E", "2"], 10),
                line(&["SQ1", "30", "TC_GRP", "2", "PA", "E", "E", "7", "", "1"], 10),
                line(&["SQ1", "30", "TC_GRP", "2", "PA", "E", "E", "8", "", "2"], 10),
                line(&["SQ1", "30", "TC_GRP", "3", "PZ", "E", "E", "1"], 10),
                line(&["SQ1", "40", "SQ2", "1", "M2", "E", "F", "MODE"], 10),
                line(&["SQ2", "10", "TC_SET", "1", "PLINE", "E", "E", "5"], 10),
                line(&["SQ2", "10", "TC_SET", "2", "PMODE", "E", "F", "M2"], 10),
            ]
            .join("\n"),
        );
        MibSet::load(vec![r], None)
    }

    fn args(e: &Expanded) -> Vec<String> {
        match &e.kind {
            ExpandedKind::Command { args, .. } => args.iter().map(|(n, v)| format!("{n}={v}")).collect(),
            ExpandedKind::Text(t) => vec![t.clone()],
        }
    }

    #[test]
    fn a_sequence_reads_its_formals_and_steps_in_order() {
        let set = fixture();
        assert_eq!(sequences(&set).len(), 3);
        let sq = sequence(&set, 0, "SQ1").unwrap();
        assert_eq!(sq.formals.iter().map(|f| (f.name.as_str(), f.default.as_str())).collect::<Vec<_>>(), vec![("LINE", "2"), ("MODE", "ON")]);
        assert_eq!(sq.steps.iter().map(|s| s.entry.as_str()).collect::<Vec<_>>(), vec!["10", "20", "30", "40", "50"]);
        assert_eq!(sq.steps[1].timing, "release 00.00.01.000 after the previous step");
        assert!(sequence(&set, 0, "NOPE").is_none());
    }

    #[test]
    fn steps_expand_with_raw_formal_and_repeated_values() {
        let set = fixture();
        let sq = sequence(&set, 0, "SQ1").unwrap();
        let mut f = HashMap::new();
        f.insert("LINE".to_string(), "4".to_string());
        let e = expand(&set, &sq, &f);
        assert_eq!(args(&e[0]), vec!["Start of checkout"]);
        // Raw "A" in PMODE's hex radix is the status text AUTO.
        assert_eq!(args(&e[1]), vec!["PLINE=4", "PMODE=AUTO"]);
        assert!(e[1].problems.is_empty(), "{:?}", e[1].problems);
        assert_eq!(args(&e[2]), vec!["PN=2", "PA=7", "PA=8", "PT=3"]);
        assert!(e[2].problems.iter().any(|p| p.contains("PZ isn't a parameter of TC_GRP")), "{:?}", e[2].problems);
        // SQ2, MODE passed through with its default ON.
        assert_eq!(args(&e[3]), vec!["sequence SQ2: Inner"]);
        assert_eq!((e[4].sequence.as_str(), e[4].depth), ("SQ2", 1));
        assert_eq!(args(&e[4]), vec!["PLINE=5", "PMODE=ON"]);
        assert!(e[5].problems.iter().any(|p| p.contains("NOPE isn't in ccf.dat")));
    }

    #[test]
    fn a_sequence_calling_itself_stops() {
        let set = fixture();
        let sq = sequence(&set, 0, "SQLOOP").unwrap();
        let e = expand(&set, &sq, &HashMap::new());
        assert_eq!(e.len(), 1);
        assert!(e[0].problems[0].contains("calls itself"), "{:?}", e[0].problems);
    }

    #[test]
    fn expanded_steps_render_through_the_templates() {
        let set = fixture();
        let sq = sequence(&set, 0, "SQ1").unwrap();
        let e = expand(&set, &sq, &HashMap::new());
        let lines = render(&set, &e[..5], "send {mnemo} {arguments}", "{name}={value}", " ", "#");
        assert_eq!(
            lines,
            vec![
                "# Start of checkout",
                "# set the mode",
                "# release 00.00.01.000 after the previous step",
                "send TC_SET PLINE=2 PMODE=AUTO",
                "send TC_GRP PN=2 PA=7 PA=8 PT=3",
                "# ! PZ isn't a parameter of TC_GRP",
                "# sequence SQ2: Inner",
                "    send TC_SET PLINE=5 PMODE=ON",
            ]
        );
    }
}
