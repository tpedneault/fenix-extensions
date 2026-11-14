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

/// The grammar as Fenix loads it from the language file: the file's, the
/// `tree_sitter_<symbol>` it exports (the file's name without
/// `tree-sitter-`, unless `symbol` says), the queries' folder.
fn load() -> (wasmtime::Engine, Language, PathBuf) {
    let l = language_file("scala");
    assert_eq!(l.language, "scala");
    let grammar = l.grammar.as_deref().expect("a grammar");
    let wasm = std::fs::read(dir(grammar)).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let stem = Path::new(grammar).file_stem().unwrap().to_string_lossy().into_owned();
    let symbol = l.symbol.clone().unwrap_or_else(|| stem.strip_prefix("tree-sitter-").unwrap_or(&stem).replace('-', "_"));
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    let language = store.load_language(&symbol, &wasm).unwrap_or_else(|e| panic!("tree_sitter_{symbol}: {e}"));
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    let queries = dir(l.queries.as_deref().map(str::to_string).unwrap_or_else(|| "queries/scala".into()).trim_end_matches('/'));
    (engine, language, queries)
}

fn parse(engine: &wasmtime::Engine, language: &Language, source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser.set_wasm_store(WasmStore::new(engine).unwrap()).unwrap();
    parser.set_language(language).unwrap();
    parser.parse(source, None).unwrap()
}

/// One capture: where it is, its name, and its pattern's index (a later
/// pattern wins in Fenix).
type Capture = (usize, usize, String, usize);

fn captures(language: &Language, tree: &tree_sitter::Tree, query: &str, source: &str) -> Vec<Capture> {
    let q = Query::new(language, query).unwrap_or_else(|e| panic!("{e}"));
    let names = q.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut it = cursor.captures(&q, tree.root_node(), source.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures[*i];
        out.push((c.node.start_byte(), c.node.end_byte(), names[c.index as usize].to_string(), m.pattern_index));
    }
    out
}

/// What Fenix shows the `nth` (from 0) occurrence of `text` as: the capture
/// of the latest pattern over exactly that text.
fn shown<'a>(captures: &'a [Capture], source: &str, text: &str, nth: usize) -> Option<&'a str> {
    let start = source.match_indices(text).nth(nth).unwrap_or_else(|| panic!("no {nth}th {text:?}")).0;
    captures.iter().filter(|(s, e, _, _)| *s == start && *e == start + text.len()).max_by_key(|c| c.3).map(|c| c.2.as_str())
}

/// `text` where `context` has it: the first `context` in the source, `text`'s first place in it.
fn shown_in<'a>(captures: &'a [Capture], source: &str, context: &str, text: &str) -> Option<&'a str> {
    let start = source.find(context).unwrap_or_else(|| panic!("no {context:?}")) + context.find(text).unwrap();
    captures.iter().filter(|(s, e, _, _)| *s == start && *e == start + text.len()).max_by_key(|c| c.3).map(|c| c.2.as_str())
}

fn read(path: &str) -> String {
    std::fs::read_to_string(dir(path)).unwrap()
}

fn query(name: &str) -> String {
    read(&format!("queries/scala/{name}.scm"))
}

#[test]
fn the_language_file_reads_as_fenix_reads_it() {
    let l = language_file("scala");
    assert_eq!(l.extensions, ["scala", "sc", "sbt", "mill"]);
    assert!(l.server.is_none(), "the server is the code's to set");
    assert_eq!(l.comment.as_deref(), Some("//"));
    assert_eq!(std::fs::read_dir(dir("languages")).unwrap().count(), 1);
    assert_eq!(crate::SERVED, ["scala"]);
}

#[test]
fn the_grammars_licence_and_source_are_beside_it() {
    let readme = read("grammars/README.md");
    let bytes = std::fs::read(dir("grammars/tree-sitter-scala.wasm")).unwrap();
    assert!(bytes.starts_with(b"\0asm"));
    let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert!(readme.contains(&sha), "its SHA-256 {sha} is in the README");
    let licence = read("grammars/LICENSE-tree-sitter-scala");
    assert!(licence.starts_with("The MIT License") && licence.contains("Max Brunsfeld"));
    assert!(readme.contains("MIT") && readme.contains("tree-sitter-scala-0.24.0.tgz"));
}

/// Both samples parse whole, and their queries compile.
#[test]
fn both_samples_parse_without_errors() {
    let (engine, language, _) = load();
    for f in ["orbit3.scala", "orbit2.scala"] {
        let source = read(&format!("src/samples/{f}"));
        let tree = parse(&engine, &language, &source);
        assert!(!tree.root_node().has_error(), "{f}: {}", tree.root_node().to_sexp());
    }
    // Scripts and sbt files are the same grammar.
    let sbt = "ThisBuild / scalaVersion := \"3.7.3\"\n\nlazy val root = (project in file(\".\"))\n  .settings(\n    name := \"orbit\",\n    libraryDependencies += \"org.scalameta\" %% \"munit\" % \"1.0.0\" % Test\n  )\n";
    let tree = parse(&engine, &language, sbt);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
    let h = captures(&language, &tree, &query("highlights"), sbt);
    assert_eq!(shown(&h, sbt, "\"orbit\"", 0), Some("string"));
    assert_eq!(shown(&h, sbt, ":=", 0), Some("operator"));
    assert_eq!(shown(&h, sbt, "lazy", 0), Some("keyword"));
    let script = "//> using dep com.lihaoyi::os-lib:0.11.3\nval files = os.list(os.pwd)\nprintln(files.size)\n";
    let tree = parse(&engine, &language, script);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

/// Each name is one Fenix colours, by its first part (`Theme::syntax_color`).
fn assert_known(captures: &[Capture]) {
    let known = [
        "keyword", "repeat", "conditional", "storageclass", "include", "string", "escape", "comment", "spell", "function", "method", "type", "namespace", "module", "number", "float",
        "constant", "boolean", "variable", "property", "label", "field", "parameter", "operator", "punctuation", "attribute", "constructor",
    ];
    for (_, _, name, _) in captures {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}: no colour of Fenix's for it");
    }
}

#[test]
fn a_scala_3_sample_is_highlighted_folded_and_indented() {
    let (engine, language, _) = load();
    let source = read("src/samples/orbit3.scala");
    let tree = parse(&engine, &language, &source);
    let h = captures(&language, &tree, &query("highlights"), &source);
    assert_known(&h);
    let at = |text: &str, nth: usize| shown(&h, &source, text, nth);
    for (text, nth, name) in [
        ("//> using scala 3.7.3", 0, "comment"),
        ("scala", 0, "attribute"),
        (" 3.7.3", 0, "string"),
        ("package", 0, "keyword"),
        ("orbit", 0, "namespace"),
        ("ArrayBuffer", 0, "type"),
        ("opaque", 0, "keyword"),
        ("Metres", 0, "type"),
        ("extension", 0, "keyword"),
        ("km", 0, "function"),
        ("enum", 0, "keyword"),
        ("derives", 0, "keyword"),
        ("CanEqual", 0, "type"),
        ("Earth", 0, "constant"),
        ("Probe", 0, "type"),
        ("5.97e24", 0, "number"),
        ("given", 0, "keyword"),
        ("with", 0, "keyword"),
        ("using", 2, "keyword"),
        ("trait", 0, "keyword"),
        ("show", 0, "function"),
        ("render", 0, "function"),
        ("\"kir\"", 0, "string"),
        ("s\"$n in the north\"", 0, "string"),
        ("$", 0, "punctuation.special"),
        ("match", 0, "conditional"),
        ("then", 0, "conditional"),
        ("do", 0, "repeat"),
        ("yield", 0, "repeat"),
        ("inline", 0, "keyword"),
        ("private", 0, "keyword"),
        ("Success", 0, "type"),
        ("_", 0, "variable.builtin"),
        ("1000.0", 0, "number"),
        ("+=", 0, "operator"),
        ("=>", 0, "operator"),
        ("{", 0, "punctuation.bracket"),
    ] {
        assert_eq!(at(text, nth), Some(name), "{text} ({nth})");
    }
    assert_eq!(shown_in(&h, &source, "val names = for", "for"), Some("repeat"));
    assert_eq!(shown_in(&h, &source, "@main def", "@"), Some("attribute"));
    assert_eq!(shown_in(&h, &source, "@main def", "main"), Some("attribute"));
    assert_eq!(shown_in(&h, &source, "end Body", "end"), Some("keyword"));
    assert_eq!(at("/** A body that orbits another.\n  * Uses [[Vec]] for its position.\n  */", 0), Some("comment"));

    // What folds: each definition, braces or indentation.
    let folds = captures(&language, &tree, &query("folds"), &source);
    assert!(folds.iter().all(|c| c.2 == "fold"));
    let folded: Vec<&str> = folds.iter().map(|(s, _, _, _)| source[*s..].lines().next().unwrap()).collect();
    for start in [
        "enum Body(val mass: Double) derives CanEqual:",
        "trait Show[A]:",
        "given Show[Int] with",
        "def render[A](a: A)(using s: Show[A]): String =",
        "extension [A](xs: List[A])",
        "class Fleet(stations: Seq[Station]) extends Iterable[Station]:",
        "@main def run(args: String*): Unit =",
        "  def describe(s: Station): String = s match",
    ] {
        assert!(folded.iter().any(|f| f.trim() == start.trim()), "{start}: {folded:?}");
    }

    // Enter indents after what opens a block, and inside brackets.
    let indents = captures(&language, &tree, &query("indents"), &source);
    assert!(indents.iter().all(|c| c.2 == "indent"));
    let indented_after = |text: &str, nth: usize| {
        let end = source.match_indices(text).nth(nth).map(|(i, _)| i + text.len()).unwrap();
        // Fenix's rule (`SyntaxState::indents_after`): a capture starting on the cursor's line before it and ending at or after it.
        let line_start = source[..end].rfind('\n').map_or(0, |i| i + 1);
        indents.iter().any(|(s, e, _, _)| (line_start..end).contains(s) && *e >= end)
    };
    assert!(indented_after("trait Show[A]:", 0), "after a colon");
    assert!(indented_after("given Show[Int] with", 0), "after with");
    assert!(indented_after("def render[A](a: A)(using s: Show[A]): String =", 0), "after =");
    assert!(indented_after("while count < 3 do", 0), "after do");
    assert!(indented_after("case Success(text) =>", 0), "after a case's arrow");
    assert!(indented_after("val fleet = Fleet(List(", 0) || indented_after("val fleet = Fleet(", 0), "inside arguments");
    assert!(!indented_after("var count = 0", 0), "a finished line");
}

#[test]
fn a_scala_2_sample_is_highlighted_folded_and_indented() {
    let (engine, language, _) = load();
    let source = read("src/samples/orbit2.scala");
    let tree = parse(&engine, &language, &source);
    let h = captures(&language, &tree, &query("highlights"), &source);
    assert_known(&h);
    let at = |text: &str, nth: usize| shown(&h, &source, text, nth);
    for (text, nth, name) in [
        ("sealed", 0, "keyword"),
        ("trait", 0, "keyword"),
        ("Body", 0, "type"),
        ("case", 0, "keyword"),
        ("Earth", 0, "type"),
        ("Probe", 0, "type"),
        ("extends", 0, "keyword"),
        ("App", 0, "type"),
        ("implicit", 0, "keyword"),
        ("val", 0, "keyword"),
        ("lazy", 0, "keyword"),
        ("period", 0, "function"),
        ("sqrt", 0, "function"),
        ("def", 0, "keyword"),
        ("=>", 0, "operator"),
        ("<-", 0, "operator"),
        ("yield", 0, "repeat"),
        ("for", 0, "repeat"),
        ("try", 0, "keyword"),
        ("catch", 0, "keyword"),
        ("finally", 0, "keyword"),
        ("throw", 0, "keyword"),
        ("new", 0, "keyword"),
        ("IllegalStateException", 0, "type"),
        ("12.5", 0, "number"),
        ("5.97e24", 0, "number"),
        ("\"p1\"", 0, "string"),
        ("math", 0, "variable"),
        ("radius", 0, "parameter"),
        ("{", 0, "punctuation.bracket"),
    ] {
        assert_eq!(at(text, nth), Some(name), "{text} ({nth})");
    }
    assert_eq!(at("Map", 0), Some("type"));

    let folds = captures(&language, &tree, &query("folds"), &source);
    let folded: Vec<&str> = folds.iter().map(|(s, _, _, _)| source[*s..].lines().next().unwrap()).collect();
    for start in ["sealed trait Body {", "object Orbits extends App {", "  implicit class RichDouble(val d: Double) extends AnyVal {", "  def period[A <: Body](body: A, radius: Double)(implicit g: Double): Double = {"] {
        assert!(folded.iter().any(|f| f.trim_end() == start.trim_end() || f.starts_with(start.trim_start())), "{start}: {folded:?}");
    }
    let indents = captures(&language, &tree, &query("indents"), &source);
    let end = source.find("object Orbits extends App {").unwrap() + "object Orbits extends App {".len();
    let line_start = source[..end].rfind('\n').map_or(0, |i| i + 1);
    assert!(indents.iter().any(|(s, e, _, _)| (line_start..end).contains(s) && *e >= end), "after a brace");
}

/// The manifest's table `name`, as an array of strings.
fn strings(manifest: &toml::Table, table: &str, key: &str) -> Vec<String> {
    manifest[table][key].as_array().unwrap().iter().filter_map(|p| p.as_str().map(str::to_string)).collect()
}

fn manifest() -> toml::Table {
    toml::from_str(&read("extension.toml")).unwrap()
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
    let snippets = files("snippets/scala", "snippet");
    assert!(snippets.len() >= 30, "{}", snippets.len());
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["scala"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("$0") && !shown.contains("${1") && !shown.contains("$1"), "{file}: a field left: {shown}");
        assert!(!text.contains('\t'), "{file}: Scala is indented with spaces");
        keys.push(s.trigger);
    }
    for key in ["main", "maino", "cc", "adt", "enum", "given", "using", "ext", "opaque", "def", "match", "for", "try", "munit", "usc", "udep", "sbtdep", "sbtproj"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    // Scala 3's and Scala 2's, each said so.
    let named = |key: &str| snippets.iter().find(|(f, _)| f == &format!("{key}.snippet")).unwrap().1.clone();
    assert!(named("enum").contains("(Scala 3)") && named("maino").contains("Scala 2 and 3") && named("impl").contains("(Scala 2)"));
}

#[test]
fn the_launch_file_reads_as_fenix_reads_it() {
    /// `fenix-gui`'s launch file: a field it doesn't know is refused.
    #[allow(dead_code)]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Launch {
        name: String,
        #[serde(default)]
        languages: Vec<String>,
        request: Option<String>,
        program: Option<toml::Value>,
        args: Option<Vec<String>>,
        cwd: Option<String>,
        env: Option<toml::Table>,
        pre_launch_task: Option<String>,
        attributes: Option<toml::Table>,
    }
    let launches = files("launch", "toml");
    assert_eq!(launches.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["metals.toml"]);
    let l: Launch = toml::from_str(&launches[0].1).unwrap();
    assert_eq!((l.name.as_str(), l.languages.as_slice(), l.request.as_deref(), l.program), ("Scala: Metals' debug session", ["scala".to_string()].as_slice(), Some("launch"), Some(toml::Value::Boolean(false))));
    // The notification names it, as the pack says it.
    assert!(launches[0].1.contains("SPC m b"), "the key that starts it");
}

#[test]
fn the_project_templates_plan_for_each_scala() {
    use fenix_project::template::{load_template_dir, Answer, Origin};
    let parent = crate::project::tests::temp("templates");
    let root = dir("templates");
    for (folder, needs, wanted) in [
        ("scala-sbt", "sbt", ["build.sbt", "project/build.properties", ".scalafmt.conf", ".gitignore", "README.md", "src/main/scala/Main.scala", "src/test/scala/MainSuite.scala"].as_slice()),
        ("scala-cli-script", "scala-cli", ["hello.sc", ".scalafmt.conf", ".gitignore", "README.md"].as_slice()),
    ] {
        let t = load_template_dir(&root.join(folder), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{folder}: {e}"));
        assert_eq!(t.needs, [needs], "{folder}");
        for scala in ["3.10.1", "3.3.6", "2.13.17"] {
            let mut answers = t.default_answers("orbit-fleet");
            answers.insert("scala".into(), Answer::Text(scala.into()));
            let plan = t.plan("orbit-fleet", &parent, &answers).unwrap_or_else(|e| panic!("{folder} {scala}: {e}"));
            assert!(plan.steps.is_empty(), "{folder}: it runs nothing, so it can't fail offline");
            let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{folder} {scala}: {path}")).contents.clone();
            let paths: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
            for want in wanted {
                assert!(paths.contains(want), "{folder} {scala}: {want}: {paths:?}");
            }
            assert_eq!(paths.len(), wanted.len(), "{folder} {scala}: {paths:?}");
            for f in plan.files.iter().chain(&plan.after_files) {
                assert!(!f.contents.contains("{{"), "{folder} {scala}: {} has a {{{{...}}}} left", f.path);
            }
            let fmt = file(".scalafmt.conf");
            assert!(fmt.contains(if scala == "2.13.17" { "runner.dialect = scala213" } else { "runner.dialect = scala3" }), "{folder} {scala}: {fmt}");
            if folder == "scala-sbt" {
                assert!(file("build.sbt").contains(&format!("scalaVersion := \"{scala}\"")) && file("build.sbt").contains("name := \"orbit-fleet\""), "{scala}");
                let main = file("src/main/scala/Main.scala");
                assert_eq!(main.contains("@main"), scala != "2.13.17", "{scala}: Scala 3's entry point, or Scala 2's App");
                assert_eq!(main.contains("extends App"), scala == "2.13.17", "{scala}");
                assert!(file("project/build.properties").contains("sbt.version=1.12.15"));
                for kept_out in ["target/", ".bsp/", ".metals/"] {
                    assert!(file(".gitignore").lines().any(|l| l == kept_out), "{kept_out}");
                }
            } else {
                assert!(file("hello.sc").starts_with(&format!("//> using scala {scala}\n")) && file("hello.sc").contains("orbit-fleet"), "{scala}");
            }
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn the_manifest_lists_the_commands_the_kit_and_the_keymap_use() {
    let m = manifest();
    let commands: Vec<String> = m["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    for want in ["scala.doctor", "scala.install.coursier", "scala.install-metals", "scala.import-build", "scala.console", "scala.watch", "scala.run-file", "scala.test-file", "scala.debug-file", "scala.debug-tests"] {
        assert!(commands.iter().any(|c| c == want), "{want}");
    }
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
    let project = crate::project::Project { tools: vec![crate::project::Tool::Sbt], ..Default::default() };
    let tools = crate::tools::Tools { coursier: Some("/t/cs".into()), ..Default::default() };
    let programs = crate::doctor::programs(&project, &tools, &crate::tools::Platform::new("linux", "x86_64"), None);
    let fixes: Vec<String> = programs.iter().filter_map(|p| p.fix.clone()).map(|(_, c, _)| c).collect();
    assert_eq!(fixes, ["scala.install.coursier", "scala.install-metals"], "Coursier's row has its fix when it isn't found; it's shown while there is no Metals");
    let none = crate::doctor::programs(&project, &Default::default(), &crate::tools::Platform::new("linux", "x86_64"), None);
    for (_, command, _) in none.iter().filter_map(|p| p.fix.clone()) {
        assert!(commands.contains(&command), "{command}");
    }
    assert!(commands.contains(&crate::doctor::INSTALL_METALS.to_string()) && commands.contains(&crate::doctor::INSTALL_COURSIER.to_string()));
}

#[test]
fn the_manifest_asks_for_what_the_pack_runs_and_downloads() {
    use crate::project::{Project, Tool};
    use crate::tools::{Platform, Tools};
    let m = manifest();
    assert_eq!(m["extension"]["id"].as_str(), Some("fenix.scala"));
    assert_eq!((m["engine"]["fenix"].as_str(), m["engine"]["api"].as_str()), (Some(">=1.5, <2"), Some("1.5")));
    let process = strings(&m, "permissions", "process");
    let named = |program: &str| process.iter().any(|p| p == program || (p == "/tools/*" && program.starts_with("/tools/")) || program.starts_with("/t/") && process.iter().any(|p| program.ends_with(&format!("/{}", p.rsplit('/').next().unwrap()))));
    // Every program a task, the REPL, the watcher, a file's run, the doctor and Metals' install run is named.
    let all = Project {
        tools: vec![Tool::Sbt, Tool::Mill, Tool::ScalaCli, Tool::Gradle, Tool::Maven],
        sbt_projects: vec!["core".into()],
        mill_modules: vec!["core".into()],
        scalafix: Some(Tool::Sbt),
        gradle_application: true,
        ..Project::default()
    };
    let found = Tools {
        javac: Some("/t/javac".into()),
        coursier: Some("/t/cs".into()),
        metals: Some("/t/metals".into()),
        scala_cli: Some("/t/scala-cli".into()),
        sbt: Some("/t/sbt".into()),
        mill: Some("/t/mill".into()),
        gradle: Some("/t/gradle".into()),
        maven: Some("/t/mvn".into()),
    };
    for tools in [&found, &Tools::default()] {
        for t in &crate::tasks::tasks(&all, tools, "/nowhere", None).tasks {
            assert!(named(&t.cmd.program), "{}", t.cmd.program);
        }
        for tool in &all.tools {
            assert!(named(&crate::tasks::program(*tool, tools).program), "{tool:?}");
        }
        assert!(named(&crate::tasks::console(&all, tools).unwrap().1.program));
        assert!(named(&crate::tasks::watch(&all, tools).unwrap().1.program));
        assert!(named(&crate::tasks::run_file(tools, "run", "/p/a.sc").program));
    }
    assert!(named(&crate::tools::install_metals("/tools/coursier/cs").program) && named(&found.metals.clone().unwrap()));
    for p in crate::doctor::programs(&all, &Tools::default(), &Platform::new("linux", "x86_64"), None) {
        assert!(process.contains(&p.name) || p.name.starts_with("/tools/"), "{}", p.name);
    }
    assert!(process.contains(&"${setting:scala.jdk}".to_string()), "the JDK you chose runs javac");
    // Every host a release is downloaded from is named, and no other.
    let hosts = strings(&m, "permissions", "network");
    for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")] {
        for i in Platform::new(os, arch).installs() {
            let host = i.url.strip_prefix("https://").unwrap().split('/').next().unwrap();
            assert!(hosts.iter().any(|h| h == host), "{}", i.url);
        }
    }
    // The port Metals' debug adapter listens on, on this machine.
    assert_eq!(strings(&m, "permissions", "sockets"), ["tcp://localhost:1024-65535", "tcp://127.0.0.1:1024-65535"]);
    assert_eq!((m["permissions"]["filesystem"].as_str(), m["permissions"]["editor"].as_str()), (Some("project-read"), Some("read")));
    // The settings it reads are the manifest's, with their defaults as text.
    let settings = m["contributes"]["settings"].as_array().unwrap();
    let keys: Vec<(&str, &str)> = settings.iter().map(|s| (s["key"].as_str().unwrap(), s["default"].as_str().unwrap())).collect();
    assert_eq!(keys, [("scala.jdk", ""), ("scala.auto_import", "all")]);
    assert_eq!(settings[1]["choices"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect::<Vec<_>>(), ["all", "initial", "off"]);
    assert_eq!(crate::Settings::default(), crate::Settings { jdk: None, auto_import: "all".into() });
    // It's Scala's language server, for the language the language file brings.
    let server = &m["contributes"]["language_servers"].as_array().unwrap()[0];
    assert_eq!((server["name"].as_str(), server["languages"][0].as_str()), (Some("metals"), Some("scala")));
    // Its context loads it for each language it brings and the files that mark a project.
    let context = &m["contributes"]["contexts"].as_array().unwrap()[0];
    assert_eq!(context["languages"][0].as_str(), Some("scala"));
    let markers: Vec<&str> = context["markers"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(markers, ["build.sbt", "build.sc", "build.mill", "project.scala", ".scalafmt.conf"]);
    // Its kind: tagged, and none of Fenix's markers (build.gradle and pom.xml are the Java pack's).
    let kind = &m["contributes"]["project_kinds"].as_array().unwrap()[0];
    assert_eq!((kind["id"].as_str(), kind["name"].as_str(), kind["tag"].as_str()), (Some("scala"), Some("Scala"), Some("SC")));
    let kind_markers: Vec<&str> = kind["markers"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(kind_markers, markers);
    for fenix_own in ["cargo.toml", "package.json", "pyproject.toml", "setup.py", "requirements.txt", "uv.lock", "go.mod", "cmakelists.txt", "pkgindex.tcl", ".git", ".projectile", "build.gradle", "pom.xml"] {
        assert!(!kind_markers.iter().any(|m| m.eq_ignore_ascii_case(fenix_own)), "{fenix_own}");
    }
    assert!(m["contributes"]["project_kinds"].as_array().unwrap().len() == 1);
    // The files of a Scala project that the doctor reads again when saved.
    for m in kind_markers {
        assert!(crate::BUILD_FILES.contains(&m), "{m} is read again when saved");
    }
}
