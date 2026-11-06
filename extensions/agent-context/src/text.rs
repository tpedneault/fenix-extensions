//! Documentation made readable for an agent: HTML as plain text (headings
//! as `#`, lists as `-`, code kept as it's laid out), a Markdown README
//! with its HTML and badges taken out, the part about one item found, and
//! the whole cut to a size.

/// Elements whose content isn't documentation.
const SKIPPED: &[&str] = &["script", "style", "noscript", "svg", "head", "nav", "footer", "header", "button", "form", "template", "iframe", "select", "aside"];

/// Elements that start a line of their own.
const BLOCKS: &[&str] = &[
    "p", "div", "section", "article", "main", "table", "ul", "ol", "dl", "blockquote", "details", "summary", "figure", "figcaption", "body", "html",
];

/// An entity's character: `&amp;`, `&#39;`, `&#x2014;`.
fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "times" => '×',
        "middot" => '·',
        "bull" => '•',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "para" => '¶',
        "sect" => '§',
        "deg" => '°',
        _ => return None,
    })
}

/// `text` with its entities decoded.
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|i| i + 1);
        match end.filter(|&e| rest[e..].starts_with(';')).and_then(|e| entity(&rest[1..e]).map(|c| (e, c))) {
            Some((e, c)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Where a tag that starts at `at` (its `<`) ends, past its `>`: quoted
/// attributes may hold one.
fn tag_end(html: &str, at: usize) -> usize {
    let b = html.as_bytes();
    let mut i = at + 1;
    let mut quote = None;
    while i < b.len() {
        match (quote, b[i]) {
            (None, b'"' | b'\'') => quote = Some(b[i]),
            (Some(q), c) if c == q => quote = None,
            (None, b'>') => return i + 1,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// A tag's name, lower case, and whether it closes: `</Div class>` is
/// `div`, true.
fn tag_name(tag: &str) -> (String, bool) {
    let inner = tag.trim_start_matches('<').trim_end_matches('>').trim_end_matches('/');
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let name: String = inner.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (name.to_ascii_lowercase(), closing)
}

/// The text a page's HTML says, laid out as Markdown would be.
pub fn html_to_text(html: &str) -> String {
    let mut out = Text::default();
    let mut i = 0;
    // How deep in a <pre> it is, and in an element it skips.
    let mut pre = 0usize;
    let mut skipping: Option<(String, usize)> = None;
    let mut code = 0usize;
    while i < html.len() {
        let Some(lt) = html[i..].find('<').map(|n| i + n) else {
            if skipping.is_none() {
                out.text(&html[i..], pre > 0);
            }
            break;
        };
        if skipping.is_none() {
            out.text(&html[i..lt], pre > 0);
        }
        if html[lt..].starts_with("<!--") {
            i = html[lt..].find("-->").map_or(html.len(), |n| lt + n + 3);
            continue;
        }
        if html[lt..].starts_with("<!") || html[lt..].starts_with("<?") {
            i = tag_end(html, lt);
            continue;
        }
        // A `<` that starts no tag is text.
        if !html[lt + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            if skipping.is_none() {
                out.text("<", pre > 0);
            }
            i = lt + 1;
            continue;
        }
        let end = tag_end(html, lt);
        let tag = &html[lt..end];
        i = end;
        let (name, closing) = tag_name(tag);
        let self_closing = tag.ends_with("/>");
        if let Some((skipped, depth)) = &mut skipping {
            if name == *skipped {
                if closing {
                    *depth -= 1;
                    if *depth == 0 {
                        skipping = None;
                    }
                } else if !self_closing {
                    *depth += 1;
                }
            }
            continue;
        }
        if SKIPPED.contains(&name.as_str()) {
            if !closing && !self_closing {
                skipping = Some((name, 1));
            }
            continue;
        }
        match (name.as_str(), closing) {
            ("pre", false) => {
                pre += 1;
                out.block();
                out.raw("```\n");
            }
            ("pre", true) => {
                pre = pre.saturating_sub(1);
                out.newline();
                out.raw("```");
                out.block();
            }
            ("code", false) if pre == 0 => {
                code += 1;
                out.text("`", false);
            }
            ("code", true) if pre == 0 && code > 0 => {
                code -= 1;
                out.raw("`");
            }
            ("br", _) => out.newline(),
            ("hr", _) => {
                out.block();
                out.raw("---");
                out.block();
            }
            (h, false) if h.len() == 2 && h.starts_with('h') && h.as_bytes()[1].is_ascii_digit() => {
                out.block();
                let level = (h.as_bytes()[1] - b'0').clamp(1, 6) as usize;
                out.raw(&format!("{} ", "#".repeat(level)));
            }
            (h, true) if h.len() == 2 && h.starts_with('h') && h.as_bytes()[1].is_ascii_digit() => out.block(),
            ("li", false) => {
                out.newline();
                out.raw("- ");
            }
            ("dt", false) => out.newline(),
            ("dd", false) => {
                out.newline();
                out.raw("  ");
            }
            ("tr", _) => out.newline(),
            ("td" | "th", false) => out.text(" | ", false),
            ("img", _) => {
                if let Some(alt) = image_alt(tag) {
                    out.text(&format!("[{alt}]"), false);
                }
            }
            (b, _) if BLOCKS.contains(&b) => out.block(),
            _ => {}
        }
    }
    out.finish()
}

/// What an image says, unless it's a badge (a build's status, a
/// version, downloads): its `alt`.
fn image_alt(tag: &str) -> Option<String> {
    let src = attribute(tag, "src").unwrap_or_default().to_ascii_lowercase();
    if ["shields.io", "badge", "travis-ci", "codecov", "coveralls", "pepy.tech"].iter().any(|b| src.contains(b)) {
        return None;
    }
    attribute(tag, "alt").map(|a| a.trim().to_string()).filter(|a| !a.is_empty())
}

/// An attribute's value in a tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!(" {name}="))? + name.len() + 2;
    let rest = &tag[at..];
    let value = match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next()?,
        _ => rest.split([' ', '>']).next()?,
    };
    Some(decode_entities(value))
}

/// Text written as it's laid out: spaces run together outside `<pre>`,
/// no more than one blank line in a row.
#[derive(Default)]
struct Text {
    out: String,
}

impl Text {
    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn trim_end_spaces(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
    }

    fn text(&mut self, text: &str, pre: bool) {
        let text = decode_entities(text);
        if pre {
            self.out.push_str(&text);
            return;
        }
        for (n, word) in text.split(|c: char| c.is_whitespace()).enumerate() {
            if n > 0 && !self.at_line_start() && !self.out.ends_with(' ') {
                self.out.push(' ');
            }
            self.out.push_str(word);
        }
    }

    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }

    /// Whether the line so far is only a list item's `- `.
    fn at_item_start(&self) -> bool {
        self.out.rsplit('\n').next().is_some_and(|l| l == "- ")
    }

    fn newline(&mut self) {
        if self.at_item_start() {
            return;
        }
        self.trim_end_spaces();
        if !self.at_line_start() {
            self.out.push('\n');
        }
    }

    /// A blank line, unless there's one.
    fn block(&mut self) {
        if self.at_item_start() {
            return;
        }
        self.newline();
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn finish(self) -> String {
        tidy(&self.out)
    }
}

/// Lines' ends trimmed, and no more than one blank line in a row.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let line = if fenced { line.trim_end() } else { line.trim() };
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Whether `text` is HTML rather than Markdown or plain text.
pub fn looks_like_html(text: &str) -> bool {
    let head = text.trim_start();
    let lower: String = head.chars().take(400).collect::<String>().to_ascii_lowercase();
    lower.starts_with("<!doctype") || lower.starts_with("<html") || lower.starts_with("<div") || lower.starts_with("<p>") || lower.starts_with("<h1") || lower.starts_with("<section") || lower.starts_with("<article")
}

/// A Markdown README for reading: HTML tags out (their text kept),
/// comments out, lines of nothing but badges out -- code blocks left
/// alone.
pub fn tidy_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    let mut in_comment = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if fenced {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut kept = String::new();
        let mut rest = line;
        loop {
            if in_comment {
                match rest.find("-->") {
                    Some(e) => {
                        rest = &rest[e + 3..];
                        in_comment = false;
                    }
                    None => break,
                }
            }
            match rest.find("<!--") {
                Some(s) => {
                    kept.push_str(&rest[..s]);
                    rest = &rest[s + 4..];
                    in_comment = true;
                }
                None => {
                    kept.push_str(rest);
                    break;
                }
            }
        }
        // A line that was all comment.
        if kept.trim().is_empty() && !line.trim().is_empty() {
            continue;
        }
        let stripped = strip_tags(&kept);
        if is_badges(&stripped) {
            continue;
        }
        if stripped.trim().is_empty() && !kept.trim().is_empty() {
            continue;
        }
        out.push_str(&stripped);
        out.push('\n');
    }
    tidy(&out)
}

/// HTML tags taken out of a line of Markdown, outside inline code.
fn strip_tags(line: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut i = 0;
    let b = line.as_bytes();
    while i < b.len() {
        let c = b[i];
        if c == b'`' {
            in_code = !in_code;
        }
        if c == b'<' && !in_code && line[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            if let Some(e) = line[i..].find('>') {
                let tag = &line[i..i + e + 1];
                // An autolink, `<https://…>`, stays.
                if !tag.contains("://") || tag.contains(' ') {
                    let (name, _) = tag_name(tag);
                    if name == "img" {
                        if let Some(alt) = image_alt(tag) {
                            out.push_str(&format!("[{alt}]"));
                        }
                    }
                    i += e + 1;
                    continue;
                }
            }
        }
        let ch = line[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    if out.contains('&') {
        decode_entities(&out)
    } else {
        out
    }
}

/// A line of nothing but badges: `[![CI](…)](…) [![crates.io](…)](…)`.
fn is_badges(line: &str) -> bool {
    let mut rest = line.trim();
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        let image = if let Some(r) = rest.strip_prefix("[![") {
            // `[![alt](img)](link)`
            let Some(close) = r.find("](") else { return false };
            let r = &r[close + 2..];
            let Some(end) = r.find(")](") else { return false };
            let r = &r[end + 3..];
            let Some(end) = r.find(')') else { return false };
            &r[end + 1..]
        } else if let Some(r) = rest.strip_prefix("![") {
            let Some(close) = r.find("](") else { return false };
            let r = &r[close + 2..];
            let Some(end) = r.find(')') else { return false };
            &r[end + 1..]
        } else {
            return false;
        };
        rest = image.trim_start();
    }
    true
}

/// The part of `text` about `item`: from the heading (or else the line)
/// that names it first, with the line before for context. `None` when
/// nothing names it.
pub fn focus(text: &str, item: &str) -> Option<String> {
    let item = item.trim();
    if item.is_empty() {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    // Its last part is what headings and signatures name: `sync::Mutex`'s
    // `Mutex`, `pkg.Func`'s `Func`.
    let last = item.rsplit([':', '.', '/']).find(|s| !s.is_empty()).unwrap_or(item);
    let names = |l: &str, w: &str| {
        l.match_indices(w).any(|(at, _)| {
            let before = l[..at].chars().next_back();
            let after = l[at + w.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    };
    let heading = |l: &&str| l.trim_start().starts_with('#');
    let at = lines
        .iter()
        .position(|l| heading(l) && names(l, item))
        .or_else(|| lines.iter().position(|l| heading(l) && names(l, last)))
        .or_else(|| lines.iter().position(|l| names(l, item)))
        .or_else(|| lines.iter().position(|l| names(l, last)))?;
    let from = if heading(&lines[at]) { at } else { at.saturating_sub(1) };
    Some(lines[from..].join("\n"))
}

/// `text` cut to at most `max` characters, at a line's end when one is
/// near; whether it was cut.
pub fn cut(text: &str, max: usize) -> (String, bool) {
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    let end = text.char_indices().nth(max).map_or(text.len(), |(i, _)| i);
    let head = &text[..end];
    let head = match head.rfind('\n') {
        Some(nl) if nl * 2 >= end => &head[..nl],
        _ => head,
    };
    (head.trim_end().to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata").join(name)).unwrap()
    }

    #[test]
    fn html_becomes_readable_text() {
        let html = "<!DOCTYPE html><html><head><title>x</title><style>p{}</style></head><body><nav>Menu <a href=a>A</a></nav>\
            <h1>Serde&nbsp;&amp; friends</h1><p>A <b>framework</b>   for\n serializing &lt;T&gt; &#8212; fast.</p>\
            <ul><li>one</li><li>two <code>x &lt; y</code></li></ul><pre><code>fn main() {\n    let a = 1 &lt; 2;\n}</code></pre>\
            <script>alert('<p>no</p>')</script><table><tr><th>K</th><th>V</th></tr><tr><td>a</td><td>1</td></tr></table>\
            <p>Logo <img src=x.png alt=\"the logo\"> &unknown; done<br>next</p><!-- <p>hidden</p> --></body></html>";
        let text = html_to_text(html);
        assert_eq!(html_to_text("<ul><li><p>First</p><p>more</p></li></ul>"), "- First\n\nmore", "a paragraph in an item starts on its line");
        assert_eq!(
            text,
            "# Serde & friends\n\nA framework for serializing <T> — fast.\n\n- one\n- two `x < y`\n\n```\nfn main() {\n    let a = 1 < 2;\n}\n```\n\n| K | V\n| a | 1\n\nLogo [the logo] &unknown; done\nnext"
        );
    }

    #[test]
    fn a_crates_readme_and_a_rustdoc_page() {
        let readme = html_to_text(&testdata("crates-readme.html"));
        assert!(readme.starts_with("# Anyhow"), "{readme}");
        assert!(readme.contains("```\n[dependencies]\nanyhow = \"1.0\"\n```"), "{readme}");
        assert!(!readme.contains("<") || readme.contains("Result<T"), "{readme}");
        let page = html_to_text(&testdata("docs-rs-item.html"));
        assert!(page.contains("# Struct tokio::sync::Mutex"), "{page}");
        assert!(page.contains("pub fn new(t: T) -> Mutex<T>"), "{page}");
        assert!(!page.contains("Settings") && !page.contains("window.rootPath"), "the sidebar, the header and scripts are left out: {page}");
    }

    #[test]
    fn a_markdown_readme_without_its_html_and_badges() {
        let md = "<p align=\"center\"><img src=\"logo.svg\" alt=\"Logo\"></p>\n\n[![CI](https://x/badge.svg)](https://x) [![npm](https://y.svg)](https://y)\n![size](https://z.svg)\n\n# Lib\n\nUse <kbd>Ctrl</kbd> or `<div>` &amp; see <https://example.com>.\n<!-- a\ncomment -->\n```html\n<div>kept</div>\n```\n";
        assert_eq!(tidy_markdown(md), "[Logo]\n\n# Lib\n\nUse Ctrl or `<div>` & see <https://example.com>.\n```html\n<div>kept</div>\n```");
        assert!(!looks_like_html(md));
        assert!(looks_like_html("<div class=\"readme\"><h1>x</h1></div>"));
    }

    #[test]
    fn the_part_about_an_item() {
        let text = "# Lib\n\nIntro mentions Client once.\n\n## Usage\n\nstuff\n\n## Client\n\nThe client.\n\n## Server\n";
        assert!(focus(text, "Client").unwrap().starts_with("## Client\n\nThe client."));
        assert!(focus(text, "lib::Client").unwrap().starts_with("## Client"), "by its last part");
        assert!(focus(text, "stuff").unwrap().starts_with("\nstuff"), "a line, with the one before");
        assert_eq!(focus(text, "Clients"), None);
        assert_eq!(focus(text, "Cli"), None, "a whole word");
    }

    #[test]
    fn cut_at_a_line_end() {
        assert_eq!(cut("short", 10), ("short".into(), false));
        let text = "aaaa\nbbbb\ncccc";
        assert_eq!(cut(text, 12), ("aaaa\nbbbb".into(), true));
        assert_eq!(cut("ééééé", 3), ("ééé".into(), true), "on a character, not a byte");
    }
}
