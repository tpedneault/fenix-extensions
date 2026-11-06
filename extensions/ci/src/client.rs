//! Talking to the forge: its runs, jobs and logs, re-running and
//! cancelling, through [`Http`] -- Fenix's own client in the extension, a
//! fake in tests. Every `GET` of JSON is asked with the `ETag` it last
//! came with (`If-None-Match`), so a poll that finds nothing new is a
//! `304` that costs GitHub's rate limit nothing; a rate limit's reset is
//! kept for the poller.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fenix_forge::{Http, Response};
use serde_json::Value;

use crate::model::{Job, Provider, Run};
use crate::remote::Repo;
use crate::summary;
use crate::{github, gitlab, log, poll};

/// What it remembers between asks, shared by every client of a repository.
#[derive(Debug, Default)]
pub struct Memory {
    /// A URL's last answer, and its ETag.
    etags: HashMap<String, (String, String)>,
    /// When a rate limit lifts (ms since the epoch).
    pub blocked_until: Option<i64>,
}

pub type Shared = Rc<RefCell<Memory>>;

/// The forge for a repository.
pub struct Client<H> {
    http: H,
    pub repo: Repo,
    token: Option<String>,
    memory: Shared,
    /// Now, in ms since the epoch: for rate limits.
    clock: fn() -> i64,
}

/// What a poll found: the branch's latest runs, newest first, and the
/// jobs of those that count.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub runs: Vec<Run>,
    pub jobs: HashMap<u64, Vec<Job>>,
}

/// The most runs a poll reads the jobs of: the latest of each workflow
/// for the commit that counts.
const MOST_RUNS_WITH_JOBS: usize = 8;

impl<H: Http> Client<H> {
    pub fn new(http: H, repo: Repo, token: Option<String>, memory: Shared, clock: fn() -> i64) -> Self {
        Client { http, repo, token: token.filter(|t| !t.trim().is_empty()), memory, clock }
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        let mut h = Vec::new();
        match self.repo.provider {
            Provider::GitHub => {
                h.push(("Accept", "application/vnd.github+json".to_string()));
                h.push(("X-GitHub-Api-Version", "2022-11-28".to_string()));
                h.push(("User-Agent", "fenix-ci".to_string()));
                if let Some(t) = &self.token {
                    h.push(("Authorization", format!("Bearer {t}")));
                }
            }
            Provider::GitLab => {
                h.push(("Accept", "application/json".to_string()));
                if let Some(t) = &self.token {
                    h.push(("PRIVATE-TOKEN", t.clone()));
                }
            }
        }
        h
    }

    /// A request, its rate limit noted; an error status as the forge says it.
    async fn send(&self, method: &str, url: &str, extra: &[(&'static str, String)], body: Option<String>) -> Result<Response, String> {
        let mut headers = self.headers();
        headers.extend(extra.iter().cloned());
        let response = self.http.request(method, url, &headers, body).await?;
        let limited = poll::rate_limited_until(response.status, &response.headers, (self.clock)());
        if limited.is_some() || response.status < 400 {
            self.memory.borrow_mut().blocked_until = limited;
        }
        if response.status >= 400 && response.status != 416 {
            return Err(self.describe(&response, limited.is_some()));
        }
        Ok(response)
    }

    fn describe(&self, response: &Response, limited: bool) -> String {
        let said = match self.repo.provider {
            Provider::GitHub => github::message(&response.body),
            Provider::GitLab => gitlab::message(&response.body),
        };
        let code = response.status;
        let what = match (code, limited) {
            (_, true) => "rate limited: it waits until the limit lifts".to_string(),
            (401, _) => format!("{} refused the token: set it in SPC , (Forges)", self.forge()),
            (403, _) if self.token.is_none() => format!("{} wants a token: set it in SPC , (Forges)", self.forge()),
            (404, _) if self.token.is_none() => format!("{} has no such project, or it's private and wants a token (SPC , Forges)", self.forge()),
            _ => fenix_forge::reason(code).to_string(),
        };
        match said {
            Some(m) if !m.is_empty() => format!("HTTP {code} ({what}): {m}"),
            _ => format!("HTTP {code} ({what})"),
        }
    }

    fn forge(&self) -> &'static str {
        match self.repo.provider {
            Provider::GitHub => "GitHub",
            Provider::GitLab => "GitLab",
        }
    }

    /// JSON at `url`, asked with its last ETag: a `304` is the last answer.
    async fn get_json(&self, url: &str) -> Result<Value, String> {
        let known = self.memory.borrow().etags.get(url).cloned();
        let extra: Vec<(&'static str, String)> = known.iter().map(|(etag, _)| ("If-None-Match", etag.clone())).collect();
        let response = self.send("GET", url, &extra, None).await?;
        let body = match (response.status, known) {
            (304, Some((_, body))) => body,
            (304, None) => return Err("HTTP 304 with nothing to go on".into()),
            _ => {
                let etag = response.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("etag")).map(|(_, v)| v.clone());
                let mut memory = self.memory.borrow_mut();
                match etag {
                    Some(etag) => {
                        // Old URLs (runs long gone) don't pile up.
                        if memory.etags.len() > 256 {
                            memory.etags.clear();
                        }
                        memory.etags.insert(url.to_string(), (etag, response.body.clone()));
                    }
                    None => {
                        memory.etags.remove(url);
                    }
                }
                response.body
            }
        };
        serde_json::from_str(&body).map_err(|e| format!("couldn't read {}'s answer: {e}", self.forge()))
    }

    /// The branch's latest runs, newest first.
    pub async fn runs(&self, branch: &str) -> Result<Vec<Run>, String> {
        let (api, project) = (&self.repo.api, &self.repo.project);
        Ok(match self.repo.provider {
            Provider::GitHub => github::runs(&self.get_json(&github::runs_url(api, project, branch)).await?),
            Provider::GitLab => gitlab::pipelines(&self.get_json(&gitlab::pipelines_url(api, project, branch)).await?),
        })
    }

    /// A run's jobs.
    pub async fn jobs(&self, run: u64) -> Result<Vec<Job>, String> {
        let (api, project) = (&self.repo.api, &self.repo.project);
        Ok(match self.repo.provider {
            Provider::GitHub => github::jobs(&self.get_json(&github::jobs_url(api, project, run)).await?),
            Provider::GitLab => gitlab::jobs(&self.get_json(&gitlab::jobs_url(api, project, run)).await?),
        })
    }

    /// The branch's runs, and the jobs of those that count for `head`
    /// (and of `also`: runs open on a page).
    pub async fn snapshot(&self, branch: &str, head: Option<&str>, also: &[u64]) -> Result<Snapshot, String> {
        let mut runs = self.runs(branch).await?;
        let (current, _) = summary::current(&runs, head);
        let mut want: Vec<u64> = current.iter().take(MOST_RUNS_WITH_JOBS).map(|r| r.id).collect();
        want.extend(also.iter().filter(|id| runs.iter().any(|r| r.id == **id)));
        want.dedup();
        let mut jobs = HashMap::new();
        for id in want {
            if jobs.contains_key(&id) {
                continue;
            }
            // A run whose jobs can't be read still counts, as one.
            if let Ok(list) = self.jobs(id).await {
                jobs.insert(id, list);
            }
        }
        // GitLab's listing has no start or finish: its jobs do.
        for run in runs.iter_mut().filter(|_| self.repo.provider == Provider::GitLab) {
            if let Some(list) = jobs.get(&run.id) {
                if let Some(start) = list.iter().filter_map(|j| j.started).min() {
                    run.started = Some(start);
                }
                if run.state.done() {
                    if let Some(end) = list.iter().filter_map(|j| j.finished).max() {
                        run.finished = Some(end);
                    }
                }
            }
        }
        Ok(Snapshot { runs, jobs })
    }

    /// The end of a job's log, clean, and whether it's only its end.
    pub async fn log(&self, job: u64) -> Result<(String, bool), String> {
        let (api, project) = (&self.repo.api, &self.repo.project);
        let url = match self.repo.provider {
            Provider::GitHub => github::log_url(api, project, job),
            Provider::GitLab => gitlab::trace_url(api, project, job),
        };
        let range = [("Range", format!("bytes=-{}", log::TAIL_BYTES))];
        let mut response = self.send("GET", &url, &range, None).await?;
        if response.status == 416 {
            // Shorter than the range: the whole of it.
            response = self.send("GET", &url, &[], None).await?;
        }
        let partial = response.status == 206;
        let mut body = response.body.into_bytes();
        let cut = body.len() > log::TAIL_BYTES;
        if cut {
            body.drain(..body.len() - log::TAIL_BYTES);
        }
        Ok((log::clean(&log::text_of(&body, partial || cut)), partial || cut))
    }

    /// Re-runs `run`'s failed jobs, or `job` alone.
    pub async fn rerun(&self, run: u64, job: Option<u64>) -> Result<(), String> {
        let (api, project) = (&self.repo.api, &self.repo.project);
        let url = match (self.repo.provider, job) {
            (Provider::GitHub, None) => github::rerun_failed_url(api, project, run),
            (Provider::GitHub, Some(job)) => github::rerun_job_url(api, project, job),
            (Provider::GitLab, None) => gitlab::retry_pipeline_url(api, project, run),
            (Provider::GitLab, Some(job)) => gitlab::retry_job_url(api, project, job),
        };
        let body = (self.repo.provider == Provider::GitHub).then(|| "{}".to_string());
        self.send("POST", &url, &[], body).await.map(|_| ())
    }

    /// Cancels `run`, or on GitLab `job` alone (GitHub cancels whole runs).
    pub async fn cancel(&self, run: u64, job: Option<u64>) -> Result<(), String> {
        let (api, project) = (&self.repo.api, &self.repo.project);
        let url = match (self.repo.provider, job) {
            (Provider::GitHub, _) => github::cancel_url(api, project, run),
            (Provider::GitLab, None) => gitlab::cancel_pipeline_url(api, project, run),
            (Provider::GitLab, Some(job)) => gitlab::cancel_job_url(api, project, job),
        };
        self.send("POST", &url, &[], None).await.map(|_| ())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::cell::RefCell;

    /// What it was asked: method, URL, headers.
    pub type Asked = (String, String, Vec<(String, String)>);

    /// A forge that answers from a list, and keeps what it was asked.
    #[derive(Default, Clone)]
    pub struct FakeHttp {
        pub answers: Rc<RefCell<Vec<(String, Response)>>>,
        pub asked: Rc<RefCell<Vec<Asked>>>,
    }

    impl FakeHttp {
        /// `method url` answered with `status`, `body` and `headers`.
        pub fn on(&self, what: &str, status: u16, body: &str, headers: &[(&str, &str)]) {
            let headers = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            self.answers.borrow_mut().push((what.to_string(), Response { status, headers, body: body.to_string() }));
        }
    }

    impl Http for FakeHttp {
        async fn request(&self, method: &str, url: &str, headers: &[(&str, String)], _body: Option<String>) -> Result<Response, String> {
            self.asked.borrow_mut().push((method.to_string(), url.to_string(), headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()));
            let what = format!("{method} {url}");
            let mut answers = self.answers.borrow_mut();
            let i = answers.iter().position(|(w, _)| *w == what).ok_or_else(|| format!("nothing for {what}"))?;
            Ok(answers.remove(i).1)
        }
    }

    fn clock() -> i64 {
        1_000_000
    }

    pub fn github_repo() -> Repo {
        Repo { provider: Provider::GitHub, api: "https://api.github.com".into(), project: "acme/rocket".into(), host: "github.com".into() }
    }

    fn gitlab_repo() -> Repo {
        Repo { provider: Provider::GitLab, api: "https://gitlab.example.com".into(), project: "space/ground/rocket".into(), host: "gitlab.example.com".into() }
    }

    const RUNS: &str = "GET https://api.github.com/repos/acme/rocket/actions/runs?branch=feature%2Ftelemetry&per_page=20";
    const HEAD: &str = "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10";

    #[test]
    fn a_snapshot_reads_the_runs_and_the_jobs_of_those_that_count() {
        let http = FakeHttp::default();
        http.on(RUNS, 200, include_str!("samples/github_runs.json"), &[("ETag", "W/\"runs1\"")]);
        http.on("GET https://api.github.com/repos/acme/rocket/actions/runs/11807430219/jobs?per_page=100", 200, include_str!("samples/github_jobs.json"), &[]);
        http.on("GET https://api.github.com/repos/acme/rocket/actions/runs/11807430188/jobs?per_page=100", 200, r#"{"total_count":0,"jobs":[]}"#, &[]);
        let client = Client::new(http.clone(), github_repo(), Some("ghp_x".into()), Shared::default(), clock);
        let snap = block_on(client.snapshot("feature/telemetry", Some(HEAD), &[])).unwrap();
        assert_eq!(snap.runs.len(), 3);
        assert_eq!(snap.jobs.get(&11807430219).map(Vec::len), Some(4));
        assert!(!snap.jobs.contains_key(&11799214410), "an earlier commit's run isn't read");
        let asked = http.asked.borrow();
        assert!(asked[0].2.contains(&("Authorization".into(), "Bearer ghp_x".into())));
        assert!(!asked[0].2.iter().any(|(k, _)| k == "If-None-Match"));
    }

    #[test]
    fn a_poll_with_nothing_new_is_a_304_and_reads_as_the_last_answer() {
        let http = FakeHttp::default();
        let memory = Shared::default();
        http.on(RUNS, 200, include_str!("samples/github_runs.json"), &[("etag", "W/\"runs1\"")]);
        http.on(RUNS, 304, "", &[("etag", "W/\"runs1\"")]);
        let client = Client::new(http.clone(), github_repo(), None, memory, clock);
        let first = block_on(client.runs("feature/telemetry")).unwrap();
        let again = block_on(client.runs("feature/telemetry")).unwrap();
        assert_eq!(first, again);
        let asked = http.asked.borrow();
        assert!(asked[1].2.contains(&("If-None-Match".into(), "W/\"runs1\"".into())), "{:?}", asked[1]);
        assert!(!asked[1].2.iter().any(|(k, _)| k == "Authorization"), "no token, no header: a public repository");
    }

    #[test]
    fn a_rate_limit_is_kept_for_the_poller_and_said() {
        let http = FakeHttp::default();
        let memory = Shared::default();
        http.on(RUNS, 403, r#"{"message":"API rate limit exceeded for 203.0.113.9."}"#, &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1800")]);
        let client = Client::new(http, github_repo(), None, memory.clone(), clock);
        let err = block_on(client.runs("feature/telemetry")).unwrap_err();
        assert!(err.starts_with("HTTP 403 (rate limited") && err.contains("API rate limit exceeded"), "{err}");
        assert_eq!(memory.borrow().blocked_until, Some(1_800_000));
    }

    #[test]
    fn errors_say_what_to_do() {
        let http = FakeHttp::default();
        http.on(RUNS, 401, r#"{"message":"Bad credentials"}"#, &[]);
        let client = Client::new(http, github_repo(), Some("old".into()), Shared::default(), clock);
        let err = block_on(client.runs("feature/telemetry")).unwrap_err();
        assert_eq!(err, "HTTP 401 (GitHub refused the token: set it in SPC , (Forges)): Bad credentials");
    }

    #[test]
    fn gitlab_pipelines_get_their_times_from_their_jobs() {
        let http = FakeHttp::default();
        http.on("GET https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/pipelines?ref=feature%2Ftelemetry&per_page=20&order_by=id&sort=desc", 200, include_str!("samples/gitlab_pipelines.json"), &[]);
        http.on("GET https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/pipelines/1468213077/jobs?per_page=100", 200, include_str!("samples/gitlab_jobs.json"), &[]);
        let client = Client::new(http.clone(), gitlab_repo(), Some("glpat-x".into()), Shared::default(), clock);
        let snap = block_on(client.snapshot("feature/telemetry", Some("c0ffee1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b"), &[])).unwrap();
        let run = &snap.runs[0];
        assert_eq!(run.started, crate::model::parse_time("2026-11-06T09:30:15.000Z"), "its first job's start");
        assert!(http.asked.borrow()[0].2.contains(&("PRIVATE-TOKEN".into(), "glpat-x".into())));
    }

    #[test]
    fn a_log_is_asked_for_its_tail() {
        let http = FakeHttp::default();
        http.on("GET https://api.github.com/repos/acme/rocket/actions/jobs/5/logs", 206, &format!("cut line\n{}", include_str!("samples/github_log.txt")), &[]);
        let client = Client::new(http.clone(), github_repo(), None, Shared::default(), clock);
        let (text, partial) = block_on(client.log(5)).unwrap();
        assert!(partial);
        assert!(text.starts_with("##[group]Run cargo test"), "{text}");
        assert!(http.asked.borrow()[0].2.contains(&("Range".into(), format!("bytes=-{}", log::TAIL_BYTES))));
        // A log shorter than the range: 416, then the whole.
        http.on("GET https://api.github.com/repos/acme/rocket/actions/jobs/6/logs", 416, "", &[]);
        http.on("GET https://api.github.com/repos/acme/rocket/actions/jobs/6/logs", 200, "short\n", &[]);
        assert_eq!(block_on(client.log(6)).unwrap(), ("short\n".to_string(), false));
    }

    #[test]
    fn reruns_and_cancels_go_to_each_forges_endpoints() {
        let http = FakeHttp::default();
        http.on("POST https://api.github.com/repos/acme/rocket/actions/runs/7/rerun-failed-jobs", 201, "", &[]);
        http.on("POST https://api.github.com/repos/acme/rocket/actions/jobs/9/rerun", 201, "", &[]);
        http.on("POST https://api.github.com/repos/acme/rocket/actions/runs/7/cancel", 202, "", &[]);
        let gh = Client::new(http.clone(), github_repo(), Some("t".into()), Shared::default(), clock);
        block_on(gh.rerun(7, None)).unwrap();
        block_on(gh.rerun(7, Some(9))).unwrap();
        block_on(gh.cancel(7, Some(9))).unwrap();
        http.on("POST https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/pipelines/7/retry", 201, "{}", &[]);
        http.on("POST https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/jobs/9/cancel", 201, "{}", &[]);
        let gl = Client::new(http.clone(), gitlab_repo(), Some("t".into()), Shared::default(), clock);
        block_on(gl.rerun(7, None)).unwrap();
        block_on(gl.cancel(7, Some(9))).unwrap();
        http.on("POST https://gitlab.example.com/api/v4/projects/space%2Fground%2Frocket/jobs/9/retry", 403, r#"{"message":"403 Forbidden"}"#, &[]);
        assert_eq!(block_on(gl.rerun(7, Some(9))).unwrap_err(), "HTTP 403 (Forbidden): 403 Forbidden");
    }
}
