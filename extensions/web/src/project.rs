//! What the project is, as its files say: `package.json` (its scripts,
//! its dependencies and their groups, `engines.node`, corepack's
//! `packageManager`), the lockfile and so the package manager, the Node
//! it pins (`.nvmrc`, `.node-version`), and the tools it uses -- ESLint,
//! Prettier or Biome, Tailwind, Vite, Next.js, Vitest, Jest.

use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};

/// A package manager, by its lockfile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Manager {
    #[default]
    Npm,
    Pnpm,
    /// Yarn 1.
    Yarn,
    /// Yarn 2 and later.
    Berry,
    Bun,
}

impl Manager {
    /// Its program.
    pub fn program(self) -> &'static str {
        match self {
            Manager::Npm => "npm",
            Manager::Pnpm => "pnpm",
            Manager::Yarn | Manager::Berry => "yarn",
            Manager::Bun => "bun",
        }
    }

    /// What the page calls it.
    pub fn name(self) -> &'static str {
        match self {
            Manager::Berry => "yarn (berry)",
            m => m.program(),
        }
    }
}

/// One of the project's own dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    /// `None` for `dependencies`; `dev`, `optional` or `peer`.
    pub group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub package_json: bool,
    pub name: Option<String>,
    /// `scripts`, by name.
    pub scripts: Vec<(String, String)>,
    pub deps: Vec<Dep>,
    /// `engines.node`.
    pub engines_node: Option<String>,
    /// corepack's `packageManager`: `pnpm@9.12.0+sha512...` as (`pnpm`,
    /// `9.12.0`).
    pub package_manager: Option<(String, String)>,
    /// `.nvmrc` or `.node-version`: the file, and the version it says.
    pub node_file: Option<(String, String)>,
    /// The lockfile there is, by name.
    pub lockfile: Option<String>,
    pub manager: Manager,
    pub workspaces: bool,
    pub typescript: bool,
    pub eslint: bool,
    pub prettier: bool,
    pub biome: bool,
    pub tailwind: bool,
    pub vite: bool,
    pub next: bool,
    pub vitest: bool,
    pub jest: bool,
}

/// The lockfiles, each for its manager; the first there wins.
pub const LOCKFILES: &[(&str, Manager)] = &[
    ("pnpm-lock.yaml", Manager::Pnpm),
    ("bun.lock", Manager::Bun),
    ("bun.lockb", Manager::Bun),
    ("yarn.lock", Manager::Yarn),
    ("package-lock.json", Manager::Npm),
    ("npm-shrinkwrap.json", Manager::Npm),
];

const ESLINT_FILES: &[&str] = &[
    "eslint.config.js", "eslint.config.mjs", "eslint.config.cjs", "eslint.config.ts", "eslint.config.mts", "eslint.config.cts",
    ".eslintrc", ".eslintrc.js", ".eslintrc.cjs", ".eslintrc.json", ".eslintrc.yml", ".eslintrc.yaml",
];
const PRETTIER_FILES: &[&str] = &[
    ".prettierrc", ".prettierrc.json", ".prettierrc.json5", ".prettierrc.yaml", ".prettierrc.yml", ".prettierrc.toml",
    ".prettierrc.js", ".prettierrc.cjs", ".prettierrc.mjs", ".prettierrc.ts", "prettier.config.js", "prettier.config.cjs", "prettier.config.mjs", "prettier.config.ts",
];
const BIOME_FILES: &[&str] = &["biome.json", "biome.jsonc"];
const TAILWIND_FILES: &[&str] = &["tailwind.config.js", "tailwind.config.cjs", "tailwind.config.mjs", "tailwind.config.ts"];

/// `name@1.2.3+sha...` as its name and version.
pub fn package_manager(field: &str) -> Option<(String, String)> {
    let (name, version) = field.trim().rsplit_once('@')?;
    let version = version.split('+').next().unwrap_or(version).trim();
    (!name.is_empty() && !version.is_empty()).then(|| (name.to_string(), version.to_string()))
}

/// The Node `.nvmrc` names, as a version: `v20.11.0`, `20`, and the LTS
/// lines' names (`lts/iron` is 20). `lts/*` and `node` (the newest) say
/// no version.
pub fn nvm_version(text: &str) -> Option<String> {
    let t = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?.to_ascii_lowercase();
    if let Some(name) = t.strip_prefix("lts/") {
        const LINES: &[(&str, &str)] = &[
            ("argon", "4"), ("boron", "6"), ("carbon", "8"), ("dubnium", "10"), ("erbium", "12"), ("fermium", "14"),
            ("gallium", "16"), ("hydrogen", "18"), ("iron", "20"), ("jod", "22"), ("krypton", "24"),
        ];
        return LINES.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string());
    }
    let v = t.trim_start_matches('v');
    v.starts_with(|c: char| c.is_ascii_digit()).then(|| v.to_string())
}

impl Project {
    /// From `package.json`'s text, and which of `files` (names in the
    /// project's folder) are there.
    pub fn parse(package_json: &str, files: &[&str]) -> Result<Project, String> {
        let doc: Value = serde_json::from_str(package_json).map_err(|e| format!("package.json: {e}"))?;
        let mut p = Project { package_json: true, ..Project::default() };
        p.name = doc["name"].as_str().map(str::to_string);
        if let Some(scripts) = doc["scripts"].as_object() {
            p.scripts = scripts.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
        }
        for (key, group) in [("dependencies", None), ("devDependencies", Some("dev")), ("optionalDependencies", Some("optional")), ("peerDependencies", Some("peer"))] {
            if let Some(deps) = doc[key].as_object() {
                for name in deps.keys() {
                    if !p.deps.iter().any(|d| d.name == *name) {
                        p.deps.push(Dep { name: name.clone(), group: group.map(str::to_string) });
                    }
                }
            }
        }
        p.engines_node = doc["engines"]["node"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        p.package_manager = doc["packageManager"].as_str().and_then(package_manager);
        p.workspaces = !doc["workspaces"].is_null();
        let has = |name: &str| files.contains(&name);
        let any = |names: &[&str]| names.iter().any(|n| has(n));
        p.lockfile = LOCKFILES.iter().find(|(f, _)| has(f)).map(|(f, _)| f.to_string());
        p.manager = match LOCKFILES.iter().find(|(f, _)| has(f)) {
            Some((_, Manager::Yarn)) if has(".yarnrc.yml") => Manager::Berry,
            Some((_, m)) => *m,
            None => match p.package_manager.as_ref().map(|(n, v)| (n.as_str(), v.as_str())) {
                Some(("pnpm", _)) => Manager::Pnpm,
                Some(("yarn", v)) if v.starts_with('1') => Manager::Yarn,
                Some(("yarn", _)) => Manager::Berry,
                Some(("bun", _)) => Manager::Bun,
                _ if has(".yarnrc.yml") => Manager::Berry,
                _ => Manager::Npm,
            },
        };
        p.typescript = p.has("typescript") || has("tsconfig.json");
        p.eslint = p.has("eslint") || any(ESLINT_FILES) || !doc["eslintConfig"].is_null();
        p.biome = p.has("@biomejs/biome") || any(BIOME_FILES);
        p.prettier = p.has("prettier") || any(PRETTIER_FILES) || !doc["prettier"].is_null();
        p.tailwind = p.has("tailwindcss") || p.has("@tailwindcss/vite") || p.has("@tailwindcss/postcss") || any(TAILWIND_FILES);
        p.vite = p.has("vite");
        p.next = p.has("next");
        p.vitest = p.has("vitest");
        p.jest = p.has("jest");
        Ok(p)
    }

    /// The project in `root`: none when there's no `package.json` (or it
    /// doesn't read; that's said where it's used).
    pub fn read(root: &str) -> Project {
        let dir = Path::new(root);
        let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { return Project::default() };
        let names: Vec<String> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        let files: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut p = Project::parse(&text, &files).unwrap_or_else(|_| Project { package_json: true, ..Project::default() });
        // A lockfile from Yarn 2 and later says so in its header.
        if p.manager == Manager::Yarn && std::fs::read_to_string(dir.join("yarn.lock")).is_ok_and(|t| t.contains("__metadata:")) {
            p.manager = Manager::Berry;
        }
        for f in [".nvmrc", ".node-version"] {
            if let Some(v) = std::fs::read_to_string(dir.join(f)).ok().and_then(|t| nvm_version(&t)) {
                p.node_file = Some((f.to_string(), v));
                break;
            }
        }
        p
    }

    /// Whether it asks for `name`, in any group.
    pub fn has(&self, name: &str) -> bool {
        self.deps.iter().any(|d| d.name == name)
    }

    /// The group of dependency `name`: `None` when it doesn't ask for it,
    /// `Some(None)` in `dependencies`.
    pub fn group_of(&self, name: &str) -> Option<Option<&str>> {
        self.deps.iter().find(|d| d.name == name).map(|d| d.group.as_deref())
    }

    /// What Node it asks for, and where it says so: the pinned one
    /// (`.nvmrc`, `.node-version`; `20.11.0` is that or a later 20), else
    /// `engines.node`.
    pub fn node_wants(&self) -> Option<(String, String)> {
        if let Some((file, v)) = &self.node_file {
            let wants = if v.split('.').count() >= 3 { format!("^{v}") } else { v.clone() };
            return Some((wants, file.clone()));
        }
        self.engines_node.clone().map(|e| (e, "engines.node".to_string()))
    }
}

/// The program a package in `node_modules` runs as `bin` (`eslint`'s
/// `bin/eslint.js`), relative to `root`: what `node` runs, so no shell
/// or `.cmd` is needed. `None` when it isn't installed.
pub fn local_bin(root: &str, package: &str, bin: &str) -> Option<String> {
    let dir = format!("node_modules/{package}");
    let text = std::fs::read_to_string(Path::new(root).join(&dir).join("package.json")).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    let rel = match &doc["bin"] {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.get(bin).and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    let rel = rel.trim_start_matches("./");
    Path::new(root).join(&dir).join(rel).is_file().then(|| format!("{dir}/{rel}"))
}

/// A local package's version, from its `package.json`.
pub fn local_version(root: &str, package: &str) -> Option<String> {
    let text = std::fs::read_to_string(Path::new(root).join("node_modules").join(package).join("package.json")).ok()?;
    serde_json::from_str::<Value>(&text).ok()?["version"].as_str().map(str::to_string)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-web-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub const PACKAGE: &str = r#"{
  "name": "orbit-web",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "packageManager": "pnpm@9.12.0+sha512.4abf725084d7bcbafbd728bfc7bee61f2f791f977fd87542b3579dcb23504d170d46337945e4c66485cd12d588a0c0e570ed9c477e7ccdd8507cf05f3f92eaca",
  "engines": { "node": ">=20" },
  "scripts": { "dev": "vite", "build": "tsc -b && vite build", "test": "vitest", "lint": "eslint ." },
  "dependencies": { "react": "^19.0.0", "lodash": "4.17.20" },
  "devDependencies": { "vite": "^7.0.0", "vitest": "^3.0.0", "typescript": "~5.9.0", "eslint": "^9.0.0", "tailwindcss": "^4.0.0" },
  "peerDependencies": { "react-dom": "^19.0.0" }
}"#;

    #[test]
    fn package_json_says_its_scripts_groups_and_tools() {
        let p = Project::parse(PACKAGE, &["pnpm-lock.yaml", "package-lock.json", "biome.json"]).unwrap();
        assert_eq!(p.name.as_deref(), Some("orbit-web"));
        assert_eq!(p.scripts.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["build", "dev", "lint", "test"]);
        assert_eq!(p.group_of("react"), Some(None));
        assert_eq!(p.group_of("vitest"), Some(Some("dev")));
        assert_eq!(p.group_of("react-dom"), Some(Some("peer")));
        assert_eq!(p.group_of("left-pad"), None);
        assert_eq!(p.engines_node.as_deref(), Some(">=20"));
        assert_eq!(p.package_manager, Some(("pnpm".into(), "9.12.0".into())));
        assert_eq!((p.manager, p.lockfile.as_deref()), (Manager::Pnpm, Some("pnpm-lock.yaml")), "pnpm's lockfile first");
        assert!(p.typescript && p.eslint && p.tailwind && p.vite && p.vitest && p.biome && !p.prettier && !p.next && !p.jest);
        assert!(Project::parse("{", &[]).unwrap_err().starts_with("package.json: "));
    }

    #[test]
    fn the_manager_is_the_lockfiles_else_packagemanagers() {
        let m = |json: &str, files: &[&str]| Project::parse(json, files).unwrap().manager;
        assert_eq!(m("{}", &[]), Manager::Npm);
        assert_eq!(m("{}", &["yarn.lock"]), Manager::Yarn);
        assert_eq!(m("{}", &["yarn.lock", ".yarnrc.yml"]), Manager::Berry);
        assert_eq!(m("{}", &["bun.lockb"]), Manager::Bun);
        assert_eq!(m(r#"{"packageManager": "yarn@4.9.2"}"#, &[]), Manager::Berry);
        assert_eq!(m(r#"{"packageManager": "yarn@1.22.22"}"#, &[]), Manager::Yarn);
        assert_eq!(m(r#"{"packageManager": "bun@1.2.0"}"#, &["package-lock.json"]), Manager::Npm, "the lockfile wins");
        assert_eq!(package_manager("@scope/pm@1.0.0"), Some(("@scope/pm".into(), "1.0.0".into())));
        assert_eq!(package_manager("pnpm"), None);
    }

    #[test]
    fn nvmrc_is_a_version_or_an_lts_line() {
        assert_eq!(nvm_version("v20.11.0\n").as_deref(), Some("20.11.0"));
        assert_eq!(nvm_version("# pinned\n22\n").as_deref(), Some("22"));
        assert_eq!(nvm_version("lts/Iron").as_deref(), Some("20"));
        assert_eq!(nvm_version("lts/*"), None);
        assert_eq!(nvm_version("node"), None);
        let p = Project { node_file: Some((".nvmrc".into(), "20.11.0".into())), engines_node: Some(">=18".into()), ..Project::default() };
        assert_eq!(p.node_wants(), Some(("^20.11.0".into(), ".nvmrc".into())), "the pin, then any later 20");
        let p = Project { engines_node: Some(">=18".into()), ..Project::default() };
        assert_eq!(p.node_wants(), Some((">=18".into(), "engines.node".into())));
    }

    #[test]
    fn a_yarn_berry_lockfile_and_a_local_bin_are_read() {
        let dir = temp("read");
        std::fs::write(dir.join("package.json"), r#"{"name": "x", "devDependencies": {"eslint": "^9"}}"#).unwrap();
        std::fs::write(dir.join("yarn.lock"), "# This file is generated by running \"yarn install\"\n\n__metadata:\n  version: 8\n").unwrap();
        std::fs::write(dir.join(".node-version"), "22.11.0\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/eslint/bin")).unwrap();
        std::fs::write(dir.join("node_modules/eslint/package.json"), r#"{"version": "9.39.1", "bin": {"eslint": "./bin/eslint.js"}}"#).unwrap();
        std::fs::write(dir.join("node_modules/eslint/bin/eslint.js"), "").unwrap();
        let root = dir.to_string_lossy();
        let p = Project::read(&root);
        assert_eq!(p.manager, Manager::Berry);
        assert_eq!(p.node_file, Some((".node-version".into(), "22.11.0".into())));
        assert_eq!(local_bin(&root, "eslint", "eslint").as_deref(), Some("node_modules/eslint/bin/eslint.js"));
        assert_eq!(local_version(&root, "eslint").as_deref(), Some("9.39.1"));
        assert_eq!(local_bin(&root, "prettier", "prettier"), None);
        assert!(!Project::read(&dir.join("nothing").to_string_lossy()).package_json);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
