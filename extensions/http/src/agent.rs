//! What coding agents are given (`http_requests`, `http_run`): the
//! project's requests listed as their files have them, and one of them
//! run -- found by its name, in the environment you chose -- answered as
//! its status, headers and body.
//!
//! Nothing here sends anything: it finds the request an agent means and
//! shapes what goes back. An agent names a request that's in the
//! project's `.http` files, never a URL of its own. What goes back has
//! the values of your secrets hidden wherever they'd show -- the URL, a
//! header, a body that echoes them -- and a long body cut, saying so.

use serde_json::{json, Value};

use crate::assert::Outcome;
use crate::model::{reason, Received};
use crate::parse::HttpFile;

/// The characters of a body answered when the agent doesn't say.
pub const DEFAULT_BODY: usize = 20_000;
/// The most it may ask for.
pub const MAX_BODY: usize = 100_000;
/// The most requests listed.
pub const MAX_LISTED: usize = 500;
/// What a secret's value is answered as.
pub const HIDDEN: &str = "«hidden»";

/// A path an agent gave, as the files are known: below the project,
/// without `/project/` or `./`.
pub fn rel_path(path: &str) -> String {
    let p = path.trim().replace('\\', "/");
    let p = p.strip_prefix("/project/").unwrap_or(&p);
    let p = p.strip_prefix("./").unwrap_or(p);
    p.trim_start_matches('/').to_string()
}

fn only<'a>(files: &'a [(String, HttpFile)], file: Option<&str>) -> Result<Vec<&'a (String, HttpFile)>, String> {
    match file.map(rel_path).filter(|f| !f.is_empty()) {
        None => Ok(files.iter().collect()),
        Some(f) => {
            let found: Vec<_> = files.iter().filter(|(rel, _)| *rel == f).collect();
            if found.is_empty() {
                return Err(format!("no .http file {f} in the project: http_requests lists them"));
            }
            Ok(found)
        }
    }
}

/// The requests of `files` (each a path below the project and its
/// text, parsed), or of the one `file` names: each one's file, the name
/// `http_run` takes, its method and URL as written -- `{{variables}}`
/// left as they are, so no value of theirs is in it -- and its line.
pub fn requests(files: &[(String, HttpFile)], file: Option<&str>) -> Result<Value, String> {
    let files = only(files, file)?;
    let mut out = Vec::new();
    let mut more = 0;
    for (rel, f) in files {
        for r in &f.requests {
            if out.len() == MAX_LISTED {
                more += 1;
                continue;
            }
            out.push(json!({ "file": rel, "name": r.label(), "method": r.method, "url": r.url, "line": r.line + 1 }));
        }
    }
    let mut v = json!({ "requests": out });
    if more > 0 {
        v["more"] = json!(format!("{more} more not listed: give file to see one file's"));
    }
    Ok(v)
}

/// The request `name` means, in `files` (or the one `file` names): its
/// file, its index there and its label. By its `# @name` or title, else
/// its label as listed; the same name in two files needs `file`.
pub fn find(files: &[(String, HttpFile)], name: &str, file: Option<&str>) -> Result<(String, usize, String), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("which request: its name, as http_requests lists it".into());
    }
    let files = only(files, file)?;
    let look = |same: &dyn Fn(&str) -> bool| -> Vec<(String, usize, String, u32)> {
        let mut found = Vec::new();
        for (rel, f) in &files {
            for (i, r) in f.requests.iter().enumerate() {
                if r.name().is_some_and(same) || same(&r.label()) {
                    found.push((rel.clone(), i, r.label(), r.line + 1));
                }
            }
        }
        found
    };
    let mut found = look(&|n| n == name);
    if found.is_empty() {
        found = look(&|n| n.eq_ignore_ascii_case(name));
    }
    match found.len() {
        0 => Err(format!("no request called {name:?} in the project's .http files: http_requests lists them")),
        1 => {
            let (rel, i, label, _) = found.remove(0);
            Ok((rel, i, label))
        }
        _ => {
            let at: Vec<String> = found.iter().map(|(rel, _, _, line)| format!("{rel}:{line}")).collect();
            Err(format!("{} requests are called {name:?} ({}): give file, or a name that's one of them alone", found.len(), at.join(", ")))
        }
    }
}

/// The values to hide: those at least 4 characters long (so `1` or
/// `dev` isn't hidden everywhere), and each as a URL has it encoded, the
/// longest first, so one inside another goes whole.
pub fn to_hide(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        for v in [v.clone(), crate::url::encode(v, false), crate::url::encode(v, true)] {
            if v.chars().count() >= 4 && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    out
}

/// `text` with each of `hidden` (from `to_hide`) put as `HIDDEN`.
pub fn redact(text: &str, hidden: &[String]) -> String {
    let mut out = text.to_string();
    for h in hidden {
        if out.contains(h.as_str()) {
            out = out.replace(h.as_str(), HIDDEN);
        }
    }
    out
}

/// A `Set-Cookie` value with the cookie's own value hidden: its name and
/// attributes kept (`session=«hidden»; Path=/; HttpOnly`).
fn cookie(value: &str) -> String {
    let (first, rest) = value.split_once(';').map_or((value, None), |(a, b)| (a, Some(b)));
    let name = first.split_once('=').map_or(first, |(n, _)| n).trim();
    match rest {
        Some(rest) => format!("{name}={HIDDEN};{rest}"),
        None => format!("{name}={HIDDEN}"),
    }
}

/// The body as an agent reads it: its text, cut to `max` characters with
/// a line saying so; or, when it isn't text, what it is.
pub fn body(r: &Received, max: usize) -> (String, bool) {
    let Ok(text) = std::str::from_utf8(&r.body) else {
        let ct = r.content_type();
        return (format!("[{} bytes of {}, not text]", r.body.len(), if ct.is_empty() { "binary" } else { ct }), false);
    };
    let total = text.chars().count();
    if total <= max {
        return (text.to_string(), false);
    }
    let shown: String = text.chars().take(max).collect();
    (format!("{shown}\n[… cut: {max} of {total} characters shown; max_body answers more, up to {MAX_BODY}]"), true)
}

/// Which request an agent ran: its label and file, the method it went
/// as, the environment it went in.
pub struct Ran<'a> {
    pub label: &'a str,
    pub file: &'a str,
    pub method: &'a str,
    pub env: Option<&'a str>,
}

/// What a request an agent ran answers: which it was, the environment,
/// the status, the headers (cookies' values hidden), the body cut to
/// `max_body` characters, how long it took, its tests -- each of
/// `hidden`'s values (from `to_hide`) put as `HIDDEN` wherever it is.
pub fn answer(ran: &Ran, r: &Received, tests: &[Outcome], hidden: &[String], max_body: usize) -> Value {
    let headers: Vec<Value> = r
        .headers
        .iter()
        .map(|(k, v)| {
            let v = if k.eq_ignore_ascii_case("set-cookie") { cookie(v) } else { redact(v, hidden) };
            json!([k, v])
        })
        .collect();
    let (body, truncated) = body(r, max_body.clamp(1, MAX_BODY));
    let mut v = json!({
        "request": ran.label,
        "file": ran.file,
        "method": ran.method,
        "environment": ran.env,
        "url": redact(&r.url, hidden),
        "status": r.status,
        "reason": reason(r.status),
        "time_ms": r.timing.total / 1000,
        "headers": headers,
        "body_bytes": r.body.len(),
        "body": redact(&body, hidden),
        "truncated": truncated,
    });
    if !tests.is_empty() {
        v["tests"] = tests.iter().map(|t| json!({ "test": t.test, "passed": t.passed, "found": redact(&t.detail, hidden) })).collect();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, HttpFile)> {
        vec![
            ("api/users.http".into(), HttpFile::parse("@base = https://x.test\n\n### Who am I\n# @name me\nGET {{base}}/me?key={{apiKey}}\n\n### List users\nGET {{base}}/users\n\n###\nDELETE {{base}}/users/1\n")),
            ("admin.http".into(), HttpFile::parse("# @name me\nGET https://admin.test/me\n")),
        ]
    }

    #[test]
    fn requests_are_listed_as_written() {
        let v = requests(&files(), None).unwrap();
        let rs = v["requests"].as_array().unwrap();
        assert_eq!(rs.len(), 4);
        assert_eq!(rs[0], json!({ "file": "api/users.http", "name": "me", "method": "GET", "url": "{{base}}/me?key={{apiKey}}", "line": 5 }), "variables as they are: no value of theirs");
        assert_eq!(rs[1]["name"], "List users");
        assert_eq!(rs[2]["name"], "DELETE {{base}}/users/1", "no name: its method and URL");
        let one = requests(&files(), Some("/project/admin.http")).unwrap();
        assert_eq!(one["requests"].as_array().unwrap().len(), 1);
        assert!(requests(&files(), Some("nope.http")).unwrap_err().contains("no .http file nope.http"));
        assert_eq!(rel_path("./api\\users.http"), "api/users.http");
    }

    #[test]
    fn a_long_list_says_theres_more() {
        let text: String = (0..MAX_LISTED + 3).map(|i| format!("###\nGET https://x.test/{i}\n")).collect();
        let v = requests(&[("many.http".into(), HttpFile::parse(&text))], None).unwrap();
        assert_eq!(v["requests"].as_array().unwrap().len(), MAX_LISTED);
        assert!(v["more"].as_str().unwrap().starts_with("3 more"));
    }

    #[test]
    fn a_request_is_found_by_its_name() {
        let f = files();
        assert_eq!(find(&f, "List users", None).unwrap(), ("api/users.http".into(), 1, "List users".into()));
        assert_eq!(find(&f, "list USERS", None).unwrap().1, 1, "any case, when only one has it");
        assert_eq!(find(&f, "DELETE {{base}}/users/1", None).unwrap().1, 2, "by its label, as listed");
        let twice = find(&f, "me", None).unwrap_err();
        assert!(twice.contains("2 requests are called \"me\"") && twice.contains("api/users.http:5") && twice.contains("admin.http:2"), "{twice}");
        assert_eq!(find(&f, "me", Some("admin.http")).unwrap(), ("admin.http".into(), 0, "me".into()));
        assert!(find(&f, "https://evil.test/", None).unwrap_err().contains("no request called"), "a URL of its own is no request");
        assert!(find(&f, "  ", None).is_err());
    }

    fn received(body: &[u8]) -> Received {
        Received {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into()), ("Set-Cookie".into(), "session=abc123; Path=/; HttpOnly".into()), ("X-Echo".into(), "s3cr3t-token".into())],
            body: body.to_vec(),
            url: "https://x.test/me?key=s3cr3t-token".into(),
            timing: crate::model::Timing { total: 42_000, ..Default::default() },
        }
    }

    #[test]
    fn secrets_are_hidden_wherever_they_are() {
        let hidden = to_hide(&["s3cr3t-token".into(), "a b&c".into(), "1".into()]);
        assert!(hidden.contains(&"a%20b%26c".to_string()) && hidden.contains(&"a+b%26c".to_string()), "as a URL has it too: {hidden:?}");
        assert!(!hidden.contains(&"1".to_string()), "too short to hide everywhere");
        let r = received(br#"{"echo":"Bearer s3cr3t-token","q":"a%20b%26c"}"#);
        let tests = [Outcome { test: "$.echo == x".into(), passed: false, detail: "Bearer s3cr3t-token".into() }];
        let v = answer(&Ran { label: "me", file: "api/users.http", method: "GET", env: Some("dev") }, &r, &tests, &hidden, DEFAULT_BODY);
        let all = v.to_string();
        assert!(!all.contains("s3cr3t") && !all.contains("abc123") && !all.contains("a%20b"), "{all}");
        assert_eq!(v["url"], "https://x.test/me?key=«hidden»");
        assert_eq!(v["headers"][1], json!(["Set-Cookie", "session=«hidden»; Path=/; HttpOnly"]), "a cookie's name and attributes, not its value");
        assert_eq!(v["body"], r#"{"echo":"Bearer «hidden»","q":"«hidden»"}"#);
        assert_eq!(v["tests"][0]["found"], "Bearer «hidden»");
        assert_eq!((v["status"].clone(), v["reason"].clone(), v["time_ms"].clone(), v["environment"].clone()), (json!(200), json!("OK"), json!(42), json!("dev")));
    }

    const A: Ran = Ran { label: "me", file: "a.http", method: "GET", env: None };

    #[test]
    fn a_long_body_is_cut_and_says_so() {
        let r = received(&b"x".repeat(50));
        let v = answer(&A, &r, &[], &[], 10);
        assert_eq!(v["body"], format!("xxxxxxxxxx\n[… cut: 10 of 50 characters shown; max_body answers more, up to {MAX_BODY}]"));
        assert_eq!((v["truncated"].clone(), v["body_bytes"].clone()), (json!(true), json!(50)));
        assert!(v.get("tests").is_none());
        let whole = answer(&A, &r, &[], &[], 50);
        assert_eq!(whole["truncated"], json!(false));
        let huge = answer(&A, &received(&b"y".repeat(MAX_BODY + 5)), &[], &[], usize::MAX);
        assert!(huge["body"].as_str().unwrap().contains(&format!("cut: {MAX_BODY} of")), "never more than the most");
        let mut png = received(&[0x89, b'P', b'N', b'G', 0xff, 0xfe]);
        png.headers[0].1 = "image/png".into();
        assert_eq!(body(&png, 10).0, "[6 bytes of image/png, not text]");
        // Cut by characters, never inside one.
        assert_eq!(body(&received("ééé".as_bytes()), 2).0.lines().next(), Some("éé"));
    }
}
