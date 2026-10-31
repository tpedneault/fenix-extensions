//! `fenix-forge`'s `Forge` trait, over GitHub's REST API -- and its
//! GraphQL API for the two things REST can't do: reading review threads
//! with their resolved state, and resolving them.
//!
//! Shaped like `gitlab`: one authenticated-request helper per verb,
//! through [`Http`], raw `serde_json::Value` read by `parse`. Which
//! repository is read from `origin`; the token is the caller's (the
//! review extension asks the GitHub CLI, else `github.token`).

pub mod parse;

use crate::{Approvals, ChangedFile, Check, Discussion, DraftComment, Forge, MergeOptions, MergeRequest, MrFilter, NewRequest, Position, Verdict};
use crate::{Http, Response};
use serde_json::{json, Value};

const API: &str = "https://api.github.com";
const PER_PAGE: usize = 100;

pub struct GitHub<H> {
    http: H,
    api: String,
    token: String,
    owner: String,
    repo: String,
    /// `owner/repo`, for a heading.
    full: String,
}

/// `owner/repo` from a GitHub remote URL -- SSH, `ssh://` or HTTPS --
/// or `None` when it isn't one.
pub fn repository(remote_url: &str) -> Option<(String, String)> {
    let url = remote_url.trim();
    let rest = url.split_once("github.com")?.1.trim_start_matches([':', '/']);
    let path = rest.trim_end_matches('/').trim_end_matches(".git");
    let (owner, repo) = path.split_once('/')?;
    (!owner.is_empty() && !repo.is_empty() && !repo.contains('/')).then(|| (owner.to_string(), repo.to_string()))
}

impl<H: Http> GitHub<H> {
    pub fn new(http: H, token: impl Into<String>, owner: impl Into<String>, repo: impl Into<String>) -> Self {
        let (owner, repo) = (owner.into(), repo.into());
        GitHub { http, api: API.to_string(), token: token.into(), full: format!("{owner}/{repo}"), owner, repo }
    }

    /// A client for the repository `remote_url` names.
    pub fn from_remote(http: H, token: impl Into<String>, remote_url: &str) -> Option<Self> {
        repository(remote_url).map(|(owner, repo)| GitHub::new(http, token, owner, repo))
    }

    /// `method` on `url`, signed, with `body`: what came back, or the
    /// status and GitHub's own message.
    async fn request(&self, method: &str, url: &str, body: Option<String>) -> Result<String, String> {
        let headers = [
            ("Authorization", format!("Bearer {}", self.token)),
            ("Accept", "application/vnd.github+json".to_string()),
            ("X-GitHub-Api-Version", "2022-11-28".to_string()),
            ("User-Agent", "fenix".to_string()),
        ];
        let response = self.http.request(method, url, &headers, body).await?;
        if response.status >= 400 {
            return Err(describe_error(&response));
        }
        Ok(response.body)
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        let url = crate::with_query(&format!("{}{path}", self.api), query);
        let text = self.request("GET", &url, None).await?;
        serde_json::from_str(&text).map_err(|e| format!("couldn't parse response as JSON: {e}"))
    }

    /// Every page of a listing, up to a sane limit.
    async fn get_all(&self, path: &str, query: &[(&str, &str)]) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for page in 1..=30 {
            let page = page.to_string();
            let per_page = PER_PAGE.to_string();
            let mut q: Vec<(&str, &str)> = query.to_vec();
            q.push(("per_page", &per_page));
            q.push(("page", &page));
            let value = self.get(path, &q).await?;
            let items = value.as_array().cloned().unwrap_or_default();
            let done = items.len() < PER_PAGE;
            out.extend(items);
            if done {
                break;
            }
        }
        Ok(out)
    }

    async fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, String> {
        let text = self.request(method, &format!("{}{path}", self.api), Some(body.to_string())).await?;
        // 204 No Content and friends: fine.
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn graphql(&self, query: &str, variables: Value) -> Result<Value, String> {
        let value = self.send("POST", "/graphql", &json!({ "query": query, "variables": variables })).await?;
        if let Some(errors) = value.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
            let messages: Vec<&str> = errors.iter().filter_map(|e| e.get("message").and_then(Value::as_str)).collect();
            return Err(format!("GitHub said: {}", messages.join("; ")));
        }
        Ok(value.get("data").cloned().unwrap_or(Value::Null))
    }

    fn repo_path(&self, suffix: &str) -> String {
        format!("/repos/{}/{}{suffix}", self.owner, self.repo)
    }

    fn pull_path(&self, number: u64, suffix: &str) -> String {
        self.repo_path(&format!("/pulls/{number}{suffix}"))
    }

    async fn node_id(&self, number: u64) -> Result<String, String> {
        let pull = self.get(&self.pull_path(number, ""), &[]).await?;
        pull.get("node_id").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "GitHub gave the pull request no node id".to_string())
    }
}

impl<H: Http> Forge for GitHub<H> {
    fn project(&self) -> &str {
        &self.full
    }

    async fn list_merge_requests(&self, filter: MrFilter) -> Result<Vec<MergeRequest>, String> {
        let pulls = self.get_all(&self.repo_path("/pulls"), &[("state", "open"), ("sort", "updated"), ("direction", "desc")]).await?;
        let me = match filter {
            MrFilter::AllOpen => String::new(),
            _ => self.current_user().await?,
        };
        Ok(pulls
            .iter()
            .filter(|p| match filter {
                MrFilter::AllOpen => true,
                MrFilter::Mine => p.get("user").and_then(|u| u.get("login")).and_then(Value::as_str) == Some(me.as_str()),
                MrFilter::ReviewRequested => parse::requested_reviewers(p).contains(&me),
                MrFilter::ForMe => parse::requested_reviewers(p).contains(&me) || parse::assignees(p).contains(&me),
            })
            .filter_map(parse::pull_request)
            .collect())
    }

    async fn merge_request(&self, number: u64) -> Result<MergeRequest, String> {
        let value = self.get(&self.pull_path(number, ""), &[]).await?;
        let mut pr = parse::pull_request(&value).ok_or_else(|| format!("#{number} came back without a number"))?;
        if let Ok(checks) = self.checks(number, &pr.sha.clone()).await {
            pr.pipeline = parse::overall(&checks);
        }
        Ok(pr)
    }

    async fn approvals(&self, number: u64) -> Result<Approvals, String> {
        let reviews = self.get_all(&self.pull_path(number, "/reviews"), &[]).await?;
        Ok(parse::approvals(&Value::Array(reviews)))
    }

    async fn changed_files(&self, number: u64) -> Result<Vec<ChangedFile>, String> {
        Ok(self.get_all(&self.pull_path(number, "/files"), &[]).await?.iter().filter_map(parse::changed_file).collect())
    }

    fn checkout_refspec(&self, number: u64) -> String {
        // GitHub publishes every pull request's head here, forks included.
        format!("refs/pull/{number}/head:mr-{number}")
    }

    async fn discussions(&self, number: u64) -> Result<Vec<Discussion>, String> {
        const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){\
            reviewThreads(first:100){nodes{id isResolved isOutdated path line originalLine diffSide \
            comments(first:100){nodes{databaseId author{login} body createdAt}}}} \
            comments(first:100){nodes{databaseId author{login} body createdAt}}}}}";
        let data = self.graphql(QUERY, json!({ "owner": self.owner, "name": self.repo, "number": number })).await?;
        let pull = data.get("repository").and_then(|r| r.get("pullRequest")).ok_or_else(|| format!("no pull request #{number}"))?;
        Ok(parse::discussions(pull))
    }

    async fn reply(&self, number: u64, discussion: &str, body: &str) -> Result<(), String> {
        // The conversation under the description isn't a thread: a reply
        // there is one more comment on it.
        if discussion.starts_with("issue-") {
            return self.send("POST", &self.repo_path(&format!("/issues/{number}/comments")), &json!({ "body": body })).await.map(|_| ());
        }
        const MUTATION: &str = "mutation($id:ID!,$body:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id,body:$body}){comment{id}}}";
        self.graphql(MUTATION, json!({ "id": discussion, "body": body })).await.map(|_| ())
    }

    async fn resolve(&self, _number: u64, discussion: &str, resolved: bool) -> Result<(), String> {
        let mutation = if resolved {
            "mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{isResolved}}}"
        } else {
            "mutation($id:ID!){unresolveReviewThread(input:{threadId:$id}){thread{isResolved}}}"
        };
        self.graphql(mutation, json!({ "id": discussion })).await.map(|_| ())
    }

    async fn comment_on_line(&self, number: u64, position: &Position, body: &str) -> Result<(), String> {
        let (path, line, side) = anchor(position);
        let head = if position.head_sha.is_empty() { self.merge_request(number).await?.sha } else { position.head_sha.clone() };
        let payload = json!({ "body": body, "commit_id": head, "path": path, "line": line, "side": side });
        self.send("POST", &self.pull_path(number, "/comments"), &payload).await.map(|_| ())
    }

    async fn approve(&self, number: u64, sha: Option<&str>) -> Result<(), String> {
        let mut body = json!({ "event": "APPROVE" });
        if let Some(sha) = sha {
            body["commit_id"] = json!(sha);
        }
        self.send("POST", &self.pull_path(number, "/reviews"), &body).await.map(|_| ())
    }

    async fn unapprove(&self, _number: u64) -> Result<(), String> {
        Err("GitHub doesn't let a reviewer withdraw an approval -- submit a review that requests changes instead".to_string())
    }

    async fn merge(&self, number: u64, options: &MergeOptions) -> Result<(), String> {
        let method = if options.squash {
            "squash"
        } else if options.rebase {
            "rebase"
        } else {
            "merge"
        };
        if options.when_checks_pass {
            const MUTATION: &str = "mutation($id:ID!,$method:PullRequestMergeMethod!){enablePullRequestAutoMerge(input:{pullRequestId:$id,mergeMethod:$method}){pullRequest{number}}}";
            let id = self.node_id(number).await?;
            return self.graphql(MUTATION, json!({ "id": id, "method": method.to_uppercase() })).await.map(|_| ());
        }
        let pull = self.get(&self.pull_path(number, ""), &[]).await?;
        let mut body = json!({ "merge_method": method });
        if let Some(sha) = &options.sha {
            body["sha"] = json!(sha);
        }
        self.send("PUT", &self.pull_path(number, "/merge"), &body).await?;
        if options.remove_source_branch {
            // Only a branch in this repository, never a fork's.
            let same_repo = pull.get("head").and_then(|h| h.get("repo")).and_then(|r| r.get("full_name")).and_then(Value::as_str) == Some(self.full.as_str());
            if let (true, Some(branch)) = (same_repo, pull.get("head").and_then(|h| h.get("ref")).and_then(Value::as_str)) {
                self.send("DELETE", &self.repo_path(&format!("/git/refs/heads/{branch}")), &Value::Null).await?;
            }
        }
        Ok(())
    }

    async fn current_user(&self) -> Result<String, String> {
        let user = self.get("/user", &[]).await?;
        user.get("login").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "GitHub didn't say who the token belongs to".to_string())
    }

    async fn request_for_branch(&self, branch: &str) -> Result<Option<MergeRequest>, String> {
        let head = format!("{}:{branch}", self.owner);
        let found = self.get(&self.repo_path("/pulls"), &[("state", "open"), ("head", &head)]).await?;
        match found.as_array().and_then(|l| l.first()).and_then(|p| p.get("number")).and_then(Value::as_u64) {
            Some(number) => self.merge_request(number).await.map(Some),
            None => Ok(None),
        }
    }

    async fn create_request(&self, request: &NewRequest) -> Result<MergeRequest, String> {
        let body = json!({
            "title": request.title,
            "head": request.source_branch,
            "base": request.target_branch,
            "body": request.description,
            "draft": request.draft,
        });
        let value = self.send("POST", &self.repo_path("/pulls"), &body).await?;
        let made = parse::pull_request(&value).ok_or_else(|| "GitHub didn't say what it made".to_string())?;
        // A pull request's labels are its issue's.
        if !request.labels.is_empty() {
            self.send("POST", &self.repo_path(&format!("/issues/{}/labels", made.number)), &json!({ "labels": request.labels })).await
                .map_err(|e| format!("opened {}, but the labels didn't go on: {e}", made.reference()))?;
        }
        // So are its assignees.
        if !request.assignees.is_empty() {
            self.send("POST", &self.repo_path(&format!("/issues/{}/assignees", made.number)), &json!({ "assignees": request.assignees })).await
                .map_err(|e| format!("opened {}, but assigning {} didn't work: {e}", made.reference(), request.assignees.join(", ")))?;
        }
        Ok(made)
    }

    async fn submit_review(&self, number: u64, head_sha: &str, verdict: Verdict, body: &str, comments: &[DraftComment]) -> Result<(), String> {
        let event = match verdict {
            Verdict::Approve => "APPROVE",
            Verdict::RequestChanges => "REQUEST_CHANGES",
            Verdict::Comment => "COMMENT",
        };
        // GitHub wants words with anything but an approval.
        let body = match (verdict, body.trim().is_empty(), comments.is_empty()) {
            (Verdict::Approve, _, _) | (_, false, _) => body.to_string(),
            (Verdict::RequestChanges, true, _) => "Requesting changes -- see the comments.".to_string(),
            (Verdict::Comment, true, false) => "Comments inline.".to_string(),
            (Verdict::Comment, true, true) => return Err("nothing to send: no comments and no summary".to_string()),
        };
        let comments: Vec<Value> = comments
            .iter()
            .map(|c| {
                let (path, line, side) = anchor(&c.position);
                let mut out = json!({ "path": path, "line": line, "side": side, "body": c.body });
                if let Some(start) = c.start_line.filter(|s| *s < line) {
                    out["start_line"] = json!(start);
                    out["start_side"] = json!(side);
                }
                out
            })
            .collect();
        let payload = json!({ "commit_id": head_sha, "body": body, "event": event, "comments": comments });
        self.send("POST", &self.pull_path(number, "/reviews"), &payload).await.map(|_| ())
    }

    async fn request_review(&self, number: u64, users: &[String]) -> Result<(), String> {
        self.send("POST", &self.pull_path(number, "/requested_reviewers"), &json!({ "reviewers": users })).await.map(|_| ())
    }

    async fn checks(&self, _number: u64, head_sha: &str) -> Result<Vec<Check>, String> {
        let value = self.get(&self.repo_path(&format!("/commits/{head_sha}/check-runs")), &[("per_page", "100")]).await?;
        Ok(value.get("check_runs").and_then(Value::as_array).map(|l| l.iter().filter_map(parse::check_run).collect()).unwrap_or_default())
    }

    async fn job_log(&self, check: &Check) -> Result<String, String> {
        // An Actions job's log; GitHub redirects to a signed download.
        self.request("GET", &format!("{}{}", self.api, self.repo_path(&format!("/actions/jobs/{}/logs", check.id))), None).await
    }

    async fn retry(&self, check: &Check) -> Result<(), String> {
        self.send("POST", &self.repo_path(&format!("/actions/jobs/{}/rerun", check.id)), &json!({})).await.map(|_| ())
    }
}

/// Where a comment goes, GitHub's way: the path, the line and the side
/// (`RIGHT` for the new version, `LEFT` for a removed line).
fn anchor(position: &Position) -> (String, usize, &'static str) {
    match (position.new_line, position.old_line) {
        (Some(new), _) => (position.new_path.clone(), new, "RIGHT"),
        (None, Some(old)) => (position.old_path.clone(), old, "LEFT"),
        (None, None) => (position.new_path.clone(), 1, "RIGHT"),
    }
}

/// A status and GitHub's own message ("Can not approve your own pull
/// request"), which says more than any code.
fn describe_error(response: &Response) -> String {
    let code = response.status;
    let status = crate::reason(code);
    let message = serde_json::from_str::<Value>(&response.body).ok().and_then(|v| {
        let top = v.get("message").and_then(Value::as_str).map(str::to_string);
        let detail = v.get("errors").and_then(Value::as_array).and_then(|e| e.first()).and_then(|e| e.get("message").or(Some(e))).map(|m| m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string()));
        match (top, detail) {
            (Some(t), Some(d)) => Some(format!("{t}: {d}")),
            (t, d) => t.or(d),
        }
    });
    match message {
        Some(m) => format!("HTTP {code} ({status}): {m}"),
        None => format!("HTTP {code} ({status})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_is_read_from_every_kind_of_remote() {
        let want = Some(("tpedneault".to_string(), "fenix".to_string()));
        assert_eq!(repository("https://github.com/tpedneault/fenix.git"), want);
        assert_eq!(repository("https://github.com/tpedneault/fenix"), want);
        assert_eq!(repository("git@github.com:tpedneault/fenix.git"), want);
        assert_eq!(repository("ssh://git@github.com/tpedneault/fenix.git"), want);
        assert_eq!(repository("https://gitlab.example.com/g/p.git"), None);
    }

    #[test]
    fn a_comment_anchors_on_the_new_side_unless_it_is_a_removed_line() {
        let base = Position { base_sha: String::new(), head_sha: String::new(), start_sha: String::new(), old_path: "a".into(), new_path: "b".into(), old_line: Some(3), new_line: Some(4) };
        assert_eq!(anchor(&base), ("b".to_string(), 4, "RIGHT"));
        let removed = Position { new_line: None, ..base };
        assert_eq!(anchor(&removed), ("a".to_string(), 3, "LEFT"));
    }

    #[test]
    fn the_checkout_ref_is_githubs_own() {
        assert_eq!(GitHub::new(crate::NoHttp, "t", "o", "r").checkout_refspec(7), "refs/pull/7/head:mr-7");
    }
}
