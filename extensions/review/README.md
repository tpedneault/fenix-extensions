# Review

Code review on GitHub and GitLab, without leaving Fenix: the pull and
merge requests waiting, one under review with its files, threads, checks
and the comments you're holding for it, sent with a verdict -- and a new
pull request for the branch, prefilled from its commits.

It's the forge for Fenix's Git pages too: the Git status page's header
says how the branch's request stands (its checks, approvals and open
threads), and `o` there reviews it, or opens a new one.

| Key | What it does |
| --- | --- |
| `SPC g M` | The reviews: open requests, yours, those waiting on you |
| `SPC g P` | A pull request for the branch |

## The reviews

| Key | What it does |
| --- | --- |
| `Enter` | Review it |
| `w`, `c` | Review it in a worktree of its own beside the repository; check it out here (`mr-N`) |
| `Tab` | Which requests: open, yours, waiting on you |
| `o`, `u`, `q` | The browser, read again, close |

## A review

| Key | What it does |
| --- | --- |
| `Tab`, `v` | Open a file's diff; mark it viewed |
| `C`, `S` | Comment on the line or the lines marked, suggest a change -- held until you submit |
| `r`, `R` | Reply to a thread, resolve it |
| `]t` | The next thread |
| `i` | Only what changed since your last review |
| `s` | Submit: `a` approve, `r` request changes, `c` comment, `e` a summary, `S S` sends it |
| `m` | Merge: squash, rebase, delete the branch, when the checks pass; `m m` |
| `w`, `o`, `q` | A worktree, the browser, close |

A check's log opens as text; a failed one can run again. Your held
comments and what you've seen are kept in the extension's data folder, so
they wait for you across restarts.

## A new request

The branch's commits against its base, whether it's pushed and merges
cleanly, and the title, description (the repository's template, or the
commits), reviewers and draft: `Enter` edits a field, `e` the
description, `S S` pushes the branch if it has to and opens it. A branch
named after a Jira issue puts the key in the title.

## Settings

Fenix's own, under Git and Forges (`SPC ,`):

| Setting | What it does |
| --- | --- |
| `git.base_branch` | The branch requests go into (else the remote's default, main or master) |
| `git.reviewers` | Who's asked to review a new request |
| `gitlab.base_url`, `gitlab.token` | Your GitLab server, and a personal access token with the `api` scope |
| `github.token` | GitHub's token, when the GitHub CLI isn't signed in (`gh auth login`) |

A GitHub `origin` is GitHub's; any other is taken to be your GitLab.
