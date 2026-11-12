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
| 1 | The shop window | JetBrains keys, screenshots, a contributor guide | M |
| 2 | Author tooling | `xtask new` and `xtask install`, a test kit, a widget gallery | M |
| 3 | Onboarding | Tutorial lessons, the API sweep, more agent tools, a web catalogue | L |
| 4 | Tests | The test explorer, in `lang-kit` and four language packs | L |
| 5 | Breadth | Theme packs and a converter, three language packs, prose checking | L |
| 6 | Integrations | Kubernetes, Jira Cloud, GitHub Issues and Linear, more languages | L |

Milestones 1 and 2 are independent and can run side by side. 3 leans on
1 (screenshots feed the catalogue). 4 leans on 2 (the test kit). 5 and 6
lean on 2 (`xtask new`) and are otherwise free to reorder.

## Where things stand

Checked against the repository when this was written:

- 46 code or data extensions and 7 bundles in `extensions/`.
- **No `jetbrains-keys`**, in the tree or its history, though Fenix's
  `COMING_FROM.md` and `WRITING_EXTENSIONS.md` link to it as the store's
  worked example of a keymap flavour.
- **No extension has a `screenshots/` folder.** The store's details page
  shows them when they're there.
- **One tutorial lesson** in the whole store:
  `extensions/review/tutorial/pull-requests.toml`.
- **41 of 46 code extensions build against API `v1.0.0`.** Five are on
  1.0.1 to 1.3.0; none is on 1.4 or 1.5.
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
- `engine.fenix = ">=1.5.3, <2"`: the debugger's F-keys only go to a
  flavour after 1.5.2.
- A README table: IntelliJ action, its shortcut, the Fenix command, and
  a list of what has no equivalent.
- **Done when** `cargo xtask validate` passes, `SPC h K` reports nothing
  after install, and both links in Fenix's docs resolve.
- **Follow-ups**, each S: Sublime Text and Visual Studio flavours.

### 1.2 Screenshots (M)

- A first pass of twelve: HTTP, Database, Containers, Logs, Review, CI,
  CSV, Diagrams, Notebook, Regex, Colours, and one language pack
  (Rust: the package page and a debug session).
- A house style in the contributor guide: one theme, one window size,
  PNG, under 2 MB, up to three per extension, the first one the hero.
- Then the rest, a batch per release.
- **Risk**: they need a real Fenix window, so they're taken by hand.
  Worth checking whether Fenix's control channel can script it; if it
  can, an `xtask screenshots` follows in milestone 3.
- **Done when** every extension in the first pass shows a picture on its
  store page, and `validate` checks size and format (it may already).

### 1.3 A contributor guide (S)

`CONTRIBUTING.md`, and a pull request template.

- How to submit: a folder under `extensions/`, what review looks at,
  who tags the release.
- Publisher ids: who may use `fenix.`, how to pick another.
- A permissions checklist for reviewers: the narrowest `process`,
  `network` and `sockets` that work, no `network = ["*"]` without a
  reason in the README, secrets by key.
- What every extension carries: README, CHANGELOG, a screenshot, tests
  where it has code.
- **Open question for the maintainer**: do outside extensions live in
  this repository, or may the index point at packages built elsewhere?
  The guide should say which.

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

- First: HTTP, Database, Containers, Notebook, and one shared lesson
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
- **Check first**: an extension built for API 1.5 needs
  `fenix = ">=1.5"`. Confirm how the store treats someone on an older
  Fenix (keeps the last release that fits, or shows nothing) before
  raising any floor. If it's the second, sweep only at the next minor
  where the floor moves anyway.

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

### 3.4 A web catalogue (M)

A static page generated by `xtask` from the registry's `index.json`:
every extension with its description, categories, permissions, hero
screenshot and changelog, searchable. Published from this repository
with GitHub Pages by a step in `release.yml`.

- **Needs from the maintainer**: Pages switched on for the repository.
- Linking to it from Fenix's README or site is a change to Fenix, so
  it's out of scope here; the page stands on its own URL.

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

### 6.2 Jira Cloud (M)

The Jira extension describes itself as for Server and Data Center.
Cloud differs in sign-in (email and API token, or OAuth), the REST
version and rich-text fields. The work is in `crates/jira`, behind the
client the extension and the agenda already share.

### 6.3 GitHub Issues and Linear in the agenda (L)

- First, a small tracker service the agenda calls, so Jira stops being a
  special case. This is the design step; do it before 6.2 if both are
  planned, so Cloud lands on the new seam.
- Then one extension per tracker that serves it.

### 6.4 Language packs, second wave (M each)

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

## Open questions

1. Outside extensions: in this repository, or packages built elsewhere?
   (1.3)
2. What does the store show an older Fenix when an extension's newest
   release needs a newer one? (3.2)
3. Can the control channel drive Fenix well enough to script
   screenshots? (1.2)
4. GitHub Pages for this repository: yes or no? (3.4)
5. Which trackers after Jira Cloud matter to the people using the
   agenda today? (6.3)
