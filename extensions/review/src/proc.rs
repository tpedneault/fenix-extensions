//! Programs it runs (`git`, `gh`), awaited: what one printed and whether
//! it succeeded. Their output comes as events, line by line; a job waits
//! on short timers until the program's exit has come.

use std::cell::RefCell;
use std::collections::HashMap;

use fenix_extension_api::{run, Command, Event};

/// A program's run.
#[derive(Debug, Default)]
struct Running {
    out: Vec<String>,
    err: Vec<String>,
    exit: Option<Option<i32>>,
}

/// What a program did: its output, and whether it succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

thread_local! {
    static RUNNING: RefCell<HashMap<u64, Running>> = RefCell::new(HashMap::new());
}

/// An event, unless it's about a program run here: then it's kept for
/// the job waiting on it, and `None` comes back.
pub fn deliver(event: Event) -> Option<Event> {
    let id = match &event {
        Event::ProcessOutput(o) => o.process,
        Event::ProcessExit((id, _)) => *id,
        _ => return Some(event),
    };
    RUNNING.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.get_mut(&id) else { return Some(event) };
        match event {
            Event::ProcessOutput(o) if o.stderr => run.err.push(o.line),
            Event::ProcessOutput(o) => run.out.push(o.line),
            Event::ProcessExit((_, code)) => run.exit = Some(code),
            _ => {}
        }
        None
    })
}

/// Runs `cmd` to its end.
pub async fn output(cmd: Command) -> Result<Output, String> {
    let id = cmd.spawn()?;
    RUNNING.with(|r| r.borrow_mut().insert(id, Running::default()));
    loop {
        let done = RUNNING.with(|r| {
            let mut r = r.borrow_mut();
            if r.get(&id).is_some_and(|run| run.exit.is_some()) {
                r.remove(&id)
            } else {
                None
            }
        });
        if let Some(run) = done {
            return Ok(Output { ok: run.exit == Some(Some(0)), stdout: run.out.join("\n"), stderr: run.err.join("\n") });
        }
        run::sleep(15).await;
    }
}

/// `git args` in `cwd`, as Fenix runs it: never waiting on an editor or
/// a credential prompt -- a fetch it can't make fails, and says why.
fn git_command(cwd: &str, args: &[&str]) -> Command {
    Command::new("git")
        .args(args.iter().copied())
        .cwd(cwd)
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
}

/// `git args` in `cwd`: what it printed, or why it failed.
pub async fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let out = output(git_command(cwd, args)).await?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(out.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed").trim().to_string())
    }
}

/// Whether `git args` succeeds in `cwd`.
pub async fn git_ok(cwd: &str, args: &[&str]) -> bool {
    output(git_command(cwd, args)).await.is_ok_and(|o| o.ok)
}
