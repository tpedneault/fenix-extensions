//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language files by the same rules
//! as `fenix-gui`'s (`languages::File`), and its two grammars loaded and
//! their queries run as `fenix-syntax` runs them -- tree-sitter 0.26's
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

fn render(text: &str) -> String {
    fenix_snippets::Snippet::parse(text).unwrap().template.render(&Default::default(), &Default::default(), "").text
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/elixir", "snippet");
    assert!(snippets.len() >= 50);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["elixir"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${") && !shown.contains('$'), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in [
        "defmodule", "def", "defp", "genserver", "init", "hcall", "hcast", "hinfo", "case", "with", "cond", "pipe", "pmap", "test", "describe", "setup", "doctest", "exunit", "sup", "spec", "doc",
    ] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_snippets_keep_elixirs_own_punctuation() {
    // The default value's backslashes, interpolation's braces, and the
    // pipe's lines.
    let server = render(include_str!("../snippets/elixir/genserver.snippet"));
    assert!(server.contains("def start_link(opts \\\\ []) do") && server.contains("{:ok, state}") && server.contains("use GenServer"), "{server}");
    assert_eq!(render(include_str!("../snippets/elixir/pipe.snippet")), "value\n|> Mod.fun()\n|> Mod.fun()");
    assert_eq!(render(include_str!("../snippets/elixir/hcall.snippet")), "@impl true\ndef handle_call(request, _from, state) do\n  {:reply, reply, state}\nend\n");
    assert_eq!(render(include_str!("../snippets/elixir/ptap.snippet")), "|> tap(&IO.inspect(&1, label: \"value\"))");
    assert_eq!(render(include_str!("../snippets/elixir/forinto.snippet")), "for x <- enumerable, into: %{} do\n  {key, value}\nend");
    assert_eq!(render(include_str!("../snippets/elixir/defmodule.snippet")), "defmodule MyApp.Name do\n  \nend");
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

/// A grammar, as Fenix loads it from its language file.
fn grammar(file: &str, name: &str) -> (LanguageFile, wasmtime::Engine, Language) {
    let l: LanguageFile = toml::from_str(&std::fs::read_to_string(dir("languages").join(file)).unwrap()).unwrap();
    assert_eq!((l.language.as_str(), l.server.as_deref(), l.symbol.as_deref(), l.queries.as_deref()), (name, None, None, None), "the server is the code's to set; queries/{name}");
    let wasm = std::fs::read(dir(l.grammar.as_deref().unwrap())).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    // `tree_sitter_<symbol>`: by default the language's name.
    let language = store.load_language(name, &wasm).unwrap();
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    (l, engine, language)
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

/// Every name a query gives is one Fenix colours, by its first part.
fn known(highlights: &[(String, String, usize)]) {
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation", "attribute", "tag"];
    for (_, name, _) in highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }
}

#[test]
fn elixirs_grammar_loads_and_its_queries_run() {
    let (l, engine, language) = grammar("elixir.toml", "elixir");
    assert_eq!(l.extensions, ["ex", "exs"]);
    assert_eq!((l.filenames.as_slice(), l.comment.as_deref()), (["mix.lock".to_string()].as_slice(), Some("#")));
    let source = include_str!("samples/crew.ex");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/elixir/highlights.scm"), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("defmodule", "keyword"),
        ("def", "keyword.definition"),
        ("defp", "keyword"),
        ("use", "keyword"),
        ("alias", "keyword"),
        ("with", "keyword"),
        ("case", "keyword"),
        ("do", "keyword"),
        ("end", "keyword"),
        ("fn", "keyword"),
        ("when", "keyword"),
        ("else", "keyword"),
        ("Orbit.Crew", "module"),
        ("GenServer", "module"),
        ("start_link", "function"),
        ("validate", "function"),
        ("1_000.5", "number"),
        ("true", "constant"),
        (":ok", "constant"),
        (":aboard", "constant"),
        ("name: ", "property"),
        ("\"Welcome aboard, #{name}!\\n\"", "string"),
        ("\\n", "string.escape"),
        ("~r/^\\w+$/i", "string.regex"),
        ("__MODULE__", "constant.builtin"),
        ("|>", "operator"),
        ("# Client", "comment"),
        ("_from", "comment.unused"),
        ("impl", "attribute"),
        ("max_crew", "attribute"),
        ("\\\\", "operator"),
    ] {
        assert_eq!(shown(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>());
    }
    for text in ["@", "spec", "type"] {
        assert!(highlights.iter().any(|(t, n, _)| t == text && n == "attribute"), "{text}");
    }
    // The documentation reads as a comment, the string and the attribute.
    assert!(highlights.iter().any(|(t, n, _)| n == "comment.documentation" && t.contains("The people aboard")), "@moduledoc's heredoc");
    assert!(highlights.iter().any(|(t, n, _)| n == "comment.documentation" && t == "\"Starts the roster, empty.\""), "@doc's string");
    assert!(highlights.iter().any(|(t, n, _)| n == "function" && t == "Enum.map" || n == "function" && t == "map"), "a remote call");
    known(&highlights);

    // What folds: the definitions and the blocks.
    let folds = captures(&language, &tree, include_str!("../queries/elixir/folds.scm"), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["defmodule Orbit.Crew do", "def start_link(opts \\\\ []) do", "defp validate(name) do", "do", "fn name -> {String.length(name), name} end"] {
        assert!(folded.contains(&start), "{start}: {folded:?}");
    }
    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, include_str!("../queries/elixir/indents.scm"), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("do\n    GenServer.start_link")), "a function's body: {:?}", indents.iter().map(|(t, _, _)| t.lines().next().unwrap().to_string()).collect::<Vec<_>>());
}

#[test]
fn mix_exs_and_scripts_read_as_elixir() {
    let (_, engine, language) = grammar("elixir.toml", "elixir");
    let source = include_str!("samples/mix.exs.phoenix");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
    let highlights = captures(&language, &tree, include_str!("../queries/elixir/highlights.scm"), source);
    assert!(highlights.iter().any(|(t, n, _)| t == "use" && n == "keyword") && highlights.iter().any(|(t, n, _)| t == "\"~> 1.15\"" && n == "string"));
}

#[test]
fn heexs_grammar_loads_and_reads_templates() {
    let (l, engine, language) = grammar("heex.toml", "heex");
    assert_eq!(l.extensions, ["heex", "eex", "leex"], "EEx is read as HEEx: its `<% %>` is the same");
    assert_eq!(l.comment, None);
    let source = include_str!("samples/page.html.heex");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, include_str!("../queries/heex/highlights.scm"), source);
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [("<%=", "tag.delimiter"), ("ul", "tag"), ("class", "tag.attribute"), ("%>", "tag.delimiter"), ("<%!--", "tag.delimiter")] {
        assert_eq!(shown(text), Some(name), "{text}");
    }
    assert!(highlights.iter().any(|(t, n, _)| n == "comment" && t.contains("The crew's page")), "{highlights:?}");
    known(&highlights);

    let folds = captures(&language, &tree, include_str!("../queries/heex/folds.scm"), source);
    assert!(folds.iter().any(|(t, n, _)| n == "fold" && t.starts_with("<ul id=\"crew\">")), "a tag");
    assert!(folds.iter().any(|(t, n, _)| n == "fold" && t.starts_with("<.header")), "a component");
    let indents = captures(&language, &tree, include_str!("../queries/heex/indents.scm"), source);
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("<ul")));

    // A plain EEx template, as `.html.eex` and `.text.eex` hold it.
    let eex = "<h1><%= @title %></h1>\n<%= for x <- @items do %>\n  <p><%= x %></p>\n<% end %>\n";
    assert!(!parse(&engine, &language, eex).root_node().has_error());
}

#[test]
fn the_manifest_lists_the_commands_the_kit_and_the_keymap_use() {
    let manifest: toml::Table = toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap();
    let commands: Vec<String> = manifest["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    for own in ["elixir.doctor", "elixir.iex", "elixir.install-hex", "elixir.install.expert"] {
        assert!(commands.iter().any(|c| c == own), "{own}: {commands:?}");
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
    // Every fix row's command, and the install's.
    let fixes: Vec<String> = crate::doctor::programs(&crate::project::Project::default(), &crate::tools::Platform::new("linux", "x86_64"), &crate::tools::Tools::default()).iter().filter_map(|p| p.fix.as_ref().map(|f| f.1.clone())).collect();
    for fix in &fixes {
        assert!(commands.contains(fix), "{fix}");
    }
    let title = manifest["contributes"]["commands"].as_array().unwrap().iter().find(|c| c["id"].as_str() == Some("elixir.install.expert")).unwrap()["title"].as_str().unwrap();
    assert!(title.contains(crate::tools::EXPERT), "the command names the version it installs");
}

#[test]
fn the_manifest_names_the_programs_the_pack_runs_and_the_hosts_it_downloads_from() {
    let manifest: toml::Table = toml::from_str(&std::fs::read_to_string(dir("extension.toml")).unwrap()).unwrap();
    let list = |key: &str| -> Vec<String> { manifest["permissions"][key].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect() };
    let process = list("process");
    for program in ["mix", "elixir", "iex", "expert", "/tools/*"] {
        assert!(process.iter().any(|p| p == program), "{program}");
    }
    let network = list("network");
    let install = crate::tools::Platform::new("linux", "x86_64").expert().unwrap();
    let host = install.url.strip_prefix("https://").unwrap().split('/').next().unwrap();
    assert!(network.iter().any(|n| n == host), "{host} in {network:?}");
    assert_eq!(manifest["contributes"]["contexts"][0]["markers"][0].as_str(), Some("mix.exs"));
    let settings: Vec<&str> = manifest["contributes"]["settings"].as_array().unwrap().iter().map(|s| s["key"].as_str().unwrap()).collect();
    assert_eq!(settings, ["elixir.compiler_warnings", "elixir.credo"]);
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    for (id, opens) in [("elixir-library", "lib/orbit_desk.ex"), ("elixir-app", "lib/orbit_desk/counter.ex")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(plan.steps.is_empty(), "{id}: nothing to run");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        for want in [opens, "mix.exs", ".formatter.exs", ".gitignore", "README.md", "test/test_helper.exs"] {
            assert!(all.contains(&want), "{id}: {want} in {all:?}");
        }
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }

        // What the pack makes of it: a Mix project, its modules named for
        // the project.
        let made = parent.join(id);
        for f in plan.files.iter().chain(&plan.after_files) {
            let path = made.join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.contents).unwrap();
        }
        let p = crate::project::Project::read(&made.to_string_lossy());
        assert_eq!((p.mix, p.phoenix, p.elixir.as_deref()), (true, false, Some("1.15")), "{id}");
        let mix = std::fs::read_to_string(made.join("mix.exs")).unwrap();
        assert!(mix.starts_with("defmodule OrbitDesk.MixProject do") && mix.contains("app: :orbit_desk"), "{id}");
        if id == "elixir-app" {
            assert!(mix.contains("OrbitDesk.Application,"), "the supervisor's module");
            assert!(std::fs::read_to_string(made.join("lib/orbit_desk/application.ex")).unwrap().contains("OrbitDesk.Counter"));
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
