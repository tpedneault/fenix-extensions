# Protobuf & gRPC

Protocol Buffers in Fenix, and a gRPC client: the grammar (Fenix has none
of its own), buf's language server, `buf format`, `buf lint` on save,
`buf breaking` against the default branch, `buf generate` and `buf build`
as tasks, the project doctor's checks with fixes that install buf, protoc
and grpcurl, and snippets. Built on lang-kit (`crates/lang-kit`), as each
language pack is; the client's page on ui-kit (`crates/ui-kit`). It needs
Fenix 1.5 or later (extension API 1.5); on an older Fenix, Protobuf & gRPC
1.1.0 is the one you get.

It starts with Fenix. buf's server is set for `.proto` files whenever buf
is found; the doctor checks buf, protoc and grpcurl in every project.

## The grammar

`*.proto` files are read with tree-sitter-proto 0.6.0's grammar
(`grammars/`, MIT): highlighted -- keywords, types and the built-in
scalars, field and option names, enum values, rpc names, import paths,
numbers, strings and their escapes --, folded (messages, enums, services,
rpcs with options, oneofs, `extend`, an option's aggregate value), with
Enter indenting inside a body and the breadcrumbs and `SPC c s` listing
the declarations. `//` comments a line.

## The server

buf's language server, `buf lsp serve` (`buf beta lsp` on a buf from
1.43 to 1.58): completion, go to definition, hover, references, rename,
document and workspace symbols, and its problems as you type. With
`protobuf.server` = `protols`, protols (`cargo install protols`)
instead; either is used when the other isn't there. `none` sets
neither.

## Formatting and checks

- `SPC c F`, and saving with `editor.format_on_save`: `buf format
  --stdin-filepath <file>`, buf's own style. That's buf 1.74's; an older
  buf formats through its language server.
- On save: `buf lint` over the workspace (`protobuf.lint`), its JSON read
  -- a rule broken is a warning on its file, with the rule's name
  (`FIELD_LOWER_SNAKE_CASE`), a file that doesn't compile an error -- and
  a file it no longer speaks of made clean.
- On the project doctor's page: **Breaking changes**, what `buf breaking
  --against '.git#branch=<branch>'` finds when the project opens and a
  `.proto` file is saved (`protobuf.breaking_check`) -- none, or how many
  and which rules -- with a fix row that runs it as a task, its changes in
  the quickfix list. The branch is `protobuf.breaking_against`, else the
  repository's default: origin's `HEAD`, else `main` or `master`. A
  workspace a folder down compares that folder (`subdir=`).

## Tasks

`SPC p t`, each in the buf workspace's folder (`buf.yaml`, or v1's
`buf.work.yaml`, at the top or a folder down):

| | |
|---|---|
| `build` | `buf build` |
| `lint` | `buf lint` |
| `format` | `buf format -w` |
| `breaking against <branch>` | `buf breaking --against '.git#branch=<branch>'` |
| `generate` | `buf generate`, when there's a `buf.gen.yaml` |
| `generate <name>` | `buf generate --template <file>`, for each `buf.gen.<name>.yaml` and `buf.<name>.gen.yaml` beside it |
| `dep update` | `buf dep update`, when `buf.yaml` has deps |
| `protoc check` | Without buf: `protoc` on every `.proto` file, its output in the pack's own folder |

buf's output, `file:line:col:message`, goes to the quickfix list.
`buf.yaml`, `buf.work.yaml`, `buf.lock` and the templates saved read the
project again. The project's `.env` goes to each.

## The doctor

| Check | | Fix row |
|---|---|---|
| buf | `buf --version`, and what that buf does: a server from 1.43, `buf lsp serve` from 1.59, a formatter on stdin from 1.74; needed in a buf workspace | buf 1.75.0 (Linux, macOS, Windows; x86_64 and arm64) |
| protoc | `protoc --version` | protoc 36.2 (Linux, macOS, Windows on x86_64) |
| grpcurl (the gRPC client) | `grpcurl -version` | grpcurl 1.9.4 (Linux, macOS, Windows; x86_64 and arm64) |
| protols | when it's the server asked for | |
| Breaking changes | above | runs it as a task |

Each release comes from its project's GitHub releases, checked against
the SHA-256 the pack knows (buf's and grpcurl's from their releases'
checksum files), unpacked into the pack's tools folder, where only the
pack looks, and used at once. Starting one is said in the message line;
how it ended is a notification, since a download can take a while:
"Installed buf 1.75.0", or "Couldn't install buf 1.75.0" with which step
failed and why, and *Try again*. One you installed yourself is used
instead: the pack looks on `PATH`, and in Go's, Cargo's, Homebrew's,
Snap's, Scoop's and winget's folders.

## The gRPC client

`SPC m g` (`C-k m g` with standard keys; or Protobuf: the gRPC client)
opens it. It runs grpcurl, which
speaks HTTP/2; the extension reads what grpcurl prints.

- **The server**: `protobuf.targets`, a server a row -- `host:port`, TLS
  on or off. `T` picks one or adds one, `a` adds one when there are none,
  `t` turns TLS on or off for it. `protobuf.insecure` takes a server whose
  certificate doesn't check out (`-insecure`).
- **Its services**: by the server's reflection (`grpcurl list`, then
  `describe` each service), or, `s`, from the project's `.proto` files --
  `buf build` writes them with their imports into a descriptor set in the
  pack's folder (`-protoset`), or without buf they're given to grpcurl as
  they are (`-import-path`, `-proto`), which needs no server at all to
  list them. A workspace with no server set starts with its protos.
- **The sidebar**: each service and its methods, how each streams beside
  it (unary, server, client, bidi); `]` its history.
- **The body**: Enter on a method describes its input message (and the
  messages and enums in it, a `describe` each until it's all known) and
  makes a body from it -- each field by its JSON name with its zero
  value, an enum's first value, a list and a map of one, a oneof's first
  choice, the well-known types as protojson writes them (a `Timestamp` a
  string, a `Duration` `"0s"`), 64-bit integers as strings -- in a scratch
  buffer beside, highlighted as JSON. Edit it there; each method keeps
  its own. `B` makes it again. A client stream's body is its messages one
  after another.
- **Metadata**: `a` on the request adds a header (`authorization: Bearer
  ...`), Enter edits one, `d` deletes it; each method has its own.
- **A call**: `r` (or Protobuf: invoke the gRPC method chosen) runs
  `grpcurl -v -H ... -d <body> <server> <service/method>`; the body is
  checked as JSON first. Its answer shows as it comes: the status, how
  long it took and how many messages (`OK · 12 ms · 1 message`,
  `NotFound: no satellite · 8 ms`), then tabs for the messages -- a server
  stream's each as it arrives, with when --, the response's headers, its
  trailers, and the timing of each. `x` cancels it. `protobuf.timeout`
  is how long a call may take (`-max-time`).
- **The history**: each call kept for its method (`protobuf.history` of
  them, newest first) with its server, body, metadata, status, time and
  first message, in the extension's folder. Enter on one puts its body
  and metadata back, and shows how it went.
- `y` copies the answer's messages; `O` opens them in a buffer.

### Keys

| Key | |
|---|---|
| `Tab`, `Shift+Tab` | the sidebar, the request, the answer |
| `j` `k`, Enter | a row; choose a method, send a call from the history again |
| `r`, `x` | invoke; cancel |
| `e`, `B` | the body's buffer; the body made again |
| `a`, `d` | add, delete metadata (on the request) |
| `T`, `t`, `s`, `R` | the server; its TLS; reflection or the project's protos; list again |
| `1`-`4`, `[` `]` | the answer's tabs |
| `y`, `O` | copy the messages; open them |
| `I` | install grpcurl, when it isn't there |

## Snippets

| Key | |
|---|---|
| `syntax` | `syntax = "proto3";` and `package` |
| `message`, `field`, `repeated`, `map` | a message with its first field; a field; a repeated one; a map |
| `enum` | an enum, its zero value `_UNSPECIFIED` as buf's lint wants |
| `oneof` | a oneof |
| `service`, `rpc`, `rpcs` | a service with its first method; a method, its request and response named for it; one the server streams |
| `option`, `import`, `reserved` | a file option (`go_package`); an import; reserved numbers and names |

## Settings

| Setting | Default | |
|---|---|---|
| `protobuf.lint` | on | `buf lint` on a saved file |
| `protobuf.server` | `buf` | `buf`, `protols` or `none` |
| `protobuf.breaking_against` | | the branch to compare with; the default branch |
| `protobuf.breaking_check` | on | `buf breaking` for the doctor, on open and save |
| `protobuf.targets` | | the gRPC servers, and TLS for each |
| `protobuf.insecure` | off | TLS servers whose certificate doesn't check out |
| `protobuf.timeout` | 30 | seconds a call may take; 0 none |
| `protobuf.history` | 50 | calls kept per method |

## Keys

After `SPC m` in a buf project, a `.proto` file or the client (`C-k m`
with standard keys): `d` checks the project again, `b` runs `buf
breaking` as a task, `g` opens the client, `r` invokes its method, `t`
picks the server.

## What it may do

It runs buf, grpcurl, protoc and protols, and what it installs in its
tools folder; downloads their releases from GitHub; reads the project
(`buf.yaml`, the templates, the `.proto` files, `.git`'s branches,
`.env`) and the file you save, for `buf lint`. A call goes where its
server is: grpcurl makes it, not Fenix.

## Not yet

- **`.grpc` files**: requests kept in files and run from the buffer, as
  the HTTP client's `.http` files are.
- **Bidirectional streams typed as you go**: a client or bidi stream's
  messages are its body's, all sent at once.
- **Breaking changes in the files**: they're on the doctor's page and in
  the task's quickfix list, not marked in the buffer.
