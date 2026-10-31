//! What a table view asks: a page of a table's rows, how many there are,
//! and a cell changed -- an `UPDATE` keyed by the row's primary key (or
//! SQLite's rowid), shown to you before it runs.

use crate::result::{Cell, Kind};
use crate::sql::Dialect;

/// A value as a literal: NULL, a number as it is, else a string.
pub fn literal(d: Dialect, value: &Cell, kind: Kind) -> String {
    match value {
        None => "NULL".into(),
        Some(v) if kind == Kind::Number && is_number(v) => v.trim().to_string(),
        Some(v) if kind == Kind::Bool && d == Dialect::Postgres && matches!(v.to_ascii_lowercase().as_str(), "t" | "f" | "true" | "false") => {
            if v.starts_with(['t', 'T']) {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Some(v) => d.string(v),
    }
}

/// A plain decimal number: digits, a sign, a point, an exponent.
fn is_number(s: &str) -> bool {
    let s = s.trim();
    let digits = s.trim_start_matches(['-', '+']);
    !digits.is_empty() && digits.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.') && s.parse::<f64>().is_ok_and(f64::is_finite) && !s.contains(['x', 'X', 'n', 'N', 'i', 'I'])
}

/// The table a view shows: where it is, and which columns key its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub dialect: Dialect,
    pub schema: Option<String>,
    pub table: String,
    /// The primary key's columns, in order; empty when it has none.
    pub key: Vec<String>,
    /// SQLite's rowid stands in for a missing key: the view selects it
    /// as its first column.
    pub rowid: bool,
}

/// The column a SQLite view selects the rowid as.
pub const ROWID: &str = "rowid";

impl Target {
    pub fn name(&self) -> String {
        self.dialect.table(self.schema.as_deref(), &self.table)
    }

    /// The columns rows are told apart by.
    pub fn keyed_by(&self) -> Vec<String> {
        if self.rowid {
            vec![ROWID.into()]
        } else {
            self.key.clone()
        }
    }

    /// Page `page` of its rows, `size` a page, those `filter` (a WHERE's
    /// condition, as you wrote it) lets through, by the key's order.
    pub fn select(&self, filter: &str, page: usize, size: usize) -> String {
        let d = self.dialect;
        let what = if self.rowid { format!("{ROWID} AS {}, *", d.ident(ROWID)) } else { "*".into() };
        let filter = filter.trim();
        let filter = filter.strip_prefix("where ").or_else(|| filter.strip_prefix("WHERE ")).unwrap_or(filter);
        let where_ = if filter.is_empty() { String::new() } else { format!(" WHERE {filter}") };
        let order: Vec<String> = self.keyed_by().iter().map(|k| d.ident(k)).collect();
        let offset = page * size;
        match d {
            Dialect::Mssql => {
                let order = if order.is_empty() { "(SELECT NULL)".to_string() } else { order.join(", ") };
                format!("SELECT {what} FROM {}{where_} ORDER BY {order} OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY", self.name())
            }
            _ => {
                let order = if order.is_empty() { String::new() } else { format!(" ORDER BY {}", order.join(", ")) };
                format!("SELECT {what} FROM {}{where_}{order} LIMIT {size} OFFSET {offset}", self.name())
            }
        }
    }

    /// How many rows `filter` lets through.
    pub fn count(&self, filter: &str) -> String {
        let filter = filter.trim();
        let filter = filter.strip_prefix("where ").or_else(|| filter.strip_prefix("WHERE ")).unwrap_or(filter);
        let where_ = if filter.is_empty() { String::new() } else { format!(" WHERE {filter}") };
        format!("SELECT COUNT(*) FROM {}{where_}", self.name())
    }

    /// Column `column` of the row keyed by `key` (its columns' values, as
    /// `keyed_by` names them) set to `value`.
    pub fn update(&self, column: &str, value: &Cell, kind: Kind, key: &[(Cell, Kind)]) -> Result<String, String> {
        let names = self.keyed_by();
        if names.is_empty() {
            return Err(format!("{} has no primary key: there's no telling its rows apart to change one", self.table));
        }
        if names.len() != key.len() {
            return Err("the row's key isn't in the view".into());
        }
        let d = self.dialect;
        let mut conds = Vec::new();
        for (name, (v, k)) in names.iter().zip(key) {
            match v {
                None => return Err(format!("the row's {name} is NULL: it can't be found by it")),
                v => conds.push(format!("{} = {}", d.ident(name), literal(d, v, *k))),
            }
        }
        Ok(format!("UPDATE {} SET {} = {} WHERE {}", self.name(), d.ident(column), literal(d, value, kind), conds.join(" AND ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users(d: Dialect) -> Target {
        Target { dialect: d, schema: Some("app".into()), table: "users".into(), key: vec!["id".into()], rowid: false }
    }

    #[test]
    fn a_cell_changed_is_an_update_keyed_by_the_primary_key() {
        let t = users(Dialect::Postgres);
        let num = |s: &str| (Some(s.to_string()), Kind::Number);
        assert_eq!(t.update("name", &Some("O'Hara".into()), Kind::Text, &[num("7")]).unwrap(), "UPDATE \"app\".\"users\" SET \"name\" = 'O''Hara' WHERE \"id\" = 7");
        assert_eq!(t.update("score", &None, Kind::Number, &[num("7")]).unwrap(), "UPDATE \"app\".\"users\" SET \"score\" = NULL WHERE \"id\" = 7");
        assert_eq!(t.update("ok", &Some("t".into()), Kind::Bool, &[num("7")]).unwrap(), "UPDATE \"app\".\"users\" SET \"ok\" = TRUE WHERE \"id\" = 7");
        assert_eq!(
            t.update("score", &Some("1e3; drop table x".into()), Kind::Number, &[num("7")]).unwrap(),
            "UPDATE \"app\".\"users\" SET \"score\" = '1e3; drop table x' WHERE \"id\" = 7",
            "not a number: quoted"
        );
        let two = Target { key: vec!["a".into(), "b".into()], ..users(Dialect::Mysql) };
        assert_eq!(two.update("c", &Some("x\\y".into()), Kind::Text, &[num("1"), (Some("k".into()), Kind::Text)]).unwrap(), "UPDATE `app`.`users` SET `c` = 'x\\\\y' WHERE `a` = 1 AND `b` = 'k'");
        let ms = Target { schema: Some("dbo".into()), ..users(Dialect::Mssql) };
        assert_eq!(ms.update("name", &Some("é".into()), Kind::Text, &[num("2")]).unwrap(), "UPDATE [dbo].[users] SET [name] = N'é' WHERE [id] = 2");
        let none = Target { key: vec![], ..users(Dialect::Postgres) };
        assert!(none.update("name", &None, Kind::Text, &[]).unwrap_err().contains("no primary key"));
        assert!(t.update("name", &None, Kind::Text, &[(None, Kind::Number)]).unwrap_err().contains("NULL"));
        let lite = Target { dialect: Dialect::Sqlite, schema: None, table: "notes".into(), key: vec![], rowid: true };
        assert_eq!(lite.update("body", &Some("x".into()), Kind::Text, &[num("12")]).unwrap(), "UPDATE \"notes\" SET \"body\" = 'x' WHERE \"rowid\" = 12");
    }

    #[test]
    fn pages_of_rows_and_their_count() {
        let t = users(Dialect::Postgres);
        assert_eq!(t.select("", 0, 100), "SELECT * FROM \"app\".\"users\" ORDER BY \"id\" LIMIT 100 OFFSET 0");
        assert_eq!(t.select("where age > 3", 2, 50), "SELECT * FROM \"app\".\"users\" WHERE age > 3 ORDER BY \"id\" LIMIT 50 OFFSET 100");
        assert_eq!(t.count("age > 3"), "SELECT COUNT(*) FROM \"app\".\"users\" WHERE age > 3");
        let ms = Target { key: vec![], ..users(Dialect::Mssql) };
        assert_eq!(ms.select("", 1, 10), "SELECT * FROM [app].[users] ORDER BY (SELECT NULL) OFFSET 10 ROWS FETCH NEXT 10 ROWS ONLY");
        let lite = Target { dialect: Dialect::Sqlite, schema: None, table: "notes".into(), key: vec![], rowid: true };
        assert_eq!(lite.select("", 0, 5), "SELECT rowid AS \"rowid\", * FROM \"notes\" ORDER BY \"rowid\" LIMIT 5 OFFSET 0");
    }
}
