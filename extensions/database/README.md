# Database browser

Connect to a database, browse its schema and run statements without
leaving Fenix. SQLite files, PostgreSQL, MySQL and MariaDB, and SQL
Server.

`SPC o b` (`database.open`) opens the browser: your saved connections in
a sidebar, each one's schema under it once it's connected -- schemas,
tables and views, their columns with their types, indexes and keys --
and beside them what the last statements you ran gave back.

```text
Connections                 │ dev  2 statements · 14 ms
                            │ 1 Rows 2   2 Done
● dev   PostgreSQL · ready  │
  ▾ app                     │ id  name   email  score
    ▾ Tables (2)            │ ─────────────────────────
      ▾ users            6  │  1  ada    a@x     1.50
          id   integer · PK │  2  grace  NULL    2.25
          name text         │
        ▸ Indexes (2)       │
        ▸ Keys (1)          │
○ prod  PostgreSQL · production
○ shop.db   SQLite          │ name text  row 1 of 2
                            │ ada
```

A `.sqlite`, `.sqlite3`, `.db` or `.db3` file opens straight into it,
however you open it.

## Connections

`database.connections` holds them, a row each: a name, the engine
(`sqlite`, `postgres`, `mysql` -- MariaDB too -- or `mssql`), the host
and port (left out: the engine's own), the database (a SQLite file's
path, in the project: `/project/data/app.db`), the user, and options:

| Option | What it does |
| --- | --- |
| `readonly` | Nothing that changes what's stored runs on it -- refused here, and the session's read-only too (PostgreSQL's `default_transaction_read_only`, MySQL's `SET SESSION TRANSACTION READ ONLY`; a SQLite file opened read-only) |
| `production` | A statement that changes what's stored is asked about first |
| `tls=prefer` | TLS if the server offers it: PostgreSQL's and MySQL's default |
| `tls=require` | TLS or nothing |
| `tls=strict` | SQL Server's TDS 8: TLS from the first byte (SQL Server 2022, Azure SQL) |
| `tls=off` | None |
| `insecure` | A server certificate taken whoever signed it: a server of yours |

A password is asked for when you connect, the prompt showing dots, and
once it signs in it's kept in the system's keychain -- Windows'
Credential Manager, the macOS Keychain, the Secret Service on Linux --
as `database.password.<name>`, and not asked for again
(`database.keep_passwords` off: kept until Fenix closes). One the server
refuses is forgotten and asked for again; `Database: forget the password
kept for a connection` forgets one. The older map `database.passwords`
(connection, password; `FENIX_DATABASE_PASSWORDS`, as JSON, overrides
it) is still read, after the keychain.

An UPDATE or DELETE without a WHERE is asked about on any connection.

## Running statements

In a `.sql` buffer:

| Key | What it does |
| --- | --- |
| `SPC m r` | Run the statement under the cursor -- or the selection, its statements in turn |
| `SPC m R` | Run every statement in the buffer |
| `SPC m c` | Pick the connection the buffer runs on |
| `SPC m h` | The history: the statements you ran, newest first, searched as you type; one picked goes in at the cursor |
| `SPC m n` | A new query buffer |
| `SPC m b` | The browser |

A buffer runs on the connection its first lines name (`-- connection:
dev`), else the one you picked for it, else the only one connected. The
modeline says which, and how it is (`db dev · ready`).

Statements are split where the engine splits them: at `;` outside
quotes, quoted names and comments; PostgreSQL's dollar quoting
(`$$ ... $$`, `$body$`) and nested comments; MySQL's `#` comments,
backslashes in strings and `DELIMITER`; SQL Server's `[names]` and `GO`
lines; SQLite's trigger bodies.

## Results

A statement's rows are a table, a tab a statement (`1`-`9`, `[` `]`):

| Key | What it does |
| --- | --- |
| `h` `j` `k` `l`, the arrows | A cell; `J` `K` a screen, `g` `G` the first and last row |
| `s` | Sort by the column: up, down, as they came. Numbers sort as numbers, NULLs last |
| `v` `V` | Select cells, rows |
| `y` | Copy the cell, or the selection (a tab between cells) |
| `Y` | Copy the row, or the selected rows, as CSV with the header |
| `x` `X` | Every row as CSV, as JSON, in a buffer beside |
| `w` `W` | Every row to a file in the project, CSV or JSON |
| `Tab` | The sidebar, and back |
| `H` | The history |

The focused cell is shown whole below the table, its column's type
beside its name.

## Table view

`Enter` on a table in the sidebar pages through its rows, by its key's
order:

| Key | What it does |
| --- | --- |
| `n` `p`, PageDown PageUp | The next page, the last |
| `/` | Filter: a WHERE's condition (`score > 1 AND name LIKE 'a%'`); Enter on nothing, every row |
| `e`, `Enter`, a double click | Change the cell: the UPDATE it makes, keyed by the row's primary key (SQLite's rowid when there's none), is shown, and runs on `y` |
| `D` | Set the cell NULL, the same way |
| `r` | Read the page again |
| `s` `v` `V` `y` `Y` | As in the results |

A table without a primary key isn't changed here: there's no telling
its rows apart.

## Completion

In a `.sql` buffer, from the connected schema: tables (and schemas)
after `FROM`, `JOIN`, `INTO` and `UPDATE`; a table's columns after
`table.` or `alias.`; the statement's tables' columns elsewhere; and
keywords.

## Engines

- **SQLite**, through [Turso](https://github.com/tursodatabase/turso)'s
  engine: SQLite in Rust, which builds for WebAssembly with no C
  compiler and reads and writes the file itself. A file in the project
  is opened where it is; one elsewhere is copied and opened read-only.
  Turso writes through a WAL: a file that kept a rollback journal is put
  back as it was once a change is written (the log checkpointed into it
  and removed, its header saying so again). Turso is still in beta;
  what it doesn't do yet it says.
- **PostgreSQL**: its protocol (v3), TLS asked for with an SSLRequest,
  SCRAM-SHA-256, md5 or a cleartext password; statements you run go as
  one simple query, a table view's as an extended one (Parse, Bind,
  Execute), which the server holds to one statement.
- **MySQL and MariaDB**: the handshake, TLS asked for with an
  SSLRequest, caching_sha2_password (fast, or in full: over TLS, or to
  the server's RSA key) and mysql_native_password, auth switches;
  COM_QUERY, several statements at once.
- **SQL Server**: TDS 7.4 -- PRELOGIN, LOGIN7 with SQL Server's own
  sign-in, SQL batches, a batch a statement -- and TDS 8 with
  `tls=strict`.

Checked against real servers: PostgreSQL 16, MariaDB 10.11, MySQL 8.4
(caching_sha2_password in full to the RSA key and fast, plain and with
TLS) and SQL Server 2022 (TDS 7.4 unencrypted: the schema, every common
type, a table view's pages and changes). TDS 8 hasn't been: the test
server wouldn't take a strict connection with its self-signed
certificate.

## Limits

- **TLS on SQL Server** is TDS 8's only. TDS 7 carries TLS inside its
  PRELOGIN packets, which Fenix's sockets, which do TLS themselves,
  can't; a server that forces encryption is reached with `tls=strict`
  (SQL Server 2022 and Azure SQL), or not. Windows sign-in (SSPI,
  Kerberos) isn't spoken, nor is Entra ID.
- **PostgreSQL**: SCRAM-SHA-256-PLUS (channel binding), GSSAPI and SSPI
  aren't spoken; a password isn't SASLprep'd.
- **MySQL**: MariaDB's ed25519 and other plugins aren't spoken.
- **A SQLite statement** runs inside the call Fenix gives it, which its
  manifest makes a minute: one that takes longer than
  `database.timeout` (30 seconds; 55 at most) is stopped, and the
  browser waits meanwhile. A file outside the
  project is a copy, read-only, and its `-wal` isn't read with it.
- **Rows kept**: `database.max_rows` (5,000) a result; a table view's
  page, `database.page_rows` (200).
- **No keychain** (Linux without the Secret Service): a password kept
  goes to `settings.toml` as plain text, which Fenix says on its output
  page. On Linux the first one read may wait on the keychain's unlock
  prompt.

## What it may do

- Open sockets to the servers in `database.connections`, and to any
  host on 5432, 3306 and 1433, for a row that leaves its port out.
- Read and keep secrets `database.password.*`, a connection's password
  each, in the system's keychain; read the older `database.passwords`.
- Take up to a minute a call (`limits.call_ms`), for a SQLite statement.
- Read and write the project's files: a SQLite file in it, an export.
- Read and change the buffer you're in: a `.sql` buffer's text, the
  cursor and the selection; a statement from the history typed at the
  cursor (`editor = "write"`).
- Be told which buffer has the focus (`events = ["focus"]`), for the
  modeline.

## Building

```text
cargo xtask pack extensions/database dist
cargo test --manifest-path extensions/database/Cargo.toml
```

`cargo test` checks statement splitting and the kinds of statements the
safety rules go by, the rules themselves, each engine's protocol against
byte sequences -- hand-made from the protocol's documents, and captured
from PostgreSQL 16 and MariaDB 10.11 (`src/testdata`) -- SCRAM-SHA-256
against RFC 7677's example, the scrambles, TDS's types, the catalog put
together into a schema and its tree, SQLite files made by SQLite itself
(Python's `sqlite3`) read and written and their journal put back, the
UPDATE builder and table pages, completion, history, and the pages'
keys and layout.
