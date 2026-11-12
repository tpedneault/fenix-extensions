//! A small extension, written as one is -- against `api` -- and tested
//! through the harness: the commits of the project, from `git log`, on a
//! page; one copied, one shown by another extension's service, the count
//! asked for and kept as a setting, and the list read again on a timer.

use fenix_test_kit::{api, assert_snapshot, render, Fake, Harness, Ran};

use api::{page, Event, Extension, Key, Page, PickerItem};

const KIND: &str = "log";

#[derive(Default)]
struct Log {
    page: Option<u64>,
    git: Option<u64>,
    lines: Vec<String>,
    commits: Vec<(String, String)>,
    focus: usize,
    failed: Option<String>,
    pick: Option<u64>,
    count_prompt: Option<u64>,
    shown: Option<u64>,
    timer: Option<u64>,
}

impl Log {
    fn count() -> u32 {
        api::setting("log.count").and_then(|c| c.parse().ok()).unwrap_or(3)
    }

    fn read(&mut self) {
        let cmd = api::Command::new("git").args(["log", "--oneline", "-n", &Log::count().to_string()]).cwd("/project");
        match api::host::spawn(&cmd) {
            Ok(id) => {
                self.git = Some(id);
                self.lines.clear();
                api::status(Some("log: reading"));
            }
            Err(why) => api::warn!("git: {why}"),
        }
    }
}

impl Extension for Log {
    fn activate(&mut self) -> Result<(), String> {
        self.timer = Some(api::host::set_timer(60_000, true));
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "log.open" => {
                self.page = Some(page::open(KIND, "Log"));
                self.read();
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::ProcessOutput(o) if Some(o.process) == self.git && !o.stderr => self.lines.push(o.line),
            Event::ProcessExit((id, code)) if Some(id) == self.git => {
                self.git = None;
                api::status(None);
                if code == Some(0) {
                    self.commits = self.lines.iter().filter_map(|l| l.split_once(' ')).map(|(h, s)| (h.to_string(), s.to_string())).collect();
                    self.failed = None;
                } else {
                    self.failed = Some(format!("git log exited {code:?}"));
                    api::host::notify(api::Level::Error, "git log failed");
                }
                if let Some(p) = self.page {
                    page::refresh(p);
                }
            }
            Event::Picked((id, Some(hash))) if Some(id) == self.pick => {
                self.shown = Some(api::call_service("review.show", &format!("{{\"commit\":\"{hash}\"}}")));
            }
            Event::ServiceReply((id, reply)) if Some(id) == self.shown => match reply {
                Ok(_) => api::notify("shown in Review"),
                Err(why) => api::notify(&format!("Review can't show it: {why}")),
            },
            Event::Prompted((id, Some(n))) if Some(id) == self.count_prompt => {
                if api::set_setting("log.count", Some(n.trim())).is_ok() {
                    self.read();
                }
            }
            Event::Timer(t) if Some(t) == self.timer && self.page.is_some() => self.read(),
            _ => {}
        }
    }

    fn page(&mut self, _page: u64, _width: u32, _height: u32) -> Page {
        let mut p = Page::new().line([page::title("Log"), page::muted(format!("  {} commits", self.commits.len()))]).rule();
        if let Some(why) = &self.failed {
            p = p.line([page::bad(why.clone())]);
        }
        for (hash, subject) in &self.commits {
            p = p.line([page::accent(hash.clone()), page::text(format!(" {subject}"))]);
        }
        if !self.commits.is_empty() {
            p = p.focus(2 + self.focus as u32);
        }
        p.key("j/k", "move").key("y", "copy").key("Enter", "show").key("n", "how many")
    }

    fn page_key(&mut self, page: u64, key: Key) {
        match key {
            Key::Char('j') => self.focus = (self.focus + 1).min(self.commits.len().saturating_sub(1)),
            Key::Char('k') => self.focus = self.focus.saturating_sub(1),
            Key::Char('y') => {
                if let Some((hash, _)) = self.commits.get(self.focus) {
                    api::copy(hash);
                }
            }
            Key::Enter => {
                let items: Vec<PickerItem> = self.commits.iter().map(|(h, s)| PickerItem::new(s.clone(), h.clone())).collect();
                self.pick = Some(api::pick("Show which commit", &items));
            }
            Key::Char('n') => self.count_prompt = Some(api::prompt("How many commits", &Log::count().to_string())),
            _ => return,
        }
        api::page::refresh(page);
    }

    fn page_closed(&mut self, page: u64) {
        if self.page == Some(page) {
            self.page = None;
        }
    }
}

const LOG: &str = "a1b2c3d Read the log on a timer\n9f8e7d6 Copy a commit's hash\n0123abc The first commit\n";

#[test]
fn a_command_runs_git_and_its_output_is_the_page() {
    let fake = Fake::new();
    fake.on_spawn("git log --oneline", Ran::ok(LOG));
    let mut h: Harness<Log> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    h.command("log.open").unwrap();

    // What it asked for, in order: the answer came after the command.
    assert_eq!(fake.take(), ["timer 1 60000 repeat", "page 2 log Log", "spawn 3 git log --oneline -n 3", "status log: reading", "status -", "refresh 2"]);
    assert_eq!(fake.process("git log").map(|p| p.cmd.cwd), Some(Some("/project".into())));
    assert_eq!(fake.status(), None, "reading's done");
    assert_eq!(fake.settings_read(), ["log.count"]);

    let page = h.page(KIND);
    assert_snapshot!("log", h.screen(page));
    assert_snapshot!("log-marked", h.marked(page));

    // j, y: the second commit's hash copied.
    h.keys(page, "jy");
    assert_eq!(fake.copied().as_deref(), Some("9f8e7d6"));
    assert!(render::marked(&h.render(page, 80, 24)).contains("> «accent:9f8e7d6» Copy"));
}

#[test]
fn a_picker_and_a_service_are_answered() {
    let fake = Fake::new();
    fake.on_spawn("git", Ran::ok(LOG)).on_pick("Show which", "The first commit").on_service("review.show", Ok("{}"));
    let mut h: Harness<Log> = Harness::with_fake(fake.clone());
    h.command("log.open").unwrap();
    h.key(h.page(KIND), Key::Enter);
    let picker = fake.pickers().pop().unwrap();
    assert_eq!(picker.items.iter().map(|i| i.value.as_str()).collect::<Vec<_>>(), ["a1b2c3d", "9f8e7d6", "0123abc"]);
    assert_eq!(fake.services().pop().map(|c| c.payload), Some("{\"commit\":\"0123abc\"}".into()));
    assert_eq!(fake.notifications(), ["shown in Review"]);

    // Review isn't there.
    fake.on_service("review.show", Err("no extension offers it"));
    h.key(h.page(KIND), Key::Enter);
    assert_eq!(fake.notifications().last().unwrap(), "Review can't show it: no extension offers it");
}

#[test]
fn a_prompt_sets_the_setting_and_a_timer_reads_again() {
    let fake = Fake::new();
    fake.on_spawn("git", Ran::ok(LOG)).on_prompt("How many", "2");
    let mut h: Harness<Log> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    h.command("log.open").unwrap();
    fake.take();
    h.key(h.page(KIND), Key::Char('n'));
    let calls = fake.take();
    assert_eq!(calls[0], "prompt 4 How many commits [3]");
    assert!(calls.contains(&"setting-set log.count 2".to_string()), "{calls:?}");
    assert!(calls.contains(&"spawn 5 git log --oneline -n 2".to_string()), "{calls:?}");
    assert_eq!(fake.setting_value("log.count").as_deref(), Some("2"));

    // The timer, with the page open: read again. Closed: not.
    h.fire_timers();
    assert!(fake.take().iter().any(|c| c.starts_with("spawn 6 git log")));
    assert_eq!(fake.timers().len(), 1, "it repeats");
    h.close(h.page(KIND));
    h.fire_timers();
    assert!(!fake.take().iter().any(|c| c.starts_with("spawn")));
    assert!(fake.page(KIND).is_none());
}

#[test]
fn a_process_that_fails_or_cant_start_is_said() {
    let fake = Fake::new();
    fake.on_spawn("git log", Ran::fails(128, "fatal: not a git repository"));
    let mut h: Harness<Log> = Harness::with_fake(fake.clone());
    h.command("log.open").unwrap();
    assert_eq!(fake.state().notifications, [(api::Level::Error, "git log failed".to_string())]);
    assert!(h.screen(h.page(KIND)).contains("git log exited Some(128)"));

    fake.on_spawn_error("git", "git isn't installed");
    h.command("log.open").unwrap();
    assert_eq!(fake.logs(), ["warn git: git isn't installed"]);
}

#[test]
fn held_events_wait_for_settle() {
    let fake = Fake::new();
    fake.on_spawn("git", Ran::running("a1b2c3d One\n"));
    let mut h: Harness<Log> = Harness::with_fake(fake.clone());
    h.settles = false;
    h.command("log.open").unwrap();
    assert_eq!(fake.pending().len(), 1);
    h.settle();
    let git = fake.process("git").unwrap().id;
    h.settles = true;
    h.said(git, "9f8e7d6 Two", false);
    h.exited(git, Some(0));
    assert!(h.screen(h.page(KIND)).contains("2 commits"));
}

#[test]
fn code_that_calls_the_host_runs_without_a_harness() {
    // A fresh thread's fake, made on first use.
    api::notify("hello");
    api::info!("logged {}", 1);
    assert_eq!(Fake::current().take(), ["notify hello", "info logged 1"]);
    assert_eq!(api::theme().name, "Test");
    assert_eq!(api::platform().os, "linux");
}
