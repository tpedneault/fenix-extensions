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
they're installed. Where issues from two are listed together, each says
whose it is; words that said "Jira" name the task's tracker. A tracker
built elsewhere is found once its name is in `agenda.trackers`.

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
services under its own name, `<name>.tracker.about`, `.assigned`,
`.issues`, `.transitions`, `.transition`, and the writes it chooses to
take. The contract -- every call, field, limit and error, how the agenda
finds trackers, and a checklist -- is
[`plans/tracker-seam.md`](../../plans/tracker-seam.md). The shapes, the
cleaning of untrusted text and a fake tracker are the shared crate
[`crates/tracker`](../../crates/tracker) (`fenix-tracker`); Jira's is
`crates/jira`'s `tracker` module.

To see the agenda drive yours, copy `src/tests.rs`: `world` starts the
agenda on the test kit's fake host with trackers wired to
`<name>.tracker.*`, and its tests run `FakeTracker` through import,
paging, moves, errors and two trackers at once.

## Building

```text
cargo xtask pack extensions/agenda dist
cargo test --manifest-path extensions/agenda/Cargo.toml
```

The tests cover the model (sync, conflicts, worklogs, `agenda.json` from
1.2), the page as text, and the agenda on the test kit with fake
trackers, the Jira extension 1.3's seam on recorded Jira responses, and
the Jira extension 1.2's old services.
