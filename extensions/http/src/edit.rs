//! The GUI's changes, as edits of the `.http` text: each field of the
//! request view is a part of the file, so changing it changes those lines
//! and nothing else -- one change you can undo, your comments and the
//! rest kept.

use crate::parse::Request;

/// Text from (line, column) up to another, both in characters, replaced
/// with `text`: as the API's `text-edit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub text: String,
}

fn len(lines: &[&str], line: u32) -> u32 {
    lines.get(line as usize).map(|l| l.trim_end_matches('\r').chars().count() as u32).unwrap_or(0)
}

/// Lines `from..to` replaced with `with` (each a line); none of them
/// removes them.
fn replace_lines(lines: &[&str], from: u32, to: u32, with: &[String]) -> Edit {
    let n = lines.len() as u32;
    if with.is_empty() {
        // The lines and the line break that ends them, or the one before
        // them at the file's end.
        return if to < n {
            Edit { start: (from, 0), end: (to, 0), text: String::new() }
        } else if from > 0 {
            Edit { start: (from - 1, len(lines, from - 1)), end: (n - 1, len(lines, n - 1)), text: String::new() }
        } else {
            Edit { start: (0, 0), end: (n.saturating_sub(1), len(lines, n.saturating_sub(1))), text: String::new() }
        };
    }
    if from >= to {
        // Put in before line `from`.
        if from < n {
            return Edit { start: (from, 0), end: (from, 0), text: with.join("\n") + "\n" };
        }
        let last = n.saturating_sub(1);
        let lead = if n == 0 || (n == 1 && len(lines, 0) == 0) { "" } else { "\n" };
        return Edit { start: (last, len(lines, last)), end: (last, len(lines, last)), text: format!("{lead}{}", with.join("\n")) };
    }
    Edit { start: (from, 0), end: (to - 1, len(lines, to - 1)), text: with.join("\n") }
}

/// The request line as it's written for `method` and `url`.
fn request_line(r: &Request, method: &str, url: &str) -> String {
    let version = r.version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
    if method == "GET" && !r.method_given {
        format!("{url}{version}")
    } else {
        format!("{method} {url}{version}")
    }
}

/// The method changed.
pub fn method(lines: &[&str], r: &Request, method: &str) -> Edit {
    replace_lines(lines, r.line, r.line + r.url_lines, &[request_line(&Request { method_given: true, ..r.clone() }, method, &r.url)])
}

/// The URL changed: on its one line, its query too.
pub fn url(lines: &[&str], r: &Request, url: &str) -> Edit {
    replace_lines(lines, r.line, r.line + r.url_lines, &[request_line(r, &r.method, url)])
}

/// Header `index` set to `name: value`; past the last, one added after
/// the others.
pub fn header(lines: &[&str], r: &Request, index: Option<usize>, name: &str, value: &str) -> Edit {
    let text = format!("{name}: {value}");
    match index.and_then(|i| r.headers.get(i)) {
        Some(h) => replace_lines(lines, h.line, h.line + 1, &[text]),
        None => replace_lines(lines, r.headers_end(), r.headers_end(), &[text]),
    }
}

pub fn remove_header(lines: &[&str], r: &Request, index: usize) -> Option<Edit> {
    let h = r.headers.get(index)?;
    Some(replace_lines(lines, h.line, h.line + 1, &[]))
}

/// Header `name` set, the one it has changed or one added.
pub fn set_header(lines: &[&str], r: &Request, name: &str, value: &str) -> Edit {
    let index = r.headers.iter().rposition(|h| h.name.eq_ignore_ascii_case(name));
    header(lines, r, index, name, value)
}

/// The query's parameters, in place of those it has.
pub fn params(lines: &[&str], r: &Request, params: &[(String, String)]) -> Edit {
    let (base, _) = crate::url::split(&r.url);
    url(lines, r, &crate::url::join(base, params))
}

/// The body, in place of the one it has (none takes it away).
pub fn body(lines: &[&str], r: &Request, text: Option<&str>) -> Edit {
    let with: Vec<String> = text.map(|t| t.split('\n').map(String::from).collect()).unwrap_or_default();
    match (&r.body, text) {
        (Some(b), Some(_)) => replace_lines(lines, b.start, b.end, &with),
        // Gone, and the blank line before it.
        (Some(b), None) => {
            let from = (r.headers_end()).min(b.start);
            replace_lines(lines, from, b.end, &[])
        }
        (None, Some(_)) => {
            let at = r.headers_end();
            let mut with = with;
            with.insert(0, String::new());
            // A blank line after it when another request follows.
            if (at as usize) < lines.len() && !lines[at as usize].trim().is_empty() {
                with.push(String::new());
            }
            replace_lines(lines, at, at, &with)
        }
        (None, None) => Edit { start: (r.line, 0), end: (r.line, 0), text: String::new() },
    }
}

/// Directive `# @name value` set: the one on line `line`, else the last
/// of that name; one added above the request line when it has none.
pub fn directive(lines: &[&str], r: &Request, name: &str, value: &str, line: Option<u32>) -> Edit {
    let text = if value.is_empty() { format!("# @{name}") } else { format!("# @{name} {value}") };
    let at = match line {
        Some(l) => r.directives.iter().find(|d| d.line == l),
        None => r.directive(name),
    };
    match at {
        Some(d) => replace_lines(lines, d.line, d.line + 1, &[text]),
        None => replace_lines(lines, r.line, r.line, &[text]),
    }
}

/// A directive added above the request line, beside those of its name:
/// another `# @test`.
pub fn add_directive(lines: &[&str], r: &Request, name: &str, value: &str) -> Edit {
    let text = if value.is_empty() { format!("# @{name}") } else { format!("# @{name} {value}") };
    let after = r.directives.iter().filter(|d| d.name == name && d.line < r.line).map(|d| d.line + 1).max();
    let at = after.unwrap_or(r.line);
    replace_lines(lines, at, at, &[text])
}

pub fn remove_line(lines: &[&str], line: u32) -> Edit {
    replace_lines(lines, line, line + 1, &[])
}

/// A new request at the file's end, `###` before it.
pub fn append(lines: &[&str], text: &str) -> Edit {
    let n = lines.len() as u32;
    let empty = lines.iter().all(|l| l.trim().is_empty());
    let mut with: Vec<String> = Vec::new();
    if !empty && !lines.last().is_some_and(|l| l.trim().is_empty()) {
        with.push(String::new());
    }
    with.extend(text.trim_end().split('\n').map(String::from));
    if empty {
        return Edit { start: (0, 0), end: (n.saturating_sub(1), len(lines, n.saturating_sub(1))), text: with.join("\n") + "\n" };
    }
    replace_lines(lines, n, n, &with)
}

/// Edits of one text, those at one place made one -- in the order given
/// -- so a header and a body both put in after the headers go in that
/// order.
pub fn merge(edits: Vec<Edit>) -> Vec<Edit> {
    let mut out: Vec<Edit> = Vec::new();
    for e in edits {
        match out.iter_mut().find(|o| o.start == e.start && o.end == e.end && o.start == o.end) {
            Some(o) => o.text.push_str(&e.text),
            None => out.push(e),
        }
    }
    out
}

/// Edits applied to `text`: what the buffer will hold. For tests, and to
/// read the request back before Fenix tells it.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let offset = |(line, col): (u32, u32)| -> usize {
        let mut at = 0;
        for l in lines.iter().take(line as usize) {
            at += l.len() + 1;
        }
        let l = lines.get(line as usize).copied().unwrap_or("");
        at + l.char_indices().nth(col as usize).map(|(i, _)| i).unwrap_or(l.len())
    };
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut out = text.to_string();
    for e in sorted {
        let (a, b) = (offset(e.start).min(out.len()), offset(e.end).min(out.len()));
        out.replace_range(a..b.max(a), &e.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::HttpFile;

    const FILE: &str = "### Users\n# @name users\nGET https://x.test/users?page=1 HTTP/1.1\nAccept: application/json\nX-Old: 1\n\n{\"a\": 1}\n\n###\nhttps://x.test/plain\n";

    fn edit(f: impl Fn(&[&str], &Request) -> Edit, which: usize) -> String {
        let file = HttpFile::parse(FILE);
        let lines: Vec<&str> = FILE.split('\n').collect();
        apply(FILE, &[f(&lines, &file.requests[which])])
    }

    #[test]
    fn the_request_line_is_rewritten() {
        let out = edit(|l, r| method(l, r, "POST"), 0);
        assert!(out.contains("\nPOST https://x.test/users?page=1 HTTP/1.1\n"), "{out}");
        let out = edit(|l, r| url(l, r, "https://y.test/{{id}}"), 1);
        assert!(out.ends_with("###\nhttps://y.test/{{id}}\n"), "a bare URL stays bare: {out}");
        let out = edit(|l, r| method(l, r, "DELETE"), 1);
        assert!(out.ends_with("###\nDELETE https://x.test/plain\n"));
        let out = edit(|l, r| params(l, r, &[("page".into(), "2".into()), ("q".into(), "{{term}}".into())]), 0);
        assert!(out.contains("GET https://x.test/users?page=2&q={{term}} HTTP/1.1\n"));
        let file = "GET https://x.test/a\n    ?b=1\n    &c=2\nAccept: */*\n";
        let r = &HttpFile::parse(file).requests[0];
        let out = apply(file, &[params(&file.split('\n').collect::<Vec<_>>(), r, &[("b".into(), "9".into())])]);
        assert_eq!(out, "GET https://x.test/a?b=9\nAccept: */*\n", "a query over several lines comes onto one");
    }

    #[test]
    fn headers_are_set_added_and_removed() {
        let out = edit(|l, r| header(l, r, Some(1), "X-New", "2"), 0);
        assert!(out.contains("Accept: application/json\nX-New: 2\n\n"), "{out}");
        let out = edit(|l, r| header(l, r, None, "Authorization", "Bearer t"), 0);
        assert!(out.contains("X-Old: 1\nAuthorization: Bearer t\n\n{"), "{out}");
        let out = edit(|l, r| remove_header(l, r, 0).unwrap(), 0);
        assert!(out.contains("HTTP/1.1\nX-Old: 1\n\n"), "{out}");
        let out = edit(|l, r| set_header(l, r, "accept", "text/plain"), 0);
        assert!(out.contains("\naccept: text/plain\nX-Old"), "{out}");
        let out = edit(|l, r| header(l, r, None, "Accept", "*/*"), 1);
        assert!(out.ends_with("https://x.test/plain\nAccept: */*\n"), "{out}");
    }

    #[test]
    fn the_body_is_replaced_added_and_taken_away() {
        let out = edit(|l, r| body(l, r, Some("{\n  \"b\": 2\n}")), 0);
        assert!(out.contains("X-Old: 1\n\n{\n  \"b\": 2\n}\n\n###"), "{out}");
        let out = edit(|l, r| body(l, r, None), 0);
        assert!(out.contains("X-Old: 1\n\n###"), "{out}");
        let out = edit(|l, r| body(l, r, Some("hello")), 1);
        assert!(out.ends_with("https://x.test/plain\n\nhello\n"), "{out}");
        let f = HttpFile::parse(&out);
        assert_eq!(f.requests[1].body.as_ref().map(|b| b.text.as_str()), Some("hello"));
    }

    #[test]
    fn directives_and_new_requests() {
        let out = edit(|l, r| add_directive(l, r, "test", "status == 200"), 0);
        assert!(out.contains("# @name users\n# @test status == 200\nGET"), "{out}");
        let twice = {
            let f = HttpFile::parse(&out);
            let lines: Vec<&str> = out.split('\n').collect();
            apply(&out, &[add_directive(&lines, &f.requests[0], "test", "time < 500")])
        };
        assert!(twice.contains("# @test status == 200\n# @test time < 500\nGET"), "{twice}");
        let out = edit(|l, r| directive(l, r, "name", "list", None), 0);
        assert!(out.contains("### Users\n# @name list\nGET"));
        let out = edit(|l, r| directive(l, r, "no-redirect", "", None), 1);
        assert!(out.contains("###\n# @no-redirect\nhttps://x.test/plain"), "{out}");
        let out = edit(|l, _| remove_line(l, 1), 0);
        assert!(out.starts_with("### Users\nGET"));
        let lines: Vec<&str> = FILE.split('\n').collect();
        let out = apply(FILE, &[append(&lines, "###\nGET https://x.test/new")]);
        assert!(out.ends_with("https://x.test/plain\n\n###\nGET https://x.test/new"), "{out:?}");
        assert_eq!(HttpFile::parse(&out).requests.len(), 3);
        assert_eq!(apply("", &[append(&[""], "GET https://a.test")]), "GET https://a.test\n");
        // A header and a body put in at one place, in that order.
        let file = "POST https://x.test\n";
        let f = HttpFile::parse(file);
        let lines: Vec<&str> = file.split('\n').collect();
        let both = merge(vec![set_header(&lines, &f.requests[0], "Content-Type", "application/json"), body(&lines, &f.requests[0], Some("{}"))]);
        assert_eq!(both.len(), 1);
        assert_eq!(apply(file, &both), "POST https://x.test\nContent-Type: application/json\n\n{}\n");
    }
}
