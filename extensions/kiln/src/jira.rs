//! The Jira mirror on the plan's item rows: `kiln list --jira --format
//! json`, read on the side, and what it says of each linked ticket --
//! `FLINT-12 In Review · Alice`, `FLINT-12 Done`, or `FLINT-12 ?` when
//! Jira doesn't know the key (Kiln's GUI, "Pages"; GUI.md).
//!
//! # Why a read of its own
//!
//! The project's reload is `list`, `status` and `check` (`project.rs`),
//! all local and fast. `list --jira` is not: kiln asks Jira unless its own
//! cache (`jira/<host>.json`, ten minutes, `kiln_jira::FRESH_SECS`) is
//! fresh and covers every key, and Jira may be slow, down, behind a VPN or
//! not signed in to. Putting it in the reload would make every reload, and
//! with it every page, as slow as Jira and fail with it. So it is a read
//! apart, and the rest of this file is the rules that keep it apart:
//!
//! - it is asked when a plan page is in view, the project is read, `[jira]
//!   url` is in `kiln.toml` and nothing else runs or waits
//!   ([`crate::project::Project::quiet`]) -- once after the first read,
//!   then again no sooner than [`FRESH_MS`] after the last answer (a
//!   one-shot timer, not a poll: nothing happens while no one looks), and
//!   at once on `r`, `Kiln: read the project again`, a changed `[jira]
//!   url` and after a Jira sync ([`Mirror::refresh`]);
//! - it is a background job ([`crate::kiln::Job::background`]): any other
//!   job -- a reload, an edit, a dry run -- stops it or takes its place
//!   in the queue, and it is asked again when the queue is quiet
//!   ([`Mirror::answered`], `Failure::Yielded`). A slow Jira costs the
//!   tickets arriving late, never a reload waiting. The timer reload that
//!   runs while a page is in view is skipped while it runs, so it can't
//!   stop it every few seconds;
//! - it has a limit, [`MIRROR_MS`]; past it kiln is stopped and the
//!   failure said;
//! - a failure is said once on the bottom line (kiln's line, and for a
//!   missing token how to sign in), not on every draw, and the rows
//!   simply have no tickets. A later failure after a good read keeps the
//!   last tickets (old news beats none, and they are replaced when Jira
//!   answers again) and says why once. A failed read is tried again after
//!   [`FRESH_MS`], like a good one: a Jira that's down isn't asked on
//!   every reload.
//!
//! The tickets are looked up by key, from each item's own `jira_keys`
//! (`list`, always current), so an item that gained a key since the mirror
//! was read shows no ticket until the next read rather than a wrong one.
//!
//! What it deliberately doesn't do: run kiln or touch the queue (`lib.rs`
//! asks, and hands the answer here), draw (a ticket is [`Cell`]s the frame
//! appends to item rows), or read a token: kiln does, from its own
//! credential store.

use std::collections::HashMap;

use serde::Deserialize;

use crate::api;
use crate::kiln::{Failure, Ran};
use crate::model::{self, Item};
use crate::pages::exports::Failed;
use crate::pages::view::{Cell, Look};
use crate::project::QueryId;

/// How long a read of the mirror is left alone before another: 10
/// minutes, kiln's own cache's (`kiln_jira::FRESH_SECS`, 600 s). Asked
/// sooner, kiln would answer from that cache anyway; asked on a timer
/// shorter, it would only spend a process to be told so.
pub const FRESH_MS: u32 = 10 * 60 * 1000;

/// How long `kiln list --jira` may take: 60 s, the queue's own default.
/// It asks Jira in chunks of 100 keys; a project of thousands of keys
/// against a slow server may need many requests. Stopping it is harmless
/// (read-only; kiln caches only what finished), and being a background job
/// nothing waits for it.
pub const MIRROR_MS: u32 = 60 * 1000;

/// `kiln list --jira --format json`: every item with its tickets.
pub fn args() -> Vec<String> {
    ["list", "--jira", "--format", "json"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// `kiln auth jira`, in a terminal.
pub fn auth_args() -> Vec<String> {
    vec!["auth".into(), "jira".into()]
}

/// Whether kiln's line is about a missing Jira token: what `kiln auth
/// jira` gives. Both ways kiln says so name `KILN_JIRA_TOKEN` -- "no Jira
/// token for …: set KILN_JIRA_TOKEN or run `kiln auth jira`" when the
/// credential store has none, and the store's own error "… (or set
/// KILN_JIRA_TOKEN)" when there is no store (crates/kiln/src/cmd/jira.rs:
/// 60-65 and 143-150, kiln-secrets). Not by exit code: the mirror's every
/// failure exits 3 where sync's and pull's token errors exit 2.
pub fn wants_token(line: &str) -> bool {
    line.contains("KILN_JIRA_TOKEN") || line.contains("kiln auth jira")
}

// -- What kiln says -------------------------------------------------------------

/// One entry of an item's `jira` array: a ticket Jira knows, or a key it
/// doesn't (`missing`). Every field optional so a newer kiln's extras (or
/// less) don't stop the rows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Entry {
    key: String,
    status: String,
    resolved: bool,
    assignee: Option<String>,
    missing: bool,
}

/// An item of `list --jira`, as far as the mirror reads it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Listed {
    jira: Vec<Entry>,
}

/// What the mirror knows of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known {
    Ticket {
        status: String,
        /// In Jira's Done category (kiln's `resolved`).
        resolved: bool,
        assignee: Option<String>,
    },
    /// Jira doesn't know the key (or the token can't see it).
    Missing,
}

/// The tickets of the keys the project links, by key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tickets {
    by_key: HashMap<String, Known>,
}

impl Tickets {
    /// `kiln list --jira --format json`'s stdout. Every key in it, once;
    /// a key that comes twice is the same ticket.
    pub fn parse(stdout: &str) -> Result<Tickets, String> {
        let items = model::array::<Listed>("kiln list --jira", stdout)?;
        let mut by_key = HashMap::new();
        for e in items.into_iter().flat_map(|i| i.jira) {
            let key = e.key.trim().to_string();
            if key.is_empty() {
                continue;
            }
            let known = if e.missing {
                Known::Missing
            } else {
                Known::Ticket {
                    status: e.status,
                    resolved: e.resolved,
                    assignee: e.assignee.filter(|a| !a.trim().is_empty()),
                }
            };
            by_key.insert(key, known);
        }
        Ok(Tickets { by_key })
    }

    /// Tickets known, for a message.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// The cell for `key`: `FLINT-12 In Review · Alice` in the good look
    /// when resolved, muted otherwise; `FLINT-12 ?` for a key Jira doesn't
    /// know. None when the mirror hasn't read this key (the item gained it
    /// since). Status and assignee are Jira's, other people's writing:
    /// plain text on one line ([`model::plain`]).
    pub fn cell(&self, key: &str) -> Option<Cell> {
        let key = key.trim();
        let shown = model::plain(key, 40);
        Some(match self.by_key.get(key)? {
            Known::Missing => Cell::muted(format!("{shown} ?")),
            Known::Ticket {
                status,
                resolved,
                assignee,
            } => {
                let status = model::plain(status, 40);
                let text = match assignee.as_deref().map(|a| model::plain(a, 40)) {
                    Some(a) if !a.is_empty() => format!("{shown} {status} · {a}"),
                    _ => format!("{shown} {status}"),
                };
                Cell::new(text, if *resolved { Look::Good } else { Look::Muted })
            }
        })
    }

    /// One cell for each of `item`'s tickets the mirror has read.
    pub fn cells(&self, item: &Item) -> Vec<Cell> {
        item.jira_keys.iter().filter_map(|k| self.cell(k)).collect()
    }
}

// -- When it is read ------------------------------------------------------------

/// What a read of the mirror came to, for `lib.rs` to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Nothing to show or say: stopped for another job (asked again when
    /// the queue is quiet), or an answer from before.
    Nothing,
    /// New tickets: the rows are built again.
    Tickets,
    /// It failed, and this is new: kiln's line to say once.
    Failed { line: String, token: bool },
}

/// The mirror, and when to read it.
#[derive(Debug)]
pub struct Mirror {
    tickets: Option<Tickets>,
    /// Wanted: asked at the next quiet moment.
    due: bool,
    /// The read out.
    asked: Option<QueryId>,
    /// The one-shot timer until the next read is allowed.
    timer: Option<u64>,
    /// Why the last read failed, while it's the latest word; and what was
    /// last said of it, so it's said once.
    failed: Option<Failed>,
    said: Option<String>,
}

impl Default for Mirror {
    fn default() -> Mirror {
        Mirror {
            tickets: None,
            // Once after the first read of the project.
            due: true,
            asked: None,
            timer: None,
            failed: None,
            said: None,
        }
    }
}

impl Mirror {
    /// The tickets read, if any have been.
    pub fn tickets(&self) -> Option<&Tickets> {
        self.tickets.as_ref()
    }

    /// Whether the last read failed for want of a token (`A` signs in).
    pub fn needs_token(&self) -> bool {
        self.failed.as_ref().is_some_and(|f| wants_token(&f.line))
    }

    /// Whether to ask now: wanted, none out, and the caller's conditions
    /// (`[jira] url`, a page in view, the project read, a quiet queue).
    pub fn wants(&self, ready: bool) -> bool {
        ready && self.due && self.asked.is_none()
    }

    /// Asked as query `id`.
    pub fn asked(&mut self, id: QueryId) {
        self.asked = Some(id);
        self.due = false;
    }

    /// Read again at the next quiet moment, whatever the timer says: `r`,
    /// a changed `[jira] url`, a sync that wrote keys. One out is left to
    /// finish (and asked again after if it was asked before this).
    pub fn refresh(&mut self) {
        self.due = true;
        self.stop_timer();
    }

    /// No Jira here any more (`[jira] url` gone): the tickets, the failure
    /// and the timer forgotten. Whether anything shown changed.
    pub fn clear(&mut self) -> bool {
        let had = self.tickets.take().is_some();
        // A read out was of the other Jira: its answer is nobody's.
        self.asked = None;
        self.failed = None;
        self.said = None;
        self.stop_timer();
        self.due = true;
        had
    }

    /// A timer went off: when it's this mirror's, the next read is due.
    pub fn timer(&mut self, timer: u64) -> bool {
        if self.timer != Some(timer) {
            return false;
        }
        self.timer = None;
        self.due = true;
        true
    }

    fn stop_timer(&mut self) {
        if let Some(t) = self.timer.take() {
            api::host::cancel_timer(t);
        }
    }

    /// The answer to query `id`, if it's the one out; else nothing.
    pub fn answered(&mut self, id: QueryId, result: &Result<Ran, Failure>) -> Heard {
        if self.asked != Some(id) {
            return Heard::Nothing;
        }
        self.asked = None;
        let failed = match result {
            // Stopped for another job, or kiln gone from under it: not
            // Jira's doing, asked again when it's quiet.
            Err(Failure::Yielded | Failure::Unusable(_)) => {
                self.due = true;
                return Heard::Nothing;
            }
            Ok(ran) if ran.code == Some(0) => match Tickets::parse(&ran.stdout) {
                Ok(t) => {
                    self.failed = None;
                    self.said = None;
                    self.tickets = Some(t);
                    self.start_timer();
                    return Heard::Tickets;
                }
                Err(line) => Failed {
                    line,
                    code: ran.code,
                },
            },
            Ok(ran) => Failed {
                line: ran.error_line(),
                code: ran.code,
            },
            Err(f) => Failed::of(f),
        };
        // Tried again in ten minutes, not on the next reload.
        self.start_timer();
        let token = wants_token(&failed.line);
        let new = self.said.as_deref() != Some(failed.line.as_str());
        let line = failed.line.clone();
        self.failed = Some(failed);
        if new {
            self.said = Some(line.clone());
            Heard::Failed { line, token }
        } else {
            Heard::Nothing
        }
    }

    fn start_timer(&mut self) {
        self.stop_timer();
        self.timer = Some(api::host::set_timer(FRESH_MS, false));
    }
}

#[cfg(test)]
mod tests;
