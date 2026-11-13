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
    assert_eq!(ids, [("\u{1f}tally", None, Kind::Group), ("\u{1f}tally\u{1f}a.tally", Some("\u{1f}tally"), Kind::File), ("t adds", Some("\u{1f}tally\u{1f}a.tally"), Kind::Test)], "the second `adds` left out");
    assert_eq!((found[1].path.as_deref(), found[2].path.as_deref(), found[2].line), (Some("/project/a.tally"), None, Some(1)));
    assert_eq!(r.host.take(), ["log tests: tally: the scan of /project/a.tally found t adds twice; the second is left out"]);
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

#[test]
fn a_second_result_for_a_test_is_merged_and_outputs_joined() {
    let mut r = rig("merge");
    r.kit.tests_plan("tally", 1, api::Ask::RunAll).unwrap();
    let out = r.kit.tests_read("tally", 1, 0, ["FAIL adds at src/a.t:4: no", "PASS adds 2ms", "OUT adds one", "OUT adds two", "OUT other three"].map(line).to_vec());
    let api::Report::Ended(last) = &out[1] else { panic!("{out:?}") };
    assert_eq!((last.outcome, last.message.as_deref(), last.at.as_ref().map(|a| (a.path.as_str(), a.line))), (Outcome::Failed, Some("no"), Some(("/project/src/a.t", 3))));
    assert_eq!(out[2], api::Report::Output { id: Some("t adds".into()), text: "one\ntwo".into() });
    assert_eq!(out.len(), 4);
    let _ = std::fs::remove_dir_all(&r.data);
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

#[test]
fn an_ask_is_given_with_the_groups_and_tests_fenix_sent() {
    let mut r = rig("ask");
    let scanned = r.kit.tests_scan("tally", "/project/a.tally", "# test adds\n# test subs\n");
    let (file, adds) = (scanned[1].clone(), scanned[2].clone());
    // A test; a file, then the tests Fenix has under it; a file alone.
    r.kit.tests_plan("tally", 1, api::Ask::Run(vec![adds.clone(), file.clone(), adds, file])).unwrap();
    let asks = &r.seen.borrow().asks;
    let Ask::Run(items) = &asks[0] else { panic!() };
    assert_eq!(items.len(), 3);
    assert!(matches!(&items[0], Asked::Test(f) if f.groups.len() == 2 && f.key.0 == "t adds" && f.line == Some(1)));
    match &items[1] {
        Asked::Group { groups, tests, known } => {
            assert_eq!(groups.iter().map(|g| (g.kind, g.label.as_str())).collect::<Vec<_>>(), [(Kind::Group, "tally"), (Kind::File, "a.tally")]);
            assert_eq!((tests.len(), known.len()), (1, 0));
        }
        other => panic!("{other:?}"),
    }
    match &items[2] {
        Asked::Group { tests, known, .. } => assert_eq!((tests.len(), known.clone()), (0, vec![Key::from("t adds"), Key::from("t subs")])),
        other => panic!("{other:?}"),
    }
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
