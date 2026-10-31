//! A picture drawn into its pane's pixels, as `View` places it: the
//! theme's background around it, a checkerboard under what's
//! transparent, and its pixels resampled -- smoothly up to three times
//! its size, as squares beyond that (to see them), and from a picture
//! halved as often as it takes when it's shown smaller, so nothing
//! shimmers.

use crate::view::View;

/// From this many pane pixels to a picture pixel, each is drawn as a
/// square rather than blended with its neighbours.
const SQUARES_FROM: f32 = 3.0;

/// Pixels, RGBA, alpha not premultiplied, row after row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub px: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32, px: Vec<u8>) -> Rgba {
        debug_assert_eq!(px.len(), width as usize * height as usize * 4);
        Rgba { width, height, px }
    }

    /// Half as wide and high, each pixel the average of four (weighted by
    /// their alpha, so a transparent one's colour doesn't bleed in).
    pub fn halved(&self) -> Rgba {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut px = vec![0u8; w as usize * h as usize * 4];
        let src = |x: u32, y: u32| {
            let at = (y.min(self.height - 1) as usize * self.width as usize + x.min(self.width - 1) as usize) * 4;
            &self.px[at..at + 4]
        };
        for y in 0..h {
            for x in 0..w {
                let quad = [src(2 * x, 2 * y), src(2 * x + 1, 2 * y), src(2 * x, 2 * y + 1), src(2 * x + 1, 2 * y + 1)];
                let a: u32 = quad.iter().map(|p| p[3] as u32).sum();
                let at = (y as usize * w as usize + x as usize) * 4;
                for c in 0..3 {
                    let sum: u32 = quad.iter().map(|p| p[c] as u32 * p[3] as u32).sum();
                    px[at + c] = sum.checked_div(a).unwrap_or(0) as u8;
                }
                px[at + 3] = ((a + 2) / 4) as u8;
            }
        }
        Rgba { width: w, height: h, px }
    }
}

/// What's behind the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backdrop {
    /// Around it: the theme's background.
    pub background: (u8, u8, u8),
    /// Under its transparent parts: two shades of it, in squares.
    pub checks: [(u8, u8, u8); 2],
    /// A square's side, in pane pixels.
    pub check: u32,
}

/// The backdrop for a theme: its background, and checks a little towards
/// its text's colour -- seen, without shouting over the picture.
pub fn backdrop(background: (u8, u8, u8), text: (u8, u8, u8), dpi: f32) -> Backdrop {
    let mix = |t: f32| {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        (m(background.0, text.0), m(background.1, text.1), m(background.2, text.2))
    };
    Backdrop { background, checks: [mix(0.06), mix(0.16)], check: (8.0 * dpi.max(1.0)).round() as u32 }
}

/// A frame's pixels: BGRA, opaque, `width` x `height`.
pub fn clear(bgra: &mut [u8], (r, g, b): (u8, u8, u8)) {
    for px in bgra.chunks_exact_mut(4) {
        px.copy_from_slice(&[b, g, r, 255]);
    }
}

/// The checks under the rectangle (x, y, w, h) of a frame `width` wide,
/// their squares counted from (`ox`, `oy`) so they move with the picture.
pub fn checks(bgra: &mut [u8], width: u32, (x, y, w, h): (u32, u32, u32, u32), (ox, oy): (f32, f32), back: &Backdrop) {
    let side = back.check.max(1) as i64;
    let (ox, oy) = (ox.round() as i64, oy.round() as i64);
    for row in y..y + h {
        let ry = (row as i64 - oy).div_euclid(side);
        let line = &mut bgra[(row as usize * width as usize + x as usize) * 4..(row as usize * width as usize + (x + w) as usize) * 4];
        for (i, px) in line.chunks_exact_mut(4).enumerate() {
            let rx = (x as i64 + i as i64 - ox).div_euclid(side);
            let (r, g, b) = back.checks[((rx + ry) & 1) as usize];
            px.copy_from_slice(&[b, g, r, 255]);
        }
    }
}

/// Where each pane column (or row) of the picture samples it: the two
/// source pixels and how much of the second, out of 256.
fn taps(start: u32, len: u32, origin: f32, scale: f32, size: u32, squares: bool) -> Vec<(u32, u32, u32)> {
    let last = size.saturating_sub(1);
    (start..start + len)
        .map(|d| {
            let s = (d as f32 + 0.5 - origin) / scale;
            if squares {
                let i = (s.floor().max(0.0) as u32).min(last);
                (i, i, 0)
            } else {
                let s = (s - 0.5).max(0.0);
                let i = (s.floor() as u32).min(last);
                let f = ((s - i as f32) * 256.0).round().clamp(0.0, 256.0) as u32;
                (i, (i + 1).min(last), f)
            }
        })
        .collect()
}

/// Of the picture halved `level` times, the one to draw at `scale` from:
/// as many halvings as keep it no smaller than it's shown.
pub fn level_for(scale: f32) -> u32 {
    let mut level = 0;
    let mut s = scale;
    while s < 0.5 && level < 12 {
        s *= 2.0;
        level += 1;
    }
    level
}

/// `image` drawn into `bgra` (the frame, `view.pane` big) where `view`
/// places it, over the checks. `image` is the picture halved `level`
/// times (`level_for`); `view` speaks of the whole one.
pub fn raster(bgra: &mut [u8], view: &View, image: &Rgba, level: u32, back: &Backdrop) {
    let (pw, _) = view.pane;
    clear(bgra, back.background);
    let Some(rect) = view.covers() else { return };
    let (origin, scale) = (view.origin(), view.scale() * (1u32 << level) as f32);
    checks(bgra, pw, rect, origin, back);
    let (x, y, w, h) = rect;
    let squares = scale >= SQUARES_FROM;
    let cols = taps(x, w, origin.0, scale, image.width, squares);
    let rows = taps(y, h, origin.1, scale, image.height, squares);
    let iw = image.width as usize;
    let px = |sx: u32, sy: u32| -> [u32; 4] {
        let at = (sy as usize * iw + sx as usize) * 4;
        let p = &image.px[at..at + 4];
        [p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32]
    };
    for (row, &(y0, y1, fy)) in rows.iter().enumerate() {
        let line = (y as usize + row) * pw as usize;
        for (col, &(x0, x1, fx)) in cols.iter().enumerate() {
            // Blended premultiplied, so a transparent neighbour's colour
            // doesn't tint the edge.
            let mut c = [0u32; 4];
            for (sx, sy, wgt) in [(x0, y0, (256 - fx) * (256 - fy)), (x1, y0, fx * (256 - fy)), (x0, y1, (256 - fx) * fy), (x1, y1, fx * fy)] {
                if wgt == 0 {
                    continue;
                }
                let p = px(sx, sy);
                c[0] += p[0] * p[3] * wgt / 255;
                c[1] += p[1] * p[3] * wgt / 255;
                c[2] += p[2] * p[3] * wgt / 255;
                c[3] += p[3] * wgt;
            }
            let (r, g, b, a) = (c[0] >> 16, c[1] >> 16, c[2] >> 16, c[3] >> 16);
            let at = (line + x as usize + col) * 4;
            over(&mut bgra[at..at + 4], [r, g, b, a]);
        }
    }
}

/// A premultiplied pixel `src` (RGBA) laid over the frame's BGRA `dst`.
#[inline]
fn over(dst: &mut [u8], [r, g, b, a]: [u32; 4]) {
    if a >= 255 {
        dst[..3].copy_from_slice(&[b.min(255) as u8, g.min(255) as u8, r.min(255) as u8]);
        return;
    }
    let keep = 255 - a;
    dst[0] = (b + (dst[0] as u32 * keep + 127) / 255).min(255) as u8;
    dst[1] = (g + (dst[1] as u32 * keep + 127) / 255).min(255) as u8;
    dst[2] = (r + (dst[2] as u32 * keep + 127) / 255).min(255) as u8;
}

/// Premultiplied RGBA pixels (an SVG drawn by resvg) laid over the
/// rectangle (x, y, w, h) of a frame `width` wide.
pub fn premultiplied(bgra: &mut [u8], width: u32, (x, y, w, h): (u32, u32, u32, u32), rgba: &[u8]) {
    for row in 0..h {
        let src = &rgba[(row * w) as usize * 4..((row + 1) * w) as usize * 4];
        let at = ((y + row) as usize * width as usize + x as usize) * 4;
        let dst = &mut bgra[at..at + w as usize * 4];
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            if s[3] != 0 {
                over(d, [s[0] as u32, s[1] as u32, s[2] as u32, s[3] as u32]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: Backdrop = Backdrop { background: (10, 20, 30), checks: [(100, 100, 100), (200, 200, 200)], check: 2 };

    fn at(bgra: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]]
    }

    fn view(image: (u32, u32), pane: (u32, u32)) -> View {
        let mut v = View::new(image, 1.0, false);
        v.set_pane(pane.0, pane.1, 1.0);
        v
    }

    #[test]
    fn around_it_the_theme_under_it_the_checks() {
        // A 2 x 2 picture: opaque red, transparent, half-transparent blue,
        // opaque green -- in a 6 x 4 pane, at its size, in the middle.
        let image = Rgba::new(2, 2, vec![255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 128, 0, 255, 0, 255]);
        let v = view((2, 2), (6, 4));
        let mut bgra = vec![0; 6 * 4 * 4];
        raster(&mut bgra, &v, &image, 0, &BACK);
        assert_eq!(at(&bgra, 6, 0, 0), [30, 20, 10, 255], "the theme's background, BGRA");
        assert_eq!(at(&bgra, 6, 2, 1), [0, 0, 255, 255], "red");
        assert_eq!(at(&bgra, 6, 3, 1), [100, 100, 100, 255], "a check, through the transparent pixel");
        let half = at(&bgra, 6, 2, 2);
        assert!(half[0] > 150 && half[2] < 60, "blue over a check: {half:?}");
        assert_eq!(at(&bgra, 6, 3, 2), [0, 255, 0, 255], "green");
    }

    #[test]
    fn zoomed_in_far_its_pixels_are_squares() {
        let image = Rgba::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let mut v = view((2, 1), (8, 4));
        v.zoom_by(4.0, None);
        let mut bgra = vec![0; 8 * 4 * 4];
        raster(&mut bgra, &v, &image, 0, &BACK);
        assert_eq!(at(&bgra, 8, 3, 1), [0, 0, 255, 255], "the red square's last column is still red");
        assert_eq!(at(&bgra, 8, 4, 1), [255, 0, 0, 255], "and the blue's first, blue");
    }

    #[test]
    fn shown_small_it_is_drawn_from_a_halved_picture() {
        assert_eq!(level_for(1.0), 0);
        assert_eq!(level_for(0.5), 0);
        assert_eq!(level_for(0.3), 1);
        assert_eq!(level_for(0.1), 3);
        // Stripes of black and white average to grey, not to whichever
        // stripe a sample lands on.
        let px: Vec<u8> = (0..16 * 16).flat_map(|i| if i % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let image = Rgba::new(16, 16, px);
        let halved = image.halved().halved();
        assert_eq!((halved.width, halved.height), (4, 4));
        assert!((120..=135).contains(&halved.px[0]), "{}", halved.px[0]);
        let clear = Rgba::new(2, 1, vec![255, 0, 0, 0, 0, 0, 255, 255]).halved();
        assert_eq!(clear.px, [0, 0, 255, 128], "a transparent pixel's colour doesn't bleed");
    }

    #[test]
    fn an_svgs_pixels_lay_over_premultiplied() {
        let mut bgra = vec![0; 2 * 4];
        clear(&mut bgra, (0, 0, 0));
        premultiplied(&mut bgra, 2, (0, 0, 2, 1), &[0, 0, 0, 0, 128, 0, 0, 128]);
        assert_eq!(at(&bgra, 2, 0, 0), [0, 0, 0, 255], "transparent: the backdrop");
        assert_eq!(at(&bgra, 2, 1, 0), [0, 0, 128, 255], "half red over black");
    }

    #[test]
    fn the_backdrop_follows_the_theme() {
        let dark = backdrop((30, 30, 46), (205, 214, 244), 2.0);
        assert_eq!(dark.background, (30, 30, 46));
        assert!(dark.checks[0].0 > 30 && dark.checks[1].0 > dark.checks[0].0, "a little lighter on a dark theme");
        assert_eq!(dark.check, 16);
        let light = backdrop((250, 250, 250), (40, 40, 40), 1.0);
        assert!(light.checks[1].0 < light.checks[0].0 && light.checks[0].0 < 250, "darker on a light one");
    }
}
