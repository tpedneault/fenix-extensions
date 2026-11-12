//! `cargo xtask screenshots [extension] [--fenix <path>] [--theme <name>] [--size WxH] [--dry-run]`
//!
//! Retakes an extension's store pictures unattended: its
//! `screenshots.toml` says, for each picture, a sample project to open
//! (from `screenshots/samples/`), files to open in it, commands to run,
//! what `fenix/state` should say before the shot, and the file it's
//! saved as under the extension's `screenshots/`.
//!
//! For each picture the extension is packed, installed into a clean
//! profile (`FENIX_HOME`, under `target/screenshots/`), and a Fenix is
//! started there with the theme and window size given. The script's
//! steps go over the control channel (Fenix's `docs/CONTROL.md`); the
//! channel can't capture, so the window is taken with the system's own
//! tool (`capture_window`). Then that Fenix is stopped.
//!
//! `cargo xtask validate` checks every script it finds (`validate_all`).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

/// The script, in an extension's folder. Never packed.
pub const SCRIPT: &str = "screenshots.toml";
/// Where the sample projects are, from the top of the repository.
const SAMPLES: &str = "screenshots/samples";
/// The house style: one theme, one window size, up to three pictures.
const THEME: &str = "Orbit Dark";
const SIZE: (u32, u32) = (1600, 1000);
const MAX_SHOTS: usize = 3;
/// How long a wait may be given, and is without one.
const MAX_TIMEOUT_S: u64 = 120;
const TIMEOUT_S: u64 = 15;
/// What's left for drawing after the wait, without `settle_ms`; and the
/// most it may be.
const SETTLE_MS: u64 = 1000;
const MAX_SETTLE_MS: u64 = 10_000;
/// How often `fenix/state` is read while waiting.
const POLL: Duration = Duration::from_millis(500);
/// Frames one poll may see drawn and still call Fenix idle: the caret
/// blinks every 500 ms for 30 s after a file opens, and answering a
/// read can draw one.
const IDLE_FRAMES: u64 = 2;
/// How long a starting Fenix has to open its channel.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether `rel`, a path in an extension's folder, is its script:
/// `pack` leaves it out.
pub fn is_script(rel: &Path) -> bool {
    rel == Path::new(SCRIPT)
}

// ------------------------------------------------------------------
// The script
// ------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(rename = "shot", default)]
    pub shots: Vec<Shot>,
}

/// One picture.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    /// Its name under the extension's `screenshots/`: `table.png`.
    pub file: String,
    /// What's in it, or what stands in for a service; printed with the plan.
    #[serde(default)]
    pub note: Option<String>,
    /// A folder under `screenshots/samples/`, opened as the project.
    #[serde(default)]
    pub project: Option<String>,
    /// Files in the project, opened in order (the last has the focus):
    /// `data/people.csv`, or with a position, `src/main.rs:12[:3]`.
    #[serde(default)]
    pub open: Vec<String>,
    /// Commands run in order with `fenix/runCommand`: Fenix's, or an
    /// extension's marked `external = true`.
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub wait: Option<Wait>,
    /// Milliseconds after the wait, before the picture.
    #[serde(default)]
    pub settle_ms: Option<u64>,
}

/// What Fenix should say before the picture is taken; every one given
/// must hold at once.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    /// `fenix/state`'s `project` is the shot's sample.
    #[serde(default)]
    pub project: bool,
    /// `file` is this one, in the project.
    #[serde(default)]
    pub file: Option<String>,
    /// There's no `file`: a page or tool has the focus.
    #[serde(default)]
    pub no_file: bool,
    /// The cursor's line, from 1.
    #[serde(default)]
    pub line: Option<u64>,
    /// Nothing but the caret is being drawn (`stats.frames`, read a poll
    /// apart).
    #[serde(default)]
    pub idle: bool,
    /// At least this many problems in `file` (`fenix/diagnostics`).
    #[serde(default)]
    pub diagnostics: Option<usize>,
    /// Seconds before giving up.
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

/// A file to open, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub path: String,
    pub line: Option<u64>,
    pub column: Option<u64>,
}

/// `data/x.csv`, `src/main.rs:12`, `src/main.rs:12:3`.
pub fn opening(spec: &str) -> Opening {
    let number = |s: &str| (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse::<u64>().ok()).flatten();
    if let Some((rest, b)) = spec.rsplit_once(':') {
        if let Some(b) = number(b) {
            if let Some((path, a)) = rest.rsplit_once(':') {
                if let Some(a) = number(a) {
                    return Opening { path: path.into(), line: Some(a), column: Some(b) };
                }
            }
            return Opening { path: rest.into(), line: Some(b), column: None };
        }
    }
    Opening { path: spec.into(), line: None, column: None }
}

/// A relative path that stays where it's put: no root, no `..`.
fn relative(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// `table.png`: lower case, digits, `-` and `_`, and PNG -- the house style.
fn picture_name(name: &str) -> bool {
    name.strip_suffix(".png").is_some_and(|stem| !stem.is_empty() && stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
}

fn command_shape(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'))
}

/// Every command the repository's extensions contribute, and which of
/// them say `external = true`; and the namespaces those ids use.
#[derive(Debug, Default)]
pub struct Known {
    /// Id: (the extension's folder, external).
    commands: BTreeMap<String, (String, bool)>,
    /// The part before the first `.`: the extension's folder.
    namespaces: BTreeMap<String, String>,
}

pub fn known_commands(root: &Path) -> Result<Known, String> {
    let mut known = Known::default();
    let dirs = std::fs::read_dir(root.join("extensions")).map_err(|e| format!("extensions/: {e}"))?;
    let mut dirs: Vec<PathBuf> = dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = dir.join("extension.toml");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
        let contributes = table.get("contributes");
        for command in contributes.and_then(|c| c.get("commands")).and_then(|c| c.as_array()).into_iter().flatten() {
            let Some(id) = command.get("id").and_then(|i| i.as_str()) else { continue };
            let external = command.get("external").and_then(|e| e.as_bool()).unwrap_or(false);
            known.commands.insert(id.to_string(), (folder.clone(), external));
            if let Some((ns, _)) = id.split_once('.') {
                known.namespaces.entry(ns.to_string()).or_insert(folder.clone());
            }
        }
        for ns in contributes.and_then(|c| c.get("namespaces")).and_then(|n| n.as_array()).into_iter().flatten().filter_map(|n| n.as_str()) {
            known.namespaces.entry(ns.to_string()).or_insert(folder.clone());
        }
    }
    Ok(known)
}

/// Reads and checks `dir`'s script: what can be known without a Fenix.
/// `samples` is `screenshots/samples`.
pub fn check(dir: &Path, samples: &Path, known: &Known) -> Result<Script, String> {
    let path = dir.join(SCRIPT);
    let at = path.display();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{at}: {e}"))?;
    let script: Script = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
    if script.shots.is_empty() {
        return Err(format!("{at}: no [[shot]]"));
    }
    if script.shots.len() > MAX_SHOTS {
        return Err(format!("{at}: {} shots -- at most {MAX_SHOTS} an extension (the first is the hero)", script.shots.len()));
    }
    let mut names = BTreeSet::new();
    for shot in &script.shots {
        let name = &shot.file;
        if !picture_name(name) {
            return Err(format!("{at}: file {name:?} -- a name for screenshots/, lower case, ending .png"));
        }
        if !names.insert(name.clone()) {
            return Err(format!("{at}: {name} is there twice"));
        }
        if shot.project.is_none() && shot.open.is_empty() && shot.commands.is_empty() {
            return Err(format!("{at}: {name} opens nothing and runs nothing"));
        }
        let project = match &shot.project {
            Some(p) if !relative(p) => return Err(format!("{at}: {name}: project {p:?} -- a folder under {SAMPLES}/")),
            Some(p) => {
                let folder = samples.join(p);
                if !folder.is_dir() {
                    return Err(format!("{at}: {name}: {SAMPLES}/{p} isn't there"));
                }
                // Without one, openProject opens a file finder over it,
                // and that would be in the picture.
                if !folder.join("README.md").is_file() {
                    return Err(format!("{at}: {name}: {SAMPLES}/{p} has no README.md -- fenix/openProject opens it, else a file finder that would be in the picture"));
                }
                Some(folder)
            }
            None => None,
        };
        for spec in &shot.open {
            let o = opening(spec);
            let Some(folder) = &project else {
                return Err(format!("{at}: {name}: open {spec:?} -- files are opened from the shot's project; it has none"));
            };
            if !relative(&o.path) || !folder.join(&o.path).is_file() {
                return Err(format!("{at}: {name}: open {spec:?} -- no such file in {SAMPLES}/{}", shot.project.as_deref().unwrap_or_default()));
            }
            if o.line == Some(0) || o.column == Some(0) {
                return Err(format!("{at}: {name}: open {spec:?} -- lines and columns count from 1"));
            }
        }
        for id in &shot.commands {
            if !command_shape(id) {
                return Err(format!("{at}: {name}: command {id:?} isn't an id"));
            }
            match known.commands.get(id) {
                Some((_, true)) => {}
                Some((folder, false)) => return Err(format!("{at}: {name}: {id} is {folder}'s, and isn't external = true -- fenix/runCommand refuses it")),
                None => {
                    if let Some(folder) = id.split_once('.').and_then(|(ns, _)| known.namespaces.get(ns)) {
                        return Err(format!("{at}: {name}: {id} -- {folder} has no such command"));
                    }
                    // Fenix's own, then: `fenix/commands` says at run time.
                }
            }
        }
        if let Some(wait) = &shot.wait {
            if wait.file.is_some() && wait.no_file {
                return Err(format!("{at}: {name}: wait for a file and for no file?"));
            }
            if let Some(file) = &wait.file {
                let Some(folder) = &project else { return Err(format!("{at}: {name}: wait.file is in the shot's project; it has none")) };
                if !relative(file) || !folder.join(file).is_file() {
                    return Err(format!("{at}: {name}: wait.file {file:?} -- no such file in the project"));
                }
            }
            if wait.project && project.is_none() {
                return Err(format!("{at}: {name}: wait.project, and no project"));
            }
            if wait.diagnostics.is_some() && wait.file.is_none() {
                return Err(format!("{at}: {name}: wait.diagnostics counts a file's -- give wait.file"));
            }
            if wait.line == Some(0) {
                return Err(format!("{at}: {name}: wait.line counts from 1"));
            }
            if let Some(t) = wait.timeout_s.filter(|t| !(1..=MAX_TIMEOUT_S).contains(t)) {
                return Err(format!("{at}: {name}: wait.timeout_s is {t}: from 1 to {MAX_TIMEOUT_S}"));
            }
            if *wait == (Wait { timeout_s: wait.timeout_s, ..Wait::default() }) {
                return Err(format!("{at}: {name}: [shot.wait] waits for nothing"));
            }
        }
        if let Some(ms) = shot.settle_ms.filter(|ms| *ms > MAX_SETTLE_MS) {
            return Err(format!("{at}: {name}: settle_ms is {ms}: at most {MAX_SETTLE_MS}"));
        }
    }
    Ok(script)
}

/// Every extension with a script, by folder name, sorted.
fn scripted(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(root.join("extensions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(SCRIPT).is_file())
        .map(|p| (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p))
        .collect();
    out.sort();
    out
}

/// `cargo xtask validate`'s part: every script there is.
pub fn validate_all(root: &Path) -> Result<(), String> {
    let known = known_commands(root)?;
    let samples = root.join(SAMPLES);
    let mut count = 0;
    for (name, dir) in scripted(root) {
        let script = check(&dir, &samples, &known)?;
        println!("ok  {name}/{SCRIPT}: {}", script.shots.iter().map(|s| s.file.as_str()).collect::<Vec<_>>().join(", "));
        count += 1;
    }
    println!("{count} screenshot script(s)");
    Ok(())
}

// ------------------------------------------------------------------
// The control channel
// ------------------------------------------------------------------

/// What a client talks to Fenix over: the socket or the pipe, or a fake.
pub trait Transport: Read + Write {}
impl<T: Read + Write + ?Sized> Transport for T {}

/// Writes one message: a `Content-Length` header, a blank line, the body.
pub fn write_frame(w: &mut dyn Write, body: &[u8]) -> std::io::Result<()> {
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())?;
    w.write_all(body)?;
    w.flush()
}

/// Reads one message's body. Headers other than `Content-Length` are
/// passed over.
pub fn read_frame(r: &mut dyn Read) -> std::io::Result<Vec<u8>> {
    let bad = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
    let mut length = None;
    for _ in 0..32 {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8];
            r.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
            if line.len() > 8192 {
                return Err(bad("a header line over 8 KiB".into()));
            }
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            let length = length.ok_or_else(|| bad("a message without Content-Length".into()))?;
            if length > 64 * 1024 * 1024 {
                return Err(bad(format!("a message of {length} bytes")));
            }
            let mut body = vec![0u8; length];
            r.read_exact(&mut body)?;
            return Ok(body);
        }
        let line = String::from_utf8_lossy(&line);
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().map_err(|e| bad(format!("Content-Length: {e}")))?);
            }
        }
    }
    Err(bad("too many header lines".into()))
}

/// What `fenix/hello` answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Hello {
    pub version: Option<String>,
    pub pid: Option<u32>,
    pub capabilities: Vec<String>,
}

/// A JSON-RPC client of Fenix's control channel: requests answered in
/// order, one at a time.
pub struct Client<T: Transport> {
    io: T,
    next: u64,
}

impl<T: Transport> Client<T> {
    pub fn new(io: T) -> Client<T> {
        Client { io, next: 1 }
    }

    /// One request and its answer: the result, or the error as
    /// `method: code message`.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        let body = serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).map_err(|e| e.to_string())?;
        write_frame(&mut self.io, &body).map_err(|e| format!("{method}: writing to Fenix: {e}"))?;
        loop {
            let reply = read_frame(&mut self.io).map_err(|e| format!("{method}: reading Fenix's answer: {e}"))?;
            let reply: Value = serde_json::from_slice(&reply).map_err(|e| format!("{method}: Fenix's answer isn't JSON: {e}"))?;
            // Anything without an id is a notification, not the answer.
            if reply.get("id").is_none_or(Value::is_null) && reply.get("error").is_none() {
                continue;
            }
            if let Some(error) = reply.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error.get("message").and_then(Value::as_str).unwrap_or("");
                return Err(format!("{method}: {code} {message}"));
            }
            if reply["id"] != json!(id) {
                return Err(format!("{method}: Fenix answered request {} for {id}", reply["id"]));
            }
            return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// `fenix/hello`, which every connection starts with.
    pub fn hello(&mut self) -> Result<Hello, String> {
        let result = self.call("fenix/hello", json!({ "protocol": 1, "client": { "name": "xtask-screenshots", "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id() } }))?;
        if result.get("protocol") != Some(&json!(1)) {
            return Err(format!("fenix/hello: protocol {} -- this speaks 1", result.get("protocol").unwrap_or(&Value::Null)));
        }
        Ok(Hello {
            version: result.get("version").and_then(Value::as_str).map(str::to_string),
            pid: result.get("pid").and_then(Value::as_u64).map(|p| p as u32),
            capabilities: result.get("capabilities").and_then(Value::as_array).map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        })
    }

    /// `fenix/commands`, as id: external.
    pub fn commands(&mut self) -> Result<BTreeMap<String, bool>, String> {
        let result = self.call("fenix/commands", json!({}))?;
        Ok(result
            .get("commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| Some((c.get("id")?.as_str()?.to_string(), c.get("external").and_then(Value::as_bool).unwrap_or(false))))
            .collect())
    }
}

/// The 64-bit FNV-1a of `FENIX_HOME`'s bytes, as the endpoint's suffix
/// has it.
pub fn home_hash(home: &OsStr) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in home.as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Where a Fenix with this `FENIX_HOME` listens, for this user.
#[cfg(unix)]
fn endpoint(home: &OsStr) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    let folder = match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        Some(runtime) => PathBuf::from(runtime).join("fenix"),
        None => {
            let uid = std::fs::metadata("/proc/self").map_err(|e| format!("your uid, from /proc/self: {e}"))?.uid();
            PathBuf::from(format!("/tmp/fenix-{uid}"))
        }
    };
    Ok(folder.join(format!("control-{}.sock", home_hash(home))))
}

#[cfg(windows)]
fn endpoint(home: &OsStr) -> Result<PathBuf, String> {
    // The user's SID and this session's id, from what ships with Windows.
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value + ' ' + (Get-Process -Id $PID).SessionId"])
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (sid, session) = text.trim().split_once(' ').ok_or_else(|| format!("powershell said {text:?}, not a SID and a session"))?;
    Ok(PathBuf::from(format!(r"\\.\pipe\fenix-{sid}-{session}-{}", home_hash(home))))
}

/// Connected to it, or why not (not listening yet, most likely).
#[cfg(unix)]
fn connect(endpoint: &Path) -> std::io::Result<Box<dyn Transport>> {
    let stream = std::os::unix::net::UnixStream::connect(endpoint)?;
    // Fenix answers within 30 s, or with -32603.
    stream.set_read_timeout(Some(Duration::from_secs(45)))?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
fn connect(endpoint: &Path) -> std::io::Result<Box<dyn Transport>> {
    use std::os::windows::fs::OpenOptionsExt;
    // SECURITY_IDENTIFICATION: Fenix may learn who this is, nothing more.
    const SECURITY_IDENTIFICATION: u32 = 1 << 16;
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..20 {
        match std::fs::OpenOptions::new().read(true).write(true).share_mode(0).security_qos_flags(SECURITY_IDENTIFICATION).open(endpoint) {
            Ok(file) => return Ok(Box::new(file)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("the pipe stayed busy"))
}

// ------------------------------------------------------------------
// Running a shot
// ------------------------------------------------------------------

/// What the runner needs besides Fenix: time, and the picture.
pub trait Host {
    fn elapsed(&self) -> Duration;
    fn sleep(&mut self, d: Duration);
    fn capture(&mut self, out: &Path) -> Result<(), String>;
}

/// Whether two paths are the same file, as Fenix and this write them.
fn same_path(a: &str, b: &Path) -> bool {
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(&b.to_string_lossy())
}

fn plain(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Runs `shot`'s steps against a Fenix with these `commands` (id:
/// external, from `fenix/commands`; `None` when it can't say), its
/// sample projects in `samples`, and takes the picture into `out`.
pub fn run_shot<T: Transport>(client: &mut Client<T>, host: &mut dyn Host, shot: &Shot, samples: &Path, commands: Option<&BTreeMap<String, bool>>, out: &Path) -> Result<(), String> {
    let name = &shot.file;
    // Refused before anything runs, rather than halfway.
    if let Some(commands) = commands {
        for id in &shot.commands {
            match commands.get(id) {
                Some(true) => {}
                Some(false) => return Err(format!("{name}: {id} isn't external in this Fenix: fenix/runCommand won't run it")),
                None => return Err(format!("{name}: this Fenix has no command {id} (is its extension installed and on?)")),
            }
        }
    }
    let project = shot.project.as_ref().map(|p| samples.join(p));
    if let Some(root) = &project {
        client.call("fenix/openProject", json!({ "root": plain(root) })).map_err(|e| format!("{name}: {e}"))?;
    }
    if !shot.open.is_empty() {
        let root = project.as_ref().ok_or(format!("{name}: files to open, and no project"))?;
        let paths: Vec<Value> = shot
            .open
            .iter()
            .map(|spec| {
                let o = opening(spec);
                let mut p = json!({ "path": plain(&root.join(&o.path)) });
                if let Some(line) = o.line {
                    p["line"] = json!(line);
                }
                if let Some(column) = o.column {
                    p["column"] = json!(column);
                }
                p
            })
            .collect();
        client.call("fenix/open", json!({ "paths": paths, "focus": true })).map_err(|e| format!("{name}: {e}"))?;
    }
    for id in &shot.commands {
        client.call("fenix/runCommand", json!({ "id": id })).map_err(|e| format!("{name}: {e}"))?;
    }
    if let Some(wait) = &shot.wait {
        wait_for(client, host, wait, project.as_deref()).map_err(|e| format!("{name}: {e}"))?;
    }
    host.sleep(Duration::from_millis(shot.settle_ms.unwrap_or(SETTLE_MS)));
    host.capture(out).map_err(|e| format!("{name}: {e}"))
}

/// Polls `fenix/state` (and `fenix/diagnostics`) until `wait` holds, or
/// its timeout passes.
fn wait_for<T: Transport>(client: &mut Client<T>, host: &mut dyn Host, wait: &Wait, project: Option<&Path>) -> Result<(), String> {
    let timeout = Duration::from_secs(wait.timeout_s.unwrap_or(TIMEOUT_S));
    let deadline = host.elapsed() + timeout;
    let file = wait.file.as_ref().zip(project).map(|(f, root)| root.join(f));
    let mut frames: Option<u64> = None;
    loop {
        let state = client.call("fenix/state", json!({}))?;
        let mut unmet = Vec::new();
        let got = |key: &str| state.get(key).and_then(Value::as_str);
        if wait.project && !got("project").zip(project).is_some_and(|(a, b)| same_path(a, b)) {
            unmet.push(format!("project {}", project.map(plain).unwrap_or_default()));
        }
        if let Some(file) = &file {
            if !got("file").is_some_and(|a| same_path(a, file)) {
                unmet.push(format!("file {}", plain(file)));
            }
        }
        if wait.no_file && got("file").is_some() {
            unmet.push("no file in focus".into());
        }
        if let Some(line) = wait.line {
            if state.get("line").and_then(Value::as_u64) != Some(line) {
                unmet.push(format!("line {line}"));
            }
        }
        if wait.idle {
            let now = state.get("stats").and_then(|s| s.get("frames")).and_then(Value::as_u64).ok_or("this Fenix's fenix/state has no stats.frames: wait.idle can't be told")?;
            if !frames.is_some_and(|before| now.saturating_sub(before) <= IDLE_FRAMES) {
                unmet.push("idle".into());
            }
            frames = Some(now);
        }
        if let (Some(least), Some(file)) = (wait.diagnostics, &file) {
            let result = client.call("fenix/diagnostics", json!({ "path": plain(file) }))?;
            let count = result.get("diagnostics").and_then(Value::as_array).map_or(0, Vec::len);
            if count < least {
                unmet.push(format!("{least} problem(s) in {} ({count} so far)", plain(file)));
            }
        }
        if unmet.is_empty() {
            return Ok(());
        }
        if host.elapsed() >= deadline {
            return Err(format!("waited {} s for {}; fenix/state said {state}", timeout.as_secs(), unmet.join(", ")));
        }
        host.sleep(POLL);
    }
}

// ------------------------------------------------------------------
// Capturing the window
// ------------------------------------------------------------------

/// The window of process `pid`, its client area, as a PNG in `out`.
/// `work` is a folder for what the tool needs.
#[cfg(windows)]
fn capture_window(pid: u32, out: &Path, work: &Path) -> Result<(), String> {
    let script = work.join("capture.ps1");
    std::fs::write(&script, CAPTURE_PS1).map_err(|e| format!("{}: {e}", script.display()))?;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-ProcessId")
        .arg(pid.to_string())
        .arg("-Out")
        .arg(out)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !output.status.success() || !out.is_file() {
        return Err(format!("the capture failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

/// PowerShell and .NET, as Windows ships them: the window's picture
/// (PrintWindow with PW_RENDERFULLCONTENT, which draws a GPU window even
/// when another covers it; the screen under it if that fails), cut to
/// its client area.
#[cfg(any(windows, test))]
const CAPTURE_PS1: &str = r#"param([int]$ProcessId, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class FenixShot {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
"@
[void][FenixShot]::SetProcessDPIAware()
$p = Get-Process -Id $ProcessId
$h = $p.MainWindowHandle
for ($i = 0; $h -eq [IntPtr]::Zero -and $i -lt 50; $i++) { Start-Sleep -Milliseconds 200; $p.Refresh(); $h = $p.MainWindowHandle }
if ($h -eq [IntPtr]::Zero) { throw "Fenix (process $ProcessId) has no window" }
if ([FenixShot]::IsIconic($h)) { [void][FenixShot]::ShowWindow($h, 9) }
[void][FenixShot]::SetForegroundWindow($h)
$w = New-Object FenixShot+RECT
[void][FenixShot]::GetWindowRect($h, [ref]$w)
$c = New-Object FenixShot+RECT
[void][FenixShot]::GetClientRect($h, [ref]$c)
$o = New-Object FenixShot+POINT
[void][FenixShot]::ClientToScreen($h, [ref]$o)
$ww = $w.Right - $w.Left
$wh = $w.Bottom - $w.Top
if ($c.Right -le 0 -or $c.Bottom -le 0) { throw "Fenix's window has no client area" }
$full = New-Object System.Drawing.Bitmap -ArgumentList $ww, $wh
$g = [System.Drawing.Graphics]::FromImage($full)
$hdc = $g.GetHdc()
$ok = [FenixShot]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
if (-not $ok) { $g.CopyFromScreen($w.Left, $w.Top, 0, 0, $full.Size) }
$g.Dispose()
$rect = New-Object System.Drawing.Rectangle -ArgumentList ($o.X - $w.Left), ($o.Y - $w.Top), $c.Right, $c.Bottom
$shot = $full.Clone($rect, $full.PixelFormat)
$shot.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$shot.Dispose()
$full.Dispose()
"#;

/// Best effort: X11 (Fenix is started without `WAYLAND_DISPLAY`, so on
/// Wayland it's Xwayland's), the window found by `xdotool`, taken by
/// ImageMagick's `import` or by `maim`.
#[cfg(not(windows))]
fn capture_window(pid: u32, out: &Path, _work: &Path) -> Result<(), String> {
    use std::process::Command;
    if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err("no DISPLAY: capturing a window needs X11 (or Xwayland)".into());
    }
    fn missing(tool: &'static str) -> impl Fn(std::io::Error) -> String {
        move |e| if e.kind() == std::io::ErrorKind::NotFound { format!("{tool} isn't installed: capturing needs xdotool, and ImageMagick's import or maim") } else { format!("{tool}: {e}") }
    }
    let found = Command::new("xdotool").args(["search", "--onlyvisible", "--pid", &pid.to_string()]).output().map_err(missing("xdotool"))?;
    let window = String::from_utf8_lossy(&found.stdout).lines().last().map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).ok_or(format!("xdotool found no window of process {pid}"))?;
    let _ = Command::new("xdotool").args(["windowactivate", "--sync", &window]).status();
    let taken = match Command::new("import").args(["-window", &window]).arg(out).status() {
        Ok(status) => status.success(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Command::new("maim").args(["-i", &window]).arg(out).status().map_err(missing("import and maim"))?.success(),
        Err(e) => return Err(format!("import: {e}")),
    };
    if !taken || !out.is_file() {
        return Err(format!("capturing window {window} failed"));
    }
    Ok(())
}

// ------------------------------------------------------------------
// The command
// ------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Options {
    pub extension: Option<String>,
    pub fenix: PathBuf,
    pub theme: String,
    pub size: (u32, u32),
    pub dry_run: bool,
}

/// The Fenix to run when `--fenix` doesn't say: `$FENIX`, else `fenix` on
/// the `PATH`.
pub(crate) fn default_fenix() -> PathBuf {
    std::env::var_os("FENIX").filter(|f| !f.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("fenix"))
}

pub fn options(args: &[String]) -> Result<Options, String> {
    let mut o = Options { extension: None, fenix: default_fenix(), theme: THEME.into(), size: SIZE, dry_run: false };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| args.next().cloned()).ok_or(format!("{flag} needs a value"));
        match flag {
            "--fenix" => o.fenix = PathBuf::from(value()?),
            "--theme" => o.theme = value()?,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once(['x', 'X']).ok_or(format!("--size {v}: WxH, as 1600x1000"))?;
                let (w, h) = (w.parse::<u32>().map_err(|_| format!("--size {v}: WxH"))?, h.parse::<u32>().map_err(|_| format!("--size {v}: WxH"))?);
                if !(640..=7680).contains(&w) || !(480..=4320).contains(&h) {
                    return Err(format!("--size {v}: from 640x480 to 7680x4320"));
                }
                o.size = (w, h);
            }
            "--dry-run" => o.dry_run = true,
            f if f.starts_with('-') => return Err(format!("screenshots: {f}? -- [extension] [--fenix <path>] [--theme <name>] [--size WxH] [--dry-run]")),
            _ if o.extension.is_none() => o.extension = Some(arg.clone()),
            _ => return Err(format!("screenshots: one extension at a time, or none for all ({arg}?)")),
        }
    }
    if o.theme.trim().is_empty() || o.theme.contains(['"', '\n', '\\']) {
        return Err(format!("--theme {:?}: a theme's name", o.theme));
    }
    Ok(o)
}

/// The plan for one extension, as `--dry-run` prints it.
fn plan(name: &str, script: &Script, o: &Options) -> String {
    let mut s = format!("{name}: {} shot(s) into extensions/{name}/screenshots/, theme {:?}, {}x{}, a clean profile in target/screenshots/{name}\n", script.shots.len(), o.theme, o.size.0, o.size.1);
    for (i, shot) in script.shots.iter().enumerate() {
        s += &format!("  {}. {}{}\n", i + 1, shot.file, if i == 0 { "  (the hero)" } else { "" });
        if let Some(note) = &shot.note {
            s += &format!("     note      {note}\n");
        }
        if let Some(p) = &shot.project {
            s += &format!("     project   {SAMPLES}/{p}\n");
        }
        for f in &shot.open {
            s += &format!("     open      {f}\n");
        }
        for c in &shot.commands {
            s += &format!("     run       {c}\n");
        }
        if let Some(w) = &shot.wait {
            let mut what = Vec::new();
            if w.project {
                what.push("the project".to_string());
            }
            if let Some(f) = &w.file {
                what.push(format!("file {f}"));
            }
            if w.no_file {
                what.push("no file in focus".into());
            }
            if let Some(l) = w.line {
                what.push(format!("line {l}"));
            }
            if let Some(d) = w.diagnostics {
                what.push(format!("{d}+ problem(s)"));
            }
            if w.idle {
                what.push("idle".into());
            }
            s += &format!("     wait      {} (at most {} s)\n", what.join(", "), w.timeout_s.unwrap_or(TIMEOUT_S));
        }
        s += &format!("     settle    {} ms, then capture\n", shot.settle_ms.unwrap_or(SETTLE_MS));
    }
    s
}

/// `cargo xtask screenshots ...`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let known = known_commands(root)?;
    let samples = root.join(SAMPLES);
    let all = scripted(root);
    let targets: Vec<(String, PathBuf)> = match &o.extension {
        Some(name) => {
            let dir = root.join("extensions").join(name);
            if !dir.join("extension.toml").is_file() {
                return Err(format!("no extensions/{name}"));
            }
            if !dir.join(SCRIPT).is_file() {
                return Err(format!("extensions/{name} has no {SCRIPT}"));
            }
            vec![(name.clone(), dir)]
        }
        None => all,
    };
    let mut scripts = Vec::new();
    for (name, dir) in targets {
        let script = check(&dir, &samples, &known)?;
        scripts.push((name, dir, script));
    }
    if o.dry_run {
        for (name, _, script) in &scripts {
            print!("{}", plan(name, script, &o));
        }
        println!("{} extension(s), {} shot(s); nothing started (--dry-run)", scripts.len(), scripts.iter().map(|s| s.2.shots.len()).sum::<usize>());
        return Ok(());
    }
    for (name, dir, script) in &scripts {
        take(root, name, dir, script, &o)?;
    }
    Ok(())
}

/// A Fenix this started, stopped when dropped. The control channel has
/// no way to quit (`app.quit` isn't external), so it's killed: its
/// profile is thrown away anyway.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct RealHost {
    start: Instant,
    pid: u32,
    work: PathBuf,
}

impl Host for RealHost {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
    fn capture(&mut self, out: &Path) -> Result<(), String> {
        capture_window(self.pid, out, &self.work)
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// The profile's settings: the theme, nothing moving, no splash -- and
/// there being a `settings.toml` at all keeps the first run's welcome away.
fn settings(theme: &str) -> String {
    format!("[editor]\ntheme = \"{theme}\"\n\n[motion]\nlevel = \"off\"\nsplash = false\n")
}

/// Where the window opens, and how big inside: Fenix restores it from
/// `state/windows.json`.
fn windows_json(size: (u32, u32)) -> String {
    json!({ "windows": [{ "x": 40, "y": 40, "width": size.0, "height": size.1, "maximized": false }] }).to_string()
}

fn take(root: &Path, name: &str, dir: &Path, script: &Script, o: &Options) -> Result<(), String> {
    let work = root.join("target/screenshots").join(name);
    let _ = std::fs::remove_dir_all(&work);
    let dist = work.join("dist");
    crate::pack(dir, &dist)?;
    let m = crate::manifest(dir)?;
    let package = dist.join(format!("{}-{}.fenixext", m.extension.id, m.extension.version));
    let shots = dir.join("screenshots");
    std::fs::create_dir_all(&shots).map_err(|e| format!("{}: {e}", shots.display()))?;
    for (i, shot) in script.shots.iter().enumerate() {
        // A Fenix of its own for each picture: nothing left over from the last.
        let home = work.join(format!("home-{}", i + 1));
        std::fs::create_dir_all(home.join("state")).map_err(|e| format!("{}: {e}", home.display()))?;
        std::fs::write(home.join("settings.toml"), settings(&o.theme)).map_err(|e| e.to_string())?;
        std::fs::write(home.join("state/windows.json"), windows_json(o.size)).map_err(|e| e.to_string())?;
        let samples = work.join(format!("samples-{}", i + 1));
        if let Some(p) = &shot.project {
            // A copy: Fenix may write into a project it opens.
            copy_dir(&root.join(SAMPLES).join(p), &samples.join(p))?;
        }
        let fenix = |args: &[&OsStr]| {
            let mut c = std::process::Command::new(&o.fenix);
            c.args(args).env("FENIX_HOME", &home).env_remove("FENIX_STANDALONE").env_remove("FENIX_MANAGED_EXTENSIONS").current_dir(&work);
            if cfg!(unix) && std::env::var_os("DISPLAY").is_some() {
                // Under X, where a window can be found by its process.
                c.env_remove("WAYLAND_DISPLAY");
            }
            c
        };
        let installed = fenix(&[OsStr::new("--install-extension"), package.as_os_str()]).status().map_err(|e| format!("{}: {e}", o.fenix.display()))?;
        if !installed.success() {
            return Err(format!("{name}: fenix --install-extension {} failed ({installed})", package.display()));
        }
        let log = work.join(format!("fenix-{}.log", i + 1));
        let log_file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let mut command = fenix(&[]);
        command.stdin(std::process::Stdio::null()).stdout(log_file.try_clone().map_err(|e| e.to_string())?).stderr(log_file);
        let mut running = Running(command.spawn().map_err(|e| format!("{}: {e}", o.fenix.display()))?);
        let pid = running.0.id();
        let endpoint = endpoint(home.as_os_str())?;
        let started = Instant::now();
        let transport = loop {
            if let Some(status) = running.0.try_wait().map_err(|e| e.to_string())? {
                return Err(format!("{name}: Fenix exited ({status}) before it listened -- {} says why", log.display()));
            }
            match connect(&endpoint) {
                Ok(t) => break t,
                Err(_) if started.elapsed() < LAUNCH_TIMEOUT => std::thread::sleep(Duration::from_millis(250)),
                Err(e) => return Err(format!("{name}: no control channel at {} after {} s: {e}", endpoint.display(), LAUNCH_TIMEOUT.as_secs())),
            }
        };
        let mut client = Client::new(transport);
        let hello = client.hello().map_err(|e| format!("{name}: {e}"))?;
        // A careful client checks the server is the one it started.
        if hello.pid.is_some_and(|p| p != pid) {
            return Err(format!("{name}: the Fenix at {} is process {}, not the {pid} started here", endpoint.display(), hello.pid.unwrap_or_default()));
        }
        let can = |m: &str| hello.capabilities.iter().any(|c| c == m);
        for (needed, by) in [("fenix/openProject", shot.project.is_some()), ("fenix/open", !shot.open.is_empty()), ("fenix/runCommand", !shot.commands.is_empty()), ("fenix/state", shot.wait.is_some()), ("fenix/diagnostics", shot.wait.as_ref().is_some_and(|w| w.diagnostics.is_some()))] {
            if by && !can(needed) {
                return Err(format!("{name}: this Fenix ({}) can't {needed}", hello.version.as_deref().unwrap_or("?")));
            }
        }
        let commands = if can("fenix/commands") && !shot.commands.is_empty() { Some(client.commands().map_err(|e| format!("{name}: {e}"))?) } else { None };
        let out = shots.join(&shot.file);
        let mut host = RealHost { start: Instant::now(), pid, work: work.clone() };
        run_shot(&mut client, &mut host, shot, &samples, commands.as_ref(), &out).map_err(|e| format!("{name}: {e}"))?;
        drop(running);
        let size = std::fs::metadata(&out).map_err(|e| e.to_string())?.len();
        if size > crate::MAX_SCREENSHOT {
            return Err(format!("{}: {} KB, and a screenshot is at most {} MB", out.display(), size / 1024, crate::MAX_SCREENSHOT / 1024 / 1024));
        }
        println!("{}  {} KB", out.display(), size / 1024);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    type Handler = Box<dyn FnMut(&str, &Value) -> Result<Value, (i64, String)>>;
    type Calls = Rc<RefCell<Vec<(String, Value)>>>;

    /// An in-process Fenix: takes framed requests as they're written,
    /// answers them through `handler`, and hands the answers back a few
    /// bytes at a time. It records every call.
    struct FakeFenix {
        inbox: Vec<u8>,
        outbox: VecDeque<u8>,
        chunk: usize,
        greeted: bool,
        handler: Handler,
        calls: Calls,
    }

    impl FakeFenix {
        fn new(handler: impl FnMut(&str, &Value) -> Result<Value, (i64, String)> + 'static) -> (FakeFenix, Calls) {
            let calls = Rc::new(RefCell::new(Vec::new()));
            (FakeFenix { inbox: Vec::new(), outbox: VecDeque::new(), chunk: 7, greeted: false, handler: Box::new(handler), calls: calls.clone() }, calls)
        }

        /// The framing as CONTROL.md has it, read independently of `read_frame`.
        fn take_request(&mut self) -> Option<Value> {
            let end = self.inbox.windows(4).position(|w| w == b"\r\n\r\n")?;
            let head = String::from_utf8(self.inbox[..end].to_vec()).unwrap();
            let length: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).expect("Content-Length").parse().unwrap();
            if self.inbox.len() < end + 4 + length {
                return None;
            }
            let body = self.inbox[end + 4..end + 4 + length].to_vec();
            self.inbox.drain(..end + 4 + length);
            Some(serde_json::from_slice(&body).unwrap())
        }

        fn answer(&mut self, reply: Value) {
            let body = serde_json::to_vec(&reply).unwrap();
            // A header Fenix might add, to be passed over.
            self.outbox.extend(format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
            self.outbox.extend(body);
        }
    }

    impl Read for FakeFenix {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.outbox.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the fake has nothing to say"));
            }
            let n = buf.len().min(self.chunk).min(self.outbox.len());
            for b in buf.iter_mut().take(n) {
                *b = self.outbox.pop_front().unwrap();
            }
            Ok(n)
        }
    }

    impl Write for FakeFenix {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inbox.extend_from_slice(buf);
            while let Some(request) = self.take_request() {
                assert_eq!(request["jsonrpc"], "2.0");
                let method = request["method"].as_str().unwrap().to_string();
                let params = request["params"].clone();
                self.calls.borrow_mut().push((method.clone(), params.clone()));
                let result = if method == "fenix/hello" {
                    self.greeted = true;
                    (self.handler)(&method, &params)
                } else if !self.greeted {
                    Err((-32002, "send fenix/hello first".into()))
                } else {
                    (self.handler)(&method, &params)
                };
                if let Some(id) = request.get("id") {
                    let reply = match result {
                        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
                        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
                    };
                    self.answer(reply);
                }
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn hello() -> Value {
        json!({ "protocol": 1, "version": "1.5.0", "pid": 4242, "capabilities": ["fenix/open", "fenix/openProject", "fenix/state", "fenix/commands", "fenix/runCommand", "fenix/diagnostics"] })
    }

    /// Time that passes only when slept.
    #[derive(Default)]
    struct FakeHost {
        now: Duration,
        sleeps: Vec<Duration>,
        captures: Vec<PathBuf>,
    }

    impl Host for FakeHost {
        fn elapsed(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, d: Duration) {
            self.now += d;
            self.sleeps.push(d);
        }
        fn capture(&mut self, out: &Path) -> Result<(), String> {
            self.captures.push(out.to_path_buf());
            Ok(())
        }
    }

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("xtask-shots-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_endpoint_suffix_is_fnv1a_of_fenix_home() {
        assert_eq!(home_hash(OsStr::new("foobar")), "85944171f73967e8", "CONTROL.md's example");
        assert_eq!(home_hash(OsStr::new("")), "cbf29ce484222325");
    }

    #[test]
    fn frames_are_written_and_read_as_control_md_has_them() {
        let mut out = Vec::new();
        write_frame(&mut out, br#"{"a":1}"#).unwrap();
        assert_eq!(out, b"Content-Length: 7\r\n\r\n{\"a\":1}");
        let mut two = b"X-Other: y\r\ncontent-length: 2\r\n\r\n{}Content-Length: 3\n\n[1]".as_slice();
        assert_eq!(read_frame(&mut two).unwrap(), b"{}");
        assert_eq!(read_frame(&mut two).unwrap(), b"[1]", "bare newlines are taken too");
        assert!(read_frame(&mut b"X: y\r\n\r\n{}".as_slice()).unwrap_err().to_string().contains("without Content-Length"));
        assert!(read_frame(&mut b"Content-Length: 9\r\n\r\n{}".as_slice()).is_err(), "cut short");
    }

    #[test]
    fn the_client_says_hello_and_reads_answers_split_up() {
        let (fake, calls) = FakeFenix::new(|method, _| match method {
            "fenix/hello" => Ok(hello()),
            "fenix/commands" => Ok(json!({ "commands": [{ "id": "agenda.open", "external": true }, { "id": "file.save", "external": false }, { "id": "x.y" }] })),
            _ => Err((-32601, "no such method".into())),
        });
        let mut client = Client::new(fake);
        let h = client.hello().unwrap();
        assert_eq!((h.pid, h.version.as_deref()), (Some(4242), Some("1.5.0")));
        assert!(h.capabilities.contains(&"fenix/runCommand".to_string()));
        let commands = client.commands().unwrap();
        assert_eq!(commands.get("agenda.open"), Some(&true));
        assert_eq!(commands.get("file.save"), Some(&false));
        assert_eq!(commands.get("x.y"), Some(&false), "external is false unless said");
        assert_eq!(client.call("fenix/nothing", json!({})).unwrap_err(), "fenix/nothing: -32601 no such method");
        let calls = calls.borrow();
        assert_eq!(calls[0].0, "fenix/hello");
        assert_eq!(calls[0].1["protocol"], 1);
        assert_eq!(calls[0].1["client"]["name"], "xtask-screenshots");
    }

    #[test]
    fn errors_come_back_with_their_code() {
        let (fake, _) = FakeFenix::new(|method, _| match method {
            "fenix/hello" => Err((-32001, "unsupported protocol".into())),
            _ => Ok(json!({})),
        });
        let mut client = Client::new(fake);
        assert!(client.hello().unwrap_err().contains("-32001"));
        let (fake, _) = FakeFenix::new(|_, _| Ok(json!({})));
        let mut client = Client::new(fake);
        assert!(client.call("fenix/state", json!({})).unwrap_err().contains("-32002"), "hello comes first");
        let (fake, _) = FakeFenix::new(|_, _| Ok(json!({ "protocol": 2 })));
        assert!(Client::new(fake).hello().unwrap_err().contains("protocol 2"));
    }

    /// A sample project and a shot of it.
    fn sample(t: &Temp) -> PathBuf {
        let samples = t.0.join("samples");
        write(&samples.join("csv/README.md"), "# A sample");
        write(&samples.join("csv/data/people.csv"), "name,age\nada,36\n");
        samples
    }

    fn shot(text: &str) -> Shot {
        let script: Script = toml::from_str(&format!("[[shot]]\n{text}")).unwrap();
        script.shots[0].clone()
    }

    #[test]
    fn a_shot_runs_its_steps_in_order_then_waits_settles_and_captures() {
        let t = Temp::new("order");
        let samples = sample(&t);
        let file = samples.join("csv/data/people.csv");
        let reads = Rc::new(RefCell::new(0u64));
        let r = reads.clone();
        let wanted = plain(&file);
        let (fake, calls) = FakeFenix::new(move |method, params| match method {
            "fenix/hello" => Ok(hello()),
            "fenix/openProject" | "fenix/open" | "fenix/runCommand" => Ok(json!({})),
            "fenix/state" => {
                *r.borrow_mut() += 1;
                let n = *r.borrow();
                // The file only on the second read; frames settle after the third.
                let file = if n >= 2 { json!(wanted) } else { Value::Null };
                let frames = [10, 40, 90, 91, 92][(n as usize - 1).min(4)];
                Ok(json!({ "project": "ignored", "file": file, "line": 3, "stats": { "frames": frames } }))
            }
            "fenix/diagnostics" => Ok(json!({ "diagnostics": [{ "path": params["path"], "line": 1 }] })),
            _ => Err((-32601, "no".into())),
        });
        let mut client = Client::new(fake);
        client.hello().unwrap();
        let s = shot("file = \"table.png\"\nproject = \"csv\"\nopen = [\"data/people.csv:3\"]\ncommands = [\"agenda.open\"]\nsettle_ms = 250\n[shot.wait]\nfile = \"data/people.csv\"\nline = 3\nidle = true\ndiagnostics = 1\n");
        let mut host = FakeHost::default();
        let commands: BTreeMap<String, bool> = [("agenda.open".to_string(), true)].into();
        run_shot(&mut client, &mut host, &s, &samples, Some(&commands), Path::new("out/table.png")).unwrap();
        let calls = calls.borrow();
        let methods: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(&methods[..4], ["fenix/hello", "fenix/openProject", "fenix/open", "fenix/runCommand"]);
        assert!(methods[4..].iter().all(|m| *m == "fenix/state" || *m == "fenix/diagnostics"), "{methods:?}");
        assert_eq!(calls[1].1["root"], plain(&samples.join("csv")));
        assert_eq!(calls[2].1, json!({ "paths": [{ "path": plain(&file), "line": 3 }], "focus": true }));
        assert_eq!(calls[3].1, json!({ "id": "agenda.open" }));
        assert_eq!(*reads.borrow(), 4, "until the file is there and two reads a poll apart drew at most {IDLE_FRAMES}");
        assert_eq!(host.sleeps.last(), Some(&Duration::from_millis(250)), "settled last");
        assert_eq!(host.captures, [PathBuf::from("out/table.png")]);
    }

    #[test]
    fn a_wait_that_never_holds_times_out_saying_what_it_waited_for() {
        let t = Temp::new("timeout");
        let samples = sample(&t);
        let (fake, _) = FakeFenix::new(|method, _| match method {
            "fenix/hello" => Ok(hello()),
            "fenix/state" => Ok(json!({ "project": "/elsewhere" })),
            _ => Ok(json!({})),
        });
        let mut client = Client::new(fake);
        client.hello().unwrap();
        let s = shot("file = \"x.png\"\nproject = \"csv\"\n[shot.wait]\nproject = true\nno_file = true\ntimeout_s = 3\n");
        let mut host = FakeHost::default();
        let err = run_shot(&mut client, &mut host, &s, &samples, None, Path::new("x.png")).unwrap_err();
        assert!(err.contains("waited 3 s for project"), "{err}");
        assert!(err.contains("/elsewhere"), "says what Fenix said: {err}");
        assert!(host.captures.is_empty());
        assert_eq!(host.now, Duration::from_secs(3));
    }

    #[test]
    fn a_command_fenix_wont_run_is_refused_before_anything_runs() {
        let t = Temp::new("refused");
        let samples = sample(&t);
        let (fake, calls) = FakeFenix::new(|method, _| match method {
            "fenix/hello" => Ok(hello()),
            "fenix/runCommand" => Err((-32602, "regex.open acts on what's in front of you in Fenix: not from outside".into())),
            _ => Ok(json!({})),
        });
        let mut client = Client::new(fake);
        client.hello().unwrap();
        let s = shot("file = \"x.png\"\nproject = \"csv\"\ncommands = [\"regex.open\"]\n");
        let mut host = FakeHost::default();
        let listed: BTreeMap<String, bool> = [("regex.open".to_string(), false)].into();
        assert!(run_shot(&mut client, &mut host, &s, &samples, Some(&listed), Path::new("x.png")).unwrap_err().contains("isn't external"));
        assert!(run_shot(&mut client, &mut host, &s, &samples, Some(&BTreeMap::new()), Path::new("x.png")).unwrap_err().contains("no command regex.open"));
        assert_eq!(calls.borrow().len(), 1, "only the hello");
        // Without a list to check, Fenix's own refusal comes through.
        let err = run_shot(&mut client, &mut host, &s, &samples, None, Path::new("x.png")).unwrap_err();
        assert!(err.contains("-32602") && err.contains("not from outside"), "{err}");
        assert!(host.captures.is_empty());
    }

    #[test]
    fn idle_needs_stats() {
        let t = Temp::new("idle");
        let samples = sample(&t);
        let (fake, _) = FakeFenix::new(|method, _| match method {
            "fenix/hello" => Ok(hello()),
            _ => Ok(json!({})),
        });
        let mut client = Client::new(fake);
        client.hello().unwrap();
        let s = shot("file = \"x.png\"\nproject = \"csv\"\n[shot.wait]\nidle = true\n");
        assert!(run_shot(&mut client, &mut FakeHost::default(), &s, &samples, None, Path::new("x.png")).unwrap_err().contains("no stats.frames"));
    }

    #[test]
    fn positions_are_read_off_the_end() {
        assert_eq!(opening("a/b.csv"), Opening { path: "a/b.csv".into(), line: None, column: None });
        assert_eq!(opening("src/main.rs:12"), Opening { path: "src/main.rs".into(), line: Some(12), column: None });
        assert_eq!(opening("src/main.rs:12:3"), Opening { path: "src/main.rs".into(), line: Some(12), column: Some(3) });
        assert_eq!(opening("odd:name.txt"), Opening { path: "odd:name.txt".into(), line: None, column: None });
    }

    /// A repository of two extensions and a sample, for `check`.
    fn repo(t: &Temp) -> (PathBuf, Known) {
        let root = t.0.join("repo");
        write(&root.join("extensions/csv/extension.toml"), "[extension]\nid = \"acme.csv\"\n\n[[contributes.commands]]\nid = \"csv.open\"\ntitle = \"x\"\n\n[[contributes.commands]]\nid = \"csv.page\"\ntitle = \"x\"\nexternal = true\n");
        write(&root.join("extensions/mib/extension.toml"), "[extension]\nid = \"acme.mib\"\n\n[contributes]\nnamespaces = [\"spacecraft\"]\n");
        write(&root.join("screenshots/samples/csv/README.md"), "# Sample");
        write(&root.join("screenshots/samples/csv/data/people.csv"), "a,b\n");
        write(&root.join("screenshots/samples/bare/x.txt"), "x");
        let known = known_commands(&root).unwrap();
        (root, known)
    }

    fn checked(root: &Path, known: &Known, script: &str) -> Result<Script, String> {
        let dir = root.join("extensions/csv");
        std::fs::write(dir.join(SCRIPT), script).unwrap();
        check(&dir, &root.join(SAMPLES), known)
    }

    #[test]
    fn a_script_is_checked_offline() {
        let t = Temp::new("check");
        let (root, known) = repo(&t);
        let good = "[[shot]]\nfile = \"table.png\"\nnote = \"a table\"\nproject = \"csv\"\nopen = [\"data/people.csv:2\"]\ncommands = [\"csv.page\", \"dashboard.open\"]\nsettle_ms = 500\n[shot.wait]\nfile = \"data/people.csv\"\nidle = true\ntimeout_s = 20\n\n[[shot]]\nfile = \"second.png\"\nproject = \"csv\"\n";
        assert_eq!(checked(&root, &known, good).unwrap().shots.len(), 2);
        let shot = |rest: &str| format!("[[shot]]\nfile = \"a.png\"\nproject = \"csv\"\n{rest}\n");
        for (bad, says) in [
            ("[[shot]]\nfile = \"a.png\"\nproject = \"csv\"\nzoom = 2\n".to_string(), "unknown field"),
            ("".into(), "no [[shot]]"),
            (shot("").repeat(4), "at most 3"),
            (shot("") + &shot(""), "there twice"),
            ("[[shot]]\nfile = \"Table.PNG\"\nproject = \"csv\"\n".into(), "ending .png"),
            ("[[shot]]\nfile = \"../a.png\"\nproject = \"csv\"\n".into(), "ending .png"),
            ("[[shot]]\nfile = \"a.jpg\"\nproject = \"csv\"\n".into(), "ending .png"),
            ("[[shot]]\nfile = \"a.png\"\n".into(), "opens nothing"),
            ("[[shot]]\nfile = \"a.png\"\nproject = \"nope\"\n".into(), "isn't there"),
            ("[[shot]]\nfile = \"a.png\"\nproject = \"../csv\"\n".into(), "a folder under"),
            ("[[shot]]\nfile = \"a.png\"\nproject = \"bare\"\n".into(), "no README.md"),
            ("[[shot]]\nfile = \"a.png\"\nopen = [\"data/people.csv\"]\n".into(), "it has none"),
            (shot("open = [\"data/nope.csv\"]"), "no such file"),
            (shot("open = [\"data/people.csv:0\"]"), "count from 1"),
            (shot("commands = [\"csv.open\"]"), "isn't external = true"),
            (shot("commands = [\"csv.nothing\"]"), "csv has no such command"),
            (shot("commands = [\"spacecraft.go\"]"), "mib has no such command"),
            (shot("commands = [\"Open\"]"), "isn't an id"),
            (shot("[shot.wait]\nfile = \"data/people.csv\"\nno_file = true"), "and for no file"),
            (shot("[shot.wait]\nfile = \"data/gone.csv\""), "no such file in the project"),
            (shot("[shot.wait]\ndiagnostics = 1"), "give wait.file"),
            (shot("[shot.wait]\nidle = true\ntimeout_s = 600"), "from 1 to 120"),
            (shot("[shot.wait]\ntimeout_s = 5"), "waits for nothing"),
            (shot("settle_ms = 60000"), "at most 10000"),
        ] {
            let err = checked(&root, &known, &bad).unwrap_err();
            assert!(err.contains(says), "{bad}\n-> {err}");
        }
    }

    #[test]
    fn the_scripts_here_hold() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        validate_all(root).unwrap();
        // And the plan prints for each.
        let o = options(&["--dry-run".to_string()]).unwrap();
        let known = known_commands(root).unwrap();
        for (name, dir) in scripted(root) {
            let script = check(&dir, &root.join(SAMPLES), &known).unwrap();
            assert!(plan(&name, &script, &o).contains("(the hero)"));
        }
    }

    #[test]
    fn options_are_read() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = options(&args(&["csv", "--fenix", "C:/fenix/fenix.exe", "--theme=Nord", "--size", "1280x800", "--dry-run"])).unwrap();
        assert_eq!((o.extension.as_deref(), o.fenix, o.theme.as_str(), o.size, o.dry_run), (Some("csv"), PathBuf::from("C:/fenix/fenix.exe"), "Nord", (1280, 800), true));
        let o = options(&[]).unwrap();
        assert_eq!((o.extension, o.theme.as_str(), o.size, o.dry_run), (None, THEME, SIZE, false));
        assert!(options(&args(&["--size", "big"])).unwrap_err().contains("WxH"));
        assert!(options(&args(&["--size", "100x100"])).unwrap_err().contains("from 640x480"));
        assert!(options(&args(&["--zoom"])).unwrap_err().contains("--zoom?"));
        assert!(options(&args(&["a", "b"])).unwrap_err().contains("one extension"));
        assert!(options(&args(&["--theme"])).unwrap_err().contains("needs a value"));
        assert!(options(&args(&["--theme", "a\"b"])).is_err());
    }

    #[test]
    fn the_profile_has_the_theme_and_the_window() {
        let s: toml::Table = toml::from_str(&settings("Nord")).unwrap();
        assert_eq!(s["editor"]["theme"].as_str(), Some("Nord"));
        assert_eq!(s["motion"]["level"].as_str(), Some("off"));
        let w: Value = serde_json::from_str(&windows_json((1600, 1000))).unwrap();
        assert_eq!(w["windows"][0], json!({ "x": 40, "y": 40, "width": 1600, "height": 1000, "maximized": false }));
        assert!(CAPTURE_PS1.contains("PrintWindow($h, $hdc, 2)"));
    }

    #[test]
    fn the_script_isnt_packed() {
        let t = Temp::new("pack");
        let dir = t.0.join("extensions/notes");
        write(&dir.join("extension.toml"), "[extension]\nid = \"acme.notes\"\nname = \"Notes\"\nversion = \"1.0.0\"\nruntime = \"none\"\n\n[engine]\nfenix = \">=1.0, <2\"\n");
        write(&dir.join("README.md"), "# Notes");
        write(&dir.join(SCRIPT), "[[shot]]\nfile = \"a.png\"\nproject = \"x\"\n");
        write(&dir.join("screenshots/a.png"), "png");
        crate::pack(&dir, &t.0.join("dist")).unwrap();
        let file = std::fs::File::open(t.0.join("dist/acme.notes-1.0.0.fenixext")).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(names, ["README.md", "extension.toml", "screenshots/a.png"]);
        assert!(is_script(Path::new(SCRIPT)) && !is_script(Path::new("screenshots/screenshots.toml")));
    }
}
