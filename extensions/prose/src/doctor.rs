//! The project doctor's checks (`SPC p h`): Harper found and asked its
//! version, with a fix row that installs the release for this machine, and
//! a row saying what the pack is checking.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{Fix, Program};

use crate::settings::Settings;
use crate::tools::{self, Platform};

/// The command that installs Harper (`prose.install.<name>`, the kit's).
pub const INSTALL: &str = "prose.install.harper";

/// Harper's check: optional, so a machine without it is warned, not
/// failed; with a fix row where there's a release for the machine.
pub fn programs(platform: &Platform) -> Vec<Program> {
    let mut harper = Program::new(tools::PROGRAM, "Harper (grammar and spelling)").optional();
    for hint in tools::hints() {
        harper = harper.hint(hint);
    }
    if platform.install().is_some() {
        harper = harper.fix(&format!("Install Harper {}", tools::HARPER), INSTALL, Fix::Install(tools::SERVER.into()));
    }
    vec![harper]
}

/// What the pack is doing with Harper, found at `path` or not, for the
/// settings: a row of its own, so it's clear why a file shows no
/// findings.
pub fn row(settings: &Settings, path: Option<&str>) -> Check {
    let (health, detail) = if !settings.enabled {
        (Health::Warn, "off for this project (prose.enabled)".to_string())
    } else if settings.languages.is_empty() {
        (Health::Warn, "no language named (prose.languages)".to_string())
    } else if path.is_none() {
        (Health::Warn, format!("{} wait for Harper to be installed", settings.languages.join(", ")))
    } else {
        (Health::Ok, format!("{}; words in {}", settings.languages.join(", "), settings.words))
    };
    Check::new(Section::Toolchain, "Prose checking", health, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::Output;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn harper_is_asked_its_version_and_a_missing_one_is_a_warning_with_its_fix() {
        let p = programs(&Platform::new("linux", "x86_64"));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].name.as_str(), p[0].version_args.as_slice()), ("harper-ls", ["--version".to_string()].as_slice()));
        assert!(p[0].hints.contains(&"/tools/harper".to_string()));
        let found = p[0].evaluate(Some("/tools/harper/harper-ls"), Some(&said("harper-ls 3.2.0\n")));
        assert_eq!(found.health, Health::Ok);
        assert!(found.detail.contains("3.2.0"), "{}", found.detail);
        let missing = p[0].evaluate(None, None);
        assert_eq!(missing.health, Health::Warn);
        assert_eq!(missing.fix.as_ref().map(|f| f.1.as_str()), Some("prose.install.harper"));
        assert!(found.fix.is_none(), "nothing to fix");
    }

    #[test]
    fn a_machine_without_a_release_has_no_fix_row() {
        let p = programs(&Platform::new("windows", "aarch64"));
        assert!(p[0].fix.is_none());
        assert!(p[0].evaluate(None, None).fix.is_none());
    }

    #[test]
    fn the_row_says_why_nothing_is_checked_or_what_is() {
        let s = Settings::default();
        let ok = row(&s, Some("/t/harper-ls"));
        assert_eq!((ok.health, ok.detail.as_str()), (Health::Ok, "markdown; words in .fenix/words.txt"));
        assert_eq!(row(&s, None).health, Health::Warn);
        assert_eq!(row(&Settings { enabled: false, ..s.clone() }, Some("/t/h")).detail, "off for this project (prose.enabled)");
        assert_eq!(row(&Settings { languages: vec![], ..s }, Some("/t/h")).detail, "no language named (prose.languages)");
    }
}
