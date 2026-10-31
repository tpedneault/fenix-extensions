//! OpenAPI 3 specs (JSON or YAML): a request for each operation, a file
//! for each tag. Its first server is `{{baseUrl}}`; path parameters are
//! variables (`/pets/{{petId}}`); required query parameters and headers
//! are written with their example; a JSON body is its example, or one
//! made from its schema. Its security schemes say the auth header.

use serde_json::{Map, Value};

use super::{slug, Collection, Draft};

const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// `#/components/schemas/Pet`, followed.
fn deref<'a>(doc: &'a Value, v: &'a Value) -> &'a Value {
    let mut v = v;
    for _ in 0..16 {
        match v.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix('#')) {
            Some(pointer) => match doc.pointer(pointer) {
                Some(target) => v = target,
                None => return v,
            },
            None => return v,
        }
    }
    v
}

/// An example for `schema`: its own, else one made from its type.
pub fn example(doc: &Value, schema: &Value, depth: usize) -> Value {
    let schema = deref(doc, schema);
    if let Some(e) = schema.get("example").or(schema.get("default")) {
        return e.clone();
    }
    if let Some(Value::Array(values)) = schema.get("enum") {
        if let Some(first) = values.first() {
            return first.clone();
        }
    }
    if depth > 6 {
        return Value::Null;
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(Value::Array(all)) = schema.get(key) {
            if key == "allOf" {
                let mut merged = Map::new();
                for s in all {
                    if let Value::Object(o) = example(doc, s, depth + 1) {
                        merged.extend(o);
                    }
                }
                return Value::Object(merged);
            }
            return all.first().map(|s| example(doc, s, depth + 1)).unwrap_or(Value::Null);
        }
    }
    let ty = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("string"),
        _ if schema.get("properties").is_some() => "object",
        _ => "",
    };
    match ty {
        "object" => {
            let mut m = Map::new();
            if let Some(Value::Object(props)) = schema.get("properties") {
                for (k, p) in props {
                    if !deref(doc, p).get("readOnly").and_then(Value::as_bool).unwrap_or(false) {
                        m.insert(k.clone(), example(doc, p, depth + 1));
                    }
                }
            }
            Value::Object(m)
        }
        "array" => Value::Array(schema.get("items").map(|i| vec![example(doc, i, depth + 1)]).unwrap_or_default()),
        "integer" => Value::from(0),
        "number" => Value::from(0.0),
        "boolean" => Value::Bool(false),
        "string" => Value::String(match schema.get("format").and_then(Value::as_str) {
            Some("date-time") => "2024-01-01T00:00:00Z".into(),
            Some("date") => "2024-01-01".into(),
            Some("email") => "ada@example.com".into(),
            Some("uuid") => "{{$uuid}}".into(),
            Some("uri" | "url") => "https://example.com".into(),
            _ => "string".into(),
        }),
        _ => Value::Null,
    }
}

fn param_example(doc: &Value, p: &Value) -> String {
    if let Some(e) = p.get("example") {
        return crate::env::as_text(e);
    }
    match p.get("schema").map(|s| example(doc, s, 0)) {
        Some(Value::Null) | None => String::new(),
        Some(v) => crate::env::as_text(&v),
    }
}

/// The header (or query) a security scheme writes.
fn security(doc: &Value, op: &Value, d: &mut Draft) {
    let reqs = op.get("security").or(doc.get("security")).and_then(Value::as_array);
    let Some(first) = reqs.and_then(|r| r.first()).and_then(Value::as_object) else { return };
    for name in first.keys() {
        let Some(scheme) = doc.pointer(&format!("/components/securitySchemes/{name}")) else { continue };
        let scheme = deref(doc, scheme);
        let s = |k: &str| scheme.get(k).and_then(Value::as_str).unwrap_or("");
        match (s("type"), s("scheme").to_ascii_lowercase().as_str()) {
            ("http", "bearer") => d.headers.push(("Authorization".into(), "Bearer {{token}}".into())),
            ("http", "basic") => d.headers.push(("Authorization".into(), "Basic {{username}}:{{password}}".into())),
            ("apiKey", _) => match s("in") {
                "header" => d.headers.push((s("name").to_string(), "{{apiKey}}".into())),
                "query" => {
                    let sep = if d.url.contains('?') { '&' } else { '?' };
                    d.url = format!("{}{sep}{}={{{{apiKey}}}}", d.url, s("name"));
                }
                _ => {}
            },
            ("oauth2" | "openIdConnect", _) => d.headers.push(("Authorization".into(), format!("Bearer {{{{$auth.token(\"{name}\")}}}}"))),
            _ => {}
        }
    }
}

pub fn import(doc: &Value) -> Result<Collection, String> {
    if let Some(v) = doc.get("swagger").and_then(Value::as_str) {
        return Err(format!("Swagger {v} (OpenAPI 2): convert it to OpenAPI 3 first"));
    }
    let title = doc.pointer("/info/title").and_then(Value::as_str).unwrap_or("API").to_string();
    let mut c = Collection { name: title, ..Collection::default() };
    let paths = doc.get("paths").and_then(Value::as_object).ok_or("an OpenAPI spec has paths")?;
    let mut base = doc.pointer("/servers/0/url").and_then(Value::as_str).unwrap_or("http://localhost").to_string();
    // A server's own variables, at their defaults.
    if let Some(Value::Object(vars)) = doc.pointer("/servers/0/variables") {
        for (k, v) in vars {
            base = base.replace(&format!("{{{k}}}"), v.get("default").and_then(Value::as_str).unwrap_or(""));
        }
    }
    let mut shared = Map::new();
    shared.insert("baseUrl".into(), Value::String(base.trim_end_matches('/').to_string()));
    c.envs.insert(crate::env::SHARED.into(), Value::Object(shared));
    for (path, item) in paths {
        let item = deref(doc, item);
        let common: Vec<Value> = item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        for method in METHODS {
            let Some(op) = item.get(*method) else { continue };
            let name = op.get("operationId").or(op.get("summary")).and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("{} {path}", method.to_ascii_uppercase()));
            let url_path: String = {
                let mut out = String::new();
                let mut rest = path.as_str();
                while let Some(i) = rest.find('{') {
                    out.push_str(&rest[..i]);
                    match rest[i..].find('}') {
                        Some(j) => {
                            out.push_str(&format!("{{{{{}}}}}", &rest[i + 1..i + j]));
                            rest = &rest[i + j + 1..];
                        }
                        None => {
                            out.push_str(&rest[i..]);
                            rest = "";
                        }
                    }
                }
                out + rest
            };
            let mut d = Draft { name, method: method.to_ascii_uppercase(), url: format!("{{{{baseUrl}}}}{url_path}"), ..Draft::default() };
            let params: Vec<&Value> = op.get("parameters").and_then(Value::as_array).into_iter().flatten().chain(common.iter()).map(|p| deref(doc, p)).collect();
            let mut query = Vec::new();
            let mut seen: Vec<(String, String)> = Vec::new();
            for p in params {
                let (n, place) = (p.get("name").and_then(Value::as_str).unwrap_or(""), p.get("in").and_then(Value::as_str).unwrap_or(""));
                if seen.contains(&(n.to_string(), place.to_string())) {
                    continue;
                }
                seen.push((n.to_string(), place.to_string()));
                let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
                match place {
                    "query" if required || p.get("example").is_some() => query.push((n.to_string(), param_example(doc, p))),
                    "header" if required => d.headers.push((n.to_string(), param_example(doc, p))),
                    _ => {}
                }
            }
            d.url = crate::url::join(&d.url, &query);
            security(doc, op, &mut d);
            if let Some(Value::Object(content)) = op.get("requestBody").map(|b| deref(doc, b)).and_then(|b| b.get("content")) {
                let (ty, media) = content.iter().find(|(t, _)| t.contains("json")).or_else(|| content.iter().next()).map(|(t, m)| (t.clone(), m.clone())).unwrap_or_default();
                d.headers.push(("Content-Type".into(), ty.clone()));
                let ex = media.get("example").cloned().or_else(|| media.pointer("/examples").and_then(Value::as_object).and_then(|e| e.values().next()).map(|e| deref(doc, e)).and_then(|e| e.get("value")).cloned()).or_else(|| media.get("schema").map(|s| example(doc, s, 0)));
                d.body = ex.map(|e| match &e {
                    Value::String(s) => s.clone(),
                    _ if ty.contains("form-urlencoded") => e.as_object().map(|o| o.iter().map(|(k, v)| format!("{k}={}", crate::env::as_text(v))).collect::<Vec<_>>().join("\n&")).unwrap_or_default(),
                    _ => serde_json::to_string_pretty(&e).unwrap_or_default(),
                });
            }
            d.note = op.get("summary").and_then(Value::as_str).filter(|s| op.get("operationId").is_some() && !s.is_empty()).map(String::from);
            let tag = op.pointer("/tags/0").and_then(Value::as_str).map(slug).unwrap_or_else(|| slug(&c.name));
            let file = format!("{tag}.http");
            match c.files.iter_mut().find(|(f, _)| *f == file) {
                Some((_, ds)) => ds.push(d),
                None => c.files.push((file, vec![d])),
            }
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r##"
openapi: 3.0.3
info: { title: Pet Store, version: "1.0" }
servers:
  - url: "https://{env}.petstore.test/v1/"
    variables: { env: { default: api } }
security: [ { bearerAuth: [] } ]
paths:
  /pets:
    get:
      operationId: listPets
      summary: List all pets
      tags: [pets]
      parameters:
        - { name: limit, in: query, required: true, schema: { type: integer, example: 20 } }
        - { name: cursor, in: query, schema: { type: string } }
        - { name: X-Trace, in: header, required: true, schema: { type: string } }
    post:
      operationId: createPet
      tags: [pets]
      requestBody:
        content:
          application/json:
            schema: { $ref: "#/components/schemas/Pet" }
  /pets/{petId}:
    parameters:
      - { name: petId, in: path, required: true, schema: { type: string } }
    delete:
      tags: [pets]
      security: [ { key: [] } ]
  /health:
    get: {}
components:
  securitySchemes:
    bearerAuth: { type: http, scheme: bearer }
    key: { type: apiKey, in: header, name: X-API-Key }
  schemas:
    Pet:
      type: object
      required: [name]
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string, example: rex }
        tags: { type: array, items: { type: string } }
        born: { type: string, format: date }
        owner: { $ref: "#/components/schemas/Owner" }
    Owner:
      allOf:
        - { type: object, properties: { email: { type: string, format: email } } }
        - { type: object, properties: { vip: { type: boolean } } }
"##;

    #[test]
    fn a_spec_becomes_a_file_a_tag() {
        let c = super::super::import(SPEC).unwrap();
        assert_eq!(c.envs["$shared"]["baseUrl"], "https://api.petstore.test/v1");
        let files: Vec<&str> = c.files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(files, ["pets.http", "Pet-Store.http"]);
        let pets = &c.files[0].1;
        let list = &pets[0];
        assert_eq!((list.name.as_str(), list.method.as_str(), list.url.as_str()), ("listPets", "GET", "{{baseUrl}}/pets?limit=20"));
        assert_eq!(list.headers, [("X-Trace".to_string(), "string".to_string()), ("Authorization".into(), "Bearer {{token}}".into())]);
        assert_eq!(list.note.as_deref(), Some("List all pets"));
        let create = &pets[1];
        let body: Value = serde_json::from_str(create.body.as_deref().unwrap()).unwrap();
        assert_eq!(body, serde_json::json!({"name": "rex", "tags": ["string"], "born": "2024-01-01", "owner": {"email": "ada@example.com", "vip": false}}));
        assert!(create.headers.contains(&("Content-Type".into(), "application/json".into())));
        let delete = &pets[2];
        assert_eq!((delete.url.as_str(), delete.headers.as_slice()), ("{{baseUrl}}/pets/{{petId}}", &[("X-API-Key".to_string(), "{{apiKey}}".to_string())][..]));
        assert_eq!(c.files[1].1[0].name, "GET /health");
        assert!(super::super::import("{\"swagger\": \"2.0\", \"paths\": {}}").unwrap_err().contains("OpenAPI 3"));
    }
}
