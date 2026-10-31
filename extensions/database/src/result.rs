//! What a statement gives back -- its columns and rows, or how many rows
//! it changed, or why it failed -- and what's done with rows: sorted,
//! copied, exported as CSV or JSON.

use std::cmp::Ordering;

/// How a column's values read: what lines them up on the right, sorts them
/// as numbers and writes them to JSON unquoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Text,
    Number,
    Bool,
    /// Shown as hex (`\x0102`); not edited.
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Column {
    pub name: String,
    /// The engine's name for its type (`int4`, `varchar`), when it says.
    pub type_name: String,
    pub kind: Kind,
}

impl Column {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>, kind: Kind) -> Column {
        Column { name: name.into(), type_name: type_name.into(), kind }
    }
}

/// A cell: its text as the engine gives it, or NULL.
pub type Cell = Option<String>;

/// Rows a statement gave.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rows {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    /// More came than are kept (`database.max_rows`).
    pub truncated: bool,
}

/// How one statement went.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Rows(Rows),
    /// Done: the engine's word for it (`UPDATE 3`), and the rows it
    /// changed, when it says.
    Done {
        tag: String,
        changed: Option<u64>,
    },
    Error(String),
}

impl Outcome {
    pub fn is_error(&self) -> bool {
        matches!(self, Outcome::Error(_))
    }

    /// A line saying how it went.
    pub fn summary(&self) -> String {
        match self {
            Outcome::Rows(r) => {
                let n = r.rows.len();
                format!("{} row{}{}", thousands(n as u64), if n == 1 { "" } else { "s" }, if r.truncated { " (more not kept)" } else { "" })
            }
            Outcome::Done { tag, changed } => match changed {
                Some(n) if tag.is_empty() => format!("{} row{} changed", thousands(*n), if *n == 1 { "" } else { "s" }),
                _ if tag.is_empty() => "done".into(),
                _ => tag.clone(),
            },
            Outcome::Error(e) => e.clone(),
        }
    }
}

/// `n` with commas: 1,204.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A number as Rust reads one (`12`, `-1.5e3`), not `NaN` or `inf`.
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Two cells compared as `kind` does: NULLs last, numbers as numbers,
/// text without case first and then with it.
pub fn compare(a: &Cell, b: &Cell, kind: Kind) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(a), Some(b)) => {
            if kind == Kind::Number {
                if let (Some(x), Some(y)) = (number(a), number(b)) {
                    return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
                }
            }
            a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
        }
    }
}

/// Which way a column's sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Up,
    Down,
}

impl Rows {
    /// The rows' indices in order of column `col`, NULLs last either way;
    /// equal rows keep their order.
    pub fn sorted(&self, col: usize, order: Order) -> Vec<usize> {
        let kind = self.columns.get(col).map(|c| c.kind).unwrap_or_default();
        let mut idx: Vec<usize> = (0..self.rows.len()).collect();
        let cell = |r: usize| self.rows[r].get(col).cloned().flatten();
        idx.sort_by(|&x, &y| {
            let (a, b) = (cell(x), cell(y));
            match (&a, &b, order) {
                (None, Some(_), _) => Ordering::Greater,
                (Some(_), None, _) => Ordering::Less,
                (_, _, Order::Up) => compare(&a, &b, kind),
                (_, _, Order::Down) => compare(&b, &a, kind),
            }
        });
        idx
    }

    /// Cells `cols` of rows `rows` (indices into `rows`), tab between
    /// cells and a line a row: what pastes into a spreadsheet.
    pub fn tsv(&self, rows: &[usize], cols: std::ops::Range<usize>) -> String {
        rows.iter().map(|&r| self.rows[r][cols.clone()].iter().map(|c| c.as_deref().unwrap_or("").replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n")
    }

    /// The rows given, in that order (all of them when `rows` is none), as
    /// CSV with a header: RFC 4180's quoting, NULL as nothing.
    pub fn csv(&self, rows: Option<&[usize]>) -> String {
        let field = |s: &str| {
            if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };
        let mut out = self.columns.iter().map(|c| field(&c.name)).collect::<Vec<_>>().join(",");
        out.push_str("\r\n");
        let all: Vec<usize> = (0..self.rows.len()).collect();
        for &r in rows.unwrap_or(&all) {
            out.push_str(&self.rows[r].iter().map(|c| c.as_deref().map(field).unwrap_or_default()).collect::<Vec<_>>().join(","));
            out.push_str("\r\n");
        }
        out
    }

    /// The rows as a JSON array of objects, a key a column (a name given
    /// twice gets `_2`): numbers and booleans as JSON's, NULL as null.
    pub fn json(&self, rows: Option<&[usize]>) -> String {
        let mut names: Vec<String> = Vec::new();
        for c in &self.columns {
            let mut name = c.name.clone();
            let mut n = 1;
            while names.contains(&name) {
                n += 1;
                name = format!("{}_{n}", c.name);
            }
            names.push(name);
        }
        let all: Vec<usize> = (0..self.rows.len()).collect();
        let items: Vec<serde_json::Value> = rows
            .unwrap_or(&all)
            .iter()
            .map(|&r| {
                let mut obj = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let kind = self.columns[i].kind;
                    let v = match self.rows[r].get(i).cloned().flatten() {
                        None => serde_json::Value::Null,
                        Some(s) => json_value(&s, kind),
                    };
                    obj.insert(name.clone(), v);
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        serde_json::to_string_pretty(&items).unwrap_or_default()
    }
}

/// A cell as JSON: a number as one if it reads as one exactly; a boolean
/// from `t`/`f`, `1`/`0`, `true`/`false`.
fn json_value(s: &str, kind: Kind) -> serde_json::Value {
    match kind {
        Kind::Number => {
            if let Ok(n) = s.parse::<i64>() {
                return n.into();
            }
            if let Ok(n) = s.parse::<u64>() {
                return n.into();
            }
            // A decimal with more digits than a double holds stays text.
            let digits = s.trim_start_matches(['-', '+']).trim_start_matches(['0', '.']).chars().take_while(|c| !matches!(c, 'e' | 'E')).filter(char::is_ascii_digit).count();
            match number(s) {
                Some(f) if f.is_finite() && digits <= 15 => serde_json::Number::from_f64(f).map_or_else(|| s.into(), serde_json::Value::Number),
                _ => s.into(),
            }
        }
        Kind::Bool => match s.to_ascii_lowercase().as_str() {
            "t" | "true" | "1" => true.into(),
            "f" | "false" | "0" => false.into(),
            _ => s.into(),
        },
        _ => s.into(),
    }
}

/// Bytes as PostgreSQL writes bytea: `\x` and hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("\\x");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Rows {
        Rows {
            columns: vec![Column::new("id", "int4", Kind::Number), Column::new("name", "text", Kind::Text), Column::new("ok", "bool", Kind::Bool)],
            rows: vec![vec![Some("10".into()), Some("bob".into()), Some("t".into())], vec![Some("9".into()), None, Some("f".into())], vec![Some("100".into()), Some("Ann, \"A\"".into()), None]],
            truncated: false,
        }
    }

    #[test]
    fn numbers_sort_as_numbers_and_nulls_go_last() {
        let r = rows();
        assert_eq!(r.sorted(0, Order::Up), [1, 0, 2]);
        assert_eq!(r.sorted(0, Order::Down), [2, 0, 1]);
        assert_eq!(r.sorted(1, Order::Up), [2, 0, 1], "ann before bob, without case; NULL last");
        assert_eq!(r.sorted(1, Order::Down), [0, 2, 1], "NULL last going down too");
    }

    #[test]
    fn rows_go_out_as_csv_json_and_tab_separated() {
        let r = rows();
        assert_eq!(r.csv(None), "id,name,ok\r\n10,bob,t\r\n9,,f\r\n100,\"Ann, \"\"A\"\"\",\r\n");
        assert_eq!(r.csv(Some(&[2])), "id,name,ok\r\n100,\"Ann, \"\"A\"\"\",\r\n");
        let json: serde_json::Value = serde_json::from_str(&r.json(Some(&[0, 1]))).unwrap();
        assert_eq!(json, serde_json::json!([{ "id": 10, "name": "bob", "ok": true }, { "id": 9, "name": null, "ok": false }]));
        assert_eq!(r.tsv(&[0, 1], 0..2), "10\tbob\n9\t");
        let dup = Rows {
            columns: vec![Column::new("a", "", Kind::Number), Column::new("a", "", Kind::Number)],
            rows: vec![vec![Some("1.50".into()), Some("12345678901234567890123".into())]],
            truncated: false,
        };
        let json: serde_json::Value = serde_json::from_str(&dup.json(None)).unwrap();
        assert_eq!(json[0]["a"], serde_json::json!(1.5));
        assert_eq!(json[0]["a_2"], serde_json::json!("12345678901234567890123"), "too long for a double: kept as text");
    }

    #[test]
    fn summaries() {
        assert_eq!(Outcome::Rows(rows()).summary(), "3 rows");
        assert_eq!(Outcome::Done { tag: "UPDATE 1204".into(), changed: Some(1204) }.summary(), "UPDATE 1204");
        assert_eq!(Outcome::Done { tag: String::new(), changed: Some(1) }.summary(), "1 row changed");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(hex(&[1, 0xab]), "\\x01ab");
    }
}
