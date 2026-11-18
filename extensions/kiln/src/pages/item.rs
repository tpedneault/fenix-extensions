//! Item: for the item handed to it with `v` on any tab, what `kiln show`
//! gives -- one row per related item or code location -- with `<` back
//! and `>` forward through the items viewed.
//!
//! Why not only `C-o` and `C-i`, Kiln's GUI's keys: Fenix keeps Ctrl+O and
//! Ctrl+I for its own jump list (WRITING_EXTENSIONS.md, `page_key_press`),
//! so they never reach a page, and in a terminal `C-i` is `Tab`, which is
//! the frame's next tab. They're still taken when they do come (a Fenix
//! that lets them through), but the hints name `<` and `>`, which always
//! arrive and are no one else's on the page.
//!
//! The rows, top down: the item itself (type, ID, title, status; `Enter`
//! opens its file) and the first line of its body, muted; its facts (tags,
//! owner, due, Jira keys, level, a milestone's progress, coverage per role,
//! `yes` in the good look and `no` in the warn one, the way `K201` treats a
//! gap); then the items around it, labelled by how they relate: `parent`,
//! `child`, `implements →` (its own links, by the role's name),
//! `← implements` (what points at it), `mentioned by`; and code locations
//! (`← verifies  code  src/a.rs:4`, and `also in` for another file with the
//! same ID) as [`Row::code`]. `v` on a related row follows it: the frame
//! hands that ID back here and it is added to the history.
//!
//! Labels are honest about direction. kiln 1.0.0's `inbound` is keyed by
//! the role's *own* name (`verifies`, `part-of`), not by the inverse
//! wording its text output uses ("verified by"), and the data carries no
//! inverse names, so inbound rows are `← verifies`, "an item at the left of
//! the arrow verifies this one", and outgoing ones `verifies →`. Inventing
//! `verified by` would be a guess for roles a project defines itself.
//!
//! The body is not shown beyond its first non-empty line (muted): the file
//! is one `Enter` away and an editor shows it better than a row can.
//!
//! The history is bounded ([`HISTORY`]), kept in the page's saved state with
//! the position in it, and browser-like: viewing something new after going
//! back drops what was ahead. Viewing the item already shown adds nothing.
//!
//! What it deliberately doesn't do: show the body, edit anything, or
//! answer from a cache -- going back asks kiln again (one `show`, cheap),
//! so what's shown is never older than the read.

use super::view::{stdout, Cell, Ctx, Look, Place, Query, Reply, Row, View};
use crate::kiln::{Failure, Ran};
use crate::model::{self, Ref, Shown, Source};
use fenix_ui_kit::{Key, KeyPress};
use serde_json::{json, Value};

/// How many items are remembered for `C-o`: the oldest goes first.
const HISTORY: usize = 50;

/// The Item tab. Its state is the items viewed and kiln's answer about the
/// current one.
#[derive(Debug, Default)]
pub struct ItemView {
    /// The items viewed, oldest first.
    history: Vec<String>,
    /// The one shown: an index into `history`.
    at: usize,
    /// kiln's answer for the current item, or why there is none. Dropped
    /// when another item becomes the current one, so a page never shows
    /// one item's rows under another's name while the answer is out.
    answer: Option<Result<Shown, String>>,
}

impl ItemView {
    /// The item shown.
    fn current(&self) -> Option<&str> {
        self.history.get(self.at).map(String::as_str)
    }

    /// Moves to `at` in the history, if it's there.
    fn go(&mut self, at: Option<usize>) -> bool {
        match at.filter(|&a| a < self.history.len()) {
            Some(a) => {
                self.at = a;
                self.answer = None;
                true
            }
            None => false,
        }
    }
}

impl View for ItemView {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let Some(id) = self.current() else {
            return vec![Row::note(
                "no item yet: press v on an item in another tab to see it here",
            )];
        };
        match &self.answer {
            Some(Ok(shown)) if shown.id == id => shown_rows(ctx, shown),
            Some(Err(why)) => vec![Row::note(why.clone())],
            _ => vec![Row::note(format!("reading {id}…"))],
        }
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = Vec::new();
        if self.at > 0 {
            keys.push(("<", "back"));
        }
        if self.at + 1 < self.history.len() {
            keys.push((">", "forward"));
        }
        keys
    }

    /// `<` and `>`, or `C-o` and `C-i` when Fenix lets them through.
    fn key(&mut self, key: KeyPress, _row: Option<&Row>, _ctx: &Ctx) -> Reply {
        if key.mods.alt || key.mods.meta {
            return Reply::Ignored;
        }
        let back = match (key.key, key.mods.ctrl) {
            (Key::Char('<'), false) | (Key::Char('o'), true) => true,
            (Key::Char('>'), false) | (Key::Char('i'), true) => false,
            _ => return Reply::Ignored,
        };
        if back {
            if self.go(self.at.checked_sub(1)) {
                Reply::Rebuild
            } else {
                Reply::Say("no earlier item".into())
            }
        } else if self.go(Some(self.at + 1)) {
            Reply::Rebuild
        } else {
            Reply::Say("no later item".into())
        }
    }

    fn flex(&self) -> usize {
        2
    }

    fn empty(&self) -> String {
        "no item yet: press v on an item in another tab".into()
    }

    /// Only an ID is asked about: what was handed on or restored with the
    /// page is kiln's argument, and one that isn't an ID (`--yes`, from a
    /// saved state someone wrote) is never put where kiln reads options.
    fn queries(&self, _ctx: &Ctx) -> Vec<Query> {
        self.current()
            .filter(|id| crate::agent::is_id(id))
            .map(|id| Query::new("show", ["show", id, "--format", "json"]))
            .into_iter()
            .collect()
    }

    fn answer(&mut self, _tag: &str, result: &Result<Ran, Failure>) {
        // Exit 0 only: `show` has no other fine code, and its error line
        // ("kiln: TASK-099 doesn't exist") is what to say.
        self.answer = Some(stdout(result, &[0]).and_then(model::show));
    }

    fn hand(&mut self, id: &str) {
        if self.current() == Some(id) {
            return;
        }
        // Browser-like: what was ahead of the item shown is dropped.
        self.history
            .truncate(self.at + usize::from(!self.history.is_empty()));
        self.history.push(id.to_string());
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.at = self.history.len() - 1;
        self.answer = None;
    }

    fn save(&self) -> Value {
        json!({ "history": self.history, "at": self.at })
    }

    fn restore(&mut self, saved: &Value) {
        let Some(history) = saved["history"].as_array().map(|a| {
            a.iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<String>>>()
        }) else {
            return;
        };
        let (Some(mut history), Some(at)) = (history, saved["at"].as_u64()) else {
            return;
        };
        let at = at as usize;
        if at >= history.len() {
            return;
        }
        // A longer history than this keeps its newest items.
        let cut = history.len().saturating_sub(HISTORY);
        history.drain(..cut.min(at));
        self.at = at - cut.min(at);
        self.history = history;
        self.answer = None;
    }
}

/// The rows for one item, as described at the top of the file.
fn shown_rows(ctx: &Ctx, s: &Shown) -> Vec<Row> {
    let mut rows = Vec::new();
    let place = (!s.path.is_empty()).then(|| Place {
        path: s.path.clone(),
        line: (s.line > 0).then_some(s.line),
    });
    rows.push(Row::about(
        &s.id,
        place,
        vec![
            Cell::muted(&s.kind),
            Cell::id(&s.id),
            Cell::title(&s.title),
            ctx.status_of(&s.id, &s.status),
        ],
    ));
    if let Some(line) = s.body.lines().map(str::trim).find(|l| !l.is_empty()) {
        rows.push(Row::note(line));
    }

    // Facts: not rows the cursor stops at, so headings of two cells.
    let mut fact = |label: &str, value: Cell| {
        rows.push(Row::heading(vec![Cell::muted(label), value]));
    };
    if !s.tags.is_empty() {
        fact("tags", Cell::text(s.tags.join(", ")));
    }
    if let Some(owner) = &s.owner {
        fact("owner", Cell::text(owner));
    }
    if let Some(due) = &s.due {
        fact("due", Cell::text(due));
    }
    if !s.jira.is_empty() {
        fact("jira", Cell::text(s.jira.join(", ")));
    }
    if let Some(level) = s.level {
        let derived = if s.derived { " (derived)" } else { "" };
        fact("level", Cell::text(format!("{level}{derived}")));
    }
    // `null`: a milestone with no work. Nothing to say, so nothing shown.
    if let Some(p) = s.progress {
        fact(
            "progress",
            Cell::text(format!("{} of {} done", p.done, p.total)),
        );
    }
    if !s.coverage.is_empty() {
        let mut cells = vec![Cell::muted("coverage")];
        cells.extend(s.coverage.iter().map(|(role, covered)| {
            if *covered {
                Cell::new(format!("{role} yes"), Look::Good)
            } else {
                Cell::new(format!("{role} no"), Look::Warn)
            }
        }));
        rows.push(Row::heading(cells));
    }

    // The items around it.
    if let Some(parent) = &s.parent {
        rows.push(related_row(ctx, "parent", parent));
    }
    for child in &s.children {
        rows.push(related_row(ctx, "child", child));
    }
    for (role, items) in &s.links {
        for r in items {
            rows.push(related_row(ctx, &format!("{role} →"), r));
        }
    }
    for (role, sources) in &s.inbound {
        let label = format!("← {role}");
        for source in sources {
            match source {
                Source::Item(r) => rows.push(related_row(ctx, &label, r)),
                Source::Code { path, line } => rows.push(code_row(&label, "code", path, *line)),
            }
        }
    }
    for r in &s.mentioned_by {
        rows.push(related_row(ctx, "mentioned by", r));
    }
    for path in &s.also_in {
        rows.push(code_row("also in", "file", path, 0));
    }
    rows
}

/// A related item: its label, ID, title and status. One `list` doesn't
/// have, or `show` marks `missing`, is in the bad look and has no ID for
/// `v` to follow: there's nothing to show of it.
fn related_row(ctx: &Ctx, label: &str, r: &Ref) -> Row {
    let key = format!("{label} {}", r.id);
    if r.missing {
        let mut row = Row::about(
            &r.id,
            None,
            vec![
                Cell::muted(label),
                Cell::new(&r.id, Look::Bad),
                Cell::new("no item has this ID", Look::Bad),
            ],
        );
        row.id = None;
        return row.keyed(key);
    }
    ctx.row(
        &r.id,
        vec![
            Cell::muted(label),
            Cell::id(&r.id),
            Cell::text(&r.title),
            ctx.status_of(&r.id, &r.status),
        ],
    )
    .keyed(key)
}

/// A place in a file: `Enter` opens it. `what` fills the ID's column so
/// the path, which is long, sits in the title's, the one that gives way.
fn code_row(label: &str, what: &str, path: &str, line: u32) -> Row {
    let place = if line > 0 {
        format!("{path}:{line}")
    } else {
        path.to_string()
    };
    Row::code(
        path,
        line,
        vec![Cell::muted(label), Cell::muted(what), Cell::text(place)],
    )
    .keyed(format!("{label} {path}:{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::fixture;
    use crate::pages::view::{Kind, Looks};
    use crate::project::Loaded;
    use crate::tests::{at, demo, foot, opened, own};
    use fenix_test_kit::{assert_snapshot, Ran as Run};

    fn demo_loaded() -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture("demo/list.json")).unwrap(),
            model::status(&fixture("demo/status.json")),
            model::check(&fixture("demo/check.json")),
        )
    }

    fn own_loaded() -> Loaded {
        Loaded::new(
            model::Version::parse("kiln 1.0.0").unwrap(),
            model::list(&fixture("own/list.json")).unwrap(),
            model::status(&fixture("own/status.json")),
            model::check(&fixture("own/check.json")),
        )
    }

    fn ok(text: String) -> Result<Ran, Failure> {
        Ok(Ran {
            code: Some(0),
            stdout: text,
            stderr: vec![],
        })
    }

    /// A tab that was handed `id` and answered with `text`.
    fn viewing(id: &str, answer: Result<Ran, Failure>) -> ItemView {
        let mut v = ItemView::default();
        v.hand(id);
        v.answer("show", &answer);
        v
    }

    /// One line per row: `#` for headings, `!` for what the cursor stops
    /// at without an ID, cells joined by ` | `.
    fn lines(l: &Loaded, v: &ItemView) -> Vec<String> {
        let looks = Looks::new(l);
        let ctx = Ctx {
            loaded: l,
            closed: false,
            reading: false,
            changes: 0,
            config: Err("not read in this test"),
            publishing: None,
            looks: &looks,
        };
        v.rows(&ctx)
            .iter()
            .map(|r| {
                let text: Vec<&str> = r.cells.iter().map(|c| c.text.as_str()).collect();
                let mark = match r.kind {
                    Kind::Heading => "# ",
                    Kind::Note => "note: ",
                    Kind::Code => "code: ",
                    _ if r.id.is_none() => "! ",
                    _ => "",
                };
                format!("{mark}{}", text.join(" | "))
            })
            .collect()
    }

    #[test]
    fn a_requirement_shows_its_relations_and_coverage() {
        let v = viewing("REQ-001", ok(fixture("demo/show-REQ-001.json")));
        assert_eq!(
            lines(&demo_loaded(), &v),
            [
                "requirement | REQ-001 | Readings are logged every second | approved",
                "note: The logger stores one reading a second from each sensor.",
                "# coverage | implements yes | verifies yes",
                "child | REQ-002 | Logged readings survive a power cut | draft",
                "← implements | TASK-001 | Set up the logger | done",
                "← implements | TASK-002 | Write readings to SQLite | doing",
                "← verifies | TEST-001 | Readings arrive every second | passing",
            ]
        );
    }

    #[test]
    fn a_milestone_has_progress_and_a_due_date_a_task_its_links() {
        let l = demo_loaded();
        let ms = lines(&l, &viewing("MS-001", ok(fixture("demo/show-MS-001.json"))));
        assert_eq!(ms[2..4], ["# due | 2030-06-30", "# progress | 1 of 5 done"]);
        assert_eq!(ms.len(), 4 + 4, "four tasks are part of it");
        assert_eq!(ms[4], "← part-of | TASK-001 | Set up the logger | done");
        let task = lines(
            &l,
            &viewing("TASK-004", ok(fixture("demo/show-TASK-004.json"))),
        );
        assert_eq!(
            task[2..],
            [
                "depends-on → | TASK-003 | Export the log as CSV | todo",
                "part-of → | MS-001 | First release | active"
            ]
        );
    }

    #[test]
    fn a_milestone_without_work_shows_no_progress() {
        let text = r#"{"kiln_json":1,"id":"MS-009","type":"milestone","title":"Later","status":"planned","progress":null}"#;
        let v = viewing("MS-009", ok(text.into()));
        assert_eq!(
            lines(&demo_loaded(), &v),
            ["milestone | MS-009 | Later | planned"]
        );
    }

    #[test]
    fn code_locations_missing_references_and_the_facts() {
        let text = r#"{"kiln_json":1,"id":"TASK-001","type":"task","title":"T","status":"todo",
            "path":"design/a.md","line":2,"body":"\n\n  First.\nSecond\n",
            "tags":["a","b"],"owner":"tom","due":"2030-01-01","jira":["K-1","K-2"],
            "level":2,"derived":true,"coverage":{"verifies":false,"implements":true},
            "also_in":["design/b.md"],
            "links":{"implements":[{"id":"REQ-009","missing":true}]},
            "inbound":{"verifies":[{"id":"TEST-001","title":"t","status":"ready"},{"path":"src/a.rs","line":4}]},
            "mentioned_by":[{"id":"ADR-001","title":"d","status":"accepted"}]}"#;
        let l = demo_loaded();
        let v = viewing("TASK-001", ok(text.into()));
        assert_eq!(
            lines(&l, &v),
            [
                "task | TASK-001 | T | todo",
                "note: First.",
                "# tags | a, b",
                "# owner | tom",
                "# due | 2030-01-01",
                "# jira | K-1, K-2",
                "# level | 2 (derived)",
                "# coverage | implements yes | verifies no",
                "! implements → | REQ-009 | no item has this ID",
                "← verifies | TEST-001 | t | ready",
                "code: ← verifies | code | src/a.rs:4",
                "mentioned by | ADR-001 | d | accepted",
                "code: also in | file | design/b.md",
            ]
        );
        // The looks: a gap is a warning, a missing item is bad, a code
        // location opens at its line.
        let looks = Looks::new(&l);
        let ctx = Ctx {
            loaded: &l,
            closed: false,
            reading: false,
            changes: 0,
            config: Err("not read in this test"),
            publishing: None,
            looks: &looks,
        };
        let rows = v.rows(&ctx);
        assert_eq!(rows[7].cells[1].look, Look::Good);
        assert_eq!(rows[7].cells[2].look, Look::Warn);
        assert_eq!(rows[8].cells[1].look, Look::Bad);
        assert_eq!(rows[8].place, None);
        assert_eq!(
            rows[10].place,
            Some(Place {
                path: "src/a.rs".into(),
                line: Some(4)
            })
        );
        assert_eq!(rows[0].place.as_ref().unwrap().path, "design/a.md");
        assert_eq!(rows[9].id.as_deref(), Some("TEST-001"));
    }

    #[test]
    fn kilns_own_items_show_code_references_and_mentions() {
        let l = own_loaded();
        let v = viewing("REQ-001", ok(fixture("own/show-REQ-001.json")));
        assert_eq!(
            lines(&l, &v),
            [
                "requirement | REQ-001 | The repository is the source of truth | approved",
                "note: Every fact Kiln shows comes from files in the repository, plus git",
                "# coverage | implements yes | verifies yes",
                "child | REQ-013 | Edits keep the author's formatting and never fight an editor | verified",
                "code: ← verifies | code | crates/kiln/tests/cli.rs:957",
                "mentioned by | ADR-014 | Kiln pushes the tasks it manages to Jira | accepted",
            ]
        );
    }

    #[test]
    fn nothing_handed_yet_says_what_to_do() {
        let v = ItemView::default();
        assert_eq!(
            lines(&demo_loaded(), &v),
            ["note: no item yet: press v on an item in another tab to see it here"]
        );
        assert!(v.queries(&ctx_of(&demo_loaded())).is_empty());
        assert!(v.keys().is_empty());
    }

    fn ctx_of(l: &Loaded) -> Ctx<'_> {
        // Leaked: a test's looks outlive nothing else, and Ctx borrows.
        let looks: &'static Looks = Box::leak(Box::new(Looks::new(l)));
        Ctx {
            loaded: l,
            closed: false,
            reading: false,
            changes: 0,
            config: Err("not read in this test"),
            publishing: None,
            looks,
        }
    }

    #[test]
    fn an_answer_that_is_not_there_yet_or_not_readable_is_one_note() {
        let l = demo_loaded();
        let mut v = ItemView::default();
        v.hand("REQ-001");
        assert_eq!(lines(&l, &v), ["note: reading REQ-001…"]);
        v.answer("show", &ok("<html>".into()));
        assert_eq!(
            lines(&l, &v),
            ["note: kiln show: kiln's JSON can't be read: expected value at line 1 column 1"]
        );
        v.answer("show", &ok("[]".into()));
        assert_eq!(
            lines(&l, &v),
            ["note: kiln show: expected a JSON object, got an array"]
        );
        v.answer("show", &ok("{\"id\":\"REQ-001\"}".into()));
        assert_eq!(
            lines(&l, &v),
            ["note: kiln show: its JSON has no kiln_json, so its shape is unknown"]
        );
        // kiln's own line, as recorded for an ID it hasn't got.
        let failed = Err(Failure::Exit {
            code: Some(2),
            line: "kiln: TASK-099 doesn't exist".into(),
        });
        v.answer("show", &failed);
        assert_eq!(lines(&l, &v), ["note: kiln: TASK-099 doesn't exist"]);
        // An answer about another item is not shown under this one's name.
        v.answer("show", &ok(fixture("demo/show-REQ-002.json")));
        assert_eq!(lines(&l, &v), ["note: reading REQ-001…"]);
    }

    #[test]
    fn the_history_goes_back_and_forward_and_is_bounded() {
        let l = demo_loaded();
        let ctx = ctx_of(&l);
        let ctrl = |c| KeyPress::new(Key::Char(c), fenix_ui_kit::Mods::CTRL);
        let plain = |c| KeyPress::new(Key::Char(c), fenix_ui_kit::Mods::NONE);
        let mut v = ItemView::default();
        v.hand("A-1");
        v.hand("A-1");
        assert_eq!(v.history, ["A-1"], "the same item twice adds nothing");
        assert_eq!(
            v.key(ctrl('o'), None, &ctx),
            Reply::Say("no earlier item".into())
        );
        v.hand("A-2");
        v.hand("A-3");
        assert_eq!(v.keys(), [("<", "back")]);
        assert_eq!(v.key(plain('<'), None, &ctx), Reply::Rebuild);
        assert_eq!(v.current(), Some("A-2"));
        assert_eq!(v.keys(), [("<", "back"), (">", "forward")]);
        assert_eq!(v.key(plain('>'), None, &ctx), Reply::Rebuild);
        assert_eq!(v.current(), Some("A-3"));
        assert_eq!(
            v.key(plain('>'), None, &ctx),
            Reply::Say("no later item".into())
        );
        // Kiln's GUI's chords, when Fenix lets them through.
        assert_eq!(v.key(ctrl('o'), None, &ctx), Reply::Rebuild);
        assert_eq!(v.key(ctrl('i'), None, &ctx), Reply::Rebuild);
        assert_eq!(v.current(), Some("A-3"));
        assert_eq!(v.key(plain('o'), None, &ctx), Reply::Ignored);
        assert_eq!(v.key(ctrl('<'), None, &ctx), Reply::Ignored);
        assert_eq!(v.key(ctrl('x'), None, &ctx), Reply::Ignored);
        // Viewing something new from the middle drops what was ahead.
        v.key(ctrl('o'), None, &ctx);
        v.key(ctrl('o'), None, &ctx);
        v.hand("B-1");
        assert_eq!(v.history, ["A-1", "B-1"]);
        // Bounded: the oldest goes.
        for n in 0..HISTORY + 5 {
            v.hand(&format!("C-{n}"));
        }
        assert_eq!(v.history.len(), HISTORY);
        assert_eq!(v.current(), Some("C-54"));
        assert_eq!(v.history[0], "C-5");
    }

    #[test]
    fn the_state_round_trips_and_a_bad_one_leaves_the_default() {
        let mut v = ItemView::default();
        for id in ["A-1", "A-2", "A-3"] {
            v.hand(id);
        }
        v.go(Some(1));
        let mut w = ItemView::default();
        w.restore(&v.save());
        assert_eq!((w.history.clone(), w.at), (v.history.clone(), 1));
        assert_eq!(w.current(), Some("A-2"));
        for bad in [
            json!(null),
            json!("x"),
            json!({"history": [1], "at": 0}),
            json!({"history": ["A-1"], "at": 3}),
            json!({"history": ["A-1"]}),
            json!({"history": "A-1", "at": 0}),
        ] {
            let mut w = ItemView::default();
            w.restore(&bad);
            assert_eq!((w.history.len(), w.at), (0, 0), "{bad}");
        }
        // A saved history longer than the bound keeps the newest.
        let long: Vec<String> = (0..HISTORY + 10).map(|n| format!("A-{n}")).collect();
        let mut w = ItemView::default();
        w.restore(&json!({"history": long, "at": HISTORY + 9}));
        assert_eq!(w.history.len(), HISTORY);
        assert_eq!(w.current(), Some("A-59"));
    }

    #[test]
    fn it_asks_kiln_for_the_current_item_only() {
        let l = demo_loaded();
        let mut v = ItemView::default();
        v.hand("REQ-002");
        assert_eq!(
            v.queries(&ctx_of(&l)),
            [Query::new("show", ["show", "REQ-002", "--format", "json"])]
        );
    }

    /// A saved state is someone's text: what isn't an ID in it is never
    /// put to kiln, where `--yes` or `--format=x` would be read as options.
    #[test]
    fn what_isnt_an_id_is_never_asked_about() {
        let l = demo_loaded();
        let mut v = ItemView::default();
        v.restore(&json!({"history": ["--yes"], "at": 0}));
        assert!(v.queries(&ctx_of(&l)).is_empty());
        v.hand("-x");
        assert!(v.queries(&ctx_of(&l)).is_empty());
        let mut i = super::super::impact::Impact::default();
        i.restore(&json!({"id": "--format=gitlab"}));
        assert!(i.queries(&ctx_of(&l)).is_empty());
        i.hand("REQ-001");
        assert_eq!(i.queries(&ctx_of(&l)).len(), 1);
    }

    // -- the page ------------------------------------------------------------

    /// The demo's kiln, answering `show` for what was recorded.
    fn shows(ids: &[&str]) -> fenix_test_kit::Fake {
        let fake = demo();
        for id in ids {
            fake.on_spawn(
                &format!("kiln show {id} --format json"),
                Run::ok(fixture(&format!("demo/show-{id}.json"))),
            );
        }
        fake
    }

    #[test]
    fn v_on_the_tree_shows_the_item_and_v_again_follows_a_relation() {
        let fake = shows(&["REQ-001", "REQ-002"]);
        let (mut h, page) = opened(&fake);
        assert!(h.screen(page).contains("Tree"));
        h.keys(page, "v");
        assert_snapshot!("item-demo", h.screen(page));
        assert_eq!(at(&h, page).as_deref(), Some("REQ-001"));
        // The child is the first row the cursor can go to.
        h.keys(page, "jv");
        assert_eq!(at(&h, page).as_deref(), Some("REQ-002"));
        assert_snapshot!("item-demo-followed", h.screen(page));
        // Back, then forward again, with the keys the hints name.
        assert_eq!(foot(&mut h, page).split("  ").next(), Some("< back"));
        h.keys(page, "<lt>");
        assert_eq!(at(&h, page).as_deref(), Some("REQ-001"));
        assert_eq!(foot(&mut h, page).split("  ").next(), Some("> forward"));
        h.keys(page, ">");
        assert_eq!(at(&h, page).as_deref(), Some("REQ-002"));
        h.keys(page, ">");
        assert_eq!(foot(&mut h, page), "no later item");
        assert!(h
            .screen(page)
            .contains("Logged readings survive a power cut"));
        // The page's state keeps the history.
        let saved = fenix_test_kit::api::Extension::page_state(&mut h.ext, page).unwrap();
        assert!(saved.contains("REQ-002"), "{saved}");
    }

    #[test]
    fn enter_opens_a_related_item_and_a_code_location() {
        let fake = shows(&["REQ-001"]);
        let (mut h, page) = opened(&fake);
        h.keys(page, "v");
        fake.take();
        h.keys(page, "<Enter>");
        assert_eq!(
            fake.take(),
            ["open /project/design/requirements/REQ-001-readings-are-logged-every-second.md:2"]
        );
        h.keys(page, "j<Enter>");
        assert_eq!(
            fake.take(),
            ["open /project/design/requirements/REQ-002-logged-readings-survive-a-power-cut.md:2"]
        );
    }

    #[test]
    fn an_unknown_item_is_kilns_own_line() {
        let fake = demo();
        fake.on_spawn(
            "kiln show TASK-099 --format json",
            Run::fails(2, fixture("errors/unknown-id.stderr")),
        );
        let (mut h, page) = opened(&fake);
        // Handed an ID the project hasn't got (a stale page, a typo).
        crate::tests::hand(&mut h, page, crate::pages::Tab::Item, "TASK-099");
        assert_snapshot!("item-demo-unknown", h.screen(page));
        assert!(h.screen(page).contains("\nkiln: TASK-099 doesn't exist"));
    }

    #[test]
    fn an_answer_that_is_not_json_is_a_note() {
        let fake = demo();
        fake.on_spawn("kiln show REQ-001", Run::ok("REQ-001: not json\n"));
        let (mut h, page) = opened(&fake);
        h.keys(page, "v");
        let s = h.screen(page);
        assert!(
            s.contains("\nkiln show: kiln's JSON can't be read: expected value"),
            "{s}"
        );
    }

    #[test]
    fn with_nothing_handed_the_tab_says_to_press_v() {
        let fake = demo();
        let (mut h, page) = opened(&fake);
        h.keys(page, "8");
        assert!(h
            .screen(page)
            .contains("\nno item yet: press v on an item in another tab to see it here\n"));
    }

    #[test]
    fn kilns_own_project_shows_code_references() {
        let fake = own();
        fake.on_spawn(
            "kiln show REQ-001 --format json",
            Run::ok(fixture("own/show-REQ-001.json")),
        );
        let (mut h, page) = opened(&fake);
        h.size = (110, 30);
        h.keys(page, "v");
        assert_snapshot!("item-own", h.screen(page));
    }
}
