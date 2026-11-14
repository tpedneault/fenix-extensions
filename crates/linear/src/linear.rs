//! Linear, as the seam and the extension's page ask of it: your issues,
//! these issues, one issue with its thread, the states, people, teams and
//! labels to choose from, and the writes. Each is a query from
//! [`crate::queries`] with its variables, over a [`Client`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fenix_tracker::{clean, code, Change, SeamError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::gql::Client;
use crate::model::{self, detail, priority_number, Conn, Detail, RawIssue, RawLabel, RawState, RawTeam, RawUser};
use crate::queries;
use crate::refs::{self, Named};

/// The states a list leaves out unless it's asked for them.
const FINISHED: [&str; 2] = ["completed", "canceled"];

/// Which issues a list is of.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// Assigned to you, not finished -- or, with `since` (an `updated`), all
    /// of yours changed at or after it, finished or not.
    Assigned { since: Option<String> },
    /// Made by you, not finished.
    Created,
    /// A team's active cycle: its key.
    Cycle { team: String },
    /// A team's backlog.
    Backlog { team: String },
    /// An `IssueFilter`, as the typed filter reads it.
    Filter(Value),
}

/// One page of a list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listed {
    pub issues: Vec<Detail>,
    /// Where the next page starts, when there is one.
    pub next: Option<String>,
}

/// Who you are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Me {
    pub id: String,
    pub name: String,
}

/// A team: for a list of teams and for a new issue's choices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamInfo {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// What an issue can be given in a team.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamMeta {
    pub team: TeamInfo,
    /// In the order of Linear's board: by type, then position.
    pub states: Vec<StateInfo>,
    pub people: Vec<fenix_tracker::Person>,
    pub labels: Vec<LabelInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabelInfo {
    pub id: String,
    pub name: String,
}

/// A new issue, as the form fills it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewIssue {
    pub team_id: String,
    pub title: String,
    pub description: String,
    /// Linear's 0 to 4.
    pub priority: Option<u8>,
    pub label_ids: Vec<String>,
    pub assignee_id: Option<String>,
    pub state_id: Option<String>,
}

/// What was made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Created {
    pub identifier: String,
    pub url: Option<String>,
}

/// A client of Linear, with the ids of the issues it has resolved: a
/// write names an issue by its id, which an identifier is looked up for
/// once.
#[derive(Clone)]
pub struct Linear {
    pub client: Client,
    ids: Rc<RefCell<HashMap<String, String>>>,
}

fn odd(what: &str, e: impl std::fmt::Display) -> SeamError {
    SeamError::new(code::FAILED, format!("Linear answered {what} oddly: {e}"))
}

fn read<T: for<'de> Deserialize<'de>>(what: &str, v: Value) -> Result<T, SeamError> {
    serde_json::from_value(v).map_err(|e| odd(what, e))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Issues {
    issues: Option<Conn<RawIssue>>,
    viewer: Option<Viewer>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Viewer {
    assigned_issues: Option<Conn<RawIssue>>,
    created_issues: Option<Conn<RawIssue>>,
}

fn listed(conn: Option<Conn<RawIssue>>) -> Listed {
    let conn = conn.unwrap_or_default();
    let next = conn.page_info.as_ref().and_then(|p| p.next());
    Listed { issues: conn.nodes.into_iter().filter_map(detail).collect(), next }
}

fn team_info(t: &RawTeam) -> TeamInfo {
    TeamInfo { id: t.id.clone(), key: clean::name(&t.key), name: clean::name(&t.name) }
}

/// States in the order of Linear's board.
fn ordered(states: Vec<RawState>) -> Vec<RawState> {
    let mut states = states;
    states.sort_by(|a, b| model::state_order(&a.kind).cmp(&model::state_order(&b.kind)).then(a.position.total_cmp(&b.position)));
    states
}

fn people(conn: Option<Conn<RawUser>>) -> Vec<fenix_tracker::Person> {
    conn.unwrap_or_default().nodes.iter().filter(|u| u.active != Some(false) && !u.id.is_empty()).map(|u| fenix_tracker::Person { id: u.id.clone(), name: u.shown().to_string() }).collect()
}

impl Linear {
    pub fn new(client: Client) -> Linear {
        Linear { client, ids: Rc::default() }
    }

    /// Who the key belongs to: also the cheapest request that shows it works.
    pub async fn viewer(&self) -> Result<Me, SeamError> {
        let data = self.client.run(queries::VIEWER, json!({})).await?;
        let me: RawUser = read("who you are", data["viewer"].clone())?;
        Ok(Me { id: me.id.clone(), name: clean::name(me.shown()) })
    }

    /// One page of `view`: at most `first` issues (held to 50, which keeps
    /// a query's cost down), from `after`.
    pub async fn list(&self, view: &View, first: u32, after: Option<&str>) -> Result<Listed, SeamError> {
        let finished = json!({ "state": { "type": { "nin": FINISHED } } });
        let filter = match view {
            View::Assigned { since: Some(since) } => json!({ "updatedAt": { "gte": since } }),
            View::Assigned { since: None } | View::Created => finished,
            View::Cycle { team } => json!({ "team": { "key": { "eqIgnoreCase": team } }, "cycle": { "isActive": { "eq": true } } }),
            View::Backlog { team } => json!({ "team": { "key": { "eqIgnoreCase": team } }, "state": { "type": { "eq": "backlog" } } }),
            View::Filter(filter) => filter.clone(),
        };
        let query = match view {
            View::Assigned { .. } => queries::ASSIGNED,
            View::Created => queries::CREATED,
            _ => queries::ISSUES,
        };
        let data = self.client.run(query, json!({ "first": first.clamp(1, 50), "after": after, "filter": filter })).await?;
        let page: Issues = read("a list of issues", data)?;
        let page = match view {
            View::Assigned { .. } => listed(page.viewer.and_then(|v| v.assigned_issues)),
            View::Created => listed(page.viewer.and_then(|v| v.created_issues)),
            _ => listed(page.issues),
        };
        for d in &page.issues {
            self.remember(d);
        }
        Ok(page)
    }

    /// One issue with its whole page's worth: comments, sub-issues.
    pub async fn issue(&self, named: &Named) -> Result<Detail, SeamError> {
        let data = self.client.run(queries::ISSUE, json!({ "id": named.text() })).await?;
        let raw: RawIssue = read("an issue", data["issue"].clone())?;
        let d = detail(raw).ok_or_else(|| SeamError::new(code::NOT_FOUND, format!("{} isn't an issue I can show", named.text())))?;
        self.remember(&d);
        Ok(d)
    }

    fn remember(&self, d: &Detail) {
        if !d.id.is_empty() {
            self.ids.borrow_mut().insert(d.issue.key.clone(), d.id.clone());
        }
    }

    /// Issues by what they were called, in one request when there are
    /// several (one issue is fetched on its own, which also finds it by an
    /// identifier it once had). The refs of those not found come back too,
    /// as they were asked.
    pub async fn issues(&self, asked: &[String]) -> Result<(Vec<Detail>, Vec<String>), SeamError> {
        let named: Vec<(&String, Option<Named>)> = asked.iter().map(|r| (r, refs::parse(r))).collect();
        let mut missing: Vec<String> = named.iter().filter(|(_, n)| n.is_none()).map(|(r, _)| (*r).clone()).collect();
        let valid: Vec<&Named> = named.iter().filter_map(|(_, n)| n.as_ref()).collect();
        let mut found: Vec<Detail> = Vec::new();
        match valid.as_slice() {
            [] => {}
            [one] => match self.issue(one).await {
                Ok(d) => found.push(d),
                Err(e) if e.is(code::NOT_FOUND) => missing.extend(named.iter().filter(|(_, n)| n.is_some()).map(|(r, _)| (*r).clone())),
                Err(e) => return Err(e),
            },
            many => {
                let mut any: Vec<Value> = Vec::new();
                let mut ids: Vec<&str> = Vec::new();
                for n in many {
                    match n {
                        Named::Identifier { team, number } => any.push(json!({ "team": { "key": { "eqIgnoreCase": team } }, "number": { "eq": number } })),
                        Named::Id(id) => ids.push(id),
                    }
                }
                if !ids.is_empty() {
                    any.push(json!({ "id": { "in": ids } }));
                }
                let data = self.client.run(queries::ISSUES, json!({ "first": many.len().min(50), "after": null, "filter": { "or": any } })).await?;
                let page: Issues = read("issues", data)?;
                found = listed(page.issues).issues;
                for d in &found {
                    self.remember(d);
                }
                for (r, n) in &named {
                    let has = match n {
                        Some(Named::Identifier { team, number }) => found.iter().any(|d| d.issue.key == format!("{team}-{number}")),
                        Some(Named::Id(id)) => found.iter().any(|d| d.id.eq_ignore_ascii_case(id)),
                        None => continue,
                    };
                    if !has {
                        missing.push((*r).clone());
                    }
                }
            }
        }
        Ok((found, missing))
    }

    /// An issue's id, what a write names it by.
    pub async fn resolve(&self, r: &str) -> Result<String, SeamError> {
        let named = refs::parse(r).ok_or_else(|| SeamError::new(code::NOT_FOUND, format!("\"{}\" isn't an issue's identifier or address", clean::line(r, 60))))?;
        if let Named::Id(id) = &named {
            return Ok(id.clone());
        }
        if let Some(id) = self.ids.borrow().get(&named.text()) {
            return Ok(id.clone());
        }
        let data = self.client.run(queries::ISSUE_ID, json!({ "id": named.text() })).await?;
        let id = data["issue"]["id"].as_str().filter(|i| !i.is_empty()).ok_or_else(|| SeamError::new(code::NOT_FOUND, format!("{} isn't there", named.text())))?.to_string();
        self.ids.borrow_mut().insert(named.text(), id.clone());
        Ok(id)
    }

    /// The states an issue can move to -- its team's, in the order of
    /// Linear's board -- and the one it's in.
    pub async fn states(&self, r: &str) -> Result<(Vec<StateInfo>, Option<String>), SeamError> {
        let named = refs::parse(r).ok_or_else(|| SeamError::new(code::NOT_FOUND, format!("\"{}\" isn't an issue's identifier or address", clean::line(r, 60))))?;
        let data = self.client.run(queries::ISSUE_STATES, json!({ "id": named.text() })).await?;
        let issue: RawIssue = read("an issue's states", data["issue"].clone())?;
        let team = issue.team.unwrap_or_default();
        let states = ordered(team.states.unwrap_or_default().nodes).into_iter().map(|s| StateInfo { id: s.id, name: clean::name(&s.name), kind: s.kind }).collect();
        Ok((states, issue.state.map(|s| s.id).filter(|s| !s.is_empty())))
    }

    /// The people an issue can be assigned to: its team's.
    pub async fn people(&self, r: &str) -> Result<Vec<fenix_tracker::Person>, SeamError> {
        let named = refs::parse(r).ok_or_else(|| SeamError::new(code::NOT_FOUND, format!("\"{}\" isn't an issue's identifier or address", clean::line(r, 60))))?;
        let data = self.client.run(queries::ISSUE_PEOPLE, json!({ "id": named.text() })).await?;
        let issue: RawIssue = read("an issue's team", data["issue"].clone())?;
        Ok(people(issue.team.unwrap_or_default().members))
    }

    /// Your teams.
    pub async fn teams(&self) -> Result<Vec<TeamInfo>, SeamError> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        // A few pages at most: a workspace with more than 250 teams isn't one this lists.
        for _ in 0..5 {
            let data = self.client.run(queries::TEAMS, json!({ "first": 50, "after": after })).await?;
            let conn: Conn<RawTeam> = read("your teams", data["teams"].clone())?;
            out.extend(conn.nodes.iter().map(team_info));
            after = conn.page_info.and_then(|p| p.next());
            if after.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// What a new issue in team `id` can be given.
    pub async fn team_meta(&self, id: &str) -> Result<TeamMeta, SeamError> {
        let data = self.client.run(queries::TEAM_META, json!({ "id": id })).await?;
        let team: RawTeam = read("a team", data["team"].clone())?;
        let labels: Option<Conn<RawLabel>> = team.labels.clone();
        Ok(TeamMeta {
            team: team_info(&team),
            states: ordered(team.states.clone().unwrap_or_default().nodes).into_iter().map(|s| StateInfo { id: s.id, name: clean::name(&s.name), kind: s.kind }).collect(),
            people: people(team.members.clone()),
            labels: labels.unwrap_or_default().nodes.into_iter().map(|l| LabelInfo { id: l.id, name: clean::name(&l.name) }).collect(),
        })
    }

    // -- Writes ----------------------------------------------------------------

    async fn update_input(&self, r: &str, input: Value) -> Result<Detail, SeamError> {
        let id = self.resolve(r).await?;
        let data = self.client.run(queries::UPDATE_ISSUE, json!({ "id": id, "input": input })).await?;
        if data["issueUpdate"]["success"].as_bool() != Some(true) {
            return Err(SeamError::new(code::FAILED, "Linear didn't make the change"));
        }
        let raw: RawIssue = read("the changed issue", data["issueUpdate"]["issue"].clone())?;
        detail(raw).ok_or_else(|| odd("the changed issue", "no identifier"))
    }

    /// Moves it to workflow state `state_id`.
    pub async fn set_state(&self, r: &str, state_id: &str) -> Result<Detail, SeamError> {
        self.update_input(r, json!({ "stateId": state_id })).await
    }

    /// One field changed (`update`).
    pub async fn change(&self, r: &str, change: &Change) -> Result<Detail, SeamError> {
        let input = match change {
            Change::Title(t) => json!({ "title": t }),
            Change::Body(b) => json!({ "description": b }),
            Change::Priority(name) => {
                let n = priority_number(name).ok_or_else(|| SeamError::new(code::BAD_REQUEST, format!("\"{}\" isn't one of Linear's priorities (Urgent, High, Medium, Low, No priority)", clean::line(name, 40))))?;
                json!({ "priority": n })
            }
            Change::Due(d) => json!({ "dueDate": d }),
            Change::Assignee(p) => json!({ "assigneeId": p.id }),
        };
        self.update_input(r, input).await
    }

    /// Priority `n` (0 to 4) set directly.
    pub async fn set_priority(&self, r: &str, n: u8) -> Result<Detail, SeamError> {
        self.update_input(r, json!({ "priority": n })).await
    }

    /// Assigned to `person` (an id), or to no one.
    pub async fn assign(&self, r: &str, person: Option<&str>) -> Result<Detail, SeamError> {
        self.update_input(r, json!({ "assigneeId": person })).await
    }

    /// A comment, in Markdown.
    pub async fn comment(&self, r: &str, body: &str) -> Result<(), SeamError> {
        let id = self.resolve(r).await?;
        let data = self.client.run(queries::CREATE_COMMENT, json!({ "input": { "issueId": id, "body": body } })).await?;
        if data["commentCreate"]["success"].as_bool() != Some(true) {
            return Err(SeamError::new(code::FAILED, "Linear didn't take the comment"));
        }
        Ok(())
    }

    /// A new issue.
    pub async fn create(&self, new: &NewIssue) -> Result<Created, SeamError> {
        let mut input = json!({ "teamId": new.team_id, "title": new.title });
        let map = input.as_object_mut().expect("an object");
        if !new.description.trim().is_empty() {
            map.insert("description".into(), json!(new.description));
        }
        if let Some(p) = new.priority {
            map.insert("priority".into(), json!(p));
        }
        if !new.label_ids.is_empty() {
            map.insert("labelIds".into(), json!(new.label_ids));
        }
        if let Some(a) = &new.assignee_id {
            map.insert("assigneeId".into(), json!(a));
        }
        if let Some(s) = &new.state_id {
            map.insert("stateId".into(), json!(s));
        }
        let data = self.client.run(queries::CREATE_ISSUE, json!({ "input": input })).await?;
        if data["issueCreate"]["success"].as_bool() != Some(true) {
            return Err(SeamError::new(code::FAILED, "Linear didn't make the issue"));
        }
        let issue = &data["issueCreate"]["issue"];
        let identifier = issue["identifier"].as_str().and_then(clean::key).ok_or_else(|| odd("the new issue", "no identifier"))?;
        if let Some(id) = issue["id"].as_str() {
            self.ids.borrow_mut().insert(identifier.clone(), id.to_string());
        }
        Ok(Created { identifier, url: issue["url"].as_str().and_then(clean::url) })
    }
}
