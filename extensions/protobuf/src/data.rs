//! The pack's data, read as Fenix reads it: its snippets by Fenix's own
//! crate, its language file by the same rules as `fenix-gui`'s
//! (`languages::File`), the manifest's commands against the code's, and
//! the proto grammar loaded and its queries run as `fenix-syntax` runs
//! them -- tree-sitter 0.26's WebAssembly store.

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

fn render(text: &str) -> String {
    fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/proto", "snippet");
    assert!(snippets.len() >= 12);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["proto"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${") && !shown.contains('$'), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["message", "enum", "service", "rpc", "oneof", "map", "option", "import", "syntax"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_snippets_mirror_their_names_and_parse_as_proto() {
    assert_eq!(render(include_str!("../snippets/proto/rpc.snippet")), "rpc Method(MethodRequest) returns (MethodResponse);", "the request and response mirror the method");
    assert_eq!(render(include_str!("../snippets/proto/rpcs.snippet")), "rpc Watch(WatchRequest) returns (stream WatchResponse);");
    assert_eq!(render(include_str!("../snippets/proto/enum.snippet")), "enum Kind {\n  KIND_UNSPECIFIED = 0;\n  KIND_FIRST = 1;\n  \n}", "buf's zero value");
    assert_eq!(render(include_str!("../snippets/proto/map.snippet")), "map<string, string> labels = 1;");

    // Put together, a file the grammar reads whole.
    let (engine, language) = proto();
    let body = ["message", "oneof", "map", "field", "repeated", "reserved"].map(|k| render(&std::fs::read_to_string(dir(&format!("snippets/proto/{k}.snippet"))).unwrap())).join("\n");
    let source = format!(
        "{}\n{}\n{}\nmessage Outer {{\n{body}\n}}\n{}\n{}\n",
        render(include_str!("../snippets/proto/syntax.snippet")),
        render(include_str!("../snippets/proto/import.snippet")),
        render(include_str!("../snippets/proto/option.snippet")),
        render(include_str!("../snippets/proto/enum.snippet")),
        render(include_str!("../snippets/proto/service.snippet")),
    );
    let tree = parse(&engine, &language, &source);
    assert!(!tree.root_node().has_error(), "{source}\n{}", tree.root_node().to_sexp());
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

/// The proto grammar, as Fenix loads it.
fn proto() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/proto.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref(), l.symbol.as_deref(), l.queries.as_deref()), ("proto", None, None, None), "the server is the code's to set; queries/proto");
    assert_eq!((l.extensions.as_slice(), l.comment.as_deref()), (["proto".to_string()].as_slice(), Some("//")));
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the language's name.
    let language = store.load_language("proto", &wasm).unwrap();
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
fn the_proto_grammar_loads_and_its_queries_run() {
    let (engine, language) = proto();
    let source = include_str!("samples/acme.proto");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/proto/highlights.scm"), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("syntax", "keyword.directive"),
        ("\"proto3\"", "string.special.symbol"),
        ("package", "keyword.import"),
        ("orders", "module"),
        ("import", "keyword.import"),
        ("\"google/protobuf/timestamp.proto\"", "string.special.path"),
        ("public", "keyword.modifier"),
        ("option", "keyword.directive"),
        ("go_package", "property"),
        ("message", "keyword.type"),
        ("Order", "type"),
        ("reserved", "keyword"),
        ("to", "keyword"),
        ("\"legacy_id\"", "string"),
        ("string", "type.builtin"),
        ("lines", "property"),
        ("repeated", "keyword.modifier"),
        ("optional", "keyword.modifier"),
        ("map", "keyword.type"),
        ("int64", "type.builtin"),
        ("json_name", "property"),
        ("redact", "variable.member"),
        ("oneof", "keyword.type"),
        ("payer", "type"),
        ("card", "property"),
        ("enum", "keyword.type"),
        ("STATE_OPEN", "constant"),
        ("service", "keyword.type"),
        ("OrderService", "type"),
        ("rpc", "keyword.function"),
        ("PlaceOrder", "function.method"),
        ("returns", "keyword.return"),
        ("stream", "keyword.modifier"),
        ("true", "boolean"),
        ("1", "number"),
        ("\\t", "string.escape"),
        ("// An order service, to colour.", "comment"),
        ("=", "operator"),
        ("{", "punctuation.bracket"),
        (";", "punctuation.delimiter"),
    ] {
        assert_eq!(shown(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation"];
    for (_, name, _) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: the declarations, a oneof, an rpc's options.
    let folds = captures(&language, &tree, include_str!("../queries/proto/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["message Order {", "message Line {", "oneof payer {", "enum State {", "service OrderService {", "rpc WatchOrders(WatchOrdersRequest) returns (stream Order) {"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }

    // Enter indents inside a body: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/proto/indents.scm"), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    for body in ["{\n  reserved 4, 9 to 11;", "{\n    string sku = 1;", "{\n  STATE_UNSPECIFIED = 0;", "service OrderService {"] {
        assert!(indents.iter().any(|(t, _, _)| t.starts_with(body)), "{body}: {indents:?}");
    }
    assert!(indents.iter().any(|(t, _, p)| t == "{" && *p == 1), "an rpc's options: its brace");
}

#[test]
fn the_buf_project_in_the_samples_reads_whole() {
    let (engine, language) = proto();
    let source = include_str!("samples/orbit.proto");
    assert!(!parse(&engine, &language, source).root_node().has_error());
}

/// The manifest, as far as the tests read it.
#[derive(Deserialize)]
struct Manifest {
    contributes: Contributes,
}

#[derive(Deserialize)]
struct Contributes {
    commands: Vec<Named>,
    settings: Vec<Setting>,
}

#[derive(Deserialize)]
struct Named {
    id: String,
}

#[derive(Deserialize)]
struct Setting {
    key: String,
}

#[test]
fn the_manifest_lists_every_command_and_setting_the_code_uses() {
    let m: Manifest = toml::from_str(include_str!("../extension.toml")).unwrap();
    let ids: Vec<&str> = m.contributes.commands.iter().map(|c| c.id.as_str()).collect();
    for id in ["protobuf.doctor", "protobuf.install.buf", "protobuf.install.grpcurl", "protobuf.install.protoc", "protobuf.client", "protobuf.invoke", "protobuf.cancel", "protobuf.target", "protobuf.breaking"] {
        assert!(ids.contains(&id), "{id}");
    }
    let keys: Vec<&str> = m.contributes.settings.iter().map(|s| s.key.as_str()).collect();
    let code = [include_str!("lib.rs")].concat();
    for key in keys.iter() {
        assert!(code.contains(&format!("\"{key}\"")), "{key} read");
    }
    for key in ["protobuf.lint", "protobuf.server", "protobuf.breaking_against", "protobuf.breaking_check", "protobuf.targets", "protobuf.insecure", "protobuf.timeout", "protobuf.history"] {
        assert!(keys.contains(&key), "{key} in the manifest");
    }
    let keymap = include_str!("../keymaps/proto.toml");
    for id in keymap.lines().filter_map(|l| l.strip_prefix("command = \"")?.strip_suffix('"')) {
        assert!(ids.contains(&id), "the keymap's {id}");
    }
}
