# Changelog

## 1.3.0

If you use Jira, your linked tasks, their moves, what Blocked means in
each project, comments, worklogs and the new-issue form work as they
did, with the Jira extension 1.2 or 1.3 -- except that, with Jira 1.3,
the first change of a session asks you to allow it (below).

- The agenda now talks to Jira the way it will talk to every tracker,
  so GitHub Issues, GitLab Issues and Linear can link your tasks too
  once their extensions are out, several at once. Where issues from two
  trackers are listed together, each says whose it is.
- `agenda.trackers` names trackers built elsewhere, for the agenda to
  ask too.
- What a tracker sends is shown as text, cleaned of control characters
  and capped in size; a description too long to keep isn't written back
  over the tracker's.
- A linked task whose issue was deleted is marked "not found" instead of
  failing the sync; one whose issue moved follows it to its new key.
- A change whose answer never came (a slow tracker) isn't sent again by
  itself, so a comment or a worklog can't go twice: the task says it may
  have been made, and `r` asks whether to send it again.
- A change the tracker doesn't take is dropped with a message, and the
  task's other changes still go.
- The first change it sends Jira in a session, Jira asks you to allow
  (Fenix doesn't tell Jira who's asking); `jira.seam_writes = always`
  stops it asking.
- A tracker you've just set up is ready at once; the agenda only wakes
  the trackers your tasks are linked to when Fenix starts.
- `A` on a linked task offers "Nobody (unassign)".
- Command names say "your tracker" where they said Jira.

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
