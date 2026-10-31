# fenix-lang-kit

What every language pack's code extension shares, through `Host`, so
each pack says only what its language has: the package manager page, the
project doctor's checks, tasks, a server or a debug adapter installed
into the tools folder, a formatter and a linter.

```toml
[dependencies]
fenix-extension-api = { git = "https://github.com/tpedneault/fenix", tag = "v1.0.0" }
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
"read"` and `events = ["saved"]` for the linter, and `api = "1.0"`.

| | |
|---|---|
| `PackageManager` | `list`, `outdated`, `audit` and their parsers; `upgrade`, `upgrade_all`, `add`, `remove` as commands run as tasks; optionally `search` (for `a`), `normalize` (Python's names) and `lockfiles` (the cache's key). Each command runs with its stdin closed; a parser gets the exit code too, since `npm outdated` and most audits exit 1 when they find something. The page runs `*_steps`: by default the one command, else several in turn (`go get`, then `go mod tidy`; a `dotnet add package` each), stopping at one that fails, or a reason it can't (`Err`), said on the page. |
| `PackagesPage` | The page: name, current, wanted, latest, direct or transitive (or its group), the advisory's id and severity; tabs for all, outdated and vulnerable; the vulnerable first, then the outdated (a major first). `u` upgrades one, `U` all (asked first), `a` adds (a search's picker, or what's typed), `d` removes (asked first), `r` lists again, Enter shows a dependency's advisories beside. The vulnerable rows are on red, the outdated on amber; a click picks a tab or a row, a double click shows its details, the wheel scrolls. Each action is a task, one at a time, and it all refreshes after. The last list shows at once from a cache in `/data` while the lockfiles hold what they did; an audit is kept an hour unless something changed. A step that failed says why on the page. |
| `Program`, `Doctor` | A program found with `find-program`, asked its version (5 s, or its `timeout`, with its `env` and `cwd`), held to a `Requirement` (`>=3.9`, `^1.22`, `>=18 <21 \|\| 22`); a fix row that runs a task, an install, or a command of the pack's. The checks are made again after a fix. A program can be checked twice under two labels (`uv`, and the Python uv finds), and `judge` -- a closure, which may hold what it expects -- reads what it said in place of a version (`uv sync --locked --dry-run`: in step or not). `Kit::extra_checks` works the pack's own checks out again each time. |
| `Version`, `Requirement`, `find_version` | Versions as tools print them (`go1.22.3`, `3.12.0rc1`, `1.0-SNAPSHOT`, pseudo-versions), compared; ranges npm, Cargo and pip write; the version in `--version`'s output. |
| `Tasks` | A table of name and command line, a `cwd`, an env file's values (`parse_env`), set with `tasks-set`. |
| `Install`, `Installer` | A release downloaded into `/tools/<name>` and checked by Fenix, unpacked a step a timer (`.zip`/`.vsix`, `.tar.gz`, `.tar.xz`, `.tar`, `.gz`, a plain file) with paths kept in the folder, made to run, marked with its URL, then set as the language server, a server beside the others (`added_language_server`), or the debug adapter, on stdio or over a socket (`debug_adapter_connect`). Used again when the pack starts. |
| `Linter`, `Lint`, `compiler_lines` | Run on `saved` in `/project` (or `Kit::file_saved`'s path), its output read into `Problem`s (`path:line:col: severity: message`, MSBuild's, or the pack's own parser) and set with `diagnostics-set`. `Linter::with_fixes` gives each problem its fixes (`diagnostics-set-ex`, `SPC c a`); `Linter::across` speaks of several files (clippy, a package's vet), each set and those it no longer speaks of made clean. |
| `Risk`, `cvss` | An advisory's severity as tools say it: a word, a score, or a CVSS 2 or 3 vector. |
| `formatter` | `project::formatter`: Fenix runs it on `SPC c F` and on save. |
| `Host` | Every host call; `Fenix` makes them. The later ones -- HTTP, `platform`, `open_url`, a server added or given settings, an adapter over a socket, problems with fixes, background tasks, launches and their variables -- have defaults that say the host can't, so a host of a pack's own needn't have them. |

A pack tests its own wiring with the host lang-kit's tests use, the
`mock` feature's `host::mock::{Mock, Shared}`: every call written down as
a line, programs found from a map.

```toml
[dev-dependencies]
fenix-lang-kit = { path = "../../crates/lang-kit", features = ["mock"] }
```

```text
cargo test --manifest-path crates/lang-kit/Cargo.toml
```
