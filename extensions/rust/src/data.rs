//! The pack's data, read as Fenix reads it: its snippets and project
//! template by Fenix's own crates, its launch files by the same rules as
//! `fenix-gui`'s (`dap::Template`); and the script the launch files give
//! lldb-dap, run on a crate for real.

use std::path::{Path, PathBuf};

use fenix_lang_kit::serde_json;
use fenix_project::template::{load_template_dir, Origin};
use serde::Deserialize;

/// What the launch files have lldb-dap run before it makes its target.
const SCRIPT: &str = include_str!("debug/fenix_cargo.py");

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

/// `fenix-gui`'s `dap::Template`.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    name: String,
    languages: Vec<String>,
    #[serde(default)]
    request: Option<String>,
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    attributes: toml::Table,
}

#[test]
fn every_snippet_reads_as_fenix_reads_it_and_none_is_rust_snippets() {
    let snippets = files("snippets/rust", "snippet");
    assert!(snippets.len() >= 20);
    let theirs: Vec<String> = std::fs::read_dir(dir("../rust-snippets/snippets/rust")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().trim_end_matches(".snippet").to_string()).collect();
    assert!(theirs.contains(&"impl".to_string()), "rust-snippets is beside it");
    let mut keys = Vec::new();
    for (file, text) in &snippets {
        let s = fenix_snippets::Snippet::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(s.scopes, ["rust"], "{file}");
        assert_eq!(format!("{}.snippet", s.trigger), *file, "a snippet's file is its key");
        assert!(!theirs.contains(&s.trigger), "{file}: rust-snippets has it");
        keys.push(s.trigger);
    }
    for key in ["thiserr", "errv", "amain", "atest", "spawn", "select", "builder", "from", "fromstr", "default", "iter"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
}

#[test]
fn every_launch_file_has_lldb_dap_run_the_script_first() {
    let launches = files("launch", "toml");
    assert_eq!(launches.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), ["binary.toml", "example.toml", "test.toml"]);
    // lldb runs what's in backticks in a command; the script is one
    // TOML string, written as JSON writes it.
    assert!(!SCRIPT.contains('`') && SCRIPT.is_ascii());
    let exec = format!("?script exec({}, globals())", serde_json::to_string(SCRIPT).unwrap());
    for (file, text) in &launches {
        let l: Launch = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(l.languages, ["rust"], "{file}");
        assert_eq!(l.program.as_deref(), Some("${project}/target/fenix-debug/debuggee"), "{file}");
        let init: Vec<&str> = l.attributes["initCommands"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        assert_eq!(init[0], exec, "{file}: the script isn't src/debug/fenix_cargo.py as it is");
        let kind = match file.as_str() {
            "binary.toml" => "bin",
            "example.toml" => "example",
            _ => "test",
        };
        assert_eq!(init[1], format!("script _fenix = fenix_cargo('{kind}', r'''${{file}}''', r'''${{project}}/target/fenix-debug/debuggee''')"), "{file}");
    }
}

/// The script, run by Python on `file` (no lldb, so no formatters and no
/// run-args): what it printed and gave back.
fn script(kind: &str, file: &Path, out: &Path) -> String {
    let code = format!("{SCRIPT}\nprint('->', fenix_cargo({kind:?}, {:?}, {:?}))", file.to_string_lossy(), out.to_string_lossy());
    let run = std::process::Command::new("python3").arg("-c").arg(code).env_remove("CARGO_TARGET_DIR").output().expect("python3");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn the_script_builds_what_the_file_says_and_links_it() {
    let root = crate::project::tests::temp("script");
    std::fs::create_dir_all(root.join("src/bin")).unwrap();
    std::fs::create_dir_all(root.join("examples")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"orbit\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub mod maths;\n").unwrap();
    std::fs::write(root.join("src/maths.rs"), "pub fn double(x: u32) -> u32 { x * 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn doubles() { assert_eq!(super::double(2), 4); }\n}\n").unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() { println!(\"{}\", orbit::maths::double(21)); }\n").unwrap();
    std::fs::write(root.join("src/bin/probe.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("examples/demo.rs"), "fn main() {}\n").unwrap();
    let out = root.join("target/fenix-debug/debuggee");

    let said = script("test", &root.join("src/maths.rs"), &out);
    assert!(said.contains("fenix: cargo test --no-run --message-format=json-render-diagnostics -p orbit --lib") && said.ends_with("-> maths::\n"), "{said}");
    assert!(out.is_file());
    let tests = std::process::Command::new(&out).arg("--list").output().unwrap();
    assert!(String::from_utf8_lossy(&tests.stdout).contains("maths::tests::doubles: test"), "the lib's tests");
    assert_eq!(std::fs::read_to_string(root.join("target/fenix-debug/.gitignore")).unwrap(), "*\n");

    let said = script("bin", &root.join("src/maths.rs"), &out);
    assert!(said.contains("--bin orbit"), "a module's crate's own binary: {said}");
    assert_eq!(String::from_utf8_lossy(&std::process::Command::new(&out).output().unwrap().stdout), "42\n");
    assert!(script("bin", &root.join("src/bin/probe.rs"), &out).contains("--bin probe"));
    assert!(script("example", &root.join("examples/demo.rs"), &out).contains("--example demo"));

    assert!(script("example", &root.join("src/main.rs"), &out).contains("--example demo"), "the crate's only one");
    // With two, a file that's neither: nothing built, nothing to debug.
    std::fs::write(root.join("examples/other.rs"), "fn main() {}\n").unwrap();
    let said = script("example", &root.join("src/main.rs"), &out);
    assert!(said.contains("fenix: nothing in orbit to debug as an example -- open its file") && !out.exists(), "{said}");
    std::fs::write(root.join("src/bin/probe.rs"), "fn main() { let x: u8 = \"no\"; }\n").unwrap();
    let said = script("bin", &root.join("src/bin/probe.rs"), &out);
    assert!(said.contains("error[E0308]") && said.contains("fenix: the build failed") && !out.exists(), "{said}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_axum_template_plans_with_its_defaults() {
    let parent = crate::project::tests::temp("templates");
    let t = load_template_dir(&dir("templates").join("rust-axum"), Origin::Extension(dir("templates"))).unwrap_or_else(|e| panic!("{e}"));
    let answers = t.default_answers("orbit-desk");
    let plan = t.plan("orbit-desk", &parent, &answers).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(plan.steps.iter().map(|s| s.display().split(' ').take(3).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>(), ["cargo init --bin", "cargo add axum", "cargo add --dev"]);
    let all: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(all, [".gitignore", "src/main.rs", "src/app.rs"], "written before cargo init, which keeps them");
    let main = &plan.files[1].contents;
    assert!(main.contains("unwrap_or(3000)") && main.contains("orbit_desk=debug"), "{main}");
    for f in &plan.files {
        assert!(!f.contents.contains("{{"), "{} has a {{{{...}}}} left", f.path);
    }
    std::fs::remove_dir_all(&parent).unwrap();
}
