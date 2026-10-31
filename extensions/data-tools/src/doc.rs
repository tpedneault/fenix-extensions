//! What each command does to a text, whichever of JSON, YAML and TOML
//! it's in: format, minify, sort, convert, query, and JSON as a string.

use crate::json::{self, Style};
use crate::node::Node;
use crate::{toml_doc, yaml};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
}

impl Format {
    /// By a file's extension (`json`, `yml`), or Fenix's name for its
    /// language.
    pub fn from_language(language: &str) -> Option<Format> {
        match language.to_ascii_lowercase().as_str() {
            "json" | "jsonc" | "json5" | "geojson" | "jsonld" | "har" | "ipynb" | "webmanifest" | "babelrc" | "eslintrc" | "prettierrc" => Some(Format::Json),
            "yaml" | "yml" => Some(Format::Yaml),
            "toml" => Some(Format::Toml),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "JSON",
            Format::Yaml => "YAML",
            Format::Toml => "TOML",
        }
    }

    /// Its file extension, and its language for highlighting.
    pub fn ext(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
        }
    }

    pub fn from_name(name: &str) -> Option<Format> {
        [Format::Json, Format::Yaml, Format::Toml].into_iter().find(|f| f.ext() == name || f.name().eq_ignore_ascii_case(name))
    }
}

/// What a text is in: its file's language, else what it looks like -- a
/// brace or a bracket is JSON, `key = value` lines TOML, the rest YAML.
pub fn detect(language: &str, text: &str) -> Format {
    if let Some(f) = Format::from_language(language) {
        return f;
    }
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with(['{', '[', '"']) && json::parse(text).is_ok() {
        return Format::Json;
    }
    if t.starts_with(['{', '[']) && yaml::parse(text).is_err() {
        return Format::Json;
    }
    let toml_like = t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).all(|l| l.starts_with('[') || l.contains(" = ") || l.contains('='));
    if toml_like && toml_doc::parse(text).is_ok() {
        return Format::Toml;
    }
    Format::Yaml
}

/// The indent formatting uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Spaces(usize),
    Tab,
}

impl Indent {
    pub fn unit(self) -> String {
        match self {
            Indent::Spaces(n) => " ".repeat(n.clamp(1, 16)),
            Indent::Tab => "\t".into(),
        }
    }

    /// YAML's: spaces, always.
    fn yaml(self) -> String {
        match self {
            Indent::Spaces(n) => " ".repeat(n.clamp(1, 16)),
            Indent::Tab => "  ".into(),
        }
    }
}

/// A text read: its document, and whether it had comments that writing
/// it again would drop.
pub struct Parsed {
    pub node: Node,
    pub comments: bool,
}

pub fn parse(format: Format, text: &str) -> Result<Parsed, String> {
    match format {
        Format::Json => json::parse(text).map(|r| Parsed { node: r.node, comments: r.comments }).map_err(|e| format!("JSON: {e}")),
        Format::Yaml => yaml::parse(text).map(|node| Parsed { node, comments: yaml::has_comments(text) }).map_err(|e| format!("YAML: {e}")),
        Format::Toml => toml_doc::parse(text).map(|node| Parsed { node, comments: text.contains('#') }).map_err(|e| format!("TOML: {e}")),
    }
}

/// `node` written in `format`.
pub fn write(format: Format, node: &Node, indent: Indent) -> Result<String, String> {
    match format {
        Format::Json => json::write(node, &Style::Pretty(indent.unit())),
        Format::Yaml => Ok(yaml::write(node, &indent.yaml())),
        Format::Toml => toml_doc::write(node).map(|t| t.trim_end().to_string()),
    }
}

/// A transform's text, and what to say about it.
#[derive(Debug, PartialEq)]
pub struct Done {
    pub text: String,
    pub note: Option<String>,
}

fn done(text: String, lost_comments: bool) -> Done {
    Done { text, note: lost_comments.then(|| "its comments are gone: written again, they don't survive".to_string()) }
}

pub fn format(format: Format, text: &str, indent: Indent) -> Result<Done, String> {
    match format {
        // TOML keeps its comments, formatted in place.
        Format::Toml => toml_doc::format(text).map(|t| done(t.trim_end().to_string(), false)).map_err(|e| format!("TOML: {e}")),
        _ => {
            let p = parse(format, text)?;
            Ok(done(write(format, &p.node, indent)?, p.comments))
        }
    }
}

pub fn minify(format: Format, text: &str) -> Result<Done, String> {
    let p = parse(format, text)?;
    match format {
        Format::Json => Ok(done(json::write(&p.node, &Style::Minified)?, p.comments)),
        Format::Yaml => Ok(done(yaml::flow(&p.node), p.comments)),
        Format::Toml => Err("TOML is a line a key: it has no one-line form (convert it to JSON first)".into()),
    }
}

/// Keys sorted: in every object, or the top level's only. JSON on one
/// line stays on one line.
pub fn sort(format: Format, text: &str, recursive: bool, indent: Indent) -> Result<Done, String> {
    if format == Format::Toml {
        return toml_doc::sort(text, recursive).map(|t| done(t.trim_end().to_string(), false)).map_err(|e| format!("TOML: {e}"));
    }
    let mut p = parse(format, text)?;
    p.node.sort_keys(recursive);
    let one_line = format == Format::Json && !text.trim().contains('\n');
    let out = if one_line { json::write(&p.node, &Style::Minified)? } else { write(format, &p.node, indent)? };
    Ok(done(out, p.comments))
}

pub fn convert(from: Format, text: &str, to: Format, indent: Indent) -> Result<Done, String> {
    let p = parse(from, text)?;
    Ok(done(write(to, &p.node, indent)?, p.comments))
}

/// A JSONPath query's matches, as JSON: the one value when there's one,
/// else a list of them.
pub fn query(format: Format, text: &str, path: &str, indent: Indent) -> Result<(String, usize), String> {
    let p = parse(format, text)?;
    let value = p.node.to_json();
    let path = serde_json_path::JsonPath::parse(path).map_err(|e| format!("JSONPath: {e}"))?;
    let found = path.query(&value).all();
    let n = found.len();
    let result = match found.as_slice() {
        [one] => (*one).clone(),
        many => serde_json::Value::Array(many.iter().map(|v| (*v).clone()).collect()),
    };
    Ok((json::write(&Node::from_json(&result), &Style::Pretty(indent.unit()))?, n))
}

/// The document as a JSON string literal: `"{\"a\":1}"`.
pub fn to_string_literal(format: Format, text: &str) -> Result<Done, String> {
    let p = parse(format, text)?;
    Ok(done(json::quote(&json::write(&p.node, &Style::Minified)?), p.comments))
}

/// A string literal that holds JSON (or YAML, or TOML), as that
/// document, formatted.
pub fn from_string_literal(text: &str, indent: Indent) -> Result<Done, String> {
    let inner = json::unquote(text).map_err(|e| format!("the string: {e}"))?;
    let format = detect("", &inner);
    let p = parse(format, &inner)?;
    Ok(done(write(format, &p.node, indent)?, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: Indent = Indent::Spaces(2);

    #[test]
    fn formats_by_language_or_by_look() {
        assert_eq!(detect("json", "a: 1"), Format::Json);
        assert_eq!(detect("yml", "{}"), Format::Yaml);
        assert_eq!(detect("md", "{\"a\": 1}"), Format::Json);
        assert_eq!(detect("md", "[1, 2]"), Format::Json);
        assert_eq!(detect("", "a = 1\n[b]\nc = \"x\"\n"), Format::Toml);
        assert_eq!(detect("", "a: 1\nb:\n  - c\n"), Format::Yaml);
        assert_eq!(detect("", "[a, b]"), Format::Yaml, "a YAML flow list");
        assert_eq!(Format::from_name("toml"), Some(Format::Toml));
    }

    #[test]
    fn format_and_minify_each() {
        assert_eq!(format(Format::Json, "{\"a\":[1,2]}", Indent::Spaces(4)).unwrap().text, "{\n    \"a\": [\n        1,\n        2\n    ]\n}");
        assert_eq!(format(Format::Json, "{\"a\":1}", Indent::Tab).unwrap().text, "{\n\t\"a\": 1\n}");
        assert_eq!(format(Format::Yaml, "a:   1\nb: [x,  z, y]\n", TWO).unwrap().text, "a: 1\nb:\n  - x\n  - z\n  - \"y\"", "y quoted: YAML 1.1 read it as true");
        assert_eq!(format(Format::Yaml, "a:\n    - 1\n", Indent::Tab).unwrap().text, "a:\n  - 1", "YAML indents with spaces");
        assert_eq!(format(Format::Toml, "a=1 # x\n", TWO).unwrap(), Done { text: "a = 1 # x".into(), note: None });
        assert_eq!(minify(Format::Json, "{\n  \"a\": [1, 2]\n}").unwrap().text, "{\"a\":[1,2]}");
        assert_eq!(minify(Format::Yaml, "a: 1\nb: [x, z]\n").unwrap().text, "{a: 1, b: [x, z]}");
        assert!(minify(Format::Toml, "a = 1").is_err());
        assert!(format(Format::Json, "{", TWO).unwrap_err().starts_with("JSON: line 1, column 2"));
    }

    #[test]
    fn dropped_comments_are_said() {
        assert!(format(Format::Json, "{\"a\": 1 // one\n}", TWO).unwrap().note.is_some());
        assert!(format(Format::Yaml, "a: 1 # one\n", TWO).unwrap().note.is_some());
        assert!(format(Format::Yaml, "a: 1\n", TWO).unwrap().note.is_none());
    }

    #[test]
    fn sort_each() {
        assert_eq!(sort(Format::Json, r#"{"b":{"d":1,"c":2},"a":1}"#, true, TWO).unwrap().text, r#"{"a":1,"b":{"c":2,"d":1}}"#, "one line stays one line");
        assert_eq!(sort(Format::Json, "{\n\"b\":{\"d\":1,\"c\":2},\"a\":1}", false, TWO).unwrap().text, "{\n  \"a\": 1,\n  \"b\": {\n    \"d\": 1,\n    \"c\": 2\n  }\n}");
        assert_eq!(sort(Format::Yaml, "b: 1\na:\n  y: 1\n  x: 2\n", true, TWO).unwrap().text, "a:\n  x: 2\n  y: 1\nb: 1");
        assert_eq!(sort(Format::Toml, "b = 1\na = 2\n", true, TWO).unwrap().text, "a = 2\nb = 1");
    }

    #[test]
    fn convert_round_trips() {
        let json = r#"{"name": "fenix", "version": 2, "tags": ["editor", "rust"], "deps": {"serde": {"version": "1", "features": ["derive"]}}, "bins": [{"name": "a"}, {"name": "b"}]}"#;
        let yaml = convert(Format::Json, json, Format::Yaml, TWO).unwrap().text;
        assert_eq!(yaml, "name: fenix\nversion: 2\ntags:\n  - editor\n  - rust\ndeps:\n  serde:\n    version: \"1\"\n    features:\n      - derive\nbins:\n  - name: a\n  - name: b");
        let toml = convert(Format::Yaml, &yaml, Format::Toml, TWO).unwrap().text;
        assert_eq!(toml, "name = \"fenix\"\nversion = 2\ntags = [\"editor\", \"rust\"]\n\n[deps.serde]\nversion = \"1\"\nfeatures = [\"derive\"]\n\n[[bins]]\nname = \"a\"\n\n[[bins]]\nname = \"b\"");
        let back = convert(Format::Toml, &toml, Format::Json, TWO).unwrap().text;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&back).unwrap(), serde_json::from_str::<serde_json::Value>(json).unwrap());
        assert!(back.starts_with("{\n  \"name\": \"fenix\",\n  \"version\": 2,"), "in its order: {back}");
        assert_eq!(convert(Format::Json, r#"{"a": null}"#, Format::Toml, TWO).unwrap_err(), "TOML has no null, and .a is null");
        assert_eq!(convert(Format::Yaml, "a: .inf", Format::Json, TWO).unwrap_err(), "inf isn't a number JSON has");
        assert_eq!(convert(Format::Yaml, "a: .inf", Format::Toml, TWO).unwrap().text, "a = inf");
    }

    #[test]
    fn queries() {
        let text = r#"{"servers": [{"host": "a", "port": 80}, {"host": "b", "port": 8080}]}"#;
        assert_eq!(query(Format::Json, text, "$.servers[*].port", TWO).unwrap(), ("[\n  80,\n  8080\n]".to_string(), 2));
        assert_eq!(query(Format::Json, text, "$.servers[1].host", TWO).unwrap(), ("\"b\"".to_string(), 1));
        assert_eq!(query(Format::Json, text, "$..nothing", TWO).unwrap(), ("[]".to_string(), 0));
        assert_eq!(query(Format::Yaml, "a:\n  - x: 1\n", "$.a[0].x", TWO).unwrap().0, "1");
        assert!(query(Format::Json, text, "$.servers[", TWO).unwrap_err().starts_with("JSONPath:"));
    }

    #[test]
    fn json_as_a_string_and_back() {
        let lit = to_string_literal(Format::Json, "{\n  \"a\": \"x\\\"y\",\n  \"b\": [1]\n}").unwrap().text;
        assert_eq!(lit, r#""{\"a\":\"x\\\"y\",\"b\":[1]}""#);
        assert_eq!(from_string_literal(&lit, TWO).unwrap().text, "{\n  \"a\": \"x\\\"y\",\n  \"b\": [\n    1\n  ]\n}");
        assert_eq!(to_string_literal(Format::Yaml, "a: 1").unwrap().text, r#""{\"a\":1}""#);
        assert!(from_string_literal(r#""{\"a\": ""#, TWO).is_err());
    }
}
