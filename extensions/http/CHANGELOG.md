# Changelog

## 1.3.0

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
- When a run ends, the modeline names what ran and how many failed, and
  how long a long one took; it says nothing while the run's report is
  in front of you.

## 1.2.0

- `http.open`, `http.history` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

- For Fenix 1.5: with standard keys, Ctrl+Enter in a .http file sends the
  request under the cursor.

## 1.0.0

- The first release, for Fenix 1.0: an HTTP client on .http files (REST Client's and JetBrains'): run the request under the cursor and see its status beside it, collections and history in a sidebar, a request view whose every field edits the file, the response's body, headers, cookies and timing, environments and secrets, OAuth 2.0, chained requests, tests and a runner, WebSocket requests, GraphQL completion from the server's schema, and imports from Postman, Insomnia, OpenAPI and curl.
