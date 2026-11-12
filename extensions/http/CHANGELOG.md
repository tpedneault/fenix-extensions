# Changelog

## 1.2.0

- `http.open`, `http.history` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

- For Fenix 1.5: with standard keys, Ctrl+Enter in a .http file sends the
  request under the cursor.

## 1.0.0

- The first release, for Fenix 1.0: an HTTP client on .http files (REST Client's and JetBrains'): run the request under the cursor and see its status beside it, collections and history in a sidebar, a request view whose every field edits the file, the response's body, headers, cookies and timing, environments and secrets, OAuth 2.0, chained requests, tests and a runner, WebSocket requests, GraphQL completion from the server's schema, and imports from Postman, Insomnia, OpenAPI and curl.
