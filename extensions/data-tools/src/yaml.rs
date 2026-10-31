//! YAML read into a document with where each value is (YAML 1.2's core
//! schema: `yes` is a string, `0x1F` a number), anchors and `<<` merges
//! followed; and written out in block style.

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::json::Error;
use crate::node::{float_text, Member, Node, Pos, Value};

enum Frame {
    Seq { start: Pos, anchor: usize, items: Vec<Node> },
    Map { start: Pos, anchor: usize, members: Vec<Member>, key: Option<(String, Pos, Pos)>, merges: Vec<Node> },
}

struct Builder<'a> {
    lines: Vec<&'a str>,
    stack: Vec<Frame>,
    anchors: std::collections::HashMap<usize, Node>,
    docs: Vec<Node>,
    error: Option<Error>,
}

fn pos(mark: &Marker) -> Pos {
    Pos::new(mark.line().saturating_sub(1) as u32, mark.col() as u32)
}

pub fn parse(text: &str) -> Result<Node, Error> {
    let mut docs = parse_all(text)?;
    match docs.len() {
        0 => Ok(Node::new(Value::Null)),
        1 => Ok(docs.remove(0)),
        n => Err(Error { message: format!("{n} documents (---) in one: these work on one at a time; select one"), at: docs[1].start }),
    }
}

/// Every document in a stream.
pub fn parse_all(text: &str) -> Result<Vec<Node>, Error> {
    let mut b = Builder { lines: text.split('\n').collect(), stack: Vec::new(), anchors: Default::default(), docs: Vec::new(), error: None };
    let mut parser = Parser::new_from_str(text);
    parser.load(&mut b, true).map_err(|e| Error { message: e.info().to_string(), at: pos(e.marker()) })?;
    match b.error {
        Some(e) => Err(e),
        None => Ok(b.docs),
    }
}

impl Builder<'_> {
    fn line(&self, line: u32) -> Vec<char> {
        self.lines.get(line as usize).map(|l| l.trim_end_matches('\r').chars().collect()).unwrap_or_default()
    }

    /// Where a scalar that starts at `start` ends, by its source.
    fn scalar_end(&self, start: Pos, style: TScalarStyle, value: &str) -> Pos {
        let mut at = start;
        let mut line = self.line(at.line);
        let close = match style {
            TScalarStyle::DoubleQuoted => '"',
            TScalarStyle::SingleQuoted => '\'',
            TScalarStyle::Plain => {
                let first = value.lines().next().unwrap_or("");
                let n = first.chars().count() as u32;
                let here: String = line.iter().skip(start.col as usize).take(n as usize).collect();
                if !value.contains('\n') && here == first {
                    return Pos::new(start.line, start.col + n);
                }
                return Pos::new(start.line, line.len() as u32);
            }
            TScalarStyle::Literal | TScalarStyle::Folded => {
                // The indicator's line, then a line each, at most.
                let lines = value.trim_end_matches('\n').lines().count().max(1) as u32;
                let last = start.line + lines;
                return Pos::new(last, self.line(last).len() as u32);
            }
        };
        at.col += 1;
        loop {
            match line.get(at.col as usize) {
                Some('\\') if close == '"' => at.col += 2,
                Some('\'') if close == '\'' && line.get(at.col as usize + 1) == Some(&'\'') => at.col += 2,
                Some(&c) if c == close => return Pos::new(at.line, at.col + 1),
                Some(_) => at.col += 1,
                None if (at.line as usize) + 1 < self.lines.len() => {
                    at = Pos::new(at.line + 1, 0);
                    line = self.line(at.line);
                }
                None => return Pos::new(at.line, line.len() as u32),
            }
        }
    }

    fn fail(&mut self, message: impl Into<String>, at: Pos) {
        if self.error.is_none() {
            self.error = Some(Error { message: message.into(), at });
        }
    }

    fn done(&mut self, node: Node, anchor: usize, scalar_key: Option<(String, bool)>) {
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            None => self.docs.push(node),
            Some(Frame::Seq { items, .. }) => items.push(node),
            Some(Frame::Map { members, key, merges, .. }) => match key.take() {
                None => match scalar_key {
                    Some((k, _)) => *key = Some((k, node.start, node.end)),
                    None => {
                        let at = node.start;
                        self.fail("a key that's a list or a map: JSON, TOML and these tools take keys that are text", at);
                    }
                },
                Some((k, key_start, key_end)) => {
                    if k == "<<" {
                        merges.push(node);
                    } else {
                        members.push(Member { key: k, key_start, key_end, value: node });
                    }
                }
            },
        }
    }
}

/// A plain scalar's value, by YAML 1.2's core schema.
fn resolve(s: &str) -> Value {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => return Value::Number("inf".into()),
        "-.inf" | "-.Inf" | "-.INF" => return Value::Number("-inf".into()),
        ".nan" | ".NaN" | ".NAN" => return Value::Number("nan".into()),
        _ => {}
    }
    if let Some(n) = number(s) {
        return Value::Number(n);
    }
    Value::String(s.to_string())
}

/// A YAML number as JSON writes it.
fn number(s: &str) -> Option<String> {
    let radix = |digits: &str, r: u32| i128::from_str_radix(digits, r).ok().map(|n| n.to_string());
    if let Some(hex) = s.strip_prefix("0x") {
        return radix(hex, 16).filter(|_| !hex.is_empty() && !hex.starts_with(['+', '-']));
    }
    if let Some(oct) = s.strip_prefix("0o") {
        return radix(oct, 8).filter(|_| !oct.is_empty() && !oct.starts_with(['+', '-']));
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if !unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.') || !unsigned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if unsigned.chars().all(|c| c.is_ascii_digit()) {
        let digits = unsigned.trim_start_matches('0');
        let digits = if digits.is_empty() { "0" } else { digits };
        return Some(if s.starts_with('-') && digits != "0" { format!("-{digits}") } else { digits.to_string() });
    }
    // [-+]? ( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let ok = int.chars().all(|c| c.is_ascii_digit())
        && frac.chars().all(|c| c.is_ascii_digit())
        && exponent.is_none_or(|e| {
            let e = e.strip_prefix(['-', '+']).unwrap_or(e);
            !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
        });
    if !ok {
        return None;
    }
    let plain = s.strip_prefix('+').unwrap_or(s);
    if crate::node::is_json_number(plain) {
        return Some(plain.to_string());
    }
    plain.parse::<f64>().ok().map(float_text)
}

fn tagged(value: &str, tag: &Tag, at: Pos) -> Result<Value, Error> {
    if tag.handle != "tag:yaml.org,2002:" {
        return Ok(resolve(value));
    }
    let wrong = |what: &str| Error { message: format!("{value:?} isn't {what} (!!{})", tag.suffix), at };
    Ok(match tag.suffix.as_str() {
        "str" => Value::String(value.to_string()),
        "null" => Value::Null,
        "bool" => match resolve(value) {
            v @ Value::Bool(_) => v,
            _ => return Err(wrong("true or false")),
        },
        "int" | "float" => match resolve(value) {
            v @ Value::Number(_) => v,
            _ => return Err(wrong("a number")),
        },
        _ => resolve(value),
    })
}

impl MarkedEventReceiver for Builder<'_> {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        if self.error.is_some() {
            return;
        }
        let at = pos(&mark);
        match ev {
            Event::DocumentStart => self.anchors.clear(),
            Event::Scalar(value, style, anchor, tag) => {
                let end = self.scalar_end(at, style, &value);
                let resolved = match (&tag, style) {
                    (Some(tag), _) => tagged(&value, tag, at),
                    (None, TScalarStyle::Plain) => Ok(resolve(&value)),
                    (None, _) => Ok(Value::String(value.clone())),
                };
                match resolved {
                    Ok(v) => self.done(Node::at(v, at, end), anchor, Some((value, style == TScalarStyle::Plain))),
                    Err(e) => self.fail(e.message, e.at),
                }
            }
            Event::Alias(id) => match self.anchors.get(&id) {
                Some(node) => {
                    let mut node = node.clone();
                    let key = match &node.value {
                        Value::String(s) => Some((s.clone(), false)),
                        _ => None,
                    };
                    node.start = at;
                    node.end = at;
                    self.done(node, 0, key)
                }
                None => self.fail("an alias (*) to no anchor (&)", at),
            },
            Event::SequenceStart(anchor, _) => self.stack.push(Frame::Seq { start: at, anchor, items: Vec::new() }),
            Event::MappingStart(anchor, _) => self.stack.push(Frame::Map { start: at, anchor, members: Vec::new(), key: None, merges: Vec::new() }),
            Event::SequenceEnd => {
                if let Some(Frame::Seq { start, anchor, items }) = self.stack.pop() {
                    self.done(Node::at(Value::Array(items), start, at), anchor, None);
                }
            }
            Event::MappingEnd => {
                if let Some(Frame::Map { start, anchor, mut members, merges, .. }) = self.stack.pop() {
                    // `<<: *base`: its keys, where they aren't given here.
                    for merge in merges {
                        let sources = match merge.value {
                            Value::Object(_) => vec![merge],
                            Value::Array(items) => items,
                            _ => {
                                self.fail("<< merges a map, or a list of maps", merge.start);
                                return;
                            }
                        };
                        for source in sources {
                            if let Value::Object(theirs) = source.value {
                                for m in theirs {
                                    if !members.iter().any(|x| x.key == m.key) {
                                        members.push(m);
                                    }
                                }
                            }
                        }
                    }
                    self.done(Node::at(Value::Object(members), start, at), anchor, None);
                }
            }
            _ => {}
        }
    }
}

/// Whether a string can be written plain, and read back as that string.
fn plain_ok(s: &str) -> bool {
    if s.is_empty() || s != s.trim() || !matches!(resolve(s), Value::String(_)) {
        return false;
    }
    let first = s.chars().next().unwrap_or(' ');
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) && !(matches!(first, '-' | '?' | ':') && s.chars().nth(1).is_some_and(|c| c != ' ')) {
        return false;
    }
    if s.starts_with("---") || s.starts_with("...") {
        return false;
    }
    // `, [ ] { }` are fine in block style but not in a flow collection.
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') || s.contains([',', '[', ']', '{', '}']) {
        return false;
    }
    !s.chars().any(|c| c.is_control() || c == '\u{feff}')
}

/// `s` as a YAML scalar on one line. YAML 1.1's booleans (`yes`, `off`)
/// are quoted too, for older readers -- but not as keys, where GitHub's
/// `on:` is meant.
fn scalar(s: &str) -> String {
    let old_bool = matches!(s.to_ascii_lowercase().as_str(), "y" | "n" | "yes" | "no" | "on" | "off");
    if plain_ok(s) && !old_bool {
        s.to_string()
    } else {
        crate::json::quote(s)
    }
}

/// `s` as a literal block (`|`), when it reads back as `s`.
fn block(s: &str, indent: &str) -> Option<String> {
    if !s.contains('\n') || s.contains('\r') || s.chars().any(|c| c.is_control() && c != '\n') || s.starts_with([' ', '\n']) {
        return None;
    }
    if s.lines().any(|l| l != l.trim_end()) {
        return None;
    }
    let body = s.strip_suffix('\n');
    let (indicator, text) = match body {
        Some(b) if b.ends_with('\n') => ("|+", b),
        Some(b) => ("|", b),
        None => ("|-", s),
    };
    let mut out = indicator.to_string();
    for line in text.split('\n') {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
    }
    if indicator == "|+" {
        out.push('\n');
    }
    Some(out)
}

fn number_text(n: &str) -> String {
    match n {
        "inf" => ".inf".into(),
        "-inf" => "-.inf".into(),
        "nan" => ".nan".into(),
        n => n.to_string(),
    }
}

/// `node` as YAML, in block style, nested `unit` deeper.
pub fn write(node: &Node, unit: &str) -> String {
    let mut out = String::new();
    match &node.value {
        Value::Array(items) if !items.is_empty() => write_seq(items, unit, 0, &mut out),
        Value::Object(members) if !members.is_empty() => write_map(members, unit, 0, &mut out),
        _ => out.push_str(&inline(node, unit, 0)),
    }
    out
}

/// One line's worth, or a block scalar that follows on.
fn inline(node: &Node, unit: &str, depth: usize) -> String {
    match &node.value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(n),
        Value::String(s) => block(s, &unit.repeat(depth + 1)).unwrap_or_else(|| scalar(s)),
        Value::Array(items) if items.is_empty() => "[]".into(),
        Value::Object(members) if members.is_empty() => "{}".into(),
        _ => flow(node),
    }
}

/// A collection on one line: `{a: 1, b: [2, 3]}`.
pub fn flow(node: &Node) -> String {
    match &node.value {
        Value::Array(items) => format!("[{}]", items.iter().map(flow).collect::<Vec<_>>().join(", ")),
        Value::Object(members) => format!("{{{}}}", members.iter().map(|m| format!("{}: {}", key(&m.key), flow(&m.value))).collect::<Vec<_>>().join(", ")),
        Value::String(s) => scalar(s),
        _ => inline(node, "", 0),
    }
}

fn nested(node: &Node) -> bool {
    matches!(&node.value, Value::Array(i) if !i.is_empty()) || matches!(&node.value, Value::Object(m) if !m.is_empty())
}

fn write_map(members: &[Member], unit: &str, depth: usize, out: &mut String) {
    let indent = unit.repeat(depth);
    for (i, m) in members.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&indent);
        }
        out.push_str(&key(&m.key));
        out.push(':');
        match &m.value.value {
            Value::Object(inner) if !inner.is_empty() => {
                out.push('\n');
                out.push_str(&unit.repeat(depth + 1));
                write_map(inner, unit, depth + 1, out);
            }
            Value::Array(items) if !items.is_empty() => {
                out.push('\n');
                out.push_str(&unit.repeat(depth + 1));
                write_seq(items, unit, depth + 1, out);
            }
            _ => {
                out.push(' ');
                out.push_str(&inline(&m.value, unit, depth));
            }
        }
    }
}

/// A sequence's items, `- ` each; what's nested in one is two columns in
/// (past its `- `).
fn write_seq(items: &[Node], unit: &str, depth: usize, out: &mut String) {
    let indent = unit.repeat(depth);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&indent);
        }
        out.push_str("- ");
        let inner = format!("{indent}  ");
        match &item.value {
            Value::Object(m) if nested(item) => write_map_at(m, unit, &inner, out),
            Value::Array(items) if nested(item) => write_seq_at(items, unit, &inner, out),
            _ => out.push_str(&inline_at(item, unit, &inner)),
        }
    }
}

// The same, at an indent that isn't a whole number of units (inside `- `).
fn write_map_at(members: &[Member], unit: &str, indent: &str, out: &mut String) {
    for (i, m) in members.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(indent);
        }
        out.push_str(&key(&m.key));
        out.push(':');
        let deeper = format!("{indent}{unit}");
        match &m.value.value {
            Value::Object(inner) if nested(&m.value) => {
                out.push('\n');
                out.push_str(&deeper);
                write_map_at(inner, unit, &deeper, out);
            }
            Value::Array(items) if nested(&m.value) => {
                out.push('\n');
                out.push_str(&deeper);
                write_seq_at(items, unit, &deeper, out);
            }
            _ => {
                out.push(' ');
                out.push_str(&inline_at(&m.value, unit, indent));
            }
        }
    }
}

fn write_seq_at(items: &[Node], unit: &str, indent: &str, out: &mut String) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(indent);
        }
        out.push_str("- ");
        let inner = format!("{indent}  ");
        match &item.value {
            Value::Object(m) if nested(item) => write_map_at(m, unit, &inner, out),
            Value::Array(items) if nested(item) => write_seq_at(items, unit, &inner, out),
            _ => out.push_str(&inline_at(item, unit, &inner)),
        }
    }
}

fn inline_at(node: &Node, unit: &str, indent: &str) -> String {
    match &node.value {
        Value::String(s) => block(s, &format!("{indent}{unit}")).unwrap_or_else(|| scalar(s)),
        _ => inline(node, unit, 0),
    }
}

fn key(k: &str) -> String {
    if plain_ok(k) {
        k.to_string()
    } else {
        crate::json::quote(k)
    }
}

/// Whether YAML text has comments, which writing it again drops: a `#`
/// that starts a line's text, or follows a space, outside quotes.
pub fn has_comments(text: &str) -> bool {
    text.lines().any(|line| {
        let mut quote = None;
        let mut last = ' ';
        for c in line.chars() {
            match (quote, c) {
                (None, '#') if last == ' ' || last == '\t' => return true,
                (None, '"' | '\'') if last == ' ' || last == ':' || last == '-' || last == '[' || last == ',' => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                _ => {}
            }
            last = c;
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> String {
        serde_json::to_string(&parse(text).unwrap().to_json()).unwrap()
    }

    #[test]
    fn scalars_are_read_by_the_core_schema() {
        assert_eq!(
            json("a: 1\nb: -2.5\nc: true\nd: ~\ne: yes\nf: '1'\ng: \"x\\ty\"\nh: 0x1F\ni: 1e3\nj: 007\nk: .5\nl:\nm: !!str 12\n"),
            r#"{"a":1,"b":-2.5,"c":true,"d":null,"e":"yes","f":"1","g":"x\ty","h":31,"i":1000.0,"j":7,"k":0.5,"l":null,"m":"12"}"#
        );
        let node = parse("x: .inf").unwrap();
        let Value::Object(m) = &node.value else { panic!() };
        assert_eq!(m[0].value.value, Value::Number("inf".into()));
    }

    #[test]
    fn blocks_anchors_and_merges() {
        let text = "base: &b\n  x: 1\n  y: 2\nmine:\n  <<: *b\n  y: 3\nlist:\n  - *b\ntext: |\n  one\n  two\nfolded: >-\n  a\n  b\n";
        assert_eq!(json(text), r#"{"base":{"x":1,"y":2},"mine":{"y":3,"x":1},"list":[{"x":1,"y":2}],"text":"one\ntwo\n","folded":"a b"}"#);
    }

    #[test]
    fn errors_and_several_documents() {
        let e = parse("a: [1, 2\nb: 3").unwrap_err();
        assert!(e.at.line <= 1, "{e:?}");
        let e = parse("a: 1\n---\nb: 2\n").unwrap_err();
        assert!(e.message.contains("2 documents"), "{}", e.message);
        assert_eq!(parse_all("a: 1\n---\nb: 2\n").unwrap().len(), 2);
        assert!(parse("? [a, b]\n: 1\n").unwrap_err().message.contains("a key that's a list"));
        assert_eq!(parse("").unwrap().value, Value::Null);
    }

    #[test]
    fn values_know_where_they_are() {
        let node = parse("servers:\n  - name: a\n    port: 80\n  - name: \"b c\"\n").unwrap();
        let Value::Object(top) = &node.value else { panic!() };
        assert_eq!(top[0].key_start, Pos::new(0, 0));
        let Value::Array(items) = &top[0].value.value else { panic!() };
        let Value::Object(first) = &items[0].value else { panic!() };
        assert_eq!((first[1].key_start, first[1].value.start, first[1].value.end), (Pos::new(2, 4), Pos::new(2, 10), Pos::new(2, 12)));
        let Value::Object(second) = &items[1].value else { panic!() };
        assert_eq!((second[0].value.start, second[0].value.end), (Pos::new(3, 10), Pos::new(3, 15)));
    }

    #[test]
    fn it_writes_block_style() {
        let doc =
            crate::json::parse(r#"{"name":"fenix","on":"yes","n":"12","list":[1,{"a":1,"b":[true,null]},[2,3]],"empty":{},"none":[],"text":"one\ntwo\n","colon":"a: b","dash":"-x","quote":"it's"}"#)
                .unwrap()
                .node;
        let yaml = write(&doc, "  ");
        assert_eq!(
            yaml,
            "name: fenix\non: \"yes\"\nn: \"12\"\nlist:\n  - 1\n  - a: 1\n    b:\n      - true\n      - null\n  - - 2\n    - 3\nempty: {}\nnone: []\ntext: |\n  one\n  two\ncolon: \"a: b\"\ndash: -x\nquote: it's"
        );
        assert_eq!(parse(&yaml).unwrap().to_json(), doc.to_json(), "it reads back as it was");
    }

    #[test]
    fn strings_that_need_quotes_get_them() {
        for s in ["", " lead", "trail ", "true", "null", "1.5", "0x1F", "# not", "a #b", "key:", "[x]", "{x}", "*ref", "&a", "!t", "|", ">", "%", "@", "`", "---", "No", "off", "a\tb\u{1}", "x, y"] {
            let doc = Node::new(Value::Object(vec![Member { key: s.into(), key_start: Pos::default(), key_end: Pos::default(), value: Node::new(Value::String(s.into())) }]));
            let yaml = write(&doc, "  ");
            assert_eq!(parse(&yaml).unwrap().to_json(), doc.to_json(), "{s:?} as {yaml}");
        }
        for s in ["a\n\nb\n", "a\nb", "a\nb\n\n", "  lead\nx", "trail \nx", "x\r\ny"] {
            let doc = Node::new(Value::Array(vec![Node::new(Value::String(s.into()))]));
            let yaml = write(&doc, "  ");
            assert_eq!(parse(&yaml).unwrap().to_json(), doc.to_json(), "{s:?} as {yaml}");
            let doc = Node::new(Value::Object(vec![Member { key: "k".into(), key_start: Pos::default(), key_end: Pos::default(), value: Node::new(Value::String(s.into())) }]));
            let yaml = write(&doc, "    ");
            assert_eq!(parse(&yaml).unwrap().to_json(), doc.to_json(), "{s:?} as {yaml}");
        }
    }

    #[test]
    fn a_flow_line() {
        let doc = crate::json::parse(r#"{"a":[1,"x, y"],"b":{}}"#).unwrap().node;
        assert_eq!(flow(&doc), r#"{a: [1, "x, y"], b: {}}"#);
        assert_eq!(parse(&flow(&doc)).unwrap().to_json(), doc.to_json());
    }

    #[test]
    fn comments_are_noticed() {
        assert!(has_comments("# top\na: 1\n"));
        assert!(has_comments("a: 1 # one\n"));
        assert!(!has_comments("a: \"x # y\"\nb: c#d\nurl: http://x/#frag\n"));
    }
}
