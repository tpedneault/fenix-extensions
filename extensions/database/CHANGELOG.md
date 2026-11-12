# Changelog

## 1.4.0

For Fenix 1.5 (extension API 1.5), which it needed since 1.2.0 -- its
manifest now says so too.

- A lesson in the tutorial (`SPC h T`, *Database*): the browser, a table's
  rows, a statement run from a `.sql` buffer, its results, the history and
  a buffer's connection, on a sample database -- nothing to install, no
  server. Its keys are said as your key scheme has them.
- **Database: a sample database to try it on** (`database.sample`): a
  small shop in SQLite, made afresh in the extension's own folder, opened
  in the browser.
- The browser names a `.sql` buffer's keys for standard keys too:
  `Ctrl+K m r` beside `SPC m r`, and `Ctrl+Enter`, in its empty results
  and its help (`?`). With no connection yet, it says how to add one, or
  where the sample is.
- Fixed: a coding agent's session went on with a connection's old host,
  user or options after its row in `database.connections` changed, until
  Fenix restarted; it's closed now, and opened again as the row says.
  `database_query`'s description names `TABLE`, which it always took, and
  says a connection can be an open SQLite file.

## 1.3.0

- `database.open`, `database.history` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.2.0

For Fenix 1.5: with standard keys, Ctrl+Enter in a .sql buffer runs the
statement under the cursor (or the selection).

## 1.1.0

For Fenix 1.1: tools for your coding agents.

- `database_schema` (read): a connection's schema -- tables and views,
  columns with their types and nullability, primary, unique and foreign
  keys, indexes -- or one table's; the connection named, else the one
  the `.sql` buffer you're in runs on.
- `database_query` (read): the rows of one statement that only reads,
  as JSON, at most 1000. Anything else is refused before it's sent --
  more than one statement, writes, `SELECT ... INTO`, `EXPLAIN
  ANALYZE`, `FOR UPDATE`, functions with effects -- and it runs on a
  session of its own, opened read-only, so the server refuses a write
  too and your own transaction is left alone. It's kept in the history.
  SQL Server has no read-only session and needs no `;` between
  statements, so there the check refuses every word that starts another
  statement, a second `SELECT` outside a `UNION`, and `NEXT VALUE FOR`.

## 1.0.0

- The first release, for Fenix 1.0: sQLite files, PostgreSQL, MySQL and MariaDB, SQL Server: the schema in a sidebar, the statement under the cursor run from a .sql buffer, results to sort, copy and export, a table's rows paged and changed, history, completion of the schema's names.
