# Linear

Your [Linear](https://linear.app) issues in Fenix: a page with your issues, a
team's active cycle and backlog and a typed filter; an issue with its
comments; a new-issue form; a branch started from an issue -- and Linear for
[the agenda](../agenda) and for coding agents.

## Getting started

1. In Linear: **Settings, Security & access, Personal API keys**, make a key.
   Name it for Fenix. (A personal key can do what you can do: it sees the
   workspaces and teams you're in, and can read, create and change issues
   and comments as you. It can't be narrowed. Revoke it there any time.)
2. In Fenix, on the settings page (`SPC ,`, or `Ctrl+,` with standard keys),
   under Linear, set **API key**. It's kept in the system's keychain, never
   in `settings.toml` (where there's no keychain, Fenix says so, and it is
   there as plain text). `FENIX_LINEAR_TOKEN` in the environment wins.
3. `SPC o w w` opens the page.

**OAuth isn't supported.** Linear's OAuth needs a client registered with its
own secret, which an extension can't keep: a personal API key is the way in,
and it's sent as Linear asks for one (`Authorization: <key>`).

## The page

Views on the left: **My issues** and **Created by me** (not finished), each
team's **Active cycle** and **Backlog** (set `linear.teams` to list only
some), and **Filter**, the typed one. Moving past the last issue asks Linear
for the next page.

An issue has the same keys on the list and on its own page, the ones the Jira
page and the agenda's tasks have where the action is the same:

| Key | Does |
|---|---|
| `j` `k` `g` `G` | move |
| `l`, `Enter` | open the issue (`Enter`: its task, when it's in your agenda) |
| `h`, `Esc` | back |
| `s` | move it to another state: one row for each of its team's workflow states, in the order of Linear's board (triage, backlog, unstarted, started, completed, canceled) |
| `p` | priority: Urgent, High, Medium, Low, No priority |
| `A` | assign it to a member of its team, or no one |
| `u` | due date, `YYYY-MM-DD` (empty clears it) |
| `e`, `E` | title, description (in Fenix's pane) |
| `C` | comment, in Markdown |
| `b` | a branch for it (below) |
| `y`, `Y`, `B` | copy its address, its identifier (`ENG-123`), its branch name |
| `o` | open it in the browser |
| `a`, `t` | add it to the agenda; and start its clock |
| `n` | a new issue: team, title, description, priority, state, assignee, labels; `S` creates it |
| `/` | a typed filter |
| `R`, `?`, `q` | refresh, every key, close |

**The typed filter**: `team:ENG state:started assignee:me label:bug login`.
`state` takes a state's name (`"In Review"`) or a type (`started`);
`assignee` and `creator` take `me`, `none` (assignee) or part of a name;
`priority` takes `urgent`, `high`, `medium`, `low`, `none`; `project` part of
a name; `is:open` (the default), `is:closed`, `is:all`. Other words are part
of the title, and `ENG-12` is that issue. Everything you type is sent to
Linear as data, never as part of a query.

Anything that changes Linear is a deliberate act: a state, priority or person
picked from a menu, a title or date entered, a comment or description sent
from its pane, a form submitted with `S`.

## A branch from an issue

`b` makes a branch for the issue in the project you're in and switches to it,
running `git` there (with the permission below; Fenix's own API has no branch
call). Its name is the one **Linear suggests** for the issue (`branchName`,
such as `jo/eng-12-fix-the-login-timeout`), or your own, with
`linear.branch_pattern`: `{id}` is `eng-12`, `{ID}` `ENG-12`, `{team}` `eng`,
`{number}` `12`, `{slug}` the title as a slug (`feature/{id}-{slug}`).

- The branch is made from where you are (`HEAD`); nothing is fetched.
- **A tree with changes is never touched silently**: it asks first, saying how
  many files changed, and only "yes" goes on. Making a branch changes no file,
  so the changes come with you.
- A branch that **exists** is switched to, not made again and not reset -- and
  not while the tree has changes: commit or stash first.
- Already on it: nothing happens.
- Not a git repository (or no project open): nothing is run; it says so and
  copies the name.
- The name is checked before `git` sees it: nothing starting with `-`, no
  spaces, control characters or `..`; then `git check-ref-format` has the
  last word.

## The agenda

[The agenda](../agenda) finds Linear and can import your assigned issues, link
a task to an issue (`ENG-123` or its address), keep it up to date, move it
between Linear's workflow states (one move for each state of its team; Linear
has no Blocked state, so Blocked stays in the agenda), set its priority
(Linear's 0 to 4 are Urgent, High, Medium, Low, and no priority), due date and
assignee, comment, and open the new-issue form from a task. Bodies are
Markdown, as Linear has them.

## Coding agents

Two read-only tools (class `read`): `linear_read_issue` (an issue by
identifier or address, with its latest comments) and `linear_list_mine` (your
open issues, titles and states). Issue text is written by other people, so it
is answered as data, with a note saying so, cleaned of control and bidirectional
characters, and capped (description 16 KiB, the latest 30 comments of 2 KiB,
the whole at 48 KiB). Nothing creates, comments or changes a state.

## Settings

| Key | |
|---|---|
| `linear.token` | your API key (a secret) |
| `linear.teams` | team keys the page lists a cycle and backlog for; empty is all |
| `linear.branch_pattern` | how a branch is named; empty is Linear's suggestion |
| `linear.sync_minutes` | how often linked agenda tasks are brought up to date |

## What it may do

- **Network**: `api.linear.app`, Linear's GraphQL API, and nothing else.
- **Secrets**: `linear.token`, your API key.
- **Programs**: `git`, in the project: `rev-parse`, `status`,
  `check-ref-format`, `symbolic-ref`, `show-ref` to look, `switch` and
  `switch -c` to change branch.
- **Files**: the project's folder, read, as the place `git` runs. It reads no
  file itself.

## Building

```text
cargo test --locked
cargo xtask pack extensions/linear dist
```

The tests run on [`crates/linear`](../../crates/linear) (the client and the
seam, on recorded transports) and on the test kit (the page, the key, the
services).

**The fixtures are not recordings.** No Linear workspace was available:
`crates/linear/tests/fixtures` are written from Linear's GraphQL schema and
documentation, and every query is held to the schema in a test (see
`crates/linear/tests/schema/README.md`).
