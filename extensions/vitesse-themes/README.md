# Vitesse themes

Anthony Fu's Vitesse, the muted, warm colour themes for VS Code, in
Fenix: two light and three dark. Pick one with `SPC t p`.

| Theme | Light or dark |
|---|---|
| Vitesse Light | light |
| Vitesse Light Soft | light, on a warm grey page |
| Vitesse Dark | dark |
| Vitesse Dark Soft | dark, on a lighter grey |
| Vitesse Black | dark, on black |

## Coming from VS Code

Each theme has the name VS Code gives it, so Fenix's settings import
(`SPC h i`, in Fenix's nightlies and its next release) brings
`"workbench.colorTheme": "Vitesse Dark"` over as Vitesse Dark once this
is installed, rather than the nearest theme Fenix ships.

## Where the colours come from

From [antfu/vscode-theme-vitesse](https://github.com/antfu/vscode-theme-vitesse)
at commit `2862595c3d5d05fabfd5aeb50bcbb79ea2f8d85f` (its `package.json`
says 1.0.1): `themes/vitesse-light.json`, `vitesse-light-soft.json`,
`vitesse-dark.json`, `vitesse-dark-soft.json` and `vitesse-black.json`,
each converted by `cargo xtask theme-import` with the name its
`package.json` gives it, and not edited after. Each theme file's header
says which file it was made from.

The themes are under the MIT licence, Copyright (c) 2020 Primer and
Copyright (c) 2021 Anthony Fu: [`LICENSE-vitesse`](LICENSE-vitesse),
copied from the repository at that commit. This pack is under the same
licence.

To make them again, from the repository's root, for each theme:

```text
cargo xtask theme-import vitesse-dark.json --out extensions/vitesse-themes/themes --name "Vitesse Dark" --source "<its URL at the commit>"
```

## What the import kept, and what it couldn't

As published: the colours are Vitesse's own, not darkened or lightened
for contrast. Vitesse keeps its comments and line numbers quiet, and
below the 4.5:1 Fenix's own themes keep -- comments 2.0:1 to 4.2:1 on
their page, line numbers 1.8:1 to 2.4:1.

**From the theme**: the editor's page, text, popups, side bar, borders,
current line, selection, bracket match and search matches; every syntax
colour, from Vitesse's own TextMate rules, matched the way VS Code
matches them; the Git colours. The active tab has the accent along its
top that Vitesse draws there.

**Worked out**, since VS Code has no colour for them:

- the caret: Vitesse leaves VS Code's cursor as it is; here it's the
  text colour;
- the mode badges, from Vitesse's terminal and link colours: Normal its
  blue, Insert its green, Replace its yellow, Command its cyan. Vitesse
  has no purple, so Visual's badge (and the picker's) is one of its
  colours turned to purple: the dark themes' yellow, the light themes'
  teal. The badges' lettering is the page colour, or black where that
  reads better;
- the folder icons: Normal's blue (in VS Code they come with an icon
  theme, not a colour theme).

**Left out, for this pack's Fenix 1.0 floor**: Vitesse's 16 terminal
colours (Fenix reads a theme's own from 1.4; until then it makes them
from the theme's Git and syntax colours), its six bracket pair colours
(1.5; until then rainbow brackets are its function, keyword and type
colours), and its semantic colour for interfaces with its TextMate ones
for parameters and macros (1.5.2).

**Not mapped**: some 170 workbench colours Fenix's themes have no place
for (buttons, lists, badges, inputs, notifications, the activity bar,
scroll bars, diff and peek views, ...); 45 TextMate rules no Fenix role
takes its colour from (most for one language: TypeScript, JSON, YAML,
Markdown, HTML, Vue); font styles (bold, italic, underline -- Fenix's
themes have none); the backgrounds five rules give their tokens; and
Monaco's own `rules`, which VS Code doesn't read either.

## What it may do

Nothing. It's data alone: five theme files and a licence, no code, and
it asks for no permissions.
