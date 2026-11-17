# Writing a tab of the plan page

Each tab is one file here, a `View` (`view.rs`). The frame (`frame.rs`)
does everything tabs share: tabs, cursor, scroll, `/` filter, `Enter`,
`r`, `a`, `v`, `i`, the mouse, the page's saved state, and the states
that aren't "loaded". `tree.rs` is the worked example: copy its shape.
Your tab is already registered (`mod.rs`'s `Tab`); edit only your file,
and add fixtures and snapshots.

## The view

```rust
#[derive(Debug, Default)]
pub struct Board { /* your own state: a toggle, the item handed to you */ }

impl View for Board {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> { /* from ctx.loaded */ }
    // all optional:
    fn keys(&self) -> Vec<(&'static str, &'static str)>   // ("H", "previous status")
    fn key(&mut self, key: KeyPress, row: Option<&Row>, ctx: &Ctx) -> Reply
    fn hides_closed(&self) -> bool                        // true: `a` works, read ctx.closed
    fn flex(&self) -> usize                               // the column that gives way: 1
    fn empty(&self) -> String                             // said with no rows
    fn queries(&self, ctx: &Ctx) -> Vec<Query>            // kiln asked for you
    fn answer(&mut self, tag: &str, result: &Result<Ran, Failure>)
    fn hand(&mut self, id: &str)                          // `v` → Item, `i` → Impact
    fn save(&self) -> serde_json::Value                   // your state, in the page's
    fn restore(&mut self, saved: &serde_json::Value)      // unreadable: leave the default
}
```

`rows` is called only when something changed (a new read, `a`, an answer,
your `Reply::Rebuild`), never per frame: it may walk the project, but stay
linear (2,000 items is the budget). Build an index once (`HashMap`), never
search `ctx.loaded.items` per item; `ctx.loaded.item(id)` is a lookup.

## Rows

One line each. `Row::heading(cells)` groups the rows after it (the cursor
skips it; the filter keeps it when a row under it matches). `Row::item(item,
cells)` (`Enter` opens the item's file at its line; `v`/`i` use its ID),
`ctx.row(id, cells)` for an ID another answer gives, `Row::code(path, line,
cells)`, `Row::finding(path, line, cells)`, `Row::note(text)` (a line the
cursor passes over: kiln's error, "reading…"). `.depth(n)` indents the
first cell; `.keyed(k)` changes what the cursor follows across reloads
(the ID by default). Cells line up in columns across the tab; don't pad.

Cells say what they are, not their colour: `Cell::id`, `Cell::text`,
`Cell::muted`, `Cell::title`, `Cell::new(t, Look::Good | Warn | Bad |
Accent)`. Statuses: `ctx.status(item)`, or `ctx.status_of(id, status)` for
one from `status`/`show` -- coloured by what they mean (`Meaning`: done,
in progress, initial, blocked, closed), the same on every tab.

## Asking kiln

Return `Query::new("impact", ["impact", id, "--format", "json"])` from
`queries` while you want it. The frame asks once a read (again after a
reload, and when the args change), shows "reading…" on the bottom line
meanwhile (`ctx.reading` too), keeps your last rows on screen, and calls
`answer(tag, result)` then `rows`. Read it with `view::stdout(result,
&[0])` and `model::object`/`model::array`; an error is one line, kiln's
own: show it as a `Row::note`. Only the tab shown asks.

## Keys

The frame owns `j k ↓ ↑ g G Home End C-d C-u PgUp PgDn Tab S-Tab ] [ 1–9
/ Enter r a v i`; Fenix owns `q`, `Esc` and Space. Everything else is
yours (`m`, `T`, `b`, `H`, `L`, `C-o`, `C-i`...): match on `key.key`,
`key.mods.ctrl`. Return `Reply::Rebuild` when your rows change,
`Reply::Say(text)` for the bottom line, `Reply::Ignored` otherwise.

## Tests

Pure tests in your file: build a `Loaded` from `tests/fixtures/out/demo`
and `own` (`model::tests::fixture`), a `Looks::new(&loaded)`, a `Ctx`, and
assert on `rows` (see `tree.rs`'s tests: a line per row). Page tests in
`lib.rs`'s style: a `Fake` answering kiln from the fixtures, `kiln.open`,
`h.keys(page, "4")` to reach your tab, then
`assert_snapshot!("board-demo", h.screen(page))` at `h.size = (110, 26)`.
Snapshots are `tests/snapshots/<tab>-<project>[-<what>].txt`; write them
with `UPDATE_SNAPSHOTS=1 cargo test` and read every line before keeping
them. Record a new fixture with `tests/fixtures/record.sh`, never by hand
(unless `tests/fixtures/hand/README` says why).

## Don't

- Edit `frame.rs`, `view.rs`, `mod.rs` or `lib.rs`: ask for a change.
- Block: no host calls, no `std::fs`, no waiting. kiln is asked through
  `queries`.
- Write an item file, or run an editing command (E2 adds those).
- Guess a shape: an answer that doesn't read is said, as kiln's line.
