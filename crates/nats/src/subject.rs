//! Subjects: tokens between dots, and the wildcards a subscription may
//! use -- `*` one token, `>` every token left.

/// Whether `s` is a subject a message can be published to: tokens, no
/// wildcards, no spaces.
pub fn check_subject(s: &str) -> Result<(), String> {
    check(s, false)
}

/// Whether `s` is a subject a subscription can take: `*` a whole token,
/// `>` a whole last one.
pub fn check_pattern(s: &str) -> Result<(), String> {
    check(s, true)
}

fn check(s: &str, wildcards: bool) -> Result<(), String> {
    if s.is_empty() {
        return Err("a subject can't be empty".into());
    }
    if s.bytes().any(|b| b <= b' ' || b == 127) {
        return Err(format!("{s:?}: a subject has no spaces or control characters"));
    }
    let mut tokens = s.split('.').peekable();
    while let Some(t) = tokens.next() {
        if t.is_empty() {
            return Err(format!("{s:?}: an empty token (two dots, or a dot at an end)"));
        }
        let wild = t == "*" || t == ">";
        if !wildcards && wild {
            return Err(format!("{s:?}: wildcards are for subscriptions, not for publishing"));
        }
        if t == ">" && tokens.peek().is_some() {
            return Err(format!("{s:?}: > can only be the last token"));
        }
    }
    Ok(())
}

/// Whether `pattern` has a wildcard.
pub fn is_wildcard(pattern: &str) -> bool {
    pattern.split('.').any(|t| t == "*" || t == ">")
}

/// Whether `subject` matches `pattern`, token by token.
pub fn matches(pattern: &str, subject: &str) -> bool {
    let mut p = pattern.split('.');
    let mut s = subject.split('.');
    loop {
        match (p.next(), s.next()) {
            (Some(">"), Some(_)) => return true,
            (Some("*"), Some(_)) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// The subjects both `a` and `b` match, as one pattern, or `None` when
/// they have none in common: what a subscription to `a` keeps of `b`.
pub fn intersect(a: &str, b: &str) -> Option<String> {
    let at: Vec<&str> = a.split('.').collect();
    let bt: Vec<&str> = b.split('.').collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    loop {
        match (at.get(i).copied(), bt.get(i).copied()) {
            (Some(">"), Some(_)) => {
                out.extend_from_slice(&bt[i..]);
                break;
            }
            (Some(_), Some(">")) => {
                out.extend_from_slice(&at[i..]);
                break;
            }
            (Some("*"), Some(t)) | (Some(t), Some("*")) => out.push(t),
            (Some(x), Some(y)) if x == y => out.push(x),
            (None, None) => break,
            _ => return None,
        }
        i += 1;
    }
    Some(out.join("."))
}

/// Whether every subject `inner` matches, `outer` matches too.
pub fn covers(outer: &str, inner: &str) -> bool {
    intersect(outer, inner).as_deref() == Some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_and_patterns_are_checked() {
        assert!(check_subject("orders.eu.new").is_ok());
        assert!(check_subject("$JS.API.STREAM.INFO.ORDERS").is_ok());
        assert!(check_subject("a*b.c").is_ok(), "a * inside a token is a character");
        for bad in ["", "a..b", ".a", "a.", "a b", "a\tb", "a.*", "a.>"] {
            assert!(check_subject(bad).is_err(), "{bad:?}");
        }
        assert!(check_pattern("a.*.c").is_ok());
        assert!(check_pattern(">").is_ok());
        assert!(check_pattern("a.>.b").is_err());
        assert!(is_wildcard("a.*") && is_wildcard(">") && !is_wildcard("a*.b"));
    }

    #[test]
    fn wildcards_match() {
        assert!(matches(">", "a"));
        assert!(matches(">", "a.b.c"));
        assert!(matches("a.>", "a.b"));
        assert!(!matches("a.>", "a"));
        assert!(matches("a.*.c", "a.b.c"));
        assert!(!matches("a.*.c", "a.b.d"));
        assert!(!matches("a.*", "a.b.c"));
        assert!(matches("a.b", "a.b"));
        assert!(!matches("a.b", "a.bc"));
        assert!(!matches("a.b.c", "a.b"));
    }

    #[test]
    fn patterns_intersect() {
        assert_eq!(intersect(">", "orders.*").as_deref(), Some("orders.*"));
        assert_eq!(intersect("orders.>", "*.eu").as_deref(), Some("orders.eu"));
        assert_eq!(intersect("orders.*", "orders.eu.new"), None);
        assert_eq!(intersect("a.*.c", "a.b.*").as_deref(), Some("a.b.c"));
        assert_eq!(intersect("a.b", "a.c"), None);
        assert_eq!(intersect("a.>", "a.>").as_deref(), Some("a.>"));
        assert!(covers(">", "a.b") && covers("a.*", "a.b") && !covers("a.b", "a.*") && !covers("a.*", "a.>"));
    }
}
