# Changelog

## 1.2.0

For Fenix 1.5 (extension API 1.5):

- What it says names the keys of the key scheme in use, and yours when
  you've bound your own -- Home's today, the empty page's "`C-k A i`
  brings in your Jira issues" with standard keys (`SPC A i` with Vim
  keys), and its messages, the Jira page's and Fenix's settings' and
  extensions' keys among them.
- A Jira sync ends in a notification, not the message line: how many
  issues it brought up to date, why it failed, or the conflicts it found,
  with the agenda a key away. A newer one replaces the last.
- The README gives standard keys beside Vim's.

## 1.1.0

- `agenda.open`, `agenda.board`, `agenda.list`, `agenda.report`, `agenda.find` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: tasks on a board, a clock for your time, and a week's timesheet -- linked to Jira issues when Jira is there.
