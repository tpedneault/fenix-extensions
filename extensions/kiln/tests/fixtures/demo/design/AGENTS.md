# Working on Demo's design

This folder is Demo's design, kept with [Kiln](https://github.com/tpedneault/kiln):
requirements, decisions and the rest as Markdown files, one item per
file, checked in CI by `kiln check`. If you are a coding agent, read
this before changing a file here or implementing what one describes.
This part is the project's to edit; the section below it is generated.

## Rules

- **Read with Kiln.** `kiln show ID` prints an item, its body and every
  link in and out (`--format json` for all of it as data); `kiln list
  --format json --body` loads every item at once; `kiln impact ID` shows
  everything that depends on one; `kiln trace` shows what is implemented
  and verified, and the gaps.
- **Take work from the plan.** `kiln next --unowned` lists the work
  ready to start that nobody has claimed, earliest milestone first, and
  `kiln status` the plan by milestone. When you take an item, claim it
  and move it past its first status in one command (`kiln set ID
  owner=… status=…`), so another agent or person doesn't take it too;
  when it is finished and `kiln check` is clean, set it to a done status.
  Replace the template's prompts in a body with what the item says
  (`kiln check` reports them, K215).
- **Create and change front matter with Kiln, not by hand.** `kiln new
  TYPE --title "…" [--parent ID] [--link ROLE=ID]… [--tag TAG]…
  [--body-file FILE|-]` allocates the next free ID and writes the item
  whole, then prints its path (`--format json` for its ID too): never
  choose an ID yourself. `kiln set ID status=…` (also `title`, `parent`,
  `owner`, `due`, `tags=a,b`, `jira=KEY`), `kiln link ID ROLE TARGET`
  and `kiln unlink ID ROLE TARGET` keep the file's comments and layout
  and refuse when the file changed under them. Write or change the body
  (everything after the second `+++`) with any editor.
- **Link once, from the source side.** A test says `verifies =
  ["REQ-014"]`; the requirement never says "verified by", which Kiln
  computes. Mention an item in prose as `[[REQ-014]]`; a mention is not
  a link and counts toward nothing.
- **Leave sign-off to people.** Moving a requirement to approved,
  accepting a decision, closing a risk or finishing a milestone is a
  person's call: propose it in the pull request. Moving work forward (a
  task to done, a test to passing when it passes) is fine. `kiln
  signoff` reports every sign-off change in CI, with its author.
- **When you implement or test a requirement, say so in the code.** A
  comment `implements: REQ-014` near the code and `verifies: REQ-014`
  on its test (see "Links" below for the roles code may use) make the
  code part of the trace. Before you open a pull request, `kiln
  affected origin/main` lists the requirements your change touches and
  the tests that verify them; run those.
- **Keep `kiln check` clean.** Run it before committing; errors fail CI.
  Fix what it reports rather than lowering a severity in `[check]`.
- **Don't edit generated files.** Exports listed below are written by
  `kiln export`; change the items or the template and run it again.
- **Two branches, one ID:** after a merge, `kiln check` reports the
  duplicate (K101) and `kiln renumber ID --to NEW` moves one copy and
  every reference to it.

Item files are Markdown with a TOML front matter block between two
`+++` lines; files here without one, like this one, aren't items. The
format is docs/FORMAT.md in Kiln's repository. After changing
`kiln.toml`, run `kiln agents` to bring the section below up to date
(`kiln check` says when it isn't; K113). Everything Kiln keeps lives
under `design/`.

<!-- kiln:agents begin: written by `kiln agents` from kiln.toml; change kiln.toml and run `kiln agents`, not this section -->
## How this project's design is organised

Items are the `.md` files under `design/` that start with `+++`. `kiln new` files each type in its own folder, below; folders carry no meaning to Kiln, and an item's ID is the `id` in its front matter, not its file name.

### Types

| Type | ID | Folder | Statuses | Parent |
|---|---|---|---|---|
| requirement (Requirement) | `REQ-001` | `design/requirements/` | draft (new), proposed, approved, implemented, verified (done), obsolete (closed) | requirement |
| design (Design) | `DES-001` | `design/designs/` | draft (new), accepted, obsolete (closed) | design |
| decision (Decision) | `ADR-001` | `design/decisions/` | proposed (new), accepted, rejected (closed), superseded (closed) | — |
| milestone (Milestone) | `MS-001` | `design/milestones/` | planned (new), active, done (done), dropped (closed) | — |
| task (Task) | `TASK-001` | `design/tasks/` | todo (new), doing, blocked, done (done), dropped (closed) | task |
| test (Test) | `TEST-001` | `design/tests/` | draft (new), ready, passing, failing, obsolete (closed) | — |
| risk (Risk) | `RISK-001` | `design/risks/` | open (new), mitigated, accepted, closed (closed) | — |

`kiln new` starts the body of requirement from `design/templates/requirement.md`, design from `design/templates/design.md`, decision from `design/templates/decision.md`, milestone from `design/templates/milestone.md`, task from `design/templates/task.md`, test from `design/templates/test.md`, risk from `design/templates/risk.md`.

"new" is the status `kiln new` writes; "done" counts toward progress; "closed" items are left out of coverage and progress (prefer closing an item to deleting its file).

### Links

Written in the source item's front matter, `[links]` table, as `role = ["ID", …]`; the inverse is computed, never written.

| Role | From | To | Read from the target as |
|---|---|---|---|
| `implements` | design, task, code (a comment) | requirement | implemented by |
| `verifies` | test, code (a comment) | requirement | verified by |
| `part-of` | task | milestone | includes |
| `supersedes` | decision | decision | superseded by |
| `mitigates` | requirement, design, task | risk | mitigated by |
| `depends-on` | task, milestone | task, milestone | blocks |

No source code is scanned for references (`[code] paths` in kiln.toml).

### Coverage

A requirement past its first status is expected to be covered by `implements` (to requirement), `verifies` (to requirement), each where its `to` allows: `kiln check` warns about the gaps (K201, K202), and `kiln trace` lists them.

### Planning

Work is a task `part-of` a milestone, ordered by `depends-on`. `kiln next` lists the work ready to start: not done or closed, not `blocked`, and nothing it `depends-on` still open, earliest milestone first; `kiln status` shows the plan by milestone, with what waits on what. Claim work by setting its `owner` when you take it; `kiln next --unowned` leaves claimed work out.

Finished work has a done status (task done); `kiln check` warns when the plan contradicts itself: a done milestone with open work (K211), done work whose dependencies aren't (K212), open work in no milestone (K213), a requirement implemented or verified while work implementing it is open (K214).

### Exports

Written by `kiln export NAME`; never edit what they write. `kiln export --check` fails when a committed one is out of date.

| Export | Format | Writes | Template |
|---|---|---|---|
| `design-doc` | markdown | `docs/DESIGN-DOCUMENT.md` | `design/templates/design-doc.md` |
<!-- kiln:agents end -->
