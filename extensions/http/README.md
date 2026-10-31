# HTTP client

An HTTP client in Fenix, on `.http` files: the format VS Code's REST
Client and JetBrains' IDEs use. You edit them as text, with Vim and all,
review them in Git and share them with people on other editors; a folder
of them is a collection.

```http
@baseUrl = https://api.example.com

### Log in
# @name login
POST {{baseUrl}}/login
Content-Type: application/json

{ "user": "ada", "password": "{{password}}" }

### Who am I
# @test status == 200
# @test $.name == "ada"
GET {{baseUrl}}/me
Authorization: Bearer {{login.response.body.$.token}}
```

`examples/` has a collection to try, with its env files.

## Running from the buffer

`SPC p r r` (or `SPC m r` in a `.http` file, or a click on the `▶`
before it) sends the request the cursor's in. Its status shows after it
-- `200 OK · 85 ms · 1.2 kB · tests 2/2` -- and its response opens
beside; a click on the status brings the response back. `SPC p r a`
runs every request in the file and reports.

`.http` and `.rest` files are highlighted by tree-sitter-http, which the
extension brings (`grammars/`): Fenix has no grammar of its own for them.
As you type, `{{` offers the variables -- the file's, the environment's,
the secrets', the system's, other requests' answers -- a header line the
header names, `Content-Type:` and `Authorization:` their usual values,
`# @` the directives; a variable nothing sets is underlined.

## The format

- Requests between `###` lines (what follows `###` is its title). A
  request line, `GET https://...` (the method left out is GET), its query
  carried on indented `?` and `&` lines; headers; a blank line; the body.
- File variables, `@name = value`, and `{{name}}` anywhere.
- Comments, `#` and `//`, and directives in them:

  | Directive | |
  | --- | --- |
  | `# @name login` | What chained requests call it |
  | `# @test status == 200` | A test on its answer (below) |
  | `# @no-redirect` | A 3xx is the answer |
  | `# @no-cookie-jar` | No cookies sent or kept |
  | `# @timeout 10` | Seconds it may take (`500 ms`, `2 m`) |
  | `# @insecure` | A server whose certificate doesn't check out |
  | `# @stream` | Its body shown as it comes: server-sent events |
  | `# @no-log` | Not kept in history |

- Bodies: text, its variables filled in; `< ./file` a file's bytes (`<@
  ./file` with its variables filled in) -- a binary body, or a part of a
  `multipart/form-data` one, its lines sent with CRLF; a form's lines
  joined (`name=ada` then `&role=admin`); GraphQL with `X-Request-Type:
  GraphQL` (or the method `GRAPHQL`): the query, a blank line, its
  variables as JSON.
- `WEBSOCKET ws://...` (or `wss://`): a WebSocket, below.
- `Authorization: Basic user:password` (or `user password`) is encoded as
  it's sent.
- `>> out.json` after the body saves the response's body beside the file
  (`out-1.json` when there's one; `>>!` writes over it). JetBrains'
  handler scripts (`> {% ... %}`) are kept but not run: they're
  JavaScript, which an extension has nothing to run. Say tests with
  `# @test`.

### Variables

Looked for in the file's variables, then the environment's, then the
secrets'. System ones: `{{$uuid}}`, `{{$timestamp}}` (`-1 d`),
`{{$isoTimestamp}}`, `{{$datetime iso8601}}` (`rfc1123`, `"YYYY-MM-DD"`,
an offset), `{{$localDatetime}}`, `{{$randomInt 1 100}}`,
`{{$random.integer(1, 100)}}`, `{{$random.alphanumeric(8)}}`,
`{{$random.email}}`, `{{$dotenv NAME}}` (the project's `.env`),
`{{$secret name}}`, `{{$auth.token("id")}}`, and `{{$processEnv NAME}}`,
one of Fenix's environment variables -- only those `http.env` lists
(`JAVA_*` for those starting so); `{{$processEnv %name}}` takes the
variable's name from the environment's `name`.

### Chained requests

`{{login.response.body.$.token}}` is a JSONPath into the last answer of
the request called `login`; `{{login.response.headers.X-Id}}` a header
of it; `.request.` the same of what was sent; `body.*` all of it. When
`login` hasn't been sent, it's sent first, then this one.

### Tests

`# @test` lines, a test each: `status`, `time` (milliseconds), `size`,
`body`, `header <name>` or a JSONPath, then `==`, `!=`, `<`, `<=`, `>`,
`>=`, `contains`, `!contains`, `startsWith`, `endsWith`, `exists` or
`!exists`, then a value (JSON, or a word). `$.items.length` is how many.
The status after a request says how many passed; the response's Tests
tab says which and what was found.

### WebSocket

JetBrains' way: the URL, its headers (`Authorization`,
`Sec-WebSocket-Protocol`), then the messages, a line `===` before each:

```http
WEBSOCKET wss://echo.example.com/socket
Authorization: Bearer {{token}}

===
{ "subscribe": "prices" }
=== wait-for-server
{ "ack": true }
```

`=== wait-for-server` holds a message back until the server has said
something. The connection stays open: what went and came, a line each
with its time, is the response's body as it goes, and `SPC p r w` sends
another message. It's over when the server closes it, or `SPC p r x`;
then it's kept in the history as any answer is. Pings are answered;
binary messages are shown as hex. Not through `http.proxy`, and without
a client certificate.

### GraphQL completion

In a GraphQL request's query, completion offers the fields of the type
the cursor's selection is on (and their types), a field's arguments in
its parentheses, and after `... on` the types a fragment may be on: from
the server's schema, which the request asks for (the introspection
query, with its URL, headers and auth) the first time you complete in
it, and keeps for the project. `SPC p r g` asks again once the schema's
changed.

### The runner

`SPC p r a` runs a file's requests in turn, `SPC p r A` the project's,
and `x` on a folder in the sidebar that folder's: the report says each
one passed -- its tests, or with none, a status under 400 -- and why
not. `Enter` there goes to the request.

## Environments

`http-client.env.json` beside the `.http` files (or in a folder above
them, up to the project), and `http-client.private.env.json` beside it
for what isn't committed -- add that one to `.gitignore`:

```json
{
  "$shared": { "version": "v1" },
  "dev": { "baseUrl": "http://localhost:8080" },
  "prod": { "baseUrl": "https://api.example.com" }
}
```

`SPC p r e` (or `e` on the client's page) chooses one, for the project;
the modeline says which. `$shared` is every environment's; a private
value wins over the public one. Each environment keeps its own cookies.

### Secrets

Kept in the system's keychain -- Windows' Credential Manager, the macOS
Keychain, the Secret Service on Linux -- not the project: `SPC p r k`
sets one (its value typed as dots; nothing forgets it), and `{{name}}`
reads it (`dev.name` is environment `dev`'s alone). Each is a secret of
its own, `http.secret.<name>`; their names, not their values, are kept
in the extension's folder for completion. The older `http.secrets`
setting (`name=value; name=value`, or `FENIX_HTTP_SECRETS`) is still
read, after the keychain, and moved into it when the extension starts.
With no keychain, Fenix keeps them in `settings.toml` as plain text, and
says so.

## Auth

The client's Auth tab writes the header: Bearer, Basic, an API key, or
OAuth 2.0. OAuth configurations are JetBrains', in the env file's
`Security.Auth` (the client secret in the private file):

```json
{
  "dev": {
    "Security": {
      "Auth": {
        "my-api": {
          "Type": "OAuth2",
          "Grant Type": "Authorization Code",
          "Auth URL": "https://auth.example.com/authorize",
          "Token URL": "https://auth.example.com/token",
          "Redirect URL": "http://localhost/callback",
          "Client ID": "my-client",
          "Scope": "openid profile",
          "PKCE": true
        }
      }
    }
  }
}
```

and `Authorization: Bearer {{$auth.token("my-api")}}`. The first request
that needs a token gets it; it's kept in the system's keychain (for the
project and environment) and refreshed with its refresh token when it
runs out. `SPC p r s` signs in again, `SPC p r S` forgets them.

| Grant Type | |
| --- | --- |
| `Authorization Code` | With PKCE (S256). The browser opens at the provider's page, and once you've signed in it's sent back to Fenix, listening on this machine (below). |
| `Client Credentials` | The client's ID and secret: `Basic`, or `"Client Credentials": "in body"`. |
| `Password` | `Username`, `Password`; a password left out is asked for, as dots. |
| `Device Authorization` | `Device Auth URL`: the code to enter is copied and its page opened; it asks until you've signed in. |

The redirect is listened for on 127.0.0.1, as RFC 8252 has it: a
`Redirect URL` of `http://127.0.0.1:8400/callback` or
`http://localhost:8400/callback` on its port; `http://localhost/callback`,
with no port, or no `Redirect URL` at all, on any free one, which the
redirect then says (providers take any port on the loopback). The
browser's page says how it went, and you go back to Fenix. A sign-in
waits five minutes for it; `SPC p r x` stops it. A redirect elsewhere
(`https://`, another host) can't be listened for: then you paste the
address the browser was sent to, or just the code. The device grant
needs no redirect. The implicit grant isn't read.

`Custom Request Parameters` (an `audience`) go with both requests; `Use
ID Token` sends the ID token.

## The client

`SPC p r o` opens the client's page:

- **Collections**, in a sidebar: the project's `.http` files by folder,
  each request with its method in its colour. `Enter` shows one, `x` runs
  a file's or a folder's, `n` adds a request, `o` opens its file.
- **History**, the sidebar's other tab (`]`): what was sent, newest
  first, searchable (`/`: method, URL, status, name, file). `Enter` shows
  it as it was sent, with its answer; `s` saves it to a collection (its
  variables as they were filled in); `r` sends it again.
- **The request**: its method and URL, then tabs for its params,
  headers, body, auth and tests. Every field is a part of the `.http`
  file, and changing it changes those lines -- one change you can undo,
  saved (`http.save_edits`). `m` the method, `u` the URL, `a` add,
  `Enter` change, `d` delete, `b` the body's type (JSON, form, multipart,
  text, XML, a file's bytes, GraphQL). A body is edited in its file:
  `Enter` on the Body tab opens it there.
- **The response**: its status, how long it took and how big it is, then
  tabs for its body (JSON, XML and HTML pretty-printed and coloured, a
  picture shown; `p` as it came), headers, cookies (what it set, and what
  the environment keeps for its host), timing (DNS, connect, TLS, first
  byte, download, total) and tests. `O` opens the body in a buffer, `S`
  saves it.

`Tab` goes between the sidebar, the request and the response; `[` `]`
and `1`-`5` between tabs; `r` (or Ctrl+Enter) sends; `e` the
environment; `y` copies as code; `\` hides the sidebar. The mouse picks
too.

## Imports and exports

`SPC p r i` imports the file selected in the explorer (or one you name):
a Postman collection (v2.1 and v2.0), an Insomnia export (v4, JSON or
YAML), or an OpenAPI 3 spec (JSON or YAML). It becomes a folder of
`.http` files -- a file for each of Postman's and Insomnia's folders, or
each of OpenAPI's tags -- and its variables go in the folder's
`http-client.env.json` (Insomnia's environments as environments). What it
can't bring -- Postman's scripts, OAuth set up in the collection -- is
said in its log (`SPC h l`).

- Postman: requests, headers, query, `:id` path variables as `{{id}}`,
  raw, form, multipart and file bodies, GraphQL, Bearer, Basic and API
  key auth (the collection's, a folder's or the request's).
- Insomnia: the same; `{{ _.name }}` as `{{name}}`.
- OpenAPI: a request each operation, `{{baseUrl}}` its first server,
  path parameters as variables, required query parameters and headers
  with their examples, a JSON body from its example or made from its
  schema, the auth header its security scheme says.

`SPC p r c` imports a `curl` command from the clipboard (or one you
type) -- Chrome's "Copy as cURL", an API's docs -- at the end of the
`.http` file you're in. `SPC p r y` copies the request under the cursor
as `curl`, Python (requests), JavaScript (fetch), Go (net/http) or Rust
(reqwest), and shows it beside: the request as it's sent, its variables
filled in -- secrets too, so mind where it goes.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC p r r` | `http.send` | Send the request under the cursor |
| `SPC p r a` | `http.send_all` | Run this file's requests, and report |
| `SPC p r A` | `http.run_project` | Run the project's, and report |
| `SPC p r o` | `http.open` | The client |
| `SPC p r h` | `http.history` | The history |
| `SPC p r v` | `http.response` | The response to the request under the cursor |
| `SPC p r e` | `http.env` | Choose the environment |
| `SPC p r y` | `http.copy_as` | Copy as curl or code |
| `SPC p r i` | `http.import` | Import a Postman, Insomnia or OpenAPI file |
| `SPC p r c` | `http.import_curl` | Import a curl command |
| `SPC p r n` | `http.new` | A new `.http` file |
| `SPC p r s` | `http.sign_in` | Sign in with OAuth 2.0 |
| `SPC p r S` | `http.sign_out` | Forget the tokens |
| `SPC p r k` | `http.secret` | Set a secret |
| `SPC p r w` | `http.ws_send` | A message to the open WebSocket |
| `SPC p r g` | `http.graphql_schema` | Ask the GraphQL server for its schema again |
| `SPC p r x` | `http.cancel` | Cancel what's in flight, close WebSockets, stop a sign-in |
| | `http.cookies_clear` | Clear the environment's cookies |

`SPC m` in a `.http` file has the same: `r` send, `a` run the file's, `v`
the response, `e` environment, `y` copy as, `o` the client, `c` import
curl, `s` sign in, `w` a WebSocket message, `g` the GraphQL schema, `x`
cancel.

## Settings

| Setting | What it does |
| --- | --- |
| `http.timeout` | Seconds a request may take (30) |
| `http.follow_redirects` | Follow 3xx answers |
| `http.insecure` | Reach servers whose certificate doesn't check out |
| `http.proxy` | An HTTP proxy, `http://host:port` |
| `http.response` | `beside`: a request sent from its file shows its response beside; `none`: only its status |
| `http.save_edits` | Save the `.http` file after each change the client makes |
| `http.send_marks` | The `▶` before each request |
| `http.history` | How many requests the history keeps (200) |
| `http.secrets` | The older secrets, moved into the keychain |
| `http.env` | The environment variables `{{$processEnv}}` may read |

The first four a project may set too (`.fenix/settings.toml`). A client
certificate is the env file's `SSLConfiguration` (`clientCertificate`,
`clientCertificateKey`, PEM files beside it); `verifyHostCertificate:
false` is `http.insecure` for that environment.

## What it keeps

In its own folder, a folder a project, outside the project so Git never
sees it: the environment you chose, the history (bodies cut at 64 kB),
each environment's cookies, which OAuth tokens it has (the tokens are in
the keychain), and the GraphQL schemas read. The secrets' names.

## What it may do

- **Reach any host:** the requests are yours.
- **Where you are, and change it:** the request under the cursor; its
  status and `▶`s drawn in the file; the client's changes made to it.
- **The project, read and changed:** the `.http` and env files;
  imports written, bodies saved.
- **Your secrets:** `http.secret.*` and `http.token.*` in the
  keychain, read and kept; the older `http.secrets`.
- **Listen on 127.0.0.1,** any port: an OAuth redirect.
- **Open sockets** to any host and port: WebSocket requests.
- **Read the environment variables `http.env` lists,** none other.
- **The clipboard's text:** a curl command, imported.

## Not here yet

XPath into XML answers, Digest auth, JetBrains' JavaScript handlers,
`{{$prompt}}` variables, Insomnia's v5 YAML, GraphQL subscriptions over
WebSocket (`graphql-transport-ws`), and a WebSocket through a proxy.
