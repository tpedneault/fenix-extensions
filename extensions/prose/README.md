# Prose

Spelling and grammar in the languages you name, offline: Harper added
beside their language servers, so what it finds shows in the problems
list and the margin like any other problem, with a word list for the
project. Built on lang-kit (`crates/lang-kit`), as each language pack is.

It starts with Fenix. It needs Fenix 1.5 or later (extension API 1.5).

## What it checks

[Harper](https://writewithharper.com) (`harper-ls`, Apache-2.0), a
grammar and spelling checker that runs on your machine: **it's offline**.
Nothing you write leaves it, and nothing is sent anywhere once Harper is
downloaded (the download is the one network use, below). It checks
spelling, repeated words, a/an, punctuation, capitalisation, common
confusions and some grammar, in English.

It reads the languages in `prose.languages`, by Fenix's names for them,
and nothing else; by default that's `markdown`.

| Language | What Harper reads |
|---|---|
| `markdown` | The text: not code blocks, URLs or inline code. |
| `html` | The text of the page. |
| `rust`, `python`, `go`, `java`, `javascript`, `typescript`, `c`, `cpp`, `csharp`, `lua`, `ruby`, `php`, `kotlin`, `swift`, `haskell`, `zig`, `toml` | Comments, and doc comments as Markdown. Not strings or identifiers. |

Not every language Fenix has: Harper calls `bash` and `tsx` by other
names than Fenix does, so it reads nothing in them. Name a code language
only if you want its comments checked: Harper is made for prose, and
comments full of code terms will show words it doesn't know (add them to
the project's list).

### Plain text and commit messages

Harper reads both, but the pack can't hand them to it: Fenix adds a
server by its **language**, and plain text and a commit message
(`COMMIT_EDITMSG`) are no language of Fenix's. Until they are, you can
read one as Markdown with a language file of your own, in your settings
folder (`languages/text.toml`):

```toml
language = "markdown"
extensions = ["txt"]
filenames = ["COMMIT_EDITMSG", "TAG_EDITMSG", "MERGE_MSG"]
```

That makes Fenix highlight those files as Markdown too, which is why the
pack doesn't ship it.

## Fixes

Harper's suggestions are code actions, and it offers to add a word to its
dictionaries. **Fenix's `SPC c a` applies only an action that is a plain
edit**, and Harper marks each of its suggestions with a command that
records a statistic, and its dictionary actions are commands: until Fenix
runs those, `SPC c a` lists no Harper fix. The finding and its message
show; fix the text by hand, and add a word with the pack's own commands (Words, below).

## Words

The project's word list is `.fenix/words.txt` (`prose.words`): one word a
line, sorted, as Harper writes it. Harper is told it's its workspace
dictionary, so a word in it isn't a finding in any file of the project;
words are case-sensitive, and the list is the project's to keep in Git.

- `SPC m w` (`prose.add-word`): the word under the cursor, or the single
  word you've selected, is added, and Harper checks the open files again
  at once.
- `SPC m W` (`prose.words`): the list, opened; made empty if the project
  has none.

Harper's own *user* dictionary (words for every project) is a file in its
own configuration folder, which an extension can't write: it isn't
offered here. Harper's other code action, ignoring one finding, saves
where Harper keeps those.

## Settings

`SPC ,`. Each can be set for one project (`.fenix/settings.toml`), which
is how it's switched off for a project.

| Setting | Default | |
|---|---|---|
| `prose.enabled` | `true` | Off: Harper isn't added for the project. |
| `prose.languages` | `markdown` | Fenix's names, separated by commas. Changed, Harper is started again for the new set. |
| `prose.severity` | `information` | `error`, `warning`, `information` or `hint`: how its findings show. |
| `prose.dialect` | `American` | `American`, `British`, `Canadian`, `Australian` or `Indian`: the spelling it holds you to. |
| `prose.words` | `.fenix/words.txt` | The project's word list, a path under the project. |

To turn a rule off, or set what the pack doesn't, use the project's own
`.fenix/lsp/harper.json` (read once the project is trusted), in Harper's
section: `{"harper-ls": {"linters": {"SpellCheck": false}}}`. The pack's
settings win over the file's where both say a thing.

## The doctor

`SPC p h`:

| Check | | Fix row |
|---|---|---|
| Harper (grammar and spelling) | `harper-ls --version`; a warning when it isn't found | Install Harper 3.2.0 |
| Prose checking | The languages and the word list in use, or why none is: off for the project, no language named, waiting for Harper | |

`SPC m d` checks again. Harper is looked for in the pack's tools folder
first, then `PATH` and Cargo's, Homebrew's and Scoop's folders: a Harper
you installed yourself (`cargo install harper-ls`, `brew install
harper`) is used as it is found.

## What it downloads

Harper 3.2.0 from its GitHub release, once, into the pack's tools folder
(`/tools/harper`), when you run the fix row or `prose.install.harper`.
The release is pinned, and Fenix refuses a download that isn't the file
whose SHA-256 is in the pack:

| Machine | File | Download | Installed |
|---|---|---|---|
| Linux x86-64 | `harper-ls-x86_64-unknown-linux-musl.tar.gz` (static) | 14.8 MB | 65 MB |
| Linux ARM64 | `harper-ls-aarch64-unknown-linux-gnu.tar.gz` | 14.2 MB | 64 MB |
| macOS Intel | `harper-ls-x86_64-apple-darwin.tar.gz` | 14.1 MB | 62 MB |
| macOS Apple silicon | `harper-ls-aarch64-apple-darwin.tar.gz` | 14.0 MB | 61 MB |
| Windows x86-64 | `harper-ls-x86_64-pc-windows-msvc.zip` | 13.8 MB | 62 MB |

Harper publishes no checksums, so the SHA-256s in `src/tools.rs` are of
the files as they were downloaded from the release on 13 November 2026:
they pin what was there then, and prove no more than that. There's no
build for Windows on ARM; there the doctor says Harper isn't found, with
no fix row, and a Harper of your own is used.

## What it may do

- **Run `harper-ls`** (`permissions.process`), where the pack installs it
  (`/tools/*`) or it's found, as the language server; and ask its
  version for the doctor. No other program.
- **Reach `github.com`** and the hosts GitHub sends release downloads
  from (`permissions.network`): to download Harper, when you ask. Nothing
  else, and nothing of yours.
- **Write in the project** (`filesystem = "project-write"`): the word
  list, `.fenix/words.txt`, when you add a word or open it. Nothing else
  is written, and a `prose.words` outside the project is refused.
- **Read where the cursor is** (`editor = "read"`): the word under it
  or your selection, for `prose.add-word`.

Harper itself runs as a language server of the project, as any does: it's
given the files you have open, and reads and writes the word list.

## Building

```text
cargo xtask pack extensions/prose dist
cargo xtask install extensions/prose
cargo test --manifest-path extensions/prose/Cargo.toml
```

`cargo test` runs the pack on the test kit's fake host: Harper is added
for exactly the languages named, with its settings, and taken away when
the project switches it off or names none; a setting changed is set again
where it changed; each machine's install is its release and checksum; the
word under the cursor goes in the list where Harper reads it, sorted and
once, and Harper is told; and the data -- that the manifest lists every
command the code and keymap use, its settings against what the code
reads, the lesson (`src/data.rs`, `src/tests.rs`).

What can't be tested here is Harper in Fenix: that it attaches, that a
finding is drawn, and what `SPC c a` shows.
