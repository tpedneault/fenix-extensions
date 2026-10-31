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

/// What a picker or prompt is for.
enum Asked {
    Kind,
    Starter(Vec<String>),
    Theme { path: String, names: Vec<String> },
    Export { path: String },
}

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
        let Some(path) = self.here_path() else { return api::notify("SPC m p previews a Mermaid diagram (.mmd)") };
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

    /// `SPC m e` / `e`: the diagram exported beside it, in the project.
    fn export(&mut self, path: String) {
        let stem = title_of(&path).rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| title_of(&path));
        let id = api::prompt("Export as (.svg or .png)", &format!("{stem}.svg"));
        self.asked.insert(id, Asked::Export { path });
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
                None => api::notify("SPC m v shows a Mermaid diagram (.mmd) on its own"),
            },
            "diagram.theme" => match self.here_path() {
                Some(path) => self.pick_theme(path),
                None => host::notify(api::Level::Error, "SPC m t sets a diagram's theme"),
            },
            "diagram.export" => match self.here_path() {
                Some(path) => self.export(path),
                None => host::notify(api::Level::Error, "SPC m e exports a diagram"),
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
                            let payload = serde_json::json!({ "text": text }).to_string();
                            api::call_service("notebook.new_diagram", &payload);
                        }
                    }
                    (Some(Asked::Theme { path, names }), Some(i)) => {
                        if let Some(name) = names.get(i) {
                            self.set_theme(&path, name);
                        }
                    }
                    _ => {}
                }
            }
            Event::Prompted((id, Some(name))) => {
                if let Some(Asked::Export { path }) = self.asked.remove(&id) {
                    let name = name.trim();
                    if !name.is_empty() {
                        self.exported(&path, name);
                    }
                }
            }
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
    use super::*;

    #[test]
    fn a_file_uri_is_its_path() {
        assert_eq!(uri_path("file:///C:/Users/me/My%20Notes/flow.mmd"), "C:/Users/me/My Notes/flow.mmd");
        assert_eq!(uri_path("file:///home/me/flow.mmd"), "/home/me/flow.mmd");
        assert!(same_path("C:\\Users\\me\\flow.mmd", "c:/users/me/flow.mmd"));
    }

    #[test]
    fn bytes_are_base64() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
