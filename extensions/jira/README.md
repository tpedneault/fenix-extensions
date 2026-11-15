# Jira

Your Jira Cloud, Server or Data Center in Fenix. Its page (`SPC j j`) lists your
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

Its site is `jira.base_url` and its token `jira.token`, both on the
settings page (`SPC ,`, or `Ctrl+,` with standard keys);
`FENIX_JIRA_TOKEN` overrides the token. The token is kept in your
system's keychain, never in the settings file, and never sent to the
agenda or written to a log.

Which Jira it is comes from the address: `*.atlassian.net` (and the
older `*.jira.com`) is Jira Cloud, anything else Server or Data Center.
The first time it reaches the site it asks it (`serverInfo`, without
your token), so a Cloud site on its own domain is found too; the page
says which it is beside the address. `jira.kind` says it yourself.

### Jira Server or Data Center

The token is a personal access token (your profile, **Personal Access
Tokens**), sent as a bearer token.

### Jira Cloud

Cloud signs you in with your Atlassian account's email and an API
token:

1. Go to <https://id.atlassian.com/manage-profile/security/api-tokens>
   (your Atlassian account, **Security**, **API tokens**) and choose
   **Create API token**. Name it (`Fenix`), pick when it expires, and
   copy it: it's shown once.
2. On the settings page, under Jira: **Jira server** is your site,
   `https://your-site.atlassian.net`; **Jira token** is the API token;
   **Jira email** is the email you sign in to Atlassian with.
3. Open the Jira page (`SPC j j`): it says `Jira Cloud` beside your site.

A token made with **Create API token with scopes** is taken only through
Atlassian's gateway, `api.atlassian.com`: turn on **Scoped API token**
(`jira.scoped_token`) too. It needs the scopes `read:jira-work`,
`write:jira-work` and `read:jira-user`. Signing in through your browser
(OAuth) isn't offered: Atlassian's OAuth needs a client secret, which an
open-source extension can't keep secret.

What Cloud doesn't do here yet, that Server does:

- **A description isn't edited from Fenix**, from the page (`E`) or the
  agenda: Cloud keeps it as rich text, which Fenix shows as plain text
  (lists, code, tables, links and mentions as you'd read them; colours,
  bold and pictures left out), and writing that back would lose the
  rest. A description you change on a linked task stays on the task.
  Comments and a new issue's description are sent as paragraphs and
  line breaks. Everything else the agenda changes -- moves, title,
  priority, due date, assignee (or nobody), labels, comments, worklogs
  -- goes to Jira Cloud as to Server, after the same question once a
  session (`jira.seam_writes`).
- **People** (`jira.users`, and those added with `a` on the page) are
  account ids on Cloud, not usernames: add someone from an issue they're
  assigned. The agenda's assignee picker lists who Jira says can be
  assigned the issue.

| Setting | What it does |
| --- | --- |
| `jira.email` | Jira Cloud: your Atlassian account's email |
| `jira.kind` | `auto`, `cloud` or `server` |
| `jira.scoped_token` | Jira Cloud: the API token is a scoped one |
| `jira.projects` | Projects whose open issues and sprint are listed |
| `jira.users` | People whose issues are listed |
| `jira.queries` | Saved searches |
| `jira.blocked` | Per project, what Blocked means: a status, the Flagged field, or nothing |
| `jira.priorities` | How Jira's priorities map onto the agenda's |
| `jira.sync_minutes` | How often the agenda's linked tasks are brought up to date (every 5 minutes at most) |
| `jira.seam_writes` | Whether extensions may change your issues through Jira: `ask` (the default), `always`, or `never` |
| `jira.allow_http` | Server or Data Center over `http://`: send your token in clear anyway (off by default) |

## What it may do

- Reach your Jira, the site in `jira.base_url`, and `api.atlassian.com`
  -- Atlassian's gateway to a Cloud site, used only with a scoped API
  token (`jira.scoped_token`).
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
extension could use the seam, and Jira can't say which one asks. The
first change the seam brings in a session asks you: *"An extension asks
to change issues in Jira for you (Fenix can't tell which; usually the
Agenda) -- allow?"*

- **Allow every extension's changes through Jira until Fenix restarts**
  lets every change through, whoever sends it, until you restart Fenix.
- **Not now** (or Esc) refuses the changes that waited, and any other
  for 10 minutes without asking again; `r` on the task tries again after
  that. Changing `jira.seam_writes` ends the 10 minutes at once.
- A question no one answers within a minute (or that Fenix replaced
  with another picker) refuses what waited, and the next change asks
  again. At most 50 changes wait for your answer; more are refused.

`jira.seam_writes` says it for good: `always` or `never`. A change sent
twice (the agenda gave up waiting, and you said send it again) is made
once in a session.

The agenda 1.2, from before the seam, uses `jira.request`,
`jira.config`, `jira.set_blocked` and `jira.new_issue`, which this one
still answers; they go at 2.0. **`jira.request` is deprecated**: it
sends whatever request it's given to your Jira as you, for any extension
that asks, without asking you. It's refused for Jira Cloud, whose API
token is account-wide (the agenda 1.2 never worked with Cloud).

## Your token over http

Your token isn't sent in clear by default. Jira Cloud is only reached
over `https://`. A Server or Data Center address starting `http://` is
refused unless you turn on **Allow a Server over http** (`jira.allow_http`), which
sends your token unencrypted -- use it only on a network you trust.

## Building

```text
cargo xtask pack extensions/jira dist
cargo test --manifest-path extensions/jira/Cargo.toml
cargo test --manifest-path crates/jira/Cargo.toml
```

The seam's Jira side is tested in `crates/jira` on recorded Jira Server
responses (`tests/fixtures`), and on Jira Cloud responses written from
the shapes Atlassian documents (`tests/fixtures/cloud`: not recordings);
`tests/server_unchanged.rs` holds every request Server and Data Center
are sent to what they were before Cloud. Here, the services the agenda
1.2 and 1.3 call, and Cloud's sign-in, on the test kit.
