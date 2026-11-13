//! The pack on the test kit's fake host: what it adds to Fenix, and when.

use super::*;
use api::{Health, Urgency};
use fenix_test_kit::{Fake, Harness, Ran};

/// A project folder of its own.
fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fenix-prose-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The pack for the project in `dir` on `platform`, its tools folder in
/// `dir` too. The fake is this thread's before the pack reads its
/// settings.
fn pack(dir: &std::path::Path, fake: &Fake, platform: Platform) -> Harness<Prose> {
    fake.install();
    let mut p = Prose::new(fake.host(), &dir.to_string_lossy(), platform);
    p.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
    Harness::wrap(fake.clone(), p)
}

fn linux() -> Platform {
    Platform::new("linux", "x86_64")
}

/// Harper found, saying its version.
fn installed() -> Fake {
    let fake = Fake::new();
    fake.program("harper-ls", "/t/harper-ls").on_spawn("/t/harper-ls --version", Ran::ok("harper-ls 3.2.0\n"));
    fake
}

/// The server and settings lines among `calls`.
fn set(calls: &[String]) -> Vec<String> {
    calls.iter().filter(|c| c.starts_with("add ") || c.starts_with("settings ")).cloned().collect()
}

#[test]
fn activating_adds_harper_for_markdown_with_its_settings_and_checks_it() {
    let dir = temp("activate");
    let fake = installed();
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    let calls = fake.take();
    let lines = set(&calls);
    assert_eq!(lines[0], "add harper markdown: /t/harper-ls --stdio");
    assert!(lines[1].starts_with("settings harper: "), "{lines:?}");
    let settings = fake.server_settings("harper").unwrap();
    assert_eq!(settings["harper-ls"]["diagnosticSeverity"], "information");
    assert_eq!(settings["harper-ls"]["dialect"], "American");
    assert_eq!(settings["harper-ls"]["workspaceDictPath"], ".fenix/words.txt");
    assert_eq!(lines.len(), 2, "once each: {calls:?}");

    let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
    assert_eq!(checks[0], ("Harper (grammar and spelling)".into(), Health::Ok, "3.2.0 (/t/harper-ls)".into()));
    assert_eq!(checks[1], ("Prose checking".into(), Health::Ok, "markdown; words in .fenix/words.txt".into()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn without_harper_nothing_is_added_and_the_fix_row_installs_it() {
    let dir = temp("missing");
    let fake = Fake::new();
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    let calls = fake.take();
    assert!(set(&calls).is_empty(), "nothing found, nothing added: {calls:?}");
    let checks = fake.checks();
    assert_eq!((checks[0].health, checks[0].fix.as_ref().map(|f| f.1.clone())), (Health::Warn, Some("prose.install.harper".into())));
    assert_eq!((checks[1].label.as_str(), checks[1].health), ("Prose checking", Health::Warn));

    h.command("prose.install.harper").unwrap();
    assert_eq!(fake.notifications(), ["Downloading Harper 3.2.0…"]);
    let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
    assert!(download.contains("https://github.com/Automattic/harper/releases/download/v3.2.0/harper-ls-x86_64-unknown-linux-musl.tar.gz"), "{download}");
    assert!(download.ends_with(" 4b44c54ff6a0831901805584bb9d70a004b768b713af9725b65d13e1b369c138"), "checked by its SHA-256: {download}");

    // Put there and checked again, as the doctor's `r` does: found, added.
    fake.program("harper-ls", "/tools/harper/harper-ls");
    h.command("prose.doctor").unwrap();
    let calls = fake.take();
    assert_eq!(set(&calls)[0], "add harper markdown: /tools/harper/harper-ls --stdio", "{calls:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn each_platforms_install_is_its_release_and_checksum() {
    let expected = [
        (("linux", "x86_64"), "harper-ls-x86_64-unknown-linux-musl.tar.gz", "4b44c54ff6a0831901805584bb9d70a004b768b713af9725b65d13e1b369c138"),
        (("linux", "aarch64"), "harper-ls-aarch64-unknown-linux-gnu.tar.gz", "e2a13cd9bb91b505ef46bcbcafeb4f11b96d95e75b9cc0a156c680474c476804"),
        (("macos", "x86_64"), "harper-ls-x86_64-apple-darwin.tar.gz", "3ca19166a4ffef14c24b0396af920bbb11abd071e6724bfedbe1f2eccad3b76d"),
        (("macos", "aarch64"), "harper-ls-aarch64-apple-darwin.tar.gz", "9dd31b0cabfb040f658a8d595cc36520eb306a5a69ac37c7b828c36574b47ce9"),
        (("windows", "x86_64"), "harper-ls-x86_64-pc-windows-msvc.zip", "a9e370166ace4e92a592f954baa2fedf4528e6daf59e64a9973fc9f3d7d30d5e"),
    ];
    for ((os, arch), file, sha) in expected {
        let dir = temp(&format!("plan-{os}-{arch}"));
        let fake = Fake::new();
        let mut h = pack(&dir, &fake, Platform::new(os, arch));
        h.activate().unwrap();
        let fix = fake.checks()[0].fix.clone().unwrap_or_else(|| panic!("{os} {arch}: no fix row"));
        assert_eq!(fix.1, "prose.install.harper");
        h.command(&fix.1).unwrap();
        let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap_or_else(|| panic!("{os} {arch}: no download"));
        assert!(download.contains(&format!("/releases/download/v3.2.0/{file}")) && download.ends_with(&format!(" {sha}")), "{os} {arch}: {download}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // No build for it: no fix row, and the command says so.
    let dir = temp("plan-none");
    let fake = Fake::new();
    let mut h = pack(&dir, &fake, Platform::new("windows", "aarch64"));
    h.activate().unwrap();
    assert!(fake.checks()[0].fix.is_none());
    assert!(h.command("prose.install.harper").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_install_is_a_notification_that_offers_it_again() {
    let dir = temp("install-told");
    let fake = Fake::new();
    fake.on_http_error("https://*", "its SHA-256 isn't the release's");
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    h.command("prose.install.harper").unwrap();
    let shown = fake.showing();
    assert_eq!(shown.len(), 1, "{shown:?}");
    let n = &shown[0];
    assert_eq!((n.summary.as_str(), n.urgency, n.tag.as_deref()), ("Couldn't install Harper 3.2.0", Urgency::Normal, Some("install.harper")));
    assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "prose.install.harper"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn it_is_added_for_exactly_the_languages_named() {
    let dir = temp("languages");
    let fake = installed();
    fake.setting("prose.languages", "markdown, rust,Python, markdown");
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    assert_eq!(set(&fake.take())[0], "add harper markdown,rust,python: /t/harper-ls --stdio");

    // Changed: added again by its name, for the new set.
    h.set_setting("prose.languages", Some("html"));
    assert_eq!(set(&fake.take())[0], "add harper html: /t/harper-ls --stdio");

    // None named: taken away, its settings first.
    h.set_setting("prose.languages", Some(""));
    let lines = set(&fake.take());
    assert_eq!(lines, ["settings harper: ", "add harper : "], "the settings go before the server");
    assert_eq!(fake.server_settings("harper"), None);
    assert_eq!(fake.checks().last().unwrap().detail, "no language named (prose.languages)");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_setting_changed_is_set_again_where_it_changed() {
    let dir = temp("settings");
    let fake = installed();
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    fake.take();

    // Severity and dialect: Harper's settings only, not the server.
    h.set_setting("prose.severity", Some("warning"));
    let lines = set(&fake.take());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("settings harper: "));
    assert_eq!(fake.server_settings("harper").unwrap()["harper-ls"]["diagnosticSeverity"], "warning");
    h.set_setting("prose.dialect", Some("British"));
    assert_eq!(fake.server_settings("harper").unwrap()["harper-ls"]["dialect"], "British");
    h.set_setting("prose.words", Some("docs/words.txt"));
    assert_eq!(fake.server_settings("harper").unwrap()["harper-ls"]["workspaceDictPath"], "docs/words.txt");
    fake.take();

    // Another setting of Fenix's, or one that changes nothing: nothing set.
    h.event(Event::SettingChanged("editor.format_on_save".into()));
    h.set_setting("prose.severity", Some("warning"));
    assert!(set(&fake.take()).is_empty());

    // The switch for the project: off takes both away, on gives them back.
    h.set_setting("prose.enabled", Some("false"));
    assert_eq!(set(&fake.take()), ["settings harper: ", "add harper : "]);
    assert_eq!(fake.checks().last().unwrap().detail, "off for this project (prose.enabled)");
    h.set_setting("prose.enabled", Some("true"));
    let lines = set(&fake.take());
    assert_eq!((lines.len(), lines[0].as_str()), (2, "add harper markdown: /t/harper-ls --stdio"));
    assert_eq!(fake.server_settings("harper").unwrap()["harper-ls"]["dialect"], "British", "as the settings now say");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn off_from_the_start_adds_nothing() {
    let dir = temp("off");
    let fake = installed();
    fake.setting("prose.enabled", "false");
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    assert!(set(&fake.take()).is_empty());
    assert!(h.command("prose.doctor").is_ok());
    assert!(h.command("prose.nothing").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_word_under_the_cursor_goes_in_the_list_where_harper_reads_it() {
    let dir = temp("words");
    let fake = installed();
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    fake.take();
    let file = format!("{}/notes.md", dir.display());
    fake.in_file(&file, "md", "Run Fenixx on the wasmtime host.\nSecond line, Zig.\n").project("p", &dir.to_string_lossy());
    fake.cursor(0, 6);
    h.command("prose.add-word").unwrap();
    let list = dir.join(".fenix/words.txt");
    assert_eq!(std::fs::read_to_string(&list).unwrap(), "Fenixx\n");
    assert_eq!(fake.notifications(), ["Added \"Fenixx\" to .fenix/words.txt"]);

    // Harper is told the list changed: its settings differ, so Fenix tells it.
    let lines = set(&fake.take());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let settings = fake.server_settings("harper").unwrap();
    assert_eq!((settings["harper-ls"]["workspaceDictPath"].as_str(), settings["harper-ls"]["fenixWordListRevision"].as_u64()), (Some(".fenix/words.txt"), Some(1)));

    // A selection; the list stays sorted; a word again is said so.
    fake.selection((0, 18), (0, 26));
    h.command("prose.add-word").unwrap();
    assert_eq!(std::fs::read_to_string(&list).unwrap(), "Fenixx\nwasmtime\n");
    fake.cursor(1, 14);
    h.command("prose.add-word").unwrap();
    assert_eq!(std::fs::read_to_string(&list).unwrap(), "Fenixx\nZig\nwasmtime\n");
    fake.take();
    h.command("prose.add-word").unwrap();
    assert_eq!(fake.notifications().last().unwrap(), "\"Zig\" is in .fenix/words.txt already");
    assert!(set(&fake.take()).is_empty(), "nothing changed, nothing told");

    // Not on a word, or a selection of several: refused, nothing written.
    fake.cursor(1, 12);
    assert_eq!(h.command("prose.add-word"), Err("the cursor isn't on a word".into()));
    fake.selection((0, 0), (0, 10));
    assert!(h.command("prose.add-word").unwrap_err().contains("isn't one word"));
    assert_eq!(std::fs::read_to_string(&list).unwrap(), "Fenixx\nZig\nwasmtime\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_list_is_the_one_the_setting_names_and_never_outside_the_project() {
    let dir = temp("words-path");
    let fake = installed();
    fake.setting("prose.words", "docs/spelling.txt");
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    fake.in_file(&format!("{}/a.md", dir.display()), "md", "Harperish\n");
    fake.cursor(0, 2);
    h.command("prose.add-word").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("docs/spelling.txt")).unwrap(), "Harperish\n");
    assert_eq!(fake.server_settings("harper").unwrap()["harper-ls"]["workspaceDictPath"], "docs/spelling.txt");

    h.set_setting("prose.words", Some("/etc/words.txt"));
    assert!(h.command("prose.add-word").unwrap_err().contains("a path under the project"));
    assert!(h.command("prose.words").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_list_is_opened_and_made_if_it_isnt_there() {
    let dir = temp("words-open");
    let fake = installed();
    let mut h = pack(&dir, &fake, linux());
    h.activate().unwrap();
    fake.take();
    h.command("prose.words").unwrap();
    assert!(dir.join(".fenix/words.txt").exists());
    let calls = fake.take();
    assert!(calls.iter().any(|c| c == "open /project/.fenix/words.txt"), "{calls:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}
