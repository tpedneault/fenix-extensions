# Changelog

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
