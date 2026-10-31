# Colours

Colour literals made visible and editable.

- **Swatches**: a block of its colour beside every colour in the file
  you're in, and in the files you've been in lately, kept up to date as
  you type. A see-through colour is shown over the editor's background,
  one out of sRGB's gamut as it maps into it. `SPC C t` turns them off
  and on.
- **The picker** (`SPC C p`): the colour under the cursor by hue,
  saturation, lightness and alpha -- keys or the mouse on their bars --
  or typed in as hex (or any literal), with what it was beside what it
  is now. Enter writes it back the way it was written, as one change you
  can undo: `#fff` stays three digits while it can, `rgba(…, 0.5)` keeps
  its commas and its alpha, `hsl(…deg …)` its unit, a name stays a name
  if the colour has one. With no colour at the cursor it types one in
  there.
- **Conversions** (`SPC C c`, or `SPC C h` `r` `s` `o` straight to
  hex, `rgb()`, `hsl()`, `oklch()`): the colour under the cursor, or
  every one in the selection, at once. OKLCH is Björn Ottosson's OKLab,
  as CSS Color 4 has it; a colour outside sRGB is mapped in as CSS does
  (its chroma brought down, its lightness and hue kept). Numbers are
  written with as few decimals as read back as the same colour.
- **Contrast** (`SPC C x`): the colour under the cursor on another (the
  next colour on its line, else the editor's background), their WCAG 2.x
  ratio, and what it passes: AA and AAA for normal and large text, and
  3:1 for a control's parts. A preview draws big letters and a box in the
  one colour on the other. `f` moves the focused colour as little as it
  takes to pass AA (`F` AAA), its hue and chroma kept; Enter writes it
  back into its literal.
- **The palette** (`SPC C P`): every colour the project's files use, read
  a few at a time (`.gitignore` kept to; binaries, pictures and files
  over 1 MB passed over), the same colours as one however they're
  written, grouped by hue -- or the near-duplicates, colours so close
  (ΔEOK under 0.03) they're likely meant to be one. Enter opens a colour
  to its uses, and a use in its file at its line.

Literals: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`; `rgb()`, `rgba()`,
`hsl()`, `hsla()` and `oklch()`, in CSS's legacy syntax (commas) and its
modern one (spaces and a `/`), with `none`, percentages and hue units;
CSS's 148 named colours, in stylesheets and markup (`.css`, `.scss`,
`.less`, `.html`, `.vue`, `.svelte`, `.svg` and the like); and a Fenix
theme's `{ color = "#rrggbb", alpha = 0.12 }`. In code, `#123` and
`#1234` (all digits) are taken for issue numbers, not colours.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC C p` | `colours.pick` | The picker, on the colour under the cursor |
| `SPC C c` | `colours.convert` | Convert it, or those selected: a picker of the four |
| `SPC C h` `r` `s` `o` | `colours.to_hex`, `to_rgb`, `to_hsl`, `to_oklch` | Straight to one |
| `SPC C x` | `colours.contrast` | The contrast checker |
| `SPC C P` | `colours.palette` | The project's palette |
| `SPC C t` | `colours.toggle_swatches` | Swatches on or off |

On the picker:

| Key | What it does |
| --- | --- |
| `j` `k` | Hue, saturation, lightness, alpha |
| `h` `l`, `H` `L` (Shift-Left, Shift-Right, PageDown, PageUp) | A step down or up, or ten: a degree, a percent |
| `0` `$` | Its ends |
| `x` `#` `Tab` | Type it in: hex, or any literal (`rgb(…)`, `tomato`), shown as you type |
| `r` | Back to what it was |
| `Enter` | Write it, and close |
| The mouse | A click or a drag on a bar sets it; the wheel steps it |

On the contrast checker: `Tab` (`j` `k`) the other colour, `h` `l`
darker and lighter (`H` `L` more), `s` swaps them, `f` `F` the nearest
that passes AA, AAA, `x` types one in, Enter writes what changed. A
click picks a colour, the wheel over it makes it darker or lighter.

On the palette: `j` `k` `J` `K` `g` `G`, Enter or `l` opens a colour to
its uses or goes to a use, `h` closes it, `Tab` (`1`, `2`) by hue or the
near-duplicates, `y` copies the colour, `r` reads the project again. A
click selects, a double click opens, the wheel scrolls.

## Settings

| Setting | | |
| --- | --- | --- |
| `colours.swatches` | `true` | Swatches beside colours |
| `colours.most` | `2000` | The most in a file (10,000 at most); in a file of more than 10,000 lines, only those within 5,000 of the cursor |
| `colours.syntax` | `modern` | How hex and names convert: `rgb(59 130 246 / 50%)`, or `legacy`, `rgba(59, 130, 246, 0.5)`. A function keeps its own |

## Permissions

`editor = "write"` for the file you're in -- its swatches, and the
colours written back -- and `filesystem = "project-read"` for the
palette. It starts with Fenix (`on_startup`), for the swatches, and
hears which file you're in and its changes (`events = ["focus",
"changed"]`).

It uses Fenix's decorations, pages with backgrounds, the mouse and more
keys.
