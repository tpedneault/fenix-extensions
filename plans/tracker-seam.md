# The tracker seam: the agenda's issues from any tracker

> Milestone 6.2, first written on 14 November 2026 before it was built,
> and revised the same day after an independent review (*Changed after
> review*, next). It's the contract between the agenda and an extension
> that knows a tracker, for the agents who write the trackers after it
> (Jira Cloud 6.3; GitHub Issues, GitLab Issues and Linear 6.4): what the
> agenda asks, what the tracker answers, in JSON, with every field. Jira
> Server and Data Center move onto it first. Every claim about Fenix
> cites its source at `/home/user/fenix` (API 1.5, `fenix-extension-api`
> tag `v1.5.0`); nothing here was run in a Fenix window or against a
> Jira server, and Atlassian's documentation couldn't be reached from
> where this was written (claims about Jira's REST API that rest on
> memory say so).

## Changed after the second review

A second independent review (15 November 2026) found the fixed seam
still had holes. What moved, for the four trackers built on it:

1. **The gate's API and words** (`fenix_tracker::gate`, §3.6). The
   gate now holds the waiting writes itself and says what to do as
   `Step`s (`check`, `answered`, `expire` at `deadline`, `reset`). A
   question lapses after 60 s -- Fenix replaces an open picker without
   answering it -- and refuses what waited; at most 50 writes wait.
   "Not now" (or Esc) refuses for 10 minutes without asking, so it
   can't be asked again until you give in; changing `<name>.seam_writes`
   ends that. The words say what's true: "An extension asks to change
   issues in Jira for you (Fenix can't tell which; usually the Agenda)
   -- allow?", **Allow every extension's changes through Jira until
   Fenix restarts** / **Not now -- refuse them for 10 minutes**. A
   `caller` can be given once Fenix says who's calling: then the
   question names it and an allow is kept for it alone.
2. **`Dedupe` is keyed by call and `op`**, and a payload that isn't the
   first one's is refused (§3.7). A write sent again carries the `op` it
   had the first time: the agenda's worklogs too.
3. **Every call about an issue is by its `id`** once the caller knows it
   (§4.2): writes, moves, priorities and people, not only reads.
4. **Only `unsupported` drops a change.** `bad_request` waits for `r`
   like `refused` (Linear's `BAD_USER_INPUT` can be this moment's). A
   dropped change leaves a mark on its task, with `r` to send it again
   or take the tracker's value back (§3.3).
5. **Keeping and showing text are two jobs** (§3.1): what's kept and
   written back is the tracker's own text, byte for byte (`title_cut`
   joins `body_cut`); what's shown is cleaned, and the cleaning keeps
   what living scripts and emoji need (ZWNJ, ZWJ, LRM, RLM, ALM, CGJ, one
   variation selector after a base) and removes the rest, variation
   selectors smuggling ASCII included.
6. The agenda sends each tracker's queue on its own; its non-Jira links
   survive a round trip through agenda 1.2 (§7). Jira refuses
   `jira.request` for Cloud and credentials over `http://` (§6).

## Changed after review

Seam 1 changed before any tracker shipped it. If you started from the
first version, this is what moved:

1. **Names**: a tracker serves `<its full extension id>.tracker.<call>`
   -- `fenix.jira.tracker.about`, not `jira.tracker.about` (§2). Made in
   one place, `fenix_tracker::service`. `about` answers its `id`, and
   the agenda refuses one that answers as another extension.
2. **`about` across versions**: answered whatever `seam` it's asked
   with, listing `seams` it speaks; the caller picks the highest it
   shares. A tracker that shares none is "installed, but too old" or
   "too new", never "not installed" (§3.5, §4.1).
3. **An issue's `id`**, immutable and never shown, beside its `ref`,
   which may change when an issue moves. `issues` takes ids too, and
   each issue answered says what it was `asked` as (§4.2, §4.4).
4. **Per-field capabilities**: `edit.title`, `edit.body`,
   `edit.priority`, `edit.due`, `edit.assignee`, `edit.labels` replace
   `edit`, `priorities` and `assign` (§4.6). `update` can unassign
   (`assignee: null`) and set `labels`.
5. **`op`**: every write carries its own id, the same each time it's
   sent, so a tracker can answer a repeat without doing it twice (§3.7).
6. **The write gate**: every tracker asks you once a session before the
   first write the seam brings it, with the same words and the same
   setting, `<name>.seam_writes` (§3.6, `fenix_tracker::gate`).
7. **`since` is gone** from `assigned`: nothing sent it, and doing it
   right needs time zones every tracker gets differently. It can come
   back in a minor (§4.3).
8. **Opaque values** -- ids, cursors, `remember`, move ids -- are kept
   byte for byte or dropped, never cut or cleaned; ids may hold spaces
   (`workflow::In Review`) (§3.1).
9. **Errors**: a new code, `unknown`, for an answer that never came; a
   write that ended so is never re-sent by itself (§3.3, §3.7). An
   unknown field in `update` is `unsupported`, not `bad_request`.
   Mapping guidance for 403 rate limits, 410 and 5xx (§3.3).
10. **From the first trackers built on it** (Linear, Jira Cloud):
    - a tracker with no Fenix project metadata says which of its
      projects you're in with a new call, `project` (§4.7);
    - `create` may answer at once with `opened` instead of holding the
      call while you fill in a form (§4.6);
    - a priority's `level` may be `null`: a priority meaning none (§4.2);
    - `about.blocked: false` says the tracker has no Blocked, so the
      agenda keeps Blocked to itself instead of a faked `local` move
      (§4.1);
    - errors are classed by whatever the API uses, status or body, and
      `retry_after` is in seconds (§3.3);
    - `missing` comes back exactly as asked (§4.4);
    - the gate covers every write wherever the extension serves it,
      `create` and `remember` included (§3.6);
    - the agenda's test world is a public, documented helper,
      `fenix_agenda::testing`, so a tracker runs the agenda against
      itself from its own crate (§8);
    - from GitHub and GitLab: `browse` is only for trackers whose
      address is a simple template -- a forge leaves it out and every
      issue carries its `url`; `assignee` stays one person, the first,
      with `mine` true if you're any of them; the gate is on writes that
      arrive through the seam only, never on the tracker's own page.
11. **Smaller**: `sync_minutes` is per tracker with a floor of 5;
    `retry_after` is honoured; `about` must be cheap, and the agenda
    asks lazily; a whole-call `not_found` is forbidden for more than one
    ref; stage mappings for GitHub and Linear's six state types; `local`
    moves for columns a tracker doesn't model; more invisible characters
    cleaned (§3.1, §4, §9).

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
2. **these issues** -- by id, key or URL, to keep linked tasks in step;
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
none of them.

## 2. Names, and who can answer to them

### What Fenix 1.5 allows

- **Routing.** A service call goes to the first installed extension,
  other than the caller, whose manifest lists that exact name
  (`fenix-gui/src/app/wasm_host.rs`, `code_server`). The installed list
  is machine-managed extensions first, then the rest **sorted by id**
  (`fenix-extensions/src/store.rs`, `fenix-gui/src/installed.rs`). A name
  nobody lists is answered `Err("nothing offers <name>")`.
- **Who may list a name.** The manifest check
  (`fenix-extensions/src/manifest.rs`, `ours`, at `Manifest::parse`) lets
  an extension serve a name that starts with **the last part of its id**
  plus a dot, **its full id** plus a dot, or **any namespace it
  declares** plus a dot. Namespaces aren't checked or reserved at all,
  and nothing refuses a second extension listing a name another serves.
- **Who called.** `serve(call, name, payload)` isn't told; Fenix knows
  (`Answer::Ext { asker }`) but doesn't pass it on.

Tried against Fenix's own `Manifest::parse` (a probe crate,
`scratchpad/m6-seam/names-probe`), for the name
`fenix.jira.tracker.about`:

| Extension | Manifest | Accepted? |
|---|---|---|
| `fenix.jira` | `services = ["fenix.jira.tracker.about"]` | yes (its full id) |
| `acme.jira` | the same | **no**: its last part is `jira`, not `fenix` |
| `fenix.tracker` | the same | **no** |
| `acme.fenix` | the same | **yes**: its last part is `fenix` |
| `evil.a` | `namespaces = ["fenix"]` | **yes** |
| `evil.b` | `namespaces = ["fenix.jira"]` | **yes** |

and the short name the first version used, `jira.tracker.about`, was
accepted for `acme.jira` with nothing declared (the review's probe).

**So no name is hijack-proof on Fenix 1.5.** Any extension can serve any
name by declaring a namespace, and one whose id sorts before the real
one's (`acme.*` before `fenix.*`) wins every call.

### The names

A tracker serves its calls under **its full extension id**:
`fenix.jira.tracker.about`, `fenix.jira.tracker.issues`, ...;
`acme.youtrack.tracker.about` for one built elsewhere. The one place the
name is made is `fenix_tracker::service(provider, call)`; if Fenix comes
to reserve names or route by id, that's what changes.

What that buys, and what it doesn't:

- **It ends accidental collisions.** An honest `acme.jira` or
  `someone.linear` (a Jira Cloud extension of someone else's, say)
  naturally owns `jira.` or `linear.` by its last part, and with short
  names would have silently taken the agenda's calls away from
  `fenix.jira`. With full ids it can't, without a deliberate claim.
- **It doesn't stop a deliberate impostor.** One declaring
  `namespaces = ["fenix"]`, or with an id ending `.fenix`, still serves
  the names, and wins if it sorts first.
- **`about` answers its `id`, and the agenda refuses a mismatch** ("…
  answers as acme.other, not fenix.jira -- not used"). That catches an
  extension answering a name meant for another by mistake; an impostor
  can just say the right id, so it proves nothing against one.

### What Fenix must add

For the owner, to take to Fenix (1.6 or later):

1. **The caller's id in `serve`** (the asker Fenix already knows), so a
   tracker can take writes from `fenix.agenda` and ask once, naming the
   caller, for anyone else. Ideally with a declared permission
   (`permissions.services = ["*.tracker.*"]`) shown at install.
2. **Reserved names**: a service, command or setting starting with an
   installed extension's full id plus a dot only ever that extension's;
   the `fenix` publisher's namespace reserved; and a namespace that's an
   installed extension's id, or a prefix of one, refused at install.
3. **No silent second claimant**: installing an extension that lists a
   name another installed extension serves is refused, or at least
   shown, rather than decided by id order.
4. **A way to list who serves a pattern**
   (`services::providers("*.tracker.about")`), and **an event when an
   extension is installed, updated or removed**, for discovery.
5. **A deadline on calls between extensions** (`code_serve` sets none
   for `Answer::Ext`).

And, in this repository, worth adding to `xtask validate` once someone
owns it: refuse a package built elsewhere whose `namespaces` or
services start with `fenix.` or with a `fenix.*` extension's short name.

### Discovery, as Fenix 1.5 allows it

The agenda asks `<id>.tracker.about` of every id it knows -- the
store's, `fenix_tracker::KNOWN` (`fenix.jira`, `fenix.github-issues`,
`fenix.gitlab-issues`, `fenix.linear`), then each id in its setting
`agenda.trackers` -- **all at once**, so one that hangs delays none of
the others. One that isn't installed answers at once that nothing
offers it. Any call to an installed tracker *starts* its extension
(`wasm_host.rs`), so the agenda asks **lazily**:

- when it starts, only the trackers your linked tasks or queued changes
  are with -- they'd be woken by the next sync anyway;
- the rest when its page opens, and when you sync, import or link;
- again whenever it's been five minutes, when `agenda.trackers`
  changes, when a tracker says it changed (`agenda.tracker_changed`,
  §4.8), and at once -- the command waiting for the answer -- when you
  ask for something and no tracker is ready, or a call answers
  `not_set_up` or `signed_out`. So a tracker you've just set up is
  never reported as not set up.

A tracker added to this repository is added to `KNOWN` in the same pull
request, in a minor release of `fenix-tracker` and the agenda.

## 3. Shapes common to every call

Every request is a JSON object with `"seam": <n>`, the major version the
caller chose (§3.5), plus the call's own fields. A tracker ignores
request fields it doesn't know. Every answer is `Ok` with a JSON object,
or `Err` with an error (§3.3).

### 3.1 Text is untrusted, opaque values aren't touched

Titles, bodies, names, comments and messages come from other people,
through someone else's server. Both sides treat them as text to show,
never to run, open or follow. **Keeping and showing are two jobs**:

- **Kept as it came.** What's stored, and written back to the tracker,
  is the tracker's own text, byte for byte -- capped in size (and said
  so: `title_cut`, `body_cut`), never cleaned. An edit starts from that
  text, so it can never write a cleaned copy back over the original
  (which would remove, for everyone, a Persian ZWNJ the cleaning took
  out). A field that was cut is never written back.
- **Cleaned to be shown** (`fenix_tracker::clean::line`, `text`,
  `name`, `message`): wherever text is drawn, picked from, notified, or
  given to a coding agent (the control channel's `agenda.tasks`). Gone:
  C0 controls but `\n` (and `\t` in a body), DEL, C1 controls, the
  bidirectional embeddings, overrides and isolates (U+202A to U+202E,
  U+2066 to U+2069), the zero-width space, the word joiner and invisible
  operators (U+2060 to U+206F), the soft hyphen, the BOM, the
  interlinear annotation marks (U+FFF9 to U+FFFB), the Hangul fillers,
  the Mongolian vowel separator, **the tag characters (U+E0000 to
  U+E007F)**, which spell hidden ASCII, and **variation selectors
  smuggling it** (U+FE00 to U+FE0F, U+E0100 to U+E01EF): only one is
  kept, right after a character it can modify. Kept, because living
  scripts need them: ZWNJ and ZWJ between two characters that show
  (Persian, Devanagari, emoji families; a stray one at an edge goes),
  LRM, RLM and ALM, the combining grapheme joiner, Mongolian free
  variation selectors. `\r\n` and a lone `\r` become `\n`; a **line**
  has its newlines and tabs turned into spaces.
- **Opaque values are never cleaned**: an issue's `id`, a state's, a
  person's, a move's `id`, a cursor (`next`), a `remember`, an `asked`.
  They go back to the tracker byte for byte, so they're kept exactly as
  they came when within their size, and dropped otherwise -- never cut,
  trimmed or changed. They may hold spaces (`workflow::In Review`, a
  Jira username). Where one is shown (a person's id in a picker), it's
  shown cleaned and sent as it was.
- **Capped**, in characters unless it says bytes:

  | What | At most |
  |---|---|
  | `ref` | 200 bytes, no whitespace, nothing hidden; an issue whose `ref` isn't kept is dropped |
  | an id (issue, state, person, move, comment) | 512 bytes, opaque; an issue or move whose id is longer is dropped |
  | a cursor, a `remember` | 4096 bytes, opaque; a longer cursor fails the call, a move with a longer `remember` is dropped |
  | `url` | 2048 bytes, `https://` or `http://` only, no whitespace, no user or password in it (dropped otherwise) |
  | `title` | 500, kept as it came; cut with `title_cut: true` (shown as one line) |
  | `body` | 64 KiB (bytes); cut at a character's edge, with `body_cut: true` |
  | a name, a label, a type, a state's name | 200, one line |
  | `labels` | 50 of them |
  | `comments` | the 100 most recent, each body 16 KiB, cut with `…` |
  | a message (`message`, `why`, `note`, `said`, `label`, `title`, `not_ready`, `blocked`, `none`) | 300, one line |
  | `updated`, `created` | 64 |
  | an answer | 8 MiB, the tracker dropping comments, oldest first, then cutting bodies to fit; the agenda doesn't even read one over 8 MiB and 64 KiB (`clean::too_big`) |

- **The tracker cleans what it sends, and the agenda cleans it again**:
  a tracker built elsewhere may not. A tracker here builds its issues
  with `fenix_tracker::Issue` and sends them `.cleaned()`.
- **A cut body isn't written back.** The agenda never sends a `body` for
  an issue that came with `body_cut`: that would cut it in the tracker
  too.
- **Refs reach URLs only checked.** A tracker validates a `ref` or id
  against its own form before it goes into a request path or query, and
  percent-encodes path segments: Jira takes `^[A-Z][A-Z0-9_]*-[0-9]+$`
  (after upper-casing) or a numeric id, nothing else. `../../x` and
  `A-1?x=1` are `bad_request`.
- **Links**: the agenda opens an issue's `url` in your browser (`o`)
  only as Fenix's `open_url` does, never as a file or command.

### 3.2 Tokens never cross

No request or answer has a field for a token, password, cookie or
header, and no tracker puts one in a field it has (a URL is sent without
its user and password). Each tracker holds its own sign-in -- Jira its
`jira.token` secret, GitHub and GitLab Fenix's `github.token` and
`gitlab.token` as Review does -- and the agenda has no
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
| `retry_after` | `rate_limited`, optional | whole seconds from now |

What the agenda does, as built:

| `code` | When | A read (a sync, a list) | A queued write |
|---|---|---|---|
| `not_set_up` | no server, workspace or sign-in | says `message`; asks the tracker again at once | waits for `r`; the tracker is asked again |
| `signed_out` | the sign-in was refused (401, a revoked token) | as `not_set_up` | as `not_set_up` |
| `not_found` | the issue isn't there, or you can't see it | that task is marked "not found" | waits for `r`, said on the task |
| `rate_limited` | the tracker said to slow down | that tracker is left alone until `retry_after` (or its next sync) | waits; sent again after `retry_after` |
| `offline` | the request didn't reach the tracker | the sync says it failed | waits; sent again at the tracker's next sync |
| `refused` | the tracker said no (403, a 400 for this change, the write gate) | the sync says it failed | waits for `r`, said on the task |
| `bad_request` | the request wasn't understood -- or this change, now (Linear's `BAD_USER_INPUT` for a state of the issue's old team) | as `refused` | as `refused`: waits for `r`. Never dropped: it may be taken later |
| `unsupported` | a call, capability, field or seam it doesn't serve | the agenda stops asking it | **dropped**, the only code that drops: the task's later changes go on, the agenda stops offering it for that tracker this session, and **the task says so** ("due date kept here -- Fenix doesn't take it"), with `r` to send it again or take the tracker's value back |
| `failed` | anything else | the sync says it failed | waits for `r` |
| `unknown` | no answer came (§3.4), or the extension stopped | as `offline` | **never re-sent by itself**: "may have been made", and `r` asks whether to send it again (with the same `op`) or forget it |

An `Err` that isn't JSON is read as `failed` with the text as its
message, except Fenix's own: `nothing offers <name>` (not installed, or
installed without this call) and `no service <name>` are `unsupported`,
and `the extension stopped: ...` is `unknown`. An extension that had
already stopped answers its stop reason alone, which reads as `failed`.

**Mapping, per tracker.** Class by whatever your API uses to say it --
the HTTP status, or the body when the API puts errors there (GraphQL
errors under any status; Linear's `RATELIMITED` comes with a 400) --
and give `retry_after` in seconds, converted from a reset time if
that's what you get. 401 is `signed_out`; 404 and **410** are
`not_found`; 429, and **GitHub's 403 with `X-RateLimit-Remaining: 0`**
(its primary rate limit) and its secondary-limit 403s, are
`rate_limited`, with `retry_after` from `Retry-After` or
`X-RateLimit-Reset`; other 403s and 4xx are `refused`; **502, 503 and
504** are `offline` (maintenance, a proxy); other 5xx `failed`; a
transport error or timeout `offline`. **Linear's GraphQL errors come
with HTTP 200**: read `errors` (`RATELIMITED` is `rate_limited`,
`AUTHENTICATION_ERROR` `signed_out`, an entity not found `not_found`,
whatever the status).

### 3.4 Timing

A tracker answers every call, `Ok` or `Err`, and answers `offline`
itself when its own request times out (Fenix's HTTP client gives up on
a request after 30 s). The agenda gives up on a read after **90
seconds** and on a write after **180** (a write may make three requests),
checking on its five-second tick; either is `unknown`, and a late answer
is dropped. `create` waits on you filling in a form, so it has no limit.

### 3.5 Versions and capabilities

`seam` is the major version: a change that would make an older agenda
or tracker misread the other is a new major. A tracker lists every major
it speaks in `about.seams`; the agenda picks the highest it shares and
sends that in every request. **`about` is answered whatever `seam` it's
asked with**, so the two can always find out; a tracker answers any
other request whose `seam` it doesn't speak with `unsupported`. A
tracker that shares no major with the agenda is shown as "installed,
but speaks the tracker seam 2 -- update the agenda" (or "update it"),
never as not installed.

Within a major, fields are only added, optional, and each side ignores
fields it doesn't know. A call or a write a tracker may leave out is
behind a **capability** in `about.can`; the agenda never makes a call,
or sends a field, whose capability isn't listed, and hides the keys
that need it.

### 3.6 Who may write: the gate

Fenix 1.5 doesn't tell a tracker who called it (§2), and calling a
service needs no permission. So any installed extension -- one whose
install showed no network and no secrets -- could ask a tracker to
close, edit or comment on your issues as you, and read them. For Jira
that isn't new (`jira.request` has sent any REST request as you since
Jira 1.0); for GitHub, GitLab and Linear the seam would be.

Until Fenix passes the caller's id, **every tracker gates the writes
that come through the seam** (`transition`, `update`, `comment`,
`worklog`, `create`, `remember`), with `fenix_tracker::gate`, so the
code and the words are the same everywhere:

- The gate sits **where every seam write passes in the extension**,
  whichever layer then serves it: Jira's `create` (its form) and
  `remember` (its settings) are served by the extension itself, not its
  client, and are gated the same.
- Before the first write it asks, with a picker: *"An extension asks to
  change issues in Jira for you (Fenix can't tell which; usually the
  Agenda) -- allow?"*, **Allow every extension's changes through Jira
  until Fenix restarts** / **Not now -- refuse them for 10 minutes**. It
  can't say who asks, so it doesn't pretend to; and the allow says what
  it allows.
- Writes that come while it's asking wait for the answer -- at most 50;
  the rest are refused at once. A question unanswered after **60 s**
  lapses: Fenix replaces an open picker without answering it (another
  picker opened, the agenda's own), so the gate can't wait for ever. What
  waited is refused (`refused`, "r on the task asks again"), and the
  next write asks afresh. The tracker sets a timer for the question's
  `deadline` and calls `expire`.
- **"Not now" (or Esc) holds for 10 minutes**: writes in that time are
  refused without a question, so an extension can't ask again and again
  until you give in. Changing `<name>.seam_writes` ends it at once.
- **When Fenix says who calls** (being built; not in 1.5), the tracker
  passes it as the write's `caller`: the question names it ("fenix.agenda
  asks to change issues in Linear for you -- allow?") and an allow is
  kept for that caller alone.
- Its setting `<name>.seam_writes` (`ask`, the default; `always`;
  `never`) says for good. `never` refuses every write.
- **Only writes that arrive through the seam.** The tracker's own page
  -- your own keys, its own forms -- writes as before, ungated: those
  are your acts, made where you are. A tracker routes its page's writes
  around the gate, and the seam's through it, even when both end in the
  same client call.
- **It's a speed bump, not authentication.** In practice the first
  write follows your own move in the agenda, so the question makes
  sense; but once you allow, any installed extension's writes go
  through until Fenix restarts. Reads aren't gated: they would ask at
  every start.

The agenda's own writes reach it like anyone's: the first after
starting Fenix asks.

### 3.7 Writes that may have happened: `op`

Every write request carries `op`, an id of its own, **the same each
time that write is sent** -- a queued change's is kept with it, and a
worklog's is made from its task, day and time entries, never from the
clock. A tracker that can, answers a repeat as it answered the first
and doesn't do it again (`fenix_tracker::Dedupe`, kept while the
tracker's extension runs, so after a restart it can't tell). A repeat
is the same **call** with the same `op` and the same payload: the same
`op` on another call is another write, and on another payload is
refused (`bad_request`). A write still under way isn't remembered yet:
sent again meanwhile, it runs twice. A caller never re-sends a write
whose answer didn't come (`unknown`) by itself: it tells you, and you
choose; a worklog that may have been logged stays out of the next send
until you do.

## 4. The calls

`<id>.tracker.` is left off below: `about` is `fenix.jira.tracker.about`.

### 4.1 `about`: who it is, and whether it's ready

Request: `{"seam": <any>}`. **Cheap**: no network, no prompt, and
activating a tracker must not ask you to sign in -- the agenda's
asking is what starts it. Answer:

```json
{
  "seam": 1,
  "seams": [1],
  "minor": 1,
  "id": "fenix.jira",
  "name": "Jira",
  "ready": true,
  "not_ready": "Jira isn't set up yet -- set its server and token in SPC , (Jira)",
  "can": ["comments", "edit.title", "edit.body", "edit.priority", "edit.due", "edit.assignee", "edit.labels", "comment", "worklog", "create", "remember"],
  "sync_minutes": 10,
  "project_meta": "jira",
  "browse": "https://jira.example.com/browse/{ref}",
  "ref_hint": "PROJ-123"
}
```

| Field | | |
|---|---|---|
| `seam` | required | the version of this answer, the highest of `seams` |
| `seams` | optional, `[seam]` | every major it speaks |
| `minor` | optional, 0 | the minor version |
| `id` | required | its extension's full id; the agenda refuses an answer whose `id` isn't the one it asked |
| `name` | required | what the agenda calls it: "Synced 3 issue(s) with Jira" (a name, §3.1) |
| `ready` | required | signed in and configured; when false, every other call answers `not_set_up` |
| `not_ready` | when not ready | what to do about it, with the keys (a message) |
| `can` | optional, `[]` | its capabilities (§4.6); unknown ones are ignored |
| `sync_minutes` | optional, 10 | how often its linked tasks should be brought up to date; 0 never on a timer; at least 5. Each tracker is synced on its own schedule |
| `project_meta` | optional | the key of Fenix's project metadata (`Here.project.meta`) naming this tracker's project for the project you're in. Fenix 1.5's metadata has only `kind` and the Jira key (`jira`), so only Jira can use it; any other tracker serves `project` instead (§4.7) |
| `browse` | optional | a URL with `{ref}` in it: an issue's address before the first sync has brought its `url`. Only for a tracker whose address is its ref dropped into a template (Jira's `/browse/PROJ-12`); a forge, whose `owner/repo#12` is `/owner/repo/issues/12`, leaves it out -- every issue carries its own `url`, and the agenda uses that first |
| `ref_hint` | optional | an example of a `ref`, for "Link to issue (e.g. PROJ-123)" |
| `blocked` | optional, true | false: it has no Blocked state of its own (GitHub). The agenda then keeps Blocked to itself: a card moved there or out of it sends nothing, and no `local` move needs faking |

### 4.2 The issue

```json
{
  "id": "10201",
  "ref": "PROJ-12",
  "asked": "PROJ-12",
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
| `id` | required | the tracker's **immutable** id for it, opaque, never shown: what a link is kept by. Jira's numeric issue id, GitHub's and GitLab's global issue id (`id`, not `number`/`iid`), Linear's UUID. A tracker with none gives its `ref` (and the agenda takes an empty `id` as the `ref`) |
| `ref` | required | its key as people write it, shown and typed (**every call about an issue a caller already knows is by its `id`**: after GitLab moves an issue, its old `group/project#n` is another issue): Jira `PROJ-12`, GitHub and GitLab `owner/repo#12` (GitLab's group paths included), Linear `ENG-123`. **It may change** -- an issue moved to another project, team or repository, a renamed repository -- and the agenda follows it by `id` |
| `asked` | in `issues`' answer | the item of `refs` this answers, as it was asked (an id, a ref, a URL), so a moved issue is matched to what asked for it |
| `url` | optional | its page, for `y` and `o` |
| `project` | optional | its project, repository or team, as `about.project_meta`'s value would name it |
| `title` | required | one line |
| `body` | optional, `""` | **plain text as a person would read it raw**: Markdown passes through as it is (GitHub, GitLab, Linear), and so does Jira Server's wiki markup, since the agenda writes back what it shows; rich text that isn't readable raw (Jira Cloud's ADF, 6.3) is flattened to paragraphs, list items as `- `, code blocks kept, everything else as its text, and such a tracker doesn't list `edit.body` unless it converts back |
| `body_cut` | optional, false | the body was longer than §3.1 allows |
| `state` | required | `id` the tracker's own (opaque); `name` as people see it; `stage` exactly one of `todo`, `doing`, `done` (anything else is read as `todo`); `column` one of `todo`, `doing`, `blocked`, `done` -- the agenda column the tracker puts it in, which is `stage` unless the tracker knows it's blocked |
| `type` | optional | a name |
| `priority` | optional | `name` the tracker's; `level` one of `urgent`, `high`, `medium`, `low`, or `null` for a priority that means none (Linear's "No priority"). No priority, or a `null` level, is the agenda's Medium |
| `assignee` | optional | `id` what `update`'s `assignee` takes back, `name` shown. **One person**: a tracker with several assignees (GitHub, GitLab) gives the first, and `mine` is true if you're any of them. Setting it replaces them all with that one; `null` clears them |
| `mine` | optional | assigned to the signed-in person; absent when the tracker can't tell |
| `labels` | optional, `[]` | names |
| `due` | optional | `YYYY-MM-DD` |
| `updated` | required | when it last changed, as the tracker writes it (RFC 3339, or Jira's `+hhmm`): only compared for equality |
| `comments` | with `comments` | the most recent, oldest first; `created` as `updated` |

**Stages, per tracker**:

- **Jira**: its status category (`new` `todo`, `indeterminate` `doing`,
  `done` `done`).
- **GitHub**: open is `todo`, closed `done`. It has no Blocked: say
  `blocked: false` in `about`. It has no Doing: offer a `local` move to
  Doing (§4.5), so a card dragged there moves in the agenda and nothing
  is sent; a later sync doesn't move it back, since GitHub's state
  didn't change.
- **GitLab**: as GitHub, unless the project's scoped `workflow::`
  labels say `doing` or `blocked`.
- **Linear**, by its state's type: `triage`, `backlog` and `unstarted`
  `todo`; `started` `doing`; `completed` and `canceled` `done`.

### 4.3 `assigned`: the issues assigned to me

```json
{"seam": 1, "limit": 100, "cursor": null}
```

| Field | | |
|---|---|---|
| `limit` | optional, 50 | 1 to 200: the most it wants; a tracker may answer fewer and give `next` |
| `cursor` | optional | a previous answer's `next`, byte for byte |

Answer: `{"issues": [<issue>...], "next": "100"}`. `next` is absent on
the last page. Open issues only (Jira: status category not Done; GitHub
and GitLab: open; Linear: not completed or canceled), assigned to the
signed-in person, most recently updated first. Offset paging drifts
while issues change, so **a caller drops issues it's already had in the
same listing** (the agenda does, by `id`) and stops at an empty page.

`since` was in the first version and is gone: nothing sent it, and a
tracker would have to turn an instant into its own query's time zone
(Jira reads JQL dates in the searching user's profile zone). It can come
back as a minor.

### 4.4 `issues`: these issues

```json
{"seam": 1, "refs": ["10201", "OPS-3", "https://jira.example.com/browse/OPS-4"]}
```

At most 50; each an issue's `id`, its `ref`, or its URL -- the agenda
asks by `id` once it knows it. Answer: `{"issues": [...], "missing":
["OPS-3"]}`, each issue with `asked` set. An issue not found, or not
visible to you, is in `missing` **exactly as it was asked**, byte for
byte, even one that isn't a ref of yours at all (`"bad ref"`), and the
call still succeeds. **A tracker never answers `not_found` for the whole call when
asked for more than one**: one that can't tell which is missing asks
them one at a time. (The agenda copes anyway: a whole-call `not_found`
of several is asked again one by one.)

### 4.5 Moving it on: `transitions` and `transition`

```json
{"seam": 1, "ref": "10201", "toward": "blocked", "from": {"id": "3", "name": "In Progress", "stage": "doing", "column": "doing"}}
```

| Field | | |
|---|---|---|
| `ref` | required | an id or a ref |
| `toward` | optional | the agenda column a card was moved to: a hint, so a tracker can offer moves that only make sense for it and leave out the rest |
| `from` | optional | the state the agenda last saw, so a tracker needn't fetch the issue |

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
| `id` | required | what `transition` takes back; opaque, may hold spaces (`workflow::In Review`) |
| `name` | required | its name |
| `to` | required | the state it lands in |
| `label` | optional | the picker's row; else `name`, or `name  ->  to.name` |
| `local` | optional, false | it changes nothing in the tracker: the agenda moves the card and sends nothing. **Offer one for each column you don't model** (GitHub's Doing and Blocked) |
| `remember` | with `remember` | a choice the tracker keeps once you pick this move (opaque); the agenda calls `remember` with it at once |
| `note` | optional | said when you pick it |
| `said` | optional | said once it's done; else "moved to `to.name`" |

| The answer's field | | |
|---|---|---|
| `transitions` | required | at most 100; possibly empty |
| `ask` | optional | the tracker wants you to choose, even among one |
| `blocked` | optional | the label of the agenda's own "Blocked" row |
| `why` | optional | why none reaches `toward` |

`transitions` changes nothing. Doing one is `transition`:

```json
{"seam": 1, "ref": "10201", "id": "31", "op": "agenda:1763112000:42"}
```

answered `{"said": "moved to In Progress"}` (`said` optional).

**Who decides to move.** The agenda performs a move only when you picked
it, or moved a card to a column exactly one move reaches; otherwise it
asks, and Esc puts the card back. A card moved to the column the issue
is already in sends nothing. With `ask`, a picker of all; else the moves
whose `to.column` is the column -- none puts the card back with `why`
(and, for Blocked, offers every move), one is done, several are a
picker. `remember` is called the moment you pick a move carrying it.

### 4.6 What a tracker may also offer (capabilities)

| `can` | Calls | What the agenda does with it |
|---|---|---|
| `comments` | issues carry `comments` | a task's Activity shows them |
| `edit.title` | `update` with `title` | a linked task's title goes to the tracker |
| `edit.body` | `update` with `body` | its description |
| `edit.priority` | `update` with `priority`; `priorities` | `p` picks one of the tracker's |
| `edit.due` | `update` with `due` | its due date |
| `edit.assignee` | `update` with `assignee` (or `null`); `people` | `A` |
| `edit.labels` | `update` with `labels` | (nothing yet: for agent tools and pages) |
| `comment` | `comment` | `C`, and posting a note as a comment |
| `worklog` | `worklog` | the week's time sent as worklogs (`W`) |
| `create` | `create` | "Create a new issue from this task" when linking |
| `remember` | `remember` | moves that carry `remember` |

Without one, the agenda keeps that part local: the edit stays on the
task, `p` is the agenda's own priorities, and the keys that would write
to the tracker say it doesn't take that from the agenda. **List only
what you serve**: GitHub has no priority or due date, so it lists
neither, and its titles and bodies still sync.

Every write below carries `op` (§3.7) and is gated (§3.6).

- **`update`** `{"seam": 1, "ref": "10201", "set": {"title": "..."}, "op": "..."}`:
  one field per call, of `title` (a line), `body` (text), `priority`
  (the tracker's `name`), `due` (`YYYY-MM-DD`, or `null` to clear),
  `assignee` (`{"id": "jo1", "name": "Jo"}`, the id from `people`, or
  `null` to unassign), `labels` (every label it should have). A field
  it doesn't know or doesn't serve is `unsupported`. Answer `{}`, or
  `{"said": "..."}`.
- **`priorities`** `{"seam": 1, "ref": "10201"}` →
  `{"priorities": [{"name": "Major", "level": "high"}]}`, at most 100.
- **`people`** `{"seam": 1, "ref": "10201"}` → `{"people": [{"id":
  "jo1", "name": "Jo"}], "none": "no people to assign to -- ..."}`, at
  most 500.
- **`comment`** `{"seam": 1, "ref": "10201", "body": "...", "op": "..."}`,
  a body of at most 64 KiB → `{}`. Posting only.
- **`worklog`** `{"seam": 1, "ref": "10201", "seconds": 4500, "started":
  "2026-11-12T09:15:00.000+01:00", "op": "..."}`: time worked, filed on
  the day it started (RFC 3339) → `{}`.
- **`create`** `{"seam": 1, "title": "...", "body": "...", "op": "..."}`:
  the tracker's own new-issue form, opened and filled in. Either held
  until you're done -- answered `{"ref": "PROJ-13"}` once you've made
  it, `{"ref": null}` if you closed it (Jira, as before, leaves a closed
  form unanswered), no time limit -- or answered **at once** with
  `{"ref": null, "opened": "Opened a new issue in your browser -- I
  links it once it's made"}` (a message), for a tracker whose form is a
  web page or its own page and that doesn't want to hold the call. The
  agenda says `opened`.
- **`remember`** `{"seam": 1, "ref": "10201", "remember": "...", "op": "..."}`
  → `{}`: keeps a choice a move carried.

### 4.7 `project`: the project you're in

`{"seam": 1}` → `{"project": "ENG"}`, behind the capability `project`:
the Fenix project you're in (the one the tracker's extension runs in;
Fenix starts an extension again in each project) as this tracker's
issues name their `project`, or `null` when it can't tell. How is the
tracker's own: GitHub and GitLab from the git remote (`owner/repo`),
Linear from a setting of its own a project may set (`linear.team`,
`project = true`), Jira Cloud as Server, from `project_meta`. The agenda
asks it when its page opens, of every ready tracker that lists the
capability; a tracker that can't tell (or doesn't serve it) puts none
of its issues in "this project only" (`P`), where tasks still count by
category.

### 4.8 What the agenda serves back

Trackers may call these, as Jira's page does. `tracker` is the full
extension id; a call without it is from Jira 1.2 or the notebook, and
means `fenix.jira` (`agenda.add_issue`) or any tracker
(`agenda.open_task`, matched by ref).

| Service | Payload | |
|---|---|---|
| `agenda.add_issue` | `{"tracker": "fenix.jira", "ref": "PROJ-12", "clock": false}` | the issue as a task (and its clock started) |
| `agenda.open_task` | `{"tracker": "fenix.jira", "ref": "PROJ-12"}` | its task's page; `Err` when it isn't in the agenda |
| `agenda.linked` | `{"tracker": "fenix.jira"}` | the refs it has linked, as a JSON array; every tracker's without `tracker` |
| `agenda.sync_quietly` | `null` | linked tasks brought up to date now, quietly |
| `agenda.tracker_changed` | `{"tracker": "fenix.jira"}` | ask it again: its settings or sign-in changed. Fire and forget; an agenda 1.2 answers `nothing offers`, which is fine |

`key` is still read in place of `ref`, as Jira 1.2 and the notebook
send it.

## 5. Several trackers at once

- **Keyed by both.** A linked task stores its tracker's id and the
  issue's `id` (and its `ref`, refreshed whenever an answer brings a new
  one); two trackers' `ENG-12` are two issues. A ref given without a
  tracker (the notebook's links) is looked up in every tracker, the first
  match winning.
- **Labelled when it matters.** Where issues from several trackers are
  listed together (the import and link pickers), each row ends with its
  tracker's name; with one tracker, nothing is added.
- **Each on its own schedule.** Every tracker is synced every
  `sync_minutes` of its own, left alone until a `retry_after` has
  passed, and its queued changes are sent whatever another tracker's
  sync did. A sync you ask for asks every ready tracker at once and
  says what happened in one notification when the last has answered:
  "Synced 5 issue(s) with Jira and Linear", or "Linear sync failed" with
  each failure and what did sync.
- **The outbox is one queue**, sent one change at a time in the order
  made, a tracker that isn't ready (or is backing off) skipped, so a
  title change never overtakes a move made before it to the same
  tracker.
- **A tracker gone** (uninstalled) leaves its tasks linked, and their
  changes waiting; the task says its tracker isn't installed.
- **A key typed into a new task's title** (`PROJ-12`) is asked of every
  ready tracker and linked to the first that has it.

## 6. Jira on the seam

`fenix.jira` 1.3 serves `fenix.jira.tracker.*` for Server and Data
Center, from `crates/jira`'s `tracker` module, so the Jira extension and
the agenda's fallback (§7) run the same code:

- **`about`**: `id` `fenix.jira`; `ready` when `jira.base_url` and
  `jira.token` are set; `not_ready` the agenda's old words;
  `sync_minutes` is `jira.sync_minutes`; `can` all of them;
  `project_meta` `jira`; `browse` `<base>/browse/{ref}`. It tells the
  agenda when a `jira.*` setting changes (`agenda.tracker_changed`), and
  forgets who you are and its Flagged field then too, and whenever the
  server's address changes.
- **Issues**: `id` is Jira's numeric id. One ref asked alone is fetched
  directly (`GET issue/{idOrKey}`, which also answers a moved issue
  under its new key); several in one search,
  `id IN (...) OR key IN (...)`, with **`validateQuery=false`** so a
  deleted key is left out rather than failing the search; and should a
  server still answer 400, each is fetched alone, a 404 put in
  `missing`. (Jira's answer to a missing key in `key IN` is a 400 from
  memory, unverified; both are handled.) `column` from `jira.blocked`
  and the status category; `level` from `jira.priorities`, else the
  guess from the name; `mine` from the assignee's username against
  `/myself`.
- **`assigned`**: the agenda's JQL as it was,
  `assignee = currentUser() AND statusCategory != Done [AND project IN (...)] ORDER BY updated DESC`,
  paged by `startAt`.
- **Moves**: the workflow's transitions, each `to.column` worked out as
  for an issue. Toward Blocked, `jira.blocked` decides: its status (one
  move, or none with `why`), Flagged (a `flag` move), local (a `local`
  move), or not decided yet (an `ask` of every reachable status, Flagged
  and local, each with `remember`). Out of Blocked while Flagged, an
  `unflag` move, or `unflag+<id>`.
- **Writes**: the same REST calls as before, plus unassigning
  (`{"name": null}`) and labels; gated (§3.6, `jira.seam_writes`); each
  `op` remembered for the session (§3.7). Refs are checked (§3.1).
- **Errors**: Jira's own words, classed by status: 401 `signed_out`, 404
  and 410 `not_found`, 429 `rate_limited`, 502/503/504 `offline`, other
  4xx `refused`, other 5xx `failed`, no answer `offline`, no server or
  token `not_set_up`.

**`jira.request` is deprecated.** It's still served, unchanged and
ungated, for the agenda 1.2: it sends any REST request it's given to
your Jira as you, for any extension that asks -- a much wider door than
the seam's gated writes. It goes, with `jira.config`, `jira.set_blocked`
and `jira.new_issue`, at Jira 2.0.

## 7. Updating one at a time

| Agenda | Jira | How |
|---|---|---|
| 1.2 | 1.2 | as before |
| 1.2 | 1.3 | Jira 1.3 still serves `jira.request`, `jira.config`, `jira.set_blocked`, `jira.new_issue`, unchanged |
| 1.3 | 1.2 | the agenda asks `fenix.jira.tracker.about`, is told nothing offers it, asks `jira.config`, and -- answered -- serves the seam to itself with `crates/jira`'s `tracker` module over `jira.request` (`extensions/agenda/src/legacy.rs`); `remember` is `jira.set_blocked`, `create` is `jira.new_issue`. Ungated, as `jira.request` is |
| 1.3 | 1.3 | the seam |

**`agenda.json`**: a Jira link stays under `"jira"` with 1.2's fields
(plus `tracker`, `id`, `url`, `project`, `column`, `description_cut`,
`transient`, `unknown` that 1.2 ignores), so going back to 1.2 keeps
your Jira links. **Every other tracker's link is kept under `"link"`**,
which 1.2 ignores: it never mistakes a Linear `ENG-12` for Jira's
(checked with agenda 1.2's own model, `scratchpad/probe-seam/agenda12`).
A link without `tracker`, or with 1.3's early `jira`, is `fenix.jira`'s;
a flagged one gets the column `blocked`; an id is learned at the first
sync.

**Agenda 1.2 drops what it doesn't know when it saves** (shown with
1.2's own model): a non-Jira link would be gone after a round trip
through 1.2, its queued changes orphaned. So 1.3 also keeps every
non-Jira link in a file of its own beside it, `agenda-links.json`, by
task id and when the task was made; loading `agenda.json`, a task that
has lost its link gets it back from there (a task 1.2 deleted, or one
made since under the same id, doesn't). Its queued changes, which 1.2
keeps, then go as before.

**When the old path goes.** Together, in the next major of each (agenda
2.0, Jira 2.0; the bundle's ranges move to `^2`): the agenda drops
`legacy.rs` and `fenix-jira-client`, and Jira stops serving the four
old services -- `jira.request` first of all.

## 8. Tests

- `crates/tracker`: cleaning (invisible and tag characters), opaque
  values kept whole, caps, errors read from every form, the gate, the
  dedupe, versions, and `FakeTracker`.
- `crates/jira`: the seam served against recorded Jira responses
  (`tests/fixtures`), each call; refs checked; moved and deleted issues;
  `validateQuery` and its 400 fallback; errors classed; no token in an
  answer.
- `extensions/jira`, on the test kit: the four old services as before;
  `about`; the gate (asked once, `always`, `never`, "Not now"); a
  repeated `op` done once; `remember`; forgetting on a settings change.
- **`fenix_agenda::testing`** (the agenda's feature `testing`) is the
  agenda's own test world, public and documented: `world_with` starts
  the agenda on the test kit's fake host with any services you wire
  (`serve` puts a `FakeTracker` behind `<id>.tracker.*`; wire your own
  serving function the same way), in a folder of the test's own, and
  `World` drives it. A tracker adds `fenix-agenda = { path =
  "../agenda", features = ["testing"] }` to its dev-dependencies and runs
  the agenda against its own answers from its own crate.
- `extensions/agenda`, on the test kit: `FakeTracker` behind
  `acme.fake.tracker.*` through import with paging and dedupe, one
  issue, a moved issue, sync, moves, each error code's handling, an
  unanswered write never re-sent, an unsupported field dropped without
  stalling, readiness right after setting up, per-tracker schedules and
  back-off, versions, an impostor's `id`, two trackers (one failing),
  untrusted text; Jira's recorded responses through the old path and
  the seam compared; both mixed-version pairs; where links are stored.

## 9. For the next tracker: a checklist

Complete, for Jira Cloud, GitHub Issues, GitLab Issues and Linear.

**Names and discovery**
- [ ] Serve `<your full id>.tracker.about`, `.assigned`, `.issues`,
      `.transitions`, `.transition`, and the writes you take, listed in
      `contributes.services`; make the names with `fenix_tracker::service`.
      Add your id to `fenix_tracker::KNOWN` in the same pull request.
- [ ] `about` answers whatever `seam` it's asked with: `id` your full
      id, `seams` from `fenix_tracker::SEAMS`. No network, no prompt;
      activating doesn't ask for a sign-in.
- [ ] `ready: false`, with `not_ready` naming your settings' keys, until
      signed in; every other call `not_set_up` then.
- [ ] Call `agenda.tracker_changed` (fire and forget) when your settings
      or sign-in change, and forget what you learned of the old server.

**Issues**
- [ ] `id` your tracker's immutable id; `ref` as people write it; build
      with `fenix_tracker::Issue`, send `.cleaned()`.
- [ ] `issues` takes ids, refs and URLs, sets `asked`, puts what it
      didn't find in `missing`, and never answers a whole-call
      `not_found` for several.
- [ ] `stage` exactly `todo`/`doing`/`done` per §4.2's mapping; `column`
      `blocked` only when you know it is.
- [ ] `assigned` paged with your own cursor, passed back byte for byte.

**Moves**
- [ ] Your moves as a list; `local` moves for the columns you don't
      model; `transitions` changes nothing.

**Writes**
- [ ] List per field what you serve (`edit.title`, ...); answer an
      unknown or unserved field `unsupported`.
- [ ] Gate every write with `fenix_tracker::gate::Gate` and a
      `<name>.seam_writes` choice setting (`ask`, `always`, `never`).
- [ ] Keep `op`s with `fenix_tracker::Dedupe`, and use a tracker-side
      idempotency key where there is one (Linear's create takes an id).
- [ ] Check every ref or id against your own form before it reaches a
      URL; percent-encode path segments.

**Errors**
- [ ] Class errors per §3.3's mapping, your own words in `message`,
      `retry_after` from the headers; 502/503/504 `offline`.

**Safety and tests**
- [ ] Hold your own token; nothing of it in an answer; `network` for
      your hosts only.
- [ ] Tests on recorded responses, and the agenda driven by your own
      answers with `fenix_agenda::testing` (§8).

**Projects, Blocked, create**
- [ ] Without a Fenix metadata key, serve `project` (§4.7): from the
      git remote, or a project-settable setting of your own.
- [ ] No Blocked of your own: `blocked: false` in `about`.
- [ ] A form that's a web page: answer `create` at once with `opened`.

What each of the four needs beyond that:

- **Jira Cloud** (6.3), in `fenix.jira`: sign-in by email and API token
  (or OAuth); REST v3, whose description and comments are ADF,
  flattened per §4.2, `edit.body` left out unless converted back;
  `assignee.id` an `accountId`, unassign with `accountId: null`;
  `validateQuery` is `strict|warn|none` on Cloud (`warn`); its search
  pages by `nextPageToken` on the newer endpoint.
- **GitHub Issues** (`fenix.github-issues`): `ref` `owner/repo#n`, `id`
  the issue's global `id`; `crates/forge` and `github.token`;
  `assigned` is `GET /issues?filter=assigned&state=open` (pull requests
  left out), paged by `Link`; moves *Close as completed*, *Close as not
  planned* (`state_reason`), *Reopen*, and `local` moves to Doing and
  Blocked; `edit.title`, `edit.body`, `edit.assignee`, `edit.labels`,
  `comment` -- no priority, due, worklog or `remember`; 403 with
  `X-RateLimit-Remaining: 0` is `rate_limited`.
- **GitLab Issues** (`fenix.gitlab-issues`): `ref` `group/project#iid`,
  `id` the global `id`; `crates/forge` and `gitlab.token`; `assigned`
  is `GET /issues?scope=assigned_to_me&state=opened`, paged by
  `X-Next-Page`; moves *Close*, *Reopen*, a move per scoped
  `workflow::` label (ids with spaces), `local` moves otherwise;
  `edit.due` from `due_date`; `worklog` from `/add_spent_time` if
  wanted.
- **Linear** (`fenix.linear`): `ref` the identifier (`ENG-123`), `id`
  the UUID (the identifier changes when an issue changes team); its
  GraphQL API and its own token; `assigned` is
  `viewer.assignedIssues`, paged by `pageInfo.endCursor`; one move per
  workflow state of the issue's team; stages per §4.2; `priority` from
  0 to 4 (`level` urgent, high, medium, low; 0 none); errors read from
  `errors` under HTTP 200; `create` with a client-chosen `id` from `op`.

## As built

Recorded after building, and after the review, on 14 November 2026.

- `crates/tracker` (`fenix-tracker` 1.0.0): the shapes (`seam`), the
  cleaning (`clean`), errors (`SeamError`), the write gate (`gate`), the
  op memory (`Dedupe`) and `FakeTracker`.
- `crates/jira` 1.1.0: `tracker`, the seam served over a `JiraClient`;
  paged and id searches; labels; refs checked; recorded responses in
  `tests/fixtures`.
- `fenix.jira` 1.3.0 serves it, gated; `fenix.agenda` 1.3.0 calls it,
  with the Jira 1.2 fallback in `legacy.rs`. The Jira bundle is
  unchanged.
- `fenix_agenda::testing` (feature `testing`): checked from a crate of
  its own that links another extension (`fenix-jira`) beside the agenda
  and drives the agenda with a `FakeTracker` -- the two extensions'
  exports don't clash.
- The agenda's service calls wait in `calls.rs`, its own small executor
  over `call_service` and `ServiceReply`, so the test kit answers them.

**Not quite as before**, for Jira users:

- A description's `\r\n` line ends arrive as `\n`. A base kept by 1.2
  compares as cleaned, so the first sync after updating sees no change.
- A deleted linked issue marks its task "not found" instead of failing
  the sync -- with one linked issue or many.
- **The first change sent to Jira after starting Fenix asks whether to
  allow the agenda to change issues for this session** (the write gate,
  §3.6); `jira.seam_writes = always` makes it as before.
- A change whose answer never came (a slow Jira, past 180 s) isn't sent
  again by the next sync: the task says it may have been made, and `r`
  asks.
- A change Jira can't take is dropped with a message instead of holding
  up the task's later changes.
- 502, 503 and 504 from Jira (maintenance) are retried by the next sync
  rather than waiting for `r`.
- Out of Blocked while Flagged: the flag cleared and the transition are
  one move, said once; when no transition reaches the column, nothing
  is sent and the card goes back (before, the flag was cleared anyway).
- A Flagged or local Blocked move asks Jira once, which answers without
  a request, so the card shows ↑ for that moment.
- Changing `jira.blocked` re-files a linked card at the next sync.
- Comments past the 100 most recent, and bodies past 64 KiB, aren't
  kept; such a description isn't edited from the agenda. Invisible
  characters in titles and names are removed.
- Jira's sync runs on `jira.sync_minutes` but never more often than
  every 5 minutes; its first timed sync is shortly after Fenix starts,
  where 1.2's was `jira.sync_minutes` after.
- `A` on a linked task also offers "Nobody (unassign)".
- Words: with no tracker installed, "that needs a tracker extension,
  such as Jira"; the commands' titles say "your tracker".
- Requests: the search asks for `labels` too, with `validateQuery=false`,
  by `id` once the agenda knows it; an import longer than Jira's page
  size asks for the rest with `startAt`.
