# Changelog

## 1.1.0

- Fenix 1.6 and later show a project with a `mix.exs` at its top as an Elixir project (`EX`), in the project hub, on Home, in the modeline and in the doctor, and open it on `lib/<folder>.ex` when you've had none of its files open; `kind = "elixir"` in a project's `.fenix/settings.toml` says it by hand. Fenix 1.5 ignores all this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.0.0

- The first release, for Fenix 1.5: Elixir: its grammar and HEEx's, Expert as the language server, `mix format`, the compiler's warnings and Credo on save, `mix`'s tasks (deps.get, compile, test, test --failed, the format checks, phx.server and credo where the project has them) and `iex -S mix` in a terminal, the project's checks with fixes that install Hex and Expert, two project templates, a lesson, and snippets.
- Expert's download is checked against the SHA-256 its release publishes. Tests run through the `mix test` tasks; ExUnit as a test provider comes later.
