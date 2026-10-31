//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its launch files and language files
//! by the same rules as `fenix-gui`'s (`dap::Template`,
//! `languages::File`).

use std::collections::BTreeMap;
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

/// `fenix-gui`'s `dap::Program`: a path, or `false` for none (0.8.5).
#[allow(dead_code)]
#[derive(Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Program {
    Path(String),
    Sent(bool),
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
    program: Option<Program>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    attributes: toml::Table,
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
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders() {
    let mut keys = Vec::new();
    for (folder, scopes) in [("javascript", &["javascript", "typescript", "tsx"][..]), ("typescript", &["typescript", "tsx"][..]), ("tsx", &["tsx"][..])] {
        for (file, text) in files(&format!("snippets/{folder}"), "snippet") {
            let s = fenix_snippets::Snippet::parse(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(s.scopes.first().map(String::as_str), Some(folder), "{file}: in its first scope's folder");
            assert!(s.scopes.iter().all(|sc| scopes.contains(&sc.as_str())), "{file}: {:?}", s.scopes);
            assert_eq!(format!("{}.snippet", s.trigger), file, "a snippet's file is its key");
            let shown = s.template.render(&BTreeMap::new(), &fenix_snippets::Context(BTreeMap::new()), "").text;
            assert!(!shown.contains("${") || file == "fetchj.snippet", "{file}: {shown}");
            keys.push((s.trigger, s.scopes, shown));
        }
    }
    assert!(keys.len() >= 35, "{}", keys.len());
    for key in ["imp", "exf", "afn", "aaf", "tryc", "desc", "it", "vt", "rfc", "us", "ue", "hook", "int", "type", "union", "guard", "rfct"] {
        assert!(keys.iter().any(|(k, _, _)| k == key), "{key}");
    }
    // React's need JSX: not in a .ts file.
    let rfc = keys.iter().find(|(k, _, _)| k == "rfc").unwrap();
    assert_eq!(rfc.1, ["javascript", "tsx"]);
    // A template literal's `${` and an object's braces come out as written.
    let shown = |key: &str| keys.iter().find(|(k, _, _)| k == key).unwrap().2.clone();
    assert!(shown("fetchj").contains("`fetch failed: ${response.status}`"), "{}", shown("fetchj"));
    assert!(shown("ctx").contains("useMemo(() => ({}), [])"), "{}", shown("ctx"));
}

#[test]
fn every_launch_file_is_js_debugs_and_says_what_runs() {
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 8);
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(!l.name.trim().is_empty() && l.languages == ["javascript", "typescript", "tsx"], "{file}");
        let kind = l.attributes.get("type").and_then(|t| t.as_str()).unwrap_or_else(|| panic!("{file}: js-debug needs its type"));
        assert!(["pwa-node", "pwa-chrome"].contains(&kind), "{file}: {kind}");
        assert_ne!(l.program, Some(Program::Sent(true)), "{file}: Fenix refuses program = true");
        match (l.request.as_deref(), kind) {
            (Some("attach"), _) => assert!((l.attributes.contains_key("port") || l.attributes.get("processId").and_then(|p| p.as_str()) == Some("${pickProcess}")) && l.program.is_none(), "{file}"),
            // A page, or a script npm runs: no program sent.
            (_, "pwa-chrome") => assert!(l.program == Some(Program::Sent(false)) && l.attributes.contains_key("url"), "{file}"),
            (_, _) if l.attributes.contains_key("runtimeExecutable") => assert_eq!(l.program, Some(Program::Sent(false)), "{file}"),
            (None | Some("launch"), _) => {}
            (Some(other), _) => panic!("{file}: {other}"),
        }
    }
}

#[test]
fn the_language_files_add_file_types_only() {
    for (file, text) in files("languages", "toml") {
        let l: Language = toml::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(format!("{}.toml", l.language), file);
        assert!(l.server.is_none() && !(l.extensions.is_empty() && l.filenames.is_empty()), "{file}: the servers are the code's to set");
        assert!(l.filenames.iter().all(|f| !f.contains('/')), "{file}");
    }
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    let ids: Vec<String> = std::fs::read_dir(dir("templates")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(ids.len(), 4, "{ids:?}");
    for (id, opens, first) in [("web-next", "app/page.tsx", "npm"), ("web-express", "src/app.ts", "npm"), ("web-library", "src/index.ts", "npm"), ("web-static", "index.html", "")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{id}: {e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(plan.steps.first().map(|s| s.display().split(' ').next().unwrap_or("").to_string()).unwrap_or_default(), first, "{id}");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        assert!(all.contains(&opens) || plan.steps.iter().any(|s| s.display().contains("next-app@latest")), "{id}: {all:?}");
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{name"), "{id}: {} has a {{{{name...}}}} left", f.path);
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
