//! `.http` files as VS Code's REST Client and JetBrains' HTTP client
//! write them: requests between `###` lines, each a request line (`GET
//! https://...`, the method left out for GET), its query carried on
//! indented `?` and `&` lines, headers, a blank line and a body; file
//! variables `@name = value`; comments `#` and `//`, and directives in
//! them (`# @name login`, `# @no-redirect`).
//!
//! Everything keeps its line, so the inline status goes after the
//! request line and an edit from the GUI goes back to where it came from.

/// A file variable: `@host = https://example.com`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVar {
    pub name: String,
    pub value: String,
    pub line: u32,
}

/// A directive in a comment before or in a request: `# @name login`,
/// `# @timeout 5`, `// @no-redirect`, `# @test status == 200`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub name: String,
    pub value: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub line: u32,
}

/// A request's body: its lines, as written (`< ./file` lines and all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub text: String,
    /// Its first line, and the line after its last.
    pub start: u32,
    pub end: u32,
}

/// What's done with the response, after the body: `>> file` saves it
/// (`>>!` over what's there); `> {% %}` and `> file.js` are JetBrains'
/// handler scripts, kept but not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    Save { path: String, overwrite: bool, line: u32 },
    Script { line: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// What `### ...` says on the separator above it.
    pub title: Option<String>,
    /// The section's first line (its `###`, or the file's first line),
    /// and the line after its last.
    pub start: u32,
    pub end: u32,
    /// The request line.
    pub line: u32,
    /// The lines its URL takes, from `line`: more than one when its
    /// query goes on below.
    pub url_lines: u32,
    pub method: String,
    /// Whether the method was written (`GET` left out is GET).
    pub method_given: bool,
    pub url: String,
    pub version: Option<String>,
    pub headers: Vec<Header>,
    pub body: Option<Body>,
    pub directives: Vec<Directive>,
    pub after: Vec<After>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpFile {
    pub vars: Vec<FileVar>,
    pub requests: Vec<Request>,
}

/// The methods known by name; any other word in capitals before a URL is
/// taken as one too (`LIST`, `PURGE`).
pub const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT", "GRAPHQL", "WEBSOCKET"];

fn is_separator(line: &str) -> bool {
    line.starts_with("###")
}

/// A comment's text, `#` or `//` and the space after it gone.
fn comment(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix('#').or_else(|| t.strip_prefix("//")).map(str::trim)
}

/// A comment's directive: `@name value`.
fn directive(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix('@')?;
    let end = rest.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return None;
    }
    let value = rest[end..].trim_start().trim_start_matches('=').trim();
    Some((name.to_ascii_lowercase(), value.to_string()))
}

/// `@name = value`, a file variable.
pub fn file_var(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '$')) {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

/// A header line: `Name: value`.
pub fn header(line: &str) -> Option<(String, String)> {
    let (name, value) = line.split_once(':')?;
    let name = name.trim_end();
    if name.is_empty() || name.starts_with(char::is_whitespace) || !name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)) {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

/// A request line: its method (and whether it was written), URL and
/// version.
pub fn request_line(line: &str) -> (String, bool, String, Option<String>) {
    let t = line.trim();
    let (method, given, rest) = match t.split_once(char::is_whitespace) {
        Some((m, rest)) if !m.is_empty() && m.chars().all(|c| c.is_ascii_uppercase()) => (m.to_string(), true, rest.trim()),
        _ => ("GET".to_string(), false, t),
    };
    let (url, version) = match rest.rsplit_once(char::is_whitespace) {
        Some((url, v)) if v.starts_with("HTTP/") => (url.trim_end().to_string(), Some(v.to_string())),
        _ => (rest.to_string(), None),
    };
    (method, given, url, version)
}

/// A line carrying the URL's query on: indented, `?` or `&` first.
fn continues_url(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && matches!(line.trim_start().chars().next(), Some('?' | '&'))
}

/// A line that goes after the body: `> {% ... %}`, `> handler.js`,
/// `>> file`, `>>! file`.
fn after_line(line: &str) -> bool {
    line.starts_with('>')
}

impl Request {
    /// What chained requests call it (`{{login.response...}}`): `# @name`,
    /// else a `###` title that's a single word.
    pub fn name(&self) -> Option<&str> {
        if let Some(d) = self.directive("name") {
            return Some(d.value.as_str()).filter(|v| !v.is_empty());
        }
        self.title.as_deref().filter(|t| !t.is_empty() && !t.contains(char::is_whitespace))
    }

    /// What it's shown as: its name or title, else its method and URL.
    pub fn label(&self) -> String {
        match self.directive("name").map(|d| d.value.as_str()).filter(|v| !v.is_empty()).or(self.title.as_deref().filter(|t| !t.is_empty())) {
            Some(t) => t.to_string(),
            None => format!("{} {}", self.method, self.url),
        }
    }

    pub fn directive(&self, name: &str) -> Option<&Directive> {
        self.directives.iter().rev().find(|d| d.name == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.directive(name).is_some()
    }

    /// Every `# @test` it has.
    pub fn tests(&self) -> impl Iterator<Item = &Directive> {
        self.directives.iter().filter(|d| d.name == "test")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().rev().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
    }

    /// The line after its last header: where a new one goes.
    pub fn headers_end(&self) -> u32 {
        self.headers.iter().map(|h| h.line + 1).max().unwrap_or(self.line + self.url_lines).max(self.line + self.url_lines)
    }

    /// Whether `line` is in its section.
    pub fn contains(&self, line: u32) -> bool {
        (self.start..self.end).contains(&line)
    }
}

impl HttpFile {
    pub fn parse(text: &str) -> HttpFile {
        let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
        let mut file = HttpFile::default();
        let mut i = 0;
        let mut section_start = 0;
        let mut title: Option<String> = None;
        let mut pending: Vec<Directive> = Vec::new();
        while i < lines.len() {
            let line = lines[i];
            let n = i as u32;
            if is_separator(line) {
                section_start = n;
                title = Some(line.trim_start_matches('#').trim().to_string()).filter(|t| !t.is_empty());
                pending.clear();
                i += 1;
                continue;
            }
            if line.trim().is_empty() {
                i += 1;
                continue;
            }
            if let Some(text) = comment(line) {
                if let Some((name, value)) = directive(text) {
                    pending.push(Directive { name, value, line: n });
                }
                i += 1;
                continue;
            }
            if let Some((name, value)) = file_var(line) {
                file.vars.push(FileVar { name, value, line: n });
                i += 1;
                continue;
            }
            // The request line, and what follows it to the next `###`.
            let (method, method_given, mut url, version) = request_line(line);
            let mut url_lines = 1;
            let mut version = version;
            i += 1;
            while i < lines.len() && continues_url(lines[i]) {
                let (part, v) = match lines[i].trim().rsplit_once(char::is_whitespace) {
                    Some((p, v)) if v.starts_with("HTTP/") => (p.trim_end().to_string(), Some(v.to_string())),
                    _ => (lines[i].trim().to_string(), None),
                };
                url.push_str(&part);
                version = v.or(version);
                url_lines += 1;
                i += 1;
            }
            let mut req = Request {
                title: title.take(),
                start: section_start,
                end: 0,
                line: n,
                url_lines,
                method,
                method_given,
                url,
                version,
                headers: Vec::new(),
                body: None,
                directives: std::mem::take(&mut pending),
                after: Vec::new(),
            };
            // Headers, up to a blank line.
            while i < lines.len() && !is_separator(lines[i]) && !lines[i].trim().is_empty() {
                let l = lines[i];
                if let Some(text) = comment(l) {
                    if let Some((name, value)) = directive(text) {
                        req.directives.push(Directive { name, value, line: i as u32 });
                    }
                } else if let Some((name, value)) = header(l) {
                    req.headers.push(Header { name, value, line: i as u32 });
                } else {
                    // Not a header: the body, with no blank line before it.
                    break;
                }
                i += 1;
            }
            // The body, up to the next `###` or a `>` line.
            let body_from = {
                let mut j = i;
                while j < lines.len() && !is_separator(lines[j]) && lines[j].trim().is_empty() {
                    j += 1;
                }
                j
            };
            let mut j = body_from;
            while j < lines.len() && !is_separator(lines[j]) && !after_line(lines[j]) {
                j += 1;
            }
            let mut last = j;
            while last > body_from && lines[last - 1].trim().is_empty() {
                last -= 1;
            }
            if last > body_from {
                req.body = Some(Body { text: lines[body_from..last].join("\n"), start: body_from as u32, end: last as u32 });
            }
            // What's done with the response.
            while j < lines.len() && !is_separator(lines[j]) {
                let l = lines[j];
                if let Some(rest) = l.strip_prefix(">>") {
                    let (overwrite, path) = match rest.strip_prefix('!') {
                        Some(p) => (true, p),
                        None => (false, rest),
                    };
                    req.after.push(After::Save { path: path.trim().to_string(), overwrite, line: j as u32 });
                } else if l.starts_with('>') {
                    req.after.push(After::Script { line: j as u32 });
                    if l.contains("{%") && !l.contains("%}") {
                        while j + 1 < lines.len() && !lines[j].contains("%}") {
                            j += 1;
                        }
                    }
                }
                j += 1;
            }
            i = j;
            // Its section ends where the next begins.
            req.end = (i as u32).max(req.line + 1);
            file.requests.push(req);
            section_start = i as u32;
        }
        file
    }

    /// The request whose section holds `line`; between requests, the
    /// next one.
    pub fn at(&self, line: u32) -> Option<usize> {
        self.requests.iter().position(|r| r.contains(line)).or_else(|| self.requests.iter().position(|r| r.line >= line))
    }

    pub fn named(&self, name: &str) -> Option<&Request> {
        self.requests.iter().find(|r| r.name() == Some(name))
    }

    /// File variable `name`: the last one set wins.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.iter().rev().find(|v| v.name == name).map(|v| v.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@host = https://api.example.com\n@token = abc\n\n### Log in\n# @name login\nPOST {{host}}/login HTTP/1.1\nContent-Type: application/json\n\n{\n  \"user\": \"ada\"\n}\n\n> {%\n  client.global.set(\"t\", response.body.token);\n%}\n\n###\n// @no-redirect\nGET {{host}}/users\n    ?page=2\n    &limit=10\nAuthorization: Bearer {{login.response.body.$.token}}\n# @test status == 200\n\n### Upload\nPOST {{host}}/files\nContent-Type: application/octet-stream\n\n< ./logo.png\n\n>> out/answer.json\n";

    #[test]
    fn requests_and_variables_are_read() {
        let f = HttpFile::parse(SAMPLE);
        assert_eq!(f.vars.len(), 2);
        assert_eq!(f.var("host"), Some("https://api.example.com"));
        assert_eq!(f.requests.len(), 3);
        let login = &f.requests[0];
        assert_eq!((login.method.as_str(), login.url.as_str(), login.version.as_deref(), login.line), ("POST", "{{host}}/login", Some("HTTP/1.1"), 5));
        assert_eq!(login.name(), Some("login"));
        assert_eq!(login.title.as_deref(), Some("Log in"));
        assert_eq!(login.header("content-type"), Some("application/json"));
        let body = login.body.as_ref().unwrap();
        assert_eq!(body.text, "{\n  \"user\": \"ada\"\n}");
        assert_eq!((body.start, body.end), (8, 11));
        assert_eq!(login.after, [After::Script { line: 12 }]);
        assert_eq!((login.start, login.end), (3, 16));
    }

    #[test]
    fn a_query_goes_on_below_and_directives_are_kept() {
        let f = HttpFile::parse(SAMPLE);
        let users = &f.requests[1];
        assert_eq!(users.url, "{{host}}/users?page=2&limit=10");
        assert_eq!((users.line, users.url_lines), (18, 3));
        assert!(users.has("no-redirect"));
        assert_eq!(users.tests().map(|d| d.value.as_str()).collect::<Vec<_>>(), ["status == 200"]);
        assert_eq!(users.headers.len(), 1);
        assert!(users.body.is_none());
        assert_eq!(users.name(), None);
        assert_eq!(users.label(), "GET {{host}}/users?page=2&limit=10");
        let upload = &f.requests[2];
        assert_eq!(upload.body.as_ref().map(|b| b.text.as_str()), Some("< ./logo.png"));
        assert_eq!(upload.after, [After::Save { path: "out/answer.json".into(), overwrite: false, line: 30 }]);
        assert_eq!(upload.name(), Some("Upload"));
    }

    #[test]
    fn the_request_at_a_line() {
        let f = HttpFile::parse(SAMPLE);
        assert_eq!(f.at(0), Some(0), "above the first: the first");
        assert_eq!(f.at(9), Some(0));
        assert_eq!(f.at(17), Some(1));
        assert_eq!(f.at(27), Some(2));
        assert_eq!(f.named("login").map(|r| r.line), Some(5));
    }

    #[test]
    fn a_bare_url_is_a_get_and_crlf_is_read() {
        let f = HttpFile::parse("https://example.com/a\r\nAccept: */*\r\n\r\n###\r\nhttps://example.com/b HTTP/2\r\n");
        assert_eq!(f.requests.len(), 2);
        assert_eq!((f.requests[0].method.as_str(), f.requests[0].method_given), ("GET", false));
        assert_eq!(f.requests[0].header("Accept"), Some("*/*"));
        assert_eq!(f.requests[1].version.as_deref(), Some("HTTP/2"));
        assert_eq!(f.requests[1].url, "https://example.com/b");
    }

    #[test]
    fn a_body_with_no_blank_line_before_it() {
        let f = HttpFile::parse("POST https://x.test\nContent-Type: text/plain\nhello there\n");
        assert_eq!(f.requests[0].body.as_ref().map(|b| b.text.as_str()), Some("hello there"));
    }

    #[test]
    fn lines_are_told_apart() {
        assert_eq!(header("X-Api-Key: {{key}}"), Some(("X-Api-Key".into(), "{{key}}".into())));
        assert_eq!(header("not a header"), None);
        assert_eq!(header("{\"a\": 1}"), None);
        assert_eq!(file_var("@base_url = http://localhost:8080"), Some(("base_url".into(), "http://localhost:8080".into())));
        assert_eq!(directive("@timeout 5 s"), Some(("timeout".into(), "5 s".into())));
        assert_eq!(directive("@name=login"), Some(("name".into(), "login".into())));
        assert_eq!(directive("just a comment"), None);
        assert_eq!(request_line("DELETE /x HTTP/1.1"), ("DELETE".into(), true, "/x".into(), Some("HTTP/1.1".into())));
    }
}
