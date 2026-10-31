//! A request as code: a `curl` command, Python's requests, JavaScript's
//! fetch, Go's net/http, Rust's reqwest. It's the request as sent, its
//! variables filled in -- secrets too, so mind where it goes.

use crate::model::Sent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Curl,
    Python,
    JavaScript,
    Go,
    Rust,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::Curl, Lang::Python, Lang::JavaScript, Lang::Go, Lang::Rust];

    pub fn name(self) -> &'static str {
        match self {
            Lang::Curl => "curl",
            Lang::Python => "Python (requests)",
            Lang::JavaScript => "JavaScript (fetch)",
            Lang::Go => "Go (net/http)",
            Lang::Rust => "Rust (reqwest)",
        }
    }

    /// Fenix's name for its highlighting.
    pub fn language(self) -> &'static str {
        match self {
            Lang::Curl => "bash",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::Go => "go",
            Lang::Rust => "rust",
        }
    }

    pub fn by_name(name: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.name() == name || l.language() == name)
    }
}

pub fn export(lang: Lang, s: &Sent) -> String {
    match lang {
        Lang::Curl => curl(s),
        Lang::Python => python(s),
        Lang::JavaScript => javascript(s),
        Lang::Go => go(s),
        Lang::Rust => rust(s),
    }
}

/// `s` in single quotes, for a POSIX shell.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn text_body(s: &Sent) -> Option<Result<&str, usize>> {
    (!s.body.is_empty()).then(|| std::str::from_utf8(&s.body).map_err(|_| s.body.len()))
}

pub fn curl(s: &Sent) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut first = String::from("curl");
    if s.method == "HEAD" {
        first.push_str(" --head");
    } else if s.method != "GET" || !s.body.is_empty() {
        first.push_str(&format!(" -X {}", s.method));
    }
    first.push(' ');
    first.push_str(&shell_quote(&s.url));
    parts.push(first);
    for (k, v) in &s.headers {
        parts.push(format!("-H {}", shell_quote(&format!("{k}: {v}"))));
    }
    match text_body(s) {
        Some(Ok(t)) => parts.push(format!("--data-raw {}", shell_quote(t))),
        Some(Err(n)) => parts.push(format!("--data-binary @body.bin  # the body: {n} bytes, not text")),
        None => {}
    }
    parts.join(" \\\n  ")
}

/// A string as a double-quoted literal Python, JavaScript and Go all read.
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

pub fn python(s: &Sent) -> String {
    let mut out = String::from("import requests\n\nresponse = requests.request(\n");
    out.push_str(&format!("    {},\n    {},\n", quoted(&s.method), quoted(&s.url)));
    if !s.headers.is_empty() {
        out.push_str("    headers={\n");
        for (k, v) in &s.headers {
            out.push_str(&format!("        {}: {},\n", quoted(k), quoted(v)));
        }
        out.push_str("    },\n");
    }
    match text_body(s) {
        Some(Ok(t)) => out.push_str(&format!("    data={}.encode(),\n", quoted(t))),
        Some(Err(n)) => out.push_str(&format!("    data=open(\"body.bin\", \"rb\"),  # {n} bytes, not text\n")),
        None => {}
    }
    out.push_str(")\nprint(response.status_code)\nprint(response.text)\n");
    out
}

pub fn javascript(s: &Sent) -> String {
    let mut out = format!("const response = await fetch({}, {{\n  method: {},\n", quoted(&s.url), quoted(&s.method));
    if !s.headers.is_empty() {
        out.push_str("  headers: {\n");
        for (k, v) in &s.headers {
            out.push_str(&format!("    {}: {},\n", quoted(k), quoted(v)));
        }
        out.push_str("  },\n");
    }
    match text_body(s) {
        Some(Ok(t)) => out.push_str(&format!("  body: {},\n", quoted(t))),
        Some(Err(n)) => out.push_str(&format!("  body: await (await fetch(\"body.bin\")).blob(), // {n} bytes, not text\n")),
        None => {}
    }
    out.push_str("});\nconsole.log(response.status);\nconsole.log(await response.text());\n");
    out
}

pub fn go(s: &Sent) -> String {
    let body = text_body(s);
    let mut imports = vec!["\"fmt\"", "\"io\"", "\"net/http\""];
    if matches!(body, Some(Ok(_))) {
        imports.push("\"strings\"");
    }
    if matches!(body, Some(Err(_))) {
        imports.push("\"os\"");
    }
    let mut out = format!("package main\n\nimport (\n{}\n)\n\nfunc main() {{\n", imports.iter().map(|i| format!("\t{i}")).collect::<Vec<_>>().join("\n"));
    let reader = match body {
        Some(Ok(t)) => {
            out.push_str(&format!("\tbody := strings.NewReader({})\n", quoted(t)));
            "body"
        }
        Some(Err(_)) => {
            out.push_str("\tbody, err := os.Open(\"body.bin\")\n\tif err != nil {\n\t\tpanic(err)\n\t}\n");
            "body"
        }
        None => "nil",
    };
    out.push_str(&format!("\treq, err := http.NewRequest({}, {}, {reader})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n", quoted(&s.method), quoted(&s.url)));
    for (k, v) in &s.headers {
        out.push_str(&format!("\treq.Header.Add({}, {})\n", quoted(k), quoted(v)));
    }
    out.push_str("\tres, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer res.Body.Close()\n\tout, _ := io.ReadAll(res.Body)\n\tfmt.Println(res.Status)\n\tfmt.Println(string(out))\n}\n");
    out
}

pub fn rust(s: &Sent) -> String {
    let mut out = String::from("// reqwest = { version = \"0.12\", features = [\"blocking\"] }\nfn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = reqwest::blocking::Client::new();\n    let response = client\n");
    out.push_str(&format!("        .request(reqwest::Method::from_bytes({:?}.as_bytes())?, {:?})\n", s.method, s.url));
    for (k, v) in &s.headers {
        out.push_str(&format!("        .header({k:?}, {v:?})\n"));
    }
    match text_body(s) {
        Some(Ok(t)) => out.push_str(&format!("        .body({t:?})\n")),
        Some(Err(n)) => out.push_str(&format!("        .body(std::fs::read(\"body.bin\")?) // {n} bytes, not text\n")),
        None => {}
    }
    out.push_str("        .send()?;\n    println!(\"{}\", response.status());\n    println!(\"{}\", response.text()?);\n    Ok(())\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent() -> Sent {
        Sent { method: "POST".into(), url: "https://api.example.com/users?x=1".into(), headers: vec![("Content-Type".into(), "application/json".into()), ("X-Note".into(), "it's".into())], body: br#"{"name":"ada"}"#.to_vec() }
    }

    #[test]
    fn curl_is_quoted_for_the_shell() {
        assert_eq!(curl(&sent()), "curl -X POST 'https://api.example.com/users?x=1' \\\n  -H 'Content-Type: application/json' \\\n  -H 'X-Note: it'\\''s' \\\n  --data-raw '{\"name\":\"ada\"}'");
        let get = Sent { method: "GET".into(), url: "https://x.test/".into(), ..Sent::default() };
        assert_eq!(curl(&get), "curl https://x.test/");
        let head = Sent { method: "HEAD".into(), ..get.clone() };
        assert_eq!(curl(&head), "curl --head https://x.test/");
        let bin = Sent { body: vec![0xff, 0xfe], ..get };
        assert!(curl(&bin).contains("--data-binary @body.bin"));
    }

    #[test]
    fn code_in_each_language() {
        let s = sent();
        let py = python(&s);
        assert!(py.contains("    \"POST\",\n    \"https://api.example.com/users?x=1\",\n"), "{py}");
        assert!(py.contains("        \"X-Note\": \"it's\",\n"));
        assert!(py.contains("data=\"{\\\"name\\\":\\\"ada\\\"}\".encode()"));
        let js = javascript(&s);
        assert!(js.starts_with("const response = await fetch(\"https://api.example.com/users?x=1\", {\n  method: \"POST\",\n"), "{js}");
        assert!(js.contains("  body: \"{\\\"name\\\":\\\"ada\\\"}\",\n"));
        let go = go(&s);
        assert!(go.contains("\t\"strings\"") && go.contains("req.Header.Add(\"Content-Type\", \"application/json\")"), "{go}");
        assert!(go.contains("http.NewRequest(\"POST\", \"https://api.example.com/users?x=1\", body)"));
        let rs = rust(&s);
        assert!(rs.contains(".header(\"X-Note\", \"it's\")") && rs.contains(".body(\"{\\\"name\\\":\\\"ada\\\"}\")"), "{rs}");
        let plain = Sent { method: "GET".into(), url: "https://x.test".into(), ..Sent::default() };
        assert!(go_has_no_strings(&plain));
        assert_eq!(Lang::by_name("python"), Some(Lang::Python));
    }

    fn go_has_no_strings(s: &Sent) -> bool {
        let g = go(s);
        !g.contains("strings") && g.contains(", nil)")
    }
}
