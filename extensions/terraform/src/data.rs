//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its language files by the same rules as
//! `fenix-gui`'s (`languages::File`), the grammars loaded and their queries
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

/// A grammar as Fenix loads it from a language file: the file's, the
/// `tree_sitter_<symbol>` it exports (the file's name without
/// `tree-sitter-`, unless `symbol` says), the queries' folder.
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
fn the_language_files_read_as_fenix_reads_them() {
    let all = files("languages", "toml");
    assert_eq!(all.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["hcl.toml", "terraform-vars.toml", "terraform.toml"]);
    let mut seen: Vec<String> = Vec::new();
    for (name, want) in [("terraform", ["tf"].as_slice()), ("terraform-vars", &["tfvars"]), ("hcl", &["hcl"])] {
        let l = language_file(name);
        assert_eq!(l.extensions, want, "{name}");
        assert!(l.server.is_none(), "{name}: the server is the code's to set");
        assert_eq!(l.comment.as_deref(), Some("#"), "{name}");
        for e in &l.extensions {
            assert!(!seen.contains(e), "{e} is one language's");
            seen.push(e.clone());
        }
    }
    // The server is told these names, which are terraform-ls's own.
    assert_eq!(crate::SERVED, ["terraform", "terraform-vars"]);
    for l in crate::FORMATTED {
        assert!(all.iter().any(|(f, _)| f == &format!("{l}.toml")), "{l}");
    }
}

#[test]
fn the_grammars_licence_and_source_are_beside_them() {
    let readme = std::fs::read_to_string(dir("grammars/README.md")).unwrap();
    for file in ["tree-sitter-terraform.wasm", "tree-sitter-hcl.wasm"] {
        let bytes = std::fs::read(dir("grammars").join(file)).unwrap();
        assert!(bytes.starts_with(b"\0asm"), "{file}");
        let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert!(readme.contains(&sha), "{file}: its SHA-256 {sha} is in the README");
    }
    let licence = std::fs::read_to_string(dir("grammars/LICENSE-tree-sitter-hcl")).unwrap();
    assert!(licence.contains("Apache License") && licence.contains("Version 2.0"));
    assert!(readme.contains("Apache-2.0"));
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/terraform", "snippet");
    assert!(snippets.len() >= 15, "{}", snippets.len());
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["terraform"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("$0") && !shown.contains("${1") && !shown.contains("$1"), "{file}: a field left: {shown}");
        assert!(!text.contains('\t'), "{file}: Terraform is indented with spaces");
        keys.push(s.trigger);
    }
    for key in ["res", "var", "out", "mod", "prov", "loc", "data", "foreach", "count", "dyn", "tf"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn terraforms_grammar_loads_and_its_queries_run() {
    let (engine, language, queries) = load("terraform");
    let source = include_str!("samples/orbit.tf");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    // What Fenix shows a text as: the capture of the latest pattern.
    let shown = |text: &str| highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str());
    for (text, name) in [
        ("# The ground station fleet.", "comment"),
        ("terraform", "keyword"),
        ("resource", "keyword"),
        ("variable", "keyword"),
        ("locals", "keyword"),
        ("lifecycle", "keyword"),
        ("aws_instance", "string.special"),
        ("required_version", "property"),
        ("\"hashicorp/aws\"", "string"),
        ("Kiruna", "string"),
        ("kir", "property"),
        ("for_each", "property"),
        ("var", "variable.builtin"),
        ("each", "variable.builtin"),
        ("count", "variable.builtin"),
        ("data", "variable.builtin"),
        ("local", "variable.builtin"),
        ("aws_ami", "property"),
        ("upper", "function.builtin"),
        ("length", "function.builtin"),
        ("values", "function.builtin"),
        ("provider::aws::arn_parse", "function"),
        ("true", "boolean"),
        ("null", "constant.builtin"),
        ("0", "number"),
        ("1.5e3", "number"),
        ("EOT", "string.special"),
        ("${", "punctuation.special"),
        ("%{", "punctuation.special"),
        ("~", "punctuation.special"),
        ("endfor", "keyword.repeat"),
        ("in", "keyword"),
        ("!=", "operator"),
        ("?", "operator"),
        ("=>", "operator"),
        ("{", "punctuation.bracket"),
        ("[*]", "punctuation.delimiter"),
    ] {
        assert_eq!(shown(text), Some(name), "{text}: {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>());
    }
    assert_eq!(shown("station"), Some("string.special"), "the block's second label");
    // Each name is one Fenix colours, by its first part.
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation"];
    for (_, name, _) in &highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }

    // What folds: blocks, objects, lists, the heredoc.
    let folds = captures(&language, &tree, &std::fs::read_to_string(queries.join("folds.scm")).unwrap(), source);
    let folded: Vec<&str> = folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap()).collect();
    for start in ["{", "resource \"aws_instance\" \"station\" {", "lifecycle {", "<<-EOT", "[for code, name in var.stations : upper(name) if code != \"wal\"]"] {
        assert!(folded.iter().any(|f| f.ends_with(start) || *f == start), "{start}: {folded:?}");
    }

    // Enter indents inside a block: an @indent starting on that line.
    let indents = captures(&language, &tree, &std::fs::read_to_string(queries.join("indents.scm")).unwrap(), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("module \"net\" {")), "a block: {}", indents.len());
}

#[test]
fn the_same_queries_run_on_hcl_and_on_tfvars() {
    let source = "# A lock file's shape.\nprovider \"registry.terraform.io/hashicorp/aws\" {\n  version     = \"5.31.0\"\n  constraints = \"~> 5.0\"\n  hashes = [\n    \"h1:abc=\",\n  ]\n}\n";
    for name in ["hcl", "terraform-vars"] {
        let (engine, language, queries) = load(name);
        assert_eq!(queries, dir("queries/terraform"));
        let tree = parse(&engine, &language, source);
        assert!(!tree.root_node().has_error(), "{name}: {}", tree.root_node().to_sexp());
        for file in ["highlights", "folds", "indents"] {
            let q = std::fs::read_to_string(queries.join(format!("{file}.scm"))).unwrap();
            let found = captures(&language, &tree, &q, source);
            assert!(!found.is_empty(), "{name} {file}");
        }
        let h = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
        assert!(h.iter().any(|(t, n, _)| t == "provider" && n == "keyword") && h.iter().any(|(t, n, _)| t == "version" && n == "property"), "{name}");
    }
    // A variable file is attributes alone.
    let (engine, language, _) = load("terraform-vars");
    let tree = parse(&engine, &language, "region = \"eu-west-1\"\nstations = {\n  kir = 1\n}\n");
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}

#[test]
fn the_project_template_plans_for_each_provider() {
    use fenix_project::template::{load_template_dir, Answer, Origin};
    let parent = crate::project::tests::temp("templates");
    let root = dir("templates");
    let t = load_template_dir(&root.join("terraform-root-module"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.needs, ["terraform"]);
    for (provider, resource, outputs) in [
        ("none", "terraform_data", "terraform_data.greeting.output"),
        ("aws", "aws_caller_identity", "account_id"),
        ("azurerm", "azurerm_client_config", "subscription_id"),
        ("google", "google_client_config", "project"),
    ] {
        let mut answers = t.default_answers("orbit-fleet");
        answers.insert("provider".into(), Answer::Text(provider.into()));
        let plan = t.plan("orbit-fleet", &parent, &answers).unwrap_or_else(|e| panic!("{provider}: {e}"));
        assert!(plan.steps.is_empty(), "{provider}: it runs nothing, so it can't fail offline");
        let file = |path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{provider}: {path}")).contents.clone();
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{provider}: {} has a {{{{...}}}} left", f.path);
        }
        assert!(file("main.tf").contains(resource) && file("outputs.tf").contains(outputs), "{provider}");
        assert!(file("variables.tf").contains("default     = \"orbit-fleet\""), "{provider}: named for the project");
        assert_eq!(file("versions.tf").contains("required_providers"), provider != "none", "{provider}");
        let ignore = file(".gitignore");
        for kept_out in [".terraform/", "*.tfstate", "tfplan"] {
            assert!(ignore.lines().any(|l| l == kept_out), "{provider}: {kept_out}");
        }
        assert!(!ignore.lines().any(|l| l == ".terraform.lock.hcl"), "{provider}: the lock file is committed");
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
fn the_manifest_lists_the_commands_the_kit_and_the_keymap_use() {
    let m = manifest();
    let commands: Vec<String> = m["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(commands, ["terraform.doctor", "terraform.install.terraform-ls"]);
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
    let programs = crate::doctor::programs(&crate::project::Project { module: Some(".".into()) }, &crate::tools::Platform::new("linux", "x86_64"));
    for (_, command, _) in programs.iter().filter_map(|p| p.fix.clone()) {
        assert!(commands.contains(&command), "{command}");
    }
}

#[test]
fn the_manifest_asks_for_what_the_pack_runs_and_downloads() {
    let m = manifest();
    assert_eq!(m["extension"]["id"].as_str(), Some("fenix.terraform"));
    assert_eq!((m["engine"]["fenix"].as_str(), m["engine"]["api"].as_str()), (Some(">=1.5, <2"), Some("1.5")));
    let process = strings(&m, "permissions", "process");
    assert_eq!(process, ["terraform", "terraform-ls", "/tools/*"]);
    // Every program a task, the formatter and the linter run is named.
    let tasks = crate::tasks::tasks(&crate::project::Project { module: Some(".".into()) }, &crate::tools::Tools::default(), "/nowhere");
    for t in &tasks.tasks {
        assert!(process.contains(&t.cmd.program), "{}", t.cmd.program);
    }
    // Every host a release is downloaded from is named, and no other.
    let hosts = strings(&m, "permissions", "network");
    assert_eq!(hosts, ["releases.hashicorp.com"]);
    for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64")] {
        for i in crate::tools::Platform::new(os, arch).installs() {
            let host = i.url.strip_prefix("https://").unwrap().split('/').next().unwrap();
            assert!(hosts.iter().any(|h| h == host), "{}", i.url);
        }
    }
    // The setting it reads is the manifest's, with its default as text.
    let settings = m["contributes"]["settings"].as_array().unwrap();
    assert_eq!(settings.len(), 1);
    assert_eq!((settings[0]["key"].as_str(), settings[0]["default"].as_str()), (Some("terraform.validate"), Some("true")));
    // Its context loads it for each language it brings.
    let context = &m["contributes"]["contexts"].as_array().unwrap()[0];
    let languages: Vec<&str> = context["languages"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(languages, ["terraform", "terraform-vars", "hcl"]);
}
