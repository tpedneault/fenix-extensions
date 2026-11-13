# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing SwiftLint ended is a
  notification rather than a line in the message line, since the
  download can take a while and you may be elsewhere -- "Installed
  SwiftLint 0.65.1", and the folder it's in; or "Couldn't install
  SwiftLint 0.65.1", saying whether its download, unpacking it or
  setting it up failed and why, with *Try again*. The newest for the
  tool takes the last one's place, and a failure's is taken away when
  you try again. Starting the install is still said in the message line.
  Installing swift-format or SwiftFormat is Homebrew's, run as a task,
  and is as it was. The README gives standard keys beside Vim's. On
  Fenix 1.0 to 1.4, 1.0.0 stays the one you have.

## 1.0.0

- The first release, for Fenix 1.0: swift on Linux and macOS: sourcekit-lsp, Swift's highlighting, swift-format or SwiftFormat, SwiftLint on save, SwiftPM's tasks, the project's checks with their fixes, snippets for SwiftUI, concurrency and both test frameworks, and a command-line package.
