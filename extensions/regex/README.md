# Regex

A playground for regular expressions (`SPC s x`): a pattern, a text to
try it on and a replacement, matched as you type.

- **Matches**, on a background in the text: the whole match the
  selection's, each capture group its own colour from the theme's
  palette, the same colours on the groups in the pattern; an empty match
  underlines the character after it. Under them every match, its line
  and column, and its groups -- numbered and named -- with where each
  matched.
- **In the file you're in, too**: it opens beside it, and while it's
  open the matches are marked there the same way, as you type the
  pattern or the file. Go to another file and they move to it; close the
  playground and they go.
- **The replacement's result**, live: `$1`, `${name}`, `$$` for a `$`.
- **Flags** as toggles: `i` case-insensitive, `m` multi-line, `s` dot
  matches newline, `x` extended, `u` Unicode.
- **The pattern explained**, a piece a row, nested as it nests: `\d{4}`
  "a digit, exactly 4 times".
- **Errors** marked under the pattern where they are, with what's wrong.
- **Flavours**: Rust's `regex`, or `fancy-regex` (`f`) for lookaround,
  atomic groups and backreferences. A note says where JavaScript, Python
  and PCRE read the pattern otherwise: lookbehind, named groups, `\d` and
  Unicode, `$`, inline flags.
- **Copies** for Rust, Python, JavaScript, Go, Java and C# (`y`): the
  string literal each wants, its flags with it, named groups written its
  way. Or the result, or the matches, a line each.
- **Saved patterns** (`w`): by name, for the project you're in, or for
  every project (`W`). `o` opens one, `D` forgets one. They're kept in
  the extension's own folder.
- **The cheatsheet** (`Tab`, or `SPC s X`): classes, anchors, quantifiers
  greedy and lazy, groups, lookaround, backreferences, flags, and recipes
  -- email, URL, IPv4, dates, times, semantic versions, UUIDs, log lines.
  `Enter` loads an example into the playground; `/` filters.

It opens on the regex under the cursor -- a Rust `Regex::new(r"...")`,
a Python `re.compile(r"...")`, a JavaScript `/.../i`, Go, Java or C#;
the nearest when the cursor's on none -- or with the selection as the
text. A line with no regex on it is the text. Open already, the
playground is brought forward with the regex or the selection you're
on; a line with no regex leaves its text as it was.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC s x` | `regex.open` | The playground |
| `SPC s X` | `regex.cheatsheet` | The cheatsheet |

On the playground:

| Key | What it does |
| --- | --- |
| `j` `k` | Move between the fields and the matches |
| `Enter` `e` `a` | Type in the field (`a` at its end); on a match, copy it |
| `c` `d` | Clear the field and type in it; clear it |
| `Tab` `Shift-Tab` | While typing: the next field, the last |
| `Esc` | Stop typing |
| `i` `m` `s` `x` `u` | Flags |
| `f` | Rust `regex` or `fancy-regex` |
| `y` | Copy the pattern for a language, the result or the matches |
| `w` `W` | Save the pattern for the project, for every project |
| `o` `D` | Open a saved pattern, forget one |
| `Tab` `?` | Not typing: the cheatsheet, and back |
| PageUp PageDown | A screen of the page; typing the text, of its lines |
| Ctrl+Z, Ctrl+Shift+Z (Ctrl+Y) | Undo, redo in the fields: characters typed on in one field are one step |

With the mouse: a click in a field types there, where you clicked; on a
match picks it, and a double click copies it; on a flag, the flavour or
a tab, that. In the cheatsheet a click picks an entry and a double click
tries it. The wheel scrolls the page.

## What it may do

- **Where you are:** read the line or selection you open it on, where
  the cursor is on it, the file's language and its project.
- **The file you're in:** be told which it is and when it's typed in
  (`events = ["focus", "changed"]`), read its text, and mark its
  matches in it while the playground's open.
