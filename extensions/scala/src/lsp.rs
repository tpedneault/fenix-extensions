//! Metals, run by the pack: the pack is Scala's language server to Fenix
//! (`contributes.language_servers`), and passes each message on to the
//! Metals it starts, and each of Metals' back -- so it can do what Fenix
//! has no way to:
//!
//! - tell Metals who its client is (`initialize`'s options: no HTTP server
//!   for its doctor, no status bar, a debugger) and what its user
//!   configuration says (`workspace/configuration`: import a build
//!   without asking);
//! - answer the questions Metals asks with `window/showMessageRequest`,
//!   which Fenix has no prompt for, so it would be left with no answer:
//!   import the build, format with scalafmt's defaults when there's no
//!   `.scalafmt.conf`, not start the HTTP server;
//! - ask Metals things of its own: `debug-adapter-start`, which starts its
//!   debug adapter on a port of its choice and says which, so Fenix can
//!   connect to it (`debug-adapter-connect`).
//!
//! A state machine: what comes from Fenix or from Metals goes in, and what
//! to do comes out ([`Out`]), which the extension does. Written against
//! what Metals 1.6.9 said (`src/samples/metals-stdout.bin`).

use std::collections::HashMap;

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::{self, json, Value};

/// The prefix of the pack's own requests' ids: Metals' answers to them stay
/// here.
const OWN: &str = "fenix.scala/";

/// What `showMessageRequest` is answered with, by the titles it offers
/// (the first row that has one of them) -- each so Metals goes on where Fenix
/// can't ask:
///
/// - *Import build*: a build it found, imported. (With
///   `autoImportBuilds` it doesn't ask; this is for the build it
///   couldn't import before.)
/// - *Import changes*: the build changed, imported again.
/// - *Run anyway*: formatting with no `.scalafmt.conf`, with scalafmt's
///   defaults (*Create .scalafmt.conf* would write a file into the project).
/// - *Not now*, to "Http server is required for such features as Metals
///   Doctor": it's off, and stays so.
pub const ANSWERS: &[(&str, Option<&str>)] = &[("Import build", None), ("Import changes", None), ("Run anyway", None), ("Not now", Some("Http server is required"))];

/// What Metals is started with, and told.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// Metals' own launcher.
    pub command: Command,
    /// `autoImportBuilds`: `all`, `initial` or `off`.
    pub auto_import: String,
    /// The JDK chosen (`scala.jdk`): Metals' `javaHome`, and `JAVA_HOME` for
    /// the launcher.
    pub jdk_home: Option<String>,
}

impl Launch {
    /// The command, with the chosen JDK as `JAVA_HOME`.
    pub fn command(&self) -> Command {
        let mut cmd = self.command.clone();
        if let Some(home) = &self.jdk_home {
            cmd.env.retain(|(k, _)| k != "JAVA_HOME");
            cmd.env.push(("JAVA_HOME".into(), home.clone()));
        }
        cmd
    }

    /// `initialize`'s params, with what Metals is told besides, where Fenix
    /// said nothing: it has no HTTP server (its doctor is the project
    /// doctor's here), no status bar, and a debugger.
    pub fn initialize(&self, mut params: Value) -> Value {
        let options = json!({ "isHttpEnabled": false, "statusBarProvider": "off", "debuggingProvider": true, "isExitOnShutdown": true });
        if let Some(p) = params.as_object_mut() {
            let mut given = p.get("initializationOptions").and_then(Value::as_object).cloned().unwrap_or_default();
            for (k, v) in options.as_object().into_iter().flatten() {
                given.entry(k.clone()).or_insert_with(|| v.clone());
            }
            p.insert("initializationOptions".into(), Value::Object(given));
        }
        params
    }

    /// Metals' user configuration, `workspace/configuration`'s `metals`
    /// section.
    pub fn configuration(&self) -> Value {
        let mut config = json!({ "autoImportBuilds": self.auto_import });
        if let Some(home) = &self.jdk_home {
            config["javaHome"] = json!(home);
        }
        config
    }
}

/// The project's full path in an `initialize` message, when it is one.
pub fn initialize_root(message: &str) -> Option<String> {
    let msg: Value = serde_json::from_str(message).ok()?;
    if msg.get("method").and_then(Value::as_str) != Some("initialize") {
        return None;
    }
    msg.pointer("/params/rootUri").and_then(Value::as_str).map(uri_path)
}

/// One message framed as LSP's stdio has it.
pub fn frame(message: &str) -> Vec<u8> {
    let mut out = format!("Content-Length: {}\r\n\r\n", message.len()).into_bytes();
    out.extend_from_slice(message.as_bytes());
    out
}

/// The messages in `buf`, taken off it; a part one stays.
pub fn unframe(buf: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
        let header = String::from_utf8_lossy(&buf[..end]).to_string();
        let Some(len) = header.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok())) else {
            // Not a header: what's before the next one is dropped.
            buf.drain(..end + 4);
            continue;
        };
        let start = end + 4;
        if buf.len() < start + len {
            break;
        }
        out.push(String::from_utf8_lossy(&buf[start..start + len]).into_owned());
        buf.drain(..start + len);
    }
    out
}

/// A `file://` URI as a path: `file:///home/u/x` is `/home/u/x`,
/// `file:///c%3A/x` is `c:/x`.
pub fn uri_path(uri: &str) -> String {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        let hex = (b == b'%').then(|| tail.get(..2)).flatten().and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(decoded) => {
                bytes.push(decoded);
                rest = &tail[2..];
            }
            None => {
                bytes.push(b);
                rest = tail;
            }
        }
    }
    let out = String::from_utf8_lossy(&bytes).into_owned();
    // `/c:/x` on Windows.
    match out.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => out[1..].to_string(),
        _ => out,
    }
}

/// A path as a `file://` URI, the other way: spaces and the like
/// percent-encoded, a Windows drive's path (`C:/x`) as `file:///C:/x`.
pub fn path_uri(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn error(id: &Value, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": message } }).to_string()
}

fn result(id: &Value, result: Value) -> Vec<u8> {
    frame(&json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

/// What the pack asked Metals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `debug-adapter-start` for a file (a full path): `run_type` is
    /// `run`, `runOrTestFile` or `testFile`.
    Debug { file: String, run_type: String },
    /// `build-import`: the build read again, whatever `autoImportBuilds` says.
    Import,
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// Start Metals, its stdout as bytes.
    Spawn(Command),
    /// Stop the Metals that's running: Fenix started the server again.
    Kill(u64),
    /// Bytes for Metals' stdin.
    Write(Vec<u8>),
    /// A message for Fenix, from server `server`.
    Send(u64, String),
    /// The answer to one of the pack's own requests.
    Answer(Ask, Result<Value, String>),
}

#[derive(Debug, Default)]
pub struct Proxy {
    /// Fenix's id for the server.
    pub server: Option<u64>,
    /// The Metals running, by its process id.
    pub process: Option<u64>,
    /// The project's full path, from `initialize`.
    pub root: Option<String>,
    /// Why there's no Metals, when there isn't one.
    pub missing: Option<String>,
    /// How Metals is configured, as the last start said.
    config: Value,
    /// Fenix's `initialize`, until Metals answers it.
    init: Option<Value>,
    buf: Vec<u8>,
    next: u64,
    asked: HashMap<String, Ask>,
}

impl Proxy {
    /// A message from Fenix for server `server`. `launch` is how to start
    /// Metals, or why it can't be.
    pub fn from_fenix(&mut self, server: u64, message: &str, launch: Result<&Launch, &str>) -> Vec<Out> {
        self.server = Some(server);
        let Ok(mut msg) = serde_json::from_str::<Value>(message) else { return Vec::new() };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let id = msg.get("id").cloned();
        let mut out = Vec::new();
        if method == "initialize" {
            let root = msg.pointer("/params/rootUri").and_then(Value::as_str).map(uri_path);
            self.root = root;
            self.init = id.clone();
            if let Some(old) = self.process {
                out.push(Out::Kill(old));
                self.process = None;
            }
            match launch {
                Ok(l) => {
                    self.missing = None;
                    self.config = l.configuration();
                    out.push(Out::Spawn(l.command()));
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    msg["params"] = l.initialize(params);
                }
                Err(why) => {
                    // Fenix is answered, so it doesn't wait; the server
                    // starts again once there's a Metals.
                    self.missing = Some(why.to_string());
                    let caps = json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {}, "serverInfo": { "name": "Metals (not running)" } } });
                    out.push(Out::Send(server, caps.to_string()));
                    self.init = None;
                    return out;
                }
            }
        }
        if self.missing.is_some() && self.process.is_none() {
            if let (Some(id), false) = (&id, method.is_empty()) {
                // What's missing now, which a fix since may have changed.
                let why = match launch {
                    Err(why) => why.to_string(),
                    Ok(_) => self.missing.clone().unwrap_or_default(),
                };
                out.push(Out::Send(server, error(id, &why)));
            }
            return out;
        }
        out.push(Out::Write(frame(&msg.to_string())));
        out
    }

    /// Metals couldn't be started: Fenix, which is waiting for its `initialize`
    /// to be answered, is told why, and what it asks after is answered the same.
    pub fn failed(&mut self, why: &str) -> Vec<Out> {
        self.missing = Some(why.to_string());
        match (self.server, self.init.take()) {
            (Some(server), Some(id)) => vec![Out::Send(server, error(&id, why))],
            _ => Vec::new(),
        }
    }

    /// Metals started, as process `process`.
    pub fn started(&mut self, process: u64) {
        self.process = Some(process);
        self.buf.clear();
    }

    /// Bytes from Metals' stdout.
    pub fn from_metals(&mut self, bytes: &[u8]) -> Vec<Out> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        for text in unframe(&mut self.buf) {
            let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
            let method = msg.get("method").and_then(Value::as_str);
            if method.is_none() && msg.get("id").is_some() && msg.get("id") == self.init.as_ref() {
                self.init = None;
            }
            // The answer to one of the pack's own requests.
            let own = method.is_none().then(|| msg.get("id").and_then(Value::as_str)).flatten().filter(|id| id.starts_with(OWN)).map(str::to_string);
            if let Some(id) = own {
                if let Some(ask) = self.asked.remove(&id) {
                    let answer = match (msg.get("result"), msg.get("error")) {
                        (_, Some(e)) => Err(e.get("message").and_then(Value::as_str).unwrap_or("Metals said no").to_string()),
                        (Some(r), None) => Ok(r.clone()),
                        _ => Ok(Value::Null),
                    };
                    out.push(Out::Answer(ask, answer));
                }
                continue;
            }
            // Questions the pack answers itself, so Fenix isn't asked what
            // it has no way to answer.
            if let (Some(method), Some(id)) = (method, msg.get("id")) {
                if let Some(answer) = self.answer(method, &msg) {
                    out.push(Out::Write(result(id, answer)));
                    continue;
                }
            }
            if let Some(server) = self.server {
                out.push(Out::Send(server, text));
            }
        }
        out
    }

    /// The answer to a request of Metals' the pack answers itself, or none
    /// for Fenix to.
    fn answer(&self, method: &str, msg: &Value) -> Option<Value> {
        match method {
            "workspace/configuration" => {
                let items = msg.pointer("/params/items").and_then(Value::as_array)?;
                let sections: Vec<Value> = items.iter().map(|i| if i.get("section").and_then(Value::as_str) == Some("metals") { self.config.clone() } else { Value::Null }).collect();
                Some(Value::Array(sections))
            }
            "window/showMessageRequest" => {
                let actions: Vec<&str> = msg.pointer("/params/actions").and_then(Value::as_array)?.iter().filter_map(|a| a.get("title").and_then(Value::as_str)).collect();
                let text = msg.pointer("/params/message").and_then(Value::as_str).unwrap_or_default();
                ANSWERS.iter().find(|(title, about)| actions.contains(title) && about.is_none_or(|a| text.starts_with(a))).map(|(title, _)| json!({ "title": title }))
            }
            _ => None,
        }
    }

    /// One of the pack's own requests: `workspace/executeCommand`.
    pub fn ask(&mut self, ask: Ask) -> Result<Vec<Out>, String> {
        if self.process.is_none() {
            return Err(self.missing.clone().unwrap_or_else(|| "Metals isn't running: open a Scala file".into()));
        }
        self.next += 1;
        let id = format!("{OWN}{}", self.next);
        let msg = match &ask {
            Ask::Debug { file, run_type } => json!({
                "jsonrpc": "2.0", "id": id, "method": "workspace/executeCommand",
                "params": { "command": "debug-adapter-start", "arguments": [{ "path": path_uri(file), "runType": run_type }] },
            }),
            Ask::Import => json!({ "jsonrpc": "2.0", "id": id, "method": "workspace/executeCommand", "params": { "command": "build-import" } }),
        };
        self.asked.insert(id, ask);
        Ok(vec![Out::Write(frame(&msg.to_string()))])
    }

    /// Metals ended: what it was asked is answered no.
    pub fn exited(&mut self) -> Vec<Out> {
        self.process = None;
        self.buf.clear();
        self.asked.drain().map(|(_, ask)| Out::Answer(ask, Err("Metals stopped".into()))).collect()
    }
}

/// `debug-adapter-start`'s answer: the session's name, and the port its
/// adapter listens on (`tcp://127.0.0.1:37437`).
pub fn debug_session(answer: &Value) -> Option<(String, u16)> {
    let name = answer.get("name").and_then(Value::as_str).unwrap_or("Scala").to_string();
    let uri = answer.get("uri").and_then(Value::as_str)?;
    let port = uri.rsplit_once(':')?.1.trim_end_matches('/').parse().ok()?;
    Some((name, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::line;

    fn launch() -> Launch {
        Launch { command: Command::new("/tools/metals/metals"), auto_import: "all".into(), jdk_home: Some("/usr/lib/jvm/java-21".into()) }
    }

    fn writes(out: &[Out]) -> Vec<Value> {
        out.iter()
            .filter_map(|o| match o {
                Out::Write(b) => {
                    let mut b = b.clone();
                    unframe(&mut b).first().map(|m| serde_json::from_str(m).unwrap())
                }
                _ => None,
            })
            .collect()
    }

    fn sent(out: &[Out]) -> Vec<Value> {
        out.iter().filter_map(|o| if let Out::Send(_, t) = o { Some(serde_json::from_str(t).unwrap()) } else { None }).collect()
    }

    #[test]
    fn messages_are_framed_and_unframed_in_parts() {
        let mut buf = frame("{\"a\":1}");
        buf.extend(frame("{\"b\":\"é\"}"));
        let tail = buf.split_off(buf.len() - 3);
        assert_eq!(unframe(&mut buf), ["{\"a\":1}"]);
        buf.extend(tail);
        assert_eq!(unframe(&mut buf), ["{\"b\":\"é\"}"], "its length in bytes");
        assert!(buf.is_empty());
        let mut junk = b"Picked up JAVA_TOOL_OPTIONS\r\n\r\nContent-Length: 2\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}".to_vec();
        assert_eq!(unframe(&mut junk), ["{}"]);
    }

    #[test]
    fn paths_and_uris_go_both_ways() {
        assert_eq!(uri_path("file:///home/u/my%20orbit"), "/home/u/my orbit");
        assert_eq!(uri_path("file:///c%3A/orbit"), "c:/orbit");
        assert_eq!(path_uri("/home/u/my orbit/Main.scala"), "file:///home/u/my%20orbit/Main.scala");
        assert_eq!(path_uri("C:\\orbit\\Main.scala"), "file:///C:/orbit/Main.scala");
        assert_eq!(path_uri("/home/u/é.scala"), "file:///home/u/%C3%A9.scala");
        assert_eq!(uri_path(&path_uri("/home/u/my orbit/é.scala")), "/home/u/my orbit/é.scala");
    }

    #[test]
    fn metals_is_started_and_told_what_its_client_is() {
        let mut p = Proxy::default();
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":7,"rootUri":"file:///home/u/orbit","capabilities":{}}}"#;
        let out = p.from_fenix(4, init, Ok(&launch()));
        let Out::Spawn(cmd) = &out[0] else { panic!("{out:?}") };
        assert_eq!((line(cmd), cmd.env.clone()), ("/tools/metals/metals".to_string(), vec![("JAVA_HOME".to_string(), "/usr/lib/jvm/java-21".to_string())]));
        let sent = writes(&out);
        assert_eq!(sent[0]["params"]["initializationOptions"], json!({ "isHttpEnabled": false, "statusBarProvider": "off", "debuggingProvider": true, "isExitOnShutdown": true }));
        assert_eq!(sent[0]["params"]["processId"], 7, "the rest as Fenix sent it");
        assert_eq!(p.root.as_deref(), Some("/home/u/orbit"));
        // What Fenix said itself wins.
        let given = launch().initialize(json!({ "initializationOptions": { "isHttpEnabled": true, "x": 1 } }));
        assert_eq!(given["initializationOptions"]["isHttpEnabled"], true);
        assert_eq!(given["initializationOptions"]["x"], 1);
        // The rest goes as it is.
        assert_eq!(writes(&p.from_fenix(4, r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#, Ok(&launch())))[0]["method"], "initialized");
        // Started again (Fenix restarted the server): the old Metals is stopped.
        p.started(9);
        let out = p.from_fenix(4, init, Ok(&launch()));
        assert_eq!(out[0], Out::Kill(9));
        assert!(matches!(out[1], Out::Spawn(_)));
    }

    #[test]
    fn without_metals_fenix_is_answered() {
        let mut p = Proxy::default();
        let out = p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///c%3A/orbit"}}"#, Err("Metals isn't installed"));
        let Out::Send(4, text) = &out[0] else { panic!("{out:?}") };
        assert!(text.contains("\"capabilities\":{}") && text.contains("Metals (not running)"), "{text}");
        assert_eq!(p.root.as_deref(), Some("c:/orbit"));
        let out = p.from_fenix(4, r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}"#, Err("Metals isn't installed"));
        assert!(matches!(&out[0], Out::Send(4, t) if t.contains("Metals isn't installed")));
        assert!(p.from_fenix(4, r#"{"jsonrpc":"2.0","method":"initialized"}"#, Err("x")).is_empty(), "a notification isn't answered");
        assert_eq!(p.ask(Ask::Debug { file: "/p/Main.scala".into(), run_type: "run".into() }), Err("Metals isn't installed".into()));
    }

    /// What Metals 1.6.9 said in a session in a scala-cli project (a real run's
    /// bytes, cut in two): `initialize`'s answer, its `workspace/configuration`
    /// and `showMessageRequest`s, the answer to a formatting request and to
    /// `debug-adapter-start`.
    #[test]
    fn what_metals_asks_the_pack_answers_and_the_rest_goes_to_fenix() {
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        p.started(9);
        let raw = include_bytes!("samples/metals-stdout.bin");
        let (a, b) = raw.split_at(raw.len() / 2);
        let mut out = p.from_metals(a);
        out.extend(p.from_metals(b));

        // Fenix gets the answer to its `initialize`, Metals' logs, progress and diagnostics.
        let to_fenix = sent(&out);
        assert!(to_fenix.iter().any(|m| m["id"] == 1 && m["result"]["capabilities"]["documentFormattingProvider"] == true));
        assert!(to_fenix.iter().any(|m| m["method"] == "textDocument/publishDiagnostics"));
        assert!(to_fenix.iter().any(|m| m["method"] == "$/progress" && m["params"]["value"]["title"] == "Indexing"));
        assert!(to_fenix.iter().any(|m| m["method"] == "window/workDoneProgress/create"), "Fenix answers that one as it does for any server");

        // Metals' user configuration is the pack's to answer.
        assert!(!to_fenix.iter().any(|m| m["method"] == "workspace/configuration"));
        let answers: Vec<Value> = writes(&out).into_iter().filter(|m| m.get("result").is_some()).collect();
        assert_eq!(answers[0], json!({ "jsonrpc": "2.0", "id": "1", "result": [{ "autoImportBuilds": "all", "javaHome": "/usr/lib/jvm/java-21" }] }));
        // And the questions it asked: not to start its HTTP server, to format with the defaults.
        let said: Vec<(String, String)> = answers[1..].iter().map(|a| (a["id"].as_str().unwrap().to_string(), a["result"]["title"].as_str().unwrap().to_string())).collect();
        assert_eq!(said, [("4".to_string(), "Not now".to_string()), ("7".to_string(), "Run anyway".to_string())]);
        // A question with no answer ready (a build target misconfigured) is Fenix's.
        assert!(to_fenix.iter().any(|m| m["id"] == "5" && m["method"] == "window/showMessageRequest"));
        // The formatting it answered, and the debug session, go to Fenix as they came.
        assert!(to_fenix.iter().any(|m| m["id"] == 2 && m["result"][0]["newText"].as_str().is_some_and(|t| t.starts_with("object Ugly {"))));
    }

    #[test]
    fn its_own_requests_are_answered_to_it_and_not_to_fenix() {
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        p.started(9);
        let asked = writes(&p.ask(Ask::Debug { file: "/home/u/my orbit/Main.scala".into(), run_type: "runOrTestFile".into() }).unwrap());
        assert_eq!(asked[0]["method"], "workspace/executeCommand");
        assert_eq!(asked[0]["params"], json!({ "command": "debug-adapter-start", "arguments": [{ "path": "file:///home/u/my%20orbit/Main.scala", "runType": "runOrTestFile" }] }));
        let id = asked[0]["id"].as_str().unwrap().to_string();
        // As Metals answered it.
        let answer = frame(&json!({ "jsonrpc": "2.0", "id": id, "result": { "name": "hello", "uri": "tcp://127.0.0.1:45007" } }).to_string());
        let out = p.from_metals(&answer);
        assert_eq!(out.len(), 1, "{out:?}");
        let Out::Answer(Ask::Debug { run_type, .. }, Ok(v)) = &out[0] else { panic!("{out:?}") };
        assert_eq!(run_type, "runOrTestFile");
        assert_eq!(debug_session(v), Some(("hello".to_string(), 45007)));

        // Refused, or Metals going away.
        let id = writes(&p.ask(Ask::Debug { file: "/p/A.scala".into(), run_type: "run".into() }).unwrap())[0]["id"].as_str().unwrap().to_string();
        let refused = frame(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": "Internal error." } }).to_string());
        assert!(matches!(&p.from_metals(&refused)[0], Out::Answer(_, Err(e)) if e == "Internal error."));
        p.ask(Ask::Debug { file: "/p/B.scala".into(), run_type: "run".into() }).unwrap();
        assert!(matches!(&p.exited()[0], Out::Answer(_, Err(e)) if e == "Metals stopped"));
        assert!(p.ask(Ask::Debug { file: "/p/B.scala".into(), run_type: "run".into() }).is_err());
        assert_eq!(debug_session(&json!({ "name": "x" })), None);
        assert_eq!(debug_session(&json!({ "uri": "tcp://[::1]:7000" })).map(|s| s.1), Some(7000));
    }

    #[test]
    fn the_build_is_imported_again_when_asked() {
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        p.started(9);
        let asked = writes(&p.ask(Ask::Import).unwrap());
        assert_eq!(asked[0]["params"], json!({ "command": "build-import" }));
        let id = asked[0]["id"].as_str().unwrap().to_string();
        let out = p.from_metals(&frame(&json!({ "jsonrpc": "2.0", "id": id, "result": null }).to_string()));
        assert_eq!(out, [Out::Answer(Ask::Import, Ok(Value::Null))]);
    }

    #[test]
    fn a_metals_that_wont_start_is_said_to_the_fenix_waiting_for_it() {
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        let out = p.failed("/t/metals isn't there");
        let said = sent(&out);
        assert_eq!((said[0]["id"].clone(), said[0]["error"]["message"].clone()), (json!(1), json!("/t/metals isn't there")));
        assert!(p.failed("again").is_empty(), "initialize is answered once");
        let out = p.from_fenix(4, r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}"#, Ok(&launch()));
        assert!(matches!(&out[0], Out::Send(4, t) if t.contains("again")), "{out:?}");

        // Answered by Metals: nothing's waiting.
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        p.started(9);
        p.from_metals(&frame(r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#));
        assert!(p.failed("x").is_empty());
    }

    #[test]
    fn what_to_answer_is_by_the_titles_offered() {
        let p = Proxy::default();
        let ask = |actions: &[&str], message: &str| {
            let a: Vec<Value> = actions.iter().map(|t| json!({ "title": t })).collect();
            p.answer("window/showMessageRequest", &json!({ "params": { "actions": a, "message": message } }))
        };
        assert_eq!(ask(&["Import build", "Not now", "Don't show again"], "New sbt workspace detected, would you like to import the build?"), Some(json!({ "title": "Import build" })));
        assert_eq!(ask(&["Import changes", "Not now"], "sbt build changed"), Some(json!({ "title": "Import changes" })));
        assert_eq!(ask(&["Start", "Not now", "Don't show again"], "Http server is required for such features as Metals Doctor"), Some(json!({ "title": "Not now" })));
        assert_eq!(ask(&["Not now", "Never"], "Something else asked"), None, "Not now is only for the HTTP server");
        assert_eq!(ask(&["Create .scalafmt.conf", "Not now"], "no"), None, "a file in the project isn't the pack's to create");
        assert_eq!(p.answer("window/showMessageRequest", &json!({ "params": {} })), None);
        assert_eq!(p.answer("client/registerCapability", &json!({})), None);
    }
}
