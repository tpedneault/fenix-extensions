//! Jira Cloud's rich text, the Atlassian Document Format (ADF): a JSON
//! tree that REST API v3 gives for a description and a comment, and
//! takes for one written.
//!
//! [`to_text`] flattens it to plain text as a person would read it raw:
//! paragraphs apart, headings as their line, lists as `- ` and `1. `
//! (nested ones indented), task items as `[ ]` and `[x]`, code blocks
//! fenced and kept as they are, quotes as `> `, a link as `text (url)`,
//! a mention as `@name`, an emoji as its short name (`:smile:`), a date
//! as `YYYY-MM-DD`, a status as `[DONE]`, a table a row a line with its
//! cells between `|`, a picture or file as `[attachment]`, a panel's text
//! after its kind (`Note: `), an expand's title then its text. Colours,
//! bold, italics, underline, sizes and the layout of columns go; a node
//! it doesn't know is skipped, its text kept.
//!
//! [`from_text`] goes the other way only as far as plain text can say
//! without claiming anything it can't keep: paragraphs (a blank line
//! between) and line breaks. So a comment or a new issue's description
//! goes as you wrote it; an existing description, which may hold more,
//! is never written back from text (`crate::tracker`).

use serde_json::{json, Value};

/// `doc`'s plain text; a string, for a field that isn't ADF (Server's
/// wiki markup, or v2's text), as it is; nothing for null.
pub fn to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            let mut out = blocks(children(value), 0);
            while out.ends_with('\n') {
                out.pop();
            }
            out
        }
        _ => String::new(),
    }
}

/// Plain text as an ADF document: a paragraph for each run of lines
/// between blank lines, a `hardBreak` for each line end within one.
pub fn from_text(text: &str) -> Value {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut content = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    let flush = |lines: &mut Vec<&str>, content: &mut Vec<Value>| {
        if lines.is_empty() {
            return;
        }
        let mut inline = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                inline.push(json!({"type": "hardBreak"}));
            }
            if !line.is_empty() {
                inline.push(json!({"type": "text", "text": line}));
            }
        }
        content.push(json!({"type": "paragraph", "content": inline}));
        lines.clear();
    };
    for line in text.split('\n') {
        if line.trim().is_empty() {
            flush(&mut lines, &mut content);
        } else {
            lines.push(line);
        }
    }
    flush(&mut lines, &mut content);
    json!({"type": "doc", "version": 1, "content": content})
}

fn kind(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn children(node: &Value) -> &[Value] {
    node.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn attr<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    node.get("attrs")?.get(name)
}

fn attr_str<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    attr(node, name)?.as_str().filter(|s| !s.is_empty())
}

/// Block nodes, each ending with a line end, a blank line between them.
fn blocks(nodes: &[Value], depth: usize) -> String {
    let mut out = String::new();
    for node in nodes {
        let text = block(node, depth);
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&text);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// `text`'s lines after `first` on the first and `rest` on the others.
fn prefixed(text: &str, first: &str, rest: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.trim_end_matches('\n').split('\n').enumerate() {
        out.push_str(if i == 0 { first } else { rest });
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn block(node: &Value, depth: usize) -> String {
    match kind(node) {
        "paragraph" | "heading" => inline(children(node)),
        "codeBlock" => {
            let code = children(node).iter().filter_map(|t| t.get("text").and_then(Value::as_str)).collect::<String>();
            format!("```{}\n{}\n```", attr_str(node, "language").unwrap_or_default(), code.trim_end_matches('\n'))
        }
        "blockquote" => prefixed(&blocks(children(node), depth), "> ", "> ").replace("> \n", ">\n"),
        "rule" => "---".into(),
        "bulletList" => list(node, depth, |_| "- ".into()),
        "orderedList" => {
            let start = attr(node, "order").and_then(Value::as_u64).unwrap_or(1);
            list(node, depth, move |i| format!("{}. ", start + i as u64))
        }
        "taskList" => list(node, depth, |_| String::new()),
        "decisionList" => list(node, depth, |_| String::new()),
        "taskItem" => {
            let mark = if attr_str(node, "state") == Some("DONE") { "[x] " } else { "[ ] " };
            item_text(node, mark, depth)
        }
        "decisionItem" => item_text(node, "Decision: ", depth),
        "panel" => {
            let label = match attr_str(node, "panelType").unwrap_or("info") {
                "note" => "Note",
                "warning" => "Warning",
                "error" => "Error",
                "success" => "Success",
                "tip" => "Tip",
                "custom" => "",
                _ => "Info",
            };
            let text = blocks(children(node), depth);
            if label.is_empty() {
                text
            } else {
                prefixed(&text, &format!("{label}: "), "")
            }
        }
        "expand" | "nestedExpand" => {
            let body = blocks(children(node), depth);
            match attr_str(node, "title") {
                Some(title) => format!("{title}\n{body}"),
                None => body,
            }
        }
        "table" => children(node).iter().map(row).filter(|r| !r.is_empty()).collect::<Vec<_>>().join("\n"),
        "mediaSingle" | "mediaGroup" => {
            let mut lines: Vec<String> = children(node).iter().filter(|c| kind(c) == "media").map(media).collect();
            if let Some(caption) = children(node).iter().find(|c| kind(c) == "caption") {
                let caption = inline(children(caption));
                if !caption.is_empty() {
                    lines.push(caption);
                }
            }
            lines.join("\n")
        }
        "media" => media(node),
        "blockCard" | "embedCard" => attr_str(node, "url").map(str::to_string).unwrap_or_else(|| inline(children(node))),
        // An app's macro: what it holds, else its name.
        "extension" | "inlineExtension" => attr_str(node, "text").or_else(|| attr_str(node, "extensionKey")).map(|k| format!("[{k}]")).unwrap_or_default(),
        // Columns, a macro's body: their blocks, in order.
        "layoutSection" | "layoutColumn" | "bodiedExtension" | "multiBodiedExtension" | "extensionFrame" | "doc" => blocks(children(node), depth),
        "text" | "hardBreak" | "mention" | "emoji" | "inlineCard" | "date" | "status" | "mediaInline" | "placeholder" => inline(std::slice::from_ref(node)),
        // Anything else: its blocks, or its text.
        _ => {
            let inner = children(node);
            if inner.iter().any(|c| matches!(kind(c), "text" | "hardBreak" | "mention" | "emoji" | "inlineCard" | "date" | "status")) {
                inline(inner)
            } else if !inner.is_empty() {
                blocks(inner, depth)
            } else {
                node.get("text").and_then(Value::as_str).unwrap_or_default().to_string()
            }
        }
    }
}

/// A list's items, after `marker(i)`, those inside indented.
fn list(node: &Value, depth: usize, marker: impl Fn(usize) -> String) -> String {
    let mut out = String::new();
    for (i, item) in children(node).iter().enumerate() {
        let text = match kind(item) {
            "listItem" => item_text(item, &marker(i), depth),
            // A task list inside a task list: nested under the one before.
            "taskList" | "decisionList" | "bulletList" | "orderedList" => prefixed(&block(item, depth + 1), "  ", "  "),
            _ => block(item, depth + 1),
        };
        out.push_str(&text);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// A list item: its first block after `marker`, the rest (a nested list
/// among them) under it, indented as far as the marker is wide.
fn item_text(item: &Value, marker: &str, depth: usize) -> String {
    let indent = " ".repeat(marker.chars().count().max(2));
    let mut out = String::new();
    let content = children(item);
    // A task or decision item holds inline nodes itself.
    if content.iter().all(is_inline) {
        return prefixed(&inline(content), marker, &indent);
    }
    let mut first = true;
    for child in content {
        let text = if is_inline(child) { inline(std::slice::from_ref(child)) } else { block(child, depth + 1) };
        if text.is_empty() {
            continue;
        }
        let nested = matches!(kind(child), "bulletList" | "orderedList" | "taskList" | "decisionList");
        if first && !nested {
            out.push_str(&prefixed(&text, marker, &indent));
        } else if first {
            out.push_str(marker.trim_end());
            out.push('\n');
            out.push_str(&prefixed(&text, &indent, &indent));
        } else {
            out.push_str(&prefixed(&text, &indent, &indent));
        }
        first = false;
    }
    if first {
        out.push_str(marker.trim_end());
        out.push('\n');
    }
    out
}

fn is_inline(node: &Value) -> bool {
    matches!(kind(node), "text" | "hardBreak" | "mention" | "emoji" | "inlineCard" | "date" | "status" | "mediaInline" | "placeholder" | "inlineExtension")
}

/// A table row: its cells' text on one line, between `|`.
fn row(node: &Value) -> String {
    let cells: Vec<String> = children(node).iter().map(|cell| blocks(children(cell), 0).split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    if cells.iter().all(String::is_empty) {
        return String::new();
    }
    format!("| {} |", cells.join(" | "))
}

/// A picture or a file: there, but not here.
fn media(node: &Value) -> String {
    match (attr_str(node, "type"), attr_str(node, "url"), attr_str(node, "alt")) {
        (Some("external"), Some(url), _) => format!("[image: {url}]"),
        (_, _, Some(alt)) => format!("[attachment: {alt}]"),
        _ => "[attachment]".into(),
    }
}

/// Inline nodes as one run of text.
fn inline(nodes: &[Value]) -> String {
    let mut out = String::new();
    for node in nodes {
        match kind(node) {
            "text" => {
                let text = node.get("text").and_then(Value::as_str).unwrap_or_default();
                let link = node.get("marks").and_then(Value::as_array).into_iter().flatten().find(|m| kind(m) == "link").and_then(|m| attr_str(m, "href"));
                match link {
                    Some(href) if href != text => out.push_str(&format!("{text} ({href})")),
                    _ => out.push_str(text),
                }
            }
            "hardBreak" => out.push('\n'),
            "mention" => {
                let name = attr_str(node, "text").unwrap_or("someone");
                if !name.starts_with('@') {
                    out.push('@');
                }
                out.push_str(name);
            }
            "emoji" => out.push_str(attr_str(node, "shortName").or_else(|| attr_str(node, "text")).unwrap_or_default()),
            "inlineCard" => out.push_str(attr_str(node, "url").unwrap_or_default()),
            "date" => {
                let ms = attr(node, "timestamp").and_then(|t| t.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| t.as_i64()));
                if let Some(day) = ms.and_then(chrono::DateTime::from_timestamp_millis) {
                    out.push_str(&day.format("%Y-%m-%d").to_string());
                }
            }
            "status" => {
                if let Some(text) = attr_str(node, "text") {
                    out.push_str(&format!("[{}]", text.to_uppercase()));
                }
            }
            "mediaInline" => out.push_str(&media(node)),
            // A template's hint, not anyone's words.
            "placeholder" => {}
            "inlineExtension" => out.push_str(&block(node, 0)),
            _ => {
                if let Some(text) = node.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                } else {
                    out.push_str(&inline(children(node)));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
