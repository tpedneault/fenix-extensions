//! The agenda's data: tasks, their time, and their link to a Jira issue
//! -- no pages, no keys, no network. What was Fenix 1.x's `fenix-agenda`,
//! and the same `agenda.json`.

pub mod jira;
mod persist;
mod store;
mod sync;
mod task;

pub use jira::{Conflict, JiraLink, OpKind, PendingOp, RemoteComment, RemoteSnapshot, RemoteUpdate, SyncField};
pub use persist::{load, reload, save};
pub use store::{ActiveTimer, AgendaStore};
pub use sync::{parse_due, WorklogRow};
pub use task::{CodeRef, NoteEntry, Priority, Status, Subtask, Task, TaskId, TimeEntry, TimeSource};
