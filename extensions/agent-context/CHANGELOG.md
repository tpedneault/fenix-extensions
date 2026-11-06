# Changelog

## 1.0.0

The first release, for Fenix 1.1 (extension API 1.2): tools for your
coding agents, in the `agent_context` toolset, offered by default.

- `agent_context_dependencies` (read): the project's direct dependencies
  per ecosystem -- Cargo (a workspace's members too), npm with
  `package-lock.json`, `pnpm-lock.yaml` or `yarn.lock` (classic and
  Berry), Python with `uv.lock`, `poetry.lock`, `pdm.lock`,
  `Pipfile.lock` or `==` pins, and Go's `go.mod` checked against
  `go.sum` -- with what each asks for and the version locked.
- `agent_context_dependency_docs` (external): a dependency's
  documentation for exactly the version locked (a package a dependency
  pulls in, too): its README from crates.io, the npm registry, PyPI or
  pkg.go.dev, or one item's page -- a docs.rs path, a Go symbol, a
  README's section -- as plain text, cut to `agent-context.max_chars`
  and kept under the extension's folder.
- Both as MCP resources: `fenix://ext/agent_context/dependencies` and
  `fenix://ext/agent_context/docs/{package}`.
