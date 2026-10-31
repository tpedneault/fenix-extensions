//! URLs: percent-encoding, a query's parameters taken apart and put back.

/// `s` percent-encoded for a query or a form: what isn't unreserved
/// (RFC 3986) is `%XX`; `form` writes a space as `+`.
pub fn encode(s: &str, form: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' if form => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `%XX` and `+` (a space) read back.
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 3;
                    continue;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A URL's path and its query's parameters, as written (not decoded): a
/// `{{variable}}` in one stays as it is.
pub fn split(url: &str) -> (&str, Vec<(String, String)>) {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, q),
        None => return (url, Vec::new()),
    };
    let query = query.split('#').next().unwrap_or("");
    let params = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (p.to_string(), String::new()),
        })
        .collect();
    (base, params)
}

/// `base` with `params` as its query.
pub fn join(base: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return base.to_string();
    }
    let q: Vec<String> = params.iter().map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}={v}") }).collect();
    format!("{base}?{}", q.join("&"))
}

/// Pairs as a form body: `a=1&b=two+words`.
pub fn form(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k, true), encode(v, true))).collect::<Vec<_>>().join("&")
}

/// A form body read back into pairs, decoded.
pub fn parse_form(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

/// A URL's scheme, host and port: `https`, `api.example.com`, 443.
pub fn host(url: &str) -> Option<(String, String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (h, after) = v6.split_once(']')?;
            (h, after.strip_prefix(':').and_then(|p| p.parse::<u16>().ok()))
        }
        None => match authority.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(p) => (h, Some(p)),
                Err(_) => (authority, None),
            },
            None => (authority, None),
        },
    };
    let port = port.unwrap_or(if scheme.eq_ignore_ascii_case("https") { 443 } else { 80 });
    Some((scheme.to_ascii_lowercase(), host.to_ascii_lowercase(), port))
}

/// A URL's path, `/` when it has none.
pub fn path(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let after = match rest.find('/') {
        Some(i) => &rest[i..],
        None => "/",
    };
    after.split(['?', '#']).next().unwrap_or("/").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_goes_both_ways() {
        assert_eq!(encode("a b&c=d/é", false), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode("a b", true), "a+b");
        assert_eq!(decode("a%20b+c%26%C3%A9"), "a b c&é");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn a_query_is_taken_apart_and_put_back() {
        let (base, params) = split("https://x.test/a?page=2&q={{term}}&flag#top");
        assert_eq!(base, "https://x.test/a");
        assert_eq!(params, [("page".into(), "2".into()), ("q".into(), "{{term}}".into()), ("flag".into(), String::new())]);
        assert_eq!(join(base, &params), "https://x.test/a?page=2&q={{term}}&flag");
        assert_eq!(join(base, &[]), base);
        assert_eq!(form(&[("name".into(), "Ada L".into()), ("x".into(), "1&2".into())]), "name=Ada+L&x=1%262");
        assert_eq!(parse_form("name=Ada+L&x=1%262"), [("name".into(), "Ada L".into()), ("x".into(), "1&2".into())]);
    }

    #[test]
    fn hosts_and_paths() {
        assert_eq!(host("https://API.example.com/v1?x"), Some(("https".into(), "api.example.com".into(), 443)));
        assert_eq!(host("http://localhost:8080"), Some(("http".into(), "localhost".into(), 8080)));
        assert_eq!(host("http://u:p@[::1]:9000/x"), Some(("http".into(), "::1".into(), 9000)));
        assert_eq!(host("not a url"), None);
        assert_eq!(path("https://x.test/a/b?c"), "/a/b");
        assert_eq!(path("https://x.test"), "/");
    }
}
