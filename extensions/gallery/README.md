# Widget gallery

Every widget of [`ui-kit`](../../crates/ui-kit) working on one page, for
you writing an extension: what each looks like, how its keys and the
mouse behave, and, a key away, the code that draws it. It's the kit's
living documentation, and its visual regression test.

`gallery.open` (`SPC h g`) opens it. The sections are down the left;
`[` and `]` go through them, or click one. Each is a widget working as
it would in a real extension, on made-up data:

| Section | What it shows |
| --- | --- |
| Table | `TableState`: 400 rows of a month's weather at twenty stations, asked for only as they're shown; fitted and fixed columns, wider than the pane, numbers on the right, cells coloured by `TableSource::style`; sorting, a selection of cells or rows, copied. |
| Tree | `TreeState`: a project's files, folders opened and closed, one (`tests/fixtures/`) a lazy branch read only when it's opened; what's selected beside it. |
| Tabs | `TabsState`: a response's parts, by Tab, Shift+Tab, a number or a click; badges; the same bar in 28 cells. |
| Text field | `TextField`: a name to edit, with a caret, a selection, words at a time, undo and pastes; a `/` filter narrowing a list as you type. |
| List | A list is a tree of leaves: a release's checklist, rows styled, the day on the right, ticked with Enter; and its empty state. |
| Popup menu | `Canvas::popup`: a menu beside the focused row, which Fenix draws over the page while the page keeps the keys. |
| Splits | `Split`: a sidebar, an editor over two panes side by side, each saying the rectangle it got, resized as you go. |
| Scrolling | `ScrollState`: a build's 300 lines, only those shown drawn, a scrollbar to click and drag; `n` to the next problem. |
| Styles | Every `Role` and `Fill`, underlines, the theme's palette as swatches, and `fit`, `fit_tail`, `slice` and `wrap` on a line with wide characters and a combining mark, at a width you choose. |
| Mouse | `Mouse::from(&event)`: a pad to draw on, and each event said in words -- the button, clicks, drags, the wheel, what was held. |

The strip at the page's foot has the keys of the section you're in.
`c` opens that section's code beside the page, highlighted as Rust: its
own source file, as built into the extension, so what you read is what
runs.

## Keys

| Key | What it does |
| --- | --- |
| `SPC h g` | Open the gallery (`gallery.open`) |
| `[` `]` | The section before, after |
| `c` | The section's code, beside the page |
| `q`, `Esc` | Close it |

And each section's own, as its strip says: in the table `hjkl`, `J` `K`,
`0` `$`, `s` to sort, `v` `V` to select, `y` to copy, `Enter`; in the
tree `j` `k`, `l` `h`, `Enter`, `+` `-`; in the tabs Tab, Shift+Tab,
`1`-`5`, `n`; in the field `e` to edit, `/` to filter, then `Enter` to
keep, `Esc` to drop, Shift and Ctrl with the arrows, Ctrl+Z; in the
menu `m` and `Enter`, then `j` `k` and a row's letter. These are the
page's keys, the same with Vim keys and standard keys.

## Where to start

Copy the section nearest to what you want from `src/sections/`. Each
is a `State` that renders into a `Rect` of a `ui_kit::Canvas` and
answers a `KeyPress` and a `Mouse`, with nothing of Fenix's in it;
`src/lib.rs` is the glue an extension needs around that: the page,
`page_key`, `page_key_press`, `page_mouse`, `page_paste`, and the
session (`page_state`, `page_restore`). The gallery keeps which section
you were on and where you were in each with the session.

## Settings

None.

## What it may do

Nothing beyond its page: `[permissions]` is empty. A copy (`y` in the
table, *Copy the path* in the menu) writes the clipboard, which needs
no permission.

## Building

```text
cargo xtask pack extensions/gallery dist
cargo test --manifest-path extensions/gallery/Cargo.toml
```

It's built against the 1.0 extension API, as `ui-kit` is: the kit's
conversions to the API's types are 1.0's, and the gallery's keys are
the page's own, the same under either key scheme, so it needs nothing
newer (no `key_for`).

`cargo test` checks that every section, in each state its keys put it
in, draws only inside its rectangle at sizes from 120 by 40 down to 1
by 1 and none; the page as text at 100 by 30 (and some at 80 by 24)
against the snapshots in `tests/snapshots/` (`GALLERY_SNAPSHOTS=write
cargo test` takes them again); each widget's keys and mouse; that the
session keeps what it should; and that every section's file has its
`// gallery:begin` and `// gallery:end` markers round the code `c`
shows. The text a canvas becomes is drawn by `tests/support/mod.rs`.

Not yet tried in a running Fenix: the snapshots are how it's been
seen. The colours, the tints, the popup's box and the code's
highlighting are Fenix's to draw, and are untested here.
