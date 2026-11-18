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
//! - **Bounded**: stdout past [`MAX_STDOUT`] bytes, or a run past its
//!   limit ([`TIMEOUT_MS`] unless the job gives its own, [`Job::limit`]),
//!   stops the process and fails the job saying which; stderr keeps its
//!   last [`MAX_STDERR_LINES`] lines.
//! - **Long jobs.** A job may give a longer limit: the Exports tab's dry
//!   run (it renders the document and may read Confluence) and its publish
//!   (minutes against a large space). A job that must not be stopped half
//!   done -- a publish -- is also kept running when another kiln is chosen
//!   ([`Job::keep_running`], [`Kiln::clear`]): its answer still comes, and
//!   the next job starts after it, with the new kiln. While a long job
//!   runs, everything else waits behind it: one kiln at a time is the
//!   rule, and the job's own limit bounds how long.
//!
//! - **Background jobs yield.** A job may say it is [`Job::background`]: the
//!   Jira mirror (`kiln list --jira`), which may wait on a slow or absent
//!   Jira and which nothing on screen needs at once. It runs when nothing
//!   else does, and the moment any other job is pushed -- a reload, an
//!   edit, a dry run -- it is stopped if it runs and dropped if it waits,
//!   answered with [`Failure::Yielded`], so the caller can ask it again
//!   when the queue is quiet. That is how a slow Jira never holds up the
//!   project's reload: the cost is a read that is stopped and asked again
//!   (kiln caches what it fetched, ten minutes, only when it finishes).
//!   It is read-only, so stopping it leaves nothing half done.
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

/// How long one kiln run may take unless its job says otherwise: 60
/// seconds. A reading command takes well under one; `check` with K207 on
/// reads Jira (kiln caches it ten minutes). A run past this is stopped, so
/// the queue never waits forever behind a kiln that hangs (a network
/// share, a prompt nobody sees).
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
    /// How long it may run before it's stopped: [`TIMEOUT_MS`] unless
    /// [`Job::limit`] says otherwise.
    pub limit_ms: u32,
    /// Not stopped by [`Kiln::clear`]: a publish, which stopped half done
    /// leaves Confluence half updated until the next one.
    pub keep_running: bool,
    /// Yields to every job that isn't one ([`Job::background`]).
    pub background: bool,
}

impl<T> Job<T> {
    pub fn new<S: Into<String>>(args: impl IntoIterator<Item = S>, tag: T) -> Job<T> {
        Job {
            args: args.into_iter().map(Into::into).collect(),
            stdin: None,
            tag,
            limit_ms: TIMEOUT_MS,
            keep_running: false,
            background: false,
        }
    }

    pub fn stdin(mut self, text: impl Into<String>) -> Job<T> {
        self.stdin = Some(text.into());
        self
    }

    /// Stopped after `ms` instead of [`TIMEOUT_MS`] (never less than a
    /// second).
    pub fn limit(mut self, ms: u32) -> Job<T> {
        self.limit_ms = ms.max(1_000);
        self
    }

    /// Kept running when another kiln is chosen ([`Kiln::clear`]).
    pub fn keep_running(mut self) -> Job<T> {
        self.keep_running = true;
        self
    }

    /// Read-only and not urgent: stopped, or dropped if it waits, when any
    /// job that isn't one is pushed ([`Failure::Yielded`]).
    pub fn background(mut self) -> Job<T> {
        self.background = true;
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
    /// It ran past its limit, this many seconds ([`TIMEOUT_MS`] unless
    /// its job gave another); it was stopped.
    TimedOut(u32),
    /// Not run: kiln can't be used here now (too old, not found, no
    /// `kiln.toml`, another kiln chosen meanwhile). The line says why.
    Unusable(String),
    /// A [`Job::background`] job stopped (or dropped before it ran) so that
    /// another job could go first. Not a failure of kiln or of Jira:
    /// whoever asked asks again when the queue is quiet.
    Yielded,
}

impl Failure {
    /// The one line to show.
    pub fn line(&self) -> String {
        match self {
            Failure::NotFound(why) | Failure::Spawn(why) | Failure::Unusable(why) => why.clone(),
            Failure::Yielded => "stopped so that something else could run first".into(),
            Failure::Exit { line, .. } => line.clone(),
            Failure::TooLarge => format!(
                "kiln's answer was over {} MiB, so it was stopped and nothing of it was read",
                MAX_STDOUT / 1024 / 1024
            ),
            Failure::TimedOut(s) if *s >= 120 && s % 60 == 0 => format!(
                "kiln gave no answer in {} minutes, so it was stopped",
                s / 60
            ),
            Failure::TimedOut(s) => format!("kiln gave no answer in {s} s, so it was stopped"),
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
        if !job.background {
            self.yield_background();
        }
        self.queue.push_back(job);
        self.start_next();
    }

    /// Whether the job that runs is a [`Job::background`] one.
    pub fn background_running(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.job.background)
    }

    /// The background job that runs stopped, and those that wait dropped,
    /// each finished with [`Failure::Yielded`], in the order they were
    /// queued.
    fn yield_background(&mut self) {
        if self.background_running() {
            if let Some(run) = self.running.take() {
                api::host::cancel_timer(run.timer);
                api::process::kill(run.process);
                self.finished.push_back(Finished {
                    line: run.job.line(),
                    tag: run.job.tag,
                    result: Err(Failure::Yielded),
                });
            }
        }
        let (background, rest): (VecDeque<Job<T>>, VecDeque<Job<T>>) =
            self.queue.drain(..).partition(|j| j.background);
        self.queue = rest;
        for job in background {
            self.finished.push_back(Finished {
                line: job.line(),
                tag: job.tag,
                result: Err(Failure::Yielded),
            });
        }
    }

    /// Whether anything runs or waits.
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// Whether a job running or waiting has a tag `is` says yes to (an
    /// edit not yet done, say).
    pub fn any(&self, is: impl Fn(&T) -> bool) -> bool {
        self.running.iter().any(|r| is(&r.job.tag)) || self.queue.iter().any(|j| is(&j.tag))
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
        let seconds = run.job.limit_ms / 1000;
        self.end(Err(Failure::TimedOut(seconds)));
        true
    }

    /// Everything waiting dropped, and what runs stopped and forgotten (its
    /// answer, if it comes, is ignored): another kiln was chosen. The tags
    /// of the jobs dropped, the running one first, so their callers can be
    /// answered. A running job that [`Job::keep_running`] isn't stopped: it
    /// goes on, is answered as usual, and what's queued after it runs
    /// with the kiln chosen now.
    pub fn clear(&mut self) -> Vec<T> {
        let mut dropped = Vec::new();
        if self.running.as_ref().is_some_and(|r| !r.job.keep_running) {
            if let Some(run) = self.running.take() {
                api::host::cancel_timer(run.timer);
                api::process::kill(run.process);
                dropped.push(run.job.tag);
            }
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
                    let timer = api::host::set_timer(job.limit_ms, false);
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
        assert_eq!(d[0], ("check", Err(Failure::TimedOut(60))));
        assert!(d[1].1.is_ok());
        assert!(fake.process("kiln check").unwrap().killed);
    }

    #[test]
    fn a_job_with_its_own_limit_is_stopped_on_it_and_says_how_long() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["export"], "long").limit(3_600_000));
        let timer = fake
            .timers()
            .iter()
            .find(|t| t.millis == 3_600_000)
            .unwrap()
            .id;
        assert!(k.timer(timer));
        let d = done(&mut k);
        assert_eq!(d, [("long", Err(Failure::TimedOut(3600)))]);
        assert_eq!(
            Failure::TimedOut(3600).line(),
            "kiln gave no answer in 60 minutes, so it was stopped"
        );
        assert_eq!(
            Failure::TimedOut(60).line(),
            "kiln gave no answer in 60 s, so it was stopped"
        );
        assert_eq!(
            Job::new(["x"], ()).limit(5).limit_ms,
            1_000,
            "a second at least"
        );
    }

    #[test]
    fn a_job_that_keeps_running_isnt_stopped_by_another_kiln() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        k.push(Job::new(["export", "--yes"], "publish").keep_running());
        k.push(Job::new(["list"], "list"));
        let p = fake.spawned()[0].0;
        assert_eq!(k.clear(), ["list"], "only what waited");
        assert!(!fake.process("kiln export").unwrap().killed);
        assert!(k.busy(), "it still runs");
        k.exited(p, Some(0));
        let d = done(&mut k);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, "publish", "and is answered");
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

    #[test]
    fn a_background_job_yields_to_every_other_and_runs_when_nothing_does() {
        let fake = Fake::new();
        fake.install();
        fake.program("kiln", "/bin/kiln")
            .on_spawn("kiln", Run::running(""));
        let mut k = Kiln::default();
        // Alone, it runs at once; a second background job waits behind it.
        k.push(Job::new(["list", "--jira"], "mirror").background());
        k.push(Job::new(["list", "--jira", "--again"], "mirror2").background());
        assert!(k.background_running());
        assert_eq!(fake.spawned().len(), 1);
        let mirror = fake.spawned()[0].0;
        // Anything else stops the one that runs and drops the one that
        // waits, answered as yielded, before it starts itself.
        k.push(Job::new(["list"], "reload"));
        assert!(
            fake.state().processes[&mirror].killed,
            "stopped, not waited for"
        );
        assert_eq!(
            done(&mut k),
            [
                ("mirror", Err(Failure::Yielded)),
                ("mirror2", Err(Failure::Yielded))
            ]
        );
        assert!(!k.background_running() && k.waiting() == 0);
        assert_eq!(fake.spawned().len(), 2, "the reload runs at once");
        // Its late exit is nobody's.
        k.exited(mirror, Some(0));
        assert!(done(&mut k).is_empty());
        // A background job pushed behind a foreground one waits its turn,
        // and a foreground job after it takes its place.
        k.push(Job::new(["list", "--jira"], "mirror").background());
        assert_eq!(k.waiting(), 1);
        k.push(Job::new(["status"], "status"));
        assert_eq!(done(&mut k), [("mirror", Err(Failure::Yielded))]);
        assert_eq!(k.waiting(), 1, "only the status waits");
    }
}
