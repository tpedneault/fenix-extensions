//! The Exports tab's tests: its rows from every recorded answer (and the
//! hand-written ones for what needs a server), the publish flow as a
//! table, the arguments it can build, and the tab on a page.

use super::*;
use crate::config::{self, Config};
use crate::model::tests::fixture;
use crate::pages::view::{publishing_line, Kind, Looks};
use crate::project::Loaded;

fn demo() -> Loaded {
    Loaded::new(
        model::Version::parse("kiln 1.0.0").unwrap(),
        model::list(&fixture("demo/list.json")).unwrap(),
        model::status(&fixture("demo/status.json")),
        model::check(&fixture("demo/check.json")),
    )
}

/// The exports demo's `kiln.toml`: five targets, two of them Confluence.
fn conf() -> Config {
    config::parse(&fixture("exports/kiln.toml")).unwrap()
}

/// The exports demo's `kiln.toml` with a `[jira]` that syncs.
fn jira_toml() -> String {
    format!(
        "{}\n[jira]\nurl = \"http://127.0.0.1:9\"\nproject = \"DEMO\"\n",
        fixture("exports/kiln.toml")
    )
}

/// A recorded answer (`out/<path>.<ext>`, its `.stderr` and `.exit`), as
/// the queue hands it over.
pub(crate) fn answer(path: &str, ext: &str) -> Result<Ran, Failure> {
    let read = |p: &str| {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/out/{p}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .ok()
    };
    let stdout = read(&format!("{path}.{ext}")).unwrap_or_default();
    let stderr = read(&format!("{path}.stderr"))
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let code = read(&format!("{path}.exit"))
        .unwrap_or_else(|| panic!("{path}.exit"))
        .trim()
        .parse()
        .ok();
    Ok(Ran {
        code,
        stdout,
        stderr,
    })
}

/// A recorded one under `out/exports/`; a hand-written one under `hand/`.
fn rec(name: &str, ext: &str) -> Result<Ran, Failure> {
    answer(&format!("exports/{name}"), ext)
}
fn hand(name: &str, ext: &str) -> Result<Ran, Failure> {
    answer(&format!("../hand/{name}"), ext)
}

struct World {
    loaded: Loaded,
    looks: Looks,
    config: Result<Config, String>,
    changes: u64,
    publishing: Option<String>,
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
            changes: 1,
            publishing: None,
            reading: false,
        }
    }

    /// With `[jira] project` in `kiln.toml`: the tab has a `jira` target.
    fn with_jira() -> World {
        let mut w = World::new();
        w.config = Ok(config::parse(&jira_toml()).unwrap());
        w
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            loaded: &self.loaded,
            closed: false,
            reading: self.reading,
            looks: &self.looks,
            changes: self.changes,
            config: self.config.as_ref().map_err(String::as_str),
            publishing: self.publishing.as_deref(),
        }
    }
}

/// The rows as text: `# ` before a heading, two spaces a level.
fn lines(x: &Exports, w: &World) -> Vec<String> {
    x.rows(&w.ctx())
        .iter()
        .map(|r| {
            let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
            let mark = if r.kind == Kind::Heading { "# " } else { "" };
            format!("{mark}{}{}", "  ".repeat(r.depth), text.join(" | "))
        })
        .collect()
}

/// The rows of target `name` alone: its heading and what's under it.
fn of(x: &Exports, w: &World, name: &str) -> Vec<String> {
    let all = lines(x, w);
    let start = all
        .iter()
        .position(|l| l.starts_with(&format!("# {name} ")) || *l == format!("# {name}"))
        .unwrap_or_else(|| panic!("no {name} in {all:#?}"));
    let end = all[start + 1..]
        .iter()
        .position(|l| l.starts_with("# "))
        .map_or(all.len(), |e| start + 1 + e);
    all[start..end].to_vec()
}

/// Every target's dry run answered with `answers` (by name), the others
/// with the demo's first, all at the world's change count.
fn answered(w: &World, answers: &[(&str, Result<Ran, Failure>)]) -> Exports {
    let mut x = Exports::default();
    for q in x.queries(&w.ctx()) {
        let name = q.tag.strip_prefix(DRY).unwrap_or("").to_string();
        let result = answers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| rec("dry-run", "json"));
        x.answer_at(&q.tag, &result, w.changes);
    }
    x
}

/// A row, by the start of its text, as the frame hands it to `key`.
fn row(x: &Exports, w: &World, starts: &str) -> Row {
    x.rows(&w.ctx())
        .into_iter()
        .find(|r| r.selectable() && r.cells[0].text.starts_with(starts))
        .unwrap_or_else(|| panic!("no row {starts}"))
}

fn press(x: &mut Exports, w: &World, c: char, at: &Row) -> Reply {
    x.key(KeyPress::from(Key::Char(c)), Some(at), &w.ctx())
}

// -- Reading ----------------------------------------------------------------

#[test]
fn one_dry_run_per_target_named_after_two_dashes_with_a_long_limit() {
    let w = World::new();
    let x = Exports::default();
    let qs = x.queries(&w.ctx());
    let names: Vec<&str> = qs.iter().map(|q| q.tag.as_str()).collect();
    assert_eq!(
        names,
        [
            "dry-run:design-doc",
            "dry-run:requirements-html",
            "dry-run:sections-md",
            "dry-run:design-doc-confluence",
            "dry-run:adopted"
        ]
    );
    assert_eq!(
        qs[3].args,
        [
            "export",
            "--dry-run",
            "--format",
            "json",
            "--",
            "design-doc-confluence"
        ]
    );
    assert!(qs
        .iter()
        .all(|q| q.round == Some(0) && q.limit_ms == Some(DRY_RUN_MS)));
}

#[test]
fn file_targets_say_what_would_change_as_the_command_line_does() {
    let w = World::new();
    let x = answered(&w, &[]);
    // Each read compared with the text kiln printed for the same state.
    assert_eq!(
        of(&x, &w, "design-doc"),
        [
            "# design-doc (markdown, document) → docs/DESIGN-DOCUMENT.md",
            "  would create docs/DESIGN-DOCUMENT.md",
        ]
    );
    assert_eq!(fixture("exports/dry-run.txt").lines().count(), 2);
    for (json, txt) in [
        ("dry-run-unchanged", "  unchanged (1 file)"),
        ("dry-run-update", "  would update docs/DESIGN-DOCUMENT.md"),
    ] {
        let x = answered(&w, &[("design-doc", rec(json, "json"))]);
        assert_eq!(of(&x, &w, "design-doc")[1], txt);
        assert!(
            fixture(&format!("exports/{json}.txt")).contains(txt),
            "{json}"
        );
    }
    let x = answered(
        &w,
        &[
            ("requirements-html", rec("dry-run-left-out", "json")),
            ("sections-md", rec("dry-run-remove", "json")),
        ],
    );
    assert_eq!(
        of(&x, &w, "requirements-html"),
        [
            "# requirements-html (html, document) → docs/requirements.html",
            "  would create docs/requirements.html",
            "  leaves out: ADR-001, MS-001, MS-002, TASK-001, TASK-002, TASK-003, TASK-004, TASK-005, TASK-006, TASK-007, TASK-008, TEST-001, RISK-001",
        ]
    );
    let sections = of(&x, &w, "sections-md");
    let printed: Vec<String> = fixture("exports/dry-run-remove.txt")
        .lines()
        .map(|l| l.to_string())
        .collect();
    assert_eq!(sections[0], format!("# {}", printed[0]));
    assert_eq!(
        sections[1..],
        printed[1..],
        "line for line as kiln printed it"
    );
}

#[test]
fn a_file_that_exists_opens_and_one_to_be_created_doesnt() {
    let w = World::new();
    let x = answered(&w, &[("design-doc", rec("dry-run-update", "json"))]);
    let update = row(&x, &w, "would update docs/DESIGN-DOCUMENT.md");
    assert_eq!(
        update.place.as_ref().map(|p| p.path.as_str()),
        Some("docs/DESIGN-DOCUMENT.md")
    );
    assert_eq!(update.id, None, "no item: the edits say so");
    let x = answered(&w, &[]);
    assert_eq!(row(&x, &w, "would create").place, None);
}

#[test]
fn confluence_plans_are_kilns_lines_ops_totals_and_refusals() {
    let w = World::new();
    for (name, snapshot) in [
        ("design-doc-confluence", "confluence-empty"),
        ("design-doc-confluence", "confluence-update"),
        ("design-doc-confluence", "confluence-drift"),
        ("design-doc-confluence", "confluence-drift-skip"),
        ("adopted", "confluence-adopt"),
    ] {
        let x = answered(&w, &[(name, rec(snapshot, "json"))]);
        let mut rows = of(&x, &w, name);
        // Kiln's text, line for line: the heading, then the plan.
        let printed: Vec<String> = fixture(&format!("exports/{snapshot}.txt"))
            .lines()
            .map(|l| l.to_string())
            .collect();
        let refused = rows.iter().any(|r| r.starts_with("  refused:"));
        if refused {
            assert_eq!(
                rows.pop().unwrap(),
                "  kiln refuses this plan, so P publishes nothing of it; overriding it (--force, --adopt) is done in a terminal",
                "{snapshot}"
            );
        }
        assert_eq!(rows[0], format!("# {}", printed[0]), "{snapshot}");
        assert_eq!(rows[1..], printed[1..], "{snapshot}");
    }
}

#[test]
fn a_target_kiln_cant_plan_shows_its_line_and_the_others_still_show() {
    let w = World::new();
    let x = answered(
        &w,
        &[
            ("design-doc-confluence", rec("confluence-no-token", "json")),
            ("adopted", rec("confluence-unreachable", "json")),
        ],
    );
    assert_eq!(
        of(&x, &w, "design-doc-confluence"),
        [
            "# design-doc-confluence (confluence, document) → space DEMO, page 123456789",
            "  kiln: couldn't read `confluence:http://127.0.0.1:9` from the credential store: Platform secure storage failure: no secret service provider or dbus session found (or set KILN_CONFLUENCE_TOKEN)",
            "  run kiln auth confluence in a terminal: A opens one below, then R reads this again",
        ]
    );
    // A network error (exit 3) is its line alone: no token would help.
    assert_eq!(
        of(&x, &w, "adopted"),
        [
            "# adopted (confluence, document) → space DEMO, page 555",
            "  kiln: Confluence GET /rest/api/content/123456789: http://127.0.0.1:9/rest/api/content/123456789?expand=version,ancestors,space: Connection Failed: Connect error: Connection refused (os error 111)",
        ]
    );
    assert_eq!(
        of(&x, &w, "design-doc")[1],
        "  would create docs/DESIGN-DOCUMENT.md"
    );
    // The hand-written "no token" of a credential store that has none.
    let x = answered(
        &w,
        &[("design-doc-confluence", hand("export-no-token", "json"))],
    );
    assert_eq!(
        of(&x, &w, "design-doc-confluence")[1..],
        [
            "  kiln: no token for http://127.0.0.1:9: run `kiln auth confluence`, or set KILN_CONFLUENCE_TOKEN (or plan offline with --snapshot FILE)",
            "  run kiln auth confluence in a terminal: A opens one below, then R reads this again",
        ]
    );
}

#[test]
fn without_kiln_toml_every_target_is_asked_at_once_and_one_failure_is_all() {
    let mut w = World::new();
    w.config = Err("there's no kiln.toml at the project's top".into());
    let mut x = Exports::default();
    let qs = x.queries(&w.ctx());
    assert_eq!(qs.len(), 1);
    assert_eq!(qs[0].args, ["export", "--dry-run", "--format", "json"]);
    assert_eq!(
        lines(&x, &w),
        [
            "there's no kiln.toml at the project's top, so kiln was asked about every target at once",
            "reading…"
        ]
    );
    x.answer_at(DRY_ALL, &rec("all-no-token", "json"), 1);
    assert_eq!(
        lines(&x, &w)[1],
        "kiln: couldn't read `confluence:http://127.0.0.1:9` from the credential store: Platform secure storage failure: no secret service provider or dbus session found (or set KILN_CONFLUENCE_TOKEN)"
    );
    x.answer_at(DRY_ALL, &rec("dry-run-all", "json"), 1);
    let all = lines(&x, &w);
    assert_eq!(
        all[1..5],
        [
            "# design-doc (markdown, document) → docs/DESIGN-DOCUMENT.md",
            "  would update docs/DESIGN-DOCUMENT.md",
            "# requirements-html (html, document) → docs/requirements.html",
            "  would create docs/requirements.html",
        ]
    );
    assert!(
        all.contains(&"# sections-md (markdown, sections)".to_string()),
        "a folder, which kiln's JSON doesn't name: {all:#?}"
    );
    // W names what kiln named.
    let at = row(&x, &w, "would update");
    match press(&mut x, &w, 'W', &at) {
        Reply::Run { args, .. } => assert_eq!(
            args,
            write_args(&["design-doc", "requirements-html", "sections-md"])
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn being_read_and_out_of_date_are_said_under_the_target() {
    let mut w = World::new();
    let x = Exports::default();
    let all = lines(&x, &w);
    assert_eq!(
        all[..2],
        [
            "# design-doc (markdown, document) → docs/DESIGN-DOCUMENT.md",
            "  reading…"
        ]
    );
    assert!(all.contains(&"  reading… (asks Confluence)".to_string()));
    let x = answered(&w, &[]);
    w.changes += 1;
    assert_eq!(
        of(&x, &w, "design-doc")[1],
        "  the project changed since this was read: R reads it again"
    );
}

#[test]
fn r_reads_every_target_again_and_not_while_one_is_read() {
    let mut w = World::new();
    let mut x = answered(&w, &[]);
    let at = row(&x, &w, "would create");
    w.reading = true;
    assert_eq!(
        press(&mut x, &w, 'R', &at),
        Reply::Say("the dry run is still being read: R once it's shown".into())
    );
    w.reading = false;
    assert_eq!(press(&mut x, &w, 'R', &at), Reply::Rebuild);
    assert!(x.queries(&w.ctx()).iter().all(|q| q.round == Some(1)));
}

// -- Writing ----------------------------------------------------------------

#[test]
fn w_writes_the_target_under_the_cursor_and_w_every_file_target_by_name() {
    let w = World::new();
    let mut x = answered(&w, &[]);
    let at = row(&x, &w, "would create docs/DESIGN-DOCUMENT.md");
    assert_eq!(
        press(&mut x, &w, 'w', &at),
        Reply::Run {
            tag: WRITE.into(),
            args: ["export", "--format", "json", "--", "design-doc"]
                .map(String::from)
                .to_vec(),
            limit_ms: Some(WRITE_MS),
            publish: None,
        }
    );
    match press(&mut x, &w, 'W', &at) {
        Reply::Run {
            args,
            publish: None,
            ..
        } => assert_eq!(
            args,
            [
                "export",
                "--format",
                "json",
                "--",
                "design-doc",
                "requirements-html",
                "sections-md"
            ],
            "never a bare kiln export, which takes the Confluence targets too"
        ),
        other => panic!("{other:?}"),
    }
    let x2 = answered(
        &w,
        &[("design-doc-confluence", rec("confluence-empty", "json"))],
    );
    let mut x2 = x2;
    let plan = row(&x2, &w, "1 create");
    assert_eq!(
        press(&mut x2, &w, 'w', &plan),
        Reply::Say("design-doc-confluence publishes to Confluence: P publishes it".into())
    );
}

#[test]
fn a_write_says_what_kiln_wrote_and_reads_that_target_again() {
    let w = World::new();
    let mut x = answered(&w, &[]);
    let at = row(&x, &w, "would create docs/DESIGN-DOCUMENT.md");
    press(&mut x, &w, 'w', &at);
    let before = x.round_of("design-doc");
    assert_eq!(
        x.ran(WRITE, &rec("write", "json"), &w.ctx()),
        Reply::Say("design-doc: created docs/DESIGN-DOCUMENT.md".into())
    );
    assert_eq!(x.round_of("design-doc"), before + 1);
    assert_eq!(x.round_of("requirements-html"), 0, "the others aren't");
    assert_eq!(
        of(&x, &w, "design-doc")[1..3],
        ["  created docs/DESIGN-DOCUMENT.md", "  reading…"]
    );
    press(&mut x, &w, 'W', &at);
    assert_eq!(
        x.ran(WRITE, &rec("write-two", "json"), &w.ctx()),
        Reply::Say(
            "design-doc: updated docs/DESIGN-DOCUMENT.md; requirements-html: created docs/requirements.html"
                .into()
        )
    );
    // An unknown name is kiln's line.
    press(&mut x, &w, 'w', &at);
    assert_eq!(
        x.ran(WRITE, &rec("unknown", "out"), &w.ctx()),
        Reply::Bad(
            "kiln: no export target `nosuch` (design-doc, requirements-html, sections-md)".into()
        )
    );
}

// -- Publishing ---------------------------------------------------------------

/// The plan of `snapshot` read for design-doc-confluence, and the cursor
/// on its totals.
fn planned(w: &World, snapshot: Result<Ran, Failure>) -> (Exports, Row) {
    let x = answered(w, &[("design-doc-confluence", snapshot)]);
    let at = x
        .rows(&w.ctx())
        .into_iter()
        .find(|r| r.selectable() && r.key().starts_with("design-doc-confluence\u{1}"))
        .unwrap();
    (x, at)
}

const LABEL: &str = "Publish to Confluence: writes 2 pages in space DEMO, under page 123456789 \"Demo space home\" (0 create, 1 update, 0 unchanged, 0 drift, 1 retire). Type design-doc-confluence to publish";

#[test]
fn the_publish_flow_as_a_table() {
    let ask = Reply::ask(CONFIRM, LABEL, "");
    let run = Reply::Run {
        tag: "publish:design-doc-confluence".into(),
        args: [
            "export",
            "--yes",
            "--format",
            "json",
            "--",
            "design-doc-confluence",
        ]
        .map(String::from)
        .to_vec(),
        limit_ms: Some(PUBLISH_MS),
        publish: Some("design-doc-confluence".into()),
    };
    type Case = (
        &'static str,
        &'static str,
        fn(&mut World),
        Option<&'static str>,
        Reply,
    );
    let none: fn(&mut World) = |_| {};
    let cases: Vec<Case> = vec![
        ("the right name", "confluence-update", none, Some("design-doc-confluence"), run.clone()),
        (
            "a wrong name",
            "confluence-update",
            none,
            Some("design-doc"),
            Reply::Say("“design-doc” isn't design-doc-confluence: nothing was published".into()),
        ),
        (
            "the name with a space",
            "confluence-update",
            none,
            Some("design-doc-confluence "),
            Reply::Say("“design-doc-confluence ” isn't design-doc-confluence: nothing was published".into()),
        ),
        (
            "y",
            "confluence-update",
            none,
            Some("y"),
            Reply::Say("“y” isn't design-doc-confluence: nothing was published".into()),
        ),
        ("cancelled (Esc): nothing comes back", "confluence-update", none, None, ask.clone()),
        (
            "a stale dry run",
            "confluence-update",
            |w| w.changes += 1,
            None,
            Reply::Say("the project changed since design-doc-confluence's plan was read: R reads it again, then P".into()),
        ),
        (
            "a refused plan",
            "confluence-drift",
            none,
            None,
            Reply::Bad(format!("kiln refuses design-doc-confluence's plan: \"Demo — design document\" was edited in Confluence (edited by Ana at 2026-02-01T09:30:00Z, version 5); copy the edit into the repository, then run again with --force, or set on_drift = \"skip\" -- {TERMINAL}")),
        ),
        (
            "nothing to do",
            "confluence-drift-skip",
            none,
            None,
            Reply::Say("nothing to publish: design-doc-confluence's plan is 0 create, 0 update, 0 unchanged, 1 drift, 0 retire".into()),
        ),
        (
            "no token",
            "confluence-no-token",
            none,
            None,
            Reply::Bad("design-doc-confluence has no plan: kiln: couldn't read `confluence:http://127.0.0.1:9` from the credential store: Platform secure storage failure: no secret service provider or dbus session found (or set KILN_CONFLUENCE_TOKEN) -- A runs kiln auth confluence in a terminal, then R".into()),
        ),
        (
            "a publish running",
            "confluence-update",
            |w| w.publishing = Some("adopted".into()),
            None,
            Reply::Say("adopted is being published: one publish at a time".into()),
        ),
    ];
    for (what, snapshot, before, typed, expect) in cases {
        let mut w = World::new();
        let (mut x, at) = planned(&w, rec(snapshot, "json"));
        before(&mut w);
        let reply = press(&mut x, &w, 'P', &at);
        let reply = match (typed, &reply) {
            (Some(text), Reply::Ask { tag, label, .. }) => {
                assert_eq!(label, LABEL, "{what}");
                x.typed(tag, text, &w.ctx())
            }
            _ => reply,
        };
        assert_eq!(reply, expect, "{what}");
    }
}

#[test]
fn a_plan_read_again_while_the_name_is_typed_publishes_nothing() {
    let mut w = World::new();
    let (mut x, at) = planned(&w, rec("confluence-update", "json"));
    let Reply::Ask { tag, .. } = press(&mut x, &w, 'P', &at) else {
        panic!()
    };
    w.changes += 1;
    assert_eq!(
        x.typed(&tag, "design-doc-confluence", &w.ctx()),
        Reply::Say("design-doc-confluence's plan isn't the one shown any more (the project or the plan was read again): nothing was published; R, then P".into())
    );
    // A second answer without a second P is no one's.
    assert_eq!(
        x.typed(&tag, "design-doc-confluence", &w.ctx()),
        Reply::Ignored
    );
}

#[test]
fn p_on_a_plan_still_being_read_or_a_file_target_says_why() {
    let w = World::new();
    let mut x = Exports::default();
    let at = Row::entry("design-doc-confluence\u{1}x", vec![]);
    assert_eq!(
        press(&mut x, &w, 'P', &at),
        Reply::Say("design-doc-confluence's plan is still being read: P once it's shown".into())
    );
    let mut x = answered(&w, &[]);
    let file = row(&x, &w, "would create docs/DESIGN-DOCUMENT.md");
    assert_eq!(
        press(&mut x, &w, 'P', &file),
        Reply::Say("design-doc is a file target: w writes it".into())
    );
}

/// What a publish said, given kiln's answer, after the plan of
/// confluence-update was confirmed.
fn published(answer: Result<Ran, Failure>) -> (Reply, Vec<String>) {
    let w = World::new();
    let (mut x, at) = planned(&w, rec("confluence-update", "json"));
    let Reply::Ask { tag, .. } = press(&mut x, &w, 'P', &at) else {
        panic!()
    };
    x.typed(&tag, "design-doc-confluence", &w.ctx());
    let before = x.round_of("design-doc-confluence");
    let said = x.ran("publish:design-doc-confluence", &answer, &w.ctx());
    if !matches!(answer, Err(Failure::Unusable(_))) {
        assert_eq!(
            x.round_of("design-doc-confluence"),
            before + 1,
            "the dry run read again"
        );
    }
    (said, of(&x, &w, "design-doc-confluence"))
}

#[test]
fn a_publish_shows_kilns_own_outcome() {
    let (said, rows) = published(hand("export-publish-applied", "json"));
    let line = "design-doc-confluence: published 0 created, 1 updated, 1 retired, 0 pruned";
    assert_eq!(said, Reply::Say(line.into()));
    assert_eq!(rows[1], format!("  {line}"), "kept under the target");
    assert_eq!(rows[2], "  reading… (asks Confluence)");

    // kiln planned again as it published, and its plan wasn't the one shown.
    let (said, _) = published(hand("export-publish-applied-other-plan", "json"));
    assert_eq!(said, Reply::Bad("design-doc-confluence: published 0 created, 1 updated, 0 retired, 0 pruned -- kiln's plan as it published was 0 create, 1 update, 0 unchanged, 0 drift, 0 retire (the one shown: 0 create, 1 update, 0 unchanged, 0 drift, 1 retire)".into()));

    let (said, _) = published(hand("export-publish-nothing", "json"));
    assert_eq!(said, Reply::Bad("design-doc-confluence: nothing published: kiln's plan, read as it published, had nothing to do -- kiln's plan as it published was 0 create, 0 update, 1 unchanged, 0 drift, 0 retire (the one shown: 0 create, 1 update, 0 unchanged, 0 drift, 1 retire)".into()));

    let (said, _) = published(hand("export-publish-refused", "json"));
    assert_eq!(said, Reply::Bad(format!("nothing published: kiln refused design-doc-confluence's plan as it published: \"Demo — design document\" was edited in Confluence (edited by Ana at 2026-02-01T09:30:00Z, version 5); copy the edit into the repository, then run again with --force, or set on_drift = \"skip\" -- {TERMINAL}")));

    let (said, _) = published(hand("export-publish-network", "json"));
    assert_eq!(said, Reply::Bad("kiln: publishing `design-doc-confluence` stopped: Confluence PUT /rest/api/content/2001: http://127.0.0.1:9/rest/api/content/2001: Connection Failed: Connect error: Connection refused (os error 111); run again to finish -- the next publish finishes what this one started".into()));

    // Recorded: a network error before anything was written is exit 3 too.
    let (said, _) = published(rec("confluence-unreachable", "json"));
    assert!(
        matches!(&said, Reply::Bad(l) if l.ends_with("-- the next publish finishes what this one started"))
    );

    let (said, _) = published(hand("export-publish-space-changed", "json"));
    assert_eq!(said, Reply::Bad("kiln: `design-doc-confluence`: the space changed since the plan was read; run again to see the new plan".into()));

    let (said, _) = published(rec("publish-no-token", "json"));
    assert_eq!(said, Reply::Bad("kiln: couldn't read `confluence:http://127.0.0.1:9` from the credential store: Platform secure storage failure: no secret service provider or dbus session found (or set KILN_CONFLUENCE_TOKEN) -- A runs kiln auth confluence in a terminal".into()));
    let (said, _) = published(hand("export-publish-no-token", "json"));
    assert_eq!(said, Reply::Bad("kiln: no token for http://127.0.0.1:9: run `kiln auth confluence`, or set KILN_CONFLUENCE_TOKEN -- A runs kiln auth confluence in a terminal".into()));

    let (said, _) = published(Err(Failure::TimedOut(PUBLISH_MS / 1000)));
    assert_eq!(said, Reply::Bad("kiln gave no answer in 120 minutes, so it was stopped: the publish may have stopped half done; the next publish finishes what it started".into()));

    // Refused at once by lib.rs: another publish runs. Nothing ran.
    let (said, _) = published(Err(Failure::Unusable(
        "adopted is being published: one publish at a time".into(),
    )));
    assert_eq!(
        said,
        Reply::Bad("adopted is being published: one publish at a time".into())
    );
}

#[test]
fn while_it_publishes_the_target_says_so() {
    let mut w = World::new();
    let (x, _) = planned(&w, rec("confluence-update", "json"));
    w.publishing = Some("design-doc-confluence".into());
    assert_eq!(
        of(&x, &w, "design-doc-confluence")[1],
        "  publishing… kiln reads the space again and publishes its own plan"
    );
}

#[test]
fn a_opens_kiln_auth_confluence_in_a_terminal_on_a_confluence_target() {
    let w = World::new();
    let (mut x, at) = planned(&w, rec("confluence-no-token", "json"));
    assert_eq!(
        press(&mut x, &w, 'A', &at),
        Reply::Terminal {
            title: "kiln auth confluence".into(),
            args: vec!["auth".into(), "confluence".into()],
        }
    );
    let file = row(&x, &w, "would create docs/DESIGN-DOCUMENT.md");
    assert_eq!(
        press(&mut x, &w, 'A', &file),
        Reply::Say(
            "A signs in: on a Confluence target (kiln auth confluence) or on jira (kiln auth jira)"
                .into()
        )
    );
}

// -- Never ------------------------------------------------------------------

/// Names a target may have, the hostile ones included.
const NAMES: [&str; 6] = [
    "design-doc",
    "spec",
    "--force",
    "--adopt",
    "-y",
    "--prune-removed=0",
];

/// Every argument list this module builds, for every name above.
fn every_args() -> Vec<Vec<String>> {
    let mut all = vec![
        dry_run_args(None),
        auth_args(),
        write_args(&NAMES),
        jira_dry_run_args(),
        sync_args(),
        crate::jira::auth_args(),
        crate::jira::args(),
        crate::pages::comments::args(),
    ];
    for n in NAMES {
        all.push(dry_run_args(Some(n)));
        all.push(write_args(&[n]));
        all.push(publish_args(n));
    }
    all
}

#[test]
fn no_argument_list_can_hold_force_adopt_prune_or_allow_shallow() {
    for args in every_args() {
        // A name comes after `--`, where kiln reads no option.
        let options = match args.iter().position(|a| a == "--") {
            Some(at) => &args[..at],
            None => &args[..],
        };
        for a in options {
            assert!(
                !FORBIDDEN
                    .iter()
                    .any(|f| a == f || a.starts_with(&format!("{f}="))),
                "{args:?}"
            );
            assert!(
                !NAMES[2..].contains(&a.as_str()),
                "a name as an option: {args:?}"
            );
        }
        // Only `export`, and these exactly: signing in, the Jira plan and
        // sync, the mirror and the comments. `--yes` only to publish, with
        // exactly one name, or in the one sync.
        let known: [&[&str]; 6] = [
            &["auth", "confluence"],
            &["auth", "jira"],
            &["jira", "sync", "--dry-run"],
            &["jira", "sync", "--yes"],
            &["list", "--jira", "--format", "json"],
            &["confluence", "comments", "--format", "json"],
        ];
        assert!(
            args[0] == "export" || known.iter().any(|k| args == *k),
            "{args:?}"
        );
        if args.iter().any(|a| a == "--yes") {
            if args[0] == "export" {
                assert_eq!(args.len(), 6, "{args:?}");
                assert_eq!(&args[..5], ["export", "--yes", "--format", "json", "--"]);
            } else {
                assert_eq!(args, ["jira", "sync", "--yes"]);
            }
        }
    }
    // And what the tab's keys and prompt reply with, through a whole flow.
    let w = World::new();
    let (mut x, at) = planned(&w, rec("confluence-update", "json"));
    let mut replies = Vec::new();
    for c in ['w', 'W', 'P', 'R', 'A'] {
        replies.push(press(&mut x, &w, c, &at));
    }
    replies.push(x.typed(CONFIRM, "design-doc-confluence", &w.ctx()));
    let ran: Vec<Vec<String>> = replies
        .iter()
        .chain(x.queries(&w.ctx()).iter().map(|_| &Reply::Ignored))
        .filter_map(|r| match r {
            Reply::Run { args, .. } | Reply::Terminal { args, .. } => Some(args.clone()),
            _ => None,
        })
        .chain(x.queries(&w.ctx()).into_iter().map(|q| q.args))
        .collect();
    assert!(!ran.is_empty());
    for args in ran {
        assert!(
            !args.iter().any(|a| FORBIDDEN.contains(&a.as_str())),
            "{args:?}"
        );
    }
}

#[test]
fn the_source_names_the_forbidden_flags_only_to_forbid_them() {
    // No string literal elsewhere in the module: the constant, and the
    // words of what's said, are the only places they appear as code.
    let src = include_str!("../exports.rs");
    for flag in FORBIDDEN {
        let quoted = format!("\"{flag}\"");
        let uses: Vec<&str> = src.lines().filter(|l| l.contains(&quoted)).collect();
        assert_eq!(
            uses,
            [
                r#"pub const FORBIDDEN: [&str; 4] = ["--force", "--adopt", "--prune-removed", "--allow-shallow"];"#
            ],
            "{flag}"
        );
    }
}

// -- When it's read ---------------------------------------------------------

#[test]
fn first_shown_one_dry_run_a_reload_none_r_one() {
    use crate::pages::frame::{Now, Plan};
    use crate::pages::Tab;
    use crate::project::State;
    let l = demo();
    let c = conf();
    let now = |loads: u64, changes: u64| Now {
        state: State::Loaded {
            loaded: &l,
            reloading: false,
            error: None,
        },
        loads,
        config: Ok(&c),
        settled: true,
        changes,
        publishing: None,
        tickets: None,
    };
    let mut p = Plan::new();
    assert!(p.wants(now(1, 1)).is_empty(), "not shown, not asked");
    p.show(Tab::Exports);
    let asks = p.wants(now(1, 1));
    assert_eq!(asks.len(), 5, "one per target");
    assert!(asks.iter().all(|a| a.limit_ms == Some(DRY_RUN_MS)));
    for (i, a) in asks.iter().enumerate() {
        p.asked(a, 10 + i as u64, now(1, 1));
    }
    assert!(p.wants(now(1, 1)).is_empty(), "out");
    for (i, a) in asks.iter().enumerate() {
        let name = a.tag.strip_prefix(DRY).unwrap();
        let r = if name == "design-doc" {
            rec("dry-run", "json")
        } else {
            rec("confluence-update", "json")
        };
        p.answered(Tab::Exports, &a.tag, 10 + i as u64, &r);
    }
    assert!(p.wants(now(2, 1)).is_empty(), "a reload asks nothing");
    assert!(p.wants(now(3, 2)).is_empty(), "a change neither: R does");
    let r = p.key(KeyPress::from(Key::Char('R')), now(3, 2));
    assert!(r.is_empty());
    assert_eq!(p.wants(now(3, 2)).len(), 5, "R: once more");
}

// -- Jira -----------------------------------------------------------------------

/// Every target's dry run answered as the demo's, and the Jira one with
/// `plan`, all at the world's change count.
fn with_jira(w: &World, plan: Result<Ran, Failure>) -> Exports {
    let mut x = Exports::default();
    for q in x.queries(&w.ctx()) {
        let result = if q.tag == JIRA_DRY {
            plan.clone()
        } else {
            rec("dry-run", "json")
        };
        x.answer_at(&q.tag, &result, w.changes);
    }
    x
}

fn jira(name: &str) -> Result<Ran, Failure> {
    hand(name, "out")
}

/// The first row the cursor stops at under the jira target.
fn jira_row(x: &Exports, w: &World) -> Row {
    x.rows(&w.ctx())
        .into_iter()
        .find(|r| r.selectable() && r.key().starts_with(JIRA_SYNC))
        .unwrap_or_else(|| panic!("no row under jira"))
}

/// The jira target's rows alone.
fn jira_of(x: &Exports, w: &World) -> Vec<String> {
    of(x, w, "jira")
}

#[test]
fn with_jira_project_the_jira_dry_run_is_one_more_target_after_the_export_targets() {
    let w = World::with_jira();
    let x = Exports::default();
    let qs = x.queries(&w.ctx());
    let tags: Vec<&str> = qs.iter().map(|q| q.tag.as_str()).collect();
    assert_eq!(tags.last(), Some(&"jira-dry-run"));
    assert_eq!(tags.len(), 6, "the five export targets and jira: {tags:?}");
    let q = qs.last().unwrap();
    assert_eq!(
        q.args,
        ["jira", "sync", "--dry-run"],
        "no --format: it has none"
    );
    assert_eq!(q.round, Some(0));
    assert_eq!(q.limit_ms, Some(JIRA_DRY_MS));
    assert!(q.settled, "asked of the project as a read leaves it");
    // Without [jira] project, no target and no query; and a url alone
    // mirrors tickets but has nothing to sync.
    for toml in [
        fixture("exports/kiln.toml"),
        format!(
            "{}\n[jira]\nurl = \"http://127.0.0.1:9\"\n",
            fixture("exports/kiln.toml")
        ),
    ] {
        let mut w = World::new();
        w.config = Ok(config::parse(&toml).unwrap());
        assert!(!Exports::default()
            .queries(&w.ctx())
            .iter()
            .any(|q| q.tag == "jira-dry-run"));
        assert!(!lines(&answered(&w, &[]), &w)
            .iter()
            .any(|l| l.starts_with("# jira")));
    }
}

#[test]
fn the_jira_target_is_last_with_kilns_heading_lines_and_totals_as_it_prints_them() {
    let w = World::with_jira();
    let x = with_jira(&w, jira("jira-sync-dry-run"));
    let all = lines(&x, &w);
    assert!(
        all.last()
            .unwrap()
            .starts_with("  1 create, 1 update, 1 move"),
        "last: {all:?}"
    );
    let printed: Vec<String> = fixture("../hand/jira-sync-dry-run.out")
        .lines()
        .map(String::from)
        .collect();
    let rows = jira_of(&x, &w);
    // Line for line as kiln printed it: its heading, then its lines,
    // indented as a row is (kiln's two spaces are the row's depth).
    assert_eq!(rows[0], format!("# {}", printed[0]));
    assert_eq!(
        rows[1..],
        printed[1..]
            .iter()
            .map(|l| format!("  {}", l.trim_start()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        rows[0], "# jira (DEMO) → DEMO-100 \"Kiln: Demo\"",
        "the heading kiln prints"
    );
}

#[test]
fn each_line_has_the_look_of_what_it_says() {
    let w = World::with_jira();
    let x = with_jira(&w, jira("jira-sync-dry-run-refused"));
    let looks: Vec<(String, Look)> = x
        .rows(&w.ctx())
        .into_iter()
        .skip_while(|r| r.kind != Kind::Heading || !r.cells[0].text.starts_with("jira"))
        .skip(1)
        .filter(|r| r.selectable())
        .map(|r| (r.cells[0].text.clone(), r.cells[0].look.clone()))
        .collect();
    let look = |starts: &str| {
        looks
            .iter()
            .find(|(t, _)| t.starts_with(starts))
            .unwrap_or_else(|| panic!("{starts} in {looks:?}"))
            .1
            .clone()
    };
    assert_eq!(look("create "), Look::Good);
    assert_eq!(look("move "), Look::Accent);
    assert_eq!(look("refused "), Look::Bad, "a refusal is in the bad look");
    assert_eq!(look("1 create"), Look::Text, "the totals");
    let x = with_jira(&w, jira("jira-sync-dry-run"));
    let rows = x.rows(&w.ctx());
    let left = rows
        .iter()
        .find(|r| r.cells[0].text.starts_with("orphan"))
        .unwrap();
    assert_eq!(left.cells[0].look, Look::Warn, "reported, never written");
}

#[test]
fn a_plan_level_refusal_and_every_other_shape_kiln_prints() {
    let w = World::with_jira();
    let x = with_jira(&w, jira("jira-sync-dry-run-plan-refused"));
    let rows = jira_of(&x, &w);
    assert!(rows.contains(&"  refused: Sub-task needs fields Kiln doesn't set (Component/s): give them defaults in Jira, or create the tickets by hand".to_string()), "{rows:#?}");
    assert!(rows
        .iter()
        .any(|l| l.contains("leaves a refused ticket alone and syncs the rest")));
    let x = with_jira(&w, jira("jira-sync-dry-run-first"));
    assert_eq!(
        jira_of(&x, &w)[0],
        "# jira (DEMO) → a new project issue (Task)"
    );
    let x = with_jira(&w, jira("jira-sync-dry-run-nothing"));
    assert_eq!(
        jira_of(&x, &w),
        [
            "# jira (DEMO) → DEMO-100 \"Kiln: Demo\"",
            "  0 create, 0 update, 0 move, 3 unchanged"
        ]
    );
}

#[test]
fn what_isnt_the_shape_is_shown_as_it_is_and_never_as_a_wrong_count() {
    let w = World::with_jira();
    // A kiln that words it otherwise: lines as they are, no write counted.
    let odd = Ok(Ran {
        code: Some(0),
        stdout: "jira (DEMO) → DEMO-100\n  Create TASK-1 now\n  3 tickets to create\n\u{1b}[31m  update\u{7} DEMO-1 x\n".into(),
        stderr: vec![],
    });
    let mut x = with_jira(&w, odd);
    let rows = jira_of(&x, &w);
    assert_eq!(rows[1], "  Create TASK-1 now");
    assert_eq!(rows[2], "  3 tickets to create");
    assert!(
        rows.iter()
            .all(|l| !l.contains('\u{1b}') && !l.contains('\u{7}')),
        "{rows:?}"
    );
    // `P` counts only lines that start create, update or move, as kiln
    // 1.0.0 writes them: none of these does, so nothing is claimed.
    let at = row(&x, &w, "Create");
    assert_eq!(
        press(&mut x, &w, 'P', &at),
        Reply::Say("nothing to sync: jira's plan is empty".into())
    );
}

#[test]
fn kilns_line_is_shown_under_the_heading_and_no_token_says_how_to_sign_in() {
    let w = World::with_jira();
    for (name, recorded_answer, token) in [
        ("no token here", recorded_jira("sync-no-token"), true),
        ("unreachable", recorded_jira("sync-unreachable"), false),
        (
            "no token, a desktop",
            hand("jira-sync-no-token", "out"),
            true,
        ),
        ("not configured", recorded_jira("sync-unconfigured"), false),
    ] {
        let line = recorded_answer.clone().unwrap().error_line();
        let x = with_jira(&w, recorded_answer);
        let rows = jira_of(&x, &w);
        assert_eq!(rows[0], "# jira (DEMO)", "{name}: no plan, no kiln heading");
        assert_eq!(rows[1], format!("  {line}"), "{name}");
        assert_eq!(rows.len(), if token { 3 } else { 2 }, "{name}: {rows:?}");
        if token {
            assert!(rows[2].contains("kiln auth jira in a terminal"));
        }
    }
}

fn recorded_jira(name: &str) -> Result<Ran, Failure> {
    answer(&format!("jira/{name}"), "out")
}

#[test]
fn being_read_and_out_of_date_are_said_under_jira_too() {
    let mut w = World::with_jira();
    let x = Exports::default();
    assert_eq!(
        jira_of(&x, &w),
        ["# jira (DEMO)", "  reading… (asks Jira)"],
        "not asked yet"
    );
    let x = with_jira(&w, jira("jira-sync-dry-run"));
    w.changes += 1;
    assert_eq!(
        jira_of(&x, &w)[1],
        "  the project changed since this was read: R reads it again"
    );
}

#[test]
fn r_reads_jira_again_with_everything_else() {
    let mut w = World::with_jira();
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    let at = jira_row(&x, &w);
    assert_eq!(press(&mut x, &w, 'R', &at), Reply::Rebuild);
    let qs = x.queries(&w.ctx());
    assert_eq!(qs.last().unwrap().round, Some(1));
    assert_eq!(jira_of(&x, &w)[1], "  reading… (asks Jira)");
    w.reading = true;
    assert!(matches!(press(&mut x, &w, 'R', &at), Reply::Say(_)));
}

const SYNC_LABEL: &str = "Sync Jira: creates, updates and moves tickets, jira (DEMO) → DEMO-100 \"Kiln: Demo\": 3 operations (1 create, 1 update, 1 move, 2 unchanged). Type DEMO to sync";

/// The sync Reply a right key gives.
fn sync_run() -> Reply {
    Reply::Run {
        tag: "sync-jira-run".into(),
        args: ["jira", "sync", "--yes"].map(String::from).to_vec(),
        limit_ms: Some(PUBLISH_MS),
        publish: Some(JIRA_SYNC.into()),
    }
}

#[test]
fn the_sync_flow_as_a_table() {
    let ask = Reply::ask("sync-jira", SYNC_LABEL, "");
    type Case = (
        &'static str,
        &'static str,
        fn(&mut World),
        Option<&'static str>,
        Reply,
    );
    let none: fn(&mut World) = |_| {};
    let cases: Vec<Case> = vec![
        ("the project key", "jira-sync-dry-run", none, Some("DEMO"), sync_run()),
        (
            "another case",
            "jira-sync-dry-run",
            none,
            Some("demo"),
            Reply::Say("“demo” isn't DEMO: nothing was synced".into()),
        ),
        (
            "a space after",
            "jira-sync-dry-run",
            none,
            Some("DEMO "),
            Reply::Say("“DEMO ” isn't DEMO: nothing was synced".into()),
        ),
        (
            "yes",
            "jira-sync-dry-run",
            none,
            Some("yes"),
            Reply::Say("“yes” isn't DEMO: nothing was synced".into()),
        ),
        (
            "the export target's name",
            "jira-sync-dry-run",
            none,
            Some("design-doc-confluence"),
            Reply::Say("“design-doc-confluence” isn't DEMO: nothing was synced".into()),
        ),
        ("cancelled (Esc): nothing comes back", "jira-sync-dry-run", none, None, ask.clone()),
        (
            "a stale dry run",
            "jira-sync-dry-run",
            |w| w.changes += 1,
            None,
            Reply::Say("the project changed since jira's plan was read: R reads it again, then P".into()),
        ),
        (
            "nothing to do",
            "jira-sync-dry-run-nothing",
            none,
            None,
            Reply::Say("nothing to sync: jira's plan is 0 create, 0 update, 0 move, 3 unchanged".into()),
        ),
        (
            "a refusal is no reason not to: kiln syncs the rest",
            "jira-sync-dry-run-refused",
            none,
            None,
            Reply::ask(
                "sync-jira",
                "Sync Jira: creates, updates and moves tickets, jira (DEMO) → DEMO-100 \"Kiln: Demo\": 2 operations (1 create, 0 update, 1 move, 1 unchanged, 1 refused); kiln leaves the 1 ticket it refuses alone and syncs the rest. Type DEMO to sync",
                "",
            ),
        ),
        (
            "a Confluence publish running",
            "jira-sync-dry-run",
            |w| w.publishing = Some("adopted".into()),
            None,
            Reply::Say("adopted is being published: one publish at a time".into()),
        ),
        (
            "a sync running",
            "jira-sync-dry-run",
            |w| w.publishing = Some(JIRA_SYNC.into()),
            None,
            Reply::Say("the Jira sync is running: one publish at a time".into()),
        ),
    ];
    for (what, plan, before, typed, expect) in cases {
        let mut w = World::with_jira();
        let mut x = with_jira(&w, jira(plan));
        let at = jira_row(&x, &w);
        before(&mut w);
        let reply = press(&mut x, &w, 'P', &at);
        let reply = match (typed, &reply) {
            (Some(text), Reply::Ask { tag, label, .. }) => {
                assert_eq!(label, SYNC_LABEL, "{what}");
                x.typed(tag, text, &w.ctx())
            }
            _ => reply,
        };
        assert_eq!(reply, expect, "{what}");
    }
}

#[test]
fn p_on_jira_with_no_plan_says_why() {
    let w = World::with_jira();
    // Not read yet.
    let mut x = Exports::default();
    let at = Row::entry(format!("{JIRA_SYNC}\u{1}x"), vec![]);
    assert_eq!(
        press(&mut x, &w, 'P', &at),
        Reply::Say("the Jira plan is still being read: P once it's shown".into())
    );
    // Read again.
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    x.round += 1;
    assert_eq!(
        press(&mut x, &w, 'P', &at),
        Reply::Say("the Jira plan is being read again: P once it's shown".into())
    );
    // No token, and the others kiln says.
    let mut x = with_jira(&w, recorded_jira("sync-no-token"));
    let reply = press(&mut x, &w, 'P', &at);
    assert!(
        matches!(&reply, Reply::Bad(l) if l.starts_with("jira has no plan: kiln: couldn't read") && l.ends_with("-- A runs kiln auth jira in a terminal, then R")),
        "{reply:?}"
    );
    let mut x = with_jira(&w, recorded_jira("sync-unreachable"));
    assert!(
        matches!(press(&mut x, &w, 'P', &at), Reply::Bad(l) if l.starts_with("jira has no plan: kiln: Jira (http://127.0.0.1:9)"))
    );
    // And no [jira] project: nothing to sync.
    let w = World::new();
    let mut x = answered(&w, &[]);
    assert_eq!(
        press(&mut x, &w, 'P', &at),
        Reply::Say("kiln.toml's [jira] has no project: there's nothing to sync".into())
    );
}

#[test]
fn a_plan_read_again_while_the_key_is_typed_syncs_nothing() {
    let mut w = World::with_jira();
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    let at = jira_row(&x, &w);
    let Reply::Ask { tag, .. } = press(&mut x, &w, 'P', &at) else {
        panic!()
    };
    w.changes += 1;
    let stale = Reply::Say("jira's plan isn't the one shown any more (the project or the plan was read again): nothing was synced; R, then P".into());
    assert_eq!(x.typed(&tag, "DEMO", &w.ctx()), stale);
    assert_eq!(
        x.typed(&tag, "DEMO", &w.ctx()),
        Reply::Ignored,
        "no second P, no answer"
    );
    // R while it's typed: the round moved.
    let w = World::with_jira();
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    let at = jira_row(&x, &w);
    let Reply::Ask { tag, .. } = press(&mut x, &w, 'P', &at) else {
        panic!()
    };
    x.round += 1;
    assert_eq!(x.typed(&tag, "DEMO", &w.ctx()), stale);
}

/// What a sync said, given kiln's answer, after the plan was confirmed.
fn synced(answer: Result<Ran, Failure>) -> (Reply, Vec<String>, u64) {
    let w = World::with_jira();
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    let at = jira_row(&x, &w);
    let Reply::Ask { tag, .. } = press(&mut x, &w, 'P', &at) else {
        panic!()
    };
    assert_eq!(x.typed(&tag, "DEMO", &w.ctx()), sync_run());
    let said = x.ran("sync-jira-run", &answer, &w.ctx());
    (said, jira_of(&x, &w), x.jira_again)
}

#[test]
fn a_sync_shows_kilns_own_outcome_and_reads_the_project_and_the_plan_again() {
    let keys = "jira: 1 created, 1 updated, 1 moved; 1 file gained ticket keys (commit them)";
    let (said, rows, again) = synced(hand("jira-sync-applied", "out"));
    // Prominent: the files it names are changed and uncommitted.
    assert_eq!(
        said,
        Reply::Reload {
            say: keys.into(),
            bad: false
        }
    );
    assert_eq!(rows[1], format!("  {keys}"), "kept under the target");
    assert_eq!(again, 1, "the plan read again");
    assert_eq!(rows[2], "  reading… (asks Jira)");
    // The outcome row is in the warning look.
    let w = World::with_jira();
    let mut x = with_jira(&w, jira("jira-sync-dry-run"));
    x.ran("sync-jira-run", &hand("jira-sync-applied", "out"), &w.ctx());
    let outcome = x
        .rows(&w.ctx())
        .into_iter()
        .find(|r| r.cells[0].text == keys)
        .unwrap();
    assert_eq!(outcome.cells[0].look, Look::Warn);

    // No keys written: nothing to commit, so no warning.
    let (said, _, _) = synced(hand("jira-sync-applied-no-keys", "out"));
    assert_eq!(
        said,
        Reply::Reload {
            say: "jira: 0 created, 0 updated, 1 moved".into(),
            bad: false
        }
    );

    // Some refused, the rest synced: exit 1, kiln's line after the summary.
    let (said, _, _) = synced(hand("jira-sync-applied-refused", "out"));
    assert_eq!(
        said,
        Reply::Reload {
            say: "jira: 1 created, 0 updated, 1 moved; 1 file gained ticket keys (commit them) -- kiln refused some of it (exit 1): the plan shows what".into(),
            bad: true
        }
    );

    // Jira failed part way: the keys are in the files even so.
    let (said, _, _) = synced(hand("jira-sync-partial", "out"));
    assert_eq!(
        said,
        Reply::Reload {
            say: "jira: 1 created, 0 updated, 0 moved; 1 file gained ticket keys (commit them) -- kiln: Jira (http://127.0.0.1:9): creating TASK-005: request failed: http://127.0.0.1:9/rest/api/2/issue: Connection Failed: Connect error: Connection refused (os error 111); run it again to finish -- the keys of the tickets it created are in your files even so: commit them, so none is created twice, then sync again to finish".into(),
            bad: true
        }
    );

    // The keys that couldn't be written, said by kiln.
    let (said, _, _) = synced(hand("jira-sync-write-error", "out"));
    assert!(
        matches!(&said, Reply::Reload { say, bad: true } if say.starts_with("jira: 1 created, 0 updated, 0 moved -- kiln: record `jira = \"DEMO-110\"` in TASK-004 by hand")),
        "{said:?}"
    );

    // No token (a desktop, and as recorded), and Jira unreachable before
    // anything was created: kiln's line, no claim about keys.
    let (said, _, _) = synced(hand("jira-sync-no-token", "out"));
    assert_eq!(said, Reply::Reload { say: "kiln: no Jira token for http://127.0.0.1:9: set KILN_JIRA_TOKEN or run `kiln auth jira` -- A runs kiln auth jira in a terminal".into(), bad: true });
    let (said, _, _) = synced(recorded_jira("sync-unreachable"));
    assert!(
        matches!(&said, Reply::Reload { say, bad: true } if say.starts_with("kiln: Jira (http://127.0.0.1:9): the issue types of project DEMO") && !say.contains("commit")),
        "{said:?}"
    );

    // Nothing to do as it synced.
    let (said, _, _) = synced(jira("jira-sync-dry-run-nothing"));
    assert_eq!(
        said,
        Reply::Reload {
            say: "nothing synced: kiln's plan, read as it synced, had nothing to do".into(),
            bad: false
        }
    );

    // Stopped by the watchdog: the keys weren't written, and that's said.
    let (said, _, _) = synced(Err(Failure::TimedOut(PUBLISH_MS / 1000)));
    assert!(
        matches!(&said, Reply::Reload { say, bad: true } if say.contains("stopped before it wrote the keys") && say.contains("orphans")),
        "{said:?}"
    );

    // Refused at once by lib.rs: another publish runs. Nothing ran, so
    // nothing is read again.
    let (said, rows, again) = synced(Err(Failure::Unusable(
        "design-doc-confluence is being published: one publish at a time".into(),
    )));
    assert_eq!(
        said,
        Reply::Bad("design-doc-confluence is being published: one publish at a time".into())
    );
    assert_eq!(again, 0);
    assert!(!rows.iter().any(|l| l.contains("published")));
}

#[test]
fn while_it_syncs_the_target_says_so() {
    let mut w = World::with_jira();
    let x = with_jira(&w, jira("jira-sync-dry-run"));
    w.publishing = Some(JIRA_SYNC.into());
    assert_eq!(
        jira_of(&x, &w)[1],
        "  syncing… kiln plans again and applies it; the keys of new tickets are written into the items"
    );
    assert_eq!(
        publishing_line(JIRA_SYNC),
        "syncing Jira… (kiln writes ticket keys into the items as it goes)"
    );
    assert_eq!(
        publishing_line("spec"),
        "publishing spec to Confluence…",
        "a Confluence target's is as it was"
    );
}

#[test]
fn a_opens_kiln_auth_jira_and_w_writes_no_file_for_jira() {
    let w = World::with_jira();
    let mut x = with_jira(&w, recorded_jira("sync-no-token"));
    let at = row(&x, &w, "kiln: couldn't read `jira:");
    assert_eq!(
        press(&mut x, &w, 'A', &at),
        Reply::Terminal {
            title: "kiln auth jira".into(),
            args: vec!["auth".into(), "jira".into()],
        }
    );
    assert_eq!(
        press(&mut x, &w, 'w', &at),
        Reply::Say("jira isn't a file target: P syncs it".into()),
        "never `kiln export -- jira`"
    );
    // W names the file targets and never jira.
    match press(&mut x, &w, 'W', &at) {
        Reply::Run { args, publish, .. } => {
            assert_eq!(
                args,
                [
                    "export",
                    "--format",
                    "json",
                    "--",
                    "design-doc",
                    "requirements-html",
                    "sections-md"
                ]
            );
            assert_eq!(publish, None);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_a_confirmed_sync_runs_kiln_jira_sync_with_yes() {
    // The source: `--yes` is built in two places only, the confluence
    // publish's and the sync's, each after the target's name or the
    // project key is typed.
    let code: String = include_str!("../exports.rs")
        .lines()
        .take_while(|l| !l.starts_with("#[cfg(test)]"))
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(code.matches("\"--yes\"").count(), 2);
    // Nothing that is run without a confirmation holds it: the dry run
    // and the sign-in.
    assert!(!jira_dry_run_args().contains(&"--yes".to_string()));
    assert!(!crate::jira::auth_args().contains(&"--yes".to_string()));
    assert_eq!(
        sync_args(),
        ["jira", "sync", "--yes"],
        "exactly this, never anything else"
    );
    // And none builds anything for the sync but sync_args.
    assert_eq!(code.matches("args: sync_args(),").count(), 1);
}

mod page {
    use super::*;
    use crate::tests::{demo, folder, foot, harness, spawned};
    use crate::Kiln;
    use fenix_test_kit::{assert_snapshot, Fake, Harness, Ran as Run};

    /// The exports demo: the demo's kiln answers, the five targets'
    /// kiln.toml at the project's top, and the dry runs as recorded.
    fn fake(test: &str, confluence: &str, adopted: &str) -> (Fake, std::path::PathBuf) {
        let fake = demo();
        // A folder of its own: tests run at once.
        let dir = folder(&format!("exports-{test}"));
        std::fs::write(dir.join("kiln.toml"), fixture("exports/kiln.toml")).unwrap();
        let dry = |name: &str| format!("kiln export --dry-run --format json -- {name}");
        let run = |r: Result<Ran, Failure>| {
            let r = r.unwrap();
            let mut run = Run::exits(r.code.unwrap_or(0)).stdout(r.stdout);
            if !r.stderr.is_empty() {
                run = run.stderr(r.stderr.join("\n"));
            }
            run
        };
        fake.on_spawn(&dry("design-doc"), run(rec("dry-run-update", "json")))
            .on_spawn(
                &dry("requirements-html"),
                run(rec("dry-run-left-out", "json")),
            )
            .on_spawn(&dry("sections-md"), run(rec("dry-run-remove", "json")))
            .on_spawn(&dry("design-doc-confluence"), run(rec(confluence, "json")))
            .on_spawn(&dry("adopted"), run(rec(adopted, "json")));
        (fake, dir)
    }

    fn open(fake: &Fake, dir: &std::path::Path) -> (Harness<Kiln>, u64) {
        let mut h = harness(fake, dir);
        h.size = (110, 26);
        h.command("kiln.open").unwrap();
        let page = h.page("plan");
        h.keys(page, "0");
        (h, page)
    }

    fn dry_runs(fake: &Fake) -> usize {
        spawned(fake)
            .iter()
            .filter(|l| l.starts_with("/usr/bin/kiln export --dry-run"))
            .count()
    }

    #[test]
    fn the_file_targets_and_the_confluence_plans() {
        let (fake, dir) = fake("plans", "confluence-update", "confluence-adopt");
        let (mut h, page) = open(&fake, &dir);
        assert_snapshot!("exports-demo", h.screen(page));
        // Shorter: the Confluence plans, the cursor on the last row.
        h.size = (110, 16);
        h.keys(page, "G");
        assert_snapshot!("exports-demo-confluence", h.screen(page));
    }

    #[test]
    fn no_token_shows_kilns_line_under_its_target_and_a_opens_a_terminal() {
        let (fake, dir) = fake("no-token", "confluence-no-token", "confluence-adopt");
        let (mut h, page) = open(&fake, &dir);
        assert_snapshot!("exports-demo-no-token", h.screen(page));
        // The cursor to the no-token target's line, then A.
        h.keys(page, "/credential<Enter>");
        fake.take();
        h.keys(page, "A");
        let calls = fake.take();
        assert!(
            calls
                .iter()
                .any(|c| c.ends_with("kiln auth confluence (below): /usr/bin/kiln auth confluence")),
            "{calls:?}"
        );
        // Never a secret, a secret prompt, or the environment.
        assert!(!fake
            .calls()
            .iter()
            .any(|c| c.contains("secret") || c.starts_with("env")));
    }

    #[test]
    fn shown_once_a_reload_asks_nothing_r_asks_again() {
        let (fake, dir) = fake("once", "confluence-update", "confluence-adopt");
        let (mut h, page) = open(&fake, &dir);
        assert_eq!(dry_runs(&fake), 5);
        h.keys(page, "r");
        h.settle();
        h.saved("design/tasks/TASK-001.md");
        h.settle();
        assert_eq!(
            dry_runs(&fake),
            0,
            "the project read again, not the exports"
        );
        assert!(
            foot(&mut h, page).starts_with("R read again"),
            "{}",
            foot(&mut h, page)
        );
        h.keys(page, "R");
        assert_eq!(dry_runs(&fake), 5);
    }

    #[test]
    fn w_writes_one_target_reads_it_again_and_reads_no_project() {
        let (fake, dir) = fake("write", "confluence-update", "confluence-adopt");
        let (mut h, page) = open(&fake, &dir);
        fake.on_spawn(
            "kiln export --format json -- design-doc",
            Run::ok(fixture("exports/write.json")),
        );
        fake.take();
        // The first row the cursor stops on: design-doc's file.
        h.keys(page, "w");
        let lines = spawned(&fake);
        assert_eq!(
            lines,
            [
                "/usr/bin/kiln export --format json -- design-doc",
                "/usr/bin/kiln export --dry-run --format json -- design-doc"
            ],
            "the write, its dry run again, and no reload"
        );
        assert_eq!(
            foot(&mut h, page),
            "design-doc: created docs/DESIGN-DOCUMENT.md"
        );
    }

    /// The demo with design-doc-confluence's plan to publish, on Exports,
    /// the cursor on its plan.
    fn ready(test: &str) -> (Fake, Harness<Kiln>, u64) {
        let (fake, dir) = fake(test, "confluence-update", "confluence-adopt");
        let (mut h, page) = open(&fake, &dir);
        h.keys(page, "/demo space home<Enter>");
        h.keys(page, "/<Esc>");
        (fake, h, page)
    }

    #[test]
    fn p_asks_for_the_name_then_runs_exactly_one_publish() {
        let (fake, mut h, page) = ready("publish");
        fake.on_prompt(
            "Type design-doc-confluence to publish",
            "design-doc-confluence",
        )
        .on_spawn("kiln export --yes", Run::running(""));
        fake.take();
        h.keys(page, "P");
        let calls = fake.take();
        let prompt = calls
            .iter()
            .find(|c| c.starts_with("prompt "))
            .expect("Fenix's prompt");
        assert!(prompt.ends_with(&format!("{LABEL} []")), "{prompt}");
        let ran: Vec<&String> = calls.iter().filter(|c| c.starts_with("spawn ")).collect();
        assert_eq!(ran.len(), 1, "{calls:?}");
        assert!(
            ran[0].ends_with(" /usr/bin/kiln export --yes --format json -- design-doc-confluence"),
            "{ran:?}"
        );
        assert!(
            fake.timers().iter().any(|t| t.millis == PUBLISH_MS),
            "the publish's own limit"
        );
        // While it runs: every tab says so, and a second P publishes nothing.
        assert!(foot(&mut h, page).starts_with("publishing design-doc-confluence to Confluence…"));
        assert_snapshot!("exports-demo-publishing", h.screen(page));
        h.keys(page, "1");
        assert!(foot(&mut h, page).starts_with("publishing design-doc-confluence to Confluence…"));
        h.keys(page, "0");
        fake.take();
        h.keys(page, "P");
        assert_eq!(
            foot(&mut h, page),
            "design-doc-confluence is being published: one publish at a time"
        );
        assert!(spawned(&fake).is_empty() && !fake.take().iter().any(|c| c.starts_with("prompt")));
        // And past the tab (another page's, say): lib.rs runs no second.
        h.ext.perform(
            page,
            vec![crate::pages::frame::Effect::Run {
                tab: crate::pages::Tab::Exports,
                tag: format!("{PUBLISH}adopted"),
                args: publish_args("adopted"),
                limit_ms: Some(PUBLISH_MS),
                publish: Some("adopted".into()),
            }],
        );
        h.ext.pass_on();
        assert!(spawned(&fake).is_empty(), "no second publish");
        assert_eq!(
            foot(&mut h, page),
            "design-doc-confluence is being published: one publish at a time"
        );
        // kiln's answer: what it applied, said, the dry run read again.
        let process = fake.process("kiln export --yes").unwrap().id;
        for line in fixture("../hand/export-publish-applied.json").lines() {
            h.said(process, line, false);
        }
        h.exited(process, Some(0));
        h.settle();
        assert_eq!(
            foot(&mut h, page),
            "design-doc-confluence: published 0 created, 1 updated, 1 retired, 0 pruned"
        );
        assert_eq!(
            spawned(&fake),
            ["/usr/bin/kiln export --dry-run --format json -- design-doc-confluence"]
        );
        assert!(h.ext.publishing.is_none());
        let all: Vec<String> = fake.calls();
        for flag in FORBIDDEN {
            assert!(!all.iter().any(|c| c.contains(flag)), "{flag}");
        }
    }

    #[test]
    fn a_wrong_name_publishes_nothing_and_says_so() {
        let (fake, mut h, page) = ready("wrong-name");
        fake.on_prompt("Type design-doc-confluence to publish", "yes");
        fake.take();
        h.keys(page, "P");
        assert!(spawned(&fake).is_empty());
        assert_eq!(
            foot(&mut h, page),
            "“yes” isn't design-doc-confluence: nothing was published"
        );
        // Esc: nothing comes back, nothing runs.
        fake.on_prompt_nothing("Type design-doc-confluence to publish");
        h.keys(page, "P");
        assert!(spawned(&fake).is_empty());
    }

    #[test]
    fn a_publish_outlives_another_kiln_and_says_what_it_did() {
        let (fake, mut h, page) = ready("outlives");
        fake.on_prompt(
            "Type design-doc-confluence to publish",
            "design-doc-confluence",
        )
        .on_spawn("kiln export --yes", Run::running(""));
        h.keys(page, "P");
        let process = fake.process("kiln export --yes").unwrap();
        h.set_setting("kiln.path", Some("/opt/kiln"));
        assert!(
            !fake.process("kiln export --yes").unwrap().killed,
            "not stopped"
        );
        assert!(h.ext.publishing.is_some(), "still running: one at a time");
        for line in fixture("../hand/export-publish-applied.json").lines() {
            h.said(process.id, line, false);
        }
        h.exited(process.id, Some(0));
        h.settle();
        assert!(h.ext.publishing.is_none(), "its answer frees the next");
        assert_eq!(
            foot(&mut h, page),
            "design-doc-confluence: published 0 created, 1 updated, 1 retired, 0 pruned",
            "said, though the project is being read by the other kiln"
        );
    }

    #[test]
    fn a_publish_outlives_its_page() {
        let (fake, mut h, page) = ready("closed");
        fake.on_prompt(
            "Type design-doc-confluence to publish",
            "design-doc-confluence",
        )
        .on_spawn("kiln export --yes", Run::running(""));
        h.keys(page, "P");
        let process = fake.process("kiln export --yes").unwrap();
        h.close(page);
        assert!(!fake.process("kiln export --yes").unwrap().killed);
        assert!(h.ext.publishing.is_some());
        h.exited(process.id, Some(3));
        h.settle();
        assert!(h.ext.publishing.is_none());
    }

    // -- Jira, on a page --------------------------------------------------------------

    /// The exports demo with `[jira]` (a url and a project) and `jira sync
    /// --dry-run` answering `plan`.
    fn jira_fake(test: &str, plan: Run) -> (Fake, std::path::PathBuf) {
        let (fake, dir) = fake(test, "confluence-update", "confluence-adopt");
        std::fs::write(dir.join("kiln.toml"), jira_toml()).unwrap();
        fake.on_spawn("kiln jira sync --dry-run", plan).on_spawn(
            "kiln list --jira --format json",
            Run::ok(fixture("jira/list.json")),
        );
        (fake, dir)
    }

    fn run_hand(name: &str) -> Run {
        let r = hand(name, "out").unwrap();
        let mut run = Run::exits(r.code.unwrap_or(0)).stdout(r.stdout);
        if !r.stderr.is_empty() {
            run = run.stderr(r.stderr.join("\n"));
        }
        run
    }

    fn jira_dry_runs(fake: &Fake) -> usize {
        spawned(fake)
            .iter()
            .filter(|l| l.ends_with("kiln jira sync --dry-run"))
            .count()
    }

    #[test]
    fn the_jira_target_after_the_export_targets() {
        let (fake, dir) = jira_fake("jira-rows", run_hand("jira-sync-dry-run"));
        let (mut h, page) = open(&fake, &dir);
        assert_eq!(jira_dry_runs(&fake), 1);
        h.size = (110, 34);
        h.keys(page, "G");
        assert_snapshot!("exports-demo-jira", h.screen(page));
    }

    #[test]
    fn no_token_shows_kilns_line_under_jira_and_a_opens_kiln_auth_jira() {
        let (fake, dir) = jira_fake("jira-no-token", run_hand("jira-sync-no-token"));
        let (mut h, page) = open(&fake, &dir);
        h.size = (110, 34);
        h.keys(page, "G");
        assert_snapshot!("exports-demo-jira-no-token", h.screen(page));
        fake.take();
        h.keys(page, "A");
        assert!(
            fake.take()
                .iter()
                .any(|c| c.ends_with("kiln auth jira (below): /usr/bin/kiln auth jira")),
            "{:?}",
            fake.calls()
        );
    }

    /// The jira plan, the cursor on its first line.
    fn jira_ready(test: &str) -> (Fake, Harness<Kiln>, u64) {
        let (fake, dir) = jira_fake(test, run_hand("jira-sync-dry-run"));
        let (mut h, page) = open(&fake, &dir);
        h.keys(page, "G");
        h.keys(page, "/create     TASK-004<Enter>");
        h.keys(page, "/<Esc>");
        (fake, h, page)
    }

    #[test]
    fn p_asks_for_the_key_then_runs_exactly_kiln_jira_sync_yes_then_reads_it_all_again() {
        let (fake, mut h, page) = jira_ready("jira-sync");
        fake.on_prompt(
            "Sync Jira: creates, updates and moves tickets, jira (DEMO) → DEMO-100 \"Kiln: Demo\": 3 operations (1 create, 1 update, 1 move, 2 unchanged). Type DEMO to sync",
            "DEMO",
        )
        .on_spawn("kiln jira sync --yes", Run::running(""));
        fake.take();
        h.keys(page, "P");
        let calls = fake.take();
        let ran: Vec<&String> = calls.iter().filter(|c| c.starts_with("spawn ")).collect();
        assert_eq!(ran.len(), 1, "{calls:?}");
        assert!(
            ran[0].ends_with(" /usr/bin/kiln jira sync --yes"),
            "{ran:?}"
        );
        assert!(
            fake.timers().iter().any(|t| t.millis == PUBLISH_MS),
            "the long limit: stopping it would lose the keys"
        );
        // While it runs, every tab says so, and a second publish or sync
        // is refused for either.
        assert!(
            foot(&mut h, page).starts_with("syncing Jira…"),
            "{}",
            foot(&mut h, page)
        );
        h.keys(page, "1");
        assert!(foot(&mut h, page).starts_with("syncing Jira…"));
        h.keys(page, "0");
        fake.take();
        h.keys(page, "P");
        assert_eq!(
            foot(&mut h, page),
            "the Jira sync is running: one publish at a time"
        );
        h.keys(page, "/old section<Enter>");
        h.keys(page, "P");
        assert_eq!(
            foot(&mut h, page).rsplit("  ").next(),
            Some("the Jira sync is running: one publish at a time"),
            "{}",
            foot(&mut h, page)
        );
        assert!(spawned(&fake).is_empty() && !fake.take().iter().any(|c| c.starts_with("prompt")));
        h.keys(page, "/<Esc>");
        // kiln's answer: what it did, said and kept; the project read, and
        // the plan read again once that read has ended -- not before.
        let process = fake.process("kiln jira sync --yes").unwrap().id;
        for line in fixture("../hand/jira-sync-applied.out").lines() {
            h.said(process, line, false);
        }
        h.exited(process, Some(0));
        h.settle();
        assert_eq!(
            foot(&mut h, page),
            "jira: 1 created, 1 updated, 1 moved; 1 file gained ticket keys (commit them)"
        );
        let after: Vec<String> = spawned(&fake)
            .into_iter()
            .map(|l| l.trim_start_matches("/usr/bin/kiln ").to_string())
            .collect();
        let at = |needle: &str| {
            after
                .iter()
                .position(|l| l == needle)
                .unwrap_or_else(|| panic!("no {needle} in {after:?}"))
        };
        assert!(at("list --format json") < at("status --format json"));
        assert!(at("status --format json") < at("check --format json"));
        assert!(
            at("check --format json") < at("jira sync --dry-run"),
            "the plan is asked of the project as that read leaves it: {after:?}"
        );
        assert!(h.ext.publishing.is_none());
        h.size = (110, 34);
        h.keys(page, "G");
        assert_snapshot!("exports-demo-jira-synced", h.screen(page));
        // Never a flag that overrides, and nothing but this one sync.
        for flag in FORBIDDEN {
            assert!(!fake.calls().iter().any(|c| c.contains(flag)), "{flag}");
        }
    }

    #[test]
    fn a_wrong_key_syncs_nothing_and_a_confluence_publish_blocks_a_sync() {
        let (fake, mut h, page) = jira_ready("jira-wrong");
        fake.on_prompt(
            "Sync Jira: creates, updates and moves tickets, jira (DEMO) → DEMO-100 \"Kiln: Demo\": 3 operations (1 create, 1 update, 1 move, 2 unchanged). Type DEMO to sync",
            "design-doc-confluence",
        );
        fake.take();
        h.keys(page, "P");
        assert!(spawned(&fake).is_empty());
        assert!(
            foot(&mut h, page).ends_with("“design-doc-confluence” isn't DEMO: nothing was synced"),
            "{}",
            foot(&mut h, page)
        );
    }

    #[test]
    fn jira_exit_3_says_to_commit_the_keys_and_sync_again() {
        let (fake, mut h, page) = jira_ready("jira-partial");
        fake.on_prompt(
            "Sync Jira: creates, updates and moves tickets, jira (DEMO) → DEMO-100 \"Kiln: Demo\": 3 operations (1 create, 1 update, 1 move, 2 unchanged). Type DEMO to sync",
            "DEMO",
        )
        .on_spawn("kiln jira sync --yes", Run::running(""));
        h.keys(page, "P");
        let process = fake.process("kiln jira sync --yes").unwrap().id;
        for line in fixture("../hand/jira-sync-partial.out").lines() {
            h.said(process, line, false);
        }
        h.said(
            process,
            fixture("../hand/jira-sync-partial.stderr").trim(),
            true,
        );
        h.exited(process, Some(3));
        h.settle();
        let line = foot(&mut h, page);
        assert!(line.starts_with("jira: 1 created, 0 updated, 0 moved; 1 file gained ticket keys (commit them) -- kiln: Jira"), "{line}");
        let s = h.screen(page);
        assert!(s.contains("commit them"), "{s}");
        assert!(h.ext.publishing.is_none());
    }

    #[test]
    fn a_dry_run_is_asked_once_and_r_reads_it_with_the_others() {
        let (fake, dir) = jira_fake("jira-once", run_hand("jira-sync-dry-run"));
        let (mut h, page) = open(&fake, &dir);
        assert_eq!(jira_dry_runs(&fake), 1);
        h.keys(page, "r");
        h.settle();
        h.saved("design/tasks/TASK-001.md");
        h.settle();
        assert_eq!(
            jira_dry_runs(&fake),
            0,
            "the project read again, not the plan"
        );
        h.keys(page, "R");
        assert_eq!(jira_dry_runs(&fake), 1);
    }
}
