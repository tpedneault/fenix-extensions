//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language and launch files by the
//! same rules as `fenix-gui`'s, its manifest against the keymap and the
//! code, and Dart's grammar loaded and its queries run as `fenix-syntax`
//! runs them -- tree-sitter 0.26's WebAssembly store.

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

/// `fenix-gui`'s `dap::Template`: what a launch file may say.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchFile {
    name: String,
    languages: Vec<String>,
    request: Option<String>,
    program: Option<toml::Value>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    attributes: toml::Table,
    #[serde(rename = "preLaunchTask")]
    pre_launch_task: Option<String>,
}

fn manifest() -> toml::Table {
    toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap()
}

#[test]
fn the_language_file_reads_as_fenix_reads_it() {
    let l: LanguageFile = toml::from_str(include_str!("../languages/dart.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref()), ("dart", None), "the server is the code's to set");
    assert_eq!(l.extensions, ["dart"]);
    assert_eq!(l.comment.as_deref(), Some("//"), "gcc in Dart, which has none of Fenix's own");
    assert!(l.symbol.is_none() && l.queries.is_none(), "tree_sitter_dart, queries/dart");
    assert_eq!(files("languages", "toml").len(), 2);
}

#[test]
fn the_yaml_file_adds_the_lockfile_to_fenixs_yaml() {
    let l: LanguageFile = toml::from_str(include_str!("../languages/yaml.toml")).unwrap();
    assert_eq!((l.language.as_str(), l.filenames.as_slice()), ("yaml", ["pubspec.lock".to_string()].as_slice()));
    assert!(l.server.is_none() && l.grammar.is_none() && l.comment.is_none(), "the rest is Fenix's");
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/dart", "snippet");
    assert!(snippets.len() >= 20);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["dart"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains('\t'), "{file}: Dart's indented with spaces");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        // A field left would be `${1...}` or `$1`; Dart's own `${...}` is fine.
        assert!(!shown.contains("${1") && !shown.contains("${2") && !shown.contains("${0"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["main", "mains", "class", "data", "enum", "sealed", "ext", "mixin", "rec", "fut", "stream", "trycatch", "switch", "test", "group", "testw", "stless", "stful", "build", "init", "setstate", "scaffold", "futurebuilder", "runapp"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn snippets_keep_darts_dollar_for_interpolation() {
    let render = |text: &str| fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text;
    let data = render(include_str!("../snippets/dart/data.snippet"));
    assert!(data.contains("String toString() => 'Point(x: $x, y: $y)';"), "{data}");
    let builder = render(include_str!("../snippets/dart/futurebuilder.snippet"));
    assert!(builder.contains("Text('${snapshot.error}')"), "{builder}");
    let stful = render(include_str!("../snippets/dart/stful.snippet"));
    assert!(stful.contains("State<MyWidget> createState() => _MyWidgetState();") && stful.contains("class _MyWidgetState extends State<MyWidget> {"), "{stful}");
    let sealed = render(include_str!("../snippets/dart/sealed.snippet"));
    assert!(sealed.starts_with("sealed class Shape {}") && sealed.contains("Circle(:final radius) =>"), "{sealed}");
}

#[test]
fn the_launch_files_read_as_fenix_reads_them() {
    let launches = files("launch", "toml");
    assert_eq!(launches.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["attach.toml", "run-file.toml"]);
    for (file, text) in &launches {
        let l: LaunchFile = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {}", e.message()));
        assert_eq!(l.languages, ["dart"], "{file}");
        assert!(l.name.starts_with("Dart: "), "{file}");
    }
    let run: LaunchFile = toml::from_str(include_str!("../launch/run-file.toml")).unwrap();
    assert_eq!(run.program, Some(toml::Value::String("${file}".into())));
    let attach: LaunchFile = toml::from_str(include_str!("../launch/attach.toml")).unwrap();
    assert_eq!(attach.request.as_deref(), Some("attach"));
    assert_eq!(attach.attributes["vmServiceInfoFile"].as_str(), Some("${project}/.dart_tool/fenix-vm-service.json"));
    // The file the `run (attachable)` task writes.
    assert!(crate::tasks::tasks(&crate::project::Project { root: "/p".into(), pubspec: Some(crate::project::Pubspec { name: "x".into(), ..Default::default() }), bins: vec!["x".into()], ..Default::default() }, &crate::tools::Tools::default(), "linux").resolved().iter().any(|t| t.name == "run (attachable)" && t.cmd.args.iter().any(|a| a == "--write-service-info=.dart_tool/fenix-vm-service.json")));
}

#[test]
fn the_manifest_names_what_the_code_runs_and_the_keymap_uses() {
    let m = manifest();
    let process: Vec<&str> = m["permissions"]["process"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
    for program in ["dart", "flutter", "kill"] {
        assert!(process.contains(&program), "permissions.process names {program}");
    }
    assert_eq!(m["permissions"]["filesystem"].as_str(), Some("project-read"));
    assert!(m["permissions"].get("network").is_none() && m["permissions"].get("editor").is_none(), "nothing it doesn't use");

    let commands: Vec<String> = m["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    for own in ["dart.packages", "dart.doctor", crate::doctor::PUB_GET, crate::doctor::GET_DART, crate::doctor::GET_FLUTTER, crate::DEVICES, crate::RUN_ON_DEVICE, crate::RUN_IN_TERMINAL, crate::STOP, crate::HOT_RELOAD, crate::HOT_RESTART] {
        assert!(commands.iter().any(|c| c == own), "{own} isn't in extension.toml");
    }
    assert_eq!(commands.len(), 11, "and no command nothing answers: {commands:?}");
    for (file, text) in files("keymaps", "toml") {
        let keymap: toml::Table = toml::from_str(&text).unwrap();
        for (_, menu) in keymap["local"].as_table().unwrap() {
            for key in menu.as_array().unwrap() {
                let command = key["command"].as_str().unwrap();
                assert!(commands.iter().any(|c| c == command), "{file}: {command} isn't in extension.toml");
                assert_ne!(command, "dart.fix", "dart fix --apply is not a one-key default");
            }
        }
    }
    let settings: Vec<&str> = m["contributes"]["settings"].as_array().unwrap().iter().map(|s| s["key"].as_str().unwrap()).collect();
    assert_eq!(settings, ["dart.sdk", "dart.flutter"]);
    let process_settings: Vec<String> = settings.iter().map(|s| format!("${{setting:{s}}}")).collect();
    assert!(process_settings.iter().all(|s| process.contains(&s.as_str())), "a program a setting names may run");
    assert_eq!([crate::tools::DART_SETTING, crate::tools::FLUTTER_SETTING], ["${setting:dart.sdk}", "${setting:dart.flutter}"]);
    assert_eq!(m["contributes"]["contexts"].as_array().unwrap()[0]["markers"].as_array().unwrap()[0].as_str(), Some("pubspec.yaml"));
}

/// Dart's grammar, as Fenix loads it.
fn dart() -> (wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(include_str!("../languages/dart.toml")).unwrap();
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.starts_with(b"\0asm"));
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the file's name.
    let language = store.load_language("dart", &wasm).unwrap();
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
fn darts_grammar_loads_and_its_queries_run() {
    let (engine, language) = dart();
    let source = include_str!("samples/orbit.dart");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/dart/highlights.scm"), source);
    // What Fenix shows of the first `text`: of the captures of just it,
    // the latest pattern's.
    let last = |text: &str| {
        // `name#2` is the text's second place, for one that's used before it's declared.
        let (text, nth) = text.split_once('#').map_or((text, 0), |(t, n)| (t, n.parse::<usize>().unwrap_or(1) - 1));
        let mut places: Vec<usize> = highlights.iter().filter(|(t, ..)| t == text).map(|(_, _, at, _)| *at).collect();
        places.sort_unstable();
        places.dedup();
        let at = *places.get(nth)?;
        highlights.iter().filter(|(t, _, a, _)| t == text && *a == at).max_by_key(|(.., p)| *p).map(|(_, n, ..)| n.as_str())
    };
    for (text, name) in [
        ("class", "keyword"),
        ("enum", "keyword"),
        ("sealed", "keyword"),
        ("mixin", "keyword"),
        ("extension", "keyword"),
        ("import", "keyword"),
        ("late", "keyword"),
        ("final", "keyword"),
        ("const", "keyword"),
        ("if", "conditional"),
        ("switch", "conditional"),
        ("when", "conditional"),
        ("for", "repeat"),
        ("while", "repeat"),
        ("return", "keyword.return"),
        ("try", "keyword.exception"),
        ("rethrow", "keyword.exception"),
        ("await", "keyword.coroutine"),
        ("async", "keyword.coroutine"),
        ("yield", "keyword.coroutine"),
        ("Orbit", "type"),
        ("Shape", "type"),
        ("double", "type.builtin"),
        ("String", "type.builtin"),
        ("Future", "type.builtin"),
        ("area", "function"),
        ("track", "function"),
        ("compareTo", "function"),
        ("altitude#2", "function"),
        ("0x2A", "number"),
        ("5520", "number"),
        ("1.5e3", "float"),
        ("true", "boolean"),
        ("this", "variable.builtin"),
        ("null", "constant.builtin"),
        ("nominal", "constant"),
        ("// Two at once.", "comment"),
        ("/// An orbit, by its period.", "comment.documentation"),
        ("@", "attribute"),
        ("immutable", "attribute"),
        ("\\n", "escape"),
        ("required", "keyword"),
        ("break", "repeat"),
    ] {
        assert_eq!(last(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, ..)| t == text.split('#').next().unwrap_or("")).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "escape", "comment", "number", "float", "boolean", "constant", "constructor", "attribute", "property", "parameter", "label", "namespace", "operator", "punctuation", "conditional", "repeat", "preproc"];
    for (_, name, ..) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: the types, their bodies and the functions'.
    let folds = captures(&language, &tree, include_str!("../queries/dart/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, ..)| n == "fold").map(|(t, ..)| t.lines().next().unwrap()).collect();
    for start in ["enum Status {", "sealed class Shape {}", "mixin Describes on Object {", "{\n  const Status(this.text, this.rank);", "{\n    return Orbit(name: json['name'] as String, period: json['period'] as double);", "{\n    for (var i = 0; i < 3; i++) {"] {
        let first = start.lines().next().unwrap();
        assert!(folded.iter().any(|f| f.starts_with(first) || *f == first), "{start}: {folded:?}");
    }

    // Enter indents inside a body or a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/dart/indents.scm"), source);
    assert!(indents.iter().all(|(_, n, ..)| n == "indent"));
    for start in ["{\n  nominal('nominal', 1)", "{\n    try {", "{\n      yield i;", "(\n    Orbit(name"] {
        let first = start.lines().next().unwrap();
        assert!(indents.iter().any(|(t, ..)| t.starts_with(first)), "{start}: {:?}", indents.iter().map(|(t, ..)| t.lines().next().unwrap_or("")).collect::<Vec<_>>());
    }
}

#[test]
fn newer_dart_parses_too() {
    let (engine, language) = dart();
    let source = "import 'package:flutter/material.dart';\n\nvoid main() => runApp(const App());\n\nclass App extends StatelessWidget {\n  const App({super.key});\n\n  @override\n  Widget build(BuildContext context) => MaterialApp(home: Scaffold(body: Center(child: Text('hi $context'))));\n}\n\nextension type const Id(int value) {}\n\nfinal r = (1, two: 'x');\nvar m = <String, Object?>{'a': [1, 2, ...[3]], 'b': null};\nint f(Object o) => switch (o) { int n when n > 1 => n, [var a, ...] => 0, _ => -1 };\n";
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

fn plan(template: &str, name: &str) -> (fenix_project::template::Plan, PathBuf) {
    let parent = crate::project::tests::temp(&format!("template-{template}"));
    let t = load_template_dir(&dir("templates").join(template), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers(name);
    let plan = t.plan(name, &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    (plan, parent)
}

#[test]
fn the_console_template_plans_with_its_defaults() {
    let (plan, parent) = plan("dart-console", "orbit-desk");
    let all: Vec<&fenix_project::template::PlannedFile> = plan.files.iter().chain(&plan.after_files).collect();
    let paths: Vec<&str> = all.iter().map(|f| f.path.as_str()).collect();
    for path in ["pubspec.yaml", "analysis_options.yaml", "bin/orbit_desk.dart", "lib/orbit_desk.dart", "test/orbit_desk_test.dart", ".gitignore", "README.md"] {
        assert!(paths.contains(&path), "{path}: {paths:?}");
    }
    for f in &all {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    // What the pack reads of it: a Dart package with a program of its name and tests.
    let pubspec = crate::project::Pubspec::parse(&all.iter().find(|f| f.path == "pubspec.yaml").unwrap().contents);
    assert_eq!(pubspec.name, "orbit_desk");
    assert!(!pubspec.is_flutter() && pubspec.tests() == Some(crate::project::Tests::Dart));
    assert_eq!(pubspec.executables, ["orbit_desk"]);
    assert_eq!(plan.steps.iter().map(|s| s.display()).collect::<Vec<_>>(), ["dart pub get"]);
    assert!(plan.file_to_open().is_none(), "nothing written yet, nothing to open");
    assert_eq!(plan.open[0], "lib/orbit_desk.dart");
    std::fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn the_library_template_plans_with_its_defaults() {
    let (plan, parent) = plan("dart-library", "orbit-kit");
    let paths: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
    for path in ["pubspec.yaml", "lib/orbit_kit.dart", "lib/src/orbit_kit_base.dart", "example/orbit_kit_example.dart", "test/orbit_kit_test.dart", "CHANGELOG.md"] {
        assert!(paths.contains(&path), "{path}: {paths:?}");
    }
    assert!(plan.files.iter().all(|f| !f.contents.contains("{{")));
    std::fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn the_flutter_template_runs_flutter_create_for_the_platforms_it_asks() {
    let parent = crate::project::tests::temp("template-flutter");
    let t = load_template_dir(&dir("templates").join("flutter-app"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.needs, ["flutter"]);
    let steps = |platforms: &str| {
        let mut answers = t.default_answers("orbit-app");
        answers.insert("platforms".into(), fenix_project::template::Answer::Text(platforms.into()));
        t.plan("orbit-app", &parent, &answers).unwrap_or_else(|e| panic!("{e}")).steps.iter().map(|s| s.display()).collect::<Vec<_>>()
    };
    assert_eq!(steps("all"), ["flutter create --project-name orbit_app --org com.example ."]);
    assert_eq!(steps("mobile"), ["flutter create --project-name orbit_app --org com.example . --platforms=android,ios"]);
    assert_eq!(steps("web"), ["flutter create --project-name orbit_app --org com.example . --platforms=web"]);
    assert_eq!(steps("desktop"), ["flutter create --project-name orbit_app --org com.example . --platforms=linux,macos,windows"]);
    let plan = t.plan("orbit-app", &parent, &t.default_answers("orbit-app")).unwrap();
    assert!(plan.files.is_empty(), "flutter create makes every file");
    std::fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn every_snippet_is_valid_dart_as_it_expands() {
    let (engine, language) = dart();
    // Where each kind sits to be Dart: a statement in a function, an
    // expression as a variable's value, a member in a class.
    let statements = ["trycatch", "switch", "forin", "test", "group", "testw", "setstate"];
    let expressions = ["scaffold", "futurebuilder"];
    let members = ["build", "init"];
    for (file, text) in files("snippets/dart", "snippet") {
        let s = fenix_snippets::Snippet::parse(&text).unwrap();
        let body = s.template.render(&Default::default(), &Default::default(), "").text;
        let key = s.trigger.as_str();
        let source = if statements.contains(&key) {
            format!("void f() {{\n{body}\n}}\n")
        } else if expressions.contains(&key) {
            format!("final x = {body};\n")
        } else if members.contains(&key) {
            format!("class A {{\n{body}\n}}\n")
        } else {
            body
        };
        let tree = parse(&engine, &language, &source);
        assert!(!tree.root_node().has_error(), "{file} expands to Dart that doesn't parse:\n{source}\n{}", tree.root_node().to_sexp());
    }
}
