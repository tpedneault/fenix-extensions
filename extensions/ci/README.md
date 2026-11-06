# CI status

Your branch's CI, without leaving Fenix: GitHub Actions or GitLab
pipelines on the modeline, a page of the branch's runs with their jobs,
steps and logs, a notification when a run finishes -- and the failing
jobs' logs for your coding agent, so it can fix them.

| Modeline | What it means |
| --- | --- |
| `CI ✓` | The runs for `HEAD` passed |
| `CI ● 3/7` | Running: 3 of 7 jobs done (`✗1` when one has failed already) |
| `CI ○ queued` | Waiting for a runner |
| `CI ✗ 2 failed` | Finished, two jobs failed |
| `CI ⊘ cancelled` | Cancelled |
| `CI ✓ (prev)` | `HEAD` has no runs (not pushed yet): the newest commit's that has |
| `CI –` | The branch has no runs |
| `… ?` | The last look failed: the page says why |

| Key | What it does |
| --- | --- |
| `SPC g C` | The CI page |
| `SPC g F` | The log of the job that failed |

## The page

The branch's latest runs (GitHub's workflow runs, GitLab's pipelines),
newest first, the first opened on its jobs. A job opens on its steps
(GitHub's), each with its state and how long it took.

| Key | What it does |
| --- | --- |
| `j`, `k`, `g`, `G` | Move |
| `Enter` | A run: its jobs. A job or a step: its log, read-only, highlighted as a log |
| `l`, `h` | Open a run or a job, close it |
| `r` | A run: re-run its failed jobs (GitLab: retry the pipeline). A job: re-run it |
| `c` | Cancel the run (GitLab: the job), asked first |
| `o` | The run or the job in the browser |
| `R` | Ask again |

A log is its end when it's big (the last 512 KiB): the colours, GitLab's
section markers and GitHub's timestamps taken off.

## When it asks

It looks at the repository every 3 seconds (Fenix's `git_state`). A new
branch, a new `HEAD` or a push asks the forge at once, and after a push
again every few seconds until its runs show up. Otherwise it asks every
30 seconds while something runs and every 2 minutes when nothing does;
failures back off, doubling up to 15 minutes. Every listing is asked with
the `ETag` it last came with, so an answer with nothing new is a `304`
that costs GitHub's rate limit nothing; a rate limit's reset
(`x-ratelimit-*`, `ratelimit-*`, `retry-after`) holds it back until it
lifts.

## Notifications

When the runs for your branch finish -- having been seen running -- a
notification says so, tagged by the branch so the next replaces it: a
pass quietly, a failure loudly (`critical`, or `normal`), with **Open the
log** (the failed job's) and **Open in the browser**.

## For coding agents

Its tool `ci_failures` gives an agent the branch's status, the failing
jobs, the steps that failed, the lines in their logs that say what went
wrong, and the end of each log (`lines`, 80 by default). It reads only;
it's offered by default.

## Where the CI is

The remote, `origin` (else the first), from `.git/config` (or `git
remote get-url origin` in a worktree): `github.com` is GitHub, `gitlab.com`
GitLab, the addresses in the settings theirs, and anything else a GitLab
at the remote's host, as the Review extension takes it.

The tokens are Fenix's own (`SPC ,`, Forges): `github.token`, else the
GitHub CLI's (`gh auth token`, `--hostname` for an Enterprise Server);
`gitlab.token` (the `read_api` scope to read, `api` to re-run and cancel).
A public repository on GitHub needs none.

## Settings

| Key | What it does |
| --- | --- |
| `ci.provider` | `github` or `gitlab`, or empty to tell by the remote |
| `ci.github_url` | Your GitHub Enterprise Server (`https://github.example.com`) |
| `ci.gitlab_url` | Your GitLab; empty uses Fenix's GitLab server (`gitlab.base_url`) |
| `ci.poll_running`, `ci.poll_idle` | Seconds between looks while something runs (30), when nothing does (120) |
| `ci.notify_success`, `ci.notify_failure` | Whether to notify of a pass, a failure |
| `ci.failure_urgency` | `critical` or `normal` |

An Enterprise Server's logs come from wherever it keeps them: when that's
another host than its own, the log can't be read (the browser can).
