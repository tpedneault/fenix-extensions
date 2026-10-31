//! Programs run for what they print: started with their stdin closed,
//! their lines kept until they end, then given back whole.

use std::collections::HashMap;

use fenix_extension_api::{Command, Event};

use crate::host::Host;

/// What a program printed, and how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Its exit code; none when it was killed, or couldn't start.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Exit code 0.
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// Its stdout read as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_str(self.stdout.trim()).map_err(|e| format!("its output isn't what was expected: {e}"))
    }

    /// Its stdout as one JSON value after another (`go list -m -json
    /// all`, `govulncheck -json`, one object a line): those that read.
    pub fn json_stream(&self) -> Vec<serde_json::Value> {
        serde_json::Deserializer::from_str(&self.stdout).into_iter::<serde_json::Value>().map_while(Result::ok).collect()
    }

    /// Its stdout, then its stderr: a `--version` says it on either.
    pub fn text(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }

    /// Why it failed, in a line: its stderr's last line that says
    /// something, or its exit code.
    pub fn why(&self) -> String {
        let last = |s: &str| s.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
        let said = last(&self.stderr).or_else(|| last(&self.stdout));
        match (said, self.code) {
            (Some(l), _) => l,
            (None, Some(c)) => format!("exit code {c}"),
            (None, None) => "it was stopped".into(),
        }
    }
}

/// Programs running, each for a `J`: what it was started for.
#[derive(Debug)]
pub struct Runs<J> {
    running: HashMap<u64, (J, Output)>,
}

impl<J> Default for Runs<J> {
    fn default() -> Runs<J> {
        Runs { running: HashMap::new() }
    }
}

impl<J: Clone> Runs<J> {
    pub fn new() -> Runs<J> {
        Runs::default()
    }

    /// Starts `cmd` for `job`, its stdin closed so a program that reads it
    /// doesn't wait. Its id, or why it couldn't start.
    pub fn start(&mut self, host: &mut dyn Host, cmd: &Command, job: J) -> Result<u64, String> {
        let id = host.spawn(cmd)?;
        host.close_stdin(id);
        self.running.insert(id, (job, Output::default()));
        Ok(id)
    }

    /// An event for one of its programs: a line kept, or the end given
    /// back with all it printed. `None` for anything else.
    pub fn event(&mut self, event: &Event) -> Option<(J, Output)> {
        match event {
            Event::ProcessOutput(o) => {
                if let Some((_, out)) = self.running.get_mut(&o.process) {
                    let to = if o.stderr { &mut out.stderr } else { &mut out.stdout };
                    if !to.is_empty() {
                        to.push('\n');
                    }
                    to.push_str(&o.line);
                }
                None
            }
            Event::ProcessExit((id, code)) => {
                let (job, mut out) = self.running.remove(id)?;
                out.code = *code;
                Some((job, out))
            }
            _ => None,
        }
    }

    /// Whether `event` is one of its programs'.
    pub fn owns(&self, event: &Event) -> bool {
        match event {
            Event::ProcessOutput(o) => self.running.contains_key(&o.process),
            Event::ProcessExit((id, _)) => self.running.contains_key(id),
            _ => false,
        }
    }

    /// Stops them all, forgetting them.
    pub fn kill_all(&mut self, host: &mut dyn Host) -> Vec<J> {
        self.kill_where(host, |_| true)
    }

    /// Stops those `which` picks, forgetting them, in the order they
    /// started.
    pub fn kill_where(&mut self, host: &mut dyn Host, which: impl Fn(&J) -> bool) -> Vec<J> {
        let mut ids: Vec<u64> = self.running.iter().filter(|(_, (j, _))| which(j)).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let mut jobs = Vec::new();
        for id in ids {
            host.kill(id);
            jobs.extend(self.running.remove(&id).map(|(j, _)| j));
        }
        jobs
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub fn len(&self) -> usize {
        self.running.len()
    }

    /// What's running for.
    pub fn jobs(&self) -> impl Iterator<Item = &J> {
        self.running.values().map(|(j, _)| j)
    }
}

/// Words of a command line, as a shell splits them: by spaces, `'...'`
/// and `"..."` holding theirs, `\` the next character.
pub fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => cur.push(c),
            (_, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                started = true;
            }
            (_, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(cur);
    }
    words
}

/// `line` as a command: its first word the program.
pub fn command(line: &str) -> Command {
    let words = split_words(line);
    let mut words = words.into_iter();
    Command::new(&words.next().unwrap_or_default()).args(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::mock::Mock;
    use fenix_extension_api::ProcessOutput;

    fn out(process: u64, stderr: bool, line: &str) -> Event {
        Event::ProcessOutput(ProcessOutput { process, stderr, line: line.into() })
    }

    #[test]
    fn a_run_gives_back_all_it_printed() {
        let mut host = Mock::default();
        let mut runs: Runs<&str> = Runs::new();
        let a = runs.start(&mut host, &Command::new("uv").args(["pip", "list"]), "list").unwrap();
        let b = runs.start(&mut host, &Command::new("pip-audit"), "audit").unwrap();
        assert_eq!(host.take(), ["spawn 1 uv pip list", "close 1", "spawn 2 pip-audit", "close 2"]);
        assert!(runs.owns(&out(a, false, "x")) && !runs.owns(&out(9, false, "x")));
        assert_eq!(runs.event(&out(a, false, "[{\"name\":")), None);
        assert_eq!(runs.event(&out(b, true, "warning")), None);
        assert_eq!(runs.event(&out(a, false, "\"x\"}]")), None);
        let (job, o) = runs.event(&Event::ProcessExit((a, Some(0)))).unwrap();
        assert_eq!((job, o.stdout.as_str(), o.ok()), ("list", "[{\"name\":\n\"x\"}]", true));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs.kill_all(&mut host), ["audit"]);
        assert_eq!(host.take(), ["kill 2"]);
        assert!(runs.is_empty());
        assert_eq!(runs.event(&Event::ProcessExit((b, None))), None, "a killed one is forgotten");
    }

    #[test]
    fn output_reads_as_json_and_says_why_it_failed() {
        let o = Output { code: Some(1), stdout: "{\"a\":1}\n{\"a\":2}\nnot json".into(), stderr: "error: no lockfile\n\n".into() };
        assert_eq!(o.json_stream().len(), 2);
        assert!(o.json::<serde_json::Value>().is_err());
        assert_eq!(o.why(), "error: no lockfile");
        assert_eq!(Output { code: Some(2), ..Output::default() }.why(), "exit code 2");
        assert_eq!(Output::default().why(), "it was stopped");
        assert_eq!(Output { stdout: "a".into(), stderr: "b".into(), ..Output::default() }.text(), "a\nb");
    }

    #[test]
    fn command_lines_split_as_a_shell_would() {
        assert_eq!(split_words(r#"uv run  pytest -k "not slow" 'a b' c\ d """#), ["uv", "run", "pytest", "-k", "not slow", "a b", "c d", ""]);
        let c = command("cargo test --workspace");
        assert_eq!((c.program.as_str(), c.args.len()), ("cargo", 2));
        assert_eq!(command("").program, "");
    }
}
