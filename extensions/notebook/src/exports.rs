//! Export: a diagram to the clipboard (PNG, SVG, a Markdown block, its
//! source) or to a file (SVG, PNG, a `.mmd`); a note to Markdown that
//! renders anywhere or one self-contained HTML page, as a file or on the
//! clipboard. Every file export is remembered so `again` writes them all
//! after an edit. Files go into the project or the notebook's folder: the
//! places the extension may write.
//!
//! The drawing is the diagrams extension's, through its services
//! (`diagrams.portable`, `.svg`, `.export`, `.copy_picture`), which use
//! its export theme and background settings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fenix_extension_api::{self as api, run, PickerItem};

use crate::book::{self, ExportRecord, Kind, Target};
use crate::export::{self as md, LinkOut};
use crate::{error, later, meta, paths, Asked, NotebookExt};

/// What's exported.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum What {
    /// A diagram's file (in the sandbox), and its entry when it has one.
    Diagram { path: PathBuf, id: Option<String> },
    /// A note (or day), by entry id.
    Note(String),
}

/// How.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum How {
    ClipPng,
    ClipSvg,
    ClipMarkdown,
    ClipSource,
    ClipHtml,
    FilePng,
    FileSvg,
    FileMmd,
    FileMarkdown,
    FileHtml,
    /// Every file it was exported to before, again.
    Again,
}

const HOWS: [(How, &str); 11] = [
    (How::ClipPng, "clip-png"),
    (How::ClipSvg, "clip-svg"),
    (How::ClipMarkdown, "clip-md"),
    (How::ClipSource, "clip-source"),
    (How::ClipHtml, "clip-html"),
    (How::FilePng, "png"),
    (How::FileSvg, "svg"),
    (How::FileMmd, "mmd"),
    (How::FileMarkdown, "md"),
    (How::FileHtml, "html"),
    (How::Again, "again"),
];

impl How {
    fn value(self) -> &'static str {
        HOWS.iter().find(|(h, _)| *h == self).map(|(_, v)| *v).unwrap_or("")
    }

    pub(crate) fn from_value(v: &str) -> Option<How> {
        HOWS.iter().find(|(_, x)| *x == v).map(|(h, _)| *h)
    }

    fn extension(self) -> &'static str {
        match self {
            How::FilePng => "png",
            How::FileSvg => "svg",
            How::FileMmd => "mmd",
            How::FileMarkdown => "md",
            How::FileHtml => "html",
            _ => "",
        }
    }

    /// A remembered export's format, as the way to do it again.
    fn of_format(format: &str) -> How {
        match format {
            "png" => How::FilePng,
            "svg" => How::FileSvg,
            "mmd" => How::FileMmd,
            "md" => How::FileMarkdown,
            _ => How::FileHtml,
        }
    }
}

/// What a link in a note becomes, worked out before the async part.
#[derive(Debug, Clone)]
enum Pre {
    Href(String),
    Text,
    Diagram(String),
    Note(String),
}

/// A note, with what it needs to be exported away from the notebook.
#[derive(Debug, Clone)]
pub(crate) struct NoteSource {
    text: String,
    title: String,
    /// Its folder, for its pictures.
    base: PathBuf,
    /// Each link's (line, column) and what it becomes.
    outs: HashMap<(usize, usize), Pre>,
}

/// `source` made portable (the export theme written in), or as it is
/// without the diagrams extension.
async fn portable(source: &str) -> String {
    let payload = serde_json::json!({ "source": source }).to_string();
    run::call("diagrams.portable", &payload).await.unwrap_or_else(|_| source.to_string())
}

/// A note as portable Markdown, and as an HTML page. With `files`,
/// pictures are copied there (beside a Markdown file); without, the HTML
/// page carries them inside it.
async fn render_note(src: NoteSource, files: Option<PathBuf>) -> Result<(String, String), String> {
    let mut portables: HashMap<String, String> = HashMap::new();
    for pre in src.outs.values() {
        if let Pre::Diagram(s) = pre {
            if !portables.contains_key(s) {
                let p = portable(s).await;
                portables.insert(s.clone(), p);
            }
        }
    }
    let out = |l: &meta::Link| -> LinkOut {
        match src.outs.get(&(l.line, l.cols.start)) {
            Some(Pre::Href(h)) => LinkOut::Href(h.clone()),
            Some(Pre::Diagram(s)) => LinkOut::Diagram(portables.get(s).cloned().unwrap_or_else(|| s.clone())),
            Some(Pre::Note(t)) => LinkOut::Note(t.clone()),
            Some(Pre::Text) | None => LinkOut::Text,
        }
    };
    let mut text = md::to_markdown(&src.text, &out);
    // ```mermaid blocks in the note itself, made portable too.
    for (lang, _, block) in meta::fenced_blocks(&text.clone()) {
        if lang == "mermaid" {
            let p = portable(&block).await;
            if p.trim_end() != block.trim_end() {
                text = text.replacen(&block, p.trim_end(), 1);
            }
        }
    }
    // Pictures: copied beside a Markdown file, or carried in the page.
    let mut inline = text.clone();
    let mut at = 0;
    while let Some(start) = text[at..].find("](").map(|i| i + at + 2) {
        let Some(end) = text[start..].find(')').map(|i| i + start) else { break };
        let link = text[start..end].to_string();
        at = end;
        if link.contains("://") || link.is_empty() || link.starts_with("data:") {
            continue;
        }
        let file = book::normalize(&src.base.join(link.replace("%20", " ")));
        let picture = file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp"));
        if !picture || !file.is_file() {
            continue;
        }
        if let Some(dir) = &files {
            let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::copy(&file, dir.join(&name)).map_err(|e| e.to_string())?;
            let rel = format!("{}/{}", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), name).replace(' ', "%20");
            text = format!("{}{}{}", &text[..start], rel, &text[end..]);
            at = start + rel.len();
        }
        if let Ok(bytes) = std::fs::read(&file) {
            let mime = match file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("svg") => "image/svg+xml",
                Some("webp") => "image/webp",
                _ => "image/png",
            };
            inline = inline.replace(&format!("]({link})"), &format!("](data:{mime};base64,{})", paths::base64(&bytes)));
        }
    }
    // The page's diagrams, drawn by the diagrams extension: which ones,
    // then each drawn, then the page with them.
    let wanted = std::cell::RefCell::new(Vec::new());
    md::to_html(&inline, &src.title, &md::Palette::light(), &|s: &str| {
        wanted.borrow_mut().push(s.to_string());
        None
    });
    let mut drawn: HashMap<String, Option<String>> = HashMap::new();
    for s in wanted.into_inner() {
        if let std::collections::hash_map::Entry::Vacant(slot) = drawn.entry(s) {
            let svg = run::call("diagrams.svg", &serde_json::json!({ "source": slot.key() }).to_string()).await.ok();
            slot.insert(svg);
        }
    }
    let html = md::to_html(&inline, &src.title, &md::Palette::light(), &|s: &str| drawn.get(s).cloned().flatten());
    Ok((text, html))
}

/// A diagram exported as `format` (`svg`, `png`): its bytes, and the
/// theme it was drawn in.
async fn diagram_file(source: &str, format: &str) -> Result<(Vec<u8>, String), String> {
    let said = run::call("diagrams.export", &serde_json::json!({ "source": source, "format": format }).to_string()).await.map_err(no_diagrams)?;
    let v: serde_json::Value = serde_json::from_str(&said).map_err(|e| e.to_string())?;
    let bytes = paths::unbase64(v.get("bytes").and_then(|b| b.as_str()).ok_or("nothing drawn")?)?;
    Ok((bytes, v.get("theme").and_then(|t| t.as_str()).unwrap_or("").to_string()))
}

/// Why a diagram couldn't be drawn, said as where drawing comes from.
fn no_diagrams(e: String) -> String {
    if e.contains("no service") {
        "diagrams are drawn by the Diagrams extension -- SPC h x installs it".into()
    } else {
        e
    }
}

/// One file export, written: its format and theme, for the record.
async fn write_export(what: Export, how: How, path: PathBuf) -> Result<(String, String), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make the folder: {e}"))?;
    }
    match (what, how) {
        (Export::Diagram(source), How::FileSvg | How::FilePng) => {
            let format = how.extension();
            let (bytes, theme) = diagram_file(&source, format).await?;
            book::write_file(&path, &bytes).map_err(|e| e.to_string())?;
            Ok((format.to_string(), theme))
        }
        (Export::Diagram(source), _) => {
            let p = portable(&source).await;
            book::write_file(&path, p.as_bytes()).map_err(|e| e.to_string())?;
            Ok(("mmd".into(), String::new()))
        }
        (Export::Note(src), How::FileMarkdown) => {
            let files = path.parent().unwrap_or(Path::new(".")).join(format!("{}_files", path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
            let (text, _) = render_note(src, Some(files)).await?;
            book::write_file(&path, text.as_bytes()).map_err(|e| e.to_string())?;
            Ok(("md".into(), String::new()))
        }
        (Export::Note(src), _) => {
            let (_, html) = render_note(src, None).await?;
            book::write_file(&path, html.as_bytes()).map_err(|e| e.to_string())?;
            Ok(("html".into(), String::new()))
        }
    }
}

/// What an export is made from, read before the async part.
#[derive(Debug, Clone)]
enum Export {
    Diagram(String),
    Note(NoteSource),
}

impl NotebookExt {
    /// `e` on the notebook page.
    pub(crate) fn export_entry(&mut self, id: &str) {
        if let Some(path) = id.strip_prefix("file:").map(PathBuf::from) {
            let is_diagram = path.extension().is_some_and(|e| e == "mmd" || e == "mermaid");
            return if is_diagram { self.export_menu(What::Diagram { path, id: None }) } else { api::notify("export is for diagrams and notebook notes") };
        }
        let Some(e) = self.book.as_ref().and_then(|nb| nb.get(id)).cloned() else { return };
        self.flush(id);
        if e.kind == Kind::Diagram {
            self.export_menu(What::Diagram { path: e.path.clone(), id: Some(e.id.clone()) });
        } else {
            self.export_menu(What::Note(id.to_string()));
        }
    }

    /// `SPC n e`: the diagram or notebook note you're in.
    pub(crate) fn export_here(&mut self) {
        let Some(path) = self.here().and_then(|h| h.path) else { return api::notify("export is for diagrams and notebook notes") };
        self.notebook();
        let is_diagram = path.to_lowercase().ends_with(".mmd") || path.to_lowercase().ends_with(".mermaid");
        let entry = self.entry_at(&path).map(|e| (e.id.clone(), e.kind));
        match (is_diagram, entry) {
            (true, entry) => {
                let guest = self.places.guest(&path).unwrap_or_else(|| PathBuf::from(&path));
                self.export_menu(What::Diagram { path: guest, id: entry.map(|(id, _)| id) });
            }
            (false, Some((id, _))) => {
                self.flush(&id);
                self.export_menu(What::Note(id));
            }
            (false, None) => api::notify("export is for diagrams and notebook notes"),
        }
    }

    fn export_menu(&mut self, what: What) {
        let again = self.export_records(&what).len();
        let rows: Vec<(How, String)> = match &what {
            What::Diagram { .. } => vec![
                (How::ClipPng, "clipboard  PNG picture (2x)".into()),
                (How::ClipSvg, "clipboard  SVG markup".into()),
                (How::ClipMarkdown, "clipboard  a ```mermaid block, as it renders anywhere".into()),
                (How::ClipSource, "clipboard  the source".into()),
                (How::FileSvg, "file       SVG...".into()),
                (How::FilePng, "file       PNG (2x)...".into()),
                (How::FileMmd, "file       .mmd into the project...".into()),
                (How::Again, format!("again      everywhere it went ({again})")),
            ],
            What::Note(_) => vec![
                (How::ClipMarkdown, "clipboard  Markdown, as it renders anywhere".into()),
                (How::ClipHtml, "clipboard  HTML (the page's markup)".into()),
                (How::FileMarkdown, "file       Markdown, pictures beside it...".into()),
                (How::FileHtml, "file       one HTML page, diagrams drawn in...".into()),
                (How::Again, format!("again      everywhere it went ({again})")),
            ],
        };
        let items: Vec<PickerItem> = rows.into_iter().map(|(how, label)| PickerItem::new(label, how.value())).collect();
        let id = api::pick("Export", &items);
        self.asked.insert(id, Asked::Export(what));
    }

    fn entry_of(&self, what: &What) -> Option<String> {
        match what {
            What::Note(id) => Some(id.clone()),
            What::Diagram { id, path } => id.clone().or_else(|| self.book.as_ref().and_then(|nb| nb.by_path(path)).map(|e| e.id.clone())),
        }
    }

    fn export_records(&self, what: &What) -> Vec<ExportRecord> {
        self.entry_of(what).and_then(|id| self.book.as_ref().and_then(|nb| nb.get(&id))).map(|e| e.exports.clone()).unwrap_or_default()
    }

    /// The name an export is called by: the entry's, else the file's.
    fn export_name(&self, what: &What) -> String {
        match self.entry_of(what).and_then(|id| self.book.as_ref().and_then(|nb| nb.get(&id))) {
            Some(e) => book::slug(&e.name),
            None => match what {
                What::Diagram { path, .. } => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "diagram".into()),
                What::Note(_) => "note".into(),
            },
        }
    }

    /// Where a file export goes unless you say otherwise (as Fenix names
    /// it): where it went last time, else the project's docs, else the
    /// notebook's attachments.
    fn export_default_path(&mut self, what: &What, how: How) -> String {
        let ext = how.extension();
        if let Some(r) = self.export_records(what).into_iter().find(|r| r.path.extension().is_some_and(|e| e == ext)) {
            return r.path.display().to_string();
        }
        let file = format!("{}.{ext}", self.export_name(what));
        let project = self.here().and_then(|h| h.project).is_some() && Path::new(paths::PROJECT).is_dir();
        let dir = match (project, what) {
            (true, What::Diagram { .. }) => Path::new(paths::PROJECT).join("docs").join("diagrams"),
            (true, What::Note(_)) => Path::new(paths::PROJECT).join("docs"),
            (false, _) => Path::new(paths::ROOT).join("attachments").join("exports"),
        };
        self.host_of(&dir.join(file))
    }

    /// What `what` is made of now.
    fn export_source(&mut self, what: &What) -> Result<Export, String> {
        match what {
            What::Diagram { path, .. } => {
                let shown = self.host_of(path);
                let text = api::buffer::text(&path.to_string_lossy()).or_else(|| api::buffer::text(&shown)).or_else(|| std::fs::read_to_string(path).ok()).ok_or("the diagram isn't open")?;
                Ok(Export::Diagram(text))
            }
            What::Note(id) => {
                let nb = self.book.as_ref().ok_or("no notebook")?;
                let entry = nb.get(id).cloned().ok_or("no such note")?;
                let text = api::buffer::text(&entry.path.to_string_lossy()).unwrap_or(entry.text.clone());
                let mut outs = HashMap::new();
                for l in meta::links(&text).into_iter().filter(|l| !l.markdown) {
                    let pre = match nb.resolve(&l, Some(&entry.path)) {
                        Target::Entry { id, .. } => match nb.get(&id) {
                            Some(e) if l.embed && e.kind == Kind::Diagram => Pre::Diagram(e.text.clone()),
                            Some(e) if l.embed => Pre::Note(e.text.clone()),
                            _ => Pre::Text,
                        },
                        Target::Url(u) => Pre::Href(u),
                        _ => Pre::Text,
                    };
                    outs.insert((l.line, l.cols.start), pre);
                }
                Ok(Export::Note(NoteSource { text, title: entry.display_name(), base: entry.path.parent().map(Path::to_path_buf).unwrap_or_default(), outs }))
            }
        }
    }

    /// A choice in the export menu.
    pub(crate) fn export_picked(&mut self, what: What, how: How) {
        match how {
            How::FilePng | How::FileSvg | How::FileMmd | How::FileMarkdown | How::FileHtml => {
                let path = self.export_default_path(&what, how);
                let id = api::prompt("Export to (in the project or the notebook's folder)", &path);
                self.asked.insert(id, Asked::ExportPath { what, how });
            }
            How::Again => {
                let records = self.export_records(&what);
                if records.is_empty() {
                    return api::notify("it hasn't been exported to a file yet");
                }
                let source = match self.export_source(&what) {
                    Ok(s) => s,
                    Err(e) => return error(&e),
                };
                let mut jobs = Vec::new();
                for r in &records {
                    match self.places.guest(&r.path.to_string_lossy()) {
                        Some(guest) => jobs.push((How::of_format(&r.format), guest, r.path.clone())),
                        None => return error(&format!("{}: not somewhere the notebook may write any more", r.path.display())),
                    }
                }
                let id = self.entry_of(&what);
                run::spawn(async move {
                    let mut done = 0;
                    for (how, guest, shown) in jobs {
                        match write_export(source.clone(), how, guest).await {
                            Ok(_) => done += 1,
                            Err(e) => {
                                let at = shown.display().to_string();
                                return later(move |_| error(&format!("{at}: {e}")));
                            }
                        }
                    }
                    later(move |ext| {
                        let _ = id;
                        ext.changed();
                        api::notify(&format!("exported again to {done} place{}", if done == 1 { "" } else { "s" }));
                    });
                });
            }
            _ => self.export_clipboard(what, how),
        }
    }

    fn export_clipboard(&mut self, what: What, how: How) {
        let source = match self.export_source(&what) {
            Ok(s) => s,
            Err(e) => return error(&format!("couldn't export: {e}")),
        };
        run::spawn(async move {
            let done: Result<&'static str, String> = async {
                match (source, how) {
                    (Export::Diagram(text), How::ClipPng) => {
                        run::call("diagrams.copy_picture", &serde_json::json!({ "source": text }).to_string()).await.map_err(no_diagrams)?;
                        Ok("the picture")
                    }
                    (Export::Diagram(text), How::ClipSvg) => {
                        api::copy(&run::call("diagrams.svg", &serde_json::json!({ "source": text }).to_string()).await.map_err(no_diagrams)?);
                        Ok("the SVG")
                    }
                    (Export::Diagram(text), How::ClipMarkdown) => {
                        api::copy(&format!("```mermaid\n{}\n```\n", portable(&text).await.trim_end()));
                        Ok("a ```mermaid block")
                    }
                    (Export::Diagram(text), _) => {
                        api::copy(&text);
                        Ok("the source")
                    }
                    (Export::Note(src), How::ClipHtml) => {
                        api::copy(&render_note(src, None).await?.1);
                        Ok("the note as HTML")
                    }
                    (Export::Note(src), _) => {
                        api::copy(&render_note(src, None).await?.0);
                        Ok("the note as Markdown")
                    }
                }
            }
            .await;
            later(move |_| match done {
                Ok(what) => api::notify(&format!("copied {what}")),
                Err(e) => error(&format!("couldn't export: {e}")),
            });
        });
    }

    /// The path prompt's Enter: written there, and remembered.
    pub(crate) fn export_to_path(&mut self, what: What, how: How, input: &str) {
        let input = input.trim().trim_matches('"');
        let guest = if paths::is_host_absolute(input) { self.places.guest(input) } else if input.starts_with(paths::ROOT) || input.starts_with(paths::PROJECT) { Some(PathBuf::from(input)) } else { Some(Path::new(paths::PROJECT).join(input)) };
        let Some(guest) = guest.filter(|g| Path::new(paths::ROOT).is_dir() && (g.starts_with(paths::ROOT) || (g.starts_with(paths::PROJECT) && Path::new(paths::PROJECT).is_dir()))) else {
            return error("exports go into the project or the notebook's folder -- the places the notebook may write");
        };
        let source = match self.export_source(&what) {
            Ok(s) => s,
            Err(e) => return error(&format!("couldn't export: {e}")),
        };
        let entry = self.entry_of(&what);
        let shown = self.host_of(&guest);
        run::spawn(async move {
            let written = write_export(source, how, guest).await;
            later(move |ext| match written {
                Ok((format, theme)) => {
                    // Remembered on the entry, for `again`.
                    if let (Some(id), Some(nb)) = (entry, ext.book.as_mut()) {
                        let _ = nb.add_export(&id, ExportRecord { path: PathBuf::from(&shown), format, theme });
                    }
                    ext.changed();
                    api::notify(&format!("exported to {shown} -- again (e) writes it there after edits"));
                }
                Err(e) => error(&format!("couldn't export: {e}")),
            });
        });
    }

    /// `SPC i D`'s choice: the diagram's block at the cursor, or it
    /// exported as an SVG beside the file you're in and linked.
    pub(crate) fn insert_diagram_picked(&mut self, id: &str, svg: bool) {
        let Some(e) = self.book.as_ref().and_then(|nb| nb.get(id)).cloned() else { return };
        if !svg {
            let text = e.text.clone();
            run::spawn(async move {
                let p = portable(&text).await;
                later(move |ext| ext.insert_text(&format!("```mermaid\n{}\n```\n", p.trim_end())));
            });
            return;
        }
        let here = self.here().and_then(|h| h.path).and_then(|p| self.places.guest(&p));
        let dir = match here.as_deref().and_then(Path::parent) {
            Some(dir) => dir.to_path_buf(),
            None if Path::new(paths::PROJECT).is_dir() => PathBuf::from(paths::PROJECT),
            None => return error("the SVG goes beside the file you're in -- in the project or the notebook"),
        };
        let file = dir.join(format!("{}.svg", book::slug(&e.name)));
        let shown = self.host_of(&file);
        let (name, entry) = (e.name.clone(), e.id.clone());
        run::spawn(async move {
            let written = write_export(Export::Diagram(e.text.clone()), How::FileSvg, file.clone()).await;
            later(move |ext| match written {
                Ok((format, theme)) => {
                    if let Some(nb) = ext.book.as_mut() {
                        let _ = nb.add_export(&entry, ExportRecord { path: PathBuf::from(&shown), format, theme });
                    }
                    let link = file.file_name().unwrap_or_default().to_string_lossy().replace(' ', "%20");
                    ext.insert_text(&format!("![{name}]({link})"));
                    api::notify(&format!("exported {shown} -- e on the diagram writes it again"));
                }
                Err(e) => error(&format!("couldn't export: {e}")),
            });
        });
    }

    /// `SPC n s`: the ```mermaid block under the cursor, saved to the
    /// notebook as a diagram.
    pub(crate) fn save_block(&mut self) {
        let Some(here) = self.here() else { return error("SPC n s saves the ```mermaid block the cursor is in") };
        let Some(path) = here.path.clone() else { return error("SPC n s saves the ```mermaid block the cursor is in") };
        let line = here.line.saturating_sub(1) as usize;
        let text = api::buffer::text(&path).unwrap_or_default();
        let Some(source) = mermaid_block_at(&text, line) else { return error("SPC n s saves the ```mermaid block the cursor is in") };
        if self.notebook().is_none() {
            return;
        }
        let stem = paths::slashed(&path).rsplit('/').next().unwrap_or("").rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_default();
        let suggested = format!("{stem} diagram");
        let name = self.book.as_ref().map(|nb| nb.free_name(&suggested)).unwrap_or(suggested);
        let id = api::prompt("Save the diagram to the notebook as", &name);
        self.asked.insert(id, Asked::SaveBlock { text: format!("{}\n", source.trim_end()) });
    }
}

/// The ```mermaid block `line` (from 0) is in: its source.
pub(crate) fn mermaid_block_at(text: &str, line: usize) -> Option<String> {
    meta::fenced_blocks(text).into_iter().find(|(lang, r, _)| lang == "mermaid" && line + 1 >= r.start && line <= r.end).map(|(_, _, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mermaid_block_is_found_from_any_of_its_lines() {
        let text = "# Readme\n\n```mermaid\nflowchart LR\n  a --> b\n```\n\nafter\n";
        assert_eq!(mermaid_block_at(text, 3).as_deref(), Some("flowchart LR\n  a --> b"));
        assert_eq!(mermaid_block_at(text, 2).as_deref(), Some("flowchart LR\n  a --> b"), "its fence");
        assert_eq!(mermaid_block_at(text, 7), None);
    }

    #[test]
    fn export_choices_go_both_ways() {
        for (how, v) in HOWS {
            assert_eq!(How::from_value(v), Some(how));
            assert_eq!(how.value(), v);
        }
        assert_eq!(How::of_format("svg"), How::FileSvg);
        assert_eq!(How::FileHtml.extension(), "html");
    }

    /// A note's embeds and links, worked out from the notebook, then
    /// rendered without the diagrams extension (its blocks as they are).
    #[test]
    fn a_note_renders_to_markdown_and_html_with_its_embeds() {
        let dir = tempfile::tempdir().unwrap();
        let mut nb = book::Notebook::open(dir.path()).unwrap();
        nb.create(Kind::Diagram, "TC flow", "flowchart TD\n    a --> b\n").unwrap();
        let id = nb.create(Kind::Note, "Bench", "# Bench\n\nSee [[TC flow]].\n\n![[TC flow]]\n").unwrap();
        let entry = nb.get(&id).unwrap().clone();
        let mut outs = HashMap::new();
        for l in meta::links(&entry.text) {
            let pre = match nb.resolve(&l, Some(&entry.path)) {
                Target::Entry { id, .. } if l.embed => Pre::Diagram(nb.get(&id).unwrap().text.clone()),
                _ => Pre::Text,
            };
            outs.insert((l.line, l.cols.start), pre);
        }
        let src = NoteSource { text: entry.text.clone(), title: "Bench".into(), base: entry.path.parent().unwrap().to_path_buf(), outs };
        let md = md::to_markdown(&src.text, &|l: &meta::Link| match src.outs.get(&(l.line, l.cols.start)) {
            Some(Pre::Diagram(s)) => LinkOut::Diagram(s.clone()),
            _ => LinkOut::Text,
        });
        assert!(md.contains("See TC flow.") && md.contains("```mermaid\nflowchart TD\n    a --> b\n```"), "{md}");
    }
}
