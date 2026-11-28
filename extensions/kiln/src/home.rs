//! Kiln's section on Fenix's Home: the work that is ready to start, and a
//! row that opens the plan.
//!
//! The rows come from `kiln status`'s groups ([`Loaded::groups`]) -- the
//! definition of ready that `kiln next` and the Board share -- earliest
//! milestone first, the work in no milestone last, as kiln orders them. At
//! most [`MOST_READY`] are shown, so the section stays a glance (the agenda's
//! shows four, the notebook's five); when more are ready, the last row says
//! how many. Picking a ready row opens the plan on that item's Item tab,
//! the last row opens the plan.
//!
//! What it deliberately doesn't do: recompute ready, sort the work its own
//! way, show owners or statuses (the Board does), or hold the project: it
//! is worked out from the state each time and given to Fenix only when it
//! differs from what Fenix was last given.

use crate::model;
use crate::project::{Loaded, State};

/// The most ready items shown; the row that opens the plan comes after
/// them.
pub const MOST_READY: usize = 4;

/// The most rows the section is allowed: the ready ones and the plan's.
pub const MOST_ROWS: u32 = MOST_READY as u32 + 1;

/// The value of the row that opens the plan: not an ID, which can't start
/// with `@`.
pub const PLAN: &str = "@plan";

/// How much of a title or a reason a row keeps.
const TITLE_CHARS: usize = 60;

/// One row of the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// `TASK-004  Add the export button`.
    pub name: String,
    /// `(MS-001)`, or what the row says besides its name.
    pub detail: String,
    /// What picking it gives back: an item's ID, or [`PLAN`].
    pub value: String,
}

/// What Home should show now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Home {
    /// Not known yet (kiln being looked for, the first read running): what
    /// was set stays.
    Keep,
    /// No section: not a project kiln can read.
    Clear,
    Set(Vec<Row>),
}

/// The section for a project in `state`.
pub fn home(state: &State) -> Home {
    match state {
        State::Idle | State::Probing | State::Loading => Home::Keep,
        State::NotFound(_)
        | State::NotKiln(_)
        | State::TooOld(_)
        | State::NoProject(_)
        | State::Failed(_) => Home::Clear,
        State::Loaded { loaded, .. } => Home::Set(rows(loaded)),
    }
}

/// The rows for a read project.
pub fn rows(loaded: &Loaded) -> Vec<Row> {
    let plan_row = |detail: String| Row {
        name: "open the plan".into(),
        detail,
        value: PLAN.into(),
    };
    if let Err(why) = &loaded.plan {
        return vec![Row {
            name: "the plan can't be read".into(),
            detail: model::plain(why, TITLE_CHARS),
            value: PLAN.into(),
        }];
    }
    let ready: Vec<_> = loaded
        .groups()
        .iter()
        .flat_map(|g| g.ready.iter().map(move |w| (g, w)))
        .collect();
    if ready.is_empty() {
        let (doing, waiting) = loaded.groups().iter().fold((0, 0), |(d, w), g| {
            (d + g.in_progress.len(), w + g.waiting.len())
        });
        return vec![Row {
            name: "nothing ready".into(),
            detail: format!("{doing} in progress, {waiting} waiting"),
            value: PLAN.into(),
        }];
    }
    let mut rows: Vec<Row> = ready
        .iter()
        .take(MOST_READY)
        .map(|(g, w)| Row {
            name: format!("{}  {}", w.id, model::plain(&w.title, TITLE_CHARS)),
            detail: g
                .milestone
                .as_ref()
                .map(|m| format!("({})", model::plain(&m.id, 20)))
                .unwrap_or_default(),
            value: w.id.clone(),
        })
        .collect();
    let more = ready.len().saturating_sub(MOST_READY);
    rows.push(plan_row(if more > 0 {
        format!("{more} more ready")
    } else {
        String::new()
    }));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, tests::fixture, Group, Milestone, Status, Version, Work};

    fn version() -> Version {
        Version {
            major: 1,
            minor: 0,
            patch: 0,
        }
    }

    fn loaded(plan: Result<Status, String>) -> Loaded {
        Loaded::new(
            version(),
            model::list(&fixture("demo/list.json")).unwrap(),
            plan,
            Ok(Vec::new()),
        )
    }

    fn work(id: &str) -> Work {
        Work {
            id: id.into(),
            title: format!("Title of {id}"),
            status: "todo".into(),
            owner: None,
        }
    }

    fn group(ms: Option<&str>, ready: &[&str], doing: usize) -> Group {
        Group {
            milestone: ms.map(|m| Milestone {
                id: m.into(),
                title: "T".into(),
                status: "active".into(),
                due: None,
            }),
            done: 0,
            total: 0,
            in_progress: (0..doing).map(|i| work(&format!("D-{i}"))).collect(),
            ready: ready.iter().map(|r| work(r)).collect(),
            waiting: Vec::new(),
        }
    }

    fn status(groups: Vec<Group>) -> Status {
        let mut s = model::status(&fixture("demo/status.json")).unwrap();
        s.milestones = groups;
        s
    }

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{} | {} | {}", r.name, r.detail, r.value))
            .collect()
    }

    #[test]
    fn the_demo_shows_its_ready_work_by_milestone_and_the_plan() {
        let l = loaded(model::status(&fixture("demo/status.json")));
        assert_eq!(
            text(&rows(&l)),
            [
                "TASK-003  Export the log as CSV | (MS-001) | TASK-003",
                "TASK-007  Batch the writes | (MS-001) | TASK-007",
                "TASK-006  Draft the user guide | (MS-002) | TASK-006",
                "TASK-008  Tidy the build scripts |  | TASK-008",
                "open the plan |  | @plan",
            ]
        );
    }

    #[test]
    fn more_than_a_handful_says_how_many_more() {
        let l = loaded(Ok(status(vec![
            group(Some("MS-001"), &["A-1", "A-2", "A-3"], 0),
            group(None, &["B-1", "B-2", "B-3"], 0),
        ])));
        let r = rows(&l);
        assert_eq!(r.len(), MOST_READY + 1);
        assert_eq!(r[3].value, "B-1");
        assert_eq!(r[3].detail, "", "work in no milestone has none to name");
        assert_eq!(r[4].detail, "2 more ready");
        assert_eq!(r[4].value, PLAN);
        // Exactly a handful: nothing more to say.
        let l = loaded(Ok(status(vec![group(
            None,
            &["A-1", "A-2", "A-3", "A-4"],
            0,
        )])));
        assert_eq!(rows(&l)[4].detail, "");
    }

    #[test]
    fn nothing_ready_says_so_with_the_counts_and_still_opens_the_plan() {
        let mut g = group(Some("MS-001"), &[], 2);
        g.waiting = model::status(&fixture("demo/status.json"))
            .unwrap()
            .milestones[0]
            .waiting
            .clone();
        let l = loaded(Ok(status(vec![g])));
        assert_eq!(
            text(&rows(&l)),
            ["nothing ready | 2 in progress, 1 waiting | @plan"]
        );
        let l = loaded(Ok(status(Vec::new())));
        assert_eq!(
            text(&rows(&l)),
            ["nothing ready | 0 in progress, 0 waiting | @plan"]
        );
    }

    #[test]
    fn a_plan_that_cant_be_read_is_said() {
        let l = loaded(Err("kiln status failed: boom\nsecond line".into()));
        assert_eq!(
            text(&rows(&l)),
            ["the plan can't be read | kiln status failed: boom second line | @plan"]
        );
    }

    #[test]
    fn a_title_is_data_cut_and_cleaned() {
        let mut w = work("A-1");
        w.title = format!("x\u{1b}[31m{}", "y".repeat(200));
        let mut g = group(None, &[], 0);
        g.ready.push(w);
        let l = loaded(Ok(status(vec![g])));
        let name = &rows(&l)[0].name;
        assert!(!name.contains('\u{1b}'), "{name:?}");
        assert!(
            name.chars().count() <= "A-1  ".len() + TITLE_CHARS + 1,
            "{name}"
        );
    }

    #[test]
    fn the_state_decides_whether_there_is_a_section() {
        let l = loaded(model::status(&fixture("demo/status.json")));
        let loaded_state = State::Loaded {
            loaded: &l,
            reloading: false,
            error: Some("a reload failed"),
        };
        assert!(matches!(home(&loaded_state), Home::Set(r) if r.len() == 5));
        for s in [State::Idle, State::Probing, State::Loading] {
            assert_eq!(home(&s), Home::Keep);
        }
        for s in [
            State::NotFound("x"),
            State::NotKiln("x"),
            State::NoProject("x"),
            State::Failed("x"),
            State::TooOld(version()),
        ] {
            assert_eq!(home(&s), Home::Clear);
        }
    }
}

/// Home's section through the extension, against test-kit's fake host. The
/// fake notes that a section was set or taken away, not its rows (those are
/// the pure tests' above).
#[cfg(test)]
mod wiring {
    use super::*;
    use crate::api::Event;
    use crate::model::tests::fixture;
    use crate::project::Why;
    use crate::tests::{demo, folder, harness};
    use fenix_test_kit::{Fake, Ran};

    fn homes(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.starts_with("home "))
            .collect()
    }

    #[test]
    fn set_once_and_again_only_when_it_changes() {
        let fake = demo();
        let dir = folder("home-set");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(homes(&fake), ["home set"]);
        // Read again, nothing new: not given again.
        h.command("kiln.reload").unwrap();
        h.ext.project.reload(Why::Timer);
        h.settle();
        assert_eq!(homes(&fake), ["home set"]);
        // The plan says something else: given again.
        fake.on_spawn("kiln status", Ran::ok(fixture("own/status.json")));
        h.command("kiln.reload").unwrap();
        assert_eq!(homes(&fake), ["home set", "home set"]);
        let told = h.ext.told.home.clone().unwrap().unwrap();
        assert_eq!(told.0, "SPC m o");
        assert_eq!(told.1.last().unwrap().value, PLAN);
        // The key scheme changed: the key it names is looked up again.
        fake.key("kiln.open", "SPC k o");
        h.key_scheme(crate::api::KeyScheme::Vim);
        assert_eq!(homes(&fake).len(), 3);
        assert_eq!(h.ext.told.home.clone().unwrap().unwrap().0, "SPC k o");
    }

    #[test]
    fn taken_away_when_the_project_is_not_a_kiln_project() {
        let fake = demo();
        let dir = folder("home-clear");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(homes(&fake), ["home set"]);
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, fixture("errors/no-kiln-toml.stderr")),
        );
        h.command("kiln.reload").unwrap();
        assert_eq!(homes(&fake), ["home set", "home -"]);
        // Said once, not on every read.
        h.command("kiln.reload").unwrap();
        assert_eq!(homes(&fake), ["home set", "home -"]);
        // Never a project: cleared at the start, once (what an earlier run
        // left is taken away).
        let fake = Fake::new();
        let mut h = harness(&fake, &folder("home-none"));
        h.activate().unwrap();
        assert_eq!(homes(&fake), ["home -"]);
    }

    #[test]
    fn picking_a_row_opens_the_plan_on_that_item_or_just_the_plan() {
        let fake = demo();
        let dir = folder("home-pick");
        let mut h = harness(&fake, &dir);
        h.size = (110, 26);
        h.activate().unwrap();
        assert!(fake.page("plan").is_none());
        h.event(Event::HomePicked("TASK-003".into()));
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), crate::pages::Tab::Item);
        assert!(
            h.screen(page).contains("TASK-003"),
            "the Item tab: {}",
            h.screen(page)
        );
        // The plan row: the page, wherever it was.
        h.ext
            .pages
            .get_mut(&page)
            .unwrap()
            .plan
            .show(crate::pages::Tab::Roadmap);
        h.event(Event::HomePicked(PLAN.into()));
        assert_eq!(h.ext.pages.len(), 1);
        assert_eq!(h.ext.pages[&page].plan.tab(), crate::pages::Tab::Roadmap);
        // An item gone since: said, not a panic.
        h.event(Event::HomePicked("TASK-999".into()));
        assert!(fake
            .notifications()
            .contains(&"Kiln: TASK-999 isn't in the project any more".to_string()));
    }

    #[test]
    fn the_plan_row_opens_the_plan_when_none_is_open() {
        let fake = demo();
        let mut h = harness(&fake, &folder("home-plan"));
        h.activate().unwrap();
        h.event(Event::HomePicked(PLAN.into()));
        assert_eq!(h.ext.pages.len(), 1);
    }
}
