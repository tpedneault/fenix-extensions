# The store: plan and roadmap

> **Draft for the maintainer to edit.** Put together in November 2026
> from this repository and Fenix's docs at 1.5. The order is a
> suggestion and nothing has a date. Sizes are relative: **S** is a day
> or two, **M** about a week, **L** several weeks.

What this is for: making Fenix nicer to work in, and more tempting to
try, **without changing Fenix**. Everything here is an extension, a
shared crate, `xtask`, CI or a document in this repository.

Three threads run through it:

- **Polish**: what the store already has, shown and taught better.
- **Breadth**: the extensions someone switching editors looks for first.
- **Authors**: making it easy for other people to write extensions.

## At a glance

| Milestone | Theme | What ships | Size |
|---|---|---|---|
| 1 | The shop window | JetBrains keys, scripted screenshots, the web catalogue, external packages in the index, a contributor guide | L |
| 2 | Author tooling | `xtask new` and `xtask install`, a test kit, a widget gallery | M |
| 3 | Onboarding | Tutorial lessons, the API sweep, more agent tools | M |
| 4 | Tests | The test explorer, in `lang-kit` and four language packs | L |
| 5 | Breadth | Theme packs and a converter, three language packs, prose checking | L |
| 6 | Integrations | Kubernetes, Jira Cloud, GitHub and GitLab Issues, Linear, more languages | L |

Milestones 1 and 2 are independent and can run side by side. 4 leans on
2 (the test kit). 5 and 6 lean on 2 (`xtask new`) and are otherwise free
to reorder.

## Decided

The maintainer's answers (12 November 2026) to what the first draft
left open:

1. **The index may point at packages built elsewhere.** An outside
   extension stays in its author's repository; this one lists it (1.4).
2. **An older Fenix is told nothing** about a release it can't run: it
   keeps the last one that fits. So raising an extension's floor strands
   nobody, but those users stop getting its fixes (3.2).
3. **The control channel drives Fenix well enough to script
   screenshots** (1.2).
4. **GitHub Pages: yes, and on.** The `gh-pages` branch holds a
   placeholder page and Pages serves it (1.5).
5. **Trackers**: Jira Cloud, then GitHub Issues and the others (6.2 to
   6.4).
6. **The keybindings branch is merged**, and this plan sits on top of
   it (13 November).
7. **Publisher ids** were left to this plan's recommendation: tied to a
   GitHub owner, below (1.4).

## Milestone 1: built, not yet proven

Built on 13 November and on `main` since pull request #27. What
each item still needs before it counts as done:

| Item | Built | Still needed |
|---|---|---|
| 1.1 JetBrains keys | The extension: 70 bindings, 7 unbinds, checked against Fenix's source and `validate` | Trying in a 1.6 nightly; then its release |
| 1.2 Screenshots | `cargo xtask screenshots`, scripts for CSV, Logs, Hex, HTTP, Colours, Data tools | A first run on Windows (`screenshots/README.md` lists the steps); see the limit below |
| 1.3 Contributor guide | `CONTRIBUTING.md`, the pull request template | Nothing |
| 1.4 External packages | `external.toml`, `publishers.toml`, `external-check`, `index-add-external`, four workflows | The workflows' first runs; a test extension in another repository |
| 1.5 Catalogue | `cargo xtask site`, the job in `release.yml`, `site.yml` | The workflow's first push to `gh-pages` |

**The limit on screenshots.** The control channel runs only commands an
extension marks `external = true`. On 13 November the commands that open
a page of their own were marked in 21 extensions (a minor release of
each), and scripts added for Regex, Env files, the Colours palette and
the HTTP client: 8 extensions and 12 shots are scripted. Still out of
reach: pages that start from the file in front of you (a diagram's
preview, a log, a hex dump opened by command), which Fenix's rule for
`external` leaves out, and anything needing a service.

**Noticed on the way, not fixed:**

- Fenix's installer doesn't compare a downloaded package's own id and
  version with the listing it came from. `xtask` now guarantees they
  match for everything it writes to the index; Fenix checking too would
  be defence in depth. An issue for Fenix.
- `extensions/http/extension.wasm` is tracked in Git.
- HTTP, Database, Notebook, MIB and Tcl say `api = "1.5"` with a `fenix`
  floor below 1.5. Fenix checks `api` on its own, so nothing breaks; the
  floors are just out of step with 3.2's rule.
- `validate` accepts `runtime = "lua"`, which Fenix can't run yet.

## Milestone 2: built, not yet proven

Built on 13 November, on `claude/pensive-carson-xu7nwb`, not yet on
`main`.

| Item | Built | Still needed |
|---|---|---|
| 2.1 `xtask new` | Five kinds (theme, keymap, snippets, language-pack, tool); each validates, packs and tests as scaffolded | A scaffold opened in Fenix |
| 2.2 `xtask install` | Pack and `fenix --install-extension`, with `--watch` | A run against a real Fenix, and on Windows |
| 2.3 Test kit | `crates/test-kit`: a stand-in for the API in tests, a fake host with canned answers, pages and canvases as text, snapshots; Regex and Zig converted | More extensions moved onto it |
| 2.4 Widget gallery | `extensions/gallery`: ten live sections, each with its code a key away; three `ui-kit` fixes it found | Opening it in Fenix; its release |

What it turned up:

- **The test kit is for API 1.0.** The kits and 39 of 47 code extensions
  build against `v1.0.0`, and a type is the kit's own only from the same
  tag; Cargo won't patch one tag with another. An extension on 1.1 to
  1.5 can't use it yet. Either the kits move up together (with 3.2's
  sweep), or Fenix's API crate gains a seam: on a native target, its
  host calls going through something a test can set, where today each
  is `unreachable!()`. An issue for Fenix.
- **Methods on API types aren't faked** (`Command::spawn`,
  `HttpRequest::send`): a test needs the `api::host::` form of the call.
- **`xtask new <kind> <name>` makes one for this repository**,
  `fenix.<name>`; `publisher.name` starts one its author hosts.
- **The gallery keeps its own small canvas-to-text helper**
  (`tests/support/mod.rs`), written before the test kit landed. Moving
  it onto `fenix_test_kit::render` is a small follow-up.
- **The stretch, `cargo xtask smoke`**, wasn't built: Fenix's headless
  worker looks usable, at the cost of building wasmtime and much of
  Fenix in CI.

## Where things stand

Checked against the repository when this was written:

- 49 extensions (47 with code, 2 of data only) and 8 bundles in
  `extensions/`.
- **No `jetbrains-keys`**, on `main`, on any branch or in the registry's
  index, though Fenix's `COMING_FROM.md`, `WRITING_EXTENSIONS.md` and
  changelog all name it as the store's worked example of a keymap
  flavour.
- **The keybindings branch is merged under this plan** (Jupyter
  notebooks, a Python bundle, and standard keys for Fenix 1.5 in HTTP,
  Database, Notebook, MIB and Tcl). MIB, Notebook and Tcl built the
  extension API from a branch of Fenix; they now build it from the
  `v1.5.0` tag. All seven were released on 12 November: Jupyter 1.0.0,
  the Python bundle 1.0.0, HTTP 1.1.0, Database 1.2.0, Notebook 1.1.0,
  MIB 1.3.0 and Tcl 1.1.0.
- **The index already carries a `url` and a `sha256` per release**, and
  Fenix checks the hash. Nothing in its format ties a package to this
  repository; what's missing is the way in.
- **No extension has a `screenshots/` folder.** The store's details page
  shows them when they're there.
- **One tutorial lesson** in the whole store:
  `extensions/review/tutorial/pull-requests.toml`.
- **39 of 47 code extensions build against API `v1.0.0`.** Five are on
  1.0.1 to 1.3.0, and three (MIB, Notebook, Tcl) on 1.5, where their
  hints already name the key for the scheme in use (`key_for`).
- **8 extensions mention agents in their manifest** (agent-context,
  ccsds, ci, database, mib, nats, pets, review). Logs, Containers and
  HTTP give agents nothing yet.
- **One theme extension** (`original-light-themes`); Fenix ships 29
  themes itself.
- **No pack** for Terraform/HCL, Elixir, Dart/Flutter, Scala, Nix,
  LaTeX or Typst.
- `xtask` has `validate`, `build`, `pack`, `for-tag` and `index-add`.
  Nothing scaffolds a new extension or installs one in one step.
- `crates/ui-kit` and `crates/lang-kit` each have a README. `lang-kit`
  has a `Host` trait its own tests fake; there's no shared test kit for
  extensions that aren't language packs.
- No `CONTRIBUTING.md`. The README says how the maintainer releases,
  not how someone else submits.

---

## Milestone 1: the shop window

The cheapest work with the most visible effect. Someone opening the
store for the first time should see pictures, and the docs should stop
pointing at something that isn't there.

### 1.1 JetBrains keys (S)

A data-only keymap flavour, `extensions/jetbrains-keys`.

- `keymaps/jetbrains.toml`: `[[keys]]` entries with
  `scheme = "standard"`, only where IntelliJ's defaults differ from
  Fenix's (which are VS Code's). `unbind = true` for IntelliJ chords
  Fenix has no command for.
- `engine.fenix = ">=1.6.0, <2"`: the debugger's F-keys only go to a
  flavour after 1.5.2, and a nightly counts as 1.6.0, so it can be tried
  there before 1.6 ships.
- A README table: IntelliJ action, its shortcut, the Fenix command, and
  a list of what has no equivalent.
- **Done when** `cargo xtask validate` passes, `SPC h K` reports nothing
  after install, and both links in Fenix's docs resolve.
- **Follow-ups**, each S: Sublime Text and Visual Studio flavours.

### 1.2 Scripted screenshots (M)

`cargo xtask screenshots [extension]`, driving a running Fenix over the
control channel so every picture can be retaken after a theme or layout
change.

- Each extension gets a `screenshots.toml`: for each picture, a sample
  project to open (`fenix/openProject`), the commands to run
  (`fenix/runCommand`), what `fenix/state` should say before the shot,
  and the file name.
- `xtask` installs the packed extension into a clean profile, starts
  Fenix at a fixed window size and theme, runs the steps and captures
  the window with the system's own tool. The control channel has no
  capture of its own, so that last step is per platform: Windows first.
- Sample projects live under `screenshots/samples/` at the top of the
  repository and aren't packed.
- A house style in the contributor guide: one theme, one window size,
  PNG, under 2 MB, up to three per extension, the first one the hero.
- First pass, twelve: HTTP, Database, Containers, Logs, Review, CI,
  CSV, Diagrams, Jupyter, Regex, Colours, and Rust (the package page
  and a debug session). Then the rest, a batch per release.
- Pages that need a service (Containers, Review, CI, Database) use a
  local stand-in or a recorded sample; say which in the script.
- **Done when** the twelve show a picture on their store page, the
  command retakes all of them unattended on Windows, and `validate`
  checks size and format.

### 1.3 A contributor guide (S)

`CONTRIBUTING.md`, and a pull request template.

- Two ways in: a folder under `extensions/` (built and released here),
  or an entry in `external.toml` for a package built elsewhere (1.4).
- Publisher ids: `fenix.` is this repository's; anyone else claims
  their own with their first submission, tied to their GitHub account
  or organisation (1.4).
- A permissions checklist for reviewers: the narrowest `process`,
  `network` and `sockets` that work, no `network = ["*"]` without a
  reason in the README, secrets by key.
- What every extension carries: README, CHANGELOG, a screenshot, tests
  where it has code.

### 1.4 External packages in the index (M)

An outside author builds and hosts their own `.fenixext`; the registry
lists it. The index needs no new field for this.

- `external.toml` on `main`: one entry per release -- `id`, `version`,
  `url`, `sha256`, `repository`. A submission is a pull request adding
  an entry.
- `cargo xtask index-add-external`: downloads the package, checks the
  hash, unpacks it, runs the same checks `validate` runs on a folder
  here, and writes the index entry **from the manifest inside the
  package**, never from what the pull request claims: name, permissions,
  `fenix`, `api`, dependencies.
- CI runs it on any pull request touching `external.toml`, and prints
  the permissions for the reviewer. `release.yml` adds the entries and
  signs, as it does for the rest.
- **Publishers.** `publishers.toml` maps each publisher id to the
  GitHub owners (a user or an organisation) that may release under it.
  The first accepted submission claims the id, first come; from then on
  a release under it must be a release asset of a repository one of its
  owners owns, which `xtask` checks from the URL. That proves who a
  publisher is with nothing to set up, and stops a second party
  shipping under someone else's name. Changing owners is a pull request
  the current owner approves. Domain proof isn't worth its cost yet.
- Other rules it enforces: an https URL that stays put (a release
  asset, not a branch), a publisher that isn't `fenix.`, a version not
  already listed, a size limit.
- A reusable workflow authors can call from their own repository to
  build, pack and attach a `.fenixext` to a release, printing the entry
  to submit.
- **Risk**: a URL can die. A scheduled job checks each one still
  answers with its hash and opens an issue when it doesn't.
- **Done when** a test extension in another repository is installed
  from the store through an `external.toml` entry.

### 1.5 The web catalogue (M)

A static site generated by `xtask` from the registry's `index.json`:
every extension with its description, categories, permissions, hero
screenshot, changelog and the Fenix it needs, searchable. External ones
are marked as such, with their repository.

- `cargo xtask site <index.json> <out>`; a step at the end of
  `release.yml` pushes the result to `gh-pages`, which exists with a
  placeholder.
- Pages is on for `gh-pages`. `release.yml` already has
  `contents: write`, which is what pushing there takes.
- It ships useful without pictures and gets better as 1.2 lands.
- Linking to it from Fenix's README or site is a change to Fenix, so
  it's out of scope here; the page stands on its own URL.

---

## Milestone 2: author tooling

### 2.1 `cargo xtask new` (M)

`cargo xtask new <kind> <publisher.name>`, kinds `theme`, `keymap`,
`snippets`, `language-pack` and `tool`.

- Each writes a folder that passes `validate` and packs as it is.
- `language-pack` starts from the smallest real pack and wires
  `lang-kit`; `tool` gives a command and a canvas page built with
  `ui-kit`.
- Tests in `xtask`: each kind scaffolds, validates and packs.

### 2.2 `cargo xtask install` (S)

Pack, then `fenix --install-extension`, in one step on Windows and
Linux. Fenix's own template packs with `package.ps1`, which needs
PowerShell; this is the same loop without it. `--watch` rebuilds on
change, as a stretch.

### 2.3 A test kit (M)

`crates/test-kit`: a fake host for any extension, grown from the one
`lang-kit`'s tests use.

- Feed `handle` events, commands and keys; record what it asked the
  host for.
- Render a `Page` or a `ui-kit` canvas to plain text, to snapshot.
- Canned answers for HTTP, processes and services.
- Convert two existing extensions to it (one page-based, one language
  pack) as proof.
- **Stretch**: `cargo xtask smoke <extension>` loading the packed
  `extension.wasm` in Fenix's headless `fenix-wasm` worker, as the MIB
  1.2 release did by hand. It needs Fenix's crate as a git dependency of
  `xtask`, so check build time in CI before committing to it.

### 2.4 A widget gallery (S)

`extensions/gallery`, category `tools`: one page showing every `ui-kit`
widget (table, tree, tabs, fields, splits, scroll, popups) with the code
for each a key away. It doubles as `ui-kit`'s visual regression test
through the test kit. Both kit READMEs get a "start here" section that
points at it and at `xtask new`.

---

## Milestone 3: onboarding

### 3.1 Tutorial lessons (M)

Lessons in `tutorial/*.toml`, the format Review already uses.

- First: HTTP, Database, Containers, Notebook, Jupyter, and one shared lesson
  shape for language packs (install the server from the doctor, run a
  task, start a debug session) filled in for Rust, Python, Web and Go.
- A lesson template in the contributor guide.
- **Done when** each has a lesson that completes end to end in a clean
  profile.

### 3.2 The API sweep (M, spread over releases)

Move extensions to the newest API where it buys something, not for its
own sake.

- **Key hints** (`key_for`, API 1.5): any hint that names a Vim key
  reads wrong to someone on standard keys. Grep for hard-coded `gd`,
  `K`, `SPC` in page text and replace.
- **Notifications** (1.3): long work that ends while you're elsewhere --
  a container build, a test run, a download, an HTTP runner.
- **Agent-assisted compose** (1.4): Review's descriptions already; the
  HTTP client's request notes and the agenda's task text are candidates.
- **Floors**: an extension built for API 1.5 needs `fenix = ">=1.5"`.
  An older Fenix isn't told about that release and keeps the last one
  it can run, so nothing breaks, but those users get no more fixes for
  it. So: raise a floor only in a minor release that uses what the
  newer API adds, say so at the top of its CHANGELOG entry, and ship any
  pending fixes in a patch release on the old floor first.
- **Started**: MIB, Notebook and Tcl are done, and are the pattern to
  follow. HTTP and Database have their standard-key chords but still
  build against an older API.

### 3.3 More agent tools (M)

Database already has `database_schema` and `database_query`. The same
shape for:

- **Logs**: entries by level, time range and pattern from an open log.
- **Containers**: list, state, and the tail of a container's logs.
  Read-only; nothing that starts or stops.
- **HTTP**: run a named request from the project's `.http` files and
  return status, headers and body.

Each within the limits (description 300 characters, schema 2 KiB), each
with a tool class so Fenix asks before anything that writes.

---

## Milestone 4: the test explorer

The largest gap for someone coming from VS Code or JetBrains, and the
one that makes every language pack better at once.

### 4.1 Design (S)

A short design note before code. What the API gives:

| Need | How |
|---|---|
| Find tests | The runner's own listing: `cargo test -- --list`, `pytest --collect-only -q`, `go test -list`, `vitest list`, `jest --listTests` |
| Run one, a file, all, the failed | `run_in_background`, at most 4 at once; `Event::TaskFinished` |
| Results | The runner's machine output (JUnit XML or JSON) parsed in the extension |
| Show them in the file | `decorations::set`: a mark after each test's line |
| Failures | `diagnostics::set`, with the assertion's message at its line |
| Debug a test | `debug::launches`, one per test under the cursor |
| Tell you | A notification when a run ends; the count on the modeline (`status`) |

Known limits to design around: no gutter marks (the result sits after
the line instead), and an extension can't ask another's language server
for symbols, so discovery is the runner's.

### 4.2 `lang-kit::tests` (L)

A `TestRunner` trait beside `PackageManager`: list, run, parse. The kit
owns the page (a tree by file and suite, filter, re-run failed, jump to
source), the decorations, the problems and the keys under `SPC m`.

### 4.3 Four packs (M)

Rust, Python, Go and Web (Vitest and Jest) implement the trait. Each a
minor release with a tutorial lesson and a screenshot.

### 4.4 The rest (S each)

Java, .NET, C/C++ (CTest), Ruby, PHP, and the others as their owners
want them.

- **Done when** in each of the four: tests are listed without running
  them, one test runs from its line, a failure shows at its assertion,
  and the failed set re-runs with one key.

---

## Milestone 5: breadth

### 5.1 Themes (M)

- `cargo xtask theme-import <vscode-theme.json>`: a VS Code colour theme
  to Fenix's theme TOML, with a report of what it couldn't map.
- Two or three theme packs built with it, each data-only. Every theme's
  licence checked and kept in the pack; only permissively licensed ones.
- Each pack names its themes so Fenix's settings importer can match
  `workbench.colorTheme` to them when installed.

### 5.2 Language packs, first wave (M each)

Terraform/HCL (terraform-ls, `terraform fmt`, validate, plan as a
task), Elixir (its grammar, a language server, `mix` tasks, ExUnit
through 4.2), Dart and Flutter (the analysis server, `dart format`,
devices and hot reload as tasks). Each from `xtask new language-pack`:
grammar, server with a doctor fix that installs it, formatter, linter,
tasks, snippets, a project template, a lesson.

### 5.3 Prose and spelling (S)

`extensions/prose`: an existing language server for typos and grammar,
downloaded into `/tools` with a checksum by `lang-kit`'s installer and
added beside the others with `project::add_language_server`, so its
fixes come through `SPC c a`. Settings for which languages it reads and
a project word list.

### 5.4 Snippet import (S)

Language packs carry their own snippets already; `rust-snippets` is the
only standalone pack. `cargo xtask snippets-import` converts VS Code
snippet JSON to Fenix's format, to top up thin packs. Licence checked
per source.

---

## Milestone 6: integrations

### 6.1 Kubernetes (L)

A sibling to Containers, driving `kubectl`: contexts and namespaces,
workloads and pods in a tree, live logs, a shell in a pod
(`run_in_terminal`), port-forwards kept as background tasks, a
resource's YAML in a buffer. Read-only agent tools. Joins the Lab
bundle.

### 6.2 A tracker seam in the agenda (M)

First, so that Jira stops being a special case: a small service the
agenda calls for "the issues assigned to me", "this issue" and "move it
on", which any tracker extension can serve. Jira Server and Data Center
move onto it with no change in behaviour. The Jira bundle keeps working
as it is.

### 6.3 Jira Cloud (M)

The Jira extension is for Server and Data Center. Cloud differs in
sign-in (email and API token, or OAuth through `socket::listen`), the
REST version and rich-text fields. The work is in `crates/jira`, behind
the client the extension and the agenda already share; one extension,
both kinds, told apart by the site's address.

### 6.4 GitHub Issues, GitLab Issues, Linear (M each)

One extension per tracker, each serving 6.2's seam and bringing its own
page: searches, an issue with its comments, a new-issue form, a branch
started from an issue.

- **GitHub Issues** first, then **GitLab Issues**: both reuse
  `crates/forge` and the sign-in Review already has, so someone using
  Review has nothing more to set up.
- **Linear** after: its own API and token.
- Agent tools on each: read an issue, list mine.

### 6.5 Language packs, second wave (M each)

Scala, Nix, LaTeX and Typst (with a PDF preview through the PDF
reader's service, if it has one to offer).

---

## What this plan leaves out

Things that came up and need Fenix itself, so they aren't here: a
minimap, inline AI completion, an undo tree, gutter marks for
extensions, and publishing `fenix-extension-api` to crates.io. If
milestone 4 shows gutter marks matter, that's an issue to open on Fenix
with the test explorer as the case for it.

## How to tell it's working

- Every extension in the store has a screenshot and, where it has pages
  or a workflow, a lesson.
- A new language pack goes from `xtask new` to a pull request in a day.
- The first extension from someone other than the maintainer is merged.
- No link in Fenix's docs points at a store extension that isn't there.

## Still open

Nothing. Decided on 13 November: page-opening commands are to be marked
`external = true`, extension by extension, so screenshots can reach
them; and `Ctrl+T` stays in JetBrains keys.
