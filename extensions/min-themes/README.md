# Min themes

Miguel Solorio's Min, the minimal colour themes for VS Code, in Fenix:
Min Light and Min Dark. Pick one with `SPC t p`.

## Coming from VS Code

Each theme has the name VS Code gives it, so Fenix's settings import
(`SPC h i`, in Fenix's nightlies and its next release) brings
`"workbench.colorTheme": "Min Dark"` over as Min Dark once this is
installed, rather than the nearest theme Fenix ships.

## Where the colours come from

From [miguelsolorio/min-theme](https://github.com/miguelsolorio/min-theme)
at commit `dd796c3f294daecfb7f10ed277ffdc30517d59cf` (its `package.json`
says 1.5.0): `themes/min-light.json` and `themes/min-dark.json`, each
converted by `cargo xtask theme-import` with the name its `package.json`
gives it, and not edited after. Each theme file's header says which file
it was made from.

The themes are under the MIT licence, Copyright (c) 2018-2021 Miguel
Solorio: [`LICENSE-min-theme`](LICENSE-min-theme), copied from the
repository at that commit. This pack is under the same licence.

To make them again, from the repository's root, for each theme:

```text
cargo xtask theme-import min-dark.json --out extensions/min-themes/themes --name "Min Dark" --source "<its URL at the commit>"
```

## What the import kept, and what it couldn't

As published: the colours are Min's own, not darkened or lightened for
contrast. Min keeps its comments and line numbers very quiet: in Min
Light 1.8:1 and 1.6:1 on white, in Min Dark 3.4:1 -- Fenix's own themes
keep 4.5:1.

**From the theme**: the editor's page, text (Min Dark's from its
TextMate rules' default, as VS Code reads it), the tab strip's colour for
Fenix's popups and status line, the side bar, borders, and every syntax
colour, from Min's TextMate rules, matched the way VS Code matches them.
Min colours no variables: they're body text, as VS Code draws them.
Min Light's bracket match is its own, and both themes' grey for ignored
files.

**Worked out**, since Min leaves VS Code's own in place or VS Code has
none:

- the caret, VS Code's cursor in both: the text colour;
- the current line: Min draws a frame round it; here its colour shades
  the line;
- the selection: the caret's colour at a quarter; search matches:
  Replace's amber at 30%; in Min Dark, the bracket match: Insert's green
  at a fifth;
- the mode badges from the theme's colours by hue -- in Min Light its
  number blue, string green, terminal purple, yellow and cyan; in Min
  Dark its constant blue, function purple and string orange, with green
  and cyan made by turning that blue's hue. The lettering is the page
  colour, or black where that reads better;
- the Git colours Min doesn't set: modified its amber or orange, added
  (Fenix's staged) and untracked its greens, conflicted its red, and in
  Min Dark green made the same way as above;
- the folder icons: Normal's blue (in VS Code they come with an icon
  theme, not a colour theme); file names: the text colour.

**Left out, for this pack's Fenix 1.0 floor**: Min Light's 16 terminal
colours (Fenix reads a theme's own from 1.4; until then it makes them
from the theme's Git and syntax colours), and the TextMate colours for
interfaces and macros (1.5.2).

**Not mapped**: 76 (Min Dark) and 97 (Min Light) workbench colours
Fenix's themes have no place for (the activity bar, badges, buttons,
lists, inputs, the status bar's own, ...); a dozen TextMate rules no
Fenix role takes its colour from (Markdown, log levels, function
arguments and parameters); font styles (bold, italic, underline --
Fenix's themes have none).

## What it may do

Nothing. It's data alone: two theme files and a licence, no code, and
it asks for no permissions.
