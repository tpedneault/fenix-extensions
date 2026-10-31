//! Paths: the pictures beside one, in the order a person counts them
//! (`shot-2` before `shot-10`); where a full path Fenix gave it is in
//! the project (`/project/...`), where it may list a folder; and the
//! pictures a line of code or Markdown names, for the preview.

use std::cmp::Ordering;

use crate::format::is_image_name;

/// `a` and `b` compared as a person would: letters without case, runs of
/// digits by their value; case only when nothing else tells them apart.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    by_reading(a, b).then_with(|| a.cmp(b))
}

fn by_reading(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut run = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        run.push(c);
                        it.next();
                    }
                    run
                };
                let (ra, rb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (ra.trim_start_matches('0'), rb.trim_start_matches('0'));
                // By value: the longer run (its zeros left off) is bigger;
                // then digit by digit; then fewer leading zeros first.
                let by = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| ra.len().cmp(&rb.len()));
                if by != Ordering::Equal {
                    return by;
                }
            }
            (Some(x), Some(y)) => {
                let by = x.to_lowercase().cmp(y.to_lowercase());
                if by != Ordering::Equal {
                    return by;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The pictures among a folder's file names, in order.
pub fn pictures(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = names.into_iter().filter(|n| is_image_name(n)).collect();
    out.sort_by(|a, b| natural_cmp(a, b));
    out
}

/// The picture `step` along from `current` in `names` (`pictures`'
/// order), round from the last to the first: its index. A `current` that
/// isn't there (renamed, deleted) goes to where it would be.
pub fn step(names: &[String], current: &str, forward: bool) -> Option<usize> {
    if names.is_empty() {
        return None;
    }
    let n = names.len();
    Some(match names.iter().position(|x| x == current) {
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None => {
            let after = names.iter().position(|x| natural_cmp(x, current) == Ordering::Greater).unwrap_or(n);
            if forward {
                after % n
            } else {
                (after + n - 1) % n
            }
        }
    })
}

/// Where a full path (`C:\work\docs\a.png`, `/home/me/p/a.png`) may be
/// under `/project`, longest first: `/project/work/docs/a.png`, then
/// `/project/docs/a.png`, then `/project/a.png`. The first that's there,
/// and is the same file, is it.
pub fn guest_candidates(full: &str) -> Vec<String> {
    let parts: Vec<&str> = full.split(['/', '\\']).filter(|p| !p.is_empty() && !p.ends_with(':')).collect();
    (1..=parts.len()).rev().map(|k| format!("/project/{}", parts[parts.len() - k..].join("/"))).collect()
}

/// A path's folder and its name: `/project/a/b.png` is (`/project/a`,
/// `b.png`).
pub fn split(path: &str) -> (&str, &str) {
    match path.rfind(['/', '\\']) {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

/// The pictures a line names: Markdown's `![alt](a.png "title")`, HTML's
/// `src="a.svg"`, a string in code, a bare `docs/a.png`. Each as written,
/// in the order they come; URLs left out.
pub fn picture_refs(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, path) in picture_spans(line) {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// `picture_refs`, the one under `column` first (in characters; on the
/// quote or bracket before it too), then the rest as they come.
pub fn picture_refs_at(line: &str, column: usize) -> Vec<String> {
    let spans = picture_spans(line);
    let mut out: Vec<String> = spans.iter().filter(|(r, _)| r.start <= column + 1 && column <= r.end).map(|(_, p)| p.clone()).take(1).collect();
    for (_, path) in spans {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Each picture a line names, and where it is: its piece of the line, in
/// characters.
fn picture_spans(line: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut piece = String::new();
    // Pieces between the characters that end a path in text.
    for (i, c) in line.chars().chain([' ']).enumerate() {
        if !(c.is_whitespace() || "\"'`()<>[]{},;=|".contains(c)) {
            piece.push(c);
            continue;
        }
        let taken = std::mem::take(&mut piece);
        let at = std::mem::replace(&mut start, i + 1);
        let trimmed = taken.trim_end_matches(['.', ':', '!', '?']);
        let path = trimmed.split(['#', '?']).next().unwrap_or_default();
        let path = path.strip_prefix("file://").unwrap_or(path);
        if path.contains("://") || path.starts_with("data:") || !is_image_name(split(path).1) {
            continue;
        }
        out.push((at..i, path.to_string()));
    }
    out
}

/// `rel` from the folder `dir`, its `.` and `..` worked out; an absolute
/// `rel` as it is.
pub fn resolve(dir: &str, rel: &str) -> String {
    let rel = rel.replace('\\', "/");
    let absolute = rel.starts_with('/') || rel.as_bytes().get(1) == Some(&b':');
    let joined = if absolute { rel } else { format!("{}/{rel}", dir.replace('\\', "/").trim_end_matches('/')) };
    let (prefix, rest) = match joined.find('/') {
        Some(0) => ("/", &joined[1..]),
        Some(i) if joined[..i].ends_with(':') => (&joined[..=i], &joined[i + 1..]),
        _ => ("", joined.as_str()),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("{prefix}{}", parts.join("/"))
}

/// A full path under the project's folder, `root`, as `/project/...`.
pub fn under_project(full: &str, root: &str) -> Option<String> {
    let (full, root) = (full.replace('\\', "/"), root.replace('\\', "/"));
    let root = root.trim_end_matches('/');
    let same = |a: &str, b: &str| if cfg!(windows) || a.as_bytes().get(1) == Some(&b':') { a.eq_ignore_ascii_case(b) } else { a == b };
    if full.len() <= root.len() || !same(&full[..root.len()], root) || full.as_bytes()[root.len()] != b'/' {
        return None;
    }
    Some(format!("/project{}", &full[root.len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_folder_counts_as_a_person_does() {
        let list = pictures(names(&["shot-10.png", "notes.md", "Shot-2.png", "shot-1.PNG", "b.svg", "a.jpg", "shot-02.png", "z.txt"]));
        assert_eq!(list, names(&["a.jpg", "b.svg", "shot-1.PNG", "Shot-2.png", "shot-02.png", "shot-10.png"]));
    }

    #[test]
    fn next_and_previous_go_round() {
        let list = names(&["a.png", "b.png", "c.png"]);
        assert_eq!(step(&list, "a.png", true), Some(1));
        assert_eq!(step(&list, "c.png", true), Some(0), "the last to the first");
        assert_eq!(step(&list, "a.png", false), Some(2), "the first to the last");
        assert_eq!(step(&list, "bb.png", true), Some(2), "gone: the one after where it was");
        assert_eq!(step(&list, "bb.png", false), Some(1));
        assert_eq!(step(&list, "zz.png", true), Some(0));
        assert_eq!(step(&[], "a.png", true), None);
    }

    #[test]
    fn a_full_path_is_looked_for_in_the_project_longest_first() {
        assert_eq!(guest_candidates("C:\\work\\docs\\a.png"), ["/project/work/docs/a.png", "/project/docs/a.png", "/project/a.png"]);
        assert_eq!(guest_candidates("/p/a.png"), ["/project/p/a.png", "/project/a.png"]);
        assert_eq!(under_project("/home/me/p/docs/a.png", "/home/me/p"), Some("/project/docs/a.png".into()));
        assert_eq!(under_project("C:\\Work\\p\\a.png", "c:/work/p/"), Some("/project/a.png".into()));
        assert_eq!(under_project("/home/me/pp/a.png", "/home/me/p"), None, "a folder beside it isn't in it");
        assert_eq!(under_project("/elsewhere/a.png", "/home/me/p"), None);
    }

    #[test]
    fn a_line_names_its_pictures() {
        assert_eq!(picture_refs("![the plot](img/plot.png \"Plot\") and ![](../logo.svg)."), ["img/plot.png", "../logo.svg"]);
        assert_eq!(picture_refs(r#"<img src="a.webp#frag" alt=x>"#), ["a.webp"]);
        assert_eq!(picture_refs(r#"let icon = include_bytes!("icons/app.ico"); load("sprites/cat.GIF");"#), ["sprites/cat.GIF"]);
        assert_eq!(picture_refs("see https://example.com/a.png or docs/a.png"), ["docs/a.png"]);
        assert_eq!(picture_refs("file:///tmp/x.jpg x.jpg? nothing.txt"), ["/tmp/x.jpg", "x.jpg"]);
        assert!(picture_refs("no pictures here").is_empty());
    }

    #[test]
    fn the_picture_under_the_cursor_comes_first() {
        let line = "![a](img/a.png) and ![b](img/b.svg), a.png again";
        assert_eq!(picture_refs_at(line, 0), ["img/a.png", "img/b.svg", "a.png"], "on none: as they come");
        assert_eq!(picture_refs_at(line, line.find("b.svg").unwrap()), ["img/b.svg", "img/a.png", "a.png"]);
        assert_eq!(picture_refs_at(line, line.find("img/b").unwrap() - 1), ["img/b.svg", "img/a.png", "a.png"], "its bracket");
        assert_eq!(picture_refs_at("é ü.png", 3), ["ü.png"], "columns in characters");
    }

    #[test]
    fn paths_resolve_from_the_file_they_are_in() {
        assert_eq!(resolve("/home/me/p/docs", "img/a.png"), "/home/me/p/docs/img/a.png");
        assert_eq!(resolve("/home/me/p/docs", "../logo.svg"), "/home/me/p/logo.svg");
        assert_eq!(resolve("/home/me/p/docs", "./a.png"), "/home/me/p/docs/a.png");
        assert_eq!(resolve("/home/me/p/docs", "/tmp/x.jpg"), "/tmp/x.jpg");
        assert_eq!(resolve("C:\\w\\p\\docs", "..\\a.png"), "C:/w/p/a.png");
        assert_eq!(split("/project/a/b.png"), ("/project/a", "b.png"));
    }
}
