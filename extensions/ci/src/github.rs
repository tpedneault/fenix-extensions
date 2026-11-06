//! GitHub Actions over GitHub's REST API: a branch's workflow runs, a
//! run's jobs and their steps, a job's log, and re-running and cancelling
//! -- on github.com (`https://api.github.com`) or GitHub Enterprise
//! Server (`https://<host>/api/v3`).

use serde_json::Value;

use crate::model::{parse_time, Job, Run, State, Step};

/// How many runs a listing asks for: the branch's latest, which is what
/// the page shows.
pub const RUNS: usize = 20;

/// The API's root for a GitHub at `web` (`https://github.com`, or an
/// Enterprise Server's own address).
pub fn api_root(web: &str) -> String {
    let web = web.trim().trim_end_matches('/');
    if web.is_empty() || web.eq_ignore_ascii_case("https://github.com") || web.eq_ignore_ascii_case("http://github.com") {
        "https://api.github.com".to_string()
    } else if web.ends_with("/api/v3") {
        web.to_string()
    } else {
        format!("{web}/api/v3")
    }
}

/// `GET /repos/{owner}/{repo}/actions/runs?branch=...`: the branch's latest runs.
pub fn runs_url(api: &str, repo: &str, branch: &str) -> String {
    fenix_forge::with_query(&format!("{api}/repos/{repo}/actions/runs"), &[("branch", branch), ("per_page", &RUNS.to_string())])
}

/// A run's jobs, its latest attempt's.
pub fn jobs_url(api: &str, repo: &str, run: u64) -> String {
    format!("{api}/repos/{repo}/actions/runs/{run}/jobs?per_page=100")
}

/// A job's log: GitHub answers with a redirect to a signed download.
pub fn log_url(api: &str, repo: &str, job: u64) -> String {
    format!("{api}/repos/{repo}/actions/jobs/{job}/logs")
}

/// Re-runs a run's failed jobs (and what depends on them).
pub fn rerun_failed_url(api: &str, repo: &str, run: u64) -> String {
    format!("{api}/repos/{repo}/actions/runs/{run}/rerun-failed-jobs")
}

/// Re-runs one job.
pub fn rerun_job_url(api: &str, repo: &str, job: u64) -> String {
    format!("{api}/repos/{repo}/actions/jobs/{job}/rerun")
}

/// Cancels a run: GitHub cancels runs, not single jobs.
pub fn cancel_url(api: &str, repo: &str, run: u64) -> String {
    format!("{api}/repos/{repo}/actions/runs/{run}/cancel")
}

/// A run's, a job's or a step's state from its `status` and `conclusion`.
pub fn state(status: &str, conclusion: &str) -> State {
    match status {
        "completed" => match conclusion {
            "success" => State::Success,
            "failure" | "timed_out" | "startup_failure" => State::Failed,
            "cancelled" => State::Cancelled,
            "skipped" | "stale" => State::Skipped,
            "action_required" => State::Manual,
            _ => State::Neutral,
        },
        "in_progress" => State::Running,
        // queued, requested, waiting, pending
        _ => State::Queued,
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn time(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_str).and_then(parse_time)
}

/// One of `workflow_runs`.
pub fn run(v: &Value) -> Option<Run> {
    let state = state(&text(v, "status"), &text(v, "conclusion"));
    let title = match text(v, "display_title") {
        t if t.is_empty() => v.pointer("/head_commit/message").and_then(Value::as_str).unwrap_or_default().lines().next().unwrap_or_default().to_string(),
        t => t,
    };
    Some(Run {
        id: v.get("id")?.as_u64()?,
        name: text(v, "name"),
        title,
        number: v.get("run_number").and_then(Value::as_u64).unwrap_or(0),
        attempt: v.get("run_attempt").and_then(Value::as_u64).unwrap_or(1) as u32,
        event: text(v, "event"),
        state,
        branch: text(v, "head_branch"),
        sha: text(v, "head_sha"),
        url: text(v, "html_url"),
        created: time(v, "created_at"),
        started: time(v, "run_started_at").or_else(|| time(v, "created_at")),
        // A finished run's last update is when it finished.
        finished: if state.done() { time(v, "updated_at") } else { None },
        workflow: v.get("workflow_id").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// `GET .../actions/runs`: its runs, newest first as GitHub gives them.
pub fn runs(v: &Value) -> Vec<Run> {
    v.get("workflow_runs").and_then(Value::as_array).map(|l| l.iter().filter_map(run).collect()).unwrap_or_default()
}

fn step(v: &Value) -> Option<Step> {
    Some(Step {
        number: v.get("number")?.as_u64()? as u32,
        name: text(v, "name"),
        state: state(&text(v, "status"), &text(v, "conclusion")),
        started: time(v, "started_at"),
        finished: time(v, "completed_at"),
    })
}

/// One of `jobs`.
pub fn job(v: &Value) -> Option<Job> {
    let state = state(&text(v, "status"), &text(v, "conclusion"));
    Some(Job {
        id: v.get("id")?.as_u64()?,
        name: text(v, "name"),
        stage: String::new(),
        state,
        // A queued job's `started_at` is when it was queued.
        started: if state == State::Queued { None } else { time(v, "started_at") },
        finished: time(v, "completed_at"),
        url: text(v, "html_url"),
        allow_failure: false,
        steps: v.get("steps").and_then(Value::as_array).map(|l| l.iter().filter_map(step).collect()).unwrap_or_default(),
    })
}

/// `GET .../runs/{id}/jobs`.
pub fn jobs(v: &Value) -> Vec<Job> {
    v.get("jobs").and_then(Value::as_array).map(|l| l.iter().filter_map(job).collect()).unwrap_or_default()
}

/// GitHub's own message for a failed request ("Bad credentials").
pub fn message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body).ok()?.get("message")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Value {
        let text = match name {
            "runs" => include_str!("samples/github_runs.json"),
            _ => include_str!("samples/github_jobs.json"),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_api_is_api_github_com_or_an_enterprise_servers_own() {
        assert_eq!(api_root("https://github.com"), "https://api.github.com");
        assert_eq!(api_root(""), "https://api.github.com");
        assert_eq!(api_root("https://github.acme.dev/"), "https://github.acme.dev/api/v3");
        assert_eq!(api_root("https://github.acme.dev/api/v3"), "https://github.acme.dev/api/v3");
        assert_eq!(runs_url("https://api.github.com", "acme/rocket", "feature/telemetry"), "https://api.github.com/repos/acme/rocket/actions/runs?branch=feature%2Ftelemetry&per_page=20");
        assert_eq!(rerun_failed_url("https://api.github.com", "acme/rocket", 7), "https://api.github.com/repos/acme/rocket/actions/runs/7/rerun-failed-jobs");
    }

    #[test]
    fn workflow_runs_are_read_with_their_state_and_times() {
        let runs = runs(&sample("runs"));
        assert_eq!(runs.len(), 3);
        let ci = &runs[0];
        assert_eq!((ci.id, ci.name.as_str(), ci.number, ci.state), (11807430219, "CI", 482, State::Running));
        assert_eq!(ci.title, "Decode PUS-17 replies");
        assert_eq!(ci.sha, "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10");
        assert_eq!(ci.url, "https://github.com/acme/rocket/actions/runs/11807430219");
        assert_eq!(ci.started, parse_time("2026-11-06T09:41:07Z"));
        assert_eq!(ci.finished, None, "still running");
        assert_eq!(ci.workflow, 70321664);
        assert_eq!((runs[1].state, runs[1].finished), (State::Success, parse_time("2026-11-06T09:42:19Z")));
        assert_eq!((runs[2].state, runs[2].attempt), (State::Failed, 2));
    }

    #[test]
    fn jobs_and_their_steps_are_read() {
        let jobs = jobs(&sample("jobs"));
        let states: Vec<State> = jobs.iter().map(|j| j.state).collect();
        assert_eq!(states, [State::Success, State::Failed, State::Running, State::Queued]);
        let test = &jobs[1];
        assert_eq!(test.name, "test (ubuntu-latest)");
        assert_eq!(test.steps.len(), 5);
        assert_eq!((test.steps[2].name.as_str(), test.steps[2].state), ("Run cargo test --workspace", State::Failed));
        assert_eq!(test.steps[3].state, State::Skipped);
        assert_eq!(jobs[2].steps[2].state, State::Queued, "a pending step");
        assert_eq!(jobs[3].started, None, "a queued job hasn't started");
    }

    #[test]
    fn every_conclusion_has_a_state() {
        assert_eq!(state("completed", "timed_out"), State::Failed);
        assert_eq!(state("completed", "cancelled"), State::Cancelled);
        assert_eq!(state("completed", "neutral"), State::Neutral);
        assert_eq!(state("completed", "action_required"), State::Manual);
        assert_eq!(state("waiting", ""), State::Queued);
        assert_eq!(message(r#"{"message":"Bad credentials","documentation_url":"x"}"#).as_deref(), Some("Bad credentials"));
    }
}
