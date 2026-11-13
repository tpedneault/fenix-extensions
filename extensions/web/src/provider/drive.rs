//! The provider through the test kit's driver, `fenix_test_kit::tests::
//! Fenix`, which plays Fenix's side as strictly as Fenix, on what Vitest
//! 5.1.0 and Jest 30.5.2 (Node 22.23.4) printed for the sample projects in
//! `src/samples/tests/vitest/orbit`, `.../jest/orbit` and
//! `.../workspace/mono` through the reporters in `src/tests/` (recorded as
//! `plans/test-providers-recipe.md` says). The driver gives each recording
//! to the provider in batches, cut, shuffled, and times every call.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_lang_kit::tests::{api, Ask, Asked, Group, Kind, Outcome, Report, StepEnd, TestRunner, DIRECT};
use fenix_lang_kit::Kit;
use fenix_test_kit::tests::{command_text, Fenix, Order, Recorded, State};
use fenix_test_kit::{assert_snapshot, Fake};

use super::layout::{Installed, Layout, Runner};
use super::{Facts, Provider, SharedFacts, PROVIDER};

const VITEST: &str = "src/samples/tests/vitest";
const JEST: &str = "src/samples/tests/jest";
const WORKSPACE: &str = "src/samples/tests/workspace";

const V_ORBIT: &str = include_str!("../samples/tests/vitest/orbit/src/orbit.test.js");
const V_GEO: &str = include_str!("../samples/tests/vitest/orbit/src/geo.test.ts");
const V_BROKEN: &str = include_str!("../samples/tests/vitest/orbit/broken.test.js.txt");
const V_CRASH: &str = include_str!("../samples/tests/vitest/orbit/crash.test.js.txt");
const J_ORBIT: &str = include_str!("../samples/tests/jest/orbit/src/orbit.test.js");
const J_GEO: &str = include_str!("../samples/tests/jest/orbit/src/geo.test.js");
const J_BROKEN: &str = include_str!("../samples/tests/jest/orbit/broken.test.js.txt");
const J_CRASH: &str = include_str!("../samples/tests/jest/orbit/crash.test.js.txt");
const APP: &str = include_str!("../samples/tests/workspace/mono/packages/app/src/orbit.test.js");
const LIB: &str = include_str!("../samples/tests/workspace/mono/packages/lib/src/orbit.test.js");

const ADDS: &str = "src/orbit.test.js > adds";
const FAILS: &str = "src/orbit.test.js > fails on purpose";
const SKIPPED: &str = "src/orbit.test.js > skipped";
const TODO: &str = "src/orbit.test.js > to do";
const TEMPLATE: &str = "src/orbit.test.js > add(%i, %i) is %i";
const CASE_OK: &str = "src/orbit.test.js > add(1, 1) is 2";
const CASE_BAD: &str = "src/orbit.test.js > add(-1, -1) is -3";
const IN_DESCRIBE: &str = "src/orbit.test.js > outer > in describe";
const DEEP: &str = "src/orbit.test.js > outer > inner > deep";
const TWICE: &str = "src/orbit.test.js > names > twice";
const TWICE_2: &str = "src/orbit.test.js > names > twice (2)";
const REGEX: &str = "src/orbit.test.js > names > add(-1, -1) [regex] $x";
const LOOP: &str = "src/orbit.test.js > names > adds ${n}";
const LOOP_1: &str = "src/orbit.test.js > names > adds 1";
const VARIABLE: &str = "src/orbit.test.js > names > from a variable";
const LEFT_OUT: &str = "src/orbit.test.js > left out > with its describe";
const SYMMETRIC_TS: &str = "src/geo.test.ts > dist is symmetric";
const ARROWS_TS: &str = "src/geo.test.ts > a > b > c";
const SYMMETRIC_JS: &str = "src/geo.test.js > dist is symmetric";
const ARROWS_JS: &str = "src/orbit.test.js > a > b > c";
/// `src/orbit.test.js` › `outer`, and its file.
const OUTER: &str = "\u{1f}src/orbit.test.js\u{1f}outer";
const NAMES: &str = "\u{1f}src/orbit.test.js\u{1f}names";

/// The sample's packages, as `Layout::read` finds them, with their runner
/// installed where npm put it (`node_modules` isn't kept).
fn facts(dir: &str) -> SharedFacts {
    let mut layout = Layout::read(dir);
    for p in &mut layout.packages {
        let up = p.dir.split('/').filter(|s| !s.is_empty()).count();
        let version = if p.runner == Runner::Vitest { "5.1.0" } else { "30.5.2" };
        p.installed = Some(Installed { script: format!("{}node_modules/{}", "../".repeat(up), p.runner.script()), version: Some(version.into()) });
    }
    Rc::new(RefCell::new(Facts { layout, node: Some("node".into()), yarn: None }))
}

/// Fenix with the pack's provider, the files scanned.
fn fenix_with(facts: SharedFacts, files: &[(&str, &str)]) -> (Fenix<Kit>, Fake) {
    let fake = Fake::new();
    let kit = Kit::with_host("web", fake.host()).tests(Box::new(Provider::new(facts)));
    let mut fenix = Fenix::new(kit);
    fenix.manifest("extension.toml").with_fake(fake.clone());
    for (path, text) in files {
        fenix.scan(PROVIDER, &format!("/project/{path}"), text);
    }
    (fenix, fake)
}

fn vitest() -> (Fenix<Kit>, Fake) {
    fenix_with(facts(&format!("{VITEST}/orbit")), &[("src/orbit.test.js", V_ORBIT), ("src/geo.test.ts", V_GEO)])
}

fn jest() -> (Fenix<Kit>, Fake) {
    fenix_with(facts(&format!("{JEST}/orbit")), &[("src/orbit.test.js", J_ORBIT), ("src/geo.test.js", J_GEO)])
}

/// The tests that time a heavy call take this, so they don't time each
/// other.
static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn heavy() -> std::sync::MutexGuard<'static, ()> {
    HEAVY.lock().unwrap_or_else(|e| e.into_inner())
}

fn recorded(dir: &str, name: &str) -> Recorded {
    Recorded::from(format!("{dir}/{name}"))
}

/// What a run of everything in Vitest's sample gives.
const V_RUN_ALL: &[(&str, State)] = &[
    (ADDS, State::Passed),
    (FAILS, State::Failed),
    (SKIPPED, State::Skipped),
    (TODO, State::Skipped),
    (CASE_OK, State::Passed),
    (CASE_BAD, State::Failed),
    (IN_DESCRIBE, State::Passed),
    (DEEP, State::Failed),
    (TWICE, State::Passed),
    (TWICE_2, State::Failed),
    (REGEX, State::Passed),
    (LOOP_1, State::Passed),
    (VARIABLE, State::Passed),
    (LEFT_OUT, State::Skipped),
    (SYMMETRIC_TS, State::Passed),
    (ARROWS_TS, State::Passed),
];

/// 1: the scan, at the tests' lines, under their file and describes; a
/// skip, a todo and a skipped describe's tests; `test.each` and a
/// template literal's title as templates; a repeated name as ` (2)`; a
/// title that isn't a literal not found; Fenix told the runner.
#[test]
fn a_scan_gives_each_test_at_its_line_under_its_describes() {
    let (fenix, fake) = vitest();
    let deep = fenix.record(PROVIDER, DEEP).unwrap();
    assert_eq!((deep.line, deep.end_line, deep.parent.as_deref(), deep.path.as_deref()), (Some(25), Some(27), Some("\u{1f}src/orbit.test.js\u{1f}outer\u{1f}inner"), Some("src/orbit.test.js")), "lines from 0 in Fenix: line 26");
    assert_eq!(fenix.record(PROVIDER, OUTER).unwrap().kind, Kind::Suite);
    assert_eq!(fenix.record(PROVIDER, SKIPPED).unwrap().skip.as_deref(), Some(""));
    assert_eq!(fenix.record(PROVIDER, TODO).unwrap().skip.as_deref(), Some("todo"));
    assert_eq!(fenix.record(PROVIDER, LEFT_OUT).unwrap().skip.as_deref(), Some(""), "its describe is skipped");
    assert!(fenix.record(PROVIDER, TEMPLATE).unwrap().template && fenix.record(PROVIDER, LOOP).unwrap().template);
    let twice = fenix.record(PROVIDER, TWICE_2).unwrap();
    assert_eq!((twice.label.as_str(), twice.line), ("twice (2)", Some(33)));
    assert!(fenix.record(PROVIDER, VARIABLE).is_none(), "its title is a variable's");
    assert_eq!(fenix.record(PROVIDER, ARROWS_TS).unwrap().parent.as_deref(), Some("\u{1f}src/geo.test.ts\u{1f}a > b"));
    assert_eq!(fenix.told(PROVIDER), Some(("Vitest 5.1.0".into(), None)));
    assert!(fake.take().iter().any(|c| c == "test-provider web: Vitest 5.1.0"));
    assert_snapshot!("vitest-scan", fenix.tree(PROVIDER));
}

/// 1: what isn't a test file isn't scanned; half-typed text keeps the
/// tests after it; odd bytes don't panic.
#[test]
fn a_file_half_typed_keeps_its_tests_and_other_files_have_none() {
    let (mut fenix, _) = vitest();
    assert!(fenix.scan(PROVIDER, "/project/src/orbit.js", "test('not a test file', () => {})").is_empty());
    assert!(fenix.scan(PROVIDER, "/project/node_modules/x/a.test.js", "test('a', () => {})").is_empty());
    let typing = V_ORBIT.replacen("test('fails on purpose', () => {", "test('fails on purpose', () => {\n  test('half', () => { expect(", 1);
    fenix.scan(PROVIDER, "/project/src/orbit.test.js", &typing);
    for id in [ADDS, FAILS, DEEP, TWICE_2, LEFT_OUT] {
        assert!(fenix.record(PROVIDER, id).is_some(), "{id} kept");
    }
    fenix.scan(PROVIDER, "/project/src/orbit.test.js", "\u{0}\u{feff}test(`\\u{zz}`, describe('");
}

/// 2: a file of 2,000 tests is scanned within the driver's budget for a
/// call (5 ms in a release build, 50 ms in a debug one), the provider's
/// scan and lang-kit's adapter together, natively. The best of five.
#[test]
fn a_file_of_two_thousand_tests_is_scanned_within_a_calls_budget() {
    let _heavy = heavy();
    let text: String = std::iter::once("import { describe, test, expect } from 'vitest'\n\ndescribe('many', () => {\n".to_string())
        .chain((0..2000).map(|i| format!("  test('fails on purpose number {i:05}', () => {{\n    expect(1 + 1).toBe(2)\n  }})\n\n")))
        .chain(std::iter::once("})\n".to_string()))
        .collect();
    let budget = if cfg!(debug_assertions) { std::time::Duration::from_millis(50) } else { std::time::Duration::from_millis(5) };
    let best = (0..5)
        .map(|_| {
            let mut kit = Kit::with_host("web", Fake::new().host()).tests(Box::new(Provider::new(facts(&format!("{VITEST}/orbit")))));
            let start = std::time::Instant::now();
            let found = kit.tests_scan(PROVIDER, "/project/src/many.test.js", &text);
            assert_eq!(found.len(), 2002, "the file, the describe and 2,000 tests");
            start.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < budget, "{best:?}");
}

/// 3: the listing adds what the scan can't see -- a title from a
/// variable under its describe, `test.each`'s and the loop's cases under
/// their templates -- and keeps the scan's lines.
#[test]
fn a_listing_adds_what_the_scan_cant_see_under_its_groups() {
    for (mut fenix, dir) in [(vitest().0, VITEST), (jest().0, JEST)] {
        fenix.on_recorded(recorded(dir, "discover"));
        let run = fenix.discover(PROVIDER);
        assert!(run.complete, "{run:?}");
        let variable = fenix.record(PROVIDER, VARIABLE).unwrap();
        assert_eq!((variable.parent.as_deref(), variable.kind), (Some(NAMES), Kind::Test), "{dir}");
        let case = fenix.record(PROVIDER, CASE_BAD).unwrap();
        assert_eq!((case.kind, case.parent.as_deref(), case.label.as_str()), (Kind::Case, Some(TEMPLATE), "add(-1, -1) is -3"), "{dir}");
        assert_eq!(fenix.record(PROVIDER, LOOP_1).unwrap().parent.as_deref(), Some(LOOP), "{dir}: a template literal's case, by its line");
        assert_eq!(fenix.record(PROVIDER, ADDS).unwrap().line, Some(if dir == VITEST { 3 } else { 2 }), "the scan's line stays");
        assert_eq!(fenix.state(PROVIDER, ADDS), State::Unknown, "{dir}: nothing ran");
    }
    let (mut fenix, _) = vitest();
    fenix.on_recorded(recorded(VITEST, "discover"));
    let run = fenix.discover(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "node node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=${data}/lang-kit/tests/fenix-vitest.mjs -t FENIX_NOTHING");
    assert_eq!(run.steps[0].needs, ["/data/lang-kit/tests/fenix-vitest.mjs"]);
    assert_snapshot!("vitest-listed", fenix.tree(PROVIDER));
}

/// 4: a run of all, every outcome; a failure at its assertion with its
/// message, its diff in its text and what it logged; a todo says so;
/// the cases found under their templates, which get no result of their own.
#[test]
fn vitest_a_run_of_all_gives_each_test_its_result_at_its_assertion() {
    let (mut fenix, _) = vitest();
    fenix.on_recorded(recorded(VITEST, "run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "node node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs");
    assert_eq!(run.steps[0].command.cwd.as_deref(), Some("/project"));
    fenix.assert_states(PROVIDER, V_RUN_ALL);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/orbit.test.js".into(), 10, Some(21))));
    let d = fenix.detail(PROVIDER, FAILS);
    assert_eq!(d.message.as_deref(), Some("expected 4 to be 5 // Object.is equality"));
    assert!(d.text.as_deref().is_some_and(|t| t.contains("- Expected") && t.contains("+ 4")), "{:?}", d.text);
    assert!(d.duration_ms.is_some_and(|ms| ms > 0.0));
    assert_eq!(fenix.model(PROVIDER).nodes[FAILS].printed, "some output before the failure");
    assert_eq!(fenix.detail(PROVIDER, TODO).message.as_deref(), Some("todo"));
    assert_eq!(fenix.at(PROVIDER, CASE_BAD), Some(("src/orbit.test.js".into(), 20, Some(21))));
    assert_eq!(fenix.at(PROVIDER, TWICE_2), Some(("src/orbit.test.js".into(), 35, Some(23))), "the second of its name");
    assert_eq!(fenix.detail(PROVIDER, TEMPLATE), Default::default(), "no result made for a template");
    assert_snapshot!("vitest-run-all", fenix.tree(PROVIDER));
}

/// 4, for Jest: the same sample (a `describe('a > b')` in its own file),
/// a skip found only at its file's end.
#[test]
fn jest_a_run_of_all_gives_each_test_its_result_at_its_assertion() {
    let (mut fenix, _) = jest();
    fenix.on_recorded(recorded(JEST, "run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "node node_modules/jest/bin/jest.js --ci --watchAll=false --testLocationInResults --reporters=default --reporters=${data}/lang-kit/tests/fenix-jest.js");
    let mut states: Vec<(&str, State)> = V_RUN_ALL.iter().filter(|(k, _)| !k.starts_with("src/geo.test.ts")).cloned().collect();
    states.extend([(SYMMETRIC_JS, State::Passed), (ARROWS_JS, State::Passed)]);
    fenix.assert_states(PROVIDER, &states);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/orbit.test.js".into(), 9, Some(21))));
    assert_eq!(fenix.detail(PROVIDER, FAILS).message.as_deref(), Some("expect(received).toBe(expected) // Object.is equality"));
    assert!(fenix.detail(PROVIDER, FAILS).text.as_deref().is_some_and(|t| t.contains("Expected: 5")));
    assert_eq!(fenix.at(PROVIDER, CASE_BAD), Some(("src/orbit.test.js".into(), 19, Some(21))), "past jest-each's frame in node_modules");
    assert_eq!(fenix.detail(PROVIDER, TODO).message.as_deref(), Some("todo"));
    assert_snapshot!("jest-run-all", fenix.tree(PROVIDER));
}

/// 5: the same recordings in every order and batch size, lines cut
/// small: one tree.
#[test]
fn any_order_and_any_batches_end_the_same() {
    for (make, dir) in [(vitest as fn() -> (Fenix<Kit>, Fake), VITEST), (jest, JEST)] {
        let (mut whole, _) = make();
        whole.on_recorded(recorded(dir, "run-all"));
        whole.run_all(PROVIDER);
        let tree = whole.tree(PROVIDER);
        for (i, order) in [Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(1), Order::Shuffled(7)].into_iter().enumerate() {
            let (mut f, _) = make();
            f.order = order.clone();
            f.batch = 1 + i * 3;
            f.cut_at = if i % 2 == 0 { 64 * 1024 } else { 23 };
            f.on_recorded(recorded(dir, "run-all"));
            f.run_all(PROVIDER);
            assert_eq!(f.tree(PROVIDER), tree, "{dir}: {order:?}, {} a call", f.batch);
        }
    }
}

/// 6: a file that doesn't load is `broken` on that file, the others'
/// results stand, nothing failed for it.
#[test]
fn a_file_that_doesnt_load_is_broken_and_the_others_stand() {
    for (dir, make, broken) in [(VITEST, vitest as fn() -> (Fenix<Kit>, Fake), V_BROKEN), (JEST, jest, J_BROKEN)] {
        let (mut fenix, _) = make();
        fenix.scan(PROVIDER, "/project/src/broken.test.js", broken);
        fenix.on_recorded(recorded(dir, "broken"));
        let run = fenix.run_all(PROVIDER);
        assert_eq!(run.broken.len(), 1, "{dir}: {run:?}");
        let (id, message) = &run.broken[0];
        assert_eq!(id.as_deref(), Some("\u{1f}src/broken.test.js"));
        assert!(message.starts_with("src/broken.test.js didn't load: "), "{message}");
        assert_eq!(fenix.state(PROVIDER, "src/broken.test.js > never loads"), State::Errored);
        assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
        assert_eq!(fenix.state(PROVIDER, FAILS), State::Failed);
        if dir == JEST {
            assert_eq!(message, "src/broken.test.js didn't load: Unexpected token (3:0)");
        } else {
            assert!(message.contains("Failed to parse source"), "{message}");
        }
    }
}

/// 7: a worker that dies errors its file's tests (Vitest and Jest report
/// none of them), the other files' results stand; Jest killed in its own
/// process (one file runs in band) ends by a signal: all it hadn't ended.
#[test]
fn a_worker_that_dies_errors_its_files_tests_and_spares_the_rest() {
    for (dir, make, crash) in [(VITEST, vitest as fn() -> (Fenix<Kit>, Fake), V_CRASH), (JEST, jest, J_CRASH)] {
        let (mut fenix, _) = make();
        fenix.scan(PROVIDER, "/project/src/crash.test.js", crash);
        fenix.on_recorded(recorded(dir, "crash"));
        let run = fenix.run_all(PROVIDER);
        assert_eq!(run.crashed, [(Some("\u{1f}src/crash.test.js".into()), "its worker was killed: SIGKILL".into())], "{dir}");
        // Jest reports a test as it ends; Vitest a file's once it ends.
        let before = if dir == VITEST { State::Errored } else { State::Passed };
        assert_eq!(fenix.state(PROVIDER, "src/crash.test.js > before the crash"), before, "{dir}");
        assert_eq!(fenix.state(PROVIDER, "src/crash.test.js > kills its worker"), State::Errored, "{dir}");
        assert_eq!(fenix.detail(PROVIDER, "src/crash.test.js > after the crash").message.as_deref(), Some("its worker was killed: SIGKILL"));
        assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed, "{dir}");
        assert!(run.broken.is_empty());
    }
    let (mut fenix, _) = jest();
    fenix.scan(PROVIDER, "/project/src/crash.test.js", J_CRASH);
    fenix.on_recorded(recorded(JEST, "crash-one"));
    let run = fenix.run_file(PROVIDER, "/project/src/crash.test.js");
    assert_eq!(run.steps[0].end, StepEnd::Exited(None));
    assert_eq!(run.crashed, [(None, "Jest was killed by a signal".into())]);
    assert_eq!(fenix.state(PROVIDER, "src/crash.test.js > before the crash"), State::Passed);
    assert_eq!(fenix.detail(PROVIDER, "src/crash.test.js > kills its worker").message.as_deref(), Some("Jest was killed by a signal"));
}

/// 8: one test from its line: by its file and its names as a regex, in
/// its package's folder; the tests `-t` left out (both runners report
/// them skipped) keep their state.
#[test]
fn one_test_runs_by_its_file_and_names() {
    let (mut fenix, _) = vitest();
    fenix.on_recorded(recorded(VITEST, "run-all"));
    fenix.run_all(PROVIDER);
    fenix.on_recorded(recorded(VITEST, "run-one"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(command_text(&run.steps[0].command), "node node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs src/orbit.test.js -t ^(?:adds)$");
    assert_eq!((run.steps[0].covers.as_slice(), run.steps[0].command.cwd.as_deref()), ([ADDS.to_string()].as_slice(), Some("/project")));
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Failed, "left out by -t, not skipped");

    let (mut fenix, _) = jest();
    fenix.on_recorded(recorded(JEST, "run-one"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(command_text(&run.steps[0].command), "node node_modules/jest/bin/jest.js --ci --watchAll=false --testLocationInResults --reporters=default --reporters=${data}/lang-kit/tests/fenix-jest.js -t ^(?:adds)$ -- /src/orbit\\.test\\.js$");
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, SKIPPED), State::Unknown, "left out by -t");
}

/// 9: the failed again: one step, each failed test by its names, a
/// repeated name by its own (both run: `-t` can't tell them apart).
#[test]
fn the_failed_run_again_by_name() {
    let (mut fenix, _) = vitest();
    fenix.on_recorded(recorded(VITEST, "run-all"));
    fenix.run_all(PROVIDER);
    fenix.on_recorded(recorded(VITEST, "run-failed"));
    let run = fenix.run_failed(PROVIDER);
    assert_eq!(run.steps.len(), 1);
    let line = command_text(&run.steps[0].command);
    assert!(line.ends_with("src/orbit.test.js -t ^(?:fails on purpose|add\\(-1, -1\\) is -3|outer > inner > deep|names > twice)$"), "{line}");
    let mut covers = run.steps[0].covers.clone();
    covers.sort();
    assert_eq!(covers, [CASE_BAD, FAILS, TWICE_2, DEEP].map(String::from).into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    fenix.assert_states(PROVIDER, &[(CASE_BAD, State::Failed), (FAILS, State::Failed), (DEEP, State::Failed), (TWICE_2, State::Failed), (TWICE, State::Passed), (ADDS, State::Passed)]);
}

/// 10: a describe asked with its fill-in names its tests exactly; one with
/// none runs every name it starts; a file runs whole, in a step of its own.
#[test]
fn a_fill_in_is_named_exactly_and_none_runs_the_describes_start() {
    let (mut fenix, _) = vitest();
    fenix.on_recorded(recorded(VITEST, "run-outer-filled"));
    let run = fenix.run(PROVIDER, &[OUTER]);
    assert!(command_text(&run.steps[0].command).ends_with("src/orbit.test.js -t ^(?:outer > in describe|outer > inner > deep)$"), "{}", command_text(&run.steps[0].command));
    assert_eq!(run.steps[0].covers, [IN_DESCRIBE, DEEP, OUTER].map(String::from));
    fenix.assert_states(PROVIDER, &[(IN_DESCRIBE, State::Passed), (DEEP, State::Failed), (ADDS, State::Unknown)]);

    // No fill-in: everything under its names.
    let facts = facts(&format!("{VITEST}/orbit"));
    let mut p = Provider::new(facts);
    p.scan("/project/src/orbit.test.js", V_ORBIT);
    let outer = vec![Group::file("src/orbit.test.js"), Group::suite("outer", "src/orbit.test.js", 23)];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: outer, tests: None, known: Vec::new() }])).unwrap();
    assert!(command_text(&steps[0].command).ends_with("src/orbit.test.js -t ^(?:outer > .*)$"));
    assert_eq!(steps[0].covers.len(), 1, "the describe's group");
    assert_eq!(command_text(&steps[0].command), std::fs::read_to_string(format!("{VITEST}/run-outer.args")).unwrap().trim(), "as recorded");
    let ask = Ask::Run(Vec::new());
    assert_eq!(results(&mut p, &ask, &steps[0], &recorded(VITEST, "run-outer")), [(IN_DESCRIBE.to_string(), Outcome::Passed), (DEEP.to_string(), Outcome::Failed)], "what -t left out isn't skipped");
    let (mut fenix, _) = vitest();

    // A file: whole, and a test of another file beside it in a step of its own.
    fenix.on_recorded(recorded(VITEST, "run-file")).on_recorded(recorded(VITEST, "run-one"));
    let run = fenix.run(PROVIDER, &["\u{1f}src/geo.test.ts", ADDS]);
    assert_eq!(run.steps.iter().map(|s| command_text(&s.command)).collect::<Vec<_>>(), [
        "node node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs src/geo.test.ts",
        "node node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs src/orbit.test.js -t ^(?:adds)$",
    ]);
    fenix.assert_states(PROVIDER, &[(SYMMETRIC_TS, State::Passed), (ARROWS_TS, State::Passed), (ADDS, State::Passed)]);

    // Jest, a describe with no fill-in: its names then a space.
    let mut p = Provider::new(self::facts(&format!("{JEST}/orbit")));
    p.scan("/project/src/orbit.test.js", J_ORBIT);
    let outer = vec![Group::file("src/orbit.test.js"), Group::suite("outer", "src/orbit.test.js", 22)];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: outer, tests: None, known: Vec::new() }])).unwrap();
    assert_eq!(command_text(&steps[0].command), std::fs::read_to_string(format!("{JEST}/run-outer.args")).unwrap().trim());
    assert_eq!(results(&mut p, &Ask::Run(Vec::new()), &steps[0], &recorded(JEST, "run-outer")), [(IN_DESCRIBE.to_string(), Outcome::Passed), (DEEP.to_string(), Outcome::Failed)]);
    let (mut fenix, _) = jest();
    fenix.on_recorded(recorded(JEST, "run-file"));
    fenix.run_file(PROVIDER, "/project/src/geo.test.js");
    assert_eq!(fenix.state(PROVIDER, SYMMETRIC_JS), State::Passed);
}

/// The results a reader of `step` gives for a recording's stdout.
fn results(p: &mut Provider, ask: &Ask, step: &fenix_lang_kit::tests::Step, r: &Recorded) -> Vec<(String, Outcome)> {
    let mut reader = p.reader(ask, step);
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&r.stdout).lines() {
        reader.line(fenix_lang_kit::tests::Line { text: line, stderr: false }, &mut out);
    }
    reader.end(&r.end, &mut out);
    out.into_iter().filter_map(|x| if let Report::Ended(e) = x { Some((e.key.0, e.outcome)) } else { None }).collect()
}

/// 11: after Fenix starts again it gives no `data`: the names come from
/// the scan's (a repeated name's), else the key.
#[test]
fn without_data_the_plan_comes_from_the_keys_and_the_scan() {
    let (mut fenix, _) = jest();
    assert!(fenix.given_back(PROVIDER, ARROWS_JS).unwrap().data.is_some(), "its describe's name holds ` > `");
    fenix.forget_data(PROVIDER);
    let api::Ask::Run(records) = fenix.ask_for(PROVIDER, &[ARROWS_JS, TWICE_2]) else { panic!() };
    assert!(records.iter().all(|r| r.data.is_none()));
    let mut p = Provider::new(facts(&format!("{JEST}/orbit")));
    p.scan("/project/src/orbit.test.js", J_ORBIT);
    let ask = |key: &str| Ask::Run(vec![Asked::Test(fenix_lang_kit::tests::Found::test(key, "x"))]);
    assert!(command_text(&p.plan(&ask(ARROWS_JS)).unwrap()[0].command).contains("-t ^(?:a > b c)$"), "Jest's names joined by spaces");
    assert!(command_text(&p.plan(&ask(TWICE_2)).unwrap()[0].command).contains("-t ^(?:names twice)$"));
    let mut fresh = Provider::new(facts(&format!("{JEST}/orbit")));
    assert!(command_text(&fresh.plan(&ask(ADDS)).unwrap()[0].command).contains("-t ^(?:adds)$"), "no scan: the key's names");
}

/// 12: a template runs by its title, its placeholders as `.*`: its cases
/// under it, the template given no result of its own and none errored.
#[test]
fn a_template_runs_all_its_cases() {
    for (dir, make) in [(VITEST, vitest as fn() -> (Fenix<Kit>, Fake)), (JEST, jest)] {
        let (mut fenix, _) = make();
        fenix.on_recorded(recorded(dir, "run-template"));
        let run = fenix.run(PROVIDER, &[TEMPLATE]);
        assert!(command_text(&run.steps[0].command).contains("-t ^(?:add\\(.*, .*\\) is .*)$"), "{dir}");
        fenix.assert_states(PROVIDER, &[(CASE_OK, State::Passed), (CASE_BAD, State::Failed), (TEMPLATE, State::Failed), (ADDS, State::Unknown)]);
        assert_eq!(fenix.record(PROVIDER, CASE_OK).unwrap().parent.as_deref(), Some(TEMPLATE), "{dir}");
        assert_eq!(fenix.detail(PROVIDER, TEMPLATE), Default::default());
    }
}

fn workspace() -> (Fenix<Kit>, Fake) {
    fenix_with(facts(&format!("{WORKSPACE}/mono")), &[("packages/app/src/orbit.test.js", APP), ("packages/lib/src/orbit.test.js", LIB)])
}

const APP_ADDS: &str = "packages/app/src/orbit.test.js > adds";
const LIB_ADDS: &str = "packages/lib/src/orbit.test.js > adds";
const APP_FAILS: &str = "packages/app/src/orbit.test.js > fails here";
const LIB_FAILS: &str = "packages/lib/src/orbit.test.js > fails here";

/// 13: npm workspaces, Vitest in one package and Jest in the other,
/// hoisted to the root's `node_modules`: a step per package in its
/// folder, its group covered; `adds` in both kept apart.
#[test]
fn a_workspace_runs_a_step_per_package_in_its_folder() {
    let (mut fenix, _) = workspace();
    assert_eq!(fenix.told(PROVIDER), Some(("Vitest 5.1.0 and Jest 30.5.2".into(), None)));
    assert_eq!(fenix.record(PROVIDER, APP_ADDS).unwrap().parent.as_deref(), Some("\u{1f}packages/app (vitest)\u{1f}packages/app/src/orbit.test.js"));
    fenix.on_recorded(recorded(WORKSPACE, "app-discover")).on_recorded(recorded(WORKSPACE, "lib-discover"));
    assert!(fenix.discover(PROVIDER).complete);
    fenix.on_recorded(recorded(WORKSPACE, "app-run-all")).on_recorded(recorded(WORKSPACE, "lib-run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps.iter().map(|s| (command_text(&s.command), s.command.cwd.clone().unwrap(), s.covers.clone())).collect::<Vec<_>>(), [
        ("node ../../node_modules/vitest/vitest.mjs run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs".to_string(), "/project/packages/app".to_string(), vec!["\u{1f}packages/app (vitest)".to_string()]),
        ("node ../../node_modules/jest/bin/jest.js --ci --watchAll=false --testLocationInResults --reporters=default --reporters=${data}/lang-kit/tests/fenix-jest.js".to_string(), "/project/packages/lib".to_string(), vec!["\u{1f}packages/lib (jest)".to_string()]),
    ]);
    fenix.assert_states(PROVIDER, &[(APP_ADDS, State::Passed), (LIB_ADDS, State::Passed), (APP_FAILS, State::Failed), (LIB_FAILS, State::Failed)]);
    assert_eq!(fenix.at(PROVIDER, LIB_FAILS), Some(("packages/lib/src/orbit.test.js".into(), 6, Some(17))));
    assert_snapshot!("workspace-run-all", fenix.tree(PROVIDER));
    // `adds` of each: a step each.
    fenix.on_recorded(recorded(WORKSPACE, "app-adds")).on_recorded(recorded(WORKSPACE, "lib-adds"));
    let run = fenix.run(PROVIDER, &[APP_ADDS, LIB_ADDS]);
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.steps[1].covers, [LIB_ADDS]);
    fenix.assert_states(PROVIDER, &[(APP_ADDS, State::Passed), (LIB_ADDS, State::Passed), (LIB_FAILS, State::Failed)]);
    // A package's group: its run of all.
    fenix.on_recorded(recorded(WORKSPACE, "lib-run-all"));
    let run = fenix.run(PROVIDER, &["\u{1f}packages/lib (jest)"]);
    assert_eq!(run.steps.len(), 1);
    assert!(run.complete);
}

/// 14: 2,000 failed tests run again, in steps each under Windows'
/// command line, every one named once.
#[test]
fn two_thousand_failed_tests_run_again_under_the_limit() {
    let _heavy = heavy();
    // Four files of 500, as a project has them (one of 2,000 is timed
    // above, best of five).
    let file = |f: usize| -> String { std::iter::once("import { test, expect } from 'vitest'\n\n".to_string()).chain((0..500).map(|i| format!("test('fails on purpose number {:05}', () => {{\n  expect(1).toBe(2)\n}})\n\n", f * 500 + i))).collect() };
    let files: Vec<(String, String)> = (0..4).map(|f| (format!("src/many{f}.test.js"), file(f))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let (mut fenix, _) = fenix_with(facts(&format!("{VITEST}/orbit")), &refs);
    let result = |name: &str, state: &str| format!("FENIX-TEST {{\"kind\":\"result\",\"file\":\"/home/u/orbit/src/many{}.test.js\",\"names\":[\"{name}\"],\"line\":null,\"state\":\"{state}\",\"duration\":1,\"errors\":[],\"cwd\":\"/home/u/orbit\"}}\n", name[name.len() - 5..].parse::<usize>().unwrap() / 500);
    let all: String = (0..2000).map(|i| result(&format!("fails on purpose number {i:05}"), "failed")).collect();
    fenix.on_step(|_| true, Recorded::stdout(all).exits(1));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.failed(PROVIDER).len(), 2000);
    fenix.on_step_with(move |s| {
        let at = s.command.args.iter().position(|a| a == "-t")?;
        let re = s.command.args[at + 1].strip_prefix("^(?:")?.strip_suffix(")$")?;
        Some(Recorded::stdout(re.split('|').map(|n| result(n, "passed")).collect::<String>()))
    });
    let run = fenix.run_failed(PROVIDER);
    assert!(run.steps.len() > 1, "{} steps", run.steps.len());
    assert!(run.steps.iter().all(|s| DIRECT.fits(&s.command)));
    assert_eq!(run.steps.iter().map(|s| s.covers.len()).sum::<usize>(), 2000);
    assert!(fenix.failed(PROVIDER).is_empty());
}

/// 15: debugging a test: at once, the runner's script under js-debug,
/// with its file and its names; a workspace's hoisted script from its
/// package's folder.
#[test]
fn debugging_a_test_launches_its_runner_with_its_file_and_names() {
    let (mut fenix, _) = vitest();
    let config = fenix.launch(PROVIDER, DEEP).config.unwrap();
    assert_eq!((config.name.as_str(), config.program.as_deref(), config.cwd.as_deref()), ("Vitest: outer > inner > deep", Some("${project}/node_modules/vitest/vitest.mjs"), Some("${project}")));
    assert_eq!(config.args, ["run", "src/orbit.test.js", "-t", "^(?:outer > inner > deep)$"]);
    let attrs: fenix_lang_kit::serde_json::Value = fenix_lang_kit::serde_json::from_str(&config.attributes).unwrap();
    assert_eq!((attrs["type"].as_str(), attrs["autoAttachChildProcesses"].as_bool(), attrs["runtimeExecutable"].as_str()), (Some("pwa-node"), Some(true), Some("node")));
    let (mut fenix, _) = jest();
    let config = fenix.launch(PROVIDER, TEMPLATE).config.unwrap();
    assert_eq!(config.args, ["--runInBand", "--watchAll=false", "-t", "^(?:add\\(.*, .*\\) is .*)$", "--", "/src/orbit\\.test\\.js$"], "a template: all its cases");
    let (mut fenix, _) = workspace();
    let config = fenix.launch(PROVIDER, LIB_ADDS).config.unwrap();
    assert_eq!((config.program.as_deref(), config.cwd.as_deref()), (Some("${project}/node_modules/jest/bin/jest.js"), Some("${project}/packages/lib")));
    // Yarn Plug'n'Play: yarn runs it.
    let f = facts(&format!("{VITEST}/orbit"));
    f.borrow_mut().layout.pnp = true;
    f.borrow_mut().yarn = Some("/u/yarn".into());
    let (mut fenix, _) = fenix_with(f, &[("src/orbit.test.js", V_ORBIT)]);
    let config = fenix.launch(PROVIDER, ADDS).config.unwrap();
    let attrs: fenix_lang_kit::serde_json::Value = fenix_lang_kit::serde_json::from_str(&config.attributes).unwrap();
    assert_eq!((config.program, attrs["runtimeExecutable"].as_str(), attrs["runtimeArgs"][0].as_str()), (None, Some("/u/yarn"), Some("vitest")));
}

/// 16: what Fenix is told, and why a run can't start: a runner that isn't
/// installed, a Vitest too old for the reporter, no runner at all (none:
/// the provider hidden), Yarn Plug'n'Play's shim under `cmd.exe`'s limit.
#[test]
fn the_runner_said_and_why_it_cant_run() {
    let f = facts(&format!("{VITEST}/orbit"));
    let mut p = Provider::new(f.clone());
    assert_eq!(p.runner(), ("Vitest 5.1.0".into(), None));
    f.borrow_mut().layout.packages[0].installed = None;
    let (runner, problem) = p.runner();
    assert_eq!((runner.as_str(), problem.as_deref()), ("Vitest", Some("Vitest isn't installed: install the project's packages (Web: install the project's packages)")));
    assert_eq!(p.plan(&Ask::RunAll).unwrap_err(), problem.unwrap(), "a plan says why");
    f.borrow_mut().layout.packages[0].installed = Some(Installed { script: "node_modules/vitest/vitest.mjs".into(), version: Some("2.1.9".into()) });
    assert!(p.runner().1.unwrap().starts_with("Vitest 2.1.9 is older than 3.0.0"));
    f.borrow_mut().layout.packages.clear();
    assert_eq!(p.runner(), (String::new(), None), "nothing to run: hidden");
    // Plug'n'Play: `yarn vitest`, a .cmd on Windows, split under 8,191.
    let f = facts(&format!("{VITEST}/orbit"));
    f.borrow_mut().layout.pnp = true;
    let mut p = Provider::new(f.clone());
    assert_eq!(p.runner().1.as_deref(), Some("Yarn isn't installed: a Yarn Plug'n'Play project's tests run with `yarn vitest` or `yarn jest`"));
    f.borrow_mut().yarn = Some("yarn".into());
    assert_eq!(p.runner(), ("Vitest 5.1.0 (Yarn Plug'n'Play)".into(), None));
    let tests: Vec<Asked> = (0..2000).map(|i| Asked::Test(fenix_lang_kit::tests::Found::test(format!("src/orbit.test.js > fails on purpose number {i:05}"), "x"))).collect();
    let steps = p.plan(&Ask::Run(tests)).unwrap();
    assert!(steps.len() > 4 && steps.iter().all(|s| fenix_lang_kit::tests::BATCH_FILE.fits(&s.command) && s.command.program == "yarn" && s.command.args[0] == "vitest"), "{} steps", steps.len());
    assert_eq!(steps.iter().map(|s| s.covers.len()).sum::<usize>(), 2000);
}

/// 17: a stop mid-step keeps what ended; Windows' paths in a reporter's
/// lines and stacks, by hand.
#[test]
fn a_stop_keeps_what_ended_and_windows_paths_are_the_projects() {
    let (mut fenix, _) = vitest();
    fenix.on_step(|_| true, recorded(VITEST, "run-all").stopped_after(25));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps[0].end, StepEnd::Stopped);
    assert_eq!(fenix.state(PROVIDER, SYMMETRIC_TS), State::Unknown);
    assert!(fenix.ids(PROVIDER).iter().any(|id| fenix.state(PROVIDER, id) == State::Passed) || fenix.state(PROVIDER, ADDS) == State::Unknown);

    let (mut fenix, _) = jest();
    let line = r#"FENIX-TEST {"kind":"result","file":"C:\\src\\orbit\\src\\orbit.test.js","names":["fails on purpose"],"line":7,"state":"failed","duration":4,"failures":["Error: expect(received).toBe(expected)\n    at Object.toBe (C:\\src\\orbit\\src\\orbit.test.js:9:21)"],"cwd":"C:\\src\\orbit"}"#;
    fenix.on_step(|_| true, Recorded::stdout(format!("{line}\n")).exits(1));
    fenix.run(PROVIDER, &[FAILS]);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Failed);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/orbit.test.js".into(), 9, Some(21))));
}

/// A step that printed no reporter line and failed (a config that
/// doesn't load): `broken` for the whole step, with the runner's error.
#[test]
fn a_runner_that_runs_nothing_is_broken() {
    let (mut fenix, _) = vitest();
    fenix.on_step(|_| true, Recorded::stdout("").and_stderr("failed to load config from /home/u/orbit/vitest.config.ts\n\nError: Cannot find module 'missing-plugin'\n").exits(1));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken, [(None, "Vitest ran nothing: Error: Cannot find module 'missing-plugin'".into())]);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
}

