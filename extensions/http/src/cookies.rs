//! Cookies, kept per environment as a browser keeps them (RFC 6265, the
//! parts a client needs): what `Set-Cookie` says is stored by its domain
//! and path, and sent back to the requests they match, until it expires.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    /// Sent only to `domain` itself, not its subdomains: no `Domain`
    /// attribute was given.
    pub host_only: bool,
    pub path: String,
    /// When it expires, in seconds since 1970; none ends with the session
    /// (kept until the jar is cleared, here).
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jar {
    pub cookies: Vec<Cookie>,
}

/// An HTTP date (`Wed, 21 Oct 2015 07:28:00 GMT`), and the older
/// `21-Oct-2015` spelling, in seconds since 1970.
pub fn http_date(s: &str) -> Option<i64> {
    use fenix_extension_api::chrono::DateTime;
    let s = s.trim();
    let fixed = s.replace('-', " ").replace("UTC", "GMT");
    DateTime::parse_from_rfc2822(s).or_else(|_| DateTime::parse_from_rfc2822(&fixed)).ok().map(|t| t.timestamp())
}

/// The folder of a request's path: a cookie's path when it doesn't say.
fn default_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || (host.ends_with(domain) && host[..host.len() - domain.len()].ends_with('.'))
}

fn path_matches(path: &str, cookie: &str) -> bool {
    path == cookie || (path.starts_with(cookie) && (cookie.ends_with('/') || path[cookie.len()..].starts_with('/')))
}

/// A `Set-Cookie` header from `host` for `path`, read at `now`.
pub fn parse(header: &str, host: &str, path: &str, now: i64) -> Option<Cookie> {
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut c = Cookie { name: name.into(), value: value.trim().trim_matches('"').into(), domain: host.to_ascii_lowercase(), host_only: true, path: default_path(path), expires: None, secure: false, http_only: false };
    let mut max_age = None;
    for attr in parts {
        let (k, v) = attr.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((attr.trim(), ""));
        match k.to_ascii_lowercase().as_str() {
            "domain" if !v.is_empty() => {
                let d = v.trim_start_matches('.').to_ascii_lowercase();
                // A domain it may not set for: dropped.
                if !domain_matches(&c.domain, &d) {
                    return None;
                }
                c.domain = d;
                c.host_only = false;
            }
            "path" if v.starts_with('/') => c.path = v.into(),
            "expires" => {
                if let Some(t) = http_date(v) {
                    c.expires = Some(t);
                }
            }
            "max-age" => max_age = v.parse::<i64>().ok(),
            "secure" => c.secure = true,
            "httponly" => c.http_only = true,
            _ => {}
        }
    }
    if let Some(m) = max_age {
        c.expires = Some(if m <= 0 { i64::MIN } else { now + m });
    }
    Some(c)
}

impl Jar {
    /// What a response from `url` said to keep, or to forget.
    pub fn store<'a>(&mut self, url: &str, set_cookies: impl IntoIterator<Item = &'a str>, now: i64) {
        let Some((_, host, _)) = crate::url::host(url) else { return };
        let path = crate::url::path(url);
        for header in set_cookies {
            let Some(c) = parse(header, &host, &path, now) else { continue };
            self.cookies.retain(|o| !(o.name == c.name && o.domain == c.domain && o.path == c.path));
            if c.expires.is_none_or(|e| e > now) {
                self.cookies.push(c);
            }
        }
    }

    /// The cookies a request to `url` sends, as a `Cookie` header's value.
    pub fn header(&self, url: &str, now: i64) -> Option<String> {
        let (scheme, host, _) = crate::url::host(url)?;
        let path = crate::url::path(url);
        let mut matching: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| c.expires.is_none_or(|e| e > now))
            .filter(|c| if c.host_only { host == c.domain } else { domain_matches(&host, &c.domain) })
            .filter(|c| path_matches(&path, &c.path))
            .filter(|c| !c.secure || scheme == "https" || host == "localhost" || host == "127.0.0.1")
            .collect();
        // Longer paths first, as browsers send them.
        matching.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        (!matching.is_empty()).then(|| matching.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
    }

    /// Those expired, gone.
    pub fn purge(&mut self, now: i64) {
        self.cookies.retain(|c| c.expires.is_none_or(|e| e > now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    #[test]
    fn set_cookie_is_read() {
        let c = parse("sid=abc123; Path=/; Domain=.example.com; Max-Age=3600; Secure; HttpOnly", "api.example.com", "/login", NOW).unwrap();
        assert_eq!((c.name.as_str(), c.value.as_str(), c.domain.as_str(), c.host_only, c.path.as_str()), ("sid", "abc123", "example.com", false, "/"));
        assert_eq!((c.expires, c.secure, c.http_only), (Some(NOW + 3600), true, true));
        let c = parse("theme=dark", "api.example.com", "/a/b/c", NOW).unwrap();
        assert_eq!((c.host_only, c.path.as_str(), c.expires), (true, "/a/b", None));
        let c = parse("x=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT", "h.test", "/", NOW).unwrap();
        assert_eq!(c.expires, Some(1_445_412_480));
        assert_eq!(http_date("Wed, 21-Oct-2015 07:28:00 GMT"), Some(1_445_412_480));
        assert!(parse("x=1; Domain=evil.test", "api.example.com", "/", NOW).is_none(), "not its domain");
        assert!(parse("novalue", "h.test", "/", NOW).is_none());
    }

    #[test]
    fn the_jar_sends_what_matches() {
        let mut jar = Jar::default();
        jar.store("https://api.example.com/login", ["sid=1; Domain=example.com; Path=/", "pref=a; Path=/v1", "tmp=x; Max-Age=10", "sec=s; Secure"], NOW);
        assert_eq!(jar.header("https://api.example.com/v1/users", NOW).as_deref(), Some("pref=a; sid=1; tmp=x; sec=s"));
        assert_eq!(jar.header("https://www.example.com/", NOW).as_deref(), Some("sid=1"), "a domain cookie on another host");
        assert_eq!(jar.header("http://api.example.com/v2", NOW).as_deref(), Some("sid=1; tmp=x"), "not secure, not /v1");
        assert_eq!(jar.header("https://api.example.com/v10", NOW).as_deref(), Some("sid=1; tmp=x; sec=s"), "/v1 isn't /v10's");
        assert_eq!(jar.header("https://api.example.com/", NOW + 11).as_deref(), Some("sid=1; sec=s"), "tmp expired");
        assert_eq!(jar.header("https://other.test/", NOW), None);
        // Set again, and taken away.
        jar.store("https://api.example.com/", ["sid=2; Domain=example.com; Path=/"], NOW);
        assert_eq!(jar.cookies.iter().filter(|c| c.name == "sid").map(|c| c.value.as_str()).collect::<Vec<_>>(), ["2"]);
        jar.store("https://api.example.com/", ["sid=; Domain=example.com; Path=/; Max-Age=0"], NOW);
        assert!(jar.cookies.iter().all(|c| c.name != "sid"));
        jar.purge(NOW + 11);
        assert!(jar.cookies.iter().all(|c| c.name != "tmp"));
    }
}
