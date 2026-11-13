# Winter is Coming themes

John Papa's Winter is Coming, the cool blue colour themes for VS Code,
in Fenix: Dark Blue, Dark Black and Light. Pick one with `SPC t p`.

## Coming from VS Code

Each theme has the name VS Code gives it, so Fenix's settings import
(`SPC h i`, in Fenix's nightlies and its next release) brings
`"workbench.colorTheme": "Winter is Coming (Dark Blue)"` over as that
theme once this is installed, rather than the nearest theme Fenix ships.
VS Code's "No Italics" versions differ from these only in italics, which
Fenix's themes don't have; the import takes each to the theme its name
begins with ("the nearest Fenix has").

## Where the colours come from

From [johnpapa/vscode-winteriscoming](https://github.com/johnpapa/vscode-winteriscoming)
at commit `260547834cb6ac37dd5b8bb5842cc1c8d3164946` (its `package.json`
says 1.5.0): `themes/WinterIsComing-dark-blue-color-theme.json`,
`WinterIsComing-dark-color-theme.json` and
`WinterIsComing-light-color-theme.json`, each converted by
`cargo xtask theme-import` with the name its `package.json` gives it,
and not edited after. Each theme file's header says which file it was
made from.

The themes are under the MIT licence, Copyright (c) 2015-2017
JohnPapa.net, LLC: [`LICENSE-winter-is-coming`](LICENSE-winter-is-coming),
the repository's `LICENSE.md` at that commit (its `package.json` says
"SEE LICENSE IN LICENSE.md"). This pack is under the same licence.

To make them again, from the repository's root, for each theme:

```text
cargo xtask theme-import WinterIsComing-dark-blue-color-theme.json --out extensions/winter-is-coming-themes/themes --name "Winter is Coming (Dark Blue)" --source "<its URL at the commit>"
```

## What the import kept, and what it couldn't

As published: the colours are Winter is Coming's own. Its light theme's
line numbers are 3.9:1 on white, a little under the 4.5:1 Fenix's own
themes keep; the rest reads at that or better.

**From the theme**: the editor's page and text, the colour of its popups
(the dark themes) or its tab strip (the light one) for Fenix's popups and
status line, the side bar and its text, borders, the caret, current
line, selection, bracket match and search matches, line numbers, every
syntax colour, from its TextMate rules, matched the way VS Code matches
them, and the Git colours it sets (modified and untracked). It colours
no punctuation: that's body text, as VS Code draws it. The dark themes'
active tab has the accent along its top they draw there.

**Worked out**, since VS Code has no colour for them:

- the mode badges, from the theme's colours by hue: Normal blue, Insert
  green, Visual purple, Replace amber, Command cyan -- from its syntax,
  terminal and Git colours. The lettering is the page colour, or black
  where that reads better;
- the Git colours it doesn't set: added (Fenix's staged) its green,
  conflicted its red, ignored the line numbers' colour;
- the folder icons: Normal's blue (in VS Code they come with an icon
  theme, not a colour theme).

**Left out, for this pack's Fenix 1.0 floor**: its 16 terminal colours
(Fenix reads a theme's own from 1.4; until then it makes them from the
theme's Git and syntax colours), and its TextMate colours for
interfaces, parameters and macros (1.5.2).

**Not mapped**: about 150 workbench colours Fenix's themes have no
place for (buttons, lists, inputs, notifications, peek views, the
status bar's bright blue -- Fenix's status line is drawn on the popups'
colour, with the text on it -- ...); 59 TextMate rules no Fenix role
takes its colour from (most for Markdown, JSON, TypeScript and
JavaScript); and its font styles (16 italic rules, bold, underline,
strikethrough -- Fenix's themes have none).

## What it may do

Nothing. It's data alone: three theme files and a licence, no code, and
it asks for no permissions.
