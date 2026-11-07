# Changelog

## 1.2.0

For Fenix 1.3 (extension API 1.4):

- `A` in the box for your review's summary (`s`, `e`), in Normal mode while
  the box is empty, has your coding agent write it, going by the request,
  its diff and the comments you're holding. It's yours to edit before
  `S S`. With text in the box, `A` is Vim's.
- `A` in a new request's description box does the same, from the branch's
  change and commits, in the repository's template. It writes over the
  template while that's untouched.
- `A` on the review page is gone, and with it the agent's summary at the
  top of the page. So is `a` on a new request: the agent no longer writes
  the title.

## 1.1.1

Built against Fenix v1.1.0's released extension API, rather than the
branch it was on before the release. Nothing changes in what it does.

## 1.1.0

For Fenix 1.1 (extension API 1.2):

- `a` on a new request has your coding agent draft its title and
  description from the branch's change and commits, in the repository's
  template, for you to edit before you open it.
- `A` on a review has your coding agent summarize the request: what it
  changes, what's risky, which files to read first. It's asked once per
  head while Fenix runs.

## 1.0.0

- The first release, for Fenix 1.0: code review on GitHub and GitLab: the reviews waiting, a request with its files, threads, checks and your held comments, submitted with a verdict -- and a new pull request for the branch.
