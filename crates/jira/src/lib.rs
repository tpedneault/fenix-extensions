//! A client for Jira's REST API -- Server and Data Center's, and Jira
//! Cloud's where it differs (`cloud`, `adf`) --
//! search and an issue's detail (`issue`), creating and changing issues,
//! comments, transitions and worklogs (`actions`, `create`) -- as Fenix
//! 1.x's `fenix-jira` was, but `async`, over a [`Transport`]: the Jira
//! extension's own HTTP, or the agenda's calls to the Jira extension.
//! And the tracker seam served over it (`tracker`), the agenda's view of
//! any tracker, for Jira.

mod actions;
pub mod adf;
pub mod cloud;
mod client;
mod create;
mod issue;
pub mod recorded;
pub mod tracker;

pub use actions::{Priority, StatusInfo, Transition, CLOUD_DESCRIPTION};
pub use cloud::{Auth, Deployment};
pub use client::{JiraClient, Reply, Transport};
pub use create::{CreateField, IssueType, NewIssue};
pub use issue::{build_ids_and_keys_jql, build_keys_jql, build_my_open_issues_jql, project_of, Comment, IssueDetail, IssueSummary};
