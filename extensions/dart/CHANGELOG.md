# Changelog

## 1.1.0

- Fenix 1.6 and later show a project with a `pubspec.yaml` at its top as a Dart project (`DART`), in the project hub, on Home, in the modeline and in the doctor, and open it on `lib/main.dart` or `bin/<folder>.dart` when you've had none of its files open; `kind = "dart"` in a project's `.fenix/settings.toml` says it by hand. Fenix doesn't read `pubspec.yaml`, so it can't tell a Flutter app from a Dart package: `kind = "flutter"` in the project's `.fenix/settings.toml` makes it a Flutter one (`FLUT`). Fenix 1.5 ignores all this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.0.0

- The first release, for Fenix 1.5: Dart and Flutter. Highlighting,
  folding and indenting from tree-sitter-dart; the analysis server
  (`dart language-server`) as the language server, so its problems and
  quick fixes are Fenix's; `dart format` as the formatter; debugging
  through the SDK's adapters (`dart debug_adapter`, `flutter
  debug_adapter` in a Flutter app), with launches for the file you're in,
  each program in `bin/`, attaching to a running program, and `flutter run`
  on each device Flutter lists; tasks for a Dart package (`pub get`,
  `run`, `test`, `analyze`, `format`, `compile exe`, `fix preview`, `fix
  apply`) and for a Flutter app (`flutter pub get`, `run`, `test`,
  `analyze`, a build for each platform it has, `doctor`, `devices`); `flutter
  run` on a device you pick, as a task of its own, with hot reload and hot
  restart as commands (a signal to the pid `flutter run` wrote; not on
  Windows, where there's a terminal to type `r` in instead); the project
  doctor's checks (which Dart SDK is in use and its version, Flutter, whether
  the packages are resolved), with the pages that say how to get each SDK;
  the pub packages page; snippets for Dart and for widgets; and a command-line
  program, a library and a Flutter app to start from. The SDKs aren't
  installed by the pack, and tests run through their tasks: no test
  provider yet.
