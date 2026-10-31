# Jira

Your Jira Server or Data Center in Fenix. Its page (`SPC j j`) lists your
searches on the left -- assigned to you, reported by you, watching,
recently updated, each tracked project's open issues and sprint, each
tracked person's, and your saved JQL -- with the selected search's issues
in the middle and a preview on the right.

`SPC j g` opens an issue by key, `SPC j /` searches (words, or JQL after
`:`), and `SPC j n` makes an issue with the project's real types and
fields.

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
settings page (`SPC ,`); `FENIX_JIRA_TOKEN` overrides the token. The
token is a personal access token, sent as a bearer token.

| Setting | What it does |
| --- | --- |
| `jira.projects` | Projects whose open issues and sprint are listed |
| `jira.users` | People whose issues are listed |
| `jira.queries` | Saved searches |
| `jira.blocked` | Per project, what Blocked means: a status, the Flagged field, or nothing |
| `jira.priorities` | How Jira's priorities map onto the agenda's |
| `jira.sync_minutes` | How often the agenda's linked tasks are brought up to date |

## For the agenda

The agenda extension reaches Jira through this one's services:
`jira.request`, `jira.config`, `jira.set_blocked` and `jira.new_issue`.
