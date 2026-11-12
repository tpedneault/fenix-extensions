//! What coding agents are given (`containers_list`, `containers_logs`,
//! `containers_images`): the containers and their state, the tail of one
//! container's log, the images. Only reads: nothing here starts, stops,
//! removes, execs or builds, and nothing reads a container's environment
//! or its `inspect`, which hold its secrets.
//!
//! A container's name or id comes from the agent and goes on the engine's
//! command line, so it's held to the characters a name can have
//! (`reference`) and, for Docker and Podman, passed after `--`
//! (`Engine::logs_tail`): it can't be an option or more than one argument.
//! A log can hold anything a program printed -- tokens, passwords -- so
//! its tail is bounded in lines and bytes, and says when it was cut.

use std::collections::VecDeque;

use serde_json::{json, Map, Value};

use crate::engine::Engine;
use crate::model::{Container, Image};

/// Lines of a log an agent is given when it doesn't say, and at most.
pub const DEFAULT_LINES: usize = 200;
pub const MAX_LINES: usize = 2000;
/// The most of a log an agent is given, in bytes, whatever the lines.
pub const MAX_BYTES: usize = 64 * 1024;
/// A line longer than this is cut.
pub const MAX_LINE_BYTES: usize = 2048;
/// The most containers or images listed.
pub const MAX_LISTED: usize = 500;
/// The longest name or id taken.
const MAX_REFERENCE: usize = 128;

/// What an agent asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// The containers; stopped ones too unless `all` is false.
    List { all: bool },
    /// The last `lines` of `container`'s log.
    Logs { container: String, lines: usize },
    Images,
}

/// Service `service`'s call, from its JSON arguments.
pub fn ask(service: &str, payload: &str) -> Result<Ask, String> {
    let args: Value = if payload.trim().is_empty() { json!({}) } else { serde_json::from_str(payload).map_err(|e| format!("the arguments aren't JSON: {e}"))? };
    match service {
        "containers.list" => Ok(Ask::List { all: args.get("all").and_then(Value::as_bool).unwrap_or(true) }),
        "containers.images" => Ok(Ask::Images),
        "containers.logs" => {
            let given = args.get("container").and_then(Value::as_str).ok_or("say which container: its name or id, as containers_list gives them")?;
            let lines = match args.get("lines") {
                None | Some(Value::Null) => DEFAULT_LINES,
                Some(v) => v.as_u64().ok_or("lines is a whole number")?.clamp(1, MAX_LINES as u64) as usize,
            };
            Ok(Ask::Logs { container: reference(given)?, lines })
        }
        other => Err(format!("no service {other}")),
    }
}

/// A container's name or id, as an agent gave it, if it can only be
/// that: letters, digits, `_`, `.` and `-`, starting with a letter or
/// digit (what Docker and Podman allow in a name; an id is hex), at most
/// 128 of them. So never an option (`-f`, `--help`), a second argument, a
/// path or anything a shell would read. One leading `/`, as `inspect`
/// writes names, is dropped.
pub fn reference(given: &str) -> Result<String, String> {
    let name = given.trim();
    let name = name.strip_prefix('/').unwrap_or(name);
    let refuse = || format!("{given:?} isn't a container's name or id: those are letters, digits, _ . and -, starting with a letter or digit");
    if name.is_empty() || name.len() > MAX_REFERENCE {
        return Err(refuse());
    }
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Err(refuse());
    }
    Ok(name.to_string())
}

/// An id as agents are given it: the 12 characters engines show, which
/// each takes back.
fn short(id: &str) -> String {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    id.chars().take(12).collect()
}

/// `containers_list`'s answer.
pub fn list_answer(engine: Engine, containers: &[Container], all: bool) -> Value {
    let shown: Vec<&Container> = containers.iter().filter(|c| all || c.running()).collect();
    let running = containers.iter().filter(|c| c.running()).count();
    let rows: Vec<Value> = shown
        .iter()
        .take(MAX_LISTED)
        .map(|c| {
            let mut row = Map::new();
            row.insert("id".into(), short(&c.id).into());
            row.insert("name".into(), c.name.clone().into());
            row.insert("image".into(), c.image.clone().into());
            row.insert("state".into(), c.state.clone().into());
            row.insert("status".into(), c.status.clone().into());
            if let Some(h) = c.health() {
                row.insert("health".into(), h.into());
            }
            if let Some(code) = c.exit_code() {
                row.insert("exit_code".into(), code.into());
            }
            if !c.ports.is_empty() {
                row.insert("ports".into(), c.ports.clone().into());
            }
            if let Some(p) = &c.project {
                row.insert("compose_project".into(), p.clone().into());
            }
            if let Some(s) = &c.service {
                row.insert("compose_service".into(), s.clone().into());
            }
            Value::Object(row)
        })
        .collect();
    let mut answer = json!({ "engine": engine.program(), "running": running, "stopped": containers.len() - running, "containers": rows });
    if shown.len() > MAX_LISTED {
        answer["truncated"] = format!("the first {MAX_LISTED} of {}", shown.len()).into();
    }
    answer
}

/// `containers_images`' answer: each image, its size and age.
pub fn images_answer(engine: Engine, images: &[Image]) -> Value {
    let rows: Vec<Value> = images
        .iter()
        .take(MAX_LISTED)
        .map(|i| {
            json!({
                "id": short(&i.id),
                "repository": i.repository,
                "tag": i.tag,
                "size_bytes": i.size,
                "created": i.created,
                "dangling": i.dangling(),
            })
        })
        .collect();
    let mut answer = json!({ "engine": engine.program(), "images": rows });
    if images.len() > MAX_LISTED {
        answer["truncated"] = format!("the first {MAX_LISTED} of {}", images.len()).into();
    }
    answer
}

/// A log's last lines as they come, kept to `lines` and `MAX_BYTES`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tail {
    want: usize,
    kept: VecDeque<String>,
    bytes: usize,
    /// Lines that came, kept or not.
    seen: usize,
    /// Lines cut to `MAX_LINE_BYTES`, among those kept.
    long: usize,
}

impl Tail {
    pub fn new(lines: usize) -> Tail {
        Tail { want: lines.clamp(1, MAX_LINES), kept: VecDeque::new(), bytes: 0, seen: 0, long: 0 }
    }

    /// A line the log wrote: its terminal colours and other control
    /// characters taken out, cut if it's very long.
    pub fn push(&mut self, line: &str) {
        self.seen += 1;
        let mut text = plain(line);
        if text.len() > MAX_LINE_BYTES {
            let mut end = MAX_LINE_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str(" …[line cut]");
            self.long += 1;
        }
        self.bytes += text.len() + 1;
        self.kept.push_back(text);
        while self.kept.len() > self.want || (self.bytes > MAX_BYTES && self.kept.len() > 1) {
            if let Some(gone) = self.kept.pop_front() {
                self.bytes -= gone.len() + 1;
                if gone.ends_with(" …[line cut]") {
                    self.long -= 1;
                }
            }
        }
    }

    /// `containers_logs`' answer, for `container`.
    pub fn answer(&self, engine: Engine, container: &str) -> Value {
        let mut answer = json!({
            "engine": engine.program(),
            "container": container,
            "lines": self.kept.len(),
            "log": self.kept.iter().map(String::as_str).collect::<Vec<_>>().join("\n"),
        });
        let mut cut = Vec::new();
        if self.kept.len() < self.seen {
            cut.push(format!("only the last {} of the {} lines read{}", self.kept.len(), self.seen, if self.kept.len() < self.want { format!(", to stay under {} KiB", MAX_BYTES / 1024) } else { String::new() }));
        }
        if self.long > 0 {
            cut.push(format!("{} line{} longer than {MAX_LINE_BYTES} bytes cut", self.long, if self.long == 1 { "" } else { "s" }));
        }
        if !cut.is_empty() {
            answer["truncated"] = cut.join("; ").into();
        }
        answer
    }
}

/// `line` without terminal escapes (`ESC [ ... m`) or control characters
/// other than a tab.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC [ params final; otherwise ESC and the one after.
            if chars.peek() == Some(&'[') {
                chars.next();
                for f in chars.by_ref() {
                    if ('@'..='~').contains(&f) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else if c == '\t' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Why an engine's answer is a failure: its last line that says
/// something, `Error: ` off.
pub fn failure(program: &str, err: &[String], out: &[String]) -> String {
    let why = err.iter().chain(out).rev().map(|l| l.trim()).find(|l| !l.is_empty()).map(|l| l.trim_start_matches("Error: ").trim_start_matches("Error response from daemon: ").to_string());
    match why {
        Some(why) => format!("{program}: {why}"),
        None => format!("{program} failed and said nothing"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;

    #[test]
    fn arguments_are_read_with_their_defaults_and_bounds() {
        assert_eq!(ask("containers.list", ""), Ok(Ask::List { all: true }));
        assert_eq!(ask("containers.list", r#"{"all":false}"#), Ok(Ask::List { all: false }));
        assert_eq!(ask("containers.images", "{}"), Ok(Ask::Images));
        assert_eq!(ask("containers.logs", r#"{"container":"shop-web-1"}"#), Ok(Ask::Logs { container: "shop-web-1".into(), lines: DEFAULT_LINES }));
        assert_eq!(ask("containers.logs", r#"{"container":"web","lines":0}"#), Ok(Ask::Logs { container: "web".into(), lines: 1 }));
        assert_eq!(ask("containers.logs", r#"{"container":"web","lines":999999}"#), Ok(Ask::Logs { container: "web".into(), lines: MAX_LINES }));
        assert!(ask("containers.logs", r#"{"container":"web","lines":"lots"}"#).is_err());
        assert!(ask("containers.logs", "{}").unwrap_err().contains("which container"));
        assert!(ask("containers.logs", "not json").is_err());
        assert!(ask("containers.exec", "{}").is_err(), "nothing but the three");
    }

    /// Nothing an agent gives can become an option or another argument.
    #[test]
    fn a_reference_is_a_name_or_an_id_and_nothing_else() {
        for ok in ["web", "shop-web-1", "shop_api.1", "8a1f3c2d9e12", "A1"] {
            assert_eq!(reference(ok).as_deref(), Ok(ok), "{ok}");
        }
        assert_eq!(reference(" /web ").as_deref(), Ok("web"), "inspect's leading slash, and spaces around");
        for bad in ["", "-f", "--help", "--tail=99999", "-", ".hidden", "_x", "web api", "web\napi", "web;rm -rf /", "$(id)", "`id`", "a/b", "../etc", "web=1", "c:\\x", "wéb", "web\0", "//web"] {
            assert!(reference(bad).is_err(), "{bad:?} let through");
        }
        assert!(reference(&"a".repeat(128)).is_ok());
        assert!(reference(&"a".repeat(129)).is_err());
        assert!(ask("containers.logs", r#"{"container":"--privileged"}"#).is_err());
        assert!(ask("containers.logs", r#"{"container":"web --since 1h"}"#).is_err());
        // What the engine is asked: the reference, alone, after `--`.
        let Ok(Ask::Logs { container, lines }) = ask("containers.logs", r#"{"container":"web","lines":50}"#) else { panic!() };
        let args = Engine::Docker.logs_tail(&container, lines);
        assert_eq!(args.iter().position(|a| a == "--"), Some(args.len() - 2));
        assert_eq!(args.last().map(String::as_str), Some("web"));
    }

    const PS: &str = r#"{"Command":"\"/entrypoint.sh --password=hunter2\"","ID":"8a1f3c2d9e1234567890","Image":"nginx:1.27","Labels":"com.docker.compose.project=shop,com.docker.compose.service=web","Names":"shop-web-1","Ports":"0.0.0.0:8080->80/tcp","State":"running","Status":"Up 2 hours (healthy)"}
{"ID":"77aa","Image":"shop-api:dev","Labels":"","Names":"migrate","Ports":"","State":"exited","Status":"Exited (1) 12 minutes ago"}"#;

    #[test]
    fn the_list_says_each_containers_state_and_not_its_command() {
        let cs = model::containers(PS);
        let a = list_answer(Engine::Docker, &cs, true);
        assert_eq!((a["engine"].as_str(), a["running"].as_u64(), a["stopped"].as_u64()), (Some("docker"), Some(1), Some(1)));
        let web = &a["containers"][0];
        assert_eq!(web["id"], "8a1f3c2d9e12");
        assert_eq!((web["name"].as_str(), web["state"].as_str(), web["health"].as_str()), (Some("shop-web-1"), Some("running"), Some("healthy")));
        assert_eq!((web["compose_project"].as_str(), web["compose_service"].as_str(), web["ports"].as_str()), (Some("shop"), Some("web"), Some("0.0.0.0:8080->80/tcp")));
        assert_eq!(a["containers"][1]["exit_code"], 1);
        assert!(a["containers"][1].get("health").is_none() && a["containers"][1].get("ports").is_none());
        assert!(!a.to_string().contains("hunter2"), "a container's command line isn't given: {a}");
        let running = list_answer(Engine::Docker, &cs, false);
        assert_eq!(running["containers"].as_array().map(Vec::len), Some(1));
        assert!(a.get("truncated").is_none());
        let many: Vec<Container> = (0..MAX_LISTED + 3).map(|n| Container { name: format!("c{n}"), ..cs[1].clone() }).collect();
        let a = list_answer(Engine::Podman, &many, true);
        assert_eq!(a["containers"].as_array().map(Vec::len), Some(MAX_LISTED));
        assert_eq!(a["truncated"], format!("the first {MAX_LISTED} of {}", MAX_LISTED + 3));
    }

    #[test]
    fn wslcs_columns_are_listed_the_same() {
        let out = "CONTAINER ID   IMAGE          COMMAND   CREATED       STATUS         PORTS   NAMES\n4d2c9f1a0b7e   nginx:latest   \"nginx\"   2 hours ago   Up 2 hours             web\n";
        let a = list_answer(Engine::Wslc, &model::wslc_containers(out), true);
        assert_eq!((a["containers"][0]["name"].as_str(), a["containers"][0]["state"].as_str(), a["engine"].as_str()), (Some("web"), Some("running"), Some("wslc")));
    }

    #[test]
    fn images_say_their_size_and_age() {
        let imgs = model::images("{\"ID\":\"sha256:3f9c0011223344556677\",\"Repository\":\"nginx\",\"Tag\":\"1.27\",\"Size\":\"188MB\",\"CreatedSince\":\"3 weeks ago\"}\n{\"ID\":\"99\",\"Repository\":\"<none>\",\"Tag\":\"<none>\",\"Size\":\"1MB\",\"CreatedSince\":\"2 hours ago\"}");
        let a = images_answer(Engine::Docker, &imgs);
        let first = &a["images"][0];
        assert_eq!((first["id"].as_str(), first["repository"].as_str(), first["tag"].as_str()), (Some("3f9c00112233"), Some("nginx"), Some("1.27")));
        assert_eq!((first["size_bytes"].as_u64(), first["created"].as_str(), first["dangling"].as_bool()), (Some(188_000_000), Some("3 weeks ago"), Some(false)));
        assert_eq!(a["images"][1]["dangling"], true);
    }

    #[test]
    fn a_tail_keeps_its_last_lines_and_says_what_it_left_out() {
        let mut t = Tail::new(3);
        for n in 1..=10 {
            t.push(&format!("line {n}"));
        }
        let a = t.answer(Engine::Docker, "web");
        assert_eq!(a["log"], "line 8\nline 9\nline 10");
        assert_eq!(a["lines"], 3);
        assert_eq!(a["truncated"], "only the last 3 of the 10 lines read");
        // All of a short log: nothing said.
        let mut t = Tail::new(200);
        t.push("ready");
        assert!(t.answer(Engine::Docker, "web").get("truncated").is_none());
        // Colours and control characters out; tabs kept.
        let mut t = Tail::new(5);
        t.push("\u{1b}[31mERROR\u{1b}[0m\tpayment \u{7}refused\r");
        assert_eq!(t.answer(Engine::Docker, "web")["log"], "ERROR\tpayment refused");
    }

    #[test]
    fn a_tail_stays_under_its_byte_cap_and_cuts_long_lines() {
        let mut t = Tail::new(MAX_LINES);
        let line = "x".repeat(1000);
        for _ in 0..MAX_LINES {
            t.push(&line);
        }
        let a = t.answer(Engine::Docker, "web");
        let log = a["log"].as_str().unwrap();
        assert!(log.len() <= MAX_BYTES, "{}", log.len());
        let kept = a["lines"].as_u64().unwrap() as usize;
        assert!(kept > 60 && kept < MAX_LINES, "{kept}");
        assert!(a["truncated"].as_str().unwrap().contains("to stay under 64 KiB"), "{a}");
        // One long line, cut on a character's boundary, and said so.
        let mut t = Tail::new(10);
        t.push(&"é".repeat(MAX_LINE_BYTES));
        t.push("short");
        let a = t.answer(Engine::Docker, "web");
        let first = a["log"].as_str().unwrap().lines().next().unwrap();
        assert!(first.ends_with(" …[line cut]") && first.len() <= MAX_LINE_BYTES + 20, "{}", first.len());
        assert_eq!(a["truncated"], format!("1 line longer than {MAX_LINE_BYTES} bytes cut"));
        // A long line dropped off the front is no longer counted.
        for n in 0..10 {
            t.push(&format!("more {n}"));
        }
        assert_eq!(t.answer(Engine::Docker, "web")["truncated"], "only the last 10 of the 12 lines read");
    }

    #[test]
    fn a_failure_says_the_engines_reason() {
        assert_eq!(failure("docker", &["Error response from daemon: No such container: nope".into()], &[]), "docker: No such container: nope");
        assert_eq!(failure("podman", &["Error: no container with name or ID \"x\" found".into(), "".into()], &[]), "podman: no container with name or ID \"x\" found");
        assert_eq!(failure("wslc", &[], &[]), "wslc failed and said nothing");
    }
}
