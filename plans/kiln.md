# Kiln for Fenix (`fenix.kiln`)

A plan, not yet approved. Kiln (https://github.com/tpedneault/kiln) keeps a
project's requirements, decisions, milestones, tasks, tests and risks as
Markdown in the repository. This extension makes a Kiln project work in
Fenix without setup, and puts Kiln's plan pages, edits, exports and agent
tools inside the editor.

## Decisions taken

| Question | Answer |
|---|---|
| Where it lives | `extensions/kiln` here, id `fenix.kiln`, on `ui-kit` and `test-kit` |
| How it reads and writes a project | It runs the `kiln` program: `--format json` to read, `set`/`link`/`unlink`/`new`/`renumber` to write. Nothing of Kiln is compiled in, and it never writes an item file itself |
| Scope of 1.0 | Setup and editing glue, the plan pages, exports with Confluence and Jira, agent tools |
| Agents | Read tools by default; edit tools that Fenix asks about; no publishing tools |
| Extras in | ID decorations, agenda clock-in, a Home section and a check notification, a tutorial lesson and screenshots |
| Kiln not installed | The doctor says so and how to install it; nothing is downloaded, no network permission |
| Repositories changed | fenix-extensions and kiln. Fenix is not touched: what it lacks is a G-item in Kiln's INTEGRATION.md and a visible workaround |
| Delivery | A commit set per milestone on `claude/pensive-meitner-yd5ok5`, gated and pushed; one pull request per repository at the end |

## Shape

```text
extensions/kiln/
  extension.toml      fenix >=1.6 (project kinds), api 1.5 (the kits')
  src/lib.rs          Extension: commands, events, pages, services
  src/kiln.rs         the one place that runs kiln: a queue of jobs, each a
                      Command, its output gathered, parsed, answered
  src/model.rs        what kiln's JSON says, as types; kiln_json checked
  src/project.rs      the Kiln project here: found, its version, its state
                      (items, status, findings), reloaded
  src/pages/*.rs      one module per page, pure: state + model -> ui-kit
  src/edit.rs         the edits as flows of picks and prompts -> one kiln call
  src/agent.rs        the agent tools' services
  src/decor.rs        IDs in a buffer -> decorations
  fixtures/           kiln's real output, recorded from two projects
  keymaps/, tutorial/, screenshots/, README.md, CHANGELOG.md
```

Permissions, and why: `process = ["kiln", "${setting:kiln.path}"]` (all it
does), `filesystem = "project-read"` (`kiln.toml`, `.fenix/tools.json`),
`editor = "read"` (the ID under the cursor, decorations). No network, no
secrets: Confluence and Jira tokens stay Kiln's, in the OS credential
store, and `kiln auth …` runs in a terminal pane.

Rules every task follows:

- A call returns in milliseconds: kiln runs through `spawn`, its answer
  comes as events, the page shows the last state meanwhile.
- One reload at a time, and one more queued at most; a reload is `kiln
  list`, `kiln status` and `kiln check`, after a save in the project, a
  new git head, a kiln edit, `r`, and every few seconds only while a page
  is in view.
- A kiln error is shown as kiln wrote it (its one `kiln: …` line).
- A `kiln_json` other than 1, or a kiln older than the minimum, disables
  the pages with the reason; nothing guesses at a shape.
- Pure logic is tested natively against `fixtures/`; pages are snapshot
  as text with test-kit. `cargo xtask validate`, `cargo test`, `cargo
  clippy -- -D warnings` and `cargo xtask pack extensions/kiln` pass at
  the end of every milestone.

## Milestones and tasks

Model: **O** Opus (design-heavy or safety-relevant), **S** Sonnet
(mechanical, or follows a pattern set by an earlier task). Tasks in a
milestone run in the order given unless marked parallel (∥).

### E0. A Kiln project works when opened

| # | Task | Model |
|---|---|---|
| E0.1 | Scaffold with `cargo xtask new tool kiln`; the manifest (activation on `kiln.toml` and `kiln.*` commands, permissions, settings `kiln.path`, `kiln.refresh_seconds`, a `kiln` project kind with tag `KILN`, an `SPC m` context) | S |
| E0.2 | `kiln.rs`, `model.rs`, `project.rs`: the job queue, typed JSON for `list`, `status`, `check`, `show`, version and `kiln_json` checks, the reload rules above; fixtures recorded from Kiln's own repository and a small demo project by a committed script | O |
| E0.3 | The language server: `project::add_language_server("kiln", …)` for Markdown and the code languages, skipped when `.fenix/tools.json` already lists kiln (no second server). Needs the languages from Kiln: task K.1 | O |
| E0.4 ∥ | The doctor: kiln found and its version, `kiln.toml` readable, the language server; when missing, the row says how to install it and `kiln.path` | S |
| E0.5 ∥ | Tasks (`kiln check`, `kiln export --check`, `kiln export`), and the modeline item: errors and warnings from the last check, a command to open Check | S |
| K.1 (kiln) | A read-only way to ask which languages `kiln lsp` serves in a project (what `init --fenix` computes), as JSON; CLI.md, CHANGELOG | S |

### E1. The plan pages, read-only

| # | Task | Model |
|---|---|---|
| E1.1 | The page frame: one page kind with tabs (as Kiln's GUI orders them), a `/` filter, the cursor kept on the same ID across reloads, `Enter` opens the item's file at its line, `page_state`/`page_restore`, keys shown with `key_for` | O |
| E1.2 | Tree and Board (ready, waiting on, owner; closed hidden until `a`) | S |
| E1.3 ∥ | Roadmap and Decisions | S |
| E1.4 ∥ | Trace (gaps, `m`, type cycling, levels) and Impact | S |
| E1.5 ∥ | Item (what `kiln show` gives, `v` to follow, back and forward) and Check (findings by file, `Enter` to the line) | S |
| E1.6 ∥ | Changes (`kiln diff` against the latest tag, `b` picks another revision) | S |
| E1.7 | Commands from a buffer: "Kiln: show the item under the cursor", "Kiln: impact of…", a picker over every item (`kiln.find`) | S |

### E2. Editing

| # | Task | Model |
|---|---|---|
| E2.1 | `edit.rs`: the flows (status, title, parent, owner, due, link, unlink, new item, new child, renumber) as picks and prompts whose choices come from the loaded project, each ending in one kiln call; the outcome line shown; kiln's refusal (a file changed on disk, a value not allowed) shown and the flow left open | O |
| E2.2 | The keys on every page (`s`, `H`/`L` on the Board, `l`, `U`, `n`, `N`, `SPC m` entries), the same flows as commands on the item under the cursor in a buffer; a new item opens in Fenix | S |

### E3. Exports, Confluence and Jira

| # | Task | Model |
|---|---|---|
| E3.1 | Exports: `kiln export --dry-run --format json` per target on a background job, read when the page is first shown and on `R`; `w`/`W` write file targets | O |
| E3.2 | Publishing: `P` on a Confluence target shows the plan and takes the target's name typed before `kiln export NAME --yes` runs; a refused plan publishes nothing and says why; `kiln auth confluence` in a terminal when there is no token. Never `--force`, `--adopt` or `--prune-removed` | O |
| E3.3 ∥ | Comments (`kiln confluence comments`, `Enter` opens the browser) | S |
| E3.4 ∥ | Jira: tickets on item rows (`list --jira`, off the reload path), `kiln jira sync --dry-run` as a target, sync after confirmation, `jira pull` | S |

### E4. Agent tools

| # | Task | Model |
|---|---|---|
| E4.1 | Read tools, offered by default: `kiln_next`, `kiln_show`, `kiln_list`, `kiln_status`, `kiln_trace`, `kiln_impact`, `kiln_check`; answers cut to a size with how to ask for the rest; item bodies marked as data, not instructions | O |
| E4.2 | Edit tools, asked about: `kiln_set` (status, owner, due, title), `kiln_link`, `kiln_new`. Class `run`, not `edit`: kiln writes the files, so the change does not pass through Agent changes, and the class must say what happens. A setting turns them off | O |

### E5. Extras

| # | Task | Model |
|---|---|---|
| E5.1 | ID decorations: title and status after an ID in item files and in code references, from the loaded project, for the lines in view; a setting to turn them off | O |
| E5.2 ∥ | "Start working on this": status to the type's in-progress status, then the agenda's task for it (by Jira key, else made with `agenda.add_task`) clocked in; without the Agenda extension, only the status | S |
| E5.3 ∥ | A Home section with the ready work, and one notification when a save takes `kiln check` from clean to errors | S |

### E6. Ready for the store

| # | Task | Model |
|---|---|---|
| E6.1 | README (what it does, what it may do and why, every key), CHANGELOG 1.0.0, the root README's table, a tutorial lesson, `screenshots.toml` | S |
| K.2 (kiln) | INTEGRATION.md gains "The Fenix extension" (what it uses of Kiln, what stays `kiln init --fenix`'s, any gap found as a G-item); `init --fenix` says the extension does this when it is installed; the wiki's Fenix page; CHANGELOG | S |
| E6.2 | A review of the whole extension against CONTRIBUTING.md's reviewer's checklist by a fresh reviewer, fixes, then the two pull requests | O |

## How the hand-off runs

Each task goes to one subagent with: this plan, the files to read first
(WRITING_EXTENSIONS.md's sections it needs, the kit's README, Kiln's CLI.md
section), its exact scope, and the gate it must pass. I review each
result against the code before the next task starts, run the gates
myself at the end of each milestone, commit with subjects in the
repositories' style (`Kiln: …`), and push. Parallel tasks get separate
files to own so they don't collide.

## Needs checking (only Thomas can)

Nothing here runs Fenix: every page is tested as text against a fake
host, and kiln's answers are its real output, recorded from kiln 1.0.0
(or hand-written from its source, `tests/fixtures/hand/`). To be checked
by hand, listed again in the pull request with steps. As it stands after
E6.1:

**Fenix on Windows, and both key schemes**

- activation on a `kiln.toml` and on a `kiln.*` command; the `KILN` project
  kind in the hub, on Home and in the modeline (a Rust project with a
  `kiln.toml` stays `RS`);
- the pages' look and every key in Vim keys and in standard keys
  (`Ctrl+K m …`); the mouse; Ctrl+O and Ctrl+I on the Item tab where Fenix
  lets them through, and `<` `>` where it doesn't; `SPC m` and its keys.

**The language server**

- registered by the extension, with kiln 1.0.0 for Markdown only (completion,
  hover, `gd`, `gr`, rename preview, diagnostics, code actions);
- no second one in a project that already ran `kiln init --fenix`, and taking
  over when the `.fenix/tools.json` entry is removed;
- K.1 (`kiln lsp --languages`), written but never run against a kiln that has
  it: the code languages once a kiln does.

**Tasks and the quickfix**

- `kiln: check`'s lines as quickfix entries; the two export tasks naming file
  targets only; the doctor's rows and its fix rows; the modeline's counts.

**The pages and their keys**

- every tab on a real project, with kiln found, missing, too old, and in a
  folder with no `kiln.toml`; the cursor kept across a reload; the session
  restoring tabs, cursors and filters; two plan pages in two panes;
- how large a project reads and draws, and how quickly.

**Edits and an open buffer**

- every edit on a real project, and what Fenix does with an item's file open
  in a buffer when kiln rewrites it (re-read in place; `[disk]` with unsaved
  edits); a refusal ("changed on disk") and the asking again; a renumber
  across many files, now allowed ten minutes.

**Exports, and a real Confluence**

- the dry run of a Confluence target against Cloud and Data Center; a
  publish from `P`, its refusal and its exit 3; `kiln auth confluence` in a
  terminal pane; the Comments tab and `o` opening the browser; every
  **verify** in Kiln's CONFLUENCE.md (Kiln's own list).

**Jira**

- tickets on the rows against Server or Data Center, with and without a
  token (`A`); `J`; the `jira` target's dry run and a sync, including the
  files that gain ticket keys and exit 3.

**Agents**

- the toolset offered in a real agent thread; the default rules allowing the
  reads and asking about the edits, `y`/`Y`/`n`; `fenix.kiln.agent_edits`
  off, then on; the change not in Agent changes; a sign-off status refused;
  a cancelled call.

**Decorations**

- titles after IDs in item files and code, drawn beside the text, moving as
  you type, following the view, and gone when kiln is lost.

**The agenda**

- "start working on this" with the Agenda extension installed (a task found
  by its Jira key, by its title, made) and without it; a running clock
  closed into a time entry.

**Home and the notification**

- the Kiln section on Home and picking a row; the notification after a save
  that breaks the project, its action, its replacement and its dismissal.

**The tutorial and the screenshots**

- the lesson `kiln.plan` played through, with kiln and without (does `s`
  skip every step that needs it; do the picker steps pass with the plan in
  front?);
- the screenshots, which need Fenix and kiln to take: only the first (titles
  after IDs) is a script today. The plan's pages need `kiln.open` and
  `kiln.findings` marked `external = true`, and a way for the control
  channel to choose a tab (the Board, the Roadmap, Trace and Exports).

## Left out on purpose

Downloading Kiln, Kiln's Home (Fenix has its own projects page), the
Fenix gaps G4 and G7 (they need Fenix), publishing tools for agents, and
Kiln's tasks as a tracker for the agenda (a candidate for 1.1).

## As built (2026-11-18)

Where the result differs from the plan above; the plan is left as it was
approved.

- **Fixtures** are under `tests/fixtures` (not `fixtures/`): `record.sh`,
  the demo project it makes, kiln's recorded answers in `out/`, and
  `hand/` for what no kiln here could print.
- **The Tree** was built with the page frame (E1.1), not in E1.2.
- **An integration task** was added after the parallel pages (E1.2 to
  E1.6): it joined them into the one page, made their marks and wording
  one, and added the prompt and the picker a tab can ask for, with E1.7.
- **`kiln.toml` is read** (from the project's top, never written) to offer
  the edits' choices: statuses in order, parents, roles and what they may
  point at, sign-off statuses, and the file targets for the export tasks.
- **Back and forward on the Item tab are `<` and `>`**: Fenix keeps
  `Ctrl+O` and `Ctrl+I`, which the page also takes where it is let.
- **`kiln.show` is `SPC m v`**, not `SPC m s`, so that the item's keys on
  the menu are the page's (`v i s l U n N E`) and `s` is status.
- **Eleven tabs, the tenth on `0`**: Exports and Comments came with E3, and
  `1`–`9` number nine; Comments is reached by `Tab`.
- **The export tasks name file targets only** (`kiln export -- NAME…`,
  titled so): a bare `kiln export` plans Confluence targets against the live
  space and fails with no token before writing any file.
- **The agent edit tools are off by default** behind `fenix.kiln.agent_edits`
  (on API 1.5 a service can't tell who called it, so any extension could
  call them without Fenix asking). They are class `run`, not `edit`, and
  `kiln_set` and `kiln_new` refuse sign-off statuses, which stay a person's
  call.
- **Decorations are in every project file** that a pane shows (item files,
  code, anything under the project's folder), not only item files and code
  references.
- **The review (E6.2) ran before E6.1**, so the README and the lesson
  were written from the keys and commands as reviewed. It fixed five
  defects, each with a test (an edit killed when `kiln.path` changed; a
  publish's outcome dropped when its page closed; a cancel that could
  reach a publish; `Enter` opening a path outside the project; an option
  reaching kiln from a restored page state), made one rule for cleaning
  every text shown, and renamed the Check tab's command `kiln.findings`.
- **K.1** (`kiln lsp --languages`) is written but unverified: Kiln can't be
  built from here (its pinned Fenix commit can't be fetched), so no released
  kiln has it, and with kiln 1.0.0 the language server covers Markdown only.
- **The Kiln side** grew beyond K.1 and K.2: the additions the extension
  needed are recorded as X1–X8 in Kiln's INTEGRATION.md.
- **E6.1 also** gave edits a ten-minute limit (`Project::edit`), where they
  had the queue's 60 seconds, so a renumber across many files on a slow disk
  isn't killed half way.
