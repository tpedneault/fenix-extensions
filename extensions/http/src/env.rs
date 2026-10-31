//! Environments: `http-client.env.json` beside the `.http` files (or in a
//! folder above them, up to the project), and `http-client.private.env.json`
//! beside it for what isn't committed -- JetBrains' files, which REST
//! Client's `rest-client.environmentVariables` setting is the same shape
//! as. Each is an object of environments, each an object of variables;
//! `$shared` is every environment's. A private value wins over the
//! public one, an environment's over `$shared`'s.
//!
//! JetBrains keeps more than variables there: `Security.Auth` (the OAuth
//! configurations, [`crate::auth`]) and `SSLConfiguration` (a client
//! certificate, whether to check the server's).

use serde_json::{Map, Value};

pub const PUBLIC: &str = "http-client.env.json";
pub const PRIVATE: &str = "http-client.private.env.json";
pub const SHARED: &str = "$shared";

/// Keys of an environment that aren't variables.
const NOT_VARIABLES: &[&str] = &["Security", "SSLConfiguration"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Environments {
    public: Map<String, Value>,
    private: Map<String, Value>,
}

/// Where a variable's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Public,
    Private,
}

/// A client certificate and whether to check the server's, from
/// `SSLConfiguration`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ssl {
    /// Paths, as the file gives them (relative to its folder).
    pub cert: Option<String>,
    pub key: Option<String>,
    pub verify: bool,
}

fn object(text: Option<&str>, file: &str) -> Result<Map<String, Value>, String> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else { return Ok(Map::new()) };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{file}: not an object of environments")),
        Err(e) => Err(format!("{file}: {e}")),
    }
}

/// A variable's value as text: a string as it is, anything else as JSON.
pub fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl Environments {
    pub fn parse(public: Option<&str>, private: Option<&str>) -> Result<Environments, String> {
        Ok(Environments { public: object(public, PUBLIC)?, private: object(private, PRIVATE)? })
    }

    /// The environments, in the order the files give them, `$shared` not
    /// one.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for name in self.public.keys().chain(self.private.keys()) {
            if name != SHARED && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn env<'a>(map: &'a Map<String, Value>, env: &str) -> Option<&'a Map<String, Value>> {
        map.get(env).and_then(Value::as_object)
    }

    /// Variable `name` in environment `env` (none: `$shared` only).
    pub fn var(&self, env: Option<&str>, name: &str) -> Option<(String, Source)> {
        if NOT_VARIABLES.contains(&name) {
            return None;
        }
        let envs = env.into_iter().chain(std::iter::once(SHARED));
        for e in envs {
            for (map, source) in [(&self.private, Source::Private), (&self.public, Source::Public)] {
                if let Some(v) = Environments::env(map, e).and_then(|m| m.get(name)) {
                    return Some((as_text(v), source));
                }
            }
        }
        None
    }

    /// Every variable `env` has, `$shared`'s with them: name, value, where
    /// from.
    pub fn vars(&self, env: Option<&str>) -> Vec<(String, String, Source)> {
        let mut names: Vec<String> = Vec::new();
        for e in env.into_iter().chain(std::iter::once(SHARED)) {
            for map in [&self.public, &self.private] {
                for key in Environments::env(map, e).map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
                    if !NOT_VARIABLES.contains(&key.as_str()) && !names.contains(&key) {
                        names.push(key);
                    }
                }
            }
        }
        names.into_iter().filter_map(|n| self.var(env, &n).map(|(v, s)| (n, v, s))).collect()
    }

    /// `Security.Auth.<id>`, the private file's over the public's, each
    /// key on its own: a client secret kept private, the rest committed.
    pub fn auth(&self, env: Option<&str>, id: &str) -> Option<Map<String, Value>> {
        let mut out: Option<Map<String, Value>> = None;
        for e in std::iter::once(SHARED).chain(env) {
            for map in [&self.public, &self.private] {
                let found = Environments::env(map, e).and_then(|m| m.get("Security")).and_then(|s| s.get("Auth")).and_then(|a| a.get(id)).and_then(Value::as_object);
                if let Some(found) = found {
                    let into = out.get_or_insert_with(Map::new);
                    for (k, v) in found {
                        into.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        out
    }

    /// The OAuth configurations `env` has, by id.
    pub fn auth_ids(&self, env: Option<&str>) -> Vec<String> {
        let mut ids = Vec::new();
        for e in std::iter::once(SHARED).chain(env) {
            for map in [&self.public, &self.private] {
                if let Some(auth) = Environments::env(map, e).and_then(|m| m.get("Security")).and_then(|s| s.get("Auth")).and_then(Value::as_object) {
                    for id in auth.keys() {
                        if !ids.contains(id) {
                            ids.push(id.clone());
                        }
                    }
                }
            }
        }
        ids
    }

    /// `SSLConfiguration`: a client certificate, whether to check the
    /// server's (`verifyHostCertificate`, true unless it says).
    pub fn ssl(&self, env: Option<&str>) -> Ssl {
        let mut ssl = Ssl { verify: true, ..Ssl::default() };
        for e in std::iter::once(SHARED).chain(env) {
            for map in [&self.public, &self.private] {
                let Some(c) = Environments::env(map, e).and_then(|m| m.get("SSLConfiguration")) else { continue };
                let path = |v: Option<&Value>| match v {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Object(o)) => o.get("path").and_then(Value::as_str).map(String::from),
                    _ => None,
                };
                if let Some(p) = path(c.get("clientCertificate")) {
                    ssl.cert = Some(p);
                }
                if let Some(p) = path(c.get("clientCertificateKey")) {
                    ssl.key = Some(p);
                }
                if let Some(v) = c.get("verifyHostCertificate").and_then(Value::as_bool) {
                    ssl.verify = v;
                }
            }
        }
        ssl
    }
}

/// The folders to look for env files in, for a `.http` file at `rel`
/// (below the project): its own, then each above it, up to the project's.
pub fn folders(rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut dir = rel.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    loop {
        out.push(dir.clone());
        if dir.is_empty() {
            break;
        }
        dir = dir.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC_FILE: &str = r#"{
      "$shared": { "version": "v1", "timeout": 30 },
      "dev": {
        "host": "http://localhost:8080",
        "Security": { "Auth": { "auth-id": { "Type": "OAuth2", "Grant Type": "Client Credentials", "Client ID": "app" } } },
        "SSLConfiguration": { "clientCertificate": "certs/me.pem", "verifyHostCertificate": false }
      },
      "prod": { "host": "https://api.example.com", "version": "v2" }
    }"#;
    const PRIVATE_FILE: &str = r#"{
      "dev": { "password": "hunter2", "Security": { "Auth": { "auth-id": { "Client Secret": "s3cret" } } } },
      "staging": { "host": "https://staging.example.com" }
    }"#;

    #[test]
    fn environments_merge_private_over_public_and_over_shared() {
        let e = Environments::parse(Some(PUBLIC_FILE), Some(PRIVATE_FILE)).unwrap();
        assert_eq!(e.names(), ["dev", "prod", "staging"]);
        assert_eq!(e.var(Some("dev"), "host"), Some(("http://localhost:8080".into(), Source::Public)));
        assert_eq!(e.var(Some("dev"), "password"), Some(("hunter2".into(), Source::Private)));
        assert_eq!(e.var(Some("dev"), "version").map(|v| v.0), Some("v1".into()), "$shared's");
        assert_eq!(e.var(Some("prod"), "version").map(|v| v.0), Some("v2".into()), "its own over $shared's");
        assert_eq!(e.var(Some("dev"), "timeout").map(|v| v.0), Some("30".into()), "a number as text");
        assert_eq!(e.var(None, "host"), None);
        assert_eq!(e.var(Some("dev"), "Security"), None, "not a variable");
        let names: Vec<String> = e.vars(Some("dev")).into_iter().map(|v| v.0).collect();
        assert_eq!(names, ["host", "password", "version", "timeout"]);
    }

    #[test]
    fn auth_and_ssl_come_from_both_files() {
        let e = Environments::parse(Some(PUBLIC_FILE), Some(PRIVATE_FILE)).unwrap();
        let auth = e.auth(Some("dev"), "auth-id").unwrap();
        assert_eq!(auth.get("Client ID").and_then(Value::as_str), Some("app"));
        assert_eq!(auth.get("Client Secret").and_then(Value::as_str), Some("s3cret"), "the private file's key with the rest");
        assert_eq!(e.auth_ids(Some("dev")), ["auth-id"]);
        assert!(e.auth(Some("prod"), "auth-id").is_none());
        let ssl = e.ssl(Some("dev"));
        assert_eq!((ssl.cert.as_deref(), ssl.verify), (Some("certs/me.pem"), false));
        assert!(e.ssl(Some("prod")).verify);
    }

    #[test]
    fn bad_files_say_which() {
        assert!(Environments::parse(Some("[1]"), None).unwrap_err().contains(PUBLIC));
        assert!(Environments::parse(None, Some("{")).unwrap_err().contains(PRIVATE));
        assert!(Environments::parse(None, None).unwrap().names().is_empty());
    }

    #[test]
    fn env_files_are_looked_for_up_to_the_project() {
        assert_eq!(folders("api/users/list.http"), ["api/users", "api", ""]);
        assert_eq!(folders("top.http"), [""]);
    }
}
