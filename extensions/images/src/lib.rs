//! The image viewer, as a WebAssembly extension: PNG, JPEG, GIF, WebP,
//! BMP and SVG opened in a pane however you open them (`contributes.opens`),
//! zoomed and panned with the diagram viewer's keys, moving pictures
//! played, and `]` `[` through the pictures of their folder. `SPC f i`
//! previews the picture a line of code or Markdown names, beside it.
//!
//! Each picture is a surface: its pixels drawn here, the whole frame at
//! once (`viewer`), so zooming, panning and playing are as smooth as the
//! pane is big, and actual size is a point a pixel -- which a canvas
//! picture, laid out in cells and decoded by Fenix at most 2400 wide,
//! can't say.

pub mod decode;
pub mod folder;
pub mod format;
pub mod render;
pub mod svg;
pub mod view;
pub mod viewer;

use std::collections::HashMap;

use fenix_extension_api::{export, host, page, surface, Event, Extension, Level, NamedKey, SurfaceInput, SurfaceKey, SurfaceOptions, SurfaceScale};

use viewer::{Action, Colors, Saved, Viewer};

/// A picture opened as a file: in the pane you're in.
const IMAGE: &str = "image";
/// A picture a line names (`SPC f i`): beside the code, the focus staying
/// there.
const PREVIEW: &str = "preview";
/// The preview's share of the pane it opens beside.
const PREVIEW_WIDTH: f32 = 0.4;

#[derive(Default)]
struct Images {
    /// By surface.
    viewers: HashMap<u64, Viewer>,
    /// A moving picture's next frame: timer, surface.
    timers: HashMap<u64, u64>,
    /// The picture looked at last: what the commands act on, and what
    /// the modeline says.
    last: Option<u64>,
    preview: Option<u64>,
    colors: Option<Colors>,
}

fn say(level: Level, message: &str) {
    host::notify(level, message)
}

/// The editor theme's colours.
fn colors() -> Colors {
    let t = fenix_extension_api::theme();
    Colors { background: t.background, text: t.text, muted: t.muted, panel: t.panel, warn: t.warn }
}

/// Where a full path Fenix gave is under `/project`, if it is: the first
/// of `folder::guest_candidates` that's a file of the same size.
fn in_project(full: &str, size: usize) -> Option<String> {
    if full.starts_with("/project/") {
        return Some(full.to_string());
    }
    folder::guest_candidates(full).into_iter().find(|c| std::fs::metadata(c).is_ok_and(|m| m.is_file() && m.len() == size as u64))
}

/// The pictures in the folder of `path`, in order: under `/project`
/// read there; elsewhere, a full path, as Fenix lists the folder of a
/// picture it gave (`folder-list`, the files it opens), which it may then
/// read.
fn folder_of(path: &str) -> Vec<String> {
    let (dir, _) = folder::split(path);
    if !path.starts_with("/project/") {
        return match fenix_extension_api::folder_list(dir) {
            Ok(entries) => folder::pictures(entries.into_iter().map(|e| e.name)),
            Err(e) => {
                fenix_extension_api::warn!("folder-list: {e}");
                Vec::new()
            }
        };
    }
    let names = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned());
    folder::pictures(names)
}

/// `path`'s place among its folder's pictures.
fn place_of(path: &str) -> Option<(usize, usize)> {
    let list = folder_of(path);
    let name = folder::split(path).1;
    list.iter().position(|n| n == name).map(|i| (i + 1, list.len()))
}

/// The file a page kept in the session showed, read again. Under
/// `/project` it must be the same file: Fenix starts the extension again
/// in each project you move to, and `/project/a.png` there is another.
fn restored_bytes(saved: &Saved) -> Option<Vec<u8>> {
    if saved.path.starts_with("/project/") {
        let root = fenix_extension_api::here().and_then(|h| h.project).map(|p| p.root);
        let bytes = fenix_extension_api::read_file(&saved.path).ok();
        return match root {
            // Where you are says which project this is: the file's in it,
            // changed or not -- or it isn't.
            Some(root) => bytes.filter(|_| folder::under_project(&saved.full, &root).as_deref() == Some(saved.path.as_str())),
            // Nothing says: the same size, as a check.
            None => bytes.filter(|b| b.len() as u64 == saved.bytes),
        };
    }
    // Fenix gives it again the files its pages were given; one reached
    // with `]` it may read once its folder's listed again.
    fenix_extension_api::read_file(&saved.full).ok().or_else(|| {
        fenix_extension_api::folder_list(folder::split(&saved.full).0).ok()?;
        fenix_extension_api::read_file(&saved.full).ok()
    })
}

impl Images {
    fn colors(&mut self) -> Colors {
        *self.colors.get_or_insert_with(colors)
    }

    /// `full`, a file Fenix gave it (`opened`), opened in the pane you're
    /// in. One already open is opened again here, as it was shown.
    fn open(&mut self, full: &str) {
        let bytes = match fenix_extension_api::read_file(full) {
            Ok(b) => b,
            Err(e) => return say(Level::Error, &e),
        };
        let path = in_project(full, bytes.len()).unwrap_or_else(|| full.to_string());
        // A surface is in one pane at a time: the one open goes, its zoom
        // coming here.
        let mut kept = None;
        let open: Vec<u64> = self.viewers.iter().filter(|(_, v)| v.kind == IMAGE && (v.full == full || v.path == path)).map(|(&s, _)| s).collect();
        for s in open {
            if let Some(old) = self.viewers.get(&s) {
                let saved = old.view.saved();
                kept = Some((saved.0, saved.1, old.details));
            }
            page::close(s);
            self.forget(s);
        }
        let mut viewer = Viewer::new(IMAGE, full, path);
        let title = viewer.name().to_string();
        let surface = match surface::open(IMAGE, &title, &SurfaceOptions::new().scale(SurfaceScale::Fit)) {
            Ok(s) => s,
            Err(e) => return say(Level::Error, &format!("{full}: {e}")),
        };
        if let Some((zoom, center, details)) = kept {
            viewer.pending = Some((zoom, center));
            viewer.details = details;
        }
        self.free_hidden();
        self.fill(&mut viewer, bytes);
        self.viewers.insert(surface, viewer);
        self.last = Some(surface);
        self.play(surface);
        self.status();
    }

    /// `viewer` given `bytes`, its file's: decoded, its place in its
    /// folder found. A file that doesn't decode says why, in its pane and
    /// the message line.
    fn fill(&mut self, viewer: &mut Viewer, bytes: Vec<u8>) {
        viewer.load(bytes);
        viewer.place = place_of(&viewer.path);
        if let Some(why) = &viewer.error {
            say(Level::Warn, &format!("{}: {why}", viewer.name()));
        }
    }

    /// The pictures of the viewers out of sight let go: they're decoded
    /// again when they're shown, so two big photos in tabs don't take
    /// more memory than an extension has.
    fn free_hidden(&mut self) {
        for v in self.viewers.values_mut().filter(|v| !v.shown) {
            v.release();
        }
    }

    /// What it kept for surface `s` let go.
    fn forget(&mut self, s: u64) {
        if let Some(v) = self.viewers.remove(&s) {
            if let Some(t) = v.timer {
                host::cancel_timer(t);
                self.timers.remove(&t);
            }
        }
        if self.preview == Some(s) {
            self.preview = None;
        }
        if self.last == Some(s) {
            self.last = self.viewers.iter().find(|(_, v)| v.shown).map(|(&s, _)| s);
        }
    }

    /// Surface `s` drawn and shown.
    fn draw(&mut self, s: u64) {
        let colors = self.colors();
        let Some(v) = self.viewers.get_mut(&s) else { return };
        let (w, h, _) = v.pane;
        if let Some(frame) = v.draw(&colors) {
            let _ = surface::write(s, 0, 0, w, h, frame);
            surface::present(s);
        }
    }

    /// The modeline: the picture looked at last, while it's in sight.
    fn status(&self) {
        let text = self.last.and_then(|s| self.viewers.get(&s)).filter(|v| v.shown).map(|v| v.details_of().status());
        fenix_extension_api::status(text.as_deref());
    }

    /// Surface `s`'s picture playing, if it moves and is to.
    fn play(&mut self, s: u64) {
        let Some(v) = self.viewers.get_mut(&s) else { return };
        if !v.playing || !v.shown || v.timer.is_some() {
            return;
        }
        if let Some(ms) = v.delay() {
            let t = host::set_timer(ms, false);
            v.timer = Some(t);
            self.timers.insert(t, s);
        }
    }

    fn stop(&mut self, s: u64) {
        if let Some(t) = self.viewers.get_mut(&s).and_then(|v| v.timer.take()) {
            host::cancel_timer(t);
            self.timers.remove(&t);
        }
    }

    /// A moving picture's next frame, and the one after that asked for.
    fn tick(&mut self, timer: u64) {
        let Some(s) = self.timers.remove(&timer) else { return };
        let Some(v) = self.viewers.get_mut(&s) else { return };
        v.timer = None;
        if !v.playing || !v.shown {
            return;
        }
        let Some(p) = v.picture.as_mut() else { return };
        if let Err(e) = p.advance() {
            v.playing = false;
            fenix_extension_api::warn!("{}: {e}", v.name());
            return;
        }
        self.draw(s);
        self.play(s);
        self.status();
    }

    /// What a key or a command asks of surface `s`.
    fn act(&mut self, s: u64, action: Action) {
        let Some(v) = self.viewers.get_mut(&s) else { return };
        self.last = Some(s);
        match action {
            Action::Next | Action::Previous => return self.step(s, action == Action::Next),
            Action::Source => return self.source(s),
            Action::Reload => {
                let path = v.path.clone();
                match fenix_extension_api::read_file(&path) {
                    Ok(bytes) => {
                        let mut v = self.viewers.remove(&s).expect("it's there");
                        let saved = v.view.saved();
                        self.stop_timer_of(&mut v);
                        self.fill(&mut v, bytes);
                        v.pending = Some(saved);
                        let (w, h, dpi) = v.pane;
                        v.resized(w, h, dpi);
                        self.viewers.insert(s, v);
                        self.play(s);
                    }
                    Err(e) => say(Level::Error, &e),
                }
            }
            Action::Play => {
                if !v.moves() {
                    say(Level::Info, "it doesn't move");
                    return;
                }
                v.playing = !v.playing;
                if v.playing {
                    self.play(s);
                } else {
                    self.stop(s);
                }
            }
            Action::Frame => {
                if !v.moves() {
                    return;
                }
                v.playing = false;
                if let Some(Err(e)) = v.picture.as_mut().map(|p| p.advance()) {
                    say(Level::Warn, &e);
                }
                self.stop(s);
            }
            Action::Details => v.toggle_details(),
            view => v.act_view(view),
        }
        self.draw(s);
        self.status();
    }

    fn stop_timer_of(&mut self, v: &mut Viewer) {
        if let Some(t) = v.timer.take() {
            host::cancel_timer(t);
            self.timers.remove(&t);
        }
    }

    /// `]` and `[`: the next or previous picture in its folder, in the
    /// same pane.
    fn step(&mut self, s: u64, forward: bool) {
        let Some(v) = self.viewers.get(&s) else { return };
        let list = folder_of(&v.path);
        let Some(i) = folder::step(&list, v.name(), forward) else { return };
        if list.len() == 1 && list[0] == v.name() {
            return say(Level::Info, "the only picture in its folder");
        }
        let next = format!("{}/{}", folder::split(&v.path).0, list[i]);
        // The same folder, as a full path: for the session.
        let full = format!("{}/{}", folder::split(&v.full).0, list[i]);
        let bytes = match fenix_extension_api::read_file(&next) {
            Ok(b) => b,
            Err(e) => return say(Level::Error, &e),
        };
        let mut old = self.viewers.remove(&s).expect("it's there");
        self.stop_timer_of(&mut old);
        let mut v = Viewer::new(old.kind, &full, next.clone());
        v.details = old.details;
        v.resized(old.pane.0, old.pane.1, old.pane.2);
        self.fill(&mut v, bytes);
        surface::retitle(s, v.name());
        self.viewers.insert(s, v);
        self.play(s);
        self.draw(s);
        self.status();
    }

    /// `s`: an SVG's source, the file's own text, to edit: Fenix opens a
    /// file it opens itself as text when it asks. An SVGZ is compressed,
    /// so its text is a copy, read-only.
    fn source(&mut self, s: u64) {
        let Some(v) = self.viewers.get(&s) else { return };
        if v.format != Some(format::Format::Svg) {
            return say(Level::Info, &format!("s shows an SVG's source; {} is a {}", v.name(), v.format.map(|f| f.name()).unwrap_or("file it doesn't read")));
        }
        let bytes = match fenix_extension_api::read_file(&v.path) {
            Ok(b) => b,
            Err(e) => return say(Level::Error, &e),
        };
        if !bytes.starts_with(&[0x1f, 0x8b]) {
            return host::open_file(&v.path, None);
        }
        let bytes = resvg::usvg::decompress_svgz(&bytes).unwrap_or(bytes);
        fenix_extension_api::open_text(v.name(), &String::from_utf8_lossy(&bytes), Some("svg"));
    }

    /// `SPC f i`: the picture the line you're on names, beside it; again
    /// on the same one closes it.
    fn preview(&mut self) -> Result<(), String> {
        let here = fenix_extension_api::here().ok_or("SPC f i previews a picture named in the file you're in")?;
        let file = here.path.ok_or("this buffer isn't a file")?;
        // The one under the cursor first, when Fenix says where it is.
        let refs = match fenix_extension_api::cursor().filter(|c| !c.selected) {
            Some(c) => folder::picture_refs_at(&here.text, c.at.1 as usize),
            None => folder::picture_refs(&here.text),
        };
        if refs.is_empty() {
            return Err("no picture named on this line (a .png, .jpg, .gif, .webp, .bmp or .svg)".into());
        }
        let root = here.project.map(|p| p.root);
        let dir = folder::split(&file).0.to_string();
        let mut why = String::new();
        for r in &refs {
            let full = folder::resolve(&dir, r);
            let Some(path) = root.as_deref().and_then(|root| folder::under_project(&full, root)) else {
                why = format!("{r} isn't in the project: the preview reads pictures there");
                continue;
            };
            let bytes = match fenix_extension_api::read_file(&path) {
                Ok(b) => b,
                Err(_) => {
                    why = format!("no {r}");
                    continue;
                }
            };
            return self.show_preview(&full, path, bytes);
        }
        Err(why)
    }

    fn show_preview(&mut self, full: &str, path: String, bytes: Vec<u8>) -> Result<(), String> {
        if let Some(s) = self.preview {
            if self.viewers.get(&s).is_some_and(|v| v.path == path) {
                page::close(s);
                self.forget(s);
                self.status();
                return Ok(());
            }
        }
        let mut viewer = Viewer::new(PREVIEW, full, path);
        let surface = match self.preview.filter(|s| self.viewers.contains_key(s)) {
            Some(s) => {
                let old = self.viewers.remove(&s).expect("it's there");
                viewer.resized(old.pane.0, old.pane.1, old.pane.2);
                viewer.details = old.details;
                if let Some(t) = old.timer {
                    host::cancel_timer(t);
                    self.timers.remove(&t);
                }
                surface::retitle(s, viewer.name());
                s
            }
            None => surface::open(PREVIEW, viewer.name(), &SurfaceOptions::new().scale(SurfaceScale::Fit).beside(PREVIEW_WIDTH))?,
        };
        self.free_hidden();
        self.fill(&mut viewer, bytes);
        self.viewers.insert(surface, viewer);
        self.preview = Some(surface);
        self.last = Some(surface);
        self.play(surface);
        self.draw(surface);
        self.status();
        Ok(())
    }

    /// What a command asks of the picture looked at last.
    fn on_last(&mut self, action: Action) -> Result<(), String> {
        let s = self.last.filter(|s| self.viewers.contains_key(s)).ok_or("no picture open")?;
        self.act(s, action);
        Ok(())
    }

    fn shown(&mut self, s: u64, shown: bool) {
        let Some(v) = self.viewers.get_mut(&s) else { return };
        v.shown = shown;
        if !shown {
            self.stop(s);
            self.status();
            return;
        }
        self.last = Some(s);
        if v.picture.is_none() && v.error.is_none() {
            // Let go while it was out of sight: decoded again.
            let path = v.path.clone();
            if let Ok(bytes) = fenix_extension_api::read_file(&path) {
                self.free_hidden();
                let mut v = self.viewers.remove(&s).expect("it's there");
                let saved = v.view.saved();
                self.fill(&mut v, bytes);
                v.pending = Some(saved);
                let (w, h, dpi) = v.pane;
                v.resized(w, h, dpi);
                self.viewers.insert(s, v);
                self.draw(s);
            }
        }
        self.play(s);
        self.status();
    }
}

impl Extension for Images {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "images.preview" => self.preview(),
            "images.next" => self.on_last(Action::Next),
            "images.previous" => self.on_last(Action::Previous),
            "images.fit" => self.on_last(Action::Fit),
            "images.actual_size" => self.on_last(Action::Actual),
            "images.source" => self.on_last(Action::Source),
            "images.play" => self.on_last(Action::Play),
            "images.details" => self.on_last(Action::Details),
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Opened(path) => self.open(&path),
            Event::SurfaceResized(size) => {
                if let Some(v) = self.viewers.get_mut(&size.surface) {
                    let (w, h) = (size.width.clamp(1, 4096), size.height.clamp(1, 4096));
                    v.resized(w, h, size.scale);
                    if let Err(e) = surface::resize(size.surface, w, h) {
                        fenix_extension_api::warn!("surface-resize: {e}");
                    }
                    self.draw(size.surface);
                    self.status();
                }
            }
            Event::Timer(t) => self.tick(t),
            Event::PageShown((page, shown)) => self.shown(page, shown),
            Event::ThemeChanged => {
                self.colors = None;
                let shown: Vec<u64> = self.viewers.iter().filter(|(_, v)| v.shown).map(|(&s, _)| s).collect();
                for s in shown {
                    if let Some(v) = self.viewers.get_mut(&s) {
                        v.resized(v.pane.0, v.pane.1, v.pane.2);
                    }
                    self.draw(s);
                }
            }
            _ => {}
        }
    }

    fn surface_input(&mut self, s: u64, input: SurfaceInput) {
        if !self.viewers.contains_key(&s) {
            return;
        }
        match input {
            SurfaceInput::Key(key) if key.down && key.mods.none() => {
                let action = match key.key {
                    SurfaceKey::Char(c) => Action::of_key(c),
                    SurfaceKey::Named(NamedKey::Left) => Some(Action::Pan(-1, 0)),
                    SurfaceKey::Named(NamedKey::Right) => Some(Action::Pan(1, 0)),
                    SurfaceKey::Named(NamedKey::Up) => Some(Action::Pan(0, -1)),
                    SurfaceKey::Named(NamedKey::Down) => Some(Action::Pan(0, 1)),
                    _ => None,
                };
                if let Some(action) = action {
                    self.act(s, action);
                }
            }
            SurfaceInput::Pointer(p) => {
                let v = self.viewers.get_mut(&s).expect("it's there");
                if p.buttons & 1 == 0 {
                    v.drag = None;
                    return;
                }
                if let Some((x, y)) = v.drag.replace((p.x, p.y)) {
                    v.view.drag((p.x - x) as f32, (p.y - y) as f32);
                    self.draw(s);
                    self.status();
                }
            }
            SurfaceInput::Wheel(w) if w.lines != 0 => self.act(s, Action::ZoomAt(w.lines, (w.x as f32, w.y as f32))),
            _ => {}
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.viewers.get(&page).filter(|v| v.kind == IMAGE).map(Viewer::state)
    }

    fn page_restore(&mut self, _page: u64, kind: &str, state: &str) -> bool {
        if kind != IMAGE {
            return false;
        }
        let Ok(saved) = serde_json::from_str::<Saved>(state) else { return false };
        // Fenix gives it the files it opens, but not again on a new start:
        // what's in the project it reads from there, and a file elsewhere
        // only if Fenix still lets it.
        let Some(bytes) = restored_bytes(&saved) else { return false };
        let full = if saved.full.is_empty() { saved.path.clone() } else { saved.full.clone() };
        let mut viewer = Viewer::new(IMAGE, &full, saved.path);
        viewer.details = saved.details;
        viewer.pending = Some((saved.zoom, saved.center));
        let Ok(surface) = surface::open(IMAGE, viewer.name(), &SurfaceOptions::new().scale(SurfaceScale::Fit)) else { return false };
        self.fill(&mut viewer, bytes);
        self.viewers.insert(surface, viewer);
        self.last.get_or_insert(surface);
        self.play(surface);
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.forget(page);
        self.status();
    }
}

export!(Images);
