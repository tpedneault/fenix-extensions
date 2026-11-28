# Changelog

## 1.0.0

The first release, for Fenix 1.6 (extension API 1.5). It runs the `kiln`
program, 1.0.0 or later, and shows what kiln says; it never writes an item
file itself.

**Opening a Kiln project**

- A folder with a `kiln.toml` at its top is a `KILN` project, with an `SPC
  m` menu and the settings `kiln.path` and `kiln.refresh_seconds`.
- Kiln's language server (`kiln lsp`) is registered as the project loads, for
  the languages `kiln lsp --languages` names, or for Markdown only with a
  kiln that doesn't have it (1.0.0), said once on the output page. None is
  added when `.fenix/tools.json` already runs kiln, and it is taken away
  when kiln is missing, too old or finds no `kiln.toml`.
- The project doctor has Kiln's rows: kiln and its version (with a fix row
  that opens Kiln's install page when it isn't found), `kiln.toml` read or
  kiln's error, the language server, and the last `kiln check`'s counts with
  a fix row that opens the plan.
- Tasks `kiln: check`, `kiln: export --check` and `kiln: export`, the last
  two naming the project's file targets only (`kiln export -- NAME…`); none
  publishes to Confluence.
- The modeline says the last check's findings (`kiln 1 error, 2 warnings`).
- After an item ID in a file of the project, its title and status, muted
  (`[[REQ-014]] · Export as CSV · approved`), for the lines in view, never
  written into the file. `kiln.decorations` turns it off.

**The plan**

- `Kiln: open the plan` opens one page with eleven tabs: Tree, Trace, Impact,
  Board, Roadmap, Decisions, Changes, Item, Check, Exports and Comments, in
  Kiln's GUI's order, reached with `Tab`, `[` `]` or `1`–`9` and `0`. The
  project's name and the check's errors are on its top line. It is read
  again on `r`, on a save, on a new git head, and every
  `kiln.refresh_seconds` while in view; a failed read keeps the last one on
  screen with kiln's line.
- Tree nests every open item by `parent`; Trace shows each requirement's
  coverage and its gaps (`m`, `T`); Impact shows what reaches an item; Board
  groups work by status, saying `ready`, `waits on …` and who claimed it;
  Roadmap lists the milestones by due date with their progress and open
  work; Decisions is the log with `supersedes` both ways; Changes is `kiln
  diff` since a baseline `b` asks for; Item follows links with `v` and goes
  back and forward with `<` `>`; Check lists the findings by file.
- On every tab the cursor stays on its item across reloads, `/` filters as you
  type, `Enter` opens the item's file at its line, the mouse clicks and
  scrolls, and the tab, cursors, filters and `a` (closed items) are kept
  with the session.
- `Kiln: show the item under the cursor`, `Kiln: impact of the item under
  the cursor` and `Kiln: find an item` work from any buffer (`SPC m v`,
  `SPC m i`, `SPC m f`).

**Editing**

- On the plan, on the item under the cursor: `s` status, `H` / `L` the
  previous / next status on the Board, `l` link, `U` unlink, `n` a new item
  (its file opens), `N` a new child, and `E` for every edit, with title,
  parent, owner, due date and renumber. The same edits are commands from a
  buffer (`SPC m s l U n N E`). Each is picks and prompts in Fenix's own,
  then one kiln command; kiln's line, or its refusal, is shown as it
  wrote it. The choices come from the project's `kiln.toml`.
- `S` and `Kiln: start working on the item here` (`SPC m S`) claim an item
  (`kiln.owner`) and move it to its in-progress status, then clock in on the
  Agenda extension's task for it, made when there is none. Without the
  Agenda only the status changes, and the line says so.

**Exports, Confluence and Jira**

- The Exports tab (`0`) shows `kiln export --dry-run` for each target, one at
  a time, so a Confluence target kiln can't plan shows kiln's line while the
  others show theirs. `w` and `W` write file targets.
- `P` on a Confluence target shows the pages it would write, and publishes
  only when the target's name is typed exactly. `A` runs `kiln auth
  confluence` in a terminal pane: the extension never sees a token. It
  never passes `--force`, `--adopt`, `--prune-removed` or `--allow-shallow`.
- The Comments tab shows the unresolved Confluence comments under each
  target, by the item they are about; `o` opens one in the browser.
- With `[jira] url`, every item row ends with its tickets, read apart from the
  project's reload so a slow Jira never holds it up. `J` takes an item's
  status from its ticket. With `[jira] project`, Exports ends with a `jira`
  target, and `P` on it syncs after the Jira project key is typed.

**For coding agents**

- The `kiln` toolset: `kiln_next`, `kiln_status`, `kiln_list`, `kiln_show`,
  `kiln_trace`, `kiln_impact` and `kiln_check`, offered by default, read
  only, with answers marked as data and cut at 64 KiB on an item's
  boundary.
- `kiln_set`, `kiln_link` and `kiln_new`, class `run` (kiln writes the files,
  so the change doesn't go through Agent changes), asked about by Fenix and
  off until `fenix.kiln.agent_edits` is on. A sign-off status is refused:
  that stays a person's call. Nothing unlinks, renumbers, exports,
  publishes, syncs Jira or signs in.

**Home and a notification**

- A Kiln section on Home with the work that is ready to start, at most four,
  earliest milestone first; picking one opens the plan on it.
- One notification when a save takes `kiln check` from no errors to some,
  with the first error and a way to open the Check tab. `kiln.notify_errors`
  turns it off.

**Safe by construction**

- An edit kiln has started runs to its end, for up to ten minutes, even when
  another kiln is chosen or the page is closed.
- Other people's text, wherever it is shown (a title, a comment, kiln's
  messages, an agent's answer), has escape sequences and invisible
  characters taken out and is never interpreted.
- Not tried yet in a running Fenix or against a real Confluence or Jira: see
  "Known limits" in the README.
