//! What coding agents are given (`database_schema`, `database_query`):
//! a connection's schema as JSON, and the rows of one statement that only
//! reads -- held to that here, before it's sent, and by the server too:
//! an agent's statements run on a session of their own, opened read-only
//! (PostgreSQL's `default_transaction_read_only`, MySQL's `SET SESSION
//! TRANSACTION READ ONLY`, a SQLite file opened read-only).

use serde_json::{json, Value};

use crate::conn::Conn;
use crate::result::{Kind as CellKind, Outcome};
use crate::schema::{KeyKind, Schema, Table};
use crate::sql::{self, Dialect, Kind};

/// The most rows an agent's statement brings back, and its default.
pub const MAX_ROWS: usize = 1000;
pub const DEFAULT_ROWS: usize = 100;

/// What a statement may start with to be run for an agent.
const READS: &[&str] = &["SELECT", "WITH", "VALUES", "TABLE", "SHOW", "DESCRIBE", "DESC", "EXPLAIN"];

/// Words that, anywhere in a statement outside its strings and comments,
/// make it one an agent may not run: they change something, run
/// something else, or (EXPLAIN ANALYZE) run what they explain.
const REFUSED: &[&str] = &[
    "INTO", "INSERT", "UPDATE", "DELETE", "MERGE", "UPSERT", "TRUNCATE", "DROP", "CREATE", "ALTER", "RENAME", "GRANT", "REVOKE", "DENY", "EXEC", "EXECUTE", "CALL", "DO", "COPY", "LOAD", "IMPORT",
    "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE", "ANALYSE", "LOCK", "UNLOCK", "PRAGMA", "OUTFILE", "DUMPFILE", "SHUTDOWN", "KILL", "BACKUP", "RESTORE", "DBCC", "WAITFOR",
];

/// Functions with effects -- a sequence moved on, a backend ended, a
/// file read, a lock taken, a statement run elsewhere -- or that only
/// wait.
const REFUSED_FUNCTIONS: &[&str] = &[
    "NEXTVAL", "SETVAL", "SET_CONFIG", "PG_TERMINATE_BACKEND", "PG_CANCEL_BACKEND", "PG_RELOAD_CONF", "PG_ROTATE_LOGFILE", "PG_SLEEP", "PG_SLEEP_FOR", "PG_SLEEP_UNTIL", "PG_SWITCH_WAL", "PG_PROMOTE",
    "PG_CREATE_RESTORE_POINT", "PG_NOTIFY", "TXID_CURRENT", "LOAD_FILE", "SLEEP", "BENCHMARK", "GET_LOCK", "RELEASE_LOCK", "RELEASE_ALL_LOCKS", "LOAD_EXTENSION", "WRITEFILE", "READFILE", "EDIT",
    "OPENROWSET", "OPENQUERY", "OPENDATASOURCE", "OPENXML",
];

/// Prefixes of such functions' names: `pg_advisory_lock`, `lo_unlink`,
/// `dblink_exec`, `pg_read_file`, `xp_cmdshell`, `sp_executesql`.
const REFUSED_PREFIXES: &[&str] = &["PG_ADVISORY", "PG_TRY_ADVISORY", "LO_", "DBLINK", "PG_READ_", "PG_LS_", "PG_STAT_RESET", "PG_FILE", "PG_LOGICAL", "PG_REPLICATION", "XP_", "SP_"];

/// `text` as one statement an agent may run, without its `;` -- or why
/// not. Conservative: what can't be told to only read is refused.
pub fn read_only_statement(text: &str, dialect: Dialect) -> Result<String, String> {
    let statements = sql::split(text, dialect);
    let statement = match statements.as_slice() {
        [] => return Err("there's no statement".into()),
        [one] => one.text.trim().trim_end_matches(';').trim().to_string(),
        many => return Err(format!("one statement at a time, and this is {}", many.len())),
    };
    let words = sql::words(&statement, dialect);
    let Some((first, _)) = words.first() else { return Err("there's no statement".into()) };
    if !READS.contains(&first.as_str()) {
        return Err(format!("only SELECT (WITH, VALUES, TABLE, SHOW, DESCRIBE, EXPLAIN) runs here, and this is {first}"));
    }
    if sql::classify(&statement, dialect) != Kind::Read {
        return Err("it may change what's stored".into());
    }
    if let Some((w, _)) = words.iter().find(|(w, _)| REFUSED.contains(&w.as_str())) {
        return Err(format!("{w} isn't for a statement that only reads"));
    }
    // A function called: its name, then `(`.
    let upper = statement.to_ascii_uppercase();
    for (w, _) in &words {
        let named = REFUSED_FUNCTIONS.contains(&w.as_str()) || REFUSED_PREFIXES.iter().any(|p| w.starts_with(p));
        if named && called(&upper, w) {
            return Err(format!("{} has effects, or only waits", w.to_ascii_lowercase()));
        }
    }
    Ok(statement)
}

/// Whether `word` is followed by `(` somewhere in `upper`, as a word of
/// its own.
fn called(upper: &str, word: &str) -> bool {
    upper.match_indices(word).any(|(at, _)| {
        let before = upper[..at].chars().next_back();
        let ok_before = !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        ok_before && upper[at + word.len()..].trim_start().starts_with('(')
    })
}

fn table_json(t: &Table) -> Value {
    let mut out = json!({
        "name": t.name,
        "columns": t.columns.iter().map(|c| json!({ "name": c.name, "type": c.ty, "nullable": c.nullable })).collect::<Vec<_>>(),
    });
    if t.view {
        out["view"] = json!(true);
    }
    let key = t.primary_key();
    if !key.is_empty() {
        out["primary_key"] = json!(key);
    }
    let unique: Vec<&Vec<String>> = t.keys.iter().filter(|k| k.kind == KeyKind::Unique).map(|k| &k.columns).collect();
    if !unique.is_empty() {
        out["unique"] = json!(unique);
    }
    let foreign: Vec<Value> = t
        .keys
        .iter()
        .filter(|k| k.kind == KeyKind::Foreign)
        .map(|k| {
            let (schema, table, columns) = k.refers.clone().unwrap_or_default();
            json!({ "name": k.name, "columns": k.columns, "references": { "schema": schema, "table": table, "columns": columns } })
        })
        .collect();
    if !foreign.is_empty() {
        out["foreign_keys"] = json!(foreign);
    }
    if !t.indexes.is_empty() {
        out["indexes"] = json!(t.indexes.iter().map(|i| json!({ "name": i.name, "unique": i.unique, "columns": i.columns })).collect::<Vec<_>>());
    }
    out
}

/// A connection, as an agent is told of it.
fn conn_json(conn: &Conn) -> Value {
    json!({ "connection": conn.name, "engine": conn.engine.name(), "database": conn.database, "read_only": conn.read_only, "production": conn.production })
}

/// `database_schema`'s answer: every table, or the one `table` names
/// (`t`, `s.t`), with its columns, keys and indexes.
pub fn schema_answer(conn: &Conn, schema: &Schema, table: Option<&str>) -> Result<Value, String> {
    let mut out = conn_json(conn);
    match table.map(str::trim).filter(|t| !t.is_empty()) {
        Some(name) => {
            let Some((ns, t)) = schema.find(name) else {
                let lower = name.to_ascii_lowercase();
                let last = lower.rsplit('.').next().unwrap_or(&lower).to_string();
                let near: Vec<String> = schema.namespaces.iter().flat_map(|ns| ns.tables.iter().map(move |t| format!("{}.{}", ns.name, t.name))).filter(|n| n.to_ascii_lowercase().contains(&last)).take(10).collect();
                let hint = if near.is_empty() { String::new() } else { format!(" -- these are like it: {}", near.join(", ")) };
                return Err(format!("{} has no table {name}{hint}", conn.name));
            };
            out["schema"] = json!(ns.name);
            out["table"] = table_json(t);
        }
        None => {
            out["schemas"] = json!(schema.namespaces.iter().map(|ns| json!({ "name": ns.name, "tables": ns.tables.iter().map(table_json).collect::<Vec<_>>() })).collect::<Vec<_>>());
        }
    }
    Ok(out)
}

/// `database_query`'s answer: its rows (no more than `limit`), or how it
/// went, or the server's error.
pub fn query_answer(conn: &Conn, statement: &str, outcomes: &[Outcome], limit: usize, millis: u64) -> Result<Value, String> {
    let mut out = conn_json(conn);
    out["statement"] = json!(statement);
    out["millis"] = json!(millis);
    if let Some(Outcome::Error(e)) = outcomes.iter().find(|o| o.is_error()) {
        return Err(format!("{}: {e}", conn.name));
    }
    match outcomes.iter().find(|o| matches!(o, Outcome::Rows(_))) {
        Some(Outcome::Rows(rows)) => {
            let kept = rows.rows.len().min(limit);
            let cells: Vec<Value> = rows
                .rows
                .iter()
                .take(kept)
                .map(|r| {
                    Value::Array(
                        r.iter()
                            .zip(rows.columns.iter().map(|c| c.kind).chain(std::iter::repeat(CellKind::Text)))
                            .map(|(cell, kind)| match (cell, kind) {
                                (None, _) => Value::Null,
                                (Some(v), CellKind::Number) => v.parse::<i64>().map(Value::from).or_else(|_| v.parse::<f64>().map(Value::from)).unwrap_or_else(|_| Value::from(v.as_str())),
                                (Some(v), CellKind::Bool) => match v.as_str() {
                                    "t" | "true" | "1" => Value::Bool(true),
                                    "f" | "false" | "0" => Value::Bool(false),
                                    _ => Value::from(v.as_str()),
                                },
                                (Some(v), _) => Value::from(v.as_str()),
                            })
                            .collect(),
                    )
                })
                .collect();
            out["columns"] = json!(rows.columns.iter().map(|c| json!({ "name": c.name, "type": c.type_name })).collect::<Vec<_>>());
            out["rows"] = Value::Array(cells);
            out["row_count"] = json!(kept);
            out["truncated"] = json!(rows.rows.len() > limit || rows.truncated);
        }
        _ => {
            let summary = outcomes.iter().map(Outcome::summary).collect::<Vec<_>>().join(" · ");
            out["done"] = json!(if summary.is_empty() { "done".to_string() } else { summary });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{Column, Rows};
    use crate::schema::{ColumnInfo, Index, Key, Namespace};

    fn ok(sql: &str) -> String {
        read_only_statement(sql, Dialect::Postgres).unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    fn refused(sql: &str, dialect: Dialect) -> String {
        read_only_statement(sql, dialect).expect_err(sql)
    }

    #[test]
    fn only_a_statement_that_reads_runs() {
        assert_eq!(ok("select * from users where name = 'x';"), "select * from users where name = 'x'");
        assert_eq!(ok("-- why\nSELECT count(*) FROM t"), "-- why\nSELECT count(*) FROM t");
        ok("with recent as (select * from orders where at > now() - interval '1 day') select * from recent");
        ok("select replace(name, 'a', 'b'), updated_at, 'delete from t' as s, \"update\" from t");
        ok("explain select * from t");
        ok("values (1), (2)");
        ok("(select 1) union (select 2)");
        assert!(read_only_statement("SHOW TABLES", Dialect::Mysql).is_ok());
        assert!(read_only_statement("select [into] from [t]", Dialect::Mssql).is_ok(), "a quoted name");

        let pg = Dialect::Postgres;
        assert!(refused("select 1; drop table t", pg).contains("one statement at a time"));
        assert!(refused("", pg).contains("no statement"));
        assert!(refused("-- only a comment", pg).contains("no statement"));
        assert!(refused("delete from t", pg).contains("this is DELETE"));
        assert!(refused("update t set a = 1", pg).contains("this is UPDATE"));
        assert!(refused("insert into t values (1)", pg).contains("INSERT"));
        assert!(refused("pragma table_info(t)", Dialect::Sqlite).contains("PRAGMA"));
        assert!(refused("with x as (delete from t returning *) select * from x", pg).contains("may change"));
        assert!(refused("select * into backup from t", pg).contains("change"));
        assert!(refused("select * from t into outfile '/tmp/x'", Dialect::Mysql).contains("change") || refused("select * from t into outfile '/tmp/x'", Dialect::Mysql).contains("INTO"));
        assert!(refused("select a into @x from t", Dialect::Mysql).contains("INTO"), "even into variables");
        assert!(refused("explain analyze delete from t", pg).contains("change"));
        assert!(refused("explain analyze select * from t", pg).contains("ANALYZE"), "it runs what it explains");
        assert!(refused("select * from t for update", pg).contains("UPDATE"), "it takes locks");
        assert!(refused("select nextval('seq')", pg).contains("nextval"));
        assert!(refused("SELECT pg_terminate_backend (123)", pg).contains("pg_terminate_backend"));
        assert!(refused("select pg_advisory_lock(1)", pg).contains("pg_advisory_lock"));
        assert!(refused("select lo_unlink(42)", pg).contains("lo_unlink"));
        assert!(refused("select * from dblink('x', 'delete from t') as t(a int)", pg).contains("dblink"));
        assert!(refused("select sleep(10)", Dialect::Mysql).contains("sleep"));
        assert!(refused("select load_file('/etc/passwd')", Dialect::Mysql).contains("load_file"));
        assert!(refused("select * from openrowset('SQLNCLI', 'x', 'select 1')", Dialect::Mssql).contains("openrowset"));
        assert!(refused("exec sp_who", Dialect::Mssql).contains("EXEC"));
        assert!(refused("call refresh()", pg).contains("CALL"));
        assert!(refused("begin", pg).contains("BEGIN"));
        assert!(refused("set search_path = x", pg).contains("SET"));
        assert!(refused("copy t to '/tmp/x'", pg).contains("COPY"));
        assert!(refused("vacuum", pg).contains("VACUUM"));
        assert!(refused("select * from t; select 2", pg).contains("this is 2"));
        // A name that only contains one of the words is fine.
        ok("select nextval_count, sleeping, lo_x_col from t");
        ok("select x.sleep from t as x");
    }

    fn shop() -> (Conn, Schema) {
        let conn = Conn::from_row(&["shop", "pg", "db", "", "shop", "ada", "readonly"].map(String::from)).unwrap();
        let col = |name: &str, ty: &str, nullable| ColumnInfo { name: name.into(), ty: ty.into(), nullable };
        let orders = Table {
            name: "orders".into(),
            view: false,
            columns: vec![col("id", "int4", false), col("user_id", "int4", false), col("note", "text", true)],
            indexes: vec![Index { name: "orders_user".into(), unique: false, columns: vec!["user_id".into()] }],
            keys: vec![
                Key { name: "orders_pkey".into(), kind: KeyKind::Primary, columns: vec!["id".into()], refers: None },
                Key { name: "orders_user_fk".into(), kind: KeyKind::Foreign, columns: vec!["user_id".into()], refers: Some(("public".into(), "users".into(), vec!["id".into()])) },
            ],
        };
        let users = Table { name: "users".into(), view: false, columns: vec![col("id", "int4", false), col("email", "varchar(200)", false)], indexes: vec![], keys: vec![Key { name: "users_email".into(), kind: KeyKind::Unique, columns: vec!["email".into()], refers: None }] };
        (conn, Schema { namespaces: vec![Namespace { name: "public".into(), tables: vec![orders, users] }] })
    }

    #[test]
    fn the_schema_as_an_agent_reads_it() {
        let (conn, schema) = shop();
        let all = schema_answer(&conn, &schema, None).unwrap();
        assert_eq!((all["connection"].as_str(), all["engine"].as_str(), all["read_only"].as_bool()), (Some("shop"), Some("PostgreSQL"), Some(true)));
        let orders = &all["schemas"][0]["tables"][0];
        assert_eq!(orders["columns"][2], json!({"name": "note", "type": "text", "nullable": true}));
        assert_eq!(orders["primary_key"], json!(["id"]));
        assert_eq!(orders["foreign_keys"][0]["references"], json!({"schema": "public", "table": "users", "columns": ["id"]}));
        assert_eq!(orders["indexes"][0]["columns"], json!(["user_id"]));
        let one = schema_answer(&conn, &schema, Some("public.USERS")).unwrap();
        assert_eq!((one["schema"].as_str(), one["table"]["name"].as_str()), (Some("public"), Some("users")));
        assert_eq!(one["table"]["unique"], json!([["email"]]));
        assert!(one.get("schemas").is_none());
        assert_eq!(schema_answer(&conn, &schema, Some("user")).unwrap_err(), "shop has no table user -- these are like it: public.users");
    }

    #[test]
    fn rows_as_an_agent_reads_them() {
        let (conn, _) = shop();
        let rows = Rows {
            columns: vec![Column::new("id", "int4", CellKind::Number), Column::new("email", "varchar", CellKind::Text), Column::new("ok", "bool", CellKind::Bool), Column::new("score", "numeric", CellKind::Number)],
            rows: vec![vec![Some("1".into()), Some("a@x".into()), Some("t".into()), Some("1.5".into())], vec![Some("2".into()), None, Some("f".into()), None], vec![Some("3".into()), Some("c@x".into()), None, None]],
            truncated: false,
        };
        let answer = query_answer(&conn, "select ...", &[Outcome::Rows(rows.clone())], 2, 7).unwrap();
        assert_eq!(answer["rows"], json!([[1, "a@x", true, 1.5], [2, null, false, null]]));
        assert_eq!(answer["columns"][1], json!({"name": "email", "type": "varchar"}));
        assert_eq!((answer["row_count"].as_u64(), answer["truncated"].as_bool(), answer["millis"].as_u64()), (Some(2), Some(true), Some(7)));
        let all = query_answer(&conn, "select ...", &[Outcome::Rows(rows)], 100, 7).unwrap();
        assert_eq!(all["truncated"], json!(false));
        let done = query_answer(&conn, "explain select 1", &[Outcome::Done { tag: "EXPLAIN".into(), changed: None }], 100, 1).unwrap();
        assert_eq!(done["done"], "EXPLAIN");
        assert_eq!(query_answer(&conn, "select x", &[Outcome::Error("ERROR 42703: column \"x\" does not exist".into())], 100, 1).unwrap_err(), "shop: ERROR 42703: column \"x\" does not exist");
    }
}
