//! JSON: JSONPath queries (RFC 9535, `serde_json_path`), and a value
//! read from a path for chained requests and tests.

use serde_json::Value;
use serde_json_path::JsonPath;

/// What `path` finds in `doc`, every match.
pub fn query(doc: &Value, path: &str) -> Result<Vec<Value>, String> {
    let p = JsonPath::parse(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(p.query(doc).all().into_iter().cloned().collect())
}

/// The first thing `path` finds in `doc`. A path ending in `.length`
/// (JetBrains' and REST Client's) is how many an array or a string has.
pub fn query_first(doc: &Value, path: &str) -> Result<Option<Value>, String> {
    if let Some(base) = path.strip_suffix(".length") {
        if let Ok(found) = query(doc, base) {
            if let Some(v) = found.first() {
                let n = match v {
                    Value::Array(a) => Some(a.len()),
                    Value::String(s) => Some(s.chars().count()),
                    Value::Object(o) => Some(o.len()),
                    _ => None,
                };
                if let Some(n) = n {
                    return Ok(Some(Value::from(n)));
                }
            }
        }
    }
    Ok(query(doc, path)?.into_iter().next())
}

/// `text` pretty-printed, when it's JSON: laid out again token by token,
/// so its numbers stay as they were written (`1.50`, `1e3`, an id past
/// what a float holds) and its keys in their order.
pub fn pretty(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                out.extend(&chars[start..=i.min(chars.len() - 1)]);
            }
            '{' | '[' => {
                // Empty: kept as `{}`.
                let close = if c == '{' { '}' } else { ']' };
                let next = chars[i + 1..].iter().position(|c| !c.is_whitespace()).map(|n| i + 1 + n);
                if next.is_some_and(|n| chars[n] == close) {
                    out.push(c);
                    out.push(close);
                    i = next.unwrap();
                } else {
                    out.push(c);
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
        i += 1;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_find_what_they_say() {
        let doc: Value = serde_json::from_str(r#"{"items":[{"id":1,"name":"a"},{"id":2,"name":"b"}],"total":2,"title":"hey"}"#).unwrap();
        assert_eq!(query_first(&doc, "$.items[1].name").unwrap(), Some(Value::from("b")));
        assert_eq!(query(&doc, "$.items[*].id").unwrap(), [Value::from(1), Value::from(2)]);
        assert_eq!(query_first(&doc, "$.items.length").unwrap(), Some(Value::from(2)));
        assert_eq!(query_first(&doc, "$.title.length").unwrap(), Some(Value::from(3)));
        assert_eq!(query_first(&doc, "$.nope").unwrap(), None);
        assert!(query_first(&doc, "$[").is_err());
    }

    #[test]
    fn json_keeps_its_order_when_pretty() {
        assert_eq!(pretty(r#"{"z":1,"a":[true,null]}"#).unwrap(), "{\n  \"z\": 1,\n  \"a\": [\n    true,\n    null\n  ]\n}");
        assert_eq!(pretty(r#"{"n": 1.50, "big": 12345678901234567890123, "e": -1.5e3, "s": "a\"b,{", "o": {}, "l": [ ]}"#).unwrap(), "{\n  \"n\": 1.50,\n  \"big\": 12345678901234567890123,\n  \"e\": -1.5e3,\n  \"s\": \"a\\\"b,{\",\n  \"o\": {},\n  \"l\": []\n}");
        assert_eq!(pretty("not json"), None);
    }
}
