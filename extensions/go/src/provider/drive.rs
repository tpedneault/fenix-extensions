//! The provider through the test kit's driver, `fenix_test_kit::tests::
//! Fenix`, which plays Fenix's side as strictly as Fenix, on what Go
//! 1.24.7 printed for the sample projects in `src/samples/tests/go/orbit`
//! and `.../ws` (recorded as `plans/test-providers-recipe.md` says, see
//! the README beside them). Each `Recorded` is a command's stdout and
//! stderr as Fenix keeps them apart, how they came, and its exit; the
//! driver gives them to the provider in batches, cut, shuffled, and times
//! every call.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_lang_kit::tests::{api, Ask, Asked, Group, Kind, StepEnd, TestRunner, DIRECT};
use fenix_lang_kit::Kit;
use fenix_test_kit::tests::{command_text, Fenix, Order, Recorded, State, NO_RESULT};
use fenix_test_kit::{assert_snapshot, Fake};

use super::{Facts, Provider, SharedFacts, PROVIDER};

const GO: &str = "src/samples/tests/go";

const ORBIT: &str = include_str!("../samples/tests/go/orbit/orbit_test.go");
const EXTRA: &str = include_str!("../samples/tests/go/orbit/extra_test.go");
const HELPER: &str = include_str!("../samples/tests/go/orbit/helper_test.go");
const GEO: &str = include_str!("../samples/tests/go/orbit/geo/geo_test.go");
const CRASH: &str = include_str!("../samples/tests/go/orbit/crash/crash_test.go");
const SLOW: &str = include_str!("../samples/tests/go/orbit/slow/slow_test.go");
const BOOT: &str = include_str!("../samples/tests/go/orbit/boot/boot_test.go");
const TAGGED: &str = include_str!("../samples/tests/go/orbit/tagged/tagged_test.go");
const GEO_BROKEN: &str = include_str!("../samples/tests/go/orbit/geo/geo_test.go.broken.txt");

const PKG: &str = "example.com/orbit";
const ADDS: &str = "example.com/orbit TestAdds";
const FAILS: &str = "example.com/orbit TestFailsOnPurpose";
const SKIPPED: &str = "example.com/orbit TestSkipped";
const TABLE: &str = "example.com/orbit TestTable";
const SMALL: &str = "example.com/orbit TestTable/small";
const NEGATIVE: &str = "example.com/orbit TestTable/negative_one";
const A_B: &str = "example.com/orbit TestTable/a/b";
const NESTED: &str = "example.com/orbit TestNested";
const OUTER: &str = "example.com/orbit TestNested/outer";
const INNER: &str = "example.com/orbit TestNested/outer/inner";
const EXAMPLE: &str = "example.com/orbit ExampleAdd";
const WRONG: &str = "example.com/orbit ExampleAdd_wrong";
const EXTERNAL: &str = "example.com/orbit TestExternal";
const PARALLEL: &str = "example.com/orbit TestParallel";
const TWO: &str = "example.com/orbit TestParallel/two";
const HELPED: &str = "example.com/orbit TestFromHelper";
const PRINTS: &str = "example.com/orbit TestPrints";
const SPECIAL: &str = "example.com/orbit TestSpecial";
const FUZZ: &str = "example.com/orbit FuzzAdd";
const DIST: &str = "example.com/orbit/geo TestDist";
const PANICS: &str = "example.com/orbit/geo TestPanics";
const NEVER: &str = "example.com/orbit/geo TestNeverRuns";
const BEFORE: &str = "example.com/orbit/crash TestBefore";
const EXITS: &str = "example.com/orbit/crash TestExits";
const AFTER: &str = "example.com/orbit/crash TestAfter";
const QUICK: &str = "example.com/orbit/slow TestQuick";
const SLEEPS: &str = "example.com/orbit/slow TestSlow";
const BOOTED: &str = "example.com/orbit/boot TestNever";
const TAGS: &str = "example.com/orbit/tagged TestNeedsTag";

/// `example.com/orbit` › `orbit_test.go`, and the packages.
const ORBIT_FILE: &str = "\u{1f}example.com/orbit\u{1f}orbit_test.go";
const EXTRA_FILE: &str = "\u{1f}example.com/orbit\u{1f}extra_test.go";
const GEO_PKG: &str = "\u{1f}example.com/orbit/geo";
const CRASH_PKG: &str = "\u{1f}example.com/orbit/crash";

fn facts() -> SharedFacts {
    Rc::new(RefCell::new(Facts { root: format!("{GO}/orbit"), go: Some("go".into()), version: Some("1.24.7".into()), module: true, os: "linux".into(), ..Facts::default() }))
}

/// Fenix with the pack's provider, the sample's files scanned.
fn fenix_with(facts: SharedFacts, files: &[(&str, &str)]) -> (Fenix<Kit>, Fake) {
    let fake = Fake::new();
    let kit = Kit::with_host("go", fake.host()).tests(Box::new(Provider::new(facts)));
    let mut fenix = Fenix::new(kit);
    fenix.manifest("extension.toml").with_fake(fake.clone());
    for (path, text) in files {
        fenix.scan(PROVIDER, &format!("/project/{path}"), text);
    }
    (fenix, fake)
}

const FILES: [(&str, &str); 8] = [
    ("orbit_test.go", ORBIT),
    ("extra_test.go", EXTRA),
    ("helper_test.go", HELPER),
    ("geo/geo_test.go", GEO),
    ("crash/crash_test.go", CRASH),
    ("slow/slow_test.go", SLOW),
    ("boot/boot_test.go", BOOT),
    ("tagged/tagged_test.go", TAGGED),
];

fn orbit() -> (Fenix<Kit>, Fake) {
    fenix_with(facts(), &FILES)
}

fn recorded(name: &str) -> Recorded {
    Recorded::from(format!("{GO}/{name}"))
}

/// What a run of everything in the sample gives.
const RUN_ALL: &[(&str, State)] = &[
    (ADDS, State::Passed),
    (FAILS, State::Failed),
    (SKIPPED, State::Skipped),
    (SMALL, State::Passed),
    (NEGATIVE, State::Failed),
    (A_B, State::Passed),
    (INNER, State::Passed),
    (EXAMPLE, State::Passed),
    (WRONG, State::Failed),
    (EXTERNAL, State::Passed),
    (TWO, State::Failed),
    (HELPED, State::Failed),
    (PRINTS, State::Passed),
    (DIST, State::Passed),
    (PANICS, State::Failed),
    (NEVER, State::Errored),
    (BEFORE, State::Passed),
    (EXITS, State::Errored),
    (AFTER, State::Errored),
    (QUICK, State::Passed),
    (SLEEPS, State::Passed),
    (BOOTED, State::Errored),
];

/// §6.3's done when 1 (the scan): each test at its line, under its
/// package and file; a template marked; a benchmark, an example with no
/// output and what isn't a test aren't there.
#[test]
fn a_scan_gives_each_test_at_its_line_under_its_package_and_file() {
    let (fenix, fake) = orbit();
    let adds = fenix.record(PROVIDER, ADDS).unwrap();
    assert_eq!((adds.line, adds.end_line, adds.parent.as_deref(), adds.path.as_deref()), (Some(7), Some(11), Some(ORBIT_FILE), Some("orbit_test.go")), "lines from 0 in Fenix: line 8");
    for template in [TABLE, NESTED, FUZZ, PARALLEL, SPECIAL] {
        assert!(fenix.record(PROVIDER, template).unwrap().template, "{template} calls t.Run, or is a fuzz target");
    }
    assert!(!fenix.record(PROVIDER, ADDS).unwrap().template);
    assert!(fenix.record(PROVIDER, "example.com/orbit BenchmarkAdd").is_none(), "run only with -bench");
    assert!(fenix.record(PROVIDER, "example.com/orbit ExampleAdd_silent").is_none(), "no Output comment: go test only builds it");
    assert_eq!(fenix.record(PROVIDER, WRONG).unwrap().parent.as_deref(), Some(EXTRA_FILE), "an external test package's file is the same package");
    let file = fenix.record(PROVIDER, ORBIT_FILE).unwrap();
    assert_eq!((file.kind, file.path.as_deref()), (Kind::File, Some("orbit_test.go")));
    assert_eq!(fenix.record(PROVIDER, "\u{1f}example.com/orbit").unwrap().kind, Kind::Group);
    assert_eq!(fenix.record(PROVIDER, DIST).unwrap().parent.as_deref(), Some("\u{1f}example.com/orbit/geo\u{1f}geo/geo_test.go"));
    assert_eq!(fenix.told(PROVIDER), Some(("go test (go1.24.7)".into(), None)));
    assert!(fake.take().iter().any(|c| c == "test-provider go: go test (go1.24.7)"));
    assert_snapshot!("go-scan", fenix.tree(PROVIDER));
}

/// A run of all: every outcome as `go test -json` said it; a failure at
/// its assertion, with its message, its text and what it printed; a skip's
/// reason; subtests found as they ran, under their parent.
#[test]
fn a_run_of_all_gives_each_test_its_result_at_its_assertion() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath ./...");
    assert!(run.steps[0].covers.is_empty() && !run.lists_after);
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(fenix.at(PROVIDER, FAILS), Some(("orbit_test.go".into(), 17, None)), "the last file:line the test printed: the Errorf, not the t.Log before it");
    let d = fenix.detail(PROVIDER, FAILS);
    assert_eq!(d.message.as_deref(), Some("Add(2, 2) = 4, want 5"));
    assert!(d.text.as_deref().is_some_and(|t| t.starts_with("    /project/orbit_test.go:15: some output before the failure") && t.contains("want 5")), "{:?}", d.text);
    assert!(fenix.model(PROVIDER).nodes[FAILS].printed.contains("some output before the failure"));
    assert_eq!(fenix.detail(PROVIDER, SKIPPED).message.as_deref(), Some("slow"));
    assert_eq!(fenix.at(PROVIDER, NEGATIVE), Some(("orbit_test.go".into(), 37, None)));
    assert_eq!(fenix.detail(PROVIDER, NEGATIVE).message.as_deref(), Some("got -2, want -3"));
    assert_eq!(fenix.at(PROVIDER, HELPED), Some(("helper_test.go".into(), 7, None)), "the helper's file, not the test's");
    assert_eq!(fenix.detail(PROVIDER, WRONG).message.as_deref(), Some("got: 4, want: 5"));
    assert_eq!(fenix.at(PROVIDER, PANICS), Some(("geo/geo_test.go".into(), 13, None)), "a panic's first frame in the project");
    assert_eq!(fenix.detail(PROVIDER, PANICS).message.as_deref(), Some("panic: assignment to entry in nil map"));
    assert!(fenix.detail(PROVIDER, PRINTS).duration_ms.is_some());
    // Found as they ran: under their parent, labelled by what their key adds.
    let negative = fenix.record(PROVIDER, NEGATIVE).unwrap();
    assert_eq!((negative.kind, negative.parent.as_deref(), negative.label.as_str()), (Kind::Case, Some(TABLE), "negative_one"));
    assert_eq!(fenix.record(PROVIDER, A_B).unwrap().parent.as_deref(), Some(TABLE), "TestTable/a never ran: a/b is one name");
    assert_eq!(fenix.record(PROVIDER, INNER).unwrap().parent.as_deref(), Some(OUTER), "TestNested/outer ran: inner is its case");
    assert_eq!(fenix.record(PROVIDER, OUTER).unwrap().parent.as_deref(), Some(NESTED));
    assert_snapshot!("go-run-all", fenix.tree(PROVIDER));
}

/// The same fixture in every stream order and batch size, and cut small:
/// one tree. (Go's JSON is one stream, so only the batches and the cuts
/// can differ; stderr's lines are Go before 1.24's, below.)
#[test]
fn the_lines_in_any_batches_and_cuts_end_the_same() {
    let (mut whole, _) = orbit();
    whole.on_recorded(recorded("run-all"));
    whole.run_all(PROVIDER);
    let tree = whole.tree(PROVIDER);
    let orders = [Order::Recorded, Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(1), Order::Shuffled(7)];
    for (i, order) in orders.into_iter().enumerate() {
        let (mut f, _) = orbit();
        f.order = order.clone();
        f.batch = 1 + i * 5;
        f.cut_at = if i % 2 == 0 { 64 * 1024 } else { 37 };
        f.on_recorded(recorded("run-all"));
        f.run_all(PROVIDER);
        assert_eq!(f.tree(PROVIDER), tree, "{order:?}, {} a call, cut at {}", f.batch, f.cut_at);
    }
}

/// Go's cache: a package that's cached replays its tests' events, with the
/// times of the run it replays (0 for a quick test), and is reported as it
/// was run.
#[test]
fn a_cached_package_is_reported_as_it_ran() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all-cached"));
    fenix.run_all(PROVIDER);
    fenix.assert_states(PROVIDER, RUN_ALL);
    assert_eq!(fenix.detail(PROVIDER, SLEEPS).duration_ms, Some(2000.0), "the replay says what the test took then");
    assert_eq!(fenix.detail(PROVIDER, QUICK).duration_ms, Some(0.0));
    assert!(std::fs::read_to_string(format!("{GO}/run-all-cached.stdout")).unwrap().contains("(cached)"));
}

/// §6.3's done when 2 (the listing): `go list` gives each package's test
/// files; one the scan hasn't seen is read and scanned, so its tests are in
/// the tree; the scan's own are left as they were.
#[test]
fn the_listing_finds_the_files_the_scan_didnt_reach() {
    // Fenix scanned only some of the files, as it does past its 2,000.
    let (mut fenix, _) = fenix_with(facts(), &[("orbit_test.go", ORBIT), ("geo/geo_test.go", GEO)]);
    assert!(fenix.record(PROVIDER, EXTERNAL).is_none() && fenix.record(PROVIDER, QUICK).is_none());
    fenix.on_recorded(recorded("list"));
    let run = fenix.discover(PROVIDER);
    assert!(run.complete, "{run:?}");
    assert_eq!(command_text(&run.steps[0].command), "go list -e -f {{.ImportPath}}{{\"\\t\"}}{{.Dir}}{{\"\\t\"}}{{join .TestGoFiles \",\"}}{{\"\\t\"}}{{join .XTestGoFiles \",\"}} ./...");
    assert_eq!(run.steps[0].command.cwd.as_deref(), Some("/project"));
    let external = fenix.record(PROVIDER, EXTERNAL).unwrap();
    assert_eq!((external.parent.as_deref(), external.line, external.path.as_deref()), (Some(EXTRA_FILE), Some(10), Some("extra_test.go")), "an xtest file's tests, at their line");
    assert!(fenix.record(PROVIDER, QUICK).is_some() && fenix.record(PROVIDER, BEFORE).is_some() && fenix.record(PROVIDER, HELPED).is_some());
    assert_eq!(fenix.record(PROVIDER, ADDS).unwrap().line, Some(7), "the scan's line stays");
    assert!(fenix.record(PROVIDER, TAGS).is_none(), "tagged: go says it has no test files, and it was never scanned here");
    assert_snapshot!("go-listed", fenix.tree(PROVIDER));
}

/// §6.3's done when 3: a package that doesn't build is `broken`, with the
/// compiler's first error and its place; the other packages' results stand
/// and no test failed.
#[test]
fn a_package_that_doesnt_build_is_broken_not_a_test_failure() {
    let (mut fenix, _) = fenix_with(facts(), &[("orbit_test.go", ORBIT), ("geo/geo_test.go", GEO_BROKEN), ("crash/crash_test.go", CRASH)]);
    fenix.on_recorded(recorded("broken"));
    let run = fenix.run_all(PROVIDER);
    let message = "example.com/orbit/geo didn't build: geo/geo_test.go:6 cannot use \"3\" (untyped string constant) as float64 value in argument to Dist";
    assert_eq!(run.broken, [(Some(GEO_PKG.into()), message.into())]);
    assert_eq!(fenix.state(PROVIDER, DIST), State::Errored);
    assert_eq!(fenix.detail(PROVIDER, DIST).message.as_deref(), Some(message));
    assert_eq!(fenix.at(PROVIDER, DIST), Some(("geo/geo_test.go".into(), 6, Some(13))));
    fenix.assert_states(PROVIDER, &[(ADDS, State::Passed), (FAILS, State::Failed), (BEFORE, State::Passed)]);
    assert!(!fenix.failed(PROVIDER).contains(&DIST.to_string()) || fenix.state(PROVIDER, DIST) == State::Errored, "never a failure");

    // vet's error stops a package the same way.
    let (mut vet, _) = orbit();
    vet.on_recorded(recorded("vet-broken"));
    let run = vet.run(PROVIDER, &[GEO_PKG]);
    assert_eq!(run.broken.len(), 1);
    assert!(run.broken[0].1.starts_with("example.com/orbit/geo didn't build: geo/geo_test.go:7 (*testing.common).Errorf format %d has arg \"x\" of wrong type string"), "{:?}", run.broken);
    assert_eq!(vet.state(PROVIDER, DIST), State::Errored);
}

/// Go before 1.24 (1.23.8, recorded): the compiler's lines are on stderr,
/// and the package's `fail` has no `FailedBuild`: its output says `[build
/// failed]`. Read from stderr when the step ends, in any order.
#[test]
fn before_go_1_24_the_error_is_read_from_stderr() {
    let mut tree = None;
    for order in [Order::Recorded, Order::StdoutFirst, Order::StderrFirst, Order::Shuffled(3)] {
        let (mut fenix, _) = fenix_with(facts(), &[("orbit_test.go", ORBIT), ("geo/geo_test.go", GEO_BROKEN), ("crash/crash_test.go", CRASH)]);
        fenix.order = order.clone();
        fenix.on_recorded(recorded("broken-1-23"));
        let run = fenix.run_all(PROVIDER);
        let message = "example.com/orbit/geo didn't build: geo/geo_test.go:6 cannot use \"3\" (untyped string constant) as float64 value in argument to Dist";
        assert_eq!(run.broken, [(Some(GEO_PKG.into()), message.into())], "{order:?}");
        assert_eq!(fenix.state(PROVIDER, DIST), State::Errored);
        match &tree {
            None => tree = Some(fenix.tree(PROVIDER)),
            Some(t) => assert_eq!(&fenix.tree(PROVIDER), t, "{order:?}"),
        }
    }
}

/// `go test ./...` where the top isn't a module of a `go.work`: Go says so
/// for a pattern, not a package: `broken` for the whole step.
#[test]
fn a_pattern_go_cant_set_up_is_broken_for_the_whole_step() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("setup-failed"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken, [(None, "pattern ./...: directory prefix . does not contain modules listed in go.work or their selected dependencies".into())], "an id no scan gave would fail the step");
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
}

/// A crash is scoped to its package: `os.Exit` in a test, a panic in a
/// test (the binary ends, the later tests never ran), a `TestMain`'s
/// panic, a timeout's; the other packages' results stand.
#[test]
fn a_crash_errors_its_own_packages_tests_and_spares_the_next() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    let run = fenix.run_all(PROVIDER);
    let mut crashed = run.crashed.clone();
    crashed.sort();
    assert_eq!(
        crashed,
        [
            (Some("\u{1f}example.com/orbit/boot".into()), "panic: setup failed (before any test ran)".into()),
            (Some(CRASH_PKG.into()), "its test binary ended while TestExits was running (os.Exit, or a crash)".into()),
            (Some(GEO_PKG.into()), "TestPanics panicked, which ends the test binary: panic: assignment to entry in nil map".into()),
        ]
    );
    fenix.assert_states(PROVIDER, &[(BEFORE, State::Passed), (EXITS, State::Errored), (AFTER, State::Errored), (DIST, State::Passed), (PANICS, State::Failed), (NEVER, State::Errored), (QUICK, State::Passed), (ADDS, State::Passed)]);
    assert_eq!(fenix.detail(PROVIDER, AFTER).message.as_deref(), Some("its test binary ended while TestExits was running (os.Exit, or a crash)"));

    // A timeout: the test it caught has the panic.
    let (mut slow, _) = orbit();
    slow.on_recorded(recorded("timeout"));
    let run = slow.run(PROVIDER, &["\u{1f}example.com/orbit/slow"]);
    assert_eq!(run.crashed, [(Some("\u{1f}example.com/orbit/slow".into()), "panic: test timed out after 1s (running: TestSlow)".into())]);
    slow.assert_states(PROVIDER, &[(QUICK, State::Passed), (SLEEPS, State::Errored)]);
}

/// One test by its name: the exact command, its `covers`, its folder.
#[test]
fn one_test_runs_by_its_anchored_name_in_its_package_only() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-one"));
    let run = fenix.run(PROVIDER, &[ADDS]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestAdds$ example.com/orbit");
    assert_eq!((run.steps[0].covers.as_slice(), run.steps[0].command.cwd.as_deref()), ([ADDS.to_string()].as_slice(), Some("/project")));
    assert!(!run.lists_after);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Unknown);

    // Two of one package: one regex.
    let (mut set, _) = orbit();
    set.on_recorded(recorded("run-set"));
    let run = set.run(PROVIDER, &[ADDS, FAILS]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^(TestAdds|TestFailsOnPurpose)$ example.com/orbit");
    set.assert_states(PROVIDER, &[(ADDS, State::Passed), (FAILS, State::Failed)]);
}

/// Subtests: a level of the regex to each `/`; a name's characters quoted;
/// `TestTable/a/b` as the two levels Go reads it as.
#[test]
fn subtests_run_by_a_level_to_each_slash_with_their_names_quoted() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    fenix.on_recorded(recorded("run-subs"));
    let run = fenix.run(PROVIDER, &[SMALL, NEGATIVE]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestTable$/^(negative_one|small)$ example.com/orbit");
    assert_eq!(run.steps[0].covers, [NEGATIVE, SMALL].map(String::from), "in the order of the regex");
    assert_eq!(fenix.state(PROVIDER, NEGATIVE), State::Failed);

    fenix.on_recorded(recorded("run-nested"));
    let run = fenix.run(PROVIDER, &[INNER]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestNested$/^outer$/^inner$ example.com/orbit");

    fenix.on_recorded(recorded("run-special"));
    let special: Vec<String> = ["a+b_(c)", "x|y", "[z]", "naïve"].iter().map(|n| format!("example.com/orbit TestSpecial/{n}")).collect();
    let ids: Vec<&str> = special.iter().map(String::as_str).collect();
    let run = fenix.run(PROVIDER, &ids);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestSpecial$/^(\\[z\\]|a\\+b_\\(c\\)|naïve|x\\|y)$ example.com/orbit");
    fenix.assert_states(PROVIDER, &special.iter().map(|k| (k.as_str(), State::Passed)).collect::<Vec<_>>());

    // A name with a slash in it, as Go reads it: two levels.
    let p = Provider::new(facts());
    let mut p = p;
    let found = fenix_lang_kit::tests::Found::case(A_B, "a/b", TABLE);
    let steps = p.plan(&Ask::Run(vec![Asked::Test(found)])).unwrap();
    assert_eq!(command_text(&steps[0].command), "go test -json -fullpath -run ^TestTable$/^a$/^b$ example.com/orbit");
}

/// The failed again: every package's failures by their names, a step for
/// each set of names; the cases by their parents' levels.
#[test]
fn the_failed_run_again_by_name_a_step_for_each_package_and_parent() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    let failed = fenix.failed(PROVIDER);
    assert!(failed.contains(&FAILS.to_string()) && failed.contains(&NEGATIVE.to_string()) && failed.contains(&PANICS.to_string()));
    let lines = Rc::new(RefCell::new(Vec::new()));
    let seen = lines.clone();
    fenix.on_step_with(move |s| {
        seen.borrow_mut().push(command_text(&s.command));
        Some(Recorded::stdout("").exits(0))
    });
    let run = fenix.run_failed(PROVIDER);
    let lines = lines.borrow().clone();
    assert_eq!(
        lines,
        [
            "go test -json -fullpath -run ^(ExampleAdd_wrong|TestFailsOnPurpose|TestFromHelper|TestParallel|TestTable)$ example.com/orbit",
            "go test -json -fullpath -run ^(TestAfter|TestExits)$ example.com/orbit/crash",
            "go test -json -fullpath -run ^TestNever$ example.com/orbit/boot",
            "go test -json -fullpath -run ^(TestNeverRuns|TestPanics)$ example.com/orbit/geo",
        ],
        "a template that failed is asked whole, its failed cases under it"
    );
    assert_eq!(run.steps.len(), 4);
    assert_eq!(run.steps[2].covers, [BOOTED].map(String::from));
}

/// A file asked with its fill-in names its top-level tests exactly (a case
/// left to its parent, which runs them all); a file or a package with none
/// runs its package, all of it, since Go has no filter for a file; the
/// group is judged on the package's last step.
#[test]
fn a_fill_in_is_named_exactly_and_none_runs_the_package() {
    let (mut fenix, _) = orbit();
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    let lines = Rc::new(RefCell::new(Vec::new()));
    let seen = lines.clone();
    fenix.on_step_with(move |s| {
        seen.borrow_mut().push((command_text(&s.command), s.covers.clone()));
        Some(Recorded::stdout("").exits(0))
    });
    fenix.run(PROVIDER, &[ORBIT_FILE]);
    let (line, covers) = lines.borrow()[0].clone();
    assert_eq!(line, "go test -json -fullpath -run ^(ExampleAdd|TestAdds|TestFailsOnPurpose|TestNested|TestSkipped|TestTable)$ example.com/orbit", "TestTable's cases are TestTable's");
    assert!(covers.contains(&ORBIT_FILE.to_string()), "{covers:?}");
    assert!(covers.contains(&TABLE.to_string()) && !covers.contains(&NEGATIVE.to_string()), "{covers:?}");

    // A package: itself, with `go test <package>`.
    fenix.run(PROVIDER, &["\u{1f}example.com/orbit/geo"]);
    let (line, covers) = lines.borrow()[1].clone();
    assert_eq!((line.as_str(), covers.as_slice()), ("go test -json -fullpath example.com/orbit/geo", [GEO_PKG.to_string()].as_slice()));

    // A file with no fill-in (Fenix had none to send): its package, whole.
    let mut p = Provider::new(facts());
    for (path, text) in FILES {
        p.scan(&format!("/project/{path}"), text);
    }
    let file = vec![Group::package("example.com/orbit/geo"), Group::file("geo/geo_test.go")];
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: file, tests: None, known: vec![DIST.into()] }])).unwrap();
    assert_eq!(command_text(&steps[0].command), "go test -json -fullpath example.com/orbit/geo");
    assert_eq!(steps[0].covers, [fenix_lang_kit::tests::Covered::Group(vec!["example.com/orbit/geo".into(), "geo/geo_test.go".into()])]);
}

/// A group is judged on its package's last step, so it isn't errored for
/// tests a later step of the package runs.
#[test]
fn a_group_is_judged_on_the_last_step_of_its_package() {
    let mut p = Provider::new(facts());
    for (path, text) in FILES {
        p.scan(&format!("/project/{path}"), text);
    }
    let tests = vec![
        fenix_lang_kit::tests::Found::test(ADDS, "TestAdds").under([Group::package(PKG), Group::file("orbit_test.go")]),
        fenix_lang_kit::tests::Found::test(TABLE, "TestTable").under([Group::package(PKG), Group::file("orbit_test.go")]).template(),
    ];
    let file = vec![Group::package(PKG), Group::file("orbit_test.go")];
    let nested = fenix_lang_kit::tests::Found::case(INNER, "inner", OUTER);
    let steps = p.plan(&Ask::Run(vec![Asked::Group { groups: file, tests: Some(tests), known: Vec::new() }, Asked::Test(nested)])).unwrap();
    assert_eq!(steps.iter().map(|s| command_text(&s.command)).collect::<Vec<_>>(), ["go test -json -fullpath -run ^(TestAdds|TestTable)$ example.com/orbit", "go test -json -fullpath -run ^TestNested$/^outer$/^inner$ example.com/orbit"]);
    let group = fenix_lang_kit::tests::Covered::Group(vec![PKG.into(), "orbit_test.go".into()]);
    assert!(!steps[0].covers.contains(&group) && steps[1].covers.contains(&group), "{:?}", steps.iter().map(|s| &s.covers).collect::<Vec<_>>());
}

/// After Fenix starts again it gives no `data` (Go never gives any): the
/// same plan, from the keys alone.
#[test]
fn without_data_the_plan_comes_from_the_keys() {
    let (mut fenix, _) = orbit();
    fenix.forget_data(PROVIDER);
    let api::Ask::Run(records) = fenix.ask_for(PROVIDER, &[ADDS, FAILS]) else { panic!() };
    assert!(records.iter().all(|r| r.data.is_none()));
    fenix.on_step(|_| true, recorded("run-set"));
    let run = fenix.run(PROVIDER, &[ADDS, FAILS]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^(TestAdds|TestFailsOnPurpose)$ example.com/orbit");
}

/// A template's cases come from a run, under it; the parent's own result is
/// its cases' and what Go said of it, and a run of the template runs them
/// all.
#[test]
fn a_templates_cases_are_found_under_it_and_a_run_of_it_runs_them_all() {
    let (mut fenix, _) = orbit();
    assert!(fenix.record(PROVIDER, SMALL).is_none(), "cases exist only once run");
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    let table = fenix.record(PROVIDER, TABLE).unwrap();
    assert!(table.template);
    for case in [SMALL, NEGATIVE, A_B] {
        assert_eq!(fenix.record(PROVIDER, case).unwrap().parent.as_deref(), Some(TABLE));
    }
    assert_eq!(fenix.record(PROVIDER, SMALL).unwrap().line, Some(24), "at its parent's line");
    assert_eq!(fenix.state(PROVIDER, TABLE), State::Failed, "its cases'");
    // Run again, only the template: all of its cases, none left without a result.
    fenix.on_step(|_| true, recorded("run-subs"));
    let run = fenix.run(PROVIDER, &[TABLE]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestTable$ example.com/orbit");
    assert!(!fenix.failed(PROVIDER).iter().any(|id| fenix.detail(PROVIDER, id).message.as_deref() == Some(NO_RESULT) && id.starts_with(TABLE)), "the cases it dropped went with their results");
}

const APP: &str = include_str!("../samples/tests/go/ws/app/app_test.go");
const LIB: &str = include_str!("../samples/tests/go/ws/lib/lib_test.go");
const UTIL: &str = include_str!("../samples/tests/go/ws/lib/util/util_test.go");
const APP_DOUBLE: &str = "example.com/app TestDouble";
const LIB_DOUBLE: &str = "example.com/lib TestDouble";
const TRIPLE: &str = "example.com/lib/util TestTriple";

/// A project with a `go.work` of two modules: each package's import path
/// from the module of its folder, `./app/...` and `./lib/...` from the
/// project's own folder (`./...` isn't a pattern there), the same name in
/// two packages kept apart.
fn workspace() -> (Fenix<Kit>, Fake) {
    let facts = Rc::new(RefCell::new(Facts { root: format!("{GO}/ws"), go: Some("go".into()), version: Some("1.24.7".into()), work: true, os: "linux".into(), ..Facts::default() }));
    fenix_with(facts, &[("app/app_test.go", APP), ("lib/lib_test.go", LIB), ("lib/util/util_test.go", UTIL)])
}

#[test]
fn a_workspace_runs_from_its_own_folder_each_modules_packages_apart() {
    let (mut fenix, _) = workspace();
    assert_eq!(fenix.told(PROVIDER), Some(("go test (go1.24.7)".into(), None)));
    assert_eq!(fenix.record(PROVIDER, APP_DOUBLE).unwrap().parent.as_deref(), Some("\u{1f}example.com/app\u{1f}app/app_test.go"));
    assert_eq!(fenix.record(PROVIDER, TRIPLE).unwrap().parent.as_deref(), Some("\u{1f}example.com/lib/util\u{1f}lib/util/util_test.go"));
    fenix.on_recorded(recorded("ws-list"));
    let run = fenix.discover(PROVIDER);
    assert!(run.complete);
    assert_eq!(command_text(&run.steps[0].command), "go list -e -f {{.ImportPath}}{{\"\\t\"}}{{.Dir}}{{\"\\t\"}}{{join .TestGoFiles \",\"}}{{\"\\t\"}}{{join .XTestGoFiles \",\"}} ./app/... ./lib/...");
    fenix.on_recorded(recorded("ws-run-all"));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath ./app/... ./lib/...");
    fenix.assert_states(PROVIDER, &[(APP_DOUBLE, State::Passed), ("example.com/app TestFour", State::Passed), (LIB_DOUBLE, State::Passed), (TRIPLE, State::Failed)]);
    assert_eq!(fenix.at(PROVIDER, TRIPLE), Some(("lib/util/util_test.go".into(), 7, None)), "a path below the project, not the module");
    assert_eq!(fenix.detail(PROVIDER, TRIPLE).message.as_deref(), Some("wrong"));
    assert_snapshot!("go-ws-run-all", fenix.tree(PROVIDER));
    // One name, in one package only, though two have it.
    fenix.on_step(|_| true, recorded("ws-one"));
    let run = fenix.run(PROVIDER, &[APP_DOUBLE]);
    assert_eq!(command_text(&run.steps[0].command), "go test -json -fullpath -run ^TestDouble$ example.com/app");
    assert_eq!(run.steps[0].command.cwd.as_deref(), Some("/project"));
    assert_eq!(fenix.state(PROVIDER, APP_DOUBLE), State::Passed);
}

/// Modules with no `go.work` can't be run together: a step for each, in
/// its own folder, where its import paths mean something.
#[test]
fn modules_without_a_go_work_run_each_in_its_own_folder() {
    let dir = std::env::temp_dir().join(format!("fenix-go-modules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in [("go.mod", "module example.com/top\n"), ("tools/go.mod", "module example.com/tools\n"), ("tools/x/go.mod", "module example.com/tools/x\n")] {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let facts = Rc::new(RefCell::new(Facts { root: dir.to_string_lossy().into_owned(), go: Some("go".into()), version: Some("1.24.7".into()), module: true, os: "linux".into(), ..Facts::default() }));
    let test = "package p\n\nimport \"testing\"\n\nfunc TestIt(t *testing.T) {}\n";
    let (mut fenix, _) = fenix_with(facts, &[("top_test.go", test), ("tools/t_test.go", test), ("tools/x/x_test.go", test), ("tools/testdata/ignored_test.go", test)]);
    let keys: Vec<String> = fenix.ids(PROVIDER).into_iter().filter(|k| !k.starts_with('\u{1f}')).collect();
    assert_eq!(keys, ["example.com/tools TestIt", "example.com/tools/x TestIt", "example.com/top TestIt"], "a nested module's own path; testdata left out");
    fenix.on_step(|_| true, Recorded::stdout("").exits(0));
    let run = fenix.run_all(PROVIDER);
    let steps: Vec<(String, Option<String>)> = run.steps.iter().map(|s| (command_text(&s.command), s.command.cwd.clone())).collect();
    assert_eq!(
        steps,
        [
            ("go test -json -fullpath ./...".to_string(), Some("/project".to_string())),
            ("go test -json -fullpath ./...".to_string(), Some("/project/tools".to_string())),
            ("go test -json -fullpath ./...".to_string(), Some("/project/tools/x".to_string())),
        ]
    );
    let run = fenix.run(PROVIDER, &["example.com/tools/x TestIt", "example.com/top TestIt"]);
    let steps: Vec<(String, Option<String>)> = run.steps.iter().map(|s| (command_text(&s.command), s.command.cwd.clone())).collect();
    assert_eq!(steps, [("go test -json -fullpath -run ^TestIt$ example.com/top".to_string(), Some("/project".to_string())), ("go test -json -fullpath -run ^TestIt$ example.com/tools/x".to_string(), Some("/project/tools/x".to_string()))]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// §6.3's done when: 2,000 failed tests run again in steps each under
/// Windows' command line, every one named once.
#[test]
fn two_thousand_failed_tests_run_again_under_the_limit() {
    let file = |f: usize| -> String { std::iter::once("package many\n\nimport \"testing\"\n\n".to_string()).chain((0..500).map(|i| format!("func TestFailsOnPurposeNumber{f}{i:04}(t *testing.T) {{\n\tt.Fatal(\"no\")\n}}\n\n"))).collect() };
    let files: Vec<(String, String)> = (0..4).map(|f| (format!("many/many{f}_test.go"), file(f))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let (mut fenix, _) = fenix_with(facts(), &refs);
    let mut out = String::new();
    let event = |action: &str, test: &str| format!("{{\"Action\":\"{action}\",\"Package\":\"example.com/orbit/many\",\"Test\":\"{test}\",\"Elapsed\":0}}\n");
    for n in 0..2000 {
        let name = format!("TestFailsOnPurposeNumber{}{:04}", n / 500, n % 500);
        out.push_str(&event("run", &name));
        out.push_str(&format!("{{\"Action\":\"output\",\"Package\":\"example.com/orbit/many\",\"Test\":\"{name}\",\"Output\":\"    /project/many/many{}_test.go:6: no\\n\"}}\n", n / 500));
        out.push_str(&event("fail", &name));
    }
    out.push_str("{\"Action\":\"fail\",\"Package\":\"example.com/orbit/many\",\"Elapsed\":0.2}\n");
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.failed(PROVIDER).len(), 2000);
    fenix.on_step_with(move |s| {
        let at = s.command.args.iter().position(|a| a == "-run")?;
        let names = s.command.args[at + 1].trim_start_matches("^(").trim_end_matches(")$").split('|').map(String::from).collect::<Vec<_>>();
        let lines: String = names.iter().map(|n| event("run", n) + &event("pass", n)).collect();
        Some(Recorded::stdout(lines))
    });
    let run = fenix.run_failed(PROVIDER);
    assert!(run.steps.len() > 1, "{} steps", run.steps.len());
    assert!(run.steps.iter().all(|s| DIRECT.fits(&s.command)));
    assert_eq!(run.steps.iter().map(|s| s.covers.len()).sum::<usize>(), 2000);
    assert!(fenix.failed(PROVIDER).is_empty());
}

/// A file of 2,000 tests is scanned within the driver's budget for a call
/// (5 ms in a release build, 50 ms in a debug one): the provider's scan
/// and lang-kit's adapter together, natively. The best of five.
#[test]
fn a_file_of_two_thousand_tests_is_scanned_within_a_calls_budget() {
    let text: String = std::iter::once("package many\n\nimport \"testing\"\n\n".to_string()).chain((0..2000).map(|i| format!("func TestFailsOnPurposeNumber{i:05}(t *testing.T) {{\n\tif got := Add({i}, 1); got != {i} + 1 {{\n\t\tt.Errorf(\"got %d\", got)\n\t}}\n}}\n\n"))).collect();
    let budget = if cfg!(debug_assertions) { std::time::Duration::from_millis(50) } else { std::time::Duration::from_millis(5) };
    let best = (0..5)
        .map(|_| {
            let mut kit = Kit::with_host("go", Fake::new().host()).tests(Box::new(Provider::new(facts())));
            let start = std::time::Instant::now();
            let found = kit.tests_scan(PROVIDER, "/project/many/many_test.go", &text);
            assert_eq!(found.len(), 2002, "the package, the file and 2,000 tests");
            start.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < budget, "{best:?}");
}

/// Debugging: Delve's test mode on the package's folder, `-test.run` the
/// test by its levels, at once (no build first).
#[test]
fn debugging_a_test_runs_delve_in_test_mode_on_its_package() {
    let (mut fenix, _) = orbit();
    let launched = fenix.launch(PROVIDER, ADDS);
    assert!(launched.built.is_none());
    let config = launched.config.unwrap();
    assert_eq!((config.name.as_str(), config.program.as_deref(), config.cwd.as_deref()), ("Go: TestAdds", Some("/project"), Some("/project")));
    assert_eq!(config.args, ["-test.run", "^TestAdds$", "-test.v"]);
    assert_eq!(config.attributes, "{\"mode\":\"test\"}");
    assert_eq!(config.languages, ["go"]);
    // A package below the project's folder, a subtest by its levels.
    fenix.on_recorded(recorded("run-all"));
    fenix.run_all(PROVIDER);
    let config = fenix.launch(PROVIDER, A_B).config.unwrap();
    assert_eq!(config.args, ["-test.run", "^TestTable$/^a$/^b$", "-test.v"], "the levels Go reads");
    let config = fenix.launch(PROVIDER, PANICS).config.unwrap();
    assert_eq!((config.program.as_deref(), config.cwd.as_deref()), (Some("/project/geo"), Some("/project/geo")));
    let (mut ws, _) = workspace();
    let config = ws.launch(PROVIDER, TRIPLE).config.unwrap();
    assert_eq!((config.name.as_str(), config.program.as_deref()), ("Go: TestTriple", Some("/project/lib/util")));
}

/// A step stopped part way: what ended keeps its result, the rest goes back.
#[test]
fn a_stop_keeps_what_ended() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, recorded("run-all").stopped_after(40));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.steps[0].end, StepEnd::Stopped);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, SLEEPS), State::Unknown, "never reached");
}

/// Windows' paths in a test's lines and a panic's frames, by hand: Go prints
/// `C:\Users\u\orbit\geo\geo_test.go` with `-fullpath` and a trace's full
/// paths with backslashes.
#[test]
fn windows_paths_are_below_the_project() {
    let (mut fenix, _) = orbit();
    let out = r#"{"Action":"run","Package":"example.com/orbit/geo","Test":"TestDist"}
{"Action":"output","Package":"example.com/orbit/geo","Test":"TestDist","Output":"    C:\\Users\\u\\orbit\\geo\\geo_test.go:6: asymmetric\n"}
{"Action":"fail","Package":"example.com/orbit/geo","Test":"TestDist","Elapsed":0}
{"Action":"run","Package":"example.com/orbit/geo","Test":"TestPanics"}
{"Action":"output","Package":"example.com/orbit/geo","Test":"TestPanics","Output":"panic: boom\n"}
{"Action":"output","Package":"example.com/orbit/geo","Test":"TestPanics","Output":"example.com/orbit/geo.TestPanics(0xc0)\n"}
{"Action":"output","Package":"example.com/orbit/geo","Test":"TestPanics","Output":"\tC:/Users/u/orbit/geo/geo_test.go:13 +0x28\n"}
{"Action":"fail","Package":"example.com/orbit/geo","Test":"TestPanics","Elapsed":0}
{"Action":"fail","Package":"example.com/orbit/geo","Elapsed":0.1}
"#;
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run(PROVIDER, &[DIST, PANICS]);
    assert_eq!(fenix.at(PROVIDER, DIST), Some(("geo/geo_test.go".into(), 6, None)));
    assert_eq!(fenix.at(PROVIDER, PANICS), Some(("geo/geo_test.go".into(), 13, None)));
}

/// Odd input never panics: a line that isn't JSON, JSON of another shape,
/// an event for a test that never began, an output cut mid-line, a result
/// with no package.
#[test]
fn lines_that_arent_events_are_passed_over() {
    let (mut fenix, _) = orbit();
    let out = "go: downloading example.com/x v1.0.0\n{\"Action\":\n[1,2]\n{\"Action\":\"output\",\"Package\":\"example.com/orbit\",\"Test\":\"TestAdds\",\"Output\":\"no newline\"}\n{\"Action\":\"pass\",\"Package\":\"example.com/orbit\",\"Test\":\"TestAdds\",\"Elapsed\":null}\n{\"Action\":\"pass\"}\n{\"Action\":\"run\",\"Package\":\"example.com/orbit\"}\n{\"Action\":\"bench\",\"Package\":\"example.com/orbit\",\"Test\":\"BenchmarkAdd\"}\n\u{feff}{\"Action\":\"pass\"}\n{\"Action\":\"fail\",\"Package\":\"example.com/orbit\",\"Test\":\"TestFailsOnPurpose\",\"Elapsed\":1e300}\n";
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run_all(PROVIDER);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Passed);
    assert_eq!(fenix.state(PROVIDER, FAILS), State::Failed);
}

/// Go didn't get to run (no module, a flag it doesn't know): its words on
/// stderr, `broken` for the step.
#[test]
fn what_go_says_when_it_cannot_run_is_broken() {
    let (mut fenix, _) = orbit();
    fenix.on_step(|_| true, Recorded::stdout("").and_stderr("go: downloading example.com/x v1.0.0\nflag provided but not defined: -fullpath\nusage: go test [build/test flags] [packages] [build/test flags & test binary flags]\n").exits(2));
    let run = fenix.run_all(PROVIDER);
    assert_eq!(run.broken, [(None, "flag provided but not defined: -fullpath".into())]);
    assert_eq!(fenix.state(PROVIDER, ADDS), State::Errored);
}

/// A test that prints a great deal keeps the start and the end of what it
/// printed (its panic and trace come last), not all of it.
#[test]
fn a_test_that_prints_a_great_deal_is_read_within_bounds() {
    let (mut fenix, _) = orbit();
    let mut out = String::from("{\"Action\":\"run\",\"Package\":\"example.com/orbit/geo\",\"Test\":\"TestPanics\"}\n");
    let long = "x".repeat(200 * 1024);
    for i in 0..5000 {
        let line = if i == 2500 { long.clone() } else { format!("line {i}") };
        out.push_str(&format!("{{\"Action\":\"output\",\"Package\":\"example.com/orbit/geo\",\"Test\":\"TestPanics\",\"Output\":\"{line}\\n\"}}\n"));
    }
    for line in ["panic: boom\\n", "example.com/orbit/geo.TestPanics(0xc0)\\n", "\\t/home/u/orbit/geo/geo_test.go:13 +0x28\\n"] {
        out.push_str(&format!("{{\"Action\":\"output\",\"Package\":\"example.com/orbit/geo\",\"Test\":\"TestPanics\",\"Output\":\"{line}\"}}\n"));
    }
    out.push_str("{\"Action\":\"fail\",\"Package\":\"example.com/orbit/geo\",\"Test\":\"TestPanics\",\"Elapsed\":0}\n{\"Action\":\"fail\",\"Package\":\"example.com/orbit/geo\",\"Elapsed\":0.1}\n");
    fenix.on_step(|_| true, Recorded::stdout(out).exits(1));
    fenix.run(PROVIDER, &[PANICS]);
    assert_eq!(fenix.detail(PROVIDER, PANICS).message.as_deref(), Some("panic: boom"));
    assert_eq!(fenix.at(PROVIDER, PANICS), Some(("geo/geo_test.go".into(), 13, None)));
    let text = fenix.detail(PROVIDER, PANICS).text.unwrap_or_default();
    assert!(text.starts_with("line 0\n") && text.contains("lines left out") && text.ends_with("geo_test.go:13 +0x28"), "{}", &text[..text.len().min(200)]);
    assert!(text.len() < 150 * 1024, "{} bytes", text.len());
}

/// The listing reads the files the scan missed a few at a time and the
/// rest when the step ends, none twice.
#[test]
fn the_listing_reads_what_is_left_when_the_step_ends() {
    let dir = std::env::temp_dir().join(format!("fenix-go-many-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/many\n").unwrap();
    let names: Vec<String> = (0..150).map(|i| format!("f{i:03}_test.go")).collect();
    for (i, n) in names.iter().enumerate() {
        std::fs::write(dir.join(n), format!("package many\n\nimport \"testing\"\n\nfunc TestN{i:03}(t *testing.T) {{}}\n")).unwrap();
    }
    let facts = Rc::new(RefCell::new(Facts { root: dir.to_string_lossy().into_owned(), go: Some("go".into()), version: Some("1.24.7".into()), module: true, os: "linux".into(), ..Facts::default() }));
    let (mut fenix, _) = fenix_with(facts, &[("f000_test.go", "package many\n\nimport \"testing\"\n\nfunc TestN000(t *testing.T) {}\n")]);
    let list = format!("example.com/many\t/project\t{}\t\n", names.join(","));
    fenix.on_step(|_| true, Recorded::stdout(list));
    let run = fenix.discover(PROVIDER);
    assert!(run.complete);
    let tests: Vec<String> = fenix.ids(PROVIDER).into_iter().filter(|k| !k.starts_with('\u{1f}')).collect();
    assert_eq!(tests.len(), 150, "all of them, the scanned one once");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A package a test package imports doesn't build: Go's `fail` for the
/// importer says so (`FailedBuild` the dependency, whose errors came
/// first), and the package itself fails the same way; a package that
/// doesn't use it runs.
#[test]
fn a_dependency_that_doesnt_build_breaks_the_packages_that_import_it() {
    let (mut fenix, _) = workspace();
    fenix.on_recorded(recorded("ws-dep-broken"));
    let run = fenix.run_all(PROVIDER);
    let message = "example.com/lib didn't build: lib/lib.go:3 invalid operation: n * \"2\" (mismatched types int and untyped string)";
    let mut broken = run.broken.clone();
    broken.sort();
    assert_eq!(broken, [(Some("\u{1f}example.com/app".into()), message.replace("example.com/lib didn't", "example.com/app didn't")), (Some("\u{1f}example.com/lib".into()), message.into())]);
    fenix.assert_states(PROVIDER, &[(APP_DOUBLE, State::Errored), (LIB_DOUBLE, State::Errored), (TRIPLE, State::Failed)]);
    assert_eq!(fenix.at(PROVIDER, APP_DOUBLE), Some(("lib/lib.go".into(), 3, Some(33))), "where the error is, in the dependency");
}
