//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its launch files and language file by
//! the same rules as `fenix-gui`'s (`dap::Template`, `languages::File`),
//! and Kotlin's grammar loaded and its queries run as `fenix-syntax` runs
//! them -- tree-sitter 0.26's WebAssembly store.

use std::path::{Path, PathBuf};

use fenix_project::template::{load_template_dir, Answer, Origin};
use serde::Deserialize;
use tree_sitter::{wasmtime, Language, Parser, Query, QueryCursor, StreamingIterator, WasmStore};

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
    program: Option<toml::Value>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    attributes: toml::Table,
}

/// `fenix-gui`'s `languages::File` (0.8.5).
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
fn every_snippet_reads_as_fenix_reads_it() {
    for (scope, at_least, keys) in [
        ("java", 20, &["cls", "rec", "main", "test", "ptest", "stream", "groupby", "sealed", "sw"][..]),
        ("kotlin", 15, &["data", "main", "sealed", "launch", "async", "rb", "flow", "test", "ctest"][..]),
    ] {
        let snippets = files(&format!("snippets/{scope}"), "snippet");
        assert!(snippets.len() >= at_least, "{scope}");
        let mut found = Vec::new();
        for (file, text) in &snippets {
            let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(s.scopes, [scope], "{file}");
            assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
            found.push(s.trigger);
        }
        for key in keys {
            assert!(found.iter().any(|k| k == key), "{scope}: {key}");
        }
    }
}

#[test]
fn every_launch_file_attaches_to_java_on_5005() {
    let launches = files("launch", "toml");
    assert_eq!(launches.len(), 3);
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(!l.name.trim().is_empty() && l.languages == ["java"], "{file}");
        // java-debug's launch needs a classpath a launch file can't carry:
        // every session attaches.
        assert_eq!(l.request.as_deref(), Some("attach"), "{file}");
        assert!(l.program.is_none(), "{file}");
        assert_eq!(l.attributes.get("port").and_then(toml::Value::as_integer), Some(i64::from(crate::debug::PORT)), "{file}");
        assert!(l.attributes.get("hostName").and_then(toml::Value::as_str).is_some(), "{file}");
    }
}

/// The Kotlin grammar, as Fenix loads it.
fn kotlin() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/kotlin.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("kotlin", None), "the server is the code's to set");
    assert_eq!(l.extensions, ["kt", "kts"]);
    assert_eq!(l.comment.as_deref(), Some("//"), "gcc in Kotlin, which has none of Fenix's own");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language("kotlin", &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (engine, language)
}

fn parse(engine: &wasmtime::Engine, language: &Language, source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser.set_wasm_store(WasmStore::new(engine).unwrap()).unwrap();
    parser.set_language(language).unwrap();
    parser.parse(source, None).unwrap()
}

/// Each capture of `query` in `source`: its text and its name.
fn captures(language: &Language, tree: &tree_sitter::Tree, query: &str, source: &str) -> Vec<(String, String)> {
    let q = Query::new(language, query).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures[*i];
        out.push((c.node.utf8_text(source.as_bytes()).unwrap().to_string(), names[c.index as usize].to_string()));
    }
    out
}

#[test]
fn kotlins_grammar_loads_and_its_queries_run() {
    let (engine, language) = kotlin();
    let source = include_str!("samples/Orbit.kt");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/kotlin/highlights.scm"), source);
    let has = |text: &str, name: &str| highlights.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [("fun", "keyword.function"), ("data", "keyword"), ("val", "keyword"), ("Orbit", "type"), ("describe", "function"), ("\"ISS\"", "string"), ("5520L", "number"), ("1.5e3", "float"), ("// Two at once.", "comment"), ("when", "conditional"), ("for", "repeat"), ("@", "attribute"), ("println", "function.builtin"), ("package", "keyword"), ("import", "keyword"), ("NOMINAL", "constant")] {
        assert!(has(text, name), "{text} as {name}: {:?}", highlights.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "escape", "comment", "number", "float", "boolean", "constant", "constructor", "attribute", "property", "parameter", "label", "namespace", "operator", "punctuation", "conditional", "repeat"];
    for (_, name) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top) || name.starts_with('_'), "{name}");
    }

    // What folds: the classes and functions.
    let folds = captures(&language, &tree, include_str!("../queries/kotlin/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    for start in ["data class Orbit(val name: String, val period: Seconds, val crew: Int = 0) {", "object Registry : Tracker {", "fun main(args: Array<String>) = runBlocking {", "companion object {"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/kotlin/indents.scm"), source);
    assert!(indents.iter().all(|(_, n)| n == "indent"));
    assert!(indents.iter().any(|(t, _)| t.starts_with("{\n        known += orbit")), "a function's body");
}

#[test]
fn each_project_template_plans() {
    let parent = crate::project::tests::temp("templates");
    for (id, needs, opens, answers) in [
        ("java-gradle-app", "gradle", "app/src/main/java/orbit_desk/App.java", vec![]),
        ("java-gradle-app", "gradle", "app/src/main/kotlin/orbit_desk/App.kt", vec![("language", "kotlin")]),
        ("java-gradle-lib", "gradle", "lib/src/main/java/orbit_desk/Library.java", vec![]),
        ("java-gradle-lib", "gradle", "lib/src/main/kotlin/orbit_desk/Library.kt", vec![("language", "kotlin"), ("java", "25")]),
        ("java-spring-boot", "gradle", "src/main/java/orbit_desk/Application.java", vec![]),
        ("java-maven", "mvn", "src/main/java/orbit_desk/App.java", vec![("java", "17")]),
    ] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let mut a = t.default_answers("orbit-desk");
        for (k, v) in &answers {
            a.insert(k.to_string(), Answer::Text(v.to_string()));
        }
        let plan = t.plan("orbit-desk", &parent, &a).unwrap_or_else(|e| panic!("{id}: {e}"));
        let all: Vec<&fenix_project::template::PlannedFile> = plan.files.iter().chain(&plan.after_files).collect();
        assert!(all.iter().any(|f| f.path == opens), "{id} {answers:?}: {:?}", all.iter().map(|f| &f.path).collect::<Vec<_>>());
        for f in &all {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }
        let steps: Vec<String> = plan.steps.iter().map(|s| s.display()).collect();
        assert!(steps.len() == 1 && steps[0].starts_with(&format!("{needs} ")), "{id}: {steps:?}");
        if needs == "gradle" {
            // The wrapper's made before the build files are there to read.
            assert!(plan.after_files.iter().any(|f| f.path.ends_with("build.gradle.kts")), "{id}");
            assert!(!plan.files.iter().any(|f| f.path.ends_with("build.gradle.kts")), "{id}");
            assert_eq!(steps[0], "gradle wrapper --gradle-version 9.8.2 --distribution-type bin");
        }
        let release = answers.iter().find(|(k, _)| *k == "java").map(|(_, v)| *v).unwrap_or("21");
        let build = all.iter().find(|f| f.path.ends_with("build.gradle.kts") || f.path == "pom.xml").unwrap();
        assert!(build.contents.contains(&format!("JavaLanguageVersion.of({release})")) || build.contents.contains(&format!("jvmToolchain({release})")) || build.contents.contains(&format!("<maven.compiler.release>{release}<")), "{id}: {}", build.contents);
        if answers.contains(&("language", "kotlin")) {
            assert!(build.contents.contains("kotlin(\"jvm\")") && !build.contents.contains("{{"), "{id}");
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
