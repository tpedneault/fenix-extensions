# The tracker seam: the agenda's issues from any tracker

> Milestone 6.2, written on 14 November 2026 before it was built, for the
> agents who write the trackers after it (Jira Cloud 6.3; GitHub Issues,
> GitLab Issues and Linear 6.4). It's the contract between the agenda
> and an extension that knows a tracker: what the agenda asks, what the
> tracker answers, in JSON, with every field. Jira Server and Data
> Center move onto it first, with nothing changing for the people using
> them. Every claim about Fenix cites its source at `/home/user/fenix`
> (API 1.5, `fenix-extension-api` tag `v1.5.0`); nothing here was run
> in a Fenix window or against a Jira server.

## 1. What it's for, and what it isn't

Until now the agenda knew Jira: it held a Jira client, sent raw REST
requests through the Jira extension (`jira.request`), read Jira's
settings (`jira.config`), worked out what "Blocked" means in a Jira
project, guessed Jira priority names and wrote "Jira" into its pages.
The seam replaces all of that with a few calls any tracker extension
can answer, so the agenda knows *a tracker*, and each tracker keeps its
own quirks.

The agenda asks a tracker for:

1. **the issues assigned to me** -- to import, or to link a task to;
2. **these issues** -- by key or URL, to keep linked tasks in step;
3. **what can I do next** with an issue, and **doing one** -- "move it
   on", as a list the tracker supplies (Jira's workflow transitions,
   GitHub's close-as-completed / close-as-not-planned / reopen, GitLab's
   close and reopen, Linear's workflow states), never a fixed enum.

and, when the tracker offers them, a few writes the agenda already
makes of Jira today (§4.6): a field changed, a comment posted, time
logged, the tracker's own new-issue form opened from a task.

**Left out, on purpose.** Each tracker's own page serves these, not the
seam: searches and saved queries; an issue's comment *thread*, edited or
deleted; the new-issue *form* itself (the seam only opens it, filled in);
attachments, links between issues, sprints, epics, milestones, watchers,
reactions; starting a branch from an issue; agent tools. They differ too
much between trackers for one shape to be worth it, and the agenda needs
none of them. A tracker can add them to its own page and agent tools
without the seam moving.

## 2. Names: a tracker serves `<id>.tracker.<call>`

Fenix routes a service by its exact name to the first installed
extension whose manifest lists it, other than the caller
(`fenix-gui/src/app/wasm_host.rs`, `code_server`); a name nobody lists
is answered `Err("nothing offers <name>")` (same file, `Request::Service`).
A service must start with its extension's own name or a namespace it
declares (`fenix-extensions/src/manifest.rs`, the `ours` check). There
is no way to list who serves a name, and two extensions listing the
same name means one of them is never asked.

So a tracker serves its calls under **its own name**: the extension
`fenix.jira` serves `jira.tracker.about`, `jira.tracker.issues`, and so
on. `<id>` is the part of its id after `fenix.` for an extension in this
repository, or its whole id for one built elsewhere (`acme.youtrack`
serves `acme.youtrack.tracker.about`, which Fenix allows: a service may
start with the extension's full id). No namespace is needed, and two
trackers never collide.

Ids already taken, for the trackers that follow:

| Tracker | Extension | `<id>` |
|---|---|---|
| Jira Server, Data Center, and Cloud (6.3) | `fenix.jira` | `jira` |
| GitHub Issues (6.4) | `fenix.github-issues` | `github-issues` |
| GitLab Issues (6.4) | `fenix.gitlab-issues` | `gitlab-issues` |
| Linear (6.4) | `fenix.linear` | `linear` |

### Discovery, as Fenix 1.5 allows it

The agenda can't enumerate, so it asks by name. It asks
`<id>.tracker.about` of every id it knows: the four above, built into
the agenda, then each id in its setting `agenda.trackers` (a list, for
trackers built elsewhere). A tracker that isn't installed answers at
once with Fenix's `nothing offers`, which costs nothing. It asks:

- when it starts (it starts with Fenix, as it always has);
- when you sync (`SPC A s`) or import (`SPC A i`), and when its page
  opens, if it last asked more than five minutes ago -- so a tracker
  installed while Fenix runs is found without a restart;
- when `agenda.trackers` changes, and with each timed sync once five
  minutes have passed;
- and a tracker no longer installed is gone from what it found the next
  time it asks.

A tracker added to this repository is added to the agenda's list in
the same pull request (`KNOWN` in `extensions/agenda/src/trackers.rs`),
in a minor release of the agenda.

**What Fenix would need for real discovery** (none of it is needed to
ship this): a host call listing the extensions that serve a name or a
pattern (`services::providers("*.tracker.about")`), or a manifest field
saying which contracts an extension provides
(`provides = ["tracker@1"]`) with a call to list them; an event when an
extension is installed, updated or removed, so callers ask again then
instead of on a timer; the caller's id in `serve`, so the agenda knows
which tracker sent `agenda.add_issue` without being told; and a deadline
on calls between extensions, which today wait as long as the one
answering takes (`code_serve` sets none for `Answer::Ext`).

## 3. Shapes common to every call

Every request is a JSON object with `"seam": 1`, the contract's major
version the caller speaks, plus the call's own fields. Every answer is
`Ok` with a JSON object, or `Err` with an error (§3.3).

### 3.1 Text is untrusted

Titles, bodies, names, comments and messages come from other people,
through someone else's server. Both sides treat them as text to show,
never to run, open or follow:

- **Cleaned**: no C0 control characters but `\n` (and `\t` in a body),
  no DEL, no C1 controls (U+0080 to U+009F), no bidirectional overrides
  or isolates (U+202A to U+202E, U+2066 to U+2069), no BOM; `\r\n` and a
  lone `\r` become `\n`. A **line** (a title, a name, a message) has its
  newlines and tabs turned into spaces and runs of spaces kept.
- **Capped**, in characters unless it says bytes:

  | What | At most |
  |---|---|
  | `ref` | 200 bytes, no whitespace; an issue whose `ref` is empty or longer is dropped |
  | `url` | 2048 bytes, `https://` or `http://` only, no whitespace, no user or password in it (dropped otherwise) |
  | `title` | 500, one line |
  | `body` | 64 KiB (bytes); cut at a character's edge, with `body_cut: true` |
  | a name, an id, a label, a type, a state | 200, one line |
  | `labels` | 50 of them |
  | `comments` | the 100 most recent, each body 16 KiB, cut with `…` |
  | a message (`message`, `why`, `note`, `said`, `label`, `title`, `not_ready`, `blocked`, `none`) | 300, one line |
  | `updated`, `created` | 64 |
  | an answer | 8 MiB, the tracker dropping comments, oldest first, then cutting bodies to fit |

- **The tracker cleans what it sends, and the agenda cleans it again**:
  a tracker built elsewhere may not. `crates/tracker` (`fenix-tracker`)
  does both sides' cleaning, so a tracker here uses its `clean` module
  and the agenda its `Issue::cleaned`.
- **A cut body isn't written back.** The agenda never sends a `body`
  for an issue that came with `body_cut`: that would cut it in the
  tracker too. It says the description is too long to edit there.
- **Links**: the agenda opens an issue's `url` in your browser (`o`)
  only as Fenix's `open_url` does, never as a file or command.

### 3.2 Tokens never cross

No request or answer has a field for a token, password, cookie or
header, and no tracker puts one in a field it has (a URL is sent
without its user and password). Each tracker holds its own sign-in --
Jira its `jira.token` secret, GitHub and GitLab Fenix's `github.token`
and `gitlab.token` as Review does -- and the agenda has no
`permissions.secrets` or `network` at all. Tests assert it on every
payload (§8).

### 3.3 Errors

An `Err` is a JSON object as text:

```json
{"code": "rate_limited", "message": "HTTP 429 (Too many requests)", "retry_after": 30}
```

| Field | | |
|---|---|---|
| `code` | always | one of the codes below |
| `message` | always | the tracker's own words, shown as they are (a message, §3.1); for Jira, exactly what the agenda showed before |
| `retry_after` | `rate_limited`, optional | seconds |

| `code` | When | What the agenda does |
|---|---|---|
| `not_set_up` | no server, workspace or sign-in configured | says `message`; nothing retried |
| `signed_out` | the sign-in was refused (401, a revoked token) | says it; the op waits for `r` |
| `not_found` | the issue isn't there, or you can't see it | a linked task is marked not found; nothing else fails |
| `rate_limited` | the tracker said to slow down | the op waits; the next timed sync retries it (minutes apart, so `retry_after` is advice it already takes) |
| `offline` | the request didn't reach the tracker | the op waits; the next timed sync retries it |
| `refused` | the tracker said no (403, a 400 for this change) | says it on the task; the op waits for `r` |
| `bad_request` | the request wasn't understood | as `refused` |
| `unsupported` | a call, capability or `seam` it doesn't serve | the agenda stops offering it |
| `failed` | anything else | as `refused` |

An `Err` that isn't JSON is read as `failed` with the text as its
message, except Fenix's own: `nothing offers <name>` (not installed)
and `no service <name>` (installed, but older than the call) are
`unsupported`, and `the extension stopped: ...` is `offline`. So an
older tracker, or one that panicked, never leaves the agenda guessing.

### 3.4 Timing

A tracker answers every call, `Ok` or `Err`, and answers `offline`
itself when its own request times out; Fenix's HTTP client has its own
timeouts. The agenda gives up on a call after **90 seconds** (it checks
on its five-second tick) and treats it as `offline`; a late answer is
dropped. `create` is the exception: it waits on you filling in a form,
so it has no limit.

### 3.5 Versions and capabilities

`seam` is the major version: a change that would make an older agenda
or tracker misread the other is a new major, and a tracker answers a
request whose `seam` it doesn't speak with `unsupported`. Within a
major, fields are only added, optional, and each side ignores fields it
doesn't know -- so a newer tracker works with an older agenda and the
other way round. A call or a write a tracker may leave out is behind a
**capability** in `about.can`; the agenda never makes a call whose
capability isn't listed, and hides the keys that need it.

## 4. The calls

`<id>.tracker.` is left off below: `about` is `jira.tracker.about`.

### 4.1 `about`: who it is, and whether it's ready

Request: `{"seam": 1}`. Answer:

```json
{
  "seam": 1,
  "minor": 0,
  "name": "Jira",
  "ready": true,
  "not_ready": "Jira isn't set up yet -- set its server and token in SPC , (Jira)",
  "can": ["comments", "edit", "priorities", "assign", "comment", "worklog", "create", "remember"],
  "sync_minutes": 10,
  "project_meta": "jira",
  "browse": "https://jira.example.com/browse/{ref}",
  "ref_hint": "PROJ-123"
}
```

| Field | | |
|---|---|---|
| `seam` | required | the major version it speaks; the agenda passes over a tracker whose major isn't its own, and says so once |
| `minor` | optional, 0 | the minor version |
| `name` | required | what the agenda calls it: "Synced 3 issue(s) with Jira", "Link to Jira" (a name, §3.1) |
| `ready` | required | signed in and configured; when false, every other call answers `not_set_up` |
| `not_ready` | when not ready | what to do about it, with the keys (a message) |
| `can` | optional, `[]` | its capabilities (§4.6); unknown ones are ignored |
| `sync_minutes` | optional, 10 | how often linked tasks should be brought up to date; 0 never on a timer. The agenda syncs at the shortest any ready tracker asks |
| `project_meta` | optional | the key of Fenix's project metadata (`Here.project.meta`) that names this tracker's project for the project you're in: `jira` is Fenix's own `.fenix/project.toml` Jira key. With an issue's `project`, it's how `P` (this project only) finds a linked task |
| `browse` | optional | a URL with `{ref}` in it: an issue's address before the first sync has brought its `url` |
| `ref_hint` | optional | an example of a `ref`, for "Link to issue (e.g. PROJ-123)" |

### 4.2 The issue

What every call that returns issues returns:

```json
{
  "ref": "PROJ-12",
  "url": "https://jira.example.com/browse/PROJ-12",
  "project": "PROJ",
  "title": "Fix the thing",
  "body": "What's wrong, as text",
  "body_cut": false,
  "state": {"id": "10103", "name": "On Hold", "stage": "doing", "column": "blocked"},
  "type": "Bug",
  "priority": {"name": "Major", "level": "high"},
  "assignee": {"id": "jo1", "name": "Jo"},
  "mine": true,
  "labels": ["backend"],
  "due": "2026-10-02",
  "updated": "2026-11-14T09:30:00.000+0000",
  "comments": [{"id": "77", "author": "Jo", "body": "Looking into it", "created": "2026-11-13T16:02:00.000+0000"}]
}
```

| Field | | |
|---|---|---|
| `ref` | required | the tracker's own key for it, unique within the tracker and stable: what the agenda stores. Jira `PROJ-12`, GitHub and GitLab `owner/repo#12` (GitLab's group paths included), Linear `ENG-123` |
| `url` | optional | its page, for `y` and `o` |
| `project` | optional | its project, repository or team, as `about.project_meta`'s value would name it |
| `title` | required | one line |
| `body` | optional, `""` | **plain text as a person would read it raw**: Markdown passes through as it is (GitHub, GitLab, Linear), and so does Jira Server's wiki markup, since the agenda writes back what it shows; rich text that isn't readable raw (Jira Cloud's ADF, 6.3) is flattened to paragraphs, list items as `- `, code blocks kept, everything else as its text, and such a tracker doesn't offer `edit` of the body unless it converts back |
| `body_cut` | optional, false | the body was longer than §3.1 allows |
| `state` | required | where it stands. `id` the tracker's own (a status id, `open`, a workflow state's id); `name` as people see it; `stage` one of `todo`, `doing`, `done` -- how far along, whatever the workflow calls it (Jira's status category, Linear's state type, GitHub's open with no assignee or label saying otherwise as `todo`); `column` one of `todo`, `doing`, `blocked`, `done` -- the agenda column the tracker puts it in, which is `stage` unless the tracker knows it's blocked (Jira: the status Blocked means in its project, or its Flagged field) |
| `type` | optional | a name |
| `priority` | optional | `name` the tracker's; `level` one of `urgent`, `high`, `medium`, `low`, the tracker's mapping onto the agenda's four. No priority is the agenda's Medium |
| `assignee` | optional | `id` what `update`'s `assignee` takes back, `name` shown. A tracker with several assignees (GitHub, GitLab) gives the first, and `mine` says whether you're among them |
| `mine` | optional | assigned to the signed-in person; absent when the tracker can't tell. A linked issue no longer yours is marked "reassigned" |
| `labels` | optional, `[]` | names |
| `due` | optional | `YYYY-MM-DD` |
| `updated` | required | when it last changed, ISO 8601 with an offset (RFC 3339, or the `+hhmm` offset Jira writes), as the tracker gives it: compared for equality and used as `since`, never parsed for anything else |
| `comments` | with `comments` | the most recent, oldest first; `created` as `updated` |

### 4.3 `assigned`: the issues assigned to me

```json
{"seam": 1, "limit": 100, "cursor": null, "since": null}
```

| Field | | |
|---|---|---|
| `limit` | optional, 50 | 1 to 200: the most it wants; a tracker may answer fewer and give `next` |
| `cursor` | optional | a previous answer's `next`, as it was |
| `since` | optional | an `updated` value: only issues changed at or after it |

Answer: `{"issues": [<issue>...], "next": "100"}`. `next` is absent on
the last page and is the tracker's own (Jira's `startAt`, a GraphQL
cursor, a page number). Open issues only (Jira: status category not
Done; GitHub and GitLab: open; Linear: not completed or cancelled),
assigned to the signed-in person, most recently updated first. Jira
scopes it to `jira.projects` when that has any, as before.

### 4.4 `issues`: these issues

```json
{"seam": 1, "refs": ["PROJ-12", "https://jira.example.com/browse/OPS-3"]}
```

At most 50 `refs`; a ref may be the issue's URL, which the tracker
resolves. Answer: `{"issues": [...], "missing": ["OPS-3"]}`. An issue
not found, or not visible to you, is in `missing` as it was asked, and
the call still succeeds -- unless the tracker can't tell which one was
missing, when it answers `not_found` for the whole call (Jira's search
does, for a key that never existed; one ref asked alone is fetched
directly, and its 404 is `not_found`). An issue found by its URL comes
back under its `ref`, and the caller matches it by `ref`.

### 4.5 Moving it on: `transitions` and `transition`

What you can do next is the tracker's list:

```json
{"seam": 1, "ref": "PROJ-12", "toward": "blocked", "from": {"id": "3", "column": "doing"}}
```

| Field | | |
|---|---|---|
| `ref` | required | |
| `toward` | optional | the agenda column a card was moved to: a hint, so a tracker can offer moves that only make sense for it (Jira's Flagged field) and leave out the rest |
| `from` | optional | the state the agenda last saw (`id`, `column`), so a tracker needn't fetch the issue to know it |

Answer:

```json
{
  "transitions": [
    {"id": "31", "name": "Start progress", "to": {"id": "3", "name": "In Progress", "stage": "doing", "column": "doing"}},
    {"id": "flag", "name": "Flag", "label": "Flag as impediment (keep the status)", "to": {"id": "3", "name": "In Progress", "stage": "doing", "column": "blocked"},
     "remember": "{\"project\":\"PROJ\",\"blocked\":\"flag\"}", "note": "Blocked in PROJ now flags the issue as an impediment", "said": "flagged"},
    {"id": "local", "name": "Keep Blocked local", "local": true, "to": {"id": "3", "name": "In Progress", "stage": "doing", "column": "blocked"}}
  ],
  "ask": {"title": "What does Blocked mean in PROJ?", "note": "Pick what Blocked means in PROJ -- asked once, then remembered (b on the project in SPC j j changes it)"},
  "blocked": "Blocked  (On Hold)",
  "why": "\"On Hold\" isn't reachable from \"Open\" -- pick a step from here"
}
```

| A move's field | | |
|---|---|---|
| `id` | required | what `transition` takes back; the tracker's own, opaque to the agenda |
| `name` | required | its name |
| `to` | required | the state it lands in: `id`, `name`, `stage`, `column` as an issue's |
| `label` | optional | the picker's row; else `name`, or `name  ->  to.name` when they differ |
| `local` | optional, false | it changes nothing in the tracker: the agenda moves the card and sends nothing |
| `remember` | with `remember` | a choice the tracker keeps once you pick this move (opaque); the agenda calls `remember` with it at once |
| `note` | optional | what the agenda says when you pick it |
| `said` | optional | what it says once it's done; else "moved to `to.name`" |

| The answer's field | | |
|---|---|---|
| `transitions` | required | at most 100; possibly empty |
| `ask` | optional | the tracker wants you to choose, even among one: the agenda shows every move under `title` and says `note` |
| `blocked` | optional | the label of the agenda's own "Blocked" row in a status picker; else "Blocked..." |
| `why` | optional | why none reaches `toward`, in the tracker's words |

`transitions` changes nothing (`remember` aside, which only keeps a
choice). Doing one is `transition`:

```json
{"seam": 1, "ref": "PROJ-12", "id": "31"}
```

answered `{"said": "moved to In Progress"}` (`said` optional, a
message). Trackers differ, and the list is how: GitHub offers *Close as
completed* (`to.stage` `done`), *Close as not planned* (`done`) and
*Reopen* (`todo`); GitLab *Close* and *Reopen*, and a move per scoped
`workflow::` label if the project uses them; Linear one move per
workflow state of the team; Jira its workflow's transitions, plus
Flagged and "local" for Blocked.

**Who decides to move.** The agenda performs a move only when you
picked it, or moved a card to a column one move -- exactly one --
reaches; otherwise it asks, and Esc puts the card back. That's Jira's
flow as it was, and every tracker gets it. A move that's queued
(offline) is sent in order with your other changes.

**How the agenda uses the list**, which is the same for every tracker:

- `s` on a linked task: every move, and a "Blocked" row (`blocked`).
- A card moved to a column the issue's `column` already is: nothing is
  sent. Otherwise `transitions` with `toward`; with `ask`, a picker of
  them all; else the moves whose `to.column` is the column -- none puts
  the card back with `why` (and, for Blocked, offers every move), one
  is done, several are a picker.
- `remember` is called the moment you pick a move carrying it.

### 4.6 What a tracker may also offer (capabilities)

| `can` | Calls | What the agenda does with it |
|---|---|---|
| `comments` | issues carry `comments` | a task's Activity shows them |
| `edit` | `update` with `title`, `body`, `priority`, `due` | a linked task's edits go to the tracker, and conflicts are settled field by field |
| `priorities` | `priorities` | `p` on a linked task picks one of the tracker's |
| `assign` | `people`; `update` with `assignee` | `A` on a linked task |
| `comment` | `comment` | `C`, and posting a note as a comment |
| `worklog` | `worklog` | the week's time sent as worklogs (`W`) |
| `create` | `create` | "Create a new issue from this task" when linking |
| `remember` | `remember` | moves that carry `remember` |

Without one, the agenda keeps that part local: an edit stays on the
task, `p` is the agenda's own priorities, and the keys that would write
to the tracker say it doesn't take that from the agenda.

- **`update`** `{"seam": 1, "ref": "PROJ-12", "set": {"title": "..."}}`:
  one field per call, of `title` (a line), `body` (text), `priority`
  (the tracker's `name`), `due` (`YYYY-MM-DD`, or `null` to clear),
  `assignee` (`{"id": "jo1", "name": "Jo"}`; the id from `people`).
  Answer `{}`, or `{"said": "..."}`.
- **`priorities`** `{"seam": 1, "ref": "PROJ-12"}` → `{"priorities":
  [{"name": "Major", "level": "high"}]}`, at most 100.
- **`people`** `{"seam": 1, "ref": "PROJ-12"}` → `{"people": [{"id":
  "jo1", "name": "Jo"}], "none": "no people to assign to -- ..."}`, at
  most 500; `none` says what to do when it's empty.
- **`comment`** `{"seam": 1, "ref": "PROJ-12", "body": "..."}`, a body
  of at most 64 KiB → `{}`. Posting only: the thread is the tracker's
  page's.
- **`worklog`** `{"seam": 1, "ref": "PROJ-12", "seconds": 4500,
  "started": "2026-11-12T09:15:00.000+01:00"}`: time worked, filed on
  the day it started (RFC 3339) → `{}`.
- **`create`** `{"seam": 1, "title": "...", "body": "..."}`: the
  tracker's own new-issue form, opened and filled in. Answered `{"ref":
  "PROJ-13"}` once you've made it, or `{"ref": null}` if you closed it
  (Jira, as before, leaves a closed form unanswered: the agenda waits on
  nothing for it). No time limit (§3.4).
- **`remember`** `{"seam": 1, "ref": "PROJ-12", "remember": "..."}` →
  `{}`: keeps a choice a move carried.

### 4.7 What the agenda serves back

Trackers may call these, as Jira's page does. `tracker` is the `<id>`;
a call without it is from Jira 1.2, before the seam, and means `jira`.

| Service | Payload | |
|---|---|---|
| `agenda.add_issue` | `{"tracker": "jira", "ref": "PROJ-12", "clock": false}` | the issue as a task (and its clock started) |
| `agenda.open_task` | `{"tracker": "jira", "ref": "PROJ-12"}` | its task's page; `Err` when it isn't in the agenda |
| `agenda.linked` | `{"tracker": "jira"}` | the refs it has linked, as a JSON array; every tracker's without `tracker` |
| `agenda.sync_quietly` | `null` | linked tasks brought up to date now, quietly |

`key` is still read in place of `ref`, as Jira 1.2 and the notebook
send it.

## 5. Several trackers at once

- **Keyed by both.** A linked task stores its tracker's `<id>` and the
  issue's `ref`; two trackers' `ENG-12` are two issues. A ref given
  without a tracker (the notebook's links, Jira 1.2) is looked up in
  every tracker, the first match winning.
- **Labelled when it matters.** A task shows its `ref` as it did. Where
  issues from several trackers are listed together (the import and link
  pickers) each row ends with its tracker's name; with one tracker,
  nothing is added. Words that named Jira name the task's tracker
  ("Take Jira's: ...", "Jira comment") or, with none or several, say
  "the tracker".
- **Merged by asking each.** Import and link ask every ready tracker's
  `assigned` and list them in the trackers' order (§2), each tracker's
  issues as it sorted them. A sync asks each tracker for its own linked
  issues, in batches of 50, all at once, and says what happened in one
  notification when the last has answered: "Synced 5 issue(s) with Jira
  and Linear", or the first failure.
- **The outbox stays one queue**, sent one change at a time in the
  order made, whichever tracker each is for, so a title change never
  overtakes a move made before it.
- **A tracker gone** (uninstalled) leaves its tasks linked, and their
  changes waiting; the task says its tracker isn't installed.
- **A ref that looks like a key typed into a new task's title**
  (`PROJ-12`, capitals, a dash and digits, as Jira and Linear write
  them) is asked of every ready tracker that answers `issues`, and
  linked to the first that has it.

## 6. Jira on the seam

`fenix.jira` 1.3 serves `jira.tracker.*` for Server and Data Center,
with what the agenda did before moved behind it, in `crates/jira`'s
`tracker` module, so the Jira extension and the agenda's fallback
(§7) run the same code:

- **`about`**: `ready` when `jira.base_url` and `jira.token` are set;
  `not_ready` the agenda's old words; `sync_minutes` is
  `jira.sync_minutes`; `can` all eight; `project_meta` `jira`;
  `browse` `<base>/browse/{ref}`.
- **Issues**: the same requests as before (`/myself` once, the Flagged
  field once when a project's Blocked is `flag`, then
  `search?jql=key IN (...)` in fifties, or `GET issue/{key}` for one),
  and the same mapping: `column` from `jira.blocked` (the status, or
  Flagged) and the status category; `level` from `jira.priorities`,
  else the guess from the name the agenda made; `mine` from the
  assignee's username against `/myself`; the description and comments
  as Jira's text.
- **`assigned`**: the agenda's JQL as it was,
  `assignee = currentUser() AND statusCategory != Done [AND project IN (...)] ORDER BY updated DESC`,
  with `AND updated >= "yyyy/MM/dd HH:mm"` for `since`, paged by
  `startAt`.
- **Moves**: the workflow's transitions, each `to.column` worked out
  as for an issue. Toward Blocked, `jira.blocked` decides: its status
  (one move, or none with `why`), Flagged (a `flag` move), local (a
  `local` move), or not decided yet (an `ask` of every reachable
  status, Flagged and local, each with `remember`, as the agenda asked
  before). Out of Blocked while Flagged, an `unflag` move, or
  `unflag+<id>` combined with a transition when the column also
  changes.
- **Writes**: the same REST calls as before.
- **Errors**: Jira's own words as before (`HTTP 404 (Issue Does Not
  Exist)`), classed by status: 401 `signed_out`, 403 and other 4xx
  `refused`, 404 `not_found`, 429 `rate_limited`, 5xx `failed`, no
  answer `offline`, no server or token `not_set_up`.

It keeps serving `jira.request`, `jira.config`, `jira.set_blocked` and
`jira.new_issue` for the agenda 1.2 (§7).

## 7. Updating one at a time

Extensions update separately, so both mixed pairs must work:

| Agenda | Jira | How |
|---|---|---|
| 1.2 | 1.2 | as before |
| 1.2 | 1.3 | Jira 1.3 still serves `jira.request`, `jira.config`, `jira.set_blocked`, `jira.new_issue`, unchanged |
| 1.3 | 1.2 | the agenda asks `jira.tracker.about`, is told `nothing offers` it, asks `jira.config`, and -- answered -- serves the seam to itself with `crates/jira`'s `tracker` module over `jira.request` (`extensions/agenda/src/legacy.rs`): the same code Jira 1.3 runs, so the same results; `remember` is `jira.set_blocked`, `create` is `jira.new_issue` |
| 1.3 | 1.3 | the seam |

The agenda's own file, `agenda.json`, keeps its shape: a link is still
under `"jira"` with the same fields, plus `tracker`, `url`, `project`,
`column`, `description_cut` and `transient` that 1.2 ignores; a link
without `tracker` is Jira's. So going back to 1.2 keeps your links.

**When the old path goes.** Together, in the next major of each
(agenda 2.0, Jira 2.0; the Jira bundle's ranges move to `^2` with them):
the agenda drops `legacy.rs` and its `fenix-jira-client` dependency, and
Jira stops serving the four old services -- `jira.request`, which lets
any extension send Jira any request as you, first. Not before both 1.3s
have been out long enough that an update reaches everyone who'd notice;
a removed service is a major release (CONTRIBUTING, Versions).

## 8. Tests

- `crates/tracker`: cleaning and capping each field, errors read from
  every form, and `FakeTracker`, an in-memory tracker that answers every
  call (and fails any of them on request) for other crates' tests.
- `crates/jira`: the seam served against recorded Jira responses
  (`tests/fixtures/`), each call; errors classed; no token in any
  answer.
- `extensions/agenda`, on the test kit: `FakeTracker` behind
  `fake.tracker.*` driving the agenda -- assigned with paging, one issue,
  moves, each error -- two trackers at once, the Jira fixtures through
  the old path and the new compared field by field, both mixed pairs,
  untrusted text cleaned and capped, no token in any payload.
- `extensions/jira`, on the test kit: the four services the agenda 1.2
  calls, still served and answering as before, and the seam's `about`,
  `remember` and errors.

## 9. For the next tracker: a checklist

An extension that serves the seam:

- [ ] Serves `<id>.tracker.about`, `.assigned`, `.issues`,
      `.transitions`, `.transition`, listed in `contributes.services`;
      `<id>` is its name (§2) and is in the agenda's `KNOWN`.
- [ ] `about.ready` false, with `not_ready` naming its settings' keys,
      until it's signed in; every other call `not_set_up` then.
- [ ] Builds each issue with `fenix_tracker::Issue` and sends it
      `cleaned()`; `ref` stable and unique; `stage` and `column` mapped
      from its own states; `level` from its priorities if it has any.
- [ ] Pages `assigned` with its own cursor, honours `since`.
- [ ] Resolves a URL in `issues`; puts what it didn't find in `missing`.
- [ ] Offers its moves as a list, never assumes the agenda's columns
      are its states; `transitions` changes nothing.
- [ ] Classes its errors into §3.3's codes, its own words in `message`;
      `retry_after` from the tracker's header.
- [ ] Lists in `can` only what it serves; writes go through the same
      client its page uses.
- [ ] Holds its own token; nothing of it in an answer. Asks `network`
      for its hosts only.
- [ ] Tests on recorded responses, and the agenda's tests copy
      `FakeTracker` for anything the agenda should do with it.

What each of the three needs beyond that:

- **GitHub Issues** (`github-issues`): `ref` `owner/repo#n`, from
  `crates/forge`'s client and Review's sign-in (`github.token`);
  `assigned` is `GET /issues?filter=assigned&state=open` (pull requests
  left out), paged by `Link`; moves *Close as completed*, *Close as not
  planned*, *Reopen* (`state_reason`); `stage` `todo` for open, `done`
  for closed, `column` from a `blocked` label if the repository uses
  one; no `priorities`, `worklog` or `remember`; `body` Markdown as is;
  `retry_after` from `Retry-After` or `X-RateLimit-Reset`.
- **GitLab Issues** (`gitlab-issues`): `ref` `group/project#iid`, from
  `crates/forge` and `gitlab.token`; `assigned` is
  `GET /issues?scope=assigned_to_me&state=opened`, paged by
  `X-Next-Page`; moves *Close*, *Reopen*, and a move per scoped
  `workflow::` label when the project has them; `due` from `due_date`;
  `worklog` from `/add_spent_time` if wanted (`seconds` as `1h30m`).
- **Linear** (`linear`): `ref` the identifier (`ENG-123`); its GraphQL
  API and its own token (`linear.token`); `assigned` is
  `viewer.assignedIssues` with a state type filter, paged by
  `pageInfo.endCursor`; one move per workflow state of the issue's
  team, `stage` from the state's type (`unstarted` `todo`, `started`
  `doing`, `completed` and `canceled` `done`); `priority` from its 0
  to 4 (`level` urgent, high, medium, low, none absent).
- **Jira Cloud** (6.3), in the same extension: sign-in by email and API
  token; REST v3, whose description and comments are ADF, flattened as
  §4.2 says, and `edit` of `body` converting back or left out;
  `assignee.id` an `accountId`.

## As built

Recorded after building, on 14 November 2026.

- `crates/tracker` (`fenix-tracker` 1.0.0): the shapes (`seam`), the
  cleaning (`clean`), errors (`SeamError`) and `FakeTracker`.
- `crates/jira` 1.1.0: `tracker`, the seam served over a `JiraClient`;
  paged search; labels read; recorded responses in `tests/fixtures`.
- `fenix.jira` 1.3.0 serves it; `fenix.agenda` 1.3.0 calls it, with the
  Jira 1.2 fallback in `legacy.rs`. The Jira bundle is unchanged: its
  `^1` ranges take both.
- The agenda's service calls wait in `calls.rs`, its own small
  executor over `call_service` and `ServiceReply`, rather than the API's
  `run`, so the test kit's fake host answers them.

**Not quite as before**, for Jira -- each found while moving it, each
small, and each listed here so it isn't mistaken for a slip:

- A description's `\r\n` line ends arrive as `\n` (§3.1). A base kept
  by 1.2 compares as cleaned, so the first sync after updating sees no
  change in it.
- A sync of one linked issue that's been deleted marks that task "not
  found" instead of failing the sync (Jira answers a single issue's
  fetch with 404; its search for several leaves the missing out).
- Out of Blocked while Flagged: the flag cleared and the transition are
  one move, said once ("unflagged, and moved to Done"); when no
  transition reaches the column, nothing is sent and the card goes back
  (before, the flag was cleared anyway).
- A Flagged or local Blocked move asks Jira once, which answers without
  a request, so the card shows ↑ for that moment.
- Changing `jira.blocked` on the settings page re-files a linked card at
  the next sync; before, only a change of its status did.
- Comments past the 100 most recent, and bodies past 64 KiB, aren't
  kept; such a description isn't edited from the agenda.
- Words: with no tracker installed, "that needs a tracker extension,
  such as Jira"; the commands' titles say "your tracker"; a failure to
  keep what Blocked means says "Jira couldn't keep that choice".
- Requests: the search asks for `labels` too; an import of more than
  Jira's page size asks for the rest with `startAt`. The first request
  is the one sent before.
