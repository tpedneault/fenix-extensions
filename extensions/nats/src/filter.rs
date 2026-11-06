//! Filters: which messages are shown, and which are coloured.
//!
//! A filter is written as words, each a condition, all of which must hold:
//!
//! ```text
//! orders.> header:Nats-Msg-Id header:region=eu re:"time(out)?" json:.status=failed size:100..2000 error
//! ```
//!
//! - a word with a dot or a wildcard and nothing else (`orders.>`), or
//!   `subject:` before it: the subject matches that pattern;
//! - `header:K` (or `h:`): it has header K; `header:K=V`, with that value;
//! - `re:` (or `regex:`): its payload matches a regular expression;
//! - `json:PATH=VALUE`: its payload is JSON with that value at that path
//!   (`.a.b[0]`, `$.a.b`); the value is JSON (`"x"`, `3`, `true`) or a bare
//!   word, taken as a string;
//! - `size:MIN..MAX`, `size:>N`, `size:<N`: its size, headers and payload;
//! - any other word: in its subject or its payload, any case.
//!
//! The subject and size are cheap: a subject's pattern is checked once a
//! subject, and the size is in each message's entry. The rest need its
//! bytes, and JSON needs them parsed: they're only checked on messages
//! being looked at, a time budget at a time.

use regex::bytes::Regex;

use crate::store::View;

/// A path into JSON: keys and indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

pub fn parse_path(path: &str) -> Result<Vec<Step>, String> {
    let p = path.trim();
    let p = p.strip_prefix('$').unwrap_or(p);
    let mut steps = Vec::new();
    let mut chars = p.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                let mut key = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '.' || c == '[' {
                        break;
                    }
                    key.push(c);
                    chars.next();
                }
                if key.is_empty() {
                    return Err(format!("{path}: a key after each dot"));
                }
                steps.push(Step::Key(key));
            }
            '[' => {
                let inner: String = chars.by_ref().take_while(|c| *c != ']').collect();
                let inner = inner.trim();
                if let Some(q) = inner.strip_prefix('"').and_then(|s| s.strip_suffix('"')).or_else(|| inner.strip_prefix('\'').and_then(|s| s.strip_suffix('\''))) {
                    steps.push(Step::Key(q.into()));
                } else {
                    steps.push(Step::Index(inner.parse().map_err(|_| format!("{path}: [{inner}] isn't an index"))?));
                }
            }
            c if steps.is_empty() && !c.is_whitespace() => {
                // A path that starts with a key: `status`.
                let mut key = c.to_string();
                while let Some(&c) = chars.peek() {
                    if c == '.' || c == '[' {
                        break;
                    }
                    key.push(c);
                    chars.next();
                }
                steps.push(Step::Key(key));
            }
            _ => return Err(format!("{path}: a path is .key, [index] or [\"key\"]")),
        }
    }
    Ok(steps)
}

pub fn lookup<'a>(v: &'a serde_json::Value, path: &[Step]) -> Option<&'a serde_json::Value> {
    path.iter().try_fold(v, |v, step| match step {
        Step::Key(k) => v.get(k.as_str()),
        Step::Index(i) => v.get(*i),
    })
}

#[derive(Debug, Clone)]
pub struct JsonEq {
    pub path: Vec<Step>,
    pub value: serde_json::Value,
}

/// A filter, read from what was written.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// As written.
    pub source: String,
    pub subjects: Vec<String>,
    pub headers: Vec<(String, Option<String>)>,
    pub regexes: Vec<Regex>,
    pub json: Vec<JsonEq>,
    pub min_size: Option<u32>,
    pub max_size: Option<u32>,
    /// Words to find in the subject or the payload, lower case.
    pub words: Vec<String>,
}

/// Splits on spaces, keeping what's in quotes together (quotes kept off).
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            // A quote opens a quoted run at a word's start or after its
            // `key:`; inside a word it's a character (json:.n="3").
            None if (c == '"' || c == '\'') && (cur.is_empty() || cur.ends_with(':')) => {
                quote = Some(c);
                any = true;
            }
            None if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            None => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A bare word with a wildcard token is a subject pattern: `orders.>`.
fn looks_like_subject(w: &str) -> bool {
    !w.contains(':') && fenix_nats::subject::is_wildcard(w) && fenix_nats::subject::check_pattern(w).is_ok()
}

impl Filter {
    pub fn parse(text: &str) -> Result<Filter, String> {
        let mut f = Filter { source: text.trim().to_string(), ..Filter::default() };
        for w in words(text) {
            let (key, rest) = w.split_once(':').map_or(("", w.as_str()), |(k, r)| (k, r));
            match key.to_ascii_lowercase().as_str() {
                "subject" | "s" => {
                    fenix_nats::subject::check_pattern(rest)?;
                    f.subjects.push(rest.to_string());
                }
                "header" | "h" => match rest.split_once('=') {
                    Some((k, v)) => f.headers.push((k.trim().to_string(), Some(v.trim().to_string()))),
                    None if !rest.is_empty() => f.headers.push((rest.trim().to_string(), None)),
                    None => return Err("header: a header's name, and =value if you like".into()),
                },
                "re" | "regex" => {
                    f.regexes.push(Regex::new(rest).map_err(|e| format!("re:{rest}: {e}"))?);
                }
                "json" | "j" => {
                    let (path, value) = rest.split_once("==").or_else(|| rest.split_once('=')).ok_or("json: a path, = and a value (json:.status=failed)")?;
                    let value = serde_json::from_str(value.trim()).unwrap_or_else(|_| serde_json::Value::String(value.trim().to_string()));
                    f.json.push(JsonEq { path: parse_path(path)?, value });
                }
                "size" => {
                    let num = |s: &str| -> Result<u32, String> { parse_size(s).ok_or(format!("size:{rest}: sizes are numbers (100, 4k, 1m)")) };
                    if let Some((a, b)) = rest.split_once("..") {
                        f.min_size = (!a.is_empty()).then(|| num(a)).transpose()?;
                        f.max_size = (!b.is_empty()).then(|| num(b)).transpose()?;
                    } else if let Some(n) = rest.strip_prefix(">=") {
                        f.min_size = Some(num(n)?);
                    } else if let Some(n) = rest.strip_prefix('>') {
                        f.min_size = Some(num(n)?.saturating_add(1));
                    } else if let Some(n) = rest.strip_prefix("<=") {
                        f.max_size = Some(num(n)?);
                    } else if let Some(n) = rest.strip_prefix('<') {
                        f.max_size = Some(num(n)?.saturating_sub(1));
                    } else {
                        let n = num(rest)?;
                        f.min_size = Some(n);
                        f.max_size = Some(n);
                    }
                }
                _ if key.is_empty() && looks_like_subject(&w) => f.subjects.push(w.clone()),
                _ => f.words.push(w.to_lowercase()),
            }
        }
        Ok(f)
    }

    pub fn is_empty(&self) -> bool {
        self.subjects.is_empty() && self.headers.is_empty() && self.regexes.is_empty() && self.json.is_empty() && self.min_size.is_none() && self.max_size.is_none() && self.words.is_empty()
    }

    /// Whether a subject can hold matches: its pattern.
    pub fn subject_ok(&self, subject: &str) -> bool {
        self.subjects.iter().all(|p| fenix_nats::subject::matches(p, subject))
    }

    /// Whether every word is in the subject: then the payload needn't be
    /// looked in for them.
    pub fn words_in_subject(&self, subject: &str) -> bool {
        let s = subject.to_lowercase();
        self.words.iter().all(|w| s.contains(w.as_str()))
    }

    /// Whether it says anything of a message beyond its subject.
    pub fn per_message(&self) -> bool {
        !self.headers.is_empty() || !self.regexes.is_empty() || !self.json.is_empty() || self.min_size.is_some() || self.max_size.is_some() || !self.words.is_empty()
    }

    /// The cheap part, from the entry alone.
    pub fn size_ok(&self, size: u32) -> bool {
        self.min_size.is_none_or(|m| size >= m) && self.max_size.is_none_or(|m| size <= m)
    }

    /// Whether message `m` matches, the subject's pattern taken as checked.
    /// `words_in_subject` says the words are in its subject already.
    pub fn message_ok(&self, m: &View<'_>, words_in_subject: bool) -> bool {
        if !self.size_ok(m.entry.size) {
            return false;
        }
        if !self.headers.is_empty() {
            let Some(h) = m.headers.and_then(|h| fenix_nats::headers::Headers::parse(h).ok()) else { return false };
            for (k, v) in &self.headers {
                let ok = h.iter().any(|(n, val)| n.eq_ignore_ascii_case(k) && v.as_deref().is_none_or(|v| val == v));
                if !ok {
                    return false;
                }
            }
        }
        if !words_in_subject && !self.words.is_empty() {
            let subject = m.subject.to_lowercase();
            if !self.words.iter().all(|w| subject.contains(w.as_str()) || contains_ignore_case(m.payload, w.as_bytes())) {
                return false;
            }
        }
        if !self.regexes.iter().all(|re| re.is_match(m.payload)) {
            return false;
        }
        if !self.json.is_empty() {
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(m.payload) else { return false };
            if !self.json.iter().all(|j| lookup(&v, &j.path) == Some(&j.value)) {
                return false;
            }
        }
        true
    }

    /// All of it: the subject, then the message.
    pub fn matches(&self, m: &View<'_>) -> bool {
        self.subject_ok(m.subject) && self.message_ok(m, self.words_in_subject(m.subject))
    }

    /// Both: what each says must hold.
    pub fn and(&self, other: &Filter) -> Filter {
        let mut f = self.clone();
        f.source = format!("{} {}", self.source, other.source).trim().to_string();
        f.subjects.extend(other.subjects.iter().cloned());
        f.headers.extend(other.headers.iter().cloned());
        f.regexes.extend(other.regexes.iter().cloned());
        f.json.extend(other.json.iter().cloned());
        f.min_size = match (self.min_size, other.min_size) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        f.max_size = match (self.max_size, other.max_size) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        f.words.extend(other.words.iter().cloned());
        f
    }
}

/// `100`, `4k`, `1m`, `2KiB`.
pub fn parse_size(s: &str) -> Option<u32> {
    let s = s.trim().to_ascii_lowercase();
    let s = s.trim_end_matches("ib").trim_end_matches('b');
    let (num, mul) = match s.chars().last()? {
        'k' => (&s[..s.len() - 1], 1024),
        'm' => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s, 1),
    };
    num.trim().parse::<u32>().ok()?.checked_mul(mul)
}

/// Whether `hay` has `needle` (lower case), any case, ASCII.
pub fn contains_ignore_case(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let first = needle[0];
    let upper = first.to_ascii_uppercase();
    let last = hay.len() - needle.len();
    let mut i = 0;
    while i <= last {
        let b = hay[i];
        if (b == first || b == upper) && hay[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            return true;
        }
        i += 1;
    }
    // Non-ASCII words: as text.
    if !needle.is_ascii() {
        return String::from_utf8_lossy(hay).to_lowercase().contains(std::str::from_utf8(needle).unwrap_or(""));
    }
    false
}

/// How a rule colours what it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    Accent,
    Good,
    Warn,
    Bad,
    Rgb(u8, u8, u8),
}

impl Colour {
    pub fn parse(s: &str) -> Option<Colour> {
        Some(match s.to_ascii_lowercase().as_str() {
            "accent" | "blue" => Colour::Accent,
            "good" | "green" => Colour::Good,
            "warn" | "yellow" | "orange" => Colour::Warn,
            "bad" | "red" => Colour::Bad,
            hex if hex.starts_with('#') && hex.len() == 7 => {
                let n = u32::from_str_radix(&hex[1..], 16).ok()?;
                Colour::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
            }
            _ => return None,
        })
    }

    pub fn name(&self) -> String {
        match self {
            Colour::Accent => "accent".into(),
            Colour::Good => "good".into(),
            Colour::Warn => "warn".into(),
            Colour::Bad => "bad".into(),
            Colour::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// A highlight rule: what it matches is coloured, not hidden; with
/// `notify`, it says so.
#[derive(Debug, Clone)]
pub struct Rule {
    pub colour: Colour,
    pub notify: bool,
    pub filter: Filter,
}

impl Rule {
    /// `warn orders.* json:.status=failed`; `!bad re:panic` notifies.
    pub fn parse(line: &str) -> Result<Rule, String> {
        let line = line.trim();
        let (head, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let (notify, colour) = match head.strip_prefix('!') {
            Some(c) => (true, c),
            None => (false, head),
        };
        let colour = Colour::parse(colour).ok_or_else(|| format!("{line}: a rule starts with its colour (accent, good, warn, bad, #rrggbb), ! before it to be told"))?;
        let filter = Filter::parse(rest)?;
        if filter.is_empty() {
            return Err(format!("{line}: a rule needs a filter after its colour"));
        }
        Ok(Rule { colour, notify, filter })
    }

    pub fn line(&self) -> String {
        format!("{}{} {}", if self.notify { "!" } else { "" }, self.colour.name(), self.filter.source)
    }
}

/// Reads rules a line each; `#` comments and blank lines skipped.
pub fn parse_rules(text: &str) -> Result<Vec<Rule>, String> {
    let comment = |l: &str| l.starts_with('#') && Colour::parse(l.split_whitespace().next().unwrap_or("")).is_none();
    let rules: Vec<Rule> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !comment(l)).map(Rule::parse).collect::<Result<_, _>>()?;
    if rules.len() > 64 {
        return Err("at most 64 rules".into());
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Limits, Store};

    fn with_msg(subject: &str, headers: Option<&[u8]>, payload: &[u8], f: impl FnOnce(&View<'_>)) {
        let mut s = Store::new(Limits::default(), 0);
        let id = s.subject(0, subject, 0);
        s.count(id, payload.len(), 0);
        let seq = s.store(id, None, headers, 0, payload, 0);
        f(&s.get(id, seq).unwrap());
    }

    #[test]
    fn a_filter_is_read() {
        let f = Filter::parse(r#"orders.> header:Nats-Msg-Id h:region=eu re:"time(out)?" json:.status=failed size:100..2k Error"#).unwrap();
        assert_eq!(f.subjects, ["orders.>"]);
        assert_eq!(f.headers, [("Nats-Msg-Id".to_string(), None), ("region".to_string(), Some("eu".to_string()))]);
        assert_eq!(f.regexes[0].as_str(), "time(out)?");
        assert_eq!(f.json[0].path, [Step::Key("status".into())]);
        assert_eq!(f.json[0].value, serde_json::json!("failed"));
        assert_eq!((f.min_size, f.max_size), (Some(100), Some(2048)));
        assert_eq!(f.words, ["error"]);
        assert!(Filter::parse("re:(").is_err());
        assert!(Filter::parse("subject:a..b").is_err());
        assert!(Filter::parse("size:big").is_err());
        assert!(Filter::parse("").unwrap().is_empty());
        assert_eq!(Filter::parse("hello.world").unwrap().words, ["hello.world"]);
        assert_eq!(Filter::parse("hello.*").unwrap().subjects, ["hello.*"]);
        assert_eq!(Filter::parse("hello").unwrap().words, ["hello"]);
    }

    #[test]
    fn paths_are_read() {
        assert_eq!(parse_path("$.a.b[2]").unwrap(), [Step::Key("a".into()), Step::Key("b".into()), Step::Index(2)]);
        assert_eq!(parse_path(".a[\"x.y\"]").unwrap(), [Step::Key("a".into()), Step::Key("x.y".into())]);
        assert_eq!(parse_path("status").unwrap(), [Step::Key("status".into())]);
        assert!(parse_path(".a[x]").is_err());
        let v = serde_json::json!({"a": {"b": [1, 2, {"c": true}]}});
        assert_eq!(lookup(&v, &parse_path(".a.b[2].c").unwrap()), Some(&serde_json::json!(true)));
        assert_eq!(lookup(&v, &parse_path(".a.z").unwrap()), None);
    }

    #[test]
    fn messages_match_or_not() {
        let headers = b"NATS/1.0\r\nregion: eu\r\nNats-Msg-Id: 7\r\n\r\n";
        with_msg("orders.new", Some(headers), br#"{"status":"failed","n":3,"msg":"Timeout here"}"#, |m| {
            for yes in ["orders.>", "orders.new", "timeout", "TIMEOUT orders", "h:region=eu", "h:nats-msg-id", "json:.status=failed", "json:.n=3", "re:Time(out)?", "size:<1k", "new"] {
                assert!(Filter::parse(yes).unwrap().matches(m), "{yes}");
            }
            for no in ["billing.>", "orders.*.x", "absent", "h:region=us", "h:x", "json:.status=ok", "json:.n=\"3\"", "re:^x", "size:>1k"] {
                assert!(!Filter::parse(no).unwrap().matches(m), "{no}");
            }
        });
        with_msg("bin", None, &[0xff, 0, 1], |m| {
            assert!(!Filter::parse("json:.a=1").unwrap().matches(m));
            assert!(Filter::parse("bin").unwrap().matches(m));
        });
    }

    #[test]
    fn case_is_ignored_in_payloads() {
        assert!(contains_ignore_case(b"Hello World", b"world"));
        assert!(contains_ignore_case(b"xxWORLD", b"world"));
        assert!(!contains_ignore_case(b"worl", b"world"));
        assert!(contains_ignore_case("Ärger".as_bytes(), "ärger".as_bytes()));
    }

    #[test]
    fn filters_combine() {
        let a = Filter::parse("orders.> size:>10").unwrap();
        let b = Filter::parse("error size:>20 size:<100").unwrap();
        let c = a.and(&b);
        assert_eq!((c.subjects.len(), c.min_size, c.max_size, c.words.len()), (1, Some(21), Some(99), 1));
    }

    #[test]
    fn rules_are_read() {
        let rules = parse_rules("# mine\nwarn orders.* json:.status=failed\n\n!bad re:panic\n#123456 h:x").unwrap();
        assert_eq!(rules.len(), 3);
        assert!(!rules[0].notify && rules[1].notify);
        assert_eq!(rules[2].colour, Colour::Rgb(0x12, 0x34, 0x56));
        assert_eq!(rules[1].line(), "!bad re:panic");
        assert!(parse_rules("purple x").is_err());
        assert!(parse_rules("warn").is_err());
    }
}
