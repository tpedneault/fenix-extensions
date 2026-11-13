# Changelog

## 1.0.0

- The first release, for Fenix 1.5: Elixir: its grammar and HEEx's, Expert as the language server, `mix format`, the compiler's warnings and Credo on save, `mix`'s tasks (deps.get, compile, test, test --failed, the format checks, phx.server and credo where the project has them) and `iex -S mix` in a terminal, the project's checks with fixes that install Hex and Expert, two project templates, a lesson, and snippets.
- Expert's download is not checksummed: its release's checksum file couldn't be read when this was made, so the pack checks none rather than one it guessed. Tests run through the `mix test` tasks; ExUnit as a test provider comes later.
