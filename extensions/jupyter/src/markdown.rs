//! A markdown cell as it reads: headings, wrapped paragraphs with their
//! emphasis and code coloured, lists, quotes, code blocks on a panel,
//! rules. Not all of CommonMark -- what notebooks' prose has.

use fenix_ui_kit::text::width;
use fenix_ui_kit::Role;

/// A line of a cell as it's drawn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MdLine {
    pub spans: Vec<(String, Role)>,
    /// A code block's line: drawn on a panel.
    pub code: bool,
    /// A rule across the cell.
    pub rule: bool,
}

impl MdLine {
    fn of(spans: Vec<(String, Role)>) -> MdLine {
        MdLine { spans, ..MdLine::default() }
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        self.spans.iter().map(|(t, _)| t.as_str()).collect()
    }
}

/// `source` laid out `w` cells wide.
pub fn render(source: &str, w: usize) -> Vec<MdLine> {
    let w = w.max(10);
    let mut out: Vec<MdLine> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut fence: Option<String> = None;
    let flush = |para: &mut Vec<String>, out: &mut Vec<MdLine>| {
        if !para.is_empty() {
            out.extend(wrap(&inline(&para.join(" "), Role::Text), w, "", "").into_iter().map(MdLine::of));
            para.clear();
        }
    };
    for raw in source.lines() {
        let line = raw.trim_end();
        let lead = line.trim_start();
        if let Some(f) = &fence {
            if lead.starts_with(f.as_str()) {
                fence = None;
            } else {
                out.push(MdLine { spans: vec![(line.replace('\t', "    "), Role::Syntax("string".into()))], code: true, rule: false });
            }
            continue;
        }
        if lead.starts_with("```") || lead.starts_with("~~~") {
            flush(&mut para, &mut out);
            fence = Some(lead[..3].to_string());
            continue;
        }
        if lead.is_empty() {
            flush(&mut para, &mut out);
            if out.last().is_some_and(|l| !l.spans.is_empty() || l.rule || l.code) {
                out.push(MdLine::default());
            }
            continue;
        }
        let hashes = lead.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && lead[hashes..].starts_with(' ') {
            flush(&mut para, &mut out);
            let title = lead[hashes..].trim().trim_end_matches('#').trim();
            let role = if hashes <= 2 { Role::Title } else { Role::Accent };
            out.extend(wrap(&inline(title, role.clone()), w, "", "").into_iter().map(|mut l| {
                for s in &mut l {
                    if s.1 == Role::Text {
                        s.1 = role.clone();
                    }
                }
                MdLine::of(l)
            }));
            continue;
        }
        if lead.len() >= 3 && ["-", "*", "_"].iter().any(|m| lead.chars().all(|c| c == m.chars().next().unwrap() || c == ' ') && lead.contains(m)) {
            flush(&mut para, &mut out);
            out.push(MdLine { rule: true, ..MdLine::default() });
            continue;
        }
        if let Some(rest) = lead.strip_prefix('>') {
            flush(&mut para, &mut out);
            out.extend(wrap(&inline(rest.trim_start(), Role::Muted), w, "│ ", "│ ").into_iter().map(|mut l| {
                if let Some(first) = l.first_mut() {
                    first.1 = Role::Muted;
                }
                MdLine::of(l)
            }));
            continue;
        }
        if let Some((marker, rest)) = item(lead) {
            flush(&mut para, &mut out);
            let indent = " ".repeat((line.len() - lead.len()).min(8));
            let first = format!("{indent}{marker} ");
            let hang = " ".repeat(width(&first));
            out.extend(wrap(&inline(rest, Role::Text), w, &first, &hang).into_iter().map(MdLine::of));
            continue;
        }
        if lead.starts_with('|') {
            // A table, as it's written: its cells lined up by the writer.
            flush(&mut para, &mut out);
            let row = if lead.chars().all(|c| "|-: ".contains(c)) { lead.replace('-', "─") } else { lead.to_string() };
            out.push(MdLine::of(vec![(row, Role::Text)]));
            continue;
        }
        para.push(lead.to_string());
    }
    flush(&mut para, &mut out);
    while out.last().is_some_and(|l| l.spans.is_empty() && !l.rule) {
        out.pop();
    }
    out
}

/// A list item's marker and its text: `- x`, `* x`, `1. x`, `- [ ] x`.
fn item(lead: &str) -> Option<(String, &str)> {
    let rest = ["- ", "* ", "+ "].iter().find_map(|m| lead.strip_prefix(m));
    if let Some(rest) = rest {
        if let Some(r) = rest.strip_prefix("[ ] ") {
            return Some(("☐".into(), r));
        }
        if let Some(r) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
            return Some(("☑".into(), r));
        }
        return Some(("•".into(), rest));
    }
    let digits = lead.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 4 {
        let after = &lead[digits..];
        if let Some(r) = after.strip_prefix(". ").or_else(|| after.strip_prefix(") ")) {
            return Some((format!("{}.", &lead[..digits]), r));
        }
    }
    None
}

/// A line's emphasis, code and links, as coloured runs; `base` is the
/// colour of the rest.
fn inline(text: &str, base: Role) -> Vec<(String, Role)> {
    let mut out: Vec<(String, Role)> = Vec::new();
    let push = |out: &mut Vec<(String, Role)>, s: &str, role: Role| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.1 == role => last.0.push_str(s),
            _ => out.push((s.to_string(), role)),
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut plain = String::new();
    let find = |from: usize, pat: &[char]| (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..j + pat.len()] == *pat);
    while i < chars.len() {
        let c = chars[i];
        let mut took = None;
        if c == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                took = Some((chars[i + 1..end].iter().collect::<String>(), Role::Syntax("string".into()), end + 1));
            }
        } else if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            if let Some(end) = find(i + 2, &[c, c]).filter(|&e| e > i + 2) {
                took = Some((chars[i + 2..end].iter().collect(), Role::Title, end + 2));
            }
        } else if (c == '*' || c == '_') && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) && (c == '*' || i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some(end) = find(i + 1, &[c]).filter(|&e| e > i + 1 && !chars[e - 1].is_whitespace()) {
                took = Some((chars[i + 1..end].iter().collect(), base.clone(), end + 1));
            }
        } else if c == '!' && chars.get(i + 1) == Some(&'[') {
            if let Some((label, _, end)) = link(&chars, i + 1) {
                took = Some((format!("[picture: {}]", if label.is_empty() { "untitled".into() } else { label }), Role::Muted, end));
            }
        } else if c == '[' {
            if let Some((label, _, end)) = link(&chars, i) {
                took = Some((label, Role::Accent, end));
            }
        } else if c == '<' {
            // An HTML tag (`<br>`, `<b>`): left out. An autolink keeps its text.
            if let Some(end) = find(i + 1, &['>']) {
                let tag: String = chars[i + 1..end].iter().collect();
                if tag.starts_with("http") {
                    took = Some((tag, Role::Accent, end + 1));
                } else if tag.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '/') {
                    took = Some((String::new(), base.clone(), end + 1));
                }
            }
        }
        match took {
            Some((t, role, next)) => {
                push(&mut out, &std::mem::take(&mut plain), base.clone());
                push(&mut out, &t, role);
                i = next;
            }
            None => {
                plain.push(c);
                i += 1;
            }
        }
    }
    push(&mut out, &plain, base);
    out
}

/// `[label](target)` at `at`: its label, its target, and where it ends.
fn link(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let close = (at + 1..chars.len()).find(|&j| chars[j] == ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = (close + 2..chars.len()).find(|&j| chars[j] == ')')?;
    Some((chars[at + 1..close].iter().collect(), chars[close + 2..end].iter().collect(), end + 1))
}

/// Runs wrapped `w` wide at their spaces: the first line after `first`,
/// the rest after `hang`.
fn wrap(runs: &[(String, Role)], w: usize, first: &str, hang: &str) -> Vec<Vec<(String, Role)>> {
    let mut lines: Vec<Vec<(String, Role)>> = Vec::new();
    let mut line: Vec<(String, Role)> = vec![(first.to_string(), Role::Accent)];
    let mut used = width(first);
    let mut fresh = true;
    // Words, each keeping the colour of the run it's in.
    let mut words: Vec<(String, Role, bool)> = Vec::new();
    // A space before a word, though it ended the run before it.
    let mut space_before = false;
    for (text, role) in runs {
        let mut word = String::new();
        for c in text.chars() {
            if c == ' ' {
                if !word.is_empty() {
                    words.push((std::mem::take(&mut word), role.clone(), space_before));
                }
                space_before = true;
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            // A word that goes on into the next run (`**x**,`) is one.
            words.push((word, role.clone(), space_before));
            space_before = text.ends_with(' ');
        }
    }
    let put = |line: &mut Vec<(String, Role)>, s: &str, role: &Role| match line.last_mut() {
        Some(last) if last.1 == *role => last.0.push_str(s),
        _ => line.push((s.to_string(), role.clone())),
    };
    for (i, (word, role, space)) in words.iter().enumerate() {
        let gap = usize::from(*space && !fresh && i > 0);
        let ww = width(word);
        if !fresh && used + gap + ww > w {
            lines.push(std::mem::take(&mut line));
            line.push((hang.to_string(), Role::Accent));
            used = width(hang);
            fresh = true;
        }
        if gap == 1 && !fresh {
            // A space inside an emphasis is its; one beside it, plain.
            let before = line.last().map_or(Role::Text, |l| l.1.clone());
            put(&mut line, " ", if before == *role { role } else { &Role::Text });
            used += 1;
        }
        // A word wider than the line: cut where it must be.
        let mut rest = word.as_str();
        while used + width(rest) > w && width(rest) > 1 && used < w {
            let mut cut = 0;
            let mut cw = 0;
            for (b, c) in rest.char_indices() {
                let x = fenix_ui_kit::text::char_width(c);
                if used + cw + x > w {
                    break;
                }
                cw += x;
                cut = b + c.len_utf8();
            }
            if cut == 0 {
                break;
            }
            put(&mut line, &rest[..cut], role);
            lines.push(std::mem::take(&mut line));
            line.push((hang.to_string(), Role::Accent));
            used = width(hang);
            rest = &rest[cut..];
        }
        put(&mut line, rest, role);
        used += width(rest);
        fresh = false;
    }
    lines.push(line);
    lines.into_iter().map(|l| l.into_iter().filter(|(t, _)| !t.is_empty()).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[MdLine]) -> Vec<String> {
        lines.iter().map(MdLine::text).collect()
    }

    #[test]
    fn headings_paragraphs_and_lists() {
        let md = "# Assignment 2\n\nWork out the **area** of a circle,\nthen plot `r²`.\n\n- radius in metres\n- show your working\n1. first";
        let lines = render(md, 40);
        assert_eq!(texts(&lines), ["Assignment 2", "", "Work out the area of a circle, then plot", "r².", "", "• radius in metres", "• show your working", "1. first"]);
        assert_eq!(lines[0].spans[0].1, Role::Title);
        assert!(lines[2].spans.iter().any(|(t, r)| t == "area" && *r == Role::Title), "{:?}", lines[2].spans);
        assert!(lines[3].spans.iter().any(|(t, r)| t == "r²" && *r == Role::Syntax("string".into())));
    }

    #[test]
    fn a_list_item_hangs() {
        let lines = render("- one two three four five six", 14);
        assert_eq!(texts(&lines), ["• one two", "  three four", "  five six"]);
    }

    #[test]
    fn code_blocks_rules_quotes_and_links() {
        let md = "```python\nx = 1\n```\n---\n> said so\n\nsee [the docs](https://x.org) and ![plot](a.png)<br>";
        let lines = render(md, 60);
        assert_eq!(texts(&lines), ["x = 1", "", "│ said so", "", "see the docs and [picture: plot]"]);
        assert!(lines[0].code);
        assert!(lines[1].rule);
    }

    #[test]
    fn a_long_word_is_cut() {
        let lines = render("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(texts(&lines), ["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }

    #[test]
    fn snake_case_isnt_emphasis() {
        assert_eq!(texts(&render("call my_func_name now", 40)), ["call my_func_name now"]);
        assert_eq!(texts(&render("an *aside* here", 40)), ["an aside here"]);
    }
}
