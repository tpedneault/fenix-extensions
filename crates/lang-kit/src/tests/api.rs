//! A copy, by hand, of extension API 1.6's `tests` module: the types a
//! test provider's five exports and its one import take, as Fenix has
//! them (`crates/fenix-extension-api/src/tests.rs` on the branch that
//! holds Fenix's review of steps 1 and 2, its plan's §19).
//!
//! API 1.6 isn't released, and lang-kit builds on 1.5 (tag `v1.5.0`),
//! so this stands in for it. On the branch `api-1.6` (piece E1b of
//! `plans/test-explorer.md`) the whole file becomes
//!
//! ```rust,ignore
//! pub use fenix_extension_api::tests::*;
//! ```
//!
//! and nothing else changes. It is the real module's shape exactly:
//! every type, field and case, in the same order, of the same types, with
//! the same derives (`Debug`, `Clone`, `PartialEq` on all; `Eq` where
//! there's no `f64` or `Command`; `Default` on `Found`, `Kind` (`Test`),
//! `Plan`, `Line` and `Location`; `Copy` and `Hash` on `Kind` and
//! `Outcome`), so the swap compiles wherever this does. What the real
//! module has beyond the types -- `From` between them and the WIT's
//! bindings, the hidden `exports` -- is the API's own business, and
//! lang-kit uses none of it. lang-kit's helpers are in the modules beside
//! this one.
//!
//! Lines are from 0 here (`test-location`, `found-test.line`), as a
//! problem's are. The doc comments are the real module's.

use fenix_extension_api::{Command, LaunchConfig};

/// A test, or a group of them, as a provider found it: from a file's text
/// (`tests_scan`), its runner's listing, or a run ([`Report::Found`]). A
/// `Found` for an id Fenix has already is merged with it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    /// The provider's own name for it, the same in what it scans, lists
    /// and reads from a run: `orbit/lib tests::adds`,
    /// `tests/test_basic.py::test_cases[negative]`. Unique among its
    /// provider's. One over 1 KiB is cut, the same way wherever it comes:
    /// its first bytes up to 1,007, back to a character's start, then `#`
    /// and the FNV-1a 64-bit hash of the whole id's UTF-8 as 16 lowercase
    /// hex digits (`WRITING_EXTENSIONS.md` has test vectors); Fenix gives
    /// it back so, and the provider names it to its runner by its `data`.
    pub id: String,
    /// The group it's in, by id: `None` for one at its provider's top.
    /// Given back, the one Fenix put it under.
    pub parent: Option<String>,
    /// What the page calls it: `adds`, `negative`, `orbit (lib)`. At most
    /// 256 characters.
    pub label: String,
    pub kind: Kind,
    /// Its file: `/project/<path>`; or relative, to the project in a
    /// scan's answer and to its step's folder in a run's or a listing's
    /// report; or a full path of the project's machine. In a scan's
    /// answer, `None` for a test or case is the file scanned. Given back,
    /// `/project/<path>` with `/` (a full path outside the project).
    pub path: Option<String>,
    /// Its first line, and its last when it knows: from 0. Given back,
    /// with `path`, from its scan (maybe of unsaved text), else its
    /// listing, else a run.
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    /// Left out by its source (`#[ignore = "slow"]`, `@skip`,
    /// `test.skip`): why, or "" when it doesn't say.
    pub skip: Option<String>,
    /// Its cases are found only by running it: a `test.each`, a
    /// `parametrize`, a Go test that calls `t.Run`.
    pub template: bool,
    /// Its own, at most 4 KiB: never shown, given back as it was wherever
    /// Fenix gives this node back ([`Ask::Run`], `tests_launch`) -- what
    /// its runner calls it when the id can't say. It can come back `None`:
    /// one over 4 KiB isn't kept, nor any past 16 MiB of a provider's, nor
    /// across a restart until a scan, listing or run gives it again. A
    /// provider names a node by its id when its `data` is `None`; a node
    /// whose id was cut and whose `data` isn't kept is never given back
    /// for itself (its nearest ancestor is asked instead).
    pub data: Option<String>,
}

/// What a node of the tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Kind {
    /// A Cargo target, a Go package, a Jest project: runs everything under it.
    Group,
    File,
    /// A Rust `mod tests`, a pytest class, a `describe`.
    Suite,
    #[default]
    Test,
    /// One case of a template (`test_cases[negative]`).
    Case,
}

/// What Fenix asks a provider for (`tests_plan`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Its runner's listing of every test it can find in the project.
    Discover,
    /// These, as Fenix has them now: tests, cases, and groups (all under
    /// them). After each `File` or `Suite` comes its fill-in: every node
    /// Fenix has under it, suites included, while the ask stays within
    /// 10,000 records; a fill-in is whole or left out, and one with
    /// nothing after it is planned by its runner's own filter for it. A
    /// record whose `parent` is in the ask is a fill-in; one whose
    /// `parent` isn't was asked for itself.
    Run(Vec<Found>),
    /// Every test.
    RunAll,
}

/// What runs, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Its listing costs little once this plan has run (it built what the
    /// listing builds), so Fenix lists after it. Ignored in a listing's
    /// own plan.
    pub list_after: bool,
}

/// One command of a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// A program its manifest lists (`permissions.process`), run as it is,
    /// with no shell between; `${data}` and `${tools}` in its args and env
    /// are filled in. Its command line, as Windows quotes it, at most
    /// 32,766 characters.
    pub command: Command,
    /// What the page says while it runs: `orbit (lib)`. At most 256
    /// characters.
    pub label: String,
    /// The ids it runs, when it knows them: a group's, a file's or a
    /// suite's stands for everything under it. Empty: it claims nothing
    /// of its own, and what of the ask no step names is judged when the
    /// plan's last step has ended. Ignored in a listing.
    pub covers: Vec<String>,
    /// Files under `/data` its command reads (a reporter, a file of ids):
    /// each must be there when the step starts.
    pub needs: Vec<String>,
}

/// A line of a step's output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// As UTF-8, what wasn't replaced by U+FFFD; colour codes taken out.
    pub text: String,
    pub stderr: bool,
    /// The line went on past 64 KiB: the rest is the next line of the
    /// same stream (lines of the other may come between, and a later
    /// call). Every piece but the last is cut; a line of 64 KiB or less,
    /// at a stream's end too, never is.
    pub cut: bool,
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepEnd {
    /// Its program ended: its exit code, `None` for a signal (Linux). On
    /// Windows a crash's NTSTATUS (`0xC0000005`) is negative.
    Exited(Option<i32>),
    /// Stopped: by you, by a run that replaced it, by its provider starting
    /// again, or never started because the run was stopped before it.
    Stopped,
    /// It ran past `tests.timeout_minutes`, and was stopped.
    TimedOut,
    /// It couldn't start, or its output couldn't be read: why.
    Failed(String),
}

/// How a test ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Passed,
    Failed,
    Skipped,
    /// It couldn't run to a result: its setup or fixture failed.
    Errored,
}

/// A place in a file: from 0, columns in characters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Location {
    /// `/project/<path>`; relative, to the step's folder; or a full path
    /// of the project's machine.
    pub path: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// A test that ended. The last `Ended` for an id in a run is its result.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    pub id: String,
    pub outcome: Outcome,
    /// How long it took, when its runner says.
    pub duration_ms: Option<f64>,
    /// One line: a failure's message, a skip's reason, why it errored.
    pub message: Option<String>,
    /// Where it failed: the assertion, the panic, the deepest frame in the
    /// project.
    pub at: Option<Location>,
    /// All of it: the diff, the traceback, what it printed.
    pub text: Option<String>,
}

/// What a provider read in its runner's output. Fenix applies them in
/// order.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// A test or group: from a listing, or found in a run.
    Found(Found),
    /// A test started, by id.
    Started(String),
    Ended(Ended),
    /// What a test printed, or the run did (`id` `None`).
    Output { id: Option<String>, text: String },
    /// What didn't build or load, never a test's failure: the group or
    /// file that didn't, `None` for all the step covers. What's under it
    /// and has no result in this run yet is errored with it.
    Broken { id: Option<String>, message: String, at: Option<Location> },
    /// A process died: the group whose process it was (what's under it and
    /// has no result in this run yet is errored with it), `None` for all
    /// the step covers that has none.
    Crashed { id: Option<String>, message: String },
}

/// How to debug a test.
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// This launch, now.
    Launch(LaunchConfig),
    /// These first -- a build that says where the test's binary is --
    /// read as a run's are, as the run `tests_launch` was asked with; then
    /// `tests_launch` is asked again, once, with the same run, if every
    /// step exited 0 and nothing was `Broken` or `Crashed`.
    BuildFirst(Plan),
}

/// What its test provider `provider` is in its project now: the runner it
/// uses (`cargo nextest`, at most 256 characters), and why it can't run
/// tests there when it can't (`pytest isn't installed in .venv`, at most 4
/// KiB). `runner` "" and no `problem` is "nothing to run in this project":
/// it isn't shown or asked there until a later call says otherwise. A call
/// that changes what Fenix had lists the project's tests again; one that
/// doesn't changes nothing. Fenix keeps it for the project until it exits,
/// across the extension starting again; call it at each start all the
/// same. Linking a call to it -- reachable from an export, whether or not
/// it runs -- makes the component need API 1.6. API 1.6.
///
/// The copy's body: API 1.5 has no such import, so this asks nothing of
/// Fenix and says it worked. The real one calls the host -- which is why
/// lang-kit builds this module, and its one caller (`Host::test_provider_set`
/// for `Fenix`), only with its feature `tests`.
pub fn provider(provider: &str, runner: &str, problem: Option<&str>) -> Result<(), String> {
    let _ = (provider, runner, problem);
    Ok(())
}

/// The shape is the real module's, field for field and case for case: a
/// constructor of each type by every field, and a match of each enum by
/// every case, written as the real module's own tests and docs write
/// them. On `api-1.6` this compiles against the real types unchanged.
#[cfg(test)]
mod shape {
    use super::*;

    #[test]
    fn every_type_field_and_case_is_the_apis() {
        let found = Found { id: "a/lib t::x".into(), parent: Some("a/lib".into()), label: "x".into(), kind: Kind::Case, path: Some("src/lib.rs".into()), line: Some(3), end_line: Some(9), skip: Some(String::new()), template: true, data: Some("{\"n\":1}".into()) };
        assert_eq!(Found::default().kind, Kind::Test);
        assert_eq!(Kind::default(), Kind::Test);
        for kind in [Kind::Group, Kind::File, Kind::Suite, Kind::Test, Kind::Case] {
            let _: u8 = match kind {
                Kind::Group => 0,
                Kind::File => 1,
                Kind::Suite => 2,
                Kind::Test => 3,
                Kind::Case => 4,
            };
        }
        for ask in [Ask::Discover, Ask::Run(vec![found.clone()]), Ask::RunAll] {
            let _: Option<&Vec<Found>> = match &ask {
                Ask::Discover | Ask::RunAll => None,
                Ask::Run(found) => Some(found),
            };
        }
        let step = Step { command: Command::new("cargo"), label: "orbit".into(), covers: vec!["orbit".into()], needs: vec!["/data/r.py".into()] };
        let plan = Plan { steps: vec![step], list_after: true };
        assert_eq!(Plan::default().steps.len(), 0);
        let line = Line { text: "x".into(), stderr: true, cut: true };
        assert_eq!(Line::default(), Line { text: String::new(), stderr: false, cut: false });
        for end in [StepEnd::Exited(Some(-1_073_741_819)), StepEnd::Exited(None), StepEnd::Stopped, StepEnd::TimedOut, StepEnd::Failed("why".into())] {
            let _: Option<Option<i32>> = match end {
                StepEnd::Exited(code) => Some(code),
                StepEnd::Stopped | StepEnd::TimedOut | StepEnd::Failed(_) => None,
            };
        }
        let outcomes: std::collections::HashSet<Outcome> = [Outcome::Passed, Outcome::Failed, Outcome::Skipped, Outcome::Errored].into_iter().collect();
        assert_eq!(outcomes.len(), 4);
        let at = Location { path: "src/lib.rs".into(), line: 4, column: Some(5) };
        assert_eq!(Location::default().column, None);
        let ended = Ended { id: "t".into(), outcome: Outcome::Errored, duration_ms: Some(0.5), message: Some("m".into()), at: Some(at.clone()), text: None };
        for report in [
            Report::Found(found),
            Report::Started("t".into()),
            Report::Ended(ended),
            Report::Output { id: None, text: "o".into() },
            Report::Broken { id: Some("g".into()), message: "b".into(), at: Some(at) },
            Report::Crashed { id: Some("g".into()), message: "SIGABRT".into() },
        ] {
            let _: u8 = match report {
                Report::Found(_) => 0,
                Report::Started(_) => 1,
                Report::Ended(_) => 2,
                Report::Output { .. } => 3,
                Report::Broken { .. } => 4,
                Report::Crashed { .. } => 5,
            };
        }
        for launch in [Launch::Launch(LaunchConfig::launch("t", &["rust"])), Launch::BuildFirst(plan)] {
            let _: u8 = match launch {
                Launch::Launch(_) => 0,
                Launch::BuildFirst(_) => 1,
            };
        }
        let _ = (line, provider("rust", "cargo test", None));
    }
}
