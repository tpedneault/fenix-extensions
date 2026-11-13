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

/// The kit's install, ended, is a notification on the one fake: what
/// failed, a way to try again, tagged so the next takes its place -- in
/// the words lang-kit's mock writes too.
#[test]
fn an_install_that_failed_is_a_notification_on_the_fake() {
    const URL: &str = "https://example.com/srv-1.0.tar.gz";
    let tools = std::env::temp_dir().join(format!("fenix-test-kit-install-{}", std::process::id()));
    let fake = Fake::new();
    fake.on_http_error(URL, "its SHA-256 isn't abc");
    let mut kit = Kit::with_host("demo", fake.host()).install(fenix_lang_kit::Install::new("srv", URL).label("Srv"));
    kit.installer.tools = tools.to_string_lossy().into_owned();
    assert_eq!(kit.run_command("demo.install.srv"), Some(Ok(())));
    assert_eq!(fake.notifications(), ["Downloading Srv…"], "starting is the message line's");
    while let Some(e) = fake.next_event() {
        kit.handle(&e);
    }
    let shown = fake.showing();
    assert_eq!((shown.len(), shown[0].urgency, shown[0].tag.as_deref()), (1, api::Urgency::Normal, Some("install.srv")));
    let line = fake.take().pop().unwrap();
    assert_eq!(line, format!("notification normal #install.srv: Couldn't install Srv -- Its download from {URL} failed: its SHA-256 isn't abc. [Try again -> demo.install.srv]"));
    assert_eq!(line, fenix_lang_kit::notification_line(&shown[0]), "the fake's words are the mock's");

    // Tried again: the old toast goes as it starts.
    kit.run_command("demo.install.srv").unwrap().unwrap();
    assert_eq!(fake.dismissed(), ["install.srv"]);
    let _ = std::fs::remove_dir_all(&tools);
}
