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
//! (kiln found, the project read, when to read it again) and `pages` (the
//! pages, from the project's state). This file meets Fenix: it hands
//! Fenix's events to the project, and passes on what the project says
//! changed. It stays the thin part that talks to the host.

pub mod kiln;
pub mod model;
pub mod pages;
pub mod project;

use std::collections::HashMap;

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::{page as pages_api, Event, Extension, Key, Page};
use project::{Notice, Project, Why};

/// The page kind the plan opens as (the manifest's context names it).
const PLAN: &str = "plan";

#[derive(Default)]
struct Kiln {
    project: Project,
    /// Its pages, and whether each is in view.
    pages: HashMap<u64, bool>,
}

impl Kiln {
    /// What the project said, passed on: a change draws its pages again.
    /// Answers to queries have no one to go to yet (the pages that ask come
    /// in E1); they're dropped.
    fn pass_on(&mut self) {
        let notices = self.project.take_notices();
        if notices.contains(&Notice::Changed) {
            for &p in self.pages.keys() {
                pages_api::refresh(p);
            }
        }
    }

    fn shown_changed(&mut self) {
        let any = self.pages.values().any(|&shown| shown);
        self.project.set_shown(any);
    }
}

impl Extension for Kiln {
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
            "kiln.reload" => self.project.reload(Why::Asked),
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
                "kiln.path" => self.project.kiln_changed(),
                "kiln.refresh_seconds" => self.project.refresh_changed(),
                _ => {}
            },
            _ => {}
        }
        self.pass_on();
    }

    fn file_saved(&mut self, path: &str) {
        self.project.saved(path);
        self.pass_on();
    }

    fn page(&mut self, _page: u64, _width: u32, _height: u32) -> Page {
        pages::plan::page(self.project.state())
    }

    fn page_key(&mut self, _page: u64, key: Key) {
        if key == Key::Char('r') {
            self.project.reload(Why::Asked);
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
            4
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
        assert_eq!(spawned(&fake).len(), 4);
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

    #[test]
    fn an_unknown_command_is_an_error() {
        let mut h: Harness<Kiln> = Harness::new();
        assert_eq!(
            h.command("kiln.nope"),
            Err("no command kiln.nope".to_string())
        );
    }
}
