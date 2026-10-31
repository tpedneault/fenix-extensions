//! A request's body, built from what the `.http` file says: text with its
//! variables filled in; a line `< ./file` is that file's bytes (`<@
//! ./file`, its variables filled in too) -- a binary body from a file, or
//! a part of a multipart one; a form's lines joined; a GraphQL query and
//! its variables made the JSON a GraphQL server takes.

/// What a body comes to: its bytes, and a header it sets
/// (`Content-Type` for GraphQL).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

/// A line that's a file: its path and whether its variables are filled
/// in.
pub fn include(line: &str) -> Option<(&str, bool)> {
    let rest = line.trim_start().strip_prefix('<')?;
    let (fill, rest) = match rest.strip_prefix('@') {
        Some(r) if r.starts_with(char::is_whitespace) => (true, r),
        // `<@latin1 ./x`, an encoding named: read as UTF-8 all the same.
        Some(r) => (true, r.split_once(char::is_whitespace)?.1),
        None if rest.starts_with(char::is_whitespace) => (false, rest),
        None => return None,
    };
    let path = rest.trim();
    (!path.is_empty()).then_some((path, fill))
}

/// The body for `text`, under content type `content_type`: `fill` fills
/// a text's variables in, `read` reads a file it names.
pub fn build(content_type: &str, graphql: bool, text: &str, fill: &mut dyn FnMut(&str) -> Result<String, String>, read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>) -> Result<Built, String> {
    let ct = content_type.to_ascii_lowercase();
    if graphql {
        return graphql_body(text, fill);
    }
    let lines: Vec<&str> = text.split('\n').collect();
    // A body that's a file, whole.
    if let [only] = lines.as_slice() {
        if let Some((path, fill_it)) = include(only) {
            let bytes = read(path)?;
            let bytes = if fill_it { fill(&String::from_utf8_lossy(&bytes))?.into_bytes() } else { bytes };
            return Ok(Built { bytes, content_type: None });
        }
    }
    if ct.starts_with("multipart/") {
        let mut out: Vec<u8> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            match include(line) {
                Some((path, fill_it)) => {
                    let bytes = read(path)?;
                    out.extend(if fill_it { fill(&String::from_utf8_lossy(&bytes))?.into_bytes() } else { bytes });
                }
                None => out.extend(fill(line)?.into_bytes()),
            }
            if i + 1 < lines.len() {
                out.extend(b"\r\n");
            }
        }
        // The closing boundary ends with a line break, as servers expect.
        if !out.ends_with(b"\r\n") {
            out.extend(b"\r\n");
        }
        return Ok(Built { bytes: out, content_type: None });
    }
    if ct.starts_with("application/x-www-form-urlencoded") {
        let joined: String = lines.iter().map(|l| l.trim()).collect();
        return Ok(Built { bytes: fill(&joined)?.into_bytes(), content_type: None });
    }
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        match include(line) {
            Some((path, fill_it)) => {
                let bytes = read(path)?;
                let t = String::from_utf8_lossy(&bytes).into_owned();
                out.push_str(&if fill_it { fill(&t)? } else { t });
            }
            None => out.push_str(&fill(line)?),
        }
        if i + 1 < lines.len() {
            out.push('\n');
        }
    }
    Ok(Built { bytes: out.into_bytes(), content_type: None })
}

/// REST Client's GraphQL: the query, a blank line, its variables as JSON.
fn graphql_body(text: &str, fill: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<Built, String> {
    let text = fill(text)?;
    let (query, variables) = split_graphql(&text);
    let mut obj = serde_json::Map::new();
    obj.insert("query".into(), serde_json::Value::String(query.trim().to_string()));
    if let Some(v) = variables {
        let v: serde_json::Value = serde_json::from_str(v).map_err(|e| format!("the GraphQL variables aren't JSON: {e}"))?;
        obj.insert("variables".into(), v);
    }
    if let Some(name) = operation_name(query) {
        obj.insert("operationName".into(), serde_json::Value::String(name));
    }
    Ok(Built { bytes: serde_json::Value::Object(obj).to_string().into_bytes(), content_type: Some("application/json".into()) })
}

/// A GraphQL body's query and its variables: after the first blank line
/// that's followed by `{` once the query's braces are closed.
pub fn split_graphql(text: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if depth <= 0 && line.trim().is_empty() && offset > 0 {
            let rest = text[offset + line.len()..].trim();
            if rest.starts_with('{') {
                return (&text[..offset], Some(rest));
            }
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        offset += line.len();
    }
    (text, None)
}

fn operation_name(query: &str) -> Option<String> {
    let t = query.trim_start();
    let rest = ["query", "mutation", "subscription"].iter().find_map(|k| t.strip_prefix(k))?;
    let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(s: &str) -> Result<String, String> {
        Ok(s.replace("{{name}}", "ada"))
    }

    fn read(path: &str) -> Result<Vec<u8>, String> {
        match path {
            "./logo.png" => Ok(vec![0x89, b'P', b'N', b'G']),
            "./data.json" => Ok(b"{\"who\": \"{{name}}\"}".to_vec()),
            p => Err(format!("{p}: no such file")),
        }
    }

    #[test]
    fn a_text_body_has_its_variables_filled_in() {
        let b = build("application/json", false, "{\"user\": \"{{name}}\"}", &mut fill, &mut read).unwrap();
        assert_eq!(b.bytes, b"{\"user\": \"ada\"}");
    }

    #[test]
    fn a_binary_body_comes_from_a_file() {
        let b = build("image/png", false, "< ./logo.png", &mut fill, &mut read).unwrap();
        assert_eq!(b.bytes, [0x89, b'P', b'N', b'G']);
        let b = build("application/json", false, "< ./data.json", &mut fill, &mut read).unwrap();
        assert_eq!(b.bytes, b"{\"who\": \"{{name}}\"}", "as it is");
        let b = build("application/json", false, "<@ ./data.json", &mut fill, &mut read).unwrap();
        assert_eq!(b.bytes, b"{\"who\": \"ada\"}", "its variables filled in");
        assert!(build("", false, "< ./nope", &mut fill, &mut read).unwrap_err().contains("no such file"));
        assert_eq!(include("<not a file>"), None);
        assert_eq!(include("<@latin1 ./x.txt"), Some(("./x.txt", true)));
    }

    #[test]
    fn a_multipart_body_takes_files_and_crlf() {
        let text = "--B\nContent-Disposition: form-data; name=\"who\"\n\n{{name}}\n--B\nContent-Disposition: form-data; name=\"logo\"; filename=\"logo.png\"\nContent-Type: image/png\n\n< ./logo.png\n--B--";
        let b = build("multipart/form-data; boundary=B", false, text, &mut fill, &mut read).unwrap();
        let mut want = b"--B\r\nContent-Disposition: form-data; name=\"who\"\r\n\r\nada\r\n--B\r\nContent-Disposition: form-data; name=\"logo\"; filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
        want.extend([0x89, b'P', b'N', b'G']);
        want.extend(b"\r\n--B--\r\n");
        assert_eq!(b.bytes, want);
    }

    #[test]
    fn a_form_is_one_line() {
        let b = build("application/x-www-form-urlencoded", false, "name={{name}}\n&role=admin", &mut fill, &mut read).unwrap();
        assert_eq!(b.bytes, b"name=ada&role=admin");
    }

    #[test]
    fn graphql_is_made_json() {
        let text = "query Get($id: ID!) {\n  user(id: $id) {\n    name\n  }\n}\n\n{\"id\": \"{{name}}\"}";
        let b = build("application/json", true, text, &mut fill, &mut read).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b.bytes).unwrap();
        assert_eq!(v["variables"]["id"], "ada");
        assert_eq!(v["operationName"], "Get");
        assert!(v["query"].as_str().unwrap().ends_with("}\n}"));
        assert_eq!(b.content_type.as_deref(), Some("application/json"));
        let b = build("", true, "{ me { id } }", &mut fill, &mut read).unwrap();
        assert_eq!(String::from_utf8(b.bytes).unwrap(), r#"{"query":"{ me { id } }"}"#);
    }
}
