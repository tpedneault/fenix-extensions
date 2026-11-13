//! What Fenix's Jest reporter (`src/tests/fenix-jest.js`) says, as
//! reports. Its lines, as Jest 30.5 printed them for the sample:
//!
//! | `kind` | Is | Reported |
//! |---|---|---|
//! | `result` | a test that ran, as it ends: `passed`, `failed` (a `todo` too, as `todo`), its time, its failures' text with their stacks | `Ended` (none in a listing); a failure at its stack's first frame in the project, its message the failure's first line |
//! | `test` | at its file's end, a test that didn't run: `pending` (`.skip`, or left out by `-t`: every test in a listing), `todo`, `disabled` | in a listing, `Found`; in a run, `Found` for one no scan found and `Ended` skipped for one the step asked for |
//! | `file-error` | a file that didn't run: one that didn't load (`Unexpected token (3:0)`), or whose worker died (`A jest worker process (pid=…) was terminated by another process: signal=SIGKILL`) | `Broken` on the file, or `Crashed` on it |
//!
//! Jest runs a few quick files in its own process: a test that kills
//! that process ends the step by a signal, `crashed` for all it hadn't
//! ended ([`super::read::Read::end`]).

use fenix_lang_kit::tests::{At, Ended, Outcome, Report};

use super::read::{first_frame, first_line, outcome, plain, Format, Purpose, Read, Said};

#[derive(Default)]
pub struct Jest;

/// A worker's death, from the error Jest gives the file it ran.
pub fn worker_died(message: &str) -> Option<String> {
    if !message.contains("worker process") || !message.contains("terminated") && !message.contains("exited") {
        return None;
    }
    let signal = message.split_once("signal=").map(|(_, s)| s.split([',', ' ']).next().unwrap_or("")).filter(|s| !s.is_empty() && *s != "null");
    let code = message.split_once("exitCode=").map(|(_, s)| s.split([',', ' ', '.']).next().unwrap_or("")).filter(|s| !s.is_empty() && *s != "null");
    Some(match (signal, code) {
        (Some(s), _) => format!("its worker was killed: {s}"),
        (None, Some(c)) => format!("its worker exited with {c}"),
        _ => "its worker died".to_string(),
    })
}

/// Babel's place in a message's first line, `…: Unexpected token (3:0)`,
/// its column from 0.
fn babel_place(line: &str) -> Option<(u32, u32)> {
    let inner = line.trim_end().strip_suffix(')')?.rsplit_once('(')?.1;
    let (l, c) = inner.split_once(':')?;
    Some((l.parse().ok()?, c.parse::<u32>().ok()? + 1))
}

impl Format for Jest {
    fn line(&mut self, r: &mut Read, said: &Said, kind: &str, out: &mut Vec<Report>) {
        match kind {
            "result" if r.ctx.purpose == Purpose::Run => {
                let Some(t) = r.named(said, "result") else { return };
                r.found(said, &t, out);
                let Some(o) = said.str("state").and_then(outcome) else { return };
                if o == Outcome::Skipped && !r.wants(&t) {
                    return;
                }
                let mut e = Ended::new(t.key.clone(), o);
                if let Some(ms) = said.v["duration"].as_f64() {
                    e = e.ms(ms);
                }
                if said.str("state") == Some("todo") {
                    e = e.message("todo");
                }
                let failures: Vec<&str> = said.v["failures"].as_array().map(|a| a.iter().filter_map(|f| f.as_str()).collect()).unwrap_or_default();
                if let Some(first) = failures.first() {
                    e.message = first_line(first).map(|l| l.strip_prefix("Error: ").map(str::to_string).unwrap_or(l));
                    e.at = failures.iter().find_map(|f| first_frame(&r.ctx, said.cwd, f));
                    e.text = Some(failures.iter().map(|f| plain(f)).collect::<Vec<_>>().join("\n\n"));
                }
                r.ended(e, out);
            }
            "test" => {
                let Some(t) = r.named(said, "test") else { return };
                r.found(said, &t, out);
                if r.ctx.purpose == Purpose::Listing || r.ended.contains(&t.key) || !r.wants(&t) {
                    return;
                }
                let mut e = Ended::new(t.key.clone(), Outcome::Skipped);
                if said.str("state") == Some("todo") {
                    e = e.message("todo");
                }
                r.ended(e, out);
            }
            "file-error" => {
                let Some(file) = said.str("file").and_then(|f| r.ctx.below(said.cwd, f)) else { return };
                let message = said.str("message").unwrap_or("");
                if let Some(why) = worker_died(message) {
                    out.push(Report::Crashed { under: Some(r.under(said.cwd, &file)), message: why });
                    return;
                }
                let first = first_line(message).unwrap_or_else(|| "it didn't load".into());
                // Babel's message starts with the file's full path.
                let why = match first.split_once(": ") {
                    Some((path, rest)) if r.ctx.below(said.cwd, path).as_deref() == Some(file.as_str()) => rest.to_string(),
                    _ => first.clone(),
                };
                let at = babel_place(&why).map(|(l, c)| At::new(file.clone(), l, Some(c))).or_else(|| said.str("stack").and_then(|s| first_frame(&r.ctx, said.cwd, s)).filter(|a| a.file == file));
                out.push(Report::Broken { under: Some(r.under(said.cwd, &file)), message: format!("{file} didn't load: {why}"), at });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workers_death_and_babels_place() {
        let m = "A jest worker process (pid=30289) was terminated by another process: signal=SIGKILL, exitCode=null. Operating system logs may contain more information on why this occurred.";
        assert_eq!(worker_died(m).as_deref(), Some("its worker was killed: SIGKILL"));
        assert_eq!(worker_died("A jest worker process (pid=1) was terminated by another process: signal=null, exitCode=3.").as_deref(), Some("its worker exited with 3"));
        assert_eq!(worker_died("Cannot find module './x'"), None);
        assert_eq!(babel_place("Unexpected token (3:0)"), Some((3, 1)));
        assert_eq!(babel_place("Cannot find module"), None);
    }
}
