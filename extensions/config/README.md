# Config & Shell

The files every project has, beside its code: YAML, JSON, TOML,
Dockerfiles, shell scripts and Markdown. Each gets a language server,
with SchemaStore's schemas for YAML and JSON (GitHub Actions, Compose,
`package.json`, `tsconfig.json`, `Cargo.toml`, `pyproject.toml` and over
a thousand more), formatting, linting, the project doctor's checks with
their fixes, and snippets. Built on lang-kit (`crates/lang-kit`), as each
language pack is.

It starts with Fenix. A server is set for its language whenever it's
found and its setting is on, so a YAML file in any project has it; the
doctor checks only the tools for the kinds of file the project has (a
lone README doesn't count as Markdown).

## The servers

| | Server | Installed by its fix row |
|---|---|---|
| YAML | `yaml-language-server --stdio` | `npm install -g yaml-language-server` |
| JSON | `vscode-json-language-server --stdio` | `npm install -g vscode-langservers-extracted` |
| TOML | `taplo lsp stdio` | Taplo 0.10.0's release |
| Dockerfiles | `docker-langserver --stdio` | `npm install -g dockerfile-language-server-nodejs` |
| Shell scripts | `bash-language-server start` | `npm install -g bash-language-server` |
| Markdown | `marksman server` | Marksman 2026-02-08's release |

The Bash server is the language's own, in place of Fenix's (the same
program, found where Fenix wouldn't look). The others are added beside
whatever else serves their language: Marksman runs beside the notebook's
server, Biome's beside the JSON server in a Biome project.

A release is downloaded from GitHub for this machine (Linux and macOS on
x86_64 and arm64, Windows on x86_64), checked against the SHA-256 the pack
knows, and unpacked into the pack's tools folder, where only the pack
looks. One you installed yourself (`cargo install taplo-cli`, `brew
install marksman`, `go install mvdan.cc/sh/v3/cmd/shfmt@latest`, winget)
is used instead: the pack looks in npm's, Cargo's and Go's folders,
Homebrew's and winget's too. The npm fix rows need Node; without it the
doctor says so.

## Schemas

yaml-language-server reads SchemaStore's catalog itself and picks a
schema by the file's name: `.github/workflows/*.yml` is a workflow,
`compose.yaml` a Compose file. A schema of your own goes in a modeline:

```yaml
# yaml-language-server: $schema=https://example.com/schema.json
```

The JSON server only uses the schemas it's given, so the pack fetches
the catalog (`https://www.schemastore.org/api/json/catalog.json`), keeps
it for a week, and gives the server every schema for JSON files. `SPC m
s` (`config.schemas`) fetches it again. A file's `$schema` key wins.
`config.schemastore` off gives neither server the catalog.

Comments and trailing commas are fine in `*.jsonc`, `tsconfig.json`,
`jsconfig.json`, `.eslintrc.json`, `deno.json`, `turbo.json`, VS Code's
settings, `.babelrc`, `.swcrc` and `devcontainer.json`; the file is still
held to its schema. Anywhere else they're errors, as JSON says.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`:

- **JSON**: Prettier, when it's installed (its fix row: `npm install -g
  prettier`). Not in a Biome project: the Web pack's Biome formats it.
- **YAML** and **Markdown**: Prettier in a project with a Prettier config
  of its own (`.prettierrc*`, `prettier.config.*`, `package.json`'s
  `prettier`); else YAML by its server, and Markdown not at all.
- **TOML**: Taplo's server (`.taplo.toml` is read).
- **Shell scripts**: shfmt, through the Bash server (`.editorconfig` is
  read).

`config.prettier` off leaves JSON, YAML and Markdown to their servers.

## Linting

- **Shell scripts**: ShellCheck, through the Bash server, as you type.
  The doctor's fix row installs ShellCheck 0.11.0.
- **Dockerfiles**: the server's own checks as you type, and hadolint's
  rules when a Dockerfile is saved (`config.hadolint`), with ShellCheck's
  for the shell in its `RUN`s. The fix row installs hadolint 2.15.1.

Tasks (`SPC p t`): *shellcheck: every script* and *hadolint: every
Dockerfile*, when the project has some and the tool is installed.

## Settings

| Setting | Default | |
|---|---|---|
| `config.yaml`, `config.json`, `config.toml`, `config.dockerfile`, `config.shell`, `config.markdown` | on | each kind's server and checks |
| `config.schemastore` | on | SchemaStore's schemas for YAML and JSON |
| `config.prettier` | on | Prettier formats JSON (and YAML and Markdown with a Prettier config) |
| `config.hadolint` | on | hadolint on a saved Dockerfile |

## Files

Besides the names Fenix knows, these are read as:

- **YAML**: `.clang-format`, `.clang-tidy`, `.clangd`, `.yamllint`
- **JSON**: `*.jsonc`, `*.webmanifest`, `*.code-workspace`, `*.geojson`,
  `*.har`, `*.jsonld`, `.babelrc`, `.eslintrc`, `.jshintrc`, `.swcrc`,
  `.watchmanconfig`
- **TOML**: `Cargo.lock`, `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile`
- **Shell scripts**: `*.bats`, `*.ksh`, `.bashrc`, `.bash_profile`,
  `.bash_aliases`, `.bash_logout`, `.profile`, `PKGBUILD`, `APKBUILD`
- **Markdown**: `*.mdown`, `*.mkd`, `*.mkdn`, `*.mdwn`

## Snippets

| Language | Keys |
|---|---|
| YAML | `workflow`, `job`, `step`, `matrix` (GitHub Actions), `compose`, `service` (Compose), `deployment`, `kservice` (Kubernetes) |
| Dockerfile | `multistage`, `apt`, `healthcheck` |
| Shell | `strict`, `fn`, `if`, `for`, `case`, `args` (getopts), `trap`, `here` |
| Markdown | `table`, `details`, `code`, `note` |

## Keys

After `SPC m` in one of its files: `d` checks the tools again, `s`
fetches SchemaStore's catalog again.

## Not yet

- **PowerShell**: Fenix has no grammar for it; it comes with its own
  grammar and PowerShell Editor Services in a later version.
- **JSON formatting without Prettier**: the JSON server formats only when
  asked to at its start (`initializationOptions`), which Fenix doesn't
  give yet.
