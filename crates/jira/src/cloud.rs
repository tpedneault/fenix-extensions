//! Which Jira it is -- Cloud, or Server and Data Center -- and how it's
//! signed in to.
//!
//! One extension serves both, told apart by the site's address: a
//! `*.atlassian.net` site (or one of the older `*.jira.com`) is Cloud,
//! anything else Server or Data Center, unless `jira.kind` says. On first
//! contact `/rest/api/2/serverInfo` says for sure (`deploymentType`),
//! which also finds a Cloud site on its own domain.
//!
//! Server and Data Center take a personal access token as a bearer
//! token, as they always have. Cloud takes your Atlassian account's
//! email and an API token, sent as HTTP Basic. A *scoped* API token
//! isn't taken at the site's own address, only through Atlassian's
//! gateway, `https://api.atlassian.com/ex/jira/<cloudId>`; the site's
//! `/_edge/tenant_info` says its `cloudId`.

/// What kind of Jira a site is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Deployment {
    /// Jira Server or Data Center: REST v2, usernames, wiki markup.
    #[default]
    Server,
    /// Jira Cloud: REST v3 where text is rich (ADF), account ids, the
    /// enhanced search.
    Cloud,
}

/// The host of a URL, lower case: `acme.atlassian.net`.
pub fn host_of(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    host.split(':').next().unwrap_or_default().to_ascii_lowercase()
}

impl Deployment {
    /// What the site's address says: Atlassian's own hosts are Cloud.
    pub fn guess(base_url: &str) -> Deployment {
        let host = host_of(base_url);
        if host.ends_with(".atlassian.net") || host.ends_with(".jira.com") {
            Deployment::Cloud
        } else {
            Deployment::Server
        }
    }

    /// `jira.kind`: `cloud` or `server` force it; `auto`, or nothing,
    /// leave it to the address and the server.
    pub fn forced(setting: Option<&str>) -> Option<Deployment> {
        match setting.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("cloud") => Some(Deployment::Cloud),
            Some("server") | Some("data center") | Some("datacenter") => Some(Deployment::Server),
            _ => None,
        }
    }

    /// What `/rest/api/2/serverInfo` says it is, and in words for the
    /// page: `Cloud`, `Data Center 9.12.4`, `Server 8.20.1`.
    pub fn from_server_info(body: &str) -> Option<(Deployment, String)> {
        let v: serde_json::Value = serde_json::from_str(body).ok()?;
        let kind = v.get("deploymentType")?.as_str()?;
        let version = v.get("version").and_then(|x| x.as_str()).unwrap_or_default();
        Some(match kind {
            "Cloud" => (Deployment::Cloud, "Cloud".to_string()),
            "DataCenter" => (Deployment::Server, format!("Data Center {version}").trim().to_string()),
            other => (Deployment::Server, format!("{other} {version}").trim().to_string()),
        })
    }

    pub fn is_cloud(self) -> bool {
        self == Deployment::Cloud
    }

    /// As the page and the messages name it.
    pub fn name(self) -> &'static str {
        match self {
            Deployment::Cloud => "Jira Cloud",
            Deployment::Server => "Jira Server or Data Center",
        }
    }
}

/// How requests are signed in. Its `Debug` never shows the token.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// Server and Data Center: a personal access token.
    Bearer(String),
    /// Cloud: your account's email and an API token.
    Basic { email: String, token: String },
}

impl Auth {
    /// The `Authorization` header's value.
    pub fn header(&self) -> String {
        match self {
            Auth::Bearer(token) => format!("Bearer {token}"),
            Auth::Basic { email, token } => format!("Basic {}", base64(format!("{email}:{token}").as_bytes())),
        }
    }
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::Bearer(_) => f.write_str("a personal access token"),
            Auth::Basic { email, .. } => write!(f, "{email} with an API token"),
        }
    }
}

/// Standard base64, padded (RFC 4648), as HTTP Basic wants it.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Where a scoped API token is taken: Atlassian's gateway for the site.
pub fn gateway(cloud_id: &str) -> String {
    format!("https://api.atlassian.com/ex/jira/{cloud_id}")
}

/// The site's `cloudId`, from its `/_edge/tenant_info`.
pub fn cloud_id(tenant_info: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(tenant_info).ok()?;
    let id = v.get("cloudId")?.as_str()?;
    // An id goes in a path: nothing but what one is made of.
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_says_which_until_the_server_does() {
        for (url, want) in [
            ("https://acme.atlassian.net", Deployment::Cloud),
            ("https://ACME.atlassian.net/", Deployment::Cloud),
            ("acme.atlassian.net", Deployment::Cloud),
            ("https://old.jira.com/browse", Deployment::Cloud),
            ("https://jira.example.com", Deployment::Server),
            ("https://atlassian.net.example.com", Deployment::Server),
            ("https://jira.example.com:8443/jira", Deployment::Server),
            ("https://notatlassian.net", Deployment::Server),
        ] {
            assert_eq!(Deployment::guess(url), want, "{url}");
        }
        assert_eq!(host_of("https://me@acme.atlassian.net:443/x"), "acme.atlassian.net");
        assert_eq!(Deployment::forced(Some("Cloud")), Some(Deployment::Cloud));
        assert_eq!(Deployment::forced(Some("server")), Some(Deployment::Server));
        assert_eq!(Deployment::forced(Some("auto")), None);
        assert_eq!(Deployment::forced(None), None);
    }

    #[test]
    fn server_info_says_for_sure() {
        let cloud = r#"{"baseUrl":"https://jira.acme.dev","version":"1001.0.0-SNAPSHOT","versionNumbers":[1001,0,0],"deploymentType":"Cloud","buildNumber":100277,"serverTitle":"Jira"}"#;
        assert_eq!(Deployment::from_server_info(cloud), Some((Deployment::Cloud, "Cloud".into())));
        let dc = r#"{"baseUrl":"https://jira.example.com","version":"9.12.4","deploymentType":"DataCenter"}"#;
        assert_eq!(Deployment::from_server_info(dc), Some((Deployment::Server, "Data Center 9.12.4".into())));
        let server = r#"{"version":"8.20.1","deploymentType":"Server"}"#;
        assert_eq!(Deployment::from_server_info(server), Some((Deployment::Server, "Server 8.20.1".into())));
        assert_eq!(Deployment::from_server_info(r#"{"version":"7.0"}"#), None, "too old to say");
        assert_eq!(Deployment::from_server_info("<html>"), None);
    }

    #[test]
    fn basic_is_the_email_and_token_in_base64_and_debug_never_shows_the_token() {
        // RFC 7617's own example.
        let auth = Auth::Basic { email: "Aladdin".into(), token: "open sesame".into() };
        assert_eq!(auth.header(), "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        let auth = Auth::Basic { email: "jo@example.com".into(), token: "ATATT3xFfGF0-secret".into() };
        assert_eq!(auth.header(), format!("Basic {}", base64(b"jo@example.com:ATATT3xFfGF0-secret")));
        assert_eq!(format!("{auth:?}"), "jo@example.com with an API token");
        assert_eq!(Auth::Bearer("pat".into()).header(), "Bearer pat");
        assert!(!format!("{:?}", Auth::Bearer("pat-secret".into())).contains("pat-secret"));
        for (input, want) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(input.as_bytes()), want, "RFC 4648 {input:?}");
        }
    }

    #[test]
    fn a_scoped_token_goes_through_the_gateway() {
        assert_eq!(cloud_id(r#"{"cloudId":"35273b54-3f06-40d2-880f-dd28cf8daafa"}"#).as_deref(), Some("35273b54-3f06-40d2-880f-dd28cf8daafa"));
        assert_eq!(gateway("35273b54-3f06-40d2-880f-dd28cf8daafa"), "https://api.atlassian.com/ex/jira/35273b54-3f06-40d2-880f-dd28cf8daafa");
        assert_eq!(cloud_id(r#"{"cloudId":"../../evil"}"#), None);
        assert_eq!(cloud_id("nope"), None);
    }
}
