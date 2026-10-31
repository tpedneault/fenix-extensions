//! What's kept in `/data/pomodoro.json`: the timer as it is, the work
//! rounds done (four weeks of them), and the agenda task they're for.

use chrono::{DateTime, Days, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::round::Timer;

/// How far back rounds are kept, in days.
pub const KEEP_DAYS: u64 = 28;

/// A work round that ran out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    /// When it ended, where you were.
    pub ended: DateTime<FixedOffset>,
    pub minutes: u32,
    /// The agenda task it was on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

/// An agenda task, as `agenda.tasks` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: u32,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub timer: Timer,
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// The task work rounds clock into, when the agenda's clock is off.
    #[serde(default)]
    pub task: Option<Task>,
    /// What the agenda's clock is on this round, and whether this
    /// started it (and so stops it when the round ends).
    #[serde(default)]
    pub on: Option<String>,
    #[serde(default)]
    pub clocked: bool,
}

impl Kept {
    /// What was kept; a file that can't be read starts afresh.
    pub fn load(text: &str) -> Kept {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn save(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Rounds older than `KEEP_DAYS` before `today` let go.
    pub fn prune(&mut self, today: NaiveDate) {
        let Some(oldest) = today.checked_sub_days(Days::new(KEEP_DAYS)) else { return };
        self.rounds.retain(|r| r.ended.date_naive() > oldest);
    }

    /// The rounds that ended on `day`, in order.
    pub fn on_day(&self, day: NaiveDate) -> impl Iterator<Item = &Round> {
        self.rounds.iter().filter(move |r| r.ended.date_naive() == day)
    }
}

/// A day of the week's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub date: NaiveDate,
    pub rounds: u32,
    pub minutes: u32,
}

/// The seven days to `today`, the oldest first.
pub fn week(kept: &Kept, today: NaiveDate) -> Vec<Day> {
    (0..7u64)
        .rev()
        .filter_map(|back| today.checked_sub_days(Days::new(back)))
        .map(|date| {
            let (rounds, minutes) = kept.on_day(date).fold((0, 0), |(n, m), r| (n + 1, m + r.minutes));
            Day { date, rounds, minutes }
        })
        .collect()
}

/// `25 min`, `1:15`: time spent, as the page shows it.
pub fn spent(minutes: u32) -> String {
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{}:{:02}", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(text).unwrap()
    }

    fn round(text: &str, minutes: u32) -> Round {
        Round { ended: at(text), minutes, task: None }
    }

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn a_week_by_day() {
        let kept = Kept {
            rounds: vec![
                round("2026-10-20T10:00:00-04:00", 25),
                round("2026-10-23T09:00:00-04:00", 25),
                round("2026-10-29T09:30:00-04:00", 25),
                round("2026-10-29T10:00:00-04:00", 50),
            ],
            ..Kept::default()
        };
        let week = week(&kept, day("2026-10-29"));
        assert_eq!(week.len(), 7);
        assert_eq!(week[0].date, day("2026-10-23"));
        assert_eq!((week[0].rounds, week[0].minutes), (1, 25));
        assert_eq!((week[6].rounds, week[6].minutes), (2, 75));
        assert_eq!(week.iter().map(|d| d.rounds).sum::<u32>(), 3, "the 20th is outside it");
    }

    #[test]
    fn days_are_where_you_were() {
        // Late in the evening at -04:00 is the next day in UTC: still today.
        let kept = Kept { rounds: vec![round("2026-10-29T23:30:00-04:00", 25)], ..Kept::default() };
        assert_eq!(kept.on_day(day("2026-10-29")).count(), 1);
    }

    #[test]
    fn old_rounds_go() {
        let mut kept = Kept { rounds: vec![round("2026-09-01T10:00:00Z", 25), round("2026-10-02T10:00:00Z", 25), round("2026-10-29T10:00:00Z", 25)], ..Kept::default() };
        kept.prune(day("2026-10-29"));
        assert_eq!(kept.rounds.len(), 2);
    }

    #[test]
    fn kept_round_trips() {
        let mut kept = Kept { rounds: vec![round("2026-10-29T10:00:00+02:00", 25)], task: Some(Task { id: 3, title: "Parser".into() }), ..Kept::default() };
        kept.timer.start(5, &crate::round::Lengths::default());
        assert_eq!(Kept::load(&kept.save()), kept);
        assert_eq!(Kept::load("not json"), Kept::default());
        assert_eq!(Kept::load("{}"), Kept::default());
    }

    #[test]
    fn time_spent() {
        assert_eq!(spent(25), "25 min");
        assert_eq!(spent(75), "1:15");
        assert_eq!(spent(120), "2:00");
    }
}
