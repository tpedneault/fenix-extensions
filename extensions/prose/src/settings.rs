//! What the pack's settings say, and what Harper is told of them.

use fenix_lang_kit::serde_json::{json, Value};

use crate::api;

/// The word list's place under the project, `prose.words`' default: with
/// the project's other Fenix files, where a project's own settings are.
pub const WORDS: &str = ".fenix/words.txt";

/// How seriously Harper's findings are taken (`diagnosticSeverity`): its
/// own words for them.
pub const SEVERITIES: &[&str] = &["error", "warning", "information", "hint"];

/// The dialects Harper spells in, as it names them.
pub const DIALECTS: &[&str] = &["American", "British", "Canadian", "Australian", "Indian"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `prose.enabled`: off, nothing is added for the project.
    pub enabled: bool,
    /// `prose.languages`: Fenix's names for the languages it reads.
    pub languages: Vec<String>,
    /// `prose.severity`.
    pub severity: String,
    /// `prose.dialect`.
    pub dialect: String,
    /// `prose.words`: the project's word list, under the project.
    pub words: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { enabled: true, languages: vec!["markdown".into()], severity: "information".into(), dialect: "American".into(), words: WORDS.into() }
    }
}

/// The names in a list setting: split at commas and spaces, lower-case,
/// each once.
pub fn names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in text.split([',', ' ', '\n', ';']).map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()) {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl Settings {
    /// From Fenix, each as its own default where it's missing or isn't
    /// one of the words it takes; the project's over yours, as Fenix
    /// gives them.
    pub fn read() -> Settings {
        let d = Settings::default();
        let get = |key: &str| api::setting(key).map(|v| v.trim().to_string());
        let severity = get("prose.severity").map(|s| s.to_lowercase()).filter(|s| SEVERITIES.contains(&s.as_str()));
        let dialect = get("prose.dialect").and_then(|s| DIALECTS.iter().find(|d| d.eq_ignore_ascii_case(&s)).map(|d| d.to_string()));
        Settings {
            enabled: !get("prose.enabled").is_some_and(|v| v.eq_ignore_ascii_case("false")),
            languages: get("prose.languages").map(|l| names(&l)).unwrap_or(d.languages),
            severity: severity.unwrap_or(d.severity),
            dialect: dialect.unwrap_or(d.dialect),
            words: get("prose.words").filter(|w| !w.is_empty()).unwrap_or(d.words),
        }
    }

    /// What Harper asks for when it starts and is told when they change
    /// (`workspace/configuration`, the section `harper-ls`): the severity,
    /// the dialect, and the project's word list as its workspace
    /// dictionary, a path under the project, which it resolves there.
    /// `revision` only makes these differ after the word list changed, so
    /// Fenix tells Harper, which reads the list again and checks open
    /// files at once; Harper ignores it.
    pub fn server_settings(&self, revision: u32) -> Value {
        let mut harper = json!({
            "diagnosticSeverity": self.severity,
            "dialect": self.dialect,
            "workspaceDictPath": self.words,
        });
        if revision > 0 {
            harper["fenixWordListRevision"] = json!(revision);
        }
        json!({ "harper-ls": harper })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::Fake;

    #[test]
    fn a_list_is_split_lower_cased_and_each_name_once() {
        assert_eq!(names("Markdown, html  rust;markdown\n"), ["markdown", "html", "rust"]);
        assert!(names("").is_empty() && names(" , ").is_empty());
    }

    #[test]
    fn what_isnt_set_is_the_default_and_what_isnt_a_word_it_takes_is_too() {
        let fake = Fake::new();
        fake.install();
        assert_eq!(Settings::read(), Settings::default());
        fake.setting("prose.enabled", "false").setting("prose.languages", "Markdown, rust").setting("prose.severity", "Warning").setting("prose.dialect", "british").setting("prose.words", "docs/words.txt");
        assert_eq!(Settings::read(), Settings { enabled: false, languages: vec!["markdown".into(), "rust".into()], severity: "warning".into(), dialect: "British".into(), words: "docs/words.txt".into() });
        fake.setting("prose.severity", "loud").setting("prose.dialect", "klingon").setting("prose.words", "").setting("prose.enabled", "true").setting("prose.languages", "");
        let s = Settings::read();
        assert_eq!((s.enabled, s.severity.as_str(), s.dialect.as_str(), s.words.as_str()), (true, "information", "American", WORDS));
        assert!(s.languages.is_empty(), "an empty list is none, not the default");
    }

    #[test]
    fn harper_is_told_its_section_and_the_list_changing_changes_it() {
        let s = Settings::default();
        let v = s.server_settings(0);
        assert_eq!(v["harper-ls"]["diagnosticSeverity"], "information");
        assert_eq!(v["harper-ls"]["dialect"], "American");
        assert_eq!(v["harper-ls"]["workspaceDictPath"], ".fenix/words.txt");
        assert!(v["harper-ls"].get("fenixWordListRevision").is_none());
        assert_ne!(s.server_settings(1), v);
        assert_eq!(s.server_settings(1)["harper-ls"]["fenixWordListRevision"], 1);
    }
}
