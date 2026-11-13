Recorded `go test -json` output, for the provider's tests (src/provider/drive.rs).

orbit/   the sample project: the plan's (orbit_test.go, geo/geo_test.go) with
         what the plan said to record added: an external test package
         (extra_test.go: t.Parallel subtests, a failing example, a fuzz test
         with seeds, names with regexp metacharacters), a helper in another
         file, os.Exit in a test (crash/), a TestMain that panics (boot/), a
         slow test (slow/), a file behind //go:build integration (tagged/).
         geo/geo_test.go.broken.txt and .vet.txt are geo's test with a
         compile error and a vet error; copy one over geo/geo_test.go to
         record the broken runs.
ws/      a go.work of two modules (app, lib), `TestDouble` in both.

Each <name>.stdout/.stderr/.exit/.order/.args is a command as Fenix's spawn
gives it: run from the sample's folder (a copy of it for broken*, vet-broken)
by a recorder as in plans/test-providers-recipe.md section 9, the folder's
path replaced by /project. All with Go 1.24.7 (linux/amd64) unless said:

  run-all           go test -json -fullpath ./...   (after go clean -testcache)
  run-all-cached    the same again: slow's package is "(cached)"
  list              go list -e -f <format> ./...
  run-one, run-set, run-subs, run-nested, run-special, run-package
                    the commands the planner makes (.args)
  broken            run-all with geo's test not compiling (Go 1.24: build-output,
                    build-fail, the package's fail with FailedBuild)
  broken-1-23       the same with GOTOOLCHAIN=go1.23.8: the errors on stderr,
                    the package's fail has no FailedBuild
  vet-broken        geo's vet error: Go 1.24 reports it as a build failure
  timeout           example.com/orbit/slow with GOFLAGS=-timeout=1s
  ws-list, ws-run-all, ws-one
                    from ws/, the patterns ./app/... ./lib/...
  ws-dep-broken     ws/ with lib's lib.go not compiling: app, which imports it, fails
                    to build too (FailedBuild names lib)
  setup-failed      go test ./... in ws/: ./... isn't a module of the work

Go 1.23.8 came through GOTOOLCHAIN (it downloaded it). Everything was run on
real Go; the tests read the recordings.
