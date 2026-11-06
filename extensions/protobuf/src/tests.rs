//! The pack wired up, on lang-kit's test host and an editor of the
//! tests' own: what it sets as it starts, buf lint on save, the doctor's
//! breaking check, and the gRPC client from listing a server's services to
//! a call's answer in the history.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::*;
use crate::project::tests::temp;
use fenix_extension_api::{Health, ProcessOutput};
use fenix_lang_kit::host::mock::Shared;

/// A command as a line, its program first.
pub fn line(cmd: &Command) -> String {
    std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

/// The editor the tests give it: scratch buffers kept here, the clock
/// set by hand.
#[derive(Default)]
struct Ed {
    next: u64,
    scratch: HashMap<u64, (String, String)>,
    settings: Vec<(String, String)>,
    copied: Vec<String>,
    opened: Vec<String>,
    now: u64,
}

#[derive(Clone, Default)]
struct SharedEd(Rc<RefCell<Ed>>);

impl Editor for SharedEd {
    fn scratch_open(&mut self, title: &str, text: &str) -> u64 {
        let mut e = self.0.borrow_mut();
        e.next += 1;
        let id = 1000 + e.next;
        e.scratch.insert(id, (title.into(), text.into()));
        id
    }
    fn scratch_text(&mut self, id: u64) -> Option<String> {
        self.0.borrow().scratch.get(&id).map(|s| s.1.clone())
    }
    fn scratch_set(&mut self, id: u64, text: &str) {
        if let Some(s) = self.0.borrow_mut().scratch.get_mut(&id) {
            s.1 = text.into();
        }
    }
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.0.borrow_mut().settings.push((key.into(), value.into()));
        Ok(())
    }
    fn copy(&mut self, text: &str) {
        self.0.borrow_mut().copied.push(text.into());
    }
    fn open_text(&mut self, title: &str, _text: &str) {
        self.0.borrow_mut().opened.push(title.into());
    }
    fn focus_page(&mut self, page: u64) {
        self.0.borrow_mut().opened.push(format!("focus {page}"));
    }
    fn now_ms(&mut self) -> u64 {
        self.0.borrow().now
    }
}

fn found(host: &Shared, programs: &[(&str, &str)]) {
    host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
}

/// The processes started, by id, each as a line.
fn spawned(calls: &[String]) -> Vec<(u64, String)> {
    calls.iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse().unwrap(), l.to_string())).collect()
}

fn find(calls: &[String], end: &str) -> u64 {
    spawned(calls).into_iter().find(|(_, l)| l.ends_with(end)).unwrap_or_else(|| panic!("{end}: {calls:?}")).0
}

fn said(p: &mut Protobuf, process: u64, text: &str, code: i32) {
    for l in text.lines() {
        p.handle(Event::ProcessOutput(ProcessOutput { process, stderr: false, line: l.into() }));
    }
    p.handle(Event::ProcessExit((process, Some(code))));
}

/// A buf workspace in a repository whose default branch is main.
fn workspace(name: &str) -> std::path::PathBuf {
    let dir = temp(name);
    std::fs::create_dir_all(dir.join("proto/orbit/v1")).unwrap();
    std::fs::create_dir_all(dir.join(".git/refs/heads")).unwrap();
    std::fs::write(dir.join(".git/refs/heads/main"), "abc\n").unwrap();
    std::fs::write(dir.join("buf.yaml"), "version: v2\nmodules:\n  - path: proto\n").unwrap();
    std::fs::write(dir.join("buf.gen.yaml"), include_str!("samples/buf.gen.yaml")).unwrap();
    std::fs::write(dir.join("proto/orbit/v1/orbit.proto"), include_str!("samples/orbit.proto")).unwrap();
    dir
}

fn targets() -> Settings {
    Settings { targets: vec![Target::new("localhost:50071", false)], ..Settings::default() }
}

fn pack(dir: &std::path::Path, host: &Shared, ed: &SharedEd, settings: fn() -> Settings) -> Protobuf {
    let mut p = Protobuf::new(Box::new(host.clone()), Box::new(ed.clone()), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
    p.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
    p.history_path = dir.join(".data/history.json").to_string_lossy().into_owned();
    p
}

#[test]
fn bufs_server_formatter_tasks_and_breaking_check_are_set() {
    let dir = workspace("activate");
    let host = Shared::default();
    found(&host, &[("buf", "/t/buf"), ("grpcurl", "/t/grpcurl")]);
    let mut p = pack(&dir, &host, &SharedEd::default(), Settings::default);
    p.activate().unwrap();
    let calls = host.take();
    let first = calls.clone();
    for want in ["tasks build, lint, format, breaking against main, generate", "server proto: /t/buf lsp serve", "formatter proto: /t/buf format --stdin-filepath ${file}"] {
        assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
    }
    let lines: Vec<String> = spawned(&calls).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, ["/t/buf --version", "/t/grpcurl -version", "/t/buf breaking --against .git#branch=main --error-format json"]);
    let task = p.kit.tasks.resolved().into_iter().find(|t| t.name == "generate").unwrap();
    assert_eq!(task.cmd.cwd.as_deref(), Some("/project"));

    // An older buf: its beta server, and no formatter of its own.
    said(&mut p, find(&calls, "buf --version"), "1.58.0", 0);
    said(&mut p, find(&calls, "grpcurl -version"), "grpcurl v1.9.4", 0);
    let calls = host.take();
    assert!(calls.iter().any(|c| c == "server proto: /t/buf beta lsp") && calls.iter().any(|c| c == "formatter proto: "), "{calls:?}");
    let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
    assert_eq!(&checks[..3], [("buf".into(), Health::Ok), ("protoc".into(), Health::Warn), ("grpcurl (the gRPC client)".into(), Health::Ok)]);
    assert_eq!(checks[3], ("Breaking changes".into(), Health::Info), "still comparing");

    // What buf breaking found, a check of the project's with a fix row.
    let breaking = find(&first, "--error-format json");
    said(&mut p, breaking, include_str!("samples/buf-breaking.json"), 100);
    let check = host.0.borrow().checks.iter().find(|c| c.label == "Breaking changes").cloned().unwrap();
    assert_eq!((check.health, check.detail.as_str()), (Health::Warn, "2 against main: FIELD_NO_DELETE, FIELD_SAME_TYPE"));
    assert_eq!(check.fix.map(|f| f.1), Some("protobuf.breaking".into()));
    host.take();
    p.run_command("protobuf.breaking").unwrap();
    let calls = host.take();
    assert!(calls.iter().any(|c| c.ends_with("breaking against main: /t/buf breaking --against .git#branch=main")), "the task: {calls:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_saved_proto_is_linted_across_the_workspace_and_compared_again() {
    let dir = workspace("lint");
    let host = Shared::default();
    found(&host, &[("buf", "/t/buf")]);
    let mut p = pack(&dir, &host, &SharedEd::default(), Settings::default);
    p.activate().unwrap();
    host.take();
    let saved = format!("{}/proto/orbit/v1/orbit.proto", dir.display());
    host.0.borrow_mut().here = Some((saved.clone(), Some(dir.to_string_lossy().into_owned())));
    p.file_saved(&saved);
    let calls = host.take();
    let lint = find(&calls, "/t/buf lint --error-format json");
    said(&mut p, lint, include_str!("samples/buf-lint.json"), 100);
    let problems = host.0.borrow().problems.clone();
    let mut files: Vec<(&str, usize)> = problems.iter().map(|(f, ps)| (f.as_str(), ps.len())).collect();
    files.sort();
    assert_eq!(files, [("/project/proto/orbit/v1/ground.proto", 5), ("/project/proto/orbit/v1/orbit.proto", 1)]);

    // Saved, by name: compared with main again.
    p.handle(Event::Saved("proto/orbit/v1/orbit.proto".into()));
    assert!(spawned(&host.take()).iter().any(|(_, l)| l.contains("buf breaking")));
    // buf.yaml saved: the project read again, its tasks set.
    std::fs::write(dir.join("buf.gen.docs.yaml"), "version: v2\nplugins:\n  - local: protoc-gen-doc\n    out: docs\n").unwrap();
    p.handle(Event::Saved("buf.yaml".into()));
    assert!(host.take().iter().any(|c| c == "tasks build, lint, format, breaking against main, generate, generate docs"));

    // Off: no linter.
    p.read_settings = || Settings { lint: false, ..Settings::default() };
    p.handle(Event::SettingChanged("protobuf.lint".into()));
    assert!(p.kit.lint.linters.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn without_buf_or_grpcurl_their_fix_rows_install_them() {
    let dir = temp("fix");
    std::fs::write(dir.join("orbit.proto"), "syntax = \"proto3\";\n").unwrap();
    let host = Shared::default();
    let ed = SharedEd::default();
    let mut p = pack(&dir, &host, &ed, Settings::default);
    p.activate().unwrap();
    let calls = host.take();
    assert!(!calls.iter().any(|c| c.starts_with("server proto: /") || c.starts_with("formatter proto: /")), "nothing found, nothing set: {calls:?}");
    let fixes: Vec<Option<String>> = p.kit.doctor.programs.iter().map(|pr| pr.fix.clone().map(|f| f.1)).collect();
    assert_eq!(fixes, [Some("protobuf.install.buf".into()), Some("protobuf.install.protoc".into()), Some("protobuf.install.grpcurl".into())]);
    p.run_command("protobuf.install.grpcurl").unwrap();
    let download = host.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
    assert!(download.contains("grpcurl_1.9.4_linux_x86_64.tar.gz") && download.ends_with(" 97e13d58d2733a0e62cd2571d1d5f0c02823f0d25282f08bddedf1ad9c5d1736"), "{download}");
    assert!(p.run_command("protobuf.breaking").is_err(), "no repository");
    assert!(p.run_command("protobuf.nothing").is_err());

    // The client says what's missing.
    p.run_command("protobuf.client").unwrap();
    let text = p.client_page(100, 24).text();
    assert!(text.contains("grpcurl isn't installed: I installs it"), "{text}");

    // Installed some other way: found and used; protols when it's all
    // there is.
    found(&host, &[("protols", "/h/.cargo/bin/protols"), ("protoc", "/t/protoc")]);
    p.handle(Event::TaskFinished((99, true)));
    let calls = host.take();
    assert!(calls.iter().any(|c| c == "server proto: /h/.cargo/bin/protols"), "{calls:?}");
    assert!(calls.iter().any(|c| c == "tasks protoc check"), "{calls:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_client_lists_a_servers_methods_makes_a_body_and_calls_one() {
    let dir = workspace("client");
    let host = Shared::default();
    let ed = SharedEd::default();
    found(&host, &[("buf", "/t/buf"), ("grpcurl", "/t/grpcurl")]);
    let mut p = pack(&dir, &host, &ed, targets);
    assert!(!p.client.protos, "a server to ask: its reflection first");
    p.activate().unwrap();
    host.take();
    p.run_command("protobuf.client").unwrap();
    let calls = host.take();
    let page = calls.iter().find_map(|c| c.strip_prefix("page ")?.split_once(' ')?.0.parse::<u64>().ok()).unwrap();
    assert_eq!(p.client.loading.as_deref(), Some("listing the services (reflection)…"));
    said(&mut p, find(&calls, "/t/grpcurl -plaintext -connect-timeout 10 localhost:50071 list"), include_str!("samples/grpcurl-list.txt"), 0);
    let calls = host.take();
    said(&mut p, find(&calls, "describe orbit.v1.OrbitService"), include_str!("samples/grpcurl-describe-service.txt"), 0);
    let text = p.client_page(110, 30).text();
    assert!(text.contains("▾ OrbitService") && text.contains("WatchPasses"), "{text}");

    // Chosen: its input described, its body made and opened beside.
    p.page_key(page, Key::Down);
    p.page_key(page, Key::Enter);
    assert_eq!(p.client.path().as_deref(), Some("orbit.v1.OrbitService/GetSatellite"));
    let calls = host.take();
    said(&mut p, find(&calls, "describe orbit.v1.GetSatelliteRequest"), "orbit.v1.GetSatelliteRequest is a message:\nmessage GetSatelliteRequest {\n  string id = 1;\n}\n", 0);
    let (scratch, (title, body)) = ed.0.borrow().scratch.iter().next().map(|(k, v)| (*k, v.clone())).unwrap();
    assert_eq!((title.as_str(), body.as_str()), ("gRPC: GetSatellite", "{\n  \"id\": \"\"\n}\n"));

    // Edited, with metadata, and called.
    ed.0.borrow_mut().scratch.get_mut(&scratch).unwrap().1 = "{\"id\": \"25544\"}".into();
    p.buffer_changed(Changed::Scratch(scratch));
    p.client.metadata.insert("orbit.v1.OrbitService/GetSatellite".into(), vec![("x-team".into(), "ops".into())]);
    ed.0.borrow_mut().now = 1_700_000_000_000;
    host.take();
    p.page_key(page, Key::Char('r'));
    let calls = host.take();
    let call = find(&calls, "-v -max-time 30 -H x-team: ops -d {\"id\": \"25544\"} localhost:50071 orbit.v1.OrbitService/GetSatellite");
    for (i, l) in include_str!("samples/grpcurl-unary.txt").lines().enumerate() {
        ed.0.borrow_mut().now = 1_700_000_000_000 + 2 * i as u64;
        p.handle(Event::ProcessOutput(ProcessOutput { process: call, stderr: false, line: l.into() }));
    }
    assert!(host.take().iter().any(|c| c == &format!("refresh {page}")), "drawn as it came");
    ed.0.borrow_mut().now = 1_700_000_000_090;
    p.handle(Event::ProcessExit((call, Some(0))));
    let text = p.client_page(110, 30).text();
    assert!(text.contains("OK · 90 ms · 1 message") && text.contains("\"name\": \"ISS ops\""), "{text}");

    // In the history, and saved.
    let kept = History::load(&p.history_path);
    let e = &kept.of("orbit.v1.OrbitService/GetSatellite")[0];
    assert_eq!((e.code.as_str(), e.ms, e.body.as_str(), e.target.as_str(), e.at), ("OK", 90, "{\"id\": \"25544\"}", "localhost:50071", 1_700_000_000));
    assert!(e.first.contains("\"satellite\""));

    // A body that isn't JSON isn't sent.
    ed.0.borrow_mut().scratch.get_mut(&scratch).unwrap().1 = "{\"id\": ".into();
    assert!(p.run_command("protobuf.invoke").unwrap_err().starts_with("the body isn't JSON"));
    // From the history: its body back in the buffer.
    p.action(Action::FromHistory(0)).unwrap();
    assert_eq!(ed.0.borrow().scratch[&scratch].1, "{\"id\": \"25544\"}");
    // Made again from the message.
    p.action(Action::ResetBody).unwrap();
    assert_eq!(ed.0.borrow().scratch[&scratch].1, "{\n  \"id\": \"\"\n}\n");
    p.action(Action::CopyResponse).unwrap();
    assert!(ed.0.borrow().copied[0].contains("ISS ops"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_server_stream_comes_a_message_at_a_time_and_cancels() {
    let dir = workspace("stream");
    let host = Shared::default();
    let ed = SharedEd::default();
    found(&host, &[("buf", "/t/buf"), ("grpcurl", "/t/grpcurl")]);
    let mut p = pack(&dir, &host, &ed, targets);
    p.activate().unwrap();
    host.take();
    p.run_command("protobuf.client").unwrap();
    let calls = host.take();
    said(&mut p, find(&calls, "list"), include_str!("samples/grpcurl-list.txt"), 0);
    let calls = host.take();
    said(&mut p, find(&calls, "describe orbit.v1.OrbitService"), include_str!("samples/grpcurl-describe-service.txt"), 0);
    p.action(Action::Select("orbit.v1.OrbitService".into(), "WatchPasses".into())).unwrap();
    let calls = host.take();
    said(&mut p, find(&calls, "describe orbit.v1.WatchPassesRequest"), "orbit.v1.WatchPassesRequest is a message:\nmessage WatchPassesRequest {\n  string satellite_id = 1;\n  .orbit.v1.Station station = 2;\n}\n", 0);
    let calls = host.take();
    said(&mut p, find(&calls, "describe orbit.v1.Station"), "orbit.v1.Station is a message:\nmessage Station {\n  double lat = 1;\n  double lon = 2;\n}\n", 0);
    let body = ed.0.borrow().scratch.values().next().unwrap().1.clone();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap(), serde_json::json!({"satelliteId": "", "station": {"lat": 0, "lon": 0}}), "described as far as it goes");
    host.take();
    p.action(Action::Invoke).unwrap();
    let call = find(&host.take(), "orbit.v1.OrbitService/WatchPasses");
    let text = include_str!("samples/grpcurl-stream.txt");
    let cut = text.find("Response contents:\n{\n  \"rise\": \"2023-11-14T23:43:20Z\"").unwrap();
    for (i, l) in text[..cut].lines().enumerate() {
        ed.0.borrow_mut().now = 10 * i as u64;
        p.handle(Event::ProcessOutput(ProcessOutput { process: call, stderr: false, line: l.into() }));
    }
    let shown = p.client_page(110, 30).text();
    assert!(shown.contains("Calling… 1 message"), "the first, before the rest: {shown}");
    host.take();
    p.run_command("protobuf.cancel").unwrap();
    assert!(host.take().contains(&format!("kill {call}")));
    assert!(p.client_page(110, 30).text().contains("Canceled: cancelled here"));
    // A client-streaming body is several messages; WatchPasses takes one.
    ed.0.borrow_mut().scratch.values_mut().next().unwrap().1 = "{} {}".into();
    assert_eq!(p.run_command("protobuf.invoke").unwrap_err(), "WatchPasses takes one message; the body has 2");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_projects_protos_are_built_by_buf_and_servers_added() {
    let dir = workspace("protos");
    let host = Shared::default();
    let ed = SharedEd::default();
    found(&host, &[("buf", "/t/buf"), ("grpcurl", "/t/grpcurl")]);
    let mut p = pack(&dir, &host, &ed, Settings::default);
    assert!(p.client.protos, "a workspace, no server: its protos");
    p.activate().unwrap();
    host.take();
    p.run_command("protobuf.client").unwrap();
    let calls = host.take();
    assert_eq!(p.client.loading.as_deref(), Some("building the project's protos (buf build)…"));
    said(&mut p, find(&calls, "/t/buf build --as-file-descriptor-set -o ${data}/schema.binpb"), "", 0);
    let calls = host.take();
    let list = find(&calls, "/t/grpcurl -protoset ${data}/schema.binpb list");
    said(&mut p, list, "orbit.v1.OrbitService\n", 0);
    host.take();

    // No server yet: a adds one, kept in the setting.
    let page = p.client.page.unwrap();
    p.page_key(page, Key::Char('a'));
    let prompt = host.take().iter().find_map(|c| c.strip_prefix("prompt ")?.split_once(' ')?.0.parse::<u64>().ok()).unwrap();
    p.handle(Event::Prompted((prompt, Some("tls://grpc.example.com:443".into()))));
    assert_eq!(ed.0.borrow().settings.last().unwrap(), &("protobuf.targets".to_string(), r#"[["grpc.example.com:443","on"]]"#.to_string()));
    assert!(p.client_page(100, 24).line(0).contains("grpc.example.com:443  TLS"));
    p.page_key(page, Key::Char('t'));
    assert_eq!(ed.0.borrow().settings.last().unwrap().1, r#"[["grpc.example.com:443","off"]]"#, "t turns TLS off");

    // s: its reflection instead; a server without it says so.
    host.take();
    p.page_key(page, Key::Char('s'));
    let list = find(&host.take(), "-plaintext -connect-timeout 10 grpc.example.com:443 list");
    p.handle(Event::ProcessOutput(ProcessOutput { process: list, stderr: true, line: "Failed to list services: server does not support the reflection API".into() }));
    p.handle(Event::ProcessExit((list, Some(1))));
    assert_eq!(p.client.error.as_deref(), Some("Failed to list services: server does not support the reflection API (s uses the project's protos)"));

    // Kept with the session, and back.
    let state = p.page_state(page).unwrap();
    let mut again = pack(&dir, &host, &ed, Settings::default);
    assert!(again.page_restore(77, CLIENT, &state));
    assert_eq!((again.client.page, again.client.protos), (Some(77), false));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bodies_are_one_or_more_json_messages() {
    assert_eq!(messages_in("{\"a\": 1}"), Ok(1));
    assert_eq!(messages_in("{} {}\n{}"), Ok(3), "a client stream's");
    assert!(messages_in("").unwrap_err().contains("empty"));
    assert!(messages_in("{").unwrap_err().starts_with("the body isn't JSON"));
}
