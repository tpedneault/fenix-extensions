//! What a file is, from its first bytes (its name only for SVG, which is
//! text), and how its details read: `PNG · 1920 × 1080 · 2.3 MB`.

/// The pictures it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Svg,
}

/// The file names it opens (`contributes.opens`), by extension: kept the
/// same as the manifest's, which a test checks.
pub const EXTENSIONS: &[&str] = &["png", "apng", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "svgz"];

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Gif => "GIF",
            Format::WebP => "WebP",
            Format::Bmp => "BMP",
            Format::Svg => "SVG",
        }
    }

    /// Whether it may have more than one frame.
    pub fn may_move(self) -> bool {
        matches!(self, Format::Png | Format::Gif | Format::WebP)
    }
}

/// Whether `name` is a file it opens, by its extension.
pub fn is_image_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// What `bytes` are. The bytes decide -- a JPEG called `.png` is a JPEG
/// -- and an SVG is told by its markup, or by its name when gzipped
/// (`.svgz`).
pub fn detect(name: &str, bytes: &[u8]) -> Option<Format> {
    let magic = |m: &[u8]| bytes.starts_with(m);
    if magic(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if magic(&[0xff, 0xd8, 0xff]) {
        return Some(Format::Jpeg);
    }
    if magic(b"GIF87a") || magic(b"GIF89a") {
        return Some(Format::Gif);
    }
    if magic(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some(Format::WebP);
    }
    if magic(b"BM") && bytes.len() > 14 {
        return Some(Format::Bmp);
    }
    let svgz = name.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("svgz"));
    if magic(&[0x1f, 0x8b]) && svgz {
        return Some(Format::Svg);
    }
    looks_like_svg(bytes).then_some(Format::Svg)
}

/// Markup with an `<svg` near its start: after a byte-order mark, an XML
/// declaration, comments or a doctype.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with('<') && text.contains("<svg")
}

/// A file's size as people say it: `812 B`, `14.2 KB`, `2.3 MB`.
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else {
        format!("{:.1} GB", b / KB / KB / KB)
    }
}

/// What the details line says of a picture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Details {
    pub name: String,
    pub format: Option<Format>,
    /// As it's shown: an EXIF rotation applied.
    pub size: (u32, u32),
    pub bytes: u64,
    /// How big it's shown, in percent of its actual size.
    pub percent: u32,
    pub fit: bool,
    /// Its place among the folder's pictures, counting from 1.
    pub place: Option<(usize, usize)>,
    /// Its frame (from 1), and how many there are once it's played
    /// through; whether it's playing.
    pub frame: Option<(usize, Option<usize>, bool)>,
    /// Decoded smaller than it is (a JPEG too big to decode whole): by
    /// how much.
    pub reduced: Option<u32>,
}

impl Details {
    /// The left half: what it is.
    pub fn what(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if let Some(f) = self.format {
            parts.push(f.name().to_string());
        }
        if self.size != (0, 0) {
            parts.push(format!("{} \u{d7} {}", self.size.0, self.size.1));
        }
        parts.push(size_text(self.bytes));
        parts.join("  \u{b7}  ")
    }

    /// The right half: how it's shown.
    pub fn how(&self) -> String {
        let mut parts = Vec::new();
        // Playing, it says so (and how many frames, once it's been round):
        // the line doesn't change with every frame. Paused, which frame.
        match self.frame {
            Some((_, count, true)) => parts.push(match count {
                Some(n) => format!("playing, {n} frames"),
                None => "playing".into(),
            }),
            Some((at, count, false)) => parts.push(format!("frame {at}{} (paused)", count.map(|n| format!("/{n}")).unwrap_or_default())),
            None => {}
        }
        if let Some(f) = self.reduced {
            parts.push(format!("decoded at 1/{f}"));
        }
        parts.push(if self.fit { format!("fit {}%", self.percent) } else { format!("{}%", self.percent) });
        if let Some((at, of)) = self.place {
            parts.push(format!("{at}/{of}"));
        }
        parts.join("  \u{b7}  ")
    }

    /// Both, for the modeline: `IMG cat.png  PNG  640 × 480  12.0 KB  100%`.
    pub fn status(&self) -> String {
        format!("IMG {}  \u{b7}  {}", self.what(), self.how())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bytes_say_what_it_is() {
        assert_eq!(detect("a.png", b"\x89PNG\r\n\x1a\n...."), Some(Format::Png));
        assert_eq!(detect("a.png", &[0xff, 0xd8, 0xff, 0xe0]), Some(Format::Jpeg), "a JPEG called .png");
        assert_eq!(detect("a.gif", b"GIF89a.."), Some(Format::Gif));
        assert_eq!(detect("a.webp", b"RIFF\x10\0\0\0WEBPVP8 "), Some(Format::WebP));
        assert_eq!(detect("a.bmp", b"BM..............."), Some(Format::Bmp));
        assert_eq!(detect("a.svg", b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<!-- x -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), Some(Format::Svg));
        assert_eq!(detect("a.svgz", &[0x1f, 0x8b, 8, 0]), Some(Format::Svg));
        assert_eq!(detect("a.gz", &[0x1f, 0x8b, 8, 0]), None, "gzip is an SVG only by its name");
        assert_eq!(detect("a.png", b"hello"), None);
        assert_eq!(detect("a.svg", b"<html><body/></html>"), None);
    }

    #[test]
    fn names_it_opens() {
        for name in ["a.png", "B.JPG", "c.jpeg", "d.Gif", "e.webp", "f.bmp", "g.svg", "h.svgz", "i.apng"] {
            assert!(is_image_name(name), "{name}");
        }
        for name in ["a.pdf", "png", ".png", "a.png.txt", "notes"] {
            assert!(!is_image_name(name), "{name}");
        }
    }

    #[test]
    fn the_manifest_opens_what_it_reads() {
        let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/extension.toml")).unwrap();
        let line = manifest.lines().find(|l| l.starts_with("opens = ")).expect("opens");
        let opens: Vec<String> = line.trim_start_matches("opens = ").trim_matches(['[', ']']).split(',').map(|p| p.trim().trim_matches('"').trim_start_matches("*.").to_string()).collect();
        assert_eq!(opens, EXTENSIONS);
    }

    #[test]
    fn sizes_read_as_people_say_them() {
        assert_eq!(size_text(812), "812 B");
        assert_eq!(size_text(14_540), "14.2 KB");
        assert_eq!(size_text(2_411_724), "2.3 MB");
    }

    #[test]
    fn details_say_what_it_is_and_how_its_shown() {
        let mut d = Details { name: "cat.gif".into(), format: Some(Format::Gif), size: (640, 480), bytes: 12_288, percent: 50, fit: true, place: Some((3, 12)), frame: Some((4, None, true)), reduced: None };
        assert_eq!(d.what(), "cat.gif  \u{b7}  GIF  \u{b7}  640 \u{d7} 480  \u{b7}  12.0 KB");
        assert_eq!(d.how(), "playing  \u{b7}  fit 50%  \u{b7}  3/12");
        d.frame = Some((4, Some(9), true));
        assert!(d.how().starts_with("playing, 9 frames"));
        d.frame = Some((4, Some(9), false));
        d.fit = false;
        d.place = None;
        assert_eq!(d.how(), "frame 4/9 (paused)  \u{b7}  50%");
    }
}
