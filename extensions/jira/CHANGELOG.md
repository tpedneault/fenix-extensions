# Changelog

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
