# Parquet viewer

Parquet files read in a pane. A `.parquet` (or `.parq`) opens in it
however you open it -- the explorer, the file picker, `:e`, a recent
file -- its rows as a table, columns fitted to what they hold, numbers
on the right, nulls dimmed, and the focused value shown whole below.

```text
trips.parquet  48,211,004 rows · 9 columns · 47 row groups · zstd · 1.9 GB
Rows  Schema  Tab switches

id   name      score  ok     day         at                    price
──────────────────────────────────────────────────────────────────────
  1  null        0.0  true   2026-01-01  2026-10-31T12:00:00Z   0.00
  2  craft-1     0.5  false  2026-01-02  2026-10-31T12:00:01Z   0.01
──────────────────────────────────────────────────────────────────────
row 2 of 48,211,004 · name Utf8
craft-1
```

Only the footer is read when it opens. Rows are read as they're shown,
from the row groups that hold them -- and, when the file has a page
index, only the pages that do -- so `G` goes to the last of a billion
rows as fast as to the tenth.

The **Schema** view lists each column with its Arrow type, how it's
stored, whether it may be null, its nulls, smallest and largest values
(from the file's statistics), its size on disk and its codec; below it,
the Parquet version, what wrote the file, and its metadata's keys
(`pandas`, `ARROW:schema`).

| Key | What it does |
| --- | --- |
| `h` `j` `k` `l`, the arrows | A cell |
| `J` `K`, PageDown PageUp | A screen down, up |
| `g` `G` | The first and last row |
| `0` `$` | The first and last column |
| Tab | Rows, Schema |
| `Enter` | In the schema, that column's rows |
| `y` | Copy the value (in the schema, the column's name) |
| `Y` | Copy the row as CSV, with its header (in the schema, the schema) |
| `R` | Read the file again |
| `q` | Close it |

With the mouse: a click focuses a cell, and the wheel moves the focused
row.

Values are written as their types write them: dates and times in ISO
form (timestamps in their zone), decimals at their scale, lists as
`[a, b]`, structs as `{x: 1, y: 2}`, binary as hex.

Snappy, zstd, gzip, LZ4 (and LZ4 raw) and Brotli are all read, every one
in pure Rust; zstd's is [ruzstd](https://crates.io/crates/ruzstd).

A file in the project is read where it is, a part at a time, whatever
its size. One elsewhere is read whole, up to the 64 MiB Fenix gives an
extension; open a bigger one from its project.

## Permissions

| Permission | Why |
| --- | --- |
| `filesystem = "project-read"` | A file in the project is read where it is, as far as each read needs |
| `editor = "read"` | Which project is open: whether a file is in it |
