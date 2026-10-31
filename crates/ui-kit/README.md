# fenix-ui-kit

Widgets for an extension's canvas pages (`Page::canvas`), and
their keys: a canvas laid out cell by cell, tables, tabs, text fields,
splits, scrolled regions, lists and trees. So an extension stops laying
out its own.

```toml
[dependencies]
fenix-ui-kit = { path = "../../crates/ui-kit" }
```

Each widget is a state struct the extension keeps, and the data it
borrows to draw. `render(&state, ..., rect, &mut canvas)` draws it into a
rectangle; `handle_key(&mut state, key, ...)` answers a key with an
`Outcome` -- `Ignored` (not its key, or at an edge: the page can use it),
`Moved`, `Edited`, `Submit` (Enter), `Cancel` (Esc in a field), `Expand`,
`Collapse`. A widget remembers where it was scrolled to and how big it was
last drawn, so rendering takes `&self`.

```rust
use fenix_ui_kit::{Canvas, Column, Key, Outcome, Rect, Split, TableState, TextField, Width};

let mut c = Canvas::new();
let (side, body) = Split::sidebar(24).render(Rect::new(0, 0, cols, rows), &mut c);
schema_state.render(&schema, side, &mut c);
let (field, grid) = body.split_top(2);
query.render(field, &mut c);
table.render(&columns, &result, grid, &mut c);
c.to_page()   // the canvas, with its backgrounds, rules, keys and mouse

// page-key
match query.handle_key(key.into()) {
    Outcome::Submit => run(query.text()),
    Outcome::Ignored => { /* the page's own keys */ }
    _ => {}
}

// page-mouse, page-key-press
table.mouse(&Mouse::from(&event), rows, cols);
if let Some(press) = KeyPress::from_api(&press) { query.handle_press(press); }
```

| | |
|---|---|
| `Canvas` | Text at a line and column, each cell styled, hairlines (`rule`), dividers (`vline`), panels, the focused row, pictures, a popup; `text()` for tests; `to_api()` for `Page::canvas`. |
| `Rect` | `split_top`, `split_bottom`, `split_left`, `split_right`, `inset`, `row`. |
| `TableState` | Columns `Fixed`, `Fit { min, max }` or `Flex(weight)`, left or right; a frozen header; a focused cell (`hjkl`, arrows, `J` `K` a screen, `g` `G`, `0` `$`, Enter); a selection of cells or rows (`select`); scrolled both ways, a scrollbar. Rows come from a `TableSource` -- `len` and `row(i)` -- asked only for those shown. |
| `TabsState` | `1 Rows 12   2 Plan`, the active one tinted; Tab and Shift+Tab, `1`-`9`. |
| `TextField` | One line or several: caret, selection (`select`, `extend` for Shift), a word at a time, Backspace a word, pastes, undo and redo, scrolled to the caret. Sets the canvas's `typing`. |
| `Split` | Side by side or one over the other, fixed or a percentage, a divider. |
| `ScrollState` | A region drawn a line at a time, only the lines shown, with a scrollbar. |
| `TreeState` | A list, or a tree of `TreeNode`s: `l` opens (a `lazy` node's children loaded on `Expand`), `h` closes or goes to the parent. |
| `text` | `width`, `fit`, `fit_tail`, `slice`, `wrap`, in display cells. |

Columns are display cells: a wide character takes two (`unicode-width`).
`to_api` turns them into the character offsets the API's `cells` are.

`Style` is a colour role, a background (`Fill`: the panel, focus,
selection, accent, good, warn, bad, or `Rgb`) and an underline.
`Canvas::fill` and `fill_rect` put cells on a background, text or none:
a swatch, a preview. `to_page` draws them all, with the underlines,
`vrule`s, and whether the page takes more keys (`set_more_keys`) and the
mouse (`set_mouse`); `to_api` is the canvas alone, its backgrounds only
`Fill::Panel`, as panels.

The mouse: `Mouse::from(&event)` is at the event's cell. Each
widget has `mouse(&m, ...)` and `click(x, y, ...)`, from where it was last
drawn: a field puts its caret there, selects a word on a double click and
all on a triple, and selects as you drag; tabs go to the one clicked; a
tree or table selects the row or cell (a tree's marker opens and closes
it), a double click submits, the wheel moves three rows a notch; a
scrolled region scrolls with the wheel and its scrollbar.
`KeyPress::from_api` is a key from `page-key-press` with its `Mods`; a
field's `handle_press` takes Shift with the moving keys, Ctrl+A, Ctrl+Z,
Ctrl+Shift+Z and Ctrl+Y, and Ctrl+Left, Right and Backspace.

The crate has its own `Key`, `Role` and `Canvas`, and its tests run on the
host. The `api` feature, on by default, converts them to the extension
API's.

```text
cargo test --manifest-path crates/ui-kit/Cargo.toml
```
