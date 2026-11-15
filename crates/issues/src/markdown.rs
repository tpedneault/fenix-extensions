//! Markdown shown readably in a page: wrapped to the width, headings and
//! code told apart, nothing of it run or followed. A body is someone
//! else's text, so it's cleaned first (`fenix_tracker::clean`).

use fenix_ui_kit::text::wrap;
use fenix_ui_kit::Role;

/// `text`'s lines at `width`, each with how it's drawn: a heading as a
/// title, a fenced block and a quote muted, the rest as text.
pub fn lines(text: &str, width: usize) -> Vec<(String, Role)> {
    let (clean, _) = fenix_tracker::clean::text(text, usize::MAX);
    let width = width.max(10);
    let mut out = Vec::new();
    let mut fenced = false;
    for line in clean.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            out.push((fenix_ui_kit::text::fit(line, width), Role::Muted));
            continue;
        }
        let role = if fenced || trimmed.starts_with('>') || trimmed.starts_with("<!--") {
            Role::Muted
        } else if trimmed.starts_with('#') {
            Role::Title
        } else {
            Role::Text
        };
        let line = line.replace('\t', "    ");
        if fenced {
            out.push((fenix_ui_kit::text::fit(&line, width), role));
        } else if line.trim().is_empty() {
            out.push((String::new(), role));
        } else {
            out.extend(wrap(&line, width).into_iter().map(|l| (l, role.clone())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_code_and_quotes_are_told_apart_and_text_wraps() {
        let shown = lines("# Title\r\nSome words that go on for a while\n```\nlet x = 1;\n```\n> quoted\n\u{1b}[31mred", 20);
        let roles: Vec<&Role> = shown.iter().map(|(_, r)| r).collect();
        assert_eq!(shown[0], ("# Title".to_string(), Role::Title));
        assert!(shown.iter().filter(|(_, r)| *r == Role::Text).count() >= 2, "{shown:?}");
        assert!(shown.iter().any(|(l, r)| l == "let x = 1;" && *r == Role::Muted));
        assert!(shown.iter().any(|(l, r)| l == "> quoted" && *r == Role::Muted));
        assert_eq!(shown.last().unwrap().0, "[31mred", "an escape code shown, never run");
        assert!(shown.iter().all(|(l, _)| fenix_ui_kit::text::width(l) <= 20), "{roles:?}");
    }
}
