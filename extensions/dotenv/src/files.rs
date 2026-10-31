//! Which files are env files, which of them are examples, and finding
//! them in the project.

use std::path::Path;

/// Names ending so are code or data about env files, not one:
/// `.env.ts`, `.env.json`, an editor's swap file.
const NOT_ENV: &[&str] = &[".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".json", ".py", ".rs", ".go", ".rb", ".php", ".yaml", ".yml", ".toml", ".md", ".sh", ".swp", ".vault"];

/// Folders not looked in: what's built, fetched or Git's.
const SKIP: &[&str] = &[".git", ".hg", ".svn", "node_modules", "target", "dist", "build", "out", "vendor", ".venv", "venv", "__pycache__", ".next", ".nuxt", ".svelte-kit", ".turbo", "coverage", ".cache", ".gradle"];

/// The deepest folder looked in, below the project's.
const DEPTH: usize = 6;

/// The most entries looked at, in a project that's very big.
const MOST: usize = 50_000;

/// Whether a file called `name` is an env file: `.env`, `.env.<anything>`,
/// `<anything>.env`.
pub fn is_env(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if NOT_ENV.iter().any(|x| lower.ends_with(x)) {
        return false;
    }
    lower == ".env" || lower.starts_with(".env.") || (lower.ends_with(".env") && lower.len() > 4)
}

/// Whether env file `name` shows what's needed rather than holding it:
/// `.env.example`, `.env.sample`, `.env.template`, `.env.dist`.
pub fn is_example(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["example", "sample", "template", "dist"].iter().any(|w| lower.split('.').any(|part| part == *w))
}

/// The file's name: what's after its last `/`.
pub fn name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Its folder, "" at the project's top.
pub fn folder(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// What profile env file `name` is: `.env.staging` and `staging.env` are
/// `staging`; `.env` is none.
pub fn profile(name: &str) -> Option<&str> {
    name.strip_prefix(".env.").or_else(|| name.strip_suffix(".env").filter(|s| !s.is_empty() && *s != "."))
}

/// The example env file `rel` is checked against: one in the same folder,
/// `.env.example` before `.env.sample`, `.env.template`, `.env.dist`, then
/// any other. None for an example itself.
pub fn example_for<'a>(rel: &str, all: &'a [String]) -> Option<&'a str> {
    if is_example(name(rel)) {
        return None;
    }
    let dir = folder(rel);
    let rank = |n: &str| [".env.example", ".env.sample", ".env.template", ".env.dist"].iter().position(|x| x.eq_ignore_ascii_case(n)).unwrap_or(4);
    all.iter().filter(|f| folder(f) == dir && is_example(name(f))).min_by_key(|f| (rank(name(f)), f.as_str())).map(String::as_str)
}

/// The order they're listed in: by folder, `.env` first, examples last.
pub fn sort(files: &mut [String]) {
    files.sort_by(|a, b| {
        let key = |f: &String| (folder(f).to_string(), name(f) != ".env", is_example(name(f)), name(f).to_ascii_lowercase());
        key(a).cmp(&key(b))
    });
}

/// The env files under `root`, as paths below it with `/`.
pub fn find(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut seen = 0;
    let mut stack = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MOST {
                break;
            }
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() { file_name.clone() } else { format!("{rel}/{file_name}") };
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth < DEPTH && !SKIP.contains(&file_name.as_str()) {
                    stack.push((entry.path(), path, depth + 1));
                }
            } else if is_env(&file_name) {
                found.push(path);
            }
        }
    }
    sort(&mut found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_files_by_name() {
        for yes in [".env", ".env.local", ".env.staging", ".ENV.Production", "prod.env", "docker.env", ".env.example", ".env.bak"] {
            assert!(is_env(yes), "{yes}");
        }
        for no in [".envrc", "env", ".env.ts", "env.d.ts", ".env.json", "environment.py", "a.envx", ".env.swp", ".en", ".env.d.ts"] {
            assert!(!is_env(no), "{no}");
        }
        assert!(is_example(".env.example") && is_example("example.env") && is_example(".env.sample") && is_example(".env.dist"));
        assert!(!is_example(".env") && !is_example(".env.production") && !is_example(".env.examples-of-x"));
        assert_eq!(profile(".env.staging"), Some("staging"));
        assert_eq!(profile("prod.env"), Some("prod"));
        assert_eq!(profile(".env"), None);
    }

    #[test]
    fn examples_are_found_beside() {
        let all: Vec<String> = ["api/.env", "api/.env.template", "api/.env.example", ".env", ".env.local", "web/.env"].map(String::from).to_vec();
        assert_eq!(example_for("api/.env", &all), Some("api/.env.example"));
        assert_eq!(example_for(".env", &all), None, "none beside it");
        assert_eq!(example_for("api/.env.example", &all), None);
        let mut sorted = all.clone();
        sort(&mut sorted);
        assert_eq!(sorted, [".env", ".env.local", "api/.env", "api/.env.example", "api/.env.template", "web/.env"]);
    }

    #[test]
    fn found_in_a_project_but_not_in_what_is_built() {
        let dir = std::env::temp_dir().join(format!("fenix-dotenv-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in [".env", ".env.example", "app/.env.local", "node_modules/x/.env", ".git/.env", "src/main.rs"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "A=1\n").unwrap();
        }
        assert_eq!(find(&dir), [".env", ".env.example", "app/.env.local"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
