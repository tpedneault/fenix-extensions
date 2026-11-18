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
  Confluence target. See "Exports and publishing" below.
- **Comments**: not built yet (`Tab` reaches it).

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
last `kiln check` found, with a fix row that opens the plan. In a folder
that isn't a Kiln project it has none.

**Tasks** (`SPC t t`), in a Kiln project: `kiln: check`, `kiln: export
--check` and `kiln: export` (no task passes `--yes`, so none publishes to
Confluence). `kiln: export` is a bare `kiln export`: in a project with a
Confluence target, kiln plans that target against the live space too,
which needs its token, and with none it stops before writing any file;
not on a terminal it then publishes nothing and exits 1. The Exports tab's
`W` names the file targets instead, and writes only them. `kiln: check`'s
lines are `path:line:col: K102 error: message`, which the task pane turns
into quickfix entries.

**The modeline** shows the last check's findings when there are any, `kiln 1
error, 2 warnings`, and nothing when there are none.

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
done until the next one). One runs at a time, for every plan page; while
it does, every tab's bottom line says `publishing NAME to Confluence…`,
and a second `P` says one runs. Choosing another kiln (`kiln.path`) or
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
| `R` | Exports: read every target's dry run again |
| `w`, `W` | Exports: write the file target under the cursor; every file target |
| `P` | Exports: publish the Confluence target under the cursor, after its name is typed |
| `A` | Exports: `kiln auth confluence` in a terminal, on a Confluence target |
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

## What it may do

- **Run `kiln`** (`process`), or the path `kiln.path` holds: nothing else.
  The extension never writes an item file itself; kiln does, when asked.
- **Read the project** (`filesystem = "project-read"`): `kiln.toml`, and
  `.fenix/tools.json`, only to tell whether it already runs kiln's
  language server.
- **Read the editor** (`editor = "read"`): where you are when you run
  `kiln.show`, `kiln.set_status` and the other commands on "the item here"
  -- the file, the cursor's line and column -- to find the item ID under
  the cursor. It changes no buffer.

No network and no secrets: Confluence and Jira credentials stay Kiln's.
Publishing to Confluence is kiln's own network access, run by `P` on the
Exports tab after the target's name is typed; `kiln auth confluence` runs
in a terminal pane, where the token goes to kiln.

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
`tests/fixtures/hand/` (its `README` says from what): `kiln lsp
--languages`, which no released kiln has yet, and what `kiln export`
prints when it publishes -- that needs a Confluence server, which the
recording never reaches (its Confluence plans are made offline, from
snapshots).

