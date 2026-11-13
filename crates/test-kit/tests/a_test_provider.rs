//! A test provider for a made-up runner, `tally`, through lang-kit's
//! adapter and the driver: every rule of the adapter and of Fenix's side
//! the driver keeps.
//!
//! `tally`'s files say their tests in comments: `# test adds` (`skip=why`
//! after it), `# cases table` (a template, its cases found by running it),
//! `# suite deep` to `# end`. `tally list` lists them (`LIST <key>`), with
//! no line, and `gen_one`, which a macro makes and no scan sees. `tally
//! run [file] [keys]` prints a record a test: `PASS <key> <ms>`, `SKIP <key>
//! <why>`, or `FAIL <key>` then `MSG`, `| text` and `AT file:line:col`
//! lines and `END`; `START <key>` as one starts, `OUT <key> <text>`,
//! `FOUND <key>` for a test that exists only once run, `BROKEN
//! <file>:<line>: <why>` for a file that doesn't build and `CRASH <why>`
//! when it dies. `tally build` prints `BUILT <path>`.

#![cfg(feature = "tests")]

use fenix_extension_api::{Command, LaunchConfig};
use fenix_lang_kit::tests::{api, command_line_len, split, Ask, Asked, At, Covered, Ended, Found, Group, Key, Kind, Launching, Line, Outcome, Reader, Report, Step, StepEnd, TestRunner, DIRECT, WINDOWS_MAX};
use fenix_lang_kit::Kit;
use fenix_test_kit::tests::{Fenix, Order, Recorded, State, NO_RESULT};
use fenix_test_kit::{assert_snapshot, Fake};

const MATH: &str = "\
# test adds
# test fails
# test slow skip=too slow
# suite deep
# test inner
# end
# cases table
";

const OTHER: &str = "# test one\n# test two\n";

/// The file a key is in: what's before its first `::`.
fn file_of(key: &str) -> &str {
    key.split("::").next().unwrap_or(key)
}

fn groups(file: &str) -> Vec<Group> {
    vec![Group::package("tally"), Group::file(file)]
}

struct Tally;

impl TestRunner for Tally {
    fn provider(&self) -> &str {
        "tally"
    }

    fn runner(&mut self) -> (String, Option<String>) {
        ("tally".into(), None)
    }

    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path);
        let mut out = Vec::new();
        let mut suite: Option<(String, u32)> = None;
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let n = i as u32 + 1;
            let Some(rest) = l.strip_prefix("# ") else { continue };
            let (word, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            let (name, skip) = match rest.split_once(" skip=") {
                Some((name, why)) => (name, Some(why)),
                None => (rest, None),
            };
            let mut under = groups(file);
            let mut key = format!("{file}::{name}");
            if let Some((s, at)) = &suite {
                let end = lines[i..].iter().position(|l| *l == "# end").map(|p| (i + p) as u32 + 1);
                under.push(Group::suite(s.clone(), file, *at).lines(*at, end));
                key = format!("{file}::{s}::{name}");
            }
            match word {
                "suite" => suite = Some((name.to_string(), n)),
                "end" => suite = None,
                "test" | "cases" => {
                    let mut f = Found::test(key, name).under(under).line(n);
                    if let Some(why) = skip {
                        f = f.skip(why);
                    }
                    if word == "cases" {
                        f = f.template();
                    }
                    out.push(f);
                }
                _ => {}
            }
        }
        out
    }

    fn plan(&mut self, ask: &Ask) -> Result<Vec<Step>, String> {
        let tally = || Command::new("tally").args(["--reporter", "${data}/lang-kit/tests/tally.rep"]);
        let items = match ask {
            Ask::Discover => return Ok(vec![Step::new(tally().arg("list"), "listing")]),
            Ask::RunAll => return Ok(vec![Step::new(tally().arg("run"), "all of it")]),
            Ask::Debug(_) => return Err("a build-first plan is the launch's".into()),
            Ask::Run(items) => items,
        };
        // A step per file: a file asked whole by its path, covering its
        // group; tests by key, split under the command line's limit.
        let mut steps = Vec::new();
        let mut keys: Vec<(String, Vec<String>)> = Vec::new();
        for item in items {
            match item {
                Asked::Group { groups, .. } if groups.last().is_some_and(|g| g.kind == Kind::File) => {
                    let labels: Vec<String> = groups.iter().map(|g| g.label.clone()).collect();
                    let file = labels.last().cloned().unwrap_or_default();
                    steps.push(Step::new(tally().args(["run", &file]), file).covers([Covered::Group(labels)]));
                }
                Asked::Group { tests, known, .. } => {
                    // Its fill-in, whole, names them exactly; with none,
                    // what lang-kit remembers under it.
                    let names: Vec<String> = match tests {
                        Some(tests) => tests.iter().map(|t| t.key.0.clone()).collect(),
                        None => known.iter().map(|k| k.0.clone()).collect(),
                    };
                    for n in names {
                        push_key(&mut keys, n);
                    }
                }
                Asked::Test(f) => push_key(&mut keys, f.key.0.clone()),
            }
        }
        for (file, names) in keys {
            for cmd in split(&names, DIRECT, |some| tally().args(["run", &file]).args(some.iter().cloned())) {
                let covered: Vec<String> = cmd.args[4..].to_vec();
                steps.push(Step::new(cmd, file.clone()).covering(covered));
            }
        }
        Ok(steps)
    }

    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        Box::new(TallyReader::default())
    }

    fn launch(&mut self, test: &Found, kept: &[(String, String)]) -> Result<Launching, String> {
        match kept.iter().find(|(name, _)| name == "binary") {
            Some((_, binary)) => Ok(Launching::Now(LaunchConfig::launch(&format!("Tally: {}", test.key), &["tally"]).program(binary).arg(test.key.0.clone()))),
            None => Ok(Launching::BuildFirst(vec![Step::new(Command::new("tally").args(["build", file_of(&test.key.0)]), "building")])),
        }
    }

    fn assets(&self) -> Vec<(&str, &[u8])> {
        vec![("tally.rep", b"tally's reporter\n")]
    }
}

fn push_key(keys: &mut Vec<(String, Vec<String>)>, key: String) {
    let file = file_of(&key).to_string();
    match keys.iter_mut().find(|(f, _)| *f == file) {
        Some((_, names)) => names.push(key),
        None => keys.push((file, vec![key])),
    }
}

/// Reads `tally`'s records: a failure is held until its `END`, so it's
/// whole when it's reported.
#[derive(Default)]
struct TallyReader {
    current: Option<String>,
    failing: Option<Ended>,
}

impl Reader for TallyReader {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        if line.stderr {
            return;
        }
        let (word, rest) = line.text.split_once(' ').unwrap_or((line.text, ""));
        if let Some(e) = &mut self.failing {
            match word {
                "MSG" => e.message = Some(rest.into()),
                "|" => {
                    let text = e.text.get_or_insert_with(String::new);
                    text.push_str(rest);
                    text.push('\n');
                }
                "AT" => {
                    let mut parts = rest.rsplitn(3, ':');
                    let (column, line, file) = (parts.next(), parts.next(), parts.next());
                    if let (Some(file), Some(Ok(line))) = (file, line.map(str::parse)) {
                        e.at = Some(At::new(file, line, column.and_then(|c| c.parse().ok())));
                    }
                }
                "END" => {
                    out.extend(self.failing.take().map(Report::Ended));
                    self.current = None;
                }
                _ => {}
            }
            return;
        }
        let (key, after) = rest.split_once(' ').unwrap_or((rest, ""));
        match word {
            "LIST" => {
                let label = key.rsplit("::").next().unwrap_or(key);
                // The listing knows only the file: lang-kit puts what a
                // scan found back under the scan's groups, at its line.
                out.push(Report::Found(Found::test(key, label).under(groups(file_of(key)))));
            }
            "FOUND" => out.push(Report::Found(Found::test(key, key.rsplit("::").next().unwrap_or(key)).under(groups(file_of(key))))),
            "START" => {
                self.current = Some(key.into());
                out.push(Report::Started(key.into()));
            }
            "PASS" => {
                out.push(Report::Ended(Ended::new(key, Outcome::Passed).ms(after.parse().unwrap_or(0.0))));
                self.current = None;
            }
            "SKIP" => out.push(Report::Ended(Ended::new(key, Outcome::Skipped).message(after))),
            "FAIL" => self.failing = Some(Ended::new(key, Outcome::Failed)),
            "OUT" => out.push(Report::Output { key: Some(key.into()), text: after.into() }),
            "BUILT" => out.push(Report::Keep { name: "binary".into(), value: key.into() }),
            "BROKEN" => {
                let (place, why) = rest.split_once(": ").unwrap_or((rest, ""));
                let (file, line) = place.rsplit_once(':').unwrap_or((place, "1"));
                out.push(Report::Broken { under: Some(vec!["tally".into(), file.into()]), message: why.into(), at: Some(At::new(file, line.parse().unwrap_or(1), None)) });
            }
            "CRASH" => {
                let under = self.current.take().map(|k| vec!["tally".to_string(), file_of(&k).to_string()]);
                out.push(Report::Crashed { under, message: rest.into() });
            }
            _ => {}
        }
    }

    fn end(&mut self, _end: &StepEnd, out: &mut Vec<Report>) {
        out.extend(self.failing.take().map(Report::Ended));
    }
}

fn pack(fake: &Fake) -> Kit {
    Kit::with_host("tally", fake.host()).tests(Box::new(Tally))
}

fn fenix() -> (Fenix<Kit>, Fake) {
    let fake = Fake::new();
    let mut fenix = Fenix::new(pack(&fake));
    fenix.programs(&["tally"]).with_fake(fake.clone());
    fenix.scan("tally", "/project/src/math.tally", MATH);
    (fenix, fake)
}

const ADDS: &str = "src/math.tally::adds";
const FAILS: &str = "src/math.tally::fails";
const SLOW: &str = "src/math.tally::slow";
const INNER: &str = "src/math.tally::deep::inner";
const TABLE: &str = "src/math.tally::table";
const FILE: &str = "\u{1f}tally\u{1f}src/math.tally";

const RUN_ALL: &str = "\
START src/math.tally::adds
OUT src/math.tally::adds two and two
PASS src/math.tally::adds 3
START src/math.tally::fails
FAIL src/math.tally::fails
MSG expected 4, got 5
| left: 4
| right: 5
AT src/math.tally:2:3
END
SKIP src/math.tally::slow too slow
PASS src/math.tally::deep::inner 1
PASS src/math.tally::table[1] 1
FAIL src/math.tally::table[2]
MSG two isn't one
END
FOUND src/math.tally::dynamic
PASS src/math.tally::dynamic 2
PASS src/math.tally::gen_two 1
";

#[test]
fn a_scan_finds_tests_at_their_lines_under_their_groups() {
    let (fenix, fake) = fenix();
    assert_eq!(fenix.told("tally"), Some(("tally".into(), None)), "told once, as it started");
    assert_eq!(fake.take().iter().filter(|c| c.starts_with("test-provider")).count(), 1);
    let adds = fenix.record("tally", ADDS).unwrap();
    assert_eq!((adds.line, adds.path.as_deref(), adds.parent.as_deref()), (Some(0), Some("src/math.tally"), Some(FILE)));
    assert_eq!(fenix.record("tally", SLOW).unwrap().skip.as_deref(), Some("too slow"));
    let deep = fenix.record("tally", &format!("{FILE}\u{1f}deep")).unwrap();
    assert_eq!((deep.kind, deep.line, deep.end_line), (Kind::Suite, Some(3), Some(5)));
    assert!(std::fs::read(fenix.data_dir().join("lang-kit/tests/tally.rep")).is_ok(), "the reporter written as it started");
    assert_snapshot!("tally-scan", fenix.tree("tally"));
}

#[test]
fn a_listing_adds_what_the_scan_missed_and_keeps_the_scans_lines() {
    let (mut fenix, _) = fenix();
    fenix.on_command("tally --reporter ${data}/lang-kit/tests/tally.rep list", Recorded::stdout(format!("LIST {INNER}\nLIST {ADDS}\nLIST src/math.tally::gen_one\n")));
    let run = fenix.discover("tally");
    assert!(run.complete, "{run:?}");
    assert_eq!(run.steps[0].needs, ["/data/lang-kit/tests/tally.rep"]);
    let inner = fenix.record("tally", INNER).unwrap();
    assert_eq!((inner.line, inner.parent.as_deref()), (Some(4), Some(format!("{FILE}\u{1f}deep").as_str())), "under its suite, at its line");
    let gen = fenix.record("tally", "src/math.tally::gen_one").unwrap();
    assert_eq!((gen.line, gen.parent.as_deref()), (None, Some(FILE)));
    assert_snapshot!("tally-listed", fenix.tree("tally"));

    // A listing that isn't complete drops nothing; a complete one drops
    // what it didn't give.
    fenix.on_command("list", Recorded::stdout(format!("LIST {ADDS}\nBROKEN src/math.tally:9: not today\n")));
    assert!(!fenix.discover("tally").complete);
    assert!(fenix.record("tally", "src/math.tally::gen_one").is_some());
    fenix.on_command("list", Recorded::stdout(format!("LIST {ADDS}\n")).exits(5));
    assert!(fenix.discover("tally").complete, "any exit code");
    assert!(fenix.record("tally", "src/math.tally::gen_one").is_none(), "only the listing had it");
    assert_eq!(fenix.record("tally", INNER).unwrap().line, Some(4), "the scan still has it");
}

#[test]
fn a_run_of_all_gives_each_test_its_result_and_its_place() {
    let (mut fenix, _) = fenix();
    // Two lines a call: records are cut between calls.
    fenix.batch = 2;
    fenix.on_command("run", Recorded::stdout(RUN_ALL).exits(1));
    let run = fenix.run_all("tally");
    assert_eq!(run.steps.len(), 1);
    assert_eq!(run.steps[0].calls, RUN_ALL.lines().count().div_ceil(2));
    fenix.assert_states(
        "tally",
        &[(ADDS, State::Passed), (FAILS, State::Failed), (SLOW, State::Skipped), (INNER, State::Passed), (TABLE, State::Failed), ("src/math.tally::table[1]", State::Passed), ("src/math.tally::dynamic", State::Passed), ("src/math.tally::gen_two", State::Passed)],
    );
    let fails = fenix.detail("tally", FAILS);
    assert_eq!((fails.message.as_deref(), fails.text.as_deref()), (Some("expected 4, got 5"), Some("left: 4\nright: 5\n")));
    assert_eq!(fenix.at("tally", FAILS), Some(("src/math.tally".into(), 2, Some(3))));
    // A template's cases go under it; it needs no result of its own: its
    // state is its cases' (Fenix §19, K7).
    assert_eq!(fenix.record("tally", "src/math.tally::table[2]").unwrap().parent.as_deref(), Some(TABLE));
    assert_eq!(fenix.detail("tally", TABLE), Default::default(), "no result made for it");
    assert!(!fenix.model("tally").is_leaf(TABLE), "not counted once its cases are found");
    assert_eq!(fenix.model("tally").nodes[ADDS].printed, "two and two", "as Fenix keeps it: what the reader gave");
    assert_snapshot!("tally-run-all", fenix.tree("tally"));
}

#[test]
fn the_same_run_shuffled_and_a_line_a_call_ends_the_same() {
    let (mut fenix, _) = fenix();
    let noise: String = (0..40).map(|i| format!("warming up {i}\n")).collect();
    fenix.on_command("run", Recorded::stdout(RUN_ALL).and_stderr(noise).exits(1));
    fenix.run_all("tally");
    let once = fenix.tree("tally");
    for seed in [1, 2, 3] {
        let (mut again, _) = self::fenix();
        again.batch = 1 + seed as usize;
        again.order = Order::Shuffled(seed);
        again.on_command("run", Recorded::stdout(RUN_ALL).and_stderr((0..40).map(|i| format!("warming up {i}\n")).collect::<String>()).exits(1));
        again.run_all("tally");
        assert_eq!(again.tree("tally"), once, "seed {seed}");
    }
}

#[test]
fn a_plan_of_two_steps_judges_each_as_it_ends() {
    let (mut fenix, _) = fenix();
    fenix.scan("tally", "/project/src/other.tally", OTHER);
    // The math step says nothing of `fails`; the other step passes both.
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\n")));
    fenix.on_command("run src/other.tally", Recorded::stdout("PASS src/other.tally::one 1\nPASS src/other.tally::two 1\n"));
    let run = fenix.run("tally", &[FILE, "\u{1f}tally\u{1f}src/other.tally"]);
    let covers: Vec<&[String]> = run.steps.iter().map(|s| s.covers.as_slice()).collect();
    assert_eq!(covers, [[FILE.to_string()].as_slice(), ["\u{1f}tally\u{1f}src/other.tally".to_string()].as_slice()], "a file's group stands for what's under it");
    fenix.assert_states("tally", &[(ADDS, State::Passed), (FAILS, State::Errored), ("src/other.tally::one", State::Passed), ("src/other.tally::two", State::Passed)]);
    assert_eq!(fenix.detail("tally", FAILS).message.as_deref(), Some(NO_RESULT));
}

#[test]
fn a_selection_asks_for_the_tests_under_a_file_and_runs_them_by_key() {
    let (mut fenix, _) = fenix();
    let api::Ask::Run(records) = fenix.ask_for("tally", &[INNER, &format!("{FILE}\u{1f}deep")]) else { panic!() };
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [&format!("{FILE}\u{1f}deep"), INNER], "the topmost, a suite, then its fill-in");
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {INNER} 1\nPASS {ADDS} 1\n")));
    let run = fenix.run("tally", &[INNER, ADDS]);
    assert_eq!(run.steps.len(), 1);
    assert_eq!(run.steps[0].covers, [ADDS, INNER], "in the page's order");
    fenix.assert_states("tally", &[(INNER, State::Passed), (ADDS, State::Passed), (FAILS, State::Unknown)]);
}

#[test]
fn a_cut_line_is_joined_before_the_reader_sees_it() {
    let (mut fenix, _) = fenix();
    let said = "x".repeat(150 * 1024);
    fenix.on_command("run", Recorded::stdout(format!("OUT {ADDS} {said}\nPASS {ADDS} 3\n")));
    let run = fenix.run("tally", &[ADDS]);
    assert_eq!(run.steps[0].lines, 4, "three pieces and a line");
    assert_eq!(fenix.model("tally").nodes[ADDS].printed.len(), 64 * 1024, "whole to the reader; Fenix keeps the end");
    assert_eq!(fenix.state("tally", ADDS), State::Passed);

    // Every line in pieces of 7 bytes.
    let (mut small, _) = self::fenix();
    small.cut_at = 7;
    small.on_command("run", Recorded::stdout(RUN_ALL).exits(1));
    small.run_all("tally");
    let (mut whole, _) = self::fenix();
    whole.on_command("run", Recorded::stdout(RUN_ALL).exits(1));
    whole.run_all("tally");
    assert_eq!(small.tree("tally"), whole.tree("tally"));
}

#[test]
fn a_file_that_doesnt_build_is_broken_not_a_failure() {
    let (mut fenix, _) = fenix();
    fenix.scan("tally", "/project/src/other.tally", OTHER);
    fenix.on_command("run src/other.tally", Recorded::stdout("BROKEN src/other.tally:2: no such word `tow`\n").exits(2));
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\n")));
    let run = fenix.run("tally", &["\u{1f}tally\u{1f}src/other.tally", ADDS]);
    assert_eq!(run.broken, [(Some("\u{1f}tally\u{1f}src/other.tally".into()), "no such word `tow`".into())]);
    assert!(!run.complete);
    fenix.assert_states("tally", &[("src/other.tally::one", State::Errored), ("src/other.tally::two", State::Errored), (ADDS, State::Passed)]);
    assert_eq!(fenix.detail("tally", "src/other.tally::one").message.as_deref(), Some("no such word `tow`"));
    assert!(fenix.failed("tally").iter().all(|id| fenix.state("tally", id) == State::Errored), "nothing failed");
}

#[test]
fn a_crash_errors_what_its_process_held_and_no_more() {
    let (mut fenix, _) = fenix();
    fenix.scan("tally", "/project/src/other.tally", OTHER);
    // Inside a test: what of its file hasn't ended in this step.
    fenix.on_command("run", Recorded::stdout(format!("PASS {ADDS} 1\nSTART {FAILS}\nCRASH signal 6\nPASS src/other.tally::one 1\nPASS src/other.tally::two 1\n")).signal());
    let run = fenix.run_all("tally");
    assert_eq!(run.crashed, [(Some(FILE.into()), "signal 6".into())]);
    fenix.assert_states("tally", &[(ADDS, State::Passed), (FAILS, State::Errored), (INNER, State::Errored), ("src/other.tally::one", State::Passed)]);
    assert_eq!(fenix.detail("tally", INNER).message.as_deref(), Some("signal 6"));

    // Outside any test, in a step that claims nothing: what of the plan
    // hasn't ended.
    fenix.on_command("run", Recorded::stdout(format!("PASS {ADDS} 1\nCRASH out of memory\n")).signal());
    fenix.run_all("tally");
    fenix.assert_states("tally", &[(ADDS, State::Passed), (FAILS, State::Errored), ("src/other.tally::two", State::Errored)]);
    assert_eq!(fenix.detail("tally", "src/other.tally::two").message.as_deref(), Some("out of memory"));
}

#[test]
fn two_thousand_failed_tests_run_again_in_steps_under_the_limit() {
    let fake = Fake::new();
    let mut fenix = Fenix::new(pack(&fake));
    fenix.programs(&["tally"]);
    // 2,000 tests in four files, each failing.
    let mut all = String::new();
    for f in 0..4 {
        let text: String = (0..500).map(|i| format!("# test fails_on_purpose_number_{i:05}\n")).collect();
        fenix.scan("tally", &format!("/project/src/many{f}.tally"), &text);
        all.extend((0..500).map(|i| format!("FAIL src/many{f}.tally::fails_on_purpose_number_{i:05}\nEND\n")));
    }
    fenix.on_command("run", Recorded::stdout(all).exits(1));
    fenix.run_all("tally");
    assert_eq!(fenix.failed("tally").len(), 2000);
    // Again: each step passes the keys its command names.
    fenix.on_step_with(|s| (s.command.args.len() > 4).then(|| Recorded::stdout(s.command.args[4..].iter().map(|k| format!("PASS {k} 1\n")).collect::<String>())));
    let run = fenix.run_failed("tally");
    assert!(run.steps.len() > 1, "{} steps", run.steps.len());
    assert!(run.steps.iter().all(|s| DIRECT.fits(&s.command) && command_line_len(&s.command) < WINDOWS_MAX));
    assert_eq!(run.steps.iter().map(|s| s.covers.len()).sum::<usize>(), 2000);
    assert!(fenix.failed("tally").is_empty());
    assert!(fenix.slowest() < fenix.budget);
}

#[test]
fn debugging_builds_first_then_launches_what_was_built() {
    let (mut fenix, _) = fenix();
    fenix.on_command("tally build", Recorded::stdout("compiling\nBUILT /project/.tally/bin/math\n"));
    let launched = fenix.launch("tally", INNER);
    let config = launched.config.unwrap();
    assert_eq!((config.program.as_deref(), config.args.as_slice()), (Some("/project/.tally/bin/math"), [INNER.to_string()].as_slice()));
    assert_eq!(launched.built.unwrap().steps[0].label, "building");

    // A build that broke: no launch, and the head says why.
    fenix.on_command("tally build", Recorded::stdout("BROKEN src/math.tally:4: no `deep` here\n").exits(1));
    let launched = fenix.launch("tally", INNER);
    assert_eq!(launched.config, Err("no `deep` here".into()));
}

#[test]
fn the_pack_starting_again_mid_run_stops_the_run() {
    let (mut fenix, _) = fenix();
    let fake = Fake::new();
    fenix.on_restart(move || pack(&fake));
    fenix.on_command("run", Recorded::stdout(RUN_ALL).exits(1));
    fenix.run_all("tally");
    let before = fenix.tree("tally");
    // Started again after two lines: the run is stopped, its tests back to
    // what they were, the new pack asked nothing of it.
    fenix.on_command("run", Recorded::stdout(RUN_ALL).restarts_after(2));
    let run = fenix.run_all("tally");
    assert_eq!(run.stopped.as_deref(), Some("stopped: tally started again"));
    assert_eq!(fenix.tree("tally"), before);
    // The new pack, which scanned nothing yet, plans and reads a run of its
    // own.
    fenix.on_command("run", Recorded::stdout(format!("PASS {ADDS} 1\n")));
    fenix.run("tally", &[ADDS]);
    assert_eq!(fenix.state("tally", ADDS), State::Passed);
}

/// A runner that plans what it likes, for the driver's own checks: Fenix
/// fails a step it wouldn't start, and says why.
struct Careless(Vec<Step>);

impl TestRunner for Careless {
    fn provider(&self) -> &str {
        "careless"
    }
    fn runner(&mut self) -> (String, Option<String>) {
        ("careless".into(), None)
    }
    fn scan(&mut self, _path: &str, _text: &str) -> Vec<Found> {
        vec![Found::test("one", "one").under([Group::package("careless")]).line(1)]
    }
    fn plan(&mut self, _ask: &Ask) -> Result<Vec<Step>, String> {
        Ok(self.0.clone())
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        Box::new(TallyReader::default())
    }
}

#[test]
fn a_step_fenix_wouldnt_start_fails_and_is_said() {
    let fake = Fake::new();
    let steps = vec![
        Step::new(Command::new("tally").arg("x".repeat(40_000)), "too long"),
        Step::new(Command::new("sh").args(["-c", "tally"]), "a shell"),
        Step::new(Command::new("tally"), "needs").needs("/data/nowhere.txt"),
    ];
    let mut fenix = Fenix::new(Kit::with_host("careless", fake.host()).tests(Box::new(Careless(steps))));
    fenix.programs(&["tally"]).lenient();
    fenix.scan("careless", "/project/a.c", "");
    let run = fenix.run_all("careless");
    let ends: Vec<&StepEnd> = run.steps.iter().map(|s| &s.end).collect();
    assert!(matches!(ends[0], StepEnd::Failed(w) if w.contains("Windows takes 32,766")), "{ends:?}");
    assert!(matches!(ends[1], StepEnd::Failed(w) if w.starts_with("sh isn't a program")));
    assert_eq!(ends[2], &StepEnd::Failed("needs /data/nowhere.txt, which isn't there".into()));
    assert_eq!(fenix.said().len(), 3);
    assert_eq!(fenix.state("careless", "one"), State::Errored);

    // Strict, as by default, it fails the test.
    let mut strict = Fenix::new(Kit::with_host("careless", fake.host()).tests(Box::new(Careless(vec![Step::new(Command::new("sh"), "a shell")]))));
    strict.programs(&["tally"]);
    strict.scan("careless", "/project/a.c", "");
    let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strict.run_all("careless"))).unwrap_err();
    assert!(said.downcast_ref::<String>().is_some_and(|s| s.contains("against Fenix's rules: sh isn't a program")));
}

#[test]
fn the_keys_and_lines_of_the_api_are_as_fenix_takes_them() {
    let (fenix, _) = fenix();
    let found = fenix.ask_for("tally", &[ADDS]);
    let api::Ask::Run(records) = found else { panic!() };
    let api::Found { id, parent, line, path, .. } = &records[0];
    assert_eq!((id.as_str(), parent.as_deref(), *line, path.as_deref()), (ADDS, Some(FILE), Some(0), Some("/project/src/math.tally")));
    let _ = Key::from(ADDS);
}

#[test]
fn a_stop_puts_back_what_hadnt_ended_and_a_timeout_errors_it() {
    let (mut fenix, _) = fenix();
    fenix.scan("tally", "/project/src/other.tally", OTHER);
    let other = "\u{1f}tally\u{1f}src/other.tally";
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\nPASS {INNER} 1\n")).stopped_after(1));
    fenix.on_command("run src/other.tally", Recorded::stdout("PASS src/other.tally::one 1\n"));
    let run = fenix.run("tally", &[FILE, other]);
    assert_eq!(run.stopped.as_deref(), Some("stopped"));
    assert_eq!(run.steps.iter().map(|s| (s.end.clone(), s.lines)).collect::<Vec<_>>(), [(StepEnd::Stopped, 1), (StepEnd::Stopped, 0)], "the second never started");
    fenix.assert_states("tally", &[(ADDS, State::Passed), (INNER, State::Unknown), ("src/other.tally::one", State::Unknown)]);

    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\n")).timed_out_after(1));
    fenix.run("tally", &[FILE, other]);
    fenix.assert_states("tally", &[(ADDS, State::Passed), (INNER, State::Errored), ("src/other.tally::one", State::Errored)]);
    assert_eq!(fenix.detail("tally", INNER).message.as_deref(), Some("timed out after 30 min"));

    // A file with unsaved edits: what the run didn't see goes back.
    fenix.unsaved("/project/src/math.tally");
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\n")));
    fenix.run("tally", &[FILE]);
    assert_eq!((fenix.state("tally", FAILS), fenix.detail("tally", FAILS).message.as_deref()), (State::Errored, Some("timed out after 30 min")), "as it was before");
}

#[test]
fn a_cases_record_goes_when_its_template_runs_again_without_it() {
    let (mut fenix, _) = fenix();
    fenix.on_command("run", Recorded::stdout(RUN_ALL).exits(1));
    fenix.run_all("tally");
    assert!(fenix.record("tally", "src/math.tally::table[2]").is_some());
    // The template again, its second case gone from the runner's output.
    fenix.on_command("run src/math.tally", Recorded::stdout("PASS src/math.tally::table[1] 1\n"));
    let run = fenix.run("tally", &[TABLE]);
    assert!(run.complete);
    assert!(fenix.record("tally", "src/math.tally::table[1]").is_some());
    assert!(fenix.record("tally", "src/math.tally::table[2]").is_none(), "not named by a complete run of its template");
    assert!(fenix.record("tally", "src/math.tally::dynamic").is_some(), "its group didn't run");
    assert_eq!(fenix.state("tally", TABLE), State::Passed);
}

/// Fenix §19 finding 2: after Fenix starts again a node's `data` is gone
/// until found again, and a pack plans from its key alone.
#[test]
fn without_data_the_pack_plans_from_its_keys() {
    let (mut fenix, _) = fenix();
    fenix.forget_data("tally");
    let api::Ask::Run(records) = fenix.ask_for("tally", &[ADDS]) else { panic!() };
    assert!(records.iter().all(|r| r.data.is_none()));
    fenix.on_command("run src/math.tally", Recorded::stdout(format!("PASS {ADDS} 1\n")));
    let run = fenix.run("tally", &[ADDS]);
    assert_eq!(run.steps[0].covers, [ADDS]);
    assert_eq!(fenix.state("tally", ADDS), State::Passed);
}

/// A runner with nothing to run in the project (`("", None)`): Fenix
/// doesn't ask it (Fenix §6.3 `test-provider-set`).
struct Nothing;

impl TestRunner for Nothing {
    fn provider(&self) -> &str {
        "nothing"
    }
    fn runner(&mut self) -> (String, Option<String>) {
        (String::new(), None)
    }
    fn scan(&mut self, _path: &str, _text: &str) -> Vec<Found> {
        vec![Found::test("one", "one")]
    }
    fn plan(&mut self, _ask: &Ask) -> Result<Vec<Step>, String> {
        panic!("asked to plan")
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        Box::new(TallyReader::default())
    }
}

#[test]
fn a_provider_with_nothing_to_run_isnt_asked() {
    let fake = Fake::new();
    let mut fenix = Fenix::new(Kit::with_host("nothing", fake.host()).tests(Box::new(Nothing)));
    fenix.programs(&[]).with_fake(fake);
    assert_eq!(fenix.told("nothing"), Some((String::new(), None)));
    let run = fenix.run_all("nothing");
    assert!(run.steps.is_empty() && !run.complete);
    assert!(fenix.said().iter().any(|s| s.contains("nothing to run")));
}
