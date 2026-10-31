//! What the playground opens with, from where you are: the selection as
//! the test text; else the regex under the cursor -- a Rust, Python,
//! JavaScript, Go, Java or C# regex literal, by the file's extension --
//! as the pattern, the one nearest it when it's on none; else the line as
//! the text.

use std::ops::Range;

use crate::engine::Flags;

/// A regex found in code: its pattern, unescaped, and the flags a
/// JavaScript literal carried (`i`, `m`, `s`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub pattern: String,
    pub flags: Option<Flags>,
}

/// What it opens with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seed {
    Pattern(Found),
    /// The selection, as the text.
    Selection(String),
    /// The cursor's line, with no regex on it, as the text.
    Line(String),
}

/// Where the cursor was (`editor-cursor`): its column on the line, in
/// characters, and whether `here`'s text is the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub column: usize,
    pub selected: bool,
}

/// The seed from `here`'s `text` in a file of `language`. Without
/// `cursor` (a Fenix before 0.8.1) the line's first regex, and a text of
/// one line is taken for the cursor's line.
pub fn seed(text: &str, language: &str, cursor: Option<Cursor>) -> Seed {
    let text = text.trim_end_matches(['\n', '\r']);
    let selected = match cursor {
        Some(c) => c.selected,
        None => text.contains('\n'),
    };
    if selected {
        return Seed::Selection(text.to_string());
    }
    let found = match cursor {
        Some(c) => regex_at(text, language, c.column),
        None => regex_on(text, language),
    };
    match found {
        Some(found) => Seed::Pattern(found),
        None => Seed::Line(text.to_string()),
    }
}

/// The regex under `column` (in characters) on `line`, or the one nearest
/// it.
pub fn regex_at(line: &str, language: &str, column: usize) -> Option<Found> {
    let distance = |r: &Range<usize>| if r.contains(&column) { 0 } else if column < r.start { r.start - column } else { column + 1 - r.end };
    regexes_on(line, language).into_iter().min_by_key(|(r, _)| (distance(r), r.start)).map(|(_, f)| f)
}

/// The calls whose first string is a regex, by language.
fn calls(language: &str) -> &'static [&'static str] {
    match language {
        "rs" => &["Regex::new(", "RegexBuilder::new(", "RegexSet::new([", "regex!("],
        "py" | "pyi" => &["re.compile(", "re.match(", "re.search(", "re.fullmatch(", "re.findall(", "re.finditer(", "re.sub(", "re.subn(", "re.split("],
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => &["RegExp("],
        "go" => &["regexp.MustCompile(", "regexp.Compile(", "regexp.MatchString("],
        "java" | "kt" => &["Pattern.compile(", "Pattern.matches(", ".matches(", ".replaceAll(", ".split("],
        "cs" => &["new Regex(", "Regex.Match(", "Regex.IsMatch(", "Regex.Matches(", "Regex.Replace(", "Regex.Split("],
        _ => &[],
    }
}

/// The first regex on `line` of a file of `language` (its extension),
/// if any: a JavaScript literal before a call.
pub fn regex_on(line: &str, language: &str) -> Option<Found> {
    let mut all = regexes_on(line, language);
    let literal = all.iter().position(|(_, f)| f.flags.is_some());
    literal.or((!all.is_empty()).then_some(0)).map(|i| all.swap_remove(i).1)
}

/// Every regex on `line`, with where it is, in characters: from its call
/// or its literal's `/` to the end of its string or its flags. In the
/// order they come.
pub fn regexes_on(line: &str, language: &str) -> Vec<(Range<usize>, Found)> {
    let col = |byte: usize| line[..byte].chars().count();
    let mut out = Vec::new();
    if matches!(language, "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx") {
        out.extend(js_literals(line));
    }
    for call in calls(language) {
        for (at, _) in line.match_indices(call) {
            // `.matches(` is in `Pattern.matches(` too: one regex, once.
            if out.iter().any(|(r, _): &(Range<usize>, Found)| r.contains(&col(at))) {
                continue;
            }
            let after = at + call.len();
            let rest = &line[after..];
            // C#'s static calls take the input first: the pattern is the
            // string after it.
            let rest = if language == "cs" && call.starts_with("Regex.") { rest.split_once(',').map_or(rest, |(_, r)| r) } else { rest };
            let start = line.len() - rest.trim_start().len();
            if let Some((pattern, len)) = string_at(&line[start..], language) {
                out.push((col(at)..col(start + len), Found { pattern, flags: None }));
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

/// The string literal `s` starts with, unescaped as its language reads
/// it, and its length in bytes, quotes and all.
fn string_at(s: &str, language: &str) -> Option<(String, usize)> {
    let mut chars = s.char_indices().peekable();
    let (_, first) = chars.next()?;
    match (language, first) {
        // Rust raw strings: r"..." and r#"..."#.
        ("rs", 'r') => {
            let hashes = s[1..].chars().take_while(|&c| c == '#').count();
            let open = 1 + hashes;
            if s[open..].starts_with('"') {
                let close = format!("\"{}", "#".repeat(hashes));
                let body = &s[open + 1..];
                return body.find(&close).map(|end| (body[..end].to_string(), open + 1 + end + close.len()));
            }
            None
        }
        // Python raw strings, any case of r, and its prefixes.
        ("py" | "pyi", 'r' | 'R') => {
            let q = s[1..].chars().next()?;
            if q == '"' || q == '\'' {
                return raw_until(&s[2..], q).map(|(b, n)| (b, 2 + n));
            }
            None
        }
        ("go", '`') => s[1..].find('`').map(|end| (s[1..1 + end].to_string(), end + 2)),
        ("cs", '@') if s[1..].starts_with('"') => {
            let body = &s[2..];
            let mut out = String::new();
            let mut it = body.char_indices().peekable();
            while let Some((i, c)) = it.next() {
                if c == '"' {
                    if it.peek().map(|&(_, c)| c) == Some('"') {
                        it.next();
                        out.push('"');
                        continue;
                    }
                    return Some((out, 2 + i + 1));
                }
                out.push(c);
            }
            None
        }
        (_, '"') | ("py" | "pyi" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx", '\'') => escaped_until(&s[1..], first).map(|(b, n)| (b, 1 + n)),
        _ => None,
    }
}

/// A raw string's body up to quote `q`: a backslash keeps the quote it
/// precedes, and itself. And its length in bytes, the quote with it.
fn raw_until(s: &str, q: char) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        if c == '\\' {
            out.push(c);
            out.push(it.next()?.1);
        } else if c == q {
            return Some((out, i + c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    None
}

/// An ordinary string's body up to quote `q`, unescaped: `\\` is one
/// backslash and `\"` a quote; other escapes stay, which the regex reads
/// the same way (`\n`, `\t`). And its length in bytes, the quote with
/// it.
fn escaped_until(s: &str, q: char) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        if c == '\\' {
            let n = it.next()?.1;
            if n != '\\' && n != q && n != '\'' && n != '"' {
                out.push('\\');
            }
            out.push(n);
        } else if c == q {
            return Some((out, i + c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    None
}

/// JavaScript regex literals, `/.../flags`, where an expression starts:
/// each with where it is, in characters.
fn js_literals(line: &str) -> Vec<(Range<usize>, Found)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut from = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c != '/' || i < from {
            continue;
        }
        let before = chars[..i].iter().rev().find(|c| !c.is_whitespace());
        let starts = match before {
            None => true,
            Some(b) => "(=,:[!&|?{};+".contains(*b) || line[..line.char_indices().nth(i).map_or(0, |(b, _)| b)].trim_end().ends_with("return"),
        };
        // `//` is a comment, `/*` too.
        if !starts || matches!(chars.get(i + 1), Some('/') | Some('*') | None) {
            continue;
        }
        let mut j = i + 1;
        let mut class = false;
        let mut body = String::new();
        while j < chars.len() {
            let c = chars[j];
            match c {
                '\\' => {
                    body.push(c);
                    j += 1;
                    if let Some(&n) = chars.get(j) {
                        body.push(n);
                    }
                }
                '[' => {
                    class = true;
                    body.push(c);
                }
                ']' => {
                    class = false;
                    body.push(c);
                }
                '/' if !class => break,
                c => body.push(c),
            }
            j += 1;
        }
        if j >= chars.len() || body.is_empty() {
            continue;
        }
        let letters: String = chars[j + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        // `u` stays as it is: JavaScript's u is about `\u{...}` and `\p{...}`,
        // which Rust reads either way.
        let flags = Flags { i: letters.contains('i'), m: letters.contains('m'), s: letters.contains('s'), ..Flags::default() };
        // `\/` is only escaped for the literal's sake.
        let end = j + 1 + letters.chars().count();
        out.push((i..end, Found { pattern: body.replace("\\/", "/"), flags: Some(flags) }));
        from = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(line: &str, lang: &str) -> Option<String> {
        regex_on(line, lang).map(|f| f.pattern)
    }

    #[test]
    fn rust_raw_and_plain_strings() {
        assert_eq!(pattern(r#"    let re = Regex::new(r"(\d+)-(\d+)").unwrap();"#, "rs").as_deref(), Some(r"(\d+)-(\d+)"));
        assert_eq!(pattern(r###"Regex::new(r#"say "(\w+)""#)?"###, "rs").as_deref(), Some(r#"say "(\w+)""#));
        assert_eq!(pattern(r#"Regex::new("\\d+\"")"#, "rs").as_deref(), Some(r#"\d+""#));
        assert_eq!(pattern("let x = 1;", "rs"), None);
    }

    #[test]
    fn python_go_java_and_csharp() {
        assert_eq!(pattern(r#"m = re.match(r'^(\w+)\s', line)"#, "py").as_deref(), Some(r"^(\w+)\s"));
        assert_eq!(pattern(r#"re.sub("\\s+", " ", s)"#, "py").as_deref(), Some(r"\s+"));
        assert_eq!(pattern(r"var re = regexp.MustCompile(`^v(\d+)`)", "go").as_deref(), Some(r"^v(\d+)"));
        assert_eq!(pattern(r#"Pattern p = Pattern.compile("\\w+@\\w+");"#, "java").as_deref(), Some(r"\w+@\w+"));
        assert_eq!(pattern(r#"var ok = Regex.IsMatch(input, @"^""\d+""$");"#, "cs").as_deref(), Some(r#"^"\d+"$"#));
    }

    #[test]
    fn a_javascript_literal_and_its_flags() {
        let f = regex_on(r"const re = /^(\d+)\/(\d+)$/gi;", "js").unwrap();
        assert_eq!(f.pattern, r"^(\d+)/(\d+)$");
        assert_eq!(f.flags.map(|f| f.letters()), Some("iu".to_string()));
        assert_eq!(pattern(r#"if (/[/]x/.test(s)) {"#, "ts").as_deref(), Some("[/]x"), "a slash in a class");
        assert_eq!(pattern("const half = a / b / c;", "js"), None, "division isn't a regex");
        assert_eq!(pattern("// a comment", "js"), None);
        assert_eq!(pattern(r#"new RegExp("\\d+")"#, "js").as_deref(), Some(r"\d+"));
    }

    #[test]
    fn the_regex_under_the_cursor() {
        let line = r#"let (a, b) = (Regex::new(r"^\d+$")?, Regex::new(r"[a-z]+")?);"#;
        let at = |col| regex_at(line, "rs", col).map(|f| f.pattern);
        assert_eq!(at(0).as_deref(), Some(r"^\d+$"), "before both: the nearer");
        assert_eq!(at(16).as_deref(), Some(r"^\d+$"));
        assert_eq!(at(line.find("[a-z]").unwrap()).as_deref(), Some("[a-z]+"));
        assert_eq!(at(line.chars().count()).as_deref(), Some("[a-z]+"), "after both: the nearer");
        let js = r"const a = /x+/g, b = /y+/i;";
        assert_eq!(regex_at(js, "js", js.find("/y").unwrap() + 2).map(|f| f.pattern).as_deref(), Some("y+"));
        let java = r#"s.matches("a+") || Pattern.matches("b+", s)"#;
        let spans: Vec<_> = regexes_on(java, "java").into_iter().map(|(r, f)| (r, f.pattern)).collect();
        assert_eq!(spans, [(1..14, "a+".to_string()), (19..39, "b+".to_string())], "Pattern.matches once, not as .matches too");
        assert_eq!(regex_at("é Regex::new(\"ü+\")", "rs", 14).map(|f| f.pattern).as_deref(), Some("ü+"), "columns in characters");
    }

    #[test]
    fn a_selection_is_the_text_and_a_line_without_one_too() {
        let line = r#"    let re = Regex::new(r"(\d+)")?;"#;
        let on = |column, selected| Some(Cursor { column, selected });
        assert_eq!(seed(line, "rs", on(20, false)), Seed::Pattern(Found { pattern: r"(\d+)".into(), flags: None }));
        assert_eq!(seed(line, "rs", on(20, true)), Seed::Selection(line.into()), "selected: it's the text, regex or not");
        assert_eq!(seed("let x = 1;\n", "rs", on(3, false)), Seed::Line("let x = 1;".into()));
        assert_eq!(seed(line, "rs", None), Seed::Pattern(Found { pattern: r"(\d+)".into(), flags: None }), "a Fenix before 0.8.1");
        assert_eq!(seed("a\nb", "rs", None), Seed::Selection("a\nb".into()));
    }
}
