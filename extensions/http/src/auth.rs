//! Auth: Basic written as `user:password` and encoded as it's sent,
//! Bearer and API keys as headers, and OAuth 2.0 from the env file's
//! `Security.Auth` (JetBrains' names) -- the authorization code grant
//! with PKCE, client credentials, password, and the device code grant --
//! with tokens refreshed when they run out.
//!
//! The authorization code comes back to a port of this machine's
//! loopback it listens on (RFC 8252): the Redirect URL's, when it's
//! `http://127.0.0.1` or `http://localhost` (with no port, or none at
//! all, any free one, the redirect then given it). A redirect elsewhere
//! can't be listened for: the browser is opened at the provider's page
//! and, once you've signed in, you paste the address it was sent to (or
//! just the code). The device code grant needs no redirect at all.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::model::Sent;
use crate::url;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded.
pub fn base64(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 without padding, `-` and `_` for `+` and `/`: PKCE's.
pub fn base64url(b: &[u8]) -> String {
    base64(b).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// Base64 read back, either alphabet, padded or not.
pub fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// An `Authorization` header's value as it's sent: `Basic user:password`
/// (or `Basic user password`, REST Client's) encoded; anything else as
/// it is.
pub fn authorization(value: &str) -> String {
    let Some((scheme, rest)) = value.trim().split_once(char::is_whitespace) else { return value.to_string() };
    if !scheme.eq_ignore_ascii_case("basic") {
        return value.to_string();
    }
    let rest = rest.trim();
    let pair = if rest.contains(':') {
        rest.to_string()
    } else if let Some((user, pass)) = rest.split_once(char::is_whitespace) {
        format!("{user}:{}", pass.trim())
    } else {
        // Encoded already.
        return value.to_string();
    };
    format!("Basic {}", base64(pair.as_bytes()))
}

/// A PKCE verifier and its S256 challenge (RFC 7636).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    /// From 32 random bytes: a verifier of 43 characters.
    pub fn new(random: &[u8; 32]) -> Pkce {
        Pkce::from_verifier(&base64url(random))
    }

    pub fn from_verifier(verifier: &str) -> Pkce {
        Pkce { verifier: verifier.to_string(), challenge: base64url(&Sha256::digest(verifier.as_bytes())) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    AuthorizationCode,
    ClientCredentials,
    Password,
    DeviceCode,
}

/// How the client says who it is to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `Authorization: Basic`, as RFC 6749 prefers.
    Basic,
    /// `client_id` and `client_secret` in the form.
    Body,
}

/// An OAuth 2.0 configuration, from `Security.Auth.<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth {
    pub id: String,
    pub grant: Grant,
    pub auth_url: String,
    pub token_url: String,
    pub device_url: String,
    pub redirect_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    pub pkce: bool,
    pub username: String,
    pub password: String,
    pub client_auth: ClientAuth,
    /// The ID token in place of the access token (`Use ID Token`).
    pub id_token: bool,
    /// More for the authorization and token requests (`Custom Request
    /// Parameters`).
    pub extra: Vec<(String, String)>,
}

impl OAuth {
    /// Configuration `id`, each of its strings given to `fill` first (for
    /// its `{{variables}}`).
    pub fn from_config(id: &str, config: &Map<String, Value>, fill: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<OAuth, String> {
        let get = |k: &str| config.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v);
        let text = |k: &str, fill: &mut dyn FnMut(&str) -> Result<String, String>| -> Result<String, String> {
            match get(k) {
                Some(Value::String(s)) => fill(s),
                Some(v @ (Value::Number(_) | Value::Bool(_))) => Ok(v.to_string()),
                _ => Ok(String::new()),
            }
        };
        let ty = text("Type", fill)?;
        if !ty.is_empty() && !ty.eq_ignore_ascii_case("oauth2") {
            return Err(format!("{id}: Type {ty}: only OAuth2 is read"));
        }
        let grant_text = text("Grant Type", fill)?;
        let grant = match grant_text.to_ascii_lowercase().replace(['_', '-'], " ").as_str() {
            "authorization code" | "" => Grant::AuthorizationCode,
            "client credentials" => Grant::ClientCredentials,
            "password" => Grant::Password,
            "device authorization" | "device code" | "device" => Grant::DeviceCode,
            "implicit" => return Err(format!("{id}: the implicit grant needs the browser to hand the token back; use Authorization Code with PKCE")),
            other => return Err(format!("{id}: Grant Type {other}: one of Authorization Code, Client Credentials, Password, Device Authorization")),
        };
        let pkce = match get("PKCE") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Object(_)) => true,
            _ => grant == Grant::AuthorizationCode,
        };
        let client_auth = match text("Client Credentials", fill)?.to_ascii_lowercase().as_str() {
            "in body" | "body" => ClientAuth::Body,
            _ => ClientAuth::Basic,
        };
        let id_token = matches!(get("Use ID Token"), Some(Value::Bool(true)));
        let mut extra = Vec::new();
        if let Some(Value::Object(params)) = get("Custom Request Parameters") {
            for (k, v) in params {
                let v = match v {
                    Value::String(s) => fill(s)?,
                    Value::Object(o) => o.get("Value").and_then(Value::as_str).map(&mut *fill).transpose()?.unwrap_or_default(),
                    other => other.to_string(),
                };
                extra.push((k.clone(), v));
            }
        }
        let o = OAuth {
            id: id.to_string(),
            grant,
            auth_url: text("Auth URL", fill)?,
            token_url: text("Token URL", fill)?,
            device_url: text("Device Auth URL", fill)?,
            redirect_url: text("Redirect URL", fill)?,
            client_id: text("Client ID", fill)?,
            client_secret: text("Client Secret", fill)?,
            scope: text("Scope", fill)?,
            pkce,
            username: text("Username", fill)?,
            password: text("Password", fill)?,
            client_auth,
            id_token,
            extra,
        };
        if o.token_url.is_empty() {
            return Err(format!("{id}: no Token URL"));
        }
        if o.client_id.is_empty() {
            return Err(format!("{id}: no Client ID"));
        }
        match o.grant {
            Grant::AuthorizationCode if o.auth_url.is_empty() => Err(format!("{id}: the authorization code grant needs an Auth URL")),
            Grant::DeviceCode if o.device_url.is_empty() => Err(format!("{id}: the device grant needs a Device Auth URL")),
            _ => Ok(o),
        }
    }

    /// Where it redirects to: its own, else one on this machine that
    /// nothing listens on, whose address you then paste.
    pub fn redirect(&self) -> &str {
        if self.redirect_url.is_empty() {
            "http://localhost/callback"
        } else {
            &self.redirect_url
        }
    }

    /// The provider's page to sign in at.
    pub fn authorize_url(&self, state: &str, pkce: Option<&Pkce>) -> String {
        let mut q = vec![("response_type".to_string(), "code".to_string()), ("client_id".into(), self.client_id.clone()), ("redirect_uri".into(), self.redirect().to_string()), ("state".into(), state.to_string())];
        if !self.scope.is_empty() {
            q.push(("scope".into(), self.scope.clone()));
        }
        if let Some(p) = pkce {
            q.push(("code_challenge".into(), p.challenge.clone()));
            q.push(("code_challenge_method".into(), "S256".into()));
        }
        q.extend(self.extra.iter().cloned());
        let sep = if self.auth_url.contains('?') { '&' } else { '?' };
        format!("{}{sep}{}", self.auth_url, url::form(&q))
    }

    /// A request to the token endpoint, for `grant`.
    pub fn token_request(&self, grant: &TokenGrant) -> Sent {
        let mut form: Vec<(String, String)> = match grant {
            TokenGrant::Code { code, verifier } => {
                let mut f = vec![("grant_type".to_string(), "authorization_code".to_string()), ("code".into(), code.clone()), ("redirect_uri".into(), self.redirect().to_string())];
                if let Some(v) = verifier {
                    f.push(("code_verifier".into(), v.clone()));
                }
                f
            }
            TokenGrant::ClientCredentials => vec![("grant_type".into(), "client_credentials".into())],
            TokenGrant::Password => vec![("grant_type".into(), "password".into()), ("username".into(), self.username.clone()), ("password".into(), self.password.clone())],
            TokenGrant::Device { device_code } => vec![("grant_type".into(), "urn:ietf:params:oauth:grant-type:device_code".into()), ("device_code".into(), device_code.clone())],
            TokenGrant::Refresh { refresh_token } => vec![("grant_type".into(), "refresh_token".into()), ("refresh_token".into(), refresh_token.clone())],
        };
        if !self.scope.is_empty() && matches!(grant, TokenGrant::ClientCredentials | TokenGrant::Password) {
            form.push(("scope".into(), self.scope.clone()));
        }
        self.with_client(form, &self.token_url)
    }

    /// The device grant's first request (RFC 8628).
    pub fn device_request(&self) -> Sent {
        let mut form = Vec::new();
        if !self.scope.is_empty() {
            form.push(("scope".to_string(), self.scope.clone()));
        }
        self.with_client(form, &self.device_url)
    }

    fn with_client(&self, mut form: Vec<(String, String)>, to: &str) -> Sent {
        let mut headers = vec![("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string()), ("Accept".to_string(), "application/json".to_string())];
        if self.client_secret.is_empty() || self.client_auth == ClientAuth::Body {
            form.push(("client_id".into(), self.client_id.clone()));
            if !self.client_secret.is_empty() {
                form.push(("client_secret".into(), self.client_secret.clone()));
            }
        } else {
            let pair = format!("{}:{}", url::encode(&self.client_id, true), url::encode(&self.client_secret, true));
            headers.push(("Authorization".into(), format!("Basic {}", base64(pair.as_bytes()))));
        }
        for (k, v) in &self.extra {
            if !form.iter().any(|(f, _)| f == k) {
                form.push((k.clone(), v.clone()));
            }
        }
        Sent { method: "POST".into(), url: to.to_string(), headers, body: url::form(&form).into_bytes() }
    }
}

/// Where a redirect to this machine is listened for: the port (0, any
/// free one) and the path. None for a redirect that isn't plain HTTP to
/// the loopback, which can't be.
pub fn loopback(redirect: &str) -> Option<(u16, String)> {
    if redirect.is_empty() {
        return Some((0, "/callback".into()));
    }
    let rest = redirect.strip_prefix("http://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, Some(p.parse::<u16>().ok()?)),
        None => (authority, None),
    };
    if !matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost") {
        return None;
    }
    Some((port.unwrap_or(0), url::path(redirect)))
}

/// The redirect sent, listening on `port`: the one written when it gave
/// its port, else it with `port` in it (`http://127.0.0.1:<port>/callback`
/// when none was written).
pub fn redirect_on(redirect: &str, port: u16) -> String {
    if redirect.is_empty() {
        return format!("http://127.0.0.1:{port}/callback");
    }
    match loopback(redirect) {
        Some((0, _)) => {
            let rest = &redirect["http://".len()..];
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            format!("http://{}:{port}{}", &rest[..end], &rest[end..])
        }
        _ => redirect.to_string(),
    }
}

/// The browser's request to the redirect, once its head has all come:
/// what it asked for (path and query), or None for one that isn't a GET.
pub fn callback_target(request: &[u8]) -> Option<Option<String>> {
    let end = request.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&request[..end]);
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split(' ');
    Some(match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Some(target.to_string()),
        _ => None,
    })
}

/// What the browser's shown at the redirect: a page saying how it went.
pub fn callback_page(status: u16, message: &str) -> Vec<u8> {
    let escaped = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let body = format!("<!doctype html><meta charset=\"utf-8\"><title>Fenix</title><body style=\"font-family:sans-serif;margin:3em\"><p>{escaped}</p></body>");
    let reason = if status == 200 { "OK" } else { "Not Found" };
    format!("HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}", body.len()).into_bytes()
}

/// What's asked of the token endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenGrant {
    Code { code: String, verifier: Option<String> },
    ClientCredentials,
    Password,
    Device { device_code: String },
    Refresh { refresh_token: String },
}

/// The code from what you pasted: the address the provider sent the
/// browser to (its `state` checked), or the code itself.
pub fn code_from_paste(pasted: &str, state: &str) -> Result<String, String> {
    let p = pasted.trim();
    if p.is_empty() {
        return Err("nothing pasted".into());
    }
    if !p.contains("code=") && !p.contains("error=") {
        return Ok(p.to_string());
    }
    let query = p.split_once('?').map(|(_, q)| q).unwrap_or(p);
    let query = query.replace('#', "&");
    let params = url::parse_form(&query);
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if let Some(error) = get("error") {
        let detail = get("error_description").map(|d| format!(": {d}")).unwrap_or_default();
        return Err(format!("the provider said {error}{detail}"));
    }
    if let Some(s) = get("state") {
        if s != state {
            return Err("that address is for another sign-in (its state isn't this one's)".into());
        }
    }
    get("code").filter(|c| !c.is_empty()).ok_or_else(|| "no code in that address".into())
}

/// A token, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access: String,
    pub refresh: Option<String>,
    pub id_token: Option<String>,
    /// When it runs out, in seconds since 1970.
    pub expires: Option<i64>,
}

impl Token {
    /// Good for a while yet: 30 seconds kept in hand.
    pub fn good(&self, now: i64) -> bool {
        self.expires.is_none_or(|e| e - 30 > now)
    }

    pub fn value(&self, id_token: bool) -> &str {
        if id_token {
            self.id_token.as_deref().unwrap_or(&self.access)
        } else {
            &self.access
        }
    }
}

/// What the token endpoint said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenAnswer {
    Token(Token),
    /// Device grant: not signed in yet; ask again (slower when it says so).
    Pending { slow_down: bool },
    Error(String),
}

/// The token endpoint's answer, read: `now` is when it came.
pub fn read_token(status: u16, body: &str, now: i64, old_refresh: Option<&str>) -> TokenAnswer {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            // A few still answer as a form (GitHub's, unasked).
            let form = url::parse_form(body.trim());
            let mut m = Map::new();
            for (k, val) in form {
                m.insert(k, Value::String(val));
            }
            Value::Object(m)
        }
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())));
    if let Some(error) = s("error") {
        return match error.as_str() {
            "authorization_pending" => TokenAnswer::Pending { slow_down: false },
            "slow_down" => TokenAnswer::Pending { slow_down: true },
            _ => TokenAnswer::Error(match s("error_description") {
                Some(d) => format!("{error}: {d}"),
                None => error,
            }),
        };
    }
    match s("access_token") {
        Some(access) if (200..300).contains(&status) => TokenAnswer::Token(Token {
            access,
            refresh: s("refresh_token").or_else(|| old_refresh.map(String::from)),
            id_token: s("id_token"),
            expires: s("expires_in").and_then(|e| e.parse::<i64>().ok()).map(|e| now + e),
        }),
        _ => TokenAnswer::Error(format!("the token endpoint answered {status} without a token")),
    }
}

/// The device grant's start (RFC 8628 §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    /// Seconds between asks.
    pub interval: u32,
    /// When it runs out, in seconds since 1970.
    pub expires: i64,
}

pub fn read_device(body: &str, now: i64) -> Result<Device, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("the device endpoint's answer isn't JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    if let Some(e) = s("error") {
        return Err(e);
    }
    Ok(Device {
        device_code: s("device_code").ok_or("no device_code")?,
        user_code: s("user_code").ok_or("no user_code")?,
        verification_uri: s("verification_uri").or_else(|| s("verification_url")).ok_or("no verification_uri")?,
        verification_uri_complete: s("verification_uri_complete"),
        interval: v.get("interval").and_then(Value::as_u64).unwrap_or(5) as u32,
        expires: now + v.get("expires_in").and_then(Value::as_i64).unwrap_or(900),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_both_ways() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(unbase64("Zm8=").unwrap(), b"fo");
        assert_eq!(unbase64("Zm9vYg").unwrap(), b"foob");
        assert_eq!(unbase64(&base64(&[0xfb, 0xff, 0x00])).unwrap(), [0xfb, 0xff, 0x00]);
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert!(unbase64("!!").is_none());
    }

    #[test]
    fn basic_is_encoded_as_it_goes() {
        assert_eq!(authorization("Basic user:pass"), "Basic dXNlcjpwYXNz");
        assert_eq!(authorization("Basic user pass"), "Basic dXNlcjpwYXNz");
        assert_eq!(authorization("Basic dXNlcjpwYXNz"), "Basic dXNlcjpwYXNz", "encoded already");
        assert_eq!(authorization("Bearer abc"), "Bearer abc");
        assert_eq!(authorization("token"), "token");
    }

    /// The challenge is the verifier's SHA-256, base64url: the value
    /// Python's hashlib and base64 give for it.
    #[test]
    fn pkce_challenge_is_s256() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r7wW1gFWFOEjXk");
        assert_eq!(p.challenge, "bwWFMyPfdG9qreDhH2lmftFx_dFeLDalzcT1gb_j68g");
        let p = Pkce::new(&[7; 32]);
        assert_eq!(p.verifier.len(), 43);
        assert!(p.verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    fn config(json: &str) -> Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    fn fill(s: &str) -> Result<String, String> {
        Ok(s.replace("{{tenant}}", "acme"))
    }

    #[test]
    fn an_authorization_code_configuration_is_read_and_used() {
        let c = config(r#"{"Type":"OAuth2","Grant Type":"Authorization Code","Auth URL":"https://{{tenant}}.auth.test/authorize","Token URL":"https://{{tenant}}.auth.test/token","Client ID":"app","Client Secret":"s","Scope":"openid profile","Redirect URL":"http://localhost:8765/cb","PKCE":true,"Custom Request Parameters":{"audience":"api"}}"#);
        let o = OAuth::from_config("main", &c, &mut fill).unwrap();
        assert_eq!((o.grant, o.pkce, o.client_auth), (Grant::AuthorizationCode, true, ClientAuth::Basic));
        assert_eq!(o.auth_url, "https://acme.auth.test/authorize");
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r7wW1gFWFOEjXk");
        let u = o.authorize_url("st4te", Some(&p));
        assert_eq!(
            u,
            "https://acme.auth.test/authorize?response_type=code&client_id=app&redirect_uri=http%3A%2F%2Flocalhost%3A8765%2Fcb&state=st4te&scope=openid+profile&code_challenge=bwWFMyPfdG9qreDhH2lmftFx_dFeLDalzcT1gb_j68g&code_challenge_method=S256&audience=api"
        );
        let r = o.token_request(&TokenGrant::Code { code: "c0de".into(), verifier: Some(p.verifier.clone()) });
        assert_eq!(r.url, "https://acme.auth.test/token");
        assert_eq!(r.header("Authorization"), Some("Basic YXBwOnM="));
        assert_eq!(String::from_utf8(r.body).unwrap(), "grant_type=authorization_code&code=c0de&redirect_uri=http%3A%2F%2Flocalhost%3A8765%2Fcb&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r7wW1gFWFOEjXk&audience=api");
    }

    #[test]
    fn other_grants() {
        let c = config(r#"{"Type":"OAuth2","Grant Type":"Client Credentials","Token URL":"https://a.test/t","Client ID":"app","Client Secret":"s","Client Credentials":"in body","Scope":"read"}"#);
        let o = OAuth::from_config("cc", &c, &mut fill).unwrap();
        let r = o.token_request(&TokenGrant::ClientCredentials);
        assert_eq!(r.header("Authorization"), None);
        assert_eq!(String::from_utf8(r.body).unwrap(), "grant_type=client_credentials&scope=read&client_id=app&client_secret=s");
        let r = o.token_request(&TokenGrant::Refresh { refresh_token: "r1".into() });
        assert_eq!(String::from_utf8(r.body).unwrap(), "grant_type=refresh_token&refresh_token=r1&client_id=app&client_secret=s");
        let c = config(r#"{"Grant Type":"Device Authorization","Device Auth URL":"https://a.test/device","Token URL":"https://a.test/t","Client ID":"cli"}"#);
        let o = OAuth::from_config("dev", &c, &mut fill).unwrap();
        assert_eq!(String::from_utf8(o.device_request().body).unwrap(), "client_id=cli");
        assert_eq!(String::from_utf8(o.token_request(&TokenGrant::Device { device_code: "d".into() }).body).unwrap(), "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&device_code=d&client_id=cli");
        assert!(OAuth::from_config("x", &config(r#"{"Client ID":"a"}"#), &mut fill).unwrap_err().contains("Token URL"));
        assert!(OAuth::from_config("x", &config(r#"{"Grant Type":"Implicit","Token URL":"t","Client ID":"a"}"#), &mut fill).unwrap_err().contains("PKCE"));
    }

    #[test]
    fn the_redirect_is_listened_for_on_the_loopback() {
        assert_eq!(loopback(""), Some((0, "/callback".into())));
        assert_eq!(loopback("http://localhost/callback"), Some((0, "/callback".into())));
        assert_eq!(loopback("http://127.0.0.1:8400/oauth/cb?x=1"), Some((8400, "/oauth/cb".into())));
        assert_eq!(loopback("http://LOCALHOST:9000"), Some((9000, "/".into())));
        assert_eq!(loopback("https://localhost/cb"), None, "TLS can't be answered");
        assert_eq!(loopback("https://app.example.com/cb"), None);
        assert_eq!(loopback("http://localhost:x/cb"), None);
        assert_eq!(redirect_on("", 51234), "http://127.0.0.1:51234/callback");
        assert_eq!(redirect_on("http://localhost/callback", 51234), "http://localhost:51234/callback");
        assert_eq!(redirect_on("http://127.0.0.1", 7), "http://127.0.0.1:7");
        assert_eq!(redirect_on("http://127.0.0.1:8400/cb", 8400), "http://127.0.0.1:8400/cb");
        assert_eq!(callback_target(b"GET /callback?code=abc&state=s HTTP/1.1\r\nHost: 127.0.0.1"), None, "not all come");
        assert_eq!(callback_target(b"GET /callback?code=abc&state=s HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"), Some(Some("/callback?code=abc&state=s".into())));
        assert_eq!(callback_target(b"POST / HTTP/1.1\r\n\r\n"), Some(None));
        assert_eq!(code_from_paste("/callback?code=abc&state=s", "s"), Ok("abc".into()), "the target is read as an address is");
        let page = String::from_utf8(callback_page(200, "Signed in <api>")).unwrap();
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n") && page.contains("Signed in &lt;api&gt;") && page.contains("Connection: close"));
        let len: usize = page.split("Content-Length: ").nth(1).unwrap().split("\r\n").next().unwrap().parse().unwrap();
        assert_eq!(len, page.split("\r\n\r\n").nth(1).unwrap().len());
    }

    #[test]
    fn the_code_comes_from_what_was_pasted() {
        assert_eq!(code_from_paste("http://localhost/callback?code=abc&state=s1", "s1"), Ok("abc".into()));
        assert_eq!(code_from_paste("  abc123  ", "s1"), Ok("abc123".into()));
        assert!(code_from_paste("http://localhost/cb?code=abc&state=other", "s1").unwrap_err().contains("another"));
        assert!(code_from_paste("http://localhost/cb?error=access_denied&error_description=No+way", "s1").unwrap_err().contains("access_denied: No way"));
        assert!(code_from_paste("", "s").is_err());
    }

    #[test]
    fn token_answers_are_read() {
        let a = read_token(200, r#"{"access_token":"at","refresh_token":"rt","expires_in":3600,"token_type":"Bearer"}"#, 1000, None);
        let TokenAnswer::Token(t) = a else { panic!("{a:?}") };
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.expires), ("at", Some("rt"), Some(4600)));
        assert!(t.good(4500) && !t.good(4580));
        let TokenAnswer::Token(t) = read_token(200, r#"{"access_token":"at2"}"#, 0, Some("rt")) else { panic!() };
        assert_eq!((t.refresh.as_deref(), t.expires), (Some("rt"), None), "the old refresh token kept");
        assert!(t.good(i64::MAX - 1));
        assert_eq!(read_token(400, r#"{"error":"authorization_pending"}"#, 0, None), TokenAnswer::Pending { slow_down: false });
        assert_eq!(read_token(400, r#"{"error":"slow_down"}"#, 0, None), TokenAnswer::Pending { slow_down: true });
        assert_eq!(read_token(400, r#"{"error":"invalid_grant","error_description":"expired"}"#, 0, None), TokenAnswer::Error("invalid_grant: expired".into()));
        assert!(matches!(read_token(200, "access_token=gh&scope=repo", 0, None), TokenAnswer::Token(t) if t.access == "gh"));
        let d = read_device(r#"{"device_code":"dc","user_code":"ABCD-EFGH","verification_uri":"https://a.test/activate","interval":7,"expires_in":600}"#, 100).unwrap();
        assert_eq!((d.user_code.as_str(), d.interval, d.expires), ("ABCD-EFGH", 7, 700));
    }
}
