//! Discord Rich Presence for Fenix, as a WebAssembly extension: the
//! project, the file and its language, and how long you've been at it,
//! on your Discord profile, and what you're debugging or the task
//! that's running. `frame` is Discord's IPC framing, `presence`
//! what's shown and `link` the connection, in plain Rust; here they're
//! tied to Fenix's local sockets, timers, settings and where you are.

pub mod frame;
pub mod link;
pub mod presence;

use std::time::{SystemTime, UNIX_EPOCH};

use fenix_extension_api::{self as api, export, host, socket, DebugState, Event, Extension, Focus};

use link::Link;
use presence::{Debug, Editor, Elapsed, Privacy, Project};

/// How often it looks whether you've gone idle, in ms.
const IDLE_CHECK: u32 = 30_000;
/// What it says while it has no application ID.
const NO_ID: &str = "Discord: set discord.client_id to your Discord application's ID to show what you're working on (the extension's README says how)";

/// Fenix, as the link asks things of it.
struct Fenix;

impl link::Host for Fenix {
    fn ipc(&mut self, name: &str) -> Result<u64, String> {
        socket::ipc(name)
    }
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
        socket::write(socket, data)
    }
    fn close(&mut self, socket: u64) {
        socket::close(socket)
    }
    fn timer(&mut self, millis: u32) -> u64 {
        host::set_timer(millis, false)
    }
    fn cancel_timer(&mut self, timer: u64) {
        host::cancel_timer(timer)
    }
    fn now(&mut self) -> u64 {
        now()
    }
    fn log(&mut self, message: &str) {
        api::info!("{message}");
    }
    fn notify(&mut self, message: &str) {
        api::notify(message);
    }
}

/// Now, in ms since 1970.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Discord {
    link: Link,
    editor: Editor,
    privacy: Privacy,
    client_id: String,
    /// Minutes without a key before you're idle; 0, never.
    idle: u32,
    idle_timer: Option<u64>,
    /// Turned off for this session (`discord.toggle`).
    off: bool,
}

impl Default for Discord {
    fn default() -> Discord {
        Discord { link: Link::new(Vec::new()), editor: Editor::new(now()), privacy: Privacy::default(), client_id: String::new(), idle: 5, idle_timer: None, off: false }
    }
}

impl Discord {
    /// The settings, read again; whether the application ID changed.
    fn settings(&mut self) -> bool {
        let on = |key: &str| api::setting(key).as_deref() == Some("true");
        self.privacy = Privacy {
            hide_files: on("discord.hide_files"),
            hide_projects: on("discord.hide_projects"),
            private: api::setting("discord.private").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
            elapsed: Elapsed::parse(&api::setting("discord.elapsed").unwrap_or_default()),
        };
        self.idle = api::setting("discord.idle").and_then(|v| v.trim().parse().ok()).unwrap_or(5);
        let id = api::setting("discord.client_id").unwrap_or_default().trim().to_string();
        let changed = id != self.client_id;
        self.client_id = id;
        changed
    }

    /// Connected as the application, or not at all.
    fn connect(&mut self) {
        if self.off || self.client_id.is_empty() {
            self.link.stop(&mut Fenix);
        } else {
            self.link.start(&mut Fenix, &self.client_id);
        }
        self.idle_check();
    }

    /// The idle check's timer, while there's an idle to look for.
    fn idle_check(&mut self) {
        let want = self.idle > 0 && !self.off && !self.client_id.is_empty();
        match (want, self.idle_timer) {
            (true, None) => self.idle_timer = Some(host::set_timer(IDLE_CHECK, true)),
            (false, Some(timer)) => {
                host::cancel_timer(timer);
                self.idle_timer = None;
            }
            _ => {}
        }
    }

    /// Discord told what to show now.
    fn update(&mut self) {
        let activity = if self.off { None } else { presence::activity(&self.editor, &self.privacy) };
        self.link.set(&mut Fenix, activity);
    }

    /// You're in `path`.
    fn moved(&mut self, path: &str) {
        let project = api::here().and_then(|h| h.project).map(|p| Project { name: p.name, root: p.root });
        if self.editor.moved(path, project, now()) {
            self.update();
        }
    }
}

impl Extension for Discord {
    fn activate(&mut self) -> Result<(), String> {
        self.link = Link::new(link::names(&api::platform().os));
        self.link.pid = Some(api::process_id());
        self.settings();
        if self.client_id.is_empty() {
            api::info!("{NO_ID}");
            api::notify(NO_ID);
        }
        self.connect();
        self.update();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        if self.client_id.is_empty() {
            api::notify(NO_ID);
            return Ok(());
        }
        match id {
            "discord.toggle" => {
                self.off = !self.off;
                if self.off {
                    // Nothing shown, then disconnected: Discord clears it as it goes.
                    self.update();
                    self.link.stop(&mut Fenix);
                    self.idle_check();
                    api::notify("Discord: not showing your activity for the rest of this session");
                } else {
                    self.connect();
                    self.update();
                    api::notify("Discord: showing your activity again");
                }
            }
            "discord.reconnect" => {
                if self.off {
                    api::notify("Discord: turned off for this session -- run Discord: toggle to show it again");
                } else {
                    self.link.reconnect(&mut Fenix);
                    self.update();
                }
            }
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timer(id) if Some(id) == self.idle_timer && self.editor.check_idle(now(), self.idle) => self.update(),
            Event::Timer(id) => {
                self.link.timer(&mut Fenix, id);
            }
            Event::SocketConnected((socket, result)) => self.link.connected(&mut Fenix, socket, result),
            Event::SocketData((socket, data)) => {
                self.link.data(&mut Fenix, socket, &data);
            }
            Event::SocketClosed((socket, why)) => {
                self.link.closed(&mut Fenix, socket, why);
            }
            Event::SettingChanged(_) => {
                if self.settings() {
                    if self.client_id.is_empty() {
                        api::notify(NO_ID);
                    }
                    self.connect();
                }
                self.idle_check();
                self.update();
            }
            // Back from idle.
            Event::Typing | Event::Active if self.editor.typed(now()) => self.update(),
            Event::TaskEnded(_) if self.editor.task_ended() => self.update(),
            _ => {}
        }
    }

    fn task_started(&mut self, name: &str) {
        if self.editor.task_started(name, now()) {
            self.update();
        }
    }

    fn debug_changed(&mut self, state: DebugState) {
        let what = match state {
            DebugState::Started(s) => Debug::Started { id: s.id, name: s.name, language: s.language, parent: s.parent },
            DebugState::Paused((id, why)) => Debug::Paused(id, why),
            DebugState::Resumed(id) => Debug::Resumed(id),
            DebugState::Ended(id) => Debug::Ended(id),
        };
        if self.editor.debug(what, now()) {
            self.update();
        }
    }

    fn focus_moved(&mut self, focus: Focus) {
        // A page, a terminal, the explorer: what you were editing stays.
        if let Focus::File(Some(path)) = focus {
            self.moved(&path);
        }
    }
}

export!(Discord);
