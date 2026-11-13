//! The provider through the test kit's driver, `fenix_test_kit::tests::
//! Fenix`, which plays Fenix's side as strictly as Fenix, on what pytest
//! 9.2.0 (and pytest-xdist 3.8.0) printed through the plugin for the
//! sample projects in `src/samples/tests/pytest` (recorded as
//! `plans/test-providers-recipe.md` says). Each `Recorded` is a command's
//! stdout and stderr as Fenix keeps them apart, how they came, and its
//! exit; the driver gives them to the provider in batches, cut, shuffled,
//! and times every call.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_lang_kit::tests::{api, Ask, Asked, Group, Kind, StepEnd, TestRunner, DIRECT};
use fenix_lang_kit::Kit;
use fenix_test_kit::tests::{command_text, Fenix, Order, Recorded, State, NO_RESULT};
use fenix_test_kit::{assert_snapshot, Fake};

use super::{Facts, Provider, SharedFacts, PROVIDER};

const PYTEST: &str = "src/samples/tests/pytest";

const BASIC: &str = include_str!("../samples/tests/pytest/orbit/tests/test_basic.py");
const EXTRA: &str = include_str!("../samples/tests/pytest/orbit/tests/test_extra.py");
const UNIT: &str = include_str!("../samples/tests/pytest/orbit/tests/sub/test_unit.py");
const CRASH: &str = include_str!("../samples/tests/pytest/orbit-crash/tests/test_crash.py");
const ABORT: &str = include_str!("../samples/tests/pytest/orbit-crash/tests/test_zabort.py");
const FINE: &str = include_str!("../samples/tests/pytest/orbit-broken/tests/test_fine.py");
const SYNTAX: &str = include_str!("../samples/tests/pytest/orbit-broken/tests/test_syntax.py");

const ADDS: &str = "tests/test_basic.py::test_adds";
const FAILS: &str = "tests/test_basic.py::test_fails_on_purpose";
const SKIPPED: &str = "tests/test_basic.py::test_skipped";
const KNOWN_BUG: &str = "tests/test_basic.py::test_known_bug";
const CASES: &str = "tests/test_basic.py::test_cases";
const SMALL: &str = "tests/test_basic.py::test_cases[small]";
const NEGATIVE: &str = "tests/test_basic.py::test_cases[negative]";
const ODD: &str = "tests/test_basic.py::test_cases[strings [odd] id]";
const IN_CLASS: &str = "tests/test_basic.py::TestOuter::test_in_class";
const DEEP: &str = "tests/test_basic.py::TestOuter::TestInner::test_deep";
const PARAM_1: &str = "tests/test_basic.py::TestOuter::test_param_in_class[1]";
const BROKEN_FIXTURE: &str = "tests/test_basic.py::test_uses_broken_fixture";
const TEARDOWN: &str = "tests/test_extra.py::test_teardown_fails";
const STRICT: &str = "tests/test_extra.py::test_strict_xpass";
const XPASS: &str = "tests/test_extra.py::test_xpass";
const JAPANESE: &str = "tests/test_extra.py::test_名前";
const SKIPIF: &str = "tests/test_extra.py::test_skipif";
const SKIPS_INSIDE: &str = "tests/test_extra.py::test_skips_inside";
const STACKED: &str = "tests/test_extra.py::test_stacked[3-1]";
const MARKED_1: &str = "tests/test_extra.py::test_marked_case[1]";
const FIXTURE_10: &str = "tests/test_extra.py::test_fixture_param[10]";
const UNIT_BAD: &str = "tests/sub/test_unit.py::TestUnit::test_bad";
const UNIT_OK: &str = "tests/sub/test_unit.py::TestUnit::test_ok";
/// The file `tests/test_basic.py` and its classes, as groups' ids.
const BASIC_FILE: &str = "\u{1f}tests/test_basic.py";
const OUTER: &str = "\u{1f}tests/test_basic.py\u{1f}TestOuter";

fn facts() -> SharedFacts {
    Rc::new(RefCell::new(Facts { python: Some("python3".into()), pytest: Some("9.2.0".into()), root: format!("{PYTEST}/orbit"), generation: 1, ..Facts::default() }))
}

/// Fenix with the pack's provider, the sample's files scanned.
fn fenix_with(facts: SharedFacts, files: &[(&str, &str)]) -> (Fenix<Kit>, Fake) {
    let fake = Fake::new();
    let kit = Kit::with_host("python", fake.host()).tests(Box::new(Provider::new(facts)));
    let mut fenix = Fenix::new(kit);
    fenix.manifest("extension.toml").with_fake(fake.clone());
    for (path, text) in files {
        fenix.scan(PROVIDER, &format!("/project/{path}"), text);
    }
    (fenix, fake)
}

const ORBIT: &[(&str, &str)] = &[("tests/test_basic.py", BASIC), ("tests/test_extra.py", EXTRA), ("tests/sub/test_unit.py", UNIT)];

fn orbit() -> (Fenix<Kit>, Fake) {
    fenix_with(facts(), ORBIT)
}

fn recorded(name: &str) -> Recorded {
    Recorded::from(format!("{PYTEST}/{name}"))
}

/// What a run of everything in the sample gives, pytest's way.
const RUN_ALL: &[(&str, State)] = &[
    (ADDS, State::Passed),
    (FAILS, State::Failed),
    (SKIPPED, State::Skipped),
    (KNOWN_BUG, State::Skipped),
    (SMALL, State::Passed),
    (NEGATIVE, State::Failed),
    (ODD, State::Passed),
    (IN_CLASS, State::Passed),
    (DEEP, State::Failed),
    (PARAM_1, State::Passed),
    (BROKEN_FIXTURE, State::Errored),
    (TEARDOWN, State::Errored),
    (STRICT, State::Failed),
    (XPASS, State::Passed),
    (JAPANESE, State::Passed),
    (SKIPIF, State::Skipped),
    (SKIPS_INSIDE, State::Skipped),
    (STACKED, State::Passed),
    (MARKED_1, State::Skipped),
    (FIXTURE_10, State::Passed),
    (UNIT_BAD, State::Failed),
    (UNIT_OK, State::Passed),
];

/// §6.2's done when 1: the scan, at the tests' lines, under their file
/// and classes; `@pytest.mark.skip(reason="slow")` skipped "slow", an
/// `xfail` isn't; a `parametrize` is a template, its cases not there yet.
#[test]
fn a_scan_gives_each_test_at_its_line_under_its_file_and_classes() {
    let (fenix, fake) = orbit();
    let adds = fenix.record(PROVIDER, ADDS).unwrap();
    assert_eq!((adds.line, adds.end_line, adds.parent.as_deref(), adds.path.as_deref()), (Some(5), Some(6), Some(BASIC_FILE), Some("tests/test_basic.py")), "lines from 0 in Fenix: line 6");
    assert_eq!(fenix.record(PROVIDER, SKIPPED).unwrap().skip.as_deref(), Some("slow"));
    assert_eq!(fenix.record(PROVIDER, KNOWN_BUG).unwrap().skip, None, "an xfail runs");
    assert_eq!(fenix.record(PROVIDER, SKIPIF).unwrap().skip, None, "a condition only the run knows");
    assert!(fenix.record(PROVIDER, CASES).unwrap().template);
    assert!(fenix.record(PROVIDER, SMALL).is_none(), "a case comes from pytest");
    assert_eq!(fenix.record(PROVIDER, JAPANESE).unwrap().label, "test_名前");
    let outer = fenix.record(PROVIDER, OUTER).unwrap();
    assert_eq!((outer.kind, outer.line, outer.end_line), (Kind::Suite, Some(29), Some(39)));
    assert_eq!(fenix.record(PROVIDER, DEEP).unwrap().parent.as_deref(), Some("\u{1f}tests/test_basic.py\u{1f}TestOuter\u{1f}TestInner"));
    assert_eq!(fenix.record(PROVIDER, BASIC_FILE).unwrap().kind, Kind::File);
    assert_eq!(fenix.told(PROVIDER), Some(("pytest 9.2.0".into(), None)));
    assert!(fake.take().iter().any(|c| c == "test-provider python: pytest 9.2.0"));
    assert_snapshot!("pytest-scan", fenix.tree(PROVIDER));
}

/// A file that isn't a test file, text being typed, and every kind of odd
/// bytes: no panic, and the tests that were there stay.
#[test]
fn what_is_not_a_test_file_and_text_being_typed_are_read_without_failing() {
    let (mut fenix, _) = orbit();
    assert!(fenix.scan(PROVIDER, "/project/orbit.py", "def test_not_a_test_file():\n    pass\n").is_empty(), "pytest collects test_*.py and *_test.py");
    assert!(fenix.scan(PROVIDER, "/project/tests/conftest.py", "def test_x(): pass").is_empty());
    for typing in ["def test_new(", "def test_new():\n    \"\"\"docs", "@pytest.mark.parametrize(\"a\", [1,"] {
        let text = format!("{BASIC}\n\n{typing}");
        let found = fenix.scan(PROVIDER, "/project/tests/test_basic.py", &text);
        assert!(found.iter().any(|f| f.id == ADDS) && found.iter().any(|f| f.id == DEEP), "{typing:?}");
    }
    for odd in ["", "\u{0}\u{1}", "def", "class Test", "\"\"\"", "def test_é(: pass"] {
        fenix.scan(PROVIDER, "/project/tests/test_odd.py", odd);
    }
    // Back as it was: the tests are.
    fenix.scan(PROVIDER, "/project/tests/test_basic.py", BASIC);
    assert!(fenix.record(PROVIDER, ADDS).is_some());
}

/// A file of 2,000 tests is scanned within the driver's budget for a call
/// (5 ms in a release build, 50 ms in a debug one): the provider's scan
/// and lang-kit's adapter together, natively. The best of five, so a busy
/// machine doesn't fail it; every other call the driver times as it goes.
#[test]
fn a_file_of_two_thousand_tests_is_scanned_within_a_calls_budget() {
    let text: String = (0..2000).map(|i| format!("def test_fails_on_purpose_number_{i:05}():\n    assert 1 + 1 == 2, \"{{}}\"\n\n\n")).collect();
    let budget = if cfg!(debug_assertions) { std::time::Duration::from_millis(50) } else { std::time::Duration::from_millis(5) };
    let best = (0..5)
        .map(|_| {
            let mut kit = Kit::with_host("python", Fake::new().host()).tests(Box::new(Provider::new(facts())));
            let start = std::time::Instant::now();
            let found = kit.tests_scan(PROVIDER, "/project/tests/test_many.py", &text);
            assert_eq!(found.len(), 2001, "the file and 2,000 tests");
            start.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < budget, "{best:?}");
}

/// §6.2's done when 2: the listing adds the cases under their templates,
/// and a fixture's params' (which the scan took for plain tests); the
/// scan's lines stay.
#[test]
fn a_listing_adds_the_cases_under_their_templates_and_keeps_the_scans_lines() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("collect"));
    let run = fenix.discover(PROVIDER);
    assert!(run.complete, "{run:?}");
    assert_eq!(command_text(&run.steps[0].command), format!("python3 -c {} ${{data}}/lang-kit/tests -q -p fenix_tests --rootdir=. --collect-only", super::plan::BOOT));
    assert_eq!((run.steps[0].command.cwd.as_deref(), run.steps[0].needs.as_slice()), (Some("/project"), ["/data/lang-kit/tests/fenix_tests.py".to_string()].as_slice()));
    assert!(!run.lists_after);
    let negative = fenix.record(PROVIDER, NEGATIVE).unwrap();
    assert_eq!((negative.kind, negative.parent.as_deref(), negative.label.as_str()), (Kind::Case, Some(CASES), "negative"));
    assert_eq!(fenix.record(PROVIDER, ODD).unwrap().label, "strings [odd] id", "brackets and spaces in a case's id");
    assert_eq!(fenix.record(PROVIDER, PARAM_1).unwrap().parent.as_deref(), Some("tests/test_basic.py::TestOuter::test_param_in_class"));
    // A fixture's params: the scan saw a test; the listing says it has cases.
    let fixture = fenix.record(PROVIDER, "tests/test_extra.py::test_fixture_param").unwrap();
    assert!(fixture.template, "found again as one");
    assert_eq!(fenix.record(PROVIDER, FIXTURE_10).unwrap().parent.as_deref(), Some("tests/test_extra.py::test_fixture_param"));
    assert_eq!(fenix.record(PROVIDER, ADDS).unwrap().line, Some(5), "the scan's line stays");
    assert_eq!(fenix.record(PROVIDER, CASES).unwrap().line, Some(25));
    assert_snapshot!("pytest-listed", fenix.tree(PROVIDER));
}

/// A key over 1 KiB: its case is cut by Fenix, and its full id is in `data`,
/// which comes back with an ask; run again after Fenix started again (no
/// `data`), its template runs.
#[test]
fn a_case_whose_id_is_over_a_kib_runs_by_its_data_or_else_by_its_template() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("collect"));
    fenix.discover(PROVIDER);
    let long = fenix.ids(PROVIDER).into_iter().find(|id| id.starts_with("tests/test_extra.py::test_long_id[y") && id.len() > 1000).expect("cut to 1 KiB");
    assert_eq!(long.len(), 1024);
    let node = fenix.given_back(PROVIDER, &long).unwrap();
    let full = node.data.clone().expect("its full id");
    assert!(full.len() > 1024 && full.starts_with("tests/test_extra.py::test_long_id[yyyy"));
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run(PROVIDER, &[long.as_str()]);
    assert_eq!(run.steps[0].command.args.last().map(String::as_str), Some(full.as_str()));
    // After a restart: Fenix asks for what it can name, the template.
    fenix.forget_data(PROVIDER);
    let run = fenix.run(PROVIDER, &[long.as_str()]);
    assert_eq!(run.steps[0].command.args.last().map(String::as_str), Some("tests/test_extra.py::test_long_id"));
}

/// §6.2's done when 3: a run of all, every outcome as pytest said it; a
/// failure at its assertion with its message, its text and what it
/// printed; a fixture's failure errored, not failed; a skip's reason; an
/// xfail skipped; a strict xpass failed.
#[test]
fn a_run_of_all_gives_each_test_its_result_at_its_assertion() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), format!("python3 -c {} ${{data}}/lang-kit/tests -q -p fenix_tests --rootdir=.", super::plan::BOOT));
    assert!(run.complete && run.crashed.is_empty() && run.broken.is_empty(), "{run:?}");
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("tests/test_basic.py".into(), 12, None)));
    let d = fenix.detail(PROVIDER, FAILS);
    assert_eq!(d.message.as_deref(), Some("AssertionError: two and two"));
    assert!(d.text.as_deref().is_some_and(|t| t.contains("assert add(2, 2) == 5") && t.contains("where 4 = add(2, 2)")), "{:?}", d.text);
    assert_eq!(fenix.model(PROVIDER).nodes[FAILS].printed.trim_end(), "some output before the failure");
    assert!(d.duration_ms.is_some_and(|ms| ms > 0.0));
    assert_eq!(fenix.at(PROVIDER, NEGATIVE), Some(("tests/test_basic.py".into(), 27, None)), "a case fails at its assertion");
    assert_eq!(fenix.record(PROVIDER, NEGATIVE).unwrap().parent.as_deref(), Some(CASES), "found in the run, under its template");
    // A test whose traceback is in unittest's: at the deepest frame in the test's own file.
    assert_eq!(fenix.at(PROVIDER, UNIT_BAD), Some(("tests/sub/test_unit.py".into(), 9, None)));
    assert_eq!(fenix.detail(PROVIDER, UNIT_BAD).message.as_deref(), Some("AssertionError: 1 != 2"));
    assert_eq!(fenix.detail(PROVIDER, SKIPPED).message.as_deref(), Some("slow"));
    assert_eq!(fenix.detail(PROVIDER, SKIPS_INSIDE).message.as_deref(), Some("not today"));
    assert_eq!(fenix.detail(PROVIDER, KNOWN_BUG).message.as_deref(), Some("xfail: known bug"));
    assert_eq!(fenix.detail(PROVIDER, SKIPIF).message.as_deref(), Some("not on this platform"));
    assert_eq!(fenix.detail(PROVIDER, STRICT).message.as_deref(), Some("[XPASS(strict)] fixed already"));
    assert_eq!(fenix.detail(PROVIDER, XPASS).message.as_deref(), Some("xfail: flaky, but it passed"));
    // A fixture that raises: errored in its setup; a teardown that raises after a pass: errored in its teardown.
    assert_eq!(fenix.detail(PROVIDER, BROKEN_FIXTURE).message.as_deref(), Some("setup: RuntimeError: fixture broke"));
    assert_eq!(fenix.at(PROVIDER, BROKEN_FIXTURE), Some(("tests/test_basic.py".into(), 45, None)), "the fixture's raise, in the test's file");
    assert_eq!(fenix.detail(PROVIDER, TEARDOWN).message.as_deref(), Some("teardown: RuntimeError: teardown broke"));
    assert_eq!(fenix.at(PROVIDER, TEARDOWN), Some(("tests/test_extra.py".into(), 9, None)), "the raise in the fixture");
    assert_eq!(fenix.detail(PROVIDER, CASES), Default::default(), "no result made for a template");
    assert_snapshot!("pytest-run-all", fenix.tree(PROVIDER));
}

/// The same recording in every stream order and every batch, lines cut
/// small, and with `-qq` (which takes pytest's last line away): one tree.
#[test]
fn the_output_in_any_order_and_any_batches_ends_the_same() {
    let (mut whole, _) = orbit();
    whole.on_recorded(recorded("run-all"));
    whole.run_all(PROVIDER);
    let tree = whole.tree(PROVIDER);
    let orders = [Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(1), Order::Shuffled(2), Order::Shuffled(3), Order::Shuffled(42)];
    for (i, order) in orders.into_iter().enumerate() {
        let (mut f, _) = orbit();
        f.order = order.clone();
        f.batch = 1 + i * 3;
        f.cut_at = if i % 2 == 0 { 64 * 1024 } else { 23 };
        f.on_recorded(recorded("run-all"));
        f.run_all(PROVIDER);
        assert_eq!(f.tree(PROVIDER), tree, "{order:?}, {} a call", f.batch);
    }
    // `-q -q` in the project's addopts: no summary line, no difference.
    let (mut quiet, _) = orbit();
    quiet.on_step(|_| true, recorded("quiet-run"));
    quiet.run_all(PROVIDER);
    quiet.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(quiet.detail(PROVIDER, FAILS).message, whole.detail(PROVIDER, FAILS).message);
}

/// pytest-xdist 3.8.0, `-n 2` in the project's addopts (verified): the
/// plugin's lines of the workers' results and starts reach the controller,
/// interleaved, with no `test` lines; the same results, the same tree.
#[test]
fn under_xdist_the_workers_results_interleave_and_the_tree_is_the_same() {
    let (mut whole, _) = orbit();
    whole.on_recorded(recorded("run-all"));
    whole.run_all(PROVIDER);
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded("xdist-run"));
    let run = fenix.run_all(PROVIDER);
    assert!(run.complete, "{run:?}");
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(fenix.detail(PROVIDER, TEARDOWN).message.as_deref(), Some("teardown: RuntimeError: teardown broke"));
    assert!(fenix.detail(PROVIDER, FAILS).text.as_deref().is_some_and(|t| t.trim_start().starts_with("def test_fails_on_purpose")), "the worker's header is off the text: {:?}", fenix.detail(PROVIDER, FAILS).text);
    // Unscanned files, the same: the cases are found from what the workers report.
    let (mut bare, _) = fenix_with(facts(), &[]);
    bare.on_step(|_| true, recorded("xdist-run"));
    bare.run_all(PROVIDER);
    assert_eq!(bare.state(PROVIDER, NEGATIVE), State::Failed);
    assert_eq!(bare.record(PROVIDER, NEGATIVE).unwrap().parent.as_deref(), Some(CASES), "a case under its template found by the run");
    assert_eq!(bare.record(PROVIDER, DEEP).unwrap().parent.as_deref(), Some("\u{1f}tests/test_basic.py\u{1f}TestOuter\u{1f}TestInner"), "a test in a file nobody scanned, under its file and classes");
}

/// xdist with a file that doesn't load, and with a worker that dies (both
/// recorded): xdist goes on past a collection error, so the tests of the
/// files that loaded run, and a dead worker is its test's `???` result.
#[test]
fn under_xdist_a_broken_file_doesnt_stop_the_rest_and_a_dead_worker_is_an_errored_test() {
    let (mut fenix, _) = fenix_with(facts(), &[("tests/test_fine.py", FINE), ("tests/test_syntax.py", SYNTAX)]);
    fenix.on_step(|_| true, recorded("xdist-broken"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken, [(None, "tests/test_syntax.py didn't load: SyntaxError: invalid syntax".into())]);
    assert_eq!(fenix.state(PROVIDER, "tests/test_fine.py::test_ok"), State::Passed, "it ran, and its result stands");
    let files = [("tests/test_crash.py", CRASH), ("tests/test_zabort.py", ABORT)];
    let (mut fenix, _) = fenix_with(facts(), &files);
    fenix.on_step(|_| true, recorded("xdist-crash"));
    let run = fenix.run_all(PROVIDER);
    assert!(run.crashed.is_empty(), "the controller lived: {run:?}");
    fenix.assert_states(PROVIDER, &[("tests/test_crash.py::test_before", State::Passed), ("tests/test_crash.py::test_exits", State::Errored), ("tests/test_crash.py::test_after", State::Passed), ("tests/test_zabort.py::test_aborts", State::Errored)]);
    assert_eq!(fenix.detail(PROVIDER, "tests/test_crash.py::test_exits").message.as_deref(), Some("worker 'gw0' crashed while running 'tests/test_crash.py::test_exits'"));
}

/// A teardown that fails after a worker already sent another test's: the
/// late phase joins what was sent (`call` failed, then `teardown`), and a
/// worker that crashed is an errored test.
#[test]
fn a_phase_that_comes_late_joins_its_test_and_a_crashed_worker_is_errored() {
    let (mut fenix, _) = fenix_with(facts(), &[("tests/test_basic.py", BASIC)]);
    let line = |s: &str| format!("\nFENIX-TEST {s}\n");
    let out = [
        line(r#"{"kind": "start", "id": "tests/test_basic.py::test_adds"}"#),
        line(r#"{"kind": "start", "id": "tests/test_basic.py::test_fails_on_purpose"}"#),
        line(r#"{"kind": "result", "id": "tests/test_basic.py::test_fails_on_purpose", "when": "call", "outcome": "failed", "duration": 0.001, "message": "AssertionError: no", "at": ["tests/test_basic.py", 12], "text": "the call's text"}"#),
        line(r#"{"kind": "result", "id": "tests/test_basic.py::test_adds", "when": "call", "outcome": "rerun", "duration": 0.001}"#),
        line(r#"{"kind": "result", "id": "tests/test_basic.py::test_adds", "when": "call", "outcome": "passed", "duration": 0.001}"#),
        line(r#"{"kind": "result", "id": "tests/test_basic.py::test_fails_on_purpose", "when": "teardown", "outcome": "failed", "duration": 0.001, "message": "RuntimeError: teardown", "text": "the teardown's text"}"#),
        line(r#"{"kind": "result", "id": "tests/test_basic.py::test_skipped", "when": "???", "outcome": "failed", "text": "worker 'gw1' crashed while running 'tests/test_basic.py::test_skipped'"}"#),
        line(r#"{"kind": "done", "status": 1}"#),
    ]
    .concat();
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Errored);
    let d = fenix.detail(PROVIDER, FAILS);
    assert_eq!(d.message.as_deref(), Some("teardown: RuntimeError: teardown"));
    assert!(d.text.as_deref().is_some_and(|t| t.contains("the call's text") && t.contains("the teardown's text")), "{:?}", d.text);
    assert_eq!(fenix.state(PROVIDER, SKIPPED), State::Errored);
    assert!(fenix.detail(PROVIDER, SKIPPED).message.as_deref().is_some_and(|m| m.starts_with("worker 'gw1' crashed")));
}

/// §6.2's done when 4: a file that doesn't load is one `broken`, no test
/// failed: pytest ran nothing (exit 2). The listing says it too, and isn't
/// complete.
#[test]
fn a_file_that_doesnt_load_is_broken_not_a_test_failure() {
    let files = [("tests/test_fine.py", FINE), ("tests/test_syntax.py", SYNTAX)];
    let (mut fenix, _) = fenix_with(facts(), &files);
    fenix.on_step(|_| true, recorded("broken-run"));
    let run = fenix.run_all(PROVIDER);
    let message = "tests/test_syntax.py didn't load: SyntaxError: invalid syntax";
    assert_eq!(run.broken, [(None, message.into())], "{run:?}");
    assert!(!run.complete && run.crashed.is_empty());
    assert!(fenix.failed(PROVIDER).iter().all(|id| fenix.state(PROVIDER, id) == State::Errored), "no test failed");
    assert_eq!(fenix.state(PROVIDER, "tests/test_fine.py::test_ok"), State::Errored, "nothing ran");
    assert_eq!(fenix.detail(PROVIDER, "tests/test_fine.py::test_ok").message.as_deref(), Some(message));
    assert_eq!(fenix.at(PROVIDER, "tests/test_fine.py::test_ok"), Some(("tests/test_syntax.py".into(), 1, None)), "where it says it didn't load");
    let (mut fenix, _) = fenix_with(facts(), &files);
    fenix.on_step(|_| true, recorded("broken-collect"));
    let run = fenix.discover(PROVIDER);
    assert_eq!(run.broken, [(None, message.into())]);
    assert!(!run.complete, "a listing with a broken file isn't whole");
    assert_eq!(fenix.state(PROVIDER, "tests/test_fine.py::test_ok"), State::Unknown, "a listing errors nothing");
}

/// A test that kills its process (`os._exit(7)`, `os.abort()`): no `done`
/// line, so the step's end says it died, and which test was running. What
/// ended before it keeps its result; the rest of the step, errored.
#[test]
fn a_process_that_dies_errors_what_had_no_result_and_names_the_test_running() {
    let files = [("tests/test_crash.py", CRASH), ("tests/test_zabort.py", ABORT)];
    let mut tree = None;
    for order in [Order::Recorded, Order::StdoutFirst, Order::Shuffled(5)] {
        let (mut fenix, _) = fenix_with(facts(), &files);
        fenix.order = order.clone();
        fenix.on_step(|_| true, recorded("crash-all"));
        let run = fenix.run_all(PROVIDER);
        assert_eq!(run.crashed, [(None, "pytest exited with code 7 before the run ended while running tests/test_crash.py::test_exits".into())], "{order:?}");
        fenix.assert_states(PROVIDER, &[("tests/test_crash.py::test_before", State::Passed), ("tests/test_crash.py::test_exits", State::Errored), ("tests/test_crash.py::test_after", State::Errored), ("tests/test_zabort.py::test_aborts", State::Errored)]);
        match &tree {
            None => tree = Some(fenix.tree(PROVIDER)),
            Some(t) => assert_eq!(&fenix.tree(PROVIDER), t, "{order:?}"),
        }
    }
    // A signal, and Python's fatal error line.
    let (mut fenix, _) = fenix_with(facts(), &files);
    fenix.on_step(|_| true, recorded("crash-abort"));
    let run = fenix.run(PROVIDER, &["tests/test_zabort.py::test_aborts"]);
    assert_eq!(run.crashed, [(None, "Fatal Python error: Aborted while running tests/test_zabort.py::test_aborts".into())]);
    assert_eq!(fenix.state(PROVIDER, "tests/test_zabort.py::test_aborts"), State::Errored);
    // Killed by a signal with nothing said but the test it was running.
    fenix.on_step(|_| true, Recorded::stdout("\nFENIX-TEST {\"kind\": \"start\", \"id\": \"tests/test_zabort.py::test_aborts\"}\n").signal());
    let run = fenix.run(PROVIDER, &["tests/test_zabort.py::test_aborts"]);
    assert_eq!(run.crashed, [(None, "pytest was killed by a signal while running tests/test_zabort.py::test_aborts".into())], "{run:?}");
}

/// Everything pytest says when it can't start: a path that's gone, a
/// `conftest.py` that doesn't import, no pytest at all: `broken`, never a
/// failure; and no pytest is the runner's problem from then on.
#[test]
fn what_stops_pytest_before_a_test_is_broken_and_a_missing_pytest_is_the_runners_problem() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded("gone"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(run.broken, [(None, "pytest couldn't start: file or directory not found: tests/gone.py::test_x".into())]);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
    fenix.on_step(|_| true, recorded("conftest-broken"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(run.broken, [(None, "pytest couldn't start: ImportError while loading conftest '/project/tests/conftest.py': ModuleNotFoundError: No module named 'nonexistent_module_xyz'".into())]);
    assert_eq!(fenix.at(PROVIDER, ADDS), Some(("tests/conftest.py".into(), 1, None)));
    // Exit 5, nothing collected: nothing to say.
    let (mut empty, _) = fenix_with(facts(), &[]);
    empty.on_step(|_| true, recorded("empty-collect"));
    let run = empty.discover(PROVIDER);
    assert!(run.complete && run.broken.is_empty() && run.crashed.is_empty(), "{run:?}");
    empty.on_step(|_| true, recorded("empty-run"));
    assert!(empty.run_all(PROVIDER).broken.is_empty());
    // No pytest in the Python: said once, and the next ask says why it won't run.
    let shared = facts();
    let (mut fenix, _) = fenix_with(shared.clone(), ORBIT);
    fenix.on_step(|_| true, recorded("no-pytest"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken, [(None, "pytest isn't installed in this environment".into())]);
    assert!(shared.borrow().missing);
    let problem = fenix.ext.tests_plan(PROVIDER, 99, api::Ask::RunAll);
    assert!(problem.unwrap_err().starts_with("pytest isn't installed in this environment: "));
}

/// §6.2's done when 3: one test by its node id: the exact command, its
/// `covers`, its `cwd`; the others stay as they were.
#[test]
fn one_test_runs_by_its_node_id() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-one"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(command_text(&run.steps[0].command), format!("python3 -c {} ${{data}}/lang-kit/tests -q -p fenix_tests --rootdir=. {ADDS}", super::plan::BOOT));
    assert_eq!((run.steps[0].covers.as_slice(), run.steps[0].command.cwd.as_deref()), ([ADDS.to_string()].as_slice(), Some("/project")));
    assert_eq!(run.steps[0].command.env, [("PYTHONIOENCODING".to_string(), "utf-8".to_string())]);
    assert!(!run.lists_after);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Unknown);
}

/// The failed again: one step, every failed or errored test by its node id.
#[test]
fn the_failed_run_again_in_one_step_by_node_id() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    fenix.on_step(|s| s.covers.len() == 7, Recorded::stdout("").exits(0));
    let run = fenix.run_failed(PROVIDER);
    assert_eq!(run.steps.len(), 1);
    let line = command_text(&run.steps[0].command);
    assert!(line.contains("--rootdir=. tests/") && line.contains(&format!(" {DEEP}")) && line.contains(&format!(" {NEGATIVE}")), "{line}");
    let mut covers = run.steps[0].covers.clone();
    covers.sort();
    let mut expected: Vec<String> = [UNIT_BAD, FAILS, NEGATIVE, DEEP, STRICT, BROKEN_FIXTURE, TEARDOWN].map(String::from).to_vec();
    expected.sort();
    assert_eq!(covers, expected);
}

/// A file or a class asked runs by its own node id: pytest's are exact, so a
/// fill-in is no use to it, and none changes nothing.
#[test]
fn a_file_or_a_class_runs_by_its_node_id_with_a_fill_in_or_without() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run_file(PROVIDER, "/project/tests/test_basic.py");
    assert_eq!(run.steps[0].command.args.last().map(String::as_str), Some("tests/test_basic.py"));
    assert_eq!(run.steps[0].covers, [BASIC_FILE]);
    let run = fenix.run(PROVIDER, &[OUTER]);
    assert_eq!(run.steps[0].command.args.last().map(String::as_str), Some("tests/test_basic.py::TestOuter"));
    assert_eq!(run.steps[0].covers, [OUTER]);
    // No fill-in (a file Fenix couldn't name all of): the same.
    let mut p = Provider::new(facts());
    p.scan("/project/tests/test_basic.py", BASIC);
    let groups = vec![Group::file("tests/test_basic.py"), Group::suite("TestOuter", "tests/test_basic.py", 30), Group::suite("TestInner", "tests/test_basic.py", 34)];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups, tests: None, known: Vec::new() }])).unwrap();
    assert_eq!(steps[0].command.args.last().map(String::as_str), Some("tests/test_basic.py::TestOuter::TestInner"));
}

/// After Fenix starts again it gives no `data`: the plan comes from the keys.
#[test]
fn without_data_the_plan_comes_from_the_keys() {
    let (mut fenix, _) = orbit();
    fenix.forget_data(PROVIDER);
    let api::Ask::Run(records) = fenix.ask_for(PROVIDER, &[ADDS, DEEP]) else { panic!() };
    assert!(records.iter().all(|r| r.data.is_none()));
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run(PROVIDER, &[ADDS, DEEP]);
    assert!(command_text(&run.steps[0].command).ends_with(&format!("--rootdir=. {ADDS} {DEEP}")));
}

/// §6.2's done when 3: a template runs by its id without brackets, all its
/// cases, and a case by its own id (brackets and spaces, no shell).
#[test]
fn a_template_runs_all_its_cases_and_a_case_by_its_own_id() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("collect"));
    fenix.discover(PROVIDER);
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run(PROVIDER, &[CASES]);
    assert_eq!(run.steps[0].command.args.last().map(String::as_str), Some(CASES));
    let run = fenix.run(PROVIDER, &[ODD, DEEP]);
    assert_eq!(run.steps[0].command.args[7..], [ODD, DEEP], "the id with brackets and spaces, one argument");
    fenix.on_step(|_| true, recorded("run-set"));
    let ids = [ODD, OUTER, UNIT_BAD];
    let run = fenix.run(PROVIDER, &ids);
    assert_eq!(run.steps[0].covers, [UNIT_BAD, ODD, OUTER], "in the page's order");
    // The run's results, whether the case is run alone or with its template.
    fenix.assert_states(PROVIDER, &[(ODD, State::Passed), (DEEP, State::Failed), (UNIT_BAD, State::Failed)]);
    // The template, run whole: its cases' results are its; no result of its own.
    let (mut again, _) = orbit();
    again.on_step(|_| true, recorded("run-all"));
    again.on_command(CASES, recorded("run-all"));
    again.run(PROVIDER, &[CASES]);
    assert_eq!(again.state(PROVIDER, CASES), State::Failed, "its cases': one failed");
    assert_eq!(again.detail(PROVIDER, CASES), Default::default());
    assert!(!again.failed(PROVIDER).iter().any(|id| again.detail(PROVIDER, id).message.as_deref() == Some(NO_RESULT)));
}

/// A project with its own pytest.ini in a folder of it: that folder is the
/// step's `cwd` and its node ids are below it; the same test file name in
/// two rootdirs stays two tests, told apart by the folder in the key.
#[test]
fn two_rootdirs_are_two_steps_with_their_keys_told_apart() {
    let dir = std::env::temp_dir().join(format!("fenix-python-roots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("services/api/tests")).unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"top\"\n").unwrap();
    std::fs::write(dir.join("services/api/pytest.ini"), "[pytest]\n").unwrap();
    let test = "def test_same():\n    assert True\n";
    let shared = facts();
    shared.borrow_mut().root = dir.to_string_lossy().into_owned();
    let (mut fenix, _) = fenix_with(shared, &[("tests/test_x.py", test), ("services/api/tests/test_x.py", test)]);
    assert!(fenix.record(PROVIDER, "tests/test_x.py::test_same").is_some() && fenix.record(PROVIDER, "services/api/tests/test_x.py::test_same").is_some());
    fenix.on_step(|s| s.command.cwd.as_deref() == Some("/project"), Recorded::stdout("\nFENIX-TEST {\"kind\": \"result\", \"id\": \"tests/test_x.py::test_same\", \"when\": \"call\", \"outcome\": \"passed\", \"duration\": 0.1}\n\nFENIX-TEST {\"kind\": \"done\", \"status\": 0}\n").exits(0));
    fenix.on_step(|s| s.command.cwd.as_deref() == Some("/project/services/api"), Recorded::stdout("\nFENIX-TEST {\"kind\": \"result\", \"id\": \"tests/test_x.py::test_same\", \"when\": \"call\", \"outcome\": \"failed\", \"duration\": 0.1, \"message\": \"AssertionError\", \"at\": [\"tests/test_x.py\", 2]}\n\nFENIX-TEST {\"kind\": \"done\", \"status\": 1}\n").exits(1));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps.iter().map(|s| s.command.cwd.clone().unwrap()).collect::<Vec<_>>(), ["/project", "/project/services/api"]);
    assert_eq!(run.steps[1].label, "every test in services/api");
    fenix.assert_states(PROVIDER, &[("tests/test_x.py::test_same", State::Passed), ("services/api/tests/test_x.py::test_same", State::Failed)]);
    assert_eq!(fenix.at(PROVIDER, "services/api/tests/test_x.py::test_same"), Some(("services/api/tests/test_x.py".into(), 2, None)), "a place below the folder it ran in, below the project");
    // Asked together, each in its own: a step each, with its ids below its folder.
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run(PROVIDER, &["tests/test_x.py::test_same", "services/api/tests/test_x.py::test_same"]);
    let args: Vec<(String, String)> = run.steps.iter().map(|s| (s.command.cwd.clone().unwrap(), s.command.args.last().cloned().unwrap())).collect();
    assert_eq!(args, [("/project".into(), "tests/test_x.py::test_same".into()), ("/project/services/api".into(), "tests/test_x.py::test_same".into())]);
    // Debugged from its own rootdir, its id below it.
    let config = fenix.launch(PROVIDER, "services/api/tests/test_x.py::test_same").config.unwrap();
    assert_eq!((config.cwd.as_deref(), config.args.as_slice()), (Some("${project}/services/api"), ["-q".to_string(), "tests/test_x.py::test_same".to_string()].as_slice()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// §6.2's done when 5: 2,000 failed tests run again: the ids in a file,
/// one step, its command line as short as ever, every test covered.
#[test]
fn two_thousand_failed_tests_run_again_from_a_file_of_ids() {
    // In four files, so no one scan is the big one the driver times.
    let files: Vec<(String, String)> = (0..4).map(|f| (format!("tests/test_many{f}.py"), (0..500).map(|i| format!("def test_fails_on_purpose_number_{f}{i:04}():\n    assert False\n\n\n")).collect())).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let (mut fenix, _) = fenix_with(facts(), &refs);
    let mut out = String::new();
    for n in 0..2000 {
        out.push_str(&format!("\nFENIX-TEST {{\"kind\": \"result\", \"id\": \"tests/test_many{}.py::test_fails_on_purpose_number_{}{:04}\", \"when\": \"call\", \"outcome\": \"failed\", \"duration\": 0.0001, \"message\": \"assert False\", \"at\": [\"tests/test_many{}.py\", {}]}}\n", n / 500, n / 500, n % 500, n / 500, (n % 500) * 4 + 2));
    }
    out.push_str("\nFENIX-TEST {\"kind\": \"done\", \"status\": 1}\n");
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.failed(PROVIDER).len(), 2000);
    // The file the step has: the driver writes it as Fenix does, and its
    // lines are the ids the second run passes.
    let data = fenix.data_dir().to_path_buf();
    fenix.on_step_with(move |s| {
        let file = s.command.args.last()?.strip_prefix("@${data}/")?;
        let ids = std::fs::read_to_string(data.join(file)).ok()?;
        let lines: String = ids.lines().map(|id| format!("\nFENIX-TEST {{\"kind\": \"result\", \"id\": \"{id}\", \"when\": \"call\", \"outcome\": \"passed\", \"duration\": 0.0001}}\n")).collect();
        Some(Recorded::stdout(format!("{lines}\nFENIX-TEST {{\"kind\": \"done\", \"status\": 0}}\n")))
    });
    let run = fenix.run_failed(PROVIDER);
    assert_eq!(run.steps.len(), 1, "pytest reads its ids from a file");
    assert!(DIRECT.fits(&run.steps[0].command) && run.steps[0].command.args.last().is_some_and(|a| a.starts_with("@${data}/lang-kit/tests/runs/")));
    assert!(run.steps[0].needs.iter().any(|n| n.ends_with("/ids.txt")) && run.steps[0].needs.iter().any(|n| n.ends_with("fenix_tests.py")));
    assert_eq!(run.steps[0].covers.len(), 2000);
    assert!(fenix.failed(PROVIDER).is_empty(), "every one ran and passed");
}

/// §6.2's launch: straight away, debugpy's pytest module on the node id,
/// from the rootdir, as `launch/pytest-file.toml`.
#[test]
fn debugging_a_test_launches_pytest_on_its_node_id_from_its_rootdir() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("collect"));
    fenix.discover(PROVIDER);
    let launched = fenix.launch(PROVIDER, DEEP);
    assert!(launched.built.is_none(), "no build first");
    let config = launched.config.unwrap();
    assert_eq!((config.name.as_str(), config.program.as_deref(), config.args.as_slice(), config.cwd.as_deref()), (format!("pytest: {DEEP}").as_str(), None, ["-q".to_string(), DEEP.to_string()].as_slice(), Some("${project}")));
    assert_eq!(config.languages, ["python"]);
    let attributes: serde_json_value::Value = serde_json_value::from_str(&config.attributes).unwrap();
    assert_eq!((attributes["module"].as_str(), attributes["justMyCode"].as_bool()), (Some("pytest"), Some(false)));
    // A template: all its cases; a case, its own id.
    assert_eq!(fenix.launch(PROVIDER, CASES).config.unwrap().args, ["-q", CASES]);
    assert_eq!(fenix.launch(PROVIDER, ODD).config.unwrap().args, ["-q", ODD]);
    // Not a file or a class.
    let mut p = Provider::new(facts());
    assert!(p.launch(&fenix_lang_kit::tests::Found::test(BASIC_FILE, "tests/test_basic.py"), &[]).is_err());
    let launch = p.launch(&fenix_lang_kit::tests::Found::test("tests/test_basic.py::test_adds", "adds"), &[]).unwrap();
    assert!(matches!(launch, fenix_lang_kit::tests::Launching::Now(_)));
}

use fenix_extension_api::serde_json as serde_json_value;

/// Windows by hand: backslashes in a place, a full path of the machine.
#[test]
fn a_windows_place_and_a_full_path_are_below_the_project() {
    let (mut fenix, _) = fenix_with(facts(), &[("tests/test_basic.py", BASIC)]);
    let out = "\nFENIX-TEST {\"kind\": \"result\", \"id\": \"tests/test_basic.py::test_fails_on_purpose\", \"when\": \"call\", \"outcome\": \"failed\", \"duration\": 0.1, \"file\": \"C:\\\\work\\\\orbit\\\\tests\\\\test_basic.py\", \"line\": 12, \"message\": \"AssertionError: no\", \"at\": [\"tests\\\\test_basic.py\", 12]}\n\nFENIX-TEST {\"kind\": \"result\", \"id\": \"tests/test_basic.py::test_uses_broken_fixture\", \"when\": \"setup\", \"outcome\": \"failed\", \"duration\": 0.1, \"file\": \"C:\\\\work\\\\orbit\\\\tests\\\\conftest.py\", \"line\": 3, \"message\": \"RuntimeError: no\"}\n\nFENIX-TEST {\"kind\": \"done\", \"status\": 1}\n";
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run(PROVIDER, &[FAILS, BROKEN_FIXTURE]);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("tests/test_basic.py".into(), 12, None)));
    assert_eq!(fenix.at(PROVIDER, BROKEN_FIXTURE), Some(("tests/conftest.py".into(), 3, None)), "a full path in the project, by the one a result of the test's own file told");
}

/// A stop mid-run: what ended keeps its result, the rest goes back.
#[test]
fn a_stop_keeps_what_ended() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded("run-all").stopped_after(90));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps[0].end, StepEnd::Stopped);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, UNIT_OK), State::Passed);
    assert_eq!(fenix.state(PROVIDER, TEARDOWN), State::Unknown);
    assert_eq!(fenix.state(PROVIDER, JAPANESE), State::Unknown);
    assert!(run.crashed.is_empty() && run.broken.is_empty(), "stopped isn't a crash");
}

/// Python or uv missing, pytest missing in the environment: the runner
/// can't run, and says why; uv runs it, and with a lock the version is
/// known.
#[test]
fn the_runner_says_pytest_and_why_it_cant_run() {
    let mut p = Provider::new(facts());
    assert_eq!(p.runner(), ("pytest 9.2.0".to_string(), None));
    let shared = facts();
    shared.borrow_mut().pytest = None;
    let mut p = Provider::new(shared.clone());
    assert_eq!(p.runner(), ("pytest".to_string(), None));
    shared.borrow_mut().missing = true;
    assert!(p.runner().1.is_some_and(|why| why.starts_with("pytest isn't installed in this environment")));
    shared.borrow_mut().uv = Some("uv".into());
    assert_eq!(p.runner().1, None, "uv brings its own");
    {
        let mut f = shared.borrow_mut();
        f.uv = None;
        f.python = None;
    }
    assert!(p.runner().1.is_some_and(|why| why.starts_with("Python isn't installed")));
    assert!(p.plan(&Ask::RunAll).unwrap_err().starts_with("Python isn't installed"));
}

/// The plans, run for real: every step the provider makes is run with the
/// Python in `FENIX_TEST_PYTHON` (one with pytest, and pytest-xdist for the
/// last part), in a copy of the sample project, and what it prints is
/// read as Fenix would read it. Not run by default (it needs that Python);
/// run it with
/// `FENIX_TEST_PYTHON=/path/to/venv/bin/python cargo test --locked -- --ignored against_real_pytest`.
/// What the recordings in `src/samples/tests/pytest` were made with.
#[test]
#[ignore = "needs a Python with pytest: FENIX_TEST_PYTHON"]
fn against_real_pytest() {
    let Ok(python) = std::env::var("FENIX_TEST_PYTHON") else { panic!("FENIX_TEST_PYTHON isn't set") };
    let work = std::env::temp_dir().join(format!("fenix-python-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (src, dst) = (e.path(), to.join(e.file_name()));
            if src.is_dir() {
                copy(&src, &dst);
            } else {
                std::fs::copy(&src, &dst).unwrap();
            }
        }
    }
    copy(&std::path::Path::new(PYTEST).join("orbit"), &work.join("orbit"));
    copy(&std::path::Path::new(PYTEST).join("orbit-broken"), &work.join("orbit-broken"));
    copy(&std::path::Path::new(PYTEST).join("orbit-crash"), &work.join("orbit-crash"));
    let project = |name: &str| std::fs::canonicalize(work.join(name)).unwrap();
    let running = |name: &str, python: String, fenix: &Fenix<Kit>| {
        let (root, data) = (project(name), fenix.data_dir().to_path_buf());
        move |s: &api::Step| -> Option<Recorded> {
            let fill = |a: &str| a.replace("${data}", &data.to_string_lossy());
            let cwd = s.command.cwd.as_deref().unwrap_or("/project").replacen("/project", &root.to_string_lossy(), 1);
            let out = std::process::Command::new(&python).args(s.command.args.iter().map(|a| fill(a))).envs(s.command.env.iter().cloned()).current_dir(cwd).output().unwrap();
            let rec = Recorded::stdout(out.stdout).and_stderr(out.stderr);
            Some(match out.status.code() {
                Some(c) => rec.exits(c),
                None => rec.signal(),
            })
        }
    };
    let scan = |name: &str, fenix: &mut Fenix<Kit>| {
        for (path, text) in [("tests/test_basic.py", BASIC), ("tests/test_extra.py", EXTRA), ("tests/sub/test_unit.py", UNIT)].iter().filter(|_| name == "orbit") {
            fenix.scan(PROVIDER, &format!("/project/{path}"), text);
        }
    };
    let shared = || -> SharedFacts { Rc::new(RefCell::new(Facts { python: Some("python3".into()), root: work.join("orbit").to_string_lossy().into_owned(), generation: 1, ..Facts::default() })) };

    // The listing, then everything, then the failed again, then a file and a class and a case.
    let (mut fenix, _) = fenix_with(shared(), &[]);
    scan("orbit", &mut fenix);
    let rule = running("orbit", python.clone(), &fenix);
    fenix.on_step_with(rule);
    let run = fenix.discover(PROVIDER);
    assert!(run.complete, "{run:?}");
    assert_eq!(fenix.record(PROVIDER, NEGATIVE).unwrap().parent.as_deref(), Some(CASES));
    let run = fenix.run_all(PROVIDER);
    assert!(run.complete && run.crashed.is_empty() && run.broken.is_empty(), "{run:?}");
    fenix.assert_states(PROVIDER, RUN_ALL);
    let run = fenix.run_failed(PROVIDER);
    assert_eq!(run.steps.len(), 1);
    assert_eq!(fenix.failed(PROVIDER).len(), 7, "the same seven fail again");
    let run = fenix.run(PROVIDER, &[ODD, OUTER]);
    assert_eq!(run.steps[0].covers.len(), 2);
    fenix.run(PROVIDER, &[CASES]);
    assert_eq!(fenix.state(PROVIDER, NEGATIVE), State::Failed);
    assert_eq!(fenix.state(PROVIDER, ODD), State::Passed);
    assert_eq!(fenix.state(PROVIDER, DEEP), State::Failed);
    let run = fenix.run_file(PROVIDER, "/project/tests/test_extra.py");
    assert!(run.complete || !run.steps.is_empty());
    assert_eq!(fenix.state(PROVIDER, STACKED), State::Passed);
    // A long id (over 1 KiB) by its data, then after a restart by its template.
    let long = fenix.ids(PROVIDER).into_iter().find(|id| id.starts_with("tests/test_extra.py::test_long_id[y")).unwrap();
    fenix.run(PROVIDER, &[long.as_str()]);
    assert_eq!(fenix.state(PROVIDER, &long), State::Passed);
    fenix.forget_data(PROVIDER);
    fenix.run(PROVIDER, &[long.as_str()]);
    assert_eq!(fenix.state(PROVIDER, &long), State::Passed);
    // More ids than go on a command line: a file of ids.
    let many: Vec<&str> = RUN_ALL.iter().map(|(id, _)| *id).collect();
    let run = fenix.run(PROVIDER, &many);
    assert!(run.steps[0].command.args.last().is_some_and(|a| a.starts_with("@${data}/")));
    fenix.assert_states(PROVIDER, RUN_ALL);

    // A file that doesn't load, a process that dies.
    let (mut broken, _) = fenix_with(shared(), &[("tests/test_fine.py", FINE), ("tests/test_syntax.py", SYNTAX)]);
    let rule = running("orbit-broken", python.clone(), &broken);
    broken.on_step_with(rule);
    let run = broken.run_all(PROVIDER);
    assert_eq!(run.broken.len(), 1, "{run:?}");
    assert!(run.broken[0].1.starts_with("tests/test_syntax.py didn't load: SyntaxError"));
    let (mut crash, _) = fenix_with(shared(), &[("tests/test_crash.py", CRASH), ("tests/test_zabort.py", ABORT)]);
    let rule = running("orbit-crash", python.clone(), &crash);
    crash.on_step_with(rule);
    let run = crash.run_all(PROVIDER);
    // Under xdist (`PYTEST_ADDOPTS=-n 2`) a worker dies, not the process.
    let xdist = std::env::var("PYTEST_ADDOPTS").is_ok_and(|o| o.contains("-n "));
    assert_eq!(run.crashed.len(), usize::from(!xdist), "{run:?}");
    assert!(xdist || run.crashed[0].1.ends_with("while running tests/test_crash.py::test_exits"), "{run:?}");
    assert_eq!(crash.state(PROVIDER, "tests/test_crash.py::test_before"), State::Passed);
    assert_eq!(crash.state(PROVIDER, "tests/test_crash.py::test_exits"), State::Errored);
    let _ = std::fs::remove_dir_all(&work);
}
