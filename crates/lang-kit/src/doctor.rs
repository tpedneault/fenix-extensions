//! Checks on the project doctor's page: each program a pack needs found
//! (`find-program`), its version read and held to what the project asks,
//! and a fix row that installs it.

use std::collections::HashMap;
use std::rc::Rc;

use fenix_extension_api::{Check, Command, Event, Health, Section};

use crate::exec::{Output, Runs};
use crate::host::Host;
use crate::version::{find_version, Requirement};

/// How long a program has to say its version.
pub const VERSION_MILLIS: u32 = 5000;

/// What a fix row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// Runs this as a task (`run-task`): a package manager, an
    /// installer.
    Task(Command),
    /// Downloads and unpacks the [`crate::install::Install`] of this name.
    Install(String),
    /// Nothing of lang-kit's: the command is the pack's own.
    Own,
}

/// What a check makes of what its program said, in place of reading a
/// version: its health, and what it says. A closure, so it can hold what
/// it expects (the toolchain `rust-toolchain.toml` pins).
pub type Judge = Rc<dyn Fn(&Output) -> (Health, String)>;

/// The pack's own checks, worked out again each time the doctor runs.
pub type Extra = Box<dyn FnMut() -> Vec<Check>>;

/// A program a pack needs, and what it asks of it.
#[derive(Clone)]
pub struct Program {
    /// As its manifest's `permissions.process` names it.
    pub name: String,
    /// What the check is called: `Python`, `uv`, `gopls`.
    pub label: String,
    pub section: Section,
    /// Where else to look (`find-program`'s hints).
    pub hints: Vec<String>,
    /// What makes it say its version; none checks only that it's there.
    pub version_args: Vec<String>,
    /// The versions the project takes.
    pub wants: Option<Requirement>,
    /// Missing or too old is bad, not a warning.
    pub required: bool,
    /// Its fix row: what it says, the command id it runs (one of the
    /// pack's, in its manifest), and what that does.
    pub fix: Option<(String, String, Fix)>,
    /// What it said, read by the pack (`uv sync --dry-run`'s changes)
    /// rather than as a version.
    pub judge: Option<Judge>,
    /// Given to it as it's asked (`GOTOOLCHAIN=local`, `COREPACK_ENABLE_DOWNLOAD_PROMPT=0`).
    pub env: Vec<(String, String)>,
    /// Where it's asked, under `/project`; the project when none.
    pub cwd: Option<String>,
    /// How long it has to answer, in milliseconds; [`VERSION_MILLIS`] when
    /// none.
    pub timeout: Option<u32>,
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("section", &self.section)
            .field("hints", &self.hints)
            .field("version_args", &self.version_args)
            .field("wants", &self.wants)
            .field("required", &self.required)
            .field("fix", &self.fix)
            .field("judge", &self.judge.is_some())
            .field("env", &self.env)
            .field("cwd", &self.cwd)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Program {
    /// `name`, asked `--version`, needed.
    pub fn new(name: &str, label: &str) -> Program {
        Program {
            name: name.into(),
            label: label.into(),
            section: Section::Toolchain,
            hints: Vec::new(),
            version_args: vec!["--version".into()],
            wants: None,
            required: true,
            fix: None,
            judge: None,
            env: Vec::new(),
            cwd: None,
            timeout: None,
        }
    }

    pub fn section(mut self, section: Section) -> Program {
        self.section = section;
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Program {
        self.hints.push(hint.into());
        self
    }

    /// What makes it say its version (`["version"]`, `["-version"]`); an
    /// empty list only finds it.
    pub fn version_args<S: Into<String>>(mut self, args: impl IntoIterator<Item = S>) -> Program {
        self.version_args = args.into_iter().map(Into::into).collect();
        self
    }

    /// `min` or later (`3.9`).
    pub fn at_least(self, min: &str) -> Program {
        self.wants(&format!(">={min}"))
    }

    /// What the project asks (`>=18 <21`, `^1.22`); left as it was when
    /// it can't be read.
    pub fn wants(mut self, requirement: &str) -> Program {
        if let Some(r) = Requirement::parse(requirement) {
            self.wants = Some(r);
        }
        self
    }

    /// Missing is a warning.
    pub fn optional(mut self) -> Program {
        self.required = false;
        self
    }

    /// A fix row saying `label` that runs its command `command`.
    pub fn fix(mut self, label: &str, command: &str, fix: Fix) -> Program {
        self.fix = Some((label.into(), command.into(), fix));
        self
    }

    /// What `version_args` makes it say read by `judge`, not as a
    /// version: a check that the project is in step (`uv sync --locked
    /// --dry-run`, `go mod verify`). Its fix row shows when it isn't ok.
    pub fn judge(mut self, judge: impl Fn(&Output) -> (Health, String) + 'static) -> Program {
        self.judge = Some(Rc::new(judge));
        self
    }

    /// `key=value` in its environment as it's asked.
    pub fn env(mut self, key: &str, value: &str) -> Program {
        self.env.retain(|(k, _)| k != key);
        self.env.push((key.into(), value.into()));
        self
    }

    /// Asked in `dir` (under `/project`), not the project.
    pub fn cwd(mut self, dir: &str) -> Program {
        self.cwd = Some(dir.into());
        self
    }

    /// `millis` to answer, not [`VERSION_MILLIS`]: `go version` may
    /// download a toolchain first.
    pub fn timeout(mut self, millis: u32) -> Program {
        self.timeout = Some(millis);
        self
    }

    /// How long it has to answer.
    pub fn millis(&self) -> u32 {
        self.timeout.unwrap_or(VERSION_MILLIS)
    }

    /// What it's asked with, found at `path`.
    pub fn command(&self, path: &str) -> Command {
        let mut cmd = Command::new(path).args(self.version_args.clone());
        cmd.env = self.env.clone();
        cmd.cwd = self.cwd.clone();
        cmd
    }

    fn check(&self, health: Health, detail: String) -> Check {
        let check = Check::new(self.section, self.label.clone(), health, detail);
        match &self.fix {
            Some((label, command, _)) if health != Health::Ok => check.fix(label, command),
            _ => check,
        }
    }

    fn bad(&self) -> Health {
        if self.required {
            Health::Bad
        } else {
            Health::Warn
        }
    }

    /// Its check: not found, or found at `path` and saying `out`.
    pub fn evaluate(&self, path: Option<&str>, out: Option<&Output>) -> Check {
        let Some(path) = path else {
            return self.check(self.bad(), format!("{} isn't installed (or isn't on PATH)", self.name));
        };
        let Some(out) = out else {
            return self.check(Health::Ok, path.to_string());
        };
        if let Some(judge) = &self.judge {
            let (health, detail) = judge(out);
            return self.check(health, detail);
        }
        let Some(version) = find_version(&out.text()) else {
            let why = if out.ok() { "its version couldn't be read".to_string() } else { format!("asking its version failed: {}", out.why()) };
            return self.check(Health::Warn, format!("{path}: {why}"));
        };
        match &self.wants {
            Some(r) if !r.matches(&version) => self.check(self.bad(), format!("{version}, but the project needs {r} ({path})")),
            _ => self.check(Health::Ok, format!("{version} ({path})")),
        }
    }
}

/// The checks a pack makes, run again when asked (a fix ran, the
/// project changed), and set on the doctor's page when all have answered.
#[derive(Default)]
pub struct Doctor {
    pub programs: Vec<Program>,
    /// The pack's own checks, set with these.
    pub extra: Vec<Check>,
    /// What works `extra` out again, when the pack gives one: called each
    /// time the checks are made (its `<prefix>.doctor`, a fix that ended).
    pub refresh: Option<Extra>,
    results: Vec<Option<Check>>,
    runs: Runs<usize>,
    /// A timer for each wait: the programs with that long to answer.
    timers: Vec<(u64, u32)>,
    /// Where each program was found, by name, last time.
    pub found: HashMap<String, String>,
}

impl std::fmt::Debug for Doctor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Doctor").field("programs", &self.programs).field("extra", &self.extra).field("refresh", &self.refresh.is_some()).field("found", &self.found).finish()
    }
}

impl Doctor {
    pub fn new() -> Doctor {
        Doctor::default()
    }

    /// One more, or in place of the one with its name and label: the same
    /// program may be checked twice, for different things (`uv`, and
    /// the project's Python as uv finds it).
    pub fn program(&mut self, program: Program) {
        self.programs.retain(|p| p.name != program.name || p.label != program.label);
        self.programs.push(program);
    }

    /// The first check of program `name`.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Program> {
        self.programs.iter_mut().find(|p| p.name == name)
    }

    /// The fix whose command is `command`.
    pub fn fix_for(&self, command: &str) -> Option<(&Program, &Fix)> {
        self.programs.iter().find_map(|p| p.fix.as_ref().filter(|(_, c, _)| c == command).map(|(_, _, f)| (p, f)))
    }

    /// Every check made again: the pack's own worked out again first, when
    /// it gave a way to.
    pub fn run(&mut self, host: &mut dyn Host) {
        if let Some(refresh) = self.refresh.as_mut() {
            self.extra = refresh();
        }
        self.runs.kill_all(host);
        self.timers.clear();
        self.results = vec![None; self.programs.len()];
        for (i, p) in self.programs.iter().enumerate() {
            let path = host.find_program(&p.name, &p.hints);
            match &path {
                Some(path) => {
                    self.found.insert(p.name.clone(), path.clone());
                }
                None => {
                    self.found.remove(&p.name);
                }
            }
            match path {
                Some(path) if !p.version_args.is_empty() => {
                    if let Err(why) = self.runs.start(host, &p.command(&path), i) {
                        self.results[i] = Some(p.check(Health::Warn, format!("{path}: it couldn't be run: {why}")));
                    }
                }
                path => self.results[i] = Some(p.evaluate(path.as_deref(), None)),
            }
        }
        if self.runs.is_empty() {
            self.publish(host);
            return;
        }
        let mut waits: Vec<u32> = self.runs.jobs().map(|&i| self.programs[i].millis()).collect();
        waits.sort_unstable();
        waits.dedup();
        for millis in waits {
            self.timers.push((host.set_timer(millis), millis));
        }
    }

    /// An event for it: a version said, or the time to say it up.
    pub fn handle(&mut self, host: &mut dyn Host, event: &Event) -> bool {
        if self.runs.owns(event) {
            if let Some((i, out)) = self.runs.event(event) {
                let path = self.found.get(&self.programs[i].name).cloned();
                self.results[i] = Some(self.programs[i].evaluate(path.as_deref(), Some(&out)));
                if self.runs.is_empty() {
                    self.timers.clear();
                    self.publish(host);
                }
            }
            return true;
        }
        let Event::Timer(t) = event else { return false };
        let Some(at) = self.timers.iter().position(|(timer, _)| timer == t) else { return false };
        let (_, millis) = self.timers.remove(at);
        let programs = &self.programs;
        for i in self.runs.kill_where(host, |&i| programs[i].millis() <= millis) {
            let p = &self.programs[i];
            let secs = p.millis().div_ceil(1000);
            let what = if p.judge.is_some() { "answer" } else { "say its version" };
            self.results[i] = Some(p.check(Health::Warn, format!("{} didn't {what} within {secs} s", p.name)));
        }
        if self.runs.is_empty() {
            self.timers.clear();
            self.publish(host);
        }
        true
    }

    /// The checks as they stand.
    pub fn checks(&self) -> Vec<Check> {
        self.results.iter().flatten().cloned().chain(self.extra.iter().cloned()).collect()
    }

    fn publish(&self, host: &mut dyn Host) {
        host.doctor(&self.checks());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::split_words;
    use crate::host::mock::Mock;
    use fenix_extension_api::ProcessOutput;

    fn said(doctor: &mut Doctor, host: &mut Mock, id: u64, text: &str, code: i32) {
        doctor.handle(host, &Event::ProcessOutput(ProcessOutput { process: id, stderr: false, line: text.into() }));
        doctor.handle(host, &Event::ProcessExit((id, Some(code))));
    }

    fn uv() -> Command {
        let w = split_words("pip install uv");
        Command::new(&w[0]).args(w[1..].iter().cloned())
    }

    fn doctor() -> Doctor {
        let mut d = Doctor::new();
        d.program(Program::new("python3", "Python").at_least("3.9"));
        d.program(Program::new("uv", "uv").fix("Install uv", "python.install-uv", Fix::Task(uv())));
        d.program(Program::new("basedpyright", "Language server").optional().version_args(Vec::<String>::new()).fix("Install basedpyright", "python.install-server", Fix::Install("basedpyright".into())));
        d.extra.push(Check::new(Section::Project, "venv", Health::Ok, "in step with uv.lock"));
        d
    }

    fn labels(checks: &[Check]) -> Vec<(String, Health, String, Option<String>)> {
        checks.iter().map(|c| (c.label.clone(), c.health, c.detail.clone(), c.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn each_program_is_found_and_its_version_held_to_what_is_asked() {
        let mut host = Mock::default();
        host.programs.insert("python3".into(), "/usr/bin/python3".into());
        let mut d = doctor();
        d.run(&mut host);
        assert_eq!(host.take(), ["spawn 1 /usr/bin/python3 --version", "close 1", "timer 2 5000"]);
        said(&mut d, &mut host, 1, "Python 3.8.10", 0);
        assert_eq!(host.take(), ["doctor 4"]);
        assert_eq!(
            labels(&host.checks),
            [
                ("Python".into(), Health::Bad, "3.8.10, but the project needs >=3.9 (/usr/bin/python3)".into(), None),
                ("uv".into(), Health::Bad, "uv isn't installed (or isn't on PATH)".into(), Some("python.install-uv".into())),
                ("Language server".into(), Health::Warn, "basedpyright isn't installed (or isn't on PATH)".into(), Some("python.install-server".into())),
                ("venv".into(), Health::Ok, "in step with uv.lock".into(), None),
            ]
        );
        assert_eq!(d.fix_for("python.install-uv").map(|(p, f)| (p.name.as_str(), f.clone())), Some(("uv", Fix::Task(uv()))));
        assert!(d.fix_for("nope").is_none());
        assert!(!d.handle(&mut host, &Event::Timer(2)), "its timer is done with");
    }

    #[test]
    fn found_and_new_enough_is_ok_and_has_no_fix_row() {
        let mut host = Mock::default();
        host.programs.insert("python3".into(), "/usr/bin/python3".into());
        host.programs.insert("uv".into(), "/home/u/.cargo/bin/uv".into());
        host.programs.insert("basedpyright".into(), "/tools/bp/bin/basedpyright".into());
        let mut d = doctor();
        d.run(&mut host);
        host.take();
        said(&mut d, &mut host, 2, "uv 0.4.18 (7b55e9790 2024-10-01)", 0);
        assert!(host.take().is_empty(), "waits for the rest");
        said(&mut d, &mut host, 1, "Python 3.12.1", 0);
        let l = labels(&host.checks);
        assert_eq!(l[0], ("Python".into(), Health::Ok, "3.12.1 (/usr/bin/python3)".into(), None));
        assert_eq!(l[1], ("uv".into(), Health::Ok, "0.4.18 (/home/u/.cargo/bin/uv)".into(), None));
        assert_eq!(l[2], ("Language server".into(), Health::Ok, "/tools/bp/bin/basedpyright".into(), None));
        assert_eq!(d.found.get("uv").map(String::as_str), Some("/home/u/.cargo/bin/uv"));
    }

    #[test]
    fn one_that_doesnt_answer_in_time_is_a_warning() {
        let mut host = Mock::default();
        host.programs.insert("python3".into(), "/usr/bin/python3".into());
        let mut d = doctor();
        d.run(&mut host);
        host.take();
        assert!(d.handle(&mut host, &Event::Timer(2)));
        assert_eq!(host.take(), ["kill 1", "doctor 4"]);
        assert_eq!(host.checks[0].health, Health::Warn);
        assert!(host.checks[0].detail.contains("didn't say its version within 5 s"));
    }

    #[test]
    fn a_version_that_cant_be_read_is_a_warning() {
        let p = Program::new("x", "X");
        let c = p.evaluate(Some("/x"), Some(&Output { code: Some(0), stdout: "hello".into(), stderr: String::new() }));
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "/x: its version couldn't be read"));
        let c = p.evaluate(Some("/x"), Some(&Output { code: Some(2), stdout: String::new(), stderr: "bad flag".into() }));
        assert_eq!(c.detail, "/x: asking its version failed: bad flag");
        // A version said on stderr (java -version) is read.
        let c = Program::new("java", "JDK").at_least("17").evaluate(Some("/j"), Some(&Output { code: Some(0), stdout: String::new(), stderr: "openjdk version \"21.0.2\" 2024-01-16".into() }));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "21.0.2 (/j)"));
        // What's asked can change once the project's read (go.mod).
        let mut d = doctor();
        d.get_mut("python3").unwrap().wants = Requirement::parse(">=3.13");
        assert!(!d.programs[0].wants.as_ref().unwrap().matches(&crate::version::Version::parse("3.12").unwrap()));
        assert_eq!(Program::new("x", "X").wants("not a range").wants, None);
    }

    fn in_step(out: &Output) -> (Health, String) {
        if out.text().contains("Would make no changes") {
            (Health::Ok, "in step".into())
        } else {
            (Health::Warn, out.why())
        }
    }

    #[test]
    fn a_program_checked_twice_is_judged_by_the_pack() {
        let mut host = Mock::default();
        host.programs.insert("uv".into(), "/bin/uv".into());
        let mut d = Doctor::new();
        d.program(Program::new("uv", "uv"));
        d.program(Program::new("uv", "Environment").version_args(["sync", "--dry-run"]).judge(in_step).fix("Sync it", "python.sync", Fix::Task(uv())));
        d.program(Program::new("uv", "uv").at_least("0.4"));
        assert_eq!(d.programs.len(), 2, "the same name and label is replaced");
        assert!(d.programs[1].wants.is_some());
        d.run(&mut host);
        assert_eq!(host.take(), ["spawn 1 /bin/uv sync --dry-run", "close 1", "spawn 2 /bin/uv --version", "close 2", "timer 3 5000"]);
        said(&mut d, &mut host, 2, "uv 0.5.1", 0);
        said(&mut d, &mut host, 1, " + idna==2.10", 0);
        assert_eq!(
            labels(&host.checks),
            [("Environment".into(), Health::Warn, "+ idna==2.10".into(), Some("python.sync".into())), ("uv".into(), Health::Ok, "0.5.1 (/bin/uv)".into(), None)]
        );
        d.run(&mut host);
        host.take();
        said(&mut d, &mut host, 4, "Would make no changes", 0);
        said(&mut d, &mut host, 5, "uv 0.5.1", 0);
        assert_eq!(labels(&host.checks)[0], ("Environment".into(), Health::Ok, "in step".into(), None));
    }

    #[test]
    fn a_judge_carries_what_it_expects_and_a_program_its_env_and_time() {
        let mut host = Mock::default();
        host.programs.insert("go".into(), "/bin/go".into());
        let wanted = String::from("go1.23");
        let mut d = Doctor::new();
        d.program(Program::new("go", "Go").version_args(["version"]).env("GOTOOLCHAIN", "auto").cwd("/project/svc").timeout(60_000).judge(move |out: &Output| {
            if out.stdout.contains(&wanted) {
                (Health::Ok, wanted.clone())
            } else {
                (Health::Warn, format!("not {wanted}"))
            }
        }));
        d.program(Program::new("go", "gofmt").version_args(["fmt", "-h"]));
        d.run(&mut host);
        assert_eq!(host.take(), ["spawn 1 /bin/go version", "close 1", "spawn 2 /bin/go fmt -h", "close 2", "timer 3 5000", "timer 4 60000"]);
        let cmd = d.programs[0].command("/bin/go");
        assert_eq!((cmd.env.clone(), cmd.cwd.as_deref()), (vec![("GOTOOLCHAIN".to_string(), "auto".to_string())], Some("/project/svc")));
        // gofmt's 5 s are up; Go's minute isn't.
        assert!(d.handle(&mut host, &Event::Timer(3)));
        assert_eq!(host.take(), ["kill 2"], "the rest still waited for");
        said(&mut d, &mut host, 1, "go version go1.23.4 linux/amd64", 0);
        assert_eq!(labels(&host.checks), [("Go".into(), Health::Ok, "go1.23".into(), None), ("gofmt".into(), Health::Warn, "go didn't say its version within 5 s".into(), None)]);
        assert!(!d.handle(&mut host, &Event::Timer(4)), "done with");
        assert!(format!("{:?}", d.programs[0]).contains("judge: true"));
    }

    #[test]
    fn the_packs_own_checks_are_worked_out_again_each_run() {
        let mut host = Mock::default();
        let runs = Rc::new(std::cell::Cell::new(0));
        let counted = runs.clone();
        let mut d = Doctor::new();
        d.refresh = Some(Box::new(move || {
            counted.set(counted.get() + 1);
            vec![Check::new(Section::Project, "node_modules", Health::Ok, format!("looked at {} times", counted.get()))]
        }));
        d.run(&mut host);
        d.run(&mut host);
        assert_eq!(runs.get(), 2);
        assert_eq!(host.checks[0].detail, "looked at 2 times");
    }
}
