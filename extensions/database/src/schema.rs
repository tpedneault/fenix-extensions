//! A database's schema -- its schemas, tables and views, their columns
//! with types, indexes and keys -- as each engine's catalog gives it, and
//! as the sidebar's tree shows it.

use fenix_ui_kit::{Role, Style, TreeNode};

use crate::conn::Engine;
use crate::result::{Outcome, Rows};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub namespaces: Vec<Namespace>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Namespace {
    pub name: String,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub name: String,
    pub view: bool,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<Index>,
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnInfo {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Primary,
    Unique,
    Foreign,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub name: String,
    pub kind: KeyKind,
    pub columns: Vec<String>,
    /// A foreign key's: the table it refers to (its schema, its name) and
    /// its columns there.
    pub refers: Option<(String, String, Vec<String>)>,
}

impl Table {
    /// The primary key's columns, in order.
    pub fn primary_key(&self) -> Vec<String> {
        self.keys.iter().find(|k| k.kind == KeyKind::Primary).map(|k| k.columns.clone()).unwrap_or_default()
    }

    pub fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.iter().find(|c| c.name == name)
    }
}

impl Schema {
    pub fn table(&self, schema: &str, table: &str) -> Option<&Table> {
        self.namespaces.iter().find(|n| n.name == schema)?.tables.iter().find(|t| t.name == table)
    }

    /// A table by name, as a statement names it: `t`, `s.t`, quoted or not,
    /// any case.
    pub fn find(&self, name: &str) -> Option<(&Namespace, &Table)> {
        let unquote = |s: &str| s.trim().trim_matches(['"', '`', '[', ']']).to_string();
        let (schema, table) = match name.rsplit_once('.') {
            Some((s, t)) => (Some(unquote(s)), unquote(t)),
            None => (None, unquote(name)),
        };
        let mut found = None;
        for ns in &self.namespaces {
            if schema.as_ref().is_some_and(|s| !s.eq_ignore_ascii_case(&ns.name)) {
                continue;
            }
            if let Some(t) = ns.tables.iter().find(|t| t.name == table).or_else(|| ns.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&table))) {
                // The usual schemas first.
                if found.is_none() || matches!(ns.name.as_str(), "public" | "dbo" | "main") {
                    found = Some((ns, t));
                }
            }
        }
        found
    }

    pub fn table_count(&self) -> usize {
        self.namespaces.iter().map(|n| n.tables.len()).sum()
    }
}

/// The catalog queries for an engine, four statements in one batch:
/// tables (schema, name, `table`/`view`); columns (schema, table, name,
/// type, `YES`/`NO`); keys (schema, table, name, `p`/`u`/`f`, column, and a
/// foreign key's schema, table and column); indexes (schema, table, name,
/// unique, column -- PostgreSQL's whole definition instead).
pub fn catalog(engine: Engine, database: &str) -> Vec<String> {
    match engine {
        Engine::Postgres => {
            let not_system = "NOT IN ('pg_catalog', 'information_schema') AND {s} NOT LIKE 'pg\\_toast%' AND {s} NOT LIKE 'pg\\_temp%'";
            let w = |s: &str| format!("{s} {}", not_system.replace("{s}", s));
            vec![
                format!("SELECT table_schema, table_name, CASE table_type WHEN 'VIEW' THEN 'view' ELSE 'table' END FROM information_schema.tables WHERE {} ORDER BY 1, 2", w("table_schema")),
                format!("SELECT table_schema, table_name, column_name, CASE WHEN data_type IN ('USER-DEFINED', 'ARRAY') THEN udt_name WHEN character_maximum_length IS NOT NULL THEN data_type || '(' || character_maximum_length || ')' ELSE data_type END, is_nullable FROM information_schema.columns WHERE {} ORDER BY 1, 2, ordinal_position", w("table_schema")),
                format!("SELECT n.nspname, c.relname, con.conname, con.contype::text, a.attname, fn.nspname, f.relname, fa.attname FROM pg_constraint con JOIN pg_class c ON c.oid = con.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS k(attnum, i) JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum LEFT JOIN pg_class f ON f.oid = con.confrelid LEFT JOIN pg_namespace fn ON fn.oid = f.relnamespace LEFT JOIN pg_attribute fa ON fa.attrelid = con.confrelid AND fa.attnum = con.confkey[k.i] WHERE con.contype IN ('p', 'u', 'f') AND {} ORDER BY 1, 2, 3, k.i", w("n.nspname")),
                format!("SELECT schemaname, tablename, indexname, indexdef FROM pg_indexes WHERE {} ORDER BY 1, 2, 3", w("schemaname")),
            ]
        }
        Engine::Mysql => {
            let w = |s: &str| if database.is_empty() { format!("{s} NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')") } else { format!("{s} = DATABASE()") };
            vec![
                format!("SELECT TABLE_SCHEMA, TABLE_NAME, IF(TABLE_TYPE = 'VIEW', 'view', 'table') FROM information_schema.TABLES WHERE {} ORDER BY 1, 2", w("TABLE_SCHEMA")),
                format!("SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE FROM information_schema.COLUMNS WHERE {} ORDER BY 1, 2, ORDINAL_POSITION", w("TABLE_SCHEMA")),
                format!("SELECT k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, CASE c.CONSTRAINT_TYPE WHEN 'PRIMARY KEY' THEN 'p' WHEN 'UNIQUE' THEN 'u' ELSE 'f' END, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.TABLE_CONSTRAINTS c ON c.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND c.TABLE_NAME = k.TABLE_NAME AND c.CONSTRAINT_NAME = k.CONSTRAINT_NAME WHERE {} ORDER BY 1, 2, 3, k.ORDINAL_POSITION", w("k.TABLE_SCHEMA")),
                format!("SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, IF(NON_UNIQUE = 0, '1', '0'), COLUMN_NAME FROM information_schema.STATISTICS WHERE {} ORDER BY 1, 2, 3, SEQ_IN_INDEX", w("TABLE_SCHEMA")),
            ]
        }
        Engine::Mssql => vec![
            "SELECT TABLE_SCHEMA, TABLE_NAME, CASE TABLE_TYPE WHEN 'VIEW' THEN 'view' ELSE 'table' END FROM INFORMATION_SCHEMA.TABLES ORDER BY 1, 2".into(),
            "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE + COALESCE('(' + CASE WHEN CHARACTER_MAXIMUM_LENGTH = -1 THEN 'max' ELSE CAST(CHARACTER_MAXIMUM_LENGTH AS varchar(10)) END + ')', ''), IS_NULLABLE FROM INFORMATION_SCHEMA.COLUMNS ORDER BY 1, 2, ORDINAL_POSITION".into(),
            "SELECT k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, CASE c.CONSTRAINT_TYPE WHEN 'PRIMARY KEY' THEN 'p' ELSE 'u' END, k.COLUMN_NAME, NULL, NULL, NULL FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE k JOIN INFORMATION_SCHEMA.TABLE_CONSTRAINTS c ON c.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND c.CONSTRAINT_NAME = k.CONSTRAINT_NAME WHERE c.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE') UNION ALL SELECT SCHEMA_NAME(t.schema_id), t.name, fk.name, 'f', pc.name, SCHEMA_NAME(rt.schema_id), rt.name, rc.name FROM sys.foreign_key_columns fkc JOIN sys.foreign_keys fk ON fk.object_id = fkc.constraint_object_id JOIN sys.tables t ON t.object_id = fkc.parent_object_id JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id JOIN sys.tables rt ON rt.object_id = fkc.referenced_object_id JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id ORDER BY 1, 2, 3".into(),
            "SELECT SCHEMA_NAME(t.schema_id), t.name, i.name, CASE WHEN i.is_unique = 1 THEN '1' ELSE '0' END, c.name FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id WHERE i.name IS NOT NULL ORDER BY 1, 2, 3, ic.key_ordinal".into(),
        ],
        // SQLite's is read with pragmas, table by table (`lite`).
        Engine::Sqlite => Vec::new(),
    }
}

fn cell(r: &[Option<String>], i: usize) -> String {
    r.get(i).cloned().flatten().unwrap_or_default()
}

/// PostgreSQL's index definition as whether it's unique and its columns:
/// `CREATE UNIQUE INDEX x ON s.t USING btree (a, lower(b))`.
pub fn index_def(def: &str) -> (bool, Vec<String>) {
    let unique = def.to_ascii_uppercase().starts_with("CREATE UNIQUE");
    let cols = match (def.find('('), def.rfind(')')) {
        (Some(a), Some(b)) if a < b => {
            let inner = &def[a + 1..b];
            // Split at top-level commas.
            let mut out = Vec::new();
            let (mut depth, mut start) = (0usize, 0usize);
            for (i, c) in inner.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => depth = depth.saturating_sub(1),
                    ',' if depth == 0 => {
                        out.push(inner[start..i].trim().trim_matches('"').to_string());
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            out.push(inner[start..].trim().trim_matches('"').to_string());
            out
        }
        _ => Vec::new(),
    };
    (unique, cols)
}

/// The four catalog results put together.
pub fn assemble(engine: Engine, tables: &Rows, columns: &Rows, keys: &Rows, indexes: &Rows) -> Schema {
    let mut schema = Schema::default();
    for r in &tables.rows {
        let (ns, name) = (cell(r, 0), cell(r, 1));
        let n = match schema.namespaces.iter().position(|n| n.name == ns) {
            Some(i) => i,
            None => {
                schema.namespaces.push(Namespace { name: ns, tables: Vec::new() });
                schema.namespaces.len() - 1
            }
        };
        schema.namespaces[n].tables.push(Table { name, view: cell(r, 2) == "view", ..Table::default() });
    }
    let table = |schema: &mut Schema, ns: &str, name: &str| -> Option<(usize, usize)> {
        let n = schema.namespaces.iter().position(|x| x.name == ns)?;
        let t = schema.namespaces[n].tables.iter().position(|x| x.name == name)?;
        Some((n, t))
    };
    for r in &columns.rows {
        if let Some((n, t)) = table(&mut schema, &cell(r, 0), &cell(r, 1)) {
            schema.namespaces[n].tables[t].columns.push(ColumnInfo { name: cell(r, 2), ty: cell(r, 3), nullable: cell(r, 4).eq_ignore_ascii_case("YES") });
        }
    }
    for r in &keys.rows {
        let Some((n, t)) = table(&mut schema, &cell(r, 0), &cell(r, 1)) else { continue };
        let name = cell(r, 2);
        let kind = match cell(r, 3).as_str() {
            "p" => KeyKind::Primary,
            "u" => KeyKind::Unique,
            _ => KeyKind::Foreign,
        };
        let tbl = &mut schema.namespaces[n].tables[t];
        let column = cell(r, 4);
        let refers = (kind == KeyKind::Foreign).then(|| (cell(r, 5), cell(r, 6), cell(r, 7)));
        match tbl.keys.iter_mut().find(|k| k.name == name && k.kind == kind) {
            Some(k) => {
                k.columns.push(column);
                if let (Some((_, _, cols)), Some((_, _, c))) = (&mut k.refers, refers) {
                    cols.push(c);
                }
            }
            None => tbl.keys.push(Key { name, kind, columns: vec![column], refers: refers.map(|(s, t, c)| (s, t, vec![c])) }),
        }
    }
    for r in &indexes.rows {
        let Some((n, t)) = table(&mut schema, &cell(r, 0), &cell(r, 1)) else { continue };
        let name = cell(r, 2);
        let tbl = &mut schema.namespaces[n].tables[t];
        if engine == Engine::Postgres {
            let (unique, columns) = index_def(&cell(r, 3));
            tbl.indexes.push(Index { name, unique, columns });
            continue;
        }
        let unique = matches!(cell(r, 3).as_str(), "1" | "t" | "true");
        match tbl.indexes.iter_mut().find(|i| i.name == name) {
            Some(i) => i.columns.push(cell(r, 4)),
            None => tbl.indexes.push(Index { name, unique, columns: vec![cell(r, 4)] }),
        }
    }
    schema
}

/// The catalog's outcomes put together, or the first error among them.
pub fn from_outcomes(engine: Engine, outcomes: &[Outcome]) -> Result<Schema, String> {
    let mut rows = Vec::new();
    for o in outcomes {
        match o {
            Outcome::Rows(r) => rows.push(r),
            Outcome::Error(e) => return Err(e.clone()),
            Outcome::Done { .. } => {}
        }
    }
    match rows.as_slice() {
        [t, c, k, i] => Ok(assemble(engine, t, c, k, i)),
        _ => Err(format!("the catalog gave {} results, not 4", rows.len())),
    }
}

/// A node's id: what it is, and its path.
pub mod id {
    pub const SEP: char = '\u{1f}';

    pub fn connection(conn: &str) -> String {
        format!("k{SEP}{conn}")
    }

    pub fn table(conn: &str, schema: &str, table: &str) -> String {
        format!("t{SEP}{conn}{SEP}{schema}{SEP}{table}")
    }

    /// A table node's connection, schema and table.
    pub fn parse_table(id: &str) -> Option<(String, String, String)> {
        let mut p = id.split(SEP);
        if p.next()? != "t" {
            return None;
        }
        Some((p.next()?.to_string(), p.next()?.to_string(), p.next()?.to_string()))
    }

    /// The connection a node is under.
    pub fn conn_of(id: &str) -> Option<&str> {
        id.split(SEP).nth(1)
    }
}

/// What a table holds, under its node: its columns, then its indexes and
/// keys.
fn table_node(conn: &str, ns: &str, t: &Table) -> TreeNode {
    let tid = id::table(conn, ns, &t.name);
    let pk = t.primary_key();
    let mut children: Vec<TreeNode> = t
        .columns
        .iter()
        .map(|c| {
            let mut detail = c.ty.clone();
            if pk.contains(&c.name) {
                detail.push_str(" · PK");
            }
            if !c.nullable {
                detail.push_str(" · not null");
            }
            TreeNode::leaf(format!("{tid}{}c{}", id::SEP, c.name), c.name.clone()).detail(detail)
        })
        .collect();
    if !t.indexes.is_empty() {
        let idx = t
            .indexes
            .iter()
            .map(|i| TreeNode::leaf(format!("{tid}{}i{}", id::SEP, i.name), i.name.clone()).style(Role::Muted).detail(format!("{}({})", if i.unique { "unique " } else { "" }, i.columns.join(", "))))
            .collect();
        children.push(TreeNode::branch(format!("{tid}{}indexes", id::SEP), format!("Indexes ({})", t.indexes.len()), idx).style(Role::Muted));
    }
    if !t.keys.is_empty() {
        let keys = t
            .keys
            .iter()
            .map(|k| {
                let what = match (&k.kind, &k.refers) {
                    (KeyKind::Primary, _) => format!("primary ({})", k.columns.join(", ")),
                    (KeyKind::Unique, _) => format!("unique ({})", k.columns.join(", ")),
                    (KeyKind::Foreign, Some((s, t, cols))) => {
                        let s = if s.is_empty() || s == ns { String::new() } else { format!("{s}.") };
                        format!("({}) → {s}{t}({})", k.columns.join(", "), cols.join(", "))
                    }
                    (KeyKind::Foreign, None) => format!("({}) →", k.columns.join(", ")),
                };
                TreeNode::leaf(format!("{tid}{}k{}", id::SEP, k.name), k.name.clone()).style(Role::Muted).detail(what)
            })
            .collect();
        children.push(TreeNode::branch(format!("{tid}{}keys", id::SEP), format!("Keys ({})", t.keys.len()), keys).style(Role::Muted));
    }
    let node = TreeNode::branch(tid, t.name.clone(), children).detail(format!("{}", t.columns.len()));
    if t.view {
        node.style(Style::new(Role::Accent))
    } else {
        node
    }
}

/// A connection's schema as the tree's nodes under it: tables and views
/// by schema (none, when there's only the one).
pub fn tree(conn: &str, schema: &Schema) -> Vec<TreeNode> {
    let group = |ns: &Namespace| -> Vec<TreeNode> {
        let base = format!("{}{}{}", id::connection(conn), id::SEP, ns.name);
        let tables: Vec<TreeNode> = ns.tables.iter().filter(|t| !t.view).map(|t| table_node(conn, &ns.name, t)).collect();
        let views: Vec<TreeNode> = ns.tables.iter().filter(|t| t.view).map(|t| table_node(conn, &ns.name, t)).collect();
        let mut out = Vec::new();
        if !tables.is_empty() {
            out.push(TreeNode::branch(format!("{base}{}tables", id::SEP), format!("Tables ({})", tables.len()), tables).style(Role::Muted));
        }
        if !views.is_empty() {
            out.push(TreeNode::branch(format!("{base}{}views", id::SEP), format!("Views ({})", views.len()), views).style(Role::Muted));
        }
        out
    };
    match schema.namespaces.as_slice() {
        [] => Vec::new(),
        [only] => group(only),
        all => all.iter().map(|ns| TreeNode::branch(format!("{}{}s{}", id::connection(conn), id::SEP, ns.name), ns.name.clone(), group(ns)).detail(ns.tables.len().to_string())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Column;

    fn rows(data: &[&[&str]]) -> Rows {
        Rows {
            columns: vec![Column::default(); data.first().map_or(0, |r| r.len())],
            rows: data.iter().map(|r| r.iter().map(|c| if c.is_empty() { None } else { Some(c.to_string()) }).collect()).collect(),
            truncated: false,
        }
    }

    fn shop() -> Schema {
        let tables = rows(&[&["public", "orders", "table"], &["public", "users", "table"], &["public", "names", "view"], &["audit", "log", "table"]]);
        let columns = rows(&[
            &["public", "users", "id", "integer", "NO"],
            &["public", "users", "email", "character varying(200)", "YES"],
            &["public", "orders", "id", "integer", "NO"],
            &["public", "orders", "user_id", "integer", "YES"],
            &["public", "orders", "n", "integer", "YES"],
        ]);
        let keys = rows(&[
            &["public", "users", "users_pkey", "p", "id", "", "", ""],
            &["public", "orders", "orders_pkey", "p", "id", "", "", ""],
            &["public", "orders", "orders_pkey", "p", "n", "", "", ""],
            &["public", "orders", "orders_user_fk", "f", "user_id", "public", "users", "id"],
        ]);
        let indexes = rows(&[&["public", "users", "users_email", "CREATE UNIQUE INDEX users_email ON public.users USING btree (lower((email)::text), id)"]]);
        assemble(Engine::Postgres, &tables, &columns, &keys, &indexes)
    }

    #[test]
    fn the_catalog_is_put_together() {
        let s = shop();
        assert_eq!(s.table_count(), 4);
        let orders = s.table("public", "orders").unwrap();
        assert_eq!(orders.primary_key(), ["id", "n"], "a key of two columns, in order");
        assert_eq!(orders.keys[1].refers, Some(("public".into(), "users".into(), vec!["id".into()])));
        let users = s.table("public", "users").unwrap();
        assert_eq!(users.indexes, [Index { name: "users_email".into(), unique: true, columns: vec!["lower((email)::text)".into(), "id".into()] }]);
        assert!(users.column("email").unwrap().nullable);
        assert!(s.table("public", "names").unwrap().view);
        assert_eq!(s.find("USERS").map(|(n, t)| (n.name.as_str(), t.name.as_str())), Some(("public", "users")));
        assert_eq!(s.find("\"audit\".\"log\"").map(|(n, _)| n.name.as_str()), Some("audit"));
        assert!(s.find("nope").is_none());
    }

    #[test]
    fn the_tree_groups_tables_and_views_by_schema() {
        let s = shop();
        let nodes = tree("prod", &s);
        assert_eq!(nodes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["public", "audit"]);
        let public = &nodes[0];
        assert_eq!(public.children.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["Tables (2)", "Views (1)"]);
        let orders = &public.children[0].children[0];
        assert_eq!(id::parse_table(&orders.id), Some(("prod".into(), "public".into(), "orders".into())));
        assert_eq!(
            orders.children.iter().map(|n| (n.label.as_str(), n.detail.as_deref().unwrap_or(""))).collect::<Vec<_>>(),
            [("id", "integer · PK · not null"), ("user_id", "integer"), ("n", "integer · PK"), ("Keys (2)", "")]
        );
        assert_eq!(orders.children[3].children[1].detail.as_deref(), Some("(user_id) → users(id)"));
        // One schema: its tables and views straight under the connection.
        let one = Schema { namespaces: vec![s.namespaces[0].clone()] };
        assert_eq!(tree("x", &one)[0].label, "Tables (2)");
        assert_eq!(id::conn_of(&orders.id), Some("prod"));
    }

    #[test]
    fn errors_and_missing_results() {
        assert_eq!(from_outcomes(Engine::Mysql, &[Outcome::Error("no".into())]), Err("no".into()));
        assert!(from_outcomes(Engine::Mysql, &[]).unwrap_err().contains("not 4"));
        assert_eq!(catalog(Engine::Postgres, "").len(), 4);
        assert!(catalog(Engine::Mysql, "shop")[0].contains("= DATABASE()"));
        assert!(catalog(Engine::Mysql, "")[0].contains("NOT IN"));
    }
}
