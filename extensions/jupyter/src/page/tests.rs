use super::*;
use crate::edit::apply;

const SAVED: &str = include_str!("../../tests/fixtures/assignment.ipynb");

/// Fenix, as far as a page sees it: a buffer, a kernel's lines, what was
/// said.
#[derive(Default)]
struct Fake {
    buffer: String,
    edits: usize,
    saved: bool,
    said: Vec<String>,
    logged: Vec<String>,
    written: Vec<String>,
    scratches: Vec<(u64, String, String, String)>,
    closed: Vec<u64>,
    started: usize,
    stopped: Vec<u64>,
    asked: Vec<String>,
    pictures: usize,
    no_python: bool,
}

impl World for Fake {
    fn buffer_text(&mut self, _path: &str) -> Option<String> {
        Some(self.buffer.clone())
    }
    fn edit(&mut self, _path: &str, edit: TextEdit) -> Result<(), String> {
        self.buffer = apply(&self.buffer, &edit);
        self.edits += 1;
        Ok(())
    }
    fn save(&mut self, _path: &str) -> Result<(), String> {
        self.saved = true;
        Ok(())
    }
    fn undo(&mut self, _path: &str, _redo: bool) -> Result<(), String> {
        Err("no undo here".into())
    }
    fn copy(&mut self, text: &str) {
        self.said.push(format!("copied {text}"));
    }
    fn say(&mut self, text: &str) {
        self.said.push(text.into());
    }
    fn log(&mut self, text: &str) {
        self.logged.push(text.into());
    }
    fn open_text(&mut self, path: &str) {
        self.said.push(format!("text of {path}"));
    }
    fn picture(&mut self, _bytes: &[u8]) -> Option<u32> {
        self.pictures += 1;
        Some(7)
    }
    fn edit_cell(&mut self, title: &str, text: &str, language: &str) -> u64 {
        let id = 100 + self.scratches.len() as u64;
        self.scratches.push((id, title.into(), text.into(), language.into()));
        id
    }
    fn close_scratch(&mut self, scratch: u64) {
        self.closed.push(scratch);
    }
    fn start_kernel(&mut self, _full: &str, _kernel: Option<&str>) -> Result<(u64, String), String> {
        if self.no_python {
            return Err("no Python found".into());
        }
        self.started += 1;
        Ok((9, ".venv".into()))
    }
    fn write(&mut self, _process: u64, line: &str) -> Result<(), String> {
        self.written.push(line.trim_end().into());
        Ok(())
    }
    fn stop(&mut self, process: u64) {
        self.stopped.push(process);
    }
    fn ask(&mut self, prompt: &str, _password: bool) -> u64 {
        self.asked.push(prompt.into());
        55
    }
    fn install_kernel(&mut self, full: &str) {
        self.said.push(format!("install for {full}"));
    }
}

fn open() -> (NotebookPage, Fake) {
    let fake = Fake { buffer: SAVED.into(), ..Fake::default() };
    (NotebookPage::new("/home/me/hw/assignment.ipynb", "/project/hw/assignment.ipynb", SAVED.into()), fake)
}

fn screen(p: &mut NotebookPage, w: &mut Fake) -> String {
    p.render(80, 60, w).text()
}

fn notebook(w: &Fake) -> Notebook {
    Notebook::parse(&w.buffer).unwrap()
}

#[test]
fn it_shows_cells_and_their_outputs() {
    let (mut p, mut w) = open();
    let s = screen(&mut p, &mut w);
    for want in ["assignment.ipynb", "Python 3 (ipykernel)", "5 cells", "Assignment 2: circles", "Work out the area of a circle", "• radius in metres", "[1]:", "math.pi * radius ** 2", "Out[1]:", "12.566370614359172", "line 2", "ZeroDivisionError: division by zero"] {
        assert!(s.contains(want), "no {want:?} in\n{s}");
    }
    assert!(!s.contains("\u{1b}"), "colours left in\n{s}");
    assert_eq!(w.pictures, 1, "the plot is drawn");
    assert_eq!(w.edits, 0, "looking changes nothing");
}

#[test]
fn a_cells_source_is_edited_beside_and_written_to_the_buffer() {
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Enter, &mut w);
    let (scratch, title, text, language) = w.scratches[0].clone();
    assert_eq!((title.as_str(), text.as_str(), language.as_str()), ("assignment.ipynb · cell 2", "import math\nradius = 2\nmath.pi * radius ** 2", "py"));
    p.scratch_changed(scratch, "import math\nradius = 3\nmath.pi * radius ** 2", &mut w);
    assert_eq!(w.edits, 1);
    assert_eq!(notebook(&w).source(1), "import math\nradius = 3\nmath.pi * radius ** 2");
    // Only what changed: the rest of the file is as Jupyter wrote it.
    assert_eq!(w.buffer, SAVED.replacen("\"radius = 2\\n\"", "\"radius = 3\\n\"", 1));
    assert!(screen(&mut p, &mut w).contains("radius = 3"));
    assert!(screen(&mut p, &mut w).contains("not saved"));
}

#[test]
fn cells_are_made_moved_turned_and_deleted() {
    let (mut p, mut w) = open();
    p.key(Key::Char('b'), &mut w);
    assert_eq!(notebook(&w).len(), 6);
    assert_eq!((p.focus, notebook(&w).kind(1)), (1, Kind::Code));
    assert_eq!(w.scratches.len(), 1, "a new cell opens to write in");
    p.key(Key::Char('m'), &mut w);
    assert_eq!(notebook(&w).kind(1), Kind::Markdown);
    p.key(Key::Char(']'), &mut w);
    assert_eq!((p.focus, notebook(&w).kind(2)), (2, Kind::Markdown));
    p.key(Key::Char('d'), &mut w);
    p.key(Key::Char('d'), &mut w);
    assert_eq!(notebook(&w).len(), 5);
    p.key(Key::Char('V'), &mut w);
    assert_eq!((notebook(&w).len(), notebook(&w).kind(2)), (6, Kind::Markdown), "the cell cut is pasted back");
    assert_eq!(notebook(&w).to_text(), w.buffer);
}

#[test]
fn a_cell_runs_and_its_outputs_are_written() {
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    assert_eq!(w.started, 1, "the first run starts the kernel");
    assert!(screen(&mut p, &mut w).contains("[*]:"));
    assert!(notebook(&w).outputs(1).is_empty(), "its old outputs go at once");
    p.said(r#"{"ev": "ready", "label": "Python 3 (ipykernel)", "mode": "jupyter"}"#, &mut w);
    let id = p.ids[1];
    assert_eq!(w.written.last().unwrap(), &format!("{{\"code\":\"import math\\nradius = 2\\nmath.pi * radius ** 2\",\"id\":{id},\"op\":\"run\"}}"));
    p.said(&format!(r#"{{"ev": "output", "id": {id}, "output": {{"output_type": "stream", "name": "stdout", "text": "hello\n"}}}}"#), &mut w);
    p.said_stderr("from a program", &mut w);
    assert!(screen(&mut p, &mut w).contains("hello"));
    p.said(&format!(r#"{{"ev": "done", "id": {id}, "count": 1, "status": "ok"}}"#), &mut w);
    let nb = notebook(&w);
    assert_eq!(nb.count(1), Some(1));
    assert_eq!(nb.outputs(1).len(), 2);
    assert_eq!(nb.outputs(1)[1]["text"], serde_json::json!(["from a program\n"]));
    assert!(screen(&mut p, &mut w).contains("idle"));
}

#[test]
fn run_all_stops_at_an_error() {
    let (mut p, mut w) = open();
    p.key(Key::Char('R'), &mut w);
    p.said(r#"{"ev": "ready", "label": "Python 3", "mode": "builtin", "note": "jupyter_client isn't installed"}"#, &mut w);
    let first = p.ids[1];
    assert!(w.written.last().unwrap().contains(&format!("\"id\":{first}")), "markdown cells are skipped");
    p.said(&format!(r#"{{"ev": "done", "id": {first}, "count": 1, "status": "error"}}"#), &mut w);
    assert_eq!(w.written.len(), 1, "nothing after the error runs");
    assert!(screen(&mut p, &mut w).contains("P installs it"), "the built-in kernel says how to get IPython");
}

#[test]
fn input_is_asked_and_answered() {
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    p.said(r#"{"ev": "ready", "label": "x"}"#, &mut w);
    let id = p.ids[1];
    p.said(&format!(r#"{{"ev": "input", "id": {id}, "prompt": "name? "}}"#), &mut w);
    assert_eq!(w.asked, ["name?"]);
    assert_eq!(p.asking(), Some(55));
    p.answered(Some("Ada".into()), &mut w);
    assert_eq!(w.written.last().unwrap(), r#"{"op":"input","text":"Ada"}"#);
}

#[test]
fn interrupt_and_restart_are_jupyters_keys() {
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    p.said(r#"{"ev": "ready", "label": "x"}"#, &mut w);
    p.key(Key::Char('i'), &mut w);
    p.key(Key::Char('i'), &mut w);
    assert_eq!(w.written.last().unwrap(), r#"{"op":"interrupt"}"#);
    p.key(Key::Char('0'), &mut w);
    p.key(Key::Char('0'), &mut w);
    assert_eq!(w.written.last().unwrap(), r#"{"op":"restart"}"#);
    assert!(screen(&mut p, &mut w).contains("starting"));
}

#[test]
fn a_kernel_that_ends_says_so_and_starts_again() {
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    p.said(r#"{"ev": "ready", "label": "x"}"#, &mut w);
    p.exited(Some(1), &mut w);
    assert!(screen(&mut p, &mut w).contains("stopped: it ended (1); SPC h l has why"), "no Fenix to ask: Vim's keys");
    assert_eq!(notebook(&w).count(1), None);
    p.key(Key::Char('r'), &mut w);
    assert_eq!(w.started, 2);
}

#[test]
fn no_python_says_why() {
    let (mut p, mut w) = open();
    w.no_python = true;
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    let s = screen(&mut p, &mut w);
    assert!(s.contains("stopped: no Python found"), "{s}");
    assert!(!s.contains("[*]"), "nothing waits for it");
}

#[test]
fn a_change_in_its_text_is_read_again() {
    let (mut p, mut w) = open();
    w.buffer = SAVED.replace("Assignment 2: circles", "Assignment 3: squares");
    assert!(p.changed(Some(w.buffer.clone()), &mut w));
    assert!(screen(&mut p, &mut w).contains("Assignment 3: squares"));
    assert!(!p.changed(Some(w.buffer.clone()), &mut w), "the same text again is nothing");
}

#[test]
fn what_isnt_a_notebook_offers_its_text() {
    let mut w = Fake { buffer: "{ oops".into(), ..Fake::default() };
    let mut p = NotebookPage::new("/x/broken.ipynb", "/x/broken.ipynb", "{ oops".into());
    let s = screen(&mut p, &mut w);
    assert!(s.contains("not a notebook") && s.contains("t opens its text"), "{s}");
    p.key(Key::Char('t'), &mut w);
    assert_eq!(w.said, ["text of /x/broken.ipynb"]);
}

#[test]
fn shift_enter_runs_and_goes_on_and_makes_a_cell_after_the_last() {
    let (mut p, mut w) = open();
    p.key(Key::Char('G'), &mut w);
    p.enter(true, false, false, &mut w);
    assert_eq!(notebook(&w).len(), 6);
    assert_eq!(p.focus, 5);
}

#[test]
fn closing_shuts_the_kernel_and_the_editors() {
    let (mut p, mut w) = open();
    p.key(Key::Enter, &mut w);
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    p.close(&mut w);
    assert_eq!(w.written.last().unwrap(), r#"{"op":"shutdown"}"#);
    assert_eq!(w.stopped, [9]);
    assert_eq!(w.closed, [100]);
}

#[test]
fn a_tall_notebook_scrolls_to_the_focused_cell() {
    let (mut p, mut w) = open();
    p.render(80, 12, &mut w);
    p.key(Key::Char('G'), &mut w);
    let s = p.render(80, 12, &mut w).text();
    assert!(s.contains("1 / 0"), "{s}");
    assert!(!s.contains("Assignment 2"), "{s}");
}

/// A kernel that ended badly names the keys of the extensions' log as the
/// scheme in use has them, when the page is drawn: drawn again after a
/// switch, it says the other's.
#[test]
fn a_kernel_that_ended_badly_names_the_logs_keys_in_use() {
    use fenix_test_kit::api::KeyScheme;
    let fake = fenix_test_kit::Fake::new();
    fake.key("extensions.output", "SPC h l");
    let (mut p, mut w) = open();
    p.key(Key::Char('j'), &mut w);
    p.key(Key::Char('r'), &mut w);
    p.said(r#"{"ev": "ready", "label": "x"}"#, &mut w);
    p.exited(Some(2), &mut w);
    assert!(screen(&mut p, &mut w).contains("stopped: it ended (2); SPC h l has why"));
    fake.key_scheme(KeyScheme::Standard);
    assert!(screen(&mut p, &mut w).contains("stopped: it ended (2); C-k h l has why"));
    fake.key_in(KeyScheme::Standard, "extensions.output", "F9");
    assert!(screen(&mut p, &mut w).contains("stopped: it ended (2); F9 has why"), "yours");
    fake.no_key("extensions.output");
    assert!(screen(&mut p, &mut w).contains("stopped: it ended (2); SPC h l has why"), "no key: Fenix's keymap's");
}
