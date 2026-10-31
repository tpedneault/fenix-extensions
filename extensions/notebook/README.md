# Notebook

Notes, a daily journal, diagrams, links between them and quick captures,
kept for you as plain files in one folder -- so making one never means
choosing where it goes. Its keys are under `SPC n`.

The folder is `notebook.folder`, or the `notebook` folder in Fenix's data
folder without one, read in place: `notes/*.md`, `journal/YYYY/YYYY-MM-DD.md`,
`diagrams/*.mmd`, and `index.toml` beside them for names, pins and exports.
Point it at a synced folder or an Obsidian vault and it works as it is.

Notes save themselves a second after you stop typing, and the version
before each editing session is kept (`h` on the notebook page).

| Key | What it does |
| --- | --- |
| `SPC n n` | The notebook page |
| `SPC n N`, `SPC n d` | A new note from a template, a new diagram |
| `SPC n j` | Today's journal day: the agenda's due tasks and yesterday's open checkboxes |
| `SPC n c` | Capture a line into today's journal (`>` first: the Inbox; `[ ]` first: a task) |
| `SPC n f`, `SPC n /`, `SPC n #` | Find by name, search (`tag:`, `type:`, `project:`, `is:todo`), tags |
| `SPC n b` | What links here, beside the note |
| `SPC n l`, `SPC n y` | Insert a link; copy a link to the line you're on |
| `SPC n t` | The checkbox under the cursor to the agenda |
| `SPC n e`, `SPC n s`, `SPC n i` | Export; save a ```mermaid block as a diagram; import a file |
| `SPC n v`, `SPC i D` | Paste a picture; insert a diagram |
| `SPC m a`, `SPC m g` | In a journal day: refresh its agenda list, add the day's commits |

In Markdown, `[[` completes a note's name and `gd` on a link follows it
(making the note when it isn't there yet).

## The page

| Key | What it does |
| --- | --- |
| `Enter`, `o`, `v` | Edit, read, view a diagram |
| `/`, `Tab`, `s` | Filter, the chips, pin the search as a chip |
| `t` `r` `P` `D` | Tags, rename (and every link to it), pin, duplicate |
| `h` | Earlier versions, and putting one back |
| `p` | Move it into the project's `docs` |
| `x x` | Delete: into the notebook's `.trash` folder |
| `e`, `T` | Export; a diagram's theme |
| `[` `]` | The journal day before, after |

## With other extensions

- **Diagrams** draws the diagrams in the page and exports (`diagrams.*`);
  `SPC n d` asks it for the kind and the starter.
- **Agenda** fills a journal day's agenda list (`agenda.due`), takes `SPC n t`
  (`agenda.add_task`), and opens a `[[FSW-212]]` link (`agenda.open_task`).

## Settings

| Setting | What it does |
| --- | --- |
| `notebook.folder` | Where the notebook is |
| `notebook.history` | Earlier versions kept of each entry |
| `notebook.journal` | Captures go to today's journal (else the Inbox) |
| `notebook.show_project_files` | List the project's own `.md` and `.mmd` files on the page |

## Outside Fenix

The control channel's `fenix/notebook.capture` is answered by this
extension. Fenix's reading view (`SPC m p`) asks it what `[[links]]` go to
(`notebook.link`, `notebook.follow`).
