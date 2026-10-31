//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language files by the same rules as `fenix-gui`'s.

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

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let mut keys = Vec::new();
    for scope in ["yaml", "dockerfile", "bash", "markdown"] {
        let snippets = files(&format!("snippets/{scope}"), "snippet");
        assert!(snippets.len() >= 3, "{scope}");
        for (file, text) in &snippets {
            let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(s.scopes, [scope], "{file}");
            assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
            if scope == "yaml" {
                assert!(!text.contains('\t'), "{file}: YAML can't be indented with tabs");
            }
            let shown = s.template.render(&Default::default(), &Default::default(), "").text;
            assert_eq!(shown.matches("${").count(), text.matches("\\${").count(), "{file}: a field left: {shown}");
            keys.push(format!("{scope}/{}", s.trigger));
        }
    }
    for key in ["yaml/workflow", "yaml/compose", "yaml/deployment", "dockerfile/multistage", "bash/strict", "bash/args", "markdown/table"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_case_snippet_keeps_its_literal_dollar_one() {
    let s = fenix_snippets::Snippet::parse(include_str!("../snippets/bash/case.snippet")).unwrap();
    let shown = s.template.render(&Default::default(), &Default::default(), "").text;
    assert!(shown.contains("echo \"unknown: $1\" >&2"), "{shown}");
    assert!(shown.starts_with("case \"$1\" in"), "{shown}");
    let matrix = fenix_snippets::Snippet::parse(include_str!("../snippets/yaml/matrix.snippet")).unwrap();
    assert!(matrix.template.render(&Default::default(), &Default::default(), "").text.contains("runs-on: ${{ matrix.os }}"));
}

/// `fenix-gui`'s `languages::File`.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Language {
    language: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    filenames: Vec<String>,
    server: Option<String>,
    grammar: Option<String>,
    comment: Option<String>,
}

#[test]
fn the_language_files_add_files_to_fenixs_own_languages() {
    let languages = files("languages", "toml");
    assert_eq!(languages.len(), 5);
    for (file, text) in &languages {
        let l: Language = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(format!("{}.toml", l.language), *file);
        assert!(["yaml", "json", "toml", "bash", "markdown"].contains(&l.language.as_str()), "{file}: one of Fenix's");
        assert!(l.server.is_none() && l.grammar.is_none() && l.comment.is_none(), "{file}: the server is the code's to set, the rest is Fenix's");
        assert!(!l.extensions.is_empty() || !l.filenames.is_empty(), "{file}");
    }
}
