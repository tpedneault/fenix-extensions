//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its language files by the same rules
//! as `fenix-gui`'s (`languages::File`), and OCaml's two grammars loaded
//! and their queries run as `fenix-syntax` runs them -- tree-sitter
//! 0.26's WebAssembly store -- with what's coloured picked as
//! `resolve_overlaps` picks it, and Enter's indent worked out as
//! `indents_after` works it out.

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

/// Each snippet in `snippets/<scope>`, read and rendered: their keys.
fn snippets(scope: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for (file, text) in &files(&format!("snippets/{scope}"), "snippet") {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, [scope], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: OCaml is indented with spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    keys
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let keys = snippets("ocaml");
    assert!(keys.len() >= 15);
    for key in ["let", "letin", "fn", "rec", "fun", "match", "if", "try", "for", "mod", "modtype", "functor", "type", "record", "exn", "main", "printf", "alcotest", "expect"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    assert_eq!(snippets("ocaml_interface"), ["mod", "type", "val"], "an interface's own");
}

#[test]
fn the_snippets_keep_ocamls_own_text() {
    assert_eq!(render(include_str!("../snippets/ocaml/printf.snippet")), "Printf.printf \"%s\\n\" x");
    let expect = render(include_str!("../snippets/ocaml/expect.snippet"));
    assert_eq!(expect, "let%expect_test \"prints\" =\n  print_endline \"hello\";\n  [%expect {| hello |}]\n");
    let alcotest = render(include_str!("../snippets/ocaml/alcotest.snippet"));
    assert!(alcotest.contains("[ (\"describe\", [ Alcotest.test_case \"same\" `Quick test_describe ]) ]"), "mirrored: {alcotest}");
    let rec_ = render(include_str!("../snippets/ocaml/rec.snippet"));
    assert!(rec_.contains("| _ :: rest -> 1 + count rest"), "{rec_}");
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

/// A grammar as Fenix loads it, from its language file: its language, the
/// symbol its WebAssembly exports, and its queries' folder.
fn load(file: &str) -> (wasmtime::Engine, Language, String) {
    let l: LanguageFile = toml::from_str(file).unwrap();
    assert!(l.server.is_none(), "the server is the code's to set");
    assert!(l.comment.is_none(), "OCaml has no line comment for gcc");
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_<language>, queries/<language>");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm") && wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name, `tree-sitter-` off.
    let symbol = l.grammar.as_deref().unwrap().trim_start_matches("grammars/tree-sitter-").trim_end_matches(".wasm").to_string();
    assert_eq!(symbol, l.language, "the language is named as its grammar's symbol");
    let language = store.load_language(&symbol, &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (engine, language, format!("queries/{}", l.language))
}

fn ocaml() -> (wasmtime::Engine, Language) {
    let (e, l, q) = load(include_str!("../languages/ocaml.toml"));
    assert_eq!(q, "queries/ocaml");
    (e, l)
}

fn interface() -> (wasmtime::Engine, Language) {
    let (e, l, q) = load(include_str!("../languages/ocaml_interface.toml"));
    assert_eq!(q, "queries/ocaml_interface");
    (e, l)
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
fn highlighted(language: &Language, tree: &tree_sitter::Tree, query: &str, source: &str) -> Vec<(String, String)> {
    let q = Query::new(language, query).unwrap_or_else(|e| panic!("{e}"));
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

/// Each name is one Fenix colours, by its first part.
fn all_known(shown: &[(String, String)]) {
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "constructor", "property", "module", "operator", "punctuation", "attribute"];
    for (_, name) in shown {
        assert!(known.contains(&name.split('.').next().unwrap()), "{name}");
    }
}

/// Whether Enter at the end of the line holding `at` indents, as
/// `fenix-syntax`'s `indents_after` works it out: an `@indent` starting on
/// that line, before the cursor, and ending at it or after.
fn indents_after(language: &Language, tree: &tree_sitter::Tree, source: &str, at: &str) -> bool {
    let start = source.find(at).unwrap_or_else(|| panic!("{at}"));
    let byte = source[start..].find('\n').map_or(source.len(), |n| start + n);
    let line_start = source[..byte].rfind('\n').map_or(0, |i| i + 1);
    let q = Query::new(language, include_str!("../queries/ocaml/indents.scm")).unwrap_or_else(|e| panic!("{e}"));
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
fn ocamls_grammar_loads_and_its_highlights_run() {
    let (engine, language) = ocaml();
    let source = include_str!("samples/orbit.ml");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let shown = highlighted(&language, &tree, include_str!("../queries/ocaml/highlights.scm"), source);
    let has = |text: &str, name: &str| shown.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [
        ("(** Orbits, kept by name. *)", "comment.documentation"), ("(* Two at once. *)", "comment"), ("open", "keyword.import"), ("Printf", "module"), ("type", "keyword"), ("let", "keyword"), ("in", "keyword"),
        ("kind", "type"), ("Leo", "constructor"), ("deriving", "attribute"), ("string", "type.builtin"), ("float", "type.builtin"), ("mutable", "keyword.modifier"), ("crew", "property"),
        ("exception", "keyword.exception"), ("Too_many", "constructor"), ("max_crew", "variable"), ("module", "keyword"), ("struct", "keyword"), ("Registry", "module"), ("TRACKER", "module"),
        ("add", "function"), ("kind_of", "function"), ("describe", "function"), ("count", "function"), ("track", "function"), ("sprintf", "function.call"), ("print_endline", "function.call"),
        ("raise", "function.builtin"), ("failwith", "function.builtin"), ("o", "variable.parameter"), ("altitude", "variable.parameter"), ("known", "variable.parameter"),
        ("if", "keyword.conditional"), ("then", "keyword.conditional"), ("else", "keyword.conditional"), ("match", "keyword.conditional"), ("when", "keyword.conditional"),
        ("for", "keyword.repeat"), ("do", "keyword.repeat"), ("done", "keyword.repeat"), ("while", "keyword.repeat"), ("function", "keyword.function"), ("fun", "keyword.function"), ("try", "keyword.exception"),
        ("method", "keyword"), ("see", "function.method"), ("class", "keyword"), ("tracker", "type"),
        ("\"ISS\"", "string"), ("'x'", "string"), ("%s", "string.special"), ("\\n", "string.escape"), ("7", "number"), ("0x2A", "number"), ("5520.", "number.float"), ("1.5e3", "number.float"),
        ("true", "boolean"), ("false", "boolean"), ("|>", "operator"), ("::", "operator"), ("->", "punctuation.delimiter"), ("(", "punctuation.bracket"), ("[@@", "punctuation.special"),
    ] {
        assert!(has(text, name), "{text} as {name}: {:?}", shown.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    assert!(!has("0x2A", "number.float"), "hex isn't a float for its digits");
    all_known(&shown);
}

#[test]
fn interfaces_have_their_own_grammar_and_the_same_queries() {
    let (engine, language) = interface();
    let source = include_str!("samples/orbit.mli");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
    let highlights = include_str!("../queries/ocaml_interface/highlights.scm");
    let shown = highlighted(&language, &tree, highlights, source);
    let has = |text: &str, name: &str| shown.iter().any(|(t, n)| t == text && n == name);
    for (text, name) in [("val", "keyword"), ("max_crew", "function"), ("kind_of", "function"), ("sig", "keyword"), ("Registry", "module"), ("t", "type"), ("int", "type.builtin"), ("prefix", "property"), ("(** Orbits, kept by name. *)", "comment.documentation")] {
        assert!(has(text, name), "{text} as {name}: {:?}", shown.iter().filter(|(t, _)| t == text).collect::<Vec<_>>());
    }
    all_known(&shown);
    // The implementations' queries, but for the node the interface
    // grammar hasn't.
    let strip = |s: &str| s.lines().filter(|l| !l.starts_with(';')).collect::<Vec<_>>().join("\n").replace(" (shebang)", "");
    assert_eq!(strip(include_str!("../queries/ocaml/highlights.scm")), strip(highlights));
    assert_eq!(include_str!("../queries/ocaml/folds.scm"), include_str!("../queries/ocaml_interface/folds.scm"));
    assert_eq!(include_str!("../queries/ocaml/indents.scm"), include_str!("../queries/ocaml_interface/indents.scm"));
    assert!(Query::new(&language, include_str!("../queries/ocaml/highlights.scm")).is_err(), "the implementations' own wouldn't load: no (shebang)");
    for q in [include_str!("../queries/ocaml_interface/folds.scm"), include_str!("../queries/ocaml_interface/indents.scm")] {
        Query::new(&language, q).unwrap_or_else(|e| panic!("{e}"));
    }
    let folds = captures(&language, &tree, include_str!("../queries/ocaml_interface/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    assert!(folded.contains(&"module Registry : sig") && folded.contains(&"type orbit = {"), "{folded:?}");
}

#[test]
fn what_folds_and_where_enter_indents() {
    let (engine, language) = ocaml();
    let source = include_str!("samples/orbit.ml");
    let tree = parse(&engine, &language, source);

    // What folds: each let, at the top and in an expression; the types,
    // the modules, the class; the blocks.
    let folds = captures(&language, &tree, include_str!("../queries/ocaml/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n)| n == "fold").map(|(t, _)| t.lines().next().unwrap()).collect();
    for start in [
        "type orbit = {", "module Registry = struct", "let add (o : orbit) (known : t) =", "module type TRACKER = sig", "let kind_of altitude =", "match altitude with", "class tracker name = object (self)",
        "let rec count = function", "let () =", "for i = 1 to 3 do",
    ] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents after a line that ends in what opens a block, not
    // after a one-line definition.
    for (line, indents) in [
        ("type orbit = {", true), ("module Registry = struct", true), ("let add (o : orbit) (known : t) =", true), ("module type TRACKER = sig", true), ("let kind_of altitude =", true),
        ("match altitude with", false), ("| a when a < 2000. -> Leo", false), ("for i = 1 to 3 do", true), ("let () =", true), ("let rec count = function", false),
        ("let max_crew = 7", false), ("type kind = Leo | Meo | Geo", false), ("let empty : t = M.empty", false), ("exception Too_many of int", false), ("val mutable seen = 0", false),
    ] {
        assert_eq!(indents_after(&language, &tree, source, line), indents, "{line}");
    }
    for (fresh, line) in [("let f x =\n  x\n", "let f x ="), ("let g = function\n  | Some x ->\n      x\n  | None -> 0\n", "  | Some x ->"), ("let xs = [\n  1;\n]\n", "let xs = ["), ("let () = if true then\n  ()\n", "if true then")] {
        let tree = parse(&engine, &language, fresh);
        assert!(indents_after(&language, &tree, fresh, line), "{fresh:?}: {}", tree.root_node().to_sexp());
    }
}

#[test]
fn newer_ocaml_parses_too() {
    let (engine, language) = ocaml();
    let source = "type _ Effect.t += Ask : int Effect.t\n\nlet ( let* ) = Option.bind\n\nlet run f =\n  match f () with\n  | v -> v\n  | effect Ask, k -> Effect.Deep.continue k 42\n\nlet sum = let* a = Some 1 in let* b = Some 2 in Some (a + b)\n\nlet s = {|raw \"text\"|}\nlet%expect_test \"x\" = print_endline \"x\"; [%expect {| x |}]\n";
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

#[test]
fn the_dune_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    let t = load_template_dir(&dir("templates").join("ocaml-dune"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers("orbit-desk");
    let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    let all: Vec<&fenix_project::template::PlannedFile> = plan.files.iter().chain(&plan.after_files).collect();
    let paths: Vec<&str> = all.iter().map(|f| f.path.as_str()).collect();
    for path in ["dune-project", "bin/dune", "bin/main.ml", "lib/dune", "lib/greeting.ml", "lib/greeting.mli", "test/dune", "test/test_orbit_desk.ml", ".ocamlformat", ".gitignore", "README.md"] {
        assert!(paths.contains(&path), "{path}: {paths:?}");
    }
    for f in &all {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    // What the pack reads of it, written out: its dune files, its
    // executable (bytecode, to debug), its tests and its .ocamlformat.
    let out = parent.join("written");
    for f in &all {
        let path = out.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &f.contents).unwrap();
    }
    let project = crate::project::Project::read(&out.to_string_lossy());
    assert_eq!(project.dune_lang(), Some("3.0"));
    assert_eq!(project.dune_project.as_ref().and_then(|d| d.name.as_deref()), Some("orbit-desk"));
    let exes: Vec<(&str, String, bool)> = project.executables.iter().map(|e| (e.label(), e.path("bc"), e.byte)).collect();
    assert_eq!(exes, [("orbit-desk", "bin/main.bc".to_string(), true)]);
    assert_eq!(project.test_dirs, ["test"]);
    assert!(project.ocamlformat && project.ocamlformat_version.is_none());
    assert!(!all.iter().find(|f| f.path == "dune-project").unwrap().contents.contains("authors"), "no author given: none written");
    let (engine, language) = ocaml();
    for f in all.iter().filter(|f| f.path.ends_with(".ml")) {
        let tree = parse(&engine, &language, &f.contents);
        assert!(!tree.root_node().has_error(), "{}: {}", f.path, tree.root_node().to_sexp());
    }
    let (engine, language) = interface();
    let mli = all.iter().find(|f| f.path.ends_with(".mli")).unwrap();
    assert!(!parse(&engine, &language, &mli.contents).root_node().has_error());
    std::fs::remove_dir_all(&parent).unwrap();
}
