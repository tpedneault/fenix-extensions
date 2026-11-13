# Changelog

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
