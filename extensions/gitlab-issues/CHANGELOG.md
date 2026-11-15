# Changelog

## 1.0.0

The first release, for Fenix 1.5 (extension API 1.5): GitLab's issues in
Fenix, on gitlab.com or your own instance.

- Its page (`SPC g I`): your searches -- assigned to you, created by
  you, mentioning you (your to-do items), the project you're in, the
  projects and searches you save, and a filter you type -- an issue with
  its comments, labels, assignees, milestone, due date, weight, time
  tracked and related merge requests, comments and edits, its moves
  (close, reopen, the project's `workflow::` steps),
  and a new-issue form with the project's templates.
- A branch started from an issue (`b`), named from it, in the project
  you're in when it's a clone of the issue's project.
- The agenda's issues from GitLab, through the tracker seam: linked
  tasks kept in step, their moves, edits, due dates, comments, assignees
  and, when you turn it on, the time you log.
- Changes the agenda (or any extension) asks for through the seam are
  asked about once a session first (`gitlab-issues.seam_writes`); what
  you do on its page never is.
- A branch from an issue takes nothing from a remote as an option to
  git, fetches only from the issue's own remote, and never over
  `file://` or `ext::`.
- A change sent through the seam goes to the issue where it is now
  (moved or transferred), never by following a redirect, and a timeout
  is tried again rather than dropped.
- Two agent tools that read: an issue with its comments, and your open
  issues.
- Signed in as Review is: Fenix's GitLab server and token.
