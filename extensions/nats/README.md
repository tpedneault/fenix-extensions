# NATS

A NATS client in Fenix: what's on your servers, live, a section a
subject -- how many, how fast, how big, the last minute's rate drawn --
opened to its messages, newest first, and a message opened to its
headers and its payload, JSON pretty-printed. Publish and request from a
buffer, send a message again or edit it first, and browse JetStream's
streams, consumers and key-value buckets. It keeps up with a busy
server: at hundreds of thousands of messages a second Fenix stays as
quick as it was, and the server never sees a slow consumer.

`SPC o m m` opens it and connects to the servers in `nats.servers`.

```toml
[nats]
servers = ["nats://localhost:4222", "tls://prod.example.com:4443"]
subjects = [">"]
```

## The live page

```text
NATS  localhost:4222  ● up  v2.10.22 · n1 · cluster lab · JetStream on · RTT 0.6 ms
2.0k msg/s · 1.2M messages · 13 subjects · stored 3.1 MiB / 64.0 MiB
subscribed >
/ find in subjects and payloads
 SUBJECT                              MESSAGES   RATE/S   LAST  AVG SIZE  LAST 30 S
▾ orders.created                          4.2k     35.7    now     158 B  ▅▆▆▅▇█▆▅▆▆▅
    05:37:44.253    159 B  h2  {"order": 183443, "customer": "c-16", "total": 155.99 …
  ▾ 05:37:44.203    158 B  h2  {"order": 183381, "customer": "c-51", "total": 98.98, …
      orders.created  05:37:44.203 · 158 B · json · #29043
      Nats-Msg-Id: 183381
      {
        "order": 183381,
        …
▸ orders.failed                            830        5    now     157 B  ▁▃▂▁▁▅▁▂▁▃▁
```

- **A subject** is a row: its messages, its rate a second (the last three
  seconds'), when it was last seen, its average size and a sparkline of
  the last 30 seconds. `Enter` opens it to its messages, newest first:
  the time, the size, how many headers, the reply subject and the start
  of the payload. `Enter` on a message opens it: its headers, then its
  payload -- JSON indented and coloured, text as it is, anything else as
  a hex dump. `o` opens it whole in a buffer.
- **The focus stays put.** New messages come in above the one you're on;
  it stays where it is on screen, and so does the message.
- **A tree** (`t`): subjects grouped by their tokens, `telemetry.*` with
  its subjects' totals, opened a token at a time.
- **Sorted** (`s`) by name, rate, count or when last seen.
- **Subscriptions**: `a` subscribes to a subject (`*` a token, `>` the
  rest), `d` unsubscribes, live, on every server; they're
  `nats.subjects`.
- **Pause** (`P`): nothing new is stored, so what you're reading stays;
  counting goes on.
- **Several servers**: each of `nats.servers` is a connection of its own;
  `c` shows the next one's subjects, `C` connects or disconnects the one
  shown.

## Filters

- `/` finds words in subjects and payloads, as you type.
- `f` writes a filter: each word a condition, all of which hold.

  | Word | Holds when |
  | --- | --- |
  | `orders.>`, `subject:orders.*.eu` | the subject matches (wildcards) |
  | `h:Nats-Msg-Id`, `header:region=eu` | it has the header, with that value |
  | `re:"time(out)?"` | its payload matches the regular expression |
  | `json:.status=failed`, `json:$.items[0].qty=2` | its payload is JSON with that value there |
  | `size:100..2k`, `size:>64k` | its size, headers and payload |
  | any other word | it's in the subject or the payload, any case |

  The filter's subjects become the subscriptions on the server: with
  `orders.*` in the filter, a `>` subscription is narrowed to `orders.*`,
  so what you'd hide isn't even sent. Its line under the subscriptions
  says so.
- `F` keeps filters by name, and uses them again.
- `H` writes highlight rules: what a rule matches is coloured, not
  hidden. A rule starting with `!` also notifies, once a rule every 30
  seconds at most, with how many matched meanwhile.

  ```text
  warn  orders.* json:.status=failed
  !bad  re:panic|fatal
  #4a90d9 h:Nats-Msg-Id size:>64k
  ```

## Keeping up

The extension runs on a thread of its own, and Fenix lays a page out
between its calls; a socket is read on another thread, 64 KiB at a time,
and stops being read when 8 MiB wait for the extension -- at which point
the server would see a slow consumer, and cut it off. So every message
costs as little as it can:

- **Each message is counted and added to its subject's rate**: a few
  additions. Counts are exact, always.
- **Its bytes are copied once**, as they are, into blocks of 1 MiB that
  are reused: no allocation per message. They're only read when a row
  shows them, and only decoded when you open one.
- **Memory is a budget** (`nats.budget_mib`, 64 MiB): every subject of
  every server together; the oldest messages go first when it's spent.
  A subject also keeps at most `nats.per_subject` messages and
  `nats.per_subject_kib`.
- **The page is drawn on a timer** (`nats.refresh_hz`, 10 a second),
  only when something changed and it's on screen -- never for a
  message -- and only the rows on screen are laid out.
- **Filters that read payloads** (a word, a regular expression, a JSON
  path, a header) read them on the subjects you're looking at, a few
  milliseconds a frame, newest first, and remember what they found; the
  subject and the size are checked from what's kept of every message
  without reading it.
- **It watches its own load**: the share of its time spent on what came,
  and how late its own timer is (a timer behind a queue of data comes as
  late as the queue). When either is high it stores 1 message in N of a
  busy subject's (`sampling 1:2`, `1:4`, ... on the page, with how many
  weren't stored) while a quiet subject (under 20 a second) keeps every
  message, and it goes back down when it's been calm for three seconds.
  At the extreme it only counts.

On a laptop-class machine the path from socket to store takes about 3
million messages a second of 16 B to 1 KiB, 5 million of 16 to 64 B,
natively, 99% of 64 KiB chunks in under 0.3 ms and the longest a few
milliseconds, one in thousands (`tests/throughput.rs`). Inside Fenix, as
WebAssembly, a local server sending as fast as it could was taken at
590,000 messages a second, busy 40% of the time, the page as quick to
answer as an idle one.

## Publishing

`p` (or `SPC o m p`) opens a buffer to write a message in, as JSON:

```json
{
  "subject": "orders.created",
  "server": "localhost:4222",
  "headers": { "Nats-Msg-Id": "42" },
  "reply": "",
  "payload": { "order": 42 }
}
```

A string payload is sent as its text, any other JSON as compact JSON,
`payload_base64` as bytes. `SPC o m s` sends it; `SPC o m r` (or `r` on
the page) sends it as a request, and the reply opens beside, with how
long it took -- or says no one answers.

The server can only say no one answers when nothing is subscribed to the
subject, and a `>` subscription is interested in everything. So with the
default `nats.subjects`, a request no one answers waits for its timeout,
and that goes for **every client of the server** while Fenix is connected.
Fenix says so once on a server that isn't this machine. On a shared
server, narrow `nats.subjects`, or a filter, to the subjects you want.

On a message: `R` sends it again as it was, `e` opens it in the buffer to
change first, `y` copies its payload, `Y` copies it as a `nats pub`
command line, headers and all. `x` writes a subject's stored messages to
a JSONL file, `X` every message the filter lets through
(`nats.exports`).

## JetStream

`J` (or `SPC o m j`): the account's streams and key-value buckets.

- **A stream** opens to its configuration, its sequence range and
  subjects, its consumers -- pending, waiting for an ack, redelivered, push
  or pull -- and its messages: `b` reads them from the first, `n` the next
  page, `g` from a sequence, `t` from a time (`10:00`, `2024-05-01 10:00`,
  `15m ago`; NATS 2.11). `P` purges it (you type its name), `d` deletes
  the message you're on.
- **A bucket** opens to its keys; a key to its value and its history. `p`
  puts a value, `d` deletes a key, `w` watches the bucket's changes as
  they come.

## Credentials

`SPC o m a` sets how a server is signed in to: a user and password, a
token, an NKey seed (signing the server's nonce), or a `.creds` file (a
JWT and its seed, read from `nats.creds_folder`). They're kept in the
system's keychain (`nats.auth.<host:port>`), never in the URL or the
settings. A sign-in the server refuses is said once, and tried again
every 30 seconds until the credentials change.

Connections are dialled again when they drop, after 0.25, 0.5, 1, 2, 4,
8, 15, then every 30 seconds, and their subscriptions made again. A PING
every 20 seconds keeps each alive and measures its round trip; three
unanswered and it's taken for dead. A server that wants TLS on a
`nats://` URL gets it after its `INFO`; `tls://` is TLS from the start.
The modeline says how they are: `NATS ● up 2.0k/s`.

## For coding agents

Four tools for your coding agents, in the `nats` toolset:

| Tool | Class | What it does |
| --- | --- | --- |
| `nats_sample` | read | Recent messages on a subject (wildcards): from what's stored, else it waits up to `wait_ms` for new ones |
| `nats_subjects` | read | The subjects, busiest first: messages, rate, average size, last seen |
| `nats_publish` | run | Publishes a message, with headers |
| `nats_request` | run | Sends a request and gives back the reply and its latency |

Payloads are cut at 4 KiB, binary ones in base64.

## Keys

| Key | What it does |
| --- | --- |
| `j` `k`, `PgUp` `PgDn`, `g` `G` | Move |
| `Enter` `l`, `h` | Open, close (a subject, a node, a message) |
| `/`, `f`, `F`, `H` | Find, filter, saved filters, highlight rules |
| `t`, `s` | The tree, the sort |
| `a`, `d` | Subscribe, unsubscribe |
| `P` | Pause storing |
| `p`, `r` | Publish, request |
| `R`, `e`, `y`, `Y`, `o` | A message: send again, edit and send, copy, as `nats pub`, open whole |
| `x`, `X` | Export the subject, the view, as JSONL |
| `c`, `C` | The next connection; connect or disconnect it |
| `J` | JetStream |
| `D` | Forget what's stored |
| `?` | The keys |

| Leader | Command | |
| --- | --- | --- |
| `SPC o m m` | `nats.open` | The live page |
| `SPC o m j` | `nats.jetstream` | JetStream |
| `SPC o m p` | `nats.publish` | A message to write |
| `SPC o m s` | `nats.send` | Send it |
| `SPC o m r` | `nats.request` | Send it as a request |
| `SPC o m a` | `nats.credentials` | A server's credentials |
| `SPC o m c` | `nats.connect` | Connect to a server, or disconnect |
| `SPC o m f`, `SPC o m h` | `nats.filter`, `nats.rules` | The filter, the highlight rules |

## Settings

| Setting | What it is |
| --- | --- |
| `nats.servers` | The servers, `nats://host:port` or `tls://host:port` |
| `nats.subjects` | What's subscribed to (`>`, which stops the server saying a request has no one to answer it) |
| `nats.budget_mib` | Memory for stored messages, 4 to 80 MiB (64) |
| `nats.per_subject`, `nats.per_subject_kib` | What a subject keeps at most (2,000 messages, 4 MiB) |
| `nats.max_subjects` | Subjects counted one by one on a server (5,000); the rest together |
| `nats.refresh_hz` | Redraws a second while messages come (10) |
| `nats.notify` | Whether `!` rules notify |
| `nats.client_name` | The name servers know the connection by (`fenix`) |
| `nats.accept_invalid_certs` | Take a TLS certificate unchecked |
| `nats.exports`, `nats.creds_folder` | Where exports go, where `.creds` files are |

Filters and rules are kept in its data folder (`nats.json`).

## Limits

- A message keeps at most its first 1 MiB; it says so when it's cut.
  One bigger than 64 MiB (more than a server's `max_payload` can be) ends
  the connection.
- Times are kept to the millisecond for 49 days after Fenix starts.
- The extension's memory is capped at 128 MiB, so the budget at 80.
- `connect_urls` (a cluster's other servers) aren't followed: each server
  is one you list, which is what its permission lets it reach.
- Reading a stream by time asks JetStream for `start_time`: NATS 2.11.
- Its own replies are left out of the live view; everyone else's
  `_INBOX` traffic a `>` sees is shown, as it is.

## What it may do

- Open sockets to the servers in `nats.servers`, and to any host on 4222,
  for one whose port is left out.
- Read and keep secrets `nats.auth.*`, a server's credentials each, in
  the system's keychain.
- Write exports to its exports folder, and read `.creds` files from its
  credentials folder.
- Notify (Fenix 1.2's API 1.3), when a rule marked `!` matches.

## Building

```text
cargo xtask pack extensions/nats dist
cargo test --manifest-path extensions/nats/Cargo.toml
cargo test --release --manifest-path extensions/nats/Cargo.toml --test throughput -- --ignored --nocapture
cargo test --manifest-path crates/nats/Cargo.toml
```

The protocol is `crates/nats` (`fenix-nats`): the parser, read with the
stream cut at every byte and every pair of bytes; `CONNECT`, `SUB`,
`PUB` and `HPUB`; headers; subjects and wildcards; NKeys against RFC
8032's Ed25519 vector and `.creds` files; inboxes; JetStream's and KV's
API. The extension's tests drive its connections, store, filters and
pages with a fake network; the throughput test feeds a million synthetic
messages through the same path.
