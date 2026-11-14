//! `cargo xtask new <kind> <name> [--name "Display name"]`: a new
//! extension, started from a template, in `extensions/<name>/`. A bare
//! `<name>` is this repository's, `fenix.<name>`; `<publisher.name>` is
//! one its author will host.
//!
//! ```text
//! theme          a light theme: manifest, one theme file
//! keymap         a flavour over standard keys: three [[keys]]
//! snippets       two snippets, for every language
//! language-pack  a pack on lang-kit: a language file, a doctor check, tasks, a snippet, tests
//! tool           a command and a canvas page built with ui-kit, a setting, a key, tests
//! ```
//!
//! What it writes passes `cargo xtask validate`, packs, and -- for the two
//! kinds with code -- builds, tests and lints as it is. The templates are
//! the files under `xtask/templates/<kind>/`, embedded here; the
//! placeholders in them are
//!
//! ```text
//! {{id}}          acme.dusk
//! {{name}}        Dusk              what the store lists it as
//! {{folder}}      dusk              the id's name part; also what its commands start with
//! {{crate}}       acme-dusk         a code kind's crate: the publisher and the folder
//! {{repository}}  where it's hosted: this repository's for fenix, else the publisher's GitHub
//! ```
//!
//! **Cargo.lock.** The two code kinds ship a `Cargo.lock` of their own, in
//! `xtask/templates/<kind>/`, with `{{crate}}` in it: `cargo test --locked`
//! and `cargo xtask pack` (which builds `--locked`) work on what's written,
//! with no network and no `cargo generate-lockfile` first, and the versions
//! are the ones the template was tested with, the same on every machine.
//! The cost is that a lock can go stale -- `crates/lang-kit` or
//! `crates/ui-kit` gaining a dependency -- so the test
//! `the_code_templates_locks_still_hold` asks Cargo, `--locked`, whether
//! each still matches. When it says no: scaffold the kind into a scratch
//! folder, run `cargo generate-lockfile` there, and copy the lock back over
//! the template's, `name = "<crate>"` turned back into `name = "{{crate}}"`.

use std::path::{Path, PathBuf};

use crate::{external::RESERVED, valid_id};

/// What a kind is: its name, what it makes, whether it's code, and its
/// files -- a path and the text, both with placeholders.
pub struct Kind {
    pub name: &'static str,
    pub about: &'static str,
    pub code: bool,
    pub files: &'static [(&'static str, &'static str)],
}

pub const KINDS: &[Kind] = &[
    Kind {
        name: "theme",
        about: "a colour theme",
        code: false,
        files: &[
            ("extension.toml", include_str!("../templates/theme/extension.toml")),
            ("themes/{{folder}}.toml", include_str!("../templates/theme/themes/{{folder}}.toml")),
            ("README.md", include_str!("../templates/theme/README.md")),
            ("CHANGELOG.md", include_str!("../templates/theme/CHANGELOG.md")),
        ],
    },
    Kind {
        name: "keymap",
        about: "a keymap flavour over standard keys",
        code: false,
        files: &[
            ("extension.toml", include_str!("../templates/keymap/extension.toml")),
            ("keymaps/{{folder}}.toml", include_str!("../templates/keymap/keymaps/{{folder}}.toml")),
            ("README.md", include_str!("../templates/keymap/README.md")),
            ("CHANGELOG.md", include_str!("../templates/keymap/CHANGELOG.md")),
        ],
    },
    Kind {
        name: "snippets",
        about: "snippets",
        code: false,
        files: &[
            ("extension.toml", include_str!("../templates/snippets/extension.toml")),
            ("snippets/all/todo.snippet", include_str!("../templates/snippets/snippets/all/todo.snippet")),
            ("snippets/all/fixme.snippet", include_str!("../templates/snippets/snippets/all/fixme.snippet")),
            ("README.md", include_str!("../templates/snippets/README.md")),
            ("CHANGELOG.md", include_str!("../templates/snippets/CHANGELOG.md")),
        ],
    },
    Kind {
        name: "language-pack",
        about: "a language pack on lang-kit",
        code: true,
        files: &[
            ("extension.toml", include_str!("../templates/language-pack/extension.toml")),
            ("Cargo.toml", include_str!("../templates/language-pack/Cargo.toml")),
            ("Cargo.lock", include_str!("../templates/language-pack/Cargo.lock")),
            (".gitignore", include_str!("../templates/language-pack/.gitignore")),
            ("languages/{{folder}}.toml", include_str!("../templates/language-pack/languages/{{folder}}.toml")),
            ("snippets/example/hello.snippet", include_str!("../templates/language-pack/snippets/example/hello.snippet")),
            ("keymaps/{{folder}}.toml", include_str!("../templates/language-pack/keymaps/{{folder}}.toml")),
            ("src/lib.rs", include_str!("../templates/language-pack/src/lib.rs")),
            ("src/doctor.rs", include_str!("../templates/language-pack/src/doctor.rs")),
            ("src/tasks.rs", include_str!("../templates/language-pack/src/tasks.rs")),
            ("src/data.rs", include_str!("../templates/language-pack/src/data.rs")),
            ("README.md", include_str!("../templates/language-pack/README.md")),
            ("CHANGELOG.md", include_str!("../templates/language-pack/CHANGELOG.md")),
        ],
    },
    Kind {
        name: "tool",
        about: "a command and a page built with ui-kit",
        code: true,
        files: &[
            ("extension.toml", include_str!("../templates/tool/extension.toml")),
            ("Cargo.toml", include_str!("../templates/tool/Cargo.toml")),
            ("Cargo.lock", include_str!("../templates/tool/Cargo.lock")),
            (".gitignore", include_str!("../templates/tool/.gitignore")),
            ("keymaps/{{folder}}.toml", include_str!("../templates/tool/keymaps/{{folder}}.toml")),
            ("src/lib.rs", include_str!("../templates/tool/src/lib.rs")),
            ("src/page.rs", include_str!("../templates/tool/src/page.rs")),
            ("src/stats.rs", include_str!("../templates/tool/src/stats.rs")),
            ("README.md", include_str!("../templates/tool/README.md")),
            ("CHANGELOG.md", include_str!("../templates/tool/CHANGELOG.md")),
        ],
    },
];

const USAGE: &str = "cargo xtask new <kind> <name | publisher.name> [--name \"Display name\"]";

/// Where an extension of this repository is hosted.
const HOME: &str = "https://github.com/tpedneault/fenix-extensions";

/// What the command line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub kind: String,
    pub id: String,
    pub name: Option<String>,
    pub fenix: bool,
}

pub fn options(args: &[String]) -> Result<Options, String> {
    let (mut name, mut positional) = (None, Vec::new());
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "--name" => name = Some(inline.or_else(|| args.next().cloned()).ok_or("--name needs a value")?),
            f if f.starts_with("--") => return Err(format!("new: {f}? -- {USAGE}")),
            _ => positional.push(arg.clone()),
        }
    }
    match positional.as_slice() {
        [kind, id] => {
            // A bare name is this repository's own: fenix.<name>.
            let id = if id.contains('.') { id.clone() } else { format!("{RESERVED}.{id}") };
            let fenix = id.split_once('.').is_some_and(|(publisher, _)| publisher == RESERVED);
            Ok(Options { kind: kind.clone(), id, name, fenix })
        }
        _ => Err(format!("{USAGE}\nkinds: {}", kinds_said())),
    }
}

fn kinds_said() -> String {
    KINDS.iter().map(|k| format!("{} ({})", k.name, k.about)).collect::<Vec<_>>().join(", ")
}

/// What `{{...}}` stands for in the templates, for one extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub crate_name: String,
    pub repository: String,
}

/// The name a folder would be given in a few words: `word-count` is
/// "Word count".
pub fn display_name(folder: &str) -> String {
    let spaced = folder.replace('-', " ");
    let mut chars = spaced.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// What `--name` may say: it goes inside quotes in TOML and in Rust.
fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("--name is empty".into());
    }
    if name.contains(['"', '\\']) || name.chars().any(char::is_control) {
        return Err(format!("--name {name:?}: no quotes, backslashes or line breaks -- a few words, as the store lists it"));
    }
    Ok(())
}

/// The id's two parts, held to what a folder and a crate can be called.
fn split_id(id: &str) -> Result<(&str, &str), String> {
    if !valid_id(id) {
        return Err(format!("{id:?} isn't an id: publisher.name, in lower-case letters, digits, - and _ (acme.dusk)"));
    }
    let Some((publisher, name)) = id.split_once('.').filter(|(_, n)| !n.contains('.')) else {
        return Err(format!("{id:?}: an extension's id is publisher.name -- one dot, and the folder is the part after it"));
    };
    let letter = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !letter(publisher) || !letter(name) {
        return Err(format!("{id:?}: the publisher and the name each start with a letter (the name is a crate's, too)"));
    }
    if name.contains('_') || name.ends_with('-') {
        return Err(format!("{id:?}: the name is the folder's, and the release tag is made from it: lower-case letters, digits and -, not ending in - ({})", name.replace('_', "-").trim_end_matches('-')));
    }
    Ok((publisher, name))
}

/// Whether another extension in `root` already has `id`.
fn taken(root: &Path, id: &str) -> Option<PathBuf> {
    let dirs = std::fs::read_dir(root.join("extensions")).ok()?;
    dirs.flatten().map(|e| e.path()).find(|dir| {
        let text = std::fs::read_to_string(dir.join("extension.toml")).unwrap_or_default();
        toml::from_str::<toml::Table>(&text).ok().and_then(|t| t.get("extension")?.get("id")?.as_str().map(|i| i == id)).unwrap_or(false)
    })
}

/// Checks the command line against `root`, and works out what the
/// placeholders stand for.
///
/// **Publisher ids**, as CONTRIBUTING.md has them: `fenix.` is this
/// repository's, and every extension under `extensions/` that's meant to
/// stay here is `fenix.<folder>` -- what a bare name gives. Any other
/// publisher is a package built elsewhere, with a publisher id of its
/// own (your GitHub name, lower-cased): it's scaffolded here to be moved
/// out, and the next steps say so.
pub fn plan(root: &Path, o: &Options) -> Result<(&'static Kind, Plan), String> {
    let kind = KINDS.iter().find(|k| k.name == o.kind).ok_or_else(|| format!("{:?} isn't a kind of extension: {}", o.kind, kinds_said()))?;
    let (publisher, folder) = split_id(&o.id)?;
    let fenix = publisher == RESERVED;
    let name = match &o.name {
        Some(name) => {
            check_name(name)?;
            name.trim().to_string()
        }
        None => display_name(folder),
    };
    let dir = root.join("extensions").join(folder);
    if dir.exists() {
        return Err(format!("{} is already there: nothing was written", dir.display()));
    }
    if let Some(other) = taken(root, &o.id) {
        return Err(format!("{} already has the id {}", other.display(), o.id));
    }
    let repository = if fenix { HOME.to_string() } else { format!("https://github.com/{publisher}/{folder}") };
    Ok((kind, Plan { id: o.id.clone(), name, folder: folder.into(), crate_name: format!("{publisher}-{folder}"), repository }))
}

/// `text` with each `{{placeholder}}` in it replaced, in one pass: a
/// name that happens to say `{{id}}` stays as it was typed. A placeholder
/// that isn't one of the five is an error: the templates' own mistake.
pub fn render(text: &str, plan: &Plan) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let end = after.find("}}").ok_or_else(|| format!("a {{{{ with no }}}} after it: {}", after.chars().take(30).collect::<String>()))?;
        out.push_str(match &after[..end] {
            "id" => &plan.id,
            "name" => &plan.name,
            "folder" => &plan.folder,
            "crate" => &plan.crate_name,
            "repository" => &plan.repository,
            other => return Err(format!("no placeholder {{{{{other}}}}}")),
        });
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Every file of `kind` for `plan`: its path under the extension's folder
/// and its text.
pub fn render_all(kind: &Kind, plan: &Plan) -> Result<Vec<(String, String)>, String> {
    kind.files.iter().map(|(path, text)| Ok((render(path, plan)?, render(text, plan)?))).collect()
}

/// What was written.
#[derive(Debug)]
pub struct Written {
    pub dir: PathBuf,
    pub files: Vec<String>,
    pub plan: Plan,
    pub code: bool,
    /// The `TODO(you)` notes the files carry.
    pub todos: usize,
}

/// Writes the extension into `root/extensions/<folder>/`. Nothing is left
/// behind when it fails part-way.
pub fn scaffold(root: &Path, o: &Options) -> Result<Written, String> {
    let (kind, plan) = plan(root, o)?;
    let files = render_all(kind, &plan)?;
    let dir = root.join("extensions").join(&plan.folder);
    std::fs::create_dir_all(dir.parent().expect("extensions/")).map_err(|e| format!("{}: {e}", dir.display()))?;
    // `create_dir`, not `create_dir_all`: if it appeared since the check, stop.
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = || -> Result<(), String> {
        for (rel, text) in &files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("a folder")).map_err(|e| format!("{}: {e}", path.display()))?;
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    let todos = files.iter().map(|(_, text)| text.matches("TODO(you)").count()).sum();
    Ok(Written { dir, files: files.into_iter().map(|(rel, _)| rel).collect(), plan, code: kind.code, todos })
}

/// What to do next, as printed.
pub fn next_steps(w: &Written, fenix: bool) -> String {
    let folder = &w.plan.folder;
    let mut s = String::from("\nWhat's next:\n");
    let mut n = 0;
    let mut step = |text: String| {
        n += 1;
        s += &format!("  {n}. {text}\n");
    };
    if w.todos > 0 {
        step(format!("settle the {} `TODO(you)` note(s): search extensions/{folder} for them", w.todos));
    }
    step("cargo xtask validate".into());
    if w.code {
        step(format!("cargo test --manifest-path extensions/{folder}/Cargo.toml"));
        step(format!("cargo clippy --manifest-path extensions/{folder}/Cargo.toml -- -D warnings"));
    }
    step(format!("cargo xtask install extensions/{folder} --watch    (packs it and installs it into Fenix, again whenever you save)"));
    step(format!("cargo xtask pack extensions/{folder} dist    (the .fenixext, with its sha256)"));
    if fenix {
        s += "\nIt's for this repository: CONTRIBUTING.md has what a pull request carries (a screenshot, its permissions' reasons, the version and CHANGELOG).\n";
    } else {
        s += &format!(
            "\n{} doesn't start with fenix.: under extensions/, only fenix.<name> extensions are built and released by this repository (CONTRIBUTING.md, \"Two ways in\"). \
             To publish it from your own repository, move extensions/{folder} there and list its package in external.toml.{} \
             To have it looked after here instead, run this again with the name {folder} alone.\n",
            w.plan.id,
            if w.code { " Its Cargo.toml reaches the shared crates by a path (../../crates/...): outside this repository it names them as git dependencies pinned to a commit, as CONTRIBUTING.md's \"Shared crates\" says." } else { "" }
        );
    }
    s
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let w = scaffold(root, &o)?;
    println!("Wrote {} ({}, {} files):", w.dir.display(), o.kind, w.files.len());
    for file in &w.files {
        println!("  {file}");
    }
    print!("{}", next_steps(&w, o.fenix));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temp folder, gone when
    /// dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("xtask-new-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("extensions")).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    fn make(root: &Path, line: &str) -> Result<Written, String> {
        scaffold(root, &options(&args(line))?)
    }

    #[test]
    fn the_command_line_is_read() {
        let o = options(&args("theme acme.dusk")).unwrap();
        assert_eq!(o, Options { kind: "theme".into(), id: "acme.dusk".into(), name: None, fenix: false });
        let o = options(&["tool".into(), "--name".into(), "Word count".into(), "words".into()]).unwrap();
        assert_eq!((o.name.as_deref(), o.id.as_str(), o.fenix), (Some("Word count"), "fenix.words", true), "a bare name is this repository's");
        assert!(options(&args("tool fenix.words")).unwrap().fenix);
        assert_eq!(options(&["tool".into(), "acme.x".into(), "--name=Two words".into()]).unwrap().name.as_deref(), Some("Two words"));
        assert!(options(&args("theme")).unwrap_err().contains("kinds: theme"));
        assert!(options(&args("theme a.b c.d")).is_err());
        assert!(options(&args("theme a.b --colour")).unwrap_err().contains("--colour"));
        assert!(options(&args("theme a.b --name")).unwrap_err().contains("needs a value"));
        assert!(options(&args("theme a.b --fenix")).is_err(), "the flag is gone");
    }

    #[test]
    fn a_folder_is_named_in_a_few_words() {
        assert_eq!(display_name("word-count"), "Word count");
        assert_eq!(display_name("dusk"), "Dusk");
        assert_eq!(display_name("sample-language-pack"), "Sample language pack");
    }

    #[test]
    fn placeholders_are_replaced_in_one_pass() {
        let t = Temp::new("render");
        let (_, plan) = plan(&t.0, &options(&args("theme acme.dusk")).unwrap()).unwrap();
        assert_eq!(render("{{id}} {{name}} {{folder}} {{crate}}", &plan).unwrap(), "acme.dusk Dusk dusk acme-dusk");
        assert_eq!(plan.repository, "https://github.com/acme/dusk");
        let (_, named) = self::plan(&t.0, &options(&["theme".into(), "acme.dusk".into(), "--name".into(), "{{id}} {{".into()]).unwrap()).unwrap();
        assert_eq!(render("a {{name}} b", &named).unwrap(), "a {{id}} {{ b", "what's typed isn't expanded again");
        assert!(render("{{nope}}", &plan).unwrap_err().contains("nope"));
        assert!(render("{{id", &plan).is_err());
        assert_eq!(render("{ single } braces", &plan).unwrap(), "{ single } braces");
    }

    #[test]
    fn every_template_uses_only_placeholders_it_has_and_leaves_none_behind() {
        let t = Temp::new("all");
        for (i, kind) in KINDS.iter().enumerate() {
            let (_, plan) = plan(&t.0, &Options { kind: kind.name.into(), id: format!("acme.sample-{}", i), name: None, fenix: false }).unwrap();
            for (path, text) in render_all(kind, &plan).unwrap_or_else(|e| panic!("{}: {e}", kind.name)) {
                assert!(!path.contains("{{") && !text.contains("{{"), "{}: {path} has a placeholder left", kind.name);
                assert!(!text.is_empty(), "{}: {path} is empty", kind.name);
            }
        }
    }

    #[test]
    fn each_kind_scaffolds_and_passes_validate() {
        for kind in KINDS {
            let t = Temp::new(kind.name);
            let id = format!("acme.sample-{}", kind.name);
            let w = make(&t.0, &format!("{} {id}", kind.name)).unwrap();
            let dir = t.0.join("extensions").join(format!("sample-{}", kind.name));
            assert_eq!(w.dir, dir);
            assert!(dir.join("extension.toml").is_file() && dir.join("README.md").is_file() && dir.join("CHANGELOG.md").is_file(), "{}: manifest, README, CHANGELOG", kind.name);
            crate::validate(&t.0, None).unwrap_or_else(|e| panic!("{}: {e}", kind.name));
            let m = crate::manifest(&dir).unwrap();
            assert_eq!((m.extension.id.as_str(), m.extension.version.as_str()), (id.as_str(), "1.0.0"));
            assert_eq!(m.extension.runtime == "wasm", kind.code, "{}: only the code kinds run code", kind.name);
            assert_eq!(dir.join("Cargo.toml").is_file() && dir.join("Cargo.lock").is_file(), kind.code, "{}: a crate and its lock", kind.name);
            assert!(m.engine.api.is_some() == kind.code, "{}: an API only for code", kind.name);
        }
    }

    #[test]
    fn the_data_kinds_pack() {
        for kind in KINDS.iter().filter(|k| !k.code) {
            let t = Temp::new(&format!("pack-{}", kind.name));
            make(&t.0, &format!("{} acme.sample-{}", kind.name, kind.name)).unwrap();
            let dir = t.0.join("extensions").join(format!("sample-{}", kind.name));
            crate::pack(&dir, &t.0.join("dist")).unwrap();
            let file = t.0.join("dist").join(format!("acme.sample-{}-1.0.0.fenixext", kind.name));
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
            let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).collect();
            assert!(names.contains(&"extension.toml".to_string()), "{}: {names:?}", kind.name);
            assert!(names.len() >= 4, "{}: {names:?}", kind.name);
        }
    }

    #[test]
    fn what_each_kind_holds() {
        let kind = |name: &str| {
            let t = Temp::new(&format!("holds-{name}"));
            let w = make(&t.0, &format!("{name} acme.sample")).unwrap();
            let read = |rel: &str| std::fs::read_to_string(w.dir.join(rel)).unwrap_or_else(|e| panic!("{name}: {rel}: {e}"));
            (w.files.clone(), read("extension.toml"), read("README.md"))
        };
        let (files, manifest, readme) = kind("theme");
        assert!(files.contains(&"themes/sample.toml".to_string()));
        assert!(manifest.contains("categories = [\"themes\"]") && !manifest.contains("[permissions]"));
        assert!(readme.contains("## What it may do"));

        let (files, manifest, _) = kind("keymap");
        assert!(files.contains(&"keymaps/sample.toml".to_string()));
        assert!(manifest.contains("fenix = \">=1.5, <2\""), "[[keys]] is API 1.5's");

        let (files, manifest, _) = kind("snippets");
        assert!(files.iter().filter(|f| f.ends_with(".snippet")).count() >= 2);
        assert!(manifest.contains("categories = [\"snippets\"]"));

        let (files, manifest, readme) = kind("language-pack");
        for f in ["languages/sample.toml", "snippets/example/hello.snippet", "src/doctor.rs", "src/tasks.rs", "src/data.rs", "Cargo.lock"] {
            assert!(files.contains(&f.to_string()), "{f}");
        }
        let permissions = toml::from_str::<toml::Table>(&manifest).unwrap()["permissions"].as_table().unwrap().clone();
        assert_eq!(permissions.keys().collect::<Vec<_>>(), ["process"], "the narrowest: a program, and nothing else");
        assert_eq!(permissions["process"].as_array().unwrap().len(), 1, "one placeholder program");
        assert!(manifest.contains("TODO(you)") && readme.contains("TODO(you)"));
        // The kind's block is commented out, and uncommented it's valid.
        assert!(manifest.contains("# [[contributes.project_kinds]]") && !manifest.contains("\n[[contributes.project_kinds]]"));
        let uncommented: String = manifest.lines().map(|l| l.strip_prefix("# ").filter(|_| l.starts_with("# [[contributes.project_kinds]]") || l.starts_with("# id =") || l.starts_with("# name =") || l.starts_with("# tag =") || l.starts_with("# color =") || l.starts_with("# markers =") || l.starts_with("# main =")).unwrap_or(l)).collect::<Vec<_>>().join("\n");
        let t = Temp::new("kinds-block");
        std::fs::create_dir_all(t.0.join("x")).unwrap();
        std::fs::write(t.0.join("x/extension.toml"), uncommented).unwrap();
        let pack = crate::kinds::check_pack(&t.0.join("x")).unwrap();
        assert_eq!((pack.kinds.len(), pack.kinds[0].id.as_str()), (1, "sample"));
        assert!(readme.contains("installs nothing"));

        let (files, manifest, readme) = kind("tool");
        for f in ["keymaps/sample.toml", "src/page.rs", "src/stats.rs", "Cargo.lock"] {
            assert!(files.contains(&f.to_string()), "{f}");
        }
        assert!(manifest.contains("[[contributes.settings]]") && manifest.contains("sample.words_per_minute") && !manifest.contains("[permissions]\n"));
        assert!(readme.contains("What it may do") && readme.contains("## Building"));
    }

    #[test]
    fn a_pack_leaves_its_placeholders_marked_and_installs_nothing() {
        let t = Temp::new("pack-marks");
        let w = make(&t.0, "language-pack acme.sample").unwrap();
        assert!(w.todos >= 8, "{} notes", w.todos);
        for f in ["languages/sample.toml", "src/doctor.rs", "src/tasks.rs", "src/lib.rs"] {
            assert!(std::fs::read_to_string(w.dir.join(f)).unwrap().contains("TODO(you)"), "{f}");
        }
        let all: String = w.files.iter().map(|f| std::fs::read_to_string(w.dir.join(f)).unwrap()).collect();
        assert!(!all.contains("releases/download") && !all.contains("Install::new("), "no real downloads, no installer");
    }

    #[test]
    fn a_name_of_your_own_is_used() {
        let t = Temp::new("named");
        let w = scaffold(&t.0, &Options { kind: "tool".into(), id: "acme.words".into(), name: Some("Word counter".into()), fenix: false }).unwrap();
        assert_eq!(w.plan.name, "Word counter");
        assert!(std::fs::read_to_string(w.dir.join("extension.toml")).unwrap().contains("name = \"Word counter\""));
        assert!(std::fs::read_to_string(w.dir.join("src/page.rs")).unwrap().contains("\"Word counter\""));
        crate::validate(&t.0, None).unwrap();
    }

    #[test]
    fn a_bare_name_is_this_repositorys_and_a_publisher_is_someone_elses() {
        let t = Temp::new("fenix");
        let other = make(&t.0, "theme acme.dawn").unwrap();
        assert_eq!(other.plan.repository, "https://github.com/acme/dawn");
        assert!(next_steps(&other, false).contains("external.toml"));
        assert_eq!(make(&t.0, "theme fenix.noon").unwrap().plan.repository, HOME);

        let w = make(&t.0, "theme dusk").unwrap();
        assert_eq!(w.plan.repository, HOME);
        assert!(next_steps(&w, true).contains("for this repository"));
        let manifest = std::fs::read_to_string(w.dir.join("extension.toml")).unwrap();
        assert!(manifest.contains("id = \"fenix.dusk\"") && manifest.contains(&format!("repository = \"{HOME}\"")));
        crate::validate(&t.0, None).unwrap();
    }

    #[test]
    fn what_cant_be_made_is_refused_and_nothing_is_written() {
        let t = Temp::new("refuse");
        let ext = t.0.join("extensions");
        let empty = |t: &Temp| std::fs::read_dir(t.0.join("extensions")).unwrap().count() == 0;
        for (line, says) in [
            ("tool Acme.dusk", "isn't an id"),
            ("tool acme.", "isn't an id"),
            ("tool acme.my$tool", "isn't an id"),
            ("tool acme.a.b", "one dot"),
            ("tool acme.my_tool", "lower-case letters, digits and -"),
            ("tool acme.dusk-", "not ending in -"),
            ("tool 9acme.dusk", "start with a letter"),
            ("tool acme.1dusk", "start with a letter"),
            ("widget acme.dusk", "isn't a kind"),
            ("tool Dusk", "isn't an id"),
        ] {
            let err = make(&t.0, line).unwrap_err();
            assert!(err.contains(says), "{line}: {err}");
            assert!(empty(&t), "{line} left something behind");
        }
        for bad in ["a\"b", "a\\b", "a\nb", "  "] {
            let err = scaffold(&t.0, &Options { kind: "theme".into(), id: "acme.dusk".into(), name: Some(bad.into()), fenix: false }).unwrap_err();
            assert!(err.contains("--name"), "{bad:?}: {err}");
        }
        assert!(empty(&t));

        // A folder that's there, and an id somebody has.
        std::fs::create_dir_all(ext.join("dusk")).unwrap();
        std::fs::write(ext.join("dusk/keep.txt"), "mine").unwrap();
        let err = make(&t.0, "theme acme.dusk").unwrap_err();
        assert!(err.contains("already there"), "{err}");
        assert_eq!(std::fs::read_to_string(ext.join("dusk/keep.txt")).unwrap(), "mine", "what was there is untouched");
        make(&t.0, "theme acme.first").unwrap();
        std::fs::rename(ext.join("first"), ext.join("elsewhere")).unwrap();
        let err = make(&t.0, "theme acme.first").unwrap_err();
        assert!(err.contains("already has the id acme.first"), "{err}");
    }

    #[test]
    fn what_comes_next_says_what_the_id_means() {
        let t = Temp::new("next");
        let w = make(&t.0, "tool acme.words").unwrap();
        let said = next_steps(&w, false);
        for s in ["cargo xtask validate", "cargo test --manifest-path extensions/words/Cargo.toml", "clippy", "cargo xtask install extensions/words --watch", "cargo xtask pack extensions/words dist", "external.toml", "git dependencies", "TODO(you)"] {
            assert!(said.contains(s), "{s}: {said}");
        }
        let w = make(&t.0, "theme acme.dusk").unwrap();
        let said = next_steps(&w, false);
        assert!(!said.contains("cargo test") && !said.contains("git dependencies"), "{said}");
    }

    /// `cargo metadata --locked`, in a scaffold of `kind` that reaches the
    /// real `crates/` by a link: whether the template's lock still holds.
    /// `None` when Cargo can't say without the network.
    #[cfg(unix)]
    fn lock_holds(kind: &Kind) -> Option<Result<(), String>> {
        let t = Temp::new(&format!("lock-{}", kind.name));
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        std::os::unix::fs::symlink(repo.join("crates"), t.0.join("crates")).unwrap();
        let w = make(&t.0, &format!("{} acme.sample", kind.name)).unwrap();
        let mut c = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        c.args(["metadata", "--locked", "--offline", "--format-version", "1"]).current_dir(&w.dir);
        for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("CARGO_") && k != "CARGO_HOME") {
            c.env_remove(key);
        }
        let out = c.output().expect("cargo");
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        if out.status.success() {
            let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let names: Vec<&str> = meta["packages"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
            assert!(names.contains(&"acme-sample"), "the crate is named for the publisher and the folder: {names:?}");
            return Some(Ok(()));
        }
        // `--offline` with nothing fetched is no verdict on the lock.
        (said.contains("lock file") || said.contains("--locked")).then_some(Err(said))
    }

    #[cfg(unix)]
    #[test]
    fn the_code_templates_locks_still_hold() {
        for kind in KINDS.iter().filter(|k| k.code) {
            if let Some(Err(said)) = lock_holds(kind) {
                panic!("{}'s Cargo.lock no longer matches its Cargo.toml and the shared crates' (see new.rs on refreshing it):\n{said}", kind.name);
            }
        }
    }
}
