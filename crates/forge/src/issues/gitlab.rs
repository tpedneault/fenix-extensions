//! GitLab's issues, over `/api/v4`, on gitlab.com or your own instance.
//!
//! Paths and fields are GitLab's API documentation (`doc/api/issues.md`,
//! `notes.md`, `labels.md`, `project_templates.md`, `todos.md`,
//! `rest/_index.md` for paging): issues, their notes and related merge
//! requests, labels, members, issue templates, and to-do items for what
//! mentions you. A project is named by its path, encoded, so nested
//! groups (`group/sub/project`) need nothing more.

use serde_json::{json, Value};

use super::{api_error, header, json, send, ApiError, Change, Host, Issue, IssueRef, Issues, Label, Linked, Listing, NewIssue, Note, Paging, Search, Template, User};
use crate::gitlab::remote::encode_project as encode;
use crate::{Http, Response};

/// How many templates are read, at most.
const TEMPLATES: usize = 10;

pub struct GitLabIssues<H> {
    http: H,
    /// The instance's root, `https://gitlab.example.com`.
    base: String,
    token: String,
}

impl<H: Http> GitLabIssues<H> {
    /// A client for the instance at `base_url` (its root, not its API).
    pub fn new(http: H, base_url: &str, token: impl Into<String>) -> Self {
        GitLabIssues { http, base: base_url.trim().trim_end_matches('/').to_string(), token: token.into() }
    }

    async fn request(&self, method: &str, path: &str, query: &[(&str, &str)], body: Option<Value>) -> Result<Response, ApiError> {
        let url = crate::with_query(&format!("{}/api/v4{path}", self.base), query);
        let mut headers = vec![("PRIVATE-TOKEN", self.token.clone()), ("Accept", "application/json".to_string())];
        if body.is_some() {
            headers.push(("Content-Type", "application/json".to_string()));
        }
        send(&self.http, Host::GitLab, method, &url, &headers, body.map(|b| b.to_string())).await
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Response, ApiError> {
        self.request("GET", path, query, None).await
    }

    fn project(project: &str) -> Result<String, ApiError> {
        IssueRef::checked(project, "1", Host::GitLab).map(|r| format!("/projects/{}", encode(&r.project))).ok_or_else(|| ApiError::status(400, format!("\"{project}\" isn't a project path, group/project")))
    }

    fn issue_path(key: &IssueRef, suffix: &str) -> Result<String, ApiError> {
        Ok(format!("{}/issues/{}{suffix}", Self::project(&key.project)?, key.number))
    }

    fn listing(&self, response: &Response, items: impl Iterator<Item = Value>) -> Listing {
        let issues = items.filter_map(|i| parse_issue(&i, &self.base)).collect();
        let next = header(response, "x-next-page").filter(|p| !p.is_empty() && p.parse::<u32>().is_ok()).map(str::to_string);
        Listing { issues, next }
    }

    async fn issues_at(&self, path: &str, extra: &[(&str, String)], paging: &Paging) -> Result<Listing, ApiError> {
        let (per_page, page) = (paging.per_page().to_string(), paging.page().to_string());
        let mut query: Vec<(&str, &str)> = vec![("order_by", "updated_at"), ("sort", "desc"), ("per_page", &per_page), ("page", &page)];
        for (k, v) in extra {
            query.push((k, v));
        }
        if let Some(since) = paging.since.as_deref() {
            query.push(("updated_after", since));
        }
        let response = self.get(path, &query).await?;
        let value = json(&response)?;
        let items = value.as_array().cloned().ok_or_else(|| ApiError::odd("GitLab's answer had no list of issues"))?;
        Ok(self.listing(&response, items.into_iter()))
    }

    /// A username's id, which GitLab's writes take.
    async fn user_id(&self, username: &str) -> Result<u64, ApiError> {
        let value = json(&self.get("/users", &[("username", username)]).await?)?;
        value.as_array().and_then(|l| l.first()).and_then(|u| u.get("id")).and_then(Value::as_u64).ok_or_else(|| ApiError::status(404, format!("no GitLab user called {username}")))
    }

    async fn user_ids(&self, usernames: &[String]) -> Result<Vec<u64>, ApiError> {
        let mut ids = Vec::new();
        for u in usernames {
            ids.push(self.user_id(u).await?);
        }
        Ok(ids)
    }

}

/// `4500` seconds as GitLab writes a duration: `1h15m`.
pub fn duration(seconds: i64) -> String {
    let minutes = (seconds.max(60) + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn user(v: &Value) -> User {
    let username = text(v, "username");
    let name = Some(text(v, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| username.clone());
    User { username, name, id: v.get("id").and_then(Value::as_u64).map(|i| i.to_string()).unwrap_or_default() }
}

/// An issue from GitLab's JSON. Its project is its full reference's
/// (`group/sub/project#12`), else its address's.
pub fn parse_issue(v: &Value, base: &str) -> Option<Issue> {
    let number = v.get("iid")?.as_u64()?;
    let web = text(v, "web_url");
    let project = v
        .get("references")
        .and_then(|r| r.get("full"))
        .and_then(Value::as_str)
        .and_then(|f| f.rsplit_once('#'))
        .filter(|(_, n)| n.parse::<u64>().ok() == Some(number))
        .map(|(p, _)| p.to_string())
        .or_else(|| IssueRef::parse(&web, Host::GitLab, base).map(|r| r.project))
        .or_else(|| {
            // An address on another name for the instance.
            let path = web.split_once("://")?.1.split_once('/')?.1;
            let path = path.split("/-/").next()?;
            let path = path.strip_suffix(&format!("/issues/{number}")).unwrap_or(path);
            IssueRef::checked(path, "1", Host::GitLab).map(|r| r.project)
        })?;
    let stats = v.get("time_stats");
    let seconds = |k: &str| stats.and_then(|s| s.get(k)).and_then(Value::as_u64).filter(|n| *n > 0);
    Some(Issue {
        key: IssueRef::new(project, number),
        id: v.get("id").and_then(Value::as_u64).map(|i| i.to_string()).unwrap_or_default(),
        title: text(v, "title"),
        body: text(v, "description"),
        open: text(v, "state") != "closed",
        state_reason: None,
        author: v.get("author").map(|a| text(a, "username")).unwrap_or_default(),
        assignees: v.get("assignees").and_then(Value::as_array).map(|l| l.iter().map(user).collect()).unwrap_or_default(),
        labels: v.get("labels").and_then(Value::as_array).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string).or_else(|| x.get("name").and_then(Value::as_str).map(str::to_string))).collect()).unwrap_or_default(),
        milestone: v.get("milestone").and_then(|m| m.get("title")).and_then(Value::as_str).map(str::to_string),
        due: v.get("due_date").and_then(Value::as_str).map(str::to_string),
        weight: v.get("weight").and_then(Value::as_u64),
        estimate: seconds("time_estimate"),
        spent: seconds("total_time_spent"),
        url: web,
        created: text(v, "created_at"),
        updated: text(v, "updated_at"),
        comments: v.get("user_notes_count").and_then(Value::as_u64).unwrap_or(0),
        confidential: v.get("confidential").and_then(Value::as_bool).unwrap_or(false),
        moved_to: v.get("moved_to_id").and_then(Value::as_u64).map(|i| i.to_string()),
    })
}

fn parse_note(v: &Value) -> Note {
    Note {
        id: v.get("id").map(|i| i.to_string()).unwrap_or_default(),
        author: v.get("author").map(|a| Some(text(a, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(a, "username"))).unwrap_or_default(),
        body: text(v, "body"),
        created: text(v, "created_at"),
        system: v.get("system").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// A query typed in GitLab's filter words: `label:bug`, `assignee:@me`,
/// `author:jo`, `milestone:"v1"`, `project:group/p`, `state:closed`
/// (`is:closed`), and words searched for in titles and descriptions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub project: Option<String>,
    pub params: Vec<(&'static str, String)>,
}

pub fn filter(q: &str) -> Filter {
    let mut f = Filter::default();
    let (mut labels, mut words, mut state) = (Vec::new(), Vec::new(), "opened".to_string());
    let (mut mine, mut scope) = (false, None::<&str>);
    for token in q.split_whitespace() {
        let (k, v) = token.split_once(':').unwrap_or(("", token));
        let v = v.trim_matches('"').to_string();
        match k {
            "label" | "labels" => labels.push(v),
            "assignee" if v == "@me" => scope = Some("assigned_to_me"),
            "assignee" => f.params.push(("assignee_username", v)),
            "author" if v == "@me" => {
                mine = true;
                scope = scope.or(Some("created_by_me"));
            }
            "author" => f.params.push(("author_username", v)),
            "milestone" => f.params.push(("milestone", v)),
            "project" | "repo" => f.project = Some(v),
            "state" | "is" => state = if matches!(v.as_str(), "closed" | "all") { v } else { "opened".into() },
            _ => words.push(token.to_string()),
        }
    }
    if !labels.is_empty() {
        f.params.push(("labels", labels.join(",")));
    }
    if !words.is_empty() {
        f.params.push(("search", words.join(" ")));
    }
    f.params.push(("state", state));
    // Across the instance a scope is needed; in a project, `all` is
    // what's there.
    match (&f.project, scope) {
        (Some(_), Some("assigned_to_me")) => f.params.push(("scope", "assigned_to_me".into())),
        (Some(_), _) if mine => f.params.push(("scope", "created_by_me".into())),
        (Some(_), _) => {}
        (None, Some(s)) => f.params.push(("scope", s.into())),
        (None, None) => f.params.push(("scope", "all".into())),
    }
    f
}

impl<H: Http> Issues for GitLabIssues<H> {
    fn host(&self) -> Host {
        Host::GitLab
    }

    fn web(&self) -> &str {
        &self.base
    }

    async fn me(&self) -> Result<User, ApiError> {
        let me = user(&json(&self.get("/user", &[]).await?)?);
        if me.username.is_empty() {
            return Err(ApiError::odd("GitLab didn't say who the token belongs to"));
        }
        Ok(me)
    }

    async fn search(&self, search: &Search, paging: &Paging) -> Result<Listing, ApiError> {
        let opened = ("state", "opened".to_string());
        match search {
            Search::Assigned => self.issues_at("/issues", &[opened, ("scope", "assigned_to_me".into())], paging).await,
            Search::Created => self.issues_at("/issues", &[opened, ("scope", "created_by_me".into())], paging).await,
            Search::Project(p) => self.issues_at(&format!("{}/issues", Self::project(p)?), &[opened], paging).await,
            Search::Query(q) => {
                let f = filter(q);
                let path = match &f.project {
                    Some(p) => format!("{}/issues", Self::project(p)?),
                    None => "/issues".to_string(),
                };
                self.issues_at(&path, &f.params, paging).await
            }
            Search::Mentioned => {
                // What mentions you is a to-do item, the issue its target.
                let (per_page, page) = (paging.per_page().to_string(), paging.page().to_string());
                let response = self.get("/todos", &[("action", "mentioned"), ("type", "Issue"), ("state", "pending"), ("per_page", &per_page), ("page", &page)]).await?;
                let value = json(&response)?;
                let targets: Vec<Value> = value.as_array().into_iter().flatten().filter_map(|t| t.get("target").cloned()).filter(|t| text(t, "state") != "closed").collect();
                Ok(self.listing(&response, targets.into_iter()))
            }
        }
    }

    async fn issue(&self, key: &IssueRef) -> Result<Issue, ApiError> {
        let value = json(&self.get(&Self::issue_path(key, "")?, &[]).await?)?;
        parse_issue(&value, &self.base).ok_or_else(|| ApiError::odd(format!("GitLab's answer for {key} had no iid")))
    }

    async fn notes(&self, key: &IssueRef) -> Result<Vec<Note>, ApiError> {
        // The newest hundred, then turned oldest first.
        let value = json(&self.get(&Self::issue_path(key, "/notes")?, &[("sort", "desc"), ("order_by", "created_at"), ("per_page", "100")]).await?)?;
        let mut notes: Vec<Note> = value.as_array().into_iter().flatten().map(parse_note).collect();
        notes.reverse();
        Ok(notes)
    }

    async fn linked(&self, key: &IssueRef) -> Result<Vec<Linked>, ApiError> {
        let value = json(&self.get(&Self::issue_path(key, "/related_merge_requests")?, &[]).await?)?;
        Ok(value
            .as_array()
            .into_iter()
            .flatten()
            .map(|mr| Linked {
                reference: mr.get("references").and_then(|r| r.get("full")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("!{}", mr.get("iid").and_then(Value::as_u64).unwrap_or(0))),
                title: text(mr, "title"),
                url: text(mr, "web_url"),
                state: text(mr, "state"),
            })
            .collect())
    }

    async fn create(&self, new: &NewIssue) -> Result<Issue, ApiError> {
        let mut body = json!({ "title": new.title, "description": new.body });
        if !new.labels.is_empty() {
            body["labels"] = json!(new.labels.join(","));
        }
        if !new.assignees.is_empty() {
            body["assignee_ids"] = json!(self.user_ids(&new.assignees).await?);
        }
        let value = json(&self.request("POST", &format!("{}/issues", Self::project(&new.project)?), &[], Some(body)).await?)?;
        parse_issue(&value, &self.base).ok_or_else(|| ApiError::odd("GitLab didn't say what it made"))
    }

    async fn change(&self, key: &IssueRef, change: &Change) -> Result<(), ApiError> {
        let body = match change {
            Change::Title(t) => json!({ "title": t }),
            Change::Body(b) => json!({ "description": b }),
            Change::Labels(l) => json!({ "labels": l.join(",") }),
            Change::AddLabel(l) => json!({ "add_labels": l }),
            Change::RemoveLabel(l) => json!({ "remove_labels": l }),
            // `0` unassigns everyone, as GitLab documents it.
            Change::Assignees(a) if a.is_empty() => json!({ "assignee_ids": [0] }),
            Change::Assignees(a) => json!({ "assignee_ids": self.user_ids(a).await? }),
            Change::Close(_) => json!({ "state_event": "close" }),
            Change::Reopen => json!({ "state_event": "reopen" }),
            Change::Due(d) => json!({ "due_date": d }),
        };
        self.request("PUT", &Self::issue_path(key, "")?, &[], Some(body)).await.map(|_| ())
    }

    async fn comment(&self, key: &IssueRef, body: &str) -> Result<(), ApiError> {
        self.request("POST", &Self::issue_path(key, "/notes")?, &[], Some(json!({ "body": body }))).await.map(|_| ())
    }

    async fn labels(&self, project: &str) -> Result<Vec<Label>, ApiError> {
        let value = json(&self.get(&format!("{}/labels", Self::project(project)?), &[("per_page", "100"), ("include_ancestor_groups", "true")]).await?)?;
        Ok(value.as_array().into_iter().flatten().map(|l| Label { name: text(l, "name"), description: text(l, "description") }).filter(|l| !l.name.is_empty()).collect())
    }

    async fn assignable(&self, project: &str) -> Result<Vec<User>, ApiError> {
        let value = json(&self.get(&format!("{}/members/all", Self::project(project)?), &[("per_page", "100")]).await?)?;
        Ok(value.as_array().into_iter().flatten().map(user).filter(|u| !u.username.is_empty()).collect())
    }

    async fn resolve(&self, id: &str) -> Result<IssueRef, ApiError> {
        if !self.is_id(id) {
            return Err(ApiError::status(400, format!("\"{id}\" isn't a GitLab issue id")));
        }
        let query = "query($id:IssueID!){issue(id:$id){iid reference(full:true)}}";
        let body = json!({ "query": query, "variables": { "id": format!("gid://gitlab/Issue/{id}") } });
        let headers = [("Authorization", format!("Bearer {}", self.token)), ("Content-Type", "application/json".to_string()), ("Accept", "application/json".to_string())];
        let value = json(&send(&self.http, Host::GitLab, "POST", &format!("{}/api/graphql", self.base), &headers, Some(body.to_string())).await?)?;
        let found = value.get("data").and_then(|d| d.get("issue")).filter(|i| !i.is_null()).and_then(|i| i.get("reference")?.as_str()?.rsplit_once('#').map(|(p, n)| (p.to_string(), n.to_string())));
        match found.and_then(|(p, n)| IssueRef::checked(&p, &n, Host::GitLab)) {
            Some(key) => Ok(key),
            None => Err(ApiError::status(404, format!("no issue with the id {id}"))),
        }
    }

    async fn templates(&self, project: &str) -> Result<Vec<Template>, ApiError> {
        let base = format!("{}/templates/issues", Self::project(project)?);
        let listing = match self.get(&base, &[]).await {
            Ok(r) => json(&r)?,
            Err(e) if e.status == Some(404) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for t in listing.as_array().into_iter().flatten().take(TEMPLATES) {
            let key = Some(text(t, "key")).filter(|k| !k.is_empty()).unwrap_or_else(|| text(t, "name"));
            if key.is_empty() {
                continue;
            }
            let one = json(&self.get(&format!("{base}/{}", encode(&key)), &[]).await?)?;
            out.push(Template { name: Some(text(&one, "name")).filter(|n| !n.is_empty()).unwrap_or(key), body: text(&one, "content"), ..Default::default() });
        }
        Ok(out)
    }

    /// Time spent, as a quick action in a note (`/spend 1h30m
    /// 2026-11-12`), which files it on that day: `add_spent_time` has no
    /// date.
    async fn spend(&self, key: &IssueRef, seconds: i64, day: &str) -> Result<(), ApiError> {
        if seconds <= 0 {
            return Err(ApiError::status(400, "nothing to log"));
        }
        let day_ok = day.len() == 10 && day.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
        let mut body = format!("/spend {}", duration(seconds));
        if day_ok {
            body.push(' ');
            body.push_str(day);
        }
        self.request("POST", &Self::issue_path(key, "/notes")?, &[], Some(json!({ "body": body }))).await.map(|_| ())
    }
}

/// What a failed request said, for a caller with a response of its own.
pub fn error_of(response: &Response) -> ApiError {
    api_error(Host::GitLab, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_written_as_gitlab_reads_one() {
        assert_eq!(duration(4500), "1h15m");
        assert_eq!(duration(3600), "1h");
        assert_eq!(duration(600), "10m");
        assert_eq!(duration(20), "1m", "a minute at least");
    }

    #[test]
    fn an_issues_project_is_its_full_reference_with_nested_groups() {
        let v = json!({"iid": 4, "references": {"full": "acme/platform/api#4"}, "web_url": "https://gitlab.example.com/acme/platform/api/-/issues/4", "state": "opened", "title": "T", "due_date": "2026-12-01", "weight": 3, "time_stats": {"time_estimate": 7200, "total_time_spent": 0}});
        let i = parse_issue(&v, "https://gitlab.example.com").unwrap();
        assert_eq!((i.key.key(), i.open, i.due.as_deref(), i.weight, i.estimate, i.spent), ("acme/platform/api#4".to_string(), true, Some("2026-12-01"), Some(3), Some(7200), None));
        // No reference: read from an address, even on another name for
        // the instance.
        let v = json!({"iid": 9, "web_url": "https://gitlab.internal/acme/platform/api/-/issues/9", "state": "closed", "title": "T"});
        assert_eq!(parse_issue(&v, "https://gitlab.example.com").unwrap().key.key(), "acme/platform/api#9");
    }

    #[test]
    fn a_typed_query_is_gitlabs_filters() {
        let f = filter("label:bug label:\"backend\" assignee:@me crash on start");
        assert_eq!(f.project, None);
        assert_eq!(f.params, [("labels", "bug,backend".to_string()), ("search", "crash on start".to_string()), ("state", "opened".to_string()), ("scope", "assigned_to_me".to_string())]);
        let f = filter("project:acme/platform/api is:closed author:jo");
        assert_eq!(f.project.as_deref(), Some("acme/platform/api"));
        assert_eq!(f.params, [("author_username", "jo".to_string()), ("state", "closed".to_string())]);
        assert!(filter("anything").params.contains(&("scope", "all".to_string())));
    }
}
