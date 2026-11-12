# {{name}}

A page that counts the words in what you type: its words, characters and
lines, and about how long it would take to read. Open it with
`SPC o w`, or `{{name}}: open the page` in the command palette, and type
or paste.

<!-- TODO(you): say what yours does and how to use it, in the second
person, with a screenshot in screenshots/ (CONTRIBUTING.md has the house
style). -->

## Keys

| Key | Command | What it does |
|---|---|---|
| `SPC o w` | `{{folder}}.open` | Open the page |
| `Esc` | | On the page: close it |

On the page, the field takes the usual keys: the arrows and the mouse,
selecting with Shift, `Ctrl+Z` and `Ctrl+Shift+Z` to undo and redo.

## Settings

| Setting | Default | |
|---|---|---|
| `{{folder}}.words_per_minute` | 200 | How fast you read, for the reading time |

## What it may do

Nothing beyond its own page: it asks for no permissions. It reads what
you type into its page and its own setting, and writes nothing, anywhere.

## Building

```text
cargo xtask pack extensions/{{folder}} dist
cargo xtask install extensions/{{folder}}
```

`cargo test` checks the counting, the reading time and the setting's
reading (`stats.rs`), and the page: what typing and Esc do to it, and
that it lays out at any size (`page.rs`).
