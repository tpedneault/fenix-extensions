//! The time of year: snow and a Santa hat in December, a pumpkin for
//! Halloween, a party hat on New Year's Day and on the day you adopted
//! a pet, a year (or more) on.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// The date `days` after 1 January 1970 (Howard Hinnant's
/// civil-from-days).
pub fn date(days: u32) -> Date {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Date { year, month, day }
}

/// What the day brings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Season {
    /// A hat everyone wears today.
    pub hat: Option<&'static str>,
    pub snow: bool,
    /// Confetti now and then: a party.
    pub party: bool,
}

pub fn season(today: Date) -> Season {
    match (today.month, today.day) {
        (12, _) => Season { hat: Some("santa_hat"), snow: true, party: false },
        (1, 1) => Season { hat: Some("party_hat"), snow: false, party: true },
        (10, 24..=31) => Season { hat: Some("pumpkin"), snow: false, party: false },
        _ => Season::default(),
    }
}

/// Whether today's the day `adopted` was, a year or more on.
pub fn anniversary(today: u32, adopted: u32) -> bool {
    let (a, t) = (date(adopted), date(today));
    today > adopted && (a.month, a.day) == (t.month, t.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_are_dates() {
        assert_eq!(date(0), Date { year: 1970, month: 1, day: 1 });
        assert_eq!(date(20_753), Date { year: 2026, month: 10, day: 27 });
        assert_eq!(date(20_819), Date { year: 2027, month: 1, day: 1 });
        assert_eq!(date(11_016), Date { year: 2000, month: 2, day: 29 });
    }

    #[test]
    fn the_seasons_dress_them_up() {
        assert_eq!(season(date(20_753)).hat, Some("pumpkin"));
        assert!(season(Date { year: 2026, month: 12, day: 3 }).snow);
        assert!(season(date(20_819)).party);
        assert_eq!(season(Date { year: 2026, month: 6, day: 3 }), Season::default());
        assert!(anniversary(20_753, 20_753 - 365));
        assert!(!anniversary(20_753, 20_753), "not the day itself");
    }
}
