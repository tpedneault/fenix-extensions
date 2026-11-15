# Agenda

Your tasks on a board, a clock for your time, and a week's timesheet --
each task linked to an issue when a tracker extension is there: Jira
today, and GitHub Issues, GitLab Issues and Linear as they come. Its
keys are under `SPC A`, and your tasks are kept in `agenda.json`.

`SPC A a` opens it where it was left; `SPC A b`, `SPC A l` and `SPC A r`
open the board, the list and this week's time. `SPC A n` adds a task,
`SPC A h` adds one about the selection or line you're on, and `SPC A t`
stops the clock, resumes it or switches it.

With standard keys they're `Ctrl+K A a` and so on. What it says names
the keys as the key scheme in use has them, and yours when you've bound
your own -- on the page, on Home's today and in its messages: "`C-k A n`
to add one" with standard keys, "`SPC A n`" with Vim keys. It needs
Fenix 1.5 or later.

## The page

| Key | What it does |
| --- | --- |
| `1`-`4`, `Tab` | Today, Board, List, Time |
| `j` `k` `g` `G` | Move; `h` `l` across the board, `H` `L` move a card |
| `Enter`, `Esc` | Open the task, back |
| `n` | A new task |
| `s` `p` `c` `u` | Status, priority, category, due date |
| `e` `E` | Title, description |
| `t` `T` | Start or stop its clock, log time |
| `N` `C` | A private note, a comment on its issue |
| `I` `A` | Link it to an issue, assign it |
| `x` `D` | Archive, delete |
| `/` `f` `P` | Search, filters, this project only |
| `?` | Every key |

## With Jira, or another tracker

With the Jira extension installed and set up, a linked task's edits go to
Jira, and Jira's come back every `jira.sync_minutes`:

- `SPC A i` imports issues assigned to you
- `SPC A s` syncs now
- `SPC A w` reviews the week's unsent time and sends it as worklogs

A sync you asked for ends in a notification: how many issues it brought
up to date, or why it failed. One that finds a conflict says so, even a
timed one; Enter on it opens the agenda. Each replaces the last.

Moving a linked card takes the one step in the issue's workflow that gets
there; when there are several it asks, and Esc puts the card back. What
Blocked means in a Jira project -- a status, the Flagged field, or the
agenda only -- is asked the first time and remembered.

Any tracker extension that serves the agenda works the same way, and
several at once: GitHub Issues, GitLab Issues and Linear are found when
they're installed. Each is synced on its own schedule, and each has its
own queue of changes: one that's slow to answer holds up only its own.
Changes go to an issue by its id, so one that moved to another project
since the last sync still gets them. Where issues from two are listed
together, each says whose it is; words that said "Jira" name the task's
tracker. A tracker built elsewhere is found once its name is in
`agenda.trackers`.

When a change doesn't go through:

- **No answer came** (a slow tracker): it may have been made, so it isn't
  sent again by itself. The task says so, and `r` asks whether to send it
  again -- with the same `op`, so a tracker that took it doesn't make it
  twice -- or to forget it. A worklog with no answer is marked "may have
  been logged" in the review: `W` leaves it out, `s` sends it again (the
  same `op`) and `x` says it was logged.
- **Refused** (signed out, a value it didn't accept): the task says why,
  and `r` sends it again once you've put it right.
- **Never taken** (the tracker has no such field): it's taken off the
  queue, so the task's other changes go, and the task is marked "not
  taken" until you choose with `r`: send it again, take the tracker's
  value back, or keep yours here only.

What a tracker sends is kept as it sent it, byte for byte, so what goes
back is the tracker's own text; it's shown cleaned -- control
characters, bidirectional overrides and invisible characters removed,
the joiners and marks living scripts and emoji need kept. A title or
description too long to keep whole isn't written back.

Links to trackers other than Jira are kept in `agenda.json` under
`"link"`, which agenda 1.2 drops when it saves the file; they're also
kept beside it, in `agenda-links.json`, which 1.2 never touches, and
put back when this agenda reads the file again. A change left waiting
for a task whose link was lost is dropped, with a note on the task.

## Settings

| Setting | What it does |
| --- | --- |
| `agenda.categories` | Categories offered when you file a task |
| `agenda.worklog_round` | Time sent as worklogs is rounded to this many minutes |
| `agenda.idle_minutes` | With the clock running, how long without a key before it asks what time to keep |
| `agenda.trackers` | Tracker extensions built elsewhere to ask for issues, by the name their services start with |

## Outside Fenix

The control channel's `fenix/agenda.tasks`, `fenix/agenda.clockIn` and
`fenix/agenda.clockOut`, and the clock in `fenix/state`, are answered by
this extension.

## What it may do

Read where you are in the editor (`editor = "read"`), for the file and
line `SPC A h` makes a task about. Nothing else: it holds no token and
reaches no server -- each tracker extension holds its own sign-in and
talks to its own server, and the agenda only asks it.

## Writing a tracker

A tracker extension serves the agenda the **tracker seam**: a few JSON
services under its own full id, `<id>.tracker.about`, `.assigned`,
`.issues`, `.transitions`, `.transition` (`fenix.jira.tracker.about`),
and the writes it chooses to take. The contract -- every call, field,
limit and error, how the agenda finds trackers, the gate on writes, and
a checklist -- is [`plans/tracker-seam.md`](../../plans/tracker-seam.md).
The shapes, the cleaning of untrusted text, the gate and a fake tracker
are the shared crate [`crates/tracker`](../../crates/tracker)
(`fenix-tracker`); Jira's is `crates/jira`'s `tracker` module.

To see the agenda drive yours, from your own crate:

```toml
[dev-dependencies]
fenix-agenda = { path = "../agenda", features = ["testing"] }
```

`fenix_agenda::testing::world_with` starts the agenda on the test kit's
fake host with the services you wire -- `serve` puts a `FakeTracker`
behind `<id>.tracker.*`; wire your own answers the same way -- and
`World` adds issues, moves cards and tells you what the agenda said.
The agenda's own `src/tests.rs` is a full set of examples.

## Building

```text
cargo xtask pack extensions/agenda dist
cargo test --manifest-path extensions/agenda/Cargo.toml
```

The tests cover the model (sync, conflicts, worklogs, `agenda.json` from
1.2), the page as text, and the agenda on the test kit with fake
trackers, the Jira extension 1.3's seam on recorded Jira responses, and
the Jira extension 1.2's old services.
