//! A file's bytes as pixels: a still picture, turned as its EXIF says; a
//! moving one (GIF, WebP, APNG), a frame at a time; or an SVG, drawn at
//! whatever size it's shown (`svg`).
//!
//! An extension has 128 MiB. A picture is decoded whole only up to
//! `MOST_PIXELS`; a bigger JPEG is decoded at a half, a quarter or an
//! eighth of its size (which a screen doesn't miss until you zoom in),
//! and a bigger picture of another kind is refused rather than taking
//! the extension down. A moving one keeps a frame, not all of them.

use std::io::Cursor;
use std::rc::Rc;

use image::{AnimationDecoder, DynamicImage, ImageDecoder};

use crate::format::Format;
use crate::render::Rgba;
use crate::svg::Svg;

/// The most pixels decoded at once: a 4K screenshot's worth (32 MiB of
/// RGBA), leaving room for the frame shown, a smaller copy of it, the
/// decoder's own buffers and the fonts.
pub const MOST_PIXELS: u64 = 1 << 23;
/// What a decoder may allocate, beyond which it says so instead of the
/// extension running out of memory.
const MOST_BYTES: u64 = 80 << 20;

pub enum Content {
    Still(Rgba),
    Moving(Box<Anim>),
    Vector(Box<Svg>),
}

/// A picture, decoded.
pub struct Picture {
    pub format: Format,
    /// Its size as it's shown (turned), in its own pixels.
    pub size: (u32, u32),
    /// Its pixels to each decoded one: 2, 4 or 8 for a JPEG decoded
    /// smaller than it is, else 1.
    pub reduced: u32,
    pub content: Content,
    /// The frame shown, halved `level` times, kept while it's drawn at
    /// that size.
    halved: Option<(u32, Rgba)>,
}

impl Picture {
    /// The decoded pixels' size: what `View` works in.
    pub fn pixels(&self) -> (u32, u32) {
        match &self.content {
            Content::Still(p) => (p.width, p.height),
            Content::Moving(a) => (a.current.width, a.current.height),
            Content::Vector(s) => s.size(),
        }
    }

    /// The frame shown, halved `level` times (`render::level_for`).
    pub fn level(&mut self, level: u32) -> Option<&Rgba> {
        let full = match &self.content {
            Content::Still(p) => p,
            Content::Moving(a) => &a.current,
            Content::Vector(_) => return None,
        };
        if level == 0 {
            return Some(full);
        }
        if self.halved.as_ref().map(|(l, _)| *l) != Some(level) {
            let mut img = full.halved();
            for _ in 1..level {
                img = img.halved();
            }
            self.halved = Some((level, img));
        }
        self.halved.as_ref().map(|(_, img)| img)
    }

    /// The next frame of a moving picture: how long it's shown, in ms.
    pub fn advance(&mut self) -> Result<u32, String> {
        let Content::Moving(anim) = &mut self.content else { return Err("it doesn't move".into()) };
        self.halved = None;
        anim.advance()?;
        Ok(anim.delay)
    }
}

/// `bytes`, of `format`, decoded.
pub fn decode(format: Format, bytes: Vec<u8>) -> Result<Picture, String> {
    if format == Format::Svg {
        let svg = Svg::parse(&bytes)?;
        return Ok(Picture { format, size: svg.size(), reduced: 1, content: Content::Vector(Box::new(svg)), halved: None });
    }
    let (w, h) = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format().map_err(|e| e.to_string())?.into_dimensions().map_err(|e| e.to_string())?;
    let pixels = w as u64 * h as u64;
    if pixels > MOST_PIXELS {
        if format == Format::Jpeg {
            return reduced_jpeg(&bytes, (w, h));
        }
        return Err(format!("{w} \u{d7} {h} is more than the viewer decodes ({:.1} megapixels; a JPEG that big is shown smaller)", MOST_PIXELS as f64 / 1e6));
    }
    if format.may_move() {
        let bytes: Rc<[u8]> = bytes.into();
        if let Some(anim) = Anim::start(format, bytes.clone())? {
            let size = (anim.current.width, anim.current.height);
            return Ok(Picture { format, size, reduced: 1, content: Content::Moving(Box::new(anim)), halved: None });
        }
        return still(format, &bytes);
    }
    still(format, &bytes)
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MOST_BYTES);
    limits
}

/// A picture of one frame, turned as its EXIF says.
fn still(format: Format, bytes: &[u8]) -> Result<Picture, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Picture { format, size: (w, h), reduced: 1, content: Content::Still(Rgba::new(w, h, rgba.into_raw())), halved: None })
}

/// How much smaller a JPEG `size` big is decoded: 2, 4 or 8, the least
/// that fits `MOST_PIXELS`; none when even an eighth doesn't.
pub fn reduction(size: (u32, u32)) -> Option<u32> {
    [2u32, 4, 8].into_iter().find(|f| (size.0.div_ceil(*f) as u64) * (size.1.div_ceil(*f) as u64) <= MOST_PIXELS)
}

/// A JPEG too big to decode whole, decoded smaller: its DCT does that
/// for a half, a quarter and an eighth at no cost.
fn reduced_jpeg(bytes: &[u8], (w, h): (u32, u32)) -> Result<Picture, String> {
    let by = reduction((w, h)).ok_or_else(|| format!("{w} \u{d7} {h} is too big to show, even an eighth of it"))?;
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().map_err(|e| e.to_string())?;
    let (dw, dh) = decoder.scale(w.div_ceil(by) as u16, h.div_ceil(by) as u16).map_err(|e| e.to_string())?;
    let px = decoder.decode().map_err(|e| e.to_string())?;
    let format = decoder.info().map(|i| i.pixel_format).ok_or("no picture in it")?;
    let rgb = match format {
        jpeg_decoder::PixelFormat::RGB24 => px,
        jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&l| [l, l, l]).collect(),
        jpeg_decoder::PixelFormat::L16 => px.chunks_exact(2).flat_map(|l| [l[0], l[0], l[0]]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => px.chunks_exact(4).flat_map(cmyk_to_rgb).collect(),
    };
    let img = image::RgbImage::from_raw(dw as u32, dh as u32, rgb).ok_or("its pixels don't add up")?;
    let mut img = DynamicImage::ImageRgb8(img);
    if let Some(o) = decoder.exif_data().and_then(image::metadata::Orientation::from_exif_chunk) {
        img.apply_orientation(o);
    }
    let turned = img.width() != dw as u32;
    let rgba = img.into_rgba8();
    let (rw, rh) = rgba.dimensions();
    let size = if turned { (h, w) } else { (w, h) };
    // The reduction the decoder made, which may be less than asked.
    let reduced = (size.0 as f32 / rw as f32).round().max(1.0) as u32;
    Ok(Picture { format: Format::Jpeg, size, reduced, content: Content::Still(Rgba::new(rw, rh, rgba.into_raw())), halved: None })
}

/// One CMYK pixel as a JPEG decoder gives it (Adobe's, inverted), RGB.
fn cmyk_to_rgb(p: &[u8]) -> [u8; 3] {
    let k = p[3] as f32 / 255.0;
    let channel = |c: u8| {
        let c = c as f32 / 255.0;
        ((1.0 - (c * (1.0 - k) + k)) * 255.0).round() as u8
    };
    [channel(p[0]), channel(p[1]), channel(p[2])]
}

type Frames = image::Frames<'static>;

/// A moving picture: its frames decoded one at a time as they're shown,
/// from the start again after the last.
pub struct Anim {
    bytes: Rc<[u8]>,
    format: Format,
    frames: Frames,
    /// A frame read ahead (the second, to tell it moves).
    ahead: Option<image::Frame>,
    pub current: Rgba,
    /// How long `current` is shown, in ms.
    pub delay: u32,
    /// `current`'s place, from 0.
    pub index: usize,
    /// How many frames it has, once it's been round once.
    pub count: Option<usize>,
}

/// Its frames from the start; none when it has one (a still GIF, a PNG
/// that isn't an APNG).
fn frames_of(format: Format, bytes: Rc<[u8]>) -> Result<Option<Frames>, String> {
    let reader = Cursor::new(bytes);
    let e = |e: image::ImageError| e.to_string();
    Ok(match format {
        Format::Gif => {
            let mut d = image::codecs::gif::GifDecoder::new(reader).map_err(e)?;
            d.set_limits(limits()).map_err(e)?;
            Some(d.into_frames())
        }
        Format::WebP => {
            let d = image::codecs::webp::WebPDecoder::new(reader).map_err(e)?;
            d.has_animation().then(|| d.into_frames())
        }
        Format::Png => {
            let mut d = image::codecs::png::PngDecoder::new(reader).map_err(e)?;
            if d.is_apng().map_err(e)? {
                d.set_limits(limits()).map_err(e)?;
                Some(d.apng().map_err(e)?.into_frames())
            } else {
                None
            }
        }
        _ => None,
    })
}

/// A frame's delay as it's shown: as browsers do, one asking for 10 ms
/// or less is shown for 100 ms -- such files mean "as fast as you like"
/// and were made for that.
pub fn shown_for(numer: u32, denom: u32) -> u32 {
    let ms = numer.checked_div(denom.max(1)).unwrap_or(0);
    if ms <= 10 {
        100
    } else {
        ms.min(60_000)
    }
}

impl Anim {
    /// Its first frame; none when there's no second.
    pub fn start(format: Format, bytes: Rc<[u8]>) -> Result<Option<Anim>, String> {
        let Some(mut frames) = frames_of(format, bytes.clone())? else { return Ok(None) };
        let first = frames.next().ok_or("no frames in it")?.map_err(|e| e.to_string())?;
        let ahead = match frames.next() {
            Some(Ok(f)) => f,
            // One frame, or a broken second: a still picture.
            _ => return Ok(None),
        };
        let (delay, current) = Anim::take(first);
        Ok(Some(Anim { bytes, format, frames, ahead: Some(ahead), current, delay, index: 0, count: None }))
    }

    fn take(frame: image::Frame) -> (u32, Rgba) {
        let (n, d) = frame.delay().numer_denom_ms();
        let buf = frame.into_buffer();
        let (w, h) = buf.dimensions();
        (shown_for(n, d), Rgba::new(w, h, buf.into_raw()))
    }

    /// On to the next frame, round to the first after the last.
    pub fn advance(&mut self) -> Result<(), String> {
        let next = match self.ahead.take() {
            Some(f) => Some(Ok(f)),
            None => self.frames.next(),
        };
        let frame = match next {
            Some(f) => {
                self.index += 1;
                f.map_err(|e| e.to_string())?
            }
            None => {
                self.count = Some(self.index + 1);
                self.frames = frames_of(self.format, self.bytes.clone())?.ok_or("it stopped moving")?;
                self.index = 0;
                self.frames.next().ok_or("no frames in it")?.map_err(|e| e.to_string())?
            }
        };
        let (delay, current) = Anim::take(frame);
        if (current.width, current.height) != (self.current.width, self.current.height) {
            return Err("its frames changed size".into());
        }
        self.delay = delay;
        self.current = current;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, ImageFormat, Rgba as Px, RgbaImage};

    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn gif(frames: &[(u8, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut out);
            enc.set_repeat(Repeat::Infinite).unwrap();
            for &(shade, ms) in frames {
                let img = RgbaImage::from_pixel(4, 3, Px([shade, 0, 0, 255]));
                enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1))).unwrap();
            }
        }
        out
    }

    #[test]
    fn a_still_picture_is_its_pixels() {
        let img = RgbaImage::from_fn(3, 2, |x, y| Px([x as u8 * 50, y as u8 * 50, 7, 255]));
        let p = decode(Format::Png, png(&img)).unwrap();
        assert_eq!((p.size, p.reduced, p.pixels()), ((3, 2), 1, (3, 2)));
        let Content::Still(px) = &p.content else { panic!("still") };
        assert_eq!(&px.px[4..8], &[50, 0, 7, 255]);
    }

    #[test]
    fn a_gif_moves_a_frame_at_a_time_and_goes_round() {
        let mut p = decode(Format::Gif, gif(&[(10, 50), (20, 0), (30, 200)])).unwrap();
        let Content::Moving(a) = &p.content else { panic!("moving") };
        assert_eq!((a.delay, a.current.px[0], a.count), (50, 10, None));
        assert_eq!(p.advance().unwrap(), 100, "0 ms is shown as 100 ms");
        assert_eq!(p.advance().unwrap(), 200);
        assert_eq!(p.advance().unwrap(), 50, "round to the first");
        let Content::Moving(a) = &p.content else { panic!("moving") };
        assert_eq!((a.index, a.current.px[0], a.count), (0, 10, Some(3)));
    }

    #[test]
    fn a_gif_of_one_frame_is_still() {
        let p = decode(Format::Gif, gif(&[(10, 50)])).unwrap();
        assert!(matches!(p.content, Content::Still(_)));
    }

    #[test]
    fn too_big_is_said_and_a_big_jpeg_is_decoded_smaller() {
        assert_eq!(reduction((4000, 3000)), Some(2), "12 megapixels: a quarter of them");
        assert_eq!(reduction((8000, 6000)), Some(4));
        assert_eq!(reduction((60_000, 60_000)), None);
        // A PNG's header says how big it is before anything's decoded.
        let mut header = png(&RgbaImage::new(1, 1));
        header[16..20].copy_from_slice(&5000u32.to_be_bytes());
        header[20..24].copy_from_slice(&5000u32.to_be_bytes());
        let crc = crc32(&header[12..29]);
        header[29..33].copy_from_slice(&crc.to_be_bytes());
        let err = decode(Format::Png, header).err().unwrap();
        assert!(err.starts_with("5000 \u{d7} 5000 is more than the viewer decodes"), "{err}");
    }

    /// PNG's chunk checksum, for a header changed by hand.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn a_jpeg_too_big_comes_at_half_size() {
        let img = image::RgbImage::from_pixel(4100, 2100, image::Rgb([200, 30, 30]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        let p = decode(Format::Jpeg, out.into_inner()).unwrap();
        assert_eq!((p.size, p.reduced, p.pixels()), ((4100, 2100), 2, (2050, 1050)));
    }

    #[test]
    fn frame_delays_are_as_browsers_show_them() {
        assert_eq!(shown_for(0, 1), 100);
        assert_eq!(shown_for(10, 1), 100);
        assert_eq!(shown_for(20, 1), 20);
        assert_eq!(shown_for(150, 2), 75);
    }
}
