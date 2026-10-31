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
    #[serde(default, rename = "preLaunchTask")]
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
    let snippets = files("snippets/python", "snippet");
    assert!(snippets.len() >= 20);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["python"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        keys.push(s.trigger);
    }
    for key in ["cls", "dc", "main", "test", "fixture", "param", "cm", "acm", "adef", "amain"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn every_launch_file_is_for_python_and_says_what_debugpy_runs() {
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 9);
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(!l.name.trim().is_empty() && l.languages == ["python"], "{file}");
        match l.request.as_deref() {
            // To a port (debugpy --listen's adapter, `debugServer`, Fenix
            // 2.1.1), or to a process picked.
            Some("attach") => assert!((l.attributes.get("debugServer").and_then(|p| p.as_integer()).is_some() || l.attributes.get("processId").and_then(|p| p.as_str()) == Some("${pickProcess}")) && l.program.is_none(), "{file}"),
            None | Some("launch") => {
                // debugpy's `module` with no program, which Fenix then
                // doesn't send (0.8.5); or the file you're in.
                if l.attributes.contains_key("module") {
                    assert!(l.program.is_none() && !l.attributes.contains_key("debugpyArgs"), "{file}");
                }
            }
            Some(other) => panic!("{file}: {other}"),
        }
    }
    let flask: Launch = toml::from_str(&launches.iter().find(|(f, _)| f == "flask.toml").unwrap().1).unwrap();
    assert_eq!((flask.attributes["module"].as_str(), flask.args[..2].to_vec()), (Some("flask"), vec!["--app".to_string(), "${file}".into()]));
}

#[test]
fn the_language_file_adds_file_types_only() {
    let l: Language = toml::from_str(include_str!("../languages/python.toml")).unwrap();
    assert_eq!(l.language, "python");
    assert!(l.extensions.contains(&"pyw".to_string()) && l.server.is_none(), "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("#"));
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    for (id, opens) in [("python-django", "pages/views.py"), ("python-flask", "src/orbit_desk/views.py")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(plan.steps[0].display().split(' ').take(2).collect::<Vec<_>>(), ["uv", "init"], "{id}");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        assert!(all.contains(&opens), "{id}: {all:?}");
        assert!(all.contains(&".env") && !all.contains(&"env"), "{id}: {all:?}");
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
