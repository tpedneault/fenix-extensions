//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language file by the same rules as
//! `fenix-gui`'s (`languages::File`), the grammar loaded and its queries
//! run as `fenix-syntax` runs them -- tree-sitter 0.26's WebAssembly store.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::Digest;
use tree_sitter::{wasmtime, Language, Parser, Query, QueryCursor, StreamingIterator, WasmStore};

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

/// `fenix-gui`'s `languages::File`: a field it doesn't know is refused.
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

fn language_file(name: &str) -> LanguageFile {
    toml::from_str(&std::fs::read_to_string(dir("languages").join(format!("{name}.toml"))).unwrap()).unwrap_or_else(|e| panic!("{name}: {}", e.message()))
}

/// The grammar as Fenix loads it from a language file: the file's, the
/// `tree_sitter_<symbol>` it exports, the queries' folder.
fn load(name: &str) -> (wasmtime::Engine, Language, PathBuf) {
    let l = language_file(name);
    assert_eq!(l.language, name);
    let grammar = l.grammar.as_deref().unwrap_or_else(|| panic!("{name}: a grammar"));
    let wasm = std::fs::read(dir(grammar)).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let stem = Path::new(grammar).file_stem().unwrap().to_string_lossy().into_owned();
    let symbol = l.symbol.clone().unwrap_or_else(|| stem.strip_prefix("tree-sitter-").unwrap_or(&stem).replace('-', "_"));
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    let language = store.load_language(&symbol, &wasm).unwrap_or_else(|e| panic!("{name}: tree_sitter_{symbol}: {e}"));
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    let queries = dir(l.queries.as_deref().map(str::to_string).unwrap_or_else(|| format!("queries/{name}")).trim_end_matches('/'));
    (engine, language, queries)
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
fn the_language_file_reads_as_fenix_reads_it() {
    let all = files("languages", "toml");
    assert_eq!(all.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["typst.toml"]);
    let l = language_file("typst");
    assert_eq!((l.extensions.as_slice(), l.comment.as_deref()), (["typ".to_string()].as_slice(), Some("//")));
    assert!(l.server.is_none(), "the server is the code's to set");
    assert_eq!(crate::SERVED, ["typst"], "the name tinymist is told");
}

#[test]
fn the_grammars_licence_and_source_are_beside_it() {
    let readme = std::fs::read_to_string(dir("grammars/README.md")).unwrap();
    let bytes = std::fs::read(dir("grammars/tree-sitter-typst.wasm")).unwrap();
    assert!(bytes.starts_with(b"\0asm"));
    let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert!(readme.contains(&sha), "its SHA-256 {sha} is in the README");
    assert!(readme.contains("46cf4ded12ee974a70bf8457263b67ad7ee0379d"), "the commit it was built from");
    let licence = std::fs::read_to_string(dir("grammars/LICENSE-tree-sitter-typst")).unwrap();
    assert!(licence.starts_with("MIT License") && licence.contains("Gerbais-Nief Eddie"));
    assert!(readme.contains("MIT"));
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/typst", "snippet");
    assert!(snippets.len() >= 20, "{}", snippets.len());
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["typst"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("$0") && !shown.contains("${1") && !shown.contains("$1"), "{file}: a field left: {shown}");
        assert!(!text.contains('\t'), "{file}: Typst is indented with spaces");
        keys.push(s.trigger);
    }
    for key in ["doc", "h1", "fig", "tab", "eq", "let", "fn", "show", "set", "tmpl", "imp", "pkg", "bib", "raw"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    // A dollar sign in a snippet is math, written `\$`: it comes out as one.
    let eq = fenix_snippets::Snippet::parse(&std::fs::read_to_string(dir("snippets/typst/eq.snippet")).unwrap()).unwrap();
    assert!(eq.template.render(&Default::default(), &Default::default(), "").text.starts_with("$ E = m c^2 $"));
}

#[test]
fn typsts_grammar_loads_and_its_queries_run() {
    let (engine, language, queries) = load("typst");
    let source = include_str!("samples/report.typ");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    let mut wrong: Vec<String> = Vec::new();
    for (text, name) in [
        ("// A report: markup, code and math, for the grammar's queries.", "comment"),
        ("import", "keyword"),
        ("set", "keyword"),
        ("let", "keyword"),
        ("show", "keyword"),
        ("for", "keyword"),
        ("if", "keyword"),
        ("else", "keyword"),
        ("context", "keyword"),
        ("\"@preview/cetz:0.3.0\"", "string"),
        ("page", "function"),
        ("rgb", "function"),
        ("range", "function"),
        ("double", "function"),
        ("callout", "function"),
        ("accent", "variable"),
        ("true", "boolean"),
        ("3", "number"),
        ("11pt", "number"),
        ("= Introduction <intro>", "text.title"),
        ("*strong*", "text.strong"),
        ("_emphasised_", "text.emphasis"),
        ("https://typst.app", "text.uri"),
        ("@intro", "text.reference"),
        ("<intro>", "label"),
        ("`raw`", "text.literal"),
        ("\\#", "string.escape"),
        ("*", "operator"),
        ("$", "punctuation.special"),
        ("#", "punctuation.special"),
    ] {
        if shown(text) != Some(name) {
            wrong.push(format!("{text}: wanted {name}, got {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>()));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation", "text", "markup", "parameter"];
    for (_, name, _) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: a heading's section, blocks, content, a raw block.
    let folds = captures(&language, &tree, &std::fs::read_to_string(queries.join("folds.scm")).unwrap(), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["== A second level", "{", "```rust"] {
        assert!(folded.iter().any(|f| f.starts_with(start) || f.ends_with(start)), "{start}: {folded:?}");
    }
    assert!(folded.iter().any(|f| f.starts_with("= Introduction")), "the first heading's section: {folded:?}");

    // Enter indents inside a block or a call: an @indent starting on that line.
    let indents = captures(&language, &tree, &std::fs::read_to_string(queries.join("indents.scm")).unwrap(), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("{\n  [Item")), "a block: {}", indents.len());
}

#[test]
fn the_article_and_package_templates_plan() {
    use fenix_project::template::{load_template_dir, Answer, Origin};
    let parent = crate::project::tests::temp("templates");
    let root = dir("templates");

    let article = load_template_dir(&root.join("typst-article"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(article.needs, ["typst"]);
    for paper in ["a4", "us-letter"] {
        let mut answers = article.default_answers("orbit-notes");
        answers.insert("paper".into(), Answer::Text(paper.into()));
        answers.insert("author".into(), Answer::Text("Grace Hopper".into()));
        let plan = article.plan("orbit-notes", &parent, &answers).unwrap_or_else(|e| panic!("{paper}: {e}"));
        assert!(plan.steps.is_empty(), "it runs nothing");
        let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{path}")).contents.clone();
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{}", f.path);
        }
        let main = file("main.typ");
        assert!(main.contains(&format!("paper: \"{paper}\"")) && main.contains("author: \"Grace Hopper\"") && main.contains("#bibliography(\"refs.bib\""), "{paper}");
        assert!(file("refs.bib").contains("@book{knuth84"));
        assert!(file(".gitignore").lines().any(|l| l == "*.pdf"));
    }

    let package = load_template_dir(&root.join("typst-package"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    let mut answers = package.default_answers("Orbit Tools");
    answers.insert("license".into(), Answer::Text("Apache-2.0".into()));
    let plan = package.plan("Orbit Tools", &parent, &answers).unwrap();
    let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{path}")).contents.clone();
    let manifest = file("typst.toml");
    let m: toml::Table = toml::from_str(&manifest).unwrap();
    let p = &m["package"];
    assert_eq!((p["name"].as_str(), p["version"].as_str(), p["entrypoint"].as_str(), p["license"].as_str()), (Some("orbit-tools"), Some("0.1.0"), Some("lib.typ"), Some("Apache-2.0")));
    assert!(file("lib.typ").contains("@preview/orbit-tools:0.1.0") && file("README.md").contains("@preview/orbit-tools:0.1.0"));
    for f in plan.files.iter().chain(&plan.after_files) {
        assert!(!f.contents.contains("{{"), "{}", f.path);
    }
    std::fs::remove_dir_all(&parent).unwrap();
}

/// The manifest's table `name`, as an array of strings.
fn strings(manifest: &toml::Table, table: &str, key: &str) -> Vec<String> {
    manifest[table][key].as_array().unwrap().iter().filter_map(|p| p.as_str().map(str::to_string)).collect()
}

fn manifest() -> toml::Table {
    toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap()
}

#[test]
fn the_manifest_lists_the_commands_the_kit_the_pack_and_the_keymap_use() {
    let m = manifest();
    let commands: Vec<String> = m["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(
        commands,
        ["typst.doctor", "typst.install.typst", "typst.install.tinymist", "typst.install.typstyle", "typst.compile", "typst.watch", "typst.preview"]
    );
    for (file, text) in files("keymaps", "toml") {
        let keymap: toml::Table = toml::from_str(&text).unwrap();
        for (_, menu) in keymap["local"].as_table().unwrap() {
            for key in menu.as_array().unwrap() {
                let command = key["command"].as_str().unwrap();
                assert!(commands.iter().any(|c| c == command), "{file}: {command} isn't in extension.toml");
            }
        }
    }
    // Each fix row runs a command the manifest lists.
    let programs = crate::doctor::programs(&crate::project::Project { main: Some("main.typ".into()), typst: true }, &crate::tools::Platform::new("linux", "x86_64"));
    for (_, command, _) in programs.iter().filter_map(|p| p.fix.clone()) {
        assert!(commands.contains(&command), "{command}");
    }
}

#[test]
fn the_manifest_asks_for_what_the_pack_runs_and_downloads() {
    let m = manifest();
    assert_eq!(m["extension"]["id"].as_str(), Some("fenix.typst"));
    assert_eq!((m["engine"]["fenix"].as_str(), m["engine"]["api"].as_str()), (Some(">=1.5, <2"), Some("1.5")));
    let process = strings(&m, "permissions", "process");
    assert_eq!(process, ["typst", "tinymist", "typstyle", "/tools/*"]);
    // Every program a task and the compile commands run is named.
    let tools = crate::tools::Tools { typstyle: Some("typstyle".into()), ..Default::default() };
    let tasks = crate::tasks::tasks(&crate::project::Project { main: Some("main.typ".into()), typst: true }, &tools);
    for t in &tasks.tasks {
        assert!(process.contains(&t.cmd.program), "{}", t.cmd.program);
    }
    // Every host a release is downloaded from is named, and no other.
    let hosts = strings(&m, "permissions", "network");
    assert_eq!(hosts, ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"]);
    for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64")] {
        for i in crate::tools::Platform::new(os, arch).installs() {
            let host = i.url.strip_prefix("https://").unwrap().split('/').next().unwrap();
            assert!(hosts.iter().any(|h| h == host), "{}", i.url);
        }
    }
    assert_eq!((m["permissions"]["filesystem"].as_str(), m["permissions"]["editor"].as_str()), (Some("project-read"), Some("read")));
    // The settings it reads are the manifest's, with their defaults as text.
    let settings: Vec<(&str, &str)> = m["contributes"]["settings"].as_array().unwrap().iter().map(|s| (s["key"].as_str().unwrap(), s["default"].as_str().unwrap())).collect();
    assert_eq!(settings, [("typst.main", ""), ("typst.preview", "true"), ("typst.compile_on_save", "false")]);
    assert_eq!(crate::Settings::default(), crate::Settings { main: None, preview: true, compile_on_save: false });
    // Its context loads it for the language, and for a project's manifest.
    let context = &m["contributes"]["contexts"].as_array().unwrap()[0];
    assert_eq!(context["languages"].as_array().unwrap()[0].as_str(), Some("typst"));
    assert_eq!(context["markers"].as_array().unwrap()[0].as_str(), Some("typst.toml"));
    // The project kind.
    let kind = &m["contributes"]["project_kinds"].as_array().unwrap()[0];
    assert_eq!((kind["id"].as_str(), kind["tag"].as_str(), kind["main"].as_array().unwrap()[0].as_str()), (Some("typst"), Some("TYP"), Some("main.typ")));
    // The PDF reader isn't a dependency: it's asked for when it's there.
    assert!(m.get("dependencies").is_none());
}
