# Jira

Your Jira Server or Data Center in Fenix. Its page (`SPC j j`) lists your
searches on the left -- assigned to you, reported by you, watching,
recently updated, each tracked project's open issues and sprint, each
tracked person's, and your saved JQL -- with the selected search's issues
in the middle and a preview on the right.

`SPC j g` opens an issue by key, `SPC j /` searches (words, or JQL after
`:`), and `SPC j n` makes an issue with the project's real types and
fields. With standard keys, `SPC` is `Ctrl+K`: `Ctrl+K j j`.

It needs Fenix 1.5 or later (extension API 1.5). What it says names the
keys of the key scheme you use, and yours where you rebound them: before
it's set up, the page and its messages point to the settings page by its
keys -- `C-,` with standard keys, `SPC ,` with Vim keys.

## An issue's keys

| Key | What it does |
| --- | --- |
| `s` | Move it through its workflow |
| `p` `A` `u` | Priority, assignee, due date |
| `e` `E` | Title, description |
| `C` | Comment |
| `T` | Log time |
| `a` `t` | Add it to the agenda (and start the clock) |
| `y` `o` | Copy its link, open it in the browser |

## Setting it up

Its server is `jira.base_url` and its token `jira.token`, both on the
settings page (`SPC ,`, or `Ctrl+,` with standard keys); `FENIX_JIRA_TOKEN` overrides the token. The
token is a personal access token, sent as a bearer token.

| Setting | What it does |
| --- | --- |
| `jira.projects` | Projects whose open issues and sprint are listed |
| `jira.users` | People whose issues are listed |
| `jira.queries` | Saved searches |
| `jira.blocked` | Per project, what Blocked means: a status, the Flagged field, or nothing |
| `jira.priorities` | How Jira's priorities map onto the agenda's |
| `jira.sync_minutes` | How often the agenda's linked tasks are brought up to date (every 5 minutes at most) |
| `jira.seam_writes` | Whether the agenda may change your issues: `ask` once a session (the default), `always`, or `never` |

## What it may do

- Reach your Jira, the server in `jira.base_url`, and nothing else.
- Read and keep your token, `jira.token`, in the system's keychain.
- Read where you are in the editor, for the project's Jira key in
  `.fenix/project.toml`.

## For the agenda

The agenda reaches Jira through the tracker seam
([`plans/tracker-seam.md`](../../plans/tracker-seam.md)) that this
extension serves, `fenix.jira.tracker.*`: the issues assigned to you, the
issues linked to tasks, their workflow's moves, and the agenda's edits,
comments and worklogs. What Blocked means in a project (`jira.blocked`)
and how priorities map (`jira.priorities`) are kept here, and the agenda
holds neither your server nor your token.

Fenix doesn't tell an extension who's calling it, so any installed
extension could use the seam. The first change the seam brings in a
session asks you: *"Agenda wants to change issues in Jira for you --
allow for this session?"*; `jira.seam_writes` says it for good. A change
sent twice (the agenda gave up waiting, and you said send it again) is
made once in a session.

The agenda 1.2, from before the seam, uses `jira.request`,
`jira.config`, `jira.set_blocked` and `jira.new_issue`, which this one
still answers, unchanged; they go at 2.0. **`jira.request` is
deprecated**: it sends whatever request it's given to your Jira as you,
for any extension that asks, without asking you.

## Building

```text
cargo xtask pack extensions/jira dist
cargo test --manifest-path extensions/jira/Cargo.toml
cargo test --manifest-path crates/jira/Cargo.toml
```

The seam's Jira side is tested in `crates/jira` on recorded Jira Server
responses (`tests/fixtures`); here, the services the agenda 1.2 and 1.3
call, on the test kit.
