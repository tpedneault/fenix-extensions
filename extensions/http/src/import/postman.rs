//! Postman collections, v2.1 (and v2.0, the same but for `url`): a folder
//! of the collection's is a `.http` file, its requests in it; the
//! collection's variables go under `$shared`. Postman writes `{{var}}` as
//! `.http` files do, so they carry over as they are. Pre-request and test
//! scripts are JavaScript, and aren't brought.

use serde_json::{Map, Value};

use super::{slug, Collection, Draft};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn enabled(v: &&Value) -> bool {
    !v.get("disabled").and_then(Value::as_bool).unwrap_or(false)
}

/// A request's URL: v2.1's object (`raw`, or its parts), or a string.
fn url(v: &Value) -> String {
    match v {
        Value::String(u) => u.clone(),
        Value::Object(o) => {
            if let Some(raw) = o.get("raw").and_then(Value::as_str) {
                return raw.to_string();
            }
            let host = match o.get("host") {
                Some(Value::Array(h)) => h.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("."),
                Some(Value::String(h)) => h.clone(),
                _ => String::new(),
            };
            let path = match o.get("path") {
                Some(Value::Array(p)) => p.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/"),
                Some(Value::String(p)) => p.trim_start_matches('/').to_string(),
                _ => String::new(),
            };
            let protocol = o.get("protocol").and_then(Value::as_str).map(|p| format!("{p}://")).unwrap_or_default();
            let query: Vec<(String, String)> = o.get("query").and_then(Value::as_array).map(|q| q.iter().filter(enabled).map(|p| (s(p, "key").to_string(), s(p, "value").to_string())).collect()).unwrap_or_default();
            crate::url::join(&format!("{protocol}{host}/{path}"), &query)
        }
        _ => String::new(),
    }
}

/// Postman's `:id` path variables, as `{{id}}`.
fn path_vars(url: &str) -> String {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    let path = path.split('/').map(|seg| if seg.starts_with(':') && seg.len() > 1 { format!("{{{{{}}}}}", &seg[1..]) } else { seg.to_string() }).collect::<Vec<_>>().join("/");
    match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    }
}

/// An `auth` block's header, when it's one a header says.
fn auth(a: &Value, d: &mut Draft, notes: &mut Vec<String>) {
    let ty = s(a, "type");
    let param = |k: &str| a.get(ty).and_then(Value::as_array).and_then(|ps| ps.iter().find(|p| s(p, "key") == k)).map(|p| crate::env::as_text(p.get("value").unwrap_or(&Value::Null))).unwrap_or_default();
    match ty {
        "bearer" => d.headers.push(("Authorization".into(), format!("Bearer {}", param("token")))),
        "basic" => d.headers.push(("Authorization".into(), format!("Basic {}:{}", param("username"), param("password")))),
        "apikey" => {
            let (key, value) = (param("key"), param("value"));
            if param("in") == "query" {
                let sep = if d.url.contains('?') { '&' } else { '?' };
                d.url = format!("{}{sep}{key}={value}", d.url);
            } else {
                d.headers.push((if key.is_empty() { "X-API-Key".into() } else { key }, value));
            }
        }
        "noauth" | "" => {}
        "oauth2" => {
            let note = "OAuth 2.0: set it up in http-client.env.json's Security.Auth, and use {{$auth.token(\"id\")}}".to_string();
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        other => {
            let note = format!("{other} auth isn't brought: set its header yourself");
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
}

fn body(b: &Value, d: &mut Draft) {
    let has_type = |d: &Draft| d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    match s(b, "mode") {
        "raw" => {
            let raw = s(b, "raw");
            if !has_type(d) {
                let ty = match b.pointer("/options/raw/language").and_then(Value::as_str) {
                    Some("json") => Some("application/json"),
                    Some("xml") => Some("application/xml"),
                    Some("html") => Some("text/html"),
                    Some("text") => Some("text/plain"),
                    _ => None,
                };
                if let Some(ty) = ty {
                    d.headers.push(("Content-Type".into(), ty.into()));
                }
            }
            d.body = Some(raw.to_string());
        }
        "urlencoded" => {
            let pairs: Vec<String> = b.get("urlencoded").and_then(Value::as_array).map(|ps| ps.iter().filter(enabled).map(|p| format!("{}={}", s(p, "key"), s(p, "value"))).collect()).unwrap_or_default();
            if !has_type(d) {
                d.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            }
            d.body = Some(pairs.join("\n&"));
        }
        "formdata" => {
            let boundary = "----FenixFormBoundary";
            let mut text = String::new();
            for p in b.get("formdata").and_then(Value::as_array).into_iter().flatten().filter(enabled) {
                text.push_str(&format!("--{boundary}\n"));
                if s(p, "type") == "file" {
                    let src = match p.get("src") {
                        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or("").to_string(),
                        Some(Value::String(x)) => x.clone(),
                        _ => String::new(),
                    };
                    let name = src.rsplit(['/', '\\']).next().unwrap_or("file").to_string();
                    text.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{name}\"\n\n< ./{name}\n", s(p, "key")));
                } else {
                    text.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\n\n{}\n", s(p, "key"), s(p, "value")));
                }
            }
            text.push_str(&format!("--{boundary}--"));
            d.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
            d.headers.push(("Content-Type".into(), format!("multipart/form-data; boundary={boundary}")));
            d.body = Some(text);
        }
        "file" => {
            let src = b.pointer("/file/src").and_then(Value::as_str).unwrap_or("");
            d.body = Some(format!("< ./{}", src.rsplit(['/', '\\']).next().unwrap_or(src)));
        }
        "graphql" => {
            let q = b.pointer("/graphql/query").and_then(Value::as_str).unwrap_or("");
            let vars = b.pointer("/graphql/variables").and_then(Value::as_str).unwrap_or("").trim().to_string();
            d.headers.push(("X-Request-Type".into(), "GraphQL".into()));
            d.body = Some(if vars.is_empty() { q.to_string() } else { format!("{}\n\n{vars}", q.trim_end()) });
        }
        _ => {}
    }
}

fn request(item: &Value, inherited: Option<&Value>, notes: &mut Vec<String>) -> Draft {
    let r = item.get("request").unwrap_or(&Value::Null);
    let mut d = Draft { name: s(item, "name").to_string(), ..Draft::default() };
    // v2.0 allowed a request that's just its URL.
    if let Value::String(u) = r {
        d.method = "GET".into();
        d.url = path_vars(u);
        return d;
    }
    d.method = match s(r, "method") {
        "" => "GET".into(),
        m => m.to_ascii_uppercase(),
    };
    d.url = path_vars(&url(r.get("url").unwrap_or(&Value::Null)));
    for h in r.get("header").and_then(Value::as_array).into_iter().flatten().filter(enabled) {
        d.headers.push((s(h, "key").to_string(), s(h, "value").to_string()));
    }
    if let Some(a) = r.get("auth").or(inherited) {
        auth(a, &mut d, notes);
    }
    if let Some(b) = r.get("body") {
        body(b, &mut d);
    }
    let desc = match r.get("description") {
        Some(Value::String(x)) => Some(x.clone()),
        Some(Value::Object(o)) => o.get("content").and_then(Value::as_str).map(String::from),
        _ => None,
    };
    d.note = desc.filter(|x| !x.trim().is_empty());
    if item.get("event").and_then(Value::as_array).is_some_and(|e| !e.is_empty()) {
        let note = "Postman's scripts (pre-request, tests) are JavaScript, and aren't brought: say tests with # @test".to_string();
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    d
}

/// Folder `items` into `files`: its own requests in a file of its path.
fn walk(items: &[Value], path: &str, inherited: Option<&Value>, c: &mut Collection) {
    let mut here = Vec::new();
    for item in items {
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let sub = if path.is_empty() { slug(s(item, "name")) } else { format!("{path}/{}", slug(s(item, "name"))) };
            walk(children, &sub, item.get("auth").or(inherited), c);
        } else {
            here.push(request(item, inherited, &mut c.notes));
        }
    }
    if !here.is_empty() {
        let file = if path.is_empty() { format!("{}.http", slug(&c.name)) } else { format!("{path}.http") };
        c.files.push((file, here));
    }
}

pub fn import(doc: &Value) -> Result<Collection, String> {
    let items = doc.get("item").and_then(Value::as_array).ok_or("a Postman collection has an item list")?;
    let mut c = Collection { name: doc.pointer("/info/name").and_then(Value::as_str).unwrap_or("Postman").to_string(), ..Collection::default() };
    walk(items, "", doc.get("auth"), &mut c);
    let mut shared = Map::new();
    for v in doc.get("variable").and_then(Value::as_array).into_iter().flatten().filter(enabled) {
        shared.insert(s(v, "key").to_string(), v.get("value").cloned().unwrap_or(Value::String(String::new())));
    }
    if !shared.is_empty() {
        c.envs.insert(crate::env::SHARED.into(), Value::Object(shared));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    const COLLECTION: &str = r#"{
      "info": { "_postman_id": "1", "name": "Pet Store", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}", "type": "string" }] },
      "item": [
        { "name": "Pets", "item": [
          { "name": "List pets", "request": { "method": "GET", "header": [{"key": "Accept", "value": "application/json"}, {"key": "X-Off", "value": "1", "disabled": true}],
            "url": { "raw": "{{baseUrl}}/pets?limit=10", "host": ["{{baseUrl}}"], "path": ["pets"] } },
            "event": [{ "listen": "test", "script": { "exec": ["pm.test()"] } }] },
          { "name": "Get pet", "request": { "method": "GET", "url": { "host": ["{{baseUrl}}"], "path": ["pets", ":petId"], "query": [{"key": "full", "value": "1"}] } } },
          { "name": "Add pet", "request": { "method": "POST", "auth": { "type": "apikey", "apikey": [{"key": "key", "value": "X-Key"}, {"key": "value", "value": "{{key}}"}] },
            "body": { "mode": "raw", "raw": "{\"name\": \"rex\"}", "options": { "raw": { "language": "json" } } }, "url": "{{baseUrl}}/pets" } }
        ]},
        { "name": "Upload", "request": { "method": "POST", "url": "{{baseUrl}}/up",
          "body": { "mode": "formdata", "formdata": [{"key": "note", "value": "hi", "type": "text"}, {"key": "file", "type": "file", "src": "/home/me/cat.png"}] } } },
        { "name": "Login", "request": { "method": "POST", "url": "{{baseUrl}}/login", "auth": { "type": "basic", "basic": [{"key": "username", "value": "ada"}, {"key": "password", "value": "pw"}] },
          "body": { "mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}, {"key": "b", "value": "2"}] } } }
      ],
      "variable": [{ "key": "baseUrl", "value": "https://petstore.test/v1" }]
    }"#;

    #[test]
    fn a_collection_becomes_files() {
        let c = super::super::import(COLLECTION).unwrap();
        assert_eq!(c.name, "Pet Store");
        let names: Vec<&str> = c.files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names, ["Pets.http", "Pet-Store.http"]);
        let pets = &c.files[0].1;
        assert_eq!(pets[0].url, "{{baseUrl}}/pets?limit=10");
        assert_eq!(pets[0].headers, [("Accept".to_string(), "application/json".to_string()), ("Authorization".into(), "Bearer {{token}}".into())], "the collection's auth, and not the disabled header");
        assert_eq!(pets[1].url, "{{baseUrl}}/pets/{{petId}}?full=1");
        assert_eq!(pets[2].headers, [("X-Key".to_string(), "{{key}}".to_string()), ("Content-Type".into(), "application/json".into())]);
        assert_eq!(pets[2].body.as_deref(), Some("{\"name\": \"rex\"}"));
        let top = &c.files[1].1;
        let upload = top[0].body.as_deref().unwrap();
        assert!(upload.contains("name=\"file\"; filename=\"cat.png\"\n\n< ./cat.png"), "{upload}");
        assert!(top[0].headers.iter().any(|(k, v)| k == "Content-Type" && v.starts_with("multipart/form-data")));
        assert_eq!(top[1].body.as_deref(), Some("a=1\n&b=2"));
        assert!(top[1].headers.contains(&("Authorization".into(), "Basic ada:pw".into())));
        assert_eq!(c.envs["$shared"]["baseUrl"], "https://petstore.test/v1");
        assert!(c.notes.iter().any(|n| n.contains("JavaScript")));
        let text = super::super::to_http(pets);
        let f = crate::parse::HttpFile::parse(&text);
        assert_eq!(f.requests.len(), 3);
        assert_eq!(f.requests[2].name(), Some("Add_pet"));
    }
}
