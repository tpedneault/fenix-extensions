//! The reader against a fake companion, through a fake Fenix: requests go
//! to the companion as `fenix-pdfium` takes them, its replies come back in
//! pieces as a pipe brings them, and what the surface ends up showing is
//! checked pixel by pixel. The fake renders each page's pixel as its own
//! coordinates -- red its x, green its y, blue its page -- so a pixel in the
//! wrong place shows.

use std::collections::{HashMap, VecDeque};

use fenix_extension_api::{HomeSection, Key, Level, Modifiers, NamedKey, PointerEvent, Role, SurfaceMark, SurfaceSize, WheelEvent};
use fenix_pdf::keys::Press;
use fenix_pdf::layout::{self, Zoom};
use fenix_pdf::protocol::{Frames, Heading, Link, Match, Reply, Request, Target, VERSION};
use fenix_pdf::reader::{Host, OpenAt, Reader, Rgb, PAPER};

const PANE: (u32, u32) = (636, 400);
const PROCESS: u64 = 77;
const GAP: Rgb = (30, 30, 46);

/// A picker asked for: its id, title and items.
type Picked = (u64, String, Vec<(String, String, String)>);

/// What Fenix did, as the reader asked.
#[derive(Default)]
struct Fake {
    companion: Option<String>,
    spawned: usize,
    sent: Vec<u8>,
    surfaces: HashMap<u64, (u32, u32, Vec<u8>)>,
    next_surface: u64,
    closed: Vec<u64>,
    presented: usize,
    copies: Vec<(u32, u32, u32, u32, u32, u32)>,
    marks: HashMap<u64, Vec<SurfaceMark>>,
    pages: Vec<(u64, String, String, Option<f32>)>,
    refreshed: Vec<u64>,
    said: Vec<(Level, String)>,
    status: Option<String>,
    prompts: Vec<(u64, String)>,
    picks: Vec<Picked>,
    copied: Vec<String>,
    urls: Vec<String>,
    files: Vec<String>,
    settings: HashMap<String, String>,
    home: Option<HomeSection>,
    root: Option<String>,
    answers: Vec<(u64, Result<String, String>)>,
    timers: Vec<u64>,
    data: HashMap<String, String>,
    ids: u64,
}

impl Fake {
    fn id(&mut self) -> u64 {
        self.ids += 1;
        self.ids
    }

    fn said(&self, part: &str) -> bool {
        self.said.iter().any(|(_, s)| s.contains(part))
    }

    /// Pixel (x, y) of `surface`: (r, g, b).
    fn px(&self, surface: u64, x: u32, y: u32) -> Rgb {
        let (w, _, bgra) = &self.surfaces[&surface];
        let at = ((y * w + x) * 4) as usize;
        (bgra[at + 2], bgra[at + 1], bgra[at])
    }
}

impl Host for Fake {
    fn find_companion(&mut self) -> Option<String> {
        self.companion.clone()
    }
    fn spawn_companion(&mut self, _program: &str) -> Result<u64, String> {
        self.spawned += 1;
        Ok(PROCESS)
    }
    fn write_companion(&mut self, process: u64, bytes: &[u8]) -> Result<(), String> {
        assert_eq!(process, PROCESS);
        self.sent.extend_from_slice(bytes);
        Ok(())
    }
    fn kill(&mut self, _process: u64) {}
    fn surface_open(&mut self, _title: &str) -> Result<u64, String> {
        self.next_surface += 1;
        let s = 100 + self.next_surface;
        self.surfaces.insert(s, (0, 0, Vec::new()));
        Ok(s)
    }
    fn surface_resize(&mut self, s: u64, w: u32, h: u32) {
        self.surfaces.insert(s, (w, h, vec![0; (w * h * 4) as usize]));
    }
    fn surface_write(&mut self, s: u64, x: u32, y: u32, w: u32, h: u32, bgra: &[u8]) {
        let (sw, sh, fb) = self.surfaces.get_mut(&s).unwrap();
        assert!(x + w <= *sw && y + h <= *sh, "inside the surface: {x},{y} {w}x{h} of {sw}x{sh}");
        assert_eq!(bgra.len(), (w * h * 4) as usize);
        for row in 0..h {
            let to = (((y + row) * *sw + x) * 4) as usize;
            fb[to..to + (w * 4) as usize].copy_from_slice(&bgra[(row * w * 4) as usize..((row + 1) * w * 4) as usize]);
        }
    }
    fn surface_fill(&mut self, s: u64, x: u32, y: u32, w: u32, h: u32, (r, g, b): Rgb) {
        let (sw, sh, fb) = self.surfaces.get_mut(&s).unwrap();
        for yy in y..(y + h).min(*sh) {
            for xx in x..(x + w).min(*sw) {
                let at = ((yy * *sw + xx) * 4) as usize;
                fb[at..at + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
    }
    fn surface_copy(&mut self, s: u64, x: u32, y: u32, w: u32, h: u32, to_x: u32, to_y: u32) {
        self.copies.push((x, y, w, h, to_x, to_y));
        let (sw, _, fb) = self.surfaces.get_mut(&s).unwrap();
        let old = fb.clone();
        for row in 0..h {
            let from = (((y + row) * *sw + x) * 4) as usize;
            let to = (((to_y + row) * *sw + to_x) * 4) as usize;
            fb[to..to + (w * 4) as usize].copy_from_slice(&old[from..from + (w * 4) as usize]);
        }
    }
    fn surface_present(&mut self, _s: u64) {
        self.presented += 1;
    }
    fn surface_marks(&mut self, s: u64, marks: &[SurfaceMark]) {
        self.marks.insert(s, marks.to_vec());
    }
    fn retitle(&mut self, _p: u64, _title: &str) {}
    fn page_open(&mut self, kind: &str, title: &str, beside: Option<f32>) -> u64 {
        let id = 500 + self.id();
        self.pages.push((id, kind.into(), title.into(), beside));
        id
    }
    fn page_refresh(&mut self, p: u64) {
        self.refreshed.push(p);
    }
    fn page_close(&mut self, p: u64) {
        self.closed.push(p);
    }
    fn say(&mut self, level: Level, message: &str) {
        self.said.push((level, message.into()));
    }
    fn status(&mut self, text: Option<&str>) {
        self.status = text.map(str::to_string);
    }
    fn prompt(&mut self, label: &str, _initial: &str) -> u64 {
        let id = self.id();
        self.prompts.push((id, label.into()));
        id
    }
    fn pick(&mut self, title: &str, items: &[(String, String, String)]) -> u64 {
        let id = self.id();
        self.picks.push((id, title.into(), items.to_vec()));
        id
    }
    fn copy(&mut self, text: &str) {
        self.copied.push(text.into());
    }
    fn open_url(&mut self, url: &str) -> Result<(), String> {
        if url.starts_with("https://") || url.starts_with("http://") {
            self.urls.push(url.into());
            Ok(())
        } else {
            Err(format!("{url}: not an http(s) URL"))
        }
    }
    fn open_file(&mut self, path: &str) {
        self.files.push(path.into());
    }
    fn setting(&mut self, key: &str) -> Option<String> {
        self.settings.get(key).cloned()
    }
    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        match value {
            Some(v) => self.settings.insert(key.into(), v.into()),
            None => self.settings.remove(key),
        };
        Ok(())
    }
    fn theme(&mut self) -> (Rgb, Rgb) {
        (GAP, (205, 214, 244))
    }
    fn home(&mut self, section: Option<&HomeSection>) {
        self.home = section.cloned();
    }
    fn project_root(&mut self) -> Option<String> {
        self.root.clone()
    }
    fn answer(&mut self, call: u64, result: Result<String, String>) {
        self.answers.push((call, result));
    }
    fn timer(&mut self, _millis: u32) -> u64 {
        let id = self.id();
        self.timers.push(id);
        id
    }
    fn load(&mut self, name: &str) -> Option<String> {
        self.data.get(name).cloned()
    }
    fn save(&mut self, name: &str, text: &str) {
        self.data.insert(name.into(), text.into());
    }
}

/// The companion, faked: US Letter pages, text on every page, a link to
/// the next page and a web link on the first, an outline, and "packet" on
/// pages 3 and 8.
struct Companion {
    pages: usize,
    /// Every request it took.
    got: Vec<Request>,
    /// Renders it holds back, answered when let go.
    holding: bool,
    held: VecDeque<Request>,
    failing: bool,
}

impl Companion {
    fn new(pages: usize) -> Companion {
        Companion { pages, got: Vec::new(), holding: false, held: VecDeque::new(), failing: false }
    }

    fn line_chars(page: u32) -> Vec<(char, [f32; 4])> {
        let mut chars = Vec::new();
        for (row, line) in [format!("Page {} begins", page + 1), "and goes on".to_string()].iter().enumerate() {
            for (col, c) in line.chars().enumerate() {
                let (x, y) = (72.0 + col as f32 * 10.0, 72.0 + row as f32 * 20.0);
                chars.push((c, [x, y, x + 10.0, y + 14.0]));
            }
            if row == 0 {
                chars.push(('\n', [0.0; 4]));
            }
        }
        chars
    }

    fn answer(&mut self, request: Request) -> Vec<Reply> {
        self.got.push(request.clone());
        match request {
            Request::Open { doc, .. } if self.failing => vec![Reply::OpenFailed { doc, message: "pdfium library not found -- ... FENIX_PDFIUM_PATH".into() }],
            Request::Open { doc, .. } => vec![Reply::Opened { doc, pages: vec![(612.0, 792.0); self.pages] }],
            r @ Request::Render { .. } if self.holding => {
                self.held.push_back(r);
                Vec::new()
            }
            Request::Render { doc, view, id, page, x, y, w, h, .. } => {
                let mut bgra = Vec::with_capacity((w * h * 4) as usize);
                for py in y..y + h {
                    for px in x..x + w {
                        bgra.extend_from_slice(&[page as u8, (py % 256) as u8, (px % 256) as u8, 255]);
                    }
                }
                vec![Reply::Rendered { doc, view, id, page, x, y, w, h, bgra }]
            }
            Request::CancelBefore { view, before, .. } => {
                self.held.retain(|r| !matches!(r, Request::Render { view: v, id, .. } if *v == view && *id < before));
                Vec::new()
            }
            Request::Outline { doc } => vec![Reply::Outline { doc, headings: vec![Heading { title: "1 Intro".into(), page: 0, depth: 0 }, Heading { title: "2 Packets".into(), page: 2, depth: 0 }, Heading { title: "2.1 Headers".into(), page: 3, depth: 1 }] }],
            Request::Text { doc, page } => vec![Reply::Text { doc, page, chars: Self::line_chars(page) }],
            Request::Links { doc, page } if page == 0 => vec![Reply::Links { doc, page, links: vec![Link { rect: [72.0, 72.0, 212.0, 86.0], to: Target::Page { page: 4, y: None } }, Link { rect: [72.0, 300.0, 200.0, 314.0], to: Target::Uri("https://example.com/".into()) }] }],
            Request::Links { doc, page } => vec![Reply::Links { doc, page, links: Vec::new() }],
            Request::Search { doc, id, query, match_case } => {
                let hits = |pages: &[u32]| pages.iter().map(|&p| Match { page: p, context: format!("a {query} on {}", p + 1), rects: vec![[72.0, 400.0, 130.0, 414.0]] }).collect::<Vec<_>>();
                let word = ["packet", "frame"].iter().any(|w| query.eq_ignore_ascii_case(w));
                if word && (!match_case || query.chars().all(|c| c.is_lowercase())) {
                    vec![Reply::Found { doc, id, done: false, matches: hits(&[2]) }, Reply::Found { doc, id, done: true, matches: hits(&[7]) }]
                } else {
                    vec![Reply::Found { doc, id, done: true, matches: Vec::new() }]
                }
            }
            Request::Close { .. } => Vec::new(),
        }
    }
}

/// The reader, Fenix and the companion, wired.
struct Bench {
    reader: Reader,
    host: Fake,
    companion: Companion,
    frames: Frames,
}

impl Bench {
    fn new() -> Bench {
        let host = Fake { companion: Some("C:/Fenix/fenix-pdfium.exe".into()), ..Default::default() };
        let mut b = Bench { reader: Reader::default(), host, companion: Companion::new(10), frames: Frames::default() };
        b.reader.activate(&mut b.host);
        b
    }

    /// Requests taken by the companion and its replies given back -- in
    /// pieces of 1000 bytes -- until it's quiet.
    fn pump(&mut self) {
        let mut said_hello = self.companion.got.is_empty();
        loop {
            let sent = std::mem::take(&mut self.host.sent);
            if sent.is_empty() {
                return;
            }
            let mut out = Vec::new();
            if std::mem::take(&mut said_hello) {
                out.extend(Reply::Hello { version: VERSION }.encode());
            }
            for body in self.frames.push(&sent).unwrap() {
                for reply in self.companion.answer(Request::decode(&body).unwrap()) {
                    out.extend(reply.encode());
                }
            }
            for piece in out.chunks(1000) {
                self.reader.companion_bytes(&mut self.host, PROCESS, piece);
            }
        }
    }

    /// The held renders answered, in order.
    fn release(&mut self) {
        self.companion.holding = false;
        let held: Vec<Request> = self.companion.held.drain(..).collect();
        let mut out = Vec::new();
        for r in held {
            for reply in self.companion.answer(r) {
                out.extend(reply.encode());
            }
        }
        self.reader.companion_bytes(&mut self.host, PROCESS, &out);
        self.pump();
    }

    /// `path` opened, its pane sized, and everything answered: its surface.
    fn open(&mut self, path: &str) -> u64 {
        self.reader.open(&mut self.host, path, OpenAt::Here);
        let surface = *self.reader.surfaces().last().unwrap();
        self.size(surface, PANE, 1.0);
        surface
    }

    fn size(&mut self, surface: u64, (width, height): (u32, u32), scale: f32) {
        self.reader.resized(&mut self.host, SurfaceSize { surface, width, height, scale });
        self.pump();
    }

    fn keys(&mut self, surface: u64, text: &str) {
        for c in text.chars() {
            self.reader.key(&mut self.host, surface, Press::char(c));
            self.pump();
        }
    }

    fn key(&mut self, surface: u64, press: Press) {
        self.reader.key(&mut self.host, surface, press);
        self.pump();
    }

    fn renders(&self) -> Vec<(u32, u32, u32, u32, u32)> {
        self.companion.got.iter().filter_map(|r| if let Request::Render { page, x, y, w, h, .. } = r { Some((*page, *x, *y, *w, *h)) } else { None }).collect()
    }

    fn scroll(&self, surface: u64) -> (f32, f32) {
        self.reader.view_scroll(surface).unwrap()
    }

    fn page(&self, surface: u64) -> u32 {
        self.reader.current_page(surface)
    }

    fn layout(&self) -> layout::Layout {
        layout::layout(&vec![(612.0, 792.0); self.companion.pages], Zoom::FitWidth, (PANE.0 as f32, PANE.1 as f32), 1.0, layout::PAGE_GAP)
    }

    /// Every pixel of the surface is what the scroll says it should be: a
    /// page's own pixel, or the gap's colour.
    fn check_pixels(&self, surface: u64) {
        let l = self.layout();
        let (sx, sy) = self.scroll(surface);
        for y in (0..PANE.1).step_by(7) {
            for x in (0..PANE.0).step_by(5) {
                let (dx, dy) = (x as f32 + sx, y as f32 + sy);
                let got = self.host.px(surface, x, y);
                let page = l.pages.iter().position(|p| dx >= p.x && dx < p.x + p.w && dy >= p.y && dy < p.y + p.h);
                let want = match page {
                    Some(i) => {
                        let p = l.pages[i];
                        (((dx - p.x) as u32 % 256) as u8, ((dy - p.y) as u32 % 256) as u8, i as u8)
                    }
                    None => GAP,
                };
                assert_eq!(got, want, "pixel {x},{y} at scroll {sx},{sy}");
            }
        }
    }
}

#[test]
fn a_pdf_opens_on_a_surface_its_pages_in_sight_rendered() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    assert_eq!(b.host.spawned, 1, "the companion started");
    assert!(matches!(b.companion.got[0], Request::Open { ref path, .. } if path == "C:/refs/spec.pdf"));
    // The first page, in sight, in bands; nothing past the pane.
    let renders = b.renders();
    assert!(!renders.is_empty() && renders.iter().all(|r| r.0 == 0), "{renders:?}");
    assert!(renders.iter().all(|r| r.4 <= 256), "in bands");
    b.check_pixels(s);
    assert_eq!(b.host.status.as_deref(), Some("PDF p. 1/10  Fit width"));
    // Home has it, and the shelf.
    assert_eq!(b.host.home.as_ref().map(|h| h.rows[0].name.as_str()), Some("spec.pdf"));
    assert_eq!(b.reader.shelf(&mut b.host)[0].2, "C:/refs/spec.pdf");
}

#[test]
fn without_its_companion_it_says_where_it_comes_from() {
    let mut b = Bench::new();
    b.host.companion = None;
    b.reader.open(&mut b.host, "C:/refs/spec.pdf", OpenAt::Here);
    assert!(b.host.said("fenix-pdfium isn't beside Fenix") && b.host.said("docs/BUILDING.md"), "{:?}", b.host.said);
    let mut b = Bench::new();
    b.companion.failing = true;
    b.open("C:/refs/spec.pdf");
    assert!(b.host.said("pdfium library not found") && b.host.said("docs/BUILDING.md says where PDFium goes"), "{:?}", b.host.said);
}

#[test]
fn scrolling_moves_what_is_shown_and_renders_only_the_strip() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    let before = b.renders().len();
    b.keys(s, "j");
    assert_eq!(b.scroll(s).1, 60.0);
    assert_eq!(b.host.copies.last(), Some(&(0, 60, 636, 340, 0, 0)), "the rest moved up");
    let strip = &b.renders()[before..];
    assert_eq!(strip.len(), 1, "{strip:?}");
    assert_eq!(strip[0].4, 60, "only the strip that came into view");
    b.check_pixels(s);
    // A count, half screens, across pages.
    b.keys(s, "5j");
    assert_eq!(b.scroll(s).1, 360.0);
    b.check_pixels(s);
    b.key(s, Press::ctrl('d'));
    assert_eq!(b.scroll(s).1, 560.0);
    b.check_pixels(s);
    b.key(s, Press::ctrl('u'));
    b.keys(s, "k");
    b.check_pixels(s);
    // The wheel, down three lines.
    b.reader.wheel(&mut b.host, s, WheelEvent { x: 10, y: 10, lines: -3, mods: Modifiers { ctrl: false, alt: false, meta: false } });
    b.pump();
    assert_eq!(b.scroll(s).1, 300.0 + 120.0);
    b.check_pixels(s);
}

#[test]
fn a_render_scrolled_past_while_it_came_lands_where_its_page_is_now() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.companion.holding = true;
    b.keys(s, "jjj");
    b.release();
    b.check_pixels(s);
}

#[test]
fn page_keys_jumps_and_ctrl_o() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "J");
    assert_eq!(b.page(s), 1);
    b.keys(s, "3J");
    assert_eq!(b.page(s), 4);
    b.keys(s, "K");
    assert_eq!(b.page(s), 3);
    b.keys(s, "8G");
    assert_eq!(b.page(s), 7);
    b.keys(s, "G");
    assert_eq!(b.page(s), 9, "the last");
    b.keys(s, "gg");
    assert_eq!(b.page(s), 0);
    b.keys(s, "99G");
    assert_eq!(b.page(s), 9, "past the end is the last page");
    b.check_pixels(s);
    // Jumps come back with Ctrl-o, and go on again with Ctrl-i.
    b.keys(s, "gg");
    b.keys(s, "6G");
    b.key(s, Press::ctrl('o'));
    assert_eq!(b.page(s), 0);
    b.key(s, Press::ctrl('i'));
    assert_eq!(b.page(s), 5);
    // Scrolling isn't a jump.
    b.keys(s, "jj");
    b.key(s, Press::ctrl('o'));
    assert_eq!(b.page(s), 0, "back past the scrolling, to before the jump");
}

#[test]
fn zoom_keeps_the_same_place_at_the_top_and_steps_from_where_it_is() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "3J");
    let anchor = b.page(s);
    b.keys(s, "+");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::Percent(110)), "fit width is 100% here, so + is 110%");
    assert!(b.page(s) >= anchor, "the same place");
    assert!(b.host.status.as_deref().unwrap().contains("110%"));
    b.keys(s, "--");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::Percent(90)));
    b.keys(s, "=");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::FitWidth));
    b.keys(s, "=");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::FitPage));
    b.keys(s, "z0");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::Percent(100)));
    b.keys(s, "zw");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::FitWidth));
    b.check_pixels(s);
    // Ctrl and the wheel zoom.
    b.reader.wheel(&mut b.host, s, WheelEvent { x: 1, y: 1, lines: 1, mods: Modifiers { ctrl: true, alt: false, meta: false } });
    b.pump();
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::Percent(110)));
    // Zoomed past the pane: h and l pan.
    b.keys(s, "+++");
    let x = b.scroll(s).0;
    b.keys(s, "l");
    assert_eq!(b.scroll(s).0, x + 60.0);
    b.keys(s, "hh");
    assert_eq!(b.scroll(s).0, x - 60.0);
}

#[test]
fn marks_come_back_to_the_same_place() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "4Jma");
    assert!(b.host.said("mark a on page 5"));
    b.keys(s, "gg'a");
    assert_eq!(b.page(s), 4);
    b.keys(s, "'b");
    assert!(b.host.said("no mark b in this document"));
}

#[test]
fn a_pdf_reopens_where_it_was_left_with_its_zoom_and_marks() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "6Gzpmq");
    // Written a moment later.
    let timer = *b.host.timers.last().expect("a save on a timer");
    b.reader.timer(&mut b.host, timer);
    let places = b.host.data["pdf_places.tsv"].clone();
    assert!(places.starts_with("C:/refs/spec.pdf\t5\t") && places.contains("\tpage\t10\tq:5:"), "{places}");
    // Another run, its places file as this one left it.
    let mut again = Bench::new();
    again.host.data.insert("pdf_places.tsv".into(), places);
    again.reader = Reader::default();
    again.reader.activate(&mut again.host);
    let s = again.open("C:/refs/spec.pdf");
    assert_eq!(again.page(s), 5);
    assert_eq!(again.reader.view_zoom(s), Some(Zoom::FitPage));
    assert_eq!(again.host.home.as_ref().unwrap().rows[0].detail, "p. 6 / 10");
    again.keys(s, "gg'q");
    assert_eq!(again.page(s), 5, "its marks too");
}

#[test]
fn with_remember_off_nothing_is_kept() {
    let mut b = Bench::new();
    b.host.settings.insert("reader.remember".into(), "false".into());
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "6G");
    assert!(b.host.timers.is_empty() && !b.host.data.contains_key("pdf_places.tsv"));
}

#[test]
fn the_default_zoom_and_page_gap_are_settings() {
    let mut b = Bench::new();
    b.host.settings.insert("reader.zoom".into(), "page".into());
    b.host.settings.insert("reader.page_gap".into(), "0".into());
    let s = b.open("C:/refs/spec.pdf");
    assert_eq!(b.reader.view_zoom(s), Some(Zoom::FitPage));
    // No gap: page 1 starts at the very top-left of its column.
    let (w, h) = PANE;
    let fit = layout::layout(&[(612.0, 792.0); 10], Zoom::FitPage, (w as f32, h as f32), 1.0, 0.0);
    assert_eq!(fit.pages[0].y, 0.0);
    let p = fit.pages[0];
    assert_eq!(b.host.px(s, p.x as u32 + 3, 2), (3, 2, 0), "the page's own pixel at its corner");
}

#[test]
fn spc_r_c_draws_the_pages_in_the_themes_colours() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    let printed = b.host.px(s, 12 + 100, 12 + 100);
    b.reader.toggle_colors(&mut b.host);
    b.pump();
    assert_eq!(b.host.settings.get("reader.colors").map(String::as_str), Some("theme"));
    assert!(b.host.said("PDF pages in the theme's colours"));
    let themed = b.host.px(s, 12 + 100, 12 + 100);
    assert_ne!(printed, themed, "drawn again, recoloured");
    let gap = b.host.px(s, 2, 2);
    assert_eq!(gap, ((GAP.0 as f32 * 0.72) as u8, (GAP.1 as f32 * 0.72) as u8, (GAP.2 as f32 * 0.72) as u8), "the gaps a shade darker");
    b.reader.toggle_colors(&mut b.host);
    b.pump();
    b.check_pixels(s);
}

#[test]
fn the_outline_opens_beside_follows_the_reading_and_goes_there() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "o");
    let (page, kind, title, beside) = b.host.pages.last().cloned().unwrap();
    assert_eq!((kind.as_str(), title.as_str(), beside), ("outline", "Outline: spec.pdf", Some(0.3)));
    assert!(b.companion.got.iter().any(|r| matches!(r, Request::Outline { .. })));
    let laid = b.reader.sidebar_page(page, 40).unwrap();
    assert_eq!(laid.lines[0].spans[0].text, "[Outline] Matches 0 Marks 0");
    assert_eq!(laid.lines.len(), 5, "the header, the rule, three headings");
    // Reading on: the outline's told, and marks the section.
    b.keys(s, "3J");
    assert!(b.host.refreshed.contains(&page));
    let laid = b.reader.sidebar_page(page, 40).unwrap();
    assert_eq!(laid.lines[4].spans[0].role, Role::Accent, "2.1 is being read");
    // j, Enter: there.
    b.reader.sidebar_key(&mut b.host, page, Key::Char('g'));
    b.reader.sidebar_key(&mut b.host, page, Key::Char('g'));
    b.reader.sidebar_key(&mut b.host, page, Key::Char('j'));
    b.reader.sidebar_key(&mut b.host, page, Key::Enter);
    b.pump();
    assert_eq!(b.page(s), 2);
    // o closes it; so does q, as Fenix closes the page.
    b.keys(s, "o");
    assert!(b.host.closed.contains(&page));
    assert_eq!(b.reader.sidebar_open(), None);
    b.keys(s, "o");
    let again = b.reader.sidebar_open().unwrap();
    b.reader.shown(&mut b.host, again, false);
    assert_eq!(b.reader.sidebar_open(), None, "gone with its pane");
}

#[test]
fn in_a_narrow_pane_or_set_over_the_outline_is_in_the_documents_pane() {
    let mut b = Bench::new();
    b.host.settings.insert("reader.sidebar".into(), "over".into());
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "o");
    let (page, _, _, beside) = b.host.pages.last().cloned().unwrap();
    assert_eq!(beside, None);
    b.reader.sidebar_key(&mut b.host, page, Key::Char('j'));
    b.reader.sidebar_key(&mut b.host, page, Key::Enter);
    b.pump();
    assert_eq!(b.page(s), 2);
    assert!(b.host.closed.contains(&page), "closed when it took you there");
}

#[test]
fn spc_r_t_picks_a_heading_by_name() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.reader.pick_heading(&mut b.host, s);
    b.pump();
    let (id, title, items) = b.host.picks.last().cloned().unwrap();
    assert_eq!(title, "Headings");
    assert_eq!(items[2], ("  2.1 Headers".to_string(), "p.4".to_string(), "3".to_string()));
    b.reader.picked(&mut b.host, id, Some("3".into()));
    b.pump();
    assert_eq!(b.page(s), 3);
}

#[test]
fn a_search_goes_to_the_first_match_from_here_and_n_goes_round() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "n");
    assert!(b.host.said("no search yet -- / searches the document"));
    b.keys(s, "4J/");
    let (id, label) = b.host.prompts.last().cloned().unwrap();
    assert_eq!(label, "search pdf");
    b.reader.prompted(&mut b.host, id, Some("packet".into()));
    b.pump();
    assert!(b.companion.got.iter().any(|r| matches!(r, Request::Search { query, match_case: false, .. } if query == "packet")), "smart case");
    assert_eq!(b.page(s), 7, "the first match at or after page 5, found in the second batch");
    assert!(b.host.status.as_deref().unwrap().contains("/packet 2/2"), "{:?}", b.host.status);
    // Highlighted on the page.
    let marks = &b.host.marks[&s];
    assert!(marks.iter().any(|m| m.role == Role::Bad), "the current match: {marks:?}");
    b.keys(s, "n");
    assert_eq!(b.page(s), 2, "round the end");
    b.keys(s, "N");
    assert_eq!(b.page(s), 7);
    // Esc hides the highlights.
    b.key(s, Press::named(NamedKey::Escape));
    assert!(b.host.marks[&s].is_empty());
    // A capital makes it exact; no match says so.
    let id = { b.keys(s, "/"); b.host.prompts.last().unwrap().0 };
    b.reader.prompted(&mut b.host, id, Some("Packet".into()));
    b.pump();
    assert!(b.companion.got.iter().any(|r| matches!(r, Request::Search { match_case: true, .. })));
    assert!(b.host.said("no matches for \"Packet\""));
}

#[test]
fn a_stale_searchs_results_are_dropped() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.reader.search(&mut b.host, s, "packet".into(), false);
    let old = std::mem::take(&mut b.host.sent);
    b.reader.search(&mut b.host, s, "nothing".into(), false);
    // The first search's answers, come late.
    b.host.sent = [old, std::mem::take(&mut b.host.sent)].concat();
    b.pump();
    assert!(b.host.said("no matches for \"nothing\""));
    assert!(!b.host.said("2 matches"), "{:?}", b.host.said);
}

#[test]
fn v_selects_from_the_top_line_j_goes_on_and_y_copies() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "v");
    assert!(b.host.said("VISUAL"));
    b.keys(s, "eell");
    assert!(b.host.marks[&s].iter().any(|m| m.role == Role::Accent), "drawn over the page");
    b.keys(s, "y");
    assert_eq!(b.host.copied.last().map(String::as_str), Some("Page 1 b"));
    assert!(b.host.said("copied 3 words"));
    assert_eq!(b.reader.selected(s), None);
}

#[test]
fn dragging_selects_and_a_click_on_a_link_follows_it() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    let at = |x: f32, y: f32| ((12.0 + x) as i32, (12.0 + y) as i32);
    let pointer = |b: &mut Bench, (x, y): (i32, i32), buttons: u8| {
        b.reader.pointer(&mut b.host, s, PointerEvent { x, y, buttons, mods: Modifiers { ctrl: false, alt: false, meta: false } });
        b.pump();
    };
    // From "Page" on the first line to "goes" on the second (the first
    // line's a link to page 5; press below it, on the second line).
    pointer(&mut b, at(73.0, 95.0), 1);
    pointer(&mut b, at(150.0, 97.0), 1);
    pointer(&mut b, at(150.0, 97.0), 0);
    assert!(b.host.said("selected -- y copies"));
    b.keys(s, "y");
    assert_eq!(b.host.copied.last().map(String::as_str), Some("and goes"));
    // The link on the first line: to page 5, and Ctrl-o comes back.
    pointer(&mut b, at(80.0, 78.0), 1);
    pointer(&mut b, at(80.0, 78.0), 0);
    assert_eq!(b.page(s), 4);
    b.key(s, Press::ctrl('o'));
    assert_eq!(b.page(s), 0);
}

#[test]
fn f_labels_the_links_in_sight_and_typing_one_follows_it() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    let paper = b.host.px(s, 12 + 72 + 2, 12 + 72 + 2);
    b.keys(s, "f");
    assert!(b.host.said("type a link's letters"));
    assert_ne!(b.host.px(s, 12 + 72 + 2, 12 + 72 + 2), paper, "a label drawn over the link");
    assert_eq!(b.host.marks[&s].iter().filter(|m| m.role == Role::Good).count(), 2);
    b.keys(s, "s");
    assert_eq!(b.host.urls, ["https://example.com/"], "the second link: the web");
    b.check_pixels(s);
    b.keys(s, "fa");
    assert_eq!(b.page(s), 4, "the first: page 5");
    // Esc lets them go.
    b.keys(s, "gg");
    b.keys(s, "f");
    b.key(s, Press::named(NamedKey::Escape));
    b.check_pixels(s);
}

#[test]
fn yp_copies_a_link_to_the_page_from_the_project() {
    let mut b = Bench::new();
    b.host.root = Some("C:\\work\\sat".into());
    let s = b.open("C:/work/sat/specs/133x0b2e2.pdf");
    b.keys(s, "3Jyp");
    assert_eq!(b.host.copied.last().map(String::as_str), Some("specs/133x0b2e2.pdf#page=4"));
    b.host.root = None;
    b.reader.copy_link(&mut b.host, s);
    assert_eq!(b.host.copied.last().map(String::as_str), Some("C:/work/sat/specs/133x0b2e2.pdf#page=4"));
}

#[test]
fn opening_an_open_pdf_again_brings_it_here_where_it_was() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "4J");
    b.reader.open(&mut b.host, "c:\\refs\\spec.pdf", OpenAt::Here);
    assert!(b.host.closed.contains(&s), "its old surface");
    let again = *b.reader.surfaces().last().unwrap();
    assert_ne!(again, s);
    b.size(again, PANE, 1.0);
    assert_eq!(b.page(again), 4);
    assert_eq!(b.companion.got.iter().filter(|r| matches!(r, Request::Open { .. })).count(), 1, "not opened twice");
    b.check_pixels(again);
}

#[test]
fn a_pdf_rewritten_on_disk_is_read_again_keeping_its_place() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "4J");
    let doc = b.companion.got.iter().find_map(|r| if let Request::Open { doc, .. } = r { Some(*doc) } else { None }).unwrap();
    let out = Reply::Changed { doc }.encode();
    b.reader.companion_bytes(&mut b.host, PROCESS, &out);
    b.pump();
    assert!(b.host.said("spec.pdf changed on disk -- reloaded"));
    assert_eq!(b.companion.got.iter().filter(|r| matches!(r, Request::Open { .. })).count(), 2);
    assert_eq!(b.page(s), 4);
    b.check_pixels(s);
}

#[test]
fn its_state_is_kept_for_the_session_and_put_back() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.keys(s, "6Gz0");
    let state = b.reader.page_state(s).unwrap();
    assert!(state.contains("\"path\":\"C:/refs/spec.pdf\"") && state.contains("\"page\":5") && state.contains("\"zoom\":\"100\""), "{state}");
    let mut next = Bench::new();
    assert!(next.reader.page_restore(&mut next.host, "pdf", &state));
    let s = *next.reader.surfaces().last().unwrap();
    next.size(s, PANE, 1.0);
    assert_eq!(next.page(s), 5);
    assert_eq!(next.reader.view_zoom(s), Some(Zoom::Percent(100)));
    assert!(!next.reader.page_restore(&mut next.host, "outline", "{}"));
    assert!(!next.reader.page_restore(&mut next.host, "pdf", "not json"));
}

#[test]
fn the_shelf_lists_yours_then_what_you_read_lately() {
    let mut b = Bench::new();
    b.reader.open_shelf(&mut b.host);
    assert!(b.host.said("no documents yet"));
    b.host.settings.insert("reader.documents".into(), r#"[["Space Packet Protocol", "C:/refs/133x0b2e2.pdf"], ["Notes", "docs/notes.md"]]"#.into());
    b.host.root = Some("C:/work/sat".into());
    b.open("C:/refs/read.pdf");
    b.open("C:/refs/133x0b2e2.pdf");
    b.reader.open_shelf(&mut b.host);
    let (id, title, items) = b.host.picks.last().cloned().unwrap();
    assert_eq!(title, "Documents");
    let labels: Vec<&str> = items.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(labels, ["Space Packet Protocol", "Notes", "recent \u{b7} read.pdf"], "listed once, in the first group it's in");
    assert_eq!(items[1].2, "C:/work/sat/docs/notes.md", "a project's paths from it");
    b.reader.picked(&mut b.host, id, Some(items[1].2.clone()));
    assert_eq!(b.host.files, ["C:/work/sat/docs/notes.md"], "not a PDF: Fenix opens it");
}

#[test]
fn fenix_opens_a_pdf_at_a_page_or_a_heading_and_searches_one() {
    let mut b = Bench::new();
    assert_eq!(b.reader.serve_open(&mut b.host, r#"{"path": "C:/refs/a.pdf", "page": 8}"#), Ok(String::new()));
    let s = *b.reader.surfaces().last().unwrap();
    b.size(s, PANE, 1.0);
    assert_eq!(b.page(s), 7);
    b.reader.serve_open(&mut b.host, r#"{"path": "C:/refs/b.pdf", "heading": "headers"}"#).unwrap();
    let s = *b.reader.surfaces().last().unwrap();
    b.size(s, PANE, 1.0);
    assert_eq!(b.page(s), 3, "through the outline");
    b.reader.serve_open(&mut b.host, r#"{"path": "C:/refs/b.pdf", "heading": "packets"}"#).unwrap();
    let s = *b.reader.surfaces().last().unwrap();
    b.size(s, PANE, 1.0);
    assert_eq!(b.page(s), 2, "\"2 Packets\", the document brought here");
    // No heading by that name: searched for, and the first match gone to.
    b.reader.serve_open(&mut b.host, r#"{"path": "C:/refs/c.pdf", "heading": "frame"}"#).unwrap();
    let s = *b.reader.surfaces().last().unwrap();
    b.size(s, PANE, 1.0);
    assert!(b.host.said("no heading named \"frame\" -- searching for it"), "{:?}", b.host.said);
    assert_eq!(b.page(s), 2);
    // Searched for Fenix, answered when every page has been.
    assert_eq!(b.reader.serve_search(&mut b.host, 9, r#"{"path": "C:/std/133.pdf", "query": "packet"}"#), None);
    b.pump();
    assert_eq!(b.host.answers.last(), Some(&(9, Ok(r#"[{"context":"a packet on 3","page":2},{"context":"a packet on 8","page":7}]"#.to_string()))));
    assert!(b.companion.got.iter().any(|r| matches!(r, Request::Close { .. })), "and let go");
    assert!(b.reader.serve_open(&mut b.host, "nope").is_err());
}

#[test]
fn spc_r_commands_act_on_the_pdf_read_last() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    assert_eq!(b.reader.target(), Some(s));
    b.reader.turn_page(&mut b.host, s, 1);
    b.pump();
    assert_eq!(b.page(s), 1);
    b.reader.ask_page(&mut b.host, s);
    let (id, label) = b.host.prompts.last().cloned().unwrap();
    assert_eq!(label, "go to page");
    b.reader.prompted(&mut b.host, id, Some("7".into()));
    b.pump();
    assert_eq!(b.page(s), 6);
    // Hidden, it's still the one acted on; Home's row brings it back.
    b.reader.shown(&mut b.host, s, false);
    assert_eq!(b.reader.target(), Some(s));
    assert_eq!(b.host.status, None, "the modeline lets go of it");
    b.reader.home_picked(&mut b.host, "C:/refs/spec.pdf");
    assert!(b.host.closed.contains(&s));
}

#[test]
fn the_companion_stopping_reopens_what_is_in_sight() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.reader.companion_ended(&mut b.host, PROCESS, Some(3));
    assert!(b.host.said("fenix-pdfium stopped (3)"));
    assert_eq!(b.host.spawned, 2, "started again");
    b.companion.got.clear();
    b.pump();
    assert!(matches!(b.companion.got.first(), Some(Request::Open { .. })));
    b.keys(s, "j");
    b.check_pixels(s);
}

#[test]
fn a_page_closed_lets_its_document_go() {
    let mut b = Bench::new();
    let s = b.open("C:/refs/spec.pdf");
    b.reader.closed(&mut b.host, s);
    b.pump();
    assert!(b.companion.got.iter().any(|r| matches!(r, Request::Close { .. })));
    assert!(b.reader.surfaces().is_empty());
    assert_eq!(b.reader.target(), None);
}

#[test]
fn a_hidpi_pane_is_drawn_at_its_pixels() {
    let mut b = Bench::new();
    b.reader.open(&mut b.host, "C:/refs/spec.pdf", OpenAt::Here);
    let s = *b.reader.surfaces().last().unwrap();
    b.size(s, (1272, 800), 2.0);
    assert_eq!(b.host.surfaces[&s].0, 1272, "the surface is the pane's pixels");
    // Gaps scale: 24 pixels at 2x.
    assert_eq!(b.host.px(s, 10, 10), GAP);
    assert_eq!(b.host.px(s, 24, 24), (0, 0, 0), "page 1's own top-left pixel");
    b.keys(s, "z0");
    let l = layout::layout(&[(612.0, 792.0); 10], Zoom::Percent(100), (1272.0, 800.0), 2.0, layout::PAGE_GAP);
    assert_eq!(l.pages[0].w, 1224.0, "100% is a point a point, times the scale");
    let _ = PAPER;
}
