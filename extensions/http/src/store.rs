//! What it keeps for a project, in its own folder (`/data/projects/<name>-
//! <hash>/`), outside the project so Git never sees it: the environment
//! you chose, the history of what was sent, the cookies of each
//! environment, and which OAuth tokens it has: those are kept in the
//! system's keychain (`secrets::token_key`), not here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::auth::Token;
use crate::cookies::Jar;
use crate::model::Exchange;

/// The most a body is kept in history; past it, it's cut.
pub const KEEP_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    /// The environment chosen, by name.
    #[serde(default)]
    pub env: Option<String>,
    /// Newest first.
    #[serde(default)]
    pub history: Vec<Exchange>,
    /// By environment ("" for none).
    #[serde(default)]
    pub cookies: BTreeMap<String, Jar>,
    /// By environment and configuration: `dev/auth-id`. Written here only
    /// when the keychain wouldn't take them; an older file's are moved.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, Token>,
    /// The tokens kept in the keychain, by the same keys.
    #[serde(default)]
    pub kept_tokens: Vec<String>,
}

/// The folder a project's things go in: its name and a hash of where it
/// is, so two projects of one name don't share one.
pub fn dir(root: &str) -> String {
    let name = root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or("project");
    let hash = root.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    format!("/data/projects/{name}-{:08x}", hash as u32)
}

pub fn token_key(env: Option<&str>, id: &str) -> String {
    format!("{}/{id}", env.unwrap_or(""))
}

impl Store {
    pub fn load(dir: &str) -> Store {
        std::fs::read_to_string(format!("{dir}/store.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &str) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let tmp = format!("{dir}/store.json.new");
        std::fs::write(&tmp, text).map_err(|e| format!("{tmp}: {e}"))?;
        std::fs::rename(&tmp, format!("{dir}/store.json")).map_err(|e| e.to_string())
    }

    /// `e` added to the history, its bodies cut to what's kept, and the
    /// oldest gone past `most`.
    pub fn remember(&mut self, mut e: Exchange, most: usize) {
        e.sent.body.truncate(KEEP_BODY);
        if let Ok(r) = &mut e.answer {
            r.body.truncate(KEEP_BODY);
        }
        self.history.insert(0, e);
        self.history.truncate(most);
    }

    pub fn jar(&mut self, env: Option<&str>) -> &mut Jar {
        self.cookies.entry(env.unwrap_or("").to_string()).or_default()
    }
}

/// The history entries `query` finds -- every word of it in the method,
/// the URL, the status, the name or the file -- by index.
pub fn search(history: &[Exchange], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    history
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let status = match &e.answer {
                Ok(r) => r.status.to_string(),
                Err(_) => "error".into(),
            };
            let hay = format!("{} {} {status} {} {}", e.sent.method, e.sent.url, e.name.as_deref().unwrap_or(""), e.file.as_deref().unwrap_or("")).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Received, Sent};

    fn e(method: &str, url: &str, status: u16) -> Exchange {
        Exchange { sent: Sent { method: method.into(), url: url.into(), ..Sent::default() }, answer: Ok(Received { status, body: vec![b'x'; KEEP_BODY + 5], ..Received::default() }), at: 0, file: Some("api/users.http".into()), name: None, env: None }
    }

    #[test]
    fn history_is_kept_newest_first_and_searched() {
        let mut s = Store::default();
        s.remember(e("GET", "https://x.test/users", 200), 2);
        s.remember(e("POST", "https://x.test/users", 201), 2);
        s.remember(e("DELETE", "https://x.test/users/1", 404), 2);
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.history[0].sent.method, "DELETE");
        assert_eq!(s.history[1].answer.as_ref().unwrap().body.len(), KEEP_BODY);
        assert_eq!(search(&s.history, "users"), [0, 1]);
        assert_eq!(search(&s.history, "post 201"), [1]);
        assert_eq!(search(&s.history, "404 users/1"), [0]);
        assert_eq!(search(&s.history, "api/users.http"), [0, 1]);
        assert!(search(&s.history, "nothing").is_empty());
    }

    #[test]
    fn it_goes_to_disk_and_back() {
        let tmp = std::env::temp_dir().join(format!("fenix-http-{}", std::process::id()));
        let dir = tmp.to_string_lossy().into_owned();
        let mut s = Store { env: Some("dev".into()), ..Store::default() };
        s.remember(e("GET", "https://x.test", 200), 10);
        s.jar(Some("dev")).store("https://x.test/", ["a=1"], 0);
        s.tokens.insert(token_key(Some("dev"), "api"), Token { access: "t".into(), refresh: None, id_token: None, expires: Some(5) });
        s.save(&dir).unwrap();
        assert_eq!(Store::load(&dir), s);
        assert_eq!(Store::load("/nowhere"), Store::default());
        std::fs::remove_dir_all(&tmp).ok();
        assert!(super::dir("/home/me/app").starts_with("/data/projects/app-"));
        assert_ne!(super::dir("/home/me/app"), super::dir("/work/app"));
    }
}
