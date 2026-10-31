//! JSON read with where each value is, numbers kept as written; and
//! written out, indented or minified. Comments and trailing commas (as
//! in `tsconfig.json`) are read, and said to be there.

use crate::node::{Member, Node, Pos, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub message: String,
    pub at: Pos,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}: {}", self.at.line + 1, self.at.col + 1, self.message)
    }
}

/// What reading a document found besides its value.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Read {
    pub node: Node,
    /// It had comments, which writing it again drops.
    pub comments: bool,
}

struct Parser {
    chars: Vec<char>,
    i: usize,
    pos: Pos,
    comments: bool,
}

pub fn parse(text: &str) -> Result<Read, Error> {
    let mut p = Parser { chars: text.chars().collect(), i: 0, pos: Pos::default(), comments: false };
    // A byte-order mark isn't the document's.
    if p.peek() == Some('\u{feff}') {
        p.bump();
    }
    p.space()?;
    let node = p.value(0)?;
    p.space()?;
    if p.peek().is_some() {
        return Err(p.error("more after the document"));
    }
    Ok(Read { node, comments: p.comments })
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.pos = Pos::new(self.pos.line + 1, 0);
        } else {
            self.pos.col += 1;
        }
        Some(c)
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error { message: message.into(), at: self.pos }
    }

    /// Whitespace and comments.
    fn space(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\n' | '\r') => {
                    self.bump();
                }
                Some('/') if self.chars.get(self.i + 1) == Some(&'/') => {
                    self.comments = true;
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                Some('/') if self.chars.get(self.i + 1) == Some(&'*') => {
                    self.comments = true;
                    let at = self.pos;
                    self.bump();
                    self.bump();
                    loop {
                        match self.bump() {
                            Some('*') if self.peek() == Some('/') => {
                                self.bump();
                                break;
                            }
                            Some(_) => {}
                            None => return Err(Error { message: "a comment that isn't closed (*/)".into(), at }),
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn expect(&mut self, word: &str) -> Result<(), Error> {
        for want in word.chars() {
            if self.peek() != Some(want) {
                return Err(self.error(format!("expected {word}")));
            }
            self.bump();
        }
        if self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return Err(self.error(format!("expected {word}")));
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Node, Error> {
        if depth > 512 {
            return Err(self.error("nested too deep"));
        }
        let start = self.pos;
        let value = match self.peek() {
            Some('{') => self.object(depth)?,
            Some('[') => self.array(depth)?,
            Some('"') => Value::String(self.string()?),
            Some('t') => {
                self.expect("true")?;
                Value::Bool(true)
            }
            Some('f') => {
                self.expect("false")?;
                Value::Bool(false)
            }
            Some('n') => {
                self.expect("null")?;
                Value::Null
            }
            Some(c) if c == '-' || c.is_ascii_digit() => Value::Number(self.number()?),
            Some(c) => return Err(self.error(format!("unexpected {c:?}: a value is an object, an array, a string, a number, true, false or null"))),
            None => return Err(self.error("the document ends where a value should be")),
        };
        Ok(Node::at(value, start, self.pos))
    }

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        self.bump();
        let mut members = Vec::new();
        loop {
            self.space()?;
            match self.peek() {
                Some('}') => {
                    self.bump();
                    return Ok(Value::Object(members));
                }
                Some('"') => {}
                Some(c) => return Err(self.error(format!("unexpected {c:?}: a key is a string in double quotes"))),
                None => return Err(self.error("the object isn't closed (})")),
            }
            let key_start = self.pos;
            let key = self.string()?;
            let key_end = self.pos;
            self.space()?;
            if self.peek() != Some(':') {
                return Err(self.error("expected : after the key"));
            }
            self.bump();
            self.space()?;
            let value = self.value(depth + 1)?;
            members.push(Member { key, key_start, key_end, value });
            self.space()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {}
                Some(c) => return Err(self.error(format!("unexpected {c:?}: expected , or }}"))),
                None => return Err(self.error("the object isn't closed (})")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        self.bump();
        let mut items = Vec::new();
        loop {
            self.space()?;
            match self.peek() {
                Some(']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                None => return Err(self.error("the array isn't closed (])")),
                _ => {}
            }
            items.push(self.value(depth + 1)?);
            self.space()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {}
                Some(c) => return Err(self.error(format!("unexpected {c:?}: expected , or ]"))),
                None => return Err(self.error("the array isn't closed (])")),
            }
        }
    }

    fn number(&mut self) -> Result<String, Error> {
        let start = self.i;
        let at = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.')) {
            self.bump();
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if crate::node::is_json_number(&text) {
            Ok(text)
        } else {
            Err(Error { message: format!("{text} isn't a number JSON has"), at })
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        let at = self.pos;
        self.bump();
        let mut out = String::new();
        loop {
            let here = self.pos;
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => out.push(self.escape(here)?),
                Some('\n') | None => return Err(Error { message: "the string isn't closed (\")".into(), at }),
                Some(c) if (c as u32) < 0x20 => return Err(Error { message: format!("a control character (U+{:04X}) in a string: it's written \\u{:04x}", c as u32, c as u32), at: here }),
                Some(c) => out.push(c),
            }
        }
    }

    fn hex4(&mut self, at: Pos) -> Result<u32, Error> {
        let mut n = 0;
        for _ in 0..4 {
            let d = self.peek().and_then(|c| c.to_digit(16)).ok_or_else(|| Error { message: "\\u takes four hex digits".into(), at })?;
            self.bump();
            n = n * 16 + d;
        }
        Ok(n)
    }

    fn escape(&mut self, at: Pos) -> Result<char, Error> {
        Ok(match self.bump() {
            Some('"') => '"',
            Some('\\') => '\\',
            Some('/') => '/',
            Some('b') => '\u{8}',
            Some('f') => '\u{c}',
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('u') => {
                let high = self.hex4(at)?;
                if (0xD800..0xDC00).contains(&high) {
                    if self.peek() != Some('\\') || self.chars.get(self.i + 1) != Some(&'u') {
                        return Err(Error { message: "half a surrogate pair".into(), at });
                    }
                    self.bump();
                    self.bump();
                    let low = self.hex4(at)?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return Err(Error { message: "half a surrogate pair".into(), at });
                    }
                    char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)).ok_or_else(|| Error { message: "not a character".into(), at })?
                } else {
                    char::from_u32(high).ok_or_else(|| Error { message: "half a surrogate pair".into(), at })?
                }
            }
            Some(c) => return Err(Error { message: format!("\\{c} isn't an escape JSON has"), at }),
            None => return Err(Error { message: "the string isn't closed (\")".into(), at }),
        })
    }
}

/// `s` as a JSON string, in its quotes.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON string literal's text: `"a\nb"` in its quotes, or what would be
/// between them.
pub fn unquote(literal: &str) -> Result<String, Error> {
    let t = literal.trim();
    let quoted = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') { t.to_string() } else { format!("\"{}\"", t.replace('\n', "\\n").replace('\r', "\\r")) };
    let mut p = Parser { chars: quoted.chars().collect(), i: 0, pos: Pos::default(), comments: false };
    let s = p.string()?;
    if p.peek().is_some() {
        return Err(p.error("a \" in the string that isn't escaped (\\\")"));
    }
    Ok(s)
}

/// How a document is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Style {
    /// A member or an item a line, indented by this.
    Pretty(String),
    Minified,
}

/// `node` as JSON. A number JSON hasn't (YAML's `.inf`) is refused.
pub fn write(node: &Node, style: &Style) -> Result<String, String> {
    let mut out = String::new();
    write_node(node, style, 0, &mut out)?;
    Ok(out)
}

fn newline(style: &Style, depth: usize, out: &mut String) {
    if let Style::Pretty(unit) = style {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(unit);
        }
    }
}

fn write_node(node: &Node, style: &Style, depth: usize, out: &mut String) -> Result<(), String> {
    match &node.value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if crate::node::is_json_number(n) => out.push_str(n),
        Value::Number(n) => return Err(format!("{n} isn't a number JSON has")),
        Value::String(s) => out.push_str(&quote(s)),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(style, depth + 1, out);
                write_node(item, style, depth + 1, out)?;
            }
            newline(style, depth, out);
            out.push(']');
        }
        Value::Object(members) if members.is_empty() => out.push_str("{}"),
        Value::Object(members) => {
            out.push('{');
            for (i, m) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(style, depth + 1, out);
                out.push_str(&quote(&m.key));
                out.push(':');
                if matches!(style, Style::Pretty(_)) {
                    out.push(' ');
                }
                write_node(&m.value, style, depth + 1, out)?;
            }
            newline(style, depth, out);
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pretty(text: &str) -> String {
        write(&parse(text).unwrap().node, &Style::Pretty("  ".into())).unwrap()
    }

    #[test]
    fn it_formats_with_the_indent_given() {
        let text = r#"{"name":"fenix","tags":["a","b"],"empty":{},"none":[],"n":{"x":1.50,"y":-2e10}}"#;
        assert_eq!(pretty(text), "{\n  \"name\": \"fenix\",\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ],\n  \"empty\": {},\n  \"none\": [],\n  \"n\": {\n    \"x\": 1.50,\n    \"y\": -2e10\n  }\n}");
        let tabs = write(&parse("[1,[2]]").unwrap().node, &Style::Pretty("\t".into())).unwrap();
        assert_eq!(tabs, "[\n\t1,\n\t[\n\t\t2\n\t]\n]");
    }

    #[test]
    fn it_minifies_and_keeps_key_order_and_numbers() {
        let text = "{\n  \"z\": 1,\n  \"a\": [ 1.0 , 2 ],\n  \"big\": 123456789012345678901234567890\n}";
        assert_eq!(write(&parse(text).unwrap().node, &Style::Minified).unwrap(), r#"{"z":1,"a":[1.0,2],"big":123456789012345678901234567890}"#);
    }

    #[test]
    fn formatting_twice_is_formatting_once() {
        let once = pretty(r#"{"a":[{"b":null,"c":true}],"d":"é\u00e9\n"}"#);
        assert_eq!(pretty(&once), once);
        assert!(once.contains(r#""d": "éé\n""#));
    }

    #[test]
    fn a_key_written_twice_stays_twice() {
        assert_eq!(write(&parse(r#"{"a":1,"a":2}"#).unwrap().node, &Style::Minified).unwrap(), r#"{"a":1,"a":2}"#);
    }

    #[test]
    fn comments_and_trailing_commas_are_read() {
        let read = parse("// top\n{\n  /* a */ \"a\": 1, // one\n  \"b\": [1, 2,],\n}\n").unwrap();
        assert!(read.comments);
        assert_eq!(write(&read.node, &Style::Minified).unwrap(), r#"{"a":1,"b":[1,2]}"#);
        assert!(!parse("{\"url\": \"http://x\"}").unwrap().comments);
    }

    #[test]
    fn errors_say_where() {
        let e = parse("{\n  \"a\": 1\n  \"b\": 2\n}").unwrap_err();
        assert_eq!((e.at, e.message.as_str()), (Pos::new(2, 2), "unexpected '\"': expected , or }"));
        assert_eq!(parse("[1, 2").unwrap_err().message, "the array isn't closed (])");
        assert_eq!(parse("{\"a\": 01}").unwrap_err().message, "01 isn't a number JSON has");
        assert_eq!(parse("[tru]").unwrap_err().message, "expected true");
        assert_eq!(parse("{} {}").unwrap_err().message, "more after the document");
        assert_eq!(parse("{'a': 1}").unwrap_err().at, Pos::new(0, 1));
        assert_eq!(parse("\"a\\qb\"").unwrap_err().message, "\\q isn't an escape JSON has");
        assert!(parse("").is_err());
    }

    #[test]
    fn values_know_where_they_are() {
        let read = parse("{\n  \"servers\": [\n    {\"port\": 80}\n  ]\n}").unwrap();
        let Value::Object(top) = &read.node.value else { panic!() };
        assert_eq!((top[0].key_start, top[0].key_end), (Pos::new(1, 2), Pos::new(1, 11)));
        let Value::Array(items) = &top[0].value.value else { panic!() };
        assert_eq!((items[0].start, items[0].end), (Pos::new(2, 4), Pos::new(2, 16)));
    }

    #[test]
    fn strings_escape_and_unescape() {
        assert_eq!(quote("a\"b\\c\nd\te\u{1}é"), r#""a\"b\\c\nd\te\u0001é""#);
        assert_eq!(unquote(r#""a\"b\\c\nd\te\u0001\u00e9\ud83d\ude00""#).unwrap(), "a\"b\\c\nd\te\u{1}é😀");
        assert_eq!(unquote(r"line\none").unwrap(), "line\none", "without its quotes");
        assert_eq!(unquote(&quote("round\ttrip \"x\"")).unwrap(), "round\ttrip \"x\"");
        assert!(unquote(r#""\ud83d""#).is_err());
        assert!(unquote(r#"a"b"#).is_err());
        let control = parse("\"a\u{1}b\"").unwrap_err();
        assert!(control.message.contains("U+0001"));
    }
}
