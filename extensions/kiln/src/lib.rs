//! Kiln in Fenix: a Kiln project's plan on pages, edits to its items,
//! exports and read tools for coding agents -- all by running the `kiln`
//! program (`--format json` to read; `set`, `link` and `new` to change).
//!
//! What it deliberately doesn't do: it never writes an item file itself,
//! compiles none of Kiln in, reaches no network and holds no Confluence
//! or Jira credential. Kiln's own checks and edits are the only ones;
//! this extension shows what kiln said and asks kiln to act.
//!
//! The parts that decide things are plain code, tested natively: `kiln`
//! (the queue that runs kiln), `model` (what kiln's JSON says), `project`
//! (kiln found, the project read, when to read it again), `lsp` (whether
//! and how Kiln's language server is registered), `doctor` (the project
//! doctor's rows), `tasks` (the task picker's), `modeline` (the item's
//! text) and `pages` (the pages, from the project's state). This file
//! meets Fenix: it hands Fenix's events to the project, passes on what the
//! project says changed, routes each query's answer to whoever asked it
//! ([`For`]), does what `lsp` decides, and gives Fenix the doctor's rows,
//! the tasks and the modeline text -- each only when it differs from what
//! it gave last. It stays the thin part that talks to the host.
//!
//! Pages ask the user through it too: a tab's prompt or picker
//! ([`pages::frame::Effect`]) is Fenix's own, asked here, and its answer
//! routed back to the page, tab and view that asked ([`Asker`]); a tab's
//! edit runs through the project's queue like a query ([`For::Edit`]).
//! The edits' flows ([`edit`]) offer choices from the project's
//! `kiln.toml`, read here ([`Kiln::read_config`]) when the extension
//! loads, on `r`, and when `kiln.toml` is saved.
//! A tab's `kiln export` (the Exports tab's `w`, `W` and `P`) runs through
//! the queue too, as a query with its own limit ([`For::Run`]): it writes
//! generated files or publishes, never an item, so no reload follows. A
//! publish is one at a time for the whole extension ([`Kiln::publishing`]):
//! a second is answered at once that one runs, and every plan page's
//! bottom line says it runs. `kiln auth confluence` opens in a terminal
//! pane, the token typed into kiln there; this file never reads a secret,
//! prompts for one, or reads the environment.
//! The commands that start in a buffer (`kiln.show`, `kiln.impact`,
//! `kiln.find`) take what `here()` and `cursor()` say as they run
//! ([`spot`]) and act once the project is read ([`Wanted`]).
//! The edits start from a buffer too (`kiln.set_status`, `kiln.link`,
//! `kiln.unlink`, `kiln.new`, `kiln.new_child`, `kiln.edit`): the same
//! flows, held one at a time by the extension ([`edit::Driver`], the page
//! frame's own), asking through Fenix's picker and prompt ([`Asker::Here`])
//! and saying what kiln said on the message line, there being no page. It
//! doesn't reload the file a flow edits: kiln rewrites it, and Fenix
//! re-reads a clean buffer that changed on disk by itself.
//!
//! The project is read once as the extension loads (a `kiln.toml` at the
//! project's top, or a `kiln.*` command): the language server is wanted
//! from the start, not only once a page opens, and deciding it needs kiln
//! found and the project read.

pub mod config;
pub mod doctor;
pub mod edit;
pub mod kiln;
pub mod lsp;
pub mod model;
pub mod modeline;
pub mod pages;
pub mod project;
pub mod spot;
pub mod tasks;

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::{
    page as pages_api, Event, Extension, Key, KeyPress, KeyScheme, MouseEvent, Page, PickerItem,
};
use edit::{Edit, Facts, Step};
use lsp::Lsp;
use pages::frame::{Effect, Hints, Now, Plan};
use pages::view::Choice;
use pages::Tab;
use project::{Notice, Project, QueryId, State, Why};
use spot::Spot;
use tasks::Offer;

/// The page kind the plan opens as (the manifest's context names it).
const PLAN: &str = "plan";

/// The open project's folder, as `std::fs` sees it (`filesystem =
/// "project-read"`).
const PROJECT: &str = "/project";

/// Who asked a query ([`Project::query`]), so its answer goes to them.
/// One map for every asker, handed its answers in [`Kiln::pass_on`]. An
/// answer no one's waiting for -- its page closed -- is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
enum For {
    /// `kiln lsp --languages`, for [`Lsp`].
    Languages,
    /// A plan page's tab: its view's query `tag`
    /// ([`pages::view::View::queries`]).
    Plan { page: u64, tab: Tab, tag: String },
    /// A plan page's tab: the edit its view asked for with `tag`
    /// ([`pages::view::Reply::Edit`]).
    Edit { page: u64, tab: Tab, tag: String },
    /// The edit of the buffer flow `serial` ([`Kiln::here`]).
    Here { serial: u64 },
    /// A plan page's tab: the command its view asked for with `tag`
    /// ([`pages::view::Reply::Run`]).
    Run { page: u64, tab: Tab, tag: String },
}

/// The publish running: its query, and the Confluence target it publishes.
struct Publishing {
    id: QueryId,
    target: String,
}

/// Who asked Fenix's prompt or picker, so the answer goes to them. Like
/// [`For`], an answer no one's waiting for -- the page closed -- is
/// dropped, and so is a prompt or picker closed with nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asker {
    /// A plan page's tab: its view's `tag` ([`pages::view::Reply::Ask`],
    /// [`pages::view::Reply::Pick`]).
    View { page: u64, tab: Tab, tag: String },
    /// `kiln.find`'s picker: the item picked goes to the Item tab.
    Find,
    /// A pick or prompt of the buffer flow `serial` ([`Kiln::here`]); an
    /// answer for a flow since replaced is dropped by its serial.
    Here { serial: u64 },
}

/// A command from a buffer waiting for the project to be read, so it can
/// tell an ID from a word: what `here()` said as it ran (it says nothing
/// once the command is over), and where to take the item.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wanted {
    /// `kiln.show` (`Tab::Item`) or `kiln.impact` (`Tab::Impact`).
    Item { spot: Spot, to: Tab },
    /// `kiln.find`: the picker over every item.
    Find,
    /// An edit from a buffer (`kiln.set_status`, `kiln.link`, `kiln.new`,
    /// `kiln.edit`…): started on the item at `spot`, or on none for a new
    /// item, once the project is read.
    Edit { spot: Spot, edit: Edit },
}

/// The most rows `kiln.find`'s picker is given. Fenix sets its picker no
/// limit (its host passes the rows on as they come); this is the bound of
/// Fenix's other lists (`folder_list`, decorations), far past Kiln's
/// budget of 2,000 items, so a project past it is cut, and said. A tab's
/// own picks choose among fewer (a type's statuses, its roles).
const MOST_CHOICES: usize = 10_000;

/// One plan page: whether it's in view, and its tabs.
struct PlanPage {
    shown: bool,
    plan: Plan,
}

/// What was last given to Fenix, so each of the three is given again only
/// when it changes. `None`: not given yet, so the first decision is always
/// given, even when it is "nothing" (it clears what an earlier run left).
#[derive(Default)]
struct Told {
    doctor: Option<Vec<api::Check>>,
    /// The kiln program the tasks run, `None` inside for none offered.
    tasks: Option<Option<String>>,
    /// The modeline's text, `None` inside for no item.
    modeline: Option<Option<String>>,
}

/// How many rounds [`Kiln::pass_on`] goes before it stops: each round
/// hands on what came and may ask once more; a few settle anything real.
const MOST_ROUNDS: usize = 16;

#[derive(Default)]
struct Kiln {
    project: Project,
    /// Its plan pages: several may be open, in different panes, each
    /// with its own tab, cursors and filters.
    pages: HashMap<u64, PlanPage>,
    lsp: Lsp,
    /// The queries waiting for an answer, and who for.
    asked: HashMap<QueryId, For>,
    /// Fenix's prompts and pickers waiting for an answer, and who for.
    answers: HashMap<u64, Asker>,
    /// A command from a buffer, waiting for the project to be read.
    wanted: Option<Wanted>,
    /// The edit flow started from a buffer, if one is under way: one at a
    /// time for the extension (a page holds its own), a new one replaces
    /// it. Its prompts and picks are Fenix's own ([`Asker::Here`]) and what
    /// it says goes to the message line, there being no page.
    here: edit::Driver,
    /// The project's folder when it isn't [`PROJECT`]: a test's.
    root: Option<PathBuf>,
    /// `kiln.toml` as last read ([`config`]), or why it couldn't be;
    /// `None` before the first read.
    config: Option<Result<config::Config, String>>,
    /// The language server's inputs moved (a save, `r`, another kiln)
    /// without the project saying anything: decide it on the next
    /// [`Kiln::pass_on`].
    decide: bool,
    told: Told,
    /// The one publish that may run ([`pages::view::Reply::Run`] with
    /// `publish`), from when it's asked until its answer comes -- even if
    /// its page closes meanwhile.
    publishing: Option<Publishing>,
}

/// The project now, for a page: its state, its reads, `kiln.toml`,
/// whether every edit has been read back, its change count and the
/// publish running. A function of the fields, not a method, so a page
/// (another field) can be borrowed beside them.
fn now_of<'a>(
    project: &'a Project,
    config: &'a Option<Result<config::Config, String>>,
    publishing: &'a Option<Publishing>,
) -> Now<'a> {
    Now {
        state: project.state(),
        loads: project.loads(),
        config: match config {
            Some(Ok(c)) => Ok(c),
            Some(Err(why)) => Err(why.as_str()),
            None => Err("kiln.toml hasn't been read yet"),
        },
        settled: project.settled(),
        changes: project.changes(),
        publishing: publishing.as_ref().map(|p| p.target.as_str()),
    }
}

impl Kiln {
    /// `args` asked of kiln for `who`.
    fn ask<S: Into<String>>(&mut self, args: impl IntoIterator<Item = S>, who: For) -> QueryId {
        let id = self.project.query(args);
        self.asked.insert(id, who);
        id
    }

    /// What the project said, passed on: a change draws its pages again,
    /// an answer goes to whoever asked it. Then, when something came (or
    /// [`Kiln::decide`] says the server's inputs moved), the language
    /// server is decided again, which may ask kiln something whose answer
    /// may already be here -- so round again until nothing more comes.
    /// Not on every call: a line of kiln's output brings no notice, and
    /// deciding looks for the kiln program.
    fn pass_on(&mut self) {
        if self.config.is_none() {
            self.read_config();
        }
        let mut changed = false;
        let mut decide = std::mem::take(&mut self.decide);
        // Anything that can move what Fenix is told: not a line of kiln's
        // output, which brings neither a notice nor a decision.
        let mut touched = decide;
        for _ in 0..MOST_ROUNDS {
            let notices = self.project.take_notices();
            touched |= !notices.is_empty();
            decide |= !notices.is_empty();
            for notice in notices {
                match notice {
                    Notice::Changed => changed = true,
                    Notice::Answer { id, result, .. } => match self.asked.remove(&id) {
                        Some(For::Languages) => {
                            self.lsp.answered(id, &result, self.project.version())
                        }
                        Some(For::Plan { page, tab, tag }) => {
                            if let Some(p) = self.pages.get_mut(&page) {
                                if p.plan.answered(tab, &tag, id, &result) {
                                    pages_api::refresh(page);
                                }
                            }
                        }
                        Some(For::Edit { page, tab, tag }) => {
                            let now = now_of(&self.project, &self.config, &self.publishing);
                            if let Some(p) = self.pages.get_mut(&page) {
                                let effects = p.plan.edited(tab, &tag, &result, now);
                                pages_api::refresh(page);
                                self.perform(page, effects);
                            }
                        }
                        Some(For::Here { serial }) => self.here_edited(serial, &result),
                        Some(For::Run { page, tab, tag }) => {
                            if self.publishing.as_ref().is_some_and(|p| p.id == id) {
                                self.publishing = None;
                                // Every page's bottom line said it ran.
                                changed = true;
                            }
                            let now = now_of(&self.project, &self.config, &self.publishing);
                            if let Some(p) = self.pages.get_mut(&page) {
                                let effects = p.plan.ran(tab, &tag, &result, now);
                                pages_api::refresh(page);
                                self.perform(page, effects);
                            }
                        }
                        None => {}
                    },
                }
            }
            // A command from a buffer, once the project's read.
            let resolved = self.resolve_wanted();
            // A page's tab may want kiln asked something: on a new read,
            // a tab shown, an item handed on. Its answer may be here at
            // once (kiln unusable): another round.
            let asked = self.ask_for_pages();
            if !decide && !asked && !resolved {
                break;
            }
            // Asking brings an answer, maybe at once: another round.
            if decide {
                decide = self.decide_lsp();
            }
        }
        if changed {
            for &p in self.pages.keys() {
                pages_api::refresh(p);
            }
        }
        if touched {
            self.publish();
        }
    }

    /// The doctor's rows, the tasks and the modeline item, given to Fenix
    /// when they differ from what it was last given. Each is worked out
    /// by a pure function ([`doctor::rows`], [`tasks::offer`],
    /// [`modeline::line`]); a state still being found out leaves what was
    /// given as it is.
    fn publish(&mut self) {
        let state = self.project.state();
        let usability = lsp::usability(&state);
        // The kiln the queue runs; looked for only in a usable project.
        let program = if usability == lsp::Usability::Usable {
            kiln::program()
        } else {
            Err(String::new())
        };
        let kiln_path = api::setting("kiln.path")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let rows = doctor::rows(&doctor::Input {
            state,
            lsp: self.lsp.status(),
            kiln_path: kiln_path.as_deref(),
            program: program.as_deref().ok(),
            version: self.project.version(),
        });
        let offer = tasks::offer(&usability, program);
        let line = modeline::line(&state);

        if let Some(rows) = rows {
            if self.told.doctor.as_ref() != Some(&rows) {
                api::project::doctor(&rows);
                self.told.doctor = Some(rows);
            }
        }
        let wanted = match offer {
            Offer::Keep => None,
            Offer::Clear => Some(None),
            Offer::With(program) => Some(Some(program)),
        };
        if let Some(program) = wanted {
            if self.told.tasks.as_ref() != Some(&program) {
                let tasks = program.as_deref().map(tasks::tasks).unwrap_or_default();
                match api::project::tasks(&tasks) {
                    Ok(()) => self.told.tasks = Some(program),
                    Err(why) => api::warn!("Fenix refused kiln's tasks: {why}"),
                }
            }
        }
        let text = match line {
            modeline::Line::Keep => None,
            modeline::Line::None => Some(None),
            modeline::Line::Text(text) => Some(Some(text)),
        };
        if let Some(text) = text {
            if self.told.modeline.as_ref() != Some(&text) {
                api::status(text.as_deref());
                self.told.modeline = Some(text);
            }
        }
    }

    /// What [`Lsp`] decides, done: whether it asked kiln.
    fn decide_lsp(&mut self) -> bool {
        let usability = lsp::usability(&self.project.state());
        let usable = usability == lsp::Usability::Usable;
        if usable && self.lsp.wants_tools() {
            self.lsp.tools(lsp::listed(self.read_tools()));
        }
        // The kiln the queue runs; looked for only when it's used.
        let program = if usable {
            kiln::program()
        } else {
            Err(String::new())
        };
        let step = self.lsp.step(&usability, program);
        for action in step.actions {
            match action {
                lsp::Action::Say(line) => api::info!("{line}"),
                lsp::Action::Register(server) => {
                    let languages: Vec<&str> =
                        server.languages.iter().map(String::as_str).collect();
                    let command = api::Command::new(&server.program).args(lsp::ARGS);
                    if let Err(why) =
                        api::project::add_language_server(lsp::NAME, &languages, Some(&command))
                    {
                        api::warn!("Fenix refused kiln's language server: {why}");
                        self.lsp.refused(server, why);
                    }
                }
                lsp::Action::Unregister(languages) => {
                    let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
                    // Taking one away can't be refused for a reason this
                    // could act on.
                    let _ = api::project::add_language_server(lsp::NAME, &languages, None);
                }
            }
        }
        if step.ask {
            let id = self.ask(lsp::ASK, For::Languages);
            self.lsp.asked(id);
        }
        step.ask
    }

    /// The project's `.fenix/tools.json`: its text, `None` when there's
    /// none, or why it can't be read. Fenix reads the same file for its
    /// own servers; this only looks at whether it runs kiln.
    fn read_tools(&self) -> Result<Option<String>, String> {
        let root = self.root.clone().unwrap_or_else(|| PathBuf::from(PROJECT));
        let path = root.join(".fenix").join("tools.json");
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(m) if m.len() > lsp::MAX_TOOLS_JSON => {
                return Err(format!("it's over {} KiB", lsp::MAX_TOOLS_JSON / 1024))
            }
            Ok(_) => {}
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The project's `kiln.toml`, read for the edits' choices: kept, and
    /// the pages told when it differs from what was read before. One that
    /// isn't at the project's top, is too large or can't be read is
    /// [`Err`] with why, and the edits ask for values as text.
    fn read_config(&mut self) {
        let root = self.root.clone().unwrap_or_else(|| PathBuf::from(PROJECT));
        let path = root.join("kiln.toml");
        let read = match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err("there's no kiln.toml at the project's top".to_string())
            }
            Err(e) => Err(format!("kiln.toml can't be read: {e}")),
            Ok(m) if m.len() > config::MAX_KILN_TOML => Err(format!(
                "kiln.toml is over {} KiB",
                config::MAX_KILN_TOML / 1024
            )),
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(text) => config::parse(&text),
                Err(e) => Err(format!("kiln.toml can't be read: {e}")),
            },
        };
        if self.config.as_ref() != Some(&read) {
            let first = self.config.is_none();
            self.config = Some(read);
            if !first {
                for (&id, p) in self.pages.iter_mut() {
                    p.plan.reconfigure();
                    pages_api::refresh(id);
                }
            }
        }
    }

    /// `r`, `kiln.reload`: the project read again, its `kiln.toml` too,
    /// and the language server's languages and tools.json looked at again.
    fn reload(&mut self) {
        self.read_config();
        self.lsp.again();
        self.decide = true;
        self.project.reload(Why::Asked);
    }

    fn shown_changed(&mut self) {
        let any = self.pages.values().any(|p| p.shown);
        self.project.set_shown(any);
    }

    /// What each page in view wants asked of kiln, asked: whether any was.
    /// A page out of view asks once it's shown again.
    fn ask_for_pages(&mut self) -> bool {
        let now = now_of(&self.project, &self.config, &self.publishing);
        let mut wanted = Vec::new();
        for (&page, p) in self.pages.iter_mut().filter(|(_, p)| p.shown) {
            wanted.extend(p.plan.wants(now).into_iter().map(|a| (page, a)));
        }
        let asked = !wanted.is_empty();
        for (page, ask) in wanted {
            let who = For::Plan {
                page,
                tab: ask.tab,
                tag: ask.tag.clone(),
            };
            let id = self
                .project
                .query_with(ask.args.clone(), ask.limit_ms, false);
            self.asked.insert(id, who);
            if let Some(p) = self.pages.get_mut(&page) {
                p.plan.asked(
                    &ask,
                    id,
                    now_of(&self.project, &self.config, &self.publishing),
                );
            }
        }
        asked
    }

    /// Something done on plan page `page` -- a key, a click -- and what it
    /// asks of Fenix done.
    fn on_page(&mut self, page: u64, act: impl FnOnce(&mut Plan, Now) -> Vec<Effect>) {
        if self.config.is_none() {
            self.read_config();
        }
        let now = now_of(&self.project, &self.config, &self.publishing);
        let Some(p) = self.pages.get_mut(&page) else {
            return;
        };
        let effects = act(&mut p.plan, now);
        self.perform(page, effects);
        self.pass_on();
    }

    /// What plan page `page` asks of Fenix or kiln, done. A prompt's or a
    /// picker's id, and an edit's, is kept with who asked, for the answer.
    fn perform(&mut self, page: u64, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Open(place) => self.open_place(&place),
                Effect::Reload => self.reload(),
                Effect::Ask {
                    tab,
                    tag,
                    label,
                    initial,
                } => {
                    let id = api::prompt(&label, &initial);
                    self.answers.insert(id, Asker::View { page, tab, tag });
                }
                Effect::Pick {
                    tab,
                    tag,
                    title,
                    items,
                } => {
                    let items = picker_items(&items);
                    let id = api::pick(&title, &items);
                    self.answers.insert(id, Asker::View { page, tab, tag });
                }
                Effect::Edit {
                    tab,
                    tag,
                    args,
                    stdin,
                } => {
                    let id = self.project.edit(args, stdin);
                    self.asked.insert(id, For::Edit { page, tab, tag });
                }
                Effect::Run {
                    tab,
                    tag,
                    args,
                    limit_ms,
                    publish,
                } => self.run(page, tab, tag, args, limit_ms, publish),
                Effect::Terminal { title, args } => self.terminal(page, &title, args),
            }
        }
    }

    /// A tab's command that changes no item, in kiln's queue; its answer
    /// to the tab ([`For::Run`]). One that publishes waits for no other
    /// publish: while one runs it is answered at once that it does, and
    /// nothing is run. It isn't stopped when another kiln is chosen (a
    /// publish stopped half done leaves Confluence half updated).
    fn run(
        &mut self,
        page: u64,
        tab: Tab,
        tag: String,
        args: Vec<String>,
        limit_ms: Option<u32>,
        publish: Option<String>,
    ) {
        if let (Some(_), Some(running)) = (&publish, &self.publishing) {
            let why = Err(kiln::Failure::Unusable(format!(
                "{} is being published: one publish at a time",
                running.target
            )));
            let now = now_of(&self.project, &self.config, &self.publishing);
            if let Some(p) = self.pages.get_mut(&page) {
                let effects = p.plan.ran(tab, &tag, &why, now);
                pages_api::refresh(page);
                self.perform(page, effects);
            }
            return;
        }
        let id = self.project.query_with(args, limit_ms, publish.is_some());
        self.asked.insert(id, For::Run { page, tab, tag });
        if let Some(target) = publish {
            self.publishing = Some(Publishing { id, target });
            for &p in self.pages.keys() {
                pages_api::refresh(p);
            }
        }
    }

    /// kiln with `args` in a terminal pane under the focused one: what is
    /// typed into kiln (`kiln auth confluence`'s token) stays between the
    /// person and kiln. The kiln is the one the queue runs; when there's
    /// none, or Fenix won't open the pane, the page says why.
    fn terminal(&mut self, page: u64, title: &str, args: Vec<String>) {
        let opened = kiln::program().and_then(|program| {
            let cmd = api::Command::new(&program).args(args);
            api::host::run_in_terminal(&cmd, title, true)
                .map_err(|why| format!("Fenix didn't open a terminal for {title}: {why}"))
        });
        if let Err(why) = opened {
            if let Some(p) = self.pages.get_mut(&page) {
                p.plan.tell(&why, true);
                pages_api::refresh(page);
            }
        }
    }

    /// A file of the project opened at a line, in Fenix.
    fn open_place(&self, place: &pages::view::Place) {
        let root = self.root.clone().unwrap_or_else(|| PathBuf::from(PROJECT));
        let path = root.join(&place.path);
        api::host::open_file(&path.to_string_lossy(), place.line);
    }

    /// A prompt's text or a picker's choice, `answer`, to whoever asked
    /// `id`; nothing when it was closed with nothing, or its page closed.
    fn answered(&mut self, id: u64, answer: Option<String>, picked: bool) {
        let Some(who) = self.answers.remove(&id) else {
            return;
        };
        let Some(answer) = answer else {
            // Closed with nothing: a buffer flow ends there.
            if let Asker::Here { serial } = who {
                self.here.cancel(serial);
            }
            return;
        };
        let now = now_of(&self.project, &self.config, &self.publishing);
        match who {
            Asker::View { page, tab, tag } => {
                let Some(p) = self.pages.get_mut(&page) else {
                    return;
                };
                let effects = if picked {
                    p.plan.picked(tab, &tag, &answer, now)
                } else {
                    p.plan.typed(tab, &tag, &answer, now)
                };
                pages_api::refresh(page);
                self.perform(page, effects);
            }
            Asker::Find => self.show_item(&answer, Tab::Item),
            Asker::Here { serial } => {
                let State::Loaded { loaded, .. } = now.state else {
                    self.here.cancel(serial);
                    api::notify(&format!("Kiln: {}", edit::DROPPED));
                    return;
                };
                let facts = Facts {
                    loaded,
                    config: now.config,
                };
                if let Some(turn) = self.here.answer(serial, &answer, &facts) {
                    self.here_turn(turn);
                }
            }
        }
    }

    /// A buffer flow's turn, done: a new item's file opened, then Fenix's
    /// picker or prompt, kiln's command, or the flow's word on the message
    /// line (which ends it). The pick, prompt and edit carry the flow's
    /// serial, so what comes back after another flow has replaced it is
    /// dropped.
    fn here_turn(&mut self, turn: edit::Turn) {
        if let Some(place) = &turn.open {
            self.open_place(place);
        }
        let serial = self.here.serial();
        match turn.step {
            Step::Pick { title, items } => {
                let id = api::pick(&title, &picker_items(&items));
                self.answers.insert(id, Asker::Here { serial });
            }
            Step::Ask { label, initial } => {
                let id = api::prompt(&label, &initial);
                self.answers.insert(id, Asker::Here { serial });
            }
            Step::Run(args) => {
                let id = self.project.edit(args, None);
                self.asked.insert(id, For::Here { serial });
            }
            Step::Say(text) => api::notify(&format!("Kiln: {text}")),
            Step::Done => {}
        }
    }

    /// How kiln's run for buffer flow `serial` went, said on the message
    /// line: kiln's outcome line (its error line as it wrote it), or the
    /// flow's own word where it has one (a new item's path: kiln's answer
    /// then is JSON, which isn't for people). Said even when the flow has
    /// been replaced -- kiln did it -- but only a flow still under way goes
    /// on (a refused typed value is asked again).
    fn here_edited(&mut self, serial: u64, result: &Result<kiln::Ran, kiln::Failure>) {
        let readable = matches!(self.project.state(), State::Loaded { .. });
        let ended = self.here.edited(serial, result, readable);
        let own_word = matches!(ended.as_ref().map(|e| &e.turn.step), Some(Step::Say(_)));
        if !own_word {
            let (line, role) = pages::frame::outcome(result);
            api::notify(&if role == fenix_ui_kit::Role::Bad {
                line
            } else {
                format!("Kiln: {line}")
            });
        }
        if let Some(ended) = ended {
            self.here_turn(ended.turn);
        }
    }

    /// The plan page to show something on: the first opened, focused,
    /// else a new one. In view either way.
    fn plan_page(&mut self) -> u64 {
        let page = match self.pages.keys().min() {
            Some(&p) => {
                pages_api::focus(p);
                p
            }
            None => {
                let p = pages_api::open(PLAN, "Kiln");
                // Opened is in view; PageShown confirms it.
                self.add_page(p, Plan::new());
                p
            }
        };
        if let Some(p) = self.pages.get_mut(&page) {
            p.shown = true;
        }
        self.project.start();
        self.shown_changed();
        page
    }

    /// Item `id` on tab `to` (Item or Impact) of the plan page, if the
    /// project has it; else said.
    fn show_item(&mut self, id: &str, to: Tab) {
        let known = self.project.loaded().is_some_and(|l| l.item(id).is_some());
        if !known {
            api::notify(&format!("Kiln: {id} isn't in the project any more"));
            return;
        }
        let page = self.plan_page();
        if let Some(p) = self.pages.get_mut(&page) {
            p.plan.hand(to, id);
            pages_api::refresh(page);
        }
    }

    /// The command from a buffer that waits for the project, done once
    /// it's read -- or said, when it can't be. Whether it did something a
    /// page may now ask kiln about.
    fn resolve_wanted(&mut self) -> bool {
        if self.wanted.is_none() {
            return false;
        }
        let state = self.project.state();
        let loaded = match state {
            State::Loaded { loaded, .. } => loaded,
            State::Idle | State::Probing | State::Loading => return false,
            other => {
                let why = unusable(&other);
                self.wanted = None;
                api::notify(&format!("Kiln: {why}"));
                return false;
            }
        };
        match self.wanted.take() {
            Some(Wanted::Item { spot, to }) => match spot::item_at(loaded, &spot) {
                Ok(id) => {
                    self.show_item(&id, to);
                    true
                }
                Err(why) => {
                    api::notify(&format!("Kiln: {why}"));
                    false
                }
            },
            Some(Wanted::Find) => {
                let items = &loaded.items;
                if items.is_empty() {
                    api::notify("Kiln: the project has no items yet: kiln new adds one");
                    return false;
                }
                if items.len() > MOST_CHOICES {
                    api::notify(&format!(
                        "Kiln: the picker has the first {MOST_CHOICES} of {} items",
                        items.len()
                    ));
                }
                let choices: Vec<Choice> = items
                    .iter()
                    .take(MOST_CHOICES)
                    .map(|i| {
                        Choice::new(format!("{}  {}", i.id, i.title), i.id.clone())
                            .detail(i.status.clone())
                    })
                    .collect();
                let id = api::pick("Kiln item", &picker_items(&choices));
                self.answers.insert(id, Asker::Find);
                false
            }
            Some(Wanted::Edit { spot, edit }) => {
                if let Some(why) = edit::blocked(&state, self.project.settled()) {
                    api::notify(&format!("Kiln: {why}"));
                    return false;
                }
                // A new item is about no item; the rest are about the one
                // here, found as `kiln.show` finds it.
                let id = if edit == Edit::New {
                    None
                } else {
                    match spot::item_at(loaded, &spot) {
                        Ok(id) => Some(id),
                        Err(why) => {
                            api::notify(&format!("Kiln: {why}"));
                            return false;
                        }
                    }
                };
                let facts = Facts {
                    loaded,
                    config: now_of(&self.project, &self.config, &self.publishing).config,
                };
                // The flow it replaces can't be answered any more.
                self.answers.retain(|_, a| !matches!(a, Asker::Here { .. }));
                let turn = self.here.start(edit, id.as_deref(), &facts);
                self.here_turn(turn);
                false
            }
            None => false,
        }
    }

    /// Where the cursor is, as `here()` and `cursor()` say it now.
    fn spot_here() -> Spot {
        let here = api::here();
        let cursor = api::cursor();
        Spot {
            path: here.as_ref().and_then(|h| h.path.clone()),
            root: here
                .as_ref()
                .and_then(|h| h.project.as_ref().map(|p| p.root.clone())),
            line: here.as_ref().map_or(0, |h| h.line),
            text: here.as_ref().map(|h| h.text.clone()).unwrap_or_default(),
            column: cursor.as_ref().filter(|c| !c.selected).map(|c| c.at.1),
            selected: cursor.as_ref().is_some_and(|c| c.selected),
        }
    }

    /// `kiln.show`, `kiln.impact`: where the cursor is, kept for when the
    /// project's read (now, usually).
    fn want_item(&mut self, to: Tab) {
        self.wanted = Some(Wanted::Item {
            spot: Self::spot_here(),
            to,
        });
        self.project.start();
    }

    /// An edit from a buffer: where the cursor is, kept as above.
    fn want_edit(&mut self, edit: Edit) {
        self.wanted = Some(Wanted::Edit {
            spot: Self::spot_here(),
            edit,
        });
        self.project.start();
    }

    /// A plan page, `page`, kept: in view.
    fn add_page(&mut self, page: u64, plan: Plan) {
        self.pages.insert(page, PlanPage { shown: true, plan });
        self.project.start();
        self.shown_changed();
    }
}

impl Extension for Kiln {
    /// The project read at once, for the language server.
    fn activate(&mut self) -> Result<(), String> {
        self.project.start();
        self.decide = true;
        self.pass_on();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            // The first opened, when there's one: it keeps its tab.
            "kiln.open" => {
                self.plan_page();
            }
            "kiln.show" => self.want_item(Tab::Item),
            "kiln.impact" => self.want_item(Tab::Impact),
            "kiln.set_status" => self.want_edit(Edit::Status),
            "kiln.link" => self.want_edit(Edit::Link),
            "kiln.unlink" => self.want_edit(Edit::Unlink),
            "kiln.new" => self.want_edit(Edit::New),
            "kiln.new_child" => self.want_edit(Edit::Child),
            "kiln.edit" => self.want_edit(Edit::More),
            "kiln.find" => {
                self.wanted = Some(Wanted::Find);
                self.project.start();
            }
            "kiln.reload" => self.reload(),
            // Nothing is downloaded: the extension has no network
            // permission. The doctor's fix row for a missing kiln runs it.
            "kiln.install_help" => {
                api::open_url(doctor::INSTALL_URL)
                    .map_err(|why| format!("can't open {}: {why}", doctor::INSTALL_URL))?;
                return Ok(());
            }
            _ => return Err(format!("no command {id}")),
        }
        self.pass_on();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::ProcessOutput(o) => self.project.output(o.process, o.stderr, &o.line),
            Event::ProcessExit((process, code)) => self.project.exited(process, code),
            Event::Timer(t) => {
                self.project.timer(t);
            }
            Event::Prompted((id, text)) => self.answered(id, text, false),
            Event::Picked((id, value)) => self.answered(id, value, true),
            Event::PageShown((p, shown)) => {
                if let Some(page) = self.pages.get_mut(&p) {
                    page.shown = shown;
                    self.shown_changed();
                }
            }
            Event::SettingChanged(key) => match key.as_str() {
                "kiln.path" => {
                    self.project.kiln_changed();
                    self.lsp.kiln_changed();
                    self.decide = true;
                }
                "kiln.refresh_seconds" => self.project.refresh_changed(),
                _ => {}
            },
            _ => {}
        }
        self.pass_on();
    }

    fn file_saved(&mut self, path: &str) {
        if std::path::Path::new(path).file_name() == Some(std::ffi::OsStr::new("kiln.toml")) {
            self.read_config();
        }
        self.lsp.saved(path);
        self.decide = true;
        self.project.saved(path);
        self.pass_on();
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let hints = Hints {
            doctor: api::key_for("project.doctor"),
            settings: api::key_for("settings.open"),
        };
        let now = now_of(&self.project, &self.config, &self.publishing);
        let Some(p) = self.pages.get_mut(&page) else {
            return Page::new().line([pages_api::muted("this page was closed")]);
        };
        p.plan
            .render(now, &hints, width as usize, height as usize)
            .to_page()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let press = fenix_ui_kit::KeyPress::from(fenix_ui_kit::Key::from(key));
        self.on_page(page, |plan, now| plan.key(press, now));
    }

    /// `C-d`, `C-u`, PageUp and PageDown; Shift and Ctrl with a key in
    /// the filter.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        if let Some(press) = fenix_ui_kit::KeyPress::from_api(&key) {
            self.on_page(page, |plan, now| plan.key(press, now));
        }
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let m = fenix_ui_kit::Mouse::from(&event);
        self.on_page(page, |plan, now| plan.mouse(&m, now));
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        self.on_page(page, |plan, _| {
            plan.paste(text);
            Vec::new()
        });
    }

    /// The tab, and each tab's cursor, filter and toggles.
    fn page_state(&mut self, page: u64) -> Option<String> {
        self.pages.get(&page).map(|p| p.plan.save())
    }

    /// A plan page as it was saved; a state it can't read opens as a new
    /// page does, rather than not at all.
    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != PLAN {
            return false;
        }
        self.add_page(page, Plan::restore(state));
        self.pass_on();
        true
    }

    /// Its prompts' and pickers' answers are dropped as they come (no page
    /// to take them); its edits still run, kiln having been asked.
    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
        self.answers
            .retain(|_, a| !matches!(a, Asker::View { page: p, .. } if *p == page));
        self.shown_changed();
        self.pass_on();
    }

    /// The hints that name Fenix's keys are drawn again.
    fn key_scheme_changed(&mut self, _scheme: KeyScheme) {
        for &p in self.pages.keys() {
            pages_api::refresh(p);
        }
    }
}

/// A view's choices as Fenix's picker's rows.
fn picker_items(choices: &[Choice]) -> Vec<PickerItem> {
    choices
        .iter()
        .map(|c| PickerItem::new(c.label.clone(), c.value.clone()).detail(c.detail.clone()))
        .collect()
}

/// Why a project that isn't loaded can't be used, in its own words.
fn unusable(state: &State) -> String {
    match state {
        State::NotFound(why) | State::NotKiln(why) | State::NoProject(why) | State::Failed(why) => {
            why.to_string()
        }
        State::TooOld(v) => format!(
            "kiln {v} is older than {}, the oldest this extension reads",
            model::MIN_VERSION
        ),
        State::Idle | State::Probing | State::Loading => "the project isn't read yet".into(),
        State::Loaded { .. } => String::new(),
    }
}

api::export!(Kiln);

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// A fake whose kiln answers as the demo project's did, check's exit 1
    /// included.
    pub(crate) fn demo() -> Fake {
        let fake = Fake::new();
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Ran::ok(fixture("version.txt")))
            .on_spawn(
                "kiln list --format json",
                Ran::ok(fixture("demo/list.json")),
            )
            .on_spawn(
                "kiln status --format json",
                Ran::ok(fixture("demo/status.json")),
            )
            .on_spawn(
                "kiln check --format json",
                Ran::exits(1)
                    .stdout(fixture("demo/check.json"))
                    .stderr(fixture("demo/check.stderr")),
            )
            // kiln 1.0.0's refusal, as recorded.
            .on_spawn(
                "kiln lsp --languages",
                Ran::fails(2, fixture("demo/lsp-languages.stderr")),
            )
            // What the tabs ask, as recorded: Trace's default type, and
            // the items `show` and `impact` were recorded for.
            .on_spawn(
                "kiln trace --format json",
                Ran::ok(fixture("demo/trace.json")),
            );
        for id in [
            "ADR-001", "MS-001", "REQ-001", "REQ-002", "TASK-002", "TASK-004", "TASK-006",
            "TEST-001",
        ] {
            fake.on_spawn(
                &format!("kiln show {id} --format json"),
                Ran::ok(fixture(&format!("demo/show-{id}.json"))),
            );
        }
        for id in ["REQ-001", "TASK-002", "TASK-006"] {
            fake.on_spawn(
                &format!("kiln impact {id} --format json"),
                Ran::ok(fixture(&format!("demo/impact-{id}.json"))),
            );
        }
        fake
    }

    pub(crate) fn spawned(fake: &Fake) -> Vec<String> {
        fake.take()
            .into_iter()
            .filter_map(|l| {
                l.strip_prefix("spawn ")
                    .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string())
            })
            .collect()
    }

    use fenix_test_kit::assert_snapshot;

    /// The size snapshots are drawn at: wide enough for every tab.
    const WIDE: (u32, u32) = (110, 26);

    /// The plan opened on the demo project, drawn wide.
    pub(crate) fn opened(fake: &Fake) -> (Harness<Kiln>, u64) {
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.size = WIDE;
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        (h, page)
    }

    /// The ID under the cursor on page `page`'s tab shown.
    pub(crate) fn at(h: &Harness<Kiln>, page: u64) -> Option<String> {
        h.ext.pages.get(&page)?.plan.id().map(str::to_string)
    }

    /// Item `id` handed to tab `tab` of page `page`, as `v` or `i` would,
    /// for an ID no row has; and what it asks, asked.
    pub(crate) fn hand(h: &mut Harness<Kiln>, page: u64, tab: Tab, id: &str) {
        h.ext.pages.get_mut(&page).unwrap().plan.hand(tab, id);
        h.ext.pass_on();
        h.settle();
    }

    /// The screen's last line: the bottom line.
    pub(crate) fn foot(h: &mut Harness<Kiln>, page: u64) -> String {
        h.screen(page).lines().last().unwrap_or("").to_string()
    }

    #[test]
    fn open_reads_the_project_and_shows_its_tree() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        assert_snapshot!("tree-demo", h.screen(page));
        assert_snapshot!("tree-demo-marked", h.marked(page));
        assert_eq!(
            spawned(&fake)
                .iter()
                .filter(|l| l.starts_with("/usr/bin/kiln"))
                .count(),
            // version, list, status, check, then lsp --languages.
            5
        );
        // At 80 cells the tabs after the one shown give way.
        h.size = (80, 24);
        assert_snapshot!("tree-demo-80", h.screen(page));
    }

    #[test]
    fn kilns_own_tree_with_closed_items_shown_on_a() {
        let fake = own();
        let (mut h, page) = opened(&fake);
        h.size = (110, 40);
        assert_snapshot!("tree-own", h.screen(page));
        h.keys(page, "/adr-01<Enter>");
        assert_snapshot!("tree-own-adr", h.screen(page));
        assert!(!h.screen(page).contains("ADR-013"));
        h.key(page, Key::Char('a'));
        let s = h.screen(page);
        assert!(s.contains("ADR-013") && s.contains("superseded"), "{s}");
        assert!(s.ends_with("/adr-01  closed items shown\n"), "{s}");
        h.key(page, Key::Char('a'));
        assert!(!h.screen(page).contains("ADR-013"));
    }

    #[test]
    fn the_filter_narrows_as_you_type() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "/sql");
        assert!(h.marked(page).ends_with("[typing, more keys, mouse]\n"));
        h.keys(page, "ite");
        assert_snapshot!("tree-demo-filter", h.screen(page));
        h.keys(page, "<Esc>");
        assert!(h.screen(page).contains("REQ-001"));
        assert!(!h.marked(page).contains("typing"));
    }

    #[test]
    fn enter_and_a_double_click_open_the_item_at_its_line() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        fake.take();
        h.keys(page, "jj<Enter>");
        assert_eq!(
            fake.take(),
            ["open /project/design/requirements/REQ-003-the-log-exports-as-csv.md:2"]
        );
        // Line 2 is the first heading; the decision's row is line 7. The
        // mouse is where the page was last drawn.
        h.screen(page);
        h.click(page, 7, 3, 1);
        assert!(fake.take().is_empty(), "one click only moves the cursor");
        assert_eq!(at(&h, page).as_deref(), Some("ADR-001"));
        h.click(page, 7, 3, 2);
        assert_eq!(
            fake.take(),
            ["open /project/design/decisions/ADR-001-store-readings-in-sqlite.md:2"]
        );
        // A click on a tab shows it.
        h.click(page, 0, 12, 1);
        assert!(h
            .screen(page)
            .contains("\nrequirement  3 items  2 gaps  columns: implements | verifies\n"));
    }

    #[test]
    fn tabs_switch_and_v_and_i_hand_the_item_on() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.key(page, Key::Tab);
        assert!(h
            .screen(page)
            .contains("\nREQ-002  Logged readings survive a power cut  gap "));
        h.keys(page, "9");
        assert!(h
            .screen(page)
            .contains("\n11:23  K102  error    reference REQ-099 doesn't exist\n"));
        h.keys(page, "1jv");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Item);
        assert!(h
            .screen(page)
            .contains("\nrequirement  REQ-002   Logged readings survive a power cut  draft\n"));
        h.keys(page, "1/TASK-002<Enter>i");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Impact);
        assert!(h
            .screen(page)
            .contains("\nTASK-007  child of TASK-002  Batch the writes\n"));
        // Back on the Tree, the cursor is where it was.
        h.keys(page, "1");
        assert_eq!(at(&h, page).as_deref(), Some("TASK-002"));
    }

    #[test]
    fn loading_shows_while_kiln_reads() {
        let fake = demo();
        fake.on_spawn("kiln list", Ran::running(""));
        let (mut h, page) = opened(&fake);
        let s = h.screen(page);
        assert!(s.contains("\nreading the project…\n"), "{s}");
        assert!(s.ends_with("r try again  Tab next tab\n"), "{s}");
    }

    #[test]
    fn output_in_chunks_and_check_exiting_1_still_loads() {
        let fake = demo();
        for pattern in ["kiln list", "kiln status", "kiln check"] {
            fake.on_spawn(pattern, Ran::running(""));
        }
        let (mut h, page) = opened(&fake);
        for (pattern, file, code) in [
            ("kiln list", "demo/list.json", 0),
            ("kiln status", "demo/status.json", 0),
            ("kiln check", "demo/check.json", 1),
        ] {
            let p = fake.process(pattern).unwrap().id;
            for line in fixture(file).lines() {
                h.said(p, line, false);
            }
            h.said(p, "kiln: 1 error, 2 warnings", true);
            h.exited(p, Some(code));
        }
        let at_once = {
            let (mut h, page) = opened(&demo());
            h.screen(page)
        };
        assert_eq!(h.screen(page), at_once);
    }

    #[test]
    fn kiln_missing_says_how_to_set_kiln_path() {
        let fake = Fake::new();
        fake.key("project.doctor", "SPC p h")
            .key("settings.open", "SPC h s");
        let (mut h, page) = opened(&fake);
        assert_snapshot!("not-found", h.screen(page));
        // On every tab.
        h.keys(page, "4");
        assert!(h.screen(page).contains("\nkiln isn't on the PATH"));
        // Set, it's looked for again at once.
        fake.on_spawn("/opt/kiln/kiln --version", Ran::ok("kiln 1.0.0\n"))
            .on_spawn("/opt/kiln/kiln list", Ran::ok(fixture("demo/list.json")));
        h.set_setting("kiln.path", Some("/opt/kiln/kiln"));
        assert!(fake
            .calls()
            .iter()
            .any(|c| c.ends_with("/opt/kiln/kiln list --format json")));
    }

    #[test]
    fn whats_in_the_way_is_said_in_the_projects_words() {
        let cases: [(&str, Ran, &str); 4] = [
            (
                "kiln --version",
                Ran::ok("kiln 0.9.4\n"),
                "kiln 0.9.4 is older than 1.0.0, the oldest this extension reads: update Kiln",
            ),
            (
                "kiln --version",
                Ran::fails(2, "error: unknown flag --version"),
                "kiln can't be used: error: unknown flag --version",
            ),
            (
                "kiln list",
                Ran::fails(2, fixture("errors/no-kiln-toml.stderr")),
                "kiln: no kiln.toml in <dir> or any folder above it (run `kiln init` to start a project)",
            ),
            (
                "kiln list",
                Ran::fails(2, "kiln: kiln.toml:4: unknown key `nmae`"),
                "kiln: kiln.toml:4: unknown key `nmae`",
            ),
        ];
        for (pattern, ran, said) in cases {
            let fake = demo();
            fake.on_spawn(pattern, ran);
            let (mut h, page) = opened(&fake);
            let s = h.screen(page);
            assert!(s.contains(&format!("\n{said}\n")), "{said}: {s}");
            assert!(s.starts_with("1 Tree "), "{s}");
        }
    }

    #[test]
    fn a_failing_reload_keeps_the_loaded_project() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "jj");
        fake.on_spawn(
            "kiln list",
            Ran::fails(
                2,
                "kiln: design/tasks/TASK-001-set-up-the-logger.md:1: front matter can't be parsed",
            ),
        );
        h.saved("/home/me/demo/design/tasks/TASK-001-set-up-the-logger.md");
        let s = h.screen(page);
        assert!(s.contains("\nTASK-001 "), "{s}");
        assert_eq!(foot(&mut h, page), "the last read failed: kiln: design/tasks/TASK-001-set-up-the-logger.md:1: front matter can't be parsed");
        assert_eq!(
            at(&h, page).as_deref(),
            Some("REQ-003"),
            "the cursor where it was"
        );
        // A key's message goes before it, then it's back.
        h.key(page, Key::Char('a'));
        assert_eq!(foot(&mut h, page), "closed items shown");
        h.key(page, Key::Char('j'));
        assert!(foot(&mut h, page).starts_with("the last read failed"));
    }

    #[test]
    fn two_reloads_while_one_runs_run_one_more() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        let loaded = h.screen(page);
        fake.take();
        fake.on_spawn("kiln list", Ran::running(fixture("demo/list.json")));
        h.command("kiln.reload").unwrap();
        h.command("kiln.reload").unwrap();
        h.key(page, Key::Char('r'));
        assert!(h.screen(page).starts_with("1 Tree "));
        assert!(h
            .screen(page)
            .lines()
            .next()
            .unwrap()
            .ends_with("reading…  Demo  1 error"));
        let first = fake.process("kiln list").unwrap().id;
        fake.on_spawn("kiln list", Ran::ok(fixture("demo/list.json")));
        h.exited(first, Some(0));
        let lists = spawned(&fake)
            .iter()
            .filter(|l| l.ends_with("kiln list --format json"))
            .count();
        assert_eq!(lists, 2);
        assert_eq!(h.screen(page), loaded);
    }

    #[test]
    fn the_page_state_round_trips_through_fenix() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "jja/req<Enter>4");
        let state = h.ext.page_state(page).unwrap();
        assert!(state.starts_with(r#"{"tab":"board","tabs":{"#), "{state}");
        let screen = h.screen(page);
        h.close(page);
        assert!(h.ext.page_restore(41, "plan", &state));
        h.settle();
        assert_eq!(h.screen(41), screen);
        h.keys(41, "1");
        let s = h.screen(41);
        assert!(
            s.ends_with("/req  closed items shown\n") || s.contains("\n/req  "),
            "{s}"
        );
        assert_eq!(at(&h, 41).as_deref(), Some("REQ-003"), "{}", h.marked(41));
        // Not ours, or a state it can't read: refused, or a new page.
        assert!(!h.ext.page_restore(42, "other", &state));
        assert!(h.ext.page_restore(43, "plan", "{\"v\": 99}"));
        assert!(h.screen(43).contains("\nREQ-001 "));
    }

    #[test]
    fn two_pages_keep_their_own_tabs_and_answers_go_to_the_asker() {
        use crate::pages::frame::tests::Asking;
        let fake = demo();
        fake.on_spawn("kiln show", Ran::ok("REQ-002, shown"));
        let (mut h, one) = opened(&fake);
        assert!(h.ext.page_restore(2, "plan", "{}"));
        h.event(Event::PageShown((2, true)));
        for p in [one, 2] {
            h.ext
                .pages
                .get_mut(&p)
                .unwrap()
                .plan
                .set_view(Tab::Item, Box::<Asking>::default());
        }
        fake.take();
        h.keys(2, "4");
        h.keys(one, "jv");
        assert_eq!(
            spawned(&fake),
            ["/usr/bin/kiln show REQ-002"],
            "one ask, from the page that handed it on"
        );
        assert!(h.screen(one).contains("\nREQ-002, shown\n"));
        assert!(h.screen(2).starts_with("1 Tree "));
        assert!(h.screen(2).contains("\nTASK-003  Export the log as CSV "));
        h.keys(2, "8");
        assert!(
            h.screen(2).contains("\nnothing\n"),
            "the other page's Item asked nothing"
        );
        // A reload asks again, for the page whose Item tab is shown; one
        // closed while its answer is out drops it.
        fake.on_spawn("kiln show", Ran::running(""));
        h.key(one, Key::Char('r'));
        assert_eq!(
            spawned(&fake)
                .iter()
                .filter(|l| l.contains(" show "))
                .count(),
            1
        );
        assert!(foot(&mut h, one) == "reading…", "{}", h.screen(one));
        assert!(
            h.screen(one).contains("\nREQ-002, shown\n"),
            "the last answer stays meanwhile"
        );
        let p = fake.process("kiln show").unwrap().id;
        h.close(one);
        h.said(p, "late", false);
        h.exited(p, Some(0));
        assert!(h.ext.asked.is_empty());
    }

    #[test]
    fn kiln_reload_before_open_reads_it() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.reload").unwrap();
        assert_eq!(spawned(&fake).len(), 5);
    }

    #[test]
    fn the_refresh_timer_follows_the_page_in_view() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        let refresh = |f: &Fake| {
            f.timers()
                .into_iter()
                .find(|t| t.millis == 5000 && t.repeat)
        };
        let timer = refresh(&fake).expect("a 5 s refresh while it's in view").id;
        fake.take();
        h.fire(timer);
        assert_eq!(spawned(&fake).len(), 3);
        h.event(Event::PageShown((page, false)));
        assert!(refresh(&fake).is_none());
        h.event(Event::PageShown((page, true)));
        assert!(refresh(&fake).is_some());
        h.close(page);
        assert!(refresh(&fake).is_none());
    }

    // -- The language server ------------------------------------------------

    /// A project folder of the test's own, for `.fenix/tools.json`: empty
    /// at first.
    pub(crate) fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-kiln-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".fenix")).unwrap();
        dir
    }

    pub(crate) fn harness(fake: &Fake, dir: &std::path::Path) -> Harness<Kiln> {
        Harness::wrap(
            fake.clone(),
            Kiln {
                root: Some(dir.to_path_buf()),
                ..Kiln::default()
            },
        )
    }

    /// What it told Fenix about its server, in order.
    fn servers(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.starts_with("add kiln "))
            .collect()
    }

    fn infos(fake: &Fake) -> Vec<String> {
        fake.logs()
            .into_iter()
            .filter_map(|l| l.strip_prefix("info ").map(str::to_string))
            .collect()
    }

    /// kiln answering `lsp --languages` as the unreleased command would
    /// (hand-written: tests/fixtures/hand/README).
    fn answers_languages(fake: &Fake, languages: &[&str]) {
        let json = serde_json::json!({"kiln_json": 1, "command": "kiln", "args": ["lsp"], "languages": languages});
        fake.on_spawn("kiln lsp --languages", Ran::ok(json.to_string()));
    }

    const TOOLS: &str = r#"{"lsp": {"additional": [{"name": "kiln", "executable": "kiln", "args": ["lsp"], "languages": ["markdown"]}]}}"#;

    #[test]
    fn loaded_it_registers_the_server_with_no_page_open() {
        let fake = demo();
        let hand = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/hand/lsp-languages.json"
        ))
        .unwrap();
        fake.on_spawn("kiln lsp --languages", Ran::ok(hand));
        let dir = folder("registers");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(
            servers(&fake),
            ["add kiln markdown,rust,tcl: /usr/bin/kiln lsp"]
        );
        assert!(spawned(&fake).contains(&"/usr/bin/kiln lsp --languages --format json".to_string()));
        assert_eq!(
            h.ext.lsp.status(),
            &lsp::Status::Registered {
                server: lsp::Server {
                    languages: vec!["markdown".into(), "rust".into(), "tcl".into()],
                    program: "/usr/bin/kiln".into()
                },
                note: None
            }
        );
    }

    #[test]
    fn kiln_1_0_gets_markdown_only_said_once() {
        let fake = demo();
        let dir = folder("old");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        let said = |f: &Fake| {
            infos(f)
                .iter()
                .filter(|l| l.starts_with("kiln 1.0.0 has no `kiln lsp --languages`"))
                .count()
        };
        assert_eq!(said(&fake), 1);
        // Asked again on r: refused again, not said or set again.
        h.command("kiln.reload").unwrap();
        assert_eq!(said(&fake), 1);
        assert_eq!(servers(&fake).len(), 1);
    }

    #[test]
    fn plain_reloads_dont_restart_it() {
        let fake = demo();
        let dir = folder("plain");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        h.saved("/home/me/demo/design/tasks/TASK-001-set-up-the-logger.md");
        h.command("kiln.reload").unwrap();
        h.key(page, Key::Char('r'));
        h.saved("/home/me/demo/kiln.toml");
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
    }

    #[test]
    fn new_languages_restart_it_once() {
        let fake = demo();
        let dir = folder("languages");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        answers_languages(&fake, &["markdown", "python"]);
        // A code file saved doesn't ask; kiln.toml does.
        h.saved("/home/me/demo/src/app.py");
        assert_eq!(servers(&fake).len(), 1);
        h.saved("/home/me/demo/kiln.toml");
        h.saved("/home/me/demo/kiln.toml");
        assert_eq!(
            servers(&fake),
            [
                "add kiln markdown: /usr/bin/kiln lsp",
                "add kiln markdown,python: /usr/bin/kiln lsp"
            ]
        );
    }

    #[test]
    fn listed_in_tools_json_it_steps_aside_until_the_entry_goes() {
        let fake = demo();
        let dir = folder("listed");
        std::fs::write(dir.join(".fenix/tools.json"), TOOLS).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        // Fenix told there's none of ours; nothing registered.
        assert_eq!(servers(&fake), ["add kiln : "]);
        assert!(matches!(
            h.ext.lsp.status(),
            lsp::Status::ToolsJson(lsp::Form::Additional)
        ));
        assert_eq!(
            infos(&fake)
                .iter()
                .filter(|l| l
                    .starts_with("the project's .fenix/tools.json runs kiln's language server"))
                .count(),
            1
        );
        // The entry removed, the file saved: ours.
        std::fs::write(dir.join(".fenix/tools.json"), "{}").unwrap();
        h.saved(&dir.join(".fenix/tools.json").display().to_string());
        assert_eq!(
            servers(&fake)[1..],
            ["add kiln markdown: /usr/bin/kiln lsp"]
        );
        // Listed again (the older form): ours taken away.
        std::fs::write(
            dir.join(".fenix/tools.json"),
            r#"{"lsp": {"markdown": {"executable": "kiln", "args": ["lsp"]}}}"#,
        )
        .unwrap();
        h.command("kiln.reload").unwrap();
        assert_eq!(servers(&fake)[2..], ["add kiln markdown: "]);
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::ToolsJson(lsp::Form::Primary(l)) if l == "markdown")
        );
    }

    #[test]
    fn a_malformed_tools_json_is_said_and_ours_registered() {
        let fake = demo();
        let dir = folder("malformed");
        std::fs::write(dir.join(".fenix/tools.json"), "{\"lsp\": ").unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        assert!(infos(&fake)
            .iter()
            .any(|l| l.starts_with(".fenix/tools.json isn't valid JSON: ")));
        // No tools.json at all is the same, unsaid.
        let fake = demo();
        let dir = folder("absent");
        std::fs::remove_dir_all(dir.join(".fenix")).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        assert!(!infos(&fake)
            .iter()
            .any(|l| l.contains("valid JSON") || l.contains("can't be read")));
    }

    #[test]
    fn kiln_lost_takes_it_away_and_another_kiln_sets_it_once() {
        let fake = demo();
        let dir = folder("lost");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        fake.on_spawn_error("/nowhere/kiln", "no such file");
        h.set_setting("kiln.path", Some("/nowhere/kiln"));
        assert_eq!(servers(&fake)[1..], ["add kiln markdown: "]);
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::NotRegistered(w) if w.starts_with("kiln.path is /nowhere/kiln, which can't be started"))
        );
        // A kiln there: set once, with it and its languages.
        fake.on_spawn("/opt/kiln --version", Ran::ok("kiln 1.1.0\n"))
            .on_spawn("/opt/kiln list", Ran::ok(fixture("demo/list.json")))
            .on_spawn("/opt/kiln status", Ran::ok(fixture("demo/status.json")))
            .on_spawn("/opt/kiln check", Ran::ok(fixture("demo/check.json")))
            .on_spawn(
                "/opt/kiln lsp --languages",
                Ran::ok(
                    serde_json::json!({"kiln_json": 1, "languages": ["markdown", "rust"]})
                        .to_string(),
                ),
            );
        h.set_setting("kiln.path", Some("/opt/kiln"));
        assert_eq!(
            servers(&fake)[2..],
            ["add kiln markdown,rust: /opt/kiln lsp"]
        );
    }

    #[test]
    fn not_a_kiln_project_registers_nothing() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, fixture("errors/no-kiln-toml.stderr")),
        );
        let dir = folder("none");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln : "]);
        // Nothing asked of kiln about languages there.
        assert!(!spawned(&fake).iter().any(|l| l.contains(" lsp ")));
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::NotRegistered(w) if w.starts_with("kiln: no kiln.toml"))
        );
    }

    #[test]
    fn an_answer_no_one_waits_for_is_dropped() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        let id = h.ext.project.query(["show", "REQ-001"]);
        h.ext.pass_on();
        h.settle();
        assert!(!h.ext.asked.contains_key(&id));
    }

    // -- The doctor, the tasks and the modeline -----------------------------

    /// A fake whose kiln answers as Kiln's own project did: no findings.
    pub(crate) fn own() -> Fake {
        let fake = Fake::new();
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Ran::ok(fixture("version.txt")))
            .on_spawn("kiln list --format json", Ran::ok(fixture("own/list.json")))
            .on_spawn(
                "kiln status --format json",
                Ran::ok(fixture("own/status.json")),
            )
            .on_spawn(
                "kiln check --format json",
                Ran::ok(fixture("own/check.json")),
            )
            .on_spawn(
                "kiln lsp --languages",
                Ran::fails(2, fixture("own/lsp-languages.stderr")),
            );
        fake
    }

    /// What it told Fenix, of `what` (`doctor`, `tasks`, `status`), in order.
    fn told(fake: &Fake, what: &str) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.split(' ').next() == Some(what))
            .collect()
    }

    /// The rows it set last, one line each: `label health detail`.
    fn rows(fake: &Fake) -> Vec<String> {
        fake.state()
            .checks
            .iter()
            .map(|c| {
                let health = format!("{:?}", c.health);
                let fix = c.fix.as_ref().map(|(l, cmd)| format!(" -> {l} ({cmd})"));
                format!(
                    "{} {} {}{}",
                    c.label,
                    health.strip_prefix("Health::").unwrap_or(&health),
                    c.detail,
                    fix.unwrap_or_default()
                )
            })
            .collect()
    }

    const DEMO_TASKS: &str = "tasks kiln: check, kiln: export --check, kiln: export";

    #[test]
    fn the_demo_project_gets_its_rows_tasks_and_modeline() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        let r = rows(&fake);
        assert_eq!(r.len(), 4, "{r:?}");
        assert_eq!(r[0], "kiln Ok kiln 1.0.0, /usr/bin/kiln");
        assert_eq!(r[1], "kiln.toml Ok read, 16 items");
        // kiln 1.0.0 has no `lsp --languages`: Markdown only, with why.
        assert!(
            r[2].starts_with("language server Info kiln lsp, for markdown only: kiln 1.0.0 has no"),
            "{}",
            r[2]
        );
        assert_eq!(
            r[3],
            "kiln check Bad 1 error, 2 warnings -> open the plan (kiln.open)"
        );
        // The tasks run the kiln found, in the project.
        let state = fake.state();
        let lines: Vec<String> = state
            .tasks
            .iter()
            .map(|t| format!("{}: {}", t.name, fenix_test_kit::line(&t.cmd)))
            .collect();
        assert_eq!(
            lines,
            [
                "kiln: check: /usr/bin/kiln check",
                "kiln: export --check: /usr/bin/kiln export --check",
                "kiln: export: /usr/bin/kiln export",
            ]
        );
        assert!(state
            .tasks
            .iter()
            .all(|t| t.cmd.cwd.as_deref() == Some(PROJECT)));
        drop(state);
        assert_eq!(told(&fake, "tasks"), [DEMO_TASKS]);
        assert_eq!(told(&fake, "status"), ["status kiln 1 error, 2 warnings"]);
        assert_eq!(fake.status().as_deref(), Some("kiln 1 error, 2 warnings"));
    }

    #[test]
    fn a_project_with_no_findings_has_no_modeline_item() {
        let fake = own();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        assert_eq!(fake.status(), None);
        assert_eq!(rows(&fake)[3], "kiln check Ok no findings");
        assert_eq!(
            rows(&fake)[1],
            format!("kiln.toml Ok read, {} items", fixture_items("own"))
        );
        assert_eq!(told(&fake, "tasks"), [DEMO_TASKS]);
    }

    /// How many items `own/list.json` holds.
    fn fixture_items(project: &str) -> String {
        model::list(&fixture(&format!("{project}/list.json")))
            .unwrap()
            .len()
            .to_string()
    }

    #[test]
    fn the_modeline_follows_the_check_and_keeps_its_text_while_reading() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        assert_eq!(fake.status().as_deref(), Some("kiln 1 error, 2 warnings"));
        // A reload that runs: the text stays, and nothing is set.
        fake.on_spawn("kiln list", Ran::running(""));
        h.command("kiln.reload").unwrap();
        assert_eq!(fake.status().as_deref(), Some("kiln 1 error, 2 warnings"));
        assert_eq!(told(&fake, "status").len(), 1);
        // It ends with the error fixed: one warning less, one set.
        let list = fake.process("kiln list").unwrap().id;
        fake.on_spawn(
            "kiln check",
            Ran::ok(
                r#"[{"code":"K201","severity":"warning","message":"m","path":"a.md","line":1,"column":1}]"#,
            ),
        );
        for line in fixture("demo/list.json").lines() {
            h.said(list, line, false);
        }
        h.exited(list, Some(0));
        assert_eq!(fake.status().as_deref(), Some("kiln 1 warning"));
        // Clean: the item goes.
        fake.on_spawn("kiln list", Ran::ok(fixture("demo/list.json")));
        fake.on_spawn("kiln check", Ran::ok("[]"));
        h.saved("/home/me/demo/design/tasks/TASK-006-draft-the-user-guide.md");
        assert_eq!(fake.status(), None);
        assert_eq!(
            told(&fake, "status"),
            [
                "status kiln 1 error, 2 warnings",
                "status kiln 1 warning",
                "status -"
            ]
        );
    }

    #[test]
    fn repeated_reloads_with_nothing_changed_set_nothing_again() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        let before = (
            told(&fake, "doctor"),
            told(&fake, "tasks"),
            told(&fake, "status"),
        );
        // The doctor was set again as the server's row went from waiting
        // to registered; once it's settled, nothing is.
        assert_eq!((before.1.len(), before.2.len()), (1, 1));
        h.command("kiln.reload").unwrap();
        h.key(page, Key::Char('r'));
        h.saved("/home/me/demo/design/tasks/TASK-001-set-up-the-logger.md");
        h.saved("/home/me/demo/kiln.toml");
        h.fire_timers();
        assert_eq!(
            (
                told(&fake, "doctor"),
                told(&fake, "tasks"),
                told(&fake, "status")
            ),
            before
        );
    }

    #[test]
    fn kiln_missing_says_so_with_its_fix_and_offers_no_tasks() {
        let fake = Fake::new();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        assert_eq!(
            rows(&fake),
            ["kiln Bad isn't on the PATH, and kiln.path is empty -> how to install Kiln (kiln.install_help)"]
        );
        assert_eq!(told(&fake, "tasks"), ["tasks "]);
        assert!(fake.state().tasks.is_empty());
        assert_eq!(fake.status(), None);
        // The fix opens Kiln's install page and nothing else.
        h.command("kiln.install_help").unwrap();
        assert_eq!(
            fake.state().urls,
            ["https://github.com/tpedneault/kiln/wiki/Installing"]
        );
        assert!(!fake.calls().iter().any(|c| c.starts_with("http")));
    }

    #[test]
    fn a_kiln_path_that_cant_run_is_named_and_a_working_one_brings_the_tasks() {
        let fake = demo();
        let dir = folder("doctor-path");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(told(&fake, "tasks").len(), 1);
        fake.on_spawn_error("/nowhere/kiln", "no such file");
        h.set_setting("kiln.path", Some("/nowhere/kiln"));
        assert_eq!(
            rows(&fake),
            ["kiln Bad kiln.path is /nowhere/kiln, which can't be run: set it to where kiln is, or empty it to use the PATH -> how to install Kiln (kiln.install_help)"]
        );
        assert!(fake.state().tasks.is_empty());
        assert_eq!(fake.status(), None);
        // Another kiln: the tasks run it, once.
        fake.on_spawn("/opt/kiln --version", Ran::ok("kiln 1.1.0\n"))
            .on_spawn("/opt/kiln list", Ran::ok(fixture("demo/list.json")))
            .on_spawn("/opt/kiln status", Ran::ok(fixture("demo/status.json")))
            .on_spawn("/opt/kiln check", Ran::ok("[]"));
        h.set_setting("kiln.path", Some("/opt/kiln"));
        assert_eq!(rows(&fake)[0], "kiln Ok kiln 1.1.0, /opt/kiln");
        assert_eq!(
            fenix_test_kit::line(&fake.state().tasks[0].cmd),
            "/opt/kiln check"
        );
        assert_eq!(told(&fake, "tasks").len(), 3);
        h.command("kiln.reload").unwrap();
        assert_eq!(told(&fake, "tasks").len(), 3);
    }

    #[test]
    fn not_a_kiln_project_clears_everything_once() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, fixture("errors/no-kiln-toml.stderr")),
        );
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        h.command("kiln.reload").unwrap();
        assert!(fake.state().checks.is_empty());
        assert!(fake.state().tasks.is_empty());
        assert_eq!(fake.status(), None);
        assert_eq!(told(&fake, "doctor"), ["doctor 0"]);
        assert_eq!(told(&fake, "tasks"), ["tasks "]);
        assert_eq!(told(&fake, "status"), ["status -"]);
    }

    #[test]
    fn a_project_kiln_cant_read_says_kilns_line_in_the_doctor() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, "kiln: kiln.toml:4: unknown key `nmae`"),
        );
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        let r = rows(&fake);
        assert_eq!(r[0], "kiln Ok kiln 1.0.0, /usr/bin/kiln");
        assert_eq!(r[1], "kiln.toml Bad kiln: kiln.toml:4: unknown key `nmae`");
        // Usable: kiln's own checks can be run to see more.
        assert_eq!(told(&fake, "tasks"), [DEMO_TASKS]);
        assert_eq!(fake.status(), None);
    }

    #[test]
    fn the_doctor_follows_the_language_server() {
        let fake = demo();
        let dir = folder("doctor-lsp");
        std::fs::write(dir.join(".fenix/tools.json"), TOOLS).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(
            rows(&fake)[2],
            "language server Ok left to .fenix/tools.json, which runs kiln lsp"
        );
        std::fs::write(dir.join(".fenix/tools.json"), "{}").unwrap();
        h.saved(&dir.join(".fenix/tools.json").display().to_string());
        assert!(rows(&fake)[2].starts_with("language server Info kiln lsp, for markdown only"));
        assert_eq!(told(&fake, "doctor").len(), 2);
    }

    // -- Prompts, pickers and edits asked by a tab ---------------------------

    /// The demo opened, its Check tab a [`Flow`](pages::frame::tests::Flow)
    /// and shown.
    fn flowing(fake: &Fake) -> (Harness<Kiln>, u64) {
        let (mut h, page) = opened(fake);
        h.ext
            .pages
            .get_mut(&page)
            .unwrap()
            .plan
            .set_view(Tab::Check, Box::<pages::frame::tests::Flow>::default());
        h.keys(page, "9");
        (h, page)
    }

    fn lines_with(fake: &Fake, what: &str) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.contains(what))
            .collect()
    }

    #[test]
    fn a_tabs_flow_goes_through_fenixs_picker_and_prompt_to_one_kiln_edit() {
        let fake = demo();
        fake.on_pick("Link REQ-001 as", "verifies")
            .on_prompt("verifies what", "TEST-001")
            .on_spawn(
                "kiln link REQ-001 verifies TEST-001",
                Ran::ok("REQ-001: + verifies TEST-001\n"),
            );
        let (mut h, page) = flowing(&fake);
        fake.take();
        h.key(page, Key::Char('x'));
        let calls = fake.take();
        let at = |what: &str| calls.iter().position(|c| c.contains(what));
        assert!(
            calls
                .iter()
                .any(|c| c.ends_with("Link REQ-001 as: verifies, implements")),
            "Fenix's own picker: {calls:?}"
        );
        assert!(calls
            .iter()
            .any(|c| c.starts_with("prompt ") && c.ends_with("REQ-001 verifies what []")));
        // One edit, then the project read again.
        let edit = at("kiln link REQ-001 verifies TEST-001").expect("the edit ran");
        let list = calls
            .iter()
            .rposition(|c| c.ends_with("kiln list --format json"))
            .expect("read again");
        assert!(edit < list, "{calls:?}");
        assert_eq!(foot(&mut h, page), "REQ-001: + verifies TEST-001");
        assert!(h
            .screen(page)
            .contains("heard: role=verifies, target=TEST-001, link done"));
        assert!(h.ext.answers.is_empty() && h.ext.asked.is_empty());
    }

    #[test]
    fn a_cancelled_pick_or_prompt_ends_the_flow_and_runs_nothing() {
        let fake = demo();
        fake.on_pick_nothing("Link REQ-001 as");
        let (mut h, page) = flowing(&fake);
        h.key(page, Key::Char('x'));
        assert!(lines_with(&fake, "prompt ").is_empty());
        assert!(h.ext.answers.is_empty());
        fake.on_pick("Link REQ-001 as", "implements")
            .on_prompt_nothing("implements what");
        h.key(page, Key::Char('x'));
        assert_eq!(lines_with(&fake, "prompt ").len(), 1);
        assert!(lines_with(&fake, " link ").is_empty(), "no edit");
        assert!(h.ext.answers.is_empty());
        assert!(h.screen(page).contains("heard: role=implements\n"));
    }

    #[test]
    fn answers_for_a_closed_page_are_dropped() {
        let fake = demo();
        let (mut h, page) = flowing(&fake);
        // Asked, unanswered; the page closes; the answer comes after.
        h.key(page, Key::Char('x'));
        let picker = fake.state().pickers.last().unwrap().id;
        h.close(page);
        assert!(h.ext.answers.is_empty(), "let go with its page");
        h.event(Event::Picked((picker, Some("verifies".into()))));
        assert!(lines_with(&fake, "prompt ").is_empty());
        // An edit out as its page closes runs on; its outcome is dropped.
        let fake = demo();
        fake.on_pick("Link REQ-001 as", "verifies")
            .on_prompt("verifies what", "TEST-001")
            .on_spawn("kiln link", Ran::running(""));
        let (mut h, page) = flowing(&fake);
        h.key(page, Key::Char('x'));
        let link = fake.process("kiln link").unwrap().id;
        h.close(page);
        h.said(link, "REQ-001: + verifies TEST-001", false);
        h.exited(link, Some(0));
        assert!(h.ext.asked.is_empty());
        assert!(
            fake.calls()
                .iter()
                .rev()
                .take_while(|c| !c.contains("kiln link"))
                .any(|c| c.ends_with("kiln list --format json")),
            "read again all the same"
        );
    }

    #[test]
    fn a_refused_edit_says_kilns_line_and_the_flow_asks_again() {
        let fake = demo();
        fake.on_pick("Link REQ-001 as", "verifies")
            .on_prompt("verifies what", "TEST-001")
            .on_spawn(
                "kiln link",
                Ran::fails(1, "kiln: design/requirements/REQ-001-readings-are-logged-every-second.md changed on disk since it was read"),
            );
        let (mut h, page) = flowing(&fake);
        h.key(page, Key::Char('x'));
        assert_eq!(
            foot(&mut h, page),
            "kiln: design/requirements/REQ-001-readings-are-logged-every-second.md changed on disk since it was read"
        );
        let prompts = lines_with(&fake, "prompt ");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert!(prompts[1].ends_with("REQ-001 again []"));
        assert_eq!(h.ext.answers.len(), 1, "the second prompt waits");
    }

    // -- Commands from a buffer ----------------------------------------------

    /// The demo, with you in `path` (from the project's top) on `text`'s
    /// line `line`, column `column`.
    fn in_file(path: &str, text: &str, line: u32, column: u32) -> Fake {
        let fake = demo();
        fake.project("demo", "/home/me/demo")
            .in_file(&format!("/home/me/demo/{path}"), "md", text)
            .cursor(line, column);
        fake
    }

    #[test]
    fn kiln_show_opens_the_item_under_the_cursor_on_the_item_tab() {
        let fake = in_file("src/log.rs", "fn log() {}\n// implements: REQ-001\n", 1, 17);
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.size = WIDE;
        h.command("kiln.show").unwrap();
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Item);
        assert!(h
            .screen(page)
            .contains("\nrequirement   REQ-001   Readings are logged every second"));
        // Again, on another ID: the same page, focused, its history grown.
        fake.in_file(
            "/home/me/demo/design/tasks/TASK-004-add-the-export-button.md",
            "md",
            "depends-on = [\"TASK-002\"]\n",
        )
        .cursor(0, 17);
        h.command("kiln.show").unwrap();
        assert_eq!(h.page("plan"), page);
        assert!(fake.calls().contains(&format!("page-focus {page}")));
        assert!(
            h.screen(page)
                .contains("TASK-002  Write readings to SQLite"),
            "{}",
            h.screen(page)
        );
        assert!(
            foot(&mut h, page).starts_with("< back"),
            "{}",
            foot(&mut h, page)
        );
    }

    #[test]
    fn kiln_impact_off_any_id_in_an_items_file_takes_that_item() {
        let fake = in_file(
            "design/tasks/TASK-006-draft-the-user-guide.md",
            "Write it for people who never used the logger.\n",
            0,
            6,
        );
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.size = WIDE;
        h.command("kiln.impact").unwrap();
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Impact);
        assert!(h
            .screen(page)
            .contains("\nTASK-006  Draft the user guide\n"));
        assert!(h.screen(page).contains("nothing reaches TASK-006"));
    }

    #[test]
    fn no_id_here_says_what_it_looked_at_and_opens_nothing() {
        let fake = in_file("src/log.rs", "fn log() {}\n", 0, 3);
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.show").unwrap();
        assert_eq!(
            fake.notifications(),
            ["Kiln: no Kiln ID under the cursor on line 1 of src/log.rs, and src/log.rs isn't an item's file"]
        );
        assert!(fake.page("plan").is_none());
        // An ID kiln doesn't list is named.
        let fake = in_file("src/log.rs", "// implements: REQ-099\n", 0, 18);
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.impact").unwrap();
        assert_eq!(
            fake.notifications(),
            ["Kiln: REQ-099 (line 1 of src/log.rs) isn't an item of this project: kiln list has no such ID"]
        );
        // Not in a file at all.
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.show").unwrap();
        assert!(
            fake.notifications()[0].contains("no file in front"),
            "{:?}",
            fake.notifications()
        );
        // No kiln: why, in the project's words.
        let fake = Fake::new();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.show").unwrap();
        assert!(
            fake.notifications()[0].starts_with("Kiln: kiln isn't on the PATH"),
            "{:?}",
            fake.notifications()
        );
    }

    #[test]
    fn a_command_before_the_project_is_read_waits_for_it() {
        let fake = in_file("src/log.rs", "// implements: REQ-001\n", 0, 17);
        fake.on_spawn("kiln list", Ran::running(""));
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.show").unwrap();
        assert!(fake.page("plan").is_none(), "not yet: no IDs to know it by");
        // Where you were is kept: you've moved on by the time it's read.
        fake.in_file("/home/me/demo/README.md", "md", "nothing\n");
        let list = fake.process("kiln list").unwrap().id;
        for line in fixture("demo/list.json").lines() {
            h.said(list, line, false);
        }
        h.exited(list, Some(0));
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Item);
        assert!(h.screen(page).contains("REQ-001"));
        assert!(fake.notifications().is_empty());
    }

    #[test]
    fn kiln_find_picks_from_every_item_then_shows_it() {
        let fake = demo();
        fake.on_pick("Kiln item", "TASK-004  Add the export button");
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.size = WIDE;
        h.command("kiln.find").unwrap();
        let picks = lines_with(&fake, "pick ");
        assert_eq!(picks.len(), 1);
        assert!(picks[0].contains("Kiln item: REQ-001  Readings are logged every second, "));
        assert_eq!(fake.state().pickers[0].items.len(), 16);
        assert_eq!(fake.state().pickers[0].items[0].detail, "approved");
        let page = h.page("plan");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Item);
        assert!(
            h.screen(page)
                .contains("\ntask          TASK-004  Add the export button  todo\n"),
            "{}",
            h.screen(page)
        );
        // Closed with nothing: nothing happens.
        let fake = demo();
        fake.on_pick_nothing("Kiln item");
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.find").unwrap();
        assert!(fake.page("plan").is_none());
        assert!(h.ext.answers.is_empty());
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        let mut h: Harness<Kiln> = Harness::new();
        assert_eq!(
            h.command("kiln.nope"),
            Err("no command kiln.nope".to_string())
        );
    }
}
