# fenix-lang-kit

What every language pack's code extension shares, through `Host`, so
each pack says only what its language has: the package manager page, the
project doctor's checks, tasks, a server or a debug adapter installed
into the tools folder, a formatter and a linter; and, for Fenix 1.6, a
test provider ([Tests](#tests)).

```toml
[dependencies]
fenix-extension-api = { git = "https://github.com/tpedneault/fenix", tag = "v1.5.0" }
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
on extension API 1.5 (tag `v1.5.0`), so a pack using it builds its API
from that tag too, and needs Fenix 1.5 (`fenix = ">=1.5, <2"`).

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
| `tests` | A test provider for Fenix 1.6: below. |
| `Host` | Every host call; `Fenix` makes them. The later ones -- HTTP, `platform`, `open_url`, a server added or given settings, an adapter over a socket, problems with fixes, background tasks, launches and their variables, a notification and its dismissal, a test provider's runner (`test_provider_set`, nothing on API 1.5) -- have defaults that say the host can't (a notification is said in the message line), so a host of a pack's own needn't have them. |

## Tests

For Fenix 1.6 (extension API 1.6), a pack can be a **test provider**:
Fenix's tests page, gutter and `SPC T` run its tests and show their
results, and the pack says only what its runner knows -- how to find
tests in a file's text, what commands list and run them, how to read
what they print, how to debug one. The design is
[`plans/test-explorer.md`](../../plans/test-explorer.md) and Fenix's
`docs/plans/test-explorer.md`. API 1.6 isn't released: `tests::api` is a
copy of its types, nothing here calls more than 1.5 has, and the five
exports are wired on the `api-1.6` branch (`plans/test-explorer.md`,
§8.2).

A pack implements `tests::TestRunner` for each
`[[contributes.test_providers]]` of its manifest and gives it to its kit
with `Kit::tests`, as it gives a `PackageManager` with `Kit::packages`:

| `TestRunner` | |
|---|---|
| `provider()` | The manifest's provider id. |
| `runner()` | The runner in use and why it can't run, when it can't (`("cargo nextest", None)`); `("", None)`: nothing to run in this project. Asked as the pack starts, on each save and on `Kit::tests_runner_changed`, so keep it quick. |
| `scan(path, text)` | The tests in a file's text, nothing run: `Found`s with a key, a label, their `groups` root first (a target or package, a file, suites), a line from 1, `skip`, `template` for a test whose cases only a run finds, `of` for a case, `data` of the pack's own. |
| `plan(ask)` | The `Step`s that list (`Ask::Discover`), run (`Ask::Run`, `Ask::RunAll`) or build before debugging (`Ask::Debug`): a command, a label, what it `covers` (keys, or a group by its labels, which stands for all under it; empty: judged when the plan ends), `needs` and `files`. An asked file or suite comes with the tests Fenix has under it (`Asked::Group.tests`); none: name it with the runner's own filter. `list_after(ask)`: listing is cheap once this ran. |
| `reader(ask, step)` | A `Reader` for one step: whole lines, one at a time, as `Report`s -- `Found`, `Started`, `Ended`, `Output`, `Broken` (didn't build or load, `under` a group), `Crashed` (`under` the group whose process died), `Keep` (for `launch`). Hold a result until it's whole. |
| `launch(test, kept)` | `Launching::Now(LaunchConfig)`, or `BuildFirst(steps)`, read as a run's; then asked again with what their readers kept. |
| `assets()` | Reporter files its commands read: written to `/data/lang-kit/tests/` as the pack starts, where they differ. |
| `is_case`, `reader_after_restart` | Rarely needed: a reported key that's a template's case when nothing else says, and a reader for a run this start of the pack didn't plan. |

What the kit does for it, behind `Kit::tests_scan`, `tests_plan`,
`tests_read`, `tests_step_ended` and `tests_launch` (the five exports):

| | |
|---|---|
| **Ids and groups** | A test's id is its key, cut to 1 KiB as Fenix cuts one. A group's id is its labels, root first, each after U+001F (`\u{1f}orbit (lib)\u{1f}src/lib.rs`), so it's never a key; each group a `Found` names goes to Fenix first, once an answer. Lines from 1 become Fenix's from 0; a relative path becomes `/project/<path>`. |
| **Readers** | One per run and step, made at its first lines, dropped at its end; a run it didn't plan (the pack started again) gets a fresh one, and a line in the log. |
| **Cut lines** | Fenix cuts a line at 64 KiB; the pieces are joined, per stream and across calls, up to 1 MiB, before the reader sees the line. |
| **What it remembers** | The last scan of each file and the last listing: a listing's or a run's `Found` for a key a scan found keeps the scan's groups, file and line; a key a run reports that a known template's key starts at a separator (`TestTable/negative_one`, `test_cases[negative]`, `tests::x::case_1`) or that sits at a template's line is that template's case, sent as `found` before its result; a file or suite asked with no tests after it comes with the tests it remembers under it (`known`). |
| **One result per test** | A second `Ended` for a key in a run is merged into the first, the worse outcome, its message and place winning, the texts joined; Fenix keeps the last, so it keeps the whole. A template whose cases ended in a step and that had no result of its own gets theirs (`1 of its 2 cases didn't pass`). |
| **Long command lines** | `tests::split(items, limit, make)` makes several commands of one selection, each under `limit` as Windows quotes it: `DIRECT` (Fenix's 32,766 less 1,024 for `${data}` filled in and the program's full path) or `BATCH_FILE` (8,191, the same, for a `.cmd` shim). A plan of more than 256 steps is an error: plan by groups. |
| **Reporters and files** | An asset its args or env name (`${data}/lang-kit/tests/fenix_tests.py`) is put in the step's `needs`; a step's `files` are written under `/data/lang-kit/tests/runs/<run>-<step>/` (`${step_files}` in its args), named in `needs`, and deleted when it ends. |
| **The runner in use** | `test-provider-set` only when `runner()` says something else than it last told. |

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
        let keys: Vec<String> = match ask {
            Ask::Run(items) => items.iter().flat_map(|a| match a {
                Asked::Test(f) => vec![f.key.0.clone()],
                Asked::Group { tests, .. } => tests.iter().map(|t| t.key.0.clone()).collect(),
            }).collect(),
            _ => Vec::new(),
        };
        // Every key, in as many steps as the command line needs.
        Ok(split(&keys, DIRECT, |some| Command::new("tally").arg("run").args(some.iter().cloned()))
            .into_iter()
            .map(|cmd| { let covered = cmd.args[1..].to_vec(); Step::new(cmd, "tally").covering(covered) })
            .collect())
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
// The pack's `impl Extension` hands Fenix's five calls to it:
// fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<tests::Found> {
//     self.kit.tests_scan(provider, path, text)
// }
// ... and `tests_plan`, `tests_read`, `tests_step_ended`, `tests_launch` the same.
# drop(kit);
```

Its tests go on the [test kit](../test-kit/README.md)'s driver,
`fenix_test_kit::tests::Fenix`, which plays Fenix's part on recorded
output of its runner; `crates/test-kit/tests/a_test_provider.rs` is this
runner grown to every rule.

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
```
