//! Mermaid diagrams, as a WebAssembly extension: drawn as you type
//! beside a `.mmd` (`SPC m p`), on their own in a viewer (`SPC m v`),
//! their errors on their lines, their themes -- Fenix's own, which follows
//! the editor's, Mermaid's, and yours -- and exports. What was Fenix 1.x's
//! built-in diagrams.
//!
//! It's a language server for `mermaid`: Fenix sends it each diagram you
//! edit as you type, and it answers with the diagram's problems. Others
//! draw diagrams through its services: Fenix's reading view, the
//! notebook (`diagrams.render`, `diagrams.svg`, `diagrams.export`).

pub mod page;

use std::collections::{HashMap, HashSet};

use fenix_extension_api::lsp::types::{Diagnostic, DiagnosticSeverity, Position, Range, ServerCapabilities, Uri};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{self as api, buffer, host, image, Event, Extension, Key, Page, PickerItem, TextEdit};

use page::{Action, DiagramPage, Drawn};

/// The scale diagrams are drawn at over their natural size: sharp on
/// high-DPI screens and zoomed a little.
const SCALE: f32 = 2.0;

/// How long after a change it's drawn again, in milliseconds: typing
/// isn't held up by drawing.
const SETTLE: u32 = 150;

/// The keys that run `command`, which is `key` in a Mermaid file's
/// `SPC m` menu, as the key scheme in use writes them: see
/// [`menu_keys_from`]. Natively, where there's no Fenix to ask, its Vim
/// keys.
fn menu_keys(command: &str, key: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        // Fenix's `key_for` knows the leader's keys and chords, not a
        // `SPC m` menu's: the menu's own keys, then this one's in it.
        menu_keys_from(api::key_for(command), api::key_for("leader.local"), key)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = command;
        menu_keys_from(None, None, key)
    }
}

/// A command's own keys when it has some (`own`: yours, a chord); else
/// `key` in the `SPC m` menu, opened by `menu` (`SPC m`, or `C-k m` with
/// standard keys); else as Vim keys have it.
fn menu_keys_from(own: Option<String>, menu: Option<String>, key: &str) -> String {
    own.or_else(|| menu.map(|menu| format!("{menu} {key}"))).unwrap_or_else(|| format!("SPC m {key}"))
}

/// What a picker or prompt is for.
enum Asked {
    Kind,
    Starter(Vec<String>),
    Theme { path: String, names: Vec<String> },
    /// How to export it: copied, or saved as which kind of file.
    ExportHow { path: String },
    /// The file's name, once it's to be saved.
    Export { path: String },
    /// Where a new diagram goes: the project (`0`) or the notebook (`1`).
    Where { text: String },
    /// Its path in the project, under the diagrams folder.
    NewIn { text: String },
    /// A diagram of the project's, to open.
    Open { paths: Vec<String> },
    /// The path `path` moves to.
    Move { path: String },
}

/// A new diagram's text, given to the notebook to name and keep.
fn to_notebook(text: &str) {
    let payload = serde_json::json!({ "text": text }).to_string();
    api::call_service("notebook.new_diagram", &payload);
}

/// `diagrams.folder`: where a project's diagrams are made, under its top.
fn diagrams_folder() -> String {
    let folder = api::setting("diagrams.folder").map(|f| f.trim().trim_matches('/').replace('\\', "/")).unwrap_or_default();
    if folder.is_empty() || folder.split('/').any(|part| part == "..") {
        "docs/diagrams".into()
    } else {
        folder
    }
}

/// A path typed for a diagram, as one under the project: `/` between its
/// folders, no way out of it, and `.mmd` when it names no kind of its own.
fn diagram_rel(typed: &str) -> Option<String> {
    let typed = typed.trim().trim_start_matches("/project/").trim_matches('/').replace('\\', "/");
    if typed.is_empty() || typed.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return None;
    }
    let lower = typed.to_lowercase();
    Some(if lower.ends_with(".mmd") || lower.ends_with(".mermaid") { typed } else { format!("{typed}.mmd") })
}

/// Folders a project's diagrams aren't looked for in.
const SKIPPED: &[&str] = &["node_modules", "target", "dist", "build", "out", "vendor", "venv", "__pycache__"];

/// Every diagram under `dir` (a `/project/` path), as paths under the
/// project, folders first in name order. At most `limit`.
fn diagrams_under(dir: &str, rel: &str, limit: usize, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(String, bool)> = read.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.file_type().map(|t| t.is_dir()).unwrap_or(false))).collect();
    entries.sort();
    for (name, is_dir) in entries {
        if out.len() >= limit {
            return;
        }
        let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if is_dir {
            if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                diagrams_under(&format!("{dir}/{name}"), &child, limit, out);
            }
        } else {
            let lower = name.to_lowercase();
            if lower.ends_with(".mmd") || lower.ends_with(".mermaid") {
                out.push(child);
            }
        }
    }
}

/// A diagram's row in the list: its file, then its folder, then what it
/// is (its first line, `flowchart LR`).
fn diagram_row(rel: &str, text: &str) -> String {
    let (folder, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let kind = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("%%") && !l.starts_with("---")).unwrap_or("").chars().take(30).collect::<String>();
    format!("{name:<32} {:<28} {kind}", if folder.is_empty() { "." } else { folder })
}

/// What `e` offers, in its picker's order.
const EXPORT_HOW: [&str; 3] = ["Copy the picture to the clipboard", "Save it as an SVG file…", "Save it as a PNG file…"];

#[derive(Default)]
pub struct Diagrams {
    servers: Servers,
    pages: HashMap<u64, DiagramPage>,
    /// Documents changed and not yet drawn: (server, URI).
    dirty: HashSet<(u64, String)>,
    settle: Option<u64>,
    asked: HashMap<u64, Asked>,
    /// The page keyed last: what `SPC m` acts on from a page.
    last_page: Option<u64>,
}

/// Its language server: it only listens, and draws what changed once
/// you've stopped typing for a moment.
struct Lsp<'a> {
    dirty: &'a mut HashSet<(u64, String)>,
}

impl LanguageServer for Lsp<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities::default()
    }

    fn changed(&mut self, server: u64, _docs: &Documents, uri: &Uri) {
        self.dirty.insert((server, uri.as_str().to_string()));
    }
}

/// A `file:` URI as a path.
fn uri_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file:///").map(|r| if r.as_bytes().get(1) == Some(&b':') { r.to_string() } else { format!("/{r}") }).unwrap_or_else(|| uri.trim_start_matches("file://").to_string());
    let bytes = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Two spellings of the same file.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

/// The themes, as your settings and the editor theme make them.
fn themes() -> fenix_diagram::Themes {
    let t = api::theme();
    let rgb = |c: (u8, u8, u8)| [c.0, c.1, c.2];
    let custom = api::setting("diagrams.themes")
        .and_then(|text| serde_json::from_str::<Vec<Vec<String>>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !row.first().map(|n| n.trim().is_empty()).unwrap_or(true))
        .map(|row| {
            let names = ["primaryColor", "primaryBorderColor", "primaryTextColor", "lineColor", "secondaryColor", "background"];
            fenix_diagram::ThemeSpec {
                name: row[0].trim().to_string(),
                base: row.get(1).cloned().filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "base".into()),
                vars: names.iter().enumerate().filter_map(|(i, k)| row.get(i + 2).map(|v| (k.to_string(), v.trim().to_string()))).collect(),
            }
        })
        .collect();
    fenix_diagram::Themes {
        default: api::setting("diagrams.theme").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "fenix".into()),
        custom,
        editor: fenix_diagram::EditorColors { bg: rgb(t.background), fg: rgb(t.text), panel: rgb(t.panel), accent: rgb(t.accent), muted: rgb(t.muted), tint: rgb(t.tint), warn: rgb(t.warn) },
        font: api::setting("diagrams.font").filter(|s| !s.trim().is_empty()),
        rounded_edges: api::setting("diagrams.edges").as_deref() == Some("rounded"),
    }
}

/// `diagrams.export_theme`, unless it's "same" (as shown).
fn export_theme() -> Option<String> {
    api::setting("diagrams.export_theme").filter(|t| !t.trim().is_empty() && !t.eq_ignore_ascii_case("same"))
}

fn export_background() -> fenix_diagram::Background {
    match api::setting("diagrams.background").as_deref() {
        Some("transparent") => fenix_diagram::Background::Transparent,
        Some("white") => fenix_diagram::Background::White,
        _ => fenix_diagram::Background::Theme,
    }
}

/// `source` as an SVG, in the export theme, on the export background.
fn svg(source: &str) -> Result<String, String> {
    let r = fenix_diagram::render(source, &themes(), export_theme().as_deref()).map_err(|d| format!("line {}: {}", d.line + 1, d.message))?;
    Ok(fenix_diagram::svg_with(&r.svg, export_background()))
}

/// A diagram drawn: the picture for a page, or why not.
struct Picture {
    png: Vec<u8>,
    drawn: Drawn,
}

fn draw(source: &str, over: Option<&str>) -> Result<Picture, (usize, String)> {
    let r = fenix_diagram::render(source, &themes(), over).map_err(|d| (d.line, d.message))?;
    let png = fenix_diagram::png(&r.svg, SCALE, fenix_diagram::Background::Theme).map_err(|e| (0, e))?;
    Ok(Picture { png, drawn: Drawn { size: (r.width, r.height), nodes: r.nodes, theme: r.theme } })
}

/// A diagram's name: its file's.
fn title_of(path: &str) -> String {
    path.replace('\\', "/").rsplit('/').next().unwrap_or("diagram").to_string()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as base64, for a service's JSON answer.
fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Diagrams {
    /// What `path` holds now: as you're editing it, else on disk.
    fn source_of(&self, path: &str) -> Option<String> {
        buffer::text(path).or_else(|| std::fs::read_to_string(path).ok())
    }

    /// Page `id` drawn again from its source.
    fn redraw(&mut self, id: u64, text: Option<String>) {
        let Some(p) = self.pages.get(&id) else { return };
        let Some(text) = text.or_else(|| self.source_of(&p.source)) else { return };
        let over = p.theme_over.clone();
        let result = draw(&text, over.as_deref());
        let Some(p) = self.pages.get_mut(&id) else { return };
        match result {
            Ok(picture) => match image::load(&picture.png) {
                Ok(info) => {
                    if let Some((old, _, _)) = p.image.replace((info.id, info.width, info.height)) {
                        image::free(old);
                    }
                    if p.drawn.nodes.len() != picture.drawn.nodes.len() {
                        p.selected = None;
                    }
                    p.drawn = picture.drawn;
                    p.error = None;
                }
                Err(e) => p.error = Some((0, e)),
            },
            Err(e) => p.error = Some(e),
        }
        api::page::refresh(id);
    }

    /// What changed while you typed: its problems said, and the pages
    /// showing it drawn again.
    fn draw_dirty(&mut self) {
        for (server, uri) in std::mem::take(&mut self.dirty) {
            let Ok(parsed) = uri.parse::<Uri>() else { continue };
            let Some(text) = self.servers.documents(server).and_then(|d| d.text(&parsed)).map(str::to_string) else { continue };
            let problem = fenix_diagram::render(&text, &themes(), None).err();
            let diagnostics = problem
                .map(|d| Diagnostic {
                    range: Range { start: Position { line: d.line as u32, character: 0 }, end: Position { line: d.line as u32, character: 200 } },
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("diagram".into()),
                    message: d.message,
                    ..Default::default()
                })
                .into_iter()
                .collect();
            lsp::publish_diagnostics(server, parsed, diagnostics);
            let path = uri_path(&uri);
            let showing: Vec<u64> = self.pages.iter().filter(|(_, p)| same_path(&p.source, &path)).map(|(id, _)| *id).collect();
            for id in showing {
                self.redraw(id, Some(text.clone()));
            }
        }
    }

    /// The Mermaid file you're in, or the diagram page keyed last.
    fn here_path(&self) -> Option<String> {
        let here = api::here().and_then(|h| h.path).filter(|p| p.to_lowercase().ends_with(".mmd") || p.to_lowercase().ends_with(".mermaid"));
        here.or_else(|| self.last_page.and_then(|id| self.pages.get(&id)).map(|p| p.source.clone()))
    }

    fn open_page(&mut self, path: &str, viewer: bool) -> u64 {
        let title = title_of(path);
        let id = if viewer { api::page::open("viewer", &title) } else { api::page::open_beside("preview", &title, 0.5) };
        self.pages.insert(id, DiagramPage::new(path.to_string(), title, viewer));
        self.redraw(id, None);
        id
    }

    /// `SPC m p`: the preview beside the diagram (again closes it).
    fn toggle_preview(&mut self) {
        let Some(path) = self.here_path() else { return api::notify(&format!("{} previews a Mermaid diagram (.mmd)", menu_keys("diagram.preview", "p"))) };
        let open: Vec<u64> = self.pages.iter().filter(|(_, p)| !p.viewer && same_path(&p.source, &path)).map(|(id, _)| *id).collect();
        if open.is_empty() {
            self.open_page(&path, false);
        }
        for id in open {
            self.pages.remove(&id);
            api::page::close(id);
        }
    }

    fn pick_theme(&mut self, path: String) {
        let names = themes().names();
        let current = self.source_of(&path).map(|t| themes().name_for(&t, None)).unwrap_or_default();
        let about = |n: &str| match n {
            "default" => "Mermaid's lavender, what GitHub shows",
            "neutral" => "greys; prints well",
            "dark" => "Mermaid's dark",
            "forest" => "greens",
            "base" => "the one themeVariables tune",
            "fenix" => "the editor theme's colours",
            _ => "yours",
        };
        let items: Vec<PickerItem> = names.iter().enumerate().map(|(i, n)| PickerItem::new(format!("{} {n:<10} {}", if *n == current { "●" } else { " " }, about(n)), i.to_string())).collect();
        let id = api::pick("Theme", &items);
        self.asked.insert(id, Asked::Theme { path, names });
    }

    /// Theme `name` written into the diagram in `path`.
    fn set_theme(&mut self, path: &str, name: &str) {
        let Some(text) = self.source_of(path) else { return };
        let new = fenix_diagram::set_theme(&text, name);
        if new != text {
            let lines = text.lines().count() as u32 + 1;
            if let Err(e) = buffer::edit(path, &[TextEdit::new((0, 0), (lines, 0), new)]) {
                return host::notify(api::Level::Error, &e);
            }
        }
        for p in self.pages.values_mut().filter(|p| same_path(&p.source, path)) {
            p.theme_over = None;
        }
        api::notify(&format!("theme: {name}"));
    }

    /// `SPC m e` / `e`: the diagram copied as a picture, or exported
    /// beside it, in the project.
    fn export(&mut self, path: String) {
        let items: Vec<PickerItem> = EXPORT_HOW.iter().enumerate().map(|(i, how)| PickerItem::new(*how, i.to_string())).collect();
        let id = api::pick("Export the diagram", &items);
        self.asked.insert(id, Asked::ExportHow { path });
    }

    /// The file's name asked for, `ext` its kind.
    fn export_as(&mut self, path: String, ext: &str) {
        let stem = title_of(&path).rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| title_of(&path));
        let id = api::prompt("Export as (.svg or .png)", &format!("{stem}.{ext}"));
        self.asked.insert(id, Asked::Export { path });
    }

    /// `SPC m y` / `y`, or the first of `e`'s: the diagram on the
    /// clipboard as a picture (a PNG), as an export would draw it.
    fn copy_picture(&mut self, path: &str) {
        let Some(text) = self.source_of(path) else { return };
        match svg(&text).and_then(|svg| fenix_diagram::png(&svg, SCALE, fenix_diagram::Background::Theme)).and_then(|png| api::copy_image(&png)) {
            Ok(()) => api::notify(&format!("copied {} as a picture", title_of(path))),
            Err(e) => host::notify(api::Level::Error, &format!("couldn't copy it: {e}")),
        }
    }

    fn exported(&mut self, path: &str, name: &str) {
        let Some(text) = self.source_of(path) else { return };
        let Some(root) = api::here().and_then(|h| h.project).map(|p| p.root) else { return host::notify(api::Level::Error, "exports go into the project -- open one") };
        let dir = std::path::Path::new(path).parent().and_then(|d| d.strip_prefix(&root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))).unwrap_or_default();
        let out = if dir.is_empty() { format!("/project/{name}") } else { format!("/project/{dir}/{name}") };
        let written = svg(&text).and_then(|svg| {
            let bytes = if name.to_lowercase().ends_with(".png") { fenix_diagram::png(&svg, SCALE, fenix_diagram::Background::Theme)? } else { svg.into_bytes() };
            std::fs::write(&out, bytes).map_err(|e| e.to_string())
        });
        match written {
            Ok(()) => api::notify(&format!("exported {}", out.trim_start_matches("/project/"))),
            Err(e) => host::notify(api::Level::Error, &format!("couldn't export: {e}")),
        }
    }

    /// A starter picked for a new diagram: into the project, when one is
    /// open, or the notebook -- asked when there's a choice.
    fn starter_picked(&mut self, text: String) {
        if api::here().and_then(|h| h.project).is_none() {
            return to_notebook(&text);
        }
        let folder = diagrams_folder();
        let items = vec![PickerItem::new(format!("In this project, under {folder}/"), "0"), PickerItem::new("In the notebook".to_string(), "1")];
        let id = api::pick("Where it goes", &items);
        self.asked.insert(id, Asked::Where { text });
    }

    /// Its path in the project asked for: a name, or folders and a name
    /// (`auth/login-flow`), under the diagrams folder.
    fn name_in_project(&mut self, text: String) {
        let id = api::prompt("Diagram (folders/name)", &format!("{}/", diagrams_folder()));
        self.asked.insert(id, Asked::NewIn { text });
    }

    /// The new diagram written where it was named, its folders made, and
    /// opened with its picture beside it.
    fn made_in_project(&mut self, typed: &str, text: &str) {
        let Some(rel) = diagram_rel(typed) else { return host::notify(api::Level::Error, &format!("{typed:?} isn't a path in the project")) };
        let path = format!("/project/{rel}");
        if std::path::Path::new(&path).exists() {
            return host::notify(api::Level::Error, &format!("{rel} is there already"));
        }
        if let Some(dir) = std::path::Path::new(&path).parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                return host::notify(api::Level::Error, &format!("couldn't make {}: {e}", dir.display()));
            }
        }
        if let Err(e) = std::fs::write(&path, text) {
            return host::notify(api::Level::Error, &format!("couldn't write {rel}: {e}"));
        }
        host::open_file(&path, None);
        self.open_page(&path, false);
        api::notify(&format!("made {rel}"));
    }

    /// `diagram.list`: the project's diagrams, by folder; Enter opens one.
    fn list_project(&mut self) {
        if api::here().and_then(|h| h.project).is_none() {
            return host::notify(api::Level::Error, "the diagrams listed are a project's -- open one");
        }
        let mut found = Vec::new();
        diagrams_under("/project", "", 2000, &mut found);
        if found.is_empty() {
            return api::notify(&format!("no diagrams in the project yet -- {} makes one", menu_keys("diagram.new", "n")));
        }
        // Folders together, then by name.
        found.sort_by(|a, b| {
            let split = |p: &str| p.rsplit_once('/').map(|(d, n)| (d.to_string(), n.to_string())).unwrap_or_default();
            split(a).cmp(&split(b)).then(a.cmp(b))
        });
        let items: Vec<PickerItem> = found.iter().enumerate().map(|(i, rel)| PickerItem::new(diagram_row(rel, &std::fs::read_to_string(format!("/project/{rel}")).unwrap_or_default()), i.to_string())).collect();
        let id = api::pick("Diagrams in this project", &items);
        self.asked.insert(id, Asked::Open { paths: found });
    }

    /// `diagram.move`: the diagram in `path` to another folder, or another
    /// name -- what's typed in it, unsaved edits too, goes along.
    fn move_prompt(&mut self, path: String) {
        let Some(root) = api::here().and_then(|h| h.project).map(|p| p.root) else { return host::notify(api::Level::Error, "a diagram moves within its project -- open one") };
        let rel = std::path::Path::new(&path).strip_prefix(&root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.trim_start_matches("/project/").to_string());
        let id = api::prompt("Move to (folders/name)", &rel);
        self.asked.insert(id, Asked::Move { path });
    }

    fn moved(&mut self, path: &str, typed: &str) {
        let Some(rel) = diagram_rel(typed) else { return host::notify(api::Level::Error, &format!("{typed:?} isn't a path in the project")) };
        let to = format!("/project/{rel}");
        let Some(root) = api::here().and_then(|h| h.project).map(|p| p.root) else { return };
        let from = match std::path::Path::new(path).strip_prefix(&root) {
            Ok(r) => format!("/project/{}", r.to_string_lossy().replace('\\', "/")),
            Err(_) => path.to_string(),
        };
        if same_path(&from, &to) {
            return;
        }
        if std::path::Path::new(&to).exists() {
            return host::notify(api::Level::Error, &format!("{rel} is there already"));
        }
        let Some(text) = self.source_of(path) else { return host::notify(api::Level::Error, "couldn't read the diagram") };
        let made = std::path::Path::new(&to).parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).and_then(|()| std::fs::write(&to, &text)).and_then(|()| std::fs::remove_file(&from));
        match made {
            Ok(()) => {
                for p in self.pages.values_mut().filter(|p| same_path(&p.source, path)) {
                    p.source = to.clone();
                    p.title = title_of(&to);
                }
                host::open_file(&to, None);
                api::notify(&format!("moved to {rel}"));
            }
            Err(e) => host::notify(api::Level::Error, &format!("couldn't move it: {e}")),
        }
    }

    /// `diagrams.new`: a type, then a starter; the notebook names it.
    fn new_diagram(&mut self) {
        let items: Vec<PickerItem> = fenix_diagram::KINDS.iter().enumerate().map(|(i, k)| PickerItem::new(format!("{:<7} {:<19} {}", k.tag, k.header, k.about), i.to_string())).collect();
        let id = api::pick("Diagram", &items);
        self.asked.insert(id, Asked::Kind);
    }

    fn kind_picked(&mut self, i: usize) {
        let Some(kind) = fenix_diagram::KINDS.get(i) else { return };
        let starters = fenix_diagram::starters(kind);
        let items: Vec<PickerItem> = starters.iter().enumerate().map(|(i, (name, text))| PickerItem::new(format!("{name:<10} {}", text.lines().nth(1).unwrap_or("").trim()), i.to_string())).collect();
        let id = api::pick("Starter", &items);
        self.asked.insert(id, Asked::Starter(starters.into_iter().map(|(_, t)| t.to_string()).collect()));
    }

    fn action(&mut self, id: u64, action: Action) {
        let Some(p) = self.pages.get(&id) else { return };
        let path = p.source.clone();
        match action {
            Action::None => {}
            Action::Close => {
                self.pages.remove(&id);
                api::page::close(id);
            }
            Action::Edit => host::open_file(&path, None),
            Action::Line(line) => host::open_file(&path, Some(line as u32 + 1)),
            Action::CycleTheme => {
                let names = themes().names();
                if let Some(p) = self.pages.get_mut(&id) {
                    let current = p.theme_over.clone().unwrap_or_else(|| p.drawn.theme.clone());
                    let at = names.iter().position(|n| *n == current).map(|i| (i + 1) % names.len()).unwrap_or(0);
                    p.theme_over = Some(names[at].clone());
                    p.note = Some((format!("shown in {} -- t sets it", names[at]), false));
                }
                self.redraw(id, None);
            }
            Action::PickTheme => self.pick_theme(path),
            Action::Export => self.export(path),
            Action::CopyPicture => self.copy_picture(&path),
        }
    }
}

/// A service's payload.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Call {
    source: String,
    path: String,
    theme: Option<String>,
    format: Option<String>,
}

impl Extension for Diagrams {
    fn activate(&mut self) -> Result<(), String> {
        let faces = fenix_diagram::fonts_loaded();
        if faces == 0 {
            api::warn!("no fonts to draw diagrams' text with (permissions.fonts)");
        } else {
            api::info!("{faces} font faces to draw with");
        }
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "diagram.preview" => self.toggle_preview(),
            "diagram.viewer" => match self.here_path() {
                Some(path) => {
                    self.open_page(&path, true);
                }
                None => api::notify(&format!("{} shows a Mermaid diagram (.mmd) on its own", menu_keys("diagram.viewer", "v"))),
            },
            "diagram.theme" => match self.here_path() {
                Some(path) => self.pick_theme(path),
                None => host::notify(api::Level::Error, &format!("{} sets a diagram's theme", menu_keys("diagram.theme", "t"))),
            },
            "diagram.export" => match self.here_path() {
                Some(path) => self.export(path),
                None => host::notify(api::Level::Error, &format!("{} exports a diagram", menu_keys("diagram.export", "e"))),
            },
            "diagram.new" => self.new_diagram(),
            "diagram.list" => self.list_project(),
            "diagram.move" => match self.here_path() {
                Some(path) => self.move_prompt(path),
                None => host::notify(api::Level::Error, &format!("{} moves a diagram within its project", menu_keys("diagram.move", "M"))),
            },
            "diagram.copy_picture" => match self.here_path() {
                Some(path) => self.copy_picture(&path),
                None => host::notify(api::Level::Error, &format!("{} copies a diagram as a picture", menu_keys("diagram.copy_picture", "y"))),
            },
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timer(t) if Some(t) == self.settle => {
                self.settle = None;
                self.draw_dirty();
            }
            Event::ThemeChanged | Event::SettingChanged(_) => {
                let ids: Vec<u64> = self.pages.keys().copied().collect();
                for id in ids {
                    self.redraw(id, None);
                }
            }
            Event::Picked((id, choice)) => {
                let i = choice.and_then(|c| c.parse::<usize>().ok());
                match (self.asked.remove(&id), i) {
                    (Some(Asked::Kind), Some(i)) => self.kind_picked(i),
                    (Some(Asked::Starter(texts)), Some(i)) => {
                        if let Some(text) = texts.get(i) {
                            self.starter_picked(text.clone());
                        }
                    }
                    (Some(Asked::Where { text }), Some(0)) => self.name_in_project(text),
                    (Some(Asked::Where { text }), Some(1)) => to_notebook(&text),
                    (Some(Asked::Open { paths }), Some(i)) => {
                        if let Some(rel) = paths.get(i) {
                            host::open_file(&format!("/project/{rel}"), None);
                        }
                    }
                    (Some(Asked::ExportHow { path }), Some(0)) => self.copy_picture(&path),
                    (Some(Asked::ExportHow { path }), Some(1)) => self.export_as(path, "svg"),
                    (Some(Asked::ExportHow { path }), Some(2)) => self.export_as(path, "png"),
                    (Some(Asked::Theme { path, names }), Some(i)) => {
                        if let Some(name) = names.get(i) {
                            self.set_theme(&path, name);
                        }
                    }
                    _ => {}
                }
            }
            Event::Prompted((id, Some(name))) => match self.asked.remove(&id) {
                Some(Asked::Export { path }) => {
                    let name = name.trim();
                    if !name.is_empty() {
                        self.exported(&path, name);
                    }
                }
                Some(Asked::NewIn { text }) => self.made_in_project(&name, &text),
                Some(Asked::Move { path }) => self.moved(&path, &name),
                _ => {}
            },
            Event::ServiceReply((_, Err(e))) => host::notify(api::Level::Error, &format!("couldn't make the diagram: {e}")),
            _ => {}
        }
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        let before = self.dirty.len();
        self.servers.handle(server, message, &mut Lsp { dirty: &mut self.dirty });
        if self.dirty.len() != before && self.settle.is_none() {
            self.settle = Some(host::set_timer(SETTLE, false));
        }
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        match self.pages.get(&page) {
            Some(p) => Page::canvas(page::layout(p, width as usize)).typing(p.typing()),
            None => Page::new(),
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.last_page = Some(page);
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key);
        api::page::refresh(page);
        self.action(page, action);
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.paste(text);
            api::page::refresh(page);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.pages.get(&page).map(|p| p.source.clone())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if !matches!(kind, "preview" | "viewer") || state.is_empty() {
            return false;
        }
        self.pages.insert(page, DiagramPage::new(state.to_string(), title_of(state), kind == "viewer"));
        self.redraw(page, None);
        true
    }

    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let call: Call = serde_json::from_str(payload).unwrap_or_default();
        Some(match name {
            // `{source, theme?}` -> `{png, width, height}`: the picture a
            // page shows for it (Fenix's reading view, the notebook's).
            "diagrams.render" => draw(&call.source, call.theme.as_deref())
                .map_err(|(line, message)| format!("line {}: {message}", line + 1))
                .map(|p| {
                    let (w, h) = (p.drawn.size.0 * SCALE, p.drawn.size.1 * SCALE);
                    serde_json::json!({ "png": base64(&p.png), "width": w.round() as u32, "height": h.round() as u32 }).to_string()
                }),
            "diagrams.svg" => svg(&call.source),
            "diagrams.portable" => Ok(fenix_diagram::portable(&call.source, &themes(), export_theme().as_deref())),
            "diagrams.problem" => Ok(fenix_diagram::render(&call.source, &themes(), None).err().map(|d| format!("line {}: {}", d.line + 1, d.message)).unwrap_or_default()),
            // `{source, format}` -> `{bytes, theme}`: an export, for the
            // asker to write where it keeps things.
            "diagrams.export" => svg(&call.source).and_then(|svg| {
                let bytes = if call.format.as_deref() == Some("png") { fenix_diagram::png(&svg, SCALE, fenix_diagram::Background::Theme)? } else { svg.into_bytes() };
                Ok(serde_json::json!({ "bytes": base64(&bytes), "theme": export_theme().unwrap_or_default() }).to_string())
            }),
            "diagrams.copy_picture" => svg(&call.source).and_then(|svg| fenix_diagram::png(&svg, SCALE, fenix_diagram::Background::Theme)).and_then(|png| api::copy_image(&png)).map(|()| String::new()),
            "diagrams.view" => {
                self.open_page(&call.path, true);
                Ok(String::new())
            }
            "diagrams.preview" => {
                if !self.pages.values().any(|p| !p.viewer && same_path(&p.source, &call.path)) {
                    self.open_page(&call.path, false);
                }
                Ok(String::new())
            }
            "diagrams.pick_theme" => {
                self.pick_theme(call.path);
                Ok(String::new())
            }
            "diagrams.new" => {
                self.new_diagram();
                Ok(String::new())
            }
            other => Err(format!("no service {other}")),
        })
    }
}

api::export!(Diagrams);

#[cfg(test)]
mod tests {
    #[test]
    fn a_typed_path_is_one_under_the_project_with_its_kind() {
        assert_eq!(diagram_rel("docs/diagrams/auth/login-flow").as_deref(), Some("docs/diagrams/auth/login-flow.mmd"));
        assert_eq!(diagram_rel("/project/arch.mermaid").as_deref(), Some("arch.mermaid"), "a kind of its own kept");
        assert_eq!(diagram_rel("docs\\x").as_deref(), Some("docs/x.mmd"));
        assert_eq!(diagram_rel("../outside"), None, "no way out");
        assert_eq!(diagram_rel("docs//x"), None);
        assert_eq!(diagram_rel("   "), None);
    }

    #[test]
    fn a_project_s_diagrams_are_found_by_folder_and_not_in_what_s_built() {
        let dir = std::env::temp_dir().join(format!("fenix-diagrams-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in [("docs/diagrams/auth/login.mmd", "flowchart LR\n a-->b"), ("docs/diagrams/overview.mermaid", "sequenceDiagram\n"), ("README.md", "# x"), ("node_modules/pkg/x.mmd", ""), (".git/x.mmd", ""), ("target/y.mmd", "")] {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let mut found = Vec::new();
        diagrams_under(&dir.to_string_lossy(), "", 100, &mut found);
        assert_eq!(found, ["docs/diagrams/auth/login.mmd", "docs/diagrams/overview.mermaid"]);
        let row = diagram_row("docs/diagrams/auth/login.mmd", "%% a note\nflowchart LR\n a-->b");
        assert!(row.starts_with("login.mmd") && row.contains("docs/diagrams/auth") && row.ends_with("flowchart LR"), "{row}");
        let mut one = Vec::new();
        diagrams_under(&dir.to_string_lossy(), "", 1, &mut one);
        assert_eq!(one.len(), 1, "at most what's asked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn a_file_uri_is_its_path() {
        assert_eq!(uri_path("file:///C:/Users/me/My%20Notes/flow.mmd"), "C:/Users/me/My Notes/flow.mmd");
        assert_eq!(uri_path("file:///home/me/flow.mmd"), "/home/me/flow.mmd");
        assert!(same_path("C:\\Users\\me\\flow.mmd", "c:/users/me/flow.mmd"));
    }

    #[test]
    fn a_menu_command_is_named_by_the_keys_of_the_scheme_in_use() {
        // Fenix gives no keys for a command only in an SPC m menu: the
        // menu's, as each scheme writes them, and the command's in it.
        assert_eq!(menu_keys_from(None, Some("SPC m".into()), "p"), "SPC m p");
        assert_eq!(menu_keys_from(None, Some("C-k m".into()), "p"), "C-k m p");
        // A key of its own (yours) wins.
        assert_eq!(menu_keys_from(Some("F6".into()), Some("C-k m".into()), "p"), "F6");
        // The menu unbound, or natively: as Vim keys have it.
        assert_eq!(menu_keys_from(None, None, "e"), "SPC m e");
        assert_eq!(menu_keys("diagram.export", "e"), "SPC m e");
    }

    #[test]
    fn bytes_are_base64() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
