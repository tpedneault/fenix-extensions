//! The pack's data, read as Fenix reads it: its manifest against the code
//! and the keymap, its settings against what the code reads.

use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn manifest() -> toml::Table {
    toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap()
}

/// The ids of the manifest's commands.
fn commands() -> Vec<String> {
    manifest()["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_manifest_lists_the_commands_the_kit_the_code_and_the_keymap_use() {
    let commands = commands();
    for own in ["prose.doctor", "prose.install.harper", "prose.add-word", "prose.words", crate::doctor::INSTALL] {
        assert!(commands.iter().any(|c| c == own), "{own}: {commands:?}");
    }
    let keymap: toml::Table = toml::from_str(&std::fs::read_to_string(dir("keymaps/prose.toml")).unwrap()).unwrap();
    let mut keys = Vec::new();
    for (_, menu) in keymap["local"].as_table().unwrap() {
        for key in menu.as_array().unwrap() {
            let command = key["command"].as_str().unwrap();
            assert!(commands.iter().any(|c| c == command), "{command} isn't in extension.toml");
            keys.push(key["keys"].as_str().unwrap().to_string());
        }
    }
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 3, "each key once");
}

#[test]
fn the_manifest_names_the_programs_and_hosts_the_pack_uses() {
    let m = manifest();
    let process: Vec<&str> = m["permissions"]["process"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
    assert!(process.contains(&crate::tools::PROGRAM) && process.contains(&"/tools/*"), "{process:?}");
    let network: Vec<&str> = m["permissions"]["network"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
    assert!(network.contains(&"github.com") && network.contains(&"release-assets.githubusercontent.com"), "{network:?}");
    assert_eq!(m["permissions"]["filesystem"].as_str(), Some("project-write"), "the word list is written");
    assert_eq!(m["permissions"]["editor"].as_str(), Some("read"), "the word under the cursor");
    assert_eq!(m["extension"]["version"].as_str(), Some("1.0.0"));
    assert_eq!(m["engine"]["api"].as_str(), Some("1.5"));
}

#[test]
fn the_settings_are_the_ones_the_code_reads_with_the_defaults_it_has() {
    let m = manifest();
    let settings = m["contributes"]["settings"].as_array().unwrap();
    let get = |key: &str| settings.iter().find(|s| s["key"].as_str() == Some(key)).unwrap_or_else(|| panic!("{key}"));
    let d = crate::settings::Settings::default();
    assert_eq!(get("prose.enabled")["default"].as_str(), Some("true"));
    assert_eq!(get("prose.languages")["default"].as_str(), Some(d.languages.join(",").as_str()));
    assert_eq!(get("prose.severity")["default"].as_str(), Some(d.severity.as_str()));
    assert_eq!(get("prose.dialect")["default"].as_str(), Some(d.dialect.as_str()));
    assert_eq!(get("prose.words")["default"].as_str(), Some(d.words.as_str()));
    let choices = |key: &str| -> Vec<String> { get(key)["choices"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect() };
    assert_eq!(choices("prose.severity"), crate::settings::SEVERITIES);
    assert_eq!(choices("prose.dialect"), crate::settings::DIALECTS);
    for s in settings {
        assert_eq!(s["project"].as_bool(), Some(true), "{:?}: each can be set for one project", s["key"]);
    }
}
