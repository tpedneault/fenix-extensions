# Changelog

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
