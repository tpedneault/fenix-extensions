//! The adapter's own tests, on a made-up runner, `tally`: a test is a `#
//! test <name>` line, a case `# case <name>[<x>]` under it, `tally run
//! <names>` runs them and prints `PASS <name> 3ms`, `FAIL <name> at
//! <file>:<line>: <message>`, `SKIP <name>`. The test kit's driver tests
//! the same shapes against Fenix's rules; these look at the adapter's
//! answers themselves.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::Command;

use super::*;
use crate::host::mock::Shared;

#[derive(Default)]
struct Seen {
    asks: Vec<Ask>,
    kept: Vec<Vec<(String, String)>>,
    readers: usize,
}

struct Tally {
    seen: Rc<RefCell<Seen>>,
    runner: Rc<RefCell<(String, Option<String>)>>,
}

struct TallyReader;

impl Reader for TallyReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        let mut words = line.text.splitn(3, ' ');
        match (words.next(), words.next(), words.next()) {
            (Some("PASS"), Some(name), ms) => out.push(Report::Ended(Ended::new(format!("t {name}"), Outcome::Passed).ms(ms.and_then(|m| m.trim_end_matches("ms").parse().ok()).unwrap_or(0.0)))),
            (Some("SKIP"), Some(name), _) => out.push(Report::Ended(Ended::new(format!("t {name}"), Outcome::Skipped))),
            (Some("FAIL"), Some(name), Some(rest)) => {
                let (place, message) = rest.strip_prefix("at ").and_then(|r| r.split_once(": ")).unwrap_or(("", rest));
                let mut e = Ended::new(format!("t {name}"), Outcome::Failed).message(message);
                if let Some((file, l)) = place.rsplit_once(':') {
                    e = e.at(At::new(file, l.parse().unwrap_or(1), None));
                }
                out.push(Report::Ended(e));
            }
            (Some("FOUND"), Some(name), _) => out.push(Report::Found(Found::test(format!("t {name}"), name))),
            (Some("OUT"), Some(name), Some(text)) => out.push(Report::Output { key: Some(format!("t {name}").into()), text: text.into() }),
            (Some("BUILT"), Some(path), _) => out.push(Report::Keep { name: "binary".into(), value: path.into() }),
            (Some("LONG"), _, _) => out.push(Report::Output { key: None, text: line.text.len().to_string() }),
            _ => {}
        }
    }
}

impl TestRunner for Tally {
    fn provider(&self) -> &str {
        "tally"
    }
    fn runner(&mut self) -> (String, Option<String>) {
        self.runner.borrow().clone()
    }
    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path).to_string();
        let groups = vec![Group::package("tally"), Group::file(file.clone())];
        text.lines()
            .enumerate()
            .filter_map(|(i, l)| {
                let name = l.strip_prefix("# test ")?;
                Some(Found::test(format!("t {name}"), name).under(groups.clone()).line(i as u32 + 1).end_line(i as u32 + 1))
            })
            .collect()
    }
    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        self.seen.borrow_mut().asks.push(ask.clone());
        let cmd = Command::new("tally").args(["run", "--reporter", "${data}/lang-kit/tests/tally.rep"]);
        Ok(vec![Step::new(cmd.arg("@${step_files}/ids.txt"), "tally").file("ids.txt", "t adds\n")])
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        self.seen.borrow_mut().readers += 1;
        Box::new(TallyReader)
    }
    fn launch(&mut self, test: &Found, kept: &[(String, String)]) -> Result<Launching, String> {
        self.seen.borrow_mut().kept.push(kept.to_vec());
        match kept.iter().find(|(n, _)| n == "binary") {
            Some((_, path)) => Ok(Launching::Now(fenix_extension_api::LaunchConfig::launch(&format!("Tally: {}", test.key), &["tally"]).program(path))),
            None => Ok(Launching::BuildFirst(vec![Step::new(Command::new("tally").arg("build"), "building")])),
        }
    }
    fn assets(&self) -> Vec<(&str, &[u8])> {
        vec![("tally.rep", b"report\n")]
    }
}

struct Rig {
    kit: Kit,
    host: Shared,
    seen: Rc<RefCell<Seen>>,
    runner: Rc<RefCell<(String, Option<String>)>>,
    data: std::path::PathBuf,
}

fn rig(name: &str) -> Rig {
    let data = std::env::temp_dir().join(format!("lang-kit-tests-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let host = Shared::default();
    let seen = Rc::new(RefCell::new(Seen::default()));
    let runner = Rc::new(RefCell::new(("tally".to_string(), None)));
    let mut kit = Kit::with_host("tally", Box::new(host.clone())).tests(Box::new(Tally { seen: seen.clone(), runner: runner.clone() }));
    kit.tests.data = data.to_string_lossy().into_owned();
    Rig { kit, host, seen, runner, data }
}

fn line(text: &str) -> api::Line {
    api::Line { text: text.into(), stderr: false, cut: false }
}

#[test]
fn a_scan_gives_its_groups_first_with_their_ids_and_lines_from_0() {
    let mut r = rig("scan");
    let found = r.kit.tests_scan("tally", "/project/a.tally", "x\n# test adds\n# test adds\n");
    let ids: Vec<(&str, Option<&str>, api::Kind)> = found.iter().map(|f| (f.id.as_str(), f.parent.as_deref(), f.kind)).collect();
    assert_eq!(ids, [("\u{1f}tally", None, Kind::Group), ("\u{1f}tally\u{1f}a.tally", Some("\u{1f}tally"), Kind::File), ("t adds", Some("\u{1f}tally\u{1f}a.tally"), Kind::Test)], "one `adds`");
    assert_eq!((found[1].path.as_deref(), found[2].path.as_deref(), found[2].line), (Some("/project/a.tally"), None, Some(2)), "the later kept, as Fenix keeps it");
    assert_eq!(r.host.take(), ["log tests: tally: the scan of /project/a.tally found t adds twice; the earlier is left out"]);
    assert!(r.kit.tests_scan("other", "/project/a.tally", "# test adds").is_empty());
    assert!(r.kit.tests_plan("other", 1, api::Ask::RunAll).is_err());
}

#[test]
fn reporters_are_written_once_named_in_needs_and_a_steps_files_go_with_it() {
    let mut r = rig("needs");
    r.kit.activate().unwrap();
    let rep = r.data.join("lang-kit/tests/tally.rep");
    assert_eq!(std::fs::read(&rep).unwrap(), b"report\n");
    let written = std::fs::metadata(&rep).unwrap().modified().unwrap();
    r.kit.activate().unwrap();
    assert_eq!(std::fs::metadata(&rep).unwrap().modified().unwrap(), written, "the same bytes aren't written again");

    let plan = r.kit.tests_plan("tally", 7, api::Ask::RunAll).unwrap();
    let step = &plan.steps[0];
    assert_eq!(step.command.args.last().map(String::as_str), Some("@${data}/lang-kit/tests/runs/7-0/ids.txt"));
    assert_eq!(step.needs, ["/data/lang-kit/tests/runs/7-0/ids.txt", "/data/lang-kit/tests/tally.rep"]);
    assert_eq!(std::fs::read_to_string(r.data.join("lang-kit/tests/runs/7-0/ids.txt")).unwrap(), "t adds\n");
    r.kit.tests_step_ended("tally", 7, 0, StepEnd::Exited(Some(0)));
    assert!(!r.data.join("lang-kit/tests/runs/7-0").exists(), "deleted as its step ends");
    std::fs::remove_dir_all(&r.data).unwrap();
}

#[test]
fn the_runner_is_told_once_then_only_when_it_changes() {
    let mut r = rig("told");
    r.kit.activate().unwrap();
    r.kit.handle(&fenix_extension_api::Event::Saved("a.tally".into()));
    assert_eq!(r.host.0.borrow().test_providers, [("tally".to_string(), "tally".to_string(), None)]);
    *r.runner.borrow_mut() = ("tally --fast".into(), Some("old".into()));
    r.kit.file_saved("/project/a.tally");
    r.kit.tests_runner_changed();
    assert_eq!(r.host.take().iter().filter(|c| c.starts_with("test-provider")).collect::<Vec<_>>(), ["test-provider tally: tally", "test-provider tally: tally --fast (old)"]);
    let _ = std::fs::remove_dir_all(&r.data);
}

#[test]
fn a_cut_line_is_joined_across_calls_per_stream() {
    let mut r = rig("cut");
    r.kit.tests_plan("tally", 1, api::Ask::RunAll).unwrap();
    let piece = |text: &str, stderr, cut| api::Line { text: text.into(), stderr, cut };
    let out = r.kit.tests_read("tally", 1, 0, vec![piece("PASS a", false, true), piece("noise", true, false), piece("dds 3", false, true)]);
    assert!(out.is_empty(), "nothing whole yet: {out:?}");
    let out = r.kit.tests_read("tally", 1, 0, vec![piece("ms", false, false), piece("LONG ", false, true)]);
    assert!(matches!(&out[..], [api::Report::Ended(e)] if e.id == "t adds" && e.duration_ms == Some(3.0)), "{out:?}");
    // Past 1 MiB the rest goes; the reader gets what came, at the step's
    // end when no last piece comes.
    let big = "x".repeat(300_000);
    let lines = (0..5).map(|_| piece(&big, false, true)).collect();
    assert!(r.kit.tests_read("tally", 1, 0, lines).is_empty());
    let out = r.kit.tests_step_ended("tally", 1, 0, StepEnd::Stopped);
    assert_eq!(out, [api::Report::Output { id: None, text: (1 << 20).to_string() }]);
    let _ = std::fs::remove_dir_all(&r.data);
}

/// Fenix §4 "A result", §19 K7: each `Ended` goes as it came -- Fenix
/// keeps the last -- and outputs of one test in a row are one.
#[test]
fn each_result_goes_as_it_came_and_outputs_are_joined() {
    let mut r = rig("results");
    r.kit.tests_plan("tally", 1, api::Ask::RunAll).unwrap();
    let out = r.kit.tests_read("tally", 1, 0, ["FAIL adds at src/a.t:4: no", "PASS adds 2ms", "OUT adds one", "OUT adds two", "OUT other three"].map(line).to_vec());
    let ended: Vec<&api::Ended> = out.iter().filter_map(|o| match o {
        api::Report::Ended(e) => Some(e),
        _ => None,
    }).collect();
    assert_eq!(ended.iter().map(|e| e.outcome).collect::<Vec<_>>(), [Outcome::Failed, Outcome::Passed], "the retry's pass is the last: Fenix keeps it");
    assert_eq!((ended[0].message.as_deref(), ended[0].at.as_ref().map(|a| (a.path.as_str(), a.line))), (Some("no"), Some(("/project/src/a.t", 3))));
    assert_eq!(out[2], api::Report::Output { id: Some("t adds".into()), text: "one\ntwo".into() });
    assert_eq!(out.len(), 4);
    let _ = std::fs::remove_dir_all(&r.data);
}

/// A reader that holds a test's phases sends one `Ended`: the worse
/// outcome, its message and place, the texts in order, the times added.
#[test]
fn phases_held_until_whole_are_one_result() {
    let call = Ended::new("t x", Outcome::Passed).ms(2.0).text("call");
    let teardown = Ended::new("t x", Outcome::Errored).ms(1.0).message("fixture `db` failed").at(At::new("conftest.py", 9, None)).text("teardown");
    let whole = call.and_then(teardown);
    assert_eq!((whole.outcome, whole.ms, whole.message.as_deref(), whole.text.as_deref()), (Outcome::Errored, Some(3.0), Some("fixture `db` failed"), Some("call\nteardown")));
    let failed = Ended::new("t x", Outcome::Failed).message("assert").and_then(Ended::new("t x", Outcome::Passed));
    assert_eq!((failed.outcome, failed.message.as_deref()), (Outcome::Failed, Some("assert")));
}

#[test]
fn a_run_the_pack_didnt_plan_gets_a_fresh_reader() {
    let mut r = rig("restart");
    r.kit.tests_plan("tally", 3, api::Ask::RunAll).unwrap();
    r.kit.tests_read("tally", 3, 0, vec![line("PASS adds 1ms")]);
    // The pack started again: a new kit, the same run.
    let mut again = rig("restart-2");
    let out = again.kit.tests_read("tally", 3, 0, vec![line("PASS subs 1ms")]);
    assert!(matches!(&out[..], [api::Report::Ended(e)] if e.id == "t subs"));
    assert_eq!(again.host.take(), ["log tests: tally: run 3 wasn't planned since the pack started; its lines are read afresh"]);
    assert!(again.kit.tests_step_ended("tally", 3, 0, StepEnd::Exited(Some(0))).is_empty());
    assert_eq!(again.seen.borrow().readers, 1);
    let _ = (std::fs::remove_dir_all(&r.data), std::fs::remove_dir_all(&again.data));
}

fn rec(id: &str, parent: Option<&str>, label: &str, kind: Kind) -> api::Found {
    api::Found { id: id.into(), parent: parent.map(String::from), label: label.into(), kind, ..api::Found::default() }
}

/// Fenix §19, finding 3: a record whose `parent` is in the ask is part of
/// a fill-in, one whose `parent` isn't was selected -- no id is read. A
/// file's fill-in is whole, its suites in its tests' groups; a file with
/// none comes with what the memory knows under it.
#[test]
fn an_ask_is_told_apart_by_its_records_parents() {
    let mut r = rig("ask");
    r.kit.tests_scan("tally", "/project/a.tally", "# test adds\n# test subs\n");
    let (tally, file, other) = ("\u{1f}tally", "\u{1f}tally\u{1f}a.tally", "\u{1f}tally\u{1f}b.tally");
    let suite = format!("{file}\u{1f}deep");
    let mut adds = rec("t adds", Some(file), "adds", Kind::Test);
    adds.path = Some("/project/a.tally".into());
    adds.line = Some(0);
    let mut file_rec = rec(file, Some(tally), "a.tally", Kind::File);
    file_rec.path = Some("/project/a.tally".into());
    // A file and its fill-in (a suite between, then a test in it, then
    // `adds`), then a test selected beside another file -- its parent, a
    // file not asked -- then a file without a fill-in.
    let records = vec![
        file_rec.clone(),
        rec(&suite, Some(file), "deep", Kind::Suite),
        rec("t deep::inner", Some(&suite), "inner", Kind::Test),
        adds.clone(),
        rec("t one", Some(other), "one", Kind::Test),
        rec("\u{1f}tally\u{1f}c.tally", Some(tally), "c.tally", Kind::File),
    ];
    r.kit.tests_plan("tally", 1, api::Ask::Run(records)).unwrap();
    let asks = &r.seen.borrow().asks;
    let Ask::Run(items) = &asks[0] else { panic!() };
    assert_eq!(items.len(), 3, "{items:?}");
    match &items[0] {
        Asked::Group { groups, tests: Some(tests), known } => {
            assert_eq!(groups.iter().map(|g| (g.kind, g.label.as_str())).collect::<Vec<_>>(), [(Kind::Group, "tally"), (Kind::File, "a.tally")]);
            assert_eq!(tests.iter().map(|t| (t.key.0.as_str(), t.groups.len())).collect::<Vec<_>>(), [("t deep::inner", 3), ("t adds", 2)]);
            assert_eq!((tests[1].file.as_deref(), tests[1].line), (Some("a.tally"), Some(1)));
            assert!(known.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&items[1], Asked::Test(f) if f.key.0 == "t one" && f.data.is_none()), "selected: its parent isn't in the ask");
    assert!(matches!(&items[2], Asked::Group { tests: None, .. }), "a file with nothing after it");
    let _ = std::fs::remove_dir_all(&r.data);

    // A file alone: no fill-in (it didn't fit, or a test in it can't be
    // named), so the runner's own filter, and what the memory knows.
    let mut r = rig("ask-alone");
    r.kit.tests_scan("tally", "/project/a.tally", "# test adds\n# test subs\n");
    r.kit.tests_plan("tally", 1, api::Ask::Run(vec![rec(file, Some(tally), "a.tally", Kind::File), rec("t zed", None, "zed", Kind::Test)])).unwrap();
    let asks = &r.seen.borrow().asks;
    let Ask::Run(items) = &asks[0] else { panic!() };
    assert_eq!(items[0], Asked::Group { groups: vec![Group::package("tally"), Group::new(Kind::File, "a.tally")], tests: None, known: vec![Key::from("t adds"), Key::from("t subs")] });
    assert!(matches!(&items[1], Asked::Test(f) if f.key.0 == "t zed" && f.groups.is_empty()), "at the top: Fenix never found its parent");
    let _ = std::fs::remove_dir_all(&r.data);
}

/// A case with cases of its own (Go's `t.Run` inside a `t.Run`) is part of
/// a file's fill-in with them: they're not told as selected on their own,
/// which a run would plan as one more step.
#[test]
fn a_case_under_a_case_is_part_of_the_fill_in() {
    let mut r = rig("ask-nested");
    let (tally, file) = ("\u{1f}tally", "\u{1f}tally\u{1f}a.tally");
    let mut file_rec = rec(file, Some(tally), "a.tally", Kind::File);
    file_rec.path = Some("/project/a.tally".into());
    let mut nested = rec("t nested", Some(file), "nested", Kind::Test);
    nested.template = true;
    let records = vec![
        file_rec,
        nested,
        rec("t nested/outer", Some("t nested"), "outer", Kind::Case),
        rec("t nested/outer/inner", Some("t nested/outer"), "inner", Kind::Case),
        rec("t other", Some(file), "other", Kind::Test),
    ];
    r.kit.tests_plan("tally", 1, api::Ask::Run(records)).unwrap();
    let asks = &r.seen.borrow().asks;
    let Ask::Run(items) = &asks[0] else { panic!() };
    assert_eq!(items.len(), 1, "{items:?}");
    let Asked::Group { tests: Some(tests), .. } = &items[0] else { panic!("{items:?}") };
    assert_eq!(tests.iter().map(|t| (t.key.0.as_str(), t.of.as_ref().map(|k| k.0.as_str()))).collect::<Vec<_>>(), [("t nested", None), ("t nested/outer", Some("t nested")), ("t nested/outer/inner", Some("t nested/outer")), ("t other", None)]);
    let _ = std::fs::remove_dir_all(&r.data);
}

/// Fenix §19, finding 2: after the pack starts again (no memory) or
/// Fenix does (no `data`), a test asked is planned from what Fenix sent:
/// its id, its place, its parent's labels.
#[test]
fn an_ask_after_a_restart_is_planned_from_the_records_alone() {
    let mut r = rig("restart-ask");
    let file = "\u{1f}tally\u{1f}src/a.tally";
    let mut adds = rec("t adds", Some(file), "adds", Kind::Test);
    adds.path = Some("/project/src/a.tally".into());
    adds.line = Some(4);
    r.kit.tests_plan("tally", 1, api::Ask::Run(vec![adds])).unwrap();
    let asks = &r.seen.borrow().asks;
    let Ask::Run(items) = &asks[0] else { panic!() };
    let Asked::Test(f) = &items[0] else { panic!() };
    assert_eq!((f.key.0.as_str(), f.file.as_deref(), f.line, f.data.as_deref()), ("t adds", Some("src/a.tally"), Some(5), None));
    assert_eq!(f.groups.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(), ["tally", "src/a.tally"], "read back from its parent's id");
    let _ = std::fs::remove_dir_all(&r.data);
}

/// Fenix §19, K9: a `found` in a build-first plan is ignored, so it isn't
/// sent.
#[test]
fn a_build_sends_no_found() {
    let mut r = rig("build-found");
    r.kit.tests_scan("tally", "/project/a.tally", "# test adds\n");
    let adds = rec("t adds", Some("\u{1f}tally\u{1f}a.tally"), "adds", Kind::Test);
    r.kit.tests_launch("tally", 4, adds).unwrap();
    assert!(r.kit.tests_read("tally", 4, 0, vec![line("FOUND gen")]).is_empty());
    let _ = std::fs::remove_dir_all(&r.data);
}

#[test]
fn a_build_first_launch_gets_what_its_build_kept() {
    let mut r = rig("launch");
    r.kit.tests_scan("tally", "/project/a.tally", "# test adds\n");
    let adds = api::Found { id: "t adds".into(), parent: Some("\u{1f}tally\u{1f}a.tally".into()), label: "adds".into(), kind: Kind::Test, path: None, line: Some(0), end_line: None, skip: None, template: false, data: None };
    let api::Launch::BuildFirst(plan) = r.kit.tests_launch("tally", 9, adds.clone()).unwrap() else { panic!() };
    assert_eq!(plan.steps[0].command.args, ["build"]);
    r.kit.tests_read("tally", 9, 0, vec![line("BUILT /project/target/adds")]);
    r.kit.tests_step_ended("tally", 9, 0, StepEnd::Exited(Some(0)));
    let api::Launch::Launch(config) = r.kit.tests_launch("tally", 9, adds).unwrap() else { panic!() };
    assert_eq!((config.name.as_str(), config.program.as_deref()), ("Tally: t adds", Some("/project/target/adds")));
    assert_eq!(r.seen.borrow().kept, [vec![], vec![("binary".to_string(), "/project/target/adds".to_string())]]);
    let _ = std::fs::remove_dir_all(&r.data);
}

/// A runner whose scan gives what it's told, and that keeps what it's
/// asked, for the groups' cache.
struct Given(Vec<Found>, Rc<RefCell<Vec<Ask>>>);

impl TestRunner for Given {
    fn provider(&self) -> &str {
        "given"
    }
    fn runner(&mut self) -> (String, Option<String>) {
        ("given".into(), None)
    }
    fn scan(&mut self, _path: &str, _text: &str) -> Vec<Found> {
        self.0.clone()
    }
    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        self.1.borrow_mut().push(ask.clone());
        Ok(Vec::new())
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        Box::new(TallyReader)
    }
}

/// Consecutive tests sharing their groups have their ids made once (a
/// scan of 2,000 tests must take a few milliseconds): each still gets its
/// own groups' id, each group goes to Fenix once, and the memory places a
/// listing's test where its scan did, whichever groups came before it.
#[test]
fn tests_that_share_their_groups_and_those_that_dont_are_each_placed() {
    let one = vec![Group::package("p"), Group::file("a.rs"), Group::suite("one", "a.rs", 1)];
    let two = vec![Group::package("p"), Group::file("a.rs"), Group::suite("two", "a.rs", 5)];
    let file = vec![Group::package("p"), Group::file("a.rs")];
    let found = vec![
        Found::test("x1", "x1").under(one.clone()).line(2),
        Found::test("x2", "x2").under(one.clone()).line(3),
        Found::test("y", "y").under(two.clone()).line(6),
        Found::test("x3", "x3").under(one.clone()).line(4),
        Found::test("z", "z").under(file.clone()).line(9),
    ];
    let asks = Rc::new(RefCell::new(Vec::new()));
    let mut kit = Kit::with_host("given", Box::new(Shared::default())).tests(Box::new(Given(found, asks.clone())));
    let answer = kit.tests_scan("given", "/project/a.rs", "");
    let parent = |id: &str| answer.iter().find(|f| f.id == id).and_then(|f| f.parent.clone());
    let one_id = group_id(&["p", "a.rs", "one"]);
    let two_id = group_id(&["p", "a.rs", "two"]);
    assert_eq!([parent("x1"), parent("x2"), parent("y"), parent("x3"), parent("z")], [Some(one_id.clone()), Some(one_id.clone()), Some(two_id.clone()), Some(one_id.clone()), Some(group_id(&["p", "a.rs"]))]);
    let groups: Vec<&str> = answer.iter().filter(|f| f.kind != Kind::Test).map(|f| f.id.as_str()).collect();
    assert_eq!(groups, [group_id(&["p"]), group_id(&["p", "a.rs"]), one_id.clone(), two_id.clone()], "each group once, before its first test");
    // What the memory has under each suite: asked with no fill-in.
    let file_id = group_id(&["p", "a.rs"]);
    kit.tests_plan("given", 1, api::Ask::Run(vec![rec(&one_id, Some(&file_id), "one", Kind::Suite), rec(&two_id, Some(&file_id), "two", Kind::Suite)])).unwrap();
    let known: Vec<Vec<Key>> = match &asks.borrow()[0] {
        Ask::Run(items) => items.iter().map(|a| if let Asked::Group { known, .. } = a { known.clone() } else { Vec::new() }).collect(),
        _ => Vec::new(),
    };
    assert_eq!(known, [vec![Key::from("x1"), Key::from("x2"), Key::from("x3")], vec![Key::from("y")]]);
}

/// What `test_exports!` writes is an `impl Extension`'s five methods, each
/// handed to the kit's own: through the trait, a provider the kit has is
/// scanned, and one it hasn't answers as `Kit` does.
#[test]
fn test_exports_forwards_the_five_methods_to_the_kit() {
    use fenix_extension_api::Extension;

    struct Pack {
        kit: Kit,
    }
    impl Default for Pack {
        fn default() -> Pack {
            let seen = Rc::new(RefCell::new(Seen::default()));
            let runner = Rc::new(RefCell::new((String::new(), None)));
            Pack { kit: Kit::with_host("tally", Box::new(Shared::default())).tests(Box::new(Tally { seen, runner })) }
        }
    }
    impl Extension for Pack {
        crate::test_exports!(kit);
    }

    let mut pack = Pack::default();
    let found = Extension::tests_scan(&mut pack, "tally", "/project/a.txt", "# test adds\n");
    assert!(found.iter().any(|f| f.label == "adds"), "{found:?}");
    assert!(Extension::tests_scan(&mut pack, "nobody", "/project/a.txt", "# test adds\n").is_empty());
    assert!(Extension::tests_plan(&mut pack, "nobody", 1, api::Ask::RunAll).is_err());
    assert!(Extension::tests_read(&mut pack, "nobody", 1, 0, vec![api::Line::default()]).is_empty());
    assert!(Extension::tests_step_ended(&mut pack, "nobody", 1, 0, api::StepEnd::Stopped).is_empty());
    assert!(Extension::tests_launch(&mut pack, "nobody", 1, api::Found::default()).is_err());
}
