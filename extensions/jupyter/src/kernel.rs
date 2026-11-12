//! The kernel a notebook runs on, through `fenix_kernel.py`: a program
//! of the extension's own, run by the notebook's Python, that starts a
//! Jupyter kernel (ipykernel, through jupyter_client) when that Python
//! has one, and otherwise runs cells itself -- so a notebook runs on a
//! Python with nothing installed.
//!
//! They talk in JSON lines: what to do on its stdin, what happened on its
//! stdout. Its stdout is kept for them: what a cell's programs print
//! goes to its stderr, and comes to the cell as its stderr.

use serde::Deserialize;
use serde_json::{json, Value};

/// The program, written to the extension's folder as it starts.
pub const SCRIPT: &str = include_str!("fenix_kernel.py");
/// Its matplotlib backend, for when it runs cells itself: figures shown
/// as pictures, as Jupyter's inline backend does.
pub const BACKEND: &str = include_str!("fenix_mpl.py");

/// What it said.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "ev", rename_all = "lowercase")]
pub enum Said {
    /// Ready for cells: what it runs them on.
    Ready {
        label: String,
        #[serde(default)]
        mode: String,
        #[serde(default)]
        note: String,
    },
    /// An output for the cell it's running, as nbformat has it.
    Output { id: u64, output: Value },
    /// `clear_output()`: now, or with the next output (`wait`).
    Clear {
        id: u64,
        #[serde(default)]
        wait: bool,
    },
    /// `input()`: a line of yours, asked for.
    Input {
        id: u64,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        password: bool,
    },
    /// The cell's run is over: its count, and `ok`, `error` or `aborted`.
    Done {
        id: u64,
        count: Option<u64>,
        #[serde(default)]
        status: String,
    },
    /// The kernel stopped, and why; it's started again for the next cell.
    Died {
        #[serde(default)]
        why: String,
    },
    /// Something for its log.
    Log { text: String },
}

impl Said {
    pub fn parse(line: &str) -> Option<Said> {
        serde_json::from_str(line.trim()).ok()
    }
}

/// What to tell it, as a line.
pub fn run(id: u64, code: &str) -> String {
    line(json!({"op": "run", "id": id, "code": code}))
}

pub fn input(text: &str) -> String {
    line(json!({"op": "input", "text": text}))
}

pub fn op(name: &str) -> String {
    line(json!({ "op": name }))
}

fn line(v: Value) -> String {
    let mut s = v.to_string();
    s.push('\n');
    s
}

/// Where the kernel is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum State {
    /// Not started: the first cell run starts it.
    #[default]
    Off,
    Starting,
    Idle,
    Busy,
    /// It ended, and why.
    Dead(String),
}

impl State {
    pub fn label(&self) -> String {
        match self {
            State::Off => "not started".into(),
            State::Starting => "starting…".into(),
            State::Idle => "idle".into(),
            State::Busy => "busy".into(),
            State::Dead(why) => format!("stopped: {why}"),
        }
    }
}

/// The Pythons there are to run it with, as found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// `jupyter.python`: the one you chose.
    pub chosen: Option<String>,
    /// The project's `.venv`'s.
    pub venv: Option<String>,
    pub uv: Option<String>,
    /// `python3`, or `python`, on `PATH`.
    pub path: Option<String>,
    /// Whether the project is uv's to run (a `pyproject.toml`).
    pub pyproject: bool,
}

/// How to start it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    /// Before the script and its arguments.
    pub args: Vec<String>,
    /// Which Python, for the header: `.venv`, `uv`, the path.
    pub label: String,
    /// The Python itself, to install ipykernel into; none when uv picks it.
    pub python: Option<String>,
    /// Whether it runs in the project, which uv needs.
    pub in_project: bool,
}

/// How to run the script, by what's found: the Python you chose, then
/// the project's `.venv`, then uv in a project of its (ipykernel brought
/// for the run), then the Python on `PATH`.
pub fn launch(found: &Found) -> Result<Launch, String> {
    let python = |p: &String, label: &str| Launch { program: p.clone(), args: Vec::new(), label: label.into(), python: Some(p.clone()), in_project: false };
    if let Some(p) = &found.chosen {
        return Ok(python(p, p));
    }
    if let Some(p) = &found.venv {
        return Ok(python(p, ".venv"));
    }
    if let (Some(uv), true) = (&found.uv, found.pyproject) {
        return Ok(Launch { program: uv.clone(), args: ["run", "--with", "ipykernel", "python"].map(String::from).to_vec(), label: "uv run".into(), python: None, in_project: true });
    }
    if let Some(p) = &found.path {
        return Ok(python(p, p.rsplit(['/', '\\']).next().unwrap_or(p)));
    }
    Err("no Python found: install one, or set jupyter.python (SPC ,)".into())
}

/// How to install ipykernel for `launch`'s Python, so cells run on a
/// real Jupyter kernel: uv for a project of its, else that Python's pip.
pub fn install_ipykernel(launch: &Launch, found: &Found) -> Option<(String, Vec<String>)> {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if let (Some(uv), true, None) = (&found.uv, found.pyproject, &found.chosen) {
        return Some((uv.clone(), args(&["add", "--dev", "ipykernel"])));
    }
    let py = launch.python.clone()?;
    if found.venv.as_deref() == Some(py.as_str()) {
        // uv's venvs have no pip of their own.
        if let Some(uv) = &found.uv {
            return Some((uv.clone(), vec!["pip".into(), "install".into(), "--python".into(), py, "ipykernel".into()]));
        }
        return Some((py, args(&["-m", "pip", "install", "ipykernel"])));
    }
    Some((py, args(&["-m", "pip", "install", "--user", "ipykernel"])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_it_says_is_read() {
        assert_eq!(Said::parse(r#"{"ev": "ready", "label": "Python 3 (ipykernel)", "mode": "jupyter"}"#), Some(Said::Ready { label: "Python 3 (ipykernel)".into(), mode: "jupyter".into(), note: String::new() }));
        assert_eq!(Said::parse(r#"{"ev": "done", "id": 3, "count": 7, "status": "ok"}"#), Some(Said::Done { id: 3, count: Some(7), status: "ok".into() }));
        assert_eq!(Said::parse(r#"{"ev": "done", "id": 3, "count": null}"#), Some(Said::Done { id: 3, count: None, status: String::new() }));
        assert!(matches!(Said::parse(r#"{"ev": "output", "id": 1, "output": {"output_type": "stream"}}"#), Some(Said::Output { id: 1, .. })));
        assert_eq!(Said::parse("Traceback (most recent call last):"), None);
    }

    #[test]
    fn what_it_is_told_is_a_line_each() {
        assert_eq!(run(4, "print('a')\nx"), "{\"code\":\"print('a')\\nx\",\"id\":4,\"op\":\"run\"}\n");
        assert_eq!(op("interrupt"), "{\"op\":\"interrupt\"}\n");
    }

    #[test]
    fn the_python_is_yours_then_the_projects_then_uvs_then_paths() {
        let mut f = Found { chosen: None, venv: None, uv: Some("/u/uv".into()), path: Some("/usr/bin/python3".into()), pyproject: false };
        assert_eq!(launch(&f).unwrap().program, "/usr/bin/python3", "uv only runs a project's");
        f.pyproject = true;
        let uv = launch(&f).unwrap();
        assert_eq!((uv.program.as_str(), uv.args.join(" ").as_str(), uv.in_project), ("/u/uv", "run --with ipykernel python", true));
        f.venv = Some("/p/.venv/bin/python".into());
        assert_eq!(launch(&f).unwrap().label, ".venv");
        f.chosen = Some("/opt/py/bin/python".into());
        assert_eq!(launch(&f).unwrap().program, "/opt/py/bin/python");
        assert!(launch(&Found::default()).unwrap_err().contains("jupyter.python"));
    }

    #[test]
    fn ipykernel_goes_where_the_python_is() {
        let venv = Found { venv: Some("/p/.venv/bin/python".into()), uv: Some("/u/uv".into()), ..Found::default() };
        assert_eq!(install_ipykernel(&launch(&venv).unwrap(), &venv), Some(("/u/uv".into(), "pip install --python /p/.venv/bin/python ipykernel".split(' ').map(String::from).collect())));
        let project = Found { uv: Some("/u/uv".into()), pyproject: true, ..Found::default() };
        assert_eq!(install_ipykernel(&launch(&project).unwrap(), &project).unwrap().1.join(" "), "add --dev ipykernel");
        let plain = Found { path: Some("python3".into()), ..Found::default() };
        assert_eq!(install_ipykernel(&launch(&plain).unwrap(), &plain).unwrap().1.join(" "), "-m pip install --user ipykernel");
    }
}
