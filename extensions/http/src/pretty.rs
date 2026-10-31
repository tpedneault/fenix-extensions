//! A response's body as the response view shows it: JSON, XML and HTML
//! pretty-printed and coloured by the theme's syntax colours, other text
//! as it came, a picture as a picture, anything else as bytes.

use fenix_ui_kit::Role;

/// A line of runs, each in a colour.
pub type Line = Vec<(String, Role)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json,
    Xml,
    Html,
    Text,
    Image,
    Binary,
}

/// What a body is, by its content type and, failing that, a look at it.
pub fn kind(content_type: &str, body: &[u8]) -> Kind {
    let ct = content_type.to_ascii_lowercase();
    let ct = ct.split(';').next().unwrap_or("").trim();
    if ct.starts_with("image/") && !ct.contains("svg") {
        return Kind::Image;
    }
    if ct.ends_with("json") || ct.ends_with("+json") || ct == "application/problem+json" {
        return Kind::Json;
    }
    if ct.contains("html") {
        return Kind::Html;
    }
    if ct.ends_with("xml") || ct.contains("svg") {
        return Kind::Xml;
    }
    let Ok(text) = std::str::from_utf8(body) else {
        return if is_image(body) { Kind::Image } else { Kind::Binary };
    };
    let t = text.trim_start();
    if (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<serde_json::Value>(text).is_ok() {
        Kind::Json
    } else if t.to_ascii_lowercase().starts_with("<!doctype html") || t.to_ascii_lowercase().starts_with("<html") {
        Kind::Html
    } else if t.starts_with("<?xml") {
        Kind::Xml
    } else {
        Kind::Text
    }
}

/// PNG, JPEG, GIF or WebP, by their first bytes.
pub fn is_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG") || b.starts_with(&[0xff, 0xd8, 0xff]) || b.starts_with(b"GIF8") || (b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

/// The body's lines, laid out and coloured; `pretty` false keeps them
/// as they came.
pub fn lines(kind: Kind, body: &[u8], pretty: bool) -> Vec<Line> {
    let text = String::from_utf8_lossy(body);
    match kind {
        Kind::Json => {
            let shown = if pretty { crate::json::pretty(&text).unwrap_or_else(|| text.to_string()) } else { text.to_string() };
            shown.lines().map(json_line).collect()
        }
        Kind::Xml | Kind::Html => {
            let shown = if pretty { markup(&text, kind == Kind::Html) } else { text.to_string() };
            shown.lines().map(markup_line).collect()
        }
        Kind::Text => text.lines().map(|l| vec![(l.to_string(), Role::Text)]).collect(),
        Kind::Image | Kind::Binary => hex(body),
    }
}

/// The first bytes, sixteen a line, as hex and as text.
fn hex(body: &[u8]) -> Vec<Line> {
    let mut out: Vec<Line> = body
        .chunks(16)
        .take(256)
        .enumerate()
        .map(|(i, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let text: String = chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
            vec![(format!("{:08x}  ", i * 16), Role::Muted), (format!("{:<48}", hex.join(" ")), Role::Text), (format!(" {text}"), Role::Muted)]
        })
        .collect();
    if body.len() > 16 * 256 {
        out.push(vec![(format!("… {} more bytes", body.len() - 16 * 256), Role::Muted)]);
    }
    out
}

fn syntax(name: &str) -> Role {
    Role::Syntax(name.into())
}

/// A line of JSON, coloured: keys, strings, numbers, the constants.
pub fn json_line(line: &str) -> Line {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Line = Vec::new();
    let mut i = 0;
    let push = |out: &mut Line, s: String, r: Role| match out.last_mut() {
        Some((t, role)) if *role == r => t.push_str(&s),
        _ => out.push((s, r)),
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            let s: String = chars[i..end].iter().collect();
            let rest = chars[end..].iter().find(|c| !c.is_whitespace());
            push(&mut out, s, if rest == Some(&':') { syntax("property") } else { syntax("string") });
            i = end;
        } else if c == '-' || c.is_ascii_digit() {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_digit() || matches!(chars[j], '.' | 'e' | 'E' | '+' | '-')) {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), syntax("number"));
            i = j;
        } else if c.is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), syntax("constant"));
            i = j;
        } else if c.is_whitespace() {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), Role::Text);
            i = j;
        } else {
            push(&mut out, c.to_string(), Role::Muted);
            i += 1;
        }
    }
    out
}

/// HTML's elements that have no end tag.
const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
/// Elements whose text is kept as it is.
const RAW: &[&str] = &["script", "style", "pre", "textarea"];

fn tag_name(tag: &str) -> String {
    tag.trim_start_matches(['<', '/']).split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or("").to_ascii_lowercase()
}

/// XML or HTML laid out a tag a line, indented by how deep it is; text
/// between tags on its own line, trimmed.
pub fn markup(text: &str, html: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut depth: usize = 0;
    let mut rest = text;
    let pad = |d: usize| "  ".repeat(d);
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let end = if stripped.starts_with("!--") { rest.find("-->").map(|i| i + 3) } else { rest.find('>').map(|i| i + 1) };
            let end = end.unwrap_or(rest.len());
            let tag = &rest[..end];
            rest = &rest[end..];
            let name = tag_name(tag);
            if tag.starts_with("</") {
                depth = depth.saturating_sub(1);
                out.push(format!("{}{tag}", pad(depth)));
            } else if tag.starts_with("<!") || tag.starts_with("<?") || tag.ends_with("/>") || (html && VOID.contains(&name.as_str())) {
                out.push(format!("{}{tag}", pad(depth)));
            } else if html && RAW.contains(&name.as_str()) {
                let close = format!("</{name}");
                let at = rest.to_ascii_lowercase().find(&close).unwrap_or(rest.len());
                let inner = &rest[..at];
                out.push(format!("{}{tag}", pad(depth)));
                for l in inner.lines().filter(|l| !l.trim().is_empty()) {
                    out.push(format!("{}{}", pad(depth + 1), l.trim_end()));
                }
                rest = &rest[at..];
                depth += 1;
            } else {
                // `<a>text</a>` on one line when the text is short.
                let close = format!("</{name}>");
                let next = rest.find('<').unwrap_or(rest.len());
                let inner = rest[..next].trim();
                if rest[next..].starts_with(&close) && !inner.contains('\n') && inner.len() <= 80 {
                    out.push(format!("{}{tag}{inner}{close}", pad(depth)));
                    rest = &rest[next + close.len()..];
                } else {
                    out.push(format!("{}{tag}", pad(depth)));
                    depth += 1;
                }
            }
        } else {
            let next = rest.find('<').unwrap_or(rest.len());
            for l in rest[..next].lines().map(str::trim).filter(|l| !l.is_empty()) {
                out.push(format!("{}{l}", pad(depth)));
            }
            rest = &rest[next..];
        }
    }
    out.join("\n")
}

/// A line of markup, coloured: tags, attributes and their values,
/// comments.
pub fn markup_line(line: &str) -> Line {
    let mut out: Line = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|i| i + 3).unwrap_or(rest.len());
            out.push((rest[..end].to_string(), syntax("comment")));
            rest = &rest[end..];
        } else if rest.starts_with('<') {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            let tag = &rest[..end];
            rest = &rest[end..];
            let name_end = tag.find(|c: char| c.is_whitespace() || c == '>' || (c == '/' && !tag.starts_with("</"))).unwrap_or(tag.len()).max(1);
            let name_end = match tag.strip_prefix("</") {
                Some(rest) => rest.find(|c: char| c.is_whitespace() || c == '>').map(|i| i + 2).unwrap_or(tag.len()),
                None => name_end,
            };
            out.push((tag[..name_end].to_string(), syntax("tag")));
            let mut attrs = &tag[name_end..];
            while !attrs.is_empty() {
                if let Some(q) = attrs.find(['"', '\'']) {
                    let quote = attrs[q..].chars().next().unwrap();
                    let close = attrs[q + 1..].find(quote).map(|i| i + q + 2).unwrap_or(attrs.len());
                    out.push((attrs[..q].to_string(), syntax("attribute")));
                    out.push((attrs[q..close].to_string(), syntax("string")));
                    attrs = &attrs[close..];
                } else {
                    let (a, b) = attrs.split_at(attrs.len() - attrs.ends_with('>') as usize - attrs.ends_with("/>") as usize);
                    if !a.is_empty() {
                        out.push((a.to_string(), syntax("attribute")));
                    }
                    if !b.is_empty() {
                        out.push((b.to_string(), syntax("tag")));
                    }
                    attrs = "";
                }
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push((rest[..end].to_string(), Role::Text));
            rest = &rest[end..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.iter().map(|(t, _)| t.as_str()).collect()).collect()
    }

    #[test]
    fn a_body_is_known_by_its_type_or_its_look() {
        assert_eq!(kind("application/json; charset=utf-8", b"{}"), Kind::Json);
        assert_eq!(kind("application/vnd.api+json", b"{}"), Kind::Json);
        assert_eq!(kind("text/html", b""), Kind::Html);
        assert_eq!(kind("application/xml", b""), Kind::Xml);
        assert_eq!(kind("image/png", b""), Kind::Image);
        assert_eq!(kind("image/svg+xml", b"<svg/>"), Kind::Xml);
        assert_eq!(kind("", b"[1, 2]"), Kind::Json);
        assert_eq!(kind("text/plain", b"<!DOCTYPE html><p>"), Kind::Html);
        assert_eq!(kind("", b"\x89PNG\r\n\x1a\n\0"), Kind::Image);
        assert_eq!(kind("", &[0, 159, 146, 150]), Kind::Binary);
        assert_eq!(kind("", b"plain"), Kind::Text);
    }

    #[test]
    fn json_is_pretty_and_coloured() {
        let lines = lines(Kind::Json, br#"{"name":"ada","n":-1.5e3,"ok":true}"#, true);
        assert_eq!(text(&lines), ["{", "  \"name\": \"ada\",", "  \"n\": -1.5e3,", "  \"ok\": true", "}"]);
        assert_eq!(lines[1][1], ("\"name\"".to_string(), Role::Syntax("property".into())));
        assert_eq!(lines[1][4], ("\"ada\"".to_string(), Role::Syntax("string".into())));
        assert!(lines[2].contains(&("-1.5e3".to_string(), Role::Syntax("number".into()))));
        assert!(lines[3].contains(&("true".to_string(), Role::Syntax("constant".into()))));
    }

    #[test]
    fn markup_is_indented() {
        let xml = "<?xml version=\"1.0\"?><list><item id=\"1\">one</item><item id=\"2\"><b>two</b></item><empty/></list>";
        assert_eq!(markup(xml, false), "<?xml version=\"1.0\"?>\n<list>\n  <item id=\"1\">one</item>\n  <item id=\"2\">\n    <b>two</b>\n  </item>\n  <empty/>\n</list>");
        let html = "<html><head><meta charset=utf-8><script>let a = 1 < 2;</script></head><body><p>Hi<br>there</p></body></html>";
        assert_eq!(
            markup(html, true),
            "<html>\n  <head>\n    <meta charset=utf-8>\n    <script>\n      let a = 1 < 2;\n    </script>\n  </head>\n  <body>\n    <p>\n      Hi\n      <br>\n      there\n    </p>\n  </body>\n</html>"
        );
        let line = markup_line("<item id=\"1\">one</item><!-- c -->");
        assert_eq!(line[0], ("<item".to_string(), Role::Syntax("tag".into())));
        assert_eq!(line[2], ("\"1\"".to_string(), Role::Syntax("string".into())));
        assert_eq!(line.last().unwrap().1, Role::Syntax("comment".into()));
        assert_eq!(text(&[line]), ["<item id=\"1\">one</item><!-- c -->"]);
    }

    #[test]
    fn bytes_are_shown_as_hex() {
        let l = lines(Kind::Binary, &[0x41, 0x00, 0xff], true);
        assert_eq!(text(&l), [format!("00000000  {:<48} A..", "41 00 ff")]);
    }
}
