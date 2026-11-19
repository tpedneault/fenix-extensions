# Kiln

[Kiln](https://github.com/tpedneault/kiln) keeps a project's requirements,
decisions, milestones, tasks, tests and risks as Markdown in the
repository. This extension is its place in Fenix: the plan on pages, edits
to the items, exports, and tools for coding agents. It works by running the
`kiln` program and showing what kiln says; it never writes an item file
itself.

It needs the `kiln` program, 1.0.0 or later, on the PATH (or its path in
the setting `kiln.path`), and Fenix 1.6 or later. Without kiln it still
loads: the plan, the doctor and every message say what is missing and how
to install it. Nothing is downloaded.

| Key | What it does |
| --- | --- |
| `SPC m o` | Open the plan |
| `SPC m f` | Find an item |
| `SPC m v` | Show the item under the cursor |
| `SPC m S` | Start working on the item here |

With standard keys they are `Ctrl+K m o` and so on.

## What you get

A folder with a `kiln.toml` at its top is a `KILN` project (a Rust or
Python project that has one stays what it is and gets all of this too).
Opening one gives you:

- **The plan**, a page with a tab per view of the project, below.
- **Kiln's language server** (`kiln lsp`): completion, hover, `gd`, `gr`,
  rename and problems in item files and code references. It is registered
  as the project loads, beside the other servers, with the same kiln the
  extension runs. Its languages are what `kiln lsp --languages` says
  (Markdown and the languages of the code `[code] paths` scans); a kiln
  without that command, 1.0.0 among them, gets Markdown only, said once on
  the extension's output page (`SPC h l`). A project whose
  `.fenix/tools.json` already runs kiln (what `kiln init --fenix` writes)
  keeps that one and the extension adds none, so kiln never runs twice. It
  is set again only when the languages or the kiln change (`kiln.path`,
  `kiln.toml` saved, `r`), and taken away when kiln is missing, too old or
  finds no `kiln.toml`.
- **The project doctor** (`SPC p h`) has Kiln's rows: kiln found and its
  version (or that it is not on the PATH, or that the path `kiln.path`
  holds can't be run, with a fix row, `Kiln: how to install kiln`, that
  opens Kiln's install page in the browser), the project read (or kiln's
  own error line), the language server, and what the last `kiln check`
  found, with a fix row that opens the plan's Check tab.
- **Tasks** (`SPC t t`): `kiln: check`, whose lines (`path:line:col: K102
  error: message`) the task pane turns into quickfix entries, and the two
  export tasks, which name the project's **file targets** (Markdown, HTML,
  Word) and say so in their titles: `kiln: export (file targets only)`
  runs `kiln export -- NAME…` and `kiln: export --check (file targets
  only)` runs `kiln export --check -- NAME…`. No task passes `--yes` and
  none can name a Confluence target, so none publishes; a bare `kiln
  export` would plan Confluence targets against the live space, which is
  why they are named. When `kiln.toml` can't be read, or lists no file
  target, only `kiln: check` is offered.
- **The modeline** says the last check's findings, `kiln 1 error, 2
  warnings`, and nothing when there are none.
- **Titles after IDs**, below.
- **A Kiln section on Home** and **a notification** when a save breaks the
  project, below.

### Titles after IDs

In the files of the project you have open, an item ID the project lists is
followed by its title and status, muted, the status in the colour its
meaning has (done green, blocked red…):

```text
parent = "MS-001" · First release · active
Anything slower; see [[ADR-001]] · Store readings in SQLite · accepted.
// verifies: TASK-005 · Ask about retention rules · blocked, TASK-004 · Add the export button · todo
```

The text goes after a `[[…]]` mention's `]]` and a quoted ID's closing
quote; titles are cut at 40 characters. Nothing is written into the file:
it is drawn beside the text, moves with it as you type, and is worked out
again when you stop typing, scroll, or the project is read again. Only IDs
the project has, as whole words: not inside a file name
(`TASK-002-write-readings.md`), a path or a URL, not on an item's own `id`
line, nor where the line already shows the title (a heading); an unknown ID
is for `kiln check` and the language server to point out. Any file under
the project's folder counts, item files and code alike, but only the lines
in view (and a margin) are looked at, and a file over 4 MiB not at all.
When kiln or the project is lost they go, rather than show stale titles.
Hover on an ID still comes from the language server; it gives no inlay
hints of its own, so nothing is shown twice. `kiln.decorations` turns them
off at once.

## The plan

`Kiln: open the plan` looks for kiln, reads the project (`kiln list`,
`kiln status`, `kiln check`) and opens a page with a tab per view, in the
order Kiln's own GUI has them, the project's name and the check's errors on
its top line. It is read again on `r`, when a file is saved, when git's head
moves, and every `kiln.refresh_seconds` while the page is in view. A read
that fails keeps the last one on screen with kiln's line under it; kiln
missing, too old, or finding no `kiln.toml` is said on every tab, in kiln's
own words. Statuses are coloured by what they mean for their type (done, in
progress, first, blocked, closed), the same on every tab.

- **Tree** (`1`): every open item nested by `parent` under a heading per
  type, `REQ-014  Title  approved`. `a` shows closed items too.
- **Trace** (`2`): `kiln trace`: each requirement, then a column per
  coverage role: what covers it (items, code locations, `+N`), or `gap`.
  `m` keeps only the items with a gap; `T` traces the next type.
- **Impact** (`3`): `kiln impact` for the item `i` was pressed on, on any
  tab: everything that reaches it, nearest first, with its path (`verifies
  REQ-001`, `child of REQ-002`), code locations included.
- **Board** (`4`): the work (tasks; else the types with done statuses, not
  milestones) under a heading per status with its count; open work says
  `ready` or `waits on TASK-003`, and `· owner` when claimed, as `kiln next`
  and `kiln status` say. Closed statuses wait for `a`. `H` and `L` move the
  item to the previous or next status in its type's order.
- **Roadmap** (`5`): the milestones by due date, each with its status and
  `kiln status`'s progress (`1/5 ██░░░░░░░░`), its open work under it with
  the Board's marks, then the work in no milestone. Closed milestones wait
  for `a`.
- **Decisions** (`6`): the decision log in ID order, `supersedes ADR-013` /
  `superseded by ADR-024`.
- **Changes** (`7`): `kiln diff` since a baseline, under Added, Changed and
  Removed, as kiln words them. `b` asks for the baseline (a tag, a branch,
  a commit, or `A..B`) in Fenix's prompt; there is none until you give one,
  and it is kept with the page.
- **Item** (`8`): `kiln show` for the item `v` was pressed on: its facts,
  its parent and children, its links both ways, mentions and code
  locations, a row each. `v` on a row follows it; `<` and `>` go back and
  forward.
- **Check** (`9`): `kiln check`'s findings under their file, in line order.
  `Enter` opens the line; `e` keeps the errors only.
- **Exports** (`0`): `kiln export --dry-run` for each target, and the Jira
  mirror: see "Exports and publishing" and "Jira".
- **Comments** (`Tab` from Exports): the unresolved Confluence comments by
  the item they are about: see "Comments".

On every tab the cursor stays on the same item when the project is read
again (or where it was, when that item went away). `/` narrows the rows as
you type (headings and parents of a match stay), `Enter` keeps the filter
and `Esc` clears it. `Enter` on a row opens the item's file at its line, or
the code location or finding the row is: only a file under the project's
folder, a path in kiln's answer that would leave it is refused, saying so.
The mouse clicks a tab or a row, a double click opens it, the wheel moves
three rows. `q` and `Esc` close the page. The page reopens with your next
session as you left it: the tab, and each tab's cursor, filter and `a`.
Several plan pages can be open in different panes, each with its own.

From a buffer, `Kiln: show the item under the cursor` (`SPC m v`) and `Kiln:
impact of the item under the cursor` (`SPC m i`) take the ID under the
cursor that the project has, else the item whose file it is, else say what
they looked at; `Kiln: find an item` (`SPC m f`) is Fenix's picker over
every item (ID, title, status), then its Item tab.

## Editing

Every edit is on the plan's keys (on the item under the cursor, on every
tab) and on commands from a buffer (on the item the cursor is on, found as
`show` finds it). Each is a pick or two and a prompt in Fenix's own picker
and prompt, then **one** kiln command, with every value as a single
argument (a title that starts with `-` is a title). What each runs:

| Edit | Asks | Runs |
|---|---|---|
| status | one of the item's type's statuses | `kiln set -- ID status=…` |
| `H` `L` (Board only) | nothing: the previous / next status in the type's order | `kiln set -- ID status=…` |
| link | a role its type may start, then a target the role allows and the item doesn't link yet | `kiln link -- ID ROLE TARGET` |
| unlink | one of the item's own links, then `yes` (`no` is first) | `kiln unlink -- ID ROLE TARGET` |
| new item | a type, then a title | `kiln new --title=… --format json -- TYPE` |
| new child | a type that may have the item as parent, then a title | `kiln new --parent=ID --title=… --format json -- TYPE` |
| title | the title, as it is | `kiln set -- ID title=…` |
| parent | an item of a type its type allows (not itself or anything under it), or none | `kiln set -- ID parent=…` |
| owner, due date | the value (empty removes it); the due date is `YYYY-MM-DD` | `kiln set -- ID owner=…`, `due=…` |
| renumber | the new ID (the next free one offered), then `yes` | `kiln renumber --to=… -- ID` |

Title, parent, owner, due date and renumber are only in the menu (`E`, or
`Kiln: edit the item here…`). A new item needs no item; `kiln new`'s answer
gives its ID and file, which opens in Fenix, and on the plan the cursor goes
to it.

The choices (each type's statuses in order, which types may be a parent,
the roles a type may start and what they may point at) come from the
project's `kiln.toml`, which the extension reads from the project's top
only for that, when it loads, on `r`, and when it is saved. It never writes
it. The choices are only offered: kiln checks every edit. Without a
`kiln.toml` it can read, the status, the role and the target are typed
instead, and `H`/`L` say why they can't; with it, the Board's status
headings come in each type's own order.

**When kiln refuses** (a file changed on disk since kiln read it, a value
the configuration doesn't allow, a date that isn't one), kiln's one line is
shown as it wrote it, on the plan's bottom line in the error look or, from a
buffer, in Fenix's message line, and nothing changes. A value you typed is
asked for again with what you typed, three times at most; a pick isn't
asked again. On success the line shows what kiln printed (`TASK-003: status
todo → doing`), and the project is read again.

No edit starts while the project isn't read yet (from a buffer, it starts
when the project is read), its last read failed, or kiln is still reading
the last edit back: the line says which. One flow at a time: a new one
replaces the one under way, and what you answer to a replaced one is
dropped; closing a pick or prompt with nothing ends the flow and nothing
runs. An edit kiln has started runs to its end, for up to ten minutes -- a
renumber stopped half way would leave the project half renumbered -- even
when you choose another kiln meanwhile or close the page (its outcome then
goes to the message line).

If an item's file is open in a buffer when kiln rewrites it, Fenix notices:
a buffer you haven't edited is re-read in place, keeping its pane's line;
one with unsaved edits is flagged `[disk]` in the modeline and `:w` refuses
to overwrite until you choose (`:w!` keeps yours, `:e!` takes kiln's).
kiln itself refuses to write a file that changed on disk since it read it.

## Start working on this

`S` on the plan and `Kiln: start working on the item here` (`SPC m S`) do
two things in order:

1. **Claim and start it, in kiln**: `kiln set -- ID owner=… status=…`, one
   command through the same edit path. The status is the type's first one
   after its initial status that isn't done, closed, `blocked` or a sign-off
   status (`todo` becomes `doing`); an item already started keeps its status
   and says so. The owner is set only when the item has none **and** the
   setting `kiln.owner` holds your name as the project writes owners: the
   extension can't know who you are without reading git's configuration or
   the environment, so with `kiln.owner` empty no owner is set. When
   `kiln.toml` can't be read, nothing is guessed. An item that is done or
   closed, a decision, or a milestone, starts nothing. If kiln refuses, its
   line is shown and **nothing is clocked**.
2. **Clock in on the Agenda extension's task for it.** The task is found
   among the agenda's open tasks, by one of the item's Jira keys, else by a
   title that is the item's ID or starts with it and a space; when there is
   none, one is made (`TASK-004 Add the export button`, with the item's file
   and line) and clocked into. A made task isn't linked to a Jira ticket: an
   item with a Jira key gets its clock only when the agenda already has the
   ticket. The agenda is one list for every project, so another project's
   `TASK-004` would be found as well.

The line says both: `TASK-004: status todo → doing; clocked in on "TASK-004
Add the export button"`. What clocking in does to a running clock is the
agenda's (it closes it into a time entry); this extension never clocks
anything out. Without the Agenda extension only the status changes, and the
line says `no clock: the Agenda extension isn't installed`. If the agenda
refuses, what it said is shown and the status stands.

## Exports and publishing

The Exports tab (`0`) shows `kiln export --dry-run` for each `[[export]]` in
`kiln.toml`: the target's line as a heading (`design-md (markdown, document)
→ docs/generated/DESIGN.md`, `spec (confluence, document) → space FLINT,
page 123456789 "Flint"`), then what would change (`would create`, `would
update`, `would remove … (no longer produced)`, `unchanged (1 file)`),
`leaves out: …`, and for Confluence the plan's operations with their
reasons and sizes, the pages that aren't Kiln's, the totals, warnings and
refusals, in kiln's words. `Enter` on a file that exists opens it.

**When it's read.** When the tab is first shown and on `R`, not on every
reload: the dry run renders the whole document and, for Confluence, reads
the space. Each target is asked on its own (`kiln export --dry-run --format
json -- NAME`), so a Confluence target with no token, or a server that
doesn't answer, shows kiln's line under its heading while the others still
show. A target says `reading… (asks Confluence)` while its dry run runs, up
to 10 minutes; a dry run read before the project changed says so, and `R`
reads it again. kiln runs one command at a time, so while a dry run or a
publish runs the other tabs keep the last read, with `reading…` on the top
line, and a reload or an edit waits.

**Writing.** `w` writes the file target under the cursor (`kiln export
--format json -- NAME`), `W` every file target, each named. It asks
nothing: the files are generated, and git has the last ones. The bottom
line says what kiln wrote and the target's dry run is read again.

**Publishing.** `P` on a Confluence target publishes it, and only when its
last dry run is current, isn't refused, and has something to do; otherwise
the bottom line says which. Fenix's prompt shows what will happen (`Publish
to Confluence: writes 2 pages in space DEMO, under page 123456789 "Demo
space home" (0 create, 1 update, 0 unchanged, 0 drift, 1 retire). Type
design-doc-confluence to publish`) and it publishes only when the target's
name is typed exactly; anything else, `y` included, cancels. It then runs
`kiln export --yes --format json -- NAME`, that one target. kiln reads the
space again as it publishes and publishes its own plan, not the dry run's:
what it did is shown as kiln says it (`published 0 created, 1 updated, 1
retired, 0 pruned`), with a word when its plan wasn't the one shown. A
refusal or an error is kiln's line, and a network or server error (exit 3)
says the next publish finishes what this one started.

A publish may run up to two hours before it is stopped (a publish stopped
half done stays half done until the next one). One runs at a time for the
extension, the Jira sync included; while it does every tab's bottom line
says `publishing NAME to Confluence…` (or `syncing Jira…`), and a second
`P` says one runs. Choosing another kiln or closing the page doesn't stop
it; with the page closed, what kiln did is said on the message line.

**Signing in.** With no token the target's line is kiln's (`no token for …:
run kiln auth confluence`), and `A` runs `kiln auth confluence` in a
terminal pane under the current one: the token is typed into kiln there,
and the extension never sees it. Then `R`.

**Never done from Fenix, and left to a terminal:** `--force` (overwrite
pages edited in Confluence), `--adopt` (replace an existing page's
content), `--prune-removed` (send retired pages to the trash) and
`--allow-shallow`. A plan kiln refuses (a page edited in Confluence, a page
to adopt that has content, a page too large) is shown with kiln's reason and
publishes nothing from here; overriding it is a decision typed at a
terminal. Nor is anything published anywhere but `P` on this tab: not by an
agent tool, a task or a command.

### Jira

With `[jira] url` in `kiln.toml`, and items that link tickets (`jira =` in
their front matter), **every item row ends with its tickets** on every tab:
`FLINT-12 In Review · Alice`, in the good look when the ticket is resolved,
muted otherwise, `FLINT-12 ?` for a key Jira doesn't know. The filter
reaches them. The tickets come from `kiln list --jira --format json`, asked
**apart from the project's reload**, so a slow or absent Jira never holds up
or fails it: once after the first read, then no sooner than ten minutes
after the last answer (kiln keeps its own ten-minute cache), and at once on
`r`. It is a background read that a reload, an edit or a dry run stops and
takes the place of, and that takes up to 60 seconds. When it can't be read,
the bottom line says why once, in kiln's words, and rows simply have no
tickets (or keep the ones read last). With no token the line starts `A signs
in (kiln auth jira)`: `A` opens `kiln auth jira` in a terminal pane. The
mirror needs Jira Server or Data Center and a personal access token, as
kiln does.

`J` on an item takes its status from its ticket: `kiln jira pull -- ID`,
through the edit routing, nothing to confirm (it is local and about one
item). kiln's line is shown (`TASK-002: status doing → done (from DEMO-102
Done)`, or why not) and the project is read again.

With `[jira] project`, Exports ends with a `jira` target: `kiln jira sync
--dry-run` as kiln prints it (kiln 1.0.0 has no JSON for it): its heading,
one line per operation (`create`, `update`, `move`, and `orphan` and `left
alone`, which are reported and never written), the totals, and `refused`
lines in the bad look. `P` on it asks for the Jira **project key** typed
exactly (`DEMO`; anything else cancels) and runs exactly `kiln jira sync
--yes`, under the same rules as publishing: one at a time, only when the
dry run is current and has something to write, for up to two hours (kiln
writes the ticket keys into the items only when it has applied everything,
so stopping it half way would create the tickets twice). Unlike a
Confluence plan, a refused operation doesn't stop a sync: kiln leaves that
ticket alone and says how many. What it says after is kiln's, **in the
warning look when files gained ticket keys: they are changed and
uncommitted, commit them**; exit 3 means the keys of the tickets it created
are written even so, and syncing again finishes it. The project is then read
again. The `jira` target needs a `kiln.toml` the extension can read.

### Comments

The Comments tab (`Tab` from Exports) shows `kiln confluence comments
--format json`: for each Confluence target a heading as kiln words it
(`spec → space FLINT: 2 unresolved comments`), under it the items that have
comments (an item row: `Enter` opens its file, `v` and `i` work), and under
each its comments: `Alice Martin · 2026-10-12  Startup in 50 ms Is that
measured cold?`. A comment about the page as a whole is under a line naming
the page; a target with no page yet says `not published yet`. `o` opens the
comment under the cursor in the browser, through Fenix, for an `http(s)`
link only. It is read when the tab is first shown and on `R`, up to 10
minutes, `reading… (asks Confluence)` meanwhile. kiln fails as a whole on
any target's error, so a failure is kiln's one line for the tab, with `A`
opening `kiln auth confluence` for a missing token. With no Confluence
target in `kiln.toml` the tab says so and asks kiln nothing. It never
answers or resolves a comment: that is Confluence's.

## Home and a notification

**Home.** In a Kiln project Fenix's Home has a **Kiln** section: the work
that is ready to start, the earliest milestone's first (as `kiln next` and
the Board say), at most four, each `TASK-004  Add the export button
(MS-001)`, then `open the plan` (saying how many more are ready). Picking a
ready row opens the plan on that item's Item tab. With nothing ready, one
row says so with the counts (`nothing ready: 2 in progress, 1 waiting`) and
opens the plan. The extension loads in a Kiln project (a `kiln.toml` at its
top) or when a `kiln.*` command runs, so the section appears once the
project is open and read.

**A notification** when saving a file takes `kiln check` from no errors to
at least one: `Kiln: 1 error after saving TASK-006-draft-the-user-guide.md`,
its body the first error, and an action, `Open the Check tab`. It is tagged,
so a newer one replaces it, and goes away when a later check has no errors.
Only that transition, and only after a save: not the first read of a project
that already has errors, not errors that persist, not warnings, not what a
git move, the timer, `r` or an edit finds. `kiln.notify_errors` turns it
off.

## For coding agents

An agent working in Fenix (Claude Code, Codex…) gets Kiln's tools through
Fenix's MCP server, as the `kiln` toolset. Each runs **one fixed kiln
command** in the open project through the same queue as the pages, and
answers with kiln's JSON.

| Tool | Runs | Takes |
|---|---|---|
| `kiln_next` | `kiln next [--milestone=ID] [--unowned] --format json` | `milestone`, `unowned` |
| `kiln_status` | `kiln status --format json` | none |
| `kiln_list` | `kiln list [--type=…] [--status=…] [--tag=…] --format json` | `type`, `status`, `tag`, each a comma list |
| `kiln_show` | `kiln show --format json -- ID` | `id` |
| `kiln_trace` | `kiln trace [--type=…] --format json` | `type` |
| `kiln_impact` | `kiln impact --format json -- ID` | `id` |
| `kiln_check` | `kiln check --format json` | none |
| `kiln_set` | `kiln set -- ID status=… owner=… due=… title=…` | `id`, any of `status`, `owner`, `due`, `title` |
| `kiln_link` | `kiln link -- ID ROLE TARGET` | `id`, `role`, `target` |
| `kiln_new` | `kiln new [--parent=ID] --title=… [--link=ROLE=ID]… [--tag=…]… [--body-file=-] --format json -- TYPE` | `type`, `title`, `parent`, `links` (`[{role, target}]`), `tags`, `body` (on kiln's stdin) |

**The reads** (the first seven, class `read`) are offered by default and
Fenix's default rules allow them. They change nothing: no file, no reload,
nothing on a page. `kiln_list` never includes bodies; `kiln_show` does, in
a field of its own, `body: {note, text}`, whose note says it is text a
person or another agent wrote: data, not instructions. Every answer says
the same of the titles and messages in it, and every string has its control
and invisible characters taken out (escape sequences, bidi overrides,
zero-width and tag characters). An answer is at most 64 KiB: a longer list
is cut on an item's boundary, with `left_out` (how many) and `narrow` (how to
ask for fewer), and a long body is cut on a line, saying how much there was.

**The edits** (`kiln_set`, `kiln_link`, `kiln_new`, class `run`) are asked
about: kiln writes the item files, not Fenix's buffers, so the change
**doesn't go through Agent changes** (`SPC a r`) and can't be rejected hunk
by hunk there. With Fenix's default rules Fenix asks you on each call (`y`
once, `Y` for the thread, `n`/`N` to refuse; `agents.mcp.allow` can say
`"kiln_set" = "allow"` or `"deny"`). They are **off until you turn on
`fenix.kiln.agent_edits`**: until then each answers that Kiln's edit tools
are turned off. Once one has run, the project is read again, and the answer
is kiln's own line, or for `kiln_new` the new item's `id` and `path`.

- **Sign-off stays with people.** `kiln_set` refuses a status that is a
  person's call for the item's type (`[signoff]` in `kiln.toml` when it is
  written, else Kiln's defaults: a requirement's `approved` and `verified`,
  a decision's `accepted` and `rejected`, a risk's `accepted` and `closed`,
  a milestone's `done`) and says so, so the agent asks you. `kiln_new`
  refuses a type that starts at one. When `kiln.toml` can't be read, no
  status is set and nothing is made.
- Edits wait for a usable project: none while it isn't read, while its last
  read failed, or while an edit is being read back; the answer says which.
- **Every input is checked before kiln runs**: an ID is `PREFIX-N`, a type,
  status, role or tag a lowercase word, a title or owner one line of visible
  text, a body Markdown with no control or hidden characters. A value rides
  inside one argument and positionals come after `--`, so nothing an agent
  gives can be read by kiln as an option. A field a tool doesn't take is an
  error naming it.
- **Never offered**: unlinking, renumbering, exporting, publishing, the Jira
  sync or pull, signing in, or any argument of the agent's own. While a
  Confluence publish or the Jira sync runs, every tool answers at once that
  kiln is busy. A call the agent gives up on, or that passes its two
  minutes, drops its answer; a read waiting or running for it is dropped or
  stopped, an edit runs to its end.
- **Who can call them.** On the extension API this extension is built for
  (1.5) a service can't tell who called it, so another installed extension
  could call `kiln.set`, `kiln.link` or `kiln.new` as a service without
  Fenix asking: the reason `fenix.kiln.agent_edits` is off by default. The
  reads give it what the plan shows.

## Keys

On the plan:

| Key | What it does |
|---|---|
| `j` `k`, `↓` `↑` | Next / previous row (headings are skipped) |
| `C-d` `C-u`, `PgDn` `PgUp` | Half a page down / up |
| `g g` `G`, `Home` `End` | First / last row |
| `Tab` `]`, `S-Tab` `[`, `1`–`9`, `0` | Next / previous tab, or one by its number (`0`: Exports; Comments by `Tab`) |
| `/` | Filter: narrows the rows as you type; `Enter` keeps it, `Esc` clears it |
| `Enter` | Open the item's file at its line (or the code location, or the finding) |
| `v` | The item under the cursor on the Item tab |
| `i` | Its impact on the Impact tab |
| `a` | Tree, Board, Roadmap: show closed items too, or hide them again |
| `r` | Read the project again |
| `m`, `T` | Trace: only the items with a gap; the next type |
| `b` | Changes: the baseline, typed in Fenix's prompt |
| `<`, `>` | Item: the item viewed before, after (`C-o` and `C-i` do the same where Fenix lets them through) |
| `e` | Check: errors only, or everything again |
| `R` | Exports, Comments: read again |
| `w`, `W` | Exports: write the file target under the cursor; every file target |
| `P` | Exports: publish the Confluence target under the cursor, after its name is typed; on `jira`, sync Jira after the project key is typed |
| `A` | Exports: `kiln auth confluence` on a Confluence target, `kiln auth jira` on `jira`; Comments: `kiln auth confluence`; anywhere else, `kiln auth jira` once Jira has asked for a token |
| `o` | Comments: open the comment under the cursor in the browser |
| `J` | Take the item's status from its Jira ticket |
| `S` | Start working on the item |
| `s` | Status: pick one of the item's type's statuses (the current one says `now`) |
| `H`, `L` | Board: the previous / next status in the type's order |
| `l` | Link: pick a role its type may start, then a target |
| `U` | Unlink: pick one of its links, then `yes` |
| `n` | A new item: its type, its title; its file opens |
| `N` | A new child of the item: its type, its title; its file opens |
| `E` | Every edit, in a pick: title, parent, owner, due date, renumber, and the ones above |

The bottom line shows `s status` and `E edits` only, to stay readable.
Kiln's own GUI has title, parent, owner, due and renumber under `SPC e`,
which a page in Fenix can't use (Space is Fenix's leader), so `E` stands for
it.

After `SPC m`, in a Kiln project, the item's keys being the plan's:

| Key | Command | What it does |
|---|---|---|
| `SPC m o` | `kiln.open` | Open the plan |
| `SPC m r` | `kiln.reload` | Read the project again |
| `SPC m v` | `kiln.show` | Show the item here on the Item tab |
| `SPC m i` | `kiln.impact` | Its impact, on the Impact tab |
| `SPC m f` | `kiln.find` | Find an item |
| `SPC m s` | `kiln.set_status` | Set the status of the item here |
| `SPC m l` | `kiln.link` | Link the item here to another |
| `SPC m U` | `kiln.unlink` | Remove a link of the item here |
| `SPC m n` | `kiln.new` | A new item (needs no item here); its file opens |
| `SPC m N` | `kiln.new_child` | A new child of the item here; its file opens |
| `SPC m E` | `kiln.edit` | The menu of every edit |
| `SPC m S` | `kiln.start` | Start working on the item here |
| `SPC m c` | `kiln.findings` | Open the plan on its Check tab |

"The item here" is found the same for all of them: the ID under the cursor
when the project has an item by it (so any prefix works, and `UTF-8` is no
ID); off any, in an item's file, that item; else the message line says what
they looked at and nothing runs (`kiln.new` needs no item). Run before the
project is read, they act once it is. Your own keybindings win over these.

## Commands

All fourteen are in the command picker (`SPC SPC`); `kiln.install_help` is
the only one with no key.

| Command | Title |
|---|---|
| `kiln.open` | Kiln: open the plan |
| `kiln.reload` | Kiln: read the project again |
| `kiln.show` | Kiln: show the item under the cursor |
| `kiln.impact` | Kiln: impact of the item under the cursor |
| `kiln.find` | Kiln: find an item |
| `kiln.set_status` | Kiln: set the status of the item here |
| `kiln.link` | Kiln: link the item here to another |
| `kiln.unlink` | Kiln: remove a link of the item here |
| `kiln.start` | Kiln: start working on the item here |
| `kiln.findings` | Kiln: open the plan's Check tab |
| `kiln.new` | Kiln: new item |
| `kiln.new_child` | Kiln: new child of the item here |
| `kiln.edit` | Kiln: edit the item here… |
| `kiln.install_help` | Kiln: how to install kiln (opens Kiln's install page; the doctor's fix row for a kiln that isn't found) |

## Settings

| Setting | Default | |
|---|---|---|
| `kiln.path` | empty | Where kiln is, when it isn't on the PATH |
| `kiln.refresh_seconds` | 5 | Seconds between reloads while a Kiln page is in view; 0: only on save and `r` |
| `kiln.decorations` | on | Titles and statuses after item IDs in the project's files; off takes them away at once |
| `kiln.owner` | empty | Who you are in Kiln, for "start working on this": the owner an item with none is given; empty: no owner is set |
| `kiln.notify_errors` | on | One notification when a save takes `kiln check` from no errors to some; off takes a showing one away |
| `fenix.kiln.agent_edits` | off | Lets coding agents use `kiln_set`, `kiln_link` and `kiln_new` (Fenix still asks about each call); sign-off statuses are refused either way |

## What it may do

- **Run `kiln`** (`process`), or the path `kiln.path` holds: nothing else.
  The extension **never writes an item file itself**; kiln does, when asked,
  by you or by an agent's `kiln_set`, `kiln_link` or `kiln_new` once
  `fenix.kiln.agent_edits` is on and Fenix's rules allow the call.
- **Read the project** (`filesystem = "project-read"`): `kiln.toml`, to
  offer choices and sign-off statuses, and `.fenix/tools.json`, to tell
  whether the project already runs kiln's language server. Nothing here
  writes a file.
- **Read the editor** (`editor = "read"`): where you are when you run the
  commands on "the item here" (the file, the cursor's line and column), to
  find the ID under the cursor; and, for the titles after IDs, the lines the
  panes show of the project's open files. It changes no buffer: the titles
  are decorations, drawn beside the text.
- **Hear events**: a file saved, git's branch and head, the lines a pane
  shows, and an open file changed, to read the project again on a change
  and to place the titles.
- **Offer services**, the ten agent tools above, to Fenix's agents.
- **Call the Agenda extension's services** (`agenda.tasks`,
  `agenda.add_task`, `agenda.clock_in`) for "start working on this": no
  permission is needed and Fenix answers that nothing offers them when the
  Agenda isn't installed. A notification and a Home section need none
  either.

**Never**: it never passes `--force`, `--adopt`, `--prune-removed` or
`--allow-shallow`; never publishes from a task, a command or an agent tool;
never sees a token (Confluence and Jira credentials stay kiln's, in the
system's keychain, and `kiln auth …` runs in a terminal pane where the token
goes to kiln); and has no network and no secrets permission. Publishing to
Confluence, the Jira sync, the mirror, `J` and the comments are kiln's own
network access, run after the target's name or the Jira project key is
typed, and the extension reads what kiln says. A comment's link is opened by
Fenix in the browser, for an `http(s)` link only.

Other people's writing (a title, a comment, kiln's own messages) is shown
the same way everywhere: an escape sequence goes whole (a terminal
hyperlink's hidden target with it), other control characters become spaces
on one line, and invisible characters (bidi overrides, zero-width and tag
characters) go. A message the extension gives reads `Kiln: …`, and kiln's
own line as kiln wrote it (`kiln: …`).

## Known limits

- **The language server covers Markdown only with kiln 1.0.0.** The
  extension asks `kiln lsp --languages` for more, and 1.0.0 doesn't have it;
  a later kiln that does adds the code languages. That path is written
  against hand-written answers, not a released kiln.
- **Changes has no default baseline.** `kiln diff` can't name "the latest
  tag" and the extension doesn't run git, so it shows nothing until `b` is
  given a tag, a branch, a commit or `A..B`.
- **The Roadmap doesn't list done work**, only what is still to do under each
  milestone, with the progress that says how much is done; the Tree and the
  Board have the done items.
- **`kiln.toml` is read to offer choices** (statuses, parents, roles,
  sign-off statuses), from the project's top only; one in a folder above it
  isn't read, and the choices are typed instead.
- **Jira** needs Jira Server or Data Center; kiln 1.0.0 has no JSON for
  `jira sync`, so its lines are shown as they are.
- **`Ctrl+O` and `Ctrl+I`**, which Kiln's own GUI uses to go back and
  forward on the Item tab, are Fenix's own, so `<` and `>` do it here.

**What has been tested.** This version has been tested against kiln 1.0.0's
real output, recorded in `tests/fixtures/`, and a fake host standing in for
Fenix; the answers of what needs a Confluence or Jira server, and
`kiln lsp --languages`, are hand-written from kiln's source. It has not yet
been tried in a running Fenix, on Windows or Linux, nor against a real
Confluence or Jira: how the pages look and take keys in both key schemes,
the language server, a publish and a sync, the agents' tools, the
decorations and the clock-in with the Agenda are all still to check there.

## Building

```text
cargo xtask pack extensions/kiln dist
cargo xtask install extensions/kiln
cargo test --locked --manifest-path extensions/kiln/Cargo.toml
```

The tests read kiln's real output, recorded in `tests/fixtures/` (Kiln's own
project and a small demo project made there by `tests/fixtures/record.sh`).
After a kiln release, record it again and read the diff:

```text
extensions/kiln/tests/fixtures/record.sh /path/to/kiln
```

Some answers aren't recorded, and are written by hand in
`tests/fixtures/hand/` (its `README` says from what, with file and line of
kiln's source): `kiln lsp --languages`, which no released kiln has yet, and
what `kiln export`, `kiln confluence comments`, `kiln jira sync` and `kiln
jira pull` print when they succeed, which needs a Confluence or Jira server
the recording never reaches (its Confluence plans are made offline, from
snapshots; Jira and the comments are recorded as the refusals, and `kiln
list --jira` over a mirror cache written for the purpose). Pages are tested
as text on the test kit's fake host; `src/pages/README.md` is the guide for
writing one.
