# Test providers: the recipe

> Written 13 November 2026, after E2 (Rust) was built, for whoever
> builds Python (E3a), Go (E3b) or Web (E3c) next with the same kit. Read
> `plans/test-explorer.md` first (its §4, your pack's §6 brief, §7, §8
> and the "As built" sections); this says how, with Rust's code as the
> worked example: `extensions/rust/src/provider/`. (The plan calls this
> file `test-explorer-recipe.md`; it's this one.) Nothing here was run in
> Fenix: there's no Fenix binary in these sessions, and API 1.6 isn't
> released. What only a nightly can confirm is at the end.

## 1. The order of work

| # | Do | Rust's, to copy from |
|---|---|---|
| 1 | Record your runner's output for the plan's sample (§6's fixtures list), into `src/samples/tests/<runner>/` (§9). Look at it before writing a reader: every surprise below was found this way. | `src/samples/tests/cargo/`, `.../nextest/` |
| 2 | Turn the features on (§3), add `src/provider/mod.rs` with a `TestRunner` that answers `runner()` and an empty `scan`, wired with `Kit::tests` | `Cargo.toml`, `lib.rs` (`share`, `started`) |
| 3 | Keys and groups (§5), the scan, its unit tests | `layout.rs`, `scan.rs` |
| 4 | Plans (§6), tested by command line | `plan.rs` |
| 5 | Readers (§7), on the recordings through the driver | `cargo.rs`, `nextest.rs`, `read.rs` |
| 6 | Launch (§8) | `mod.rs` `launch` |
| 7 | The driver's tests for the checklist (§10), snapshots | `drive.rs`, `tests/snapshots/` |
| 8 | README section (what's true on `main`: built, not offered), §12's list for the branch; commit in stages | `README.md` "Tests, for Fenix 1.6" |
| 9 | All the checks (§13); `cargo xtask pack` before and after, `wasm-tools component wit` both: the imports must be the same | |

## 2. File layout

```text
extensions/<pack>/
  src/provider/mod.rs      the TestRunner (`Provider`), its shared facts, runner(), launch()
  src/provider/layout.rs   what a file is (package, target, module; Python's rootdir, Go's import path)
  src/provider/scan.rs     the scanner: text -> tests, nothing else
  src/provider/index.rs    the provider's own memory of its scans (for readers)
  src/provider/plan.rs     asks -> steps
  src/provider/read.rs     what the readers share
  src/provider/<runner>.rs one reader per output format
  src/provider/drive.rs    #[cfg(test)]: the driver's tests
  src/tests/<reporter>     a reporter's source, include_bytes!'d (the plan's place for them)
  src/samples/tests/<runner>/  fixtures (src isn't packed)
  tests/snapshots/*.txt    assert_snapshot!'s files (tests/ isn't packed)
```

**`provider`, not `tests`**: every pack's `lib.rs` has `#[cfg(test)] mod
tests`, and other modules name `crate::tests::line`; a `mod tests` of
the provider would clash. On the `api-1.6` branch the API's module is
`fenix_extension_api::tests` too, so say `api::tests::Found` there.

## 3. Wiring, on `main`

```toml
[dependencies]
fenix-lang-kit = { path = "../../crates/lang-kit", features = ["tests"] }
[dev-dependencies]
fenix-test-kit = { path = "../../crates/test-kit", default-features = false, features = ["tests"] }
```

The first changes no lock line; the second adds `fenix-test-kit` (and
nothing else for Rust: `unicode-width` was in already) to the pack's
`Cargo.lock`. The API lines stay `v1.5.0`. `cargo metadata --locked`
passes after `cargo build` writes the lock once.

The runner lives inside the `Kit` (`Box<dyn TestRunner>`), but what it
needs -- the tool's path, the project's packages -- is the pack's. Share
it the way `packages::Cargo.latest` is shared, an `Rc<RefCell<..>>`:

```rust
// lib.rs, Rust::new
let tests = Rc::new(RefCell::new(provider::Facts { root: root.into(), ..provider::Facts::default() }));
let kit = Kit::with_host("rust", host).packages(Box::new(cargo)).tests(Box::new(provider::Provider::new(tests.clone())));

// at the end of configure(), which every change goes through
fn share(&mut self) {
    let mut f = self.tests.borrow_mut();
    if f.metadata != self.metadata { f.generation += 1; f.metadata = self.metadata.clone(); }
    f.cargo = self.tools.cargo.clone();
    drop(f);
    if self.started { self.kit.tests_runner_changed(); }   // not before activate
}
```

- `started` is set after `self.kit.activate()`: `Kit::activate` tells
  Fenix the runner once; a `tests_runner_changed()` before it would be
  the first call, ahead of the tasks in the host's log (the pack's own
  tests read that log by index).
- `tests_runner_changed()` is cheap: lang-kit calls `runner()` and tells
  Fenix only if the answer changed. Call it whenever a fact changes.
- A version the runner needs (`cargo nextest --version`) is asked with
  the pack's own `Runs` (Rust's `Ask::Nextest`) at activation and when
  the tool appears; `runner()` must not spawn (it has no host). It's a
  background process, not a task: nothing a user sees on `main`.
- A Cargo.toml/package.json the scan reads itself (before the tool has
  answered): `std::fs` under `Facts.root` (`/project` in Fenix, a temp
  folder in tests), cached, dropped when `generation` moves.

## 4. The trait, method by method

| Method | Rust's | The rule |
|---|---|---|
| `provider()` | `"rust"` | the manifest's id, a `const` |
| `runner()` | `("", None)` without Cargo.toml; `("cargo test", Some("cargo isn't installed: ..."))`; `("cargo nextest 0.9.151", None)` when nextest ≥ 0.9.103 | read the shared facts, no I/O. Python: "pytest isn't installed in .venv" etc.; Web: `("", None)` hides it where no runner (F8 as ruled) |
| `scan(path, text)` | line scanner; `[]` for a file no target tests | `path` is `/project/<rel>`. Never panic (§7). 2,000 tests in a few ms (§11) |
| `plan(ask)` | `Err(problem)` when `runner()` has one; discover/run-all/run per §6 | steps in order; `covers` per §6 |
| `list_after(ask)` | `RunAll` only: it built every test target | true only when the listing is then cheap |
| `reader(ask, step)` | by `step.command.args[0]` (`nextest` or not); purpose from the ask (`Discover` listing, `Debug` build) | a fresh reader per step; pass it an `Rc<Ctx>` snapshot, never `&self` |
| `launch(test, kept)` | `kept` empty: `BuildFirst([cargo test --no-run ...])`; else `Now(LaunchConfig)` from `kept`'s `exe <target>` | Python, Go, Web: `Now` at once |
| `is_case`, `assets` | defaults | pytest's plugin and the JS reporters are `assets()` (written to `/data/lang-kit/tests/`, `${data}/lang-kit/tests/<name>` in args) |

`LastGood` is for a scanner that *parses* and fails on half-typed text.
Rust's is a line scanner that always answers; instead it recovers: an
item starting a line at the indentation of an `fn` still open closes
that `fn` (`scan.rs`, `recover`), so typing a new test in the middle of
a file doesn't drop the tests after it. Go's `func TestX(` scanner needs
neither. Use `LastGood` only if you parse (`syn`, a JS parser).

## 5. Keys and groups: so results match scans

The id of a test is its `Key`; a group's id is its labels joined
(lang-kit's `group_id`). Every source -- scan, listing, run, launch --
must build **the same key and the same group labels** for one test.

| Rust | |
|---|---|
| Key | `<package>/<target> <path>`: `orbit/lib tests::adds`, `orbit/test:it integration_fails`, `orbit-cli/bin:orbit-cli tests::sums::case_1`, `orbit/doc src/lib.rs - add (line 6)` |
| Groups | `orbit (lib)` (kind group) › `src/lib.rs` (file, project-relative) › `tests` (suite, its `mod` line) › `nested` |
| One function for each | `TargetKey::{key, label, group, from_label, flags}`, `parse_key` (`layout.rs`); the reader, the scan and the planner all call them |

What made "Fenix doesn't know the id" here, and how it's avoided:

- **A key built two ways.** The listing's `Doc-tests orbit_core` says the
  *crate* (`orbit_core`), the key wants the *package* (`orbit-core`):
  map through the layout (`Layout::doc_target`). nextest says the lib's
  binary as the crate name too (`orbit-core::orbit_core$tests::it_works`).
- **A `Crashed { under }` or `Broken { under }` whose labels aren't the
  scan's.** `under` is the group's labels root first, exactly as the
  scan gave them (`vec![target.label()]`); any other spelling names a
  node Fenix never had, which errors the whole step instead.
- **Paths.** A report's relative path is made `/project/<path>` by
  lang-kit (not "relative to the step's folder" as the WIT allows): give
  project-relative paths. rustc's are the workspace root's; Rust maps
  them (`Layout::place`) for a project opened at a member. Normalise `\`
  to `/` (Windows panics say `src\lib.rs:25:9`). pytest's node ids are
  its rootdir's; Go's `-fullpath` gives full paths: map both.
- **A case no source places.** lang-kit puts a *run's* unknown key under
  a template whose key is its prefix; it does **not** do so for a
  *listing's* `Found` (the adapter's `place` runs only for runs). A
  listing reader sets `of` itself: Rust's `read::found` asks its own
  index for the template (`Index::template_of`). pytest's
  `--collect-only` cases, Vitest's `test.each` in a discover step: same.
- **A test the scan didn't see, found in a run, with no `at`** (a
  passing macro-made test): Fenix would put it at the provider's top.
  Report a `Found` under its module's groups first (`read::found`, from
  the index's module groups). Don't re-send `Found` for scanned keys.
- **Ids over 1 KiB**: lang-kit cuts them as Fenix does. Rust's never
  are; if yours can be (pytest parameters), put the full id in `data`
  and plan from `data` when it's there.

## 6. Plans

- **`covers`**: a test asked by itself → `Covered::Test(key)` on the step
  that runs it. A group asked → `Covered::Group(labels)` on the **last**
  step of its package, so it's judged once all of it ran (a group on an
  early step errors "no result" its tests that later steps run; a later
  `Ended` would fix it, but the page flickers). Run-all: one step,
  `covers` empty (judged at the plan's end).
- **Fill-in**: `Asked::Group { tests: Some(..) }` is everything under a
  file or suite: name them exactly (`-- --exact a b c`), a template by
  its own filter (Rust: the path as a substring, `-- tests::sums`), and
  skip a case whose template is in the list. `tests: None` (a target, or
  no room): the runner's own filter (`--test it`; `-- geo::tests::`).
  The driver always sends a fill-in for a file/suite it has; test `None`
  by calling `Provider::plan` directly with an `Asked::Group` you build
  (`drive.rs`, `a_fill_in_is_named_exactly_...`).
- **`split`**: lang-kit's `split(items, DIRECT, make)` builds and counts
  one whole command *per item* to price it: 2,000 names was 2.2 ms
  natively in release by itself. Give it batches (`plan.rs`,
  `split_batched`: up to 64 items or 2 KiB a batch), which keeps it
  generic and every command under the limit.
- **Per step**: `cwd` (Rust: `/project`; pytest: its rootdir; Web: the
  package's folder) and `env` (nextest's
  `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`; Python's `PYTHONIOENCODING`).
  The program is `Facts.cargo` (the found path) or the bare name: both
  pass `permissions.process` (the driver checks with `manifest(..)`).
- **`Err(why)`** from `plan` errors the whole ask: use it for "the tool
  isn't installed", not for an ask you can't place (skip that item).

## 7. Readers: a state machine a line at a time

```rust
impl Reader for CargoReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        if line.stderr { self.stderr(line.text) } else if line.text.starts_with("{\"reason\":") { self.json(line.text, out) } else { self.stdout(line.text) }
        self.pump(out);   // emit what can be emitted now
    }
    fn end(&mut self, _end: &StepEnd, out: &mut Vec<Report>) { /* flush what's held, Broken at the end */ }
}
```

- **What the kit guarantees**: whole lines (a cut one is joined per
  stream, across calls), ANSI gone, `\r` gone, lossy UTF-8; each
  stream's own order; **not** the order between stdout and stderr.
  Don't redo any of it.
- **One whole `Ended` per test.** Hold a result until all of it has come
  (libtest's `FAILED`, then its `---- name stdout ----` section at the
  binary's end; pytest's setup/call/teardown -- `Ended::and_then` joins
  phases; Go's output lines before its `fail`), then send it once with
  `message` (one line), `at`, `text`, and what it printed as `Output`.
  At a step's end send what's still held, as it is.
- **A retry** (nextest's `#2`, Go's `-count`): each attempt's `Ended` may
  go; the last wins.
- **Never a result for a template** (`#[rstest]`, `parametrize`,
  `test.each`): only its cases'. Only `Ended`s whose key starts with the
  template's at a separator, or that `is_case` claims.
- **`Broken`** (didn't build/load) vs **`Crashed`** (a process died):
  never a failure. Rust sends one `Broken` per target that didn't build
  (`orbit (lib) didn't build: src/lib.rs:19 mismatched types`, Fenix's
  head shows the first) and one with `under: None` for the rest of the
  step, because cargo then runs nothing. A crash is scoped to its
  binary's group so the next binaries' results stand.
- **Two streams that must be paired** (Rust only, so far): never by
  their order between each other. See `cargo.rs`'s module doc: segments
  on stdout, headers on stderr, the k-th with the k-th, a header settled
  once the next one came, a segment's first test name as evidence,
  segments held until their header comes. Tested with `Order::StdoutFirst`,
  `StderrFirst` and several `Shuffled(seed)`, all giving one tree.
- **Never panic** on input: `text.get(a..b)` not `&text[a..b]` where a
  byte index may fall inside a character; parse JSON with
  `serde_json::from_str` and pass over what fails. A trap stops the pack.
- **Cost**: per line, no rescans of what was read; a few hash lookups.
  Allocate per test, not per byte.

## 8. Launch

Rust's is `build-first`: the build is read by the same reader with
`Purpose::Build`, which `Keep`s `exe <package>/<target>` for each test
binary (`compiler-artifact` with `profile.test` true -- not the `[[bin]]`
built for integration tests to run); the second `launch` call picks it.
A build that fails is `Broken`, and Fenix doesn't ask again (the driver's
`launch` returns `Err(<the broken message>)`). The `LaunchConfig`:
`program` the binary, `args [<path>, "--exact", "--nocapture"]`, `cwd`
and `CARGO_MANIFEST_DIR` the package's folder, `attributes` the pack's
own (`debug::attributes(sysroot)`). A doc test: `Err(..)`. Python, Go,
Web: `Launching::Now` straight away (their §6 briefs).

## 9. Recording fixtures

The driver's layout: `<name>.stdout`, `<name>.stderr`, `<name>.exit` (a
code or `signal`), `<name>.order` (`o`/`e` a line, how they came) and
`<name>.args` (the command line, for `on_recorded`, which matches the
step's `program args...` joined by spaces exactly). **No dot in
`<name>`**: `Recorded::from` uses `with_extension`, so `run-1.97` reads
`run-1.stdout`. Paths of the recording machine become `/project` (and
home folders `/home/u`).

The recorder used for Rust (Python 3, stdlib only), run from the
sample's folder as cwd:

```python
# rec.py <cwd> <out-prefix> <program> [args...]
import os, selectors, subprocess, sys
cwd, out, *cmd = sys.argv[1:]
cwd = os.path.realpath(cwd)
p = subprocess.Popen(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
sel = selectors.DefaultSelector()
sel.register(p.stdout, selectors.EVENT_READ, 'o'); sel.register(p.stderr, selectors.EVENT_READ, 'e')
buf, data, order, live = {'o': b'', 'e': b''}, {'o': b'', 'e': b''}, [], 2
while live:
    for key, _ in sel.select():
        s, chunk = key.data, os.read(key.fileobj.fileno(), 65536)
        if not chunk:
            sel.unregister(key.fileobj); live -= 1
            if buf[s]: order.append(s); data[s] += buf[s]; buf[s] = b''
            continue
        buf[s] += chunk
        while b'\n' in buf[s]:
            line, buf[s] = buf[s].split(b'\n', 1); order.append(s); data[s] += line + b'\n'
code = p.wait()
clean = lambda b: b.decode('utf-8', 'replace').replace(cwd, '/project').replace(os.path.expanduser('~'), '/home/u').encode()
open(out + '.stdout', 'wb').write(clean(data['o'])); open(out + '.stderr', 'wb').write(clean(data['e']))
open(out + '.order', 'w').write('\n'.join(order) + '\n')
open(out + '.exit', 'w').write((str(code) if code >= 0 else 'signal') + '\n')
open(out + '.args', 'w').write(' '.join(['cargo'] + cmd[1:]) + '\n')   # your program's name
```

Record warm (build once first) unless the case is the build; record the
exact command your plan makes (copy it from a failing `on_recorded`:
the driver prints the step it has no recording for). Keep the sample
project's sources beside the fixtures (`src/samples/tests/cargo/orbit/`)
and scan those in the tests, so lines match what the runner printed.
Extend the plan's sample minimally for a case it lacks (Rust added a
`harness = false` test, a crash, a workspace with rstest and test-case).
A tool that isn't installed: `cargo install <it> --locked --root
<scratch>` worked for cargo-nextest and wasm-tools through the proxy.

## 10. The driver

```rust
let fake = Fake::new();
let kit = Kit::with_host("rust", fake.host()).tests(Box::new(Provider::new(facts)));
let mut fenix = Fenix::new(kit);
fenix.manifest("extension.toml").with_fake(fake.clone());      // programs from permissions.process; told() from the fake
fenix.scan("rust", "/project/src/lib.rs", LIB);                 // include_str! of the sample
fenix.on_recorded(Recorded::from("src/samples/tests/cargo/run-all"));
let run = fenix.run_all("rust");
fenix.assert_states("rust", &[("orbit/lib tests::adds", State::Passed)]);
assert_eq!(fenix.at("rust", "orbit/lib tests::fails_on_purpose"), Some(("src/lib.rs".into(), 25, Some(9))));
assert_snapshot!("cargo-run-all", fenix.tree("rust"));
```

| | |
|---|---|
| `on_recorded` / `on_command(pattern)` / `on_step(pred)` / `on_step_with(f)` | the last matching rule answers; `on_step_with` builds output from the step's own args (Rust's 2,000-test re-run) |
| `order`, `batch`, `cut_at` | run the same fixture with `StdoutFirst`, `StderrFirst`, `Shuffled(n)`, `batch = 1`, `cut_at = 23`: compare `tree()`s |
| strict (default) / `lenient()` | strict fails the test on anything Fenix would refuse; keep it strict |
| `forget_data(provider)` | as after Fenix restarts: plan from keys alone |
| `run(ids)`, `run_file`, `run_failed`, `discover`, `launch(id)` | Fenix's asks as `fenix-tests` builds them (topmost, fill-ins) |
| `record`, `state`, `detail`, `at`, `told`, `failed`, `tree` | what Fenix holds; lines in `record` are from 0, `at()` from 1 |
| budget | every call is timed: 50 ms debug, 5 ms release. A heavy one-off measurement (a 2,000-test scan) is best-of-5 in its own test, so a busy machine doesn't fail it |
| snapshots | `UPDATE_SNAPSHOTS=1 cargo test` writes `tests/snapshots/`; read every one before committing |

Wire the pack's own struct only for what the struct does (Rust's
`the_test_provider_says_cargo_test_then_nextest_...` in `lib.rs`, on
lang-kit's mock); drive the provider with `Facts` you fill in.

## 11. The checklist every provider tests

From Rust's `drive.rs` (one test each, named by what it shows):

1. The scan: tests at their lines, under their groups; skips with their
   reason; templates marked; what isn't a test isn't (half-typed text,
   comments, strings, odd bytes: no panic).
2. A file of 2,000 tests scanned within a call's budget (best of five).
3. The listing: what the scan can't see (macro-made, cases, doc tests),
   placed under the scan's groups; the scan's lines kept.
4. A run of all: every outcome; a failure at its assertion with message,
   text and output; a skip's reason; `assert_snapshot!`.
5. The same fixture in every stream order and batch size: one tree.
6. A build/load failure: `broken` on the right group, no test failed.
7. A crash: its own group's tests errored, the rest's results stand.
8. One test by its line: the exact command, its `covers`, its `cwd`.
9. The failed again (`run_failed`): one step, exact names.
10. A fill-in named exactly; no fill-in by the runner's own filter.
11. `forget_data`: the same plan from keys alone.
12. A template's cases under it; no result made for the template.
13. A workspace/monorepo/several packages in one step, same names in
    two targets kept apart.
14. 2,000 failed re-run: several steps, each `DIRECT.fits`, all covered.
15. Launch: the config's program, args, cwd, env (build-first if yours).
16. The runner told, and told again when a fact changes (the pack's
    struct, on lang-kit's mock or the fake).
17. A stop mid-step; a Windows path by hand.

## 12. Off `main`: what the `api-1.6` branch adds to each pack

On `main` nothing names 1.6: no `[[contributes.test_providers]]` (Fenix
1.6 refuses a provider whose `engine.api` is under 1.6), no engine
change, no version bump, no forwards, no CHANGELOG entry (no release,
nothing visible). The branch adds, per pack, once E1b has moved the
kits:

1. **The manifest section** (Rust's, filled in):

   ```toml
   [engine]
   fenix = ">=1.6, <2"
   # The test provider (tests-scan .. tests-launch, test-provider-set);
   # the keys a hint names (key_for, 1.5).
   api = "1.6"

   [[contributes.test_providers]]
   id = "rust"
   name = "cargo test"
   languages = ["rust"]
   markers = ["Cargo.toml"]
   scan = ["src/**/*.rs", "tests/**/*.rs", "*/src/**/*.rs", "*/tests/**/*.rs", "*/*/src/**/*.rs", "*/*/tests/**/*.rs"]
   ```

   (Fenix's walk now leaves out what `.gitignore` ignores and hidden
   folders, per `WRITING_EXTENSIONS.md`; keep path patterns all the
   same: a project with no `.gitignore` has `target/` full of `.rs`.)
   No `permissions.process` change: `cargo` covers `cargo nextest`.
2. **The version**: `1.3.0` in `extension.toml` and `Cargo.toml`.
3. **The five forwards** in `impl Extension for Rust`, nothing else:

   ```rust
   fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<api::tests::Found> {
       self.kit.tests_scan(provider, path, text)
   }
   fn tests_plan(&mut self, provider: &str, run: u64, ask: api::tests::Ask) -> Result<api::tests::Plan, String> {
       self.kit.tests_plan(provider, run, ask)
   }
   fn tests_read(&mut self, provider: &str, run: u64, step: u32, lines: Vec<api::tests::Line>) -> Vec<api::tests::Report> {
       self.kit.tests_read(provider, run, step, lines)
   }
   fn tests_step_ended(&mut self, provider: &str, run: u64, step: u32, end: api::tests::StepEnd) -> Vec<api::tests::Report> {
       self.kit.tests_step_ended(provider, run, step, end)
   }
   fn tests_launch(&mut self, provider: &str, run: u64, test: api::tests::Found) -> Result<api::tests::Launch, String> {
       self.kit.tests_launch(provider, run, test)
   }
   ```

4. **A doctor row** for the runner's optional tool, if yours has one
   (Rust: `cargo-nextest`, optional, "gives each test's time", fix
   `cargo install --locked cargo-nextest`) -- user-visible, so the
   branch's, not `main`'s.
5. **The CHANGELOG entry**, for Rust:

   > ## 1.3.0
   >
   > - For Fenix 1.6 (extension API 1.6): your tests on Fenix's tests
   >   page (`SPC T T`), in the gutter and after each test's line, run
   >   from any of them and debugged with `SPC T d`. A test is found as
   >   you type (`#[test]`, `#[tokio::test]` and any `#[<crate>::test]`,
   >   `#[ignore]`d ones as skipped, `#[rstest]` and `#[test_case]` cases
   >   once run); what a macro makes and doc tests come from `cargo test
   >   -- --list`, which builds the tests first. Runs are `cargo test
   >   --no-fail-fast`, by exact names, a step per package; with
   >   cargo-nextest 0.9.103 or later installed, `cargo nextest run`,
   >   which gives each test's time (cargo test doesn't). A failure is
   >   shown at its panic, with its message and output; a build that
   >   fails says so and fails no test; a binary that crashes errors its
   >   own tests only. Debugging a test builds its binary, then runs it
   >   under lldb-dap or CodeLLDB with `<name> --exact --nocapture`. On
   >   Fenix 1.5, 1.2.0 stays the one you have.

6. **README**: the "Tests, for Fenix 1.6" section reworded for what's
   offered (keys, the page), and the versions verified (cargo 1.100,
   cargo-nextest 0.9.151).
7. Check: `wasm-tools component wit extension.wasm` lists
   `test-provider-set` among `host`'s imports and the five exports.

## 13. Checks

```text
cargo test --locked --manifest-path extensions/<pack>/Cargo.toml
cargo clippy --locked --manifest-path extensions/<pack>/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --manifest-path extensions/<pack>/Cargo.toml --target wasm32-wasip2 -- -D warnings
cargo xtask pack extensions/<pack> <scratch>        # before and after: the size
wasm-tools component wit <scratch>/extension.wasm   # before and after: the same imports
cargo clippy --locked --all-targets -- -D warnings ; cargo test --locked -p xtask ; cargo xtask validate
```

Rust's: package 450,072 → 519,797 bytes, `extension.wasm` 1,200,519 →
1,411,455; the component's WIT and its 177 core import names identical.

## 14. What the kit did that you should know

| Found | What was done |
|---|---|
| A scan made each test's group ids again (memory and answer): most of the adapter's time for 2,000 tests | **Fixed** in lang-kit (consecutive tests sharing groups reuse the chain), with a test (`tests_that_share_their_groups_and_those_that_dont_are_each_placed`) |
| `split` builds and counts a whole command per item to price it | Worked around in the pack (`split_batched`); a kit fix would price appended args without rebuilding the command |
| At 2,000 records an ask's conversion (`ask_from`) and the steps' bookkeeping take ~4 ms natively in release, so a 2,000-test plan is over the release budget (5 ms) whatever the pack does; the kit's own `tally` 2,000 test flakes the same way in release | Not changed (a redesign); debug runs, which CI makes, are well inside 50 ms. Wasm's cost is the nightly's to measure |
| A listing's `Found` isn't placed under a template by lang-kit (only a run's) | The reader sets `of` itself (§5) |
| Relative report paths become `/project/<path>`, not the step's folder's | Give project-relative paths (§5) |
| `Recorded::from` and dots in a fixture's name | No dots (§9) |
| The mock's and the fake's log gains a `test-provider <id>: <runner>` line at activation once the feature is on | The pack's existing tests that read the log by index still passed; check yours |

## 15. Rust's own facts, for comparison

- `cargo test -- --exact` never matches a doc test (cargo 1.100): they
  run by their name as a substring, which no other doc test's contains
  (`src/lib.rs - add (line 6)`, the `)` included).
- `--doc` can't go beside another target flag ("can't mix --doc with
  other target selecting options"): doc tests are a step of their own.
- A binary with no tests still prints `running 0 tests` and `test
  result:`; a `harness = false` one prints what it likes (here, a panic
  on stderr and no segment): the pairing goes past it by evidence.
- `--jobs` changes only the build; cargo runs test binaries one at a time.
- cargo 1.100 puts test binaries at `target/debug/build/<pkg>/<hash>/out/<name>-<hash>`;
  match a header to its binary by file name, not by a folder layout.
- nextest under `-E` reports a binary's *ignored* tests it wasn't asked
  for (`started`, then `ignored`): the reader drops them unless the step
  named them. A retry's name ends `#2`. An abort's `failed` has no
  output; its human line says `SIGABRT`.

## 16. Pitfalls ahead, per runner

**Python (pytest).**
- Load the plugin with `-p fenix_tests` and the boot that takes its
  folder as an argument (§5 of the plan); the plugin is an asset, so its
  `${data}` path puts it in `needs` for you.
- Ids: pytest's node ids are relative to its **rootdir**, the step's
  `cwd`; turn them into project-relative keys (§5), the same in scan,
  collect and run. Parametrised ids (`test_cases[strings [odd] id]`)
  hold brackets and spaces: no shell, so no quoting, but a key over 1 KiB
  needs `data`. A case's template is the id without `[...]`: set `of` in
  the collect reader (§5, listings aren't placed by the kit).
- More than a few ids: `@${step_files}/ids.txt` (a `Step::file`), not
  `split` -- pytest takes the file.
- Phases: hold `setup`/`call`/`teardown` until the next test's first or
  the step's end; a setup failure is `errored`, not `failed`.
- `collect-error` is `Broken { under: None }`: pytest ran nothing. Exit 5
  (nothing collected) is a normal end.

**Go (`go test -json`).**
- One stream (stdout), no pairing. Packages interleave: key every event
  by `Package`. Subtests appear only once run: their `run` event makes a
  `Found` (kind case, `of` the parent) before their result; a name's
  spaces became `_` and `/` is a level.
- Build failures from Go 1.24: `build-output`/`build-fail` and the
  package's `fail` with `FailedBuild` → `Broken { under: the package }`;
  before 1.24 the text is on stderr (not verified): a `fail` with no test
  events.
- `-run` is a regex per `/` level: build it with `regexp.QuoteMeta`'s
  rules, and give `split` a `make` that joins names into one argument.
- A cached package replays its events with `Elapsed` 0: fine.

**Web (Vitest, Jest).**
- Run `node node_modules/vitest/vitest.mjs` / `.../jest/bin/jest.js`,
  never `npx` or a `.bin` shim (a `.cmd` on Windows: `BATCH_FILE`'s
  8,191). With `node`, `DIRECT` is the limit.
- Listing: Vitest through the reporter with `-t FENIX_NOTHING` (every
  test, `skip`/`todo` and `test.each` cases, exit 0); Jest the same trick
  with `--testLocationInResults`. In that step a `result` line is no
  result: send nothing for it.
- `-t` is a regex on names joined by ` > ` (Vitest) or spaces (Jest):
  escape regex characters (`add\(-1, -1\)`), anchor it.
- Two providers' worth in one: `runner()` says which; a monorepo is a
  step per package folder (its `cwd`), `covers` the package's group on
  its last step.
- Frames: read `at` from the stack's end (`:9:21)`) so a drive letter's
  colon stays; take the first frame in the project, `file://` off.

## 17. What Python (E3a) found the recipe lacked

(`plans/test-explorer.md`, "As built: E3a" has the whole table.)

- **§16's "its `${data}` path puts it in `needs` for you" holds only for
  a path that names the asset.** The boot takes the plugin's *folder*, so
  the step names `/data/lang-kit/tests/fenix_tests.py` in `needs` itself
  (`Step::needs`); the driver fails the step otherwise.
- **A runner whose process can die with nothing said needs a marker of
  its own end.** pytest's last line is gone under `-qq` (a project's
  `addopts = -q` makes our `-q` one) and decorated under `-v`; exit codes
  are ambiguous (`os._exit(1)` is "tests failed"). The plugin says
  `done` with the exit status; no `done` is a crash, and a `start` line
  per test names the one running. Add the same to any reporter you ship.
- **A listing run is also a run's first lines**: pytest's plugin prints
  each `test` in a run too (not under xdist, whose workers' stdout isn't
  forwarded: a test is found from its first `start` or `result` then).
  Send a `Found` only for what no scan has (`read::found`), and keep what
  you listed so a run doesn't send it again (`Index::listed`).
- **Pin the rootdir** (`--rootdir=.`) when node ids are your keys: pytest
  walks up for configuration, outside the project too, and a sample
  project with no `pyproject.toml` of its own found the pack's.
- **Key by the project, step by the root.** A monorepo's two
  `tests/test_x.py` are two tests; the key is below the project, the
  step's `cwd` and its node ids below the rootdir, the reader's `Ctx` has
  the prefix.
- **An id file replaces `split`**: `Step::file("ids.txt", ..)` with
  `@${step_files}/ids.txt` in the args; the driver's `on_step_with` can
  read the file back through `fenix.data_dir()` to answer it (the run
  number is in its path, so `on_recorded` can't match such a step).
- **A heavy fixture belongs in many small scans**: one driver-timed call
  that scans 2,000 tests fails the budget on a loaded machine; twenty
  files of 100 don't, and the test for the clock stays alone, best of
  several.
- **A step that printed nothing never gets `Reader::end`** (a kit finding
  in E3a's "As built"): a runner that can die before its first line is
  reported by nothing; the tests are errored "the run ended without a
  result for it".
- **Ids and `Found` details that bit**: a case's id holds `::` and
  brackets (`test_url[http://x::y]`): split the test at the first `[`;
  a template is a `Found` with `template` set and its cases `Found::case`
  with `of`; `Found.data` is for a key over 1 KiB only, and not past 4
  KiB (the driver refuses it); cases take their template's line.
- **Run a real runner once through the driver**: an `#[ignore]`d test
  whose `on_step_with` runs each step's command for real
  (`against_real_pytest`) found what recordings couldn't (xdist's
  collection errors don't stop a run; a dead worker is a test's result,
  not the process's end).

## 18. Notes from E3c (Web)

What §16 and the rest didn't say, found building Vitest's and Jest's
(`extensions/web/src/provider/`; `plans/test-explorer.md`, "As built:
E3c"):

- **A run by name reports what it left out.** Vitest and Jest report
  every collected test of the files given, those `-t` didn't match as
  `skipped` (Vitest's `mode` `skip`, Jest's `pending`): a reader that
  sends them errs every run of one test. The reader knows the step's
  selection from its `covers` (exact keys, a describe's key prefix, a
  template's title) and sends a skip only for what the step asked.
- **Jest's `--reporters` takes every argument after it**: files must come
  after `--`. Its path patterns are regexes on the full path; escape
  `.` but not `/` (on Windows Jest turns `/` into `\\`, and `\/` breaks).
- **A reporter's paths need its folder**: the reporters now print `cwd`
  on every line; a path is made the project's by taking off that folder
  less the package's own (`/home/ann/orbit/packages/app` less
  `packages/app`), with no knowledge of where the project is.
- **A worker's death** is nowhere but Vitest's `onTestRunEnd` unhandled
  errors (`cause`: `Worker exited unexpectedly with signal SIGKILL …
  while running test file <path>`; the file's tests get no result at
  all) and Jest's `testExecError` (`A jest worker process … was
  terminated`; the file's tests that ended keep theirs). Jest runs one
  quick file, or files whose cached timings are fast, in its own
  process: killed, the step ends by a signal. Record Jest's crash with
  `TMPDIR` set to a fresh folder (its cache is under it) to get workers.
- **`split`'s commands don't say which items they hold.** Rust counts
  args; a `make` that joins items into one regex can't. Record each
  `make`'s slice (its offset in the items and length) with a hash of the
  command it gave, and match the commands `split` returns to those
  (`plan.rs`, `named`); making each prefix again to compare is quadratic
  (40 ms for 2,000). A batch must be well under the *smallest* limit you
  split for: `split` never cuts one item, so an 8 KiB batch under
  `BATCH_FILE` would go out over 8,191.
- **Per-test lookups must be O(1)**: an index whose "is this key a
  template" was a linear `find` over the file's tests made a 2,000-test
  plan 130 ms. Debug builds magnify everything ~5×; the driver's 50 ms
  is per call, and a 2,000-test scan of one file was ~23 ms in debug
  alone and over 50 ms while the rest of the suite ran beside it. The
  timed tests take a lock so they don't time each other, and the
  2,000-rerun test scans four files of 500 (one call each), as Rust's
  does.
- **Recording**: a removal of a relative glob after `cd` is refused by
  the session's safety check; give absolute paths.

## 19. Added by Go (E3b), 13 November 2026

What §16 got wrong or left out for Go, checked on Go 1.24.7 (1.23.8 and
1.20.14 by `GOTOOLCHAIN`; `go` downloads a toolchain through the proxy):

- **A cached package replays the elapsed times of the run it replays**
  (`TestSlow` 2,000 ms again), not 0: 0 is a quick test's.
- **`go test ./...` at a `go.work` whose top isn't a module fails**
  (`directory prefix . does not contain modules listed in go.work`, as
  `FailedBuild` of a package named `./...`): name the `use` folders
  (`./app/... ./lib/...`). Such a `Broken` has no group to name: `under:
  None`, or the driver fails the step. Modules with no `go.work` need a
  step each, in their own folder.
- **A test's `skip` is not a result.** A run of all errors a test the run
  gave no result for, `skip` or not; a test Go doesn't build (a build tag)
  needs an `Ended { Skipped }` from the reader once its package ended.
- **A fill-in drops a case under a case** (kit defect, fixed in lang-kit
  `adapter.rs` with a test): Go's nested `t.Run`. Check a nested case in a
  file's fill-in with the driver (`ask_for`).
- **`os.Exit` in a test leaves no line**: the test's `run`, then the
  package's `fail`, nothing about the exit under `-json`. A timeout's panic
  is attributed to the test it caught. A panic in a test ends the binary:
  its `fail`, then the package's, the later tests never run.
- **Pre-1.24 build errors and stdout have no order between them:** hold the
  package until `end`, then read stderr's `# pkg [pkg.test]` block.
- **`-fullpath` is Go 1.21's.** Go 1.20's binary says `flag provided but not
  defined: -fullpath` as package output: say it in `runner()`.
- **`go list -f` with a Go slice (`{{.TestGoFiles}}`) breaks on a space in a
  path**: `join` with a separator and tabs between fields. `-e` lists a
  package that doesn't load.
- **A listing reader may read files** (`std::fs` under `/project`): to put
  tests the scan missed in the tree, give it a queue (a few a call, the
  rest at `end`) and a cap.
- **A test's lines are unbounded**: keep its first and last few hundred.
- **`split` again was too slow** for 2,000 names (quoting each name for the
  regex 14 times, in debug 70 ms): quote once, measure by batches.
- **An Example with no `// Output:` isn't run**, and `go test -list` doesn't
  list it; vet (run by `go test` since 1.24) refuses an example of a name
  that isn't in the package (`ExampleNoOutput`), so record with
  `ExampleAdd_silent`.
