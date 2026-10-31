//! A pull or merge request being written and not opened yet, kept: what
//! its form says -- title, base, description, reviewers and the rest --
//! for its project's branch, in the extension's own folder
//! (`/data/requests/`). Written as the form changes, read when the form
//! opens again for that branch, gone once the request is opened. A
//! description half written isn't lost to a closed page or window.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where drafts are kept: `/data/requests` for the extension.
pub const REQUESTS: &str = "/data/requests";

/// What the form says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestDraft {
    pub base: String,
    pub title: String,
    pub description: String,
    pub draft: bool,
    pub reviewers: String,
    pub assignees: String,
    pub labels: String,
}

fn slug(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// `project`'s draft for `branch`, under `base`.
fn file(base: &Path, project: &str, branch: &str) -> PathBuf {
    base.join(slug(project)).join(format!("{}.json", slug(branch)))
}

pub fn load(base: &Path, project: &str, branch: &str) -> Option<RequestDraft> {
    std::fs::read_to_string(file(base, project, branch)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn save(base: &Path, project: &str, branch: &str, draft: &RequestDraft) -> Result<(), String> {
    let path = file(base, project, branch);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(draft).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn clear(base: &Path, project: &str, branch: &str) {
    let _ = std::fs::remove_file(file(base, project, branch));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_is_kept_per_branch_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("requests");
        let draft = RequestDraft { title: "Status page".into(), description: "Half of it".into(), reviewers: "ana".into(), ..Default::default() };
        save(&base, "group/project", "feature/FNX-58-status", &draft).unwrap();
        assert_eq!(load(&base, "group/project", "feature/FNX-58-status"), Some(draft));
        assert_eq!(load(&base, "group/project", "main"), None);
        assert!(base.join("group_project/feature_FNX-58-status.json").exists());
        clear(&base, "group/project", "feature/FNX-58-status");
        assert_eq!(load(&base, "group/project", "feature/FNX-58-status"), None);
    }
}
