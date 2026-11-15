//! Issues on a forge -- GitHub's and GitLab's -- in neutral terms: a
//! search, an issue with its comments, a new one, and the changes made
//! to one (its title, body, state, labels, assignees). Shared by the
//! GitHub Issues and GitLab Issues extensions, and by the tracker seam
//! they serve the agenda (`crate::tracker`, feature `tracker`).
//!
//! Like the review clients, these talk HTTP through [`Http`] and keep no
//! state but where the forge is and the token. Unlike them, a failure is
//! an [`ApiError`] rather than text, because the issue extensions do more
//! with one than show it: a 404 is an issue that isn't there, a 401 a
//! sign-in to renew, a rate limit a wait of so many seconds.

pub mod github;
pub mod gitlab;

use crate::{Http, Response};

/// Which forge an issue is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    GitHub,
    GitLab,
}

impl Host {
    pub fn name(self) -> &'static str {
        match self {
            Host::GitHub => "GitHub",
            Host::GitLab => "GitLab",
        }
    }

    /// What a request on it is called: "pull request", "merge request".
    pub fn request(self) -> &'static str {
        match self {
            Host::GitHub => "pull request",
            Host::GitLab => "merge request",
        }
    }

    /// What it calls a place issues live: "repository", "project".
    pub fn place(self) -> &'static str {
        match self {
            Host::GitHub => "repository",
            Host::GitLab => "project",
        }
    }
}

/// An issue's address: its repository or project (`owner/repo`,
/// `group/sub/project`) and its number there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueRef {
    pub project: String,
    pub number: u64,
}

impl IssueRef {
    pub fn new(project: impl Into<String>, number: u64) -> IssueRef {
        IssueRef { project: project.into(), number }
    }

    /// `owner/repo#12`: what the tracker seam keeps.
    pub fn key(&self) -> String {
        format!("{}#{}", self.project, self.number)
    }

    /// `owner/repo#12`, or an issue's address on the forge at `web`
    /// (`https://github.com`, `https://gitlab.example.com`): GitHub's
    /// `/owner/repo/issues/12`, GitLab's `/group/sub/project/-/issues/12`
    /// (or `/-/work_items/12`, or the older `/issues/12`). A project path
    /// has two parts on GitHub and at least two on GitLab.
    pub fn parse(text: &str, host: Host, web: &str) -> Option<IssueRef> {
        let text = text.trim();
        if let Some((project, number)) = text.rsplit_once('#') {
            if !project.contains("://") {
                return IssueRef::checked(project, number, host);
            }
        }
        let lower = text.to_ascii_lowercase();
        let web = web.trim_end_matches('/').to_ascii_lowercase();
        let rest = lower.strip_prefix(&web).filter(|r| r.starts_with('/'))?;
        let rest = &text[text.len() - rest.len() + 1..];
        let rest = rest.split(['?', '#']).next().unwrap_or(rest).trim_end_matches('/');
        let (project, number) = match host {
            Host::GitHub => rest.rsplit_once("/issues/")?,
            Host::GitLab => rest.rsplit_once("/-/issues/").or_else(|| rest.rsplit_once("/-/work_items/")).or_else(|| rest.rsplit_once("/issues/"))?,
        };
        IssueRef::checked(project, number, host)
    }

    fn checked(project: &str, number: &str, host: Host) -> Option<IssueRef> {
        let number: u64 = number.trim().parse().ok().filter(|n| *n > 0)?;
        let project = project.trim().trim_matches('/');
        let parts: Vec<&str> = project.split('/').collect();
        let ok_part = |p: &&str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && *p != "." && *p != "..";
        let shape = match host {
            Host::GitHub => parts.len() == 2,
            Host::GitLab => parts.len() >= 2 && parts.len() <= 20,
        };
        (shape && parts.iter().all(ok_part)).then(|| IssueRef::new(project, number))
    }

    /// The same issue, told apart from another case-blind (forges match
    /// a path whatever its case).
    pub fn same(&self, other: &IssueRef) -> bool {
        self.number == other.number && self.project.eq_ignore_ascii_case(&other.project)
    }
}

impl std::fmt::Display for IssueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.project, self.number)
    }
}

/// Someone on the forge. `username` is what assigns them, on both:
/// GitHub's login, GitLab's username.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct User {
    pub username: String,
    /// Their name, when the forge gives it; else the username.
    pub name: String,
    /// GitLab's numeric id, which its writes take; empty on GitHub.
    pub id: String,
}

impl User {
    pub fn named(username: &str) -> User {
        User { username: username.into(), name: username.into(), id: String::new() }
    }
}

/// An issue, as both forges have one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Issue {
    pub key: IssueRef,
    /// The forge's own id for it, which doesn't change when it moves to
    /// another repository or project: GitHub's `node_id`, GitLab's
    /// global `id`.
    pub id: String,
    pub title: String,
    /// Markdown, as it was written.
    pub body: String,
    pub open: bool,
    /// Why it's closed (GitHub: `completed`, `not_planned`, `duplicate`;
    /// `reopened` once reopened); none on GitLab.
    pub state_reason: Option<String>,
    pub author: String,
    pub assignees: Vec<User>,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    /// `YYYY-MM-DD` (GitLab).
    pub due: Option<String>,
    /// GitLab's weight (Premium), when it has one.
    pub weight: Option<u64>,
    /// GitLab's time tracking, in seconds: estimated and spent.
    pub estimate: Option<u64>,
    pub spent: Option<u64>,
    pub url: String,
    pub created: String,
    pub updated: String,
    /// How many comments it has.
    pub comments: u64,
    /// GitLab: the global id of the issue it was moved to; a moved issue
    /// stays behind, closed.
    pub moved_to: Option<String>,
    pub confidential: bool,
}

impl Default for IssueRef {
    fn default() -> Self {
        IssueRef::new("", 0)
    }
}

impl Issue {
    /// Whether `username` is among its assignees.
    pub fn assigned_to(&self, username: &str) -> bool {
        self.assignees.iter().any(|a| a.username.eq_ignore_ascii_case(username))
    }

    /// Whether it has label `name` (as forges match labels: case-blind).
    pub fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|l| l.eq_ignore_ascii_case(name))
    }

    /// "open", "closed", "closed as not planned".
    pub fn state_label(&self) -> String {
        match (self.open, self.state_reason.as_deref()) {
            (true, _) => "open".into(),
            (false, Some("not_planned")) => "closed as not planned".into(),
            (false, Some("duplicate")) => "closed as a duplicate".into(),
            (false, _) => "closed".into(),
        }
    }
}

/// One comment on an issue. `system` is the forge narrating ("changed
/// the label"), which GitLab keeps among the comments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created: String,
    pub system: bool,
}

/// A pull or merge request that mentions or closes an issue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linked {
    /// `owner/repo#7` or `group/project!7`.
    pub reference: String,
    pub title: String,
    pub url: String,
    /// `open`, `closed`, `merged`.
    pub state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub description: String,
}

/// An issue template a repository or project keeps: a starting body,
/// and on GitHub a title, labels and assignees from its front matter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
}

/// A page of issues, and the cursor for the next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub issues: Vec<Issue>,
    pub next: Option<String>,
}

/// Which issues to list. Open ones, most recently updated first,
/// except a [`Search::Query`], which says for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Search {
    /// Assigned to you, across repositories or projects.
    Assigned,
    /// Opened by you.
    Created,
    /// Mentioning you.
    Mentioned,
    /// A repository's or project's.
    Project(String),
    /// Typed in the forge's own search syntax.
    Query(String),
}

/// Asked of `assigned`: only those changed at or after `since` (RFC
/// 3339), as the seam's `since` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paging {
    pub cursor: Option<String>,
    pub per_page: u32,
    pub since: Option<String>,
}

impl Paging {
    pub fn first(per_page: u32) -> Paging {
        Paging { cursor: None, per_page, since: None }
    }

    /// The page number the cursor names, from 1.
    pub fn page(&self) -> u32 {
        self.cursor.as_deref().and_then(|c| c.parse().ok()).filter(|p| *p > 0).unwrap_or(1)
    }

    pub fn per_page(&self) -> u32 {
        self.per_page.clamp(1, 100)
    }
}

/// A new issue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewIssue {
    pub project: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// Usernames.
    pub assignees: Vec<String>,
}

/// One change made to an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Title(String),
    Body(String),
    /// Its labels, all of them.
    Labels(Vec<String>),
    AddLabel(String),
    RemoveLabel(String),
    /// Its assignees, all of them, by username; none unassigns.
    Assignees(Vec<String>),
    /// Closed, with GitHub's reason (`completed`, `not_planned`);
    /// GitLab has none.
    Close(Option<String>),
    Reopen,
    /// GitLab's due date; none clears it.
    Due(Option<String>),
}

/// Why a request didn't do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// The forge's status, or none when nothing came back.
    pub status: Option<u16>,
    /// The status and the forge's own words: `HTTP 404 (Not Found)`.
    pub message: String,
    /// Slow down for this long, when the forge said so.
    pub retry_after: Option<u64>,
}

impl ApiError {
    /// Nothing came back: why.
    pub fn offline(why: impl Into<String>) -> ApiError {
        ApiError { status: None, message: format!("request failed: {}", why.into()), retry_after: None }
    }

    pub fn status(status: u16, message: impl Into<String>) -> ApiError {
        ApiError { status: Some(status), message: message.into(), retry_after: None }
    }

    /// Something came back that wasn't what the forge says it sends.
    pub fn odd(what: impl Into<String>) -> ApiError {
        ApiError { status: Some(200), message: what.into(), retry_after: None }
    }

    pub fn rate_limited(&self) -> bool {
        self.retry_after.is_some()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// An issue forge's client: what the issue extensions and the tracker
/// seam ask of GitHub and GitLab alike.
#[allow(async_fn_in_trait)]
pub trait Issues {
    fn host(&self) -> Host;

    /// The forge's web address: `https://github.com`.
    fn web(&self) -> &str;

    /// Who the token belongs to.
    async fn me(&self) -> Result<User, ApiError>;

    /// A page of issues: never a pull or merge request.
    async fn search(&self, search: &Search, paging: &Paging) -> Result<Listing, ApiError>;

    /// One issue; a pull request's number is `not found`, as it isn't an
    /// issue.
    async fn issue(&self, key: &IssueRef) -> Result<Issue, ApiError>;

    /// Its comments, the most recent 100, oldest first, the forge's own
    /// narration among them (`system`).
    async fn notes(&self, key: &IssueRef) -> Result<Vec<Note>, ApiError>;

    /// The pull or merge requests that mention or close it.
    async fn linked(&self, key: &IssueRef) -> Result<Vec<Linked>, ApiError>;

    async fn create(&self, new: &NewIssue) -> Result<Issue, ApiError>;

    async fn change(&self, key: &IssueRef, change: &Change) -> Result<(), ApiError>;

    async fn comment(&self, key: &IssueRef, body: &str) -> Result<(), ApiError>;

    /// A repository's or project's labels.
    async fn labels(&self, project: &str) -> Result<Vec<Label>, ApiError>;

    /// Who issues there can be assigned to.
    async fn assignable(&self, project: &str) -> Result<Vec<User>, ApiError>;

    /// Its issue templates, when it keeps any.
    async fn templates(&self, project: &str) -> Result<Vec<Template>, ApiError>;

    /// Whether `text` is this forge's immutable id for an issue rather
    /// than a ref or an address: GitHub's node id (`I_kwDO...`), GitLab's
    /// global id (digits).
    fn is_id(&self, text: &str) -> bool {
        let t = text.trim();
        match self.host() {
            Host::GitHub => !t.is_empty() && t.len() <= 100 && !t.contains(['/', '#', ':']) && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=')) && t.chars().any(|c| c.is_ascii_alphabetic()),
            Host::GitLab => !t.is_empty() && t.len() <= 20 && t.chars().all(|c| c.is_ascii_digit()),
        }
    }

    /// The issue an immutable id names, wherever it lives now (it may
    /// have moved): asked of the forge's GraphQL API, the only one that
    /// finds an issue by it for anyone but an administrator.
    async fn resolve(&self, id: &str) -> Result<IssueRef, ApiError>;

    /// Time spent on it, filed on `day` (`YYYY-MM-DD`): GitLab's time
    /// tracking. GitHub has none.
    async fn spend(&self, key: &IssueRef, seconds: i64, day: &str) -> Result<(), ApiError> {
        let _ = (key, seconds, day);
        Err(ApiError::status(422, format!("{} issues have no time tracking", self.host().name())))
    }
}

/// A header's value, its name matched case-blind.
pub fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
}

/// The URL a `Link` header gives as `rel="next"`.
pub fn link_next(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        params.split(';').any(|p| p.trim().replace(' ', "") == "rel=\"next\"").then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_string())
    })
}

/// The `page` a URL asks for.
pub fn page_of(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix("page=")).map(str::to_string).filter(|p| p.parse::<u32>().is_ok())
}

/// Seconds since 1970 of an HTTP date, `Sun, 06 Nov 1994 08:49:37 GMT`
/// (the form servers send; the two obsolete ones aren't read).
pub fn http_date(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, zone] = parts.as_slice() else { return None };
    if *zone != "GMT" && *zone != "UTC" {
        return None;
    }
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| m.eq_ignore_ascii_case(month))? as i64 + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let hms: Vec<i64> = time.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [h, m, s] = hms.as_slice() else { return None };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + m * 60 + s).ok()
}

/// How long `Retry-After` says to wait: seconds, or an HTTP date less
/// the response's own `Date`.
fn retry_after_header(response: &Response) -> Option<u64> {
    let value = header(response, "retry-after")?;
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let (at, now) = (http_date(value)?, http_date(header(response, "date")?)?);
    Some(at.saturating_sub(now))
}

/// Seconds until a reset the forge gives as a Unix time, counted from
/// the response's `Date` (the sandbox's clock isn't asked).
fn until_reset(response: &Response, reset_header: &str) -> Option<u64> {
    let reset: u64 = header(response, reset_header)?.parse().ok()?;
    let now = http_date(header(response, "date")?)?;
    Some(reset.saturating_sub(now).max(1))
}

/// How long a forge that said to slow down waits, when it gives no time:
/// GitHub's advice for its secondary limits, a minute.
pub const DEFAULT_WAIT: u64 = 60;

/// The forge's message in a JSON error body: GitHub's `message` (and
/// its first `errors` entry), GitLab's `message` or `error` (a string,
/// a list, or fields with lists).
pub fn error_words(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let text = |v: &serde_json::Value| -> Option<String> {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(items) => {
                let parts: Vec<String> = items.iter().filter_map(|i| i.as_str().map(str::to_string).or_else(|| i.get("message").and_then(|m| m.as_str()).map(str::to_string))).collect();
                (!parts.is_empty()).then(|| parts.join("; "))
            }
            serde_json::Value::Object(fields) => {
                let parts: Vec<String> = fields.iter().map(|(k, v)| match v {
                    serde_json::Value::Array(l) => format!("{k} {}", l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
                    serde_json::Value::String(s) => format!("{k} {s}"),
                    other => format!("{k} {other}"),
                }).collect();
                (!parts.is_empty()).then(|| parts.join("; "))
            }
            _ => None,
        }
    };
    let top = v.get("message").and_then(text).or_else(|| v.get("error").and_then(text));
    let detail = v.get("errors").and_then(|e| e.as_array()).and_then(|e| e.first()).and_then(|e| e.get("message").and_then(|m| m.as_str()).map(str::to_string).or_else(|| e.as_str().map(str::to_string)));
    match (top, detail) {
        (Some(t), Some(d)) if t != d => Some(format!("{t}: {d}")),
        (t, d) => t.or(d),
    }
}

/// A response of 400 or more as an error: the status, the forge's own
/// words, and how long to wait when it was told to slow down.
pub fn api_error(host: Host, response: &Response) -> ApiError {
    let status = response.status;
    let words = error_words(&response.body);
    let message = match &words {
        Some(w) => format!("HTTP {status} ({}): {w}", crate::reason(status)),
        None => format!("HTTP {status} ({})", crate::reason(status)),
    };
    let lower = words.unwrap_or_default().to_ascii_lowercase();
    let retry_after = match host {
        // Primary limit: 403 or 429 with none remaining, until the reset.
        // Secondary: 403 or 429 with Retry-After, or saying so.
        Host::GitHub if status == 429 || status == 403 => {
            let spent = header(response, "x-ratelimit-remaining") == Some("0");
            let said = lower.contains("rate limit");
            match retry_after_header(response) {
                Some(secs) => Some(secs),
                None if spent => Some(until_reset(response, "x-ratelimit-reset").unwrap_or(DEFAULT_WAIT)),
                None if said || status == 429 => Some(DEFAULT_WAIT),
                None => None,
            }
        }
        Host::GitLab if status == 429 => Some(retry_after_header(response).or_else(|| until_reset(response, "ratelimit-reset")).unwrap_or(DEFAULT_WAIT)),
        _ => None,
    };
    ApiError { status: Some(status), message, retry_after }
}

/// `method` on `url` through `http`: the response when it's under 400,
/// else the error.
pub(crate) async fn send<H: Http>(http: &H, host: Host, method: &str, url: &str, headers: &[(&str, String)], body: Option<String>) -> Result<Response, ApiError> {
    let response = http.request(method, url, headers, body).await.map_err(ApiError::offline)?;
    if response.status >= 400 {
        return Err(api_error(host, &response));
    }
    // A write answered with a redirect wasn't done: Fenix's client, asked
    // not to follow one for a write, hands it back -- and one that did
    // would have made the PATCH a bodiless GET.
    if method != "GET" && (300..400).contains(&response.status) {
        let to = header(&response, "location").unwrap_or("elsewhere");
        return Err(ApiError::status(response.status, format!("HTTP {} ({}): sent on to {to} -- nothing was changed", response.status, crate::reason(response.status))));
    }
    Ok(response)
}

pub(crate) fn json(response: &Response) -> Result<serde_json::Value, ApiError> {
    serde_json::from_str(&response.body).map_err(|e| ApiError::odd(format!("couldn't read the answer as JSON: {e}")))
}

/// Front matter at the top of a Markdown template (`---` lines around
/// `key: value`), and the body under it.
pub fn front_matter(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.replace("\r\n", "\n");
    let Some(rest) = text.strip_prefix("---\n") else { return (Vec::new(), text) };
    let Some((head, body)) = rest.split_once("\n---\n").or_else(|| rest.strip_suffix("\n---").map(|h| (h, ""))) else { return (Vec::new(), text.clone()) };
    let fields = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches(['"', '\'']).to_string()))
        .collect();
    (fields, body.trim_start_matches('\n').to_string())
}

/// A front-matter list: `a, b`, or `[a, "b"]`.
pub fn list_field(value: &str) -> Vec<String> {
    value.trim().trim_start_matches('[').trim_end_matches(']').split(',').map(|s| s.trim().trim_matches(['"', '\'']).to_string()).filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ref_is_read_from_its_key_or_its_address() {
        let gh = |s: &str| IssueRef::parse(s, Host::GitHub, "https://github.com");
        assert_eq!(gh("tpedneault/fenix#12"), Some(IssueRef::new("tpedneault/fenix", 12)));
        assert_eq!(gh("https://github.com/tpedneault/fenix/issues/12#issuecomment-3"), Some(IssueRef::new("tpedneault/fenix", 12)));
        assert_eq!(gh("https://GitHub.com/Tpedneault/Fenix/issues/12/"), Some(IssueRef::new("Tpedneault/Fenix", 12)));
        assert_eq!(gh("https://evil.example.com/tpedneault/fenix/issues/12"), None, "another host's address");
        assert_eq!(gh("a/b/c#1"), None, "three parts aren't a repository");
        assert_eq!(gh("a/b#0"), None);
        assert_eq!(gh("a b/c#1"), None);
        assert_eq!(gh("https://github.com/o/r/pull/3"), None, "a pull request's address");

        let gl = |s: &str| IssueRef::parse(s, Host::GitLab, "https://gitlab.example.com/");
        assert_eq!(gl("group/sub/deeper/project#7"), Some(IssueRef::new("group/sub/deeper/project", 7)));
        assert_eq!(gl("https://gitlab.example.com/group/sub/project/-/issues/7"), Some(IssueRef::new("group/sub/project", 7)));
        assert_eq!(gl("https://gitlab.example.com/group/project/-/work_items/7?x=1"), Some(IssueRef::new("group/project", 7)));
        assert_eq!(gl("https://gitlab.example.com/group/project/issues/7"), Some(IssueRef::new("group/project", 7)), "the older address");
        assert_eq!(gl("https://gitlab.example.com/group/project/-/merge_requests/7"), None);
        assert_eq!(gl("project#7"), None, "a project lives under a group");
        assert!(IssueRef::new("A/B", 1).same(&IssueRef::new("a/b", 1)));
    }

    #[test]
    fn the_next_page_is_read_from_a_link_header() {
        let link = r#"<https://api.github.com/search/issues?q=x&page=2>; rel="next", <https://api.github.com/search/issues?q=x&page=34>; rel="last""#;
        assert_eq!(link_next(link).as_deref(), Some("https://api.github.com/search/issues?q=x&page=2"));
        assert_eq!(page_of(&link_next(link).unwrap()).as_deref(), Some("2"));
        assert_eq!(link_next(r#"<https://x/?page=1>; rel="prev""#), None);
    }

    #[test]
    fn an_http_date_is_seconds_since_1970() {
        assert_eq!(http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(http_date("Sat, 14 Nov 2026 10:00:00 GMT"), Some(1_794_650_400));
        assert_eq!(http_date("tomorrow"), None);
    }

    fn response(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
        Response { status, headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body: body.into() }
    }

    #[test]
    fn githubs_limits_say_how_long_to_wait() {
        let date = ("Date", "Sat, 14 Nov 2026 10:00:00 GMT");
        // Primary: none remaining, until the reset.
        let primary = response(403, &[date, ("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1794650520")], r#"{"message": "API rate limit exceeded for user ID 1."}"#);
        let e = api_error(Host::GitHub, &primary);
        assert_eq!((e.retry_after, e.message.as_str()), (Some(120), "HTTP 403 (Forbidden): API rate limit exceeded for user ID 1."));
        // Secondary: Retry-After.
        let secondary = response(429, &[date, ("retry-after", "45")], r#"{"message": "You have exceeded a secondary rate limit."}"#);
        assert_eq!(api_error(Host::GitHub, &secondary).retry_after, Some(45));
        // Secondary, saying so and nothing more: a minute.
        let said = response(403, &[], r#"{"message": "You have exceeded a secondary rate limit. Please wait a few minutes."}"#);
        assert_eq!(api_error(Host::GitHub, &said).retry_after, Some(60));
        // A plain 403 is a refusal, not a limit.
        let refused = response(403, &[("x-ratelimit-remaining", "4999")], r#"{"message": "Must have admin rights to Repository."}"#);
        assert_eq!(api_error(Host::GitHub, &refused).retry_after, None);
    }

    #[test]
    fn gitlabs_limits_say_how_long_to_wait() {
        let after = response(429, &[("Retry-After", "30"), ("RateLimit-Remaining", "0")], "Retry later");
        assert_eq!(api_error(Host::GitLab, &after).retry_after, Some(30));
        let reset = response(429, &[("Date", "Sat, 14 Nov 2026 10:00:00 GMT"), ("RateLimit-Reset", "1794650410")], "");
        assert_eq!(api_error(Host::GitLab, &reset).retry_after, Some(10));
        let date = response(429, &[("Date", "Sat, 14 Nov 2026 10:00:00 GMT"), ("Retry-After", "Sat, 14 Nov 2026 10:01:00 GMT")], "");
        assert_eq!(api_error(Host::GitLab, &date).retry_after, Some(60));
        assert_eq!(api_error(Host::GitLab, &response(403, &[], r#"{"message": "403 Forbidden"}"#)).retry_after, None);
    }

    #[test]
    fn errors_keep_the_forges_own_words() {
        assert_eq!(api_error(Host::GitLab, &response(400, &[], r#"{"message": {"title": ["can't be blank"]}}"#)).message, "HTTP 400 (Bad Request): title can't be blank");
        assert_eq!(api_error(Host::GitLab, &response(404, &[], r#"{"message": "404 Project Not Found"}"#)).message, "HTTP 404 (Not Found): 404 Project Not Found");
        assert_eq!(api_error(Host::GitLab, &response(401, &[], r#"{"error": "invalid_token"}"#)).message, "HTTP 401 (Unauthorized): invalid_token");
        assert_eq!(api_error(Host::GitHub, &response(422, &[], r#"{"message": "Validation Failed", "errors": [{"message": "labels is invalid"}]}"#)).message, "HTTP 422 (Unprocessable Entity): Validation Failed: labels is invalid");
        assert_eq!(api_error(Host::GitHub, &response(502, &[], "<html>")).message, "HTTP 502 (Bad Gateway)");
    }

    #[test]
    fn a_templates_front_matter_is_read_and_the_body_kept() {
        let (fields, body) = front_matter("---\nname: Bug report\ntitle: \"[Bug] \"\nlabels: bug, triage\nassignees: ''\n---\n\n## What happened\n");
        assert_eq!(fields[0], ("name".to_string(), "Bug report".to_string()));
        assert_eq!(fields[1].1, "[Bug] ");
        assert_eq!(list_field(&fields[2].1), ["bug", "triage"]);
        assert!(list_field(&fields[3].1).is_empty());
        assert_eq!(body, "## What happened\n");
        assert_eq!(list_field("[\"a\", 'b']"), ["a", "b"]);
        assert_eq!(front_matter("no front matter").1, "no front matter");
    }
}
