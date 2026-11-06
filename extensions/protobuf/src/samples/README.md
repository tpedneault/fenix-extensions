What the tests read: buf's and grpcurl's own output, as they printed it.

- `buf-lint.json`, `buf-lint-compile.json`, `buf-breaking.json`: buf 1.75.0's
  `--error-format json` on `orbit.proto` and a sibling breaking its rules,
  one that doesn't compile, and `orbit.proto` changed against `main`.
- `grpcurl-*.txt`: grpcurl 1.9.4 against a server of `orbit.proto` with
  reflection (`list`, `describe`, a call with `-v`, a server stream, a
  NotFound), and `describe` from a `.proto` file of nested types
  (`-import-path`, `-proto`).
- `buf.yaml`, `buf.gen.yaml`: a v2 workspace and template.
