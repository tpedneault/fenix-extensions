//! GitHub's issues, over its REST API: github.com, or a GitHub
//! Enterprise Server at `https://<host>/api/v3`.
//!
//! The paths and fields are GitHub's REST API description
//! (`github/rest-api-description`, `api.github.com.json` 1.1.4): issues
//! and their comments, timeline, labels and assignees, the issue search,
//! and a repository's contents for its issue templates. GitHub counts a
//! pull request as an issue in every one of these; it's left out here
//! wherever one comes back.

use serde_json::{json, Value};

use super::{api_error, front_matter, header, json, link_next, list_field, page_of, send, ApiError, Change, Host, Issue, IssueRef, Issues, Label, Linked, Listing, NewIssue, Note, Paging, Search, Template, User};
use crate::gitlab::remote::encode_project as encode;
use crate::{Http, Response};

/// How many templates are read, at most.
const TEMPLATES: usize = 10;

pub struct GitHubIssues<H> {
    http: H,
    /// `https://api.github.com`, or `https://<host>/api/v3`.
    api: String,
    /// `https://github.com`, or `https://<host>`.
    web: String,
    token: String,
}

/// The host a setting names: `github.com`, `ghe.example.com`, or a URL
/// of it; none or empty is github.com.
pub fn server_host(server: Option<&str>) -> String {
    let s = server.unwrap_or_default().trim();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let s = s.split('/').next().unwrap_or_default().trim();
    if s.is_empty() || s.eq_ignore_ascii_case("api.github.com") {
        "github.com".into()
    } else {
        s.to_ascii_lowercase()
    }
}

impl<H: Http> GitHubIssues<H> {
    /// A client for github.com, or the Enterprise Server `server` names
    /// (a host, or its address).
    pub fn new(http: H, server: Option<&str>, token: impl Into<String>) -> Self {
        let host = server_host(server);
        let (api, web) = if host == "github.com" { ("https://api.github.com".to_string(), "https://github.com".to_string()) } else { (format!("https://{host}/api/v3"), format!("https://{host}")) };
        GitHubIssues { http, api, web, token: token.into() }
    }

    /// Where its API is.
    pub fn api(&self) -> &str {
        &self.api
    }

    fn headers(&self, accept: &str) -> Vec<(&'static str, String)> {
        vec![
            ("Authorization", format!("Bearer {}", self.token)),
            ("Accept", accept.to_string()),
            ("X-GitHub-Api-Version", "2022-11-28".to_string()),
            ("User-Agent", "fenix".to_string()),
        ]
    }

    async fn request(&self, method: &str, path: &str, query: &[(&str, &str)], body: Option<Value>, accept: &str) -> Result<Response, ApiError> {
        let url = crate::with_query(&format!("{}{path}", self.api), query);
        let mut headers = self.headers(accept);
        if body.is_some() {
            headers.push(("Content-Type", "application/json".to_string()));
        }
        send(&self.http, Host::GitHub, method, &url, &headers, body.map(|b| b.to_string())).await
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Response, ApiError> {
        self.request("GET", path, query, None, "application/vnd.github+json").await
    }

    async fn write(&self, method: &str, path: &str, body: Value) -> Result<Response, ApiError> {
        self.request(method, path, &[], Some(body), "application/vnd.github+json").await
    }

    fn repo(project: &str) -> Result<String, ApiError> {
        IssueRef::checked(project, "1", Host::GitHub).map(|r| format!("/repos/{}", r.project)).ok_or_else(|| ApiError::status(400, format!("\"{project}\" isn't a repository, owner/repo")))
    }

    fn issue_path(key: &IssueRef, suffix: &str) -> Result<String, ApiError> {
        Ok(format!("{}/issues/{}{suffix}", Self::repo(&key.project)?, key.number))
    }

    /// Issues in a listing, pull requests left out, and its next page.
    fn listing(&self, response: &Response, items: Option<&Value>) -> Result<Listing, ApiError> {
        let items = items.and_then(Value::as_array).ok_or_else(|| ApiError::odd("GitHub's answer had no list of issues"))?;
        let issues = items.iter().filter(|i| i.get("pull_request").is_none()).filter_map(|i| parse_issue(i, &self.api, &self.web)).collect();
        let next = header(response, "link").and_then(link_next).and_then(|u| page_of(&u));
        Ok(Listing { issues, next })
    }

    async fn search_query(&self, q: &str, paging: &Paging) -> Result<Listing, ApiError> {
        let (per_page, page) = (paging.per_page().to_string(), paging.page().to_string());
        let response = self.get("/search/issues", &[("q", q), ("sort", "updated"), ("order", "desc"), ("per_page", &per_page), ("page", &page)]).await?;
        let value = json(&response)?;
        self.listing(&response, value.get("items"))
    }
}

/// `owner/repo` of a repository's API address, `<api>/repos/owner/repo`.
fn repo_of(url: &str, api: &str) -> Option<String> {
    let rest = url.strip_prefix(api)?.strip_prefix("/repos/")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn user(v: &Value) -> User {
    let login = text(v, "login");
    let name = v.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| login.clone());
    User { username: login, name, id: String::new() }
}

/// A label, given as its name or as an object with one.
fn label_name(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).or_else(|| v.get("name").and_then(Value::as_str).map(str::to_string))
}

/// An issue from GitHub's JSON; none for one with no repository or
/// number. Its repository is read from its API address, as GitHub cases
/// it.
pub fn parse_issue(v: &Value, api: &str, web: &str) -> Option<Issue> {
    let number = v.get("number")?.as_u64()?;
    let html = text(v, "html_url");
    let project = v.get("repository_url").and_then(Value::as_str).and_then(|u| repo_of(u, api)).or_else(|| IssueRef::parse(&html, Host::GitHub, web).map(|r| r.project))?;
    Some(Issue {
        key: IssueRef::new(project, number),
        id: v.get("node_id").and_then(Value::as_str).map(str::to_string).or_else(|| v.get("id").and_then(Value::as_u64).map(|i| i.to_string())).unwrap_or_default(),
        title: text(v, "title"),
        body: text(v, "body"),
        open: text(v, "state") != "closed",
        state_reason: v.get("state_reason").and_then(Value::as_str).map(str::to_string),
        author: v.get("user").map(|u| text(u, "login")).unwrap_or_default(),
        assignees: v.get("assignees").and_then(Value::as_array).map(|l| l.iter().map(user).collect()).unwrap_or_default(),
        labels: v.get("labels").and_then(Value::as_array).map(|l| l.iter().filter_map(label_name).collect()).unwrap_or_default(),
        milestone: v.get("milestone").and_then(|m| m.get("title")).and_then(Value::as_str).map(str::to_string),
        due: None,
        weight: None,
        estimate: None,
        spent: None,
        url: html,
        created: text(v, "created_at"),
        updated: text(v, "updated_at"),
        comments: v.get("comments").and_then(Value::as_u64).unwrap_or(0),
        confidential: false,
    })
}

fn parse_note(v: &Value) -> Note {
    Note {
        id: v.get("id").map(|i| i.to_string()).unwrap_or_default(),
        author: v.get("user").map(|u| text(u, "login")).unwrap_or_default(),
        body: text(v, "body"),
        created: text(v, "created_at"),
        system: false,
    }
}

/// A `since` safe to put in a search: a date and time, nothing more.
fn since_ok(since: &str) -> bool {
    !since.is_empty() && since.len() <= 40 && since.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'T' | b'Z' | b'+' | b'.'))
}

/// A page given as `rel="last"`.
fn last_page(response: &Response) -> Option<u32> {
    let link = header(response, "link")?;
    link.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        params.contains("rel=\"last\"").then(|| page_of(url.trim().trim_start_matches('<').trim_end_matches('>')))?
    }).and_then(|p| p.parse().ok())
}

impl<H: Http> Issues for GitHubIssues<H> {
    fn host(&self) -> Host {
        Host::GitHub
    }

    fn web(&self) -> &str {
        &self.web
    }

    async fn me(&self) -> Result<User, ApiError> {
        let value = json(&self.get("/user", &[]).await?)?;
        let me = user(&value);
        if me.username.is_empty() {
            return Err(ApiError::odd("GitHub didn't say who the token belongs to"));
        }
        Ok(me)
    }

    async fn search(&self, search: &Search, paging: &Paging) -> Result<Listing, ApiError> {
        let since = paging.since.as_deref().filter(|s| since_ok(s)).map(|s| format!(" updated:>={s}")).unwrap_or_default();
        match search {
            Search::Assigned => self.search_query(&format!("is:issue is:open assignee:@me archived:false{since}"), paging).await,
            Search::Created => self.search_query(&format!("is:issue is:open author:@me archived:false{since}"), paging).await,
            Search::Mentioned => self.search_query(&format!("is:issue is:open mentions:@me archived:false{since}"), paging).await,
            Search::Query(q) => {
                let lower = q.to_ascii_lowercase();
                let says = ["is:issue", "type:issue", "is:pr", "type:pr", "is:pull-request"].iter().any(|w| lower.contains(w));
                let q = if says { q.trim().to_string() } else { format!("is:issue {}", q.trim()) };
                self.search_query(&q, paging).await
            }
            Search::Project(project) => {
                let (per_page, page) = (paging.per_page().to_string(), paging.page().to_string());
                let mut query = vec![("state", "open"), ("sort", "updated"), ("direction", "desc"), ("per_page", per_page.as_str()), ("page", page.as_str())];
                if let Some(s) = paging.since.as_deref().filter(|s| since_ok(s)) {
                    query.push(("since", s));
                }
                let response = self.get(&format!("{}/issues", Self::repo(project)?), &query).await?;
                let value = json(&response)?;
                self.listing(&response, Some(&value))
            }
        }
    }

    async fn issue(&self, key: &IssueRef) -> Result<Issue, ApiError> {
        let value = json(&self.get(&Self::issue_path(key, "")?, &[]).await?)?;
        if value.get("pull_request").is_some() {
            return Err(ApiError::status(404, format!("{key} is a pull request, not an issue")));
        }
        parse_issue(&value, &self.api, &self.web).ok_or_else(|| ApiError::odd(format!("GitHub's answer for {key} had no number")))
    }

    async fn notes(&self, key: &IssueRef) -> Result<Vec<Note>, ApiError> {
        let path = Self::issue_path(key, "/comments")?;
        let first = self.get(&path, &[("per_page", "100"), ("page", "1")]).await?;
        let mut pages = vec![json(&first)?];
        // Oldest first, a hundred a page: the most recent are the last
        // page and the one before it.
        if let Some(last) = last_page(&first).filter(|l| *l > 1) {
            pages.clear();
            for page in [last - 1, last] {
                let page = page.to_string();
                pages.push(json(&self.get(&path, &[("per_page", "100"), ("page", &page)]).await?)?);
            }
        }
        let mut notes: Vec<Note> = pages.iter().filter_map(Value::as_array).flatten().map(parse_note).collect();
        if notes.len() > 100 {
            notes.drain(..notes.len() - 100);
        }
        Ok(notes)
    }

    async fn linked(&self, key: &IssueRef) -> Result<Vec<Linked>, ApiError> {
        let value = json(&self.get(&Self::issue_path(key, "/timeline")?, &[("per_page", "100")]).await?)?;
        let mut out: Vec<Linked> = Vec::new();
        for event in value.as_array().into_iter().flatten().filter(|e| text(e, "event") == "cross-referenced") {
            let Some(source) = event.get("source").and_then(|s| s.get("issue")) else { continue };
            let Some(pull) = source.get("pull_request") else { continue };
            let Some(issue) = parse_issue(source, &self.api, &self.web) else { continue };
            let merged = pull.get("merged_at").is_some_and(|m| !m.is_null());
            let reference = issue.key.key();
            if out.iter().any(|l| l.reference == reference) {
                continue;
            }
            out.push(Linked { reference, title: issue.title, url: issue.url, state: if merged { "merged".into() } else if issue.open { "open".into() } else { "closed".into() } });
        }
        Ok(out)
    }

    async fn create(&self, new: &NewIssue) -> Result<Issue, ApiError> {
        let mut body = json!({ "title": new.title, "body": new.body });
        if !new.labels.is_empty() {
            body["labels"] = json!(new.labels);
        }
        if !new.assignees.is_empty() {
            body["assignees"] = json!(new.assignees);
        }
        let value = json(&self.write("POST", &format!("{}/issues", Self::repo(&new.project)?), body).await?)?;
        parse_issue(&value, &self.api, &self.web).ok_or_else(|| ApiError::odd("GitHub didn't say what it made"))
    }

    async fn change(&self, key: &IssueRef, change: &Change) -> Result<(), ApiError> {
        let path = Self::issue_path(key, "")?;
        let patch = |body: Value| self.write("PATCH", &path, body);
        match change {
            Change::Title(t) => patch(json!({ "title": t })).await?,
            Change::Body(b) => patch(json!({ "body": b })).await?,
            Change::Labels(l) => patch(json!({ "labels": l })).await?,
            Change::Assignees(a) => patch(json!({ "assignees": a })).await?,
            Change::Close(reason) => patch(json!({ "state": "closed", "state_reason": reason.as_deref().unwrap_or("completed") })).await?,
            Change::Reopen => patch(json!({ "state": "open", "state_reason": "reopened" })).await?,
            Change::AddLabel(l) => self.write("POST", &Self::issue_path(key, "/labels")?, json!({ "labels": [l] })).await?,
            Change::RemoveLabel(l) => match self.request("DELETE", &Self::issue_path(key, &format!("/labels/{}", encode(l)))?, &[], None, "application/vnd.github+json").await {
                // Not on it: nothing to take off.
                Err(e) if e.status == Some(404) => return Ok(()),
                other => other?,
            },
            Change::Due(_) => return Err(ApiError::status(422, "GitHub issues have no due date")),
        };
        Ok(())
    }

    async fn comment(&self, key: &IssueRef, body: &str) -> Result<(), ApiError> {
        self.write("POST", &Self::issue_path(key, "/comments")?, json!({ "body": body })).await.map(|_| ())
    }

    async fn labels(&self, project: &str) -> Result<Vec<Label>, ApiError> {
        let value = json(&self.get(&format!("{}/labels", Self::repo(project)?), &[("per_page", "100")]).await?)?;
        Ok(value.as_array().into_iter().flatten().map(|l| Label { name: text(l, "name"), description: text(l, "description") }).filter(|l| !l.name.is_empty()).collect())
    }

    async fn assignable(&self, project: &str) -> Result<Vec<User>, ApiError> {
        let value = json(&self.get(&format!("{}/assignees", Self::repo(project)?), &[("per_page", "100")]).await?)?;
        Ok(value.as_array().into_iter().flatten().map(user).filter(|u| !u.username.is_empty()).collect())
    }

    async fn resolve(&self, id: &str) -> Result<IssueRef, ApiError> {
        // GraphQL is `/graphql` on github.com, `/api/graphql` on a server.
        let url = match self.api.strip_suffix("/api/v3") {
            Some(host) => format!("{host}/api/graphql"),
            None => format!("{}/graphql", self.api),
        };
        let query = "query($id:ID!){node(id:$id){... on Issue{number repository{nameWithOwner}}}}";
        let body = json!({ "query": query, "variables": { "id": id } });
        let mut headers = self.headers("application/vnd.github+json");
        headers.push(("Content-Type", "application/json".to_string()));
        let value = json(&send(&self.http, Host::GitHub, "POST", &url, &headers, Some(body.to_string())).await?)?;
        let node = value.get("data").and_then(|d| d.get("node")).filter(|n| !n.is_null());
        let found = node.and_then(|n| Some((n.get("repository")?.get("nameWithOwner")?.as_str()?.to_string(), n.get("number")?.as_u64()?)));
        match found {
            Some((project, number)) => Ok(IssueRef::new(project, number)),
            None => Err(ApiError::status(404, format!("no issue with the id {id}"))),
        }
    }

    async fn templates(&self, project: &str) -> Result<Vec<Template>, ApiError> {
        let dir = format!("{}/contents/.github/ISSUE_TEMPLATE", Self::repo(project)?);
        let listing = match self.get(&dir, &[]).await {
            Ok(r) => json(&r)?,
            Err(e) if e.status == Some(404) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        let files = listing.as_array().into_iter().flatten().filter(|f| text(f, "type") == "file" && text(f, "name").to_ascii_lowercase().ends_with(".md"));
        for file in files.take(TEMPLATES) {
            let name = text(file, "name");
            // Its text as it is, rather than base64 in JSON.
            let raw = self.request("GET", &format!("{dir}/{}", encode(&name)), &[], None, "application/vnd.github.raw+json").await?;
            let (fields, body) = front_matter(&raw.body);
            let field = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).unwrap_or_default();
            out.push(Template {
                name: Some(field("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| name.trim_end_matches(".md").to_string()),
                title: field("title"),
                body,
                labels: list_field(&field("labels")),
                assignees: list_field(&field("assignees")),
            });
        }
        Ok(out)
    }
}

/// What a failed request said, the forge's own words, for a caller that
/// has a response of its own.
pub fn error_of(response: &Response) -> ApiError {
    api_error(Host::GitHub, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_github_com_unless_it_names_an_enterprise_one() {
        for s in [None, Some(""), Some("github.com"), Some("https://github.com/"), Some("api.github.com")] {
            let c = GitHubIssues::new(crate::NoHttp, s, "t");
            assert_eq!((c.api(), c.web()), ("https://api.github.com", "https://github.com"), "{s:?}");
        }
        let c = GitHubIssues::new(crate::NoHttp, Some("https://GHE.example.com/some/where"), "t");
        assert_eq!((c.api(), c.web()), ("https://ghe.example.com/api/v3", "https://ghe.example.com"));
    }

    #[test]
    fn an_issue_is_read_with_its_repository_as_github_cases_it() {
        let v = json!({"number": 3, "repository_url": "https://ghe.example.com/api/v3/repos/Acme/Rocket", "html_url": "https://ghe.example.com/Acme/Rocket/issues/3", "title": "T", "state": "closed", "state_reason": "not_planned", "labels": ["a", {"name": "b"}], "assignees": [{"login": "jo"}]});
        let i = parse_issue(&v, "https://ghe.example.com/api/v3", "https://ghe.example.com").unwrap();
        assert_eq!((i.key.key(), i.open, i.state_label()), ("Acme/Rocket#3".to_string(), false, "closed as not planned".to_string()));
        assert_eq!(i.labels, ["a", "b"]);
        assert_eq!(i.assignees, [User::named("jo")]);
    }

    #[test]
    fn a_since_that_isnt_a_time_stays_out_of_the_query() {
        assert!(since_ok("2026-11-14T09:30:00Z") && since_ok("2026-11-14T09:30:00.000+01:00"));
        assert!(!since_ok("2026 OR repo:x/y") && !since_ok(""));
    }
}
