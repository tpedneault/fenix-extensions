//! The notebook, as a WebAssembly extension: notes, a daily journal,
//! diagrams, links between them and quick captures, kept as plain files in
//! one folder -- what Fenix 1.x had built in under `SPC n`, with the same
//! commands, keys, settings and files.
//!
//! The folder is your `notebook.folder` (Fenix's data folder without one),
//! read in place at `/folders/notebook`. Notes save themselves: the
//! extension is a language server for Markdown and Mermaid, so it sees
//! each notebook file change as you type, and writes it a second after
//! you stop, keeping the version before each editing session. `[[` in
//! Markdown completes a note's name and `gd` on a link follows it.
//! Diagrams are drawn by the diagrams extension (`diagrams.render`); a
//! journal day's agenda comes from the agenda extension (`agenda.due`).

pub mod blocks;
pub mod book;
pub mod clock;
pub mod export;
pub mod links_page;
pub mod meta;
pub mod page;
pub mod paths;
pub mod query;
pub mod reading;
pub mod templates;

mod actions;
mod capture;
mod exports;
mod links;
mod lsp;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fenix_extension_api::lsp::Servers;
use fenix_extension_api::{self as api, host, image, run, Event, Extension, HomeMark, HomeRow, HomeSection, Key, Level, Page, PickerItem};

use book::{Kind, Notebook};
use links_page::LinksPage;
use page::NotebookPage;
use paths::Places;
use reading::Picture;

/// The notebook page's kind (`page-open`, session restore).
const NOTEBOOK_KIND: &str = "notebook";
/// The links sidebar's.
const LINKS_KIND: &str = "links";

/// What a picker, a prompt or a process it started is for.
pub(crate) enum Asked {
    /// A new note's template; the picked value is its index.
    Template(Vec<(String, String)>),
    NoteName { template: String, body: String },
    DiagramName { text: String },
    /// A line to capture, the link back to where it was captured from.
    Capture { link: Option<String> },
    Search,
    /// A picker whose values are entry ids (`id` or `id:line`).
    Open,
    /// A picker whose values are filters for the notebook page.
    Filter,
    /// A picker whose values are names to link to.
    InsertLink,
    /// A picker whose values are `block:<id>` or `svg:<id>`.
    InsertDiagram,
    Export(exports::What),
    ExportPath { what: exports::What, how: exports::How },
    SaveBlock { text: String },
    Import,
    /// `git` for a journal day's commits: what it's printed so far.
    Git { step: capture::GitStep, out: Vec<String> },
}

/// A picture the notebook page shows: loaded, or why not.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Img {
    Pending,
    Ready { id: u32, width: u32, height: u32 },
    Failed(String),
}

/// Work left for the extension.
type Later = Box<dyn FnOnce(&mut NotebookExt)>;

thread_local! {
    /// What async work found, done on the extension once it's in hand
    /// again (a job can't borrow it while it waits).
    static LATER: RefCell<Vec<Later>> = RefCell::new(Vec::new());
}

/// Runs `f` on the extension when the work that called it is done.
pub(crate) fn later(f: impl FnOnce(&mut NotebookExt) + 'static) {
    LATER.with(|l| l.borrow_mut().push(Box::new(f)));
}

/// Something wrong, said in the message line.
pub(crate) fn error(message: &str) {
    host::notify(Level::Error, message);
}

#[derive(Default)]
pub struct NotebookExt {
    pub(crate) book: Option<Notebook>,
    /// Why it couldn't be opened, once said.
    pub(crate) failed: Option<String>,
    /// Where the notebook and the project are on this machine.
    pub(crate) places: Places,
    /// Project folders it's seen (language servers' roots, `here`), for
    /// `[[project:path]]` links.
    pub(crate) projects: Vec<String>,
    pub(crate) asked: HashMap<u64, Asked>,
    pub(crate) notebook_page: Option<(u64, NotebookPage)>,
    pub(crate) links_page: Option<(u64, LinksPage)>,
    pub(crate) images: HashMap<Picture, Img>,
    pub(crate) servers: Servers,
    /// Notebook files changed and not yet written: URI, and when.
    pub(crate) dirty: HashMap<String, std::time::Instant>,
    pub(crate) autosave: Option<u64>,
    /// The agenda's open tasks with a due date, as it last said
    /// (`agenda.due`): a journal day lists those due by then.
    pub(crate) agenda_due: Vec<(chrono::NaiveDate, String)>,
}

/// Whether a setting's text means on.
fn on(text: Option<String>, default: bool) -> bool {
    match text.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("true" | "on" | "yes" | "1") => true,
        Some("false" | "off" | "no" | "0") => false,
        _ => default,
    }
}

impl NotebookExt {
    /// The notebook, opened the first time it's needed.
    pub(crate) fn notebook(&mut self) -> Option<&mut Notebook> {
        if self.book.is_none() {
            match Notebook::open(Path::new(paths::ROOT)) {
                Ok(mut nb) => {
                    nb.history_limit = api::setting("notebook.history").and_then(|s| s.trim().parse().ok()).unwrap_or(50).max(1);
                    self.book = Some(nb);
                    self.failed = None;
                }
                Err(e) => {
                    let why = format!("couldn't open the notebook at {}: {e}", self.places.notebook.as_deref().unwrap_or(paths::ROOT));
                    if self.failed.as_deref() != Some(why.as_str()) {
                        error(&why);
                    }
                    self.failed = Some(why);
                    return None;
                }
            }
        }
        self.book.as_mut()
    }

    /// Where the notebook is on this machine: the setting (Fenix gives a
    /// folder's setting as where it is, set or not).
    fn read_places(&mut self) {
        self.places.notebook = api::setting("notebook.folder").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }

    /// Where you are, the project you're in noted on the way.
    pub(crate) fn here(&mut self) -> Option<api::Here> {
        let here = api::here()?;
        if let Some(p) = &here.project {
            self.note_project(&p.root);
            self.places.project = Some(p.root.clone());
        }
        Some(here)
    }

    pub(crate) fn note_project(&mut self, root: &str) {
        if !root.is_empty() && !self.projects.iter().any(|p| paths::same(p, root)) {
            self.projects.push(root.to_string());
        }
    }

    /// The project a note made now is about: the project you're in.
    pub(crate) fn project_name(&mut self) -> Option<String> {
        self.here().and_then(|h| h.project).map(|p| p.name)
    }

    /// A notebook file as Fenix names it.
    pub(crate) fn host_of(&self, guest: &Path) -> String {
        self.places.shown(guest)
    }

    /// The entry a file Fenix names is, if it's one of the notebook's.
    pub(crate) fn entry_at(&self, host: &str) -> Option<&book::Entry> {
        let guest = self.places.guest(host)?;
        self.book.as_ref()?.by_path(&guest)
    }

    /// Opens an entry for editing, in the focused pane; at `line` (from
    /// 0) when given.
    pub(crate) fn open_entry(&mut self, id: &str, line: Option<usize>) {
        let Some(e) = self.book.as_ref().and_then(|nb| nb.get(id)) else {
            return error("that entry isn't in the notebook any more");
        };
        let (path, kind) = (e.path.clone(), e.kind);
        host::open_file(&path.to_string_lossy(), line.map(|l| l as u32 + 1));
        if kind == Kind::Diagram {
            // Its preview, when the diagrams extension is there to draw it.
            let payload = serde_json::json!({ "path": self.host_of(&path) }).to_string();
            run::spawn(async move {
                let _ = run::call("diagrams.preview", &payload).await;
            });
        }
    }

    /// Everything that shows the notebook, shown again: its page, the
    /// links sidebar, its section on Home.
    pub(crate) fn changed(&mut self) {
        self.refresh_notebook_page();
        self.refresh_links_page();
        self.refresh_home();
    }

    /// The rows the notebook page shows.
    fn rows(&mut self) -> Vec<page::Row> {
        let project_files = on(api::setting("notebook.show_project_files"), false);
        let Some(nb) = self.notebook() else { return Vec::new() };
        let mut rows: Vec<page::Row> = nb
            .entries()
            .iter()
            .map(|e| page::Row {
                id: e.id.clone(),
                kind: e.kind,
                name: e.display_name(),
                tags: e.tags.clone(),
                about: e.about.clone(),
                pinned: e.pinned,
                modified: e.modified,
                date: e.date(),
                text: e.text.clone(),
                open_tasks: e.open_tasks().len(),
                versions: nb.history(&e.id).len(),
                exports: e.exports.iter().map(|x| x.path.display().to_string()).collect(),
                problem: None,
                project_file: None,
            })
            .collect();
        for row in rows.iter_mut().filter(|r| r.kind == Kind::Diagram) {
            if let Some(Img::Failed(why)) = self.images.get(&Picture::Diagram(row.text.trim_end().to_string())) {
                row.problem = Some(why.clone()).filter(|w| w.starts_with("line "));
            }
        }
        if project_files && Path::new(paths::PROJECT).is_dir() {
            let mut files = Vec::new();
            collect_docs(Path::new(paths::PROJECT), &mut files, 0);
            for path in files {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let kind = if path.extension().is_some_and(|e| e == "md") { Kind::Note } else { Kind::Diagram };
                rows.push(page::Row {
                    id: format!("file:{}", path.display()),
                    kind,
                    name: path.strip_prefix(paths::PROJECT).unwrap_or(&path).display().to_string().replace('\\', "/"),
                    tags: if kind == Kind::Note { meta::tags(&text) } else { Vec::new() },
                    about: None,
                    pinned: false,
                    modified: std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
                    date: None,
                    open_tasks: meta::tasks(&text).iter().filter(|t| !t.done).count(),
                    text,
                    versions: 0,
                    exports: Vec::new(),
                    problem: None,
                    project_file: Some(path),
                });
            }
        }
        rows
    }

    /// The tags and saved searches the page's chips are made of.
    fn chips(&self) -> (Vec<String>, Vec<String>) {
        match &self.book {
            Some(nb) => (nb.tags().into_iter().map(|(t, _)| t).collect(), nb.searches().to_vec()),
            None => (Vec::new(), Vec::new()),
        }
    }

    pub(crate) fn refresh_notebook_page(&mut self) {
        if self.notebook_page.is_none() {
            return;
        }
        let rows = self.rows();
        let (tags, searches) = self.chips();
        if let Some((id, p)) = &mut self.notebook_page {
            p.refresh(rows, tags, searches);
            api::page::refresh(*id);
        }
    }

    /// `SPC n n`: the notebook page (one: opened again where you are).
    pub(crate) fn open_notebook_page(&mut self) -> Option<&mut NotebookPage> {
        let nb = self.notebook()?;
        let _ = nb.rescan();
        let rows = self.rows();
        let (tags, searches) = self.chips();
        let old = self.notebook_page.take();
        let folder = self.places.shown(Path::new(paths::ROOT));
        let mut page = NotebookPage::new(rows, tags, searches, folder);
        if let Some((old_id, old_page)) = old {
            let selected = old_page.selected().map(|r| r.id.clone());
            page.filter = old_page.filter;
            if let Some(sel) = selected {
                page.select(&sel);
            }
            api::page::close(old_id);
        }
        let id = api::page::open(NOTEBOOK_KIND, "*notebook*");
        self.notebook_page = Some((id, page));
        self.notebook_page.as_mut().map(|(_, p)| p)
    }

    /// The notebook page, filtered.
    pub(crate) fn notebook_page_filtered(&mut self, filter: &str) {
        if let Some(p) = self.open_notebook_page() {
            p.filter.clear();
            p.filter.text = filter.to_string();
        }
    }

    /// Its section on Home: today's journal day, the pinned entries, then
    /// the last touched.
    pub(crate) fn refresh_home(&mut self) {
        let Some(nb) = self.book.as_ref() else { return };
        let today = clock::today();
        let mut rows: Vec<HomeRow> = Vec::new();
        let row = |e: &book::Entry, name: String, detail: &str| HomeRow { name, detail: detail.into(), mark: HomeMark::None, value: e.id.clone() };
        if let Some(day) = nb.day(today) {
            rows.push(row(day, "Today's journal".into(), "today"));
        }
        for e in nb.recent() {
            if rows.len() >= 5 {
                break;
            }
            if e.kind == Kind::Day || rows.iter().any(|r| r.value == e.id) {
                continue;
            }
            let kind = match e.kind {
                Kind::Diagram => "diagram",
                _ if e.pinned => "pinned",
                _ => "note",
            };
            rows.push(row(e, e.display_name(), kind));
        }
        if rows.is_empty() {
            api::home(None);
        } else {
            api::home(Some(&HomeSection { title: "notebook".into(), key: "SPC n n".into(), rows, most: 5 }));
        }
    }

    /// A picture the notebook page shows, as it's loaded; asked for the
    /// first time it's wanted.
    fn picture(&mut self, what: &Picture) -> Option<u32> {
        match self.images.get(what) {
            Some(Img::Ready { id, .. }) => return Some(*id),
            Some(_) => return None,
            None => {}
        }
        if self.images.len() > 200 {
            for img in self.images.values() {
                if let Img::Ready { id, .. } = img {
                    image::free(*id);
                }
            }
            self.images.clear();
        }
        match what {
            Picture::File(path) => {
                let img = match image::file(&path.to_string_lossy()) {
                    Ok(info) => Img::Ready { id: info.id, width: info.width, height: info.height },
                    Err(e) => Img::Failed(e),
                };
                let id = match &img {
                    Img::Ready { id, .. } => Some(*id),
                    _ => None,
                };
                self.images.insert(what.clone(), img);
                id
            }
            Picture::Diagram(source) => {
                self.images.insert(what.clone(), Img::Pending);
                let (key, payload) = (what.clone(), serde_json::json!({ "source": source }).to_string());
                run::spawn(async move {
                    let said = run::call("diagrams.render", &payload).await;
                    later(move |ext| ext.rendered(key, said));
                });
                None
            }
        }
    }

    /// A diagram came back drawn (or not): loaded, and the page again.
    fn rendered(&mut self, key: Picture, said: Result<String, String>) {
        let img = said.and_then(|text| {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let png = paths::unbase64(v.get("png").and_then(|p| p.as_str()).ok_or("no picture")?)?;
            let info = image::load(&png)?;
            Ok(Img::Ready { id: info.id, width: info.width, height: info.height })
        });
        let img = img.unwrap_or_else(|e| {
            if e.starts_with("line ") {
                Img::Failed(e)
            } else if e.contains("no service") || e.contains("diagrams.render") {
                Img::Failed("drawn by the Diagrams extension -- SPC h x installs it".into())
            } else {
                Img::Failed(e)
            }
        });
        self.images.insert(key, img);
        if let Some((id, _)) = &self.notebook_page {
            api::page::refresh(*id);
        }
    }

    /// The notebook page laid out `width` wide.
    fn notebook_page_canvas(&mut self, width: usize) -> Option<Page> {
        let (_, p) = self.notebook_page.as_ref()?;
        let base = p.selected().and_then(|r| r.project_file.clone().or_else(|| self.book.as_ref().and_then(|nb| nb.get(&r.id)).map(|e| e.path.clone())));
        let wanted: RefCell<Vec<Picture>> = RefCell::new(Vec::new());
        let images = &self.images;
        let nb = self.book.as_ref();
        let base_dir = base.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(paths::ROOT));
        let image_size = |what: &Picture| match images.get(what) {
            Some(Img::Ready { width, height, .. }) => Some((*width, *height)),
            _ => None,
        };
        let diagram_error = |source: &str| match images.get(&Picture::Diagram(source.to_string())) {
            Some(Img::Failed(why)) => Some(why.clone()),
            _ => None,
        };
        let image_path = |src: &str| {
            if src.contains("://") {
                return None;
            }
            let path = book::normalize(&base_dir.join(src.replace("%20", " ")));
            path.exists().then_some(path)
        };
        let embed = |link: &meta::Link| match nb.map(|nb| (nb, nb.resolve(link, base.as_deref()))) {
            Some((nb, book::Target::Entry { id, .. })) => match nb.get(&id) {
                Some(e) if e.kind == Kind::Diagram => reading::Embedded::Diagram { name: e.name.clone(), source: e.text.trim_end().to_string() },
                Some(e) => reading::Embedded::Note { name: e.display_name(), text: e.text.clone() },
                None => reading::Embedded::Missing(link.target.clone()),
            },
            _ => reading::Embedded::Missing(link.target.clone()),
        };
        let link_exists = |link: &meta::Link| nb.is_none_or(|nb| !matches!(nb.resolve(link, base.as_deref()), book::Target::Missing(_)));
        let ctx = reading::Ctx { image_size: &image_size, diagram_error: &diagram_error, image_path: &image_path, embed: &embed, link_exists: &link_exists, cell: (9.0, 19.0) };
        let image = |what: &Picture| match images.get(what) {
            Some(Img::Ready { id, .. }) => Some(*id),
            Some(_) => None,
            None => {
                wanted.borrow_mut().push(what.clone());
                None
            }
        };
        let (_, p) = self.notebook_page.as_ref()?;
        let canvas = page::layout(p, width, &ctx, &image);
        let typing = p.typing();
        // Pictures it wants and hasn't asked for: asked for now, the page
        // laid out again when they come.
        let wanted = wanted.into_inner();
        let mut again = false;
        for what in wanted {
            again |= self.picture(&what).is_some();
        }
        if again {
            if let Some((id, _)) = &self.notebook_page {
                api::page::refresh(*id);
            }
        }
        Some(Page::canvas(canvas).typing(typing))
    }

    /// Does what async work left for it.
    fn drain(&mut self) {
        loop {
            let next = LATER.with(|l| {
                let mut l = l.borrow_mut();
                (!l.is_empty()).then(|| l.remove(0))
            });
            match next {
                Some(f) => f(self),
                None => break,
            }
        }
    }

    /// A picker's choice, or a prompt's text.
    fn answered(&mut self, id: u64, value: Option<String>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let Some(value) = value else { return };
        self.on_answer(asked, value);
    }
}

/// A project's `.md`/`.mmd` files, not too deep, skipping build output.
fn collect_docs(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 || out.len() > 400 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "build" | "dist" | "vendor") {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_docs(&path, out, depth + 1),
            Ok(t) if t.is_file() => {
                if matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "mmd" | "mermaid")) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
}

/// A picker of entries, each giving back its id.
pub(crate) fn entry_items<'a>(entries: impl IntoIterator<Item = &'a book::Entry>) -> Vec<PickerItem> {
    entries
        .into_iter()
        .map(|e| {
            let tags: String = e.tags.iter().take(3).map(|t| format!(" #{t}")).collect();
            PickerItem::new(format!("{}  {}{tags}", e.kind.tag(), e.display_name()), e.id.clone())
        })
        .collect()
}

impl Extension for NotebookExt {
    fn activate(&mut self) -> Result<(), String> {
        self.read_places();
        if self.notebook().is_some() {
            self.refresh_home();
        }
        // What's due in the agenda, for the next journal day.
        self.fetch_due(|_| {});
        self.drain();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = self.command(id);
        self.drain();
        done
    }

    fn handle(&mut self, event: Event) {
        let Some(event) = run::deliver(event) else {
            self.drain();
            return;
        };
        match event {
            Event::Picked((id, choice)) => self.answered(id, choice),
            Event::Prompted((id, text)) => self.answered(id, text),
            Event::Timer(t) if Some(t) == self.autosave => {
                self.autosave = None;
                self.autosave_due();
            }
            Event::ProcessOutput(out) => {
                if let Some(Asked::Git { out: lines, .. }) = self.asked.get_mut(&out.process) {
                    if !out.stderr {
                        lines.push(out.line);
                    }
                }
            }
            Event::ProcessExit((id, code)) => {
                if let Some(Asked::Git { step, out }) = self.asked.remove(&id) {
                    self.git_done(step, code == Some(0), out);
                }
            }
            Event::SettingChanged(key) => match key.as_str() {
                "notebook.history" => {
                    if let Some(nb) = &mut self.book {
                        nb.history_limit = api::setting("notebook.history").and_then(|s| s.trim().parse().ok()).unwrap_or(50).max(1);
                    }
                }
                "notebook.show_project_files" => self.refresh_notebook_page(),
                _ => {}
            },
            Event::ThemeChanged => {
                // Diagrams are drawn in the editor's colours.
                let drawn: Vec<Picture> = self.images.keys().filter(|k| matches!(k, Picture::Diagram(_))).cloned().collect();
                for key in drawn {
                    if let Some(Img::Ready { id, .. }) = self.images.remove(&key) {
                        image::free(id);
                    }
                }
                self.refresh_notebook_page();
            }
            Event::HomePicked(id) => self.open_entry(&id, None),
            _ => {}
        }
        self.drain();
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        let out = if self.notebook_page.as_ref().is_some_and(|(id, _)| *id == page) {
            self.notebook_page_canvas(width as usize).unwrap_or_default()
        } else if let Some((_, p)) = self.links_page.as_ref().filter(|(id, _)| *id == page) {
            Page::canvas(links_page::layout(p, width as usize))
        } else {
            Page::new()
        };
        self.drain();
        out
    }

    fn page_key(&mut self, page: u64, key: Key) {
        if let Some((id, p)) = self.notebook_page.as_mut().filter(|(id, _)| *id == page) {
            let id = *id;
            let action = p.key(key);
            api::page::refresh(id);
            self.notebook_action(id, action);
        } else if let Some((id, p)) = self.links_page.as_mut().filter(|(id, _)| *id == page) {
            let id = *id;
            let action = p.key(key);
            api::page::refresh(id);
            self.links_action(id, action);
        }
        self.drain();
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some((id, p)) = self.notebook_page.as_mut().filter(|(id, _)| *id == page) {
            p.paste(text);
            api::page::refresh(*id);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        if self.notebook_page.as_ref().is_some_and(|(id, _)| *id == page) {
            return Some(String::new());
        }
        self.links_page.as_ref().filter(|(id, _)| *id == page).map(|(_, p)| p.id.clone())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        match kind {
            NOTEBOOK_KIND => {
                if self.notebook().is_none() {
                    return false;
                }
                let rows = self.rows();
                let (tags, searches) = self.chips();
                let folder = self.places.shown(Path::new(paths::ROOT));
                self.notebook_page = Some((page, NotebookPage::new(rows, tags, searches, folder)));
                true
            }
            LINKS_KIND => {
                let Some(name) = self.notebook().and_then(|nb| nb.get(state)).map(|e| e.display_name()) else { return false };
                self.links_page = Some((page, LinksPage::new(state.to_string(), name)));
                self.refresh_links_page();
                true
            }
            _ => false,
        }
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.language_server(server, message);
        self.drain();
    }

    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = self.service(name, payload);
        self.drain();
        Some(answer)
    }
}

impl NotebookExt {
    /// One of its services: what the control channel, Fenix's reading
    /// view and the diagrams extension ask of it.
    fn service(&mut self, name: &str, payload: &str) -> Result<String, String> {
        let call: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
        let text = |key: &str| call.get(key).and_then(|v| v.as_str()).map(str::to_string);
        match name {
            // `{to: "inbox" | "journal", text}` -> the line filed there as
            // `SPC n c` files it, without a link back; the file's path (the
            // control channel's `fenix/notebook.capture`).
            "notebook.capture" => {
                let to = match text("to").as_deref() {
                    Some("inbox") => capture::To::Inbox,
                    Some("journal") => capture::To::Journal,
                    other => return Err(format!("nowhere to capture to called {}", other.unwrap_or(""))),
                };
                let path = self.capture_into(to, false, None, &text("text").unwrap_or_default())?;
                api::notify(&format!("captured to {}", to.label()));
                Ok(self.host_of(&path))
            }
            // `{text}` -> asks the new diagram's name, then keeps it in the
            // notebook and opens it (the diagrams extension's new diagram).
            "notebook.new_diagram" => {
                self.notebook().ok_or("no notebook")?;
                let id = api::prompt("New diagram -- name", "");
                self.asked.insert(id, Asked::DiagramName { text: text("text").unwrap_or_default() });
                Ok(String::new())
            }
            // `{path}` -> the name the notebook keeps that file under, or
            // nothing when it isn't one of the notebook's.
            "notebook.entry_name" => {
                self.notebook();
                Ok(text("path").and_then(|p| self.entry_at(&p).map(|e| e.name.clone())).unwrap_or_default())
            }
            // `{from, links: [{target, heading, markdown, embed}]}` -> for
            // each, whether it goes anywhere and, for an entry, what it is:
            // Fenix's reading view shows embeds and missing links with it.
            "notebook.link" => {
                self.notebook().ok_or("no notebook")?;
                let from = text("from").and_then(|p| self.places.guest(&p));
                let links: Vec<meta::Link> = call.get("links").and_then(|l| l.as_array()).map(|a| a.iter().map(links::link_of).collect()).unwrap_or_default();
                let nb = self.book.as_ref().ok_or("no notebook")?;
                let out: Vec<serde_json::Value> = links.iter().map(|l| links::describe(nb, l, from.as_deref())).collect();
                Ok(serde_json::Value::Array(out).to_string())
            }
            // `{from, link: {target, heading, markdown, embed}}` -> follows
            // it as `gd` does (Fenix's reading view's Enter on a link).
            "notebook.follow" => {
                self.notebook().ok_or("no notebook")?;
                let from = text("from");
                let link = call.get("link").map(links::link_of).ok_or("no link to follow")?;
                self.follow(link, from.as_deref());
                Ok(String::new())
            }
            other => Err(format!("no service {other}")),
        }
    }
}

api::export!(NotebookExt);
