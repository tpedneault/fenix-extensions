# The test explorer: the packs' side

> Milestone 4.1, first written on 13 November 2026 as an explorer built
> entirely in extensions, and rewritten the same day for the split the
> maintainer decided: Fenix owns the surface, each language pack its
> runner, as a **test provider** through extension API 1.6. Fenix's half
> and the API are designed in Fenix's repository, in
> `docs/plans/test-explorer.md` (*Fenix's plan*; its sections are cited
> as *Fenix §6.3*, and a bare *§6* is this plan's) and
> `docs/adr/0005-test-providers.md` (*ADR 0005*). This is the
> extensions' half, for the agents who build it. Every claim about a
> runner comes from a run on this machine (the first version's, or a
> rerun on 13 November marked *rerun*), unless it says **not verified**;
> every claim about the API cites Fenix's plan or a file and line of
> Fenix's source at `b5c3765`. Nothing was run in Fenix: there's no Fenix
> binary where this was written, and API 1.6 isn't built yet.

## 1. What changed, and what this repository delivers

The first version put the page, the model, the run queue, the marks,
the problems and the agents' tools in `lang-kit`, on API 1.5. Every
serious limit it found was an extension's: it couldn't kill a runner's
children, draw in the gutter, start a debug session from a launch it
gives, read a background task's output, or keep its failures apart from
the same pack's linter; and it would have been a second way to run a
test beside `SPC p r`, the lens and `run_test` (ADR 0005, "Context").
So on 13 November 2026 the maintainer split it (ADR 0005, "Decision"):

- **Fenix** owns the surface: the tests page, the model, the run
  lifecycle, the gutter, results after the line, failures as problems,
  the debugger hand-off, the modeline, the notification, `last_task`,
  the agents' tools (Fenix §4, §7 to §11). It runs every
  command itself through the task pane's supervisor (Fenix §6.1, §8.2).
- **A language pack** owns what its runner knows: how to find tests in a
  file's text, what commands list and run them, how to read what they
  print, and how to debug one. It says so as a **test provider**:
  `[[contributes.test_providers]]` in its manifest (Fenix §5) and five
  optional exports, `tests-scan`, `tests-plan`, `tests-read`,
  `tests-step-ended`, `tests-launch`, with one import,
  `test-provider-set` (Fenix §6.3).

This repository now delivers:

1. **`lang-kit::tests`**: the `TestRunner` trait a pack implements (§4
   below), and the adapter that puts it on the five exports, holding a
   reader per run and step, joining cut lines, splitting long
   selections, writing the reporters to `/data` and naming them in each
   step's `needs`.
2. **The test kit's driver**: a stand-in for Fenix's side of the five
   exports, so a provider is tested natively on recorded runner output.
3. **Four providers**: Rust (`cargo test`, nextest when installed),
   Python (pytest), Go (`go test`), Web (Vitest and Jest), each a minor
   release on `api = "1.6"`, published once Fenix 1.6.0 is out.

What stays as the first version found it, nearly word for word: the
facts per runner (§3), the commands, the reporters, the build-failure
tells, the debug arguments and the recorded outputs (appendix). What's
new is the shape (§4), the per-pack briefs (§6), the plan of work (§8)
and what this side asks of Fenix's plan (§11).

## 2. What's Fenix's now

These were the first version's and aren't this repository's job any
more. Nothing of them is built here.

| The first version's | Where Fenix's plan has it |
|---|---|
| The model: tests, groups, states, matching results to what was found, what's dropped (its §3) | §4 |
| The page, its keys, the filter, 10,000 tests (its §4) | §7.2, §7.3, §7.5 |
| The `SPC m t` leader group, and Go's `tidy` moving for it (its §4, D6) | §7.4: Fenix's `SPC T`. Go's `SPC m t` stays `tidy`; no pack needs a major |
| Marks after the line, and the gutter marks it asked Fenix for (its §5) | §9.1, §9.2 |
| Failures as problems, and lang-kit's problems ledger (its §5, piece 1) | §9.3: Fenix publishes under `fenix:#tests`, so no ledger |
| The test the cursor is in (its §5) | §9.4 |
| One run at a time, the queue, stopping, run on save, staleness, the failed again (its §6) | §8.1 to §8.4 |
| The run's output, `o` (its §6) | §8.5 |
| Starting a debug session through the picker and a launch variable (its §7) | §11: `tests-launch`, no picker |
| The modeline item and the notification (its §8) | §10.1 |
| The agents' tools `tests_list`, `tests_failures`, `tests_run` (its §8) | §10.3: Fenix's `tests`, `test_failures`, `run_tests`, `debug_test` |
| Results kept across a restart, `page_state` (its §9) | §4, "What's kept"; §7.6 |
| The issues it wanted filed on Fenix: tree kill, `debug::start`, gutter marks, a background task's output, `last_task` for background tasks (its D5, piece 5) | §8.2, §11, §9.1; the other two aren't needed (Fenix §6.1) |
| Its decisions D3 (run on save), D5, D6, D8, D9 (problems) | §8.4, §16, ADR 0005; see §7 below |

## 3. The facts

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

### Checked again for this revision

Four small runs on 13 November 2026 (*rerun*), from fresh copies of the
samples, to settle what the split changed:

| | What it did | |
|---|---|---|
| Vitest, listing through the reporter | `vitest run --includeTaskLocation --reporter=<fenix-vitest.mjs> -t FENIX_NOTHING`: a `test` line for each of the 9 tests with its line, `test.skip`, `test.todo` and each `test.each` case included, then a `result` line each (`skipped`); exit 0, 0.43 s. Every test's `mode` is `skip` in this run, so a real skip or todo isn't told apart. Jest's trick, for Vitest. | verified (5.1.0) |
| pytest, ids from a file | `pytest -q @ids.txt`, one node id a line, ran `tests/test_basic.py::test_cases[strings [odd] id]` and another. | verified (9.2.0) |
| pytest, the plugin's folder as an argument | `python -c "import sys; sys.path.insert(0, sys.argv.pop(1)); import pytest; raise SystemExit(pytest.main(sys.argv[1:]))" "<dir with a space and a '>" -q -p fenix_tests --collect-only` collected all 14 through the plugin. | verified on Linux |
| go test `-fullpath`, and the cache | `-fullpath` puts the file's full path in a failure's line (`/…/orbit_test.go:37: got -2, want -3`). A package whose result is cached replays its tests' `run`, `output` and `pass` events, with `Elapsed` 0 and `(cached)` in the package's output. | verified (1.24.7) |

The pytest plugin writes its lines with `json.dumps`, which escapes
everything that isn't ASCII (read from `fenix_tests.py`): a test's name
reaches the reader whatever the pipe's encoding.

### What the API 1.5 route was, in one line

On 1.5 only `Command::spawn` gave an extension a runner's lines as they
came, and its `process::kill` stopped only the child; that and the rest
of the first version's table are why the split was made (ADR 0005,
"Context"). On 1.6 Fenix runs the commands (Fenix §6.1), so none
of it applies.

## 4. The shape: `lang-kit::tests`

### 4.1 What Fenix calls, as a pack sees it

From Fenix §6.3 (the WIT) and §6.6 (the Rust side), with Fenix §6.4's
promises:

| Export (Fenix §6.3) | `Extension` method (Fenix §6.6) | When Fenix calls it |
|---|---|---|
| `tests-scan(provider, path, text) -> list<found-test>` | `tests_scan` | a file of the provider's `languages` opened, 500 ms after typing pauses, saved; each of its `scan` files when the provider starts (Fenix §7.1). One file, at most 1 MiB (Fenix §6.4). |
| `tests-plan(provider, run, ask) -> result<test-plan, string>` | `tests_plan` | a listing (`discover`), a run (`run(list<found-test>)`, `run-all`) |
| `tests-read(provider, run, step, lines) -> list<test-report>` | `tests_read` | a step's output, at most 512 lines and 256 KiB a call, in order (Fenix §6.4) |
| `tests-step-ended(provider, run, step, end) -> list<test-report>` | `tests_step_ended` | exactly once per step, `stopped` for one that never started (Fenix §6.4) |
| `tests-launch(provider, run, test) -> result<test-launch, string>` | `tests_launch` | debugging a test; asked again once after a `build-first` plan (Fenix §11) |
| import `test-provider-set(provider, runner, problem)` | `tests::provider` | the pack's, when its runner or its problem changes; Fenix lists again (Fenix §6.3) |

A line is lossy UTF-8, `\r` and colour codes gone, cut at 64 KiB with
`cut` set on each piece but the last (Fenix §6.4). Each call is an ordinary
call with the extension's deadline (5 s unless `limits.call_ms` says)
and must do only a few milliseconds of work (Fenix §6.5). Fenix keeps at most
10,000 `found-test` a scan, 256 steps a plan, 10,000 reports a call, an
id of 1 KiB and a label of 256 characters (Fenix §6.5).

### 4.2 What a pack implements

The first version's trait (its §3), changed where Fenix §6.6's table
says, and where this plan's cross-check (§11) found it had to be:

```rust
/// A runner's own name for a test, the same in what the pack scans,
/// lists and reads: `orbit/lib geo::tests::unwraps_none`,
/// `tests/test_basic.py::test_cases[negative]`,
/// `example.com/orbit TestTable/negative_one`,
/// `src/orbit.test.js > outer > inner > deep`. It is the test's id in
/// Fenix (§4.4).
pub struct Key(pub String);

/// A group a test is in, root first: a target or package, a file, a suite.
pub struct Group {
    pub kind: Kind,                  // Group | File | Suite (the API's `test-kind`)
    pub label: String,               // "orbit (lib)", "src/lib.rs", "tests"
    pub file: Option<String>,        // under /project
    pub line: Option<u32>,           // from 1: a `mod tests`, a class, a describe
    pub end_line: Option<u32>,
}

/// A test or a case, as a scan, a listing or a run finds it.
pub struct Found {
    pub key: Key,
    pub label: String,               // "adds", "negative", "add(-1, -1) is -3"
    pub kind: Kind,                  // Test | Case
    pub groups: Vec<Group>,          // root first
    pub file: Option<String>,        // under /project
    pub line: Option<u32>,           // from 1, as runners write them
    pub end_line: Option<u32>,
    pub skip: Option<String>,        // #[ignore = "slow"], @skip, test.skip: why, or ""
    /// Its cases come only from a run: test.each, a parametrize the scan
    /// saw, a Go test that calls t.Run, an #[rstest].
    pub template: bool,
    /// A case's template, when the pack knows it: pytest's
    /// `…::test_cases` of `…::test_cases[negative]`, Go's `TestTable`.
    pub of: Option<Key>,
}

/// What Fenix asked for, as lang-kit hands it on.
pub enum Asked {
    /// A test or case Fenix has, with what it knows of it.
    Test(Found),
    /// A group: everything under it. `tests` is what lang-kit knows is
    /// under it (§4.3, "What it remembers"), possibly not all of it.
    Group { groups: Vec<Group>, tests: Vec<Key> },
}
pub enum Ask { Discover, Run(Vec<Asked>), RunAll }

/// One command of a plan.
pub struct Step {
    pub command: Command,            // the API's: program, args, cwd, env
    pub label: String,               // "orbit (lib)", "building orbit's tests to list them"
    /// What it runs, when it knows: keys, or groups by their labels.
    /// Empty: what of the ask the steps before it don't cover (Fenix §6.3).
    pub covers: Vec<Covered>,
}
pub enum Covered { Test(Key), Group(Vec<String>) }

/// A whole line of a step's output: lang-kit has joined a cut line's pieces.
pub struct Line<'a> { pub text: &'a str, pub stderr: bool }

/// What a reader read.
pub enum Report {
    Found(Found),
    Started(Key),
    Ended { key: Key, outcome: Outcome, ms: Option<f64>, message: Option<String>, at: Option<At>, text: Option<String> },
    Output { key: Option<Key>, text: String },
    /// Nothing of it ran: a target didn't build, a file didn't load. Never
    /// a test's failure. `under` is the group's labels; none: all the step covers.
    Broken { under: Option<Vec<String>>, message: String, at: Option<At> },
    /// The process died. `under`: the binary's or package's group, when known.
    Crashed { under: Option<Vec<String>>, message: String },
    /// Something to keep for this run, not for Fenix: a test binary's
    /// path from a build-first plan, given to `launch` when Fenix asks again.
    Keep { name: String, value: String },
}
pub use api::tests::Outcome;         // Passed | Failed | Skipped | Errored
pub struct At { pub file: String, pub line: u32, pub column: Option<u32> }   // from 1

/// Reads one step's output; a new one for each step.
pub trait Reader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>);
    fn end(&mut self, end: &StepEnd, out: &mut Vec<Report>);   // the API's `StepEnd`
}

pub enum Launching { Now(LaunchConfig), BuildFirst(Vec<Step>) }

pub trait TestRunner {
    /// The manifest's `[[contributes.test_providers]].id` it answers for.
    fn provider(&self) -> &str;
    /// The runner it uses in this project, and why it can't run, when it
    /// can't: ("cargo nextest", None), ("pytest", Some("pytest isn't
    /// installed in .venv")). Asked at activation and after a project
    /// file changes; lang-kit tells Fenix only what changed (§4.3).
    fn runner(&self) -> (String, Option<String>);
    /// The tests in a file's text, nothing run: instant.
    fn scan(&self, path: &str, text: &str) -> Vec<Found>;
    /// The steps that list or run `ask`. lang-kit splits a step whose
    /// command line is too long (`split`) and fills in `needs`.
    fn plan(&self, ask: &Ask) -> Result<Vec<Step>, String>;
    /// A reader for step `step` of a plan it made.
    fn reader(&self, ask: &Ask, step: &Step) -> Box<dyn Reader>;
    /// Whether `reported` is one of `template`'s cases, when the line
    /// alone doesn't say (§4.3). By default: none is.
    fn is_case(&self, _template: &Found, _reported: &Key) -> bool { false }
    /// How to debug `test`; `kept` is what a build-first plan's reader kept.
    fn launch(&self, _test: &Found, _kept: &[(String, String)]) -> Result<Launching, String> {
        Err("no debugging for tests here".into())
    }
    /// Files its commands read, written under /data/lang-kit/tests/ when
    /// the pack starts: ("fenix_tests.py", bytes).
    fn assets(&self) -> &[(&str, &[u8])] { &[] }
}
```

`Kit::tests(Box<dyn TestRunner>)` gives the kit one, as
`Kit::packages` gives a `PackageManager`; a pack with two providers
calls it twice. It's still `PackageManager`'s pattern: commands out,
parsed text in.

What changed from the first version's trait, and why:

| First version | Now | Why |
|---|---|---|
| `name()` | `runner()` | The name is the manifest's; `test-provider-set` carries the runner in use and a problem (Fenix §6.3). |
| `scans(path)` | gone | Fenix decides what's scanned, by `languages` and `scan` (Fenix §5, §7.1). |
| `Found.path: Vec<String>` | `groups: Vec<Group>` and `label` | A node in Fenix has a kind and, for a suite, a line of its own (Fenix §4, the kinds table; Fenix §9.1 marks a group's first line). |
| `list() -> Vec<Command>` | `plan(&Ask::Discover)` | A listing is a plan (Fenix §6.6). |
| `plan(&Selection)` | `plan(&Ask)` | The ask carries Fenix's records (Fenix §6.6); a group comes with what lang-kit knows is under it (§11, F2). |
| `reader()` | `reader(&Ask, &Step)` | The reader knows which step it reads: Rust's target, Go's packages. |
| `Line { text, stderr }` | the same, whole lines | lang-kit joins `cut` pieces (§4.3). |
| `Reader::end(code)` | `end(&StepEnd)` | `exited`, `stopped`, `timed-out`, `failed` (Fenix §6.3). |
| `Ended { failure: Option<Failure> }`, `Outcome::Skipped(reason)` | flattened, `Skipped` with `message`, `Errored` new | The API's `test-ended` (Fenix §6.6's table). |
| `Broken { file, at, message }`, `Crashed(String)` | `under` a group | A step can hold several binaries or packages (§11, F4). |
| — | `Report::Keep` | `build-first` asks again with the same run (Fenix §11): the binary's path has to survive between the two calls. |
| `launch(&Test) -> Option<LaunchConfig>` | `launch(&Found, kept) -> Result<Launching, _>` | `build-first` (Fenix §6.3). |
| `assets()` | the same | Fenix §6.6's table. |

### 4.3 What lang-kit does

| | |
|---|---|
| **The five methods** | `Kit::tests_scan`, `tests_plan`, `tests_read`, `tests_step_ended`, `tests_launch`, the pack's `impl Extension` forwarding to them as it forwards `run_command` and `handle` today (`extensions/rust/src/lib.rs:399-430`). A macro, `lang_kit::test_exports!()`, writes the five forwards. The `provider` argument picks the `TestRunner`; an unknown one answers empty, or `Err`. |
| **Ids and groups** | A test's id is its `Key`. A group's id is its labels root first, each preceded by U+001F (`\u{1f}orbit (lib)\u{1f}src/lib.rs`): Fenix §6.6's rule, with a leading separator so a group's id can never equal a key. Every group a `Found` names is given to Fenix as its own `found-test`, before the first test under it, once per answer. Lines become from 0 (Fenix §6.3). Kinds map one for one. |
| **What it remembers** | Per provider, the last scan of each file and the last listing: key → file, line, groups, template. Used to fill in what a listing or a run doesn't say (Rust's `--list` gives no line, and no file for an inline `mod`; §11, F3), to give a group's tests with the ask (`Asked::Group.tests`), to put a test found in a run under its template or its module (`of`, or a known key that's a prefix: `tests::x::case_1` under `tests::x`), and to know which tests a crashed binary held. About 150 bytes a test: 15 MB for 100,000, Fenix's most (Fenix §6.5). Lost when the extension starts again; Fenix scans again then (Fenix §7.1). |
| **Templates and cases** | A `Found` from a run that's a case (`of` set) is sent as `found` (kind `case`, parent the template) before its `ended`, so Fenix adds it (Fenix §4, matching rule 2). With no `of`: a reported key unknown to it, in the same file and on the same line as a template (Vitest's and Jest's `test.each` cases are at the template's line, §3), is that template's case; else `TestRunner::is_case`. |
| **Readers** | One per `(provider, run, step)`, made at the step's first `tests_read`, dropped at its `tests_step_ended` (Fenix §6.6's table). A call for a step it has no reader for (the extension started again mid-run) gets a new one, and a line on the output page. Per run, what readers `Keep` and the asked templates, freed after the plan's last step ends, or, after a build-first plan, at the second `tests_launch` (the last 8 runs kept at most). |
| **Cut lines** | Pieces with `cut` are joined, across calls, into one line before the reader sees it, up to 1 MiB; past that the rest is dropped and the reader gets what came, so a pytest result with 9 sections of 8,000 characters escaped (more than 64 KiB) still parses. |
| **Crashes** | A `Crashed { under }`: every test lang-kit knows under that group that hasn't ended in this step is ended `errored` with the message at once; the API's `crashed` is sent at the step's end, when everything else in the step has reported (§11, F4). |
| **One `ended` per test** | A second `Ended` for a key in a step is merged into the first, the worse outcome winning (failed or errored over passed), its text appended (§11, F5). |
| **Output** | Consecutive `Output`s of one test within a call are sent as one. |
| **Long command lines** | `split(command, items, budget)` is how a pack's `plan` makes several steps of one selection: each step's command line, as Windows quotes it, under 24,000 characters (Windows takes 32,767; a `.cmd` through `cmd.exe`, 8,191, which no pack here runs). A plan of more than 256 steps runs the selection's groups instead (coarser, never wrong). |
| **Reporters** | `assets()` written to `/data/lang-kit/tests/` at activation, only where the bytes differ. A step whose args or env name one (`${data}/lang-kit/tests/fenix_tests.py`) gets it in `needs`, as files a remote project copies first (Fenix §6.3, §8.6). Files a plan writes for one run (pytest's ids, `/data/lang-kit/tests/runs/<run>-<step>.txt`) are in `needs` too, and deleted at the step's end. |
| **The runner in use** | `runner()` asked at activation and after `Event::Saved` of a project file; `tests::provider(id, runner, problem)` called only when it differs from what was last told (or, at the first, from the manifest's `name` and no problem), so a start doesn't make Fenix list again (§11, F8). |
| **The millisecond rule** | Nothing is parsed whole at a step's end: every reader works a line at a time, so a call costs its 512 lines. Each of the five is timed in the driver's tests natively (§4.5). |
| **Panics** | None on input: a reader that can't parse a line passes it over. A trap stops the whole pack (Fenix §6.4, "After a trap"). |

### 4.4 Ids

The first version's keys (its §3, "Matching"), with one change: Rust's
carries the package, since `lib tests::adds` is in every crate of a
workspace.

| Pack | Key | Example |
|---|---|---|
| Rust | `<package>/<target> <path>`; target `lib`, `bin:<name>`, `test:<name>`, `example:<name>`, `doc` | `orbit/lib geo::tests::unwraps_none`, `orbit/test:it integration_fails`, `orbit/doc src/lib.rs - add (line 6)` |
| Python | the node id, as pytest writes it from its rootdir | `tests/test_basic.py::test_cases[strings [odd] id]` |
| Go | `<import path> <name>`, subtests as `go test` reports them | `example.com/orbit TestTable/negative_one` |
| Vitest, Jest | `<file below the project> > <names joined by " > ">` | `src/orbit.test.js > outer > inner > deep` |

- **At most 1 KiB** (Fenix §6.5). A longer key (a pytest case whose
  parameters are long strings) is cut by the pack itself, the same way in
  scan, listing and run: its first 900 bytes and `#` with the SHA-1 of
  the whole, so it still matches. Such a case can't be named to its
  runner from its id, so a run of it runs its template.
- **Repeated names** (two `test("adds")` in one `describe`, which Jest
  and Vitest allow): the second and later get ` (2)`, ` (3)` by their
  order in the file. A run of either runs both: `-t` can't tell them
  apart.
- **Names containing ` > `**: the plan splits the key's names back on
  ` > `, which a describe named `a > b` breaks: its tests run with a
  regex that matches more. Fixing it needs §11's F6 (a field for the
  pack's own data).

### 4.5 The test kit's driver

A provider is tested natively, on recorded output, through the same
lang-kit code the exports call. The test kit (feature `lang-kit`) gets
`fenix_test_kit::tests::Fenix`, which does Fenix's side of the promises in Fenix §6.4:

```rust
let mut fenix = tests::Fenix::new(Rust::with_host(fake));     // the pack, on the fake host
fenix.manifest("extension.toml");                             // its providers and permissions.process
let found = fenix.scan("rust", "/project/src/lib.rs", include_str!("../src/samples/tests/cargo/lib.rs"));
fenix.on_step(|s| s.args_have(&["--list"]), Recorded::from("src/samples/tests/cargo/list").exits(0));
fenix.on_step(|s| s.args_have(&["--lib"]), Recorded::from("src/samples/tests/cargo/run-human").exits(101));
let run = fenix.run("rust", Ask::RunAll);
assert_eq!(run.state("orbit/lib tests::case_negative"), State::Failed);
assert_eq!(run.at("orbit/lib tests::case_negative"), Some(("src/lib.rs", 43, Some(5))));
```

| It does | Fenix's plan |
|---|---|
| Feeds each step's recorded stdout and stderr (`<name>.stdout`, `<name>.stderr`) as lines: lossy UTF-8, `\r` dropped, ANSI removed, lines cut at 64 KiB with `cut`, in batches of at most 512 lines and 256 KiB | §6.4 |
| Interleaves the two streams as told: `Order::Recorded` (a `<name>.order` file of `o`/`e`), stdout first, stderr first, or shuffled per 10 ms tick from a seed, each stream's own order kept | §6.4 (Windows: per tick) |
| Then `tests_step_ended` with the recorded exit, or `stopped`, `timed-out`, `failed`; `stopped` for steps after a stop | §6.4 |
| Checks every plan: each program one `permissions.process` allows (names, `${setting:…}`, `/data/`, `/tools/`), at most 256 steps, each command line under 32,767 characters as Windows quotes it, `needs` under `/data` and written | §5, §6.5 |
| Checks every answer: ids at most 1 KiB, labels 256 characters, no id twice in one scan, at most 10,000 found or reports a call, a group before its children, a case's `found` before its `ended` | §6.5, §4 |
| Keeps a small model by Fenix's rules, so a test asks what the page would show: state per id (unknown, queued, running, passed, failed, skipped, errored), `at`, message, text; ids found in a run; what a step ended without reporting (`errored` "the run ended without a result for it", or back to before when stopped); `broken` and `crashed` applied as §8.5 says | §4, §8.5 |
| Times each call natively and fails a test over 5 ms a 512-line batch (wasm is slower: **not measured**) | §6.5, §6.7 |
| Answers `test-provider-set` and keeps every call (`fenix.providers()`) | §6.3 |
| `tests_launch`, `build-first` included: runs its plan's steps as a run's, then asks again once | §11 |

On API 1.5 (§8.2 here) the driver calls `Kit`'s five methods directly; once
the branch has 1.6, `Harness` gains `h.tests_scan(..)`, `h.tests_run(..)`,
`h.tests_launch(..)` that go through the pack's `impl Extension`, and
the fake keeps `test-provider-set` as `fake.test_providers()`.

**Where fixtures live.** The recorded outputs are in this session's
scratch folder (appendix), which isn't kept. Each piece of work copies
what it needs into the repository as it starts: lang-kit's own into
`crates/lang-kit/src/tests/samples/`, a pack's into
`extensions/<pack>/src/samples/tests/<runner>/` beside the samples the
packs already keep (`extensions/rust/src/samples/cargo-clippy.json`),
which `pack` leaves out of the package (`xtask/src/main.rs:151-154`:
`src` and `tests` are source only). The reporters go in
`extensions/<pack>/src/tests/` and into the pack's wasm with
`include_bytes!`. A fixture is the command line in an `.args` file, its
`.stdout` and `.stderr` as Fenix gives them apart, and its exit code;
paths of the machine that recorded it are replaced by `/project`.

## 5. Running, as the first version found it

The commands, the build-failure tells and the debug arguments, as the
first version wrote them (its §6 and §7). On 1.6 each command is a
step of a plan, run by Fenix (Fenix §6.1); "the explorer"
below is now Fenix, and what changes per pack is in §6.

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

What changes on 1.6, for every pack:

- **Each is a step**, its `command` with `cwd` and `env` (the API's
  `command` record has both, `world.wit:233-248`), `${data}` filled in
  by Fenix (Fenix §6.3). The program is checked against `permissions.process`
  (Fenix §5): every one above is already in its pack's manifest (§6).
- **The boot takes the plugin's folder as an argument**, not inside a
  raw string, so a `'` in the path can't end it (*rerun*: a folder named
  with a space and a `'`).
- **Long selections** go in several steps (§4.3, `split`), or, for
  pytest, in a file of ids (`@file`, *rerun*).
- **Vitest lists through its reporter** (*rerun*), not `vitest list
  --json`: the JSON is one pretty-printed array, about eight lines a
  test, with no `skip`, `todo` or `test.each` cases, and a reader would
  have to hold all of it.
- **nextest streams** `--message-format libtest-json` (§6.1, D-R1 in §7)
  rather than leaving its failures in a JUnit file for the end, which a
  reader would parse whole in one call and a remote project can't read.

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

As reports: a build failure is `Broken { under }` (the target, the
package, the file; none when nothing of the step built), with the first
error's message and place; Fenix errors the tests under it and sets no
problem for it (Fenix §8.5). A crash is `Crashed { under }` (§4.3). "Go back
to what they were" for pytest's other files is gone: Fenix decides
(Fenix §8.5), and pytest's collection error is a `Broken` with `under` none,
since nothing ran.

### Debugging

| Pack | Today | The explorer adds |
|---|---|---|
| Rust | `launch/test.toml`, "Rust: tests in this file": lldb-dap runs `src/debug/fenix_cargo.py`, which builds `cargo test --no-run` for the file's target and passes the module as a filter (`only`). Every test of the file. | A launch for one test: `program` the test binary (`compiler-artifact`'s `executable` from the last run's JSON; its hash holds while the profile and features do), `args = [name, "--exact", "--nocapture"]`, `preLaunchTask` the `cargo test --no-run` task for its target. |
| Python | `pytest: all tests`, `pytest: current file` (`module = "pytest"`, `args = ["-q", "${file}"]`) | `args = ["-q", "<node id>"]`. |
| Go | `Go: test this package` (`mode = "test"`, `program = "${fileDir}"`, "add `-test.run` for one") | `args = ["-test.run", "^TestTable$/^negative_one$", "-test.v"]`. |
| Web | `Vitest: current file` (`run ${file}`), `Jest: current file` (`--runInBand --watchAll=false ${file}`) | Vitest `args = ["run", "<file>:<line>"]`; Jest `args = ["--runInBand", "--watchAll=false", "<file>", "-t", "^<name>$"]`. |

On 1.6 these are `tests-launch`'s answers (Fenix §11), with no
picker, no launch variable and no `debug.pick_launch`: the first
version's hand-off (its §7, "It's fragile") isn't needed. Rust's isn't
a `preLaunchTask` any more but a `build-first` plan (§6.1 below).

## 6. The packs, one brief each

Each brief is what its piece of work (§8) builds. All four already
have `filesystem = "project-read"` and `editor = "read"` and are on
`fenix = ">=1.5, <2"`, `api = "1.5"`, version 1.2.0 (their
`extension.toml`). Each gains, on the branch of §8.2:

```toml
[engine]
fenix = ">=1.6, <2"
api = "1.6"              # a test provider: the tests page, the gutter, SPC T

[[contributes.test_providers]]
# per pack, below
```

and its `impl Extension` gains `lang_kit::test_exports!()` (§4.3).
None needs a new `permissions.process` entry: Fenix holds a plan's
commands to the same list as `spawn`'s and `tasks-set`'s (Fenix
§5), and every program below is listed. None adds a key: the leader is
Fenix's `SPC T` (Fenix §7.4), and Go's `SPC m t` stays `tidy`.

### 6.1 Rust (E2)

**Manifest.**

```toml
[[contributes.test_providers]]
id = "rust"
name = "cargo test"
languages = ["rust"]
markers = ["Cargo.toml"]
scan = ["src/**/*.rs", "tests/**/*.rs", "*/src/**/*.rs", "*/tests/**/*.rs", "*/*/src/**/*.rs", "*/*/tests/**/*.rs"]
```

The pattern list is a crate at the top or a workspace's members one or
two folders down (`crates/orbit/src`), so nothing under `target/`
matches (generated code is in `target/debug/build/*/out/`). If Fenix's
scan honours the project's ignores (§11, F9), it becomes `["*.rs"]`.

**Permissions.** `cargo` (nextest runs as `cargo nextest`), `lldb-dap`
and `codelldb` are listed. Nothing to add.

**Scan.** Ports the rules of Fenix's `fenix-tasks/src/test_at.rs` (the
first version's §2): the module path from the file's place under its
target's root and the `mod` blocks around it; a test is an `fn` after
`#[test]`, `#[tokio::test]`, any `#[<path>::test]`, `#[rstest]` or
`#[test_case(..)]` (the last two `template`, their cases found in a
run); `#[ignore]` and `#[ignore = "why"]` set `skip`; a `#[cfg(test)]
mod tests {` line is the suite's. The target comes from `cargo metadata`,
which the pack reads already: the lib's, a bin's, or `tests/<name>.rs`'s
own. Groups: the target (`orbit (lib)`, kind group), the file, each
`mod` (suite). `end_line` by brace counting outside strings and
comments, given only when it closes. A macro-made test (`cases! { .. }`)
has no `fn`: nothing.

**Plans.**

| Ask | Steps |
|---|---|
| discover | One: `cargo test --workspace --no-fail-fast --message-format=json -- --list`, labelled "building the tests to list them" (it builds every test target: 20.9 s cold on lang-kit, 0.18 s warm, §3). Doc tests in the same listing: **not verified** (the first version listed them with `--doc -- --list` apart). |
| run-all | One: `cargo test --workspace --no-fail-fast --message-format=json`, `covers` empty. |
| run-these, a file, a group | One per package: `cargo test --no-fail-fast --message-format=json -p <pkg> <target flags> -- --exact <paths…>`, the target flags those of its tests (`--lib`, `--bin x`, `--test it`, several at once), `covers` the asked keys; doc tests in a step of their own with `--doc`, which cargo doesn't take beside other target flags (**not verified**). A target group alone: its flag, no names. A file or a `mod` group: `--exact` with the tests lang-kit knows under it (`Asked::Group.tests`), else the module path as a substring (`geo::`, which also runs `other::geo::`, §3). Split by `split` (2,000 failed names of 40 characters are 80,000). |
| With nextest (D2 of the first version, still standing) | The same selections as `cargo nextest run --no-fail-fast --message-format libtest-json` with `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` in `env`, filters `-E 'test(=a) \| test(=b)'` (one `test(=...)` verified, the `\|` not); doc tests (nextest has none) in a second `cargo test --doc` step. |

**Which binary a line is from.** cargo names the binary only on stderr
(`Running unittests src/lib.rs (target/debug/deps/orbit-…)`, `Running
tests/it.rs (…)`, `Doc-tests orbit`), its tests' lines are on stdout
(*verified*, first version's `filters.txt`), and on Windows Fenix keeps
their order only per 10 ms tick (Fenix §6.4). So the reader doesn't use the
order between them: each binary's stdout is one segment, from `running
N tests` to `test result:`, and the k-th segment is the k-th header's.
A segment whose header hasn't come yet is held. Each stream's own order
is all it needs.

**Reader** (libtest's human lines, cargo's JSON):

| It reads | It reports |
|---|---|
| stdout `{"reason":"compiler-artifact",…}` | `Keep` the target's `executable` |
| stdout `{"reason":"compiler-message",…}` with `level: error` | the first: message, `spans[0]` place, its target |
| stdout `{"reason":"build-finished","success":false}` | `Broken { under: that target's group, or none if no segment follows }` |
| stderr `Running …` / `Doc-tests …` | the next header |
| stdout `running N tests` | a segment opens |
| `test NAME ... ok`, `NAME - should panic ... ok` | `Ended` passed, no time (stable libtest has none per test, §3) |
| `test NAME ... ignored, slow` / `... ignored` | `Ended` skipped, message "slow" / none |
| `test NAME ... FAILED` | held until its block |
| `---- NAME stdout ----` to the next `----` or `failures:` | the failure's text; `thread 'NAME' (N) panicked at src/lib.rs:25:9:` gives `at`, the line after it the message; then the held `Ended` failed |
| `name: test` lines (a listing) | `Found`, the file and line from lang-kit's memory of the scan, else the module's file guessed, no line |
| `test result:` | the segment closes |
| stderr `process didn't exit successfully: … (signal: 6, SIGABRT…)` with a segment open | `Crashed { under: its target }`: "its binary crashed: SIGABRT" |
| A key the scan didn't see | `Found` under its module's suite; a key whose prefix is a known test (`tests::x::case_1` of an `#[rstest] fn x`) with `of` that test |

With nextest, its JSON lines name crate and binary (`orbit::orbit$tests::adds`,
*verified*) and carry `exec_time` and the failure's `stdout`: one reader,
no pairing.

**Launch.** `BuildFirst([cargo test --no-run --message-format=json -p <pkg> <target flag>])`,
whose reader `Keep`s the `executable`; asked again, `Now(LaunchConfig {
name: "Rust: <path>", languages: ["rust"], program: <executable>, args:
[<path>, "--exact", "--nocapture"], cwd: <the package's folder>, env:
[("CARGO_MANIFEST_DIR", <it>)], attributes: what `launch/test.toml`
gives lldb-dap, or CodeLLDB's })`. cargo runs a test binary in its
package's folder and sets `CARGO_MANIFEST_DIR`, so a test that reads its
fixtures by either finds them under the debugger too. A doc test:
`Err("a doc test can't be debugged on its own")`. The existing "Rust:
tests in this file" launch stays.

**Assets.** None.

**Fixtures** (`extensions/rust/src/samples/tests/cargo/`, `…/nextest/`):
from the appendix, `run-human`, `run-json` (libtest's unstable JSON, for
the nextest reader's shape), `compile-error-json`, `crash-json`,
`list`, nextest's `run-libtest-json`; to record: a workspace of two
packages with `it_works` in two targets; a crash in one of three
binaries in one step; `--workspace -- --list` with doc tests;
`--color=always`; a doc test run with `--exact`; `#[rstest]` and
`#[test_case]` cases; Windows paths by hand (`src\lib.rs:25:9`).

**Done when** (on the driver, then in a nightly from Fenix's step 3):

1. The sample's `src/lib.rs` scanned gives `tests::adds` and the rest at
   their lines, under `orbit (lib)` › `src/lib.rs` › `tests`; `ignored_one`
   skipped "slow".
2. The listing adds `tests::case_negative` (macro-made) under `tests`
   with no line.
3. A run of all: every outcome as recorded; `fails_on_purpose` failed at
   `src/lib.rs:25:9` with "assertion `left == right` failed: two and
   two" and its text; the build failure is one `broken` on `orbit
   (lib)`, no failed test; the crash errors that binary's tests only.
4. The same with stdout and stderr shuffled per tick: the same states.
5. 2,000 failed tests run again: steps each under 24,000 characters.
6. Debugging `tests::adds`: a build-first plan, then a launch of the
   `compiler-artifact`'s binary with `["tests::adds", "--exact", "--nocapture"]`.
7. In a nightly: listed without running, one runs from its line, a
   failure at its assertion, the failed again with one key (Fenix
   §15, E2).

Then E2 writes the recipe, `plans/test-explorer-recipe.md`: what a pack
implements, the manifest lines, how to record fixtures, the driver's
calls, the pitfalls met.

**Weak spots.**

- **No time per test on stable** without nextest; the doctor says
  "cargo-nextest gives each test's time".
- **Macro-made tests are located only when run or listed**, with no
  line; a failure's `at` is the macro's call (`src/lib.rs:43:5`, the
  `cases! {` line, §3).
- `#[path]` and `include!` modules aren't followed by the scan.
- **A doc test's key holds its line** (`src/lib.rs - add (line 6)`):
  editing above it makes it another test, its result lost.
- A listing builds every test target; a run waits behind rust-analyzer's
  or clippy's build (cargo's lock), which the step's output says.
- A test name in two binaries of one package runs in both when either is
  asked; both get results.
- Panic places are relative to the workspace's root, so a project opened
  at a member's folder gets them through `cargo metadata`'s
  `workspace_root`.

### 6.2 Python (E3a)

**Manifest.**

```toml
[[contributes.test_providers]]
id = "python"
name = "pytest"
languages = ["python"]
markers = ["pyproject.toml", "uv.lock", "requirements.txt", "setup.py", "setup.cfg", "pytest.ini", "tox.ini"]
scan = ["test_*.py", "*_test.py"]     # pytest's own python_files; F9: .venv is full of them
```

**Permissions.** `uv`, `python3`, `python` and
`${setting:python.interpreter}` are listed; a project's `.venv` Python
is found as the pack's tasks find it (`find_program` with the project's
path as a hint, `fenix-wasm/src/host.rs:2545-2556`, which then lets it
run, `1194-1196`). Nothing to add.

**The command.** As the pack's `test` task chooses (`src/tasks.rs`):
`uv run python` when uv manages the project and has pytest, `uv run
--with pytest python` when it doesn't, else the environment's Python;
then the boot of §5 with the plugin's folder as its first argument,
`-p fenix_tests`, and `env` `PYTHONIOENCODING=utf-8` plus the project's
`.env` as the tasks pass it. `cwd` the folder of the pytest config
(`pytest.ini`, `pyproject.toml` with `[tool.pytest.ini_options]`, else
the project): pytest's node ids are relative to its rootdir, so the
reader joins them to that folder.

**Plans.** Always one step.

| Ask | Arguments after the boot |
|---|---|
| discover | `--collect-only -q -p fenix_tests` |
| run-all | `-q -p fenix_tests` |
| a file, a class, a test, a case | its node id (any number of them; brackets and spaces need nothing, no shell) |
| more than a few | `@/data/lang-kit/tests/runs/<run>-0.txt`, one id a line (*rerun*), in `needs` |
| a template (`parametrize` seen by the scan) | its id without `[...]`: all its cases |
| a case cut to 1 KiB (§4.4) | its template |

**Ids.** The node id; groups the file and each class (suite).
`test_cases[negative]` is a case with `of` `…::test_cases`.

**Reader** (the plugin's `FENIX-TEST {json}` lines; every other line,
pytest's own, is passed over):

| `kind` | It reports |
|---|---|
| `test` | `Found` with its file and line (the plugin's line is from 1) |
| `result` | held until the next test's first result or the step's end, so its `setup`, `call` and `teardown` reports make one `Ended`: failed in `call` → failed; failed in `setup` (a fixture) → errored "setup: …"; a teardown failure after a pass → errored "teardown: …"; `skipped` → skipped with its message; `xfail` → skipped "xfail: …"; `at` the deepest frame in the test's file (`tests/sub/test_unit.py:9` for a `unittest` assertion, §3), else the crash's place; `text`, and each captured section as `Output` |
| `collect-error` | `Broken { under: none }`, the file and message: pytest ran nothing (exit 2, §3) |

Exit 5 (no tests collected) ends with nothing to say. `No module named
pytest` on stderr: the step's end, and `runner()` says "pytest isn't
installed in this environment" from then on.

**Launch.** `Now(LaunchConfig { name: "pytest: <id>", languages:
["python"], program: None, args: ["-q", "<node id>"], cwd: <the
rootdir>, attributes: {"module": "pytest", "justMyCode": false} })`, as
`launch/pytest-file.toml` with the node id for `${file}`.

**Assets.** `fenix_tests.py`, the first version's 46 lines unchanged, in
`extensions/python/src/tests/`.

**Fixtures** (`extensions/python/src/samples/tests/pytest/`): from the
appendix, `run-plugin`, `collect-plugin`, `collect-error`; the rerun's
`argsfile-and-boot`. To record: `os._exit` in a test (a crash), a
teardown that fails after a pass, a strict xpass, a test named in
Japanese (the plugin's escaped JSON), pytest-xdist's `-n 2` (whether the
plugin sees the workers' reports: **not verified**), exit 5.

**Done when.** The four of §6.1's 7, on the fixtures: listed without
running (from the scan, then the listing's cases), one by its node id,
a failure at its assertion with the traceback, the failed again in one
step; a fixture's failure is errored, not failed; a collection error is
one `broken`.

**Weak spots.** A listing imports every test module (its top-level code
runs): Fenix runs listings only in a trusted project (Fenix §12). Ids over
1 KiB run their template. Captured output over 8,000 characters a
section is cut by the plugin. On Windows, pytest's own text in *Test
output* needs `PYTHONIOENCODING` (**not verified on Windows**); the
plugin's lines don't (escaped ASCII, §3). A project whose tests need
`DJANGO_SETTINGS_MODULE` or a plugin's options gets them from its
config as pytest does; one that needs a wrapper script isn't served.

### 6.3 Go (E3b)

**Manifest.**

```toml
[[contributes.test_providers]]
id = "go"
name = "go test"
languages = ["go"]
markers = ["go.mod", "go.work"]
scan = ["*_test.go"]
```

**Permissions.** `go` and `dlv` are listed. Nothing to add.

**Scan.** `func TestXxx(t *testing.T)`, `ExampleXxx()`, `FuzzXxx(f
*testing.F)` at a line's start (benchmarks aren't run without `-bench`:
left out). The import path from the module's path in `go.mod` (or each
module of `go.work`) and the file's folder; a `package x_test` file is
in the same package for `go test`. A test whose body calls `t.Run(` is a
`template`. Groups: the package (kind group, labelled by import path),
the file. `end_line` by brace counting.

**Plans.**

| Ask | Steps |
|---|---|
| discover | One: `go list -f '{{.ImportPath}} {{.Dir}} {{.TestGoFiles}} {{.XTestGoFiles}}' ./...` (no build, even when the code doesn't compile, §3): each package and test file as a group, so files the scan didn't reach are in the tree. No `go test -list`: it builds every test binary and gives no lines. |
| run-all | One: `go test -json -fullpath ./...` |
| top-level tests | One: `go test -json -fullpath -run '^(TestA\|TestB)$' <their package folders>`, names through `regexp.QuoteMeta`; a name in another listed package runs there too, and is reported |
| subtests | One more step per parent: `-run '^TestTable$/^(negative_one\|small)$'`, a level per `/` (`TestTable/a/b` as `'^TestTable$/^a$/^b$'`, §3) |
| long selections | `split` |

Go's cache stays on: a cached package still gives each test's events
(*rerun*), with `Elapsed` 0, so its times show as 0 ms; no `-count=1`
(D-G1 in §7).

**Ids.** `<import path> <name>`; a subtest is a case with `of` its
parent, found in a run.

**Reader** (`go test -json` on stdout; stderr only for Go before 1.24):

| Event | It reports |
|---|---|
| `run` | `Found` for a subtest (kind case; parent `TestTable/a` if it ran, else `TestTable`, which makes `"a/b"` a name), then `Started` |
| `output` with `Test` | `Output`; a `<full path>:<line>: <text>` line (`-fullpath`, *rerun*) kept as the test's `at` and message |
| `pass`, `fail`, `skip` with `Test` | `Ended`, `Elapsed` × 1000 ms; a skip's message from its last `x_test.go:12: reason` line; a panic's `fail` failed with "panic: …" |
| `build-output`, `build-fail`, a package `fail` with `FailedBuild` | `Broken { under: the package }`, the first error and its place (`geo/geo_test.go:6:13`, Go 1.24, §3) |
| a package `fail` after a panic, its later tests unreported | `Crashed { under: the package }` |
| a package `fail` with no test events and no `FailedBuild` (before Go 1.24), stderr text | `Broken`, the stderr lines as the message (**not verified**, §3) |

**Launch.** `Now(LaunchConfig { name: "Go: <name>", languages: ["go"],
program: <the package's folder>, args: ["-test.run", "^TestTable$/^negative_one$", "-test.v"],
cwd: <it>, attributes: {"mode": "test"} })`, as `launch/test-package.toml`.

**Assets.** None.

**Fixtures** (`extensions/go/src/samples/tests/`): from the appendix,
`run-json`, `compile-error-json`, `go-list`; the rerun's
`fullpath-and-cache`. To record: Go before 1.24's build failure, a
`TestMain` that panics, `t.Parallel` subtests interleaved, a fuzz test's
seeds (`FuzzX/seed#0`), an `Example` whose output differs, a `go.work`
of two modules.

**Done when.** The four, on the fixtures; subtests appear under their
parent once run; one package's build failure leaves the others' results
standing.

**Weak spots.** Subtests exist only once run, at their parent's line; a
subtest named `"negative one"` is `negative_one`, and `"a/b"` can't be
told from a nested one except by whether `a` ran; tests in files past
Fenix's 2,000-file first scan (Fenix §6.7) appear when their file is opened or
run; a file excluded by a build tag (`//go:build integration`) is
scanned anyway and its tests never run.

### 6.4 Web: Vitest and Jest (E3c)

One provider for both (D-W1 in §7): a project's runner comes from its
`package.json`, and a monorepo with both gets a step per runner.

**Manifest.**

```toml
[[contributes.test_providers]]
id = "web"
name = "Vitest or Jest"            # test-provider-set says which
languages = ["javascript", "typescript", "tsx"]
markers = ["package.json"]
scan = ["src/**/*.test.*", "src/**/*.spec.*", "test/**", "tests/**", "**/__tests__/**",
        "packages/*/src/**/*.test.*", "packages/*/src/**/*.spec.*", "apps/*/src/**/*.test.*"]
```

Paths, not names: `*.test.js` as a name matches every test file under
`node_modules`. With F9, names (`*.test.*`, `*.spec.*`).

**Permissions.** `node` and `${setting:web.node}` are listed. The
runners are run as the launch files run them, `node
node_modules/vitest/vitest.mjs` and `node node_modules/jest/bin/jest.js`
(`launch/vitest-file.toml`, `jest-file.toml`), never `npx` or a
`node_modules/.bin` shim: on Windows those are `.cmd` files run through
`cmd.exe`, whose line is 8,191 characters. Nothing to add.

**Which runner.** `runner()`: the package folders (the project's, and
each workspace package's) whose `package.json` has `vitest` or `jest`
in its dependencies; "vitest", "jest", or "vitest and jest". Neither:
`problem` "no test runner in package.json" (§11, F8, would hide it
instead). Yarn Plug'n'Play (no `node_modules`): "Yarn Plug'n'Play
projects aren't served yet". Each step's `cwd` is its package's folder.

**Ids.** `<file below the project> > <names joined by " > ">`; groups the
package folder (when there are two or more), the file, each `describe`
(suite). `test.each` cases have `of` their template when the scan saw
it: same file, same line (§4.3).

**Vitest plans.**

| Ask | Arguments after `node node_modules/vitest/vitest.mjs` |
|---|---|
| discover | `run --includeTaskLocation --reporter=${data}/lang-kit/tests/fenix-vitest.mjs -t FENIX_NOTHING` (*rerun*: every test, cases expanded, lines, exit 0) |
| run-all | `run --includeTaskLocation --reporter=default --reporter=${data}/lang-kit/tests/fenix-vitest.mjs` |
| files, tests, groups | the same with the files and `-t '^(outer > inner > deep\|adds)$'`, a regex on the full name joined by ` > ` (§3), names escaped |
| a template the scan saw (`add(%i, %i) is %i`) | `<file>:<line>` (§3): its name has placeholders |

By name, not `file:line`, because the scan's line can be the unsaved
buffer's and Vitest reads the file on disk (§11, F18).

**Jest plans.**

| Ask | Arguments after `node node_modules/jest/bin/jest.js` |
|---|---|
| discover | `--ci --testLocationInResults --reporters=${data}/lang-kit/tests/fenix-jest.js -t FENIX_NOTHING` (§3: every test `pending` with its line, cases expanded, exit 0) |
| run-all | `--ci --watchAll=false --testLocationInResults --reporters=default --reporters=${data}/lang-kit/tests/fenix-jest.js` |
| files, tests, groups | the same with the files and `-t '^(outer inner deep\|adds)$'`, names joined by spaces and escaped (`add\(-1, -1\)`, §3); a group: `-t '^outer inner '` |

**Readers** (the reporters' `FENIX-TEST {json}` lines; the default
reporter's text is passed over):

| Line | It reports |
|---|---|
| Vitest `test` | in a discover step: `Found` (its `mode` is `skip` for all there, so `skip` comes from the scan); in a run: `Found` for a case the scan didn't have |
| Vitest `result` | in a discover step, nothing; else `Ended`: `state` passed, failed, skipped; `duration` ms; message the first error's; `at` the first frame of its stack in the project (`at …/src/orbit.test.js:27:25`, `file://` taken off); `text` the stack and `diff` |
| Vitest `module-error` | `Broken { under: the file }` (§3) |
| Jest `test` | in a discover step, `Found`; in a run, a test that didn't run (`pending`, `todo`, `skipped`): `Ended` skipped |
| Jest `result` | `Ended`: `status` passed, failed, pending → skipped; `duration` ms; message the failure's first line; `at` its first frame in the project (`at Object.toBe (/…/src/orbit.test.js:9:21)`) |
| Jest `file-error` | `Broken { under: the file }` (**not verified**, §3) |

A frame's place is read from its end (`:9:21)`), so `C:\src\orbit\x.test.js:9:21`
keeps its drive letter (**not verified on Windows**).

**Launch.** Vitest: `program` `<package>/node_modules/vitest/vitest.mjs`,
`args ["run", "<file>:<line>"]`; Jest: `program`
`<package>/node_modules/jest/bin/jest.js`, `args ["--runInBand",
"--watchAll=false", "<file>", "-t", "^<names>$"]`; `cwd` the package,
`attributes` as the launch files' (`type = "pwa-node"`, `skipFiles`,
Vitest's `autoAttachChildProcesses`).

**Assets.** `fenix-vitest.mjs` (31 lines) and `fenix-jest.js` (22 lines),
the first version's, unchanged, in `extensions/web/src/tests/`.

**Fixtures** (`extensions/web/src/samples/tests/vitest/`, `…/jest/`):
from the appendix, Vitest's `run-reporter`, `syntax-error`,
`ansi-forced`; Jest's `discover-reporter`, `run-reporter`; the rerun's
Vitest `discover-reporter`. To record: a Jest file that doesn't load
(`testExecError`), a Vitest and a Jest worker killed mid-run, two tests
with one name, a monorepo with Vitest in one package and Jest in
another, Vitest's `projects` config, Windows stacks by hand.

**Done when.** The four, for each runner, on the fixtures; `test.each`
cases under their template; a file that doesn't load is `broken` and
the other files' results stand.

**Weak spots.** A listing loads every test file, Jest's through its
transforms (slow on a large project, **not measured**), and runs their
top-level code (trust, Fenix §12). `test.each` cases are at their
template's line. Vitest's listing through the reporter can't tell
`skip` from `todo`. Names with regex characters are escaped; a describe
named with ` > ` (§4.4). Vitest's reporter API is 3's and later
(verified on 5.1.0 only), Jest's `onTestCaseResult` (30.5.2 only).
Yarn Plug'n'Play, Deno's and bun's runners aren't served.

## 7. Decisions

### The first version's

| | Was | Now |
|---|---|---|
| D1 | Rust's locations: scan, checked by `--list` after a build | **Stands**, the pack's (§6.1). |
| D2 | Rust: libtest's human lines, nextest when installed | **Stands**; `test-provider-set` says "cargo nextest" when it's used. How nextest is read is D-R1 below. |
| D3 | Run on save off by default | **Fenix's**: `tests.on_save`, off (Fenix §8.4, §13). |
| D4 | `lang-kit::tests` rather than a crate of its own | **Settled by the split**: what's left is the adapter, which needs lang-kit's `Kit` (Fenix §6.6, §15 E1). |
| D5 | What to ask of Fenix (tree kill, `debug::start`, gutter marks, a task's output, `last_task`) | **Settled by the split**: Fenix §8.2, §11, §9.1, §10.2. |
| D6 | `SPC m t`, Go's `tidy` moving | **Settled**: Fenix's `SPC T` (Fenix §7.4); was Fenix's Q3. Go keeps `tidy`; no major. |
| D7 | The floor: Fenix 1.5 | **Replaced**: the four packs need `api = "1.6"`, `fenix = ">=1.6, <2"` (Fenix §6.2). |
| D8 | Should this be Fenix's own? | **Settled**: yes, ADR 0005. |
| D9 | Problems for failures, a setting to turn off | **Fenix's**: `tests.problems` (Fenix §9.3, §13). |

Fenix's open questions Q1 to Q7 (Fenix §16) are all Fenix's; none waits
on this side. Q6 (readers for standard formats in Fenix) would change
nothing here if taken later: the packs keep their readers.

### Still open here

| | Question | Options | Recommended |
|---|---|---|---|
| D-A1 | Where the providers' code lives while API 1.6 is unreleased | (a) on `main` against types mirrored from Fenix §6.6, only the API pin and the exports on a branch (§8.2); (b) all of it on a long-lived branch | **(a)**: `main`'s CI tests it every day, the branch stays a few small commits, and E1 can start before Fenix's step 2. The cost: packs released from `main` before 1.6 carry the unused provider (size measured in each pull request, as `kits-to-1.5.md` measured packs). |
| D-A2 | The 24 kit users that don't run tests, when the kits move to `v1.6.0` | (a) their locks only, no release, `api` left at 1.5; (b) each a minor on `api = "1.6"` | **(a)**, as `CONTRIBUTING.md`'s "Shared crates" allows when behaviour doesn't change. Their next release keeps `api = "1.5"` only once a 1.6-built component is seen to load on Fenix 1.5 (§11, F15); otherwise it raises its floor. |
| D-W1 | Web: one provider or two (`web.vitest`, `web.jest`) | (a) one, `web`, its runner per package; (b) two, each hidden where it doesn't serve | **(a)**: works with Fenix's plan as it stands. (b) needs a way to say "not here" (§11, F8) and shows two roots in a project with both. |
| D-R1 | How nextest is read | (a) `--message-format libtest-json` with `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`; (b) its human lines, the failures from its JUnit file at the end | **(a)**, checked on 0.9.151 (§6.1): one stream with times, binaries and failures' output, nothing parsed whole at the end, nothing read from disk (which a remote project can't). It's experimental: the doctor checks nextest's version, and (b)'s human lines (times, no failure text) are the fallback when the JSON doesn't parse. |
| D-G1 | Go's cache | (a) keep it (Go's default); (b) `-count=1` on every run | **(a)**: a cached package still reports each test (*rerun*); "run again" of a passing test is then instant and says 0 ms. (b) is a setting if users ask. |

## 8. The work, cut for delegation

### 8.1 The pieces

Aligned with Fenix's steps (Fenix §15). Sizes as the roadmap
writes them (S a few days, M one to two weeks).

| # | Piece | Needs | Size | Tier | Owns | Done when |
|---|---|---|---|---|---|---|
| E1a | **lang-kit's adapter, on `main`**: `crates/lang-kit/src/tests/` (`mod.rs` the trait and `Kit`'s five methods, `api.rs` the 1.6 types mirrored from Fenix §6.6, `adapter.rs` readers per step, cut lines, groups, what it remembers, crashes, `split`, assets and `needs`, `provider.rs` the runner told on change); the test kit's driver (§4.5) behind its `lang-kit` feature; lang-kit's README section | nothing of Fenix's: Fenix's plan §6.3 and §6.6 as written, plus whatever the maintainer takes of §11 before step 2 | M | **design-heavy, top tier**: it fixes the trait four packs (and .NET and Java after) build on, and the driver's model must match Fenix's rules exactly or the packs' tests pass on a lie | `crates/lang-kit`, `crates/test-kit` | A made-up runner (scan `# test name` lines, run `PASS name 3ms` lines, as Fenix's own guest, §14) passes through every row of §4.3 and §4.5 in tests; every lang-kit pack's tests pass unchanged (`cargo test --locked`, 16 packs) |
| E1b | **On the branch, against Fenix's step 2**: the kits' API to Fenix's commit, the 30 locks (§8.2), `api.rs` replaced by `pub use fenix_extension_api::tests::*`, `test_exports!()`, the harness's `tests_*` and the fake's `test-provider-set`, `xtask validate` checking `[[contributes.test_providers]]` as Fenix §5 does | Fenix's step 2 merged on Fenix's `master` (a commit to pin), or its pull request's head while it's open | S | recipe-following | the kits' `Cargo.toml` and locks, every extension's `Cargo.lock`, `xtask/src/main.rs` | The branch builds and its CI passes; the mirror compiled away with no change to E1a's code (a difference is a finding for Fenix's step 2, not a patch here) |
| E2 | **Rust** (§6.1) and the recipe | E1a; its manifest and exports on the branch after E1b | M | **design-heavy, top tier**: the weakest runner, the pairing of streams, and the recipe the others copy | `extensions/rust` (and `plans/test-explorer-recipe.md`) | §6.1's 1 to 6 on the driver; 7 in a nightly from Fenix's step 3 (debugging from its step 6) |
| E3a | **Python** (§6.2) | E2's recipe | S | recipe-following | `extensions/python` | §6.2's, on the driver; then in a nightly |
| E3b | **Go** (§6.3) | E2's recipe | S | recipe-following | `extensions/go` | §6.3's |
| E3c | **Web** (§6.4) | E2's recipe | M | recipe-following, the most careful of the three: two runners, two listings, packages | `extensions/web` | §6.4's, for each runner |
| E4 | **Releases** (§9) | Fenix 1.6.0 tagged; E2, E3a-c | S | recipe-following | the kits' pins, every lock, the four packs' versions and CHANGELOGs | §9 |

- E1a starts now; E2 beside it once E1a's trait is merged as a stub (E2
  keeps E1a honest, as the first version planned its pieces 2 and 3).
  E3a, E3b and E3c run in parallel after E2's recipe; each owns one
  pack's folder and nothing else. A change one needs in the kit goes back
  to E1a's owner, not into the kit from a pack's branch.
- E1b waits for Fenix's step 2. The packs' manifests and `test_exports!()`
  go on the branch as each pack's piece ends.
- **Without Fenix** (on the driver): every scan, plan, reader and launch
  answer, on recorded output, both stream orders, the limits of Fenix §6.5,
  the time per call natively. **Only in a nightly**: the model's real
  behaviour from these reports, Windows (paths, command lines, the order
  of streams, `PYTHONIOENCODING`), the cost of a call in wasm, a stop
  while cargo's test binary runs, a debug session from a launch, the
  listing that `test-provider-set` starts, 10,000 real tests. These go in
  each pack's "Where to look first in a real Fenix" list, as milestone 3
  did.
- What goes from the first version's list: piece 1 (lang-kit's seams
  and the problems ledger), most of piece 2 (model, page, store,
  agents), piece 5 (issues for Fenix) and piece 6's screenshots of the
  page (it's Fenix's). Fenix §15, "What goes from the first
  design's work list", says the same.

### 8.2 Building against an unreleased API 1.6

The kits build `fenix-extension-api` from `tag = "v1.5.0"`, and every
kit user must build it from the same source, or its types aren't the
kit's (`plans/kits-to-1.5.md`); the 28 kit users and the two
`xtask new` templates have that source in their `Cargo.lock`, and CI
runs `cargo test --locked` on every one of them on every push
(`.github/workflows/ci.yml`). API 1.6 exists only on Fenix's `master`
from step 2 until 1.6.0 is tagged (Fenix §6.2). So:

| When | On `main` | On the branch `api-1.6` |
|---|---|---|
| Now | E1a, E2, E3: everything but the exports and the manifests, against `lang-kit/src/tests/api.rs`, a copy of Fenix §6.6's types (`Command` and `LaunchConfig` are 1.5's already). Every pin stays `v1.5.0`; nothing on `main` names 1.6. | — |
| Fenix's step 2 merged at commit `X` | — | Made from `main`: the three kits' `fenix-extension-api = { git = "https://github.com/tpedneault/fenix", rev = "X" }`; `cargo update -p fenix-extension-api` in all 28 kit users and the two templates, so CI's `--locked` passes on the branch; E1b. A pin by `rev`, not `branch`, so the lock says exactly what was built. |
| As each pack's piece ends | its provider and tests | its manifest's provider and `[engine]`, `test_exports!()`, version 1.3.0: packed and tried in a nightly |
| Fenix's `master` changes the API before 1.6.0 (Fenix §6.2 allows it, by agreement) | — | `rev` moved, locks again: one commit |
| Fenix 1.6.0 tagged | One pull request: the kits to `tag = "v1.6.0"`, the 28 locks and the templates (step 3 of `kits-to-1.5.md`'s recipe); the 24 that don't run tests keep their version and `api` (D-A2). Then the branch, rebased, its `rev` now the tag, merged. | deleted |

`main` stays green throughout: until the tag, nothing on it needs 1.6.
The branch's own commits stay few (pins, locks, the mirror's removal,
four manifests, the glue), so rebasing it is cheap.

Not a cargo feature: a feature can't move a git dependency's tag, and a
second copy of the API crate in one build would export the `extension`
world twice (the crate generates the world's exports,
`crates/fenix-extension-api/src/lib.rs:47-59` in Fenix). Not a `[patch]`
in each pack: cargo doesn't let a patch point at the source it patches
(**not tried**), and it would put an unreleased API into whatever `main`
releases.

## 9. Releases

Only after Fenix 1.6.0 is out (Fenix §6.2: no pack with `api =
"1.6"` goes to the store before).

1. The kits' move to `v1.6.0` and the branch, merged (§8.2).
2. **Rust 1.3.0, Python 1.3.0, Go 1.3.0, Web 1.3.0**, each a minor with
   `api = "1.6"`, `fenix = ">=1.6, <2"`, the same day, Rust's first if
   they're staggered (it was tried longest in the nightly). Each
   CHANGELOG opens "For Fenix 1.6 (extension API 1.6): your tests on
   Fenix's tests page (`SPC T T`) ...", saying what its runner gives
   and what it doesn't (Rust's times without nextest, Go's subtests once
   run). No major: no key moves.
3. Each README: the tests, the runners and versions verified, what a
   listing runs (trust), the weak spots of §6.

**Users on Fenix 1.5** see nothing change: a 1.3.0 doesn't install on
1.5 (`api = "1.6"`), and the registry offers them 1.2.0, the newest
that fits (Fenix §6.2). The Python bundle names `fenix.python`
by `^1` (`extensions/python-bundle/extension.toml`), so it does the
same. The 24 other kit users aren't released (D-A2).

## 10. Risks, on this side

- **The API freezes at Fenix's step 2.** What this side found that
  needs a field or a case (§11: F1, F4, F6, F7, F8) has to be in the WIT
  before step 2 merges, or it's a new export later (Fenix §6.2).
  If it isn't taken, the workarounds of §4.3 stand, at the costs §11
  gives.
- **The mirror drifts** from what step 2 merges. E1b's compile is the
  check; a difference is settled with Fenix before the branch goes on.
- **Rust's run pairs stdout's segments with stderr's headers by order.**
  A binary that dies before printing `running N tests` leaves a header
  with no segment, which shifts the rest: the reader then checks each
  segment's names against the target's tests it knows, and says when
  they don't fit. Not seen in the recorded runs.
- **Per-call cost in wasm** isn't measured. The driver times calls
  natively; the nightly gives the real figure. If 512 lines is too much
  for one call, it's Fenix's batch size that changes (Fenix
  §17), not the packs.
- **A 1.6-built extension with `api = "1.5"` on Fenix 1.5** isn't known
  to load (§11, F15). It decides whether the 24 kit users' next releases
  can keep their floor. Nothing is released on the assumption.
- **Runner versions**: the reporters use Vitest's 3+ reporter API
  (verified on 5.1.0 only), Jest's `onTestCaseResult` (30.5.2 only),
  pytest's hooks (9.2.0 only; they're old and stable), Go's build events
  (1.24+). Older versions are **not verified**; each pack's doctor checks
  the version and says what falls back. nextest's libtest JSON is
  experimental (D-R1).
- **Listing runs code**: pytest imports the test modules, Vitest and Jest
  load them, cargo builds `build.rs`. Fenix lists automatically only in
  a trusted project (Fenix §12); each README says so.
- **Windows** is read from source and recorded output only: paths in
  frames, command-line lengths, Python's encoding, the order of streams.
  The driver covers what a recording can show; the rest is the nightly's.
- **Size**: every lang-kit pack carries the adapter, used or not; the
  four carry their providers. Measured in each pull request; if it's
  large, lang-kit's `tests` module goes behind a feature the four turn on.

## 11. For Fenix's plan

> **Ruled on, 13 November 2026.** Fenix's plan took all nineteen (its
> §18, "Review, 13 November 2026"); F1 and F2 with a fix other than the
> one proposed here. The API changed in four places, and E1a's copy of
> the types follows them:
>
> - `found-test` has `data: option<string>` (4 KiB at most), given back
>   in an ask and in `tests-launch` (F6).
> - `test-plan` has `list-after: bool` (F7).
> - `crashed` carries `test-crash { id: option<string>, message }` (F4).
> - **F1**: a group, file or suite id in `covers` stands for what's under
>   it; an empty `covers` claims nothing, and what no step names is
>   judged when the plan's last step ends.
> - **F2**: after an asked file or suite, Fenix sends the tests it has
>   under it (never under a group), 10,000 records an ask at most.
>
> Whether a component built on the 1.6 crate with `api = "1.5"` loads on
> Fenix 1.5 (F15, D-A2) is read as yes, and is a test in Fenix's step 2.
> What follows is the review as it was sent.

The cross-check: Fenix's plan read as the person who implements a
provider against it, with §3's facts in hand. Each finding says where
in Fenix's plan, what doesn't fit, the runner case, a fix, and where the
fix belongs. Fenix's plan isn't changed here; these go to the
maintainer. Findings fixed in this plan are at the end.

### Findings for Fenix's plan

**F1. A step's `covers` can't name a group** (Fenix §6.3, `test-step.covers`;
§8.5). `covers` is "the ids it runs"; empty is "what the steps before it
don't cover". A plan of several steps that each run a group (Rust per
package, Go per package set, Web per runner in a monorepo) must list
every test id for each, which the provider doesn't have (Fenix has the
model): with `covers` empty, the first step claims the whole ask, and
when it ends, the tests the later steps will run are errored "the run
ended without a result for it". *Fix*: a group's id in `covers` stands
for everything under it. Belongs in Fenix's plan. Here: Rust's and Go's
run-all are one step; lang-kit lists the keys it knows for a group's
step, which may miss some.

**F2. Asking to run a group gives the provider the group alone**
(Fenix §6.3, `test-ask.run`: "groups (all under them)"). libtest's filter is
a substring (`cargo test --lib -- geo::` also runs `other::geo::`, §3),
so running a Rust module exactly needs its tests' names; a file of a
Jest project with two describes of one name, likewise. *Fix*: with each
group asked, Fenix also sends the tests and cases it has under it, or
the doc says it never does. Fenix's plan. Here: lang-kit sends what it
remembers (`Asked::Group.tests`), else the substring.

**F3. What a `found` for a known id does isn't said** (Fenix §4, "Ids, and
matching"; "What's dropped"). cargo's `--list` and Go's `go list` give
no line, and Rust's listing can't tell `src/lib.rs`'s inline `mod tests`
from `src/tests.rs`; a listing's `found` replacing the scan's would take
the gutter marks away and move tests between files. *Fix*: a later
`found` for a known id sets the fields it gives and keeps the ones it
leaves `none` (`path`, `line`, `end-line`, `skip`), and a node keeps
which sources gave it (scan, listing, run), each source's dropping rule
applying to its own. Fenix's plan. Here: lang-kit fills a listing's
gaps from the last scan.

**F4. `crashed` has no scope** (Fenix §6.3, `test-report.crashed`: "what the
step covers that hasn't ended is errored with it"). `cargo test
--workspace` runs every test binary in one step and `go test ./...`
every package; with `--no-fail-fast` a crashed binary's neighbours still
run (`process didn't exit successfully: … (signal: 6, SIGABRT …)`, §3).
A `crashed` read when the first binary dies errors the tests of
binaries not yet started, whose results then come. *Fix*: `crashed`
gets `id: option<string>` as `test-broken` has. Fenix's plan (WIT).
Here: lang-kit errors the crashed group's known tests itself and sends
`crashed` only at the step's end.

**F5. A second `ended` for one id in a run isn't defined** (Fenix §4,
"States"; §6.3). pytest's plugin reports a test's `call` passed, then
its `teardown` failed (two `result` lines, one id); Jest allows two
tests of one name; libtest says `FAILED` long before the failure's text.
*Fix*: say that a later `ended` in the same run replaces the earlier.
Fenix's plan (one sentence). Here: readers hold a result until it's
whole, and lang-kit merges duplicates.

**F6. A `found-test` carries nothing of the provider's own** (Fenix §6.3).
Every plan rebuilds the runner's arguments from the id and label: a
describe named `a > b` can't be split back from `file > a > b > test`;
a pytest id over 1 KiB is cut by Fenix (Fenix §6.5) and no longer names the
case; Rust's target and binary have to be parsed out of the id. *Fix*:
`data: option<string>` (at most 4 KiB), given back as it was in
`test-ask.run` and `tests-launch`, never shown. Cheap now, a new export
later. Fenix's plan (WIT). Here: ids are built to be parsed back (§4.4),
cut ones run their template.

**F7. "After a run that built" can't be known** (Fenix §7.1, "List"). Nothing
in a step's reports or end says that it built; Fenix can only guess
from the provider. Rust's listing is free after a run (0.18 s warm, §3)
and costs a full build before one. *Fix*: `test-plan` gains
`list-after: bool` (or a report case), set by a provider whose listing
is cheap after the plan ran. Fenix's plan (WIT). Here: without it, Rust
lists only when Fenix asks.

**F8. `test-provider-set` lists again on every call, and has no "not
here"** (Fenix §6.3, the import; §7.1). A pack that tells Fenix its runner as
it starts (nextest found) would make Rust build its tests at every
start, against Fenix §7.1's "not when the project opens"; a reader that sets a
problem when its listing fails would loop. And a pack whose markers
match a project it has nothing to run in (Web in a project whose
`package.json` names no runner) can only show a problem on the page.
*Fix*: a call with what Fenix already has changes nothing; the listing
it causes follows Fenix §7.1 (only once the page has opened in the session,
in a trusted project); and `runner = ""` with no problem means "nothing
to run here", hiding the provider in this project. Fenix's plan. Here:
lang-kit tells Fenix only what changed; Web shows the problem.

**F9. The project scan has no exclusions** (Fenix §5, `scan`; §6.7). Name
patterns match at any depth (`file_matches`,
`crates/fenix-extensions/src/manifest.rs:408-415`): `*.test.js` matches
`node_modules`, `test_*.py` matches `.venv`'s site-packages (numpy alone
ships hundreds of test files), `*.rs` matches `target/`. With 2,000
files a provider (Fenix §6.7), the project's own tests may never be scanned.
*Fix*: the walk honours the project's ignores as Search does, and says
so. Fenix's plan. Here: path patterns per pack (§6), narrower than they
should be.

**F10. The forms of paths aren't the same in both directions** (Fenix §6.3).
`tests-scan`'s `path` is "a full path", which a provider can't turn into
its module path or read beside with `std::fs` unless it's `/project/…`;
and a `found-test.path` that's relative is "relative to the project"
while a `test-location.path` is "relative to the step's folder". *Fix*:
`tests-scan` gives `/project/<path>` with `/` for a project's file;
both records take relative paths the same way (the step's folder for
reports from a run, the project for scans). Fenix's plan. Here: lang-kit
makes every path `/project/…` itself (pytest's rootdir, Web's package
folders).

**F11. A command line's length isn't a stated limit** (Fenix §6.5, §8.2).
Windows takes 32,767 characters, and `cmd.exe` 8,191 for a `.cmd` shim,
which `resolve_command` runs on Windows (Fenix §8.2). 2,000 failed Rust tests
run again by name are about 80,000 characters. *Fix*: Fenix §6.5 states the
limit, and a step over it ends `failed("its command line is N
characters; Windows takes 32,767")` on every platform, so a provider's
mistake shows on Linux too. Fenix's plan. Here: `split`, pytest's
`@file`, and Web never runs a shim.

**F12. A run whose provider is started again for another project**
(Fenix §6.4, "After a trap" covers a trap only). Moving to another project
starts a language pack again there (`WRITING_EXTENSIONS.md`, and
`WasmHost::stale` in `app/wasm_host.rs`), and its readers' state is
gone. *Fix*: say what happens: the run is stopped as after a trap, or
the old instance keeps its calls. Fenix's plan. Here: a step with no
reader gets a new one.

**F13. Where `needs` land on a remote machine** (Fenix §6.3, `needs`; §8.6).
"Copied to `~/.cache/fenix/tests/<extension>/`, and `${data}` is that
folder" doesn't say a need keeps its path below `/data`
(`/data/lang-kit/tests/fenix_tests.py` → `${data}/lang-kit/tests/fenix_tests.py`),
nor that a file written for one run (pytest's ids) is copied each time.
*Fix*: say both. Fenix's plan.

**F14. A "complete" listing isn't defined** (Fenix §4, "What's dropped").
Exit codes are the runners': pytest exits 5 when it collects nothing and
2 on a collection error, Jest's listing trick and Vitest's exit 0 (§3).
*Fix*: a listing is complete when every step `exited` (any code), none
was stopped, timed out or failed, and no `broken` was reported. Fenix's
plan.

**F15. A guest built from the 1.6 crate that keeps `api = "1.5"`**
(Fenix §6.2, §6.6). Every kit user rebuilds on the 1.6 crate when the kits
move, and that crate exports the five functions, whose types are new
records of `interface types` (Fenix §6.6: "every extension rebuilt on the 1.6
crate exports these"). Whether such a component loads on Fenix 1.5.x
isn't said; step 2's tests check the other way ("a 1.5 guest still
loads"). It decides whether 24 extensions here must raise their floor
at their next release. *Fix*: step 2 loads a guest built from the 1.6
crate, `api = "1.5"`, on the 1.5 host (`fenix-wasm` at `v1.5.2`), and
Fenix §6.2 says the result. Fenix's plan.

**F16. Python's encoding on Windows, as Fenix §17 says it**. "Non-ASCII test
names arrive replaced": not through the plugin, whose lines are escaped
ASCII (`json.dumps`, §3). What's at risk is pytest's own text in *Test
output*, which `PYTHONIOENCODING=utf-8` still fixes. *Fix*: the risk's
wording. Fenix's plan.

**F17. Names and numbers** (Fenix §6.3, §15). `test-provider-set`'s doc
names `contributes.test-providers`; the manifest's is
`[[contributes.test_providers]]` (Fenix §5). Fenix §15's E1 includes "the packs'
engine to 1.6", which here is each pack's piece, with E1 split in two
(E1a, E1b) and releases as E4 (§8). The example id `lib tests::adds`
(Fenix §4, §6.3) is `orbit/lib tests::adds` here (§4.4). Fenix §6.6's group ids
start with U+001F here. *Fix*: the doc comment, and Fenix §15's table to
match §8.1. Fenix's plan.

**F18. A run reads the disk; a scan may be the buffer's** (Fenix §7.1, §9.4).
A test typed and not saved is in the model, and `SPC T t` on it runs a
runner that can't see it; a plan built from an unsaved scan's line
(Vitest's `file:line`) runs another test. *Fix*: say whether a run
saves the project's files first (as `agents.save_before_prompt` offers),
or that a run of an unsaved file says so. Fenix's plan. Here: Vitest
runs by name.

**F19. After a `build-first` plan that failed** (Fenix §11, step 2). "Then
`tests-launch` is asked again with the same run, once": also when the
build didn't build? *Fix*: ask again only when no `broken` came and
every step exited 0; else the head says the build failed. Fenix's plan.

### Where the two fit

Checked and fitting, one line each:

- Per-step `cwd` and `env`: the API's `command` has both
  (`world.wit:233-248`); pytest's rootdir, Web's package folders,
  `PYTHONIOENCODING`, nextest's switch.
- A reporter's path: `${data}` is filled in a step's args (Fenix §6.3,
  `host.rs:1165-1179`), and `needs` covers remote.
- Discovery for each runner as a plan: Rust's `--list` after a build,
  pytest's plugin with `--collect-only`, Go's `go list` beside the scan,
  Jest's and Vitest's `-t` that matches nothing.
- Tests that exist only once run (Go's subtests, `test.each` cases,
  Rust's macro-made and `rstest` cases): a `found` before the `ended`
  (Fenix §4, rule 2).
- Readers on batches with state kept between calls: one per run and
  step, every step ended exactly once (Fenix §6.4).
- What a result carries: duration, message, `at`, the whole text,
  captured output, a skip's reason, a build failure apart from a test's
  (`broken`), a fixture's failure (`errored`).
- Lines that aren't UTF-8 and lines past 64 KiB: lossy and `cut`
  (Fenix §6.4); lang-kit joins the pieces.
- stdout and stderr: each stream's order is kept; Rust pairs by order
  within each (§6.1), the rest read one stream.
- Windows' `.cmd` shims: none run (Web runs `node` with the script).
- Sizes: 10,000 found a scan, 10,000 reports a call and 512 lines a
  batch are far from what any reader here makes per call; lang-kit's
  memory of 100,000 tests is about 15 MB of the 128 MiB.
- Debugging: `launch` with a `launch-config` for Python, Go, Vitest and
  Jest; `build-first` for Rust.
- Permissions: nothing to add in any of the four manifests (Fenix §5's claim
  checked against each `[permissions]`).
- API number 1.6, the five export names, `test-provider-set`, the
  records' fields, the settings' names: the same in both documents.

### Fixed in this plan

- Rust's ids carry the package (§4.4): `lib tests::adds` repeats in a
  workspace.
- Rust's run of all is one step for the workspace, its binaries told
  apart by pairing stdout's segments with stderr's headers (§6.1), not
  by their order between streams.
- lang-kit remembers scans and listings to fill a listing's gaps, give
  a group's tests, place a case or a macro-made test, and scope a crash
  (§4.3).
- One `ended` per test: pytest's three phases and libtest's late failure
  text held until whole (§6.1, §6.2).
- Cut lines joined; nothing parsed whole at a step's end: Vitest lists
  through its reporter (*rerun*), nextest streams JSON (D-R1).
- Long selections split under 24,000 characters; pytest takes a file
  of ids (*rerun*).
- pytest's boot takes its folder as an argument (*rerun*), so a `'` in
  the path is harmless.
- Go uses `-fullpath` (*rerun*): a failure's file was the name alone,
  in a package's folder.
- Vitest runs by name, not by the scan's line.
- Repeated Jest and Vitest names, and ids over 1 KiB (§4.4).
- `test-provider-set` called only on a change (§4.3).
- A step with no reader (the pack started again mid-run) gets one.
- Frames read from their end, so a drive letter's colon isn't a line.

## Appendix: what the runners printed

Kept, with the sample projects and the three reporters, under the
scratchpad of the session that wrote this:
`/tmp/claude-0/-home-user/61419de0-c6d3-5f12-9814-ecf7298003a1/scratchpad/m4/`
(`README.txt`, `samples/`, `reporters/`, `outputs/<runner>/`,
`outputs/timings.txt`, and this revision's reruns). That folder isn't
kept: E1a and each pack's piece copy what they need into the repository
as they start (§4.5, "Where fixtures live"), and record again from the
samples what's gone. Trimmed here.

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

**Vitest, listing through the reporter** (*rerun*,
`outputs/vitest/discover-reporter.stdout`, `-t FENIX_NOTHING`):

```text
FENIX-TEST {"kind":"test","file":".../src/orbit.test.js","names":["to do"],"line":14,"column":6,"mode":"skip"}
FENIX-TEST {"kind":"test","file":".../src/orbit.test.js","names":["add(-1, -1) is -3"],"line":19,"column":3,"mode":"skip"}
FENIX-TEST {"kind":"result","file":".../src/orbit.test.js","names":["adds"],"line":4,"state":"skipped","duration":null,"errors":[]}
```

**go test, `-fullpath` and the cache** (*rerun*, `outputs/go/fullpath-and-cache.txt`):

```text
{"Action":"output","Package":"example.com/orbit","Test":"TestTable/negative_one","Output":"    /…/go/orbit_test.go:37: got -2, want -3\n"}
{"Action":"pass","Package":"example.com/orbit","Test":"TestAdds","Elapsed":0}
{"Action":"output","Package":"example.com/orbit","Output":"ok  \texample.com/orbit\t(cached)\n"}
```

## As built: E1a

Built on 13 November 2026 on `main`'s API 1.5: `crates/lang-kit/src/tests/`
(`mod.rs` the trait and `Kit`'s five methods, `api.rs` the copy of Fenix
§6.6 and §6.3 with §18's four changes, `adapter.rs`, `memory.rs`,
`line.rs`), `Host::test_provider_set` (nothing on 1.5), and the test
kit's driver, `fenix_test_kit::tests::Fenix` (feature `lang-kit`). The
made-up runner `tally` and recorded `go test -json` output go through
both in `crates/test-kit/tests/`. Where it differs from §4:

| §4 said | As built | Why |
|---|---|---|
| `scan`, `plan`, `reader` take `&self` | `&mut self` for all but `provider` and `assets`; `assets` gives a `Vec` | A pack keeps what it reads once (`cargo metadata`) without a `RefCell`. |
| `Found` as sketched | Gains `data` | F6, as ruled. |
| `Asked::Group { groups, tests: Vec<Key> }` from lang-kit's memory | `{ groups, tests: Vec<Found>, known: Vec<Key> }`: `tests` what Fenix sent after a file or suite, `known` the memory's only when Fenix sent none | F2 as ruled: Fenix sends them, never under a group, and none past 10,000, when the runner's own filter is right; the memory's may be fewer than there are. |
| `Ask { Discover, Run, RunAll }` | Also `Ask::Debug(Found)` | A build-first plan's reader is told it's reading a build. |
| `plan` returns steps | Steps; `list_after(ask)` beside it | F7 (`Plan.list_after`). |
| `Step { command, label, covers }` | Also `needs` (more under `/data`) and `files` (written for the step under `runs/<run>-<step>/`, `${step_files}` in its args, deleted at its end) | Pytest's file of ids, with nothing but the step to name it. |
| `Report::Ended { .. }` inline | `Report::Ended(Ended)`, with a builder | Readers build results in pieces (held until whole). |
| A crash: lang-kit errors the group's known tests itself and sends `crashed` at the step's end | `Crashed { under }` goes at once as `crashed { id }` | F4 as ruled: Fenix scopes it. |
| Cases placed for a `Found` | Also for an `Ended` or `Started` whose key nothing found: a `found` (kind case, labelled by what the key adds) goes first | Go's subtests and pytest's cases come as results; without it Fenix adds them as tests under the file (§4, rule 3). |
| One `ended` per test: merged in a step | Merged per run; and a template whose cases ended in a step, with no result of its own, gets theirs | Fenix judges a covered test without an `ended` as errored (Fenix §8.5), which a parametrize or `test.each` template never gets from its runner. |
| `split(command, items, budget)`, 24,000 | `split(items, limit, make)`; `DIRECT` = 32,766 − 1,024 and `BATCH_FILE` = 8,191 − 1,024, the limit the pack's per program | Fenix §6.5's numbers; the 1,024 is for `${data}` filled in and the program's full path, which Fenix's plan doesn't say it counts. `make` lets a step's names go in one argument (Go's `-run` regex). |
| More than 256 steps: lang-kit runs the groups instead | An error from `tests_plan` | Only the pack knows its groups' commands. |
| The runner told when it differs from the manifest's `name` | Told as the pack starts, then on each change | Fenix ignores a call that says what it has (F8 as ruled), and lang-kit doesn't read the manifest. |
| Ids over 1 KiB: the pack cuts with a SHA-1 | lang-kit cuts every id at 1,024 bytes, as Fenix does (`cut_id`); a pack that wants a hash makes its key with one | Fenix cuts the same way everywhere (§4), so a cut key still matches; `data` names such a case to its runner. |
| Fixtures in `crates/lang-kit/src/tests/samples/` | `crates/test-kit/tests/samples/go/` | The recorded run is driven through the driver, which lives in the test kit. |
| The driver times calls at 5 ms | 5 ms in a release build, 50 ms in a debug one | Unoptimised code is about ten times slower; wasm's own cost is still the nightly's. |

For E1b: `Fenix`'s `Host::test_provider_set` calls `tests::api::provider`,
so once that's the real import every lang-kit pack's component imports
`test-provider-set` (the vtable keeps it), runner or not. If a 1.6-built
pack with `api = "1.5"` must load on Fenix 1.5 (F15, D-A2), make the call
reachable only through `Kit::tests` (a function pointer it sets), not
through `Fenix`'s impl.

What Fenix's plan leaves unspecified, each read strictly by the driver and
listed in `crates/test-kit/README.md` ("Not yet specified by Fenix"): how
an id over 1 KiB is cut; what the 32,766 and 8,191 count (`${data}`
filled in, the program's full path); whether a cut line's pieces can have
the other stream's lines between them; a line of exactly 64 KiB at a
stream's end; a `crashed` or `broken` with no id in a step whose `covers`
is empty; whether `broken` errors tests that already ended; a template
that never gets its own `ended`; whether a plan goes on after a step
Fenix failed, and what a plan of no steps does to its ask; `found` in a build-first plan and `broken` in a listing; a
`found` before its parent and an id twice in one scan; a group's state
with failed and errored children; which of a file's tests an ask leaves
out past 10,000; whether `test-provider-set`'s runner survives the pack
starting again; and a case dropped by a complete run although Fenix had
just errored it.
