//! What to run for an ask: pytest, through the plugin, a step for each
//! rootdir the project has (usually one).
//!
//! | Ask | Arguments after the boot (`-q -p fenix_tests --rootdir=.`) |
//! |---|---|
//! | discover | `--collect-only` |
//! | run all | none |
//! | tests, files, classes | their node ids: a file's path, `path::Class`, `path::test`, `path::test[case]`; a few of them as arguments, more in a file of ids (`@${step_files}/ids.txt`, one a line, which pytest reads) |
//! | a template | its id without the brackets: all its cases |
//! | a case whose id is over 1 KiB | its full id from `data`; without it, its file |
//!
//! pytest's ids for a file or a class are exact, so a group's fill-in isn't
//! needed: asking for the node runs everything under it. The step's `cwd`
//! is the rootdir the ids are relative to ([`super::layout`]); `--rootdir=.`
//! says it to pytest so a configuration file above the project can't move
//! it. Each step covers the tests it names and, on its rootdir's step, the
//! groups asked.

use std::collections::BTreeMap;

use fenix_extension_api::Command;
use fenix_lang_kit::tasks::parse_env;
use fenix_lang_kit::tests::{is_cut_id, Ask, Asked, Covered, Step};

use super::layout::{key, split_key, Layout};
use super::Facts;

/// Python's own boot: pytest's folder of ours is its first argument, so a
/// `'` in the path can't end a string and no `PYTHONPATH` hides the
/// project's.
pub const BOOT: &str = "import sys; sys.path.insert(0, sys.argv.pop(1)); import pytest; raise SystemExit(pytest.main(sys.argv[1:]))";

/// The folder of the plugin, as Fenix fills `${data}` in.
pub const PLUGIN_DIR: &str = "${data}/lang-kit/tests";

/// The plugin, as lang-kit writes it.
pub const PLUGIN: &str = "fenix_tests.py";

/// The project as Fenix gives it to a step.
const PROJECT: &str = "/project";

/// Node ids given as arguments, up to this many and this long all
/// together; more go in a file.
pub const INLINE_IDS: usize = 8;
pub const INLINE_BYTES: usize = 1500;

pub struct Planner<'a> {
    pub facts: &'a Facts,
    pub layout: &'a Layout,
    /// The folders (below the project) of the files scanned.
    pub files: Vec<String>,
    /// The project's `.env`, which the tasks pass too.
    pub env: Vec<(String, String)>,
}

/// What one entry runs.
struct Pick {
    /// The folder pytest runs in.
    root: String,
    /// A node id below it.
    node: String,
    cover: Covered,
}

/// The project's `.env` as the pack's tasks pass it.
pub fn dotenv(root: &str) -> Vec<(String, String)> {
    std::fs::read_to_string(format!("{root}/.env")).map(|t| parse_env(&t)).unwrap_or_default()
}

impl Planner<'_> {
    /// Python with pytest in the project's environment, as the pack's
    /// `test` task chooses: `uv run python` for a project uv manages,
    /// `uv run --with pytest python` when it doesn't ask for pytest, else
    /// the environment's Python.
    pub fn python(&self) -> Command {
        let f = self.facts;
        match &f.uv {
            Some(uv) => {
                let cmd = Command::new(uv).arg("run");
                let cmd = if f.with_pytest { cmd.args(["--with", "pytest"]) } else { cmd };
                cmd.arg("python")
            }
            None => Command::new(f.python.as_deref().unwrap_or("python3")),
        }
    }

    /// The command that starts pytest in `root`, with `extra` after its own
    /// arguments.
    pub fn command(&self, root: &str, extra: &[String]) -> Command {
        let cwd = if root.is_empty() { PROJECT.to_string() } else { format!("{PROJECT}/{root}") };
        let mut env = self.env.clone();
        if !env.iter().any(|(k, _)| k == "PYTHONIOENCODING") {
            env.push(("PYTHONIOENCODING".into(), "utf-8".into()));
        }
        let mut cmd = self.python().args(["-c", BOOT, PLUGIN_DIR, "-q", "-p", "fenix_tests", "--rootdir=."]).args(extra.iter().cloned()).cwd(&cwd);
        for (k, v) in env {
            cmd = cmd.env(&k, &v);
        }
        cmd
    }

    fn step(&self, root: &str, extra: &[String], label: &str) -> Step {
        let label = if root.is_empty() { label.to_string() } else { format!("{label} in {root}") };
        Step::new(self.command(root, extra), label).needs(format!("/data/lang-kit/tests/{PLUGIN}"))
    }

    /// The folders pytest runs in for the whole project: where the files
    /// scanned have theirs, or the project's own.
    pub fn roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = self.files.iter().map(|f| self.layout.root_of(f)).collect();
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            roots.push(String::new());
        }
        roots
    }

    pub fn discover(&self) -> Vec<Step> {
        self.roots().iter().map(|r| self.step(r, &["--collect-only".into()], "listing the tests")).collect()
    }

    pub fn run_all(&self) -> Vec<Step> {
        self.roots().iter().map(|r| self.step(r, &[], "every test")).collect()
    }

    /// `key` as a folder and a node id below it.
    fn node(&self, key_: &str) -> (String, String) {
        let (file, rest) = split_key(key_);
        let root = self.layout.root_of(file);
        let below = if root.is_empty() { key_ } else { key_.strip_prefix(&format!("{root}/")).unwrap_or(key_) };
        debug_assert!(rest.is_empty() || key_.ends_with(rest));
        (root, below.to_string())
    }

    fn pick(&self, item: &Asked) -> Option<Pick> {
        match item {
            Asked::Test(f) => {
                // A key over 1 KiB is cut: its full one is in `data`; without
                // it, all there is is its file's.
                let full = match (&f.data, is_cut_id(&f.key.0)) {
                    (Some(d), _) => d.as_str(),
                    (None, false) => f.key.0.as_str(),
                    (None, true) => split_key(&f.key.0).0,
                };
                let (root, node) = self.node(full);
                Some(Pick { root, node, cover: Covered::Test(f.key.clone()) })
            }
            Asked::Group { groups, .. } => {
                // A file's group first, then its classes: `tests/test_a.py::TestA`.
                // Its label is its path below the project.
                let file = groups.first().map(|g| g.label.as_str())?;
                let root = self.layout.root_of(file);
                let mut node = key(&root, file);
                node = if root.is_empty() { node } else { node.strip_prefix(&format!("{root}/")).unwrap_or(&node).to_string() };
                for g in &groups[1..] {
                    node = format!("{node}::{}", g.label);
                }
                Some(Pick { root, node, cover: Covered::Group(groups.iter().map(|g| g.label.clone()).collect()) })
            }
        }
    }

    pub fn run(&self, items: &[Asked]) -> Vec<Step> {
        let mut by_root: BTreeMap<String, Vec<Pick>> = BTreeMap::new();
        for p in items.iter().filter_map(|i| self.pick(i)) {
            by_root.entry(p.root.clone()).or_default().push(p);
        }
        let mut steps = Vec::new();
        for (root, picks) in by_root {
            let mut seen = std::collections::HashSet::new();
            let nodes: Vec<&str> = picks.iter().map(|p| p.node.as_str()).filter(|n| seen.insert(*n)).collect();
            let bytes: usize = nodes.iter().map(|n| n.len() + 1).sum();
            let mut step;
            if nodes.len() <= INLINE_IDS && bytes <= INLINE_BYTES {
                let args: Vec<String> = nodes.iter().map(|n| n.to_string()).collect();
                step = self.step(&root, &args, "pytest");
            } else {
                // One id a line: pytest reads them, whatever there are, and
                // the command line stays as short as it was.
                step = self.step(&root, &["@${step_files}/ids.txt".into()], "pytest");
                step = step.file("ids.txt", nodes.iter().map(|n| format!("{n}\n")).collect::<String>().into_bytes());
            }
            steps.push(step.covers(picks.into_iter().map(|p| p.cover)));
        }
        steps
    }
}

/// The steps for `ask`.
pub fn plan(p: &Planner, ask: &Ask) -> Vec<Step> {
    match ask {
        Ask::Discover => p.discover(),
        Ask::RunAll => p.run_all(),
        Ask::Run(items) => p.run(items),
        Ask::Debug(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::tests::{Found, Group};

    fn facts() -> Facts {
        Facts { python: Some("python3".into()), ..Facts::default() }
    }

    #[test]
    fn the_command_is_the_projects_pythons_boot_and_the_plugin() {
        let layout = Layout::new("/nowhere");
        let f = Facts { uv: Some("/u/uv".into()), with_pytest: true, ..facts() };
        let p = Planner { facts: &f, layout: &layout, files: Vec::new(), env: vec![("DJANGO_DEBUG".into(), "1".into())] };
        let cmd = p.command("", &["--collect-only".into()]);
        assert_eq!(cmd.program, "/u/uv");
        assert_eq!(&cmd.args[..4], ["run", "--with", "pytest", "python"]);
        assert_eq!(&cmd.args[4..], ["-c", BOOT, "${data}/lang-kit/tests", "-q", "-p", "fenix_tests", "--rootdir=.", "--collect-only"]);
        assert_eq!(cmd.env, [("DJANGO_DEBUG".to_string(), "1".to_string()), ("PYTHONIOENCODING".to_string(), "utf-8".to_string())]);
        assert_eq!(cmd.cwd.as_deref(), Some("/project"));
        let f = Facts { uv: Some("uv".into()), ..facts() };
        let p = Planner { facts: &f, layout: &layout, files: Vec::new(), env: Vec::new() };
        assert_eq!(&p.command("services/api", &[]).args[..2], ["run", "python"]);
        assert_eq!(p.command("services/api", &[]).cwd.as_deref(), Some("/project/services/api"));
        let p = Planner { facts: &facts(), layout: &layout, files: Vec::new(), env: vec![("PYTHONIOENCODING".into(), "latin-1".into())] };
        assert_eq!((p.command("", &[]).program.as_str(), p.command("", &[]).env.len()), ("python3", 1), "the project's own setting stays");
    }

    #[test]
    fn what_is_asked_runs_by_node_id_inline_or_in_a_file() {
        let layout = Layout::new("/nowhere");
        let f = facts();
        let p = Planner { facts: &f, layout: &layout, files: Vec::new(), env: Vec::new() };
        let test = |k: &str| Asked::Test(Found::test(k, "t"));
        let steps = p.run(&[test("tests/test_a.py::test_one"), Asked::Group { groups: vec![Group::file("tests/test_b.py"), Group::suite("TestB", "tests/test_b.py", 3)], tests: None, known: Vec::new() }]);
        assert_eq!(steps.len(), 1);
        assert_eq!(&steps[0].command.args[7..], ["tests/test_a.py::test_one", "tests/test_b.py::TestB"]);
        assert_eq!(steps[0].covers, [Covered::Test("tests/test_a.py::test_one".into()), Covered::Group(vec!["tests/test_b.py".into(), "TestB".into()])]);
        assert_eq!(steps[0].needs, ["/data/lang-kit/tests/fenix_tests.py"]);
        // More than a few: a file, one a line, the command line as short as ever.
        let many: Vec<Asked> = (0..INLINE_IDS + 1).map(|i| test(&format!("tests/test_a.py::test_{i}"))).collect();
        let steps = p.run(&many);
        assert_eq!(steps[0].command.args.last().map(String::as_str), Some("@${step_files}/ids.txt"));
        assert_eq!(steps[0].files.len(), 1);
        assert_eq!(String::from_utf8_lossy(&steps[0].files[0].1).lines().count(), INLINE_IDS + 1);
        assert_eq!(steps[0].covers.len(), INLINE_IDS + 1);
    }

    #[test]
    fn a_cut_key_runs_by_its_data_and_without_it_by_its_file() {
        let layout = Layout::new("/nowhere");
        let f = facts();
        let p = Planner { facts: &f, layout: &layout, files: Vec::new(), env: Vec::new() };
        let long = format!("tests/test_a.py::test_x[{}]", "y".repeat(1100));
        let cut = fenix_lang_kit::tests::cut_id(&long).into_owned();
        assert!(is_cut_id(&cut) && cut.len() <= 1024);
        let with = p.run(&[Asked::Test(Found::test(cut.clone(), "y").data(long.clone()))]);
        assert_eq!(with[0].command.args.last().map(String::as_str), Some(long.as_str()));
        assert_eq!(with[0].covers, [Covered::Test(cut.clone().into())]);
        let without = p.run(&[Asked::Test(Found::test(cut.clone(), "y"))]);
        assert_eq!(without[0].command.args.last().map(String::as_str), Some("tests/test_a.py"), "all of its file: runs at least that case");
    }
}
