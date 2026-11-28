//! "Start working on this": claim an item and start its clock, as one
//! step for the person and two for the extension.
//!
//! 1. **Kiln first.** What Kiln's own guidance says for claiming work --
//!    `kiln set ID owner=… status=…`, in one command -- through the same
//!    edit path every other edit takes ([`crate::edit::set_args`], the
//!    queue, [`crate::edit::blocked`] held by the caller). The status is the
//!    type's first one after its `initial` that is not done, closed,
//!    blocked or a sign-off ([`status_plan`]); an item already past its
//!    initial status keeps the one it has. The owner is set only for an
//!    item with none, and only when the setting `kiln.owner` says who the
//!    user is: the extension has no way to know that without a permission it
//!    doesn't hold (no git config, no environment), so empty means "don't
//!    claim". When `kiln.toml` can't be read, or the type has no such
//!    status, nothing is guessed: the status is left alone and the message
//!    says why.
//! 2. **Then the clock**, on the Agenda extension's task for the item,
//!    found or made ([`find`]):
//!    * through the item's Jira key, when it has one and an agenda task
//!      carries it (`agenda.tasks` gives `jiraKey` and `tracker`);
//!    * else through a task this extension made before. The agenda's
//!      `agenda.add_task` takes a title and an optional file and line
//!      (`{"title", "path", "line"}`) and answers nothing -- not the new
//!      task's id, no Jira key, no link -- and `agenda.tasks` doesn't give a
//!      task's file, so the only thing that tells the task back is its
//!      title, which starts with the item's ID (`TASK-004 Add the export
//!      button`). Kiln IDs are unique in a project but not across projects:
//!      the agenda is one list for every project, so another project's
//!      `TASK-004` would be found too. That is the tradeoff of a title; the
//!      file and line (the item's own file) go with the task, for the agenda
//!      to open, but can't be matched on;
//!    * else a task is made with `agenda.add_task`, the agenda asked for its
//!      tasks again to learn its id, and the clock is put on it with
//!      `agenda.clock_in`.
//!
//!    What clocking in does to a running clock is the agenda's: it closes
//!    the running one into a time entry on the task it ran on and starts
//!    the new one. This module never clocks anything out.
//!
//! The agenda is not required. When it isn't installed the first call fails
//! with Fenix's "nothing offers agenda.tasks" ([`not_installed`]), which is
//! said plainly in the message and is not an error: the status change
//! stands. When it refuses or answers something unreadable, what it said is
//! shown, and the status change stands too. A refusal from kiln stops
//! everything: no clock.
//!
//! [`Run`] is the state machine of those steps: pure, it holds no host and
//! calls nothing. The caller ([`crate::Kiln`]) does each [`Do`] it hands
//! back and gives the answer to the next method, so the whole thing is
//! tested as tables of answers.
//!
//! What it deliberately doesn't do: read git config or the environment to
//! guess an owner, stop or edit an agenda task, create a Jira key, write
//! anything itself, or ask the agenda for the running clock first (clocking
//! in on the task already on the clock closes that sitting and starts
//! another, which loses nothing).

use serde::Deserialize;

use crate::config::{Config, Type};
use crate::kiln::{Failure, Ran};
use crate::model::{self, Item};

/// The setting that says who the user is, for the owner of an item they
/// start: text, empty by default (don't set an owner).
pub const OWNER_SETTING: &str = "kiln.owner";

/// The agenda's services, as `extensions/agenda` serves them.
pub const TASKS: &str = "agenda.tasks";
pub const ADD_TASK: &str = "agenda.add_task";
pub const CLOCK_IN: &str = "agenda.clock_in";

/// Jira's tracker id in the agenda (`fenix.jira`): an agenda task's
/// `tracker` when its `jiraKey` is a Jira ticket's.
const JIRA_TRACKER: &str = "fenix.jira";

/// The longest title, in characters, made for the agenda and said in the
/// message.
const TITLE_CHARS: usize = 120;

/// What to do with an item's status when it is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusPlan {
    /// Set it to this.
    Set(String),
    /// Leave it: the item is already past its initial status.
    Keep,
    /// Nothing is guessed: why no status can be chosen.
    Unknown(String),
}

/// The status "start" moves `item` to, from `kiln.toml`.
pub fn status_plan(item: &Item, config: Result<&Config, &str>) -> StatusPlan {
    let config = match config {
        Ok(c) => c,
        Err(why) => {
            return StatusPlan::Unknown(format!(
                "kiln.toml can't be read ({})",
                model::plain(why, 120)
            ))
        }
    };
    let Some(kind) = config.kind(&item.kind) else {
        return StatusPlan::Unknown(format!("kiln.toml has no type {}", item.kind));
    };
    if item.status != kind.initial {
        return StatusPlan::Keep;
    }
    match in_progress(config, kind) {
        Some(status) => StatusPlan::Set(status.to_string()),
        None => StatusPlan::Unknown(format!(
            "type {} has no status after {} that isn't done, closed, blocked or a sign-off",
            kind.name, kind.initial
        )),
    }
}

/// The first status of `kind` after its initial one that is neither done,
/// closed, `blocked` nor a sign-off (a person's call: kiln refuses an
/// agent's, and "start" shouldn't be one that needs a person).
fn in_progress<'a>(config: &Config, kind: &'a Type) -> Option<&'a str> {
    let after = kind.position(&kind.initial)? + 1;
    let signoff = config.signoff_statuses(&kind.name);
    kind.statuses
        .iter()
        .skip(after)
        .map(String::as_str)
        .find(|s| {
            *s != "blocked"
                && !kind.done.iter().any(|d| d == s)
                && !kind.closed.iter().any(|c| c == s)
                && !signoff.contains(s)
        })
}

/// Whether `kind` is work that can be started: it has a done status, and
/// isn't a milestone -- the Board's own rule of thumb for work
/// ([`crate::pages::board`]), from `kiln.toml` where the Board can only
/// look at items.
fn is_work(kind: &Type) -> bool {
    kind.name != "milestone" && !kind.done.is_empty()
}

/// The owner to set, when there is one: the setting is a name (nothing in it
/// a person couldn't have typed in a prompt) and the item has no owner.
pub fn owner_plan(item: &Item, setting: &str) -> Option<String> {
    let name = setting.trim();
    let clean = !name.is_empty() && !name.chars().any(char::is_control);
    let unowned = item.owner.as_deref().is_none_or(|o| o.trim().is_empty());
    (clean && unowned).then(|| name.to_string())
}

/// One task of the agenda, as `agenda.tasks` gives it (the control
/// protocol's `AgendaTask`, `extensions/agenda/src/lib.rs`
/// `control_tasks`): `{"id", "title", "status", "elapsedMinutes",
/// "jiraKey"?, "tracker"?}`, `status` one of `todo`, `inProgress`,
/// `blocked`, `done`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgendaTask {
    pub id: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "jiraKey")]
    pub jira_key: Option<String>,
    #[serde(default)]
    pub tracker: Option<String>,
}

/// `agenda.tasks`' answer: `{"tasks": [...]}`, or why it can't be read.
pub fn tasks(text: &str) -> Result<Vec<AgendaTask>, String> {
    #[derive(Deserialize)]
    struct Answer {
        tasks: Vec<AgendaTask>,
    }
    serde_json::from_str::<Answer>(text)
        .map(|a| a.tasks)
        .map_err(|e| format!("{e}: {}", model::plain(text, 80)))
}

/// How the agenda's task for an item was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found<'a> {
    /// By one of the item's Jira keys.
    ByJira(&'a AgendaTask),
    /// By a title that starts with the item's ID: one made for it before.
    ByTitle(&'a AgendaTask),
}

impl<'a> Found<'a> {
    pub fn task(&self) -> &'a AgendaTask {
        match self {
            Found::ByJira(t) | Found::ByTitle(t) => t,
        }
    }
}

/// The agenda's task for item `id` with Jira keys `jira`, among `tasks`:
/// an open one carrying one of the keys (a Jira task: its `tracker` is
/// Jira's or not given), else an open one whose title is the ID or starts
/// with the ID and a space. A done task isn't one to clock in on again: a
/// new one is made, and the old one keeps its time. The first in the
/// agenda's order wins among equals.
pub fn find<'a>(tasks: &'a [AgendaTask], id: &str, jira: &[String]) -> Option<Found<'a>> {
    let open = || tasks.iter().filter(|t| t.status != "done");
    let by_key = open().find(|t| {
        let ours = t.tracker.as_deref().is_none_or(|tr| tr == JIRA_TRACKER);
        ours && t
            .jira_key
            .as_deref()
            .is_some_and(|k| jira.iter().any(|j| j.eq_ignore_ascii_case(k)))
    });
    if let Some(t) = by_key {
        return Some(Found::ByJira(t));
    }
    let with_id = format!("{id} ");
    open()
        .find(|t| t.title == id || t.title.starts_with(&with_id))
        .map(Found::ByTitle)
}

/// Whether the agenda's answer to a call says no extension serves it: Fenix's
/// "nothing offers agenda.tasks", the refusal of a name left out
/// (`agenda.tasks: … -- refused to …`), or an extension's own "no service".
/// Anything else is the agenda answering, to be said as it said it.
pub fn not_installed(why: &str) -> bool {
    why.starts_with("nothing offers ")
        || why.contains(" -- refused to ")
        || why.starts_with("no service ")
}

/// What the caller is to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// kiln, with these arguments after `kiln`, through the edit path; the
    /// outcome to [`Run::kiln_done`].
    Kiln(Vec<String>),
    /// A service of the agenda; its reply to [`Run::replied`].
    Service { name: &'static str, payload: String },
    /// Over, with this line. `bad`: said as an error.
    Say { text: String, bad: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Kiln,
    Listing,
    Adding,
    Relisting,
    Clocking(String),
    Over,
}

/// One start under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    id: String,
    /// The agenda's title for the item: `ID Title`.
    title: String,
    jira: Vec<String>,
    /// The item's file, absolute, and its line, for the agenda's link back.
    code: Option<(String, u32)>,
    /// What was said of the item so far: kiln's line, or the run's own
    /// when kiln had nothing to change.
    line: String,
    stage: Stage,
}

impl Run {
    /// A start of `item`: the run, when there is something to do, and the
    /// first [`Do`]. `owner` is the setting [`OWNER_SETTING`]; `root` the
    /// project's folder as Fenix names it, when known (the file the agenda
    /// links back to is under it).
    pub fn begin(
        item: &Item,
        config: Result<&Config, &str>,
        owner: &str,
        root: Option<&str>,
    ) -> (Option<Run>, Do) {
        let id = item.id.as_str();
        let say = |text: String| (None, Do::Say { text, bad: false });
        if item.done || item.closed {
            let what = if item.done { "done" } else { "closed" };
            return say(format!(
                "{id} is already {what} ({}): nothing to start",
                item.status
            ));
        }
        if let Some(kind) = config.ok().and_then(|c| c.kind(&item.kind)) {
            if !is_work(kind) {
                return say(format!(
                    "{id} isn't work to start: type {} has no done status",
                    kind.name
                ));
            }
        }
        let plan = status_plan(item, config);
        let owner = owner_plan(item, owner);
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if let Some(o) = &owner {
            pairs.push(("owner", o));
        }
        if let StatusPlan::Set(s) = &plan {
            pairs.push(("status", s));
        }
        let note = match &plan {
            StatusPlan::Unknown(why) => format!("status not changed: {why}"),
            StatusPlan::Keep => format!("already {}", item.status),
            StatusPlan::Set(_) => String::new(),
        };
        let mut run = Run {
            id: id.to_string(),
            title: format!("{id} {}", model::plain(&item.title, TITLE_CHARS)),
            jira: item.jira_keys.clone(),
            code: root.filter(|r| !r.is_empty()).map(|r| {
                (
                    format!("{}/{}", r.trim_end_matches(['/', '\\']), item.path),
                    item.line.max(1),
                )
            }),
            line: String::new(),
            stage: Stage::Kiln,
        };
        if pairs.is_empty() {
            // Nothing for kiln to change: on to the clock.
            run.line = format!("{id}: {note}");
            run.stage = Stage::Listing;
            return (Some(run), tasks_step());
        }
        let args = crate::edit::set_args(id, &pairs);
        // A note about the status goes after kiln's line, once it comes.
        run.line = note;
        (Some(run), Do::Kiln(args))
    }

    /// What was said so far: kiln's line, then the run's own words.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The item's ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How kiln's `set` went. A refusal ends the run with kiln's line --
    /// no clock. Otherwise kiln's line leads and the agenda is asked.
    pub fn kiln_done(&mut self, result: &Result<Ran, Failure>) -> Do {
        let (line, role) = crate::pages::frame::outcome(result);
        if role == fenix_ui_kit::Role::Bad {
            self.stage = Stage::Over;
            return Do::Say {
                text: line,
                bad: true,
            };
        }
        let note = std::mem::take(&mut self.line);
        self.line = if note.is_empty() {
            line
        } else {
            format!("{line}; {note}")
        };
        self.stage = Stage::Listing;
        tasks_step()
    }

    /// The agenda's reply to the last [`Do::Service`].
    pub fn replied(&mut self, reply: Result<String, String>) -> Do {
        let stage = std::mem::replace(&mut self.stage, Stage::Over);
        let over = |line: &str, why: String| Do::Say {
            text: format!("{line}; {why}"),
            bad: true,
        };
        let line = self.line.clone();
        match (stage, reply) {
            (Stage::Listing, Err(why)) if not_installed(&why) => Do::Say {
                text: format!("{line}; no clock: the Agenda extension isn't installed"),
                bad: false,
            },
            (Stage::Listing | Stage::Relisting, Err(why)) => over(
                &line,
                format!(
                    "the agenda didn't list its tasks: {}",
                    model::plain(&why, 160)
                ),
            ),
            (stage @ (Stage::Listing | Stage::Relisting), Ok(text)) => {
                let list = match tasks(&text) {
                    Ok(l) => l,
                    Err(why) => {
                        return over(&line, format!("the agenda's tasks can't be read: {why}"))
                    }
                };
                match find(&list, &self.id, &self.jira) {
                    Some(found) => {
                        let task = found.task();
                        self.stage = Stage::Clocking(task.title.clone());
                        Do::Service {
                            name: CLOCK_IN,
                            payload: serde_json::json!({ "task_id": task.id }).to_string(),
                        }
                    }
                    None if stage == Stage::Listing => {
                        self.stage = Stage::Adding;
                        Do::Service {
                            name: ADD_TASK,
                            payload: self.add_payload(),
                        }
                    }
                    None => over(
                        &line,
                        "the agenda made a task but doesn't list it to clock in on".into(),
                    ),
                }
            }
            (Stage::Adding, Ok(_)) => {
                self.stage = Stage::Relisting;
                tasks_step()
            }
            (Stage::Adding, Err(why)) => over(
                &line,
                format!(
                    "the agenda wouldn't add a task: {}",
                    model::plain(&why, 160)
                ),
            ),
            (Stage::Clocking(title), Ok(_)) => Do::Say {
                text: format!("{line}; clocked in on \"{title}\""),
                bad: false,
            },
            (Stage::Clocking(_), Err(why)) => over(
                &line,
                format!("the agenda didn't clock in: {}", model::plain(&why, 160)),
            ),
            // An answer to nothing asked.
            (Stage::Kiln | Stage::Over, _) => Do::Say {
                text: line,
                bad: false,
            },
        }
    }

    /// `agenda.add_task`'s payload: the title, and the item's file and line
    /// when the project's folder is known.
    fn add_payload(&self) -> String {
        match &self.code {
            Some((path, line)) => {
                serde_json::json!({ "title": self.title, "path": path, "line": line })
            }
            None => serde_json::json!({ "title": self.title }),
        }
        .to_string()
    }
}

/// `agenda.tasks`, asked for every task (the agenda leaves out archived
/// ones itself; a done one is skipped by [`find`]).
fn tasks_step() -> Do {
    Do::Service {
        name: TASKS,
        payload: "{}".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::model::tests::fixture;

    fn demo_config() -> Config {
        config::parse(&fixture("exports/kiln.toml")).unwrap()
    }

    /// A configuration with one type, `t`, as written.
    fn with(body: &str) -> Config {
        config::parse(&format!(
            "format = 1\nname = \"T\"\n[types.t]\nprefix = \"T\"\n{body}\n"
        ))
        .unwrap()
    }

    fn item(kind: &str, id: &str, status: &str) -> Item {
        Item {
            id: id.into(),
            kind: kind.into(),
            title: "Add the export button".into(),
            status: status.into(),
            parent: None,
            tags: Vec::new(),
            jira_keys: Vec::new(),
            owner: None,
            due: None,
            done: false,
            closed: false,
            links: Default::default(),
            path: format!("design/tasks/{id}-add-the-export-button.md"),
            line: 2,
            body: None,
        }
    }

    #[test]
    fn the_status_start_picks_for_each_shape_of_type() {
        let cases: &[(&str, &str, StatusPlan)] = &[
            // The common shape: todo, doing, blocked, done, dropped.
            (
                "statuses = [\"todo\", \"doing\", \"blocked\", \"done\", \"dropped\"]\ninitial = \"todo\"\ndone = [\"done\"]\nclosed = [\"dropped\"]",
                "todo",
                StatusPlan::Set("doing".into()),
            ),
            // Blocked first, then the real one.
            (
                "statuses = [\"todo\", \"blocked\", \"review\", \"done\"]\ninitial = \"todo\"\ndone = [\"done\"]",
                "todo",
                StatusPlan::Set("review".into()),
            ),
            // The statuses before the initial one don't count.
            (
                "statuses = [\"idea\", \"todo\", \"doing\", \"done\"]\ninitial = \"todo\"\ndone = [\"done\"]",
                "todo",
                StatusPlan::Set("doing".into()),
            ),
            // A sign-off is a person's call, skipped.
            (
                "statuses = [\"open\", \"signing\", \"going\", \"done\"]\ninitial = \"open\"\ndone = [\"done\"]\n[signoff]\nt = [\"signing\"]",
                "open",
                StatusPlan::Set("going".into()),
            ),
            // Nothing but done, closed or blocked after the initial one.
            (
                "statuses = [\"todo\", \"blocked\", \"done\", \"dropped\"]\ninitial = \"todo\"\ndone = [\"done\"]\nclosed = [\"dropped\"]",
                "todo",
                StatusPlan::Unknown("type t has no status after todo that isn't done, closed, blocked or a sign-off".into()),
            ),
            // Only the initial status.
            (
                "statuses = [\"todo\"]\ninitial = \"todo\"\ndone = [\"todo\"]",
                "todo",
                StatusPlan::Unknown("type t has no status after todo that isn't done, closed, blocked or a sign-off".into()),
            ),
            // Already started: left alone, whatever it is.
            (
                "statuses = [\"todo\", \"doing\", \"review\", \"done\"]\ninitial = \"todo\"\ndone = [\"done\"]",
                "doing",
                StatusPlan::Keep,
            ),
            (
                "statuses = [\"todo\", \"doing\", \"blocked\", \"done\"]\ninitial = \"todo\"\ndone = [\"done\"]",
                "blocked",
                StatusPlan::Keep,
            ),
        ];
        for (body, now, want) in cases {
            let c = with(body);
            assert_eq!(
                status_plan(&item("t", "T-1", now), Ok(&c)),
                *want,
                "{body} at {now}"
            );
        }
    }

    #[test]
    fn no_configuration_and_no_such_type_guess_nothing() {
        let i = item("task", "TASK-004", "todo");
        let StatusPlan::Unknown(why) =
            status_plan(&i, Err("there's no kiln.toml at the project's top"))
        else {
            panic!("guessed")
        };
        assert_eq!(
            why,
            "kiln.toml can't be read (there's no kiln.toml at the project's top)"
        );
        let c = demo_config();
        assert_eq!(
            status_plan(&item("story", "S-1", "todo"), Ok(&c)),
            StatusPlan::Unknown("kiln.toml has no type story".into())
        );
        assert_eq!(
            status_plan(&i, Ok(&c)),
            StatusPlan::Set("doing".into()),
            "the demo's task"
        );
    }

    #[test]
    fn the_owner_is_set_only_for_an_unowned_item_and_a_named_user() {
        let mut i = item("task", "TASK-004", "todo");
        assert_eq!(owner_plan(&i, "ana"), Some("ana".into()));
        assert_eq!(owner_plan(&i, "  ana  "), Some("ana".into()));
        assert_eq!(owner_plan(&i, ""), None, "empty setting: no owner");
        assert_eq!(owner_plan(&i, "   "), None);
        assert_eq!(
            owner_plan(&i, "a\nb"),
            None,
            "nothing a prompt couldn't carry"
        );
        i.owner = Some("bo".into());
        assert_eq!(owner_plan(&i, "ana"), None, "claimed already");
        i.owner = Some(" ".into());
        assert_eq!(
            owner_plan(&i, "ana"),
            Some("ana".into()),
            "a blank one isn't"
        );
    }

    fn task(id: u32, title: &str, status: &str, key: Option<(&str, &str)>) -> AgendaTask {
        AgendaTask {
            id,
            title: title.into(),
            status: status.into(),
            jira_key: key.map(|(k, _)| k.into()),
            tracker: key.map(|(_, t)| t.into()),
        }
    }

    fn found(list: &[AgendaTask], id: &str, jira: &[&str]) -> Option<(u32, &'static str)> {
        let jira: Vec<String> = jira.iter().map(|j| j.to_string()).collect();
        find(list, id, &jira).map(|f| {
            (
                f.task().id,
                match f {
                    Found::ByJira(_) => "jira",
                    Found::ByTitle(_) => "title",
                },
            )
        })
    }

    #[test]
    fn the_agenda_task_is_found_by_jira_key_then_by_title() {
        let list = vec![
            task(1, "Something else", "todo", None),
            task(
                2,
                "Fix the export",
                "inProgress",
                Some(("FLINT-12", "fenix.jira")),
            ),
            task(3, "TASK-004 Add the export button", "todo", None),
            task(4, "TASK-0040 Another", "todo", None),
            task(5, "TASK-004", "todo", None),
        ];
        // By key, even when a title would match too.
        assert_eq!(found(&list, "TASK-004", &["FLINT-12"]), Some((2, "jira")));
        assert_eq!(found(&list, "TASK-004", &["flint-12"]), Some((2, "jira")));
        // By title: the ID and a space, or the ID alone; not a longer ID.
        assert_eq!(found(&list, "TASK-004", &[]), Some((3, "title")));
        assert_eq!(found(&list, "TASK-004", &["FLINT-99"]), Some((3, "title")));
        assert_eq!(found(&list[3..4], "TASK-004", &[]), None);
        assert_eq!(found(&list[4..], "TASK-004", &[]), Some((5, "title")));
        // None.
        assert_eq!(found(&list, "TASK-009", &[]), None);
        assert_eq!(found(&[], "TASK-004", &["FLINT-12"]), None);
    }

    #[test]
    fn a_done_task_or_another_trackers_key_is_not_the_one() {
        let list = vec![
            task(
                1,
                "Fix the export",
                "done",
                Some(("FLINT-12", "fenix.jira")),
            ),
            task(2, "TASK-004 Add the export button", "done", None),
            task(
                3,
                "Their ticket",
                "todo",
                Some(("FLINT-12", "fenix.linear")),
            ),
        ];
        assert_eq!(found(&list, "TASK-004", &["FLINT-12"]), None);
        // Without a tracker named, the key is taken as Jira's (an older
        // agenda).
        let mut older = task(4, "Fix it", "todo", Some(("FLINT-12", "x")));
        older.tracker = None;
        assert_eq!(
            found(&[older], "TASK-004", &["FLINT-12"]),
            Some((4, "jira"))
        );
        // An open one after a done one.
        let list = vec![
            task(1, "TASK-004 Add the export button", "done", None),
            task(2, "TASK-004 Add the export button", "inProgress", None),
        ];
        assert_eq!(found(&list, "TASK-004", &[]), Some((2, "title")));
    }

    #[test]
    fn the_agendas_tasks_are_read_as_it_gives_them() {
        let text = r#"{"tasks":[{"id":3,"title":"TASK-004 Add","status":"todo","elapsedMinutes":0},
            {"id":9,"title":"Fix","status":"inProgress","elapsedMinutes":12,"jiraKey":"FLINT-12","tracker":"fenix.jira"}]}"#;
        let t = tasks(text).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].jira_key.as_deref(), Some("FLINT-12"));
        assert_eq!(t[0].jira_key, None);
        assert!(tasks("[]").is_err());
        assert!(tasks("not json").unwrap_err().contains("not json"));
        assert!(tasks(r#"{"tasks":[{"title":"no id"}]}"#).is_err());
    }

    #[test]
    fn what_an_absent_agenda_says_is_told_from_what_it_says() {
        assert!(not_installed("nothing offers agenda.tasks"));
        assert!(not_installed(
            "agenda.tasks: only fenix.agenda may serve it -- refused to acme.agenda"
        ));
        assert!(not_installed("no service agenda.tasks"));
        assert!(!not_installed("no agenda task 7"));
        assert!(!not_installed("an agenda call: missing field `title`"));
        assert!(!not_installed(""));
    }

    fn started(item: &Item, owner: &str, root: Option<&str>) -> (Run, Do) {
        let c = demo_config();
        let (run, step) = Run::begin(item, Ok(&c), owner, root);
        (run.expect("a run"), step)
    }

    fn ran(stdout: &str) -> Result<Ran, Failure> {
        Ok(Ran {
            code: Some(0),
            stdout: stdout.into(),
            stderr: Vec::new(),
        })
    }

    fn say(step: Do) -> (String, bool) {
        match step {
            Do::Say { text, bad } => (text, bad),
            other => panic!("not the end: {other:?}"),
        }
    }

    fn service(step: Do) -> (&'static str, serde_json::Value) {
        match step {
            Do::Service { name, payload } => (name, serde_json::from_str(&payload).unwrap()),
            other => panic!("not a service: {other:?}"),
        }
    }

    #[test]
    fn begin_asks_kiln_for_owner_and_status_in_one_command() {
        let i = item("task", "TASK-004", "todo");
        let (_, step) = started(&i, "ana", None);
        assert_eq!(
            step,
            Do::Kiln(
                ["set", "--", "TASK-004", "owner=ana", "status=doing"]
                    .map(String::from)
                    .to_vec()
            )
        );
        // No owner setting: only the status.
        let (_, step) = started(&i, "", None);
        assert_eq!(
            step,
            Do::Kiln(
                ["set", "--", "TASK-004", "status=doing"]
                    .map(String::from)
                    .to_vec()
            )
        );
        // A value that looks like an option rides inside its argument.
        let (_, step) = started(&i, "--help", None);
        let Do::Kiln(args) = step else { panic!() };
        assert_eq!(args[..4], ["set", "--", "TASK-004", "owner=--help"]);
    }

    #[test]
    fn an_item_already_started_and_owned_goes_straight_to_the_clock() {
        let mut i = item("task", "TASK-004", "doing");
        i.owner = Some("bo".into());
        let (run, step) = started(&i, "ana", None);
        assert_eq!(service(step), (TASKS, serde_json::json!({})));
        assert_eq!(run.line(), "TASK-004: already doing");
        // Started, unowned, with the setting: kiln claims it, status kept.
        i.owner = None;
        let (mut run, step) = started(&i, "ana", None);
        assert_eq!(
            step,
            Do::Kiln(
                ["set", "--", "TASK-004", "owner=ana"]
                    .map(String::from)
                    .to_vec()
            )
        );
        let step = run.kiln_done(&ran("TASK-004: owner → ana\n"));
        assert_eq!(service(step).0, TASKS);
        assert_eq!(run.line(), "TASK-004: owner → ana; already doing");
    }

    #[test]
    fn items_that_are_not_work_or_are_finished_start_nothing() {
        let c = demo_config();
        let mut done = item("task", "TASK-001", "done");
        done.done = true;
        let (run, step) = Run::begin(&done, Ok(&c), "ana", None);
        assert!(run.is_none());
        assert_eq!(
            say(step),
            (
                "TASK-001 is already done (done): nothing to start".into(),
                false
            )
        );
        let mut dropped = item("task", "TASK-001", "dropped");
        dropped.closed = true;
        assert!(say(Run::begin(&dropped, Ok(&c), "", None).1)
            .0
            .contains("already closed"));
        let (run, step) = Run::begin(&item("decision", "ADR-001", "proposed"), Ok(&c), "", None);
        assert!(run.is_none());
        assert_eq!(
            say(step).0,
            "ADR-001 isn't work to start: type decision has no done status"
        );
        let (run, _) = Run::begin(&item("milestone", "MS-001", "planned"), Ok(&c), "", None);
        assert!(run.is_none());
    }

    #[test]
    fn an_unreadable_configuration_changes_no_status_but_goes_on_to_the_clock() {
        let i = item("task", "TASK-004", "todo");
        let (run, step) = Run::begin(&i, Err("kiln.toml isn't valid TOML"), "", None);
        let mut run = run.unwrap();
        assert_eq!(service(step).0, TASKS);
        assert_eq!(
            run.line(),
            "TASK-004: status not changed: kiln.toml can't be read (kiln.toml isn't valid TOML)"
        );
        // With an owner to set, kiln runs, the status note after its line.
        let (run2, step) = Run::begin(&i, Err("x"), "ana", None);
        assert_eq!(
            step,
            Do::Kiln(
                ["set", "--", "TASK-004", "owner=ana"]
                    .map(String::from)
                    .to_vec()
            )
        );
        let mut run2 = run2.unwrap();
        run2.kiln_done(&ran("TASK-004: owner → ana\n"));
        assert_eq!(
            run2.line(),
            "TASK-004: owner → ana; status not changed: kiln.toml can't be read (x)"
        );
        // And the run still finishes.
        let (text, bad) = say(run.replied(Err("nothing offers agenda.tasks".into())));
        assert!(!bad);
        assert!(
            text.ends_with("; no clock: the Agenda extension isn't installed"),
            "{text}"
        );
    }

    #[test]
    fn a_refusal_from_kiln_ends_it_without_the_clock() {
        let i = item("task", "TASK-004", "todo");
        let (mut run, _) = started(&i, "ana", None);
        let refused = Ok(Ran {
            code: Some(1),
            stdout: String::new(),
            stderr: vec![
                "kiln: design/tasks/TASK-004.md changed on disk since it was read; run the command again"
                    .into(),
            ],
        });
        let (text, bad) = say(run.kiln_done(&refused));
        assert!(bad);
        assert!(
            text.starts_with("kiln: design/tasks/TASK-004.md changed on disk"),
            "{text}"
        );
        // Nothing is asked after it.
        assert_eq!(say(run.replied(Ok("{\"tasks\":[]}".into()))).0, "");
    }

    /// A start of TASK-004 with kiln's edit done.
    fn after_kiln(i: &Item, root: Option<&str>) -> Run {
        let (mut run, _) = started(i, "", root);
        let step = run.kiln_done(&ran("TASK-004: status todo → doing\n"));
        assert_eq!(service(step), (TASKS, serde_json::json!({})));
        run
    }

    #[test]
    fn the_whole_run_on_a_task_found_by_its_jira_key() {
        let mut i = item("task", "TASK-004", "todo");
        i.jira_keys = vec!["FLINT-12".into()];
        let mut run = after_kiln(&i, None);
        let list = r#"{"tasks":[{"id":7,"title":"Fix the export","status":"todo","elapsedMinutes":0,"jiraKey":"FLINT-12","tracker":"fenix.jira"}]}"#;
        let step = run.replied(Ok(list.into()));
        assert_eq!(service(step), (CLOCK_IN, serde_json::json!({"task_id": 7})));
        let (text, bad) = say(run.replied(Ok(String::new())));
        assert!(!bad);
        assert_eq!(
            text,
            "TASK-004: status todo → doing; clocked in on \"Fix the export\""
        );
    }

    #[test]
    fn the_whole_run_on_a_task_made_before() {
        let i = item("task", "TASK-004", "todo");
        let mut run = after_kiln(&i, None);
        let list = r#"{"tasks":[{"id":3,"title":"TASK-004 Add the export button","status":"inProgress","elapsedMinutes":5}]}"#;
        let step = run.replied(Ok(list.into()));
        assert_eq!(service(step), (CLOCK_IN, serde_json::json!({"task_id": 3})));
        assert_eq!(
            say(run.replied(Ok(String::new()))).0,
            "TASK-004: status todo → doing; clocked in on \"TASK-004 Add the export button\""
        );
    }

    #[test]
    fn the_whole_run_making_the_task() {
        let i = item("task", "TASK-004", "todo");
        let mut run = after_kiln(&i, Some("/home/me/p"));
        let step = run.replied(Ok(r#"{"tasks":[]}"#.into()));
        // The title, and the item's file and line to come back to.
        assert_eq!(
            service(step),
            (
                ADD_TASK,
                serde_json::json!({
                    "title": "TASK-004 Add the export button",
                    "path": "/home/me/p/design/tasks/TASK-004-add-the-export-button.md",
                    "line": 2
                })
            )
        );
        // It answers nothing: asked again for its id.
        let step = run.replied(Ok(String::new()));
        assert_eq!(service(step), (TASKS, serde_json::json!({})));
        let list = r#"{"tasks":[{"id":12,"title":"TASK-004 Add the export button","status":"todo","elapsedMinutes":0}]}"#;
        let step = run.replied(Ok(list.into()));
        assert_eq!(
            service(step),
            (CLOCK_IN, serde_json::json!({"task_id": 12}))
        );
        assert_eq!(
            say(run.replied(Ok(String::new()))).0,
            "TASK-004: status todo → doing; clocked in on \"TASK-004 Add the export button\""
        );
    }

    #[test]
    fn the_made_task_without_a_known_folder_links_to_no_file() {
        let i = item("task", "TASK-004", "todo");
        let mut run = after_kiln(&i, None);
        let step = run.replied(Ok(r#"{"tasks":[]}"#.into()));
        assert_eq!(
            service(step),
            (
                ADD_TASK,
                serde_json::json!({"title": "TASK-004 Add the export button"})
            )
        );
    }

    #[test]
    fn a_title_with_control_characters_is_made_clean() {
        let mut i = item("task", "TASK-004", "todo");
        i.title = "Add\u{1b}[31m the\nbutton".into();
        let mut run = after_kiln(&i, None);
        let (_, payload) = service(run.replied(Ok(r#"{"tasks":[]}"#.into())));
        let title = payload["title"].as_str().unwrap();
        assert!(!title.chars().any(char::is_control), "{title:?}");
        assert!(title.starts_with("TASK-004 Add"), "{title:?}");
    }

    #[test]
    fn without_the_agenda_it_says_so_plainly_and_is_not_an_error() {
        for why in [
            "nothing offers agenda.tasks",
            "agenda.tasks: only fenix.agenda may serve it -- refused to acme.agenda",
        ] {
            let i = item("task", "TASK-004", "todo");
            let mut run = after_kiln(&i, None);
            let (text, bad) = say(run.replied(Err(why.into())));
            assert!(!bad, "{why}");
            assert_eq!(
                text,
                "TASK-004: status todo → doing; no clock: the Agenda extension isn't installed"
            );
        }
    }

    #[test]
    fn when_the_agenda_refuses_or_is_unreadable_what_it_said_is_said() {
        let i = item("task", "TASK-004", "todo");
        let prefix = "TASK-004: status todo → doing; ";
        // Its list can't be asked.
        let mut run = after_kiln(&i, None);
        let (text, bad) = say(run.replied(Err("the agenda is busy".into())));
        assert!(bad);
        assert_eq!(
            text,
            format!("{prefix}the agenda didn't list its tasks: the agenda is busy")
        );
        // Its list is something else.
        let mut run = after_kiln(&i, None);
        let (text, bad) = say(run.replied(Ok("<html>nope</html>".into())));
        assert!(bad);
        assert!(
            text.starts_with(&format!("{prefix}the agenda's tasks can't be read: ")),
            "{text}"
        );
        assert!(text.contains("<html>nope</html>"), "{text}");
        // It won't add a task.
        let mut run = after_kiln(&i, None);
        run.replied(Ok(r#"{"tasks":[]}"#.into()));
        let (text, bad) = say(run.replied(Err("an agenda call: missing field `title`".into())));
        assert!(bad);
        assert_eq!(
            text,
            format!(
                "{prefix}the agenda wouldn't add a task: an agenda call: missing field `title`"
            )
        );
        // It made it, but can't find it.
        let mut run = after_kiln(&i, None);
        run.replied(Ok(r#"{"tasks":[]}"#.into()));
        run.replied(Ok(String::new()));
        let (text, bad) = say(run.replied(Ok(r#"{"tasks":[]}"#.into())));
        assert!(bad);
        assert_eq!(
            text,
            format!("{prefix}the agenda made a task but doesn't list it to clock in on")
        );
        // It won't clock in.
        let mut run = after_kiln(&i, None);
        run.replied(Ok(
            r#"{"tasks":[{"id":3,"title":"TASK-004 x","status":"todo"}]}"#.into(),
        ));
        let (text, bad) = say(run.replied(Err("no agenda task 3".into())));
        assert!(bad);
        assert_eq!(
            text,
            format!("{prefix}the agenda didn't clock in: no agenda task 3")
        );
        // Not installed is only the first answer's shape: later it is a refusal.
        let mut run = after_kiln(&i, None);
        run.replied(Ok(r#"{"tasks":[]}"#.into()));
        let (text, bad) = say(run.replied(Err("nothing offers agenda.add_task".into())));
        assert!(
            bad && text.contains("wouldn't add a task: nothing offers"),
            "{text}"
        );
    }

    #[test]
    fn what_the_agenda_says_is_cleaned_and_cut() {
        let i = item("task", "TASK-004", "todo");
        let mut run = after_kiln(&i, None);
        let (text, _) = say(run.replied(Err(format!("bad\u{1b}[31m{}", "x".repeat(500)))));
        assert!(
            !text.contains('\u{1b}') && text.chars().count() < 300,
            "{text}"
        );
    }
}

/// The whole start through the extension, against test-kit's fake host: its
/// kiln, and its answers for the agenda's services.
#[cfg(test)]
mod wiring {
    use super::*;
    use crate::api::Key;
    use crate::tests::{at, demo, folder, foot, harness};
    use crate::Kiln;
    use fenix_test_kit::{Fake, Harness, Ran};
    use std::cell::Cell;
    use std::rc::Rc;

    const SET: &str = "kiln set -- TASK-004 status=doing";
    const MADE: &str = "TASK-004 Add the export button";

    fn kiln_says(fake: &Fake) {
        fake.on_spawn(SET, Ran::ok("TASK-004: status todo → doing\n"));
    }

    /// The demo opened on the plan, its `kiln.toml` at the project's top.
    fn opened(fake: &Fake, name: &str) -> (Harness<Kiln>, u64) {
        let dir = folder(name);
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo/kiln.toml"),
            dir.join("kiln.toml"),
        )
        .unwrap();
        let mut h = harness(fake, &dir);
        h.size = (110, 26);
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        (h, page)
    }

    /// The extension in a folder of its own with the demo's `kiln.toml`, no
    /// page opened: for a start from a buffer.
    fn bare(fake: &Fake, name: &str) -> Harness<Kiln> {
        let dir = folder(name);
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo/kiln.toml"),
            dir.join("kiln.toml"),
        )
        .unwrap();
        harness(fake, &dir)
    }

    /// In the file of TASK-004, the cursor off any ID.
    fn in_buffer(fake: &Fake) {
        fake.project("demo", "/home/me/demo").in_file(
            "/home/me/demo/design/tasks/TASK-004-add-the-export-button.md",
            "md",
            "Some prose, no ID.\n",
        );
    }

    fn on(h: &mut Harness<Kiln>, page: u64, id: &str) {
        h.keys(page, &format!("/{id}<Enter>/<Esc>"));
        assert_eq!(at(h, page).as_deref(), Some(id));
    }

    fn sets(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter_map(|c| c.strip_prefix("spawn ").map(str::to_string))
            .filter(|c| c.contains("/usr/bin/kiln set "))
            .collect()
    }

    /// The agenda's calls so far: `name payload`.
    fn asked(fake: &Fake) -> Vec<(String, String)> {
        fake.services()
            .into_iter()
            .map(|s| (s.name, s.payload))
            .collect()
    }

    fn names(fake: &Fake) -> Vec<String> {
        asked(fake).into_iter().map(|(n, _)| n).collect()
    }

    /// An agenda that has `tasks` (the JSON array's text) and clocks in
    /// whatever it is asked.
    fn agenda(fake: &Fake, tasks: &str) {
        let list = format!(r#"{{"tasks":{tasks}}}"#);
        fake.on_service(TASKS, Ok(&list))
            .on_service(CLOCK_IN, Ok(""));
    }

    const FOUND: &str =
        r#"[{"id":3,"title":"TASK-004 Add the export button","status":"todo","elapsedMinutes":0}]"#;

    #[test]
    fn s_on_a_page_claims_the_item_and_clocks_in_on_its_task() {
        let fake = demo();
        kiln_says(&fake);
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-page");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(sets(&fake).len(), 1, "{:?}", sets(&fake));
        assert!(sets(&fake)[0].ends_with("/usr/bin/kiln set -- TASK-004 status=doing"));
        assert_eq!(
            asked(&fake),
            [
                (TASKS.to_string(), "{}".to_string()),
                (CLOCK_IN.to_string(), r#"{"task_id":3}"#.to_string()),
            ]
        );
        assert_eq!(
            foot(&mut h, page),
            format!("TASK-004: status todo → doing; clocked in on \"{MADE}\"")
        );
        assert!(h.ext.starting.is_none() && h.ext.agenda_calls.is_empty());
        assert!(h.ext.asked.is_empty());
        // The project is read again after the edit, as after any.
        let calls = fake.calls();
        let at = calls
            .iter()
            .position(|c| c.contains("kiln set --"))
            .unwrap();
        assert!(calls[at..]
            .iter()
            .any(|c| c.ends_with("kiln list --format json")));
    }

    #[test]
    fn s_with_no_task_for_the_item_makes_one_and_clocks_in_on_it() {
        let fake = demo();
        kiln_says(&fake);
        let made = Rc::new(Cell::new(false));
        let (m1, m2) = (made.clone(), made.clone());
        fake.on_service_with(TASKS, move |_| {
            Ok(if m1.get() {
                format!(r#"{{"tasks":{FOUND}}}"#)
            } else {
                r#"{"tasks":[]}"#.to_string()
            })
        })
        .on_service_with(ADD_TASK, move |_| {
            m2.set(true);
            Ok(String::new())
        })
        .on_service(CLOCK_IN, Ok(""));
        let fake = {
            fake.project("demo", "/home/me/demo");
            fake
        };
        let (mut h, page) = opened(&fake, "start-make");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(names(&fake), [TASKS, ADD_TASK, TASKS, CLOCK_IN]);
        // The title and, the project's folder known, the item's file and line.
        let add: serde_json::Value = serde_json::from_str(&asked(&fake)[1].1).unwrap();
        assert_eq!(add["title"], MADE);
        assert_eq!(add["line"], 2);
        assert!(
            add["path"]
                .as_str()
                .unwrap()
                .ends_with("/design/tasks/TASK-004-add-the-export-button.md"),
            "{add}"
        );
        assert_eq!(
            foot(&mut h, page),
            format!("TASK-004: status todo → doing; clocked in on \"{MADE}\"")
        );
    }

    #[test]
    fn an_item_with_a_jira_key_is_clocked_in_on_the_task_with_that_key() {
        let fake = demo();
        let list = fixture_with_jira();
        fake.on_spawn("kiln list --format json", Ran::ok(list));
        kiln_says(&fake);
        agenda(
            &fake,
            r#"[{"id":3,"title":"TASK-004 Add the export button","status":"todo"},
                {"id":8,"title":"Export button","status":"inProgress","jiraKey":"FLINT-12","tracker":"fenix.jira"}]"#,
        );
        let (mut h, page) = opened(&fake, "start-jira");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(
            asked(&fake)[1],
            (CLOCK_IN.to_string(), r#"{"task_id":8}"#.to_string())
        );
        assert_eq!(
            foot(&mut h, page),
            "TASK-004: status todo → doing; clocked in on \"Export button\""
        );
    }

    /// The demo's items with TASK-004 linked to a Jira ticket.
    fn fixture_with_jira() -> String {
        let text = crate::model::tests::fixture("demo/list.json");
        let mut items: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
        for i in &mut items {
            if i["id"] == "TASK-004" {
                i["jira_keys"] = serde_json::json!(["FLINT-12"]);
            }
        }
        serde_json::to_string(&items).unwrap()
    }

    #[test]
    fn the_owner_setting_claims_the_item_in_the_same_command() {
        let fake = demo();
        fake.setting(OWNER_SETTING, " ana ").on_spawn(
            "kiln set -- TASK-004 owner=ana status=doing",
            Ran::ok("TASK-004: owner → ana; status todo → doing\n"),
        );
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-owner");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert!(sets(&fake)[0].ends_with("set -- TASK-004 owner=ana status=doing"));
        assert_eq!(
            foot(&mut h, page),
            format!("TASK-004: owner → ana; status todo → doing; clocked in on \"{MADE}\"")
        );
        // An item somebody has: the setting doesn't take it.
        fake.take();
        h.keys(page, "/TASK-003<Enter>/<Esc>");
        fake.on_spawn(
            "kiln set -- TASK-003 status=doing",
            Ran::ok("TASK-003: status todo → doing\n"),
        );
        h.key(page, Key::Char('S'));
        assert!(
            sets(&fake)
                .last()
                .unwrap()
                .ends_with("set -- TASK-003 status=doing"),
            "{:?}",
            sets(&fake)
        );
    }

    #[test]
    fn started_from_a_buffer_it_says_both_on_the_message_line() {
        let fake = demo();
        in_buffer(&fake);
        kiln_says(&fake);
        agenda(&fake, FOUND);
        let mut h = bare(&fake, "start-buffer");
        h.command("kiln.start").unwrap();
        assert_eq!(sets(&fake).len(), 1);
        assert_eq!(
            fake.notifications(),
            [
                "Kiln: TASK-004: status todo → doing".to_string(),
                format!("Kiln: TASK-004: status todo → doing; clocked in on \"{MADE}\""),
            ]
        );
        assert_eq!(names(&fake), [TASKS, CLOCK_IN]);
        // No page was needed.
        assert!(fake.page("plan").is_none());
    }

    #[test]
    fn a_buffer_off_any_item_says_so_and_starts_nothing() {
        let fake = demo();
        fake.project("demo", "/home/me/demo").in_file(
            "/home/me/demo/src/log.rs",
            "rs",
            "fn main() {}\n",
        );
        agenda(&fake, FOUND);
        let mut h = bare(&fake, "start-noid");
        h.command("kiln.start").unwrap();
        assert!(sets(&fake).is_empty() && asked(&fake).is_empty());
        assert_eq!(
            fake.notifications(),
            ["Kiln: no Kiln ID under the cursor on line 1 of src/log.rs, and src/log.rs isn't an item's file"]
        );
    }

    #[test]
    fn without_the_agenda_the_status_changes_and_it_says_why_once_and_calmly() {
        let fake = demo();
        in_buffer(&fake);
        kiln_says(&fake);
        fake.on_service(TASKS, Err("nothing offers agenda.tasks"));
        let mut h = bare(&fake, "start-noagenda");
        h.command("kiln.start").unwrap();
        assert_eq!(sets(&fake).len(), 1, "the status still changes");
        assert_eq!(names(&fake), [TASKS], "and the agenda is asked no more");
        let said = fake.notifications();
        assert_eq!(
            said.last().unwrap(),
            "Kiln: TASK-004: status todo → doing; no clock: the Agenda extension isn't installed"
        );
        // Not an error: the message line's info level, nothing logged as one.
        assert!(
            !fake.calls().iter().any(|c| c.starts_with("notify error")),
            "{:?}",
            fake.calls()
        );
        assert!(h.ext.starting.is_none());
    }

    #[test]
    fn on_a_page_without_the_agenda_the_bottom_line_says_it_the_same() {
        let fake = demo();
        kiln_says(&fake);
        fake.on_service(TASKS, Err("nothing offers agenda.tasks"));
        let (mut h, page) = opened(&fake, "start-noagenda-page");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(
            foot(&mut h, page),
            "TASK-004: status todo → doing; no clock: the Agenda extension isn't installed"
        );
    }

    #[test]
    fn when_kiln_refuses_nothing_is_clocked() {
        let fake = demo();
        fake.on_spawn(
            SET,
            Ran::fails(
                1,
                "kiln: design/tasks/TASK-004-add-the-export-button.md changed on disk since it was read; run the command again",
            ),
        );
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-refused");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(sets(&fake).len(), 1);
        assert!(asked(&fake).is_empty(), "no clock: {:?}", asked(&fake));
        let line = foot(&mut h, page);
        assert!(
            line.starts_with(
                "kiln: design/tasks/TASK-004-add-the-export-button.md changed on disk"
            ),
            "{line}"
        );
        assert!(h.ext.starting.is_none() && h.ext.agenda_calls.is_empty());
        // From a buffer, in the message line, as an error.
        let fake = demo();
        in_buffer(&fake);
        fake.on_spawn(SET, Ran::fails(2, "kiln: status doing isn't one of task's"));
        agenda(&fake, FOUND);
        let mut h = bare(&fake, "start-refused-buffer");
        h.command("kiln.start").unwrap();
        assert!(asked(&fake).is_empty());
        assert_eq!(
            fake.notifications(),
            ["kiln: status doing isn't one of task's"]
        );
        assert!(fake
            .calls()
            .iter()
            .any(|c| c.starts_with("notify error kiln: status doing")));
    }

    #[test]
    fn when_the_agenda_refuses_it_says_what_it_said_and_the_status_stands() {
        let fake = demo();
        kiln_says(&fake);
        fake.on_service(TASKS, Ok(&format!(r#"{{"tasks":{FOUND}}}"#)))
            .on_service(CLOCK_IN, Err("no agenda task 3"));
        let (mut h, page) = opened(&fake, "start-agenda-refuses");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(sets(&fake).len(), 1);
        assert_eq!(
            foot(&mut h, page),
            "TASK-004: status todo → doing; the agenda didn't clock in: no agenda task 3"
        );
        // Unreadable: said with a piece of what it gave.
        let fake = demo();
        in_buffer(&fake);
        kiln_says(&fake);
        fake.on_service(TASKS, Ok("<html>"));
        let mut h = bare(&fake, "start-agenda-garbage");
        h.command("kiln.start").unwrap();
        let said = fake.notifications();
        assert!(
            said.last().unwrap().starts_with(
                "Kiln: TASK-004: status todo → doing; the agenda's tasks can't be read: "
            ),
            "{said:?}"
        );
        assert!(said.last().unwrap().contains("<html>"));
        assert!(fake
            .calls()
            .iter()
            .any(|c| c.starts_with("notify error Kiln: TASK-004")));
    }

    #[test]
    fn an_item_already_started_is_only_clocked_in() {
        let fake = demo();
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-started");
        // TASK-002 is doing, ana's.
        on(&mut h, page, "TASK-002");
        let found =
            r#"[{"id":5,"title":"TASK-002 Write readings to SQLite","status":"inProgress"}]"#;
        agenda(&fake, found);
        h.key(page, Key::Char('S'));
        assert!(sets(&fake).is_empty(), "no kiln call: {:?}", sets(&fake));
        assert_eq!(names(&fake), [TASKS, CLOCK_IN]);
        assert_eq!(
            foot(&mut h, page),
            "TASK-002: already doing; clocked in on \"TASK-002 Write readings to SQLite\""
        );
    }

    #[test]
    fn nothing_starts_while_an_edit_cant_and_on_a_row_with_no_item() {
        let fake = demo();
        kiln_says(&fake);
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-blocked");
        // No row matches the filter: no item.
        h.keys(page, "/zzz<Enter>");
        h.key(page, Key::Char('S'));
        assert_eq!(foot(&mut h, page), "/zzz  no item here");
        assert!(sets(&fake).is_empty() && asked(&fake).is_empty());
        // kiln can't be read now.
        let fake = Fake::new();
        agenda(&fake, FOUND);
        let (mut h, page) = {
            let dir = folder("start-nokiln");
            let mut h = harness(&fake, &dir);
            h.size = (110, 26);
            h.command("kiln.open").unwrap();
            let page = h.page("plan");
            (h, page)
        };
        h.key(page, Key::Char('S'));
        assert!(asked(&fake).is_empty());
    }

    #[test]
    fn no_start_while_the_last_read_failed() {
        let fake = demo();
        kiln_says(&fake);
        agenda(&fake, FOUND);
        let (mut h, page) = opened(&fake, "start-failed-read");
        on(&mut h, page, "TASK-004");
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, "kiln: the design folder is gone"),
        );
        h.key(page, Key::Char('r'));
        h.key(page, Key::Char('S'));
        assert!(sets(&fake).is_empty() && asked(&fake).is_empty());
        let line = foot(&mut h, page);
        assert!(
            line.contains("no edits while the last read failed"),
            "{line}"
        );
    }

    #[test]
    fn a_second_start_replaces_the_first_and_the_late_answer_is_dropped() {
        let fake = demo();
        kiln_says(&fake);
        // The agenda never answers the first list.
        let (mut h, page) = opened(&fake, "start-replaced");
        on(&mut h, page, "TASK-004");
        h.key(page, Key::Char('S'));
        assert_eq!(names(&fake), [TASKS]);
        let serial = h.ext.starting.as_ref().unwrap().serial;
        fake.on_spawn(
            "kiln set -- TASK-003 status=doing",
            Ran::ok("TASK-003: status todo → doing\n"),
        );
        agenda(
            &fake,
            r#"[{"id":4,"title":"TASK-003 Export the log as CSV","status":"todo"}]"#,
        );
        on(&mut h, page, "TASK-003");
        h.key(page, Key::Char('S'));
        assert!(h.ext.starting.is_none());
        assert_ne!(serial, h.ext.start_serial);
        assert_eq!(
            foot(&mut h, page),
            "TASK-003: status todo → doing; clocked in on \"TASK-003 Export the log as CSV\""
        );
        // The first one's reply, late: nothing happens.
        let before = names(&fake).len();
        h.event(crate::api::Event::ServiceReply((
            *h.ext.agenda_calls.keys().next().unwrap_or(&9999),
            Ok(String::new()),
        )));
        assert_eq!(names(&fake).len(), before);
    }
}
