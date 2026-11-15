# Changelog

## 1.0.0

The first release, for Fenix 1.5 (extension API 1.5): GitHub's issues in
Fenix.

- Its page (`SPC g i`): your searches -- assigned to you, created by
  you, mentioning you, the repository you're in, the repositories and
  searches you save, and one you type in GitHub's own syntax -- an issue
  with its comments, labels, assignees, milestone and linked pull
  requests, comments and edits, its moves (close as completed, close as
  not planned, reopen), and a new-issue form with the
  repository's templates.
- A branch started from an issue (`b`), named from it, in the project
  you're in when it's a clone of the issue's repository.
- The agenda's issues from GitHub, through the tracker seam: linked
  tasks kept in step, their moves, edits, comments and assignees.
- Changes the agenda (or any extension) asks for through the seam are
  asked about once a session first (`github-issues.seam_writes`); what
  you do on its page never is.
- A branch from an issue takes nothing from a remote as an option to
  git, fetches only from the issue's own remote, and never over
  `file://` or `ext::`.
- A change sent through the seam goes to the issue where it is now
  (moved or transferred), never by following a redirect, and a timeout
  is tried again rather than dropped.
- On a GitHub Enterprise Server it signs in with the GitHub CLI's token
  for that host only: Fenix's `github.token` is github.com's, and is
  never sent elsewhere.
- Two agent tools that read: an issue with its comments, and your open
  issues.
- Signed in as Review is: Fenix's GitHub token, or the GitHub CLI's.
  GitHub Enterprise Server too.
