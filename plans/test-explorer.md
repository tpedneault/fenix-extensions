# The test explorer: design

> Milestone 4.1, written on 13 November 2026 for the maintainer to decide
> and agents to build from. Every claim about a runner comes from a run
> on this machine (the tools and versions are in [the facts](#2-the-facts)),
> every claim about the API from a file and line of Fenix's source at
> `b5c3765` or the `v1.5.0` tag, unless it says **not verified**. The
> sample projects and what each command printed are kept outside the
> repository, as fixtures for the agents who build this (see the
> [appendix](#appendix-what-the-runners-printed)). Nothing was run in
> Fenix: there's no Fenix binary where this was written.

## 1. What it is for, and what it isn't

You open a project and a page lists its tests, by file and suite, before
anything has run. A key on a test, a file or the whole tree runs it, and
the results come in as each test ends: a mark and a time on the page and
after the test's line in the file, a failure as a problem at the
assertion's line with the message beside the tree. One key runs the
failed again; another runs the test the cursor is in; another debugs it.
It works the same way for Rust, Python, Go and JavaScript, because the
page, the model and the keys are lang-kit's and a pack only says how its
runner lists, runs and reports.

Not part of it:

| | Why not |
|---|---|
| Coverage | Another tool, other output (lcov, `cargo llvm-cov`), its own page. A later milestone if wanted. |
| Watch mode (`vitest`, `jest --watch`, `cargo watch`) | A process that never ends doesn't fit a run with an end; run on save (§6) is the cheap part of it. |
| Writing tests | Fenix's agents already have `write_tests` ([AGENTS.md](https://github.com/tpedneault/fenix/blob/master/docs/AGENTS.md)). |
| Benchmarks | `cargo bench`, `go test -bench`: no pass or fail. They stay tasks. |
| Running in a terminal, or as Fenix's task | `SPC p t` and `SPC p r` already do that, and stay (§6). |
| A test runner of our own | The project's runner runs; the explorer reads what it says. |
| Snapshot updating (`-u`), retries, sharding | Flags a later release can add to a run's plan. |

## 2. The facts

Run on this machine, 13 November 2026:

| Tool | Version | Here |
|---|---|---|
| cargo, rustc | 1.97.0 | on `PATH` |
| cargo-nextest | 0.9.151 | not on the machine; `cargo install cargo-nextest --locked` worked (crates.io reachable) |
| Go | 1.24.7 | on `PATH` |
| pytest | 9.2.0 (Python 3.13) | not on the machine; installed in a scratch venv |
| Vitest | 5.1.0 (Node 22.23.4) | installed in a scratch project |
| Jest | 30.5.2 | installed in a scratch project |

All five runners were run. Each sample has passing, failing,
ignored/skipped, parameterised and nested tests, and a broken copy that
doesn't build or load. What isn't verified is marked so in the tables.

### cargo test (libtest, stable)

| | What it does | |
|---|---|---|
| Discovery | `cargo test --no-fail-fast <target> -- --list`: `name: test` lines, one block per test binary; which binary is said on **stderr** (`Running unittests src/lib.rs (...)`), the names on stdout. Ignored tests are listed like the rest, unmarked (`tests::ignored_one: test`); the scan sees their `#[ignore]`. | verified |
| Location | **None.** Names only (`geo::tests::unwraps_none`). The file comes from the module path and the target's root (`src/lib.rs`, `src/geo/mod.rs`); the line from scanning that file for `fn <name>`. A macro-made test (`cases! { case_negative: ... }`) has no `fn` to find. Doc tests' names hold both: `src/lib.rs - add (line 6)`. | verified |
| Location, unstable | libtest's JSON listing has `source_path`, `start_line`, `start_col` for every test, macro-made ones at the line of their case (`case_negative` at 45). It needs `-Z unstable-options`: `RUSTC_BOOTSTRAP=1` on cargo **rebuilds about 20 crates every run** (17 s on lang-kit, each time); `RUSTC_BOOTSTRAP=1` given only to the test binary through a runner (`--config "target.'cfg(all())'.runner=['env','RUSTC_BOOTSTRAP=1']"`) rebuilds nothing (0.20 s), but doc tests then refuse `-Z`, and `env` isn't a Windows program. | verified on Linux; Windows not verified |
| Cost | Discovery is a build of every test target. lang-kit (65 tests): 20.9 s cold, 0.18 s warm, 0.80 s after touching `src/lib.rs`. | verified |
| Run one | `cargo test --lib -- --exact tests::adds`. Without `--exact` a filter is a substring: `cargo test adds` ran `tests::adds` and `integration_adds`. | verified |
| Run a file / module | `cargo test --lib -- geo::` (a substring: `other::geo::` too); an integration file is its own target, `--test it`. | verified |
| Run all | `cargo test --no-fail-fast`. **Without `--no-fail-fast` cargo stops at the first failing binary**: the integration tests and doc tests didn't run. | verified |
| Run a set | Several names after `--exact` in one command: `cargo test --no-fail-fast -- --exact tests::case_negative geo::tests::unwraps_none integration_fails` ran exactly those three, across two binaries. A name in two binaries (`it_works` in `tests/a.rs` and `tests/b.rs`) would run in both. | verified |
| Results | Human only on stable: `test NAME ... ok`, `... FAILED`, `... ignored, slow`, `NAME - should panic ... ok`, one line as each test ends; then per binary a `---- NAME stdout ----` block for each failure, with its output and the panic, `thread 'NAME' (9756) panicked at src/geo/mod.rs:15:11:` and the message; then `test result: FAILED. 5 passed; 3 failed; 1 ignored; ...`. **No time per test** on stable (only per binary). | verified |
| Assertion's place | In the panic line: `src/lib.rs:25:9`. A macro-made test's is the macro call (`src/lib.rs:43:5`, the `cases! {` line), not its case. | verified |
| Streaming | One line per test as it ends; failures' detail only when the binary ends. | verified |
| Doesn't build | No `running N tests`; `error[E0308]` on stderr and exit 101. With `--message-format=json`, cargo's own lines on **stdout** start `{"reason":` (`compiler-message`, then `{"reason":"build-finished","success":false}`), apart from libtest's, which never start so. | verified |
| A binary crashes | `std::process::abort()` in a test: the tests that hadn't ended say nothing more; stderr says `process didn't exit successfully: ... (signal: 6, SIGABRT: process abort signal)`. | verified |

### cargo-nextest

| | | |
|---|---|---|
| Discovery | `cargo nextest list --message-format json`: every binary and test, `ignored` flags, **no location**. Builds, as `cargo test` does. No doc tests. | verified |
| Run | `cargo nextest run --no-fail-fast`, filters `-E 'test(=tests::adds)'`, `--run-ignored only`. Each test in a process of its own. | verified |
| Results | Human (`PASS [   0.006s] (8/10) orbit tests::panics_as_it_should`), with **times**; JUnit XML once the run ends, set by a config file: `--tool-config-file fenix:<path>` with `[profile.default.junit] path = "fenix-junit.xml"` wrote `target/nextest/default/fenix-junit.xml` without touching the project's `.config/`. The failure's `message` holds `panicked at src/geo/mod.rs:15:11`. Ignored tests aren't in it. `--message-format libtest-json` streams, only with `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`. | verified |

### pytest

| | | |
|---|---|---|
| Discovery | `pytest --collect-only -q`: node ids (`tests/test_basic.py::TestOuter::TestInner::test_deep`, `tests/test_basic.py::test_cases[strings [odd] id]`), each case of a `parametrize` its own. **No line.** Imports every test module (its top-level code runs); compiles nothing. 0.01 s on the sample; a real project not verified. | verified |
| Location | A plugin of ours (`fenix_tests.py`, 46 lines, loaded with `-p fenix_tests`) prints each item's `item.location`: file and the `def`'s line (`test_skipped` at 15, its decorator at 14). JUnit with `-o junit_family=xunit1` has `file` and `line` (0-based) but only after a run; the default `xunit2` has neither. | verified |
| Run | Node ids as arguments, any number: `pytest -q "tests/test_basic.py::test_cases[strings [odd] id]" tests/test_basic.py::TestOuter::TestInner` ran that case and that class. A file: its path. `-k adds` is a substring expression. No shell, so brackets and spaces need no quoting. | verified |
| Results | The same plugin prints a line per test as it ends (`pytest_runtest_logreport`): outcome, duration, the crash's file and line, the message, the traceback, captured output. The crash's place for a `unittest` assertion is in `unittest/case.py:900`, so the plugin also gives the deepest frame in the test's own file (`tests/sub/test_unit.py:9`). Without the plugin: `--junitxml` (end of run only). | verified |
| Streaming | Yes, a line per test. pytest's progress dots have no newline, so the plugin starts its lines with one (`\nFENIX-TEST {...}`). | verified |
| Doesn't load | A syntax error in one test file: exit 2, `Interrupted: 1 error during collection`, **nothing runs**, the plugin's `collect-error` line names the file. Exit 5 when no tests were collected. `python3 -m pytest` without pytest: `No module named pytest`. | verified |

### go test

| | | |
|---|---|---|
| Discovery | `go test -list '.*' ./...`: top-level names per package (`TestTable`), no subtests (they exist only when run), **no location**; builds every test binary: 8.4 s cold, 0.84 s warm for the standard library's `./encoding/...` (17 packages, 409 tests). `go list -f '{{.ImportPath}} {{.Dir}} {{.TestGoFiles}} {{.XTestGoFiles}}' ./...` gives each package's test files without building, even when they don't compile. | verified |
| Location | Scanning the test files for `func TestXxx(t *testing.T)` (and `Example`, `Fuzz`): exact, instant. Subtests have none of their own; a failure says `orbit_test.go:37: got -2, want -3` (the file's name, not its path). | verified |
| Run | `go test -json -run '^TestTable$/^negative_one$' .` ran one subtest. `-run` is a regex per `/`-separated level: a subtest named `"negative one"` is reported as `negative_one`, and one named `"a/b"` as `TestTable/a/b`, which `-run` reads as two levels (`'^TestTable$/^a$/^b$'` matched it). `go test -run '^(TestAdds\|TestSkipped)$' ./...` runs a set in every package. Passed results are cached (`(cached)`); `-count=1` runs again. | verified |
| Results | `go test -json`, built in: `run`, `pass`, `fail`, `skip` events with `Package`, `Test`, `Elapsed` (seconds), and `output` events with each line printed. Subtests are their own events. | verified |
| Streaming | Yes, events as they happen; packages run side by side and interleave, each event naming its package. A panic ends its package's binary: the later tests in it don't run. | verified |
| Doesn't build | Go 1.24: `{"Action":"build-output",...}` lines with the compiler's errors (`geo/geo_test.go:6:13: cannot use "3" ...`), `build-fail`, then the package's `fail` with `"FailedBuild"`. The other packages still ran. Before 1.24 the errors are text on stderr: **not verified**. | verified (1.24.7) |

### Vitest

| | | |
|---|---|---|
| Discovery | `vitest list --json`: each test's `name` (`outer > inner > deep`), `file` and `location` (line, column) by default in 5.1. A `test.each` is listed once, as its template (`add(%i, %i) is %i`); `skip` and `todo` tests aren't listed. Loads the test files; 0.27 s on the sample. | verified (5.1.0) |
| Run | `vitest run src/orbit.test.js:26`: by file and line, several at once (`... :8 ... :26 src/geo.test.ts:5`). `-t` is a regex on the full name joined by ` > ` (`-t 'outer > inner'`; `'outer inner'` matched nothing). A file filter is a substring of its path. | verified |
| Results | `--reporter=json --outputFile=x` at the end: per test `status`, `duration` (ms), `failureMessages` with the stack (`at .../src/orbit.test.js:10:21`), no location. A reporter of ours (`fenix-vitest.mjs`, 31 lines; `--reporter=default --reporter=<file> --includeTaskLocation`) prints every test collected, `skip` and `todo` and each case of `test.each` included, then each result as it ends with its errors. | verified |
| Streaming | With the reporter, yes. | verified |
| Doesn't load | A syntax error: the reporter's `module-error` line for that file; the others run. | verified |

### Jest

| | | |
|---|---|---|
| Discovery | `--listTests` lists **files only**. A run whose `-t` matches nothing (`-t FENIX_NOTHING`) with `--testLocationInResults` reports every test as `pending` with its line, each case of `test.each` expanded, without running one: 0.46 s on the sample, exit 0. It loads every test file. | verified |
| Run | A file: its path; tests: `-t` is a regex on the full name joined by spaces (`'^outer inner deep$'`), across every file given. No file:line filter. A name's regex characters need escaping (`add\(-1, -1\)`). | verified |
| Results | A reporter of ours (`fenix-jest.js`, 22 lines; `--reporters=default --reporters=<file> --testLocationInResults`): `onTestCaseResult` as each test ends (state, duration, failure with the stack `at Object.toBe (/.../src/orbit.test.js:9:21)`); tests that didn't run only at the file's end (`onTestResult`). `--json --outputFile` at the end. `--ci` keeps ANSI out of the messages. | verified (30.5.2) |
| Streaming | Yes, per test. | verified |
| Doesn't load | The reporter reads `testExecError`. | **not verified** |

### Locations, in short

| Runner | Each test's file:line from | Exact? | Builds? |
|---|---|---|---|
| cargo test | Scanning the source (module path, `fn`), checked against `--list` after a build | Yes for `#[test] fn`; no line for macro-made tests | Scan no; `--list` yes |
| pytest | Our plugin's `item.location` | Yes, the `def` | No (imports) |
| go test | Scanning `*_test.go`, packages from `go list` | Yes for top-level; subtests at their parent's | No |
| Vitest | `vitest list --json`; our reporter for the rest | Yes; `test.each` cases at the template's line | No (loads) |
| Jest | A run with `-t` matching nothing, through our reporter | Yes; cases at the template's line | No (loads) |

### What the API gives the extension

| Route | The extension reads the output? | Notes |
|---|---|---|
| `run_in_background(name)` | **No.** The output goes to the `*name*` buffer only (`fenix-gui/src/app/wasm_host.rs:2105-2190`); `Event::TaskFinished` is `(id, bool)` (`world.wit:992` at `v1.5.0`). No `last_task` for it: that's set for the task pane alone (`app.rs:11113-11119`), and so is Fenix's notification (`notifications.rs:364-398`). | At most 4 (`fenix-wasm/src/host.rs:84-85`, `1927`). Cancelling kills the whole tree: it runs as a process-group leader (`fenix-tasks/src/supervisor.rs:86-89`). |
| `run_as_task(name)` | **No.** `last_task()` gives only `(passed, failed)` (`world.wit:1113-1121`), read from cargo's `test result:`, pytest's `====` line, Jest's and Vitest's `Tests` line and `go test -v`'s `--- PASS` (`fenix-tasks/src/tests_count.rs:8-39`). | One task at a time, and it ends the one you started (`world.wit:1262-1264`). |
| A report file under `/data` | Yes, when the task ends: `${data}` in a command's args and env is the extension's folder (`fenix-wasm/src/host.rs:1165-1177`), which `std::fs` reads. | pytest's JUnit, nextest's JUnit, Vitest's and Jest's `--outputFile`, our reporters writing to a file. **Not cargo test, not go test**: their results are on stdout only (libtest's `--logfile` is overwritten by each binary: one line left after a whole run). |
| `Command::spawn()` | **Yes**, a line at a time: `Event::ProcessOutput { process, stderr, line }`, then `Event::ProcessExit` (`world.wit:473-477`, `981-983`). | No cap. stdout and stderr are read by two threads (`fenix-wasm/src/host.rs:1565-1580`), so their order between them isn't kept. No limit on a line's length (`BufRead::lines`). A line that isn't UTF-8 **stops that stream's reader** (`let Ok(line) = line else { break }`, `1567`). `ProcessExit` comes after both readers end (`1583-1585`). `process::kill` kills **the child only** (`2344-2350`, `Child::kill`): killing `cargo test` left its test binary running and the pipe open, and the output ended 6.2 s later, when the test did (an experiment with the same call outside Fenix). |

So: **only `Command::spawn` gives the extension a runner's results as
they come, for all five runners.** That's the route (§6). A run is then
not a Fenix task: it isn't in the task pane, Fenix doesn't notify when it
ends, `last_task` doesn't see it, and stopping one doesn't stop its
children.

### What Fenix already does with tests

| | Where | So the explorer |
|---|---|---|
| `SPC p r` runs the test the cursor is in, alone, in Task Output: Rust (`cargo test -p <pkg> -- <path> --exact`), pytest, Go | `FEATURES.md:1578-1583`, `fenix-tasks/src/test_at.rs` | leaves it as it is: a task, with the quickfix list and the 10-second notification. The explorer's own key records the result. |
| Code lens "▶ Run test" over the same three, runs as `SPC p r` | `FEATURES.md:270-275` (`lsp.code_lens` off by default) | doesn't draw a second ▶; its mark goes at the end of the line. |
| `run_test` for agents, the test at a line, waited for | `AGENTS.md:431` | offers agents what `run_test` doesn't: the list, the failures, a set (§8). |
| A task's test counts (`last_task`) and its notification | `tests_count.rs`, `notifications.rs:364-398` | isn't a task, so neither applies: it notifies itself (§8). |
| Debugging one test from a lens: "follow-up work" in Fenix's plan | `docs/plans/code-lens.md:392-398` | does it per pack through `debug::launches` (§7). |
| Nothing in `docs/plans/` plans a test page | `docs/plans/*.md` | doesn't collide today. |

The code that finds the test at a line, `fenix-tasks/src/test_at.rs`,
reads Rust's module path from the file's place and the `mod`s around it,
pytest's classes and Go's `func Test`. It isn't reachable from an
extension, and `fenix-tasks` brings process code that isn't for wasm, so
the Rust and Go scanners port its rules rather than depend on it.

## 3. The shape

The explorer is a module of lang-kit, `lang-kit::tests`, beside the
packages page. A pack gives `Kit` a `TestRunner` the way it gives a
`PackageManager`; the kit does the rest.

```rust
Kit::new("rust")
    .packages(Box::new(Cargo::new(..)))
    .tests(Box::new(CargoTest::new(..)))   // new
```

### What a pack implements

```rust
/// A runner's own name for a test, as its listing and its results both
/// write it: `lib tests::adds`, `tests/test_basic.py::test_cases[negative]`,
/// `example.com/orbit TestTable/negative_one`, `src/orbit.test.js > outer > deep`.
pub struct Key(pub String);

/// A test, or a group of them, as a runner or a scan finds it.
pub struct Found {
    pub key: Key,
    /// Its place in the tree, the root first: a target or package, the
    /// file, its suites, then the test: ["orbit (lib)", "src/lib.rs", "tests", "adds"].
    pub path: Vec<String>,
    pub file: Option<String>,        // under /project
    pub line: Option<u32>,           // from 1
    pub skip: Option<String>,        // #[ignore = "slow"], @skip, test.skip
    /// A template whose cases come only from a run: test.each, a
    /// parametrize the scan saw, a Go test with t.Run.
    pub template: bool,
}

/// What a runner said, line by line.
pub enum Report {
    Found(Found),
    Started(Key),
    Ended { key: Key, outcome: Outcome, ms: Option<f64>, failure: Option<Failure> },
    /// What a test printed (Go's output events, a reporter's captured output).
    Output { key: Option<Key>, text: String },
    /// Nothing of it ran: the code didn't build, a file didn't load. Never
    /// a test's failure.
    Broken { file: Option<String>, at: Option<(u32, u32)>, message: String },
    /// The process died: its tests that hadn't ended didn't run.
    Crashed(String),
}

pub enum Outcome { Passed, Failed, Skipped(Option<String>) }

pub struct Failure {
    pub message: String,                 // the assertion's, short
    pub at: Option<(String, u32, u32)>,  // where it failed: file, line, column
    pub text: String,                    // the whole of it: diff, traceback, output
}

/// What to run.
pub enum Selection<'a> { All, Files(Vec<&'a str>), Tests(Vec<&'a Test>) }

/// A line of a process's output, as `Event::ProcessOutput` gives it.
pub struct Line<'a> { pub text: &'a str, pub stderr: bool }

/// Reads one command's output; a new one for each command.
pub trait Reader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>);
    fn end(&mut self, code: Option<i32>, out: &mut Vec<Report>);
}

pub trait TestRunner {
    fn name(&self) -> &str;                                   // "cargo test", "pytest"
    /// Tests read from a file's text, nothing run: instant, on open and save.
    fn scans(&self, _path: &str) -> bool { false }
    fn scan(&self, _path: &str, _text: &str) -> Vec<Found> { Vec::new() }
    /// The runner's own listing, run in turn; may build.
    fn list(&self) -> Vec<Command>;
    /// The commands that run `what`, in turn.
    fn plan(&self, what: &Selection) -> Result<Vec<Command>, String>;
    fn reader(&self) -> Box<dyn Reader>;
    /// Whether `reported` is one of `template`'s cases (`add(-1, -1) is -3`
    /// of `add(%i, %i) is %i`). By default: none is.
    fn is_case(&self, _template: &Test, _reported: &Key) -> bool { false }
    /// One test as a launch for the debugger (§7).
    fn launch(&self, _test: &Test) -> Option<LaunchConfig> { None }
    /// Files its reporter needs, written under /data when the pack starts:
    /// ("pytest/fenix_tests.py", bytes).
    fn assets(&self) -> &[(&str, &[u8])] { &[] }
}
```

It's `PackageManager`'s pattern: commands out, parsed text in. The
difference is that a test run streams, so the parser is a `Reader` fed
line by line, not a function given the whole `Output` at the end.

### What the kit owns

| | |
|---|---|
| The model | Every test and group, by `Key`, in one arena; states, last results, failures (capped), times. |
| Discovery | Scans open and saved files at once; the runner's listing on start (when it doesn't build: pytest, Go's `go list`, Vitest, Jest) or after the first build (cargo); merging the two. |
| Runs | One at a time per project, a queue of one behind it, commands of a plan in turn, the `Reader`, cancellation. |
| The page | `tests`, built with ui-kit's `TreeState`, `TabsState`, `TextField` (the filter) and `ScrollState` (the details). |
| In the file | Decorations after each test's line, failures as problems merged with the linter's (§5). |
| Keys and commands | `<prefix>.tests`, `<prefix>.tests.run-here`, `.run-file`, `.run-all`, `.run-failed`, `.run-again`, `.debug-here`, `.stop` (§4). |
| Telling you | The modeline item, the notification, the agent tools' services (§8). |
| The debug hand-off | The launch from `TestRunner::launch`, set and started (§7). |
| State | `page_state`, results under `/data/lang-kit/tests/` (§9). |

### The data model

```rust
pub struct Test {
    pub key: Key,
    pub kind: Kind,                // Group(Target | Package | File | Suite) | Test | Case
    pub label: String,             // the last part of its path
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub skip: Option<String>,
    pub template: bool,
    pub seen: Seen,                // Scanned | Listed | Ran: who has said it exists
    pub state: State,
    pub stale: bool,               // its file was saved after its last result
    pub last: Option<Ended>,       // outcome, ms, failure, when
}

pub enum State { Unknown, Queued, Running, Passed, Failed, Skipped, NotRun(String) }
```

- **States.** Unknown until it runs. Queued when a run takes it, Running
  when the runner says it started (or, for a runner that doesn't say,
  cargo's human output, when its command starts), then Passed, Failed or
  Skipped. `NotRun(why)` when a run that took it ended without a word on
  it: the build failed, the file didn't load, the binary crashed, the
  run was stopped. **Stale** is a flag over the last result, not a state
  of its own: the mark stays, dimmed, so you still see what it was.
- **A group's state** is worked out from its children: failed if any
  failed, running if any runs, passed if all that ran passed.
- **Parameterised cases** are `Case`s under their template: pytest's
  `test_cases[negative]` (its own id, listed before a run), Vitest's and
  Jest's `test.each` (Jest lists them, Vitest only once they run), Go's
  subtests (only once they run). A case found in a run is added and
  kept; a template's cases from an older run go when it runs again.

### Matching results to what was discovered

The listing and the results don't always name a test the same way, so
the kit matches in this order, and stops at the first that fits:

1. **The same `Key`.** Each pack writes its keys so that its listing and
   its results agree: Rust `"<target> <path>"` (`lib geo::tests::unwraps_none`,
   `test:it integration_fails`, `doc src/lib.rs - add (line 6)`), pytest
   the node id, Go `"<import path> <name>"`, Vitest and Jest `"<file> > <names>"`.
2. **A case of a template**, by `TestRunner::is_case`: Vitest's
   `add(%i, %i) is %i` against `add(-1, -1) is -3` (its `%` and `$`
   placeholders as wildcards).
3. **Under a test it starts with**: Go's `TestTable/negative_one` under
   `TestTable`, pytest's `test_cases[x]` under `test_cases`. Added as a
   `Case`, at its parent's line.
4. **Nothing fits**: a test the scan couldn't see (a macro-made Rust
   test, a test in a file the scan doesn't read). Added under its file
   (from its key, or its failure's place), with no line, marked "found
   in a run".

A scanned test that a full listing or a full run doesn't name is dropped
(a `#[cfg(feature = "x")]` module that wasn't built), unless its file has
been saved since: the scan is newer than the run.

## 4. The page and the keys

The page is `tests`, opened by `<prefix>.tests`. Wide, the tree is on the
left and the focused test's details on the right; narrow (under 90
columns), the details go under the tree. `›` below marks the focused row,
which Fenix draws as a background.

At 100×30, pytest's sample after a run:

```text
Tests  pytest   ✓ 7  ✗ 5  ○ 2  ? 12      ran at 14:02 in 0.4 s · 1 file saved since
────────────────────────────────────────────────────────────────────────────────────────────────────
1 All 26   2 Failed 5   3 Not run 14

▾ tests/sub/test_unit.py             ✗ 1      0.2 ms │ test_fails_on_purpose                ✗ failed
  ▾ TestUnit                         ✗ 1      0.2 ms │ tests/test_basic.py:12 · 0.3 ms · ran 14:02
      test_bad                       ✗        0.1 ms │
      test_ok                        ✓        0.1 ms │ AssertionError: two and two
▾ tests/test_basic.py                ✗ 4      1.6 ms │ assert 4 == 5
    test_adds                        ✓        0.0 ms │  +  where 4 = add(2, 2)
  › test_fails_on_purpose            ✗        0.3 ms │
    test_skipped                     ○ slow          │     def test_fails_on_purpose():
    test_known_bug                   ○ xfail         │         print("some output before the fai…
  ▾ test_cases                       ✗ 1      0.6 ms │ >       assert add(2, 2) == 5, "two and two"
      small                          ✓        0.0 ms │ E       AssertionError: two and two
      negative                       ✗        0.4 ms │
      strings [odd] id               ✓        0.0 ms │ Captured stdout
  ▾ TestOuter                        ✗ 1      0.3 ms │ some output before the failure
      test_in_class                  ✓        0.0 ms │
    ▾ TestInner                      ✗ 1      0.1 ms │
        test_deep                    ✗        0.1 ms │
    ▸ test_param_in_class            ✓ 2      0.1 ms │
    test_uses_broken_fixture         ✗ setup  0.1 ms │
▸ tests/test_cli.py                   ? 12           │
                                                     │
                                                     │
                                                     │
────────────────────────────────────────────────────────────────────────────────────────────────────
r run   R all   f the failed   d debug   ⏎ go to it   o its output   / filter   x stop
[ ] failures   h l close, open   + - all   y copy the failure   SPC m t … from any file
```

At 80×24, Rust's sample (no times: stable libtest has none per test):

```text
Tests  cargo test   ✓ 6  ✗ 4  ○ 1  ? 1   ran at 14:02 in 1.9 s
────────────────────────────────────────────────────────────────────────────────
1 All 12   2 Failed 4   3 Not run 2
▾ orbit (lib)                                                                ✗ 3
  ▾ src/geo/mod.rs  geo::tests                                               ✗ 1
      dist_is_symmetric                                                        ✓
      unwraps_none                                                             ✗
  ▾ src/lib.rs  tests                                                        ✗ 2
      adds                                                                     ✓
    › case_negative                                                            ✗
      case_small                                                               ✓
      fails_on_purpose                                                         ✗
      ignored_one                                                         ○ slow
      panics_as_it_should                                                      ✓
    ▸ nested                                                                 ✓ 1
────────────────────────────────────────────────────────────────────────────────
tests::case_negative                                                    ✗ failed
src/lib.rs:43 (the cases! macro that makes it)
assertion `left == right` failed
  left: -2
 right: -3


r run  R all  f failed  d debug  ⏎ go  o output  / filter  x stop  [ ] next
```

- **The head** says the runner, the counts (✓ passed, ✗ failed, ○
  skipped, ? not run yet), when the last run was and how long it took,
  and how many files were saved since. While a run goes: `running 12 of
  40 · ✗ 1 · x stops it`. A build that failed: `the build failed:
  src/lib.rs:19 mismatched types · o shows it`, in red, the marks kept
  as they were.
- **The tree**: a group's detail is its failures, else its count; a
  test's its mark and time. Groups start closed except those holding a
  failure, so a large project opens on what's wrong.
- **The tabs**: All, Failed (the failed and their groups), Not run
  (unknown, skipped, not run). `1` `2` `3`.
- **The details**: name, place, time, when; the failure's message; the
  rest of it (diff, traceback, output) scrolled with `J` `K` when the
  details have the focus (Tab moves it there).

### Page keys

The same with Vim keys and standard keys: a page's letters act where
you can't type (`WRITING_EXTENSIONS.md:1088-1090`).

| Key | Does | On |
|---|---|---|
| `j` `k` ↑ ↓, `g` `G`, `J` `K` | Move, first, last, a screen | tree (ui-kit's `TreeState`) |
| `l` →, `h` ← | Open, close or go to the parent | tree |
| `+` `-` | Open all, close all | tree |
| Enter | Go to the test's line (`open_file`); on a group, open or close it | tree |
| `r` | Run the focused test, case, file or group | |
| `R` | Run all | |
| `f` | Run the failed again | |
| `d` | Debug the focused test (§7) | test |
| `x` | Stop the run | while one runs |
| `o` | The whole output of its last run, beside (`open_text_beside`) | |
| `y` | Copy the failure: name, place, message | failed test |
| `[` `]` | The previous, next failure | |
| `/` | Filter by name (a `TextField`; Esc clears, Enter keeps) | |
| `1` `2` `3` | The tabs | |
| Tab | To the details and back (`J` `K` scroll them there) | |
| `q`, Esc | Close the page (Fenix's) | |

Checked against what's taken: on a page Fenix keeps `q`, Esc, Space (the
leader) and its chords (`WRITING_EXTENSIONS.md:81`, `90`); lang-kit's
packages page uses `u U a d r` and Enter (`crates/lang-kit/src/page.rs:559`)
on its own page, so `r` and `d` mean other things on the two pages:
"refresh" and "remove" there, "run" and "debug" here. That's the one
overlap, and it's between two pages, not on one.

### Leader keys: a `SPC m t` group

In a file of the pack's language, its `SPC m` menu gets a group `t`,
"tests"; the same group is on the tests page (a context with `pages =
["tests"]`, `WRITING_EXTENSIONS.md:1031-1036`). With standard keys it's
`Ctrl+K m t …`: Fenix rewrites the leader (roadmap, milestone 3).

| Keys | Does | Command |
|---|---|---|
| `SPC m t t` | Run the test the cursor is in | `<prefix>.tests.run-here` |
| `SPC m t f` | Run this file's tests | `<prefix>.tests.run-file` |
| `SPC m t a` | Run all | `<prefix>.tests.run-all` |
| `SPC m t r` | Run the failed again | `<prefix>.tests.run-failed` |
| `SPC m t l` | Run the last run again | `<prefix>.tests.run-again` |
| `SPC m t d` | Debug the test the cursor is in | `<prefix>.tests.debug-here` |
| `SPC m t o` | Open the tests page | `<prefix>.tests` |
| `SPC m t k` | Stop the run (as `SPC p k` ends a task) | `<prefix>.tests.stop` |

What's under `SPC m` now, in every language pack
(`extensions/*/keymaps/*.toml`):

| Pack | Keys | Collides with `t` |
|---|---|---|
| Rust | `p` `d` `c` | no |
| Python | `p` `d` `s` | no |
| Web | `p` `d` `i` `s` `S` | no |
| **Go** | `p` `d` **`t` tidy** | **yes** |
| .NET, Java (4.4) | `t` "debug the tests" | yes, and it's a test key: it becomes `t d` |
| Protobuf | `t` "pick the server" | no tests; not a pack the explorer reaches |
| Ada, Config, C/C++, Haskell, Lua, OCaml, PHP, Ruby, Swift, Zig | no `t` | no |

So Go's tidy has to move, or the group goes somewhere else: a decision
(§11, D6). Fenix's own `SPC p r` (run the test at the cursor as a task)
and `SPC p k` stay Fenix's: nothing here rebinds a Fenix key. A local
key may be a sequence and an entry may be a `group`
(`fenix-gui/src/keymap.rs:267-289`). `key_for` answers nothing for a
`SPC m` key (roadmap, milestone 3), so the page's hint at the foot
("SPC m t … from any file") is written from the leader as Rust's
`keys.rs` does.

No chords of its own are proposed: VS Code's `Ctrl+; A` style of chord
would be a flavour's business, and `Ctrl+;` is a leader some users pick
(`crates/test-kit/README.md`, "Keys of the scheme in use").

### With 10,000 tests

- **The tree** is built only as far as it's open. `TreeState::rows`
  walks the open branches (`crates/ui-kit/src/tree.rs:107-120`), so
  closed files cost nothing; the kit makes `TreeNode`s for open
  branches only, again only when the model changes, not on each frame.
- **The filter** matches names as you type; 10,000 short strings is
  well under a millisecond natively (an estimate, **not measured** in
  wasm). Matches show with their groups opened.
- **Counts** are kept per group as results come, not summed per frame.
- **A run of all** sends one `Event::ProcessOutput` per line: 10,000
  results is at least 10,000 calls (§9).
- **Decorations**: at most 10,000 a buffer (`world.wit:1446-1453`); a
  file with more tests than that gets marks on the first 10,000. Not a
  real case.

## 5. In the file

### Marks after a test's line

`decorations::set(file, ..)` with `Decoration::after((line, last column),
" ✓ 3 ms", Role::Good)` (`fenix-extension-api/src/lib.rs:1545` at
`v1.5.0`) on each test's first line: the text is drawn after the line's
end, taking room, moving with your edits until it's set again
(`world.wit:736-745`, `763-771`).

| State | After the line |
|---|---|
| Passed | ` ✓ 3 ms` in good (Rust on stable: ` ✓`) |
| Failed | ` ✗ assertion failed: two and two` in bad, cut at 48 characters |
| Running | ` ●` in accent |
| Queued | ` ·` muted |
| Skipped | ` ○ slow` muted |
| Stale | the last mark, muted |
| A group (a `mod tests`, a `describe`, a class) | ` ✓ 12  ✗ 1` |

- **Only open buffers** take decorations: for a file that isn't open
  Fenix says `decorations-set: ... isn't open` on the output page and
  keeps nothing (`fenix-gui/src/app/wasm_host.rs:3217-3220`, its test at
  `5455`). So the kit sets a file's marks when it comes into view
  (`Event::FocusChanged`, which every extension gets) and after each
  result in an open file, at most every 250 ms by a timer, not per line.
- The last column comes from the buffer as it is (`buffer::text`, which
  `editor = "read"` gives: every pack has it).
- **A mark can be clicked**: `Decoration::tagged(id)` comes back to
  `decoration_clicked` (`lib.rs:1556`, `world.wit:1708-1710`), so a click
  on ` ✗` runs that test again. Said in the README, not relied on.
- **No gutter marks.** A decoration's looks are text, a swatch, hidden
  text, a tint and an underline (`world.wit:747-761`); nothing draws in
  the gutter (`grep gutter world.wit`: nothing). What to ask of Fenix:
  a `DecorationLook::Gutter { glyph, role }` at a line, drawn where
  breakpoints are, clickable like a tagged decoration. Until then the
  mark sits at the line's end. Gutter marks for extensions are already
  in the roadmap's "needs Fenix" list.

### Failures as problems

A failed test's failure is a problem at its assertion's line:
`Problem::error((line, col), (line, col + 1), message).source("cargo test").code(name)`
(`lib.rs:1685-1704`). Its place is the `Failure::at` the pack read (Rust's
panic line, pytest's deepest frame in the test's file, Go's
`orbit_test.go:37`, Jest's and Vitest's first stack frame in the project),
else the test's own line. Not in the project (a library's frame), the
test's own line.

- **Fenix keeps one list per extension per file**: `diagnostics::set`
  is "in place of those it gave before", stored under the extension's
  own key (`wasm_host.rs:2211`, `2262-2264`). The pack's linter (clippy,
  ruff, golangci-lint, ESLint's checks run by the pack) and the explorer
  are **the same extension**, so whichever sets a file's problems last
  wipes the other's. lang-kit therefore gets a ledger, `Problems`: each
  source (`lint:clippy`, `tests`) puts its problems for a file, and the
  kit sets the union. `Lint` moves onto it. This is the one change the
  explorer makes to code every pack uses already (§12, piece 1).
- The language servers' problems are theirs (another key), shown beside.
  A failure shows once per test; the same line failing in two tests (a
  helper's assertion) shows twice, each with its test's name as `code`.
- **Cleared** when that test runs again, whatever the result; when its
  file is saved (the line may not be the line any more); when the test
  is gone from the model. Not when another test runs.
- No fixes: a failure has no edit to offer.
- Build errors and files that don't load aren't set as problems: the
  language server and the linter already show those, and twice is noise.
  The page says them (§6).

### The test the cursor is in

`<prefix>.tests.run-here` reads `here()` and `cursor()` (both with
`editor = "read"`; `lib.rs:830`, `1588`): the file, and the line. The
kit takes the file's tests from the model (scanned on open, so a new,
unsaved test counts once it's been scanned from the buffer's text) and
picks the last test whose line is at or above the cursor, within the
test's group (a `describe`, a class, a `mod`). Above every test, or in a
group's line: that group. Not in a test file: the tests whose names
mention the file's stem (`geo.rs` → `geo::tests`, `orbit.py` →
`tests/test_orbit.py`), else it says so. A test's end isn't known, so a
cursor between two tests counts as the first's: the same rule as Fenix's
`test_at.rs` for Rust.

## 6. Running

### The route

Every run and every listing is `Command::spawn`, read line by line by
the runner's `Reader` (§2, the API table). Not `run_in_background`:
its output can't be read, and cargo and Go can't write a report file.
One route for all five keeps one lifecycle, one set of tests, one way
to stop.

Each pack's commands, with what's verified:

| Pack | List | Run (plan) | Reads |
|---|---|---|---|
| Rust | scan; after a build `cargo test --no-fail-fast --message-format=json <target> -- --list` per target, `--doc -- --list` for doc tests | per target, in turn: `cargo test --no-fail-fast --message-format=json -p <pkg> --lib -- --exact <names>` (`--test x`, `--bin y`, `--doc`); all: one per target | libtest's human lines on stdout; cargo's `{"reason":...}` lines on stdout for the build and the test binaries' paths |
| Rust with nextest (D2) | the same scan | `cargo nextest run --no-fail-fast --tool-config-file fenix:${data}/nextest.toml -E 'test(=a) \| test(=b)'` (one `test(=...)` tried; the `\|` not) | its human lines (times), then its JUnit for the failures' text |
| Python | `python -c <boot> --collect-only -q -p fenix_tests` | `python -c <boot> -q -p fenix_tests <node ids or files>` (through `uv run` when the pack's tasks use it) | the plugin's `FENIX-TEST {json}` lines |
| Go | scan, packages from `go list -f ...` | `go test -json -run '^(A\|B)$' <package dirs>`; a subtest `'^TestTable$/^negative_one$'` | `go test -json` events |
| Vitest | `vitest list --json` | `vitest run --includeTaskLocation --reporter=default --reporter=${data}/fenix-vitest.mjs <file:line ...>` | the reporter's lines |
| Jest | `jest --ci --testLocationInResults --reporters=${data}/fenix-jest.js -t FENIX_NOTHING` | `jest --ci --watchAll=false --testLocationInResults --reporters=default --reporters=${data}/fenix-jest.js <files> -t '^(a\|b)$'` | the reporter's lines |

`<boot>` is `import sys; sys.path.insert(0, r'${data}/pytest'); import
pytest; raise SystemExit(pytest.main(sys.argv[1:]))`: it finds the plugin
without setting `PYTHONPATH`, which would hide yours (verified; the raw
string is for a Windows path's backslashes, **not verified on Windows**).
The reporters are files of the pack's, written to `/data` when it starts
(`TestRunner::assets`). Each runner's program is one the pack's manifest
already allows (`cargo`, `python3`/`uv`, `go`, `node`); the reporters
need none. `vitest` and `jest` above are shorthand: Web runs the
project's own, `node node_modules/vitest/vitest.mjs` and `node
node_modules/jest/bin/jest.js`, as its launch files do
(`extensions/web/launch/vitest-file.toml`, `jest-file.toml`).

### Its life

```text
     a key            the runner says it started     it ended
  ─────────►  Queued ──────────────────────► Running ─────────► Passed / Failed / Skipped
                 │                                │
                 └─ the build failed, the file    └─ the binary crashed, you stopped it,
                    didn't load, you stopped it      the process ended without a word on it
                                   └──────────► NotRun(why) ◄──┘
```

- **One run at a time per project.** cargo locks its build folder, and
  two runs of the same tests would fight over the marks. A request while
  one runs waits, the page saying `then: 3 tests`; another request joins
  that one (the union), so at most one waits. `f` and `R` while the same
  run is going are ignored.
- **The 4-task cap doesn't apply**: spawned programs aren't background
  tasks (`host.rs:1924-1934` counts only those). What the pack runs in
  the background (Web's `web.dev`, a server's install) isn't touched,
  and a test run doesn't take a slot from them.
- **Discovery runs** when the pack starts (if it doesn't build: Python,
  Go's `go list`, Vitest, Jest), after a run that built (Rust's `--list`,
  free once built: 0.18 s), and when a test file is saved (its scan).
  Never while a run goes.
- **Stopping** (`x`, `SPC m t k`, or a run replaced): `process::kill`,
  and the kit marks what hadn't ended `NotRun("stopped")` at once,
  ignoring the process's later lines. But `kill` stops only the program
  it started, not what that started (§2): a cargo test binary, Go's
  test binaries, Vitest's and Jest's workers run on until they end. The
  next run waits for the old one's exit (`Event::ProcessExit` comes only
  then), and the page says `stopping…` meanwhile. A fix needs Fenix
  (D5).

### When it doesn't build, or a file doesn't load

Never as test failures. Told apart by what each runner says:

| Runner | Says | The explorer |
|---|---|---|
| cargo test | `{"reason":"build-finished","success":false}`, `compiler-message`s with `level: error` (verified) | `Broken`: the head says the first error and its place; queued tests go back to what they were; `o` shows the build's messages. No problems: rust-analyzer and clippy have them. |
| pytest | the plugin's `collect-error` line, exit 2 (verified) | The file's row says `couldn't load: SyntaxError at line 1`; its tests `NotRun`. The other files' tests didn't run either (pytest stops), so they go back to what they were. |
| go test | `build-output`, `build-fail`, a package `fail` with `FailedBuild` (Go 1.24, verified) | The package's row says it didn't build; the other packages' results stand. Before Go 1.24 (stderr text): **not verified**; read as "didn't build" when a package fails with no test events. |
| Vitest | the reporter's `module-error` (verified) | The file's row, as pytest's. |
| Jest | `testExecError` in the reporter (**not verified**) | The same. |
| A crash | Rust: tests without a result and `process didn't exit successfully ... (signal: 6 ...)` on stderr (verified); Go: a panic's `fail` for the test, the package's later tests absent (verified) | Those without a result: `NotRun("its binary crashed: SIGABRT")`. |

Rust's case in one line: cargo's JSON messages and libtest's lines share
stdout, and cargo's always start `{"reason":` (verified); compile errors
are only ever in those.

### The failed again, staleness, on save

- **The failed again** (`f`, `SPC m t r`): the tests whose last result is
  failed, as one selection. Each pack's plan makes it as few commands as
  it can (Rust one per target, Go one per run with an alternation over
  the packages that have failures, pytest one with the node ids, Vitest
  one with `file:line`s, Jest one per file group with an anchored
  alternation). A template's failed cases run their template's case only
  where the runner can name it (pytest's ids, Go's subtest path); else
  the template.
- **Stale**: saving a file marks the tests in it stale (the mark dims,
  the head counts the files saved since); a scan of the saved text adds
  and removes tests at once. A save elsewhere doesn't mark anything
  stale: knowing what a change reaches is the runner's job, not ours.
- **Run on save: off by default**, a setting `<prefix>.tests.on_save`
  with `off`, `file` (the saved file's tests) and `failed` (the failed
  again). Off, because a Rust or Go run builds, and the pack already runs
  clippy or a vet on save in the same build folder: two builds on every
  save would make both slow (cargo's lock: known behaviour, **not
  measured here**). On, a save's run replaces a waiting one.

## 7. Debugging a test

| Pack | Today | The explorer adds |
|---|---|---|
| Rust | `launch/test.toml`, "Rust: tests in this file": lldb-dap runs `src/debug/fenix_cargo.py`, which builds `cargo test --no-run` for the file's target and passes the module as a filter (`only`). Every test of the file. | A launch for one test: `program` the test binary (`compiler-artifact`'s `executable` from the last run's JSON; its hash holds while the profile and features do), `args = [name, "--exact", "--nocapture"]`, `preLaunchTask` the `cargo test --no-run` task for its target. |
| Python | `pytest: all tests`, `pytest: current file` (`module = "pytest"`, `args = ["-q", "${file}"]`) | `args = ["-q", "<node id>"]`. |
| Go | `Go: test this package` (`mode = "test"`, `program = "${fileDir}"`, "add `-test.run` for one") | `args = ["-test.run", "^TestTable$/^negative_one$", "-test.v"]`. |
| Web | `Vitest: current file` (`run ${file}`), `Jest: current file` (`--runInBand --watchAll=false ${file}`) | Vitest `args = ["run", "<file>:<line>"]`; Jest `args = ["--runInBand", "--watchAll=false", "<file>", "-t", "^<name>$"]`. |

How it's handed over, with what the API has:

1. `debug::launches` gets the pack's launches plus one more, the same
   name every time: "<Language>: the test from Tests". What changes from
   test to test goes in a variable, `${fenix_test}` in its args, which
   the kit fills with `debug::variable("fenix_test", &[one choice])`: one
   choice is its value, with nothing asked (`WRITING_EXTENSIONS.md:497-501`).
   Rust's `program` changes too (the test binary), so Rust sets its
   launches again after a build.
2. The test's file is opened at its line (`open_file`): Fenix picks the
   adapter by the focused buffer's language (`app.rs:11166`).
3. `host::run_command("debug.pick_launch")`: the launch picker, with
   "the test from Tests" in it. Once you've picked it, Fenix remembers it
   for the project by name (`app.rs:11172-11176`), so later the kit runs
   `debug.start_or_continue`, which starts it at once.

It's fragile, and says so in the README: after you debug something else,
`debug.start_or_continue` starts that instead, so the kit always uses
`debug.pick_launch` unless the last one it started was its own; and a
project with its own launch in `.fenix/tools.json` never sees the
extensions' launches (`app.rs:11168-11169`). What to ask of Fenix:
`debug::start(&LaunchConfig)`, a session started from a launch the
extension gives, no picker (D5). **None of this was run in Fenix.**

## 8. Telling you

### The modeline

`status(Some(..))` (`lib.rs:1481`): while a run goes, `tests 12/40 ✗ 1`;
after one with failures, `tests ✗ 3` until they pass; nothing when all
passed, so a green project is quiet. A click isn't offered by `status`.

### At the end of a long run

A run is a spawned program, not a task, so **Fenix doesn't notify when
it ends**: its notification is for the task pane's tasks only
(`notifications.rs:364-398`, called from `finish_task_run`,
`app.rs:11121-11123`). There's no second toast to avoid; the explorer
gives the only one, and keeps Fenix's rule:

| When | Summary | Body | Urgency | Tag | Actions |
|---|---|---|---|---|---|
| A run of 10 s or more ended, and the tests page isn't in view (`Event::PageShown`) | `Tests passed` / `3 tests failed` | `41 passed in 1 min 12 s` / `tests::case_negative and 2 more · 41 passed` | low / normal | `tests` | *Show the failures* (`<prefix>.tests`), *Run the failed again* (`<prefix>.tests.run-failed`) |
| The build failed | `The tests didn't build` | its first error | normal | `tests` | *Show it* |

The tag replaces the last one (`WRITING_EXTENSIONS.md:112`). Whether to
read Fenix's own `notifications.tasks` (all, long, failed, never) needs
the extension to read another's setting, which `permissions.settings`
gives for extensions' settings (`WRITING_EXTENSIONS.md:992`); for
Fenix's: **not verified**. Until then the rule is fixed at "long", and
`notifications.rules` (by the extension's id) mutes it.

`SPC p r` and `SPC p t` stay tasks, so they keep Fenix's notification;
the explorer doesn't hear them (it could, through `last_task`, but only
the counts).

### Tools for agents

Fenix's `run_test` already runs the test at a line and waits
(`AGENTS.md:431`). The explorer adds what it lacks: the list, the
failures with their places, and a set. One service each, in the kit; each
pack's manifest lists them (`<prefix>` is the pack's: `rust_tests_list`).

| Tool | Class | Input | Answers | Default |
|---|---|---|---|---|
| `tests_list` | read | `file?`, `state?` (`failed`, `not_run`, ...), `filter?` | Tests with key, file, line, state, time; at most 500, saying how many more | yes |
| `tests_failures` | read | `file?` | Each failure: key, file:line, message, the failure's text (cut at 4 KiB each, 32 KiB in all) | yes |
| `tests_run` | run | `keys?`, `file?`, `failed?`, `all?` | Waits for the run (`serve` answers `None`, then `answer(call, ..)`), then the counts and the failures as `tests_failures` | no |

- `tests_run` is asked about by default (the `run` class,
  `AGENTS.md`, "What they may do"); `timeout_ms` 600000, the most a tool
  may wait (`WRITING_EXTENSIONS.md:796`); `service_cancelled` stops the
  run. A description under 300 characters and a schema under 2 KiB each
  (`WRITING_EXTENSIONS.md:811-814`).
- Its description says "for the one test at a line, Fenix's `run_test`
  does it too", so an agent isn't torn between two.
- Answers name only the project's tests and what they printed: nothing
  else of yours.

## 9. State and performance

| What | Where | Why there |
|---|---|---|
| The page: tab, filter, open groups, focused row | `page_state` (asked after each key, `WRITING_EXTENSIONS.md:82`) | Small; comes back with the session |
| The last results: key, state, ms, when, failures cut to 2 KiB | `/data/lang-kit/tests/<project hash>.json`, written at a run's end | So a restart shows what failed before, marked stale until it runs again |
| The reporters (`fenix_tests.py`, `fenix-vitest.mjs`, `fenix-jest.js`), nextest's config | `/data/lang-kit/tests/runner/` | Written at start from the pack's assets |

Another project is another start of the extension
(`WRITING_EXTENSIONS.md:201`), so one project's model is in memory at a
time; the file name keeps projects apart.

**Memory** (128 MiB, `WRITING_EXTENSIONS.md:1127`): a test is about
300 bytes in the model; 10,000 is 3 MB. A failure's text is kept to
16 KiB, and the run's whole output to 8 MiB (the oldest lines go), which
`o` shows; past that it says what was dropped.

**A call returns in milliseconds** (`WRITING_EXTENSIONS.md:1116-1126`):

- Each output line is its own event, so parsing is chunked for free: a
  line is one `serde_json` value (pytest, Go, Vitest, Jest) or one
  pattern (libtest), microseconds natively (**not measured** in wasm).
  What each line costs in Fenix's queue is **not measured**: a run of
  100,000 lines is 100,000 calls.
- The page is refreshed at most every 100 ms (a timer), the file's
  decorations every 250 ms; `page::refresh` is cheap anyway (one layout
  a frame).
- Scanning: one file per call, on a timer (as the installer unpacks a
  step a timer, `crates/lang-kit/README.md`); a project's first scan of
  1,000 files spreads over a second or two.
- Listings: Jest's comes through our reporter, a line a test. Vitest's
  `vitest list --json` is one JSON array, parsed in one call: for 10,000
  tests about 2 MB (an estimate; **not measured** in wasm). If that's too
  slow for a call, the kit lists a file at a time (`vitest list --json
  <file>`), one call each.
- Writing the results file: one `serde_json::to_vec` at a run's end; at
  3 MB, a few milliseconds natively (**not measured** in wasm).

## 10. How it's tested

Without Fenix, as the packages page is:

- **The kit**: `lang-kit::tests` with a made-up `TestRunner` in its own
  tests, on lang-kit's mock host (its `Host` grows `decorations`,
  `status`, `buffer_text`, `cursor`, `open_file`, `run_command`,
  `launches`, `variable`, with defaults). The model, matching, states,
  the queue, stopping, the problems ledger with a linter's problems,
  decorations, notifications and the agent services each tested as
  calls written down.
- **The page**: rendered to text and snapshot, `fenix_test_kit::render`,
  as the gallery's are (`extensions/gallery/tests/snapshots/`): 100×30
  and 80×24, empty, running, failures, a build that failed, the filter,
  10,000 tests (the tree's speed in a test that times it).
- **Each pack**: on the test kit's `Fake` (feature `lang-kit`), which
  answers `spawn` with canned lines (`on_spawn`, `Ran`), and checks
  `decorations(path)`, `problems(path)`, `status`, `toasts()`
  (`crates/test-kit/README.md`). Its `Reader` tested on recorded
  output, kept as fixtures in `tests/fixtures/` of the pack (not packed).

The fixtures to record, from the samples kept with this note:

| Fixture | Have it | Still to make |
|---|---|---|
| A whole run, pass, fail, skip, cases, nested | all five runners, stdout and stderr apart | |
| A build that fails / a file that doesn't load | cargo (human and JSON), pytest, Go 1.24, Vitest | Jest; Go before 1.24 |
| A crash mid-run | cargo (`abort`), Go (a panic) | pytest (`os._exit`), Vitest, Jest |
| Parallel output interleaved | Go (two packages) | cargo with `--nocapture`; pytest-xdist (does the plugin see workers' reports? **not verified**) |
| ANSI colour | Vitest with `FORCE_COLOR=1` | cargo with `--color=always`; the kit strips escapes from every line before a `Reader` sees it |
| Windows paths | — | made by hand from the Linux ones (`src\lib.rs:25:9`, `C:\...`): **not verified** on Windows |
| A line that isn't UTF-8 | — | made by hand; shows what Fenix's reader does with it (stops), so the kit's `end` copes |
| Big | — | 10,000 tests, generated |

What only a real Fenix shows:

- Lines arriving out of order between stdout and stderr, and the speed
  of 10,000 events.
- Decorations at the end of the line: how they look, and how they move
  as you type.
- The debug hand-off: the picker, the last launch, a project's own.
- A notification while you're elsewhere; the modeline item.
- Problems from the tests and the linter together in `SPC c a` and the
  problems list.
- A stop while cargo's test binary goes on.

These go in each pack's lesson and in a "Where to look first in a real
Fenix" list, as milestone 3 did.

## 11. Decisions for the maintainer

| | Choice | Options | Recommended | Cost |
|---|---|---|---|---|
| D1 | Where Rust's locations come from | (a) scan the source, checked by `--list` after a build; (b) libtest's JSON through a runner with `RUSTC_BOOTSTRAP=1`; (c) nextest's list | **(a)** | Macro-made tests have no line until a failure gives one; `#[path]` and `include!` modules aren't followed. (b) is exact but unstable, Unix-only as tried, fails doc tests, and leans on a flag Rust says not to use; (c) has no locations. |
| D2 | Rust: nextest or libtest's human output | (a) libtest's human lines, nextest when installed; (b) need nextest; (c) libtest only | **(a)** | Two readers for Rust. On stable libtest no time per test, so the tree shows none unless nextest is there; the doctor says so ("cargo-nextest gives each test's time"). (b) asks every user to install something. |
| D3 | Run on save | off, `file`, `failed` | **off by default**, the setting there | Nothing; on, it doubles builds with clippy on save. |
| D4 | lang-kit or its own crate | (a) `lang-kit::tests`; (b) `crates/test-explorer` | **(a)** | Every lang-kit pack's wasm grows, used or not (**not measured**; the packages page is the size guide). (b) would need lang-kit's `Host`, `Runs` and the problems ledger anyway, so it'd be a crate leaning on lang-kit for most of what it does. |
| D5 | What to ask of Fenix | (1) `process::kill` stops the tree, or a `Command::group()`; (2) `debug::start(&LaunchConfig)`; (3) gutter marks; (4) a background task's output for its extension (`Event::TaskOutput`), so runs could be tasks; (5) `last_task` for background tasks | **Open issues for 1 and 2**; 3 when the explorer has been used; 4 and 5 not needed | Without (1), stopping a cargo run waits for its binary; without (2), debugging goes through the picker. Everything else works on 1.5 as it is. |
| D6 | The `SPC m t` group and Go's `tidy` | (a) `t` in every pack, Go's tidy to `SPC m m` in a major (Go 2.0.0), bundles' ranges raised; (b) `SPC m T` everywhere; (c) `t` but Go's at `T` | **(a)** | A major for Go (keys you know changed, `CONTRIBUTING.md`, "Versions"), and bundles naming `fenix.go` by `^1` stay on 1.x until raised. .NET and Java follow in 4.4 the same way (`t` "debug the tests" becomes `t d`). (b) costs a Shift on every use, forever. |
| D7 | The floor | Fenix 1.5, API 1.5 | **1.5** | None: all four packs are on 1.5 already, and nothing here needs more. What D5 brings would raise it later, for that part only. |
| D8 | Should this be Fenix's own? | (a) here, as planned; (b) in Fenix, beside `SPC p r`, the lenses and `run_test` | **(a) now**, and say it to Fenix's plans | Fenix is growing test features of its own (`SPC p r`, the lens, `run_test`, single-test debugging planned). Built here, the explorer and those are two ways in that don't share results. In Fenix it'd have gutter marks, the task pane and tree-killing for free, and four packs' worth of runner knowledge to write in Fenix. The roadmap's premise is "without changing Fenix"; the maintainer owns both. |
| D9 | Problems for failures | (a) on, at the assertion; (b) off, page and marks only; (c) a setting | **(a)**, with a setting `<prefix>.tests.problems` to turn off | The ledger refactor of lang-kit's `Lint` (piece 1). |

## 12. The work, cut for delegation

As milestones 1 to 3 ran: a core that proves the design on one runner,
then the others in parallel from a written recipe.

| # | Piece | Needs | Tier | Owns | Done when |
|---|---|---|---|---|---|
| 1 | **lang-kit's seams**: `Host` grows decorations, status, buffer text, cursor, open file, run command, launches, variable (defaults for a host without them); the test kit's `Fake` implements them; the `Problems` ledger, `Lint` moved onto it | — | recipe-following, careful | `crates/lang-kit/src/{host,lint,lib}.rs`, a new `problems.rs`; `crates/test-kit/src/` | Every lang-kit pack's tests pass unchanged (16 packs, run as `kits-to-1.5.md`'s trial did); a test shows a linter's and a second source's problems surviving each other |
| 2 | **The kit's explorer**: `lang-kit::tests` (`mod.rs`, `model.rs`, `runner.rs`, `run.rs`, `page.rs`, `file.rs`, `store.rs`, `agents.rs`), `Kit::tests`, the README's section | 1 | **design-heavy, top tier** | `crates/lang-kit/src/tests/`, `crates/lang-kit/src/lib.rs` (the wiring), `crates/lang-kit/README.md` | A made-up runner drives the page through the snapshots of §10; stopping, the queue, matching, staleness, the notification and the three services tested; the trait is the one the Rust pack then uses without change |
| 3 | **Rust**: `CargoTest` (scan, libtest's reader, cargo's JSON, per-target plans), nextest's reader if installed (D2), the launch, keys, manifest (commands, the agent tools), fixtures from the samples, a lesson's steps, the README, CHANGELOG | 2's trait (can start on a stub once the trait is merged) | **design-heavy, top tier** | `extensions/rust/` only | The four "done when" of 4.4 in its tests: listed without running, one runs from its line, a failure at its assertion, the failed again with one key; plus **writes the recipe**, `plans/test-explorer-recipe.md`: what a pack implements, the fixtures to record, the manifest lines, the pitfalls met |
| 4a | **Python** (pytest: the plugin as an asset, `uv run` or not, the reader) | 3's recipe | recipe-following | `extensions/python/` | The four, on its fixtures |
| 4b | **Go** (scan, `go list`, `-json`, `-run` building, build events; the key move of D6) | 3's recipe | recipe-following | `extensions/go/` | The four; Go before 1.24 handled as §6 says |
| 4c | **Web** (Vitest and Jest: which one from `package.json`, both reporters as assets, `file:line` and `-t` plans) | 3's recipe | recipe-following, the hardest of the three | `extensions/web/` | The four, for each runner |
| 5 | **Issues for Fenix** (D5's 1 and 2, gutter marks with the explorer as the case) | 2 | small | nothing in the repository; issue texts for the maintainer | The maintainer has them to file |
| 6 | **Releases and pictures**: each pack's version and CHANGELOG ("Tests: ..."), screenshots scripted (`screenshots.toml`, the tests page) | 3, 4a-c | recipe-following | each pack's `screenshots.toml`, `CHANGELOG.md`; the roadmap's milestone 4 section | `cargo xtask validate`, `pack` for each; the roadmap says "built, not yet proven" with what's left |

- 1 lands first; 2 next. 3 starts as soon as 2's trait is merged as a
  stub (2 and 3 then run side by side, 3 keeping 2 honest). 4a, 4b and 4c
  run in parallel after 3's recipe; each owns one pack's folder and
  nothing else, so they don't collide. A change they need in the kit goes
  back to 2's owner, not into the kit from a pack's branch.
- Sizes as the roadmap writes them: 1 S, 2 L, 3 M, 4a S, 4b S, 4c M, 5 S,
  6 S.
- **Releases**: Rust 1.3.0, Python 1.3.0, Web 1.3.0, and Go 2.0.0 under
  D6 (a), 1.3.0 under (b); the bundles holding Go raised after it. Each
  pack is one minor release with lang-kit's change in it; lang-kit isn't
  released on its own (`publish = false`), but piece 1 changes every
  lang-kit pack's code, so those whose behaviour didn't change need no
  release (`CONTRIBUTING.md`, "Shared crates"). So **four releases** for
  4.3, and **a fifth** for the bundles if Go goes to 2.0.0. 4.4 is one
  minor release per further pack.

## 13. Risks, and what could make it not worth doing as designed

- **Stopping a run doesn't stop the runner's children** (verified with
  the same call Fenix makes). For cargo the test binary runs to its end;
  for a hung test, until you kill it outside Fenix. The page says
  `stopping…` and the next run waits. If D5's first issue isn't taken,
  this stays. The fallback is to run through `run_in_background` for
  the runners that can write a file (pytest, Vitest, Jest, nextest),
  which kills the tree but gives results only at the end: worse for the
  common case to fix the rare one. Not recommended.
- **The explorer duplicates Fenix's test features** (D8). Two "run the
  test here" keys with different results surfaces will confuse. The
  README says plainly: `SPC p r` is a task; `SPC m t t` shows in the
  explorer. If Fenix plans a test page of its own, this milestone should
  stop at piece 1 and give Fenix the runner notes.
- **Rust is the weakest of the four on stable**: no time per test, no
  line for macro-made tests, a build before `--list`, cargo's lock shared
  with rust-analyzer and clippy on save. It still does the four things
  4.4 asks. If times matter, nextest is the answer, not the kit.
- **Event volume**: a run's every line is a call into the extension.
  Verbose suites (Go's `output` events for every `t.Log`) could be
  100,000 calls. Not measured. If it's slow, a `Reader` can ask for less
  (no `-v`, no output events except for failures), and Fenix could batch
  lines (an issue only if seen).
- **A non-UTF-8 line stops Fenix's reader** for that stream, and the run
  is then blind until it ends (read from source, not tried in Fenix).
  The kit ends such a run as `NotRun("its output stopped")` when the
  process exits with tests unaccounted for.
- **stdout and stderr out of order** between them: the design reads
  each runner from stdout alone (cargo's JSON and libtest's lines both
  on stdout) and never relies on their order.
- **Runner versions**: the reporters use Vitest's 3+ reporter API
  (verified on 5.1.0 only), Jest's `onTestCaseResult` (30.5.2 only),
  pytest's hooks (9.2.0 only; they're old and stable), Go's build events
  (1.24+). Older versions are **not verified**; each pack's doctor checks
  the version and says what falls back.
- **The debug hand-off** goes through the launch picker and Fenix's
  memory of the last launch; it can start the wrong one after you debug
  something else, and never works in a project with its own launch.
  Without D5's second issue it stays a convenience, not a promise.
- **The problems ledger touches every lang-kit pack's linter.** The
  behaviour shouldn't change, and piece 1's "done" is all 16 packs'
  tests passing; but it's the one place this milestone can break
  something that works today.
- **Listing runs your code** (pytest imports the test modules, Vitest
  and Jest load them): a module with side effects at import does them on
  every listing, as the runners' own `--collect-only` does and as other
  editors' explorers do. Said in each README; Rust and Go list without
  it, from the source.
- **Size**: every lang-kit pack carries the explorer's code; not
  measured. If it's large, a cargo feature (`tests`) keeps it out of
  packs that don't use it.

What would make it not worth doing as designed: Fenix deciding to build
the explorer itself (D8), or `Command::spawn` turning out too slow for a
real suite's output in a real Fenix. Neither can be known from here; the
second is the first thing piece 3 should try in a nightly.

## Appendix: what the runners printed

Kept, with the sample projects and the three reporters, under the
scratchpad of the session that wrote this:
`/tmp/claude-0/-home-user/61419de0-c6d3-5f12-9814-ecf7298003a1/scratchpad/m4/`
(`README.txt`, `samples/`, `reporters/`, `outputs/<runner>/`,
`outputs/timings.txt`). They aren't in the repository; piece 3 copies
what it needs into `extensions/rust/tests/fixtures/`, and so on. Trimmed
here.

**cargo test, stable** (`outputs/rust/run-human.stdout`; the `Running`
lines were on stderr):

```text
running 9 tests
test geo::tests::unwraps_none ... FAILED
test geo::tests::dist_is_symmetric ... ok
test tests::case_negative ... FAILED
test tests::adds ... ok
test tests::ignored_one ... ignored, slow
…
test tests::panics_as_it_should - should panic ... ok

failures:
…

---- tests::fails_on_purpose stdout ----
some output before the failure

thread 'tests::fails_on_purpose' (9760) panicked at src/lib.rs:25:9:
assertion `left == right` failed: two and two
  left: 4
 right: 5

test result: FAILED. 5 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**libtest's JSON listing**, unstable (`outputs/rust/list-json-runner.stdout`):

```text
{ "type": "test", "event": "discovered", "name": "tests::case_negative", "ignore": false, "ignore_message": "", "source_path": "src/lib.rs", "start_line": 45, "start_col": 9, "end_line": 45, "end_col": 22 }
{ "type": "test", "event": "discovered", "name": "tests::ignored_one", "ignore": true, "ignore_message": "slow", "source_path": "src/lib.rs", "start_line": 30, ... }
```

**A build that fails** (`outputs/rust/compile-error-json.stdout`, the
`reason` of each line): `compiler-artifact` ×2, `compiler-message` ×2,
`{"reason":"build-finished","success":false}`; on stderr ``error: could
not compile `orbit` (lib test) due to 1 previous error``.

**nextest's JUnit** (`outputs/nextest/junit.xml`):

```text
<testcase name="geo::tests::unwraps_none" classname="orbit" timestamp="..." time="0.005">
  <failure message="thread &apos;geo::tests::unwraps_none&apos; (10171) panicked at src/geo/mod.rs:15:11" type="test failure with exit code 101">...
```

**pytest, through the plugin** (`outputs/pytest/run-plugin.stdout`):

```text
FENIX-TEST {"kind": "test", "id": "tests/test_basic.py::test_cases[negative]", "file": "tests/test_basic.py", "line": 25, "name": "test_cases[negative]"}
FENIX-TEST {"kind": "result", "id": "tests/sub/test_unit.py::TestUnit::test_bad", "when": "call", "outcome": "failed", "duration": 0.000149, "file": "/usr/local/lib/python3.13/unittest/case.py", "line": 900, "message": "AssertionError: 1 != 2", "at": ["tests/sub/test_unit.py", 9], "text": "..."}
FENIX-TEST {"kind": "result", "id": "tests/test_basic.py::test_uses_broken_fixture", "when": "setup", "outcome": "failed", ..., "message": "RuntimeError: fixture broke", ...}
FENIX-TEST {"kind": "collect-error", "id": "tests/test_syntax.py", "message": "..."}
```

**go test -json** (`outputs/go/run-json.stdout`, `compile-error-json.stdout`):

```text
{"Action":"run","Package":"example.com/orbit","Test":"TestTable/negative_one"}
{"Action":"output","Package":"example.com/orbit","Test":"TestTable/negative_one","Output":"    orbit_test.go:37: got -2, want -3\n"}
{"Action":"fail","Package":"example.com/orbit","Test":"TestTable/negative_one","Elapsed":0}
{"ImportPath":"example.com/orbit/geo [example.com/orbit/geo.test]","Action":"build-output","Output":"geo/geo_test.go:6:13: cannot use \"3\" (untyped string constant) as float64 value in argument to Dist\n"}
{"Action":"fail","Package":"example.com/orbit/geo","Elapsed":0,"FailedBuild":"example.com/orbit/geo [example.com/orbit/geo.test]"}
```

**Vitest, list and reporter** (`outputs/vitest/list.json`, `run-reporter.stdout`):

```text
{ "name": "add(%i, %i) is %i", "file": ".../src/orbit.test.js", "location": { "line": 19, "column": 3 } }
FENIX-TEST {"kind":"result","file":".../src/orbit.test.js","names":["outer","inner","deep"],"line":26,"state":"failed","duration":0.479,"errors":[{"message":"expected +0 to be 1 // Object.is equality","stack":"AssertionError: ...\n    at .../src/orbit.test.js:27:25\n..."}]}
```

**Jest, discovery and reporter** (`outputs/jest/discover-reporter.stdout`, `run-reporter.stdout`):

```text
FENIX-TEST {"kind":"test","file":".../src/orbit.test.js","names":["add(-1, -1) is -3"],"line":18,"state":"pending"}
FENIX-TEST {"kind":"result","file":".../src/orbit.test.js","names":["fails on purpose"],"line":7,"state":"failed","duration":14,"failures":["Error: expect(received).toBe(expected) // Object.is equality\n\nExpected: 5\nReceived: 4\n    at Object.toBe (.../src/orbit.test.js:9:21)\n..."]}
```

**Stopping cargo** (`outputs/rust/kill-experiment.txt`, `Child::kill` on
`cargo test` 1.5 s into an 8 s test):

```text
after killing cargo, the test binary: 23992 .../target/debug/deps/slow-569e76b4f086810c
stdout reached EOF after 6.2 s more
```
