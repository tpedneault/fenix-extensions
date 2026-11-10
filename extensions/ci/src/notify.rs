//! When a run for your branch finishes, what to say: it watches the runs
//! that count go from running to done, and makes the notification --
//! passed quietly, failed loudly with its log a key away. One tag per
//! branch, so a new status replaces the last.

use std::collections::{BTreeSet, HashMap};

use crate::model::{duration, short, took, Job, Run, State};
use crate::summary::{summarise, Overall};

/// Which runs it has seen unfinished.
#[derive(Debug, Default)]
pub struct Watcher {
    branch: Option<String>,
    active: BTreeSet<u64>,
}

/// How a notification wants you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loudness {
    Low,
    Normal,
    Critical,
}

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub summary: String,
    pub body: String,
    pub loudness: Loudness,
    /// (label, command): its own, or `url:https://...`.
    pub actions: Vec<(String, String)>,
    pub tag: String,
    /// The failed job "Open the log" opens: (run, job).
    pub failed: Option<(u64, u64)>,
}

/// What the settings say to notify of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub success: bool,
    pub failure: bool,
    pub critical: bool,
}

impl Default for Wanted {
    fn default() -> Self {
        Wanted { success: true, failure: true, critical: true }
    }
}

/// The command "Open the log" runs.
pub const OPEN_LOG: &str = "ci.failed_log";

pub fn tag(branch: &str) -> String {
    format!("ci:{branch}")
}

impl Watcher {
    /// The runs that count for `branch` now: whether they've just all
    /// finished, having been seen running. The first look at a branch
    /// says nothing -- what finished before it looked isn't news.
    pub fn observe(&mut self, branch: &str, current: &[&Run]) -> bool {
        let unfinished: BTreeSet<u64> = current.iter().filter(|r| !r.state.done()).map(|r| r.id).collect();
        if self.branch.as_deref() != Some(branch) {
            self.branch = Some(branch.to_string());
            self.active = unfinished;
            return false;
        }
        let finished_now = current.iter().any(|r| r.state.done() && self.active.contains(&r.id));
        self.active = unfinished;
        finished_now && self.active.is_empty() && !current.is_empty()
    }
}

/// What to say of `branch`'s runs, all finished, or None when the
/// settings say nothing.
pub fn note(branch: &str, current: &[&Run], jobs: &HashMap<u64, Vec<Job>>, wanted: Wanted, now: i64) -> Option<Note> {
    let summary = summarise(current, jobs, false);
    let first = current.first()?;
    let took = current.iter().filter_map(|r| took(r.started, r.finished, now)).max().map(duration);
    let names = current.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
    // A Jenkins build Git had no part in names no commit.
    let (at, passed) = match short(&first.sha) {
        "" => (branch.to_string(), names.clone()),
        sha => (format!("{branch} at {sha}"), format!("{names} · {sha}")),
    };
    match summary.overall {
        Overall::Success if wanted.success => Some(Note {
            summary: format!("CI passed on {branch}"),
            body: match took {
                Some(t) => format!("{passed} · in {t}"),
                None => passed,
            },
            loudness: Loudness::Low,
            actions: vec![("Open in the browser".into(), format!("url:{}", first.url))],
            tag: tag(branch),
            failed: None,
        }),
        Overall::Failed if wanted.failure => {
            let failed: Vec<(&Run, &Job)> = current.iter().flat_map(|r| jobs.get(&r.id).into_iter().flatten().filter(|j| j.failed()).map(move |j| (*r, j))).collect();
            let run = failed.first().map(|(r, _)| *r).or_else(|| current.iter().copied().find(|r| r.state == State::Failed)).unwrap_or(first);
            let what = match failed.len() {
                0 => format!("{} failed", run.name),
                1 => format!("{} failed", failed[0].1.name),
                n => format!("{} and {} more failed", failed[0].1.name, n - 1),
            };
            let mut actions = Vec::new();
            if !failed.is_empty() {
                actions.push(("Open the log".to_string(), OPEN_LOG.to_string()));
            }
            let url = failed.first().map(|(_, j)| j.url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| run.url.clone());
            actions.push(("Open in the browser".to_string(), format!("url:{url}")));
            Some(Note {
                summary: format!("CI failed on {branch}"),
                body: format!("{what} · {at}"),
                loudness: if wanted.critical { Loudness::Critical } else { Loudness::Normal },
                actions,
                tag: tag(branch),
                failed: failed.first().map(|(r, j)| (r.id, j.id)),
            })
        }
        Overall::Cancelled if wanted.failure => Some(Note {
            summary: format!("CI cancelled on {branch}"),
            body: format!("{names} · {at}"),
            loudness: Loudness::Normal,
            actions: vec![("Open in the browser".into(), format!("url:{}", first.url))],
            tag: tag(branch),
            failed: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::current;
    use crate::{github, model::parse_time};

    const HEAD: &str = "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10";

    fn runs() -> Vec<Run> {
        github::runs(&serde_json::from_str(include_str!("samples/github_runs.json")).unwrap())
    }

    fn jobs() -> Vec<Job> {
        github::jobs(&serde_json::from_str(include_str!("samples/github_jobs.json")).unwrap())
    }

    #[test]
    fn only_a_run_seen_running_that_finishes_is_news() {
        let mut w = Watcher::default();
        let mut all = runs();
        assert!(!w.observe("feature/telemetry", &current(&all, Some(HEAD)).0), "the first look");
        assert!(!w.observe("feature/telemetry", &current(&all, Some(HEAD)).0), "still running");
        all[0].state = State::Failed;
        assert!(w.observe("feature/telemetry", &current(&all, Some(HEAD)).0));
        assert!(!w.observe("feature/telemetry", &current(&all, Some(HEAD)).0), "once");
        // Finished before it looked at another branch: nothing.
        let mut w = Watcher::default();
        w.observe("feature/telemetry", &current(&all, Some(HEAD)).0);
        all[0].state = State::Success;
        assert!(!w.observe("feature/telemetry", &current(&all, Some(HEAD)).0));
    }

    #[test]
    fn a_failure_is_critical_and_opens_the_failed_jobs_log() {
        let mut all = runs();
        all[0].state = State::Failed;
        all[0].finished = parse_time("2026-11-06T09:46:07Z");
        let mut map = HashMap::new();
        map.insert(all[0].id, jobs());
        let (cur, _) = current(&all, Some(HEAD));
        let n = note("feature/telemetry", &cur, &map, Wanted::default(), 0).unwrap();
        assert_eq!(n.summary, "CI failed on feature/telemetry");
        assert_eq!(n.body, "test (ubuntu-latest) failed · feature/telemetry at 9f1c2e7");
        assert_eq!(n.loudness, Loudness::Critical);
        assert_eq!(n.tag, "ci:feature/telemetry");
        assert_eq!(n.actions, [
            ("Open the log".to_string(), "ci.failed_log".to_string()),
            ("Open in the browser".to_string(), "url:https://github.com/acme/rocket/actions/runs/11807430219/job/32897410321".to_string()),
        ]);
        assert_eq!(n.failed, Some((11807430219, 32897410321)));
        let normal = note("feature/telemetry", &cur, &map, Wanted { critical: false, ..Wanted::default() }, 0).unwrap();
        assert_eq!(normal.loudness, Loudness::Normal);
        assert_eq!(note("feature/telemetry", &cur, &map, Wanted { failure: false, ..Wanted::default() }, 0), None);
    }

    #[test]
    fn a_pass_is_quiet_and_says_how_long_it_took() {
        let mut all = runs();
        all[0].state = State::Success;
        all[0].finished = parse_time("2026-11-06T09:46:19Z");
        let (cur, _) = current(&all, Some(HEAD));
        let n = note("feature/telemetry", &cur, &HashMap::new(), Wanted::default(), 0).unwrap();
        assert_eq!((n.summary.as_str(), n.body.as_str(), n.loudness), ("CI passed on feature/telemetry", "CI, Lint · 9f1c2e7 · in 5m 12s", Loudness::Low));
        assert_eq!(n.actions, [("Open in the browser".to_string(), "url:https://github.com/acme/rocket/actions/runs/11807430219".to_string())]);
        assert_eq!(note("feature/telemetry", &cur, &HashMap::new(), Wanted { success: false, ..Wanted::default() }, 0), None);
    }
}
