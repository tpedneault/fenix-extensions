//! GitLab pipelines over `/api/v4`: a branch's pipelines, a pipeline's
//! jobs, a job's trace, and retrying and cancelling -- on gitlab.com or a
//! GitLab of your own.

use serde_json::Value;

use crate::model::{parse_time, Job, Run, State};

/// How many pipelines a listing asks for.
pub const PIPELINES: usize = 20;

fn api(base: &str, project: &str) -> String {
    format!("{}/api/v4/projects/{}", base.trim_end_matches('/'), fenix_forge::gitlab::remote::encode_project(project))
}

/// `GET /projects/:id/pipelines?ref=...`: the branch's latest pipelines.
pub fn pipelines_url(base: &str, project: &str, branch: &str) -> String {
    fenix_forge::with_query(&format!("{}/pipelines", api(base, project)), &[("ref", branch), ("per_page", &PIPELINES.to_string()), ("order_by", "id"), ("sort", "desc")])
}

/// A pipeline's jobs (retried ones left out, as GitLab does by default).
pub fn jobs_url(base: &str, project: &str, pipeline: u64) -> String {
    format!("{}/pipelines/{pipeline}/jobs?per_page=100", api(base, project))
}

/// A job's log.
pub fn trace_url(base: &str, project: &str, job: u64) -> String {
    format!("{}/jobs/{job}/trace", api(base, project))
}

/// Retries a pipeline's failed and cancelled jobs.
pub fn retry_pipeline_url(base: &str, project: &str, pipeline: u64) -> String {
    format!("{}/pipelines/{pipeline}/retry", api(base, project))
}

pub fn retry_job_url(base: &str, project: &str, job: u64) -> String {
    format!("{}/jobs/{job}/retry", api(base, project))
}

pub fn cancel_pipeline_url(base: &str, project: &str, pipeline: u64) -> String {
    format!("{}/pipelines/{pipeline}/cancel", api(base, project))
}

pub fn cancel_job_url(base: &str, project: &str, job: u64) -> String {
    format!("{}/jobs/{job}/cancel", api(base, project))
}

/// GitLab's statuses, as its pipelines and jobs APIs document them.
pub fn state(raw: &str) -> State {
    match raw {
        "success" => State::Success,
        "failed" => State::Failed,
        "running" | "canceling" => State::Running,
        "canceled" | "cancelled" => State::Cancelled,
        "skipped" => State::Skipped,
        "manual" | "scheduled" => State::Manual,
        // created, pending, preparing, waiting_for_resource, waiting_for_callback
        _ => State::Queued,
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn time(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_str).and_then(parse_time)
}

/// One pipeline of a listing. A listing carries no start or finish:
/// those come from its jobs (`with_jobs`).
pub fn pipeline(v: &Value) -> Option<Run> {
    let state = state(&text(v, "status"));
    let name = match text(v, "name") {
        n if n.is_empty() => "Pipeline".to_string(),
        n => n,
    };
    Some(Run {
        id: v.get("id")?.as_u64()?,
        name,
        title: String::new(),
        number: v.get("iid").and_then(Value::as_u64).unwrap_or(0),
        attempt: 1,
        event: text(v, "source"),
        state,
        branch: text(v, "ref"),
        sha: text(v, "sha"),
        url: text(v, "web_url"),
        created: time(v, "created_at"),
        started: time(v, "started_at").or_else(|| time(v, "created_at")),
        finished: time(v, "finished_at").or_else(|| if state.done() { time(v, "updated_at") } else { None }),
        workflow: 0,
    })
}

/// `GET .../pipelines`.
pub fn pipelines(v: &Value) -> Vec<Run> {
    v.as_array().map(|l| l.iter().filter_map(pipeline).collect()).unwrap_or_default()
}

/// One job of a pipeline.
pub fn job(v: &Value) -> Option<Job> {
    Some(Job {
        id: v.get("id")?.as_u64()?,
        name: text(v, "name"),
        stage: text(v, "stage"),
        state: state(&text(v, "status")),
        started: time(v, "started_at"),
        finished: time(v, "finished_at"),
        url: text(v, "web_url"),
        allow_failure: v.get("allow_failure").and_then(Value::as_bool).unwrap_or(false),
        steps: Vec::new(),
    })
}

/// `GET .../pipelines/:id/jobs`, in the order the pipeline runs them: by
/// stage as they first appear (GitLab lists the newest first), then by id.
pub fn jobs(v: &Value) -> Vec<Job> {
    let mut jobs: Vec<Job> = v.as_array().map(|l| l.iter().filter_map(job).collect()).unwrap_or_default();
    jobs.sort_by_key(|j| j.id);
    let mut stages: Vec<String> = Vec::new();
    for j in &jobs {
        if !stages.contains(&j.stage) {
            stages.push(j.stage.clone());
        }
    }
    jobs.sort_by_key(|j| (stages.iter().position(|s| *s == j.stage), j.id));
    jobs
}

/// GitLab's own message for a failed request (`{"message": "403 Forbidden"}`).
pub fn message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let m = v.get("message").or_else(|| v.get("error"))?;
    Some(m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_name_the_project_by_its_encoded_path() {
        let base = "https://gitlab.example.com/";
        assert_eq!(pipelines_url(base, "space/ground/rocket", "feature/telemetry"), "https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/pipelines?ref=feature%2Ftelemetry&per_page=20&order_by=id&sort=desc");
        assert_eq!(trace_url(base, "g/p", 5), "https://gitlab.example.com/api/v4/projects/g%2Fp/jobs/5/trace");
        assert_eq!(retry_pipeline_url(base, "g/p", 9), "https://gitlab.example.com/api/v4/projects/g%2Fp/pipelines/9/retry");
    }

    #[test]
    fn pipelines_are_read_with_their_state() {
        let runs = pipelines(&serde_json::from_str(include_str!("samples/gitlab_pipelines.json")).unwrap());
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].id, runs[0].number, runs[0].state, runs[0].name.as_str()), (1468213077, 2214, State::Failed, "Pipeline"));
        assert_eq!(runs[0].finished, parse_time("2026-11-06T09:39:55.871Z"));
        assert_eq!(runs[1].name, "Nightly checks");
        assert_eq!((runs[2].state, runs[2].event.as_str()), (State::Cancelled, "web"));
        assert_eq!(runs[2].created, parse_time("2026-11-05T09:02:00Z"), "an offset is read");
    }

    #[test]
    fn jobs_come_in_stage_order_with_allowed_failures_marked() {
        let jobs = jobs(&serde_json::from_str(include_str!("samples/gitlab_jobs.json")).unwrap());
        let names: Vec<&str> = jobs.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(names, ["build", "lint: markdown", "unit tests", "deploy: staging"]);
        assert!(jobs[1].state == State::Failed && jobs[1].allow_failure && !jobs[1].failed());
        assert!(jobs[2].failed());
        assert_eq!(jobs[3].state, State::Manual);
        assert_eq!(jobs[2].stage, "test");
    }

    #[test]
    fn every_status_has_a_state() {
        assert_eq!(state("waiting_for_resource"), State::Queued);
        assert_eq!(state("canceling"), State::Running);
        assert_eq!(state("scheduled"), State::Manual);
        assert_eq!(message(r#"{"message":"403 Forbidden"}"#).as_deref(), Some("403 Forbidden"));
        assert_eq!(message(r#"{"message":{"base":["Pipeline is not retryable"]}}"#).as_deref(), Some(r#"{"base":["Pipeline is not retryable"]}"#));
    }
}
