//! The edits of Kiln's GUI ("Editing" in Kiln's GUI.md) as flows of picks
//! and prompts, each ending in one kiln editing command: `set`, `link`,
//! `unlink`, `new` or `renumber`.
//!
//! A [`Flow`] is pure data. It is started on an item ID with the project
//! as kiln last read it and `kiln.toml` as [`crate::config`] read it
//! ([`Facts`]), and each answer -- the value picked, the text typed --
//! gives the next [`Step`]: another pick, a prompt, the kiln command to
//! run ([`Step::Run`], its arguments after `kiln`), a line to say, or
//! nothing more. When kiln has run it, [`Flow::edited`] reads how it went:
//! the ID the cursor follows (the new one after a renumber or a new item),
//! the file to open (a new item), or the prompt asked again after a
//! refusal. Whoever drives it -- the plan page's frame for the keys, a
//! command from a buffer later -- asks Fenix and kiln and hands answers
//! back; this module calls neither, so every flow is tested as a table:
//! answers in, kiln's arguments out.
//!
//! # Where the choices come from
//!
//! Items, their titles, statuses, parents and links: from the loaded
//! project (`kiln list`). Types, their statuses in order, allowed parents,
//! link roles and their targets: from `kiln.toml` ([`crate::config`]),
//! when it could be read. When it couldn't (or doesn't have the item's
//! type), a flow asks for the value as text instead -- the status typed,
//! the role and the target typed -- and `H`/`L` say why they can't. What's
//! offered is never a check: kiln checks every edit, and a choice it
//! refuses is said as kiln's line.
//!
//! # Arguments
//!
//! Each value is one argument, never through a shell, and nothing a user
//! types can be read by kiln as an option: values ride in `--title=…`,
//! `--parent=…`, `--to=…` and `KEY=…`, and `--` comes before the
//! positional arguments (the ID, the role, the target, the type). kiln's
//! clap reads `kiln new task --title "-v"` as an unknown option `-v`
//! (recorded: `tests/fixtures/out/edits/new-dash-unsafe.*`); `--title=-v`
//! is the title `-v` (`new-dash.*`).
//!
//! # Refusals
//!
//! kiln refuses with exit 1 (a file changed on disk since it read it) or
//! exit 2 (a value the configuration doesn't allow), one `kiln: …` line
//! that the driver says. A flow whose last value was typed asks for it
//! again with what was typed, so a typo isn't retyped from nothing, at
//! most [`MOST_TRIES`] times; a flow of picks only ends there (picking the
//! same again would be refused again).
//!
//! What it deliberately doesn't do: run kiln, ask Fenix, read files, or
//! check a value itself (a date, a title's length): kiln's checks are the
//! only ones. It doesn't renumber one copy of a duplicated ID (`--file`,
//! from Check: not offered yet), edit tags or Jira keys, or undo.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::config::{Config, Type};
use crate::kiln::{Failure, Ran};
use crate::model::{self, Item};
use crate::pages::view::{Choice, Place};
use crate::project::{Loaded, State};

/// How many times a refused value is asked for again before the flow
/// ends: enough to fix a typo or retry a file changed on disk, few enough
/// that a value kiln will never take can't loop.
pub const MOST_TRIES: u8 = 3;

/// The most items a pick offers (a link's target, a parent): Fenix's
/// lists' bound, far past Kiln's budget of 2,000 items.
pub const MOST_CHOICES: usize = 10_000;

/// The value of the "no parent" choice. Not an ID (IDs are `PREFIX-N`),
/// so it can't be mistaken for one; typed, it means none too.
const NO_PARENT: &str = "(none)";

/// The answers of a confirmation, the safe one first.
const NO: &str = "no";
const YES: &str = "yes";

/// An edit, as the keys and the menu name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// `s`: pick one of its type's statuses.
    Status,
    /// `H` (Board): the previous status in the type's order.
    Back,
    /// `L` (Board): the next status.
    Forward,
    /// `l`: a role, then a target.
    Link,
    /// `U`: one of its links, then yes.
    Unlink,
    /// `n`: a type, then a title. Needs no item.
    New,
    /// `N`: a type that may have it as parent, then a title.
    Child,
    Title,
    Parent,
    Owner,
    Due,
    /// The new ID typed, then yes.
    Renumber,
    /// The menu of every edit ([`MENU_KEY`]).
    More,
}

/// The key that opens the menu of edits on the plan page: `E`. Kiln's GUI
/// puts title, parent, owner, due and renumber under `SPC e`, which a
/// Fenix page can't have (Space is Fenix's leader); `E` keeps the `e` of
/// it, is free on every tab (`e` is Check's), and, shifted, isn't pressed
/// by accident.
pub const MENU_KEY: char = 'E';

impl Edit {
    /// The edit a key starts on the plan page, if any.
    pub fn by_key(c: char) -> Option<Edit> {
        Some(match c {
            's' => Edit::Status,
            'H' => Edit::Back,
            'L' => Edit::Forward,
            'l' => Edit::Link,
            'U' => Edit::Unlink,
            'n' => Edit::New,
            'N' => Edit::Child,
            MENU_KEY => Edit::More,
            _ => return None,
        })
    }

    /// Its name in the menu's values and in commands.
    pub fn name(self) -> &'static str {
        match self {
            Edit::Status => "status",
            Edit::Back => "back",
            Edit::Forward => "forward",
            Edit::Link => "link",
            Edit::Unlink => "unlink",
            Edit::New => "new",
            Edit::Child => "child",
            Edit::Title => "title",
            Edit::Parent => "parent",
            Edit::Owner => "owner",
            Edit::Due => "due",
            Edit::Renumber => "renumber",
            Edit::More => "more",
        }
    }

    pub fn by_name(name: &str) -> Option<Edit> {
        MENU.iter()
            .map(|(e, _, _)| *e)
            .chain([Edit::Back, Edit::Forward, Edit::More])
            .find(|e| e.name() == name)
    }
}

/// The menu: each edit, what it's called, and its own key on the page
/// when it has one. The ones only the menu reaches come first.
const MENU: &[(Edit, &str, &str)] = &[
    (Edit::Title, "title", ""),
    (Edit::Parent, "parent", ""),
    (Edit::Owner, "owner", ""),
    (Edit::Due, "due date", ""),
    (Edit::Renumber, "renumber", ""),
    (Edit::Status, "status", "s"),
    (Edit::Link, "link", "l"),
    (Edit::Unlink, "unlink", "U"),
    (Edit::Child, "new child", "N"),
    (Edit::New, "new item", "n"),
];

/// What a flow knows the project by: as kiln last read it, and
/// `kiln.toml` (or why it couldn't be read).
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub loaded: &'a Loaded,
    pub config: Result<&'a Config, &'a str>,
}

impl<'a> Facts<'a> {
    /// `kind`'s definition, when `kiln.toml` was read and has it.
    fn kind(&self, kind: &str) -> Option<&'a Type> {
        self.config.ok()?.kind(kind)
    }
}

/// What comes next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// One of `items` picked; its value to [`Flow::answer`].
    Pick { title: String, items: Vec<Choice> },
    /// A line typed; it to [`Flow::answer`].
    Ask { label: String, initial: String },
    /// kiln run with these arguments (after `kiln`); how it went to
    /// [`Flow::edited`].
    Run(Vec<String>),
    /// Said on the bottom line; the flow is over.
    Say(String),
    /// Over, with nothing to say.
    Done,
}

/// How an edit ended, for the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// What next: [`Step::Ask`] again after a refusal, [`Step::Say`] for a
    /// word of the flow's own (a new item's path), else [`Step::Done`]
    /// (kiln's line, said by the driver, stands).
    pub step: Step,
    /// The ID the cursor follows now: the item's (a new one after
    /// `renumber`), or the new item's.
    pub follow: Option<String>,
    /// A new item's file, to open.
    pub open: Option<Place>,
}

/// Where a flow is: what its next answer is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Menu,
    Status,
    Role,
    Target {
        role: String,
    },
    Unlink,
    UnlinkSure {
        role: String,
        target: String,
    },
    Kind {
        child: bool,
    },
    NewTitle {
        kind: String,
        child: bool,
    },
    Title,
    Parent,
    Owner,
    Due,
    Renumber,
    RenumberSure {
        to: String,
    },
    /// kiln runs it; then [`Flow::edited`].
    Running,
    Over,
}

/// What a success means for the cursor and the files.
#[derive(Debug, Clone, PartialEq, Eq)]
enum After {
    /// The item keeps its ID.
    Same,
    /// `renumber`: it's this ID now.
    Renamed(String),
    /// `new --format json`: read the new item from kiln's answer.
    Made,
}

/// The last prompt answered: asked again, with what was typed, when kiln
/// refuses the edit it led to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Typed {
    stage: Stage,
    label: String,
    text: String,
}

/// One edit under way on item `id` (empty for a new item from nowhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    id: String,
    stage: Stage,
    /// The label of the prompt out, when the step out is one.
    asking: Option<String>,
    typed: Option<Typed>,
    after: After,
    tries: u8,
}

impl Flow {
    /// `edit` started on item `id` (the row's; `None` on a row with no
    /// item). The flow, while it goes on, and its first step. An edit that
    /// needs an item says so when there's none here, or when `id` isn't
    /// in the project as kiln last read it (a removed item on Changes).
    pub fn start(edit: Edit, id: Option<&str>, facts: &Facts) -> (Option<Flow>, Step) {
        let mut flow = Flow {
            id: id.unwrap_or_default().to_string(),
            stage: Stage::Over,
            asking: None,
            typed: None,
            after: After::Same,
            tries: 0,
        };
        let step = if edit == Edit::New {
            flow.new_item(None, facts)
        } else {
            match id.map(|id| (id, facts.loaded.item(id))) {
                None => Step::Say("no item here".into()),
                Some((id, None)) => Step::Say(format!(
                    "no item here: {id} isn't in the project as kiln last read it"
                )),
                Some((_, Some(item))) => flow.begin(edit, item, facts),
            }
        };
        let going = matches!(step, Step::Pick { .. } | Step::Ask { .. } | Step::Run(_));
        (going.then_some(flow), step)
    }

    /// The item it's about.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether it waits for kiln ([`Flow::edited`]) rather than an answer.
    pub fn running(&self) -> bool {
        self.stage == Stage::Running
    }

    fn begin(&mut self, edit: Edit, item: &Item, facts: &Facts) -> Step {
        let id = item.id.clone();
        match edit {
            Edit::Status => match facts.kind(&item.kind) {
                Some(t) => {
                    let items = t
                        .statuses
                        .iter()
                        .map(|s| Choice::new(s, s).detail(status_note(t, s, &item.status)))
                        .collect();
                    self.pick(Stage::Status, format!("Status of {id}"), items)
                }
                None => self.ask(
                    Stage::Status,
                    format!("Status of {id} (now {})", item.status),
                    "",
                ),
            },
            Edit::Back | Edit::Forward => self.step(edit == Edit::Forward, item, facts),
            Edit::Link => match (facts.config, facts.kind(&item.kind)) {
                (Ok(c), Some(_)) => {
                    let roles = c.roles_from(&item.kind);
                    if roles.is_empty() {
                        return Step::Say(format!(
                            "a {} can start no link role in kiln.toml",
                            item.kind
                        ));
                    }
                    let items = roles
                        .iter()
                        .map(|r| {
                            Choice::new(&r.name, &r.name).detail(format!("to {}", r.to.join(", ")))
                        })
                        .collect();
                    self.pick(Stage::Role, format!("Link {id} as"), items)
                }
                _ => self.ask(Stage::Role, format!("Link {id} as (a role)"), ""),
            },
            Edit::Unlink => {
                let items: Vec<Choice> = item
                    .links
                    .iter()
                    .flat_map(|(role, targets)| targets.iter().map(move |t| (role, t)))
                    .map(|(role, target)| {
                        let title = facts.loaded.item(target).map_or("", |i| i.title.as_str());
                        Choice::new(format!("{role}  {target}"), format!("{role} {target}"))
                            .detail(title)
                    })
                    .collect();
                if items.is_empty() {
                    return Step::Say(format!(
                        "{id} has no links of its own (a link is removed from the item it's written on)"
                    ));
                }
                self.pick(Stage::Unlink, format!("Unlink from {id}"), items)
            }
            Edit::New => self.new_item(None, facts),
            Edit::Child => self.new_item(Some(item), facts),
            Edit::Title => self.ask(Stage::Title, format!("Title of {id}"), item.title.clone()),
            Edit::Parent => match facts.kind(&item.kind) {
                Some(t) => self.pick_parent(item, t, facts),
                None => self.ask(
                    Stage::Parent,
                    format!("Parent of {id} (empty: none)"),
                    item.parent.clone().unwrap_or_default(),
                ),
            },
            Edit::Owner => self.ask(
                Stage::Owner,
                format!("Owner of {id} (empty: none)"),
                item.owner.clone().unwrap_or_default(),
            ),
            Edit::Due => self.ask(
                Stage::Due,
                format!("Due date of {id}, YYYY-MM-DD (empty: none)"),
                item.due.clone().unwrap_or_default(),
            ),
            Edit::Renumber => self.ask(
                Stage::Renumber,
                format!("Renumber {id} to"),
                next_free(facts.loaded, &id),
            ),
            Edit::More => {
                let items = MENU
                    .iter()
                    .map(|(e, label, key)| Choice::new(*label, e.name()).detail(*key))
                    .collect();
                self.pick(Stage::Menu, format!("Edit {id}"), items)
            }
        }
    }

    /// `H`/`L`: the status before or after its own, in its type's order.
    fn step(&mut self, forward: bool, item: &Item, facts: &Facts) -> Step {
        let (id, kind, status) = (&item.id, &item.kind, &item.status);
        let Some(t) = facts.kind(kind) else {
            let why = match facts.config {
                Err(why) => why.to_string(),
                Ok(_) => format!("kiln.toml has no type {kind}"),
            };
            return Step::Say(format!(
                "H and L need kiln.toml's order of {kind} statuses: {why}"
            ));
        };
        let Some(at) = t.position(status) else {
            return Step::Say(format!(
                "{status} isn't one of {kind}'s statuses in kiln.toml (s picks one)"
            ));
        };
        let to = if forward {
            t.statuses.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|b| t.statuses.get(b))
        };
        match to {
            Some(to) => self.run(set(id, "status", to), After::Same),
            None if forward => Step::Say(format!("{id} is at {kind}'s last status, {status}")),
            None => Step::Say(format!("{id} is at {kind}'s first status, {status}")),
        }
    }

    /// `n` (no `parent`) or `N`: the type, from `kiln.toml` when it was
    /// read (for a child, the types that may have it as parent), else
    /// typed.
    fn new_item(&mut self, parent: Option<&Item>, facts: &Facts) -> Step {
        let child = parent.is_some();
        let (title, label) = match parent {
            Some(p) => (
                format!("New child of {}", p.id),
                format!("Type of the new child of {}", p.id),
            ),
            None => ("New item's type".into(), "Type of the new item".into()),
        };
        let Ok(c) = facts.config else {
            return self.ask(Stage::Kind { child }, label, "");
        };
        let kinds: Vec<&Type> = match parent {
            Some(p) => c.children_of(&p.kind),
            None => c.types.iter().collect(),
        };
        if kinds.is_empty() {
            let kind = parent.map_or("", |p| p.kind.as_str());
            return Step::Say(format!("no type in kiln.toml may have a {kind} as parent"));
        }
        let items = kinds
            .iter()
            .map(|t| Choice::new(&t.name, &t.name).detail(format!("{}-", t.prefix)))
            .collect();
        self.pick(Stage::Kind { child }, title, items)
    }

    /// The parent picked among the items of the types `t` allows, not the
    /// item or anything under it (kiln would refuse the cycle); "none"
    /// first when it has one.
    fn pick_parent(&mut self, item: &Item, t: &Type, facts: &Facts) -> Step {
        let id = &item.id;
        let below = below(facts.loaded, id);
        let mut items: Vec<Choice> = Vec::new();
        if item.parent.is_some() {
            items.push(Choice::new(NO_PARENT, NO_PARENT).detail("removes the parent"));
        }
        items.extend(
            facts
                .loaded
                .items
                .iter()
                .filter(|i| {
                    t.parents.contains(&i.kind) && i.id != *id && !below.contains(i.id.as_str())
                })
                .take(MOST_CHOICES)
                .map(|i| {
                    let now = item.parent.as_deref() == Some(i.id.as_str());
                    item_choice(i).detail(if now {
                        "now".to_string()
                    } else {
                        i.status.clone()
                    })
                }),
        );
        if items.is_empty() {
            return Step::Say(if t.parents.is_empty() {
                format!("a {} can't have a parent (kiln.toml)", item.kind)
            } else {
                format!("no {} can be {id}'s parent", t.parents.join(" or "))
            });
        }
        self.pick(Stage::Parent, format!("Parent of {id}"), items)
    }

    /// The next answer: the value picked or the text typed, for the step
    /// last given. An answer with nothing to go on (the item gone since)
    /// ends the flow, saying why.
    pub fn answer(&mut self, value: &str, facts: &Facts) -> Step {
        if let Some(label) = self.asking.take() {
            self.typed = Some(Typed {
                stage: self.stage.clone(),
                label,
                text: value.to_string(),
            });
        }
        let stage = std::mem::replace(&mut self.stage, Stage::Over);
        let value = value.trim();
        let id = self.id.clone();
        // A new item that isn't a child is about no item.
        if matches!(
            stage,
            Stage::Kind { child: false } | Stage::NewTitle { child: false, .. }
        ) {
            return self.answer_new(stage, value, None);
        }
        let Some(item) = facts.loaded.item(&id) else {
            return Step::Say(format!("{id} isn't in the project any more"));
        };
        match stage {
            Stage::Menu => match Edit::by_name(value) {
                Some(edit) => {
                    let (flow, step) = Flow::start(edit, Some(&id), facts);
                    *self = flow.unwrap_or_else(|| Flow {
                        stage: Stage::Over,
                        ..self.clone()
                    });
                    step
                }
                None => Step::Done,
            },
            Stage::Status => {
                if value.is_empty() {
                    Step::Done
                } else if value == item.status {
                    Step::Say(format!("{id} is {value} already"))
                } else {
                    self.run(set(&id, "status", value), After::Same)
                }
            }
            Stage::Role => {
                if value.is_empty() {
                    return Step::Done;
                }
                let role = value.to_string();
                match facts
                    .config
                    .ok()
                    .and_then(|c| c.roles.iter().find(|r| r.name == role))
                {
                    Some(r) => {
                        let linked: HashSet<&str> = item
                            .links
                            .get(&role)
                            .map(|v| v.iter().map(String::as_str).collect())
                            .unwrap_or_default();
                        let items: Vec<Choice> = facts
                            .loaded
                            .items
                            .iter()
                            .filter(|i| {
                                r.to.contains(&i.kind)
                                    && i.id != id
                                    && !linked.contains(i.id.as_str())
                            })
                            .take(MOST_CHOICES)
                            .map(|i| item_choice(i).detail(i.status.clone()))
                            .collect();
                        if items.is_empty() {
                            return Step::Say(format!(
                                "no {} left for {id} to {role}",
                                r.to.join(" or ")
                            ));
                        }
                        self.pick(
                            Stage::Target { role: role.clone() },
                            format!("{id} {role}"),
                            items,
                        )
                    }
                    None => self.ask(
                        Stage::Target { role: role.clone() },
                        format!("{id} {role} (an ID)"),
                        "",
                    ),
                }
            }
            Stage::Target { role } => {
                if value.is_empty() {
                    return Step::Done;
                }
                self.run(args(["link", "--", &id, &role, value]), After::Same)
            }
            Stage::Unlink => match value.split_once(' ') {
                Some((role, target)) => self.pick(
                    Stage::UnlinkSure {
                        role: role.to_string(),
                        target: target.to_string(),
                    },
                    format!("Remove {id} {role} {target}?"),
                    confirm("unlink"),
                ),
                None => Step::Done,
            },
            Stage::UnlinkSure { role, target } => {
                if value == YES {
                    self.run(args(["unlink", "--", &id, &role, &target]), After::Same)
                } else {
                    Step::Say(format!("kept: {id} {role} {target}"))
                }
            }
            stage @ (Stage::Kind { .. } | Stage::NewTitle { .. }) => {
                self.answer_new(stage, value, Some(item))
            }
            Stage::Title => {
                if value == item.title {
                    Step::Say(format!("{id}'s title is as it was"))
                } else {
                    self.run(set(&id, "title", value), After::Same)
                }
            }
            Stage::Parent => {
                let to = if value == NO_PARENT { "" } else { value };
                if to == item.parent.as_deref().unwrap_or("") {
                    Step::Say(format!("{id}'s parent is as it was"))
                } else {
                    self.run(set(&id, "parent", to), After::Same)
                }
            }
            Stage::Owner => self.set_or_keep(&id, "owner", value, item.owner.as_deref()),
            Stage::Due => self.set_or_keep(&id, "due", value, item.due.as_deref()),
            Stage::Renumber => {
                if value.is_empty() {
                    Step::Done
                } else if value == id {
                    Step::Say(format!("{id} is {id} already"))
                } else {
                    self.pick(
                        Stage::RenumberSure {
                            to: value.to_string(),
                        },
                        format!("Renumber {id} to {value}? Every reference to it is rewritten"),
                        confirm("renumber"),
                    )
                }
            }
            Stage::RenumberSure { to } => {
                if value == YES {
                    let renamed = After::Renamed(to.clone());
                    self.run(
                        args(["renumber", &format!("--to={to}"), "--", &id]),
                        renamed,
                    )
                } else {
                    Step::Say(format!("{id} keeps its ID"))
                }
            }
            Stage::Running | Stage::Over => Step::Done,
        }
    }

    /// A new item's type, then its title (`parent`: a child of it).
    fn answer_new(&mut self, stage: Stage, value: &str, parent: Option<&Item>) -> Step {
        match stage {
            Stage::Kind { child } => {
                if value.is_empty() {
                    return Step::Done;
                }
                let under = match (child, parent) {
                    (true, Some(p)) => format!(" under {}", p.id),
                    _ => String::new(),
                };
                self.ask(
                    Stage::NewTitle {
                        kind: value.to_string(),
                        child,
                    },
                    format!("Title of the new {value}{under}"),
                    "",
                )
            }
            Stage::NewTitle { kind, child } => {
                if value.is_empty() {
                    return Step::Say("no title, so nothing was made".into());
                }
                let mut a = vec!["new".to_string()];
                if child {
                    a.push(format!("--parent={}", self.id));
                }
                a.extend([
                    format!("--title={value}"),
                    "--format".into(),
                    "json".into(),
                    "--".into(),
                    kind,
                ]);
                self.run(a, After::Made)
            }
            _ => Step::Done,
        }
    }

    fn set_or_keep(&mut self, id: &str, key: &str, value: &str, now: Option<&str>) -> Step {
        if value == now.unwrap_or("") {
            Step::Say(format!("{id}'s {key} is as it was"))
        } else {
            self.run(set(id, key, value), After::Same)
        }
    }

    /// How kiln's run of the step [`Step::Run`] went. Done: the cursor
    /// follows the item (or its new ID, or the new item, whose file
    /// opens). Refused (exit 1 or 2): the last prompt again with what was
    /// typed, while tries are left; otherwise over -- the driver has said
    /// kiln's line, which stands.
    pub fn edited(&mut self, result: &Result<Ran, Failure>) -> Outcome {
        let after = std::mem::replace(&mut self.after, After::Same);
        self.stage = Stage::Over;
        let over = |step: Step, follow: Option<String>| Outcome {
            step,
            follow,
            open: None,
        };
        let ran = match result {
            Ok(ran) if ran.code == Some(0) => ran,
            Ok(Ran {
                code: Some(1 | 2), ..
            })
            | Err(Failure::Exit {
                code: Some(1 | 2), ..
            }) => {
                return match self.typed.clone() {
                    Some(t) if self.tries < MOST_TRIES => {
                        self.tries += 1;
                        let step = self.ask(t.stage, t.label, t.text);
                        over(step, None)
                    }
                    _ => over(Step::Done, None),
                };
            }
            _ => return over(Step::Done, None),
        };
        match after {
            After::Same => over(Step::Done, Some(self.id.clone())),
            After::Renamed(to) => over(Step::Done, Some(to)),
            After::Made => match made(&ran.stdout) {
                Ok(m) => Outcome {
                    step: Step::Say(format!("{}: {}", m.id, m.path)),
                    follow: Some(m.id),
                    open: Some(Place {
                        path: m.path,
                        line: m.line.filter(|&l| l > 0),
                    }),
                },
                Err(why) => over(
                    Step::Say(format!(
                        "kiln made the item, but its answer can't be read: {why}"
                    )),
                    None,
                ),
            },
        }
    }

    fn pick(&mut self, stage: Stage, title: String, items: Vec<Choice>) -> Step {
        self.stage = stage;
        self.asking = None;
        Step::Pick { title, items }
    }

    fn ask(&mut self, stage: Stage, label: String, initial: impl Into<String>) -> Step {
        self.stage = stage;
        self.asking = Some(label.clone());
        Step::Ask {
            label,
            initial: initial.into(),
        }
    }

    fn run(&mut self, args: Vec<String>, after: After) -> Step {
        self.stage = Stage::Running;
        self.asking = None;
        self.after = after;
        Step::Run(args)
    }
}

/// `kiln new --format json`'s answer: the new item's ID and file (and its
/// line, should a kiln give one; 1.0.0 doesn't).
#[derive(Debug, Deserialize)]
struct Made {
    id: String,
    path: String,
    #[serde(default)]
    line: Option<u32>,
}

fn made(stdout: &str) -> Result<Made, String> {
    let m: Made = model::object("kiln new", stdout)?;
    if m.id.is_empty() || m.path.is_empty() {
        return Err(format!(
            "no id or path in: {}",
            model::clip(stdout.trim(), 200)
        ));
    }
    Ok(m)
}

/// `set -- ID KEY=VALUE`: the value inside its one argument, so it can't
/// be an option whatever it starts with.
fn set(id: &str, key: &str, value: &str) -> Vec<String> {
    args(["set", "--", id, &format!("{key}={value}")])
}

fn args<'a>(a: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    a.into_iter().map(str::to_string).collect()
}

/// An item as a pick shows it: `ID  title`, its ID the value.
fn item_choice(i: &Item) -> Choice {
    Choice::new(format!("{}  {}", i.id, i.title), &i.id)
}

/// A confirmation: no first, so Enter on it changes nothing.
fn confirm(what: &str) -> Vec<Choice> {
    vec![
        Choice::new(format!("no, don't {what}"), NO),
        Choice::new(format!("yes, {what}"), YES),
    ]
}

/// What a status is for its type, beside it in the pick.
fn status_note(t: &Type, s: &str, now: &str) -> &'static str {
    if s == now {
        "now"
    } else if t.closed.iter().any(|c| c == s) {
        "closed"
    } else if t.done.iter().any(|d| d == s) {
        "done"
    } else {
        ""
    }
}

/// Every item under `id` by `parent`, at any depth: one pass to index
/// the children, then a walk that visits each once (a cycle in the files,
/// which kiln's check reports, can't loop it).
fn below<'a>(loaded: &'a Loaded, id: &str) -> HashSet<&'a str> {
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for i in &loaded.items {
        if let Some(p) = i.parent.as_deref() {
            children.entry(p).or_default().push(i.id.as_str());
        }
    }
    let mut seen = HashSet::new();
    let mut todo: Vec<&str> = children.get(id).cloned().unwrap_or_default();
    while let Some(c) = todo.pop() {
        if seen.insert(c) {
            todo.extend(children.get(c).into_iter().flatten().copied());
        }
    }
    seen
}

/// The next free ID with `id`'s prefix, to offer when renumbering: one
/// above the highest used (closed items included, as `kiln new` counts),
/// as wide as `id`'s number. Only offered: kiln checks it.
pub fn next_free(loaded: &Loaded, id: &str) -> String {
    let Some((prefix, number)) = id.rsplit_once('-') else {
        return id.to_string();
    };
    let width = number.len();
    let top = loaded
        .items
        .iter()
        .filter_map(|i| i.id.rsplit_once('-'))
        .filter(|(p, _)| *p == prefix)
        .filter_map(|(_, n)| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}-{:0width$}", top.saturating_add(1))
}

/// Why no edit can start now, if one can't: the project isn't read, the
/// last read failed (what's shown may be out of date), or an edit's
/// change hasn't been read back yet (`settled`: [`crate::project::Project::settled`]).
/// Starting one then would choose from a read known to be stale, so it
/// says so instead of queueing it.
pub fn blocked(state: &State, settled: bool) -> Option<String> {
    Some(match state {
        State::Loaded { error: Some(e), .. } => {
            format!("no edits while the last read failed: {e}")
        }
        State::Loaded { .. } if !settled => {
            "kiln is still reading the last edit back: try again in a moment".into()
        }
        State::Loaded { .. } => return None,
        State::Idle | State::Probing | State::Loading => {
            "the project isn't read yet: edits wait for it".into()
        }
        State::NotFound(why) | State::NotKiln(why) | State::NoProject(why) | State::Failed(why) => {
            format!("no edits: {why}")
        }
        State::TooOld(v) => format!("no edits: kiln {v} is older than {}", model::MIN_VERSION),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::model::tests::fixture;
    use crate::pages::frame::tests::demo;

    fn facts<'a>(l: &'a Loaded, c: &'a Config) -> Facts<'a> {
        Facts {
            loaded: l,
            config: Ok(c),
        }
    }

    fn unread(l: &Loaded) -> Facts<'_> {
        Facts {
            loaded: l,
            config: Err("there's no kiln.toml at the project's top"),
        }
    }

    /// `edit` on `id`, each answer given in turn: every step, the last
    /// first-class (the kiln arguments, a word, …).
    fn drive(edit: Edit, id: Option<&str>, answers: &[&str], f: &Facts) -> Vec<Step> {
        let (flow, first) = Flow::start(edit, id, f);
        let mut steps = vec![first];
        let Some(mut flow) = flow else { return steps };
        for a in answers {
            let step = flow.answer(a, f);
            let end = !matches!(step, Step::Pick { .. } | Step::Ask { .. });
            steps.push(step);
            if end {
                break;
            }
        }
        steps
    }

    fn last(steps: &[Step]) -> &Step {
        steps.last().unwrap()
    }

    fn run(a: &[&str]) -> Step {
        Step::Run(a.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn every_flow_ends_in_one_kiln_command() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        let table: &[(Edit, Option<&str>, &[&str], Step)] = &[
            (
                Edit::Status,
                Some("TASK-003"),
                &["doing"],
                run(&["set", "--", "TASK-003", "status=doing"]),
            ),
            (
                Edit::Forward,
                Some("TASK-003"),
                &[],
                run(&["set", "--", "TASK-003", "status=doing"]),
            ),
            (
                Edit::Back,
                Some("TASK-002"),
                &[],
                run(&["set", "--", "TASK-002", "status=todo"]),
            ),
            (
                Edit::Link,
                Some("TASK-003"),
                &["implements", "REQ-002"],
                run(&["link", "--", "TASK-003", "implements", "REQ-002"]),
            ),
            (
                Edit::Unlink,
                Some("TASK-003"),
                &["implements REQ-003", "yes"],
                run(&["unlink", "--", "TASK-003", "implements", "REQ-003"]),
            ),
            (
                Edit::New,
                None,
                &["task", "Write the manual"],
                run(&[
                    "new",
                    "--title=Write the manual",
                    "--format",
                    "json",
                    "--",
                    "task",
                ]),
            ),
            (
                Edit::New,
                Some("REQ-001"),
                &["risk", " Power fails "],
                run(&[
                    "new",
                    "--title=Power fails",
                    "--format",
                    "json",
                    "--",
                    "risk",
                ]),
            ),
            (
                Edit::Child,
                Some("TASK-002"),
                &["task", "Index the readings"],
                run(&[
                    "new",
                    "--parent=TASK-002",
                    "--title=Index the readings",
                    "--format",
                    "json",
                    "--",
                    "task",
                ]),
            ),
            (
                Edit::Title,
                Some("TASK-008"),
                &["-v, a title like an option"],
                run(&["set", "--", "TASK-008", "title=-v, a title like an option"]),
            ),
            (
                Edit::Parent,
                Some("TASK-008"),
                &["TASK-002"],
                run(&["set", "--", "TASK-008", "parent=TASK-002"]),
            ),
            (
                Edit::Parent,
                Some("TASK-007"),
                &["(none)"],
                run(&["set", "--", "TASK-007", "parent="]),
            ),
            (
                Edit::Owner,
                Some("TASK-003"),
                &[""],
                run(&["set", "--", "TASK-003", "owner="]),
            ),
            (
                Edit::Owner,
                Some("TASK-008"),
                &["ana"],
                run(&["set", "--", "TASK-008", "owner=ana"]),
            ),
            (
                Edit::Due,
                Some("TASK-003"),
                &["2030-01-31"],
                run(&["set", "--", "TASK-003", "due=2030-01-31"]),
            ),
            (
                Edit::Due,
                Some("MS-001"),
                &[""],
                run(&["set", "--", "MS-001", "due="]),
            ),
            (
                Edit::Renumber,
                Some("TASK-008"),
                &["TASK-020", "yes"],
                run(&["renumber", "--to=TASK-020", "--", "TASK-008"]),
            ),
            (
                Edit::Renumber,
                Some("TASK-008"),
                &["-x", "yes"],
                run(&["renumber", "--to=-x", "--", "TASK-008"]),
            ),
            (
                Edit::More,
                Some("TASK-003"),
                &["owner", "cy"],
                run(&["set", "--", "TASK-003", "owner=cy"]),
            ),
            (
                Edit::More,
                Some("TASK-003"),
                &["status", "done"],
                run(&["set", "--", "TASK-003", "status=done"]),
            ),
        ];
        for (edit, id, answers, want) in table {
            let steps = drive(*edit, *id, answers, &f);
            assert_eq!(
                last(&steps),
                want,
                "{edit:?} {id:?} {answers:?}: {steps:#?}"
            );
        }
    }

    #[test]
    fn without_kiln_toml_the_values_are_typed() {
        let l = demo();
        let f = unread(&l);
        let table: &[(Edit, Option<&str>, &[&str], Step)] = &[
            (
                Edit::Status,
                Some("TASK-003"),
                &["doing"],
                run(&["set", "--", "TASK-003", "status=doing"]),
            ),
            (
                Edit::Link,
                Some("TASK-003"),
                &["implements", "REQ-002"],
                run(&["link", "--", "TASK-003", "implements", "REQ-002"]),
            ),
            (
                Edit::New,
                None,
                &["task", "x"],
                run(&["new", "--title=x", "--format", "json", "--", "task"]),
            ),
            (
                Edit::Child,
                Some("TASK-002"),
                &["task", "y"],
                run(&[
                    "new",
                    "--parent=TASK-002",
                    "--title=y",
                    "--format",
                    "json",
                    "--",
                    "task",
                ]),
            ),
            (
                Edit::Parent,
                Some("TASK-007"),
                &[""],
                run(&["set", "--", "TASK-007", "parent="]),
            ),
            // Unlink reads the item's own links: no kiln.toml needed.
            (
                Edit::Unlink,
                Some("TASK-003"),
                &["part-of MS-001", "yes"],
                run(&["unlink", "--", "TASK-003", "part-of", "MS-001"]),
            ),
        ];
        for (edit, id, answers, want) in table {
            let steps = drive(*edit, *id, answers, &f);
            assert!(
                steps[..steps.len() - 1]
                    .iter()
                    .all(|s| matches!(s, Step::Ask { .. }) || *edit == Edit::Unlink),
                "typed: {steps:#?}"
            );
            assert_eq!(last(&steps), want, "{edit:?}: {steps:#?}");
        }
        assert_eq!(
            drive(Edit::Status, Some("TASK-003"), &[], &f)[0],
            Step::Ask {
                label: "Status of TASK-003 (now todo)".into(),
                initial: String::new()
            }
        );
        assert_eq!(
            drive(Edit::Forward, Some("TASK-003"), &[], &f)[0],
            Step::Say("H and L need kiln.toml's order of task statuses: there's no kiln.toml at the project's top".into())
        );
    }

    #[test]
    fn the_choices_come_from_kiln_toml_and_the_project() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        let labels = |s: &Step| match s {
            Step::Pick { items, .. } => items
                .iter()
                .map(|i| format!("{}|{}|{}", i.label, i.value, i.detail))
                .collect::<Vec<_>>(),
            other => panic!("not a pick: {other:?}"),
        };
        let s = drive(Edit::Status, Some("TASK-002"), &[], &f);
        assert!(matches!(&s[0], Step::Pick { title, .. } if title == "Status of TASK-002"));
        assert_eq!(
            labels(&s[0]),
            [
                "todo|todo|",
                "doing|doing|now",
                "blocked|blocked|",
                "done|done|done",
                "dropped|dropped|closed"
            ]
        );
        // The roles a task may start, then what the role may point at and
        // isn't linked yet.
        let s = drive(Edit::Link, Some("TASK-003"), &["implements"], &f);
        assert_eq!(
            labels(&s[0]),
            [
                "implements|implements|to requirement",
                "part-of|part-of|to milestone",
                "mitigates|mitigates|to risk",
                "depends-on|depends-on|to task, milestone"
            ]
        );
        assert_eq!(
            labels(&s[1]),
            [
                "REQ-001  Readings are logged every second|REQ-001|approved",
                "REQ-002  Logged readings survive a power cut|REQ-002|draft"
            ],
            "REQ-003 is linked already"
        );
        let s = drive(Edit::Link, Some("TASK-003"), &["depends-on"], &f);
        assert!(
            !labels(&s[1]).iter().any(|l| l.starts_with("TASK-003")),
            "not itself"
        );
        // Its links, then no first.
        let s = drive(Edit::Unlink, Some("TASK-003"), &["implements REQ-003"], &f);
        assert_eq!(
            labels(&s[0]),
            [
                "implements  REQ-003|implements REQ-003|The log exports as CSV",
                "part-of  MS-001|part-of MS-001|First release"
            ]
        );
        assert_eq!(labels(&s[1]), ["no, don't unlink|no|", "yes, unlink|yes|"]);
        // Types for a new item, in kiln.toml's order; for a child, those
        // that may have it as parent.
        let s = drive(Edit::New, None, &[], &f);
        assert_eq!(labels(&s[0]).len(), 7);
        assert_eq!(labels(&s[0])[0], "requirement|requirement|REQ-");
        let s = drive(Edit::Child, Some("REQ-001"), &["requirement"], &f);
        assert_eq!(labels(&s[0]), ["requirement|requirement|REQ-"]);
        assert_eq!(
            s[1],
            Step::Ask {
                label: "Title of the new requirement under REQ-001".into(),
                initial: String::new()
            }
        );
        // A parent: none first, the current one marked, never itself or
        // what's under it.
        let s = drive(Edit::Parent, Some("TASK-002"), &[], &f);
        let p = labels(&s[0]);
        assert!(
            !p.iter()
                .any(|l| l.contains("TASK-002|") || l.contains("TASK-007|")),
            "{p:?}"
        );
        assert!(
            p[0].starts_with("TASK-001  "),
            "no (none): it has no parent: {p:?}"
        );
        let s = drive(Edit::Parent, Some("TASK-007"), &[], &f);
        assert_eq!(labels(&s[0])[0], "(none)|(none)|removes the parent");
        assert!(
            labels(&s[0]).contains(&"TASK-002  Write readings to SQLite|TASK-002|now".to_string())
        );
        // What's prefilled: the current value, or the next free ID.
        let s = drive(Edit::Title, Some("TASK-003"), &[], &f);
        assert_eq!(
            s[0],
            Step::Ask {
                label: "Title of TASK-003".into(),
                initial: "Export the log as CSV".into()
            }
        );
        let s = drive(Edit::Owner, Some("TASK-003"), &[], &f);
        assert_eq!(
            s[0],
            Step::Ask {
                label: "Owner of TASK-003 (empty: none)".into(),
                initial: "bo".into()
            }
        );
        let s = drive(Edit::Due, Some("MS-001"), &[], &f);
        assert_eq!(
            s[0],
            Step::Ask {
                label: "Due date of MS-001, YYYY-MM-DD (empty: none)".into(),
                initial: "2030-06-30".into()
            }
        );
        let s = drive(Edit::Renumber, Some("TASK-003"), &["TASK-020"], &f);
        assert_eq!(
            s[0],
            Step::Ask {
                label: "Renumber TASK-003 to".into(),
                initial: "TASK-009".into()
            }
        );
        assert_eq!(
            labels(&s[1]),
            ["no, don't renumber|no|", "yes, renumber|yes|"]
        );
        // The menu: the edits only it reaches first.
        let s = drive(Edit::More, Some("TASK-003"), &[], &f);
        assert_eq!(
            labels(&s[0])[..2],
            ["title|title|".to_string(), "parent|parent|".to_string()]
        );
        assert_eq!(labels(&s[0])[5], "status|status|s");
    }

    #[test]
    fn what_changes_nothing_says_so_and_runs_nothing() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        let said = |edit, id, answers: &[&str]| match last(&drive(edit, Some(id), answers, &f)) {
            Step::Say(s) => s.clone(),
            other => panic!("{edit:?} {id}: {other:?}"),
        };
        assert_eq!(
            said(Edit::Status, "TASK-003", &["todo"]),
            "TASK-003 is todo already"
        );
        assert_eq!(
            said(Edit::Back, "TASK-003", &[]),
            "TASK-003 is at task's first status, todo"
        );
        assert_eq!(
            said(Edit::Title, "TASK-003", &["Export the log as CSV"]),
            "TASK-003's title is as it was"
        );
        assert_eq!(
            said(Edit::Owner, "TASK-003", &["bo"]),
            "TASK-003's owner is as it was"
        );
        assert_eq!(
            said(Edit::Parent, "TASK-007", &["TASK-002"]),
            "TASK-007's parent is as it was"
        );
        assert_eq!(
            said(Edit::Unlink, "TASK-003", &["part-of MS-001", "no"]),
            "kept: TASK-003 part-of MS-001"
        );
        assert_eq!(
            said(Edit::Renumber, "TASK-003", &["TASK-020", "no"]),
            "TASK-003 keeps its ID"
        );
        assert_eq!(
            said(Edit::Renumber, "TASK-003", &["TASK-003"]),
            "TASK-003 is TASK-003 already"
        );
        assert_eq!(
            said(Edit::New, "TASK-003", &["task", "  "]),
            "no title, so nothing was made"
        );
        assert_eq!(
            said(Edit::Unlink, "REQ-003", &[]),
            "REQ-003 has no links of its own (a link is removed from the item it's written on)"
        );
        assert_eq!(
            said(Edit::Link, "RISK-001", &[]),
            "a risk can start no link role in kiln.toml"
        );
        assert_eq!(
            said(Edit::Link, "ADR-001", &["supersedes"]),
            "no decision left for ADR-001 to supersedes"
        );
        assert_eq!(
            said(Edit::Child, "RISK-001", &[]),
            "no type in kiln.toml may have a risk as parent"
        );
        assert_eq!(
            said(Edit::Parent, "RISK-001", &[]),
            "a risk can't have a parent (kiln.toml)"
        );
        // Emptied prompts end it quietly.
        assert_eq!(
            last(&drive(Edit::Status, Some("TASK-003"), &[""], &unread(&l))),
            &Step::Done
        );
        assert_eq!(
            last(&drive(Edit::Renumber, Some("TASK-003"), &[""], &f)),
            &Step::Done
        );
    }

    #[test]
    fn a_row_with_no_item_says_so() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        for edit in [
            Edit::Status,
            Edit::Link,
            Edit::Unlink,
            Edit::Child,
            Edit::More,
            Edit::Back,
        ] {
            let (flow, step) = Flow::start(edit, None, &f);
            assert!(flow.is_none());
            assert_eq!(step, Step::Say("no item here".into()));
            let (flow, step) = Flow::start(edit, Some("TASK-099"), &f);
            assert!(flow.is_none());
            assert_eq!(
                step,
                Step::Say(
                    "no item here: TASK-099 isn't in the project as kiln last read it".into()
                )
            );
        }
        // A new item needs none.
        assert!(Flow::start(Edit::New, None, &f).0.is_some());
    }

    fn ok(stdout: &str) -> Result<Ran, Failure> {
        Ok(Ran {
            code: Some(0),
            stdout: stdout.into(),
            stderr: vec![],
        })
    }

    fn refused(code: i32, file: &str) -> Result<Ran, Failure> {
        Err(Failure::Exit {
            code: Some(code),
            line: fixture(file).trim().to_string(),
        })
    }

    #[test]
    fn done_the_cursor_follows_and_a_new_item_opens() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        let (mut flow, _) = Flow::start(Edit::Forward, Some("TASK-003"), &f);
        let out = flow
            .as_mut()
            .unwrap()
            .edited(&ok(&fixture("edits/set-status.txt")));
        assert_eq!(
            out,
            Outcome {
                step: Step::Done,
                follow: Some("TASK-003".into()),
                open: None
            }
        );

        let (flow, _) = Flow::start(Edit::Renumber, Some("TASK-008"), &f);
        let mut flow = flow.unwrap();
        flow.answer("TASK-020", &f);
        assert_eq!(
            flow.answer("yes", &f),
            run(&["renumber", "--to=TASK-020", "--", "TASK-008"])
        );
        assert!(flow.running());
        let out = flow.edited(&ok(&fixture("edits/renumber.txt")));
        assert_eq!(out.follow.as_deref(), Some("TASK-020"));

        let (flow, _) = Flow::start(Edit::New, None, &f);
        let mut flow = flow.unwrap();
        flow.answer("task", &f);
        flow.answer("Write the manual", &f);
        let out = flow.edited(&ok(&fixture("edits/new.json")));
        assert_eq!(
            out,
            Outcome {
                step: Step::Say("TASK-009: design/tasks/TASK-009-write-the-manual.md".into()),
                follow: Some("TASK-009".into()),
                open: Some(Place {
                    path: "design/tasks/TASK-009-write-the-manual.md".into(),
                    line: None
                }),
            }
        );
        // An answer that can't be read says what kiln printed.
        let (flow, _) = Flow::start(Edit::New, None, &f);
        let mut flow = flow.unwrap();
        flow.answer("risk", &f);
        flow.answer("x", &f);
        let out = flow.edited(&ok(&fixture("edits/new-text.txt")));
        assert!(
            matches!(&out.step, Step::Say(s) if s.starts_with("kiln made the item, but its answer can't be read: ")),
            "{out:?}"
        );
        assert_eq!(out.open, None);
    }

    #[test]
    fn a_refused_typed_value_is_asked_again_with_what_was_typed_a_bounded_number_of_times() {
        let l = demo();
        let c = config::tests::demo();
        let f = facts(&l, &c);
        let (flow, _) = Flow::start(Edit::Due, Some("TASK-003"), &f);
        let mut flow = flow.unwrap();
        assert_eq!(
            flow.answer("soon", &f),
            run(&["set", "--", "TASK-003", "due=soon"])
        );
        let again = Step::Ask {
            label: "Due date of TASK-003, YYYY-MM-DD (empty: none)".into(),
            initial: "soon".into(),
        };
        for _ in 0..MOST_TRIES {
            let out = flow.edited(&refused(2, "edits/set-due-bad.stderr"));
            assert_eq!(out.step, again);
            assert_eq!(out.follow, None);
            flow.answer("soon", &f);
        }
        assert_eq!(
            flow.edited(&refused(2, "edits/set-due-bad.stderr")).step,
            Step::Done,
            "bounded"
        );

        // exit 1, a file changed on disk: the same.
        let (flow, _) = Flow::start(Edit::Title, Some("TASK-003"), &f);
        let mut flow = flow.unwrap();
        flow.answer("Stream the CSV", &f);
        let changed = Err(Failure::Exit { code: Some(1), line: "kiln: design/tasks/TASK-003-export-the-log-as-csv.md changed on disk since it was read".into() });
        assert_eq!(
            flow.edited(&changed).step,
            Step::Ask {
                label: "Title of TASK-003".into(),
                initial: "Stream the CSV".into()
            }
        );

        // Renumber asks for the ID again, not the confirmation.
        let (flow, _) = Flow::start(Edit::Renumber, Some("TASK-008"), &f);
        let mut flow = flow.unwrap();
        flow.answer("TASK-001", &f);
        flow.answer("yes", &f);
        assert_eq!(
            flow.edited(&refused(2, "edits/renumber-taken.stderr")).step,
            Step::Ask {
                label: "Renumber TASK-008 to".into(),
                initial: "TASK-001".into()
            }
        );
        // A new item asks for its title again, with the type kept.
        let (flow, _) = Flow::start(Edit::Child, Some("REQ-001"), &unread(&l));
        let mut flow = flow.unwrap();
        flow.answer("task", &unread(&l));
        flow.answer("Misplaced", &unread(&l));
        let out = flow.edited(&refused(2, "edits/new-child-wrong.stderr"));
        assert_eq!(
            out.step,
            Step::Ask {
                label: "Title of the new task under REQ-001".into(),
                initial: "Misplaced".into()
            }
        );
        assert_eq!(
            flow.answer("Misplaced", &unread(&l)),
            run(&[
                "new",
                "--parent=REQ-001",
                "--title=Misplaced",
                "--format",
                "json",
                "--",
                "task"
            ])
        );

        // Picks only: over, kiln's line stands.
        let (flow, _) = Flow::start(Edit::Status, Some("TASK-003"), &f);
        let mut flow = flow.unwrap();
        flow.answer("dropped", &f);
        assert_eq!(
            flow.edited(&refused(2, "edits/set-status-unknown.stderr"))
                .step,
            Step::Done
        );
        // kiln not run at all: over.
        let (flow, _) = Flow::start(Edit::Owner, Some("TASK-003"), &f);
        let mut flow = flow.unwrap();
        flow.answer("x", &f);
        assert_eq!(flow.edited(&Err(Failure::TimedOut)).step, Step::Done);
    }

    #[test]
    fn an_item_gone_mid_flow_ends_it() {
        let l = demo();
        let c = config::tests::demo();
        let (flow, _) = Flow::start(Edit::Owner, Some("TASK-003"), &facts(&l, &c));
        let mut flow = flow.unwrap();
        let mut items = l.items.clone();
        items.retain(|i| i.id != "TASK-003");
        let gone = Loaded::new(l.version, items, l.plan.clone(), l.findings.clone());
        assert_eq!(
            flow.answer("cy", &facts(&gone, &c)),
            Step::Say("TASK-003 isn't in the project any more".into())
        );
    }

    #[test]
    fn the_next_free_id_keeps_the_width() {
        let l = demo();
        assert_eq!(next_free(&l, "TASK-003"), "TASK-009");
        assert_eq!(next_free(&l, "REQ-1"), "REQ-4");
        assert_eq!(next_free(&l, "NEW-01"), "NEW-01");
        assert_eq!(next_free(&l, "odd"), "odd");
    }

    #[test]
    fn edits_wait_for_a_good_settled_read() {
        let l = demo();
        let loaded = |error| State::Loaded {
            loaded: &l,
            reloading: true,
            error,
        };
        assert_eq!(
            blocked(&loaded(None), true),
            None,
            "a refresh under way is fine"
        );
        assert_eq!(
            blocked(&loaded(None), false).unwrap(),
            "kiln is still reading the last edit back: try again in a moment"
        );
        assert!(blocked(&loaded(Some("kiln: bad")), true)
            .unwrap()
            .ends_with(": kiln: bad"));
        assert_eq!(
            blocked(&State::Loading, true).unwrap(),
            "the project isn't read yet: edits wait for it"
        );
        assert_eq!(
            blocked(&State::NoProject("kiln: no kiln.toml"), true).unwrap(),
            "no edits: kiln: no kiln.toml"
        );
    }

    #[test]
    fn keys_and_names() {
        for (c, e) in [
            ('s', Edit::Status),
            ('H', Edit::Back),
            ('L', Edit::Forward),
            ('l', Edit::Link),
            ('U', Edit::Unlink),
            ('n', Edit::New),
            ('N', Edit::Child),
            ('E', Edit::More),
        ] {
            assert_eq!(Edit::by_key(c), Some(e));
            assert_eq!(Edit::by_name(e.name()), Some(e));
        }
        assert_eq!(Edit::by_key('x'), None);
        for (e, _, _) in MENU {
            assert_eq!(Edit::by_name(e.name()), Some(*e));
        }
    }
}

/// The edits on the plan page, through Fenix's fake: keys, Fenix's own
/// picker and prompt, kiln's recorded answers (`tests/fixtures/out/edits`).
#[cfg(test)]
mod page_tests {
    use crate::api::Key;
    use crate::model::tests::fixture;
    use crate::tests::{at, demo, folder, foot, harness};
    use crate::Kiln;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// The demo opened on the plan, its `kiln.toml` at the project's top
    /// (unless `config` is false: then there's none to read).
    fn opened(fake: &Fake, name: &str, config: bool) -> (Harness<Kiln>, u64) {
        let dir = folder(name);
        if config {
            std::fs::copy(
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo/kiln.toml"),
                dir.join("kiln.toml"),
            )
            .unwrap();
        }
        let mut h = harness(fake, &dir);
        h.size = (110, 26);
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        (h, page)
    }

    /// The cursor put on `id` (the filter typed, then cleared).
    fn on(h: &mut Harness<Kiln>, page: u64, id: &str) {
        h.keys(page, &format!("/{id}<Enter>/<Esc>"));
        assert_eq!(at(h, page).as_deref(), Some(id), "{}", h.screen(page));
    }

    /// Every edit kiln was asked to make so far, after `kiln`.
    fn edits(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter_map(|l| {
                l.strip_prefix("spawn ")
                    .and_then(|l| l.split_once(' '))
                    .map(|(_, l)| l.to_string())
            })
            .filter_map(|l| l.strip_prefix("/usr/bin/kiln ").map(str::to_string))
            .filter(|l| {
                ["set ", "link ", "unlink ", "new ", "renumber "]
                    .iter()
                    .any(|c| l.starts_with(c))
            })
            .collect()
    }

    fn calls_with(fake: &Fake, what: &str) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.contains(what))
            .collect()
    }

    fn txt(name: &str) -> Ran {
        let code: i32 = fixture(&format!("edits/{name}.exit"))
            .trim()
            .parse()
            .unwrap();
        if code == 0 {
            Ran::ok(fixture(&format!("edits/{name}.txt")))
        } else {
            Ran::fails(code, fixture(&format!("edits/{name}.stderr")))
        }
    }

    #[test]
    fn s_picks_a_status_of_its_type_and_sets_it() {
        let fake = demo();
        fake.on_pick("Status of TASK-003", "doing")
            .on_spawn("kiln set -- TASK-003 status=doing", txt("set-status"));
        let (mut h, page) = opened(&fake, "edit-status", true);
        on(&mut h, page, "TASK-003");
        fake.take();
        h.key(page, Key::Char('s'));
        let picks = calls_with(&fake, "pick ");
        assert_eq!(picks.len(), 1);
        assert!(
            picks[0].ends_with("Status of TASK-003: todo, doing, blocked, done, dropped"),
            "{picks:?}"
        );
        assert_eq!(fake.state().pickers[0].items[0].detail, "now");
        assert_eq!(edits(&fake), ["set -- TASK-003 status=doing"]);
        assert_eq!(foot(&mut h, page), "TASK-003: status todo → doing");
        assert_eq!(
            at(&h, page).as_deref(),
            Some("TASK-003"),
            "the cursor follows it"
        );
        assert!(h.ext.answers.is_empty() && h.ext.asked.is_empty());
        // Cancelled: nothing runs.
        fake.on_pick_nothing("Status of TASK-003");
        h.key(page, Key::Char('s'));
        assert_eq!(edits(&fake).len(), 1);
        assert!(h.ext.answers.is_empty());
    }

    #[test]
    fn h_and_l_move_the_status_on_the_board_and_say_so_at_the_ends_and_elsewhere() {
        let fake = demo();
        fake.on_spawn("kiln set -- TASK-003 status=doing", txt("set-status"));
        let (mut h, page) = opened(&fake, "edit-hl", true);
        h.keys(page, "4");
        assert_eq!(at(&h, page).as_deref(), Some("TASK-003"));
        assert!(
            foot(&mut h, page).starts_with("H L move  Enter open"),
            "{}",
            foot(&mut h, page)
        );
        h.key(page, Key::Char('H'));
        assert_eq!(
            foot(&mut h, page),
            "TASK-003 is at task's first status, todo"
        );
        assert!(edits(&fake).is_empty());
        h.key(page, Key::Char('L'));
        assert_eq!(edits(&fake), ["set -- TASK-003 status=doing"]);
        assert_eq!(foot(&mut h, page), "TASK-003: status todo → doing");
        // Not on another tab.
        h.keys(page, "1");
        on(&mut h, page, "TASK-003");
        h.key(page, Key::Char('L'));
        assert_eq!(
            foot(&mut h, page),
            "H and L move an item's status on the Board"
        );
        assert_eq!(edits(&fake).len(), 1);
    }

    #[test]
    fn l_picks_a_role_then_a_target_not_linked_yet() {
        let fake = demo();
        fake.on_pick("Link TASK-003 as", "implements")
            .on_pick(
                "TASK-003 implements",
                "REQ-002  Logged readings survive a power cut",
            )
            .on_spawn("kiln link -- TASK-003 implements REQ-002", txt("link"));
        let (mut h, page) = opened(&fake, "edit-link", true);
        on(&mut h, page, "TASK-003");
        h.key(page, Key::Char('l'));
        let picks = calls_with(&fake, "pick ");
        assert!(
            picks[0].ends_with("Link TASK-003 as: implements, part-of, mitigates, depends-on"),
            "{picks:?}"
        );
        assert!(
            picks[1].ends_with("TASK-003 implements: REQ-001  Readings are logged every second, REQ-002  Logged readings survive a power cut"),
            "REQ-003 is linked already: {picks:?}"
        );
        assert_eq!(edits(&fake), ["link -- TASK-003 implements REQ-002"]);
        assert_eq!(foot(&mut h, page), "TASK-003 implements REQ-002");
        // Cancelled at the target, then at the role.
        fake.on_pick_nothing("TASK-003 implements");
        h.key(page, Key::Char('l'));
        fake.on_pick_nothing("Link TASK-003 as");
        h.key(page, Key::Char('l'));
        assert_eq!(edits(&fake).len(), 1);
        assert!(h.ext.answers.is_empty());
    }

    #[test]
    fn capital_u_picks_a_link_then_asks_no_first() {
        let fake = demo();
        fake.on_pick("Unlink from TASK-003", "implements  REQ-003")
            .on_pick("Remove TASK-003 implements REQ-003?", "no, don't unlink");
        let (mut h, page) = opened(&fake, "edit-unlink", true);
        on(&mut h, page, "TASK-003");
        h.key(page, Key::Char('U'));
        assert!(edits(&fake).is_empty());
        assert_eq!(foot(&mut h, page), "kept: TASK-003 implements REQ-003");
        let picks = calls_with(&fake, "pick ");
        assert!(
            picks[1]
                .ends_with("Remove TASK-003 implements REQ-003?: no, don't unlink, yes, unlink"),
            "{picks:?}"
        );
        fake.on_pick("Remove TASK-003 implements REQ-003?", "yes, unlink")
            .on_spawn(
                "kiln unlink",
                Ran::ok("TASK-003 no longer implements REQ-003\n"),
            );
        h.key(page, Key::Char('U'));
        assert_eq!(edits(&fake), ["unlink -- TASK-003 implements REQ-003"]);
        assert_eq!(foot(&mut h, page), "TASK-003 no longer implements REQ-003");
        // Cancelled at the confirmation: nothing more.
        fake.on_pick_nothing("Remove TASK-003");
        h.key(page, Key::Char('U'));
        assert_eq!(edits(&fake).len(), 1);
    }

    #[test]
    fn n_makes_an_item_and_opens_it_with_the_cursor_on_it() {
        let fake = demo();
        fake.on_pick("New item's type", "task")
            .on_prompt("Title of the new task", "Write the manual")
            .on_spawn("kiln new", Ran::ok(fixture("edits/new.json")));
        let (mut h, page) = opened(&fake, "edit-new", true);
        // The list read after it has the new item.
        let with_it = fixture("demo/list.json").replacen(
            "[\n",
            "[\n  {\"id\": \"TASK-009\", \"type\": \"task\", \"title\": \"Write the manual\", \"status\": \"todo\", \"path\": \"design/tasks/TASK-009-write-the-manual.md\", \"line\": 2},\n",
            1,
        );
        fake.on_spawn("kiln list", Ran::ok(with_it));
        // On a heading: a new item needs no item.
        h.keys(page, "k");
        fake.take();
        h.key(page, Key::Char('n'));
        assert_eq!(
            edits(&fake),
            ["new --title=Write the manual --format json -- task"]
        );
        let opens = calls_with(&fake, "open ");
        assert_eq!(opens.len(), 1, "{opens:?}");
        assert!(
            opens[0].ends_with("/design/tasks/TASK-009-write-the-manual.md"),
            "{opens:?}"
        );
        assert_eq!(
            foot(&mut h, page),
            "TASK-009: design/tasks/TASK-009-write-the-manual.md"
        );
        assert_eq!(at(&h, page).as_deref(), Some("TASK-009"));
        // Cancelled at the title: nothing made.
        fake.on_prompt_nothing("Title of the new task");
        h.key(page, Key::Char('n'));
        assert_eq!(edits(&fake).len(), 1);
    }

    #[test]
    fn capital_n_offers_the_types_that_may_have_it_as_parent() {
        let fake = demo();
        fake.on_pick("New child of TASK-002", "task")
            .on_prompt(
                "Title of the new task under TASK-002",
                "-v, a title like an option",
            )
            .on_spawn("kiln new", Ran::ok(fixture("edits/new-dash.json")));
        let (mut h, page) = opened(&fake, "edit-child", true);
        on(&mut h, page, "TASK-002");
        h.key(page, Key::Char('N'));
        assert!(calls_with(&fake, "pick ")[0].ends_with("New child of TASK-002: task"));
        assert_eq!(
            edits(&fake),
            ["new --parent=TASK-002 --title=-v, a title like an option --format json -- task"]
        );
        // A risk may be no type's parent.
        on(&mut h, page, "RISK-001");
        h.key(page, Key::Char('N'));
        assert_eq!(
            foot(&mut h, page),
            "no type in kiln.toml may have a risk as parent"
        );
    }

    #[test]
    fn a_refused_date_is_asked_again_with_what_was_typed_and_not_forever() {
        let fake = demo();
        fake.on_pick("Edit TASK-003", "due date")
            .on_prompt("Due date of TASK-003", "soon")
            .on_spawn("kiln set -- TASK-003 due=soon", txt("set-due-bad"));
        let (mut h, page) = opened(&fake, "edit-due", true);
        on(&mut h, page, "TASK-003");
        fake.take();
        h.key(page, Key::Char('E'));
        let prompts = calls_with(&fake, "prompt ");
        assert!(
            prompts[0].ends_with("Due date of TASK-003, YYYY-MM-DD (empty: none) []"),
            "{prompts:?}"
        );
        assert!(
            prompts[1].ends_with("Due date of TASK-003, YYYY-MM-DD (empty: none) [soon]"),
            "again, with what was typed: {prompts:?}"
        );
        // The fake types `soon` every time: asked again three times, then over.
        assert_eq!(edits(&fake).len(), 1 + super::MOST_TRIES as usize);
        assert_eq!(prompts.len(), 1 + super::MOST_TRIES as usize);
        assert!(h.ext.answers.is_empty() && h.ext.asked.is_empty());
        assert_eq!(foot(&mut h, page), "kiln: `soon` isn't a date (2026-11-30)");
        // In the error look.
        assert!(
            h.marked(page)
                .contains("«bad:kiln: `soon` isn't a date (2026-11-30)»"),
            "{}",
            h.marked(page)
        );
        // Typed right the next time: done, once.
        fake.on_prompt("Due date of TASK-003", "2030-01-31")
            .on_spawn("kiln set -- TASK-003 due=2030-01-31", txt("set-due"));
        h.key(page, Key::Char('E'));
        assert_eq!(
            edits(&fake).last().unwrap(),
            "set -- TASK-003 due=2030-01-31"
        );
        assert_eq!(foot(&mut h, page), "TASK-003: due (none) → 2030-01-31");
    }

    #[test]
    fn renumber_asks_the_new_id_then_yes_and_the_cursor_follows_it() {
        let fake = demo();
        fake.on_pick("Edit TASK-008", "renumber")
            .on_prompt("Renumber TASK-008 to", "TASK-020")
            .on_pick("Renumber TASK-008 to TASK-020?", "yes, renumber")
            .on_spawn("kiln renumber", txt("renumber"));
        let (mut h, page) = opened(&fake, "edit-renumber", true);
        on(&mut h, page, "TASK-008");
        fake.on_spawn(
            "kiln list",
            Ran::ok(fixture("demo/list.json").replace("TASK-008", "TASK-020")),
        );
        fake.take();
        h.key(page, Key::Char('E'));
        assert!(
            calls_with(&fake, "prompt ")[0].ends_with("Renumber TASK-008 to [TASK-009]"),
            "the next free ID offered"
        );
        assert_eq!(edits(&fake), ["renumber --to=TASK-020 -- TASK-008"]);
        assert!(foot(&mut h, page).starts_with("TASK-008 → TASK-020: 0 references"));
        assert_eq!(at(&h, page).as_deref(), Some("TASK-020"));
        // "no" renumbers nothing.
        fake.on_pick("Edit TASK-020", "renumber")
            .on_prompt("Renumber TASK-020 to", "TASK-030")
            .on_pick("Renumber TASK-020 to TASK-030?", "no, don't renumber");
        h.key(page, Key::Char('E'));
        assert_eq!(edits(&fake).len(), 1);
        assert_eq!(foot(&mut h, page), "TASK-020 keeps its ID");
    }

    #[test]
    fn a_row_with_no_item_says_so() {
        let fake = demo();
        let (mut h, page) = opened(&fake, "edit-noid", true);
        // Check's rows are findings, not items.
        h.keys(page, "9");
        for k in ['s', 'l', 'U', 'N', 'E'] {
            h.key(page, Key::Char(k));
            assert_eq!(foot(&mut h, page), "no item here", "{k}");
        }
        assert!(calls_with(&fake, "pick ").is_empty() && calls_with(&fake, "prompt ").is_empty());
    }

    #[test]
    fn without_kiln_toml_the_status_is_typed() {
        let fake = demo();
        fake.on_prompt("Status of TASK-003 (now todo)", "doing")
            .on_spawn("kiln set -- TASK-003 status=doing", txt("set-status"));
        let (mut h, page) = opened(&fake, "edit-unread", false);
        on(&mut h, page, "TASK-003");
        h.key(page, Key::Char('s'));
        assert!(calls_with(&fake, "pick ").is_empty());
        assert_eq!(edits(&fake), ["set -- TASK-003 status=doing"]);
        // H and L say why they can't.
        h.keys(page, "4");
        h.key(page, Key::Char('L'));
        assert_eq!(
            foot(&mut h, page),
            "H and L need kiln.toml's order of task statuses: there's no kiln.toml at the project's top"
        );
        // A refused status typed is asked again with it.
        fake.on_prompt("Status of TASK-003 (now todo)", "finished")
            .on_spawn(
                "kiln set -- TASK-003 status=finished",
                txt("set-status-unknown"),
            );
        h.keys(page, "1");
        on(&mut h, page, "TASK-003");
        fake.take();
        h.key(page, Key::Char('s'));
        let prompts = calls_with(&fake, "prompt ");
        assert!(
            prompts[1].ends_with("Status of TASK-003 (now todo) []"),
            "{prompts:?}"
        );
        assert!(
            prompts[2..].iter().all(|p| p.ends_with("[finished]")),
            "{prompts:?}"
        );
        assert_eq!(prompts.len(), 2 + super::MOST_TRIES as usize);
    }

    #[test]
    fn no_edit_starts_before_kiln_reads_the_last_one_back() {
        let fake = demo();
        fake.on_pick("Status of TASK-003", "doing")
            .on_spawn("kiln set -- TASK-003 status=doing", txt("set-status"));
        let (mut h, page) = opened(&fake, "edit-stale", true);
        on(&mut h, page, "TASK-003");
        fake.on_spawn("kiln list", Ran::running(""));
        h.key(page, Key::Char('s'));
        assert_eq!(edits(&fake).len(), 1);
        // The read after it runs: the next edit waits, and says why.
        h.key(page, Key::Char('s'));
        assert_eq!(
            foot(&mut h, page),
            "kiln is still reading the last edit back: try again in a moment"
        );
        assert_eq!(edits(&fake).len(), 1);
        // Read back: it starts.
        let list = fake.process("kiln list").unwrap().id;
        fake.on_spawn("kiln list", Ran::ok(fixture("demo/list.json")));
        for line in fixture("demo/list.json").lines() {
            h.said(list, line, false);
        }
        h.exited(list, Some(0));
        h.key(page, Key::Char('s'));
        assert_eq!(edits(&fake).len(), 2);
    }

    #[test]
    fn kiln_toml_saved_is_read_again() {
        let fake = demo();
        let (mut h, page) = opened(&fake, "edit-saved", false);
        on(&mut h, page, "TASK-003");
        fake.on_prompt_nothing("Status of");
        h.key(page, Key::Char('s'));
        assert_eq!(calls_with(&fake, "prompt ").len(), 1, "typed: no kiln.toml");
        let dir = h.ext.root.clone().unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo/kiln.toml"),
            dir.join("kiln.toml"),
        )
        .unwrap();
        h.saved(&dir.join("kiln.toml").display().to_string());
        fake.on_pick_nothing("Status of TASK-003");
        h.key(page, Key::Char('s'));
        assert_eq!(calls_with(&fake, "pick ").len(), 1, "picked now");
    }
}
