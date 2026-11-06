//! A job's log made readable: the terminal's colours, GitLab's section
//! markers and GitHub's timestamps taken off, a line overwritten with `\r`
//! kept as it ended -- and its tail, for a log too big to read whole.

/// The most of a log it reads: the end of it, which is where it failed.
pub const TAIL_BYTES: usize = 512 * 1024;

/// A log as text, clean.
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = strip_ansi(line);
        // GitLab: `section_start:1700000000:name\r` before the line's text,
        // `section_end:...\r` after it; with a `\r` in a line, what was
        // written last is what shows.
        let mut kept = "";
        for part in line.split('\r') {
            let part = part.trim_start_matches('\u{1b}');
            if part.starts_with("section_start:") || part.starts_with("section_end:") {
                continue;
            }
            if !part.is_empty() || kept.is_empty() {
                kept = part;
            }
        }
        out.push_str(strip_timestamp(kept));
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// `\x1b[...m` and the other CSI sequences, gone.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Parameters, then one final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// GitHub's `2026-11-06T09:41:19.0821044Z ` before each line, gone.
fn strip_timestamp(line: &str) -> &str {
    let b = line.as_bytes();
    let stamp = b.len() > 20 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[..4].iter().all(u8::is_ascii_digit);
    if !stamp {
        return line;
    }
    match line.find("Z ") {
        Some(i) if i < 32 => &line[i + 2..],
        _ if line.ends_with('Z') && line.len() < 32 => "",
        _ => line,
    }
}

/// A log's bytes as text. `partial`: they're its end, so the first line
/// is likely cut and goes.
pub fn text_of(body: &[u8], partial: bool) -> String {
    let text = String::from_utf8_lossy(body);
    if partial {
        match text.find('\n') {
            Some(i) => text[i + 1..].to_string(),
            None => text.into_owned(),
        }
    } else {
        text.into_owned()
    }
}

/// Its last `lines` lines.
pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.trim_end().lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The lines that say what went wrong: GitHub's `##[error]`, Rust's
/// `error:`/`panicked at`, `FAILED`, GitLab's `ERROR: Job failed`.
pub fn errors(text: &str, most: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        let hit = l.starts_with("##[error]") || l.starts_with("error") || l.starts_with("ERROR") || l.contains("panicked at") || l.ends_with("FAILED") || l.starts_with("FAIL ") || l.starts_with("E   ");
        if hit && !out.iter().any(|o| o == l) {
            out.push(l.trim_start_matches("##[error]").to_string());
            if out.len() == most {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn githubs_timestamps_and_colours_come_off() {
        let log = clean(include_str!("samples/github_log.txt"));
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines[0], "##[group]Run cargo test --workspace");
        assert_eq!(lines[1], "cargo test --workspace");
        assert_eq!(lines[4], "   Compiling rocket-telemetry v0.4.0 (/home/runner/work/rocket/rocket/telemetry)");
        assert!(!log.contains('\u{1b}') && !log.contains("2026-11-06T"), "{log}");
        assert_eq!(lines[8], "", "an empty line stays");
    }

    #[test]
    fn gitlabs_sections_go_and_overwritten_lines_keep_their_end() {
        let log = clean(include_str!("samples/gitlab_trace.txt"));
        assert!(!log.contains("section_") && !log.contains('\u{1b}') && !log.contains('\r'), "{log}");
        assert!(log.contains("\nPreparing the \"docker\" executor\n"), "{log}");
        assert!(log.contains("\nDownloading crates ... 100%\n") && !log.contains("10%"), "{log}");
        assert!(log.contains("\nERROR: Job failed: exit code 101"), "{log}");
        assert!(log.starts_with("Running with gitlab-runner 17.5.2"), "{log}");
    }

    #[test]
    fn a_tail_drops_its_cut_first_line() {
        assert_eq!(text_of(b"ut line\nwhole\nlast\n", true), "whole\nlast\n");
        assert_eq!(text_of(b"first\nlast", false), "first\nlast");
        assert_eq!(tail("a\nb\nc\nd\n", 2), "c\nd");
        assert_eq!(tail("a", 5), "a");
    }

    #[test]
    fn the_lines_that_failed_are_picked_out() {
        let log = clean(include_str!("samples/github_log.txt"));
        let errors = errors(&log, 10);
        assert_eq!(errors, [
            "test pus::tests::service_17_reply ... FAILED",
            "thread 'pus::tests::service_17_reply' panicked at telemetry/src/pus.rs:212:9:",
            "error: test failed, to rerun pass `-p rocket-telemetry --lib`",
            "Process completed with exit code 101.",
        ]);
    }
}
