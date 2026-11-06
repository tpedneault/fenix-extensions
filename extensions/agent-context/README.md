# Agent context

Context for your coding agents (Claude Code, Codex, any agent talking to
Fenix over MCP) that Fenix itself doesn't give them: what the project
depends on, at the versions its lockfiles hold, and the documentation
for exactly those versions -- so an agent reads the API it's calling, not
the one it remembers.

## Tools

They're in the `agent_context` toolset, offered to agents by default.

| Tool | Class | What it answers |
| --- | --- | --- |
| `agent_context_dependencies` | read | The direct dependencies of each manifest in the project (its top folder, and the folders right under it): what each asks for, the version locked, and whether it's a dev, build, optional or peer one |
| `agent_context_dependency_docs` | external | A dependency's documentation for the version locked: its README, or one item's docs |

`dependency_docs` takes `package` (as the manifest names it), and
optionally `item`, `ecosystem`, `version` (only for a package the
project doesn't lock) and `max_chars`.

| Ecosystem | Manifests and lockfiles | Documentation |
| --- | --- | --- |
| Cargo | `Cargo.toml` (a workspace's members, `workspace = true`), `Cargo.lock` | crates.io's README for that version; an item (`sync::Mutex`) from docs.rs's rustdoc for it; the crate's docs.rs front page when it has no README |
| npm | `package.json` with `package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml` or `yarn.lock` | The npm registry's README for that version, else the one published in the package (jsDelivr) |
| Python | `pyproject.toml` (PEP 621, dependency groups, Poetry) or `requirements*.txt`, with `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock` or `==` pins | PyPI's description of that release |
| Go | `go.mod`, checked against `go.sum` | pkg.go.dev at that version, a package or symbol of it (`cobra.Command`); the module proxy's `.info` and `go.mod` when pkg.go.dev hasn't got it |

A package a dependency pulls in is found in the lockfile too. HTML is
made plain text -- headings as `#`, lists as `-`, code as it's laid out,
badges, navigation and scripts left out -- and cut at
`agent-context.max_chars` (20000), saying so. What's fetched is kept in
the extension's folder, since a version's documentation doesn't change;
**Agent context: forget the documentation kept** clears it.

## Resources

For an agent whose client reads MCP resources:
`fenix://ext/agent_context/dependencies`, and
`fenix://ext/agent_context/docs/{package}` (a scoped npm name as
`%40scope%2Fname`).

## Permissions

It reads the project's manifests and lockfiles, and reaches only the
registries above. Fenix's own tools already give agents the problems,
the language servers, the debugger, Git, search and tasks; this adds
nothing of those.
