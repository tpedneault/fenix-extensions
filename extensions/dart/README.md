# Dart

Dart and Flutter in Fenix: highlighting, folding and indenting from
tree-sitter-dart (Fenix has no Dart of its own), the SDK's analysis
server as the language server, `dart format` as the formatter, debugging
through the SDK's debug adapters (`dart debug_adapter`, `flutter
debug_adapter`), tasks for a Dart package and for a Flutter app, `flutter
run` on a device you pick with hot reload and restart, the project
doctor's checks, the pub packages page, snippets for Dart and for
widgets, and projects to start from. Built on lang-kit
(`crates/lang-kit`), as each language pack is. It needs Fenix 1.5 or
later (extension API 1.5).

It starts with Fenix. The server, the formatter and the debug adapter are
set whenever Dart is found; the doctor checks, and the tasks run, in a
project with a `pubspec.yaml`. A pubspec that depends on Flutter
(`flutter`, `flutter_test`, ...) makes it a Flutter app, and the pack
works as Flutter's tools do.

## The SDK

The pack doesn't install Dart or Flutter: both come with their own
installers, and Flutter is several gigabytes. The doctor's fix rows
(`dart.get-dart`, `dart.get-flutter`) say how for this machine and open
the page: `brew install dart` (after `brew tap dart-lang/dart`) on macOS,
`choco install dart-sdk` on Windows, the apt repository on Linux --
dart.dev/get-dart -- or Flutter, which has Dart in it.

`dart` and `flutter` are looked for, in this order:

1. the settings `dart.sdk` and `dart.flutter` (a program, or its folder);
2. the project's own pinned Flutter, FVM's `.fvm/flutter_sdk/bin`;
3. `PATH`;
4. where Flutter's install guide unpacks it (`~/flutter/bin`,
   `~/development/flutter/bin`, ...), the package managers' folders and
   `C:\src\flutter\bin`; Dart's own (`/usr/lib/dart/bin`, Homebrew's,
   `C:\tools\dart-sdk\bin`); and **beside the `flutter` found**: Flutter
   ships its Dart as `flutter/bin/dart`, so a machine with only Flutter
   has both.

**Which Dart.** One `dart` runs the server, the formatter, the debug
adapter and the Dart tasks, and in a Flutter app it should be Flutter's
own: the Dart a Flutter version was built with. The doctor's *Dart SDK*
row says which it is -- `3.14.0 stable (standalone, /usr/lib/dart/bin/dart)`
or `(Flutter's, ~/flutter/bin/dart)` -- and warns when a Flutter app is
using a standalone one: put Flutter's `bin` first on `PATH`, or set
`dart.sdk` to `flutter/bin/dart`.

## The server

`dart language-server`: the analysis server, over LSP, which ships with
the SDK. It's where a Dart file's problems come from -- errors, warnings,
hints and lints from `analysis_options.yaml` -- and completion,
hover, go to definition, references, rename, code actions and `dart fix`'s
quick fixes (`SPC c a`). There's no linter beside it: `dart analyze`
would say the same a second time. `dart analyze` is a task (below), for
the whole project at once; each line has its `path:line:col`, and `Enter`
on it opens the place.

It needs the packages resolved: without `.dart_tool/package_config.json`
every `package:` import is an error. The doctor's *Packages* row says so
and its fix runs `pub get` (`SPC m g`).

## Formatting

`SPC c F`, and saving with `editor.format_on_save`: `dart format
--stdin-name <file>`, reading the buffer on its standard input. It finds
the file's package from the name, so its language version and the
`formatter:` rules in `analysis_options.yaml` (`page_width`,
`trailing_commas`, Dart 3.7 and later) are the project's. A file with a
syntax error is left as it was.

## Tasks

In a Dart package (`SPC p t`): *pub get*, *run* (`dart run`, when
`bin/<package>.dart` is there) and *run <name>* for each other file in
`bin/` (*run main* for a `lib/main.dart` alone), *run (attachable)*
(`dart run --enable-vm-service=0 --write-service-info=...`, for the
attach launch), *test* (`dart test`, when `test/` is there or `test` is a
dev dependency), *analyze*, *format* (`dart format .`), *compile exe*,
*pub outdated*, *pub upgrade*, *fix preview* (`dart fix --dry-run`) and
*fix apply* (`dart fix --apply`).

In a Flutter app: *pub get* (`flutter pub get`), *run*, *test* (`flutter
test`), *analyze* (`flutter analyze`), *format*, a *build* for each
platform it has a folder for and this machine can make -- `apk` and
`appbundle` (`android/`), `web`, `ios` and `macos` on a Mac, `linux` on
Linux, `windows` on Windows -- *devices*, *doctor* (`flutter doctor
-v`), *clean*, *pub outdated*, *pub upgrade*, and *fix* as above.

`dart fix --apply` rewrites files all over the project, so it has no key,
it's last in the list, and *fix preview* is the one before it: look,
then apply (and `git diff`).

A package in a pub workspace (`resolution: workspace`) leaves resolving
to the workspace's root: no *pub get* or *pub upgrade* in it.

**Running tests** is through these tasks for now (*test*, with `SPC p t`):
their output is `dart test`'s or `flutter test`'s own. Fenix's test
explorer -- the tests page, the gutter, a test run from its line -- needs
a test provider, which this pack doesn't have yet.

## Flutter: run on a device

`SPC m e`, *Dart: run Flutter on a device*: `flutter devices --machine`
lists what's there -- emulators, phones, Linux, macOS, Windows, Chrome --
and you pick one, or, with one only, it's used. Then `flutter run -d
<device> --pid-file .dart_tool/fenix-flutter.pid` runs as a **background
task**, in a buffer of its own (`*flutter run*`), so the task pane stays
free for tests and builds. `SPC m x` stops it. *Dart: list Flutter's
devices* (`SPC m D`) says what Flutter finds; so does a start in a
Flutter app, quietly: it gives *Debug* a launch for each device.

The *run* task starts `flutter run` too, in the task pane and with no
device chosen, which Flutter picks if there's one.

### Hot reload and hot restart

`SPC m r` and `SPC m R`. Flutter's tool takes SIGUSR1 for a hot reload and
SIGUSR2 for a hot restart, and the pid file says whose: the pack reads it
and runs `kill -s USR1 <pid>`. It works for a `flutter run` started by
the picker, the *run* task, or by hand, as long as it was started with
`--pid-file .dart_tool/fenix-flutter.pid`.

Why not send `r` and `R`? Fenix's extension API (1.5) has no way to write
to a task or a terminal: a process the extension starts itself can be
written to, and `flutter run` there would have no terminal to draw its
output in. So:

- **Windows has no signals.** *Dart: run Flutter on a device, in a
  terminal* (`dart.run-in-terminal`, no key) runs `flutter run` in a
  terminal pane, where `r` reloads and `R` restarts, as in any terminal;
  the two commands say so on Windows.
- A pid file left by a run that's over is said when `kill` finds no such
  process; a run that ended leaves the file, and the next one writes it
  again.
- The task's buffer doesn't take keys: `r` there isn't a reload.

## Debugging

`SPC u U` offers the launches; `SPC u b` sets a breakpoint. The adapter is
the SDK's, over stdio: `dart debug_adapter`, or `flutter debug_adapter`
in a Flutter app (when Flutter is found). It's set for Dart files, so
that's one adapter per project, by what the project is.

Launches (`launch/`, and worked out as the pack runs):

- **Dart: run this file**: the file you're in, with `${project}` as its
  folder. A test file runs its tests, in the debugger.
- **Dart: run bin/\<name\>.dart**: one for each program in `bin/`.
- **Dart: attach to a program started with --write-service-info**: a
  running program's VM service, found in `.dart_tool/fenix-vm-service.json`
  -- the *run (attachable)* task writes it.
- In a Flutter app: **Flutter: run**, **Flutter: run this file** (a
  `main_dev.dart`), and **Flutter: run on \<device\>** for each device
  `flutter devices` listed, which sends `flutter run -d <id>` the way
  `toolArgs` do.

The debug console is where the program's output goes, and a program that
reads the keyboard can't be answered there: Fenix doesn't answer the
adapter's request for a terminal. Run it as a task for that.

What's not here: **debugging a Flutter test**. `flutter test` needs
`flutter debug_adapter --test`, a second mode of the adapter, and Fenix
has one adapter for a language. A Dart test (`dart test`) runs in the
debugger as *Dart: run this file*.

## The doctor

| Check | | Fix |
|---|---|---|
| Dart SDK | `dart --version`: its version and channel, which Dart it is, held to the pubspec's `environment: sdk:`; needed in a package | the page that says how to get Dart |
| Flutter | in a Flutter app: `flutter --version --machine`, held to `environment: flutter:`; its channel and the Dart it has | the page that says how to get Flutter |
| Packages | whether `pub get` has run (`.dart_tool/package_config.json`) | `pub get` |

`SPC m d` checks again, and the pack looks for the programs again when a
task ends or `dart.sdk` or `dart.flutter` changes.

## Packages

`SPC m p` (`dart.packages`): the pub packages -- `dart pub deps --json`,
and `dart pub outdated --json` for what's newer -- the outdated first.
`u` upgrades one past its constraint (`pub upgrade --major-versions
<name>`), `U` all, `a` adds one (`pub add`; `name@^1.2.3` or `dev:name`),
`d` removes one. In a Flutter app they're `flutter pub ...`. A package
pub.dev has a security advisory against is in the vulnerable tab (a newer
pub says so in `pub outdated --json`, as 3.14's does).

## Settings

| Setting | Default | |
|---|---|---|
| `dart.sdk` | empty | The `dart` to use: its program, or its `bin` folder |
| `dart.flutter` | empty | The `flutter` to use: its program, or its `bin` folder |

## Files

`*.dart` is Dart; `//` comments with `gcc`. `pubspec.yaml` and
`analysis_options.yaml` are YAML, which Fenix has, and so is
`pubspec.lock`, which the pack tells it.

## Snippets

| Keys | |
|---|---|
| `main`, `mains` | `main`, and an async `main(List<String> args)` |
| `class`, `data`, `enum`, `sealed`, `ext`, `mixin`, `rec` | a class with final fields; an immutable class with `==`, `hashCode`, `toString` and `copyWith`; an enhanced enum; a sealed hierarchy with its exhaustive `switch`; an extension; a mixin; a record typedef |
| `fut`, `stream`, `trycatch`, `switch`, `forin` | an async function, an `async*` generator, `try`/`on`/`catch`/`finally`, a switch expression, `for-in` |
| `test`, `group`, `testw` | package:test's `test` and `group` with `setUp`, flutter_test's `testWidgets` |
| `stless`, `stful`, `build`, `init`, `setstate` | a `StatelessWidget`; a `StatefulWidget` with its `State`; the `build` method; `initState` and `dispose` around a controller; `setState` |
| `scaffold`, `futurebuilder`, `runapp` | a `Scaffold` with an app bar, a `FutureBuilder`, `main` with `runApp` and a `MaterialApp` |

Each expands to Dart that parses, with the fields to fill in (`data.rs`
checks that, with the grammar).

## Project templates

In the new-project wizard (`SPC p n`), under *Dart*:

- **Dart · command-line program**: a package with a program in `bin/`
  that parses arguments with `package:args`, a library in `lib/` that does
  the work, `package:test` tests and `analysis_options.yaml` with
  `package:lints`; then `dart pub get`.
- **Dart · library**: a package for pub.dev: `lib/<name>.dart` exporting
  `lib/src/`, an example, tests, a `CHANGELOG.md`.
- **Flutter · app**: `flutter create` runs in the new folder, with
  `--project-name`, `--org` (asked) and the platforms you pick: all,
  mobile, web or desktop. The template has no files of its own:
  `flutter create` writes them, the counter app, and runs `pub get`.

## Project kind

Fenix 1.6 and later know a Dart project (`DART`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `pubspec.yaml` at its top, and opened on `lib/main.dart` or `bin/<folder>.dart` when you've had none of its files open. Fenix doesn't read `pubspec.yaml`, so it can't tell a Flutter app from a Dart package: `kind = "flutter"` in the project's `.fenix/settings.toml` makes it a Flutter one (`FLUT`). `kind = "dart"` in `.fenix/settings.toml` says it by hand. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in a Dart package or file (`C-k m` with standard keys):

| Vim | Standard | |
|---|---|---|
| `SPC m p` | `C-k m p` | pub packages |
| `SPC m d` | `C-k m d` | check the project again |
| `SPC m g` | `C-k m g` | pub get |
| `SPC m e` | `C-k m e` | Flutter: run on a device |
| `SPC m r` | `C-k m r` | Flutter: hot reload |
| `SPC m R` | `C-k m R` | Flutter: hot restart |
| `SPC m x` | `C-k m x` | Flutter: stop the run |
| `SPC m D` | `C-k m D` | Flutter: list the devices |

`dart.run-in-terminal`, `dart.get-dart` and `dart.get-flutter` are in
`SPC :`; `dart fix --apply` is a task, with no key.

## Grammar

`grammars/tree-sitter-dart.wasm` is UserNobody14's tree-sitter-dart,
built by the pack: `grammars/README.md` says from which commit and how.
It's MIT-licensed (`grammars/LICENSE-tree-sitter-dart`). `queries/dart/`
is that grammar's `highlights.scm` (MIT) with Fenix's capture names,
and the pack's own `folds.scm` and `indents.scm`.

## What it may do

- **Run `dart`** (`permissions.process`): the analysis server, the
  formatter, the debug adapter, `pub`, the tests and the tasks you start,
  and `dart --version` for the doctor.
- **Run `flutter`**: `flutter --version --machine` and `flutter devices
  --machine` (in a Flutter app, as Fenix starts, and when you ask for the
  devices), and the tasks and `flutter run` you start.
- **Run `kill`**: only `kill -s USR1 <pid>` and `kill -s USR2 <pid>`, for
  hot reload and restart, with the pid `flutter run` wrote to
  `.dart_tool/fenix-flutter.pid`.
- **Run what `dart.sdk` and `dart.flutter` name**, when you set them.
- **Read the project's files** (`filesystem = "project-read"`):
  `pubspec.yaml`, the names in `bin/` and the platform folders, and the
  pid file.

It asks for nothing else: no network (the pages it opens for installing
the SDK open in your browser, through Fenix), no writing to the project,
no editor, no tools folder.

## Building

```text
cargo xtask pack extensions/dart dist
cargo xtask install extensions/dart
cargo test --manifest-path extensions/dart/Cargo.toml
```

`cargo test` runs the pack on the test kit's fake host, which plays
Fenix's part: the server, formatter, adapter and launches set for a Dart
package, a Flutter app and a machine with neither; the doctor's rows; the
device picker, `flutter run` as a task and in a terminal, stopping it,
the pid file and the `kill` it sends; the pub page, drawn. Beside that,
the pieces on what the tools print (`src/samples/`):

- **Recorded from Dart 3.14.0** (`dart create`, then `pub add`):
  `dart --version`, `dart pub deps --json`, `dart pub outdated --json`,
  `pubspec.yaml`. The formatter's standard-input form, the analysis
  server's LSP diagnostics, the debug adapter (launch and attach) and the
  templates were run by hand against the same SDK, and the pack's
  grammar parses the sample and every snippet.
- **Written from Flutter's documented formats**, not recorded: `flutter
  devices --machine`, `flutter --version --machine`, a Flutter app's
  `pubspec.yaml`, and `flutter run`'s `--pid-file` and signals. No
  Flutter SDK was installed where this was written.
- One pub advisory (`isCurrentAffectedByAdvisory`) is the recorded
  outdated list with a field changed, since no SDK package has one.
