//! Which JSON Schema a file is checked against -- one picked for it, the
//! one it names (`$schema`, YAML's `# yaml-language-server: $schema=`,
//! TOML's `#:schema`), or SchemaStore's for its name -- and checking it,
//! its problems placed where they are.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::node::{Node, Value as V};

/// A schema SchemaStore's catalog lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub url: String,
    pub file_match: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub entries: Vec<Entry>,
}

impl Catalog {
    /// SchemaStore's `catalog.json`.
    pub fn parse(text: &str) -> Result<Catalog, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("the catalog: {e}"))?;
        let entries = v
            .get("schemas")
            .and_then(Value::as_array)
            .ok_or("the catalog has no schemas")?
            .iter()
            .filter_map(|s| {
                let text = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let url = text("url");
                (!url.is_empty()).then(|| Entry {
                    name: text("name"),
                    description: text("description"),
                    url,
                    file_match: s.get("fileMatch").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
                })
            })
            .collect();
        Ok(Catalog { entries })
    }

    /// The first schema for a file by its path, as SchemaStore matches.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| {
            let (not, yes): (Vec<&String>, Vec<&String>) = e.file_match.iter().partition(|p| p.starts_with('!'));
            yes.iter().any(|p| crate::glob::file_matches(p, path)) && not.iter().all(|p| crate::glob::file_matches(p, path))
        })
    }
}

/// The schema a document names itself.
pub fn declared(text: &str, node: &Node) -> Option<String> {
    for line in text.lines().take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')) {
        let comment = line.trim_start().trim_start_matches('#').trim();
        if let Some(url) = comment.strip_prefix("yaml-language-server:").map(str::trim).and_then(|r| r.strip_prefix("$schema=")) {
            return Some(url.trim().to_string());
        }
        if let Some(url) = line.trim_start().strip_prefix("#:schema") {
            return Some(url.trim().to_string());
        }
    }
    match &node.value {
        V::Object(members) => members.iter().find(|m| m.key == "$schema").and_then(|m| match &m.value.value {
            V::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        }),
        _ => None,
    }
}

/// Where a schema is: a URL, or a file by its full path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Url(String),
    File(String),
}

impl Source {
    pub fn key(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::File(f) => format!("file://{f}"),
        }
    }
}

/// `reference` (a URL, or a path from the file `beside`'s folder) as a
/// schema's place.
pub fn source(reference: &str, beside: &str) -> Source {
    let r = reference.trim();
    if r.starts_with("http://") || r.starts_with("https://") {
        return Source::Url(without_fragment(r));
    }
    if let Some(path) = r.strip_prefix("file://") {
        return Source::File(path.to_string());
    }
    let r = r.replace('\\', "/");
    let full = if r.starts_with('/') || r.chars().nth(1) == Some(':') {
        r
    } else {
        let beside = beside.replace('\\', "/");
        let dir = beside.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{dir}/{r}")
    };
    Source::File(normalize(&full))
}

fn without_fragment(url: &str) -> String {
    url.split('#').next().unwrap_or(url).to_string()
}

/// `a/./b/../c` as `a/c`.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for (i, part) in path.split('/').enumerate() {
        match part {
            "." => {}
            "" if i > 0 => {}
            ".." if out.len() > 1 || out.first().is_some_and(|f| !f.is_empty()) => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// A problem a schema found: where (a JSON Pointer), and what.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub pointer: String,
    pub message: String,
}

/// Schemas by URL (no fragment), for `$ref`s to other files.
#[derive(Clone, Default)]
pub struct Store(pub Arc<Mutex<HashMap<String, Value>>>);

struct Retriever {
    store: Store,
    missing: Arc<Mutex<Vec<String>>>,
}

impl jsonschema::Retrieve for Retriever {
    fn retrieve(&self, uri: &jsonschema::Uri<String>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let key = without_fragment(uri.as_str());
        if let Some(v) = self.store.0.lock().ok().and_then(|s| s.get(&key).cloned()) {
            return Ok(v);
        }
        if let Ok(mut m) = self.missing.lock() {
            if !m.contains(&key) {
                m.push(key.clone());
            }
        }
        Err(format!("{key} isn't fetched yet").into())
    }
}

/// A schema made ready to check with, once what it refers to is there.
pub enum Compiled {
    Ready(Box<jsonschema::Validator>),
    Needs(Vec<String>),
    Broken(String),
}

pub fn compile(schema: &Value, base: Option<&str>, store: &Store) -> Compiled {
    let missing = Arc::new(Mutex::new(Vec::new()));
    let mut options = jsonschema::options().with_retriever(Retriever { store: store.clone(), missing: missing.clone() });
    if let Some(base) = base.filter(|b| b.starts_with("http")) {
        options = options.with_base_uri(base.to_string());
    }
    let built = options.build(schema);
    let missing = missing.lock().map(|m| m.clone()).unwrap_or_default();
    match built {
        Ok(v) if missing.is_empty() => Compiled::Ready(Box::new(v)),
        _ if !missing.is_empty() => Compiled::Needs(missing),
        Ok(v) => Compiled::Ready(Box::new(v)),
        Err(e) => Compiled::Broken(e.to_string()),
    }
}

pub fn check(validator: &jsonschema::Validator, instance: &Value) -> Vec<Problem> {
    let mut out: Vec<Problem> = validator.iter_errors(instance).map(|e| Problem { pointer: e.instance_path().as_str().to_string(), message: e.to_string() }).collect();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What checking came to.
    #[derive(Debug, PartialEq)]
    pub enum Checked {
        Problems(Vec<Problem>),
        /// Schemas it refers to that aren't fetched yet, by URL.
        Needs(Vec<String>),
        /// The schema itself is wrong.
        Broken(String),
    }

    /// `check` with the schema compiled first.
    pub fn check_with(schema: &Value, base: Option<&str>, store: &Store, instance: &Value) -> Checked {
        match compile(schema, base, store) {
            Compiled::Ready(v) => Checked::Problems(check(&v, instance)),
            Compiled::Needs(n) => Checked::Needs(n),
            Compiled::Broken(e) => Checked::Broken(e),
        }
    }

    const CATALOG: &str = r#"{"schemas": [
        {"name": "package.json", "description": "npm", "fileMatch": ["package.json"], "url": "https://www.schemastore.org/package.json"},
        {"name": "GitHub Workflow", "fileMatch": ["**/.github/workflows/*.yml", "**/.github/workflows/*.yaml"], "url": "https://www.schemastore.org/github-workflow.json"},
        {"name": "tsconfig", "fileMatch": ["tsconfig.json", "tsconfig.*.json", "!tsconfig.skip.json"], "url": "https://www.schemastore.org/tsconfig.json"},
        {"name": "no url", "fileMatch": ["x.json"]}
    ]}"#;

    #[test]
    fn the_catalog_matches_files() {
        let c = Catalog::parse(CATALOG).unwrap();
        assert_eq!(c.entries.len(), 3);
        assert_eq!(c.find("/p/package.json").unwrap().name, "package.json");
        assert_eq!(c.find("/r/.github/workflows/ci.yml").unwrap().name, "GitHub Workflow");
        assert_eq!(c.find("/r/tsconfig.app.json").unwrap().name, "tsconfig");
        assert!(c.find("/r/tsconfig.skip.json").is_none());
        assert!(c.find("/r/other.json").is_none());
        assert!(Catalog::parse("{}").is_err());
    }

    #[test]
    fn what_a_file_names() {
        let json = r#"{"$schema": "./schema.json", "a": 1}"#;
        assert_eq!(declared(json, &crate::json::parse(json).unwrap().node).as_deref(), Some("./schema.json"));
        let yaml = "# yaml-language-server: $schema=https://x.org/s.json\na: 1\n";
        assert_eq!(declared(yaml, &crate::yaml::parse(yaml).unwrap()).as_deref(), Some("https://x.org/s.json"));
        let toml = "#:schema ../s.json\na = 1\n";
        assert_eq!(declared(toml, &crate::toml_doc::parse(toml).unwrap()).as_deref(), Some("../s.json"));
        assert_eq!(declared("a: 1", &crate::yaml::parse("a: 1").unwrap()), None);
    }

    #[test]
    fn sources_from_beside_the_file() {
        assert_eq!(source("https://x.org/s.json#/defs", "/p/a.json"), Source::Url("https://x.org/s.json".into()));
        assert_eq!(source("./schemas/s.json", "/p/conf/a.json"), Source::File("/p/conf/schemas/s.json".into()));
        assert_eq!(source("../s.json", "/p/conf/a.json"), Source::File("/p/s.json".into()));
        assert_eq!(source("s.json", "C:\\p\\a.json"), Source::File("C:/p/s.json".into()));
        assert_eq!(source("/abs/s.json", "/p/a.json"), Source::File("/abs/s.json".into()));
    }

    #[test]
    fn checking_finds_problems_where_they_are() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}, "port": {"type": "integer", "maximum": 65535}},
            "required": ["name"],
            "additionalProperties": false
        });
        let store = Store::default();
        assert_eq!(check_with(&schema, None, &store, &json!({"name": "a", "port": 80})), Checked::Problems(vec![]));
        let Checked::Problems(p) = check_with(&schema, None, &store, &json!({"port": 70000, "x": 1})) else { panic!() };
        let pointers: Vec<&str> = p.iter().map(|p| p.pointer.as_str()).collect();
        assert!(pointers.contains(&"/port") && pointers.contains(&""), "{p:?}");
        assert!(p.iter().any(|p| p.message.contains("70000")), "{p:?}");
    }

    #[test]
    fn schemas_it_refers_to_are_asked_for_then_used() {
        let schema = json!({"$ref": "https://example.org/other.json"});
        let store = Store::default();
        assert_eq!(check_with(&schema, None, &store, &json!(1)), Checked::Needs(vec!["https://example.org/other.json".into()]));
        store.0.lock().unwrap().insert("https://example.org/other.json".into(), json!({"type": "string"}));
        let Checked::Problems(p) = check_with(&schema, None, &store, &json!(1)) else { panic!() };
        assert_eq!(p.len(), 1);
        assert!(matches!(check_with(&json!({"type": 12}), None, &store, &json!(1)), Checked::Broken(_)));
    }
}
