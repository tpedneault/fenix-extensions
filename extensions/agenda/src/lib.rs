//! The agenda, as a WebAssembly extension: tasks on a board, a clock for
//! your time, a week's timesheet, and -- with the Jira extension -- each
//! task's link to an issue. What was Fenix 1.x's `SPC A`, keeping its
//! `agenda.json`, its keys and its commands.
//!
//! `model` is the data, `page` the page (laid out cell by cell, as it was
//! in Fenix), `sync` the Jira side. Here: opening the page, carrying out
//! what its keys ask, the clock and its pickers, Home, the modeline, and
//! the control channel's methods.

pub mod clock;
pub mod model;
pub mod page;
pub mod sync;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chrono::{DateTime, NaiveDate};
use fenix_extension_api::{self as api, host, run, Event, Extension, HomeMark, HomeRow, HomeSection, Key, Page, PickerItem};

use clock::Local;
use model::{AgendaStore, CodeRef, OpKind, Priority, Status, Task, TaskId};
use page::{Action as A, AgendaPage, NewTask, Project, Tab};
use sync::{Blocked, Done, IssueFetch, Pick, PickerCtx};

/// Where the agenda is kept: `agenda.json`, adopted from Fenix 1.x.
const KEPT: &str = "/data/agenda.json";

/// How often a running clock's last-seen time is saved, in seconds.
const SEEN_SAVE_EVERY: u64 = 300;

/// The page's kind, for session restore.
const KIND: &str = "agenda";

/// One row of the clock picker (`SPC A t`), or of the question asked
/// when the clock ran on while you were away.
#[derive(Debug, Clone, PartialEq)]
enum ClockPick {
    Stop,
    Start(TaskId),
    /// Keep the time up to when you were last seen, and stop there.
    KeepUntil(DateTime<Local>),
    /// It was all work: carry on.
    KeepAll,
    /// Drop this sitting.
    Drop,
}

/// What a picker, prompt or compose pane is for.
enum Asked {
    Clock(Vec<ClockPick>),
    Sync(PickerCtx),
    /// Linking a task to an issue typed by key.
    LinkKey(TaskId),
    Description(TaskId),
    Comment(TaskId),
}

/// What `SPC A h` captured, waiting for the new-task form to finish.
struct Capture {
    code: CodeRef,
    text: String,
}

#[derive(Default)]
pub struct Agenda {
    store: AgendaStore,
    /// `agenda.json`'s modification time as last read or written: another
    /// one means someone else wrote it.
    stamp: Option<std::time::SystemTime>,
    /// The page, when it's open: Fenix's id for it.
    page: Option<(u64, AgendaPage)>,
    /// The project of the file you were in when the page last opened.
    project: Option<Project>,
    capture: Option<Capture>,
    /// The clock picker open is asking about time away.
    gap_asked: bool,
    seen_saved: Option<Instant>,
    /// Worklog review edits and drops, by (task, day).
    worklog_edits: HashMap<(TaskId, NaiveDate), i64>,
    worklog_dropped: HashSet<(TaskId, NaiveDate)>,
    asked: HashMap<u64, Asked>,
    sync: sync::Sync,
    /// The minute the page and the modeline were last drawn in.
    minute: i64,
    /// Its timer: the minute, and `agenda.json` written elsewhere.
    tick: Option<u64>,
}

impl Agenda {
    fn categories(&self) -> Vec<String> {
        api::setting("agenda.categories").map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
    }

    fn minutes_setting(key: &str, default: u32) -> u32 {
        api::setting(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    fn round(&self) -> u32 {
        Self::minutes_setting("agenda.worklog_round", 15)
    }

    // -- Saving -----------------------------------------------------------

    fn save(&mut self) {
        if let Err(err) = model::save(std::path::Path::new(KEPT), &self.store) {
            host::notify(api::Level::Error, &format!("couldn't save agenda.json: {err}"));
        }
        self.stamp = std::fs::metadata(KEPT).and_then(|m| m.modified()).ok();
    }

    /// `agenda.json` written by someone else since: taken in, so the next
    /// save doesn't put back what they changed. A file gone or unreadable
    /// is left alone, and the agenda kept.
    fn reload_if_changed(&mut self) {
        let stamp = std::fs::metadata(KEPT).and_then(|m| m.modified()).ok();
        if stamp.is_none() || stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        match model::reload(std::path::Path::new(KEPT)) {
            Ok(Some(store)) => {
                self.store = store;
                self.refresh();
            }
            Ok(None) => {}
            Err(err) => host::notify(api::Level::Error, &format!("agenda.json changed on disk but can't be read ({err}) -- keeping the agenda as it was")),
        }
    }

    /// Saved, and everything showing it drawn again.
    fn save_and_refresh(&mut self) {
        self.save();
        self.refresh();
    }

    /// The page, the modeline and Home, again.
    fn refresh(&mut self) {
        if let Some((id, page)) = &mut self.page {
            page.forget_missing(&self.store);
            api::page::refresh(*id);
        }
        self.status();
        self.home();
    }

    /// Saves, draws, and has the sync send what the edit queued.
    fn after_edit(&mut self) {
        self.save_and_refresh();
        self.drain_outbox();
    }

    // -- The modeline and Home --------------------------------------------

    /// The running clock in the modeline -- easy to forget once you've
    /// gone and started the work, which is when it needs to show.
    fn status(&self) {
        let Some(timer) = &self.store.active_timer else {
            let busy = self.sync.pulling.then_some("syncing Jira");
            return api::status(busy);
        };
        let title = self.store.task(timer.task_id).map(|t| t.title.as_str()).unwrap_or("?");
        let elapsed = page::format_minutes((clock::now() - timer.started_at).num_minutes());
        api::status(Some(&format!("\u{23f1} {title} {elapsed}")));
    }

    /// Home's today: the one on the clock, then in progress, then what's
    /// due within a week, then what's ready to start.
    fn home(&self) {
        let now = clock::now();
        let live = self.store.active_timer.as_ref().map(|t| (t.task_id, now.signed_duration_since(t.started_at)));
        let week = now.date_naive() + chrono::Duration::days(7);
        let store = &self.store;
        let mut open: Vec<&Task> = store
            .tasks
            .iter()
            .filter(|t| !t.archived && t.status != Status::Done)
            .filter(|t| t.status == Status::InProgress || (t.status == Status::Todo && store.is_ready(t.id)) || live.is_some_and(|(id, _)| id == t.id))
            .collect();
        open.sort_by_key(|t| (live.is_none_or(|(id, _)| id != t.id), t.status != Status::InProgress, t.due.filter(|d| *d <= week).is_none(), t.due, std::cmp::Reverse(t.priority), t.order));
        let rows = open
            .into_iter()
            .map(|t| HomeRow {
                name: keyed(t),
                detail: String::new(),
                mark: match live.filter(|(id, _)| *id == t.id) {
                    Some((_, d)) => HomeMark::Live(format_elapsed(d)),
                    None => HomeMark::Task(matches!(t.priority, Priority::High | Priority::Urgent)),
                },
                value: t.id.0.to_string(),
            })
            .collect();
        api::home(Some(&HomeSection { title: "today".into(), key: "SPC A a".into(), rows, most: 4 }));
    }

    // -- The page ---------------------------------------------------------

    fn worklog_rows(&self) -> Vec<model::WorklogRow> {
        let mut rows = self.store.worklog_batch(self.round());
        rows.retain(|r| !self.worklog_dropped.contains(&(r.task, r.date)));
        for row in &mut rows {
            if let Some(&minutes) = self.worklog_edits.get(&(row.task, row.date)) {
                row.minutes = minutes;
            }
        }
        rows
    }

    /// "synced 3m ago", "syncing", or nothing when Jira isn't set up.
    fn sync_label(&self) -> String {
        if self.sync.config().is_none() {
            return String::new();
        }
        if self.sync.pulling {
            return "syncing".to_string();
        }
        match self.sync.last_pull {
            Some(at) if at.elapsed().as_secs() < 60 => "synced just now".to_string(),
            Some(at) => format!("synced {}m ago", at.elapsed().as_secs() / 60),
            None if self.store.tasks.iter().any(|t| t.jira.is_some()) => "not synced yet".to_string(),
            None => String::new(),
        }
    }

    /// The page, on `tab` when given, else where it was left.
    fn open_page(&mut self, tab: Option<Tab>) {
        self.sync_if_stale();
        let project = here_project();
        if project.is_some() {
            self.project = project.clone();
        }
        match &mut self.page {
            Some((_, page)) => {
                if project.is_some() {
                    page.project = project;
                }
                if let Some(tab) = tab {
                    page.show_tab(tab);
                }
                page.note = None;
            }
            None => {
                let id = api::page::open(KIND, "agenda");
                self.page = Some((id, AgendaPage::new(tab.unwrap_or(Tab::Today), self.project.clone())));
            }
        }
        self.worklog_edits.clear();
        self.worklog_dropped.clear();
        self.refresh();
    }

    /// Presses `key` on the page, as if typed.
    fn page_key_now(&mut self, key: Key) {
        let Some((id, mut page)) = self.page.take() else { return };
        let worklogs = self.worklog_rows();
        let categories = self.categories();
        let ctx = page::Ctx { store: &self.store, now: clock::now(), categories: &categories, worklogs: &worklogs, round: self.round(), sync: self.sync_label() };
        let action = page.key(key, &ctx);
        self.page = Some((id, page));
        api::page::refresh(id);
        self.page_action(action);
    }

    fn page_id(&self) -> Option<u64> {
        self.page.as_ref().map(|(id, _)| *id)
    }

    fn ensure_category(&mut self, name: &str) {
        let mut categories = self.categories();
        if categories.iter().any(|c| c.eq_ignore_ascii_case(name)) {
            return;
        }
        categories.push(name.to_string());
        if let Err(err) = api::set_setting("agenda.categories", Some(&categories.join(","))) {
            host::notify(api::Level::Error, &format!("couldn't save the categories: {err}"));
        }
    }

    fn create(&mut self, spec: NewTask) {
        let capture = self.capture.take();
        let description = capture.as_ref().map(|c| c.text.clone()).unwrap_or_default();
        if let Some(category) = &spec.category {
            self.ensure_category(category);
        }
        let category = spec.category.as_ref().map(|c| self.categories().iter().find(|x| x.eq_ignore_ascii_case(c)).cloned().unwrap_or_else(|| c.clone()));
        let task = self.store.create_task(spec.title.clone(), description, spec.priority, category);
        if spec.status != Status::Todo {
            self.store.move_to_status(task, spec.status);
        }
        self.store.set_due(task, spec.due);
        if let Some(capture) = capture {
            self.store.set_code(task, Some(capture.code));
        }
        if spec.clock {
            self.store.clock_in(task);
        }
        self.save_and_refresh();
        if let Some((_, page)) = &mut self.page {
            page.select(task);
            page.note = Some((format!("added \"{}\"", spec.title), false));
        }
        if let Some(key) = spec.link {
            self.link_key(task, key);
        }
    }

    fn page_action(&mut self, action: A) {
        match action {
            A::None => {}
            A::Close => {
                if let Some((id, _)) = self.page.take() {
                    api::page::close(id);
                }
            }
            A::SetStatus(task, status) => self.set_status(task, status),
            A::JiraStatus(task) => self.with_jira(|a| a.linked_status_picker(task)),
            A::SetPriority(task, priority) => {
                self.store.set_priority(task, priority);
                self.save_and_refresh();
            }
            A::JiraPriority(task) => self.with_jira(|_| sync::priorities(task)),
            A::SetCategory(task, category) => {
                self.store.set_category(task, category);
                self.save_and_refresh();
            }
            A::AddCategory(task, name) => {
                self.ensure_category(&name);
                self.store.set_category(task, Some(name));
                self.save_and_refresh();
            }
            A::SetDue(task, due) => {
                self.store.set_due(task, due);
                self.store.enqueue(task, OpKind::SetDue(due.map(|d| d.format("%Y-%m-%d").to_string())));
                self.after_edit();
            }
            A::SetTitle(task, title) => {
                self.store.set_title(task, title.clone());
                self.store.enqueue(task, OpKind::SetSummary(title));
                self.after_edit();
            }
            A::EditDescription(task) => {
                let seed = self.store.task(task).map(|t| t.description.clone()).unwrap_or_default();
                let id = api::compose("Description", "save", &seed, true);
                self.asked.insert(id, Asked::Description(task));
            }
            A::AddNote(task, text) => {
                self.store.add_note(task, text);
                self.save_and_refresh();
            }
            A::EditNote(task, i, text) => {
                self.store.edit_note(task, i, text);
                self.save_and_refresh();
            }
            A::RemoveNote(task, i) => {
                self.store.remove_note(task, i);
                self.save_and_refresh();
            }
            A::Comment(task) => {
                let id = api::compose("Jira comment", "post", "", false);
                self.asked.insert(id, Asked::Comment(task));
            }
            A::PostNote(task, i) => {
                let Some(t) = self.store.task(task) else { return };
                let (Some(key), Some(note)) = (t.jira_key().map(str::to_string), t.notes.get(i).map(|n| n.text.clone())) else { return };
                self.store.enqueue(task, OpKind::AddComment(note));
                api::notify(&format!("Posting the note to {key} as a comment..."));
                self.after_edit();
            }
            A::Clock(task) => {
                if matches!(&self.store.active_timer, Some(t) if t.task_id == task) {
                    self.store.clock_out();
                } else {
                    self.store.clock_in(task);
                }
                self.save_and_refresh();
            }
            A::LogSpan(task, start, end) => {
                self.store.log_span(task, start, end);
                self.save_and_refresh();
            }
            A::EditTime(task, i, start, end) => {
                if !self.store.edit_time_entry(task, i, start, end) {
                    host::notify(api::Level::Error, "the end has to be after the start");
                }
                self.save_and_refresh();
            }
            A::RemoveTime(task, i) => {
                self.store.remove_time_entry(task, i);
                self.save_and_refresh();
            }
            A::AddCheck(task, text) => {
                self.store.add_subtask(task, text);
                self.save_and_refresh();
            }
            A::EditCheck(task, i, text) => {
                self.store.edit_subtask(task, i, text);
                self.save_and_refresh();
            }
            A::ToggleCheck(task, i) => {
                self.store.toggle_subtask(task, i);
                self.save_and_refresh();
            }
            A::RemoveCheck(task, i) => {
                self.store.remove_subtask(task, i);
                self.save_and_refresh();
            }
            A::AddDependency(task, on) => {
                if !self.store.add_dependency(task, on) {
                    host::notify(api::Level::Error, "that would make the two wait on each other");
                }
                self.save_and_refresh();
            }
            A::RemoveDependency(task, on) => {
                self.store.remove_dependency(task, on);
                self.save_and_refresh();
            }
            A::Resolve(task, field, mine) => {
                self.store.resolve_conflict(task, field, mine);
                self.after_edit();
            }
            A::Link(task) => self.with_jira(|a| a.start_link(task)),
            A::Assign(task) => self.with_jira(|a| a.assignee_picker(task)),
            A::CopyLink(task) => {
                if let Some(url) = self.issue_url(task) {
                    api::copy(&url);
                    api::notify(&format!("Copied: {url}"));
                }
            }
            A::Browser(task) => {
                if let Some(url) = self.issue_url(task) {
                    let _ = api::open_url(&url);
                }
            }
            A::Retry(task) => self.with_jira(|a| a.retry(task)),
            A::OpenCode(task) => {
                let Some(code) = self.store.task(task).and_then(|t| t.code.clone()) else { return };
                host::open_file(&code.path.to_string_lossy(), Some(code.line as u32));
            }
            A::Archive(task) => {
                self.store.archive(task);
                self.save_and_refresh();
            }
            A::Delete(task) => {
                let key = self.store.task(task).and_then(|t| t.jira_key()).map(str::to_string);
                self.store.delete(task);
                self.save_and_refresh();
                api::notify(&match key {
                    Some(key) => format!("Deleted -- {key} in Jira isn't affected"),
                    None => "Deleted".to_string(),
                });
            }
            A::Reorder(task, by) => {
                self.store.reorder_within_status(task, by);
                self.save_and_refresh();
            }
            A::Create(spec) => self.create(spec),
            A::Sync => self.with_jira(|a| a.sync_now(false)),
            A::WorklogMinutes(task, date, minutes) => {
                self.worklog_edits.insert((task, date), minutes);
                self.refresh();
            }
            A::WorklogDrop(task, date) => {
                self.worklog_dropped.insert((task, date));
                self.refresh();
            }
            A::WorklogDismiss(task, date) => {
                let Some(row) = self.worklog_rows().into_iter().find(|r| r.task == task && r.date == date) else { return };
                self.store.mark_sent(task, &row.entries);
                self.save_and_refresh();
                api::notify(&format!("{} on {}: dismissed, won't be sent", row.key, row.date));
            }
            A::SendWorklogs => self.with_jira(|a| a.send_worklogs()),
            A::Copy(text) => {
                api::copy(&text);
                api::notify("Copied the week");
            }
        }
    }

    // -- The clock --------------------------------------------------------

    /// `SPC A t`: stop the running clock, resume what you worked on last,
    /// or switch -- recent tasks first, then what's in progress.
    fn toggle_clock(&mut self) {
        let store = &self.store;
        let running = store.active_timer.as_ref().map(|t| t.task_id);
        let mut rows: Vec<(String, ClockPick)> = Vec::new();
        if let Some(timer) = &store.active_timer {
            let name = store.task(timer.task_id).map(keyed).unwrap_or_default();
            let minutes = (clock::now() - timer.started_at).num_minutes();
            rows.push((format!("Stop  ·  {name}  ·  {}", page::format_minutes(minutes)), ClockPick::Stop));
        }
        let mut seen: Vec<TaskId> = running.into_iter().collect();
        let mut add = |t: &Task, prefix: &str, rows: &mut Vec<(String, ClockPick)>| {
            if !seen.contains(&t.id) {
                seen.push(t.id);
                rows.push((format!("{prefix}{}", keyed(t)), ClockPick::Start(t.id)));
            }
        };
        for (i, (id, _)) in store.recently_worked().into_iter().enumerate() {
            if let Some(t) = store.task(id).filter(|t| t.status != Status::Done) {
                let prefix = if i == 0 && running.is_none() { "Resume  ·  " } else { "" };
                add(t, prefix, &mut rows);
            }
        }
        let mut rest: Vec<&Task> = store.tasks.iter().filter(|t| !t.archived && t.status != Status::Done).collect();
        rest.sort_by_key(|t| (t.status != Status::InProgress, std::cmp::Reverse(t.priority), t.order));
        for t in rest {
            add(t, "", &mut rows);
        }
        if rows.is_empty() {
            return host::notify(api::Level::Error, "no agenda tasks to clock in on yet -- SPC A n to add one");
        }
        self.gap_asked = false;
        self.clock_picker("Clock", rows);
    }

    fn clock_picker(&mut self, title: &str, rows: Vec<(String, ClockPick)>) {
        let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
        let id = api::pick(title, &items);
        self.asked.insert(id, Asked::Clock(rows.into_iter().map(|(_, p)| p).collect()));
    }

    /// `SPC A T`: the clock back on what you worked on last.
    fn resume(&mut self) {
        if let Some(timer) = &self.store.active_timer {
            let name = self.store.task(timer.task_id).map(|t| t.title.clone()).unwrap_or_default();
            return api::notify(&format!("The clock is already on \"{name}\""));
        }
        let Some((id, _)) = self.store.recently_worked().into_iter().next() else {
            return host::notify(api::Level::Error, "nothing worked on yet to resume -- SPC A t picks a task");
        };
        self.clock_pick(ClockPick::Start(id));
    }

    fn clock_pick(&mut self, pick: ClockPick) {
        self.gap_asked = false;
        match pick {
            ClockPick::Stop => self.store.clock_out(),
            ClockPick::Start(id) => {
                self.store.clock_in(id);
                let name = self.store.task(id).map(|t| t.title.clone()).unwrap_or_default();
                api::notify(&format!("Clock on \"{name}\""));
            }
            ClockPick::KeepUntil(at) => {
                self.store.clock_out_at(at);
                api::notify(&format!("Kept the time until {} -- SPC A T starts the clock again", at.format("%H:%M")));
            }
            ClockPick::KeepAll => {
                if let Some(timer) = &mut self.store.active_timer {
                    timer.last_seen = Some(clock::now());
                }
            }
            ClockPick::Drop => {
                self.store.drop_timer();
                api::notify("Dropped that sitting -- SPC A T starts the clock again");
            }
        }
        self.seen_saved = Some(Instant::now());
        self.save_and_refresh();
    }

    /// A key was pressed: you're here while the clock runs -- or, back
    /// after longer than `agenda.idle_minutes`, asked what to keep.
    fn note_activity(&mut self) {
        let Some(timer) = &self.store.active_timer else { return };
        if self.gap_asked {
            return;
        }
        let now = clock::now();
        let last = timer.last_seen.map(|seen| seen.max(timer.started_at));
        let idle = Self::minutes_setting("agenda.idle_minutes", 60);
        if let Some(last) = last.filter(|last| idle > 0 && now - *last > chrono::Duration::minutes(i64::from(idle))) {
            return self.ask_about_gap(last);
        }
        if let Some(timer) = &mut self.store.active_timer {
            timer.last_seen = Some(now);
        }
        if self.seen_saved.is_none_or(|at| at.elapsed().as_secs() >= SEEN_SAVE_EVERY) {
            self.seen_saved = Some(Instant::now());
            self.save();
        }
    }

    fn ask_about_gap(&mut self, last: DateTime<Local>) {
        let Some(timer) = &self.store.active_timer else { return };
        let now = clock::now();
        let name = self.store.task(timer.task_id).map(|t| t.title.clone()).unwrap_or_default();
        let kept = page::format_minutes((last - timer.started_at).num_minutes());
        let all = page::format_minutes((now - timer.started_at).num_minutes());
        let rows = vec![
            (format!("Keep until {}  ·  {kept}", last.format("%H:%M")), ClockPick::KeepUntil(last)),
            (format!("Keep all of it  ·  {all}"), ClockPick::KeepAll),
            ("Drop this sitting".to_string(), ClockPick::Drop),
        ];
        api::notify(&format!("The clock ran on \"{name}\" while you were away -- nothing happened in Fenix from {} to {}", last.format("%H:%M"), now.format("%H:%M")));
        self.gap_asked = true;
        self.clock_picker("Away", rows);
    }

    /// `SPC A h`: a task about the selection, or the line, you're on --
    /// its file and line kept to come back to.
    fn task_from_here(&mut self) {
        let Some(here) = api::here().filter(|h| h.path.is_some()) else {
            return host::notify(api::Level::Error, "SPC A h makes a task from a file -- this buffer isn't one");
        };
        let path = std::path::PathBuf::from(here.path.clone().unwrap_or_default());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = format!("{name}:{}\n\n```{}\n{}\n```", here.line, here.language, here.text.trim_end());
        self.capture = Some(Capture { code: CodeRef { path, line: here.line as usize }, text });
        self.open_page(None);
        self.page_key_now(Key::Char('n'));
    }

    // -- The control channel ----------------------------------------------

    fn control_tasks(&self, payload: &str) -> Result<String, String> {
        #[derive(serde::Deserialize, Default)]
        struct Params {
            status: Option<Vec<String>>,
        }
        let params: Params = serde_json::from_str(payload).unwrap_or_default();
        let name = |s: Status| match s {
            Status::Todo => "todo",
            Status::InProgress => "inProgress",
            Status::Blocked => "blocked",
            Status::Done => "done",
        };
        let mut tasks: Vec<&Task> = self.store.tasks.iter().filter(|t| !t.archived).filter(|t| params.status.as_ref().is_none_or(|wanted| wanted.iter().any(|w| w == name(t.status)))).collect();
        tasks.sort_by_key(|t| (Status::ALL.iter().position(|s| *s == t.status), t.order));
        let tasks: Vec<serde_json::Value> = tasks
            .into_iter()
            // As the control protocol writes them (`fenix_control::AgendaTask`).
            .map(|t| {
                let mut task = serde_json::json!({ "id": t.id.0, "title": t.title, "status": name(t.status), "elapsedMinutes": self.store.elapsed_on(t.id).num_minutes() });
                if let Some(key) = t.jira_key() {
                    task["jiraKey"] = key.into();
                }
                task
            })
            .collect();
        Ok(serde_json::json!({ "tasks": tasks }).to_string())
    }

    fn control_timer(&self) -> String {
        match &self.store.active_timer {
            Some(timer) => serde_json::json!({
                "taskId": timer.task_id.0,
                "title": self.store.task(timer.task_id).map(|t| t.title.clone()).unwrap_or_default(),
                "since": timer.started_at.to_rfc3339(),
            })
            .to_string(),
            None => "null".into(),
        }
    }
}

/// The project of the file you're in, and its Jira key.
fn here_project() -> Option<Project> {
    let project = api::here()?.project?;
    Some(Project { jira_key: project.meta.iter().find(|(k, _)| k == "jira").map(|(_, v)| v.clone()), name: project.name })
}

/// "PROJ-12 Its title", or its title.
fn keyed(t: &Task) -> String {
    match t.jira_key() {
        Some(key) => format!("{key} {}", t.title),
        None => t.title.clone(),
    }
}

/// `0:18`, `1:05`: how long a clock has run, as Home shows it.
fn format_elapsed(d: chrono::Duration) -> String {
    let minutes = d.num_minutes().max(0);
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

// -- Jira -----------------------------------------------------------------

impl Agenda {
    /// Runs `f` when Jira is there, or says it isn't.
    fn with_jira(&mut self, f: impl FnOnce(&mut Agenda)) {
        match &self.sync.config {
            Some(Ok(c)) if c.base_url.is_some() => f(self),
            Some(Ok(_)) => host::notify(api::Level::Error, "Jira isn't set up yet -- set its server and token in SPC , (Jira)"),
            _ => api::notify("Nothing syncs the agenda -- that needs the Jira extension (SPC h x)"),
        }
    }

    fn config(&self) -> sync::Config {
        self.sync.config().cloned().unwrap_or_default()
    }

    fn remote_update(&self, d: &fenix_jira_client::IssueDetail) -> model::RemoteUpdate {
        sync::remote_update(&self.config(), self.sync.me.as_deref(), d)
    }

    fn issue_url(&self, task: TaskId) -> Option<String> {
        let Some(key) = self.store.task(task).and_then(|t| t.jira_key()).map(str::to_string) else {
            api::notify("This task isn't linked to Jira -- I links it");
            return None;
        };
        let base = self.sync.config()?.base_url.clone()?;
        Some(format!("{}/browse/{key}", base.trim_end_matches('/')))
    }

    /// Linked tasks stay current while Fenix runs.
    fn start_ticker(&mut self) {
        if self.sync.ticker.is_some() {
            return;
        }
        let minutes = self.config().sync_minutes;
        if minutes > 0 {
            self.sync.ticker = Some(host::set_timer(minutes * 60_000, true));
        }
    }

    /// `SPC A s`: every linked task from Jira in one search, then what's
    /// waiting sent. `quiet` keeps a failure from shouting.
    fn sync_now(&mut self, quiet: bool) {
        if self.sync.config().is_none() {
            return;
        }
        self.start_ticker();
        if self.sync.pulling {
            return;
        }
        let keys: Vec<String> = self.store.tasks.iter().filter_map(|t| t.jira_key().map(str::to_string)).collect();
        if keys.is_empty() {
            if !quiet {
                api::notify("No tasks are linked to Jira yet -- I on a task links one, SPC A i imports your issues");
            }
            return;
        }
        let want_flag = self.config().flags_anywhere();
        self.sync.pulling = true;
        self.sync.last_pull = Some(Instant::now());
        self.status();
        sync::pull(keys, quiet, self.sync.me.is_none(), want_flag, self.sync.flag_field.clone());
    }

    fn sync_if_stale(&mut self) {
        let fresh = self.sync.last_pull.is_some_and(|at| at.elapsed() < sync::STALE_AFTER);
        if !fresh && self.store.tasks.iter().any(|t| t.jira.is_some()) {
            self.sync_now(true);
        }
    }

    fn pulled(&mut self, keys: Vec<String>, quiet: bool, result: Result<Vec<fenix_jira_client::IssueDetail>, String>) {
        self.sync.pulling = false;
        let issues = match result {
            Ok(issues) => issues,
            Err(err) => {
                if !quiet {
                    host::notify(api::Level::Error, &format!("Jira sync failed: {err}"));
                }
                return self.status();
            }
        };
        let mut conflicts = 0;
        for detail in &issues {
            let Some(id) = self.store.find_by_key(&detail.key) else { continue };
            let update = self.remote_update(detail);
            self.store.apply_remote(id, update);
            if self.store.task(id).and_then(|t| t.jira.as_ref()).is_some_and(|l| l.error.as_deref() == Some(sync::NOT_FOUND)) {
                self.store.set_link_error(id, None);
            }
            conflicts += self.store.task(id).and_then(|t| t.jira.as_ref()).map_or(0, |l| l.conflicts.len());
        }
        for key in &keys {
            if !issues.iter().any(|d| &d.key == key) {
                if let Some(id) = self.store.find_by_key(key) {
                    self.store.set_link_error(id, Some(sync::NOT_FOUND.to_string()));
                }
            }
        }
        self.save_and_refresh();
        if conflicts > 0 {
            host::notify(api::Level::Error, &format!("Jira sync: {conflicts} conflicting change(s) -- open the task and press Enter on the conflict"));
        } else if !quiet {
            api::notify(&format!("Synced {} issue(s) with Jira", issues.len()));
        }
        self.drain_outbox();
    }

    /// The next queued op, sent, if nothing's in flight.
    fn drain_outbox(&mut self) {
        if self.sync.in_flight.is_some() || self.sync.config().is_none() {
            return;
        }
        let Some(op) = self.store.next_op().cloned() else {
            if std::mem::take(&mut self.sync.pull_after_drain) {
                self.sync_now(true);
            }
            return;
        };
        let Some(key) = self.store.task(op.task).and_then(|t| t.jira_key()).map(str::to_string) else {
            self.store.discard_op(op.id);
            return;
        };
        self.sync.in_flight = Some(op.id);
        sync::send_op(op, key, self.sync.flag_field.clone());
    }

    fn op_done(&mut self, op_id: u64, key: String, what: String, result: Result<(), String>) {
        self.sync.in_flight = None;
        let comment = self.store.outbox.iter().any(|op| op.id == op_id && matches!(op.kind, OpKind::AddComment(_)));
        match result {
            Ok(()) => {
                self.store.op_succeeded(op_id);
                self.sync.pull_after_drain |= comment;
                api::notify(&format!("{key}: {what}"));
                self.save_and_refresh();
                self.drain_outbox();
            }
            Err(err) => {
                self.store.op_failed(op_id, err.clone());
                host::notify(api::Level::Error, &format!("{key}: Jira refused a change ({what}): {err} -- r on the task retries"));
                self.save_and_refresh();
            }
        }
    }

    fn retry(&mut self, id: TaskId) {
        if self.store.task(id).and_then(|t| t.jira.as_ref()).is_none() {
            return;
        }
        self.store.retry(id);
        self.after_edit();
        self.sync_now(true);
    }

    /// Moves a task to `target`: a local one just moves; a linked one
    /// moves at once too, then the matching Jira change is worked out --
    /// nothing, a Flagged toggle, or a workflow transition. If no
    /// transition gets there, the card goes back.
    fn set_status(&mut self, id: TaskId, target: Status) {
        let Some(task) = self.store.task(id) else { return };
        let Some(link) = task.jira.clone().filter(|_| self.sync.config().is_some()) else {
            self.store.move_to_status(id, target);
            return self.save_and_refresh();
        };
        let prev = (task.status, task.order);
        if prev.0 == target {
            return;
        }
        let project = link.project().to_string();
        let blocked = self.config().blocked_for(&project);
        self.store.move_to_status(id, target);
        if prev.0 == Status::Blocked && blocked == Some(Blocked::Flag) && link.base.flagged {
            self.store.enqueue(id, OpKind::SetFlag(false));
        }
        let on_blocked_status = matches!(&blocked, Some(Blocked::Status { id: sid, .. }) if *sid == link.base.status_id);
        let need_transition = match (target, &blocked) {
            (Status::Blocked, Some(Blocked::Local)) => false,
            (Status::Blocked, Some(Blocked::Flag)) => {
                self.store.enqueue(id, OpKind::SetFlag(true));
                false
            }
            (Status::Blocked, Some(Blocked::Status { .. })) => !on_blocked_status,
            (Status::Blocked, None) => true,
            (_, _) => {
                let category = model::jira::category_for_status(target).unwrap_or("new");
                link.base.status_category != category || on_blocked_status
            }
        };
        if !need_transition {
            return self.after_edit();
        }
        self.store.set_resolving(id, true);
        self.save_and_refresh();
        sync::transitions(id, link.key.clone(), Some(target), prev);
    }

    /// `s` on a linked task: its real transitions, plus a Blocked row.
    fn linked_status_picker(&mut self, id: TaskId) {
        let Some(task) = self.store.task(id) else { return };
        let Some(key) = task.jira_key().map(str::to_string) else { return };
        sync::transitions(id, key, None, (task.status, task.order));
    }

    fn transitions_came(&mut self, id: TaskId, target: Option<Status>, prev: (Status, i64), result: Result<Vec<fenix_jira_client::Transition>, String>) {
        self.store.set_resolving(id, false);
        let Some(link) = self.store.task(id).and_then(|t| t.jira.clone()) else { return };
        let key = link.key.clone();
        let project = link.project().to_string();
        let current = link.base.status_name.clone();
        let revert = |a: &mut Agenda| {
            if target.is_some() {
                a.store.restore_status(id, prev.0, prev.1);
            }
            a.save_and_refresh();
        };
        let transitions = match result {
            Ok(transitions) => transitions,
            Err(err) => {
                revert(self);
                return host::notify(api::Level::Error, &format!("{key}: couldn't read Jira's workflow: {err}"));
            }
        };
        let blocked = self.config().blocked_for(&project);
        match target {
            None => self.transition_picker(id, &key, transitions, None),
            Some(Status::Blocked) => match blocked {
                Some(Blocked::Status { id: status_id, name }) => match transitions.iter().find(|t| t.to_id == status_id) {
                    Some(t) => {
                        self.enqueue_transition(id, t.clone());
                        self.after_edit();
                    }
                    None => {
                        revert(self);
                        host::notify(api::Level::Error, &format!("{key}: \"{name}\" isn't reachable from \"{current}\" -- pick a step from here"));
                        self.transition_picker(id, &key, transitions, None);
                    }
                },
                None => {
                    let mut rows: Vec<(String, Pick)> = transitions
                        .into_iter()
                        .filter(|t| !t.to_id.is_empty())
                        .map(|t| (format!("{}   (via \"{}\")", t.to_name, t.name), Pick::Blocked(Blocked::Status { id: t.to_id.clone(), name: t.to_name.clone() }, Some(t))))
                        .collect();
                    rows.push(("Flag as impediment (keep the status)".into(), Pick::Blocked(Blocked::Flag, None)));
                    rows.push((format!("Keep Blocked local for {project}"), Pick::Blocked(Blocked::Local, None)));
                    self.sync_picker(&format!("What does Blocked mean in {project}?"), Some(id), Some(project.clone()), Some(prev), rows);
                    api::notify(&format!("Pick what Blocked means in {project} -- asked once, then remembered (b on the project in SPC j j changes it)"));
                }
                Some(_) => self.save_and_refresh(),
            },
            Some(target) => {
                let category = model::jira::category_for_status(target).unwrap_or("new");
                let blocked_id = match &blocked {
                    Some(Blocked::Status { id, .. }) => Some(id.clone()),
                    _ => None,
                };
                let matching: Vec<_> = transitions.iter().filter(|t| t.to_category == category && Some(&t.to_id) != blocked_id.as_ref()).cloned().collect();
                match matching.len() {
                    0 => {
                        revert(self);
                        host::notify(api::Level::Error, &format!("{key}: no transition from \"{current}\" to a {} status", target.label()));
                    }
                    1 => {
                        self.enqueue_transition(id, matching[0].clone());
                        self.after_edit();
                    }
                    _ => self.transition_picker(id, &key, matching, Some(prev)),
                }
            }
        }
    }

    fn transition_picker(&mut self, id: TaskId, key: &str, transitions: Vec<fenix_jira_client::Transition>, revert: Option<(Status, i64)>) {
        let mut rows: Vec<(String, Pick)> = transitions.into_iter().map(|t| (sync::transition_label(&t), Pick::Transition(t))).collect();
        if revert.is_none() {
            let project = fenix_jira_client::project_of(key).to_string();
            let blocked = match self.config().blocked_for(&project) {
                Some(Blocked::Status { name, .. }) => format!("Blocked  ({name})"),
                Some(Blocked::Flag) => "Blocked  (flag as impediment)".to_string(),
                Some(Blocked::Local) => "Blocked  (local only)".to_string(),
                None => "Blocked...".to_string(),
            };
            rows.push((blocked, Pick::StartBlocked));
        }
        self.sync_picker(&format!("{key} status"), Some(id), None, revert, rows);
    }

    /// Queues `t` and puts the card in the column it lands in.
    fn enqueue_transition(&mut self, id: TaskId, t: fenix_jira_client::Transition) {
        let Some(key) = self.store.task(id).and_then(|t| t.jira_key()).map(str::to_string) else { return };
        let project = fenix_jira_client::project_of(&key).to_string();
        let flagged = self.store.task(id).and_then(|t| t.jira.as_ref()).is_some_and(|l| l.base.flagged);
        let status = self.config().status_for(&project, &t.to_id, &t.to_category, flagged);
        if self.store.task(id).is_some_and(|task| task.status != status) {
            self.store.move_to_status(id, status);
        }
        self.store.enqueue(id, OpKind::Transition { id: t.id, to_id: t.to_id, to_name: t.to_name, to_category: t.to_category });
    }

    /// `A` on a linked task: one of the tracked people.
    fn assignee_picker(&mut self, id: TaskId) {
        let Some(key) = self.store.task(id).and_then(|t| t.jira_key()).map(str::to_string) else {
            return api::notify("A reassigns a Jira issue -- link this task first with I");
        };
        let users = self.config().users;
        if users.is_empty() {
            return host::notify(api::Level::Error, "no people to assign to -- a on the Jira page (SPC j j) adds one");
        }
        let rows = users.into_iter().map(|(uid, name)| (format!("{name} ({uid})"), Pick::Assignee(uid, name))).collect();
        self.sync_picker(&format!("Assign {key}"), Some(id), None, None, rows);
    }

    /// `I`: link a task to an issue -- or, linked already, unlink or open it.
    fn start_link(&mut self, id: TaskId) {
        let Some(task) = self.store.task(id) else { return };
        if let Some(key) = task.jira_key().map(str::to_string) {
            let rows = vec![(format!("Open {key} in the browser"), Pick::OpenInBrowser), (format!("Unlink {key} (keep this as a local task)"), Pick::Unlink)];
            return self.sync_picker(&key, Some(id), None, None, rows);
        }
        let jql = fenix_jira_client::build_my_open_issues_jql(&self.config().projects);
        api::notify("Fetching your open Jira issues...");
        sync::search(IssueFetch::LinkPicker(id), jql, 100, self.sync.flag_wanted());
    }

    /// `SPC A i`: any of your open issues, several at once.
    fn import(&mut self) {
        let jql = fenix_jira_client::build_my_open_issues_jql(&self.config().projects);
        api::notify("Fetching your open Jira issues...");
        sync::search(IssueFetch::Import, jql, 200, self.sync.flag_wanted());
    }

    fn link_key(&mut self, task: TaskId, key: String) {
        let flag = self.sync.flag_wanted();
        self.with_jira(|_| sync::issue(IssueFetch::LinkKey(task), key.to_uppercase(), flag));
    }

    fn link_detail(&mut self, id: TaskId, detail: &fenix_jira_client::IssueDetail) {
        if let Some(existing) = self.store.find_by_key(&detail.key).filter(|&t| t != id) {
            let title = self.store.task(existing).map(|t| t.title.clone()).unwrap_or_default();
            return host::notify(api::Level::Error, &format!("{} is already linked to \"{title}\"", detail.key));
        }
        let update = self.remote_update(detail);
        self.store.link(id, detail.key.clone(), update);
        self.start_ticker();
        self.save_and_refresh();
        api::notify(&format!("Linked to {} -- edits now go to Jira too", detail.key));
    }

    fn issues_came(&mut self, purpose: IssueFetch, result: Result<Vec<fenix_jira_client::IssueDetail>, String>) {
        match purpose {
            IssueFetch::LinkPicker(id) => {
                let mut rows = vec![("+ Create a new Jira issue from this task...".to_string(), Pick::CreateIssue), ("Type an issue key...".to_string(), Pick::TypeKey)];
                match result {
                    Ok(issues) => rows.extend(issues.into_iter().filter(|d| self.store.find_by_key(&d.key).is_none()).map(|d| (sync::issue_label(&d), Pick::Issue(Box::new(d))))),
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't list your Jira issues: {err}")),
                }
                self.sync_picker("Link to Jira", Some(id), None, None, rows);
            }
            IssueFetch::Import => {
                let issues = match result {
                    Ok(issues) => issues,
                    Err(err) => return host::notify(api::Level::Error, &format!("couldn't list your Jira issues: {err}")),
                };
                let rows: Vec<_> = issues.into_iter().filter(|d| self.store.find_by_key(&d.key).is_none()).map(|d| (sync::issue_label(&d), Pick::Import(Box::new(d)))).collect();
                if rows.is_empty() {
                    return api::notify("Every open issue assigned to you is already in the agenda");
                }
                let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
                let picker = api::pick_many("Import  (Tab marks)", &items);
                self.asked.insert(picker, Asked::Sync(PickerCtx { task: None, project: None, revert: None, rows: rows.into_iter().map(|(_, p)| p).collect() }));
                api::notify("Tab marks issues to import, Enter adds them");
            }
            IssueFetch::LinkKey(id) | IssueFetch::Created(id) => match result {
                Ok(issues) => match issues.first().cloned() {
                    Some(detail) => self.link_detail(id, &detail),
                    None => host::notify(api::Level::Error, "Jira returned no issue"),
                },
                Err(err) => host::notify(api::Level::Error, &format!("couldn't link: {err}")),
            },
            IssueFetch::AddFromJira { clock } => match result {
                Ok(issues) => {
                    let Some(detail) = issues.first().cloned() else { return };
                    let id = match self.store.find_by_key(&detail.key) {
                        Some(id) => id,
                        None => {
                            let update = self.remote_update(&detail);
                            self.store.create_linked(detail.key.clone(), update, None)
                        }
                    };
                    self.finish_add_from_jira(id, &detail.key, clock);
                }
                Err(err) => host::notify(api::Level::Error, &format!("couldn't fetch the issue: {err}")),
            },
        }
    }

    fn finish_add_from_jira(&mut self, id: TaskId, key: &str, clock: bool) {
        self.start_ticker();
        if clock {
            self.store.clock_in(id);
            api::notify(&format!("{key} is in your agenda, clock running"));
        } else {
            api::notify(&format!("{key} is in your agenda"));
        }
        self.save_and_refresh();
    }

    /// `agenda.add_issue`, from the Jira page's `a` and `t`.
    fn add_issue(&mut self, key: String, clock: bool) {
        match self.store.find_by_key(&key) {
            Some(id) => self.finish_add_from_jira(id, &key, clock),
            None => {
                let flag = self.sync.flag_wanted();
                sync::issue(IssueFetch::AddFromJira { clock }, key, flag);
            }
        }
    }

    /// `b` on a project's search on the Jira page, or a first block:
    /// what Blocked means there, from every status it uses.
    fn project_statuses_came(&mut self, project: String, result: Result<Vec<fenix_jira_client::StatusInfo>, String>) {
        let statuses = match result {
            Ok(statuses) => statuses,
            Err(err) => return host::notify(api::Level::Error, &format!("couldn't list {project}'s statuses: {err}")),
        };
        let current = self.config().blocked_for(&project);
        let mark = |b: &Blocked| if current.as_ref() == Some(b) { "  (current)" } else { "" };
        let mut rows: Vec<(String, Pick)> = statuses
            .into_iter()
            .map(|s| {
                let blocked = Blocked::Status { id: s.id, name: s.name.clone() };
                (format!("{}{}", s.name, mark(&blocked)), Pick::Blocked(blocked, None))
            })
            .collect();
        rows.push((format!("Flag as impediment (keep the status){}", mark(&Blocked::Flag)), Pick::Blocked(Blocked::Flag, None)));
        rows.push((format!("Keep Blocked local{}", mark(&Blocked::Local)), Pick::Blocked(Blocked::Local, None)));
        self.sync_picker(&format!("What does Blocked mean in {project}?"), None, Some(project), None, rows);
    }

    fn sync_picker(&mut self, title: &str, task: Option<TaskId>, project: Option<String>, revert: Option<(Status, i64)>, rows: Vec<(String, Pick)>) {
        let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
        let id = api::pick(title, &items);
        self.asked.insert(id, Asked::Sync(PickerCtx { task, project, revert, rows: rows.into_iter().map(|(_, p)| p).collect() }));
    }

    /// `Esc` on a sync picker: a board move it was finishing goes back.
    fn sync_cancelled(&mut self, ctx: PickerCtx) {
        if let (Some(id), Some((status, order))) = (ctx.task, ctx.revert) {
            self.store.restore_status(id, status, order);
            self.save_and_refresh();
            api::notify("Move cancelled");
        }
    }

    fn sync_picked(&mut self, ctx: PickerCtx, picks: Vec<Pick>) {
        let (task, project) = (ctx.task, ctx.project.clone());
        let Some(pick) = picks.first().cloned() else { return };
        match pick {
            Pick::Issue(detail) => {
                if let Some(id) = task {
                    self.link_detail(id, &detail);
                }
            }
            Pick::CreateIssue => {
                if let Some(t) = task.and_then(|id| self.store.task(id)) {
                    sync::new_issue(t.id, t.title.clone(), t.description.clone());
                }
            }
            Pick::TypeKey => {
                if let Some(id) = task {
                    let prompt = api::prompt("Link to issue (e.g. PROJ-123)", "");
                    self.asked.insert(prompt, Asked::LinkKey(id));
                }
            }
            Pick::Unlink => {
                if let Some(id) = task {
                    let key = self.store.task(id).and_then(|t| t.jira_key()).map(str::to_string).unwrap_or_default();
                    self.store.unlink(id);
                    self.save_and_refresh();
                    api::notify(&format!("Unlinked {key} -- the task stays, Jira isn't touched"));
                }
            }
            Pick::OpenInBrowser => {
                if let Some(url) = task.and_then(|id| self.issue_url(id)) {
                    let _ = api::open_url(&url);
                }
            }
            Pick::Import(_) => {
                let count = picks.len();
                self.start_ticker();
                for pick in picks {
                    if let Pick::Import(detail) = pick {
                        if self.store.find_by_key(&detail.key).is_none() {
                            let update = self.remote_update(&detail);
                            self.store.create_linked(detail.key.clone(), update, None);
                        }
                    }
                }
                self.save_and_refresh();
                api::notify(&format!("Added {count} issue(s) to the agenda"));
            }
            Pick::Transition(t) => {
                if let Some(id) = task {
                    self.enqueue_transition(id, t);
                    self.after_edit();
                }
            }
            Pick::StartBlocked => {
                if let Some(id) = task {
                    self.set_status(id, Status::Blocked);
                }
            }
            Pick::Blocked(blocked, transition) => {
                let Some(project) = project else { return };
                sync::save_blocked(&project, &blocked);
                if let Some(Ok(config)) = &mut self.sync.config {
                    match config.blocked.iter_mut().find(|(p, _)| *p == project) {
                        Some(entry) => entry.1 = blocked.text(),
                        None => config.blocked.push((project.clone(), blocked.text())),
                    }
                }
                let meaning = match &blocked {
                    Blocked::Status { name, .. } => format!("moves the issue to \"{name}\""),
                    Blocked::Flag => "flags the issue as an impediment".to_string(),
                    Blocked::Local => "stays in your agenda only".to_string(),
                };
                if let Some(id) = task {
                    match (&blocked, transition) {
                        (Blocked::Status { .. }, Some(t)) => self.enqueue_transition(id, t),
                        (Blocked::Flag, _) => self.store.enqueue(id, OpKind::SetFlag(true)),
                        _ => {}
                    }
                    self.after_edit();
                }
                api::notify(&format!("Blocked in {project} now {meaning}"));
            }
            Pick::Priority(name) => {
                if let Some(id) = task {
                    let level = self.config().priority(&name);
                    self.store.set_priority(id, level);
                    self.store.enqueue(id, OpKind::SetPriority(name));
                    self.after_edit();
                }
            }
            Pick::Assignee(uid, name) => {
                if let Some(id) = task {
                    self.store.enqueue(id, OpKind::SetAssignee { id: uid, name });
                    self.after_edit();
                }
            }
        }
    }

    /// `W` on the review: every row sent, filed on the day it happened.
    fn send_worklogs(&mut self) {
        if self.sync.sending_worklogs {
            return;
        }
        let rows: Vec<model::WorklogRow> = self.worklog_rows().into_iter().filter(|r| r.minutes > 0).collect();
        if rows.is_empty() {
            return api::notify("Nothing to send");
        }
        self.sync.sending_worklogs = true;
        api::notify("Sending worklogs...");
        sync::worklogs(rows);
    }

    fn worklogs_sent(&mut self, results: Vec<sync::WorklogSent>) {
        self.sync.sending_worklogs = false;
        let (mut sent, mut minutes, mut failures) = (0, 0, Vec::new());
        for sync::WorklogSent { task, entries, key, minutes: row_minutes, result } in results {
            match result {
                Ok(()) => {
                    self.store.mark_sent(task, &entries);
                    self.worklog_edits.retain(|(t, _), _| *t != task);
                    sent += 1;
                    minutes += row_minutes;
                }
                Err(err) => failures.push(format!("{key}: {err}")),
            }
        }
        self.save_and_refresh();
        if failures.is_empty() {
            api::notify(&format!("Sent {sent} worklog(s), {}", page::format_minutes(minutes)));
        } else {
            host::notify(api::Level::Error, &format!("Sent {sent}, {} failed -- {}", failures.len(), failures.join("; ")));
        }
    }

    /// The sync's jobs that have ended, taken in.
    fn take_done(&mut self) {
        for done in sync::take_done() {
            match done {
                Done::Config(config) => {
                    let first = self.sync.config.is_none();
                    self.sync.config = Some(config);
                    // Linked tasks current while Fenix runs, and what was
                    // queued before the last exit sent.
                    if first && self.store.tasks.iter().any(|t| t.jira.is_some()) && self.sync.config().is_some() {
                        self.start_ticker();
                        self.drain_outbox();
                    }
                    self.refresh();
                }
                Done::Pulled { keys, quiet, me, flag_field, result } => {
                    if me.is_some() {
                        self.sync.me = me;
                    }
                    if flag_field.is_some() {
                        self.sync.flag_field = flag_field;
                    }
                    self.pulled(keys, quiet, result);
                }
                Done::OpDone { op_id, key, what, flag_field, result } => {
                    if flag_field.is_some() {
                        self.sync.flag_field = flag_field;
                    }
                    self.op_done(op_id, key, what, result);
                }
                Done::Issues { purpose, result } => self.issues_came(purpose, result),
                Done::Transitions { task, target, prev, result } => self.transitions_came(task, target, prev, result),
                Done::Priorities { task, result } => match result {
                    Ok(priorities) => {
                        let key = self.store.task(task).and_then(|t| t.jira_key()).unwrap_or_default().to_string();
                        let rows = priorities.into_iter().map(|p| (p.name.clone(), Pick::Priority(p.name))).collect();
                        self.sync_picker(&format!("{key} priority"), Some(task), None, None, rows);
                    }
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't read Jira's priorities: {err}")),
                },
                Done::ProjectStatuses { project, result } => self.project_statuses_came(project, result),
                Done::WorklogsSent(results) => self.worklogs_sent(results),
                Done::Created { task, result } => match result {
                    Ok(key) if !key.is_empty() => self.link_key(task, key),
                    Ok(_) => {}
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't make the issue: {err}")),
                },
                Done::Saved(Err(err)) => host::notify(api::Level::Error, &format!("couldn't save what Blocked means: {err}")),
                Done::Saved(Ok(())) => {}
            }
        }
    }

    /// The periodic sync: a dropped connection retried once it's back.
    fn sync_tick(&mut self) {
        let transient: Vec<TaskId> = self.store.tasks.iter().filter(|t| t.jira.as_ref().and_then(|l| l.error.as_deref()).is_some_and(|e| e.starts_with("request failed"))).map(|t| t.id).collect();
        for id in transient {
            self.store.retry(id);
        }
        self.sync_now(true);
        self.drain_outbox();
    }
}

impl Extension for Agenda {
    fn activate(&mut self) -> Result<(), String> {
        self.store = model::load(std::path::Path::new(KEPT));
        self.stamp = std::fs::metadata(KEPT).and_then(|m| m.modified()).ok();
        // The minute (the clock in the modeline, the page) and another
        // writer's `agenda.json`.
        self.tick = Some(host::set_timer(5_000, true));
        self.minute = clock::now().timestamp() / 60;
        sync::fetch_config();
        self.status();
        self.home();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "agenda.open" => self.open_page(None),
            "agenda.board" => self.open_page(Some(Tab::Board)),
            "agenda.list" => self.open_page(Some(Tab::List)),
            "agenda.report" => self.open_page(Some(Tab::Time)),
            "agenda.new_task" => {
                self.capture = None;
                self.open_page(None);
                self.page_key_now(Key::Char('n'));
            }
            "agenda.from_here" => self.task_from_here(),
            "agenda.find" => {
                self.open_page(None);
                self.page_key_now(Key::Char('/'));
            }
            "agenda.toggle_clock" => self.toggle_clock(),
            "agenda.resume" => self.resume(),
            "agenda.import" => self.with_jira(|a| a.import()),
            "agenda.sync" => self.with_jira(|a| a.sync_now(false)),
            "agenda.worklogs" => {
                self.open_page(Some(Tab::Time));
                if let Some((_, page)) = &mut self.page {
                    page.open_worklogs();
                }
                self.refresh();
            }
            other => return Err(format!("no command {other}")),
        }
        self.take_done();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let event = run::deliver(event);
        self.take_done();
        let Some(event) = event else { return };
        match event {
            Event::Timer(t) if Some(t) == self.tick => {
                self.reload_if_changed();
                let minute = clock::now().timestamp() / 60;
                if minute != self.minute && self.store.active_timer.is_some() {
                    self.minute = minute;
                    self.refresh();
                }
            }
            Event::Timer(t) if Some(t) == self.sync.ticker => self.sync_tick(),
            Event::Active => self.note_activity(),
            Event::SettingChanged(_) => self.refresh(),
            Event::PageShown((page, false)) if Some(page) == self.page_id() => {}
            Event::HomePicked(value) => {
                self.open_page(Some(Tab::Today));
                if let (Ok(n), Some((_, page))) = (value.parse(), &mut self.page) {
                    page.select(TaskId(n));
                }
            }
            Event::Picked((id, choice)) => match self.asked.remove(&id) {
                Some(Asked::Clock(rows)) => {
                    if let Some(pick) = choice.and_then(|c| c.parse::<usize>().ok()).and_then(|i| rows.get(i).cloned()) {
                        self.clock_pick(pick);
                    } else {
                        self.gap_asked = false;
                    }
                }
                Some(Asked::Sync(ctx)) => match choice.and_then(|c| c.parse::<usize>().ok()).and_then(|i| ctx.rows.get(i).cloned()) {
                    Some(pick) => self.sync_picked(ctx, vec![pick]),
                    None => self.sync_cancelled(ctx),
                },
                _ => {}
            },
            Event::PickedMany((id, marked)) => {
                if let Some(Asked::Sync(ctx)) = self.asked.remove(&id) {
                    let picks: Vec<Pick> = marked.unwrap_or_default().iter().filter_map(|c| c.parse::<usize>().ok()).filter_map(|i| ctx.rows.get(i).cloned()).collect();
                    if picks.is_empty() {
                        self.sync_cancelled(ctx);
                    } else {
                        self.sync_picked(ctx, picks);
                    }
                }
            }
            Event::Prompted((id, text)) => {
                if let (Some(Asked::LinkKey(task)), Some(text)) = (self.asked.remove(&id), text) {
                    let key = text.trim().to_string();
                    if !key.is_empty() {
                        self.link_key(task, key);
                    }
                }
            }
            Event::Composed((id, text)) => match (self.asked.remove(&id), text) {
                (Some(Asked::Description(task)), Some(body)) => {
                    if self.store.task(task).is_some_and(|t| t.description != body) {
                        self.store.set_description(task, body.clone());
                        self.store.enqueue(task, OpKind::SetDescription(body));
                        self.after_edit();
                    }
                }
                (Some(Asked::Comment(task)), Some(body)) => {
                    self.store.enqueue(task, OpKind::AddComment(body));
                    self.after_edit();
                }
                _ => {}
            },
            _ => {}
        }
        self.take_done();
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        let Some((id, p)) = &self.page else { return Page::new() };
        if *id != page {
            return Page::new();
        }
        let worklogs = self.worklog_rows();
        let categories = self.categories();
        let ctx = page::Ctx { store: &self.store, now: clock::now(), categories: &categories, worklogs: &worklogs, round: self.round(), sync: self.sync_label() };
        Page::canvas(page::layout(p, &ctx, width as usize)).typing(p.typing()).space(p.claims_space())
    }

    fn page_key(&mut self, page: u64, key: Key) {
        if self.page_id() == Some(page) {
            self.page_key_now(key);
            self.take_done();
        }
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some((id, p)) = &mut self.page {
            if *id == page {
                p.paste(text);
            }
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        let (id, p) = self.page.as_ref()?;
        (*id == page).then(|| p.tab.label().to_string())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        let tab = Tab::ALL.into_iter().find(|t| t.label() == state).unwrap_or(Tab::Today);
        self.page = Some((page, AgendaPage::new(tab, self.project.clone())));
        true
    }

    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = match name {
            "agenda.tasks" => self.control_tasks(payload),
            "agenda.clock_in" => {
                #[derive(serde::Deserialize)]
                struct ClockIn {
                    task_id: u32,
                }
                match serde_json::from_str::<ClockIn>(payload) {
                    Ok(call) if self.store.task(TaskId(call.task_id)).is_some_and(|t| !t.archived) => {
                        self.clock_pick(ClockPick::Start(TaskId(call.task_id)));
                        Ok(String::new())
                    }
                    Ok(call) => Err(format!("no agenda task {}", call.task_id)),
                    Err(e) => Err(format!("an agenda call: {e}")),
                }
            }
            "agenda.clock_out" => {
                if self.store.active_timer.is_some() {
                    self.clock_pick(ClockPick::Stop);
                }
                Ok(String::new())
            }
            "agenda.timer" => Ok(self.control_timer()),
            // The Jira page's `a` and `t`, and its "in your agenda".
            "agenda.add_issue" => {
                #[derive(serde::Deserialize)]
                struct Add {
                    key: String,
                    #[serde(default)]
                    clock: bool,
                }
                match serde_json::from_str::<Add>(payload) {
                    Ok(add) => {
                        self.add_issue(add.key, add.clock);
                        Ok(String::new())
                    }
                    Err(e) => Err(format!("an agenda call: {e}")),
                }
            }
            // The Jira page's "in your agenda".
            "agenda.linked" => Ok(serde_json::to_string(&self.store.tasks.iter().filter_map(|t| t.jira_key().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default()),
            // Enter on an issue that's in the agenda: its task page.
            "agenda.open_task" => {
                let key = serde_json::from_str::<serde_json::Value>(payload).ok().and_then(|v| v.get("key").and_then(|k| k.as_str()).map(str::to_string)).unwrap_or_default();
                match self.store.find_by_key(&key) {
                    Some(task) => {
                        self.open_page(None);
                        if let Some((_, page)) = &mut self.page {
                            page.open_task(task);
                        }
                        self.refresh();
                        Ok(String::new())
                    }
                    None => Err(format!("{key} isn't in the agenda")),
                }
            }
            // A journal day's list: open tasks with a due date.
            "agenda.due" => {
                let due: Vec<serde_json::Value> = self
                    .store
                    .tasks
                    .iter()
                    .filter(|t| !t.archived && t.status != Status::Done)
                    .filter_map(|t| t.due.map(|d| serde_json::json!({ "due": d.format("%Y-%m-%d").to_string(), "text": keyed(t) })))
                    .collect();
                Ok(serde_json::Value::Array(due).to_string())
            }
            // The notebook's `SPC n t`: a checkbox as a task, keeping its line.
            "agenda.add_task" => {
                #[derive(serde::Deserialize)]
                struct Add {
                    title: String,
                    path: Option<String>,
                    #[serde(default)]
                    line: usize,
                }
                match serde_json::from_str::<Add>(payload) {
                    Ok(add) => {
                        let id = self.store.create_task(add.title, String::new(), Priority::Medium, None);
                        if let Some(path) = add.path {
                            self.store.set_code(id, Some(CodeRef { path: path.into(), line: add.line }));
                        }
                        self.save_and_refresh();
                        Ok(String::new())
                    }
                    Err(e) => Err(format!("an agenda call: {e}")),
                }
            }
            // An issue in the agenda changed on the Jira page.
            "agenda.sync_quietly" => {
                self.sync_now(true);
                Ok(String::new())
            }
            other => Err(format!("no service {other}")),
        };
        self.take_done();
        Some(answer)
    }
}

api::export!(Agenda);
