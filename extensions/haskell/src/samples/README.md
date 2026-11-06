# Samples

What the tests read:

- `Orbit.hs`: Haskell the grammar's queries run on.
- `orbit.cabal`: a package with a library and an internal one, two
  executables, a test suite and a benchmark, a `common` stanza and a
  conditional.
- `package.yaml`: hpack's, for a Stack project with no `.cabal` file.
- `Hints.hs` and `hlint.json`: a file, and what `hlint --json` says of it
  -- a redundant bracket, `print`, a lambda, an eta reduction, a hint
  ignored -- and of another file that doesn't parse.

`hlint.json` is written in the format hlint 3.10 prints, not captured
from a run.
