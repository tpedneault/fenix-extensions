//! Linters run when a file is saved, what they print read as problems and
//! shown with the language servers' (`diagnostics-set`); and formatters,
//! which Fenix runs itself (`formatter-set`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use fenix_extension_api::{Command, Event, Problem, Severity};

use crate::exec::{Output, Runs};
use crate::host::Host;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

type Build = Box<dyn Fn(&str) -> Command>;
type Parse = Box<dyn Fn(&Output, &str) -> Result<Vec<Problem>, String>>;

/// Problems, with their fixes, in each file a linter spoke of: by its path
/// in the project (`src/lib.rs`), or a full one.
pub type Files = Vec<(String, Vec<Problem>)>;

type ParseFiles = Rc<dyn Fn(&Output, &str) -> Result<Files, String>>;

/// A linter: the files it's for, its command for a file, and how its
/// output reads.
pub struct Linter {
    pub name: String,
    /// File extensions, without the dot: `py`, `go`.
    pub extensions: Vec<String>,
    /// File names, any case, with or without a `.suffix` ([`Linter::named`]).
    pub names: Vec<String>,
    /// Its command for a file, by its path in the project (`src/app.py`);
    /// it runs in `/project`.
    pub command: Build,
    /// Its problems in that file, from what it printed.
    pub parse: Parse,
    /// What it found in every file it spoke of, with fixes, when it reads
    /// that way ([`Linter::with_fixes`], [`Linter::across`]): used in place
    /// of `parse`.
    pub files: Option<ParseFiles>,
    /// Problems are set on each file it spoke of, and taken off those it
    /// spoke of last time and not this time: a crate-wide linter (clippy).
    pub across: bool,
}

impl Linter {
    pub fn new(name: &str, extensions: &[&str], command: impl Fn(&str) -> Command + 'static, parse: impl Fn(&Output, &str) -> Result<Vec<Problem>, String> + 'static) -> Linter {
        Linter {
            name: name.into(),
            extensions: extensions.iter().map(|e| e.to_string()).collect(),
            names: Vec::new(),
            command: Box::new(command),
            parse: Box::new(parse),
            files: None,
            across: false,
        }
    }

    /// One whose problems come with their fixes (ruff's, clippy's
    /// suggestions), offered by `SPC c a` (`diagnostics-set-ex`).
    pub fn with_fixes(name: &str, extensions: &[&str], command: impl Fn(&str) -> Command + 'static, parse: impl Fn(&Output, &str) -> Result<Vec<Problem>, String> + 'static) -> Linter {
        Linter::across_files(name, extensions, command, move |out, file| Ok(vec![(file.to_string(), parse(out, file)?)]), false)
    }

    /// One that speaks of more files than the one saved -- a crate, a
    /// package (`cargo clippy`, `go vet ./...`): each file's problems set,
    /// and a file it spoke of last time and not now made clean.
    pub fn across(name: &str, extensions: &[&str], command: impl Fn(&str) -> Command + 'static, parse: impl Fn(&Output, &str) -> Result<Files, String> + 'static) -> Linter {
        Linter::across_files(name, extensions, command, parse, true)
    }

    fn across_files(name: &str, extensions: &[&str], command: impl Fn(&str) -> Command + 'static, parse: impl Fn(&Output, &str) -> Result<Files, String> + 'static, across: bool) -> Linter {
        let files: ParseFiles = Rc::new(parse);
        let one = files.clone();
        let saved = move |out: &Output, file: &str| -> Result<Vec<Problem>, String> {
            let all = one(out, file)?;
            Ok(all.into_iter().filter(|(f, _)| same_file(f, file)).flat_map(|(_, ps)| ps.into_iter()).collect())
        };
        Linter { files: Some(files), across, ..Linter::new(name, extensions, command, saved) }
    }

    /// Files it's for by their names too, not only their extensions:
    /// `dockerfile` takes `Dockerfile` and `Dockerfile.prod`.
    pub fn named(mut self, names: &[&str]) -> Linter {
        self.names = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        self
    }

    pub fn takes(&self, file: &str) -> bool {
        let name = file.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
        if self.names.iter().any(|n| name == *n || name.strip_prefix(n.as_str()).is_some_and(|rest| rest.starts_with('.'))) {
            return true;
        }
        file.rsplit_once('.').is_some_and(|(_, ext)| self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }
}

/// The linters, and those running.
#[derive(Default)]
pub struct Lint {
    pub linters: Vec<Linter>,
    runs: Runs<(usize, String)>,
    /// The files each linter that speaks of several set problems on last,
    /// by its name.
    spoke_of: HashMap<String, BTreeSet<String>>,
    /// Saved files come with their paths (`file_saved`): `Saved`'s
    /// names are left alone.
    by_path: bool,
}

/// Whether `a` and `b`, each a path in the project or a full one, are the
/// same file.
fn same_file(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_start_matches("./").trim_start_matches("/project/").to_string();
    let (a, b) = (norm(a), norm(b));
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

/// A linter's path for a file as Fenix takes it: under `/project`, unless
/// it's a full one.
fn full(file: &str) -> String {
    let file = file.replace('\\', "/");
    if file.starts_with('/') || file.as_bytes().get(1) == Some(&b':') {
        file
    } else {
        format!("{PROJECT}/{}", file.trim_start_matches("./"))
    }
}

/// `path` (a full one) in `root`, with `/` between: `None` when it isn't
/// in it.
pub fn relative(path: &str, root: &str) -> Option<String> {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_string();
    let (path, root) = (norm(path), norm(root));
    let rest = if cfg!(windows) || path.contains(':') {
        // C:\ and c:\ are the same folder.
        path.get(..root.len()).filter(|p| p.eq_ignore_ascii_case(&root)).map(|_| &path[root.len()..])
    } else {
        path.strip_prefix(&root)
    }?;
    let rest = rest.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

impl Lint {
    pub fn new() -> Lint {
        Lint::default()
    }

    pub fn add(&mut self, linter: Linter) {
        self.linters.retain(|l| l.name != linter.name);
        self.linters.push(linter);
    }

    /// Linter `name` taken away (turned off): a run of it stopped, and the
    /// problems it set on the files it speaks of taken off.
    pub fn remove(&mut self, host: &mut dyn Host, name: &str) {
        let Some(i) = self.linters.iter().position(|l| l.name == name) else { return };
        // Those after it move up a place: their runs go too.
        self.runs.kill_where(host, |(j, _)| *j >= i);
        self.linters.remove(i);
        for gone in self.spoke_of.remove(name).into_iter().flatten() {
            if let Err(why) = host.diagnostics(&gone, &[]) {
                host.log(&format!("{name}: {why}"));
            }
        }
    }

    /// Linter `name` run now on `file` (in the project), whatever file it's
    /// for: `SPC m c`, clippy on the crate.
    pub fn run(&mut self, host: &mut dyn Host, name: &str, file: &str) -> Result<(), String> {
        let i = self.linters.iter().position(|l| l.name == name).ok_or_else(|| format!("{name} isn't on"))?;
        self.runs.kill_where(host, |(j, _)| *j == i);
        let mut cmd = (self.linters[i].command)(file);
        if cmd.cwd.is_none() {
            cmd.cwd = Some(PROJECT.into());
        }
        self.runs.start(host, &cmd, (i, file.to_string())).map(|_| ())
    }

    /// A file was saved (`Event::Saved`): each linter for it run on it, one
    /// still running on it stopped first. Once a saved file has come by its
    /// path ([`Lint::file_saved`]), only that's gone by.
    pub fn saved(&mut self, host: &mut dyn Host, name: &str) {
        if self.by_path {
            return;
        }
        let Some((path, Some(root))) = host.here() else { return };
        // `here` is the file saved; make sure.
        if !path.replace('\\', "/").ends_with(&name.replace('\\', "/")) {
            return;
        }
        if let Some(rel) = relative(&path, &root) {
            self.lint(host, &rel);
        }
    }

    /// A file was saved, by its path (`file_saved`): its full path,
    /// or one under `/project`. A file outside the project isn't linted.
    pub fn file_saved(&mut self, host: &mut dyn Host, path: &str) {
        self.by_path = true;
        let path = path.replace('\\', "/");
        let rel = match path.strip_prefix("/project/") {
            Some(rel) => Some(rel.to_string()),
            None => host.here().and_then(|(_, root)| relative(&path, &root?)),
        };
        if let Some(rel) = rel {
            self.lint(host, &rel);
        }
    }

    /// Each linter for `file` (in the project, `src/app.py`) run on it.
    pub fn lint(&mut self, host: &mut dyn Host, file: &str) {
        for (i, l) in self.linters.iter().enumerate().filter(|(_, l)| l.takes(file)) {
            self.runs.kill_where(host, |(j, f)| *j == i && f == file);
            let mut cmd = (l.command)(file);
            if cmd.cwd.is_none() {
                cmd.cwd = Some(PROJECT.into());
            }
            if let Err(why) = self.runs.start(host, &cmd, (i, file.to_string())) {
                host.log(&format!("{}: {why}", l.name));
            }
        }
    }

    /// A linter's output or end: its problems set when it ends.
    pub fn handle(&mut self, host: &mut dyn Host, event: &Event) -> bool {
        if !self.runs.owns(event) {
            return false;
        }
        if let Some(((i, file), out)) = self.runs.event(event) {
            if self.linters[i].files.is_some() {
                self.set_files(host, i, &out, &file);
                return true;
            }
            let l = &self.linters[i];
            match (l.parse)(&out, &file) {
                Ok(mut problems) => {
                    for p in &mut problems {
                        if p.source.is_none() {
                            p.source = Some(l.name.clone());
                        }
                    }
                    if let Err(why) = host.diagnostics(&format!("{PROJECT}/{file}"), &problems) {
                        host.log(&format!("{}: {why}", l.name));
                    }
                }
                Err(why) => host.log(&format!("{} on {file}: {why} ({})", l.name, out.why())),
            }
        }
        true
    }

    /// What linter `i` found, file by file, set: with their fixes when
    /// there are any, and a file it spoke of last time made clean.
    fn set_files(&mut self, host: &mut dyn Host, i: usize, out: &Output, file: &str) {
        let l = &self.linters[i];
        let Some(parse) = l.files.clone() else { return };
        let found = match parse(out, file) {
            Ok(found) => found,
            Err(why) => return host.log(&format!("{} on {file}: {why} ({})", l.name, out.why())),
        };
        let mut by_file: BTreeMap<String, Vec<Problem>> = BTreeMap::new();
        for (f, mut problems) in found {
            for p in &mut problems {
                if p.source.is_none() {
                    p.source = Some(l.name.clone());
                }
            }
            by_file.entry(full(&f)).or_default().extend(problems);
        }
        // The file saved, clean now.
        by_file.entry(full(file)).or_default();
        let (name, across) = (l.name.clone(), l.across);
        let now: BTreeSet<String> = by_file.iter().filter(|(_, ps)| !ps.is_empty()).map(|(f, _)| f.clone()).collect();
        if across {
            for gone in self.spoke_of.get(&name).into_iter().flatten().filter(|f| !by_file.contains_key(*f)) {
                if let Err(why) = host.diagnostics(gone, &[]) {
                    host.log(&format!("{name}: {why}"));
                }
            }
            self.spoke_of.insert(name.clone(), now);
        }
        for (path, problems) in by_file {
            let done = if problems.iter().any(|p| !p.fixes.is_empty()) {
                host.diagnostics_with_fixes(&path, &problems)
            } else {
                host.diagnostics(&path, &problems.into_iter().collect::<Vec<_>>())
            };
            if let Err(why) = done {
                host.log(&format!("{name}: {why}"));
            }
        }
    }
}

/// A place a tool gives from 1 (line, column) as the API's, from 0.
pub fn from_one(line: u32, column: u32) -> (u32, u32) {
    (line.saturating_sub(1), column.saturating_sub(1))
}

/// A severity as a tool says it: `error`, `warning`, `note`, `E`, `W`.
pub fn severity(word: &str) -> Option<Severity> {
    Some(match word.trim().to_ascii_lowercase().as_str() {
        "error" | "fatal" | "fatal error" | "e" | "err" | "f" => Severity::Error,
        "warning" | "warn" | "w" => Severity::Warning,
        "info" | "information" | "i" | "convention" | "c" | "refactor" | "r" => Severity::Info,
        "note" | "hint" | "help" | "n" => Severity::Hint,
        _ => return None,
    })
}

/// Problems in `file` from lines as compilers and most linters print
/// them: `path:line:col: severity: message`, `path:line: message`, and
/// MSBuild's `path(line,col): error CODE: message`. A line about another
/// file, or that isn't one of these, is left out; a code in brackets at
/// the end (`[E501]`, `(no-unused)`) or MSBuild's is kept. `fallback` is
/// the severity of a line that says none.
pub fn compiler_lines(text: &str, file: &str, fallback: Severity) -> Vec<Problem> {
    let file = file.replace('\\', "/");
    let file = file.trim_start_matches("./");
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((path, line_no, column, rest)) = split_place(line) else { continue };
        let path = path.replace('\\', "/");
        let path = path.trim_start_matches("./");
        if !(path == file || path.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{path}"))) {
            continue;
        }
        let (l, c) = from_one(line_no, column.unwrap_or(1));
        let mut message = rest.trim();
        let mut sev = fallback;
        let mut code = None;
        if let Some((word, after)) = message.split_once(':') {
            let mut words = word.split_whitespace().peekable();
            if let Some(first) = words.next() {
                if let Some(s) = severity(first) {
                    sev = s;
                    // `fatal error:` is one severity, not one and a code.
                    if first.eq_ignore_ascii_case("fatal") && words.peek().is_some_and(|w| w.eq_ignore_ascii_case("error")) {
                        words.next();
                    }
                    // `error CS0103:` -- MSBuild's code after its severity.
                    code = words.next().map(str::to_string);
                    message = after.trim();
                }
            }
        }
        let mut message = message.to_string();
        if code.is_none() {
            for (open, close) in [('[', ']'), ('(', ')')] {
                if message.ends_with(close) {
                    if let Some(at) = message.rfind(open) {
                        let inner = &message[at + 1..message.len() - 1];
                        if !inner.is_empty() && !inner.contains(' ') {
                            code = Some(inner.to_string());
                            message = message[..at].trim_end().to_string();
                            break;
                        }
                    }
                }
            }
        }
        let mut p = Problem::new(sev, (l, c), (l, c + 1), message);
        p.code = code;
        out.push(p);
    }
    out
}

/// A line's file, its line and maybe column, and the rest.
fn split_place(line: &str) -> Option<(&str, u32, Option<u32>, &str)> {
    // MSBuild: path(line,col): ...
    if let Some(open) = line.find('(') {
        if let Some(close) = line[open..].find("):").map(|c| open + c) {
            let inside = &line[open + 1..close];
            let mut nums = inside.split(',').map(|n| n.trim().parse::<u32>());
            if let Some(Ok(l)) = nums.next() {
                let c = nums.next().and_then(Result::ok);
                if !line[..open].is_empty() && !line[..open].contains(' ') {
                    return Some((&line[..open], l, c, &line[close + 2..]));
                }
            }
        }
    }
    // path:line:col: ... ; a drive's colon (C:\x) is part of the path.
    let skip = if line.len() > 2 && line.as_bytes()[1] == b':' && line.as_bytes()[0].is_ascii_alphabetic() { 2 } else { 0 };
    let first = line[skip..].find(':')? + skip;
    let path = &line[..first];
    if path.is_empty() || path.contains(' ') {
        return None;
    }
    let rest = &line[first + 1..];
    let (l, rest) = rest.split_once(':')?;
    let l: u32 = l.trim().parse().ok()?;
    match rest.split_once(':') {
        Some((c, after)) if c.trim().parse::<u32>().is_ok() => Some((path, l, c.trim().parse().ok(), after)),
        _ => Some((path, l, None, rest)),
    }
}

/// The formatter for `languages`: Fenix runs `cmd` with the text on its
/// stdin, `${file}` in its args the file's path (`ruff format
/// --stdin-filename ${file} -`, `gofmt`, `prettier --stdin-filepath
/// ${file}`).
pub fn formatter(host: &mut dyn Host, languages: &[&str], cmd: Option<&Command>) -> Result<(), String> {
    let languages: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
    host.formatter(&languages, cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::mock::Mock;
    use fenix_extension_api::ProcessOutput;

    fn ruff() -> Linter {
        Linter::new(
            "ruff",
            &["py", "pyi"],
            |file| Command::new("ruff").args(["check", "--output-format", "concise", file]),
            |out, file| Ok(compiler_lines(&out.stdout, file, Severity::Warning)),
        )
    }

    #[test]
    fn a_saved_file_is_linted_and_its_problems_set() {
        let mut host = Mock { here: Some(("/home/u/app/src/main.py".into(), Some("/home/u/app".into()))), ..Mock::default() };
        let mut lint = Lint::new();
        lint.add(ruff());
        lint.saved(&mut host, "main.py");
        assert_eq!(host.take(), ["spawn 1 ruff check --output-format concise src/main.py", "close 1"]);
        // Saved again before it ends: that one's stopped.
        lint.saved(&mut host, "main.py");
        assert_eq!(host.take(), ["kill 1", "spawn 2 ruff check --output-format concise src/main.py", "close 2"]);
        for line in ["src/main.py:3:1: F401 [*] `os` imported but unused", "src/other.py:1:1: E501 Line too long", "Found 2 errors."] {
            lint.handle(&mut host, &Event::ProcessOutput(ProcessOutput { process: 2, stderr: false, line: line.into() }));
        }
        assert!(lint.handle(&mut host, &Event::ProcessExit((2, Some(1)))));
        assert_eq!(host.take(), ["problems /project/src/main.py 1"]);
        let p = &host.problems["/project/src/main.py"][0];
        assert_eq!((p.start, p.end, p.severity, p.source.as_deref()), ((2, 0), (2, 1), Severity::Warning, Some("ruff")));
        assert_eq!(p.message, "F401 [*] `os` imported but unused");
        assert!(!lint.handle(&mut host, &Event::ProcessExit((7, Some(0)))));
    }

    #[test]
    fn a_file_it_isnt_for_or_outside_the_project_isnt_linted() {
        let mut lint = Lint::new();
        lint.add(ruff());
        let mut host = Mock { here: Some(("/home/u/app/README.md".into(), Some("/home/u/app".into()))), ..Mock::default() };
        lint.saved(&mut host, "README.md");
        let mut elsewhere = Mock { here: Some(("/tmp/x.py".into(), Some("/home/u/app".into()))), ..Mock::default() };
        lint.saved(&mut elsewhere, "x.py");
        let mut other = Mock { here: Some(("/home/u/app/a.py".into(), Some("/home/u/app".into()))), ..Mock::default() };
        lint.saved(&mut other, "b.py");
        assert!(host.take().is_empty() && elsewhere.take().is_empty() && other.take().is_empty());
        assert!(ruff().takes("x.PYI") && !ruff().takes("py"));
        let hadolint = Linter::new("hadolint", &["dockerfile"], |f| Command::new("hadolint").arg(f), |_, _| Ok(Vec::new())).named(&["Dockerfile", "Containerfile"]);
        for file in ["Dockerfile", "deploy/Dockerfile.prod", "containerfile", "api.dockerfile", "deploy\\DOCKERFILE"] {
            assert!(hadolint.takes(file), "{file}");
        }
        for file in ["Dockerfiles", "Dockerfile-old", "docker/README.md"] {
            assert!(!hadolint.takes(file), "{file}");
        }
    }

    #[test]
    fn paths_are_made_relative_to_the_project() {
        assert_eq!(relative("/home/u/app/src/a.py", "/home/u/app").as_deref(), Some("src/a.py"));
        assert_eq!(relative("/home/u/app/src/a.py", "/home/u/app/").as_deref(), Some("src/a.py"));
        assert_eq!(relative("C:\\Users\\u\\app\\a.py", "c:\\users\\u\\app").as_deref(), Some("a.py"));
        assert_eq!(relative("/home/u/apple/a.py", "/home/u/app"), None);
        assert_eq!(relative("/home/u/app", "/home/u/app"), None);
    }

    #[test]
    fn compiler_lines_read_as_most_tools_print_them() {
        let text = "\
./main.go:10:2: undefined: foo
main.go:12: missing return
src/a.c:4:9: warning: unused variable 'x' [-Wunused-variable]
src/a.c:5:1: error: expected ';'
C:\\p\\src\\a.c:6:3: note: declared here
Program.cs(7,13): error CS0103: The name 'x' does not exist [C:\\p\\app.csproj]
src/a.c: In function 'main':
lib/b.js:1:1: Unexpected var (no-var)
In file included from x.h:1:
";
        let go = compiler_lines(text, "main.go", Severity::Error);
        assert_eq!(go.iter().map(|p| (p.start, p.message.as_str())).collect::<Vec<_>>(), [((9, 1), "undefined: foo"), ((11, 0), "missing return")]);
        let c = compiler_lines(text, "src/a.c", Severity::Error);
        assert_eq!(
            c.iter().map(|p| (p.start, p.severity, p.message.as_str(), p.code.as_deref())).collect::<Vec<_>>(),
            [((3, 8), Severity::Warning, "unused variable 'x'", Some("-Wunused-variable")), ((4, 0), Severity::Error, "expected ';'", None), ((5, 2), Severity::Hint, "declared here", None)]
        );
        let cs = compiler_lines(text, "Program.cs", Severity::Warning);
        assert_eq!((cs[0].start, cs[0].severity, cs[0].code.as_deref()), ((6, 12), Severity::Error, Some("CS0103")));
        assert!(cs[0].message.starts_with("The name 'x' does not exist"));
        let js = compiler_lines(text, "lib/b.js", Severity::Warning);
        assert_eq!((js[0].message.as_str(), js[0].code.as_deref()), ("Unexpected var", Some("no-var")));
        assert_eq!(from_one(0, 0), (0, 0));
        assert_eq!(severity("Fatal Error"), Some(Severity::Error));
    }

    #[test]
    fn a_formatter_is_set_for_its_languages() {
        let mut host = Mock::default();
        formatter(&mut host, &["python"], Some(&Command::new("ruff").args(["format", "--stdin-filename", "${file}", "-"]))).unwrap();
        formatter(&mut host, &["go"], None).unwrap();
        assert_eq!(host.take(), ["formatter python: ruff format --stdin-filename ${file} -", "formatter go: "]);
    }

    /// clippy, much as `--message-format json` reads: `file:line:message`.
    fn crate_wide() -> Linter {
        Linter::across(
            "clippy",
            &["rs"],
            |_| Command::new("cargo").args(["clippy"]),
            |out, _| {
                Ok(out
                    .stdout
                    .lines()
                    .filter_map(|l| {
                        let mut parts = l.splitn(3, ':');
                        let (file, line, message) = (parts.next()?, parts.next()?.parse::<u32>().ok()?, parts.next()?);
                        let p = Problem::warning((line, 0), (line, 1), message);
                        let p = if message.contains("needless") { p.fix("Remove it", vec![fenix_extension_api::TextEdit::new((line, 0), (line, 4), "")]) } else { p.into() };
                        Some((file.to_string(), vec![p]))
                    })
                    .collect())
            },
        )
    }

    fn ran(lint: &mut Lint, host: &mut Mock, id: u64, lines: &[&str]) {
        for line in lines {
            lint.handle(host, &Event::ProcessOutput(ProcessOutput { process: id, stderr: false, line: line.to_string() }));
        }
        assert!(lint.handle(host, &Event::ProcessExit((id, Some(0)))));
    }

    #[test]
    fn a_crate_wide_linter_sets_each_file_and_cleans_those_it_no_longer_speaks_of() {
        let mut host = Mock::default();
        let mut lint = Lint::new();
        lint.add(crate_wide());
        lint.file_saved(&mut host, "/project/src/main.rs");
        assert_eq!(host.take(), ["spawn 1 cargo clippy", "close 1"]);
        ran(&mut lint, &mut host, 1, &["src/lib.rs:3:needless return", "src/lib.rs:9:x", "src/util.rs:1:y"]);
        let mut calls = host.take();
        calls.sort();
        assert_eq!(calls, ["problems /project/src/lib.rs 2 (1 fixes)", "problems /project/src/main.rs 0", "problems /project/src/util.rs 1"]);
        assert_eq!(host.problems["/project/src/lib.rs"][0].source.as_deref(), Some("clippy"));
        assert_eq!(host.fixes["/project/src/lib.rs"][0].fixes[0].title, "Remove it");
        // util.rs is clean now: its problems taken away.
        lint.lint(&mut host, "src/lib.rs");
        host.take();
        ran(&mut lint, &mut host, 2, &["src/lib.rs:9:x"]);
        let mut calls = host.take();
        calls.sort();
        assert_eq!(calls, ["problems /project/src/lib.rs 1", "problems /project/src/util.rs 0"]);
        // Once saved files come by path, Saved's names are left alone.
        host.here = Some(("/home/u/app/src/main.rs".into(), Some("/home/u/app".into())));
        lint.saved(&mut host, "main.rs");
        assert!(host.take().is_empty());
        lint.file_saved(&mut host, "/home/u/app/src/main.rs");
        assert_eq!(host.take(), ["spawn 3 cargo clippy", "close 3"]);
        lint.file_saved(&mut host, "/elsewhere/x.rs");
        assert!(host.take().is_empty(), "not in the project");
        // Run now on any file; turned off, what it said taken off.
        lint.run(&mut host, "clippy", "Cargo.toml").unwrap();
        assert_eq!(host.take(), ["kill 3", "spawn 4 cargo clippy", "close 4"]);
        lint.remove(&mut host, "clippy");
        assert_eq!(host.take(), ["kill 4", "problems /project/src/lib.rs 0"]);
        assert!(lint.linters.is_empty() && lint.run(&mut host, "clippy", "a.rs").is_err());
    }

    #[test]
    fn a_linter_with_fixes_gives_them_on_the_saved_file_only() {
        let mut host = Mock::default();
        let mut lint = Lint::new();
        lint.add(Linter::with_fixes(
            "ruff",
            &["py"],
            |f| Command::new("ruff").args(["check", f]),
            |_, file| Ok(vec![Problem::warning((0, 0), (0, 2), format!("in {file}")).fix("Remove import", vec![fenix_extension_api::TextEdit::new((0, 0), (1, 0), "")])]),
        ));
        lint.lint(&mut host, "app.py");
        host.take();
        ran(&mut lint, &mut host, 1, &[]);
        assert_eq!(host.take(), ["problems /project/app.py 1 (1 fixes)"]);
        assert_eq!((lint.linters[0].parse)(&Output::default(), "app.py").unwrap()[0].message, "in app.py", "parse still gives the plain problems");
    }

    #[test]
    fn fatal_error_is_a_severity_not_a_code() {
        let text = "src/a.c:1:10: fatal error: nope.h: No such file or directory\nC:\\p\\a.cpp(3): fatal error C1083: Cannot open include file\n";
        let c = compiler_lines(text, "src/a.c", Severity::Warning);
        assert_eq!((c[0].severity, c[0].code.as_deref(), c[0].message.as_str()), (Severity::Error, None, "nope.h: No such file or directory"));
        let msvc = compiler_lines(text, "a.cpp", Severity::Warning);
        assert_eq!((msvc[0].severity, msvc[0].code.as_deref(), msvc[0].message.as_str()), (Severity::Error, Some("C1083"), "Cannot open include file"));
    }
}
