//! Requests from elsewhere, as `.http` files: Postman collections (v2.1),
//! Insomnia exports (v4), OpenAPI 3 specs, and `curl` commands.
//!
//! An import is a folder of `.http` files -- a file a folder of the
//! collection's, its requests in it -- and the variables it came with,
//! which go in the folder's `http-client.env.json` (under `$shared`, or an
//! environment of their own when the export had them).

pub mod curl;
pub mod insomnia;
pub mod openapi;
pub mod postman;

use serde_json::{Map, Value};

/// A request as an importer found it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// As the `.http` file writes it: `< ./file` lines and all.
    pub body: Option<String>,
    /// `# @name`-like directives: (name, value).
    pub directives: Vec<(String, String)>,
    /// A comment above it: a description.
    pub note: Option<String>,
}

/// What an import comes to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collection {
    pub name: String,
    /// Each file, below the collection's folder, and its requests.
    pub files: Vec<(String, Vec<Draft>)>,
    /// The environments it had: `$shared` for a collection's variables.
    pub envs: Map<String, Value>,
    /// What it couldn't bring, said once each.
    pub notes: Vec<String>,
}

/// A name made fit for a file or a `# @name`: letters, digits, `-` and
/// `_`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "requests".into()
    } else {
        out
    }
}

/// `drafts` as a `.http` file's text.
pub fn to_http(drafts: &[Draft]) -> String {
    let mut out = String::new();
    let mut names: Vec<String> = Vec::new();
    for d in drafts {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("### {}\n", d.name.lines().next().unwrap_or("").trim()));
        if let Some(note) = &d.note {
            for l in note.lines().filter(|l| !l.trim().is_empty()).take(5) {
                out.push_str(&format!("# {}\n", l.trim()));
            }
        }
        // A name chained requests can use, unique in the file.
        let mut ident = slug(&d.name).replace('-', "_");
        if names.contains(&ident) {
            let mut n = 2;
            while names.contains(&format!("{ident}_{n}")) {
                n += 1;
            }
            ident = format!("{ident}_{n}");
        }
        names.push(ident.clone());
        out.push_str(&format!("# @name {ident}\n"));
        for (k, v) in &d.directives {
            out.push_str(&format!("# @{k}{}\n", if v.is_empty() { String::new() } else { format!(" {v}") }));
        }
        out.push_str(&format!("{} {}\n", if d.method.is_empty() { "GET" } else { &d.method }, d.url));
        for (k, v) in &d.headers {
            out.push_str(&format!("{k}: {v}\n"));
        }
        if let Some(body) = d.body.as_deref().filter(|b| !b.trim().is_empty()) {
            out.push('\n');
            out.push_str(body.trim_end());
            out.push('\n');
        }
    }
    out
}

/// A collection's environments merged into an env file's text: those it
/// has kept, the import's added to them (its own values winning).
pub fn merge_env(existing: Option<&str>, envs: &Map<String, Value>) -> Result<String, String> {
    let mut base: Map<String, Value> = match existing.filter(|t| !t.trim().is_empty()) {
        Some(t) => match serde_json::from_str(t) {
            Ok(Value::Object(m)) => m,
            _ => return Err(format!("{}: isn't an object of environments; not changed", crate::env::PUBLIC)),
        },
        None => Map::new(),
    };
    for (env, vars) in envs {
        let into = base.entry(env.clone()).or_insert_with(|| Value::Object(Map::new()));
        if let (Value::Object(into), Value::Object(vars)) = (into, vars) {
            for (k, v) in vars {
                into.insert(k.clone(), v.clone());
            }
        }
    }
    serde_json::to_string_pretty(&Value::Object(base)).map(|s| s + "\n").map_err(|e| e.to_string())
}

/// YAML read as JSON, for OpenAPI and Insomnia's YAML.
pub fn yaml_to_json(text: &str) -> Result<Value, String> {
    use yaml_rust2::{Yaml, YamlLoader};
    fn conv(y: &Yaml) -> Value {
        match y {
            Yaml::Real(r) => r.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or_else(|| Value::String(r.clone())),
            Yaml::Integer(i) => Value::from(*i),
            Yaml::String(s) => Value::String(s.clone()),
            Yaml::Boolean(b) => Value::Bool(*b),
            Yaml::Array(a) => Value::Array(a.iter().map(conv).collect()),
            Yaml::Hash(h) => {
                let mut m = Map::new();
                for (k, v) in h {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Real(r) => r.clone(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => continue,
                    };
                    m.insert(key, conv(v));
                }
                Value::Object(m)
            }
            _ => Value::Null,
        }
    }
    let docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    Ok(docs.first().map(conv).unwrap_or(Value::Null))
}

/// JSON, or YAML when it isn't.
pub fn read_doc(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(json) => yaml_to_json(text).map_err(|yaml| format!("neither JSON ({json}) nor YAML ({yaml})")),
    }
}

/// What a file holds: which importer reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Postman,
    Insomnia,
    OpenApi,
}

pub fn detect(doc: &Value) -> Option<Format> {
    if doc.get("openapi").is_some() || doc.get("swagger").is_some() {
        return Some(Format::OpenApi);
    }
    if doc.get("_type").and_then(Value::as_str) == Some("export") || doc.get("__export_format").is_some() {
        return Some(Format::Insomnia);
    }
    if doc.get("info").and_then(|i| i.get("_postman_id").or(i.get("schema"))).is_some() || (doc.get("item").is_some() && doc.get("info").is_some()) {
        return Some(Format::Postman);
    }
    None
}

/// A file's collection, whatever it is.
pub fn import(text: &str) -> Result<Collection, String> {
    let doc = read_doc(text)?;
    match detect(&doc) {
        Some(Format::Postman) => postman::import(&doc),
        Some(Format::Insomnia) => insomnia::import(&doc),
        Some(Format::OpenApi) => openapi::import(&doc),
        None => Err("not a Postman collection, an Insomnia export or an OpenAPI spec".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::HttpFile;

    #[test]
    fn drafts_become_a_file_that_reads_back() {
        let drafts = vec![
            Draft { name: "List users".into(), method: "GET".into(), url: "{{baseUrl}}/users".into(), headers: vec![("Accept".into(), "application/json".into())], note: Some("All of them.".into()), ..Draft::default() },
            Draft { name: "List users".into(), method: "POST".into(), url: "{{baseUrl}}/users".into(), body: Some("{\"name\": \"ada\"}".into()), directives: vec![("no-redirect".into(), String::new())], ..Draft::default() },
        ];
        let text = to_http(&drafts);
        assert_eq!(text, "### List users\n# All of them.\n# @name List_users\nGET {{baseUrl}}/users\nAccept: application/json\n\n### List users\n# @name List_users_2\n# @no-redirect\nPOST {{baseUrl}}/users\n\n{\"name\": \"ada\"}\n");
        let f = HttpFile::parse(&text);
        assert_eq!(f.requests.len(), 2);
        assert_eq!(f.requests[1].name(), Some("List_users_2"));
        assert_eq!(f.requests[1].body.as_ref().unwrap().text, "{\"name\": \"ada\"}");
        assert!(f.requests[1].has("no-redirect"));
    }

    #[test]
    fn names_and_env_files() {
        assert_eq!(slug("My API / v2 (beta)"), "My-API-v2-beta");
        assert_eq!(slug("  "), "requests");
        let mut envs = Map::new();
        envs.insert("$shared".into(), serde_json::json!({"baseUrl": "https://x.test", "k": "new"}));
        let merged = merge_env(Some(r#"{"dev": {"a": 1}, "$shared": {"k": "old", "keep": true}}"#), &envs).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["$shared"]["k"], "new");
        assert_eq!(v["$shared"]["keep"], true);
        assert_eq!(v["dev"]["a"], 1);
        assert!(merge_env(Some("[]"), &envs).is_err());
    }

    #[test]
    fn yaml_reads_as_json() {
        let v = read_doc("openapi: 3.0.0\ninfo:\n  title: T\n  version: 1\npaths: {}\nn: 1.5\n").unwrap();
        assert_eq!(v["info"]["title"], "T");
        assert_eq!(v["n"], 1.5);
        assert_eq!(detect(&v), Some(Format::OpenApi));
        assert!(import("just: text").is_err());
    }
}
