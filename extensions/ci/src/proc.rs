//! Programs it runs (`git` for a worktree's remote, `gh auth token`),
//! awaited: what one printed and whether it succeeded. Their output comes
//! as events, line by line; a job waits on short timers until the
//! program's exit has come. As the Review extension does.

use std::cell::RefCell;
use std::collections::HashMap;

use fenix_extension_api::{run, Command, Event};

#[derive(Debug, Default)]
struct Running {
    out: Vec<String>,
    exit: Option<Option<i32>>,
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
            Event::ProcessOutput(o) if !o.stderr => run.out.push(o.line),
            Event::ProcessExit((_, code)) => run.exit = Some(code),
            _ => {}
        }
        None
    })
}

/// What `cmd` printed, when it succeeded.
pub async fn stdout(cmd: Command) -> Option<String> {
    let id = cmd.spawn().ok()?;
    RUNNING.with(|r| r.borrow_mut().insert(id, Running::default()));
    for _ in 0..1000 {
        let done = RUNNING.with(|r| {
            let mut r = r.borrow_mut();
            if r.get(&id).is_some_and(|run| run.exit.is_some()) {
                r.remove(&id)
            } else {
                None
            }
        });
        if let Some(run) = done {
            return (run.exit == Some(Some(0))).then(|| run.out.join("\n"));
        }
        run::sleep(15).await;
    }
    RUNNING.with(|r| r.borrow_mut().remove(&id));
    None
}
