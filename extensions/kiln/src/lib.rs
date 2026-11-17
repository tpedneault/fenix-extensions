//! Kiln in Fenix: a Kiln project's plan on pages, edits to its items,
//! exports and read tools for coding agents -- all by running the `kiln`
//! program (`--format json` to read; `set`, `link` and `new` to change).
//!
//! What it deliberately doesn't do: it never writes an item file itself,
//! compiles none of Kiln in, reaches no network and holds no Confluence
//! or Jira credential. Kiln's own checks and edits are the only ones;
//! this extension shows what kiln said and asks kiln to act.
//!
//! The parts that decide things are plain code, tested natively: `kiln`
//! (the queue that runs kiln), `model` (what kiln's JSON says), `project`
//! (kiln found, the project read, when to read it again), `lsp` (whether
//! and how Kiln's language server is registered) and `pages` (the pages,
//! from the project's state). This file meets Fenix: it hands Fenix's
//! events to the project, passes on what the project says changed, routes
//! each query's answer to whoever asked it ([`For`]), and does what `lsp`
//! decides. It stays the thin part that talks to the host.
//!
//! The project is read once as the extension loads (a `kiln.toml` at the
//! project's top, or a `kiln.*` command): the language server is wanted
//! from the start, not only once a page opens, and deciding it needs kiln
//! found and the project read.

pub mod kiln;
pub mod lsp;
pub mod model;
pub mod pages;
pub mod project;

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::{page as pages_api, Event, Extension, Key, Page};
use lsp::Lsp;
use project::{Notice, Project, QueryId, Why};

/// The page kind the plan opens as (the manifest's context names it).
const PLAN: &str = "plan";

/// The open project's folder, as `std::fs` sees it (`filesystem =
/// "project-read"`).
const PROJECT: &str = "/project";

/// Who asked a query ([`Project::query`]), so its answer goes to them.
/// One map for every asker: a page that asks adds its own variant (with
/// its page id) and is handed its answers in [`Kiln::pass_on`]. An answer
/// no one's waiting for -- its page closed -- is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum For {
    /// `kiln lsp --languages`, for [`Lsp`].
    Languages,
}

/// How many rounds [`Kiln::pass_on`] goes before it stops: each round
/// hands on what came and may ask once more; a few settle anything real.
const MOST_ROUNDS: usize = 16;

#[derive(Default)]
struct Kiln {
    project: Project,
    /// Its pages, and whether each is in view.
    pages: HashMap<u64, bool>,
    lsp: Lsp,
    /// The queries waiting for an answer, and who for.
    asked: HashMap<QueryId, For>,
    /// The project's folder when it isn't [`PROJECT`]: a test's.
    root: Option<PathBuf>,
    /// The language server's inputs moved (a save, `r`, another kiln)
    /// without the project saying anything: decide it on the next
    /// [`Kiln::pass_on`].
    decide: bool,
}

impl Kiln {
    /// `args` asked of kiln for `who`.
    fn ask<S: Into<String>>(&mut self, args: impl IntoIterator<Item = S>, who: For) -> QueryId {
        let id = self.project.query(args);
        self.asked.insert(id, who);
        id
    }

    /// What the project said, passed on: a change draws its pages again,
    /// an answer goes to whoever asked it. Then, when something came (or
    /// [`Kiln::decide`] says the server's inputs moved), the language
    /// server is decided again, which may ask kiln something whose answer
    /// may already be here -- so round again until nothing more comes.
    /// Not on every call: a line of kiln's output brings no notice, and
    /// deciding looks for the kiln program.
    fn pass_on(&mut self) {
        let mut changed = false;
        let mut decide = std::mem::take(&mut self.decide);
        for _ in 0..MOST_ROUNDS {
            let notices = self.project.take_notices();
            decide |= !notices.is_empty();
            for notice in notices {
                match notice {
                    Notice::Changed => changed = true,
                    Notice::Answer { id, result, .. } => match self.asked.remove(&id) {
                        Some(For::Languages) => {
                            self.lsp.answered(id, &result, self.project.version())
                        }
                        None => {}
                    },
                }
            }
            if !decide {
                break;
            }
            // Asking brings an answer, maybe at once: another round.
            decide = self.decide_lsp();
        }
        if changed {
            for &p in self.pages.keys() {
                pages_api::refresh(p);
            }
        }
    }

    /// What [`Lsp`] decides, done: whether it asked kiln.
    fn decide_lsp(&mut self) -> bool {
        let usability = lsp::usability(&self.project.state());
        let usable = usability == lsp::Usability::Usable;
        if usable && self.lsp.wants_tools() {
            self.lsp.tools(lsp::listed(self.read_tools()));
        }
        // The kiln the queue runs; looked for only when it's used.
        let program = if usable {
            kiln::program()
        } else {
            Err(String::new())
        };
        let step = self.lsp.step(&usability, program);
        for action in step.actions {
            match action {
                lsp::Action::Say(line) => api::info!("{line}"),
                lsp::Action::Register(server) => {
                    let languages: Vec<&str> =
                        server.languages.iter().map(String::as_str).collect();
                    let command = api::Command::new(&server.program).args(lsp::ARGS);
                    if let Err(why) =
                        api::project::add_language_server(lsp::NAME, &languages, Some(&command))
                    {
                        api::warn!("Fenix refused kiln's language server: {why}");
                        self.lsp.refused(server, why);
                    }
                }
                lsp::Action::Unregister(languages) => {
                    let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
                    // Taking one away can't be refused for a reason this
                    // could act on.
                    let _ = api::project::add_language_server(lsp::NAME, &languages, None);
                }
            }
        }
        if step.ask {
            let id = self.ask(lsp::ASK, For::Languages);
            self.lsp.asked(id);
        }
        step.ask
    }

    /// The project's `.fenix/tools.json`: its text, `None` when there's
    /// none, or why it can't be read. Fenix reads the same file for its
    /// own servers; this only looks at whether it runs kiln.
    fn read_tools(&self) -> Result<Option<String>, String> {
        let root = self.root.clone().unwrap_or_else(|| PathBuf::from(PROJECT));
        let path = root.join(".fenix").join("tools.json");
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(m) if m.len() > lsp::MAX_TOOLS_JSON => {
                return Err(format!("it's over {} KiB", lsp::MAX_TOOLS_JSON / 1024))
            }
            Ok(_) => {}
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// `r`, `kiln.reload`: the project read again, and the language
    /// server's languages and tools.json looked at again.
    fn reload(&mut self) {
        self.lsp.again();
        self.decide = true;
        self.project.reload(Why::Asked);
    }

    fn shown_changed(&mut self) {
        let any = self.pages.values().any(|&shown| shown);
        self.project.set_shown(any);
    }
}

impl Extension for Kiln {
    /// The project read at once, for the language server.
    fn activate(&mut self) -> Result<(), String> {
        self.project.start();
        self.decide = true;
        self.pass_on();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "kiln.open" => {
                match self.pages.keys().next() {
                    Some(&p) => pages_api::focus(p),
                    None => {
                        let p = pages_api::open(PLAN, "Kiln");
                        // Opened is in view; PageShown confirms it.
                        self.pages.insert(p, true);
                    }
                }
                self.project.start();
                self.shown_changed();
            }
            "kiln.reload" => self.reload(),
            _ => return Err(format!("no command {id}")),
        }
        self.pass_on();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::ProcessOutput(o) => self.project.output(o.process, o.stderr, &o.line),
            Event::ProcessExit((process, code)) => self.project.exited(process, code),
            Event::Timer(t) => {
                self.project.timer(t);
            }
            Event::PageShown((p, shown)) => {
                if let Some(s) = self.pages.get_mut(&p) {
                    *s = shown;
                    self.shown_changed();
                }
            }
            Event::SettingChanged(key) => match key.as_str() {
                "kiln.path" => {
                    self.project.kiln_changed();
                    self.lsp.kiln_changed();
                    self.decide = true;
                }
                "kiln.refresh_seconds" => self.project.refresh_changed(),
                _ => {}
            },
            _ => {}
        }
        self.pass_on();
    }

    fn file_saved(&mut self, path: &str) {
        self.lsp.saved(path);
        self.decide = true;
        self.project.saved(path);
        self.pass_on();
    }

    fn page(&mut self, _page: u64, _width: u32, _height: u32) -> Page {
        pages::plan::page(self.project.state())
    }

    fn page_key(&mut self, _page: u64, key: Key) {
        if key == Key::Char('r') {
            self.reload();
            self.pass_on();
        }
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
        self.shown_changed();
        self.pass_on();
    }
}

api::export!(Kiln);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// A fake whose kiln answers as the demo project's did, check's exit 1
    /// included.
    fn demo() -> Fake {
        let fake = Fake::new();
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Ran::ok(fixture("version.txt")))
            .on_spawn(
                "kiln list --format json",
                Ran::ok(fixture("demo/list.json")),
            )
            .on_spawn(
                "kiln status --format json",
                Ran::ok(fixture("demo/status.json")),
            )
            .on_spawn(
                "kiln check --format json",
                Ran::exits(1)
                    .stdout(fixture("demo/check.json"))
                    .stderr(fixture("demo/check.stderr")),
            )
            // kiln 1.0.0's refusal, as recorded.
            .on_spawn(
                "kiln lsp --languages",
                Ran::fails(2, fixture("demo/lsp-languages.stderr")),
            );
        fake
    }

    fn spawned(fake: &Fake) -> Vec<String> {
        fake.take()
            .into_iter()
            .filter_map(|l| {
                l.strip_prefix("spawn ")
                    .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string())
            })
            .collect()
    }

    const LOADED: &str = "Demo\n16 items · kiln 1.0.0\nMS-001 First release  1 of 5 done\nMS-002 Second release  0 of 2 done\nno milestone  0 of 1 done\n1 error, 2 warnings\n\nr read again\n";

    #[test]
    fn open_reads_the_project_and_shows_it() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        assert_eq!(h.screen(page), LOADED);
        assert_eq!(
            spawned(&fake)
                .iter()
                .filter(|l| l.starts_with("/usr/bin/kiln"))
                .count(),
            // version, list, status, check, then lsp --languages.
            5
        );
    }

    #[test]
    fn loading_shows_while_kiln_reads() {
        let fake = demo();
        fake.on_spawn("kiln list", Ran::running(""));
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        assert_eq!(
            h.screen(page),
            "Kiln\nreading the project…\n\nr read again\n"
        );
    }

    #[test]
    fn output_in_chunks_and_check_exiting_1_still_loads() {
        let fake = demo();
        for pattern in ["kiln list", "kiln status", "kiln check"] {
            fake.on_spawn(pattern, Ran::running(""));
        }
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        for (pattern, file, code) in [
            ("kiln list", "demo/list.json", 0),
            ("kiln status", "demo/status.json", 0),
            ("kiln check", "demo/check.json", 1),
        ] {
            let p = fake.process(pattern).unwrap().id;
            for line in fixture(file).lines() {
                h.said(p, line, false);
            }
            h.said(p, "kiln: 1 error, 2 warnings", true);
            h.exited(p, Some(code));
        }
        assert_eq!(h.screen(page), LOADED);
    }

    #[test]
    fn kiln_missing_says_how_to_set_kiln_path() {
        let fake = Fake::new();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        assert!(h.screen(page).starts_with(
            "Kiln\nkiln isn't on the PATH: install Kiln, or set kiln.path to where kiln is\n"
        ));
        // Set, it's looked for again at once.
        fake.on_spawn("/opt/kiln/kiln --version", Ran::ok("kiln 1.0.0\n"))
            .on_spawn("/opt/kiln/kiln list", Ran::ok(fixture("demo/list.json")));
        h.set_setting("kiln.path", Some("/opt/kiln/kiln"));
        assert!(fake
            .calls()
            .iter()
            .any(|c| c.ends_with("/opt/kiln/kiln list --format json")));
    }

    #[test]
    fn a_kiln_error_is_shown_as_kiln_wrote_it() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, "kiln: kiln.toml:4: unknown key `nmae`"),
        );
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        assert_eq!(
            h.screen(page),
            "Kiln\nkiln: kiln.toml:4: unknown key `nmae`\n\nr read again\n"
        );
    }

    #[test]
    fn a_failing_reload_keeps_the_loaded_project() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        fake.on_spawn(
            "kiln list",
            Ran::fails(
                2,
                "kiln: design/tasks/TASK-001-set-up-the-logger.md:1: front matter can't be parsed",
            ),
        );
        h.saved("/home/me/demo/design/tasks/TASK-001-set-up-the-logger.md");
        let s = h.screen(page);
        assert!(s.starts_with("Demo\n16 items"), "{s}");
        assert!(s.contains("the last read failed: kiln: design/tasks/TASK-001-set-up-the-logger.md:1: front matter can't be parsed\n"), "{s}");
    }

    #[test]
    fn two_reloads_while_one_runs_run_one_more() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        fake.take();
        fake.on_spawn("kiln list", Ran::running(fixture("demo/list.json")));
        h.command("kiln.reload").unwrap();
        h.command("kiln.reload").unwrap();
        let page = h.page("plan");
        h.key(page, Key::Char('r'));
        assert!(h.screen(page).ends_with("reading again…\n\nr read again\n"));
        let first = fake.process("kiln list").unwrap().id;
        fake.on_spawn("kiln list", Ran::ok(fixture("demo/list.json")));
        h.exited(first, Some(0));
        let lists = spawned(&fake)
            .iter()
            .filter(|l| l.ends_with("kiln list --format json"))
            .count();
        assert_eq!(lists, 2);
        assert_eq!(h.screen(page), LOADED);
    }

    #[test]
    fn kiln_reload_before_open_reads_it() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.reload").unwrap();
        assert_eq!(spawned(&fake).len(), 5);
    }

    #[test]
    fn the_refresh_timer_follows_the_page_in_view() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        let refresh = |f: &Fake| {
            f.timers()
                .into_iter()
                .find(|t| t.millis == 5000 && t.repeat)
        };
        let timer = refresh(&fake).expect("a 5 s refresh while it's in view").id;
        fake.take();
        h.fire(timer);
        assert_eq!(spawned(&fake).len(), 3);
        h.event(Event::PageShown((page, false)));
        assert!(refresh(&fake).is_none());
        h.event(Event::PageShown((page, true)));
        assert!(refresh(&fake).is_some());
        h.close(page);
        assert!(refresh(&fake).is_none());
    }

    // -- The language server ------------------------------------------------

    /// A project folder of the test's own, for `.fenix/tools.json`: empty
    /// at first.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-kiln-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".fenix")).unwrap();
        dir
    }

    fn harness(fake: &Fake, dir: &std::path::Path) -> Harness<Kiln> {
        Harness::wrap(
            fake.clone(),
            Kiln {
                root: Some(dir.to_path_buf()),
                ..Kiln::default()
            },
        )
    }

    /// What it told Fenix about its server, in order.
    fn servers(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.starts_with("add kiln "))
            .collect()
    }

    fn infos(fake: &Fake) -> Vec<String> {
        fake.logs()
            .into_iter()
            .filter_map(|l| l.strip_prefix("info ").map(str::to_string))
            .collect()
    }

    /// kiln answering `lsp --languages` as the unreleased command would
    /// (hand-written: tests/fixtures/hand/README).
    fn answers_languages(fake: &Fake, languages: &[&str]) {
        let json = serde_json::json!({"kiln_json": 1, "command": "kiln", "args": ["lsp"], "languages": languages});
        fake.on_spawn("kiln lsp --languages", Ran::ok(json.to_string()));
    }

    const TOOLS: &str = r#"{"lsp": {"additional": [{"name": "kiln", "executable": "kiln", "args": ["lsp"], "languages": ["markdown"]}]}}"#;

    #[test]
    fn loaded_it_registers_the_server_with_no_page_open() {
        let fake = demo();
        let hand = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/hand/lsp-languages.json"
        ))
        .unwrap();
        fake.on_spawn("kiln lsp --languages", Ran::ok(hand));
        let dir = folder("registers");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(
            servers(&fake),
            ["add kiln markdown,rust,tcl: /usr/bin/kiln lsp"]
        );
        assert!(spawned(&fake).contains(&"/usr/bin/kiln lsp --languages --format json".to_string()));
        assert_eq!(
            h.ext.lsp.status(),
            &lsp::Status::Registered {
                server: lsp::Server {
                    languages: vec!["markdown".into(), "rust".into(), "tcl".into()],
                    program: "/usr/bin/kiln".into()
                },
                note: None
            }
        );
    }

    #[test]
    fn kiln_1_0_gets_markdown_only_said_once() {
        let fake = demo();
        let dir = folder("old");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        let said = |f: &Fake| {
            infos(f)
                .iter()
                .filter(|l| l.starts_with("kiln 1.0.0 has no `kiln lsp --languages`"))
                .count()
        };
        assert_eq!(said(&fake), 1);
        // Asked again on r: refused again, not said or set again.
        h.command("kiln.reload").unwrap();
        assert_eq!(said(&fake), 1);
        assert_eq!(servers(&fake).len(), 1);
    }

    #[test]
    fn plain_reloads_dont_restart_it() {
        let fake = demo();
        let dir = folder("plain");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        h.saved("/home/me/demo/design/tasks/TASK-001-set-up-the-logger.md");
        h.command("kiln.reload").unwrap();
        h.key(page, Key::Char('r'));
        h.saved("/home/me/demo/kiln.toml");
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
    }

    #[test]
    fn new_languages_restart_it_once() {
        let fake = demo();
        let dir = folder("languages");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        answers_languages(&fake, &["markdown", "python"]);
        // A code file saved doesn't ask; kiln.toml does.
        h.saved("/home/me/demo/src/app.py");
        assert_eq!(servers(&fake).len(), 1);
        h.saved("/home/me/demo/kiln.toml");
        h.saved("/home/me/demo/kiln.toml");
        assert_eq!(
            servers(&fake),
            [
                "add kiln markdown: /usr/bin/kiln lsp",
                "add kiln markdown,python: /usr/bin/kiln lsp"
            ]
        );
    }

    #[test]
    fn listed_in_tools_json_it_steps_aside_until_the_entry_goes() {
        let fake = demo();
        let dir = folder("listed");
        std::fs::write(dir.join(".fenix/tools.json"), TOOLS).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        // Fenix told there's none of ours; nothing registered.
        assert_eq!(servers(&fake), ["add kiln : "]);
        assert!(matches!(
            h.ext.lsp.status(),
            lsp::Status::ToolsJson(lsp::Form::Additional)
        ));
        assert_eq!(
            infos(&fake)
                .iter()
                .filter(|l| l
                    .starts_with("the project's .fenix/tools.json runs kiln's language server"))
                .count(),
            1
        );
        // The entry removed, the file saved: ours.
        std::fs::write(dir.join(".fenix/tools.json"), "{}").unwrap();
        h.saved(&dir.join(".fenix/tools.json").display().to_string());
        assert_eq!(
            servers(&fake)[1..],
            ["add kiln markdown: /usr/bin/kiln lsp"]
        );
        // Listed again (the older form): ours taken away.
        std::fs::write(
            dir.join(".fenix/tools.json"),
            r#"{"lsp": {"markdown": {"executable": "kiln", "args": ["lsp"]}}}"#,
        )
        .unwrap();
        h.command("kiln.reload").unwrap();
        assert_eq!(servers(&fake)[2..], ["add kiln markdown: "]);
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::ToolsJson(lsp::Form::Primary(l)) if l == "markdown")
        );
    }

    #[test]
    fn a_malformed_tools_json_is_said_and_ours_registered() {
        let fake = demo();
        let dir = folder("malformed");
        std::fs::write(dir.join(".fenix/tools.json"), "{\"lsp\": ").unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        assert!(infos(&fake)
            .iter()
            .any(|l| l.starts_with(".fenix/tools.json isn't valid JSON: ")));
        // No tools.json at all is the same, unsaid.
        let fake = demo();
        let dir = folder("absent");
        std::fs::remove_dir_all(dir.join(".fenix")).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln markdown: /usr/bin/kiln lsp"]);
        assert!(!infos(&fake)
            .iter()
            .any(|l| l.contains("valid JSON") || l.contains("can't be read")));
    }

    #[test]
    fn kiln_lost_takes_it_away_and_another_kiln_sets_it_once() {
        let fake = demo();
        let dir = folder("lost");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        fake.on_spawn_error("/nowhere/kiln", "no such file");
        h.set_setting("kiln.path", Some("/nowhere/kiln"));
        assert_eq!(servers(&fake)[1..], ["add kiln markdown: "]);
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::NotRegistered(w) if w.starts_with("kiln.path is /nowhere/kiln, which can't be started"))
        );
        // A kiln there: set once, with it and its languages.
        fake.on_spawn("/opt/kiln --version", Ran::ok("kiln 1.1.0\n"))
            .on_spawn("/opt/kiln list", Ran::ok(fixture("demo/list.json")))
            .on_spawn("/opt/kiln status", Ran::ok(fixture("demo/status.json")))
            .on_spawn("/opt/kiln check", Ran::ok(fixture("demo/check.json")))
            .on_spawn(
                "/opt/kiln lsp --languages",
                Ran::ok(
                    serde_json::json!({"kiln_json": 1, "languages": ["markdown", "rust"]})
                        .to_string(),
                ),
            );
        h.set_setting("kiln.path", Some("/opt/kiln"));
        assert_eq!(
            servers(&fake)[2..],
            ["add kiln markdown,rust: /opt/kiln lsp"]
        );
    }

    #[test]
    fn not_a_kiln_project_registers_nothing() {
        let fake = demo();
        fake.on_spawn(
            "kiln list",
            Ran::fails(2, fixture("errors/no-kiln-toml.stderr")),
        );
        let dir = folder("none");
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(servers(&fake), ["add kiln : "]);
        // Nothing asked of kiln about languages there.
        assert!(!spawned(&fake).iter().any(|l| l.contains(" lsp ")));
        assert!(
            matches!(h.ext.lsp.status(), lsp::Status::NotRegistered(w) if w.starts_with("kiln: no kiln.toml"))
        );
    }

    #[test]
    fn an_answer_no_one_waits_for_is_dropped() {
        let fake = demo();
        let mut h: Harness<Kiln> = Harness::with_fake(fake.clone());
        h.activate().unwrap();
        let id = h.ext.project.query(["show", "REQ-001"]);
        h.ext.pass_on();
        h.settle();
        assert!(!h.ext.asked.contains_key(&id));
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        let mut h: Harness<Kiln> = Harness::new();
        assert_eq!(
            h.command("kiln.nope"),
            Err("no command kiln.nope".to_string())
        );
    }
}
