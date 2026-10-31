//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its launch files and language file by
//! the same rules as `fenix-gui`'s (`dap::Template`, `languages::File`).

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
    #[serde(rename = "preLaunchTask")]
    pre_launch_task: Option<String>,
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
fn every_snippet_reads_as_fenix_reads_it() {
    let snippets = files("snippets/csharp", "snippet");
    assert!(snippets.len() >= 25);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["csharp"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        keys.push(s.trigger);
    }
    for key in ["cls", "rec", "prop", "propf", "am", "linq", "ws", "fact", "theory", "test", "tcase", "fixture"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_launch_file_attaches_to_a_process_picked() {
    // The programs' launches are the pack's, one for each (`debug.rs`).
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 1);
    let l: Launch = toml::from_str(&launches[0].1).unwrap();
    assert!(l.languages == ["csharp"] && l.request.as_deref() == Some("attach") && l.program.is_none());
    assert_eq!(l.attributes["processId"].as_str(), Some("${pickProcess}"), "netcoredbg's attach takes its id");
}

#[test]
fn the_language_file_adds_file_types_only() {
    let l: Language = toml::from_str(include_str!("../languages/csharp.toml")).unwrap();
    assert_eq!(l.language, "csharp");
    assert!(l.extensions.contains(&"cake".to_string()) && l.server.is_none(), "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("//"));
}

#[test]
fn each_project_template_plans_dotnet_new() {
    let parent = crate::project::tests::temp("templates");
    for (id, kind, opens) in [
        ("dotnet-console", "console", "src/OrbitDesk/Program.cs"),
        ("dotnet-classlib", "classlib", "src/OrbitDesk/Class1.cs"),
        ("dotnet-webapi", "webapi", "src/OrbitDesk/Program.cs"),
        ("dotnet-worker", "worker", "src/OrbitDesk/Worker.cs"),
        ("dotnet-xunit", "xunit", "UnitTest1.cs"),
    ] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(plan.open[0], opens, "{id}");
        let steps: Vec<String> = plan.steps.iter().map(|s| s.display()).collect();
        for s in &steps {
            assert!(s.starts_with("dotnet ") && !s.contains("{{") && !s.contains(" -f "), "{id}: {s}: the SDK's framework by default");
        }
        assert!(steps.iter().any(|s| s.starts_with(&format!("dotnet new {kind} -n OrbitDesk"))), "{id}: {steps:?}");
        if id != "dotnet-xunit" {
            assert_eq!(steps[0], "dotnet new sln -n OrbitDesk", "{id}");
            assert!(steps.iter().any(|s| s == "dotnet add tests/OrbitDesk.Tests reference src/OrbitDesk"), "{id}: tests by default");
            // Without tests, for .NET 8.
            let mut answers = answers.clone();
            answers.insert("tests".into(), fenix_project::template::Answer::Bool(false));
            answers.insert("framework".into(), fenix_project::template::Answer::Text("net8.0".into()));
            let plan = t.plan("orbit-desk", &parent, &answers).unwrap();
            let steps: Vec<String> = plan.steps.iter().map(|s| s.display()).collect();
            assert!(!steps.iter().any(|s| s.contains("Tests")), "{id}: {steps:?}");
            assert!(steps.contains(&format!("dotnet new {kind} -n OrbitDesk -o src/OrbitDesk -f net8.0")), "{id}: {steps:?}");
        }
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
