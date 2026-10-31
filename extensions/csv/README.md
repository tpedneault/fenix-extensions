# CSV table

CSV and TSV files as a table. A `.csv` or `.tsv` opens in it however you
open it -- the explorer, the file picker, `:e`, a recent file -- its
columns lined up and fitted to what they hold, numbers on the right, the
rows fitted to the pane with the header kept above them as they scroll,
and the focused cell, on the accent, shown whole below them.

```text
people.csv  1,204 rows · 4 columns · , " · header · sorted by age ↓

name       age ↓  city       note
──────────────────────────────────────────────────────
grace         85  Arlington  wrote the first compiler…
alan          41  london
ada           36  London     "the Analytical Engine…
──────────────────────────────────────────────────────
note  row 1 of 1,204 (row 2 in the file)
wrote the first compiler, and found the first bug
```

| Key | What it does |
| --- | --- |
| `h` `j` `k` `l`, the arrows | A cell |
| `J` `K`, PageDown PageUp | A screen down, up |
| `g` `G` | The first and last row |
| `0` `$` | The first and last column |
| `s` | Sort by the focused column: up, down, as the file has it. Numbers sort as numbers, text without case, blanks last |
| `/` | Filter the rows: by a text in any cell, or by conditions on columns |
| `e`, `Enter` | Change the focused cell (`Enter` keeps it, `Esc` drops it; a cell of several lines keeps them, and `Tab` keeps it) |
| `E` | Change the column's name |
| `o` `O` | A row below, above |
| `a` `A` | A column after, before |
| `dd` `dc` | Delete the row, the column |
| `u` `U` | Undo, redo the table's changes |
| Ctrl+Z, Ctrl+Shift+Z (Ctrl+Y) | The buffer's undo and redo, as `u` and Ctrl+R in its text: its changes and the table's. In a field, its typing |
| `w` | Save -- with the rows in the sort's order, if it's sorted |
| `v` `V` | Select cells, rows (again stops) |
| `y` | Copy the cell's value, or the selection as CSV |
| `Y` | Copy the row (or the selection) as CSV |
| `m` | Copy the row (or the selection) as a Markdown table |
| `c` | The focused column's stats: count, distinct, blank, min, max, mean |
| `;` `"` `H` | Fields split by `,` `;` tab `\|`; quotes `"` `'` none; a header or not |
| `t` | The file's text, to edit (`SPC f o` too, and back to the table) |
| `q` | Close it |

With the mouse: a click focuses a cell (a column's name, its column), a
double click changes it (a column's name, the name), a drag selects
cells, and the wheel moves the focused row, three a notch.

A filter is a text, found in any cell without case, or conditions on
columns, by name (any case) or number (`3`, `#3`), with `&` between
them:

```text
price > 3.5
name ~ ada            the name holds "ada"
city !~ lon           it doesn't
status = done         any case; != for not
age >= 18 & city = paris
```

Numbers compare as numbers (`price > 3` passes `12`, which as text
doesn't), the rest as text. `/` then Enter on nothing shows every row
again.

The delimiter, the quotes and whether the first row is a header are
guessed from the file's first lines: the delimiter that splits them into
the same number of fields, and a first row of names over values that
aren't. `;`, `"` and `H` change them, and what you chose is kept for
that file (`dialects.json` in the extension's folder); changed back to
the guess, it's forgotten.

## Changes go through the file's buffer

A cell, a row or a column changed is an edit of the file's buffer, as
if typed: the buffer is changed, not the file, until `w` saves it, and
Fenix's undo has each change. `u` and `U` undo and redo here. Edited in
its text (`t`), or by a replace across files, the buffer is read again
as you go, once it's been still a moment. Before each change it checks
the buffer still has what the table shows, and if something else
changed it, reads it again instead.

A sort is the view's until you save: `w` writes the rows in its order,
one change you can undo, and the table is then in the file's order
again. A filter is only ever the view's.

Cells are written back as the csv crate writes them: quoted when they
hold the delimiter, a quote or a line break, the quote doubled. The
rest of the file -- its line breaks (`\n` or `\r\n`), a byte-order
mark, how other cells are quoted -- is left as it is.

## Big files

The file's records are found once, and a row's cells are read only when
it's shown, sorted on, filtered or summed up. A million rows (37 MiB)
open in about a tenth of a second and scroll at once; sorting them, a
filter or the stats take a fifth of a second or less (natively; the
sandbox is slower, within a key's second).

- **The size.** Fenix gives an extension a file of 64 MiB at most, and
  an extension has 128 MiB.
- **Changes to every row** -- a column added or deleted, a sort saved --
  are refused in a file over 20 MiB: the old text, the new and what
  undoes them don't fit. Change it as text. A change over 32 MiB is made
  but can't be undone here (Fenix's undo in the buffer still has it).
- **The check** that the buffer still holds what the table shows is
  skipped over 16 MiB.

## Limits

- **Rows shown.** As many as the pane has room for, so the header stays
  in view; `csv.rows` sets how many instead, and the page then scrolls
  in a short pane.
- **The wheel** moves the focused row, and the table scrolls with it.
- **Not UTF-8.** A file that isn't is shown, and not changed.
- **The session.** A table opens again with the session, where you were
  in it: one in the project is read from there, one from elsewhere
  because Fenix gives the extension again the files its pages show.

## What it may do

- Read, change and save the buffer of a file it shows, and know where
  you are in the editor (`editor = "write"`).
- Be told when a file's buffer changes (`events = ["changed"]`), to read
  its tables' again.
- Read the project you're in, to open a table kept in the session again.

## Building

```text
cargo xtask pack extensions/csv dist
cargo run --release --example million --manifest-path extensions/csv/Cargo.toml
```

`cargo test` checks the dialect guess, records and fields as the csv
crate reads them, changes as splices of the text and the buffer's
edits they become (lines and columns as Fenix counts them), undo,
filters, sorts, stats, copies, and the page: its keys and how it's laid
out. The example times a million rows.
