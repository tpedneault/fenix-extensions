//! `INFO`, what a server says of itself, and `CONNECT`, what a client says
//! back.

use serde::{Deserialize, Serialize};

/// A server's `INFO`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ServerInfo {
    pub server_id: String,
    pub server_name: String,
    pub version: String,
    pub go: String,
    pub host: String,
    pub port: u16,
    pub proto: i32,
    /// Whether it takes headers (`HPUB`, and sends `HMSG`).
    pub headers: bool,
    pub max_payload: u64,
    pub client_id: u64,
    pub client_ip: String,
    pub auth_required: bool,
    pub tls_required: bool,
    pub tls_verify: bool,
    pub tls_available: bool,
    /// What an NKey signs.
    pub nonce: Option<String>,
    pub cluster: Option<String>,
    pub domain: Option<String>,
    pub jetstream: bool,
    /// The cluster's other servers a client may use.
    pub connect_urls: Vec<String>,
    /// In lame-duck mode: it's going away, and its clients should move.
    pub ldm: bool,
}

impl ServerInfo {
    pub fn parse(json: &str) -> Result<ServerInfo, String> {
        serde_json::from_str(json).map_err(|e| format!("INFO: {e}"))
    }
}

/// `CONNECT`'s options. [`crate::encode::connect`] writes them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Connect {
    pub verbose: bool,
    pub pedantic: bool,
    pub tls_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub lang: String,
    pub version: String,
    pub protocol: u8,
    /// Whether its own messages come back to it.
    pub echo: bool,
    pub headers: bool,
    /// A request no one listens on is answered at once, with status 503.
    pub no_responders: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt: Option<String>,
    /// An NKey's public key, with `sig`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nkey: Option<String>,
    /// The server's nonce signed, base64url.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

impl Default for Connect {
    fn default() -> Connect {
        Connect {
            verbose: false,
            pedantic: false,
            tls_required: false,
            name: None,
            lang: "rust".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: 1,
            echo: true,
            headers: true,
            no_responders: true,
            user: None,
            pass: None,
            auth_token: None,
            jwt: None,
            nkey: None,
            sig: None,
        }
    }
}

impl Connect {
    pub fn new(name: &str) -> Connect {
        Connect { name: (!name.is_empty()).then(|| name.to_string()), ..Connect::default() }
    }

    pub fn user_password(mut self, user: &str, pass: &str) -> Connect {
        self.user = Some(user.into());
        self.pass = Some(pass.into());
        self
    }

    pub fn token(mut self, token: &str) -> Connect {
        self.auth_token = Some(token.into());
        self
    }

    /// Signs in with a key pair: the user's JWT, if it has one (a `.creds`
    /// file's), and `key`'s signature of the server's nonce.
    pub fn nkey(mut self, key: &crate::auth::KeyPair, jwt: Option<&str>, nonce: &str) -> Connect {
        self.jwt = jwt.map(Into::into);
        if jwt.is_none() {
            self.nkey = Some(key.public_key());
        }
        self.sig = Some(key.sign_nonce(nonce));
        self
    }

    pub fn json(&self) -> String {
        serde_json::to_string(self).expect("CONNECT is JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_is_read_and_its_unknowns_ignored() {
        let info = ServerInfo::parse(r#"{"server_id":"NB","server_name":"n1","version":"2.10.14","proto":1,"go":"go1.22","host":"0.0.0.0","port":4222,"headers":true,"max_payload":1048576,"jetstream":true,"client_id":5,"cluster":"c1","nonce":"abc","connect_urls":["10.0.0.1:4222"],"xkey":"X"}"#).unwrap();
        assert_eq!((info.version.as_str(), info.max_payload, info.jetstream, info.cluster.as_deref(), info.nonce.as_deref()), ("2.10.14", 1048576, true, Some("c1"), Some("abc")));
        assert!(ServerInfo::parse("nope").is_err());
    }

    #[test]
    fn connect_leaves_out_what_it_doesnt_use() {
        let json = Connect::new("fenix").user_password("u", "p").json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["user"], "u");
        assert_eq!(v["headers"], true);
        assert_eq!(v["no_responders"], true);
        assert!(v.get("auth_token").is_none() && v.get("sig").is_none());
    }
}
