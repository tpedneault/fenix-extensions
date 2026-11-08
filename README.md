# fenix-extensions

Extensions for [Fenix](https://github.com/tpedneault/fenix), and the
registry Fenix installs them from.

```text
extensions/<name>/        one extension: extension.toml, its data, README.md, CHANGELOG.md,
                          and screenshots/ if it likes
xtask/                    validate, pack, and add a release to the index
.github/workflows/        ci.yml (validate and pack), release.yml (release, index, sign)
```

The `registry` branch holds `index.json` and its signature
`index.json.minisig`. Fenix reads them from
`https://raw.githubusercontent.com/tpedneault/fenix-extensions/registry/index.json`
(`extensions.registry` points it at a mirror instead), refuses an index
not signed with the registry key built into it, and refuses a package
whose SHA-256 isn't the one the index gives.

Each release in the index has the manifest's `categories`; a bundle's
also has `bundle`, its members' ids. Bundles are listed under `bundles`,
beside `extensions`, so a Fenix that doesn't know them reads the index as
it did.

## What's here

Every extension below is in the store inside Fenix (`SPC h x`); the
bundles install several at once, and are what the welcome's packs offer.

### Extensions

| Id | Name | What it does |
|---|---|---|
| [`fenix.ada`](extensions/ada) | Ada and SPARK | Ada and SPARK: their grammar, the Ada Language Server for Ada and GNAT project files, Alire's or gprbuild's tasks, GNATprove's findings as problems, the project's checks with fixes that install Alire, GNAT and the server, and snippets |
| [`fenix.agenda`](extensions/agenda) | Agenda | Tasks on a board, a clock for your time, and a week's timesheet -- linked to Jira issues when Jira is there |
| [`fenix.agent-context`](extensions/agent-context) | Agent context | Context for your coding agents: the project's dependencies at the versions its lockfiles hold, and each one's documentation for exactly that version -- crates.io and docs.rs, npm, PyPI, pkg.go.dev -- as tools and resources they call through Fenix |
| [`fenix.arduino`](extensions/arduino) | Arduino | Arduino sketches: build, upload, the serial monitor, debugging, boards, ports, libraries, and the right language server |
| [`fenix.ccsds`](extensions/ccsds) | CCSDS & ECSS | Space packets, transfer frames and CLTUs decoded through the MIB, recordings and live telemetry with a parameter followed through them, on-board times, hex, and the CCSDS and ECSS standards |
| [`fenix.ci`](extensions/ci) | CI status | Your branch's CI on the modeline -- GitHub Actions or GitLab pipelines -- a page of its runs, jobs, steps and logs with re-runs and cancels, a notification when a run finishes, and the failing jobs' logs for your coding agent |
| [`fenix.colours`](extensions/colours) | Colours | Colour literals made visible and editable: a swatch beside each (hex, rgb(), hsl(), oklch(), CSS names, a theme's), a picker that writes it back as it was written, conversions between hex, RGB, HSL and OKLCH, a WCAG contrast checker, and the project's palette |
| [`fenix.config`](extensions/config) | Config & Shell | YAML, JSON, TOML, Dockerfiles, shell scripts and Markdown: their language servers, SchemaStore's schemas for GitHub Actions, Compose, package.json, tsconfig and the rest, Taplo, shfmt and Prettier formatting, ShellCheck and hadolint, the project's checks with their fixes, and snippets |
| [`fenix.containers`](extensions/containers) | Containers | Docker, Podman and wslc: containers, Compose, live logs, performance, images, volumes and networks |
| [`fenix.cpp`](extensions/cpp) | C/C++ | C and C++: clangd with the build's compile_commands.json, debugging each CMake target with lldb-dap or GDB, CMake's presets as tasks, clang-format, clang-tidy on save with its fixes, the project's checks with their fixes, vcpkg and Conan packages, snippets, and vcpkg and Conan projects |
| [`fenix.csv`](extensions/csv) | CSV table | CSV and TSV files as a table: columns lined up, sorted and filtered, cells, rows and columns changed through the file's buffer, column stats, copies as CSV or Markdown -- a million rows as fast as ten |
| [`fenix.data-tools`](extensions/data-tools) | Data tools | JSON, YAML and TOML: format with the project's indent, minify, sort keys, convert between them, query with JSONPath or jq, check against a JSON Schema (SchemaStore's by the file's name), escape strings, copy a key's path, and types from a sample |
| [`fenix.database`](extensions/database) | Database browser | SQLite files, PostgreSQL, MySQL and MariaDB, SQL Server: the schema in a sidebar, the statement under the cursor run from a .sql buffer, results to sort, copy and export, a table's rows paged and changed, history, completion of the schema's names |
| [`fenix.diagrams`](extensions/diagrams) | Diagrams | Mermaid diagrams drawn as you type, their errors on their lines, Fenix's themes and yours, and exports |
| [`fenix.discord`](extensions/discord) | Discord | Discord Rich Presence: the project, the file and its language, what you're debugging or running, and how long you've been at it, on your Discord profile |
| [`fenix.dotenv`](extensions/dotenv) | Env files | Env files (.env): values hidden but on the cursor's line, every env file side by side, profiles switched with a backup kept or given to tasks, a warning when Git would commit one, keys checked against .env.example, and variable names completed after process.env., os.environ[ and the like |
| [`fenix.dotnet`](extensions/dotnet) | C# and .NET | C# and .NET: csharp-ls and its problems, debugging with netcoredbg -- each program built first, a process picked to attach to --, CSharpier, the solution's tasks, the project's checks with their fixes, outdated and vulnerable NuGet packages, snippets, and dotnet new's projects |
| [`fenix.go`](extensions/go) | Go | Go: gopls, debugging with Delve, go vet or golangci-lint on save, gofmt or goimports as the formatter, tasks, the project's checks with their fixes, outdated and vulnerable modules, snippets, and CLI, HTTP service and library projects |
| [`fenix.haskell`](extensions/haskell) | Haskell | Haskell: its grammar, the Haskell Language Server, ormolu or fourmolu, hlint on save with its rewrites as fixes, Cabal's and Stack's tasks for each component, the project's checks with fixes that run ghcup, snippets, and a Cabal project |
| [`fenix.hex`](extensions/hex) | Hex viewer | Binary files as a hex dump: offsets, bytes and their text, read a block at a time so a file of any size opens at once, searched for bytes or text, the cursor's bytes read as integers and floats both ways round and as UTF-8, selections copied as hex, text or code -- and any file at all shown in hex |
| [`fenix.http`](extensions/http) | HTTP client | An HTTP client on .http files (REST Client's and JetBrains'): run the request under the cursor and see its status beside it, collections and history in a sidebar, a request view whose every field edits the file, the response's body, headers, cookies and timing, environments and secrets, OAuth 2.0, chained requests, tests and a runner, WebSocket requests, GraphQL completion from the server's schema, and imports from Postman, Insomnia, OpenAPI and curl |
| [`fenix.images`](extensions/images) | Image viewer | Pictures in a pane: PNG, JPEG, GIF, WebP, BMP and SVG, zoomed and panned, moving ones played, their folder walked, and a preview of the one a line names |
| [`fenix.java`](extensions/java) | Java and Kotlin | Java and Kotlin: Eclipse JDT LS, debugging with java-debug, kotlin-language-server and Kotlin's highlighting, google-java-format and ktlint, Gradle's and Maven's tasks, the project's checks with their fixes, outdated and vulnerable dependencies, snippets, and Gradle, Spring Boot and Maven projects |
| [`fenix.jira`](extensions/jira) | Jira | Your Jira Server or Data Center: searches, issues and their workflow, a new-issue form with the project's real fields -- and the agenda kept in step with it |
| [`fenix.logs`](extensions/logs) | Log viewer | Log files read in a pane: each entry's level and time from the formats logs are written in -- JSON lines, logfmt, syslog, journalctl, env_logger and tracing, log4j, Python's logging -- stack traces folded into the entry they follow, filtered by level, text, pattern, field and time, searched, counted over time, and followed as they grow, read a block at a time so a log of gigabytes opens at once |
| [`fenix.lua`](extensions/lua) | Lua | Lua: lua-language-server, StyLua formatting, selene or luacheck on save, busted, LuaRocks and LÖVE tasks, the project's checks with their fixes, snippets, and LÖVE game and LuaRocks module projects |
| [`fenix.mib`](extensions/mib) | SCOS-2000 MIB | SCOS-2000 MIBs: browse them, telecommands and parameters in your scripts (K, gd, completion, checks), the telecommand insert form with repeated groups and value sets, command sequences, two MIBs compared, and XTCE, C, Python or a Wireshark dissector generated from them |
| [`fenix.nats`](extensions/nats) | NATS | A NATS client: live subjects with their rates and messages, payloads pretty-printed, filters and highlights, publish and request, JetStream's streams, consumers and key-value buckets -- at any message rate |
| [`fenix.notebook`](extensions/notebook) | Notebook | Notes, a daily journal, links and quick captures, kept for you in one folder -- an Obsidian vault works as it is |
| [`fenix.ocaml`](extensions/ocaml) | OCaml | OCaml: its grammars for .ml and .mli, ocaml-lsp-server, ocamlformat where the project has an .ocamlformat, dune's tasks with a dune exec for each executable, debugging bytecode with earlybird, the project's checks with fixes that run opam install, snippets, and a dune project |
| [`fenix.original-light-themes`](extensions/original-light-themes) | Original light themes | The light themes Fenix ships, in their published colours, untouched |
| [`fenix.parquet`](extensions/parquet) | Parquet viewer | Parquet files read in a pane: their rows as a table, read a page at a time so a billion rows open at once, and their schema with each column's type, nulls, range, size and codec -- snappy, zstd, gzip, LZ4 and Brotli |
| [`fenix.pdf`](extensions/pdf) | PDF reader | PDFs read in a pane: continuous pages, zoom, an outline, search, selection, links and marks, and a shelf of the documents you read |
| [`fenix.pets`](extensions/pets) | Pets | Little 3D pets on the modeline: they nap, play fetch, cheer your passing tests -- and Ember burns up when one fails |
| [`fenix.php`](extensions/php) | PHP | PHP: tree-sitter-php's highlighting, Intelephense or Phpactor, the project's Pint or PHP CS Fixer as the formatter, PHPStan on save, Composer's scripts and the project's tools as tasks, the project's checks with their fixes, snippets, and a Composer package project |
| [`fenix.pomodoro`](extensions/pomodoro) | Pomodoro | A focus timer: work rounds and breaks counted down on the modeline, today's rounds and the week's -- clocked into the agenda when it's there |
| [`fenix.protobuf`](extensions/protobuf) | Protobuf & gRPC | Protocol Buffers and gRPC: the grammar, buf's language server, buf format, buf lint on save, buf breaking against the default branch, buf generate and buf build as tasks, the project's checks with fixes that install buf, protoc and grpcurl, snippets, and a gRPC client -- a server's services by reflection or the project's protos, a body made from the method's message, server streams as they come, status, trailers, timing and history |
| [`fenix.python`](extensions/python) | Python | Python: basedpyright with ruff's server beside it, debugging with debugpy (the project's scripts, pytest, Django, Flask, FastAPI, attaching to a process), ruff as the formatter, tasks, the project's checks with their fixes, outdated and vulnerable packages for uv and pip, snippets, and Django and Flask projects |
| [`fenix.regex`](extensions/regex) | Regex | A regex playground: matches and their groups coloured as you type, the replacement's result, the pattern explained and its errors pointed at, copies for six languages, saved patterns -- and a cheatsheet with recipes |
| [`fenix.review`](extensions/review) | Review | Code review on GitHub and GitLab: the reviews waiting, a request with its files, threads, checks and your held comments, submitted with a verdict -- and a new pull request for the branch |
| [`fenix.ruby`](extensions/ruby) | Ruby | Ruby: tree-sitter-ruby's highlighting, folding and indenting, Ruby LSP or Solargraph, RuboCop and Standard formatting and linting with their corrections, Bundler's, Rake's, RSpec's and Rails' tasks, the project's checks with their fixes, and snippets |
| [`fenix.rust`](extensions/rust) | Rust | Rust: rust-analyzer, debugging with lldb-dap or CodeLLDB (each binary and example, the tests in your file, built by cargo first), clippy on save with its suggestions as fixes, rustfmt, tasks from Cargo.toml's targets, the toolchain checked with its fixes, outdated and vulnerable crates, snippets, and an Axum service |
| [`fenix.rust-snippets`](extensions/rust-snippets) | Rust snippets | Snippets for everyday Rust: functions, tests, impls, matches and more |
| [`fenix.swift`](extensions/swift) | Swift | Swift on Linux and macOS: sourcekit-lsp, Swift's highlighting, swift-format or SwiftFormat, SwiftLint on save, SwiftPM's tasks, the project's checks with their fixes, snippets for SwiftUI, concurrency and both test frameworks, and a command-line package |
| [`fenix.tcl`](extensions/tcl) | Tcl | Tcl scripts: Tcl's own usage lines on K, completion of options, subcommands, built-ins and your procs, go to definition, and the project's definitions by name |
| [`fenix.vnc`](extensions/vnc) | VNC | Remote desktops in a pane: the VNC (RFB) machines you list, each kept connected while Fenix runs |
| [`fenix.web`](extensions/web) | Web: JavaScript and TypeScript | JavaScript and TypeScript: typescript-language-server with the CSS, HTML, Tailwind, ESLint and Biome servers beside it, JSON with its schemas from Config & Shell, debugging with js-debug (Node, each of package.json's scripts, a Vite page in Chrome, Vitest and Jest, attaching to a process), Prettier or Biome, package.json's scripts as tasks, the project's checks with their fixes, outdated and vulnerable packages for npm, pnpm, Yarn and bun, snippets, and Next.js, Express, library and static site projects |
| [`fenix.zig`](extensions/zig) | Zig | Zig: its grammar, ZLS, zig fmt, zig ast-check on save, zig build's steps as tasks, the project's checks with fixes that install Zig and ZLS, and snippets |

### Bundles

| Id | Name | What it does |
|---|---|---|
| [`fenix.jira-bundle`](extensions/jira-bundle) | Jira | Jira, with the agenda kept in step |
| [`fenix.kotlin-bundle`](extensions/kotlin-bundle) | Kotlin | Kotlin: its highlighting, kotlin-lsp or kotlin-language-server, ktlint, Gradle's tasks, snippets and Kotlin DSL projects -- the Java and Kotlin pack |
| [`fenix.lab-bundle`](extensions/lab-bundle) | Lab | Arduino, containers, VNC |
| [`fenix.mission-bundle`](extensions/mission-bundle) | Mission | SCOS-2000 MIB, CCSDS & PUS, Tcl |
| [`fenix.personal-bundle`](extensions/personal-bundle) | Personal | Notebook, Diagrams, Agenda |
| [`fenix.reading-bundle`](extensions/reading-bundle) | Reading | The PDF reader |
| [`fenix.review-bundle`](extensions/review-bundle) | Review | GitHub pull requests, GitLab merge requests |

## Making an extension

An extension carries data -- themes, snippets, project templates and
keymaps, laid out like your own in Fenix's settings folder -- or code: a
WebAssembly component built with `fenix-extension-api` (Fenix's
`docs/WRITING_EXTENSIONS.md`), like `agenda`, `jira`, `containers`,
`arduino` and `pets` here. A code extension's folder holds its crate;
`cargo xtask pack` builds it for `wasm32-wasip2` (`rustup target add
wasm32-wasip2`) and leaves the source out of the package. Crates more than
one extension uses go under `crates/` (`crates/jira`, the Jira client the
agenda and Jira share; `crates/ui-kit`, the widgets canvas pages are
built from; `crates/lang-kit`, what the language packs share: the
package manager page, doctor checks, tasks, installing a server, a
formatter and a linter). So do stand-ins for crates that need C, which
an extension's build has no compiler for: `crates/zstd-shim` is the
`zstd` crate's decoder in pure Rust, patched in for it
(`[patch.crates-io]`, as `extensions/parquet` does).

```text
extensions/my-themes/
  extension.toml
  themes/dusk.toml
  README.md
  CHANGELOG.md
```

```toml
[extension]
id = "fenix.my-themes"          # publisher.name
name = "My themes"
version = "1.0.0"
description = "What it adds, in a line"
runtime = "none"                # data only

[engine]
fenix = ">=1.0, <2"
```

`categories` are from a fixed list, the Extensions page's: `languages`,
`themes`, `snippets`, `keymaps`, `templates`, `tools`, `data`, `web`,
`mission`, `fun`. A `screenshots/` folder (PNG, JPEG, GIF or WebP, 2 MB
at most each) is packed with the rest and shown on the extension's
details.

```text
cargo xtask validate                  # or: cargo xtask validate index.json
cargo xtask pack extensions/my-themes dist
fenix --install-extension dist/fenix.my-themes-1.0.0.fenixext   # try it
```

## Bundles

A bundle is extensions that go together, installed as one -- the
welcome's Personal, Mission, Review, Jira, Lab and Reading
(`extensions/*-bundle`). Its manifest is its members, as dependencies,
and a description; its folder holds only that, README.md, CHANGELOG.md
and screenshots.

```toml
[extension]
id = "fenix.lab-bundle"
name = "Lab"
version = "1.0.0"
description = "Arduino, containers, VNC"
runtime = "bundle"

[engine]
fenix = ">=1.0, <2"

[dependencies]
"fenix.arduino" = "^2"
"fenix.containers" = "^2"
"fenix.vnc" = "^2"
```

Installing it installs each member (and what they need); removing it
removes the members nothing else needs and you didn't install on their
own. `cargo xtask validate` checks every dependency, a bundle's members
with them, is in `extensions/` (or, given the registry's `index.json`,
there) in a version its range takes, and that none goes round in a
circle.

## Releasing

Bump `version` in the manifest, add the CHANGELOG entry, and push a tag
`<folder>-v<version>`, e.g. `my-themes-v1.0.0`. `release.yml` packs it,
attaches it to a GitHub release, adds it to `index.json` on `registry`, and
signs the index. Or run Release by hand (Actions, Release, Run workflow, on
`main`) with the tag: it releases main's head and the release makes the tag.
Releases run one at a time, as each pushes `registry`.

The signature needs the registry key: `MINISIGN_SECRET_KEY` (the secret
key file's contents) and, if the key has one, `MINISIGN_PASSWORD`, as
Actions secrets. Its public half is `REGISTRY_KEY` in Fenix's
`crates/fenix-gui/src/installed.rs`.

## Licence

MIT OR Apache-2.0, as Fenix.
