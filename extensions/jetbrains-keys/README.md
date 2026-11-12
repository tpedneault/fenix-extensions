# JetBrains keys

The IntelliJ platform's default shortcuts -- the Windows and Linux
keymap IntelliJ IDEA, PyCharm, CLion, Rider and the rest share -- laid
over Fenix's standard keys. It's data alone: one keymap file, no code.

Fenix's standard keys are VS Code's. This only names the shortcuts where
IntelliJ's differ, and only for commands Fenix has; everything else
stays Fenix's. Vim keys are left as they are.

## On and off

Install it from the store (`Ctrl+K h x`), with standard keys on
(`Ctrl+K t k` switches between them and Vim keys). It's in use at once.
Switching it off on the same page brings Fenix's own keys back.

`Ctrl+K h ?` lists the chords in use, each of this extension's marked
`from jetbrains`. `Ctrl+K h K` lists anything wrong with a keymap.
Your own `keybindings.toml` goes over this one, so a chord you'd rather
have as it was is a `[[keys]]` entry there.

It needs Fenix 1.6 (the nightly counts): before that, the debugger kept `F9` for
its breakpoint, whatever a keymap said.

## The keys

In a file's text unless it says anywhere.

### Navigation

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Go to declaration | `Ctrl+B` | `code.goto_definition` |
| Go to implementation | `Ctrl+Alt+B` | `code.implementations` |
| Find usages | `Alt+F7` | `code.references`: the list of uses |
| Show usages | `Ctrl+Alt+F7` | `code.references` |
| Call hierarchy | `Ctrl+Alt+H` | `code.callers` |
| Go to class | `Ctrl+N` | `code.project_symbols`, anywhere |
| Go to symbol | `Ctrl+Alt+Shift+N` | `code.project_symbols`, anywhere |
| Go to file | `Ctrl+Shift+N` | `project.find_file`, anywhere |
| File structure | `Ctrl+F12` | `code.breadcrumbs`: this file's scopes |
| Back / forward | `Ctrl+Alt+Left` / `Ctrl+Alt+Right` | `nav.jump_back` / `nav.jump_forward`, anywhere |
| Previous / next tab | `Alt+Left` / `Alt+Right` | `tab.prev` / `tab.next`, anywhere |
| Close tab | `Ctrl+F4` | `tab.close`, anywhere |
| Next / previous error | `F2` / `Shift+F2` | `nav.next_problem` / `nav.prev_problem` |
| Next / previous occurrence | `Ctrl+Alt+Down` / `Ctrl+Alt+Up` | `project.quickfix_next` / `project.quickfix_prev`: the search's or the references' list, anywhere |
| Matching brace | `Ctrl+Shift+M` | `nav.matching_bracket` |

### Editing

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Duplicate line | `Ctrl+D` | `edit.copy_lines_down` |
| Delete line | `Ctrl+Y` | `edit.delete_line` |
| Move line up / down | `Alt+Shift+Up` / `Alt+Shift+Down` | `edit.move_lines_up` / `edit.move_lines_down` |
| Move statement up / down | `Ctrl+Shift+Up` / `Ctrl+Shift+Down` | `edit.move_lines_up` / `edit.move_lines_down`: lines, not statements |
| Join lines | `Ctrl+Shift+J` | `edit.join_lines` |
| Start new line | `Shift+Enter` | `edit.new_line_below` |
| Start new line before current | `Ctrl+Alt+Enter` | `edit.new_line_above` |
| Add selection for next occurrence | `Alt+J` | `edit.add_next_occurrence` |
| Select all occurrences | `Ctrl+Alt+Shift+J` | `edit.select_all_occurrences` |
| Collapse / expand | `Ctrl+Minus` / `Ctrl+=` (or `Ctrl++`) | `code.fold` / `code.unfold` |
| Insert live template | `Ctrl+J` | `insert.snippet`: Fenix's snippets |
| Copy path | `Ctrl+Shift+C` | `file.yank_path` |

### Code and refactoring

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Show context actions | `Alt+Enter` | `code.lsp_code_action` |
| Reformat code | `Ctrl+Alt+L` | `code.format_buffer`: the extension's or language server's formatter, else a reindent |
| Rename | `Shift+F6` | `code.lsp_rename` |
| Quick documentation | `Ctrl+Q` | `code.hover` |

### Search

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Replace | `Ctrl+R` | `search.replace_buffer`, anywhere |
| Replace in files | `Ctrl+Shift+R` | `search.replace_project`, anywhere |
| Find action | `Ctrl+Shift+A` | `commands.palette`, anywhere |

`Ctrl+F`, `Ctrl+Shift+F`, `F3`, `Shift+F3`, `Ctrl+G`, `Ctrl+E`,
`Ctrl+/`, `Ctrl+Tab` and `Ctrl+Shift+Z` are the same in both, and stay
Fenix's.

### Run and debug

Anywhere.

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Run | `Shift+F10` | `task.rerun_last`: the last task again |
| Run... | `Alt+Shift+F10` | `task.run`: pick a task |
| Run context configuration | `Ctrl+Shift+F10` | `task.run_test_at_cursor`, in a file's text |
| Debug | `Shift+F9` | `debug.start_or_continue` |
| Debug... | `Alt+Shift+F9` | `debug.pick_launch` |
| Resume | `F9` | `debug.start_or_continue` |
| Step over | `F8` | `debug.step_over` |
| Step into | `F7` | `debug.step_into` |
| Step out | `Shift+F8` | `debug.step_out` |
| Run to cursor | `Alt+F9` | `debug.run_to_cursor` |
| Evaluate expression | `Alt+F8` | `debug.evaluate` |
| Toggle line breakpoint | `Ctrl+F8` | `debug.toggle_breakpoint` |
| Stop | `Ctrl+F2` | `debug.stop`: the debug session (a task is `task.kill`, which has no key here) |
| Rerun | `Ctrl+F5` | `debug.restart` |

### Tool windows

Anywhere.

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Project | `Alt+1` | `explorer.toggle_sidebar` |
| Select in | `Alt+F1` | `explorer.reveal_current`: the file, in the sidebar |
| Find | `Alt+3` | `search.last`: the Search page as you left it |
| Debug | `Alt+5` | `debug.toggle_dock` |
| Problems | `Alt+6` | `code.problems` |
| Git | `Alt+9` | `git.log` |
| Commit | `Alt+0` | `git.open`: Git's page, where you stage and commit |
| Terminal | `Alt+F12` | `terminal.toggle` |
| Settings | `Ctrl+Alt+S` | `settings.open` |
| Project structure | `Ctrl+Alt+Shift+S` | `project.settings` |

### Version control

| IntelliJ | Shortcut | In Fenix |
|---|---|---|
| Update project | `Ctrl+T` | `git.pull_rebase`, anywhere: it pulls at once, rebasing your commits, with no dialog first |
| Rollback lines | `Ctrl+Alt+Z` | `git.hunk_discard`: the changed hunk at the cursor |
| Next / previous change | `Ctrl+Alt+Shift+Down` / `Ctrl+Alt+Shift+Up` | `nav.next_hunk` / `nav.prev_hunk` |

### Taken away

These are Fenix's standard keys, which IntelliJ uses for something Fenix
has no command for. Left as they were they'd do something else, and
most would edit your text, so with this on they do nothing:

| Shortcut | IntelliJ | Fenix's, without this |
|---|---|---|
| `Ctrl+W` | Extend selection | Close the tab (`Ctrl+F4` here) |
| `Alt+Up` / `Alt+Down` | Previous / next method | Move lines (`Alt+Shift+Up` / `Alt+Shift+Down` here) |
| `Ctrl+[` / `Ctrl+]` | Code block start / end | Outdent / indent |
| `Ctrl+Shift+K` | Push | Delete the line (`Ctrl+Y` here) |
| `Ctrl+Q` outside a file's text | Quick documentation | Quit Fenix |

A few of Fenix's standard keys move to make room: next occurrence is
`Alt+J` rather than `Ctrl+D`, the sidebar is `Alt+1` rather than
`Ctrl+B` in a file's text, the terminal `Alt+F12` rather than `Ctrl+J`
there, the next problem `F2` rather than `F8`, rename `Shift+F6` rather
than `F2`, adding a cursor above or below has no key (Fenix's is
`Ctrl+Alt+Up` / `Ctrl+Alt+Down`, which go through the list here), and
the text size is `Ctrl+=` and `Ctrl+Minus` only outside a file's text.
Everything is still in the palette and under `Ctrl+K`.

## What it doesn't have

IntelliJ shortcuts with nothing in Fenix to run, which keep whatever
Fenix's standard keys give them (often nothing):

- Search everywhere (`Shift` twice): a key pressed twice isn't a chord.
  `Ctrl+Shift+A` and `Ctrl+Shift+N` cover much of it.
- Clone caret above / below (`Ctrl` twice, then an arrow), and adding a
  caret with `Alt+Shift+Click`.
- Basic and smart completion (`Ctrl+Space`, `Ctrl+Shift+Space`), and
  complete current statement (`Ctrl+Shift+Enter`, which stays Fenix's new
  line above).
- Parameter info (`Ctrl+P`, which stays Fenix's go to file), quick
  definition (`Ctrl+Shift+I`), expression type (`Ctrl+Shift+P`, Fenix's
  palette).
- Go to type declaration (`Ctrl+Shift+B`), super method (`Ctrl+U`,
  which stays Fenix's cursor undo), test (`Ctrl+Shift+T`, which stays
  Fenix's reopen tab), last edit location (`Ctrl+Shift+Backspace`),
  recent locations (`Ctrl+Shift+E`, which stays Fenix's file in the
  sidebar).
- Type and method hierarchy (`Ctrl+H`, `Ctrl+Shift+H`, which stay
  Fenix's replace and replace in the project).
- Highlight usages in file (`Ctrl+Shift+F7`).
- Shrink selection (`Ctrl+Shift+W`), as there's no extend selection.
- Block comment (`Ctrl+Shift+/`), toggle case (`Ctrl+Shift+U`: Fenix has
  upper and lower case apart), auto-indent lines (`Ctrl+Alt+I`), optimize
  imports (`Ctrl+Alt+O`), surround with (`Ctrl+Alt+T`), generate
  (`Alt+Insert`), override and implement methods (`Ctrl+O`, which stays
  Fenix's open a file by path, and `Ctrl+I`).
- Refactor this, extract variable, method, field, constant and
  parameter, inline, move, copy, change signature, safe delete. A
  language server may offer some of them as code actions (`Alt+Enter`).
- View breakpoints (`Ctrl+Shift+F8`), smart step into (`Shift+F7`), and
  debug the context's configuration (`Ctrl+Shift+F9`).
- Bookmarks (`F11`, `Ctrl+F11`, `Shift+F11`, `Ctrl+1` to `Ctrl+9`, which
  stay Fenix's panes).
- Push (`Ctrl+Shift+K`), the VCS operations popup (`` Alt+` ``), and
  show diff.
- New scratch file (`Ctrl+Alt+Shift+Insert`): Fenix's keymaps can't
  name `Insert`. `Ctrl+N` was Fenix's new scratch buffer; it's in the
  palette (`buffer.scratch`).
- Synchronize (`Ctrl+Alt+Y`): Fenix's reload throws away unsaved edits,
  which IntelliJ's doesn't, so it isn't bound.
- Hide all tool windows (`Ctrl+Shift+F12`), jump to the last tool
  window (`F12`, which stays Fenix's go to definition).

## What a keymap can't take

- **`Ctrl+K`** is Fenix's leader, so IntelliJ's commit isn't there:
  `Alt+0` opens Git's page to commit from. **`Ctrl+Shift+P`**, **`Ctrl+\`**
  and **`Alt+X`** are Fenix's too.
- **The debugger's own F-keys.** Fenix's debugger has `F5` (start or
  continue), `Shift+F5` (stop), `Ctrl+Shift+F5` (restart), `F6` (pause),
  `F10` (step over), `F11` (step into) and `Shift+F11` (step out) in
  every mode. A keymap can bind one, but taking one away gives it back
  to the debugger. So IntelliJ's copy (`F5`), move (`F6`) and bookmark
  (`F11`) keys run the debugger's instead.
- **A terminal that has the keyboard.** Only Fenix's chords and your own
  reach past it, so `Alt+F12` opens the terminal but doesn't close it
  from inside; Fenix's `Ctrl+J` and ``Ctrl+` `` still do.
- **Your desktop's keys.** On Linux the desktop often takes `Ctrl+Alt+L`
  (lock the screen), `Ctrl+Alt+Left` and `Ctrl+Alt+Right` (workspaces)
  and `Ctrl+Alt+F1` to `F12` (consoles) before any program sees them, as
  it does with IntelliJ.
