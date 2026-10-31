//! An RFB (VNC) client, sans I/O: what came on the socket goes in
//! (`feed`), what it means comes out as `Frame`s, and what to send back
//! waits in `take_outgoing`. Nothing here knows about sockets, timers or
//! Fenix, so all of it runs under a native test against a fake server.
//!
//! What it speaks, as Fenix 1.x's built-in viewer did:
//!
//! - RFB 3.3, 3.7 and 3.8; a server offering another 3.x is spoken to as
//!   3.3 (RFC 6143 7.1.1), and a later one as 3.8.
//! - Security: None, and VNC Authentication (DES) with the password it's
//!   given -- empty, as 1.x had, unless one is set.
//! - Pixels: 32-bit BGRA, asked for whatever the server's own is, so they
//!   go to the surface as they come.
//! - Encodings: ZRLE, CopyRect and Raw; the pseudo-encodings Cursor (the
//!   guest's pointer drawn by Fenix, not into the picture), DesktopSize
//!   and ExtendedDesktopSize (the guest resized, and resizing it to the
//!   pane), and LastRect.

use fenix_extension_api::socket::Inbox;

use crate::des;

/// One thing the server said, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// The handshake's done: the desktop's name and size. Nothing else
    /// comes before it.
    Ready { name: String, width: u16, height: u16 },
    /// The framebuffer's size: at the start, and when it changes.
    Resolution { width: u16, height: u16 },
    /// A rectangle of pixels, BGRA, opaque.
    Rect { x: u16, y: u16, width: u16, height: u16, bgra: Vec<u8> },
    /// A rectangle in one colour.
    Fill { x: u16, y: u16, width: u16, height: u16, rgb: (u8, u8, u8) },
    /// `CopyRect`: the rectangle at (`from_x`, `from_y`) copied to (`x`, `y`).
    Copy { x: u16, y: u16, width: u16, height: u16, from_x: u16, from_y: u16 },
    /// Everything since the last was one `FramebufferUpdate`: the picture
    /// is whole again, and may be shown.
    UpdateEnd,
    /// The guest's pointer: BGRA with its mask as alpha, and its hot
    /// spot. Empty (0 x 0) when there's none to draw.
    Cursor { width: u16, height: u16, hot_x: u16, hot_y: u16, bgra: Vec<u8> },
    Bell,
    /// The guest's clipboard changed.
    ClipboardText(String),
    /// The server can be asked to resize its desktop (`set_desktop_size`):
    /// it said so with an ExtendedDesktopSize rectangle, the only way RFB
    /// has. Asking one that can't would put the connection out of step.
    ResizeSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V3_3,
    V3_7,
    V3_8,
}

/// The version a server's `RFB xxx.yyy\n` is spoken to as.
pub fn negotiate(greeting: &[u8]) -> Result<Version, String> {
    let text = std::str::from_utf8(greeting).map_err(|_| "not a VNC server: its greeting isn't RFB's".to_string())?;
    let rest = text.strip_prefix("RFB ").and_then(|r| r.strip_suffix('\n')).ok_or_else(|| format!("not a VNC server: it said {:?}", text.trim_end()))?;
    let (major, minor) = rest.split_once('.').ok_or_else(|| format!("not a VNC server: it said {:?}", text.trim_end()))?;
    let (major, minor): (u32, u32) = (major.parse().map_err(|_| format!("not a VNC server: it said {text:?}"))?, minor.parse().map_err(|_| format!("not a VNC server: it said {text:?}"))?);
    Ok(match (major, minor) {
        (3, 7) => Version::V3_7,
        (3, m) if m >= 8 => Version::V3_8,
        (3, _) => Version::V3_3,
        (m, _) if m > 3 => Version::V3_8,
        _ => return Err(format!("the server speaks RFB {major}.{minor}, older than 3.3")),
    })
}

const NONE: u8 = 1;
const VNC_AUTH: u8 = 2;

/// A security type's name, for saying which ones a server wanted.
fn security_name(t: u8) -> String {
    match t {
        5 | 6 => "RA2".into(),
        16 => "Tight".into(),
        18 => "TLS".into(),
        19 => "VeNCrypt".into(),
        20 => "SASL".into(),
        30..=35 => "Apple".into(),
        113 => "MS-Logon II".into(),
        t => format!("type {t}"),
    }
}

pub mod encoding {
    pub const RAW: i32 = 0;
    pub const COPY_RECT: i32 = 1;
    pub const ZRLE: i32 = 16;
    pub const CURSOR: i32 = -239;
    pub const DESKTOP_SIZE: i32 = -223;
    pub const LAST_RECT: i32 = -224;
    pub const EXTENDED_DESKTOP_SIZE: i32 = -308;
}

/// What it asks the server for, most wanted first. Not Tight: its JPEG
/// would need a JPEG decoder, and ZRLE does well on a LAN.
pub const ENCODINGS: [i32; 7] = [encoding::ZRLE, encoding::COPY_RECT, encoding::RAW, encoding::CURSOR, encoding::DESKTOP_SIZE, encoding::EXTENDED_DESKTOP_SIZE, encoding::LAST_RECT];

/// 32 bits a pixel, depth 24, little-endian, true colour, red at bit 16,
/// green 8, blue 0: in memory, B G R x.
pub const PIXEL_FORMAT: [u8; 16] = [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0];

#[derive(Debug)]
enum State {
    Version,
    /// 3.3: the server picks the type, a u32.
    SecurityChosen,
    /// 3.7, 3.8: the server lists the types it takes.
    SecurityTypes,
    Challenge,
    SecurityResult,
    /// Why it refused, a u32 length and the text.
    Reason(&'static str),
    ServerInit,
    Messages,
    /// Inside a `FramebufferUpdate`, this many rectangles still to come.
    Rects(u16),
    Failed,
}

pub struct Client {
    state: State,
    version: Version,
    password: String,
    inbox: Inbox,
    out: Vec<u8>,
    zlib: flate2::Decompress,
    width: u16,
    height: u16,
}

impl Client {
    /// A client for a connection just made, that will answer VNC
    /// Authentication with `password`.
    pub fn new(password: &str) -> Client {
        Client { state: State::Version, version: Version::V3_8, password: password.into(), inbox: Inbox::new(), out: Vec::new(), zlib: flate2::Decompress::new(true), width: 0, height: 0 }
    }

    /// Whether the handshake's done.
    pub fn ready(&self) -> bool {
        matches!(self.state, State::Messages | State::Rects(_))
    }

    /// The framebuffer's size, as the server last said.
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// What's to be sent to the server, taken.
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// What came from the server, and what it means: as much as has come
    /// whole. An error ends the connection; nothing more is read after it.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<Frame>, String> {
        if matches!(self.state, State::Failed) {
            return Err("the connection already failed".into());
        }
        self.inbox.push(data);
        let mut frames = Vec::new();
        match self.read(&mut frames) {
            Ok(()) => Ok(frames),
            Err(e) => {
                self.state = State::Failed;
                Err(e)
            }
        }
    }

    fn read(&mut self, frames: &mut Vec<Frame>) -> Result<(), String> {
        loop {
            let progressed = match self.state {
                State::Version => self.read_version()?,
                State::SecurityChosen => self.read_security_chosen()?,
                State::SecurityTypes => self.read_security_types()?,
                State::Challenge => self.read_challenge()?,
                State::SecurityResult => self.read_security_result()?,
                State::Reason(what) => self.read_reason(what)?,
                State::ServerInit => self.read_server_init(frames)?,
                State::Messages => self.read_message(frames)?,
                State::Rects(left) => self.read_rect(left, frames)?,
                State::Failed => false,
            };
            if !progressed {
                return Ok(());
            }
        }
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        self.inbox.peek(at + 4).map(|b| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]))
    }

    fn read_version(&mut self) -> Result<bool, String> {
        let Some(greeting) = self.inbox.take(12) else { return Ok(false) };
        self.version = negotiate(&greeting)?;
        self.out.extend_from_slice(match self.version {
            Version::V3_3 => b"RFB 003.003\n",
            Version::V3_7 => b"RFB 003.007\n",
            Version::V3_8 => b"RFB 003.008\n",
        });
        self.state = if self.version == Version::V3_3 { State::SecurityChosen } else { State::SecurityTypes };
        Ok(true)
    }

    fn read_security_chosen(&mut self) -> Result<bool, String> {
        let Some(t) = self.u32_at(0) else { return Ok(false) };
        self.inbox.take(4);
        self.state = match t {
            0 => State::Reason("the server refused the connection"),
            1 => self.client_init(),
            2 => State::Challenge,
            t => return Err(format!("the server wants security Fenix doesn't speak ({})", security_name(t as u8))),
        };
        Ok(true)
    }

    fn read_security_types(&mut self) -> Result<bool, String> {
        let Some(&[n]) = self.inbox.peek(1) else { return Ok(false) };
        if n == 0 {
            self.inbox.take(1);
            self.state = State::Reason("the server refused the connection");
            return Ok(true);
        }
        let Some(list) = self.inbox.take(1 + n as usize) else { return Ok(false) };
        let types = &list[1..];
        if types.contains(&NONE) {
            self.out.push(NONE);
            // 3.7 says nothing more after None; 3.8 says it went well.
            self.state = if self.version == Version::V3_8 { State::SecurityResult } else { self.client_init() };
        } else if types.contains(&VNC_AUTH) {
            self.out.push(VNC_AUTH);
            self.state = State::Challenge;
        } else {
            let names: Vec<String> = types.iter().map(|t| security_name(*t)).collect();
            return Err(format!("the server wants security Fenix doesn't speak ({}); it speaks None and VNC Authentication", names.join(", ")));
        }
        Ok(true)
    }

    fn read_challenge(&mut self) -> Result<bool, String> {
        let Some(challenge) = self.inbox.take(16) else { return Ok(false) };
        let challenge: [u8; 16] = challenge.try_into().expect("16 bytes");
        self.out.extend_from_slice(&des::vnc_auth_response(&challenge, &self.password));
        self.state = State::SecurityResult;
        Ok(true)
    }

    fn read_security_result(&mut self) -> Result<bool, String> {
        let Some(result) = self.u32_at(0) else { return Ok(false) };
        self.inbox.take(4);
        self.state = match (result, self.version) {
            (0, _) => self.client_init(),
            // 3.8 says why.
            (_, Version::V3_8) => State::Reason("authentication failed"),
            _ => return Err(if self.password.is_empty() { "authentication failed: the server wants a password".into() } else { "authentication failed".into() }),
        };
        Ok(true)
    }

    fn read_reason(&mut self, what: &'static str) -> Result<bool, String> {
        let Some(len) = self.u32_at(0) else { return Ok(false) };
        let Some(bytes) = self.inbox.take(4 + len as usize) else { return Ok(false) };
        let reason = String::from_utf8_lossy(&bytes[4..]).trim().to_string();
        let wants_password = what == "authentication failed" && self.password.is_empty();
        Err(match (reason.is_empty(), wants_password) {
            (true, true) => format!("{what}: the server wants a password"),
            (true, false) => what.to_string(),
            (false, _) => format!("{what}: {reason}"),
        })
    }

    /// Shared: other viewers stay connected.
    fn client_init(&mut self) -> State {
        self.out.push(1);
        State::ServerInit
    }

    fn read_server_init(&mut self, frames: &mut Vec<Frame>) -> Result<bool, String> {
        let Some(name_len) = self.u32_at(20) else { return Ok(false) };
        let Some(init) = self.inbox.take(24 + name_len as usize) else { return Ok(false) };
        self.width = u16::from_be_bytes([init[0], init[1]]);
        self.height = u16::from_be_bytes([init[2], init[3]]);
        let name = String::from_utf8_lossy(&init[24..]).into_owned();
        // Its pixels in the format the surface takes, whatever the server's own.
        self.out.extend_from_slice(&[0, 0, 0, 0]);
        self.out.extend_from_slice(&PIXEL_FORMAT);
        self.out.extend_from_slice(&[2, 0]);
        self.out.extend_from_slice(&(ENCODINGS.len() as u16).to_be_bytes());
        for e in ENCODINGS {
            self.out.extend_from_slice(&e.to_be_bytes());
        }
        // The whole of it, the first time.
        self.request_update(false);
        frames.push(Frame::Ready { name, width: self.width, height: self.height });
        frames.push(Frame::Resolution { width: self.width, height: self.height });
        self.state = State::Messages;
        Ok(true)
    }

    fn read_message(&mut self, frames: &mut Vec<Frame>) -> Result<bool, String> {
        let Some(&[kind]) = self.inbox.peek(1) else { return Ok(false) };
        match kind {
            // FramebufferUpdate: padding, how many rectangles.
            0 => {
                let Some(head) = self.inbox.take(4) else { return Ok(false) };
                let count = u16::from_be_bytes([head[2], head[3]]);
                if count == 0 {
                    frames.push(Frame::UpdateEnd);
                } else {
                    self.state = State::Rects(count);
                }
            }
            // SetColourMapEntries: true colour has no use for it.
            1 => {
                let Some(head) = self.inbox.peek(6) else { return Ok(false) };
                let colours = u16::from_be_bytes([head[4], head[5]]) as usize;
                if self.inbox.take(6 + 6 * colours).is_none() {
                    return Ok(false);
                }
            }
            2 => {
                self.inbox.take(1);
                frames.push(Frame::Bell);
            }
            // ServerCutText: padding, length, Latin-1 (UTF-8 from some).
            3 => {
                let Some(len) = self.u32_at(4) else { return Ok(false) };
                let Some(bytes) = self.inbox.take(8 + len as usize) else { return Ok(false) };
                let text = &bytes[8..];
                let text = match std::str::from_utf8(text) {
                    Ok(utf8) => utf8.to_string(),
                    Err(_) => text.iter().map(|&b| b as char).collect(),
                };
                frames.push(Frame::ClipboardText(text));
            }
            other => return Err(format!("the server sent a message Fenix doesn't know (type {other})")),
        }
        Ok(true)
    }

    fn read_rect(&mut self, left: u16, frames: &mut Vec<Frame>) -> Result<bool, String> {
        let Some(head) = self.inbox.peek(12) else { return Ok(false) };
        let be16 = |i: usize| u16::from_be_bytes([head[i], head[i + 1]]);
        let (x, y, width, height) = (be16(0), be16(2), be16(4), be16(6));
        let enc = i32::from_be_bytes([head[8], head[9], head[10], head[11]]);
        let pixels = width as usize * height as usize;
        let body = match enc {
            encoding::RAW => pixels * 4,
            encoding::COPY_RECT => 4,
            encoding::ZRLE => match self.u32_at(12) {
                Some(len) => 4 + len as usize,
                None => return Ok(false),
            },
            encoding::CURSOR => pixels * 4 + (width as usize).div_ceil(8) * height as usize,
            encoding::DESKTOP_SIZE | encoding::LAST_RECT => 0,
            encoding::EXTENDED_DESKTOP_SIZE => match self.inbox.peek(13) {
                Some(b) => 4 + 16 * b[12] as usize,
                None => return Ok(false),
            },
            other => return Err(format!("the server sent a rectangle in an encoding Fenix didn't ask for ({other})")),
        };
        let Some(all) = self.inbox.take(12 + body) else { return Ok(false) };
        let body = &all[12..];
        let mut last = left == 1;
        match enc {
            encoding::RAW => {
                let mut bgra = body.to_vec();
                // Opaque: the fourth byte is padding to the server.
                for px in bgra.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
                frames.push(Frame::Rect { x, y, width, height, bgra });
            }
            encoding::COPY_RECT => {
                let (from_x, from_y) = (u16::from_be_bytes([body[0], body[1]]), u16::from_be_bytes([body[2], body[3]]));
                frames.push(Frame::Copy { x, y, width, height, from_x, from_y });
            }
            encoding::ZRLE => frames.push(zrle(&mut self.zlib, &body[4..], x, y, width, height)?),
            encoding::CURSOR => frames.push(cursor(body, x, y, width, height)),
            encoding::DESKTOP_SIZE => {
                self.width = width;
                self.height = height;
                frames.push(Frame::Resolution { width, height });
            }
            encoding::EXTENDED_DESKTOP_SIZE => {
                // x is why (0 the server, 1 this client, 2 another), y
                // whether a request of this client's was refused. Its
                // width and height are the framebuffer's, either way.
                frames.push(Frame::ResizeSupported);
                self.width = width;
                self.height = height;
                frames.push(Frame::Resolution { width, height });
            }
            _ => last = true, // LastRect: the update ends here.
        }
        if last {
            frames.push(Frame::UpdateEnd);
            self.state = State::Messages;
        } else {
            self.state = State::Rects(left - 1);
        }
        Ok(true)
    }

    // What it sends.

    /// FramebufferUpdateRequest for all of it: what's changed
    /// (`incremental`), or everything.
    pub fn request_update(&mut self, incremental: bool) {
        self.out.extend_from_slice(&[3, incremental as u8, 0, 0, 0, 0]);
        self.out.extend_from_slice(&self.width.to_be_bytes());
        self.out.extend_from_slice(&self.height.to_be_bytes());
    }

    pub fn key(&mut self, keysym: u32, down: bool) {
        self.out.extend_from_slice(&[4, down as u8, 0, 0]);
        self.out.extend_from_slice(&keysym.to_be_bytes());
    }

    /// The pointer at (`x`, `y`) with every button held: 1 left, 2 middle,
    /// 4 right, 8 and 16 the wheel up and down. Stateful: the whole mask
    /// goes with every move.
    pub fn pointer(&mut self, x: u16, y: u16, buttons: u8) {
        self.out.extend_from_slice(&[5, buttons]);
        self.out.extend_from_slice(&x.to_be_bytes());
        self.out.extend_from_slice(&y.to_be_bytes());
    }

    /// ClientCutText: the clipboard, in Latin-1 as RFB has it; what
    /// Latin-1 can't say is a `?`.
    pub fn cut_text(&mut self, text: &str) {
        let latin1: Vec<u8> = text.chars().map(|c| u8::try_from(c as u32).unwrap_or(b'?')).collect();
        self.out.extend_from_slice(&[6, 0, 0, 0]);
        self.out.extend_from_slice(&(latin1.len() as u32).to_be_bytes());
        self.out.extend_from_slice(&latin1);
    }

    /// SetDesktopSize: one screen, at the origin, `width` x `height`.
    /// Only once the server has said it can (`Frame::ResizeSupported`).
    pub fn set_desktop_size(&mut self, width: u16, height: u16) {
        self.out.extend_from_slice(&[251, 0]);
        self.out.extend_from_slice(&width.to_be_bytes());
        self.out.extend_from_slice(&height.to_be_bytes());
        self.out.extend_from_slice(&[1, 0]);
        self.out.extend_from_slice(&0u32.to_be_bytes());
        self.out.extend_from_slice(&[0, 0, 0, 0]);
        self.out.extend_from_slice(&width.to_be_bytes());
        self.out.extend_from_slice(&height.to_be_bytes());
        self.out.extend_from_slice(&0u32.to_be_bytes());
    }
}

/// The CursorPseudo rectangle: its pixels, then a bitmask of which are
/// the pointer's. The rectangle's x and y are its hot spot.
fn cursor(body: &[u8], hot_x: u16, hot_y: u16, width: u16, height: u16) -> Frame {
    let (w, h) = (width as usize, height as usize);
    let (pixels, mask) = body.split_at(w * h * 4);
    let row_bytes = w.div_ceil(8);
    let mut bgra = pixels.to_vec();
    for y in 0..h {
        for x in 0..w {
            let on = mask[y * row_bytes + x / 8] & (0x80 >> (x % 8)) != 0;
            bgra[(y * w + x) * 4 + 3] = if on { 255 } else { 0 };
        }
    }
    Frame::Cursor { width, height, hot_x, hot_y, bgra }
}

/// All of a ZRLE rectangle's zlib data, inflated. One zlib stream runs
/// the length of the connection; each rectangle ends on a flush.
fn inflate(z: &mut flate2::Decompress, mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(input.len() * 4 + 4096);
    loop {
        if out.capacity() - out.len() < 16 << 10 {
            out.reserve(64 << 10);
        }
        let (in0, out0) = (z.total_in(), z.total_out());
        z.decompress_vec(input, &mut out, flate2::FlushDecompress::Sync).map_err(|e| format!("ZRLE: {e}"))?;
        let consumed = (z.total_in() - in0) as usize;
        let produced = z.total_out() - out0;
        input = &input[consumed..];
        if consumed == 0 && produced == 0 {
            break;
        }
    }
    if !input.is_empty() {
        return Err("ZRLE: zlib data left over".into());
    }
    Ok(out)
}

/// Reads a ZRLE rectangle's inflated bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or("ZRLE: a tile ran short")?;
        self.at += 1;
        Ok(b)
    }

    /// A CPIXEL: three bytes, B G R, for the pixel format asked for.
    fn cpixel(&mut self) -> Result<[u8; 4], String> {
        let b = self.bytes.get(self.at..self.at + 3).ok_or("ZRLE: a tile ran short")?;
        self.at += 3;
        Ok([b[0], b[1], b[2], 255])
    }

    fn run(&mut self) -> Result<usize, String> {
        let mut len = 1;
        loop {
            let b = self.u8()?;
            len += b as usize;
            if b != 255 {
                return Ok(len);
            }
        }
    }
}

/// A ZRLE rectangle: 64 x 64 tiles, each raw, solid, packed from a
/// palette, or run-length encoded. One colour throughout comes out as a
/// `Fill`.
fn zrle(z: &mut flate2::Decompress, data: &[u8], x: u16, y: u16, width: u16, height: u16) -> Result<Frame, String> {
    let bytes = inflate(z, data)?;
    let mut r = Reader { bytes: &bytes, at: 0 };
    let (w, h) = (width as usize, height as usize);
    let mut bgra = vec![0u8; w * h * 4];
    let mut solid: Option<Option<[u8; 4]>> = None;
    for ty in (0..h).step_by(64) {
        for tx in (0..w).step_by(64) {
            let (tw, th) = ((w - tx).min(64), (h - ty).min(64));
            let mut tile = Vec::with_capacity(tw * th);
            let sub = r.u8()?;
            let mut palette = Vec::new();
            let palette_size = (sub & 0x7F) as usize;
            for _ in 0..palette_size {
                palette.push(r.cpixel()?);
            }
            match sub {
                0 => {
                    for _ in 0..tw * th {
                        tile.push(r.cpixel()?);
                    }
                }
                1 => tile.resize(tw * th, palette[0]),
                2..=16 => {
                    let bits = match palette_size {
                        2 => 1,
                        3 | 4 => 2,
                        _ => 4,
                    };
                    let per_row = (tw * bits).div_ceil(8);
                    for _ in 0..th {
                        let row = r.bytes.get(r.at..r.at + per_row).ok_or("ZRLE: a tile ran short")?;
                        for px in 0..tw {
                            let bit = px * bits;
                            let index = (row[bit / 8] >> (8 - bits - bit % 8)) & ((1 << bits) - 1) as u8;
                            tile.push(*palette.get(index as usize).ok_or("ZRLE: a palette index past the palette")?);
                        }
                        r.at += per_row;
                    }
                }
                128 => {
                    while tile.len() < tw * th {
                        let px = r.cpixel()?;
                        let run = r.run()?;
                        tile.extend(std::iter::repeat_n(px, run));
                    }
                }
                130..=255 => {
                    while tile.len() < tw * th {
                        let index = r.u8()?;
                        let run = if index & 0x80 != 0 { r.run()? } else { 1 };
                        let px = *palette.get((index & 0x7F) as usize).ok_or("ZRLE: a palette index past the palette")?;
                        tile.extend(std::iter::repeat_n(px, run));
                    }
                }
                other => return Err(format!("ZRLE: a tile in a sub-encoding there isn't ({other})")),
            }
            if tile.len() != tw * th {
                return Err("ZRLE: a run past its tile".into());
            }
            solid = match (solid, sub) {
                (None, 1) => Some(Some(palette[0])),
                (Some(Some(c)), 1) if c == palette[0] => Some(Some(c)),
                _ => Some(None),
            };
            for row in 0..th {
                let at = ((ty + row) * w + tx) * 4;
                for (i, px) in tile[row * tw..(row + 1) * tw].iter().enumerate() {
                    bgra[at + i * 4..at + i * 4 + 4].copy_from_slice(px);
                }
            }
        }
    }
    Ok(match solid {
        Some(Some([b, g, r, _])) => Frame::Fill { x, y, width, height, rgb: (r, g, b) },
        _ => Frame::Rect { x, y, width, height, bgra },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_negotiated_as_rfc_6143_says() {
        assert_eq!(negotiate(b"RFB 003.008\n"), Ok(Version::V3_8));
        assert_eq!(negotiate(b"RFB 003.007\n"), Ok(Version::V3_7));
        assert_eq!(negotiate(b"RFB 003.003\n"), Ok(Version::V3_3));
        assert_eq!(negotiate(b"RFB 003.005\n"), Ok(Version::V3_3), "another 3.x is 3.3");
        assert_eq!(negotiate(b"RFB 003.889\n"), Ok(Version::V3_8), "Apple's");
        assert_eq!(negotiate(b"RFB 004.001\n"), Ok(Version::V3_8));
        assert!(negotiate(b"SSH-2.0-Open").unwrap_err().contains("not a VNC server"));
    }

    #[test]
    fn a_cursor_takes_its_alpha_from_the_mask() {
        // 2 x 1: the left pixel the pointer's, the right not.
        let body = [1, 2, 3, 0, 4, 5, 6, 0, 0b1000_0000];
        let Frame::Cursor { width, height, hot_x, hot_y, bgra } = cursor(&body, 1, 0, 2, 1) else { panic!() };
        assert_eq!((width, height, hot_x, hot_y), (2, 1, 1, 0));
        assert_eq!(bgra, [1, 2, 3, 255, 4, 5, 6, 0]);
    }

    #[test]
    fn messages_are_encoded_as_rfb_has_them() {
        let mut c = Client::new("");
        c.width = 800;
        c.height = 600;
        c.request_update(true);
        c.key(0xFF0D, true);
        c.pointer(10, 20, 0x09);
        c.cut_text("héllo★");
        c.set_desktop_size(1024, 768);
        let out = c.take_outgoing();
        let mut want = vec![3, 1, 0, 0, 0, 0, 3, 32, 2, 88];
        want.extend([4, 1, 0, 0, 0, 0, 0xFF, 0x0D]);
        want.extend([5, 9, 0, 10, 0, 20]);
        want.extend([6, 0, 0, 0, 0, 0, 0, 6, b'h', 0xE9, b'l', b'l', b'o', b'?']);
        want.extend([251, 0, 4, 0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 0, 0, 0, 0, 0]);
        assert_eq!(out, want);
        assert!(c.take_outgoing().is_empty(), "taken");
    }
}
