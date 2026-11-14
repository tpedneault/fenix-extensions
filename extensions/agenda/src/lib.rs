//! The agenda, as a WebAssembly extension: tasks on a board, a clock for
//! your time, a week's timesheet, and -- with a tracker extension (Jira,
//! and those after it) -- each task's link to an issue. What was Fenix
//! 1.x's `SPC A`, keeping its `agenda.json`, its keys and its commands.
//!
//! `model` is the data, `page` the page (laid out cell by cell, as it was
//! in Fenix), `sync` the trackers' side, through the tracker seam
//! (`plans/tracker-seam.md`): `trackers` finds them, `calls` waits for
//! their answers, `legacy` is what's left of the time the agenda knew
//! only Jira. Here: opening the page, carrying out what its keys ask, the
//! clock and its pickers, Home, the modeline, and the control channel's
//! methods.

mod calls;
pub mod clock;
mod keys;
pub mod legacy;
pub mod model;
pub mod page;
pub mod sync;
pub mod trackers;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod linear_tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[cfg(not(any(test, feature = "testing")))]
use fenix_extension_api as api;
#[cfg(any(test, feature = "testing"))]
use fenix_test_kit::api;

use api::{host, Event, Extension, HomeMark, HomeRow, HomeSection, Key, KeyScheme, Notification, Page, PickerItem, Urgency};
use chrono::{DateTime, NaiveDate};
use fenix_tracker::{self as seam, can, code, Issue, Move, Moves, SeamError};

use clock::Local;
use model::{AgendaStore, CodeRef, OpKind, Priority, Status, Task, TaskId};
use page::{Action as A, AgendaPage, NewTask, Project, Tab};
use sync::{Done, IssueFetch, Pick, PickerCtx};
use trackers::{Tracker, Trackers, Via};

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
    /// Where `agenda.json` is, when not [`KEPT`]: a test's own folder.
    kept: Option<std::path::PathBuf>,
}

impl Agenda {
    /// Where `agenda.json` is.
    fn kept(&self) -> std::path::PathBuf {
        self.kept.clone().unwrap_or_else(|| KEPT.into())
    }

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

    /// `agenda.json` read, as this agenda keeps it.
    fn load(path: &std::path::Path) -> AgendaStore {
        let mut store = model::load(path);
        legacy::migrate(&mut store);
        store
    }

    fn save(&mut self) {
        if let Err(err) = model::save(&self.kept(), &self.store) {
            host::notify(api::Level::Error, &format!("couldn't save agenda.json: {err}"));
        }
        self.stamp = std::fs::metadata(self.kept()).and_then(|m| m.modified()).ok();
    }

    /// `agenda.json` written by someone else since: taken in, so the next
    /// save doesn't put back what they changed. A file gone or unreadable
    /// is left alone, and the agenda kept.
    fn reload_if_changed(&mut self) {
        let stamp = std::fs::metadata(self.kept()).and_then(|m| m.modified()).ok();
        if stamp.is_none() || stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        match model::reload(&self.kept()) {
            Ok(Some(mut store)) => {
                legacy::migrate(&mut store);
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
            let busy = self.sync.pulling().then(|| format!("syncing {}", self.label()));
            return api::status(busy.as_deref());
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
        api::home(Some(&HomeSection { title: "today".into(), key: keys::keys_for("agenda.open", "SPC A a"), rows, most: 4 }));
    }

    // -- The page ---------------------------------------------------------

    fn worklog_rows(&self) -> Vec<model::WorklogRow> {
        let mut rows = self.store.worklog_batch(self.round(), |tracker| self.sync.trackers.can(tracker, can::WORKLOG));
        rows.retain(|r| !self.worklog_dropped.contains(&(r.task, r.date)));
        for row in &mut rows {
            if let Some(&minutes) = self.worklog_edits.get(&(row.task, row.date)) {
                row.minutes = minutes;
            }
        }
        rows
    }

    /// "synced 3m ago", "syncing", or nothing when no tracker is ready.
    fn sync_label(&self) -> String {
        if !self.sync.any_ready() {
            return String::new();
        }
        if self.sync.pulling() {
            return "syncing".to_string();
        }
        match self.sync.last_pull {
            Some(at) if at.elapsed().as_secs() < 60 => "synced just now".to_string(),
            Some(at) => format!("synced {}m ago", at.elapsed().as_secs() / 60),
            None if self.store.tasks.iter().any(|t| t.link.is_some()) => "not synced yet".to_string(),
            None => String::new(),
        }
    }

    /// The page, on `tab` when given, else where it was left.
    fn open_page(&mut self, tab: Option<Tab>) {
        self.probe_if_stale();
        self.sync_if_stale();
        self.ask_projects();
        let project = here_project(&self.sync.trackers);
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
        let ctx = page::Ctx { store: &self.store, now: clock::now(), categories: &categories, worklogs: &worklogs, round: self.round(), sync: self.sync_label(), import_keys: keys::keys_for("agenda.import", "SPC A i"), trackers: &self.sync.trackers };
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
            A::TrackerStatus(task) => self.linked_status_picker(task),
            A::SetPriority(task, priority) => {
                self.store.set_priority(task, priority);
                self.save_and_refresh();
            }
            A::TrackerPriority(task) => self.priority_picker(task),
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
                self.enqueue(task, OpKind::SetDue(due.map(|d| d.format("%Y-%m-%d").to_string())));
                self.after_edit();
            }
            A::SetTitle(task, title) => {
                self.store.set_title(task, title.clone());
                self.enqueue(task, OpKind::SetSummary(title));
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
                if let Some(why) = self.cant_comment(task) {
                    return api::notify(&why);
                }
                let id = api::compose(&format!("{} comment", self.task_tracker_name(task)), "post", "", false);
                self.asked.insert(id, Asked::Comment(task));
            }
            A::PostNote(task, i) => {
                if let Some(why) = self.cant_comment(task) {
                    return api::notify(&why);
                }
                let Some(t) = self.store.task(task) else { return };
                let (Some(key), Some(note)) = (t.issue_key().map(str::to_string), t.notes.get(i).map(|n| n.text.clone())) else { return };
                self.enqueue(task, OpKind::AddComment(note));
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
            A::Link(task) => self.with_tracker(move |a| a.start_link(task)),
            A::Assign(task) => self.assignee_picker(task),
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
            A::Retry(task) => self.with_tracker(move |a| a.retry(task)),
            A::OpenCode(task) => {
                let Some(code) = self.store.task(task).and_then(|t| t.code.clone()) else { return };
                host::open_file(&code.path.to_string_lossy(), Some(code.line as u32));
            }
            A::Archive(task) => {
                self.store.archive(task);
                self.save_and_refresh();
            }
            A::Delete(task) => {
                let link = self.store.task(task).and_then(|t| t.link.as_ref()).map(|l| (l.key.clone(), self.sync.trackers.name(&l.tracker)));
                self.store.delete(task);
                self.save_and_refresh();
                api::notify(&match link {
                    Some((key, name)) => format!("Deleted -- {key} in {name} isn't affected"),
                    None => "Deleted".to_string(),
                });
            }
            A::Reorder(task, by) => {
                self.store.reorder_within_status(task, by);
                self.save_and_refresh();
            }
            A::Create(spec) => self.create(spec),
            A::Sync => {
                self.probe_if_stale();
                self.with_tracker(|a| a.sync_now(false));
            }
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
            A::SendWorklogs => self.with_tracker(|a| a.send_worklogs()),
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
            return host::notify(api::Level::Error, &format!("no agenda tasks to clock in on yet -- {} to add one", keys::keys_for("agenda.new_task", "SPC A n")));
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
            return host::notify(api::Level::Error, &format!("nothing worked on yet to resume -- {} picks a task", keys::keys_for("agenda.toggle_clock", "SPC A t")));
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
                api::notify(&format!("Kept the time until {} -- {} starts the clock again", at.format("%H:%M"), keys::keys_for("agenda.resume", "SPC A T")));
            }
            ClockPick::KeepAll => {
                if let Some(timer) = &mut self.store.active_timer {
                    timer.last_seen = Some(clock::now());
                }
            }
            ClockPick::Drop => {
                self.store.drop_timer();
                api::notify(&format!("Dropped that sitting -- {} starts the clock again", keys::keys_for("agenda.resume", "SPC A T")));
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
            return host::notify(api::Level::Error, &format!("{} makes a task from a file -- this buffer isn't one", keys::keys_for("agenda.from_here", "SPC A h")));
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
                // Fenix's protocol names an issue's key `jiraKey`, from
                // before there were other trackers; `tracker` says whose.
                if let Some(link) = &t.link {
                    task["jiraKey"] = link.key.clone().into();
                    task["tracker"] = link.tracker.clone().into();
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

/// The project of the file you're in, and its projects in each tracker,
/// from Fenix's project metadata (`.fenix/project.toml`'s Jira key is
/// `jira`), as each tracker names its key (`about.project_meta`).
fn here_project(trackers: &Trackers) -> Option<Project> {
    let project = api::here()?.project?;
    let issues = trackers.list.iter().filter_map(|t| {
        let meta = t.about.project_meta.as_deref()?;
        project.meta.iter().find(|(k, _)| k == meta).map(|(_, v)| (t.id.clone(), v.clone()))
    });
    Some(Project { issues: issues.collect(), name: project.name })
}

/// "PROJ-12 Its title", or its title.
fn keyed(t: &Task) -> String {
    match t.issue_key() {
        Some(key) => format!("{key} {}", t.title),
        None => t.title.clone(),
    }
}

/// The tag a sync's notification goes by: the next sync's replaces it.
const SYNC_TAG: &str = "agenda.sync";

/// What a sync that ended says, as a notification -- it can take a
/// while, and you may be elsewhere by then: how many issues it brought
/// up to date from `names` ("Jira", "Jira and Linear"), or why it failed,
/// or the conflicts it found (which a quiet, timed sync says too).
/// Nothing for a quiet one that went well.
fn sync_note(names: &str, synced: Result<usize, &str>, conflicts: usize, quiet: bool) -> Option<Notification> {
    let note = match synced {
        Err(err) if !quiet => Notification::new(format!("{names} sync failed")).body(err),
        Err(_) => return None,
        Ok(_) if conflicts > 0 => Notification::new(format!("{names} sync: {conflicts} conflicting change(s)")).body("Open the task and press Enter on the conflict.").action("Open the agenda", "agenda.open"),
        Ok(n) if !quiet => Notification::new(format!("Synced {n} issue(s) with {names}")).urgency(Urgency::Low),
        Ok(_) => return None,
    };
    Some(note.tag(SYNC_TAG))
}

/// `0:18`, `1:05`: how long a clock has run, as Home shows it.
fn format_elapsed(d: chrono::Duration) -> String {
    let minutes = d.num_minutes().max(0);
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

// -- Trackers -------------------------------------------------------------

/// Which capability an op needs of its tracker, if any.
fn needs(op: &OpKind) -> Option<&'static str> {
    match op {
        OpKind::SetSummary(_) => Some(can::EDIT_TITLE),
        OpKind::SetDescription(_) => Some(can::EDIT_BODY),
        OpKind::SetPriority(_) => Some(can::EDIT_PRIORITY),
        OpKind::SetDue(_) => Some(can::EDIT_DUE),
        OpKind::SetAssignee { .. } => Some(can::EDIT_ASSIGNEE),
        OpKind::AddComment(_) => Some(can::COMMENT),
        OpKind::Transition { .. } | OpKind::SetFlag(_) => None,
    }
}

impl Agenda {
    /// Asks the trackers in `ids` who they are, all at once.
    fn probe_ids(&mut self, ids: Vec<String>) {
        if self.sync.is_probing() {
            self.sync.probe_again = true;
            return;
        }
        if ids.is_empty() {
            return self.probed_all();
        }
        self.sync.probing = ids.iter().map(|id| (id.clone(), None)).collect();
        for id in ids {
            sync::probe(id);
        }
    }

    /// Asks every tracker it knows of.
    fn probe(&mut self) {
        self.probe_ids(trackers::ids(api::setting("agenda.trackers").as_deref()));
    }

    /// At start, only the trackers your linked tasks and queued changes
    /// are with: asking a tracker starts its extension.
    fn probe_in_use(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        for link in self.store.tasks.iter().filter_map(|t| t.link.as_ref()) {
            if !ids.contains(&link.tracker) {
                ids.push(link.tracker.clone());
            }
        }
        self.probe_ids(ids);
    }

    /// Asks again when it last asked a while ago: a tracker installed
    /// since is found.
    fn probe_if_stale(&mut self) {
        let asked_all = self.sync.probed.is_some_and(|at| at.elapsed() <= sync::PROBE_AFTER);
        if !self.sync.is_probing() && !asked_all {
            self.probe();
        }
    }

    /// One tracker said who it is (or nothing did).
    fn probed_one(&mut self, id: String, found: trackers::Found) {
        if let Some(slot) = self.sync.probing.iter_mut().find(|(i, _)| *i == id) {
            slot.1 = Some(found);
        }
        if self.sync.probing.iter().any(|(_, f)| f.is_none()) {
            return;
        }
        let asked = std::mem::take(&mut self.sync.probing);
        let all = asked.len() == trackers::ids(api::setting("agenda.trackers").as_deref()).len();
        let first = self.sync.trackers.list.is_empty();
        for (id, found) in asked {
            match found.and_then(|f| f.tracker()) {
                Some(t) => {
                    if !t.ready() && !self.sync.warned.contains(&id) && t.about.not_ready.as_deref().is_some_and(|w| w.contains("tracker seam") || w.contains("says it's")) {
                        host::notify(api::Level::Error, t.about.not_ready.as_deref().unwrap_or_default());
                        self.sync.warned.insert(id.clone());
                    }
                    match self.sync.trackers.list.iter_mut().find(|x| x.id == id) {
                        Some(old) => *old = t,
                        None => self.sync.trackers.list.push(t),
                    }
                }
                None => self.sync.trackers.list.retain(|x| x.id != id),
            }
        }
        if all {
            self.sync.probed = Some(Instant::now());
        }
        let order = trackers::ids(api::setting("agenda.trackers").as_deref());
        self.sync.trackers.list.sort_by_key(|t| order.iter().position(|i| *i == t.id).unwrap_or(usize::MAX));
        if first && self.sync.any_ready() {
            self.drain_outbox();
        }
        self.probed_all();
    }

    /// The asking's done: what waited on it runs, and any asking wanted
    /// meanwhile starts.
    fn probed_all(&mut self) {
        for then in std::mem::take(&mut self.sync.waiting) {
            then(self);
        }
        if std::mem::take(&mut self.sync.probe_again) {
            self.probe();
        }
        self.refresh();
    }

    /// Runs `f` when a tracker is ready. When none is, the trackers are
    /// asked now -- one just set up is ready by then -- and `f` runs when
    /// they've answered, or what's wrong is said.
    fn with_tracker(&mut self, f: impl FnOnce(&mut Agenda) + 'static) {
        if self.sync.any_ready() {
            return f(self);
        }
        self.sync.waiting.push(Box::new(move |a: &mut Agenda| {
            if a.sync.any_ready() {
                return f(a);
            }
            match a.sync.trackers.list.first() {
                Some(t) => host::notify(api::Level::Error, &t.about.not_ready.clone().unwrap_or_else(|| format!("{} isn't set up yet", t.name()))),
                None => api::notify(&format!("Nothing syncs the agenda -- that needs a tracker extension, such as Jira ({})", keys::keys_for("extensions.open", "SPC h x"))),
            }
        }));
        if !self.sync.is_probing() {
            self.probe();
        }
    }

    /// A call answered that the tracker isn't set up or signed in: asked
    /// again, so the agenda knows as soon as it is.
    fn not_ready(&mut self, tracker: &str, err: &SeamError) {
        if err.is(code::NOT_SET_UP) || err.is(code::SIGNED_OUT) {
            if let Some(t) = self.sync.trackers.list.iter_mut().find(|t| t.id == tracker) {
                t.about.ready = false;
                t.about.not_ready = Some(err.message.clone());
            }
            self.probe_ids(vec![tracker.to_string()]);
        }
    }

    /// The tracker task `id`'s issue is in, when it's ready -- else says
    /// why not.
    fn task_tracker(&self, id: TaskId) -> Option<(Via, String)> {
        let link = self.store.task(id)?.link.as_ref()?;
        match self.sync.trackers.get(&link.tracker) {
            Some(t) if t.ready() => Some((t.via(), link.key.clone())),
            Some(t) => {
                host::notify(api::Level::Error, &t.about.not_ready.clone().unwrap_or_else(|| format!("{} isn't set up yet", t.name())));
                None
            }
            None => {
                host::notify(api::Level::Error, &format!("{} is {}'s, which isn't installed -- {} installs it", link.key, link.tracker, keys::keys_for("extensions.open", "SPC h x")));
                None
            }
        }
    }

    /// Queues `kind` for a linked task, when its tracker takes it: an
    /// edit its tracker doesn't take stays on the task.
    fn enqueue(&mut self, id: TaskId, kind: OpKind) {
        let Some(link) = self.store.task(id).and_then(|t| t.link.as_ref()) else { return };
        let tracker = self.sync.trackers.get(&link.tracker);
        if let (Some(cap), Some(t)) = (needs(&kind), tracker) {
            if !t.can(cap) {
                return;
            }
        }
        if matches!(kind, OpKind::SetDescription(_)) && link.base.description_cut {
            let name = self.sync.trackers.name(&link.tracker);
            return host::notify(api::Level::Error, &format!("{}'s description is too long to edit here -- yours stays on the task; edit it in {name}", link.key));
        }
        self.store.enqueue(id, kind);
    }

    /// The tracker's name for words about all of them: "Jira", or
    /// "tracker".
    fn label(&self) -> String {
        self.sync.trackers.label()
    }

    /// "your open Jira issues", or "your open issues".
    fn your_open_issues(&self) -> String {
        match self.sync.trackers.one() {
            Some(name) => format!("your open {name} issues"),
            None => "your open issues".to_string(),
        }
    }

    fn issue_url(&self, task: TaskId) -> Option<String> {
        let Some(link) = self.store.task(task).and_then(|t| t.link.as_ref()) else {
            api::notify(&format!("This task isn't linked to {} -- I links it", self.label()));
            return None;
        };
        let url = link.base.url.clone().or_else(|| self.sync.trackers.get(&link.tracker).and_then(|t| t.about.browse_url(&link.key)));
        if url.is_none() {
            api::notify(&format!("{} has no address yet -- a sync brings it", link.key));
        }
        url
    }

    /// The timed syncs: each tracker on its own schedule (`sync_minutes`,
    /// never under 5), none before its `retry_after`; changes that failed
    /// for want of a connection tried again.
    fn sync_due(&mut self, now: Instant) {
        let due: Vec<String> = self.sync.trackers.ready().filter(|t| self.sync.trackers.due(&t.id, now)).map(|t| t.id.clone()).collect();
        if due.is_empty() {
            return;
        }
        let transient: Vec<TaskId> = self.store.tasks.iter().filter(|t| t.link.as_ref().is_some_and(|l| l.error.is_some() && l.transient && due.contains(&l.tracker))).map(|t| t.id).collect();
        for id in transient {
            self.store.retry(id);
        }
        self.probe_if_stale();
        self.sync_these(&due, true, now);
        self.drain_outbox_at(now);
    }

    /// `SPC A s`: every linked task from its tracker, then what's waiting
    /// sent. `quiet` keeps a failure from shouting.
    fn sync_now(&mut self, quiet: bool) {
        let now = Instant::now();
        let ids: Vec<String> = self.sync.trackers.ready().filter(|t| !self.sync.trackers.backing_off(&t.id, now)).map(|t| t.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        if !self.sync_these(&ids, quiet, now) && !quiet {
            api::notify(&format!("No tasks are linked to {} yet -- I on a task links one, {} imports your issues", self.label(), keys::keys_for("agenda.import", "SPC A i")));
        }
    }

    /// A sync of trackers `ids`' linked tasks; false when none has any.
    fn sync_these(&mut self, ids: &[String], quiet: bool, now: Instant) -> bool {
        if self.sync.pulling() {
            return true;
        }
        let mut asks: Vec<(Via, Vec<String>)> = Vec::new();
        for t in self.sync.trackers.ready().filter(|t| ids.contains(&t.id)) {
            let asked: Vec<String> = self.store.tasks.iter().filter_map(|task| task.link.as_ref().filter(|l| l.tracker == t.id).map(|l| l.asked().to_string())).collect();
            if !asked.is_empty() {
                asks.push((t.via(), asked));
            }
        }
        if asks.is_empty() {
            return false;
        }
        self.sync.round = Some(sync::Round { quiet, waiting: asks.len(), ..Default::default() });
        self.sync.last_pull = Some(now);
        self.status();
        for (via, asked) in asks {
            self.sync.trackers.synced.insert(via.id.clone(), now);
            sync::pull(via, asked);
        }
        true
    }

    fn sync_if_stale(&mut self) {
        let fresh = self.sync.last_pull.is_some_and(|at| at.elapsed() < sync::STALE_AFTER);
        if !fresh && self.store.tasks.iter().any(|t| t.link.is_some()) {
            self.sync_now(true);
        }
    }

    /// Tracker `tracker` said to slow down: left alone until then.
    fn back_off(&mut self, tracker: &str, err: &SeamError) {
        if err.is(code::RATE_LIMITED) {
            let wait = std::time::Duration::from_secs(err.retry_after.unwrap_or(60).clamp(1, 24 * 3600));
            self.sync.trackers.backoff.insert(tracker.to_string(), Instant::now() + wait);
        }
    }

    /// One tracker's part of a sync, taken in; the sync said once the
    /// last has answered.
    fn pulled(&mut self, tracker: String, asked: Vec<String>, result: Result<fenix_tracker::Found, SeamError>) {
        let name = self.sync.trackers.name(&tracker);
        let missing = sync::not_found(&name);
        let (mut synced, mut conflicts, mut failure) = (None, 0, None);
        match result {
            Ok(found) => {
                let mut answered: Vec<String> = Vec::new();
                for issue in &found.issues {
                    // What asked for it -- by id, or a ref that may since
                    // have changed -- else its id, else its ref.
                    let task = issue
                        .asked
                        .as_deref()
                        .and_then(|a| self.store.find_by_key(&tracker, a))
                        .or_else(|| self.store.find_issue(&tracker, &issue.id, &issue.key));
                    let Some(id) = task else { continue };
                    if let Some(a) = &issue.asked {
                        answered.push(a.clone());
                    }
                    answered.push(issue.id.clone());
                    answered.push(issue.key.clone());
                    self.store.rekey(id, &issue.id, &issue.key);
                    self.store.apply_remote(id, sync::remote_update(issue));
                    if self.store.task(id).and_then(|t| t.link.as_ref()).is_some_and(|l| l.error.as_deref() == Some(missing.as_str())) {
                        self.store.set_link_error(id, None);
                    }
                    conflicts += self.store.task(id).and_then(|t| t.link.as_ref()).map_or(0, |l| l.conflicts.len());
                }
                for a in &asked {
                    if !answered.contains(a) {
                        if let Some(id) = self.store.find_by_key(&tracker, a) {
                            self.store.set_link_error(id, Some(missing.clone()));
                        }
                    }
                }
                synced = Some(found.issues.len());
                self.save();
            }
            Err(err) if err.is(code::NOT_FOUND) && asked.len() == 1 => {
                if let Some(id) = self.store.find_by_key(&tracker, &asked[0]) {
                    self.store.set_link_error(id, Some(missing));
                }
                synced = Some(0);
                self.save();
            }
            Err(err) => {
                self.back_off(&tracker, &err);
                self.not_ready(&tracker, &err);
                failure = Some(err.message);
            }
        }
        let Some(round) = &mut self.sync.round else { return self.refresh() };
        round.conflicts += conflicts;
        if let Some(n) = synced {
            round.synced.push((tracker.clone(), n));
        }
        if let Some(why) = failure {
            round.failures.push((tracker, why));
        }
        round.waiting = round.waiting.saturating_sub(1);
        if round.waiting > 0 {
            return self.refresh();
        }
        let Some(round) = self.sync.round.take() else { return };
        self.refresh();
        if let Some(note) = self.round_note(&round) {
            api::host::notify_with(&note);
        }
        // Each tracker's changes go, whatever another's sync did.
        self.drain_outbox();
    }

    /// What a finished sync says: every failure, and what did sync.
    fn round_note(&self, round: &sync::Round) -> Option<Notification> {
        let synced_ids: Vec<String> = round.synced.iter().map(|(t, _)| t.clone()).collect();
        let n: usize = round.synced.iter().map(|(_, n)| n).sum();
        if round.failures.is_empty() {
            return sync_note(&self.sync.trackers.names(&synced_ids), Ok(n), round.conflicts, round.quiet);
        }
        let failed: Vec<String> = round.failures.iter().map(|(t, _)| t.clone()).collect();
        let mut body: Vec<String> = match round.failures.as_slice() {
            [(_, why)] => vec![why.clone()],
            all => all.iter().map(|(t, why)| format!("{}: {why}", self.sync.trackers.name(t))).collect(),
        };
        if !round.synced.is_empty() {
            body.push(format!("Synced {n} issue(s) with {}.", self.sync.trackers.names(&synced_ids)));
        }
        sync_note(&self.sync.trackers.names(&failed), Err(&body.join(" ")), round.conflicts, round.quiet)
    }

    /// The next queued op, sent, if nothing's in flight.
    fn drain_outbox(&mut self) {
        self.drain_outbox_at(Instant::now());
    }

    /// `drain_outbox`, a tracker backing off until after `now` skipped.
    fn drain_outbox_at(&mut self, now: Instant) {
        if self.sync.in_flight.is_some() || !self.sync.any_ready() {
            return;
        }
        let ready: Vec<String> = self.sync.trackers.ready().filter(|t| !self.sync.trackers.backing_off(&t.id, now)).map(|t| t.id.clone()).collect();
        let Some(op) = self.store.next_op(|tracker| ready.iter().any(|r| r == tracker)).cloned() else {
            if std::mem::take(&mut self.sync.pull_after_drain) {
                self.sync_now(true);
            }
            return;
        };
        let Some((via, key)) = self.store.task(op.task).and_then(|t| t.link.as_ref()).and_then(|l| Some((self.sync.ready(&l.tracker)?.via(), l.key.clone()))) else {
            self.store.discard_op(op.id);
            return;
        };
        self.sync.in_flight = Some(op.id);
        sync::send_op(via, op, key);
    }

    fn op_done(&mut self, op_id: u64, key: String, tracker: String, what: String, result: Result<Option<String>, SeamError>) {
        self.sync.in_flight = None;
        let op = self.store.outbox.iter().find(|op| op.id == op_id).cloned();
        let comment = op.as_ref().is_some_and(|op| matches!(op.kind, OpKind::AddComment(_)));
        let name = self.sync.trackers.name(&tracker);
        match result {
            Ok(said) => {
                self.store.op_succeeded(op_id);
                self.sync.pull_after_drain |= comment;
                api::notify(&format!("{key}: {}", said.unwrap_or(what)));
                self.save_and_refresh();
                self.drain_outbox();
            }
            // It'll never be taken: dropped, and the task's later
            // changes go on.
            Err(err) if err.never() => {
                if let (true, Some(cap)) = (err.is(code::UNSUPPORTED), op.as_ref().and_then(|op| needs(&op.kind))) {
                    self.sync.trackers.drop_capability(&tracker, cap);
                }
                self.store.discard_op(op_id);
                host::notify(api::Level::Error, &format!("{key}: {name} doesn't take that change ({what}): {err} -- it stays on the task"));
                self.save_and_refresh();
                self.drain_outbox();
            }
            // No answer: it may have been made. Never sent again by itself.
            Err(err) if err.is(code::UNKNOWN) => {
                self.store.op_failed(op_id, format!("{what} may have been made -- no answer from {name} ({err})"), false, true);
                host::notify(api::Level::Error, &format!("{key}: no answer from {name} about a change ({what}) -- it may have been made; r on the task asks whether to send it again"));
                self.save_and_refresh();
                self.drain_outbox();
            }
            Err(err) => {
                self.back_off(&tracker, &err);
                self.not_ready(&tracker, &err);
                self.store.op_failed(op_id, err.message.clone(), err.transient(), false);
                host::notify(api::Level::Error, &format!("{key}: {name} refused a change ({what}): {err} -- r on the task retries"));
                self.save_and_refresh();
                self.drain_outbox();
            }
        }
    }

    /// `r`: a failed change sent again -- or, when it may have been made,
    /// asked first.
    fn retry(&mut self, id: TaskId) {
        let Some(link) = self.store.task(id).and_then(|t| t.link.as_ref()) else { return };
        if link.unknown {
            let what = self.store.held_op(id).map(|op| op.kind.describe()).unwrap_or_default();
            let rows = vec![(format!("Send it again ({what})"), Pick::SendAgain), (format!("It was made -- forget it ({what})"), Pick::Forget)];
            return self.sync_picker(&format!("{}: no answer came -- was it made?", link.key), Some(id), None, None, rows);
        }
        self.store.retry(id);
        self.after_edit();
        self.sync_now(true);
    }

    /// Moves a task to `target`: a local one just moves; a linked one
    /// moves at once too, then the matching move in its tracker is
    /// worked out -- nothing, when the tracker has it in that column
    /// already or has no Blocked of its own, or one of the moves it
    /// offers. If none gets there, the card goes back.
    fn set_status(&mut self, id: TaskId, target: Status) {
        let Some(task) = self.store.task(id) else { return };
        let Some((link, t)) = task.link.clone().and_then(|l| Some((l.clone(), self.sync.ready(&l.tracker)?.clone()))) else {
            self.store.move_to_status(id, target);
            return self.save_and_refresh();
        };
        let prev = (task.status, task.order);
        if prev.0 == target {
            return;
        }
        self.store.move_to_status(id, target);
        let column = model::link::column_for_status(target);
        let blocked_here = !t.about.blocked && (target == Status::Blocked || (prev.0 == Status::Blocked && link.column() == Some(column)));
        if link.column() == Some(column) || blocked_here {
            return self.after_edit();
        }
        self.store.set_resolving(id, true);
        self.save_and_refresh();
        sync::transitions(t.via(), id, link.key.clone(), sync::seen(&link.base), Some(target), prev);
    }

    /// `s` on a linked task: its tracker's moves, plus a Blocked row.
    fn linked_status_picker(&mut self, id: TaskId) {
        let Some((via, key)) = self.task_tracker(id) else { return };
        let Some(task) = self.store.task(id) else { return };
        let (Some(link), prev) = (task.link.as_ref(), (task.status, task.order)) else { return };
        sync::transitions(via, id, key, sync::seen(&link.base), None, prev);
    }

    fn transitions_came(&mut self, id: TaskId, target: Option<Status>, prev: (Status, i64), result: Result<Moves, SeamError>) {
        self.store.set_resolving(id, false);
        let Some(link) = self.store.task(id).and_then(|t| t.link.clone()) else { return };
        let key = link.key.clone();
        let current = link.base.status_name.clone();
        let revert = |a: &mut Agenda| {
            if target.is_some() {
                a.store.restore_status(id, prev.0, prev.1);
            }
            a.save_and_refresh();
        };
        let moves = match result {
            Ok(moves) => moves,
            Err(err) => {
                revert(self);
                self.not_ready(&link.tracker, &err);
                return host::notify(api::Level::Error, &format!("{key}: couldn't read {}'s workflow: {err}", self.sync.trackers.name(&link.tracker)));
            }
        };
        let Some(target) = target else {
            return self.move_picker(id, &link.tracker, &key, moves.transitions.clone(), moves.blocked.clone(), None, None);
        };
        if let Some(ask) = &moves.ask {
            if let Some(note) = &ask.note {
                api::notify(note);
            }
            return self.move_picker(id, &link.tracker, &key, moves.transitions.clone(), None, Some(prev), Some(ask.title.clone()));
        }
        let column = model::link::column_for_status(target);
        let matching: Vec<Move> = moves.transitions.iter().filter(|m| m.to.column == column).cloned().collect();
        match matching.len() {
            0 => {
                revert(self);
                let why = moves.why.clone().unwrap_or_else(|| format!("no transition from \"{current}\" to a {} status", target.label()));
                host::notify(api::Level::Error, &format!("{key}: {why}"));
                // Blocked is spelt differently everywhere: a step from here.
                if target == Status::Blocked {
                    self.move_picker(id, &link.tracker, &key, moves.transitions.clone(), moves.blocked.clone(), None, None);
                }
            }
            1 => {
                self.take_move(id, &link.tracker, matching.into_iter().next().unwrap_or_default());
                self.after_edit();
            }
            _ => self.move_picker(id, &link.tracker, &key, matching, None, Some(prev), None),
        }
    }

    /// A picker of `moves`; a status picker (no `revert`) has the
    /// agenda's own Blocked row too.
    #[allow(clippy::too_many_arguments)]
    fn move_picker(&mut self, id: TaskId, tracker: &str, key: &str, moves: Vec<Move>, blocked: Option<String>, revert: Option<(Status, i64)>, title: Option<String>) {
        let asked = title.is_some();
        let mut rows: Vec<(String, Pick)> = moves.into_iter().map(|m| (m.row(), Pick::Move(Box::new(m)))).collect();
        if revert.is_none() && !asked {
            rows.push((blocked.unwrap_or_else(|| "Blocked...".to_string()), Pick::StartBlocked));
        }
        let title = title.unwrap_or_else(|| format!("{key} status"));
        self.sync_picker(&title, Some(id), Some(tracker.to_string()), revert, rows);
    }

    /// A fresh `op` for a write that isn't queued.
    fn op_id(&self) -> String {
        format!("agenda:{}:x{}", clock::now().timestamp_millis(), self.store.outbox.len())
    }

    /// A move picked or worked out: the card into the column it lands
    /// in, and the move queued -- unless it's local, which sends nothing.
    fn take_move(&mut self, id: TaskId, tracker: &str, m: Move) {
        let status = model::link::status_for_column(&m.to.column);
        if self.store.task(id).is_some_and(|task| task.status != status) {
            self.store.move_to_status(id, status);
        }
        if let (Some(remember), Some(via)) = (&m.remember, self.sync.ready(tracker).map(Tracker::via)) {
            let key = self.store.task(id).and_then(|t| t.issue_key()).unwrap_or_default().to_string();
            sync::remember(via, key, remember.clone(), self.op_id());
        }
        if m.local {
            return;
        }
        self.enqueue(id, OpKind::Transition { id: m.id, to_id: m.to.id, to_name: m.to.name, to_category: m.to.stage, to_column: Some(m.to.column), said: m.said });
    }

    /// `p` on a linked task: its tracker's priorities.
    fn priority_picker(&mut self, id: TaskId) {
        let Some((via, key)) = self.task_tracker(id) else { return };
        sync::priorities(via, id, key);
    }

    /// `A` on a linked task: the people its tracker offers, and nobody.
    fn assignee_picker(&mut self, id: TaskId) {
        let Some(link) = self.store.task(id).and_then(|t| t.link.as_ref()) else {
            return api::notify(&format!("A reassigns a {} issue -- link this task first with I", self.label()));
        };
        let name = self.sync.trackers.name(&link.tracker);
        if !self.sync.trackers.can(&link.tracker, can::EDIT_ASSIGNEE) && self.sync.ready(&link.tracker).is_some() {
            return api::notify(&format!("{name} doesn't take an assignee from the agenda -- assign it on its own page"));
        }
        let Some((via, key)) = self.task_tracker(id) else { return };
        sync::people(via, id, key);
    }

    /// `I`: link a task to an issue -- or, linked already, unlink or open it.
    fn start_link(&mut self, id: TaskId) {
        let Some(task) = self.store.task(id) else { return };
        if let Some(key) = task.issue_key().map(str::to_string) {
            let rows = vec![(format!("Open {key} in the browser"), Pick::OpenInBrowser), (format!("Unlink {key} (keep this as a local task)"), Pick::Unlink)];
            return self.sync_picker(&key, Some(id), None, None, rows);
        }
        api::notify(&format!("Fetching {}...", self.your_open_issues()));
        sync::assigned(IssueFetch::LinkPicker(id), self.sync.trackers.ready().map(Tracker::via).collect(), 100);
    }

    /// `SPC A i`: any of your open issues, several at once.
    fn import(&mut self) {
        api::notify(&format!("Fetching {}...", self.your_open_issues()));
        sync::assigned(IssueFetch::Import, self.sync.trackers.ready().map(Tracker::via).collect(), 200);
    }

    /// Links `task` to issue `key`, from whichever tracker has it.
    fn link_key(&mut self, task: TaskId, key: String) {
        self.with_tracker(move |a| sync::issue(IssueFetch::LinkKey(task), a.sync.trackers.ready().map(Tracker::via).collect(), key.trim().to_string()));
    }

    fn link_issue(&mut self, id: TaskId, tracker: &str, issue: &Issue) {
        if let Some(existing) = self.store.find_issue(tracker, &issue.id, &issue.key).filter(|&t| t != id) {
            let title = self.store.task(existing).map(|t| t.title.clone()).unwrap_or_default();
            return host::notify(api::Level::Error, &format!("{} is already linked to \"{title}\"", issue.key));
        }
        self.store.link_with_id(id, tracker, Some(issue.id.clone()), issue.key.clone(), sync::remote_update(issue));
        self.save_and_refresh();
        api::notify(&format!("Linked to {} -- edits now go to {} too", issue.key, self.sync.trackers.name(tracker)));
    }

    /// Several trackers' issues listed together end with their names.
    fn tracker_tag(&self, tracker: &str) -> Option<String> {
        (self.sync.trackers.ready().count() > 1).then(|| self.sync.trackers.name(tracker))
    }

    fn issues_came(&mut self, purpose: IssueFetch, found: Vec<(String, Issue)>, failures: Vec<(String, SeamError)>) {
        for (tracker, err) in &failures {
            self.not_ready(tracker, err);
        }
        let say_failures = |a: &Agenda| {
            for (tracker, err) in &failures {
                host::notify(api::Level::Error, &format!("couldn't list your {} issues: {err}", a.sync.trackers.name(tracker)));
            }
        };
        let unlinked = |a: &Agenda, (t, i): &(String, Issue)| a.store.find_issue(t, &i.id, &i.key).is_none();
        match purpose {
            IssueFetch::LinkPicker(id) => {
                let mut rows: Vec<(String, Pick)> = self
                    .sync
                    .trackers
                    .ready()
                    .filter(|t| t.can(can::CREATE))
                    .map(|t| (format!("+ Create a new {} issue from this task...", t.name()), Pick::CreateIssue(t.id.clone())))
                    .collect();
                rows.push(("Type an issue key...".to_string(), Pick::TypeKey));
                say_failures(self);
                rows.extend(found.into_iter().filter(|f| unlinked(self, f)).map(|(t, i)| (sync::issue_label(&i, self.tracker_tag(&t).as_deref()), Pick::Issue(t, Box::new(i)))));
                let title = match self.sync.trackers.one() {
                    Some(name) => format!("Link to {name}"),
                    None => "Link to an issue".to_string(),
                };
                self.sync_picker(&title, Some(id), None, None, rows);
            }
            IssueFetch::Import => {
                say_failures(self);
                if found.is_empty() && !failures.is_empty() {
                    return;
                }
                let rows: Vec<(String, Pick)> = found.into_iter().filter(|f| unlinked(self, f)).map(|(t, i)| (sync::issue_label(&i, self.tracker_tag(&t).as_deref()), Pick::Import(t, Box::new(i)))).collect();
                if rows.is_empty() {
                    return api::notify("Every open issue assigned to you is already in the agenda");
                }
                let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
                let picker = api::pick_many("Import  (Tab marks)", &items);
                self.asked.insert(picker, Asked::Sync(PickerCtx { task: None, tracker: None, revert: None, rows: rows.into_iter().map(|(_, p)| p).collect() }));
                api::notify("Tab marks issues to import, Enter adds them");
            }
            IssueFetch::LinkKey(id) | IssueFetch::Created(id) => match found.into_iter().next() {
                Some((tracker, issue)) => self.link_issue(id, &tracker, &issue),
                None if failures.is_empty() => host::notify(api::Level::Error, &format!("{} returned no issue", self.label())),
                None => host::notify(api::Level::Error, &format!("couldn't link: {}", failures.iter().map(|(_, e)| e.message.clone()).collect::<Vec<_>>().join("; "))),
            },
            IssueFetch::AddIssue { clock } => match found.into_iter().next() {
                Some((tracker, issue)) => {
                    let id = match self.store.find_issue(&tracker, &issue.id, &issue.key) {
                        Some(id) => id,
                        None => self.store.create_linked(&tracker, Some(issue.id.clone()), issue.key.clone(), sync::remote_update(&issue), None),
                    };
                    self.finish_add_issue(id, &issue.key, clock);
                }
                None => {
                    let why = failures.iter().map(|(_, e)| e.message.clone()).collect::<Vec<_>>().join("; ");
                    host::notify(api::Level::Error, &format!("couldn't fetch the issue: {why}"));
                }
            },
        }
    }

    fn finish_add_issue(&mut self, id: TaskId, key: &str, clock: bool) {
        if clock {
            self.store.clock_in(id);
            api::notify(&format!("{key} is in your agenda, clock running"));
        } else {
            api::notify(&format!("{key} is in your agenda"));
        }
        self.save_and_refresh();
    }

    /// `agenda.add_issue`, from a tracker's page (Jira's `a` and `t`).
    fn add_issue(&mut self, tracker: String, key: String, clock: bool) {
        match self.store.find_by_key(&tracker, &key) {
            Some(id) => self.finish_add_issue(id, &key, clock),
            None => self.with_tracker(move |a| match a.sync.ready(&tracker).map(Tracker::via) {
                Some(via) => sync::issue(IssueFetch::AddIssue { clock }, vec![via], key),
                None => host::notify(api::Level::Error, &format!("couldn't fetch {key}: {tracker} isn't ready")),
            }),
        }
    }

    fn sync_picker(&mut self, title: &str, task: Option<TaskId>, tracker: Option<String>, revert: Option<(Status, i64)>, rows: Vec<(String, Pick)>) {
        let items: Vec<PickerItem> = rows.iter().enumerate().map(|(i, (label, _))| PickerItem::new(label.clone(), i.to_string())).collect();
        let id = api::pick(title, &items);
        self.asked.insert(id, Asked::Sync(PickerCtx { task, tracker, revert, rows: rows.into_iter().map(|(_, p)| p).collect() }));
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
        let task = ctx.task;
        let Some(pick) = picks.first().cloned() else { return };
        match pick {
            Pick::Issue(tracker, issue) => {
                if let Some(id) = task {
                    self.link_issue(id, &tracker, &issue);
                }
            }
            Pick::CreateIssue(tracker) => {
                let via = self.sync.ready(&tracker).map(Tracker::via);
                let op = self.op_id();
                if let (Some(t), Some(via)) = (task.and_then(|id| self.store.task(id)), via) {
                    sync::create(via, t.id, t.title.clone(), t.description.clone(), op);
                }
            }
            Pick::TypeKey => {
                if let Some(id) = task {
                    let hint = self.sync.trackers.ready().find_map(|t| t.about.ref_hint.clone()).unwrap_or_else(|| "PROJ-123".to_string());
                    let prompt = api::prompt(&format!("Link to issue (e.g. {hint})"), "");
                    self.asked.insert(prompt, Asked::LinkKey(id));
                }
            }
            Pick::Unlink => {
                if let Some(id) = task {
                    let (key, tracker) = self.store.task(id).and_then(|t| t.link.as_ref()).map(|l| (l.key.clone(), l.tracker.clone())).unwrap_or_default();
                    self.store.unlink(id);
                    self.save_and_refresh();
                    api::notify(&format!("Unlinked {key} -- the task stays, {} isn't touched", self.sync.trackers.name(&tracker)));
                }
            }
            Pick::OpenInBrowser => {
                if let Some(url) = task.and_then(|id| self.issue_url(id)) {
                    let _ = api::open_url(&url);
                }
            }
            Pick::Import(..) => {
                let count = picks.len();
                for pick in picks {
                    if let Pick::Import(tracker, issue) = pick {
                        if self.store.find_issue(&tracker, &issue.id, &issue.key).is_none() {
                            self.store.create_linked(&tracker, Some(issue.id.clone()), issue.key.clone(), sync::remote_update(&issue), None);
                        }
                    }
                }
                self.save_and_refresh();
                api::notify(&format!("Added {count} issue(s) to the agenda"));
            }
            Pick::Move(m) => {
                let (Some(id), Some(tracker)) = (task, ctx.tracker.clone()) else { return };
                let note = m.note.clone();
                self.take_move(id, &tracker, *m);
                self.after_edit();
                if let Some(note) = note {
                    api::notify(&note);
                }
            }
            Pick::StartBlocked => {
                if let Some(id) = task {
                    self.set_status(id, Status::Blocked);
                }
            }
            Pick::Priority(p) => {
                if let Some(id) = task {
                    self.store.set_priority(id, model::link::priority_for_level(p.level.as_deref()));
                    self.enqueue(id, OpKind::SetPriority(p.name));
                    self.after_edit();
                }
            }
            Pick::Assignee(p) => {
                if let Some(id) = task {
                    let (uid, name) = p.map(|p| (p.id, p.name)).unwrap_or_default();
                    self.enqueue(id, OpKind::SetAssignee { id: uid, name });
                    self.after_edit();
                }
            }
            Pick::SendAgain => {
                if let Some(id) = task {
                    self.store.retry(id);
                    self.after_edit();
                }
            }
            Pick::Forget => {
                if let Some(id) = task {
                    if let Some(op) = self.store.held_op(id).cloned() {
                        // Made: folded into what the agenda last saw.
                        self.store.op_succeeded(op.id);
                    }
                    self.store.retry(id);
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
        let stamp = clock::now().timestamp_millis();
        let rows: Vec<(Via, model::WorklogRow, String)> = self
            .worklog_rows()
            .into_iter()
            .filter(|r| r.minutes > 0)
            .filter_map(|r| {
                let op = format!("agenda:{stamp}:w{}:{}", r.task.0, r.date);
                Some((self.sync.ready(&r.tracker)?.via(), r, op))
            })
            .collect();
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
                // No answer: it may have been logged. Said so, so it isn't
                // simply sent again.
                Err(err) if err.is(code::UNKNOWN) => failures.push(format!("{key}: no answer -- it may have been logged; check before sending it again")),
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

    /// Asks each tracker that can tell, and has no Fenix metadata key,
    /// which of its projects you're in.
    fn ask_projects(&mut self) {
        let vias: Vec<Via> = self.sync.trackers.ready().filter(|t| t.can(can::PROJECT) && t.about.project_meta.is_none()).map(Tracker::via).collect();
        if !vias.is_empty() {
            sync::projects(vias);
        }
    }

    /// The sync's jobs that have ended, taken in.
    fn take_done(&mut self) {
        for done in sync::take_done() {
            match done {
                Done::Probed { id, found } => self.probed_one(id, found),
                Done::Pulled { tracker, asked, result } => self.pulled(tracker, asked, result),
                Done::OpDone { op_id, key, tracker, what, result } => self.op_done(op_id, key, tracker, what, result),
                Done::Issues { purpose, found, failures } => self.issues_came(purpose, found, failures),
                Done::Transitions { task, target, prev, result } => self.transitions_came(task, target, prev, result),
                Done::Priorities { task, result } => match result {
                    Ok(priorities) => {
                        let key = self.store.task(task).and_then(|t| t.issue_key()).unwrap_or_default().to_string();
                        let tracker = self.store.task(task).and_then(|t| t.link.as_ref()).map(|l| l.tracker.clone());
                        let rows = priorities.into_iter().map(|p| (p.name.clone(), Pick::Priority(p))).collect();
                        self.sync_picker(&format!("{key} priority"), Some(task), tracker, None, rows);
                    }
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't read {}'s priorities: {err}", self.task_tracker_name(task))),
                },
                Done::People { task, result } => match result {
                    Ok(people) if people.people.is_empty() => {
                        host::notify(api::Level::Error, &people.none.unwrap_or_else(|| format!("no people to assign to in {}", self.task_tracker_name(task))));
                    }
                    Ok(people) => {
                        let key = self.store.task(task).and_then(|t| t.issue_key()).unwrap_or_default().to_string();
                        let mut rows: Vec<(String, Pick)> = people.people.into_iter().map(|p| (format!("{} ({})", p.name, fenix_tracker::clean::line(&p.id, 100)), Pick::Assignee(Some(p)))).collect();
                        rows.push(("Nobody (unassign)".to_string(), Pick::Assignee(None)));
                        self.sync_picker(&format!("Assign {key}"), Some(task), None, None, rows);
                    }
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't list who to assign to: {err}")),
                },
                Done::WorklogsSent(results) => self.worklogs_sent(results),
                Done::Created { task, tracker, result } => match result {
                    Ok(seam::Created { key: Some(key), .. }) => {
                        if let Some(via) = self.sync.ready(&tracker).map(Tracker::via) {
                            sync::issue(IssueFetch::Created(task), vec![via], key);
                        }
                    }
                    Ok(seam::Created { opened: Some(said), .. }) => api::notify(&said),
                    Ok(_) => {}
                    Err(err) => host::notify(api::Level::Error, &format!("couldn't make the issue: {err}")),
                },
                Done::Remembered { tracker, result: Err(err) } => host::notify(api::Level::Error, &format!("{} couldn't keep that choice: {err}", self.sync.trackers.name(&tracker))),
                Done::Remembered { result: Ok(()), .. } => {}
                Done::Projects(found) => {
                    for (tracker, project) in found {
                        for p in self.project.iter_mut().chain(self.page.as_mut().and_then(|(_, page)| page.project.as_mut())) {
                            p.issues.retain(|(t, _)| *t != tracker);
                            p.issues.push((tracker.clone(), project.clone()));
                        }
                    }
                    self.refresh();
                }
            }
        }
    }

    /// The name of the tracker task `id` is linked to.
    fn task_tracker_name(&self, id: TaskId) -> String {
        self.store.task(id).and_then(|t| t.link.as_ref()).map_or_else(|| self.label(), |l| self.sync.trackers.name(&l.tracker))
    }
}

impl Extension for Agenda {
    /// Home's key and the page's hints, as the new scheme has them.
    fn key_scheme_changed(&mut self, _scheme: KeyScheme) {
        self.refresh();
    }

    fn activate(&mut self) -> Result<(), String> {
        self.store = Self::load(&self.kept());
        self.stamp = std::fs::metadata(self.kept()).and_then(|m| m.modified()).ok();
        // The minute (the clock in the modeline, the page) and another
        // writer's `agenda.json`.
        self.tick = Some(host::set_timer(5_000, true));
        self.minute = clock::now().timestamp() / 60;
        self.probe_in_use();
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
            "agenda.import" => {
                self.probe_if_stale();
                self.with_tracker(|a| a.import());
            }
            "agenda.sync" => {
                self.probe_if_stale();
                self.with_tracker(|a| a.sync_now(false));
            }
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
        let event = calls::deliver(event);
        self.take_done();
        let Some(event) = event else { return };
        match event {
            Event::Timer(t) if Some(t) == self.tick => {
                calls::expire(Instant::now());
                self.take_done();
                self.sync_due(Instant::now());
                self.reload_if_changed();
                let minute = clock::now().timestamp() / 60;
                if minute != self.minute && self.store.active_timer.is_some() {
                    self.minute = minute;
                    self.refresh();
                }
            }
            Event::Active => self.note_activity(),
            Event::SettingChanged(key) => {
                if key == "agenda.trackers" {
                    self.probe();
                }
                self.refresh();
            }
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
                        self.enqueue(task, OpKind::SetDescription(body));
                        self.after_edit();
                    }
                }
                (Some(Asked::Comment(task)), Some(body)) => {
                    self.enqueue(task, OpKind::AddComment(body));
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
        let ctx = page::Ctx { store: &self.store, now: clock::now(), categories: &categories, worklogs: &worklogs, round: self.round(), sync: self.sync_label(), import_keys: keys::keys_for("agenda.import", "SPC A i"), trackers: &self.sync.trackers };
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
            // A tracker page's add (Jira's `a` and `t`).
            "agenda.add_issue" => match IssueRef::read(payload) {
                Some(IssueRef { tracker, key, clock }) => {
                    self.add_issue(tracker.unwrap_or_else(|| legacy::ID.to_string()), key, clock);
                    Ok(String::new())
                }
                None => Err(format!("an agenda call: no issue in {payload}")),
            },
            // A tracker page's "in your agenda": its own, or every one's.
            "agenda.linked" => {
                let tracker = IssueRef::tracker_of(payload);
                let keys: Vec<&str> = self.store.tasks.iter().filter_map(|t| t.link.as_ref()).filter(|l| tracker.as_ref().is_none_or(|t| *t == l.tracker)).map(|l| l.key.as_str()).collect();
                Ok(serde_json::to_string(&keys).unwrap_or_default())
            }
            // Enter on an issue that's in the agenda: its task page.
            "agenda.open_task" => {
                let found = IssueRef::read(payload).and_then(|r| match &r.tracker {
                    Some(tracker) => self.store.find_by_key(tracker, &r.key),
                    None => self.store.find_by_any(&r.key),
                });
                let key = IssueRef::read(payload).map(|r| r.key).unwrap_or_default();
                match found {
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
            // An issue in the agenda changed on a tracker's page.
            "agenda.sync_quietly" => {
                self.sync_now(true);
                Ok(String::new())
            }
            // A tracker's settings or sign-in changed: asked again.
            "agenda.tracker_changed" => {
                match IssueRef::tracker_of(payload) {
                    Some(tracker) => self.probe_ids(vec![tracker]),
                    None => self.probe(),
                }
                Ok(String::new())
            }
            other => Err(format!("no service {other}")),
        };
        self.take_done();
        Some(answer)
    }
}

/// An issue named in a call to the agenda: `{"tracker", "ref"}`, or
/// `{"key"}` from Jira 1.2 and the notebook.
struct IssueRef {
    tracker: Option<String>,
    key: String,
    clock: bool,
}

impl IssueRef {
    fn read(payload: &str) -> Option<IssueRef> {
        let v: serde_json::Value = serde_json::from_str(payload).ok()?;
        let key = v.get("ref").or_else(|| v.get("key")).and_then(|k| k.as_str()).and_then(fenix_tracker::clean::key)?;
        Some(IssueRef { tracker: Self::tracker_of(payload), key, clock: v.get("clock").and_then(|c| c.as_bool()).unwrap_or(false) })
    }

    fn tracker_of(payload: &str) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(payload).ok()?.get("tracker")?.as_str().and_then(fenix_tracker::clean::key)
    }
}

impl Agenda {
    /// Why `C` can't post to task `id`'s tracker, when it can't.
    fn cant_comment(&self, id: TaskId) -> Option<String> {
        let link = self.store.task(id)?.link.as_ref()?;
        let t = self.sync.trackers.get(&link.tracker)?;
        (!t.can(can::COMMENT)).then(|| format!("{} doesn't take comments from the agenda -- post it on its own page", t.name()))
    }
}

api::export!(Agenda);

