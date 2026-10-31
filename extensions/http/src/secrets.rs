//! Secrets: each kept in the system's keychain on its own,
//! `http.secret.<name>` (`dev.name` environment dev's alone), and OAuth
//! tokens as `http.token.<hash>`. A keychain can't be listed, so the
//! names it holds -- never their values -- are kept beside, for
//! completion. The older `http.secrets` setting (`name=value; ...`) is
//! still read, after the keychain, and moved into it.

use sha2::{Digest, Sha256};

/// The secret `name` is kept as, or why it can't be one.
pub fn key(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.contains(|c: char| c.is_whitespace() || matches!(c, '=' | ';' | '/' | '\\' | '*' | '?' | '{' | '}')) {
        return Err(format!("{name:?}: a secret's name is a word, with no spaces, =, ;, /, *, ? or braces (dev.token is dev's alone)"));
    }
    Ok(format!("http.secret.{name}"))
}

/// Secret `name`'s value: environment `env`'s own (`env.name`), then the
/// one for every environment, from `get` (the keychain) and then from
/// the older pairs.
pub fn lookup(name: &str, env: Option<&str>, get: &dyn Fn(&str) -> Option<String>, legacy: &[(String, String)]) -> Option<String> {
    let scoped = env.map(|e| format!("{e}.{name}"));
    let names: Vec<&str> = scoped.as_deref().into_iter().chain([name]).collect();
    for n in &names {
        if let Some(v) = key(n).ok().and_then(|k| get(&k)) {
            return Some(v);
        }
    }
    names.iter().find_map(|n| legacy.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()))
}

/// Where the OAuth token `key` (`env/id`) of the project kept in `dir` is
/// in the keychain: a hash, so neither shows there.
pub fn token_key(dir: &str, key: &str) -> String {
    let h = Sha256::digest(format!("{dir}\n{key}").as_bytes());
    let hex: String = h.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("http.token.{hex}")
}

/// The names kept, from their file; none when there's none.
pub fn load_names(path: &str) -> Vec<String> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_names(path: &str, names: &[String]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(names).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
}

/// `names` with `name` in it, or not, sorted.
pub fn with_name(names: &[String], name: &str, kept: bool) -> Vec<String> {
    let mut out: Vec<String> = names.iter().filter(|n| *n != name).cloned().collect();
    if kept {
        out.push(name.to_string());
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn names_become_keys() {
        assert_eq!(key("token").unwrap(), "http.secret.token");
        assert_eq!(key(" dev.password ").unwrap(), "http.secret.dev.password");
        for bad in ["", "a b", "a=b", "a;b", "a/b", "a*", "{{x}}"] {
            assert!(key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_environments_own_first_then_everyones_then_the_older_setting() {
        let kept: HashMap<String, String> = [("http.secret.token", "all"), ("http.secret.dev.token", "dev's"), ("http.secret.key", "k")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let get = |k: &str| kept.get(k).cloned();
        let legacy = vec![("old".to_string(), "o".to_string()), ("prod.old".to_string(), "prod's o".to_string()), ("key".to_string(), "not this".to_string())];
        assert_eq!(lookup("token", Some("dev"), &get, &legacy).as_deref(), Some("dev's"));
        assert_eq!(lookup("token", Some("prod"), &get, &legacy).as_deref(), Some("all"));
        assert_eq!(lookup("token", None, &get, &legacy).as_deref(), Some("all"));
        assert_eq!(lookup("key", None, &get, &legacy).as_deref(), Some("k"), "the keychain over the setting");
        assert_eq!(lookup("old", Some("prod"), &get, &legacy).as_deref(), Some("prod's o"));
        assert_eq!(lookup("old", Some("dev"), &get, &legacy).as_deref(), Some("o"));
        assert_eq!(lookup("nope", None, &get, &legacy), None);
        assert_eq!(lookup("a b", None, &get, &legacy), None);
    }

    #[test]
    fn tokens_and_names() {
        let a = token_key("/data/projects/app-1", "dev/api");
        assert!(a.starts_with("http.token.") && a.len() == "http.token.".len() + 24);
        assert_ne!(a, token_key("/data/projects/app-2", "dev/api"));
        assert_ne!(a, token_key("/data/projects/app-1", "prod/api"));
        assert_eq!(a, token_key("/data/projects/app-1", "dev/api"));
        let names = with_name(&[], "b", true);
        let names = with_name(&names, "a", true);
        assert_eq!(names, ["a", "b"]);
        assert_eq!(with_name(&names, "a", false), ["b"]);
        let tmp = std::env::temp_dir().join(format!("fenix-http-names-{}.json", std::process::id()));
        let path = tmp.to_string_lossy().into_owned();
        save_names(&path, &names).unwrap();
        assert_eq!(load_names(&path), names);
        std::fs::remove_file(&tmp).ok();
        assert!(load_names("/nowhere/names.json").is_empty());
    }
}
