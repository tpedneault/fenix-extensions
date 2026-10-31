//! Paths into a document: the key under the cursor as `.servers[2].port`
//! (or JSONPath, or a JSON Pointer), and where a JSON Pointer a schema's
//! error names is.

use crate::node::{Node, Pos, Value};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seg {
    Key(String),
    Index(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// jq's: `.servers[2].port`, `.["a b"]`.
    Jq,
    /// `$.servers[2].port`, `$['a b']`.
    JsonPath,
    /// `/servers/2/port`.
    Pointer,
}

impl Style {
    pub fn from_setting(s: &str) -> Style {
        match s.trim() {
            "jsonpath" => Style::JsonPath,
            "pointer" => Style::Pointer,
            _ => Style::Jq,
        }
    }
}

fn identifier(k: &str) -> bool {
    let mut chars = k.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn render(path: &[Seg], style: Style) -> String {
    let mut out = String::new();
    match style {
        Style::Jq => {
            for seg in path {
                match seg {
                    Seg::Key(k) if identifier(k) => out.push_str(&format!(".{k}")),
                    Seg::Key(k) => out.push_str(&format!(".[{}]", crate::json::quote(k))),
                    Seg::Index(i) => out.push_str(&format!("[{i}]")),
                }
            }
            if out.is_empty() {
                out.push('.');
            }
        }
        Style::JsonPath => {
            out.push('$');
            for seg in path {
                match seg {
                    Seg::Key(k) if identifier(k) => out.push_str(&format!(".{k}")),
                    Seg::Key(k) => out.push_str(&format!("['{}']", k.replace('\\', "\\\\").replace('\'', "\\'"))),
                    Seg::Index(i) => out.push_str(&format!("[{i}]")),
                }
            }
        }
        Style::Pointer => {
            for seg in path {
                out.push('/');
                match seg {
                    Seg::Key(k) => out.push_str(&k.replace('~', "~0").replace('/', "~1")),
                    Seg::Index(i) => out.push_str(&i.to_string()),
                }
            }
        }
    }
    out
}

/// A value in a document: its path, its key's place when it has one, and
/// where it runs (its key and value both).
#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub path: Vec<Seg>,
    pub key: Option<(Pos, Pos)>,
    pub start: Pos,
    pub end: Pos,
}

/// Every value in `node`, with where it is.
pub fn locate(node: &Node) -> Vec<Located> {
    let mut out = vec![Located { path: Vec::new(), key: None, start: node.start, end: node.end }];
    walk(node, &mut Vec::new(), &mut out);
    out
}

fn walk(node: &Node, path: &mut Vec<Seg>, out: &mut Vec<Located>) {
    match &node.value {
        Value::Object(members) => {
            for m in members {
                path.push(Seg::Key(m.key.clone()));
                out.push(Located { path: path.clone(), key: Some((m.key_start, m.key_end)), start: m.key_start.min(m.value.start), end: m.value.end.max(m.key_end) });
                walk(&m.value, path, out);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                out.push(Located { path: path.clone(), key: None, start: item.start, end: item.end });
                walk(item, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// The deepest value at `pos`.
pub fn at(located: &[Located], pos: Pos) -> Option<&Located> {
    // Of two that touch there, the one that starts there.
    located.iter().filter(|l| l.start <= pos && pos <= l.end).max_by(|a, b| a.path.len().cmp(&b.path.len()).then_with(|| a.start.cmp(&b.start)))
}

/// A JSON Pointer's segments (`/a/0` is `a`, `0`), as text.
pub fn pointer_segments(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return Vec::new();
    }
    pointer.trim_start_matches('/').split('/').map(|s| s.replace("~1", "/").replace("~0", "~")).collect()
}

/// What a JSON Pointer names, or the nearest value it's inside.
pub fn by_pointer<'a>(located: &'a [Located], pointer: &str) -> Option<&'a Located> {
    let want = pointer_segments(pointer);
    let matches = |l: &Located| {
        l.path.len() <= want.len()
            && l.path.iter().zip(&want).all(|(seg, w)| match seg {
                Seg::Key(k) => k == w,
                Seg::Index(i) => i.to_string() == *w,
            })
    };
    // The last of a key written twice is the one read.
    located.iter().filter(|l| matches(l)).max_by_key(|l| l.path.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_at(located: &[Located], line: u32, col: u32, style: Style) -> String {
        render(&at(located, Pos::new(line, col)).unwrap().path, style)
    }

    #[test]
    fn paths_render_three_ways() {
        let path = vec![Seg::Key("servers".into()), Seg::Index(2), Seg::Key("port".into())];
        assert_eq!(render(&path, Style::Jq), ".servers[2].port");
        assert_eq!(render(&path, Style::JsonPath), "$.servers[2].port");
        assert_eq!(render(&path, Style::Pointer), "/servers/2/port");
        let odd = vec![Seg::Key("a b".into()), Seg::Key("x/y~".into()), Seg::Key("it's".into())];
        assert_eq!(render(&odd, Style::Jq), r#".["a b"].["x/y~"].["it's"]"#);
        assert_eq!(render(&odd, Style::JsonPath), r"$['a b']['x/y~']['it\'s']");
        assert_eq!(render(&odd, Style::Pointer), "/a b/x~1y~0/it's");
        assert_eq!(render(&[], Style::Jq), ".");
        assert_eq!(render(&[], Style::JsonPath), "$");
        assert_eq!(render(&[], Style::Pointer), "");
    }

    #[test]
    fn the_key_under_the_cursor_in_json() {
        let text = "{\n  \"name\": \"x\",\n  \"servers\": [\n    {\"host\": \"a\", \"port\": 80},\n    {\"host\": \"b\", \"port\": 8080}\n  ]\n}";
        let located = locate(&crate::json::parse(text).unwrap().node);
        assert_eq!(path_at(&located, 1, 4, Style::Jq), ".name", "on the key");
        assert_eq!(path_at(&located, 1, 12, Style::Jq), ".name", "on the value");
        assert_eq!(path_at(&located, 4, 27, Style::Jq), ".servers[1].port");
        assert_eq!(path_at(&located, 4, 6, Style::Jq), ".servers[1].host");
        assert_eq!(path_at(&located, 4, 4, Style::Jq), ".servers[1]", "on its brace");
        assert_eq!(path_at(&located, 5, 2, Style::Jq), ".servers", "on the closing bracket");
        assert_eq!(path_at(&located, 0, 0, Style::Jq), ".");
    }

    #[test]
    fn the_key_under_the_cursor_in_yaml() {
        let text = "name: x\nservers:\n  - host: a\n    port: 80\n  - host: b\n    port: 8080\n";
        let located = locate(&crate::yaml::parse(text).unwrap());
        assert_eq!(path_at(&located, 0, 2, Style::Jq), ".name");
        assert_eq!(path_at(&located, 5, 11, Style::Jq), ".servers[1].port");
        assert_eq!(path_at(&located, 3, 4, Style::Pointer), "/servers/0/port");
        assert_eq!(path_at(&located, 1, 3, Style::Jq), ".servers");
    }

    #[test]
    fn the_key_under_the_cursor_in_toml() {
        let text = "title = \"x\"\n\n[server]\nhost = \"h\"\nports = [80, 81]\n\n[[db]]\nname = \"a\"\n\n[[db]]\nname = \"b\"\n";
        let located = locate(&crate::toml_doc::parse(text).unwrap());
        assert_eq!(path_at(&located, 0, 1, Style::Jq), ".title");
        assert_eq!(path_at(&located, 2, 3, Style::Jq), ".server", "on the header");
        assert_eq!(path_at(&located, 3, 0, Style::Jq), ".server.host");
        assert_eq!(path_at(&located, 4, 14, Style::Jq), ".server.ports[1]");
        assert_eq!(path_at(&located, 10, 9, Style::Jq), ".db[1].name");
        assert_eq!(path_at(&located, 8, 0, Style::Jq), ".db[0]", "a blank line in its body");
        assert_eq!(path_at(&located, 9, 0, Style::Jq), ".db[1]", "the next header");
    }

    #[test]
    fn pointers_find_values() {
        let text = "{\"a\": {\"b c\": [1, {\"d/e\": 2}]}}";
        let located = locate(&crate::json::parse(text).unwrap().node);
        let l = by_pointer(&located, "/a/b c/1/d~1e").unwrap();
        assert_eq!(l.key.unwrap().0, Pos::new(0, 19));
        assert_eq!(by_pointer(&located, "/a/missing").unwrap().path, vec![Seg::Key("a".into())], "the nearest");
        assert_eq!(by_pointer(&located, "").unwrap().path, vec![]);
    }
}
