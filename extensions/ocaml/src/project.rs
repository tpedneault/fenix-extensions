//! What the pack knows of the project, from its files: `dune-project`
//! (the dune language it's written for), the executables and tests its
//! `dune` files name, its `.opam` files, `.ocamlformat` (the ocamlformat
//! version it pins), and, with no dune at all, the OCaml files at its
//! top, each run on its own.
//!
//! dune's files are S-expressions, read here by a reader of their own:
//! atoms, strings, `;` comments, `#|...|#` blocks and `#;` before a
//! stanza that's left out. Only the stanzas the tasks and the debugger
//! need are looked at: `executable(s)` and `test(s)`.

use std::path::Path;

/// How deep the `dune` files are looked for, how many are read, how many
/// lone files get tasks, and how much of each file is read.
const DEPTH: usize = 5;
const DUNE_FILES: usize = 200;
pub const LONE_FILES: usize = 20;
const READ_BYTES: usize = 256 << 10;

/// An S-expression: an atom (or a string), or a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

impl Sexp {
    fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a) => Some(a),
            Sexp::List(_) => None,
        }
    }

    fn list(&self) -> &[Sexp] {
        match self {
            Sexp::List(l) => l,
            Sexp::Atom(_) => &[],
        }
    }

    /// The list's head, when it's an atom: `executable` of
    /// `(executable ...)`.
    fn head(&self) -> Option<&str> {
        self.list().first()?.atom()
    }

    /// The field `(name value...)` in a stanza: its values' atoms.
    fn field(&self, name: &str) -> Option<Vec<&str>> {
        let f = self.list().iter().find(|f| f.head() == Some(name))?;
        Some(f.list()[1..].iter().filter_map(Sexp::atom).collect())
    }
}

/// `text`'s S-expressions, as far as they read: what's unclosed at the
/// end is left out.
pub fn parse_sexps(text: &str) -> Vec<Sexp> {
    let chars: Vec<char> = text.chars().collect();
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];
    // `#;`: the next expression, at each depth, is left out.
    let mut skip: Vec<usize> = Vec::new();
    let mut i = 0;
    let push = |stack: &mut Vec<Vec<Sexp>>, skip: &mut Vec<usize>, s: Sexp| {
        let depth = stack.len();
        if let Some(at) = skip.iter().position(|&d| d == depth) {
            skip.remove(at);
            return;
        }
        if let Some(top) = stack.last_mut() {
            top.push(s);
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '#' if chars.get(i + 1) == Some(&'|') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '|' && chars[i + 1] == '#') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            '#' if chars.get(i + 1) == Some(&';') => {
                skip.push(stack.len());
                i += 2;
                continue;
            }
            '(' => stack.push(Vec::new()),
            ')' => {
                if stack.len() > 1 {
                    let list = stack.pop().unwrap_or_default();
                    push(&mut stack, &mut skip, Sexp::List(list));
                }
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                push(&mut stack, &mut skip, Sexp::Atom(s));
            }
            c if c.is_whitespace() => {}
            _ => {
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '(' | ')' | '"' | ';') {
                    i += 1;
                }
                push(&mut stack, &mut skip, Sexp::Atom(chars[start..i].iter().collect()));
                continue;
            }
        }
        i += 1;
    }
    stack.into_iter().next().unwrap_or_default()
}

/// An executable, or a test, a `dune` file names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executable {
    /// Its `dune` file's folder under the project, empty at the top.
    pub dir: String,
    /// Its module (`main`): `main.exe` is what dune builds.
    pub name: String,
    /// What it's installed as, and `dune exec` takes.
    pub public_name: Option<String>,
    /// `(modes byte ...)`: dune builds `main.bc`, which earlybird debugs.
    pub byte: bool,
}

impl Executable {
    /// What `dune exec` runs it by: its public name, else its path.
    pub fn exec_target(&self) -> String {
        match &self.public_name {
            Some(p) => p.clone(),
            None => format!("./{}", self.path("exe")),
        }
    }

    /// Its build's path under `_build/default`, and the project:
    /// `bin/main.exe`, `bin/main.bc`.
    pub fn path(&self, ext: &str) -> String {
        if self.dir.is_empty() {
            format!("{}.{ext}", self.name)
        } else {
            format!("{}/{}.{ext}", self.dir, self.name)
        }
    }

    /// What it's called in a task's name and a launch's.
    pub fn label(&self) -> &str {
        self.public_name.as_deref().unwrap_or(&self.name)
    }
}

/// What a `dune` file in `dir` names: its executables, and the folders
/// with tests.
pub fn parse_dune(text: &str, dir: &str) -> (Vec<Executable>, bool) {
    let mut exes = Vec::new();
    let mut tests = false;
    for stanza in parse_sexps(text) {
        match stanza.head() {
            Some("executable") | Some("executables") => {
                let names = stanza.field("names").or_else(|| stanza.field("name")).unwrap_or_default();
                let publics = stanza.field("public_names").or_else(|| stanza.field("public_name")).unwrap_or_default();
                let byte = stanza.field("modes").is_some_and(|m| m.iter().any(|m| m.starts_with("byte")));
                for (i, name) in names.iter().enumerate() {
                    // `-` in public_names: that one isn't installed.
                    let public_name = publics.get(i).filter(|p| **p != "-").map(|p| p.to_string());
                    exes.push(Executable { dir: dir.into(), name: name.to_string(), public_name, byte });
                }
            }
            Some("test") | Some("tests") => tests = true,
            _ => {}
        }
    }
    (exes, tests)
}

/// `dune-project`, as far as the pack reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuneProject {
    /// `(lang dune 3.16)`: the least dune that reads it.
    pub lang: Option<String>,
    pub name: Option<String>,
}

impl DuneProject {
    pub fn parse(text: &str) -> DuneProject {
        let mut d = DuneProject::default();
        for s in parse_sexps(text) {
            match s.head() {
                Some("lang") if s.list().get(1).and_then(Sexp::atom) == Some("dune") => d.lang = s.list().get(2).and_then(Sexp::atom).map(str::to_string),
                Some("name") => d.name = s.list().get(1).and_then(Sexp::atom).map(str::to_string),
                _ => {}
            }
        }
        d
    }
}

/// The ocamlformat version `.ocamlformat` pins (`version = 0.27.0`):
/// ocamlformat of another version refuses to format.
pub fn ocamlformat_version(text: &str) -> Option<String> {
    text.lines().filter(|l| !l.trim_start().starts_with('#')).find_map(|l| {
        let (key, value) = l.split_once('=')?;
        (key.trim() == "version").then(|| value.trim().to_string()).filter(|v| !v.is_empty())
    })
}

/// An OCaml file at the project's top, in a project without dune.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lone {
    pub file: String,
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `dune-project` at the top, read.
    pub dune_project: Option<DuneProject>,
    /// A `dune` file anywhere: dune builds it.
    pub dune: bool,
    pub executables: Vec<Executable>,
    /// The folders with tests, under the project.
    pub test_dirs: Vec<String>,
    /// The `.opam` files at the top: the project's packages.
    pub opam_files: Vec<String>,
    /// `.ocamlformat` at the top: ocamlformat formats it; and the version
    /// it pins.
    pub ocamlformat: bool,
    pub ocamlformat_version: Option<String>,
    pub lone: Vec<Lone>,
}

/// The `dune` files under `root`, by their folders, as far down as
/// [`DEPTH`]: never `_build`, `_opam`, or a folder starting with `.`.
fn dune_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(String::new(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let at = if dir.is_empty() { root.to_path_buf() } else { root.join(&dir) };
        if at.join("dune").is_file() {
            out.push(dir.clone());
            if out.len() >= DUNE_FILES {
                break;
            }
        }
        if depth >= DEPTH {
            continue;
        }
        let mut subs: Vec<String> = std::fs::read_dir(&at)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with(['.', '_']) && n != "node_modules")
            .collect();
        subs.sort();
        for sub in subs.into_iter().rev() {
            stack.push((if dir.is_empty() { sub } else { format!("{dir}/{sub}") }, depth + 1));
        }
    }
    out
}

fn read(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b[..b.len().min(READ_BYTES)]).into_owned())
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let mut p = Project { dune_project: read(&root.join("dune-project")).map(|t| DuneProject::parse(&t)), ..Project::default() };
        for dir in dune_files(root) {
            p.dune = true;
            let text = read(&root.join(&dir).join("dune")).unwrap_or_default();
            let (exes, tests) = parse_dune(&text, &dir);
            p.executables.extend(exes);
            if tests {
                p.test_dirs.push(dir);
            }
        }
        let mut top: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        top.sort();
        p.opam_files = top.iter().filter(|n| n.ends_with(".opam")).cloned().collect();
        if let Some(text) = read(&root.join(".ocamlformat")) {
            p.ocamlformat = true;
            p.ocamlformat_version = ocamlformat_version(&text);
        }
        if !p.is_dune() {
            p.lone = top.into_iter().filter(|n| n.ends_with(".ml")).take(LONE_FILES).map(|file| Lone { file }).collect();
        }
        p
    }

    /// Whether dune builds it.
    pub fn is_dune(&self) -> bool {
        self.dune_project.is_some() || self.dune
    }

    /// Whether it's an OCaml project at all: dune's, opam's, or OCaml
    /// files.
    pub fn is_ocaml(&self) -> bool {
        self.is_dune() || !self.opam_files.is_empty() || !self.lone.is_empty()
    }

    /// The least dune it's written for: `dune-project`'s `(lang dune ...)`.
    pub fn dune_lang(&self) -> Option<&str> {
        self.dune_project.as_ref()?.lang.as_deref()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-ocaml-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A dune project as `dune init project` makes it, and more: two more
    /// executables and a test, and an `.ocamlformat`.
    pub fn dune_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("dune-project"), include_str!("samples/dune-project")).unwrap();
        std::fs::write(dir.join("bin/dune"), include_str!("samples/dune")).unwrap();
        std::fs::write(dir.join("lib/dune"), "(library\n (name orbit))\n").unwrap();
        std::fs::write(dir.join("orbit.opam"), "opam-version: \"2.0\"\n").unwrap();
        std::fs::write(dir.join(".ocamlformat"), include_str!("samples/ocamlformat")).unwrap();
        dir
    }

    #[test]
    fn sexps_read_as_dune_reads_them() {
        let s = parse_sexps("(a \"b c\" ; a comment\n (d)) #| a\n block |# #;(gone) (e \"\\\"q\\\"\")");
        assert_eq!(
            s,
            [
                Sexp::List(vec![Sexp::Atom("a".into()), Sexp::Atom("b c".into()), Sexp::List(vec![Sexp::Atom("d".into())])]),
                Sexp::List(vec![Sexp::Atom("e".into()), Sexp::Atom("\"q\"".into())]),
            ]
        );
        assert_eq!(parse_sexps("(open (still"), Vec::new(), "unclosed: left out");
        assert_eq!(parse_sexps("(a #;b c)"), [Sexp::List(vec![Sexp::Atom("a".into()), Sexp::Atom("c".into())])]);
    }

    #[test]
    fn a_dune_file_names_its_executables_and_tests() {
        let (exes, tests) = parse_dune(include_str!("samples/dune"), "bin");
        let rows: Vec<(&str, Option<&str>, bool)> = exes.iter().map(|e| (e.name.as_str(), e.public_name.as_deref(), e.byte)).collect();
        assert_eq!(rows, [("main", Some("orbit"), true), ("sim", None, false), ("replay", None, false)], "the one left out by #; isn't there");
        assert!(tests);
        assert_eq!((exes[0].exec_target(), exes[1].exec_target()), ("orbit".to_string(), "./bin/sim.exe".to_string()));
        assert_eq!((exes[0].path("bc"), exes[0].label(), exes[1].label()), ("bin/main.bc".to_string(), "orbit", "sim"));
        let (exes, _) = parse_dune("(executables (names a b) (public_names - b-tool))", "");
        assert_eq!(exes.iter().map(|e| (e.exec_target(), e.path("exe"))).collect::<Vec<_>>(), [("./a.exe".to_string(), "a.exe".to_string()), ("b-tool".to_string(), "b.exe".to_string())]);
    }

    #[test]
    fn dune_project_and_ocamlformat_say_the_versions() {
        let d = DuneProject::parse(include_str!("samples/dune-project"));
        assert_eq!(d, DuneProject { lang: Some("3.16".into()), name: Some("orbit".into()) });
        assert_eq!(ocamlformat_version(include_str!("samples/ocamlformat")).as_deref(), Some("0.27.0"));
        assert_eq!(ocamlformat_version("profile = conventional\n# version = 0.1\n"), None);
    }

    #[test]
    fn a_project_is_read_from_its_files() {
        let dir = dune_project("read");
        std::fs::create_dir_all(dir.join("_build/default/bin")).unwrap();
        std::fs::write(dir.join("_build/default/bin/dune"), "(executable (name copy))").unwrap();
        std::fs::create_dir_all(dir.join("test")).unwrap();
        std::fs::write(dir.join("test/dune"), "(test (name test_orbit))\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.is_dune() && p.is_ocaml());
        assert_eq!(p.dune_lang(), Some("3.16"));
        assert_eq!(p.executables.iter().map(|e| e.label()).collect::<Vec<_>>(), ["orbit", "sim", "replay"], "_build's copy left out");
        assert_eq!(p.test_dirs, ["bin", "test"]);
        assert_eq!(p.opam_files, ["orbit.opam"]);
        assert!(p.ocamlformat && p.ocamlformat_version.as_deref() == Some("0.27.0"));
        assert!(p.lone.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_of_scripts_has_its_files_run_alone() {
        let dir = temp("lone");
        std::fs::write(dir.join("hello.ml"), "let () = print_endline \"hello\"\n").unwrap();
        std::fs::write(dir.join("orbit.mli"), "val period : float\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.lone, [Lone { file: "hello.ml".into() }]);
        assert!(!p.is_dune() && p.is_ocaml());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).is_ocaml());
    }
}
