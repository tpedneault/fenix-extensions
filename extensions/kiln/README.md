# Kiln

[Kiln](https://github.com/tpedneault/kiln) keeps a project's requirements,
decisions, milestones, tasks, tests and risks as Markdown in the
repository. This extension is its place in Fenix; it works by running the
`kiln` program and shows what kiln says.

**This version is the start of it.** A folder with a `kiln.toml` at its
top is a `KILN` project. `Kiln: open the plan` looks for kiln (1.0.0 or
later), reads the project (`kiln list`, `kiln status`, `kiln check`) and
opens the plan: a page with a tab per view of it, in the order Kiln's own
GUI has them, the project's name and the check's errors on its top line.
It's read again on `r` (or `Kiln: read the project again`), when a file is
saved, when git's head moves, and every `kiln.refresh_seconds` while the
page is in view; a read that fails keeps the last one on screen with
kiln's line under it. When kiln isn't there, is too old, or finds no
`kiln.toml`, every tab says so in kiln's own words.

The tabs (each can edit the item under the cursor, below); statuses are coloured
by what they mean for their type (done, in progress, first, blocked,
closed), the same on every tab:

- **Tree**: every open item, nested by `parent` under a heading per type,
  `REQ-014  Title  approved`. `a` shows closed items too.
- **Trace**: `kiln trace` -- each requirement, then a column per coverage
  role: what covers it (items, code locations, `+N`), or `gap`. `m` keeps
  only the items with a gap; `T` traces the next type.
- **Impact**: `kiln impact` for the item `i` was pressed on, on any tab:
  everything that reaches it, nearest first, with its path (`verifies
  REQ-001`, `child of REQ-002`), code locations included.
- **Board**: the work (tasks; else the types with done statuses, not
  milestones) under a heading per status with its count; open work says
  `ready` or `waits on TASK-003`, and `· owner` when claimed, as `kiln
  next` and `kiln status` say. Closed statuses wait for `a`.
- **Roadmap**: the milestones by due date, each with its status and
  `kiln status`'s progress (`1/5 ██░░░░░░░░`), its open work under it with
  the Board's marks, then the work in no milestone. Closed milestones
  wait for `a`.
- **Decisions**: the decision log in ID order, `supersedes ADR-013` /
  `superseded by ADR-024`.
- **Changes**: `kiln diff` since a baseline, under Added, Changed and
  Removed, each worded as kiln words it. `b` asks for the baseline (a tag,
  a branch, a commit, or `A..B`); there's none until you give one, since
  `kiln diff` can't name "the latest tag" and the extension doesn't run
  git. It's kept with the page.
- **Item**: `kiln show` for the item `v` was pressed on: its facts, its
  parent and children, its links both ways, mentions and code locations,
  a row each; `v` on a row follows it, `<` and `>` go back and forward.
- **Check**: `kiln check`'s findings under their file, in line order;
  `Enter` opens the line, `e` keeps the errors only.
- **Exports** (`0`): `kiln export --dry-run`, target by target, as the
  command line prints it; `w` and `W` write file targets, `P` publishes a
  Confluence target or syncs Jira. See "Exports and publishing" and "Jira"
  below.
- **Comments** (`Tab` from Exports): `kiln confluence comments`, the
  unresolved comments under each Confluence target, by the item they are
  about. See "Comments" below.

**Editing**: the same edits from the plan's keys (on the item under the
cursor, on every tab) and from a buffer (the commands under "Keys", on
the item the cursor is on). Each is a pick or two and a prompt in Fenix's
own picker and prompt, then **one** kiln command; the extension never
writes an item file, kiln does. What each runs, with the values as single
arguments (a title that starts with `-` is a title):

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

The choices come from the project and its `kiln.toml` (each type's
statuses in order, which types may be a parent, the roles a type may start
and what they may point at). The extension reads `kiln.toml` only for that,
from the project's top, when it loads, on `r`, and when `kiln.toml` is
saved; it never writes it. The choices are only offered: kiln checks every
edit. Without a `kiln.toml` it can read (or one above the project's
folder), the status, the role and the target are typed instead, and `H`/`L`
say why they can't. With it read, the Board's status headings come in each
type's own order.

**When kiln refuses** (a file changed on disk since kiln read it, a value
the configuration doesn't allow, a date that isn't one), kiln's one line is
shown as it wrote it -- on the plan's bottom line in the error look, from a
buffer in Fenix's message line -- and nothing changes. A value you typed
(a title, an owner, a date, a status or role without a `kiln.toml`) is asked
for again with what you typed, three times at most; a pick isn't asked again
(the same pick would be refused again). On success the bottom line or the
message line shows what kiln printed (`TASK-003: status todo → doing`).

No edit starts while the project isn't read yet (from a buffer, it starts
when the project is read), its last read failed, or kiln is still reading
the last edit back: the line says which, since the choices would come from
a read known to be out of date. The project is read again after every edit.
One edit at a time: on a page, a new one replaces the flow under way; from a
buffer, a new command replaces the one under way (one for the extension),
and what you answer to a replaced one is dropped. Closing a pick or prompt
with nothing ends the flow, and nothing runs.

If an item's file is open in a buffer when kiln rewrites it, Fenix notices
(it looks at open files on disk a couple of times a second and when its
window regains focus): a buffer you haven't edited is re-read in place,
keeping its pane's line; one with unsaved edits is flagged `[disk]` in the
modeline and `:w` refuses to overwrite until you choose (`:w!` keeps yours,
`:e!` takes kiln's). The extension does nothing about it, and kiln itself
refuses to write a file that changed on disk since it read it.

On every tab, the cursor stays on the same item when the project is read
again (or where it was, when that item went away), `/` narrows the rows as
you type, `Enter` opens the item's file at its line, and the page reopens
with your next session as you left it: the tab, and each tab's cursor,
filter and `a`. Several plan pages can be open in different panes, each
with its own.

**Kiln's language server** (`kiln lsp`: completion, hover, `gd`, `gr`,
rename and problems in item files and code references) is registered as
the project loads, beside the other servers, with the same kiln the
extension runs. Its languages are what `kiln lsp --languages` says (Markdown
and the languages of the code `[code] paths` scans); a kiln without that
command -- 1.0.0 is one -- gets Markdown only, said once on the extension's
output page (`SPC h l`), and a later kiln adds the code languages. A
project whose `.fenix/tools.json` already runs kiln (what `kiln init
--fenix` writes, or its older form) keeps that one, and the extension adds
none, so kiln never runs twice; remove the entry and save, and the
extension's takes over. It's set again only when the languages or the kiln
change (`kiln.path`, `kiln.toml` saved, `r`), and taken away when kiln
isn't found, is too old, or finds no `kiln.toml`.

**The project doctor** (`SPC p h`) has Kiln's rows: kiln found and its
version (or, when it isn't, that it's not on the PATH and `kiln.path` is
empty, or that the path it holds can't be run, with a fix row that opens
Kiln's install instructions in the browser -- nothing is downloaded), the
project read (or kiln's own error line), the language server, and what the
last `kiln check` found, with a fix row that opens the plan's Check tab. In a folder
that isn't a Kiln project it has none.

**Tasks** (`SPC t t`), in a Kiln project: `kiln: check`, and the two export
tasks, which name the project's **file targets** (Markdown, HTML, Word) as
`kiln.toml` lists them and say so in their titles: `kiln: export (file
targets only)` runs `kiln export -- NAME…` and `kiln: export --check (file
targets only)` runs `kiln export --check -- NAME…`, the same names the
Exports tab's `W` writes. No task passes `--yes`, and none can name a
Confluence target, so none publishes. (A bare `kiln export` -- what
`kiln: export` used to run -- is wrong in a project with a Confluence
target: kiln then plans that target against the live space too, so with no
token the whole run fails before it writes any file, and in a task pane
that is a terminal kiln would ask whether to publish.) When the extension
can't read `kiln.toml`, or it names no file target, the two export tasks
aren't offered, and `kiln: check` still is; a new file target saved in
`kiln.toml` brings them. `kiln: check`'s lines are `path:line:col: K102
error: message`, which the task pane turns into quickfix entries.

**The modeline** shows the last check's findings when there are any, `kiln 1
error, 2 warnings`, and nothing when there are none.

## Start working on this

`S` on the plan (on the item under the cursor, on every tab) and `Kiln:
start working on the item here` (`SPC m S`, on the item the cursor is on or
whose file it is, found as `kiln.show` finds it) do two things in order:

1. **Claim and start it, in kiln.** One command, as Kiln's own guidance has
   it: `kiln set -- ID owner=… status=…`, through the same edit path as
   every other edit (none starts while the project isn't read, or its last
   read failed, or kiln is still reading the last edit back). The status is
   the type's first one after its initial status that isn't done, closed,
   `blocked` or a sign-off status (`todo` becomes `doing`); an item already
   past its initial status keeps the one it has, and says so
   (`TASK-004: already doing`). The owner is set only when the item has none
   **and** the setting `kiln.owner` holds your name as the project writes
   owners: the extension can't know who you are without reading git's
   configuration or the environment, which it has no permission to, so
   with `kiln.owner` empty (the default) no owner is set. When `kiln.toml`
   can't be read, or the type has no such status, nothing is guessed: the
   status stays and the line says why. An item that's done or closed, or of
   a type with no done status (a decision) or a milestone, starts nothing.
   If kiln refuses, its line is shown and **nothing is clocked**.
2. **Clock in, on the Agenda extension's task for it.** The task is
   found, in this order, among the agenda's open tasks (`agenda.tasks`): one
   that carries one of the item's Jira keys (the agenda's `jiraKey`, from
   Jira), else one whose title is the item's ID or starts with it and a
   space (`TASK-004 Add the export button`: what this extension makes). When
   there is none, one is made with `agenda.add_task` -- the title is `ID
   Title`, and the item's file and line go with it for the agenda to come
   back to -- and the agenda is asked for its tasks again to learn which is
   the new one (`agenda.add_task` answers nothing), then `agenda.clock_in`
   starts the clock on it. A title is all that tells a made task back, and
   the agenda is one list for every project, so another project's `TASK-004`
   would be found as well. A made task isn't linked to a Jira ticket
   (`agenda.add_task` takes none): an item with a Jira key gets its clock
   only when the agenda already has the ticket (import it there).

The line says both: `TASK-004: status todo → doing; clocked in on "TASK-004
Add the export button"`, on the page's bottom line or the message line.

**What clocking in does to a running clock** is the agenda's doing, not
this extension's: it closes the running one into a time entry on the task it
ran on and starts the new one, so switching never needs a clock-out first.
This extension never clocks anything out.

**Without the Agenda extension** the status change still happens, and the
line says `no clock: the Agenda extension isn't installed` -- not an error.
If the agenda refuses or answers something this can't read, the line says
what it said (`the agenda didn't clock in: …`); the status change stands.
Nothing here needs a new permission: the agenda's services are open to any
installed extension, and this one only calls `agenda.tasks`,
`agenda.add_task` and `agenda.clock_in`. One start at a time: a new one
replaces the one under way (its late answer is dropped).

## Home and a notification

**Home.** In a Kiln project, Fenix's Home has a **Kiln** section: the work
that is ready to start, the earliest milestone's first (`kiln status`'s own
ready, the one `kiln next` and the Board use), at most four, each `TASK-004
Add the export button` with its milestone (`(MS-001)`), then `open the plan`
(saying how many more are ready when there are). Picking a ready row opens
the plan on that item's Item tab; `open the plan` opens the plan. With
nothing ready, one row says so with the counts (`nothing ready: 2 in
progress, 1 waiting`) and opens the plan. The section is given to Fenix only
when it changes, and taken away when the folder isn't a Kiln project kiln can
read. The extension only loads in a Kiln project (a `kiln.toml` at its top)
or when a `kiln.*` command runs, so the section appears once the project is
open and read, not on Home of a folder with no `kiln.toml`.

**One notification** when saving a file takes `kiln check` from no errors to
at least one: `Kiln: 1 error after saving TASK-006-draft-the-user-guide.md`,
its body the first error (`design/tasks/TASK-006-….md:11  K102  reference
REQ-099 doesn't exist`), and one action, `Open the Check tab`
(`kiln.check`, `Kiln: open the plan's Check tab`, `SPC m c`). It is tagged,
so a newer one replaces it, and it goes away when a later check has no
errors. Only that transition, and only after a save: not the first read of a
project that already has errors, not every read while they persist, not
warnings, not what a git move, the refresh timer, `r` or an edit through
kiln finds. `kiln.notify_errors` turns it off (and takes a showing one
away). The doctor's `kiln check` fix row opens the same Check tab.

## Titles after IDs

In the files of the project you have open, an item ID the project lists
is followed by its title and status, muted, the status in the colour its
meaning has (done green, blocked red…):

```text
parent = "MS-001" · First release · active
Anything slower; see [[ADR-001]] · Store readings in SQLite · accepted.
// verifies: TASK-005 · Ask about retention rules · blocked, TASK-004 · Add the export button · todo
```

The text goes after a `[[…]]` mention's `]]` and a quoted ID's closing
quote, titles are cut at 40 characters, and nothing is written into the
file: it's drawn beside the text, and moves with it as you type until it's
worked out again (when you stop typing, when you scroll, and when the
project is read again). Only IDs the project has, as whole words: not
inside a file name (`TASK-002-write-readings.md`), a path or a URL, not
on an item's own `id = "…"` line, nor where the line already shows the
title (a heading); an unknown ID is `kiln check`'s and the language
server's to point out. Any file under the project's folder counts, item
files and code alike, but only the lines in view (and a margin) are looked
at, and a file over 4 MiB not at all. When kiln or the project is lost
they go, rather than show stale titles; an ordinary reload keeps them.

Turn them off with `kiln.decorations` (Settings): they go at once. Hover
on an ID still comes from Kiln's language server (`kiln lsp`), with the
type, parent, links and coverage; it gives no inlay hints of its own, so
nothing is shown twice.

## Exports and publishing

The Exports tab (`0`) shows `kiln export --dry-run` for each `[[export]]`
in `kiln.toml`: the target's line as a heading (`design-md (markdown,
document) → docs/generated/DESIGN.md`, `spec (confluence, document) →
space FLINT, page 123456789 "Flint"`), then what would change (`would
create`, `would update`, `would remove … (no longer produced)`, or
`unchanged (1 file)`), `leaves out: …`, and for Confluence the plan's
operations with their reasons and sizes, the pages that aren't Kiln's,
the totals line, warnings and refusals -- in kiln's words. `Enter` on a
file that exists opens it.

**When it's read.** When the tab is first shown, and on `R`; not on every
reload (the dry run renders the whole document, and for Confluence reads
the space). Each target is asked on its own (`kiln export --dry-run
--format json -- NAME`): in kiln 1.0.0 one target kiln can't plan fails
the whole command, so a Confluence target with no token, or a server that
doesn't answer, shows kiln's line under its heading and the other targets
still show. A target says `reading… (asks Confluence)` while its dry run
runs, which may take up to 10 minutes; a dry run read before the project
changed (a save, an edit, a checkout, or a read that found something new)
says so, and `R` reads it again. Without a `kiln.toml` the extension can
read, every target is asked at once, and kiln's answer (or its one line)
is all there is.

kiln runs one command at a time, so while a dry run or a publish runs, the
project's reloads and edits wait for it: the other tabs keep the last
read, with `reading…` on the top line, and an edit runs once it's done.

**Writing.** `w` writes the file target under the cursor (`kiln export
--format json -- NAME`), `W` every file target, each named. It asks
nothing (the files are generated, and git has the last ones): the bottom
line says what kiln wrote, and the target's dry run is read again. The
project isn't, since no item changed.

**Publishing.** `P` on a Confluence target publishes it, and only when its
last dry run is current (read since the project last changed), isn't
refused, and has something to do; otherwise the bottom line says which,
and `R` reads it again. It shows what will happen in Fenix's prompt --
`Publish to Confluence: writes 2 pages in space DEMO, under page 123456789
"Demo space home" (0 create, 1 update, 0 unchanged, 0 drift, 1 retire).
Type design-doc-confluence to publish` -- and publishes only when the
target's name is typed exactly; anything else, `y` included, cancels and
says so. It then runs `kiln export --yes --format json -- NAME`, that one
target. kiln reads the space again as it publishes and publishes its own
plan, not the dry run's: what it did is shown as kiln says it (`published
0 created, 1 updated, 1 retired, 0 pruned`), with a word when its plan
wasn't the one shown; a refusal or an error is kiln's line, and a network
or server error (exit 3) says the next publish finishes what this one
started. The dry run is read again after.

A publish may run for up to two hours before the extension stops it (a
large space takes minutes, and a publish stopped half done stays half
done until the next one). One runs at a time, for every plan page, the Jira
sync (see "Jira") included; while it does, every tab's bottom line says
`publishing NAME to Confluence…` (or `syncing Jira…`), and a second `P`
says one runs. Choosing another kiln (`kiln.path`) or
closing the page doesn't stop it.

**Signing in.** With no token, the target's line is kiln's (`no token for
…: run kiln auth confluence`, or the credential store's own error), and
`A` runs `kiln auth confluence` in a terminal under the current pane: the
token is typed into kiln there. Then `R`.

**Never done here**, and left to a terminal: `--force` (overwrite pages
edited in Confluence), `--adopt` (replace an existing page's content),
`--prune-removed` (send retired pages to the trash) and `--allow-shallow`.
A plan kiln refuses -- a page edited in Confluence, a page to adopt that
has content, a page too large -- is shown with kiln's reason and
publishes nothing from here; overriding it is a decision typed at a
terminal. Nor does the extension publish anywhere but `P` on this tab:
not as an agent tool, not as a task, not as a command. It never sees a
token: no secret, no prompt for one, no environment variable.

## Jira

With `[jira] url` in `kiln.toml`, and items that link tickets (`jira =` in
their front matter), **every row that is an item ends with its tickets**,
on every tab: `FLINT-12 In Review · Alice`, in the good look when the
ticket is resolved (Jira's Done category), muted otherwise, and `FLINT-12 ?`
for a key Jira doesn't know. An item with two tickets has two cells. The
filter (`/`) reaches them.

Tickets come from `kiln list --jira --format json`, asked **apart from the
project's reload**, so a slow or absent Jira never holds up or fails it: it
is asked when a plan page is in view, the project is read and nothing else
is running; once after the first read, then no sooner than ten minutes
after the last answer (kiln keeps its own ten-minute cache, so asking more
often would only spend a process), and at once on `r`, `Kiln: read the
project again`, a changed `[jira] url` and after a sync. It is a background
read: when a reload, an edit or any other command needs kiln, the read is
stopped for it and asked again when kiln is idle (what it fetched isn't
cached until it finishes; it only reads). It may take 60 seconds. When it
can't be read, the bottom line says why **once**, in kiln's words, and rows
simply have no tickets -- or keep the ones read last, after a later failure,
until Jira answers again. With no token the line starts `A signs in (kiln
auth jira)`: `A` opens `kiln auth jira` in a terminal under the current
pane, the token typed into kiln (Exports' `A` on a `jira` row does the same).
The mirror needs Jira Server or Data Center and a personal access token,
like kiln's (a Jira Cloud URL is refused by kiln, naming its G6).

`J` on an item takes its status from its ticket: `kiln jira pull -- ID`,
through the same routing as the edits -- nothing to confirm (it is local and
about one item), kiln's line is shown (`TASK-002: status doing → done (from
DEMO-102 Done)`, or `every item's status already matches its ticket`, or
why not), and the project is read again. An item that links no ticket, a
project with no `[jira]`, and a row that is no item say so without asking
kiln. With no token, kiln's line is shown with the same `A` hint.

**The `jira` target on Exports.** With `[jira] project` in `kiln.toml` (the
Jira project key `kiln jira sync` puts tickets in), the Exports tab ends
with one more target, `jira`: `kiln jira sync --dry-run`, shown as kiln
prints it -- its heading (`jira (DEMO) → DEMO-100 "Kiln: Demo"`), one line
per operation (`create`, `update`, `move`, and in the warning look `orphan`
and `left alone`, which are reported and never written), the totals, and
`refused` lines in the bad look. `jira sync` has no `--format json` in kiln
1.0.0, so these are kiln's text lines as they are: nothing is parsed but
which line is the heading, the totals, a refusal, and which operations
write. Read when the tab is first shown and on `R` like the others, and
again after a sync, once the project has been read again.

`P` on it syncs, under the rules of publishing: one at a time with
Confluence publishes (a second says which runs); only when the dry run is
current (read since the project last changed) and has something to write; it
shows what will happen in Fenix's prompt -- `Sync Jira: creates, updates and
moves tickets, jira (DEMO) → DEMO-100 "Kiln: Demo": 3 operations (1 create,
1 update, 1 move, 2 unchanged). Type DEMO to sync` -- and runs only when
the Jira **project key** is typed exactly (`DEMO`; `y`, another case and
the name of an export target all cancel). It runs exactly `kiln jira sync
--yes`, never anything else, and may run for up to two hours: kiln creates a
ticket per request and writes the keys into the items only when it has
applied everything, so stopping it half way would leave tickets no item
names, and the next sync would create them again. Unlike a Confluence plan,
a refused operation doesn't stop a sync -- kiln leaves that ticket alone and
syncs the rest -- so `P` is allowed with `refused` lines and says how many.

What it says after is kiln's: the summary (`jira: 1 created, 1 updated, 1
moved; 1 file gained ticket keys (commit them)`), **shown prominently in the
warning look under the target and on the bottom line when it says files
gained keys -- they are changed and uncommitted**, the refusals after it,
or kiln's line. Exit 3 (Jira failed part way) says what kiln's CLI.md does:
the keys of the tickets it created are written into the items even so --
commit them, so none is created twice -- and syncing again finishes it. Then
the project is read again (sync writes `jira =` into items and `kiln.toml`)
and the plan read again after that. While it runs every tab's bottom line
says `syncing Jira…`. The Jira target needs `kiln.toml` to be readable by the
extension; when it can't be, Exports asks kiln about every export target at
once and has no `jira` target.

## Comments

The Comments tab (`Tab` from Exports) shows `kiln confluence comments
--format json`: for each Confluence target, a heading as kiln words it
(`spec → space FLINT: 2 unresolved comments`), under it the items that have
comments -- an item row: `Enter` opens its file, `v` and `i` work -- and
under each its comments, `Alice Martin · 2026-10-12  Startup in 50 ms Is
that measured cold?`. A comment about the page as a whole, which kiln
can't place on an item, is under a line naming the page. `o` opens the
comment under the cursor in the browser (through Fenix, as an `http(s)`
link only: a link of any other kind, or with a space in it, is refused with a
line saying so). A target with no page yet says `not published yet`.

It is read when the tab is first shown and on `R` -- not on every reload; it
asks the server for every target's pages and their comments, so it may take
up to 10 minutes, and says `reading… (asks Confluence)` meanwhile. kiln's
command fails *as a whole* on any target's error (no token, a server that
doesn't answer) and prints nothing per target, so a failure is kiln's one
line for the whole tab, with `A` opening `kiln auth confluence` in a
terminal when it's a missing token. With `kiln.toml` read and no Confluence
target in it, the tab says so and asks kiln nothing.

Comment text is other people's writing: it is shown as plain text on one
line, never interpreted -- control characters (an escape sequence, a
newline) become spaces and the length is cut, and no Markdown, link or
markup in it does anything. The only thing done with a comment is `o`.
It never answers or resolves one: that is Confluence's.

## For coding agents

An agent working in Fenix (Claude Code, Codex…) gets Kiln's tools through
Fenix's MCP server, as the `kiln` toolset. Each runs **one fixed kiln
command** in the open project, through the same queue as the pages, and
answers with kiln's JSON.

| Tool | Runs | Takes |
|---|---|---|
| `kiln_next` | `kiln next [--milestone=ID] [--unowned] --format json` | `milestone`, `unowned` |
| `kiln_status` | `kiln status --format json` | -- |
| `kiln_list` | `kiln list [--type=…] [--status=…] [--tag=…] --format json` | `type`, `status`, `tag`, each a comma list |
| `kiln_show` | `kiln show --format json -- ID` | `id` |
| `kiln_trace` | `kiln trace [--type=…] --format json` | `type` |
| `kiln_impact` | `kiln impact --format json -- ID` | `id` |
| `kiln_check` | `kiln check --format json` | -- |
| `kiln_set` | `kiln set -- ID status=… owner=… due=… title=…` | `id`, any of `status`, `owner`, `due`, `title` |
| `kiln_link` | `kiln link -- ID ROLE TARGET` | `id`, `role`, `target` |
| `kiln_new` | `kiln new [--parent=ID] --title=… [--link=ROLE=ID]… [--tag=…]… [--body-file=-] --format json -- TYPE` | `type`, `title`, `parent`, `links` (`[{role, target}]`), `tags`, `body` (on kiln's stdin) |

**The reads** (class `read`) are offered by default, and Fenix's default
rules allow them. They change nothing -- no file, no reload, nothing on a
page. `kiln_list` never includes bodies; `kiln_show` does, in a field of its
own, `body: {note, text}`, whose note says it's the item's text as a
person or another agent wrote it: data, not instructions. Every answer
says the same of the titles and messages in it, and every string has its
control and invisible characters taken out (escape sequences, bidi
overrides, zero-width and tag characters), which could hide text from
you reading the thread. An answer is at most 64 KiB (Fenix's own default
for its tools; it doesn't cut an extension's): a longer list is cut on an
item's boundary, with `left_out` (how many) and `narrow` (how to ask for
fewer: `type`, `status`, `tag`, `kiln_show` for one item); a long body is
cut on a line, saying how much there was and in which file the rest is.

**The edits** (class `run`) are asked about: kiln writes the item files,
not Fenix's buffers, so the change **doesn't go through Agent changes**
(`SPC a r`) and can't be rejected hunk by hunk there -- it's a program run
that changes files, which is what `run` says. With Fenix's default rules,
Fenix asks you on each call, as a card in the agent's thread showing the
tool and its arguments: `y` once, `Y` for the rest of the thread, `n`/`N`
to refuse (`agents.mcp.allow` can say `"kiln_set" = "allow"`, or `"deny"`).
They are **off until you turn on `fenix.kiln.agent_edits`**: until then
each answers that you've turned Kiln's edit tools off. They come with the
toolset the reads bring (Fenix offers a toolset, not single tools), and
`writes = true` holds them back from Fenix's control channel unless your
rules allow running. Once one has run, the project is read again, as for
any edit, and the answer is kiln's own line (`TASK-003: status todo →
doing`), or for `kiln_new` the new item's `id` and `path`; kiln's refusal
is its one `kiln: …` line.

- **Sign-off stays with people.** `kiln_set` refuses a status that is a
  person's call for the item's type -- `[signoff]` in `kiln.toml` when it's
  written, else Kiln's defaults (a requirement's `approved` and `verified`,
  a decision's `accepted` and `rejected`, a risk's `accepted` and `closed`,
  a milestone's `done`), as `kiln signoff` reads them -- and says so, so
  the agent asks you. `kiln_new` refuses a type that starts at one. When
  `kiln.toml` can't be read from the project's top, no status is set and
  nothing is made, since sign-off can't be told apart.
- Edits wait for a usable project: none while it isn't read, while its last
  read failed, or while an edit is being read back -- the answer says
  which.

**Every input is checked before kiln runs**: an ID is `PREFIX-N`, a type,
status, role or tag a lowercase word; a title or owner is one line of
visible text; a body is Markdown with no control or hidden characters. A
value rides inside one argument and positionals come after `--`, so
nothing an agent gives can be read by kiln as an option. A field a tool
doesn't take is an error naming it. **Never offered**: unlinking,
renumbering, exporting, publishing, the Jira sync or pull, signing in, or
any argument of the agent's own. While a Confluence publish or the Jira
sync runs (they can hold kiln for minutes), every tool answers at once that
kiln is busy, rather than waiting into its deadline. A call the agent
gives up on (or that passes its two minutes) drops its answer, and a read
waiting or running for it is dropped or stopped; an edit runs to its end.

**Who can call them.** Fenix's ask applies to agents' calls. On the
extension API this extension is built for (1.5), a service can't tell who
called it, so another installed extension could call `kiln.set`,
`kiln.link` or `kiln.new` as a service without Fenix asking -- the reason
`fenix.kiln.agent_edits` is off by default. The reads give it what the
plan shows.

## Keys

On the plan:

| Key | What it does |
|---|---|
| `j` `k`, `↓` `↑` | Next / previous row (headings are skipped) |
| `C-d` `C-u`, `PgDn` `PgUp` | Half a page down / up |
| `g g` `G`, `Home` `End` | First / last row |
| `Tab` `]`, `S-Tab` `[`, `1`–`9`, `0` | Next / previous tab, or one by its number (`0`: Exports; Comments by `Tab`) |
| `/` | Filter: narrows the rows as you type (headings and parents of a match stay); `Enter` keeps it, `Esc` clears it |
| `Enter` | Open the item's file at its line (or the code location, or the finding) |
| `v` | The item under the cursor on the Item tab |
| `i` | Its impact on the Impact tab |
| `a` | Tree, Board, Roadmap: show closed items too, or hide them again |
| `r` | Read the project again |
| `m`, `T` | Trace: only the items with a gap; the next type |
| `b` | Changes: the baseline, typed in Fenix's prompt |
| `<`, `>` | Item: the item viewed before, after (Kiln's `C-o` and `C-i` are Fenix's own here) |
| `e` | Check: errors only, or everything again |
| `R` | Exports, Comments: read again |
| `w`, `W` | Exports: write the file target under the cursor; every file target |
| `P` | Exports: publish the Confluence target under the cursor, after its name is typed; on `jira`, sync Jira after the project key is typed |
| `A` | Exports: `kiln auth confluence` in a terminal on a Confluence target, `kiln auth jira` on `jira`; Comments: `kiln auth confluence`; anywhere else, `kiln auth jira` once Jira has asked for a token |
| `o` | Comments: open the comment under the cursor in the browser |
| `J` | Take the item's status from its Jira ticket (`kiln jira pull -- ID`) |
| `S` | Start working on the item: `kiln set` its owner (`kiln.owner`) and status, then the Agenda's clock on its task ("Start working on this") |
| `s` | Status: pick one of the item's type's statuses (the current one says `now`) |
| `H`, `L` | Board: the previous / next status in the type's order (at the first or last, it says so) |
| `l` | Link: pick a role its type may start, then a target the role allows that isn't linked yet |
| `U` | Unlink: pick one of its links, then `yes` (`no` comes first) |
| `n` | A new item: pick its type, type its title; its file opens |
| `N` | A new child of the item: pick a type that may have it as parent, type its title; its file opens |
| `E` | Every edit, in a pick: title, parent, owner, due date, renumber, and the ones above |

The bottom line shows `s status` and `E edits` only, to stay readable;
`E`'s pick names each edit's own key. Kiln's GUI has title, parent, owner,
due and renumber under `SPC e`, which a page in Fenix can't use (Space is
Fenix's leader), so `E` stands for it.

The mouse: a click on a tab shows it, on a row moves the cursor there, a
double click opens it; the wheel moves three rows. `q` and `Esc` close the
page, as on any of Fenix's.

After `SPC m`, in a Kiln project. The item's keys are the plan's (`v` `i`
`s` `l` `U` `n` `N` `E`), so a hand that knows one knows the other:

| Key | Command | What it does |
|---|---|---|
| `SPC m o` | `kiln.open` | Open the plan |
| `SPC m r` | `kiln.reload` | Read the project again |
| `SPC m v` | `kiln.show` | Show the item here on the Item tab |
| `SPC m i` | `kiln.impact` | Its impact, on the Impact tab |
| `SPC m f` | `kiln.find` | Find an item: Fenix's picker over every item (ID, title, status), then its Item tab |
| `SPC m s` | `kiln.set_status` | Set the status of the item here |
| `SPC m l` | `kiln.link` | Link the item here to another |
| `SPC m U` | `kiln.unlink` | Remove a link of the item here |
| `SPC m n` | `kiln.new` | A new item (needs no item here); its file opens |
| `SPC m N` | `kiln.new_child` | A new child of the item here; its file opens |
| `SPC m E` | `kiln.edit` | The menu of every edit: title, parent, owner, due date, renumber and the rest |
| `SPC m S` | `kiln.start` | Start working on the item here: claim it, then clock in on the agenda |
| `SPC m c` | `kiln.check` | Open the plan on its Check tab |

`SPC m s` was `kiln.show` until the edits came; status is `s` on the plan
and in Kiln's GUI, so show moved to `v`, which is its key on the plan.

"The item here" is found the same for all of them: the ID under the cursor
when the project has an item by it (so any prefix works, and `UTF-8` is no
ID); off any, in an item's file, that item; else the message line says what
they looked at and nothing runs (`kiln.new` needs no item). Run before the
project is read, they act once it is. Your own keybindings win over these.

`Kiln: how to install kiln` (`kiln.install_help`) is the doctor's fix row
for a kiln that isn't found; it has no key.

## Settings

| Setting | Default | |
|---|---|---|
| `kiln.path` | empty | Where kiln is, when it isn't on the PATH |
| `kiln.refresh_seconds` | 5 | Seconds between reloads while a Kiln page is in view; 0: only on save and `r` |
| `kiln.owner` | empty | Who you are in Kiln, for "start working on this": the owner an item with none is given; empty: no owner is set |
| `kiln.notify_errors` | on | One notification when a save takes `kiln check` from no errors to some; off takes a showing one away |
| `kiln.decorations` | on | Titles and statuses after item IDs in the project's files ("Titles after IDs"); off takes them away at once |
| `fenix.kiln.agent_edits` | off | Lets coding agents use `kiln_set`, `kiln_link` and `kiln_new` (Fenix still asks about each call); sign-off statuses are refused either way |

## What it may do

- **Run `kiln`** (`process`), or the path `kiln.path` holds: nothing else.
  The extension never writes an item file itself; kiln does, when asked --
  by you, or by an agent's `kiln_set`, `kiln_link` or `kiln_new` once
  `fenix.kiln.agent_edits` is on and Fenix's rules (by default, you) allow
  the call.
- **Read the project** (`filesystem = "project-read"`): `kiln.toml`, and
  `.fenix/tools.json`, only to tell whether it already runs kiln's
  language server.
- **Read the editor** (`editor = "read"`): where you are when you run
  `kiln.show`, `kiln.set_status` and the other commands on "the item here"
  -- the file, the cursor's line and column -- to find the item ID under
  the cursor; and, for the titles after IDs, the lines the panes show of
  the project's open files (Fenix gives a file's text whole; only the
  shown lines of it are looked at). It changes no buffer: the titles are
  decorations, drawn beside the text.

- **Call the Agenda extension's services** (`agenda.tasks`,
  `agenda.add_task`, `agenda.clock_in`) for "start working on this": no
  permission is needed, any installed extension may call them, and Fenix
  answers that nothing offers them when the Agenda isn't installed. It
  shows a notification (about errors) and a Home section the same way: no
  permission.

No network and no secrets: Confluence and Jira credentials stay Kiln's.
Publishing to Confluence is kiln's own network access, run by `P` on the
Exports tab after the target's name is typed; so are the Jira sync (after
the project key is typed), the Jira mirror, `J` and the comments -- kiln
reads Confluence and Jira, the extension reads what kiln says. `kiln auth
confluence` and `kiln auth jira` run in a terminal pane, where the token
goes to kiln. A comment's link is opened by Fenix in the browser, for an
`http(s)` link only.

## Building

```text
cargo xtask pack extensions/kiln dist
cargo xtask install extensions/kiln
```

The tests read kiln's real output, recorded in `tests/fixtures/` (Kiln's
own project and a small demo project made there). After a kiln release,
record it again and read the diff:

```text
extensions/kiln/tests/fixtures/record.sh /path/to/kiln
```

Some answers aren't recorded, and are written by hand in
`tests/fixtures/hand/` (its `README` says from what, with file and line of
kiln's source): `kiln lsp --languages`, which no released kiln has yet, and
what `kiln export`, `kiln confluence comments`, `kiln jira sync` and `kiln
jira pull` print when they succeed -- that needs a Confluence or Jira
server, which the recording never reaches (its Confluence plans are made
offline, from snapshots; Jira and the comments are recorded as the refusals,
and `kiln list --jira` over a mirror cache written for the purpose: kiln's
real JSON around invented tickets).

