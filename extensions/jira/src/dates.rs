//! A due date as you type it: `fri`, `+3`, `2w`, `10-02`, `2026-10-02`,
//! `none` -- the agenda's way.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

fn next_weekday(from: NaiveDate, day: Weekday) -> NaiveDate {
    let ahead = (7 + day.num_days_from_monday() as i64 - from.weekday().num_days_from_monday() as i64) % 7;
    from + Duration::days(ahead)
}

pub fn parse_date(text: &str, today: NaiveDate) -> Result<Option<NaiveDate>, String> {
    let t = text.trim().to_lowercase();
    if t.is_empty() || t == "none" || t == "-" {
        return Ok(None);
    }
    let bad = || format!("\"{text}\" isn't a date -- try fri, +3, 2w or 2026-10-02");
    match t.as_str() {
        "today" | "tod" => return Ok(Some(today)),
        "tomorrow" | "tom" => return Ok(Some(today + Duration::days(1))),
        _ => {}
    }
    let days = [
        (Weekday::Mon, "monday"),
        (Weekday::Tue, "tuesday"),
        (Weekday::Wed, "wednesday"),
        (Weekday::Thu, "thursday"),
        (Weekday::Fri, "friday"),
        (Weekday::Sat, "saturday"),
        (Weekday::Sun, "sunday"),
    ];
    if t.len() >= 3 {
        if let Some((day, _)) = days.iter().find(|(_, name)| name.starts_with(&t)) {
            return Ok(Some(next_weekday(today, *day)));
        }
    }
    let rel = t.strip_prefix('+').unwrap_or(&t);
    if let Some(n) = rel.strip_suffix('w').and_then(|n| n.parse::<i64>().ok()) {
        return Ok(Some(today + Duration::weeks(n)));
    }
    if let Some(n) = rel.strip_suffix('d').unwrap_or(rel).parse::<i64>().ok().filter(|_| t.starts_with('+') || t.ends_with('d')) {
        return Ok(Some(today + Duration::days(n)));
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return Ok(Some(d));
    }
    if let Some((m, d)) = t.split_once('-') {
        if let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>()) {
            let this_year = NaiveDate::from_ymd_opt(today.year(), m, d).ok_or_else(bad)?;
            let date = if this_year < today { NaiveDate::from_ymd_opt(today.year() + 1, m, d).ok_or_else(bad)? } else { this_year };
            return Ok(Some(date));
        }
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_read_the_agendas_way() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        assert_eq!(parse_date("fri", today), Ok(NaiveDate::from_ymd_opt(2026, 10, 30)));
        assert_eq!(parse_date("+3", today), Ok(NaiveDate::from_ymd_opt(2026, 10, 31)));
        assert_eq!(parse_date("none", today), Ok(None));
        assert!(parse_date("someday", today).is_err());
    }
}
