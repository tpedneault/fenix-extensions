These are the sample projects, reporters and recorded runner outputs the
test explorer's plan (../test-explorer.md) was worked out on: saved here from
a scratch folder so they are not lost. Each pack copies what it needs into its
own fixtures when its provider is written; this folder goes then.

Test explorer (milestone 4) experiments, 13 November 2026. Fixtures for
plans/test-explorer.md; nothing here is in the repository.

samples/     tiny projects: passing, failing, ignored/skipped, parameterised,
             nested tests, one each per runner; *-broken don't compile/collect.
             rust/tests-crash.rs.txt is the test that aborts its binary (copy
             it to tests/crash.rs to use it).
reporters/   the pytest plugin, the Vitest and Jest reporters that print a
             "FENIX-TEST {json}" line per test (what the design proposes).
outputs/     what each command printed, by runner: *.stdout and *.stderr kept
             apart, as Fenix's spawn delivers them. outputs/api-lib.rs and
             world.wit are fenix-extension-api at v1.5.0, for reference.

Tools used (none were on the machine but cargo, go, node; reinstall to rerun):
  cargo 1.97.0, rustc 1.97.0          (on PATH)
  cargo-nextest 0.9.151                cargo install cargo-nextest --locked
  go 1.24.7                            (on PATH)
  pytest 9.2.0, Python 3.13            python3 -m venv venv && venv/bin/pip install pytest
  vitest 5.1.0, Node 22.23.4           cd samples/vitest && npm ci
  jest 30.5.2                          cd samples/jest && npm ci
Build output (target/, node_modules/, the venv) was deleted afterwards.

Added on the second pass (the split plan), 13 November 2026, from fresh
copies in ../m4x (vitest's node_modules, a venv with pytest 9.2.0, go 1.24.7):
  outputs/vitest/discover-reporter.{stdout,stderr,txt}  vitest run -t FENIX_NOTHING
      through fenix-vitest.mjs: every test listed with its line, skip/todo and
      each test.each case included (mode "skip" for all), exit 0, 0.43 s
  outputs/pytest/argsfile-and-boot.txt  pytest -q @ids.txt runs ids with spaces
      and brackets; the boot taking the plugin's folder as argv (a path with a
      space and a quote) collects through -p fenix_tests
  outputs/go/fullpath-and-cache.txt  -fullpath puts the full path in a failure's
      line; a cached package replays its per-test events (Elapsed 0)
