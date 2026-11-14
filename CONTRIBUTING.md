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
- [Starting one](#starting-one)
- [A theme from VS Code](#a-theme-from-vs-code)
- [Snippets from VS Code](#snippets-from-vs-code)
- [Building and checking](#building-and-checking)
- [Screenshots](#screenshots)
- [Tutorial lessons](#tutorial-lessons)
- [Shared crates](#shared-crates)
- [Review and release](#review-and-release)
- [Reporting a security problem](#reporting-a-security-problem)
- [Licence](#licence)

Start from a template: `cargo xtask new <kind> <name>`
([Starting one](#starting-one)). For a page, the
[widget gallery](extensions/gallery) (`gallery.open`, `SPC h g`) has
every `ui-kit` widget working, each with the code that draws it a key
away: copy the section nearest to what you want. To test one without
Fenix, the [test kit](crates/test-kit/README.md).

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

**`validate` also checks** a theme reads as Fenix reads it -- every key
one it knows, each colour `#rrggbb`, `#rrggbbaa` or `{ color, alpha }`,
text never see-through -- in the oldest Fenix its `engine.fenix` takes
(a key newer than that makes that Fenix refuse the whole theme: the
terminal's colours are 1.4's, `[brackets]` 1.5's, `interface`,
`parameter` and `macro` 1.5.2's); a snippet has its
`# key:`, its `# --` line and a body; screenshots are PNG, JPEG, GIF or
WebP, 2 MB at most; ids are unique; every dependency is here or in the
index, in a version its range takes, with no circle; and the project kinds
and the templates' kinds above.

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

**Project kinds** (`[[contributes.project_kinds]]`, read by Fenix 1.6): a
language pack can say what kind of project it knows -- named, tagged and
coloured in the hub, on Home, in the modeline and in the doctor, found by
the files at its top, and what `kind = "java"` in a project's
`.fenix/settings.toml` means. It's data in the manifest, no code:

```toml
[[contributes.project_kinds]]
id = "ruby"                  # a-z first, then a-z 0-9 - _; 24 at most
name = "Ruby"                # 32 characters at most
tag = "RB"                   # 1 to 4 of A-Z 0-9 + # . with a letter
color = "constant"           # keyword, string, function, type, number, constant (the default), attribute, variable
markers = ["Gemfile", "gems.rb", ".ruby-version", "config.ru"]   # file names at the project's top
main = ["config.ru"]         # what it opens on when you've had none of its files open; the first that exists
```

- **Markers** are file names, not paths: at most 16, each at most 64
  bytes, no `/`, `\`, `..`, `*`, `?`, `[` or `]`, never `.fenix`,
  `{folder}` (the folder's own name) at most once and no other braces, no
  control characters, none given twice. A
  kind's markers say what a project **is**, and one match wins over every
  other kind, so they're narrower than the context's, which only say when
  the pack loads: leave `Rakefile`, `package.json` and the like in the
  context. A kind with no markers is only ever declared (Kotlin among
  Java's Gradle projects, Flutter beside Dart).
- **`main`**: at most 8 paths below the project, `/` between the parts,
  `{folder}` allowed, at most 128 bytes each, no empty part, no `..`, no
  `\`, not starting with `/`. A `name` needs a visible character.
- **At most 8 kinds** to a pack, each id once. Not `other`, `monorepo`
  or an alias Fenix reads (`py`, `uv`, `ino`, `sketch`, `rs`, `c++`, `c`,
  `cmake`, `js`, `ts`, `javascript`, `typescript`, `mib`).
- **Fenix's own ids** (`python`, `arduino`, `rust`, `tcl`, `cpp`, `node`,
  `go`) are allowed with `markers` and nothing else: the pack adds files
  that say a project is one (`deno.json` for `node`), and the kind's
  name, tag, colour and main files stay Fenix's.
- **Across the store**: one pack to an id, no tag on two kinds and none
  of Fenix's (`PY`, `INO`, `RS`, `TCL`, `C++`, `JS`, `GO`, `MONO`,
  `DIR`); two packs' kinds sharing a marker is a warning; and the whole
  store stays under Fenix's 256 kinds and 256 distinct markers (`validate`
  prints where it stands).
- **Keep `api` and `fenix` as they are.** Fenix 1.0 to 1.5.2 ignore the
  section, so the pack installs and runs as before without its kind; a
  kind isn't an API, and `validate` doesn't tie it to `engine.api`.
- **Templates**: a template's `kind` is one of Fenix's ids, or one a pack
  in the store declares (`validate` refuses a name no pack does, and warns
  when it's another pack's you don't depend on). Fenix 1.5 refuses a
  template naming a kind it doesn't know, so one naming the pack's own
  kind needs `fenix = ">=1.6, <2"`; `validate` refuses it below that.
  Until the pack's floor is 1.6, templates stay `kind = "other"`.
- **Version**: adding the section is a **minor** release (something new
  for the person using it, nothing moved). It's only worth a release
  with the first Fenix that reads it: an update prompt for a section
  Fenix 1.5 ignores has nothing in it. Say so in the CHANGELOG ("Fenix
  1.6 and later show this as a Java project...; earlier Fenix ignores
  it").

`cargo xtask new language-pack` writes the block commented out beside the
context, for you to fill in.

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

Build against the oldest API that has what you use -- unless you use a
shared kit (`ui-kit`, `lang-kit`, `test-kit`): they build against 1.5,
and an extension on one builds against the same, so its floor is Fenix
1.5. Most extensions here are on 1.5.

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

## Starting one

```text
cargo xtask new <kind> <name | publisher.name> [--name "Display name"]
```

`<kind>` is `theme`, `keymap`, `snippets`, `language-pack` or `tool`,
and the name is the folder: `cargo xtask new tool word-count` writes
`extensions/word-count/`, as `fenix.word-count`. What it writes is the smallest working
extension of the kind -- the same files [What every extension
carries](#what-every-extension-carries) lists, a README with its **What it
may do**, a CHANGELOG for 1.0.0, and the narrowest permissions the kind
needs -- and it passes `cargo xtask validate`, packs, and, for the two
kinds with code, builds, tests and lints as it is:

| Kind | What it starts you with |
|---|---|
| `theme` | A light theme in Fenix's theme format, its colours kept at a readable contrast |
| `keymap` | A flavour over standard keys: three `[[keys]]`, as [JetBrains keys](extensions/jetbrains-keys) has them |
| `snippets` | Two snippets for every language, as [Rust snippets](extensions/rust-snippets) has its own |
| `language-pack` | A pack on lang-kit: a language file, a doctor check, tasks, a snippet, tests, and a placeholder tool, `example-tool`, that it installs nothing for |
| `tool` | A command, a canvas page built with `ui-kit`, a setting, a key under `SPC o`, tests |

Where something is yours to say -- the author, the description, the tool
a language pack is about -- the file has a `TODO(you)` note saying what
goes there; search the folder for them. A code kind's `Cargo.lock` is
committed with it, so `--locked` holds from the start.

- **The id.** A bare name is this repository's: `fenix.<name>`
  ([Publisher ids](#publisher-ids)), as everything under `extensions/`
  is. `publisher.name`, one dot, is one of your own, for a package you
  build and host: `new` says so when it finishes, and what to move.
- **Refused**, with nothing written: an id that isn't `publisher.name`
  (the name is lower-case letters, digits and `-`, starting with a
  letter), a kind it doesn't have, a folder or an id that's already taken.

To try it in Fenix:

```text
cargo xtask install extensions/<name>             # packs it into a temporary folder, then: fenix --install-extension <the .fenixext>
cargo xtask install extensions/<name> --watch     # and again each time a file under it changes; Ctrl+C stops
```

Fenix is `$FENIX` if it's set, else `fenix` on the `PATH`, else what
`--fenix <path>` says; it works the same on Windows and Linux. `--watch`
polls the folder's files twice a second, ignoring `target/`, `dist/`
and the `extension.wasm` a build makes, and installs once things have been
still for a moment. It's the old `package.ps1 -Install`, in one command.

## A theme from VS Code

```text
cargo xtask theme-import <theme.json> [--out <dir>] [--name "Its name"] [--fenix <version>] [--source "<URL at a commit>"]
```

converts a VS Code colour theme -- its JSON, comments and trailing
commas and all, with what it `include`s -- into a Fenix theme file in
`<dir>` (`.` unless said), and prints a report of what it did. Start
with `cargo xtask new theme <name>`, delete its sample theme, and import
into its `themes/`.

- **Only a theme whose licence lets you.** MIT, Apache-2.0, BSD or ISC,
  checked in its own repository, not a marketplace page: its licence
  file goes beside the manifest as `LICENSE-<theme>`, the manifest's
  `license` is that licence, and the README says where the theme is
  from, at which commit, under which licence. Import from the file at
  that commit, and pass its URL as `--source`: the theme file's header
  keeps it.
- **`--name` is the name VS Code gives the theme**: the `label` (or
  `id`) in its `package.json`'s `contributes.themes`, which is what
  `workbench.colorTheme` holds. Fenix's settings import (`SPC h i`)
  then brings someone's VS Code theme over as this one, once it's
  installed. Never one of Fenix's own names: that would take its place.
- **`--fenix`** is the oldest Fenix the extension is for, as its
  `engine.fenix` says (1.0 unless said). Fenix refuses a whole theme with
  a key it doesn't know, so the theme's own terminal colours are only
  written for 1.4 and later, its bracket pair colours for 1.5, and
  interface, parameter and macro colours for 1.5.2; the report says what
  was left out and which `--fenix` keeps it.

How it maps, by what each colour is for: the editor's page and text are
VS Code's; Fenix's popups and status line are drawn on the first of VS
Code's popup, tab strip, title bar, status bar and side bar colours that
keeps the text readable; syntax colours are found the way VS Code
colours a token -- the TextMate scopes a grammar gives each kind of thing
(`keyword.control`, `entity.name.function`, `constant.numeric`, ...),
matched against the theme's rules, the most specific winning -- and a
kind no rule colours is body text, as VS Code would draw it; Git's
colours are `gitDecoration.*`, "added" being Fenix's staged. What VS
Code has no colour for is worked out from the theme's own and said: the
mode badges by hue from its terminal, Git and syntax colours (Normal
blue, Insert green, Visual purple, Replace amber, Command cyan, a hue
turned where it has none), their lettering whichever of its page and
text reads better, folder icons Normal's blue. Text with alpha is laid
over its page.

**The report** lists what each Fenix key was given and from where, what
was worked out and how, what was left for Fenix to work out, what the
Fenix it's for can't read, every workbench colour with no place in a
Fenix theme, the token rules no Fenix colour came from, font styles and
token backgrounds (Fenix's themes have neither), semantic colours it
didn't use, and body text, comments or line numbers under 4.5:1 as
published. Say what it couldn't map in the README, as
[Vitesse themes](extensions/vitesse-themes/README.md) does.

Every theme it writes is checked against Fenix's own rules before it's
written, as `validate` checks them, so it loads. Nothing here opens it
in Fenix: install it (`cargo xtask install`) and look before you send
it.

## Snippets from VS Code

```text
cargo xtask snippets-import <snippets.json> --language <id> [--into <extension dir>] [--approximate] [--indent <n>]
```

converts a VS Code snippets file (`rust.json`, `x.code-snippets`) into
Fenix's `.snippet` files: one per prefix, in
`snippets/<language>/<prefix>.snippet` under `--into` (an extension's
folder; nothing there is ever overwritten -- if a file is there already,
nothing is written). Without `--into` it prints them. `<language>` is
the language as Fenix names it (`rust`, `cpp`, `bash`, `tsx`); a
snippet whose `scope` names other languages is left out.

**Exactly**: text, `$1` stops, `${1:text}` placeholders, `$0`, a stop
again (a mirror), `TM_FILENAME`, `TM_FILENAME_BASE`, `TM_FILEPATH`,
`TM_DIRECTORY`, `CURRENT_YEAR`, `CURRENT_YEAR-CURRENT_MONTH-CURRENT_DATE`
(Fenix's `DATE`), `CURRENT_HOUR:CURRENT_MINUTE:CURRENT_SECOND` (`TIME`),
an upper- or lower-casing transform of a whole stop or variable
(`${1|upper}`), and `TM_SELECTED_TEXT`, which is empty -- or its default
-- whenever a snippet is expanded from its prefix, the only way Fenix
expands one.

**What Fenix's format can't say is left out**, with the reason in the
report, rather than written to behave differently: a choice
(`${1|a,b|}`), a placeholder inside another, `${0:text}`, two defaults
for one stop, a variable Fenix doesn't have (`CLIPBOARD`, `UUID`, ...)
or a variable's default, any other regular expression transform, a
transform of a stop you never type into, an unknown `$name`, a prefix
with a space, a snippet with no prefix. **`--approximate`** writes the
nearest instead -- a choice's first option, the first default, the
value untransformed -- and lists each one; nested placeholders are
refused either way.

Bodies that indent with tabs: VS Code indents them as you do, Fenix
inserts the tab. `--indent 4` writes each as four spaces. Descriptions
aren't kept (a Fenix snippet has a name, and the name is the snippet's
own). Every body written is read by a copy of Fenix's snippet parser
first, so Fenix loads what's written.

Snippets someone else wrote come in only under a licence that lets them
(MIT, Apache-2.0, BSD, ISC), checked in their own repository, with the
licence file and a line in the README saying where from and at which
commit -- as a theme's colours do.

## Building and checking

You need Rust stable with Clippy, and the WebAssembly target, once:

```text
rustup target add wasm32-wasip2
```

From the repository's root, the same steps CI runs:

```text
cargo clippy --locked -- -D warnings          # xtask
cargo test --locked -p xtask                  # xtask's tests
cargo xtask validate                          # every manifest, keymap, snippet, theme (as Fenix reads it), screenshot
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
cargo xtask install extensions/<name>
```

which is `pack` and then `fenix --install-extension` on the package
(`dist/fenix.<name>-<version>.fenixext` does the same by hand).

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
open, a page closed, a picker, the practice text changed, the cursor
somewhere. It's never the keys pressed that are checked, so any way
there passes. An extension with pages or a workflow should have one.
Lessons are data: `tutorial/<topic>.toml` in the extension's folder,
packed with the rest, listed after Fenix's own under their `track`. The
format is Fenix's (`crates/fenix-gui/src/tutorial.rs`); in the store,
[Review's](extensions/review/tutorial/pull-requests.toml),
[Notebook's](extensions/notebook/tutorial/notes.toml),
[Jupyter's](extensions/jupyter/tutorial/notebooks.toml) and the
language packs' (below).

```toml
id = "dusk.contrast"        # <name>.<topic>: its own name first; Fenix keeps what's done by it
track = "Dusk"              # its section in the tutorial, after Fenix's own (never Editing, Fenix or Git)
order = 1                   # its place in the track, from 1
title = "Checking contrast"
about = "One line under the title: what it teaches, and what it needs (a project on GitHub, a server)."
text = """
The practice buffer: what the lesson is about and its keys, to read
while you do it -- and what a text step changes.

SPC d c   the contrast page
"""

[[step]]
say = "SPC d c: the contrast page."              # one thing to do
hint = "SPC d, then c."                          # after a try that didn't pass, or 20 seconds
page = "fenix.dusk/contrast"                     # passes with this page in front: <id>/<kind>

[[step]]
say = "q closes it."
hint = "q, on the page."
closed = "fenix.dusk/contrast"                   # passes once no page of this kind is open
```

The fields, and the Fenix that reads each -- everything not listed is
Fenix 1.0's:

| Field | | Fenix |
|---|---|---|
| `id`, `track`, `title` | needed | 1.0 |
| `order`, `about`, `text` | | 1.0 |
| `[[step]]` | at least one: `say`, `hint`, and its checks | 1.0 |
| `[repo]` | `file` and `committed`: a Git repository made fresh for the lesson, its practice buffer that file (Fenix's Git lesson) | 1.0 |
| `scheme` | `vim`, `standard` or `any`: only listed for that key scheme | 1.5 |
| `{leader}`, `{key:command.id}` in its words | the leader, and the keys that run a command, as the scheme in use has them | 1.5 |

A step's checks, all of which have to hold:

| Check | Passes when |
|---|---|
| `page` | this page is in front: one of Fenix's (`doctor`, `settings`, `projects`, `git`, `extensions`, `tutorial`, ...; `notifications` and `agent-usage` from 1.2), a tab or fact of one (`extensions/browse`, `git/staged`, `git/menu/commit`), or an extension's, `<id>/<kind>` |
| `closed` | no page of this kind is open, after one was during the step |
| `picker` | `files`, `commands`, `bindings`, `lines`, `buffers`, `tasks`; `any` (an extension's picker, the launches, ...); `none` once one was put away |
| `text` | the practice buffer has the lines this step changes, back in Normal mode |
| `cursor`, `line` | the cursor in the practice buffer, `[line, column]` from 1 |
| `mode`, `leader`, `windows`, `moved`, `terminal` | as `tutorial.rs` says |

- **What a lesson can see** is only that: the page or picker in front,
  the pages open, the practice buffer, the windows, the terminal. Not
  the debugger, a task's output, a prompt, a fix the doctor ran, a file
  opened, a page an extension opens *beside* (focus stays where it was:
  check `windows = "+1"` instead).
- **Every step checks something**: Fenix refuses a lesson with a step
  that doesn't, so there's no step that only says. An action nothing
  shows goes in the `say` of the step whose check comes after it --
  "`f` on the row runs its fix. Then `q`", `closed = "doctor"` -- as
  Review's "Enter on one would review it ... `q` closes the list for
  now" does.
- **`s` on the tutorial page skips a step**, and skipping a step that
  opens a page or a picker skips the one that closes it. Say in its
  `hint` what it needs and that `s` skips it ("Without cargo, `s` on the
  tutorial page skips this"), so someone without the tool gets through.
- **Write keys as `SPC`**: Fenix 1.5 says `SPC` as standard keys' leader
  (`Ctrl+K`) by itself. `{leader}`, `{key:...}` and `scheme` need
  `fenix = ">=1.5"`; an older Fenix refuses `scheme` and shows the
  others as written. Don't raise an extension's floor for a lesson:
  write it without them.
- A new lesson is new packed content: a minor release, as anything new
  is ([Versions](#versions)).
- Try it through to the end in a clean profile, with and without the
  tools it needs.

**`cargo xtask validate` checks every lesson** as Fenix reads it --
every field known and of its type, what's needed there, a check on
every step and the values Fenix knows -- and what would load and not
work: an id that isn't `<name>.<topic>`, is taken (by another lesson
here or one of Fenix's), a track that's Fenix's, an `order` missing or
taken in its track, a `page` that isn't Fenix's or an extension's here
-- yours, or one you depend on, whose source opens that kind -- a
`closed` on a tab or a fact (never a kind, so never passes), a field or
placeholder the extension's `fenix` floor can't read (it says which
floor would), a `{key:...}` of an extension's that isn't its command, a
step whose `text` is what it starts from (it passes at once), a
`[repo]` file in a folder, a file in `tutorial/` Fenix won't read.

### A language pack's lesson

One shape for every language pack, so a learner who has done one knows
the next. Rust's, Python's, Web's and Go's are it, each
`tutorial/project.toml`:

- `id = "<name>.project"`, `track` the pack's `name` (`"Rust"`,
  `"Web: JavaScript and TypeScript"`), `order = 1`, `title = "A <language>
  project: doctor, <packages>, tasks, debugging"`.
- `about` says it's done in a project of the learner's (and how to make
  one: `cargo new`, `uv init`, `npm init -y`, `go mod init`), and that `s`
  skips a step whose tool isn't installed yet.
- `text` lists the keys, and a practice program of a few lines with a
  loop, for a project with none of its own, and the line to break on.
- Ten steps, five pairs:

| | Step | Check |
|---|---|---|
| 1 | `SPC p h`: the doctor, naming the pack's checks | `page = "doctor"` |
| 2 | `f` on the server's row runs its fix (say the command); then `q` | `closed = "doctor"` |
| 3 | `SPC m p`: the packages page | `page = "fenix.<name>/packages"` |
| 4 | `u` `a` `d`, as tasks; `q` | `closed = "fenix.<name>/packages"` |
| 5 | `SPC p t`: the tasks (say which) | `picker = "tasks"` |
| 6 | the test task, Enter (or Esc) | `picker = "none"` |
| 7 | in a file of the language, `SPC u b`; then `SPC u U`, the launches | `picker = "any"` |
| 8 | the launch for the practice program, Enter: it stops at the breakpoint; `n`, `c`, `SPC u q` | `picker = "none"` |
| 9 | `SPC ,`: `editor.format_on_save`, naming the formatter | `page = "settings"` |
| 10 | `q`; saving now formats and lints (say with what, and its setting) | `closed = "settings"` |

- The pack's own words in each: its doctor rows and their fix commands,
  what its packages page calls them (crates, modules), its task names,
  its launch's name, its formatter and linter and their settings -- from
  its manifest, README and source.
- Without the toolchain, steps 1-2 and 9-10 work anywhere, and the
  doctor's rows say what's missing; 3-8 are skipped, three `s`, each
  skipping its pair. `SPC u U` rather than `SPC u u`: it always offers
  the launches, where `SPC u u` starts the last one again, and with no
  adapter neither opens anything.
- The breakpoint and the stop aren't seen by the tutorial: step 8
  passes when the picker goes, and says what follows.

## Shared crates

Code more than one extension uses goes under `crates/`, as a crate the
extensions depend on by path
(`fenix-ui-kit = { path = "../../crates/ui-kit" }`):

- [`ui-kit`](crates/ui-kit/README.md): widgets for canvas pages --
  tables, trees, tabs, text fields, splits, scrolled regions -- and
  their keys and mouse. Use it rather than laying out your own; the
  [widget gallery](extensions/gallery) shows each one working.
- [`lang-kit`](crates/lang-kit/README.md): what language packs share --
  the packages page, doctor checks, tasks, installing a server, a
  formatter and a linter -- and a fake host to test them with.
- [`test-kit`](crates/test-kit/README.md): testing an extension without
  Fenix -- a fake host, a harness, pages as text and snapshots. A
  dev-dependency: nothing of it is packed.

  Test providers for Fenix 1.6 are being added to lang-kit and the test
  kit: lang-kit's `tests` (a pack's `TestRunner` on the five exports) and
  the test kit's driver (Fenix's side, on recorded output), against a copy
  of API 1.6's types until 1.6 is released; see
  [`plans/test-explorer.md`](plans/test-explorer.md).
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
