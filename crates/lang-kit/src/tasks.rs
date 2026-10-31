//! A pack's tasks from a small table -- a name and a command line each
//! -- with an env file's values, set in the task picker (`tasks-set`).

use fenix_extension_api::{Command, Task};

use crate::exec::command;
use crate::host::Host;

/// The tasks a pack offers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tasks {
    pub tasks: Vec<Task>,
    /// Given to every task (an env file's).
    pub env: Vec<(String, String)>,
}

impl Tasks {
    /// From `(name, command line)` rows: `("test", "uv run pytest")`.
    pub fn table(rows: &[(&str, &str)]) -> Tasks {
        let mut t = Tasks::default();
        for (name, line) in rows {
            t.add(name, command(line));
        }
        t
    }

    /// One more, or in place of the one of that name.
    pub fn add(&mut self, name: &str, cmd: Command) {
        self.tasks.retain(|t| t.name != name);
        self.tasks.push(Task { name: name.into(), cmd });
    }

    /// The same, as a row.
    pub fn line(mut self, name: &str, line: &str) -> Tasks {
        self.add(name, command(line));
        self
    }

    /// Each runs in `dir` (under `/project`), unless it says where.
    pub fn cwd(mut self, dir: &str) -> Tasks {
        for t in &mut self.tasks {
            if t.cmd.cwd.is_none() {
                t.cmd.cwd = Some(dir.into());
            }
        }
        self
    }

    /// An env file's values (`/project/.env`), given to each: read if
    /// it's there, nothing if not.
    pub fn env_file(mut self, path: &str) -> Tasks {
        if let Ok(text) = std::fs::read_to_string(path) {
            self.env = parse_env(&text);
        }
        self
    }

    /// The tasks as Fenix takes them: each with the env, its own values
    /// winning.
    pub fn resolved(&self) -> Vec<Task> {
        self.tasks
            .iter()
            .map(|t| {
                let mut t = t.clone();
                let own: Vec<String> = t.cmd.env.iter().map(|(k, _)| k.clone()).collect();
                let mut env: Vec<(String, String)> = self.env.iter().filter(|(k, _)| !own.contains(k)).cloned().collect();
                env.append(&mut t.cmd.env);
                t.cmd.env = env;
                t
            })
            .collect()
    }

    /// Set in the task picker.
    pub fn apply(&self, host: &mut dyn Host) -> Result<(), String> {
        host.tasks(&self.resolved())
    }
}

/// An env file's `KEY=value` lines: `export` before one, `#` comments,
/// quotes taken off (`"..."` with `\n` and `\"` in it), a value
/// referring to an earlier one (`${KEY}`, `$KEY`) filled in.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim();
        let value = if let Some(v) = value.strip_prefix('"').and_then(|v| v.rfind('"').map(|end| &v[..end])) {
            fill(&v.replace("\\n", "\n").replace("\\\"", "\""), &out)
        } else if let Some(v) = value.strip_prefix('\'').and_then(|v| v.rfind('\'').map(|end| &v[..end])) {
            v.to_string()
        } else {
            // A comment after a space ends an unquoted value.
            let v = value.split(" #").next().unwrap_or(value).trim();
            fill(v, &out)
        };
        out.retain(|(k, _)| k != key);
        out.push((key.to_string(), value));
    }
    out
}

fn fill(value: &str, known: &[(String, String)]) -> String {
    let get = |name: &str| known.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default();
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(inner) = after.strip_prefix('{') {
            if let Some(end) = inner.find('}') {
                out.push_str(&get(&inner[..end]));
                rest = &inner[end + 1..];
                continue;
            }
        }
        let n = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        if n == 0 {
            out.push('$');
        } else {
            out.push_str(&get(&after[..n]));
        }
        rest = &after[n..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::mock::Mock;

    #[test]
    fn tasks_come_from_a_table() {
        let t = Tasks::table(&[("test", "uv run pytest -q"), ("lint", "ruff check ."), ("test", "uv run pytest")]).line("format", "ruff format .").cwd("/project/app");
        let mut host = Mock::default();
        t.apply(&mut host).unwrap();
        assert_eq!(host.take(), ["tasks lint, test, format"]);
        let test = &host.tasks[1];
        assert_eq!((test.cmd.program.as_str(), test.cmd.args.clone(), test.cmd.cwd.as_deref()), ("uv", vec!["run".to_string(), "pytest".into()], Some("/project/app")));
    }

    #[test]
    fn an_env_file_goes_to_each_but_a_tasks_own_value_wins() {
        let mut t = Tasks::table(&[("run", "python app.py")]);
        t.tasks[0].cmd.env.push(("DEBUG".into(), "0".into()));
        t.env = parse_env("DEBUG=1\nPORT=8000");
        assert_eq!(t.resolved()[0].cmd.env, [("PORT".to_string(), "8000".to_string()), ("DEBUG".into(), "0".into())]);
        let missing = Tasks::default().env_file("/no/such/.env");
        assert!(missing.env.is_empty());
    }

    #[test]
    fn env_files_read_as_dotenv_reads_them() {
        let env = parse_env(
            "# a comment\n\
             export HOST=localhost\n\
             PORT = 8000 # the port\n\
             URL=\"http://${HOST}:$PORT/x\"\n\
             RAW='$HOST stays'\n\
             MULTI=\"a\\nb \\\"c\\\"\"\n\
             bad line\n\
             1-BAD=x\n\
             PORT=9000\n\
             COST=$5 and $\n",
        );
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("HOST"), Some("localhost"));
        assert_eq!(get("PORT"), Some("9000"));
        assert_eq!(get("URL"), Some("http://localhost:8000/x"));
        assert_eq!(get("RAW"), Some("$HOST stays"));
        assert_eq!(get("MULTI"), Some("a\nb \"c\""));
        assert_eq!(get("1-BAD"), None);
        assert_eq!(get("COST"), Some(" and $"));
        assert_eq!(env.len(), 6);
    }
}
