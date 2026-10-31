//! GraphQL: a server's schema, read from its answer to the introspection
//! query, and what's offered from it as you write a query -- the fields
//! of the type the cursor's selection is on, a field's arguments, the
//! types a fragment may be on.

use std::collections::BTreeMap;

use serde_json::Value;

/// What's asked of a server for its schema: its types, their fields and
/// arguments, as deep as lists of non-nulls go.
pub const INTROSPECTION: &str = "query IntrospectionQuery { __schema { queryType { name } mutationType { name } subscriptionType { name } types { kind name description fields(includeDeprecated: true) { name description args { name type { ...TypeRef } } type { ...TypeRef } } inputFields { name type { ...TypeRef } } enumValues(includeDeprecated: true) { name } possibleTypes { name } } } } fragment TypeRef on __Type { kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } } }";

/// The introspection query as a request's body.
pub fn introspection_body() -> Vec<u8> {
    serde_json::json!({ "query": INTROSPECTION, "operationName": "IntrospectionQuery" }).to_string().into_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// As written: `[User!]!`.
    pub ty: String,
    /// The named type under the lists and non-nulls: `User`.
    pub base: String,
    pub args: Vec<(String, String)>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Type {
    /// `OBJECT`, `INTERFACE`, `UNION`, `ENUM`, `INPUT_OBJECT`, `SCALAR`.
    pub kind: String,
    pub fields: Vec<Field>,
    pub description: String,
    /// A union's members, an interface's implementations.
    pub possible: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    pub query: Option<String>,
    pub mutation: Option<String>,
    pub subscription: Option<String>,
    pub types: BTreeMap<String, Type>,
}

/// A type reference written as GraphQL writes it, and its named type.
fn type_ref(v: &Value) -> (String, String) {
    match v.get("kind").and_then(Value::as_str) {
        Some("NON_NULL") => {
            let (t, base) = type_ref(v.get("ofType").unwrap_or(&Value::Null));
            (format!("{t}!"), base)
        }
        Some("LIST") => {
            let (t, base) = type_ref(v.get("ofType").unwrap_or(&Value::Null));
            (format!("[{t}]"), base)
        }
        _ => {
            let name = v.get("name").and_then(Value::as_str).unwrap_or("?").to_string();
            (name.clone(), name)
        }
    }
}

impl Schema {
    /// From the server's answer: `{"data": {"__schema": ...}}`, or the
    /// `__schema` alone.
    pub fn from_introspection(body: &[u8]) -> Result<Schema, String> {
        let v: Value = serde_json::from_slice(body).map_err(|e| format!("the answer isn't JSON: {e}"))?;
        if let Some(errors) = v.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
            if v.pointer("/data/__schema").is_none() {
                let first = errors[0].get("message").and_then(Value::as_str).unwrap_or("an error");
                return Err(format!("the server refused the introspection query: {first}"));
            }
        }
        let s = v.pointer("/data/__schema").or_else(|| v.get("__schema")).ok_or("no __schema in the answer: is it a GraphQL endpoint, with introspection on?")?;
        let root = |k: &str| s.pointer(&format!("/{k}/name")).and_then(Value::as_str).map(String::from);
        let mut types = BTreeMap::new();
        for t in s.get("types").and_then(Value::as_array).into_iter().flatten() {
            let Some(name) = t.get("name").and_then(Value::as_str) else { continue };
            let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let field = |f: &Value| {
                let (ty, base) = type_ref(f.get("type").unwrap_or(&Value::Null));
                let args = f.get("args").and_then(Value::as_array).into_iter().flatten().map(|a| (text(a, "name"), type_ref(a.get("type").unwrap_or(&Value::Null)).0)).collect();
                Field { name: text(f, "name"), ty, base, args, description: text(f, "description") }
            };
            let fields = t.get("fields").and_then(Value::as_array).or_else(|| t.get("inputFields").and_then(Value::as_array)).into_iter().flatten().map(field).collect();
            let possible = t.get("possibleTypes").and_then(Value::as_array).into_iter().flatten().map(|p| text(p, "name")).collect();
            types.insert(name.to_string(), Type { kind: text(t, "kind"), fields, description: text(t, "description"), possible });
        }
        if types.is_empty() {
            return Err("the schema has no types".into());
        }
        Ok(Schema { query: root("queryType"), mutation: root("mutationType"), subscription: root("subscriptionType"), types })
    }

    fn field(&self, ty: &str, name: &str) -> Option<&Field> {
        self.types.get(ty)?.fields.iter().find(|f| f.name == name)
    }
}

/// What's offered, and as what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// A field of the type, its type and what it says it is.
    Field { name: String, ty: String, description: String },
    /// An argument of the field being called: `name: Type`.
    Arg { name: String, ty: String },
    /// A type a fragment may be on (`... on User`).
    Type { name: String, kind: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Name(String),
    Punct(char),
    Spread,
    /// A string, a number: a value.
    Value,
}

/// `text`'s tokens, strings and `#` comments skipped.
fn tokens(text: &str) -> Vec<Token> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() || ch == ',' {
            i += 1;
        } else if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '"' {
            let block = c[i..].starts_with(&['"', '"', '"']);
            i += if block { 3 } else { 1 };
            while i < c.len() {
                if block && c[i..].starts_with(&['"', '"', '"']) {
                    i += 3;
                    break;
                }
                if !block && c[i] == '\\' {
                    i += 2;
                    continue;
                }
                if !block && (c[i] == '"' || c[i] == '\n') {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Token::Value);
        } else if ch == '.' && c[i..].starts_with(&['.', '.', '.']) {
            out.push(Token::Spread);
            i += 3;
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(c[start..i].iter().collect()));
        } else if ch.is_ascii_digit() || ch == '-' {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || matches!(c[i], '.' | '-' | '+')) {
                i += 1;
            }
            out.push(Token::Value);
        } else {
            out.push(Token::Punct(ch));
            i += 1;
        }
    }
    out
}

#[derive(Debug, Clone)]
enum Frame {
    /// A selection set on a type (none when the schema doesn't know it).
    Select { ty: Option<String>, last: Option<String> },
    /// A field's arguments: the type it's on and its name; `depth` counts
    /// the brackets of the values inside.
    Args { ty: Option<String>, field: String, depth: u32 },
    /// Brackets to skip to their end: a query's variables, a directive's
    /// arguments.
    Skip(u32),
}

/// What's offered where `before` ends (the query up to the cursor), its
/// last word what's been typed of it.
pub fn offers(schema: &Schema, before: &str) -> Vec<Offer> {
    let toks = tokens(before);
    // A word being typed is what's replaced, not read.
    let typing = before.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_');
    let toks = if typing { &toks[..toks.len().saturating_sub(1)] } else { &toks[..] };
    let mut stack: Vec<Frame> = Vec::new();
    // At the top: the operation's root, or a fragment's type.
    let mut pending: Option<String> = None;
    let mut spread = false;
    // After `@`: 1; after its name, waiting for its arguments: 2.
    let mut directive = 0u8;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        match stack.last_mut() {
            Some(Frame::Skip(depth)) => match t {
                Token::Punct('(' | '[' | '{') => *depth += 1,
                Token::Punct(')' | ']' | '}') => {
                    *depth -= 1;
                    if *depth == 0 {
                        stack.pop();
                    }
                }
                _ => {}
            },
            Some(Frame::Args { depth, .. }) => match t {
                Token::Punct('(' | '[' | '{') => *depth += 1,
                Token::Punct(')' | ']' | '}') if *depth > 0 => *depth -= 1,
                Token::Punct(')') => {
                    stack.pop();
                }
                _ => {}
            },
            Some(Frame::Select { ty, last }) => match t {
                Token::Punct('{') => {
                    directive = 0;
                    let inner = if spread {
                        pending.take()
                    } else {
                        let field = last.clone().unwrap_or_default();
                        ty.as_deref().and_then(|ty| schema.field(ty, &field)).map(|f| f.base.clone())
                    };
                    spread = false;
                    stack.push(Frame::Select { ty: inner, last: None });
                }
                Token::Punct('}') => {
                    stack.pop();
                    spread = false;
                }
                Token::Punct('(') if directive == 2 => {
                    directive = 0;
                    stack.push(Frame::Skip(1));
                }
                Token::Punct('(') => {
                    let field = last.clone().unwrap_or_default();
                    let ty = ty.clone();
                    stack.push(Frame::Args { ty, field, depth: 0 });
                }
                Token::Punct('@') => directive = 1,
                Token::Punct(':') => *last = None,
                Token::Spread => {
                    spread = true;
                    pending = None;
                }
                Token::Name(n) if spread && n == "on" => {}
                Token::Name(n) if spread => {
                    if i > 0 && toks[i - 1] == Token::Name("on".into()) {
                        pending = Some(n.clone());
                    } else {
                        // A named fragment spread: no selection follows.
                        spread = false;
                    }
                }
                Token::Name(_) if directive == 1 => directive = 2,
                Token::Name(n) => {
                    directive = 0;
                    *last = Some(n.clone());
                }
                _ => {}
            },
            None => match t {
                Token::Name(n) if n == "query" => pending = schema.query.clone(),
                Token::Name(n) if n == "mutation" => pending = schema.mutation.clone(),
                Token::Name(n) if n == "subscription" => pending = schema.subscription.clone(),
                Token::Name(n) if n == "on" => {
                    if let Some(Token::Name(ty)) = toks.get(i + 1) {
                        pending = Some(ty.clone());
                        i += 1;
                    }
                }
                Token::Punct('(') => stack.push(Frame::Skip(1)),
                Token::Punct('{') => {
                    let ty = pending.take().or_else(|| schema.query.clone());
                    stack.push(Frame::Select { ty, last: None });
                }
                _ => {}
            },
        }
        i += 1;
    }
    match stack.last() {
        Some(Frame::Select { ty, .. }) if spread => {
            // A union's members, an interface's implementations, itself;
            // else the types a selection may be on, not the roots.
            let possible = ty.as_deref().and_then(|t| schema.types.get(t)).map(|t| t.possible.clone()).unwrap_or_default();
            let roots = [&schema.query, &schema.mutation, &schema.subscription];
            schema
                .types
                .iter()
                .filter(|(n, t)| {
                    if possible.is_empty() {
                        matches!(t.kind.as_str(), "OBJECT" | "INTERFACE" | "UNION") && !n.starts_with("__") && !roots.iter().any(|r| r.as_deref() == Some(n.as_str()))
                    } else {
                        possible.contains(n) || ty.as_deref() == Some(n.as_str())
                    }
                })
                .map(|(n, t)| Offer::Type { name: n.clone(), kind: t.kind.to_lowercase() })
                .collect()
        }
        Some(Frame::Select { ty: Some(ty), .. }) => {
            let mut out: Vec<Offer> = schema.types.get(ty).into_iter().flat_map(|t| &t.fields).map(|f| Offer::Field { name: f.name.clone(), ty: f.ty.clone(), description: f.description.clone() }).collect();
            out.push(Offer::Field { name: "__typename".into(), ty: "String!".into(), description: "the object's type".into() });
            out
        }
        Some(Frame::Args { ty: Some(ty), field, depth: 0 }) => schema.field(ty, field).into_iter().flat_map(|f| &f.args).map(|(n, t)| Offer::Arg { name: n.clone(), ty: t.clone() }).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A schema as a server answers the introspection query: a Query with
    /// `user(id: ID!): User` and `search(term: String, first: Int):
    /// [Result!]!`, a User with `id`, `name` and `friends: [User!]`, a
    /// Post, a union of them, and a Mutation.
    pub fn answer() -> Vec<u8> {
        let named = |kind: &str, name: &str| serde_json::json!({ "kind": kind, "name": name, "ofType": null });
        let non_null = |t: Value| serde_json::json!({ "kind": "NON_NULL", "name": null, "ofType": t });
        let list = |t: Value| serde_json::json!({ "kind": "LIST", "name": null, "ofType": t });
        let field = |name: &str, ty: Value, args: Value| serde_json::json!({ "name": name, "description": format!("the {name}"), "args": args, "type": ty });
        serde_json::json!({ "data": { "__schema": {
            "queryType": { "name": "Query" }, "mutationType": { "name": "Mutation" }, "subscriptionType": null,
            "types": [
                { "kind": "OBJECT", "name": "Query", "fields": [
                    field("user", named("OBJECT", "User"), serde_json::json!([{ "name": "id", "type": non_null(named("SCALAR", "ID")) }])),
                    field("search", non_null(list(non_null(named("UNION", "Result")))), serde_json::json!([{ "name": "term", "type": named("SCALAR", "String") }, { "name": "first", "type": named("SCALAR", "Int") }])),
                ] },
                { "kind": "OBJECT", "name": "User", "fields": [
                    field("id", non_null(named("SCALAR", "ID")), serde_json::json!([])),
                    field("name", named("SCALAR", "String"), serde_json::json!([])),
                    field("friends", list(non_null(named("OBJECT", "User"))), serde_json::json!([{ "name": "first", "type": named("SCALAR", "Int") }])),
                ] },
                { "kind": "OBJECT", "name": "Post", "fields": [field("title", named("SCALAR", "String"), serde_json::json!([]))] },
                { "kind": "UNION", "name": "Result", "fields": null, "possibleTypes": [{ "name": "User" }, { "name": "Post" }] },
                { "kind": "OBJECT", "name": "Mutation", "fields": [field("rename", named("OBJECT", "User"), serde_json::json!([{ "name": "name", "type": non_null(named("SCALAR", "String")) }]))] },
                { "kind": "SCALAR", "name": "String" },
                { "kind": "OBJECT", "name": "__Type", "fields": [] },
            ]
        } } })
        .to_string()
        .into_bytes()
    }

    fn names(o: &[Offer]) -> Vec<String> {
        o.iter()
            .map(|o| match o {
                Offer::Field { name, .. } | Offer::Arg { name, .. } | Offer::Type { name, .. } => name.clone(),
            })
            .collect()
    }

    #[test]
    fn the_schema_is_read_from_the_introspection_answer() {
        let s = Schema::from_introspection(&answer()).unwrap();
        assert_eq!((s.query.as_deref(), s.mutation.as_deref(), s.subscription.as_deref()), (Some("Query"), Some("Mutation"), None));
        let search = s.field("Query", "search").unwrap();
        assert_eq!((search.ty.as_str(), search.base.as_str()), ("[Result!]!", "Result"));
        assert_eq!(search.args, [("term".to_string(), "String".to_string()), ("first".to_string(), "Int".to_string())]);
        assert_eq!(s.field("User", "friends").unwrap().ty, "[User!]");
        assert!(Schema::from_introspection(b"{\"errors\":[{\"message\":\"introspection is off\"}]}").unwrap_err().contains("introspection is off"));
        assert!(Schema::from_introspection(b"<html>").unwrap_err().contains("JSON"));
        assert!(Schema::from_introspection(b"{\"data\":{}}").unwrap_err().contains("__schema"));
        assert!(String::from_utf8(introspection_body()).unwrap().contains("\"operationName\":\"IntrospectionQuery\""));
    }

    #[test]
    fn fields_of_the_type_the_cursor_is_in() {
        let s = Schema::from_introspection(&answer()).unwrap();
        assert_eq!(names(&offers(&s, "{ ")), ["user", "search", "__typename"]);
        assert_eq!(names(&offers(&s, "query Me {\n  us")), ["user", "search", "__typename"]);
        assert_eq!(names(&offers(&s, "query Me($id: ID!) {\n  user(id: $id) {\n    ")), ["id", "name", "friends", "__typename"]);
        assert_eq!(names(&offers(&s, "{ user(id: \"{ not this }\") { friends(first: 2) { na")), ["id", "name", "friends", "__typename"]);
        assert_eq!(names(&offers(&s, "{ user(id: 1) { id }\n  ")), ["user", "search", "__typename"], "back out of user");
        assert_eq!(names(&offers(&s, "mutation { ")), ["rename", "__typename"]);
        assert_eq!(names(&offers(&s, "{ me: user(id: 1) { ")), ["id", "name", "friends", "__typename"], "an alias");
        assert_eq!(names(&offers(&s, "{ user(id: 1) @include(if: true) { ")), ["id", "name", "friends", "__typename"], "a directive");
        assert_eq!(names(&offers(&s, "fragment F on User { ")), ["id", "name", "friends", "__typename"]);
        assert!(offers(&s, "{ nope { ").is_empty(), "a field the schema doesn't have");
        assert!(offers(&s, "").is_empty());
    }

    #[test]
    fn arguments_and_fragments() {
        let s = Schema::from_introspection(&answer()).unwrap();
        assert_eq!(names(&offers(&s, "{ search(")), ["term", "first"]);
        assert_eq!(names(&offers(&s, "{ search(term: \"x\", fi")), ["term", "first"]);
        assert!(offers(&s, "{ search(term: [").is_empty(), "inside a value");
        assert_eq!(names(&offers(&s, "{ search { ... on ")), ["Post", "Result", "User"]);
        assert_eq!(names(&offers(&s, "{ search { ... on Post { ")), ["title", "__typename"]);
        assert_eq!(names(&offers(&s, "{ user(id: 1) { ... on ")), ["Post", "Result", "User"], "no union here: any but the roots");
        assert_eq!(names(&offers(&s, "{ search { ...F\n ")), ["__typename"], "a union has no fields of its own");
    }
}
