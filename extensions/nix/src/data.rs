//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language file by the same rules as
//! `fenix-gui`'s (`languages::File`), the grammar loaded and its queries run
//! as `fenix-syntax` runs them -- tree-sitter 0.26's WebAssembly store --
//! and its manifest against the code.

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

fn language_file() -> LanguageFile {
    toml::from_str(&std::fs::read_to_string(dir("languages/nix.toml")).unwrap()).unwrap_or_else(|e| panic!("{}", e.message()))
}

/// The grammar as Fenix loads it from the language file: the file's, the
/// `tree_sitter_<symbol>` it exports (the file's name without `tree-sitter-`,
/// unless `symbol` says), the queries' folder.
fn load() -> (wasmtime::Engine, Language, PathBuf) {
    let l = language_file();
    assert_eq!(l.language, "nix");
    let grammar = l.grammar.as_deref().expect("a grammar");
    let wasm = std::fs::read(dir(grammar)).unwrap();
    assert!(wasm.len() as u64 <= 32 << 20, "Fenix takes a grammar of 32 MiB at most");
    let stem = Path::new(grammar).file_stem().unwrap().to_string_lossy().into_owned();
    let symbol = l.symbol.clone().unwrap_or_else(|| stem.strip_prefix("tree-sitter-").unwrap_or(&stem).replace('-', "_"));
    let engine = wasmtime::Engine::default();
    let mut store = WasmStore::new(&engine).unwrap();
    let language = store.load_language(&symbol, &wasm).unwrap_or_else(|e| panic!("tree_sitter_{symbol}: {e}"));
    assert!((13..=15).contains(&language.abi_version()), "ABI {}", language.abi_version());
    let queries = dir(l.queries.as_deref().map(str::to_string).unwrap_or_else(|| "queries/nix".into()).trim_end_matches('/'));
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

/// Whether `source` parses without an error node.
fn parses(engine: &wasmtime::Engine, language: &Language, source: &str) -> bool {
    !parse(engine, language, source).root_node().has_error()
}

#[test]
fn the_language_file_reads_as_fenix_reads_it() {
    let all = files("languages", "toml");
    assert_eq!(all.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["nix.toml"]);
    let l = language_file();
    assert_eq!(l.extensions, ["nix"]);
    assert_eq!(l.comment.as_deref(), Some("#"));
    assert!(l.server.is_none(), "the server is the code's to set: nixd or nil, as found");
    assert_eq!(crate::LANGUAGES, ["nix"]);
}

#[test]
fn the_grammars_licence_and_source_are_beside_it() {
    let readme = std::fs::read_to_string(dir("grammars/README.md")).unwrap();
    let bytes = std::fs::read(dir("grammars/tree-sitter-nix.wasm")).unwrap();
    assert!(bytes.starts_with(b"\0asm"));
    let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert!(readme.contains(&sha), "its SHA-256 {sha} is in the README");
    assert!(readme.contains("ea1d87f7996be1329ef6555dcacfa63a69bd55c6") && readme.contains("@lumis-sh/wasm-nix"), "where it's from, and at which commit");
    let licence = std::fs::read_to_string(dir("grammars/LICENSE-tree-sitter-nix")).unwrap();
    assert!(licence.starts_with("MIT License") && licence.contains("Charles Strahan"));
    assert!(readme.contains("MIT-licensed"));
}

#[test]
fn nixs_grammar_loads_and_its_queries_run_on_a_flake() {
    let (engine, language, queries) = load();
    let source = include_str!("samples/flake.nix");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("description", "property"),
        ("Ground station tools", "string"),
        ("RUST_LOG", "property"),
        ("outputs", "property"),
        ("self", "variable.parameter"),
        ("...", "punctuation.special"),
        ("let", "keyword"),
        ("in", "keyword"),
        ("rec", "keyword"),
        ("inherit", "keyword"),
        ("with", "keyword"),
        ("eachDefaultSystem", "function"),
        ("import", "function.builtin"),
        ("toString", "function.builtin"),
        ("or", "keyword"),
        ("0", "number"),
        ("./.", "string.special"),
        ("mkDerivation", "function"),
        ("buildInputs", "property"),
        ("orbit", "string"),
        ("\"", "string"),
        ("${", "punctuation.special"),
        ("=", "punctuation.delimiter"),
        ("{", "punctuation.bracket"),
        ("[", "punctuation.bracket"),
        (":", "operator"),
        ("@", "operator"),
    ] {
        assert_eq!(shown(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>());
    }
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation", "escape"];
    for (_, name, _) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: sets, lists, `let`, multi-line strings, and bindings holding them.
    let folds = captures(&language, &tree, &std::fs::read_to_string(queries.join("folds.scm")).unwrap(), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["{", "let", "devShells.default = pkgs.mkShell {", "buildPhase = ''", "[ pkgs.nixd pkgs.nixfmt-rfc-style ]"] {
        assert!(folded.iter().any(|f| f.trim_end() == start || f.ends_with(start)), "{start}: {folded:?}");
    }

    // Enter indents inside a set or a list: an @indent starting on that line.
    let indents = captures(&language, &tree, &std::fs::read_to_string(queries.join("indents.scm")).unwrap(), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("{\n      description") || t.starts_with("{\n  description")), "the top set");
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("let\n")), "a let");
}

#[test]
fn and_on_a_nixos_module() {
    let (engine, language, queries) = load();
    let source = include_str!("samples/module.nix");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("# A NixOS module: a service for the ground station.", "comment"),
        ("config", "variable.parameter"),
        ("enable", "property"),
        ("mkEnableOption", "function"),
        ("mkOption", "function"),
        ("mkIf", "function"),
        ("types", "variable"),
        ("builtins", "variable.builtin"),
        ("true", "boolean"),
        ("null", "constant.builtin"),
        ("https://example.org/orbit", "string.special"),
        ("./config/orbit.conf", "string.special"),
        ("\\t", "string.escape"),
        ("\\n", "string.escape"),
        ("4.5e2", "number"),
        ("||", "operator"),
        ("-", "operator"),
        ("lib", "variable"),
        ("rec", "keyword"),
    ] {
        // A name can be a plain variable in one place and the function called in another: the one the text is, somewhere, among its captures.
        let all: Vec<&str> = highlights.iter().filter(|(t, _, _)| t == text).map(|(_, n, _)| n.as_str()).collect();
        assert!(shown(text) == Some(name) || all.contains(&name), "{text}: {all:?}");
    }
    assert_eq!(shown("\"multi-user.target\""), None);
    assert_eq!(shown("multi-user.target"), Some("string"), "a string's text");
    // An interpolation's code is code, not string.
    assert_eq!(shown("toString"), Some("function.builtin"));
    let folds = captures(&language, &tree, &std::fs::read_to_string(queries.join("folds.scm")).unwrap(), source);
    assert!(folds.iter().any(|(t, _, _)| t.starts_with("systemd.services.orbit = {")), "a binding that holds a set: the scope SPC c s lists");
}

#[test]
fn a_file_with_an_error_is_one_the_grammar_says_so_of() {
    let (engine, language, _) = load();
    assert!(!parses(&engine, &language, "{ a = ; }"));
    assert!(parses(&engine, &language, "{ a = 1; }"));
}

#[test]
fn what_the_formatters_wrote_is_nix() {
    // Recorded: `alejandra --quiet -` (4.0.0) and `nixpkgs-fmt` (1.3.0) on src/samples/ugly.nix.
    let (engine, language, _) = load();
    for sample in [include_str!("samples/ugly.nix"), include_str!("samples/ugly.alejandra.nix"), include_str!("samples/ugly.nixpkgs-fmt.nix")] {
        assert!(parses(&engine, &language, sample), "{sample}");
    }
    assert!(include_str!("samples/ugly.alejandra.nix").starts_with("{pkgs, ...}: {\n  a = 1;"));
    assert!(include_str!("samples/ugly.nixpkgs-fmt.nix").starts_with("{ pkgs, ... }: {\n  a = 1;"));
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole_and_is_nix() {
    let (engine, language, _) = load();
    let snippets = files("snippets/nix", "snippet");
    assert!(snippets.len() >= 20, "{}", snippets.len());
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["nix"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("$0") && !shown.contains("${1") && !shown.contains("$1"), "{file}: a field left: {shown}");
        assert!(!text.contains('\t'), "{file}: Nix is indented with spaces");
        // As whole expressions, or as the bindings of a set, or a function
        // whose body is what's typed next.
        let ok = [shown.clone(), format!("{{ {shown} }}"), format!("{shown} null"), format!("let x = 1; in {shown}")].iter().any(|v| parses(&engine, &language, v));
        assert!(ok, "{file} isn't Nix once filled in:\n{shown}");
        keys.push(s.trigger);
    }
    for key in ["flake", "devshell", "mkshell", "drv", "module", "overlay", "let", "with", "inh", "inhf", "opt", "fn"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_project_templates_plan_and_what_they_write_is_nix() {
    use fenix_project::template::{load_template_dir, Answer, Origin};
    let (engine, language, _) = load();
    let parent = crate::project::tests::temp("templates");
    let root = dir("templates");

    let t = load_template_dir(&root.join("nix-flake"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    assert!(t.needs.is_empty(), "it runs nothing, so it needs nothing");
    for formatter in ["nixfmt", "alejandra", "nixpkgs-fmt"] {
        let mut answers = t.default_answers("orbit-tools");
        answers.insert("formatter".into(), Answer::Text(formatter.into()));
        let plan = t.plan("orbit-tools", &parent, &answers).unwrap_or_else(|e| panic!("{formatter}: {e}"));
        assert!(plan.steps.is_empty(), "{formatter}: it runs nothing");
        let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{formatter}: {path}")).contents.clone();
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{formatter}: {} has a {{{{...}}}} left", f.path);
        }
        let flake = file("flake.nix");
        assert!(parses(&engine, &language, &flake), "{formatter}: flake.nix\n{flake}");
        // What the pack reads from it: the project's formatter is the one picked.
        let scan = crate::project::scan_flake(&flake);
        let want = crate::project::Formatter::from_name(formatter).unwrap();
        assert_eq!((scan.formatter_output, scan.hint), (true, Some(crate::project::Hint::Tool(want))), "{formatter}");
        assert!(flake.contains("bin/orbit-tools") && flake.contains(&format!("pkgs.{formatter}")), "{formatter}");
        assert!(file(".gitignore").lines().any(|l| l == "result"));
        assert!(file("README.md").contains("git add"), "flakes only see what Git tracks");
    }

    let t = load_template_dir(&root.join("nixos-module"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    let plan = t.plan("orbit-tools", &parent, &t.default_answers("orbit-tools")).unwrap_or_else(|e| panic!("{e}"));
    assert!(plan.steps.is_empty());
    let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{path}")).contents.clone();
    for path in ["flake.nix", "module.nix"] {
        let text = file(path);
        assert!(!text.contains("{{") && parses(&engine, &language, &text), "{path}\n{text}");
    }
    assert!(file("module.nix").contains("services.orbit_tools") && file("flake.nix").contains("nixosModules.default"));
    assert!(!file("flake.nix").contains("switch") && !file("README.md").contains("nixos-rebuild switch"), "the template never switches");
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
fn the_manifest_lists_the_commands_the_kit_the_keymap_and_the_fix_rows_use() {
    let m = manifest();
    let commands: Vec<String> = m["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    for want in ["nix.doctor", "nix.install-help", "nix.server.dev-shell", "nix.server.nixpkgs", "nix.flake.build", "nix.flake.run", "nix.flake.develop", "nix.develop"] {
        assert!(commands.iter().any(|c| c == want), "{want}");
    }
    assert_eq!(commands.len(), 8, "{commands:?}");
    for (file, text) in files("keymaps", "toml") {
        let keymap: toml::Table = toml::from_str(&text).unwrap();
        let mut keys = Vec::new();
        for (_, menu) in keymap["local"].as_table().unwrap() {
            for key in menu.as_array().unwrap() {
                let command = key["command"].as_str().unwrap();
                assert!(commands.iter().any(|c| c == command), "{file}: {command} isn't in extension.toml");
                keys.push(command.to_string());
            }
        }
        assert!(!keys.iter().any(|c| c.contains("update") || c.contains("switch")), "no key for what rewrites the lock or the system: {keys:?}");
    }
    // Each fix row runs a command the manifest lists, and each is one the pack answers.
    let tools = crate::tools::Tools::default();
    let project = crate::project::Project { flake: true, ..Default::default() };
    let settings = crate::choose::Settings::default();
    let decision = crate::choose::formatter(&settings, &project, &tools);
    let (programs, extra) = crate::doctor::checks(&project, &tools, &settings, &decision);
    for (_, command, _) in programs.iter().filter_map(|p| p.fix.clone()) {
        assert!(commands.contains(&command), "{command}");
    }
    assert!(commands.contains(&crate::doctor::SERVER_DEV_SHELL.to_string()) && commands.contains(&crate::doctor::SERVER_NIXPKGS.to_string()));
    let with_nix = crate::tools::Tools { nix: Some("/n/nix".into()), ..Default::default() };
    let (programs, extra_with) = crate::doctor::checks(&project, &with_nix, &settings, &decision);
    for command in programs.iter().filter_map(|p| p.fix.clone()).map(|f| f.1).chain(extra.iter().chain(&extra_with).filter_map(|c| c.fix.clone()).map(|f| f.1)) {
        assert!(commands.contains(&command), "{command}");
    }
}

#[test]
fn the_manifest_asks_for_what_the_pack_runs_and_nothing_it_downloads() {
    let m = manifest();
    assert_eq!(m["extension"]["id"].as_str(), Some("fenix.nix"));
    assert_eq!((m["extension"]["version"].as_str(), m["engine"]["fenix"].as_str(), m["engine"]["api"].as_str()), (Some("1.0.0"), Some(">=1.5, <2"), Some("1.5")));
    let process = strings(&m, "permissions", "process");
    assert_eq!(process, crate::tools::NAMES, "each program the pack finds, and no /tools: nothing is installed");
    assert!(m["permissions"].get("network").is_none(), "nothing is downloaded by the pack: nix does its own");
    assert_eq!(m["permissions"]["filesystem"].as_str(), Some("project-read"));
    assert_eq!(m["permissions"]["editor"].as_str(), Some("read"));
    // Every program a task, the formatter, a linter and the server run is named.
    let project = crate::project::Project { flake: true, flake_formatter: true, nixos: true, home_manager: true, default_nix: true, shell_nix: true, ..Default::default() };
    for t in &crate::tasks::tasks(&project, &crate::tools::Tools::default()).tasks {
        assert!(process.contains(&t.cmd.program), "{}", t.cmd.program);
    }
    for f in crate::project::Formatter::ALL {
        assert!(process.iter().any(|p| p == f.name()), "{}", f.name());
    }
    // The settings it reads are the manifest's, with their defaults as text.
    let settings = m["contributes"]["settings"].as_array().unwrap();
    let keys: Vec<&str> = settings.iter().map(|s| s["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["nix.server", "nix.server_runner", "nix.formatter", "nix.statix", "nix.deadnix", "nix.deadnix_arguments"]);
    for s in settings {
        assert!(s["default"].is_str(), "{}: a default as text", s["key"]);
    }
    let defaults = crate::choose::Settings::default();
    let by = |key: &str| settings.iter().find(|s| s["key"].as_str() == Some(key)).unwrap();
    assert_eq!(by("nix.server_runner")["default"].as_str(), Some("path"), "nothing starts through Nix until it's set");
    assert_eq!(by("nix.server_runner")["project"].as_bool(), Some(true));
    assert_eq!((defaults.runner, defaults.statix, defaults.deadnix), (crate::choose::Runner::Path, true, true));
    assert_eq!(strings_of(by("nix.formatter")["choices"].as_array().unwrap()), ["auto", "nixfmt", "alejandra", "nixpkgs-fmt", "off"]);
    assert_eq!(strings_of(by("nix.server")["choices"].as_array().unwrap()), ["auto", "nixd", "nil"]);
    assert_eq!(strings_of(by("nix.server_runner")["choices"].as_array().unwrap()), ["path", "dev-shell", "nixpkgs"]);
    // Its context loads it for the language, and for a project with one of these.
    let context = &m["contributes"]["contexts"].as_array().unwrap()[0];
    assert_eq!(strings_of(context["languages"].as_array().unwrap()), ["nix"]);
    assert_eq!(strings_of(context["markers"].as_array().unwrap()), ["flake.nix", "shell.nix", "default.nix"]);
}

fn strings_of(a: &[toml::Value]) -> Vec<String> {
    a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

#[test]
fn the_project_kind_is_declared_only() {
    let m = manifest();
    let kinds = m["contributes"]["project_kinds"].as_array().unwrap();
    assert_eq!(kinds.len(), 1);
    let k = &kinds[0];
    assert_eq!((k["id"].as_str(), k["tag"].as_str(), k["name"].as_str()), (Some("nix"), Some("NIX"), Some("Nix")));
    // A flake.nix, shell.nix or default.nix is usually another project's dev shell or
    // package, and a kind's markers win over other packs' kinds (ordered by id): none
    // is one, so `kind = "nix"` says it. They're the context's, which only loads the pack.
    assert!(k.get("markers").is_none(), "declared only");
    assert_eq!(strings_of(k["main"].as_array().unwrap()), ["flake.nix", "default.nix"]);
    // Templates stay `other`: a kind of the pack's own would need Fenix 1.6.
    for t in ["nix-flake", "nixos-module"] {
        let text = std::fs::read_to_string(dir("templates").join(t).join("template.toml")).unwrap();
        assert!(text.contains("kind = \"other\""), "{t}");
    }
}
