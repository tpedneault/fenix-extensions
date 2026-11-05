# Changelog

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
