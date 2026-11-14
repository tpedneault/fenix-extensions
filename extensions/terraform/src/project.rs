//! What the pack knows of the project, from its files: where its root
//! module is -- the folder `terraform init`, `plan` and `apply` run in --
//! which is the project's top when it holds `.tf` files, else the one
//! folder under it that does (`infra/`, `terraform/`).

use std::path::Path;

/// Folders a root module is often kept in, tried in this order when
/// several folders hold `.tf` files and the top doesn't.
pub const USUAL: &[&str] = &["terraform", "infra", "infrastructure", "tf", "deploy", "iac"];

/// Folders never looked in: a checkout of providers and modules, a
/// dependency folder, a build folder.
const SKIPPED: &[&str] = &["node_modules", "target", "vendor", "modules", "examples", "test", "tests"];

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The root module's folder in the project: `.` for the top, or
    /// `infra`. `None`: no `.tf` file where it looked, so not a Terraform
    /// project (yet).
    pub module: Option<String>,
}

fn has_tf(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "tf") && e.path().is_file())
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        if has_tf(root) {
            return Project { module: Some(".".into()) };
        }
        let mut found: Vec<String> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.') && !SKIPPED.contains(&n.as_str()) && has_tf(&root.join(n)))
            .collect();
        found.sort();
        let module = match found.len() {
            0 => None,
            1 => found.pop(),
            // Several: the one named as usual; none of them is the one.
            _ => USUAL.iter().find(|u| found.iter().any(|f| f == *u)).map(|u| u.to_string()),
        };
        Project { module }
    }

    /// Whether it holds a root module the pack can run things in.
    pub fn is_terraform(&self) -> bool {
        self.module.is_some()
    }

    /// `-chdir=<folder>` for Terraform to run in the root module's folder
    /// when it isn't the project's top; nothing for the top.
    pub fn chdir(&self) -> Option<String> {
        self.module.as_deref().filter(|m| *m != ".").map(|m| format!("-chdir={m}"))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-terraform-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, path: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "").unwrap();
    }

    #[test]
    fn the_root_module_is_the_top_or_the_folder_that_holds_one() {
        let dir = temp("module");
        assert_eq!(Project::read(&dir.to_string_lossy()), Project { module: None });
        assert!(!Project::read(&dir.to_string_lossy()).is_terraform());

        // Modules and examples below a root module aren't candidates.
        touch(&dir, "modules/net/main.tf");
        touch(&dir, "infra/main.tf");
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.module.as_deref(), p.chdir().as_deref()), (Some("infra"), Some("-chdir=infra")), "the one folder with .tf files");

        touch(&dir, "stacks/prod/main.tf");
        touch(&dir, "stacks/prod.tf");
        assert_eq!(Project::read(&dir.to_string_lossy()).module.as_deref(), Some("infra"), "stacks/ has no .tf of its own; infra is the only one");
        touch(&dir, "terraform/main.tf");
        assert_eq!(Project::read(&dir.to_string_lossy()).module.as_deref(), Some("terraform"), "two folders: the usual name first");
        touch(&dir, "zzz/main.tf");
        touch(&dir, "aaa/main.tf");
        std::fs::remove_dir_all(dir.join("terraform")).unwrap();
        std::fs::remove_dir_all(dir.join("infra")).unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()).module, None, "several, none the usual: it isn't guessed");

        touch(&dir, "main.tf");
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.module.as_deref(), p.chdir()), (Some("."), None));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
