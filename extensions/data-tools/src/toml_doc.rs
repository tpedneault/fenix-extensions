//! TOML read into a document with where each value is; written from one;
//! and formatted or its keys sorted in place, its comments kept.

use std::ops::Range;

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table};

use crate::json::Error;
use crate::node::{float_text, is_json_number, Lines, Member, Node, Pos, Value};

fn error(e: &toml_edit::TomlError, lines: &Lines) -> Error {
    let at = e.span().map_or(Pos::default(), |s| lines.pos(s.start));
    Error { message: e.message().trim().to_string(), at }
}

pub fn parse(text: &str) -> Result<Node, Error> {
    let lines = Lines::new(text);
    let doc = toml_edit::Document::parse(text).map_err(|e| error(&e, &lines))?;
    // Where each `[header]` starts: a table's own lines run to the next.
    let mut headers = Vec::new();
    collect_headers(doc.as_table(), &mut headers);
    headers.sort_unstable();
    let r = Reader { text, lines, headers };
    let mut node = r.table(doc.as_table());
    node.start = Pos::default();
    node.end = crate::node::end_of(text);
    Ok(node)
}

fn collect_headers(table: &Table, out: &mut Vec<usize>) {
    for (_, item) in table.iter() {
        match item {
            Item::Table(t) => {
                if let Some(s) = t.span().filter(|_| !t.is_implicit() && !t.is_dotted()) {
                    out.push(s.start);
                }
                collect_headers(t, out);
            }
            Item::ArrayOfTables(a) => {
                for t in a.iter() {
                    if let Some(s) = t.span() {
                        out.push(s.start);
                    }
                    collect_headers(t, out);
                }
            }
            _ => {}
        }
    }
}

struct Reader<'a> {
    text: &'a str,
    lines: Lines<'a>,
    headers: Vec<usize>,
}

impl Reader<'_> {
    fn pos(&self, byte: usize) -> Pos {
        self.lines.pos(byte)
    }

    /// The end of the table whose header starts at `start`: the next
    /// header, or the end.
    fn body_end(&self, start: usize) -> Pos {
        let next = self.headers.iter().find(|&&h| h > start).copied().unwrap_or(self.text.len());
        self.pos(next)
    }

    /// A node from its span, or from what's in it.
    fn spanned(&self, value: Value, span: Option<Range<usize>>) -> Node {
        match span {
            Some(s) => Node::at(value, self.pos(s.start), self.pos(s.end)),
            None => {
                let (start, end) = extent(&value);
                Node::at(value, start, end)
            }
        }
    }

    fn table(&self, table: &Table) -> Node {
        let mut members = Vec::new();
        for (k, item) in table.iter() {
            let key_span = table.key(k).and_then(|k| k.span());
            let value = match item {
                Item::None => continue,
                Item::Value(v) => self.value(v),
                Item::Table(t) => {
                    let mut node = self.table(t);
                    if let Some(s) = t.span().filter(|_| !t.is_implicit() && !t.is_dotted()) {
                        node.start = self.pos(s.start);
                        node.end = node.end.max(self.body_end(s.start));
                    }
                    node
                }
                Item::ArrayOfTables(a) => self.array_of_tables(a),
            };
            let (key_start, key_end) = match key_span {
                Some(s) => (self.pos(s.start), self.pos(s.end)),
                None => (value.start, value.start),
            };
            members.push(Member { key: k.to_string(), key_start, key_end, value });
        }
        let value = Value::Object(members);
        let (start, end) = extent(&value);
        Node::at(value, start, end)
    }

    fn array_of_tables(&self, a: &ArrayOfTables) -> Node {
        let items: Vec<Node> = a
            .iter()
            .map(|t| {
                let mut node = self.table(t);
                if let Some(s) = t.span() {
                    node.start = self.pos(s.start);
                    node.end = node.end.max(self.body_end(s.start));
                }
                node
            })
            .collect();
        let value = Value::Array(items);
        let (start, end) = extent(&value);
        Node::at(value, start, end)
    }

    fn inline_table(&self, t: &InlineTable) -> Value {
        Value::Object(
            t.iter()
                .map(|(k, v)| {
                    let value = self.value(v);
                    let (key_start, key_end) = t.key(k).and_then(|k| k.span()).map_or((value.start, value.start), |s| (self.pos(s.start), self.pos(s.end)));
                    Member { key: k.to_string(), key_start, key_end, value }
                })
                .collect(),
        )
    }

    fn array(&self, a: &Array) -> Value {
        Value::Array(a.iter().map(|v| self.value(v)).collect())
    }

    fn value(&self, v: &toml_edit::Value) -> Node {
        use toml_edit::Value as T;
        let span = v.span();
        let value = match v {
            T::String(s) => Value::String(s.value().clone()),
            T::Integer(i) => Value::Number(i.value().to_string()),
            T::Float(f) => {
                // As written, where JSON writes it so too (`1.50`, `1e3`).
                let written = span.clone().and_then(|s| self.text.get(s)).map(|t| t.replace('_', "")).map(|t| t.strip_prefix('+').map(str::to_string).unwrap_or(t));
                Value::Number(written.filter(|t| is_json_number(t)).unwrap_or_else(|| float_text(*f.value())))
            }
            T::Boolean(b) => Value::Bool(*b.value()),
            T::Datetime(d) => Value::String(d.value().to_string()),
            T::Array(a) => self.array(a),
            T::InlineTable(t) => self.inline_table(t),
        };
        self.spanned(value, span)
    }
}

/// Where what's in a collection starts and ends.
fn extent(value: &Value) -> (Pos, Pos) {
    let spans: Vec<(Pos, Pos)> = match value {
        Value::Object(members) => members.iter().map(|m| (m.key_start.min(m.value.start), m.value.end)).collect(),
        Value::Array(items) => items.iter().map(|n| (n.start, n.end)).collect(),
        _ => Vec::new(),
    };
    let start = spans.iter().map(|s| s.0).min().unwrap_or_default();
    let end = spans.iter().map(|s| s.1).max().unwrap_or_default();
    (start, end)
}

/// `node` as TOML: a table, nothing in it null.
pub fn write(node: &Node) -> Result<String, String> {
    let toml::Value::Table(table) = to_toml(node, &mut Vec::new())? else {
        return Err("a TOML document is a table: this is a list or a single value".into());
    };
    toml::to_string(&table).map_err(|e| e.to_string())
}

fn to_toml(node: &Node, path: &mut Vec<crate::path::Seg>) -> Result<toml::Value, String> {
    use crate::path::Seg;
    Ok(match &node.value {
        Value::Null => {
            let at = if path.is_empty() { "the document".to_string() } else { crate::path::render(path, crate::path::Style::Jq) };
            return Err(format!("TOML has no null, and {at} is null"));
        }
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Number(n) => match n.as_str() {
            "inf" => toml::Value::Float(f64::INFINITY),
            "-inf" => toml::Value::Float(f64::NEG_INFINITY),
            "nan" => toml::Value::Float(f64::NAN),
            n => match n.parse::<i64>() {
                Ok(i) => toml::Value::Integer(i),
                Err(_) => toml::Value::Float(n.parse::<f64>().map_err(|_| format!("{n} isn't a number"))?),
            },
        },
        Value::Array(items) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                out.push(to_toml(item, path)?);
                path.pop();
            }
            toml::Value::Array(out)
        }
        Value::Object(members) => {
            let mut table = toml::Table::new();
            for m in members {
                path.push(Seg::Key(m.key.clone()));
                table.insert(m.key.clone(), to_toml(&m.value, path)?);
                path.pop();
            }
            toml::Value::Table(table)
        }
    })
}

/// Comments a decor holds, a line each, without their indentation or
/// trailing spaces, blank lines one at a time. Its last piece is what's
/// before the key or header on its own line: nothing.
fn tidy_prefix(prefix: &str) -> String {
    let mut pieces: Vec<&str> = prefix.split('\n').collect();
    pieces.pop();
    let mut out = String::new();
    let mut blank = false;
    for line in pieces.into_iter().map(str::trim) {
        if line.is_empty() {
            blank = true;
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    if blank {
        out.push('\n');
    }
    out
}

fn raw(decor: Option<&toml_edit::RawString>) -> String {
    decor.and_then(|r| r.as_str()).unwrap_or("").to_string()
}

/// A comment after a value or header, ` # like this`; or nothing.
fn tidy_suffix(suffix: &str) -> String {
    match suffix.trim() {
        "" => String::new(),
        comment => format!(" {comment}"),
    }
}

fn tidy_value(v: &mut toml_edit::Value) {
    let written = v.to_string();
    if written.contains('#') || written.trim().contains('\n') {
        return;
    }
    match v {
        toml_edit::Value::Array(a) => {
            a.iter_mut().for_each(tidy_value);
            a.fmt();
        }
        toml_edit::Value::InlineTable(t) if t.is_empty() => t.set_trailing(""),
        toml_edit::Value::InlineTable(t) => {
            t.iter_mut().for_each(|(_, v)| tidy_value(v));
            t.fmt();
        }
        _ => {}
    }
}

fn tidy_table(table: &mut Table, root: bool) {
    if !root && table.position().is_some() && !table.is_implicit() && !table.is_dotted() {
        let decor = table.decor_mut();
        let mut prefix = tidy_prefix(&raw(decor.prefix()));
        // A blank line before each header.
        if !prefix.starts_with('\n') {
            prefix.insert(0, '\n');
        }
        decor.set_prefix(prefix);
        decor.set_suffix(tidy_suffix(&raw(decor.suffix())));
    }
    for (mut key, item) in table.iter_mut() {
        match item {
            Item::Value(v) => {
                let prefix = tidy_prefix(&raw(key.leaf_decor().prefix()));
                key.leaf_decor_mut().set_prefix(prefix);
                key.leaf_decor_mut().set_suffix(" ");
                v.decor_mut().set_prefix(" ");
                let suffix = tidy_suffix(&raw(v.decor().suffix()));
                v.decor_mut().set_suffix(suffix);
                tidy_value(v);
            }
            Item::Table(t) => tidy_table(t, false),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(|t| tidy_table(t, false)),
            Item::None => {}
        }
    }
}

/// TOML formatted, its comments kept: `key = value` spaced so, nothing
/// indented, one blank line at most, one before each header, short arrays
/// and inline tables `[1, 2]` and `{ a = 1 }`.
pub fn format(text: &str) -> Result<String, Error> {
    let lines = Lines::new(text);
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| error(&e, &lines))?;
    tidy_table(doc.as_table_mut(), true);
    let trailing = tidy_prefix(&format!("{}\n", raw(Some(doc.trailing()))));
    doc.set_trailing(trailing);
    let out = doc.to_string();
    Ok(format!("{}\n", out.trim_start_matches('\n').trim_end()))
}

/// A table's place in key order, for its header to be put there.
type Rank = Vec<(usize, usize)>;

fn sort_table(table: &mut Table, recursive: bool, depth: usize, rank: &mut Rank, order: &mut Vec<(Rank, isize, usize)>) {
    if depth == 0 || recursive {
        table.sort_values();
    }
    let me = order.len();
    if let Some(p) = table.position() {
        order.push((rank.clone(), p, me));
    }
    for (i, (_, item)) in table.iter_mut().enumerate() {
        rank.push((i, 0));
        match item {
            Item::Table(t) => sort_table(t, recursive, depth + 1, rank, order),
            Item::ArrayOfTables(a) => {
                for (j, t) in a.iter_mut().enumerate() {
                    rank.last_mut().unwrap().1 = j;
                    sort_table(t, recursive, depth + 1, rank, order);
                }
            }
            _ => {}
        }
        rank.pop();
    }
}

/// The headers given their new positions, by `order`'s ranks.
fn place_tables(table: &mut Table, positions: &mut std::collections::VecDeque<isize>) {
    if table.position().is_some() {
        if let Some(p) = positions.pop_front() {
            table.set_position(Some(p));
        }
        // A blank line before each header, wherever it's gone.
        if !table.is_implicit() && !table.is_dotted() {
            let prefix = raw(table.decor().prefix());
            if !prefix.starts_with('\n') {
                table.decor_mut().set_prefix(format!("\n{prefix}"));
            }
        }
    }
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(t) => place_tables(t, positions),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(|t| place_tables(t, positions)),
            _ => {}
        }
    }
}

/// TOML with its keys sorted in place, comments going with what they're
/// above: every table's, or only the top level's, its tables' headers
/// following their keys' order.
pub fn sort(text: &str, recursive: bool) -> Result<String, Error> {
    let lines = Lines::new(text);
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| error(&e, &lines))?;
    let mut order = Vec::new();
    sort_table(doc.as_table_mut(), recursive, 0, &mut Vec::new(), &mut order);
    // Visited in the new order: each header gets the next position, in
    // order of key (recursive), or of its top-level key and then where it
    // was (top level only).
    let mut ranked: Vec<(usize, &(Rank, isize, usize))> = order.iter().enumerate().collect();
    if recursive {
        ranked.sort_by(|a, b| a.1 .0.cmp(&b.1 .0));
    } else {
        ranked.sort_by_key(|(_, (rank, p, _))| (rank.first().map(|r| r.0), *p));
    }
    let mut new = vec![0isize; order.len()];
    for (place, (visited, _)) in ranked.iter().enumerate() {
        new[*visited] = place as isize;
    }
    place_tables(doc.as_table_mut(), &mut new.into_iter().collect());
    let mut out = doc.to_string();
    // The first header may have moved down, and another to the top.
    out = out.trim_start_matches('\n').to_string();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> String {
        crate::json::write(&parse(text).unwrap(), &crate::json::Style::Minified).unwrap_or_else(|e| e)
    }

    #[test]
    fn it_reads_values_in_order() {
        let text = "title = \"x\"\nn = 1_000\nf = 1.50\ng = +2e3\nwhen = 1979-05-27T07:32:00Z\n[owner]\nname = \"T\"\nlist = [1, 2]\n[[servers]]\nport = 80\n[[servers]]\nport = 81\n[a.b]\nc = { d = true }\n";
        assert_eq!(
            json(text),
            r#"{"title":"x","n":1000,"f":1.50,"g":2e3,"when":"1979-05-27T07:32:00Z","owner":{"name":"T","list":[1,2]},"servers":[{"port":80},{"port":81}],"a":{"b":{"c":{"d":true}}}}"#
        );
        assert_eq!(json("x = -inf"), "-inf isn't a number JSON has");
        let Value::Object(m) = parse("x = -inf").unwrap().value else { panic!() };
        assert_eq!(m[0].value.value, Value::Number("-inf".into()));
    }

    #[test]
    fn errors_say_where() {
        let e = parse("a = 1\nb = \n").unwrap_err();
        assert_eq!(e.at.line, 1, "{e:?}");
        assert!(parse("a = 1\na = 2\n").is_err(), "a key twice");
    }

    #[test]
    fn tables_run_to_the_next_header() {
        let node = parse("top = 1\n\n[server]\nhost = \"h\"\n\nport = 80\n\n[[db]]\nname = \"a\"\n").unwrap();
        let Value::Object(m) = &node.value else { panic!() };
        assert_eq!((m[1].key_start, m[1].value.start, m[1].value.end), (Pos::new(2, 1), Pos::new(2, 0), Pos::new(7, 0)));
        let Value::Array(db) = &m[2].value.value else { panic!() };
        assert_eq!((db[0].start, db[0].end), (Pos::new(7, 0), Pos::new(9, 0)));
    }

    #[test]
    fn it_writes_tables_and_arrays_of_tables() {
        let doc = crate::json::parse(r#"{"name":"x","n":1,"f":1.5,"tags":["a","b"],"owner":{"id":7},"servers":[{"port":80},{"port":81}]}"#).unwrap().node;
        let toml = write(&doc).unwrap();
        assert_eq!(toml, "name = \"x\"\nn = 1\nf = 1.5\ntags = [\"a\", \"b\"]\n\n[owner]\nid = 7\n\n[[servers]]\nport = 80\n\n[[servers]]\nport = 81\n");
        assert_eq!(parse(&toml).unwrap().to_json(), doc.to_json());
    }

    #[test]
    fn what_toml_cant_say() {
        let null = crate::json::parse(r#"{"a":{"b":[1,null]}}"#).unwrap().node;
        assert_eq!(write(&null).unwrap_err(), "TOML has no null, and .a.b[1] is null");
        assert!(write(&crate::json::parse("[1]").unwrap().node).unwrap_err().contains("a table"));
    }

    #[test]
    fn it_formats_keeping_comments() {
        let text = "# head\n\n\n  a   =   1    # one\nb=[1,2,  3]\n  [server]   # the server\n\n\n   # its host\n   host=\"h\"\nempty={  }\n[[x]]\ny = 2\n\n\n";
        let formatted = format(text).unwrap();
        assert_eq!(formatted, "# head\n\na = 1 # one\nb = [1, 2, 3]\n\n[server] # the server\n\n# its host\nhost = \"h\"\nempty = {}\n\n[[x]]\ny = 2\n");
        assert_eq!(format(&formatted).unwrap(), formatted, "formatting twice is formatting once");
        let multi = "a = [\n  1, # one\n  2,\n]\n";
        assert_eq!(format(multi).unwrap(), multi, "an array with comments is left as it is");
    }

    #[test]
    fn it_sorts_keys_with_their_comments() {
        let text = "# about b\nb = 1\na = 2\n\n[z]\ny = 1\nx = 2\n\n[c]\nk = 1\n\n[z.inner]\nq = 1\n";
        let all = sort(text, true).unwrap();
        assert_eq!(all, "a = 2\n# about b\nb = 1\n\n[c]\nk = 1\n\n[z]\nx = 2\ny = 1\n\n[z.inner]\nq = 1\n");
        let top = sort(text, false).unwrap();
        assert_eq!(top, "a = 2\n# about b\nb = 1\n\n[c]\nk = 1\n\n[z]\ny = 1\nx = 2\n\n[z.inner]\nq = 1\n");
        assert_eq!(parse(&all).unwrap().to_json(), parse(text).unwrap().to_json());
    }

    #[test]
    fn sorting_keeps_arrays_of_tables_in_order() {
        let text = "[[s]]\nb = 1\na = 1\n\n[[s]]\nd = 1\nc = 1\n\n[a]\nx = 1\n";
        assert_eq!(sort(text, true).unwrap(), "[a]\nx = 1\n\n[[s]]\na = 1\nb = 1\n\n[[s]]\nc = 1\nd = 1\n");
    }
}
