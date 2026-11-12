# {{name}}

A few shortcuts laid over Fenix's standard keys. It's data alone: one
keymap file, no code.

Fenix's standard keys are VS Code's. This only names the shortcuts where
yours differ, and only for commands Fenix has; everything else stays
Fenix's. Vim keys are left as they are.

<!-- TODO(you): say whose shortcuts these are, and what has no
equivalent in Fenix. -->

## On and off

Install it from the store (`Ctrl+K h x`), with standard keys on
(`Ctrl+K t k` switches between them and Vim keys). It's in use at once.
Switching it off on the same page brings Fenix's own keys back.

`Ctrl+K h ?` lists the chords in use, each of this extension's marked
`from {{folder}}`. `Ctrl+K h K` lists anything wrong with a keymap. Your
own `keybindings.toml` goes over this one, so a chord you'd rather have
as it was is a `[[keys]]` entry there.

It needs Fenix 1.5, the first to read `[[keys]]` in an extension's
keymap.

## Keys

In a file's text.

| Shortcut | Command | What it does |
|---|---|---|
| `Shift+F6` | `code.lsp_rename` | Rename the name under the cursor |
| `Alt+F7` | `code.references` | List where it's used |
| `Ctrl+Alt+B` | `code.implementations` | Go to where it's implemented |

## What it may do

Nothing. It's data alone: one keymap file, no code, and it asks for no
permissions.
