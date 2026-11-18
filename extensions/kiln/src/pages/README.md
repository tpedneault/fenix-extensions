# Writing a tab of the plan page

Each tab is one file here, a `View` (`view.rs`). The frame (`frame.rs`)
does everything tabs share: tabs, cursor, scroll, `/` filter, `Enter`,
`r`, `a`, `v`, `i`, the mouse, the page's saved state, the states that
aren't "loaded", and asking the user or kiln for a view. `tree.rs` is the
worked example: copy its shape. `shared.rs` has what several tabs say
alike -- the plan's mark (`ready`, `waits on …`), the owner (`· ana`),
natural ID order -- and `Meaning::rank` orders statuses by meaning: use
them rather than writing your own. A tab is registered in `mod.rs`'s
`Tab`.

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
    fn answer_at(&mut self, tag: &str, result: &Result<Ran, Failure>, changes: u64) // the same, with
                                                          //   ctx.changes as it was asked (default: answer)
    fn hand(&mut self, id: &str)                          // `v` → Item, `i` → Impact
    fn typed(&mut self, tag: &str, text: &str, ctx: &Ctx) -> Reply    // after Reply::Ask
    fn picked(&mut self, tag: &str, value: &str, ctx: &Ctx) -> Reply  // after Reply::Pick
    fn edited(&mut self, tag: &str, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply // after Reply::Edit
    fn ran(&mut self, tag: &str, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply    // after Reply::Run
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
cells)`, `Row::finding(path, line, cells)`, `Row::entry(key, cells)` (a row
the cursor stops on that is no item and no file: an export target's line),
`Row::note(text)` (a line the cursor passes over: kiln's error,
"reading…"). `.depth(n)` indents the first cell (a heading's and a note's
text too); `.keyed(k)` changes what the cursor follows across reloads
(the ID by default). Cells line up in columns across the tab; don't pad.

Besides the project, `Ctx` has `config` (`kiln.toml` as the extension
read it, or why not), `changes` (the project's change count: it moves on
a save, an edit, a new git head, and a read that loads something new --
not on a reload that finds nothing) and `publishing` (the Confluence
target being published, one at a time for the whole extension).

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

A query too costly for every reload (Exports' dry run renders the whole
document and may ask Confluence) says `.per_round(n)`: the frame then asks
it once for round `n` of yours, however many reads load -- so when the tab
is first shown -- and again when you move to the next round (Exports'
`R`), or its args change. `.limit(ms)` gives kiln longer than the queue's
60 s (the job is stopped past it). `answer_at` hands you the project's
change count as the query was asked: an answer asked at `ctx.changes`
still shows the project as it is.

## Keys

The frame owns `j k ↓ ↑ g G Home End C-d C-u PgUp PgDn Tab S-Tab ] [ 1–9 0
/ Enter r a v i` and the edits' `s l U n N E H L` (`0` is the tenth tab,
Exports; the eleventh, Comments, is reached with `Tab`); Fenix owns `q`, `Esc` and Space, and keeps its own
chords from every page -- Ctrl+O and Ctrl+I among them (its jump list),
so a tab can't rely on them (in a terminal `C-i` is `Tab` anyway).
Everything else is yours: match on `key.key`, `key.mods.ctrl`. Return
`Reply::Rebuild` when your rows change, `Reply::Say(text)` for the bottom
line, `Reply::Ignored` otherwise.

Taken by tabs now: Trace `m` `T`, Changes `b`, Item `<` `>` (and `C-o`
`C-i` when they arrive), Check `e`, Exports `R` `w` `W` `P` `A`. The edits are the frame's, on every
tab: `s` `l` `U` `n` `N` `E`, and `H` `L` (Board only). The frame runs
their flows (`src/edit.rs`) on the row's `id`, so a row that is an item
gets them with nothing more; a row without one says "no item here".

## Asking the user, and editing

A view can't call the host, so it asks by replying, from `key` or from
any of the answer methods:

```rust
Reply::ask(tag, label, initial)      // Fenix's prompt  -> typed(tag, text, ctx)
Reply::pick(tag, title, items)       // Fenix's picker  -> picked(tag, value, ctx)
                                     //   items: Vec<Choice>, Choice::new(label, value).detail(d)
Reply::edit(tag, ["set", id, "status=doing"])   // kiln, through Project::edit
                                     //   (Reply::Edit { tag, args, stdin } for one that reads stdin)
                                     //   -> edited(tag, &result, ctx)
```

Three more, for what isn't an edit:

```rust
Reply::Run { tag, args, limit_ms, publish } // kiln, in the queue, changing no item (`kiln export`):
                                     //   no reload after, nothing said by the frame -> ran(tag, &result, ctx),
                                     //   which says the outcome; `publish: Some(target)` is the one
                                     //   publish at a time (a second is answered at once that one runs),
                                     //   not stopped when kiln.path changes, said on every bottom line
Reply::Bad(text)                     // said on the bottom line in the error look
Reply::Terminal { title, args }      // kiln in a terminal pane below (`kiln auth confluence`)
```

The frame turns each into an `Effect` naming the tab; `lib.rs` asks
Fenix's own prompt or picker, or runs the edit in kiln's queue, and
routes the answer back to the same page, tab and view with your `tag`
(whichever tab is shown by then). Your answer method replies the same
way, so an edit is a chain: pick a role → `picked` keeps it in a field
and replies `Reply::ask` for the target → `typed` replies `Reply::edit`
→ `edited`. A prompt or picker closed with nothing calls nothing: the
flow just ends (keep no state that needs closing). An answer for a page
that has closed is dropped. An answer that comes while the project isn't
read (kiln lost since) is dropped, and the bottom line says so.

An edit's outcome is said on the bottom line before `edited` is called:
kiln's stdout (`REQ-014: status draft → approved`), or its error line
(`kiln: … changed on disk …`) in the error look; a `Say` from `edited`
replaces it. The project is read again after every edit, whether kiln
changed anything or refused. Your rows are built again after every
answer, whatever you reply. Tag answers you can tell apart: the tag is
all that says which step an answer is for. Tags starting `kiln.edit#` are
the frame's own edit flows: don't use it.

## Tests

Pure tests in your file: build a `Loaded` from `tests/fixtures/out/demo`
and `own` (`model::tests::fixture`), a `Looks::new(&loaded)`, a `Ctx`, and
assert on `rows` (see `tree.rs`'s tests: a line per row). Page tests in your
file too, with `lib.rs`'s helpers (`crate::tests::{demo, own, opened, at,
foot, hand}`; the demo's fake already answers `trace`, and `show` and
`impact` for the items they were recorded for; add another answer with
`.on_spawn("kiln trace --type task --format json", Ran::ok(fixture(..)))`,
a prompt's or a pick's with `.on_prompt(label, text)` and
`.on_pick(title, row)`, or their `_nothing` for `Esc`; `Flow` in
`frame.rs`'s tests is a view whose `x` is a whole edit flow): `kiln.open`,
`h.keys(page, "4")` to reach your tab, then
`assert_snapshot!("board-demo", h.screen(page))` at `h.size = (110, 26)`.
Snapshots are `tests/snapshots/<tab>-<project>[-<what>].txt`; write them
with `UPDATE_SNAPSHOTS=1 cargo test` and read every line before keeping
them. Record a new fixture with `tests/fixtures/record.sh`, never by hand
(unless `tests/fixtures/hand/README` says why).

## Don't

- Block: no host calls, no `std::fs`, no waiting. kiln is asked through
  `queries`.
- Write an item file, or run kiln yourself: an edit is a `Reply::Edit`,
  a read a `Query`.
- Guess a shape: an answer that doesn't read is said, as kiln's line.
