//! A Secret, shown without its values. kubectl gives it as JSON
//! (`kubectl::get`); before anything else sees it, every value under
//! `data` and `stringData` is replaced by how long it is, and so is every
//! annotation's and label's value: any of them can copy the data
//! (`kubectl.kubernetes.io/last-applied-configuration` holds the whole
//! Secret as it was applied; tools write checksums and configs there too).
//! What's left is written as YAML, its keys in place: you see which keys
//! it has, never what they hold. Nothing here keeps the JSON it was given.

use serde_json::{Map, Value};

/// `secret` (kubectl's JSON) with its values taken out; an error, holding
/// nothing of the text, when it isn't one.
pub fn redact(json: &str) -> Result<Value, String> {
    let mut v: Value = serde_json::from_str(json).map_err(|_| "kubectl's answer wasn't the Secret's JSON".to_string())?;
    if v.get("kind").and_then(Value::as_str) != Some("Secret") {
        return Err("kubectl's answer wasn't a Secret".into());
    }
    for field in ["data", "stringData"] {
        if let Some(Value::Object(map)) = v.get_mut(field) {
            for (_, value) in map.iter_mut() {
                let bytes = match value {
                    // `data` is base64: the length of what it holds.
                    Value::String(s) if field == "data" => decoded_len(s),
                    Value::String(s) => s.len(),
                    _ => 0,
                };
                *value = Value::String(format!("<hidden: {bytes} bytes>"));
            }
        }
    }
    for field in ["/metadata/annotations", "/metadata/labels"] {
        if let Some(Value::Object(map)) = v.pointer_mut(field) {
            for (_, value) in map.iter_mut() {
                let bytes = value.as_str().map_or(0, str::len);
                *value = Value::String(format!("<hidden: {bytes} bytes>"));
            }
        }
    }
    // managedFields name the keys (not their values), but say nothing
    // you need here.
    if let Some(Value::Object(meta)) = v.get_mut("metadata") {
        meta.remove("managedFields");
    }
    Ok(v)
}

/// How many bytes base64 `s` decodes to.
fn decoded_len(s: &str) -> usize {
    let s = s.trim();
    let pad = s.chars().rev().take_while(|c| *c == '=').count();
    (s.len() / 4) * 3 + match s.len() % 4 {
        2 => 1,
        3 => 2,
        _ => 0,
    } - pad.min((s.len() / 4) * 3)
}

/// The keys a redacted Secret holds.
pub fn keys(v: &Value) -> Vec<String> {
    ["data", "stringData"].iter().filter_map(|f| v.get(*f).and_then(Value::as_object)).flat_map(|m| m.keys().cloned()).collect()
}

/// `v` as YAML: block mappings and sequences, two spaces a level, a
/// string plain only when YAML can't read it as anything else, quoted as
/// JSON quotes it otherwise (which YAML reads the same).
pub fn yaml(v: &Value) -> String {
    let mut out = String::new();
    match v {
        Value::Object(m) if !m.is_empty() => map(m, 0, &mut out),
        Value::Array(a) if !a.is_empty() => seq(a, 0, &mut out),
        other => {
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
    out
}

fn map(m: &Map<String, Value>, indent: usize, out: &mut String) {
    for (k, v) in m {
        out.push_str(&" ".repeat(indent));
        out.push_str(&string(k));
        out.push(':');
        child(v, indent, out);
    }
}

fn seq(a: &[Value], indent: usize, out: &mut String) {
    for v in a {
        out.push_str(&" ".repeat(indent));
        out.push('-');
        match v {
            Value::Object(m) if !m.is_empty() => {
                // The first key on the dash's line, the rest under it.
                let mut inner = String::new();
                map(m, indent + 2, &mut inner);
                out.push(' ');
                out.push_str(inner.trim_start());
            }
            _ => child(v, indent, out),
        }
    }
}

fn child(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            map(m, indent + 2, out);
        }
        Value::Array(a) if !a.is_empty() => {
            out.push('\n');
            seq(a, indent + 2, out);
        }
        other => {
            out.push(' ');
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => string(s),
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
        other => other.to_string(),
    }
}

/// A string plain when it's safe to be (`web`, `kubernetes.io/tls`),
/// else in double quotes.
fn string(s: &str) -> String {
    const WORDS: [&str; 12] = ["true", "false", "yes", "no", "on", "off", "null", "~", "y", "n", "<<", "="];
    let plain = !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && !WORDS.contains(&s.to_ascii_lowercase().as_str());
    if plain {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Secret as `kubectl get secret -o json` gives it, hand-written from
    /// the API's documented shape, with its values in the places they can
    /// hide.
    const SECRET: &str = r#"{
        "apiVersion": "v1",
        "kind": "Secret",
        "type": "Opaque",
        "metadata": {
            "name": "db-credentials",
            "namespace": "shop",
            "annotations": {
                "kubectl.kubernetes.io/last-applied-configuration": "{\"apiVersion\":\"v1\",\"data\":{\"password\":\"aHVudGVyMg==\"},\"kind\":\"Secret\"}\n",
                "team": "payments",
                "checksum/config": "cGFzc3dvcmQ9aHVudGVyMg=="
            },
            "labels": {"app": "db", "token": "s3cr3t-t0ken"},
            "managedFields": [{"manager": "kubectl", "fieldsV1": {"f:data": {"f:password": {}}}}]
        },
        "data": {"password": "aHVudGVyMg==", "username": "YWRtaW4="},
        "stringData": {"token": "s3cr3t-t0ken"}
    }"#;

    #[test]
    fn its_values_never_reach_what_is_shown() {
        let v = redact(SECRET).unwrap();
        let text = yaml(&v);
        for secret in ["aHVudGVyMg", "hunter2", "YWRtaW4", "admin", "s3cr3t-t0ken"] {
            assert!(!text.contains(secret), "{secret} in:\n{text}");
        }
        assert!(text.contains("password: \"<hidden: 7 bytes>\""), "{text}");
        assert!(text.contains("username: \"<hidden: 5 bytes>\""), "{text}");
        assert!(text.contains("token: \"<hidden: 12 bytes>\""), "{text}");
        assert!(text.contains("team: \"<hidden: 8 bytes>\"") && text.contains("checksum/config: \"<hidden: 24 bytes>\""), "every annotation's value hidden, its key kept: {text}");
        assert!(text.contains("app: \"<hidden: 2 bytes>\"") && !text.contains("cGFzc3dvcmQ"), "labels too: {text}");
        assert!(!text.contains("managedFields"));
        assert_eq!(keys(&v), ["password", "username", "token"]);
    }

    #[test]
    fn what_isnt_a_secret_is_refused_without_saying_what_it_held() {
        let e = redact(r#"{"kind":"ConfigMap","data":{"x":"hunter2"}}"#).unwrap_err();
        assert!(!e.contains("hunter2"));
        let e = redact("not json hunter2").unwrap_err();
        assert!(!e.contains("hunter2"));
    }

    #[test]
    fn yaml_is_written_so_it_reads_back_the_same() {
        let v: Value = serde_json::from_str(r#"{"a":"plain","b":"true","c":"10","d":"a: b","e":"","f":[1,"x",{"g":"h","i":null}],"j":{},"k":[],"l":"line\nnext","m":false}"#).unwrap();
        assert_eq!(yaml(&v), "a: plain\nb: \"true\"\nc: \"10\"\nd: \"a: b\"\ne: \"\"\nf:\n  - 1\n  - x\n  - g: h\n    i: null\nj: {}\nk: []\nl: \"line\\nnext\"\nm: false\n");
        assert_eq!(decoded_len("YQ=="), 1);
        assert_eq!(decoded_len("YWI="), 2);
        assert_eq!(decoded_len("YWJj"), 3);
        assert_eq!(decoded_len(""), 0);
    }
}
