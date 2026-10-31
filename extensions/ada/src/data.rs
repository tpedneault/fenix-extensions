//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language files by the same rules as `fenix-gui`'s
//! (`languages::File`), and Ada's grammar loaded and its queries run as
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

/// Ada's grammar, as Fenix loads it for `file` (`ada.toml`, `gpr.toml`).
fn ada(file: &str) -> (wasmtime::Engine, Language, LanguageFile) {
    let l: LanguageFile = toml::from_str(&std::fs::read_to_string(dir("languages").join(file)).unwrap()).unwrap();
    assert_eq!(l.server, None, "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("--"), "gcc in Ada, which has none of Fenix's own");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm"));
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language(l.symbol.as_deref().unwrap_or("ada"), &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (engine, language, l)
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

/// What Fenix shows of the first `text` in `highlights`: of the captures
/// of just it, the latest pattern's.
fn shown<'a>(highlights: &'a [(String, String, usize, usize)], text: &str) -> Option<&'a str> {
    let at = highlights.iter().filter(|(t, ..)| t == text).map(|(_, _, at, _)| *at).min()?;
    highlights.iter().filter(|(t, _, a, _)| t == text && *a == at).max_by_key(|(.., p)| *p).map(|(_, n, ..)| n.as_str())
}

const HIGHLIGHTS: &str = include_str!("../queries/ada/highlights.scm");

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_parses_as_ada() {
    let snippets = files("snippets/ada", "snippet");
    assert!(snippets.len() >= 15);
    let (engine, language, _) = ada("ada.toml");
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["ada"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: Ada's indented with spaces, three (GNAT's style)");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        // What it makes is Ada: in a package body or a procedure, where it's a
        // declaration or a statement.
        let unit = if shown.starts_with("with ") || shown.starts_with("package") || shown.starts_with("generic") || shown.starts_with("procedure ") && shown.contains("begin") {
            shown.clone()
        } else if ["case", "for", "begin", "pragma"].iter().any(|w| shown.starts_with(w)) {
            format!("procedure P is\nbegin\n{shown}\nend P;\n")
        } else {
            format!("package body P is\n{shown}\nend P;\n")
        };
        let tree = parse(&engine, &language, &unit);
        assert!(!tree.root_node().has_error(), "{file}: {unit}\n{}", tree.root_node().to_sexp());
        keys.push(s.trigger);
    }
    for key in ["pkg", "pkgb", "main", "proc", "func", "exprf", "rec", "tagged", "gen", "task", "prot", "case", "for", "exc", "contract", "spark", "inv", "aunit"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn adas_grammar_loads_and_its_queries_run() {
    let (engine, language, l) = ada("ada.toml");
    assert_eq!((l.language.as_str(), l.extensions.as_slice()), ("ada", &["ads".to_string(), "adb".to_string(), "ada".to_string()][..]));
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_ada, queries/ada");
    for (name, source) in [("orbits.ads", include_str!("samples/orbits.ads")), ("orbits.adb", include_str!("samples/orbits.adb"))] {
        let tree = parse(&engine, &language, source);
        assert!(!tree.root_node().has_error(), "{name}: {}", tree.root_node().to_sexp());
    }

    let spec = include_str!("samples/orbits.ads");
    let tree = parse(&engine, &language, spec);
    let highlights = captures(&language, &tree, HIGHLIGHTS, spec);
    for (text, name) in [
        ("--  Orbits: an orbit by its period, and a registry of them.", "comment.documentation"),
        ("--  An orbit, by its period in minutes.", "comment"),
        ("with", "include"),
        ("Ada.Strings.Unbounded", "namespace"),
        ("package", "keyword.function"),
        ("Orbits", "namespace"),
        ("SPARK_Mode", "attribute"),
        ("Global", "attribute"),
        ("Pre", "attribute"),
        ("Post", "attribute"),
        ("Old", "property"),
        ("constant", "storageclass"),
        ("16#40#", "number"),
        ("type", "keyword.type"),
        ("Altitude", "type"),
        ("record", "keyword.type"),
        ("Name", "property"),
        ("Unbounded_String", "type"),
        ("92.68", "number"),
        ("tagged", "keyword.type"),
        ("function", "keyword.function"),
        ("Is_Low", "function"),
        ("O", "parameter"),
        ("Boolean", "type"),
        ("in", "keyword.operator"),
        ("Raise_By", "function"),
        ("out", "keyword"),
        ("null", "keyword"),
        (":=", "operator"),
        ("=>", "operator"),
        ("generic", "keyword"),
        ("\"<\"", "function"),
        ("private", "include"),
    ] {
        assert_eq!(shown(&highlights, text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, ..)| t == text).collect::<Vec<_>>());
    }

    let body = include_str!("samples/orbits.adb");
    let tree = parse(&engine, &language, body);
    let highlights = captures(&language, &tree, HIGHLIGHTS, body);
    for (text, name) in [
        ("begin", "keyword"),
        ("for", "repeat"),
        ("loop", "repeat"),
        ("Range", "property"),
        ("pragma", "keyword"),
        ("Loop_Invariant", "attribute"),
        ("return", "keyword.return"),
        ("task", "keyword.type"),
        ("entry", "keyword"),
        ("accept", "keyword"),
        ("select", "conditional"),
        ("terminate", "keyword"),
        ("protected", "keyword.type"),
        ("case", "conditional"),
        ("when", "keyword"),
        ("raise", "keyword.exception"),
        ("Program_Error", "variable"),
        ("elsif", "conditional"),
        ("exception", "keyword.exception"),
        ("Constraint_Error", "variable"),
        ("Put_Line", "function"),
        ("'?'", "string"),
    ] {
        let got = shown(&highlights, text).unwrap_or("variable");
        assert_eq!(got, name, "{text}: {:?}", highlights.iter().filter(|(t, ..)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "constant", "attribute", "property", "parameter", "label", "namespace", "operator", "punctuation", "conditional", "repeat", "storageclass", "include", "preproc"];
    for (_, name, ..) in captures(&language, &tree, HIGHLIGHTS, body).iter().chain(&highlights) {
        assert!(known.contains(&name.split('.').next().unwrap()), "{name}");
    }

    // What folds: the units, the types, the statements that hold others.
    let folds = captures(&language, &tree, include_str!("../queries/ada/folds.scm"), body);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, ..)| n == "fold").map(|(t, ..)| t.lines().next().unwrap()).collect();
    for start in ["package body Orbits", "procedure Raise_By (H : in out Altitude; By : Altitude) is", "task type Beacon (Id : Positive) is", "task body Beacon is", "protected type Counter is", "protected body Counter is", "for I in A'Range loop", "case S is", "if S = Lost then", "when E : Constraint_Error =>"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }
    let spec_folds = captures(&language, &parse(&engine, &language, spec), include_str!("../queries/ada/folds.scm"), spec);
    assert!(spec_folds.iter().any(|(t, ..)| t.starts_with("record\n")), "a record");

    // Enter indents inside a unit, a body, a statement: an @indent
    // starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/ada/indents.scm"), body);
    assert!(indents.iter().all(|(_, n, ..)| n == "indent"));
    for start in ["package body Orbits", "procedure Raise_By", "for I in A'Range loop", "task body Beacon is", "select", "case S is", "when Nominal =>", "if S = Lost then", "when E : Constraint_Error =>"] {
        assert!(indents.iter().any(|(t, ..)| t.starts_with(start)), "{start}");
    }
}

#[test]
fn a_project_file_reads_as_ada_well_enough_to_colour() {
    let (engine, language, l) = ada("gpr.toml");
    assert_eq!((l.language.as_str(), l.extensions.as_slice(), l.symbol.as_deref(), l.queries.as_deref()), ("gpr", &["gpr".to_string()][..], Some("ada"), Some("queries/ada")));
    assert_eq!(l.grammar.as_deref(), Some("grammars/tree-sitter-ada.wasm"), "Ada's grammar");
    let source = include_str!("samples/orbit.gpr");
    let tree = parse(&engine, &language, source);
    assert!(tree.root_node().has_error(), "`project ... is` isn't Ada: the language file says so");
    let highlights = captures(&language, &tree, HIGHLIGHTS, source);
    for (text, name) in [("with", "include"), ("\"config/orbit_config.gpr\"", "string"), ("for", "repeat"), ("use", "include"), ("package", "keyword.function"), ("end", "keyword"), ("\"-Es\"", "string")] {
        assert_eq!(shown(&highlights, text), Some(name), "{text}");
    }
}
