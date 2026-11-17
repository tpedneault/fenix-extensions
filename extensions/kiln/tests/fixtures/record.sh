#!/usr/bin/env bash
# Records what kiln really prints, for the extension's tests.
#
#   tests/fixtures/record.sh [KILN]        # or KILN=/path/to/kiln record.sh
#
# KILN is the kiln program (default: `kiln` on the PATH). KILN_SOURCE is a
# clone of Kiln's repository (default: ../kiln beside this repository) and
# KILN_REV the revision of it to read (default: master, which a release is
# made from: give the one KILN was built from). It is cloned again into a
# temporary folder, so only that commit is read -- not what's checked out
# or uncommitted there -- and nothing is written to it. The commit read is
# in out/own/commit.txt.
#
# It writes, beside this script:
#
#   demo/      a small Kiln project, made here from nothing with kiln's own
#              commands (init, new, set, link), so every state a page shows
#              is in it -- see "The demo" below. Its source is committed so
#              later tests can read it; this script is what makes it.
#   out/own/   kiln's answers about Kiln's own project
#   out/demo/  kiln's answers about the demo
#   out/version.txt, out/errors/   `kiln --version`, and two errors
#
# `lsp-languages` in out/demo/ and out/own/ is `kiln lsp --languages
# --format json`. Kiln 1.0.0 has no --languages: recorded with it, these
# hold its refusal (exit 2, one stderr line, no stdout), which the tests
# read as such. A kiln that has it records its JSON there instead; until
# one is released, the JSON the tests read is hand-written, in ../hand/
# (not made or touched by this script).
#
# Each answer is `<name>.<ext>` (stdout as kiln printed it), with
# `<name>.stderr` when kiln said something on stderr and `<name>.exit`
# holding its exit code. Nothing written holds a path of this machine's:
# kiln prints project-relative paths, and the one error that names a
# folder has it replaced by `<dir>` (checked at the end).
#
# Run twice, it gives the same files: git's dates and names are fixed, and
# the demo's milestone dates are far from today (K206 compares with
# today).
#
# The demo (project "Demo", a data logger):
#
#   REQ-001 approved      Readings are logged every second (verified by TEST-001)
#     REQ-002 draft       Logged readings survive a power cut (child of REQ-001)
#   REQ-003 approved      The log exports as CSV -- no test: a K201 warning, kept
#   ADR-001 accepted      Store readings in SQLite
#   MS-001 active         First release, due 2030-06-30
#   MS-002 planned        Second release
#   TASK-001 done         Set up the logger                 MS-001
#   TASK-002 doing (ana)  Write readings to SQLite          MS-001  in progress
#     TASK-007 todo       Batch the writes (sub-task)       MS-001  ready
#   TASK-003 todo (bo)    Export the log as CSV             MS-001  ready, owned
#   TASK-004 todo         Add the export button             MS-001  waiting on TASK-003
#   TASK-005 blocked      Ask about retention rules         MS-002  blocked
#   TASK-006 todo         Draft the user guide              MS-002  ready, unowned;
#                         its body mentions REQ-099, which doesn't exist: a
#                         K102 error, kept, so `check` exits 1
#   TASK-008 todo         Tidy the build scripts            no milestone: a
#                         K213 warning, kept, and `status`'s work in no
#                         milestone
#   TEST-001 passing      Readings arrive every second (verifies REQ-001)
#   RISK-001 open         The SD card wears out (REQ-002 mitigates it)
#
# Its history: the first commit (tagged v0.1) has the requirements, the
# decision, the milestones and TASK-001..003 at todo; the second has the
# rest. `diff v0.1` is recorded against it.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KILN="${1:-${KILN:-kiln}}"
KILN="$(command -v "$KILN" || true)"
[ -n "$KILN" ] || { echo "record.sh: no kiln (give its path as KILN or the first argument)" >&2; exit 2; }
KILN_SOURCE="${KILN_SOURCE:-$here/../../../../../kiln}"
KILN_REV="${KILN_REV:-master}"
[ -f "$KILN_SOURCE/kiln.toml" ] || { echo "record.sh: no Kiln repository at $KILN_SOURCE (set KILN_SOURCE)" >&2; exit 2; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# git as nobody's: no global or system configuration, fixed names and dates.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME="Demo" GIT_AUTHOR_EMAIL="demo@example.com" GIT_COMMITTER_NAME="Demo" GIT_COMMITTER_EMAIL="demo@example.com"
export TZ=UTC LC_ALL=C
# kiln's editor and Jira are never reached.
unset KILN_EDITOR VISUAL EDITOR KILN_JIRA_TOKEN KILN_CONFLUENCE_TOKEN KILN_CONFLUENCE_USER || true

commit() { # commit DATE MESSAGE
  GIT_AUTHOR_DATE="$1" GIT_COMMITTER_DATE="$1" git -c commit.gpgsign=false commit -q -m "$2"
}

# -- The demo, made from nothing -------------------------------------------

demo="$tmp/demo"
mkdir -p "$demo"
cd "$demo"
git -c init.defaultBranch=main init -q
k() { "$KILN" "$@" >/dev/null 2>&1 || { echo "record.sh: kiln $* failed" >&2; "$KILN" "$@"; exit 1; }; }
body() { printf '%s\n' "$@" > "$tmp/body.md"; echo "$tmp/body.md"; }

k init --name Demo
k new requirement --title "Readings are logged every second" --body-file "$(body "" "The logger stores one reading a second from each sensor.")"
k new requirement --title "Logged readings survive a power cut" --parent REQ-001 --body-file "$(body "" "A reading written before the power goes stays readable after.")"
k new requirement --title "The log exports as CSV" --body-file "$(body "" "One row per reading, a header first.")"
k set REQ-001 status=approved
k set REQ-003 status=approved
k new decision --title "Store readings in SQLite" --body-file "$(body "" "## Context" "" "Readings must survive a power cut." "" "## Decision" "" "SQLite in WAL mode." "" "## Consequences" "" "One file to back up.")"
k set ADR-001 status=accepted
k new milestone --title "First release" --body-file "$(body "" "Logging and the CSV export.")"
k new milestone --title "Second release" --body-file "$(body "" "Retention and the user guide.")"
k set MS-001 status=active due=2030-06-30
k new task --title "Set up the logger" --link part-of=MS-001 --link implements=REQ-001 --body-file "$(body "" "The service and its configuration.")"
k new task --title "Write readings to SQLite" --link part-of=MS-001 --link implements=REQ-001 --body-file "$(body "" "Through ADR-001's schema.")"
k new task --title "Export the log as CSV" --link part-of=MS-001 --link implements=REQ-003 --tag csv --body-file "$(body "" "Streams rows; never loads the whole log.")"
git add -A
commit "2026-01-05T10:00:00Z" "Demo: the requirements, the decision, the milestones, three tasks"
git tag v0.1

k set TASK-001 status=done
k set TASK-002 status=doing owner=ana
k set TASK-003 owner=bo
k new task --title "Add the export button" --link part-of=MS-001 --link depends-on=TASK-003 --body-file "$(body "" "On the main window, beside the log.")"
k new task --title "Ask about retention rules" --link part-of=MS-002 --body-file "$(body "" "Waiting on legal.")"
k set TASK-005 status=blocked
k new task --title "Draft the user guide" --link part-of=MS-002 --body-file "$(body "" "Follow the outline in REQ-099.")"
k new task --title "Batch the writes" --parent TASK-002 --body-file "$(body "" "A transaction a second, not a reading.")"
k new task --title "Tidy the build scripts" --body-file "$(body "" "One script per platform.")"
k new test --title "Readings arrive every second" --link verifies=REQ-001 --body-file "$(body "" "Run the logger for a minute; count 60 readings a sensor.")"
k set TEST-001 status=passing
k new risk --title "The SD card wears out" --body-file "$(body "" "Writes a second for years.")"
k link REQ-002 mitigates RISK-001
git add -A
commit "2026-02-02T10:00:00Z" "Demo: work under way, a test and a risk"

# -- Recording ---------------------------------------------------------------

# rec DIR NAME EXT ARGS...: kiln ARGS in the current folder, its stdout to
# DIR/NAME.EXT, its stderr (when any) to DIR/NAME.stderr, its exit code to
# DIR/NAME.exit.
rec() {
  local dir="$1" name="$2" ext="$3"
  shift 3
  mkdir -p "$dir"
  local code=0
  "$KILN" "$@" > "$dir/$name.$ext" 2> "$dir/$name.stderr" || code=$?
  echo "$code" > "$dir/$name.exit"
  [ -s "$dir/$name.stderr" ] || rm -f "$dir/$name.stderr"
}

out="$here/out"
rm -rf "$out" "$here/demo"
mkdir -p "$out"

"$KILN" --version > "$out/version.txt"

record_project() { # record_project DIR SHOW-IDS...
  local d="$1"
  shift
  rec "$d" list json list --format json
  rec "$d" list-body json list --body --format json
  rec "$d" status json status --format json
  rec "$d" next json next --format json
  rec "$d" check json check --format json
  for id in "$@"; do
    rec "$d" "show-$id" json show "$id" --format json
  done
  rec "$d" trace json trace --format json
  rec "$d" export-dry-run json export --dry-run --format json
  rec "$d" lsp-languages json lsp --languages --format json
}

cd "$demo"
record_project "$out/demo" REQ-001 REQ-002 ADR-001 MS-001 TASK-002 TASK-004 TEST-001 TASK-006
rec "$out/demo" impact-REQ-001 json impact REQ-001 --format json
rec "$out/demo" impact-TASK-002 json impact TASK-002 --format json
rec "$out/demo" impact-TASK-006 json impact TASK-006 --format json
rec "$out/demo" trace-task json trace --type task --format json
rec "$out/demo" trace-risk json trace --type risk --format json
rec "$out/demo" diff-v0.1 json diff v0.1 --format json
rec "$out/demo" diff-v0.1 txt diff v0.1
# The other way round, what v0.1 would lose: the added items as removed.
rec "$out/demo" diff-removed json diff HEAD..v0.1 --format json
rec "$out/demo" diff-removed txt diff HEAD..v0.1
# Nothing changed since the last commit.
rec "$out/demo" diff-none json diff HEAD --format json
rec "$out/demo" next-unowned json next --unowned --format json
rec "$out/demo" check txt check
rec "$out/errors" unknown-id out show TASK-099 --format json
rec "$out/errors" impact-unknown-id out impact TASK-099 --format json
rec "$out/errors" trace-unknown-type out trace --type nosuch --format json
rec "$out/errors" trace-levels out trace --levels --format json
rec "$out/errors" bad-id out show NOPE-1 --format json
rec "$out/errors" bad-status out set TASK-001 status=finished
rec "$out/errors" diff-unknown-rev out diff v9.9 --format json

own="$tmp/kiln"
git clone -q --no-checkout "$KILN_SOURCE" "$own"
cd "$own"
# A branch is the source's (origin/<it> here); a tag or a commit is itself.
rev="$(git rev-parse -q --verify "origin/$KILN_REV^{commit}" || git rev-parse --verify "$KILN_REV^{commit}")"
git checkout -q --detach "$rev"
mkdir -p "$out/own"
git rev-parse HEAD > "$out/own/commit.txt"
record_project "$out/own" REQ-001 ADR-001 MS-002 TASK-001
rec "$out/own" impact-REQ-001 json impact REQ-001 --format json

empty="$tmp/empty"
mkdir -p "$empty"
cd "$empty"
rec "$out/errors" no-kiln-toml out list --format json
# The folder kiln names is this machine's: `<dir>` instead.
sed -i "s|$empty|<dir>|g" "$out/errors/no-kiln-toml.stderr"

# A project outside any repository: `diff` has nothing to read.
loose="$tmp/loose"
mkdir -p "$loose"
cd "$loose"
"$KILN" init --name Demo >/dev/null 2>&1
rec "$out/errors" diff-no-repo out diff v0.1 --format json
sed -i "s|$loose|<dir>|g" "$out/errors/diff-no-repo.stderr"

# The demo's source, without its git history.
mkdir -p "$here/demo"
(cd "$demo" && git ls-files -z | xargs -0 cp --parents -t "$here/demo")

if grep -rlF -e "$tmp" -e "$HOME" "$out" "$here/demo" >/dev/null; then
  echo "record.sh: a recorded file holds a path of this machine's:" >&2
  grep -rlF -e "$tmp" -e "$HOME" "$out" "$here/demo" >&2
  exit 1
fi
echo "recorded with $("$KILN" --version) into $out"
