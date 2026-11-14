//! The tasks (`SPC p t`), each run in the project's folder:
//!
//! | Task | Runs |
//! |---|---|
//! | `build` | what builds the document: latexmk, Tectonic or an engine |
//! | `clean` | `latexmk -c`: the auxiliary files |
//! | `clean (all)` | `latexmk -C`: and the PDF |
//! | `word count` | `texcount -brief -inc` of the document (texcount) |
//!
//! `build continuously` isn't here: a task in the picker runs in the task
//! pane, one at a time, so `latexmk -pvc` would hold it for good.
//! `latex.watch` runs it in the background (`SPC m w`). `clean` is latexmk's:
//! it's the only one that knows what it left.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::build::Builder;
use crate::tools::Tools;

/// The tasks for the document `main`, built by `builder`; the word count
/// needs only texcount.
pub fn tasks(main: Option<&str>, builder: Option<&Builder>, tools: &Tools) -> Tasks {
    let mut t = Tasks::default();
    let Some(main) = main else { return t };
    if let Some(builder) = builder {
        if let Some(c) = builder.command(main, false) {
            t.add("build", c);
        }
        if let Some(c) = builder.clean(main, false) {
            t.add("clean", c);
        }
        if let Some(c) = builder.clean(main, true) {
            t.add("clean (all)", c);
        }
    }
    if let Some(texcount) = &tools.texcount {
        t.add("word count", word_count(texcount, main));
    }
    t
}

/// `texcount` on `main`, its `\input`s and `\include`s counted too.
pub fn word_count(texcount: &str, main: &str) -> Command {
    Command::new(texcount).args(["-brief", "-inc", "-utf8"]).arg(main)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::line;

    fn lines(t: &Tasks) -> Vec<(String, String)> {
        t.tasks.iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    #[test]
    fn latexmk_builds_cleans_and_counts_and_the_others_build_and_count() {
        let latexmk = Builder::Latexmk { path: "/t/latexmk".into(), engine: Some("pdflatex".into()) };
        let tools = Tools { texcount: Some("/t/texcount".into()), ..Default::default() };
        let got = lines(&tasks(Some("thesis/main.tex"), Some(&latexmk), &tools));
        assert_eq!(got[0], ("build".into(), "/t/latexmk -pdf -interaction=nonstopmode -file-line-error -synctex=1 -cd thesis/main.tex".into()));
        assert_eq!(got[1], ("clean".into(), "/t/latexmk -c -cd thesis/main.tex".into()));
        assert_eq!(got[2], ("clean (all)".into(), "/t/latexmk -C -cd thesis/main.tex".into()));
        assert_eq!(got[3], ("word count".into(), "/t/texcount -brief -inc -utf8 thesis/main.tex".into()));

        let tectonic = Builder::Tectonic { path: "/t/tectonic".into() };
        let got = lines(&tasks(Some("main.tex"), Some(&tectonic), &Tools::default()));
        assert_eq!(got, [("build".to_string(), "/t/tectonic -X compile --keep-logs --synctex main.tex".to_string())], "nothing of its own to clean; no texcount");
        let only_count = lines(&tasks(Some("main.tex"), None, &tools));
        assert_eq!(only_count.len(), 1);
        assert!(tasks(None, Some(&latexmk), &tools).tasks.is_empty(), "no document, no tasks");
    }
}
