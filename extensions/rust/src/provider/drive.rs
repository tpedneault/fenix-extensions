//! The provider through the test kit's driver, `fenix_test_kit::tests::
//! Fenix`, which plays Fenix's side as strictly as Fenix, on what cargo
//! 1.100 and cargo-nextest 0.9.151 printed for the sample projects in
//! `src/samples/tests/cargo/orbit` and `.../ws` (recorded as
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
use crate::project::tests::ok;
use crate::project::Metadata;

const CARGO: &str = "src/samples/tests/cargo";
const NEXTEST: &str = "src/samples/tests/nextest";

const LIB: &str = include_str!("../samples/tests/cargo/orbit/src/lib.rs");
const GEO: &str = include_str!("../samples/tests/cargo/orbit/src/geo/mod.rs");
const IT: &str = include_str!("../samples/tests/cargo/orbit/tests/it.rs");
const CRASH: &str = include_str!("../samples/tests/cargo/orbit/tests/crash.rs");
const CUSTOM: &str = include_str!("../samples/tests/cargo/orbit/tests/custom.rs");
const BROKEN: &str = include_str!("../samples/tests/cargo/orbit/lib-broken.rs.txt");

const ADDS: &str = "orbit/lib tests::adds";
const FAILS: &str = "orbit/lib tests::fails_on_purpose";
const IGNORED: &str = "orbit/lib tests::ignored_one";
const PANICS: &str = "orbit/lib tests::panics_as_it_should";
const NEGATIVE: &str = "orbit/lib tests::case_negative";
const SMALL: &str = "orbit/lib tests::case_small";
const DEEP: &str = "orbit/lib tests::nested::deep";
const UNWRAPS: &str = "orbit/lib geo::tests::unwraps_none";
const SYMMETRIC: &str = "orbit/lib geo::tests::dist_is_symmetric";
const IT_ADDS: &str = "orbit/test:it integration_adds";
const IT_FAILS: &str = "orbit/test:it integration_fails";
const DOC: &str = "orbit/doc src/lib.rs - add (line 6)";
/// `orbit (lib)` › `src/lib.rs` › `tests`.
const TESTS: &str = "\u{1f}orbit (lib)\u{1f}src/lib.rs\u{1f}tests";
const GEO_FILE: &str = "\u{1f}orbit (lib)\u{1f}src/geo/mod.rs";

fn facts(metadata: &str) -> SharedFacts {
    let m = Metadata::parse(&ok(&std::fs::read_to_string(format!("{CARGO}/{metadata}")).unwrap())).unwrap();
    Rc::new(RefCell::new(Facts { cargo_project: true, cargo: Some("cargo".into()), metadata: Some(m), project_dir: Some("/project".into()), root: format!("{CARGO}/orbit"), generation: 1, ..Facts::default() }))
}

/// Fenix with the pack's provider, the sample's files scanned.
fn fenix_with(facts: SharedFacts, files: &[(&str, &str)]) -> (Fenix<Kit>, Fake) {
    let fake = Fake::new();
    let kit = Kit::with_host("rust", fake.host()).tests(Box::new(Provider::new(facts)));
    let mut fenix = Fenix::new(kit);
    fenix.manifest("extension.toml").with_fake(fake.clone());
    for (path, text) in files {
        fenix.scan(PROVIDER, &format!("/project/{path}"), text);
    }
    (fenix, fake)
}

fn orbit() -> (Fenix<Kit>, Fake) {
    fenix_with(facts("metadata-orbit.json"), &[("src/lib.rs", LIB), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT)])
}

fn recorded(dir: &str, name: &str) -> Recorded {
    Recorded::from(format!("{dir}/{name}"))
}

/// What a run of everything in the sample gives, cargo's way.
const RUN_ALL: &[(&str, State)] = &[
    (ADDS, State::Passed),
    (FAILS, State::Failed),
    (IGNORED, State::Skipped),
    (PANICS, State::Passed),
    (NEGATIVE, State::Failed),
    (SMALL, State::Passed),
    (DEEP, State::Passed),
    (UNWRAPS, State::Failed),
    (SYMMETRIC, State::Passed),
    (IT_ADDS, State::Passed),
    (IT_FAILS, State::Failed),
    (DOC, State::Passed),
];

/// §6.1's done when 1: the scan, at the tests' lines, under their target,
/// file and module; `#[ignore = "slow"]` skipped "slow".
#[test]
fn a_scan_gives_each_test_at_its_line_under_its_target_file_and_module() {
    let (fenix, fake) = orbit();
    let adds = fenix.record(PROVIDER, ADDS).unwrap();
    assert_eq!((adds.line, adds.end_line, adds.parent.as_deref(), adds.path.as_deref()), (Some(17), Some(19), Some(TESTS), Some("src/lib.rs")), "lines from 0 in Fenix: line 18");
    assert_eq!(fenix.record(PROVIDER, IGNORED).unwrap().skip.as_deref(), Some("slow"));
    let suite = fenix.record(PROVIDER, TESTS).unwrap();
    assert_eq!((suite.kind, suite.line, suite.end_line), (Kind::Suite, Some(13), Some(51)));
    assert_eq!(fenix.record(PROVIDER, "\u{1f}orbit (lib)").unwrap().kind, Kind::Group);
    assert_eq!(fenix.record(PROVIDER, IT_FAILS).unwrap().parent.as_deref(), Some("\u{1f}orbit (test it)\u{1f}tests/it.rs"));
    assert!(fenix.record(PROVIDER, NEGATIVE).is_none(), "made by a macro: no fn to scan");
    assert_eq!(fenix.told(PROVIDER), Some(("cargo test".into(), None)));
    assert!(fake.take().iter().any(|c| c == "test-provider rust: cargo test"));
    assert_snapshot!("cargo-scan", fenix.tree(PROVIDER));
}

/// §6.1's done when 2: the listing adds what a macro made under its
/// module with no line, and the doc test at its line; it keeps the
/// scan's lines.
#[test]
fn a_listing_adds_what_a_macro_made_and_the_doc_tests() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "list"));
    let run = fenix.discover(PROVIDER);
    assert!(run.complete, "{run:?}");
    assert_eq!(run.steps[0].label, "building the tests to list them");
    let negative = fenix.record(PROVIDER, NEGATIVE).unwrap();
    assert_eq!((negative.parent.as_deref(), negative.line), (Some(TESTS), None), "under tests, no line");
    let doc = fenix.record(PROVIDER, DOC).unwrap();
    assert_eq!((doc.parent.as_deref(), doc.line, doc.label.as_str()), (Some("\u{1f}orbit (doc)\u{1f}src/lib.rs"), Some(5), "add (line 6)"));
    assert_eq!(fenix.record(PROVIDER, ADDS).unwrap().line, Some(17), "the scan's line stays");
    assert_snapshot!("cargo-listed", fenix.tree(PROVIDER));
}

/// §6.1's done when 3: a run of all, every outcome as cargo said it; a
/// failure at its assertion, with its message, its text and what it
/// printed; a macro-made test found in the run, at its macro's call.
#[test]
fn a_run_of_all_gives_each_test_its_result_at_its_assertion() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --workspace --no-fail-fast --message-format=json");
    assert!(run.lists_after, "a run of all built what the listing builds");
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/lib.rs".into(), 25, Some(9))));
    let d = fenix.detail(PROVIDER, FAILS);
    assert_eq!(d.message.as_deref(), Some("assertion `left == right` failed: two and two"));
    assert!(d.text.as_deref().is_some_and(|t| t.starts_with("some output before the failure") && t.contains("right: 5")), "{:?}", d.text);
    assert_eq!(fenix.model(PROVIDER).nodes[FAILS].printed, "some output before the failure");
    assert_eq!(fenix.at(PROVIDER, NEGATIVE), Some(("src/lib.rs".into(), 43, Some(5))), "a macro's test fails at the macro's call");
    assert_eq!(fenix.record(PROVIDER, NEGATIVE).unwrap().parent.as_deref(), Some(TESTS), "found in the run, under its module");
    assert_eq!(fenix.detail(PROVIDER, IGNORED).message.as_deref(), Some("slow"));
    assert_eq!(fenix.at(PROVIDER, IT_FAILS), Some(("tests/it.rs".into(), 8, Some(5))));
    assert!(fenix.detail(PROVIDER, ADDS).duration_ms.is_none(), "libtest gives no time per test");
    assert_snapshot!("cargo-run-all", fenix.tree(PROVIDER));
}

/// §6.1's done when 4: stdout and stderr shuffled per tick, both whole
/// before the other, in calls of a line or a few, lines cut small: the
/// same results. The binaries are told apart by pairing each stream's own
/// order, never the order between them.
#[test]
fn the_streams_in_any_order_and_any_batches_end_the_same() {
    let (mut whole, _) = orbit();
    whole.on_recorded(recorded(CARGO, "run-all"));
    whole.run_all(PROVIDER);
    let tree = whole.tree(PROVIDER);
    let orders = [Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(1), Order::Shuffled(2), Order::Shuffled(3), Order::Shuffled(42)];
    for (i, order) in orders.into_iter().enumerate() {
        let (mut f, _) = orbit();
        f.order = order.clone();
        f.batch = 1 + i * 3;
        f.cut_at = if i % 2 == 0 { 64 * 1024 } else { 23 };
        f.on_recorded(recorded(CARGO, "run-all"));
        f.run_all(PROVIDER);
        assert_eq!(f.tree(PROVIDER), tree, "{order:?}, {} a call", f.batch);
    }
    // --jobs and --test-threads, and colours forced: the same.
    for name in ["run-all-jobs", "run-all-color"] {
        let (mut f, _) = orbit();
        f.on_step(|_| true, recorded(CARGO, name));
        f.run_all(PROVIDER);
        f.assert_states(PROVIDER, RUN_ALL);
    }
}

/// libtest's lines without cargo's JSON (cargo 1.97, `cargo test --lib`):
/// the binary told from its header alone.
#[test]
fn libtests_lines_without_cargos_json_are_read_by_their_header() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded(CARGO, "run-human-old"));
    fenix.run_all(PROVIDER);
    fenix.assert_states(PROVIDER, &RUN_ALL[..9]);
}

/// §6.1's done when 3: a build that fails is `broken` on the target that
/// didn't build, with its first error and its place; nothing failed.
#[test]
fn a_build_that_fails_is_broken_not_a_test_failure() {
    let (mut fenix, _) = fenix_with(facts("metadata-orbit.json"), &[("src/lib.rs", BROKEN), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT)]);
    fenix.on_recorded(recorded(CARGO, "broken"));
    let run = fenix.run_all(PROVIDER);
    let message = "orbit (lib) didn't build: src/lib.rs:19 mismatched types";
    assert_eq!(run.broken, [(Some("\u{1f}orbit (lib)".into()), message.into()), (None, message.into())], "the target, then the rest of the step: cargo ran nothing");
    assert!(fenix.failed(PROVIDER).iter().all(|id| fenix.state(PROVIDER, id) == State::Errored), "no test failed");
    assert_eq!(fenix.detail(PROVIDER, ADDS).message.as_deref(), Some(message));
    assert_eq!(fenix.state(PROVIDER, IT_ADDS), State::Errored);
    // A workspace whose one package doesn't build: nothing ran in either.
    let (mut ws, _) = workspace();
    ws.on_recorded(recorded(CARGO, "ws-broken"));
    let run = ws.run_all(PROVIDER);
    assert_eq!(run.broken[0].0.as_deref(), Some("\u{1f}orbit-core (lib)"));
    assert!(run.broken[0].1.starts_with("orbit-core (lib) didn't build: orbit-core/src/lib.rs:9 "), "{:?}", run.broken);
    assert_eq!(ws.state(PROVIDER, CLI_WORKS), State::Errored);
}

/// A binary that aborts errors its own tests, and the next binaries'
/// results stand (`--no-fail-fast`); a `harness = false` binary that
/// panics in its `main` prints no segment, and the pairing goes past it.
#[test]
fn a_crash_errors_its_own_binarys_tests_and_spares_the_next() {
    let files = [("src/lib.rs", LIB), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT), ("tests/crash.rs", CRASH), ("tests/custom.rs", CUSTOM)];
    let mut tree = None;
    for order in [Order::Recorded, Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(5), Order::Shuffled(6)] {
        let (mut fenix, _) = fenix_with(facts("metadata-orbit-crash.json"), &files);
        fenix.order = order.clone();
        fenix.on_recorded(recorded(CARGO, "crash"));
        let run = fenix.run_all(PROVIDER);
        let mut crashed = run.crashed.clone();
        crashed.sort();
        assert_eq!(
            crashed,
            [
                (Some("\u{1f}orbit (test crash)".into()), "its binary crashed: SIGABRT".into()),
                (Some("\u{1f}orbit (test custom)".into()), "its test binary panicked outside any test: the fixtures folder is missing (tests/custom.rs:4:5)".into()),
            ],
            "{order:?}"
        );
        fenix.assert_states(PROVIDER, &[("orbit/test:crash before_the_crash", State::Passed), ("orbit/test:crash aborts", State::Errored), (IT_ADDS, State::Passed), (IT_FAILS, State::Failed), (DOC, State::Passed), (FAILS, State::Failed)]);
        assert_eq!(fenix.detail(PROVIDER, "orbit/test:crash aborts").message.as_deref(), Some("its binary crashed: SIGABRT"));
        match &tree {
            None => tree = Some(fenix.tree(PROVIDER)),
            Some(t) => assert_eq!(&fenix.tree(PROVIDER), t, "{order:?}"),
        }
    }
}

/// §6.1's done when 7 on the driver: one test run from its line, by its
/// exact name in its target only.
#[test]
fn one_test_runs_by_its_exact_name() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "run-one"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --lib -- --exact tests::adds");
    assert_eq!((run.steps[0].covers.as_slice(), run.steps[0].command.cwd.as_deref()), ([ADDS.to_string()].as_slice(), Some("/project")));
    assert!(!run.lists_after, "it built one target: the listing would build the rest");
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Unknown);
}

/// The failed again: one step for the package, every failed test named
/// exactly, across its targets.
#[test]
fn the_failed_run_again_by_name_across_targets() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "run-all"));
    fenix.run_all(PROVIDER);
    fenix.on_step(|s| command_text(&s.command).contains("--exact"), recorded(CARGO, "run-failed"));
    let run = fenix.run_failed(PROVIDER);
    assert_eq!(run.steps.len(), 1);
    let line = command_text(&run.steps[0].command);
    assert!(line.starts_with("cargo test --no-fail-fast --message-format=json -p orbit --lib --test it -- --exact "), "{line}");
    let mut covers = run.steps[0].covers.clone();
    covers.sort();
    assert_eq!(covers, [UNWRAPS, FAILS, NEGATIVE, IT_FAILS].map(String::from).iter().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    fenix.assert_states(PROVIDER, &[(UNWRAPS, State::Failed), (NEGATIVE, State::Failed), (FAILS, State::Failed), (IT_FAILS, State::Failed), (ADDS, State::Passed)]);
}

/// A file asked with its fill-in names its tests exactly; a suite asked
/// with none runs its module path, a substring (libtest's filter); a
/// target runs by its flag.
#[test]
fn a_fill_in_is_named_exactly_and_none_runs_the_runners_own_filter() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|s| command_text(&s.command).contains("-- --exact"), recorded(CARGO, "run-module"));
    let run = fenix.run(PROVIDER, &[GEO_FILE]);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --lib -- --exact geo::tests::dist_is_symmetric geo::tests::unwraps_none");
    assert_eq!(run.steps[0].covers, [SYMMETRIC, UNWRAPS, GEO_FILE].map(String::from));
    fenix.assert_states(PROVIDER, &[(SYMMETRIC, State::Passed), (UNWRAPS, State::Failed)]);

    // No fill-in (Fenix had none to send): the module's own filter.
    let facts = facts("metadata-orbit.json");
    let mut p = Provider::new(facts);
    p.scan("/project/src/geo/mod.rs", GEO);
    p.scan("/project/src/lib.rs", LIB);
    let geo = vec![Group::package("orbit (lib)"), Group::file("src/geo/mod.rs"), Group::suite("tests", "src/geo/mod.rs", 6)];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: geo, tests: None, known: Vec::new() }])).unwrap();
    assert_eq!(command_text(&steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --lib -- geo::tests::", "which also runs other::geo::tests:: -- a substring");
    let lib = vec![Group::package("orbit (lib)"), Group::file("src/lib.rs")];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: lib, tests: None, known: Vec::new() }])).unwrap();
    assert_eq!(command_text(&steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --lib", "the target's root: all of it");
    let it = vec![Group::package("orbit (test it)")];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: it, tests: None, known: Vec::new() }])).unwrap();
    assert_eq!((command_text(&steps[0].command).as_str(), steps[0].covers.len()), ("cargo test --no-fail-fast --message-format=json -p orbit --test it", 1));
}

/// Fenix §19 finding 2: after Fenix starts again it gives no `data`; the
/// provider plans from its keys alone (it never gives `data`).
#[test]
fn without_data_the_plan_comes_from_the_keys() {
    let (mut fenix, _) = orbit();
    fenix.forget_data(PROVIDER);
    let api::Ask::Run(records) = fenix.ask_for(PROVIDER, &[ADDS, IT_FAILS]) else { panic!() };
    assert!(records.iter().all(|r| r.data.is_none()));
    fenix.on_step(|_| true, recorded(CARGO, "run-failed"));
    let run = fenix.run(PROVIDER, &[ADDS, IT_FAILS]);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --lib --test it -- --exact tests::adds integration_fails");
}

/// A doc test: listed and run by its name (a substring: `--exact` matches
/// no doc test), never debugged.
#[test]
fn a_doc_test_runs_by_its_name_and_isnt_debugged() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "list"));
    fenix.discover(PROVIDER);
    fenix.on_recorded(recorded(CARGO, "doc-one"));
    let run = fenix.run(PROVIDER, &[DOC]);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit --doc -- src/lib.rs - add (line 6)");
    assert_eq!(fenix.state(PROVIDER, DOC), State::Passed);
    let launched = fenix.launch(PROVIDER, DOC);
    assert!(launched.config.unwrap_err().starts_with("a doc test can't be debugged on its own"));
}

const CORE_LIB: &str = include_str!("../samples/tests/cargo/ws/orbit-core/src/lib.rs");
const CORE_IT: &str = include_str!("../samples/tests/cargo/ws/orbit-core/tests/it_works.rs");
const CLI: &str = include_str!("../samples/tests/cargo/ws/orbit-cli/src/main.rs");
const CLI_EMPTY: &str = include_str!("../samples/tests/cargo/ws/orbit-cli/tests/empty.rs");
const CORE_WORKS: &str = "orbit-core/lib tests::it_works";
const CORE_IT_WORKS: &str = "orbit-core/test:it_works it_works";
const CLI_WORKS: &str = "orbit-cli/bin:orbit-cli tests::it_works";
const SUMS: &str = "orbit-cli/bin:orbit-cli tests::sums";
const CASED: &str = "orbit-cli/bin:orbit-cli tests::adds";

fn workspace() -> (Fenix<Kit>, Fake) {
    let facts = facts("metadata-ws.json");
    facts.borrow_mut().root = format!("{CARGO}/ws");
    fenix_with(facts, &[("orbit-core/src/lib.rs", CORE_LIB), ("orbit-core/tests/it_works.rs", CORE_IT), ("orbit-cli/src/main.rs", CLI), ("orbit-cli/tests/empty.rs", CLI_EMPTY)])
}

/// A workspace of two packages: one step, each binary's lines told apart
/// by cargo's headers -- `it_works` in three targets, three results; a
/// binary with no tests; rstest's and test-case's cases under their
/// templates; a doc test of a member, at its line.
#[test]
fn a_workspace_in_one_step_each_binary_its_own() {
    let (mut fenix, _) = workspace();
    assert!(fenix.record(PROVIDER, SUMS).unwrap().template && fenix.record(PROVIDER, CASED).unwrap().template, "#[rstest] with cases, #[test_case]");
    fenix.on_recorded(recorded(CARGO, "ws-list"));
    fenix.discover(PROVIDER);
    let case = fenix.record(PROVIDER, "orbit-cli/bin:orbit-cli tests::sums::case_2_negative").unwrap();
    assert_eq!((case.kind, case.parent.as_deref(), case.label.as_str()), (Kind::Case, Some(SUMS), "case_2_negative"), "a listed case under its template");
    let doc = fenix.record(PROVIDER, "orbit-core/doc orbit-core/src/lib.rs - double (line 3)").unwrap();
    assert_eq!((doc.path.as_deref(), doc.line), (Some("orbit-core/src/lib.rs"), Some(2)));
    fenix.on_recorded(recorded(CARGO, "ws-run-all"));
    fenix.run_all(PROVIDER);
    fenix.assert_states(
        PROVIDER,
        &[
            (CORE_WORKS, State::Passed),
            (CORE_IT_WORKS, State::Failed),
            (CLI_WORKS, State::Passed),
            ("orbit-cli/bin:orbit-cli tests::sums::case_1", State::Passed),
            ("orbit-cli/bin:orbit-cli tests::sums::case_2_negative", State::Failed),
            ("orbit-cli/bin:orbit-cli tests::adds::test_case_1_small", State::Passed),
            ("orbit-cli/bin:orbit-cli tests::adds::test_case_2_wrong", State::Failed),
            (SUMS, State::Failed),
            ("orbit-core/doc orbit-core/src/lib.rs - double (line 3)", State::Passed),
        ],
    );
    assert_eq!(fenix.at(PROVIDER, CORE_IT_WORKS), Some(("orbit-core/tests/it_works.rs".into(), 3, Some(5))));
    assert_eq!(fenix.detail(PROVIDER, SUMS), Default::default(), "no result made for a template");
    assert_snapshot!("cargo-ws-run-all", fenix.tree(PROVIDER));
}

/// An rstest template runs by its path, a substring: all its cases.
#[test]
fn a_template_runs_all_its_cases() {
    let (mut fenix, _) = workspace();
    fenix.on_recorded(recorded(CARGO, "ws-cases"));
    let run = fenix.run(PROVIDER, &[SUMS, CASED]);
    assert_eq!(command_text(&run.steps[0].command), "cargo test --no-fail-fast --message-format=json -p orbit-cli --bin orbit-cli -- tests::sums tests::adds");
    fenix.assert_states(PROVIDER, &[("orbit-cli/bin:orbit-cli tests::sums::case_1", State::Passed), ("orbit-cli/bin:orbit-cli tests::adds::test_case_2_wrong", State::Failed), (SUMS, State::Failed), (CASED, State::Failed)]);
    assert_eq!(fenix.record(PROVIDER, "orbit-cli/bin:orbit-cli tests::adds::test_case_1_small").unwrap().parent.as_deref(), Some(CASED));
    // The run covered the two templates: nothing of them left without a result.
    assert!(!fenix.failed(PROVIDER).iter().any(|id| fenix.detail(PROVIDER, id).message.as_deref() == Some(NO_RESULT)));
}

/// §6.1's done when 5: 2,000 failed tests run again in steps each under
/// Windows' command line, every one named once.
#[test]
fn two_thousand_failed_tests_run_again_under_the_limit() {
    let module = |f: usize| -> String { std::iter::once("#[cfg(test)]\nmod tests {\n".to_string()).chain((0..500).map(|i| format!("    #[test]\n    fn fails_on_purpose_number_{f}{i:04}() {{\n        panic!(\"no\");\n    }}\n\n"))).chain(std::iter::once("}\n".to_string())).collect() };
    let files: Vec<(String, String)> = (0..4).map(|f| (format!("src/many{f}.rs"), module(f))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let (mut fenix, _) = fenix_with(facts("metadata-orbit.json"), &refs);
    let mut out = String::from("\nrunning 2000 tests\n");
    out.extend((0..2000).map(|n| format!("test many{}::tests::fails_on_purpose_number_{}{:04} ... FAILED\n", n / 500, n / 500, n % 500)));
    out.push_str("\ntest result: FAILED. 0 passed; 2000 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s\n");
    let header = "     Running unittests src/lib.rs (target/debug/deps/orbit-0123456789abcdef)\n";
    fenix.on_step(|_| true, Recorded::stdout(out).and_stderr(header).exits(101));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.failed(PROVIDER).len(), 2000);
    fenix.on_step_with(move |s| {
        let at = s.command.args.iter().position(|a| a == "--exact")?;
        let names = &s.command.args[at + 1..];
        let lines: String = names.iter().map(|n| format!("test {n} ... ok\n")).collect();
        Some(Recorded::stdout(format!("running {} tests\n{lines}test result: ok.\n", names.len())).and_stderr(header))
    });
    let run = fenix.run_failed(PROVIDER);
    assert!(run.steps.len() > 1, "{} steps", run.steps.len());
    assert!(run.steps.iter().all(|s| DIRECT.fits(&s.command)));
    assert_eq!(run.steps.iter().map(|s| s.covers.len()).sum::<usize>(), 2000);
    assert!(fenix.failed(PROVIDER).is_empty());
}

/// A file of 2,000 tests is scanned within the driver's budget for a call
/// (5 ms in a release build, 50 ms in a debug one): the provider's scan
/// and lang-kit's adapter together, natively. The best of five, so a busy
/// machine doesn't fail it; every other call the driver times as it goes.
#[test]
fn a_file_of_two_thousand_tests_is_scanned_within_a_calls_budget() {
    let text: String = std::iter::once("#[cfg(test)]\nmod tests {\n".to_string()).chain((0..2000).map(|i| format!("    #[test]\n    fn fails_on_purpose_number_{i:05}() {{\n        assert_eq!(1 + 1, 2, \"{{}}\");\n    }}\n\n"))).chain(std::iter::once("}\n".to_string())).collect();
    let budget = if cfg!(debug_assertions) { std::time::Duration::from_millis(50) } else { std::time::Duration::from_millis(5) };
    let best = (0..5)
        .map(|_| {
            let mut kit = Kit::with_host("rust", Fake::new().host()).tests(Box::new(Provider::new(facts("metadata-orbit.json"))));
            let start = std::time::Instant::now();
            let found = kit.tests_scan(PROVIDER, "/project/src/lib.rs", &text);
            assert_eq!(found.len(), 2003, "the target, the file, the module and 2,000 tests");
            start.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < budget, "{best:?}");
}

/// §6.1's done when 6: debugging builds the test's binary first, then
/// launches it with `<path> --exact --nocapture` from its package's folder.
#[test]
fn debugging_a_test_builds_its_binary_then_launches_it() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded(CARGO, "no-run"));
    let launched = fenix.launch(PROVIDER, ADDS);
    let built = launched.built.unwrap();
    assert_eq!(command_text(&built.steps[0].command), "cargo test --no-run --message-format=json -p orbit --lib");
    let config = launched.config.unwrap();
    assert_eq!(config.program.as_deref(), Some("/project/target/debug/build/orbit/b1fa5706f773b725/out/orbit-b1fa5706f773b725"));
    assert_eq!(config.args, ["tests::adds", "--exact", "--nocapture"]);
    assert_eq!((config.name.as_str(), config.cwd.as_deref()), ("Rust: tests::adds", Some("/project")));
    assert_eq!(config.env, [("CARGO_MANIFEST_DIR".to_string(), "/project".to_string())]);
    assert!(config.attributes.contains("sourceLanguages"));
    // A build that fails: no launch, and why.
    fenix.on_step(|_| true, recorded(CARGO, "broken"));
    assert_eq!(fenix.launch(PROVIDER, ADDS).config.unwrap_err(), "orbit (lib) didn't build: src/lib.rs:19 mismatched types");
}

fn nextest(facts: SharedFacts, files: &[(&str, &str)]) -> (Fenix<Kit>, Fake) {
    facts.borrow_mut().nextest = Some("0.9.151".into());
    fenix_with(facts, files)
}

/// With cargo-nextest: its libtest JSON, each test with its time; the doc
/// tests by cargo; Fenix told the runner.
#[test]
fn nextest_streams_each_result_with_its_time() {
    let (mut fenix, _) = nextest(facts("metadata-orbit.json"), &[("src/lib.rs", LIB), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT)]);
    assert_eq!(fenix.told(PROVIDER), Some(("cargo nextest 0.9.151".into(), None)));
    fenix.on_recorded(recorded(NEXTEST, "run-all"));
    fenix.on_recorded(recorded(NEXTEST, "doc"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps.iter().map(|s| command_text(&s.command)).collect::<Vec<_>>(), ["cargo nextest run --workspace --no-fail-fast --message-format libtest-json-plus", "cargo test --workspace --doc --no-fail-fast --message-format=json"]);
    assert_eq!(run.steps[0].command.env, [("NEXTEST_EXPERIMENTAL_LIBTEST_JSON".to_string(), "1".to_string())]);
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert!(fenix.detail(PROVIDER, ADDS).duration_ms.is_some_and(|ms| ms > 0.0));
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/lib.rs".into(), 25, Some(9))));
    assert_eq!(fenix.detail(PROVIDER, FAILS).message.as_deref(), Some("assertion `left == right` failed: two and two"));
    assert_snapshot!("nextest-run-all", fenix.tree(PROVIDER));
    // The same shuffled.
    let tree = fenix.tree(PROVIDER);
    let (mut again, _) = nextest(facts("metadata-orbit.json"), &[("src/lib.rs", LIB), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT)]);
    again.order = Order::Shuffled(9);
    again.batch = 2;
    again.on_recorded(recorded(NEXTEST, "run-all")).on_recorded(recorded(NEXTEST, "doc"));
    again.run_all(PROVIDER);
    assert_eq!(again.tree(PROVIDER), tree);
}

/// nextest by exact filters, each in its binary; an ignored test it
/// reports though not asked is left alone; a retry's last attempt wins;
/// a test that aborts says its signal.
#[test]
fn nextest_filters_retries_and_signals() {
    let files = [("src/lib.rs", LIB), ("src/geo/mod.rs", GEO), ("tests/it.rs", IT), ("tests/crash.rs", CRASH)];
    let (mut fenix, _) = nextest(facts("metadata-orbit-crash.json"), &files);
    fenix.on_step(|_| true, recorded(NEXTEST, "run-set"));
    let run = fenix.run(PROVIDER, &[ADDS, UNWRAPS]);
    assert_eq!(command_text(&run.steps[0].command), "cargo nextest run --no-fail-fast --message-format libtest-json-plus -p orbit --lib -E (binary_id(orbit) & test(=geo::tests::unwraps_none)) | (binary_id(orbit) & test(=tests::adds))");
    fenix.assert_states(PROVIDER, &[(ADDS, State::Passed), (UNWRAPS, State::Failed), (IGNORED, State::Unknown)]);

    fenix.on_step(|_| true, recorded(NEXTEST, "retries"));
    fenix.run(PROVIDER, &[ADDS, FAILS]);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Failed);
    assert!(fenix.detail(PROVIDER, FAILS).text.is_some(), "the last attempt's");

    fenix.on_step(|_| true, recorded(NEXTEST, "abort"));
    fenix.run(PROVIDER, &["\u{1f}orbit (test crash)"]);
    assert_eq!(fenix.state(PROVIDER, "orbit/test:crash aborts"), State::Failed);
    assert_eq!(fenix.detail(PROVIDER, "orbit/test:crash aborts").message.as_deref(), Some("it crashed: SIGABRT"));
}

/// nextest's build failures and a binary that can't list its tests:
/// `broken`, never failures.
#[test]
fn nextest_build_failures_are_broken() {
    let (mut fenix, _) = nextest(facts("metadata-orbit.json"), &[("src/lib.rs", BROKEN), ("tests/it.rs", IT)]);
    fenix.on_recorded(recorded(NEXTEST, "broken")).on_recorded(recorded(NEXTEST, "doc"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken[0], (Some("\u{1f}orbit (lib)".into()), "orbit (lib) didn't build: src/lib.rs:19 mismatched types".into()));
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
    let files = [("src/lib.rs", LIB), ("tests/custom.rs", CUSTOM)];
    let (mut fenix, _) = nextest(facts("metadata-orbit-crash.json"), &files);
    fenix.on_recorded(recorded(NEXTEST, "crash")).on_step(|s| s.command.args.contains(&"--doc".to_string()), recorded(NEXTEST, "doc"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken[0], (Some("\u{1f}orbit (test custom)".into()), "orbit (test custom) couldn't list its tests: the fixtures folder is missing".into()));
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
}

/// Windows' paths in a panic, by hand.
#[test]
fn a_windows_panic_place_is_a_path_below_the_project() {
    let (mut fenix, _) = orbit();
    let out = "\nrunning 1 test\ntest tests::fails_on_purpose ... FAILED\n\nfailures:\n\n---- tests::fails_on_purpose stdout ----\n\nthread 'tests::fails_on_purpose' (12) panicked at src\\lib.rs:25:9:\nassertion failed\n\nfailures:\n    tests::fails_on_purpose\n\ntest result: FAILED. 0 passed; 1 failed\n";
    fenix.on_step(|_| true, Recorded::stdout(out).and_stderr("     Running unittests src\\lib.rs (target\\debug\\deps\\orbit-1.exe)\n").exits(101));
    fenix.run(PROVIDER, &[FAILS]);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("src/lib.rs".into(), 25, Some(9))));
}

/// A stop mid-run: what ended keeps its result, the rest goes back.
#[test]
fn a_stop_keeps_what_ended() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded(CARGO, "run-all").stopped_after(60));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps[0].end, StepEnd::Stopped);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, IT_ADDS), State::Unknown);
}
