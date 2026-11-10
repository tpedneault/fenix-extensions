//! Jenkins over its JSON API: a job's builds (a pipeline's, or a
//! multibranch pipeline's branch job), a build's stages and a failed
//! stage's steps (the Pipeline Stage View plugin's `wfapi`), its console,
//! and building again and stopping.
//!
//! Jenkins doesn't know a repository by its remote: the job is named in
//! the settings (`ci.jenkins_url`, `ci.jenkins_job`). A multibranch
//! pipeline (or a folder) has a job per branch, named by the branch; any
//! other job's builds are told apart by the branch Git built them from.

use serde_json::Value;

use crate::model::{Job, Run, State, Step};

/// How many builds the page shows.
pub const BUILDS: usize = 20;
/// How many it asks for: a job that builds every branch has others' among them.
const ASKED: usize = 40;
/// A stage's id within its build: a build's stages are numbered by its
/// flow's nodes, far fewer than this.
const NODE_BITS: u32 = 20;

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%').then(|| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The job `ci.jenkins_job` names, folder by folder: `space/rocket`,
/// `job/space/job/rocket`, or the job's whole address.
pub fn job_path(text: &str) -> Vec<String> {
    let text = text.trim();
    let path = match text.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, p)| p).unwrap_or(""),
        None => text,
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    // As its address has it: job/<folder>/job/<name>, after the server's own prefix.
    if let Some(at) = parts.iter().position(|p| *p == "job") {
        let rest = &parts[at..];
        if rest.len().is_multiple_of(2) &&rest.iter().step_by(2).all(|p| *p == "job") {
            return rest.iter().skip(1).step_by(2).map(|p| decode(p)).collect();
        }
    }
    if text.contains("://") {
        return Vec::new();
    }
    parts.iter().map(|p| p.to_string()).collect()
}

/// A job's address on the Jenkins at `base`.
pub fn job_url(base: &str, path: &[String]) -> String {
    let mut url = base.trim().trim_end_matches('/').to_string();
    for name in path {
        url.push_str("/job/");
        url.push_str(&encode(name));
    }
    url
}

/// A branch's job under a multibranch pipeline: named by the branch, its
/// `/` (and whatever else isn't a letter) percent-encoded -- so encoded
/// twice in the address.
pub fn branch_job_url(job: &str, branch: &str) -> String {
    format!("{job}/job/{}", encode(&encode(branch)))
}

/// A job's latest builds, and whether it's a job at all or a folder of them.
pub fn builds_url(job: &str) -> String {
    let tree = format!("_class,inQueue,nextBuildNumber,queueItem[why,inQueueSince],builds[number,url,result,building,timestamp,duration,actions[causes[_class,shortDescription],lastBuiltRevision[SHA1,branch[name]]],changeSets[items[msg]]]{{0,{ASKED}}}");
    fenix_forge::with_query(&format!("{job}/api/json"), &[("tree", &tree)])
}

/// One build: for one that has no stages.
pub fn build_url(job: &str, build: u64) -> String {
    fenix_forge::with_query(&format!("{job}/{build}/api/json"), &[("tree", "number,url,result,building,timestamp,duration")])
}

/// A build's stages (the Pipeline Stage View plugin).
pub fn stages_url(job: &str, build: u64) -> String {
    format!("{job}/{build}/wfapi/describe")
}

/// A stage's steps.
pub fn stage_url(job: &str, build: u64, node: u64) -> String {
    format!("{job}/{build}/execution/node/{node}/wfapi/describe")
}

/// A build's console, as text.
pub fn console_url(job: &str, build: u64) -> String {
    format!("{job}/{build}/consoleText")
}

/// Builds the job again; `with_parameters` for a job that has them (their defaults).
pub fn build_again_url(job: &str, with_parameters: bool) -> String {
    format!("{job}/{}", if with_parameters { "buildWithParameters" } else { "build" })
}

pub fn stop_url(job: &str, build: u64) -> String {
    format!("{job}/{build}/stop")
}

/// A stage's id among every build's: its build's number and its node's.
pub fn stage_id(build: u64, node: u64) -> u64 {
    build << NODE_BITS | (node & ((1 << NODE_BITS) - 1))
}

/// The build and the node of a stage's id; node 0 is the build as a whole.
pub fn split(id: u64) -> (u64, u64) {
    (id >> NODE_BITS, id & ((1 << NODE_BITS) - 1))
}

/// `user:token` as HTTP's Basic authorization has it.
pub fn basic(credentials: &str) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("Basic ");
    for chunk in credentials.as_bytes().chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Whether what `builds_url` answered is a multibranch pipeline or a
/// folder: its builds are its branch's job's.
pub fn is_folder(v: &Value) -> bool {
    let class = text(v, "_class");
    class.contains("MultiBranch") || class.contains("Folder")
}

/// A build's state from `building` and its `result`.
pub fn build_state(building: bool, result: &str) -> State {
    match result {
        _ if building => State::Running,
        "SUCCESS" => State::Success,
        // Unstable: it built, and its tests failed.
        "FAILURE" | "UNSTABLE" => State::Failed,
        "ABORTED" => State::Cancelled,
        "NOT_BUILT" => State::Skipped,
        _ => State::Queued,
    }
}

/// A stage's or a step's state, as the Stage View says it.
pub fn stage_state(status: &str) -> State {
    match status {
        "SUCCESS" => State::Success,
        "FAILED" | "UNSTABLE" => State::Failed,
        "IN_PROGRESS" => State::Running,
        "PAUSED_PENDING_INPUT" => State::Manual,
        "ABORTED" => State::Cancelled,
        "NOT_EXECUTED" => State::Skipped,
        _ => State::Queued,
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Milliseconds since the epoch at `key`, as seconds; none when it's 0.
fn seconds(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64).filter(|ms| *ms > 0).map(|ms| ms / 1000)
}

fn finished(v: &Value, start: &str, duration: &str, state: State) -> Option<i64> {
    let start_ms = v.get(start).and_then(Value::as_i64).filter(|ms| *ms > 0)?;
    let took = v.get(duration).and_then(Value::as_i64).unwrap_or(0);
    state.done().then_some((start_ms + took) / 1000)
}

/// Whether Git's name for what it built is `branch`: the branch's own (a
/// multibranch pipeline's), `origin/<branch>` as the Git plugin has it,
/// or a ref's. Not any name that ends with it: `feature/x` isn't `x`.
fn on_branch(name: &str, branch: &str) -> bool {
    let Some(before) = name.strip_suffix(branch) else { return false };
    let before = before.strip_prefix("refs/").unwrap_or(before);
    matches!(before, "" | "origin/" | "heads/" | "remotes/origin/")
}

/// What Git built for a build: (commit, the branches it was).
fn revisions(build: &Value) -> Vec<(String, Vec<String>)> {
    let actions = build.get("actions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    actions
        .iter()
        .filter_map(|a| a.get("lastBuiltRevision"))
        .map(|r| (text(r, "SHA1"), r.get("branch").and_then(Value::as_array).map(|l| l.iter().map(|b| text(b, "name")).collect()).unwrap_or_default()))
        .filter(|(sha, _)| !sha.is_empty())
        .collect()
}

/// What started a build, in a word, from its cause's class.
fn event(class: &str) -> &'static str {
    let name = class.rsplit(['.', '$']).next().unwrap_or(class);
    match name {
        "UserIdCause" => "manual",
        "ReplayCause" => "replay",
        "TimerTriggerCause" => "schedule",
        "UpstreamCause" | "BuildUpstreamCause" => "upstream",
        "BranchIndexingCause" => "indexing",
        "RemoteCause" => "remote",
        n if n.contains("SCMTrigger") || n.contains("BranchEvent") || n.contains("Push") || n.contains("WebHook") => "push",
        n if n.contains("PullRequest") || n.contains("MergeRequest") => "pull_request",
        _ => "",
    }
}

/// One of `builds`, as a run of `name` for `branch`. The commit is the
/// one Git built `branch` at: a build checks out other repositories too
/// (a shared library), which come first.
fn build(v: &Value, name: &str, branch: &str) -> Option<Run> {
    let number = v.get("number")?.as_u64()?;
    let state = build_state(v.get("building").and_then(Value::as_bool).unwrap_or(false), &text(v, "result"));
    let revisions = revisions(v);
    let sha = revisions.iter().find(|(_, names)| names.iter().any(|n| on_branch(n, branch))).or(revisions.last()).map(|(sha, _)| sha.clone()).unwrap_or_default();
    let cause = v.get("actions").and_then(Value::as_array).and_then(|l| l.iter().find_map(|a| a.get("causes")?.as_array()?.first()));
    let said = v.pointer("/changeSets/0/items/0/msg").and_then(Value::as_str).and_then(|m| m.lines().next()).map(str::to_string);
    let started = seconds(v, "timestamp");
    Some(Run {
        id: number,
        name: name.to_string(),
        title: said.or_else(|| cause.map(|c| text(c, "shortDescription"))).unwrap_or_default(),
        number,
        attempt: 1,
        event: cause.map(|c| event(&text(c, "_class")).to_string()).unwrap_or_default(),
        state,
        branch: branch.to_string(),
        sha,
        url: text(v, "url"),
        created: started,
        started,
        finished: finished(v, "timestamp", "duration", state),
        workflow: 0,
    })
}

/// `builds_url`'s answer as `branch`'s runs of the job `name`, newest
/// first. `shared`: the job builds every branch, so only the builds Git
/// made of `branch` are its (all of them, when Git made none).
pub fn builds(v: &Value, name: &str, branch: &str, shared: bool) -> Vec<Run> {
    let list = v.get("builds").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let git = list.iter().any(|b| !revisions(b).is_empty());
    let mut runs: Vec<Run> = Vec::new();
    // What waits in the queue is the next build; whose branch's, a shared job doesn't say.
    if !(shared && git) && v.get("inQueue").and_then(Value::as_bool).unwrap_or(false) {
        if let Some(number) = v.get("nextBuildNumber").and_then(Value::as_u64) {
            let queued = v.get("queueItem").cloned().unwrap_or(Value::Null);
            runs.push(Run { id: number, name: name.to_string(), title: text(&queued, "why"), number, attempt: 1, event: String::new(), state: State::Queued, branch: branch.to_string(), sha: String::new(), url: String::new(), created: seconds(&queued, "inQueueSince"), started: None, finished: None, workflow: 0 });
        }
    }
    let theirs = |b: &Value| {
        let revisions = revisions(b);
        match (shared && git, revisions.is_empty()) {
            (false, _) => true,
            // Not checked out yet: whose it is shows once it has.
            (true, true) => false,
            (true, false) => revisions.iter().any(|(_, names)| names.iter().any(|n| on_branch(n, branch))),
        }
    };
    let queued = runs.first().map(|q| q.id);
    runs.extend(list.iter().filter(|b| theirs(b)).filter_map(|b| build(b, name, branch)).filter(|r| queued != Some(r.id)));
    runs.truncate(BUILDS);
    runs
}

fn node(v: &Value) -> Option<u64> {
    let id = v.get("id")?;
    id.as_u64().or_else(|| id.as_str()?.parse().ok())
}

/// `stages_url`'s answer: build `build`'s stages, as its jobs.
pub fn stages(v: &Value, job: &str, build: u64) -> Vec<Job> {
    let list = v.get("stages").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .filter_map(|s| {
            let state = stage_state(&text(s, "status"));
            Some(Job {
                id: stage_id(build, node(s)?),
                name: text(s, "name"),
                stage: String::new(),
                state,
                started: seconds(s, "startTimeMillis").filter(|_| !matches!(state, State::Queued | State::Skipped)),
                finished: finished(s, "startTimeMillis", "durationMillis", state).filter(|_| state != State::Skipped),
                url: format!("{job}/{build}/console"),
                allow_failure: false,
                steps: Vec::new(),
            })
        })
        .collect()
}

/// `stage_url`'s answer: the stage's steps, each with what it ran.
pub fn steps(v: &Value) -> Vec<Step> {
    let list = v.get("stageFlowNodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .filter_map(|s| {
            let state = stage_state(&text(s, "status"));
            let ran = text(s, "parameterDescription");
            let name = match ran.lines().next().filter(|l| !l.trim().is_empty()) {
                Some(line) => format!("{}: {}", text(s, "name"), line.trim()),
                None => text(s, "name"),
            };
            Some(Step { number: node(s)? as u32, name, state, started: seconds(s, "startTimeMillis"), finished: finished(s, "startTimeMillis", "durationMillis", state) })
        })
        .collect()
}

/// `build_url`'s answer as the build's one job: a build with no stages
/// (not a pipeline, or a Jenkins without the Stage View) is its console.
pub fn whole(v: &Value, job: &str, build: u64) -> Job {
    let state = build_state(v.get("building").and_then(Value::as_bool).unwrap_or(false), &text(v, "result"));
    Job { id: stage_id(build, 0), name: "console".to_string(), stage: String::new(), state, started: seconds(v, "timestamp"), finished: finished(v, "timestamp", "duration", state), url: format!("{job}/{build}/console"), allow_failure: false, steps: Vec::new() }
}

/// The part of a pipeline's console that is stage `name`'s: from its
/// `[Pipeline] { (name)` to the `[Pipeline] }` that closes it. None when
/// it isn't there (the log's start was cut, or it never ran).
pub fn stage_log(console: &str, name: &str) -> Option<String> {
    let opens = format!("[Pipeline] {{ ({name})");
    let mut lines = console.lines().skip_while(|l| !l.trim_end().ends_with(&opens)).skip(1);
    let mut depth = 1;
    let mut out = String::new();
    for line in lines.by_ref() {
        if line.contains("[Pipeline] {") {
            depth += 1;
        } else if line.contains("[Pipeline] }") {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    (!out.trim().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_time;

    const JOB: &str = "https://ci.example.com/job/space/job/rocket/job/feature%252Ftelemetry";

    fn sample(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_job_is_named_by_its_folders_or_by_its_address() {
        assert_eq!(job_path("space/rocket"), ["space", "rocket"]);
        assert_eq!(job_path(" /job/space/job/rocket/ "), ["space", "rocket"]);
        assert_eq!(job_path("https://ci.example.com/jenkins/job/space/job/Rocket%20Ground/"), ["space", "Rocket Ground"]);
        assert_eq!(job_path("rocket"), ["rocket"]);
        assert!(job_path("https://ci.example.com/").is_empty());
        assert!(job_path("").is_empty());
        let job = job_url("https://ci.example.com/jenkins/", &job_path("space/Rocket Ground"));
        assert_eq!(job, "https://ci.example.com/jenkins/job/space/job/Rocket%20Ground");
        assert_eq!(branch_job_url("https://ci.example.com/job/space/job/rocket", "feature/telemetry"), JOB);
    }

    #[test]
    fn urls_ask_for_what_it_reads_and_no_more() {
        let url = builds_url(JOB);
        assert!(url.starts_with(&format!("{JOB}/api/json?tree=_class%2CinQueue%2C")), "{url}");
        assert!(url.ends_with("%7B0%2C40%7D") && !url.contains('['), "{url}");
        assert_eq!(stages_url(JOB, 13), format!("{JOB}/13/wfapi/describe"));
        assert_eq!(stage_url(JOB, 13, 23), format!("{JOB}/13/execution/node/23/wfapi/describe"));
        assert_eq!(console_url(JOB, 13), format!("{JOB}/13/consoleText"));
        assert_eq!((build_again_url(JOB, false), build_again_url(JOB, true)), (format!("{JOB}/build"), format!("{JOB}/buildWithParameters")));
        assert_eq!(stop_url(JOB, 14), format!("{JOB}/14/stop"));
    }

    #[test]
    fn a_user_and_a_token_go_as_basic_authorization() {
        assert_eq!(basic("alex:11aa"), "Basic YWxleDoxMWFh");
        assert_eq!(basic("a:b"), "Basic YTpi");
        assert_eq!(basic("ab:c"), "Basic YWI6Yw==");
    }

    #[test]
    fn builds_are_read_with_their_state_commit_and_what_started_them() {
        let runs = builds(&sample(include_str!("samples/jenkins_builds.json")), "rocket", "feature/telemetry", false);
        assert_eq!(runs.len(), 3);
        let b = &runs[0];
        assert_eq!((b.id, b.number, b.name.as_str(), b.state), (14, 14, "rocket", State::Running));
        assert_eq!(b.sha, "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10", "the branch's commit, not the shared library's");
        assert_eq!((b.title.as_str(), b.event.as_str()), ("Decode PUS-17 replies", "push"));
        assert_eq!((b.started, b.finished), (parse_time("2026-11-06T09:41:07Z"), None));
        assert_eq!(b.url, format!("{JOB}/14/"));
        let b = &runs[1];
        assert_eq!((b.state, b.event.as_str(), b.title.as_str()), (State::Failed, "manual", "Started by user Alex"));
        assert_eq!(b.finished, parse_time("2026-11-06T08:47:59Z"), "its start and how long it took");
        assert_eq!((runs[2].state, runs[2].event.as_str()), (State::Cancelled, "schedule"));
    }

    #[test]
    fn a_build_in_the_queue_is_the_next_one() {
        let mut v = sample(include_str!("samples/jenkins_builds.json"));
        v["inQueue"] = Value::Bool(true);
        v["queueItem"] = sample(r#"{"why":"Waiting for next available executor","inQueueSince":1793958100000}"#);
        let runs = builds(&v, "rocket", "feature/telemetry", false);
        assert_eq!((runs[0].id, runs[0].state, runs[0].title.as_str(), runs[0].sha.as_str()), (15, State::Queued, "Waiting for next available executor", ""));
        assert_eq!(runs.len(), 4);
        // A job every branch builds in doesn't say whose it is.
        assert_eq!(builds(&v, "rocket", "feature/telemetry", true)[0].id, 14);
    }

    #[test]
    fn a_job_every_branch_builds_in_gives_only_the_branchs_builds() {
        let v = sample(include_str!("samples/jenkins_builds.json"));
        // Git's name for it there: origin/<branch>.
        let text = include_str!("samples/jenkins_builds.json").replace(r#""name": "feature/telemetry""#, r#""name": "origin/feature/telemetry""#);
        assert_eq!(builds(&sample(&text), "rocket", "feature/telemetry", true).iter().map(|r| r.number).collect::<Vec<_>>(), [14, 13]);
        let runs = builds(&v, "rocket", "main", true);
        assert_eq!(runs.iter().map(|r| r.number).collect::<Vec<_>>(), [12], "the one main was built in");
        assert!(builds(&v, "rocket", "telemetry", true).is_empty(), "a branch's name whole");
        // A job Git builds nothing in: its builds are the job's.
        let plain = sample(r#"{"builds":[{"number":3,"building":false,"result":"UNSTABLE","timestamp":1793958067000,"duration":1000,"url":"u"}]}"#);
        let runs = builds(&plain, "nightly", "main", true);
        assert_eq!((runs.len(), runs[0].state, runs[0].sha.as_str()), (1, State::Failed, ""));
    }

    #[test]
    fn a_multibranch_pipeline_is_known_by_its_class() {
        assert!(is_folder(&sample(r#"{"_class":"org.jenkinsci.plugins.workflow.multibranch.WorkflowMultiBranchProject"}"#)));
        assert!(is_folder(&sample(r#"{"_class":"com.cloudbees.hudson.plugins.folder.Folder"}"#)));
        assert!(!is_folder(&sample(include_str!("samples/jenkins_builds.json"))));
    }

    #[test]
    fn stages_are_a_builds_jobs_and_a_stages_steps_say_what_they_ran() {
        let jobs = stages(&sample(include_str!("samples/jenkins_stages.json")), JOB, 13);
        let seen: Vec<(&str, State)> = jobs.iter().map(|j| (j.name.as_str(), j.state)).collect();
        assert_eq!(seen, [("Checkout", State::Success), ("Build", State::Success), ("Test", State::Failed), ("Deploy", State::Skipped)]);
        assert_eq!(split(jobs[2].id), (13, 23));
        assert_eq!(jobs[2].finished.zip(jobs[2].started).map(|(f, s)| f - s), Some(186));
        assert_eq!((jobs[3].started, jobs[3].finished), (None, None), "it never ran");
        assert_eq!(jobs[2].url, format!("{JOB}/13/console"));
        let steps = steps(&sample(include_str!("samples/jenkins_stage.json")));
        assert_eq!(steps.len(), 2);
        assert_eq!((steps[1].number, steps[1].name.as_str(), steps[1].state), (25, "Shell Script: cargo test --workspace", State::Failed));
    }

    #[test]
    fn a_build_without_stages_is_its_console() {
        let job = whole(&sample(r#"{"number":7,"building":false,"result":"FAILURE","timestamp":1793958067000,"duration":60000}"#), JOB, 7);
        assert_eq!((split(job.id), job.name.as_str(), job.state), ((7, 0), "console", State::Failed));
        assert_eq!(job.finished.zip(job.started).map(|(f, s)| f - s), Some(60));
    }

    #[test]
    fn every_status_has_a_state() {
        assert_eq!(build_state(true, ""), State::Running);
        assert_eq!(build_state(false, "ABORTED"), State::Cancelled);
        assert_eq!(build_state(false, "NOT_BUILT"), State::Skipped);
        assert_eq!(build_state(false, ""), State::Queued);
        assert_eq!(stage_state("PAUSED_PENDING_INPUT"), State::Manual);
        assert_eq!(stage_state("IN_PROGRESS"), State::Running);
        assert_eq!(stage_state("QUEUED"), State::Queued);
    }

    #[test]
    fn a_stages_log_is_its_part_of_the_console() {
        let console = include_str!("samples/jenkins_console.txt");
        let test = stage_log(console, "Test").unwrap();
        assert!(test.starts_with("[Pipeline] sh\n+ cargo test --workspace\n"), "{test}");
        assert!(test.trim_end().ends_with("error: test failed, to rerun pass `-p rocket-telemetry --lib`"), "{test}");
        assert!(!test.contains("cargo build") && !test.contains("Deploy"), "{test}");
        // A stage with stages in it keeps them.
        let nested = "[Pipeline] { (Outer)\n[Pipeline] stage\n[Pipeline] { (Inner)\nin\n[Pipeline] }\n[Pipeline] // stage\nafter\n[Pipeline] }\nout\n";
        assert_eq!(stage_log(nested, "Outer").unwrap(), "[Pipeline] stage\n[Pipeline] { (Inner)\nin\n[Pipeline] }\n[Pipeline] // stage\nafter\n");
        assert_eq!(stage_log(console, "Publish"), None);
    }
}
