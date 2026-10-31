//! The pack's data, read as Fenix reads it: its snippets and project
//! templates by Fenix's own crates, its language file by the same rules
//! as `fenix-gui`'s.

use std::path::{Path, PathBuf};

use fenix_project::template::{load_template_dir, Origin};
use serde::Deserialize;

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

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_renders_whole() {
    let snippets = files("snippets/lua", "snippet");
    assert!(snippets.len() >= 15);
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["lua"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!text.contains("    "), "{file}: indented with tabs, as StyLua does");
        let shown = s.template.render(&Default::default(), &Default::default(), "").text;
        assert!(!shown.contains("${") && !shown.contains("$0"), "{file}: a field left: {shown}");
        keys.push(s.trigger);
    }
    for key in ["fn", "lfn", "mod", "for", "fori", "forp", "if", "ife", "pcall", "class", "describe", "it", "love", "keymap"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn the_class_snippet_mirrors_its_name() {
    let s = fenix_snippets::Snippet::parse(include_str!("../snippets/lua/class.snippet")).unwrap();
    let shown = s.template.render(&Default::default(), &Default::default(), "").text;
    assert!(shown.starts_with("local Class = {}\nClass.__index = Class\n"), "{shown}");
    assert!(shown.contains("setmetatable({}, Class)") && shown.ends_with("return Class\n"), "{shown}");
}

/// `fenix-gui`'s `languages::File`.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Language {
    language: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    filenames: Vec<String>,
    server: Option<String>,
    grammar: Option<String>,
    comment: Option<String>,
}

#[test]
fn the_language_file_adds_files_to_fenixs_own_lua() {
    let l: Language = toml::from_str(include_str!("../languages/lua.toml")).unwrap();
    assert_eq!(l.language, "lua");
    assert!(l.extensions.contains(&"rockspec".to_string()) && l.filenames.contains(&".luacheckrc".to_string()));
    assert!(!l.extensions.contains(&"lua".to_string()), "Fenix's own");
    assert!(l.server.is_none() && l.grammar.is_none() && l.comment.is_none(), "the server is the code's to set, the rest is Fenix's");
}

#[test]
fn each_project_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    for (id, opens) in [("lua-love", "main.lua"), ("lua-rock", "src/orbit_desk.lua")] {
        let t = load_template_dir(&dir("templates").join(id), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
        let answers = t.default_answers("orbit-desk");
        let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(plan.steps.is_empty(), "{id}: nothing to run");
        let all: Vec<&str> = plan.files.iter().chain(&plan.after_files).map(|f| f.path.as_str()).collect();
        assert!(all.contains(&opens) && all.contains(&".gitignore") && all.contains(&"stylua.toml") && all.contains(&".luarc.json"), "{id}: {all:?}");
        for f in plan.files.iter().chain(&plan.after_files) {
            assert!(!f.contents.contains("{{"), "{id}: {} has a {{{{...}}}} left", f.path);
        }

        // What the pack makes of it: a LÖVE game; a rock with specs.
        let made = parent.join(id);
        for f in plan.files.iter().chain(&plan.after_files) {
            let path = made.join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.contents).unwrap();
        }
        let p = crate::project::Project::read(&made.to_string_lossy());
        assert!(p.lua && p.stylua && p.luarc, "{id}: {p:?}");
        match id {
            "lua-love" => assert!(p.love && p.rockspecs.is_empty()),
            _ => assert!(p.busted && p.rockspecs == ["orbit-desk-dev-1.rockspec"] && !p.love),
        }
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
