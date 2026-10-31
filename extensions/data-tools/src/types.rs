//! Types made from a JSON sample: TypeScript interfaces, Rust structs
//! (serde), Python dataclasses, Go structs. Every object of a list makes
//! one type: a key some lack is optional, a key that's sometimes null is
//! nullable.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    TypeScript,
    Rust,
    Python,
    Go,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::TypeScript, Lang::Rust, Lang::Python, Lang::Go];

    pub fn name(self) -> &'static str {
        match self {
            Lang::TypeScript => "TypeScript",
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::Go => "Go",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Lang::TypeScript => "typescript",
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::Go => "go",
        }
    }

    /// Its file extension, for the buffer's highlighting.
    pub fn ext(self) -> &'static str {
        match self {
            Lang::TypeScript => "ts",
            Lang::Rust => "rs",
            Lang::Python => "py",
            Lang::Go => "go",
        }
    }

    pub fn from_id(id: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    /// Nothing to go by: an empty list's items, or only nulls.
    Unknown,
    Bool,
    Int,
    Float,
    Str,
    Array(Box<Ty>, bool),
    Object(usize),
    /// Several kinds.
    Mixed,
}

struct Field {
    key: String,
    ty: Ty,
    optional: bool,
    nullable: bool,
}

struct Shape {
    name: String,
    fields: Vec<Field>,
}

#[derive(Default)]
struct Infer {
    shapes: Vec<Shape>,
}

fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        word.push(c);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn pascal(s: &str) -> String {
    let p: String = words(s)
        .iter()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect::<String>()).unwrap_or_default()
        })
        .collect();
    match p.chars().next() {
        None => "Field".into(),
        Some(c) if c.is_ascii_digit() => format!("N{p}"),
        _ => p,
    }
}

fn snake(s: &str) -> String {
    let p = words(s).iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_");
    match p.chars().next() {
        None => "field".into(),
        Some(c) if c.is_ascii_digit() => format!("n{p}"),
        _ => p,
    }
}

/// One of a list's items, by the list's name.
fn singular(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies").filter(|s| !s.is_empty()) {
        format!("{stem}y")
    } else if name.ends_with("ss") {
        format!("{name}Item")
    } else if let Some(stem) = name.strip_suffix('s').filter(|s| !s.is_empty()) {
        stem.to_string()
    } else {
        format!("{name}Item")
    }
}

impl Infer {
    fn name(&self, hint: &str) -> String {
        let base = pascal(hint);
        let mut name = base.clone();
        let mut n = 2;
        while self.shapes.iter().any(|s| s.name == name) {
            name = format!("{base}{n}");
            n += 1;
        }
        name
    }

    /// The type of every value in `values`, and whether some are null.
    fn many(&mut self, values: &[&Value], hint: &str) -> (Ty, bool) {
        let nullable = values.iter().any(|v| v.is_null());
        let present: Vec<&Value> = values.iter().copied().filter(|v| !v.is_null()).collect();
        if present.is_empty() {
            return (Ty::Unknown, nullable);
        }
        let all = |f: fn(&Value) -> bool| present.iter().all(|v| f(v));
        let ty = if all(Value::is_boolean) {
            Ty::Bool
        } else if all(Value::is_number) {
            if present.iter().all(|v| v.is_i64() || v.is_u64()) {
                Ty::Int
            } else {
                Ty::Float
            }
        } else if all(Value::is_string) {
            Ty::Str
        } else if all(Value::is_array) {
            let items: Vec<&Value> = present.iter().flat_map(|v| v.as_array().into_iter().flatten()).collect();
            let (item, item_null) = self.many(&items, &singular(hint));
            Ty::Array(Box::new(item), item_null)
        } else if all(Value::is_object) {
            let objects: Vec<&serde_json::Map<String, Value>> = present.iter().filter_map(|v| v.as_object()).collect();
            let index = self.shapes.len();
            let name = self.name(hint);
            self.shapes.push(Shape { name, fields: Vec::new() });
            let mut keys: Vec<&String> = Vec::new();
            for o in &objects {
                for k in o.keys() {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
            }
            let mut fields = Vec::new();
            for k in keys {
                let have: Vec<&Value> = objects.iter().filter_map(|o| o.get(k)).collect();
                let (ty, nullable) = self.many(&have, k);
                fields.push(Field { key: k.clone(), ty, optional: have.len() < objects.len(), nullable });
            }
            self.shapes[index].fields = fields;
            Ty::Object(index)
        } else {
            Ty::Mixed
        };
        (ty, nullable)
    }
}

/// Types for `sample`, the outermost called after `root`.
pub fn generate(sample: &Value, root: &str, lang: Lang) -> String {
    let mut infer = Infer::default();
    let (ty, nullable) = infer.many(&[sample], root);
    let root_name = pascal(root);
    let mut out = String::new();
    match lang {
        Lang::TypeScript => typescript(&infer, &ty, nullable, &root_name, &mut out),
        Lang::Rust => rust(&infer, &ty, nullable, &root_name, &mut out),
        Lang::Python => python(&infer, &ty, nullable, &root_name, &mut out),
        Lang::Go => go(&infer, &ty, nullable, &root_name, &mut out),
    }
    out.trim_end().to_string() + "\n"
}

/// The outermost isn't an object: a name for it all the same.
fn alias_needed(ty: &Ty, infer: &Infer, root: &str) -> bool {
    !matches!(ty, Ty::Object(i) if infer.shapes[*i].name == root)
}

fn ts_ty(infer: &Infer, ty: &Ty) -> String {
    match ty {
        Ty::Unknown | Ty::Mixed => "unknown".into(),
        Ty::Bool => "boolean".into(),
        Ty::Int | Ty::Float => "number".into(),
        Ty::Str => "string".into(),
        Ty::Array(item, true) => format!("({} | null)[]", ts_ty(infer, item)),
        Ty::Array(item, false) => format!("{}[]", ts_ty(infer, item)),
        Ty::Object(i) => infer.shapes[*i].name.clone(),
    }
}

fn typescript(infer: &Infer, ty: &Ty, nullable: bool, root: &str, out: &mut String) {
    if alias_needed(ty, infer, root) {
        out.push_str(&format!("export type {root} = {}{};\n\n", ts_ty(infer, ty), if nullable { " | null" } else { "" }));
    }
    for shape in &infer.shapes {
        out.push_str(&format!("export interface {} {{\n", shape.name));
        for f in &shape.fields {
            let ident = f.key.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$') && f.key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');
            let key = if ident { f.key.clone() } else { crate::json::quote(&f.key) };
            out.push_str(&format!("  {key}{}: {}{};\n", if f.optional { "?" } else { "" }, ts_ty(infer, &f.ty), if f.nullable { " | null" } else { "" }));
        }
        out.push_str("}\n\n");
    }
}

fn rust_ty(infer: &Infer, ty: &Ty) -> String {
    match ty {
        Ty::Unknown | Ty::Mixed => "serde_json::Value".into(),
        Ty::Bool => "bool".into(),
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Str => "String".into(),
        Ty::Array(item, true) => format!("Vec<Option<{}>>", rust_ty(infer, item)),
        Ty::Array(item, false) => format!("Vec<{}>", rust_ty(infer, item)),
        Ty::Object(i) => infer.shapes[*i].name.clone(),
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "gen", "macro", "override", "priv", "try", "typeof",
    "unsized", "virtual", "yield",
];

fn rust(infer: &Infer, ty: &Ty, nullable: bool, root: &str, out: &mut String) {
    out.push_str("use serde::{Deserialize, Serialize};\n\n");
    if alias_needed(ty, infer, root) {
        let t = rust_ty(infer, ty);
        out.push_str(&format!("pub type {root} = {};\n\n", if nullable { format!("Option<{t}>") } else { t }));
    }
    for shape in &infer.shapes {
        out.push_str(&format!("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\npub struct {} {{\n", shape.name));
        for f in &shape.fields {
            let mut name = snake(&f.key);
            if matches!(name.as_str(), "self" | "super" | "crate" | "_") {
                name.push('_');
            }
            let rename = name != f.key;
            if RUST_KEYWORDS.contains(&name.as_str()) {
                name = format!("r#{name}");
            }
            let mut attrs = Vec::new();
            if rename {
                attrs.push(format!("rename = {}", crate::json::quote(&f.key)));
            }
            if f.optional {
                attrs.push("default, skip_serializing_if = \"Option::is_none\"".into());
            }
            if !attrs.is_empty() {
                out.push_str(&format!("    #[serde({})]\n", attrs.join(", ")));
            }
            let t = rust_ty(infer, &f.ty);
            let t = if f.optional || f.nullable { format!("Option<{t}>") } else { t };
            out.push_str(&format!("    pub {name}: {t},\n"));
        }
        out.push_str("}\n\n");
    }
}

fn py_ty(infer: &Infer, ty: &Ty) -> String {
    match ty {
        Ty::Unknown | Ty::Mixed => "Any".into(),
        Ty::Bool => "bool".into(),
        Ty::Int => "int".into(),
        Ty::Float => "float".into(),
        Ty::Str => "str".into(),
        Ty::Array(item, true) => format!("list[Optional[{}]]", py_ty(infer, item)),
        Ty::Array(item, false) => format!("list[{}]", py_ty(infer, item)),
        Ty::Object(i) => infer.shapes[*i].name.clone(),
    }
}

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
];

fn python(infer: &Infer, ty: &Ty, nullable: bool, root: &str, out: &mut String) {
    out.push_str("from __future__ import annotations\n\nfrom dataclasses import dataclass\nfrom typing import Any, Optional\n\n\n");
    if alias_needed(ty, infer, root) {
        let t = py_ty(infer, ty);
        out.push_str(&format!("{root} = {}\n\n\n", if nullable { format!("Optional[{t}]") } else { t }));
    }
    for shape in &infer.shapes {
        out.push_str(&format!("@dataclass\nclass {}:\n", shape.name));
        if shape.fields.is_empty() {
            out.push_str("    pass\n");
        }
        // Those with a default after those without.
        let (required, optional): (Vec<&Field>, Vec<&Field>) = shape.fields.iter().partition(|f| !f.optional);
        for f in required.into_iter().chain(optional) {
            let mut name = snake(&f.key);
            if PY_KEYWORDS.contains(&name.as_str()) {
                name.push('_');
            }
            let t = py_ty(infer, &f.ty);
            let t = if f.optional || f.nullable { format!("Optional[{t}]") } else { t };
            let default = if f.optional { " = None" } else { "" };
            let from = if name != f.key { format!("  # {}", crate::json::quote(&f.key)) } else { String::new() };
            out.push_str(&format!("    {name}: {t}{default}{from}\n"));
        }
        out.push_str("\n\n");
    }
}

fn go_ty(infer: &Infer, ty: &Ty) -> String {
    match ty {
        Ty::Unknown | Ty::Mixed => "any".into(),
        Ty::Bool => "bool".into(),
        Ty::Int => "int64".into(),
        Ty::Float => "float64".into(),
        Ty::Str => "string".into(),
        Ty::Array(item, true) if !matches!(**item, Ty::Unknown | Ty::Mixed) => format!("[]*{}", go_ty(infer, item)),
        Ty::Array(item, _) => format!("[]{}", go_ty(infer, item)),
        Ty::Object(i) => infer.shapes[*i].name.clone(),
    }
}

fn go(infer: &Infer, ty: &Ty, _nullable: bool, root: &str, out: &mut String) {
    if alias_needed(ty, infer, root) {
        out.push_str(&format!("type {root} {}\n\n", go_ty(infer, ty)));
    }
    for shape in &infer.shapes {
        out.push_str(&format!("type {} struct {{\n", shape.name));
        let rows: Vec<(String, String, String)> = shape
            .fields
            .iter()
            .map(|f| {
                let t = go_ty(infer, &f.ty);
                // A pointer for what may be missing or null, but a slice or
                // `any` is nil already.
                let pointer = (f.optional || f.nullable) && !t.starts_with("[]") && t != "any";
                let tag = format!("`json:\"{}{}\"`", f.key.replace('"', "\\\""), if f.optional { ",omitempty" } else { "" });
                (pascal(&f.key), if pointer { format!("*{t}") } else { t }, tag)
            })
            .collect();
        let name_w = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
        let ty_w = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
        for (name, t, tag) in rows {
            out.push_str(&format!("\t{name:name_w$} {t:ty_w$} {tag}\n"));
        }
        out.push_str("}\n\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "name": "fenix",
            "version": 2,
            "ratio": 1.5,
            "my-key": null,
            "tags": ["a", "b"],
            "servers": [
                {"host": "a", "port": 80},
                {"host": "b", "port": 8080, "tls": true}
            ],
            "type": "editor",
            "empty": []
        })
    }

    #[test]
    fn typescript_interfaces() {
        assert_eq!(
            generate(&sample(), "config", Lang::TypeScript),
            "export interface Config {\n  name: string;\n  version: number;\n  ratio: number;\n  \"my-key\": unknown | null;\n  tags: string[];\n  servers: Server[];\n  type: string;\n  empty: unknown[];\n}\n\nexport interface Server {\n  host: string;\n  port: number;\n  tls?: boolean;\n}\n"
        );
    }

    #[test]
    fn rust_structs() {
        let out = generate(&sample(), "config", Lang::Rust);
        assert!(out.starts_with("use serde::{Deserialize, Serialize};\n\n#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\npub struct Config {\n    pub name: String,\n    pub version: i64,\n    pub ratio: f64,\n    #[serde(rename = \"my-key\")]\n    pub my_key: Option<serde_json::Value>,\n    pub tags: Vec<String>,\n    pub servers: Vec<Server>,\n    pub r#type: String,\n    pub empty: Vec<serde_json::Value>,\n}\n"), "{out}");
        assert!(
            out.contains("pub struct Server {\n    pub host: String,\n    pub port: i64,\n    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n    pub tls: Option<bool>,\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn python_dataclasses() {
        let out = generate(&sample(), "config", Lang::Python);
        assert!(out.contains("@dataclass\nclass Config:\n    name: str\n    version: int\n    ratio: float\n    my_key: Optional[Any]  # \"my-key\"\n    tags: list[str]\n    servers: list[Server]\n    type: str\n    empty: list[Any]\n"), "{out}");
        assert!(out.contains("class Server:\n    host: str\n    port: int\n    tls: Optional[bool] = None\n"), "{out}");
    }

    #[test]
    fn go_structs() {
        let out = generate(&sample(), "config", Lang::Go);
        assert!(out.contains("type Server struct {\n\tHost string `json:\"host\"`\n\tPort int64  `json:\"port\"`\n\tTls  *bool  `json:\"tls,omitempty\"`\n}\n"), "{out}");
        assert!(out.contains("\tMyKey   any      `json:\"my-key\"`\n"), "{out}");
    }

    #[test]
    fn a_list_at_the_top_and_names_that_clash() {
        let list = json!([{"id": 1, "child": {"id": 2}}, {"id": 3, "child": {"id": 4, "x": null}}]);
        assert_eq!(
            generate(&list, "items", Lang::TypeScript),
            "export type Items = Item[];\n\nexport interface Item {\n  id: number;\n  child: Child;\n}\n\nexport interface Child {\n  id: number;\n  x?: unknown | null;\n}\n"
        );
        let clash = json!({"a": {"data": {"v": 1}}, "data": {"w": 2}});
        let out = generate(&clash, "data", Lang::Rust);
        assert!(out.contains("pub struct Data {") && out.contains("pub struct Data2 {") && out.contains("pub struct Data3 {"), "{out}");
        assert_eq!(singular("categories"), "category");
        assert_eq!(singular("class"), "classItem");
        assert_eq!(pascal("http_server-config"), "HttpServerConfig");
        assert_eq!(snake("myKeyName"), "my_key_name");
        assert_eq!(pascal("2fa"), "N2fa");
    }
}
