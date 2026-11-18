//! The one notification: a save took `kiln check` from no errors to at
//! least one.
//!
//! A notification interrupts, so it is for news and only for news. That is
//! one transition: the last read the extension saw had no errors, the one a
//! save caused has some. Not the first read of a project that already has
//! errors (nothing changed), not every read while errors persist (the same
//! news), not warnings (the modeline and the doctor say them), not a read a
//! git move, a timer, `r` or kiln's own edit asked for (the person did not
//! just save something). It is tagged, so a newer one replaces it, and taken
//! away when a later check has no errors.
//!
//! What it deliberately doesn't do: read `kiln check` itself (the project
//! did), decide from warnings, or show more than the first error -- the
//! Check tab has the rest, and the notification's one action opens it.

use std::path::Path;

use crate::model::{self, count, Finding, Severity};
use crate::project::Loaded;

/// The setting that turns it off: on unless it says `false`.
pub const SETTING: &str = "kiln.notify_errors";

/// What replaces and dismisses it.
pub const TAG: &str = "kiln.check-errors";

/// The command its action runs: the plan's Check tab.
pub const OPEN_CHECK: &str = "kiln.findings";

/// What to do about the notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Nothing,
    Show,
    /// Take it away: errors are gone (or it is turned off).
    Dismiss,
}

/// Whether the setting is on: anything but `false` (a missing or unreadable
/// setting is the default, on).
pub fn enabled(setting: Option<&str>) -> bool {
    setting.map(str::trim) != Some("false")
}

/// The errors `kiln check` found in `loaded`: `None` when it gave no answer
/// (the count `kiln status` carries is no substitute: it isn't the check a
/// save is about).
pub fn errors(loaded: &Loaded) -> Option<u32> {
    loaded
        .findings
        .as_ref()
        .ok()
        .map(|f| model::counts(f).errors)
}

/// What a read that loaded does to the notification. `before`: the errors of
/// the read before it, `None` for none (the first read, or one with no
/// check); `after`: this read's; `saved`: a save asked for it; `on`: the
/// setting; `showing`: one was shown and not taken away since.
pub fn decide(
    before: Option<u32>,
    after: Option<u32>,
    saved: bool,
    on: bool,
    showing: bool,
) -> Act {
    match after {
        // No answer from the check: what is shown stays as it is.
        None => Act::Nothing,
        Some(0) if showing => Act::Dismiss,
        Some(0) => Act::Nothing,
        Some(_) if !on && showing => Act::Dismiss,
        Some(_) if on && saved && before == Some(0) => Act::Show,
        Some(_) => Act::Nothing,
    }
}

/// The notification's summary and body for the errors in `loaded`, after
/// `saved` was saved: `Kiln: 1 error after saving TASK-006-….md` and
/// `design/tasks/TASK-006-….md:7  K102  reference REQ-099 doesn't exist`.
/// `None` when there is no error to show. The file name and the message are
/// the project's text: cleaned and cut.
pub fn text(loaded: &Loaded, saved: Option<&str>) -> Option<(String, String)> {
    let findings = loaded.findings.as_ref().ok()?;
    let first: &Finding = findings.iter().find(|f| f.severity == Severity::Error)?;
    let n = model::counts(findings).errors as usize;
    let summary = match saved
        .and_then(|p| Path::new(p).file_name())
        .map(|n| model::plain(&n.to_string_lossy(), 48))
        .filter(|n| !n.is_empty())
    {
        Some(file) => format!("Kiln: {} after saving {file}", count(n, "error")),
        None => format!("Kiln: {} after a save", count(n, "error")),
    };
    let place = match first.line {
        0 => model::plain(&first.path, 120),
        line => format!("{}:{line}", model::plain(&first.path, 120)),
    };
    let body = format!(
        "{place}  {}  {}",
        model::plain(&first.code, 12),
        model::plain(&first.message, 200)
    );
    Some((summary, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use crate::model::Version;

    fn loaded(findings: Result<Vec<Finding>, String>) -> Loaded {
        Loaded::new(
            Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            Vec::new(),
            Err("no plan".into()),
            findings,
        )
    }

    #[test]
    fn every_combination_of_before_after_reason_and_setting() {
        let nothing = Act::Nothing;
        // (before, after, saved, on, showing) -> act, written out.
        #[allow(clippy::type_complexity)]
        let cases: &[(Option<u32>, Option<u32>, bool, bool, bool, Act)] = &[
            // Clean to errors, by a save, on: the one case that shows.
            (Some(0), Some(1), true, true, false, Act::Show),
            (Some(0), Some(3), true, true, false, Act::Show),
            // ... not by anything else.
            (Some(0), Some(1), false, true, false, nothing),
            // ... not when it is off.
            (Some(0), Some(1), true, false, false, nothing),
            (Some(0), Some(1), false, false, false, nothing),
            // The first read of a project that has errors.
            (None, Some(1), true, true, false, nothing),
            (None, Some(1), false, true, false, nothing),
            // Errors persisting, or growing: the same news.
            (Some(1), Some(1), true, true, true, nothing),
            (Some(1), Some(2), true, true, true, nothing),
            (Some(1), Some(1), true, true, false, nothing),
            (Some(2), Some(1), true, true, true, nothing),
            // A check with no errors: the notification goes, once.
            (Some(1), Some(0), true, true, true, Act::Dismiss),
            (Some(1), Some(0), false, true, true, Act::Dismiss),
            (Some(1), Some(0), true, false, true, Act::Dismiss),
            (Some(1), Some(0), true, true, false, nothing),
            (Some(0), Some(0), true, true, false, nothing),
            (None, Some(0), false, true, false, nothing),
            // Turned off while it shows: taken away.
            (Some(1), Some(1), false, false, true, Act::Dismiss),
            (Some(1), Some(2), true, false, true, Act::Dismiss),
            // The check gave no answer: nothing changes.
            (Some(0), None, true, true, false, nothing),
            (Some(1), None, true, true, true, nothing),
            (None, None, false, true, false, nothing),
        ];
        for &(before, after, saved, on, showing, want) in cases {
            assert_eq!(
                decide(before, after, saved, on, showing),
                want,
                "before {before:?}, after {after:?}, saved {saved}, on {on}, showing {showing}"
            );
        }
        // And exhaustively: it shows only on the one transition.
        for before in [None, Some(0), Some(2)] {
            for after in [None, Some(0), Some(1)] {
                for saved in [false, true] {
                    for on in [false, true] {
                        for showing in [false, true] {
                            let act = decide(before, after, saved, on, showing);
                            let transition =
                                before == Some(0) && after.is_some_and(|a| a > 0) && saved && on;
                            assert_eq!(
                                act == Act::Show,
                                transition,
                                "{before:?} {after:?} {saved} {on} {showing}"
                            );
                            if act == Act::Dismiss {
                                assert!(showing, "nothing to take away");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_setting_is_on_unless_it_says_false() {
        assert!(enabled(None));
        assert!(enabled(Some("true")));
        assert!(enabled(Some("")));
        assert!(enabled(Some("nonsense")));
        assert!(!enabled(Some("false")));
        assert!(!enabled(Some(" false ")));
    }

    #[test]
    fn errors_are_the_checks_not_the_plans() {
        assert_eq!(errors(&loaded(Ok(Vec::new()))), Some(0));
        assert_eq!(errors(&loaded(Err("no check".into()))), None);
        let f = model::check(&fixture("demo/check.json")).unwrap();
        assert_eq!(errors(&loaded(Ok(f))), Some(1));
    }

    #[test]
    fn the_text_names_the_file_saved_and_the_first_error() {
        let f = model::check(&fixture("demo/check.json")).unwrap();
        let l = loaded(Ok(f));
        let (summary, body) = text(&l, Some("/home/me/p/design/tasks/TASK-006-draft.md")).unwrap();
        assert_eq!(summary, "Kiln: 1 error after saving TASK-006-draft.md");
        assert_eq!(
            body,
            "design/tasks/TASK-006-draft-the-user-guide.md:11  K102  reference REQ-099 doesn't exist"
        );
        let (summary, _) = text(&l, None).unwrap();
        assert_eq!(summary, "Kiln: 1 error after a save");
    }

    #[test]
    fn the_text_is_plural_cut_and_cleaned() {
        let finding = |sev, code: &str, msg: &str, line| Finding {
            code: code.into(),
            severity: sev,
            message: msg.into(),
            path: "a.md".into(),
            line,
            column: 0,
        };
        let l = loaded(Ok(vec![
            finding(Severity::Warning, "K201", "a warning first", 1),
            finding(
                Severity::Error,
                "K102",
                &format!("bad\u{7}{}", "x".repeat(400)),
                0,
            ),
            finding(Severity::Error, "K103", "second", 4),
        ]));
        let (summary, body) = text(&l, Some(&format!("{}.md", "n".repeat(100)))).unwrap();
        assert!(
            summary.starts_with("Kiln: 2 errors after saving nnn"),
            "{summary}"
        );
        assert!(summary.chars().count() < 80, "{summary}");
        assert!(
            body.starts_with("a.md  K102  bad"),
            "no line, no colon: {body}"
        );
        assert!(
            !body.contains('\u{7}') && body.chars().count() < 260,
            "{body}"
        );
        assert_eq!(text(&loaded(Ok(Vec::new())), None), None);
        assert_eq!(text(&loaded(Err("x".into())), None), None);
    }
}

/// The notification through the extension, against test-kit's fake host.
#[cfg(test)]
mod wiring {
    use super::*;
    use crate::model::tests::fixture;
    use crate::tests::{demo, folder, harness};
    use fenix_test_kit::{Fake, Harness, Ran};

    const SAVED: &str = "/project/design/tasks/TASK-006-draft-the-user-guide.md";

    fn clean(fake: &Fake) {
        fake.on_spawn("kiln check --format json", Ran::ok("[]\n"));
    }

    fn errors(fake: &Fake) {
        fake.on_spawn(
            "kiln check --format json",
            Ran::exits(1).stdout(fixture("demo/check.json")),
        );
    }

    /// The demo, its check clean, activated.
    fn started(name: &str) -> (Fake, Harness<crate::Kiln>) {
        let fake = demo();
        clean(&fake);
        let mut h = harness(&fake, &folder(name));
        h.size = (110, 26);
        h.activate().unwrap();
        assert!(fake.toasts().is_empty());
        (fake, h)
    }

    #[test]
    fn a_save_that_takes_the_check_from_clean_to_errors_notifies_once() {
        let (fake, mut h) = started("alert-once");
        errors(&fake);
        h.saved(SAVED);
        let toasts = fake.toasts();
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        let t = &toasts[0];
        assert_eq!(
            t.summary,
            "Kiln: 1 error after saving TASK-006-draft-the-user-guide.md"
        );
        assert_eq!(
            t.body,
            "design/tasks/TASK-006-draft-the-user-guide.md:11  K102  reference REQ-099 doesn't exist"
        );
        assert_eq!(t.tag.as_deref(), Some(TAG));
        assert_eq!(t.actions.len(), 1);
        assert_eq!(t.actions[0].command, "kiln.findings");
        assert_eq!(t.urgency, crate::api::Urgency::Normal);
        // Saved again with the error still there: no second one.
        h.saved(SAVED);
        h.saved("/project/design/tasks/TASK-004-add-the-export-button.md");
        assert_eq!(fake.toasts().len(), 1);
        // A timer's, git's or `r`'s read of the same errors: nothing.
        h.command("kiln.reload").unwrap();
        assert_eq!(fake.toasts().len(), 1);
        assert!(fake.dismissed().is_empty());
    }

    #[test]
    fn gone_when_a_later_check_has_no_errors_and_new_again_after() {
        let (fake, mut h) = started("alert-dismiss");
        errors(&fake);
        h.saved(SAVED);
        assert_eq!(fake.showing().len(), 1);
        // Fixed: the check is clean, by whatever read.
        clean(&fake);
        h.command("kiln.reload").unwrap();
        assert_eq!(fake.dismissed(), [TAG]);
        assert!(fake.showing().is_empty());
        // Clean again: nothing more to dismiss.
        h.saved(SAVED);
        assert_eq!(fake.dismissed(), [TAG]);
        // Broken again by a save: a new one, replacing none (it is gone).
        errors(&fake);
        h.saved("/project/design/requirements/REQ-001-readings-are-logged-every-second.md");
        assert_eq!(fake.toasts().len(), 2);
        assert!(fake.toasts()[1]
            .summary
            .ends_with("after saving REQ-001-readings-are-logged-every-second.md"));
        assert_eq!(fake.showing().len(), 1);
    }

    #[test]
    fn not_for_a_project_that_already_had_errors_when_first_read() {
        let fake = demo();
        let mut h = harness(&fake, &folder("alert-first"));
        h.activate().unwrap();
        h.saved(SAVED);
        h.saved(SAVED);
        assert!(fake.toasts().is_empty(), "{:?}", fake.toasts());
    }

    #[test]
    fn not_for_warnings_a_git_move_a_timer_or_kiln_s_own_edit() {
        let (fake, mut h) = started("alert-other");
        // Warnings only.
        let warn = fixture("demo/check.json").replace("\"error\"", "\"warning\"");
        fake.on_spawn("kiln check --format json", Ran::ok(warn));
        h.saved(SAVED);
        assert!(fake.toasts().is_empty());
        // Errors, found by what isn't a save.
        clean(&fake);
        h.saved(SAVED);
        errors(&fake);
        h.ext.project.reload(crate::project::Why::Git);
        h.settle();
        h.ext.project.reload(crate::project::Why::Timer);
        h.settle();
        h.ext.project.edited();
        h.settle();
        h.command("kiln.reload").unwrap();
        assert!(fake.toasts().is_empty(), "{:?}", fake.toasts());
        // The first save after is not news either: errors were already
        // there when it read.
        h.saved(SAVED);
        assert!(fake.toasts().is_empty());
    }

    #[test]
    fn the_setting_turns_it_off_and_takes_a_shown_one_away() {
        let fake = demo();
        clean(&fake);
        fake.setting(SETTING, "false");
        let mut h = harness(&fake, &folder("alert-off"));
        h.activate().unwrap();
        errors(&fake);
        h.saved(SAVED);
        assert!(fake.toasts().is_empty());
        // On again, then broken once more by a save after a clean one.
        fake.setting(SETTING, "true");
        h.set_setting(SETTING, Some("true"));
        clean(&fake);
        h.saved(SAVED);
        errors(&fake);
        h.saved(SAVED);
        assert_eq!(fake.toasts().len(), 1);
        // Off while it shows: gone at once.
        h.set_setting(SETTING, Some("false"));
        assert_eq!(fake.dismissed(), [TAG]);
        assert!(fake.showing().is_empty());
    }

    #[test]
    fn the_action_opens_the_plans_check_tab_and_so_does_the_doctors_fix() {
        let (fake, mut h) = started("alert-open");
        errors(&fake);
        h.saved(SAVED);
        let action = fake.toasts()[0].actions[0].command.clone();
        h.command(&action).unwrap();
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), crate::pages::Tab::Check);
        assert!(h.screen(page).contains("K102"), "{}", h.screen(page));
        // The plan already open on another tab: the same page, shown there.
        h.keys(page, "1");
        h.command("kiln.findings").unwrap();
        assert_eq!(h.ext.pages.len(), 1);
        assert_eq!(h.ext.pages[&page].plan.tab(), crate::pages::Tab::Check);
    }
}
