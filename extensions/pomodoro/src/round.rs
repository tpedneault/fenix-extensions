//! The rounds: work, then a break -- a long one every few rounds --
//! counted down against the wall clock (milliseconds since 1970), so a
//! round carries on while Fenix is closed. Nothing here talks to Fenix.

use serde::{Deserialize, Serialize};

const MINUTE: u64 = 60_000;

/// What a round is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn is_break(self) -> bool {
        self != Phase::Work
    }

    /// As a sentence says it: "work", "a short break".
    pub fn name(self) -> &'static str {
        match self {
            Phase::Work => "work",
            Phase::ShortBreak => "a short break",
            Phase::LongBreak => "a long break",
        }
    }
}

/// How long each round is, in minutes, and how many work rounds come
/// before a long break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lengths {
    pub work: u32,
    pub short_break: u32,
    pub long_break: u32,
    pub long_every: u32,
}

impl Default for Lengths {
    fn default() -> Lengths {
        Lengths { work: 25, short_break: 5, long_break: 15, long_every: 4 }
    }
}

impl Lengths {
    /// A round of `phase`, in minutes: at least one.
    pub fn minutes(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Work => self.work,
            Phase::ShortBreak => self.short_break,
            Phase::LongBreak => self.long_break,
        }
        .max(1)
    }

    /// The same, in ms.
    pub fn of(&self, phase: Phase) -> u64 {
        self.minutes(phase) as u64 * MINUTE
    }

    /// Work rounds to a long break: at least one.
    pub fn every(&self) -> u32 {
        self.long_every.max(1)
    }
}

/// Where the round is. `length` is the round's as it started, so
/// changing a setting doesn't stretch one under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "is")]
pub enum Run {
    /// Waiting to be started.
    #[default]
    Idle,
    Running { ends_at: u64, length: u64 },
    Paused { left: u64, length: u64 },
}

/// A round that's over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    pub phase: Phase,
    /// When: its end, or when it was skipped.
    pub at: u64,
    pub length: u64,
    /// It ran out, rather than being skipped: a work round that did is
    /// one for the history.
    pub counted: bool,
    /// The work round it was, or followed.
    pub round: u32,
    pub next: Phase,
    /// The next one started by itself (a break, with `auto_breaks`).
    pub started_next: bool,
}

/// The timer: which round, and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timer {
    #[serde(default)]
    pub phase: Phase,
    /// Work rounds done since the last long break.
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub run: Run,
}

impl Timer {
    /// The work round this is, or the one this break follows.
    pub fn round(&self) -> u32 {
        match self.phase {
            Phase::Work => self.done + 1,
            _ => self.done.max(1),
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }

    /// Starts the round, or goes on with a paused one. False when it was
    /// already running.
    pub fn start(&mut self, now: u64, lengths: &Lengths) -> bool {
        self.run = match self.run {
            Run::Idle => {
                let length = lengths.of(self.phase);
                Run::Running { ends_at: now + length, length }
            }
            Run::Paused { left, length } => Run::Running { ends_at: now + left, length },
            Run::Running { .. } => return false,
        };
        true
    }

    /// Pauses a running round. False when none was.
    pub fn pause(&mut self, now: u64) -> bool {
        let Run::Running { ends_at, length } = self.run else { return false };
        self.run = Run::Paused { left: ends_at.saturating_sub(now), length };
        true
    }

    /// How long is left, in ms; none while it waits to start.
    pub fn left(&self, now: u64) -> Option<u64> {
        match self.run {
            Run::Idle => None,
            Run::Running { ends_at, .. } => Some(ends_at.saturating_sub(now)),
            Run::Paused { left, .. } => Some(left),
        }
    }

    /// How long this round is, in ms: as it started, or as it would.
    pub fn length(&self, lengths: &Lengths) -> u64 {
        match self.run {
            Run::Idle => lengths.of(self.phase),
            Run::Running { length, .. } | Run::Paused { length, .. } => length,
        }
    }

    /// The round that ran out by `now`, if one did; the next is then set
    /// up. Call it again: a break started by itself may be over too
    /// (after Fenix was closed a while).
    pub fn tick(&mut self, now: u64, lengths: &Lengths, auto_breaks: bool) -> Option<Ended> {
        match self.run {
            Run::Running { ends_at, length } if now >= ends_at => Some(self.finish(ends_at, length, true, lengths, auto_breaks)),
            _ => None,
        }
    }

    /// Ends the round now, uncounted, and goes on to the next as if it
    /// had run out.
    pub fn skip(&mut self, now: u64, lengths: &Lengths, auto_breaks: bool) -> Ended {
        let length = self.length(lengths);
        self.finish(now, length, false, lengths, auto_breaks)
    }

    /// Back to the first work round, waiting.
    pub fn reset(&mut self) {
        *self = Timer::default();
    }

    fn finish(&mut self, at: u64, length: u64, counted: bool, lengths: &Lengths, auto_breaks: bool) -> Ended {
        let (phase, round) = (self.phase, self.round());
        let next = match phase {
            Phase::Work => {
                self.done += 1;
                if self.done >= lengths.every() {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak => Phase::Work,
            Phase::LongBreak => {
                self.done = 0;
                Phase::Work
            }
        };
        self.phase = next;
        let started_next = next.is_break() && auto_breaks;
        self.run = if started_next {
            // From when the last one ended: a break that began while
            // Fenix was closed is already under way.
            let length = lengths.of(next);
            Run::Running { ends_at: at + length, length }
        } else {
            Run::Idle
        };
        Ended { phase, at, length, counted, round, next, started_next }
    }
}

/// `18:42`, or `1:02:05` past an hour: what's left, the seconds rounded
/// up, so a round starts on its whole length.
pub fn clock(ms: u64) -> String {
    let secs = ms.div_ceil(1000);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: Lengths = Lengths { work: 25, short_break: 5, long_break: 15, long_every: 4 };

    /// Runs the round to its end.
    fn run_out(t: &mut Timer, now: &mut u64, auto: bool) -> Ended {
        if !t.running() {
            t.start(*now, &L);
        }
        *now += t.left(*now).unwrap();
        t.tick(*now, &L, auto).expect("it ran out")
    }

    #[test]
    fn a_long_break_every_four_rounds() {
        let (mut t, mut now) = (Timer::default(), 0);
        let mut phases = Vec::new();
        for _ in 0..9 {
            let e = run_out(&mut t, &mut now, false);
            phases.push((e.phase, e.round));
        }
        use Phase::*;
        assert_eq!(phases, [(Work, 1), (ShortBreak, 1), (Work, 2), (ShortBreak, 2), (Work, 3), (ShortBreak, 3), (Work, 4), (LongBreak, 4), (Work, 1)]);
        assert_eq!(t.phase, ShortBreak);
    }

    #[test]
    fn lengths_count_down() {
        let mut t = Timer::default();
        assert_eq!(t.left(0), None);
        assert!(t.start(1_000, &L));
        assert!(!t.start(2_000, &L), "already running");
        assert_eq!(t.left(1_000), Some(25 * MINUTE));
        assert_eq!(t.left(1_000 + 7 * MINUTE), Some(18 * MINUTE));
        assert_eq!(t.tick(1_000 + 25 * MINUTE - 1, &L, false), None);
        let e = t.tick(1_000 + 25 * MINUTE, &L, false).unwrap();
        assert_eq!((e.phase, e.next, e.counted, e.started_next), (Phase::Work, Phase::ShortBreak, true, false));
        assert_eq!(t.run, Run::Idle);
        t.start(0, &L);
        assert_eq!(t.left(0), Some(5 * MINUTE));
    }

    #[test]
    fn pausing_keeps_what_was_left() {
        let mut t = Timer::default();
        t.start(0, &L);
        assert!(t.pause(10 * MINUTE));
        assert!(!t.pause(11 * MINUTE));
        assert_eq!(t.left(60 * MINUTE), Some(15 * MINUTE));
        assert_eq!(t.tick(60 * MINUTE, &L, false), None, "a paused round doesn't run out");
        t.start(60 * MINUTE, &L);
        assert_eq!(t.left(60 * MINUTE), Some(15 * MINUTE));
        assert_eq!(t.length(&L), 25 * MINUTE);
    }

    #[test]
    fn breaks_can_start_by_themselves() {
        let (mut t, mut now) = (Timer::default(), 0);
        let e = run_out(&mut t, &mut now, true);
        assert!(e.started_next);
        assert_eq!(t.left(now), Some(5 * MINUTE));
        // The work after it waits for you.
        let e = run_out(&mut t, &mut now, true);
        assert_eq!((e.phase, e.started_next), (Phase::ShortBreak, false));
        assert_eq!(t.run, Run::Idle);
    }

    #[test]
    fn rounds_that_ended_while_closed_are_caught_up() {
        let mut t = Timer::default();
        t.start(0, &L);
        // An hour on: the work round, then the break it started.
        let now = 60 * MINUTE;
        let first = t.tick(now, &L, true).unwrap();
        assert_eq!(first.at, 25 * MINUTE);
        let second = t.tick(now, &L, true).unwrap();
        assert_eq!((second.phase, second.at), (Phase::ShortBreak, 30 * MINUTE));
        assert_eq!(t.tick(now, &L, true), None);
        assert_eq!((t.phase, t.run), (Phase::Work, Run::Idle));
    }

    #[test]
    fn skipping_moves_on_uncounted() {
        let mut t = Timer::default();
        t.start(0, &L);
        let e = t.skip(MINUTE, &L, false);
        assert_eq!((e.phase, e.counted, e.next), (Phase::Work, false, Phase::ShortBreak));
        let e = t.skip(2 * MINUTE, &L, false);
        assert_eq!((e.phase, e.next), (Phase::ShortBreak, Phase::Work));
        assert_eq!(t.round(), 2);
    }

    #[test]
    fn reset_goes_back_to_the_first_round() {
        let (mut t, mut now) = (Timer::default(), 0);
        run_out(&mut t, &mut now, true);
        t.reset();
        assert_eq!(t, Timer::default());
        assert_eq!(t.round(), 1);
    }

    #[test]
    fn a_round_started_keeps_its_length() {
        let mut t = Timer::default();
        t.start(0, &L);
        let longer = Lengths { work: 50, ..L };
        assert_eq!(t.length(&longer), 25 * MINUTE);
        assert_eq!(t.tick(25 * MINUTE, &longer, false).map(|e| e.length), Some(25 * MINUTE));
    }

    #[test]
    fn odd_settings_are_kept_sane() {
        let l = Lengths { work: 0, short_break: 0, long_break: 0, long_every: 0 };
        assert_eq!(l.of(Phase::Work), MINUTE);
        let (mut t, mut now) = (Timer::default(), 0);
        t.start(now, &l);
        now += MINUTE;
        assert_eq!(t.tick(now, &l, false).unwrap().next, Phase::LongBreak);
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(25 * MINUTE), "25:00");
        assert_eq!(clock(18 * MINUTE + 41_001), "18:42");
        assert_eq!(clock(1), "00:01");
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(62 * MINUTE + 5_000), "1:02:05");
    }
}
