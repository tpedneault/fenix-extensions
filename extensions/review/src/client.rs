//! A forge client for a repository: GitHub's, by its `origin`, else
//! GitLab's -- talking HTTP through Fenix (`run::fetch`).

use fenix_extension_api::{self as api, host, run, Command, HttpRequest};
use fenix_forge::github::GitHub;
use fenix_forge::gitlab::GitLab;
use fenix_forge::{Approvals, ChangedFile, Check, Discussion, DraftComment, Forge, Http, MergeOptions, MergeRequest, MrFilter, NewRequest, Position, Response, Verdict};

use crate::proc;

/// Fenix's HTTP client.
pub struct Fetch;

impl Http for Fetch {
    async fn request(&self, method: &str, url: &str, headers: &[(&str, String)], body: Option<String>) -> Result<Response, String> {
        let req = HttpRequest { method: method.to_string(), url: url.to_string(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), body: body.map(String::into_bytes).unwrap_or_default(), options: Default::default() };
        let r = run::fetch(req).await?;
        Ok(Response { status: r.status, headers: r.headers, body: String::from_utf8_lossy(&r.body).into_owned() })
    }
}

/// A repository's forge.
pub enum Client {
    GitHub(GitHub<Fetch>),
    GitLab(GitLab<Fetch>),
}

impl Client {
    /// "GitHub" or "GitLab", for a heading.
    pub fn name(&self) -> &'static str {
        match self {
            Client::GitHub(_) => "GitHub",
            Client::GitLab(_) => "GitLab",
        }
    }
}

/// The GitHub CLI's token, when it's signed in.
async fn gh_token() -> Option<String> {
    let out = proc::output(Command::new("gh").args(["auth", "token"])).await.ok().filter(|o| o.ok)?;
    Some(out.stdout.trim().to_string()).filter(|t| !t.is_empty())
}

fn setting(key: &str) -> Option<String> {
    api::setting(key).filter(|v| !v.trim().is_empty())
}

/// The client for the repository at `cwd` (where `git` runs for it), or
/// the reason there isn't one: an `origin` that names a project, and the
/// token for its forge.
pub async fn for_repo(cwd: &str) -> Result<Client, String> {
    let url = proc::git(cwd, &["remote", "get-url", "origin"]).await.map_err(|_| "this repo has no `origin` remote to work out the project from".to_string())?;
    let url = url.trim().to_string();
    // GitHub by its host; anything else is taken to be GitLab, which is
    // routinely self-hosted under any name.
    if fenix_forge::github::repository(&url).is_some() {
        let token = match host::secret("github.token").filter(|t| !t.trim().is_empty()) {
            Some(token) => token,
            None => gh_token().await.ok_or("sign the GitHub CLI in (gh auth login), or set the GitHub token in SPC , (Forges)")?,
        };
        return GitHub::from_remote(Fetch, token, &url).map(Client::GitHub).ok_or_else(|| format!("couldn't read owner/repo out of origin's URL ({url})"));
    }
    let base_url = setting("gitlab.base_url").ok_or("set the GitLab server in SPC , (Forges) first")?;
    let token = host::secret("gitlab.token").filter(|t| !t.trim().is_empty()).ok_or("set the GitLab token in SPC , (Forges) first -- a personal access token with the api scope")?;
    GitLab::from_remote(Fetch, base_url, token, &url).map(Client::GitLab).ok_or_else(|| format!("couldn't read a GitLab project path out of origin's URL ({url})"))
}

/// Every call, to whichever forge it is.
macro_rules! both {
    ($self:ident, $c:ident => $e:expr) => {
        match $self {
            Client::GitHub($c) => $e,
            Client::GitLab($c) => $e,
        }
    };
}

impl Forge for Client {
    fn project(&self) -> &str {
        both!(self, c => c.project())
    }
    async fn list_merge_requests(&self, filter: MrFilter) -> Result<Vec<MergeRequest>, String> {
        both!(self, c => c.list_merge_requests(filter).await)
    }
    async fn merge_request(&self, number: u64) -> Result<MergeRequest, String> {
        both!(self, c => c.merge_request(number).await)
    }
    async fn approvals(&self, number: u64) -> Result<Approvals, String> {
        both!(self, c => c.approvals(number).await)
    }
    async fn changed_files(&self, number: u64) -> Result<Vec<ChangedFile>, String> {
        both!(self, c => c.changed_files(number).await)
    }
    fn checkout_refspec(&self, number: u64) -> String {
        both!(self, c => c.checkout_refspec(number))
    }
    async fn discussions(&self, number: u64) -> Result<Vec<Discussion>, String> {
        both!(self, c => c.discussions(number).await)
    }
    async fn reply(&self, number: u64, discussion: &str, body: &str) -> Result<(), String> {
        both!(self, c => c.reply(number, discussion, body).await)
    }
    async fn resolve(&self, number: u64, discussion: &str, resolved: bool) -> Result<(), String> {
        both!(self, c => c.resolve(number, discussion, resolved).await)
    }
    async fn comment_on_line(&self, number: u64, position: &Position, body: &str) -> Result<(), String> {
        both!(self, c => c.comment_on_line(number, position, body).await)
    }
    async fn approve(&self, number: u64, sha: Option<&str>) -> Result<(), String> {
        both!(self, c => c.approve(number, sha).await)
    }
    async fn unapprove(&self, number: u64) -> Result<(), String> {
        both!(self, c => c.unapprove(number).await)
    }
    async fn merge(&self, number: u64, options: &MergeOptions) -> Result<(), String> {
        both!(self, c => c.merge(number, options).await)
    }
    async fn current_user(&self) -> Result<String, String> {
        both!(self, c => c.current_user().await)
    }
    async fn request_for_branch(&self, branch: &str) -> Result<Option<MergeRequest>, String> {
        both!(self, c => c.request_for_branch(branch).await)
    }
    async fn create_request(&self, request: &NewRequest) -> Result<MergeRequest, String> {
        both!(self, c => c.create_request(request).await)
    }
    async fn default_description(&self) -> Result<Option<String>, String> {
        both!(self, c => c.default_description().await)
    }
    async fn submit_review(&self, number: u64, head_sha: &str, verdict: Verdict, body: &str, comments: &[DraftComment]) -> Result<(), String> {
        both!(self, c => c.submit_review(number, head_sha, verdict, body, comments).await)
    }
    async fn request_review(&self, number: u64, users: &[String]) -> Result<(), String> {
        both!(self, c => c.request_review(number, users).await)
    }
    async fn checks(&self, number: u64, head_sha: &str) -> Result<Vec<Check>, String> {
        both!(self, c => c.checks(number, head_sha).await)
    }
    async fn job_log(&self, check: &Check) -> Result<String, String> {
        both!(self, c => c.job_log(check).await)
    }
    async fn retry(&self, check: &Check) -> Result<(), String> {
        both!(self, c => c.retry(check).await)
    }
}
