//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its language file by the same rules as
//! `fenix-gui`'s (`languages::File`), and PHP's grammar loaded and its
//! queries run as `fenix-syntax` runs them -- tree-sitter 0.26's
//! WebAssembly store.

use std::path::{Path, PathBuf};

use fenix_project::template::{load_template_dir, Origin};
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

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_keeps_its_dollars() {
    let snippets = files("snippets/php", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["php"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: PSR-12 indents with spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["class", "fn", "met", "ctor", "foreach", "match", "enum", "rclass", "test", "try", "php", "arrow"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    // PHP's variables are literal, beside a field's mirror.
    let foreach = fenix_snippets::Snippet::parse(include_str!("../snippets/php/foreach.snippet")).unwrap();
    assert_eq!(foreach.template.render(&Default::default(), &Default::default(), "").text, "foreach ($items as $item) {\n    \n}\n");
    let ctor = fenix_snippets::Snippet::parse(include_str!("../snippets/php/ctor.snippet")).unwrap();
    assert!(ctor.template.render(&Default::default(), &Default::default(), "").text.contains("private readonly string $name,"));
}

/// `fenix-gui`'s `languages::File` (0.8.7).
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

/// PHP's grammar, as Fenix loads it.
fn php() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/php.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("php", None), "the server is the code's to set");
    assert!(l.extensions.contains(&"php".to_string()) && l.extensions.contains(&"phtml".to_string()));
    assert_eq!(l.comment.as_deref(), Some("//"), "gcc in PHP, which has none of Fenix's own");
    let grammar = l.grammar.as_deref().unwrap();
    let wasm = std::fs::read(dir(grammar)).unwrap();
    assert!(wasm.starts_with(b"\0asm") && wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    // `tree_sitter_<symbol>`: by default the file's name, `tree-sitter-` off.
    let symbol = l.symbol.unwrap_or_else(|| Path::new(grammar).file_stem().unwrap().to_string_lossy().trim_start_matches("tree-sitter-").replace('-', "_"));
    assert_eq!(symbol, "php", "PHP in HTML, not php_only");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    let language = store.load_language(&symbol, &wasm).unwrap();
    assert_eq!(language.abi_version(), 15, "the ABI grammars/README.md says; Fenix loads 13 to 15");
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
fn phps_grammar_loads_and_its_queries_run() {
    let (engine, language) = php();
    let source = include_str!("samples/Orbit.php");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/php/highlights.scm"), source);
    let has = |text: &str, name: &str| highlights.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [
        ("<?php", "tag"), ("namespace", "keyword"), ("Orbit", "module"), ("readonly", "keyword"), ("class", "keyword"), ("Satellite", "type"), ("function", "keyword"),
        ("__construct", "constructor"), ("string", "type.builtin"), ("$name", "variable"), ("this", "variable.builtin"), ("describe", "function.method"),
        ("sprintf", "function"), ("'%s: %d min'", "string"), ("\\n", "escape"), ("90", "number"), ("1.5e3", "float"), ("// Two at once.", "comment"),
        ("match", "keyword"), ("enum", "keyword"), ("Status", "type"), ("Nominal", "constant"), ("true", "boolean"), ("null", "constant.builtin"),
        ("Override", "attribute"), ("SECONDS", "constant"), ("foreach", "keyword"), ("fn", "keyword"), ("Satellite", "constructor"), ("period", "property"),
    ] {
        assert!(has(text, name), "{text} as {name}: {:?}", highlights.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "escape", "comment", "number", "float", "boolean", "constant", "constructor", "attribute", "property", "parameter", "module", "namespace", "operator", "punctuation", "tag"];
    for (_, name) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: the types, functions and methods.
    let folds = captures(&language, &tree, include_str!("../queries/php/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    for start in ["final readonly class Satellite", "enum Status: string", "public function isLow(): bool", "#[\\Override]", "function brightest(array $satellites): ?Satellite", "interface Tracker"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/php/indents.scm"), source);
    assert!(indents.iter().all(|(_, n)| n == "indent"));
    assert!(indents.iter().any(|(t, _)| t.starts_with("{\n        return sprintf")), "a method's body");
    assert!(indents.iter().any(|(t, _)| t.starts_with("(\n        public string $name")), "promoted parameters");
}

#[test]
fn the_composer_package_template_plans() {
    let parent = crate::project::tests::temp("templates");
    let t = load_template_dir(&dir("templates").join("php-package"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers("orbit-desk");
    let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
    for path in ["composer.json", "src/OrbitDesk.php", "tests/OrbitDeskTest.php", "phpunit.xml.dist", ".gitignore", "README.md"] {
        assert!(all.contains(&path), "{path}: {all:?}");
    }
    for f in plan.files.iter().chain(&plan.after_files) {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    let composer = plan.files.iter().find(|f| f.path == "composer.json").unwrap();
    let json: fenix_lang_kit::serde_json::Value = fenix_lang_kit::serde_json::from_str(&composer.contents).unwrap_or_else(|e| panic!("composer.json: {e}"));
    assert_eq!(json["name"], "acme/orbit-desk");
    assert_eq!(json["autoload"]["psr-4"]["Acme\\OrbitDesk\\"], "src/");
    let project = crate::project::Project::parse(&composer.contents).unwrap();
    assert_eq!(project.php.as_deref(), Some("^8.2"));
    let steps: Vec<String> = plan.steps.iter().map(|s| s.display()).collect();
    assert_eq!(steps, ["composer install"]);
    std::fs::remove_dir_all(&parent).unwrap();
}
