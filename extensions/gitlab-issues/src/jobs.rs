//! Work that waits -- an HTTP request, a program -- written as `async`:
//! what `fenix_extension_api::run` does, but over the API's `host::http`
//! and `host::spawn` and their events, so the test kit's fake host
//! answers them in `cargo test` as Fenix does in the editor.
//!
//! A job is polled when it's spawned and each time an answer it may be
//! waiting for comes ([`deliver`]). Nothing here retries: a request that
//! fails says so, once.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::api::{self, Command, Event, HttpRequest, HttpResponse};

type Job = Pin<Box<dyn Future<Output = ()>>>;
type Slot<T> = Rc<RefCell<Option<T>>>;

/// What a program did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Default)]
struct Running {
    out: Vec<String>,
    err: Vec<String>,
    slot: Slot<Result<Output, String>>,
}

thread_local! {
    static HTTP: RefCell<HashMap<u64, Slot<Result<HttpResponse, String>>>> = RefCell::new(HashMap::new());
    static PROCS: RefCell<HashMap<u64, Running>> = RefCell::new(HashMap::new());
    static JOBS: RefCell<Vec<Job>> = const { RefCell::new(Vec::new()) };
    static POLLING: Cell<bool> = const { Cell::new(false) };
}

/// An answer, once it's come.
pub struct Wait<T>(Slot<T>);

impl<T> Future for Wait<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<T> {
        match self.0.borrow_mut().take() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

/// `req`, made by Fenix's HTTP client.
pub fn fetch(req: HttpRequest) -> Wait<Result<HttpResponse, String>> {
    let slot: Slot<Result<HttpResponse, String>> = Rc::new(RefCell::new(None));
    match api::host::http(&req) {
        Ok(id) => {
            HTTP.with(|h| h.borrow_mut().insert(id, slot.clone()));
        }
        Err(e) => *slot.borrow_mut() = Some(Err(e)),
    }
    Wait(slot)
}

/// `cmd`, run to its end.
pub fn run(cmd: Command) -> Wait<Result<Output, String>> {
    let slot: Slot<Result<Output, String>> = Rc::new(RefCell::new(None));
    match api::host::spawn(&cmd) {
        Ok(id) => {
            PROCS.with(|p| p.borrow_mut().insert(id, Running { slot: slot.clone(), ..Default::default() }));
        }
        Err(e) => *slot.borrow_mut() = Some(Err(e)),
    }
    Wait(slot)
}

/// Runs `job` as far as it can go now.
pub fn spawn(job: impl Future<Output = ()> + 'static) {
    JOBS.with(|j| j.borrow_mut().push(Box::pin(job)));
    poll();
}

fn poll() {
    if POLLING.with(|p| p.replace(true)) {
        return;
    }
    let mut cx = Context::from_waker(Waker::noop());
    let mut todo: VecDeque<Job> = JOBS.with(|j| std::mem::take(&mut *j.borrow_mut())).into();
    let mut parked = Vec::new();
    while let Some(mut job) = todo.pop_front() {
        if job.as_mut().poll(&mut cx).is_pending() {
            parked.push(job);
        }
        todo.extend(JOBS.with(|j| std::mem::take(&mut *j.borrow_mut())));
    }
    JOBS.with(|j| j.borrow_mut().extend(parked));
    POLLING.with(|p| p.set(false));
}

/// An event, unless it's an answer one of its jobs waits for: then it's
/// taken, and the jobs go on.
pub fn deliver(event: Event) -> Option<Event> {
    match event {
        Event::HttpResponse((id, answer)) => {
            let Some(slot) = HTTP.with(|h| h.borrow_mut().remove(&id)) else { return Some(Event::HttpResponse((id, answer))) };
            *slot.borrow_mut() = Some(answer);
        }
        Event::ProcessOutput(o) => {
            let mine = PROCS.with(|p| match p.borrow_mut().get_mut(&o.process) {
                Some(run) => {
                    if o.stderr { run.err.push(o.line.clone()) } else { run.out.push(o.line.clone()) }
                    true
                }
                None => false,
            });
            return if mine { None } else { Some(Event::ProcessOutput(o)) };
        }
        Event::ProcessExit((id, code)) => {
            let Some(run) = PROCS.with(|p| p.borrow_mut().remove(&id)) else { return Some(Event::ProcessExit((id, code))) };
            *run.slot.borrow_mut() = Some(Ok(Output { ok: code == Some(0), stdout: run.out.join("\n"), stderr: run.err.join("\n") }));
        }
        other => return Some(other),
    }
    poll();
    None
}
