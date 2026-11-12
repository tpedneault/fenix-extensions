//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language file by the same rules as `fenix-gui`'s, and its
//! manifest against the keymap and the code.

use std::path::{Path, PathBuf};

use serde::Deserialize;

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn files(name: &str, ext: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir(name))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    out.sort();
    out
}

/// `fenix-gui`'s `languages::File`: a field it doesn't know is refused.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageFile {
    language: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    filenames: Vec<String>,
    server: Option<String>,
    grammar: Option<String>,
    symbol: Option<String>,
    queries: Option<String>,
    comment: Option<String>,
}

#[test]
fn the_language_file_reads_as_fenix_reads_it() {
    let files = files("languages", "toml");
    assert!(!files.is_empty(), "a pack has a language file");
    for (file, text) in &files {
        let l: LanguageFile = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {}", e.message()));
        assert!(!l.language.trim().is_empty(), "{file}: a language");
        assert!(!l.extensions.is_empty() || !l.filenames.is_empty(), "{file}: the files it reads");
    }
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let language: LanguageFile = toml::from_str(&files("languages", "toml")[0].1).unwrap();
    let snippets = files("snippets/example", "snippet");
    assert!(!snippets.is_empty());
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, [language.language.as_str()], "{file}: for the pack's language");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains('$'), "{file}: a field left: {shown}");
    }
}

/// The ids of the manifest's commands.
fn commands() -> Vec<String> {
    let manifest: toml::Table = toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap();
    let list = manifest["contributes"]["commands"].as_array().unwrap();
    list.iter().map(|c| c["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_manifest_lists_the_commands_the_kit_and_the_keymap_use() {
    let commands = commands();
    assert!(commands.contains(&format!("{}.doctor", crate::PREFIX)), "the kit's own: {commands:?}");
    for (file, text) in files("keymaps", "toml") {
        let keymap: toml::Table = toml::from_str(&text).unwrap();
        for (_, menu) in keymap["local"].as_table().unwrap() {
            for key in menu.as_array().unwrap() {
                let command = key["command"].as_str().unwrap();
                assert!(commands.iter().any(|c| c == command), "{file}: {command} isn't in extension.toml");
            }
        }
    }
}

#[test]
fn the_manifest_names_the_program_the_pack_runs() {
    let manifest: toml::Table = toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap();
    let process: Vec<&str> = manifest["permissions"]["process"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
    assert!(process.contains(&crate::doctor::TOOL), "permissions.process names {}", crate::doctor::TOOL);
}
