# Contributing an extension

This is for anyone adding an extension to the store, or improving one
that's here. You don't need to be the maintainer: open a pull request.

How to write an extension -- the API, the manifest, what each call does
-- is Fenix's
[Writing an extension](https://github.com/tpedneault/fenix/blob/master/docs/WRITING_EXTENSIONS.md)
(code) and
[Extensions you install](https://github.com/tpedneault/fenix/blob/master/docs/CONFIGURATION.md#extensions-you-install)
(data). This guide is what this repository asks of one, and how it gets
to the store.

- [Two ways in](#two-ways-in)
- [Publisher ids](#publisher-ids)
- [What every extension carries](#what-every-extension-carries)
- [Versions](#versions)
- [Permissions](#permissions)
- [Building and checking](#building-and-checking)
- [Screenshots](#screenshots)
- [Tutorial lessons](#tutorial-lessons)
- [Shared crates](#shared-crates)
- [Review and release](#review-and-release)
- [Reporting a security problem](#reporting-a-security-problem)
- [Licence](#licence)

Coming, and not here yet: `cargo xtask new` to start an extension from
a template, `cargo xtask install` to pack and install in one step, and a
widget gallery
([milestone 2](plans/roadmap.md#milestone-2-author-tooling)). Until
then, copy the nearest extension here and change it.

## Two ways in

**A folder under `extensions/`.** The extension lives here, is built by
this repository's CI and released by its workflow. Its id starts with
`fenix.`. Choose this when you'd like it looked after with the rest:
the maintainer can fix it when Fenix's API moves, and it can use the
shared crates by path.

**A package you build and host.** The extension stays in your own
repository; you build the `.fenixext` and attach it to a GitHub
release there. This repository lists it: your pull request adds an
entry to `external.toml` with its `id`, `version`, `url`, `sha256`
(what `cargo xtask pack` printed) and `repository`.

- One entry per release. A new version is a new entry.
- The URL is a release asset of a repository your publisher's owner
  owns (below), over https -- not a branch, nothing that changes.
- What the store shows -- name, description, permissions, `fenix`,
  `api`, dependencies -- is read from the manifest inside the package,
  never from the entry.
- `cargo xtask external-check` downloads it, checks the hash and runs
  the same checks `validate` runs on a folder here. CI runs it on your
  pull request and prints the permissions for the reviewer.

This way in is being built now
([roadmap 1.4](plans/roadmap.md#14-external-packages-in-the-index-m));
[Extensions built elsewhere](README.md#extensions-built-elsewhere) in
the README has the details as they land.

Either way, the package is what Fenix installs, the index that lists it
is signed here, and Fenix checks the package's SHA-256 against it.

## Publisher ids

An id is `publisher.name`: lower-case letters, digits, `-` and `_`,
two parts or more (`acme.dusk`).

- **`fenix.`** is this repository's. Every extension under
  `extensions/` is `fenix.<folder>`, whoever wrote it; you're in its
  `authors`. A package built elsewhere can't use it.
- **Yours**, for a package built elsewhere, is claimed by your first
  accepted submission: `publishers.toml` then ties it to your GitHub
  account or organisation, and only releases from a repository it owns
  are accepted under it. First come. Moving it to other owners is a pull
  request the current owner approves.
- **Choosing one**: your GitHub name, lower-cased (`Acme-Labs` is
  `acme-labs`), unless it's taken. It's in every id you publish, and
  ids don't change once people have installed them, so choose one you
  will keep.

## What every extension carries

```text
extensions/<name>/
  extension.toml     the manifest
  README.md          what it does, its keys, settings, and what it may do
  CHANGELOG.md       a section per version, newest first
  screenshots/       a picture or up to three; the first is the hero
  Cargo.toml         code only: the crate, with Cargo.lock and .gitignore
  src/               code only
  themes/ snippets/ keymaps/ templates/ languages/ launch/ tutorial/ ...
```

A bundle carries only its manifest, README, CHANGELOG and screenshots
(see [Bundles](README.md#bundles)).

**The manifest.** As the README's example and Fenix's
[Its manifest](https://github.com/tpedneault/fenix/blob/master/docs/WRITING_EXTENSIONS.md#its-manifest),
plus what this repository fills in on each:

```toml
[extension]
id = "fenix.dusk"
name = "Dusk"                       # as the store lists it
version = "1.0.0"
description = "A theme for late evenings"   # a line: what it does, not how good it is
authors = ["Your Name"]
license = "MIT OR Apache-2.0"
repository = "https://github.com/tpedneault/fenix-extensions"
categories = ["themes"]
runtime = "none"                    # none (data), wasm (code) or bundle

[engine]
fenix = ">=1.0, <2"
```

- **Names**: the folder is the id's name part (`extensions/dusk`,
  `fenix.dusk`), lower-case letters, digits and `-`: the release tag is
  made from it. The `name` is what it is, in a few words ("CSV table",
  "Log viewer"). A command, setting or service starts with the name
  part (`dusk.open`, `dusk.contrast`).
- **Categories**, one or more, from the Extensions page's fixed list:
  `languages`, `themes`, `snippets`, `keymaps`, `templates`, `tools`,
  `data`, `web`, `mission`, `fun`. Anything else fails `validate`.
- **`runtime`**: `validate` also takes `lua`, but no Fenix runs Lua yet.
- **A setting's `default`** is written as text (`"true"`, `"30"`):
  `validate` refuses a bare `true`, which older Fenix releases can't
  read.
- **Comments in `[permissions]`**: one above each, saying what it's for,
  as the extensions here do. Reviewers read them.

**The README** says what it does and how to use it, in the second
person, then **Keys** (a table of key, command, what it does; standard
keys beside Vim's where it has chords), **Settings**, and **What it may
do**: each permission in a line of plain words, as the
[HTTP client's](extensions/http/README.md#what-it-may-do) does. Code
extensions add **Building**: how to pack it and what `cargo test`
covers, as [CSV](extensions/csv/README.md#building) does. Anything
borrowed (a grammar, a theme's colours) says where from, at which
commit, under which licence, with the licence file beside it
(`extensions/ruby/grammars/`).

**The CHANGELOG** has a section per version, `## 1.2.0`, newest first:
what changed for the person using it, in sentences. The first release
says so and the Fenix it's for: "The first release, for Fenix 1.0:
...". The whole file is the GitHub release's notes, and Fenix shows it
on the extension's details.

**Tests**, where it has code: unit tests in the crate, run on your
machine by `cargo test`. Natively the host's calls panic, so keep the
logic -- parsing, layout, what a key does -- in functions that don't
call the host, and test the rest on the
[test kit](crates/test-kit/README.md): `fenix_test_kit::api` in place of
the API under `#[cfg(test)]`, a fake host that writes down what it was
asked and answers as Fenix would, a harness for commands, keys and
events, and pages as text to snapshot. A language pack gives the same
fake to its `lang-kit` kit. [Regex](extensions/regex/src/driven.rs) and
[Zig](extensions/zig/src/lib.rs) are the examples. Fixtures and
snapshots go in `tests/`.

**`validate` also checks** a theme has a `name`; a snippet has its
`# key:`, its `# --` line and a body; screenshots are PNG, JPEG, GIF or
WebP, 2 MB at most; ids are unique; every dependency is here or in the
index, in a version its range takes, with no circle.

**`[[keys]]` in its keymaps** (chords, API 1.5): one key with its
modifiers; never `C-k`, `C-S-p`, `C-\` or `M-x`, which are Fenix's;
`scheme` is `vim`, `standard` or `any`; `when` is `anywhere`, `text`,
`typing`, `not_typing` or `page`, never `terminal`; a `context` is one
of its `[[contributes.contexts]]`; a `command`, or `unbind = true`. A
chord standard keys already use (`C-s`, `F12`, ...) with no `context`
is a warning: give it one. See Fenix's
[Keymap flavours](https://github.com/tpedneault/fenix/blob/master/docs/WRITING_EXTENSIONS.md#keymap-flavours).

**Agent tools** (`[[contributes.agent_tools]]`): a name of lower-case
letters, digits and `_`; one of its own `services`; a description of at
most 300 characters, which is how an agent decides to call it; an
`input` schema that's a JSON object of at most 2 KiB; `timeout_ms` from
1 to 600000; and a `class` -- `read`, `show`, `edit`, `run` or
`external`. An `external` tool needs `network` or `sockets`; an `edit`
one `editor = "write"` or `filesystem = "project-write"`; a `read` or
`show` one can't say `writes = true`. Pick the class honestly: it's what
users' rules allow or refuse.

## Versions

Versions are [semantic](https://semver.org), seen from the person using
it:

- **Patch** (1.0.x): fixes. The same permissions, the same `fenix` and
  `api`.
- **Minor** (1.x.0): something new -- a command, a setting, a page, a
  key, an agent tool -- or a new permission, or a higher floor. What
  they set up and the keys they know keep working.
- **Major** (x.0.0): something taken away or changed under them: a
  setting renamed or meaning something else, a command or a service
  gone, its saved data no longer read. Bundles and other extensions name
  their members by range (`"fenix.arduino" = "^2"`), so they stay on
  the old major until their range changes.

Every pull request that changes what's packed bumps the version and adds
the CHANGELOG entry. A README fix alone needn't.

**`fenix` and `api`.** `engine.fenix` is the Fenix releases it runs in,
`">=1.0, <2"`. A code extension also says the extension API it was built
against, `engine.api`: the API of the Fenix tag its
`fenix-extension-api` dependency names
(`fenix-extension-api = { git = "https://github.com/tpedneault/fenix", tag = "v1.5.0" }`
is `api = "1.5"`). The floor is the first Fenix offering that API:

| `api` | 1.0 | 1.1 | 1.2 | 1.3 | 1.4 | 1.5 |
|---|---|---|---|---|---|---|
| First Fenix | 1.0.0 | 1.0.1 | 1.1.0 | 1.2.0 | 1.3.0 | 1.5.0 |

Build against the oldest API that has what you use. Most extensions here
are on 1.0.

**Raising a floor.** An older Fenix isn't told about a release it can't
run: it keeps the last one that fits. Nothing breaks, but those users
get no more fixes. So
([roadmap 3.2](plans/roadmap.md#32-the-api-sweep-m-spread-over-releases)):

1. raise `fenix` or `api` only in a minor release that uses what the
   newer one adds;
2. say so at the top of that CHANGELOG entry ("For Fenix 1.5 (extension
   API 1.5): ...");
3. ship any fixes waiting first, in a patch release on the old floor.

## Permissions

An extension can do nothing it doesn't ask for, and Fenix shows what it
asks before anyone installs it. Ask for the narrowest that works: a
host, not `*`; one program, not a pattern; `read`, not `write`.

What each lets it do (Fenix's
[Its manifest](https://github.com/tpedneault/fenix/blob/master/docs/WRITING_EXTENSIONS.md#its-manifest)
has the syntax):

| Permission | Lets it |
|---|---|
| `process` | Run the programs it names: by name (`git`), the path a setting holds (`${setting:x.git}`), or under its own folders (`/tools/*`, `/data/tools/*`). |
| `network` | Reach these hosts over HTTP: `api.example.com`, `*.example.com`, a setting's (`${setting:x.url}`), or `*`, any, which the store shows as "Reach any host on the internet". |
| `sockets` | Open TCP and UDP sockets to these hosts and ports (`*:5900-5999`, `tcp://*.example.com:443`, a setting's addresses). |
| `ipc` | Connect to local programs' named pipes or Unix sockets, by name. |
| `listen` | Listen on these ports of 127.0.0.1 (an OAuth redirect). |
| `secrets` | Read and keep these secrets, by key (`x.token`) or pattern (`x.secret.*`), in the system's keychain. |
| `filesystem` | The open project: `project-read` or `project-write`. Without it, only its own folders. |
| `editor` | `read`: where you are, the file and selection, buffers' text. `write`: change what you edit too. |
| `folders` | A folder of yours, from a setting, at `/folders/<name>`. |
| `settings` | Read another extension's settings, by its prefix. |
| `env` | Read these of Fenix's environment variables. |
| `clipboard` | Read the clipboard's text (writing it needs nothing). |
| `agents` | Ask your coding agents, and hear what they said in their turns. |
| `draw` | Draw over the editor, in layers. |
| `sound`, `fonts` | Play its own WAVs; read the system's fonts. |

### Reviewer's checklist

For a new extension, and for any pull request that changes
`[permissions]`. The author answers it in the pull request; the reviewer
checks it against the code.

- [ ] Each permission has a comment in the manifest and a line in the
      README's **What it may do**, and the code uses it.
- [ ] `process` names programs, or the path a setting holds; a
      pattern only under its own `/tools` or `/data`.
- [ ] `network` names hosts, or a setting's host. `network = ["*"]` only
      when the hosts are the user's to choose (an HTTP client), with
      that reason in the README.
- [ ] `sockets` is narrowed by host, port or a setting as far as the
      protocol allows. `tcp://*:*` needs the same reason as
      `network = ["*"]`.
- [ ] Secrets are asked for by key or a pattern under its own name
      (`x.secret.*`), never another extension's; a token is a `secret`
      setting or kept with `set_secret`, never a plain setting or a
      file of its own. Fenix's own forge tokens (`github.token`,
      `gitlab.token`) only when it talks to that forge.
- [ ] `filesystem`: `project-read` unless it writes files in the
      project; `project-write` says what it writes.
- [ ] `editor`: `read` unless it changes your buffers; `write` says
      what it changes.
- [ ] `agents`, `clipboard`, `listen`, `env`, `ipc`, `draw`, `sound`
      and `fonts` only when a feature needs them; `env` and `listen` as
      narrow as the rest.
- [ ] An agent tool's `class` matches what it does.

## Building and checking

You need Rust stable with Clippy, and the WebAssembly target, once:

```text
rustup target add wasm32-wasip2
```

From the repository's root, the same steps CI runs:

```text
cargo clippy --locked -- -D warnings          # xtask
cargo test --locked -p xtask                  # xtask's tests
cargo xtask validate                          # every manifest, keymap, snippet, theme, screenshot
cargo test --locked --manifest-path extensions/<name>/Cargo.toml   # your extension's tests
cargo xtask pack extensions/<name> dist       # builds, then packs dist/<id>-<version>.fenixext
```

- `validate` prints `ok` with each extension and stops at the first
  problem, saying where. `cargo xtask validate index.json`, given the
  registry's index, also finds dependencies released but no longer here.
- `pack` builds a code extension first (`cargo build --release --locked
  --target wasm32-wasip2` in its folder), leaves `src/`, `tests/`,
  `target/` and `Cargo.*` out, and prints the package's id, version,
  file, sha256 and size. It leaves `extension.wasm` and `target/` in the
  folder: your `.gitignore` keeps them out of Git.
- CI runs `cargo clippy` on `xtask` only. Run it on your crate too, and
  fix what it says:
  `cargo clippy --locked --manifest-path extensions/<name>/Cargo.toml -- -D warnings`.
- Changed a crate under `crates/`? Run its tests, and those of every
  extension using it: CI runs them all.

Then try it in Fenix (Fenix's
[Starting one](https://github.com/tpedneault/fenix/blob/master/docs/WRITING_EXTENSIONS.md#starting-one)):

```text
fenix --install-extension dist/fenix.<name>-<version>.fenixext
```

A running Fenix notices within a few seconds (themes and languages wait
for `SPC q r`); `SPC h l` shows what it logs, and why it stopped if it
did; `SPC h x` shows its details as the store will, permissions and
screenshots included. Say in your pull request which Fenix you tried it
in.

**CI** runs those steps on every push and pull request, for every
extension at once: it tests every crate and packs every extension, so a
run takes 40 minutes to an hour. A first-time contributor's run may wait
for the maintainer to approve it. Run the steps for your extension
locally first.

## Screenshots

The first thing someone sees in the store. The house style
([roadmap 1.2](plans/roadmap.md#12-scripted-screenshots-m)):

- One theme and one window size for the whole store, as `screenshots.toml`
  sets them.
- PNG, under 2 MB each (`validate` refuses more).
- Up to three, in `screenshots/`. Fenix orders them by file name, and
  the first is the hero, shown on the list: name them `1-<what>.png`,
  `2-<what>.png`.
- The extension doing its main thing, on a real-looking project. No
  personal data, tokens, or other people's code.

Screenshots are being scripted: each extension gets a `screenshots.toml`
-- the sample project to open, the commands to run, what Fenix's state
should be before the shot, the file name -- and
`cargo xtask screenshots [extension]` drives a running Fenix over its
control channel to take them, so they can all be taken again after a
theme changes. Sample projects go in `screenshots/samples/`, which isn't
packed. [screenshots/README.md](screenshots/README.md) has the format
and the steps as they land. Until your extension has a script, a
screenshot taken by hand in the house style is fine.

## Tutorial lessons

A lesson is a few steps in Fenix's tutorial (`SPC h T`), checked as you
go: each says what to do, and passes when Fenix sees it done -- a page
open, a page closed, the practice text changed, the cursor somewhere.
An extension with pages or a workflow should have one. Lessons are data:
`tutorial/<topic>.toml` in the extension's folder, packed with the rest,
listed after Fenix's own under their `track`. The format is Fenix's
(`crates/fenix-gui/src/tutorial.rs`); the one in the store is
[Review's](extensions/review/tutorial/pull-requests.toml).

```toml
id = "dusk.contrast"        # its own name first, unique
track = "Dusk"              # its section in the tutorial
order = 1                   # its place in the track
title = "Checking contrast"
about = "One line under the title: what it teaches, and what it needs (a project on GitHub, a server)."
text = """
The practice buffer: what the lesson is about and its keys, to read
while you do it.

SPC d c   the contrast page
"""

[[step]]
say = "SPC d c: the contrast page."           # what to do
hint = "Space, d, c."                        # shown after a try that didn't pass, or a pause
page = "fenix.dusk/contrast"                 # passes with this page in front: <id>/<kind>

[[step]]
say = "q closes it."
hint = "q, on the page."
closed = "fenix.dusk/contrast"               # passes once no page of this kind is open
```

- A step's checks all have to hold: `page`, `closed`, `text` (the
  practice buffer's whole text), `cursor` (`[line, column]` from 1),
  `line`, `mode`, `picker`, `windows`, `terminal` and the rest in
  `tutorial.rs`. Unknown fields are refused.
- `say` is one action. `hint` says the keys one at a time, and what to
  do when the step can't work here (Review's: "s on the tutorial page
  skips this").
- From Fenix 1.5, `scheme = "vim"` or `"standard"` keeps a lesson to
  one key scheme, and its text can write `{leader}` and
  `{key:command.id}` for keys as the scheme in use has them. An older
  Fenix can't read a lesson that does, so its extension needs
  `fenix = ">=1.5"`.
- Try it through to the end in a clean profile.

## Shared crates

Code more than one extension uses goes under `crates/`, as a crate the
extensions depend on by path
(`fenix-ui-kit = { path = "../../crates/ui-kit" }`):

- [`ui-kit`](crates/ui-kit/README.md): widgets for canvas pages --
  tables, trees, tabs, text fields, splits, scrolled regions -- and
  their keys and mouse. Use it rather than laying out your own.
- [`lang-kit`](crates/lang-kit/README.md): what language packs share --
  the packages page, doctor checks, tasks, installing a server, a
  formatter and a linter -- and a fake host to test them with.
- [`test-kit`](crates/test-kit/README.md): testing an extension without
  Fenix -- a fake host, a harness, pages as text and snapshots. A
  dev-dependency: nothing of it is packed.
- `forge` (GitHub and GitLab), `jira`, `mib`, `ccsds`, `nats`,
  `diagram`: the code two extensions share, or an extension and its
  agent tools.

A new crate there is worth it when a second extension needs the code,
not before. Changing one is changing every extension that uses it: run
their tests, and bump and release those whose behaviour changes.

**Nothing that needs C.** An extension is built for `wasm32-wasip2`,
and that build has no C compiler. A crate that needs C gets a stand-in
in pure Rust under `crates/`, patched in for it: `crates/zstd-shim` is
`zstd`'s decoder, and `extensions/parquet` uses it with
`[patch.crates-io] zstd = { path = "../../crates/zstd-shim" }`. A
tree-sitter grammar is C too: it's compiled to WebAssembly once and
committed, with its source commit and licence beside it.

A package built elsewhere can use these crates as git dependencies of
this repository, pinned to a commit:
`fenix-ui-kit = { git = "https://github.com/tpedneault/fenix-extensions", rev = "..." }`.

## Review and release

**Review** reads the pull request as a user would meet it: the README
and CHANGELOG, the permissions against the
[checklist](#reviewers-checklist) and the code that uses them, the
version bump, the screenshot, the tests, and that it does one thing.
Expect questions on permissions more than style. Keep a pull request to
one extension, or one change across several.

**Release** is the maintainer's: once it's merged, they push the tag
`<folder>-v<version>` (`dusk-v1.0.0`), or run the Release workflow on
`main` with it. `release.yml` packs it, attaches it to a GitHub release
with its CHANGELOG as the notes, adds it to `index.json` on the
`registry` branch and signs the index. A package built elsewhere is
added the same way once its `external.toml` entry is merged.

**Reaching people**: Fenix reads the signed index. A new extension is on
the Extensions page's Browse tab (`SPC h x`, `r` reads it again); a new
version is on its Updates tab, for those who have it. Someone on a
Fenix older than the new release's `fenix` or `api` isn't offered it:
they keep the version they have, and the store goes on listing the
newest one that fits their Fenix.

## Reporting a security problem

Don't open a public issue or pull request for something that could put
people at risk: an extension doing more than its permissions say, a
package whose hash or signature doesn't check out, a way to get into
the index or under someone else's publisher id.

Report it privately, as Fenix's
[SECURITY.md](https://github.com/tpedneault/fenix/blob/master/SECURITY.md)
says: on Fenix's repository, **Security**, **Report a vulnerability**.
Name the extension, its version, and whether it's in `extensions/` or
built elsewhere. A fix is prepared in private and released before the
details are published.

An extension built elsewhere is its author's: tell them too, privately.

## Licence

Everything here is MIT OR Apache-2.0, as Fenix is. Unless you say
otherwise, a contribution you submit for inclusion in this repository,
as defined in the Apache-2.0 licence, is dual-licensed under MIT OR
Apache-2.0, without any additional terms or conditions. Something you
bring that's under another licence (a grammar, a theme's colours) keeps
it, with its licence file beside it and a line in the README. A package
built elsewhere is under the licence its author gives it, in its
manifest's `license`.
