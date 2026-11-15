# Changelog

## 1.4.0

Jira Cloud, in the same extension. Nothing changes for Server and Data
Center.

- Set the address of your `*.atlassian.net` site, your Atlassian
  account's email (the new **Jira email** setting) and an API token, and
  the page, the new-issue form and the agenda work with Jira Cloud. The
  README says where to make the token, step by step.
- Which Jira it is comes from the address, and the site itself says for
  sure the first time it's reached; the page shows it beside the
  address. **Which Jira** (`jira.kind`) says it yourself.
- A scoped API token works too, through Atlassian's gateway: turn on
  **Scoped API token**. So it may reach `api.atlassian.com` as well as
  your site.
- On Cloud, descriptions and comments are shown as plain text, and a
  description isn't edited from Fenix -- its rich text would be lost.
  Everything else the agenda sends goes to Jira Cloud as to Server:
  moves, title, priority, due date, assignee (or nobody), labels,
  comments and worklogs. Comments and new issues are sent as paragraphs
  and line breaks.
- The agenda's changes to a Cloud site are asked about once a session,
  as for Server (`jira.seam_writes`), and a change sent twice is made
  once.
- When Jira says to slow down, the agenda is told how long to wait.
- The question before the agenda's changes says what it can know: an
  extension asks (Fenix can't tell which; usually the Agenda), and
  allowing lets every extension's changes through until Fenix restarts.
  Not now (or Esc) refuses for 10 minutes without asking again; a
  question left unanswered for a minute, or replaced by another picker,
  refuses what waited, and the next change asks again. At most 50
  changes wait.
- A change is known by its call and `op` together: a move sent with the
  `op` of an earlier comment is still made.
- `jira.request` (the agenda 1.2's) is refused for Jira Cloud.
- **Changed:** your token isn't sent over `http://`. Jira Cloud is
  https only; for Server and Data Center, turn on the new **Allow a Server over http**
  setting (`jira.allow_http`) to send it in clear anyway.
- Issue text reaches the agenda as Jira has it, byte for byte, so what
  is written back is Jira's own; the agenda cleans it to show it.

## 1.3.0

Nothing changes for you: the agenda's linked tasks, their moves, what
Blocked means in each project, comments and worklogs work as they did.

- It serves the agenda the tracker seam, the way every tracker will, so
  the agenda knows nothing of Jira itself any more. The agenda 1.2 keeps
  working with it as it did.
- The first change the agenda sends in a session asks whether to allow
  it: Fenix doesn't say which extension is asking, so Jira asks you.
  `jira.seam_writes` (ask, always, never) says it for good.
- A deleted issue no longer fails the agenda's whole sync: its task is
  marked not found. A moved issue is followed to its new key.
- Jira's maintenance pages (502, 503, 504) are retried by the next sync.
- Changing Jira's server forgets who you were on the old one.
- `jira.request`, which the agenda 1.2 uses, is deprecated: it lets any
  extension send any request to your Jira as you, without asking. It
  goes at 2.0, with the agenda 1.2's other three services.

## 1.2.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme in use, and yours where you rebound them -- before Jira is
  set up, the page and its messages point to the settings page by its
  keys, `C-,` with standard keys and `SPC ,` with Vim keys, and the page
  says it again when you switch.

## 1.1.0

- `jira.open`, `jira.search`, `jira.goto_issue` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: your Jira Server or Data Center: searches, issues and their workflow, a new-issue form with the project's real fields -- and the agenda kept in step with it.
