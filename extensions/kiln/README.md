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

**Tree** is the first tab: every open item, nested by `parent` under a
heading per type, `REQ-014  Title  approved`, its status coloured by what
it means for its type (done, in progress, first, blocked, closed). `a`
shows closed items too. The other tabs are coming in the next versions.

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
| `Enter` | Open the item's file at its line |
| `a` | Show closed items too, or hide them again |
| `r` | Read the project again |

The mouse: a click on a tab shows it, on a row moves the cursor there, a
double click opens it; the wheel moves three rows. `q` and `Esc` close the
page, as on any of Fenix's.

After `SPC m`, in a Kiln project:

| Key | Command | What it does |
|---|---|---|
| `SPC m o` | `kiln.open` | Open the plan |
| `SPC m r` | `kiln.reload` | Read the project again |

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
- **Read the editor** (`editor = "read"`): where you are, so a page can
  follow the item ID under the cursor. It changes no buffer.

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

