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
  Comments and new issues are sent as paragraphs and line breaks. The
  agenda keeps title, priority and due date changes on the task for a
  Cloud issue; moves, comments, worklogs and assignees go to Jira.
- When Jira says to slow down, the agenda is told how long to wait.

## 1.3.0

Nothing changes for you: the agenda's linked tasks, their moves, what
Blocked means in each project, comments and worklogs work as they did.

- It serves the agenda the tracker seam, the way every tracker will, so
  the agenda knows nothing of Jira itself any more. The agenda 1.2 keeps
  working with it as it did.
- Its search for the agenda pages through more than one screen of
  issues, and an issue's labels come with it.

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
