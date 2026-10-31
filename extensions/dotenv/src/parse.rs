//! An env file read the way the tools that load one read it -- dotenv
//! for Node, python-dotenv, dotenvy, Docker Compose -- where they agree:
//! `KEY=value` lines, `export ` before a key, `#` comments, values in
//! single, double or back quotes that may run over several lines,
//! escapes in double quotes, and `${VAR}` filled in from the keys above.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    None,
    Single,
    Double,
    Back,
}

/// Characters of a line: columns `start` up to `end`, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(line: u32, start: u32, end: u32) -> Span {
        Span { line, start, end }
    }
}

/// One `KEY=value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// The value as written: inside its quotes, escapes and `$` as they
    /// are.
    pub raw: String,
    pub quote: Quote,
    pub export: bool,
    /// The key, where it is.
    pub key_span: Span,
    /// The value's text (inside its quotes), a run for each line it's on;
    /// none when it's empty.
    pub value: Vec<Span>,
}

impl Entry {
    pub fn line(&self) -> u32 {
        self.key_span.line
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

/// An env file, read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    pub entries: Vec<Entry>,
    pub problems: Vec<Problem>,
}

fn skip_space(line: &[char], mut c: usize) -> usize {
    while c < line.len() && (line[c] == ' ' || line[c] == '\t') {
        c += 1;
    }
    c
}

fn starts_with(line: &[char], at: usize, word: &str) -> bool {
    let word: Vec<char> = word.chars().collect();
    line.len() >= at + word.len() && line[at..at + word.len()] == word[..]
}

/// What's wrong with `key`, if anything.
fn bad_key(key: &str) -> Option<(Severity, String)> {
    if key.is_empty() {
        return Some((Severity::Error, "no key before `=`".into()));
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Some((Severity::Error, format!("`{key}`: a key is letters, digits and `_`")));
    }
    if key.starts_with(|c: char| c.is_ascii_digit()) || key.contains(['.', '-']) {
        return Some((Severity::Warning, format!("`{key}` isn't a name a shell takes: `${key}` and `export` don't reach it")));
    }
    None
}

/// Where quote `quote` closes, from (`line`, `col`): a backslash escapes
/// the character after it when `escapes`.
fn closing(lines: &[Vec<char>], mut line: usize, mut col: usize, quote: char, escapes: bool) -> Option<(usize, usize)> {
    while line < lines.len() {
        let l = &lines[line];
        while col < l.len() {
            match l[col] {
                '\\' if escapes => col += 2,
                c if c == quote => return Some((line, col)),
                _ => col += 1,
            }
        }
        line += 1;
        col = 0;
    }
    None
}

impl EnvFile {
    pub fn parse(text: &str) -> EnvFile {
        let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect()).collect();
        let mut file = EnvFile::default();
        let problem = |file: &mut EnvFile, span: Span, severity: Severity, message: String| file.problems.push(Problem { span, severity, message });
        let mut i = 0;
        while i < lines.len() {
            let line = &lines[i];
            let n = i as u32;
            let mut c = skip_space(line, 0);
            if c >= line.len() || line[c] == '#' {
                i += 1;
                continue;
            }
            let mut export = false;
            if starts_with(line, c, "export") && line.get(c + 6).is_some_and(|ch| *ch == ' ' || *ch == '\t') {
                export = true;
                c = skip_space(line, c + 6);
            }
            let key_start = c;
            while c < line.len() && line[c] != '=' && line[c] != ' ' && line[c] != '\t' {
                c += 1;
            }
            let key: String = line[key_start..c].iter().collect();
            let key_span = Span::new(n, key_start as u32, c as u32);
            c = skip_space(line, c);
            if line.get(c) != Some(&'=') {
                problem(&mut file, Span::new(n, key_start as u32, line.len() as u32), Severity::Error, "not KEY=value: a line is a key, `=` and its value, or a # comment".into());
                i += 1;
                continue;
            }
            if let Some((severity, message)) = bad_key(&key) {
                let span = if key.is_empty() { Span::new(n, c as u32, c as u32 + 1) } else { key_span };
                problem(&mut file, span, severity, message);
            }
            c = skip_space(line, c + 1);
            let quote = match line.get(c) {
                Some('\'') => Quote::Single,
                Some('"') => Quote::Double,
                Some('`') => Quote::Back,
                _ => Quote::None,
            };
            let mut entry = Entry { key, raw: String::new(), quote, export, key_span, value: Vec::new() };
            let closed = (quote != Quote::None).then(|| closing(&lines, i, c + 1, line[c], quote == Quote::Double)).flatten();
            match closed {
                Some((j, k)) => {
                    let mut parts = Vec::new();
                    for (at, l) in lines.iter().enumerate().take(j + 1).skip(i) {
                        let from = if at == i { c + 1 } else { 0 };
                        let to = if at == j { k } else { l.len() };
                        parts.push(l[from..to].iter().collect::<String>());
                        if to > from {
                            entry.value.push(Span::new(at as u32, from as u32, to as u32));
                        }
                    }
                    entry.raw = parts.join("\n");
                    let after = skip_space(&lines[j], k + 1);
                    if after < lines[j].len() && lines[j][after] != '#' {
                        problem(&mut file, Span::new(j as u32, after as u32, lines[j].len() as u32), Severity::Warning, "after the closing quote: this isn't part of the value".into());
                    }
                    i = j;
                }
                None => {
                    if quote != Quote::None {
                        problem(&mut file, Span::new(n, c as u32, c as u32 + 1), Severity::Error, "this quote isn't closed: loaders read the rest of the file into the value, or stop here".into());
                        entry.quote = Quote::None;
                    }
                    // Unquoted: up to a `#` at its start or after a space.
                    let mut end = c;
                    while end < line.len() && !(line[end] == '#' && (end == c || line[end - 1] == ' ' || line[end - 1] == '\t')) {
                        end += 1;
                    }
                    while end > c && (line[end - 1] == ' ' || line[end - 1] == '\t') {
                        end -= 1;
                    }
                    entry.raw = line[c..end].iter().collect();
                    if end > c {
                        entry.value.push(Span::new(n, c as u32, end as u32));
                    }
                }
            }
            if !entry.key.is_empty() {
                file.entries.push(entry);
            }
            i += 1;
        }
        // A key set twice: loaders differ on which they keep.
        let mut first: HashMap<&str, u32> = HashMap::new();
        let mut twice = Vec::new();
        for e in &file.entries {
            match first.get(e.key.as_str()) {
                Some(line) => twice.push((e.key_span, format!("`{}` is set again (line {} set it first): most loaders keep this one, dotenvy the first", e.key, line + 1))),
                None => {
                    first.insert(&e.key, e.line());
                }
            }
        }
        for (span, message) in twice {
            file.problems.push(Problem { span, severity: Severity::Warning, message });
        }
        file
    }

    /// Its keys, each once, in the order they first come.
    pub fn keys(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        self.entries.iter().map(|e| e.key.as_str()).filter(|k| seen.insert(*k)).collect()
    }

    /// The entry for `key` that loaders keep: the last.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| e.key == key)
    }

    /// Each entry's value as it's loaded -- escapes undone, `${VAR}` filled
    /// in from the keys above it -- and the names it refers to that none
    /// above sets (the environment's, if anything's).
    pub fn values(&self) -> Vec<(String, Vec<String>)> {
        let mut known: HashMap<&str, String> = HashMap::new();
        self.entries
            .iter()
            .map(|e| {
                let (value, unset) = expand(&e.raw, e.quote, &|name| known.get(name).cloned());
                known.insert(&e.key, value.clone());
                (value, unset)
            })
            .collect()
    }

    /// Each key's loaded value (the last entry's), by key.
    pub fn map(&self) -> HashMap<String, String> {
        self.entries.iter().zip(self.values()).map(|(e, (v, _))| (e.key.clone(), v)).collect()
    }
}

/// A default after a name: `${VAR:-default}` (when it's unset or empty)
/// or `${VAR-default}` (when it's unset).
struct Default {
    empty_too: bool,
    text: String,
}

/// The `$NAME` or `${NAME...}` at the start of `chars` (which is `$`): the
/// name, its default, and how many characters it takes.
fn reference(chars: &[char]) -> Option<(String, Option<Default>, usize)> {
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    match chars.get(1) {
        Some('{') => {
            let mut depth = 0;
            let close = chars.iter().enumerate().skip(1).find_map(|(i, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
                None
            })?;
            let inner: String = chars[2..close].iter().collect();
            let name: String = inner.chars().take_while(|c| name_char(*c)).collect();
            if name.is_empty() {
                return None;
            }
            let rest = &inner[name.len()..];
            let default = if let Some(text) = rest.strip_prefix(":-") {
                Some(Default { empty_too: true, text: text.into() })
            } else {
                rest.strip_prefix('-').map(|text| Default { empty_too: false, text: text.into() })
            };
            Some((name, default, close + 1))
        }
        Some(c) if c.is_ascii_alphabetic() || *c == '_' => {
            let name: String = chars[1..].iter().take_while(|c| name_char(**c)).collect();
            let n = name.chars().count();
            Some((name, None, 1 + n))
        }
        _ => None,
    }
}

/// `raw` as it's loaded: in double quotes `\n`, `\t`, `\"` and the rest
/// undone; outside single and back quotes `${VAR}`, `$VAR`,
/// `${VAR:-default}` and `${VAR-default}` filled in by `lookup` (`\$` is a
/// `$`). Also the names `lookup` didn't know and no default stood in for.
pub fn expand(raw: &str, quote: Quote, lookup: &dyn Fn(&str) -> Option<String>) -> (String, Vec<String>) {
    if matches!(quote, Quote::Single | Quote::Back) {
        return (raw.to_string(), Vec::new());
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut unset = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if quote == Quote::Double {
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' | '\\' | '$' | '\'' | '`' => out.push(next),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
                i += 2;
                continue;
            }
            if next == '$' {
                out.push('$');
                i += 2;
                continue;
            }
        }
        if c == '$' {
            if let Some((name, default, taken)) = reference(&chars[i..]) {
                match (lookup(&name), default) {
                    (Some(v), Some(d)) if d.empty_too && v.is_empty() => out.push_str(&expand(&d.text, quote, lookup).0),
                    (Some(v), _) => out.push_str(&v),
                    (None, Some(d)) => out.push_str(&expand(&d.text, quote, lookup).0),
                    (None, None) => unset.push(name),
                }
                i += taken;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    (out, unset)
}

/// What a value shows as when it's hidden.
pub const MASK: &str = "••••••";

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Entry {
        let f = EnvFile::parse(text);
        assert_eq!(f.entries.len(), 1, "{f:?}");
        f.entries[0].clone()
    }

    #[test]
    fn plain_lines_comments_and_export() {
        let f = EnvFile::parse("# a comment\n\nexport  API_KEY = abc123  # the key\nEMPTY=\nURL=http://x/#frag\nHASH=#all comment\n");
        let keys: Vec<_> = f.entries.iter().map(|e| (e.key.as_str(), e.raw.as_str(), e.export)).collect();
        assert_eq!(keys, [("API_KEY", "abc123", true), ("EMPTY", "", false), ("URL", "http://x/#frag", false), ("HASH", "", false)]);
        assert!(f.problems.is_empty(), "{:?}", f.problems);
        let e = &f.entries[0];
        assert_eq!(e.key_span, Span::new(2, 8, 15));
        assert_eq!(e.value, [Span::new(2, 18, 24)]);
        assert!(f.entries[1].value.is_empty(), "nothing to hide");
    }

    #[test]
    fn quotes_and_escapes() {
        let e = one("A=\"a\\nb \\\"c\\\" \\$HOME\"  # after\n");
        assert_eq!(e.quote, Quote::Double);
        assert_eq!(e.raw, "a\\nb \\\"c\\\" \\$HOME");
        assert_eq!(expand(&e.raw, e.quote, &|_| Some("x".into())).0, "a\nb \"c\" $HOME");
        let e = one("B='no \\n $HOME here'");
        assert_eq!(expand(&e.raw, e.quote, &|_| Some("x".into())).0, "no \\n $HOME here");
        assert_eq!(e.value, [Span::new(0, 3, 19)]);
        let e = one("C=`back`");
        assert_eq!((e.quote, e.raw.as_str()), (Quote::Back, "back"));
        let e = one("D=\"\"");
        assert!(e.value.is_empty());
    }

    #[test]
    fn values_run_over_lines_in_quotes() {
        let f = EnvFile::parse("KEY=\"-----BEGIN-----\nabc\n-----END-----\"\nNEXT=1\r\n");
        assert_eq!(f.entries.len(), 2);
        let e = &f.entries[0];
        assert_eq!(e.raw, "-----BEGIN-----\nabc\n-----END-----");
        assert_eq!(e.value, [Span::new(0, 5, 20), Span::new(1, 0, 3), Span::new(2, 0, 13)]);
        assert_eq!((f.entries[1].key.as_str(), f.entries[1].raw.as_str(), f.entries[1].line()), ("NEXT", "1", 3));
        let f = EnvFile::parse("A='x\n\ny'\n");
        assert_eq!(f.entries[0].value, [Span::new(0, 3, 4), Span::new(2, 0, 1)], "an empty line has nothing to hide");
    }

    #[test]
    fn interpolation_from_the_keys_above() {
        let f = EnvFile::parse("HOST=db\nPORT=5432\nURL=\"postgres://${HOST}:$PORT/${NAME:-app}\"\nRAW='${HOST}'\nODD=${MISSING}x\nE=\nF=${E:-dflt}\nG=${E-dflt}\nH=\\$HOST\nLATER=${AFTER}\nAFTER=1\n");
        let values: Vec<String> = f.values().into_iter().map(|(v, _)| v).collect();
        assert_eq!(values, ["db", "5432", "postgres://db:5432/app", "${HOST}", "x", "", "dflt", "", "$HOST", "", "1"]);
        let unset: Vec<Vec<String>> = f.values().into_iter().map(|(_, u)| u).collect();
        assert_eq!(unset[4], ["MISSING"]);
        assert_eq!(unset[9], ["AFTER"], "only the keys above");
        assert!(unset[2].is_empty(), "a default stands in");
    }

    #[test]
    fn problems_are_said_where_they_are() {
        let f = EnvFile::parse("just words\n=x\n1BAD=x\nmy.key=y\nA=\"open\nB=2\nC='x' junk\nB=3\nWE IRD=1\n");
        let said: Vec<(u32, Severity, &str)> = f.problems.iter().map(|p| (p.span.line, p.severity, p.message.as_str())).collect();
        assert_eq!(said[0].0, 0);
        assert!(said[0].2.starts_with("not KEY=value"));
        assert_eq!((said[1].0, said[1].1), (1, Severity::Error), "no key");
        assert_eq!((said[2].0, said[2].1), (2, Severity::Warning));
        assert_eq!((said[3].0, said[3].1), (3, Severity::Warning));
        assert!(said[4].2.contains("isn't closed") && said[4].0 == 4);
        assert!(said[5].2.contains("after the closing quote") && said[5].0 == 6);
        assert!(said[6].2.contains("not KEY=value") && said[6].0 == 8, "{said:?}");
        assert!(said[7].2.contains("`B` is set again (line 6"), "{said:?}");
        // An unclosed quote takes the rest of its line only.
        assert_eq!(f.get("A").unwrap().raw, "\"open");
        assert_eq!(f.get("B").unwrap().raw, "3", "the last is kept");
        assert_eq!(f.keys(), ["1BAD", "my.key", "A", "B", "C"]);
    }
}
