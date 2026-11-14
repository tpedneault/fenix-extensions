//! What the pack knows of the project, from its files: which `.tex` file is
//! the document -- the one the builder is given -- and what asks for a
//! builder of its own (a `.latexmkrc`, a `Tectonic.toml`).
//!
//! The document is, in this order: the `.tex` file you're in, when it says
//! `% !TEX root = ../main.tex` (the root it names), or has a `\documentclass`
//! itself; the `latex.main` setting; `main.tex` at the top; the one `.tex`
//! file at the top with a `\documentclass`. A project none of these names has
//! no document: the commands say so.

use std::path::Path;

/// What a file's magic comments say (`% !TEX root = ../main.tex`,
/// `% !TEX program = xelatex`): TeXShop's, TeXstudio's and TeXworks'.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Magic {
    pub root: Option<String>,
    pub program: Option<String>,
}

/// The magic comments in the first 50 lines of `text`.
pub fn magic(text: &str) -> Magic {
    let mut m = Magic::default();
    for line in text.lines().take(50) {
        let Some(rest) = line.trim_start().strip_prefix('%') else { continue };
        let rest = rest.trim_start();
        let Some(head) = rest.get(..5) else { continue };
        if !head.eq_ignore_ascii_case("!TEX ") {
            continue;
        }
        let Some((key, value)) = rest[5..].split_once('=') else { continue };
        let value = value.trim().trim_matches('"').to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "root" => m.root = m.root.take().or(Some(value)),
            "program" | "ts-program" => m.program = m.program.take().or(Some(value.to_ascii_lowercase())),
            _ => {}
        }
    }
    m
}

/// `rel`, a path from the folder `from` (in the project), made one from the
/// project's top: `..` and `.` folded in. `None` when it leaves the
/// project, or is absolute.
pub fn join(from: &str, rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    if rel.starts_with('/') || rel.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    let mut parts: Vec<&str> = from.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The folder part of a path in the project: `chapters` for
/// `chapters/a.tex`, empty for a file at the top.
pub fn folder(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The document, in the project: `main.tex`, `thesis/main.tex`.
    pub main: Option<String>,
    /// A `.latexmkrc` or `latexmkrc` at the top: latexmk decides what it
    /// builds with.
    pub latexmkrc: bool,
    /// A `Tectonic.toml` at the top, and the output it names (`default`).
    pub tectonic: Option<String>,
    /// Whether it holds anything of LaTeX's: those, or a `.tex` file at the top.
    pub latex: bool,
}

fn tex_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "tex"))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The first `[[output]]`'s name in a `Tectonic.toml`.
pub fn tectonic_output(manifest: &str) -> String {
    let mut in_output = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_output = line == "[[output]]";
        } else if in_output {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "name" {
                    return v.split('#').next().unwrap_or("").trim().trim_matches('"').to_string();
                }
            }
        }
    }
    "default".into()
}

/// Whether `text` has a `\documentclass` that isn't commented out.
pub fn is_document(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("\\documentclass"))
}

impl Project {
    /// The project in `root`, with the document `setting` names, if any.
    pub fn read(root: &str, setting: Option<&str>) -> Project {
        let root = Path::new(root);
        let exists = |p: &str| root.join(p).is_file();
        let top = tex_files(root);
        let latexmkrc = exists(".latexmkrc") || exists("latexmkrc");
        let tectonic = std::fs::read_to_string(root.join("Tectonic.toml")).ok().map(|t| tectonic_output(&t));
        let named = setting.map(|s| s.trim().replace('\\', "/")).map(|s| s.trim_start_matches("./").to_string()).filter(|s| !s.is_empty() && !s.starts_with('/') && !s.contains(".."));
        let documents: Vec<&String> = top.iter().filter(|f| std::fs::read_to_string(root.join(f)).is_ok_and(|t| is_document(&t))).collect();
        let main = named
            .filter(|s| exists(s))
            .or_else(|| exists("main.tex").then(|| "main.tex".to_string()))
            .or_else(|| (documents.len() == 1).then(|| documents[0].clone()));
        let latex = latexmkrc || tectonic.is_some() || !top.is_empty();
        Project { main, latexmkrc, tectonic, latex }
    }

    /// The document for the file `file` (in the project) with `text`: the
    /// root it names, one of its own, else the project's.
    pub fn document(&self, file: Option<(&str, &str)>) -> Option<String> {
        if let Some((path, text)) = file {
            if let Some(root) = magic(text).root.and_then(|r| join(folder(path), &r)) {
                return Some(root);
            }
            if is_document(text) {
                return Some(path.to_string());
            }
        }
        self.main.clone()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test, its own: the test's name and the process.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-latex-{name}-{}", std::process::id()));
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
    fn magic_comments_name_the_root_and_the_engine() {
        let m = magic("% !TEX root = ../main.tex\n% !TeX program = XeLaTeX\n\\section{A}\n");
        assert_eq!(m, Magic { root: Some("../main.tex".into()), program: Some("xelatex".into()) });
        assert_eq!(magic("%!TEX TS-program = lualatex\n").program.as_deref(), Some("lualatex"), "TeXstudio's");
        assert_eq!(magic("% !TEX encoding = UTF-8\n% a comment\n"), Magic::default());
        assert_eq!(magic("% !TEX root =\n").root, None);
        let late: String = "\n".repeat(60) + "% !TEX root = x.tex\n";
        assert_eq!(magic(&late).root, None, "only the first lines are read");
        assert_eq!(magic("% !TEX root = a.tex\n% !TEX root = b.tex\n").root.as_deref(), Some("a.tex"), "the first");
    }

    #[test]
    fn a_root_is_joined_to_the_folder_of_the_file_that_names_it() {
        assert_eq!(join("chapters", "../main.tex").as_deref(), Some("main.tex"));
        assert_eq!(join("", "main.tex").as_deref(), Some("main.tex"));
        assert_eq!(join("a/b", "./../c/main.tex").as_deref(), Some("a/c/main.tex"));
        assert_eq!(join("", "../x.tex"), None, "it leaves the project");
        assert_eq!(join("a", "/etc/x.tex"), None);
        assert_eq!(join("a", "C:\\x.tex"), None);
        assert_eq!((folder("chapters/a.tex"), folder("a.tex")), ("chapters", ""));
    }

    #[test]
    fn the_document_is_the_setting_then_main_then_the_only_document() {
        let dir = temp("document");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(Project::read(&root, None), Project::default());

        touch(&dir, "notes.tex", "\\documentclass{article}\n");
        let p = Project::read(&root, None);
        assert_eq!((p.main.as_deref(), p.latex), (Some("notes.tex"), true), "the only one with a \\documentclass");
        touch(&dir, "other.tex", "\\documentclass{book}\n");
        assert_eq!(Project::read(&root, None).main, None, "two documents, neither main.tex: not guessed");
        touch(&dir, "chap.tex", "\\chapter{A}\n");
        touch(&dir, "main.tex", "% \\documentclass{x}\n");
        assert_eq!(Project::read(&root, None).main.as_deref(), Some("main.tex"));
        assert_eq!(Project::read(&root, Some("./notes.tex")).main.as_deref(), Some("notes.tex"), "the setting wins");
        assert_eq!(Project::read(&root, Some("gone.tex")).main.as_deref(), Some("main.tex"), "one that isn't there is ignored");
        assert_eq!(Project::read(&root, Some("../x.tex")).main.as_deref(), Some("main.tex"), "and one that leaves the project");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_latexmkrc_and_a_tectonic_toml_are_seen() {
        let dir = temp("markers");
        let root = dir.to_string_lossy().into_owned();
        touch(&dir, ".latexmkrc", "$pdf_mode = 5;\n");
        let p = Project::read(&root, None);
        assert_eq!((p.latexmkrc, p.latex, p.tectonic.clone()), (true, true, None));
        std::fs::remove_file(dir.join(".latexmkrc")).unwrap();
        touch(&dir, "latexmkrc", "");
        assert!(Project::read(&root, None).latexmkrc, "the one without the dot too");
        touch(&dir, "Tectonic.toml", "[doc]\nname = \"thesis\"\n\n[[output]]\nname = \"paper\"\ntype = \"pdf\"\n");
        assert_eq!(Project::read(&root, None).tectonic.as_deref(), Some("paper"));
        assert_eq!(tectonic_output("[doc]\nname = \"x\"\n"), "default");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_you_are_in_names_its_document_or_is_one() {
        let p = Project { main: Some("main.tex".into()), ..Default::default() };
        assert_eq!(p.document(Some(("chapters/a.tex", "% !TEX root = ../paper.tex\n\\section{A}"))).as_deref(), Some("paper.tex"));
        assert_eq!(p.document(Some(("talk/slides.tex", "\\documentclass{beamer}\n"))).as_deref(), Some("talk/slides.tex"), "a document of its own");
        assert_eq!(p.document(Some(("chapters/a.tex", "\\section{A}"))).as_deref(), Some("main.tex"), "else the project's");
        assert_eq!(p.document(None).as_deref(), Some("main.tex"));
        assert_eq!(Project::default().document(Some(("a.tex", "\\section{A}"))), None);
        assert_eq!(p.document(Some(("c/a.tex", "% !TEX root = ../../x.tex\n\\documentclass{x}"))).as_deref(), Some("c/a.tex"), "a root outside the project is ignored");
    }
}
