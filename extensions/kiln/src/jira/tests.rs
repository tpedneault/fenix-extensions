//! The Jira mirror's tests: what it reads from kiln's real output (over a
//! cache the recording writes, tests/fixtures/hand/README), the rules for
//! when it is asked, and the plan page with tickets on its rows, a slow or
//! absent Jira never holding up a reload.

use super::*;
use crate::kiln::Ran;
use crate::model::tests::fixture;
use fenix_test_kit::Fake;

/// A recorded answer under `out/jira/` (`.exit` and `.stderr` beside it).
fn recorded(name: &str, ext: &str) -> Result<Ran, Failure> {
    crate::pages::exports::tests::answer(&format!("jira/{name}"), ext)
}

/// A hand-written one under `hand/`.
fn hand(name: &str, ext: &str) -> Result<Ran, Failure> {
    crate::pages::exports::tests::answer(&format!("../hand/{name}"), ext)
}

fn ok(stdout: &str) -> Result<Ran, Failure> {
    Ok(Ran {
        code: Some(0),
        stdout: stdout.to_string(),
        stderr: Vec::new(),
    })
}

fn items() -> Vec<Item> {
    model::list(&fixture("jira/list.json")).unwrap()
}

fn item<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items.iter().find(|i| i.id == id).unwrap()
}

/// Cells as `text|look`, for a table.
fn shown(cells: Vec<Cell>) -> Vec<String> {
    cells
        .into_iter()
        .map(|c| format!("{}|{:?}", c.text, c.look))
        .collect()
}

// -- What it reads ----------------------------------------------------------------

#[test]
fn kilns_real_answer_gives_each_ticket_in_the_goods_look_when_resolved() {
    let t = Tickets::parse(&fixture("jira/list.json")).unwrap();
    assert_eq!(t.len(), 6, "five tickets and the key Jira doesn't know");
    let items = items();
    let cells = |id: &str| shown(t.cells(item(&items, id)));
    // Resolved is good, anything else muted; the assignee after a dot,
    // none when nobody has it; `?` for a key Jira doesn't know.
    assert_eq!(cells("TASK-001"), ["DEMO-101 Done · Ana Lopez|Good"]);
    assert_eq!(cells("TASK-002"), ["DEMO-102 In Review · Ana Lopez|Muted"]);
    assert_eq!(cells("TASK-003"), ["DEMO-103 To Do|Muted"]);
    assert_eq!(cells("TASK-004"), ["DEMO-777 ?|Muted"]);
    // One cell for each of an item's tickets.
    assert_eq!(
        cells("TASK-006"),
        [
            "DEMO-104 Done · Bo Chen|Good",
            "DEMO-105 In Progress · Bo Chen|Muted"
        ]
    );
    assert!(cells("TASK-005").is_empty(), "no ticket linked");
    assert!(cells("REQ-001").is_empty());
}

#[test]
fn the_text_kiln_prints_says_the_same_thing() {
    // `list --jira` in text, recorded beside the JSON: its tickets are the
    // ones the cells show.
    let text = fixture("jira/list.txt");
    let t = Tickets::parse(&fixture("jira/list.json")).unwrap();
    let items = items();
    for id in ["TASK-001", "TASK-002", "TASK-003"] {
        let cell = &t.cells(item(&items, id))[0].text;
        assert!(text.contains(cell.as_str()), "{cell} in {text}");
    }
    assert!(text.contains("DEMO-777 (not in Jira)"));
}

#[test]
fn no_linked_key_is_an_empty_mirror_and_needs_no_jira() {
    let t = Tickets::parse(&fixture("jira/list-no-keys.json")).unwrap();
    assert!(t.is_empty());
    assert!(t.cells(item(&items(), "TASK-001")).is_empty());
}

#[test]
fn a_key_the_mirror_hasnt_read_shows_nothing_rather_than_a_wrong_ticket() {
    let t = Tickets::parse(&fixture("jira/list.json")).unwrap();
    assert_eq!(t.cell("DEMO-999"), None);
    assert_eq!(
        t.cell(" DEMO-103 ").map(|c| c.text),
        Some("DEMO-103 To Do".into())
    );
}

#[test]
fn jiras_words_are_plain_text_on_one_line() {
    let json = serde_json::json!([{
        "id": "TASK-001",
        "jira": [{
            "key": "DEMO-1",
            "status": "In\nReview\u{1b}[31m\u{7}",
            "resolved": false,
            "assignee": "Al\u{202e}ice\tSmith"
        }]
    }])
    .to_string();
    let t = Tickets::parse(&json).unwrap();
    let cell = t.cell("DEMO-1").unwrap();
    assert_eq!(cell.text, "DEMO-1 In Review [31m · Al ice Smith");
    assert!(!cell.text.chars().any(|c| c.is_control()));
    assert!(!cell.text.contains('\u{202e}'));
}

#[test]
fn what_isnt_an_array_of_items_is_said_not_guessed_at() {
    assert!(Tickets::parse("{}")
        .unwrap_err()
        .contains("expected a JSON array"));
    assert!(Tickets::parse("not json").is_err());
    // Extras and gaps are fine: nothing here is required.
    let t =
        Tickets::parse(r#"[{"id":"T-1","extra":1,"jira":[{"key":"K-1"}]},{"id":"T-2"}]"#).unwrap();
    assert_eq!(t.cell("K-1").map(|c| c.text), Some("K-1 ".into()));
}

// -- Why it fails -----------------------------------------------------------------

#[test]
fn a_missing_token_is_told_by_its_words_not_its_exit_code() {
    for (line, token) in [
        (
            recorded("list-no-token", "json").unwrap().error_line(),
            true,
        ),
        (
            hand("jira-list-no-token", "json").unwrap().error_line(),
            true,
        ),
        (
            hand("jira-sync-no-token", "json").unwrap().error_line(),
            true,
        ),
        (
            recorded("list-unreachable", "json").unwrap().error_line(),
            false,
        ),
        (
            recorded("list-unconfigured", "json").unwrap().error_line(),
            false,
        ),
    ] {
        assert_eq!(wants_token(&line), token, "{line}");
    }
    // The mirror exits 3 for it where sync and pull exit 2.
    assert_eq!(recorded("list-no-token", "json").unwrap().code, Some(3));
    assert_eq!(recorded("sync-no-token", "out").unwrap().code, Some(2));
}

// -- When it is read ----------------------------------------------------------------

fn host() -> Fake {
    let fake = Fake::new();
    fake.install();
    fake
}

fn timers(fake: &Fake) -> Vec<u32> {
    fake.timers().iter().map(|t| t.millis).collect()
}

#[test]
fn asked_once_then_not_until_its_timer_or_a_refresh() {
    let fake = host();
    let mut m = Mirror::default();
    // Wanted at the start, once the caller says it's the moment.
    assert!(
        !m.wants(false),
        "not while the queue is busy or no page is in view"
    );
    assert!(m.wants(true));
    m.asked(7);
    assert!(!m.wants(true), "one out");
    // Its answer: new tickets, a ten-minute timer, and nothing more asked.
    assert_eq!(
        m.answered(7, &ok(&fixture("jira/list.json"))),
        Heard::Tickets
    );
    assert_eq!(timers(&fake), [FRESH_MS]);
    assert_eq!(FRESH_MS, 600_000, "kiln's own cache's ten minutes");
    assert!(!m.wants(true), "not again at the next reload");
    // The timer: due again, and a timer that isn't its own isn't.
    let timer = fake.timers()[0].id;
    assert!(!m.timer(timer + 100));
    assert!(m.timer(timer));
    assert!(m.wants(true));
    m.asked(8);
    assert_eq!(
        m.answered(8, &ok(&fixture("jira/list.json"))),
        Heard::Tickets
    );
    // `r`: at once, whatever the timer says, and the timer is let go.
    assert!(!m.wants(true));
    let before = timers(&fake).len();
    m.refresh();
    assert!(m.wants(true));
    assert_eq!(
        timers(&fake).len(),
        before - 1,
        "the old timer is cancelled"
    );
}

#[test]
fn an_answer_that_isnt_the_one_out_is_nobodys() {
    let _fake = host();
    let mut m = Mirror::default();
    m.asked(3);
    assert_eq!(m.answered(2, &ok("[]")), Heard::Nothing);
    assert!(m.tickets().is_none());
    assert_eq!(m.answered(3, &ok("[]")), Heard::Tickets);
    assert!(m.tickets().is_some_and(Tickets::is_empty));
}

#[test]
fn stopped_for_another_job_it_is_asked_again_without_a_word() {
    let fake = host();
    let mut m = Mirror::default();
    m.asked(1);
    assert_eq!(m.answered(1, &Err(Failure::Yielded)), Heard::Nothing);
    assert!(m.wants(true), "when the queue is quiet");
    assert!(
        timers(&fake).is_empty(),
        "no timer: not a read that finished"
    );
    m.asked(2);
    let gone = Err(Failure::Unusable("another kiln was chosen".into()));
    assert_eq!(m.answered(2, &gone), Heard::Nothing);
    assert!(m.wants(true));
}

#[test]
fn a_failure_is_said_once_the_rows_keep_the_last_tickets_and_it_is_tried_in_ten_minutes() {
    let fake = host();
    let mut m = Mirror::default();
    // Never read: no tickets, and why, once.
    m.asked(1);
    let line = recorded("list-unreachable", "json").unwrap().error_line();
    let heard = m.answered(1, &recorded("list-unreachable", "json"));
    assert_eq!(
        heard,
        Heard::Failed {
            line: line.clone(),
            token: false
        }
    );
    assert!(m.tickets().is_none());
    assert!(!m.wants(true), "not on the next reload: the timer's");
    assert_eq!(timers(&fake), [FRESH_MS]);
    // Tried again, and the same line is not said again.
    assert!(m.timer(fake.timers()[0].id));
    m.asked(2);
    assert_eq!(
        m.answered(2, &recorded("list-unreachable", "json")),
        Heard::Nothing
    );
    // A different one is.
    m.refresh();
    m.asked(3);
    let other = Err(Failure::TimedOut(60));
    assert!(matches!(m.answered(3, &other), Heard::Failed { .. }));
    // A read that works clears it; a failure after is said again, and the
    // tickets read stay.
    m.refresh();
    m.asked(4);
    assert_eq!(
        m.answered(4, &ok(&fixture("jira/list.json"))),
        Heard::Tickets
    );
    assert!(!m.needs_token());
    m.refresh();
    m.asked(5);
    assert!(matches!(
        m.answered(5, &recorded("list-unreachable", "json")),
        Heard::Failed { .. }
    ));
    assert_eq!(
        m.tickets().map(Tickets::len),
        Some(6),
        "old news beats none"
    );
}

#[test]
fn no_token_is_remembered_so_a_can_sign_in() {
    let _fake = host();
    let mut m = Mirror::default();
    m.asked(1);
    let heard = m.answered(1, &recorded("list-no-token", "json"));
    assert!(
        matches!(heard, Heard::Failed { token: true, .. }),
        "{heard:?}"
    );
    assert!(m.needs_token());
    // The same words from a desktop with a credential store.
    let mut m = Mirror::default();
    m.asked(1);
    let heard = m.answered(1, &hand("jira-list-no-token", "json"));
    assert!(
        matches!(heard, Heard::Failed { token: true, .. }),
        "{heard:?}"
    );
}

#[test]
fn an_unreadable_answer_is_a_failure_with_kilns_shape_named() {
    let _fake = host();
    let mut m = Mirror::default();
    m.asked(1);
    let heard = m.answered(1, &ok("{\"kiln_json\": 2}"));
    assert!(
        matches!(&heard, Heard::Failed { line, .. } if line.contains("kiln list --jira")),
        "{heard:?}"
    );
}

#[test]
fn clear_forgets_the_tickets_and_the_read_that_was_out() {
    let fake = host();
    let mut m = Mirror::default();
    m.asked(1);
    m.answered(1, &ok(&fixture("jira/list.json")));
    m.asked(2);
    assert!(m.clear(), "there were tickets to take away");
    assert!(m.tickets().is_none() && timers(&fake).is_empty());
    // What was out answers for the other Jira: dropped.
    assert_eq!(
        m.answered(2, &ok(&fixture("jira/list.json"))),
        Heard::Nothing
    );
    assert!(m.wants(true), "the next Jira is read");
    assert!(!m.clear(), "nothing to take away now");
}

// -- On the plan ------------------------------------------------------------------------

mod page {
    use super::*;
    use crate::pages::Tab;
    use crate::tests::{folder, foot, harness, spawned};
    use crate::Kiln;
    use fenix_test_kit::{assert_snapshot, Harness, Ran as Run};

    /// kiln answering as the demo project does, its items linking tickets
    /// as ../out/jira/list.json's do, and `list --jira` with `mirror`.
    fn fake(mirror: Run) -> Fake {
        let fake = Fake::new();
        fake.program("kiln", "/usr/bin/kiln")
            .on_spawn("kiln --version", Run::ok(fixture("version.txt")))
            .on_spawn(
                "kiln list --format json",
                Run::ok(fixture("jira/list.json")),
            )
            .on_spawn(
                "kiln status --format json",
                Run::ok(fixture("demo/status.json")),
            )
            .on_spawn(
                "kiln check --format json",
                Run::exits(1)
                    .stdout(fixture("demo/check.json"))
                    .stderr(fixture("demo/check.stderr")),
            )
            .on_spawn(
                "kiln lsp --languages",
                Run::fails(2, fixture("demo/lsp-languages.stderr")),
            )
            .on_spawn(
                "kiln trace --format json",
                Run::ok(fixture("demo/trace.json")),
            )
            .on_spawn("kiln list --jira --format json", mirror);
        fake
    }

    /// The mirror as the recording read it, and the plan open.
    fn run(r: Result<Ran, Failure>) -> Run {
        let r = r.unwrap();
        let mut run = Run::exits(r.code.unwrap_or(0)).stdout(r.stdout);
        if !r.stderr.is_empty() {
            run = run.stderr(r.stderr.join("\n"));
        }
        run
    }

    fn open(test: &str, toml: Option<&str>, mirror: Run) -> (Fake, Harness<Kiln>, u64) {
        let fake = fake(mirror);
        let dir = folder(&format!("jira-{test}"));
        if let Some(toml) = toml {
            std::fs::write(dir.join("kiln.toml"), toml).unwrap();
        }
        let mut h = harness(&fake, &dir);
        h.size = (110, 26);
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        (fake, h, page)
    }

    fn jira_toml() -> String {
        fixture("jira/kiln.toml")
    }

    fn mirror_runs(fake: &Fake) -> usize {
        spawned(fake)
            .iter()
            .filter(|l| l.ends_with("kiln list --jira --format json"))
            .count()
    }

    #[test]
    fn once_the_project_is_read_the_mirror_is_asked_behind_it_and_rows_end_with_tickets() {
        let (fake, mut h, page) = open("rows", Some(&jira_toml()), run(recorded("list", "json")));
        // In this order: the project's own reads, then the mirror, never
        // in between them.
        let lines = spawned(&fake);
        let order: Vec<&str> = lines
            .iter()
            .filter(|l| l.starts_with("/usr/bin/kiln"))
            .map(|l| l.trim_start_matches("/usr/bin/kiln "))
            .collect();
        assert_eq!(
            order,
            [
                "--version",
                "list --format json",
                "status --format json",
                "check --format json",
                // The language server's question goes first: asked beside
                // it, the mirror would be stopped at once.
                "lsp --languages --format json",
                "list --jira --format json"
            ],
            "{order:?}"
        );
        assert_snapshot!("tree-demo-jira", h.screen(page));
        // Every tab's item rows, once, in one place: the Board's too.
        h.keys(page, "4");
        assert_snapshot!("board-demo-jira", h.screen(page));
        // The filter reaches a ticket.
        h.keys(page, "1/in review<Enter>");
        let s = h.screen(page);
        assert!(s.contains("TASK-002") && !s.contains("TASK-003"), "{s}");
    }

    #[test]
    fn without_jira_in_kiln_toml_nothing_is_asked_and_rows_are_what_they_were() {
        let plain = fixture("exports/kiln.toml");
        let (fake, mut h, page) = open("none", Some(&plain), run(recorded("list", "json")));
        assert_eq!(mirror_runs(&fake), 0);
        assert!(!h.screen(page).contains("DEMO-10"));
        // And with no kiln.toml the extension can read at all.
        let (fake, _h, _page) = open("unread", None, run(recorded("list", "json")));
        assert_eq!(mirror_runs(&fake), 0);
    }

    #[test]
    fn no_page_in_view_no_mirror() {
        let fake = fake(run(recorded("list", "json")));
        let dir = folder("jira-no-page");
        std::fs::write(dir.join("kiln.toml"), jira_toml()).unwrap();
        let mut h = harness(&fake, &dir);
        h.activate().unwrap();
        assert_eq!(mirror_runs(&fake), 0, "nobody to show the tickets to");
        h.command("kiln.open").unwrap();
        assert_eq!(mirror_runs(&fake), 1);
    }

    #[test]
    fn a_failing_jira_leaves_the_project_read_the_rows_bare_and_says_why_once() {
        let (fake, mut h, page) = open(
            "fails",
            Some(&jira_toml()),
            run(recorded("list-unreachable", "json")),
        );
        let said = recorded("list-unreachable", "json").unwrap().error_line();
        // The project is read and shown; the failure is the bottom line's.
        let first = foot(&mut h, page);
        assert!(
            first.starts_with("Jira: kiln: Jira at http://127.0.0.1:9: request failed"),
            "{first}"
        );
        let s = h.screen(page);
        assert!(s.contains("TASK-002") && !s.contains("DEMO-10"), "{s}");
        assert!(!s.contains("the last read failed"), "not a failed reload");
        assert_snapshot!("tree-demo-jira-failed", h.screen(page));
        // Not on every draw: the next key clears the line, and another
        // draw doesn't bring it back.
        h.keys(page, "j");
        assert_ne!(foot(&mut h, page), first);
        assert_ne!(foot(&mut h, page), first);
        // Nor when it's tried again and fails the same: ten minutes on.
        let timer = fake
            .timers()
            .into_iter()
            .find(|t| t.millis == FRESH_MS)
            .unwrap()
            .id;
        h.fire(timer);
        h.settle();
        assert_eq!(mirror_runs(&fake), 2, "tried again after the timer");
        assert_ne!(foot(&mut h, page), first, "said once");
        let _ = said;
    }

    #[test]
    fn no_token_says_kilns_line_and_a_opens_kiln_auth_jira_in_a_terminal() {
        let (fake, mut h, page) = open(
            "token",
            Some(&jira_toml()),
            run(recorded("list-no-token", "json")),
        );
        let line = foot(&mut h, page);
        assert!(
            line.starts_with("Jira: A signs in (kiln auth jira) -- kiln: couldn't read `jira:http://127.0.0.1:9`"),
            "{line}"
        );
        fake.take();
        h.keys(page, "j");
        h.keys(page, "A");
        assert!(
            fake.take()
                .iter()
                .any(|c| c.ends_with("kiln auth jira (below): /usr/bin/kiln auth jira")),
            "{:?}",
            fake.calls()
        );
        // Never a secret, a prompt for one, or the environment.
        assert!(!fake
            .calls()
            .iter()
            .any(|c| c.contains("secret") || c.starts_with("env") || c.starts_with("prompt")));
    }

    #[test]
    fn a_without_a_token_to_ask_for_says_what_it_is_for() {
        let (fake, mut h, page) = open("a", Some(&jira_toml()), run(recorded("list", "json")));
        fake.take();
        h.keys(page, "A");
        assert!(fake.take().iter().all(|c| !c.contains("kiln auth")));
        assert!(foot(&mut h, page).starts_with("A signs in to Jira"));
    }

    #[test]
    fn the_mirror_is_asked_once_not_on_every_reload_and_on_r_and_after_ten_minutes() {
        let (fake, mut h, page) = open("ten", Some(&jira_toml()), run(recorded("list", "json")));
        // `mirror_runs` counts what was asked since it last looked.
        assert_eq!(mirror_runs(&fake), 1, "once, after the first read");
        // A save, a reload, the refresh timer: none asks.
        h.saved("design/tasks/TASK-001.md");
        h.settle();
        h.keys(page, "2");
        h.keys(page, "1");
        for t in fake.timers().into_iter().filter(|t| t.millis == 5_000) {
            h.fire(t.id);
            h.settle();
        }
        assert_eq!(
            mirror_runs(&fake),
            0,
            "kiln's own cache is ten minutes: no sooner"
        );
        // After the ten minutes, the next quiet moment.
        let timer = fake
            .timers()
            .into_iter()
            .find(|t| t.millis == FRESH_MS)
            .expect("the ten-minute timer")
            .id;
        h.fire(timer);
        h.settle();
        assert_eq!(mirror_runs(&fake), 1);
        // `r` reads it at once, whatever the timer says.
        h.keys(page, "r");
        h.settle();
        assert_eq!(mirror_runs(&fake), 1);
    }

    /// The ids of the processes started for `needle`, and the order of
    /// the calls (`kill N`, `spawn N …`) as the fake logged them.
    fn spawn_ids(calls: &[String], needle: &str) -> Vec<u64> {
        calls
            .iter()
            .filter_map(|c| c.strip_prefix("spawn "))
            .filter(|c| c.ends_with(needle))
            .filter_map(|c| c.split(' ').next()?.parse().ok())
            .collect()
    }

    fn at_call(calls: &[String], call: &str) -> usize {
        calls
            .iter()
            .position(|c| c == call || c.starts_with(&format!("{call} ")))
            .unwrap_or_else(|| panic!("no {call} in {calls:#?}"))
    }

    #[test]
    fn a_slow_jira_never_holds_up_a_reload_and_is_asked_again_after() {
        let (fake, mut h, page) = open("slow", Some(&jira_toml()), Run::running(""));
        // It waits on Jira.
        let calls = fake.take();
        let slow = spawn_ids(&calls, "kiln list --jira --format json");
        assert_eq!(slow.len(), 1);
        assert!(!fake.state().processes[&slow[0]].killed);
        // The reload starts at once, in the queue ahead of anything else:
        // the mirror is stopped for it, not waited for.
        h.keys(page, "r");
        let calls = fake.take();
        assert!(fake.state().processes[&slow[0]].killed);
        let list = spawn_ids(&calls, "kiln list --format json");
        assert_eq!(list.len(), 1, "{calls:#?}");
        assert!(
            at_call(&calls, &format!("kill {}", slow[0]))
                < at_call(&calls, &format!("spawn {}", list[0])),
            "stopped before the reload's first read starts: {calls:#?}"
        );
        // The project is read -- not failed -- and the mirror asked again
        // only after the last of its reads, when the queue is quiet.
        assert!(!h.screen(page).contains("the last read failed"));
        let order: Vec<&str> = calls
            .iter()
            .filter_map(|c| c.strip_prefix("spawn "))
            .filter_map(|c| c.split_once(' ').map(|(_, rest)| rest))
            .collect();
        let at = |needle: &str| {
            order
                .iter()
                .position(|l| l.ends_with(needle))
                .unwrap_or_else(|| panic!("no {needle} in {order:?}"))
        };
        assert!(at("status --format json") < at("check --format json"));
        assert!(
            at("check --format json") < at("list --jira --format json"),
            "{order:?}"
        );
        // A save while it runs stops it the same: the newest mirror read.
        let slow = spawn_ids(&calls, "kiln list --jira --format json");
        assert_eq!(slow.len(), 1);
        h.saved("design/tasks/TASK-001.md");
        assert!(fake.state().processes[&slow[0]].killed);
        let again = fake.take();
        assert_eq!(
            spawn_ids(&again, "kiln list --jira --format json").len(),
            1,
            "asked again behind the reload: {again:#?}"
        );
    }

    #[test]
    fn the_refresh_timer_does_not_keep_stopping_a_read_that_is_slow() {
        let (fake, mut h, _page) = open("timer", Some(&jira_toml()), Run::running(""));
        let slow = spawn_ids(&fake.take(), "kiln list --jira --format json");
        for t in fake.timers().into_iter().filter(|t| t.millis == 5_000) {
            h.fire(t.id);
        }
        assert!(
            !fake.state().processes[&slow[0]].killed,
            "a timer's reload is skipped while the mirror runs"
        );
        assert!(spawned(&fake).is_empty());
    }

    #[test]
    fn a_reload_that_fails_is_the_reloads_and_not_jiras() {
        // The other way round: kiln.toml's Jira is fine, the project is
        // not -- the mirror isn't asked of a project that isn't read.
        let fake = fake(run(recorded("list", "json")));
        fake.on_spawn(
            "kiln list --format json",
            Run::fails(2, "kiln: kiln.toml:4: unknown key"),
        );
        let dir = folder("jira-failed-project");
        std::fs::write(dir.join("kiln.toml"), jira_toml()).unwrap();
        let mut h = harness(&fake, &dir);
        h.command("kiln.open").unwrap();
        assert_eq!(mirror_runs(&fake), 0);
    }

    #[test]
    fn removing_jira_from_kiln_toml_takes_the_tickets_away_and_adding_it_reads_them() {
        let (fake, mut h, page) = open("config", Some(&jira_toml()), run(recorded("list", "json")));
        assert!(h.screen(page).contains("DEMO-102 In Review"));
        let dir = h.ext.root.clone().unwrap();
        std::fs::write(dir.join("kiln.toml"), fixture("exports/kiln.toml")).unwrap();
        h.saved("kiln.toml");
        h.settle();
        assert!(!h.screen(page).contains("DEMO-102"), "no Jira, no tickets");
        let _ = mirror_runs(&fake);
        std::fs::write(dir.join("kiln.toml"), jira_toml()).unwrap();
        h.saved("kiln.toml");
        h.settle();
        assert_eq!(mirror_runs(&fake), 1, "read once more, for the new Jira");
        assert!(h.screen(page).contains("DEMO-102 In Review"));
    }

    // -- J ---------------------------------------------------------------------------

    /// The Tree with the cursor on `id`.
    fn on(h: &mut Harness<Kiln>, page: u64, id: &str) {
        h.keys(page, &format!("/{id}<Enter>"));
        h.keys(page, "/<Esc>");
        assert_eq!(crate::tests::at(h, page).as_deref(), Some(id));
    }

    #[test]
    fn j_pulls_the_status_of_the_item_through_the_edit_routing_and_says_kilns_line() {
        let (fake, mut h, page) = open("pull", Some(&jira_toml()), run(recorded("list", "json")));
        on(&mut h, page, "TASK-002");
        fake.on_spawn(
            "kiln jira pull -- TASK-002",
            Run::ok(fixture("../hand/jira-pull.out")),
        );
        fake.take();
        h.keys(page, "J");
        let calls = fake.take();
        assert!(
            !calls
                .iter()
                .any(|c| c.starts_with("prompt") || c.starts_with("pick")),
            "nothing to confirm: {calls:?}"
        );
        let ran = spawned_in(&calls);
        assert_eq!(
            ran.first().map(String::as_str),
            Some("/usr/bin/kiln jira pull -- TASK-002"),
            "{ran:?}"
        );
        assert_eq!(
            foot(&mut h, page),
            "TASK-002: status doing → done (from DEMO-102 Done)"
        );
        // The project is read again after, as for any edit.
        assert!(
            ran.iter().any(|l| l.ends_with("kiln list --format json")),
            "{ran:?}"
        );
    }

    /// The `spawn` lines of `calls`, as `spawned` gives them.
    fn spawned_in(calls: &[String]) -> Vec<String> {
        calls
            .iter()
            .filter_map(|l| l.strip_prefix("spawn "))
            .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string())
            .collect()
    }

    #[test]
    fn j_shows_kilns_refusals_as_they_are() {
        let (fake, mut h, page) = open(
            "pull-refused",
            Some(&jira_toml()),
            run(recorded("list", "json")),
        );
        on(&mut h, page, "TASK-004");
        fake.on_spawn(
            "kiln jira pull -- TASK-004",
            Run::fails(1, fixture("../hand/jira-pull-no-ticket.stderr")),
        );
        h.keys(page, "J");
        assert_eq!(
            foot(&mut h, page),
            "kiln: TASK-004 has no ticket under the project issue"
        );
        // Nothing to take from Jira: kiln's own line.
        fake.on_spawn(
            "kiln jira pull -- TASK-003",
            Run::ok(fixture("../hand/jira-pull-nothing.out")),
        );
        on(&mut h, page, "TASK-003");
        h.keys(page, "J");
        assert_eq!(
            foot(&mut h, page),
            "every item's status already matches its ticket"
        );
    }

    #[test]
    fn j_with_no_token_says_kilns_line_and_a_signs_in() {
        let (fake, mut h, page) = open(
            "pull-token",
            Some(&jira_toml()),
            run(recorded("list", "json")),
        );
        on(&mut h, page, "TASK-002");
        fake.on_spawn(
            "kiln jira pull -- TASK-002",
            Run::fails(2, fixture("../hand/jira-sync-no-token.stderr")),
        );
        h.keys(page, "J");
        let line = foot(&mut h, page);
        assert!(
            line.starts_with(
                "A signs in (kiln auth jira) -- kiln: no Jira token for http://127.0.0.1:9"
            ),
            "{line}"
        );
        fake.take();
        h.keys(page, "A");
        assert!(fake
            .take()
            .iter()
            .any(|c| c.ends_with("kiln auth jira (below): /usr/bin/kiln auth jira")));
    }

    #[test]
    fn j_on_an_item_with_no_ticket_or_without_jira_or_on_no_item_asks_kiln_nothing() {
        let (fake, mut h, page) = open(
            "pull-none",
            Some(&jira_toml()),
            run(recorded("list", "json")),
        );
        on(&mut h, page, "TASK-005");
        fake.take();
        h.keys(page, "J");
        assert!(spawned(&fake).is_empty());
        assert_eq!(
            foot(&mut h, page),
            "TASK-005 links no Jira ticket (jira = in its file)"
        );
        // A heading is no item: Exports' row, say.
        h.keys(page, "0");
        h.keys(page, "J");
        assert_eq!(foot(&mut h, page), "no item here");
        // And a project with no [jira] at all.
        let plain = fixture("exports/kiln.toml");
        let (fake, mut h, page) = open("pull-nojira", Some(&plain), run(recorded("list", "json")));
        on(&mut h, page, "TASK-002");
        fake.take();
        h.keys(page, "J");
        assert!(spawned(&fake).is_empty());
        assert_eq!(
            foot(&mut h, page),
            "kiln.toml has no [jira]: nothing to take a status from"
        );
    }

    #[test]
    fn j_works_on_every_tab_that_has_an_item_and_no_tab_takes_it() {
        let (fake, mut h, page) = open(
            "pull-tabs",
            Some(&jira_toml()),
            run(recorded("list", "json")),
        );
        fake.on_spawn(
            "kiln jira pull -- TASK-002",
            Run::ok(fixture("../hand/jira-pull.out")),
        );
        // The Board's first row with a ticket, the Item tab handed it.
        for tab in [Tab::Tree, Tab::Board, Tab::Roadmap] {
            h.keys(page, &(tab.index() + 1).to_string());
            h.keys(page, "/TASK-002<Enter>");
            // The Roadmap keeps the milestone above it, and the cursor.
            if crate::tests::at(&h, page).as_deref() != Some("TASK-002") {
                h.keys(page, "j");
            }
            fake.take();
            h.keys(page, "J");
            let ran = spawned(&fake);
            assert!(
                ran.iter()
                    .any(|l| l.ends_with("kiln jira pull -- TASK-002")),
                "{tab:?}: {ran:?} {} {}",
                foot(&mut h, page),
                h.screen(page)
            );
            h.keys(page, "/<Esc>");
        }
        // Every tab's own keys are untouched by it: the source says which
        // letters they take, and J isn't among them.
        for src in [
            include_str!("../pages/trace.rs"),
            include_str!("../pages/changes.rs"),
            include_str!("../pages/item.rs"),
            include_str!("../pages/check.rs"),
            include_str!("../pages/exports.rs"),
            include_str!("../pages/comments.rs"),
        ] {
            assert!(!src.contains("Key::Char('J')") && !src.contains("'J' =>"));
        }
    }
}
