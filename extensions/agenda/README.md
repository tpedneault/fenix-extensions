# Agenda

Your tasks on a board, a clock for your time, and a week's timesheet --
each task linked to a Jira issue when the Jira extension is there. Its
keys are under `SPC A`, and your tasks are kept in `agenda.json`.

`SPC A a` opens it where it was left; `SPC A b`, `SPC A l` and `SPC A r`
open the board, the list and this week's time. `SPC A n` adds a task,
`SPC A h` adds one about the selection or line you're on, and `SPC A t`
stops the clock, resumes it or switches it.

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
| `N` `C` | A private note, a Jira comment |
| `I` `A` | Link it to Jira, assign it |
| `x` `D` | Archive, delete |
| `/` `f` `P` | Search, filters, this project only |
| `?` | Every key |

## With Jira

With the Jira extension installed and set up, a linked task's edits go to
Jira, and Jira's come back every `jira.sync_minutes`:

- `SPC A i` imports issues assigned to you
- `SPC A s` syncs now
- `SPC A w` reviews the week's unsent time and sends it as worklogs

## Settings

| Setting | What it does |
| --- | --- |
| `agenda.categories` | Categories offered when you file a task |
| `agenda.worklog_round` | Time sent to Jira is rounded to this many minutes |
| `agenda.idle_minutes` | With the clock running, how long without a key before it asks what time to keep |

## Outside Fenix

The control channel's `fenix/agenda.tasks`, `fenix/agenda.clockIn` and
`fenix/agenda.clockOut`, and the clock in `fenix/state`, are answered by
this extension.
