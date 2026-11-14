//! The tasks (`SPC p t`), each run in the project's folder:
//!
//! | Task | Runs |
//! |---|---|
//! | `compile` | `typst compile` of the document to its PDF |
//! | `compile (png)` | the document's pages as `<name>-{p}.png` |
//! | `fonts` | `typst fonts`: the fonts Typst finds |
//! | `format -check` | `typstyle --check` of the project's `.typ` files |
//!
//! `watch` isn't here: a task in the picker runs in the task pane, one at a
//! time, so a watch would hold it for good. `typst.watch` runs it in the
//! background (`SPC m w`).

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::compile;
use crate::project::Project;
use crate::tools::Tools;

/// The tasks for the document `main` (in the project), or none when there
/// is none and nothing of Typst's.
pub fn tasks(project: &Project, tools: &Tools) -> Tasks {
    let mut t = Tasks::default();
    if !project.typst {
        return t;
    }
    if let Some(main) = &project.main {
        let pdf = Project::pdf(main);
        let png = pdf.trim_end_matches(".pdf").to_string() + "-{p}.png";
        t.add("compile", compile::compile(tools.typst(), main, &pdf));
        t.add("compile (png)", compile::compile(tools.typst(), main, &png));
    }
    t.add("fonts", tools.typst().arg("fonts"));
    if let Some(style) = &tools.typstyle {
        t.add("format -check", Command::new(style).args(["--check", "."]));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::line;

    fn lines(t: &Tasks) -> Vec<(String, String)> {
        t.tasks.iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    #[test]
    fn the_document_compiles_and_the_rest_are_there_without_one() {
        let project = Project { main: Some("src/main.typ".into()), typst: true };
        let tools = Tools { typst: Some("/t/typst".into()), typstyle: Some("/t/typstyle".into()), ..Default::default() };
        let got = lines(&tasks(&project, &tools));
        assert_eq!(got[0], ("compile".into(), "/t/typst compile --diagnostic-format short --root . src/main.typ src/main.pdf".into()));
        assert_eq!(got[1].1, "/t/typst compile --diagnostic-format short --root . src/main.typ src/main-{p}.png");
        assert_eq!(got[2], ("fonts".into(), "/t/typst fonts".into()));
        assert_eq!(got[3], ("format -check".into(), "/t/typstyle --check .".into()));

        let no_doc = lines(&tasks(&Project { main: None, typst: true }, &Tools::default()));
        assert_eq!(no_doc, [("fonts".to_string(), "typst fonts".to_string())], "typstyle isn't found: no format task");
        assert!(tasks(&Project::default(), &tools).tasks.is_empty(), "not a Typst project");
    }
}
