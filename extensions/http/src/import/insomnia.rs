//! Insomnia's exports, v4 (`__export_format: 4`, JSON or YAML): its
//! request groups are `.http` files, its environments environments.
//! Insomnia's `{{ _.name }}` is `{{name}}` here.

use serde_json::{Map, Value};

use super::{slug, Collection, Draft};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn on(v: &&Value) -> bool {
    !v.get("disabled").and_then(Value::as_bool).unwrap_or(false)
}

/// `{{ _.base_url }}` as `{{base_url}}`; a template tag (`{% %}`) left as
/// it is, with a note.
pub fn vars(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("}}") {
            Some(j) => {
                let inner = after[..j].trim();
                let inner = inner.strip_prefix("_.").unwrap_or(inner);
                out.push_str(&format!("{{{{{inner}}}}}"));
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn request(r: &Value, notes: &mut Vec<String>) -> Draft {
    let mut d = Draft { name: s(r, "name").to_string(), method: s(r, "method").to_ascii_uppercase(), url: vars(s(r, "url")), ..Draft::default() };
    if d.method.is_empty() {
        d.method = "GET".into();
    }
    let params: Vec<(String, String)> = r.get("parameters").and_then(Value::as_array).into_iter().flatten().filter(on).map(|p| (vars(s(p, "name")), vars(s(p, "value")))).collect();
    if !params.is_empty() {
        let sep = if d.url.contains('?') { "&" } else { "?" };
        d.url = format!("{}{sep}{}", d.url, params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"));
    }
    for h in r.get("headers").and_then(Value::as_array).into_iter().flatten().filter(on) {
        d.headers.push((s(h, "name").to_string(), vars(s(h, "value"))));
    }
    if let Some(a) = r.get("authentication").filter(|a| !a.get("disabled").and_then(Value::as_bool).unwrap_or(false)) {
        match s(a, "type") {
            "bearer" => d.headers.push(("Authorization".into(), format!("{} {}", if s(a, "prefix").is_empty() { "Bearer" } else { s(a, "prefix") }, vars(s(a, "token"))))),
            "basic" => d.headers.push(("Authorization".into(), format!("Basic {}:{}", vars(s(a, "username")), vars(s(a, "password"))))),
            "apikey" => {
                if s(a, "addTo") == "queryParams" {
                    let sep = if d.url.contains('?') { '&' } else { '?' };
                    d.url = format!("{}{sep}{}={}", d.url, s(a, "key"), vars(s(a, "value")));
                } else {
                    d.headers.push((s(a, "key").to_string(), vars(s(a, "value"))));
                }
            }
            "" | "none" => {}
            other => {
                let note = format!("Insomnia's {other} auth isn't brought: set it up in http-client.env.json (OAuth 2.0) or as a header");
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
        }
    }
    if let Some(b) = r.get("body").filter(|b| b.as_object().is_some_and(|o| !o.is_empty())) {
        let mime = s(b, "mimeType");
        if !mime.is_empty() && !d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            d.headers.push(("Content-Type".into(), mime.into()));
        }
        if let Some(params) = b.get("params").and_then(Value::as_array) {
            if mime.starts_with("multipart/") {
                let boundary = "----FenixFormBoundary";
                d.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
                d.headers.push(("Content-Type".into(), format!("multipart/form-data; boundary={boundary}")));
                let mut text = String::new();
                for p in params.iter().filter(on) {
                    text.push_str(&format!("--{boundary}\n"));
                    if s(p, "type") == "file" {
                        let file = s(p, "fileName");
                        let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
                        text.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{name}\"\n\n< ./{name}\n", s(p, "name")));
                    } else {
                        text.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\n\n{}\n", s(p, "name"), vars(s(p, "value"))));
                    }
                }
                text.push_str(&format!("--{boundary}--"));
                d.body = Some(text);
            } else {
                d.body = Some(params.iter().filter(on).map(|p| format!("{}={}", s(p, "name"), vars(s(p, "value")))).collect::<Vec<_>>().join("\n&"));
            }
        } else if !s(b, "fileName").is_empty() {
            let f = s(b, "fileName");
            d.body = Some(format!("< ./{}", f.rsplit(['/', '\\']).next().unwrap_or(f)));
        } else if mime == "application/graphql" {
            // Insomnia keeps it as the JSON a server takes.
            d.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
            d.headers.push(("Content-Type".into(), "application/json".into()));
            d.body = Some(vars(s(b, "text")));
        } else {
            d.body = Some(vars(s(b, "text")));
        }
    }
    let note = s(r, "description");
    d.note = (!note.trim().is_empty()).then(|| note.to_string());
    d
}

/// A request group's path: its groups' names up to the workspace.
fn path_of(by_id: &Map<String, Value>, id: &str) -> String {
    let mut parts = Vec::new();
    let mut id = id.to_string();
    while let Some(r) = by_id.get(&id) {
        if s(r, "_type") != "request_group" {
            break;
        }
        parts.push(slug(s(r, "name")));
        id = s(r, "parentId").to_string();
    }
    parts.reverse();
    parts.join("/")
}

pub fn import(doc: &Value) -> Result<Collection, String> {
    let resources = doc.get("resources").and_then(Value::as_array).ok_or("an Insomnia export has resources (v5's YAML isn't read: export as v4)")?;
    let by_id: Map<String, Value> = resources.iter().map(|r| (s(r, "_id").to_string(), r.clone())).collect();
    let workspace = resources.iter().find(|r| s(r, "_type") == "workspace");
    let mut c = Collection { name: workspace.map(|w| s(w, "name").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Insomnia".into()), ..Collection::default() };
    for r in resources.iter().filter(|r| s(r, "_type") == "request") {
        let path = path_of(&by_id, s(r, "parentId"));
        let file = if path.is_empty() { format!("{}.http", slug(&c.name)) } else { format!("{path}.http") };
        let draft = request(r, &mut c.notes);
        match c.files.iter_mut().find(|(f, _)| *f == file) {
            Some((_, drafts)) => drafts.push(draft),
            None => c.files.push((file, vec![draft])),
        }
    }
    // The base environment is every one's; its sub-environments are
    // environments.
    for e in resources.iter().filter(|r| s(r, "_type") == "environment") {
        let Some(Value::Object(data)) = e.get("data") else { continue };
        let base = by_id.get(s(e, "parentId")).is_some_and(|p| s(p, "_type") == "workspace");
        let name = if base { crate::env::SHARED.to_string() } else { s(e, "name").to_string() };
        if !data.is_empty() && !name.is_empty() {
            c.envs.insert(name, Value::Object(data.clone()));
        }
    }
    for r in resources.iter().filter(|r| s(r, "_type").starts_with("websocket") || s(r, "_type").starts_with("grpc")) {
        let note = format!("{}: Insomnia's {} requests aren't brought", s(r, "name"), s(r, "_type"));
        c.notes.push(note);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"{
      "_type": "export", "__export_format": 4,
      "resources": [
        { "_id": "wrk_1", "_type": "workspace", "name": "Shop API" },
        { "_id": "fld_1", "_type": "request_group", "parentId": "wrk_1", "name": "Orders" },
        { "_id": "fld_2", "_type": "request_group", "parentId": "fld_1", "name": "Admin" },
        { "_id": "req_1", "_type": "request", "parentId": "fld_1", "name": "List orders", "method": "GET", "url": "{{ _.base_url }}/orders",
          "parameters": [{"name": "page", "value": "1"}, {"name": "off", "value": "x", "disabled": true}],
          "headers": [{"name": "Accept", "value": "application/json"}],
          "authentication": { "type": "bearer", "token": "{{ _.token }}" }, "body": {} },
        { "_id": "req_2", "_type": "request", "parentId": "fld_2", "name": "Refund", "method": "post", "url": "{{ _.base_url }}/refunds",
          "body": { "mimeType": "application/json", "text": "{\"order\": \"{{ _.order }}\"}" } },
        { "_id": "req_3", "_type": "request", "parentId": "wrk_1", "name": "Sign in", "method": "POST", "url": "{{ _.base_url }}/login",
          "body": { "mimeType": "application/x-www-form-urlencoded", "params": [{"name": "user", "value": "ada"}] },
          "authentication": { "type": "oauth2" } },
        { "_id": "env_1", "_type": "environment", "parentId": "wrk_1", "name": "Base Environment", "data": { "base_url": "https://shop.test" } },
        { "_id": "env_2", "_type": "environment", "parentId": "env_1", "name": "Staging", "data": { "base_url": "https://staging.shop.test" } }
      ]
    }"#;

    #[test]
    fn an_export_becomes_files_and_environments() {
        let c = super::super::import(EXPORT).unwrap();
        assert_eq!(c.name, "Shop API");
        let files: Vec<&str> = c.files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(files, ["Orders.http", "Orders/Admin.http", "Shop-API.http"]);
        let list = &c.files[0].1[0];
        assert_eq!(list.url, "{{base_url}}/orders?page=1");
        assert_eq!(list.headers, [("Accept".to_string(), "application/json".to_string()), ("Authorization".into(), "Bearer {{token}}".into())]);
        assert_eq!(list.body, None);
        let refund = &c.files[1].1[0];
        assert_eq!((refund.method.as_str(), refund.body.as_deref()), ("POST", Some("{\"order\": \"{{order}}\"}")));
        assert_eq!(c.files[2].1[0].body.as_deref(), Some("user=ada"));
        assert_eq!(c.envs["$shared"]["base_url"], "https://shop.test");
        assert_eq!(c.envs["Staging"]["base_url"], "https://staging.shop.test");
        assert!(c.notes.iter().any(|n| n.contains("oauth2")));
        assert_eq!(vars("{{_.a}} and {{ b }} and {{ open"), "{{a}} and {{b}} and {{ open");
    }
}
