//! The one place that runs kiln: a queue of jobs, each one kiln command
//! line, started with Fenix's `spawn` one at a time, its stdout and stderr
//! gathered from the events that come back, and finished on its exit with
//! what it printed and its exit code.
//!
//! - **One kiln at a time.** kiln is quick (Kiln's own project reads in
//!   well under a second), and its edits refuse a file that changed since
//!   it read it, so two at once could only race each other. A job waits
//!   its turn instead; the queue is first in, first out.
//! - **Nothing blocks.** Every method returns once it has asked Fenix for
//!   something; the answer is a later event, handed here by `lib.rs`
//!   ([`Kiln::output`], [`Kiln::exited`], [`Kiln::timer`]). What finished
//!   is taken with [`Kiln::take_finished`].
//! - **Where it runs**: no `cwd` is given, which Fenix's API says is the
//!   open project's folder -- its top, where `kiln.toml` is (the
//!   extension activates on that marker). kiln walks up from there to the
//!   nearest `kiln.toml`, as it does from a terminal at the project's top.
//! - **Which kiln**: the `kiln.path` setting as it is, when it's set (the
//!   manifest's `${setting:kiln.path}` lets it run); else `find_program`'s
//!   `kiln` (beside Fenix, its tools folder, the `PATH`). A path that's set
//!   is never silently replaced by another kiln found elsewhere: when it's
//!   wrong, starting it fails and says so.
//! - **Bounded**: stdout past [`MAX_STDOUT`] bytes, or a run past
//!   [`TIMEOUT_MS`], stops the process and fails the job saying which;
//!   stderr keeps its last [`MAX_STDERR_LINES`] lines.
//!
//! What it deliberately doesn't do: read kiln's JSON (`model.rs`), decide
//! when to reload or what an exit code means for a command (`project.rs`
//! and [`Ran::expect`] given the codes that are fine), or retry anything.

use std::collections::VecDeque;

use crate::api;
use crate::model;

/// The most stdout a kiln answer may have: 16 MiB. Kiln's own project
/// lists in 42 KiB (84 KiB with bodies); a project a hundred times its size
/// fits, and an extension's memory (128 MiB) keeps room for the answer
/// read into types beside its text. Past it the answer is cut, which
/// would be unreadable JSON, so the job fails instead.
pub const MAX_STDOUT: usize = 16 * 1024 * 1024;

/// The lines of stderr kept, the last ones: kiln's error is one line, and
/// a summary or two; more is noise.
pub const MAX_STDERR_LINES: usize = 50;

/// How long one kiln run may take: 60 seconds. A reading command takes
/// well under one; `check` with K207 on reads Jira (kiln caches it ten
/// minutes). A run past this is stopped, so the queue never waits forever
/// behind a kiln that hangs (a network share, a prompt nobody sees).
pub const TIMEOUT_MS: u32 = 60_000;

/// One kiln command line to run, and what its answer is for (`tag`, the
/// caller's own).
#[derive(Debug, Clone, PartialEq)]
pub struct Job<T> {
    /// kiln's arguments: `["list", "--format", "json"]`. Each is one
    /// argument as it is; no shell reads them.
    pub args: Vec<String>,
    /// Written to kiln's stdin, which is then closed (`new --body-file
    /// -`). Without it, stdin is closed at once: kiln never waits on it.
    pub stdin: Option<String>,
    pub tag: T,
}

impl<T> Job<T> {
    pub fn new<S: Into<String>>(args: impl IntoIterator<Item = S>, tag: T) -> Job<T> {
        Job {
            args: args.into_iter().map(Into::into).collect(),
            stdin: None,
            tag,
        }
    }

    pub fn stdin(mut self, text: impl Into<String>) -> Job<T> {
        self.stdin = Some(text.into());
        self
    }

    /// `kiln list --format json`, for messages.
    pub fn line(&self) -> String {
        std::iter::once("kiln")
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a kiln run that ended printed. Its exit code alone doesn't say
/// whether it went well -- `check` exits 1 with its findings on stdout --
/// so the caller says which codes are fine ([`Ran::expect`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// None when it ended without one (a signal).
    pub code: Option<i32>,
    /// Its lines, each ended by `\n` (Fenix gives them a line at a time).
    pub stdout: String,
    /// Its last lines on stderr.
    pub stderr: Vec<String>,
}

impl Ran {
    /// Its stdout, when it exited with one of `fine`; else why not.
    pub fn expect(&self, fine: &[i32]) -> Result<&str, Failure> {
        match self.code {
            Some(c) if fine.contains(&c) => Ok(&self.stdout),
            code => Err(Failure::Exit {
                code,
                line: self.error_line(),
            }),
        }
    }

    /// kiln's one-line error (`kiln: …`) as it wrote it: the last stderr
    /// line that starts so, else its last line, else its exit code.
    pub fn error_line(&self) -> String {
        let lines = || {
            self.stderr
                .iter()
                .rev()
                .map(|l| l.trim_end())
                .filter(|l| !l.is_empty())
        };
        lines()
            .find(|l| l.starts_with("kiln: "))
            .or_else(|| lines().next())
            .map(|l| model::clip(l, 500))
            .unwrap_or_else(|| match self.code {
                Some(c) => format!("kiln exited with code {c} and said nothing"),
                None => "kiln was stopped before it exited".into(),
            })
    }
}

/// Why a job has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No kiln to run: not on the PATH, or `kiln.path` can't be started.
    /// The line says which, and how to fix it.
    NotFound(String),
    /// Fenix couldn't start it (a permission, a broken file): its words.
    Spawn(String),
    /// It ended with a code the caller didn't expect: kiln's line.
    Exit { code: Option<i32>, line: String },
    /// Its stdout passed [`MAX_STDOUT`]; it was stopped.
    TooLarge,
    /// It ran past [`TIMEOUT_MS`]; it was stopped.
    TimedOut,
    /// Not run: kiln can't be used here now (too old, not found, no
    /// `kiln.toml`, another kiln chosen meanwhile). The line says why.
    Unusable(String),
}

impl Failure {
    /// The one line to show.
    pub fn line(&self) -> String {
        match self {
            Failure::NotFound(why) | Failure::Spawn(why) | Failure::Unusable(why) => why.clone(),
            Failure::Exit { line, .. } => line.clone(),
            Failure::TooLarge => format!(
                "kiln's answer was over {} MiB, so it was stopped and nothing of it was read",
                MAX_STDOUT / 1024 / 1024
            ),
            Failure::TimedOut => format!(
                "kiln gave no answer in {} s, so it was stopped",
                TIMEOUT_MS / 1000
            ),
        }
    }

    /// kiln's exit code, when it exited with one (2: usage or
    /// configuration, 3: network; CLI.md, "Errors and exit codes").
    pub fn code(&self) -> Option<i32> {
        match self {
            Failure::Exit { code, .. } => *code,
            _ => None,
        }
    }
}

/// A job that's done: its tag, its command line, and how it went.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished<T> {
    pub tag: T,
    pub line: String,
    pub result: Result<Ran, Failure>,
}

#[derive(Debug)]
struct Running<T> {
    process: u64,
    job: Job<T>,
    stdout: String,
    stderr: VecDeque<String>,
    timer: u64,
}

/// The queue. One per project; `T` is what the caller tags its jobs with.
#[derive(Debug)]
pub struct Kiln<T> {
    queue: VecDeque<Job<T>>,
    running: Option<Running<T>>,
    finished: VecDeque<Finished<T>>,
}

impl<T> Default for Kiln<T> {
    fn default() -> Self {
        Kiln {
            queue: VecDeque::new(),
            running: None,
            finished: VecDeque::new(),
        }
    }
}

/// The kiln program to run: `kiln.path` when it's set, else the one
/// `find_program` finds; else why there's none, saying how to give one.
pub fn program() -> Result<String, String> {
    if let Some(path) = api::setting("kiln.path")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    {
        return Ok(path);
    }
    api::find_program("kiln", &[]).ok_or_else(|| {
        "kiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is".to_string()
    })
}

impl<T> Kiln<T> {
    /// `job` queued, and started if nothing runs. A start that fails is
    /// finished at once ([`Kiln::take_finished`]).
    pub fn push(&mut self, job: Job<T>) {
        self.queue.push_back(job);
        self.start_next();
    }

    /// Whether anything runs or waits.
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// How many jobs wait, not counting the one running.
    pub fn waiting(&self) -> usize {
        self.queue.len()
    }

    /// The jobs that have finished since last asked, in the order they
    /// did.
    pub fn take_finished(&mut self) -> Option<Finished<T>> {
        self.finished.pop_front()
    }

    /// A line kiln printed (`Event::ProcessOutput`). Not its process: not
    /// ours, or one already stopped; ignored.
    pub fn output(&mut self, process: u64, stderr: bool, line: &str) {
        let Some(run) = self.running.as_mut().filter(|r| r.process == process) else {
            return;
        };
        if stderr {
            if run.stderr.len() >= MAX_STDERR_LINES {
                run.stderr.pop_front();
            }
            run.stderr.push_back(model::clip(line, 2000));
            return;
        }
        if run.stdout.len() + line.len() + 1 > MAX_STDOUT {
            api::process::kill(process);
            self.end(Err(Failure::TooLarge));
            return;
        }
        run.stdout.push_str(line);
        run.stdout.push('\n');
    }

    /// kiln ended (`Event::ProcessExit`).
    pub fn exited(&mut self, process: u64, code: Option<i32>) {
        let Some(run) = self.running.as_mut().filter(|r| r.process == process) else {
            return;
        };
        let ran = Ran {
            code,
            stdout: std::mem::take(&mut run.stdout),
            stderr: run.stderr.drain(..).collect(),
        };
        self.end(Ok(ran));
    }

    /// A timer went off: when it's the running job's, the job has taken
    /// too long and is stopped. Whether it was this queue's timer.
    pub fn timer(&mut self, timer: u64) -> bool {
        let Some(run) = self.running.as_ref().filter(|r| r.timer == timer) else {
            return false;
        };
        api::process::kill(run.process);
        self.end(Err(Failure::TimedOut));
        true
    }

    /// Everything waiting dropped, and what runs stopped and forgotten (its
    /// answer, if it comes, is ignored): another kiln was chosen. The tags
    /// of the jobs dropped, the running one first, so their callers can be
    /// answered.
    pub fn clear(&mut self) -> Vec<T> {
        let mut dropped = Vec::new();
        if let Some(run) = self.running.take() {
            api::host::cancel_timer(run.timer);
            api::process::kill(run.process);
            dropped.push(run.job.tag);
        }
        dropped.extend(self.queue.drain(..).map(|j| j.tag));
        dropped
    }

    fn end(&mut self, result: Result<Ran, Failure>) {
        if let Some(run) = self.running.take() {
            api::host::cancel_timer(run.timer);
            self.finished.push_back(Finished {
                line: run.job.line(),
                tag: run.job.tag,
                result,
            });
        }
        self.start_next();
    }

    fn start_next(&mut self) {
        while self.running.is_none() {
            let Some(job) = self.queue.pop_front() else {
                return;
            };
            let program = match program() {
                Ok(p) => p,
                Err(why) => {
                    self.finished.push_back(Finished {
                        line: job.line(),
                        tag: job.tag,
                        result: Err(Failure::NotFound(why)),
                    });
                    continue;
                }
            };
            let cmd = api::Command::new(&program).args(job.args.iter().cloned());
            match api::host::spawn(&cmd) {
                Ok(process) => {
                    if let Some(text) = &job.stdin {
                        // A refusal to take it shows as kiln's own error.
                        let _ = api::process::write(process, text.as_bytes());
                    }
                    api::process::close_stdin(process);
                    let timer = api::host::set_timer(TIMEOUT_MS, false);
                    self.running = Some(Running {
                        process,
                        job,
                        stdout: String::new(),
                        stderr: VecDeque::new(),
                        timer,
                    });
                }
                Err(why) => {
                    let failure = if api::setting("kiln.path").is_some_and(|p| !p.trim().is_empty())
                    {
                        Failure::NotFound(format!(
                            "kiln.path is {program}, which can't be started: {why}"
                        ))
                    } else {
                        Failure::Spawn(format!("{program} can't be started: {why}"))
                    };
                    self.finished.push_back(Finished {
                        line: job.line(),
                        tag: job.tag,
                        result: Err(failure),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::{Fake, Ran as Run};

    /// The fake's events for what was spawned, handed to the queue as
    /// lib.rs does.
    fn pump(fake: &Fake, k: &mut Kiln<&'static str>) {
        while let Some(e) = fake.next_event() {
            match e {
                api::Event::ProcessOutput(o) => k.output(o.process, o.stderr, &o.line),
                api::Event::ProcessExit((p, code)) => k.exited(p, code),
                _ => {}
            }
        }
    }

    fn done(k: &mut Kiln<&'static str>) -> Vec<(&'static str, Result<Ran, Failure>)> {
        std::iter::from_fn(|| k.take_finished())
            .map(|f| (f.tag, f.result))
            .collect()
    }

    #[test]
    fn one_runs_at_a_time_in_order() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln list", Run::running("[]"))
            .on_spawn("kiln status", Run::ok("{}"));
        let mut k = Kiln::default();
        k.push(Job::new(["list", "--format", "json"], "list"));
        k.push(Job::new(["status", "--format", "json"], "status"));
        assert_eq!(k.waiting(), 1);
        assert_eq!(fake.spawned().len(), 1);
        pump(&fake, &mut k);
        assert!(done(&mut k).is_empty(), "list still runs");
        let list = fake.spawned()[0].0;
        k.exited(list, Some(0));
        pump(&fake, &mut k);
        let d = done(&mut k);
        assert_eq!(
            d.iter()
                .map(|(t, r)| (*t, r.as_ref().map(|r| r.stdout.clone()).ok()))
                .collect::<Vec<_>>(),
            [
                ("list", Some("[]\n".into())),
                ("status", Some("{}\n".into()))
            ]
        );
        assert!(!k.busy());
        // No cwd: Fenix runs it in the project, at its top.
        assert!(fake
            .state()
            .processes
            .values()
            .all(|p| p.cmd.cwd.is_none() && p.cmd.program == "/bin/kiln" && p.stdin_closed));
    }

    #[test]
    fn output_in_several_chunks_is_gathered() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["status"], "s"));
        let p = fake.spawned()[0].0;
        for line in ["{", "  \"a\": 1", "}"] {
            k.output(p, false, line);
        }
        k.output(p, true, "kiln: 1 warning");
        k.output(p + 100, false, "not ours");
        k.exited(p, Some(0));
        let d = done(&mut k);
        let ran = d[0].1.as_ref().unwrap();
        assert_eq!(
            (ran.stdout.as_str(), ran.stderr.clone()),
            ("{\n  \"a\": 1\n}\n", vec!["kiln: 1 warning".to_string()])
        );
    }

    #[test]
    fn kilns_error_line_is_kept_as_written() {
        let ran = Ran {
            code: Some(2),
            stdout: String::new(),
            stderr: vec!["kiln: TASK-099 doesn't exist".into(), "".into()],
        };
        assert_eq!(
            ran.expect(&[0]),
            Err(Failure::Exit {
                code: Some(2),
                line: "kiln: TASK-099 doesn't exist".into()
            })
        );
        let check = Ran {
            code: Some(1),
            stdout: "[]\n".into(),
            stderr: vec!["kiln: 1 error".into()],
        };
        assert_eq!(check.expect(&[0, 1]), Ok("[]\n"));
        assert_eq!(
            Ran {
                code: Some(3),
                stdout: String::new(),
                stderr: vec![]
            }
            .error_line(),
            "kiln exited with code 3 and said nothing"
        );
        assert_eq!(
            Ran {
                code: None,
                stdout: String::new(),
                stderr: vec![]
            }
            .error_line(),
            "kiln was stopped before it exited"
        );
        assert_eq!(
            Ran {
                code: Some(101),
                stdout: String::new(),
                stderr: vec![
                    "thread 'main' panicked".into(),
                    "note: run with RUST_BACKTRACE".into()
                ]
            }
            .error_line(),
            "note: run with RUST_BACKTRACE"
        );
    }

    #[test]
    fn no_kiln_finishes_at_once_saying_how_to_give_one() {
        let fake = Fake::new();
        fake.install();
        let mut k = Kiln::default();
        k.push(Job::new(["--version"], "v"));
        assert_eq!(
            done(&mut k),
            [(
                "v",
                Err(Failure::NotFound(
                    "kiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is"
                        .into()
                ))
            )]
        );
        assert!(!k.busy());
    }

    #[test]
    fn a_kiln_path_that_cant_start_names_the_setting() {
        let fake = Fake::new();
        fake.install();
        fake.setting("kiln.path", "/opt/kiln/kiln")
            .program("kiln", "/bin/kiln")
            .on_spawn_error("/opt/kiln/kiln", "no such file");
        let mut k = Kiln::default();
        k.push(Job::new(["--version"], "v"));
        assert_eq!(
            done(&mut k),
            [(
                "v",
                Err(Failure::NotFound(
                    "kiln.path is /opt/kiln/kiln, which can't be started: no such file".into()
                ))
            )]
        );
    }

    #[test]
    fn a_spawn_failure_is_fenixs_words_and_the_next_job_still_runs() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn_error("kiln list", "not allowed")
            .on_spawn("kiln status", Run::ok("{}"));
        let mut k = Kiln::default();
        k.push(Job::new(["list"], "list"));
        k.push(Job::new(["status"], "status"));
        pump(&fake, &mut k);
        let d = done(&mut k);
        assert_eq!(
            d[0],
            (
                "list",
                Err(Failure::Spawn(
                    "/bin/kiln can't be started: not allowed".into()
                ))
            )
        );
        assert!(d[1].1.is_ok());
    }

    #[test]
    fn too_much_output_stops_it() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["list"], "list"));
        let p = fake.spawned()[0].0;
        let chunk = "x".repeat(1024 * 1024);
        for _ in 0..16 {
            k.output(p, false, &chunk);
        }
        let d = done(&mut k);
        assert_eq!(d, [("list", Err(Failure::TooLarge))]);
        assert!(fake.process("kiln list").unwrap().killed);
        assert_eq!(
            Failure::TooLarge.line(),
            "kiln's answer was over 16 MiB, so it was stopped and nothing of it was read"
        );
        // Its exit, when it comes, is no one's.
        k.exited(p, None);
        assert!(done(&mut k).is_empty());
    }

    #[test]
    fn a_run_that_hangs_is_stopped_on_its_timer() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln check", Run::running(""))
            .on_spawn("kiln list", Run::ok("[]"));
        let mut k = Kiln::default();
        k.push(Job::new(["check"], "check"));
        k.push(Job::new(["list"], "list"));
        let timer = fake
            .timers()
            .iter()
            .find(|t| t.millis == TIMEOUT_MS)
            .unwrap()
            .id;
        assert!(!k.timer(timer + 1000));
        assert!(k.timer(timer));
        pump(&fake, &mut k);
        let d = done(&mut k);
        assert_eq!(d[0], ("check", Err(Failure::TimedOut)));
        assert!(d[1].1.is_ok());
        assert!(fake.process("kiln check").unwrap().killed);
    }

    #[test]
    fn stdin_is_written_then_closed() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["new", "task", "--body-file", "-"], "new").stdin("Body\n"));
        let p = fake.process("kiln new").unwrap();
        assert_eq!(
            (p.stdin.as_slice(), p.stdin_closed),
            (b"Body\n".as_slice(), true)
        );
    }

    #[test]
    fn stderr_keeps_its_last_lines() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["check"], "c"));
        let p = fake.spawned()[0].0;
        for n in 0..(MAX_STDERR_LINES + 5) {
            k.output(p, true, &format!("line {n}"));
        }
        k.exited(p, Some(0));
        let ran = done(&mut k).remove(0).1.unwrap();
        assert_eq!(
            (ran.stderr.len(), ran.stderr[0].as_str()),
            (MAX_STDERR_LINES, "line 5")
        );
    }

    #[test]
    fn cleared_forgets_what_runs() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["list"], "a"));
        k.push(Job::new(["status"], "b"));
        let p = fake.spawned()[0].0;
        assert_eq!(k.clear(), ["a", "b"]);
        k.exited(p, Some(0));
        assert!(done(&mut k).is_empty() && !k.busy());
    }
}
