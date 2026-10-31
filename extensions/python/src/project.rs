//! What the project is, as its files say: `pyproject.toml` (its name, the
//! Python it asks for, its dependencies and their groups, its scripts),
//! whether uv manages it (`uv.lock`, `[tool.uv]`), the requirements files
//! of one that isn't, and what there is to run.

use std::path::Path;

use fenix_lang_kit::packages::python_name;

/// One of the project's own dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// In PEP 503's form: `typing-extensions`.
    pub name: String,
    /// `None` for `[project].dependencies`; a dependency group's name
    /// (`dev`, `docs`); an extra's in brackets (`[web]`).
    pub group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `[project].name`.
    pub name: Option<String>,
    /// `[project].requires-python`, as uv and pip read it.
    pub requires_python: Option<String>,
    pub pyproject: bool,
    /// uv manages it: there's a `uv.lock`, or a `[tool.uv]`.
    pub uv: bool,
    pub locked: bool,
    pub deps: Vec<Dep>,
    /// `[project.scripts]`' names, in order.
    pub scripts: Vec<String>,
    /// What each runs, by its name: `orbit = "orbit.cli:main"`.
    pub script_targets: Vec<(String, String)>,
    /// A package with a `__main__.py` (`src/<name>/` or `<name>/`): run
    /// with `python -m`.
    pub main_module: Option<String>,
    /// Django's `manage.py`.
    pub django: bool,
    /// `main.py` or `app.py`, at the top.
    pub entry: Option<String>,
}

/// A PEP 508 requirement's name: `requests[socks]>=2; python_version <
/// "3.13"` is `requests`.
pub fn requirement_name(req: &str) -> Option<String> {
    let req = req.trim();
    let end = req.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(req.len());
    let name = &req[..end];
    (!name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())).then(|| python_name(name))
}

/// A requirements file's names: options (`-r`, `-e`, `--index-url`),
/// comments and URLs without a name left out.
pub fn requirements(text: &str) -> Vec<String> {
    text.lines().map(|l| l.split(" #").next().unwrap_or(l).trim()).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-')).filter_map(requirement_name).collect()
}

fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

impl Project {
    /// From `pyproject.toml`'s text alone.
    pub fn parse(pyproject: &str) -> Result<Project, String> {
        let doc: toml::Table = toml::from_str(pyproject).map_err(|e| format!("pyproject.toml: {}", e.message()))?;
        let mut p = Project { pyproject: true, ..Project::default() };
        let project = doc.get("project").and_then(toml::Value::as_table);
        let tool_uv = doc.get("tool").and_then(|t| t.get("uv"));
        p.uv = tool_uv.is_some();
        if let Some(project) = project {
            p.name = project.get("name").and_then(toml::Value::as_str).map(str::to_string);
            p.requires_python = project.get("requires-python").and_then(toml::Value::as_str).map(str::to_string);
            p.add(None, &strings(project.get("dependencies")));
            if let Some(extras) = project.get("optional-dependencies").and_then(toml::Value::as_table) {
                for (extra, reqs) in extras {
                    p.add(Some(format!("[{extra}]")), &strings(Some(reqs)));
                }
            }
            if let Some(scripts) = project.get("scripts").and_then(toml::Value::as_table) {
                p.scripts = scripts.keys().cloned().collect();
                p.script_targets = scripts.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.trim().to_string()))).collect();
            }
        }
        // PEP 735's groups; an `{include-group = ...}` is the other's.
        if let Some(groups) = doc.get("dependency-groups").and_then(toml::Value::as_table) {
            for (group, reqs) in groups {
                p.add(Some(group.clone()), &strings(Some(reqs)));
            }
        }
        // uv's older way of saying dev dependencies.
        p.add(Some("dev".into()), &strings(tool_uv.and_then(|u| u.get("dev-dependencies"))));
        Ok(p)
    }

    fn add(&mut self, group: Option<String>, reqs: &[String]) {
        for name in reqs.iter().filter_map(|r| requirement_name(r)) {
            if !self.deps.iter().any(|d| d.name == name && d.group == group) {
                self.deps.push(Dep { name, group: group.clone() });
            }
        }
    }

    /// The project in folder `root` (`/project`). One with no
    /// `pyproject.toml`, or one that doesn't read, is what its other files
    /// say.
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let mut p = text("pyproject.toml").and_then(|t| Project::parse(&t).ok()).unwrap_or_default();
        p.pyproject |= root.join("pyproject.toml").is_file();
        p.locked = root.join("uv.lock").is_file();
        p.uv |= p.locked;
        if p.deps.is_empty() {
            for (file, group) in [("requirements.txt", None), ("requirements-dev.txt", Some("dev")), ("dev-requirements.txt", Some("dev"))] {
                if let Some(t) = text(file) {
                    p.add(group.map(str::to_string), &requirements(&t));
                }
            }
        }
        p.django = root.join("manage.py").is_file();
        p.entry = ["main.py", "app.py"].into_iter().find(|f| root.join(f).is_file()).map(str::to_string);
        p.main_module = p.find_main_module(root);
        p
    }

    /// The package with a `__main__.py`: the project's own name first,
    /// then any, under `src/` or at the top.
    fn find_main_module(&self, root: &Path) -> Option<String> {
        let own = self.name.as_deref().map(|n| python_name(n).replace('-', "_"));
        let mut found = Vec::new();
        for base in [root.join("src"), root.to_path_buf()] {
            let Ok(entries) = std::fs::read_dir(&base) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') && e.path().join("__main__.py").is_file() {
                    found.push(name);
                }
            }
        }
        found.sort();
        own.filter(|o| found.contains(o)).or_else(|| found.into_iter().next())
    }

    /// Whether the project asks for `name`, in any group.
    pub fn has(&self, name: &str) -> bool {
        let name = python_name(name);
        self.deps.iter().any(|d| d.name == name)
    }

    /// Its group, when the project asks for `name`: `Some(None)` for a
    /// main dependency.
    pub fn group_of(&self, name: &str) -> Option<Option<&str>> {
        let name = python_name(name);
        let mut groups = self.deps.iter().filter(|d| d.name == name).map(|d| d.group.as_deref());
        let first = groups.next()?;
        // A main dependency wins over the same in a group.
        Some(if first.is_none() { None } else { groups.find(Option::is_none).unwrap_or(first) })
    }

    /// The project's own package's name, as `pip list` gives it.
    pub fn own_name(&self) -> Option<String> {
        self.name.as_deref().map(python_name)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const PYPROJECT: &str = include_str!("samples/pyproject.toml");

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-python-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn requirement_names_are_read_as_pip_reads_them() {
        assert_eq!(requirement_name("requests[socks]>=2.31; python_version < \"3.13\"").as_deref(), Some("requests"));
        assert_eq!(requirement_name("Typing_Extensions").as_deref(), Some("typing-extensions"));
        assert_eq!(requirement_name("fastapi[standard] @ https://x/f.whl").as_deref(), Some("fastapi"));
        assert_eq!(requirement_name("  "), None);
        assert_eq!(requirements("# pinned\n-r base.txt\n-e .\nflask==3.0  # web\n--index-url https://x\nJinja2>=3\n"), ["flask", "jinja2"]);
    }

    #[test]
    fn pyproject_says_its_dependencies_and_their_groups() {
        let p = Project::parse(PYPROJECT).unwrap();
        assert_eq!(p.name.as_deref(), Some("orbit-api"));
        assert_eq!(p.requires_python.as_deref(), Some(">=3.11"));
        assert!(p.uv, "[tool.uv]");
        assert_eq!(p.scripts, ["orbit", "orbit-admin"]);
        let deps: Vec<(&str, Option<&str>)> = p.deps.iter().map(|d| (d.name.as_str(), d.group.as_deref())).collect();
        assert_eq!(
            deps,
            [
                ("fastapi", None),
                ("pydantic-settings", None),
                ("httpx", Some("[client]")),
                ("orjson", Some("[client]")),
                ("pytest", Some("dev")),
                ("ruff", Some("dev")),
                ("mypy", Some("dev")),
                ("mkdocs", Some("docs")),
                ("pytest-cov", Some("dev")),
            ]
        );
        assert_eq!(p.group_of("Pytest"), Some(Some("dev")));
        assert_eq!(p.group_of("fastapi"), Some(None));
        assert_eq!(p.group_of("uvicorn"), None);
        assert!(p.has("pydantic_settings") && !p.has("django"));
        assert!(Project::parse("[project\n").unwrap_err().starts_with("pyproject.toml: "));
    }

    #[test]
    fn a_folder_says_whether_uv_manages_it_and_what_runs() {
        let dir = temp("read");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(Project::read(&root), Project::default(), "nothing there");

        std::fs::write(dir.join("requirements.txt"), "flask\nrequests==2.31\n").unwrap();
        std::fs::write(dir.join("requirements-dev.txt"), "pytest\n").unwrap();
        std::fs::write(dir.join("app.py"), "").unwrap();
        let p = Project::read(&root);
        assert!(!p.uv && !p.pyproject);
        assert_eq!(p.deps.iter().map(|d| (d.name.as_str(), d.group.as_deref())).collect::<Vec<_>>(), [("flask", None), ("requests", None), ("pytest", Some("dev"))]);
        assert_eq!(p.entry.as_deref(), Some("app.py"));

        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"Star_Map\"\ndependencies = [\"numpy\"]\n").unwrap();
        std::fs::write(dir.join("uv.lock"), "version = 1\n").unwrap();
        std::fs::create_dir_all(dir.join("src/star_map")).unwrap();
        std::fs::write(dir.join("src/star_map/__main__.py"), "").unwrap();
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        std::fs::write(dir.join("tools/__main__.py"), "").unwrap();
        std::fs::write(dir.join("manage.py"), "").unwrap();
        let p = Project::read(&root);
        assert!(p.uv && p.locked && p.pyproject && p.django);
        assert_eq!(p.deps.len(), 1, "pyproject's, not the requirements files'");
        assert_eq!(p.main_module.as_deref(), Some("star_map"), "the project's own package first");
        assert_eq!(p.own_name().as_deref(), Some("star-map"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
