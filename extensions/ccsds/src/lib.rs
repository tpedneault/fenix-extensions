//! CCSDS and ECSS in Fenix, as Fenix 1.x had them built in: packets,
//! frames and CLTUs decoded into a tree (`SPC k d`), recordings and live
//! telemetry as a list of packets with a parameter followed through them
//! (`SPC k f`, `SPC k l`), on-board times converted (`SPC k T`), a file
//! as hex (`SPC f x`), the standards library (`SPC k ?`), `K` on hex or
//! an on-board time anywhere, and the mission clock in the modeline.
//!
//! The mission's profile is its `ccsds.*` settings, the project's over
//! yours. Packets are named through the MIB: the same MIBs the MIB
//! extension reads (`mib.roots`, `permissions.settings = ["mib"]`), read
//! here with the shared crate. Live sources come over its sockets, held
//! to the addresses `ccsds.sources` lists; it only receives.

mod decode;
mod hex_page;
mod packet_page;
mod page;
mod standards_page;
mod stream;
mod stream_page;
mod time_page;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fenix_ccsds::stream::{Framing, Splitter};
use fenix_extension_api::lsp::types::{Hover, HoverContents, HoverParams, HoverProviderCapability, MarkupContent, MarkupKind, ServerCapabilities};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{self as api, export, host, socket, Event, Extension, Key, Page, PickerItem};
use fenix_mib::mission::{map_entries, Mission};
use fenix_mib::{DefRef, Kind, MibRoot, MibSet};

use hex_page::HexPage;
use packet_page::PacketPage;
use standards_page::StandardsPage;
use stream::LiveSplitter;
use stream_page::StreamPage;
use time_page::TimePage;

/// The kinds of page it opens.
const HEX_KIND: &str = "ccsds-hex";
const PACKET_KIND: &str = "ccsds-packet";
const STREAM_KIND: &str = "ccsds-stream";
const TIME_KIND: &str = "ccsds-time";
const STANDARDS_KIND: &str = "ccsds-standards";

const PROJECT: &str = "/project";
/// Where the standards folder (`ccsds.library`) is.
const LIBRARY: &str = "/folders/standards";
/// Where the MIB folders `mib.roots` names are.
const MIBS: &str = "/folders/mib";

/// A recording is read a chunk at a time, this often, so a big one
/// doesn't hold the extension up.
const READ_MILLIS: u32 = 15;
const CHUNK: usize = 1024 * 1024;
/// A file source is looked at for what's new this often.
const TAIL_MILLIS: u32 = 250;
/// A live source that dropped is tried again after this.
const RETRY_MILLIS: u32 = 2_000;
/// The mission clock's tick.
const CLOCK_MILLIS: u32 = 250;

#[derive(Default)]
pub struct CcsdsExtension {
    servers: Servers,
    ccsds: Ccsds,
}

/// One of its pages.
enum Open {
    Hex(Box<HexPage>),
    Packet(Box<PacketPage>),
    Stream(Box<StreamPage>, From),
    Time(Box<TimePage>, Origin),
    Standards(Box<StandardsPage>),
}

/// Where a stream page's packets come from.
#[derive(Debug, Clone)]
enum From {
    /// A recording: where it is for Fenix, and how it's framed (`None`,
    /// guessed).
    File(String, Option<Framing>),
    Live(Source),
}

/// A live source's address and how to read it (`ccsds.sources`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    name: String,
    address: String,
    subject: String,
    framing: String,
    /// With `records` framing, the record header's size in octets.
    header: usize,
}

/// Where the time converter's `i` inserts: the file and line it was
/// opened from.
#[derive(Debug, Clone)]
struct Origin {
    path: Option<String>,
    line: u32,
}

/// A stream page's reader.
enum Reader {
    /// A recording, read a chunk at a time.
    Recording { file: Box<dyn Read>, splitter: Splitter, framing: Framing, total: usize },
    Tcp { socket: u64, splitter: LiveSplitter },
    Udp { socket: u64, splitter: LiveSplitter },
    Nats { socket: u64, splitter: LiveSplitter, inbox: socket::Inbox, subject: String, payload: Option<(usize, usize)> },
    /// A file still being written, read from where it was.
    Tail { path: String, at: u64, splitter: LiveSplitter },
}

/// What a picker it opened is for.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// A live source, by its place in `ccsds.sources`.
    Source,
    /// A TM parameter to follow on stream page `page`.
    Parameter(u64),
}

/// What a timer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tick {
    Read,
    Tail(u64),
    Retry(u64),
    Clock,
}

#[derive(Default)]
pub(crate) struct Ccsds {
    /// The MIBs, read, and what they were read from.
    set: Option<(MibKey, Arc<MibSet>)>,
    pages: HashMap<u64, Open>,
    /// Each stream page's reader, by page.
    readers: HashMap<u64, Reader>,
    /// Each socket's page.
    sockets: HashMap<u64, u64>,
    timers: HashMap<u64, Tick>,
    /// The recordings timer, while one reads.
    reading: Option<u64>,
    clock: Option<u64>,
    asked: HashMap<u64, Asked>,
    /// Each standards search under way: its page, standard and search.
    searches: HashMap<u64, (u64, usize, u64)>,
    /// Each standards page's latest search: answers to one replaced are
    /// dropped.
    searched: HashMap<u64, u64>,
    next_search: u64,
    /// Services it called: what to say when there's none.
    calls: HashMap<u64, &'static str>,
}

/// What a set of MIBs was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MibKey {
    roots: Vec<MibRoot>,
    default: Option<String>,
    options: fenix_mib::ReadOptions,
}

// -- Settings -----------------------------------------------------------------

fn text(key: &str) -> Option<String> {
    api::setting(key).filter(|t| !t.trim().is_empty())
}

fn on(key: &str, default: bool) -> bool {
    match text(key).as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("true" | "on" | "yes" | "1") => true,
        Some("false" | "off" | "no" | "0") => false,
        _ => default,
    }
}

/// The mission profile: the CCSDS settings, the project's over yours.
fn mission() -> Mission {
    Mission::read(api::setting, None)
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

/// The MIBs here, as the MIB extension reads them: those `mib.roots`
/// names, else the project when it's a MIB.
fn mib_key() -> MibKey {
    use fenix_mib::{schema, Icd, Schema};
    let map = |key: &str| text(key).map(|t| map_entries(&t)).unwrap_or_default();
    let mut roots: Vec<MibRoot> = map("mib.roots").into_iter().map(|(label, _)| MibRoot { path: PathBuf::from(format!("{MIBS}/{label}")), label }).filter(|r| r.path.is_dir()).collect();
    if roots.is_empty() {
        let tables = ["vdf.dat", "ccf.dat", "pcf.dat", "pid.dat", "tpcf.dat", "caf.dat", "txf.dat"];
        let root = Path::new(PROJECT);
        if let Some(dir) = [root.join("mib"), root.to_path_buf()].into_iter().find(|d| tables.iter().any(|t| d.join(t).is_file())) {
            roots.push(MibRoot { label: "project".into(), path: dir });
        }
    }
    let issue = |t: &str| if t.trim() == "auto" { None } else { Some(Icd::parse(t).unwrap_or(Icd::LATEST)) };
    let schema = Schema {
        icd: text("mib.icd").map_or(Some(Icd::LATEST), |t| issue(&t)),
        per_root: map("mib.icd_per_mib").into_iter().map(|(label, t)| (label, issue(&t))).collect(),
        columns: map("mib.columns").into_iter().map(|(t, c)| (t.trim().to_ascii_lowercase(), schema::parse_columns(&c))).collect(),
    };
    MibKey { roots, default: text("mib.default"), options: fenix_mib::ReadOptions { schema, ..Default::default() } }
}

/// The live sources the project (or you) lists.
fn sources() -> Vec<Source> {
    let rows: Vec<Vec<String>> = text("ccsds.sources").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    rows.into_iter()
        .filter(|r| r.len() >= 2 && !r[1].trim().is_empty())
        .map(|r| Source {
            name: r[0].clone(),
            address: r[1].trim().to_string(),
            subject: r.get(2).cloned().unwrap_or_default(),
            framing: r.get(3).cloned().unwrap_or_default(),
            header: r.get(4).and_then(|h| h.trim().parse().ok()).unwrap_or(0),
        })
        .collect()
}

// -- Paths ----------------------------------------------------------------------

/// The project's folder, for Fenix.
fn project_root() -> Option<String> {
    api::here().and_then(|h| h.project).map(|p| p.root)
}

/// Where `path` (Fenix's) is for it: under `/project` when it's in the
/// project.
fn guest_of(path: &str) -> Option<String> {
    guest_in(path, &project_root()?)
}

fn guest_in(path: &str, root: &str) -> Option<String> {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_string();
    let (path, root) = (norm(path), norm(root));
    if path.len() < root.len() || !path[..root.len()].eq_ignore_ascii_case(&root) {
        return None;
    }
    let rest = &path[root.len()..];
    (rest.is_empty() || rest.starts_with('/')).then(|| format!("{PROJECT}{rest}"))
}

/// A file's bytes, up to `limit`, and its whole size: through the
/// project when it's in it, else by the path Fenix gave it.
fn read_up_to(path: &str, limit: u64) -> Result<(Vec<u8>, u64), String> {
    if let Some(guest) = guest_of(path) {
        let total = std::fs::metadata(&guest).map(|m| m.len()).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        std::fs::File::open(&guest).and_then(|f| f.take(limit).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
        return Ok((bytes, total));
    }
    let mut bytes = api::read_file(path)?;
    let total = bytes.len() as u64;
    bytes.truncate(limit as usize);
    Ok((bytes, total))
}

/// The name a path ends in.
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// The file a command acts on: the explorer's selection, else the file
/// you're in.
fn file_here() -> Option<String> {
    api::explorer_selected().or_else(|| api::here().and_then(|h| h.path))
}

/// A standard's PDF, by Fenix's path: in the standards folder.
fn library_path(guest: &Path) -> Option<String> {
    let rest = guest.strip_prefix(LIBRARY).ok()?;
    let folder = api::setting("ccsds.library").filter(|f| !f.trim().is_empty())?;
    Some(Path::new(&folder).join(rest).to_string_lossy().into_owned())
}

/// The bytes here: what's selected, else the longest run of hex on the
/// cursor's line, else the clipboard -- and where they came from.
fn bytes_here() -> Option<(Vec<u8>, String)> {
    if let Some(here) = api::here() {
        let name = here.path.as_deref().map(file_name).unwrap_or_else(|| "buffer".into());
        let text = here.text.trim_end_matches(['\n', '\r']);
        if let Some(bytes) = fenix_ccsds::hex::parse(text) {
            return Some((bytes, format!("{name}:{}", here.line)));
        }
        if let Some(bytes) = longest_hex(text.lines().next().unwrap_or("")) {
            return Some((bytes, format!("{name}:{}", here.line)));
        }
    }
    api::pasted_text().and_then(|clip| fenix_ccsds::hex::parse(&clip)).map(|b| (b, "the clipboard".to_string()))
}

/// The longest run of hex on `line`: of words that are all hex (`send
/// 1BF2C007 0009` has two), or as `SPC k d` reads one at a column.
fn longest_hex(line: &str) -> Option<Vec<u8>> {
    let mut best: Vec<u8> = Vec::new();
    let mut run = String::new();
    for w in line.split_whitespace().map(decode::hex_word).chain([None]) {
        match w {
            Some(w) => run.push_str(&w),
            None => {
                if let Some(bytes) = fenix_ccsds::hex::parse(&run).filter(|b| b.len() > best.len()) {
                    best = bytes;
                }
                run.clear();
            }
        }
    }
    let at = (0..line.chars().count()).filter_map(|col| fenix_ccsds::hex::around(line, col).map(|(_, b)| b)).max_by_key(Vec::len);
    let best = match at {
        Some(b) if b.len() > best.len() => b,
        _ => best,
    };
    (best.len() >= 4).then_some(best)
}

// -- The MIBs ------------------------------------------------------------------

impl Ccsds {
    /// The MIBs here, read now if they weren't.
    fn set(&mut self) -> Option<Arc<MibSet>> {
        let key = mib_key();
        if key.roots.is_empty() {
            return None;
        }
        match &self.set {
            Some((k, set)) if *k == key => Some(set.clone()),
            _ => {
                let set = Arc::new(MibSet::load_with(key.roots.clone(), key.default.as_deref(), key.options.clone()));
                self.set = Some((key, set.clone()));
                Some(set)
            }
        }
    }

    fn timer(&mut self, millis: u32, repeat: bool, tick: Tick) -> u64 {
        let id = host::set_timer(millis, repeat);
        self.timers.insert(id, tick);
        id
    }

    fn open_page(&mut self, kind: &str, title: &str, page: Open) -> u64 {
        let id = api::page::open(kind, title);
        self.pages.insert(id, page);
        id
    }

    // -- Commands -------------------------------------------------------------

    fn command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "ccsds.time" => self.cmd_time(),
            "ccsds.hex" => match file_here() {
                Some(path) => self.open_hex(&path, None),
                None => error("no file here to show as hex"),
            },
            "ccsds.recording" => match file_here() {
                Some(path) => self.open_recording(&path, None),
                None => error("select a recording in the explorer (SPC f j), then SPC k f"),
            },
            "ccsds.live" => self.cmd_live(),
            "ccsds.standards" => self.cmd_standards(),
            "ccsds.decode" => match bytes_here() {
                Some((bytes, source)) => self.open_packet(bytes, source, None),
                None => error("no hex under the cursor, selected or on the clipboard"),
            },
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    /// `SPC k T`: the time converter, on the time on the cursor's line
    /// when there's one in the mission's format, else now.
    fn cmd_time(&mut self) {
        let m = mission();
        let p = m.profile.clone();
        let mut tai = fenix_ccsds::time::utc_to_tai(chrono::Utc::now().naive_utc(), &p.leap);
        let here = api::here();
        if let Some(line) = here.as_ref().map(|h| h.text.lines().next().unwrap_or("").to_string()) {
            let decoded = (0..line.chars().count()).filter_map(|col| fenix_ccsds::hex::around(&line, col)).find_map(|(_, b)| fenix_ccsds::time::decode(&b, p.tm_time, p.epoch, &p.leap, 0));
            if let Some(d) = decoded {
                tai = d.tai;
            }
        }
        let mut page = TimePage::new(p, tai);
        page.nominal = m.nominal_epoch;
        let origin = Origin { path: here.as_ref().and_then(|h| h.path.clone()), line: here.map(|h| h.line).unwrap_or(1) };
        self.open_page(TIME_KIND, "*time*", Open::Time(Box::new(page), origin));
    }

    /// `SPC f x`: a file as hex.
    fn open_hex(&mut self, path: &str, at: Option<usize>) {
        match read_up_to(path, hex_page::LIMIT) {
            Ok((bytes, total)) => {
                let over = (total > hex_page::LIMIT).then_some(total);
                let mut page = HexPage::new(PathBuf::from(path), bytes, over);
                if let Some(at) = at {
                    page.goto(at);
                }
                let title = hex_page::title(&page);
                self.open_page(HEX_KIND, &title, Open::Hex(Box::new(page)));
            }
            Err(e) => error(&format!("couldn't read {}: {e}", file_name(path))),
        }
    }

    /// The inspector on `bytes`, reading a CADU or CLTU with `profile`
    /// when given (how the source that received it read it).
    fn open_packet(&mut self, bytes: Vec<u8>, source: String, profile: Option<fenix_ccsds::frames::FrameProfile>) {
        let mut page = PacketPage::new(bytes, source);
        page.profile = profile;
        self.decode_into(&mut page);
        let title = packet_page::title(&page);
        self.open_page(PACKET_KIND, &title, Open::Packet(Box::new(page)));
    }

    /// Decodes the page's bytes as it reads them now.
    fn decode_into(&mut self, page: &mut PacketPage) {
        let set = self.set();
        let mut m = mission();
        match page.profile.clone() {
            Some(p) if p.kind == fenix_ccsds::frames::FrameKind::Tc => m.tc = p,
            Some(p) => m.frames = p,
            None => {}
        }
        let (root, _) = decode::decode_bytes(&page.bytes, page.reading, set.as_deref(), &m);
        page.root = root;
        if let Some(problem) = m.problems.first() {
            page.note = Some((format!("{problem} -- using the default"), true));
        }
    }

    /// `SPC k l`: a live source to follow.
    fn cmd_live(&mut self) {
        let sources = sources();
        if sources.is_empty() {
            return error("no live sources -- add one in the project's CCSDS settings (ccsds.sources)");
        }
        let items: Vec<PickerItem> = sources.iter().enumerate().map(|(i, s)| PickerItem::new(format!("{}  {}  {}", s.name, s.address, s.subject), i.to_string())).collect();
        let id = api::pick("Follow a live source", &items);
        self.asked.insert(id, Asked::Source);
    }

    /// `SPC k ?`: the standards library.
    fn cmd_standards(&mut self) {
        let folder = Path::new(LIBRARY).is_dir().then(|| PathBuf::from(LIBRARY));
        self.open_page(STANDARDS_KIND, "*standards*", Open::Standards(Box::new(StandardsPage::new(folder))));
    }

    // -- Stream pages ---------------------------------------------------------

    /// A recording, read off as it's shown.
    fn open_recording(&mut self, path: &str, framing: Option<Framing>) {
        let page = StreamPage::new(file_name(path), false);
        let id = self.open_page(STREAM_KIND, &stream_page::title(&page), Open::Stream(Box::new(page), From::File(path.to_string(), framing.clone())));
        self.start_recording(id, path, framing);
    }

    fn start_recording(&mut self, id: u64, path: &str, framing: Option<Framing>) {
        let m = mission();
        let (mut file, head): (Box<dyn Read>, Vec<u8>) = match guest_of(path) {
            Some(guest) => match std::fs::File::open(&guest) {
                Ok(mut f) => {
                    let mut head = vec![0u8; 256 * 1024];
                    let n = f.read(&mut head).unwrap_or(0);
                    head.truncate(n);
                    (Box::new(f), head)
                }
                Err(e) => return self.stream_failed(id, &format!("couldn't open: {e}")),
            },
            // Outside the project: as Fenix gives it, whole.
            None => match api::read_file(path) {
                Ok(bytes) => {
                    let head = bytes[..bytes.len().min(256 * 1024)].to_vec();
                    let rest = bytes[head.len()..].to_vec();
                    (Box::new(std::io::Cursor::new(rest)), head)
                }
                Err(e) => return self.stream_failed(id, &format!("couldn't open: {e}")),
            },
        };
        let set = self.set();
        if framing.is_none() && stream::looks_like_text(&head) {
            let mut text = head.clone();
            let _ = (&mut file).take(stream::TEXT_LIMIT).read_to_end(&mut text);
            if let Some(items) = fenix_ccsds::stream::hex_lines(&String::from_utf8_lossy(&text)) {
                self.stream_info(id, "hex lines · a text log with a packet in hex on a line".into());
                self.stream_rows(id, items, set.as_deref(), &m, None, true);
                return;
            }
        }
        let (framing, why) = match framing {
            Some(f) => (f, "as asked".to_string()),
            None => stream::guessed(&head, &m),
        };
        self.stream_info(id, format!("{} · {why}", framing.label()));
        let mut splitter = Splitter::new(framing.clone());
        let items = splitter.feed(&head);
        let total = items.iter().filter(|i| matches!(i, fenix_ccsds::stream::Item::Packet { .. })).count();
        self.stream_rows(id, items, set.as_deref(), &m, Some(&framing), false);
        self.readers.insert(id, Reader::Recording { file, splitter, framing, total });
        if self.reading.is_none() {
            self.reading = Some(self.timer(READ_MILLIS, true, Tick::Read));
        }
    }

    /// The recordings timer: a chunk more of each.
    fn read_recordings(&mut self) {
        let m = mission();
        let set = self.set();
        let ids: Vec<u64> = self.readers.iter().filter(|(_, r)| matches!(r, Reader::Recording { .. })).map(|(id, _)| *id).collect();
        for id in ids {
            let Some(Reader::Recording { file, splitter, framing, total }) = self.readers.get_mut(&id) else { continue };
            let mut chunk = vec![0u8; CHUNK];
            let (items, done, framing) = match file.read(&mut chunk) {
                Ok(0) | Err(_) => (splitter.finish(), true, framing.clone()),
                Ok(n) => (splitter.feed(&chunk[..n]), false, framing.clone()),
            };
            *total += items.iter().filter(|i| matches!(i, fenix_ccsds::stream::Item::Packet { .. })).count();
            let full = *total >= stream::MAX_ROWS;
            self.stream_rows(id, items, set.as_deref(), &m, Some(&framing), done || full);
            if full {
                self.stream_status(id, format!("stopped at {} packets", stream::MAX_ROWS), false);
            }
            if done || full {
                self.readers.remove(&id);
            }
        }
        if !self.readers.values().any(|r| matches!(r, Reader::Recording { .. })) {
            if let Some(t) = self.reading.take() {
                host::cancel_timer(t);
                self.timers.remove(&t);
            }
        }
    }

    /// A live source, followed in a page of its own.
    fn open_live(&mut self, src: Source) {
        let page = StreamPage::new(src.name.clone(), true);
        let id = self.open_page(STREAM_KIND, &stream_page::title(&page), Open::Stream(Box::new(page), From::Live(src.clone())));
        let m = mission();
        match stream::framing_of(&src.framing, src.header, &m) {
            Some(f) => self.stream_info(id, format!("{} · {}", f.label(), src.address)),
            None => self.stream_info(id, format!("guessing the framing from the first octets · {}", src.address)),
        }
        if src.framing.trim() == "records" && src.header == 0 {
            self.stream_status(id, "records framing without a record header size -- guessing instead".into(), false);
        }
        self.connect(id);
    }

    /// Stream page `id`'s live source, connected (again).
    fn connect(&mut self, id: u64) {
        let Some(Open::Stream(_, From::Live(src))) = self.pages.get(&id) else { return };
        let src = src.clone();
        let m = mission();
        let splitter = LiveSplitter::new(stream::framing_of(&src.framing, src.header, &m));
        let (scheme, rest) = src.address.split_once("://").unwrap_or(("tcp", src.address.as_str()));
        let host_port = |rest: &str| -> Result<(String, u16), String> {
            let (host, port) = rest.rsplit_once(':').ok_or_else(|| format!("{rest}: host:port"))?;
            Ok((if host.is_empty() { "127.0.0.1".into() } else { host.to_string() }, port.parse().map_err(|_| format!("{port} isn't a port"))?))
        };
        if scheme == "nats" && src.subject.trim().is_empty() {
            return self.retry(id, "a NATS source needs a subject");
        }
        let reader = match scheme {
            "tcp" | "nats" => host_port(rest).and_then(|(h, p)| socket::connect(&h, p)).map(|socket| {
                self.sockets.insert(socket, id);
                if scheme == "nats" {
                    Reader::Nats { socket, splitter, inbox: socket::Inbox::new(), subject: src.subject.trim().to_string(), payload: None }
                } else {
                    Reader::Tcp { socket, splitter }
                }
            }),
            "udp" => host_port(rest).and_then(|(_, p)| socket::bind_udp(p)).map(|socket| {
                self.sockets.insert(socket, id);
                Reader::Udp { socket, splitter }
            }),
            "file" => {
                let path = if Path::new(rest).is_absolute() || rest.contains(':') { guest_of(rest).unwrap_or_else(|| rest.to_string()) } else { format!("{PROJECT}/{}", rest.trim_start_matches("./")) };
                if std::fs::metadata(&path).is_err() {
                    Err(format!("{rest}: not in the project, or not there yet"))
                } else {
                    self.timer(TAIL_MILLIS, true, Tick::Tail(id));
                    Ok(Reader::Tail { path, at: 0, splitter })
                }
            }
            other => Err(format!("{other}:// -- tcp, udp, file or nats")),
        };
        match reader {
            Ok(reader) => {
                match &reader {
                    Reader::Udp { .. } => self.stream_status(id, format!("{} listening on {rest}", src.name), true),
                    Reader::Tail { path, .. } => {
                        let name = file_name(path);
                        self.stream_status(id, format!("{} following {name}", src.name), true);
                    }
                    _ => {}
                }
                self.readers.insert(id, reader);
            }
            Err(e) => self.retry(id, &e),
        }
    }

    /// Stream page `id`'s source dropped: said, and tried again soon.
    fn retry(&mut self, id: u64, why: &str) {
        if let Some(reader) = self.readers.remove(&id) {
            self.drop_reader(id, reader);
        }
        let Some(Open::Stream(_, From::Live(src))) = self.pages.get(&id) else { return };
        let name = src.name.clone();
        self.stream_status(id, format!("{name}: {why} -- trying again"), false);
        self.timer(RETRY_MILLIS, false, Tick::Retry(id));
    }

    /// Page `id`'s reader, let go: its socket closed, its timer stopped.
    fn drop_reader(&mut self, id: u64, reader: Reader) {
        if let Reader::Tcp { socket, .. } | Reader::Udp { socket, .. } | Reader::Nats { socket, .. } = reader {
            self.sockets.remove(&socket);
            socket::close(socket);
        }
        let tails: Vec<u64> = self.timers.iter().filter(|(_, t)| **t == Tick::Tail(id)).map(|(t, _)| *t).collect();
        for t in tails {
            host::cancel_timer(t);
            self.timers.remove(&t);
        }
    }

    /// Bytes that came on stream page `id`'s source.
    fn fed(&mut self, id: u64, bytes: &[u8]) {
        let m = mission();
        let set = self.set();
        let Some(reader) = self.readers.get_mut(&id) else { return };
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut pong = None;
        let splitter = match reader {
            Reader::Tcp { splitter, .. } | Reader::Udp { splitter, .. } | Reader::Tail { splitter, .. } => {
                chunks.push(bytes.to_vec());
                splitter
            }
            // NATS: its protocol's lines, and what MSG carries.
            Reader::Nats { socket, splitter, inbox, payload, .. } => {
                inbox.push(bytes);
                loop {
                    if let Some((total, header)) = *payload {
                        let Some(msg) = inbox.take(total + 2) else { break };
                        chunks.push(msg[header.min(total)..total].to_vec());
                        *payload = None;
                        continue;
                    }
                    let Some(line) = inbox.take_line() else { break };
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    match parts.first().copied().unwrap_or("") {
                        "PING" => pong = Some(*socket),
                        "MSG" | "HMSG" => {
                            let total: usize = parts.last().and_then(|n| n.parse().ok()).unwrap_or(0);
                            let header: usize = if parts[0] == "HMSG" { parts.get(parts.len() - 2).and_then(|n| n.parse().ok()).unwrap_or(0) } else { 0 };
                            *payload = Some((total, header));
                        }
                        _ => {}
                    }
                }
                splitter
            }
            Reader::Recording { .. } => return,
        };
        let mut items = Vec::new();
        let mut decided = None;
        for chunk in chunks {
            let (more, label) = splitter.feed(&chunk, &m);
            items.extend(more);
            decided = decided.or(label);
        }
        let framing = splitter.splitter.as_ref().map(Splitter::framing).cloned();
        if let Some(socket) = pong {
            let _ = socket::write(socket, b"PONG\r\n");
        }
        if let Some(label) = decided {
            let address = match self.pages.get(&id) {
                Some(Open::Stream(_, From::Live(src))) => src.address.clone(),
                _ => String::new(),
            };
            self.stream_info(id, format!("{label} · {address}"));
        }
        self.stream_rows(id, items, set.as_deref(), &m, framing.as_ref(), false);
    }

    /// A file source's timer: what's been written since.
    fn tail(&mut self, id: u64) {
        let Some(Reader::Tail { path, at, .. }) = self.readers.get_mut(&id) else { return };
        let len = std::fs::metadata(&*path).map(|m| m.len()).unwrap_or(0);
        if len < *at {
            // Cut short (rotated, rewritten): read again from its start.
            return self.retry(id, "the file got shorter -- reading it again");
        }
        if len == *at {
            return;
        }
        let mut bytes = Vec::new();
        let read = std::fs::File::open(&*path).and_then(|mut f| {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(*at))?;
            f.take(CHUNK as u64 * 4).read_to_end(&mut bytes)
        });
        match read {
            Ok(n) => {
                *at += n as u64;
                self.fed(id, &bytes);
            }
            Err(e) => self.retry(id, &e.to_string()),
        }
    }

    /// Rows for stream page `id`, out of `items`: through the MIB, with
    /// the followed parameter's new samples, a live source saying when
    /// it crosses a limit.
    fn stream_rows(&mut self, id: u64, items: Vec<fenix_ccsds::stream::Item>, set: Option<&MibSet>, m: &Mission, framing: Option<&Framing>, done: bool) {
        let (rows, lost, junk) = stream::convert(items, set, m, framing);
        if rows.is_empty() && lost.is_empty() && junk.is_empty() && !done {
            return;
        }
        let Some(Open::Stream(p, _)) = self.pages.get_mut(&id) else { return };
        let from = p.rows.len();
        p.rows.extend(rows);
        let mut notice = None;
        if let (Some(def), Some(set), Some((name, samples))) = (p.param_def, set, &mut p.param) {
            let fresh = stream::samples_of(&p.rows, from, set, m, def);
            let mut was = samples.last().and_then(|s| s.off.clone());
            for s in fresh.iter().filter(|s| !(s.partial && s.off.is_none())) {
                if s.off != was && p.live {
                    let at = s.time.clone().unwrap_or_default();
                    notice = Some(match &s.off {
                        Some((why, hard)) => (format!("{}: {name} {why} -- {} at {at}", p.title, s.value), *hard),
                        None => (format!("{}: {name} back within limits -- {} at {at}", p.title, s.value), false),
                    });
                }
                was = s.off.clone();
            }
            samples.extend(fresh);
        }
        if p.rows.len() > stream::MAX_ROWS {
            let drop = p.rows.len() - stream::MAX_ROWS;
            p.rows.drain(..drop);
            if let Some((_, samples)) = &mut p.param {
                samples.retain(|s| s.row >= drop);
                for s in samples.iter_mut() {
                    s.row -= drop;
                }
            }
        }
        p.lost.extend(lost);
        p.junk.extend(junk);
        p.done |= done;
        p.arrived();
        api::page::refresh(id);
        match notice {
            Some((text, true)) => error(&text),
            Some((text, false)) => api::notify(&text),
            None => {}
        }
    }

    fn stream_info(&mut self, id: u64, framing: String) {
        if let Some(Open::Stream(p, _)) = self.pages.get_mut(&id) {
            p.framing = framing;
            api::page::refresh(id);
        }
    }

    fn stream_status(&mut self, id: u64, text: String, ok: bool) {
        if let Some(Open::Stream(p, _)) = self.pages.get_mut(&id) {
            p.status = Some((text, ok));
            api::page::refresh(id);
        }
    }

    fn stream_failed(&mut self, id: u64, why: &str) {
        self.stream_status(id, why.to_string(), false);
        let m = mission();
        self.stream_rows(id, Vec::new(), None, &m, None, true);
    }

    /// The parameter picked on a stream page: its samples.
    fn follow(&mut self, id: u64, def: DefRef) {
        let Some(set) = self.set() else { return };
        let m = mission();
        let name = set.get(def).name.clone();
        let carriers: Vec<String> = set.used_by(def).iter().map(|d| set.get(*d).name.clone()).collect();
        let Some(Open::Stream(p, _)) = self.pages.get_mut(&id) else { return };
        let samples = stream::samples_of(&p.rows, 0, &set, &m, def);
        let n = samples.len();
        p.param = Some((name.clone(), samples));
        p.param_def = Some(def);
        p.note = Some((format!("{name}: {n} samples in {} packets carrying it", carriers.join(", ")), n == 0));
        p.key(page::Key::Char('3'));
        api::page::refresh(id);
        api::page::focus(id);
    }

    /// `t` on a TM parameter, or a MIB definition's `f`: followed through
    /// the recording or live source opened last.
    fn follow_on_stream(&mut self, name: &str) -> Result<(), String> {
        let Some(set) = self.set() else { return Err("no MIB for this project, so no parameters to follow".into()) };
        let Some(def) = set.resolve(name) else { return Err(format!("{name} isn't in the project's MIBs")) };
        let Some(&stream) = self.pages.iter().filter(|(_, p)| matches!(p, Open::Stream(..))).map(|(id, _)| id).max() else {
            return Err("no recording or live source open -- SPC k f or SPC k l, then t".into());
        };
        self.follow(stream, def);
        Ok(())
    }

    /// Writes what a stream page shows beside its recording, or into the
    /// project's `recordings/` for a live source (or a recording outside
    /// the project).
    fn stream_write(&mut self, id: u64, kind: stream_page::Write) {
        let Some(Open::Stream(p, from)) = self.pages.get(&id) else { return };
        let beside = match from {
            From::File(path, _) => guest_of(path).map(|g| g.rsplit_once('.').map(|(b, _)| b.to_string()).unwrap_or(g)),
            From::Live(_) => None,
        };
        let base = match beside {
            Some(b) => b,
            None => {
                let dir = format!("{PROJECT}/recordings");
                if std::fs::create_dir_all(&dir).is_err() {
                    return error("open a file of the project first -- what's written goes in its recordings/ folder");
                }
                format!("{dir}/{}-{}", p.title, api::clock::now().format("%Y%m%d-%H%M%S"))
            }
        };
        let (path, data): (String, Vec<u8>) = match kind {
            stream_page::Write::Binary => (format!("{base}-filtered.bin"), p.shown().iter().flat_map(|&i| p.rows[i].bytes.clone()).collect()),
            stream_page::Write::Hex => (format!("{base}-filtered.hex.txt"), p.shown().iter().map(|&i| format!("{:08X}  {}\n", p.rows[i].offset, fenix_ccsds::field::hex(&p.rows[i].bytes))).collect::<String>().into_bytes()),
            stream_page::Write::Csv => {
                let Some((name, _)) = &p.param else { return };
                let mut csv = String::from("time,raw,value,limits\n");
                for s in p.samples_shown() {
                    csv.push_str(&format!("{},{},\"{}\",\"{}\"\n", s.time.clone().unwrap_or_default(), s.raw, s.value, s.off.as_ref().map(|o| o.0.clone()).unwrap_or_default()));
                }
                (format!("{base}-{name}.csv"), csv.into_bytes())
            }
        };
        match std::fs::write(&path, data) {
            Ok(()) => api::notify(&format!("wrote {}", path.trim_start_matches("/project/"))),
            Err(e) => error(&format!("couldn't write {path}: {e}")),
        }
    }

    // -- Pages ------------------------------------------------------------------

    fn layout(&mut self, id: u64, width: usize) -> Page {
        match self.pages.get(&id) {
            Some(Open::Hex(p)) => Page::canvas(hex_page::layout(p, width)).typing(p.typing()),
            Some(Open::Packet(p)) => Page::canvas(packet_page::layout(p, width)).typing(p.typing()),
            Some(Open::Stream(p, _)) => Page::canvas(stream_page::layout(p, width)).typing(p.typing()),
            Some(Open::Time(p, _)) => Page::canvas(time_page::layout(p, width)).typing(p.typing()),
            Some(Open::Standards(p)) => Page::canvas(standards_page::layout(p, width)).typing(p.typing()),
            None => Page::new(),
        }
    }

    fn key(&mut self, id: u64, key: page::Key) {
        let Some(open) = self.pages.get_mut(&id) else { return };
        api::page::refresh(id);
        match open {
            Open::Hex(p) => {
                let action = p.key(key);
                self.hex_action(id, action);
            }
            Open::Packet(p) => {
                let action = p.key(key);
                host::page_retitle(id, &packet_page::title(p));
                self.packet_action(id, action);
            }
            Open::Stream(p, _) => {
                let action = p.key(key);
                self.stream_action(id, action);
            }
            Open::Time(p, _) => {
                let action = p.key(key);
                self.time_action(id, action);
            }
            Open::Standards(p) => {
                let action = p.key(key);
                self.standards_action(id, action);
            }
        }
    }

    fn close(&mut self, id: u64) {
        self.forget(id);
        api::page::close(id);
    }

    /// Page `id` is gone: its reader stops.
    fn forget(&mut self, id: u64) {
        if let Some(Open::Stream(p, _)) = self.pages.remove(&id) {
            p.close();
        }
        if let Some(reader) = self.readers.remove(&id) {
            self.drop_reader(id, reader);
        }
        let retries: Vec<u64> = self.timers.iter().filter(|(_, t)| **t == Tick::Retry(id)).map(|(t, _)| *t).collect();
        for t in retries {
            host::cancel_timer(t);
            self.timers.remove(&t);
        }
    }

    fn hex_action(&mut self, id: u64, action: hex_page::Action) {
        match action {
            hex_page::Action::None => {}
            hex_page::Action::Close => self.close(id),
            hex_page::Action::Decode(at) => {
                let Some(Open::Hex(p)) = self.pages.get(&id) else { return };
                let rest = &p.bytes[at.min(p.bytes.len())..];
                let m = mission();
                let len = if rest.starts_with(&fenix_ccsds::coding::ASM) {
                    4 + fenix_ccsds::stream::rs_geometry(m.frames.length, m.frames.rs_depth).map(|(n, _)| n).unwrap_or(m.frames.length)
                } else {
                    match fenix_ccsds::PrimaryHeader::decode(rest) {
                        Some(h) if h.plausible() => h.packet_len(),
                        _ => 64,
                    }
                };
                let bytes = rest[..len.min(rest.len())].to_vec();
                let source = format!("{} @0x{at:X}", p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                self.open_packet(bytes, source, None);
            }
        }
    }

    fn packet_action(&mut self, id: u64, action: packet_page::Action) {
        use packet_page::Action as A;
        match action {
            A::None => {}
            A::Close => self.close(id),
            A::Redecode => {
                if let Some(Open::Packet(mut p)) = self.pages.remove(&id) {
                    self.decode_into(&mut p);
                    self.pages.insert(id, Open::Packet(p));
                }
            }
            A::Copy(text, what) => {
                api::copy(&text);
                api::notify(&format!("copied the {what}"));
            }
            A::Settings => host::run_command("settings.this_project"),
            A::Follow(link) => self.follow_link(link),
            A::FollowParameter(name) => {
                if let Err(e) = self.follow_on_stream(&name) {
                    error(&e);
                }
            }
        }
    }

    /// Opens what a decoded field names: a MIB definition (with the MIB
    /// extension), or a standard's heading.
    fn follow_link(&mut self, link: fenix_ccsds::field::Link) {
        use fenix_ccsds::field::Link;
        let name = match &link {
            Link::Parameter(n) | Link::Telecommand(n) | Link::Spid(n) => n.clone(),
            Link::Standard(std, heading) => return self.open_standard(std, Some(heading)),
        };
        let call = api::call_service("mib.open", &serde_json::json!({ "name": name }).to_string());
        self.calls.insert(call, "opening a MIB definition needs the MIB extension");
    }

    /// A standard's PDF at `heading`, when the library has it.
    fn open_standard(&mut self, std: &str, heading: Option<&str>) {
        let Some(standard) = standards_page::STANDARDS.iter().find(|s| s.id == std) else {
            return api::notify(&format!("{std}{}", heading.map(|h| format!(" · {h}")).unwrap_or_default()));
        };
        let pdfs = standards_page::pdfs_in(Path::new(LIBRARY));
        match standards_page::file_for(standard, &pdfs).cloned() {
            Some(path) => self.open_pdf(&path, None, heading),
            None => error(&format!("{std} ({}) isn't in the standards folder -- SPC k ? says where to get it", standard.title)),
        }
    }

    /// A standard's PDF, in the PDF reader: at a page (from 1) or a
    /// heading.
    fn open_pdf(&mut self, guest: &Path, page: Option<u32>, heading: Option<&str>) {
        let Some(path) = library_path(guest) else { return error("the standards folder isn't set (ccsds.library)") };
        let payload = serde_json::json!({ "path": path, "page": page, "heading": heading }).to_string();
        let call = api::call_service("pdf.open", &payload);
        self.calls.insert(call, "standards open in the PDF reader, the fenix.pdf extension: SPC h x installs it");
    }

    fn stream_action(&mut self, id: u64, action: stream_page::Action) {
        use stream_page::Action as A;
        match action {
            A::None => {}
            A::Close => self.close(id),
            A::Inspect(row) => {
                let Some(Open::Stream(p, _)) = self.pages.get(&id) else { return };
                let Some(r) = p.rows.get(row) else { return };
                let (bytes, source) = (r.bytes.clone(), format!("{} @0x{:X}", p.title, r.offset));
                self.open_packet(bytes, source, None);
            }
            A::Unit(row) => {
                let Some(Open::Stream(p, _)) = self.pages.get(&id) else { return };
                let Some((unit, offset, profile)) = p.rows.get(row).and_then(|r| Some((r.unit.clone()?, r.offset, r.unit_profile.clone()))) else { return };
                let kind = if unit.starts_with(&fenix_ccsds::coding::CLTU_START) { "CLTU" } else { "CADU" };
                let source = format!("{} @0x{offset:X} · the {kind} it came in", p.title);
                self.open_packet(unit.to_vec(), source, profile);
            }
            A::Hex(offset) => match self.pages.get(&id) {
                Some(Open::Stream(_, From::File(path, _))) => {
                    let path = path.clone();
                    self.open_hex(&path, Some(offset));
                }
                _ => api::notify("a live source has no file to show as hex -- w writes one"),
            },
            A::PickParameter => {
                let Some(set) = self.set() else { return error("no MIB for this project, so no parameters to follow") };
                let items: Vec<PickerItem> = set.entries(Kind::TmParam).iter().enumerate().map(|(index, e)| PickerItem::new(format!("{:<10}  {}", e.name, e.description), index.to_string())).collect();
                let pick = api::pick("Follow a TM parameter", &items);
                self.asked.insert(pick, Asked::Parameter(id));
            }
            A::Write(kind) => self.stream_write(id, kind),
            A::Reframe => {
                let Some(Open::Stream(_, From::File(path, framing))) = self.pages.get(&id) else { return };
                let (path, framing) = (path.clone(), framing.clone());
                let m = mission();
                let next = match framing {
                    None => Some(Framing::Packets),
                    Some(Framing::Packets) => Some(Framing::Frames(m.frames.clone())),
                    Some(Framing::Frames(f)) if f.asm => Some(Framing::Frames(fenix_ccsds::frames::FrameProfile { asm: false, ..f })),
                    Some(Framing::Frames(_)) => Some(Framing::Cltus(m.tc.clone())),
                    Some(_) => None,
                };
                self.close(id);
                self.open_recording(&path, next);
            }
        }
    }

    fn time_action(&mut self, id: u64, action: time_page::Action) {
        match action {
            time_page::Action::None => {}
            time_page::Action::Close => self.close(id),
            time_page::Action::Copy(text) => {
                api::copy(&text);
                api::notify(&format!("copied {text}"));
            }
            // At the end of the line it was opened from.
            time_page::Action::Insert(text) => {
                let Some(Open::Time(_, origin)) = self.pages.get(&id) else { return };
                let Some(path) = origin.path.clone() else { return error("open the file the time goes in first") };
                let line = origin.line.saturating_sub(1);
                let end = api::buffer::text(&path).and_then(|t| t.lines().nth(line as usize).map(|l| l.chars().count() as u32)).unwrap_or(0);
                match api::buffer::edit(&path, &[api::TextEdit::new((line, end), (line, end), text.clone())]) {
                    Ok(()) => api::notify(&format!("inserted {text}")),
                    Err(e) => error(&format!("couldn't insert it: {e}")),
                }
            }
        }
    }

    fn standards_action(&mut self, id: u64, action: standards_page::Action) {
        use standards_page::Action as A;
        match action {
            A::None => {}
            A::Close => self.close(id),
            A::Open(path) => self.open_pdf(&path, None, None),
            A::Url(url) => {
                if let Err(e) = api::open_url(url) {
                    error(&e);
                }
            }
            A::Settings => host::run_command("settings.open"),
            A::Search(query) => self.search(id, query),
            A::OpenAt(path, page) => self.open_pdf(&path, Some(page + 1), None),
        }
    }

    /// Searches every standard the folder has, through the PDF reader
    /// (`pdf.search`); what's found goes to the page as each answers.
    fn search(&mut self, id: u64, query: String) {
        let Some(Open::Standards(p)) = self.pages.get_mut(&id) else { return };
        let files = p.searchable();
        if files.is_empty() {
            return error("no standards to search -- f sets the folder they're in (ccsds.library)");
        }
        p.search_started(&query, files.len());
        self.next_search += 1;
        let search = self.next_search;
        self.searched.insert(id, search);
        for (standard, path) in files {
            let Some(path) = library_path(&path) else { continue };
            let call = api::call_service("pdf.search", &serde_json::json!({ "path": path, "query": query }).to_string());
            self.searches.insert(call, (id, standard, search));
        }
    }

    /// A service answered: a standards search's hits, or why one it
    /// called couldn't be.
    fn replied(&mut self, call: u64, answer: Result<String, String>) {
        if let Some((page, standard, search)) = self.searches.remove(&call) {
            if self.searched.get(&page) != Some(&search) {
                return;
            }
            #[derive(serde::Deserialize)]
            struct Hit {
                page: u32,
                context: String,
            }
            let failed = answer.as_ref().err().cloned();
            let hits: Vec<(u32, String)> = answer.ok().and_then(|t| serde_json::from_str::<Vec<Hit>>(&t).ok()).unwrap_or_default().into_iter().map(|h| (h.page, h.context)).collect();
            if let Some(Open::Standards(p)) = self.pages.get_mut(&page) {
                // One that couldn't be read has nothing in it.
                p.found(standard, hits, true);
                api::page::refresh(page);
            }
            if let Some(why) = failed.filter(|w| w.contains("serve")) {
                error(&format!("searching the standards takes the PDF reader, the fenix.pdf extension: SPC h x installs it ({why})"));
            }
            return;
        }
        if let (Some(why), Err(e)) = (self.calls.remove(&call), answer) {
            error(&format!("{why} ({e})"));
        }
    }

    fn picked(&mut self, id: u64, value: Option<String>) {
        let (Some(asked), Some(value)) = (self.asked.remove(&id), value) else { return };
        let Ok(index) = value.parse::<usize>() else { return };
        match asked {
            Asked::Source => {
                if let Some(src) = sources().into_iter().nth(index) {
                    self.open_live(src);
                }
            }
            Asked::Parameter(page) => self.follow(page, DefRef { kind: Kind::TmParam, index }),
        }
    }

    // -- The mission clock ----------------------------------------------------

    /// `ccsds.clock`: the time now as the mission writes it on board, in
    /// the modeline.
    fn clock(&mut self) {
        let wanted = on("ccsds.clock", false);
        match (wanted, self.clock) {
            (true, None) => self.clock = Some(self.timer(CLOCK_MILLIS, true, Tick::Clock)),
            (false, Some(t)) => {
                host::cancel_timer(t);
                self.timers.remove(&t);
                self.clock = None;
                api::status(None);
            }
            _ => {}
        }
    }

    fn clock_tick(&mut self) {
        let m = mission();
        let tai = fenix_ccsds::time::utc_to_tai(chrono::Utc::now().naive_utc(), &m.profile.leap);
        api::status(Some(&format!("OBT {}", time_page::mission_code(tai, &m.profile))));
    }

    fn timer_went(&mut self, timer: u64) {
        match self.timers.get(&timer).copied() {
            Some(Tick::Read) => self.read_recordings(),
            Some(Tick::Tail(id)) => self.tail(id),
            Some(Tick::Retry(id)) => {
                self.timers.remove(&timer);
                if self.pages.contains_key(&id) {
                    self.connect(id);
                }
            }
            Some(Tick::Clock) => self.clock_tick(),
            None => {}
        }
    }

    fn socket_event(&mut self, event: Event) {
        match event {
            Event::SocketConnected((socket, result)) => {
                let Some(&id) = self.sockets.get(&socket) else { return };
                match result {
                    Ok(()) => {
                        let name = match self.pages.get(&id) {
                            Some(Open::Stream(_, From::Live(src))) => src.name.clone(),
                            _ => String::new(),
                        };
                        // NATS: what receiving needs, CONNECT and SUB.
                        if let Some(Reader::Nats { subject, .. }) = self.readers.get(&id) {
                            let subject = subject.clone();
                            let hello = format!("CONNECT {{\"verbose\":false,\"pedantic\":false,\"name\":\"fenix\",\"lang\":\"rust\",\"version\":\"1\"}}\r\nSUB {subject} 1\r\nPING\r\n");
                            let _ = socket::write(socket, hello.as_bytes());
                            self.stream_status(id, format!("{name} subscribed to {subject}"), true);
                        } else {
                            self.stream_status(id, format!("{name} connected"), true);
                        }
                    }
                    Err(e) => self.retry(id, &e),
                }
            }
            Event::SocketData((socket, data)) => {
                if let Some(&id) = self.sockets.get(&socket) {
                    self.fed(id, &data);
                }
            }
            Event::Datagram(d) => {
                if let Some(&id) = self.sockets.get(&d.socket) {
                    self.fed(id, &d.data);
                }
            }
            Event::SocketClosed((socket, why)) => {
                if let Some(id) = self.sockets.remove(&socket) {
                    self.readers.remove(&id);
                    self.retry(id, why.as_deref().unwrap_or("closed by the other end"));
                }
            }
            _ => {}
        }
    }
}

/// `K` on hex or an on-board time, in any file.
impl LanguageServer for Ccsds {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities { hover_provider: Some(HoverProviderCapability::Simple(true)), ..Default::default() }
    }

    fn hover(&mut self, _server: u64, docs: &Documents, params: HoverParams) -> Option<Hover> {
        let at = &params.text_document_position_params;
        let line = docs.line(&at.text_document.uri, at.position.line)?.trim_end_matches(['\n', '\r']).to_string();
        let col = lsp::utf16_to_char(&line, at.position.character);
        let m = mission();
        // The MIB only for hex worth reading through it.
        let set = decode::hex_at(&line, col).filter(|b| b.len() >= 6).and_then(|_| self.set());
        let value = decode::hover(&line, col, set.as_deref(), &m)?;
        Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::PlainText, value }), range: None })
    }
}

impl Extension for CcsdsExtension {
    fn activate(&mut self) -> Result<(), String> {
        self.ccsds.clock();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        self.ccsds.command(id)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Picked((id, value)) => self.ccsds.picked(id, value),
            Event::Timer(t) => self.ccsds.timer_went(t),
            Event::ServiceReply((id, answer)) => self.ccsds.replied(id, answer),
            Event::SettingChanged(key) => {
                if key.starts_with("mib.") {
                    self.ccsds.set = None;
                }
                if key == "ccsds.clock" {
                    self.ccsds.clock();
                }
            }
            Event::FocusChanged(_) => self.ccsds.clock(),
            e @ (Event::SocketConnected(_) | Event::SocketData(_) | Event::Datagram(_) | Event::SocketClosed(_)) => self.ccsds.socket_event(e),
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        self.ccsds.layout(page, width as usize)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.ccsds.key(page, key.into());
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        match self.ccsds.pages.get_mut(&page) {
            Some(Open::Hex(p)) => p.paste(text),
            Some(Open::Packet(p)) => p.paste(text),
            Some(Open::Stream(p, _)) => p.paste(text),
            Some(Open::Time(p, _)) => p.paste(text),
            Some(Open::Standards(p)) => p.query.paste(text),
            None => return,
        }
        api::page::refresh(page);
    }

    fn page_closed(&mut self, page: u64) {
        self.ccsds.forget(page);
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.servers.handle(server, message, &mut self.ccsds);
    }

    /// `ccsds.hex {path}`: a file as hex (Fenix, for a binary file);
    /// `ccsds.decode {hex, source}`: the inspector on bytes (the MIB's
    /// insert form); `ccsds.follow {name}`: a TM parameter followed on
    /// the recording or live source open (a MIB definition's `f`).
    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let call: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
        let field = |key: &str| call.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let answer = match name {
            "ccsds.hex" => match field("path") {
                Some(path) => {
                    self.ccsds.open_hex(&path, None);
                    Ok(String::new())
                }
                None => Err("ccsds.hex: {path}".into()),
            },
            "ccsds.decode" => match field("hex").as_deref().and_then(fenix_ccsds::hex::parse) {
                Some(bytes) => {
                    self.ccsds.open_packet(bytes, field("source").unwrap_or_else(|| "bytes".into()), None);
                    Ok(String::new())
                }
                None => Err("ccsds.decode: {hex} isn't hex".into()),
            },
            "ccsds.follow" => match field("name") {
                Some(name) => self.ccsds.follow_on_stream(&name).map(|()| String::new()),
                None => Err("ccsds.follow: {name}".into()),
            },
            other => Err(format!("no service {other}")),
        };
        Some(answer)
    }
}

export!(CcsdsExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_in_the_project_is_under_project() {
        assert_eq!(guest_in("C:\\w\\mission\\recordings\\a.bin", "C:\\w\\mission").as_deref(), Some("/project/recordings/a.bin"));
        assert_eq!(guest_in("c:/W/Mission/a.bin", "C:\\w\\mission").as_deref(), Some("/project/a.bin"), "Windows paths, either case");
        assert_eq!(guest_in("/home/me/m/x", "/home/me/m").as_deref(), Some("/project/x"));
        assert_eq!(guest_in("C:\\w\\missionx\\a.bin", "C:\\w\\mission"), None, "a folder that only starts the same");
        assert_eq!(guest_in("C:\\other\\a.bin", "C:\\w\\mission"), None);
    }

    #[test]
    fn the_longest_hex_on_a_line_is_the_one() {
        assert_eq!(longest_hex("pkt = \"1B F2 C0 07 00 09\" # ZTC08101"), Some(vec![0x1B, 0xF2, 0xC0, 0x07, 0x00, 0x09]));
        assert_eq!(longest_hex("puts hello"), None);
        assert_eq!(longest_hex("    send 1BF2C007 0009 now"), Some(vec![0x1B, 0xF2, 0xC0, 0x07, 0x00, 0x09]), "a word before it that ends in hex");
        assert_eq!(longest_hex("bytes = {0x1B, 0xF2, 0xC0, 0x07}"), Some(vec![0x1B, 0xF2, 0xC0, 0x07]));
    }
}
