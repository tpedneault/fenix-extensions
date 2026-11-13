//! What a file is to pytest: the folder its node ids are relative to (its
//! rootdir, which is also the step's `cwd`), and what its configuration
//! says is a test (`python_files`, `python_classes`, `python_functions`).
//!
//! A key is a node id below the *project*: `tests/test_basic.py::test_adds`
//! when the rootdir is the project's, `services/api/tests/test_x.py::t` for
//! a rootdir of `services/api`. Scan, listing, run and launch all build it
//! with [`key`], and plans take it apart with [`split_key`].

use std::cell::RefCell;
use std::collections::HashMap;

/// The longest a file we read may be: configuration files are small.
const READ_MAX: u64 = 1 << 20;

/// What decides a file is a test, as pytest's options say (their defaults
/// when the project's configuration doesn't).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// `python_files`: globs of file names.
    pub files: Vec<String>,
    /// `python_classes`: prefixes or globs.
    pub classes: Vec<String>,
    /// `python_functions`: prefixes or globs.
    pub functions: Vec<String>,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { files: vec!["test_*.py".into(), "*_test.py".into()], classes: vec!["Test".into()], functions: vec!["test".into()] }
    }
}

/// `text` against a glob of `*` and `?`.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// pytest's `python_classes` and `python_functions`: a name starts with
/// the pattern, or matches it as a glob.
fn prefix_or_glob(pattern: &str, name: &str) -> bool {
    if pattern.contains(['*', '?']) {
        glob(pattern, name)
    } else {
        name.starts_with(pattern)
    }
}

impl Rules {
    /// Whether `path` (below its rootdir) is a test file.
    pub fn file(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.files.iter().any(|p| glob(p, name) || (p.contains('/') && glob(p, path)))
    }

    pub fn class(&self, name: &str) -> bool {
        self.classes.iter().any(|p| prefix_or_glob(p, name))
    }

    pub fn function(&self, name: &str) -> bool {
        self.functions.iter().any(|p| prefix_or_glob(p, name))
    }
}

/// A node id's key: below the project, as `root` (a folder below it, `""`
/// for the project itself) puts it.
pub fn key(root: &str, node: &str) -> String {
    if root.is_empty() {
        node.to_string()
    } else {
        format!("{root}/{node}")
    }
}

/// A key's file below the project, and what follows it (`::Class::name`,
/// `::name[case]`, or nothing for a file).
pub fn split_key(key: &str) -> (&str, &str) {
    match key.find("::") {
        Some(at) => key.split_at(at),
        None => (key, ""),
    }
}

/// What a folder says of pytest.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    /// pytest's own configuration: `pytest.ini`, `pyproject.toml` with its
    /// table, `tox.ini`'s `[pytest]`, `setup.cfg`'s `[tool:pytest]`: where
    /// its rootdir is, and what `python_files` and the like say.
    Config(Rules),
    /// `setup.py`: pytest's rootdir when there's no configuration.
    Setup,
    /// A project file with no pytest table: the rootdir when nothing else is.
    Weak,
    None,
}

/// The project's folders as pytest sees them, read once from the files
/// under `root` and again after the pack's `forget`.
#[derive(Debug, Default)]
pub struct Layout {
    /// The project's folder on this machine: `/project`, a test's own.
    root: String,
    folders: RefCell<HashMap<String, Marker>>,
}

/// `python_files = a b` lines of an ini file's section, and the same of a
/// TOML table.
fn rules_from_ini(text: &str, section: &str) -> Rules {
    let mut rules = Rules::default();
    let mut inside = false;
    let mut key: Option<String> = None;
    let mut values: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            inside = trimmed == section;
            key = None;
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(k) = &key {
                let v = values.entry(k.clone()).or_default();
                v.push(' ');
                v.push_str(trimmed);
            }
            continue;
        }
        match trimmed.split_once(['=', ':']) {
            Some((k, v)) => {
                let k = k.trim().to_string();
                values.insert(k.clone(), v.trim().to_string());
                key = Some(k);
            }
            None => key = None,
        }
    }
    let words = |k: &str| values.get(k).map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>()).filter(|w| !w.is_empty());
    rules.files = words("python_files").unwrap_or(rules.files);
    rules.classes = words("python_classes").unwrap_or(rules.classes);
    rules.functions = words("python_functions").unwrap_or(rules.functions);
    rules
}

fn rules_from_toml(table: &toml::Table) -> Rules {
    let mut rules = Rules::default();
    let pytest = table.get("tool").and_then(|t| t.get("pytest"));
    let options = pytest.and_then(|p| p.get("ini_options")).or(pytest);
    let words = |k: &str| {
        let v = options?.get(k)?;
        let words: Vec<String> = match v {
            toml::Value::String(s) => s.split_whitespace().map(str::to_string).collect(),
            toml::Value::Array(a) => a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        Some(words).filter(|w| !w.is_empty())
    };
    rules.files = words("python_files").unwrap_or(rules.files);
    rules.classes = words("python_classes").unwrap_or(rules.classes);
    rules.functions = words("python_functions").unwrap_or(rules.functions);
    rules
}

impl Layout {
    pub fn new(root: &str) -> Layout {
        Layout { root: root.to_string(), folders: RefCell::default() }
    }

    /// What was read is read again: a configuration file was saved.
    pub fn forget(&self) {
        self.folders.borrow_mut().clear();
    }

    fn read(&self, dir: &str, name: &str) -> Option<String> {
        let path = if dir.is_empty() { format!("{}/{name}", self.root) } else { format!("{}/{dir}/{name}", self.root) };
        std::fs::metadata(&path).ok().filter(|m| m.is_file() && m.len() <= READ_MAX)?;
        std::fs::read_to_string(path).ok()
    }

    fn exists(&self, dir: &str, name: &str) -> bool {
        let path = if dir.is_empty() { format!("{}/{name}", self.root) } else { format!("{}/{dir}/{name}", self.root) };
        std::path::Path::new(&path).is_file()
    }

    /// What folder `dir` (below the project) says, in pytest's own order:
    /// `pytest.ini`, then `pyproject.toml`, `tox.ini` and `setup.cfg` that
    /// have its section.
    fn marker(&self, dir: &str) -> Marker {
        if let Some(m) = self.folders.borrow().get(dir) {
            return m.clone();
        }
        let found = if let Some(text) = self.read(dir, "pytest.ini") {
            Marker::Config(rules_from_ini(&text, "[pytest]"))
        } else if let Some(table) = self.read(dir, "pyproject.toml").filter(|t| t.contains("[tool.pytest")).and_then(|t| toml::from_str::<toml::Table>(&t).ok()) {
            Marker::Config(rules_from_toml(&table))
        } else if let Some(text) = self.read(dir, "tox.ini").filter(|t| t.contains("[pytest]")) {
            Marker::Config(rules_from_ini(&text, "[pytest]"))
        } else if let Some(text) = self.read(dir, "setup.cfg").filter(|t| t.contains("[tool:pytest]")) {
            Marker::Config(rules_from_ini(&text, "[tool:pytest]"))
        } else if self.exists(dir, "setup.py") {
            Marker::Setup
        } else if ["pyproject.toml", "tox.ini", "setup.cfg"].iter().any(|n| self.exists(dir, n)) {
            Marker::Weak
        } else {
            Marker::None
        };
        self.folders.borrow_mut().insert(dir.to_string(), found.clone());
        found
    }

    /// `file`'s folder and each folder above it, below the project, nearest
    /// first, ending with the project's own (`""`).
    fn ancestors(file: &str) -> Vec<String> {
        let mut dirs = Vec::new();
        let mut dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        while !dir.is_empty() {
            dirs.push(dir.to_string());
            dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        }
        dirs.push(String::new());
        dirs
    }

    /// The folder (below the project; `""` for the project) pytest's node
    /// ids for `file` are relative to: the nearest one with its
    /// configuration; else the nearest with a `setup.py`; else the nearest
    /// with a `pyproject.toml` and the like; else the project's.
    pub fn root_of(&self, file: &str) -> String {
        let dirs = Layout::ancestors(file);
        for want in [0, 1, 2] {
            let hit = dirs.iter().find(|d| matches!((self.marker(d), want), (Marker::Config(_), 0) | (Marker::Setup, 1) | (Marker::Weak, 2)));
            if let Some(d) = hit {
                return d.clone();
            }
        }
        String::new()
    }

    /// What `root`'s configuration says makes a test.
    pub fn rules(&self, root: &str) -> Rules {
        match self.marker(root) {
            Marker::Config(r) => r,
            _ => Rules::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_is_fnmatchs() {
        assert!(glob("test_*.py", "test_basic.py") && glob("*_test.py", "basic_test.py"));
        assert!(!glob("test_*.py", "basic.py") && !glob("test_*.py", "test_basic.pyc"));
        assert!(glob("t?sts.py", "tests.py") && glob("*", "") && !glob("?", ""));
        assert!(glob("a*b*c", "aXXbYYc") && !glob("a*b*c", "aXXbYY"));
    }

    #[test]
    fn the_options_default_and_are_read_from_the_ini_and_the_toml() {
        let r = Rules::default();
        assert!(r.file("tests/test_a.py") && r.file("a_test.py") && !r.file("tests.py") && !r.file("conftest.py"));
        assert!(r.class("TestA") && r.class("Test") && !r.class("Suite") && r.function("test_x") && r.function("testing") && !r.function("check_x"));
        let ini = "[tool:pytest]\npython_files = tests.py test_*.py\n    *_tests.py\npython_classes = *Suite\npython_functions = check_*\nother = x\n[flake8]\npython_files = no\n";
        let r = rules_from_ini(ini, "[tool:pytest]");
        assert_eq!(r.files, ["tests.py", "test_*.py", "*_tests.py"]);
        assert!(r.class("AdderSuite") && !r.class("TestA") && r.function("check_it") && !r.function("test_it"));
        let toml = "[tool.pytest.ini_options]\npython_files = [\"check_*.py\"]\npython_classes = \"Check\"\n";
        let r = rules_from_toml(&toml::from_str(toml).unwrap());
        assert_eq!((r.files.as_slice(), r.classes.as_slice()), (["check_*.py".to_string()].as_slice(), ["Check".to_string()].as_slice()));
        let native = rules_from_toml(&toml::from_str("[tool.pytest]\npython_functions = [\"it_\"]\n").unwrap());
        assert_eq!(native.functions, ["it_"], "pytest 9's own table");
    }

    #[test]
    fn keys_are_below_the_project_and_split_at_the_file() {
        assert_eq!(key("", "tests/test_a.py::t"), "tests/test_a.py::t");
        assert_eq!(key("services/api", "tests/test_a.py::t"), "services/api/tests/test_a.py::t");
        assert_eq!(split_key("a/test_b.py::TestC::test_d[x::y]"), ("a/test_b.py", "::TestC::test_d[x::y]"));
        assert_eq!(split_key("a/test_b.py"), ("a/test_b.py", ""));
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-python-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_files_rootdir_is_the_nearest_folder_pytest_would_find() {
        let dir = temp("roots");
        let at = |p: &str, text: &str| {
            std::fs::create_dir_all(dir.join(p).parent().unwrap()).unwrap();
            std::fs::write(dir.join(p), text).unwrap();
        };
        at("pyproject.toml", "[project]\nname = \"x\"\n");
        at("services/api/pytest.ini", "[pytest]\npython_files = check_*.py\n");
        at("libs/core/setup.py", "");
        at("libs/util/pyproject.toml", "[project]\nname = \"u\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n");
        let layout = Layout::new(&dir.to_string_lossy());
        assert_eq!(layout.root_of("tests/test_a.py"), "", "the project's, which has only a pyproject.toml with no pytest table");
        assert_eq!(layout.root_of("services/api/tests/test_a.py"), "services/api");
        assert_eq!(layout.root_of("libs/util/tests/sub/test_u.py"), "libs/util");
        assert_eq!(layout.root_of("libs/core/tests/test_c.py"), "libs/core", "setup.py: pytest's rootdir without configuration");
        assert!(layout.rules("services/api").file("check_x.py") && !layout.rules("services/api").file("test_x.py"));
        assert_eq!(layout.rules(""), Rules::default());
        // Configuration above beats a setup.py nearer: pytest looks for
        // configuration first. Read again after `forget`, not before.
        at("libs/pytest.ini", "");
        assert_eq!(layout.root_of("libs/core/tests/test_c.py"), "libs/core");
        layout.forget();
        assert_eq!(layout.root_of("libs/core/tests/test_c.py"), "libs");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
