# GitLab Issues

GitLab's issues in Fenix, on gitlab.com or your own instance: your
searches, an issue with its comments, a new issue from the project's
templates, a branch started from an issue -- and, for the agenda, GitLab
as a tracker its tasks link to. Projects in nested groups
(`group/sub/project`) are as any other.

**If you already use Review, there's nothing to set up**: it reads the
same GitLab server and token.

## The page

`SPC g I` (`Ctrl+K g I` with standard keys) opens it. Its searches are
across the top: assigned to you, created by you, mentioning you (your
pending to-do items that mention you), the project you're in (from its
remote), the projects you list, your saved searches, and a filter you
type (`/`) in GitLab's filter words: `label:bug`, `assignee:@me`,
`author:jo`, `milestone:v2`, `project:group/sub/project`, `is:closed`,
and words searched for in titles and descriptions.

| Key | What it does |
| --- | --- |
| `Tab`, `1`-`9` | The next search, or one by number |
| `/` | A filter in GitLab's words |
| `Enter` | Open the issue: its fields, due date, weight, time tracked, related merge requests, description and comments (GitLab's own notes muted) |
| `C` | Comment, in a pane of its own |
| `e`, `E` | Its title, its description |
| `l`, `A` | Its labels, its assignees (usernames, commas between) |
| `s` | Move it: close, reopen, a step of the project's `workflow::` labels |
| `b` | Start a branch for it |
| `a` | Add it to the agenda |
| `o`, `y`, `Y` | The browser; copy its ref (`group/project#12`), its address |
| `n` | A new issue |
| `u`, `q` | Read again; back, or close |

Nothing changes on GitLab from one key: a comment or an edit is sent
from its pane or prompt, a move is picked from the list `s` shows, and
a new issue goes with `S S`.

**A new issue** (`n`): its project (the one you're in), title,
description, labels, assignees, and one of the project's issue
templates. `Enter` edits a field, `e` the description, `t` a template,
`S S` sends it, `q` puts it away.

**A branch from an issue** (`b`): its name from the issue,
`12-short-title` (the pattern is a setting), proposed in a prompt you
can change. It's made only when the project you're in has a remote whose
path is the issue's project -- by any host name, as GitLab is often
cloned through an SSH alias or another name; otherwise you're told which
project this is, and nothing is made. It starts from the remote's
default branch (fetched first) when the remote says which, else from
what's checked out. Changes you haven't committed are named in the
prompt before you press Enter. GitLab's own "create a branch or merge
request from this issue" isn't used: the branch is made here.

Nothing that came from a remote reaches git as an option: a remote's
name, or the default branch its server named, is used only when it's a
name git takes as a branch (never one starting with `-`), and goes after
`--end-of-options`; the fetch is from the issue's own remote alone, asks
for `refs/heads/<branch>`, and may use only https, http, ssh and git
(`GIT_ALLOW_PROTOCOL`) -- never `file://` or `ext::`. A remote that
isn't on the network (a path, `file://`) is never the issue's
repository.

## For the agenda

The agenda finds it by itself (`fenix.gitlab-issues.tracker.*`, the
tracker seam): your open issues to import, linked tasks kept in step,
and their moves -- close, reopen, and a step of the `workflow::` scoped
labels when the project uses them (how far along each step is, To do,
Doing, Blocked or Done, is read from its name: `workflow::Blocked` is
the Blocked column). A column no step reaches moves in your agenda only,
and nothing is sent. Title, description, due date, assignee, labels and
comments go to GitLab; with `gitlab-issues.time_tracking` on, the time
you log goes to the issue's time tracking on the day you spent it
(`/spend`). GitLab has no priority, so that stays on the task. The
agenda's "this project only" knows your issues by the project the
project's remote is.

**Changes from the agenda are asked about.** Any installed extension can
call the tracker seam, and Fenix doesn't say which one did, so the first
change that comes through it in a session -- a move, an edit, a comment,
a new issue -- asks you first: *Agenda wants to change issues in GitLab
for you -- allow for this session?* `gitlab-issues.seam_writes` says it for
good: `ask` (the default), `always` or `never`. What you do on this
extension's own page is never asked about. A change sent twice (the
agenda gave up waiting, and you said send it again) is done once.

## For agents

Two tools that only read: `read_issue` (an issue with its fields,
description, related merge requests and most recent comments, capped)
and `my_issues` (your open issues). What they answer is said to be
other people's words, cleaned and capped; nothing they can do writes.

## Settings

Fenix's own, under Forges (`SPC ,`, `Ctrl+,` with standard keys):
`gitlab.base_url`, your GitLab's address, and `gitlab.token`, a personal
access token with the `api` scope (`read_api` is enough for reading).

| Setting | What it does |
| --- | --- |
| `gitlab-issues.projects` | Projects whose open issues the page lists |
| `gitlab-issues.queries` | Saved filters, by name |
| `gitlab-issues.branch_pattern` | A branch's name: `{number}`, `{title}`, `{project}` (`{number}-{title}`) |
| `gitlab-issues.seam_writes` | Changes from the agenda: `ask` once a session, `always`, `never` |
| `gitlab-issues.workflow_scope` | The scoped labels that are the workflow (`workflow`, for `workflow::Doing`) |
| `gitlab-issues.time_tracking` | The agenda's logged time goes to the issue (off) |
| `gitlab-issues.sync_minutes` | How often the agenda's linked tasks are brought up to date |

## What it may do

- Reach your GitLab, the server in `gitlab.base_url`, and nothing else.
- Read Fenix's GitLab settings (`gitlab.*`) and token, `gitlab.token`,
  the ones Review reads. It never writes or copies the token: it goes
  to your GitLab's API in a header, and nowhere else -- not to the
  agenda, an agent, a notification or a log.
- Run `git` in the project you're in: its remotes and status, and the
  branch you start from an issue.
- Open the project for `git` to run in; it reads and writes nothing
  there itself.

It serves no service that sends GitLab a request of another extension's
choosing: the agenda's calls are the tracker seam's, each a fixed thing
to do with an issue.

## Building

```text
cargo xtask pack extensions/gitlab-issues dist
cargo test --manifest-path extensions/gitlab-issues/Cargo.toml
cargo test --manifest-path crates/forge/Cargo.toml
cargo test --manifest-path crates/issues/Cargo.toml
```

GitLab's side -- the client, the seam, errors and paging -- is tested in
`crates/forge` on responses written from GitLab's API documentation (not
recordings); the page, branch and agent answers in `crates/issues`;
here, the extension on the test kit's fake host.
