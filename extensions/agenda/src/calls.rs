//! Calls to other extensions' services that wait for their answer, as
//! `async`: what `fenix_extension_api::run` does, but over the API's
//! `call_service` and `Event::ServiceReply`, so the test kit's fake host
//! answers them in `cargo test` as Fenix does in the editor.
//!
//! A job is polled when it's spawned and each time an answer it may be
//! waiting for comes ([`deliver`]). A call that's had no answer in
//! [`LIMIT`] is given up as offline ([`expire`]); a late answer is then
//! dropped. One that waits on you (a form) has no limit.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use crate::api;

/// How long a read may go unanswered.
pub const LIMIT: Duration = Duration::from_secs(90);

/// How long a write may: it may make three requests of 30 s each.
pub const WRITE_LIMIT: Duration = Duration::from_secs(180);

type Slot = Rc<RefCell<Option<Result<String, String>>>>;
type Job = Pin<Box<dyn Future<Output = ()>>>;

struct Waiting {
    slot: Slot,
    name: String,
    deadline: Option<(Instant, Duration)>,
}

thread_local! {
    static WAITING: RefCell<HashMap<u64, Waiting>> = RefCell::new(HashMap::new());
    static JOBS: RefCell<Vec<Job>> = const { RefCell::new(Vec::new()) };
    static POLLING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A call's answer, once it's come.
pub struct Reply(Slot);

impl Future for Reply {
    type Output = Result<String, String>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.borrow_mut().take() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

fn ask(name: &str, payload: &str, limit: Option<Duration>) -> Reply {
    let id = api::call_service(name, payload);
    let slot: Slot = Rc::new(RefCell::new(None));
    WAITING.with(|w| w.borrow_mut().insert(id, Waiting { slot: slot.clone(), name: name.to_string(), deadline: limit.map(|l| (Instant::now() + l, l)) }));
    Reply(slot)
}

/// Service `name`, asked with `payload`; given up after [`LIMIT`].
pub fn call(name: &str, payload: &str) -> Reply {
    ask(name, payload, Some(LIMIT))
}

/// Service `name`, given up after `limit`.
pub fn call_for(name: &str, payload: &str, limit: Duration) -> Reply {
    ask(name, payload, Some(limit))
}

/// Service `name`, for as long as its answer takes: one that waits on
/// you.
pub fn call_unbounded(name: &str, payload: &str) -> Reply {
    ask(name, payload, None)
}

/// Runs `job`, as far as it can go now.
pub fn spawn(job: impl Future<Output = ()> + 'static) {
    JOBS.with(|j| j.borrow_mut().push(Box::pin(job)));
    poll();
}

/// Every job polled once, and those they spawn; the ones still waiting
/// kept for the next answer.
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

/// An event, unless it's the answer to one of its calls: then it's
/// taken, and the jobs waiting go on.
pub fn deliver(event: api::Event) -> Option<api::Event> {
    if let api::Event::ServiceReply((id, answer)) = &event {
        if let Some(waiting) = WAITING.with(|w| w.borrow_mut().remove(id)) {
            *waiting.slot.borrow_mut() = Some(answer.clone());
            poll();
            return None;
        }
    }
    Some(event)
}

/// Calls unanswered past their deadline at `now`, given up: `unknown`,
/// since what was asked may have been done.
pub fn expire(now: Instant) {
    let late: Vec<Waiting> = WAITING.with(|w| {
        let mut w = w.borrow_mut();
        let ids: Vec<u64> = w.iter().filter(|(_, c)| c.deadline.is_some_and(|(d, _)| d <= now)).map(|(id, _)| *id).collect();
        ids.into_iter().filter_map(|id| w.remove(&id)).collect()
    });
    if late.is_empty() {
        return;
    }
    for waiting in late {
        let secs = waiting.deadline.map_or(0, |(_, l)| l.as_secs());
        let why = fenix_tracker::SeamError::new(fenix_tracker::code::UNKNOWN, format!("no answer from {} in {secs} s", waiting.name));
        *waiting.slot.borrow_mut() = Some(Err(why.to_err()));
    }
    poll();
}

/// How many calls are waiting for an answer.
#[cfg(test)]
pub fn waiting() -> usize {
    WAITING.with(|w| w.borrow().len())
}
