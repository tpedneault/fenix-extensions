//! A document read from JSON, YAML or TOML: its values in the order they
//! were written, numbers as written, and where each one is.

use serde_json::Value as Json;

/// A place in a text: line and column, from 0, in characters -- as
/// Fenix's cursor and edits count them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub fn new(line: u32, col: u32) -> Pos {
        Pos { line, col }
    }
}

/// Where `text` ends.
pub fn end_of(text: &str) -> Pos {
    let line = text.matches('\n').count() as u32;
    let last = text.rsplit('\n').next().unwrap_or("");
    Pos::new(line, last.chars().count() as u32)
}

/// Byte offsets in a text as places in it.
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Lines<'a> {
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Lines { text, starts }
    }

    pub fn pos(&self, byte: usize) -> Pos {
        let byte = byte.min(self.text.len());
        let line = self.starts.partition_point(|&s| s <= byte) - 1;
        let start = self.starts[line];
        let col = self.text.get(start..byte).map_or(0, |s| s.chars().count());
        Pos::new(line as u32, col as u32)
    }

    /// Line `line`'s text, without its newline.
    pub fn line(&self, line: u32) -> &'a str {
        let Some(&start) = self.starts.get(line as usize) else { return "" };
        let end = self.starts.get(line as usize + 1).map_or(self.text.len(), |&e| e - 1);
        self.text[start..end].trim_end_matches('\r')
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// As JSON writes it (`-1.5e3`), or `inf`, `-inf`, `nan`, which YAML
    /// and TOML have and JSON hasn't.
    Number(String),
    String(String),
    Array(Vec<Node>),
    /// Its members in order, a key twice if it was written twice.
    Object(Vec<Member>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub value: Value,
    pub start: Pos,
    pub end: Pos,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub key: String,
    pub key_start: Pos,
    pub key_end: Pos,
    pub value: Node,
}

impl Default for Node {
    fn default() -> Node {
        Node::new(Value::Null)
    }
}

impl Node {
    pub fn new(value: Value) -> Node {
        Node { value, start: Pos::default(), end: Pos::default() }
    }

    pub fn at(value: Value, start: Pos, end: Pos) -> Node {
        Node { value, start, end }
    }

    /// As serde_json's value: the last of a key written twice wins, and a
    /// number JSON hasn't is `null`.
    pub fn to_json(&self) -> Json {
        match &self.value {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Number(n) => serde_json::from_str::<serde_json::Number>(n).map(Json::Number).unwrap_or(Json::Null),
            Value::String(s) => Json::String(s.clone()),
            Value::Array(items) => Json::Array(items.iter().map(Node::to_json).collect()),
            Value::Object(members) => {
                let mut map = serde_json::Map::new();
                for m in members {
                    map.insert(m.key.clone(), m.value.to_json());
                }
                Json::Object(map)
            }
        }
    }

    /// From serde_json's value, nowhere in particular.
    pub fn from_json(value: &Json) -> Node {
        Node::new(match value {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Number(n.to_string()),
            Json::String(s) => Value::String(s.clone()),
            Json::Array(items) => Value::Array(items.iter().map(Node::from_json).collect()),
            Json::Object(map) => Value::Object(map.iter().map(|(k, v)| Member { key: k.clone(), key_start: Pos::default(), key_end: Pos::default(), value: Node::from_json(v) }).collect()),
        })
    }

    /// Its objects' keys sorted: in every object, or only its own.
    pub fn sort_keys(&mut self, recursive: bool) {
        match &mut self.value {
            Value::Object(members) => {
                members.sort_by(|a, b| a.key.cmp(&b.key));
                if recursive {
                    members.iter_mut().for_each(|m| m.value.sort_keys(true));
                }
            }
            Value::Array(items) if recursive => items.iter_mut().for_each(|n| n.sort_keys(true)),
            _ => {}
        }
    }
}

/// Whether `n` is a number as JSON writes it.
pub fn is_json_number(n: &str) -> bool {
    let b = n.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let from = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - from
    };
    let int_from = i;
    let int = digits(&mut i);
    if int == 0 || (int > 1 && b[int_from] == b'0') {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// A float as JSON writes it, a point or an exponent in it, or `inf`,
/// `-inf`, `nan`.
pub fn float_text(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        }
    } else {
        // Debug writes `1.0` and `1e30`: JSON's, both.
        format!("{f:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_counted_in_characters() {
        let text = "ab\né😀x\n";
        let lines = Lines::new(text);
        assert_eq!(lines.pos(0), Pos::new(0, 0));
        assert_eq!(lines.pos(3), Pos::new(1, 0));
        assert_eq!(lines.pos(text.find('x').unwrap()), Pos::new(1, 2));
        assert_eq!(lines.line(1), "é😀x");
        assert_eq!(end_of(text), Pos::new(2, 0));
        assert_eq!(end_of("a\nbé"), Pos::new(1, 2));
    }

    #[test]
    fn json_numbers() {
        for good in ["0", "-0", "12", "1.5", "-1e3", "2E+10", "0.0e-1"] {
            assert!(is_json_number(good), "{good}");
        }
        for bad in ["", "-", "01", "1.", ".5", "+1", "1e", "0x1F", "inf", "1_000"] {
            assert!(!is_json_number(bad), "{bad}");
        }
        assert_eq!(float_text(1.0), "1.0");
        assert_eq!(float_text(1e30), "1e30");
        assert_eq!(float_text(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn keys_sort_everywhere_or_at_the_top() {
        let doc = serde_json::json!({"b": {"y": 1, "x": 2}, "a": [{"d": 1, "c": 2}]});
        let mut top = Node::from_json(&doc);
        top.sort_keys(false);
        assert_eq!(serde_json::to_string(&top.to_json()).unwrap(), r#"{"a":[{"d":1,"c":2}],"b":{"y":1,"x":2}}"#);
        let mut all = Node::from_json(&doc);
        all.sort_keys(true);
        assert_eq!(serde_json::to_string(&all.to_json()).unwrap(), r#"{"a":[{"c":2,"d":1}],"b":{"x":2,"y":1}}"#);
    }
}
