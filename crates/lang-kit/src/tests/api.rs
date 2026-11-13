//! A copy, by hand, of extension API 1.6's `tests` module: the types a
//! test provider's five exports and its one import take, as Fenix's
//! plan writes them (Fenix's `docs/plans/test-explorer.md`, §6.6 for the
//! Rust side and §6.3 for the WIT, with the four changes of its §18:
//! `Found.data`, `Plan.list_after`, `Report::Crashed { id, message }`).
//!
//! API 1.6 isn't released, and lang-kit builds on 1.5 (tag `v1.5.0`),
//! so this stands in for it. On the branch `api-1.6` (piece E1b of
//! `plans/test-explorer.md`) the whole file becomes
//!
//! ```rust,ignore
//! pub use fenix_extension_api::tests::*;
//! ```
//!
//! and nothing else changes. Any difference from the types Fenix merges
//! is a bug here: the swap is meant to fail to compile wherever they
//! differ. So nothing is in this file that isn't in the API: lang-kit's
//! own helpers are in the modules beside it. The derives are a guess
//! (§6.6 doesn't write them): those the 1.5 crate gives its own records,
//! `Debug`, `Clone` and `PartialEq` (`additional_derives: [PartialEq]`),
//! and `Copy` and `Eq` for the two plain enums. lang-kit's code uses only
//! `Debug`, `Clone` and `PartialEq` of them.
//!
//! Lines are from 0 here (`test-location`, `found-test.line`), as a
//! problem's are.

use fenix_extension_api::{Command, LaunchConfig};

/// `found-test`: a test, or a group of them, from a file's text
/// (`tests-scan`), its runner's listing, or a run (`Report::Found`).
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    pub parent: Option<String>,
    pub label: String,
    pub kind: Kind,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub skip: Option<String>,
    pub template: bool,
    pub data: Option<String>,
}

/// `test-kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Group,
    File,
    Suite,
    Test,
    Case,
}

/// `test-ask`.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Discover,
    Run(Vec<Found>),
    RunAll,
}

/// `test-plan`.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub list_after: bool,
}

/// `test-step`.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub command: Command,
    pub label: String,
    pub covers: Vec<String>,
    pub needs: Vec<String>,
}

/// `test-line`.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub stderr: bool,
    pub cut: bool,
}

/// `test-step-end`.
#[derive(Debug, Clone, PartialEq)]
pub enum StepEnd {
    Exited(Option<i32>),
    Stopped,
    TimedOut,
    Failed(String),
}

/// `test-outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    Skipped,
    Errored,
}

/// `test-location`.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub path: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// `test-ended`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    pub id: String,
    pub outcome: Outcome,
    pub duration_ms: Option<f64>,
    pub message: Option<String>,
    pub at: Option<Location>,
    pub text: Option<String>,
}

/// `test-report`.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    Found(Found),
    Started(String),
    Ended(Ended),
    Output { id: Option<String>, text: String },
    Broken { id: Option<String>, message: String, at: Option<Location> },
    Crashed { id: Option<String>, message: String },
}

/// `test-launch`.
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    Launch(LaunchConfig),
    BuildFirst(Plan),
}

/// `test-provider-set`: `runner` "" and no `problem` is "nothing to run in
/// this project".
///
/// The copy's body: API 1.5 has no such import, so this asks nothing of
/// Fenix and says it worked. The real one calls the host.
pub fn provider(provider: &str, runner: &str, problem: Option<&str>) -> Result<(), String> {
    let _ = (provider, runner, problem);
    Ok(())
}
