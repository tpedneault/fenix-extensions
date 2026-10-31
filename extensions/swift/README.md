# Swift

Swift in Fenix, on Linux and macOS: highlighting, folding and indenting
from tree-sitter-swift (Fenix has no Swift of its own), sourcekit-lsp as
the language server, swift-format or SwiftFormat as the formatter,
SwiftLint on save, SwiftPM's tasks, the project doctor's checks with
their fixes, snippets, and a command-line package to start from. Built
on lang-kit (`crates/lang-kit`), as each language pack is.

It starts with Fenix. The server and the formatter are set whenever
they're found; the doctor checks, and the tasks run, in a Swift project:
one with a `Package.swift`, an Xcode project, or Swift files at its top
or in `Sources/`. The first Swift file saved in a folder makes it one.

## Swift itself

The pack doesn't install Swift: a toolchain is the platform's. The
doctor's fix row (`SPC m g`, `swift.get-swift`) opens swift.org's page
for this machine, which says how: swiftly (`swiftly install latest`) on
Linux and macOS, Xcode or its command line tools on macOS, the installer
on Windows.

Swift is looked for on the `PATH`, and where swiftly puts it
(`~/.local/share/swiftly/bin` on Linux, `~/.swiftly/bin` on macOS),
where the tarballs usually go (`/opt/swift/usr/bin`), the command line
tools' and Xcode's folders, and Homebrew's. sourcekit-lsp and
swift-format are looked for beside the `swift` found first, so they're
its toolchain's.

## The server

`sourcekit-lsp`, which ships with every toolchain. On macOS without one
found, Xcode's: `xcrun sourcekit-lsp`. It reads a package by itself; an
Xcode project needs a `compile_commands.json` or a Build Server Protocol
config (xcode-build-server) for its cross-file features.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`. `swift.formatter`:

- **auto** (the default): SwiftFormat in a project with a `.swiftformat`
  (and no `.swift-format`), else swift-format.
- **swift-format**: `swift-format format --assume-filename <file>`, its
  `.swift-format` found from the file's folder up. The one installed, or
  Swift 6's own `swift format` (Swift 5 has none).
- **SwiftFormat**: Nick Lockwood's, `swiftformat --stdinpath <file>`,
  its `.swiftformat` found from the file's path.
- **off**: Fenix's own.

On macOS the doctor's fix rows install either with Homebrew.

## SwiftLint

On a saved Swift file, in a project with a `.swiftlint.yml`
(`swift.swiftlint` = `project`), or with SwiftLint's default rules
anywhere (`always`). It runs in the project, so its config (and those
nested beside the file) is read, and a file the config excludes stays
unlinted. Its rule is each problem's code.

The doctor's fix row installs SwiftLint 0.65.1's release into the pack's
tools folder, checked against its SHA-256: Linux on x86_64 and arm64
(which links libcurl and libxml2, as Swift itself does), and macOS's
universal binary. One you installed yourself (Homebrew, Mint) is used
instead.

## Tasks

In a package (`SPC p t`): *build*, *build release*, *run* (*run
<product>* for each executable product, or each executable target where
it names none), *test*, *resolve* and *update* (`swift package`), *lint*
(SwiftLint, when it lints the project), and *format* (the formatter, in
place, on `Package.swift`, `Sources`, `Tests` and `Plugins`). The
project's `.env` goes to each; a task of the same name in
`.fenix/tools.json` wins.

`Package.swift` is read lightly, without running it: its first line's
`swift-tools-version` and the products it names.

## The doctor

| Check | | Fix |
|---|---|---|
| Swift | `swift --version`, at least `Package.swift`'s tools version; needed in a package | swift.org's page for this OS |
| sourcekit-lsp | found beside Swift, or Xcode's (`xcrun --find`) | swift.org's page |
| swift-format | when it formats: the program, or `swift format --version` | Homebrew, on macOS |
| SwiftFormat | when it formats | Homebrew, on macOS |
| SwiftLint | when it lints | SwiftLint 0.65.1's release |

## Settings

| Setting | Default | |
|---|---|---|
| `swift.formatter` | `auto` | `auto`, `swift-format`, `SwiftFormat` or `off` |
| `swift.swiftlint` | `project` | `project`, `always` or `off` |

## Files

`*.swift` and `*.swiftinterface` are Swift; `.swift-format` and
`Package.resolved` are JSON. `gcc` comments with `//`.

## Snippets

| Keys | |
|---|---|
| `struct`, `class`, `enum`, `protocol`, `ext`, `codable`, `actor` | types: an enum with associated values, an extension conforming to a protocol |
| `func`, `afunc`, `closure`, `main` | functions: async throwing, `@main` |
| `guard`, `iflet`, `switch`, `docatch` | control flow |
| `task` | `Task { }` |
| `view`, `observable` | a SwiftUI view with its `#Preview`, an `@Observable` model |
| `test`, `suite`, `xctest` | Swift Testing's `@Test` and `#expect`, a parameterized `@Suite`, an XCTest case |

## Project template

**Swift · command-line tool**: a package with a library target that does
the work, an executable over it, Swift Testing tests, and a `.gitignore`.

## Keys

After `SPC m` in a package or a Swift file: `d` checks the project
again, `g` says how to get Swift.

## Not yet

- **Debugging**: lldb-dap (Swift 6's toolchain has it) as the debug
  adapter.
- **Xcode projects' tasks**: xcodebuild's schemes.
- **SwiftFormat on Linux** from its release; it's found if you've built
  or installed it.
- **Typed throws** (`throws(MyError)`): the grammar doesn't read them yet.
