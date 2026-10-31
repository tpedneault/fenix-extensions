//! A client for a self-hosted Jira Server/Data Center's REST API --
//! search and an issue's detail (`issue`), creating and changing issues,
//! comments, transitions and worklogs (`actions`, `create`) -- as Fenix
//! 1.x's `fenix-jira` was, but `async`, over a [`Transport`]: the Jira
//! extension's own HTTP, or the agenda's calls to the Jira extension.

mod actions;
mod client;
mod create;
mod issue;

pub use actions::{Priority, StatusInfo, Transition};
pub use client::{JiraClient, Reply, Transport};
pub use create::{CreateField, IssueType, NewIssue};
pub use issue::{build_keys_jql, build_my_open_issues_jql, project_of, Comment, IssueDetail, IssueSummary};
