//! Kiln's item in the modeline: the last `kiln check`'s findings, short
//! (`kiln 1 error, 2 warnings`), and nothing when there are none -- a
//! clean project shows no item rather than a reassurance that takes room.
//!
//! The text is worked out from the project's state, so while a reload
//! runs it is still the previous read's (the state keeps it), and it
//! changes only when a read says something else. `lib.rs` sets it when it
//! differs from the last text set.
//!
//! What it deliberately doesn't do: say anything about the plan, offer a
//! click or a command (the Check tab is `kiln.check`), or know the findings (kiln's counts, as [`crate::project::Loaded::counts`]
//! has them, are all it uses).

use crate::project::State;

/// What the modeline should say now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Not known yet (kiln being looked for, the first read running):
    /// what was set stays.
    Keep,
    /// No item: clean, not loaded, or not a Kiln project.
    None,
    Text(String),
}

/// The item for a project in `state`.
pub fn line(state: &State) -> Line {
    match state {
        State::Idle | State::Probing | State::Loading => Line::Keep,
        State::NotFound(_)
        | State::NotKiln(_)
        | State::TooOld(_)
        | State::NoProject(_)
        | State::Failed(_) => Line::None,
        State::Loaded { loaded, .. } => match loaded.counts().and_then(|c| c.said()) {
            Some(said) => Line::Text(format!("kiln {said}")),
            None => Line::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture, Counts, Version};
    use crate::project::Loaded;

    fn loaded(
        findings: Result<Vec<model::Finding>, String>,
        plan: Result<model::Status, String>,
    ) -> Loaded {
        Loaded::new(
            Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            model::list(&fixture("demo/list.json")).unwrap(),
            plan,
            findings,
        )
    }

    fn state(l: &Loaded) -> State<'_> {
        State::Loaded {
            loaded: l,
            reloading: false,
            error: None,
        }
    }

    #[test]
    fn errors_and_warnings_are_counted_short() {
        let demo = loaded(
            model::check(&fixture("demo/check.json")),
            model::status(&fixture("demo/status.json")),
        );
        assert_eq!(
            line(&state(&demo)),
            Line::Text("kiln 1 error, 2 warnings".into())
        );
    }

    #[test]
    fn only_what_there_is_is_said() {
        let status = |errors, warnings| {
            let mut s = model::status(&fixture("demo/status.json")).unwrap();
            s.findings = Counts { errors, warnings };
            s
        };
        // `check` failed: status's counts stand in.
        for (e, w, said) in [
            (2, 0, "kiln 2 errors"),
            (0, 1, "kiln 1 warning"),
            (0, 3, "kiln 3 warnings"),
        ] {
            let l = loaded(Err("kiln: boom".into()), Ok(status(e, w)));
            assert_eq!(line(&state(&l)), Line::Text(said.into()));
        }
    }

    #[test]
    fn none_when_clean_or_unknown_or_not_loaded() {
        let clean = loaded(Ok(Vec::new()), model::status(&fixture("demo/status.json")));
        assert_eq!(line(&state(&clean)), Line::None);
        let unknown = loaded(Err("x".into()), Err("y".into()));
        assert_eq!(line(&state(&unknown)), Line::None);
        for s in [
            State::NotFound("x"),
            State::NotKiln("x"),
            State::NoProject("x"),
            State::Failed("x"),
            State::TooOld(Version {
                major: 0,
                minor: 9,
                patch: 0,
            }),
        ] {
            assert_eq!(line(&s), Line::None);
        }
    }

    #[test]
    fn while_it_is_being_found_or_read_the_text_stays() {
        for s in [State::Idle, State::Probing, State::Loading] {
            assert_eq!(line(&s), Line::Keep);
        }
    }

    #[test]
    fn a_reload_or_a_failed_one_keeps_the_last_reads_text() {
        let demo = loaded(
            model::check(&fixture("demo/check.json")),
            model::status(&fixture("demo/status.json")),
        );
        let reloading = State::Loaded {
            loaded: &demo,
            reloading: true,
            error: None,
        };
        let failed = State::Loaded {
            loaded: &demo,
            reloading: false,
            error: Some("kiln: bad"),
        };
        let said = Line::Text("kiln 1 error, 2 warnings".into());
        assert_eq!(line(&reloading), said);
        assert_eq!(line(&failed), said);
    }
}
