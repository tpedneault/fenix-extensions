//! What the pack knows of the project, from its files: which `.typ` file is
//! the document -- the one `typst compile` is given and whose PDF is
//! previewed -- and where its PDF goes.
//!
//! The document is, in this order: the `typst.main` setting; `main.typ` at
//! the top; `src/main.typ`; a `typst.toml`'s `entrypoint` (a package's own
//! file, usually `lib.typ`); the one `.typ` file at the top when there's
//! only one. A project that none of these names has no document, and the
//! commands compile the file you're in.

use std::path::Path;

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The document, in the project: `main.typ`, `src/main.typ`.
    pub main: Option<String>,
    /// Whether it holds anything of Typst's: a `typst.toml` or a `.typ` file
    /// at its top.
    pub typst: bool,
}

/// `entrypoint = "lib.typ"` in a `typst.toml`'s `[package]` table.
pub fn entrypoint(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(table) = line.strip_prefix('[') {
            in_package = table.trim_end_matches(']').trim() == "package";
        } else if in_package {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "entrypoint" {
                    let value = value.split('#').next().unwrap_or("").trim();
                    let value = value.trim_matches(|c| c == '"' || c == '\'');
                    return (!value.is_empty()).then(|| value.to_string());
                }
            }
        }
    }
    None
}

fn typ_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "typ"))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

impl Project {
    /// The project in `root`, with the document `setting` names, if any.
    pub fn read(root: &str, setting: Option<&str>) -> Project {
        let root = Path::new(root);
        let toml = std::fs::read_to_string(root.join("typst.toml")).ok();
        let top = typ_files(root);
        let exists = |p: &str| root.join(p).is_file();
        let named = setting.map(|s| s.trim().replace('\\', "/")).map(|s| s.trim_start_matches("./").to_string()).filter(|s| !s.is_empty() && !s.starts_with('/') && !s.contains(".."));
        let main = named
            .filter(|s| exists(s))
            .or_else(|| ["main.typ", "src/main.typ"].into_iter().find(|p| exists(p)).map(str::to_string))
            .or_else(|| toml.as_deref().and_then(entrypoint).filter(|e| exists(e) && !e.starts_with('/') && !e.contains("..")))
            .or_else(|| (top.len() == 1).then(|| top[0].clone()));
        Project { main, typst: toml.is_some() || !top.is_empty() }
    }

    /// The PDF the document compiles to, beside it: `main.pdf`.
    pub fn pdf(main: &str) -> String {
        match main.strip_suffix(".typ") {
            Some(stem) => format!("{stem}.pdf"),
            None => format!("{main}.pdf"),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test, its own: the test's name and the process.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-typst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn touch(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    #[test]
    fn the_document_is_the_setting_then_main_then_the_entrypoint_then_the_only_file() {
        let dir = temp("document");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(Project::read(&root, None), Project { main: None, typst: false });

        touch(&dir, "notes.typ", "");
        assert_eq!(Project::read(&root, None), Project { main: Some("notes.typ".into()), typst: true }, "the only one");
        touch(&dir, "other.typ", "");
        assert_eq!(Project::read(&root, None).main, None, "two, neither main: not guessed");

        touch(&dir, "typst.toml", "[package]\nname = \"orbit\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\" # the package\n");
        assert_eq!(Project::read(&root, None).main, None, "lib.typ isn't there");
        touch(&dir, "lib.typ", "");
        assert_eq!(Project::read(&root, None).main.as_deref(), Some("lib.typ"));
        touch(&dir, "src/main.typ", "");
        assert_eq!(Project::read(&root, None).main.as_deref(), Some("src/main.typ"));
        touch(&dir, "main.typ", "");
        assert_eq!(Project::read(&root, None).main.as_deref(), Some("main.typ"));

        assert_eq!(Project::read(&root, Some("./notes.typ")).main.as_deref(), Some("notes.typ"), "the setting wins");
        assert_eq!(Project::read(&root, Some("gone.typ")).main.as_deref(), Some("main.typ"), "one that isn't there is ignored");
        assert_eq!(Project::read(&root, Some("../x.typ")).main.as_deref(), Some("main.typ"), "and one that leaves the project");
        assert_eq!(Project::read(&root, Some("/etc/passwd")).main.as_deref(), Some("main.typ"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_manifest_alone_makes_it_a_typst_project() {
        let dir = temp("manifest-only");
        touch(&dir, "typst.toml", "[package]\nentrypoint = \"../up.typ\"\n");
        let p = Project::read(&dir.to_string_lossy(), None);
        assert_eq!(p, Project { main: None, typst: true });
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_entrypoint_is_the_package_tables_only() {
        assert_eq!(entrypoint("[package]\nentrypoint = \"lib.typ\"\n").as_deref(), Some("lib.typ"));
        assert_eq!(entrypoint("[tool.x]\nentrypoint = \"a.typ\"\n[package]\nname = \"p\"\n"), None);
        assert_eq!(entrypoint("[package]\nentrypoint = ''\n"), None);
        assert_eq!(Project::pdf("src/main.typ"), "src/main.pdf");
        assert_eq!(Project::pdf("odd"), "odd.pdf");
    }
}
