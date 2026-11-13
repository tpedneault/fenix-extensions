//! The driver held to Fenix's rulings on what the packs' kit asked (Fenix
//! `docs/plans/test-explorer.md` §19, "What the packs' kit asked", K1 to
//! K14, and findings 2, 3, 8, 9) and to `fenix-tests`' Decisions, on a
//! raw provider: an `Exports` that answers what a test gives it, with no
//! lang-kit between, so what's checked is Fenix's side alone.

#![cfg(feature = "tests")]

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::Command;
use fenix_lang_kit::tests::{api, cut_id};
use fenix_test_kit::tests::{Exports, Fenix, Recorded, State, NO_RESULT};

use api::{Ask, Ended, Found, Kind, Line, Outcome, Plan, Report, Step, StepEnd};

/// A provider that answers as told: its scan, its plan, and each step's
/// reports at the step's end. What it was asked is kept.
#[derive(Default, Clone)]
struct Raw {
    scan: Vec<Found>,
    plan: Option<Plan>,
    reports: Vec<Vec<Report>>,
    asks: Rc<RefCell<Vec<Ask>>>,
    launches: Rc<RefCell<Vec<(u64, Found)>>>,
}

impl Exports for Raw {
    fn tests_scan(&mut self, _provider: &str, _path: &str, _text: &str) -> Vec<Found> {
        self.scan.clone()
    }
    fn tests_plan(&mut self, _provider: &str, _run: u64, ask: Ask) -> Result<Plan, String> {
        self.asks.borrow_mut().push(ask);
        self.plan.clone().ok_or_else(|| "no plan".to_string())
    }
    fn tests_read(&mut self, _provider: &str, _run: u64, _step: u32, _lines: Vec<Line>) -> Vec<Report> {
        Vec::new()
    }
    fn tests_step_ended(&mut self, _provider: &str, _run: u64, step: u32, _end: StepEnd) -> Vec<Report> {
        self.reports.get(step as usize).cloned().unwrap_or_default()
    }
    fn tests_launch(&mut self, _provider: &str, run: u64, test: Found) -> Result<api::Launch, String> {
        self.launches.borrow_mut().push((run, test));
        Ok(api::Launch::BuildFirst(Plan { steps: vec![step(&[])], list_after: false }))
    }
}

fn node(id: &str, parent: Option<&str>, kind: Kind, line: u32) -> Found {
    Found { id: id.into(), parent: parent.map(String::from), label: id.rsplit("::").next().unwrap_or(id).into(), kind, line: Some(line), ..Found::default() }
}

fn step(covers: &[&str]) -> Step {
    Step { command: Command::new("raw"), label: "raw".into(), covers: covers.iter().map(|c| c.to_string()).collect(), needs: Vec::new() }
}

fn ended(id: &str, outcome: Outcome) -> Report {
    Report::Ended(Ended { id: id.into(), outcome, duration_ms: None, message: None, at: None, text: None })
}

const FILE: &str = "tests/test_a.py";
const SUITE: &str = "tests/test_a.py::TestMath";
const ADDS: &str = "tests/test_a.py::TestMath::test_adds";
const SUBS: &str = "tests/test_a.py::TestMath::test_subs";
const TOP: &str = "tests/test_a.py::test_top";

fn tree() -> Vec<Found> {
    vec![node(FILE, None, Kind::File, 0), node(SUITE, Some(FILE), Kind::Suite, 2), node(ADDS, Some(SUITE), Kind::Test, 3), node(SUBS, Some(SUITE), Kind::Test, 6), node(TOP, Some(FILE), Kind::Test, 30)]
}

fn fenix(raw: Raw) -> Fenix<Raw> {
    let mut fenix = Fenix::new(raw);
    fenix.programs(&["raw"]);
    fenix.on_step_with(|_| Some(Recorded::stdout("")));
    fenix
}

fn with_tree() -> Fenix<Raw> {
    let mut fenix = fenix(Raw { scan: tree(), ..Raw::default() });
    fenix.scan("py", &format!("/project/{FILE}"), "");
    fenix
}

/// §19 finding 3, Decisions 20: an ask holds the topmost selected, and
/// after a file or suite its fill-in, suites between included; a record
/// whose `parent` is in the ask is a fill-in, one whose isn't was
/// selected.
#[test]
fn an_ask_is_told_apart_by_parent_as_fenix_builds_it() {
    let fenix = with_tree();
    let Ask::Run(records) = fenix.ask_for("py", &[FILE, ADDS]) else { panic!() };
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [FILE, SUITE, ADDS, SUBS, TOP], "the file is topmost; its fill-in in the page's order");
    let asked: Vec<&str> = ids.clone();
    let selected: Vec<&str> = records.iter().filter(|r| !r.parent.as_deref().is_some_and(|p| asked.contains(&p))).map(|r| r.id.as_str()).collect();
    assert_eq!(selected, [FILE]);
    assert_eq!(records[2].path.as_deref(), Some("/project/tests/test_a.py"), "given back as /project/<path>");
    let Ask::Run(records) = fenix.ask_for("py", &[TOP, SUITE]) else { panic!() };
    assert_eq!(records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), [SUITE, ADDS, SUBS, TOP], "a suite's fill-in, then a test beside it");
}

/// §19 K12, Decisions 20: past 10,000 records a fill-in is left out
/// whole -- a later, smaller one may still fit -- and a selected test is
/// still told apart.
#[test]
fn past_10000_records_a_fill_in_is_left_out_whole() {
    let big = "tests/test_big.py";
    let mut scan = vec![node(big, None, Kind::File, 0)];
    scan.extend((0..9_999).map(|i| node(&format!("{big}::t{i}"), Some(big), Kind::Test, i + 1)));
    let mut fenix = fenix(Raw { scan, ..Raw::default() });
    fenix.scan("py", &format!("/project/{big}"), "");
    let mut raw = Raw { scan: tree(), ..Raw::default() };
    std::mem::swap(&mut fenix.ext, &mut raw);
    fenix.scan("py", &format!("/project/{FILE}"), "");
    let Ask::Run(records) = fenix.ask_for("py", &[big, SUITE, TOP]) else { panic!() };
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [SUITE, ADDS, SUBS, TOP, big], "in the page's order; the big file's fill-in doesn't fit, the suite's does");
    assert_eq!(records[3].parent.as_deref(), Some(FILE), "not in the ask: selected");
}

/// §19 findings 2 and 9, Decisions 32 and 33: a cut id is asked for
/// itself only with its `data`; without (Fenix started again), as its
/// nearest ancestor, and a fill-in holding it is left out.
#[test]
fn a_cut_id_without_its_data_is_asked_as_its_parent() {
    let long = format!("{FILE}::test_cases[{}]", "x".repeat(2_000));
    let mut case = node(&long, Some(FILE), Kind::Case, 9);
    case.data = Some("its own name".into());
    let mut scan = tree();
    scan.push(case);
    let mut fenix = fenix(Raw { scan, ..Raw::default() });
    fenix.scan("py", &format!("/project/{FILE}"), "");
    let cut = cut_id(&long).into_owned();
    assert!(fenix.said().iter().any(|s| s.contains("an id over 1 KiB was cut")), "said, not refused");
    let Ask::Run(records) = fenix.ask_for("py", &[&cut]) else { panic!() };
    assert_eq!((records.len(), records[0].id.as_str(), records[0].data.as_deref()), (1, cut.as_str(), Some("its own name")));
    fenix.forget_data("py");
    let Ask::Run(records) = fenix.ask_for("py", &[&cut]) else { panic!() };
    assert_eq!(records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), [FILE], "its file, without the fill-in that holds it");
    assert!(records[0].data.is_none());
    assert!(fenix.launch("py", &cut).config.is_err(), "not debugged");
}

/// §19 K5, K6, Decisions 11: a `crashed` or `broken`, with an id or
/// none, errors only what has no result in the run yet; with no id in a
/// step that claims nothing, its scope is the plan's rest.
#[test]
fn a_crash_or_broken_leaves_what_already_ended_in_the_run() {
    let raw = Raw {
        scan: tree(),
        plan: Some(Plan { steps: vec![step(&[ADDS]), step(&[])], list_after: false }),
        reports: vec![vec![ended(ADDS, Outcome::Passed)], vec![ended(SUBS, Outcome::Failed), Report::Broken { id: Some(SUITE.into()), message: "no".into(), at: None }, Report::Crashed { id: None, message: "SIGSEGV".into() }]],
        ..Raw::default()
    };
    let mut fenix = fenix(raw);
    fenix.scan("py", &format!("/project/{FILE}"), "");
    let run = fenix.run_all("py");
    assert!(!run.complete);
    fenix.assert_states("py", &[(ADDS, State::Passed), (SUBS, State::Failed), (TOP, State::Errored)]);
    assert_eq!(fenix.detail("py", TOP).message.as_deref(), Some("SIGSEGV"), "the plan's rest: what no step names");
}

/// §19 K8, Decisions 13: a plan of no steps has its ask errored at once;
/// a listing of none isn't complete.
#[test]
fn a_plan_of_no_steps_errors_its_ask_at_once() {
    let mut fenix = with_tree();
    fenix.ext.plan = Some(Plan::default());
    let run = fenix.run("py", &[SUITE]);
    assert!(!run.complete && run.steps.is_empty());
    fenix.assert_states("py", &[(ADDS, State::Errored), (SUBS, State::Errored), (TOP, State::Unknown)]);
    assert_eq!(fenix.detail("py", ADDS).message.as_deref(), Some(NO_RESULT));
    assert!(!fenix.discover("py").complete);

    // A plan that's an error errors what it was asked with it (Decisions 12).
    fenix.ext.plan = None;
    let run = fenix.run("py", &[TOP]);
    assert_eq!(run.error.as_deref(), Some("no plan"));
    assert_eq!(fenix.detail("py", TOP).message.as_deref(), Some("no plan"));
}

/// §19 K9, Decisions 17: a `found` in a build-first plan is ignored, and
/// the build runs as the launch's run, which the second ask carries
/// (§19 finding 1, Decisions 34).
#[test]
fn a_build_ignores_found_and_runs_as_the_launchs_run() {
    let mut fenix = with_tree();
    fenix.ext.reports = vec![vec![Report::Found(node("tests/test_a.py::made", None, Kind::Test, 1))]];
    let launched = fenix.launch("py", ADDS);
    assert!(launched.built.is_some_and(|b| b.complete));
    assert!(fenix.record("py", "tests/test_a.py::made").is_none(), "ignored");
    let launches = fenix.ext.launches.borrow();
    assert_eq!((launches.len(), launches[0].0), (2, launches[1].0), "asked twice, with one run");
    assert_eq!(launches[0].1.parent.as_deref(), Some(SUITE));
}

/// §19 K10, Decisions 3 and 6: the same id twice in a scan, the later
/// wins; a `found` before its parent is at the top until the parent
/// comes, then under it.
#[test]
fn a_found_before_its_parent_waits_at_the_top() {
    let mut fenix = fenix(Raw { scan: vec![node(ADDS, Some(SUITE), Kind::Test, 3), node(ADDS, Some(SUITE), Kind::Test, 4)], ..Raw::default() });
    fenix.scan("py", &format!("/project/{FILE}"), "");
    assert_eq!(fenix.record("py", ADDS).unwrap().line, Some(4), "the later");
    assert_eq!(fenix.given_back("py", ADDS).unwrap().parent, None, "at the top: its parent isn't found");
    assert!(fenix.said().is_empty(), "nothing to say: Fenix takes both");
    fenix.ext.scan = tree();
    fenix.scan("py", &format!("/project/{FILE}"), "");
    assert_eq!(fenix.given_back("py", ADDS).unwrap().parent.as_deref(), Some(SUITE));
}

/// §19 K11, Decisions 8: a group with a failed and an errored test is
/// failed; and a template whose cases have results is theirs, not
/// errored, needing no result of its own (§19 K7, Decisions 7).
#[test]
fn a_groups_state_and_a_templates_come_from_what_is_counted_under_them() {
    let table = "tests/test_a.py::test_table";
    let mut scan = tree();
    scan.push(Found { template: true, ..node(table, Some(FILE), Kind::Test, 40) });
    let raw = Raw {
        scan,
        plan: Some(Plan { steps: vec![step(&[FILE])], list_after: false }),
        reports: vec![vec![
            ended(ADDS, Outcome::Failed),
            Report::Crashed { id: Some(SUITE.into()), message: "died".into() },
            ended(TOP, Outcome::Passed),
            Report::Found(node(&format!("{table}[1]"), Some(table), Kind::Case, 40)),
            ended(&format!("{table}[1]"), Outcome::Passed),
        ]],
        ..Raw::default()
    };
    let mut fenix = fenix(raw);
    fenix.scan("py", &format!("/project/{FILE}"), "");
    fenix.run("py", &[FILE]);
    fenix.assert_states("py", &[(ADDS, State::Failed), (SUBS, State::Errored), (SUITE, State::Failed), (table, State::Passed), (FILE, State::Failed)]);
    assert!(fenix.detail("py", table).message.is_none(), "not errored for want of its own `ended`");
}

/// §19 K14, Decisions 31 and 29: a case a complete run of its template
/// dropped goes with its result; a test that leaves its file's scan and
/// comes back keeps its result.
#[test]
fn a_dropped_case_goes_with_its_result_and_a_test_that_comes_back_keeps_its() {
    let table = "tests/test_a.py::test_table";
    let mut scan = tree();
    scan.push(Found { template: true, ..node(table, Some(FILE), Kind::Test, 40) });
    let case = |n: u32| Report::Found(node(&format!("{table}[{n}]"), Some(table), Kind::Case, 40));
    let raw = Raw { scan: scan.clone(), plan: Some(Plan { steps: vec![step(&[table])], list_after: false }), reports: vec![vec![case(1), case(2), ended(&format!("{table}[1]"), Outcome::Passed), ended(&format!("{table}[2]"), Outcome::Failed)]], ..Raw::default() };
    let mut fenix = fenix(raw);
    fenix.scan("py", &format!("/project/{FILE}"), "");
    fenix.run("py", &[table]);
    assert_eq!(fenix.state("py", table), State::Failed);
    fenix.ext.reports = vec![vec![case(1), ended(&format!("{table}[1]"), Outcome::Passed)]];
    assert!(fenix.run("py", &[table]).complete);
    assert!(fenix.record("py", &format!("{table}[2]")).is_none());
    assert!(fenix.model("py").restored.is_empty(), "its result went with it");
    assert_eq!(fenix.state("py", table), State::Passed);

    // An empty scan drops the file's tests (§19 finding 8); back, ADDS has
    // its result again.
    fenix.ext.reports = vec![vec![ended(ADDS, Outcome::Failed)]];
    fenix.ext.plan = Some(Plan { steps: vec![step(&[ADDS])], list_after: false });
    fenix.run("py", &[ADDS]);
    fenix.ext.scan = Vec::new();
    fenix.scan("py", &format!("/project/{FILE}"), "");
    assert!(fenix.record("py", ADDS).is_none());
    fenix.ext.scan = scan;
    fenix.scan("py", &format!("/project/{FILE}"), "");
    assert_eq!(fenix.state("py", ADDS), State::Failed);
}

/// Fenix §6.5, §19 K2: a step's command line is counted after `${data}`
/// is filled in, with its program's full path; one over fails its step
/// and the plan goes on (Decisions 13).
#[test]
fn a_command_line_is_counted_as_fenix_counts_it() {
    let mut fenix = with_tree();
    fenix.lenient();
    // 32,700 characters as written; over 32,766 once `${data}` is filled
    // in 100 times.
    let args: Vec<String> = (0..100).map(|_| format!("${{data}}/{}", "x".repeat(316))).collect();
    let long = Step { command: Command::new("raw").args(args), ..step(&[ADDS]) };
    assert!(fenix_lang_kit::tests::command_line_len(&long.command) < 32_766);
    fenix.ext.plan = Some(Plan { steps: vec![long, step(&[SUBS])], list_after: false });
    fenix.ext.reports = vec![Vec::new(), vec![ended(SUBS, Outcome::Passed)]];
    let run = fenix.run("py", &[SUITE]);
    assert!(matches!(&run.steps[0].end, StepEnd::Failed(w) if w.starts_with("its command line is 3") && w.ends_with("Windows takes 32,766")), "{:?}", run.steps[0].end);
    fenix.assert_states("py", &[(ADDS, State::Errored), (SUBS, State::Passed)]);

    // A `.cmd`: the line Rust gives `cmd.exe`, and its 8,191.
    fenix.batch_file("raw");
    let args: Vec<String> = (0..50).map(|i| format!("a%{i}%")).collect();
    let step_ = Step { command: Command::new("raw").args(args), ..step(&[ADDS]) };
    let line = fenix_lang_kit::tests::batch_file_line_len(&step_.command).unwrap();
    assert!(line > fenix_lang_kit::tests::command_line_len(&step_.command) + 50 * 14, "each % as %%cd:~,% and quoted");
}

/// Decisions 27: a file's tests are its topmost nodes but groups; and a
/// provider that said it has nothing to run isn't asked (Fenix §6.3).
#[test]
fn a_files_tests_and_a_provider_with_nothing_to_run() {
    let mut fenix = with_tree();
    fenix.ext.plan = Some(Plan { steps: vec![step(&[])], list_after: false });
    fenix.ext.reports = vec![vec![ended(ADDS, Outcome::Passed), ended(SUBS, Outcome::Passed), ended(TOP, Outcome::Passed)]];
    let run = fenix.run_file("py", &format!("/project/{FILE}"));
    assert!(run.complete);
    let asks = fenix.ext.asks.borrow();
    let Ask::Run(records) = asks.last().unwrap() else { panic!() };
    assert_eq!(records[0].id, FILE, "the file, topmost, with its fill-in");
    drop(asks);
    fenix.assert_states("py", &[(FILE, State::Passed)]);
}
