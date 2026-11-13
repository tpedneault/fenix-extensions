# Changelog

## 1.3.0

For Fenix 1.5 (extension API 1.5): its manifest now says so, and it's
built against API 1.5, as the shared `ui-kit` it draws with now is. On
Fenix 1.0 to 1.4, 1.2.0 stays the one you have.

- A lesson in the tutorial (`SPC h T`, under HTTP): reading a `.http`
  file, sending the request under the cursor and seeing its response,
  variables, environments and the client, on a practice file. Steps
  that need the internet can be skipped (`s`) offline.
- `SPC p r r` in text that's no file -- a scratch buffer, the
  tutorial's practice -- sends the request on the cursor's line, or the
  one selected with its variables; `SPC p r v` shows its response again.
- Tools for coding agents, from Fenix 1.1: `http_requests` lists the
  project's requests, and `http_run` sends one by its name, in the
  environment you chose, and answers its status, headers, body and
  tests, your secrets' values hidden and a long body cut. Running is an
  external tool, which Fenix refuses until your `agents.mcp.allow` lets
  agents run it (`http_run = "ask"`).
- When a run ends while its report isn't in front of you, it says what
  ran and how many failed: in the message line, or, for a run of 10
  seconds or more, in a notification -- of low urgency when all passed,
  normal when some failed, a newer run's in place of the last, and
  *Open the report* (`http.report`) bringing up the report; closing the
  report takes it away. It says nothing while the report is in front of
  you.
- An import that couldn't bring everything names the keys of the log
  that lists what it left out as your key scheme has them (`C-k h l`
  with standard keys, `SPC h l` with Vim keys), or yours.

## 1.2.0

- `http.open`, `http.history` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

- For Fenix 1.5: with standard keys, Ctrl+Enter in a .http file sends the
  request under the cursor.

## 1.0.0

- The first release, for Fenix 1.0: an HTTP client on .http files (REST Client's and JetBrains'): run the request under the cursor and see its status beside it, collections and history in a sidebar, a request view whose every field edits the file, the response's body, headers, cookies and timing, environments and secrets, OAuth 2.0, chained requests, tests and a runner, WebSocket requests, GraphQL completion from the server's schema, and imports from Postman, Insomnia, OpenAPI and curl.
