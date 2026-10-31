//! The reader: every document open, the surface each is read on, and the
//! companion program that renders them (`fenix-pdfium`); what Fenix says
//! to it, and what it asks of Fenix in turn -- through `Host`, so all of it
//! runs under a native test with a fake host and a fake companion
//! (`tests/`).
//!
//! A document is shown on a surface the size of its pane: one column of
//! pages (`layout::Layout`) scrolled to a place in it. What the surface
//! shows is drawn from the companion's renders of the parts of pages in
//! sight, a rectangle at a time; a scroll moves what's there
//! (`surface-copy`) and asks only for the strip that came into view. A
//! zoom, a resize, the colours changing or the file rewritten draws it all
//! again. Until a part comes, it's blank paper.

use std::collections::{BTreeMap, HashMap, HashSet};

use fenix_extension_api::{HomeMark, HomeRow, HomeSection, Key, Level, Page, PointerEvent, Role, SurfaceMark, SurfaceSize, WheelEvent};

use crate::font;
use crate::keys::{Cmd, Keys, Outcome, Press};
use crate::layout::{self, Layout, SavedPlace, Zoom};
use crate::protocol::{Frames, Heading, Link, Match, Reply, Request, Target, VERSION};
use crate::sidebar::{self, MatchRow, SideKey, Sidebar, Tab};
use crate::text::{self, Ch, Motion, TextPos};

pub type Rgb = (u8, u8, u8);

/// The companion program's name, as the manifest's `permissions.process`
/// lists it.
pub const COMPANION: &str = "fenix-pdfium";

/// How far `j`/`k`/`h`/`l` move, in pixels.
pub const PAN_STEP_PX: f32 = 60.0;
/// How far a line of the wheel scrolls, before the display's scale.
pub const WHEEL_STEP_PX: f32 = 40.0;
/// Renders asked for a view at once; what else came into view waits for
/// them, so a fast scroll doesn't queue up strips long gone.
const IN_FLIGHT: usize = 4;
/// The most rows one render is: a whole pane comes as a few bands, each
/// shown as it comes.
const BAND_ROWS: u32 = 256;
/// A page not rendered yet: blank paper.
pub const PAPER: Rgb = (232, 231, 227);
/// The files kept in its folder: where each PDF was left (1.x's own file,
/// taken over), and the PDFs read lately.
pub const PLACES_FILE: &str = "pdf_places.tsv";
pub const RECENT_FILE: &str = "recent.txt";
/// How many it remembers, and how many Home shows.
const RECENT_KEPT: usize = 50;
const HOME_ROWS: usize = 3;
/// How long after a change the places file is written.
const SAVE_AFTER_MILLIS: u32 = 2000;

/// What the reader asks of Fenix.
pub trait Host {
    /// Where `fenix-pdfium` is (`find-program`).
    fn find_companion(&mut self) -> Option<String>;
    /// Starts it, its stdout as bytes (`spawn-raw`).
    fn spawn_companion(&mut self, program: &str) -> Result<u64, String>;
    fn write_companion(&mut self, process: u64, bytes: &[u8]) -> Result<(), String>;
    fn kill(&mut self, process: u64);
    fn surface_open(&mut self, title: &str) -> Result<u64, String>;
    fn surface_resize(&mut self, surface: u64, width: u32, height: u32);
    fn surface_write(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]);
    fn surface_fill(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, rgb: Rgb);
    #[allow(clippy::too_many_arguments)]
    fn surface_copy(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32);
    fn surface_present(&mut self, surface: u64);
    fn surface_marks(&mut self, surface: u64, marks: &[SurfaceMark]);
    fn retitle(&mut self, page: u64, title: &str);
    /// A page of `kind`: in the focused pane, or beside it, `beside` of it.
    fn page_open(&mut self, kind: &str, title: &str, beside: Option<f32>) -> u64;
    fn page_refresh(&mut self, page: u64);
    fn page_close(&mut self, page: u64);
    fn say(&mut self, level: Level, message: &str);
    /// Its item in the modeline.
    fn status(&mut self, text: Option<&str>);
    fn prompt(&mut self, label: &str, initial: &str) -> u64;
    /// A picker of (label, detail, value).
    fn pick(&mut self, title: &str, items: &[(String, String, String)]) -> u64;
    fn copy(&mut self, text: &str);
    fn open_url(&mut self, url: &str) -> Result<(), String>;
    /// A file that isn't a PDF, from the shelf: opened by Fenix.
    fn open_file(&mut self, path: &str);
    fn setting(&mut self, key: &str) -> Option<String>;
    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String>;
    /// The editor theme's background and text.
    fn theme(&mut self) -> (Rgb, Rgb);
    fn home(&mut self, section: Option<&HomeSection>);
    /// The project you were in (`editor-here`), as a full path.
    fn project_root(&mut self) -> Option<String>;
    fn answer(&mut self, call: u64, result: Result<String, String>);
    fn timer(&mut self, millis: u32) -> u64;
    /// One of its files in `/data`.
    fn load(&mut self, name: &str) -> Option<String>;
    fn save(&mut self, name: &str, text: &str);
}

/// A place in a document: a page, and how far down it (0..1).
pub type Place = (u32, f32);

/// Where a document is to open, once it has.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenAt {
    Here,
    /// Page n, from 1.
    Page(u32),
    /// The heading whose title holds this.
    Heading(String),
}

/// What a fetched outline is for.
#[derive(Debug, Clone, PartialEq)]
enum OutlineFor {
    Sidebar,
    Picker,
    /// To go to the heading whose title holds this text.
    Find(String),
}

/// A search of a document: the one under way or last made.
#[derive(Debug, Default)]
struct Search {
    id: u64,
    query: String,
    matches: Vec<Match>,
    done: bool,
    /// The match `n` last went to.
    current: Option<usize>,
    /// Whether the matches are highlighted (`Esc` hides them).
    highlights: bool,
    /// `Enter` before the first match came: go to the first at or after
    /// this page when it does.
    jump_when_found: Option<u32>,
}

/// One open document.
struct Doc {
    path: String,
    name: String,
    /// Every page's size in points; empty until it's opened.
    pages: Vec<(f32, f32)>,
    opened: bool,
    outline: Option<Vec<Heading>>,
    outline_asked: bool,
    outline_for: Vec<OutlineFor>,
    text: HashMap<u32, Vec<Ch>>,
    links: HashMap<u32, Vec<Link>>,
    asked_text: HashSet<u32>,
    asked_links: HashSet<u32>,
    search: Search,
    marks: BTreeMap<char, Place>,
    /// Where it was left: the page at the pane's top, how far down it, the
    /// zoom.
    last_place: Place,
    last_zoom: Zoom,
    /// Its surface, when it's shown on one.
    view: Option<u64>,
    /// Where it's to open, once it has.
    open_at: Option<OpenAt>,
    /// Opened only to be searched for Fenix (`pdf.search`): the call, and
    /// what's been found.
    serving: Option<(u64, Vec<Match>)>,
}

impl Doc {
    fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }
}

/// What the surface holds: the layout it was drawn for (an epoch), and
/// where it was scrolled to.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drawn {
    epoch: u64,
    scroll: (f32, f32),
}

/// A part of a page to be rendered: its page, and the rectangle of it in
/// the page's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Part {
    page: u32,
    rect: (u32, u32, u32, u32),
}

/// Link hints showing: each link's label, page and index in that page's
/// links, what's been typed, and where each label was drawn.
struct Hints {
    links: Vec<(String, u32, usize)>,
    typed: String,
    drawn: Vec<(u32, u32, u32, u32)>,
}

/// A document shown on a surface.
struct View {
    doc: u64,
    title: String,
    zoom: Zoom,
    /// Its pane in pixels and the display's scale; (0, 0) until it's told.
    pane: (u32, u32),
    dpi: f32,
    layout: Layout,
    layout_for: Option<LayoutFor>,
    /// Bumped when the layout or the colours change: everything's drawn
    /// again, and renders asked for before are let go.
    epoch: u64,
    /// The first render id of this epoch.
    epoch_from: u64,
    drawn: Option<Drawn>,
    /// Top-left of what's shown, in the layout's pixels, whole.
    scroll: (f32, f32),
    pending_jump: Option<Place>,
    shown: bool,
    in_flight: usize,
    dirty: Vec<Part>,
    /// Where jumps came from and, after `Ctrl-o`, went to.
    back: Vec<Place>,
    forward: Vec<Place>,
    selection: Option<(TextPos, TextPos)>,
    /// `v`: the keys move the selection's end and `y` copies it.
    visual: bool,
    visual_waiting: Option<u32>,
    /// The left button's down, selecting.
    dragging: bool,
    drag_from: Option<(u32, (f32, f32))>,
    hints: Option<Hints>,
    /// The page last told to the outline, so it follows the reading.
    told_page: u32,
}

/// What a view was laid out for: its zoom, pane, scale, page count and
/// gap (the floats by their bits).
type LayoutFor = (Zoom, (u32, u32), u32, usize, u32);

/// A render to ask for: its page, the page's size in pixels, and the
/// rectangle of it.
type Ask = (u32, (u32, u32), (u32, u32, u32, u32));

/// The outline sidebar, while it's open: its page, the surface it's for.
struct SidebarPage {
    page: u64,
    view: u64,
    side: Sidebar,
    /// Opened in the document's own pane (`reader.sidebar = "over"`): it
    /// closes when it takes you somewhere.
    over: bool,
}

/// What a prompt or a picker of its is for.
#[derive(Debug, Clone, PartialEq)]
enum Asked {
    Search(u64),
    GotoPage(u64),
    Heading(u64),
    Shelf,
}

#[derive(Default)]
pub struct Reader {
    /// The companion's process, and whether it said hello.
    companion: Option<(u64, bool)>,
    frames: Frames,
    next_id: u64,
    docs: BTreeMap<u64, Doc>,
    views: BTreeMap<u64, View>,
    /// The view used last: what `SPC r` acts on.
    last_view: Option<u64>,
    keys: Keys,
    keys_view: Option<u64>,
    places: HashMap<String, SavedPlace>,
    places_dirty: bool,
    save_timer: Option<u64>,
    recent: Vec<String>,
    loaded: bool,
    sidebar: Option<SidebarPage>,
    asked: HashMap<u64, Asked>,
    /// Why the companion couldn't run, said once.
    missing: Option<String>,
}

fn intersect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> Option<(f32, f32, f32, f32)> {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// `bgra` in the theme's colours: white becomes `paper`, black `ink`, and
/// a colour keeps its hue -- as Fenix 1.x's shader drew them.
pub fn recolor(bgra: &mut [u8], paper: Rgb, ink: Rgb) {
    let (p, k) = ([paper.0 as f32, paper.1 as f32, paper.2 as f32], [ink.0 as f32, ink.1 as f32, ink.2 as f32]);
    for px in bgra.as_chunks_mut::<4>().0 {
        let (r, g, b) = (px[2] as f32, px[1] as f32, px[0] as f32);
        let light = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;
        let out = |c: f32, i: usize| (k[i] + (p[i] - k[i]) * light + (c - light * 255.0)).round().clamp(0.0, 255.0) as u8;
        px[2] = out(r, 0);
        px[1] = out(g, 1);
        px[0] = out(b, 2);
    }
}

/// The JSON a service call or a page's state carries.
#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
struct OpenCall {
    path: String,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    heading: Option<String>,
}

#[derive(serde::Deserialize)]
struct SearchCall {
    path: String,
    query: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
struct ViewState {
    path: String,
    page: u32,
    frac: f32,
    zoom: String,
}

impl Reader {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // -- Settings, places, the shelf --------------------------------------

    fn remembers(&self, host: &mut dyn Host) -> bool {
        host.setting("reader.remember").is_none_or(|v| v.trim() != "false")
    }

    fn default_zoom(&self, host: &mut dyn Host) -> Zoom {
        host.setting("reader.zoom").and_then(|z| layout::parse_zoom(&z)).unwrap_or(layout::DEFAULT_ZOOM)
    }

    fn gap(&self, host: &mut dyn Host) -> f32 {
        host.setting("reader.page_gap").and_then(|g| g.trim().parse::<f32>().ok()).map(|g| g.clamp(0.0, 40.0)).unwrap_or(layout::PAGE_GAP)
    }

    /// The colours pages are drawn in (`reader.colors = "theme"`): what
    /// white and black become.
    fn recoloring(&self, host: &mut dyn Host) -> Option<(Rgb, Rgb)> {
        (host.setting("reader.colors").as_deref() == Some("theme")).then(|| host.theme())
    }

    /// Blank paper, and the gaps around pages.
    fn colors(&self, host: &mut dyn Host) -> (Rgb, Rgb) {
        let (bg, _) = host.theme();
        match self.recoloring(host) {
            // Pages in the theme's colours are its background, so the
            // gaps go a shade darker.
            Some((paper, _)) => (paper, ((bg.0 as f32 * 0.72) as u8, (bg.1 as f32 * 0.72) as u8, (bg.2 as f32 * 0.72) as u8)),
            None => (PAPER, bg),
        }
    }

    /// Its files read, the first time it needs them.
    fn load(&mut self, host: &mut dyn Host) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.places = layout::parse_places(&host.load(PLACES_FILE).unwrap_or_default());
        self.recent = host.load(RECENT_FILE).unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    }

    /// At start: Home's section.
    pub fn activate(&mut self, host: &mut dyn Host) {
        self.load(host);
        self.refresh_home(host);
    }

    fn place_of(&self, path: &str) -> Option<&SavedPlace> {
        self.places.iter().find(|(p, _)| layout::same_path(p, path)).map(|(_, place)| place)
    }

    /// Notes where `doc` is, for the places file, written a moment later.
    fn store_place(&mut self, host: &mut dyn Host, doc: u64) {
        if !self.remembers(host) {
            return;
        }
        let Some(d) = self.docs.get(&doc).filter(|d| d.serving.is_none() && !d.pages.is_empty()) else { return };
        let place = SavedPlace { page: d.last_place.0, frac: d.last_place.1, zoom: d.last_zoom, pages: d.page_count(), marks: d.marks.clone() };
        let key = self.places.keys().find(|p| layout::same_path(p, &d.path)).cloned().unwrap_or_else(|| d.path.clone());
        if self.places.get(&key) != Some(&place) {
            self.places.insert(key, place);
            self.places_dirty = true;
            if self.save_timer.is_none() {
                self.save_timer = Some(host.timer(SAVE_AFTER_MILLIS));
            }
        }
    }

    fn save_places(&mut self, host: &mut dyn Host) {
        self.save_timer = None;
        if std::mem::take(&mut self.places_dirty) {
            host.save(PLACES_FILE, &layout::format_places(&self.places));
            self.refresh_home(host);
        }
    }

    /// `path` read just now: first among the recent.
    fn note_recent(&mut self, host: &mut dyn Host, path: &str) {
        self.recent.retain(|p| !layout::same_path(p, path));
        self.recent.insert(0, path.to_string());
        self.recent.truncate(RECENT_KEPT);
        host.save(RECENT_FILE, &(self.recent.join("\n") + "\n"));
        self.refresh_home(host);
    }

    /// "p. 38 / 212", where `path` was left.
    fn where_left(&self, path: &str) -> String {
        self.place_of(path).map(|p| format!("p. {} / {}", p.page + 1, p.pages)).unwrap_or_default()
    }

    /// Home's Reading: the PDFs read lately, with where you were in each.
    fn refresh_home(&mut self, host: &mut dyn Host) {
        let rows: Vec<HomeRow> = self.recent.iter().take(HOME_ROWS).map(|path| HomeRow { name: layout::file_name(path), detail: self.where_left(path), mark: HomeMark::None, value: path.clone() }).collect();
        if rows.is_empty() {
            host.home(None);
        } else {
            host.home(Some(&HomeSection { title: "reading".into(), key: "SPC r f".into(), rows, most: HOME_ROWS as u32 }));
        }
    }

    /// `SPC r f`'s list, in order: the shelf (`reader.documents`, a
    /// project's own when it has one), then the PDFs read lately. A
    /// document is listed once, in the first group it's in.
    pub fn shelf(&self, host: &mut dyn Host) -> Vec<(String, String, String)> {
        let mut list: Vec<(String, String, String)> = Vec::new();
        let add = |list: &mut Vec<(String, String, String)>, label: String, detail: String, path: String| {
            if !list.iter().any(|(_, _, p)| layout::same_path(p, &path)) {
                list.push((label, detail, path));
            }
        };
        let root = host.project_root();
        let documents: Vec<(String, String)> = host.setting("reader.documents").and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default();
        for (name, path) in documents {
            // A project's entries may be relative to it.
            let full = match &root {
                Some(root) if !(path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':')) => format!("{}/{}", root.trim_end_matches(['/', '\\']), path),
                _ => path,
            };
            let detail = self.where_left(&full);
            add(&mut list, name, detail, full);
        }
        for path in &self.recent {
            add(&mut list, format!("recent \u{b7} {}", layout::file_name(path)), self.where_left(path), path.clone());
        }
        list
    }

    pub fn open_shelf(&mut self, host: &mut dyn Host) {
        self.load(host);
        let items = self.shelf(host);
        if items.is_empty() {
            host.say(Level::Error, "no documents yet -- add them in SPC , (reader.documents), or open a PDF and it's listed here");
            return;
        }
        let id = host.pick("Documents", &items);
        self.asked.insert(id, Asked::Shelf);
    }

    // -- The companion ----------------------------------------------------

    /// The companion, started if it isn't: false (and said why) when it
    /// can't be.
    fn companion(&mut self, host: &mut dyn Host) -> Option<u64> {
        if let Some((process, _)) = self.companion {
            return Some(process);
        }
        let started = match host.find_companion() {
            Some(program) => host.spawn_companion(&program),
            None => Err(format!("{COMPANION} isn't beside Fenix -- it comes with Fenix's own release; a build of Fenix needs `cargo build -p {COMPANION}` (docs/BUILDING.md)")),
        };
        match started {
            Ok(process) => {
                self.companion = Some((process, false));
                self.frames = Frames::default();
                self.missing = None;
                Some(process)
            }
            Err(why) => {
                if self.missing.as_deref() != Some(why.as_str()) {
                    host.say(Level::Error, &why);
                    self.missing = Some(why);
                }
                None
            }
        }
    }

    fn send(&mut self, host: &mut dyn Host, request: Request) {
        let Some(process) = self.companion(host) else { return };
        if let Err(e) = host.write_companion(process, &request.encode()) {
            host.say(Level::Error, &format!("{COMPANION}: {e}"));
        }
    }

    /// What the companion wrote.
    pub fn companion_bytes(&mut self, host: &mut dyn Host, process: u64, bytes: &[u8]) {
        if self.companion.map(|(p, _)| p) != Some(process) {
            return;
        }
        let bodies = match self.frames.push(bytes) {
            Ok(bodies) => bodies,
            Err(e) => {
                host.say(Level::Error, &format!("{COMPANION}: {e}"));
                host.kill(process);
                return;
            }
        };
        for body in bodies {
            match Reply::decode(&body) {
                Ok(reply) => self.reply(host, reply),
                Err(e) => host.say(Level::Warn, &format!("{COMPANION}: {e}")),
            }
        }
    }

    /// The companion ended: what it had open is opened again the next
    /// time it's needed.
    pub fn companion_ended(&mut self, host: &mut dyn Host, process: u64, code: Option<i32>) {
        if self.companion.map(|(p, _)| p) != Some(process) {
            return;
        }
        self.companion = None;
        let shown: Vec<u64> = self.views.iter().filter(|(_, v)| v.shown).map(|(&s, _)| s).collect();
        for doc in self.docs.values_mut() {
            doc.opened = false;
        }
        for view in self.views.values_mut() {
            view.in_flight = 0;
        }
        host.say(Level::Error, &format!("{COMPANION} stopped ({}) -- the PDFs open are read again", code.map(|c| c.to_string()).unwrap_or_else(|| "no exit code".into())));
        // The ones in sight, opened again now.
        for surface in shown {
            if let Some(doc) = self.views.get(&surface).map(|v| v.doc) {
                self.reopen(host, doc);
            }
        }
    }

    fn reopen(&mut self, host: &mut dyn Host, doc: u64) {
        let Some(path) = self.docs.get(&doc).map(|d| d.path.clone()) else { return };
        self.send(host, Request::Open { doc, path });
    }

    fn reply(&mut self, host: &mut dyn Host, reply: Reply) {
        match reply {
            Reply::Hello { version } => {
                if let Some(c) = &mut self.companion {
                    c.1 = true;
                }
                if version != VERSION {
                    host.say(Level::Error, &format!("{COMPANION} speaks protocol {version}, this reader {VERSION} -- update the PDF reader and Fenix together"));
                }
            }
            Reply::Opened { doc, pages } => self.opened(host, doc, pages),
            Reply::OpenFailed { doc, message } => {
                let Some(d) = self.docs.get(&doc) else { return };
                if let Some((call, _)) = d.serving {
                    self.docs.remove(&doc);
                    host.answer(call, Err(message));
                    return;
                }
                let path = d.path.clone();
                let hint = if message.contains("pdfium library not found") { " (docs/BUILDING.md says where PDFium goes)" } else { "" };
                host.say(Level::Error, &format!("couldn't open {path}: {message}{hint}"));
                if let Some(surface) = d.view {
                    self.draw(host, surface);
                }
            }
            Reply::Rendered { doc, view, id, page, x, y, w, h, bgra } => self.rendered(host, doc, view, id, Part { page, rect: (x, y, w, h) }, bgra),
            Reply::RenderFailed { doc, id, message } => {
                let surface = self.docs.get(&doc).and_then(|d| d.view);
                if let Some(v) = surface.and_then(|s| self.views.get_mut(&s)).filter(|v| id >= v.epoch_from) {
                    v.in_flight = v.in_flight.saturating_sub(1);
                    host.say(Level::Error, &format!("couldn't render a page: {message}"));
                }
            }
            Reply::Outline { doc, headings } => self.outline_came(host, doc, headings),
            Reply::Text { doc, page, chars } => self.text_came(host, doc, page, chars.into_iter().map(|(c, rect)| Ch { c, rect }).collect()),
            Reply::Links { doc, page, links } => {
                if let Some(d) = self.docs.get_mut(&doc) {
                    d.links.insert(page, links);
                }
            }
            Reply::Found { doc, id, done, matches } => self.found(host, doc, id, done, matches),
            Reply::Changed { doc } => self.changed(host, doc),
        }
    }

    // -- Opening ----------------------------------------------------------

    /// Opens `path` on a surface in the pane you're in -- or brings it there
    /// when it's open -- at `at`.
    pub fn open(&mut self, host: &mut dyn Host, path: &str, at: OpenAt) {
        self.load(host);
        let existing = self.docs.iter().find(|(_, d)| d.serving.is_none() && layout::same_path(&d.path, path)).map(|(&id, _)| id);
        let doc = match existing {
            Some(doc) => {
                self.bring_here(host, doc);
                doc
            }
            None => {
                let doc = self.new_doc(host, path);
                let title = self.docs[&doc].name.clone();
                match host.surface_open(&title) {
                    Ok(surface) => self.attach(host, doc, surface),
                    Err(e) => host.say(Level::Error, &format!("{path}: {e}")),
                }
                self.send(host, Request::Open { doc, path: path.to_string() });
                doc
            }
        };
        self.note_recent(host, path);
        self.go(host, doc, at);
    }

    /// A document for `path`, starting where it was left.
    fn new_doc(&mut self, host: &mut dyn Host, path: &str) -> u64 {
        let doc = self.id();
        let saved = if self.remembers(host) { self.place_of(path).cloned() } else { None };
        let default_zoom = self.default_zoom(host);
        self.docs.insert(
            doc,
            Doc {
                path: path.to_string(),
                name: layout::file_name(path),
                pages: Vec::new(),
                opened: false,
                outline: None,
                outline_asked: false,
                outline_for: Vec::new(),
                text: HashMap::new(),
                links: HashMap::new(),
                asked_text: HashSet::new(),
                asked_links: HashSet::new(),
                search: Search { done: true, ..Default::default() },
                marks: saved.as_ref().map(|p| p.marks.clone()).unwrap_or_default(),
                last_place: saved.as_ref().map(|p| (p.page, p.frac)).unwrap_or((0, 0.0)),
                last_zoom: saved.map(|p| p.zoom).unwrap_or(default_zoom),
                view: None,
                open_at: None,
                serving: None,
            },
        );
        doc
    }

    /// `doc` shown on `surface`, starting where it was left.
    fn attach(&mut self, host: &mut dyn Host, doc: u64, surface: u64) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.view = Some(surface);
        let title = d.name.clone();
        let view = View {
            doc,
            title,
            zoom: d.last_zoom,
            pane: (0, 0),
            dpi: 1.0,
            layout: Layout::default(),
            layout_for: None,
            epoch: 0,
            epoch_from: 0,
            drawn: None,
            scroll: (0.0, 0.0),
            pending_jump: Some(d.last_place),
            shown: true,
            in_flight: 0,
            dirty: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            selection: None,
            visual: false,
            visual_waiting: None,
            dragging: false,
            drag_from: None,
            hints: None,
            told_page: u32::MAX,
        };
        self.views.insert(surface, view);
        self.last_view = Some(surface);
        self.show_status(host);
    }

    /// `doc`'s surface brought to the pane you're in: a new one in place of
    /// the one it had, which Fenix 0.8 gives no other way to bring here,
    /// drawn afresh where it was.
    fn bring_here(&mut self, host: &mut dyn Host, doc: u64) {
        let title = self.docs.get(&doc).map(|d| d.name.clone()).unwrap_or_default();
        let old = self.docs.get(&doc).and_then(|d| d.view);
        let surface = match host.surface_open(&title) {
            Ok(surface) => surface,
            Err(e) => return host.say(Level::Error, &e),
        };
        if let Some(old) = old {
            host.page_close(old);
            if let Some(mut view) = self.views.remove(&old) {
                view.drawn = None;
                view.pane = (0, 0);
                view.layout_for = None;
                view.in_flight = 0;
                view.dirty.clear();
                view.shown = true;
                self.views.insert(surface, view);
                if let Some(d) = self.docs.get_mut(&doc) {
                    d.view = Some(surface);
                }
                if let Some(side) = &mut self.sidebar {
                    if side.view == old {
                        side.view = surface;
                    }
                }
                self.last_view = Some(surface);
                return;
            }
        }
        self.attach(host, doc, surface);
        if !self.docs.get(&doc).is_some_and(|d| d.opened) {
            self.reopen(host, doc);
        }
    }

    /// Goes to `at` in `doc`: now, or once it's opened.
    fn go(&mut self, host: &mut dyn Host, doc: u64, at: OpenAt) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if !d.opened {
            if at != OpenAt::Here {
                d.open_at = Some(at);
            }
            return;
        }
        let Some(surface) = d.view else { return };
        match at {
            OpenAt::Here => {}
            OpenAt::Page(n) => self.jump(host, surface, (n.saturating_sub(1), 0.0)),
            OpenAt::Heading(heading) => {
                if self.docs.get(&doc).is_some_and(|d| d.outline.is_some()) {
                    self.find_heading(host, doc, &heading);
                } else {
                    self.want_outline(host, doc, OutlineFor::Find(heading));
                }
            }
        }
    }

    fn opened(&mut self, host: &mut dyn Host, doc: u64, pages: Vec<(f32, f32)>) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.last_place.0 = d.last_place.0.min((pages.len() as u32).saturating_sub(1));
        d.pages = pages;
        d.opened = true;
        // Searched for Fenix: the search, at once.
        if d.serving.is_some() {
            let query = d.search.query.clone();
            let id = d.search.id;
            let match_case = query.chars().any(char::is_uppercase);
            self.send(host, Request::Search { doc, id, query, match_case });
            return;
        }
        let at = d.open_at.take();
        if let Some(surface) = d.view {
            if let Some(view) = self.views.get_mut(&surface) {
                view.layout_for = None;
            }
            self.draw(host, surface);
        }
        if let Some(at) = at {
            self.go(host, doc, at);
        }
        self.store_place(host, doc);
        self.show_status(host);
    }

    /// The file was written again (a build, a download): read again, each
    /// view keeping its place.
    fn changed(&mut self, host: &mut dyn Host, doc: u64) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.outline = None;
        d.outline_asked = false;
        d.search = Search { done: true, ..Default::default() };
        d.text.clear();
        d.links.clear();
        d.asked_text.clear();
        d.asked_links.clear();
        d.opened = false;
        let (path, name) = (d.path.clone(), d.name.clone());
        self.send(host, Request::Close { doc });
        self.send(host, Request::Open { doc, path });
        host.say(Level::Info, &format!("{name} changed on disk -- reloaded"));
    }

    /// The PDF at `path` searched for Fenix (`pdf.search`): answered when
    /// every page has been.
    pub fn serve_search(&mut self, host: &mut dyn Host, call: u64, payload: &str) -> Option<Result<String, String>> {
        let call_args: SearchCall = match serde_json::from_str(payload) {
            Ok(c) => c,
            Err(e) => return Some(Err(format!("pdf.search: {e}"))),
        };
        let doc = self.new_doc(host, &call_args.path);
        let id = self.id();
        if let Some(d) = self.docs.get_mut(&doc) {
            d.serving = Some((call, Vec::new()));
            d.search = Search { id, query: call_args.query, ..Default::default() };
        }
        if self.companion(host).is_none() {
            self.docs.remove(&doc);
            return Some(Err(self.missing.clone().unwrap_or_else(|| format!("{COMPANION} couldn't start"))));
        }
        self.send(host, Request::Open { doc, path: call_args.path });
        None
    }

    /// Fenix's `pdf.open`: a file at a page or a heading.
    pub fn serve_open(&mut self, host: &mut dyn Host, payload: &str) -> Result<String, String> {
        let call: OpenCall = serde_json::from_str(payload).map_err(|e| format!("pdf.open: {e}"))?;
        let at = match (call.page, call.heading) {
            (_, Some(h)) if !h.trim().is_empty() => OpenAt::Heading(h),
            (Some(p), _) => OpenAt::Page(p),
            _ => OpenAt::Here,
        };
        self.open(host, &call.path, at);
        Ok(String::new())
    }

    // -- Pages: their state and the session -------------------------------

    /// What a surface needs to open again: its document, and where it was.
    pub fn page_state(&self, surface: u64) -> Option<String> {
        let view = self.views.get(&surface)?;
        let doc = self.docs.get(&view.doc)?;
        let (page, frac) = self.place(surface).unwrap_or(doc.last_place);
        serde_json::to_string(&ViewState { path: doc.path.clone(), page, frac, zoom: layout::zoom_text(view.zoom) }).ok()
    }

    /// A surface from the last session, opened again in its place.
    pub fn page_restore(&mut self, host: &mut dyn Host, kind: &str, state: &str) -> bool {
        if kind != "pdf" {
            return false;
        }
        let Ok(state) = serde_json::from_str::<ViewState>(state) else { return false };
        self.load(host);
        let doc = self.new_doc(host, &state.path);
        if let Some(d) = self.docs.get_mut(&doc) {
            d.last_place = (state.page, state.frac);
            if let Some(zoom) = layout::parse_zoom(&state.zoom) {
                d.last_zoom = zoom;
            }
        }
        let title = self.docs[&doc].name.clone();
        match host.surface_open(&title) {
            Ok(surface) => self.attach(host, doc, surface),
            Err(_) => {
                self.docs.remove(&doc);
                return false;
            }
        }
        self.send(host, Request::Open { doc, path: state.path });
        true
    }

    /// One of its pages came into view, or went out of every pane.
    pub fn shown(&mut self, host: &mut dyn Host, page: u64, shown: bool) {
        if let Some(view) = self.views.get_mut(&page) {
            view.shown = shown;
            if shown {
                self.last_view = Some(page);
            }
            self.show_status(host);
            return;
        }
        // The outline closed (`q`) or hidden: let go of it.
        if !shown && self.sidebar.as_ref().is_some_and(|s| s.page == page) {
            self.sidebar = None;
            host.page_close(page);
        }
    }

    /// You closed one of its pages: what it kept for it goes.
    pub fn closed(&mut self, host: &mut dyn Host, page: u64) {
        if self.sidebar.as_ref().is_some_and(|s| s.page == page) {
            self.sidebar = None;
            return;
        }
        let Some(view) = self.views.remove(&page) else { return };
        if self.last_view == Some(page) {
            self.last_view = None;
        }
        self.store_place(host, view.doc);
        if self.docs.remove(&view.doc).is_some() {
            self.send(host, Request::Close { doc: view.doc });
        }
        self.show_status(host);
    }

    // -- Layout and drawing -----------------------------------------------

    /// Lays the view out again if its zoom, pane, scale or document
    /// changed, keeping the same place at the pane's top; then applies a
    /// pending jump and keeps the scroll in bounds and whole.
    fn relayout(&mut self, host: &mut dyn Host, surface: u64) {
        let gap = self.gap(host);
        let Some(view) = self.views.get_mut(&surface) else { return };
        let Some(doc) = self.docs.get(&view.doc) else { return };
        if view.pane.0 == 0 || view.pane.1 == 0 || doc.pages.is_empty() {
            return;
        }
        let pane = (view.pane.0 as f32, view.pane.1 as f32);
        let inputs = (view.zoom, view.pane, view.dpi.to_bits(), doc.pages.len(), gap.to_bits());
        if view.layout_for != Some(inputs) {
            let old = std::mem::take(&mut view.layout);
            let anchor = (!old.pages.is_empty()).then(|| old.anchor(view.scroll.1));
            let x_ratio = if old.width > 0.0 { (view.scroll.0 + pane.0 / 2.0) / old.width } else { 0.5 };
            view.layout = layout::layout(&doc.pages, view.zoom, pane, view.dpi, gap);
            view.layout_for = Some(inputs);
            if let Some(anchor) = anchor {
                view.scroll.1 = view.layout.scroll_for(anchor);
            }
            view.scroll.0 = x_ratio * view.layout.width - pane.0 / 2.0;
            view.epoch += 1;
        }
        if let Some((page, frac)) = view.pending_jump.take() {
            let page = (page as usize).min(doc.pages.len() - 1);
            view.scroll.1 = if frac > 0.0 { view.layout.scroll_for((page, frac)) } else { view.layout.top_of(page) };
        }
        let (max_x, max_y) = view.layout.max_scroll(pane);
        view.scroll = (view.scroll.0.clamp(0.0, max_x).round(), view.scroll.1.clamp(0.0, max_y).round());
    }

    /// Everything for the surface after a change: laid out, what moved
    /// moved, what came into view blank paper and asked for, the marks over
    /// it, the hints' labels, shown.
    fn draw(&mut self, host: &mut dyn Host, surface: u64) {
        self.relayout(host, surface);
        let (paper, gap) = self.colors(host);
        let Some(view) = self.views.get(&surface) else { return };
        let (w, h) = view.pane;
        if w == 0 || h == 0 {
            return;
        }
        let opened = self.docs.get(&view.doc).is_some_and(|d| !d.pages.is_empty());
        if !opened || view.layout.pages.is_empty() {
            host.surface_fill(surface, 0, 0, w, h, gap);
            host.surface_present(surface);
            return;
        }
        let (epoch, scroll, doc, drawn) = (view.epoch, view.scroll, view.doc, view.drawn);
        let mut exposed: Vec<(f32, f32, f32, f32)> = Vec::new();
        match drawn {
            Some(drawn) if drawn.epoch == epoch => {
                let (dx, dy) = ((scroll.0 - drawn.scroll.0) as i64, (scroll.1 - drawn.scroll.1) as i64);
                let (wi, hi) = (w as i64, h as i64);
                if dx == 0 && dy == 0 {
                } else if dx.abs() < wi && dy.abs() < hi {
                    // What's still in sight moves; the strips it left are new.
                    let (cw, ch) = ((wi - dx.abs()) as u32, (hi - dy.abs()) as u32);
                    host.surface_copy(surface, dx.max(0) as u32, dy.max(0) as u32, cw, ch, (-dx).max(0) as u32, (-dy).max(0) as u32);
                    if dy > 0 {
                        exposed.push((0.0, (hi - dy) as f32, w as f32, dy as f32));
                    } else if dy < 0 {
                        exposed.push((0.0, 0.0, w as f32, (-dy) as f32));
                    }
                    if dx > 0 {
                        exposed.push(((wi - dx) as f32, 0.0, dx as f32, h as f32));
                    } else if dx < 0 {
                        exposed.push((0.0, 0.0, (-dx) as f32, h as f32));
                    }
                } else {
                    exposed.push((0.0, 0.0, w as f32, h as f32));
                }
            }
            // Laid out anew, or never drawn at this size: everything, and
            // what was asked for before is let go.
            _ => {
                let before = self.next_id + 1;
                if let Some(view) = self.views.get_mut(&surface) {
                    view.epoch_from = before;
                    view.in_flight = 0;
                    view.dirty.clear();
                }
                self.send(host, Request::CancelBefore { doc, view: surface, before });
                exposed.push((0.0, 0.0, w as f32, h as f32));
            }
        }
        for area in exposed {
            self.paint(host, surface, area, paper, gap);
        }
        if let Some(view) = self.views.get_mut(&surface) {
            view.drawn = Some(Drawn { epoch, scroll });
        }
        self.flush(host, surface);
        self.want_in_sight(host, surface);
        self.draw_hints(host, surface);
        self.marks(host, surface);
        host.surface_present(surface);
        self.after_move(host, surface);
    }

    /// `area` of the pane (its pixels) as blank paper on gaps, and its
    /// pages' parts to be rendered.
    fn paint(&mut self, host: &mut dyn Host, surface: u64, area: (f32, f32, f32, f32), paper: Rgb, gap: Rgb) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        let (ax, ay, aw, ah) = area;
        host.surface_fill(surface, ax as u32, ay as u32, aw as u32, ah as u32, gap);
        // In the layout's pixels.
        let doc_area = (ax + view.scroll.0, ay + view.scroll.1, aw, ah);
        for page in view.layout.visible(doc_area.1, doc_area.3) {
            let p = view.layout.pages[page];
            let Some((ix, iy, iw, ih)) = intersect(doc_area, (p.x, p.y, p.w, p.h)) else { continue };
            host.surface_fill(surface, (ix - view.scroll.0) as u32, (iy - view.scroll.1) as u32, iw as u32, ih as u32, paper);
            view.dirty.push(Part { page: page as u32, rect: ((ix - p.x) as u32, (iy - p.y) as u32, iw as u32, ih as u32) });
        }
    }

    /// Asks for the parts of pages waiting to be rendered, as far as there's
    /// room: a page's parts as one rectangle, cut to what's still in sight,
    /// in bands.
    fn flush(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        if view.dirty.is_empty() || view.in_flight >= IN_FLIGHT {
            return;
        }
        let mut by_page: BTreeMap<u32, (u32, u32, u32, u32)> = BTreeMap::new();
        for part in view.dirty.drain(..) {
            let (x, y, w, h) = part.rect;
            by_page
                .entry(part.page)
                .and_modify(|r| {
                    let (x0, y0) = (r.0.min(x), r.1.min(y));
                    let (x1, y1) = ((r.0 + r.2).max(x + w), (r.1 + r.3).max(y + h));
                    *r = (x0, y0, x1 - x0, y1 - y0);
                })
                .or_insert(part.rect);
        }
        let sight = (view.scroll.0, view.scroll.1, view.pane.0 as f32, view.pane.1 as f32);
        let mut asks: Vec<Ask> = Vec::new();
        for (page, (x, y, w, h)) in by_page {
            let Some(p) = view.layout.pages.get(page as usize).copied() else { continue };
            let Some((ix, iy, iw, ih)) = intersect((p.x + x as f32, p.y + y as f32, w as f32, h as f32), sight) else { continue };
            let (rx, ry, rw, rh) = ((ix - p.x) as u32, (iy - p.y) as u32, iw as u32, ih as u32);
            let mut band = 0;
            while band < rh {
                let bh = BAND_ROWS.min(rh - band);
                asks.push((page, p.px(), (rx, ry + band, rw, bh)));
                band += bh;
            }
        }
        let doc = view.doc;
        let room = IN_FLIGHT - view.in_flight;
        // What doesn't fit now waits its turn.
        let (now, later) = asks.split_at(asks.len().min(room));
        for &(page, _, rect) in later {
            view.dirty.push(Part { page, rect });
        }
        view.in_flight += now.len();
        let now = now.to_vec();
        for (page, (page_w, page_h), (x, y, w, h)) in now {
            let id = self.id();
            self.send(host, Request::Render { doc, view: surface, id, page, page_w, page_h, x, y, w, h });
        }
    }

    fn rendered(&mut self, host: &mut dyn Host, doc: u64, surface: u64, id: u64, part: Part, mut bgra: Vec<u8>) {
        let Part { page, rect: (x, y, w, h) } = part;
        let recolor = self.recoloring(host);
        let Some(view) = self.views.get_mut(&surface) else { return };
        if view.doc != doc || id < view.epoch_from {
            return;
        }
        view.in_flight = view.in_flight.saturating_sub(1);
        if let (Some(p), true) = (view.layout.pages.get(page as usize).copied(), bgra.len() == (w as usize * h as usize * 4)) {
            let sight = (view.scroll.0, view.scroll.1, view.pane.0 as f32, view.pane.1 as f32);
            if let Some((ix, iy, iw, ih)) = intersect((p.x + x as f32, p.y + y as f32, w as f32, h as f32), sight) {
                if let Some((paper, ink)) = recolor {
                    recolor_into(&mut bgra, paper, ink);
                }
                let (sx, sy) = ((ix - p.x) as u32 - x, (iy - p.y) as u32 - y);
                let (iw, ih) = (iw as u32, ih as u32);
                let part: Vec<u8> = if (sx, sy, iw, ih) == (0, 0, w, h) {
                    bgra
                } else {
                    let mut out = Vec::with_capacity((iw * ih * 4) as usize);
                    for row in sy..sy + ih {
                        let at = ((row * w + sx) * 4) as usize;
                        out.extend_from_slice(&bgra[at..at + (iw * 4) as usize]);
                    }
                    out
                };
                host.surface_write(surface, (ix - view.scroll.0) as u32, (iy - view.scroll.1) as u32, iw, ih, &part);
            }
        }
        self.flush(host, surface);
        self.draw_hints(host, surface);
        host.surface_present(surface);
    }

    /// The links and text of the pages in sight, asked for, so a click, `f`
    /// or a drag finds them without waiting.
    fn want_in_sight(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get(&surface) else { return };
        let (doc, visible) = (view.doc, view.layout.visible(view.scroll.1, view.pane.1 as f32));
        for page in visible {
            self.want_links(host, doc, page as u32);
            self.want_text(host, doc, page as u32);
        }
    }

    fn want_text(&mut self, host: &mut dyn Host, doc: u64, page: u32) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if d.text.contains_key(&page) || !d.asked_text.insert(page) {
            return;
        }
        self.send(host, Request::Text { doc, page });
    }

    fn want_links(&mut self, host: &mut dyn Host, doc: u64, page: u32) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if d.links.contains_key(&page) || !d.asked_links.insert(page) {
            return;
        }
        self.send(host, Request::Links { doc, page });
    }

    /// A box in points on `page` as the pane's pixels, cut to it.
    fn to_screen(view: &View, page: u32, r: &[f32; 4]) -> Option<(f32, f32, f32, f32)> {
        let p = view.layout.pages.get(page as usize)?;
        let s = view.layout.scale;
        let rect = (p.x + r[0] * s - view.scroll.0, p.y + r[1] * s - view.scroll.1, (r[2] - r[0]) * s, (r[3] - r[1]) * s);
        intersect(rect, (0.0, 0.0, view.pane.0 as f32, view.pane.1 as f32))
    }

    /// The boxes tinted over the pages: the selection, the search's
    /// matches (the current one brighter), the links while hints show.
    fn marks(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get(&surface) else { return };
        let Some(doc) = self.docs.get(&view.doc) else { return };
        let mut marks: Vec<SurfaceMark> = Vec::new();
        let mut push = |r: Option<(f32, f32, f32, f32)>, role: Role| {
            if let Some((x, y, w, h)) = r {
                marks.push(SurfaceMark::new(x.round() as i32, y.round() as i32, w.round().max(1.0) as u32, h.round().max(1.0) as u32, role));
            }
        };
        if let Some((a, b)) = view.selection {
            let (from, to) = (a.min(b), a.max(b));
            for page in from.page..=to.page {
                let Some(chars) = doc.text.get(&page) else { continue };
                let start = if page == from.page { from.idx } else { 0 };
                let end = if page == to.page { to.idx } else { chars.len().saturating_sub(1) };
                for r in text::line_rects(chars, start, end) {
                    push(Self::to_screen(view, page, &r), Role::Accent);
                }
            }
        }
        if doc.search.highlights {
            let visible = view.layout.visible(view.scroll.1, view.pane.1 as f32);
            for (i, m) in doc.search.matches.iter().enumerate().filter(|(_, m)| visible.contains(&(m.page as usize))) {
                let role = if doc.search.current == Some(i) { Role::Bad } else { Role::Warn };
                for r in &m.rects {
                    push(Self::to_screen(view, m.page, r), role.clone());
                }
            }
        }
        if let Some(hints) = &view.hints {
            for (label, page, at) in &hints.links {
                if !label.starts_with(&hints.typed) {
                    continue;
                }
                if let Some(link) = doc.links.get(page).and_then(|l| l.get(*at)) {
                    push(Self::to_screen(view, *page, &link.rect), Role::Good);
                }
            }
        }
        host.surface_marks(surface, &marks);
    }

    /// The hints' labels, drawn into the pixels over their links.
    fn draw_hints(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get(&surface) else { return };
        let Some(hints) = &view.hints else { return };
        let Some(doc) = self.docs.get(&view.doc) else { return };
        let scale = (view.dpi * 2.0).round().max(1.0) as u32;
        let mut drawn = Vec::new();
        for (label, page, at) in &hints.links {
            if !label.starts_with(&hints.typed) {
                continue;
            }
            let Some(link) = doc.links.get(page).and_then(|l| l.get(*at)) else { continue };
            let Some((x, y, _, _)) = Self::to_screen(view, *page, &link.rect) else { continue };
            let (lw, lh, bgra) = font::label(label, scale);
            let (x, y) = (x as u32, y as u32);
            if x + lw > view.pane.0 || y + lh > view.pane.1 {
                continue;
            }
            host.surface_write(surface, x, y, lw, lh, &bgra);
            drawn.push((x, y, lw, lh));
        }
        if let Some(h) = self.views.get_mut(&surface).and_then(|v| v.hints.as_mut()) {
            h.drawn = drawn;
        }
    }

    /// After the view moved: where it is noted, the outline following it,
    /// the modeline.
    fn after_move(&mut self, host: &mut dyn Host, surface: u64) {
        self.remember_place(host, surface);
        let current = self.current_page(surface);
        if let Some(view) = self.views.get_mut(&surface) {
            if view.told_page != current {
                view.told_page = current;
                if let Some(side) = self.sidebar.as_ref().filter(|s| s.view == surface) {
                    host.page_refresh(side.page);
                }
            }
        }
        self.show_status(host);
    }

    // -- Where it is ------------------------------------------------------

    /// The page the view is reading: the one across the middle of its pane.
    pub fn current_page(&self, surface: u64) -> u32 {
        match self.views.get(&surface) {
            Some(view) if !view.layout.pages.is_empty() => view.layout.current(view.scroll.1, (view.pane.0 as f32, view.pane.1 as f32)) as u32,
            Some(view) => view.pending_jump.map(|(p, _)| p).or_else(|| self.docs.get(&view.doc).map(|d| d.last_place.0)).unwrap_or(0),
            None => 0,
        }
    }

    /// Where the pane's top is, as a place to come back to.
    fn place(&self, surface: u64) -> Option<Place> {
        let view = self.views.get(&surface)?;
        if let Some(place) = view.pending_jump {
            return Some(place);
        }
        let l = &view.layout;
        (!l.pages.is_empty()).then(|| {
            let page = l.page_at(view.scroll.1);
            // In the gap above the next page: that page, at its top -- as a
            // jump to it puts it.
            if view.scroll.1 >= l.pages[page].y + l.pages[page].h && page + 1 < l.pages.len() {
                return (page as u32 + 1, 0.0);
            }
            let (page, frac) = l.anchor(view.scroll.1);
            (page as u32, frac)
        })
    }

    fn remember_place(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(place) = self.place(surface) else { return };
        let Some((doc, zoom)) = self.views.get(&surface).map(|v| (v.doc, v.zoom)) else { return };
        if let Some(d) = self.docs.get_mut(&doc) {
            d.last_place = place;
            d.last_zoom = zoom;
        }
        self.store_place(host, doc);
    }

    /// The view the commands act on: the one used last while it's shown,
    /// else one that's shown.
    pub fn target(&self) -> Option<u64> {
        self.last_view.filter(|s| self.views.get(s).is_some_and(|v| v.shown)).or_else(|| self.views.iter().find(|(_, v)| v.shown).map(|(&s, _)| s)).or(self.last_view.filter(|s| self.views.contains_key(s)))
    }

    /// The modeline's page, zoom and search, for the view used last.
    pub fn status_text(&self) -> Option<String> {
        let surface = self.target()?;
        let view = self.views.get(&surface)?;
        if !view.shown {
            return None;
        }
        let doc = self.docs.get(&view.doc)?;
        let pending = if self.keys.is_pending() && self.keys_view == Some(surface) { format!("{}  ", self.keys.pending_text()) } else { String::new() };
        let s = &doc.search;
        let search = match (s.highlights, s.current) {
            (true, Some(at)) => format!("  /{} {}/{}", s.query, at + 1, s.matches.len()),
            (true, None) if !s.query.is_empty() => format!("  /{} {}", s.query, s.matches.len()),
            _ => String::new(),
        };
        Some(if doc.pages.is_empty() {
            format!("{pending}PDF opening\u{2026}")
        } else {
            format!("{pending}PDF p. {}/{}  {}{search}", self.current_page(surface) + 1, doc.page_count(), layout::zoom_label(view.zoom))
        })
    }

    fn show_status(&self, host: &mut dyn Host) {
        host.status(self.status_text().as_deref());
    }

    // -- The surface's size and input -------------------------------------

    pub fn resized(&mut self, host: &mut dyn Host, size: SurfaceSize) {
        let Some(view) = self.views.get_mut(&size.surface) else { return };
        let pane = (size.width.clamp(1, 4096), size.height.clamp(1, 4096));
        if view.pane == pane && view.dpi == size.scale {
            return;
        }
        view.pane = pane;
        view.dpi = size.scale.max(0.1);
        view.drawn = None;
        host.surface_resize(size.surface, pane.0, pane.1);
        self.draw(host, size.surface);
    }

    /// A key on a document's surface.
    pub fn key(&mut self, host: &mut dyn Host, surface: u64, press: Press) {
        if !self.views.contains_key(&surface) {
            return;
        }
        if self.keys_view != Some(surface) {
            self.keys.reset();
            self.keys_view = Some(surface);
        }
        self.last_view = Some(surface);
        if self.views.get(&surface).is_some_and(|v| v.hints.is_some()) {
            return self.hint_key(host, surface, press);
        }
        if self.views.get(&surface).is_some_and(|v| v.visual) {
            return self.visual_key(host, surface, press);
        }
        match self.keys.key(press) {
            Outcome::Run(cmd) => self.run(host, surface, cmd),
            Outcome::Pending | Outcome::Dropped | Outcome::Pass => self.show_status(host),
        }
    }

    pub fn run(&mut self, host: &mut dyn Host, surface: u64, cmd: Cmd) {
        let pane_h = self.views.get(&surface).map(|v| v.pane.1 as f32).filter(|&h| h > 0.0).unwrap_or(400.0);
        match cmd {
            Cmd::Scroll(n) => self.scroll(host, surface, n as f32 * PAN_STEP_PX),
            Cmd::HalfScreen(n) => self.scroll(host, surface, n as f32 * (pane_h / 2.0).round()),
            Cmd::Screen(n) => self.scroll(host, surface, n as f32 * (pane_h - PAN_STEP_PX).max(PAN_STEP_PX)),
            Cmd::Pan(n) => self.pan(host, surface, n as f32 * PAN_STEP_PX),
            Cmd::Page(n) => self.turn_page(host, surface, n),
            Cmd::GotoPage(n) => self.jump(host, surface, (n.saturating_sub(1), 0.0)),
            Cmd::LastPage => {
                let count = self.views.get(&surface).and_then(|v| self.docs.get(&v.doc)).map(|d| d.page_count()).unwrap_or(1);
                self.jump(host, surface, (count.saturating_sub(1), 0.0));
            }
            Cmd::ZoomIn => self.zoom_step(host, surface, true),
            Cmd::ZoomOut => self.zoom_step(host, surface, false),
            Cmd::ToggleFit => {
                let fit_width = self.views.get(&surface).is_some_and(|v| v.zoom == Zoom::FitWidth);
                self.set_zoom(host, surface, if fit_width { Zoom::FitPage } else { Zoom::FitWidth });
            }
            Cmd::FitWidth => self.set_zoom(host, surface, Zoom::FitWidth),
            Cmd::FitPage => self.set_zoom(host, surface, Zoom::FitPage),
            Cmd::ActualSize => self.set_zoom(host, surface, Zoom::Percent(100)),
            Cmd::Outline => self.toggle_outline(host, surface),
            Cmd::Search => {
                let id = host.prompt("search pdf", "");
                self.asked.insert(id, Asked::Search(surface));
            }
            Cmd::NextMatch => self.step_match(host, surface, true),
            Cmd::PrevMatch => self.step_match(host, surface, false),
            Cmd::SetMark(c) => self.set_mark(host, surface, c),
            Cmd::GotoMark(c) => self.goto_mark(host, surface, c),
            Cmd::Escape => self.escape(host, surface),
            Cmd::CopyLink => self.copy_link(host, surface),
            Cmd::Visual => self.start_visual(host, surface),
            Cmd::Hints => self.show_hints(host, surface),
            Cmd::JumpBack => self.jump_history(host, surface, true),
            Cmd::JumpForward => self.jump_history(host, surface, false),
        }
    }

    pub fn wheel(&mut self, host: &mut dyn Host, surface: u64, wheel: WheelEvent) {
        self.last_view = Some(surface);
        if wheel.mods.ctrl {
            if wheel.lines > 0 {
                self.zoom_step(host, surface, true);
            } else if wheel.lines < 0 {
                self.zoom_step(host, surface, false);
            }
            return;
        }
        let dpi = self.views.get(&surface).map(|v| v.dpi).unwrap_or(1.0);
        self.scroll(host, surface, -(wheel.lines as f32) * (WHEEL_STEP_PX * dpi).round());
    }

    /// The pointer: a press on a link follows it; a drag selects text, and
    /// leaves it selected for `y`.
    pub fn pointer(&mut self, host: &mut dyn Host, surface: u64, p: PointerEvent) {
        let Some(view) = self.views.get(&surface) else { return };
        self.last_view = Some(surface);
        let left = p.buttons & 1 != 0;
        let pos = (p.x as f32, p.y as f32);
        match (left, view.dragging) {
            (true, false) => self.press(host, surface, pos),
            (true, true) => self.drag(host, surface, pos),
            (false, true) => self.release(host, surface),
            (false, false) => {}
        }
    }

    /// Where a point of the pane is on a page: the page, and points from
    /// its top-left.
    fn point_at(&self, surface: u64, local: (f32, f32)) -> Option<(u32, (f32, f32))> {
        let view = self.views.get(&surface)?;
        if view.layout.pages.is_empty() {
            return None;
        }
        let (x, y) = (local.0 + view.scroll.0, local.1 + view.scroll.1);
        let page = view.layout.page_at(y);
        let p = view.layout.pages[page];
        Some((page as u32, ((x - p.x) / view.layout.scale, (y - p.y) / view.layout.scale)))
    }

    fn press(&mut self, host: &mut dyn Host, surface: u64, local: (f32, f32)) {
        let Some((page, pt)) = self.point_at(surface, local) else { return };
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        let link = self.docs.get(&doc).and_then(|d| d.links.get(&page)).and_then(|links| links.iter().find(|l| pt.0 >= l.rect[0] && pt.0 <= l.rect[2] && pt.1 >= l.rect[1] && pt.1 <= l.rect[3]).cloned());
        if let Some(link) = link {
            // Taken by the link: the release that follows selects nothing.
            if let Some(view) = self.views.get_mut(&surface) {
                view.dragging = true;
                view.selection = None;
                view.drag_from = None;
            }
            return self.follow(host, surface, &link.to);
        }
        let at = self.docs.get(&doc).and_then(|d| d.text.get(&page)).and_then(|chars| text::char_at(chars, pt));
        if let Some(view) = self.views.get_mut(&surface) {
            view.visual = false;
            view.selection = at.map(|idx| (TextPos { page, idx }, TextPos { page, idx }));
            view.dragging = true;
            view.drag_from = at.is_none().then_some((page, pt));
        }
        self.want_text(host, doc, page);
        self.marks(host, surface);
        host.surface_present(surface);
    }

    fn drag(&mut self, host: &mut dyn Host, surface: u64, local: (f32, f32)) {
        let Some((page, pt)) = self.point_at(surface, local) else { return };
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        let Some(idx) = self.docs.get(&doc).and_then(|d| d.text.get(&page)).and_then(|chars| text::char_at(chars, pt)) else {
            self.want_text(host, doc, page);
            return;
        };
        let head = TextPos { page, idx };
        // A drag begun before its page's text came starts now.
        let from = self.views.get(&surface).and_then(|v| v.drag_from);
        let anchor = from.and_then(|(fp, fpt)| self.docs.get(&doc).and_then(|d| d.text.get(&fp)).and_then(|c| text::char_at(c, fpt)).map(|idx| TextPos { page: fp, idx }));
        if let Some(view) = self.views.get_mut(&surface) {
            if let Some(anchor) = anchor {
                view.selection = Some((anchor, head));
                view.drag_from = None;
            } else if let Some((anchor, _)) = view.selection {
                view.selection = Some((anchor, head));
            }
        }
        self.marks(host, surface);
        host.surface_present(surface);
    }

    fn release(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        view.dragging = false;
        view.drag_from = None;
        match view.selection {
            Some((a, b)) if a != b => view.visual = true,
            _ => view.selection = None,
        }
        let visual = view.visual;
        if visual {
            host.say(Level::Info, "selected -- y copies, Esc clears");
        }
        self.marks(host, surface);
        host.surface_present(surface);
    }

    // -- Moving -----------------------------------------------------------

    pub fn scroll(&mut self, host: &mut dyn Host, surface: u64, delta: f32) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        let max_y = view.layout.max_scroll((view.pane.0 as f32, view.pane.1 as f32)).1;
        view.scroll.1 = (view.scroll.1 + delta).clamp(0.0, max_y).round();
        self.draw(host, surface);
    }

    fn pan(&mut self, host: &mut dyn Host, surface: u64, delta: f32) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        let max_x = view.layout.max_scroll((view.pane.0 as f32, view.pane.1 as f32)).0;
        view.scroll.0 = (view.scroll.0 + delta).clamp(0.0, max_x).round();
        self.draw(host, surface);
    }

    /// `J`/`K`, `SPC r n`/`SPC r p`: `delta` pages on from the one being
    /// read, at its top.
    pub fn turn_page(&mut self, host: &mut dyn Host, surface: u64, delta: i32) {
        let current = self.current_page(surface) as i64;
        self.show(host, surface, ((current + delta as i64).max(0) as u32, 0.0));
    }

    /// Shows `place` at the top of the pane (a page past the end is the
    /// last). Before the view's laid out, it goes there once it is.
    fn show(&mut self, host: &mut dyn Host, surface: u64, place: Place) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        let count = self.docs.get(&view.doc).map(|d| d.page_count()).unwrap_or(0);
        let page = if count > 0 { place.0.min(count - 1) } else { place.0 };
        view.pending_jump = Some((page, place.1));
        if let Some(d) = self.docs.get_mut(&view.doc) {
            d.last_place.0 = page;
        }
        self.draw(host, surface);
    }

    /// `show`, remembering where it came from for `Ctrl-o`.
    pub fn jump(&mut self, host: &mut dyn Host, surface: u64, place: Place) {
        if let Some(from) = self.place(surface) {
            if let Some(view) = self.views.get_mut(&surface) {
                view.back.push(from);
                view.forward.clear();
            }
        }
        self.show(host, surface, place);
    }

    fn jump_history(&mut self, host: &mut dyn Host, surface: u64, back: bool) {
        let here = self.place(surface);
        let Some(view) = self.views.get_mut(&surface) else { return };
        let (from, to) = if back { (&mut view.back, &mut view.forward) } else { (&mut view.forward, &mut view.back) };
        let Some(place) = from.pop() else {
            return host.say(Level::Info, if back { "no jump to go back from" } else { "no jump to go forward to" });
        };
        if let Some(here) = here {
            to.push(here);
        }
        self.show(host, surface, place);
    }

    // -- Zoom -------------------------------------------------------------

    fn zoom_step(&mut self, host: &mut dyn Host, surface: u64, in_: bool) {
        let Some(view) = self.views.get(&surface) else { return };
        let current = match view.zoom {
            Zoom::Percent(p) => p,
            _ if view.layout.scale > 0.0 => (view.layout.scale / view.dpi.max(0.1) * 100.0).round() as u32,
            _ => 100,
        };
        self.set_zoom(host, surface, Zoom::Percent(layout::step_zoom(current, in_)));
    }

    pub fn set_zoom(&mut self, host: &mut dyn Host, surface: u64, zoom: Zoom) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        view.zoom = zoom;
        self.draw(host, surface);
    }

    // -- Marks ------------------------------------------------------------

    fn set_mark(&mut self, host: &mut dyn Host, surface: u64, c: char) {
        let Some(place) = self.place(surface) else { return };
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        if let Some(d) = self.docs.get_mut(&doc) {
            d.marks.insert(c, place);
        }
        self.store_place(host, doc);
        self.refresh_sidebar(host);
        host.say(Level::Info, &format!("mark {c} on page {}", place.0 + 1));
    }

    fn goto_mark(&mut self, host: &mut dyn Host, surface: u64, c: char) {
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        match self.docs.get(&doc).and_then(|d| d.marks.get(&c).copied()) {
            Some(place) => self.jump(host, surface, place),
            None => host.say(Level::Info, &format!("no mark {c} in this document -- m{c} sets one")),
        }
    }

    /// `Esc`: clears a selection, else hides the search's highlights, else
    /// closes the outline.
    fn escape(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get_mut(&surface) else { return };
        if view.selection.take().is_some() {
            view.visual = false;
        } else if let Some(d) = self.docs.get_mut(&view.doc).filter(|d| d.search.highlights) {
            d.search.highlights = false;
        } else if let Some(side) = self.sidebar.take() {
            host.page_close(side.page);
        }
        self.marks(host, surface);
        host.surface_present(surface);
        self.show_status(host);
    }

    // -- The outline ------------------------------------------------------

    /// `o`, `SPC r o`: the outline on a page beside the document (or over
    /// it, `reader.sidebar`), or closed.
    pub fn toggle_outline(&mut self, host: &mut dyn Host, surface: u64) {
        if let Some(side) = self.sidebar.take() {
            host.page_close(side.page);
            if side.view == surface {
                return;
            }
        }
        let Some(view) = self.views.get(&surface) else { return };
        let (doc, title) = (view.doc, view.title.clone());
        // As 1.x: beside the pages when there's room for 70 columns of them.
        let wide = view.pane.0 as f32 / view.dpi.max(0.1) >= 560.0 || view.pane.0 == 0;
        let over = match host.setting("reader.sidebar").as_deref() {
            Some("over") => true,
            Some("beside") => false,
            _ => !wide,
        };
        let page = host.page_open("outline", &format!("Outline: {title}"), (!over).then_some(0.3));
        let mut side = Sidebar::open(Tab::Outline);
        let rows = self.sidebar_rows(&side, surface);
        side.move_to_here(&rows);
        self.sidebar = Some(SidebarPage { page, view: surface, side, over });
        self.want_outline(host, doc, OutlineFor::Sidebar);
    }

    fn refresh_sidebar(&self, host: &mut dyn Host) {
        if let Some(side) = &self.sidebar {
            host.page_refresh(side.page);
        }
    }

    fn sidebar_rows(&self, side: &Sidebar, surface: u64) -> Vec<sidebar::Row> {
        let Some(view) = self.views.get(&surface) else { return Vec::new() };
        let Some(doc) = self.docs.get(&view.doc) else { return Vec::new() };
        let outline = doc.outline.as_deref().unwrap_or(&[]);
        let matches: Vec<MatchRow> = doc.search.matches.iter().map(|m| MatchRow { page: m.page, context: m.context.clone() }).collect();
        let marks: Vec<(char, u32)> = doc.marks.iter().map(|(&c, &(page, _))| (c, page)).collect();
        side.rows(outline, &matches, &marks, self.current_page(surface), doc.search.current)
    }

    /// The outline's page, laid out.
    pub fn sidebar_page(&self, page: u64, width: u32) -> Option<Page> {
        let side = self.sidebar.as_ref().filter(|s| s.page == page)?;
        let rows = self.sidebar_rows(&side.side, side.view);
        let doc = self.views.get(&side.view).and_then(|v| self.docs.get(&v.doc))?;
        let header = side.side.header(doc.search.matches.len(), doc.marks.len(), !doc.search.done);
        let empty = match (side.side.tab(), &doc.outline) {
            (Tab::Outline, None) => "loading the outline\u{2026}",
            (Tab::Outline, Some(_)) => "no bookmarks -- Tab for matches",
            (Tab::Matches, _) => "no matches -- / searches",
            (Tab::Marks, _) => "no marks -- m{a} sets one",
        };
        Some(side.side.page(width, &rows, &header, empty))
    }

    /// A key on the outline's page.
    pub fn sidebar_key(&mut self, host: &mut dyn Host, page: u64, key: Key) {
        let Some(mut side) = self.sidebar.take().filter(|s| s.page == page) else { return };
        let rows = self.sidebar_rows(&side.side, side.view);
        let outline = self.views.get(&side.view).and_then(|v| self.docs.get(&v.doc)).and_then(|d| d.outline.clone()).unwrap_or_default();
        let result = side.side.key(key, &rows, &outline);
        let (surface, over) = (side.view, side.over);
        match result {
            SideKey::Close => {
                host.page_close(page);
                return;
            }
            SideKey::Go(target) => {
                self.sidebar = Some(side);
                match target {
                    sidebar::Target::Page(p) => self.jump(host, surface, (p, 0.0)),
                    sidebar::Target::Match(at) => {
                        if let Some(doc) = self.views.get(&surface).map(|v| v.doc) {
                            self.go_to_match(host, doc, at);
                        }
                    }
                    sidebar::Target::Mark(c) => self.goto_mark(host, surface, c),
                }
                // Over the document: it's shown again.
                if over {
                    if let Some(side) = self.sidebar.take() {
                        host.page_close(side.page);
                    }
                }
                return;
            }
            SideKey::Taken | SideKey::Pass => {}
        }
        self.sidebar = Some(side);
        host.page_refresh(page);
    }

    /// Asks for `doc`'s outline if it hasn't come yet, and says what for.
    fn want_outline(&mut self, host: &mut dyn Host, doc: u64, what: OutlineFor) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if d.outline.is_some() {
            let headings = d.outline.clone().unwrap_or_default();
            return self.outline_for(host, doc, what, &headings);
        }
        d.outline_for.push(what);
        if !std::mem::replace(&mut d.outline_asked, true) {
            self.send(host, Request::Outline { doc });
        }
    }

    fn outline_came(&mut self, host: &mut dyn Host, doc: u64, headings: Vec<Heading>) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.outline = Some(headings.clone());
        for what in std::mem::take(&mut d.outline_for) {
            self.outline_for(host, doc, what, &headings);
        }
    }

    fn outline_for(&mut self, host: &mut dyn Host, doc: u64, what: OutlineFor, headings: &[Heading]) {
        match what {
            OutlineFor::Sidebar => {
                if headings.is_empty() {
                    host.say(Level::Info, "this PDF has no bookmarks");
                }
                // To the section being read.
                if let Some(side) = self.sidebar.take() {
                    let rows = self.sidebar_rows(&side.side, side.view);
                    let mut side = side;
                    side.side.move_to_here(&rows);
                    host.page_refresh(side.page);
                    self.sidebar = Some(side);
                }
            }
            OutlineFor::Picker => {
                if headings.is_empty() {
                    return host.say(Level::Info, "this PDF has no bookmarks");
                }
                let Some(surface) = self.docs.get(&doc).and_then(|d| d.view) else { return };
                let items: Vec<(String, String, String)> = headings.iter().map(|h| (format!("{}{}", "  ".repeat(h.depth as usize), h.title), format!("p.{}", h.page + 1), h.page.to_string())).collect();
                let id = host.pick("Headings", &items);
                self.asked.insert(id, Asked::Heading(surface));
            }
            OutlineFor::Find(heading) => self.find_heading(host, doc, &heading),
        }
    }

    /// `SPC r t`: a picker over the document's headings.
    pub fn pick_heading(&mut self, host: &mut dyn Host, surface: u64) {
        if let Some(doc) = self.views.get(&surface).map(|v| v.doc) {
            self.want_outline(host, doc, OutlineFor::Picker);
        }
    }

    /// To the heading whose title holds `heading` -- a field's clause in a
    /// standard -- through the outline, else through a search.
    fn find_heading(&mut self, host: &mut dyn Host, doc: u64, heading: &str) {
        let want = heading.to_lowercase();
        let page = self.docs.get(&doc).and_then(|d| d.outline.as_ref()).and_then(|o| o.iter().find(|e| e.title.to_lowercase().contains(&want)).map(|e| e.page));
        let Some(surface) = self.docs.get(&doc).and_then(|d| d.view) else { return };
        match page {
            Some(page) => {
                self.jump(host, surface, (page, 0.0));
                host.say(Level::Info, &format!("{heading} -- p. {}", page + 1));
            }
            None => {
                self.search(host, surface, heading.to_string(), true);
                host.say(Level::Info, &format!("no heading named \"{heading}\" -- searching for it"));
            }
        }
    }

    // -- Search -----------------------------------------------------------

    /// Searches the document for `query`, from the start, in place of the
    /// last search; smart case. With `go`, to the first match at or after
    /// the page being read, now or when it's found.
    pub fn search(&mut self, host: &mut dyn Host, surface: u64, query: String, go: bool) {
        let current = self.current_page(surface);
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        let id = self.id();
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.search = Search { id, query: query.clone(), done: query.trim().is_empty(), highlights: true, jump_when_found: go.then_some(current), ..Default::default() };
        if query.trim().is_empty() {
            return;
        }
        let match_case = query.chars().any(char::is_uppercase);
        self.send(host, Request::Search { doc, id, query, match_case });
        self.refresh_sidebar(host);
        self.show_status(host);
    }

    fn found(&mut self, host: &mut dyn Host, doc: u64, id: u64, done: bool, matches: Vec<Match>) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if id != d.search.id {
            return;
        }
        // Searched for Fenix: answered once every page has been.
        if let Some((call, found)) = &mut d.serving {
            found.extend(matches);
            if !done {
                return;
            }
            let call = *call;
            let hits: Vec<serde_json::Value> = found.iter().map(|m| serde_json::json!({ "page": m.page, "context": m.context })).collect();
            self.docs.remove(&doc);
            self.send(host, Request::Close { doc });
            host.answer(call, Ok(serde_json::Value::Array(hits).to_string()));
            return;
        }
        d.search.matches.extend(matches);
        d.search.done = done;
        let pending = d.search.jump_when_found.and_then(|from| d.search.matches.iter().position(|m| m.page >= from));
        let mut go = None;
        if let Some(at) = pending {
            d.search.jump_when_found = None;
            go = Some(at);
        } else if done && d.search.jump_when_found.is_some() && !d.search.matches.is_empty() {
            d.search.jump_when_found = None;
            go = Some(0);
        }
        let (count, query, jump_waited) = (d.search.matches.len(), d.search.query.clone(), d.search.jump_when_found.is_some());
        if done && jump_waited {
            d.search.jump_when_found = None;
        }
        if let Some(at) = go {
            self.go_to_match(host, doc, at);
        } else if done {
            if count == 0 {
                host.say(Level::Info, &format!("no matches for \"{query}\""));
            } else {
                host.say(Level::Info, &format!("{count} match{} for \"{query}\" -- n and N go through them", if count == 1 { "" } else { "es" }));
            }
        }
        if let Some(surface) = self.docs.get(&doc).and_then(|d| d.view) {
            self.marks(host, surface);
            host.surface_present(surface);
        }
        self.refresh_sidebar(host);
        self.show_status(host);
    }

    /// `n`/`N`: the next (or previous) match, going round the end. With no
    /// match gone to yet, the first after the page being read.
    fn step_match(&mut self, host: &mut dyn Host, surface: u64, forward: bool) {
        let current_page = self.current_page(surface);
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        let Some(d) = self.docs.get_mut(&doc) else { return };
        let s = &mut d.search;
        if s.matches.is_empty() {
            let message = if s.query.is_empty() { "no search yet -- / searches the document".to_string() } else { format!("no matches for \"{}\"", s.query) };
            return host.say(Level::Info, &message);
        }
        s.highlights = true;
        let n = s.matches.len();
        let at = match s.current {
            Some(at) if forward => (at + 1) % n,
            Some(at) => (at + n - 1) % n,
            None if forward => s.matches.iter().position(|m| m.page >= current_page).unwrap_or(0),
            None => s.matches.iter().rposition(|m| m.page <= current_page).unwrap_or(n - 1),
        };
        self.go_to_match(host, doc, at);
    }

    /// Match `at` of `doc`'s search: highlighted as the current one and
    /// brought into view, a third of the way down the pane.
    fn go_to_match(&mut self, host: &mut dyn Host, doc: u64, at: usize) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        let Some(m) = d.search.matches.get(at) else { return };
        let (page, top_pts, total) = (m.page, m.rects.first().map(|r| r[1]).unwrap_or(0.0), d.search.matches.len());
        let page_h = d.pages.get(page as usize).map(|p| p.1).unwrap_or(792.0);
        d.search.current = Some(at);
        d.search.highlights = true;
        let Some(surface) = d.view else { return };
        let third = self.views.get(&surface).filter(|v| v.layout.scale > 0.0).map(|v| v.pane.1 as f32 / 3.0 / v.layout.scale).unwrap_or(0.0);
        let frac = ((top_pts - third) / page_h.max(1.0)).max(0.0);
        self.jump(host, surface, (page, frac));
        self.refresh_sidebar(host);
        host.say(Level::Info, &format!("match {}/{total} on page {}", at + 1, page + 1));
    }

    // -- Prompts and pickers ----------------------------------------------

    /// `SPC r g`: asks for a page number.
    pub fn ask_page(&mut self, host: &mut dyn Host, surface: u64) {
        let id = host.prompt("go to page", "");
        self.asked.insert(id, Asked::GotoPage(surface));
    }

    pub fn prompted(&mut self, host: &mut dyn Host, id: u64, text: Option<String>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let Some(text) = text else { return };
        match asked {
            Asked::Search(surface) => self.search(host, surface, text, true),
            Asked::GotoPage(surface) => match text.trim().parse::<u32>() {
                Ok(n) => self.jump(host, surface, (n.saturating_sub(1), 0.0)),
                Err(_) => host.say(Level::Info, &format!("{text}: not a page number")),
            },
            _ => {}
        }
    }

    pub fn picked(&mut self, host: &mut dyn Host, id: u64, value: Option<String>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let Some(value) = value else { return };
        match asked {
            Asked::Heading(surface) => {
                if let Ok(page) = value.parse::<u32>() {
                    self.jump(host, surface, (page, 0.0));
                }
            }
            Asked::Shelf => self.open_document(host, &value),
            _ => {}
        }
    }

    /// One of the shelf's documents: a PDF here, anything else in Fenix.
    pub fn open_document(&mut self, host: &mut dyn Host, path: &str) {
        if layout::looks_like_pdf(path) {
            self.open(host, path, OpenAt::Here);
        } else {
            host.open_file(path);
        }
    }

    // -- Links ------------------------------------------------------------

    /// `yp`, `SPC r y`: a link to the page being read -- the path from the
    /// project (or in full) and `#page=38` -- to the clipboard, for notes
    /// and tasks. `gf` on it opens the document there.
    pub fn copy_link(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(path) = self.views.get(&surface).and_then(|v| self.docs.get(&v.doc)).map(|d| d.path.clone()) else { return };
        let slashed = path.replace('\\', "/");
        let shown = host
            .project_root()
            .map(|root| root.replace('\\', "/").trim_end_matches('/').to_string() + "/")
            .and_then(|root| slashed.get(..root.len()).filter(|head| layout::same_path(head, &root)).map(|_| slashed[root.len()..].to_string()))
            .unwrap_or(slashed);
        let link = format!("{shown}#page={}", self.current_page(surface) + 1);
        host.copy(&link);
        host.say(Level::Info, &format!("copied {link}"));
    }

    /// `f`: a label on every link in sight; typing one follows it.
    fn show_hints(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get(&surface) else { return };
        let visible = view.layout.visible(view.scroll.1, view.pane.1 as f32);
        let Some(doc) = self.docs.get(&view.doc) else { return };
        let found: Vec<(u32, usize)> = visible.clone().flat_map(|page| doc.links.get(&(page as u32)).map(|l| (0..l.len()).map(move |i| (page as u32, i)).collect::<Vec<_>>()).unwrap_or_default()).collect();
        if found.is_empty() {
            let asked = visible.clone().any(|page| !doc.links.contains_key(&(page as u32)));
            return host.say(Level::Info, if asked { "reading the page's links\u{2026} -- f again in a moment" } else { "no links in sight" });
        }
        let labels = text::hint_labels(found.len());
        let links = labels.into_iter().zip(found).map(|(label, (page, at))| (label, page, at)).collect();
        if let Some(view) = self.views.get_mut(&surface) {
            view.hints = Some(Hints { links, typed: String::new(), drawn: Vec::new() });
        }
        self.draw_hints(host, surface);
        self.marks(host, surface);
        host.surface_present(surface);
        host.say(Level::Info, "type a link's letters to follow it -- Esc cancels");
    }

    /// A key while link hints show.
    fn hint_key(&mut self, host: &mut dyn Host, surface: u64, press: Press) {
        let typed = {
            let Some(hints) = self.views.get_mut(&surface).and_then(|v| v.hints.as_mut()) else { return };
            match (press.plain_char(), press.key) {
                (Some(c), _) if c.is_ascii_lowercase() => {
                    hints.typed.push(c);
                    Some(hints.typed.clone())
                }
                (_, fenix_extension_api::SurfaceKey::Named(fenix_extension_api::NamedKey::Backspace)) => {
                    hints.typed.pop();
                    Some(hints.typed.clone())
                }
                _ => None,
            }
        };
        let Some(typed) = typed else { return self.end_hints(host, surface, None) };
        let matching: Vec<(String, u32, usize)> = self.views.get(&surface).and_then(|v| v.hints.as_ref()).map(|h| h.links.iter().filter(|(l, _, _)| l.starts_with(&typed)).cloned().collect()).unwrap_or_default();
        match matching.as_slice() {
            [] => self.end_hints(host, surface, None),
            [(label, page, at)] if *label == typed => {
                let to = self.views.get(&surface).and_then(|v| self.docs.get(&v.doc)).and_then(|d| d.links.get(page)).and_then(|l| l.get(*at)).map(|l| l.to.clone());
                self.end_hints(host, surface, to);
            }
            _ => {
                // What's left of the labels: the others' painted over.
                self.repaint_hints(host, surface);
                self.draw_hints(host, surface);
                self.marks(host, surface);
                host.surface_present(surface);
            }
        }
    }

    /// The labels painted over again: as paper, their pages rendered again.
    fn repaint_hints(&mut self, host: &mut dyn Host, surface: u64) {
        let (paper, gap) = self.colors(host);
        let drawn = self.views.get(&surface).and_then(|v| v.hints.as_ref()).map(|h| h.drawn.clone()).unwrap_or_default();
        for (x, y, w, h) in drawn {
            self.paint(host, surface, (x as f32, y as f32, w as f32, h as f32), paper, gap);
        }
        self.flush(host, surface);
    }

    fn end_hints(&mut self, host: &mut dyn Host, surface: u64, follow: Option<Target>) {
        self.repaint_hints(host, surface);
        if let Some(view) = self.views.get_mut(&surface) {
            view.hints = None;
        }
        self.marks(host, surface);
        host.surface_present(surface);
        if let Some(to) = follow {
            self.follow(host, surface, &to);
        }
    }

    /// Goes where a link points: a page of this document is a jump
    /// (`Ctrl-o` comes back), a web link opens in the browser.
    fn follow(&mut self, host: &mut dyn Host, surface: u64, to: &Target) {
        match to {
            Target::Page { page, y } => {
                let page_h = self.views.get(&surface).and_then(|v| self.docs.get(&v.doc)).and_then(|d| d.pages.get(*page as usize)).map(|p| p.1).unwrap_or(792.0);
                let frac = y.map(|y| (y / page_h.max(1.0)).clamp(0.0, 1.0)).unwrap_or(0.0);
                self.jump(host, surface, (*page, frac));
                host.say(Level::Info, &format!("page {}", page + 1));
            }
            Target::Uri(uri) => match host.open_url(uri) {
                Ok(()) => host.say(Level::Info, &format!("opened {uri}")),
                Err(e) => host.say(Level::Error, &e),
            },
        }
    }

    // -- Text: selecting and copying --------------------------------------

    fn text_came(&mut self, host: &mut dyn Host, doc: u64, page: u32, chars: Vec<Ch>) {
        let empty = chars.is_empty();
        let Some(d) = self.docs.get_mut(&doc) else { return };
        d.text.insert(page, chars);
        let waiting: Vec<u64> = self.views.iter().filter(|(_, v)| v.doc == doc && v.visual_waiting == Some(page)).map(|(&s, _)| s).collect();
        for surface in waiting {
            if let Some(view) = self.views.get_mut(&surface) {
                view.visual_waiting = None;
            }
            if empty {
                host.say(Level::Info, "this page has no text to select -- a scan?");
            } else {
                self.start_visual(host, surface);
            }
        }
    }

    /// `v`: a selection from the start of the top line in sight, the keys
    /// moving its end. Waits for the page's text if it hasn't come.
    pub fn start_visual(&mut self, host: &mut dyn Host, surface: u64) {
        let Some(view) = self.views.get(&surface) else { return };
        if view.layout.pages.is_empty() {
            return;
        }
        let page = view.layout.page_at(view.scroll.1) as u32;
        let p = view.layout.pages[page as usize];
        let top = ((view.scroll.1 - p.y) / view.layout.scale).max(0.0);
        let doc = view.doc;
        let Some(chars) = self.docs.get(&doc).and_then(|d| d.text.get(&page)) else {
            if let Some(view) = self.views.get_mut(&surface) {
                view.visual_waiting = Some(page);
            }
            self.want_text(host, doc, page);
            return host.say(Level::Info, "reading the page's text\u{2026}");
        };
        let Some(idx) = text::line_start_near(chars, top + 6.0) else {
            return host.say(Level::Info, "this page has no text to select -- a scan?");
        };
        let at = TextPos { page, idx };
        if let Some(view) = self.views.get_mut(&surface) {
            view.selection = Some((at, at));
            view.visual = true;
        }
        self.marks(host, surface);
        host.surface_present(surface);
        host.say(Level::Info, "VISUAL -- w b e 0 $ j k h l move, y copies, Esc stops");
    }

    fn visual_key(&mut self, host: &mut dyn Host, surface: u64, press: Press) {
        use fenix_extension_api::{NamedKey, SurfaceKey};
        if !press.plain() {
            return;
        }
        let motion = match press.key {
            SurfaceKey::Char('h') | SurfaceKey::Named(NamedKey::Left) => Motion::Left,
            SurfaceKey::Char('l') | SurfaceKey::Named(NamedKey::Right) => Motion::Right,
            SurfaceKey::Char('w') => Motion::WordForward,
            SurfaceKey::Char('b') => Motion::WordBack,
            SurfaceKey::Char('e') => Motion::WordEnd,
            SurfaceKey::Char('0') => Motion::LineStart,
            SurfaceKey::Char('$') => Motion::LineEnd,
            SurfaceKey::Char('j') | SurfaceKey::Named(NamedKey::Down) => Motion::LineDown,
            SurfaceKey::Char('k') | SurfaceKey::Named(NamedKey::Up) => Motion::LineUp,
            SurfaceKey::Char('y') => return self.copy_selection(host, surface),
            SurfaceKey::Char('v') | SurfaceKey::Named(NamedKey::Escape) => {
                if let Some(view) = self.views.get_mut(&surface) {
                    view.visual = false;
                    view.selection = None;
                }
                self.marks(host, surface);
                return host.surface_present(surface);
            }
            _ => return,
        };
        self.move_selection(host, surface, motion);
    }

    /// Moves the selection's end by `motion`, on to the next or previous
    /// page at an edge (once its text has come), and keeps it in sight.
    fn move_selection(&mut self, host: &mut dyn Host, surface: u64, motion: Motion) {
        let Some((anchor, head)) = self.views.get(&surface).and_then(|v| v.selection) else { return };
        let Some(doc) = self.views.get(&surface).map(|v| v.doc) else { return };
        let count = self.docs.get(&doc).map(|d| d.page_count()).unwrap_or(0);
        let Some(chars) = self.docs.get(&doc).and_then(|d| d.text.get(&head.page)) else { return };
        let next = match text::step(chars, head.idx, motion) {
            Some(idx) => Some(TextPos { page: head.page, idx }),
            None => {
                let forward = matches!(motion, Motion::LineDown | Motion::WordForward | Motion::WordEnd | Motion::Right);
                let page = if forward { head.page + 1 } else { head.page.wrapping_sub(1) };
                if page >= count {
                    None
                } else if let Some(other) = self.docs.get(&doc).and_then(|d| d.text.get(&page)) {
                    let idx = if forward { 0 } else { other.len().saturating_sub(1) };
                    (!other.is_empty()).then_some(TextPos { page, idx })
                } else {
                    self.want_text(host, doc, page);
                    None
                }
            }
        };
        let Some(next) = next else { return };
        if let Some(view) = self.views.get_mut(&surface) {
            view.selection = Some((anchor, next));
        }
        self.keep_in_sight(host, surface, next);
    }

    /// Scrolls so the character at `at` shows.
    fn keep_in_sight(&mut self, host: &mut dyn Host, surface: u64, at: TextPos) {
        let Some(view) = self.views.get(&surface) else { return };
        let Some(r) = self.docs.get(&view.doc).and_then(|d| d.text.get(&at.page)).and_then(|c| c.get(at.idx)).map(|c| c.rect) else { return };
        let Some(p) = view.layout.pages.get(at.page as usize).copied() else { return };
        let (top, bottom) = (p.y + r[1] * view.layout.scale, p.y + r[3] * view.layout.scale);
        let (pane_h, scroll) = (view.pane.1 as f32, view.scroll.1);
        let margin = pane_h / 6.0;
        let max_y = view.layout.max_scroll((view.pane.0 as f32, pane_h)).1;
        let to = if top < scroll {
            (top - margin).max(0.0)
        } else if bottom > scroll + pane_h {
            (bottom + margin - pane_h).min(max_y)
        } else {
            scroll
        };
        if let Some(view) = self.views.get_mut(&surface) {
            view.scroll.1 = to.round();
        }
        self.draw(host, surface);
    }

    /// `y` on a selection: its text, lines run together, to the clipboard.
    fn copy_selection(&mut self, host: &mut dyn Host, surface: u64) {
        let Some((a, b)) = self.views.get(&surface).and_then(|v| v.selection) else { return };
        let (from, to) = (a.min(b), a.max(b));
        let Some(doc) = self.views.get(&surface).and_then(|v| self.docs.get(&v.doc)) else { return };
        let mut parts = Vec::new();
        for page in from.page..=to.page {
            let Some(chars) = doc.text.get(&page) else { continue };
            let start = if page == from.page { from.idx } else { 0 };
            let end = if page == to.page { to.idx } else { chars.len().saturating_sub(1) };
            parts.push(text::copy_text(chars, start, end));
        }
        let copied = parts.join(" ");
        let words = copied.split_whitespace().count();
        host.copy(&copied);
        if let Some(view) = self.views.get_mut(&surface) {
            view.visual = false;
            view.selection = None;
        }
        self.marks(host, surface);
        host.surface_present(surface);
        host.say(Level::Info, &format!("copied {words} word{}", if words == 1 { "" } else { "s" }));
    }

    // -- Settings and the theme -------------------------------------------

    /// `SPC r c`: switches `reader.colors` between paper and the theme.
    pub fn toggle_colors(&mut self, host: &mut dyn Host) {
        let next = if self.recoloring(host).is_some() { "paper" } else { "theme" };
        match host.set_setting("reader.colors", Some(next)) {
            Ok(()) => host.say(Level::Info, if next == "theme" { "PDF pages in the theme's colours" } else { "PDF pages as printed" }),
            Err(e) => host.say(Level::Error, &e),
        }
        self.redraw_all(host);
    }

    /// The colours or the layout's inputs changed: every view drawn anew.
    pub fn redraw_all(&mut self, host: &mut dyn Host) {
        let surfaces: Vec<u64> = self.views.keys().copied().collect();
        for surface in surfaces {
            if let Some(view) = self.views.get_mut(&surface) {
                view.epoch += 1;
            }
            self.draw(host, surface);
        }
    }

    pub fn setting_changed(&mut self, host: &mut dyn Host, key: &str) {
        match key {
            "reader.colors" | "reader.page_gap" => self.redraw_all(host),
            "reader.documents" => self.refresh_home(host),
            _ => {}
        }
    }

    pub fn timer(&mut self, host: &mut dyn Host, timer: u64) {
        if self.save_timer == Some(timer) {
            self.save_places(host);
        }
    }

    /// A row of its section on Home: that PDF.
    pub fn home_picked(&mut self, host: &mut dyn Host, path: &str) {
        self.open_document(host, path);
    }

    /// Tests: the views, by surface.
    #[doc(hidden)]
    pub fn view_scroll(&self, surface: u64) -> Option<(f32, f32)> {
        self.views.get(&surface).map(|v| v.scroll)
    }

    #[doc(hidden)]
    pub fn view_zoom(&self, surface: u64) -> Option<Zoom> {
        self.views.get(&surface).map(|v| v.zoom)
    }

    #[doc(hidden)]
    pub fn surfaces(&self) -> Vec<u64> {
        self.views.keys().copied().collect()
    }

    #[doc(hidden)]
    pub fn sidebar_open(&self) -> Option<u64> {
        self.sidebar.as_ref().map(|s| s.page)
    }

    #[doc(hidden)]
    pub fn selected(&self, surface: u64) -> Option<(TextPos, TextPos)> {
        self.views.get(&surface).and_then(|v| v.selection)
    }
}

fn recolor_into(bgra: &mut [u8], paper: Rgb, ink: Rgb) {
    recolor(bgra, paper, ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoloring_turns_white_into_paper_and_black_into_ink() {
        let mut px = vec![255, 255, 255, 255, 0, 0, 0, 255];
        recolor(&mut px, (30, 30, 46), (205, 214, 244));
        assert_eq!(&px[0..3], &[46, 30, 30], "white is the theme's background (BGR)");
        assert_eq!(&px[4..7], &[244, 214, 205], "black is its text");
    }

    #[test]
    fn rectangles_meet_where_they_overlap() {
        assert_eq!(intersect((0.0, 0.0, 10.0, 10.0), (5.0, 5.0, 10.0, 10.0)), Some((5.0, 5.0, 5.0, 5.0)));
        assert_eq!(intersect((0.0, 0.0, 10.0, 10.0), (10.0, 0.0, 5.0, 5.0)), None);
    }
}
