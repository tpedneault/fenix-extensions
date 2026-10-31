//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its launch files by the same rules as
//! `fenix-gui`'s (`dap::Template`).

use std::path::{Path, PathBuf};

use fenix_project::template::{load_template_dir, Origin};
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

/// `fenix-gui`'s `dap::Template`.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    name: String,
    languages: Vec<String>,
    #[serde(default)]
    request: Option<String>,
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    attributes: toml::Table,
}

#[test]
fn every_snippet_reads_as_fenix_reads_it() {
    let snippets = files("snippets/go", "snippet");
    assert!(snippets.len() >= 20);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["go"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains("    "), "{file}: Go's indented with tabs");
        keys.push(s.trigger);
    }
    for key in ["iferr", "errw", "tt", "test", "hf", "hjson", "gof", "wg", "sel", "st", "meth", "new"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn every_launch_file_is_for_go_and_says_delves_mode() {
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 5);
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(!l.name.trim().is_empty() && l.languages == ["go"], "{file}");
        let mode = l.attributes.get("mode").and_then(toml::Value::as_str);
        match l.request.as_deref() {
            Some("attach") => {
                assert!(l.program.is_none(), "{file}");
                match mode {
                    Some("remote") => assert!(!l.attributes.contains_key("processId"), "{file}"),
                    // Delve's local attach needs the process.
                    Some("local") => assert_eq!(l.attributes["processId"].as_str(), Some("${pickProcess}"), "{file}"),
                    other => panic!("{file}: {other:?}"),
                }
            }
            None | Some("launch") => {
                // Delve builds a package: a folder, never the file.
                assert!(matches!(mode, Some("debug" | "test")), "{file}");
                assert!(matches!(l.program.as_deref(), Some("${project}" | "${fileDir}")), "{file}");
            }
            Some(other) => panic!("{file}: {other}"),
        }
    }
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
    comment: Option<String>,
}

#[test]
fn the_language_file_says_the_comment_only() {
    let l: Language = toml::from_str(include_str!("../languages/go.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.comment.as_deref()), ("go", Some("//")));
    assert!(l.extensions.is_empty() && l.server.is_none(), "the server is the code's to set");
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    for (id, opens) in [("go-cli", "main.go"), ("go-service", "internal/server/server.go"), ("go-library", "orbit_desk.go")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(plan.steps[0].display(), "go mod init example.com/orbit-desk", "{id}");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        assert!(all.contains(&opens), "{id}: {all:?}");
        assert!(all.contains(&".gitignore"), "{id}: {all:?}");
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
