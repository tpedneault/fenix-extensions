//! What a log's line says of itself: its level, its time, and whether it
//! starts an entry -- read from the formats logs are written in: JSON
//! lines (`"level":`), logfmt (`level=`), bracketed levels (`[WARN]`),
//! Rust's env_logger and tracing, log4j and logback, Python's logging,
//! syslog (`<34>`), journalctl's text and JSON, logcat, glog, .NET's,
//! logrus's, Apache's access and error logs -- and patterns of your own.

use regex::bytes::Regex;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::time;

/// How much of a line is read to tell what it is: the rest of a longer
/// one is left alone.
pub const CAP: usize = 64 * 1024;
/// How far into a line's text a level is looked for.
const LOOK: usize = 200;

/// A line's level, the worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
    /// A line that says none.
    Other,
}

/// How many levels there are.
pub const LEVELS: usize = 7;

impl Level {
    pub const ALL: [Level; LEVELS] = [Level::Fatal, Level::Error, Level::Warn, Level::Info, Level::Debug, Level::Trace, Level::Other];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Level {
        Level::ALL.get(i).copied().unwrap_or(Level::Other)
    }

    /// Its badge in the page's gutter.
    pub fn badge(self) -> char {
        match self {
            Level::Fatal => 'F',
            Level::Error => 'E',
            Level::Warn => 'W',
            Level::Info => 'I',
            Level::Debug => 'D',
            Level::Trace => 'T',
            Level::Other => ' ',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Fatal => "fatal",
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
            Level::Other => "other",
        }
    }

    /// Error or worse.
    pub fn is_error(self) -> bool {
        self <= Level::Error
    }

    /// A word for a level, as logs write it, any case: `WARNING`,
    /// `err`, `Information`, `CRIT`, `dbug`, logrus's `ERRO`.
    pub fn word(w: &str) -> Option<Level> {
        let w = w.trim();
        if w.len() > 11 {
            return None;
        }
        let lower = w.to_ascii_lowercase();
        Some(match lower.as_str() {
            "fatal" | "fata" | "critical" | "crit" | "emerg" | "emergency" | "alert" | "panic" | "pani" | "f" | "c" => Level::Fatal,
            "error" | "err" | "erro" | "eror" | "severe" | "fail" | "failure" | "e" => Level::Error,
            "warn" | "warning" | "wrn" | "w" => Level::Warn,
            "info" | "information" | "informational" | "inf" | "notice" | "note" | "i" | "n" => Level::Info,
            "debug" | "debu" | "dbug" | "dbg" | "fine" | "config" | "d" => Level::Debug,
            "trace" | "trac" | "trce" | "trc" | "finer" | "finest" | "verbose" | "verb" | "v" | "t" => Level::Trace,
            _ => return None,
        })
    }

    /// A number for a level: syslog's severities 0 to 7, else pino's and
    /// bunyan's 10 to 60.
    pub fn number(n: f64) -> Level {
        if n < 10.0 {
            return Level::syslog(n.max(0.0) as u8);
        }
        match n as i64 {
            60.. => Level::Fatal,
            50..=59 => Level::Error,
            40..=49 => Level::Warn,
            30..=39 => Level::Info,
            20..=29 => Level::Debug,
            _ => Level::Trace,
        }
    }

    /// A syslog severity: emerg, alert and crit are fatal, notice info.
    pub fn syslog(n: u8) -> Level {
        match n {
            0..=2 => Level::Fatal,
            3 => Level::Error,
            4 => Level::Warn,
            5 | 6 => Level::Info,
            _ => Level::Debug,
        }
    }
}

/// Patterns of your own (`logs.patterns`), checked before the rest, and
/// the year for dates written without one.
#[derive(Debug, Clone)]
pub struct Rules {
    pub custom: Vec<(Regex, Level)>,
    pub year: i64,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { custom: Vec::new(), year: 1970 }
    }
}

impl Rules {
    /// The rules `logs.patterns` gives, as JSON pairs of a regular
    /// expression and a level (`[["^!!", "error"]]`); those that don't
    /// read are left out, and said.
    pub fn new(patterns: &[(String, String)], year: i64) -> (Rules, Vec<String>) {
        let mut bad = Vec::new();
        let mut custom = Vec::new();
        for (p, l) in patterns {
            let Some(level) = Level::word(l).or_else(|| (l.trim() == "other" || l.trim() == "none").then_some(Level::Other)) else {
                bad.push(format!("{p}: {l:?} isn't a level"));
                continue;
            };
            match Regex::new(p) {
                Ok(re) => custom.push((re, level)),
                Err(e) => bad.push(format!("{p}: {e}")),
            }
        }
        (Rules { custom, year }, bad)
    }
}

/// What a line says of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub level: Level,
    pub time: Option<i64>,
    /// Whether it starts an entry: a time or a level at its start, a JSON
    /// object, a logfmt record. A line that starts with a space never
    /// does.
    pub head: bool,
}

impl Seen {
    const NOTHING: Seen = Seen { level: Level::Other, time: None, head: false };
}

/// What `line` (without its newline, at most [`CAP`] of it) says.
pub fn detect(line: &[u8], rules: &Rules) -> Seen {
    let line = &line[..line.len().min(CAP)];
    if line.first().is_none_or(|b| b.is_ascii_whitespace()) {
        let mut seen = Seen::NOTHING;
        // An indented line starts nothing, but may say what went wrong:
        // what's after it then counts when the entry's head said nothing.
        seen.level = known_trouble(trim_start(line));
        return seen;
    }
    if let Some(seen) = journal_export(line) {
        return seen;
    }
    for (re, level) in &rules.custom {
        if re.is_match(&line[..line.len().min(LOOK * 4)]) {
            let mut seen = plain(line, rules.year);
            seen.level = *level;
            seen.head = true;
            return seen;
        }
    }
    if line[0] == b'{' {
        if let Some(seen) = json(line, rules.year) {
            return seen;
        }
    }
    if let Some(seen) = logfmt(line, rules.year) {
        return seen;
    }
    plain(line, rules.year)
}

/// The format an entry's first line is written in, as `logs_summary`
/// names it: the branch of [`detect`] that reads it.
pub fn format(line: &[u8], rules: &Rules) -> &'static str {
    let line = &line[..line.len().min(CAP)];
    if line.first().is_none_or(|b| b.is_ascii_whitespace()) {
        return "text";
    }
    if line.starts_with(b"__CURSOR=") {
        return "journalctl export";
    }
    if rules.custom.iter().any(|(re, _)| re.is_match(&line[..line.len().min(LOOK * 4)])) {
        return "your patterns (logs.patterns)";
    }
    if line[0] == b'{' && json(line, rules.year).is_some() {
        return if memchr::memmem::find(line, b"\"__CURSOR\"").is_some() { "journalctl JSON" } else { "JSON lines" };
    }
    if logfmt(line, rules.year).is_some() {
        return "logfmt";
    }
    let seen = plain(line, rules.year);
    if line[0] == b'<' && seen.head {
        return "syslog";
    }
    match (seen.time.is_some(), seen.level != Level::Other) {
        (true, true) => "text with a time and a level",
        (true, false) => "text with a time",
        (false, true) => "text with a level",
        (false, false) => "text",
    }
}

/// A line of `journalctl -o export`: an entry is its fields a line
/// each, starting with `__CURSOR=`; its level is `PRIORITY=`, its time
/// `__REALTIME_TIMESTAMP=` (microseconds).
fn journal_export(line: &[u8]) -> Option<Seen> {
    if line.starts_with(b"__CURSOR=") {
        return Some(Seen { head: true, ..Seen::NOTHING });
    }
    if let Some(p) = line.strip_prefix(b"PRIORITY=") {
        let n = std::str::from_utf8(p).ok()?.trim().parse::<u8>().ok()?;
        return Some(Seen { level: Level::syslog(n), ..Seen::NOTHING });
    }
    if let Some(us) = line.strip_prefix(b"__REALTIME_TIMESTAMP=") {
        let us = std::str::from_utf8(us).ok()?.trim().parse::<i64>().ok()?;
        return Some(Seen { time: Some(us / 1000), ..Seen::NOTHING });
    }
    None
}

/// The fields of a `journalctl -o export` entry a row shows:
/// `sshd[42]: Accepted publickey`.
pub fn journal_summary(entry: &[u8]) -> Option<String> {
    if !entry.starts_with(b"__CURSOR=") {
        return None;
    }
    let mut who = None;
    let mut pid = None;
    let mut message = None;
    for line in entry.split(|&b| b == b'\n') {
        let text = |p: &[u8]| String::from_utf8_lossy(&line[p.len()..]).into_owned();
        if line.starts_with(b"MESSAGE=") {
            message = Some(text(b"MESSAGE="));
        } else if line.starts_with(b"SYSLOG_IDENTIFIER=") {
            who = Some(text(b"SYSLOG_IDENTIFIER="));
        } else if line.starts_with(b"_COMM=") && who.is_none() {
            who = Some(text(b"_COMM="));
        } else if line.starts_with(b"_PID=") {
            pid = Some(text(b"_PID="));
        }
    }
    let message = message.unwrap_or_default();
    Some(match (who, pid) {
        (Some(w), Some(p)) => format!("{w}[{p}]: {message}"),
        (Some(w), None) => format!("{w}: {message}"),
        _ => message,
    })
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &s[n..]
}

/// What a line that starts no format's head says anyway: a Rust panic, a
/// Java exception, a Python traceback, a Go panic.
fn known_trouble(s: &[u8]) -> Level {
    let starts = |p: &[u8]| s.starts_with(p);
    if (starts(b"thread '") && memchr::memmem::find(s, b"' panicked at").is_some()) || starts(b"panic: ") || starts(b"fatal error: ") {
        Level::Fatal
    } else if starts(b"Traceback (most recent call last)") || starts(b"Exception in thread ") || starts(b"Caused by: ") {
        Level::Error
    } else {
        Level::Other
    }
}

/// A JSON object's level and time, by the keys loggers use for them.
fn json(line: &[u8], year: i64) -> Option<Seen> {
    let end = line.iter().rposition(|b| !b.is_ascii_whitespace())?;
    if line[end] != b'}' {
        return None;
    }
    let mut de = serde_json::Deserializer::from_slice(&line[..=end]);
    let probe = Probe::deserialize(&mut de).ok()?;
    let level = probe.level.or(probe.priority).unwrap_or(Level::Other);
    let time = probe.time.as_deref().and_then(|t| time::parse_value(t, year)).or(probe.journal_us.map(|us| us / 1000));
    Some(Seen { level, time, head: true })
}

/// The fields of a JSON line a level and a time are read from; the rest
/// are skipped as they're read.
#[derive(Default)]
struct Probe {
    level: Option<Level>,
    /// journald's `PRIORITY`, a syslog severity.
    priority: Option<Level>,
    time: Option<String>,
    /// journald's `__REALTIME_TIMESTAMP`, in microseconds.
    journal_us: Option<i64>,
}

const LEVEL_KEYS: [&str; 9] = ["level", "lvl", "severity", "loglevel", "log_level", "levelname", "log.level", "@l", "@level"];
const TIME_KEYS: [&str; 10] = ["time", "timestamp", "ts", "@timestamp", "@t", "t", "datetime", "asctime", "date", "logged_at"];

impl<'de> Deserialize<'de> for Probe {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Probe, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Probe;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Probe, A::Error> {
                let mut p = Probe::default();
                let mut time_rank = usize::MAX;
                while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
                    let k = key.to_ascii_lowercase();
                    if p.level.is_none() && LEVEL_KEYS.contains(&k.as_str()) {
                        p.level = match map.next_value::<Scalar>()? {
                            Scalar::Text(s) => Level::word(&s).or_else(|| s.trim().parse::<f64>().ok().map(Level::number)),
                            Scalar::Number(n) => Some(Level::number(n)),
                            Scalar::None => None,
                        };
                    } else if key == "PRIORITY" || k == "syslog.severity" {
                        p.priority = match map.next_value::<Scalar>()? {
                            Scalar::Text(s) => s.trim().parse::<u8>().ok().map(Level::syslog),
                            Scalar::Number(n) => Some(Level::syslog(n as u8)),
                            Scalar::None => None,
                        };
                    } else if let Some(rank) = TIME_KEYS.iter().position(|t| *t == k).filter(|&r| r < time_rank) {
                        if let Some(t) = map.next_value::<Scalar>()?.text() {
                            p.time = Some(t);
                            time_rank = rank;
                        }
                    } else if key == "__REALTIME_TIMESTAMP" {
                        p.journal_us = map.next_value::<Scalar>()?.text().and_then(|s| s.parse().ok());
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(p)
            }
        }
        d.deserialize_map(V)
    }
}

/// A JSON value read for a level or a time: text, a number, or anything
/// else, skipped.
enum Scalar {
    Text(String),
    Number(f64),
    None,
}

impl Scalar {
    fn text(self) -> Option<String> {
        match self {
            Scalar::Text(s) => Some(s),
            Scalar::Number(n) => Some(if n.fract() == 0.0 && n.abs() < 1e18 { format!("{}", n as i64) } else { n.to_string() }),
            Scalar::None => None,
        }
    }
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Scalar, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Scalar;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a value")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Scalar, E> {
                Ok(Scalar::Number(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Scalar, E> {
                Ok(Scalar::Number(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Scalar, E> {
                Ok(Scalar::Number(v as f64))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Scalar, E> {
                Ok(Scalar::None)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar::None)
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Scalar, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Scalar::None)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Scalar, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Scalar::None)
            }
        }
        d.deserialize_any(V)
    }
}

/// A logfmt record's pairs: `key=value`, `key="a value"`, in order --
/// for a line with at least two of them.
pub fn logfmt_pairs(line: &[u8]) -> Vec<(&[u8], Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        let k0 = i;
        while i < line.len() && !matches!(line[i], b'=' | b' ' | b'"') {
            i += 1;
        }
        if i >= line.len() || line[i] != b'=' || i == k0 {
            // Not a pair: skip the word.
            while i < line.len() && line[i] != b' ' {
                i += 1;
            }
            continue;
        }
        let key = &line[k0..i];
        i += 1;
        let mut value = Vec::new();
        if line.get(i) == Some(&b'"') {
            i += 1;
            while i < line.len() && line[i] != b'"' {
                if line[i] == b'\\' && i + 1 < line.len() {
                    i += 1;
                }
                value.push(line[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < line.len() && line[i] != b' ' {
                value.push(line[i]);
                i += 1;
            }
        }
        out.push((key, value));
    }
    out
}

/// A logfmt line's level and time, when it has a `level=` (or `lvl=`,
/// `severity=`) or a `time=` (`ts=`, `t=`) pair.
fn logfmt(line: &[u8], year: i64) -> Option<Seen> {
    // Cheap first: a level or time key at a word's start.
    let has = |k: &[u8]| memchr::memmem::find_iter(line, k).any(|at| at == 0 || line[at - 1] == b' ');
    if !(has(b"level=") || has(b"lvl=") || has(b"severity=") || has(b"time=") || has(b"ts=")) {
        return None;
    }
    let pairs = logfmt_pairs(&line[..line.len().min(LOOK * 8)]);
    if pairs.len() < 2 {
        return None;
    }
    let mut seen = Seen { head: true, ..Seen::NOTHING };
    for (k, v) in &pairs {
        let v = String::from_utf8_lossy(v);
        match k.to_ascii_lowercase().as_slice() {
            b"level" | b"lvl" | b"severity" if seen.level == Level::Other => seen.level = Level::word(&v).unwrap_or(Level::Other),
            b"time" | b"ts" | b"t" | b"timestamp" if seen.time.is_none() => seen.time = time::parse_value(&v, year),
            _ => {}
        }
    }
    // A line that starts with its own timestamp (`2024-... level=info`).
    if seen.time.is_none() {
        seen.time = time::parse(line, year).map(|s| s.ms);
    }
    Some(seen)
}

/// A line of text: a timestamp at its start (after a syslog priority, a
/// `[`), and the first word in its first part that's a level.
fn plain(line: &[u8], year: i64) -> Seen {
    let mut seen = Seen::NOTHING;
    let mut i = 0;
    // <34>, syslog's priority, and RFC 5424's version after it.
    if line[0] == b'<' {
        if let Some(close) = line.iter().take(5).position(|&b| b == b'>') {
            if let Ok(pri) = std::str::from_utf8(&line[1..close]).unwrap_or("").parse::<u8>() {
                seen.level = Level::syslog(pri & 7);
                seen.head = true;
                i = close + 1;
                if line.get(i) == Some(&b'1') && line.get(i + 1) == Some(&b' ') {
                    i += 2;
                }
            }
        }
    }
    // glog: E0301 12:00:03.250000  1234 file.cc:12] message
    if let (Some(&c), true) = (line.first(), line.len() > 6 && line[1..5].iter().all(u8::is_ascii_digit) && line[5] == b' ') {
        if let Some(level) = matches!(c, b'I' | b'W' | b'E' | b'F').then(|| Level::word(&(c as char).to_string())).flatten() {
            let rest = &line[6..];
            if let Some(st) = time::parse(rest, year) {
                let (m, d) = ((line[1] - b'0') * 10 + (line[2] - b'0'), (line[3] - b'0') * 10 + (line[4] - b'0'));
                let day = time::days(year, m.clamp(1, 12) as u32, d.clamp(1, 31) as u32);
                return Seen { level, time: Some(day * time::DAY + st.ms.rem_euclid(time::DAY)), head: true };
            }
        }
    }
    let bracket = line.get(i) == Some(&b'[');
    let at = if bracket { i + 1 } else { i };
    let mut after = at;
    if let Some(st) = time::parse(&line[at..], year) {
        seen.time = Some(st.ms);
        seen.head = true;
        after = at + st.len;
    } else if let Some(open) = line.iter().take(64).position(|&b| b == b'[') {
        // 127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET ...": the time
        // in brackets near the start.
        if let Some(st) = time::parse(&line[open + 1..], year) {
            seen.time = Some(st.ms);
            seen.head = true;
            after = open + 1 + st.len;
            if let Some(status) = http_status(line) {
                seen.level = status;
                return seen;
            }
        }
    }
    if seen.level == Level::Other {
        if let Some((level, first)) = level_word(&line[after..line.len().min(after + LOOK)], after == 0) {
            seen.level = level;
            // `[WARN] ...`, `WARNING:root:...`, `info: ...` start entries
            // too.
            if first {
                seen.head = true;
            }
        }
    }
    if seen.level == Level::Other {
        seen.level = known_trouble(line);
        // What a runtime prints itself, not through the log, starts an
        // entry of its own: a panic, an uncaught exception.
        seen.head |= seen.level == Level::Fatal || line.starts_with(b"Exception in thread ");
    }
    seen
}

/// An access log's status after the request: 5xx an error, 4xx a
/// warning, the rest info.
fn http_status(line: &[u8]) -> Option<Level> {
    let at = memchr::memmem::find(line, b" HTTP/")?;
    let close = at + memchr::memchr(b'"', &line[at..])?;
    let rest = line.get(close + 1..close + 5)?;
    if rest[0] != b' ' || !rest[1..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(match rest[1] {
        b'5' => Level::Error,
        b'4' => Level::Warn,
        _ => Level::Info,
    })
}

/// The first word of `s` that's a level, and whether it was the first
/// word: an upper-case one anywhere (`ERROR`, `WARN`); one in another
/// case (`info`, `Warning`) when it's bracketed (`[info]`, `<warn>`) or
/// first and followed by `:` or a space; a single letter (`E`, logcat's)
/// only bracketed or as the first word, alone.
fn level_word(s: &[u8], line_start: bool) -> Option<(Level, bool)> {
    let mut i = 0;
    let mut words = 0;
    while i < s.len() && words < 12 {
        if !s[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < s.len() && s[i].is_ascii_alphabetic() {
            i += 1;
        }
        let word = &s[start..i];
        let first = words == 0;
        words += 1;
        if word.len() > 11 {
            continue;
        }
        let before = start.checked_sub(1).map(|j| s[j]);
        let after = s.get(i).copied();
        // A word glued to digits or underscores (`E1234`, `ERROR_CODE`,
        // `x.error`) is part of something else.
        let glued = |b: Option<u8>| b.is_some_and(|b| b.is_ascii_digit() || b == b'_' || b == b'.' || b == b'/' || b == b'-' && word.len() > 1);
        if glued(before) || glued(after) && after != Some(b'-') {
            continue;
        }
        let Some(level) = Level::word(std::str::from_utf8(word).unwrap_or("")) else { continue };
        // `[info]`, `<warn>`, and Apache's `[core:error]`.
        let bracketed = matches!(before, Some(b'[' | b'<' | b'(' | b'|' | b':')) && matches!(after, Some(b']' | b'>' | b')' | b'|'));
        let upper = word.iter().all(u8::is_ascii_uppercase);
        let at_start = first && (start == 0 || !line_start);
        let ok = if word.len() == 1 {
            upper && (bracketed || (first && matches!(before, Some(b' ')) && after == Some(b' ')))
        } else {
            upper || bracketed || (at_start && matches!(after, None | Some(b':' | b' ' | b']')))
        };
        if ok {
            return Some((level, first && start < 24));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules { custom: Vec::new(), year: 2024 }
    }

    fn seen(line: &str) -> (Level, Option<String>, bool) {
        let s = detect(line.as_bytes(), &rules());
        (s.level, s.time.map(time::format), s.head)
    }

    fn level(line: &str) -> Level {
        seen(line).0
    }

    #[test]
    fn json_lines_by_their_keys() {
        assert_eq!(seen(r#"{"time":"2024-03-01T12:00:03Z","level":"warn","msg":"x"}"#), (Level::Warn, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(seen(r#"{"level":50,"time":1709294403000,"msg":"pino"}"#), (Level::Error, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(level(r#"{"@t":"2024-03-01T12:00:03Z","@l":"Information","@m":"serilog"}"#), Level::Info);
        assert_eq!(level(r#"{"severity":"CRITICAL","message":"gcp"}"#), Level::Fatal);
        assert_eq!(level(r#"{"log.level":"debug","@timestamp":"2024-03-01T12:00:03Z"}"#), Level::Debug);
        let journal = r#"{"__REALTIME_TIMESTAMP":"1709294403000000","PRIORITY":"3","MESSAGE":"journald","_SYSTEMD_UNIT":"x.service"}"#;
        assert_eq!(seen(journal), (Level::Error, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(level(r#"{"level":"info","nested":{"level":"error"},"list":[1,{"a":2}]}"#), Level::Info, "nested keys don't count");
        assert_eq!(seen(r#"{"msg":"no level"}"#), (Level::Other, None, true));
        assert_eq!(level(r#"{"ts":1709294403.5,"level":"trace"}"#), Level::Trace);
        assert_eq!(seen(r#"{"broken": "#), (Level::Other, None, false), "not JSON after all");
    }

    #[test]
    fn logfmt_pairs_and_records() {
        assert_eq!(seen(r#"time=2024-03-01T12:00:03Z level=error msg="it broke" err="EOF""#), (Level::Error, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(level("ts=1709294403 lvl=warn caller=main.go:12"), Level::Warn);
        let pairs = logfmt_pairs(br#"a=1 b="two words" c=x\"y d"#);
        assert_eq!(
            pairs.iter().map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned())).collect::<Vec<_>>(),
            [("a".into(), "1".into()), ("b".into(), "two words".into()), ("c".into(), "x\\\"y".into())]
        );
    }

    #[test]
    fn bracketed_and_logger_formats() {
        assert_eq!(seen("[WARN] disk is filling"), (Level::Warn, None, true));
        assert_eq!(seen("[2024-03-01T12:00:03Z INFO  my_crate::db] connected"), (Level::Info, Some("2024-03-01 12:00:03".into()), true), "env_logger");
        assert_eq!(seen("2024-03-01T12:00:03.123456Z  WARN my_crate: slow"), (Level::Warn, Some("2024-03-01 12:00:03.123".into()), true), "tracing");
        assert_eq!(level("2024-03-01 12:00:03,123 [main] ERROR com.acme.App - boom"), Level::Error, "log4j");
        assert_eq!(level("2024-03-01 12:00:03.123  INFO 4242 --- [main] c.a.App : Started"), Level::Info, "Spring");
        assert_eq!(level("2024-03-01 12:00:03,123 - app.db - WARNING - retrying"), Level::Warn, "Python");
        assert_eq!(seen("WARNING:root:basicConfig's"), (Level::Warn, None, true), "Python's default");
        assert_eq!(seen("CRITICAL:app:gone"), (Level::Fatal, None, true));
        assert_eq!(seen("info: Microsoft.Hosting.Lifetime[0]"), (Level::Info, None, true), ".NET");
        assert_eq!(level("fail: Microsoft.AspNetCore[1]"), Level::Error);
        assert_eq!(level("ERRO[0000] logrus says"), Level::Error);
        assert_eq!(level("2024-03-01 12:00:03 [error] 123#0: nginx"), Level::Error);
        assert_eq!(level("[Wed Oct 11 14:32:52.123456 2000] [core:error] [pid 1] apache"), Level::Error);
        assert_eq!(level("2024-03-01 12:00:03 <debug> x"), Level::Debug);
        assert_eq!(level("2024-03-01 12:00:03 Started the server: no error"), Level::Other, "a word in the message in lower case isn't a level");
        assert_eq!(level("2024-03-01 12:00:03 INFO request ended in ERROR"), Level::Info, "the first level counts");
        assert_eq!(level("2024-03-01 12:00:03 user_error_count=3"), Level::Other);
        assert_eq!(level("2024-03-01 12:00:03 E1234 not a level"), Level::Other);
    }

    #[test]
    fn syslog_journal_logcat_glog_and_access_logs() {
        assert_eq!(seen("<34>1 2024-03-01T12:00:03Z host app 1 - - crit thing"), (Level::Fatal, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(seen("<14>Mar  1 12:00:03 host app: hello"), (Level::Info, Some("2024-03-01 12:00:03".into()), true));
        assert_eq!(seen("Mar  1 12:00:03 host sshd[42]: Accepted publickey"), (Level::Other, Some("2024-03-01 12:00:03".into()), true), "journalctl's short");
        assert_eq!(level("Mar  1 12:00:03 host kernel: [ 1.0] usb 1-1: ERROR reading"), Level::Error);
        assert_eq!(seen("03-01 12:00:03.250  1234  1234 E ActivityManager: ANR"), (Level::Error, Some("2024-03-01 12:00:03.250".into()), true), "logcat");
        assert_eq!(level("03-01 12:00:03.250  1234  1234 I Tag: hi"), Level::Info);
        assert_eq!(seen("W0301 12:00:03.250000  1234 file.cc:12] glog"), (Level::Warn, Some("2024-03-01 12:00:03.250".into()), true));
        assert_eq!(seen(r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /a HTTP/1.1" 503 12"#), (Level::Error, Some("2000-10-10 20:55:36".into()), true));
        assert_eq!(level(r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /a HTTP/1.1" 404 12"#), Level::Warn);
        assert_eq!(level(r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /a HTTP/1.1" 200 12"#), Level::Info);
    }

    #[test]
    fn indented_lines_and_trouble_start_nothing() {
        assert_eq!(seen("    at com.acme.App.main(App.java:12)"), (Level::Other, None, false));
        assert_eq!(seen("\tat x"), (Level::Other, None, false));
        assert_eq!(seen(""), (Level::Other, None, false));
        assert_eq!(seen("Traceback (most recent call last):"), (Level::Error, None, false));
        assert_eq!(seen("thread 'main' panicked at src/main.rs:2:5:"), (Level::Fatal, None, true), "a panic starts an entry of its own");
        assert_eq!(seen("Exception in thread \"main\" java.lang.Error"), (Level::Error, None, true));
        assert_eq!(seen("ValueError: bad"), (Level::Other, None, false));
    }

    #[test]
    fn journalctl_s_export_format_field_by_field() {
        assert_eq!(seen("__CURSOR=s=abc;i=1"), (Level::Other, None, true));
        assert_eq!(seen("PRIORITY=3"), (Level::Error, None, false));
        assert_eq!(seen("__REALTIME_TIMESTAMP=1709294403000000"), (Level::Other, Some("2024-03-01 12:00:03".into()), false));
        let entry = b"__CURSOR=s=1\n_PID=42\nSYSLOG_IDENTIFIER=sshd\nMESSAGE=Accepted publickey\n";
        assert_eq!(journal_summary(entry).as_deref(), Some("sshd[42]: Accepted publickey"));
        assert_eq!(journal_summary(b"plain"), None);
    }

    #[test]
    fn patterns_of_your_own_come_first() {
        let (r, bad) = Rules::new(&[("^!!".into(), "error".into()), ("(".into(), "warn".into()), ("x".into(), "loud".into())], 2024);
        assert_eq!(bad.len(), 2, "{bad:?}");
        assert_eq!(detect(b"!! it broke", &r), Seen { level: Level::Error, time: None, head: true });
        assert_eq!(detect(b"2024-03-01 12:00:03 INFO !! no", &r).level, Level::Info);
    }
}
