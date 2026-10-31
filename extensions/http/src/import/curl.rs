//! A `curl` command as a request: what Chrome's "Copy as cURL", an API's
//! docs and [`crate::export::curl`] write. Its words split as a POSIX
//! shell splits them (quotes, `$'...'`, backslashes, lines carried on);
//! the options a request is made of read, the rest passed over.

use super::Draft;

/// A shell command's words.
pub fn words(cmd: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(n) => {
                    cur.push(n);
                    has = true;
                }
                None => {}
            },
            // Windows' cmd carries a line on with ^.
            '^' if matches!(chars.peek(), Some('\n' | '\r')) => {
                while matches!(chars.peek(), Some('\n' | '\r')) {
                    chars.next();
                }
            }
            '\'' => {
                has = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("a ' isn't closed".into()),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                has = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some('r') => cur.push('\r'),
                            Some(c) => cur.push(c),
                            None => return Err("a $' isn't closed".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("a $' isn't closed".into()),
                    }
                }
            }
            '"' => {
                has = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => return Err("a \" isn't closed".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("a \" isn't closed".into()),
                    }
                }
            }
            c if c.is_whitespace() => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    Ok(out)
}

/// Options that take a value, and so aren't the URL.
const TAKES_VALUE: &[&str] = &["-o", "--output", "-w", "--write-out", "--connect-timeout", "-m", "--max-time", "--retry", "-x", "--proxy", "--cacert", "--cert", "--key", "-c", "--cookie-jar", "--resolve", "-T", "--upload-file", "-r", "--range", "--limit-rate"];

/// The request a `curl` command makes.
pub fn parse(cmd: &str) -> Result<Draft, String> {
    let words = words(cmd.trim())?;
    let mut it = words.into_iter().peekable();
    match it.next() {
        Some(w) if w == "curl" || w.ends_with("/curl") || w.eq_ignore_ascii_case("curl.exe") => {}
        _ => return Err("not a curl command".into()),
    }
    let mut d = Draft::default();
    let mut method: Option<String> = None;
    let mut data: Vec<String> = Vec::new();
    let mut form: Vec<(String, String)> = Vec::new();
    let mut get = false;
    let mut json = false;
    let mut url: Option<String> = None;
    while let Some(w) = it.next() {
        // `-XPOST`, `-HAccept: x`, `--data=x`.
        let (flag, inline) = if let Some(rest) = w.strip_prefix("--") {
            match rest.split_once('=') {
                Some((f, v)) => (format!("--{f}"), Some(v.to_string())),
                None => (w.clone(), None),
            }
        } else if w.starts_with('-') && w.len() > 2 && !w.starts_with("--") && "XHdbuAeF".contains(&w[1..2]) {
            (w[..2].to_string(), Some(w[2..].to_string()))
        } else {
            (w.clone(), None)
        };
        let value = |it: &mut std::iter::Peekable<std::vec::IntoIter<String>>| inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{flag} wants a value"));
        match flag.as_str() {
            "-X" | "--request" => method = Some(value(&mut it)?.to_ascii_uppercase()),
            "-H" | "--header" => {
                let h = value(&mut it)?;
                if let Some((k, v)) = h.split_once(':') {
                    d.headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            "-d" | "--data" | "--data-ascii" | "--data-raw" | "--data-binary" => {
                let v = value(&mut it)?;
                data.push(match v.strip_prefix('@').filter(|_| flag != "--data-raw") {
                    Some(file) => format!("< {file}"),
                    None => v,
                });
            }
            "--data-urlencode" => {
                let v = value(&mut it)?;
                data.push(match v.split_once('=') {
                    Some((k, val)) => format!("{k}={}", crate::url::encode(val, true)),
                    None => crate::url::encode(&v, true),
                });
            }
            "--json" => {
                data.push(value(&mut it)?);
                json = true;
            }
            "-F" | "--form" | "--form-string" => {
                let v = value(&mut it)?;
                if let Some((k, val)) = v.split_once('=') {
                    form.push((k.to_string(), val.to_string()));
                }
            }
            "-u" | "--user" => d.headers.push(("Authorization".into(), format!("Basic {}", value(&mut it)?))),
            "-A" | "--user-agent" => d.headers.push(("User-Agent".into(), value(&mut it)?)),
            "-e" | "--referer" => d.headers.push(("Referer".into(), value(&mut it)?)),
            "-b" | "--cookie" => d.headers.push(("Cookie".into(), value(&mut it)?)),
            "--oauth2-bearer" => d.headers.push(("Authorization".into(), format!("Bearer {}", value(&mut it)?))),
            "--url" => url = Some(value(&mut it)?),
            "-G" | "--get" => get = true,
            "-I" | "--head" => method = Some("HEAD".into()),
            "-k" | "--insecure" => d.directives.push(("insecure".into(), String::new())),
            f if TAKES_VALUE.contains(&f) => {
                value(&mut it)?;
            }
            f if f.starts_with('-') => {}
            _ => {
                if url.is_none() {
                    url = Some(w);
                }
            }
        }
    }
    d.url = url.ok_or("no URL in it")?;
    if get && !data.is_empty() {
        let sep = if d.url.contains('?') { '&' } else { '?' };
        d.url = format!("{}{sep}{}", d.url, data.join("&"));
        data.clear();
    }
    if !form.is_empty() {
        let boundary = "----FenixFormBoundary";
        let mut body = String::new();
        for (k, v) in &form {
            body.push_str(&format!("--{boundary}\n"));
            match v.strip_prefix('@') {
                Some(file) => {
                    let (path, ty) = match file.split_once(";type=") {
                        Some((p, t)) => (p, Some(t)),
                        None => (file, None),
                    };
                    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
                    body.push_str(&format!("Content-Disposition: form-data; name=\"{k}\"; filename=\"{name}\"\n"));
                    if let Some(t) = ty {
                        body.push_str(&format!("Content-Type: {t}\n"));
                    }
                    body.push_str(&format!("\n< {path}\n"));
                }
                None => body.push_str(&format!("Content-Disposition: form-data; name=\"{k}\"\n\n{v}\n")),
            }
        }
        body.push_str(&format!("--{boundary}--"));
        if !d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            d.headers.push(("Content-Type".into(), format!("multipart/form-data; boundary={boundary}")));
        }
        d.body = Some(body);
    } else if !data.is_empty() {
        let has_type = d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if json {
            if !has_type {
                d.headers.push(("Content-Type".into(), "application/json".into()));
            }
            if !d.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) {
                d.headers.push(("Accept".into(), "application/json".into()));
            }
        } else if !has_type && !data.iter().any(|x| x.starts_with("< ")) {
            d.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
        }
        d.body = Some(data.join("&"));
    }
    d.method = method.unwrap_or_else(|| if d.body.is_some() { "POST".into() } else { "GET".into() });
    d.name = format!("{} {}", d.method, crate::url::path(&d.url));
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sent;

    #[test]
    fn words_split_as_a_shell_splits_them() {
        assert_eq!(words(r#"curl 'a b' "c \"d\" \$e" $'f\ng' h\ i"#).unwrap(), ["curl", "a b", "c \"d\" $e", "f\ng", "h i"]);
        assert_eq!(words("curl \\\n  -X POST\\\n  x").unwrap(), ["curl", "-X", "POST", "x"]);
        assert_eq!(words("curl '' x").unwrap(), ["curl", "", "x"]);
        assert!(words("curl 'open").is_err());
    }

    #[test]
    fn a_copied_command_is_read() {
        let d = parse(
            r#"curl 'https://api.example.com/v1/users?page=2' \
  -H 'accept: application/json' \
  -H 'authorization: Bearer abc' \
  --data-raw '{"name":"ada"}' \
  --compressed"#,
        )
        .unwrap();
        assert_eq!((d.method.as_str(), d.url.as_str()), ("POST", "https://api.example.com/v1/users?page=2"));
        assert_eq!(d.headers, [("accept".to_string(), "application/json".to_string()), ("authorization".into(), "Bearer abc".into()), ("Content-Type".into(), "application/x-www-form-urlencoded".into())]);
        assert_eq!(d.body.as_deref(), Some(r#"{"name":"ada"}"#));
        let d = parse("curl -XDELETE -u ada:pw https://x.test/a/1 -o out.txt -k").unwrap();
        assert_eq!((d.method.as_str(), d.headers[0].1.as_str()), ("DELETE", "Basic ada:pw"));
        assert_eq!(d.directives, [("insecure".to_string(), String::new())]);
        let d = parse("curl -G https://x.test/search -d q=rust -d page=2").unwrap();
        assert_eq!((d.method.as_str(), d.url.as_str(), d.body.as_deref()), ("GET", "https://x.test/search?q=rust&page=2", None));
        let d = parse("curl --json '{\"a\":1}' https://x.test").unwrap();
        assert!(d.headers.contains(&("Content-Type".into(), "application/json".into())));
        let d = parse("curl --data-binary @photo.jpg -H 'Content-Type: image/jpeg' https://x.test/up").unwrap();
        assert_eq!(d.body.as_deref(), Some("< photo.jpg"));
        assert!(parse("wget x").is_err());
        assert!(parse("curl -v").unwrap_err().contains("no URL"));
    }

    #[test]
    fn a_form_is_multipart() {
        let d = parse("curl -F name=ada -F 'avatar=@me.png;type=image/png' https://x.test/u").unwrap();
        let body = d.body.unwrap();
        assert!(body.contains("name=\"avatar\"; filename=\"me.png\"\nContent-Type: image/png\n\n< me.png\n"), "{body}");
        assert!(d.headers[0].1.starts_with("multipart/form-data; boundary="));
    }

    /// What export writes, import reads back the same.
    #[test]
    fn curl_goes_round() {
        let sent = Sent { method: "PUT".into(), url: "https://x.test/a b?q='1'".into(), headers: vec![("Content-Type".into(), "application/json".into()), ("X-Quote".into(), "it's \"q\"".into())], body: b"{\"a\": \"it's\"}\n".to_vec() };
        let d = parse(&crate::export::curl(&sent)).unwrap();
        assert_eq!(d.method, sent.method);
        assert_eq!(d.url, sent.url);
        assert_eq!(d.headers, sent.headers);
        assert_eq!(d.body.map(String::into_bytes), Some(sent.body));
        let get = Sent { method: "GET".into(), url: "https://x.test/".into(), ..Sent::default() };
        let d = parse(&crate::export::curl(&get)).unwrap();
        assert_eq!((d.method.as_str(), d.url.as_str(), d.headers.len(), d.body), ("GET", "https://x.test/", 0, None));
    }
}
