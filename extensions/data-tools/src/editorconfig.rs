//! The indent `.editorconfig` gives a file.

use crate::doc::Indent;
use crate::glob::Glob;

#[derive(Default)]
struct Props {
    style: Option<String>,
    size: Option<String>,
    tab_width: Option<usize>,
}

/// The indent for a file, from the `.editorconfig` files above it,
/// nearest first: each its text, and the file's path from its folder
/// (`src/a.json`).
pub fn indent(configs: &[(String, String)]) -> Option<Indent> {
    // Up to the first that says it's the root, read from the top down so
    // the nearer win.
    let last = configs.iter().position(|(_, text)| is_root(text)).map_or(configs.len(), |i| i + 1);
    let mut props = Props::default();
    for (path, text) in configs[..last].iter().rev() {
        apply(text, path, &mut props);
    }
    let size = props.size.as_deref().map(str::trim);
    match props.style.as_deref() {
        Some("tab") => Some(Indent::Tab),
        Some("space") => match size {
            Some("tab") => props.tab_width.map(Indent::Spaces),
            Some(n) => n.parse().ok().map(Indent::Spaces),
            None => props.tab_width.map(Indent::Spaces),
        },
        _ => size.and_then(|n| n.parse().ok()).map(Indent::Spaces),
    }
}

fn is_root(text: &str) -> bool {
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            return false;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("root") && v.trim().eq_ignore_ascii_case("true") {
                return true;
            }
        }
    }
    false
}

fn apply(text: &str, path: &str, props: &mut Props) {
    let mut on = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let pattern = if section.contains('/') { section.trim_start_matches('/').to_string() } else { format!("**/{section}") };
            on = Glob::new(&pattern).matches(path);
            continue;
        }
        let (Some((k, v)), true) = (line.split_once('='), on) else { continue };
        let v = v.trim().to_ascii_lowercase();
        match k.trim().to_ascii_lowercase().as_str() {
            "indent_style" => props.style = Some(v),
            "indent_size" => props.size = Some(v),
            "tab_width" => props.tab_width = v.parse().ok(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_section_wins() {
        let top = "root = true\n[*]\nindent_style = space\nindent_size = 4\n\n[*.{json,yml}]\nindent_size = 2\n".to_string();
        assert_eq!(indent(&[("a/b.json".into(), top.clone())]), Some(Indent::Spaces(2)));
        assert_eq!(indent(&[("a/b.toml".into(), top.clone())]), Some(Indent::Spaces(4)));
        let near = "[*.json]\nindent_style = tab\n".to_string();
        assert_eq!(indent(&[("b.json".into(), near.clone()), ("a/b.json".into(), top.clone())]), Some(Indent::Tab));
        // A root nearer than another stops the search there.
        let root_near = "root = true\n[*]\nindent_size = 8\n".to_string();
        assert_eq!(indent(&[("b.json".into(), root_near), ("a/b.json".into(), top)]), Some(Indent::Spaces(8)));
    }

    #[test]
    fn paths_and_tab_widths() {
        let text = "[/config/*.json]\nindent_style = space\nindent_size = tab\ntab_width = 3\n".to_string();
        assert_eq!(indent(&[("config/a.json".into(), text.clone())]), Some(Indent::Spaces(3)));
        assert_eq!(indent(&[("other/config/a.json".into(), text)]), None);
        assert_eq!(indent(&[("a.json".into(), "# nothing\n".into())]), None);
    }
}
