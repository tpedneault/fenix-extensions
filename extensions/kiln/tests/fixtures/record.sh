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
#   out/own/   kiln's answers about Kiln's own project, and its kiln.toml
#   out/demo/  kiln's answers about the demo
#   out/edits/ what the editing commands print, done and refused, run on
#              a copy of the demo (see "The editing commands" below)
#   out/exports/ what `kiln export` prints for the Exports tab, file targets
#              and Confluence ones planned offline, run on copies of the
#              demo (see "Exports" below)
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

# -- The editing commands ----------------------------------------------------
#
# What each edit prints, done and refused, as the extension runs it: its
# values as one argument each (`--title=…`, `--to=…`), `--` before the
# positional ones, so a title or ID starting with `-` is never an option.
# On a throwaway copy of the demo, so nothing recorded above changes, and
# in this order (each edit sees the ones before it). out/edits/NAME.*.
#
# Not recordable from here: exit 1, "changed on disk", which needs a file
# changed between kiln's read and its write; the tests write that line by
# hand.
edits="$tmp/edits"
cp -r "$demo" "$edits"
cd "$edits"
e="$out/edits"
rec "$e" set-status txt set -- TASK-003 status=doing
rec "$e" set-status-unknown txt set -- TASK-003 status=finished
rec "$e" set-parent-cycle txt set -- TASK-002 parent=TASK-007
rec "$e" set-parent txt set -- TASK-008 parent=TASK-002
rec "$e" set-parent-none txt set -- TASK-008 parent=
rec "$e" set-due-bad txt set -- TASK-003 due=soon
rec "$e" set-due txt set -- TASK-003 due=2030-01-31
rec "$e" set-due-none txt set -- TASK-003 due=
rec "$e" set-owner txt set -- TASK-008 owner=ana
rec "$e" set-owner-none txt set -- TASK-008 owner=
rec "$e" set-title-dash txt set -- TASK-008 "title=-v, a title like an option"
rec "$e" set-title-empty txt set -- TASK-008 title=
rec "$e" link txt link -- TASK-003 implements REQ-002
rec "$e" link-twice txt link -- TASK-003 implements REQ-002
rec "$e" link-wrong-source txt link -- REQ-001 verifies TEST-001
rec "$e" link-inverse txt link -- TEST-001 verified-by REQ-001
rec "$e" unlink txt unlink -- TASK-003 implements REQ-002
rec "$e" unlink-missing txt unlink -- TASK-003 implements REQ-002
rec "$e" new json new "--title=Write the manual" --format json -- task
rec "$e" new-text txt new "--title=Power fails mid-write" -- risk
rec "$e" new-child json new --parent=TASK-002 "--title=Index the readings" --format json -- task
rec "$e" new-child-wrong json new --parent=REQ-001 --title=Misplaced --format json -- task
rec "$e" new-unknown-type json new --title=x --format json -- nosuch
# A title like an option, given safely, then as `--title VALUE` would
# give it: clap takes it for an option and refuses.
rec "$e" new-dash json new "--title=-v, a title like an option" --format json -- task
rec "$e" new-dash-unsafe json new task --title "-v, a title like an option" --format json
rec "$e" renumber txt renumber --to=TASK-020 -- TASK-008
rec "$e" renumber-taken txt renumber --to=TASK-001 -- TASK-020
rec "$e" renumber-prefix txt renumber --to=REQ-050 -- TASK-020
rec "$e" renumber-dash txt renumber --to=-x -- TASK-020

# -- Exports -----------------------------------------------------------------
#
# What `kiln export` prints, as the Exports tab runs it: one target named
# after `--` (a name is never read as an option), `--dry-run --format json`
# to read, `--format json` to write file targets, `--yes --format json` to
# publish. On throwaway copies of the demo, so nothing above changes, in a
# subshell whose HOME and XDG folders are in the temporary folder (kiln's
# page cache and diagram cache go there, not into this machine's).
#
# Confluence is never reached. `[confluence] url` is http://127.0.0.1:9
# (nothing listens there: a connection is refused at once), and a plan is
# made offline from a snapshot written below (`--snapshot`, the format of
# kiln-confluence's `Snapshot`, crates/kiln-confluence/src/model.rs). With
# no token, kiln stops before connecting; recorded as this machine gives it:
# with no Secret Service (a headless container), the credential store's
# own error (exit 2); on a desktop with one and no token stored, kiln's
# "no token for …" line instead, which the tests also read by hand
# (../hand/README). When strace is installed, the publish without a token
# is run under it and the script fails if kiln opened any network
# connection. The one run with a token (a made-up one, in the environment
# of that one command) points at 127.0.0.1:9 and records kiln's real exit
# 3. What only a server gives -- `applied`, a refusal at publish, a publish
# stopped half-way -- is hand-written in ../hand/.
(
x="$out/exports"
export HOME="$tmp/home" XDG_CONFIG_HOME="$tmp/home/config" XDG_STATE_HOME="$tmp/home/state" XDG_CACHE_HOME="$tmp/home/cache" XDG_DATA_HOME="$tmp/home/data"
mkdir -p "$HOME"
exp="$tmp/exports"
cp -r "$demo" "$exp"
cd "$exp"
# A file target, read, written, read again unchanged, then after an edit.
rec "$x" dry-run json export --dry-run --format json -- design-doc
rec "$x" dry-run txt export --dry-run -- design-doc
rec "$x" write json export --format json -- design-doc
rec "$x" dry-run-unchanged json export --dry-run --format json -- design-doc
rec "$x" dry-run-unchanged txt export --dry-run -- design-doc
k set TASK-003 status=doing
rec "$x" dry-run-update json export --dry-run --format json -- design-doc
rec "$x" dry-run-update txt export --dry-run -- design-doc
# A second file target whose template places only the requirements, so it
# leaves the rest out; a target split into sections, whose template then
# loses a section, so a file it wrote is removed.
printf '# Requirements\n\n<!-- kiln:requirements -->\n' > design/templates/requirements.md
cat >> kiln.toml <<'EOF'

[[export]]
name = "requirements-html"
format = "html"
title = "Demo — requirements"
template = "design/templates/requirements.md"
path = "docs/requirements.html"

[[export]]
name = "sections-md"
format = "markdown"
layout = "sections"
title = "Demo — in sections"
template = "design/templates/sections.md"
path = "docs/sections"
EOF
printf '# Requirements\n\n<!-- kiln:requirements -->\n\n# Risks\n\n<!-- kiln:items type="risk" -->\n' > design/templates/sections.md
git add kiln.toml design/templates
commit "2026-03-02T10:00:00Z" "Demo: two more file targets"
rec "$x" dry-run-left-out json export --dry-run --format json -- requirements-html
rec "$x" dry-run-left-out txt export --dry-run -- requirements-html
# Every target, none named, and the two named as W names them.
rec "$x" dry-run-all json export --dry-run --format json
rec "$x" write-two json export --format json -- design-doc requirements-html
rec "$x" write-sections json export --format json -- sections-md
printf '# Requirements\n\n<!-- kiln:requirements -->\n' > design/templates/sections.md
rec "$x" dry-run-remove json export --dry-run --format json -- sections-md
rec "$x" dry-run-remove txt export --dry-run -- sections-md
git checkout -q -- design/templates/sections.md
rec "$x" unknown out export --dry-run --format json -- nosuch

# Confluence targets: a document under a parent page, and one adopting an
# existing page.
cat >> kiln.toml <<'EOF'

[confluence]
url = "http://127.0.0.1:9"
space = "DEMO"

[[export]]
name = "design-doc-confluence"
format = "confluence"
title = "Demo — design document"
template = "design/templates/design-doc.md"
[export.confluence]
parent = 123456789

[[export]]
name = "adopted"
format = "confluence"
title = "Demo — adopted"
[export.confluence]
page = 555
EOF
git add kiln.toml
commit "2026-03-03T10:00:00Z" "Demo: two Confluence targets"
# Its configuration now, for the extension's own reading of the targets.
cp kiln.toml "$x/kiln.toml"
snap="$tmp/snapshots"
mkdir -p "$snap"
# Nothing published yet: the parent page alone.
cat > "$snap/empty.json" <<'EOF'
{"anchorPage": 123456789, "anchorTitle": "Demo space home"}
EOF
# Published once, at another commit: the document's page is Kiln's (its
# property), one page Kiln wrote is no longer produced, one isn't Kiln's.
cat > "$snap/update.json" <<'EOF'
{
  "anchorPage": 123456789,
  "anchorTitle": "Demo space home",
  "root": {
    "id": 2001, "title": "Demo — design document", "parentId": 123456789, "version": 3,
    "versionAuthor": "Kiln", "versionWhen": "2026-01-05T10:00:00Z",
    "property": {"schema": 1, "project": "Demo", "export": "design-doc-confluence", "key": "doc",
      "items": {"REQ-001": "0000000000000000000000000000000000000000"},
      "published": "0000000000000000000000000000000000000000",
      "renderHash": "sha256:before", "remoteHash": "sha256:remote", "publishedVersion": 3}
  },
  "descendants": [
    {"id": 2002, "title": "Old section", "parentId": 2001, "version": 2,
     "property": {"schema": 1, "project": "Demo", "export": "design-doc-confluence", "key": "old-section",
       "renderHash": "sha256:x", "remoteHash": "sha256:y", "publishedVersion": 2}},
    {"id": 2003, "title": "Meeting notes", "parentId": 2001, "version": 1}
  ]
}
EOF
# The document's page edited in Confluence after Kiln's last write.
cat > "$snap/drift.json" <<'EOF'
{
  "anchorPage": 123456789,
  "anchorTitle": "Demo space home",
  "root": {
    "id": 2001, "title": "Demo — design document", "parentId": 123456789, "version": 5,
    "versionAuthor": "Ana", "versionWhen": "2026-02-01T09:30:00Z",
    "property": {"schema": 1, "project": "Demo", "export": "design-doc-confluence", "key": "doc",
      "items": {"REQ-001": "0000000000000000000000000000000000000000"},
      "renderHash": "sha256:before", "remoteHash": "sha256:remote", "publishedVersion": 3},
    "bodyHash": "sha256:edited"
  }
}
EOF
# The page to adopt already has someone's content.
cat > "$snap/adopt.json" <<'EOF'
{"anchorPage": 555, "anchorTitle": "Team page", "root": {"id": 555, "title": "Team page", "parentId": 1, "version": 7, "empty": false}}
EOF
mkdir -p "$x/snapshots"
cp "$snap"/*.json "$x/snapshots/"
for s in empty update drift; do
  rec "$x" "confluence-$s" json export --dry-run --snapshot "$snap/$s.json" --format json -- design-doc-confluence
  rec "$x" "confluence-$s" txt export --dry-run --snapshot "$snap/$s.json" -- design-doc-confluence
done
rec "$x" confluence-adopt json export --dry-run --snapshot "$snap/adopt.json" --format json -- adopted
rec "$x" confluence-adopt txt export --dry-run --snapshot "$snap/adopt.json" -- adopted
# No token and no snapshot: the dry run, every target at once (one failing
# target fails them all), a bare export as a task would run it (no file
# written either), and a publish -- each stops before any connection.
rec "$x" confluence-no-token json export --dry-run --format json -- design-doc-confluence
rec "$x" all-no-token json export --dry-run --format json
rec "$x" bare-no-token json export --format json
rec "$x" publish-no-token json export --yes --format json -- design-doc-confluence
if command -v strace >/dev/null; then
  strace -f -e trace=connect -o "$tmp/strace.txt" "$KILN" export --yes --format json -- design-doc-confluence >/dev/null 2>&1 || true
  if grep -E 'AF_INET6?' "$tmp/strace.txt" >/dev/null; then
    echo "record.sh: kiln export --yes without a token opened a network connection:" >&2
    grep -E 'AF_INET6?' "$tmp/strace.txt" >&2
    exit 1
  fi
fi
# A (made-up) token, so kiln connects: to 127.0.0.1:9, where nothing
# listens. Its real network error, exit 3.
(export KILN_CONFLUENCE_TOKEN=not-a-token; rec "$x" confluence-unreachable json export --dry-run --format json -- design-doc-confluence)
# on_drift = "skip": the edited page is left alone, so the plan has nothing
# to do.
sed -i 's|^space = "DEMO"$|space = "DEMO"\non_drift = "skip"|' kiln.toml
rec "$x" confluence-drift-skip json export --dry-run --snapshot "$snap/drift.json" --format json -- design-doc-confluence
rec "$x" confluence-drift-skip txt export --dry-run --snapshot "$snap/drift.json" -- design-doc-confluence
git checkout -q -- kiln.toml
)

own="$tmp/kiln"
git clone -q --no-checkout "$KILN_SOURCE" "$own"
cd "$own"
# A branch is the source's (origin/<it> here); a tag or a commit is itself.
rev="$(git rev-parse -q --verify "origin/$KILN_REV^{commit}" || git rev-parse --verify "$KILN_REV^{commit}")"
git checkout -q --detach "$rev"
mkdir -p "$out/own"
git rev-parse HEAD > "$out/own/commit.txt"
# Its configuration, for the extension's own reading of kiln.toml.
cp kiln.toml "$out/own/kiln.toml"
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
