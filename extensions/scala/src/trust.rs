//! What you've said about each project: may Metals run in it, importing its
//! build. A build is code (`build.sbt` and `project/*.scala`, `build.mill`, a
//! Gradle script, a `.bsp/*.json`'s command), and Metals connects to it when it
//! starts, so it isn't started in a project until you've said Import. The answer
//! is kept in the pack's own data folder, by the project's full path, so a
//! copy of the project somewhere else is asked about again. Nothing of the
//! project's own -- not `.fenix/settings.toml` -- can say it.

use std::collections::BTreeMap;
use std::path::Path;

use fenix_lang_kit::serde_json;

/// The file in the data folder.
pub const FILE: &str = "scala-projects.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Never,
}

fn read(dir: &str) -> BTreeMap<String, String> {
    std::fs::read_to_string(Path::new(dir).join(FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// What was said of the project at `project` (its full path).
pub fn get(dir: &str, project: &str) -> Option<Decision> {
    match read(dir).get(project).map(String::as_str) {
        Some("allow") => Some(Decision::Allow),
        Some("never") => Some(Decision::Never),
        _ => None,
    }
}

/// Kept, or forgotten with `None`.
pub fn set(dir: &str, project: &str, decision: Option<Decision>) -> Result<(), String> {
    let mut all = read(dir);
    match decision {
        Some(Decision::Allow) => all.insert(project.to_string(), "allow".into()),
        Some(Decision::Never) => all.insert(project.to_string(), "never".into()),
        None => all.remove(project),
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(dir).join(FILE), text).map_err(|e| format!("{dir}: {e}"))
}
