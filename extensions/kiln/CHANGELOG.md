# Changelog

## 1.0.0

- The first release, for Fenix 1.6 (extension API 1.5): a folder with a `kiln.toml` is a Kiln project (`KILN`), with `Kiln: open the plan` and `Kiln: read the project again`, the settings `kiln.path` and `kiln.refresh_seconds`, and an `SPC m` menu. The plan page says the project's state: kiln found (1.0.0 or later) or how to set `kiln.path`, the project's items, milestones' progress and findings, or kiln's error as it wrote it; it's read again on `r`, a save, a new git head, and while in view every `kiln.refresh_seconds`. Kiln's language server (`kiln lsp`) is registered as the project loads, for the languages `kiln lsp --languages` names, or Markdown only with a kiln that doesn't have it (1.0.0), said once on the output page; none when `.fenix/tools.json` already runs kiln, in either form; set again only when the languages or the kiln change, and taken away when kiln is missing, too old or finds no `kiln.toml`. The plan's full pages are not in yet.
