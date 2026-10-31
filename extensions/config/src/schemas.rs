//! Schemas for YAML and JSON files, by their names. yaml-language-server
//! reads SchemaStore's catalog itself (`yaml.schemaStore.enable`); the
//! JSON server only takes the schemas it's given (`json.schemas`), so
//! the pack fetches the catalog, keeps it a week in its data folder, and
//! gives the server each schema whose files are JSON.
//!
//! JSON with comments: `*.jsonc`, `tsconfig.json`, VS Code's settings and
//! the like have a schema of the pack's own that allows them ([`JSONC`]),
//! beside SchemaStore's.

use fenix_lang_kit::serde_json::{self, json, Value};

/// SchemaStore's catalog.
pub const CATALOG: &str = "https://www.schemastore.org/api/json/catalog.json";

/// Where the catalog is kept, and for how long, in seconds.
pub const CACHE: &str = "/data/schemastore.json";
pub const WEEK: u64 = 7 * 24 * 60 * 60;

/// Files that are JSON with comments and trailing commas, whose schema in
/// SchemaStore (if any) doesn't say so: TypeScript's reads comments, its
/// schema doesn't allow them. A file both match is held to both, with
/// comments allowed.
pub const JSONC: &[&str] = &[
    "*.jsonc", "tsconfig*.json", "jsconfig*.json", ".eslintrc.json", "deno.json", "turbo.json", ".babelrc", ".jshintrc", ".swcrc",
    "**/.vscode/*.json", "*.code-workspace", "devcontainer.json", ".devcontainer.json",
];

/// One schema the JSON server is given: the files it's for, and where
/// it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub files: Vec<String>,
    pub url: String,
}

/// Whether a `fileMatch` pattern names files the JSON server sees, not
/// YAML's or TOML's.
fn is_json(pattern: &str) -> bool {
    let lower = pattern.to_ascii_lowercase();
    !(lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".toml"))
}

/// The catalog's schemas for JSON files.
pub fn parse_catalog(text: &str) -> Result<Vec<Schema>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("SchemaStore's catalog: {e}"))?;
    let schemas = v.get("schemas").and_then(Value::as_array).ok_or("SchemaStore's catalog has no schemas")?;
    Ok(schemas
        .iter()
        .filter_map(|s| {
            let url = s.get("url")?.as_str()?.to_string();
            let files: Vec<String> = s.get("fileMatch")?.as_array()?.iter().filter_map(Value::as_str).filter(|p| is_json(p)).map(str::to_string).collect();
            (!files.is_empty()).then_some(Schema { files, url })
        })
        .collect())
}

/// The catalog as it's kept: when it was fetched, and its JSON schemas.
pub fn cache_text(fetched: u64, schemas: &[Schema]) -> String {
    let list: Vec<Value> = schemas.iter().map(|s| json!({ "fileMatch": s.files, "url": s.url })).collect();
    json!({ "fetched": fetched, "schemas": list }).to_string()
}

/// What was kept: when it was fetched, and its schemas.
pub fn read_cache(text: &str) -> Option<(u64, Vec<Schema>)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let fetched = v.get("fetched")?.as_u64()?;
    let schemas = v.get("schemas")?.as_array()?.iter().filter_map(|s| {
        let files = s.get("fileMatch")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect();
        Some(Schema { files, url: s.get("url")?.as_str()?.to_string() })
    });
    Some((fetched, schemas.collect()))
}

/// Whether a catalog fetched at `fetched` should be fetched again at
/// `now` (both in seconds).
pub fn stale(fetched: u64, now: u64) -> bool {
    now.saturating_sub(fetched) >= WEEK
}

/// The JSON server's settings: `schemas`, and the pack's own that lets
/// JSON with comments have them.
pub fn json_settings(schemas: &[Schema]) -> Value {
    let mut list: Vec<Value> = schemas.iter().map(|s| json!({ "fileMatch": s.files, "url": s.url })).collect();
    list.push(json!({ "fileMatch": JSONC, "schema": { "allowComments": true, "allowTrailingCommas": true } }));
    json!({
        "json": { "validate": { "enable": true }, "format": { "enable": true }, "schemaDownload": { "enable": true }, "schemas": list },
        "http": { "proxyStrictSSL": true },
    })
}

/// yaml-language-server's settings: SchemaStore's catalog read by the
/// server itself, or not.
pub fn yaml_settings(schemastore: bool) -> Value {
    json!({
        "yaml": {
            "validate": true,
            "hover": true,
            "completion": true,
            "format": { "enable": true },
            "schemaStore": { "enable": schemastore, "url": CATALOG },
            "keyOrdering": false,
        },
        "redhat": { "telemetry": { "enabled": false } },
    })
}

/// bash-language-server's settings: ShellCheck and shfmt where they were
/// found, which `PATH` may not have.
pub fn bash_settings(shellcheck: Option<&str>, shfmt: Option<&str>) -> Value {
    let mut ide = json!({ "enableSourceErrorDiagnostics": false });
    if let Some(path) = shellcheck {
        ide["shellcheckPath"] = json!(path);
    }
    if let Some(path) = shfmt {
        ide["shfmt"] = json!({ "path": path });
    }
    json!({ "bashIde": ide })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG_SAMPLE: &str = r#"{
        "$schema": "https://json.schemastore.org/schema-catalog.json",
        "version": 1,
        "schemas": [
            {"name": "tsconfig.json", "fileMatch": ["tsconfig*.json"], "url": "https://www.schemastore.org/tsconfig.json"},
            {"name": "GitHub Workflow", "fileMatch": ["**/.github/workflows/*.yml", "**/.github/workflows/*.yaml"], "url": "https://www.schemastore.org/github-workflow.json"},
            {"name": "Cargo", "fileMatch": ["Cargo.toml"], "url": "https://www.schemastore.org/cargo.json"},
            {"name": "Babel", "fileMatch": [".babelrc", "babel.config.json", ".babelrc.yml"], "url": "https://www.schemastore.org/babelrc.json"},
            {"name": "No files", "url": "https://www.schemastore.org/nothing.json"}
        ]
    }"#;

    #[test]
    fn the_catalog_gives_the_json_server_its_json_schemas() {
        let schemas = parse_catalog(CATALOG_SAMPLE).unwrap();
        assert_eq!(
            schemas,
            [
                Schema { files: vec!["tsconfig*.json".into()], url: "https://www.schemastore.org/tsconfig.json".into() },
                Schema { files: vec![".babelrc".into(), "babel.config.json".into()], url: "https://www.schemastore.org/babelrc.json".into() },
            ],
            "YAML's and TOML's are their servers'"
        );
        assert!(parse_catalog("{}").is_err() && parse_catalog("not json").is_err());

        let kept = cache_text(1_000, &schemas);
        assert_eq!(read_cache(&kept), Some((1_000, schemas.clone())));
        assert_eq!(read_cache("{}"), None);
        assert!(!stale(1_000, 1_000 + WEEK - 1) && stale(1_000, 1_000 + WEEK));

        let settings = json_settings(&schemas);
        let list = settings["json"]["schemas"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], json!({"fileMatch": ["tsconfig*.json"], "url": "https://www.schemastore.org/tsconfig.json"}));
        assert_eq!(list[2]["schema"]["allowComments"], true, "JSON with comments, last");
        assert!(list[2]["fileMatch"].as_array().unwrap().contains(&json!("*.jsonc")));
    }

    #[test]
    fn the_servers_settings() {
        let yaml = yaml_settings(true);
        assert_eq!(yaml["yaml"]["schemaStore"], json!({"enable": true, "url": CATALOG}));
        assert_eq!(yaml["redhat"]["telemetry"]["enabled"], false);
        assert_eq!(yaml_settings(false)["yaml"]["schemaStore"]["enable"], false);
        assert_eq!(bash_settings(Some("/t/shellcheck"), Some("/t/shfmt")), json!({"bashIde": {"enableSourceErrorDiagnostics": false, "shellcheckPath": "/t/shellcheck", "shfmt": {"path": "/t/shfmt"}}}));
        assert_eq!(bash_settings(None, None), json!({"bashIde": {"enableSourceErrorDiagnostics": false}}));
        assert_eq!(json_settings(&[])["json"]["schemas"].as_array().unwrap().len(), 1);
    }
}
