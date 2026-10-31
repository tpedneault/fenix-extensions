//! How a picture sits in its pane: fitted, or at a scale, and which of
//! its points is in the middle. Pure arithmetic, in pixels: the pane's
//! (the display's own, `dpi` of them to a point) and the picture's.

/// How big it's shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// As big as the pane holds it, and kept so as the pane changes.
    Fit,
    /// Pane pixels to a picture pixel.
    Scale(f32),
}

/// One step of `+` and `-`, as the diagram viewer has it.
pub const STEP: f32 = 1.25;
/// A pan key moves a tenth of the pane.
const PAN: f32 = 0.1;
/// The most it's zoomed in: 32 times its actual size.
const MOST: f32 = 32.0;

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The pane, in its pixels.
    pub pane: (u32, u32),
    /// Its pixels to a point: 2 on a screen twice as dense as most.
    pub dpi: f32,
    /// The picture, in the pixels it was decoded at.
    pub image: (u32, u32),
    /// Its actual pixels to each decoded one: more than 1 for a JPEG
    /// decoded smaller than it is.
    pub unit: f32,
    /// Whether fitting may make it bigger than it is: an SVG's drawn
    /// sharp at any size, a photo's pixels would only blur.
    pub grow: bool,
    pub zoom: Zoom,
    /// The picture's point in the middle of the pane, in its pixels.
    pub center: (f32, f32),
}

impl View {
    pub fn new(image: (u32, u32), unit: f32, grow: bool) -> View {
        let image = (image.0.max(1), image.1.max(1));
        View { pane: (0, 0), dpi: 1.0, image, unit: unit.max(1.0), grow, zoom: Zoom::Fit, center: (image.0 as f32 / 2.0, image.1 as f32 / 2.0) }
    }

    /// Pane pixels to a picture pixel at its actual size: a point to each
    /// of its pixels, as the system shows pictures.
    pub fn actual(&self) -> f32 {
        self.dpi * self.unit
    }

    /// The scale that fits it in the pane -- no bigger than it is, unless
    /// it may `grow`.
    pub fn fit_scale(&self) -> f32 {
        if self.pane.0 == 0 || self.pane.1 == 0 {
            return self.actual();
        }
        let fits = (self.pane.0 as f32 / self.image.0 as f32).min(self.pane.1 as f32 / self.image.1 as f32);
        if self.grow {
            fits
        } else {
            fits.min(self.actual())
        }
    }

    pub fn scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fit_scale(),
            Zoom::Scale(s) => s,
        }
    }

    /// How big it's shown, in percent of its actual size.
    pub fn percent(&self) -> u32 {
        (self.scale() / self.actual() * 100.0).round() as u32
    }

    /// The pane is this big now. Fitted, it fits again; zoomed, its middle
    /// stays in the middle.
    pub fn set_pane(&mut self, width: u32, height: u32, dpi: f32) {
        self.pane = (width, height);
        self.dpi = if dpi > 0.0 { dpi } else { 1.0 };
        self.clamp();
    }

    /// `0`: fitted, and in the middle.
    pub fn fit(&mut self) {
        self.zoom = Zoom::Fit;
        self.center = (self.image.0 as f32 / 2.0, self.image.1 as f32 / 2.0);
    }

    /// `1`: at its actual size, about the same middle.
    pub fn actual_size(&mut self) {
        self.set_scale(self.actual(), None);
    }

    /// `+` and `-`, and the wheel: `factor` bigger, about point `at` of the
    /// pane (its middle when none) -- the picture's point under it stays
    /// there. No smaller than fitted: there it fits again.
    pub fn zoom_by(&mut self, factor: f32, at: Option<(f32, f32)>) {
        self.set_scale(self.scale() * factor, at);
    }

    fn set_scale(&mut self, scale: f32, at: Option<(f32, f32)>) {
        let fit = self.fit_scale();
        let most = (self.actual() * MOST).max(fit);
        let scale = scale.clamp(fit, most);
        if scale <= fit * 1.0001 {
            return self.fit();
        }
        let at = at.unwrap_or((self.pane.0 as f32 / 2.0, self.pane.1 as f32 / 2.0));
        let (ox, oy) = self.origin();
        let s = self.scale();
        // The picture's point under `at`, kept under it.
        let (px, py) = ((at.0 - ox) / s, (at.1 - oy) / s);
        self.zoom = Zoom::Scale(scale);
        let (nox, noy) = (at.0 - px * scale, at.1 - py * scale);
        self.center = ((self.pane.0 as f32 / 2.0 - nox) / scale, (self.pane.1 as f32 / 2.0 - noy) / scale);
        self.clamp();
    }

    /// The picture moved `dx`, `dy` pane pixels under the pane: a drag.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        let s = self.scale();
        self.center.0 -= dx / s;
        self.center.1 -= dy / s;
        self.clamp();
    }

    /// `h` `j` `k` `l`: the view moved a tenth of the pane, that way.
    pub fn pan(&mut self, right: i32, down: i32) {
        let (w, h) = (self.pane.0.max(1) as f32, self.pane.1.max(1) as f32);
        self.drag(-(right as f32) * w * PAN, -(down as f32) * h * PAN);
    }

    /// Kept in sight: a picture smaller than the pane sits in its middle,
    /// a bigger one doesn't leave an edge of the pane bare.
    fn clamp(&mut self) {
        let s = self.scale();
        let axis = |center: f32, image: u32, pane: u32| {
            let (image, half) = (image as f32, pane as f32 / s / 2.0);
            if image <= half * 2.0 {
                image / 2.0
            } else {
                center.clamp(half, image - half)
            }
        };
        self.center = (axis(self.center.0, self.image.0, self.pane.0), axis(self.center.1, self.image.1, self.pane.1));
    }

    /// Where the picture's top left corner is in the pane.
    pub fn origin(&self) -> (f32, f32) {
        let s = self.scale();
        (self.pane.0 as f32 / 2.0 - self.center.0 * s, self.pane.1 as f32 / 2.0 - self.center.1 * s)
    }

    /// The part of the pane it covers: x, y, width, height, cut to the
    /// pane; none when it's out of sight.
    pub fn covers(&self) -> Option<(u32, u32, u32, u32)> {
        let s = self.scale();
        let (ox, oy) = self.origin();
        let x0 = ox.round().max(0.0);
        let y0 = oy.round().max(0.0);
        let x1 = (ox + self.image.0 as f32 * s).round().min(self.pane.0 as f32);
        let y1 = (oy + self.image.1 as f32 * s).round().min(self.pane.1 as f32);
        (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// What's kept in the session to show it again as it was: its zoom
    /// relative to its actual size (none when fitted) and its middle as
    /// fractions of it -- so a different pane, a different screen, works.
    pub fn saved(&self) -> (Option<f32>, (f32, f32)) {
        let zoom = match self.zoom {
            Zoom::Fit => None,
            Zoom::Scale(s) => Some(s / self.actual()),
        };
        (zoom, (self.center.0 / self.image.0 as f32, self.center.1 / self.image.1 as f32))
    }

    /// Shown again as `saved` kept it: called once the pane (and how dense
    /// its screen is) is known, so the percent comes back the same.
    pub fn restore(&mut self, zoom: Option<f32>, center: (f32, f32)) {
        self.center = (center.0 * self.image.0 as f32, center.1 * self.image.1 as f32);
        self.zoom = match zoom {
            Some(z) if z.is_finite() && z > 0.0 => Zoom::Scale(z * self.actual()),
            _ => Zoom::Fit,
        };
        if self.pane != (0, 0) {
            self.clamp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(image: (u32, u32), pane: (u32, u32)) -> View {
        let mut v = View::new(image, 1.0, false);
        v.set_pane(pane.0, pane.1, 1.0);
        v
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn fit_shrinks_a_big_picture_and_leaves_a_small_one_as_it_is() {
        let v = view((2000, 1000), (1000, 1000));
        assert!(close(v.scale(), 0.5));
        assert_eq!(v.percent(), 50);
        assert_eq!(v.covers(), Some((0, 250, 1000, 500)), "in the middle");
        let v = view((100, 50), (1000, 1000));
        assert!(close(v.scale(), 1.0), "no bigger than it is");
        let mut svg = View::new((100, 50), 1.0, true);
        svg.set_pane(1000, 1000, 1.0);
        assert!(close(svg.scale(), 10.0), "an SVG grows to fit");
    }

    #[test]
    fn actual_size_is_a_point_a_pixel() {
        let mut v = View::new((2000, 1000), 1.0, false);
        v.set_pane(1000, 1000, 2.0);
        assert!(close(v.fit_scale(), 0.5));
        v.actual_size();
        assert!(close(v.scale(), 2.0), "two pane pixels to one on a dense screen");
        assert_eq!(v.percent(), 100);
        let mut big_jpeg = View::new((500, 250), 4.0, false);
        big_jpeg.set_pane(1000, 1000, 1.0);
        big_jpeg.actual_size();
        assert_eq!(big_jpeg.percent(), 100);
        assert!(close(big_jpeg.scale(), 4.0), "decoded at a quarter, shown at its whole size");
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = view((1000, 1000), (500, 500));
        v.actual_size();
        v.center = (300.0, 300.0);
        let (ox, oy) = v.origin();
        let at = (100.0, 400.0);
        let before = ((at.0 - ox) / v.scale(), (at.1 - oy) / v.scale());
        v.zoom_by(STEP, Some(at));
        let (ox, oy) = v.origin();
        let after = ((at.0 - ox) / v.scale(), (at.1 - oy) / v.scale());
        assert!(close(before.0, after.0) && close(before.1, after.1), "{before:?} {after:?}");
    }

    #[test]
    fn zooming_out_stops_at_fit_and_in_at_thirty_two_times() {
        let mut v = view((1000, 1000), (500, 500));
        v.zoom_by(STEP, None);
        assert_eq!(v.zoom, Zoom::Scale(0.5 * STEP));
        v.zoom_by(1.0 / STEP / STEP, None);
        assert_eq!(v.zoom, Zoom::Fit);
        v.set_pane(250, 250, 1.0);
        assert!(close(v.scale(), 0.25), "fitted, it fits the new pane");
        for _ in 0..100 {
            v.zoom_by(STEP, None);
        }
        assert!(close(v.scale(), 32.0));
    }

    #[test]
    fn panning_stops_at_the_edges() {
        let mut v = view((1000, 1000), (500, 500));
        v.actual_size();
        assert_eq!(v.center, (500.0, 500.0));
        v.pan(1, 0);
        assert_eq!(v.center, (550.0, 500.0), "a tenth of the pane");
        for _ in 0..20 {
            v.pan(1, 1);
        }
        assert_eq!(v.center, (750.0, 750.0), "the right and bottom edges at the pane's");
        assert_eq!(v.covers(), Some((0, 0, 500, 500)));
        v.drag(10_000.0, 0.0);
        assert_eq!(v.center.0, 250.0);
        let mut small = view((100, 100), (500, 500));
        small.pan(5, 5);
        assert_eq!(small.center, (50.0, 50.0), "smaller than the pane, it stays in the middle");
    }

    #[test]
    fn the_session_keeps_the_zoom_as_a_percent() {
        let mut v = view((1000, 800), (500, 500));
        v.zoom_by(4.0, None);
        v.pan(1, 1);
        let (zoom, center) = v.saved();
        let mut w = View::new((1000, 800), 1.0, false);
        w.restore(zoom, center);
        w.set_pane(500, 500, 1.0);
        assert_eq!(w.zoom, v.zoom);
        assert!(close(w.center.0, v.center.0) && close(w.center.1, v.center.1));
        let mut fitted = View::new((10, 10), 1.0, false);
        fitted.restore(None, (0.5, 0.5));
        assert_eq!(fitted.zoom, Zoom::Fit);
    }
}
