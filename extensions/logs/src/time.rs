//! Timestamps as logs write them, read into milliseconds since 1970 --
//! ISO 8601 and its kin, syslog's, Apache's, ctime's, logcat's, a time of
//! day alone -- and written back for the page.
//!
//! A time with no zone is taken as written: every time in a log is read
//! the same way, so their order and the gaps between them hold.

/// Milliseconds in a day.
pub const DAY: i64 = 86_400_000;

const MONTHS: [&[u8; 3]; 12] = [b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov", b"dec"];
const WEEKDAYS: [&[u8; 3]; 7] = [b"mon", b"tue", b"wed", b"thu", b"fri", b"sat", b"sun"];

/// Days from 1970-01-01 to `y`-`m`-`d`, in the proleptic Gregorian
/// calendar.
pub fn days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01: year, month, day.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A time as the page shows it: `2024-03-01 12:00:03.250`, or only the
/// time of day for one read without a date.
pub fn format(ms: i64) -> String {
    let (day, rest) = (ms.div_euclid(DAY), ms.rem_euclid(DAY));
    let clock = format!("{:02}:{:02}:{:02}", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60);
    let frac = if rest % 1000 != 0 { format!(".{:03}", rest % 1000) } else { String::new() };
    if day == 0 {
        return format!("{clock}{frac}");
    }
    let (y, m, d) = civil(day);
    format!("{y:04}-{m:02}-{d:02} {clock}{frac}")
}

/// A span of time as it's said: `350 ms`, `12 s`, `5 min`, `3 h`, `2 d`.
pub fn span(ms: i64) -> String {
    match ms.abs() {
        n if n < 1000 => format!("{ms} ms"),
        n if n < 120_000 => format!("{} s", ms / 1000),
        n if n < 7_200_000 => format!("{} min", ms / 60_000),
        n if n < 2 * DAY => format!("{} h", ms / 3_600_000),
        _ => format!("{} d", ms / DAY),
    }
}

/// A timestamp read from the start of some text: when, and how many
/// bytes it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub ms: i64,
    pub len: usize,
}

fn digit(s: &[u8], i: usize) -> Option<i64> {
    s.get(i).filter(|b| b.is_ascii_digit()).map(|b| (b - b'0') as i64)
}

/// `n` digits at `i`, as a number.
fn number(s: &[u8], i: usize, n: usize) -> Option<i64> {
    (i..i + n).try_fold(0, |acc, j| Some(acc * 10 + digit(s, j)?))
}

/// A month's three letters at `i`, any case: 1 to 12.
fn month(s: &[u8], i: usize) -> Option<u32> {
    let w = s.get(i..i + 3)?;
    MONTHS.iter().position(|m| w.eq_ignore_ascii_case(*m)).map(|p| p as u32 + 1)
}

/// `HH:MM:SS`, `HH:MM`, with `.fff` or `,fff` after the seconds: the
/// milliseconds into the day, and the bytes taken.
fn clock(s: &[u8], i: usize) -> Option<(i64, usize)> {
    let h = number(s, i, 2)?;
    if s.get(i + 2) != Some(&b':') {
        return None;
    }
    let m = number(s, i + 3, 2)?;
    let mut at = i + 5;
    let mut sec = 0;
    if s.get(at) == Some(&b':') {
        sec = number(s, at + 1, 2)?;
        at += 3;
    }
    if h > 23 || m > 59 || sec > 60 {
        return None;
    }
    let mut ms = 0;
    if matches!(s.get(at), Some(b'.' | b',')) && digit(s, at + 1).is_some() {
        at += 1;
        let mut n = 0;
        while let Some(d) = digit(s, at) {
            if n < 3 {
                ms = ms * 10 + d;
            }
            n += 1;
            at += 1;
        }
        for _ in n..3 {
            ms *= 10;
        }
    }
    Some(((h * 3600 + m * 60 + sec.min(59)) * 1000 + ms, at - i))
}

/// A zone after a time: `Z`, `+02:00`, `-0500`, ` +0000`, ` UTC`; the
/// milliseconds to take away for UTC, and the bytes taken.
fn zone(s: &[u8], i: usize) -> (i64, usize) {
    match s.get(i) {
        Some(b'Z' | b'z') => (0, 1),
        Some(&c @ (b'+' | b'-')) => {
            let Some(h) = number(s, i + 1, 2) else { return (0, 0) };
            let (m, n) = if s.get(i + 3) == Some(&b':') {
                (number(s, i + 4, 2), 6)
            } else if let Some(m) = number(s, i + 3, 2) {
                (Some(m), 5)
            } else {
                (Some(0), 3)
            };
            let Some(m) = m else { return (0, 0) };
            let off = (h * 60 + m) * 60_000;
            (if c == b'+' { off } else { -off }, n)
        }
        Some(b' ') if matches!(s.get(i + 1), Some(b'+' | b'-')) && digit(s, i + 2).is_some() => {
            let (off, n) = zone(s, i + 1);
            if n == 0 {
                (0, 0)
            } else {
                (off, n + 1)
            }
        }
        Some(b' ') if s.get(i + 1..i + 4).is_some_and(|w| w == b"UTC" || w == b"GMT") && !s.get(i + 4).is_some_and(u8::is_ascii_alphanumeric) => (0, 4),
        _ => (0, 0),
    }
}

/// A valid date, as days.
fn date(y: i64, m: i64, d: i64) -> Option<i64> {
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days(y, m as u32, d as u32))
}

/// The timestamp `s` starts with, if it does. `year` is the one for
/// dates written without it (syslog's `Mar  1`).
pub fn parse(s: &[u8], year: i64) -> Option<Stamp> {
    let first = *s.first()?;
    if first.is_ascii_digit() {
        // 2024-03-01T12:00:03.250Z, 2024/03/01 12:00:03, 2024-03-01_12:00:03
        if let (Some(y), Some(sep)) = (number(s, 0, 4), s.get(4).copied()) {
            if (sep == b'-' || sep == b'/') && s.get(7) == Some(&sep) {
                let day = date(y, number(s, 5, 2)?, number(s, 8, 2)?)?;
                if matches!(s.get(10), Some(b'T' | b' ' | b'_')) {
                    if let Some((t, n)) = clock(s, 11) {
                        let (off, z) = zone(s, 11 + n);
                        return Some(Stamp { ms: day * DAY + t - off, len: 11 + n + z });
                    }
                }
                return None;
            }
            // 20240301T120003Z, as some write ISO 8601 short
            if let (Some(m), Some(d), Some(b'T')) = (number(s, 4, 2), number(s, 6, 2), s.get(8)) {
                let day = date(y, m, d)?;
                let (h, mi, sec) = (number(s, 9, 2)?, number(s, 11, 2)?, number(s, 13, 2)?);
                let (off, z) = zone(s, 15);
                return Some(Stamp { ms: day * DAY + (h * 3600 + mi * 60 + sec) * 1000 - off, len: 15 + z });
            }
        }
        // 10/Oct/2000:13:55:36 -0700, Apache's and nginx's
        if let (Some(d), Some(b'/'), Some(m), Some(b'/'), Some(y), Some(b':')) = (number(s, 0, 2), s.get(2), month(s, 3), s.get(6), number(s, 7, 4), s.get(11))
        {
            let (t, n) = clock(s, 12)?;
            let (off, z) = zone(s, 12 + n);
            return Some(Stamp { ms: date(y, m as i64, d)? * DAY + t - off, len: 12 + n + z });
        }
        // 03-01 12:00:03.250, logcat's
        if let (Some(m), Some(b'-'), Some(d), Some(b' ')) = (number(s, 0, 2), s.get(2), number(s, 3, 2), s.get(5)) {
            let (t, n) = clock(s, 6)?;
            return Some(Stamp { ms: date(year, m, d)? * DAY + t, len: 6 + n });
        }
        // 12:00:03.250 alone: a time of day, on no day.
        if let Some((t, n)) = clock(s, 0) {
            if n >= 8 {
                return Some(Stamp { ms: t, len: n });
            }
        }
        return None;
    }
    // Mar  1 12:00:03, syslog's; Fri Mar  1 12:00:03.250 2024, ctime's
    let (at, weekday) = match s.get(0..3) {
        Some(w) if WEEKDAYS.iter().any(|d| w.eq_ignore_ascii_case(*d)) && s.get(3) == Some(&b' ') => (4, true),
        _ => (0, false),
    };
    let m = month(s, at)?;
    if s.get(at + 3) != Some(&b' ') {
        return None;
    }
    let mut i = at + 4;
    if s.get(i) == Some(&b' ') {
        i += 1;
    }
    let d = number(s, i, 2).map(|d| (d, 2)).or_else(|| number(s, i, 1).map(|d| (d, 1)))?;
    i += d.1;
    if s.get(i) != Some(&b' ') {
        return None;
    }
    let (t, n) = clock(s, i + 1)?;
    let mut end = i + 1 + n;
    let mut y = year;
    if weekday && s.get(end) == Some(&b' ') {
        if let Some(full) = number(s, end + 1, 4) {
            y = full;
            end += 5;
        }
    } else if s.get(end) == Some(&b' ') && number(s, end + 1, 4).is_some() && s.get(end + 5).is_none_or(|b| !b.is_ascii_digit()) {
        // Mar  1 12:00:03 2024
        y = number(s, end + 1, 4)?;
        end += 5;
    }
    Some(Stamp { ms: date(y, m as i64, d.0)? * DAY + t, len: end })
}

/// A time as a JSON or logfmt field holds it: one of the forms above, or
/// a number since 1970 -- seconds, milliseconds, microseconds or
/// nanoseconds, by its size.
pub fn parse_value(s: &str, year: i64) -> Option<i64> {
    let t = s.trim();
    if let Some(st) = parse(t.as_bytes(), year) {
        return Some(st.ms);
    }
    let n: f64 = t.parse().ok()?;
    epoch(n)
}

/// A number since 1970 in whichever unit its size says.
pub fn epoch(n: f64) -> Option<i64> {
    let ms = match n.abs() {
        a if a < 1e11 => n * 1000.0,
        a if a < 1e14 => n,
        a if a < 1e17 => n / 1000.0,
        _ => n / 1_000_000.0,
    };
    ms.is_finite().then_some(ms as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Option<(String, usize)> {
        parse(s.as_bytes(), 2024).map(|st| (format(st.ms), st.len))
    }

    #[test]
    fn days_and_dates_go_both_ways() {
        assert_eq!(days(1970, 1, 1), 0);
        assert_eq!(days(2000, 3, 1), 11_017);
        for d in [-1000, 0, 59, 11_016, 19_782, 30_000] {
            let (y, m, dd) = civil(d);
            assert_eq!(days(y, m, dd), d);
        }
    }

    #[test]
    fn iso_times_with_and_without_a_zone() {
        assert_eq!(at("2024-03-01T12:00:03.250Z rest"), Some(("2024-03-01 12:00:03.250".into(), 24)));
        assert_eq!(at("2024-03-01 12:00:03,123 - x"), Some(("2024-03-01 12:00:03.123".into(), 23)));
        assert_eq!(at("2024-03-01T12:00:03+02:00"), Some(("2024-03-01 10:00:03".into(), 25)));
        assert_eq!(at("2024-03-01T12:00:03.123456789-0500 x"), Some(("2024-03-01 17:00:03.123".into(), 34)));
        assert_eq!(at("2024/03/01 12:00:03 go"), Some(("2024-03-01 12:00:03".into(), 19)));
        assert_eq!(at("2024-03-01 12:00:03 UTC"), Some(("2024-03-01 12:00:03".into(), 23)));
        assert_eq!(at("20240301T120003Z"), Some(("2024-03-01 12:00:03".into(), 16)));
        assert_eq!(at("2024-13-01 12:00:03"), None, "no 13th month");
        assert_eq!(at("2024-03-01 nothing"), None);
    }

    #[test]
    fn syslog_apache_ctime_logcat_and_a_bare_clock() {
        assert_eq!(at("Mar  1 12:00:03 host sshd[1]: x"), Some(("2024-03-01 12:00:03".into(), 15)));
        assert_eq!(at("Nov 16 01:02:03 host"), Some(("2024-11-16 01:02:03".into(), 15)));
        assert_eq!(at("10/Oct/2000:13:55:36 -0700] \"GET"), Some(("2000-10-10 20:55:36".into(), 26)));
        assert_eq!(at("Wed Oct 11 14:32:52.123456 2000] [core"), Some(("2000-10-11 14:32:52.123".into(), 31)));
        assert_eq!(at("03-01 12:00:03.250  123  456 E Tag: x"), Some(("2024-03-01 12:00:03.250".into(), 18)));
        assert_eq!(at("12:00:03.250 something"), Some(("12:00:03.250".into(), 12)));
        assert_eq!(at("12:00 is not enough"), None);
        assert_eq!(at("March"), None);
    }

    #[test]
    fn numbers_since_1970_by_their_size() {
        let want = days(2023, 11, 14) * DAY + (22 * 3600 + 13 * 60 + 20) * 1000;
        assert_eq!(parse_value("1700000000", 2024), Some(want));
        assert_eq!(parse_value("1700000000123", 2024), Some(want + 123));
        assert_eq!(parse_value("1700000000123456", 2024), Some(want + 123));
        assert_eq!(parse_value("1700000000.5", 2024), Some(want + 500));
        assert_eq!(parse_value("2024-03-01T00:00:00Z", 2024), Some(days(2024, 3, 1) * DAY));
        assert_eq!(parse_value("soon", 2024), None);
        assert_eq!(span(90_000), "90 s");
        assert_eq!(span(3 * DAY), "3 d");
    }
}
