# GitHub Issues

GitHub's issues in Fenix: your searches, an issue with its comments, a
new issue from the repository's templates, a branch started from an
issue -- and, for the agenda, GitHub as a tracker its tasks link to.

**If you already use Review, there's nothing to set up**: it signs in
the same way, with the same token.

## The page

`SPC g i` (`Ctrl+K g i` with standard keys) opens it. Its searches are
across the top: assigned to you, created by you, mentioning you, the
repository you're in (from its remote), the repositories you list, your
saved searches, and one you type in GitHub's own syntax (`/`). Pull
requests are never among them.

| Key | What it does |
| --- | --- |
| `Tab`, `1`-`9` | The next search, or one by number |
| `/` | A search typed in GitHub's syntax (`label:bug repo:acme/rocket`); `is:issue` is added |
| `Enter` | Open the issue: its fields, linked pull requests, description and comments |
| `C` | Comment, in a pane of its own |
| `e`, `E` | Its title, its description |
| `l`, `A` | Its labels, its assignees (commas between) |
| `s` | Move it: close as completed, close as not planned, reopen, the blocked label on or off |
| `b` | Start a branch for it |
| `a` | Add it to the agenda |
| `o`, `y`, `Y` | The browser; copy its ref (`owner/repo#12`), its address |
| `n` | A new issue |
| `u`, `q` | Read again; back, or close |

Nothing changes on GitHub from one key: a comment or an edit is sent
from its pane or prompt, a move is picked from the list `s` shows, and
a new issue goes with `S S`.

**A new issue** (`n`): its repository (the one you're in), title,
description, labels, assignees, and a template -- the repository's
Markdown issue templates (`.github/ISSUE_TEMPLATE/*.md`; its YAML issue
forms aren't read). `Enter` edits a field, `e` the description, `t` a
template, `S S` sends it, `q` puts it away.

**A branch from an issue** (`b`): its name from the issue, `12-short-title`
(the pattern is a setting), proposed in a prompt you can change.
It's made only when the project you're in has a remote that is the
issue's repository on the same GitHub: otherwise you're told which
repository the project is, and nothing is made. It starts from the
remote's default branch (fetched first) when the remote says which,
else from what's checked out. Changes you haven't committed are named in
the prompt before you press Enter (they come along, as `git switch`
carries them; git refuses when they'd be overwritten). A branch of that
name already there is switched to.

## For the agenda

The agenda finds it by itself (`github-issues.tracker.*`, the tracker
seam): your open issues to import, linked tasks kept in step, and their
moves -- close as completed, close as not planned, reopen. An open issue
is To do, or Doing with the in-progress label; the blocked label is the
Blocked column, and without one Blocked stays in your agenda. Title,
description, assignee and comments go to GitHub; GitHub has no priority
or due date, so those stay on the task. A task's new issue opens this
page's form, filled in.

## For agents

Two tools that only read: `read_issue` (an issue with its fields,
description, linked pull requests and most recent comments, capped) and
`my_issues` (your open issues). What they answer is said to be other
people's words, cleaned and capped; nothing they can do writes.

## Settings

Fenix's own, under Forges (`SPC ,`, `Ctrl+,` with standard keys):
`github.token`, when the GitHub CLI isn't signed in (`gh auth login`).

| Setting | What it does |
| --- | --- |
| `github-issues.server` | Your GitHub Enterprise Server's address; empty for github.com |
| `github-issues.repositories` | Repositories whose open issues the page lists |
| `github-issues.queries` | Saved searches, by name |
| `github-issues.branch_pattern` | A branch's name: `{number}`, `{title}`, `{project}` (`{number}-{title}`) |
| `github-issues.blocked_label` | The label that means blocked (`blocked`) |
| `github-issues.doing_label` | The label that means under way (`in progress`) |
| `github-issues.sync_minutes` | How often the agenda's linked tasks are brought up to date |

## What it may do

- Reach GitHub's API, `api.github.com`, and your Enterprise Server when
  you set one -- nothing else.
- Read Fenix's GitHub token, `github.token`, the one Review reads. It
  never writes or copies it: the token goes to GitHub's API in a
  header, and nowhere else -- not to the agenda, an agent, a
  notification or a log.
- Run `git` in the project you're in (its remotes and status, and the
  branch you start from an issue), and `gh auth token` for the GitHub
  CLI's token.
- Open the project for `git` to run in; it reads and writes nothing
  there itself.

It serves no service that sends GitHub a request of another extension's
choosing: the agenda's calls are the tracker seam's, each a fixed thing
to do with an issue.

## Building

```text
cargo xtask pack extensions/github-issues dist
cargo test --manifest-path extensions/github-issues/Cargo.toml
cargo test --manifest-path crates/forge/Cargo.toml
cargo test --manifest-path crates/issues/Cargo.toml
```

GitHub's side -- the client, the seam, errors and paging -- is tested in
`crates/forge` on responses written from GitHub's REST API description
(not recordings); the page, branch and agent answers in `crates/issues`;
here, the extension on the test kit's fake host.
