//! Eclipse JDT LS, run by the pack: the pack is Java's language server
//! to Fenix (`contributes.language_servers`), and passes each message on
//! to the JDT LS it starts, and each of JDT LS's back -- so it can say
//! more in `initialize` (java-debug's bundle, the project's JDK) and ask
//! JDT LS things of its own: `vscode.java.startDebugSession`, which
//! starts java-debug and says its port, and the main classes and their
//! classpaths that debugging a main class needs.
//!
//! A state machine: what comes from Fenix or from JDT LS goes in, and
//! what to do comes out ([`Out`]), which the extension does.

use std::collections::HashMap;

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::{self, json, Value};

use crate::tools::{JDTLS_LAUNCHER, JDTLS_JAVA};

/// The prefix of the pack's own requests' ids: JDT LS's answers to them
/// stay here.
const OWN: &str = "fenix.java/";

/// How JDT LS is started for a project, and what it's told.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    /// The `java` it runs on: 25 or later.
    pub java: String,
    /// `config_linux`, `config_mac_arm`, `config_win`.
    pub config: String,
    /// The tools folder's full path, for java-debug's bundle: JDT LS
    /// reads it from `initialize`, where `${tools}` isn't filled in.
    pub tools: Option<String>,
    /// java-debug's jar, under `/tools`, when it's installed.
    pub java_debug: Option<String>,
    /// The project's JDK: its release and its home.
    pub jdk: Option<(u32, String)>,
}

impl Launch {
    /// JDT LS's command line, for the project at `root` (its full path):
    /// its workspace, a folder of the pack's own for each project
    /// (`${data}`, 0.8.7), apart from what it installs.
    pub fn command(&self, root: &str) -> Command {
        let workspace = format!("${{data}}/jdtls-workspace/{:016x}", fenix_lang_kit::packages::hash(root.as_bytes()));
        Command::new(&self.java).args([
            "-Declipse.application=org.eclipse.jdt.ls.core.id1".to_string(),
            "-Dosgi.bundles.defaultStartLevel=4".into(),
            "-Declipse.product=org.eclipse.jdt.ls.core.product".into(),
            "-Dosgi.checkConfiguration=true".into(),
            format!("-Dosgi.sharedConfiguration.area=${{tools}}/jdtls/{}", self.config),
            "-Dosgi.sharedConfiguration.area.readOnly=true".into(),
            "-Dosgi.configuration.cascaded=true".into(),
            // Eclipse's .project, .classpath and .settings kept in its
            // workspace, not written into the project's folders.
            "-Djava.import.generatesMetadataFilesAtProjectRoot=false".into(),
            "-Xms256m".into(),
            "-XX:+UseParallelGC".into(),
            "--add-modules=ALL-SYSTEM".into(),
            "--add-opens".into(),
            "java.base/java.util=ALL-UNNAMED".into(),
            "--add-opens".into(),
            "java.base/java.lang=ALL-UNNAMED".into(),
            "-jar".into(),
            format!("${{tools}}/jdtls/{JDTLS_LAUNCHER}"),
            "-data".into(),
            workspace,
        ])
    }

    /// `initialize`'s params, with what JDT LS is told besides: the
    /// bundles, the JDK, and what its client can do.
    pub fn initialize(&self, mut params: Value) -> Value {
        let bundles: Vec<String> = match (&self.tools, &self.java_debug) {
            (Some(tools), Some(jar)) => vec![format!("{tools}/{}", jar.trim_start_matches("/tools/"))],
            _ => Vec::new(),
        };
        let mut java = json!({
            "import": { "generatesMetadataFilesAtProjectRoot": false },
            "configuration": { "updateBuildConfiguration": "automatic" },
            "format": { "enabled": true },
            "signatureHelp": { "enabled": true },
            "contentProvider": { "preferred": "fernflower" },
        });
        if let Some((release, home)) = &self.jdk {
            java["configuration"]["runtimes"] = json!([{ "name": runtime_name(*release), "path": home, "default": true }]);
            java["import"]["gradle"] = json!({ "java": { "home": home } });
        }
        let folders: Vec<Value> = params.get("rootUri").and_then(Value::as_str).map(|u| vec![json!(u)]).unwrap_or_default();
        let options = json!({
            "bundles": bundles,
            "workspaceFolders": folders,
            "settings": { "java": java },
            "extendedClientCapabilities": {
                "classFileContentsSupport": false,
                "progressReportProvider": false,
                "advancedExtractRefactoringSupport": false,
                "resolveAdditionalTextEditsSupport": true,
            },
        });
        if let Some(p) = params.as_object_mut() {
            p.insert("initializationOptions".into(), options);
        }
        params
    }
}

/// The name JDT LS gives Java `release`'s execution environment.
pub fn runtime_name(release: u32) -> String {
    if release <= 8 {
        format!("JavaSE-1.{release}")
    } else {
        format!("JavaSE-{release}")
    }
}

/// `config_linux`, `config_mac_arm`, `config_win`: JDT LS's own for the
/// machine.
pub fn config_dir(os: &str, arch: &str) -> String {
    let arm = arch == "aarch64";
    match os {
        "macos" if arm => "config_mac_arm".into(),
        "macos" => "config_mac".into(),
        "windows" => "config_win".into(),
        _ if arm => "config_linux_arm".into(),
        _ => "config_linux".into(),
    }
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

/// What the pack asked JDT LS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `vscode.java.startDebugSession`: java-debug's port.
    DebugSession,
    /// `vscode.java.resolveMainClass`: the main classes.
    MainClasses,
    /// `vscode.java.resolveClasspath` for a main class and its project.
    Classpath(MainClass),
}

/// A main class JDT LS found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MainClass {
    pub main: String,
    pub project: Option<String>,
    /// Its file, a full path.
    pub file: Option<String>,
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// Start JDT LS, its stdout as bytes.
    Spawn(Command),
    /// Bytes for JDT LS's stdin.
    Write(Vec<u8>),
    /// A message for Fenix, from server `server`.
    Send(u64, String),
    /// JDT LS has read the project.
    Ready,
    /// The answer to one of the pack's own requests.
    Answer(Ask, Result<Value, String>),
}

#[derive(Debug, Default)]
pub struct Proxy {
    /// Fenix's id for the server.
    pub server: Option<u64>,
    /// The JDT LS running, by its process id.
    pub process: Option<u64>,
    /// The project's full path, from `initialize`.
    pub root: Option<String>,
    pub ready: bool,
    /// Why there's no JDT LS, when there isn't one.
    pub missing: Option<String>,
    buf: Vec<u8>,
    next: u64,
    asked: HashMap<String, Ask>,
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

fn error(id: &Value, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": message } }).to_string()
}

impl Proxy {
    /// A message from Fenix for server `server`. `launch` is how to start
    /// JDT LS, or why it can't be.
    pub fn from_fenix(&mut self, server: u64, message: &str, launch: Result<&Launch, &str>) -> Vec<Out> {
        self.server = Some(server);
        let Ok(mut msg) = serde_json::from_str::<Value>(message) else { return Vec::new() };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let id = msg.get("id").cloned();
        let mut out = Vec::new();
        if method == "initialize" {
            let root = msg.pointer("/params/rootUri").and_then(Value::as_str).map(uri_path);
            self.root = root.clone();
            match launch {
                Ok(l) => {
                    self.missing = None;
                    out.push(Out::Spawn(l.command(root.as_deref().unwrap_or("."))));
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    msg["params"] = l.initialize(params);
                }
                Err(why) => {
                    // Fenix is answered, so it doesn't wait; the server
                    // starts again once there's a JDT LS.
                    self.missing = Some(why.to_string());
                    let caps = json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {}, "serverInfo": { "name": "jdtls (not running)" } } });
                    out.push(Out::Send(server, caps.to_string()));
                    return out;
                }
            }
        }
        if self.missing.is_some() && self.process.is_none() {
            if let (Some(id), false) = (&id, method.is_empty()) {
                out.push(Out::Send(server, error(id, self.missing.as_deref().unwrap_or("no JDT LS"))));
            }
            return out;
        }
        out.push(Out::Write(frame(&msg.to_string())));
        out
    }

    /// JDT LS started, as process `process`.
    pub fn started(&mut self, process: u64) {
        self.process = Some(process);
        self.ready = false;
        self.buf.clear();
    }

    /// Bytes from JDT LS's stdout.
    pub fn from_jdtls(&mut self, bytes: &[u8]) -> Vec<Out> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        for text in unframe(&mut self.buf) {
            let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
            let own = msg.get("method").is_none().then(|| msg.get("id").and_then(Value::as_str)).flatten().filter(|id| id.starts_with(OWN)).map(str::to_string);
            if let Some(id) = own {
                if let Some(ask) = self.asked.remove(&id) {
                    let answer = match (msg.get("result"), msg.get("error")) {
                        (_, Some(e)) => Err(e.get("message").and_then(Value::as_str).unwrap_or("JDT LS said no").to_string()),
                        (Some(r), None) => Ok(r.clone()),
                        _ => Ok(Value::Null),
                    };
                    out.push(Out::Answer(ask, answer));
                }
                continue;
            }
            if msg.get("method").and_then(Value::as_str) == Some("language/status") && msg.pointer("/params/type").and_then(Value::as_str) == Some("ServiceReady") && !self.ready {
                self.ready = true;
                out.push(Out::Ready);
            }
            if let Some(server) = self.server {
                out.push(Out::Send(server, text));
            }
        }
        out
    }

    /// One of the pack's own requests: `workspace/executeCommand`.
    pub fn ask(&mut self, ask: Ask) -> Result<Vec<Out>, String> {
        if self.process.is_none() || !self.ready {
            return Err(self.missing.clone().unwrap_or_else(|| "JDT LS is still reading the project".into()));
        }
        self.next += 1;
        let id = format!("{OWN}{}", self.next);
        let (command, args) = match &ask {
            Ask::DebugSession => ("vscode.java.startDebugSession", json!([])),
            Ask::MainClasses => ("vscode.java.resolveMainClass", json!([self.root.as_deref().map(|r| format!("file://{}", if r.starts_with('/') { r.to_string() } else { format!("/{r}") }))])),
            Ask::Classpath(m) => ("vscode.java.resolveClasspath", json!([m.main, m.project])),
        };
        self.asked.insert(id.clone(), ask);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": "workspace/executeCommand", "params": { "command": command, "arguments": args } });
        Ok(vec![Out::Write(frame(&msg.to_string()))])
    }

    /// JDT LS ended: what it was asked is answered no.
    pub fn exited(&mut self) -> Vec<Out> {
        self.process = None;
        self.ready = false;
        self.buf.clear();
        self.asked.drain().map(|(_, ask)| Out::Answer(ask, Err("JDT LS stopped".into()))).collect()
    }
}

/// `vscode.java.resolveMainClass`'s answer.
pub fn main_classes(answer: &Value) -> Vec<MainClass> {
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(MainClass {
                main: m.get("mainClass")?.as_str()?.to_string(),
                project: m.get("projectName").and_then(Value::as_str).map(str::to_string),
                file: m.get("filePath").and_then(Value::as_str).map(|p| p.replace('\\', "/")),
            })
        })
        .collect()
}

/// `vscode.java.resolveClasspath`'s answer: module path, then class path.
pub fn classpath(answer: &Value) -> (Vec<String>, Vec<String>) {
    let list = |i: usize| answer.get(i).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    (list(0), list(1))
}

/// Whether `java`'s major version runs JDT LS.
pub fn runs_jdtls(major: Option<u32>) -> bool {
    major.is_some_and(|m| m >= JDTLS_JAVA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn launch() -> Launch {
        Launch { java: "/tools/jre/bin/java".into(), config: "config_linux".into(), tools: Some("/home/u/tools/fenix.java".into()), java_debug: Some("/tools/java-debug/com.microsoft.java.debug.plugin-0.53.1.jar".into()), jdk: Some((21, "/usr/lib/jvm/java-21".into())) }
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
    fn jdtls_is_started_and_told_of_java_debug_and_the_jdk() {
        let mut p = Proxy::default();
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":7,"rootUri":"file:///home/u/orbit","capabilities":{}}}"#;
        let out = p.from_fenix(4, init, Ok(&launch()));
        let Out::Spawn(cmd) = &out[0] else { panic!("{out:?}") };
        let l = line(cmd);
        assert!(l.starts_with("/tools/jre/bin/java -Declipse.application=org.eclipse.jdt.ls.core.id1"), "{l}");
        assert!(l.contains("-Djava.import.generatesMetadataFilesAtProjectRoot=false"), "{l}");
        assert!(l.contains("-Dosgi.sharedConfiguration.area=${tools}/jdtls/config_linux") && l.contains("-jar ${tools}/jdtls/plugins/org.eclipse.equinox.launcher_"), "{l}");
        assert!(l.contains(&format!("-data ${{data}}/jdtls-workspace/{:016x}", fenix_lang_kit::packages::hash(b"/home/u/orbit"))), "{l}");
        let sent = writes(&out);
        let opts = &sent[0]["params"]["initializationOptions"];
        assert_eq!(opts["bundles"], json!(["/home/u/tools/fenix.java/java-debug/com.microsoft.java.debug.plugin-0.53.1.jar"]));
        assert_eq!(opts["settings"]["java"]["import"], json!({ "generatesMetadataFilesAtProjectRoot": false, "gradle": { "java": { "home": "/usr/lib/jvm/java-21" } } }));
        assert_eq!(opts["settings"]["java"]["configuration"]["runtimes"], json!([{ "name": "JavaSE-21", "path": "/usr/lib/jvm/java-21", "default": true }]));
        assert_eq!(sent[0]["params"]["processId"], 7, "the rest as Fenix sent it");
        assert_eq!(p.root.as_deref(), Some("/home/u/orbit"));
        // The rest goes as it is.
        assert_eq!(writes(&p.from_fenix(4, r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#, Ok(&launch())))[0]["method"], "initialized");
        assert_eq!(runtime_name(8), "JavaSE-1.8");
        assert_eq!(config_dir("macos", "aarch64"), "config_mac_arm");
        assert_eq!(config_dir("windows", "x86_64"), "config_win");
    }

    #[test]
    fn without_jdtls_fenix_is_answered() {
        let mut p = Proxy::default();
        let out = p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///c%3A/orbit"}}"#, Err("JDT LS isn't installed"));
        let Out::Send(4, text) = &out[0] else { panic!("{out:?}") };
        assert!(text.contains("\"capabilities\":{}"), "{text}");
        assert_eq!(p.root.as_deref(), Some("c:/orbit"));
        let out = p.from_fenix(4, r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}"#, Err("JDT LS isn't installed"));
        assert!(matches!(&out[0], Out::Send(4, t) if t.contains("JDT LS isn't installed")));
        assert!(p.from_fenix(4, r#"{"jsonrpc":"2.0","method":"initialized"}"#, Err("x")).is_empty());
        assert_eq!(p.ask(Ask::DebugSession), Err("JDT LS isn't installed".into()));
    }

    #[test]
    fn its_own_requests_are_answered_to_it_and_the_rest_go_to_fenix() {
        let mut p = Proxy::default();
        p.from_fenix(4, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///home/u/orbit"}}"#, Ok(&launch()));
        p.started(9);
        assert!(p.ask(Ask::DebugSession).is_err(), "not ready yet");
        // What JDT LS said, as it said it (a real run's bytes, cut in two).
        let raw = include_bytes!("samples/jdtls-stdout.bin");
        let (a, b) = raw.split_at(raw.len() / 2);
        let mut out = p.from_jdtls(a);
        out.extend(p.from_jdtls(b));
        assert!(out.contains(&Out::Ready));
        let sent: Vec<&String> = out.iter().filter_map(|o| if let Out::Send(4, t) = o { Some(t) } else { None }).collect();
        assert!(sent.iter().any(|t| t.contains("\"id\":1") && t.contains("executeCommandProvider")), "initialize answered");
        assert!(sent.iter().any(|t| t.contains("ServiceReady")));

        let asked = writes(&p.ask(Ask::DebugSession).unwrap());
        assert_eq!(asked[0]["params"]["command"], "vscode.java.startDebugSession");
        let id = asked[0]["id"].as_str().unwrap().to_string();
        let answer = frame(&json!({ "jsonrpc": "2.0", "id": id, "result": 42067 }).to_string());
        assert_eq!(p.from_jdtls(&answer), [Out::Answer(Ask::DebugSession, Ok(json!(42067)))], "not Fenix's");

        let asked = writes(&p.ask(Ask::MainClasses).unwrap());
        assert_eq!(asked[0]["params"]["arguments"], json!(["file:///home/u/orbit"]));
        assert_eq!(p.exited(), [Out::Answer(Ask::MainClasses, Err("JDT LS stopped".into()))]);
    }

    #[test]
    fn main_classes_and_their_classpath_are_read() {
        let m = main_classes(&json!([{ "mainClass": "orbit.app.App", "projectName": "app", "filePath": "/home/u/orbit/app/src/main/java/orbit/app/App.java" }, { "projectName": "x" }]));
        assert_eq!(m, [MainClass { main: "orbit.app.App".into(), project: Some("app".into()), file: Some("/home/u/orbit/app/src/main/java/orbit/app/App.java".into()) }]);
        assert_eq!(classpath(&json!([[], ["/home/u/orbit/app/bin/main", "/m2/guava.jar"]])), (Vec::new(), vec!["/home/u/orbit/app/bin/main".to_string(), "/m2/guava.jar".to_string()]));
        assert!(runs_jdtls(Some(25)) && !runs_jdtls(Some(21)) && !runs_jdtls(None));
        assert_eq!(uri_path("file:///home/u/my%20orbit"), "/home/u/my orbit");
    }
}
