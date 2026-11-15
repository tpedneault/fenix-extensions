# Changelog

## 1.0.0

The first release, for Fenix 1.5 (extension API 1.5): your Linear issues on a page, in your agenda, and for coding agents.

- **The page**, `SPC o w w`: your issues, the issues you made, each team's active cycle and backlog, and a typed filter (`team:ENG state:started assignee:me label:bug login`). An issue opens on its own page with its state, priority, assignee, labels, cycle, project, parent, sub-issues, description and comments.
- **Changing an issue**, each a choice you make: `s` moves it to any of its team's workflow states, `p` priority, `A` assign, `u` due date, `e` title, `E` description, `C` comment, and `n` a new issue with its team, title, description, priority, state, assignee and labels. Nothing is sent until you pick or send it.
- **A branch from an issue**, `b`: Linear's own suggested branch name for it (or your pattern, in `linear.branch_pattern`), made and switched to in the project you're in. A tree with changes is asked about first; a branch that exists is switched to, and not over changes; outside a git repository it says so and copies the name. `y` copies the address, `Y` the identifier, `B` the branch name.
- **The agenda** can import your Linear issues, link a task to one, keep it in step, move it between Linear's states, comment on it, set its priority, due date and assignee, and open Linear's new-issue form from a task.
- **Coding agents** get two read-only tools: `linear_read_issue` (an issue with its latest comments) and `linear_list_mine`. What other people wrote in Linear comes marked as data, cleaned and capped.
- **Changes from the agenda are asked about** once a session (`linear.seam_writes`: ask, always, never); your own changes on the page never are.
- **Your API key** is kept in the system's keychain (`linear.token`) and sent only to `api.linear.app`, in the `Authorization` header. OAuth isn't supported: it needs a client secret registered with Linear.
- **A branch name can never be read as an option by `git`**: every call that holds it puts it after `refs/heads/`, after `--end-of-options`, or as the value of `-c` (README lists each `git` run exactly).
- **A change Linear refuses for what's true now** -- a state, person or label that's gone, an archived issue, a plan limit -- is told to you and waits for `r`, instead of being dropped by the agenda as never acceptable; a lock timeout counts as offline and is tried again. Only a request Linear can never accept is dropped.
- **`linear.team`** (the one setting a project may set) is held to a team key: anything else answers "no team", and it is never sent to Linear or to a program.
