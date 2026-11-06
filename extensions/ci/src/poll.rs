//! When to ask the forge again: often while something runs, seldom when
//! nothing does, at once when the branch moves or you push, backing off
//! while it fails, and not before a rate limit is lifted. Times are
//! milliseconds since the epoch, given in, so it's all tested here.

/// How often, as the settings say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intervals {
    pub running_ms: u64,
    pub idle_ms: u64,
}

impl Default for Intervals {
    fn default() -> Self {
        Intervals { running_ms: 30_000, idle_ms: 120_000 }
    }
}

impl Intervals {
    /// From the settings' seconds, each at least 5 s, `idle` no less often
    /// than `running`'s.
    pub fn from_seconds(running: Option<&str>, idle: Option<&str>) -> Intervals {
        let read = |s: Option<&str>, default: u64| s.and_then(|s| s.trim().parse::<u64>().ok()).filter(|s| *s > 0).map(|s| s.max(5) * 1000).unwrap_or(default);
        let running_ms = read(running, 30_000);
        Intervals { running_ms, idle_ms: read(idle, 120_000).max(running_ms) }
    }
}

/// The longest it waits after failures.
pub const MAX_BACKOFF_MS: u64 = 15 * 60_000;
/// After a push, how soon it looks again for the runs it started.
pub const AFTER_PUSH_MS: u64 = 8_000;
/// How many quick looks a push gets before the usual pace again.
pub const PUSH_LOOKS: u32 = 4;

/// What the repository looked like the last time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub ahead: u32,
}

/// What changed in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    None,
    /// The first look, or another repository.
    First,
    /// Another branch is checked out: what's known is another's.
    Branch,
    /// A commit, a reset, a pull: `HEAD` moved on the branch.
    Head,
    /// A push: ahead of its upstream, and now not.
    Pushed,
}

#[derive(Debug, Clone, Default)]
pub struct Poller {
    /// When it should next ask.
    pub next_at: i64,
    /// A request is out.
    pub in_flight: bool,
    /// Asks in a row that failed.
    pub failures: u32,
    /// A rate limit says not before.
    pub blocked_until: Option<i64>,
    /// Quick looks left after a push.
    pub push_looks: u32,
    seen: Option<Seen>,
}

impl Poller {
    /// The repository as it is now: what changed, and when to ask.
    pub fn git(&mut self, now: Seen, at: i64) -> Change {
        let change = match &self.seen {
            None => Change::First,
            Some(before) if before.branch != now.branch => Change::Branch,
            Some(before) if before.ahead > 0 && now.ahead == 0 && before.head == now.head => Change::Pushed,
            // A commit and its push between two looks.
            Some(before) if before.head != now.head && now.ahead == 0 && before.ahead == 0 => Change::Pushed,
            Some(before) if before.head != now.head => Change::Head,
            Some(_) => Change::None,
        };
        self.seen = Some(now);
        match change {
            Change::None => {}
            Change::Pushed => {
                self.push_looks = PUSH_LOOKS;
                self.next_at = at;
            }
            _ => {
                self.push_looks = 0;
                self.next_at = at;
            }
        }
        change
    }

    /// Whether to ask now.
    pub fn due(&self, at: i64) -> bool {
        !self.in_flight && at >= self.next_at && self.blocked_until.is_none_or(|b| at >= b)
    }

    /// Ask now, whatever the time: `R`, a command. Still not past a rate
    /// limit.
    pub fn now(&mut self, at: i64) {
        self.next_at = at;
    }

    pub fn started(&mut self) {
        self.in_flight = true;
    }

    /// An ask came back: `ok` or not, whether something is still to
    /// finish (`active`), and whether `HEAD` has runs yet (`waiting`:
    /// not, after a push). `blocked_until` is when a rate limit lifts.
    /// Returns how long until the next.
    pub fn finished(&mut self, ok: bool, active: bool, waiting: bool, blocked_until: Option<i64>, intervals: Intervals, at: i64) -> u64 {
        self.in_flight = false;
        self.failures = if ok { 0 } else { self.failures.saturating_add(1) };
        self.blocked_until = blocked_until.filter(|b| *b > at);
        let mut delay = self.delay(active, intervals);
        if ok && self.push_looks > 0 {
            if waiting || active {
                self.push_looks -= 1;
                if waiting {
                    delay = delay.min(AFTER_PUSH_MS);
                }
            } else {
                self.push_looks = 0;
            }
        }
        if let Some(b) = self.blocked_until {
            delay = delay.max((b - at) as u64);
        }
        self.next_at = at + delay as i64;
        delay
    }

    /// The pace: running or idle, doubled for each failure in a row, up
    /// to `MAX_BACKOFF_MS`.
    pub fn delay(&self, active: bool, intervals: Intervals) -> u64 {
        let base = if active { intervals.running_ms } else { intervals.idle_ms };
        if self.failures == 0 {
            return base;
        }
        let factor = 1u64 << self.failures.min(10);
        base.saturating_mul(factor).min(MAX_BACKOFF_MS.max(base))
    }
}

/// When a rate limit lifts, from an answer's status and headers (GitHub's
/// `x-ratelimit-*`, GitLab's `ratelimit-*`, `retry-after`), in ms since
/// the epoch; `now` is too. None when it isn't limited.
pub fn rate_limited_until(status: u16, headers: &[(String, String)], now: i64) -> Option<i64> {
    let header = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string());
    let number = |name: &str| header(name).and_then(|v| v.parse::<i64>().ok());
    let remaining = number("x-ratelimit-remaining").or_else(|| number("ratelimit-remaining"));
    let reset = number("x-ratelimit-reset").or_else(|| number("ratelimit-reset"));
    let retry_after = number("retry-after");
    let limited = status == 429 || (matches!(status, 403 | 429) && (remaining == Some(0) || retry_after.is_some())) || remaining == Some(0);
    if !limited {
        return None;
    }
    if let Some(secs) = retry_after {
        return Some(now + secs.max(1) * 1000);
    }
    match reset {
        // Seconds since the epoch.
        Some(at) => Some((at * 1000).max(now + 1000)),
        // No word on when: a minute, as GitHub asks for secondary limits.
        None => Some(now + 60_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(branch: &str, head: &str, ahead: u32) -> Seen {
        Seen { branch: Some(branch.into()), head: Some(head.into()), ahead }
    }

    const IV: Intervals = Intervals { running_ms: 30_000, idle_ms: 120_000 };

    #[test]
    fn running_polls_often_and_idle_seldom() {
        let mut p = Poller::default();
        assert_eq!(p.git(seen("main", "a", 0), 0), Change::First);
        assert!(p.due(0));
        p.started();
        assert!(!p.due(0), "not twice at once");
        assert_eq!(p.finished(true, true, false, None, IV, 0), 30_000);
        assert!(!p.due(29_999) && p.due(30_000));
        p.started();
        assert_eq!(p.finished(true, false, false, None, IV, 30_000), 120_000);
        assert_eq!(p.next_at, 150_000);
    }

    #[test]
    fn failures_back_off_and_success_resets_it() {
        let mut p = Poller::default();
        p.git(seen("main", "a", 0), 0);
        assert_eq!(p.finished(false, false, false, None, IV, 0), 240_000);
        assert_eq!(p.finished(false, false, false, None, IV, 0), 480_000);
        assert_eq!(p.finished(false, false, false, None, IV, 0), MAX_BACKOFF_MS);
        assert_eq!(p.finished(false, false, false, None, IV, 0), MAX_BACKOFF_MS, "capped");
        assert_eq!(p.finished(true, true, false, None, IV, 0), 30_000);
    }

    #[test]
    fn a_rate_limit_holds_it_back_until_it_lifts() {
        let mut p = Poller::default();
        p.git(seen("main", "a", 0), 0);
        let delay = p.finished(false, true, false, Some(600_000), IV, 0);
        assert_eq!(delay, 600_000);
        p.now(10_000);
        assert!(!p.due(10_000), "R doesn't get past it");
        assert!(p.due(600_000));
    }

    #[test]
    fn a_new_branch_or_head_asks_at_once() {
        let mut p = Poller::default();
        p.git(seen("main", "a", 0), 0);
        p.finished(true, false, false, None, IV, 0);
        assert_eq!(p.git(seen("main", "a", 0), 5_000), Change::None);
        assert!(!p.due(5_000));
        assert_eq!(p.git(seen("feature", "b", 0), 6_000), Change::Branch);
        assert!(p.due(6_000));
        p.finished(true, false, false, None, IV, 6_000);
        assert_eq!(p.git(seen("feature", "c", 1), 7_000), Change::Head, "a commit");
        assert!(p.due(7_000));
    }

    #[test]
    fn a_push_looks_quickly_until_its_runs_show() {
        let mut p = Poller::default();
        p.git(seen("main", "a", 2), 0);
        p.finished(true, false, false, None, IV, 0);
        assert_eq!(p.git(seen("main", "a", 0), 10_000), Change::Pushed);
        assert!(p.due(10_000));
        // HEAD has no runs yet: again soon.
        assert_eq!(p.finished(true, false, true, None, IV, 10_000), AFTER_PUSH_MS);
        assert_eq!(p.finished(true, false, true, None, IV, 18_000), AFTER_PUSH_MS);
        // They've started: the running pace.
        assert_eq!(p.finished(true, true, false, None, IV, 26_000), 30_000);
        // Done: the usual idle pace, the push's looks over.
        assert_eq!(p.finished(true, false, false, None, IV, 56_000), 120_000);
        assert_eq!(p.push_looks, 0);
        // A commit pushed between two looks is a push too.
        assert_eq!(p.git(seen("main", "b", 0), 60_000), Change::Pushed);
    }

    #[test]
    fn the_push_looks_run_out() {
        let mut p = Poller::default();
        p.git(seen("main", "a", 1), 0);
        p.git(seen("main", "a", 0), 0);
        for _ in 0..PUSH_LOOKS {
            assert_eq!(p.finished(true, false, true, None, IV, 0), AFTER_PUSH_MS);
        }
        assert_eq!(p.finished(true, false, true, None, IV, 0), 120_000, "no runs came: back to idle");
    }

    #[test]
    fn intervals_come_from_the_settings_in_seconds() {
        assert_eq!(Intervals::from_seconds(Some("10"), Some("300")), Intervals { running_ms: 10_000, idle_ms: 300_000 });
        assert_eq!(Intervals::from_seconds(Some("1"), None), Intervals { running_ms: 5_000, idle_ms: 120_000 }, "at least 5 s");
        assert_eq!(Intervals::from_seconds(Some("x"), Some("")), Intervals::default());
        assert_eq!(Intervals::from_seconds(Some("200"), Some("60")), Intervals { running_ms: 200_000, idle_ms: 200_000 });
    }

    #[test]
    fn rate_limits_are_read_from_either_forges_headers() {
        let h = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(rate_limited_until(200, &h(&[("X-RateLimit-Remaining", "4999")]), 0), None);
        assert_eq!(rate_limited_until(403, &h(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1800")]), 1_000_000), Some(1_800_000));
        assert_eq!(rate_limited_until(429, &h(&[("Retry-After", "30")]), 1_000), Some(31_000));
        assert_eq!(rate_limited_until(429, &h(&[("RateLimit-Remaining", "0"), ("RateLimit-Reset", "2000")]), 0), Some(2_000_000));
        assert_eq!(rate_limited_until(403, &h(&[]), 0), None, "a plain 403 is a permission, not a limit");
        assert_eq!(rate_limited_until(429, &h(&[]), 0), Some(60_000));
    }
}
