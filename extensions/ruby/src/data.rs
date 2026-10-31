//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language file by the same rules as `fenix-gui`'s
//! (`languages::File`), and Ruby's grammar loaded and its queries run as
//! `fenix-syntax` runs them -- tree-sitter 0.26's WebAssembly store --
//! with Enter's indent worked out as `indents_after` works it out.

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
    let snippets = files("snippets/ruby", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["ruby"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: Ruby is indented with two spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["frozen", "class", "module", "def", "each", "attr", "struct", "data", "casein", "describe", "it", "let", "test", "begin"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    let main = fenix_snippets::Snippet::parse(include_str!("../snippets/ruby/main.snippet")).unwrap();
    assert!(main.template.render(&Default::default(), &Default::default(), "").text.starts_with("if $PROGRAM_NAME == __FILE__\n"), "its dollar kept");
    let test = fenix_snippets::Snippet::parse(include_str!("../snippets/ruby/test.snippet")).unwrap();
    assert!(test.template.render(&Default::default(), &Default::default(), "").text.contains("class ThingTest < Minitest::Test\n  def setup\n    @thing = Thing.new"));
}

/// Ruby's grammar, as Fenix loads it.
fn ruby() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/ruby.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("ruby", None), "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("#"), "gcc in Ruby, which has none of Fenix's own");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm") && wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language("ruby", &wasm).unwrap();
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

/// What each text is coloured as, as Fenix picks among its captures: the
/// narrowest, then the later pattern.
fn highlighted(language: &Language, tree: &tree_sitter::Tree, source: &str) -> Vec<(String, String)> {
    let q = Query::new(language, include_str!("../queries/ruby/highlights.scm")).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    let mut all: Vec<(std::ops::Range<usize>, usize, &str)> = Vec::new();
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures[*i];
        all.push((c.node.byte_range(), m.pattern_index, names[c.index as usize]));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for (range, _, _) in &all {
        let best = all.iter().filter(|(r, _, _)| r == range).max_by_key(|(_, p, _)| *p).unwrap();
        let entry = (source[range.clone()].to_string(), best.2.to_string());
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// Whether Enter at the end of the line holding `at` indents, as
/// `fenix-syntax`'s `indents_after` works it out: an `@indent` starting on
/// that line, before the cursor, and ending at it or after.
fn indents_after(language: &Language, tree: &tree_sitter::Tree, source: &str, at: &str) -> bool {
    let start = source.find(at).unwrap_or_else(|| panic!("{at}"));
    let byte = source[start..].find('\n').map_or(source.len(), |n| start + n);
    let line_start = source[..byte].rfind('\n').map_or(0, |i| i + 1);
    let q = Query::new(language, include_str!("../queries/ruby/indents.scm")).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte.saturating_sub(1)..byte + 1);
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let node = m.captures[*i].node;
        if names[m.captures[*i].index as usize] == "indent" && (line_start..byte).contains(&node.start_byte()) && node.end_byte() >= byte {
            return true;
        }
    }
    false
}

#[test]
fn rubys_grammar_loads_and_its_highlights_run() {
    let (engine, language) = ruby();
    let source = include_str!("samples/orbit.rb");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let shown = highlighted(&language, &tree, source);
    let has = |text: &str, name: &str| shown.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [
        ("def", "keyword.function"), ("class", "keyword"), ("module", "keyword"), ("end", "keyword"), ("Satellite", "type"), ("Orbit", "type"), ("MU", "constant"),
        ("initialize", "function"), ("full_name", "function"), ("parse", "function"), ("fetch", "function.method"), ("sqrt", "function.method"),
        ("require", "include"), ("require_relative", "include"), ("attr_reader", "keyword"), ("puts", "function.builtin"), ("printf", "function.builtin"), ("block_given?", "function.builtin"),
        ("\"json\"", "string"), (":seconds", "string.special.symbol"), ("symbolize_names", "string.special.symbol"), ("398_600.4418", "float"), ("400", "number"), ("2.5", "float"),
        ("nil", "constant.builtin"), ("true", "boolean"), ("@tags", "property"), ("@@count", "property"), ("$PROGRAM_NAME", "variable.builtin"), ("__FILE__", "constant.builtin"),
        ("self", "variable.builtin"), ("radius", "variable"), ("name", "parameter"), ("s", "parameter"), ("# Orbits, kept by name.", "comment"), ("/\\d+/", "string.regex"),
        ("case", "conditional"), ("when", "conditional"), ("else", "conditional"), ("unless", "conditional"), ("if", "conditional"), ("rescue", "keyword"), ("return", "keyword.return"), ("yield", "keyword.return"), ("next", "keyword.return"),
        ("#{", "punctuation.special"), ("=>", "operator"), ("||=", "operator"), ("::", "punctuation.delimiter"), ("|", "punctuation.bracket"), ("\\n", "escape"),
    ] {
        assert!(has(text, name), "{text} as {name}: {:?}", shown.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "escape", "comment", "number", "float", "boolean", "constant", "attribute", "property", "parameter", "namespace", "operator", "punctuation", "conditional", "repeat", "include", "embedded"];
    for (_, name) in &shown {
        assert!(known.contains(&name.split('.').next().unwrap()), "{name}");
    }
}

#[test]
fn what_folds_and_where_enter_indents() {
    let (engine, language) = ruby();
    let source = include_str!("samples/orbit.rb");
    let tree = parse(&engine, &language, source);

    // What folds: the modules, classes, methods and blocks.
    let folds = captures(&language, &tree, include_str!("../queries/ruby/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    for start in ["module Orbit", "class Satellite < Struct.new(:name, :altitude, keyword_init: true)", "def initialize(name:, altitude: 400, **rest)", "def self.parse(text)", "do |s|", "case altitude", "if $PROGRAM_NAME == __FILE__"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents after a line that opens one, not after one that
    // closes on its own line.
    for (line, indents) in [
        ("module Orbit", true), ("class Satellite", true), ("def initialize(name:", true), ("def period", true), ("satellites.each do |s|", true), ("case altitude", true), ("when 2000...35_786", true), ("else\n        :geo", true),
        ("rescue JSON::ParserError", true), ("if $PROGRAM_NAME", true),
        ("def full_name =", false), ("@tags = rest.fetch", false), ("x = [1, 2.5, nil, true]", false), ("Orbit.each_visible(sats) { |s|", false), ("next if s.altitude.nil?", false), ("attr_reader :tags", false),
    ] {
        assert_eq!(indents_after(&language, &tree, source, line), indents, "{line}");
    }

    // A `when` on one line ends there too, but there's no telling it from
    // one whose body is to come.
    assert!(indents_after(&language, &tree, source, "when 0...2000 then :leo"));

    // A `def` just typed, its `end` not yet: the grammar takes the next
    // `end` for its own, or one missing at the end, and Enter indents.
    for (fresh, line) in [("class Orbit\n  def a\n  end\n\n  def b\nend\n", "def b"), ("def period\n", "def period"), ("items.each do |x|\n", "do |x|")] {
        let tree = parse(&engine, &language, fresh);
        assert!(indents_after(&language, &tree, fresh, line), "{fresh:?}: {}", tree.root_node().to_sexp());
    }
}

#[test]
fn the_language_file_names_rubys_files() {
    let l: LanguageFile = toml::from_str(include_str!("../languages/ruby.toml")).unwrap();
    assert!(l.extensions.iter().all(|e| crate::rubocop::EXTENSIONS.contains(&e.as_str())) && l.extensions.len() == crate::rubocop::EXTENSIONS.len(), "the linter takes what Fenix reads as Ruby");
    assert!(l.filenames.iter().all(|f| crate::rubocop::NAMES.contains(&f.as_str()) && !f.contains('/')));
    assert!(!l.filenames.iter().any(|f| f.ends_with(".lock")), "Gemfile.lock isn't Ruby");
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_ruby, from queries/ruby");
}

