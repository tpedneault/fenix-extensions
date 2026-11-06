//! A request body to start from: a method's input message as JSON, as
//! protojson reads it -- each field by its JSON name with its zero value,
//! an enum's first value, a list of one, a map of one, a oneof's first
//! choice, the well-known types as they're written (a `Timestamp` a
//! string, a `Duration` `"0s"`), 64-bit integers as strings. A message met
//! again inside itself is `null`, and so is one not described.

use serde_json::{json, Map, Value};

use super::describe::{Described, Field, Label};

/// How deep messages in messages are filled in.
pub const DEPTH: usize = 6;

/// A scalar's zero value; `None` for a type that isn't one.
fn scalar(ty: &str) -> Option<Value> {
    Some(match ty {
        "string" | "bytes" => json!(""),
        "bool" => json!(false),
        "int32" | "uint32" | "sint32" | "fixed32" | "sfixed32" | "float" | "double" => json!(0),
        "int64" | "uint64" | "sint64" | "fixed64" | "sfixed64" => json!("0"),
        _ => return None,
    })
}

/// A well-known type, as protojson writes it.
fn well_known(ty: &str) -> Option<Value> {
    let name = ty.strip_prefix("google.protobuf.")?;
    Some(match name {
        "Timestamp" => json!("1970-01-01T00:00:00Z"),
        "Duration" => json!("0s"),
        "FieldMask" => json!(""),
        "Struct" | "Empty" => json!({}),
        "ListValue" => json!([]),
        "Value" | "NullValue" => Value::Null,
        "Any" => json!({"@type": ""}),
        "DoubleValue" | "FloatValue" | "Int32Value" | "UInt32Value" => json!(0),
        "Int64Value" | "UInt64Value" => json!("0"),
        "BoolValue" => json!(false),
        "StringValue" | "BytesValue" => json!(""),
        _ => return None,
    })
}

/// The types a message refers to, through its fields and theirs, that
/// `d` hasn't described yet: what to `describe` next. Well-known types are
/// known without.
pub fn wanted(d: &Described, message: &str) -> Vec<String> {
    fn walk(d: &Described, name: &str, seen: &mut Vec<String>, out: &mut Vec<String>) {
        if seen.iter().any(|s| s == name) {
            return;
        }
        seen.push(name.to_string());
        let Some(m) = d.messages.get(name) else { return };
        for f in &m.fields {
            let ty = f.ty.as_str();
            if scalar(ty).is_some() || well_known(ty).is_some() || d.enums.contains_key(ty) {
                continue;
            }
            if d.messages.contains_key(ty) {
                walk(d, ty, seen, out);
            } else if !out.iter().any(|o| o == ty) && !d.missing.iter().any(|o| o == ty) {
                out.push(ty.to_string());
            }
        }
    }
    let mut out = Vec::new();
    if !d.messages.contains_key(message) && well_known(message).is_none() && !d.missing.iter().any(|m| m == message) {
        return vec![message.to_string()];
    }
    walk(d, message, &mut Vec::new(), &mut out);
    out
}

/// One value of `ty`, `path` the messages it's inside.
fn value(d: &Described, ty: &str, path: &mut Vec<String>) -> Value {
    if let Some(v) = scalar(ty).or_else(|| well_known(ty)) {
        return v;
    }
    if let Some(e) = d.enums.get(ty) {
        return e.values.first().map(|v| json!(v)).unwrap_or(json!(0));
    }
    match d.messages.get(ty) {
        Some(_) if path.iter().any(|p| p == ty) || path.len() >= DEPTH => Value::Null,
        Some(m) => {
            path.push(ty.to_string());
            let mut out = Map::new();
            let mut oneofs: Vec<&str> = Vec::new();
            for f in &m.fields {
                if let Some(o) = f.oneof.as_deref() {
                    if oneofs.contains(&o) {
                        continue;
                    }
                    oneofs.push(o);
                }
                out.insert(f.json(), field(d, f, path));
            }
            path.pop();
            Value::Object(out)
        }
        None => Value::Null,
    }
}

fn field(d: &Described, f: &Field, path: &mut Vec<String>) -> Value {
    let one = value(d, &f.ty, path);
    match (&f.key, f.label) {
        (Some(key), _) => {
            let k = match key.as_str() {
                "string" => String::new(),
                "bool" => "false".into(),
                _ => "0".into(),
            };
            let mut m = Map::new();
            m.insert(k, one);
            Value::Object(m)
        }
        (None, Label::Repeated) => Value::Array(if one.is_null() { Vec::new() } else { vec![one] }),
        _ => one,
    }
}

/// `message` as a body to edit: pretty JSON, two spaces as grpcurl prints
/// it, ending in a newline.
pub fn skeleton(d: &Described, message: &str) -> String {
    let v = value(d, message, &mut Vec::new());
    let v = if v.is_null() { json!({}) } else { v };
    let mut text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::describe::parse_describe;

    #[test]
    fn a_request_is_its_fields_zero_values() {
        let d = parse_describe(include_str!("../samples/grpcurl-describe.txt"));
        assert_eq!(skeleton(&d, "orbit.v1.GetSatelliteRequest"), "{\n  \"id\": \"\"\n}\n");
        let body: Value = serde_json::from_str(&skeleton(&d, "orbit.v1.WatchPassesRequest")).unwrap();
        assert_eq!(body, json!({"satelliteId": "", "station": {"lat": 0, "lon": 0}}), "a message in it filled in");
        let sat: Value = serde_json::from_str(&skeleton(&d, "orbit.v1.Satellite")).unwrap();
        assert_eq!(
            sat,
            json!({"id": "", "name": "", "periodMinutes": 0, "kind": "KIND_UNSPECIFIED", "tags": [""], "labels": {"": ""}, "launched": "1970-01-01T00:00:00Z", "agency": ""}),
            "an enum's first value, a list and a map of one, a Timestamp's string, a oneof's first choice"
        );
        assert_eq!(skeleton(&d, "orbit.v1.Nowhere"), "{}\n");
    }

    #[test]
    fn nested_and_well_known_types_and_a_message_in_itself() {
        let d = parse_describe(include_str!("../samples/grpcurl-describe-nested.txt"));
        let order: Value = serde_json::from_str(&skeleton(&d, "acme.v1.Order")).unwrap();
        assert_eq!(order["orderId"], "", "json_name");
        assert_eq!(order["lines"], json!([{"sku": "", "quantity": 0}]));
        assert_eq!((order["state"].as_str(), order["note"].as_str()), (Some("STATE_UNSPECIFIED"), Some("")));
        assert_eq!((order["ttl"].as_str(), order["coupon"].as_str(), &order["extra"]), (Some("0s"), Some(""), &json!({})));
        assert_eq!(order["bySku"], json!({"": {"sku": "", "quantity": 0}}));
        assert_eq!((order["totalCents"].as_str(), order["paid"].as_bool()), (Some("0"), Some(false)), "int64 as a string");
        assert_eq!(order["parent"], Value::Null, "Order in Order");
        let keys: Vec<&String> = order.as_object().unwrap().keys().collect();
        assert_eq!(keys.first().map(|k| k.as_str()), Some("orderId"), "in the fields' order");
    }

    #[test]
    fn what_to_describe_next() {
        let d = parse_describe(include_str!("../samples/grpcurl-describe-service.txt"));
        assert_eq!(wanted(&d, "orbit.v1.GetSatelliteResponse"), ["orbit.v1.GetSatelliteResponse"], "the message itself first");
        let mut d = parse_describe("orbit.v1.GetSatelliteResponse is a message:\nmessage GetSatelliteResponse {\n  .orbit.v1.Satellite satellite = 1;\n}\n");
        assert_eq!(wanted(&d, "orbit.v1.GetSatelliteResponse"), ["orbit.v1.Satellite"]);
        d.merge(parse_describe(include_str!("../samples/grpcurl-describe.txt")));
        assert!(wanted(&d, "orbit.v1.GetSatelliteResponse").is_empty(), "Kind and Timestamp known");
        let n = parse_describe(include_str!("../samples/grpcurl-describe-nested.txt"));
        assert!(wanted(&n, "acme.v1.Order").is_empty(), "nested types come with it");
        let gone = parse_describe("Failed to resolve symbol \"x.Y\": Symbol not found: x.Y\n");
        assert!(wanted(&gone, "x.Y").is_empty(), "not asked again");
    }
}
