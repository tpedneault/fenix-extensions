//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its language files by the same rules
//! as `fenix-gui`'s (`languages::File`), and Swift's grammar loaded and
//! its queries run as `fenix-syntax` runs them -- tree-sitter 0.26's
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

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/swift", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["swift"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: Swift's indented with spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["struct", "class", "enum", "protocol", "ext", "func", "guard", "iflet", "switch", "afunc", "task", "actor", "view", "observable", "xctest", "test", "suite"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn snippets_keep_swifts_dollar_and_interpolation() {
    let render = |text: &str| fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text;
    let view = render(include_str!("../snippets/swift/view.snippet"));
    assert!(view.contains("Text(\"\\(title)\")") || view.contains("Text(title)"), "{view}");
    assert!(view.contains("#Preview"), "{view}");
    let guarded = render(include_str!("../snippets/swift/guard.snippet"));
    assert!(guarded.starts_with("guard let value else {\n    return\n}"), "{guarded}");
    let expect = render(include_str!("../snippets/swift/test.snippet"));
    assert!(expect.contains("@Test") && expect.contains("#expect("), "{expect}");
}

#[test]
fn the_json_file_adds_swifts_configs_to_fenixs_json() {
    let l: LanguageFile = toml::from_str(include_str!("../languages/json.toml")).unwrap();
    assert_eq!(l.language, "json");
    assert_eq!(l.filenames, [".swift-format", "Package.resolved"]);
    assert!(l.server.is_none() && l.grammar.is_none() && l.comment.is_none(), "the rest is Fenix's");
}

/// Swift's grammar, as Fenix loads it.
fn swift() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/swift.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("swift", None), "the server is the code's to set");
    assert_eq!(l.extensions, ["swift", "swiftinterface"]);
    assert_eq!(l.comment.as_deref(), Some("//"), "gcc in Swift, which has none of Fenix's own");
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_swift, queries/swift");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm"));
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language("swift", &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (engine, language)
}

fn parse(engine: &wasmtime::Engine, language: &Language, source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser.set_wasm_store(WasmStore::new(engine).unwrap()).unwrap();
    parser.set_language(language).unwrap();
    parser.parse(source, None).unwrap()
}

/// Each capture of `query` in `source`: its text, its name, where it
/// starts and its pattern's place in the file.
fn captures(language: &Language, tree: &tree_sitter::Tree, query: &str, source: &str) -> Vec<(String, String, usize, usize)> {
    let q = Query::new(language, query).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures[*i];
        out.push((c.node.utf8_text(source.as_bytes()).unwrap().to_string(), names[c.index as usize].to_string(), c.node.start_byte(), m.pattern_index));
    }
    out
}

#[test]
fn swifts_grammar_loads_and_its_queries_run() {
    let (engine, language) = swift();
    let source = include_str!("samples/Orbit.swift");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/swift/highlights.scm"), source);
    // What Fenix shows of the first `text`: of the captures of just it,
    // the latest pattern's.
    let last = |text: &str| {
        let at = highlights.iter().filter(|(t, ..)| t == text).map(|(_, _, at, _)| *at).min()?;
        highlights.iter().filter(|(t, _, a, _)| t == text && *a == at).max_by_key(|(.., p)| *p).map(|(_, n, ..)| n.as_str())
    };
    for (text, name) in [
        ("func", "keyword.function"),
        ("struct", "keyword.type"),
        ("actor", "keyword.type"),
        ("let", "keyword"),
        ("import", "keyword"),
        ("Foundation", "namespace"),
        ("Orbit", "type"),
        ("describe", "function.method"),
        ("ISS", "string"),
        ("5520", "number"),
        ("1.5e3", "float"),
        ("0x2A", "number"),
        ("// Two at once.", "comment"),
        ("/// An orbit, by its period.", "comment.documentation"),
        ("switch", "conditional"),
        ("guard", "conditional"),
        ("for", "repeat"),
        ("return", "keyword.return"),
        ("throw", "keyword.exception"),
        ("await", "keyword.coroutine"),
        ("@", "attribute"),
        ("discardableResult", "attribute"),
        ("print", "function.builtin"),
        ("Task", "constructor"),
        ("nominal", "constant"),
        ("self", "variable.builtin"),
        ("nil", "constant.builtin"),
        ("true", "boolean"),
        ("init", "constructor"),
        ("private", "keyword.modifier"),
    ] {
        assert_eq!(last(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, ..)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "escape", "comment", "number", "float", "boolean", "constant", "constructor", "attribute", "property", "parameter", "label", "namespace", "operator", "punctuation", "conditional", "repeat", "preproc"];
    for (_, name, ..) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: the types and functions.
    let folds = captures(&language, &tree, include_str!("../queries/swift/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, ..)| n == "fold").map(|(t, ..)| t.lines().next().unwrap()).collect();
    for start in ["public struct Orbit: Codable, Hashable {", "enum Status {", "protocol Tracker {", "actor Registry {", "extension Orbit: CustomStringConvertible {", "func first(of orbits: [Orbit]) throws -> Orbit {", "init(name: String, period: Double) {", "switch status {"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/swift/indents.scm"), source);
    assert!(indents.iter().all(|(_, n, ..)| n == "indent"));
    for start in ["{\n        known[orbit.name] = orbit", "guard let first = orbits.first else {", "case .degraded(let reason):", "{\n        await Registry().add(iss)"] {
        assert!(indents.iter().any(|(t, ..)| t.starts_with(start)), "{start}");
    }
}

#[test]
fn newer_swift_parses_too() {
    let (engine, language) = swift();
    let source = "import SwiftUI\nimport Testing\n\n@Observable\nfinal class Model {\n    var count = 0\n}\n\nstruct CounterView: View {\n    @State private var model = Model()\n\n    var body: some View {\n        Button(\"Add\") { model.count += 1 }\n    }\n}\n\n#Preview {\n    CounterView()\n}\n\n@Test func adds() async throws {\n    let m = Model()\n    #expect(m.count == 0)\n}\n\nfunc load<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {\n    try JSONDecoder().decode(type, from: data)\n}\n";
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

#[test]
fn the_package_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    let t = load_template_dir(&dir("templates").join("swift-executable"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers("orbit-desk");
    let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    let all: Vec<&fenix_project::template::PlannedFile> = plan.files.iter().chain(&plan.after_files).collect();
    let paths: Vec<&str> = all.iter().map(|f| f.path.as_str()).collect();
    for path in ["Package.swift", "Sources/OrbitDesk/OrbitDesk.swift", "Sources/OrbitDeskCore/Greeting.swift", "Tests/OrbitDeskCoreTests/GreetingTests.swift", ".gitignore", "README.md"] {
        assert!(paths.contains(&path), "{path}: {paths:?}");
    }
    for f in &all {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    // What the pack reads of it: its tools version and its executable.
    let manifest = &all.iter().find(|f| f.path == "Package.swift").unwrap().contents;
    let package = crate::project::Package::parse(manifest);
    assert_eq!(package.tools.as_deref(), Some("6.0"));
    assert_eq!(package.executables, ["orbit-desk"]);
    std::fs::remove_dir_all(&parent).unwrap();
}
