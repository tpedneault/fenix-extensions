//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language file by the same rules as `fenix-gui`'s
//! (`languages::File`), and Zig's grammar loaded and its queries run as
//! `fenix-syntax` runs them -- tree-sitter 0.26's WebAssembly store.

use std::path::{Path, PathBuf};

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
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/zig", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["zig"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${") && !shown.contains('$'), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["fn", "main", "test", "struct", "enum", "union", "sw", "for", "while", "errdefer", "gpa", "print", "gen"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_snippets_keep_zigs_own_braces() {
    let render = |text: &str| fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text;
    assert_eq!(render(include_str!("../snippets/zig/print.snippet")), "std.debug.print(\"value: {any}\\n\", .{x});");
    let catch = render(include_str!("../snippets/zig/catch.snippet"));
    assert!(catch.contains("std.log.err(\"failed: {}\", .{err});"), "{catch}");
    let gpa = render(include_str!("../snippets/zig/gpa.snippet"));
    assert!(gpa.contains("std.heap.DebugAllocator(.{}) = .init;") && gpa.contains("const gpa = debug_allocator.allocator();"), "{gpa}");
    assert_eq!(render(include_str!("../snippets/zig/imp.snippet")), "const std = @import(\"std\");", "the import mirrors the name");
}

/// `fenix-gui`'s `languages::File`.
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

/// The Zig grammar, as Fenix loads it.
fn zig() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/zig.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref(), l.symbol.as_deref(), l.queries.as_deref()), ("zig", None, None, None), "the server is the code's to set; queries/zig");
    assert_eq!(l.extensions, ["zig", "zon"]);
    assert_eq!(l.comment.as_deref(), Some("//"));
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the language's name.
    let language = store.load_language("zig", &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (engine, language)
}

fn parse(engine: &wasmtime::Engine, language: &Language, source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser.set_wasm_store(WasmStore::new(engine).unwrap()).unwrap();
    parser.set_language(language).unwrap();
    parser.parse(source, None).unwrap()
}

/// Each capture of `query` in `source`: its text, its name and its
/// pattern's index (a later pattern wins in Fenix).
fn captures(language: &Language, tree: &tree_sitter::Tree, query: &str, source: &str) -> Vec<(String, String, usize)> {
    let q = Query::new(language, query).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures[*i];
        out.push((c.node.utf8_text(source.as_bytes()).unwrap().to_string(), names[c.index as usize].to_string(), m.pattern_index));
    }
    out
}

#[test]
fn zigs_grammar_loads_and_its_queries_run() {
    let (engine, language) = zig();
    let source = include_str!("samples/orbit.zig");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/zig/highlights.scm"), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("fn", "keyword.function"),
        ("const", "keyword"),
        ("pub", "keyword.modifier"),
        ("struct", "keyword.type"),
        ("Orbit", "type"),
        ("u32", "type.builtin"),
        ("period", "property"),
        ("describe", "function"),
        ("@import", "keyword.import"),
        ("std", "module"),
        ("@intCast", "function.builtin"),
        ("\"ISS\"", "string"),
        ("5520", "number"),
        ("1.5e3", "number.float"),
        ("true", "boolean"),
        ("null", "constant.builtin"),
        ("MAX_CREW", "constant"),
        ("// Two at once.", "comment"),
        ("//! Orbits, kept and told.", "comment.documentation"),
        ("if", "keyword.conditional"),
        ("for", "keyword.repeat"),
        ("try", "keyword.exception"),
        ("return", "keyword.return"),
        ("\\n", "string.escape"),
    ] {
        assert_eq!(shown(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>());
    }
    assert_eq!(shown("crew"), Some("property"), "a field, not a type: #match? ran");
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation"];
    for (_, name, _) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: the declarations, the tests, the containers.
    let folds = captures(&language, &tree, include_str!("../queries/zig/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["pub fn describe(self: Orbit, writer: anytype) !void {", "test \"an orbit describes itself\" {", "struct {", "switch (self.kind) {"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/zig/indents.scm"), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("{\n        try writer.print(")), "a function's body: {indents:?}");
}

#[test]
fn build_zig_zon_reads_as_zig() {
    // The grammar has no ZON of its own: the manifest's one expression
    // reads as a field missing its `..`, nothing skipped, so all of it is
    // coloured.
    let (engine, language) = zig();
    let source = include_str!("samples/build.zig.zon");
    let tree = parse(&engine, &language, source);
    let sexp = tree.root_node().to_sexp();
    assert!(!sexp.contains("ERROR") && sexp.matches("MISSING").count() == 1, "{sexp}");
    let highlights = captures(&language, &tree, include_str!("../queries/zig/highlights.scm"), source);
    for (text, name) in [("\"0.17.0\"", "string"), ("0x9c2b7a4f1e0d3c58", "number"), ("minimum_zig_version", "property")] {
        assert!(highlights.iter().any(|(t, n, _)| t == text && n == name), "{text} as {name}");
    }
}
