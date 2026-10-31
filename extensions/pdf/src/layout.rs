//! How a document is laid out in its pane: every page one under the
//! other at one scale, the zoom that scale comes from, and where a
//! document was left (`reader.remember`). Fenix 1.x's reader's own, kept.

use std::collections::{BTreeMap, HashMap};

/// How a view is zoomed. `FitPage`/`FitWidth` follow the pane's size, so
/// a resize renders again; `Percent` doesn't.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    FitPage,
    FitWidth,
    /// Percent of the page's real size on this screen: 100 is one point
    /// per point, times the display's scale.
    Percent(u32),
}

/// The zoom a document opens at: the page's width fills the pane.
pub const DEFAULT_ZOOM: Zoom = Zoom::FitWidth;

/// `+` and `-` move between these, as browsers do.
pub const ZOOM_STEPS: &[u32] = &[25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400];

/// The zoom step after `current` (`in_ = true`) or before it: the next
/// one strictly larger or smaller, so a fitted 97% goes to 100% and then
/// 110%. Stays at either end.
pub fn step_zoom(current: u32, in_: bool) -> u32 {
    let (first, last) = (ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
    if in_ {
        ZOOM_STEPS.iter().copied().find(|&s| s > current).unwrap_or(last)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|&s| s < current).unwrap_or(first)
    }
}

/// How a zoom reads in the modeline.
pub fn zoom_label(zoom: Zoom) -> String {
    match zoom {
        Zoom::FitPage => "Fit page".to_string(),
        Zoom::FitWidth => "Fit width".to_string(),
        Zoom::Percent(percent) => format!("{percent}%"),
    }
}

/// Pixels between pages, and around them, before the display's scale --
/// `reader.page_gap`'s default.
pub const PAGE_GAP: f32 = 12.0;

/// Where one page sits in the document's column, in pixels from the
/// column's top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PageRect {
    /// The size it's rendered at.
    pub fn px(&self) -> (u32, u32) {
        (self.w.round().max(1.0) as u32, self.h.round().max(1.0) as u32)
    }
}

/// Every page of a document, one under the other, at one scale.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// Pixels per point.
    pub scale: f32,
    pub gap: f32,
    pub pages: Vec<PageRect>,
    /// The column's size: at least the pane's width, so a narrow page is
    /// centred in it.
    pub width: f32,
    pub height: f32,
}

/// Lays `pages` (sizes in points) out for `zoom` in a pane `pane` pixels
/// big, on a display of scale `dpi`, `gap` pixels (before scaling)
/// between and around them. Fitting fits the widest page (and, for fit
/// page, the tallest), so every page has one scale and a narrower one
/// sits centred. Everything lands on whole pixels.
pub fn layout(pages: &[(f32, f32)], zoom: Zoom, pane: (f32, f32), dpi: f32, gap: f32) -> Layout {
    let dpi = dpi.max(0.1);
    let gap = (gap.max(0.0) * dpi).round();
    let max_w = pages.iter().map(|p| p.0).fold(0.0f32, f32::max).max(1.0);
    let max_h = pages.iter().map(|p| p.1).fold(0.0f32, f32::max).max(1.0);
    let room = ((pane.0 - gap * 2.0).max(1.0), (pane.1 - gap * 2.0).max(1.0));
    let scale = match zoom {
        Zoom::FitWidth => room.0 / max_w,
        Zoom::FitPage => (room.0 / max_w).min(room.1 / max_h),
        Zoom::Percent(p) => p as f32 / 100.0 * dpi,
    }
    .max(0.01);
    let width = pane.0.max((max_w * scale).round() + gap * 2.0);
    let mut y = gap;
    let pages: Vec<PageRect> = pages
        .iter()
        .map(|&(w, h)| {
            let (w, h) = ((w * scale).round().max(1.0), (h * scale).round().max(1.0));
            let rect = PageRect { x: ((width - w) / 2.0).round(), y, w, h };
            y += h + gap;
            rect
        })
        .collect();
    let height = if pages.is_empty() { 0.0 } else { y };
    Layout { scale, gap, pages, width, height }
}

impl Layout {
    /// The page at column height `y` -- a gap counts as the page above it.
    pub fn page_at(&self, y: f32) -> usize {
        if self.pages.is_empty() {
            return 0;
        }
        self.pages.partition_point(|p| p.y <= y).saturating_sub(1)
    }

    /// The pages any of `top..top + h` shows.
    pub fn visible(&self, top: f32, h: f32) -> std::ops::Range<usize> {
        if self.pages.is_empty() {
            return 0..0;
        }
        let first = self.page_at(top);
        let last = self.page_at(top + h.max(1.0) - 1.0);
        first..last + 1
    }

    pub fn max_scroll(&self, pane: (f32, f32)) -> (f32, f32) {
        ((self.width - pane.0).max(0.0), (self.height - pane.1).max(0.0))
    }

    /// The scroll that puts `page`'s top at the pane's top, less the gap.
    pub fn top_of(&self, page: usize) -> f32 {
        self.pages.get(page).map(|p| (p.y - self.gap).max(0.0)).unwrap_or(0.0)
    }

    /// The page being read: the one across the middle of the pane, or
    /// the last when scrolled to the end.
    pub fn current(&self, scroll_y: f32, pane: (f32, f32)) -> usize {
        if self.pages.is_empty() {
            return 0;
        }
        if scroll_y >= self.max_scroll(pane).1 - 0.5 && self.height > pane.1 {
            return self.pages.len() - 1;
        }
        self.page_at(scroll_y + pane.1 / 2.0)
    }

    /// Where the pane's top is, as a page and how far down it (0..1), so
    /// the same place can be found in another layout of the document.
    pub fn anchor(&self, scroll_y: f32) -> (usize, f32) {
        let page = self.page_at(scroll_y);
        let Some(p) = self.pages.get(page) else { return (0, 0.0) };
        (page, (scroll_y - p.y) / p.h.max(1.0))
    }

    pub fn scroll_for(&self, anchor: (usize, f32)) -> f32 {
        self.pages.get(anchor.0).map(|p| p.y + anchor.1 * p.h).unwrap_or(0.0).max(0.0)
    }
}

/// Where a document was left, as `reader.remember` keeps it between
/// runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedPlace {
    pub page: u32,
    /// How far down the page the pane's top was, 0..1.
    pub frac: f32,
    pub zoom: Zoom,
    /// The document's page count, for "p. 38 / 212".
    pub pages: u32,
    pub marks: BTreeMap<char, (u32, f32)>,
}

pub fn zoom_text(zoom: Zoom) -> String {
    match zoom {
        Zoom::FitWidth => "width".to_string(),
        Zoom::FitPage => "page".to_string(),
        Zoom::Percent(p) => p.to_string(),
    }
}

/// `width`, `page` or a percentage, as settings and the places file
/// write a zoom.
pub fn parse_zoom(text: &str) -> Option<Zoom> {
    match text.trim() {
        "width" => Some(Zoom::FitWidth),
        "page" => Some(Zoom::FitPage),
        p => p.parse().ok().map(|p: u32| Zoom::Percent(p.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]))),
    }
}

/// The places file, `pdf_places.tsv` as Fenix 1.x wrote it: a line per
/// document, tab-separated -- path, page, how far down it, zoom, page
/// count, then marks as `a:3:0.25`. A line that doesn't read is skipped.
pub fn parse_places(text: &str) -> HashMap<String, SavedPlace> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let path = f.next()?.to_string();
            let page = f.next()?.parse().ok()?;
            let frac = f.next()?.parse().ok()?;
            let zoom = parse_zoom(f.next()?)?;
            let pages = f.next()?.parse().ok()?;
            let marks = f
                .next()
                .unwrap_or("")
                .split(',')
                .filter_map(|m| {
                    let mut p = m.split(':');
                    let c = p.next()?.chars().next()?;
                    Some((c, (p.next()?.parse().ok()?, p.next()?.parse().ok()?)))
                })
                .collect();
            Some((path, SavedPlace { page, frac, zoom, pages, marks }))
        })
        .collect()
}

pub fn format_places(places: &HashMap<String, SavedPlace>) -> String {
    let mut lines: Vec<String> = places
        .iter()
        .map(|(path, p)| {
            let marks: Vec<String> = p.marks.iter().map(|(c, (page, frac))| format!("{c}:{page}:{frac:.3}")).collect();
            format!("{}\t{}\t{:.3}\t{}\t{}\t{}", path, p.page, p.frac, zoom_text(p.zoom), p.pages, marks.join(","))
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

/// The same file however its path is written: `\` or `/`, and a drive's
/// letter in either case on Windows -- so a place or a shelf entry finds
/// the document it's about.
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.trim_start_matches(r"\\?\").replace('\\', "/");
        if p.as_bytes().get(1) == Some(&b':') {
            p.to_lowercase()
        } else {
            p
        }
    };
    norm(a) == norm(b)
}

/// A file's name, from its full path.
pub fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

pub fn looks_like_pdf(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_go_to_the_next_step_and_stop_at_the_ends() {
        assert_eq!(step_zoom(100, true), 110);
        assert_eq!(step_zoom(97, true), 100, "a fitted 97% goes to the next real step");
        assert_eq!(step_zoom(97, false), 90);
        assert_eq!(step_zoom(400, true), 400);
        assert_eq!(step_zoom(25, false), 25);
        assert_eq!(step_zoom(1000, false), 400);
    }

    fn letter(n: usize) -> Vec<(f32, f32)> {
        vec![(612.0, 792.0); n]
    }

    #[test]
    fn fit_width_stacks_the_pages_one_under_the_other() {
        let l = layout(&letter(3), Zoom::FitWidth, (636.0, 400.0), 1.0, PAGE_GAP);
        assert_eq!(l.scale, 1.0);
        assert_eq!(l.pages[0], PageRect { x: 12.0, y: 12.0, w: 612.0, h: 792.0 });
        assert_eq!(l.pages[1].y, 12.0 + 792.0 + 12.0);
        assert_eq!(l.height, 12.0 + 3.0 * (792.0 + 12.0));
        assert_eq!(l.width, 636.0);
    }

    #[test]
    fn a_narrower_page_is_centred_at_the_same_scale() {
        let l = layout(&[(612.0, 792.0), (306.0, 396.0)], Zoom::FitWidth, (636.0, 400.0), 1.0, PAGE_GAP);
        assert_eq!((l.pages[1].w, l.pages[1].x), (306.0, 165.0));
    }

    #[test]
    fn fit_page_fits_the_tallest_page_and_a_percent_ignores_the_pane() {
        let l = layout(&letter(2), Zoom::FitPage, (1000.0, 420.0), 1.0, PAGE_GAP);
        assert_eq!(l.pages[0].h, 396.0);
        let l = layout(&letter(2), Zoom::Percent(200), (100.0, 100.0), 1.5, PAGE_GAP);
        assert_eq!(l.pages[0].w, 1836.0);
        assert!(l.width > 100.0, "wider than the pane, so it pans");
    }

    #[test]
    fn what_shows_and_what_is_being_read() {
        let l = layout(&letter(5), Zoom::FitWidth, (636.0, 400.0), 1.0, PAGE_GAP);
        assert_eq!(l.visible(0.0, 400.0), 0..1);
        assert_eq!(l.visible(700.0, 400.0), 0..2);
        assert_eq!(l.current(0.0, (636.0, 400.0)), 0);
        assert_eq!(l.current(700.0, (636.0, 400.0)), 1, "page 2 crosses the middle");
        let end = l.max_scroll((636.0, 400.0)).1;
        assert_eq!(l.current(end, (636.0, 400.0)), 4, "the last page at the end");
        assert_eq!(l.top_of(2), l.pages[2].y - 12.0);
    }

    #[test]
    fn an_anchor_finds_the_same_place_at_another_zoom() {
        let small = layout(&letter(5), Zoom::FitWidth, (336.0, 400.0), 1.0, PAGE_GAP);
        let big = layout(&letter(5), Zoom::FitWidth, (1236.0, 400.0), 1.0, PAGE_GAP);
        let y = small.pages[3].y + small.pages[3].h / 4.0;
        let anchor = small.anchor(y);
        assert_eq!(anchor.0, 3);
        assert!((anchor.1 - 0.25).abs() < 0.01);
        let again = big.scroll_for(anchor);
        assert_eq!(big.page_at(again), 3);
    }

    #[test]
    fn places_read_back_what_was_written_as_1x_wrote_them() {
        let mut places = HashMap::new();
        let mut marks = BTreeMap::new();
        marks.insert('a', (3, 0.25));
        places.insert("C:/refs/a b.pdf".to_string(), SavedPlace { page: 37, frac: 0.5, zoom: Zoom::Percent(125), pages: 212, marks });
        places.insert("/x.pdf".to_string(), SavedPlace { page: 0, frac: 0.0, zoom: Zoom::FitWidth, pages: 3, marks: Default::default() });
        let text = format_places(&places);
        assert_eq!(parse_places(&text), places);
        assert!(parse_places("garbage\nC:/y.pdf\tnope").is_empty(), "lines that don't read are skipped");
        // A line from Fenix 1.x's own file.
        let old = parse_places("C:\\refs\\133x0b2e2.pdf\t37\t0.000\twidth\t212\ta:3:0.250\n");
        assert_eq!(old["C:\\refs\\133x0b2e2.pdf"].marks[&'a'], (3, 0.25));
    }

    #[test]
    fn paths_are_the_same_however_they_are_written() {
        assert!(same_path(r"C:\refs\a.pdf", "c:/refs/a.pdf"));
        assert!(same_path(r"\\?\C:\refs\a.pdf", "C:/refs/a.pdf"));
        assert!(!same_path("/home/A.pdf", "/home/a.pdf"), "case counts off Windows' drives");
        assert_eq!(file_name(r"C:\refs\spec.pdf"), "spec.pdf");
        assert!(looks_like_pdf("x.PDF") && !looks_like_pdf("x.md"));
    }
}
