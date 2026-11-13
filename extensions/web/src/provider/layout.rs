//! What the project's test runners are: each package folder (the
//! project's, and each workspace package's) whose `package.json` or
//! config says Vitest or Jest, and where its runner is installed; what a
//! test file is; and the keys, the same in scan, listing, run and launch.
//!
//! | | |
//! |---|---|
//! | Key | `<file below the project> > <names joined by " > ">`: `src/orbit.test.js > outer > inner > deep`; a repeated name's second and later get ` (2)`, ` (3)` (plan §4.4) |
//! | Groups | the package (`packages/app (vitest)`, only when there are two or more), the file (`src/orbit.test.js`), each `describe` |
//! | `data` | the names as JSON, when the key can't say them: a name holding ` > `, a repeated name |

use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::tests::Group;

/// The runner of a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Runner {
    Vitest,
    Jest,
}

impl Runner {
    /// What Fenix is told: `Vitest 5.1.0`.
    pub fn name(self) -> &'static str {
        match self {
            Runner::Vitest => "Vitest",
            Runner::Jest => "Jest",
        }
    }

    /// Its package's name, and its bin as `yarn <it>` runs it.
    pub fn id(self) -> &'static str {
        match self {
            Runner::Vitest => "vitest",
            Runner::Jest => "jest",
        }
    }

    /// Its program below `node_modules`, run by Node: what the launch
    /// files run (`launch/vitest-file.toml`, `jest-file.toml`).
    pub fn script(self) -> &'static str {
        match self {
            Runner::Vitest => "vitest/vitest.mjs",
            Runner::Jest => "jest/bin/jest.js",
        }
    }

    /// What `-t` joins a test's names with (plan §3).
    pub fn joiner(self) -> &'static str {
        match self {
            Runner::Vitest => " > ",
            Runner::Jest => " ",
        }
    }

    /// Its reporter, one of [`crate::provider::assets`].
    pub fn reporter(self) -> &'static str {
        match self {
            Runner::Vitest => "fenix-vitest.mjs",
            Runner::Jest => "fenix-jest.js",
        }
    }
}

/// A package's runner as installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Its program, relative to the package's folder:
    /// `node_modules/vitest/vitest.mjs`, or a workspace's hoisted one,
    /// `../../node_modules/vitest/vitest.mjs`.
    pub script: String,
    pub version: Option<String>,
}

/// A folder whose tests one runner runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Below the project, `/`-separated; "" for the project's own.
    pub dir: String,
    pub runner: Runner,
    pub installed: Option<Installed>,
}

impl Package {
    /// Its group's label: `packages/app (vitest)`, `. (jest)`.
    pub fn label(&self) -> String {
        format!("{} ({})", if self.dir.is_empty() { "." } else { &self.dir }, self.runner.id())
    }

    /// A step's folder.
    pub fn cwd(&self) -> String {
        if self.dir.is_empty() {
            crate::PROJECT.to_string()
        } else {
            format!("{}/{}", crate::PROJECT, self.dir)
        }
    }

    /// `file` (below the project) below this package's folder.
    pub fn below<'a>(&self, file: &'a str) -> &'a str {
        if self.dir.is_empty() {
            file
        } else {
            file.strip_prefix(&self.dir).and_then(|r| r.strip_prefix('/')).unwrap_or(file)
        }
    }

    /// Whether `file` (below the project) is in its folder.
    pub fn holds(&self, file: &str) -> bool {
        self.dir.is_empty() || file.strip_prefix(&self.dir).is_some_and(|r| r.starts_with('/'))
    }
}

/// The project's runners.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub packages: Vec<Package>,
    /// Yarn Plug'n'Play: `.pnp.cjs`, and no `node_modules` to run from;
    /// the runners are run as `yarn vitest`, `yarn jest`.
    pub pnp: bool,
}

/// Config files that say a runner, by the start of their names.
const VITEST_FILES: &[&str] = &["vitest.config.", "vitest.workspace.", "vitest.projects."];
const JEST_FILES: &[&str] = &["jest.config."];

/// The extensions a test file has.
const EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

impl Layout {
    /// The package `file` (below the project) is in: the deepest whose
    /// folder holds it.
    pub fn package_of(&self, file: &str) -> Option<usize> {
        self.packages.iter().enumerate().filter(|(_, p)| p.holds(file)).max_by_key(|(_, p)| p.dir.len()).map(|(i, _)| i)
    }

    /// The package whose step's folder is `cwd` (`/project/packages/app`).
    pub fn package_at(&self, cwd: &str) -> Option<usize> {
        let dir = cwd.strip_prefix(crate::PROJECT)?.trim_start_matches('/');
        self.packages.iter().position(|p| p.dir == dir)
    }

    /// Whether the page has a group for each package: two or more.
    pub fn grouped(&self) -> bool {
        self.packages.len() > 1
    }

    /// A test's groups before its file's: its package's, when grouped.
    pub fn base(&self, package: usize) -> Vec<Group> {
        match self.packages.get(package) {
            Some(p) if self.grouped() => vec![Group::package(p.label())],
            _ => Vec::new(),
        }
    }

    /// The project in `root` (`/project` in Fenix), read with `std::fs`:
    /// its `package.json`, its workspaces (`workspaces`, or
    /// `pnpm-workspace.yaml`'s `packages`), and each one's runner.
    pub fn read(root: &str) -> Layout {
        let root = Path::new(root);
        let Some(top) = read_json(&root.join("package.json")) else { return Layout::default() };
        let mut dirs = vec![String::new()];
        for pattern in workspaces(&top, &std::fs::read_to_string(root.join("pnpm-workspace.yaml")).unwrap_or_default()) {
            let (exclude, pattern) = match pattern.strip_prefix('!') {
                Some(p) => (true, p.to_string()),
                None => (false, pattern),
            };
            let found = expand(root, &pattern);
            if exclude {
                dirs.retain(|d| !found.contains(d));
            } else {
                for d in found {
                    if !dirs.contains(&d) {
                        dirs.push(d);
                    }
                }
            }
        }
        let mut packages = Vec::new();
        // A root that runs its packages as `projects` runs them all.
        let mut covered: Vec<Runner> = Vec::new();
        for dir in &dirs {
            let folder = if dir.is_empty() { root.to_path_buf() } else { root.join(dir) };
            let doc = if dir.is_empty() { Some(top.clone()) } else { read_json(&folder.join("package.json")) };
            let Some(doc) = doc else { continue };
            let names = names_in(&folder);
            let Some(runner) = runner_of(&doc, &names) else { continue };
            if dir.is_empty() {
                if runs_projects(runner, &doc, &folder, &names) {
                    covered.push(runner);
                }
            } else if covered.contains(&runner) {
                continue;
            }
            let installed = installed(root, dir, runner);
            packages.push(Package { dir: dir.clone(), runner, installed });
        }
        let pnp = root.join(".pnp.cjs").is_file() && !root.join("node_modules").is_dir();
        Layout { packages, pnp }
    }

    /// What Fenix is told: `Vitest 5.1.0`, `Vitest 5.1.0 and Jest 30.5.2`;
    /// "" for none.
    pub fn runners(&self) -> String {
        let mut said: Vec<String> = Vec::new();
        for p in &self.packages {
            let s = match p.installed.as_ref().and_then(|i| i.version.as_deref()) {
                Some(v) => format!("{} {v}", p.runner.name()),
                None => p.runner.name().to_string(),
            };
            if !said.contains(&s) {
                said.push(s);
            }
        }
        match said.len() {
            0 => String::new(),
            1 => said.remove(0),
            _ => {
                let last = said.pop().expect("two");
                format!("{} and {last}", said.join(", "))
            }
        }
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
}

/// The workspace patterns: `package.json`'s `workspaces` (a list, or
/// `{ "packages": [..] }`), and `pnpm-workspace.yaml`'s `packages:` list.
pub fn workspaces(package_json: &Value, pnpm_workspace: &str) -> Vec<String> {
    let list = match &package_json["workspaces"] {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.get("packages").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut out: Vec<String> = list.iter().filter_map(Value::as_str).map(str::to_string).collect();
    let mut inside = false;
    for line in pnpm_workspace.lines() {
        let t = line.trim_end();
        if !t.starts_with([' ', '\t', '-']) && !t.is_empty() {
            inside = t.trim() == "packages:";
            continue;
        }
        if let Some(item) = t.trim_start().strip_prefix('-').filter(|_| inside) {
            let item = item.split(" #").next().unwrap_or(item).trim().trim_matches(|c| c == '"' || c == '\'');
            if !item.is_empty() {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// A workspace pattern's folders below `root` that have a `package.json`:
/// `packages/*`, `apps/**`, `tools/cli`. Never `node_modules` or a
/// hidden folder.
pub fn expand(root: &Path, pattern: &str) -> Vec<String> {
    let pattern = pattern.trim().trim_start_matches("./").trim_end_matches('/');
    let parts: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let mut out = Vec::new();
    walk(root, "", &parts, 0, &mut out);
    out.retain(|d| !d.is_empty() && root.join(d).join("package.json").is_file());
    out.sort();
    out.dedup();
    out
}

fn walk(root: &Path, at: &str, parts: &[&str], depth: usize, out: &mut Vec<String>) {
    let Some((first, rest)) = parts.split_first() else {
        out.push(at.to_string());
        return;
    };
    let join = |name: &str| if at.is_empty() { name.to_string() } else { format!("{at}/{name}") };
    if *first == "**" {
        walk(root, at, rest, depth, out);
        if depth < 6 {
            for name in subdirs(&root.join(at)) {
                walk(root, &join(&name), parts, depth + 1, out);
            }
        }
    } else if first.contains('*') {
        for name in subdirs(&root.join(at)) {
            if glob(first, &name) {
                walk(root, &join(&name), rest, depth + 1, out);
            }
        }
    } else if root.join(at).join(first).is_dir() {
        walk(root, &join(first), rest, depth + 1, out);
    }
}

fn subdirs(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    out.retain(|n| n != "node_modules" && !n.starts_with('.'));
    out.sort();
    out
}

/// `*` in one name.
fn glob(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = name.len() - last.len();
    for p in &parts[1..parts.len() - 1] {
        match name[at..end].find(p) {
            Some(i) => at += i + p.len(),
            None => return false,
        }
    }
    true
}

/// The runner a package's `package.json` and files say: Vitest or Jest
/// in its dependencies, or their config files (`vitest.config.ts`,
/// `jest.config.js`, `package.json`'s `jest`). With both, the one with a
/// config, else the one its `test` script runs, else Vitest.
pub fn runner_of(doc: &Value, names: &[String]) -> Option<Runner> {
    let has = |dep: &str| ["dependencies", "devDependencies", "optionalDependencies"].iter().any(|g| doc[g].get(dep).is_some());
    let file = |starts: &[&str]| names.iter().any(|n| starts.iter().any(|s| n.starts_with(s)));
    let (vitest_config, jest_config) = (file(VITEST_FILES), file(JEST_FILES) || doc["jest"].is_object());
    let vitest = has("vitest") || vitest_config;
    let jest = has("jest") || jest_config;
    match (vitest, jest) {
        (true, false) => Some(Runner::Vitest),
        (false, true) => Some(Runner::Jest),
        (false, false) => None,
        (true, true) => {
            let script = doc["scripts"]["test"].as_str().unwrap_or("");
            if (jest_config && !vitest_config) || (!vitest_config && script.contains("jest") && !script.contains("vitest")) {
                Some(Runner::Jest)
            } else {
                Some(Runner::Vitest)
            }
        }
    }
}

/// Whether the project's own config runs its packages as `projects`
/// (Vitest's `test.projects` or a `vitest.workspace` file; Jest's
/// `projects`): then their tests are its, and they're no packages of
/// their own.
fn runs_projects(runner: Runner, doc: &Value, folder: &Path, names: &[String]) -> bool {
    let configs: Vec<&String> = names.iter().filter(|n| (if runner == Runner::Vitest { VITEST_FILES } else { JEST_FILES }).iter().any(|s| n.starts_with(s))).collect();
    if runner == Runner::Vitest && configs.iter().any(|n| n.starts_with("vitest.workspace.")) {
        return true;
    }
    if runner == Runner::Jest && doc["jest"]["projects"].is_array() {
        return true;
    }
    configs.iter().any(|n| std::fs::read_to_string(folder.join(n)).is_ok_and(|t| t.contains("projects")))
}

/// Where Node finds `runner` from package `dir`: its own `node_modules`,
/// else a folder above it, up to the project's (a workspace's hoisted
/// one).
fn installed(root: &Path, dir: &str, runner: Runner) -> Option<Installed> {
    let segments: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for up in 0..=segments.len() {
        let at = segments[..segments.len() - up].join("/");
        let modules = if at.is_empty() { root.join("node_modules") } else { root.join(&at).join("node_modules") };
        if modules.join(runner.script()).is_file() {
            let version = read_json(&modules.join(runner.id()).join("package.json")).and_then(|d| d["version"].as_str().map(str::to_string));
            return Some(Installed { script: format!("{}node_modules/{}", "../".repeat(up), runner.script()), version });
        }
    }
    None
}

/// Whether `file` (below the project) is a test file `runner` runs by
/// default: `*.test.*` and `*.spec.*` (Vitest's `**/*.{test,spec}.?(c|m)[jt]s?(x)`),
/// and for Jest also anything under `__tests__`. Never in `node_modules`.
pub fn is_test_file(file: &str, runner: Runner) -> bool {
    if file.split('/').any(|s| s == "node_modules") {
        return false;
    }
    let name = file.rsplit('/').next().unwrap_or(file);
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !EXTENSIONS.contains(&ext) {
        return false;
    }
    if stem.ends_with(".test") || stem.ends_with(".spec") {
        return true;
    }
    runner == Runner::Jest && ["js", "jsx", "ts", "tsx"].contains(&ext) && file.split('/').any(|s| s == "__tests__")
}

/// A test's key: its file, then its names joined by ` > `.
pub fn key(file: &str, names: &[String]) -> String {
    let mut k = String::with_capacity(file.len() + names.iter().map(|n| n.len() + 3).sum::<usize>());
    k.push_str(file);
    for n in names {
        k.push_str(" > ");
        k.push_str(n);
    }
    k
}

/// `name` as the `k`-th of its names in its file (from 1): ` (2)` after
/// the second.
pub fn nth(name: &str, k: usize) -> String {
    if k <= 1 {
        name.to_string()
    } else {
        format!("{name} ({k})")
    }
}

/// A key's file and names, split on ` > ` (which a name holding ` > `
/// breaks: such a test has its names in `data`).
pub fn parse_key(key: &str) -> Option<(&str, Vec<String>)> {
    let (file, rest) = key.split_once(" > ")?;
    Some((file, rest.split(" > ").map(str::to_string).collect()))
}

/// The names a test's `data` holds: a JSON list.
pub fn data_names(data: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(data).ok().filter(|n| !n.is_empty())
}

/// The `data` for a test whose key can't say its names.
pub fn names_data(names: &[String]) -> String {
    serde_json::to_string(names).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;

    fn write(dir: &Path, path: &str, text: &str) {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn a_packages_runner_is_its_dependencies_or_its_config() {
        let r = |json: &str, names: &[&str]| runner_of(&serde_json::from_str(json).unwrap(), &names.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(r(r#"{"devDependencies": {"vitest": "^5"}}"#, &[]), Some(Runner::Vitest));
        assert_eq!(r(r#"{"devDependencies": {"jest": "^30"}}"#, &[]), Some(Runner::Jest));
        assert_eq!(r(r#"{"jest": {"testEnvironment": "node"}}"#, &[]), Some(Runner::Jest), "package.json's jest key");
        assert_eq!(r("{}", &["vitest.config.ts"]), Some(Runner::Vitest), "a config alone (hoisted from a workspace's root)");
        assert_eq!(r(r#"{"devDependencies": {"vitest": "^5", "jest": "^30"}}"#, &["jest.config.js"]), Some(Runner::Jest));
        assert_eq!(r(r#"{"devDependencies": {"vitest": "^5", "jest": "^30"}, "scripts": {"test": "jest --ci"}}"#, &[]), Some(Runner::Jest));
        assert_eq!(r(r#"{"devDependencies": {"vitest": "^5", "jest": "^30"}}"#, &[]), Some(Runner::Vitest));
        assert_eq!(r(r#"{"devDependencies": {"mocha": "^10"}}"#, &[]), None);
    }

    #[test]
    fn workspaces_are_read_and_their_runners_found_where_node_finds_them() {
        let dir = temp("layout");
        write(&dir, "package.json", r#"{"workspaces": ["packages/*", "!packages/old"]}"#);
        write(&dir, "packages/app/package.json", r#"{"devDependencies": {"vitest": "^5"}}"#);
        write(&dir, "packages/lib/package.json", r#"{"devDependencies": {"jest": "^30"}}"#);
        write(&dir, "packages/old/package.json", r#"{"devDependencies": {"jest": "^30"}}"#);
        write(&dir, "packages/docs/package.json", r#"{}"#);
        write(&dir, "node_modules/vitest/vitest.mjs", "");
        write(&dir, "node_modules/vitest/package.json", r#"{"version": "5.1.0"}"#);
        write(&dir, "packages/lib/node_modules/jest/bin/jest.js", "");
        write(&dir, "packages/lib/node_modules/jest/package.json", r#"{"version": "30.5.2"}"#);
        let l = Layout::read(&dir.to_string_lossy());
        assert_eq!(l.packages.iter().map(|p| (p.dir.as_str(), p.runner)).collect::<Vec<_>>(), [("packages/app", Runner::Vitest), ("packages/lib", Runner::Jest)]);
        assert_eq!(l.packages[0].installed, Some(Installed { script: "../../node_modules/vitest/vitest.mjs".into(), version: Some("5.1.0".into()) }), "hoisted");
        assert_eq!(l.packages[1].installed.as_ref().unwrap().script, "node_modules/jest/bin/jest.js");
        assert_eq!(l.runners(), "Vitest 5.1.0 and Jest 30.5.2");
        assert_eq!((l.package_of("packages/app/src/a.test.js"), l.package_of("packages/application/x.test.js")), (Some(0), None));
        assert_eq!(l.package_at("/project/packages/lib"), Some(1));
        assert_eq!(l.packages[0].label(), "packages/app (vitest)");
        // pnpm's list, and a root running them as projects.
        write(&dir, "package.json", r#"{"devDependencies": {"vitest": "^5"}}"#);
        write(&dir, "pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n  # a comment\n  - \"!packages/old\"\nonlyBuiltDependencies:\n  - esbuild\n");
        write(&dir, "vitest.config.ts", "export default { test: { projects: ['packages/*'] } }\n");
        let l = Layout::read(&dir.to_string_lossy());
        assert_eq!(l.packages.iter().map(|p| (p.dir.as_str(), p.runner)).collect::<Vec<_>>(), [("", Runner::Vitest), ("packages/lib", Runner::Jest)], "the root's projects run app");
        assert!(!l.pnp);
        assert!(Layout::read(&dir.join("nothing").to_string_lossy()).packages.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_files_are_the_runners_defaults_and_keys_split_back() {
        assert!(is_test_file("src/orbit.test.js", Runner::Vitest) && is_test_file("src/a.spec.tsx", Runner::Jest) && is_test_file("x.test.mts", Runner::Vitest));
        assert!(is_test_file("src/__tests__/a.js", Runner::Jest) && !is_test_file("src/__tests__/a.js", Runner::Vitest));
        assert!(!is_test_file("node_modules/x/a.test.js", Runner::Vitest) && !is_test_file("src/orbit.js", Runner::Jest) && !is_test_file("src/a.test.css", Runner::Jest));
        let names = vec!["outer".to_string(), "inner".to_string(), "deep".to_string()];
        assert_eq!(key("src/orbit.test.js", &names), "src/orbit.test.js > outer > inner > deep");
        assert_eq!(parse_key("src/orbit.test.js > outer > inner > deep"), Some(("src/orbit.test.js", names.clone())));
        assert_eq!(data_names(&names_data(&names)), Some(names));
        assert_eq!(nth("twice", 2), "twice (2)");
        assert!(glob("pkg-*", "pkg-a") && glob("*-ui", "web-ui") && !glob("pkg-*", "app"));
    }
}
