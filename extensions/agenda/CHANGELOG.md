# Changelog

## 1.3.0

If you use Jira, your linked tasks, their moves, what Blocked means in
each project, comments, worklogs and the new-issue form work as they
did, with the Jira extension 1.2 or 1.3 -- except that, with Jira 1.3,
the first change asks you to allow it (below).

- The agenda now talks to Jira the way it will talk to every tracker,
  so GitHub Issues, GitLab Issues and Linear can link your tasks too
  once their extensions are out, several at once. Where issues from two
  trackers are listed together, each says whose it is.
- `agenda.trackers` names trackers built elsewhere, for the agenda to
  ask too.
- What a tracker sends is kept as it sent it, byte for byte, capped in
  size, and shown cleaned of control characters, bidirectional overrides
  and invisible characters -- keeping the joiners and marks Persian,
  Hebrew, Arabic, Hindi and emoji need. A title or description too long
  to keep isn't written back over the tracker's.
- A linked task whose issue was deleted is marked "not found" instead of
  failing the sync; one whose issue moved follows it to its new key.
- A change whose answer never came (a slow tracker) isn't sent again by
  itself, so a comment or a worklog can't go twice: the task says it may
  have been made, and `r` asks whether to send it again, with the same
  `op`. A worklog with no answer is marked "may have been logged": `W`
  leaves it out, `s` in the review sends it again with the same `op`
  (made of the task, the day and its time, never the clock), `x` says it
  was logged.
- A change the tracker will never take (it has no such field) leaves
  the queue, so the task's other changes still go, and marks the task
  "not taken" until `r` sends it again, takes the tracker's value back,
  or keeps yours here only. One it refused for now (a value it didn't
  accept) waits for `r`.
- Changes, moves, priorities and people go to an issue by its id, so an
  issue that moved since the last sync still gets them.
- Each tracker has its own queue: one slow to answer holds up only its
  own changes; a tracker that's ready is used without waiting for a slow
  one to say who it is; a key you type is asked of every tracker at once.
- Links to trackers other than Jira survive agenda 1.2 saving
  `agenda.json` (which drops them): they're kept in `agenda-links.json`
  too, and put back.
- The first change it sends Jira (or another tracker), the tracker asks
  you to allow -- Fenix doesn't tell it who's asking, so it says an
  extension asks; allowing lets changes through until Fenix restarts,
  and "not now" refuses them for 10 minutes. `jira.seam_writes = always`
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
