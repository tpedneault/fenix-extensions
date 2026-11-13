# fenix-lang-kit

What every language pack's code extension shares, through `Host`, so
each pack says only what its language has: the package manager page, the
project doctor's checks, tasks, a server or a debug adapter installed
into the tools folder, a formatter and a linter; and, for Fenix 1.6, a
test provider ([Tests](#tests)).

```toml
[dependencies]
fenix-extension-api = { git = "https://github.com/tpedneault/fenix", rev = "a39d18b273ace00091d8d170382456bbad0a7c2f" }
fenix-lang-kit = { path = "../../crates/lang-kit" }
```

A pack implements `PackageManager` for its manager -- a command for
each of list, outdated, audit, upgrade, add and remove, and a parser for
what the first three print -- and hands its extension's calls to a `Kit`:

```rust,no_run
use fenix_extension_api::{export, Command, Event, Extension, Key, MouseEvent, Page, Severity};
use fenix_lang_kit::{command, compiler_lines, Dependency, Fix, Install, Kit, Linter, Output, PackageManager, Program, Tasks};

struct Uv;
impl PackageManager for Uv {
    fn name(&self) -> &str { "uv" }
    fn list(&self) -> Command { command("uv pip list --format json") }
    fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
        let rows: Vec<fenix_lang_kit::serde_json::Value> = out.json()?;
        Ok(rows.iter().map(|r| Dependency::new(r["name"].as_str().unwrap_or(""), r["version"].as_str().unwrap_or(""))).collect())
    }
    fn upgrade(&self, d: &Dependency) -> Option<Command> { Some(command(&format!("uv add {}@latest", d.name))) }
    fn upgrade_all(&self, _: &[&Dependency]) -> Option<Command> { Some(command("uv lock --upgrade")) }
    fn add(&self, package: &str) -> Command { command(&format!("uv add {package}")) }
    fn remove(&self, d: &Dependency) -> Option<Command> { d.direct.then(|| command(&format!("uv remove {}", d.name))) }
}

struct Python(Kit);
impl Default for Python {
    fn default() -> Python {
        Python(Kit::new("python")
            .packages(Box::new(Uv))
            .program(Program::new("python3", "Python").at_least("3.9"))
            .program(Program::new("uv", "uv").fix("Install uv", "python.install-uv", Fix::Task(command("pipx install uv"))))
            .tasks(Tasks::table(&[("test", "uv run pytest"), ("lint", "uv run ruff check .")]).env_file("/project/.env"))
            .install(Install::new("ruff", "https://github.com/astral-sh/ruff/releases/download/0.7.0/ruff-x86_64-unknown-linux-gnu.tar.gz").strip(1).language_server(&["python"], ["server"]))
            .formatter(&["python"], command("ruff format --stdin-filename ${file} -"))
            .linter(Linter::new("ruff", &["py"], |f| command(&format!("ruff check --output-format concise {f}")), |o, f| Ok(compiler_lines(&o.stdout, f, Severity::Warning)))))
    }
}
impl Extension for Python {
    fn activate(&mut self) -> Result<(), String> { self.0.activate() }
    fn run_command(&mut self, id: &str) -> Result<(), String> { self.0.run_command(id).unwrap_or_else(|| Err(format!("no command {id}"))) }
    fn handle(&mut self, event: Event) { self.0.handle(&event); }
    fn page(&mut self, page: u64, width: u32, height: u32) -> Page { self.0.page(page, width, height).unwrap_or_default() }
    fn page_key(&mut self, page: u64, key: Key) { self.0.page_key(page, key); }
    fn page_mouse(&mut self, page: u64, event: MouseEvent) { self.0.page_mouse(page, &event); }
    fn page_state(&mut self, page: u64) -> Option<String> { self.0.page_state(page) }
    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool { self.0.page_restore(page, kind, state) }
    fn page_closed(&mut self, page: u64) { self.0.page_closed(page) }
}
export!(Python);
```

Its manifest lists the kit's commands -- `python.packages`, `python.doctor`,
`python.install.ruff` and each fix row's (`python.install-uv`) -- the
programs it runs (`uv`, `python3`, `/tools/*` for what it installs),
the hosts it downloads from, `filesystem = "project-read"`, `editor =
"read"` and `events = ["saved"]` for the linter, and `api = "1.5"`: the kit is built
on extension API 1.6 (Fenix's `a39d18b`, by `rev`, until Fenix 1.6.0 is
tagged), so a pack using it builds its API from that source too; what it
declares is what it uses -- a pack that runs no tests needs only Fenix 1.5
(`fenix = ">=1.5, <2"`, `api = "1.5"`), one with the feature `tests`
needs 1.6 (`fenix = ">=1.6, <2"`, `api = "1.6"`).

| | |
|---|---|
| `PackageManager` | `list`, `outdated`, `audit` and their parsers; `upgrade`, `upgrade_all`, `add`, `remove` as commands run as tasks; optionally `search` (for `a`), `normalize` (Python's names) and `lockfiles` (the cache's key). Each command runs with its stdin closed; a parser gets the exit code too, since `npm outdated` and most audits exit 1 when they find something. The page runs `*_steps`: by default the one command, else several in turn (`go get`, then `go mod tidy`; a `dotnet add package` each), stopping at one that fails, or a reason it can't (`Err`), said on the page. |
| `PackagesPage` | The page: name, current, wanted, latest, direct or transitive (or its group), the advisory's id and severity; tabs for all, outdated and vulnerable; the vulnerable first, then the outdated (a major first). `u` upgrades one, `U` all (asked first), `a` adds (a search's picker, or what's typed), `d` removes (asked first), `r` lists again, Enter shows a dependency's advisories beside. The vulnerable rows are on red, the outdated on amber; a click picks a tab or a row, a double click shows its details, the wheel scrolls. Each action is a task, one at a time, and it all refreshes after. The last list shows at once from a cache in `/data` while the lockfiles hold what they did; an audit is kept an hour unless something changed. A step that failed says why on the page. |
| `Program`, `Doctor` | A program found with `find-program`, asked its version (5 s, or its `timeout`, with its `env` and `cwd`), held to a `Requirement` (`>=3.9`, `^1.22`, `>=18 <21 \|\| 22`); a fix row that runs a task, an install, or a command of the pack's. The checks are made again after a fix. A program can be checked twice under two labels (`uv`, and the Python uv finds), and `judge` -- a closure, which may hold what it expects -- reads what it said in place of a version (`uv sync --locked --dry-run`: in step or not). `Kit::extra_checks` works the pack's own checks out again each time. |
| `Version`, `Requirement`, `find_version` | Versions as tools print them (`go1.22.3`, `3.12.0rc1`, `1.0-SNAPSHOT`, pseudo-versions), compared; ranges npm, Cargo and pip write; the version in `--version`'s output. |
| `Tasks` | A table of name and command line, a `cwd`, an env file's values (`parse_env`), set with `tasks-set`. |
| `Install`, `Installer` | A release downloaded into `/tools/<name>` and checked by Fenix, unpacked a step a timer (`.zip`/`.vsix`, `.tar.gz`, `.tar.xz`, `.tar`, `.gz`, a plain file) with paths kept in the folder, made to run, marked with its URL, then set as the language server, a server beside the others (`added_language_server`), or the debug adapter, on stdio or over a socket (`debug_adapter_connect`). Used again when the pack starts. Starting one is said in the message line; how it ended is a notification (API 1.3), since a download can take minutes while you work elsewhere: tagged `install.<name>`, so the newest for a tool takes the older's place; low urgency when it worked ("Installed ZLS", what it's now used as), normal when it didn't, saying which step failed and why, with *Try again* (`<prefix>.install.<name>`). A failure's toast is taken away when that install starts again. |
| `Linter`, `Lint`, `compiler_lines` | Run on `saved` in `/project` (or `Kit::file_saved`'s path), its output read into `Problem`s (`path:line:col: severity: message`, MSBuild's, or the pack's own parser) and set with `diagnostics-set`. `Linter::with_fixes` gives each problem its fixes (`diagnostics-set-ex`, `SPC c a`); `Linter::across` speaks of several files (clippy, a package's vet), each set and those it no longer speaks of made clean. |
| `Risk`, `cvss` | An advisory's severity as tools say it: a word, a score, or a CVSS 2 or 3 vector. |
| `formatter` | `project::formatter`: Fenix runs it on `SPC c F` and on save. |
| `tests` | A test provider for Fenix 1.6, with the feature `tests`: below. |
| `Host` | Every host call; `Fenix` makes them. The later ones -- HTTP, `platform`, `open_url`, a server added or given settings, an adapter over a socket, problems with fixes, background tasks, launches and their variables, a notification and its dismissal, a test provider's runner (`test_provider_set`, with the feature `tests`; nothing on API 1.5) -- have defaults that say the host can't (a notification is said in the message line), so a host of a pack's own needn't have them. |

## Tests

For Fenix 1.6 (extension API 1.6), a pack can be a **test provider**:
Fenix's tests page, gutter and `SPC T` run its tests and show their
results, and the pack says only what its runner knows -- how to find
tests in a file's text, what commands list and run them, how to read
what they print, how to debug one. The design is
[`plans/test-explorer.md`](../../plans/test-explorer.md) and Fenix's
`docs/plans/test-explorer.md`, whose §19 settled the rules this follows.
`tests::api` is API 1.6's own `fenix_extension_api::tests`, re-exported
(Fenix's `a39d18b`, by `rev`, until 1.6.0 is tagged: `plans/test-explorer.md`,
§8.2); a test there builds every type by every field and matches every
case, so a change in Fenix's API fails to compile at the pin that moved.
`test_exports!(kit)` writes the five forwards of an `impl Extension`.

### Opting in: the feature `tests`

It's all behind lang-kit's feature `tests`. A pack that runs tests
turns it on; no other pack does:

```toml
[dependencies]
fenix-lang-kit = { path = "../../crates/lang-kit", features = ["tests"] }
```

Without it the `tests` module, `Kit::tests`, `Kit::tests_scan` and the
rest, `Host::test_provider_set` and the one call of the API's
`tests::provider` are not compiled at all: `Kit::activate`, `handle` and
`file_saved` reach nothing of them, and the pack's WebAssembly is what
it was before the module existed (Zig's and Lua's are the same size as
before it, their bytes differing only in lang-kit's source paths and
line numbers). That matters because Fenix 1.5 refuses a component that
imports `test-provider-set`, and the linker keeps a call reachable from
an export even behind a condition that never holds (Fenix §19, finding
4): so a pack without the feature can keep `api = "1.5"` once rebuilt on
the 1.6 crate, and one with it needs `api = "1.6"`. Each extension is
built on its own, so one pack's features never reach another's. Its
tests turn it on through the test kit's `tests` feature (which turns on
this one), or `features = ["tests", "mock"]` here.

The five forwards in a pack's `impl Extension` (below), `test_exports!`
and `api.rs`'s `pub use fenix_extension_api::tests::*` are behind the
feature too: a pack without it writes no forwards, the API's defaults
answer for it, and nothing it links calls `tests::provider`. What every
extension built on the 1.6 crate has, feature or not, is the five
`tests-*` exports with those defaults, which Fenix's `export!` writes:
about 28 to 30 KB more in `extension.wasm`, and no import.

### What a pack writes

A pack implements `tests::TestRunner` for each
`[[contributes.test_providers]]` of its manifest and gives it to its kit
with `Kit::tests`, as it gives a `PackageManager` with `Kit::packages`:

| `TestRunner` | |
|---|---|
| `provider()` | The manifest's provider id. |
| `runner()` | The runner in use and why it can't run, when it can't (`("cargo nextest", None)`); `("", None)`: nothing to run in this project. Told to Fenix as the pack starts -- Fenix keeps it per project across the pack starting again, and a call that changes nothing does nothing -- then on each save or `Kit::tests_runner_changed` where it changed. Keep it quick. |
| `scan(path, text)` | The tests in a file's text, nothing run: `Found`s with a key, a label, their `groups` root first (a target or package, a file, suites), a line from 1, `skip`, `template` for a test whose cases only a run finds, `of` for a case, `data` of the pack's own. The answer replaces the file's last, and an empty one drops its tests: a scanner that parses, and can't read half-typed text, answers what it last read well (`tests::LastGood`). The same key twice: the later is kept. |
| `plan(ask)` | The `Step`s that list (`Ask::Discover`), run (`Ask::Run`, `Ask::RunAll`) or build before debugging (`Ask::Debug`): a command, a label, what it `covers` (keys, or a group by its labels, which stands for all under it; empty: judged when the plan ends), `needs` and `files`. `list_after(ask)`: listing is cheap once this ran. A plan of no steps has its whole ask errored at once; after a step Fenix couldn't start, the plan goes on. |
| `reader(ask, step)` | A `Reader` for one step: whole lines, one at a time, as `Report`s -- `Found`, `Started`, `Ended`, `Output`, `Broken` (didn't build or load, `under` a group), `Crashed` (`under` the group whose process died), `Keep` (for `launch`). |
| `launch(test, kept)` | `Launching::Now(LaunchConfig)`, or `BuildFirst(steps)`, read as a run's (a `Found` in them is ignored); then asked again, as the same run, with what their readers kept. |
| `assets()` | Reporter files its commands read: written to `/data/lang-kit/tests/` as the pack starts, where they differ. |
| `is_case`, `reader_after_restart` | Rarely needed: a reported key that's a template's case when nothing else says, and a reader for a run this start of the pack didn't plan. |

**What a runner can rely on in an `Ask::Run`** (Fenix §4, §19 findings
2 and 3). Fenix sends the nodes picked -- the topmost of them, in the
page's order -- and after each file or suite its *fill-in*: every node
under it, suites included. lang-kit tells the two apart by `parent`
alone (a record whose `parent` is another record of the ask is a
fill-in), never by reading ids, and gives the runner:

- `Asked::Test(found)` for each test or case picked; `Asked::Group {
  groups, tests, known }` for each group, file or suite picked, `groups`
  root first.
- `tests: Some(..)`: the file's or suite's fill-in, whole -- every test
  and case under it, in the page's order, the suites between in their
  `groups`. A runner whose filter is a substring (libtest's `geo::`)
  names them exactly.
- `tests: None`: no fill-in -- a target or package (named as itself),
  one that didn't fit in the ask's 10,000 records, or one holding a test
  Fenix can't name. Plan it with the runner's own filter for it, which
  runs at least what's under it; `known` is what lang-kit remembers
  under it from this start's scans and listings.
- What Fenix gives back is the node as it has it: the path as
  `/project/<path>` (lang-kit gives the runner `src/lib.rs`; a full path
  outside the project as it is), the line with that path from the first
  of scan, listing and run that gives either (a scan's may be of unsaved
  text), the parent Fenix put it under (none when its parent was never
  found), and **`data` that may be `None`**: over 4 KiB, past 16 MiB of
  the provider's, or after Fenix starts again until a scan, listing or
  run gives it again. So a key of 1 KiB or less must be enough to run its
  test. A key over 1 KiB comes back cut (below), and Fenix never asks for
  it without its `data`: it asks the nearest group it can name instead.
- After the pack starts again, lang-kit remembers nothing until it
  scans: a key is what Fenix sent, groups come from the parent's id.

What the kit does behind `Kit::tests_scan`, `tests_plan`, `tests_read`,
`tests_step_ended` and `tests_launch` (the five exports):

| | |
|---|---|
| **Ids and groups** | A test's id is its key, cut as Fenix cuts one: a key over 1,024 bytes is its first 1,007 bytes (back to a character's start), `#` and the FNV-1a 64-bit hash of the whole key in 16 lowercase hex digits (`tests::cut_id`, byte for byte `fenix_tests::cut_id`, pinned by Fenix's test vectors), so two that share their first KiB stay two. A group's id is its labels, root first, each after U+001F (`\u{1f}orbit (lib)\u{1f}src/lib.rs`), so it's never a key; each group a `Found` names goes to Fenix first, once an answer. Lines from 1 become Fenix's from 0; a relative path becomes `/project/<path>`. |
| **Readers** | One per run and step, made at its first lines, dropped at its end; a run it didn't plan (the pack started again) gets a fresh one, and a line in the log. |
| **Cut lines** | Fenix cuts a line longer than 64 KiB into pieces; a piece's rest is the next line *of the same stream*, the other stream's lines and a call's end coming between. The pieces are joined per stream, across calls, up to 1 MiB, before the reader sees the line. |
| **What it remembers** | The last scan of each file and the last listing: a listing's or a run's `Found` for a key a scan found keeps the scan's groups, file and line; a key a run reports that a known template's key starts at a separator (`TestTable/negative_one`, `test_cases[negative]`, `tests::x::case_1`) or that sits at a template's line is that template's case, sent as `found` before its result; a file or suite asked with no fill-in comes with what it remembers under it (`known`). |
| **Results** | Each `Ended` goes as it came: Fenix keeps the last for a key in a run, so a runner's retry wins. A reader whose runner reports a test in phases (pytest's setup, call and teardown) holds them until the test is whole and reports one, `Ended::and_then` joining them (the worse outcome, its message and place, the times added, the texts in order). A template whose cases have results needs no result of its own: lang-kit makes none (Fenix doesn't count it, and its state is its cases'); report one only when the runner gives one (Go's parent test). `Broken` and `Crashed` error only what has no result in the run yet. |
| **Long command lines** | `tests::split(items, limit, make)` makes several commands of one selection, each fitting `limit`: `DIRECT` (32,766) or `BATCH_FILE` (8,191, a `.cmd` shim run through `cmd.exe`). They're counted as Fenix counts them -- as Rust's standard library builds the line for `CreateProcessW` (or gives a batch file to `cmd.exe`), after `${data}` and `${tools}` are filled in, with the program's full path -- and, since a pack knows neither, with each of those three counted as 260 characters holding a space (`PATH_ALLOWANCE`, `Limit::len`): that's the margin. A plan of more than 256 steps is an error: plan by groups. |
| **Reporters and files** | An asset its args or env name (`${data}/lang-kit/tests/fenix_tests.py`) is put in the step's `needs`; a step's `files` are written under `/data/lang-kit/tests/runs/<run>-<step>/` (`${step_files}` in its args), named in `needs`, and deleted when it ends. |
| **Builds** | A `build-first` plan runs as the run of the first `tests_launch`, and the second carries it: what its readers `Keep` is given to `launch` then. A `Found` they report isn't sent (Fenix ignores it). |
| **The runner in use** | `test-provider-set` as the pack starts, then only when `runner()` says something else. |

A made-up runner, `tally`, whose files say `# test <name>` and whose
`tally run <keys>` prints `PASS <key> <ms>` or `FAIL <key> <why>`:

```rust,no_run
use fenix_extension_api::Command;
use fenix_lang_kit::tests::{split, Ask, Asked, Ended, Found, Group, Line, Outcome, Reader, Report, Step, TestRunner, DIRECT};
use fenix_lang_kit::Kit;

struct Tally;

impl TestRunner for Tally {
    fn provider(&self) -> &str { "tally" }
    fn runner(&mut self) -> (String, Option<String>) { ("tally".into(), None) }
    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path);
        text.lines().enumerate()
            .filter_map(|(i, l)| l.strip_prefix("# test ").map(|name| (i, name)))
            .map(|(i, name)| Found::test(format!("{file}::{name}"), name).under([Group::package("tally"), Group::file(file)]).line(i as u32 + 1))
            .collect()
    }
    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        let mut steps = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        if let Ask::Run(items) = ask {
            for item in items {
                match item {
                    Asked::Test(f) => keys.push(f.key.0.clone()),
                    // A file's fill-in names its tests exactly.
                    Asked::Group { tests: Some(tests), .. } => keys.extend(tests.iter().map(|t| t.key.0.clone())),
                    // None: the runner's own filter for the file runs them.
                    Asked::Group { groups, tests: None, .. } => {
                        let file = groups.last().map(|g| g.label.clone()).unwrap_or_default();
                        steps.push(Step::new(Command::new("tally").args(["run", "--file", &file]), file));
                    }
                }
            }
        }
        // Every key, in as many steps as the command line needs.
        steps.extend(split(&keys, DIRECT, |some| Command::new("tally").arg("run").args(some.iter().cloned()))
            .into_iter()
            .map(|cmd| { let covered = cmd.args[1..].to_vec(); Step::new(cmd, "tally").covering(covered) }));
        Ok(steps)
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> { Box::new(Results) }
}

struct Results;

impl Reader for Results {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        let mut words = line.text.splitn(3, ' ');
        match (words.next(), words.next(), words.next()) {
            (Some("PASS"), Some(key), ms) => out.push(Report::Ended(Ended::new(key, Outcome::Passed).ms(ms.and_then(|m| m.parse().ok()).unwrap_or(0.0)))),
            (Some("FAIL"), Some(key), why) => out.push(Report::Ended(Ended::new(key, Outcome::Failed).message(why.unwrap_or("")))),
            _ => {} // not a result: passed over
        }
    }
}

let kit = Kit::new("tally").tests(Box::new(Tally));
// The pack's `impl Extension` hands Fenix's five calls to it (on API 1.6):
// fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<tests::Found> {
//     self.kit.tests_scan(provider, path, text)
// }
// ... and `tests_plan`, `tests_read`, `tests_step_ended`, `tests_launch` the same.
# drop(kit);
```

Its tests go on the [test kit](../test-kit/README.md)'s driver,
`fenix_test_kit::tests::Fenix` (the test kit's feature `tests`), which
plays Fenix's part, as strictly as Fenix, on recorded output of its
runner; `crates/test-kit/tests/a_test_provider.rs` is this runner grown
to every rule, and `fenix_rulings.rs` holds the driver to Fenix's §19.

A pack tests its own wiring with the host lang-kit's tests use, the
`mock` feature's `host::mock::{Mock, Shared}`: every call written down as
a line (a notification as `notification low #install.zls: Installed ZLS --
...`, as the test kit's `Fake` writes it too), programs found from a map. Or, better, with the
[test kit](../test-kit/README.md)'s `Fake` (its `lang-kit` feature), which
is a `Host` writing the same lines, and answers too -- a process's
output, a download -- sees the pack's own calls to the API, drives it as
Fenix does and reads its pages as text. Zig's tests use it. lang-kit's own
tests keep the mock, since the test kit depends on lang-kit.

```toml
[dev-dependencies]
fenix-lang-kit = { path = "../../crates/lang-kit", features = ["mock"] }
```

```text
cargo test --manifest-path crates/lang-kit/Cargo.toml
cargo test --manifest-path crates/lang-kit/Cargo.toml --features tests   # the test provider, and this README's examples
cargo test --manifest-path crates/lang-kit/Cargo.toml --all-features
```
