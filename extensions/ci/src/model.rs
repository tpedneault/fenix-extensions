//! What the CI status is made of, the same for GitHub Actions and GitLab
//! pipelines: runs (a workflow run, a pipeline), their jobs, and a job's
//! steps -- each with its state and when it started and finished.

/// Which forge runs the repository's CI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    GitLab,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub Actions",
            Provider::GitLab => "GitLab pipelines",
        }
    }

    /// A run's name for it: a workflow run, a pipeline.
    pub fn run_word(self) -> &'static str {
        match self {
            Provider::GitHub => "run",
            Provider::GitLab => "pipeline",
        }
    }
}

/// Where a run, a job or a step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    /// Waiting for a runner, or for what it needs first.
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
    Skipped,
    /// Waiting for someone to start it (a manual job, an approval).
    Manual,
    /// Finished, neither passed nor failed (GitHub's `neutral`).
    Neutral,
}

impl State {
    /// Whether it has finished, one way or another.
    pub fn done(self) -> bool {
        !matches!(self, State::Queued | State::Running)
    }

    pub fn icon(self) -> &'static str {
        match self {
            State::Queued => "○",
            State::Running => "●",
            State::Success => "✓",
            State::Failed => "✗",
            State::Cancelled => "⊘",
            State::Skipped => "–",
            State::Manual => "▷",
            State::Neutral => "·",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Success => "passed",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
            State::Skipped => "skipped",
            State::Manual => "manual",
            State::Neutral => "neutral",
        }
    }
}

/// One step of a job (GitHub's; GitLab's jobs have none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub number: u32,
    pub name: String,
    pub state: State,
    /// Seconds since the epoch.
    pub started: Option<i64>,
    pub finished: Option<i64>,
}

/// A job of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    /// GitLab's stage; empty on GitHub.
    pub stage: String,
    pub state: State,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub url: String,
    /// GitLab's `allow_failure`: its failing doesn't fail the pipeline.
    pub allow_failure: bool,
    pub steps: Vec<Step>,
}

impl Job {
    /// It failed, and that counts.
    pub fn failed(&self) -> bool {
        self.state == State::Failed && !self.allow_failure
    }
}

/// A workflow run, or a pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: u64,
    /// The workflow's name ("CI"); "Pipeline" on GitLab.
    pub name: String,
    /// What it's about: the commit's title, GitHub's `display_title`.
    pub title: String,
    /// `#123`: the run's number, the pipeline's iid.
    pub number: u64,
    pub attempt: u32,
    /// `push`, `pull_request`, `schedule`, `web`...
    pub event: String,
    pub state: State,
    pub branch: String,
    pub sha: String,
    pub url: String,
    pub created: Option<i64>,
    pub started: Option<i64>,
    /// When it finished, once it has.
    pub finished: Option<i64>,
    /// Which workflow it's a run of; 0 on GitLab, which has one pipeline
    /// per commit that counts.
    pub workflow: u64,
}

/// How long something took, or has taken so far.
pub fn took(started: Option<i64>, finished: Option<i64>, now: i64) -> Option<i64> {
    let start = started?;
    Some((finished.unwrap_or(now) - start).max(0))
}

/// "42s", "3m 12s", "1h 04m".
pub fn duration(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, secs % 3600 / 60),
    }
}

/// "just now", "5m ago", "3h ago", "2d ago".
pub fn ago(at: i64, now: i64) -> String {
    let secs = (now - at).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// The first seven characters of a commit's SHA.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// An RFC 3339 time (`2026-11-06T09:41:07Z`, `2026-11-06T09:41:07.123+02:00`)
/// as seconds since the epoch.
pub fn parse_time(text: &str) -> Option<i64> {
    let t = text.trim();
    let num = |s: &str| -> Option<i64> { (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten() };
    let (date, rest) = t.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let (year, month, day) = (num(d.next()?)?, num(d.next()?)?, num(d.next()?)?);
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // The time, then a fraction, then `Z` or an offset.
    let (clock, zone) = match rest.find(['Z', 'z', '+', '-']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "Z"),
    };
    let clock = clock.split('.').next()?;
    let mut c = clock.split(':');
    let (hour, minute, second) = (num(c.next()?)?, num(c.next()?)?, num(c.next().unwrap_or("0"))?);
    let offset = match zone {
        "Z" | "z" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let z = &z[1..];
            let (h, m) = z.split_once(':').unwrap_or((z.get(..2)?, z.get(2..).unwrap_or("0")));
            sign * (num(h)? * 3600 + num(if m.is_empty() { "0" } else { m })? * 60)
        }
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_read_in_every_form_the_forges_write() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-06T09:41:07Z"), Some(1_793_958_067));
        assert_eq!(parse_time("2026-11-06T09:41:07.123Z"), Some(1_793_958_067));
        assert_eq!(parse_time("2026-11-06T11:41:07.123+02:00"), Some(1_793_958_067));
        assert_eq!(parse_time("2026-11-06T04:41:07-05:00"), Some(1_793_958_067));
        assert_eq!(parse_time("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("yesterday"), None);
        assert_eq!(parse_time("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn durations_and_ages_read_as_people_say_them() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(192), "3m 12s");
        assert_eq!(duration(3840), "1h 04m");
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 7200), "2h ago");
        assert_eq!(ago(0, 3 * 86_400), "3d ago");
        assert_eq!(took(Some(10), Some(70), 1000), Some(60));
        assert_eq!(took(Some(10), None, 100), Some(90), "still going: so far");
        assert_eq!(took(None, Some(70), 100), None);
    }

    #[test]
    fn only_running_and_queued_are_unfinished() {
        assert!(!State::Queued.done() && !State::Running.done());
        assert!(State::Success.done() && State::Failed.done() && State::Cancelled.done() && State::Manual.done());
    }
}
