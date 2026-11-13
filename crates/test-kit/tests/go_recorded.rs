//! Real output through the adapter: `go test -json ./...` as Go 1.24
//! printed it for the sample project (`tests/samples/go/`, recorded for
//! `plans/test-explorer.md`), read by a reader made for this test only --
//! the Go pack's own is its piece of work. What it checks is lang-kit's
//! side and the driver's: the same results whatever the batches, however
//! the lines are cut, and subtests put under their test by their keys.

#![cfg(feature = "lang-kit")]

use fenix_extension_api::Command;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::tests::{Ask, Ended, Found, Group, Line, Outcome, Reader, Report, Step, TestRunner};
use fenix_lang_kit::Kit;
use fenix_test_kit::tests::{Fenix, Order, Recorded, State};
use fenix_test_kit::{assert_snapshot, Fake};

const MODULE: &str = "example.com/orbit";

struct GoTest;

impl TestRunner for GoTest {
    fn provider(&self) -> &str {
        "go"
    }
    fn runner(&mut self) -> (String, Option<String>) {
        ("go test".into(), None)
    }
    /// `func TestX(` and `func ExampleX(` at a line's start; one whose
    /// body calls `t.Run(` is a template.
    fn scan(&mut self, path: &str, text: &str) -> Vec<Found> {
        let file = path.strip_prefix("/project/").unwrap_or(path);
        let package = match file.rsplit_once('/') {
            Some((dir, _)) => format!("{MODULE}/{dir}"),
            None => MODULE.to_string(),
        };
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            let Some(name) = l.strip_prefix("func ").and_then(|r| r.split_once('(')).map(|(n, _)| n) else { continue };
            if !(name.starts_with("Test") || name.starts_with("Example")) {
                continue;
            }
            let body = lines[i + 1..].iter().take_while(|l| !l.starts_with("func "));
            let mut f = Found::test(format!("{package} {name}"), name).under([Group::package(package.clone()), Group::file(file)]).line(i as u32 + 1);
            if body.clone().any(|l| l.contains("t.Run(")) {
                f = f.template();
            }
            out.push(f);
        }
        out
    }
    fn plan(&mut self, _ask: &Ask) -> Result<Vec<Step>, String> {
        Ok(vec![Step::new(Command::new("go").args(["test", "-json", "./..."]), "./...")])
    }
    fn reader(&mut self, _ask: &Ask, _step: &Step) -> Box<dyn Reader> {
        Box::new(Events::default())
    }
}

/// `go test -json`'s events: a result per test, its last `file:line:`
/// output line as its message.
#[derive(Default)]
struct Events {
    said: std::collections::HashMap<String, String>,
}

impl Reader for Events {
    fn line(&mut self, line: Line, out: &mut Vec<Report>) {
        let Ok(e) = fenix_lang_kit::serde_json::from_str::<Value>(line.text) else { return };
        let (Some(action), Some(package), Some(test)) = (e["Action"].as_str(), e["Package"].as_str(), e["Test"].as_str()) else { return };
        let key = format!("{package} {test}");
        let outcome = match action {
            "run" => return out.push(Report::Started(key.into())),
            "output" => {
                let text = e["Output"].as_str().unwrap_or("").trim();
                if text.contains(".go:") {
                    self.said.insert(key, text.to_string());
                }
                return;
            }
            "pass" => Outcome::Passed,
            "fail" => Outcome::Failed,
            "skip" => Outcome::Skipped,
            _ => return,
        };
        let mut ended = Ended::new(key.clone(), outcome).ms(e["Elapsed"].as_f64().unwrap_or(0.0) * 1000.0);
        if let Some(said) = self.said.remove(&key) {
            ended = ended.message(said);
        }
        out.push(Report::Ended(ended));
    }
}

fn fenix() -> Fenix<Kit> {
    let fake = Fake::new();
    let mut fenix = Fenix::new(Kit::with_host("go", fake.host()).tests(Box::new(GoTest)));
    fenix.programs(&["go", "dlv"]).on_recorded(Recorded::from("tests/samples/go/run-json"));
    fenix.scan("go", "/project/orbit_test.go", include_str!("samples/go/orbit_test.go"));
    fenix.scan("go", "/project/geo/geo_test.go", include_str!("samples/go/geo/geo_test.go"));
    fenix
}

#[test]
fn a_recorded_go_test_run_ends_as_it_did() {
    let mut fenix = fenix();
    let run = fenix.run_all("go");
    assert_eq!((run.steps[0].end.clone(), run.steps[0].calls, run.steps[0].lines), (fenix_lang_kit::tests::StepEnd::Exited(Some(1)), 1, 79));
    fenix.assert_states(
        "go",
        &[
            ("example.com/orbit TestAdds", State::Passed),
            ("example.com/orbit TestFailsOnPurpose", State::Failed),
            ("example.com/orbit TestSkipped", State::Skipped),
            ("example.com/orbit TestTable", State::Failed),
            ("example.com/orbit TestTable/negative_one", State::Failed),
            ("example.com/orbit TestTable/a/b", State::Passed),
            ("example.com/orbit TestNested/outer/inner", State::Passed),
            ("example.com/orbit ExampleAdd", State::Passed),
            ("example.com/orbit/geo TestDist", State::Passed),
            ("example.com/orbit/geo TestPanics", State::Failed),
        ],
    );
    let record = fenix.record("go", "example.com/orbit TestTable/negative_one").unwrap();
    assert_eq!((record.parent.as_deref(), record.label.as_str()), (Some("example.com/orbit TestTable"), "negative_one"), "a subtest under its test, by its key");
    assert_eq!(fenix.detail("go", "example.com/orbit TestTable/negative_one").message.as_deref(), Some("orbit_test.go:37: got -2, want -3"));
    assert_snapshot!("go-run-json", fenix.tree("go"));
}

#[test]
fn the_same_lines_cut_small_in_small_batches_and_shuffled_end_the_same() {
    let mut whole = fenix();
    whole.run_all("go");
    for (cut, batch, seed) in [(40, 3, 1), (17, 1, 2), (200, 7, 3)] {
        let mut small = fenix();
        small.cut_at = cut;
        small.batch = batch;
        small.order = Order::Shuffled(seed);
        let run = small.run_all("go");
        assert!(run.steps[0].lines > 79, "cut into pieces: {}", run.steps[0].lines);
        assert_eq!(small.tree("go"), whole.tree("go"), "cut at {cut}, {batch} a call");
    }
}
