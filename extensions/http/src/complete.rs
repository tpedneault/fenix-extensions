//! What's offered as you type in a `.http` file -- `{{variables}}` (the
//! file's, the environment's, the secrets', the system's, other requests'
//! answers), header names and their usual values, methods, directives --
//! and the variables it uses that nothing sets. In a GraphQL request's
//! query, the fields and arguments of its server's schema, once read.

use std::sync::Arc;

use crate::graphql::{self, Offer, Schema};
use crate::parse::{HttpFile, Request};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Variable,
    Header,
    Value,
    Method,
    Directive,
    /// A GraphQL field or argument.
    Field,
    /// A GraphQL type, after `... on`.
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub detail: String,
    /// What goes in, from column `from` to the cursor.
    pub insert: String,
    pub kind: Kind,
}

/// What it knows besides the file: the environment's variables and the
/// secrets' names, the OAuth configurations.
#[derive(Debug, Clone, Default)]
pub struct Known {
    pub env: Vec<(String, String)>,
    pub env_name: Option<String>,
    pub secrets: Vec<String>,
    pub auth_ids: Vec<String>,
    /// GraphQL schemas read, by the URL of the request as it's written.
    pub schemas: Vec<(String, Arc<Schema>)>,
}

pub const SYSTEM: &[(&str, &str)] = &[
    ("$uuid", "a random UUID"),
    ("$timestamp", "seconds since 1970 ($timestamp -1 d)"),
    ("$isoTimestamp", "now, ISO 8601, UTC"),
    ("$datetime iso8601", "now ($datetime rfc1123, \"YYYY-MM-DD\" 1 d)"),
    ("$localDatetime iso8601", "now, here"),
    ("$randomInt 0 100", "a random integer, from up to"),
    ("$random.alphanumeric(8)", "random letters and digits"),
    ("$random.email", "a random address"),
    ("$dotenv NAME", "from the project's .env"),
    ("$secret name", "from the keychain (HTTP: set a secret)"),
    ("$processEnv NAME", "Fenix's environment variable (http.env names those it may read)"),
];

pub const HEADERS: &[&str] = &[
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Disposition",
    "Content-Length",
    "Content-Type",
    "Cookie",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "Origin",
    "Prefer",
    "Range",
    "Referer",
    "User-Agent",
    "X-API-Key",
    "X-Request-Id",
    "X-Request-Type",
    "X-Requested-With",
];

const TYPES: &[&str] = &["application/json", "application/x-www-form-urlencoded", "multipart/form-data; boundary=----FenixFormBoundary", "text/plain", "text/html", "application/xml", "application/octet-stream", "application/graphql"];

pub const DIRECTIVES: &[(&str, &str)] = &[
    ("name", "what chained requests call it: {{name.response...}}"),
    ("test", "a test on its answer: status == 200, $.id exists"),
    ("no-redirect", "a 3xx is the answer"),
    ("no-cookie-jar", "no cookies sent or kept"),
    ("timeout", "how long it may take: 10 (seconds), 500 ms, 2 m"),
    ("insecure", "a server whose certificate doesn't check out"),
    ("stream", "its body shown as it comes: server-sent events"),
    ("no-log", "not kept in history"),
];

fn item(label: &str, detail: &str, insert: String, kind: Kind) -> Item {
    Item { label: label.to_string(), detail: detail.to_string(), insert, kind }
}

/// Whether `r` is a GraphQL request: JetBrains' method, or REST Client's
/// header.
pub fn is_graphql(r: &Request) -> bool {
    r.method == "GRAPHQL" || r.header("x-request-type").is_some_and(|v| v.eq_ignore_ascii_case("graphql"))
}

/// The GraphQL request whose query `line` is in, if it is.
pub fn graphql_query_at(file: &HttpFile, line: u32) -> Option<&Request> {
    file.requests.iter().find(|r| is_graphql(r) && r.body.as_ref().is_some_and(|b| line >= b.start && line < b.end.max(b.start + 1)))
}

/// What a GraphQL query offers at the cursor: `before`, the query up to
/// it.
fn graphql_items(schema: &Schema, before: &str) -> Vec<Item> {
    graphql::offers(schema, before)
        .into_iter()
        .map(|o| match o {
            Offer::Field { name, ty, description } => {
                let detail = if description.is_empty() { ty } else { format!("{ty} · {description}") };
                item(&name, &detail, name.clone(), Kind::Field)
            }
            Offer::Arg { name, ty } => item(&name, &ty, format!("{name}: "), Kind::Field),
            Offer::Type { name, kind } => item(&name, &kind, name.clone(), Kind::Type),
        })
        .collect()
}

/// What's offered at (`line`, `col`) of `text` (columns in characters),
/// and the column it replaces from.
pub fn at(text: &str, line: u32, col: usize, known: &Known) -> Option<(usize, Vec<Item>)> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let cur = *lines.get(line as usize)?;
    let chars: Vec<char> = cur.chars().collect();
    let col = col.min(chars.len());
    let before: String = chars[..col].iter().collect();
    let after: String = chars[col..].iter().collect();
    let file = HttpFile::parse(text);

    // {{ ... a variable.
    if let Some(open) = before.rfind("{{").filter(|&i| !before[i..].contains("}}")) {
        let typed = before[open + 2..].trim_start();
        let from = col - typed.chars().count();
        let close = if after.trim_start().starts_with("}}") { "" } else { "}}" };
        let mut items = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut add = |items: &mut Vec<Item>, name: &str, detail: String| {
            if !seen.iter().any(|s| s == name) {
                seen.push(name.to_string());
                items.push(item(name, &detail, format!("{name}{close}"), Kind::Variable));
            }
        };
        for v in &file.vars {
            add(&mut items, &v.name, format!("file · {}", v.value));
        }
        for (n, v) in &known.env {
            add(&mut items, n, format!("{} · {v}", known.env_name.as_deref().unwrap_or("$shared")));
        }
        for n in &known.secrets {
            add(&mut items, n, "secret".into());
        }
        for (n, d) in SYSTEM {
            add(&mut items, n, (*d).into());
        }
        for id in &known.auth_ids {
            add(&mut items, &format!("$auth.token(\"{id}\")"), "an OAuth token, got when it's needed".into());
        }
        for r in &file.requests {
            if let Some(n) = r.name() {
                add(&mut items, &format!("{n}.response.body.$."), format!("{} {}'s answer, by JSONPath", r.method, r.url));
                add(&mut items, &format!("{n}.response.headers."), "a header of its answer".into());
            }
        }
        return Some((from, items));
    }

    let trimmed = before.trim_start();
    // # @ ... a directive.
    if let Some(rest) = trimmed.strip_prefix('#').or_else(|| trimmed.strip_prefix("//")).map(str::trim_start) {
        let typed = rest.strip_prefix('@')?;
        if typed.contains(char::is_whitespace) {
            return None;
        }
        let from = col - typed.chars().count();
        return Some((from, DIRECTIVES.iter().map(|(n, d)| item(n, d, format!("{n} "), Kind::Directive)).collect()));
    }

    // A GraphQL query, with its server's schema.
    if let Some(r) = graphql_query_at(&file, line) {
        let schema = known.schemas.iter().find(|(u, _)| *u == r.url).map(|(_, s)| s.clone())?;
        let start = r.body.as_ref().map_or(line, |b| b.start);
        let mut query: String = lines[start as usize..line as usize].iter().map(|l| format!("{l}\n")).collect();
        query.push_str(&before);
        if crate::body::split_graphql(&query).1.is_some() {
            // In its variables.
            return None;
        }
        let typed = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').count();
        return Some((col - typed, graphql_items(&schema, &query)));
    }

    // Where the line is: a request's line, its headers, its body, or
    // between requests.
    let request = file.requests.iter().find(|r| r.contains(line));
    let in_headers = request.is_some_and(|r| {
        let first = r.line + r.url_lines;
        line >= first && (first..line).all(|l| !lines.get(l as usize).is_none_or(|t| t.trim().is_empty()))
    });
    if in_headers {
        match before.split_once(':') {
            None => {
                let from = col - trimmed.chars().count();
                return Some((from, HEADERS.iter().map(|h| item(h, "header", format!("{h}: "), Kind::Header)).collect()));
            }
            Some((name, value)) => {
                let typed = value.trim_start();
                let from = col - typed.chars().count();
                let name = name.trim();
                let values: Vec<Item> = if name.eq_ignore_ascii_case("content-type") || name.eq_ignore_ascii_case("accept") {
                    TYPES.iter().map(|t| item(t, "", t.to_string(), Kind::Value)).collect()
                } else if name.eq_ignore_ascii_case("authorization") {
                    let mut v = vec![item("Bearer", "a token", "Bearer ".into(), Kind::Value), item("Basic", "user:password, encoded as it's sent", "Basic ".into(), Kind::Value)];
                    v.extend(known.auth_ids.iter().map(|id| item(&format!("Bearer {{{{$auth.token(\"{id}\")}}}}"), "OAuth 2.0, from the env file", format!("Bearer {{{{$auth.token(\"{id}\")}}}}"), Kind::Value)));
                    v
                } else if name.eq_ignore_ascii_case("x-request-type") {
                    vec![item("GraphQL", "the body is a query, then its variables", "GraphQL".into(), Kind::Value)]
                } else {
                    return None;
                };
                return Some((from, values));
            }
        }
    }
    // A request line's method.
    let at_request_line = request.is_some_and(|r| r.line == line) || request.is_none();
    if at_request_line && !trimmed.contains(char::is_whitespace) && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        let from = col - trimmed.chars().count();
        return Some((from, crate::parse::METHODS.iter().map(|m| item(m, "", format!("{m} "), Kind::Method)).collect()));
    }
    None
}

/// A `{{variable}}` nothing sets: (line, from column, to column, what's
/// said), columns in characters.
pub fn unset(text: &str, known: &Known) -> Vec<(u32, usize, usize, String)> {
    let file = HttpFile::parse(text);
    let mut out = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        let t = line.trim_start();
        if t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        for (inner, range) in crate::vars::find(line) {
            let from = line[..range.start].chars().count();
            let to = from + line[range.clone()].chars().count();
            let said = if let Some(system) = inner.strip_prefix('$') {
                let name = system.split([' ', '(']).next().unwrap_or("");
                let known_system = SYSTEM.iter().any(|(s, _)| s[1..].split([' ', '(']).next() == Some(name)) || matches!(name, "guid" | "random.uuid" | "random.integer" | "random.alphabetic" | "random.hexadecimal" | "auth.token" | "auth.idToken" | "processEnv");
                (!known_system).then(|| format!("${name} isn't a system variable"))
            } else if let Some(i) = inner.find(".response.").or_else(|| inner.find(".request.")) {
                let name = &inner[..i];
                file.named(name).is_none().then(|| format!("no request here is called {name} (# @name {name})"))
            } else {
                let set = file.var(&inner).is_some() || known.env.iter().any(|(k, _)| *k == inner) || known.secrets.contains(&inner);
                (!set).then(|| match &known.env_name {
                    Some(env) => format!("{{{{{inner}}}}} isn't set: not in this file, environment {env} or the secrets"),
                    None => format!("{{{{{inner}}}}} isn't set: not in this file or the secrets, and no environment is chosen"),
                })
            };
            if let Some(said) = said {
                out.push((n as u32, from, to, said));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "@host = https://x.test\n\n### login\n# @name login\nPOST {{host}}/login\nContent-Type: application/json\n\n{}\n\n###\nGET {{host}}/me\nAuthorization: Bearer {{login.response.body.$.token}}\nX-Env: {{ver}} {{nope}} {{$nope}} {{other.response.body.$.x}}\n";

    fn known() -> Known {
        Known { env: vec![("ver".into(), "v1".into())], env_name: Some("dev".into()), secrets: vec!["pw".into()], auth_ids: vec!["api".into()], schemas: Vec::new() }
    }

    fn labels(text: &str, line: u32, col: usize) -> (usize, Vec<String>) {
        let (from, items) = at(text, line, col, &known()).unwrap_or_default();
        (from, items.into_iter().map(|i| i.label).collect())
    }

    #[test]
    fn variables_after_braces() {
        let text = FILE.replace("GET {{host}}/me", "GET {{ho");
        let (from, l) = labels(&text, 10, 8);
        assert_eq!(from, 6);
        assert!(l.contains(&"host".to_string()) && l.contains(&"ver".to_string()) && l.contains(&"pw".to_string()));
        assert!(l.contains(&"$uuid".to_string()) && l.contains(&"login.response.body.$.".to_string()) && l.contains(&"$auth.token(\"api\")".to_string()));
        let (_, items) = at(&text, 10, 8, &known()).unwrap();
        assert_eq!(items[0].insert, "host}}", "closed when it isn't");
        let (_, items) = at(FILE, 10, 10, &known()).unwrap();
        assert_eq!(items[0].insert, "host", "not closed again");
    }

    #[test]
    fn headers_values_methods_and_directives() {
        let text = FILE.replace("Content-Type: application/json", "Con");
        let (from, l) = labels(&text, 5, 3);
        assert_eq!(from, 0);
        assert!(l.contains(&"Content-Type".to_string()));
        let text = FILE.replace("Content-Type: application/json", "Content-Type: app");
        let (from, l) = labels(&text, 5, 17);
        assert_eq!((from, l[0].as_str()), (14, "application/json"));
        let text = FILE.replace("Authorization: Bearer {{login.response.body.$.token}}", "Authorization: ");
        let (_, l) = labels(&text, 11, 15);
        assert!(l.contains(&"Bearer {{$auth.token(\"api\")}}".to_string()));
        let text = FILE.replace("GET {{host}}/me", "PO");
        let (from, l) = labels(&text, 10, 2);
        assert_eq!((from, l.contains(&"POST".to_string())), (0, true));
        let text = FILE.replace("# @name login", "# @no");
        let (from, l) = labels(&text, 3, 5);
        assert_eq!(from, 3);
        assert!(l.contains(&"no-redirect".to_string()));
        assert_eq!(at(FILE, 7, 1, &known()), None, "nothing in a body");
    }

    #[test]
    fn a_graphql_query_offers_its_schemas_fields() {
        let text = "@api = https://x.test/graphql\n\nGRAPHQL {{api}}\n\nquery {\n  user(id: 1) {\n    na\n  }\n}\n\n{\n  \"a\": 1\n}\n";
        let mut k = known();
        assert_eq!(at(text, 6, 6, &k), None, "no schema read yet");
        k.schemas.push(("{{api}}".into(), Arc::new(Schema::from_introspection(&graphql::tests::answer()).unwrap())));
        let (from, items) = at(text, 6, 6, &k).unwrap();
        assert_eq!(from, 4);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["id", "name", "friends", "__typename"]);
        assert_eq!((items[2].detail.as_str(), items[2].kind), ("[User!] · the friends", Kind::Field));
        let (_, items) = at(text, 5, 7, &k).unwrap();
        assert_eq!(items[0].insert, "id: ", "an argument");
        assert_eq!(at(text, 11, 2, &k), None, "nothing in its variables");
        let rest = "POST https://x.test/graphql\nX-Request-Type: GraphQL\n\n{ ";
        k.schemas.push(("https://x.test/graphql".into(), k.schemas[0].1.clone()));
        let (_, items) = at(rest, 3, 2, &k).unwrap();
        assert_eq!(items[0].label, "user", "REST Client's header");
        assert!(graphql_query_at(&HttpFile::parse(rest), 3).is_some());
        assert!(graphql_query_at(&HttpFile::parse(FILE), 7).is_none());
    }

    #[test]
    fn unset_variables_are_found() {
        let found: Vec<String> = unset(FILE, &known()).into_iter().map(|(l, a, b, s)| format!("{l}:{a}-{b} {s}")).collect();
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("12:15-23 {{nope}} isn't set"), "{found:?}");
        assert!(found[1].contains("$nope isn't a system variable"));
        assert!(found[2].contains("no request here is called other"));
    }
}
