//! One picture on a surface: the file it came from, how it's shown, and
//! its frame drawn -- the picture, and under it the details line.

use crate::decode::{self, Content, Picture};
use crate::folder;
use crate::format::{self, Details};
use crate::render::{self, Backdrop};
use crate::svg;
use crate::view::{View, STEP};

/// What a key, the wheel or a command asks of a viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    ZoomIn,
    ZoomOut,
    /// The wheel: so many steps, about this point of the pane.
    ZoomAt(i32, (f32, f32)),
    Fit,
    Actual,
    Pan(i32, i32),
    Next,
    Previous,
    Source,
    Play,
    Frame,
    Details,
    Reload,
}

impl Action {
    /// What a key on its surface does.
    pub fn of_key(c: char) -> Option<Action> {
        Some(match c {
            '+' => Action::ZoomIn,
            '-' => Action::ZoomOut,
            '0' | '=' => Action::Fit,
            '1' => Action::Actual,
            'h' => Action::Pan(-1, 0),
            'l' => Action::Pan(1, 0),
            'k' => Action::Pan(0, -1),
            'j' => Action::Pan(0, 1),
            ']' => Action::Next,
            '[' => Action::Previous,
            's' => Action::Source,
            'p' => Action::Play,
            '.' => Action::Frame,
            'd' => Action::Details,
            'r' => Action::Reload,
            _ => return None,
        })
    }
}

/// The colours it's drawn in: the editor theme's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub background: (u8, u8, u8),
    pub text: (u8, u8, u8),
    pub muted: (u8, u8, u8),
    pub panel: (u8, u8, u8),
    pub warn: (u8, u8, u8),
}

pub struct Viewer {
    /// `image`, or `preview` beside the code that names it.
    pub kind: &'static str,
    /// The file: under `/project` when it's there, else the full path
    /// Fenix gave.
    pub path: String,
    /// Whether `path` is under `/project`, where its folder may be
    /// listed for `]` and `[`.
    pub in_project: bool,
    /// Its full path -- the one Fenix gave, or beside it after `]` -- kept
    /// for the session.
    pub full: String,
    pub bytes: u64,
    pub format: Option<format::Format>,
    pub picture: Option<Picture>,
    pub error: Option<String>,
    pub view: View,
    /// A zoom and middle from the session, put back once the pane is
    /// known.
    pub pending: Option<(Option<f32>, (f32, f32))>,
    /// The surface's size, in its pixels, and how dense the screen is.
    pub pane: (u32, u32, f32),
    pub shown: bool,
    pub playing: bool,
    pub timer: Option<u64>,
    /// Where the pointer was, while a button's held: a drag.
    pub drag: Option<(i32, i32)>,
    pub details: bool,
    /// Its place among its folder's pictures, counting from 1.
    pub place: Option<(usize, usize)>,
    /// Its pixels, BGRA, kept between frames.
    frame: Vec<u8>,
    /// The details line as last drawn: what it said, and its pixels.
    strip: Option<(String, Vec<u8>)>,
}

/// The details line's height, in points.
const STRIP: f32 = 24.0;
/// Its letters', in points.
const STRIP_TEXT: f32 = 12.5;

impl Viewer {
    pub fn new(kind: &'static str, full: &str, path: String) -> Viewer {
        Viewer {
            kind,
            in_project: path.starts_with("/project/"),
            path,
            full: full.to_string(),
            bytes: 0,
            format: None,
            picture: None,
            error: None,
            view: View::new((1, 1), 1.0, false),
            pending: None,
            pane: (0, 0, 1.0),
            shown: true,
            playing: true,
            timer: None,
            drag: None,
            details: true,
            place: None,
            frame: Vec::new(),
            strip: None,
        }
    }

    pub fn name(&self) -> &str {
        folder::split(&self.path).1
    }

    /// `bytes`, the file's, decoded and shown fitted. A file that isn't a
    /// picture it reads is shown as why.
    pub fn load(&mut self, bytes: Vec<u8>) {
        self.bytes = bytes.len() as u64;
        self.format = format::detect(self.name(), &bytes);
        self.picture = None;
        self.error = None;
        self.strip = None;
        let picture = match self.format {
            Some(f) => decode::decode(f, bytes),
            None => Err("not a picture it reads: PNG, JPEG, GIF, WebP, BMP or SVG".into()),
        };
        match picture {
            Ok(p) => {
                let grow = matches!(p.content, Content::Vector(_));
                let mut view = View::new(p.pixels(), p.reduced as f32, grow);
                view.set_pane(self.view.pane.0, self.view.pane.1, self.view.dpi);
                self.view = view;
                self.picture = Some(p);
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Its pixels let go while it's out of sight: `load` brings them back.
    pub fn release(&mut self) {
        self.picture = None;
        self.frame = Vec::new();
        self.strip = None;
    }

    /// Whether it's a picture that moves.
    pub fn moves(&self) -> bool {
        matches!(self.picture.as_ref().map(|p| &p.content), Some(Content::Moving(_)))
    }

    /// The frame shown now: how long for.
    pub fn delay(&self) -> Option<u32> {
        match self.picture.as_ref().map(|p| &p.content) {
            Some(Content::Moving(a)) => Some(a.delay),
            _ => None,
        }
    }

    /// The details line's height, in the surface's pixels: none when it's
    /// hidden or the pane's too small for it.
    fn strip_height(&self) -> u32 {
        let h = (STRIP * self.pane.2).round() as u32;
        if self.details && self.pane.1 > h * 3 {
            h
        } else {
            0
        }
    }

    /// The surface is `width` x `height` now.
    pub fn resized(&mut self, width: u32, height: u32, dpi: f32) {
        self.pane = (width, height, dpi);
        let strip = self.strip_height();
        self.view.set_pane(width, height - strip, dpi);
        if let Some((zoom, center)) = self.pending.take() {
            self.view.restore(zoom, center);
        }
        self.strip = None;
    }

    /// The details line hidden or shown: the picture gets its room.
    pub fn toggle_details(&mut self) {
        self.details = !self.details;
        let (w, h, dpi) = self.pane;
        let strip = self.strip_height();
        self.view.set_pane(w, h - strip, dpi);
        self.strip = None;
    }

    pub fn act_view(&mut self, action: Action) {
        match action {
            Action::ZoomIn => self.view.zoom_by(STEP, None),
            Action::ZoomOut => self.view.zoom_by(1.0 / STEP, None),
            Action::ZoomAt(steps, at) => self.view.zoom_by(STEP.powi(steps), Some(at)),
            Action::Fit => self.view.fit(),
            Action::Actual => self.view.actual_size(),
            Action::Pan(x, y) => self.view.pan(x, y),
            _ => {}
        }
    }

    pub fn details_of(&self) -> Details {
        let frame = match self.picture.as_ref().map(|p| &p.content) {
            Some(Content::Moving(a)) => Some((a.index + 1, a.count, self.playing)),
            _ => None,
        };
        let picture = self.picture.as_ref();
        Details {
            name: self.name().to_string(),
            format: self.format,
            size: picture.map(|p| p.size).unwrap_or_default(),
            bytes: self.bytes,
            percent: self.view.percent(),
            fit: self.view.zoom == crate::view::Zoom::Fit,
            place: self.place,
            frame,
            reduced: picture.map(|p| p.reduced).filter(|r| *r > 1),
        }
    }

    /// Its frame, drawn: the picture over the theme, and the details line.
    /// BGRA, `pane` big; none before the pane's known.
    pub fn draw(&mut self, colors: &Colors) -> Option<&[u8]> {
        let (w, h, dpi) = self.pane;
        if w == 0 || h == 0 {
            return None;
        }
        self.frame.resize(w as usize * h as usize * 4, 0);
        let strip = self.strip_height();
        let back: Backdrop = render::backdrop(colors.background, colors.text, dpi);
        let area = (w as usize * (h - strip) as usize) * 4;
        let picture_px = &mut self.frame[..area];
        match self.picture.as_mut() {
            Some(p) => {
                if let Content::Vector(svg) = &p.content {
                    svg.draw(picture_px, &self.view, &back);
                } else {
                    let level = render::level_for(self.view.scale());
                    if let Some(img) = p.level(level) {
                        render::raster(picture_px, &self.view, img, level, &back);
                    }
                }
            }
            None => {
                render::clear(picture_px, colors.background);
                if let Some(why) = &self.error {
                    let size = 14.0 * dpi;
                    let line = (size * 2.0) as u32;
                    if let Some(text) = svg::text(why, w, line.min(h - strip), w as f32 / 2.0, Some("middle"), size, colors.warn) {
                        let y = (h - strip).saturating_sub(line) / 2;
                        render::premultiplied(picture_px, w, (0, y, w, line.min(h - strip)), &text);
                    }
                }
            }
        }
        if strip > 0 {
            self.strip_for(w, strip, dpi, colors);
            if let Some((_, bgra)) = self.strip.as_ref().filter(|(_, b)| b.len() == self.frame.len() - area) {
                self.frame[area..].copy_from_slice(bgra);
            }
        }
        Some(&self.frame)
    }

    /// The details line: what it is on the left, how it's shown on the
    /// right, in the theme's panel colour. Drawn again only when it says
    /// something else.
    fn strip_for(&mut self, w: u32, h: u32, dpi: f32, colors: &Colors) {
        let d = self.details_of();
        let (mut what, how) = (d.what(), d.how());
        let key = format!("{what}\n{how}\n{w}x{h}");
        if self.strip.as_ref().map(|(k, _)| k) != Some(&key) {
            let mut bgra = vec![0u8; w as usize * h as usize * 4];
            render::clear(&mut bgra, colors.panel);
            let size = STRIP_TEXT * dpi;
            let pad = 8.0 * dpi;
            // About how wide its letters are, to leave the right half room.
            let wide = |s: &str| s.chars().count() as f32 * size * 0.55;
            let room = w as f32 - pad * 3.0 - wide(&how);
            if wide(&what) > room {
                let keep = (room / (size * 0.55)).max(0.0) as usize;
                what = what.chars().take(keep.saturating_sub(1)).collect::<String>() + "\u{2026}";
            }
            if let Some(text) = svg::text(&what, w, h, pad, None, size, colors.text) {
                render::premultiplied(&mut bgra, w, (0, 0, w, h), &text);
            }
            if room > 0.0 {
                if let Some(text) = svg::text(&how, w, h, w as f32 - pad, Some("end"), size, colors.muted) {
                    render::premultiplied(&mut bgra, w, (0, 0, w, h), &text);
                }
            }
            self.strip = Some((key, bgra));
        }
    }

    /// What's kept in the session to open it again as it was.
    pub fn state(&self) -> String {
        let (zoom, center) = self.view.saved();
        serde_json::json!({ "path": self.path, "full": self.full, "bytes": self.bytes, "zoom": zoom, "center": [center.0, center.1], "details": self.details }).to_string()
    }
}

/// What `state` kept.
#[derive(Debug, serde::Deserialize)]
pub struct Saved {
    pub path: String,
    #[serde(default)]
    pub full: String,
    /// Its size then: under `/project`, a file of another size is
    /// another file (the project changed).
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub zoom: Option<f32>,
    #[serde(default = "middle")]
    pub center: (f32, f32),
    #[serde(default = "yes")]
    pub details: bool,
}

fn middle() -> (f32, f32) {
    (0.5, 0.5)
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLORS: Colors = Colors { background: (10, 10, 10), text: (250, 250, 250), muted: (150, 150, 150), panel: (40, 40, 40), warn: (250, 200, 0) };

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn keys_do_what_the_diagram_viewers_do() {
        assert_eq!(Action::of_key('+'), Some(Action::ZoomIn));
        assert_eq!(Action::of_key('0'), Some(Action::Fit));
        assert_eq!(Action::of_key('='), Some(Action::Fit));
        assert_eq!(Action::of_key('1'), Some(Action::Actual));
        assert_eq!(Action::of_key('h'), Some(Action::Pan(-1, 0)));
        assert_eq!(Action::of_key(']'), Some(Action::Next));
        assert_eq!(Action::of_key('q'), None, "q is Fenix's: it closes the surface");
    }

    #[test]
    fn a_picture_is_drawn_over_the_theme_with_its_details_below() {
        let mut v = Viewer::new("image", "/home/me/p/red.png", "/project/red.png".into());
        v.load(png(40, 20));
        assert_eq!(v.format, Some(format::Format::Png));
        v.resized(200, 200, 1.0);
        assert_eq!(v.view.pane, (200, 176), "the details line takes 24 points");
        let frame = v.draw(&COLORS).unwrap().to_vec();
        assert_eq!(frame.len(), 200 * 200 * 4);
        let px = |x: usize, y: usize| &frame[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4];
        assert_eq!(px(100, 88), &[0, 0, 200, 255], "the picture, in the middle, at its size");
        assert_eq!(px(5, 5), &[10, 10, 10, 255], "the theme's background around it");
        assert_eq!(px(0, 199), &[40, 40, 40, 255], "the details line's panel");
        assert!(v.details_of().what().contains("40 \u{d7} 20"));
        v.toggle_details();
        assert_eq!(v.view.pane, (200, 200));
    }

    #[test]
    fn what_isnt_a_picture_says_why() {
        let mut v = Viewer::new("image", "/x/notes.png", "/x/notes.png".into());
        v.load(b"hello".to_vec());
        assert!(v.picture.is_none());
        assert!(v.error.as_deref().unwrap().starts_with("not a picture it reads"));
        assert!(!v.in_project);
        v.resized(50, 100, 1.0);
        assert!(v.draw(&COLORS).is_some(), "drawn all the same: the background, and why");
    }

    #[test]
    fn the_session_brings_back_its_zoom() {
        let mut v = Viewer::new("image", "/home/me/p/red.png", "/project/red.png".into());
        v.load(png(400, 400));
        v.resized(200, 224, 1.0);
        v.act_view(Action::Actual);
        v.act_view(Action::Pan(1, 1));
        let saved: Saved = serde_json::from_str(&v.state()).unwrap();
        assert_eq!((saved.path.as_str(), saved.full.as_str(), saved.zoom), ("/project/red.png", "/home/me/p/red.png", Some(1.0)));
        let mut w = Viewer::new("image", &saved.full, saved.path);
        w.load(png(400, 400));
        w.pending = Some((saved.zoom, saved.center));
        w.resized(200, 224, 2.0);
        assert_eq!(w.view.percent(), 100, "the same percent on a denser screen");
    }
}
