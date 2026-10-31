//! What its commands do, what a key on the notebook page asks for, and
//! what comes back from its pickers and prompts: new notes from a
//! template and a name, journal days, finding entries, and the page's
//! renames, tags, pins, history and deletes.

use std::path::{Path, PathBuf};

use fenix_extension_api::{self as api, host, run, PickerItem};

use crate::book::{self, Kind};
use crate::page::Action as NbAction;
use crate::{capture, entry_items, error, exports, later, meta, paths, templates, Asked, NotebookExt};

impl NotebookExt {
    /// One of its commands.
    pub(crate) fn command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "notebook.open" => {
                self.open_notebook_page();
            }
            "notebook.new_note" => self.new_note(),
            "notebook.new_diagram" => self.new_diagram(),
            "notebook.find" => self.find(),
            "notebook.recent" => self.reopen(),
            "notebook.journal" => self.open_day(crate::clock::today()),
            "notebook.capture" => self.capture(),
            "notebook.search" => self.search(),
            "notebook.tags" => self.tags(),
            "notebook.project" => self.project_notes(),
            "notebook.to_agenda" => self.send_to_agenda(),
            "notebook.journal_agenda" => self.journal_refresh_agenda(),
            "notebook.journal_commits" => self.journal_commits(),
            "notebook.export" => self.export_here(),
            "notebook.save_block" => self.save_block(),
            "notebook.import" => self.import(),
            "insert.diagram" => self.insert_diagram(),
            "notebook.backlinks" => self.backlinks(),
            "notebook.insert_link" => self.insert_link(),
            "notebook.copy_link" => self.copy_link(),
            "notebook.paste_image" => self.paste_image(),
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    /// What came back for `asked`: the picked value or the typed text.
    pub(crate) fn on_answer(&mut self, asked: Asked, value: String) {
        let name = value.trim().to_string();
        match asked {
            Asked::Template(templates) => {
                let Some((template, body)) = value.parse::<usize>().ok().and_then(|i| templates.get(i).cloned()) else { return };
                let id = api::prompt(&format!("New note ({template}) -- name"), "");
                self.asked.insert(id, Asked::NoteName { template, body });
            }
            Asked::NoteName { template, body } => {
                if name.is_empty() {
                    error("a note needs a name");
                    let id = api::prompt(&format!("New note ({template}) -- name"), "");
                    self.asked.insert(id, Asked::NoteName { template, body });
                    return;
                }
                self.create_note(&name, &body);
            }
            Asked::DiagramName { text } => {
                if name.is_empty() {
                    error("a diagram needs a name");
                    let id = api::prompt("New diagram -- name", "");
                    self.asked.insert(id, Asked::DiagramName { text });
                    return;
                }
                self.create_diagram(&name, &text);
            }
            Asked::Capture { link } => self.capture_done(link, &value),
            Asked::Search => {
                if !name.is_empty() {
                    self.search_results(&name);
                }
            }
            Asked::Open => {
                let (id, line) = match value.rsplit_once(':') {
                    Some((id, line)) if line.chars().all(|c| c.is_ascii_digit()) && !line.is_empty() => (id.to_string(), line.parse().ok()),
                    _ => (value.clone(), None),
                };
                self.open_entry(&id, line);
            }
            Asked::Filter => self.notebook_page_filtered(&value),
            Asked::InsertLink => self.insert_text(&format!("[[{value}]]")),
            Asked::InsertDiagram => {
                if let Some((how, id)) = value.split_once(':') {
                    self.insert_diagram_picked(id, how == "svg");
                }
            }
            Asked::Export(what) => {
                if let Some(how) = exports::How::from_value(&value) {
                    self.export_picked(what, how);
                }
            }
            Asked::ExportPath { what, how } => {
                if !name.is_empty() {
                    self.export_to_path(what, how, &name);
                }
            }
            Asked::SaveBlock { text } => {
                if !name.is_empty() {
                    self.create_diagram(&name, &text);
                }
            }
            Asked::Import => {
                if !name.is_empty() {
                    self.import_path(&name);
                }
            }
            Asked::Git { .. } => {}
        }
    }

    // ------------------------------------------------------------------
    // New notes and diagrams
    // ------------------------------------------------------------------

    /// `SPC n N`: pick a template, then name the note.
    fn new_note(&mut self) {
        if self.notebook().is_none() {
            return;
        }
        let templates: Vec<(String, String)> = templates::BUILT_IN.iter().map(|t| (t.name.to_string(), t.body.to_string())).collect();
        let items: Vec<PickerItem> = templates::BUILT_IN.iter().enumerate().map(|(i, t)| PickerItem::new(format!("{:<18} {}", t.name, t.about), i.to_string())).collect();
        let id = api::pick("Template", &items);
        self.asked.insert(id, Asked::Template(templates));
    }

    /// Makes a note called `name` from template `body` and opens it at its
    /// `{{cursor}}`.
    pub(crate) fn create_note(&mut self, name: &str, body: &str) {
        let project = self.project_name();
        let Some(nb) = self.notebook() else { return };
        if let Some(existing) = nb.by_name(name) {
            let id = existing.id.clone();
            api::notify(&format!("{name} is in the notebook already -- opened it"));
            return self.open_entry(&id, None);
        }
        let fill = templates::Fill { title: name.to_string(), project, now: None };
        let (text, (line, _)) = templates::render(body, &fill);
        match nb.create(Kind::Note, name, &text) {
            Ok(id) => {
                self.open_entry(&id, Some(line));
                self.changed();
                api::notify(&format!("{name} -- in your notebook; it saves itself as you type"));
            }
            Err(e) => error(&format!("couldn't make the note: {e}")),
        }
    }

    /// `SPC n d`: the diagrams extension asks the type and starter, then
    /// gives it back here to be named (`notebook.new_diagram`). Without it,
    /// a flowchart to start from.
    fn new_diagram(&mut self) {
        if self.notebook().is_none() {
            return;
        }
        run::spawn(async {
            if run::call("diagrams.new", "{}").await.is_err() {
                later(|ext| {
                    let id = api::prompt("New diagram (a flowchart; the Diagrams extension offers more) -- name", "");
                    ext.asked.insert(id, Asked::DiagramName { text: "flowchart TD\n    a[Start] --> b[End]\n".into() });
                });
            }
        });
    }

    /// Makes a diagram called `name` holding `text`, opens it and its
    /// preview.
    pub(crate) fn create_diagram(&mut self, name: &str, text: &str) {
        let Some(nb) = self.notebook() else { return };
        if let Some(existing) = nb.by_name(name) {
            let id = existing.id.clone();
            api::notify(&format!("{name} is in the notebook already -- opened it"));
            return self.open_entry(&id, None);
        }
        match nb.create(Kind::Diagram, name, text) {
            Ok(id) => {
                // The cursor on the blank line under the header.
                let line = text.lines().position(|l| l.trim().is_empty()).unwrap_or(1);
                self.open_entry(&id, Some(line));
                self.changed();
                api::notify(&format!("{name} -- draws as you type; saves itself"));
            }
            Err(e) => error(&format!("couldn't make the diagram: {e}")),
        }
    }

    // ------------------------------------------------------------------
    // Finding
    // ------------------------------------------------------------------

    /// `SPC n f`: every entry by name.
    fn find(&mut self) {
        let Some(nb) = self.notebook() else { return };
        let _ = nb.rescan();
        let items = entry_items(nb.recent());
        let id = api::pick("Notebook", &items);
        self.asked.insert(id, Asked::Open);
    }

    /// `SPC n r`: the entry you touched last.
    fn reopen(&mut self) {
        let Some(nb) = self.notebook() else { return };
        let last = nb.entries().iter().filter(|e| e.kind != Kind::Day).max_by_key(|e| e.modified).map(|e| e.id.clone());
        match last {
            Some(id) => self.open_entry(&id, None),
            None => api::notify("the notebook is empty -- SPC n N makes a note"),
        }
    }

    // ------------------------------------------------------------------
    // Journal days
    // ------------------------------------------------------------------

    /// What's due in the agenda, asked of the agenda extension; `then`
    /// once it's said (with what it said before, without one).
    pub(crate) fn fetch_due(&mut self, then: impl FnOnce(&mut NotebookExt) + 'static) {
        run::spawn(async move {
            let said = run::call("agenda.due", "null").await;
            later(move |ext| {
                if let Ok(text) = said {
                    // `[{"due": "2026-10-28", "text": "FEN-12 Its title"}, ...]`
                    let due: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
                    ext.agenda_due = due
                        .iter()
                        .filter_map(|d| Some((chrono::NaiveDate::parse_from_str(d.get("due")?.as_str()?, "%Y-%m-%d").ok()?, d.get("text")?.as_str()?.to_string())))
                        .collect();
                }
                then(ext);
            });
        });
    }

    /// The text a new journal day starts with: the agenda's tasks due by
    /// then, and the last day's open checkboxes.
    pub(crate) fn journal_template(&self, date: chrono::NaiveDate) -> String {
        let mut fill = templates::DayFill::default();
        for (due, task) in &self.agenda_due {
            if *due <= date {
                let when = if *due == date { "due today".to_string() } else { format!("overdue since {}", due.format("%b %-d")) };
                fill.agenda.push(format!("{task} · {when}"));
            }
        }
        if let Some(nb) = self.book.as_ref() {
            let before = nb.entries().iter().filter(|e| e.date().is_some_and(|d| d < date)).max_by_key(|e| e.date());
            if let Some(prev) = before {
                fill.carried = prev.open_tasks().into_iter().map(|t| t.text).filter(|t| !t.is_empty() && !fill.agenda.iter().any(|a| a.contains(t.as_str()))).collect();
            }
        }
        templates::journal(date, &fill)
    }

    /// Opens (making it if need be) the journal day `date`, with what's
    /// due by then in the agenda.
    pub(crate) fn open_day(&mut self, date: chrono::NaiveDate) {
        if self.notebook().is_none() {
            return;
        }
        if self.book.as_ref().and_then(|nb| nb.day(date)).is_some() {
            return self.open_day_now(date);
        }
        self.fetch_due(move |ext| ext.open_day_now(date));
    }

    fn open_day_now(&mut self, date: chrono::NaiveDate) {
        let template = self.journal_template(date);
        let Some(nb) = self.notebook() else { return };
        match nb.ensure_day(date, || template) {
            Ok((id, made)) => {
                let lines = nb.get(&id).map(|e| e.text.lines().count()).unwrap_or(1);
                self.open_entry(&id, Some(lines.saturating_sub(1)));
                self.changed();
                if made {
                    api::notify(&format!("{} -- a new day in your journal", date.format("%A %-d %B")));
                }
            }
            Err(e) => error(&format!("couldn't open the journal: {e}")),
        }
    }

    // ------------------------------------------------------------------
    // The page's actions
    // ------------------------------------------------------------------

    fn note(&mut self, text: String, bad: bool) {
        if let Some((id, p)) = &mut self.notebook_page {
            p.note = Some((text, bad));
            api::page::refresh(*id);
        }
    }

    pub(crate) fn notebook_action(&mut self, page_id: u64, action: NbAction) {
        match action {
            NbAction::None => {}
            NbAction::Close => {
                self.notebook_page = None;
                api::page::close(page_id);
            }
            NbAction::Edit(id) => self.open_entry(&id, None),
            NbAction::Read(id) => {
                self.open_entry(&id, None);
                host::run_command("notebook.read");
            }
            NbAction::OpenFile(path) => host::open_file(&path.to_string_lossy(), None),
            NbAction::NewNote => self.new_note(),
            NbAction::NewDiagram => self.new_diagram(),
            NbAction::Import => self.import(),
            NbAction::Day(date) => self.open_day(date),
            NbAction::PinSearch(query) => {
                if let Some(nb) = self.notebook() {
                    let said = match nb.toggle_search(&query) {
                        Ok(true) => (format!("pinned /{query} as a chip"), false),
                        Ok(false) => (format!("unpinned /{query}"), false),
                        Err(e) => (format!("couldn't save it: {e}"), true),
                    };
                    self.note(said.0, said.1);
                }
                self.refresh_notebook_page();
            }
            NbAction::Rename(id, name) => match self.rename(&id, &name) {
                Ok(n) => self.note(format!("renamed to {name}{}", if n > 0 { format!(" -- and {n} link{} to it", if n == 1 { "" } else { "s" }) } else { String::new() }), false),
                Err(e) => self.note(e, true),
            },
            NbAction::SetTags(id, tags) => {
                let text = self.book.as_ref().and_then(|nb| nb.get(&id)).map(|e| e.text.clone()).unwrap_or_default();
                let value = (!tags.is_empty()).then(|| meta::yaml_list(&tags));
                let new_text = meta::set_front_matter(&text, "tags", value.as_deref());
                self.write_entry(&id, &new_text);
                self.note(if tags.is_empty() { "tags cleared".into() } else { format!("tagged {}", tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")) }, false);
            }
            NbAction::Pin(id, pinned) => {
                if let Some(nb) = self.notebook() {
                    let _ = nb.set_pinned(&id, pinned);
                }
                self.changed();
                self.note(if pinned { "pinned".into() } else { "unpinned".into() }, false);
            }
            NbAction::Duplicate(id) => {
                self.flush(&id);
                let Some(nb) = self.notebook() else { return };
                let Some(e) = nb.get(&id).cloned() else { return };
                let name = nb.free_name(&format!("{} copy", e.name));
                match nb.create(if e.kind == Kind::Day { Kind::Note } else { e.kind }, &name, &e.text) {
                    Ok(new) => {
                        self.changed();
                        if let Some((_, p)) = &mut self.notebook_page {
                            p.select(&new);
                        }
                        self.note(format!("made {name}"), false);
                    }
                    Err(e) => self.note(format!("couldn't copy it: {e}"), true),
                }
            }
            NbAction::Delete(id) => {
                self.flush(&id);
                let name = self.book.as_ref().and_then(|nb| nb.get(&id)).map(|e| e.display_name()).unwrap_or_default();
                let result = self.notebook().map(|nb| nb.delete(&id));
                match result {
                    Some(Ok(())) => self.note(format!("{name} is in the notebook's .trash folder"), false),
                    Some(Err(e)) => self.note(format!("couldn't delete it: {e}"), true),
                    None => {}
                }
                self.changed();
            }
            NbAction::History(id) => {
                let versions: Vec<(String, PathBuf, String)> = self
                    .book
                    .as_ref()
                    .map(|nb| nb.history(&id))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(when, path)| {
                        let first = std::fs::read_to_string(&path).ok().and_then(|t| t.lines().find(|l| !l.trim().is_empty() && l.trim() != "---").map(str::to_string)).unwrap_or_default();
                        (when, path, first)
                    })
                    .collect();
                if let Some((pid, p)) = &mut self.notebook_page {
                    p.show_history(id, versions);
                    api::page::refresh(*pid);
                }
            }
            NbAction::Restore(id, version) => {
                self.flush(&id);
                let restored = self.notebook().map(|nb| nb.restore(&id, &version));
                match restored {
                    Some(Ok(text)) => {
                        self.reload_open(&id, &text);
                        self.note("put back -- the version it replaced is kept too".into(), false);
                    }
                    Some(Err(e)) => self.note(format!("couldn't put it back: {e}"), true),
                    None => {}
                }
                self.changed();
            }
            NbAction::ToProject(id) => self.move_to_project(&id),
            NbAction::Export(id) => self.export_entry(&id),
            NbAction::Theme(id) => self.diagram_service("diagrams.pick_theme", &id),
            NbAction::View(id) => self.diagram_service("diagrams.view", &id),
        }
    }

    /// The diagrams extension's `service` on entry `id`'s file.
    fn diagram_service(&mut self, service: &'static str, id: &str) {
        let Some(path) = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.clone()) else { return };
        let payload = serde_json::json!({ "path": self.host_of(&path) }).to_string();
        run::spawn(async move {
            if let Err(e) = run::call(service, &payload).await {
                later(move |_| error(&format!("{e} -- diagrams are the Diagrams extension's (SPC h x installs it)")));
            }
        });
    }

    /// `p` on the page: the entry moved into the project's docs.
    fn move_to_project(&mut self, id: &str) {
        let root = self.here().and_then(|h| h.project).map(|p| p.root);
        if root.is_none() || !Path::new(paths::PROJECT).is_dir() {
            return self.note("no project open -- open a file in one first".into(), true);
        }
        self.flush(id);
        let kind = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.kind);
        let dir = if kind == Some(Kind::Diagram) { Path::new(paths::PROJECT).join("docs").join("diagrams") } else { Path::new(paths::PROJECT).join("docs") };
        let moved = self.notebook().map(|nb| nb.move_out(id, &dir));
        match moved {
            Some(Ok(path)) => {
                self.changed();
                let shown = self.host_of(&path);
                self.note(format!("moved to {shown} -- it's the project's now"), false);
            }
            Some(Err(e)) => self.note(format!("couldn't move it: {e}"), true),
            None => {}
        }
    }

    /// Renames an entry: its file and the links to it. How many other
    /// entries' links changed.
    fn rename(&mut self, id: &str, name: &str) -> Result<usize, String> {
        self.flush(id);
        let old = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.clone()).ok_or("no such entry")?;
        let was_open = api::buffer::text(&old.to_string_lossy()).is_some();
        let changes = self.notebook().ok_or("no notebook")?.rename(id, name).map_err(|e| e.to_string())?;
        let new = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.clone()).unwrap_or(old.clone());
        // Open, its buffer is on a file that's gone: the new one opened.
        if was_open && new != old {
            host::open_file(&new.to_string_lossy(), None);
        }
        let n = changes.len();
        for (eid, text) in changes {
            self.write_entry(&eid, &text);
        }
        self.changed();
        Ok(n)
    }

    /// Writes `text` into an entry: into its buffer when it's open (and
    /// saves it), else to its file.
    pub(crate) fn write_entry(&mut self, id: &str, text: &str) {
        let Some(path) = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.clone()) else { return };
        let guest = path.to_string_lossy().into_owned();
        match api::buffer::text(&guest) {
            Some(now) => {
                if now != text {
                    if let Err(e) = api::buffer::edit(&guest, &[whole(&now, text)]) {
                        return error(&format!("couldn't change {}: {e}", self.host_of(&path)));
                    }
                }
                if let Some(nb) = self.book.as_mut() {
                    let _ = nb.snapshot(id);
                }
                if let Err(e) = api::buffer::save(&guest) {
                    error(&format!("couldn't save {}: {e}", self.host_of(&path)));
                }
                if let Some(nb) = self.book.as_mut() {
                    nb.refresh_text(id, text);
                }
            }
            None => {
                if let Some(nb) = self.notebook() {
                    if let Err(e) = nb.write_text(id, text) {
                        error(&format!("couldn't write {}: {e}", path.display()));
                    }
                }
            }
        }
        self.changed();
    }

    /// Puts `text` into an open buffer of the entry, as the file now is.
    fn reload_open(&mut self, id: &str, text: &str) {
        let Some(path) = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.to_string_lossy().into_owned()) else { return };
        if let Some(now) = api::buffer::text(&path) {
            if now != text {
                let _ = api::buffer::edit(&path, &[whole(&now, text)]);
                let _ = api::buffer::save(&path);
            }
        }
    }

    /// `SPC n /`: the query, then what matches.
    fn search(&mut self) {
        if self.notebook().is_none() {
            return;
        }
        let id = api::prompt("Search the notebook (words, tag:, type:, project:, is:todo)", "");
        self.asked.insert(id, Asked::Search);
    }

    /// `SPC n #`: every tag, most used first.
    fn tags(&mut self) {
        let Some(nb) = self.notebook() else { return };
        let tags = nb.tags();
        if tags.is_empty() {
            return api::notify("no tags yet -- #word in a note, or t on the notebook page");
        }
        let items: Vec<PickerItem> = tags.into_iter().map(|(t, n)| PickerItem::new(format!("#{t}  ({n})"), format!("tag:{t}"))).collect();
        let id = api::pick("Tag", &items);
        self.asked.insert(id, Asked::Filter);
    }

    /// `SPC n p`: the notes about the project you're in.
    fn project_notes(&mut self) {
        match self.project_name() {
            Some(name) => self.notebook_page_filtered(&format!("project:{name}")),
            None => error("no project here -- open a file in one first"),
        }
    }

    /// `SPC n i`: a `.md` or `.mmd` file copied into the notebook.
    fn import(&mut self) {
        if self.notebook().is_none() {
            return;
        }
        let start = self.here().and_then(|h| h.path).unwrap_or_default();
        let id = api::prompt("Import (a .md or .mmd file: open, or in the project)", &start);
        self.asked.insert(id, Asked::Import);
    }

    /// The file at `input` into the notebook: as its buffer has it, else
    /// read from the project.
    fn import_path(&mut self, input: &str) {
        let given = input.trim().trim_matches('"').to_string();
        let guest = self.places.guest(&given);
        let text = api::buffer::text(&given).or_else(|| guest.as_ref().and_then(|g| std::fs::read_to_string(g).ok()));
        let Some(text) = text else {
            return error(&format!("couldn't read {given} -- open it first, or put it in the project"));
        };
        let Some(nb) = self.notebook() else { return };
        match nb.import_text(Path::new(&given.replace('\\', "/")), &text) {
            Ok(id) => {
                let name = nb.get(&id).map(|e| e.name.clone()).unwrap_or_default();
                self.changed();
                self.open_entry(&id, None);
                api::notify(&format!("imported as {name}"));
            }
            Err(e) => error(&format!("couldn't import {given}: {e}")),
        }
    }

    /// Writes notebook file `id` now if it has unsaved typing.
    pub(crate) fn flush(&mut self, id: &str) {
        let Some(path) = self.book.as_ref().and_then(|nb| nb.get(id)).map(|e| e.path.clone()) else { return };
        let dirty: Vec<String> = self.dirty.keys().filter(|uri| self.places.guest(&paths::uri_path(uri)).is_some_and(|g| book::Notebook::same_file(&g, &path))).cloned().collect();
        for uri in dirty {
            self.dirty.remove(&uri);
            self.save_document(&uri);
        }
    }

    /// Types `text` at the cursor, in the file you're in.
    pub(crate) fn insert_text(&mut self, text: &str) {
        if let Err(e) = api::buffer::insert(text) {
            error(&format!("couldn't put it in: {e}"));
        }
    }
}

/// The edit that makes `now` into `new`: all of it replaced.
pub(crate) fn whole(now: &str, new: &str) -> api::TextEdit {
    let lines = now.split('\n').count() as u32;
    let last = now.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    api::TextEdit::new((0, 0), (lines.saturating_sub(1), last), new)
}

impl capture::To {
    pub(crate) fn label(self) -> &'static str {
        match self {
            capture::To::Journal => "today's journal",
            capture::To::Inbox => "Inbox",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_buffer_is_replaced_from_start_to_end() {
        let e = whole("a\nbc\n", "x");
        assert_eq!((e.start, e.end, e.text.as_str()), ((0, 0), (2, 0), "x"));
        let e = whole("a\nbcd", "x");
        assert_eq!(e.end, (1, 3));
    }
}
