# PDF reader

PDFs read in a pane. A
`.pdf` opens in it however you open it -- the explorer, the file picker,
`:e`, a recent file, the command line, `gf` on `spec.pdf#page=38` -- on a
surface in the pane you're in: every page one under the other, fitted to
the pane's width the first time, and where you left it after that.

The pages are rendered by **`fenix-pdfium`**, a small program that comes
with Fenix, beside `fenix.exe`, and uses the PDFium library you put there
(Fenix's docs/BUILDING.md says where). PDFium isn't compiled to
WebAssembly (docs/adr/0001-wasm-runtime.md), so the extension sends it
requests over a pipe and draws what comes back: only the parts of pages in
sight, and only the strip that scrolled into view as you read on.

## Reading

| Key | What it does |
| --- | --- |
| `j` `k`, `h` `l` | Scroll down and up, pan sideways; a count repeats (`5j`) |
| `Ctrl-d` `Ctrl-u`, `Ctrl-f` `Ctrl-b`, `PageDown` `PageUp` | Half a screen, a screen |
| `J` `K` | The next and previous page, at its top |
| `gg` `G`, `{n}G` | The first and last page, page n |
| `+` `-`, `=`, `zw` `zp` `z0` | Zoom in and out; fit width and fit page in turn; fit width, fit page, 100% |
| `Ctrl` and the wheel | Zoom |
| `/`, `n` `N` | Search (smart case: a capital makes it exact); the next and previous match |
| `o` | The outline, the matches and the marks, beside the pages (`Tab` switches; `za` `zM` `zR` fold; `Enter` goes there) |
| `m{a}`, `'{a}` | A mark here; back to it |
| `Ctrl-o` `Ctrl-i` | Back where a jump came from, and on again |
| `v`, then `w` `b` `e` `0` `$` `j` `k` `h` `l`, `y` | Select text with the keys, and copy it |
| A drag, then `y` | Select text with the pointer, and copy it |
| A click on a link, `f` | Follow it; label every link in sight, and follow the one you type |
| `yp` | Copy a link to this page, `specs/x.pdf#page=38` |
| `Esc` | Clear a selection, hide the search's highlights, close the outline |
| `q` | Close it |

`SPC r` has the same, for the PDF read last, from anywhere:

| Key | Command | What it does |
| --- | --- | --- |
| `SPC r f` | `pdf.documents` | The shelf: `reader.documents`, a project's own, the PDFs read lately |
| `SPC r n` `SPC r p` | `pdf.next_page` `pdf.prev_page` | The next and previous page |
| `SPC r [` `SPC r ]` | `pdf.first_page` `pdf.last_page` | The first and last page |
| `SPC r g` | `pdf.goto_page` | A page, by number |
| `SPC r =` `SPC r -` | `pdf.zoom_in` `pdf.zoom_out` | Zoom |
| `SPC r 0` `SPC r w` | `pdf.fit_page` `pdf.fit_width` | Fit the page, fit the width |
| `SPC r o` | `pdf.toggle_outline` | The outline |
| `SPC r t` | `pdf.headings` | A heading, picked by name |
| `SPC r /` | `pdf.search` | Search |
| `SPC r c` | `pdf.colors` | The pages as printed, or in the theme's colours |
| `SPC r y` | `pdf.copy_link` | A link to the page |

`SPC m` on a PDF has them too. `pdf.documents` may run from outside Fenix
(`fenix/runCommand`). The modeline says where you are: `PDF p. 38/212  Fit
width  /packet 2/5`. Home's **reading** lists the PDFs read lately, with
the page you were on.

## Settings

| Setting | What it is |
| --- | --- |
| `reader.colors` | `paper` (as printed) or `theme` (its colours) |
| `reader.zoom` | How a PDF opens the first time: `width` or `page` |
| `reader.page_gap` | Pixels between pages, 0 to 40 |
| `reader.remember` | Reopen each PDF where you left it, with its zoom and marks |
| `reader.sidebar` | Where `o` opens the outline: `beside`, `over` (in the PDF's pane, closing when it takes you somewhere), or `auto` |
| `reader.documents` | The shelf, `name = "file"`; a project may have its own (its paths relative to it) |

```toml
[reader.documents]
"Space Packet Protocol" = 'C:\refs\133x0b2e2.pdf'
"Time Code Formats" = 'C:\refs\301x0b4.pdf'
```

## Fenix asks it too

- `pdf.open`: a PDF at a page or a heading -- `gf` on `x.pdf#page=38`, a
  standard's clause from the CCSDS toolkit.
- `pdf.search`: a PDF searched -- the standards library's search.

## What it may do

- Run `fenix-pdfium`, which comes with Fenix. It reads the PDFs you open.
- Know where you are in the editor: a project's own shelf, and links from
  it (`yp`).

## Building

```text
cargo xtask pack extensions/pdf dist
```

`cargo test` runs the reader against a fake `fenix-pdfium` and a fake
Fenix (`tests/reader.rs`), checking what the pane shows pixel by pixel as
you scroll, zoom and jump. Fenix's own tests drive the real program on a
real PDF.
