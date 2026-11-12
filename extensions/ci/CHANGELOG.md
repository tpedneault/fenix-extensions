# Changelog

## 1.2.0

- `ci.open` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

- Jenkins: name your server and the project's job (`ci.jenkins_url`, `ci.jenkins_job`) and the branch's builds are on the modeline and the page as GitHub's runs and GitLab's pipelines are -- a multibranch pipeline's by the branch you're on, any other job's by the branch Git built. A build opens on its stages, a failed stage on its steps, and `Enter` on its part of the console; `r` builds the job again, `c` stops the build. Notifications and the agents' `ci_failures` tool work as they do for the others.

## 1.0.0

- The first release, for Fenix 1.2 (API 1.3): the branch's GitHub Actions runs or GitLab pipelines on the modeline, a page of their jobs, steps and logs with re-runs and cancels, notifications when they finish, and the failing jobs' logs as a tool for coding agents.
