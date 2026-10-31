//! Saved connections: `database.connections`' rows (a name, the engine,
//! host and port, the database, the user, options), their passwords from
//! the system's keychain (`database.password.<name>`, or the older map
//! `database.passwords`), and the safety rules that go with each:
//! read-only, production.

use crate::sql::{self, Dialect};

/// Which engine a connection talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Sqlite,
    Postgres,
    Mysql,
    Mssql,
}

impl Engine {
    pub fn parse(s: &str) -> Option<Engine> {
        Some(match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
            "sqlite" | "sqlite3" => Engine::Sqlite,
            "postgres" | "postgresql" | "pg" => Engine::Postgres,
            "mysql" | "mariadb" => Engine::Mysql,
            "mssql" | "sqlserver" | "tsql" => Engine::Mssql,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Engine::Sqlite => "SQLite",
            Engine::Postgres => "PostgreSQL",
            Engine::Mysql => "MySQL",
            Engine::Mssql => "SQL Server",
        }
    }

    pub fn dialect(self) -> Dialect {
        match self {
            Engine::Sqlite => Dialect::Sqlite,
            Engine::Postgres => Dialect::Postgres,
            Engine::Mysql => Dialect::Mysql,
            Engine::Mssql => Dialect::Mssql,
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Sqlite => 0,
            Engine::Postgres => 5432,
            Engine::Mysql => 3306,
            Engine::Mssql => 1433,
        }
    }
}

/// TLS on a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tls {
    Off,
    /// If the server offers it (PostgreSQL's and MySQL's STARTTLS).
    Prefer,
    /// Or not at all.
    Require,
    /// From the first byte: SQL Server's TDS 8 (`encrypt=strict`).
    Strict,
}

/// A connection, as saved.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Conn {
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    /// The database; a SQLite file's path.
    pub database: String,
    pub user: String,
    pub read_only: bool,
    pub production: bool,
    pub tls: Tls,
    /// A certificate taken whoever signed it: a server of yours.
    pub insecure: bool,
}

impl Conn {
    /// A SQLite file, opened from the explorer: no row of the setting.
    pub fn file(path: &str, read_only: bool) -> Conn {
        let name = path.replace('\\', "/").rsplit('/').next().unwrap_or(path).to_string();
        Conn { name, engine: Engine::Sqlite, host: String::new(), port: 0, database: path.to_string(), user: String::new(), read_only, production: false, tls: Tls::Off, insecure: false }
    }

    /// One row of `database.connections`: name, engine, host, port,
    /// database, user, options (`readonly`, `production`, `tls=require`,
    /// `insecure`), the last ones left out if they're blank.
    pub fn from_row(row: &[String]) -> Result<Conn, String> {
        let field = |i: usize| row.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let name = field(0);
        if name.is_empty() {
            return Err("a connection without a name".into());
        }
        let engine = Engine::parse(&field(1)).ok_or_else(|| format!("{name}: engine {:?} -- sqlite, postgres, mysql or mssql", field(1)))?;
        let port = match field(3).as_str() {
            "" => engine.default_port(),
            p => p.parse().map_err(|_| format!("{name}: port {p:?} isn't a number"))?,
        };
        let mut conn = Conn {
            name,
            engine,
            host: if field(2).is_empty() && engine != Engine::Sqlite { "localhost".into() } else { field(2) },
            port,
            database: field(4),
            user: field(5),
            read_only: false,
            production: false,
            tls: match engine {
                Engine::Postgres | Engine::Mysql => Tls::Prefer,
                _ => Tls::Off,
            },
            insecure: false,
        };
        for word in field(6).split([',', ' ', ';']).map(str::trim).filter(|w| !w.is_empty()) {
            match word.to_ascii_lowercase().as_str() {
                "readonly" | "read-only" | "ro" => conn.read_only = true,
                "production" | "prod" => conn.production = true,
                "tls" | "ssl" | "tls=require" | "ssl=require" | "encrypt" => conn.tls = Tls::Require,
                "tls=prefer" | "ssl=prefer" => conn.tls = Tls::Prefer,
                "tls=off" | "ssl=off" | "notls" | "tls=disable" | "ssl=disable" => conn.tls = Tls::Off,
                "tls=strict" | "encrypt=strict" | "strict" => conn.tls = Tls::Strict,
                "insecure" | "accept-invalid" | "trust" | "trustservercertificate" => conn.insecure = true,
                other => return Err(format!("{}: option {other:?} -- readonly, production, tls=off|prefer|require|strict, insecure", conn.name)),
            }
        }
        if conn.engine == Engine::Sqlite && conn.database.is_empty() {
            return Err(format!("{}: a SQLite connection's database is its file's path", conn.name));
        }
        Ok(conn)
    }

    /// Where it is, for the sidebar: `app on db.example.com:5432`.
    pub fn place(&self) -> String {
        match self.engine {
            Engine::Sqlite => self.database.clone(),
            _ => {
                let db = if self.database.is_empty() { String::new() } else { format!("{} on ", self.database) };
                format!("{db}{}:{}", self.host, self.port)
            }
        }
    }
}

/// The rows of `database.connections` (records, as JSON: `[[name, engine,
/// ...], ...]`): each connection, or why its row isn't one.
pub fn parse_all(json: &str) -> Vec<Result<Conn, String>> {
    let json = json.trim();
    if json.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<Vec<serde_json::Value>>>(json) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| r.into_iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() })).collect::<Vec<_>>())
            .filter(|r| r.iter().any(|f| !f.trim().is_empty()))
            .map(|r| Conn::from_row(&r))
            .collect(),
        Err(e) => vec![Err(format!("database.connections: {e}"))],
    }
}

/// The password for connection `name` from `database.passwords` (a map,
/// read as JSON -- `[["name", "password"], ...]` or `{"name": "..."}`).
pub fn password(json: &str, name: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    match v {
        serde_json::Value::Object(map) => map.get(name).and_then(|p| p.as_str()).map(str::to_string),
        serde_json::Value::Array(rows) => rows.iter().find_map(|r| {
            let r = r.as_array()?;
            (r.first()?.as_str()? == name).then(|| r.get(1).and_then(|p| p.as_str()).unwrap_or("").to_string())
        }),
        _ => None,
    }
}

/// The secret connection `name`'s password is kept as, in the system's
/// keychain: `database.password.<name>`, what isn't a letter, a digit,
/// `-` or `_` written `%XX` (UTF-8), so no two names share one and none
/// strays past the pattern (`database.password.*`).
pub fn secret_key(name: &str) -> String {
    let mut key = String::from("database.password.");
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            key.push(b as char);
        } else {
            key.push_str(&format!("%{b:02X}"));
        }
    }
    key
}

/// Whether a connection failed for its password: the server said the
/// sign-in was refused (PostgreSQL's 28P01, MySQL's 1045, SQL
/// Server's 18456), so a password kept for it is wrong.
pub fn refused_sign_in(why: &str) -> bool {
    [" 28P01:", "ERROR 1045 ", "Msg 18456,"].iter().any(|code| why.contains(code))
}

/// Whether statements may run on a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Run,
    /// Asked first: why.
    Ask(String),
    Refuse(String),
}

/// What the safety rules say of `statements` on `conn`: on a read-only
/// one, nothing that changes what's stored runs; on a production one it's
/// asked first; and on any, an UPDATE or DELETE of every row is asked
/// first.
pub fn check(conn: &Conn, statements: &[&str]) -> Verdict {
    let d = conn.engine.dialect();
    let first_word = |s: &str| sql::strip_leading(s, d).split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("").to_ascii_uppercase();
    let changing: Vec<&str> = statements.iter().copied().filter(|s| sql::classify(s, d).changes()).collect();
    if let Some(s) = changing.first() {
        if conn.read_only {
            return Verdict::Refuse(format!("{} is read-only: {} changes what's stored", conn.name, first_word(s)));
        }
    }
    let every: Vec<&str> = statements.iter().copied().filter(|s| sql::every_row(s, d)).collect();
    if !changing.is_empty() && conn.production {
        let what = if changing.len() == 1 { format!("{} changes", first_word(changing[0])) } else { format!("{} statements change", changing.len()) };
        let all = if every.is_empty() { "" } else { ", every row of a table" };
        return Verdict::Ask(format!("{} is production: {what} what's stored{all}", conn.name));
    }
    if let Some(s) = every.first() {
        return Verdict::Ask(format!("{} with no WHERE: every row of the table", first_word(s)));
    }
    Verdict::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(fields: &[&str]) -> Vec<String> {
        fields.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rows_become_connections() {
        let c = Conn::from_row(&row(&["prod", "PostgreSQL", "db.example.com", "", "app", "me", "production, tls=require"])).unwrap();
        assert_eq!((c.engine, c.port, c.production, c.read_only, c.tls), (Engine::Postgres, 5432, true, false, Tls::Require));
        assert_eq!(c.place(), "app on db.example.com:5432");
        let c = Conn::from_row(&row(&["local", "mariadb"])).unwrap();
        assert_eq!((c.engine, c.host.as_str(), c.port, c.tls), (Engine::Mysql, "localhost", 3306, Tls::Prefer));
        let c = Conn::from_row(&row(&["dw", "sql server", "dw", "1433", "", "sa", "ro strict insecure"])).unwrap();
        assert_eq!((c.engine, c.read_only, c.tls, c.insecure), (Engine::Mssql, true, Tls::Strict, true));
        assert!(Conn::from_row(&row(&["x", "oracle"])).unwrap_err().contains("engine"));
        assert!(Conn::from_row(&row(&["x", "pg", "h", "abc"])).unwrap_err().contains("port"));
        assert!(Conn::from_row(&row(&["x", "sqlite"])).unwrap_err().contains("path"));
        assert!(Conn::from_row(&row(&["x", "pg", "", "", "", "", "turbo"])).unwrap_err().contains("turbo"));
        let all = parse_all(r#"[["a","sqlite","","","/project/a.db"],["","","","","","",""],["b","pg","h",5433]]"#);
        assert_eq!(all.len(), 2, "a blank row is left out");
        assert_eq!(all[1].as_ref().unwrap().port, 5433, "a number as a number");
        assert_eq!(Conn::file("C:\\data\\shop.sqlite", true).name, "shop.sqlite");
    }

    #[test]
    fn passwords_come_from_the_map() {
        assert_eq!(password(r#"[["prod","s3cret"],["dev",""]]"#, "prod").as_deref(), Some("s3cret"));
        assert_eq!(password(r#"[["dev",""]]"#, "dev").as_deref(), Some(""));
        assert_eq!(password(r#"{"prod":"x"}"#, "prod").as_deref(), Some("x"));
        assert_eq!(password("[]", "prod"), None);
        assert_eq!(password("not json", "prod"), None);
    }

    #[test]
    fn a_password_is_kept_under_its_connection_name() {
        assert_eq!(secret_key("prod"), "database.password.prod");
        assert_eq!(secret_key("shop db/eu"), "database.password.shop%20db%2Feu");
        assert_eq!(secret_key("a.b"), "database.password.a%2Eb");
        assert_eq!(secret_key("café"), "database.password.caf%C3%A9");
        assert_ne!(secret_key("a b"), secret_key("a_b"));
        assert_ne!(secret_key("a%20b"), secret_key("a b"));
    }

    #[test]
    fn a_refused_sign_in_is_told_from_other_failures() {
        assert!(refused_sign_in("FATAL 28P01: password authentication failed for user \"ada\""));
        assert!(refused_sign_in("ERROR 1045 (28000): Access denied for user 'ada'@'172.17.0.1' (using password: YES)"));
        assert!(refused_sign_in("Msg 18456, level 14, line 1: Login failed for user 'ada'."));
        assert!(!refused_sign_in("localhost:5432: connection refused"));
        assert!(!refused_sign_in("FATAL 28000: no pg_hba.conf entry for host \"10.0.0.2\""), "not the password's fault");
        assert!(!refused_sign_in("FATAL 3D000: database \"x\" does not exist"));
    }

    #[test]
    fn the_safety_rules() {
        let mut c = Conn::from_row(&row(&["prod", "pg"])).unwrap();
        assert_eq!(check(&c, &["select 1", "update t set a = 1 where id = 2"]), Verdict::Run);
        assert_eq!(check(&c, &["delete from t"]), Verdict::Ask("DELETE with no WHERE: every row of the table".into()));
        c.production = true;
        assert_eq!(check(&c, &["select 1"]), Verdict::Run);
        assert_eq!(check(&c, &["update t set a = 1 where id = 2"]), Verdict::Ask("prod is production: UPDATE changes what's stored".into()));
        assert_eq!(check(&c, &["drop table t", "delete from u"]), Verdict::Ask("prod is production: 2 statements change what's stored, every row of a table".into()));
        assert_eq!(check(&c, &["call refresh()"]), Verdict::Ask("prod is production: CALL changes what's stored".into()), "it can't tell: asked");
        c.read_only = true;
        assert_eq!(check(&c, &["-- why\ninsert into t values (1)"]), Verdict::Refuse("prod is read-only: INSERT changes what's stored".into()));
        assert_eq!(check(&c, &["begin", "select 1", "commit"]), Verdict::Run);
    }
}
