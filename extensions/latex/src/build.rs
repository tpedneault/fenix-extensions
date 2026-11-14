//! What builds the document: what the user has, and the command line for it.
//!
//! In this order, the first that applies:
//!
//! 1. A `Tectonic.toml` at the top, and Tectonic: `tectonic -X build`.
//! 2. A `.latexmkrc`, and latexmk: it decides the engine and everything else.
//! 3. A magic comment in the document, `% !TEX program = xelatex`: latexmk with
//!    that engine, else the engine itself, when it's found.
//! 4. latexmk (with pdflatex).
//! 5. Tectonic (`tectonic -X compile`).
//! 6. An engine by itself: pdflatex, then xelatex, then lualatex. It runs
//!    once: it doesn't run BibTeX or again for the references, so the first
//!    build says "undefined references" until you build again.
//!
//! Every command is given `-interaction=nonstopmode`, so TeX never stops to
//! ask what to do with an error, and `-file-line-error`, so an error says its
//! file and line; each asks for SyncTeX data.

use fenix_extension_api::Command;

use crate::project::{folder, Project};
use crate::tools::Tools;

/// An engine latexmk or the pack runs by name.
pub const ENGINES: &[&str] = &["pdflatex", "xelatex", "lualatex"];

/// What builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Builder {
    /// latexmk, by its path: the engine to ask for, or none (a `.latexmkrc` decides).
    Latexmk { path: String, engine: Option<String> },
    /// Tectonic's `compile`, on one file.
    Tectonic { path: String },
    /// Tectonic's `build`, in a project with a `Tectonic.toml` and an output of this name.
    TectonicProject { path: String, output: String },
    /// An engine by itself: its name and path.
    Engine { name: String, path: String },
}

/// What builds `project`'s document `main`, whose first lines say `program`
/// (a magic comment), with `tools`: none when nothing here builds.
pub fn choose(tools: &Tools, project: &Project, program: Option<&str>) -> Option<Builder> {
    if let (Some(output), Some(path)) = (&project.tectonic, &tools.tectonic) {
        return Some(Builder::TectonicProject { path: path.clone(), output: output.clone() });
    }
    if let (true, Some(path)) = (project.latexmkrc, &tools.latexmk) {
        return Some(Builder::Latexmk { path: path.clone(), engine: None });
    }
    let program = program.filter(|p| ENGINES.contains(p));
    if let Some(name) = program {
        if let Some(path) = &tools.latexmk {
            return Some(Builder::Latexmk { path: path.clone(), engine: Some(name.to_string()) });
        }
        if let Some(path) = tools.engine(name) {
            return Some(Builder::Engine { name: name.to_string(), path: path.to_string() });
        }
    }
    if let Some(path) = &tools.latexmk {
        return Some(Builder::Latexmk { path: path.clone(), engine: Some("pdflatex".into()) });
    }
    if let Some(path) = &tools.tectonic {
        return Some(Builder::Tectonic { path: path.clone() });
    }
    ENGINES.iter().find_map(|name| tools.engine(name).map(|path| Builder::Engine { name: name.to_string(), path: path.to_string() }))
}

impl Builder {
    /// What it's called to the user: `latexmk (pdflatex)`, `Tectonic`.
    pub fn describe(&self) -> String {
        match self {
            Builder::Latexmk { engine: Some(e), .. } => format!("latexmk with {e}"),
            Builder::Latexmk { engine: None, .. } => "latexmk, as .latexmkrc says".into(),
            Builder::Tectonic { .. } => "Tectonic".into(),
            Builder::TectonicProject { .. } => "Tectonic, as Tectonic.toml says".into(),
            Builder::Engine { name, .. } => name.clone(),
        }
    }

    /// The command that builds `main` (in the project). Run in the
    /// project's folder. `continuous`: `latexmk -pvc`, which builds again
    /// when a file changes (only latexmk does).
    pub fn command(&self, main: &str, continuous: bool) -> Option<Command> {
        let dir = folder(main);
        let flags = ["-interaction=nonstopmode", "-file-line-error", "-synctex=1"];
        Some(match self {
            Builder::Latexmk { path, engine } => {
                let mut c = Command::new(path);
                if let Some(e) = engine {
                    c = c.arg(match e.as_str() {
                        "xelatex" => "-xelatex",
                        "lualatex" => "-lualatex",
                        _ => "-pdf",
                    });
                }
                c = c.args(flags);
                // -cd: in the document's folder, so its output is beside it.
                if !dir.is_empty() {
                    c = c.arg("-cd");
                }
                if continuous {
                    c = c.args(["-pvc", "-view=none"]);
                }
                c.arg(main)
            }
            _ if continuous => return None,
            Builder::Tectonic { path } => Command::new(path).args(["-X", "compile", "--keep-logs", "--synctex"]).arg(main),
            Builder::TectonicProject { path, .. } => Command::new(path).args(["-X", "build", "--keep-logs"]),
            Builder::Engine { path, .. } => {
                let mut c = Command::new(path).args(flags);
                if !dir.is_empty() {
                    c = c.arg(format!("-output-directory={dir}"));
                }
                c.arg(main)
            }
        })
    }

    /// The command that cleans what it built: latexmk's `-c`, or with `all`
    /// `-C`, which takes the PDF too. Only latexmk knows what it left.
    pub fn clean(&self, main: &str, all: bool) -> Option<Command> {
        match self {
            Builder::Latexmk { path, .. } => {
                let mut c = Command::new(path).arg(if all { "-C" } else { "-c" });
                if !folder(main).is_empty() {
                    c = c.arg("-cd");
                }
                Some(c.arg(main))
            }
            _ => None,
        }
    }

    /// The PDF it writes, in the project: beside `main`, or in `build/<output>`.
    pub fn pdf(&self, main: &str) -> String {
        match self {
            Builder::TectonicProject { output, .. } => format!("build/{output}/{output}.pdf"),
            _ => with_extension(main, "pdf"),
        }
    }

    /// The log it writes, in the project: beside the PDF.
    pub fn log(&self, main: &str) -> String {
        with_extension(&self.pdf(main), "log")
    }

    /// The folder file names in its log are relative to: where it ran. latexmk's
    /// `-cd` and Tectonic run in the document's folder; the others in the project's.
    pub fn runs_in(&self, main: &str) -> String {
        match self {
            Builder::Latexmk { .. } | Builder::Tectonic { .. } => folder(main).to_string(),
            _ => String::new(),
        }
    }
}

/// `path` with `.tex` (or any extension) replaced by `ext`.
pub fn with_extension(path: &str, ext: &str) -> String {
    let (dir, name) = path.rsplit_once('/').map(|(d, n)| (format!("{d}/"), n)).unwrap_or((String::new(), path));
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    format!("{dir}{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::line;

    fn tools(names: &[&str]) -> Tools {
        let mut t = Tools::default();
        for n in names {
            *t.slot(n).unwrap() = Some(format!("/t/{n}"));
        }
        t
    }

    fn doc() -> Project {
        Project { main: Some("main.tex".into()), latex: true, ..Default::default() }
    }

    fn cmd(b: &Builder, main: &str) -> String {
        line(&b.command(main, false).unwrap())
    }

    #[test]
    fn what_the_user_has_decides_in_the_order_the_module_says() {
        // latexmk beats Tectonic beats a lone engine.
        let all = tools(&["latexmk", "tectonic", "pdflatex", "xelatex"]);
        let b = choose(&all, &doc(), None).unwrap();
        assert_eq!(b, Builder::Latexmk { path: "/t/latexmk".into(), engine: Some("pdflatex".into()) });
        assert_eq!(cmd(&b, "main.tex"), "/t/latexmk -pdf -interaction=nonstopmode -file-line-error -synctex=1 main.tex");
        let b = choose(&tools(&["tectonic", "pdflatex"]), &doc(), None).unwrap();
        assert_eq!(b, Builder::Tectonic { path: "/t/tectonic".into() });
        assert_eq!(cmd(&b, "main.tex"), "/t/tectonic -X compile --keep-logs --synctex main.tex");
        let b = choose(&tools(&["xelatex", "lualatex"]), &doc(), None).unwrap();
        assert_eq!(cmd(&b, "main.tex"), "/t/xelatex -interaction=nonstopmode -file-line-error -synctex=1 main.tex", "pdflatex first, but it isn't here");
        assert_eq!(choose(&tools(&["texlab", "tex-fmt", "chktex"]), &doc(), None), None, "none of those builds");
        assert_eq!(choose(&Tools::default(), &doc(), None), None);
    }

    #[test]
    fn a_latexmkrc_a_tectonic_toml_and_a_magic_comment_each_decide() {
        let rc = Project { latexmkrc: true, ..doc() };
        let b = choose(&tools(&["latexmk", "tectonic"]), &rc, Some("xelatex")).unwrap();
        assert_eq!(b, Builder::Latexmk { path: "/t/latexmk".into(), engine: None }, ".latexmkrc beats the magic comment: it says its own engine");
        assert_eq!(cmd(&b, "main.tex"), "/t/latexmk -interaction=nonstopmode -file-line-error -synctex=1 main.tex", "no engine flag: the rc's");
        assert_eq!(choose(&tools(&["tectonic"]), &rc, None).unwrap(), Builder::Tectonic { path: "/t/tectonic".into() }, "no latexmk: not the rc's to use");

        let toml = Project { tectonic: Some("paper".into()), ..doc() };
        let b = choose(&tools(&["latexmk", "tectonic"]), &toml, None).unwrap();
        assert_eq!(cmd(&b, "main.tex"), "/t/tectonic -X build --keep-logs");
        assert_eq!((b.pdf("main.tex").as_str(), b.log("main.tex").as_str()), ("build/paper/paper.pdf", "build/paper/paper.log"));
        assert_eq!(choose(&tools(&["latexmk"]), &toml, None).unwrap(), Builder::Latexmk { path: "/t/latexmk".into(), engine: Some("pdflatex".into()) }, "no Tectonic: the Tectonic.toml is ignored");

        let b = choose(&tools(&["latexmk", "xelatex"]), &doc(), Some("xelatex")).unwrap();
        assert_eq!(cmd(&b, "main.tex"), "/t/latexmk -xelatex -interaction=nonstopmode -file-line-error -synctex=1 main.tex");
        let b = choose(&tools(&["lualatex", "pdflatex"]), &doc(), Some("lualatex")).unwrap();
        assert_eq!(b, Builder::Engine { name: "lualatex".into(), path: "/t/lualatex".into() }, "without latexmk, the engine itself");
        let b = choose(&tools(&["pdflatex"]), &doc(), Some("xelatex")).unwrap();
        assert_eq!(b, Builder::Engine { name: "pdflatex".into(), path: "/t/pdflatex".into() }, "the one it asked for isn't here");
        let b = choose(&tools(&["latexmk"]), &doc(), Some("latex")).unwrap();
        assert!(matches!(b, Builder::Latexmk { engine: Some(ref e), .. } if e == "pdflatex"), "an engine the pack doesn't know is ignored");
    }

    #[test]
    fn a_document_in_a_folder_builds_there() {
        let latexmk = Builder::Latexmk { path: "latexmk".into(), engine: Some("lualatex".into()) };
        assert_eq!(cmd(&latexmk, "thesis/main.tex"), "latexmk -lualatex -interaction=nonstopmode -file-line-error -synctex=1 -cd thesis/main.tex");
        assert_eq!(latexmk.runs_in("thesis/main.tex"), "thesis");
        let engine = Builder::Engine { name: "pdflatex".into(), path: "pdflatex".into() };
        assert_eq!(cmd(&engine, "thesis/main.tex"), "pdflatex -interaction=nonstopmode -file-line-error -synctex=1 -output-directory=thesis thesis/main.tex");
        assert_eq!(engine.runs_in("thesis/main.tex"), "", "it runs in the project's folder");
        assert_eq!((engine.pdf("thesis/main.tex").as_str(), engine.log("thesis/main.tex").as_str()), ("thesis/main.pdf", "thesis/main.log"));
        assert_eq!(with_extension("a.b/c", "pdf"), "a.b/c.pdf");
    }

    #[test]
    fn continuous_is_latexmks_and_clean_is_too() {
        let b = Builder::Latexmk { path: "latexmk".into(), engine: Some("pdflatex".into()) };
        assert_eq!(line(&b.command("main.tex", true).unwrap()), "latexmk -pdf -interaction=nonstopmode -file-line-error -synctex=1 -pvc -view=none main.tex");
        assert_eq!(line(&b.clean("main.tex", false).unwrap()), "latexmk -c main.tex");
        assert_eq!(line(&b.clean("a/main.tex", true).unwrap()), "latexmk -C -cd a/main.tex");
        for other in [Builder::Tectonic { path: "t".into() }, Builder::Engine { name: "xelatex".into(), path: "x".into() }] {
            assert!(other.command("main.tex", true).is_none(), "{}", other.describe());
            assert!(other.clean("main.tex", false).is_none());
        }
        assert_eq!(b.describe(), "latexmk with pdflatex");
    }
}
