//! Which projects the language server may be run through the dev shell
//! for. A dev shell runs the project's own code (its `shellHook`, what its
//! flake evaluates), so opening a `.nix` file in a repository you've just
//! cloned must never start one: Fenix applies a project's
//! `.fenix/settings.toml` without asking, so a setting there can't be what
//! says so. The choice is kept in the pack's own data folder, by the
//! project's full path, and written only when you run the doctor's fix row
//! and the task it starts works -- never read from the project.

use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};

/// The file, in the pack's data folder.
pub const FILE: &str = "dev-shell-projects.json";

/// A path as it's compared: `/` between parts, none at the end.
pub fn normal(path: &str) -> String {
    let p = path.replace('\\', "/");
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() { p } else { trimmed.to_string() }
}

/// The projects remembered; none when there's no file, or it isn't ours.
pub fn list(data: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(Path::new(data).join(FILE)) else { return Vec::new() };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(normal)).collect(),
        _ => Vec::new(),
    }
}

pub fn contains(data: &str, root: &str) -> bool {
    let root = normal(root);
    list(data).contains(&root)
}

fn write(data: &str, projects: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(data).map_err(|e| format!("{data}: {e}"))?;
    let text = serde_json::to_string_pretty(projects).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(data).join(FILE), text + "\n").map_err(|e| format!("{data}/{FILE}: {e}"))
}

/// `root` remembered.
pub fn add(data: &str, root: &str) -> Result<(), String> {
    let root = normal(root);
    let mut all = list(data);
    if !all.contains(&root) {
        all.push(root);
        all.sort();
    }
    write(data, &all)
}

/// `root` forgotten; whether it was there.
pub fn remove(data: &str, root: &str) -> Result<bool, String> {
    let root = normal(root);
    let mut all = list(data);
    let before = all.len();
    all.retain(|p| *p != root);
    write(data, &all)?;
    Ok(all.len() != before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_remembered_by_its_full_path_and_only_that_one() {
        let dir = crate::project::tests::temp("trust");
        let data = dir.to_string_lossy().into_owned();
        assert!(!contains(&data, "/home/me/a") && list(&data).is_empty());
        add(&data, "/home/me/a/").unwrap();
        add(&data, "/home/me/a").unwrap();
        assert_eq!(list(&data), ["/home/me/a"], "once, however it's spelled");
        assert!(contains(&data, "/home/me/a") && contains(&data, "\\home\\me\\a"));
        assert!(!contains(&data, "/home/me/ab") && !contains(&data, "/home/me") && !contains(&data, "/home/me/a/b"), "not a neighbour, a parent or a child");
        add(&data, "/home/me/b").unwrap();
        assert!(remove(&data, "/home/me/a").unwrap() && !remove(&data, "/home/me/a").unwrap());
        assert_eq!(list(&data), ["/home/me/b"]);
        std::fs::write(dir.join(FILE), "not json").unwrap();
        assert!(list(&data).is_empty(), "what isn't ours is nothing");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
