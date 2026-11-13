# fenix-test-kit

Testing an extension in plain `cargo test`, without Fenix: a fake host
that writes down what the extension asked of it and answers with what
the test gives it, a harness that drives the extension as Fenix does,
and its pages as text to snapshot.

It's built on extension API 1.5 (`fenix-extension-api`, tag `v1.5.0`),
as ui-kit and lang-kit are: an extension using it builds its API from
that tag too, so their types are one (`engine.api = "1.5"`, Fenix 1.5 and
after).

```toml
[dev-dependencies]
fenix-test-kit = { path = "../../crates/test-kit" }
# A language pack: the fake as lang-kit's Host too.
fenix-test-kit = { path = "../../crates/test-kit", features = ["lang-kit"] }
# A pack that's a test provider: the driver, tests::Fenix.
fenix-test-kit = { path = "../../crates/test-kit", features = ["tests"] }
```

## Why it's needed

An extension reaches Fenix through the API's host calls, which are
WebAssembly imports. Built natively for `cargo test` they compile, but
every one of them panics (`internal error: entered unreachable code`),
so a test could only reach code that never calls the host. The kit gives
a test a host of its own, and two ways to reach it:

- **`fenix_test_kit::api`**, the API with the fake for its host. It's
  everything `fenix_extension_api` has -- types, `Page`'s builders,
  `export!` -- with each free function, each `page::`, `buffer::`,
  `project::` and the like call, every `host::` call of API 1.5 (1.0's
  and what 1.1 to 1.5 added: `key_for`, `key_scheme`, `notify_dismiss`,
  `compose_with_ask`, `progress`, `host::notify_with`,
  `host::git_state`, `host::last_task`), and `info!`, `warn!` and
  `error!` going to the fake. Each is checked at compile time to have
  the API's own signature. An extension takes it
  in its tests in place of the API, by the name it calls the API by:

  ```rust,ignore
  #[cfg(not(test))]
  use fenix_extension_api as api;
  #[cfg(test)]
  use fenix_test_kit::api;
  ```

  Outside its tests that's the line it had, so its WebAssembly doesn't
  change, and the kit stays a dev-dependency.
- **lang-kit's `Host`**, for a language pack: `Fake` is one (feature
  `lang-kit`), given to the pack's `Kit::with_host`. What the kit asks
  and what the pack asks through `api` go to the one fake, in one log.

## A test

```rust
use fenix_test_kit::{api, render, Fake, Harness, Ran};
use api::{page, Event, Extension, Key, Page};

#[derive(Default)]
struct Branches { page: Option<u64>, git: Option<u64>, names: Vec<String> }

impl Extension for Branches {
    fn run_command(&mut self, _id: &str) -> Result<(), String> {
        self.page = Some(page::open("branches", "Branches"));
        self.git = Some(api::host::spawn(&api::Command::new("git").args(["branch", "--format=%(refname:short)"]))?);
        Ok(())
    }
    fn handle(&mut self, event: Event) {
        match event {
            Event::ProcessOutput(o) if Some(o.process) == self.git => self.names.push(o.line),
            Event::ProcessExit((id, _)) if Some(id) == self.git => page::refresh(self.page.unwrap()),
            Event::Picked((_, Some(name))) => api::notify(&format!("on {name}")),
            _ => {}
        }
    }
    fn page(&mut self, _: u64, _: u32, _: u32) -> Page {
        let p = Page::new().line([page::title("Branches")]).rule();
        self.names.iter().fold(p, |p, n| p.line([page::text(n.clone())])).focus(2)
    }
    fn page_key(&mut self, _: u64, key: Key) {
        if key == Key::Enter {
            let items: Vec<_> = self.names.iter().map(|n| api::PickerItem::new(n.clone(), n.clone())).collect();
            api::pick("Switch to", &items);
        }
    }
}

let fake = Fake::new();
fake.on_spawn("git branch", Ran::ok("main\nfeature/kit\n"))
    .on_pick("Switch to", "feature/kit");
let mut h: Harness<Branches> = Harness::with_fake(fake.clone());

h.command("branches.open").unwrap();
let page = h.page("branches");
assert_eq!(h.screen(page), "Branches\n───────────\nmain\nfeature/kit\n");
assert!(render::marked(&h.render(page, 80, 24)).starts_with("  «title:Branches»"));

h.key(page, Key::Enter);
assert_eq!(fake.notifications(), ["on feature/kit"]);
assert_eq!(fake.take()[..3], ["page 1 branches Branches", "spawn 2 git branch --format=%(refname:short)", "refresh 1"]);
```

## Keys of the scheme in use

`key_for` and `key_scheme` (API 1.5) are the fake's too, so a hint that
names a key -- "`SPC h g` opens the gallery", `C-k h g` with standard
keys -- is tested as it is, natively, with no `#[cfg(target_arch =
"wasm32")]` around the call and no literal fallback for `cargo test`:
the code calls `api::key_for`, and the test says what Fenix would
answer. A key bound only in a buffer's `SPC m` menu is no key to
Fenix's `key_for`, nor to the fake's (`menu_key`): the extension writes
those itself, from the leader.

```rust
use fenix_test_kit::{api, Fake, Harness};
use api::{page, Extension, KeyScheme, Page};

/// The keys that run `command` now, else `vim` with the leader of the
/// scheme in use.
fn keys(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| match (api::key_scheme(), vim.strip_prefix("SPC ")) {
        (KeyScheme::Standard, Some(rest)) => format!("C-k {rest}"),
        _ => vim.to_string(),
    })
}

#[derive(Default)]
struct Hints(Option<u64>);

impl Extension for Hints {
    fn run_command(&mut self, _id: &str) -> Result<(), String> {
        self.0 = Some(page::open("hints", "Hints"));
        Ok(())
    }
    fn page(&mut self, _: u64, _: u32, _: u32) -> Page {
        Page::new()
            .line([page::text(format!("{} opens the gallery", keys("gallery.open", "SPC h g")))])
            .line([page::text(format!("{} formats the file", keys("demo.format", "SPC m f")))])
    }
    // Told the scheme changed: the hints are drawn again.
    fn key_scheme_changed(&mut self, _: KeyScheme) {
        if let Some(p) = self.0 {
            page::refresh(p);
        }
    }
}

let fake = Fake::new();
fake.key("gallery.open", "SPC h g").menu_key("demo.format", "f");
let mut h: Harness<Hints> = Harness::with_fake(fake.clone());
h.command("hints.open").unwrap();
let p = h.page("hints");
assert_eq!(h.screen(p), "SPC h g opens the gallery\nSPC m f formats the file\n");

h.key_scheme(KeyScheme::Standard); // set in the fake, then `key_scheme_changed`
assert_eq!(h.screen(p), "C-k h g opens the gallery\nC-k m f formats the file\n");
assert_eq!(fake.take().last().map(String::as_str), Some("refresh 1"));
```

`fake.key(command, "SPC h g")` is written as Vim keys write it; with
standard keys the fake says its `SPC` as the leader chord, `C-k` (or what
`leader_chord` sets), as Fenix's key menu does. `key_in(scheme, command,
keys)` gives one scheme keys of its own (`F12` for `gd`), `no_key` takes
them away.

## Notifications

`api::host::notify_with(&notification)` (API 1.3) is the fake's;
`Notification::show()`, the API's method, calls Fenix's and panics
natively, so code a test reaches calls the host function, which is what
`show` does:

```rust,ignore
api::host::notify_with(&api::Notification::new("Build passed").body("in 41 s").urgency(api::Urgency::Low).tag("build"));
```

`fake.toasts()` is every one, in order, as given -- summary, body,
urgency, tag, actions, progress; `fake.showing()` those Fenix would show
now, one with a tag in place of the one before with that tag, none whose
tag was dismissed since (`api::notify_dismiss`, `fake.dismissed()`). A
line in the log is `notification low #build: Build passed -- in 41 s`,
with ` [Label -> command]` for each action. `fake.notifications()` is
still the message line's, `api::notify`.

Kept as a snapshot instead, in `tests/snapshots/branches.txt` beside the
crate's tests:

```rust,ignore
fenix_test_kit::assert_snapshot!("branches", h.screen(page));
```

A missing or different snapshot fails the test, with a diff;
`UPDATE_SNAPSHOTS=1 cargo test` writes them as they are now. Read the
diff before you commit them. `tests/` isn't packed.

## What's in it

| | |
|---|---|
| `Fake` | The host. Set up what the extension finds: `setting`, `secret`, `env`, `program` (for `find-program`), `platform`, `theme`, `palette`, `in_file` (where you are, and that buffer's text), `cursor`, `selection`, `project`, `buffer`, `file` (for `read-file`), `clipboard`, `clock` (`api::clock::now`), `now` (lang-kit's); and from API 1.1 to 1.5 `key_scheme`, `key`, `key_in`, `no_key`, `menu_key` and `leader_chord` (what `key_for` answers), `git` (`git_state`), `last_task`. Give its answers: `on_spawn` (a `Ran`: stdout and stderr in lines, an exit code; or still running), `on_spawn_error`, `on_spawn_with` (a closure), `on_http` and `on_http_error` (`URL`, `METHOD URL`, a `*` at the end), `on_download` (writes the file), `on_service` and `on_service_with`, `on_pick`, `on_pick_many`, `on_pick_nothing`, `on_prompt` and `on_prompt_nothing` (prompts, secrets' prompts, compose panes, the agent's too), `on_task` and `on_task_tests` (its tests counted, for `last_task`). The last answer given for a pattern wins. Read what it did: `take` (the calls since last asked, a line each), `calls`, `notifications` (the message line's), `toasts`, `showing` and `dismissed` (notifications), `logs`, `status`, `open_pages` and `page(kind)`, `spawned` and `process(pattern)` (stdin written, closed, killed), `requests`, `timers`, `pickers`, `prompts` (a compose pane's `ask`), `services`, `progress_of(call)`, `problems(path)`, `decorations(path)`, `text(path)` (a buffer with its edits made), `copied`, `checks`, `tasks`, `settings_read`, `keys_asked`, `setting_value`; the rest, whole, in `state()`. |
| `Harness` | `activate`, `command`, `event`, `key`, `keys("jj<Enter>")`, `press`, `ctrl`, `named`, `paste`, `click`, `mouse`, `close` (you closed a page), `saved`, `focus`, `changed`, `serve`, `set_setting` (set, then `setting-changed`), `key_scheme` (set, then `key-scheme-changed`), `cancelled` (`service-cancelled`), `ask_progress` (`agent-ask-progress`), `fire` and `fire_timers`, `said` and `exited` (a process's lines by hand); then `render`, `screen` and `marked` a page. After each call the events waiting are given to the extension (`settles`; off, `settle` does it). `Harness::new` makes the extension with `Default`; `with_fake` once a fake is set up; `wrap` takes one made some other way. |
| `render` | `text(&Page)`: a page's lines, or a canvas's cells, a line each; hairlines as `─` and `│`, pictures as `▒`, a popup under its line, the keys at the foot. `marked(&Page)`: the same, the focused line with `>` in the gutter, and a run in a role, on a background, underlined or in a colour as `«accent bg=panel:text»`. `canvas` and `canvas_marked` take a `ui-kit` `Canvas` (feature `ui-kit`, on by default): a widget's test, or a gallery's. Spaces at lines' ends go. |
| `assert_snapshot!` | Text against `tests/snapshots/<name>.txt`; `UPDATE_SNAPSHOTS=1` writes it. |
| `tests::Fenix` | Feature `tests` (which turns on `lang-kit` and lang-kit's own `tests`): Fenix's side of a test provider, on recorded output ([below](#a-test-provider-on-recorded-output)). |
| `Fake` as lang-kit's `Host` | Feature `lang-kit`. Its lines are lang-kit's mock's (`spawn 3 /t/zig version`, `tasks build, fmt`, `problems /project/a.zig 1`, `notification low #install.zls: Installed ZLS -- ...`, and with the feature `tests` `test-provider rust: cargo nextest`), except a log line says its level (`warn ...` for what the mock wrote `log ...`). An install the kit ran is in `toasts` and `showing`. `fake.host()` is it boxed for `Kit::with_host`; `server_settings(name)` what a server was given. |

## What it doesn't do

- **Methods on the API's types that call the host**: `Command::spawn`,
  `spawn_raw`, `run_as_task`, `run_in_background`, `run_in_terminal`,
  `run_below`, `HttpRequest::send`, `send_with` and `download`,
  `Notification::show`. They're the API's code, so natively they panic.
  Call what they stand for: `api::host::spawn(&cmd)`,
  `api::host::run_task(name, &cmd)`, `api::host::http(&req)`,
  `api::host::notify_with(&n)` -- the same calls, in both builds.
- **Asking an agent**: `agents::ask` and `agents::ask_with` wait on the
  API's own `run` executor, whose service calls are Fenix's. A test can
  give the extension the answer as an event (`ServiceReply`) and drive
  the streamed draft with `Harness::ask_progress`.
- **A call by the API's own path** (`fenix_extension_api::notify`, a
  `use fenix_extension_api::page`): Fenix's, so it panics. Every call
  goes through the `api` name.
- **Code in another crate**. lang-kit, forge and the rest are built
  without the extension's `cfg(test)`, so their calls are the API's.
  lang-kit has its `Host` for that, which the fake is; the others have no
  seam yet.
- **The API's other modules' host calls**: its `lsp`, `socket`,
  `surface`, `run` (async) and `forge` modules call the host themselves. Their
  `host::` calls are here (sockets and surfaces answer that the test
  host can't), but not those modules' own functions.
- **The filesystem**: `std::fs` is the machine's, and `/data`,
  `/project` and `/tools` aren't there. Keep paths in a field a test can
  point at a temporary folder, as the language packs do.
- **Another tag of the API**: it's built on `v1.5.0`, as ui-kit and
  lang-kit are. An extension on another tag has its types from that tag,
  which aren't the kit's, and Cargo won't patch one tag of a git
  dependency with another: it moves to `v1.5.0` to use the kit. An
  extension on 1.0 to 1.4 that doesn't want Fenix 1.5's floor can't use
  it.
- **Timing**: the fake answers at once, in order; timers go off when a
  test fires them.

## A test provider, on recorded output

For Fenix 1.6 a language pack can be a test provider (lang-kit's
[Tests](../lang-kit/README.md#tests), `plans/test-explorer.md`). Fenix
runs its commands and keeps the results; the pack scans, plans and
reads. `fenix_test_kit::tests::Fenix` (feature `tests`) is Fenix's side,
so a provider is tested natively on what its runner printed once: it
scans a file's text, asks for a plan, "runs" each step by giving the
provider the recorded lines as Fenix would, ends it, and holds what
Fenix would then hold. It follows Fenix's plan as its review of steps 1
and 2 settled it (Fenix's `docs/plans/test-explorer.md` §19) and Fenix's
model, `fenix-tests`, whose numbered Decisions its code cites. On API
1.5 it calls `Kit`'s five methods (its `Exports` trait, which `Kit` and
`&mut Kit` are, and which a test can implement itself to drive Fenix's
side alone); once the kits are on 1.6, a pack's `impl Extension` too.

The feature `tests` turns on lang-kit's `tests` for the pack's tests
only (a dev-dependency's features don't reach its WebAssembly); the pack
turns lang-kit's on in its own `[dependencies]` too, as lang-kit's
README says.

```rust,ignore
use fenix_test_kit::tests::{Fenix, Order, Recorded, State};
use fenix_test_kit::{assert_snapshot, Fake};

let fake = Fake::new();
let mut fenix = Fenix::new(Kit::with_host("rust", fake.host()).tests(Box::new(Cargo::default())));
fenix.manifest("extension.toml").with_fake(fake.clone());   // the programs it may run
fenix.scan("rust", "/project/src/lib.rs", include_str!("samples/tests/cargo/lib.rs"));
fenix.on_command("-- --list", Recorded::from("src/samples/tests/cargo/list"));
fenix.on_command("--lib", Recorded::from("src/samples/tests/cargo/run-human"));
let run = fenix.run_all("rust");
fenix.assert_states("rust", &[("orbit/lib tests::adds", State::Passed), ("orbit/lib tests::case_negative", State::Failed)]);
assert_eq!(fenix.at("rust", "orbit/lib tests::fails_on_purpose"), Some(("src/lib.rs".into(), 25, Some(9))));
assert_snapshot!("cargo-run-all", fenix.tree("rust"));
// The same, the streams interleaved as Windows might, three lines a call.
fenix.order = Order::Shuffled(7);
fenix.batch = 3;
// As after Fenix starts again: no `data` until found again; the pack
// plans from its keys.
fenix.forget_data("rust");
fenix.run("rust", &["orbit/lib tests::adds"]);
```

**A fixture** is what one command printed: `<name>.stdout` and
`<name>.stderr` as Fenix keeps them apart, and, when there,
`<name>.exit` (a code, or `signal`), `<name>.order` (`o` and `e`, a line
each, how the two came) and `<name>.args` (the command line, for
`on_recorded`). Paths of the machine that recorded it are replaced by
`/project`. A pack keeps its fixtures in
`extensions/<pack>/src/samples/tests/<runner>/` (`src` isn't packed) and
its reporters in `extensions/<pack>/src/tests/`. `Recorded::stdout(..)`
writes one inline; `exits`, `signal`, `fails`, `stopped_after`,
`timed_out_after` and `restarts_after` end it otherwise.

| It does | Fenix |
|---|---|
| Gives each step's lines as Fenix gives them: a line at each `\n`, its last `\r` dropped, lossy UTF-8, colour codes taken out; a line longer than 64 KiB (`cut_at`) in pieces cut at a character's start, each but the last `cut` -- one of 64 KiB or less, at a stream's end too, never; the streams interleaved as recorded, stdout first, or shuffled per tick from a seed (`order`), each in its own order, so a cut line's pieces may have the other stream's lines and a call's end between them; at most 512 lines and 256 KiB a call (`batch`) | §6.4, §19 K3, K4 |
| Then `tests_step_ended` with the recorded end; after a stop or a timeout, `stopped` for each step not started, what those answer dropped; a pack that started again gets no call about the run, which is stopped, its tests back to what they were | §6.4, §8.3; Decisions 15 |
| Holds every step to what Fenix starts: a program `permissions.process` allows (a name, `${setting:…}`, `/data/` or `/tools/`); its command line counted as Rust builds it for `CreateProcessW`, after `${data}` and `${tools}` are filled in, with the program's full path (a name taken as under `PROGRAMS_ON_WINDOWS`), at most 32,766 -- for a `batch_file`, the line Rust gives `cmd.exe`, at most 8,191; each `need` a `/data/...` path with `/`, of at most 4 KiB, that's there. One that isn't ends `failed`, its tests errored with why, and the plan goes on | §5, §6.5, §19 K2, K8; Decisions 13 |
| Holds every plan and answer to Fenix's limits: 10,000 found a scan, 10,000 reports a call, 256 steps, 10,000 ids of covers, 32 needs a step, 64 KiB of env, `data` 4 KiB (one over isn't kept), 16 MiB of `data` a provider (past it, a node's isn't kept, and that's said) | §6.5; Decisions 26 |
| Cuts what Fenix cuts, and says it once (`said()`): an id over 1 KiB as Fenix cuts it (`cut_id`), a `parent` and `covers` the same; a label over 256 characters, a path over 4 KiB, a skip over 1 KiB, a message over 4 KiB, a text over 64 KiB (its end) | §6.5, §19 finding 9; Decisions 33 |
| Keeps a record per source for each node -- each file's scan, the listing, a run -- merged as Fenix merges them: field by field scan, else listing, else run; the place (`path`, `line`, `end_line`) as one from the first that gives a path or a line; `skip` the scan's where it has the node; `label`, `kind`, `parent`, `template` the first's. A scan's missing path is the file scanned (but for a group). Each source drops its own: a file's next scan (an empty one drops the file's tests), a complete listing, a complete run of a template or group. A test that leaves every source keeps its result for when it comes back; a case its template's complete run dropped goes with its | §4; Decisions 2, 5, 24, 29, 30, 31 |
| Places a node under its parent when it has it; a parent not found yet, itself, a loop or a chain past 256 puts it at the top until it's found. Orders children as the page does: by path, line, then when found. The same id twice in one scan: the later wins | §4; Decisions 3, 6 |
| Matches results by id: a known one; one a `found` added in the run; an unknown one added under the deepest group, file or suite of its `at`'s file holding its line, *found in a run*. The last `ended` in a run is the result; what a test printed before it is kept with it | §4; Decisions 21 |
| Counts a test or case with none under it: a template once its cases are found isn't counted, its state is its cases' (and its own result's, when the runner gives one); a group's state is failed, errored, running, queued, passed, else unknown | §4; Decisions 7, 8 |
| Judges a step's scope as it ends -- what it covers (a group's, file's or suite's id standing for what's under it), within what the run asked -- and the plan's rest as its last ends: `errored` "the run ended without a result for it" if it exited, back to what it was if stopped or of a file with unsaved edits, `errored` with why if it failed or timed out. A plan of no steps has its ask judged at once; one that's an error errors its ask with it; a timeout errors what had no result | §8.3, §8.4, §8.5, §19 K8; Decisions 10, 12, 13, 14 |
| `broken` and `crashed`, with an id or none, error only what has no result in this run yet: what's under the id, or, with none (or one never found, which is said), the step's scope -- for a step that claims nothing, the plan's rest. In a listing they error nothing and make it incomplete | §8.5, §19 K5, K6, K9; Decisions 11, 17 |
| Builds the ask Fenix makes (`ask_for`, `run`, `run_failed`), as `fenix-tests` does: the topmost selected, a node it can't name (a cut id without `data`) as its nearest ancestor that can be, past 10,000 the deepest as their parents (at the top, `run-all`); in the page's order, each file or suite followed by its whole fill-in -- every node under it, suites included -- while it fits and holds nothing that can't be named. Each node as Fenix gives it back (`given_back`): its path `/project/<path>`, its place, the parent Fenix put it under, its `data` as kept. `forget_data` drops every `data`, as after Fenix starts again | §4, §6.4, §19 findings 2, 3, K12; Decisions 20, 32 |
| `launch`: a node it can't name isn't debugged; a `build-first` plan runs as the launch's run (its `found`s and `ended`s ignored), then it's asked again once, with that run, only if every step exited 0 and nothing was broken or crashed | §11, §19 finding 1, K9; Decisions 17, 32, 34 |
| A run says whether it's `complete` (planned, a step or more, all exited, nothing broken or crashed, not stopped) and whether Fenix would list after it (`lists_after`) | §4; Decisions 23 |
| Times each call natively: over `budget` (5 ms in a release build, 50 ms in a debug one) fails the test | §6.5, §6.7 |

It's as strict as Fenix: what Fenix refuses or loses -- a step it
wouldn't start, a plan or answer past its counts, `data` over 4 KiB --
fails the test, saying which; `lenient()` handles it as Fenix does
instead and keeps what Fenix would say (`said()`). What Fenix accepts
and only says once on the provider's output page -- an id over 1 KiB, a
label or path cut, a `broken` naming a node it never found -- is in
`said()` either way, and fails nothing. Read the result with `state`,
`detail` (message, place, text, time), `at`, `record`, `given_back`,
`failed`, `assert_states`, `told` (what `test-provider-set` said, from
the fake), `model`, and `tree`: every node a line, in the page's order,
with its kind, line, state, result and sources, stable enough to
snapshot.

### Where it can't be Fenix exactly

- **The program's full path** is the machine's: the driver counts a
  name as `C:\Users\someone\AppData\Local\Programs\<name>\<name>.exe`
  (`.cmd` for a `batch_file`), `/data/...` and `/tools/...` under
  Windows-length folders. lang-kit's `split` counts each of these at 260
  characters, so what it splits fits either way.
- **What isn't simulated**: the queue and several providers in one run,
  stale marks, the results file across a restart of Fenix (`forget_data`
  stands for its `data` side), a listing started by `list_after` (the
  run says `lists_after`; `discover` runs one), a provider falling behind
  its step's output, and the 16 MiB a call's answer may hold.

Every rule the kit's first version found unspecified is ruled now
(Fenix §19, "What the packs' kit asked", K1 to K14), and the driver
follows each ruling; nothing it relies on is left open.

## lang-kit's mock

lang-kit's `host::mock` (its `mock` feature) is the fake this grew from,
and lang-kit's own tests keep it: lang-kit can't use this kit, which
depends on it. For a pack, this one does what the mock did and more --
the answers, the harness, its pages as text -- and sees the pack's own
`api` calls too. Moving a pack over is its tests only: `Shared::default()`
becomes `Fake::new()`, `Box::new(host.clone())` becomes `fake.host()`,
`host.0.borrow_mut().programs` becomes `fake.program(..)`,
`host.0.borrow().checks` becomes `fake.checks()`, and a `log ...` line
becomes `warn ...`; a process answered by hand can be `on_spawn` instead.
[Zig's tests](../../extensions/zig/src/lib.rs) are the example.

```text
cargo test --manifest-path crates/test-kit/Cargo.toml   # its own tests have every feature
```
