//! Completion in `.sql` buffers from the connected schema: tables (and
//! schemas) where a table goes, a table's columns after `table.` or
//! `alias.`, and the columns of the statement's tables anywhere else.

use crate::schema::Schema;
use crate::sql::{self, Dialect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Schema,
    Table,
    View,
    Column,
    Keyword,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    pub what: What,
    /// A column's type and table; a table's schema.
    pub detail: String,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "FULL",
    "CROSS",
    "ON",
    "USING",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "AS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "UNION",
    "ALL",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "VIEW",
    "INDEX",
    "ALTER",
    "DROP",
    "WITH",
    "RETURNING",
    "ASC",
    "DESC",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
];

/// The words that name a table after them.
const TABLE_AFTER: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "DESCRIBE", "DESC"];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The tables a statement names, and their aliases: `from users u join
/// orders as o` gives (users, u), (orders, o).
pub fn tables_in(statement: &str) -> Vec<(String, Option<String>)> {
    let unquote = |s: &str| s.trim_matches(['"', '`', '[', ']']).to_string();
    let tokens: Vec<String> = statement.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == ';').filter(|t| !t.is_empty()).map(str::to_string).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let up = tokens[i].to_ascii_uppercase();
        if TABLE_AFTER.contains(&up.as_str()) {
            if let Some(name) = tokens.get(i + 1) {
                let name = name.split('.').map(unquote).collect::<Vec<_>>().join(".");
                let mut alias = None;
                let mut j = i + 2;
                if tokens.get(j).is_some_and(|t| t.eq_ignore_ascii_case("AS")) {
                    j += 1;
                }
                if let Some(a) = tokens.get(j) {
                    let a_up = a.to_ascii_uppercase();
                    if a.chars().all(is_word)
                        && !KEYWORDS.contains(&a_up.as_str())
                        && !matches!(a_up.as_str(), "WHERE" | "SET" | "ON" | "LEFT" | "RIGHT" | "INNER" | "GROUP" | "ORDER" | "LIMIT" | "JOIN" | "USING" | "VALUES")
                    {
                        alias = Some(unquote(a));
                    }
                }
                if !name.is_empty() && !name.starts_with('(') && name.chars().all(|c| is_word(c) || c == '.') {
                    out.push((name, alias));
                }
            }
        }
        i += 1;
    }
    out
}

/// What completes at byte `at` of `text`: the word being typed there and
/// its candidates, from `schema` (none: only keywords).
pub fn complete(schema: Option<&Schema>, text: &str, at: usize, dialect: Dialect) -> (String, Vec<Item>) {
    let at = at.min(text.len());
    let before = &text[..at];
    let word_start = before.char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(i, _)| i);
    let word = &before[word_start..];
    let qualifier = before[..word_start].strip_suffix('.').map(|q| {
        let start = q.char_indices().rev().take_while(|(_, c)| is_word(*c) || matches!(c, '"' | '`' | '[' | ']')).last().map_or(q.len(), |(i, _)| i);
        q[start..].trim_matches(['"', '`', '[', ']']).to_string()
    });
    let statement = sql::statement_at(text, at, dialect).map(|s| s.text).unwrap_or_default();
    let mentioned = tables_in(&statement);
    let mut items = Vec::new();
    let low = word.to_lowercase();
    let starts = |s: &str| s.to_lowercase().starts_with(&low);
    if let Some(schema) = schema {
        if let Some(q) = &qualifier {
            // A schema's tables, or a table's (or an alias's) columns.
            if let Some(ns) = schema.namespaces.iter().find(|n| n.name.eq_ignore_ascii_case(q)) {
                for t in ns.tables.iter().filter(|t| starts(&t.name)) {
                    items.push(Item { label: t.name.clone(), what: if t.view { What::View } else { What::Table }, detail: ns.name.clone() });
                }
            }
            let table = mentioned.iter().find(|(_, a)| a.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(q))).map(|(t, _)| t.clone()).unwrap_or_else(|| q.clone());
            if let Some((_, t)) = schema.find(&table) {
                for c in t.columns.iter().filter(|c| starts(&c.name)) {
                    items.push(Item { label: c.name.clone(), what: What::Column, detail: format!("{} · {}", c.ty, t.name) });
                }
            }
            return (word.to_string(), items);
        }
        let prev = before[..word_start].split_whitespace().last().unwrap_or("").to_ascii_uppercase();
        let tables_here = TABLE_AFTER.contains(&prev.as_str());
        // Columns first where they go, tables first after FROM.
        if !tables_here {
            for (name, _) in &mentioned {
                if let Some((_, t)) = schema.find(name) {
                    for c in t.columns.iter().filter(|c| starts(&c.name)) {
                        if !items.iter().any(|i: &Item| i.label == c.name && i.what == What::Column) {
                            items.push(Item { label: c.name.clone(), what: What::Column, detail: format!("{} · {}", c.ty, t.name) });
                        }
                    }
                }
            }
        }
        for ns in &schema.namespaces {
            for t in ns.tables.iter().filter(|t| starts(&t.name)) {
                items.push(Item { label: t.name.clone(), what: if t.view { What::View } else { What::Table }, detail: ns.name.clone() });
            }
            if schema.namespaces.len() > 1 && starts(&ns.name) {
                items.push(Item { label: ns.name.clone(), what: What::Schema, detail: "schema".into() });
            }
        }
        if tables_here {
            return (word.to_string(), items);
        }
    }
    if !word.is_empty() {
        let upper = word.chars().next().is_some_and(char::is_uppercase);
        for k in KEYWORDS.iter().filter(|k| starts(k)) {
            items.push(Item { label: if upper { k.to_string() } else { k.to_lowercase() }, what: What::Keyword, detail: String::new() });
        }
    }
    (word.to_string(), items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, Namespace, Table};

    fn schema() -> Schema {
        let t =
            |name: &str, cols: &[&str]| Table { name: name.into(), columns: cols.iter().map(|c| ColumnInfo { name: c.to_string(), ty: "int".into(), nullable: true }).collect(), ..Table::default() };
        Schema {
            namespaces: vec![
                Namespace { name: "public".into(), tables: vec![t("users", &["id", "name", "email"]), t("orders", &["id", "user_id", "total"])] },
                Namespace { name: "audit".into(), tables: vec![t("log", &["at"])] },
            ],
        }
    }

    fn labels(text: &str) -> Vec<String> {
        let at = text.find('|').unwrap();
        let text = text.replace('|', "");
        complete(Some(&schema()), &text, at, Dialect::Postgres).1.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn tables_after_from_and_columns_after_a_dot() {
        assert_eq!(labels("select * from u|"), ["users"]);
        assert_eq!(labels("select * from |"), ["users", "orders", "public", "log", "audit"], "tables, and schemas");
        assert_eq!(labels("select u.| from users u"), ["id", "name", "email"]);
        assert_eq!(labels("select o.u| from users u join orders as o on o.user_id = u.id"), ["user_id"]);
        assert_eq!(labels("select users.e| from users"), ["email"]);
        assert_eq!(labels("select * from audit.|"), ["log"]);
    }

    #[test]
    fn columns_of_the_statements_tables_then_tables_and_keywords() {
        assert_eq!(labels("select na| from users; select 1"), ["name"]);
        assert_eq!(labels("select i| from users join orders on true")[..2], ["id".to_string(), "is".to_string()], "the column once, though both have it; then keywords");
        assert_eq!(labels("SELECT * FROM users WHERE em| "), ["email"]);
        assert_eq!(labels("sel|"), ["SELECT".to_lowercase()]);
        assert_eq!(labels("SEL|"), ["SELECT"]);
        assert_eq!(labels("select * from users wh|"), ["where", "when"]);
        assert_eq!(labels("select * from users where a|")[..2], ["audit".to_string(), "and".to_string()], "a schema, then keywords");
        let (word, items) = complete(None, "sel", 3, Dialect::Postgres);
        assert_eq!((word.as_str(), items.len()), ("sel", 1), "no schema: keywords");
    }

    #[test]
    fn tables_and_their_aliases() {
        assert_eq!(tables_in("select * from public.users u left join \"orders\" o on o.x = u.x where 1"), [("public.users".into(), Some("u".into())), ("orders".into(), Some("o".into()))]);
        assert_eq!(tables_in("update users set a = 1"), [("users".to_string(), None)]);
        assert_eq!(tables_in("insert into t values (1)"), [("t".to_string(), None)]);
    }
}
