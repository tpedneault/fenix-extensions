//! What API 1.1 to 1.5 added, through `api` and the harness: hints that
//! name the keys of the scheme in use (`key_for`, `key_scheme`, and
//! `key_scheme_changed` drawing them again), notifications and their
//! dismissal, a service's progress and its cancelling, a compose pane the
//! agent can write, the repository's state and the last task.

use fenix_test_kit::{api, Fake, Harness, Ran};

use api::{page, Event, Extension, GitSummary, KeyScheme, Notification, Page, TaskResult, Urgency};

const KIND: &str = "build";

/// A build page: its hints name the keys of the scheme in use, a build's
/// end is a notification, and it serves `build.run` with progress.
#[derive(Default)]
struct Build {
    page: Option<u64>,
    task: Option<u64>,
    redrawn: u32,
    serving: Option<u64>,
    cancelled: Vec<u64>,
    draft: String,
}

/// The keys of `command` now, else `vim` -- Vim keys' -- with the leader
/// the scheme writes.
fn keys(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| match (api::key_scheme(), vim.strip_prefix("SPC ")) {
        (KeyScheme::Standard, Some(rest)) => format!("C-k {rest}"),
        _ => vim.to_string(),
    })
}

impl Extension for Build {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "build.open" => self.page = Some(page::open(KIND, "Build")),
            "build.run" => {
                let cmd = api::Command::new("make");
                self.task = Some(api::host::run_task("make", &cmd)?);
            }
            "build.describe" => {
                let ask = api::agents::Ask::new("Describe the build's failure.");
                api::compose_with_ask("Describe it", "post", "", false, ask, false);
            }
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        if let Event::TaskFinished((id, ok)) = event {
            if Some(id) != self.task {
                return;
            }
            let last = api::host::last_task();
            let body = match last.and_then(|t| t.tests) {
                Some((passed, failed)) => format!("{passed} passed, {failed} failed"),
                None => String::new(),
            };
            let n = if ok {
                Notification::new("make succeeded").urgency(Urgency::Low)
            } else {
                Notification::new("make failed").action("Run it again", "build.run")
            };
            api::host::notify_with(&n.body(body).tag("build"));
        }
    }

    fn serve(&mut self, call: u64, _name: &str, _payload: &str) -> Option<Result<String, String>> {
        self.serving = Some(call);
        api::progress(call, "building", Some(0.5));
        None
    }

    fn service_cancelled(&mut self, call: u64) {
        self.cancelled.push(call);
        api::notify_dismiss("build");
    }

    fn agent_ask_progress(&mut self, _call: u64, text: &str) {
        self.draft = text.to_string();
    }

    fn key_scheme_changed(&mut self, _scheme: KeyScheme) {
        self.redrawn += 1;
        if let Some(p) = self.page {
            page::refresh(p);
        }
    }

    fn page(&mut self, _page: u64, _width: u32, _height: u32) -> Page {
        let branch = api::host::git_state().and_then(|g| g.branch).unwrap_or_else(|| "no repository".into());
        Page::new()
            .line([page::title("Build"), page::muted(format!("  {branch}"))])
            .line([page::text(format!("{} runs it again", keys("build.run", "SPC b r")))])
            .line([page::text(format!("{} describes a failure", keys("build.describe", "SPC m d")))])
            .line([page::text(format!("{} opens the problems", keys("project.quickfix_next", "]q")))])
    }
}

#[test]
fn a_hint_names_the_keys_of_the_scheme_in_use_and_is_drawn_again_when_it_changes() {
    let fake = Fake::new();
    // Bound under the leader; one only in `SPC m`, which Fenix's
    // `key_for` doesn't know; one bound differently in each scheme.
    fake.key("build.run", "SPC b r").menu_key("build.describe", "d").key("project.quickfix_next", "]q").key_in(KeyScheme::Standard, "project.quickfix_next", "F8");
    let mut h: Harness<Build> = Harness::with_fake(fake.clone());
    h.command("build.open").unwrap();
    let page = h.page(KIND);
    assert_eq!(h.screen(page), "Build  no repository\nSPC b r runs it again\nSPC m d describes a failure\n]q opens the problems\n");

    fake.take();
    h.key_scheme(KeyScheme::Standard);
    assert_eq!(h.ext.redrawn, 1);
    assert_eq!(fake.take(), ["refresh 1"], "told, it draws its hints again");
    assert_eq!(h.screen(page), "Build  no repository\nC-k b r runs it again\nC-k m d describes a failure\nF8 opens the problems\n");
    assert_eq!(api::key_for("build.describe"), None, "an `SPC m` key is no key to key_for");
    assert!(fake.keys_asked().contains(&"build.run".to_string()));

    // Rebound, a leader of the user's own, and no key at all.
    fake.leader_chord("C-;");
    assert_eq!(api::key_for("build.run").as_deref(), Some("C-; b r"));
    fake.no_key("build.run");
    assert_eq!(api::key_for("build.run"), None);
    assert_eq!(api::key_scheme(), KeyScheme::Standard);
}

#[test]
fn a_notification_is_kept_with_what_it_said_and_a_tag_takes_its_place() {
    let fake = Fake::new();
    fake.on_task("make", false).on_task("make", false);
    let mut h: Harness<Build> = Harness::with_fake(fake.clone());
    h.command("build.run").unwrap();
    let failed = fake.toasts().pop().unwrap();
    assert_eq!((failed.summary.as_str(), failed.urgency, failed.tag.as_deref()), ("make failed", Urgency::Normal, Some("build")));
    assert_eq!((failed.actions[0].label.as_str(), failed.actions[0].command.as_str()), ("Run it again", "build.run"));
    assert_eq!(fake.take(), ["task 1 make: make", "notification normal #build: make failed [Run it again -> build.run]"]);

    // Its tests counted: the last task says them, and the newer toast
    // takes the older's place.
    fake.on_task_tests("make", 12, 0);
    h.command("build.run").unwrap();
    assert_eq!(api::host::last_task(), Some(TaskResult { name: "make".into(), ok: true, tests: Some((12, 0)) }));
    assert_eq!(fake.toasts().len(), 2);
    let showing = fake.showing();
    assert_eq!(showing.len(), 1);
    assert_eq!((showing[0].summary.as_str(), showing[0].body.as_str(), showing[0].urgency), ("make succeeded", "12 passed, 0 failed", Urgency::Low));
    assert!(fake.notifications().is_empty(), "the message line said nothing");

    // Dismissed: gone from what's showing, kept in what it said.
    h.cancelled(99);
    assert_eq!(fake.dismissed(), ["build"]);
    assert!(fake.showing().is_empty());
    assert_eq!(fake.toasts().len(), 2);
    assert_eq!(h.ext.cancelled, [99]);
}

#[test]
fn a_service_says_its_progress_and_hears_it_was_given_up() {
    let fake = Fake::new();
    let mut h: Harness<Build> = Harness::with_fake(fake.clone());
    assert_eq!(h.serve("build.run", "{}"), None, "answered later");
    let call = h.ext.serving.unwrap();
    let said = fake.progress_of(call);
    assert_eq!((said[0].message.as_str(), said[0].fraction), ("building", Some(0.5)));
    assert_eq!(fake.take(), [format!("progress {call} building (50%)")]);
    h.cancelled(call);
    assert_eq!(h.ext.cancelled, [call]);
}

#[test]
fn a_compose_pane_the_agent_can_write_is_answered_as_any_compose_pane() {
    let fake = Fake::new();
    fake.on_prompt("Describe it", "It fails on the linker.");
    let mut h: Harness<Build> = Harness::with_fake(fake.clone());
    h.settles = false;
    h.command("build.describe").unwrap();
    let pane = fake.prompts().pop().unwrap();
    let (payload, over) = pane.ask.clone().unwrap();
    assert!(pane.compose && !over);
    assert!(payload.contains("\"prompt\":\"Describe the build's failure.\"") && payload.contains("\"stream\":true"), "{payload}");
    assert_eq!(fake.pending(), [Event::Composed((pane.id, Some("It fails on the linker.".into())))]);
    h.settle();
    h.ask_progress(7, "It fails on");
    assert_eq!(h.ext.draft, "It fails on");
}

#[test]
fn the_repositorys_state_is_what_the_test_says() {
    let fake = Fake::new();
    fake.git(Some(GitSummary { branch: Some("main".into()), head: Some("a1b2c3d".into()), upstream: Some("origin/main".into()), ahead: 1, behind: 0, conflicts: 0 }));
    fake.on_spawn("make", Ran::ok(""));
    let mut h: Harness<Build> = Harness::with_fake(fake.clone());
    h.command("build.open").unwrap();
    assert!(h.screen(h.page(KIND)).starts_with("Build  main\n"));
    fake.git(None);
    assert_eq!(api::host::git_state(), None);
}

/// A test provider for API 1.6, as a pack writes one: it says its runner
/// as it starts, and answers the five exports.
#[derive(Default)]
struct Suite;

impl Extension for Suite {
    fn activate(&mut self) -> Result<(), String> {
        api::tests::provider("suite", "suite run", Some("suite isn't installed"))?;
        api::tests::provider("suite", "suite run", None)
    }

    fn tests_scan(&mut self, provider: &str, path: &str, text: &str) -> Vec<api::tests::Found> {
        let file = api::tests::Found { id: path.into(), label: path.into(), kind: api::tests::Kind::File, path: Some(path.into()), ..Default::default() };
        let tests = text.lines().enumerate().filter_map(|(n, l)| {
            l.strip_prefix("# test ").map(|name| api::tests::Found { id: format!("{provider}:{path}::{name}"), parent: Some(path.into()), label: name.into(), line: Some(n as u32), ..Default::default() })
        });
        std::iter::once(file).chain(tests).collect()
    }

    fn tests_plan(&mut self, _provider: &str, run: u64, ask: api::tests::Ask) -> Result<api::tests::Plan, String> {
        match ask {
            api::tests::Ask::Run(found) if found.is_empty() => Err("nothing asked".into()),
            _ => Ok(api::tests::Plan { steps: vec![api::tests::Step { command: api::Command::new("suite").arg(run.to_string()), label: "suite".into(), covers: Vec::new(), needs: Vec::new() }], list_after: false }),
        }
    }

    fn tests_read(&mut self, _provider: &str, _run: u64, step: u32, lines: Vec<api::tests::Line>) -> Vec<api::tests::Report> {
        lines.into_iter().map(|l| api::tests::Report::Output { id: None, text: format!("{step}: {}", l.text) }).collect()
    }

    fn tests_step_ended(&mut self, _provider: &str, _run: u64, _step: u32, end: api::tests::StepEnd) -> Vec<api::tests::Report> {
        vec![api::tests::Report::Output { id: None, text: format!("{end:?}") }]
    }

    fn tests_launch(&mut self, _provider: &str, _run: u64, test: api::tests::Found) -> Result<api::tests::Launch, String> {
        Ok(api::tests::Launch::Launch(api::LaunchConfig::launch(&test.label, &["suite"])))
    }
}

#[test]
fn a_test_provider_says_its_runner_and_is_driven_through_its_five_exports() {
    let fake = Fake::new();
    let mut h: Harness<Suite> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    assert_eq!(fake.take(), ["test-provider suite: suite run (suite isn't installed)", "test-provider suite: suite run"]);
    assert_eq!(fake.test_providers(), [("suite".to_string(), "suite run".to_string(), Some("suite isn't installed".to_string())), ("suite".to_string(), "suite run".to_string(), None)]);

    let found = h.tests_scan("suite", "/project/a.txt", "# test adds\n\n# test subtracts\n");
    assert_eq!(found.len(), 3);
    assert_eq!((found[2].label.as_str(), found[2].line), ("subtracts", Some(2)));

    let plan = h.tests_plan("suite", 7, api::tests::Ask::RunAll).unwrap();
    assert_eq!(plan.steps[0].command.args, ["7"]);
    assert_eq!(h.tests_plan("suite", 8, api::tests::Ask::Run(Vec::new())), Err("nothing asked".into()));

    let line = api::tests::Line { text: "PASS adds".into(), ..Default::default() };
    assert_eq!(h.tests_read("suite", 7, 1, vec![line]), [api::tests::Report::Output { id: None, text: "1: PASS adds".into() }]);
    assert_eq!(h.tests_step_ended("suite", 7, 1, api::tests::StepEnd::Exited(Some(0))), [api::tests::Report::Output { id: None, text: "Exited(Some(0))".into() }]);
    let launch = h.tests_launch("suite", 7, found[1].clone()).unwrap();
    assert!(matches!(launch, api::tests::Launch::Launch(c) if c.name == "adds"));
}
