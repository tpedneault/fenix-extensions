//! The viewer: every session, what Fenix says to it, and what it asks of
//! Fenix in turn -- through `Host`, so all of it runs under a native test
//! with a fake host and a fake RFB server (`tests/`).
//!
//! A session is a host from `vnc.hosts`, by name. It's dialled the first
//! time it's opened, and then stays connected in the background, shown or
//! not, until it's closed (`SPC v q`) -- as 1.x's were. Its surface opens
//! once the handshake's done; a connection that drops is dialled again,
//! backing off, and gives up after a few tries until it's opened again.

use std::collections::BTreeMap;

use fenix_extension_api::{CursorImage, KeyEvent, Level, PointerEvent, SurfaceInput, SurfaceSize, WheelEvent};

use crate::framebuffer::Framebuffer;
use crate::keys;
use crate::rfb::{self, Frame};

/// How long a connection has for its whole handshake (TCP and RFB). A
/// port that takes the connection but never speaks RFB -- a container's
/// port proxy with nothing behind it yet -- would otherwise leave it
/// "connecting" for good.
pub const HANDSHAKE_MILLIS: u32 = 10_000;

/// The least time between one update ending and asking for the next,
/// while it's shown: about 60 a second. One request is out at a time, so
/// the real rate is what the server manages; this only stops a server
/// that answers at once (with nothing) from spinning.
pub const ACTIVE_REFRESH_MILLIS: u32 = 16;

/// The same while it's in no pane: current enough that showing it again
/// doesn't start from a stale frame, and cheap.
pub const IDLE_REFRESH_MILLIS: u32 = 500;

/// How many times in a row a dropped session is dialled again before it
/// waits for you (`SPC v v`).
pub const MAX_RECONNECT_ATTEMPTS: u32 = 6;

/// What a server closing its end says: the VNC server stopped, the VM
/// shut down.
pub const SERVER_CLOSED: &str = "the server closed the connection";

/// The wait before reconnect attempt `attempts` (from 0): 1, 2, 4, 8, 16
/// seconds, then 16 again.
pub fn reconnect_delay_millis(attempts: u32) -> u32 {
    2u32.saturating_pow(attempts.min(4)).min(30) * 1000
}

/// Where a session's connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Dialling it the first time: nothing to show yet.
    Connecting,
    Live,
    /// It dropped, and it's being dialled again.
    Reconnecting,
    /// Reconnecting gave up: `SPC v v` starts again.
    Down,
}

/// The surface's title for a session `link` -- the one place a dropped
/// connection shows on the pane itself, so a frozen last frame doesn't
/// look like an idle desktop.
pub fn title(name: &str, link: Link) -> String {
    match link {
        Link::Connecting | Link::Live => format!("VNC: {name}"),
        Link::Reconnecting => format!("VNC: {name} (disconnected, reconnecting...)"),
        Link::Down => format!("VNC: {name} (disconnected -- SPC v v to reconnect)"),
    }
}

/// What to do with a resolution the server reports, given the last one
/// shown (`None` before the first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionAction {
    /// Resize to it. `full_refresh` for a real change: a server needn't
    /// send pixels with the change itself, so the next request asks for
    /// everything rather than what changed.
    Resize { full_refresh: bool },
    /// The size it is already -- an ExtendedDesktopSize announcement, a
    /// refused request's reply, a reconnect to the same desktop: resizing
    /// would only blank a good picture.
    Keep,
}

pub fn resolution_action(previous: Option<(u16, u16)>, new: (u16, u16)) -> ResolutionAction {
    match previous {
        None => ResolutionAction::Resize { full_refresh: false },
        Some(prev) if prev != new => ResolutionAction::Resize { full_refresh: true },
        Some(_) => ResolutionAction::Keep,
    }
}

/// A configured host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
}

/// `vnc.hosts` as the setting gives it: JSON rows of name, host and port
/// (5900 when it's left out). Rows with no name or host are skipped.
pub fn hosts(setting: Option<&str>) -> Vec<HostEntry> {
    let Some(rows) = setting.and_then(|s| serde_json::from_str::<Vec<Vec<serde_json::Value>>>(s).ok()) else { return Vec::new() };
    let text = |v: Option<&serde_json::Value>| match v {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    rows.iter()
        .filter_map(|row| {
            let (name, host, port) = (text(row.first()), text(row.get(1)), text(row.get(2)));
            (!name.is_empty() && !host.is_empty()).then(|| HostEntry { name, host, port: port.parse().unwrap_or(5900) })
        })
        .collect()
}

/// A screenshot's file name: the session's, made safe, and the time.
pub fn screenshot_name(name: &str, stamp: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    format!("vnc-{safe}-{stamp}.png")
}

/// What the viewer asks of Fenix. The extension's is the real host; the
/// tests' records it.
pub trait Host {
    fn connect(&mut self, host: &str, port: u16) -> Result<u64, String>;
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String>;
    fn close(&mut self, socket: u64);
    fn timer(&mut self, millis: u32) -> u64;
    fn cancel_timer(&mut self, timer: u64);
    fn surface_open(&mut self, title: &str) -> Result<u64, String>;
    fn surface_resize(&mut self, surface: u64, width: u32, height: u32);
    fn surface_write(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]);
    fn surface_fill(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, rgb: (u8, u8, u8));
    #[allow(clippy::too_many_arguments)]
    fn surface_copy(&mut self, surface: u64, x: u32, y: u32, width: u32, height: u32, to_x: u32, to_y: u32);
    fn surface_present(&mut self, surface: u64);
    fn surface_cursor(&mut self, surface: u64, cursor: Option<CursorImage>);
    fn retitle(&mut self, page: u64, title: &str);
    fn page_close(&mut self, page: u64);
    fn say(&mut self, level: Level, message: &str);
    fn copy(&mut self, text: &str);
    fn pasted(&mut self) -> Option<String>;
    fn pick(&mut self, title: &str, items: &[(String, String, String)]) -> u64;
    fn setting(&mut self, key: &str) -> Option<String>;
    /// The time where you are, for a file name: `20261028-141502`.
    fn stamp(&mut self) -> String;
    /// Writes a screenshot: where it went, or why it couldn't.
    fn save(&mut self, file: &str, png: &[u8]) -> Result<String, String>;
}

struct Session {
    host: String,
    port: u16,
    link: Link,
    /// Whether it's never been connected yet: a first connect that fails
    /// says so and is done, rather than dialling a typo forever.
    first: bool,
    socket: Option<u64>,
    rfb: Option<rfb::Client>,
    handshake_timer: Option<u64>,
    reconnect_timer: Option<u64>,
    refresh_timer: Option<u64>,
    attempts: u32,
    surface: Option<u64>,
    fb: Framebuffer,
    shown: bool,
    /// An update request is out and not yet answered.
    awaiting: bool,
    full_refresh: bool,
    resolution: Option<(u16, u16)>,
    resize_supported: bool,
    /// The pane's size, which the desktop is asked to be.
    wanted: Option<(u16, u16)>,
    requested: Option<(u16, u16)>,
    buttons: u8,
    cursor: Option<CursorImage>,
}

impl Session {
    fn new(host: String, port: u16) -> Session {
        Session {
            host,
            port,
            link: Link::Connecting,
            first: true,
            socket: None,
            rfb: None,
            handshake_timer: None,
            reconnect_timer: None,
            refresh_timer: None,
            attempts: 0,
            surface: None,
            fb: Framebuffer::new(),
            shown: true,
            awaiting: false,
            full_refresh: false,
            resolution: None,
            resize_supported: false,
            wanted: None,
            requested: None,
            buttons: 0,
            cursor: None,
        }
    }
}

/// What a picker that's open is for.
enum Picking {
    Open,
    Close,
    Screenshot,
}

#[derive(Default)]
pub struct Viewer {
    sessions: BTreeMap<String, Session>,
    picking: Option<(u64, Picking)>,
    /// The session you were last in: the one that last had the keyboard
    /// or the pointer.
    last: Option<String>,
}

impl Viewer {
    pub fn new() -> Viewer {
        Viewer::default()
    }

    /// The sessions there are, and how each stands.
    pub fn sessions(&self) -> Vec<(String, Link)> {
        self.sessions.iter().map(|(n, s)| (n.clone(), s.link)).collect()
    }

    fn by_socket(&self, socket: u64) -> Option<String> {
        self.sessions.iter().find(|(_, s)| s.socket == Some(socket)).map(|(n, _)| n.clone())
    }

    fn by_surface(&self, surface: u64) -> Option<String> {
        self.sessions.iter().find(|(_, s)| s.surface == Some(surface)).map(|(n, _)| n.clone())
    }

    fn by_timer(&self, timer: u64) -> Option<String> {
        self.sessions.iter().find(|(_, s)| [s.handshake_timer, s.reconnect_timer, s.refresh_timer].contains(&Some(timer))).map(|(n, _)| n.clone())
    }

    // Commands.

    /// `SPC v v`: a picker over the configured hosts.
    pub fn open_picker(&mut self, host: &mut dyn Host) {
        let configured = hosts(host.setting("vnc.hosts").as_deref());
        if configured.is_empty() {
            host.say(Level::Error, "no VNC hosts yet -- add them in SPC , (vnc.hosts)");
            return;
        }
        let items: Vec<(String, String, String)> = configured
            .iter()
            .map(|h| {
                let state = match self.sessions.get(&h.name).map(|s| s.link) {
                    Some(Link::Live) => "  connected",
                    Some(Link::Connecting) => "  connecting",
                    Some(Link::Reconnecting) => "  reconnecting",
                    Some(Link::Down) => "  disconnected",
                    None => "",
                };
                (h.name.clone(), format!("{}:{}{state}", h.host, h.port), h.name.clone())
            })
            .collect();
        let id = host.pick("VNC", &items);
        self.picking = Some((id, Picking::Open));
    }

    /// Opens session `name`: dials it the first time, else shows it here.
    pub fn open(&mut self, host: &mut dyn Host, name: &str) -> Result<(), String> {
        if let Some(s) = self.sessions.get_mut(name) {
            match s.link {
                // `SPC v v` twice before it's through: it's still dialling.
                Link::Connecting => return Ok(()),
                Link::Down => {
                    self.show_here(host, name);
                    self.reconnect_now(host, name);
                }
                Link::Reconnecting => {
                    self.show_here(host, name);
                    host.say(Level::Info, &format!("VNC {name}: still reconnecting..."));
                }
                Link::Live => self.show_here(host, name),
            }
            return Ok(());
        }
        let Some(entry) = hosts(host.setting("vnc.hosts").as_deref()).into_iter().find(|h| h.name == name) else {
            return Err(format!("no VNC host configured named {name}"));
        };
        self.sessions.insert(name.to_string(), Session::new(entry.host, entry.port));
        host.say(Level::Info, &format!("Connecting to {name}..."));
        self.dial(host, name);
        Ok(())
    }

    /// The session's surface, opened in the pane you're in -- a new one in
    /// place of any it had, which Fenix gives no other way to bring here --
    /// and drawn whole from what it last showed.
    fn show_here(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        if let Some(old) = s.surface.take() {
            host.page_close(old);
        }
        match host.surface_open(&title(name, s.link)) {
            Ok(surface) => {
                s.surface = Some(surface);
                s.shown = true;
                let (w, h) = s.fb.dimensions();
                if !s.fb.is_empty() {
                    host.surface_resize(surface, w as u32, h as u32);
                    host.surface_write(surface, 0, 0, w as u32, h as u32, s.fb.pixels());
                    host.surface_present(surface);
                }
                if s.cursor.is_some() {
                    host.surface_cursor(surface, s.cursor.clone());
                }
                self.last = Some(name.to_string());
            }
            Err(e) => host.say(Level::Error, &format!("VNC {name}: {e}")),
        }
    }

    /// `SPC v q`.
    pub fn close_focused(&mut self, host: &mut dyn Host) -> Result<(), String> {
        match self.focused() {
            Some(name) => {
                self.close(host, &name);
                Ok(())
            }
            None if self.sessions.len() > 1 => {
                self.pick_session(host, "Close VNC session", Picking::Close);
                Ok(())
            }
            None => Err("no VNC session open".into()),
        }
    }

    /// Ends session `name`: its connection, its surface, its timers.
    pub fn close(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.remove(name) else { return };
        if let Some(socket) = s.socket {
            host.close(socket);
        }
        for timer in [s.handshake_timer, s.reconnect_timer, s.refresh_timer].into_iter().flatten() {
            host.cancel_timer(timer);
        }
        if let Some(surface) = s.surface {
            host.page_close(surface);
        }
        if self.last.as_deref() == Some(name) {
            self.last = None;
        }
    }

    /// `SPC v s`: the focused session's picture, as a PNG.
    pub fn screenshot_focused(&mut self, host: &mut dyn Host) -> Result<(), String> {
        match self.focused() {
            Some(name) => self.screenshot(host, &name),
            None if self.sessions.len() > 1 => {
                self.pick_session(host, "Screenshot of VNC session", Picking::Screenshot);
                Ok(())
            }
            None => Err("no VNC session open".into()),
        }
    }

    pub fn screenshot(&mut self, host: &mut dyn Host, name: &str) -> Result<(), String> {
        let s = self.sessions.get(name).ok_or_else(|| format!("no VNC session {name}"))?;
        if s.fb.is_empty() {
            return Err(format!("VNC {name}: no frame to save yet"));
        }
        let (w, h) = s.fb.dimensions();
        let png = crate::screenshot::png(w as u32, h as u32, &s.fb.rgba())?;
        let file = screenshot_name(name, &host.stamp());
        let at = host.save(&file, &png).map_err(|e| format!("couldn't save screenshot: {e}"))?;
        host.say(Level::Info, &format!("saved {at}"));
        Ok(())
    }

    /// The session you're in: the one you last were, if it's shown; else
    /// the only one shown; else the only one.
    fn focused(&self) -> Option<String> {
        if let Some(last) = self.last.as_ref().filter(|l| self.sessions.get(*l).is_some_and(|s| s.shown && s.surface.is_some())) {
            return Some(last.clone());
        }
        let shown: Vec<&String> = self.sessions.iter().filter(|(_, s)| s.shown && s.surface.is_some()).map(|(n, _)| n).collect();
        if shown.len() == 1 {
            return Some(shown[0].clone());
        }
        (self.sessions.len() == 1).then(|| self.sessions.keys().next().cloned()).flatten()
    }

    fn pick_session(&mut self, host: &mut dyn Host, title: &str, what: Picking) {
        let items: Vec<(String, String, String)> = self.sessions.iter().map(|(n, s)| (n.clone(), format!("{}:{}", s.host, s.port), n.clone())).collect();
        let id = host.pick(title, &items);
        self.picking = Some((id, what));
    }

    /// A picker closed.
    pub fn picked(&mut self, host: &mut dyn Host, id: u64, value: Option<&str>) -> bool {
        let Some((asked, _)) = &self.picking else { return false };
        if *asked != id {
            return false;
        }
        let (_, what) = self.picking.take().expect("there");
        let Some(name) = value else { return true };
        let result = match what {
            Picking::Open => self.open(host, name),
            Picking::Close => {
                self.close(host, name);
                Ok(())
            }
            Picking::Screenshot => self.screenshot(host, name),
        };
        if let Err(e) = result {
            host.say(Level::Error, &e);
        }
        true
    }

    // The connection.

    fn dial(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        s.rfb = Some(rfb::Client::new(""));
        match host.connect(&s.host, s.port) {
            Ok(socket) => {
                s.socket = Some(socket);
                s.handshake_timer = Some(host.timer(HANDSHAKE_MILLIS));
            }
            Err(e) => self.failed(host, name, &e),
        }
    }

    /// A connection that never got through its handshake.
    fn failed(&mut self, host: &mut dyn Host, name: &str, why: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        if let Some(socket) = s.socket.take() {
            host.close(socket);
        }
        if let Some(timer) = s.handshake_timer.take() {
            host.cancel_timer(timer);
        }
        s.rfb = None;
        if s.first {
            self.sessions.remove(name);
            host.say(Level::Error, &format!("couldn't connect to {name}: {why}"));
            return;
        }
        s.attempts += 1;
        let attempts = s.attempts;
        if attempts >= MAX_RECONNECT_ATTEMPTS {
            s.link = Link::Down;
            self.retitle(host, name);
            host.say(Level::Error, &format!("VNC {name}: reconnect failed after {attempts} attempts ({why}) -- giving up; SPC v v {name} to try again"));
        } else {
            host.say(Level::Error, &format!("VNC {name}: reconnect failed ({why}); retrying..."));
            s.reconnect_timer = Some(host.timer(reconnect_delay_millis(attempts)));
        }
    }

    /// A live connection that ended.
    fn dropped(&mut self, host: &mut dyn Host, name: &str, why: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        if let Some(socket) = s.socket.take() {
            host.close(socket);
        }
        if let Some(timer) = s.refresh_timer.take() {
            host.cancel_timer(timer);
        }
        s.rfb = None;
        s.awaiting = false;
        s.buttons = 0;
        s.link = Link::Reconnecting;
        s.attempts = 0;
        s.reconnect_timer = Some(host.timer(reconnect_delay_millis(0)));
        self.retitle(host, name);
        host.say(Level::Error, &format!("VNC {name} disconnected: {why} -- reconnecting..."));
    }

    /// `SPC v v` on a session that gave up: again, at once, with every
    /// attempt it had.
    fn reconnect_now(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        s.link = Link::Reconnecting;
        s.attempts = 0;
        if let Some(timer) = s.reconnect_timer.take() {
            host.cancel_timer(timer);
        }
        self.retitle(host, name);
        host.say(Level::Info, &format!("Reconnecting to {name}..."));
        self.dial(host, name);
    }

    fn retitle(&mut self, host: &mut dyn Host, name: &str) {
        if let Some((surface, link)) = self.sessions.get(name).and_then(|s| s.surface.map(|p| (p, s.link))) {
            host.retitle(surface, &title(name, link));
        }
    }

    /// What it has to send, sent.
    fn flush(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        let (Some(socket), Some(rfb)) = (s.socket, s.rfb.as_mut()) else { return };
        let out = rfb.take_outgoing();
        if !out.is_empty() {
            if let Err(e) = host.write(socket, &out) {
                self.dropped_or_failed(host, name, &e);
            }
        }
    }

    fn dropped_or_failed(&mut self, host: &mut dyn Host, name: &str, why: &str) {
        let live = self.sessions.get(name).is_some_and(|s| s.link == Link::Live);
        if live {
            self.dropped(host, name, why);
        } else {
            self.failed(host, name, why);
        }
    }

    pub fn socket_connected(&mut self, host: &mut dyn Host, socket: u64, result: Result<(), String>) {
        let Some(name) = self.by_socket(socket) else { return };
        if let Err(e) = result {
            self.failed(host, &name, &e);
        }
    }

    pub fn socket_closed(&mut self, host: &mut dyn Host, socket: u64, why: Option<String>) {
        let Some(name) = self.by_socket(socket) else { return };
        if let Some(s) = self.sessions.get_mut(&name) {
            // Closed already, on our side.
            s.socket = None;
        }
        self.dropped_or_failed(host, &name, why.as_deref().unwrap_or(SERVER_CLOSED));
    }

    pub fn socket_data(&mut self, host: &mut dyn Host, socket: u64, data: &[u8]) {
        let Some(name) = self.by_socket(socket) else { return };
        let Some(rfb) = self.sessions.get_mut(&name).and_then(|s| s.rfb.as_mut()) else { return };
        let result = rfb.feed(data);
        self.flush(host, &name);
        match result {
            Ok(frames) => {
                for frame in frames {
                    self.frame(host, &name, frame);
                }
                self.flush(host, &name);
            }
            Err(e) => self.dropped_or_failed(host, &name, &e),
        }
    }

    fn frame(&mut self, host: &mut dyn Host, name: &str, frame: Frame) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        match frame {
            Frame::Ready { .. } => {
                if let Some(timer) = s.handshake_timer.take() {
                    host.cancel_timer(timer);
                }
                // The first request, for everything, went with the handshake.
                s.awaiting = true;
                s.requested = None;
                s.resize_supported = false;
                let first = s.first;
                s.first = false;
                s.link = Link::Live;
                s.attempts = 0;
                if first {
                    self.show_here(host, name);
                } else {
                    self.retitle(host, name);
                    host.say(Level::Info, &format!("VNC {name} reconnected"));
                }
            }
            Frame::Resolution { width, height } => {
                let action = resolution_action(s.resolution, (width, height));
                s.resolution = Some((width, height));
                if let ResolutionAction::Resize { full_refresh } = action {
                    s.full_refresh |= full_refresh;
                    s.fb.resize(width, height);
                    if let Some(surface) = s.surface {
                        host.surface_resize(surface, width as u32, height as u32);
                    }
                }
            }
            Frame::Rect { x, y, width, height, bgra } => {
                s.fb.apply_rect(x, y, width, height, &bgra);
                if let Some(surface) = s.surface {
                    host.surface_write(surface, x as u32, y as u32, width as u32, height as u32, &bgra);
                }
            }
            Frame::Fill { x, y, width, height, rgb } => {
                s.fb.fill(x, y, width, height, rgb);
                if let Some(surface) = s.surface {
                    host.surface_fill(surface, x as u32, y as u32, width as u32, height as u32, rgb);
                }
            }
            Frame::Copy { x, y, width, height, from_x, from_y } => {
                s.fb.apply_copy(x, y, width, height, from_x, from_y);
                if let Some(surface) = s.surface {
                    host.surface_copy(surface, from_x as u32, from_y as u32, width as u32, height as u32, x as u32, y as u32);
                }
            }
            Frame::UpdateEnd => {
                if let Some(surface) = s.surface {
                    host.surface_present(surface);
                }
                s.awaiting = false;
                if s.full_refresh {
                    self.request(host, name);
                } else {
                    s.refresh_timer = Some(host.timer(if s.shown { ACTIVE_REFRESH_MILLIS } else { IDLE_REFRESH_MILLIS }));
                }
            }
            Frame::Cursor { width, height, hot_x, hot_y, bgra } => {
                // A cursor of no size: none drawn there.
                s.cursor = (width > 0 && height > 0).then_some(CursorImage { width: width as u32, height: height as u32, hot_x: hot_x as u32, hot_y: hot_y as u32, bgra });
                if let Some(surface) = s.surface {
                    host.surface_cursor(surface, s.cursor.clone());
                }
            }
            Frame::Bell => host.say(Level::Info, &format!("VNC {name}: bell")),
            Frame::ClipboardText(text) => host.copy(&text),
            Frame::ResizeSupported => {
                s.resize_supported = true;
                self.resize_desktop(name);
            }
        }
    }

    /// The next update asked for: what changed, or all of it after a
    /// resize. One at a time.
    fn request(&mut self, host: &mut dyn Host, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        if s.link != Link::Live || s.awaiting {
            return;
        }
        let Some(rfb) = s.rfb.as_mut() else { return };
        rfb.request_update(!s.full_refresh);
        s.full_refresh = false;
        s.awaiting = true;
        self.flush(host, name);
    }

    /// The desktop asked to be the pane's size, once the server can be
    /// asked and if it isn't already.
    fn resize_desktop(&mut self, name: &str) {
        let Some(s) = self.sessions.get_mut(name) else { return };
        let (Some(want), true, Some(rfb)) = (s.wanted, s.resize_supported, s.rfb.as_mut()) else { return };
        if s.requested == Some(want) || s.resolution == Some(want) {
            return;
        }
        rfb.set_desktop_size(want.0, want.1);
        s.requested = Some(want);
    }

    pub fn timer(&mut self, host: &mut dyn Host, timer: u64) {
        let Some(name) = self.by_timer(timer) else { return };
        let Some(s) = self.sessions.get_mut(&name) else { return };
        if s.handshake_timer == Some(timer) {
            s.handshake_timer = None;
            let why = format!("no VNC handshake from {}:{} within {}s", s.host, s.port, HANDSHAKE_MILLIS / 1000);
            self.failed(host, &name, &why);
        } else if s.reconnect_timer == Some(timer) {
            s.reconnect_timer = None;
            self.dial(host, &name);
        } else {
            s.refresh_timer = None;
            self.request(host, &name);
        }
    }

    // The surface.

    pub fn shown(&mut self, page: u64, shown: bool) {
        if let Some(name) = self.by_surface(page) {
            if let Some(s) = self.sessions.get_mut(&name) {
                s.shown = shown;
            }
        }
    }

    pub fn resized(&mut self, host: &mut dyn Host, size: SurfaceSize) {
        let Some(name) = self.by_surface(size.surface) else { return };
        let Some(s) = self.sessions.get_mut(&name) else { return };
        s.wanted = Some((size.width.clamp(1, 4096) as u16, size.height.clamp(1, 4096) as u16));
        self.resize_desktop(&name);
        self.flush(host, &name);
    }

    pub fn input(&mut self, host: &mut dyn Host, surface: u64, input: SurfaceInput) {
        let Some(name) = self.by_surface(surface) else { return };
        self.last = Some(name.clone());
        let Some(s) = self.sessions.get_mut(&name) else { return };
        if s.link != Link::Live {
            return;
        }
        let (w, h) = s.fb.dimensions();
        let at = |x: i32, y: i32| (x.clamp(0, w.saturating_sub(1) as i32) as u16, y.clamp(0, h.saturating_sub(1) as i32) as u16);
        let pasted = matches!(input, SurfaceInput::Capture(true)).then(|| host.pasted()).flatten();
        let Some(rfb) = s.rfb.as_mut() else { return };
        match input {
            SurfaceInput::Key(KeyEvent { key, down, mods }) => {
                for (sym, sym_down) in keys::key_sequence(keys::keysym_for(&key), mods, down) {
                    rfb.key(sym, sym_down);
                }
            }
            SurfaceInput::Pointer(PointerEvent { x, y, buttons, .. }) => {
                s.buttons = buttons & 0x07;
                let (x, y) = at(x, y);
                rfb.pointer(x, y, s.buttons);
            }
            // A wheel notch: its button pressed and let go where the
            // pointer is.
            SurfaceInput::Wheel(WheelEvent { x, y, lines, .. }) => {
                if lines != 0 {
                    let bit = if lines > 0 { 0x08 } else { 0x10 };
                    let (x, y) = at(x, y);
                    rfb.pointer(x, y, s.buttons | bit);
                    rfb.pointer(x, y, s.buttons);
                }
            }
            // Taking the keyboard, it takes your clipboard to the guest:
            // its own paste keys then paste it.
            SurfaceInput::Capture(true) => {
                if let Some(text) = pasted.filter(|t| !t.is_empty()) {
                    rfb.cut_text(&text);
                }
            }
            SurfaceInput::Capture(false) => {}
        }
        self.flush(host, &name);
    }

    /// The `vnc.connect` service: a `[workspaces]` entry's `vnc:NAME`.
    pub fn serve_connect(&mut self, host: &mut dyn Host, name: &str) -> Result<String, String> {
        self.open(host, name.trim().trim_matches('"')).map(|_| String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_say_how_the_connection_stands() {
        assert_eq!(title("vm", Link::Live), "VNC: vm");
        assert_eq!(title("vm", Link::Reconnecting), "VNC: vm (disconnected, reconnecting...)");
        assert_eq!(title("vm", Link::Down), "VNC: vm (disconnected -- SPC v v to reconnect)");
    }

    #[test]
    fn reconnects_back_off_and_are_capped() {
        let waits: Vec<u32> = (0..7).map(reconnect_delay_millis).collect();
        assert_eq!(waits, [1000, 2000, 4000, 8000, 16000, 16000, 16000]);
    }

    #[test]
    fn the_first_resolution_resizes_without_asking_for_everything() {
        assert_eq!(resolution_action(None, (1024, 768)), ResolutionAction::Resize { full_refresh: false });
    }

    #[test]
    fn a_real_change_resizes_and_asks_for_everything() {
        assert_eq!(resolution_action(Some((1024, 768)), (1280, 800)), ResolutionAction::Resize { full_refresh: true });
    }

    #[test]
    fn the_same_size_again_is_kept() {
        // An ExtendedDesktopSize announcement after the handshake, a
        // refused SetDesktopSize's reply: the size as it was.
        assert_eq!(resolution_action(Some((1024, 768)), (1024, 768)), ResolutionAction::Keep);
    }

    #[test]
    fn hosts_are_read_from_the_settings_rows() {
        let rows = r#"[["build-vm","10.0.0.5","5900"],["test-vm","vm.local","5901"],["no-port","10.0.0.7",""],["","x","1"],["bad","",""]]"#;
        assert_eq!(
            hosts(Some(rows)),
            [
                HostEntry { name: "build-vm".into(), host: "10.0.0.5".into(), port: 5900 },
                HostEntry { name: "test-vm".into(), host: "vm.local".into(), port: 5901 },
                HostEntry { name: "no-port".into(), host: "10.0.0.7".into(), port: 5900 },
            ]
        );
        assert_eq!(hosts(Some(r#"[["n","h",5902]]"#))[0].port, 5902, "a number as it is");
        assert!(hosts(None).is_empty() && hosts(Some("")).is_empty() && hosts(Some("nonsense")).is_empty());
    }

    #[test]
    fn a_screenshots_name_is_safe_for_a_file() {
        assert_eq!(screenshot_name("build vm/1", "20261028-141502"), "vnc-build-vm-1-20261028-141502.png");
    }
}
