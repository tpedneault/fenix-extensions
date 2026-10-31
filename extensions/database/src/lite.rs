//! SQLite files, through Turso's engine (`turso_core`): SQLite in Rust,
//! which builds for wasm32-wasip2 with no C compiler and reads and writes
//! the file through `std::fs`. The file's opened read-only; a statement
//! that changes it opens it to write for as long as that takes. Turso
//! writes through a WAL, so a file that kept a rollback journal is put
//! back as it was after: the log checkpointed into it and gone, its
//! header saying so again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use turso_core::{ConnectOpts, Connection, Database, OpenFlags, OpenOptions, PlatformIO, StepResult, Value, IO};

use crate::result::{hex, Column, Kind, Outcome, Rows};
use crate::schema::{ColumnInfo, Index, Key, KeyKind, Namespace, Schema, Table};
use crate::sql::{self, Dialect};

/// How long a statement may run, in milliseconds: `database.timeout`,
/// within the minute Fenix gives a call (`limits.call_ms`).
static TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_TIMEOUT_MS);

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// The most: what's left of the call's minute for the rest.
pub const MAX_TIMEOUT_MS: u64 = 55_000;

/// How long a statement may run from now on, held to 1 s to 55 s.
pub fn set_timeout(ms: u64) {
    TIMEOUT_MS.store(ms.clamp(1000, MAX_TIMEOUT_MS), Ordering::Relaxed);
}

pub fn timeout() -> Duration {
    Duration::from_millis(TIMEOUT_MS.load(Ordering::Relaxed))
}

pub struct Lite {
    /// Where it's opened: under `/project`, or a copy under `/data`.
    pub path: String,
    /// It may not be written: a copy, or a read-only connection.
    pub read_only: bool,
    db: Option<(Arc<Database>, Arc<Connection>)>,
}

fn open(path: &str, write: bool) -> Result<(Arc<Database>, Arc<Connection>), String> {
    let io: Arc<dyn IO> = Arc::new(PlatformIO::new().map_err(|e| e.to_string())?);
    let mut options = OpenOptions::new(Arc::new(turso_core::dialect::SqliteDialect));
    if !write {
        options = options.flags(OpenFlags::ReadOnly);
    }
    let db = Database::open(io, path, options).map_err(|e| format!("{path}: {e}"))?;
    let conn = db.connect(ConnectOpts::new()).map_err(|e| e.to_string())?;
    conn.set_query_timeout(timeout());
    Ok((db, conn))
}

/// A value as a cell, and the kind it is.
fn cell(v: &Value) -> (Option<String>, Option<Kind>) {
    match v {
        Value::Null => (None, None),
        Value::Numeric(_) => (Some(v.to_string()), Some(Kind::Number)),
        Value::Text(t) => (Some(t.as_str().to_string()), Some(Kind::Text)),
        Value::Blob(b) => (Some(hex(b)), Some(Kind::Bytes)),
    }
}

/// A declared type's affinity, as SQLite reads it.
fn affinity(decl: &str) -> Kind {
    let d = decl.to_ascii_uppercase();
    if d.contains("INT") || ["REAL", "FLOA", "DOUB", "NUMERIC", "DECIMAL"].iter().any(|w| d.contains(w)) || d == "BOOLEAN" {
        Kind::Number
    } else if d.contains("BLOB") {
        Kind::Bytes
    } else {
        Kind::Text
    }
}

impl Lite {
    pub fn open(path: &str, read_only: bool) -> Result<Lite, String> {
        let db = open(path, false)?;
        Ok(Lite { path: path.to_string(), read_only, db: Some(db) })
    }

    fn conn(&mut self) -> Result<Arc<Connection>, String> {
        if self.db.is_none() {
            self.db = Some(open(&self.path, false)?);
        }
        Ok(self.db.as_ref().map(|d| d.1.clone()).expect("opened"))
    }

    /// One statement on `conn`, its rows up to `max_rows`.
    fn statement(conn: &Arc<Connection>, text: &str, max_rows: usize) -> Outcome {
        let mut stmt = match conn.prepare(text) {
            Ok(s) => s,
            Err(e) => return Outcome::Error(e.to_string()),
        };
        let n = stmt.num_columns();
        let mut columns: Vec<Column> = (0..n)
            .map(|i| {
                let decl = stmt.get_column_decltype(i).unwrap_or_default();
                Column::new(stmt.get_column_name(i).into_owned(), decl.to_ascii_lowercase(), if decl.is_empty() { Kind::Text } else { affinity(&decl) })
            })
            .collect();
        let mut seen: Vec<Option<Kind>> = vec![None; n];
        let mut rows = Rows::default();
        loop {
            match stmt.step() {
                Ok(StepResult::Row) => {
                    if rows.rows.len() >= max_rows.max(1) {
                        rows.truncated = true;
                        break;
                    }
                    let Some(row) = stmt.row() else { break };
                    let cells = (0..n)
                        .map(|i| {
                            let (c, k) = cell(row.get_value(i));
                            if seen[i].is_none() {
                                seen[i] = k;
                            }
                            c
                        })
                        .collect();
                    rows.rows.push(cells);
                }
                Ok(StepResult::Done) => break,
                Ok(StepResult::IO | StepResult::Yield | StepResult::Sleep { .. }) => {
                    if let Err(e) = stmt._io().step() {
                        return Outcome::Error(e.to_string());
                    }
                }
                Ok(StepResult::Busy) => return Outcome::Error("the file is locked: another program is writing it".into()),
                Ok(StepResult::Interrupt) => return Outcome::Error(format!("it took longer than {:.0} s, the most a statement may run (database.timeout)", timeout().as_secs_f32())),
                Err(e) => return Outcome::Error(e.to_string()),
            }
        }
        if n == 0 {
            let changed = stmt.n_change().max(0) as u64;
            return Outcome::Done { tag: String::new(), changed: Some(changed) };
        }
        // An expression's column has no declared type: its values say.
        for (c, k) in columns.iter_mut().zip(seen) {
            if c.type_name.is_empty() {
                c.kind = k.unwrap_or_default();
            }
        }
        rows.columns = columns;
        Outcome::Rows(rows)
    }

    /// `text`'s statements, in turn, until one fails.
    pub fn run(&mut self, text: &str, max_rows: usize) -> Vec<Outcome> {
        let statements = sql::split(text, Dialect::Sqlite);
        let writes = statements.iter().any(|s| sql::classify(&s.text, Dialect::Sqlite).changes());
        if writes && !self.read_only {
            return self.write(&statements.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), max_rows);
        }
        let conn = match self.conn() {
            Ok(c) => c,
            Err(e) => return vec![Outcome::Error(e)],
        };
        let mut out = Vec::new();
        for s in &statements {
            let o = Self::statement(&conn, &s.text, max_rows);
            let failed = o.is_error();
            out.push(o);
            if failed {
                break;
            }
        }
        out
    }

    /// Statements that change the file: run on it opened to write, then
    /// the log checkpointed, and the journal put back as it was.
    fn write(&mut self, statements: &[&str], max_rows: usize) -> Vec<Outcome> {
        // Let go of the read-only handle first: Turso keeps one database
        // per file.
        self.db = None;
        let journal = std::fs::read(&self.path).ok().and_then(|b| b.get(18..20).map(|v| (v[0], v[1])));
        let (db, conn) = match open(&self.path, true) {
            Ok(d) => d,
            Err(e) => return vec![Outcome::Error(e)],
        };
        let mut out = Vec::new();
        for s in statements {
            let o = Self::statement(&conn, s, max_rows);
            let failed = o.is_error();
            out.push(o);
            if failed {
                break;
            }
        }
        let _ = Self::statement(&conn, "PRAGMA wal_checkpoint(TRUNCATE)", 1);
        let _ = conn.close();
        drop(conn);
        drop(db);
        if journal == Some((1, 1)) {
            if let Err(e) = restore_journal(&self.path) {
                out.push(Outcome::Error(format!("the file's journal: {e}")));
            }
        }
        out
    }

    /// The file's tables and views, their columns, indexes and keys.
    pub fn schema(&mut self) -> Result<Schema, String> {
        let conn = self.conn()?;
        let rows = |sql: &str| -> Result<Rows, String> {
            match Self::statement(&conn, sql, 100_000) {
                Outcome::Rows(r) => Ok(r),
                Outcome::Error(e) => Err(e),
                Outcome::Done { .. } => Ok(Rows::default()),
            }
        };
        let get = |r: &[Option<String>], i: usize| r.get(i).cloned().flatten().unwrap_or_default();
        let mut ns = Namespace { name: "main".into(), tables: Vec::new() };
        let objects = rows("SELECT name, type FROM sqlite_schema WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name")?;
        for o in &objects.rows {
            let name = get(o, 0);
            let q = Dialect::Sqlite.ident(&name);
            let mut table = Table { name: name.clone(), view: get(o, 1) == "view", ..Table::default() };
            let info = rows(&format!("PRAGMA table_info({q})"))?;
            let mut pk: Vec<(i64, String)> = Vec::new();
            for c in &info.rows {
                let col = get(c, 1);
                let order: i64 = get(c, 5).parse().unwrap_or(0);
                if order > 0 {
                    pk.push((order, col.clone()));
                }
                table.columns.push(ColumnInfo { name: col, ty: get(c, 2).to_ascii_lowercase(), nullable: get(c, 3) != "1" });
            }
            pk.sort();
            if !pk.is_empty() {
                table.keys.push(Key { name: "primary key".into(), kind: KeyKind::Primary, columns: pk.into_iter().map(|p| p.1).collect(), refers: None });
            }
            if !table.view {
                for i in &rows(&format!("PRAGMA index_list({q})"))?.rows {
                    let iname = get(i, 1);
                    let cols = rows(&format!("PRAGMA index_info({})", Dialect::Sqlite.ident(&iname)))?.rows.iter().map(|c| get(c, 2)).collect();
                    table.indexes.push(Index { name: iname, unique: get(i, 2) == "1", columns: cols });
                }
                for f in &rows(&format!("PRAGMA foreign_key_list({q})"))?.rows {
                    let id = format!("foreign key {}", get(f, 0));
                    let (to_table, from, to) = (get(f, 2), get(f, 3), get(f, 4));
                    match table.keys.iter_mut().find(|k| k.name == id) {
                        Some(k) => {
                            k.columns.push(from);
                            if let Some((_, _, cols)) = &mut k.refers {
                                cols.push(to);
                            }
                        }
                        None => table.keys.push(Key { name: id, kind: KeyKind::Foreign, columns: vec![from], refers: Some(("main".into(), to_table, vec![to])) }),
                    }
                }
            }
            ns.tables.push(table);
        }
        Ok(Schema { namespaces: vec![ns] })
    }
}

/// A file Turso moved to WAL put back to a rollback journal, as SQLite's
/// `journal_mode=DELETE` does: the log, checkpointed and empty, removed,
/// and the header's two version bytes (18, 19) back at 1.
fn restore_journal(path: &str) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};
    let wal = format!("{path}-wal");
    if let Ok(meta) = std::fs::metadata(&wal) {
        if meta.len() > 0 {
            return Err("its write-ahead log wasn't emptied into it, so it's left in WAL mode".into());
        }
        std::fs::remove_file(&wal).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(18)).map_err(|e| e.to_string())?;
    f.write_all(&[1, 1]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file as SQLite itself makes them: a rollback journal, a table
    /// with a key, a foreign key, an index, a view.
    fn sample(dir: &std::path::Path) -> String {
        let path = dir.join("shop.db");
        let status = std::process::Command::new("python3")
            .args([
                "-I",
                "-c",
                "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.executescript('''create table users(id integer primary key, name text not null, email text unique, score real); create table orders(id integer primary key, user_id integer references users(id), total numeric, note blob); create index orders_user on orders(user_id); create view names as select name from users; insert into users values (1, 'ada', 'a@x', 1.5), (2, 'grace', null, 2); insert into orders values (1, 1, 9.99, x'0102');'''); c.commit()",
                path.to_str().unwrap(),
            ])
            .status();
        assert!(status.is_ok_and(|s| s.success()), "python3 makes the sample");
        path.to_string_lossy().into_owned()
    }

    fn python(path: &str, code: &str) -> String {
        let out = std::process::Command::new("python3").args(["-I", "-c", &format!("import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print({code})"), path]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn the_timeout_is_held_within_the_call() {
        set_timeout(5);
        assert_eq!(timeout(), Duration::from_secs(1));
        set_timeout(600_000);
        assert_eq!(timeout(), Duration::from_millis(MAX_TIMEOUT_MS));
        set_timeout(DEFAULT_TIMEOUT_MS);
        assert_eq!(timeout(), Duration::from_secs(30));
    }

    #[test]
    fn a_file_is_read_and_its_schema_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample(dir.path());
        let mut db = Lite::open(&path, false).unwrap();
        let out = db.run("select id, name, score, 1 + 1 as two from users order by id; select note from orders", 10);
        let Outcome::Rows(r) = &out[0] else { panic!("{out:?}") };
        assert_eq!(r.columns.iter().map(|c| (c.name.as_str(), c.kind)).collect::<Vec<_>>(), [("id", Kind::Number), ("name", Kind::Text), ("score", Kind::Number), ("two", Kind::Number)]);
        assert_eq!(r.rows[1], [Some("2".into()), Some("grace".into()), Some("2.0".into()), Some("2".into())]);
        let Outcome::Rows(r) = &out[1] else { panic!() };
        assert_eq!(r.rows[0][0].as_deref(), Some("\\x0102"));
        let s = db.schema().unwrap();
        let users = s.table("main", "users").unwrap();
        assert_eq!(users.primary_key(), ["id"]);
        assert!(!users.column("name").unwrap().nullable);
        let orders = s.table("main", "orders").unwrap();
        assert_eq!(orders.indexes.iter().map(|i| (i.name.as_str(), i.columns.clone())).collect::<Vec<_>>(), [("orders_user", vec!["user_id".to_string()])]);
        assert_eq!(orders.keys[1].refers, Some(("main".into(), "users".into(), vec!["id".into()])));
        assert!(s.table("main", "names").unwrap().view);
        let out = db.run("select nope from users", 10);
        assert!(matches!(&out[..], [Outcome::Error(_)]));
        let out = db.run("select * from users", 1);
        assert!(matches!(&out[..], [Outcome::Rows(r)] if r.truncated && r.rows.len() == 1));
    }

    #[test]
    fn a_change_is_written_and_the_journal_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample(dir.path());
        let mut db = Lite::open(&path, false).unwrap();
        let out = db.run("update users set score = 3 where id = 2", 10);
        assert_eq!(out, [Outcome::Done { tag: String::new(), changed: Some(1) }]);
        assert_eq!(
            python(&path, "c.execute('select score from users where id=2').fetchone()[0], c.execute('pragma journal_mode').fetchone()[0], c.execute('pragma integrity_check').fetchone()[0]"),
            "3.0 delete ok"
        );
        assert!(!std::path::Path::new(&format!("{path}-wal")).exists());
        // And it reads on after.
        assert!(matches!(&db.run("select count(*) from users", 10)[..], [Outcome::Rows(r)] if r.rows[0][0].as_deref() == Some("2")));
        // Read-only: refused by the file itself.
        let mut ro = Lite::open(&path, true).unwrap();
        assert!(matches!(&ro.run("delete from users", 10)[..], [Outcome::Error(_)]));
        assert_eq!(python(&path, "c.execute('select count(*) from users').fetchone()[0]"), "2");
    }
}
