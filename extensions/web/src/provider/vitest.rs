//! What Fenix's Vitest reporter (`src/tests/fenix-vitest.mjs`) says, as
//! reports. Its lines, as Vitest 5.1 printed them for the sample:
//!
//! | `kind` | Is | Reported |
//! |---|---|---|
//! | `test` | a test collected, with its line and `mode` (`run`, `skip`, `todo`; every test `skip` in a listing, and one `-t` left out) | `Found` for one no scan found (a `test.each` case under its template); its `todo` remembered |
//! | `log` | what a test logged, before its result | its `Output`, with its result |
//! | `result` | a test's end: `passed`, `failed`, `skipped` (a `todo` too), its time, its errors' messages, stacks and diffs | `Ended` (none in a listing, none for a skip `-t` left out); a failure at its stack's first frame in the project |
//! | `module-error` | a file that didn't load (a syntax error): its tests never collected | `Broken` on the file |
//! | `run-error` | an error outside any test; `Worker exited unexpectedly with signal SIGKILL … while running test file <path>` when a worker died | `Crashed` on that file (its tests have no results: Vitest reports a file's tests only once it ends); else the run's `Output` |

use fenix_lang_kit::tests::{Ended, Outcome, Report};

use super::read::{first_frame, first_line, outcome, plain, Format, Purpose, Read, Said};

#[derive(Default)]
pub struct Vitest {
    /// The tests whose `mode` was `todo`.
    todo: std::collections::HashSet<String>,
}

/// A worker's death, from the error Vitest gives for it: why, and the
/// file it was running.
pub fn worker_died(cause: &str) -> Option<(String, &str)> {
    let rest = cause.strip_prefix("Worker exited unexpectedly")?;
    let file = rest.split_once("while running test file ").map(|(_, f)| f.trim())?;
    let why = if let Some(s) = rest.split_once("with signal ").map(|(_, s)| s.split_whitespace().next().unwrap_or("")) {
        format!("its worker was killed: {s}")
    } else if let Some(c) = rest.split_once("with code ").map(|(_, c)| c.split_whitespace().next().unwrap_or("")) {
        format!("its worker exited with {c}")
    } else {
        "its worker exited unexpectedly".to_string()
    };
    Some((why, file))
}

impl Format for Vitest {
    fn line(&mut self, r: &mut Read, said: &Said, kind: &str, out: &mut Vec<Report>) {
        match kind {
            "test" => {
                let Some(t) = r.named(said, "test") else { return };
                if said.str("mode") == Some("todo") {
                    self.todo.insert(t.key.clone());
                }
                r.found(said, &t, out);
            }
            "log" => {
                let Some(t) = r.named(said, "log") else { return };
                // Each log is its own line: counted, a repeated name's
                // logs would go to the next of its name.
                r.forget(&t, "log");
                if let Some(text) = said.str("text") {
                    r.printed.entry(t.key).or_default().push_str(&plain(text));
                }
            }
            "result" if r.ctx.purpose == Purpose::Run => {
                let Some(t) = r.named(said, "result") else { return };
                let Some(o) = said.str("state").and_then(outcome) else { return };
                if o == Outcome::Skipped && !r.wants(&t) {
                    r.printed.remove(&t.key);
                    return;
                }
                let mut e = Ended::new(t.key.clone(), o);
                // Vitest's times are `performance.now()`'s: to the µs.
                if let Some(ms) = said.v["duration"].as_f64() {
                    e = e.ms((ms * 1000.0).round() / 1000.0);
                }
                if o == Outcome::Skipped && self.todo.contains(&t.key) {
                    e = e.message("todo");
                }
                let errors = said.v["errors"].as_array().cloned().unwrap_or_default();
                if let Some(first) = errors.first() {
                    e.message = first["message"].as_str().and_then(first_line);
                }
                e.at = errors.iter().find_map(|x| x["stack"].as_str().and_then(|s| first_frame(&r.ctx, said.cwd, s)));
                let text: Vec<String> = errors
                    .iter()
                    .map(|x| {
                        let body = x["stack"].as_str().or(x["message"].as_str()).unwrap_or("");
                        match x["diff"].as_str() {
                            Some(d) => format!("{}\n\n{}", plain(body), plain(d)),
                            None => plain(body),
                        }
                    })
                    .collect();
                if !text.is_empty() {
                    e.text = Some(text.join("\n\n"));
                }
                r.ended(e, out);
            }
            "module-error" => {
                let Some(file) = said.str("file").and_then(|f| r.ctx.below(said.cwd, f)) else { return };
                let why = said.v["errors"][0]["message"].as_str().and_then(first_line).unwrap_or_else(|| "it didn't load".into());
                let at = said.v["errors"][0]["stack"].as_str().and_then(|s| first_frame(&r.ctx, said.cwd, s)).filter(|a| a.file == file);
                out.push(Report::Broken { under: Some(r.under(said.cwd, &file)), message: format!("{file} didn't load: {why}"), at });
            }
            "run-error" => {
                let error = &said.v["error"];
                let cause = error["cause"].as_str().unwrap_or("");
                match worker_died(cause).and_then(|(why, f)| Some((why, r.ctx.below(said.cwd, f)?))) {
                    Some((why, file)) => out.push(Report::Crashed { under: Some(r.under(said.cwd, &file)), message: why }),
                    None => {
                        let text = [error["stack"].as_str().or(error["message"].as_str()), Some(cause).filter(|c| !c.is_empty())].into_iter().flatten().map(plain).collect::<Vec<_>>().join("\nCaused by: ");
                        if !text.is_empty() {
                            out.push(Report::Output { key: None, text });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workers_death_says_why_and_its_file() {
        let cause = "Worker exited unexpectedly with signal SIGKILL during started state while running test file /p/src/crash.test.js";
        assert_eq!(worker_died(cause), Some(("its worker was killed: SIGKILL".into(), "/p/src/crash.test.js")));
        assert_eq!(worker_died("Worker exited unexpectedly with code 3 during started state while running test file C:\\p\\a.test.js"), Some(("its worker exited with 3".into(), "C:\\p\\a.test.js")));
        assert_eq!(worker_died("Something else"), None);
    }
}
