//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its language file by the same rules
//! as `fenix-gui`'s (`languages::File`), and Haskell's grammar loaded and
//! its queries run as `fenix-syntax` runs them -- tree-sitter 0.26's
//! WebAssembly store -- with what's coloured picked as `resolve_overlaps`
//! picks it, and Enter's indent worked out as `indents_after` works it
//! out.

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

fn render(text: &str) -> String {
    fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/haskell", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["haskell"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: Haskell is indented with spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["mod", "main", "fn", "data", "rec", "newtype", "class", "inst", "case", "where", "do", "let", "if", "guard", "imp", "lang", "hspec", "desc", "it", "prop"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_snippets_keep_haskells_dollars_and_lambdas() {
    assert_eq!(render(include_str!("../snippets/haskell/lam.snippet")), "\\x -> x");
    let it = render(include_str!("../snippets/haskell/it.snippet"));
    assert_eq!(it, "it \"does something\" $ do\n  True `shouldBe` True");
    let prop = render(include_str!("../snippets/haskell/prop.snippet"));
    assert!(prop.starts_with("prop \"reverses twice to itself\" $ \\xs ->\n"), "{prop}");
    let hspec = render(include_str!("../snippets/haskell/hspec.snippet"));
    assert!(hspec.starts_with("module OrbitSpec (spec) where\n\nimport Test.Hspec\nimport Orbit\n"), "the module mirrored: {hspec}");
    assert!(render(include_str!("../snippets/haskell/newtype.snippet")).starts_with("newtype Name = Name { unName :: String }"));
    assert_eq!(render(include_str!("../snippets/haskell/lang.snippet")), "{-# LANGUAGE OverloadedStrings #-}");
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

/// Haskell's grammar, as Fenix loads it.
fn haskell() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/haskell.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("haskell", None), "the server is the code's to set");
    assert_eq!(l.extensions, crate::hlint::EXTENSIONS, "hlint takes what Fenix reads as Haskell");
    assert_eq!(l.comment.as_deref(), Some("--"), "gcc in Haskell, which has none of Fenix's own");
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_haskell, queries/haskell");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm") && wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language("haskell", &wasm).unwrap();
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
/// narrowest, then the later pattern. Every capture counts, `@_name`s
/// too: Fenix colours each by its name.
fn highlighted(language: &Language, tree: &tree_sitter::Tree, source: &str) -> Vec<(String, String)> {
    let q = Query::new(language, include_str!("../queries/haskell/highlights.scm")).unwrap_or_else(|e| panic!("{e}"));
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
    let q = Query::new(language, include_str!("../queries/haskell/indents.scm")).unwrap_or_else(|e| panic!("{e}"));
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
fn haskells_grammar_loads_and_its_highlights_run() {
    let (engine, language) = haskell();
    let source = include_str!("samples/Orbit.hs");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let shown = highlighted(&language, &tree, source);
    let has = |text: &str, name: &str| shown.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [
        ("{-# LANGUAGE OverloadedStrings #-}", "preproc"), ("-- | Orbits, kept by name.", "comment.documentation"), ("module", "keyword.import"), ("import", "keyword.import"), ("qualified", "keyword.import"),
        ("Strict", "module"), ("where", "keyword"), ("data", "keyword"), ("newtype", "keyword"), ("class", "keyword"), ("instance", "keyword"), ("deriving", "keyword"),
        ("Orbit", "type"), ("Orbit", "constructor"), ("String", "type"), ("Leo", "constructor"), ("a", "type"), ("crew", "property"),
        ("describe", "function"), ("kind", "function"), ("register", "function"), ("main", "function"), ("putStrLn", "function.call"), ("show", "function.call"), ("lookup", "function.call"),
        ("o", "variable.parameter"), ("altitude", "variable.parameter"), ("x", "variable.parameter"), ("known", "variable"), ("iss", "variable"), ("maxCrew", "variable"),
        ("case", "keyword.conditional"), ("of", "keyword.conditional"), ("if", "keyword.conditional"), ("then", "keyword.conditional"), ("do", "keyword"), ("let", "keyword"),
        ("throwIO", "keyword.exception"), ("otherwise", "boolean"), ("True", "boolean"), ("()", "constant.builtin"),
        ("\"ISS\"", "string"), ("'x'", "string"), ("2000", "number"), ("1.5e3", "number.float"), ("::", "operator"), ("->", "operator"), ("++", "operator"), ("(", "punctuation.bracket"), (",", "punctuation.delimiter"),
    ] {
        assert!(has(text, name), "{text} as {name}: {:?}", shown.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    assert!(!has("altitude", "type") && !has("known", "type"), "a variable isn't a type: the grammar's (variable) @type made one of every name");
    // Each name is one Fenix colours, by its first part: none of the
    // grammar's @_names left to win a text and colour it as nothing.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "constructor", "property", "module", "operator", "punctuation", "preproc"];
    for (_, name) in &shown {
        assert!(known.contains(&name.split('.').next().unwrap()), "{name}");
    }
}

#[test]
fn what_folds_and_where_enter_indents() {
    let (engine, language) = haskell();
    let source = include_str!("samples/Orbit.hs");
    let tree = parse(&engine, &language, source);

    // What folds: the declarations, each equation, the blocks.
    let folds = captures(&language, &tree, include_str!("../queries/haskell/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    for start in ["module Orbit", "import qualified Data.Map.Strict as Map", "data Orbit = Orbit", "class Described a where", "instance Described Orbit where", "kind altitude", "register o (Registry known) =", "case Map.lookup (name o) known of", "main = do", "if crew iss > 0"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }
    assert!(!folded.contains(&"a -> String"), "a function's type isn't a function");

    // Enter indents after a line that ends in what opens a block, not
    // after a one-line equation.
    for (line, indents) in [
        ("module Orbit", false), ("data Orbit = Orbit", false), ("class Described a where", true), ("instance Described Orbit where", true), ("register o (Registry known) =", true), ("case Map.lookup (name o) known of", true),
        ("main = do", true), ("Just _ -> Left", false), ("if crew iss > 0", false), ("then putStrLn", false), ("  where", true), ("  ( Orbit (..)", false),
        ("maxCrew = 7", false), ("describe :: a -> String", false), ("kind :: Double -> Kind", false), ("| altitude < 2000 = Leo", false), ("import Data.List (sortOn)", false),
    ] {
        assert_eq!(indents_after(&language, &tree, source, line), indents, "{line}");
    }
    for (fresh, line) in [("main :: IO ()\nmain = do\n", "main = do"), ("f x =\n", "f x ="), ("xs = [\n  1\n  ]\n", "xs = ["), ("g y = case y of\n", "case y of"), ("h = if a\n  then\n    b\n  else c\n", "  then")] {
        let tree = parse(&engine, &language, fresh);
        assert!(indents_after(&language, &tree, fresh, line), "{fresh:?}: {}", tree.root_node().to_sexp());
    }
}

#[test]
fn newer_haskell_parses_too() {
    let (engine, language) = haskell();
    let source = "{-# LANGUAGE LambdaCase, DerivingVia, GADTs #-}\nmodule Shapes where\n\nimport Data.Kind (Type)\n\ndata Shape :: Type where\n  Circle :: Double -> Shape\n  Square :: Double -> Shape\n\narea :: Shape -> Double\narea = \\case\n  Circle r -> pi * r * r\n  Square s -> s * s\n\nnewtype Metres = Metres Double\n  deriving (Show, Eq, Ord) via Double\n\nsign :: Int -> String\nsign n = if | n < 0 -> \"-\" | otherwise -> \"+\"\n";
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

#[test]
fn the_cabal_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    let t = load_template_dir(&dir("templates").join("haskell-cabal"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers("orbit-desk");
    let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    let all: Vec<&fenix_project::template::PlannedFile> = plan.files.iter().chain(&plan.after_files).collect();
    let paths: Vec<&str> = all.iter().map(|f| f.path.as_str()).collect();
    for path in ["orbit-desk.cabal", "src/OrbitDesk.hs", "app/Main.hs", "test/Main.hs", "README.md", ".gitignore"] {
        assert!(paths.contains(&path), "{path}: {paths:?}");
    }
    for f in &all {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    // What the pack reads of it: each component, and Haskell that parses.
    let cabal = &all.iter().find(|f| f.path == "orbit-desk.cabal").unwrap().contents;
    assert!(!cabal.contains("author:"), "no author given: none written");
    let package = crate::project::parse_cabal(cabal);
    let components: Vec<(crate::project::Kind, &str)> = package.components.iter().map(|c| (c.kind, c.name.as_str())).collect();
    assert_eq!(components, [(crate::project::Kind::Library, "orbit-desk"), (crate::project::Kind::Executable, "orbit-desk"), (crate::project::Kind::Test, "orbit-desk-test")]);
    let (engine, language) = haskell();
    for f in all.iter().filter(|f| f.path.ends_with(".hs")) {
        let tree = parse(&engine, &language, &f.contents);
        assert!(!tree.root_node().has_error(), "{}: {}", f.path, tree.root_node().to_sexp());
    }
    let mut with = answers.clone();
    with.insert("author".into(), fenix_project::template::Answer::Text("Ada".into()));
    let plan = t.plan("orbit-desk", &parent, &with).unwrap();
    assert!(plan.files.iter().any(|f| f.contents.contains("author:             Ada\n")));
    std::fs::remove_dir_all(&parent).unwrap();
}
