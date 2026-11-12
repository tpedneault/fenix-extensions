//! A language pack's kit and its own code on the one fake: the kit's
//! linter run on a saved file, its output canned, the problems set; and
//! the pack's own setting read through `api`.

#![cfg(feature = "lang-kit")]

use fenix_lang_kit::{command, compiler_lines, Kit, Linter};
use fenix_test_kit::{api, Fake, Harness, Ran};

use api::{Event, Extension, Severity};

struct Pack(Kit);

impl Default for Pack {
    fn default() -> Pack {
        let kit = Kit::with_host("demo", Fake::current().host());
        let on = api::setting("demo.lint").as_deref() != Some("false");
        let kit = if on { kit.linter(Linter::new("demo-lint", &["demo"], |f| command(&format!("demo-lint {f}")), |o, f| Ok(compiler_lines(&o.stderr, f, Severity::Warning)))) } else { kit };
        Pack(kit)
    }
}

impl Extension for Pack {
    fn activate(&mut self) -> Result<(), String> {
        self.0.activate()
    }
    fn handle(&mut self, event: Event) {
        self.0.handle(&event);
    }
    fn file_saved(&mut self, path: &str) {
        self.0.file_saved(path);
    }
}

#[test]
fn a_saved_file_is_linted_and_its_problems_set() {
    let fake = Fake::new();
    fake.on_spawn("demo-lint src/main.demo", Ran::fails(1, "src/main.demo:3:5: error: no such name `x`"));
    let mut h: Harness<Pack> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    h.saved("/project/src/main.demo");
    assert_eq!(fake.take(), ["doctor 0", "spawn 1 demo-lint src/main.demo", "close 1", "problems /project/src/main.demo 1"]);
    let problems = fake.problems("/project/src/main.demo");
    assert_eq!((problems[0].start, problems[0].severity, problems[0].message.as_str()), ((2, 4), Severity::Error, "no such name `x`"));
    assert_eq!(fake.settings_read(), ["demo.lint"], "the pack's own call, on the same fake");
}

#[test]
fn a_setting_read_as_the_pack_is_made() {
    let fake = Fake::new();
    fake.setting("demo.lint", "false");
    let mut h: Harness<Pack> = Harness::with_fake(fake.clone());
    h.saved("/project/src/main.demo");
    assert!(fake.spawned().is_empty(), "off: no linter");
}
