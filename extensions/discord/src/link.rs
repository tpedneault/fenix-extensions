//! The connection to Discord's desktop app: found by its local socket,
//! shaken hands with, sent the activity -- no more often than Discord
//! takes it -- and found again, backing off, when it isn't there or goes.
//! Sans Fenix: what it asks for goes through [`Host`].

use serde_json::{json, Value};

use crate::frame::{self, Frame, Reader};

/// The first wait before looking again, doubled each time it isn't
/// there, up to [`RETRY_MAX`].
pub const RETRY_FIRST: u32 = 5_000;
pub const RETRY_MAX: u32 = 60_000;
/// How long Discord has to answer the handshake.
pub const HANDSHAKE_WAIT: u32 = 10_000;
/// The least time between two activities sent: Discord takes five in
/// 20 seconds.
pub const GAP: u64 = 4_000;

/// What the link asks of Fenix. The extension's is the real host; the
/// tests' is a fake.
pub trait Host {
    /// A local socket by name (`permissions.ipc`).
    fn ipc(&mut self, name: &str) -> Result<u64, String>;
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String>;
    fn close(&mut self, socket: u64);
    /// A one-shot timer.
    fn timer(&mut self, millis: u32) -> u64;
    fn cancel_timer(&mut self, timer: u64);
    /// Now, in ms since 1970.
    fn now(&mut self) -> u64;
    /// For its output page.
    fn log(&mut self, message: &str);
    /// For the message line.
    fn notify(&mut self, message: &str);
}

/// The names Discord's socket may have, in the order they're tried:
/// `discord-ipc-0` to `-9`, and on Linux in Flatpak's and Snap's folders
/// as well.
pub fn names(os: &str) -> Vec<String> {
    let folders: &[&str] = if os == "linux" { &["", "app/com.discordapp.Discord/", "snap.discord/"] } else { &[""] };
    (0..10).flat_map(|n| folders.iter().map(move |f| format!("{f}discord-ipc-{n}"))).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// No application ID, or turned off: nothing's tried.
    Off,
    /// Discord said no to the application ID; tried again when it
    /// changes, or on `discord.reconnect`.
    Refused,
    /// Discord wasn't there: looked for again when `timer` goes off.
    Waiting { timer: u64 },
    /// Connected, the handshake sent; given up on when `timer` goes off.
    Handshake { socket: u64, timer: u64 },
    Ready { socket: u64 },
}

#[derive(Debug)]
pub struct Link {
    pub state: State,
    client_id: String,
    names: Vec<String>,
    /// Waits that found nothing, in a row.
    misses: u32,
    reader: Reader,
    /// The activity to show (none: nothing), what Discord shows (nothing,
    /// after the handshake), and when that was sent.
    want: Option<Value>,
    sent: Option<Value>,
    sent_at: Option<u64>,
    /// A send held back by [`GAP`].
    flush: Option<u64>,
    nonce: u64,
    /// `discord.reconnect` asked: say how it went.
    asked: bool,
    /// Fenix's process id, which `SET_ACTIVITY` takes: Discord clears the
    /// activity once it's gone.
    pub pid: Option<u32>,
}

impl Link {
    pub fn new(names: Vec<String>) -> Link {
        Link { state: State::Off, client_id: String::new(), names, misses: 0, reader: Reader::default(), want: None, sent: None, sent_at: None, flush: None, nonce: 0, asked: false, pid: None }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, State::Ready { .. })
    }

    /// Connects as application `client_id` -- again if it was already,
    /// or with none, not at all.
    pub fn start(&mut self, host: &mut dyn Host, client_id: &str) {
        self.stop(host);
        self.client_id = client_id.trim().to_string();
        self.misses = 0;
        if !self.client_id.is_empty() {
            self.connect(host);
        }
    }

    /// Looks again now, and says what came of it.
    pub fn reconnect(&mut self, host: &mut dyn Host) {
        let id = self.client_id.clone();
        self.start(host, &id);
        self.asked = !id.is_empty();
        if self.is_ready() || matches!(self.state, State::Handshake { .. }) {
            return;
        }
        self.tell_missing(host);
    }

    /// Disconnected, and nothing more tried.
    pub fn stop(&mut self, host: &mut dyn Host) {
        match std::mem::replace(&mut self.state, State::Off) {
            State::Waiting { timer } => host.cancel_timer(timer),
            State::Handshake { socket, timer } => {
                host.cancel_timer(timer);
                host.close(socket);
            }
            State::Ready { socket } => {
                let _ = host.write(socket, &frame::encode(frame::CLOSE, &json!({})));
                host.close(socket);
            }
            State::Off | State::Refused => {}
        }
        if let Some(timer) = self.flush.take() {
            host.cancel_timer(timer);
        }
        self.reader.clear();
        self.sent = None;
        self.sent_at = None;
    }

    /// The activity to show: sent now, or once [`GAP`] has passed since
    /// the last.
    pub fn set(&mut self, host: &mut dyn Host, activity: Option<Value>) {
        self.want = activity;
        self.send(host);
    }

    fn send(&mut self, host: &mut dyn Host) {
        let State::Ready { socket } = self.state else { return };
        if self.sent == self.want || self.flush.is_some() {
            return;
        }
        let now = host.now();
        if let Some(at) = self.sent_at.filter(|at| now < at + GAP) {
            self.flush = Some(host.timer((at + GAP - now) as u32));
            return;
        }
        self.nonce += 1;
        let mut args = json!({});
        if let Some(pid) = self.pid {
            args["pid"] = pid.into();
        }
        if let Some(activity) = &self.want {
            args["activity"] = activity.clone();
        }
        let command = json!({ "cmd": "SET_ACTIVITY", "args": args, "nonce": self.nonce.to_string() });
        match host.write(socket, &frame::encode(frame::FRAME, &command)) {
            Ok(()) => {
                self.sent = self.want.clone();
                self.sent_at = Some(now);
            }
            Err(why) => self.lost(host, socket, Some(why)),
        }
    }

    /// A timer went off: whether it was one of the link's.
    pub fn timer(&mut self, host: &mut dyn Host, id: u64) -> bool {
        if self.flush == Some(id) {
            self.flush = None;
            self.send(host);
            return true;
        }
        match self.state {
            State::Waiting { timer } if timer == id => self.connect(host),
            State::Handshake { socket, timer } if timer == id => {
                host.log("Discord didn't answer the handshake");
                self.lost(host, socket, None);
            }
            _ => return false,
        }
        true
    }

    /// Tries each name until one's there, and shakes hands; else waits,
    /// a little longer each time.
    fn connect(&mut self, host: &mut dyn Host) {
        for name in self.names.clone() {
            let Ok(socket) = host.ipc(&name) else { continue };
            let hello = json!({ "v": 1, "client_id": self.client_id });
            if host.write(socket, &frame::encode(frame::HANDSHAKE, &hello)).is_err() {
                host.close(socket);
                continue;
            }
            host.log(&format!("connected to {name}"));
            self.reader.clear();
            self.state = State::Handshake { socket, timer: host.timer(HANDSHAKE_WAIT) };
            return;
        }
        self.wait(host);
    }

    fn wait(&mut self, host: &mut dyn Host) {
        let millis = RETRY_FIRST.saturating_mul(1 << self.misses.min(8)).min(RETRY_MAX);
        self.misses += 1;
        self.state = State::Waiting { timer: host.timer(millis) };
    }

    fn tell_missing(&mut self, host: &mut dyn Host) {
        if std::mem::take(&mut self.asked) {
            host.notify("Discord isn't running -- it's looked for again every minute or so");
        }
    }

    /// The socket's gone: looked for again soon.
    fn lost(&mut self, host: &mut dyn Host, socket: u64, why: Option<String>) {
        if let State::Handshake { timer, .. } = self.state {
            host.cancel_timer(timer);
        }
        if let Some(timer) = self.flush.take() {
            host.cancel_timer(timer);
        }
        host.close(socket);
        if let Some(why) = why {
            host.log(&format!("the connection to Discord closed: {why}"));
        }
        self.reader.clear();
        self.sent = None;
        self.sent_at = None;
        // Straight after a connection that worked, the first wait is short.
        if self.is_ready() {
            self.misses = 0;
        }
        self.wait(host);
    }

    fn socket(&self) -> Option<u64> {
        match self.state {
            State::Handshake { socket, .. } | State::Ready { socket } => Some(socket),
            _ => None,
        }
    }

    /// Its socket connected, or couldn't.
    pub fn connected(&mut self, host: &mut dyn Host, socket: u64, result: Result<(), String>) {
        if let (Some(ours), Err(why)) = (self.socket(), result) {
            if ours == socket {
                self.lost(host, socket, Some(why));
            }
        }
    }

    /// What came on a socket: whether it was the link's.
    pub fn data(&mut self, host: &mut dyn Host, socket: u64, data: &[u8]) -> bool {
        if self.socket() != Some(socket) {
            return false;
        }
        self.reader.push(data);
        while let Some(frame) = self.reader.take() {
            match frame {
                Ok(frame) => self.frame(host, socket, frame),
                Err(why) => self.lost(host, socket, Some(why)),
            }
            if self.socket() != Some(socket) {
                break;
            }
        }
        true
    }

    /// A socket closed: whether it was the link's.
    pub fn closed(&mut self, host: &mut dyn Host, socket: u64, why: Option<String>) -> bool {
        if self.socket() != Some(socket) {
            return false;
        }
        self.lost(host, socket, Some(why.unwrap_or_else(|| "by Discord".into())));
        true
    }

    fn frame(&mut self, host: &mut dyn Host, socket: u64, frame: Frame) {
        let field = |key: &str| frame.payload.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match frame.op {
            frame::FRAME if field("evt") == "READY" => {
                if let State::Handshake { timer, .. } = self.state {
                    host.cancel_timer(timer);
                }
                self.state = State::Ready { socket };
                self.misses = 0;
                host.log("Discord is ready");
                if std::mem::take(&mut self.asked) {
                    host.notify("Connected to Discord");
                }
                self.send(host);
            }
            frame::FRAME if field("evt") == "ERROR" => {
                let data = frame.payload.get("data").cloned().unwrap_or_default();
                host.log(&format!("Discord said no to {}: {data}", field("cmd")));
            }
            frame::FRAME => {}
            frame::PING => {
                let _ = host.write(socket, &frame::encode(frame::PONG, &frame.payload));
            }
            frame::CLOSE => {
                let code = frame.payload.get("code").and_then(Value::as_u64);
                let message = field("message");
                host.log(&format!("Discord closed the connection: {} {message}", code.map(|c| c.to_string()).unwrap_or_default()));
                // 4000: the application ID isn't one.
                if code == Some(4000) || message.to_ascii_lowercase().contains("client id") {
                    self.lost(host, socket, None);
                    if let State::Waiting { timer } = self.state {
                        host.cancel_timer(timer);
                    }
                    self.state = State::Refused;
                    host.notify(&format!("Discord doesn't know application {} -- check discord.client_id", self.client_id));
                } else {
                    self.lost(host, socket, None);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// Fenix as the tests have it: sockets that are there or not, what's
    /// written to each, timers and the time.
    #[derive(Default)]
    struct Fake {
        /// The socket names there are now.
        there: HashSet<String>,
        next: u64,
        tried: Vec<String>,
        written: HashMap<u64, Vec<Frame>>,
        closed: Vec<u64>,
        timers: HashMap<u64, u32>,
        now: u64,
        logged: Vec<String>,
        notified: Vec<String>,
    }

    impl Fake {
        fn id(&mut self) -> u64 {
            self.next += 1;
            self.next
        }

        /// The timer that's set: its id and how long.
        fn timer_set(&self) -> (u64, u32) {
            assert_eq!(self.timers.len(), 1, "{:?}", self.timers);
            self.timers.iter().map(|(id, ms)| (*id, *ms)).next().unwrap()
        }

        fn sent(&self, socket: u64) -> Vec<Frame> {
            self.written.get(&socket).cloned().unwrap_or_default()
        }
    }

    impl Host for Fake {
        fn ipc(&mut self, name: &str) -> Result<u64, String> {
            self.tried.push(name.into());
            if self.there.contains(name) {
                Ok(self.id())
            } else {
                Err(format!("{name}: no such socket"))
            }
        }
        fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
            let mut reader = Reader::default();
            reader.push(data);
            let frame = reader.take().unwrap().unwrap();
            self.written.entry(socket).or_default().push(frame);
            Ok(())
        }
        fn close(&mut self, socket: u64) {
            self.closed.push(socket);
        }
        fn timer(&mut self, millis: u32) -> u64 {
            let id = self.id();
            self.timers.insert(id, millis);
            id
        }
        fn cancel_timer(&mut self, timer: u64) {
            self.timers.remove(&timer);
        }
        fn now(&mut self) -> u64 {
            self.now
        }
        fn log(&mut self, message: &str) {
            self.logged.push(message.into());
        }
        fn notify(&mut self, message: &str) {
            self.notified.push(message.into());
        }
    }

    /// The timer `id` goes off.
    fn fire(link: &mut Link, host: &mut Fake, id: u64) {
        host.now += u64::from(host.timers.remove(&id).unwrap());
        assert!(link.timer(host, id));
    }

    fn ready(link: &mut Link, host: &mut Fake, socket: u64) {
        let frame = frame::encode(frame::FRAME, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1 } }));
        assert!(link.data(host, socket, &frame));
    }

    fn up(host: &mut Fake) -> (Link, u64) {
        host.there.insert("discord-ipc-0".into());
        let mut link = Link::new(names("linux"));
        link.start(host, "123");
        let State::Handshake { socket, .. } = link.state else { panic!("{:?}", link.state) };
        ready(&mut link, host, socket);
        (link, socket)
    }

    #[test]
    fn the_names_on_each_system() {
        let linux = names("linux");
        assert_eq!(linux.len(), 30);
        assert_eq!(&linux[..3], ["discord-ipc-0", "app/com.discordapp.Discord/discord-ipc-0", "snap.discord/discord-ipc-0"]);
        assert_eq!(names("windows"), (0..10).map(|n| format!("discord-ipc-{n}")).collect::<Vec<_>>());
    }

    #[test]
    fn no_application_id_tries_nothing() {
        let mut host = Fake::default();
        let mut link = Link::new(names("linux"));
        link.start(&mut host, "  ");
        assert_eq!(link.state, State::Off);
        assert!(host.tried.is_empty() && host.timers.is_empty());
    }

    #[test]
    fn it_shakes_hands_with_the_first_socket_there() {
        let mut host = Fake::default();
        host.there.insert("snap.discord/discord-ipc-1".into());
        let mut link = Link::new(names("linux"));
        link.start(&mut host, "123");
        assert_eq!(host.tried.last().unwrap(), "snap.discord/discord-ipc-1");
        assert_eq!(host.tried.len(), 6);
        let State::Handshake { socket, timer } = link.state else { panic!() };
        assert_eq!(host.timers[&timer], HANDSHAKE_WAIT);
        assert_eq!(host.sent(socket), [Frame { op: frame::HANDSHAKE, payload: json!({ "v": 1, "client_id": "123" }) }]);
        // Nothing is sent until Discord's ready.
        link.set(&mut host, Some(json!({ "details": "a" })));
        assert_eq!(host.sent(socket).len(), 1);
        ready(&mut link, &mut host, socket);
        assert!(link.is_ready());
        assert!(host.timers.is_empty(), "the handshake's wait is cancelled");
        let set = &host.sent(socket)[1];
        assert_eq!(set.op, frame::FRAME);
        assert_eq!(set.payload["cmd"], "SET_ACTIVITY");
        assert_eq!(set.payload["args"]["activity"], json!({ "details": "a" }));
        assert_eq!(set.payload["nonce"], "1");
        assert!(set.payload["args"].get("pid").is_none(), "none known");
    }

    #[test]
    fn fenixs_process_id_goes_with_each_activity() {
        let mut host = Fake::default();
        let (mut link, socket) = up(&mut host);
        link.pid = Some(4242);
        link.set(&mut host, Some(json!({ "details": "a" })));
        host.now += GAP;
        link.set(&mut host, None);
        let sent = host.sent(socket);
        let sets: Vec<&Frame> = sent.iter().filter(|f| f.payload["cmd"] == "SET_ACTIVITY").collect();
        assert_eq!(sets.len(), 2);
        assert!(sets.iter().all(|f| f.payload["args"]["pid"] == 4242), "{sets:?}");
        assert!(sets[1].payload["args"].get("activity").is_none(), "cleared");
    }

    #[test]
    fn discord_started_after_fenix_is_found_backing_off() {
        let mut host = Fake::default();
        let mut link = Link::new(names("windows"));
        link.start(&mut host, "123");
        let mut waits = Vec::new();
        for _ in 0..6 {
            let (timer, millis) = host.timer_set();
            waits.push(millis);
            fire(&mut link, &mut host, timer);
        }
        assert_eq!(waits, [5_000, 10_000, 20_000, 40_000, 60_000, 60_000]);
        assert_eq!(host.tried.len(), 70);
        // Discord starts.
        host.there.insert("discord-ipc-0".into());
        let (timer, _) = host.timer_set();
        fire(&mut link, &mut host, timer);
        let State::Handshake { socket, .. } = link.state else { panic!("{:?}", link.state) };
        ready(&mut link, &mut host, socket);
        // Nothing to show is nothing sent; nothing after something clears it.
        link.set(&mut host, None);
        assert_eq!(host.sent(socket).len(), 1);
        link.set(&mut host, Some(json!({ "details": "a" })));
        host.now += GAP;
        link.set(&mut host, None);
        assert_eq!(host.sent(socket).last().unwrap().payload["args"], json!({}));
    }

    #[test]
    fn discord_quitting_is_noticed_and_it_comes_back() {
        let mut host = Fake::default();
        let (mut link, socket) = up(&mut host);
        link.set(&mut host, Some(json!({ "details": "a" })));
        host.there.clear();
        assert!(link.closed(&mut host, socket, None));
        assert!(host.closed.contains(&socket));
        let (timer, millis) = host.timer_set();
        assert_eq!(millis, RETRY_FIRST);
        fire(&mut link, &mut host, timer);
        assert_eq!(host.timer_set().1, 10_000);
        // Back: the activity it had is sent again, at once.
        host.there.insert("discord-ipc-0".into());
        let (timer, _) = host.timer_set();
        fire(&mut link, &mut host, timer);
        let State::Handshake { socket, .. } = link.state else { panic!() };
        ready(&mut link, &mut host, socket);
        assert_eq!(host.sent(socket).last().unwrap().payload["args"]["activity"], json!({ "details": "a" }));
        // Another's socket isn't its.
        assert!(!link.closed(&mut host, 999, None));
        assert!(!link.data(&mut host, 999, b"x"));
    }

    #[test]
    fn a_handshake_never_answered_is_given_up() {
        let mut host = Fake::default();
        host.there.insert("discord-ipc-0".into());
        let mut link = Link::new(names("macos"));
        link.start(&mut host, "123");
        let State::Handshake { socket, timer } = link.state else { panic!() };
        fire(&mut link, &mut host, timer);
        assert!(host.closed.contains(&socket));
        assert!(matches!(link.state, State::Waiting { .. }));
    }

    #[test]
    fn activities_are_held_to_discords_rate() {
        let mut host = Fake::default();
        let (mut link, socket) = up(&mut host);
        link.set(&mut host, Some(json!({ "details": "a" })));
        // The same again isn't sent.
        link.set(&mut host, Some(json!({ "details": "a" })));
        host.now += 1_000;
        link.set(&mut host, Some(json!({ "details": "b" })));
        link.set(&mut host, Some(json!({ "details": "c" })));
        assert_eq!(host.sent(socket).len(), 2);
        let (timer, millis) = host.timer_set();
        assert_eq!(millis, 3_000);
        fire(&mut link, &mut host, timer);
        // Only the last.
        let sent = host.sent(socket);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[2].payload["args"]["activity"], json!({ "details": "c" }));
        assert_eq!(sent[2].payload["nonce"], "2");
    }

    #[test]
    fn pings_are_answered() {
        let mut host = Fake::default();
        let (mut link, socket) = up(&mut host);
        link.data(&mut host, socket, &frame::encode(frame::PING, &json!({ "n": 7 })));
        assert_eq!(host.sent(socket).last().unwrap(), &Frame { op: frame::PONG, payload: json!({ "n": 7 }) });
    }

    #[test]
    fn an_unknown_application_id_stops_it_till_it_changes() {
        let mut host = Fake::default();
        host.there.insert("discord-ipc-0".into());
        let mut link = Link::new(names("linux"));
        link.start(&mut host, "nope");
        let State::Handshake { socket, .. } = link.state else { panic!() };
        link.data(&mut host, socket, &frame::encode(frame::CLOSE, &json!({ "code": 4000, "message": "Invalid Client ID" })));
        assert_eq!(link.state, State::Refused);
        assert!(host.timers.is_empty());
        assert!(host.notified[0].contains("discord.client_id"));
        link.start(&mut host, "123");
        assert!(matches!(link.state, State::Handshake { .. }));
    }

    #[test]
    fn a_broken_stream_reconnects() {
        let mut host = Fake::default();
        let (mut link, socket) = up(&mut host);
        link.data(&mut host, socket, &[1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        assert!(host.closed.contains(&socket));
        assert_eq!(host.timer_set().1, RETRY_FIRST);
    }

    #[test]
    fn reconnect_says_how_it_went() {
        let mut host = Fake::default();
        let mut link = Link::new(names("windows"));
        link.start(&mut host, "123");
        link.reconnect(&mut host);
        assert!(host.notified[0].contains("isn't running"));
        host.there.insert("discord-ipc-3".into());
        link.reconnect(&mut host);
        let State::Handshake { socket, .. } = link.state else { panic!() };
        ready(&mut link, &mut host, socket);
        assert_eq!(host.notified[1], "Connected to Discord");
        // Stopped: closed, and its timers gone.
        link.stop(&mut host);
        assert_eq!(link.state, State::Off);
        assert!(host.closed.contains(&socket) && host.timers.is_empty());
        assert_eq!(host.sent(socket).last().unwrap().op, frame::CLOSE);
    }
}
