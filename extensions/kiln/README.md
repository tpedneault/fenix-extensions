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

**Editing**, on every tab, on the item under the cursor: each edit is a
pick or two and a prompt in Fenix's own picker and prompt, then **one**
kiln command (`kiln set`, `link`, `unlink`, `new` or `renumber`); the
extension never writes an item file. The bottom line shows what kiln
printed (`TASK-003: status todo → doing`), or its refusal in the error
look (a file changed on disk, a value kiln doesn't allow), and a value you
typed is asked for again with what you typed (three times at most). The
project is read again after every edit, and the cursor follows the item
-- to its new ID after a renumber, to a new item, whose file opens. The
choices come from the project and its `kiln.toml` (each type's statuses in
order, which types may be a parent, the roles a type may start and what
they may point at), read by the extension from the project's top when it
loads, on `r`, and when `kiln.toml` is saved. They are only offered: kiln
checks every edit. Without a `kiln.toml` it can read (or one above the
project's folder), the status, the role and the target are typed instead,
and `H`/`L` say why they can't. No edit starts while the project isn't
read, its last read failed, or kiln is still reading the last edit back.
With `kiln.toml` read, the Board's status headings come in each type's own
order.

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
--check` and `kiln: export` (which writes the file targets, never
publishes to Confluence: no task passes `--yes`). `kiln: check`'s lines are
`path:line:col: K102 error: message`, which the task pane turns into
quickfix entries.

**The modeline** shows the last check's findings when there are any, `kiln 1
error, 2 warnings`, and nothing when there are none.

## Keys

On the plan:

| Key | What it does |
|---|---|
| `j` `k`, `↓` `↑` | Next / previous row (headings are skipped) |
| `C-d` `C-u`, `PgDn` `PgUp` | Half a page down / up |
| `g g` `G`, `Home` `End` | First / last row |
| `Tab` `]`, `S-Tab` `[`, `1`–`9` | Next / previous tab, or one by its number |
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
| `s` | Status: pick one of the item's type's statuses (the current one says `now`) |
| `H`, `L` | Board: the previous / next status in the type's order (at the first or last, it says so) |
| `l` | Link: pick a role its type may start, then a target the role allows that isn't linked yet |
| `U` | Unlink: pick one of its links, then `yes` (`no` comes first) |
| `n` | A new item: pick its type, type its title; its file opens |
| `N` | A new child of the item: pick a type that may have it as parent, type its title; its file opens |
| `E` | Every edit, in a pick: title, parent (an item of an allowed type, or none), owner, due date (`YYYY-MM-DD`; empty removes it), renumber (the next free ID offered, then `yes`), and the ones above |

The bottom line shows `s status` and `E edits` only, to stay readable;
`E`'s pick names each edit's own key. Kiln's GUI has title, parent, owner,
due and renumber under `SPC e`, which a page in Fenix can't use (Space is
Fenix's leader), so `E` stands for it.

The mouse: a click on a tab shows it, on a row moves the cursor there, a
double click opens it; the wheel moves three rows. `q` and `Esc` close the
page, as on any of Fenix's.

After `SPC m`, in a Kiln project:

| Key | Command | What it does |
|---|---|---|
| `SPC m o` | `kiln.open` | Open the plan |
| `SPC m r` | `kiln.reload` | Read the project again |
| `SPC m s` | `kiln.show` | Show the item under the cursor on the Item tab |
| `SPC m i` | `kiln.impact` | Its impact, on the Impact tab |
| `SPC m f` | `kiln.find` | Find an item: Fenix's picker over every item (ID, title, status), then its Item tab |

`kiln.show` and `kiln.impact` take the ID under the cursor when the
project has an item by it (so any prefix works, and `UTF-8` is no ID); off
any, in an item's file, that item; else the message line says what they
looked at. Run before the project is read, they act once it is.

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
  `kiln.show` or `kiln.impact` -- the file, the cursor's line and column
  -- to find the item ID under the cursor. It changes no buffer.

No network and no secrets: Confluence and Jira credentials stay Kiln's.

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

One file isn't recorded: `tests/fixtures/hand/lsp-languages.json`, the
answer of `kiln lsp --languages`, which no released kiln has yet, is
written by hand (its `README` says from what).

