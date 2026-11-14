# Scala

Scala 2 and 3 in Fenix: their grammar (Fenix has none of its own), Metals as
the language server -- found where Coursier puts it, or installed through
Coursier from the project doctor -- with debugging through Metals' debug
adapter, tasks for sbt, scala-cli, Mill, Gradle and Maven, a REPL, a build
that compiles as sources change, the file you're in run by scala-cli, the
project doctor's checks, snippets, and an sbt project and a scala-cli script
in the new-project wizard. Built on lang-kit (`crates/lang-kit`), as each
language pack is.

It starts with Fenix. Metals is started when a Scala file is opened; the
tasks and the checks are for a project with a build tool's files (or Scala
sources) at its top. It needs Fenix 1.5 or later (extension API 1.5), a JDK 17
or later, and the build tool of your project, which it doesn't install: sbt,
scala-cli, Mill, Gradle or Maven. `cs install scala-cli sbt mill` installs
the first three once there's a Coursier.

## Files

| Files | Read as |
|---|---|
| `*.scala` | Scala |
| `*.sc` | Scala: Ammonite's and scala-cli's scripts, Mill's older build files |
| `*.sbt` | Scala: sbt's build files |
| `*.mill` | Scala: Mill's own (`build.mill`, `package.mill`) |

`//` comments a line.

## The grammar

Read with tree-sitter-scala 0.24.0 (`grammars/`, MIT): Scala 2 and Scala 3,
with indentation syntax, `given` and `using`, `enum`, extension methods,
`opaque` types, `derives`, `end` markers and scala-cli's `//> using`
directives. Highlighted, folded (each class, object, trait, enum, method,
`given` and `extension`, and block comments), with Enter indenting inside
braces and brackets and after what opens an indented block (`=`, `:`, `=>`,
`with`, `then`, `else`, `do`), and the definitions as the scopes `SPC c s` and
the breadcrumbs list: `class Fleet(stations: Seq[Station]) extends
Iterable[Station]:`. The grammar's package ships queries for other editors;
the highlights here are adapted from them for Fenix's capture names, and the
folds and indents are the pack's own (`queries/scala/`). The wasm, its hashes
and its licence are in `grammars/README.md`.

## Metals

Metals is the language server: completion, hover, go to definition and
references, rename, code actions, and the compiler's problems -- Metals
compiles the project through its build server (Bloop, sbt's or scala-cli's
own), so the diagnostics are the compiler's, and there's no linter beside it.

**How it's obtained.** Metals isn't a release binary. Its maintainers' way is
Coursier, which writes a small launcher that runs Metals from Coursier's
cache, so that's what the pack uses, in this order:

1. a `metals` already there: on `PATH`, in Coursier's bin folder
   (`~/.local/share/coursier/bin`, `~/Library/Application Support/Coursier/bin`,
   `%LOCALAPPDATA%\Coursier\data\bin`, `$COURSIER_BIN_DIR`), in Homebrew's,
   `~/.local/bin` and the pack's own folder;
2. otherwise the project doctor (`SPC p h`) has two fix rows, both visible as
   they run: **Coursier** (`f`), then **Metals** (`f` again).

Coursier's row downloads its native launcher for this machine -- 2.1.26, from
its GitHub release: Linux and macOS on x86_64 and arm64, Windows on x86_64 --
checks its SHA-256, unpacks it into the pack's tools folder and uses it.
Coursier publishes no checksum file with its releases, so the hashes the pack
checks (in `src/tools.rs`) were computed from the files as downloaded from the
release, the one thing that could be done: they pin what was reviewed, not an
upstream's word. Metals' row runs `cs install --install-dir ${tools}/metals
metals:1.6.9` as a task, so its progress is in Task Output; that downloads
Metals and its dependencies (about 180 MB) into Coursier's own cache, from Maven
Central, where Maven Central's own checksums and Coursier check them. 1.6.9 is
the stable release the pack was tried with; `cs update metals` in a terminal
(or a `metals` of yours earlier on `PATH`) is how you move on. Why not the other
ways: a Metals jar on its own isn't runnable (its dependencies are resolved by
Coursier), and a launcher of the pack's own would be a second thing to keep
in step with Metals' JVM options.

**A JDK.** Metals 1.6 runs on Java 17 or later (its classes are Java 17's).
The doctor's *JDK* row says which `javac` it found -- `21.0.12.1
(/usr/lib/jvm/java-21/bin/javac)` -- looking, as the Java pack does, in the
setting `scala.jdk`, then `JAVA_HOME`, then `PATH`, and warns below 17. A
runtime alone has no `javac`: Metals and the build tools need a JDK. With
`scala.jdk` set, that JDK's home is Metals' and the tasks' `JAVA_HOME`;
without it the environment's own is used. The pack doesn't install a JDK.

**Run by the pack.** The pack is the language server to Fenix, not a bare
`language-server-set`, as the Java pack is for JDT LS: it starts Metals,
passes each message through, and does three things Fenix can't:

- It tells Metals who its client is (no HTTP server for its doctor, no status
  bar, a debugger) and answers its user configuration: `autoImportBuilds`
  (`scala.auto_import`, all by default), so a build is imported without a
  question Fenix has no prompt for.
- It answers Metals' `window/showMessageRequest` questions where the answer
  is the one you'd give: *Import build*; *Import changes*; with no
  `.scalafmt.conf`, formatting *Run anyway* with scalafmt's defaults (Metals
  would otherwise format nothing and wait); *Not now* to starting its HTTP
  server. Any other question goes to Fenix, which has no prompt for it, as it
  would without the pack.
- It asks Metals for its debug adapter (below).

Without a Metals, Fenix is answered at once (no capabilities, and an error to
what it asks that says to open the project doctor) so it doesn't wait, and the
server starts when one is installed. Metals' logs are in the project's
`.metals/metals.log`; Metals' own `.metals/` and `.bloop/` folders go in your
`.gitignore`.

## Formatting

Metals formats, with the scalafmt your `.scalafmt.conf` pins (`version =
"3.8.3"` -- Metals downloads that scalafmt itself the first time) and its
`runner.dialect`: the format command (`SPC c F`) asks Metals, a resident
process, so it takes milliseconds (about 20 ms for a small file once scalafmt
is loaded) and there is no JVM to start for each format. With no
`.scalafmt.conf` it formats with scalafmt's defaults (the pack answers
Metals' question).

**On save is different.** `editor.format_on_save` runs only the formatter an
extension gives -- a program that reads the text on its stdin and writes the
formatted text -- never a language server's formatting; and the programs there
are scalafmt's, which on the JVM takes seconds to start and, for a version it
hasn't got, to download, past the 3 seconds Fenix gives a formatter. scalafmt's
native build starts in milliseconds but formats only with the version it is.
So the pack has both, each where it works: when `.scalafmt.conf` pins
**3.11.5** -- the version the pack's templates pin -- the doctor's *scalafmt*
row has a fix that installs the native scalafmt 3.11.5 from scalafmt's release
(`scalafmt-<machine>.zip`: again no checksum file, so the pack's hashes are of
the downloaded files), and that's the formatter for saving: `scalafmt --stdin
--assume-filename ${file}`, run in the project so it finds `.scalafmt.conf`.
For any other version, saving doesn't format, and the format command (Metals)
does; the row says so.

## Tasks

`SPC p t`, each in the project's top folder, for the build tools the project
uses. The first tool's are plain; another's are prefixed with its name
(`mill: compile`).

| Tool | Tasks |
|---|---|
| sbt | `compile`, `test`, `run`, `clean`; `run <project>` and `test <project>` (`sbt core/run`) for each project `build.sbt` names (`lazy val core = project`); `scalafix` (`sbt scalafixAll`) when `project/plugins.sbt` has sbt-scalafix |
| scala-cli | `compile .`, `test .`, `run .`, `clean .`; `scalafix` (`scala-cli --power fix .`) when there's a `.scalafix.conf` |
| Mill | `compile` and `test` of every module (`mill __.compile`), `run <module>` for each `object x extends ScalaModule`, `clean`; `scalafix` (`mill __.fix`) with a `ScalafixModule` |
| Gradle | `compileScala`, `compileTestScala`, `test`, `run` with the `application` plugin, `scaladoc`, `clean` |
| Maven | `compile`, `test`, `package`, `clean` |

A Gradle or Maven project counts as Scala when its build says so: the `scala`
plugin, `scala-library`, `scala3-library`, `scala-maven-plugin`. The Java pack
has those tools' own tasks, and its project kind has their files; these are the
Scala ones. A Mill build is found by `build.mill`, `build.mill.scala` or
`build.sc` (a `./mill` wrapper is used when there is one), scala-cli by
`project.scala`, `.scala-build` or `.bsp/scala-cli.json` -- or, with no other
build tool, by `.sc` and `.scala` files at the top.

scalafix is a task only: nothing runs it on save, and it's offered where the
build already has it set up. The project's `.env` goes to each task, and the
`scala.jdk` JDK as `JAVA_HOME`. A task of the same name in `.fenix/tools.json`
wins.

A task's output is read, not typed into, so what needs a terminal or has no
end is a command (`SPC SPC`, or the keys below):

| Command | |
|---|---|
| *Scala: a REPL for the project* | in a terminal: `sbt console`, `scala-cli repl .`, `mill -i <module>.console`, `mvn scala:console`; with no build tool, a bare `scala-cli repl` where scala-cli is. Gradle has none |
| *Scala: compile as sources change* | in the background (one of Fenix's background tasks, its output in a buffer of its own): sbt's `~compile`, `scala-cli compile . --watch`, `mill -w __.compile`. The same command stops it |
| *Scala: run the file you're in with scala-cli* | `scala-cli run <file>`: a script or a single source with its `//> using` directives, in any folder, build file or none |
| *Scala: test the file you're in with scala-cli* | `scala-cli test <file>` |
| *Scala: import the build into Metals again* | Metals' `build-import`: for `scala.auto_import` set to `initial` or `off` |

## Debugging

*Scala: debug the file you're in with Metals* (`SPC m b`) asks Metals to run
the file -- its main class, or its tests if it's a suite -- under its debug
adapter (`debug-adapter-start`, `runOrTestFile`), which Metals starts on a
port of its choosing. The pack connects Fenix's debugger to that port
(`debug-adapter-connect`) and says so: then `SPC u u` and the launch **Scala:
Metals' debug session** start the session, which stops at the breakpoints you
set (`SPC u b`). *Scala: debug the tests of the file you're in* does the same
for `testFile`. Metals decides what runs, so the launch sends no program of its
own. It needs the build read and compiled (Metals' first start takes a
moment), and the file to be one Metals knows: a main class, or a test suite of
a framework it supports (MUnit, ScalaTest, JUnit, ...). What Metals says when it
can't (`No tests could be found in file: ...`) is shown.

This is the Java pack's design, which Fenix 1.5 supports: the language server
starts the adapter, and the pack connects to it. The adapter is Metals' own
(DAP on a TCP port). While the pack was written, Metals 1.6.9 was driven
directly, by a DAP client of the author's, in a scala-cli project: `runOrTestFile`
on a main file (`initialize`, `launch`, `configurationDone`: the program's
output and its exit came back), and `testFile` on a MUnit suite (its results,
with the failing test's diff, came back as output events); `testFile` on a file
with no tests came back as the error above. What was never done is Fenix's
debugger connecting to it.

## The doctor

`SPC p h`:

| Check | | Fix row |
|---|---|---|
| JDK | `javac -version`, 17 or later (a warning below, saying Metals runs on 17); needed in a Scala project; which `javac` it found, and `scala.jdk` when that's the one | none: Temurin, your package manager, `cs java --jvm temurin:21` |
| Coursier | `cs version`, shown while there's no Metals | Coursier 2.1.26 (Linux, macOS; x86_64 and arm64; Windows x86_64) |
| Metals | `metals --version`, optional | Metals 1.6.9, with Coursier |
| sbt, Mill, scala-cli | `sbt --version`, `mill --version`, `scala-cli version`, for each the project uses; Mill's `./mill` first | none: `cs install` them |
| scalafmt | `.scalafmt.conf`'s version and dialect, and which formatter does what | scalafmt 3.11.5 (the native one), when the project pins that |

Gradle and Maven aren't asked: the Java pack checks their wrappers. Starting
an install is said in the message line; how it ended is a notification:
"Installed Coursier 2.1.26", or "Couldn't install Coursier 2.1.26" with which
step failed and why, and *Try again*.

## Snippets

| Key | |
|---|---|
| `main`, `maino`, `app` | an `@main` method (Scala 3); `object Main { def main ... }`; `object Main extends App` |
| `obj`, `cls`, `cc`, `trait` | an object; a class; a case class; a trait |
| `adt`, `enum`, `enump` | a sealed trait and its case classes; an `enum`; an `enum` with parameters and a method |
| `given`, `using`, `ext`, `opaque` | a `given` instance; a method with a `using` clause; extension methods; an opaque type |
| `def`, `defg`, `impl` | a method; a generic one; an implicit class (Scala 2) |
| `match`, `matchb`, `optm` | a `match`; one with braces; a match on an `Option` |
| `for`, `forb`, `fore` | a `for` comprehension; one with braces; a `for ... do` loop |
| `try`, `tryb`, `fut`, `pl` | `try`/`catch`/`finally`, with and without braces; a `Future`; `println` |
| `munit`, `test`, `scalatest` | a MUnit suite; a MUnit test; a ScalaTest `AnyFunSuite` |
| `usc`, `udep`, `utest` | scala-cli's `//> using scala`, `dep`, `test.dep` |
| `sbtdep`, `sbtproj` | sbt's `libraryDependencies +=`; a `lazy val x = (project in file("x"))` |

## New projects

*Scala · sbt project* in the new-project wizard: `build.sbt`,
`project/build.properties` (sbt 1.12.15), `.scalafmt.conf` (scalafmt 3.11.5,
the dialect for the Scala you pick), a `.gitignore` that keeps `target/`,
`.bsp/`, `.bloop/` and `.metals/` out of Git, a `Main` and a MUnit suite.
Scala 3.10.1, 3.3.6 (Scala 3's long-term support line) or 2.13.17: Scala 3's
is an `@main` method, Scala 2's `object Main extends App`.
*Scala · scala-cli script*: a `hello.sc` with its `//> using` directives
(`scala-cli run hello.sc`, `SPC m r`), scalafmt's configuration and a
`.gitignore`. Both run nothing when they're made; the first sbt or scala-cli
run downloads the compiler. Each was built and run with the real tool when the
pack was written (below).

## Settings

| Setting | Default | |
|---|---|---|
| `scala.jdk` | none | the JDK to use -- its `java`, its `bin` folder or its home -- when it isn't `JAVA_HOME`'s or `PATH`'s: the doctor checks it, and Metals and the tasks run with it as `JAVA_HOME` |
| `scala.auto_import` | `all` | when Metals imports the build by itself: `all` (the first time, and each time a build file changes), `initial`, or `off` |

## Project kind

Fenix 1.6 and later know a Scala project (`SC`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `build.sbt`, `build.sc`, `build.mill`, `project.scala` or `.scalafmt.conf` at its top, and opened on `src/main/scala/Main.scala` (or `main.scala`, `{folder}.scala`, `Main.scala`, `project.scala`, `build.sbt`) when you've had none of its files open. `kind = "scala"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "scala"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. A Scala project built by Gradle or Maven has those tools' files, which are the Java pack's kind's: it stays Java until `kind = "scala"` says otherwise. Your home folder is never a project, whatever dotfiles it holds (`.scalafmt.conf` among them). Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and here they are the same list.

## Keys

After `SPC m` in a Scala project or file (`C-k m` with standard keys): `d`
checks the project again, `c` opens a REPL, `w` compiles as sources change
(again to stop), `r` and `t` run and test the file you're in with scala-cli, `b`
debugs it with Metals. Debugging the tests and importing the build again are
commands to pick.

## Not yet

- **Test providers**: ScalaTest and MUnit aren't in the test explorer. Tests
  run through the tasks, `scala-cli test`, and Metals' debug of the tests.
- **A Windows on ARM Coursier or scalafmt**: neither has a release for it.
  Install them yourself and the pack finds them.
- **Scala.js and Scala Native**: they build as the project does, and Metals
  reads them; nothing here is specific to them.
- **Scala worksheets and Metals' decorations** (inferred types, the doctor's
  page, tree view): Fenix has no place for them.

## What it may do

- **Run `metals`, `cs` (or `coursier`), `scala-cli`, `sbt`, `mill` (and
  `./mill`), `gradle` and `mvn` (with their wrappers `gradlew` and `mvnw`),
  `javac`, the JDK you chose (`${setting:scala.jdk}`) and what's in the pack's
  tools folder** (`permissions.process`): the versions for the doctor, Metals
  as the server, the tasks you start, the REPL, the file you run, and the
  installs. No other program. The formatter on save is the native scalafmt in the
  tools folder.
- **Reach `github.com`, `objects.githubusercontent.com` and
  `release-assets.githubusercontent.com`** (`permissions.network`): to download
  Coursier's and scalafmt's releases when you run a fix row. Coursier and the
  build tools reach what they need (Maven Central, GitHub) as they do in a
  terminal; that's theirs, not the pack's.
- **Connect to a local port** (`permissions.sockets`, `localhost` and
  `127.0.0.1`, ports 1024 and up): Metals' debug adapter, on a port Metals
  chooses.
- **Read the project** (`filesystem = "project-read"`): the build files at the
  top and one folder down, `.scalafmt.conf` and `.env`, to see which tools it
  uses. It writes nothing.
- **Read the file you're in** (`editor = "read"`): to run, test and debug it.

## Building

```text
cargo xtask pack extensions/scala dist
cargo xtask install extensions/scala
cargo test --locked --manifest-path extensions/scala/Cargo.toml
```

`cargo test` runs the pack on lang-kit's fake host and the test kit's: its
tasks are set, the doctor reads the versions the tools printed, the fix rows'
downloads name the releases and their SHA-256s for each platform, Metals is
started and answered with what it really said (`src/samples/metals-stdout.bin`),
the debug adapter's port is asked for and connected to, the REPL, the watching
build and the file's run are started, and the native scalafmt becomes the
formatter when its version is pinned (`src/lib.rs`); each piece on its own
(`src/tools.rs`, `doctor.rs`, `tasks.rs`, `project.rs`, `lsp.rs`); and the data
as Fenix reads it (`src/data.rs`) -- the language file, the grammar loaded and
its queries run on a Scala 3 and a Scala 2 sample, the snippets, the project
templates for each Scala, the launch file, and the manifest against the code.
What the tools printed in `src/samples/` was recorded from the real tools on
Linux x86_64 (see `src/samples/README.md`); nothing here has run in a Fenix
window.

## Licence

The pack is MIT OR Apache-2.0, as the repository is. The grammar keeps its own:
tree-sitter-scala's MIT (`grammars/LICENSE-tree-sitter-scala`).
