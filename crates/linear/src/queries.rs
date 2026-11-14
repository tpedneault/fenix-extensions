//! Every query and mutation this crate sends, written by hand as a
//! string with variables, against Linear's GraphQL schema
//! (`tests/schema/README.md` says which version, and `tests/schema.rs`
//! holds each one to it: a field or argument that isn't in the schema
//! fails a test).
//!
//! **Variables only.** Nothing a person typed -- a title, a comment, a
//! filter -- is ever put into a query's text: each is a variable, which
//! Linear reads as data. [`crate::Client::run`] takes a `&'static str` so
//! that can't be done by accident, and `tests/seam.rs` sends a title full
//! of quotes and braces to show the query doesn't change.
//!
//! Connections are asked `first`/`after` with `pageInfo`. Comments come
//! newest first with `first` (the schema says so), and are put in order
//! after.

/// What a list of issues needs of each: `ListFields`, and the same with
/// more comments, the sub-issues and the creator for one issue's page
/// (`DetailFields`).
macro_rules! issue_fields {
    ($comments:literal) => {
        concat!(
            "id identifier url title description priority branchName dueDate createdAt updatedAt ",
            "state { id name type } team { id key name } ",
            "assignee { id name displayName isMe } creator { id name displayName } ",
            "labels(first: 50) { nodes { id name } } ",
            "cycle { id number name } project { id name } parent { identifier title } ",
            "comments(first: ",
            $comments,
            ") { nodes { id body createdAt user { name displayName } botActor { name } } pageInfo { hasNextPage } } "
        )
    };
}

macro_rules! list_fragment {
    () => {
        concat!("fragment ListFields on Issue { ", issue_fields!("20"), "}")
    };
}

macro_rules! detail_fragment {
    () => {
        concat!("fragment DetailFields on Issue { ", issue_fields!("100"), "children(first: 50) { nodes { identifier title state { name type } } } }")
    };
}

/// Who you are.
pub const VIEWER: &str = "query Viewer { viewer { id name displayName } }";

/// Your issues, filtered by the variable `filter` (an `IssueFilter`),
/// most recently updated first.
pub const ASSIGNED: &str = concat!(
    "query AssignedIssues($first: Int!, $after: String, $filter: IssueFilter) { viewer { assignedIssues(first: $first, after: $after, filter: $filter, orderBy: updatedAt) { nodes { ...ListFields } pageInfo { hasNextPage endCursor } } } } ",
    list_fragment!()
);

/// The issues you made.
pub const CREATED: &str = concat!(
    "query CreatedIssues($first: Int!, $after: String, $filter: IssueFilter) { viewer { createdIssues(first: $first, after: $after, filter: $filter, orderBy: updatedAt) { nodes { ...ListFields } pageInfo { hasNextPage endCursor } } } } ",
    list_fragment!()
);

/// Issues by a filter: a team's active cycle, its backlog, a typed filter,
/// and these identifiers.
pub const ISSUES: &str = concat!(
    "query Issues($first: Int!, $after: String, $filter: IssueFilter) { issues(first: $first, after: $after, filter: $filter, orderBy: updatedAt) { nodes { ...ListFields } pageInfo { hasNextPage endCursor } } } ",
    list_fragment!()
);

/// One issue, by identifier (`ENG-123`, or an older one it had) or id, with
/// what its page shows.
pub const ISSUE: &str = concat!("query Issue($id: String!) { issue(id: $id) { ...DetailFields } } ", detail_fragment!());

/// An issue's id: what a write names it by.
pub const ISSUE_ID: &str = "query IssueId($id: String!) { issue(id: $id) { id identifier } }";

/// The states an issue can be moved to: its team's.
pub const ISSUE_STATES: &str = "query IssueStates($id: String!) { issue(id: $id) { id identifier state { id } team { id key states(first: 100) { nodes { id name type position } } } } }";

/// The people an issue can be assigned to: its team's.
pub const ISSUE_PEOPLE: &str = "query IssuePeople($id: String!) { issue(id: $id) { team { members(first: 250) { nodes { id name displayName active } } } } }";

/// The labels an issue can have: its team's.
pub const ISSUE_LABELS: &str = "query IssueLabels($id: String!) { issue(id: $id) { team { labels(first: 250) { nodes { id name } } } } }";

/// Your teams.
pub const TEAMS: &str = "query Teams($first: Int!, $after: String) { teams(first: $first, after: $after) { nodes { id key name } pageInfo { hasNextPage endCursor } } }";

/// What a new issue in a team can have: its states, people and labels.
pub const TEAM_META: &str = "query TeamMeta($id: String!) { team(id: $id) { id key name states(first: 100) { nodes { id name type position } } members(first: 250) { nodes { id name displayName active } } labels(first: 250) { nodes { id name } } } }";

/// A change to one issue: `input` is an `IssueUpdateInput`.
pub const UPDATE_ISSUE: &str = concat!("mutation UpdateIssue($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success issue { ...ListFields } } } ", list_fragment!());

/// A comment: `input` is a `CommentCreateInput`.
pub const CREATE_COMMENT: &str = "mutation CreateComment($input: CommentCreateInput!) { commentCreate(input: $input) { success comment { id } } }";

/// A new issue: `input` is an `IssueCreateInput`.
pub const CREATE_ISSUE: &str = "mutation CreateIssue($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier url } } }";

/// Every one, for the test that holds them to the schema.
pub const ALL: [(&str, &str); 14] = [
    ("Viewer", VIEWER),
    ("AssignedIssues", ASSIGNED),
    ("CreatedIssues", CREATED),
    ("Issues", ISSUES),
    ("Issue", ISSUE),
    ("IssueId", ISSUE_ID),
    ("IssueStates", ISSUE_STATES),
    ("IssuePeople", ISSUE_PEOPLE),
    ("IssueLabels", ISSUE_LABELS),
    ("Teams", TEAMS),
    ("TeamMeta", TEAM_META),
    ("UpdateIssue", UPDATE_ISSUE),
    ("CreateComment", CREATE_COMMENT),
    ("CreateIssue", CREATE_ISSUE),
];
