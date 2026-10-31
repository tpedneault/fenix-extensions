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
    for (language, at_least, keys) in [
        ("cpp", 18, &["guard", "once", "cls", "raii", "forr", "main", "mainv", "ns", "uptr", "test"][..]),
        ("c", 6, &["guard", "main", "mainv", "fori", "st"][..]),
    ] {
        let snippets = files(&format!("snippets/{language}"), "snippet");
        assert!(snippets.len() >= at_least, "{language}: {}", snippets.len());
        let mut found = Vec::new();
        for (file, text) in &snippets {
            let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(s.scopes, [language], "{file}");
            assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
            assert!(!["header", "section"].contains(&s.trigger.as_str()), "Fenix has {}", s.trigger);
            found.push(s.trigger);
        }
        for key in keys {
            assert!(found.iter().any(|k| k == key), "{language}: {key}");
        }
    }
}

#[test]
fn the_rule_of_five_has_all_five() {
    let cls = std::fs::read_to_string(dir("snippets/cpp/cls.snippet")).unwrap();
    for member in ["$1(const $1& other);", "$1($1&& other) noexcept;", "$1& operator=(const $1& other);", "$1& operator=($1&& other) noexcept;", "~$1();"] {
        assert!(cls.contains(member), "{member}");
    }
}

#[test]
fn the_language_file_adds_file_types_only() {
    let l: Language = toml::from_str(include_str!("../languages/cpp.toml")).unwrap();
    assert_eq!(l.language, "cpp");
    assert!(l.extensions.contains(&"cppm".to_string()) && l.server.is_none(), "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("//"));
}

#[test]
fn the_launch_files_pick_a_target_or_a_process() {
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 2);
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(l.languages, ["c", "cpp"], "{file}");
        match l.request.as_deref() {
            Some("attach") => assert_eq!(l.attributes["pid"].as_str(), Some("${pickProcess}"), "{file}: lldb-dap's and GDB's"),
            None => {
                assert_eq!(l.program.as_deref(), Some(format!("${{{}}}", crate::launch::VARIABLE).as_str()), "{file}");
                assert_eq!(l.pre_launch_task.as_deref(), Some("build"), "{file}: a task every CMake project has");
            }
            other => panic!("{file}: {other:?}"),
        }
    }
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    for (id, first, needs) in [("cpp-vcpkg", "vcpkg", "vcpkg"), ("cpp-conan", "conan", "conan")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        assert!(t.needs.iter().any(|n| n == needs) && t.needs.iter().any(|n| n == "cmake"), "{id}");
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(plan.steps[0].display().split(' ').next(), Some(first), "{id}");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        for f in ["CMakeLists.txt", "src/main.cpp", ".gitignore", "README.md"] {
            assert!(all.contains(&f), "{id}: {f} in {all:?}");
        }
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }
        let cmake = &plan.files.iter().find(|f| f.path == "CMakeLists.txt").unwrap().contents;
        assert!(cmake.contains("project(orbit_desk") && cmake.contains("find_package(fmt CONFIG REQUIRED)"), "{id}: {cmake}");
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
