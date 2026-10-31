//! Colours and the spaces they're written in: sRGB, HSL, OKLab and OKLCH;
//! sRGB's gamut, and mapping into it as CSS Color 4 does; WCAG contrast.

/// A colour in sRGB, each channel from 0 to 1 as it's encoded (not
/// linear), and its alpha. Out of gamut a channel may be outside 0..1,
/// until it's mapped in ([`Colour::gamut_map`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colour {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// A notch of sRGB that rounds to the same byte.
const EPS: f64 = 1e-9;

impl Colour {
    pub const BLACK: Colour = Colour { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Colour = Colour { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Colour {
        Colour { r, g, b, a }
    }

    pub fn from_u8(r: u8, g: u8, b: u8) -> Colour {
        Colour::new(f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0, 1.0)
    }

    pub fn with_alpha(self, a: f64) -> Colour {
        Colour { a: a.clamp(0.0, 1.0), ..self }
    }

    /// Its channels as bytes, rounded, clipped to the gamut.
    pub fn bytes(&self) -> [u8; 4] {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(self.r), byte(self.g), byte(self.b), byte(self.a)]
    }

    pub fn rgb(&self) -> (u8, u8, u8) {
        let [r, g, b, _] = self.bytes();
        (r, g, b)
    }

    /// Opaque, and the same in bytes: what a swatch or a byte format
    /// writes, compared.
    pub fn same_bytes(&self, other: &Colour) -> bool {
        self.bytes() == other.bytes()
    }

    pub fn in_gamut(&self) -> bool {
        [self.r, self.g, self.b].iter().all(|v| (-EPS..=1.0 + EPS).contains(v))
    }

    /// Each channel clipped to 0..1.
    pub fn clip(&self) -> Colour {
        Colour::new(self.r.clamp(0.0, 1.0), self.g.clamp(0.0, 1.0), self.b.clamp(0.0, 1.0), self.a)
    }

    /// Over `back` (opaque): what it looks like there.
    pub fn over(&self, back: &Colour) -> Colour {
        let mix = |f: f64, b: f64| f * self.a + b * (1.0 - self.a);
        Colour::new(mix(self.r, back.r), mix(self.g, back.g), mix(self.b, back.b), 1.0)
    }

    // HSL.

    /// Hue in degrees (0 for a grey), saturation and lightness from 0 to 1.
    pub fn hsl(&self) -> (f64, f64, f64) {
        let (r, g, b) = (self.r, self.g, self.b);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d.abs() < EPS {
            return (0.0, 0.0, l);
        }
        let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    pub fn from_hsl(h: f64, s: f64, l: f64, a: f64) -> Colour {
        let h = h.rem_euclid(360.0) / 360.0;
        let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
        if s == 0.0 {
            return Colour::new(l, l, l, a);
        }
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let channel = |t: f64| {
            let t = t.rem_euclid(1.0);
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Colour::new(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0), a)
    }

    // OKLab and OKLCH (Björn Ottosson's, as CSS Color 4 has them).

    /// Linear-light sRGB.
    pub fn linear(&self) -> [f64; 3] {
        [to_linear(self.r), to_linear(self.g), to_linear(self.b)]
    }

    pub fn from_linear([r, g, b]: [f64; 3], a: f64) -> Colour {
        Colour::new(from_linear(r), from_linear(g), from_linear(b), a)
    }

    /// OKLab: lightness from 0 to 1, and a and b.
    pub fn oklab(&self) -> [f64; 3] {
        let [r, g, b] = self.linear();
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        [
            0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
            1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
            0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
        ]
    }

    pub fn from_oklab([ll, aa, bb]: [f64; 3], alpha: f64) -> Colour {
        let l = (ll + 0.396_337_777_4 * aa + 0.215_803_757_3 * bb).powi(3);
        let m = (ll - 0.105_561_345_8 * aa - 0.063_854_172_8 * bb).powi(3);
        let s = (ll - 0.089_484_177_5 * aa - 1.291_485_548_0 * bb).powi(3);
        Colour::from_linear(
            [
                4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
                -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
                -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
            ],
            alpha,
        )
    }

    /// OKLCH: lightness from 0 to 1, chroma, and hue in degrees (0 when
    /// there's no chroma to speak of).
    pub fn oklch(&self) -> [f64; 3] {
        let [l, a, b] = self.oklab();
        let c = (a * a + b * b).sqrt();
        let h = if c < 1e-6 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
        [l, c, h]
    }

    /// The colour at OKLCH (`l`, `c`, `h`), out of sRGB's gamut as it may
    /// be: see [`Colour::gamut_map`].
    pub fn from_oklch(l: f64, c: f64, h: f64, a: f64) -> Colour {
        let (s, co) = h.to_radians().sin_cos();
        Colour::from_oklab([l, c.max(0.0) * co, c.max(0.0) * s], a)
    }

    /// The nearest colour in sRGB, as CSS Color 4 maps one in: its
    /// lightness and hue kept, its chroma brought down until clipping it
    /// changes it by no more than can be seen (ΔEOK 0.02).
    pub fn gamut_map(&self) -> Colour {
        if self.in_gamut() {
            return self.clip();
        }
        let [l, c, h] = self.oklch();
        if l >= 1.0 {
            return Colour::WHITE.with_alpha(self.a);
        }
        if l <= 0.0 {
            return Colour::BLACK.with_alpha(self.a);
        }
        const JND: f64 = 0.02;
        const EPSILON: f64 = 0.0001;
        let at = |chroma: f64| Colour::from_oklch(l, chroma, h, self.a);
        let mut clipped = self.clip();
        if delta_e_ok(&clipped, self) < JND {
            return clipped;
        }
        let (mut min, mut max) = (0.0, c);
        let mut min_in_gamut = true;
        while max - min > EPSILON {
            let chroma = (min + max) / 2.0;
            let current = at(chroma);
            if min_in_gamut && current.in_gamut() {
                min = chroma;
                continue;
            }
            clipped = current.clip();
            let e = delta_e_ok(&clipped, &current);
            if e < JND {
                if JND - e < EPSILON {
                    return clipped;
                }
                min_in_gamut = false;
                min = chroma;
            } else {
                max = chroma;
            }
        }
        clipped
    }

    // WCAG 2.x.

    /// Relative luminance, as WCAG 2.x defines it.
    pub fn luminance(&self) -> f64 {
        let [r, g, b] = self.clip().linear();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
}

/// How far apart two colours look: the distance in OKLab.
pub fn delta_e_ok(x: &Colour, y: &Colour) -> f64 {
    let (p, q) = (x.oklab(), y.oklab());
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

fn to_linear(v: f64) -> f64 {
    let a = v.abs();
    let lin = if a <= 0.04045 { a / 12.92 } else { ((a + 0.055) / 1.055).powf(2.4) };
    lin.copysign(v)
}

fn from_linear(v: f64) -> f64 {
    let a = v.abs();
    let enc = if a <= 0.003_130_8 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
    enc.copysign(v)
}

/// WCAG 2.x's contrast ratio of text `fg` on `bg`, from 1 to 21: a
/// see-through `fg` over `bg`, a see-through `bg` over `under` (the page).
pub fn contrast(fg: &Colour, bg: &Colour, under: &Colour) -> f64 {
    let bg = bg.over(under);
    let fg = fg.over(&bg);
    let (a, b) = (fg.luminance(), bg.luminance());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// What a contrast ratio passes, WCAG 2.x's success criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passes {
    /// 1.4.3, 4.5:1: text.
    pub aa: bool,
    /// 1.4.3, 3:1: large text (18 pt, or 14 pt bold).
    pub aa_large: bool,
    /// 1.4.6, 7:1.
    pub aaa: bool,
    /// 1.4.6, 4.5:1.
    pub aaa_large: bool,
    /// 1.4.11, 3:1: icons, borders, a control's parts.
    pub ui: bool,
}

/// What `ratio` passes. WCAG doesn't round: 4.499 fails AA.
pub fn passes(ratio: f64) -> Passes {
    Passes { aa: ratio >= 4.5, aa_large: ratio >= 3.0, aaa: ratio >= 7.0, aaa_large: ratio >= 4.5, ui: ratio >= 3.0 }
}

/// `fg` made lighter or darker, its OKLCH chroma and hue kept as far as
/// the gamut lets them, as little as it takes for its contrast on `bg` to
/// reach `target`; none when neither black nor white does.
pub fn nearest_passing(fg: &Colour, bg: &Colour, under: &Colour, target: f64) -> Option<Colour> {
    if contrast(fg, bg, under) >= target {
        return Some(*fg);
    }
    let [l0, c, h] = fg.oklch();
    let at = |l: f64| Colour::from_oklch(l, c, h, fg.a).gamut_map();
    let mut best: Option<(f64, Colour)> = None;
    // Up towards white, and down towards black: the nearer that passes.
    for end in [1.0, 0.0] {
        if contrast(&at(end), bg, under) < target {
            continue;
        }
        let (mut near, mut far) = (l0, end);
        for _ in 0..40 {
            let mid = (near + far) / 2.0;
            if contrast(&at(mid), bg, under) >= target {
                far = mid;
            } else {
                near = mid;
            }
        }
        // Bytes round: step on until the colour written passes too.
        let step = if end > l0 { 0.001 } else { -0.001 };
        let mut l = far;
        let mut found = at(l);
        while contrast(&Colour::from_u8(found.rgb().0, found.rgb().1, found.rgb().2).with_alpha(fg.a), bg, under) < target && (0.0..=1.0).contains(&l) {
            l += step;
            found = at(l.clamp(0.0, 1.0));
        }
        let d = (l - l0).abs();
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, found));
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Colour {
        let v = u32::from_str_radix(s, 16).unwrap();
        Colour::from_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    fn near(a: f64, b: f64, by: f64) -> bool {
        (a - b).abs() <= by
    }

    #[test]
    fn hsl_both_ways() {
        assert_eq!(hex("ff0000").hsl(), (0.0, 1.0, 0.5));
        let (h, s, l) = hex("3b82f6").hsl();
        assert!(near(h, 217.2, 0.1) && near(s, 0.912, 0.001) && near(l, 0.598, 0.001), "{h} {s} {l}");
        assert_eq!(Colour::from_hsl(120.0, 1.0, 0.25, 1.0).rgb(), (0, 128, 0));
        assert_eq!(Colour::from_hsl(-120.0, 1.0, 0.5, 1.0).rgb(), (0, 0, 255), "hue goes round");
        assert_eq!(hex("808080").hsl().1, 0.0, "a grey has no saturation");
        for v in ["3b82f6", "123456", "fedcba", "00ff7f", "7f7f00"] {
            let c = hex(v);
            let (h, s, l) = c.hsl();
            assert!(Colour::from_hsl(h, s, l, 1.0).same_bytes(&c), "{v}");
        }
    }

    /// CSS Color 4's own values, and Ottosson's.
    #[test]
    fn oklch_of_known_colours() {
        let cases = [
            ("ff0000", 0.627_955, 0.257_683, 29.234),
            ("00ff00", 0.866_440, 0.294_827, 142.495),
            ("0000ff", 0.452_014, 0.313_214, 264.052),
            ("ffffff", 1.0, 0.0, 0.0),
            ("000000", 0.0, 0.0, 0.0),
            ("808080", 0.599_871, 0.0, 0.0),
            ("663399", 0.440_272, 0.160_296, 303.373),
        ];
        for (v, l, c, h) in cases {
            let [gl, gc, gh] = hex(v).oklch();
            assert!(near(gl, l, 1e-4) && near(gc, c, 1e-4) && near(gh, h, 0.02), "{v}: {gl} {gc} {gh}");
        }
        // Back again.
        for v in ["ff0000", "3b82f6", "663399", "f0f8ff", "010203"] {
            let [l, c, h] = hex(v).oklch();
            assert!(Colour::from_oklch(l, c, h, 1.0).same_bytes(&hex(v)), "{v}");
        }
        let [l, a, b] = hex("ff0000").oklab();
        assert!(near(l, 0.627_955, 1e-5) && near(a, 0.224_863, 1e-5) && near(b, 0.125_846, 1e-5));
    }

    #[test]
    fn out_of_gamut_is_mapped_in() {
        // Display P3's green and a chroma no screen has.
        let wild = Colour::from_oklch(0.7, 0.4, 145.0, 1.0);
        assert!(!wild.in_gamut());
        let mapped = wild.gamut_map();
        assert!(mapped.in_gamut());
        let [l, c, h] = mapped.oklch();
        assert!(near(l, 0.7, 0.02) && near(h, 145.0, 3.0), "lightness and hue kept: {l} {h}");
        assert!(c < 0.4 && c > 0.15, "chroma brought down: {c}");
        // Clipping alone moves the hue; mapping keeps it nearer.
        let clipped_h = wild.clip().oklch()[2];
        assert!((h - 145.0).abs() <= (clipped_h - 145.0).abs() + 1e-9);
        assert_eq!(Colour::from_oklch(1.2, 0.1, 30.0, 1.0).gamut_map().rgb(), (255, 255, 255));
        assert_eq!(Colour::from_oklch(-0.1, 0.1, 30.0, 1.0).gamut_map().rgb(), (0, 0, 0));
        let red = hex("ff0000");
        assert_eq!(red.gamut_map(), red, "in gamut, as it is");
        // CSS Color 4's example: oklch(90% 0.3 145) maps to about
        // rgb(140 255 133)'s neighbourhood -- a light, strong green.
        let (r, g, b) = Colour::from_oklch(0.9, 0.3, 145.0, 1.0).gamut_map().rgb();
        assert!(g >= 250 && r < 200 && b < 200, "{r} {g} {b}");
    }

    #[test]
    fn wcag_contrast() {
        let w = Colour::WHITE;
        assert!(near(contrast(&Colour::BLACK, &w, &w), 21.0, 1e-9));
        assert!(near(contrast(&w, &w, &w), 1.0, 1e-9));
        assert!(near(contrast(&hex("767676"), &w, &w), 4.54, 0.005), "the lightest grey that passes AA on white");
        assert!(contrast(&hex("777777"), &w, &w) < 4.5);
        assert!(near(contrast(&hex("0000ff"), &w, &w), 8.59, 0.005));
        assert!(near(contrast(&w, &hex("0000ff"), &w), 8.59, 0.005), "either way round");
        // Half black on white is a mid grey.
        let half = Colour::BLACK.with_alpha(0.5);
        assert!(near(contrast(&half, &w, &w), contrast(&hex("808080"), &w, &w), 0.05));
        let p = passes(4.5);
        assert!(p.aa && p.aa_large && !p.aaa && p.aaa_large && p.ui);
        assert!(!passes(4.499).aa, "not rounded up");
        assert!(passes(7.0).aaa);
        assert!(!passes(2.99).ui);
    }

    #[test]
    fn the_nearest_colour_that_passes() {
        let w = Colour::WHITE;
        let fg = hex("999999");
        let got = nearest_passing(&fg, &w, &w, 4.5).unwrap();
        let ratio = contrast(&Colour::from_u8(got.rgb().0, got.rgb().1, got.rgb().2), &w, &w);
        assert!((4.5..4.7).contains(&ratio), "just passes: {ratio}");
        assert!(got.luminance() < fg.luminance(), "darker, on white");
        let blue = hex("3b82f6");
        let got = nearest_passing(&blue, &w, &w, 7.0).unwrap();
        assert!(near(got.oklch()[2], blue.oklch()[2], 3.0), "its hue kept");
        assert_eq!(nearest_passing(&hex("000000"), &w, &w, 4.5), Some(hex("000000")), "already passes");
        assert_eq!(nearest_passing(&hex("777777"), &hex("777777"), &w, 22.0), None);
    }
}
