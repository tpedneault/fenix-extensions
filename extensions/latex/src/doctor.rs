//! The project doctor's LaTeX checks: whether anything builds the document
//! (latexmk, Tectonic, or an engine) with a fix that installs Tectonic when
//! nothing does; texlab, the language server; tex-fmt, the formatter; and
//! chktex, texcount and synctex, which the lint, the word count and the
//! forward search use. Each of the three the pack knows has a fix row that
//! installs the release for this machine, checked against its SHA-256.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{Fix, Program};

use crate::build;
use crate::project::Project;
use crate::tools::{hints, Platform, Tools};

/// `name`, looked for where the pack installs it and where users put it.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint).optional()
}

/// The checks of single programs.
pub fn programs(platform: &Platform) -> Vec<Program> {
    let installs = platform.installs();
    let has = |name: &str| installs.iter().any(|i| i.name == name);
    let mut texlab = program("texlab", "texlab").version_args(["--version"]);
    if has("texlab") {
        texlab = texlab.fix("Install texlab", "latex.install.texlab", Fix::Install("texlab".into()));
    }
    let mut tex_fmt = program("tex-fmt", "tex-fmt");
    if has("tex-fmt") {
        tex_fmt = tex_fmt.fix("Install tex-fmt", "latex.install.tex-fmt", Fix::Install("tex-fmt".into()));
    }
    // texcount and synctex have no `--version`: `-version` and `help` print theirs.
    vec![texlab, tex_fmt, program("chktex", "chktex"), program("texcount", "texcount").version_args(["-version"]), program("synctex", "synctex").version_args(["help"])]
}

/// The check of what builds: what will, for the document `main`, or what to do.
pub fn build_check(tools: &Tools, project: &Project, platform: &Platform, program: Option<&str>) -> Check {
    match build::choose(tools, project, program) {
        Some(builder) => {
            let path = match &builder {
                build::Builder::Latexmk { path, .. } | build::Builder::Tectonic { path } | build::Builder::TectonicProject { path, .. } | build::Builder::Engine { path, .. } => path,
            };
            Check::new(Section::Toolchain, "LaTeX build", Health::Ok, format!("{} ({path})", builder.describe()))
        }
        None => {
            let health = if project.latex { Health::Bad } else { Health::Warn };
            let has_tectonic = platform.tectonic().is_some();
            let detail = if has_tectonic {
                "no latexmk, Tectonic, pdflatex, xelatex or lualatex found. A TeX distribution (TeX Live, MiKTeX, MacTeX) is yours to install; the fix installs Tectonic instead, one self-contained program that downloads the packages a document uses the first time it builds".to_string()
            } else {
                "no latexmk, Tectonic, pdflatex, xelatex or lualatex found: install a TeX distribution (TeX Live, MiKTeX, MacTeX)".to_string()
            };
            let check = Check::new(Section::Toolchain, "LaTeX build", health, detail);
            if has_tectonic {
                check.fix("Install Tectonic (it downloads packages on first build)", "latex.install.tectonic")
            } else {
                check
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::Output;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn tools(names: &[&str]) -> Tools {
        let mut t = Tools::default();
        for n in names {
            *t.slot(n).unwrap() = Some(format!("/t/{n}"));
        }
        t
    }

    #[test]
    fn the_server_and_the_formatter_have_fix_rows_where_there_is_a_release() {
        let linux = Platform::new("linux", "x86_64");
        let p = programs(&linux);
        assert_eq!(p.iter().map(|p| (p.label.as_str(), p.required)).collect::<Vec<_>>(), [("texlab", false), ("tex-fmt", false), ("chktex", false), ("texcount", false), ("synctex", false)]);
        assert_eq!((p[0].fix.as_ref().map(|f| f.1.as_str()), p[1].fix.as_ref().map(|f| f.1.as_str())), (Some("latex.install.texlab"), Some("latex.install.tex-fmt")));
        assert!(p[2..].iter().all(|p| p.fix.is_none()), "a TeX distribution's own");
        let c = p[0].evaluate(Some("/tools/texlab/texlab"), Some(&said(include_str!("samples/texlab-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "5.26.0 (/tools/texlab/texlab)"));
        let c = p[1].evaluate(Some("/t/tex-fmt"), Some(&said(include_str!("samples/tex-fmt-version.txt"))));
        assert_eq!(c.detail, "0.5.7 (/t/tex-fmt)");
        let missing = p[0].evaluate(None, None);
        assert_eq!((missing.health, missing.fix.map(|f| f.1)), (Health::Warn, Some("latex.install.texlab".into())), "Fenix works without it");
        assert!(programs(&Platform::new("linux", "riscv64")).iter().all(|p| p.fix.is_none()));
        // chktex's and texcount's own words.
        let c = p[2].evaluate(Some("/usr/bin/chktex"), Some(&said("ChkTeX v1.7.8 - Copyright 1995-96 Jens T. Berger Thielemann.\n")));
        assert_eq!(c.detail, "1.7.8 (/usr/bin/chktex)");
        let c = p[3].evaluate(Some("/usr/bin/texcount"), Some(&said("\nTeXcount version 3.2.0.41, 2020-11-22\n")));
        assert!(c.detail.starts_with("3.2.0"), "{}", c.detail);
    }

    #[test]
    fn what_builds_is_said_and_when_nothing_does_tectonic_is_the_fix_and_its_text_says_it_downloads() {
        let linux = Platform::new("linux", "x86_64");
        let project = Project { main: Some("main.tex".into()), latex: true, ..Default::default() };
        let c = build_check(&tools(&["latexmk", "pdflatex"]), &project, &linux, None);
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "latexmk with pdflatex (/t/latexmk)"));
        let c = build_check(&tools(&["tectonic"]), &project, &linux, None);
        assert_eq!(c.detail, "Tectonic (/t/tectonic)");
        let c = build_check(&tools(&["xelatex"]), &project, &linux, Some("xelatex"));
        assert_eq!(c.detail, "xelatex (/t/xelatex)");

        let none = build_check(&Tools::default(), &project, &linux, None);
        assert_eq!(none.health, Health::Bad, "a LaTeX project needs one");
        let fix = none.fix.clone().unwrap();
        assert_eq!(fix.1, "latex.install.tectonic");
        assert!(fix.0.contains("downloads packages on first build"), "{}", fix.0);
        assert!(none.detail.contains("TeX Live") && none.detail.contains("downloads the packages a document uses the first time it builds"), "{}", none.detail);

        let other = build_check(&Tools::default(), &Project::default(), &linux, None);
        assert_eq!(other.health, Health::Warn, "a folder that isn't LaTeX's isn't failed for it");
        let arm = build_check(&Tools::default(), &project, &Platform::new("windows", "aarch64"), None);
        assert!(arm.fix.is_none(), "no Tectonic release for Windows on ARM");
        assert!(arm.detail.starts_with("no latexmk") && !arm.detail.contains("Tectonic instead"), "{}", arm.detail);
    }
}
