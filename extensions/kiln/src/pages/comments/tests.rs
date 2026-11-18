//! The Comments tab's tests: its rows from kiln's recorded refusals and
//! the hand-written answers (tests/fixtures/hand/README says from what),
//! the arguments it asks with, other people's writing shown as plain text,
//! and the tab on a page.

use super::*;
use crate::config::{self, Config};
use crate::model::tests::fixture;
use crate::pages::exports::tests::answer;
use crate::pages::view::{Kind, Looks};
use crate::project::Loaded;

fn demo() -> Loaded {
    Loaded::new(
        model::Version::parse("kiln 1.0.0").unwrap(),
        model::list(&fixture("demo/list.json")).unwrap(),
        model::status(&fixture("demo/status.json")),
        model::check(&fixture("demo/check.json")),
    )
}

/// The exports demo's `kiln.toml`: two Confluence targets among five.
fn conf() -> Config {
    config::parse(&fixture("exports/kiln.toml")).unwrap()
}

fn hand(name: &str, ext: &str) -> Result<Ran, Failure> {
    answer(&format!("../hand/{name}"), ext)
}

fn rec(name: &str, ext: &str) -> Result<Ran, Failure> {
    answer(&format!("comments/{name}"), ext)
}

struct World {
    loaded: Loaded,
    looks: Looks,
    config: Result<Config, String>,
    reading: bool,
}

impl World {
    fn new() -> World {
        let loaded = demo();
        let looks = Looks::new(&loaded);
        World {
            loaded,
            looks,
            config: Ok(conf()),
            reading: false,
        }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            loaded: &self.loaded,
            closed: false,
            reading: self.reading,
            looks: &self.looks,
            changes: 1,
            config: self.config.as_ref().map_err(String::as_str),
            publishing: None,
        }
    }
}

/// The rows as text: `# ` before a heading, two spaces a level.
fn lines(c: &Comments, w: &World) -> Vec<String> {
    c.rows(&w.ctx())
        .iter()
        .map(|r| {
            let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
            let mark = if r.kind == Kind::Heading { "# " } else { "" };
            format!("{mark}{}{}", "  ".repeat(r.depth), text.join(" | "))
        })
        .collect()
}

/// The tab having been asked and answered `result`.
fn heard(w: &World, result: Result<Ran, Failure>) -> Comments {
    let mut c = Comments::default();
    for q in c.queries(&w.ctx()) {
        c.answer(&q.tag, &result);
    }
    c
}

fn row(c: &Comments, w: &World, starts: &str) -> Row {
    c.rows(&w.ctx())
        .into_iter()
        .find(|r| r.selectable() && r.cells[0].text.starts_with(starts))
        .unwrap_or_else(|| panic!("no row {starts}"))
}

fn press(c: &mut Comments, w: &World, key: char, at: Option<&Row>) -> Reply {
    c.key(KeyPress::from(Key::Char(key)), at, &w.ctx())
}

// -- Asking ---------------------------------------------------------------------

#[test]
fn one_read_of_every_target_with_a_long_limit_once_a_round() {
    let w = World::new();
    let c = Comments::default();
    let qs = c.queries(&w.ctx());
    assert_eq!(qs.len(), 1);
    assert_eq!(
        qs[0].args,
        ["confluence", "comments", "--format", "json"],
        "no target named: kiln takes every Confluence one"
    );
    assert_eq!(qs[0].round, Some(0));
    assert_eq!(qs[0].limit_ms, Some(COMMENTS_MS));
    assert!(!qs[0].settled);
}

#[test]
fn with_no_confluence_target_in_kiln_toml_it_says_so_and_asks_kiln_nothing() {
    let mut w = World::new();
    w.config = Ok(config::parse(&fixture("jira/kiln.toml")).unwrap());
    let c = Comments::default();
    assert!(c.queries(&w.ctx()).is_empty());
    assert_eq!(
        lines(&c, &w),
        ["kiln.toml has no Confluence export target, so there are no comments to read"]
    );
    // Not even on R.
    let mut c = Comments::default();
    assert_eq!(press(&mut c, &w, 'R', None), Reply::Rebuild);
    assert!(c.queries(&w.ctx()).is_empty());
}

#[test]
fn when_kiln_toml_cant_be_read_kiln_is_asked_all_the_same() {
    let mut w = World::new();
    w.config = Err("kiln.toml can't be read".into());
    let c = Comments::default();
    assert_eq!(c.queries(&w.ctx()).len(), 1, "kiln is the authority");
    // What kiln lists is what's shown.
    let c = heard(&w, hand("comments", "json"));
    let l = lines(&c, &w);
    assert!(l[0].starts_with("# design-doc-confluence → space DEMO: 4 unresolved comments"));
    assert!(l.iter().any(|l| l.starts_with("# adopted")));
}

#[test]
fn r_reads_again_and_not_while_one_is_read() {
    let mut w = World::new();
    let mut c = heard(&w, hand("comments", "json"));
    w.reading = true;
    assert_eq!(
        press(&mut c, &w, 'R', None),
        Reply::Say("the comments are still being read: R once they're shown".into())
    );
    assert_eq!(c.queries(&w.ctx())[0].round, Some(0));
    w.reading = false;
    assert_eq!(press(&mut c, &w, 'R', None), Reply::Rebuild);
    assert_eq!(c.queries(&w.ctx())[0].round, Some(1), "the next round");
    // The last answer stays under "reading…" until the new one comes.
    let l = lines(&c, &w);
    assert_eq!(l[0], "reading… (asks Confluence)");
    assert!(l[1].starts_with("# design-doc-confluence"), "{l:?}");
}

// -- What it shows -----------------------------------------------------------------

#[test]
fn before_the_first_answer_it_says_it_is_reading_confluence() {
    let w = World::new();
    assert_eq!(
        lines(&Comments::default(), &w),
        ["reading… (asks Confluence)"]
    );
}

#[test]
fn the_comments_by_target_and_by_the_item_they_are_about() {
    let w = World::new();
    let c = heard(&w, hand("comments", "json"));
    assert_eq!(
        lines(&c, &w),
        [
            "# design-doc-confluence → space DEMO: 4 unresolved comments",
            "  REQ-001 | Readings are logged every second",
            "    Alice Martin · 2026-10-12 | Readings are logged every second Is that measured cold?",
            "    Bob Chen · 2026-10-13 | Per sensor, or in total?",
            "  TASK-006 | Draft the user guide",
            "    Bob Chen · 2026-10-13 | REQ-099 doesn't exist, which outline did you mean?",
            "  about the page “Demo — design document”",
            "    Carla Gómez · 2026-10-14 | Can we get a PDF of this?",
            "# adopted → space DEMO: 0 unresolved comments",
            "  nothing to answer",
        ]
    );
    // The same as kiln's own text says: the heading is its words.
    let text = "spec → space FLINT: 2 unresolved comments";
    assert!(text.contains("→ space ") && text.ends_with("unresolved comments"));
}

#[test]
fn an_item_row_is_the_items_and_a_comment_row_is_not() {
    let w = World::new();
    let c = heard(&w, hand("comments", "json"));
    let item = row(&c, &w, "REQ-001");
    assert_eq!(item.id.as_deref(), Some("REQ-001"), "v and i use it");
    assert_eq!(
        item.place.as_ref().map(|p| (p.path.as_str(), p.line)),
        Some((
            "design/requirements/REQ-001-readings-are-logged-every-second.md",
            Some(2)
        )),
        "Enter opens its file"
    );
    let comment = row(&c, &w, "Alice Martin");
    assert!(comment.id.is_none() && comment.place.is_none());
    // The page's own comment has no item above it.
    assert!(row(&c, &w, "Carla").id.is_none());
}

#[test]
fn an_item_the_project_hasnt_is_said_so_not_hidden() {
    let w = World::new();
    let json = fixture("../hand/comments.json").replace("TASK-006", "TASK-404");
    let c = heard(
        &w,
        Ok(Ran {
            code: Some(0),
            stdout: json,
            stderr: vec![],
        }),
    );
    assert!(lines(&c, &w).contains(&"  TASK-404 | (not in the project)".to_string()));
    assert_eq!(row(&c, &w, "TASK-404").place, None, "no file to open");
}

#[test]
fn a_target_with_no_page_yet_is_not_published_and_asks_for_nothing_more() {
    let w = World::new();
    let c = heard(&w, hand("comments-unpublished", "json"));
    assert_eq!(
        lines(&c, &w),
        [
            "# design-doc-confluence → not published yet",
            "  kiln lists no comments for a target with no page: publish it first",
            "# adopted → space DEMO: 0 unresolved comments",
            "  nothing to answer",
        ]
    );
}

#[test]
fn nothing_published_is_an_empty_array_and_every_target_says_so() {
    let w = World::new();
    let c = heard(
        &w,
        Ok(Ran {
            code: Some(0),
            stdout: "[]\n".into(),
            stderr: vec![],
        }),
    );
    let l = lines(&c, &w);
    assert_eq!(l[0], "# design-doc-confluence → not published yet");
    assert_eq!(l[2], "# adopted → not published yet");
}

#[test]
fn other_peoples_writing_is_plain_text_on_one_line() {
    let w = World::new();
    let c = heard(&w, hand("comments-hostile", "json"));
    let rows = c.rows(&w.ctx());
    let comment = rows.iter().find(|r| r.depth == 2).unwrap();
    let who = &comment.cells[0].text;
    let text = &comment.cells[1].text;
    // No escape sequence, control character, newline or tab survives; the
    // markup and the link in it are text, not interpreted.
    for shown in [who, text] {
        assert!(!shown.chars().any(|c| c.is_control()), "{shown:?}");
    }
    assert_eq!(who, "Mal Lory · 2026-10-15");
    // The terminal hyperlink (OSC 8) goes whole, its hidden target with it:
    // only what a reader would see is left, as text.
    assert!(
        text.starts_with("<script>alert(1)</script> **not bold** [[REQ-002]] click and a new line"),
        "{text}"
    );
    assert!(!text.contains("evil.example"), "{text}");
    // Every cell of every row is one line.
    assert!(rows
        .iter()
        .flat_map(|r| &r.cells)
        .all(|c| !c.text.contains('\n') && !c.text.contains('\u{1b}')));
}

#[test]
fn a_long_text_is_cut() {
    let w = World::new();
    let json = serde_json::json!([{"name": "design-doc-confluence", "space": "DEMO", "comments": [{
        "author": "A".repeat(500), "created": "2026-10-12T09:15:00.000Z", "item": null,
        "page": 1, "page_title": "T".repeat(500), "text": "x".repeat(5000), "url": "http://127.0.0.1:9/x"
    }]}])
    .to_string();
    let c = heard(
        &w,
        Ok(Ran {
            code: Some(0),
            stdout: json,
            stderr: vec![],
        }),
    );
    for r in c.rows(&w.ctx()) {
        for cell in r.cells {
            assert!(cell.text.chars().count() <= 400, "{}", cell.text.len());
        }
    }
}

// -- What kiln refuses -----------------------------------------------------------

#[test]
fn kilns_line_is_the_whole_tabs_not_a_targets() {
    let w = World::new();
    // Recorded here: a token with nothing listening (exit 3), and no token
    // on a machine with no credential store (exit 2).
    for (name, hint) in [("unreachable", false), ("no-token", true)] {
        let r = rec(name, "json");
        let line = r.as_ref().unwrap().error_line();
        let c = heard(&w, r);
        let l = lines(&c, &w);
        assert_eq!(l[0], model::plain(&line, 500), "{name}");
        assert_eq!(l.len(), if hint { 2 } else { 1 }, "{name}: {l:?}");
        assert!(!l.iter().any(|l| l.starts_with("# ")), "no target headings");
        if hint {
            assert!(l[1].contains("A opens one below"));
        }
    }
    // No token on a desktop with a credential store (hand-written).
    let c = heard(&w, hand("comments-no-token", "json"));
    let l = lines(&c, &w);
    assert!(l[0].starts_with("kiln: no token for http://127.0.0.1:9: run `kiln auth confluence`"));
    assert!(l[1].contains("kiln auth confluence in a terminal"));
}

#[test]
fn the_recorded_refusals_with_a_kiln_toml_the_extension_couldnt_read() {
    let mut w = World::new();
    w.config = Err("no kiln.toml".into());
    for (name, line) in [
        ("no-confluence", "kiln: kiln.toml has no [confluence]"),
        (
            "no-target",
            "kiln: kiln.toml declares no Confluence export target",
        ),
    ] {
        let c = heard(&w, rec(name, "json"));
        assert_eq!(lines(&c, &w), [line], "{name}");
    }
}

#[test]
fn a_answer_that_isnt_the_shape_is_said_not_guessed_at() {
    let w = World::new();
    let c = heard(
        &w,
        Ok(Ran {
            code: Some(0),
            stdout: "{}".into(),
            stderr: vec![],
        }),
    );
    assert!(lines(&c, &w)[0].starts_with("kiln confluence comments: expected a JSON array"));
    let c = heard(&w, Err(Failure::TimedOut(600)));
    assert_eq!(
        lines(&c, &w),
        ["kiln gave no answer in 10 minutes, so it was stopped"]
    );
}

// -- Keys --------------------------------------------------------------------------

#[test]
fn o_opens_the_comment_under_the_cursor_and_nothing_else() {
    let w = World::new();
    let mut c = heard(&w, hand("comments", "json"));
    let alice = row(&c, &w, "Alice Martin");
    assert_eq!(
        press(&mut c, &w, 'o', Some(&alice)),
        Reply::Url(
            "http://127.0.0.1:9/pages/viewpage.action?pageId=2001&focusedCommentId=456#comment-456"
                .into()
        )
    );
    let carla = row(&c, &w, "Carla");
    assert!(
        matches!(press(&mut c, &w, 'o', Some(&carla)), Reply::Url(u) if u.ends_with("#comment-457"))
    );
    // An item is no comment.
    let item = row(&c, &w, "REQ-001");
    assert!(
        matches!(press(&mut c, &w, 'o', Some(&item)), Reply::Say(s) if s.contains("this is an item"))
    );
    assert_eq!(
        press(&mut c, &w, 'o', None),
        Reply::Say("no comment here".into())
    );
    // A comment kiln gave no link for says so.
    let json = fixture("../hand/comments.json").replace(
        "http://127.0.0.1:9/pages/viewpage.action?pageId=2001&focusedCommentId=457#comment-457",
        "",
    );
    let mut c = heard(
        &w,
        Ok(Ran {
            code: Some(0),
            stdout: json,
            stderr: vec![],
        }),
    );
    let carla = row(&c, &w, "Carla");
    assert_eq!(
        press(&mut c, &w, 'o', Some(&carla)),
        Reply::Say("kiln gave no link for this comment".into())
    );
    // Keys held with a modifier, and others, aren't its.
    assert_eq!(press(&mut c, &w, 'x', None), Reply::Ignored);
    assert_eq!(
        c.key(
            KeyPress::new(Key::Char('o'), fenix_ui_kit::Mods::CTRL),
            Some(&carla),
            &w.ctx()
        ),
        Reply::Ignored
    );
}

#[test]
fn a_hostile_link_is_handed_on_as_it_is_for_the_frame_to_refuse() {
    // The view doesn't judge a link: the frame's `Reply::Url` does (and is
    // tested there); here, that the view hands over what kiln gave.
    let w = World::new();
    let mut c = heard(&w, hand("comments-hostile", "json"));
    let r = row(&c, &w, "Mal");
    assert_eq!(
        press(&mut c, &w, 'o', Some(&r)),
        Reply::Url("javascript:alert(1)".into())
    );
}

#[test]
fn a_opens_kiln_auth_confluence_in_a_terminal_only_for_a_missing_token() {
    let w = World::new();
    let mut c = heard(&w, rec("no-token", "json"));
    assert_eq!(
        press(&mut c, &w, 'A', None),
        Reply::Terminal {
            title: "kiln auth confluence".into(),
            args: auth_args()
        }
    );
    // The helper Exports uses, not another.
    assert_eq!(auth_args(), ["auth", "confluence"]);
    let mut c = heard(&w, rec("unreachable", "json"));
    assert!(matches!(press(&mut c, &w, 'A', None), Reply::Say(_)));
    let mut c = heard(&w, hand("comments", "json"));
    assert!(matches!(press(&mut c, &w, 'A', None), Reply::Say(_)));
}

#[test]
fn it_builds_no_command_that_writes() {
    // The one it asks; and no reply of the kinds that run kiln but this.
    assert_eq!(args(), ["confluence", "comments", "--format", "json"]);
    let code: String = include_str!("../comments.rs")
        .lines()
        .take_while(|l| !l.starts_with("#[cfg(test)]"))
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for word in [
        "Reply::Run",
        "Reply::Edit",
        "--yes",
        "--force",
        "\"export\"",
    ] {
        assert!(!code.contains(word), "{word}");
    }
}

// -- On a page ----------------------------------------------------------------------

mod page {
    use super::*;
    use crate::pages::frame::{Hints, Now, Plan};
    use crate::pages::Tab;
    use crate::project::State;
    use crate::tests::{demo as demo_fake, folder, foot, harness, spawned};
    use crate::Kiln;
    use fenix_test_kit::{assert_snapshot, Fake, Harness, Ran as Run};

    fn run(r: Result<Ran, Failure>) -> Run {
        let r = r.unwrap();
        let mut run = Run::exits(r.code.unwrap_or(0)).stdout(r.stdout);
        if !r.stderr.is_empty() {
            run = run.stderr(r.stderr.join("\n"));
        }
        run
    }

    /// The demo's kiln, the exports demo's `kiln.toml`, and `comments` as
    /// kiln's answer to `confluence comments`.
    fn fake(test: &str, comments: Run) -> (Fake, std::path::PathBuf) {
        let fake = demo_fake();
        let dir = folder(&format!("comments-{test}"));
        std::fs::write(dir.join("kiln.toml"), fixture("exports/kiln.toml")).unwrap();
        fake.on_spawn("kiln confluence comments --format json", comments);
        (fake, dir)
    }

    fn open(fake: &Fake, dir: &std::path::Path) -> (Harness<Kiln>, u64) {
        let mut h = harness(fake, dir);
        h.size = (110, 16);
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        // Straight to Comments: `Tab` through the others would ask what
        // each of them asks (Exports' dry runs).
        h.ext.pages.get_mut(&page).unwrap().plan.show(Tab::Comments);
        h.ext.pass_on();
        h.settle();
        (h, page)
    }

    fn asks(fake: &Fake) -> usize {
        spawned(fake)
            .iter()
            .filter(|l| l.ends_with("kiln confluence comments --format json"))
            .count()
    }

    #[test]
    fn the_comments_under_each_target_by_item() {
        let (fake, dir) = fake("rows", run(hand("comments", "json")));
        let (mut h, page) = open(&fake, &dir);
        assert_snapshot!("comments-demo", h.screen(page));
    }

    #[test]
    fn first_shown_one_read_a_reload_none_r_one() {
        let (fake, dir) = fake("once", run(hand("comments", "json")));
        let (mut h, page) = open(&fake, &dir);
        assert_eq!(asks(&fake), 1);
        h.keys(page, "r");
        h.settle();
        h.saved("design/tasks/TASK-001.md");
        h.settle();
        assert_eq!(asks(&fake), 0, "the project read again, not the comments");
        h.keys(page, "R");
        assert_eq!(asks(&fake), 1);
        // Other tabs don't ask: only the one shown.
        h.keys(page, "1");
        h.keys(page, "R");
        assert_eq!(asks(&fake), 0);
    }

    #[test]
    fn it_says_it_reads_confluence_while_kiln_works() {
        let (fake, dir) = fake("reading", Run::running(""));
        let (mut h, page) = open(&fake, &dir);
        let s = h.screen(page);
        assert!(s.contains("reading… (asks Confluence)"), "{s}");
        assert_eq!(foot(&mut h, page), "reading…");
        assert_snapshot!("comments-demo-reading", s);
    }

    #[test]
    fn o_opens_the_comment_in_the_browser() {
        let (fake, dir) = fake("open", run(hand("comments", "json")));
        let (mut h, page) = open(&fake, &dir);
        // The cursor on the first comment: past the item's row.
        // The filter keeps the item above the comment, the cursor on it.
        h.keys(page, "/Alice<Enter>j");
        h.keys(page, "o");
        assert_eq!(
            fake.state().urls,
            [
                "http://127.0.0.1:9/pages/viewpage.action?pageId=2001&focusedCommentId=456#comment-456"
            ]
        );
        // On an item's row it opens nothing and says so; Enter opens its file.
        h.keys(page, "/<Esc>");
        h.keys(page, "gg");
        fake.take();
        h.keys(page, "o");
        assert_eq!(fake.state().urls.len(), 1);
        assert!(
            foot(&mut h, page).contains("this is an item"),
            "{}",
            foot(&mut h, page)
        );
        h.keys(page, "<Enter>");
        let opened = fake.take();
        assert!(
            opened.len() == 1
                && opened[0].ends_with(
                    "/design/requirements/REQ-001-readings-are-logged-every-second.md:2"
                ),
            "{opened:?}"
        );
        // And `v` follows it, as on any tab.
        h.keys(page, "v");
        assert_eq!(h.ext.pages[&page].plan.tab(), Tab::Item);
    }

    #[test]
    fn a_link_that_isnt_http_is_refused_and_nothing_is_opened() {
        let (fake, dir) = fake("hostile", run(hand("comments-hostile", "json")));
        let (mut h, page) = open(&fake, &dir);
        h.keys(page, "/Mal<Enter>j");
        h.keys(page, "o");
        assert!(fake.state().urls.is_empty());
        assert!(
            foot(&mut h, page)
                .ends_with("not opening javascript:alert(1): only http and https links are opened"),
            "{}",
            foot(&mut h, page)
        );
    }

    #[test]
    fn the_host_is_asked_to_open_only_an_http_link_even_past_the_frame() {
        use crate::pages::frame::Effect;
        let (fake, dir) = fake("guard", run(hand("comments", "json")));
        let (mut h, page) = open(&fake, &dir);
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://x/y",
            "C:\\x.exe",
            "",
        ] {
            h.ext.perform(page, vec![Effect::Url(bad.into())]);
        }
        assert!(fake.state().urls.is_empty(), "{:?}", fake.state().urls);
        assert!(foot(&mut h, page).starts_with("not opening"));
        h.ext.perform(
            page,
            vec![Effect::Url("https://confluence.example.com/x".into())],
        );
        assert_eq!(fake.state().urls, ["https://confluence.example.com/x"]);
    }

    #[test]
    fn no_token_shows_kilns_line_and_a_opens_a_terminal() {
        let (fake, dir) = fake("no-token", run(rec("no-token", "json")));
        let (mut h, page) = open(&fake, &dir);
        assert_snapshot!("comments-demo-no-token", h.screen(page));
        fake.take();
        h.keys(page, "A");
        assert!(
            fake.take()
                .iter()
                .any(|c| c.ends_with("kiln auth confluence (below): /usr/bin/kiln auth confluence")),
            "{:?}",
            fake.calls()
        );
        assert!(!fake
            .calls()
            .iter()
            .any(|c| c.contains("secret") || c.starts_with("env")));
    }

    #[test]
    fn a_project_with_no_confluence_target_says_so_and_asks_kiln_nothing() {
        let fake = demo_fake();
        let dir = folder("comments-none");
        std::fs::write(dir.join("kiln.toml"), fixture("jira/kiln.toml")).unwrap();
        let (mut h, page) = open(&fake, &dir);
        assert_eq!(asks(&fake), 0);
        assert_snapshot!("comments-demo-no-target", h.screen(page));
    }

    #[test]
    fn the_frame_asks_it_once_a_round_and_holds_nothing_back() {
        let l = demo();
        let c = conf();
        let now = |loads: u64| Now {
            state: State::Loaded {
                loaded: &l,
                reloading: false,
                error: None,
            },
            loads,
            config: Ok(&c),
            settled: true,
            changes: 1,
            publishing: None,
            tickets: None,
        };
        let mut p = Plan::new();
        p.show(Tab::Comments);
        let asks = p.wants(now(1));
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].tag, "comments");
        assert_eq!(asks[0].limit_ms, Some(COMMENTS_MS));
        p.asked(&asks[0], 5, now(1));
        assert!(p.wants(now(1)).is_empty(), "out");
        assert!(p.answered(Tab::Comments, "comments", 5, &hand("comments", "json")));
        assert!(p.wants(now(2)).is_empty(), "a reload asks nothing");
        let _ = Hints::default();
    }
}
