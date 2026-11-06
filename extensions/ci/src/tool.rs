//! The agents' tool (`ci_failures`): the branch's CI, its failing jobs
//! and the steps that failed, and the end of each one's log with the
//! lines that say what went wrong -- what a coding agent needs to fix it.

use std::collections::HashMap;

use serde_json::Value;

use crate::log;
use crate::model::{short, Job, Run};
use crate::summary::Summary;

/// The most failed jobs whose logs it reads.
pub const MOST_LOGS: usize = 4;

/// What the agent asked: how many lines of each log (default 80, at most 400).
pub fn lines_asked(payload: &str) -> usize {
    serde_json::from_str::<Value>(payload).ok().and_then(|v| v.get("lines").and_then(Value::as_u64)).map(|n| n.clamp(10, 400) as usize).unwrap_or(80)
}

/// The failed jobs of `current`, run by run.
pub fn failed<'a>(current: &[&'a Run], jobs: &'a HashMap<u64, Vec<Job>>) -> Vec<(&'a Run, &'a Job)> {
    current.iter().flat_map(|r| jobs.get(&r.id).into_iter().flatten().filter(|j| j.failed()).map(move |j| (*r, j))).collect()
}

/// The report, as Markdown.
pub struct Report<'a> {
    pub project: &'a str,
    pub forge: &'a str,
    pub branch: &'a str,
    pub head: Option<&'a str>,
    pub summary: &'a Summary,
    pub current: &'a [&'a Run],
    pub jobs: &'a HashMap<u64, Vec<Job>>,
    /// Each failed job's log, by its id: its text, or why it couldn't be read.
    pub logs: &'a HashMap<u64, Result<String, String>>,
    pub lines: usize,
}

impl Report<'_> {
    pub fn text(&self) -> String {
        let mut out = format!("# CI for {} ({}), branch {}\n\n", self.project, self.forge, self.branch);
        let sha = short(&self.summary.sha);
        out.push_str(&format!("Status: {} (commit {sha}", self.summary.sentence()));
        if let Some(h) = self.head {
            if self.summary.earlier {
                out.push_str(&format!("; HEAD {} has no runs yet", short(h)));
            }
        }
        out.push_str(")\n");
        for run in self.current {
            out.push_str(&format!("- {} #{}: {} -- {}\n", run.name, run.number, run.state.label(), run.url));
        }
        let failed = failed(self.current, self.jobs);
        if failed.is_empty() {
            out.push_str(if self.summary.active() { "\nNothing has failed so far; it's still running.\n" } else { "\nNo job failed.\n" });
            return out;
        }
        for (run, job) in &failed {
            out.push_str(&format!("\n## Failed: {} / {}\n\n", run.name, job.name));
            if !job.stage.is_empty() {
                out.push_str(&format!("Stage: {}\n", job.stage));
            }
            let steps: Vec<String> = job.steps.iter().filter(|s| s.state == crate::model::State::Failed).map(|s| format!("{} ({})", s.name, s.number)).collect();
            if !steps.is_empty() {
                out.push_str(&format!("Failed step: {}\n", steps.join(", ")));
            }
            out.push_str(&format!("Job: {}\n", job.url));
            match self.logs.get(&job.id) {
                Some(Ok(text)) => {
                    let errors = log::errors(text, 12);
                    if !errors.is_empty() {
                        out.push_str("\nErrors in the log:\n");
                        for e in errors {
                            out.push_str(&format!("- {e}\n"));
                        }
                    }
                    out.push_str(&format!("\nLast {} lines of the log:\n\n```text\n{}\n```\n", self.lines, log::tail(text, self.lines)));
                }
                Some(Err(e)) => out.push_str(&format!("\nIts log couldn't be read: {e}\n")),
                None => out.push_str(&format!("\nIts log wasn't read (only the first {MOST_LOGS} failed jobs' are).\n")),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::{current, summarise};
    use crate::{github, model::State};

    #[test]
    fn the_failed_jobs_come_with_their_errors_and_the_end_of_their_logs() {
        let mut runs = github::runs(&serde_json::from_str(include_str!("samples/github_runs.json")).unwrap());
        runs[0].state = State::Failed;
        let mut jobs = HashMap::new();
        jobs.insert(runs[0].id, github::jobs(&serde_json::from_str(include_str!("samples/github_jobs.json")).unwrap()));
        let head = "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10";
        let (cur, earlier) = current(&runs, Some(head));
        let summary = summarise(&cur, &jobs, earlier);
        let mut logs = HashMap::new();
        logs.insert(32897410321, Ok(log::clean(include_str!("samples/github_log.txt"))));
        let report = Report { project: "acme/rocket", forge: "GitHub Actions", branch: "feature/telemetry", head: Some(head), summary: &summary, current: &cur, jobs: &jobs, logs: &logs, lines: 5 }.text();
        assert!(report.starts_with("# CI for acme/rocket (GitHub Actions), branch feature/telemetry\n\nStatus: Failed: 1 job (commit 9f1c2e7)\n"), "{report}");
        assert!(report.contains("- CI #482: failed -- https://github.com/acme/rocket/actions/runs/11807430219\n- Lint #301: passed"), "{report}");
        assert!(report.contains("## Failed: CI / test (ubuntu-latest)\n\nFailed step: Run cargo test --workspace (3)\nJob: https://github.com/acme/rocket/actions/runs/11807430219/job/32897410321"), "{report}");
        assert!(report.contains("- thread 'pus::tests::service_17_reply' panicked at telemetry/src/pus.rs:212:9:"), "{report}");
        assert!(report.contains("Last 5 lines of the log:\n\n```text\n  left: Unknown(17)\n right: AreYouAlive\ntest result: FAILED."), "{report}");
        assert!(report.trim_end().ends_with("Process completed with exit code 101.\n```"), "{report}");
    }

    #[test]
    fn nothing_failed_says_so() {
        let runs = github::runs(&serde_json::from_str(include_str!("samples/github_runs.json")).unwrap());
        let (cur, _) = current(&runs, Some("9f1c2e7"));
        let summary = summarise(&cur, &HashMap::new(), false);
        let report = Report { project: "acme/rocket", forge: "GitHub Actions", branch: "b", head: None, summary: &summary, current: &cur, jobs: &HashMap::new(), logs: &HashMap::new(), lines: 80 }.text();
        assert!(report.contains("Nothing has failed so far; it's still running."), "{report}");
        assert_eq!(lines_asked(r#"{"lines": 1000}"#), 400);
        assert_eq!(lines_asked("{}"), 80);
        assert_eq!(lines_asked(r#"{"lines": 30}"#), 30);
    }
}
