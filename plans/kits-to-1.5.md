# Moving the kits' dependents to extension API 1.5

> Written on 12 November 2026, as `ui-kit`, `lang-kit` and `test-kit`
> moved to API 1.5 (`kits/core`). Three dependents moved with them, as
> the proof: the widget gallery, Regex and Zig. This is how, so the other
> 25 can follow. Nothing was run in Fenix: there's no Fenix binary where
> this was done.

## What moved

- **The kits** build `fenix-extension-api` from tag `v1.5.0` (API 1.5),
  where they pinned `v1.0.0`. Nothing under `crates/` else pins the API
  (`ccsds`, `diagram`, `forge`, `jira`, `mib`, `nats`, `zstd-shim` don't
  depend on it). Each kit's `Cargo.lock` changed in the API's two lines
  only: the API's dependency list is the same at both tags.
- **The API change is additive for the kits**: they built and their tests
  passed on `v1.5.0` with no change to their code. The survey read it so;
  this built it. The fixes made on the way were Clippy's, not the API's
  (below).
- **The test kit** fakes what API 1.1 to 1.5 added (below).
- **lang-kit** tells how an install ended as a notification (below).
- **ui-kit**: nothing in it draws key hints -- no key strip, no "press
  ..." text; the gallery's strip is the gallery's own -- so it gained
  nothing but the tag.
- **`xtask new`**: the tool and language-pack templates start on
  `v1.5.0`, `api = "1.5"`, `fenix = ">=1.5, <2"`, their locks refreshed.

An extension that uses a kit has to build the API from the kit's tag, or
its types aren't the kit's. So until each of the 25 moves, **it doesn't
build**: its `Cargo.lock` names `v1.0.0`, the kit asks for `v1.5.0`, and
`--locked` refuses. That's expected.

## The recipe, for one dependent

In the extension's folder, `extensions/<name>/`, in this order:

1. **`Cargo.toml`**
   - `fenix-extension-api = { git = "https://github.com/tpedneault/fenix", tag = "v1.5.0" }`
     (was `v1.0.0`).
   - `version` to the next **minor** (`1.2.0` -> `1.3.0`). Raising the
     floor is a minor release ([CONTRIBUTING](../CONTRIBUTING.md#versions)).
   - Leave `fenix-snippets` (a language pack's dev-dependency) on
     `v1.0.0`: `crates/fenix-snippets` is the same at both tags
     (`git -C <fenix> diff v1.0.0 v1.5.0 -- crates/fenix-snippets` is
     empty), and moving it would only churn the lock.
2. **`extension.toml`**: the same `version`; under `[engine]`,
   `fenix = ">=1.5, <2"` and `api = "1.5"`, with a comment saying what
   it uses of 1.5 (or that the kit it's built on is on 1.5). Do this
   **before** packing: `pack` puts the manifest in the package as it is.
3. **`Cargo.lock`**: `cargo update -p fenix-extension-api`. It swaps the
   API's source (`?tag=v1.0.0#b4b3567...` for `?tag=v1.5.0#58a1771...`,
   version `1.0.0` -> `1.5.0`) and the crate's own version. Nothing else
   is needed for a pack on lang-kit or a page on ui-kit; Cargo may also
   pick another `windows-sys` for some Windows-only dependencies (below),
   which is harmless.
4. **Build, test, lint**, from the folder:
   ```text
   cargo test --locked
   cargo clippy --locked -- -D warnings
   cargo clippy --locked --all-targets -- -D warnings
   cargo clippy --locked --target wasm32-wasip2 -- -D warnings
   ```
   and from the repository's root
   `cargo xtask pack extensions/<name> <dir>` (note the size before and
   after) and `cargo xtask validate`.
5. **Use what 1.5 gives**, so the floor buys something
   ([CONTRIBUTING](../CONTRIBUTING.md#versions), "raise `fenix` or `api`
   only in a minor release that uses what the newer one adds"): hints
   that name a leader key through `key_for` (and `key_scheme_changed` to
   draw them again), a notification for something long that ends while
   you're elsewhere. A language pack gets lang-kit's install
   notification with no code of its own. See "Using 1.5" below.
6. **`CHANGELOG.md`**: a section for the new version, opening "For
   Fenix 1.5 (extension API 1.5): ..." and saying what changed for the
   person using it, in sentences. Say plainly when nothing they see
   changed (Regex 1.2.0's: "built against API 1.5, as the shared test kit
   its tests run on now is. The playground and the cheatsheet work as
   they did; on Fenix 1.0 to 1.4, 1.1.0 stays the one you have").
7. **`README.md`**: where it says the Fenix it needs or the API it's
   built on, 1.5; standard keys beside Vim's for its leader keys (`SPC s
   x` and `C-k s x`), as the swept extensions' READMEs have them.
8. **Clean up**: `rm -rf target extension.wasm`.

The three moved here, as examples:

| | Was | Now | Packed size |
|---|---|---|---|
| Widget gallery | 1.0.0, API 1.0 | 1.1.0, API 1.5: its foot says the keys that open it, from `key_for`; its snapshots drawn by `fenix_test_kit::render` | 173,565 -> 174,302 bytes |
| Regex | 1.1.0, API 1.0 | 1.2.0, API 1.5: nothing it does changed | 582,036 -> 582,136 bytes |
| Zig | 1.0.0, API 1.0 | 1.1.0, API 1.5: lang-kit's install notification | 374,316 -> 378,718 bytes |

## What broke, and the fixes

Nothing broke from the API itself, in the kits or the three. What did:

- **lang-kit's Clippy** (Rust 1.97's): `problems.iter().map(|p|
  p.clone())` twice in `host.rs` (now `problems.to_vec()`, and the
  default `diagnostics_with_fixes` passes `problems` on as they are), and
  a `.into()` from `Problem` to `Problem` in a test in `lint.rs`. These
  were there before the move; `cargo clippy --all-targets -- -D
  warnings` is now clean.
- **The test kit's own Clippy**: a tuple type too complex in the fake
  (`task_ends`), given a `type` name.
- **The gallery's snapshots**, when its canvas-to-text helper went onto
  the test kit's: only `menu-open` and `80x24-menu-open` changed. The test
  kit draws a popup under the line it's beside, as Fenix does; the
  gallery's helper listed its rows below the screen. The rows are now in
  the screen, and the footer keeps the popup's place (`popup: beside line
  6, from cell 29`) without listing them again. Every other snapshot is
  byte for byte as it was.
- **Packing before the manifest was edited** packs the old `[engine]`:
  edit `extension.toml` first (step 2).

## What `cargo update -p fenix-extension-api` changed

- **The kits**: the API's `version` and `source`, nothing else.
- **Gallery**: the API's source; `fenix-gallery` 1.0.0 -> 1.1.0; and
  `fenix-test-kit` added, a new dev-dependency (for the fake host in its
  tests and the renderer).
- **Regex**: the API's source; `fenix-regex` 1.1.0 -> 1.2.0.
- **Zig**: the API's source; `fenix-zig` 1.0.0 -> 1.1.0; and Cargo chose
  `windows-sys` 0.60.2 rather than 0.61.2 for `dirs-sys`, `errno`,
  `rustix`, `tempfile` and `winapi-util` (both versions were in the lock
  before and are after; these are Windows-only, under the dev-dependencies
  `fenix-snippets` and `tree-sitter`, and in nothing packed). A lock with
  only the API's lines changed holds too (`cargo metadata --locked`), but
  what `cargo update` writes is what's committed, so that another run
  gives the same.
- **The templates** (`xtask/templates/{tool,language-pack}/Cargo.lock`):
  the API's two lines. `the_code_templates_locks_still_hold` passes.

## Using 1.5

### The test kit's stand-ins

`fenix_test_kit::api` now has every host call of API 1.5, each checked at
compile time against the API's own signature: the friendly
`key_for(command)`, `key_scheme()`, `notify_dismiss(tag)`,
`compose_with_ask(..)` and `progress(call, message, fraction)`, and
`host::key_for`, `host::editor_keys`, `host::notify_with`,
`host::notify_dismiss`, `host::compose_with_ask`, `host::git_state`,
`host::last_task`, `host::service_progress`. The harness drives the
exports 1.2 and 1.5 added: `h.key_scheme(KeyScheme::Standard)` (set in
the fake, then `key_scheme_changed`), `h.cancelled(call)`
(`service_cancelled`), `h.ask_progress(call, text)`
(`agent_ask_progress`).

**Keys.** A test says what Fenix would answer:

```rust
fake.key("gallery.open", "SPC h g")                    // with standard keys: C-k h g
    .key_in(KeyScheme::Standard, "code.goto_definition", "F12")
    .menu_key("zig.doctor", "d")                        // in SPC m only: key_for answers None, as Fenix's does
    .no_key("demo.thing");                              // nothing runs it
fake.leader_chord("C-;");                               // a user's own leader
```

So an extension using the kit's `api` in its tests calls `api::key_for`
natively. Today extensions wrap it in `#[cfg(target_arch = "wasm32")]`,
with a literal for `cargo test`, because the real call panics natively;
on the kit that guard isn't needed, and the test can check both schemes
-- see the test kit's README, "Keys of the scheme in use", and the
gallery's `src/lib.rs`. That holds only for code that calls through the
`api` name in the extension's own crate (`#[cfg(test)] use
fenix_test_kit::api;`): a call by `fenix_extension_api::` still panics.
Don't strip guards from extensions you aren't moving.

**Notifications.** `Notification::show()` is the API's method and calls
Fenix's: in code a test reaches, write
`api::host::notify_with(&notification)`, which is what `show` does, in
both builds. Then `fake.toasts()` is every one as given (summary, body,
urgency, tag, actions, progress), `fake.showing()` what Fenix would
show now (a tag replaces the one before it; a dismissed tag is gone),
`fake.dismissed()` the tags taken away. The log line is
`notification low #build: make succeeded -- 12 passed [Run it again -> build.run]`.
`fake.notifications()` stays the message line's.

**The rest**: `fake.git(Some(GitSummary { .. }))` for `git_state`;
`fake.last_task(..)`, or `on_task_tests(name, passed, failed)`, which
sets it as the task ends; `fake.progress_of(call)`; a compose pane's
`ask` payload in `fake.prompts()`. Not faked: `agents::ask` and
`ask_with`, which wait on the API's own executor.

### lang-kit's install notification

A pack gets it with no code: `Kit` gives its `Installer` the pack's
prefix. `Installer::start` still says "Downloading ZLS 0.17.0…" in the
message line. How it ended is a `Notification` through the new
`Host::notification`:

| | Summary | Body | Urgency | Tag | Action |
|---|---|---|---|---|---|
| Worked | `Installed <label>` | `Now the language server for zig.` (what its uses make it), or the folder | Low | `install.<name>` | -- |
| Failed | `Couldn't install <label>` | `Its download from <url> failed: <why>.` / `It downloaded, but unpacking it failed: ...` / `It downloaded and unpacked, but setting it up failed: ...` | Normal | `install.<name>` | *Try again*, `<prefix>.install.<name>` |

Fenix scopes a tag to the extension and drops an action whose command
isn't registered, so *Try again* is safe for a pack whose manifest
doesn't list its install command. A failure's toast is dismissed
(`Host::dismiss`) when that install starts again. `Handled::Ended`
gives the same `Err(why)` as before.

For a pack's tests:

- On **lang-kit's mock** (`features = ["mock"]`), the end of an install
  is now the line `notification low #install.<name>: Installed <label>
  -- ...` where it was `notify Installed <label>`, and `notification
  normal #install.<name>: Couldn't install ... [Try again -> ...]` where
  it was `notify Couldn't install <label>: <why>`; a retry after a failure
  adds `dismiss install.<name>` before `notify Downloading`. The mock
  keeps them in `notifications` too. No pack's tests here assert those
  lines (searched for `notify Installed`, `notify Couldn't install`,
  `notify Downloading`); a test that compares a whole list of calls
  across an install's end would see them.
- On **the test kit's `Fake`**, the same lines, and `toasts()`,
  `showing()`, `dismissed()`.
- A `Host` of a pack's own needs nothing: `notification` defaults to the
  message line (`summary: body`), `dismiss` to nothing.

lang-kit's packages page and doctor text name no leader keys (only page
keys -- `u`, `U`, `a`, `d`, `r`, Enter -- the same in both schemes), so
it has no hint to resolve; a pack's own hints are its own, built with
`api::key_for` as above.

## The 25 still to move

Every `extensions/*/Cargo.toml` that depends on a kit by path, checked
with `grep -l 'crates/\(ui\|lang\|test\)-kit"' extensions/*/Cargo.toml`
(28, less the three moved). Version and floor as they are now:

**ui-kit** (9):

| Extension | Version | `fenix` / `api` now |
|---|---|---|
| colours | 1.1.0 | >=1.0 / 1.0 |
| csv | 1.0.0 | >=1.0 / 1.0 |
| database | 1.4.0 | >=1.5 / 1.5, but builds the API from `v1.0.0` |
| dotenv | 1.1.0 | >=1.0 / 1.0 |
| hex | 1.0.1 | >=1.0 / 1.0 |
| http | 1.3.0 | >=1.5 / 1.5, but builds the API from `v1.0.0` |
| jupyter | 1.1.0 | >=1.0 / 1.0 |
| logs | 1.1.0 | >=1.1 / 1.2, builds the API from `v1.0.0` |
| parquet | 1.0.1 | >=1.0 / 1.0 |

**lang-kit** (16; protobuf uses ui-kit too): ada 1.0.0, config 1.0.0,
cpp 1.1.0, dotnet 1.1.0, go 1.2.0, haskell 1.0.0, java 1.1.0, lua 1.0.0,
ocaml 1.0.0, php 1.0.0, protobuf 1.1.0, python 1.2.0, ruby 1.0.0, rust
1.2.0, swift 1.0.0, web 1.2.0 -- all `fenix = ">=1.0, <2"`. Every one
has lang-kit's `mock` as a dev-dependency and `fenix-snippets` at
`v1.0.0`; none uses the test kit.

**test-kit**: none left (Regex, Zig and the gallery were its users).

Database and HTTP already say `api = "1.5"` and `fenix = ">=1.5"` (their
keymaps' `[[keys]]`), so moving them raises no floor: their CHANGELOG
entry needn't open "For Fenix 1.5".

### Their locks

**Every one of the 25 locks is invalid as soon as the kits move**: each
`Cargo.lock` has `fenix-extension-api` from `?tag=v1.0.0`, which the kit
no longer asks for, and `cargo metadata --locked` says "cannot update the
lock file ... because --locked was passed" in all 25 (checked). It's the
API's source that does it: each lock also lists the kits' own
dependencies (`fenix-ui-kit`: `fenix-extension-api`, `unicode-width`;
`fenix-lang-kit`: the API, ui-kit, serde, serde_json, zip, tar, flate2,
lzma-rust2), and those lists didn't change, so step 3 is all a lock
needs. The 28 locks that name a kit are exactly the 28 dependents; no
other extension's lock embeds one. CI's `--locked` steps for these 25
fail until each is moved.

### A trial run

Each of the 25 was copied to a scratch folder (outside the repository,
the kits linked in), given step 1's tag and step 3's lock, and built and
tested; nothing in `extensions/` was touched. What it said:

- **All 25 build** on `v1.5.0` after steps 1 and 3 alone, and **all 25
  pass their tests** (`cargo test --locked`) but for one test of Rust's
  that the trial itself broke: `every_snippet_reads_as_fenix_reads_it_and_none_is_rust_snippets`
  reads `../rust-snippets`, which the scratch copy didn't have. In the
  repository it's there. No pack's tests met the install notification's
  new mock lines.
- **`cargo clippy --locked --target wasm32-wasip2 -- -D warnings` is
  clean for 22.** Python, Ruby and Rust stop at Clippy's
  "redundant redefinition of a binding `p`": `let mut p = p;` right after `let mut p =
  Problem::new(..)` (`python/src/ruff.rs:140`, `ruby/src/rubocop.rs:177`,
  `rust/src/clippy.rs:101`). It's the code's, not the API's -- `Problem`
  is the same at both tags -- and the fix is to delete the line.
- `cargo update -p fenix-extension-api` swapped the API's source and
  nothing else the trial noticed; `pack`, the host `clippy` runs and
  each one's own use of 1.5 weren't tried.

## Risks

- **Until the last one moves, CI is red** for the ones not yet moved:
  `cargo test --locked` and `pack` fail at their locks. Move them in
  one change, or accept a red run per step.
- **Bundles** list members by range and say Fenix 1.0: an older Fenix
  gets the newest member that fits, which after the move is the release
  before it, for 25 more extensions.
- **The floor raise strands Fenix 1.0 to 1.4 users from these
  extensions' fixes**, as decided. Ship any fix waiting first, in a patch
  on the old floor ([CONTRIBUTING](../CONTRIBUTING.md#versions), rule 3).
- **Rule 1** (raise a floor only in a release that uses what's new) is
  met by lang-kit's notification for the packs and by a hint for pages
  that name a leader key; an extension with neither (Regex was one) gets
  a release that changes nothing for its users. That's the decision's
  cost: say so in its CHANGELOG.
- **Nothing was tried in Fenix.** The notification's look, its urgency's
  effect, *Try again*, and the gallery's foot are read from Fenix's
  source and docs, not seen.
