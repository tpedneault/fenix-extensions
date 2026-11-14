//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language files by the same rules as
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
fn the_language_files_read_as_fenix_reads_them() {
    let all = files("languages", "toml");
    assert_eq!(all.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["bibtex.toml", "latex.toml"]);
    let mut seen: Vec<String> = Vec::new();
    for (name, want) in [("latex", ["tex", "sty", "cls"].as_slice()), ("bibtex", &["bib"])] {
        let l = language_file(name);
        assert_eq!(l.extensions, want, "{name}");
        assert!(l.server.is_none(), "{name}: the server is the code's to set");
        assert_eq!(l.comment.as_deref(), Some("%"), "{name}");
        for e in &l.extensions {
            assert!(!seen.contains(e), "{e} is one language's");
            seen.push(e.clone());
        }
    }
    // The server is told these names, which are texlab's own.
    assert_eq!(crate::SERVED, ["latex", "bibtex"]);
    for l in crate::FORMATTED {
        assert!(all.iter().any(|(f, _)| f == &format!("{l}.toml")), "{l}");
    }
}

#[test]
fn the_grammars_licences_and_sources_are_beside_them() {
    let readme = std::fs::read_to_string(dir("grammars/README.md")).unwrap();
    for file in ["tree-sitter-latex.wasm", "tree-sitter-bibtex.wasm"] {
        let bytes = std::fs::read(dir("grammars").join(file)).unwrap();
        assert!(bytes.starts_with(b"\0asm"), "{file}");
        let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert!(readme.contains(&sha), "{file}: its SHA-256 {sha} is in the README");
    }
    for commit in ["7e0ecdc02926c7b9b2e0c76003d4fe7b0944f957", "8d04ed27b3bc7929f14b7df9236797dab9f3fa66"] {
        assert!(readme.contains(commit), "the commit {commit} they were built from");
    }
    for (file, holder) in [("LICENSE-tree-sitter-latex", "Patrick Förster"), ("LICENSE-tree-sitter-bibtex", "Patrick Förster")] {
        let licence = std::fs::read_to_string(dir("grammars").join(file)).unwrap();
        assert!(licence.starts_with("MIT License") && licence.contains(holder), "{file}");
    }
    assert!(readme.contains("MIT"));
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    for (folder, language, wanted, minimum) in [
        ("latex", "latex", ["doc", "sec", "env", "ite", "fig", "tab", "eq", "ali", "frame", "thm", "cmd", "bib", "cite", "pkg", "im"].as_slice(), 30),
        ("bibtex", "bibtex", &["art", "book", "inp", "thesis", "online", "misc", "str"], 7),
    ] {
        let snippets = files(&format!("snippets/{folder}"), "snippet");
        assert!(snippets.len() >= minimum, "{folder}: {}", snippets.len());
        let mut keys = Vec::new();
        for (file, text) in &snippets {
            let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(s.scopes, [language], "{file}");
            assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
            let shown = s.template.render(&Default::default(), &Default::default(), "").text;
            assert!(!shown.contains("$0") && !shown.contains("${1") && !shown.contains("$1"), "{file}: a field left: {shown}");
            assert!(!text.contains('\t'), "{file}: indented with spaces");
            keys.push(s.trigger);
        }
        for key in wanted {
            assert!(keys.iter().any(|k| k == key), "{folder}: {key}");
        }
    }
    // What the escapes make: a backslash, a line break, a dollar sign, a closing brace.
    let render = |path: &str| {
        let s = fenix_snippets::Snippet::parse(&std::fs::read_to_string(dir(path)).unwrap()).unwrap();
        s.template.render(&Default::default(), &Default::default(), "").text
    };
    assert!(render("snippets/latex/tab.snippet").contains("Name & Value & Error \\\\\n"), "a row ends with \\\\");
    assert_eq!(render("snippets/latex/im.snippet").trim_end(), "$x$");
    assert!(render("snippets/latex/cmd.snippet").starts_with("\\newcommand{\\name}[1]{#1}"), "{}", render("snippets/latex/cmd.snippet"));
    assert!(render("snippets/bibtex/art.snippet").trim_end().ends_with("}"), "the entry is closed");
    assert!(render("snippets/bibtex/str.snippet").starts_with("@string{name = \"value\"}"));
}

/// What a text is shown as: the capture of the latest pattern.
fn shown<'a>(highlights: &'a [(String, String, usize)], text: &str) -> Option<&'a str> {
    highlights.iter().filter(|(t, _, _)| t == text).max_by_key(|(_, _, p)| *p).map(|(_, n, _)| n.as_str())
}

/// Each name is one Fenix colours, by its first part.
fn colours_known(highlights: &[(String, String, usize)]) {
    let known = ["variable", "type", "function", "keyword", "string", "comment", "number", "boolean", "constant", "property", "label", "module", "operator", "punctuation", "text", "markup", "parameter"];
    for (_, name, _) in highlights {
        let top = name.split('.').next().unwrap();
        assert!(known.contains(&top), "{name}");
    }
}

fn capture_folds(language: &Language, tree: &tree_sitter::Tree, queries: &Path, source: &str) -> Vec<String> {
    let folds = captures(language, tree, &std::fs::read_to_string(queries.join("folds.scm")).unwrap(), source);
    folds.iter().filter(|(_, n, _)| n == "fold").map(|(t, _, _)| t.lines().next().unwrap().to_string()).collect()
}

#[test]
fn latexs_grammar_loads_and_its_queries_run() {
    let (engine, language, queries) = load("latex");
    let source = include_str!("samples/report.tex");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());

    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    let mut wrong: Vec<String> = Vec::new();
    for (text, name) in [
        ("% A paper: sections, environments, math and references, for the grammar's queries.", "comment"),
        ("\\documentclass", "keyword"),
        ("\\usepackage", "keyword"),
        ("\\section", "keyword"),
        ("\\subsection", "keyword"),
        ("\\begin", "keyword"),
        ("\\end", "keyword"),
        ("\\label", "keyword"),
        ("\\cite", "keyword"),
        ("\\ref", "keyword"),
        ("\\input", "keyword"),
        ("\\newcommand", "keyword"),
        ("\\emph", "function"),
        ("\\textbf", "function"),
        ("\\maketitle", "function"),
        ("itemize", "type"),
        ("equation", "type"),
        ("sec:intro", "label"),
        ("knuth84", "label"),
        ("amsmath", "string"),
        ("fig/orbit.pdf", "string"),
        ("$x^2 + y_1$", "string.special"),
        ("https://tug.org", "text.uri"),
        ("{", "punctuation.bracket"),
    ] {
        if shown(&highlights, text) != Some(name) {
            wrong.push(format!("{text}: wanted {name}, got {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>()));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    colours_known(&highlights);
    // A section's title is a title.
    assert!(highlights.iter().any(|(t, n, _)| t == "{Introduction}" && n == "text.title"), "{:?}", highlights.iter().filter(|(_, n, _)| n == "text.title").collect::<Vec<_>>());

    // What folds: a section down to the next, an environment, math.
    let folded = capture_folds(&language, &tree, &queries, source);
    for start in ["\\section{Introduction}\\label{sec:intro}", "\\subsection{Method}", "\\begin{itemize}", "\\begin{equation}\\label{eq:one}", "\\begin{verbatim}"] {
        assert!(folded.iter().any(|f| f == start), "{start}: {folded:?}");
    }

    // Enter indents inside an environment: an @indent starting on that line.
    let indents = captures(&language, &tree, &std::fs::read_to_string(queries.join("indents.scm")).unwrap(), source);
    assert!(indents.iter().all(|(_, n, _)| n == "indent"));
    assert!(indents.iter().any(|(t, _, _)| t.starts_with("\\begin{itemize}")), "an environment: {}", indents.len());
}

#[test]
fn bibtexs_grammar_loads_and_its_queries_run() {
    let (engine, language, queries) = load("bibtex");
    let source = include_str!("samples/refs.bib");
    let tree = parse(&engine, &language, source);
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
    let highlights = captures(&language, &tree, &std::fs::read_to_string(queries.join("highlights.scm")).unwrap(), source);
    let mut wrong: Vec<String> = Vec::new();
    for (text, name) in [
        ("@book", "keyword"),
        ("@article", "keyword"),
        ("@string", "keyword"),
        ("author", "property"),
        ("title", "property"),
        ("knuth84", "label"),
        ("Donald", "string"),
        ("Lamport", "string"),
        ("1984", "number"),
        ("tug", "constant"),
        ("=", "operator"),
        ("{", "punctuation.bracket"),
        (",", "punctuation.delimiter"),
    ] {
        if shown(&highlights, text) != Some(name) {
            wrong.push(format!("{text}: wanted {name}, got {:?}", highlights.iter().filter(|(t, _, _)| t == text).collect::<Vec<_>>()));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert!(highlights.iter().any(|(t, n, _)| n == "comment" && t.starts_with("% A database")), "the line comment");
    assert!(highlights.iter().any(|(t, n, _)| t == "tug" && n == "parameter"), "a string used in a value");
    colours_known(&highlights);
    let folded = capture_folds(&language, &tree, &queries, source);
    assert!(folded.iter().any(|f| f == "@book{knuth84,") && folded.iter().any(|f| f == "@article{lamport94,"), "{folded:?}");
}

#[test]
fn the_article_beamer_and_thesis_templates_plan() {
    use fenix_project::template::{load_template_dir, Answer, Origin};
    let parent = crate::project::tests::temp("templates");
    let root = dir("templates");
    let plan_of = |t: &fenix_project::template::Template, name: &str, set: &[(&str, &str)]| {
        let mut answers = t.default_answers(name);
        for (k, v) in set {
            answers.insert((*k).into(), Answer::Text((*v).into()));
        }
        let plan = t.plan(name, &parent, &answers).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(plan.steps.is_empty(), "it runs nothing");
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{}", f.path);
        }
        plan
    };
    let file = |plan: &fenix_project::template::Plan, path: &str| plan.files.iter().chain(&plan.after_files).find(|f| f.path == path).unwrap_or_else(|| panic!("{path}")).contents.clone();

    let article = load_template_dir(&root.join("latex-article"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    assert!(article.needs.is_empty(), "any builder will do");
    for paper in ["a4paper", "letterpaper"] {
        let plan = plan_of(&article, "orbit-notes", &[("paper", paper), ("author", "Grace Hopper")]);
        let main = file(&plan, "main.tex");
        assert!(main.contains(&format!("\\documentclass[11pt,{paper}]{{article}}")) && main.contains("Grace Hopper") && main.contains("orbit-notes") && main.contains("\\bibliography{refs}"), "{paper}");
        assert!(file(&plan, "refs.bib").contains("@book{knuth84"));
        assert!(file(&plan, ".latexmkrc").contains("$pdf_mode = 1;"));
        assert!(file(&plan, ".gitignore").lines().any(|l| l == "*.synctex.gz"));
    }

    let beamer = load_template_dir(&root.join("latex-beamer"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    let plan = plan_of(&beamer, "talk", &[("theme", "metropolis")]);
    let main = file(&plan, "main.tex");
    assert!(main.contains("\\documentclass[aspectratio=169]{beamer}") && main.contains("\\usetheme{%\n  metropolis%\n}"), "{main}");

    let thesis = load_template_dir(&root.join("latex-thesis"), Origin::Extension(root.clone())).unwrap_or_else(|e| panic!("{e}"));
    let plan = plan_of(&thesis, "my-thesis", &[("degree", "Doctor of Philosophy")]);
    for chapter in ["introduction", "background", "conclusion", "appendix"] {
        let text = file(&plan, &format!("chapters/{chapter}.tex"));
        assert!(text.starts_with("% !TEX root = ../main.tex\n"), "{chapter}: it names its root");
        assert!(file(&plan, "main.tex").contains(&format!("\\include{{chapters/{chapter}}}")), "{chapter}");
    }
    assert!(file(&plan, "main.tex").contains("Doctor of Philosophy") && file(&plan, "refs.bib").contains("knuth84"));
    // Each document's own magic comment is the one the pack reads.
    assert_eq!(crate::project::magic(&file(&plan, "chapters/background.tex")).root.as_deref(), Some("../main.tex"));
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
        [
            "latex.doctor",
            "latex.install.texlab",
            "latex.install.tectonic",
            "latex.install.tex-fmt",
            "latex.build",
            "latex.watch",
            "latex.preview",
            "latex.forward_search",
            "latex.clean",
            "latex.clean_all",
            "latex.word_count"
        ]
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
    // Each fix row, on every platform, runs a command the manifest lists.
    for (os, arch) in [("linux", "x86_64"), ("windows", "aarch64"), ("macos", "aarch64")] {
        let platform = crate::tools::Platform::new(os, arch);
        let mut fixes: Vec<String> = crate::doctor::programs(&platform).iter().filter_map(|p| p.fix.clone()).map(|f| f.1).collect();
        let check = crate::doctor::build_check(&crate::tools::Tools::default(), &crate::project::Project { latex: true, ..Default::default() }, &platform, None);
        fixes.extend(check.fix.map(|f| f.1));
        for command in fixes {
            assert!(commands.contains(&command), "{command}");
        }
    }
}

#[test]
fn the_manifest_asks_for_what_the_pack_runs_and_downloads() {
    let m = manifest();
    assert_eq!(m["extension"]["id"].as_str(), Some("fenix.latex"));
    assert_eq!((m["engine"]["fenix"].as_str(), m["engine"]["api"].as_str()), (Some(">=1.5, <2"), Some("1.5")));
    let process = strings(&m, "permissions", "process");
    assert_eq!(process, ["latexmk", "tectonic", "pdflatex", "xelatex", "lualatex", "texlab", "tex-fmt", "latexindent", "chktex", "texcount", "synctex", "/tools/*"]);
    // Every program a task, a builder, the formatter and the linter run is named.
    let tools = crate::tools::Tools { texcount: Some("texcount".into()), ..Default::default() };
    for builder in [
        crate::build::Builder::Latexmk { path: "latexmk".into(), engine: None },
        crate::build::Builder::Tectonic { path: "tectonic".into() },
        crate::build::Builder::Engine { name: "xelatex".into(), path: "xelatex".into() },
    ] {
        for t in &crate::tasks::tasks(Some("main.tex"), Some(&builder), &tools).tasks {
            assert!(process.contains(&t.cmd.program), "{}", t.cmd.program);
        }
    }
    for name in crate::build::ENGINES {
        assert!(process.iter().any(|p| p == name), "{name}");
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
    assert_eq!(settings, [("latex.main", ""), ("latex.preview", "true"), ("latex.build_on_save", "false"), ("latex.chktex", "true"), ("latex.formatter", "auto")]);
    assert_eq!(crate::Settings::default(), crate::Settings { main: None, preview: true, build_on_save: false, chktex: true, formatter: "auto".into() });
    // Its context loads it for each language it brings.
    let context = &m["contributes"]["contexts"].as_array().unwrap()[0];
    let languages: Vec<&str> = context["languages"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(languages, ["latex", "bibtex"]);
    // The project kind: markers only for what says a project is LaTeX's.
    let kind = &m["contributes"]["project_kinds"].as_array().unwrap()[0];
    let markers: Vec<&str> = kind["markers"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!((kind["id"].as_str(), kind["tag"].as_str(), markers.as_slice(), kind["main"].as_array().unwrap()[0].as_str()), (Some("latex"), Some("TEX"), [".latexmkrc", "latexmkrc", "Tectonic.toml"].as_slice(), Some("main.tex")));
    // The PDF reader isn't a dependency: it's asked for when it's there.
    assert!(m.get("dependencies").is_none());
}
