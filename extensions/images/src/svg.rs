//! SVG, by resvg: drawn again at every zoom, only the part in sight, so
//! it's sharp however close you look. Its text is set in the system's
//! fonts, which the sandbox has at `/fonts/<n>` (`permissions.fonts`);
//! the same fonts write the details line (`text`).

use std::sync::{Arc, OnceLock};

use resvg::tiny_skia;
use resvg::usvg::{self, fontdb};

use crate::render::{self, Backdrop};
use crate::view::View;

/// Files of these families are read, by how their files are named: the
/// usual sans-serifs, serifs and monospaced faces of Windows, macOS and
/// Linux.
const FAMILIES: &[&str] = &[
    "dejavusans", "dejavuserif", "liberationsans", "liberationserif", "liberationmono", "arial", "times", "cour", "verdana", "georgia", "tahoma", "trebuc", "segoeui", "calibri", "cambria", "consola", "helvetica", "sfns", "menlo", "notosans-", "notoserif-", "notosansmono-", "roboto", "opensans", "ubuntu", "cantarell", "inter", "freesans", "freeserif", "freemono",
];
/// A face bigger than this is left out (a CJK font is tens of MiB).
const BIGGEST_FONT: u64 = 2 << 20;
/// The most read in all: room for the pictures.
const FONT_BUDGET: u64 = 8 << 20;

/// What the generic families are, in order of preference.
const SANS: &[&str] = &["Segoe UI", "DejaVu Sans", "Liberation Sans", "Arial", "Helvetica", "Noto Sans", "Ubuntu", "Cantarell", "Roboto", "Open Sans", "Inter", "FreeSans", "Verdana", "Tahoma"];
const SERIF: &[&str] = &["Times New Roman", "DejaVu Serif", "Liberation Serif", "Georgia", "Cambria", "Noto Serif", "FreeSerif"];
const MONO: &[&str] = &["Consolas", "DejaVu Sans Mono", "Liberation Mono", "Menlo", "Courier New", "Noto Sans Mono", "FreeMono"];

static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

/// The fonts, read the first time they're needed.
pub fn fonts() -> Arc<fontdb::Database> {
    FONTS.get_or_init(|| Arc::new(load_fonts())).clone()
}

fn load_fonts() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    let mut files = Vec::new();
    // Each of the system's font folders, and the folders in them: on
    // Linux the faces are a few down (`/usr/share/fonts/truetype/dejavu`).
    let mut folders: Vec<(std::path::PathBuf, u32)> = Vec::new();
    for n in 0.. {
        let root = std::path::PathBuf::from(format!("/fonts/{n}"));
        if !root.is_dir() {
            break;
        }
        folders.push((root, 0));
    }
    while let Some((dir, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if depth < 4 {
                    folders.push((entry.path(), depth + 1));
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase().replace([' ', '_'], "");
            let font = name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc");
            let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            if let Some(rank) = FAMILIES.iter().position(|f| name.replace('-', "").starts_with(&f.replace('-', "")) && (!f.ends_with('-') || name.starts_with(f))).filter(|_| font && size <= BIGGEST_FONT) {
                // Regular faces first, so a tight budget still has them.
                let plain = !["bold", "italic", "oblique", "light", "black", "thin", "condensed"].iter().any(|s| name.contains(s));
                files.push((!plain, rank, size, entry.path()));
            }
        }
    }
    files.sort();
    let mut read = 0;
    for (_, _, size, path) in files {
        if read + size > FONT_BUDGET {
            continue;
        }
        // Read here: fontdb would map the file, which the sandbox can't.
        if let Ok(bytes) = std::fs::read(&path) {
            read += size;
            db.load_font_data(bytes);
        }
    }
    let has = |db: &fontdb::Database, name: &str| db.faces().any(|f| f.families.iter().any(|(n, _)| n == name));
    let first = |db: &fontdb::Database, names: &[&str]| names.iter().find(|n| has(db, n)).map(|n| n.to_string());
    let any = db.faces().next().and_then(|f| f.families.first()).map(|(n, _)| n.clone());
    if let Some(sans) = first(&db, SANS).or(any.clone()) {
        db.set_sans_serif_family(sans);
    }
    if let Some(serif) = first(&db, SERIF).or(any.clone()) {
        db.set_serif_family(serif);
    }
    if let Some(mono) = first(&db, MONO).or(any) {
        db.set_monospace_family(mono);
    }
    db
}

fn options(fonts: Arc<fontdb::Database>) -> usvg::Options<'static> {
    let family = fonts.faces().next().map(|_| "sans-serif".to_string()).unwrap_or_else(|| "Times New Roman".into());
    usvg::Options { fontdb: fonts, font_family: family, ..Default::default() }
}

/// An SVG, parsed.
pub struct Svg {
    tree: usvg::Tree,
}

impl Svg {
    /// Its markup (gzipped too). The fonts are read only for one with
    /// text in it.
    pub fn parse(bytes: &[u8]) -> Result<Svg, String> {
        let gzipped = bytes.starts_with(&[0x1f, 0x8b]);
        let fonts = if gzipped || bytes.windows(5).any(|w| w == b"<text") { fonts() } else { Arc::new(fontdb::Database::new()) };
        let tree = usvg::Tree::from_data(bytes, &options(fonts)).map_err(|e| format!("not an SVG it draws: {e}"))?;
        Ok(Svg { tree })
    }

    /// Its size, in its own pixels.
    pub fn size(&self) -> (u32, u32) {
        let s = self.tree.size();
        (s.width().ceil().max(1.0) as u32, s.height().ceil().max(1.0) as u32)
    }

    /// Drawn into `bgra` (the frame, `view.pane` big) where `view` places
    /// it, over the checks: only what's in sight.
    pub fn draw(&self, bgra: &mut [u8], view: &View, back: &Backdrop) {
        render::clear(bgra, back.background);
        let Some(rect) = view.covers() else { return };
        let (x, y, w, h) = rect;
        let origin = view.origin();
        render::checks(bgra, view.pane.0, rect, origin, back);
        let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else { return };
        let s = view.scale();
        let transform = tiny_skia::Transform::from_row(s, 0.0, 0.0, s, origin.0 - x as f32, origin.1 - y as f32);
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());
        render::premultiplied(bgra, view.pane.0, rect, pixmap.data());
    }
}

/// A line of text, `size` pixels high, in `rgb`, as premultiplied RGBA
/// `width` x `height`, set from `x`: starting there, or its `middle` or
/// `end` there (SVG's `text-anchor`). None when there are no fonts to
/// set it in.
pub fn text(line: &str, width: u32, height: u32, x: f32, anchor: Option<&str>, size: f32, rgb: (u8, u8, u8)) -> Option<Vec<u8>> {
    let fonts = fonts();
    if fonts.is_empty() {
        return None;
    }
    let escaped = line.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let anchor = anchor.unwrap_or("start");
    let baseline = (height as f32 + size * 0.7) / 2.0;
    let markup = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><text x="{x}" y="{baseline}" font-family="sans-serif" font-size="{size}" fill="#{:02x}{:02x}{:02x}" text-anchor="{anchor}">{escaped}</text></svg>"##,
        rgb.0, rgb.1, rgb.2
    );
    let tree = usvg::Tree::from_str(&markup, &options(fonts)).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    Some(pixmap.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: Backdrop = Backdrop { background: (0, 0, 0), checks: [(100, 100, 100), (200, 200, 200)], check: 4 };

    #[test]
    fn it_is_drawn_sharp_at_the_size_shown() {
        // A 10 x 10 SVG, its left half red: fitted to a 100 x 50 pane it's
        // drawn 50 x 50, in the middle, its edge between pixels 49 and 50.
        let svg = Svg::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="5" height="10" fill="red"/></svg>"#).unwrap();
        assert_eq!(svg.size(), (10, 10));
        let mut v = View::new(svg.size(), 1.0, true);
        v.set_pane(100, 50, 1.0);
        let mut bgra = vec![0; 100 * 50 * 4];
        svg.draw(&mut bgra, &v, &BACK);
        let px = |x: usize, y: usize| &bgra[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
        assert_eq!(px(10, 10), &[0, 0, 0, 255], "the background, beside it");
        assert_eq!(px(26, 10), &[0, 0, 255, 255], "red");
        assert_eq!(px(49, 25), &[0, 0, 255, 255], "red to its edge");
        assert!(px(50, 25)[2] < 250, "then the checks: {:?}", px(50, 25));
    }

    #[test]
    fn markup_that_isnt_svg_is_said() {
        assert!(Svg::parse(b"<html/>").err().unwrap().starts_with("not an SVG it draws"));
    }
}
