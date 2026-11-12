//! A small Markdown renderer for extensions' READMEs and CHANGELOGs:
//! headings, paragraphs, lists, tables, block quotes, fenced code, and
//! inline code, links, images, emphasis and strike-through.
//!
//! What it makes is safe whoever wrote the text: every character of it
//! is escaped, raw HTML included (it shows as written), and a link or a
//! picture goes where the caller's `link` and `image` say -- or nowhere.

use std::collections::BTreeMap;

/// `&`, `<`, `>`, `"` and `'`, escaped: for text and for attributes.
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// What a URL may be: http(s), or relative (a path, `?query` or
/// `#anchor`). Anything with another scheme (`javascript:`, `data:`), a
/// host of its own (`//host`) or a character a URL never carries is not.
pub(crate) fn safe_url(url: &str) -> bool {
    if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\' || c == '<' || c == '>' || c == '"' || c == '\'' || c == '`') {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return url.len() > lower.find("//").unwrap_or(0) + 2;
    }
    if url.starts_with("//") {
        return false;
    }
    // A scheme is letters before a colon that comes before any / ? #.
    let head = url.split(['/', '?', '#']).next().unwrap_or_default();
    !head.contains(':')
}

/// How the caller places what the text points at.
pub(crate) struct Options<'a> {
    /// Put before every heading's id, and before an `#anchor` link's, so
    /// two documents on one page keep theirs apart.
    pub id_prefix: &'a str,
    /// Added to each heading's level (a README's `#` under the page's own
    /// `<h1>`), at most `<h6>`.
    pub shift: usize,
    /// A link's URL, given one that's relative and not an anchor: where it
    /// goes, or `None` for text without a link.
    pub link: &'a dyn Fn(&str) -> Option<String>,
    /// A picture's URL, given one that's relative: where it is, or `None`
    /// to show its text instead. A picture from elsewhere (http) is never
    /// loaded: it's a link to it.
    pub image: &'a dyn Fn(&str) -> Option<String>,
}

/// A document's HTML.
pub(crate) fn render(text: &str, options: &Options) -> String {
    let lines: Vec<String> = text.replace("\r\n", "\n").replace('\r', "\n").lines().map(expand_tabs).collect();
    let mut ids = BTreeMap::new();
    let mut out = String::new();
    blocks(&lines, options, &mut ids, &mut out);
    out
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        if c == '\t' {
            let n = 4 - out.chars().count() % 4;
            out.extend(std::iter::repeat_n(' ', n));
        } else {
            out.push(c);
        }
    }
    out
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A fence's character, its length and its info string.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    let info = t[n..].trim();
    (n >= 3 && !(c == '`' && info.contains('`'))).then_some((c, n, info))
}

fn heading(line: &str) -> Option<(usize, &str)> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let rest = &t[n..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    // Closing #s go, as long as a space comes before them.
    let rest = rest.trim();
    let without = rest.trim_end_matches('#');
    let text = if without.is_empty() || without.ends_with(' ') { without.trim_end() } else { rest };
    Some((n, text))
}

fn rule(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && ['-', '*', '_'].iter().any(|m| t.chars().all(|c| c == *m))
}

/// A list item's marker: whether it's ordered, its number, the
/// character that ends it (`-`, `*`, `+`, `.` or `)`), and the column its
/// content starts at.
fn item(line: &str) -> Option<(bool, u64, char, usize)> {
    let lead = indent(line);
    if lead > 3 {
        return None;
    }
    let t = &line[lead..];
    let (ordered, number, mark, width) = if let Some(c) = t.chars().next().filter(|c| matches!(c, '-' | '*' | '+')) {
        (false, 0, c, 1)
    } else {
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        let mark = t[digits..].chars().next().filter(|c| *c == '.' || *c == ')')?;
        if digits == 0 || digits > 9 {
            return None;
        }
        (true, t[..digits].parse().ok()?, mark, digits + 1)
    };
    let after = &t[width..];
    if after.is_empty() {
        return Some((ordered, number, mark, lead + width + 1));
    }
    let spaces = indent(after);
    if spaces == 0 {
        return None;
    }
    let spaces = if spaces > 4 || blank(after) { 1 } else { spaces };
    Some((ordered, number, mark, lead + width + spaces))
}

fn quote(line: &str) -> Option<&str> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start().strip_prefix('>')?;
    Some(t.strip_prefix(' ').unwrap_or(t))
}

/// A table row's cells: split on `|`, but not one escaped or in code.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") { &t[..t.len() - 1] } else { t };
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut ticks = 0;
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '`' => {
                let mut n = 1;
                while chars.peek() == Some(&'`') {
                    chars.next();
                    n += 1;
                }
                cell.extend(std::iter::repeat_n('`', n));
                ticks = if ticks == 0 { n } else if ticks == n { 0 } else { ticks };
            }
            '|' if ticks == 0 => out.push(std::mem::take(&mut cell)),
            c => cell.push(c),
        }
    }
    out.push(cell);
    out.into_iter().map(|c| c.trim().to_string()).collect()
}

/// The alignments a table's delimiter row gives, if it is one.
fn delimiter(line: &str) -> Option<Vec<&'static str>> {
    if !line.contains('-') {
        return None;
    }
    let row = cells(line);
    row.iter()
        .map(|c| {
            let inner = c.trim_start_matches(':').trim_end_matches(':');
            if inner.is_empty() || !inner.chars().all(|x| x == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => "center",
                (false, true) => "right",
                (true, false) => "left",
                _ => "",
            })
        })
        .collect()
}

fn table_start(lines: &[String], i: usize) -> bool {
    lines[i].contains('|') && indent(&lines[i]) <= 3 && lines.get(i + 1).and_then(|l| delimiter(l)).is_some_and(|d| d.len() == cells(&lines[i]).len())
}

/// Whether this line starts a block other than a paragraph (so it ends
/// one: a paragraph has no lazy line that's a heading).
fn interrupts(lines: &[String], i: usize) -> bool {
    let l = &lines[i];
    fence(l).is_some() || heading(l).is_some() || rule(l) || quote(l).is_some() || table_start(lines, i) || item(l).is_some_and(|(ordered, n, _, col)| !(ordered && n != 1) && !blank(&l[col.min(l.len())..]))
}

fn blocks(lines: &[String], o: &Options, ids: &mut BTreeMap<String, usize>, out: &mut String) {
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        if let Some((c, n, info)) = fence(line) {
            let strip = indent(line);
            let lang: String = info.split_whitespace().next().unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '+').collect();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = &lines[i];
                if fence(l).is_some_and(|(c2, n2, info2)| c2 == c && n2 >= n && info2.is_empty()) {
                    i += 1;
                    break;
                }
                body.push(&l[indent(l).min(strip)..]);
                i += 1;
            }
            let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{lang}\"") };
            out.push_str(&format!("<pre><code{class}>{}</code></pre>\n", esc(&body.iter().map(|l| format!("{l}\n")).collect::<String>())));
            continue;
        }
        if let Some((level, text)) = heading(line) {
            let level = (level + o.shift).min(6);
            let id = slug(text, ids);
            out.push_str(&format!("<h{level} id=\"{}{}\">{}</h{level}>\n", esc(o.id_prefix), esc(&id), inline(text, o)));
            i += 1;
            continue;
        }
        if rule(line) {
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }
        if quote(line).is_some() {
            let mut inner = Vec::new();
            while i < lines.len() && !blank(&lines[i]) {
                match quote(&lines[i]) {
                    Some(rest) => inner.push(rest.to_string()),
                    None if !interrupts(lines, i) => inner.push(lines[i].clone()),
                    None => break,
                }
                i += 1;
            }
            out.push_str("<blockquote>\n");
            blocks(&inner, o, ids, out);
            out.push_str("</blockquote>\n");
            continue;
        }
        if table_start(lines, i) {
            let head = cells(line);
            let align = delimiter(&lines[i + 1]).unwrap_or_default();
            let attr = |k: usize| match align.get(k) {
                Some(a) if !a.is_empty() => format!(" style=\"text-align:{a}\""),
                _ => String::new(),
            };
            out.push_str("<div class=\"table\"><table>\n<thead><tr>");
            for (k, cell) in head.iter().enumerate() {
                out.push_str(&format!("<th{}>{}</th>", attr(k), inline(cell, o)));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            i += 2;
            while i < lines.len() && !blank(&lines[i]) && lines[i].contains('|') && !(fence(&lines[i]).is_some() || heading(&lines[i]).is_some()) {
                let row = cells(&lines[i]);
                out.push_str("<tr>");
                for k in 0..head.len() {
                    out.push_str(&format!("<td{}>{}</td>", attr(k), inline(row.get(k).map(String::as_str).unwrap_or_default(), o)));
                }
                out.push_str("</tr>\n");
                i += 1;
            }
            out.push_str("</tbody>\n</table></div>\n");
            continue;
        }
        if let Some((ordered, start, mark, _)) = item(line) {
            i = list(lines, i, ordered, start, mark, o, ids, out);
            continue;
        }
        // A paragraph: lines until a blank or another block.
        let mut text = vec![line.trim_start().to_string()];
        i += 1;
        while i < lines.len() && !blank(&lines[i]) && !interrupts(lines, i) {
            text.push(lines[i].trim_start().to_string());
            i += 1;
        }
        out.push_str(&format!("<p>{}</p>\n", inline(&text.join("\n"), o)));
    }
}

/// A list from its first item at `lines[i]`; where it ended.
#[allow(clippy::too_many_arguments)]
fn list(lines: &[String], mut i: usize, ordered: bool, start: u64, mark: char, o: &Options, ids: &mut BTreeMap<String, usize>, out: &mut String) -> usize {
    let mut items: Vec<Vec<String>> = Vec::new();
    let mut loose = false;
    while i < lines.len() {
        let Some((o2, _, m2, col)) = item(&lines[i]) else { break };
        if o2 != ordered || m2 != mark {
            break;
        }
        let first = &lines[i];
        let mut body = vec![first.get(col..).unwrap_or_default().to_string()];
        i += 1;
        let mut after_blank = false;
        while i < lines.len() {
            let l = &lines[i];
            if blank(l) {
                // A blank line stays in the item when what follows it does.
                let next = lines[i..].iter().position(|l| !blank(l)).map(|k| i + k);
                match next {
                    Some(n) if indent(&lines[n]) >= col => {
                        body.extend(std::iter::repeat_n(String::new(), n - i));
                        after_blank = true;
                        i = n;
                        continue;
                    }
                    _ => break,
                }
            }
            if indent(l) >= col {
                body.push(l[col..].to_string());
            } else if !after_blank && !interrupts(lines, i) && item(l).is_none() && !body.last().is_some_and(|b| blank(b) || fence(b).is_some()) {
                // A lazy line, carrying the paragraph on.
                body.push(l.trim_start().to_string());
            } else {
                break;
            }
            i += 1;
        }
        loose |= gap(&body);
        items.push(body);
        // A blank line between two items makes the list loose.
        if i < lines.len() && blank(&lines[i]) {
            let next = lines[i..].iter().position(|l| !blank(l)).map(|k| i + k);
            match next.and_then(|n| item(&lines[n]).map(|it| (n, it))) {
                Some((n, (o3, _, m3, _))) if o3 == ordered && m3 == mark => {
                    loose = true;
                    i = n;
                }
                _ => break,
            }
        }
    }
    let tag = if ordered { "ol" } else { "ul" };
    let start_attr = if ordered && start != 1 { format!(" start=\"{start}\"") } else { String::new() };
    let class = if loose { "" } else { " class=\"tight\"" };
    out.push_str(&format!("<{tag}{start_attr}{class}>\n"));
    for body in items {
        out.push_str("<li>");
        let mut inner = String::new();
        blocks(&body, o, ids, &mut inner);
        out.push_str(inner.trim_end());
        out.push_str("</li>\n");
    }
    out.push_str(&format!("</{tag}>\n"));
    i
}

/// Whether a blank line, not one in a fenced block, comes between two
/// of an item's blocks: a loose list's paragraphs.
fn gap(body: &[String]) -> bool {
    let mut open: Option<(char, usize)> = None;
    for (k, l) in body.iter().enumerate() {
        if let Some((c, n)) = open {
            if fence(l).is_some_and(|(c2, n2, info)| c2 == c && n2 >= n && info.is_empty()) {
                open = None;
            }
            continue;
        }
        if let Some((c, n, _)) = fence(l) {
            open = Some((c, n));
        } else if blank(l) && body[..k].iter().any(|b| !blank(b)) && body[k + 1..].iter().any(|b| !blank(b)) {
            return true;
        }
    }
    false
}

/// A heading's id, GitHub's way: lower case, letters, digits, - and _,
/// spaces as -; `-1`, `-2` after one already given.
fn slug(text: &str, ids: &mut BTreeMap<String, usize>) -> String {
    let plain = plain_text(text);
    let mut s: String = plain.to_lowercase().chars().filter_map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { Some(c) } else if c == ' ' { Some('-') } else { None }).collect();
    if s.is_empty() {
        s = "section".into();
    }
    let n = ids.entry(s.clone()).or_insert(0);
    let id = if *n == 0 { s } else { format!("{s}-{n}") };
    *n += 1;
    id
}

/// What a heading reads as, without its marks or its links' URLs: for
/// its id, and a picture's alt text.
fn plain_text(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '`' | '*' | '~' | '[' => {}
            ']' if chars.peek() == Some(&'(') => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            ']' => {}
            c => out.push(c),
        }
    }
    out
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// A run of text's HTML: code, links, pictures, emphasis; the rest
/// escaped.
pub(crate) fn inline(text: &str, o: &Options) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    spans(&chars, o, &mut out, true);
    out
}

fn spans(chars: &[char], o: &Options, out: &mut String, links: bool) {
    let mut i = 0;
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut String| {
        out.push_str(&esc(text));
        text.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        // \ before punctuation: that character as it is. Before a line's
        // end: a line break.
        if c == '\\' {
            match chars.get(i + 1) {
                Some(n) if is_punct(*n) => {
                    text.push(*n);
                    i += 2;
                    continue;
                }
                Some('\n') => {
                    flush(&mut text, out);
                    out.push_str("<br>\n");
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        if c == '\n' {
            // Two spaces before it: a line break.
            let trailing = text.len() - text.trim_end_matches(' ').len();
            let t = text.trim_end_matches(' ').to_string();
            text = t;
            flush(&mut text, out);
            out.push_str(if trailing >= 2 { "<br>\n" } else { "\n" });
            i += 1;
            continue;
        }
        if c == '`' {
            let n = run(chars, i, '`');
            if let Some(end) = find_run(chars, i + n, '`', n) {
                flush(&mut text, out);
                let code: String = chars[i + n..end].iter().map(|c| if *c == '\n' { ' ' } else { *c }).collect();
                let code = if code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() { code[1..code.len() - 1].to_string() } else { code };
                out.push_str(&format!("<code>{}</code>", esc(&code)));
                i = end + n;
                continue;
            }
            text.extend(&chars[i..i + n]);
            i += n;
            continue;
        }
        if c == '<' {
            // <https://...>: a link to itself.
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '>' || *c == '<' || c.is_whitespace()).map(|k| i + 1 + k).filter(|e| chars[*e] == '>') {
                let url: String = chars[i + 1..end].iter().collect();
                let lower = url.to_ascii_lowercase();
                if links && (lower.starts_with("https://") || lower.starts_with("http://")) && safe_url(&url) {
                    flush(&mut text, out);
                    out.push_str(&format!("<a href=\"{}\">{}</a>", esc(&url), esc(&url)));
                    i = end + 1;
                    continue;
                }
            }
        }
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            if let Some((label, url, end)) = link_at(chars, i + 1) {
                flush(&mut text, out);
                let alt = plain_text(&label.iter().collect::<String>());
                image(&alt, &url, o, out, links);
                i = end;
                continue;
            }
        }
        if c == '[' && links {
            if let Some((label, url, end)) = link_at(chars, i) {
                flush(&mut text, out);
                let mut inner = String::new();
                spans(&label, o, &mut inner, false);
                match target(&url, o) {
                    Some(href) => out.push_str(&format!("<a href=\"{}\">{inner}</a>", esc(&href))),
                    None => out.push_str(&inner),
                }
                i = end;
                continue;
            }
        }
        if c == '*' || c == '_' || c == '~' {
            let n = run(chars, i, c);
            if let Some((inner, end, strong)) = emphasis(chars, i, c, n) {
                flush(&mut text, out);
                let tag = match (c, strong) {
                    ('~', _) => "del",
                    (_, true) => "strong",
                    _ => "em",
                };
                let mut inner_html = String::new();
                spans(inner, o, &mut inner_html, links);
                out.push_str(&format!("<{tag}>{inner_html}</{tag}>"));
                i = end;
                continue;
            }
            text.extend(&chars[i..i + n]);
            i += n;
            continue;
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, out);
}

/// How many of `c` start at `i`.
fn run(chars: &[char], i: usize, c: char) -> usize {
    chars[i..].iter().take_while(|x| **x == c).count()
}

/// The next run of exactly `n` of `c` at or after `from`.
fn find_run(chars: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut k = from;
    while k < chars.len() {
        if chars[k] == c {
            let m = run(chars, k, c);
            if m == n {
                return Some(k);
            }
            k += m;
        } else {
            k += 1;
        }
    }
    None
}

/// `*x*`, `**x**`, `_x_`, `__x__` or `~~x~~` at `i`: what's inside,
/// where it ends, and whether it's strong.
fn emphasis(chars: &[char], i: usize, c: char, n: usize) -> Option<(&[char], usize, bool)> {
    let width = match (c, n) {
        ('~', 2) => 2,
        ('~', _) => return None,
        (_, 1) => 1,
        (_, 2) => 2,
        _ => return None,
    };
    let before = if i == 0 { None } else { Some(chars[i - 1]) };
    let open_next = *chars.get(i + width)?;
    if open_next.is_whitespace() {
        return None;
    }
    // An underscore inside a word (snake_case) is just an underscore.
    if c == '_' && before.is_some_and(|b| b.is_alphanumeric()) {
        return None;
    }
    let mut k = i + width;
    while k < chars.len() {
        if chars[k] == '`' {
            // Code inside: skip past it.
            let m = run(chars, k, '`');
            k = find_run(chars, k + m, '`', m).map(|e| e + m).unwrap_or(k + m);
            continue;
        }
        if chars[k] == '\\' {
            k += 2;
            continue;
        }
        if chars[k] == c {
            let m = run(chars, k, c);
            let after = chars.get(k + m);
            let closes = m == width && !chars[k - 1].is_whitespace() && !(c == '_' && after.is_some_and(|a| a.is_alphanumeric()));
            if closes && k > i + width {
                return Some((&chars[i + width..k], k + m, width == 2 && c != '~'));
            }
            k += m;
            continue;
        }
        k += 1;
    }
    None
}

/// `[label](url "title")` at `i`: its label, URL and where it ends.
fn link_at(chars: &[char], i: usize) -> Option<(Vec<char>, String, usize)> {
    let mut depth = 0;
    let mut k = i;
    let close = loop {
        match chars.get(k)? {
            '\\' => k += 1,
            '`' => {
                let m = run(chars, k, '`');
                k = find_run(chars, k + m, '`', m).map(|e| e + m - 1).unwrap_or(k + m - 1);
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break k;
                }
            }
            _ => {}
        }
        k += 1;
    };
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut k = close + 2;
    while chars.get(k).is_some_and(|c| *c == ' ') {
        k += 1;
    }
    let mut url = String::new();
    if chars.get(k) == Some(&'<') {
        k += 1;
        while let Some(c) = chars.get(k) {
            if *c == '>' {
                break;
            }
            if *c == '\n' || *c == '<' {
                return None;
            }
            url.push(*c);
            k += 1;
        }
        k += 1;
    } else {
        let mut parens = 0;
        while let Some(&c) = chars.get(k) {
            if c.is_whitespace() || (c == ')' && parens == 0) {
                break;
            }
            if c == '(' {
                parens += 1;
            }
            if c == ')' {
                parens -= 1;
            }
            if c == '\\' && chars.get(k + 1).is_some_and(|n| is_punct(*n)) {
                k += 1;
                url.push(chars[k]);
            } else {
                url.push(c);
            }
            k += 1;
        }
    }
    while chars.get(k).is_some_and(|c| c.is_whitespace()) {
        k += 1;
    }
    // A title, which is dropped.
    if let Some(&q) = chars.get(k).filter(|c| matches!(c, '"' | '\'' | '(')) {
        let end = if q == '(' { ')' } else { q };
        k += 1;
        while chars.get(k).is_some_and(|c| *c != end) {
            k += 1;
        }
        k += 1;
        while chars.get(k).is_some_and(|c| c.is_whitespace()) {
            k += 1;
        }
    }
    if chars.get(k) != Some(&')') {
        return None;
    }
    Some((chars[i + 1..close].to_vec(), url, k + 1))
}

/// Where a link goes: http(s) as it is, an anchor into this document,
/// anything relative where the caller says.
fn target(url: &str, o: &Options) -> Option<String> {
    if !safe_url(url) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(url.to_string());
    }
    if let Some(anchor) = url.strip_prefix('#') {
        return Some(format!("#{}{anchor}", o.id_prefix));
    }
    (o.link)(url).filter(|u| safe_url(u))
}

fn image(alt: &str, url: &str, o: &Options, out: &mut String, links: bool) {
    let lower = url.to_ascii_lowercase();
    let remote = lower.starts_with("http://") || lower.starts_with("https://");
    let label = if alt.trim().is_empty() { "a picture".to_string() } else { alt.to_string() };
    if safe_url(url) && !remote {
        if let Some(src) = (o.image)(url).filter(|u| safe_url(u)) {
            out.push_str(&format!("<img src=\"{}\" alt=\"{}\" loading=\"lazy\">", esc(&src), esc(alt)));
            return;
        }
    }
    if remote && safe_url(url) && links {
        out.push_str(&format!("<a href=\"{}\">{} (picture)</a>", esc(url), esc(&label)));
    } else {
        out.push_str(&esc(&label));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(text: &str) -> String {
        let link = |u: &str| Some(format!("https://example.com/{u}"));
        let image = |u: &str| (u == "shot.png").then(|| format!("media/{u}"));
        render(text, &Options { id_prefix: "r-", shift: 1, link: &link, image: &image })
    }

    #[test]
    fn headings_paragraphs_and_rules() {
        assert_eq!(md("# Title\n\nSome *text*\nand **more**.\n\n---\n## Next ##"), "<h2 id=\"r-title\">Title</h2>\n<p>Some <em>text</em>\nand <strong>more</strong>.</p>\n<hr>\n<h3 id=\"r-next\">Next</h3>\n");
        assert_eq!(md("## A\n## A\n#nope"), "<h3 id=\"r-a\">A</h3>\n<h3 id=\"r-a-1\">A</h3>\n<p>#nope</p>\n");
        assert_eq!(md("###### deep"), "<h6 id=\"r-deep\">deep</h6>\n", "never past h6");
        assert!(md("## `fenix.http`'s keys").contains("id=\"r-fenixhttps-keys\""));
    }

    #[test]
    fn html_is_shown_not_run() {
        let out = md("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)> & \"q\" 'a'");
        assert!(!out.contains("<script") && !out.contains("<img"), "{out}");
        assert!(out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(out.contains("&amp; &quot;q&quot; &#39;a&#39;"));
        assert_eq!(md("`<b>` and <file>"), "<p><code>&lt;b&gt;</code> and &lt;file&gt;</p>\n");
        assert_eq!(md("```html\n<b>x</b>\n```"), "<pre><code class=\"language-html\">&lt;b&gt;x&lt;/b&gt;\n</code></pre>\n");
        assert_eq!(md("```\"><script>\nx\n```"), "<pre><code class=\"language-script\">x\n</code></pre>\n", "a fence's language is letters only");
    }

    #[test]
    fn links_go_only_where_they_may() {
        assert_eq!(md("[a](https://x.org/a_(b)) [b](#the-format)"), "<p><a href=\"https://x.org/a_(b)\">a</a> <a href=\"#r-the-format\">b</a></p>\n");
        assert_eq!(md("[c](docs/x.md \"title\")"), "<p><a href=\"https://example.com/docs/x.md\">c</a></p>\n");
        for bad in ["javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,x", "//evil.org/x", "vbscript:x", "java\nscript:x", "file:///etc/passwd"] {
            let out = md(&format!("[bad]({bad})"));
            assert!(!out.contains("href"), "{bad}: {out}");
        }
        assert_eq!(md("[x](https://a.org/\"onclick=\"y)"), "<p>x</p>\n", "a quote never reaches an attribute");
        assert_eq!(md("<https://x.org> <javascript:x>"), "<p><a href=\"https://x.org\">https://x.org</a> &lt;javascript:x&gt;</p>\n");
        assert_eq!(md("[no link"), "<p>[no link</p>\n");
        assert_eq!(md("[`a]`](https://x.org)"), "<p><a href=\"https://x.org\"><code>a]</code></a></p>\n");
        assert!(safe_url("https://x.org") && safe_url("a/b.png") && safe_url("#x") && safe_url("?q=1") && !safe_url("https://") && !safe_url("mailto:a@b.c"));
    }

    #[test]
    fn pictures_are_ours_or_links() {
        assert_eq!(md("![A shot](shot.png)"), "<p><img src=\"media/shot.png\" alt=\"A shot\" loading=\"lazy\"></p>\n");
        assert_eq!(md("![gone](gone.png)"), "<p>gone</p>\n");
        assert_eq!(md("![far](https://x.org/a.png)"), "<p><a href=\"https://x.org/a.png\">far (picture)</a></p>\n", "nothing loaded from elsewhere");
        assert_eq!(md("![x\" onerror=\"y](shot.png)"), "<p><img src=\"media/shot.png\" alt=\"x&quot; onerror=&quot;y\" loading=\"lazy\"></p>\n");
    }

    #[test]
    fn emphasis_and_code() {
        assert_eq!(md("snake_case_name and _this_ and __that__ and ~~gone~~"), "<p>snake_case_name and <em>this</em> and <strong>that</strong> and <del>gone</del></p>\n");
        assert_eq!(md("a * b * c and 2*3*4"), "<p>a * b * c and 2<em>3</em>4</p>\n");
        assert_eq!(md("`` a ` b `` and `x*y*z`"), "<p><code>a ` b</code> and <code>x*y*z</code></p>\n");
        assert_eq!(md("\\*not\\* \\<b\\>"), "<p>*not* &lt;b&gt;</p>\n");
        assert_eq!(md("**`code` in bold**"), "<p><strong><code>code</code> in bold</strong></p>\n");
        assert_eq!(md("an `unclosed tick"), "<p>an `unclosed tick</p>\n");
        assert_eq!(md("line  \nbreak"), "<p>line<br>\nbreak</p>\n");
    }

    #[test]
    fn lists_nest_and_carry_blocks() {
        assert_eq!(md("- one\n- two\n  carried on\n- three"), "<ul class=\"tight\">\n<li><p>one</p></li>\n<li><p>two\ncarried on</p></li>\n<li><p>three</p></li>\n</ul>\n");
        assert_eq!(md("- a\n  - b\n  - c\n- d"), "<ul class=\"tight\">\n<li><p>a</p>\n<ul class=\"tight\">\n<li><p>b</p></li>\n<li><p>c</p></li>\n</ul></li>\n<li><p>d</p></li>\n</ul>\n");
        assert_eq!(md("3. x\n4. y"), "<ol start=\"3\" class=\"tight\">\n<li><p>x</p></li>\n<li><p>y</p></li>\n</ol>\n");
        let table = md("- Comments:\n\n  | A | B |\n  | --- | --- |\n  | `x` | y |\n\n- Next");
        assert!(table.starts_with("<ul>\n<li><p>Comments:</p>\n<div class=\"table\"><table>"), "{table}");
        assert!(table.contains("<td><code>x</code></td><td>y</td>") && table.contains("<li><p>Next</p></li>"), "{table}");
        let code = md("1. Run it:\n\n   ```sh\n   cargo run\n   ```\n2. Done");
        assert!(code.contains("<pre><code class=\"language-sh\">cargo run\n</code></pre>") && code.contains("<li><p>Done</p></li>"), "{code}");
        assert_eq!(md("- a\n\npara"), "<ul class=\"tight\">\n<li><p>a</p></li>\n</ul>\n<p>para</p>\n");
        assert_eq!(md("text\n- item"), "<p>text</p>\n<ul class=\"tight\">\n<li><p>item</p></li>\n</ul>\n");
        assert_eq!(md("- a\n* b"), "<ul class=\"tight\">\n<li><p>a</p></li>\n</ul>\n<ul class=\"tight\">\n<li><p>b</p></li>\n</ul>\n");
    }

    #[test]
    fn tables_align_and_keep_pipes_in_code() {
        let out = md("| Key | What |\n|:---|---:|\n| `a|b` | x \\| y |\n| short |");
        assert_eq!(out, "<div class=\"table\"><table>\n<thead><tr><th style=\"text-align:left\">Key</th><th style=\"text-align:right\">What</th></tr></thead>\n<tbody>\n<tr><td style=\"text-align:left\"><code>a|b</code></td><td style=\"text-align:right\">x | y</td></tr>\n<tr><td style=\"text-align:left\">short</td><td style=\"text-align:right\"></td></tr>\n</tbody>\n</table></div>\n");
        assert_eq!(md("a | b\nnot a table"), "<p>a | b\nnot a table</p>\n");
    }

    #[test]
    fn quotes_and_fences() {
        assert_eq!(md("> quoted\n> **on**"), "<blockquote>\n<p>quoted\n<strong>on</strong></p>\n</blockquote>\n");
        assert_eq!(md("~~~\n```\nin\n~~~"), "<pre><code>```\nin\n</code></pre>\n");
        assert_eq!(md("```\nnever closed"), "<pre><code>never closed\n</code></pre>\n");
        assert_eq!(md("\tx\ty"), "<p>x   y</p>\n");
    }
}
