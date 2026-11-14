use crate::adf;
use crate::client::JiraClient;

/// One row of a search result -- just enough to list (key, summary,
/// status, assignee, when it last changed), not the full issue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueSummary {
    pub key: String,
    pub summary: String,
    pub status: String,
    /// Jira's own bucket for `status`: `"new"`, `"indeterminate"` or
    /// `"done"` -- the one thing about a status every workflow agrees on,
    /// whatever the status itself is called.
    pub status_category: String,
    pub assignee: Option<String>,
    pub updated: String,
    pub priority: Option<String>,
    pub issue_type: Option<String>,
    /// `YYYY-MM-DD`.
    pub due: Option<String>,
}

/// One issue's own comment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created: String,
}

/// A single issue's full detail (`GET /rest/api/2/issue/{key}`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueDetail {
    pub key: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: String,
    /// The status's id -- stable across renames, unlike its name.
    pub status_id: String,
    /// See `IssueSummary::status_category`.
    pub status_category: String,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    /// The assignee's username (`name`) on Server, account id
    /// (`accountId`) on Cloud, for "is this still mine" checks --
    /// `assignee` is only the display name.
    pub assignee_id: Option<String>,
    /// Whether the board's Flagged (impediment) field is set -- only ever
    /// true when the fetch was told which field that is.
    pub flagged: bool,
    pub reporter: Option<String>,
    pub created: String,
    pub updated: String,
    pub comments: Vec<Comment>,
    /// The due date, `YYYY-MM-DD`, when the issue has one.
    pub due: Option<String>,
    pub issue_type: Option<String>,
    pub labels: Vec<String>,
}

/// The JQL for "every issue assigned to `user_id`, scoped to whichever
/// projects are currently tracked, minus whichever statuses are
/// currently excluded" -- pure and directly testable. The `AND project
/// IN (...)` clause is omitted entirely when `project_keys` is empty
/// (an empty `IN ()` is invalid JQL, and "no projects tracked yet"
/// should mean "search everywhere," not "search nothing"), and project
/// keys are joined verbatim (Jira project keys are always plain
/// alphanumeric identifiers, no quoting/escaping needed the way a
/// free-text value would). `excluded_statuses` gets the same
/// conditional-clause treatment -- omitted when empty -- but each name
/// *is* quoted (unlike project keys, a status name routinely contains
/// spaces, e.g. `"In Progress"`), with any embedded `"` escaped so a
/// status name can't break out of its own JQL string literal.
/// Quotes `s` as a JQL string literal.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Every open issue assigned to whoever the token belongs to, across
/// `project_keys` (everywhere when empty) -- what the agenda offers to
/// link or import.
pub fn build_my_open_issues_jql(project_keys: &[String]) -> String {
    let mut jql = "assignee = currentUser() AND statusCategory != Done".to_string();
    if !project_keys.is_empty() {
        jql.push_str(&format!(" AND project IN ({})", project_keys.join(",")));
    }
    jql.push_str(" ORDER BY updated DESC");
    jql
}

/// Exactly these issues -- how the agenda refreshes the ones it links to.
pub fn build_keys_jql(keys: &[String]) -> String {
    let quoted: Vec<String> = keys.iter().map(|k| quote(k)).collect();
    format!("key IN ({})", quoted.join(","))
}

/// The project a key belongs to (`PROJ-12` -> `PROJ`).
pub fn project_of(key: &str) -> &str {
    key.rsplit_once('-').map_or(key, |(project, _)| project)
}

const DETAIL_FIELDS: &str = "summary,description,status,priority,assignee,reporter,created,updated,comment,duedate,issuetype,labels";

/// How many pages of the enhanced search one call follows, at most.
const CLOUD_PAGES: usize = 10;

impl JiraClient {
    /// The issues `jql` finds, with `fields`: one page of Server's
    /// `/rest/api/2/search` from `start_at`, with how many match in all;
    /// on Cloud, `cloud_page` of the enhanced search, and the token of
    /// the next.
    async fn find(&self, jql: &str, page: Page<'_>, max_results: u32, fields: &str) -> Result<Found, String> {
        let max = max_results.to_string();
        match page {
            Page::Server(start_at) => {
                let start = start_at.to_string();
                let mut query = vec![("jql", jql)];
                if start_at > 0 {
                    query.push(("startAt", &start));
                }
                query.extend([("maxResults", max.as_str()), ("fields", fields)]);
                let body = self.request("/rest/api/2/search", &query).await?;
                let issues = body.get("issues").and_then(|v| v.as_array()).ok_or_else(|| "unexpected search response shape".to_string())?.clone();
                let total = body.get("total").and_then(|t| t.as_u64()).map(|t| t as u32);
                Ok(Found { issues, total, next: None })
            }
            Page::Cloud(token) => {
                let mut query = vec![("jql", jql)];
                if let Some(token) = token {
                    query.push(("nextPageToken", token));
                }
                query.extend([("maxResults", max.as_str()), ("fields", fields)]);
                let body = match self.request("/rest/api/3/search/jql", &query).await {
                    Ok(body) => body,
                    // A site that hasn't the enhanced search (a Data
                    // Center taken for Cloud): the search it has, its
                    // first page.
                    Err(e) if token.is_none() && (e.starts_with("HTTP 404") || e.starts_with("HTTP 405")) => {
                        return Box::pin(self.find(jql, Page::Server(0), max_results, fields)).await;
                    }
                    Err(e) => return Err(e),
                };
                let issues = body.get("issues").and_then(|v| v.as_array()).ok_or_else(|| "unexpected search response shape".to_string())?.clone();
                let last = body.get("isLast").and_then(|l| l.as_bool()).unwrap_or(false);
                let next = body.get("nextPageToken").and_then(|t| t.as_str()).filter(|t| !last && !t.is_empty()).map(str::to_string);
                Ok(Found { issues, total: None, next })
            }
        }
    }

    /// The first page, as this Jira pages.
    fn first(&self) -> Page<'static> {
        if self.is_cloud() {
            Page::Cloud(None)
        } else {
            Page::Server(0)
        }
    }

    /// `GET /rest/api/2/search` (Cloud: `/rest/api/3/search/jql`) --
    /// runs `jql`, returns up to `max_results` matching issues as
    /// `IssueSummary`s.
    pub async fn search_issues(&self, jql: &str, max_results: u32) -> Result<Vec<IssueSummary>, String> {
        let found = self.find(jql, self.first(), max_results, "summary,status,assignee,updated,priority,issuetype,duedate").await?;
        Ok(found.issues.iter().filter_map(parse_issue_summary).collect())
    }

    /// `GET /rest/api/2/issue/{key}` (Cloud: v3, its rich text read as
    /// text) -- Jira embeds comments under `fields.comment.comments[]`
    /// when that field is requested, so one call covers the full detail
    /// view including comments, no separate paginated fetch needed.
    pub async fn get_issue(&self, key: &str) -> Result<IssueDetail, String> {
        self.get_issue_with(key, None).await
    }

    /// `get_issue`, also reading the Flagged field when `flag_field`
    /// names it (see `find_flagged_field`).
    pub async fn get_issue_with(&self, key: &str, flag_field: Option<&str>) -> Result<IssueDetail, String> {
        let path = format!("/rest/api/{}/issue/{key}", if self.is_cloud() { 3 } else { 2 });
        let fields = detail_fields(flag_field);
        let body = self.request(&path, &[("fields", &fields)]).await?;
        parse_issue_detail(&body, flag_field).ok_or_else(|| "unexpected issue response shape".to_string())
    }

    /// `search_issues`, but every row carries the full detail (description,
    /// priority, comments...) -- one request refreshes every linked task
    /// (on Cloud, one a page of the enhanced search, which pages by
    /// itself).
    pub async fn search_details(&self, jql: &str, max_results: u32, flag_field: Option<&str>) -> Result<Vec<IssueDetail>, String> {
        let fields = detail_fields(flag_field);
        let mut found = self.find(jql, self.first(), max_results, &fields).await?;
        let mut issues = std::mem::take(&mut found.issues);
        let mut pages = 1;
        while let Some(token) = found.next.take().filter(|_| pages < CLOUD_PAGES && (issues.len() as u32) < max_results) {
            found = self.find(jql, Page::Cloud(Some(&token)), max_results - issues.len() as u32, &fields).await?;
            issues.append(&mut found.issues);
            pages += 1;
        }
        Ok(issues.iter().filter_map(|v| parse_issue_detail(v, flag_field)).collect())
    }

    /// `search_details` from the `start_at`th match: the issues, and how
    /// many match in all -- one page of a longer list (Server's paging).
    pub async fn search_details_page(&self, jql: &str, start_at: u32, max_results: u32, flag_field: Option<&str>) -> Result<(Vec<IssueDetail>, u32), String> {
        let fields = detail_fields(flag_field);
        let found = self.find(jql, Page::Server(start_at), max_results, &fields).await?;
        let issues: Vec<IssueDetail> = found.issues.iter().filter_map(|v| parse_issue_detail(v, flag_field)).collect();
        let total = found.total.unwrap_or(start_at + issues.len() as u32);
        Ok((issues, total))
    }

    /// One page of `search_details` on Cloud's enhanced search, from the
    /// page `token` names (the first when none): the issues, and the next
    /// page's token on all but the last.
    pub async fn search_details_after(&self, jql: &str, token: Option<&str>, max_results: u32, flag_field: Option<&str>) -> Result<(Vec<IssueDetail>, Option<String>), String> {
        let fields = detail_fields(flag_field);
        let found = self.find(jql, Page::Cloud(token), max_results, &fields).await?;
        Ok((found.issues.iter().filter_map(|v| parse_issue_detail(v, flag_field)).collect(), found.next))
    }
}

/// Which page of a search: Server's `startAt`, or Cloud's token.
#[derive(Clone, Copy)]
enum Page<'a> {
    Server(u32),
    Cloud(Option<&'a str>),
}

/// A page of a search.
struct Found {
    issues: Vec<serde_json::Value>,
    total: Option<u32>,
    next: Option<String>,
}

fn detail_fields(flag_field: Option<&str>) -> String {
    match flag_field {
        Some(field) => format!("{DETAIL_FIELDS},{field}"),
        None => DETAIL_FIELDS.to_string(),
    }
}

fn status_category(fields: &serde_json::Value) -> String {
    fields
        .get("status")
        .and_then(|s| s.get("statusCategory"))
        .and_then(|c| c.get("key"))
        .and_then(|k| k.as_str())
        .unwrap_or("new")
        .to_string()
}

/// A multi-checkbox custom field like Flagged is `null` or `[]` when
/// unset and `[{"value": "Impediment"}]` when set.
fn is_set(value: Option<&serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Array(items)) => !items.is_empty(),
        Some(_) => true,
    }
}

fn text_field(fields: &serde_json::Value, name: &str) -> Option<String> {
    fields.get(name).and_then(|v| v.as_str()).map(str::to_string)
}

/// A text field: Server's text as it is, Cloud's rich text (ADF) as
/// plain text.
fn rich_text(fields: &serde_json::Value, name: &str) -> Option<String> {
    match fields.get(name)? {
        serde_json::Value::String(s) => Some(s.clone()),
        v @ serde_json::Value::Object(_) => Some(adf::to_text(v)),
        _ => None,
    }
}

fn person_name(fields: &serde_json::Value, name: &str) -> Option<String> {
    fields.get(name)?.get("displayName")?.as_str().map(str::to_string)
}

fn parse_issue_summary(v: &serde_json::Value) -> Option<IssueSummary> {
    let key = v.get("key")?.as_str()?.to_string();
    let fields = v.get("fields")?;
    Some(IssueSummary {
        key,
        summary: text_field(fields, "summary").unwrap_or_default(),
        status: fields.get("status").and_then(|s| s.get("name")).and_then(|n| n.as_str()).unwrap_or("Unknown").to_string(),
        status_category: status_category(fields),
        assignee: person_name(fields, "assignee"),
        updated: text_field(fields, "updated").unwrap_or_default(),
        priority: fields.get("priority").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_string),
        issue_type: fields.get("issuetype").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_string),
        due: text_field(fields, "duedate"),
    })
}

fn parse_comment(v: &serde_json::Value) -> Option<Comment> {
    Some(Comment {
        id: v.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string(),
        author: v.get("author").and_then(|a| a.get("displayName")).and_then(|n| n.as_str()).unwrap_or("Unknown").to_string(),
        body: v.get("body").map(adf::to_text).unwrap_or_default(),
        created: v.get("created").and_then(|c| c.as_str()).unwrap_or_default().to_string(),
    })
}

fn parse_issue_detail(v: &serde_json::Value, flag_field: Option<&str>) -> Option<IssueDetail> {
    let key = v.get("key")?.as_str()?.to_string();
    let fields = v.get("fields")?;
    let comments = fields
        .get("comment")
        .and_then(|c| c.get("comments"))
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(parse_comment).collect())
        .unwrap_or_default();
    Some(IssueDetail {
        key,
        summary: text_field(fields, "summary").unwrap_or_default(),
        description: rich_text(fields, "description"),
        status: fields.get("status").and_then(|s| s.get("name")).and_then(|n| n.as_str()).unwrap_or("Unknown").to_string(),
        status_id: fields.get("status").and_then(|s| s.get("id")).and_then(|n| n.as_str()).unwrap_or_default().to_string(),
        status_category: status_category(fields),
        priority: fields.get("priority").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_string),
        assignee: person_name(fields, "assignee"),
        assignee_id: fields.get("assignee").and_then(|a| a.get("name").or_else(|| a.get("accountId"))).and_then(|n| n.as_str()).map(str::to_string),
        flagged: flag_field.is_some_and(|f| is_set(fields.get(f))),
        reporter: person_name(fields, "reporter"),
        created: text_field(fields, "created").unwrap_or_default(),
        updated: text_field(fields, "updated").unwrap_or_default(),
        comments,
        due: text_field(fields, "duedate"),
        issue_type: fields.get("issuetype").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_string),
        labels: fields.get("labels").and_then(|l| l.as_array()).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_response(issues: &str) -> serde_json::Value {
        serde_json::from_str(&format!(r#"{{"issues": [{issues}]}}"#)).unwrap()
    }

    #[test]
    fn parse_issue_summary_reads_a_typical_search_result_row() {
        let v = search_response(
            r#"{"key": "PROJ-1", "fields": {"summary": "Fix the thing", "status": {"name": "In Progress"}, "assignee": {"displayName": "John Doe"}, "updated": "2024-01-15T10:30:00.000+0000"}}"#,
        );
        let issue = parse_issue_summary(&v["issues"][0]).unwrap();
        assert_eq!(issue.key, "PROJ-1");
        assert_eq!(issue.summary, "Fix the thing");
        assert_eq!(issue.status, "In Progress");
        assert_eq!(issue.assignee, Some("John Doe".to_string()));
        assert_eq!(issue.updated, "2024-01-15T10:30:00.000+0000");
    }

    #[test]
    fn parse_issue_summary_handles_an_unassigned_issue() {
        let v = search_response(r#"{"key": "PROJ-2", "fields": {"summary": "Unassigned", "status": {"name": "Open"}, "assignee": null, "updated": "2024-01-01T00:00:00.000+0000"}}"#);
        let issue = parse_issue_summary(&v["issues"][0]).unwrap();
        assert_eq!(issue.assignee, None);
    }

    #[test]
    fn parse_issue_summary_returns_none_for_a_malformed_row() {
        let v: serde_json::Value = serde_json::from_str(r#"{"nope": true}"#).unwrap();
        assert!(parse_issue_summary(&v).is_none());
    }

    #[test]
    fn parse_issue_detail_reads_description_status_people_and_comments() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{
                "key": "PROJ-1",
                "fields": {
                    "summary": "Fix the thing",
                    "description": "Full description here",
                    "status": {"name": "In Progress"},
                    "assignee": {"displayName": "John Doe"},
                    "reporter": {"displayName": "Jane Smith"},
                    "created": "2024-01-01T00:00:00.000+0000",
                    "updated": "2024-01-15T10:30:00.000+0000",
                    "comment": {"comments": [
                        {"author": {"displayName": "Jane Smith"}, "body": "Looking into it", "created": "2024-01-02T00:00:00.000+0000"}
                    ]}
                }
            }"#,
        )
        .unwrap();
        let detail = parse_issue_detail(&v, None).unwrap();
        assert_eq!(detail.key, "PROJ-1");
        assert_eq!(detail.description, Some("Full description here".to_string()));
        assert_eq!(detail.reporter, Some("Jane Smith".to_string()));
        assert_eq!(detail.comments.len(), 1);
        assert_eq!(detail.comments[0].author, "Jane Smith");
        assert_eq!(detail.comments[0].body, "Looking into it");
    }

    #[test]
    fn parse_issue_detail_handles_a_missing_description_and_no_comments() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"key": "PROJ-2", "fields": {"summary": "No description", "description": null, "status": {"name": "Open"}, "assignee": null, "reporter": null, "created": "2024-01-01T00:00:00.000+0000", "updated": "2024-01-01T00:00:00.000+0000"}}"#,
        )
        .unwrap();
        let detail = parse_issue_detail(&v, None).unwrap();
        assert_eq!(detail.description, None);
        assert!(detail.comments.is_empty());
    }

    #[test]
    fn parse_issue_detail_reads_the_due_date() {
        let v: serde_json::Value = serde_json::from_str(r#"{"key": "PROJ-4", "fields": {"summary": "S", "duedate": "2026-10-02"}}"#).unwrap();
        assert_eq!(parse_issue_detail(&v, None).unwrap().due.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn parse_issue_detail_returns_none_for_a_malformed_body() {
        let v: serde_json::Value = serde_json::from_str(r#"{"nope": true}"#).unwrap();
        assert!(parse_issue_detail(&v, None).is_none());
    }

    #[test]
    fn my_open_issues_jql_scopes_to_projects_and_skips_done() {
        assert_eq!(build_my_open_issues_jql(&[]), "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC");
        assert_eq!(
            build_my_open_issues_jql(&["PROJ".to_string(), "OPS".to_string()]),
            "assignee = currentUser() AND statusCategory != Done AND project IN (PROJ,OPS) ORDER BY updated DESC"
        );
    }

    #[test]
    fn keys_jql_quotes_every_key() {
        assert_eq!(build_keys_jql(&["PROJ-1".to_string(), "OPS-22".to_string()]), r#"key IN ("PROJ-1","OPS-22")"#);
    }

    #[test]
    fn project_of_strips_the_issue_number() {
        assert_eq!(project_of("PROJ-12"), "PROJ");
        assert_eq!(project_of("MY-TEAM-3"), "MY-TEAM");
    }

    #[test]
    fn parse_issue_detail_reads_status_category_priority_assignee_id_and_flag() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"key": "PROJ-3", "fields": {
                "summary": "S", "status": {"id": "10103", "name": "On Hold", "statusCategory": {"key": "indeterminate"}},
                "priority": {"name": "Major"}, "assignee": {"name": "jo1", "displayName": "Jo"},
                "customfield_10000": [{"value": "Impediment"}],
                "comment": {"comments": [{"id": "77", "author": {"displayName": "Jo"}, "body": "hi", "created": "x"}]}
            }}"#,
        )
        .unwrap();
        let detail = parse_issue_detail(&v, Some("customfield_10000")).unwrap();
        assert_eq!(detail.status_id, "10103");
        assert_eq!(detail.status_category, "indeterminate");
        assert_eq!(detail.priority.as_deref(), Some("Major"));
        assert_eq!(detail.assignee_id.as_deref(), Some("jo1"));
        assert!(detail.flagged);
        assert_eq!(detail.comments[0].id, "77");
        assert_eq!(detail.due, None);
        assert!(!parse_issue_detail(&v, None).unwrap().flagged, "no flag field named means not flagged");
    }
}
