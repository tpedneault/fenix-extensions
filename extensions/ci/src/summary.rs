//! The branch's CI in a word: which runs count (the latest of each
//! workflow for `HEAD`, else for the newest commit that has any), what
//! they add up to, and the modeline's text for it.

use std::collections::HashMap;

use crate::model::{Job, Run, State};

/// What the runs that count add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overall {
    /// No runs for the branch.
    None,
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub overall: Overall,
    /// Jobs finished, of how many (a run whose jobs aren't known counts as one).
    pub done: usize,
    pub total: usize,
    /// Jobs that failed (and count: not GitLab's allowed failures).
    pub failed: usize,
    /// The commit the runs are for.
    pub sha: String,
    /// They're for an earlier commit than `HEAD`: it has none yet.
    pub earlier: bool,
}

/// The runs that count, newest first: the latest run of each workflow for
/// `head`, else for the newest commit that has any. And whether that's an
/// earlier commit than `head`.
pub fn current<'a>(runs: &'a [Run], head: Option<&str>) -> (Vec<&'a Run>, bool) {
    let matches = |sha: &str, head: &str| !sha.is_empty() && !head.is_empty() && (sha.starts_with(head) || head.starts_with(sha));
    let for_head = head.filter(|h| runs.iter().any(|r| matches(&r.sha, h)));
    let (sha, earlier) = match for_head {
        Some(h) => (h.to_string(), false),
        None => match runs.iter().max_by_key(|r| (r.created.unwrap_or(0), r.id)) {
            Some(newest) => (newest.sha.clone(), head.is_some()),
            None => return (Vec::new(), false),
        },
    };
    let mut seen = Vec::new();
    let mut out: Vec<&Run> = runs.iter().filter(|r| matches(&r.sha, &sha)).collect();
    out.sort_by_key(|r| std::cmp::Reverse((r.created.unwrap_or(0), r.id)));
    out.retain(|r| {
        let first = !seen.contains(&r.workflow);
        seen.push(r.workflow);
        first
    });
    (out, earlier)
}

/// What `runs` add up to, their jobs where they're known.
pub fn summarise(runs: &[&Run], jobs: &HashMap<u64, Vec<Job>>, earlier: bool) -> Summary {
    let (mut done, mut total, mut failed) = (0, 0, 0);
    for run in runs {
        match jobs.get(&run.id).filter(|j| !j.is_empty()) {
            Some(jobs) => {
                total += jobs.len();
                done += jobs.iter().filter(|j| j.state.done()).count();
                failed += jobs.iter().filter(|j| j.failed()).count();
            }
            None => {
                total += 1;
                done += usize::from(run.state.done());
                failed += usize::from(run.state == State::Failed);
            }
        }
    }
    let any = |s: State| runs.iter().any(|r| r.state == s);
    let overall = if runs.is_empty() {
        Overall::None
    } else if any(State::Running) || (runs.iter().any(|r| !r.state.done()) && done > 0) {
        Overall::Running
    } else if any(State::Queued) {
        Overall::Queued
    } else if any(State::Failed) || failed > 0 {
        Overall::Failed
    } else if any(State::Cancelled) {
        Overall::Cancelled
    } else {
        Overall::Success
    };
    Summary { overall, done, total, failed, sha: runs.first().map(|r| r.sha.clone()).unwrap_or_default(), earlier }
}

impl Summary {
    /// Whether something is still to finish.
    pub fn active(&self) -> bool {
        matches!(self.overall, Overall::Queued | Overall::Running)
    }

    /// The modeline's item: `CI ✓`, `CI ● 3/7`, `CI ✗ 2 failed`.
    pub fn modeline(&self) -> String {
        let text = match self.overall {
            Overall::None => "CI –".to_string(),
            Overall::Queued => "CI ○ queued".to_string(),
            Overall::Running if self.failed > 0 => format!("CI ● {}/{} ✗{}", self.done, self.total, self.failed),
            Overall::Running => format!("CI ● {}/{}", self.done, self.total),
            Overall::Success => "CI ✓".to_string(),
            Overall::Failed if self.failed > 0 => format!("CI ✗ {} failed", self.failed),
            Overall::Failed => "CI ✗ failed".to_string(),
            Overall::Cancelled => "CI ⊘ cancelled".to_string(),
        };
        if self.earlier && self.overall != Overall::None {
            format!("{text} (prev)")
        } else {
            text
        }
    }

    /// In a few words, for the page's header.
    pub fn sentence(&self) -> String {
        let of = if self.earlier { " for an earlier commit (HEAD has no runs yet)" } else { "" };
        match self.overall {
            Overall::None => "No runs for this branch yet".to_string(),
            Overall::Queued => format!("Queued{of}"),
            Overall::Running => format!("Running: {} of {} jobs done{}{of}", self.done, self.total, if self.failed > 0 { format!(", {} failed", self.failed) } else { String::new() }),
            Overall::Success => format!("Passed{of}"),
            Overall::Failed => format!("Failed{}{of}", if self.failed > 0 { format!(": {} {}", self.failed, if self.failed == 1 { "job" } else { "jobs" }) } else { String::new() }),
            Overall::Cancelled => format!("Cancelled{of}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{github, gitlab};

    const HEAD: &str = "9f1c2e7d4b8a6f3e2d1c0b9a8f7e6d5c4b3a2f10";

    fn sample_runs() -> Vec<Run> {
        github::runs(&serde_json::from_str(include_str!("samples/github_runs.json")).unwrap())
    }

    fn sample_jobs() -> Vec<Job> {
        github::jobs(&serde_json::from_str(include_str!("samples/github_jobs.json")).unwrap())
    }

    #[test]
    fn heads_runs_count_one_per_workflow() {
        let runs = sample_runs();
        let (current, earlier) = current(&runs, Some(HEAD));
        let names: Vec<&str> = current.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["CI", "Lint"]);
        assert!(!earlier);
        // A short HEAD matches too.
        assert_eq!(super::current(&runs, Some("9f1c2e7")).0.len(), 2);
    }

    #[test]
    fn a_head_with_no_runs_falls_back_to_the_newest_commit_that_has_some() {
        let runs = sample_runs();
        let (current, earlier) = current(&runs, Some("0000000000000000000000000000000000000000"));
        assert!(earlier);
        assert_eq!(current.len(), 2, "the newest commit's");
        assert_eq!(current[0].sha, HEAD);
        assert_eq!(super::current(&[], Some(HEAD)), (Vec::new(), false));
    }

    #[test]
    fn running_counts_finished_jobs_and_says_what_failed_so_far() {
        let runs = sample_runs();
        let (current, earlier) = current(&runs, Some(HEAD));
        let mut jobs = HashMap::new();
        jobs.insert(runs[0].id, sample_jobs());
        let s = summarise(&current, &jobs, earlier);
        // CI: 2 of 4 jobs done (one failed); Lint: no jobs known, done.
        assert_eq!((s.overall, s.done, s.total, s.failed), (Overall::Running, 3, 5, 1));
        assert_eq!(s.modeline(), "CI ● 3/5 ✗1");
        assert!(s.active());
        // Without the jobs, a run is one.
        let s = summarise(&current, &HashMap::new(), false);
        assert_eq!(s.modeline(), "CI ● 1/2");
    }

    #[test]
    fn finished_runs_say_passed_failed_or_cancelled() {
        let mut runs = sample_runs();
        runs[0].state = State::Success;
        let (current, _) = current(&runs, Some(HEAD));
        assert_eq!(summarise(&current, &HashMap::new(), false).modeline(), "CI ✓");
        assert_eq!(summarise(&current, &HashMap::new(), true).modeline(), "CI ✓ (prev)");
        runs[0].state = State::Failed;
        let (current, _) = super::current(&runs, Some(HEAD));
        let mut jobs = HashMap::new();
        let mut finished = sample_jobs();
        finished[2].state = State::Failed;
        finished[3].state = State::Skipped;
        jobs.insert(runs[0].id, finished);
        let s = summarise(&current, &jobs, false);
        assert_eq!((s.overall, s.failed), (Overall::Failed, 2));
        assert_eq!(s.modeline(), "CI ✗ 2 failed");
        assert_eq!(s.sentence(), "Failed: 2 jobs");
        runs[0].state = State::Cancelled;
        let (current, _) = super::current(&runs, Some(HEAD));
        assert_eq!(summarise(&current, &HashMap::new(), false).modeline(), "CI ⊘ cancelled");
        assert_eq!(summarise(&[], &HashMap::new(), false).modeline(), "CI –");
    }

    #[test]
    fn queued_is_queued_until_a_job_starts() {
        let mut runs = sample_runs();
        runs[0].state = State::Queued;
        runs.truncate(1);
        let (current, _) = current(&runs, Some(HEAD));
        assert_eq!(summarise(&current, &HashMap::new(), false).modeline(), "CI ○ queued");
    }

    #[test]
    fn gitlabs_allowed_failures_dont_fail_the_summary() {
        let runs = gitlab::pipelines(&serde_json::from_str(include_str!("samples/gitlab_pipelines.json")).unwrap());
        let (current, _) = current(&runs, Some("c0ffee1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b"));
        assert_eq!(current.len(), 1);
        let mut jobs = HashMap::new();
        jobs.insert(current[0].id, gitlab::jobs(&serde_json::from_str(include_str!("samples/gitlab_jobs.json")).unwrap()));
        let s = summarise(&current, &jobs, false);
        assert_eq!((s.overall, s.failed, s.done, s.total), (Overall::Failed, 1, 4, 4));
        assert_eq!(s.modeline(), "CI ✗ 1 failed");
    }
}
