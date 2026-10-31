//! Which kinds of config file the project has, found by walking it (not
//! into dependencies, builds or virtual environments, and only so far):
//! the doctor checks the tools for those, and no others. A lone README
//! doesn't make a Markdown project.

use std::path::Path;

/// How deep the walk goes, and how many entries it reads at most.
pub const DEPTH: usize = 6;
pub const ENTRIES: usize = 20_000;

/// Folders never walked into.
const SKIP: &[&str] = &["node_modules", "target", "vendor", "dist", "build", "out", "bin", "obj", "venv", "__pycache__", "bower_components", "Pods"];

/// Prettier's config files; `package.json`'s `prettier` key is one too.
const PRETTIER: &[&str] = &[
    ".prettierrc", ".prettierrc.json", ".prettierrc.json5", ".prettierrc.yaml", ".prettierrc.yml", ".prettierrc.toml", ".prettierrc.js",
    ".prettierrc.cjs", ".prettierrc.mjs", ".prettierrc.ts", ".prettierrc.cts", ".prettierrc.mts", "prettier.config.js", "prettier.config.cjs",
    "prettier.config.mjs", "prettier.config.ts", "prettier.config.cts", "prettier.config.mts",
];

/// The kinds the pack serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Yaml,
    Json,
    Toml,
    Dockerfile,
    Shell,
    Markdown,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Yaml, Kind::Json, Kind::Toml, Kind::Dockerfile, Kind::Shell, Kind::Markdown];

    /// A file's kind, by its name.
    pub fn of(name: &str) -> Option<Kind> {
        let lower = name.to_ascii_lowercase();
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if lower == "dockerfile" || lower == "containerfile" || lower.starts_with("dockerfile.") || lower.starts_with("containerfile.") || ext == "dockerfile" {
            return Some(Kind::Dockerfile);
        }
        Some(match ext {
            "yaml" | "yml" => Kind::Yaml,
            "json" | "jsonc" => Kind::Json,
            "toml" => Kind::Toml,
            "sh" | "bash" | "bats" | "ksh" => Kind::Shell,
            "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdwn" => Kind::Markdown,
            _ => return None,
        })
    }

    /// Fenix's name for its language.
    pub fn language(self) -> &'static str {
        match self {
            Kind::Yaml => "yaml",
            Kind::Json => "json",
            Kind::Toml => "toml",
            Kind::Dockerfile => "dockerfile",
            Kind::Shell => "bash",
            Kind::Markdown => "markdown",
        }
    }
}

/// What the walk found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// How many files of each kind, in [`Kind::ALL`]'s order.
    pub counts: [usize; 6],
    /// The shell scripts and Dockerfiles, by their paths in the project:
    /// the tasks lint them all.
    pub scripts: Vec<String>,
    pub dockerfiles: Vec<String>,
    /// A Prettier config at the top.
    pub prettier: bool,
    /// A Biome config at the top: the Web pack's Biome formats its JSON.
    pub biome: bool,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let mut p = Project::default();
        let mut budget = ENTRIES;
        walk(Path::new(root), "", 0, &mut budget, &mut p);
        p.scripts.sort();
        p.dockerfiles.sort();
        p.prettier = PRETTIER.iter().any(|f| Path::new(root).join(f).is_file())
            || std::fs::read_to_string(Path::new(root).join("package.json")).is_ok_and(|t| fenix_lang_kit::serde_json::from_str::<fenix_lang_kit::serde_json::Value>(&t).is_ok_and(|v| v.get("prettier").is_some()));
        p.biome = ["biome.json", "biome.jsonc"].iter().any(|f| Path::new(root).join(f).is_file());
        p
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.counts[Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0)]
    }

    /// Whether it has `kind`'s files enough to check their tools: any,
    /// but more than a README for Markdown.
    pub fn has(&self, kind: Kind) -> bool {
        match kind {
            Kind::Markdown => self.count(kind) > 1,
            _ => self.count(kind) > 0,
        }
    }
}

fn walk(dir: &Path, rel: &str, depth: usize, budget: &mut usize, p: &mut Project) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            // Dot folders hold tools' state, but GitHub's and editors' config.
            let hidden = name.starts_with('.') && !matches!(name.as_str(), ".github" | ".gitlab" | ".circleci" | ".devcontainer" | ".vscode" | ".config");
            if depth + 1 < DEPTH && !hidden && !SKIP.contains(&name.as_str()) {
                walk(&entry.path(), &path, depth + 1, budget, p);
            }
            continue;
        }
        let Some(k) = Kind::of(&name) else { continue };
        p.counts[Kind::ALL.iter().position(|x| *x == k).unwrap_or(0)] += 1;
        match k {
            Kind::Shell => p.scripts.push(path),
            Kind::Dockerfile => p.dockerfiles.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, files: &[&str]) {
        for f in files {
            let path = dir.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
    }

    #[test]
    fn files_are_known_by_their_names() {
        assert_eq!(Kind::of("Dockerfile"), Some(Kind::Dockerfile));
        assert_eq!(Kind::of("Containerfile.dev"), Some(Kind::Dockerfile));
        assert_eq!(Kind::of("api.dockerfile"), Some(Kind::Dockerfile));
        assert_eq!(Kind::of("ci.YML"), Some(Kind::Yaml));
        assert_eq!(Kind::of("tsconfig.json"), Some(Kind::Json));
        assert_eq!(Kind::of("Cargo.toml"), Some(Kind::Toml));
        assert_eq!(Kind::of("install.sh"), Some(Kind::Shell));
        assert_eq!(Kind::of("README.md"), Some(Kind::Markdown));
        assert_eq!(Kind::of("main.rs"), None);
        assert_eq!(Kind::Shell.language(), "bash");
    }

    #[test]
    fn the_walk_counts_the_projects_files_and_skips_dependencies() {
        let dir = temp("walk");
        write(
            &dir,
            &[
                "README.md", "Cargo.toml", ".github/workflows/ci.yml", "deploy/Dockerfile", "deploy/Dockerfile.prod", "scripts/release.sh",
                "node_modules/x/package.json", ".git/config.json", "target/debug/x.json", ".prettierrc",
            ],
        );
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.counts, [1, 0, 1, 2, 1, 1]);
        assert!(p.has(Kind::Yaml) && p.has(Kind::Toml) && !p.has(Kind::Json));
        assert!(!p.has(Kind::Markdown), "a README alone");
        assert_eq!(p.dockerfiles, ["deploy/Dockerfile", "deploy/Dockerfile.prod"]);
        assert_eq!(p.scripts, ["scripts/release.sh"]);
        assert!(p.prettier);
        write(&dir, &["docs/guide.md"]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.has(Kind::Markdown));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn package_jsons_prettier_key_is_a_config() {
        let dir = temp("pkg");
        std::fs::write(dir.join("package.json"), r#"{"name": "x", "prettier": {"semi": false}}"#).unwrap();
        assert!(Project::read(&dir.to_string_lossy()).prettier);
        std::fs::write(dir.join("package.json"), r#"{"name": "x"}"#).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).prettier);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
