//! What grpcurl says of a server's or a project's schema: `list`, a
//! service a line, and `describe`, each symbol as Protobuf source after a
//! line saying what it is (`orbit.v1.Satellite is a message:`). Messages
//! come with the types nested in them; several `describe`s' outputs read
//! as one.

use std::collections::BTreeMap;

/// A field's cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    Single,
    Optional,
    Repeated,
}

/// A message's field, as `describe` writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// `[json_name = "orderId"]`, when it says one.
    pub json_name: Option<String>,
    /// A scalar (`string`, `int64`), or a type's full name without its
    /// leading dot (`orbit.v1.Kind`).
    pub ty: String,
    pub label: Label,
    /// A map's key type; `ty` is then its value's.
    pub key: Option<String>,
    /// The oneof it's in.
    pub oneof: Option<String>,
    pub number: u32,
}

impl Field {
    /// Its name in JSON: `json_name`, else lowerCamelCase, as protojson
    /// writes it.
    pub fn json(&self) -> String {
        if let Some(j) = &self.json_name {
            return j.clone();
        }
        let mut out = String::new();
        let mut up = false;
        for c in self.name.chars() {
            if c == '_' {
                up = true;
            } else if up {
                out.extend(c.to_uppercase());
                up = false;
            } else {
                out.push(c);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Enum {
    pub name: String,
    pub values: Vec<String>,
}

/// How a method's messages go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streaming {
    Unary,
    Server,
    Client,
    Both,
}

impl Streaming {
    /// A word for it, as the client shows it.
    pub fn word(self) -> &'static str {
        match self {
            Streaming::Unary => "unary",
            Streaming::Server => "server stream",
            Streaming::Client => "client stream",
            Streaming::Both => "bidi stream",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub input: String,
    pub output: String,
    pub streaming: Streaming,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub methods: Vec<Method>,
}

impl Service {
    /// A method's full name as grpcurl takes it: `orbit.v1.OrbitService/GetSatellite`.
    pub fn path(&self, method: &str) -> String {
        format!("{}/{method}", self.name)
    }
}

/// What one or more `describe`s said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Described {
    pub messages: BTreeMap<String, Message>,
    pub enums: BTreeMap<String, Enum>,
    pub services: Vec<Service>,
    /// Symbols it couldn't find (`Symbol not found: x`).
    pub missing: Vec<String>,
}

impl Described {
    /// Another's, added: what it said of a symbol wins.
    pub fn merge(&mut self, other: Described) {
        self.messages.extend(other.messages);
        self.enums.extend(other.enums);
        for s in other.services {
            self.services.retain(|o| o.name != s.name);
            self.services.push(s);
        }
        self.missing.extend(other.missing);
    }

    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }
}

/// What `list` printed: the services, sorted. The reflection service
/// itself is left out: it's how grpcurl asks, not what you'd call.
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains(' ') && !l.starts_with("grpc.reflection.")).map(str::to_string).collect();
    out.sort();
    out.dedup();
    out
}

enum Scope {
    Message(String),
    Enum(String),
    Oneof(String),
    Service(String),
    /// A block it doesn't read: an rpc's options, an option's value.
    Other,
}

fn strip_dot(ty: &str) -> String {
    ty.trim().trim_start_matches('.').to_string()
}

/// `name = 3 [json_name = "x", deprecated = true];` read: the name, its
/// number, and the `json_name` it says.
fn assignment(text: &str) -> Option<(String, u32, Option<String>)> {
    let (name, rest) = text.split_once('=')?;
    let rest = rest.trim().trim_end_matches(';').trim();
    let (number, options) = match rest.split_once('[') {
        Some((n, o)) => (n.trim(), Some(o)),
        None => (rest, None),
    };
    let json_name = options.and_then(|o| {
        let at = o.find("json_name")?;
        let after = o[at..].split_once('"')?.1;
        Some(after.split('"').next()?.to_string())
    });
    // Enum values may be negative; fields' numbers aren't.
    let number = number.trim_start_matches('-').parse().ok()?;
    Some((name.trim().to_string(), number, json_name))
}

/// A field's line, in the oneof `oneof` when it's in one.
fn field(line: &str, oneof: Option<&str>) -> Option<Field> {
    let line = line.trim();
    let (label, rest) = match line.split_once(' ') {
        Some(("repeated", r)) => (Label::Repeated, r),
        Some(("optional", r)) => (Label::Optional, r),
        Some(("required", r)) => (Label::Single, r),
        _ => (Label::Single, line),
    };
    let (key, ty, rest) = if let Some(m) = rest.strip_prefix("map<") {
        let (types, rest) = m.split_once('>')?;
        let (k, v) = types.split_once(',')?;
        (Some(strip_dot(k)), strip_dot(v), rest)
    } else {
        let (ty, rest) = rest.split_once(' ')?;
        (None, strip_dot(ty), rest)
    };
    let (name, number, json_name) = assignment(rest)?;
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    Some(Field { name, json_name, ty, label: if key.is_some() { Label::Repeated } else { label }, key, oneof: oneof.map(str::to_string), number })
}

/// `rpc Get ( .a.Req ) returns ( stream .a.Res );` read.
fn rpc(line: &str) -> Option<Method> {
    let rest = line.trim().strip_prefix("rpc ")?;
    let (name, rest) = rest.split_once('(')?;
    let (input, rest) = rest.split_once(')')?;
    let rest = rest.trim().strip_prefix("returns")?.trim().strip_prefix('(')?;
    let (output, _) = rest.split_once(')')?;
    let streamed = |t: &str| t.trim().strip_prefix("stream ").map(|t| (true, strip_dot(t))).unwrap_or((false, strip_dot(t)));
    let ((cs, input), (ss, output)) = (streamed(input), streamed(output));
    let streaming = match (cs, ss) {
        (false, false) => Streaming::Unary,
        (false, true) => Streaming::Server,
        (true, false) => Streaming::Client,
        (true, true) => Streaming::Both,
    };
    Some(Method { name: name.trim().to_string(), input, output, streaming })
}

/// What `describe` printed, one symbol or several.
pub fn parse_describe(text: &str) -> Described {
    let mut d = Described::default();
    let mut stack: Vec<Scope> = Vec::new();
    // The symbol the block that's next is, by its header line.
    let mut header: Option<String> = None;
    for raw in text.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if raw.trim_start().starts_with("//") || line.is_empty() {
            continue;
        }
        if let Some(rest) = raw.find("Symbol not found: ").map(|i| &raw[i + "Symbol not found: ".len()..]) {
            d.missing.push(rest.trim().to_string());
            continue;
        }
        if stack.is_empty() {
            if let Some((symbol, _)) = line.split_once(" is a").filter(|_| line.ends_with(':')) {
                header = Some(strip_dot(symbol));
                continue;
            }
        }
        // The full name of what's opened here: the header's at the top,
        // else in the enclosing message.
        let full = |name: &str, stack: &[Scope], header: &Option<String>| -> String {
            let parent = stack.iter().rev().find_map(|s| match s {
                Scope::Message(m) => Some(m.clone()),
                _ => None,
            });
            match parent {
                Some(p) => format!("{p}.{name}"),
                None => header.clone().unwrap_or_else(|| name.to_string()),
            }
        };
        if line == "}" {
            stack.pop();
            continue;
        }
        if let Some(name) = line.strip_prefix("message ").and_then(|r| r.strip_suffix('{')) {
            let name = full(name.trim(), &stack, &header);
            d.messages.entry(name.clone()).or_insert_with(|| Message { name: name.clone(), fields: Vec::new() }).fields.clear();
            stack.push(Scope::Message(name));
            continue;
        }
        if let Some(name) = line.strip_prefix("enum ").and_then(|r| r.strip_suffix('{')) {
            let name = full(name.trim(), &stack, &header);
            d.enums.insert(name.clone(), Enum { name: name.clone(), values: Vec::new() });
            stack.push(Scope::Enum(name));
            continue;
        }
        if let Some(name) = line.strip_prefix("service ").and_then(|r| r.strip_suffix('{')) {
            let name = header.clone().unwrap_or_else(|| name.trim().to_string());
            d.services.retain(|s| s.name != name);
            d.services.push(Service { name: name.clone(), methods: Vec::new() });
            stack.push(Scope::Service(name));
            continue;
        }
        if let Some(name) = line.strip_prefix("oneof ").and_then(|r| r.strip_suffix('{')) {
            stack.push(Scope::Oneof(name.trim().to_string()));
            continue;
        }
        // A method on its own (`describe pkg.Svc.Method`), or in a service.
        if line.starts_with("rpc ") {
            if let Some(m) = rpc(line) {
                let in_service = matches!(stack.last(), Some(Scope::Service(_)));
                match stack.iter().rev().find_map(|s| if let Scope::Service(n) = s { Some(n.clone()) } else { None }) {
                    Some(svc) if in_service => {
                        if let Some(s) = d.services.iter_mut().find(|s| s.name == svc) {
                            s.methods.push(m);
                        }
                    }
                    _ => {
                        // `pkg.Svc.Method is a method:`: its service is the
                        // header's but its last part.
                        if let Some(svc) = header.as_deref().and_then(|h| h.rsplit_once('.')).map(|(s, _)| s.to_string()) {
                            match d.services.iter_mut().find(|s| s.name == svc) {
                                Some(s) => s.methods.push(m),
                                None => d.services.push(Service { name: svc, methods: vec![m] }),
                            }
                        }
                    }
                }
            }
            if line.ends_with('{') {
                stack.push(Scope::Other);
            }
            continue;
        }
        if line.ends_with('{') {
            stack.push(Scope::Other);
            continue;
        }
        if ["option ", "reserved ", "extensions ", "extend "].iter().any(|k| line.starts_with(k)) {
            continue;
        }
        match stack.last() {
            Some(Scope::Enum(e)) => {
                if let Some((name, _, _)) = assignment(line) {
                    if let Some(en) = d.enums.get_mut(e) {
                        en.values.push(name);
                    }
                }
            }
            Some(Scope::Message(m)) => {
                let m = m.clone();
                if let (Some(f), Some(msg)) = (field(line, None), d.messages.get_mut(&m)) {
                    msg.fields.push(f);
                }
            }
            Some(Scope::Oneof(o)) => {
                let o = o.clone();
                let parent = stack.iter().rev().find_map(|s| if let Scope::Message(m) = s { Some(m.clone()) } else { None });
                if let Some(m) = parent {
                    if let (Some(f), Some(msg)) = (field(line, Some(&o)), d.messages.get_mut(&m)) {
                        msg.fields.push(f);
                    }
                }
            }
            _ => {}
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_gives_the_services_but_reflection() {
        assert_eq!(parse_list(include_str!("../samples/grpcurl-list.txt")), ["orbit.v1.OrbitService"]);
        assert_eq!(parse_list("b.Svc\na.Svc\n\nFailed to list services: server does not support the reflection API\n"), ["a.Svc", "b.Svc"], "an error's words aren't a service");
    }

    #[test]
    fn a_service_has_its_methods_and_how_they_stream() {
        let d = parse_describe(include_str!("../samples/grpcurl-describe-service.txt"));
        let s = d.service("orbit.v1.OrbitService").unwrap();
        assert_eq!(s.methods[0], Method { name: "GetSatellite".into(), input: "orbit.v1.GetSatelliteRequest".into(), output: "orbit.v1.GetSatelliteResponse".into(), streaming: Streaming::Unary });
        assert_eq!((s.methods[1].name.as_str(), s.methods[1].output.as_str(), s.methods[1].streaming), ("WatchPasses", "orbit.v1.Pass", Streaming::Server));
        assert_eq!(s.path("WatchPasses"), "orbit.v1.OrbitService/WatchPasses");

        let nested = parse_describe(include_str!("../samples/grpcurl-describe-nested.txt"));
        let orders = nested.service("acme.v1.Orders").unwrap();
        let kinds: Vec<(&str, Streaming)> = orders.methods.iter().map(|m| (m.name.as_str(), m.streaming)).collect();
        assert_eq!(kinds, [("Chat", Streaming::Both), ("Place", Streaming::Unary), ("Upload", Streaming::Client)], "an rpc's options block is read past");
        assert_eq!(orders.methods[2].input, "acme.v1.Order.Line");

        let one = parse_describe("orbit.v1.OrbitService.WatchPasses is a method:\n// Its passes.\nrpc WatchPasses ( .orbit.v1.WatchPassesRequest ) returns ( stream .orbit.v1.Pass );\n");
        assert_eq!(one.service("orbit.v1.OrbitService").unwrap().methods[0].streaming, Streaming::Server, "a method described on its own");
    }

    #[test]
    fn messages_have_their_fields_nested_types_and_oneofs() {
        let d = parse_describe(include_str!("../samples/grpcurl-describe.txt"));
        let s = &d.messages["orbit.v1.Satellite"];
        type Row<'a> = (&'a str, &'a str, Label, Option<&'a str>, Option<&'a str>);
        let rows: Vec<Row> = s.fields.iter().map(|f| (f.name.as_str(), f.ty.as_str(), f.label, f.key.as_deref(), f.oneof.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("id", "string", Label::Single, None, None),
                ("name", "string", Label::Single, None, None),
                ("period_minutes", "double", Label::Single, None, None),
                ("kind", "orbit.v1.Kind", Label::Single, None, None),
                ("tags", "string", Label::Repeated, None, None),
                ("labels", "string", Label::Repeated, Some("string"), None),
                ("launched", "google.protobuf.Timestamp", Label::Single, None, None),
                ("agency", "string", Label::Single, None, Some("owner")),
                ("company", "string", Label::Single, None, Some("owner")),
            ]
        );
        assert_eq!(d.enums["orbit.v1.Kind"].values, ["KIND_UNSPECIFIED", "KIND_WEATHER", "KIND_COMMS"]);
        assert_eq!(d.messages["orbit.v1.WatchPassesRequest"].fields[1].ty, "orbit.v1.Station");
        let gone = parse_describe("Failed to resolve symbol \"orbit.v1.OrbitService.WatchPasses\": Symbol not found: orbit.v1.OrbitService.WatchPasses\n");
        assert_eq!(gone.missing, ["orbit.v1.OrbitService.WatchPasses"], "reflection doesn't find a method by its own name");
        assert_eq!(s.fields[2].json(), "periodMinutes");

        let n = parse_describe(include_str!("../samples/grpcurl-describe-nested.txt"));
        let order = &n.messages["acme.v1.Order"];
        assert_eq!(order.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["id", "lines", "state", "note", "blob", "ttl", "coupon", "extra", "by_sku", "total_cents", "paid", "parent"], "reserved left out, nested types' fields their own");
        assert_eq!((order.fields[0].json(), order.fields[3].label), ("orderId".to_string(), Label::Optional));
        assert_eq!((order.fields[8].key.as_deref(), order.fields[8].ty.as_str()), (Some("string"), "acme.v1.Order.Line"));
        assert_eq!(order.fields[9].number, 12);
        assert_eq!(n.messages["acme.v1.Order.Line"].fields.len(), 2);
        assert_eq!(n.enums["acme.v1.Order.State"].values, ["STATE_UNSPECIFIED", "STATE_OPEN"]);
    }
}
