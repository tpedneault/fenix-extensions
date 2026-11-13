//! API 1.6's `tests` module, the real one: the types a test provider's
//! five exports and its one import take, and [`provider`], the call that
//! tells Fenix the runner (`test-provider-set`).
//!
//! lang-kit used to carry a copy of these by hand while API 1.6 was
//! unreleased (`plans/test-explorer.md`, "As built: E1a"); on this branch
//! it builds on Fenix's own, pinned by `rev` in `Cargo.toml`, so a type
//! here is the type of the pack's own `fenix_extension_api`.
//!
//! The whole module is built only with lang-kit's feature `tests`: the one
//! caller of [`provider`] is `Host::test_provider_set` for `Fenix`, and a
//! call reachable from an export links the import even behind a condition
//! that never holds.

pub use fenix_extension_api::tests::*;

/// The shape is the real module's, field for field and case for case: a
/// constructor of each type by every field, and a match of each enum by
/// every case, written as the real module's own tests and docs write
/// them. It compiles against the real types unchanged: if Fenix's API
/// changes a field, a case or a derive lang-kit relies on, this stops
/// compiling at the pin that moved.
#[cfg(test)]
mod shape {
    use super::*;
    use fenix_extension_api::{Command, LaunchConfig};

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
        // The one import, by its signature: called, it would be Fenix's.
        let _: fn(&str, &str, Option<&str>) -> Result<(), String> = provider;
        let _ = line;
    }
}
