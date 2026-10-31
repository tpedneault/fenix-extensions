//! The viewer against a fake RFB server, through a fake Fenix: the bytes
//! go back and forth as a socket would carry them -- in pieces as small
//! as one byte -- and what the surface ends up showing is checked against
//! the server's own framebuffer.

use std::collections::{BTreeMap, VecDeque};

use fenix_extension_api::{CursorImage, KeyEvent, Level, Modifiers, NamedKey, PointerEvent, SurfaceInput, SurfaceKey, SurfaceSize, WheelEvent};
use fenix_vnc::rfb;
use fenix_vnc::viewer::{self, Host, Link, Viewer};

// The server.

#[derive(Clone)]
enum Security {
    None,
    Password(&'static str),
    /// Only these types, none of which the client speaks.
    Only(Vec<u8>),
}

/// What the client sent, read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClientMsg {
    PixelFormat([u8; 16]),
    Encodings(Vec<i32>),
    Request { incremental: bool, width: u16, height: u16 },
    Key(u32, bool),
    Pointer(u16, u16, u8),
    CutText(Vec<u8>),
    DesktopSize(u16, u16),
}

#[derive(Debug, PartialEq, Eq)]
enum Stage {
    Version,
    Security,
    Challenge([u8; 16]),
    Init,
    Messages,
    Closed,
}

struct Server {
    greeting: &'static [u8; 12],
    /// The version the client answered with.
    client: [u8; 12],
    security: Security,
    stage: Stage,
    inbox: Vec<u8>,
    out: Vec<u8>,
    width: u16,
    height: u16,
    /// BGRA, opaque: what the client should be showing.
    fb: Vec<[u8; 4]>,
    zlib: flate2::Compress,
    /// Updates to send, one per request, in turn; none left, it's idle.
    updates: VecDeque<Vec<u8>>,
    /// Announce ExtendedDesktopSize in the first update, and take resizes.
    resizable: bool,
    announced: bool,
    got: Vec<ClientMsg>,
}

impl Server {
    fn new(width: u16, height: u16) -> Server {
        Server {
            greeting: b"RFB 003.008\n",
            client: [0; 12],
            security: Security::None,
            stage: Stage::Version,
            inbox: Vec::new(),
            out: b"RFB 003.008\n".to_vec(),
            width,
            height,
            fb: vec![[0, 0, 0, 255]; width as usize * height as usize],
            zlib: flate2::Compress::new(flate2::Compression::default(), true),
            updates: VecDeque::new(),
            resizable: false,
            announced: false,
            got: Vec::new(),
        }
    }

    fn version(mut self, greeting: &'static [u8; 12]) -> Server {
        self.greeting = greeting;
        self.out = greeting.to_vec();
        self
    }

    fn security(mut self, security: Security) -> Server {
        self.security = security;
        self
    }

    fn v33(&self) -> bool {
        self.client == *b"RFB 003.003\n"
    }

    fn v38(&self) -> bool {
        self.client == *b"RFB 003.008\n"
    }

    fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        (self.inbox.len() >= n).then(|| self.inbox.drain(..n).collect())
    }

    fn feed(&mut self, data: &[u8]) {
        self.inbox.extend_from_slice(data);
        while self.step() {}
    }

    fn challenge() -> [u8; 16] {
        *b"0123456789abcdef"
    }

    fn step(&mut self) -> bool {
        match self.stage {
            Stage::Version => {
                let Some(client) = self.take(12) else { return false };
                self.client = client.try_into().unwrap();
                match (&self.security, self.v33()) {
                    (Security::None, true) => {
                        self.out.extend_from_slice(&1u32.to_be_bytes());
                        self.stage = Stage::Init;
                    }
                    (Security::Password(_), true) => {
                        self.out.extend_from_slice(&2u32.to_be_bytes());
                        self.out.extend_from_slice(&Self::challenge());
                        self.stage = Stage::Challenge(Self::challenge());
                    }
                    (Security::None, false) => {
                        self.out.extend_from_slice(&[1, 1]);
                        self.stage = Stage::Security;
                    }
                    (Security::Password(_), false) => {
                        self.out.extend_from_slice(&[2, 16, 2]);
                        self.stage = Stage::Security;
                    }
                    (Security::Only(types), _) => {
                        self.out.push(types.len() as u8);
                        self.out.extend_from_slice(types);
                        self.stage = Stage::Security;
                    }
                }
                true
            }
            Stage::Security => {
                let Some(chosen) = self.take(1) else { return false };
                match chosen[0] {
                    1 => {
                        if self.v38() {
                            self.out.extend_from_slice(&0u32.to_be_bytes());
                        }
                        self.stage = Stage::Init;
                    }
                    2 => {
                        self.out.extend_from_slice(&Self::challenge());
                        self.stage = Stage::Challenge(Self::challenge());
                    }
                    _ => self.stage = Stage::Closed,
                }
                true
            }
            Stage::Challenge(challenge) => {
                let Some(answer) = self.take(16) else { return false };
                let Security::Password(password) = self.security else { unreachable!() };
                if answer == fenix_vnc::des::vnc_auth_response(&challenge, password) {
                    self.out.extend_from_slice(&0u32.to_be_bytes());
                    self.stage = Stage::Init;
                } else {
                    self.out.extend_from_slice(&1u32.to_be_bytes());
                    if self.v38() {
                        let why = b"wrong password";
                        self.out.extend_from_slice(&(why.len() as u32).to_be_bytes());
                        self.out.extend_from_slice(why);
                    }
                    self.stage = Stage::Closed;
                }
                true
            }
            Stage::Init => {
                let Some(_shared) = self.take(1) else { return false };
                self.out.extend_from_slice(&self.width.to_be_bytes());
                self.out.extend_from_slice(&self.height.to_be_bytes());
                // The server's own format: 16-bit, which the client replaces.
                self.out.extend_from_slice(&[16, 16, 0, 1, 0, 31, 0, 63, 0, 31, 11, 5, 0, 0, 0, 0]);
                self.out.extend_from_slice(&4u32.to_be_bytes());
                self.out.extend_from_slice(b"fake");
                self.stage = Stage::Messages;
                true
            }
            Stage::Messages => self.message(),
            Stage::Closed => false,
        }
    }

    fn message(&mut self) -> bool {
        let Some(&kind) = self.inbox.first() else { return false };
        let msg = match kind {
            0 => self.take(20).map(|m| ClientMsg::PixelFormat(m[4..20].try_into().unwrap())),
            2 => {
                let Some(n) = (self.inbox.len() >= 4).then(|| u16::from_be_bytes([self.inbox[2], self.inbox[3]]) as usize) else { return false };
                self.take(4 + 4 * n).map(|m| ClientMsg::Encodings(m[4..].chunks(4).map(|c| i32::from_be_bytes(c.try_into().unwrap())).collect()))
            }
            3 => self.take(10).map(|m| ClientMsg::Request { incremental: m[1] == 1, width: u16::from_be_bytes([m[6], m[7]]), height: u16::from_be_bytes([m[8], m[9]]) }),
            4 => self.take(8).map(|m| ClientMsg::Key(u32::from_be_bytes(m[4..8].try_into().unwrap()), m[1] == 1)),
            5 => self.take(6).map(|m| ClientMsg::Pointer(u16::from_be_bytes([m[2], m[3]]), u16::from_be_bytes([m[4], m[5]]), m[1])),
            6 => {
                let Some(len) = (self.inbox.len() >= 8).then(|| u32::from_be_bytes(self.inbox[4..8].try_into().unwrap()) as usize) else { return false };
                self.take(8 + len).map(|m| ClientMsg::CutText(m[8..].to_vec()))
            }
            251 => {
                let Some(n) = (self.inbox.len() >= 8).then(|| self.inbox[6] as usize) else { return false };
                self.take(8 + 16 * n).map(|m| ClientMsg::DesktopSize(u16::from_be_bytes([m[2], m[3]]), u16::from_be_bytes([m[4], m[5]])))
            }
            other => panic!("the client sent message type {other}"),
        };
        let Some(msg) = msg else { return false };
        match &msg {
            ClientMsg::Request { .. } => {
                let mut update = self.updates.pop_front();
                if self.resizable && !self.announced {
                    self.announced = true;
                    // Spontaneous, in the first update: it can be resized.
                    let mut rects = vec![ext_desktop_size(0, 0, self.width, self.height)];
                    if let Some(u) = update.take() {
                        rects.push(u[4..].to_vec());
                        let count = u16::from_be_bytes([u[2], u[3]]) + 1;
                        let mut msg = vec![0, 0];
                        msg.extend_from_slice(&count.to_be_bytes());
                        msg.extend(rects.concat());
                        update = Some(msg);
                    } else {
                        update = Some(fb_update(rects));
                    }
                }
                if let Some(u) = update {
                    self.out.extend(u);
                }
            }
            ClientMsg::DesktopSize(w, h) if self.resizable => {
                self.width = *w;
                self.height = *h;
                self.fb = vec![[0, 0, 0, 255]; *w as usize * *h as usize];
                self.out.extend(fb_update(vec![ext_desktop_size(1, 0, *w, *h)]));
            }
            _ => {}
        }
        self.got.push(msg);
        true
    }

    fn requests(&self) -> Vec<(bool, u16, u16)> {
        self.got
            .iter()
            .filter_map(|m| match m {
                ClientMsg::Request { incremental, width, height } => Some((*incremental, *width, *height)),
                _ => None,
            })
            .collect()
    }

    // Updates, kept in the server's own framebuffer as they're made.

    fn set(&mut self, x: u16, y: u16, px: [u8; 4]) {
        self.fb[y as usize * self.width as usize + x as usize] = px;
    }

    fn get(&self, x: u16, y: u16) -> [u8; 4] {
        self.fb[y as usize * self.width as usize + x as usize]
    }

    /// A Raw rectangle of `pixels` (BGRA, opaque); sent with a padding
    /// byte of 0, which the client must show opaque anyway.
    fn raw(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[[u8; 4]]) -> Vec<u8> {
        let mut rect = header(x, y, w, h, rfb::encoding::RAW);
        for (i, px) in pixels.iter().enumerate() {
            self.set(x + (i % w as usize) as u16, y + (i / w as usize) as u16, *px);
            rect.extend_from_slice(&[px[0], px[1], px[2], 0]);
        }
        rect
    }

    fn copy(&mut self, x: u16, y: u16, w: u16, h: u16, from_x: u16, from_y: u16) -> Vec<u8> {
        let src: Vec<[u8; 4]> = (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).map(|(c, r)| self.get(from_x + c, from_y + r)).collect();
        for (i, px) in src.into_iter().enumerate() {
            self.set(x + (i % w as usize) as u16, y + (i / w as usize) as u16, px);
        }
        let mut rect = header(x, y, w, h, rfb::encoding::COPY_RECT);
        rect.extend_from_slice(&from_x.to_be_bytes());
        rect.extend_from_slice(&from_y.to_be_bytes());
        rect
    }

    /// A ZRLE rectangle whose tiles, left to right and top to bottom, are
    /// written as `kinds` says (the last kind repeating), from `pixel(x, y)`.
    fn zrle(&mut self, x: u16, y: u16, w: u16, h: u16, kinds: &[Tile], pixel: impl Fn(u16, u16) -> [u8; 4]) -> Vec<u8> {
        let mut raw = Vec::new();
        let mut n = 0;
        for ty in (0..h).step_by(64) {
            for tx in (0..w).step_by(64) {
                let (tw, th) = ((w - tx).min(64), (h - ty).min(64));
                let px: Vec<[u8; 4]> = (0..th).flat_map(|r| (0..tw).map(move |c| (c, r))).map(|(c, r)| pixel(x + tx + c, y + ty + r)).collect();
                for (i, p) in px.iter().enumerate() {
                    self.set(x + tx + (i % tw as usize) as u16, y + ty + (i / tw as usize) as u16, *p);
                }
                raw.extend(zrle_tile(kinds[n.min(kinds.len() - 1)], &px, tw as usize));
                n += 1;
            }
        }
        let mut compressed = Vec::new();
        let mut input = &raw[..];
        loop {
            compressed.reserve(raw.len() + 1024);
            let before = self.zlib.total_in();
            self.zlib.compress_vec(input, &mut compressed, flate2::FlushCompress::Sync).unwrap();
            input = &input[(self.zlib.total_in() - before) as usize..];
            if input.is_empty() && compressed.len() < compressed.capacity() {
                break;
            }
        }
        let mut rect = header(x, y, w, h, rfb::encoding::ZRLE);
        rect.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        rect.extend(compressed);
        rect
    }
}

fn header(x: u16, y: u16, w: u16, h: u16, encoding: i32) -> Vec<u8> {
    let mut rect = Vec::new();
    for v in [x, y, w, h] {
        rect.extend_from_slice(&v.to_be_bytes());
    }
    rect.extend_from_slice(&encoding.to_be_bytes());
    rect
}

fn ext_desktop_size(reason: u16, result: u16, w: u16, h: u16) -> Vec<u8> {
    let mut rect = header(reason, result, w, h, rfb::encoding::EXTENDED_DESKTOP_SIZE);
    rect.extend_from_slice(&[1, 0, 0, 0]);
    rect.extend_from_slice(&0u32.to_be_bytes());
    for v in [0, 0, w, h] {
        rect.extend_from_slice(&v.to_be_bytes());
    }
    rect.extend_from_slice(&0u32.to_be_bytes());
    rect
}

fn fb_update(rects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut msg = vec![0, 0];
    msg.extend_from_slice(&(rects.len() as u16).to_be_bytes());
    msg.extend(rects.concat());
    msg
}

/// An update ended by LastRect rather than a count.
fn fb_update_last_rect(rects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut msg = vec![0, 0, 0xFF, 0xFF];
    msg.extend(rects.concat());
    msg.extend(header(0, 0, 0, 0, rfb::encoding::LAST_RECT));
    msg
}

fn cursor_rect(hot_x: u16, hot_y: u16) -> Vec<u8> {
    // 2 x 2: the top row the pointer's, the bottom not.
    let mut rect = header(hot_x, hot_y, 2, 2, rfb::encoding::CURSOR);
    rect.extend_from_slice(&[10, 20, 30, 0, 11, 21, 31, 0, 12, 22, 32, 0, 13, 23, 33, 0]);
    rect.extend_from_slice(&[0b1100_0000, 0]);
    rect
}

#[derive(Clone, Copy)]
enum Tile {
    Raw,
    Solid,
    Packed,
    PlainRle,
    PaletteRle,
}

fn cpixel(px: [u8; 4]) -> [u8; 3] {
    [px[0], px[1], px[2]]
}

fn run(out: &mut Vec<u8>, mut len: usize) {
    len -= 1;
    while len >= 255 {
        out.push(255);
        len -= 255;
    }
    out.push(len as u8);
}

fn zrle_tile(kind: Tile, px: &[[u8; 4]], tw: usize) -> Vec<u8> {
    let mut palette: Vec<[u8; 4]> = Vec::new();
    for p in px {
        if !palette.contains(p) {
            palette.push(*p);
        }
    }
    let mut out = Vec::new();
    match kind {
        Tile::Raw => {
            out.push(0);
            px.iter().for_each(|p| out.extend(cpixel(*p)));
        }
        Tile::Solid => {
            assert_eq!(palette.len(), 1, "a solid tile is one colour");
            out.push(1);
            out.extend(cpixel(palette[0]));
        }
        Tile::Packed => {
            assert!((2..=16).contains(&palette.len()));
            out.push(palette.len() as u8);
            palette.iter().for_each(|p| out.extend(cpixel(*p)));
            let bits = match palette.len() {
                2 => 1,
                3 | 4 => 2,
                _ => 4,
            };
            for row in px.chunks(tw) {
                let mut byte = 0u8;
                let mut used = 0;
                for p in row {
                    let i = palette.iter().position(|q| q == p).unwrap() as u8;
                    byte |= i << (8 - bits - used);
                    used += bits;
                    if used == 8 {
                        out.push(byte);
                        byte = 0;
                        used = 0;
                    }
                }
                if used > 0 {
                    out.push(byte);
                }
            }
        }
        Tile::PlainRle => {
            out.push(128);
            let mut i = 0;
            while i < px.len() {
                let len = px[i..].iter().take_while(|p| **p == px[i]).count();
                out.extend(cpixel(px[i]));
                run(&mut out, len);
                i += len;
            }
        }
        Tile::PaletteRle => {
            assert!((2..=127).contains(&palette.len()));
            out.push(128 + palette.len() as u8);
            palette.iter().for_each(|p| out.extend(cpixel(*p)));
            let mut i = 0;
            while i < px.len() {
                let len = px[i..].iter().take_while(|p| **p == px[i]).count();
                let index = palette.iter().position(|q| *q == px[i]).unwrap() as u8;
                if len == 1 {
                    out.push(index);
                } else {
                    out.push(index | 0x80);
                    run(&mut out, len);
                }
                i += len;
            }
        }
    }
    out
}

// Fenix.

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Connect(String, u16),
    Close(u64),
    Timer(u64, u32),
    CancelTimer(u64),
    SurfaceOpen(u64, String),
    Resize(u64, u32, u32),
    Present(u64),
    Cursor(u64, Option<CursorImage>),
    Retitle(u64, String),
    PageClose(u64),
    Say(Level, String),
    Copy(String),
    Pick(u64, Vec<String>),
    Save(String),
}

#[derive(Default)]
struct Fake {
    next: u64,
    calls: Vec<Call>,
    /// What each socket was sent, not yet given to its server.
    sent: BTreeMap<u64, Vec<u8>>,
    open: Vec<u64>,
    timers: BTreeMap<u64, u32>,
    settings: BTreeMap<String, String>,
    clipboard: Option<String>,
    refuse: Option<String>,
    /// Each surface's pixels, as Fenix would hold them.
    surfaces: BTreeMap<u64, (u32, u32, Vec<u8>)>,
    saved: Vec<(String, Vec<u8>)>,
}

impl Fake {
    fn with_hosts(rows: &str) -> Fake {
        let mut fake = Fake::default();
        fake.settings.insert("vnc.hosts".into(), rows.into());
        fake
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn said(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Say(_, m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    fn errors(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Say(Level::Error, m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    fn last_timer(&self) -> (u64, u32) {
        self.timers.iter().next_back().map(|(id, ms)| (*id, *ms)).expect("a timer")
    }

    fn surface(&self) -> u64 {
        *self.surfaces.keys().next_back().expect("a surface")
    }

    fn titles(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::SurfaceOpen(_, t) | Call::Retitle(_, t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    fn pixels(&self, surface: u64) -> Vec<[u8; 4]> {
        self.surfaces[&surface].2.chunks(4).map(|c| c.try_into().unwrap()).collect()
    }
}

impl Host for Fake {
    fn connect(&mut self, host: &str, port: u16) -> Result<u64, String> {
        self.calls.push(Call::Connect(host.into(), port));
        if let Some(why) = self.refuse.clone() {
            return Err(why);
        }
        let id = self.id();
        self.open.push(id);
        Ok(id)
    }
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
        if !self.open.contains(&socket) {
            return Err(format!("socket {socket} is closed"));
        }
        self.sent.entry(socket).or_default().extend_from_slice(data);
        Ok(())
    }
    fn close(&mut self, socket: u64) {
        self.open.retain(|s| *s != socket);
        self.calls.push(Call::Close(socket));
    }
    fn timer(&mut self, millis: u32) -> u64 {
        let id = self.id();
        self.timers.insert(id, millis);
        self.calls.push(Call::Timer(id, millis));
        id
    }
    fn cancel_timer(&mut self, timer: u64) {
        self.timers.remove(&timer);
        self.calls.push(Call::CancelTimer(timer));
    }
    fn surface_open(&mut self, title: &str) -> Result<u64, String> {
        let id = self.id();
        self.surfaces.insert(id, (0, 0, Vec::new()));
        self.calls.push(Call::SurfaceOpen(id, title.into()));
        Ok(id)
    }
    fn surface_resize(&mut self, surface: u64, width: u32, height: u32) {
        self.surfaces.insert(surface, (width, height, [0, 0, 0, 255].repeat((width * height) as usize)));
        self.calls.push(Call::Resize(surface, width, height));
    }
    fn surface_write(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]) {
        let (w, _, px) = self.surfaces.get_mut(&surface).unwrap();
        for row in 0..height {
            for col in 0..width {
                let from = ((row * width + col) * 4) as usize;
                let to = (((y + row) * *w + x + col) * 4) as usize;
                px[to..to + 4].copy_from_slice(&bgra[from..from + 4]);
            }
        }
    }
    fn surface_fill(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, (r, g, b): (u8, u8, u8)) {
        let (w, _, px) = self.surfaces.get_mut(&surface).unwrap();
        for row in y..y + height {
            for col in x..x + width {
                let to = ((row * *w + col) * 4) as usize;
                px[to..to + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
    }
    fn surface_copy(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32) {
        let (w, _, px) = self.surfaces.get_mut(&surface).unwrap();
        let src = px.clone();
        for row in 0..height {
            for col in 0..width {
                let from = (((y + row) * *w + x + col) * 4) as usize;
                let to = (((to_y + row) * *w + to_x + col) * 4) as usize;
                px[to..to + 4].copy_from_slice(&src[from..from + 4]);
            }
        }
    }
    fn surface_present(&mut self, surface: u64) {
        self.calls.push(Call::Present(surface));
    }
    fn surface_cursor(&mut self, surface: u64, cursor: Option<CursorImage>) {
        self.calls.push(Call::Cursor(surface, cursor));
    }
    fn retitle(&mut self, page: u64, title: &str) {
        self.calls.push(Call::Retitle(page, title.into()));
    }
    fn page_close(&mut self, page: u64) {
        self.surfaces.remove(&page);
        self.calls.push(Call::PageClose(page));
    }
    fn say(&mut self, level: Level, message: &str) {
        self.calls.push(Call::Say(level, message.into()));
    }
    fn copy(&mut self, text: &str) {
        self.calls.push(Call::Copy(text.into()));
    }
    fn pasted(&mut self) -> Option<String> {
        self.clipboard.clone()
    }
    fn pick(&mut self, _title: &str, items: &[(String, String, String)]) -> u64 {
        let id = self.id();
        self.calls.push(Call::Pick(id, items.iter().map(|(l, d, _)| format!("{l} {d}")).collect()));
        id
    }
    fn setting(&mut self, key: &str) -> Option<String> {
        self.settings.get(key).cloned()
    }
    fn stamp(&mut self) -> String {
        "20261028-141502".into()
    }
    fn save(&mut self, file: &str, png: &[u8]) -> Result<String, String> {
        self.saved.push((file.into(), png.to_vec()));
        self.calls.push(Call::Save(file.into()));
        Ok(format!("C:/shots/{file}"))
    }
}

const HOSTS: &str = r#"[["vm","127.0.0.1","5900"],["lab","10.0.0.6","5901"]]"#;

/// Carries the bytes both ways, `chunk` at a time, until neither side has
/// anything more to say.
fn pump(viewer: &mut Viewer, fake: &mut Fake, socket: u64, server: &mut Server, chunk: usize) {
    loop {
        let to_server = fake.sent.remove(&socket).unwrap_or_default();
        let to_client = std::mem::take(&mut server.out);
        if to_server.is_empty() && to_client.is_empty() {
            return;
        }
        server.feed(&to_server);
        for piece in to_client.chunks(chunk) {
            viewer.socket_data(fake, socket, piece);
        }
    }
}

/// `vm` opened and connected to `server`; its socket.
fn connected(viewer: &mut Viewer, fake: &mut Fake, server: &mut Server, chunk: usize) -> u64 {
    viewer.open(fake, "vm").unwrap();
    let socket = *fake.open.last().expect("dialled");
    viewer.socket_connected(fake, socket, Ok(()));
    pump(viewer, fake, socket, server, chunk);
    socket
}

fn first_update(server: &mut Server) -> Vec<u8> {
    let (w, h) = (server.width, server.height);
    let raw: Vec<[u8; 4]> = (0..4 * 3).map(|i| [i as u8, 2 * i as u8, 3 * i as u8, 255]).collect();
    let mut rects = vec![server.raw(1, 1, 4, 3, &raw)];
    // A ZRLE rectangle over most of it: 2 x 2 tiles and a ragged edge, each
    // tile its own kind.
    let stripes = |x: u16, y: u16| if (x / 3 + y / 5).is_multiple_of(3) { [200, 10, 10, 255] } else if x % 7 < 3 { [10, 200, 10, 255] } else { [10, 10, 200, 255] };
    rects.push(server.zrle(0, 8, w, h - 8, &[Tile::Raw, Tile::Packed, Tile::PlainRle, Tile::PaletteRle], stripes));
    // A solid one: comes out as a fill.
    rects.push(server.zrle(100, 0, 20, 6, &[Tile::Solid], |_, _| [1, 2, 3, 255]));
    rects.push(server.copy(10, 0, 4, 3, 1, 1));
    rects.push(cursor_rect(1, 0));
    fb_update(rects)
}

#[test]
fn it_connects_with_no_password_and_draws_what_the_server_sends() {
    for chunk in [1, 7, 4096] {
        let mut fake = Fake::with_hosts(HOSTS);
        let mut viewer = Viewer::new();
        let mut server = Server::new(130, 90);
        let update = first_update(&mut server);
        server.updates.push_back(update);
        connected(&mut viewer, &mut fake, &mut server, chunk);
        assert!(fake.calls.contains(&Call::Connect("127.0.0.1".into(), 5900)));
        assert_eq!(fake.said()[0], "Connecting to vm...");
        assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Live)]);
        // It asked for its own format and encodings, then for everything.
        assert_eq!(server.got[0], ClientMsg::PixelFormat(rfb::PIXEL_FORMAT));
        assert_eq!(server.got[1], ClientMsg::Encodings(rfb::ENCODINGS.to_vec()));
        assert_eq!(server.requests()[0], (false, 130, 90));
        let surface = fake.surface();
        assert_eq!(fake.titles(), ["VNC: vm"]);
        assert!(fake.calls.contains(&Call::Resize(surface, 130, 90)));
        assert!(fake.calls.contains(&Call::Present(surface)), "the whole update shown at its end");
        assert_eq!(fake.pixels(surface), server.fb, "chunks of {chunk}: the picture is the server's");
        let cursor = fake.calls.iter().find_map(|c| match c {
            Call::Cursor(_, Some(c)) => Some(c.clone()),
            _ => None,
        });
        assert_eq!(cursor, Some(CursorImage { width: 2, height: 2, hot_x: 1, hot_y: 0, bgra: vec![10, 20, 30, 255, 11, 21, 31, 255, 12, 22, 32, 0, 13, 23, 33, 0] }));
        // The next request waits for its timer, then asks for what changed.
        let (timer, ms) = fake.last_timer();
        assert_eq!(ms, viewer::ACTIVE_REFRESH_MILLIS);
        let socket = *fake.open.last().unwrap();
        viewer.timer(&mut fake, timer);
        pump(&mut viewer, &mut fake, socket, &mut server, chunk);
        assert_eq!(server.requests().last(), Some(&(true, 130, 90)));
    }
}

#[test]
fn the_zlib_stream_runs_on_from_one_rectangle_to_the_next() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(64, 64);
    let a = server.zrle(0, 0, 64, 32, &[Tile::PlainRle], |x, y| [x as u8, y as u8, 7, 255]);
    let b = server.zrle(0, 32, 64, 32, &[Tile::Raw], |x, y| [y as u8, x as u8, 9, 255]);
    server.updates.push_back(fb_update(vec![a]));
    server.updates.push_back(fb_update_last_rect(vec![b]));
    let socket = connected(&mut viewer, &mut fake, &mut server, 13);
    let (timer, _) = fake.last_timer();
    viewer.timer(&mut fake, timer);
    pump(&mut viewer, &mut fake, socket, &mut server, 13);
    assert_eq!(fake.pixels(fake.surface()), server.fb);
    assert_eq!(fake.calls.iter().filter(|c| matches!(c, Call::Present(_))).count(), 2, "LastRect ended the second update");
}

#[test]
fn vnc_authentication_and_the_older_versions_handshake() {
    for (greeting, security) in [
        (b"RFB 003.008\n", Security::Password("")),
        (b"RFB 003.007\n", Security::None),
        (b"RFB 003.007\n", Security::Password("")),
        (b"RFB 003.003\n", Security::None),
        (b"RFB 003.003\n", Security::Password("")),
        (b"RFB 003.889\n", Security::None),
    ] {
        let mut fake = Fake::with_hosts(HOSTS);
        let mut viewer = Viewer::new();
        let mut server = Server::new(8, 8).version(greeting).security(security);
        connected(&mut viewer, &mut fake, &mut server, 3);
        assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Live)], "{}", String::from_utf8_lossy(greeting));
        assert!(fake.errors().is_empty(), "{:?}", fake.errors());
    }
}

#[test]
fn a_server_wanting_a_password_is_refused_with_why() {
    for (greeting, said) in [(b"RFB 003.008\n", "couldn't connect to vm: authentication failed: wrong password"), (b"RFB 003.007\n", "couldn't connect to vm: authentication failed: the server wants a password")] {
        let mut fake = Fake::with_hosts(HOSTS);
        let mut viewer = Viewer::new();
        let mut server = Server::new(8, 8).version(greeting).security(Security::Password("secret"));
        connected(&mut viewer, &mut fake, &mut server, 64);
        assert_eq!(fake.errors(), [said]);
        assert!(viewer.sessions().is_empty(), "a first connect that fails is done");
        assert!(fake.surfaces.is_empty());
    }
}

#[test]
fn security_it_doesnt_speak_is_named() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8).security(Security::Only(vec![19, 18]));
    connected(&mut viewer, &mut fake, &mut server, 64);
    let errors = fake.errors();
    assert!(errors[0].contains("VeNCrypt, TLS") && errors[0].starts_with("couldn't connect to vm"), "{errors:?}");
}

#[test]
fn keys_the_pointer_the_wheel_and_the_clipboard_reach_the_guest() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(100, 50);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    let surface = fake.surface();
    let none = Modifiers { ctrl: false, alt: false, meta: false };
    let ctrl = Modifiers { ctrl: true, ..none };
    fake.clipboard = Some("from Fenix".into());
    viewer.input(&mut fake, surface, SurfaceInput::Capture(true));
    viewer.input(&mut fake, surface, SurfaceInput::Key(KeyEvent { key: SurfaceKey::Char('c'), down: true, mods: ctrl }));
    viewer.input(&mut fake, surface, SurfaceInput::Key(KeyEvent { key: SurfaceKey::Char('c'), down: false, mods: ctrl }));
    viewer.input(&mut fake, surface, SurfaceInput::Key(KeyEvent { key: SurfaceKey::Named(NamedKey::CapsLock), down: true, mods: none }));
    viewer.input(&mut fake, surface, SurfaceInput::Pointer(PointerEvent { x: 10, y: 20, buttons: 1, mods: none }));
    // Past the edge: kept to it.
    viewer.input(&mut fake, surface, SurfaceInput::Pointer(PointerEvent { x: 500, y: -3, buttons: 1, mods: none }));
    viewer.input(&mut fake, surface, SurfaceInput::Wheel(WheelEvent { x: 5, y: 6, lines: -3, mods: none }));
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    let input: Vec<ClientMsg> = server.got.iter().filter(|m| !matches!(m, ClientMsg::Request { .. } | ClientMsg::PixelFormat(_) | ClientMsg::Encodings(_))).cloned().collect();
    assert_eq!(
        input,
        [
            ClientMsg::CutText(b"from Fenix".to_vec()),
            ClientMsg::Key(0xFFE3, true),
            ClientMsg::Key(b'c' as u32, true),
            ClientMsg::Key(b'c' as u32, false),
            ClientMsg::Key(0xFFE3, false),
            ClientMsg::Key(0xFFE5, true),
            ClientMsg::Pointer(10, 20, 1),
            ClientMsg::Pointer(99, 0, 1),
            ClientMsg::Pointer(5, 6, 1 | 16),
            ClientMsg::Pointer(5, 6, 1),
        ]
    );
}

#[test]
fn the_guests_clipboard_and_bell_come_to_fenix() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    server.out.extend([3, 0, 0, 0, 0, 0, 0, 5]);
    server.out.extend(b"hello");
    server.out.extend([3, 0, 0, 0, 0, 0, 0, 3, b'c', 0xE9, b'!']);
    server.out.push(2);
    pump(&mut viewer, &mut fake, socket, &mut server, 2);
    assert!(fake.calls.contains(&Call::Copy("hello".into())));
    assert!(fake.calls.contains(&Call::Copy("cé!".into())), "Latin-1");
    assert!(fake.said().contains(&"VNC vm: bell".to_string()));
}

#[test]
fn the_desktop_is_resized_to_the_pane_once_the_server_says_it_can_be() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(80, 60);
    server.resizable = true;
    viewer.open(&mut fake, "vm").unwrap();
    let socket = *fake.open.last().unwrap();
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    let surface = fake.surface();
    viewer.resized(&mut fake, SurfaceSize { surface, width: 120, height: 70, scale: 1.0 });
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    assert!(server.got.contains(&ClientMsg::DesktopSize(120, 70)));
    assert!(fake.calls.contains(&Call::Resize(surface, 120, 70)), "the surface follows the desktop");
    // A real change: everything asked for again, at once.
    assert_eq!(server.requests().last(), Some(&(false, 120, 70)));
    // The same size again isn't asked for again.
    let before = server.got.len();
    viewer.resized(&mut fake, SurfaceSize { surface, width: 120, height: 70, scale: 1.0 });
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    assert_eq!(server.got.len(), before);
}

#[test]
fn a_server_that_cant_resize_is_never_asked_to() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(80, 60);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    let surface = fake.surface();
    viewer.resized(&mut fake, SurfaceSize { surface, width: 120, height: 70, scale: 1.0 });
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    assert!(!server.got.iter().any(|m| matches!(m, ClientMsg::DesktopSize(..))));
}

#[test]
fn hidden_it_asks_for_updates_less_often() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    server.updates.push_back(fb_update(vec![]));
    server.updates.push_back(fb_update(vec![]));
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    viewer.shown(fake.surface(), false);
    let (timer, _) = fake.last_timer();
    viewer.timer(&mut fake, timer);
    pump(&mut viewer, &mut fake, socket, &mut server, 64);
    assert_eq!(fake.last_timer().1, viewer::IDLE_REFRESH_MILLIS);
}

#[test]
fn a_dropped_connection_says_so_and_reconnects_keeping_the_picture() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(16, 16);
    let red: Vec<[u8; 4]> = vec![[0, 0, 255, 255]; 16 * 16];
    let update = fb_update(vec![server.raw(0, 0, 16, 16, &red)]);
    server.updates.push_back(update);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    let surface = fake.surface();
    viewer.socket_closed(&mut fake, socket, None);
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Reconnecting)]);
    assert_eq!(fake.titles().last().unwrap(), "VNC: vm (disconnected, reconnecting...)");
    assert_eq!(fake.errors().last().unwrap(), &format!("VNC vm disconnected: {} -- reconnecting...", viewer::SERVER_CLOSED));
    let (timer, ms) = fake.last_timer();
    assert_eq!(ms, 1000);
    // Keys while it's down go nowhere, and don't fail.
    viewer.input(&mut fake, surface, SurfaceInput::Key(KeyEvent { key: SurfaceKey::Char('x'), down: true, mods: Modifiers { ctrl: false, alt: false, meta: false } }));
    viewer.timer(&mut fake, timer);
    let again = *fake.open.last().unwrap();
    assert_ne!(again, socket);
    let mut server = Server::new(16, 16);
    pump(&mut viewer, &mut fake, again, &mut server, 64);
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Live)]);
    assert_eq!(fake.titles().last().unwrap(), "VNC: vm");
    assert!(fake.said().contains(&"VNC vm reconnected".to_string()));
    assert_eq!(fake.surface(), surface, "the same surface");
    assert!(fake.pixels(surface).iter().all(|p| *p == [0, 0, 255, 255]), "the last picture kept, not blanked");
}

#[test]
fn reconnecting_gives_up_after_its_attempts_and_spc_v_v_starts_again() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    viewer.socket_closed(&mut fake, socket, Some("reset".into()));
    let mut waits = Vec::new();
    for _ in 0..viewer::MAX_RECONNECT_ATTEMPTS {
        let (timer, ms) = fake.last_timer();
        waits.push(ms);
        viewer.timer(&mut fake, timer);
        let s = *fake.open.last().unwrap();
        viewer.socket_connected(&mut fake, s, Err("refused".into()));
    }
    assert_eq!(waits, [1000, 2000, 4000, 8000, 16000, 16000]);
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Down)]);
    assert_eq!(fake.titles().last().unwrap(), "VNC: vm (disconnected -- SPC v v to reconnect)");
    assert!(fake.errors().last().unwrap().starts_with("VNC vm: reconnect failed after 6 attempts (refused) -- giving up"));
    let connects = fake.calls.iter().filter(|c| matches!(c, Call::Connect(..))).count();
    viewer.open(&mut fake, "vm").unwrap();
    assert_eq!(fake.calls.iter().filter(|c| matches!(c, Call::Connect(..))).count(), connects + 1, "dialled at once");
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Reconnecting)]);
    let mut server = Server::new(8, 8);
    let s = *fake.open.last().unwrap();
    pump(&mut viewer, &mut fake, s, &mut server, 64);
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Live)]);
}

#[test]
fn a_first_connect_that_fails_says_why_and_leaves_nothing() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    viewer.open(&mut fake, "vm").unwrap();
    let socket = *fake.open.last().unwrap();
    viewer.socket_connected(&mut fake, socket, Err("connection refused".into()));
    assert_eq!(fake.errors(), ["couldn't connect to vm: connection refused"]);
    assert!(viewer.sessions().is_empty() && fake.surfaces.is_empty());
    // Not allowed at all: said the same way.
    fake.refuse = Some("10.0.0.6:5901 isn't one it may reach".into());
    viewer.open(&mut fake, "lab").unwrap();
    assert_eq!(fake.errors().last().unwrap(), "couldn't connect to lab: 10.0.0.6:5901 isn't one it may reach");
    assert_eq!(viewer.open(&mut fake, "nope"), Err("no VNC host configured named nope".into()));
}

#[test]
fn a_port_that_never_speaks_rfb_times_out() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    viewer.open(&mut fake, "vm").unwrap();
    let socket = *fake.open.last().unwrap();
    viewer.socket_connected(&mut fake, socket, Ok(()));
    let (timer, ms) = fake.last_timer();
    assert_eq!(ms, viewer::HANDSHAKE_MILLIS);
    // Opened again while it's dialling: nothing more.
    viewer.open(&mut fake, "vm").unwrap();
    assert_eq!(fake.calls.iter().filter(|c| matches!(c, Call::Connect(..))).count(), 1);
    viewer.timer(&mut fake, timer);
    assert_eq!(fake.errors(), ["couldn't connect to vm: no VNC handshake from 127.0.0.1:5900 within 10s"]);
    assert!(fake.calls.contains(&Call::Close(socket)));
}

#[test]
fn opening_a_live_session_again_shows_it_here_whole() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(16, 8);
    let green: Vec<[u8; 4]> = vec![[0, 255, 0, 255]; 16 * 8];
    let update = fb_update(vec![server.raw(0, 0, 16, 8, &green)]);
    server.updates.push_back(update);
    connected(&mut viewer, &mut fake, &mut server, 64);
    let old = fake.surface();
    viewer.open(&mut fake, "vm").unwrap();
    let new = fake.surface();
    assert_ne!(old, new);
    assert!(fake.calls.contains(&Call::PageClose(old)));
    assert_eq!(fake.pixels(new), server.fb, "drawn whole at once");
    assert_eq!(fake.calls.iter().filter(|c| matches!(c, Call::Connect(..))).count(), 1, "the same connection");
}

#[test]
fn spc_v_q_closes_the_session_youre_in() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    let surface = fake.surface();
    viewer.close_focused(&mut fake).unwrap();
    assert!(viewer.sessions().is_empty());
    assert!(fake.calls.contains(&Call::Close(socket)) && fake.calls.contains(&Call::PageClose(surface)));
    assert!(fake.timers.is_empty(), "its timers cancelled");
    assert_eq!(viewer.close_focused(&mut fake), Err("no VNC session open".into()));
    // A late event for it finds nothing.
    viewer.socket_closed(&mut fake, socket, None);
}

#[test]
fn with_several_and_none_in_view_it_asks_which() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut a = Server::new(8, 8);
    connected(&mut viewer, &mut fake, &mut a, 64);
    let vm = fake.surface();
    viewer.open(&mut fake, "lab").unwrap();
    let socket = *fake.open.last().unwrap();
    let mut b = Server::new(8, 8);
    pump(&mut viewer, &mut fake, socket, &mut b, 64);
    let lab = fake.surface();
    viewer.shown(vm, false);
    viewer.shown(lab, false);
    viewer.close_focused(&mut fake).unwrap();
    let Some(Call::Pick(id, items)) = fake.calls.iter().rev().find(|c| matches!(c, Call::Pick(..))).cloned() else { panic!("a picker") };
    assert_eq!(items, ["lab 10.0.0.6:5901", "vm 127.0.0.1:5900"]);
    viewer.picked(&mut fake, id, Some("lab"));
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Live)]);
}

#[test]
fn spc_v_s_saves_the_picture_as_a_png() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    assert_eq!(viewer.screenshot_focused(&mut fake), Err("no VNC session open".into()));
    let mut server = Server::new(3, 2);
    let px: Vec<[u8; 4]> = (0..6).map(|i| [i * 10, i * 20, i * 30, 255]).collect();
    let update = fb_update(vec![server.raw(0, 0, 3, 2, &px)]);
    server.updates.push_back(update);
    connected(&mut viewer, &mut fake, &mut server, 64);
    viewer.screenshot_focused(&mut fake).unwrap();
    let (file, png) = fake.saved.last().unwrap();
    assert_eq!(file, "vnc-vm-20261028-141502.png");
    assert_eq!(fake.said().last().unwrap(), "saved C:/shots/vnc-vm-20261028-141502.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(png.clone())).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    let want: Vec<u8> = server.fb.iter().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    assert_eq!(buf, want, "RGBA");
}

#[test]
fn the_picker_lists_the_hosts_and_how_each_stands() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    connected(&mut viewer, &mut fake, &mut server, 64);
    viewer.open_picker(&mut fake);
    let Some(Call::Pick(id, items)) = fake.calls.iter().rev().find(|c| matches!(c, Call::Pick(..))).cloned() else { panic!("a picker") };
    assert_eq!(items, ["vm 127.0.0.1:5900  connected", "lab 10.0.0.6:5901"]);
    viewer.picked(&mut fake, id, Some("lab"));
    assert!(fake.calls.contains(&Call::Connect("10.0.0.6".into(), 5901)));
    let mut none = Fake::default();
    viewer.open_picker(&mut none);
    assert_eq!(none.errors(), ["no VNC hosts yet -- add them in SPC , (vnc.hosts)"]);
}

#[test]
fn a_workspace_opens_a_host_through_the_service() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    assert_eq!(viewer.serve_connect(&mut fake, "vm"), Ok(String::new()));
    assert!(fake.calls.contains(&Call::Connect("127.0.0.1".into(), 5900)));
    assert_eq!(viewer.serve_connect(&mut fake, "\"nope\""), Err("no VNC host configured named nope".into()));
}

#[test]
fn a_server_speaking_nonsense_drops_the_connection() {
    let mut fake = Fake::with_hosts(HOSTS);
    let mut viewer = Viewer::new();
    let mut server = Server::new(8, 8);
    let socket = connected(&mut viewer, &mut fake, &mut server, 64);
    viewer.socket_data(&mut fake, socket, &[77]);
    assert_eq!(viewer.sessions(), [("vm".to_string(), Link::Reconnecting)]);
    assert!(fake.errors().last().unwrap().contains("a message Fenix doesn't know (type 77)"));
    assert!(fake.calls.contains(&Call::Close(socket)));
}
