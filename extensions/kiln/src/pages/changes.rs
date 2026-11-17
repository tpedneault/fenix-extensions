//! Changes: the items added, changed and removed since a baseline
//! revision (`kiln diff REV --format json`), under a heading each --
//! `TASK-002  Write readings to SQLite  status todo → doing; owner (none)
//! → ana` -- worded as `kiln diff`'s own text output words it.
//!
//! The baseline is a revision: a tag, a branch or a commit (`REV..REV`
//! compares two). Kiln's GUI defaults it to the most recent tag, but
//! `kiln diff` has no way to say "the latest tag" (that default is the
//! GUI's own, `kiln gui --since`), and a view can't run git to find one. So
//! the tab starts with no baseline and says how to give one, rather than
//! guessing a tag. Once there is one -- restored with the page, or set by
//! `b` when the frame can take a typed line -- it is part of the query's
//! args, so the frame asks again when it changes.
//!
//! What it deliberately doesn't do: run git or anything but `kiln diff`;
//! show a change's body or diff (`kiln diff` says only which parts
//! changed); open a removed item (its file is gone: its row has an ID and
//! no place, and `v`/`i` have nothing to ask about); or pick a tag (a
//! picker needs git, which only kiln may run).

use serde::Deserialize;

use super::view::{stdout, Cell, Ctx, Look, Place, Query, Reply, Row, View};
use crate::kiln::{Failure, Ran};
use crate::model::{self, count};
use fenix_ui_kit::{Key, KeyPress};

/// `kiln diff --format json`, as far as the tab reads it. Every field is
/// optional so a newer kiln's extras (and an older one's gaps) don't stop
/// the tab; `kiln_json` itself is checked by [`model::object`]. kiln 1.0.0
/// writes the commit as camelCase `fromCommit`.
#[derive(Debug, Default, Deserialize)]
struct Diff {
    /// The baseline as it was named (`v0.1`).
    #[serde(default)]
    from: String,
    /// The commit it named.
    #[serde(default, rename = "fromCommit")]
    from_commit: String,
    /// What it was compared with: `working tree`, or the second revision.
    #[serde(default)]
    to: String,
    #[serde(default)]
    items: Vec<Change>,
}

/// One item's change.
#[derive(Debug, Default, Deserialize)]
struct Change {
    /// `added`, `changed` or `removed`; another word a newer kiln may add
    /// is shown under a heading of its own.
    #[serde(default)]
    change: String,
    /// What changed, as kiln words it (`status todo → done`).
    #[serde(default)]
    details: Vec<String>,
    #[serde(default)]
    id: String,
    /// Where the item's file is (was, for a removed one), from the top.
    #[serde(default)]
    path: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// What the tab has heard from kiln, for the baseline it asked about.
#[derive(Debug)]
struct Heard {
    baseline: String,
    diff: Result<Diff, String>,
}

/// The Changes tab: the baseline, and kiln's last answer about it.
#[derive(Debug, Default)]
pub struct Changes {
    /// A revision `kiln diff` takes. None: nothing is asked.
    baseline: Option<String>,
    heard: Option<Heard>,
}

impl Changes {
    /// Sets the baseline from text a person typed or a saved page holds.
    /// A blank one means none; one that starts with `-` would be read by
    /// kiln as an option, so it is refused, not passed on. Returns whether
    /// it was taken.
    fn set_baseline(&mut self, rev: &str) -> bool {
        let rev = rev.trim();
        if rev.starts_with('-') {
            return false;
        }
        let rev = (!rev.is_empty()).then(|| rev.to_string());
        if rev != self.baseline {
            // What was heard was about another baseline.
            self.heard = None;
            self.baseline = rev;
        }
        true
    }
}

impl View for Changes {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let Some(baseline) = &self.baseline else {
            return vec![
                Row::note("no baseline yet: kiln diff compares with a revision, and can't say \"the latest tag\""),
                Row::note("press b to set one: a tag, a branch or a commit (v1.0, main, 1a2b3c4), or A..B for two"),
            ];
        };
        // Only an answer about this baseline is shown; until one comes,
        // no rows (the frame says it is reading).
        let diff = match &self.heard {
            Some(h) if &h.baseline == baseline => &h.diff,
            _ => return Vec::new(),
        };
        let diff = match diff {
            Ok(d) => d,
            Err(why) => return vec![Row::note(why.clone())],
        };
        if diff.items.is_empty() {
            return Vec::new();
        }

        // One pass: each change into its group, in kiln's order (it lists
        // type by type). The three known groups come first, in the order
        // the tab reads in; a word kiln adds later follows them.
        let mut groups: Vec<(&str, Vec<&Change>)> = vec![
            ("added", Vec::new()),
            ("changed", Vec::new()),
            ("removed", Vec::new()),
        ];
        for c in &diff.items {
            match groups.iter_mut().find(|(name, _)| *name == c.change) {
                Some((_, list)) => list.push(c),
                None => groups.push((&c.change, vec![c])),
            }
        }

        let mut rows = vec![Row::note(baseline_line(diff, baseline))];
        for (name, list) in groups.iter().filter(|(_, l)| !l.is_empty()) {
            rows.push(Row::heading(vec![
                Cell::title(heading(name)),
                Cell::muted(count(list.len(), "item")),
            ]));
            rows.extend(list.iter().map(|c| row(ctx, name, c)));
        }
        rows
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("b", "baseline")]
    }

    /// `b` can't prompt: a view has no way to ask for a line of text (the
    /// frame's only one is the `/` filter), and it isn't built here. So it
    /// says so instead of doing nothing; the baseline set in the page's
    /// saved state, or by `set_baseline` once a prompt exists, is used.
    fn key(&mut self, key: KeyPress, _row: Option<&Row>, _ctx: &Ctx) -> Reply {
        if key.mods.ctrl || key.mods.alt || key.mods.meta || key.key != Key::Char('b') {
            return Reply::Ignored;
        }
        Reply::Say(
            "b: a view can't ask for a revision yet, so the baseline can't be typed here".into(),
        )
    }

    fn empty(&self) -> String {
        match (&self.baseline, &self.heard) {
            (None, _) => "set a baseline with b".into(),
            (Some(b), Some(h)) if &h.baseline == b => match &h.diff {
                Ok(d) => format!("no changes since {}", baseline_name(d, b)),
                Err(why) => why.clone(),
            },
            (Some(b), _) => format!("reading the changes since {b}…"),
        }
    }

    fn queries(&self, _ctx: &Ctx) -> Vec<Query> {
        self.baseline
            .iter()
            .map(|b| Query::new("diff", ["diff", b.as_str(), "--format", "json"]))
            .collect()
    }

    fn answer(&mut self, _tag: &str, result: &Result<Ran, Failure>) {
        let Some(baseline) = self.baseline.clone() else {
            return;
        };
        let diff = stdout(result, &[0]).and_then(|text| model::object::<Diff>("kiln diff", text));
        self.heard = Some(Heard { baseline, diff });
    }

    fn save(&self) -> serde_json::Value {
        match &self.baseline {
            Some(b) => serde_json::json!({ "baseline": b }),
            None => serde_json::Value::Null,
        }
    }

    fn restore(&mut self, saved: &serde_json::Value) {
        if let Some(b) = saved["baseline"].as_str() {
            self.set_baseline(b);
        }
    }
}

/// A group's heading: `added` as `Added`.
fn heading(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "Other".into(),
    }
}

/// The baseline as kiln's text names it: `v0.1 (f496961)`.
fn baseline_name(diff: &Diff, asked: &str) -> String {
    let name = if diff.from.is_empty() {
        asked
    } else {
        &diff.from
    };
    match diff.from_commit.get(..7) {
        Some(short) => format!("{name} ({short})"),
        None => name.to_string(),
    }
}

/// The first line, as `kiln diff`'s text has it: `v0.1 (f496961) → the
/// working tree` (`to` is `working tree` or the second revision).
fn baseline_line(diff: &Diff, asked: &str) -> String {
    let to = match diff.to.as_str() {
        "" | "working tree" => "the working tree".to_string(),
        other => other.to_string(),
    };
    format!("{} → {to}", baseline_name(diff, asked))
}

/// One change's row: ID, title and what changed -- the details joined as
/// `kiln diff` joins them, or the type in brackets for an item that
/// appeared or went, which has nothing else to say.
fn row(ctx: &Ctx, group: &str, c: &Change) -> Row {
    let what = if c.details.is_empty() {
        match c.kind.as_str() {
            "" => String::new(),
            kind => format!("({kind})"),
        }
    } else {
        c.details.join("; ")
    };
    let look = match group {
        "added" => Look::Good,
        "removed" => Look::Bad,
        _ => Look::Text,
    };
    let cells = vec![
        Cell::id(&c.id),
        if group == "removed" {
            Cell::muted(&c.title)
        } else {
            Cell::text(&c.title)
        },
        Cell::new(what, look),
    ];
    if group == "removed" {
        // Its file is gone: no place for `Enter`, and no ID for `v`, `i`
        // or an edit to ask kiln about (kiln would say it doesn't exist).
        let mut row = Row::about(&c.id, None, cells);
        row.id = None;
        return row;
    }
    match ctx.loaded.item(&c.id) {
        Some(item) => Row::item(item, cells),
        // The project read doesn't have it (an answer from before a
        // reload): kiln's path, no line.
        None => Row::about(
            &c.id,
            (!c.path.is_empty()).then(|| Place {
                path: c.path.clone(),
                line: None,
            }),
            cells,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
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

    fn lines(view: &Changes, l: &Loaded) -> Vec<String> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        view.rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = if r.kind == Kind::Heading { "# " } else { "" };
                format!("{mark}{}", text.join(" | "))
            })
            .collect()
    }

    fn since(rev: &str) -> Changes {
        let mut v = Changes::default();
        v.restore(&serde_json::json!({ "baseline": rev }));
        v
    }

    fn ran(code: i32, out: &str, err: &str) -> Ran {
        Ran {
            code: Some(code),
            stdout: out.to_string(),
            stderr: err.lines().map(str::to_string).collect(),
        }
    }

    fn told(v: &mut Changes, out: &str) {
        v.answer("diff", &Ok(ran(0, out, "")));
    }

    #[test]
    fn with_no_baseline_it_says_how_to_set_one_and_asks_nothing() {
        let l = demo();
        let v = Changes::default();
        let rows = lines(&v, &l);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].starts_with("no baseline yet"), "{rows:?}");
        assert!(rows[1].contains("a tag, a branch or a commit"), "{rows:?}");
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        assert!(v.queries(&ctx).is_empty());
        assert_eq!(v.save(), serde_json::Value::Null);
    }

    #[test]
    fn the_demo_since_v0_1_is_added_and_changed_items_worded_as_kiln_does() {
        let l = demo();
        let mut v = since("v0.1");
        assert!(lines(&v, &l).is_empty(), "nothing until kiln answers");
        assert_eq!(v.empty(), "reading the changes since v0.1…");
        told(&mut v, &fixture("demo/diff-v0.1.json"));
        assert_eq!(
            lines(&v, &l),
            [
                "v0.1 (f496961) → the working tree",
                "# Added | 7 items",
                "TASK-004 | Add the export button | (task)",
                "TASK-005 | Ask about retention rules | (task)",
                "TASK-006 | Draft the user guide | (task)",
                "TASK-007 | Batch the writes | (task)",
                "TASK-008 | Tidy the build scripts | (task)",
                "TEST-001 | Readings arrive every second | (test)",
                "RISK-001 | The SD card wears out | (risk)",
                "# Changed | 4 items",
                "REQ-002 | Logged readings survive a power cut | + mitigates RISK-001",
                "TASK-001 | Set up the logger | status todo → done",
                "TASK-002 | Write readings to SQLite | status todo → doing; owner (none) → ana",
                "TASK-003 | Export the log as CSV | owner (none) → bo",
            ]
        );
    }

    /// The text form kiln prints is what the rows say, line for line.
    #[test]
    fn the_rows_say_what_kilns_text_output_says() {
        let l = demo();
        let mut v = since("v0.1");
        told(&mut v, &fixture("demo/diff-v0.1.json"));
        let text = fixture("demo/diff-v0.1.txt");
        let want: Vec<String> = text
            .lines()
            .filter(|t| t.starts_with("+ ") || t.starts_with("~ "))
            .map(|t| {
                // `~ ID Title: details` and `+ ID Title (type)`.
                let t = &t[2..];
                let (id, rest) = t.split_once(' ').unwrap();
                match rest.rsplit_once(": ") {
                    Some((title, what)) => format!("{id} | {title} | {what}"),
                    None => {
                        let (title, kind) = rest.rsplit_once(" (").unwrap();
                        format!("{id} | {title} | ({kind}")
                    }
                }
            })
            .collect();
        let mut got: Vec<String> = lines(&v, &l)
            .into_iter()
            .filter(|r| !r.starts_with('#') && r.contains(" | "))
            .collect();
        got.sort();
        let mut want = want;
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn a_removed_item_is_a_row_with_no_place_and_nothing_for_v_or_i() {
        let l = demo();
        let mut v = since("HEAD..v0.1");
        told(&mut v, &fixture("demo/diff-removed.json"));
        let rows = lines(&v, &l);
        assert_eq!(rows[0], "HEAD (8c0580d) → v0.1");
        assert_eq!(rows[1], "# Changed | 4 items");
        assert_eq!(
            rows[2],
            "REQ-002 | Logged readings survive a power cut | − mitigates RISK-001"
        );
        assert_eq!(rows[6], "# Removed | 7 items");
        assert_eq!(rows[7], "TASK-004 | Add the export button | (task)");

        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        let built = v.rows(&ctx);
        let removed = &built[7];
        assert_eq!(
            (removed.id.as_deref(), removed.place.as_ref()),
            (None, None)
        );
        assert!(removed.selectable());
        assert_eq!(removed.key(), "TASK-004");
        // An item that is still there opens at its file; an added one in
        // this demo was changed, so it has a place too.
        let kept = &built[2];
        assert_eq!(kept.id.as_deref(), Some("REQ-002"));
        assert_eq!(
            kept.place.as_ref().map(|p| p.path.as_str()),
            Some("design/requirements/REQ-002-logged-readings-survive-a-power-cut.md")
        );
    }

    #[test]
    fn no_changes_is_said_naming_the_baseline() {
        let l = demo();
        let mut v = since("HEAD");
        told(&mut v, &fixture("demo/diff-none.json"));
        assert!(lines(&v, &l).is_empty());
        assert_eq!(v.empty(), "no changes since HEAD (8c0580d)");
    }

    #[test]
    fn an_unknown_revision_is_one_note_with_kilns_line() {
        let l = demo();
        let mut v = since("v9.9");
        let stderr = fixture("errors/diff-unknown-rev.stderr");
        v.answer("diff", &Ok(ran(2, "", &stderr)));
        assert_eq!(
            lines(&v, &l),
            ["kiln: v9.9: git failed: `v9.9` isn't a revision in this repository"]
        );
    }

    #[test]
    fn a_folder_that_is_not_a_repository_is_one_note_with_kilns_line() {
        let l = demo();
        let mut v = since("v0.1");
        let stderr = fixture("errors/diff-no-repo.stderr");
        v.answer("diff", &Ok(ran(2, "", &stderr)));
        assert_eq!(
            lines(&v, &l),
            ["kiln: v0.1: not a git repository with commits"]
        );
        // kiln not run at all is one note too.
        v.answer("diff", &Err(Failure::TimedOut));
        assert_eq!(lines(&v, &l).len(), 1);
    }

    #[test]
    fn an_answer_that_is_not_json_is_one_note() {
        let l = demo();
        let mut v = since("v0.1");
        for text in ["", "not json", "[]", "{\"items\": 3, \"kiln_json\": 1}"] {
            told(&mut v, text);
            let rows = lines(&v, &l);
            assert_eq!(rows.len(), 1, "{text}: {rows:?}");
            assert!(rows[0].starts_with("kiln diff: "), "{rows:?}");
        }
    }

    #[test]
    fn a_new_kinds_of_change_and_missing_fields_do_not_stop_the_tab() {
        let l = demo();
        let mut v = since("v1");
        told(
            &mut v,
            r#"{"kiln_json":1,"from":"v1","items":[
                {"id":"X-1","change":"moved","title":"t"},
                {"id":"X-2","change":"added","extra":true}]}"#,
        );
        assert_eq!(
            lines(&v, &l),
            [
                "v1 → the working tree",
                "# Added | 1 item",
                "X-2 |  | ",
                "# Moved | 1 item",
                "X-1 | t | "
            ]
        );
    }

    #[test]
    fn the_baseline_is_kept_in_the_pages_state_and_asked_for() {
        let l = demo();
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        let mut v = since(" v0.1 ");
        let saved = v.save();
        assert_eq!(saved, serde_json::json!({ "baseline": "v0.1" }));
        assert_eq!(
            v.queries(&ctx),
            [Query::new("diff", ["diff", "v0.1", "--format", "json"])]
        );
        let mut w = Changes::default();
        w.restore(&saved);
        assert_eq!(w.baseline.as_deref(), Some("v0.1"));

        // Another baseline: asked again, and the old answer isn't shown.
        told(&mut v, &fixture("demo/diff-v0.1.json"));
        assert!(v.set_baseline("main"));
        assert_eq!(v.queries(&ctx)[0].args[1], "main");
        assert!(lines(&v, &l).is_empty());
        // Blank: none. An option, or an unreadable state: left alone.
        assert!(!v.set_baseline("--format"));
        assert_eq!(v.baseline.as_deref(), Some("main"));
        w.restore(&serde_json::json!("nonsense"));
        w.restore(&serde_json::json!({ "baseline": 4 }));
        w.restore(&serde_json::json!({ "baseline": "--results" }));
        assert_eq!(w.baseline.as_deref(), Some("v0.1"));
        w.restore(&serde_json::json!({ "baseline": "  " }));
        assert_eq!(w.baseline, None);
    }

    #[test]
    fn b_says_it_cannot_ask_yet_and_other_keys_are_not_its() {
        let l = demo();
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            looks: &looks,
        };
        let mut v = Changes::default();
        let key = |c: char| KeyPress::from(Key::Char(c));
        assert!(
            matches!(v.key(key('b'), None, &ctx), Reply::Say(s) if s.contains("can't be typed"))
        );
        assert_eq!(v.key(key('x'), None, &ctx), Reply::Ignored);
        assert_eq!(v.keys(), [("b", "baseline")]);
    }

    #[test]
    fn two_thousand_changes_stay_cheap() {
        let l = demo();
        let mut v = since("v0.1");
        let items: Vec<String> = (0..2000)
            .map(|n| {
                format!(
                    r#"{{"id":"T-{n}","change":"added","title":"t","type":"task","details":[]}}"#
                )
            })
            .collect();
        told(
            &mut v,
            &format!(
                r#"{{"kiln_json":1,"from":"v0.1","items":[{}]}}"#,
                items.join(",")
            ),
        );
        assert_eq!(lines(&v, &l).len(), 2002);
    }

    // -- The page ----------------------------------------------------------

    use crate::api::Event;
    use crate::tests::{at, demo as demo_fake, foot, opened};
    use fenix_test_kit::api::Extension as _;
    use fenix_test_kit::Ran as FakeRan;
    use fenix_test_kit::{assert_snapshot, Fake, Harness};

    /// The plan restored on its Changes tab with baseline `rev`, kiln
    /// answering `diff` with `ran`; returns the harness and the page.
    fn restored(fake: &Fake, rev: &str, ran: FakeRan) -> (Harness<crate::Kiln>, u64) {
        fake.on_spawn(&format!("kiln diff {rev} --format json"), ran);
        let (mut h, page) = opened(fake);
        h.close(page);
        let state = serde_json::json!({
            "v": 1,
            "tab": "changes",
            "tabs": { "changes": { "view": { "baseline": rev } } }
        })
        .to_string();
        assert!(h.ext.page_restore(41, "plan", &state));
        h.event(Event::PageShown((41, true)));
        h.settle();
        (h, 41)
    }

    fn diffs(fake: &Fake) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.contains(" diff "))
            .collect()
    }

    #[test]
    fn page_with_no_baseline_says_how_to_set_one_and_runs_no_diff() {
        let fake = demo_fake();
        let (mut h, page) = opened(&fake);
        h.keys(page, "7");
        assert_snapshot!("changes-demo-no-baseline", h.screen(page));
        assert!(diffs(&fake).is_empty(), "{:?}", diffs(&fake));
        h.keys(page, "b");
        assert!(
            foot(&mut h, page).contains("can't be typed"),
            "{}",
            foot(&mut h, page)
        );
        assert!(diffs(&fake).is_empty());
    }

    #[test]
    fn page_since_v0_1_shows_what_was_added_and_changed() {
        let fake = demo_fake();
        let (mut h, page) = restored(
            &fake,
            "v0.1",
            FakeRan::ok(crate::model::tests::fixture("demo/diff-v0.1.json")),
        );
        assert_snapshot!("changes-demo", h.screen(page));
        // Once for each read of the project, always for the baseline.
        let reads = fake
            .calls()
            .iter()
            .filter(|c| c.ends_with("kiln list --format json"))
            .count();
        assert_eq!(diffs(&fake).len(), reads, "{:?}", fake.calls());
        assert!(diffs(&fake)
            .iter()
            .all(|c| c.ends_with("kiln diff v0.1 --format json")));
        // The cursor starts on the first item row; Enter would open it.
        assert_eq!(at(&h, page).as_deref(), Some("TASK-004"));
        // The baseline is in the state the page saves.
        let state = h.ext.page_state(page).unwrap();
        assert!(state.contains(r#""baseline":"v0.1""#), "{state}");
    }

    #[test]
    fn page_removed_items_have_no_id_for_the_cursor() {
        let fake = demo_fake();
        let (mut h, page) = restored(
            &fake,
            "HEAD..v0.1",
            FakeRan::ok(crate::model::tests::fixture("demo/diff-removed.json")),
        );
        assert_snapshot!("changes-demo-removed", h.screen(page));
        // Down past the four changed items: the first removed row.
        h.keys(page, "jjjj");
        assert_eq!(at(&h, page), None, "{}", h.marked(page));
        let before = fake.calls().len();
        h.keys(page, "<Enter>vi");
        let said: Vec<String> = fake.calls()[before..]
            .iter()
            .filter(|c| c.starts_with("spawn") || c.starts_with("open"))
            .cloned()
            .collect();
        assert!(said.is_empty(), "{said:?}");
    }

    #[test]
    fn page_no_changes_names_the_baseline() {
        let fake = demo_fake();
        let (mut h, page) = restored(
            &fake,
            "HEAD",
            FakeRan::ok(crate::model::tests::fixture("demo/diff-none.json")),
        );
        assert!(
            h.screen(page).contains("no changes since HEAD (8c0580d)"),
            "{}",
            h.screen(page)
        );
    }

    #[test]
    fn page_errors_are_one_note_each() {
        use crate::model::tests::fixture;
        let cases = [
            (
                "v9.9",
                FakeRan::fails(2, fixture("errors/diff-unknown-rev.stderr")),
                "kiln: v9.9: git failed: `v9.9` isn't a revision in this repository",
            ),
            (
                "v0.1",
                FakeRan::fails(2, fixture("errors/diff-no-repo.stderr")),
                "kiln: v0.1: not a git repository with commits",
            ),
            (
                "v0.1",
                FakeRan::ok("not json"),
                "kiln diff: kiln's JSON can't be read",
            ),
        ];
        for (rev, ran, note) in cases {
            let fake = demo_fake();
            let (mut h, page) = restored(&fake, rev, ran);
            let screen = h.screen(page);
            assert!(screen.contains(note), "{note}\n{screen}");
            assert_eq!(at(&h, page), None);
        }
    }
}
