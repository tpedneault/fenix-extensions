//! Tests on a response, a `# @test` line each in the request:
//!
//! ```text
//! # @test status == 200
//! # @test status < 400
//! # @test time < 500
//! # @test header Content-Type contains json
//! # @test $.items.length > 0
//! # @test $.user.name == "ada"
//! # @test $.token exists
//! # @test body contains "ok"
//! ```
//!
//! What's tested is `status`, `time` (milliseconds), `size` (bytes),
//! `body`, `header <name>`, or a JSONPath into a JSON body; then `==`,
//! `!=`, `<`, `<=`, `>`, `>=`, `contains`, `!contains`, `startsWith`,
//! `endsWith`, `exists` or `!exists`; then a value, JSON (`"ada"`, `3`,
//! `true`, `null`) or a word.
//!
//! JetBrains' handler scripts (`> {% client.test(...) %}`) are JavaScript,
//! which an extension has nothing to run; these say the usual checks
//! without it.

use serde_json::Value;

use crate::model::Received;

/// One test, and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub test: String,
    pub passed: bool,
    /// What was found, or why it couldn't be tested.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Exists,
    Missing,
}

fn op(word: &str) -> Option<Op> {
    Some(match word {
        "==" | "=" | "is" => Op::Eq,
        "!=" | "<>" => Op::Ne,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        "contains" => Op::Contains,
        "!contains" => Op::NotContains,
        "startsWith" | "startswith" => Op::StartsWith,
        "endsWith" | "endswith" => Op::EndsWith,
        "exists" => Op::Exists,
        "!exists" | "missing" => Op::Missing,
        _ => return None,
    })
}

/// The first word of `s`, a JSONPath's brackets and quotes kept whole.
fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let (mut depth, mut quote) = (0i32, None::<char>);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') => depth -= 1,
            (None, c) if c.is_whitespace() && depth <= 0 => return (&s[..i], s[i..].trim_start()),
            _ => {}
        }
    }
    (s, "")
}

fn literal(text: &str) -> Value {
    let t = text.trim();
    serde_json::from_str(t).unwrap_or_else(|_| Value::String(t.to_string()))
}

fn as_text(v: &Value) -> String {
    crate::env::as_text(v)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn compare(found: Option<&Value>, op: Op, want: &Value) -> bool {
    match op {
        Op::Exists => found.is_some_and(|v| !v.is_null()),
        Op::Missing => found.is_none_or(Value::is_null),
        _ => {
            let Some(found) = found else { return false };
            match op {
                Op::Eq | Op::Ne => {
                    let same = match (number(found), number(want)) {
                        (Some(a), Some(b)) if !matches!((found, want), (Value::String(_), Value::String(_))) => a == b,
                        _ => found == want || as_text(found) == as_text(want),
                    };
                    same == (op == Op::Eq)
                }
                Op::Lt | Op::Le | Op::Gt | Op::Ge => match (number(found), number(want)) {
                    (Some(a), Some(b)) => match op {
                        Op::Lt => a < b,
                        Op::Le => a <= b,
                        Op::Gt => a > b,
                        _ => a >= b,
                    },
                    _ => false,
                },
                Op::Contains | Op::NotContains => {
                    let has = match found {
                        Value::Array(items) => items.iter().any(|i| i == want || as_text(i) == as_text(want)),
                        other => as_text(other).contains(&as_text(want)),
                    };
                    has == (op == Op::Contains)
                }
                Op::StartsWith => as_text(found).starts_with(&as_text(want)),
                Op::EndsWith => as_text(found).ends_with(&as_text(want)),
                Op::Exists | Op::Missing => unreachable!(),
            }
        }
    }
}

/// How test `test` goes on `r`, which took `millis`.
pub fn check(test: &str, r: &Received, millis: u64) -> Outcome {
    let out = |passed: bool, detail: String| Outcome { test: test.to_string(), passed, detail };
    let (subject, rest) = first_word(test);
    let (subject, rest) = if subject.eq_ignore_ascii_case("header") {
        let (name, rest) = first_word(rest);
        (format!("header {name}"), rest)
    } else {
        (subject.to_string(), rest)
    };
    let (op_word, value) = first_word(rest);
    let Some(op) = op(op_word) else { return out(false, format!("`{op_word}` isn't a test: ==, !=, <, >, contains, exists...")) };
    let want = literal(value);
    let found: Option<Value> = match subject.as_str() {
        "status" => Some(Value::from(r.status)),
        "time" => Some(Value::from(millis)),
        "size" => Some(Value::from(r.body.len())),
        "body" => Some(Value::String(r.text())),
        s if s.starts_with("header ") => r.header(&s[7..]).map(|v| Value::String(v.to_string())),
        s if s.starts_with('$') => match serde_json::from_slice::<Value>(&r.body) {
            Ok(doc) => match crate::json::query_first(&doc, s) {
                Ok(v) => v,
                Err(e) => return out(false, e),
            },
            Err(_) => return out(false, "the body isn't JSON".into()),
        },
        other => return out(false, format!("`{other}`: test status, time, size, body, header <name> or a JSONPath ($.id)")),
    };
    let passed = compare(found.as_ref(), op, &want);
    let detail = match &found {
        Some(v) => {
            let t = as_text(v);
            if t.chars().count() > 60 {
                format!("{}…", t.chars().take(60).collect::<String>())
            } else {
                t
            }
        }
        None => "nothing there".into(),
    };
    out(passed, detail)
}

/// Every test of a request, on its answer.
pub fn check_all<'a>(tests: impl IntoIterator<Item = &'a str>, r: &Received, millis: u64) -> Vec<Outcome> {
    tests.into_iter().map(|t| check(t, r, millis)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> Received {
        Received {
            status: 201,
            headers: vec![("Content-Type".into(), "application/json; charset=utf-8".into())],
            body: br#"{"id":"7","user":{"name":"ada","roles":["admin","dev"]},"items":[1,2,3],"token":"t","gone":null}"#.to_vec(),
            ..Received::default()
        }
    }

    fn passes(test: &str) -> bool {
        check(test, &answer(), 120).passed
    }

    #[test]
    fn status_time_size_and_headers() {
        assert!(passes("status == 201"));
        assert!(!passes("status == 200"));
        assert!(passes("status < 300"));
        assert!(passes("status >= 201"));
        assert!(passes("status != 404"));
        assert!(passes("time < 500"));
        assert!(!passes("time > 500"));
        assert!(passes("size > 10"));
        assert!(passes("header content-type contains json"));
        assert!(passes("header Content-Type startsWith application/"));
        assert!(passes("header X-Nope !exists"));
        assert!(passes("body contains \"admin\""));
    }

    #[test]
    fn jsonpath_tests() {
        assert!(passes("$.user.name == \"ada\""));
        assert!(passes("$.user.name == ada"));
        assert!(passes("$.id == 7"), "a number in a string is the number");
        assert!(!passes("$.id == \"8\""));
        assert!(passes("$.items.length == 3"));
        assert!(passes("$.items[0] < 2"));
        assert!(passes("$.user.roles contains admin"));
        assert!(passes("$.user.roles !contains root"));
        assert!(passes("$.token exists"));
        assert!(passes("$.gone !exists"), "null is missing");
        assert!(passes("$.nothing !exists"));
        assert!(passes("$['user']['name'] endsWith da"));
    }

    #[test]
    fn what_cant_be_tested_says_why() {
        let o = check("$.user.name is_like ada", &answer(), 0);
        assert!(!o.passed && o.detail.contains("isn't a test"));
        let o = check("colour == red", &answer(), 0);
        assert!(!o.passed && o.detail.contains("JSONPath"));
        let r = Received { body: b"<html>".to_vec(), ..answer() };
        assert_eq!(check("$.a exists", &r, 0).detail, "the body isn't JSON");
        let o = check("$.user.name == bob", &answer(), 0);
        assert_eq!((o.passed, o.detail.as_str()), (false, "ada"));
    }
}
