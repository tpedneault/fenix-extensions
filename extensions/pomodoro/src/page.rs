//! The page (`SPC o p o`): the round under way, today's rounds, and the
//! week's.

use chrono::{Datelike, NaiveDate};
use fenix_extension_api::{page, Page};

use crate::history::{spent, week, Kept};
use crate::round::{clock, Lengths, Phase, Run};

pub fn render(kept: &Kept, lengths: &Lengths, now: u64, today: NaiveDate, width: u32) -> Page {
    let timer = &kept.timer;
    let every = lengths.every();
    let mut p = Page::new().line([page::title("Pomodoro"), page::muted(format!("  round {} of {every}", timer.round().min(every)))]).rule();

    // The round.
    let (icon, what) = match timer.phase {
        Phase::Work => ("\u{1f345}", "Work"),
        Phase::ShortBreak => ("\u{2615}", "Short break"),
        Phase::LongBreak => ("\u{2615}", "Long break"),
    };
    let length = timer.length(lengths);
    let left = timer.left(now).unwrap_or(length);
    let state = match timer.run {
        Run::Running { .. } => page::good("running"),
        Run::Paused { .. } => page::warn("paused -- s goes on"),
        Run::Idle => page::muted("waiting -- s starts it"),
    };
    p = p.line([page::text(format!("{icon} ")), page::accent(what), page::muted("  "), page::title(clock(left)), page::muted("  "), state]);
    let bar = (width.saturating_sub(20) as usize).clamp(10, 40);
    let gone = length.saturating_sub(left);
    let filled = (gone as u128 * bar as u128 / length.max(1) as u128) as usize;
    p = p.line([
        page::muted("  "),
        if timer.phase.is_break() { page::good("█".repeat(filled)) } else { page::bad("█".repeat(filled)) },
        page::muted("░".repeat(bar - filled.min(bar))),
        page::muted(format!("  {} of {} min", gone / 60_000, length / 60_000)),
    ]);
    if let Some(on) = &kept.on {
        p = p.line([page::muted("  on "), page::text(on.clone()), page::muted(if kept.clocked { "  (clocked in)" } else { "  (the agenda's clock)" })]);
    } else if let Some(task) = &kept.task {
        p = p.line([page::muted("  for "), page::text(task.title.clone()), page::muted("  (clocks in as work starts)")]);
    }
    let next = match timer.phase {
        Phase::Work if timer.done + 1 >= every => Phase::LongBreak,
        Phase::Work => Phase::ShortBreak,
        _ => Phase::Work,
    };
    p = p.line([page::muted(format!("  then {} ({} min)", next.name(), lengths.minutes(next)))]).blank();

    // Today.
    let rounds: Vec<_> = kept.on_day(today).collect();
    let minutes: u32 = rounds.iter().map(|r| r.minutes).sum();
    p = p.line([page::title("Today"), page::muted(format!("  {} round{} · {} focused", rounds.len(), if rounds.len() == 1 { "" } else { "s" }, spent(minutes)))]).rule();
    if rounds.is_empty() {
        p = p.line([page::muted("  None yet.")]);
    }
    for r in &rounds {
        let mut line = vec![page::muted("  "), page::text(r.ended.format("%H:%M").to_string()), page::muted(format!("  {:>6}", spent(r.minutes)))];
        if let Some(task) = &r.task {
            line.push(page::muted("  "));
            line.push(page::text(task.clone()));
        }
        p = p.line(line);
    }
    p = p.blank();

    // The week.
    let days = week(kept, today);
    let total: u32 = days.iter().map(|d| d.minutes).sum();
    p = p.line([page::title("This week"), page::muted(format!("  {} rounds · {} focused", days.iter().map(|d| d.rounds).sum::<u32>(), spent(total)))]).rule();
    let most = days.iter().map(|d| d.rounds).max().unwrap_or(0).max(1) as usize;
    let room = (width.saturating_sub(30) as usize).clamp(4, 24);
    for d in &days {
        let name = format!("  {} {:>2}  ", d.date.weekday(), d.date.day());
        let label = if d.date == today { page::accent(name) } else { page::muted(name) };
        let filled = (d.rounds as usize * room).div_ceil(most).min(room);
        p = p.line([label, page::bad("█".repeat(filled)), page::muted(format!("{}  {:>2}  ", " ".repeat(room - filled), d.rounds)), page::text(if d.minutes > 0 { spent(d.minutes) } else { String::new() })]);
    }
    p.key("s", "start / pause").key("n", "skip").key("r", "reset").key("t", "agenda task")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Round;

    #[test]
    fn it_lays_out_at_any_width_and_round() {
        let today: NaiveDate = "2026-10-29".parse().unwrap();
        let ended = chrono::DateTime::parse_from_rfc3339("2026-10-29T10:00:00-04:00").unwrap();
        let mut kept = Kept { rounds: vec![Round { ended, minutes: 25, task: Some("Parser".into()) }], ..Kept::default() };
        let l = Lengths::default();
        for width in [0, 20, 80, 300] {
            assert!(render(&kept, &l, 0, today, width).len() > 10);
        }
        kept.timer.start(0, &l);
        render(&kept, &l, 10 * 60_000, today, 80);
        kept.timer.pause(10 * 60_000);
        render(&kept, &l, 99 * 60_000, today, 80);
        kept.timer.skip(0, &l, true);
        render(&kept, &l, u64::MAX, today, 80);
    }
}
