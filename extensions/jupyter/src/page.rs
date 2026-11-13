//! A notebook's page: its cells one under another -- code highlighted on
//! a panel with its `[n]` beside it, markdown as it reads, outputs below
//! their cell with their pictures -- a cell focused, edited in a pane
//! beside, run on the notebook's kernel.
//!
//! What it changes it changes in the notebook's buffer, as an edit of
//! the file's text (`write`): saving, and Fenix's undo, are the
//! buffer's. What it asks of Fenix goes through [`World`], so it runs in
//! tests without Fenix.

use std::collections::{HashMap, HashSet, VecDeque};

use fenix_ui_kit::text::{char_width, fit, width};
use fenix_ui_kit::{Canvas, Key, Mouse, MouseAction, Rect, Role, Style};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::edit::{diff, TextEdit};
use crate::highlight;
use crate::kernel::{self, Said, State};
use crate::markdown;
use crate::nb::{Kind, Notebook};
use crate::output::{self, Shown, Tone};

/// What the page asks of Fenix.
pub trait World {
    fn buffer_text(&mut self, path: &str) -> Option<String>;
    fn edit(&mut self, path: &str, edit: TextEdit) -> Result<(), String>;
    fn save(&mut self, path: &str) -> Result<(), String>;
    fn undo(&mut self, path: &str, redo: bool) -> Result<(), String>;
    fn copy(&mut self, text: &str);
    fn say(&mut self, text: &str);
    fn log(&mut self, text: &str);
    /// The notebook's text in a buffer, as Fenix opens a file's.
    fn open_text(&mut self, path: &str);
    /// A picture's id for the canvas: loaded once, kept after.
    fn picture(&mut self, bytes: &[u8]) -> Option<u32>;
    /// A cell's source in a pane beside, to edit: the scratch buffer's id.
    fn edit_cell(&mut self, title: &str, text: &str, language: &str) -> u64;
    fn close_scratch(&mut self, scratch: u64);
    /// The kernel for the notebook at `full` started: its process, and
    /// which Python it's run with.
    fn start_kernel(&mut self, full: &str, kernel: Option<&str>) -> Result<(u64, String), String>;
    fn write(&mut self, process: u64, line: &str) -> Result<(), String>;
    /// Its stdin closed: it shuts its kernel down and ends.
    fn stop(&mut self, process: u64);
    /// A line asked of you, for `input()`: the prompt's id.
    fn ask(&mut self, prompt: &str, password: bool) -> u64;
    /// ipykernel installed into the notebook's Python, as a task.
    fn install_kernel(&mut self, full: &str);
}

/// What's kept of a page with the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// Where it's read from: under `/project` when it's there.
    pub path: String,
    /// Its full path.
    pub full: String,
    #[serde(default)]
    pub focus: usize,
    #[serde(default)]
    pub scroll: usize,
}

/// Columns before a cell's text: the focus bar, `[12]:`, a space.
const GUTTER: usize = 10;
/// A text output's lines shown at most; the rest are in the text (`t`).
const MAX_LINES: usize = 400;
/// One cell's size in pixels, for pictures' proportions.
const CELL_PX: (f32, f32) = (9.0, 19.0);

/// What a line of the page holds.
#[derive(Debug, Clone, PartialEq)]
enum Body {
    Text(Vec<(String, Role)>),
    /// Code: on a panel.
    Code(Vec<(String, Role)>),
    Rule,
    /// A picture's first line: which, and how many lines and columns.
    Picture { picture: usize, rows: usize, cols: usize },
    /// A picture's other lines.
    PictureRest,
    Blank,
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    cell: Option<usize>,
    gutter: Option<(String, Role)>,
    body: Body,
}

/// The notebook laid out for a width.
#[derive(Debug, Clone, Default)]
struct Laid {
    width: usize,
    version: u64,
    rows: Vec<Row>,
    /// Each cell's first row and the row after its last.
    spans: Vec<(usize, usize)>,
    pictures: Vec<Vec<u8>>,
}

/// The kernel, as this page has it.
#[derive(Debug, Default)]
struct Kernel {
    process: Option<u64>,
    state: State,
    /// What it runs on: `Python 3 (ipykernel)`, and the Python.
    label: String,
    python: String,
    /// Why it isn't a Jupyter kernel, when it isn't.
    note: String,
    /// The cell running, and those waiting, by their ids here.
    running: Option<u64>,
    queue: VecDeque<u64>,
    /// `clear_output(wait=True)`: the outputs go with the next one.
    clear_next: bool,
    /// The prompt asked for `input()`, waiting.
    asking: Option<u64>,
}

pub struct NotebookPage {
    /// The notebook's full path, and where it's read from.
    pub full: String,
    pub path: String,
    nb: Result<Notebook, String>,
    /// The buffer's text as it was last read or written here.
    text: String,
    /// Each cell's id here, which moves with it.
    ids: Vec<u64>,
    next_id: u64,
    pub focus: usize,
    scroll: usize,
    /// Whether the focused cell is kept in view (until the wheel moves it).
    follow: bool,
    version: u64,
    laid: Laid,
    kernel: Kernel,
    /// Its cells' scratch buffers, by the buffer: the cell's id.
    pub scratches: HashMap<u64, u64>,
    /// Cells whose outputs are hidden (`o`).
    hidden: HashSet<u64>,
    /// A key waiting for its second: `d` of `dd`, `i` of `ii`, `0` of `00`.
    pending: Option<char>,
    /// A cell copied or cut, to paste.
    clipboard: Option<Value>,
    /// Changed here since it was saved.
    edited: bool,
    /// A line for the foot of the page, in place of the keys.
    message: Option<(String, Role)>,
    body_height: usize,
}

fn name_of(full: &str) -> &str {
    full.rsplit(['/', '\\']).next().unwrap_or(full)
}

/// A scratch buffer's language for a cell: highlighted as its file would be.
fn scratch_language(kind: Kind, language: &str) -> &'static str {
    match (kind, language) {
        (Kind::Markdown, _) => "md",
        (Kind::Raw, _) => "txt",
        (_, l) if l.starts_with("python") => "py",
        (_, "r") => "r",
        (_, "julia") => "jl",
        (_, "javascript") | (_, "typescript") => "js",
        _ => "txt",
    }
}

/// `runs` cut into lines `w` cells wide.
fn hard_wrap(runs: &[(String, Role)], w: usize) -> Vec<Vec<(String, Role)>> {
    let w = w.max(4);
    let mut lines = vec![Vec::new()];
    let mut used = 0;
    for (text, role) in runs {
        let mut piece = String::new();
        for c in text.chars() {
            let cw = char_width(c);
            if used + cw > w {
                if !piece.is_empty() {
                    lines.last_mut().unwrap().push((std::mem::take(&mut piece), role.clone()));
                }
                lines.push(Vec::new());
                used = 0;
            }
            piece.push(c);
            used += cw;
        }
        if !piece.is_empty() {
            lines.last_mut().unwrap().push((piece, role.clone()));
        }
    }
    lines
}

impl NotebookPage {
    pub fn new(full: &str, path: &str, text: String) -> NotebookPage {
        let nb = Notebook::parse(&text);
        let count = nb.as_ref().map(Notebook::len).unwrap_or(0);
        NotebookPage {
            full: full.into(),
            path: path.into(),
            nb,
            text,
            ids: (1..=count as u64).collect(),
            next_id: count as u64 + 1,
            focus: 0,
            scroll: 0,
            follow: true,
            version: 1,
            laid: Laid::default(),
            kernel: Kernel::default(),
            scratches: HashMap::new(),
            hidden: HashSet::new(),
            pending: None,
            clipboard: None,
            edited: false,
            message: None,
            body_height: 20,
        }
    }

    pub fn name(&self) -> String {
        name_of(&self.full).to_string()
    }

    pub fn saved(&self) -> Saved {
        Saved { path: self.path.clone(), full: self.full.clone(), focus: self.focus, scroll: self.scroll }
    }

    pub fn restore(&mut self, s: &Saved) {
        self.focus = s.focus.min(self.len().saturating_sub(1));
        self.scroll = s.scroll;
        self.follow = false;
    }

    pub fn process(&self) -> Option<u64> {
        self.kernel.process
    }

    pub fn asking(&self) -> Option<u64> {
        self.kernel.asking
    }

    fn len(&self) -> usize {
        self.nb.as_ref().map(Notebook::len).unwrap_or(0)
    }

    fn index(&self, id: u64) -> Option<usize> {
        self.ids.iter().position(|&i| i == id)
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn touch(&mut self) {
        self.version += 1;
    }

    fn say(&mut self, text: impl Into<String>, role: Role) {
        self.message = Some((text.into(), role));
    }

    // ------------------------------------------------------------ the buffer

    /// The buffer's text changed (typed in its text, undone, replaced
    /// across files): the notebook read again, if it isn't what's here.
    /// Whether it was.
    pub fn changed(&mut self, text: Option<String>, world: &mut dyn World) -> bool {
        let Some(text) = text else { return false };
        if text == self.text {
            return false;
        }
        let before = self.len();
        self.nb = Notebook::parse(&text);
        self.text = text;
        let after = self.len();
        if after != before {
            // Cells came or went where this page didn't see: their ids
            // start again, and their editors close.
            self.ids = (0..after).map(|_| 0).collect();
            for i in 0..after {
                self.ids[i] = self.fresh_id();
            }
            for (scratch, _) in self.scratches.drain() {
                world.close_scratch(scratch);
            }
            self.kernel.queue.clear();
        }
        self.focus = self.focus.min(after.saturating_sub(1));
        self.touch();
        true
    }

    /// The buffer read, if it changed since: before a change here, so
    /// it's made to what's there.
    fn sync(&mut self, world: &mut dyn World) {
        let text = world.buffer_text(&self.path);
        self.changed(text, world);
    }

    /// The notebook written to its buffer: one edit, of what changed.
    fn write(&mut self, world: &mut dyn World) {
        let Ok(nb) = &self.nb else { return };
        let new = nb.to_text();
        let old = world.buffer_text(&self.path).unwrap_or_else(|| self.text.clone());
        if let Some(e) = diff(&old, &new) {
            if let Err(err) = world.edit(&self.path, e) {
                world.say(&format!("{}: {err}", self.name()));
                return;
            }
            self.edited = true;
        }
        self.text = new;
    }

    /// A change to the notebook, made to what the buffer holds, then
    /// written: `f` says whether it changed anything.
    fn change(&mut self, world: &mut dyn World, f: impl FnOnce(&mut NotebookPage) -> bool) {
        self.sync(world);
        if self.nb.is_err() {
            return;
        }
        if f(self) {
            self.touch();
            self.write(world);
        }
    }

    // ------------------------------------------------------------ editing

    /// The focused cell's source, in a pane beside.
    fn edit_focused(&mut self, world: &mut dyn World) {
        let Ok(nb) = &self.nb else { return };
        let i = self.focus;
        if i >= nb.len() {
            return;
        }
        let id = self.ids[i];
        let (text, kind, language) = (nb.source(i), nb.kind(i), nb.language());
        let open: Vec<u64> = self.scratches.iter().filter(|(_, &c)| c == id).map(|(&s, _)| s).collect();
        for s in open {
            self.scratches.remove(&s);
            world.close_scratch(s);
        }
        let title = format!("{} · cell {}", self.name(), i + 1);
        let scratch = world.edit_cell(&title, &text, scratch_language(kind, &language));
        self.scratches.insert(scratch, id);
    }

    /// One of its cells' editors changed: the cell's source is its text.
    pub fn scratch_changed(&mut self, scratch: u64, text: &str, world: &mut dyn World) {
        let Some(&id) = self.scratches.get(&scratch) else { return };
        self.change(world, |p| {
            let Some(i) = p.index(id) else { return false };
            let nb = p.nb.as_mut().expect("change checks it");
            if nb.source(i) == text {
                return false;
            }
            nb.set_source(i, text);
            true
        });
    }

    fn insert(&mut self, at: usize, kind: Kind, world: &mut dyn World) {
        self.change(world, |p| {
            let at = at.min(p.len());
            p.nb.as_mut().unwrap().insert(at, kind);
            let id = p.fresh_id();
            p.ids.insert(at, id);
            p.focus = at;
            p.follow = true;
            true
        });
        self.edit_focused(world);
    }

    fn remove(&mut self, world: &mut dyn World, keep: bool) {
        self.change(world, |p| {
            let i = p.focus;
            let Some(cell) = p.nb.as_mut().unwrap().remove(i) else { return false };
            if keep {
                p.clipboard = Some(cell);
            }
            let id = p.ids.remove(i);
            p.kernel.queue.retain(|&q| q != id);
            if p.len() == 0 {
                p.nb.as_mut().unwrap().insert(0, Kind::Code);
                let id = p.fresh_id();
                p.ids.push(id);
            }
            p.focus = i.min(p.len() - 1);
            p.follow = true;
            true
        });
    }

    fn paste(&mut self, below: bool, world: &mut dyn World) {
        let Some(cell) = self.clipboard.clone() else {
            self.say("nothing copied: c copies a cell, x cuts it", Role::Muted);
            return;
        };
        self.change(world, |p| {
            let at = if below { (p.focus + 1).min(p.len()) } else { p.focus };
            p.nb.as_mut().unwrap().put(at, cell);
            let id = p.fresh_id();
            p.ids.insert(at, id);
            p.focus = at;
            p.follow = true;
            true
        });
    }

    fn move_cell(&mut self, down: bool, world: &mut dyn World) {
        self.change(world, |p| {
            let i = p.focus;
            let j = if down { i + 1 } else { i.wrapping_sub(1) };
            if j >= p.len() {
                return false;
            }
            p.nb.as_mut().unwrap().swap(i, j);
            p.ids.swap(i, j);
            p.focus = j;
            p.follow = true;
            true
        });
    }

    fn set_kind(&mut self, kind: Kind, world: &mut dyn World) {
        self.change(world, |p| {
            let i = p.focus;
            let nb = p.nb.as_mut().unwrap();
            if i >= nb.len() || nb.kind(i) == kind {
                return false;
            }
            nb.set_kind(i, kind);
            true
        });
    }

    fn clear(&mut self, all: bool, world: &mut dyn World) {
        self.change(world, |p| {
            let nb = p.nb.as_mut().unwrap();
            let cells: Vec<usize> = if all { (0..nb.len()).collect() } else { vec![p.focus] };
            let mut any = false;
            for i in cells {
                if !nb.outputs(i).is_empty() || nb.count(i).is_some() {
                    nb.clear_outputs(i);
                    any = true;
                }
            }
            any
        });
    }

    pub fn clear_all(&mut self, world: &mut dyn World) {
        self.clear(true, world)
    }

    pub fn save(&mut self, world: &mut dyn World) {
        self.write(world);
        match world.save(&self.path) {
            Ok(()) => {
                self.edited = false;
                self.say(format!("saved {}", self.name()), Role::Good);
            }
            Err(e) => world.say(&format!("{}: {e}", self.name())),
        }
    }

    fn undo(&mut self, redo: bool, world: &mut dyn World) {
        if let Err(e) = world.undo(&self.path, redo) {
            world.say(&e);
        }
        self.sync(world);
    }

    // ------------------------------------------------------------ the kernel

    /// Cell `i` queued to run (cleared of what it showed), and the queue
    /// moved on.
    fn queue(&mut self, i: usize) {
        let Ok(nb) = &self.nb else { return };
        if i >= nb.len() || nb.kind(i) != Kind::Code {
            return;
        }
        let id = self.ids[i];
        if self.kernel.running == Some(id) || self.kernel.queue.contains(&id) {
            return;
        }
        self.nb.as_mut().unwrap().clear_outputs(i);
        self.kernel.queue.push_back(id);
        self.touch();
    }

    pub fn run(&mut self, cells: Vec<usize>, world: &mut dyn World) {
        self.sync(world);
        for i in cells {
            self.queue(i);
        }
        self.write(world);
        self.pump(world);
    }

    pub fn run_all(&mut self, world: &mut dyn World) {
        let all = (0..self.len()).collect();
        self.run(all, world);
    }

    /// The next cell waiting given to the kernel, starting it if it's
    /// not running.
    fn pump(&mut self, world: &mut dyn World) {
        if self.kernel.queue.is_empty() {
            return;
        }
        match self.kernel.state {
            _ if self.kernel.state.stopped() && self.kernel.process.is_none() => {
                let kernel = self.nb.as_ref().ok().and_then(Notebook::kernel_name);
                match world.start_kernel(&self.full, kernel.as_deref()) {
                    Ok((process, python)) => {
                        self.kernel.process = Some(process);
                        self.kernel.python = python;
                        self.kernel.state = State::Starting;
                    }
                    Err(e) => {
                        self.kernel.state = State::not_started(e.clone());
                        self.kernel.queue.clear();
                        self.say(e, Role::Bad);
                    }
                }
                self.touch();
                return;
            }
            State::Idle => {}
            _ => return,
        }
        let Some(process) = self.kernel.process else { return };
        while let Some(id) = self.kernel.queue.pop_front() {
            let Some(i) = self.index(id) else { continue };
            let code = self.nb.as_ref().map(|nb| nb.source(i)).unwrap_or_default();
            if let Err(e) = world.write(process, &kernel::run(id, &code)) {
                self.say(format!("the kernel: {e}"), Role::Bad);
                self.kernel.queue.clear();
                return;
            }
            self.kernel.running = Some(id);
            self.kernel.clear_next = false;
            self.kernel.state = State::Busy;
            self.touch();
            return;
        }
    }

    /// A line from the kernel's stdout.
    pub fn said(&mut self, line: &str, world: &mut dyn World) {
        let Some(said) = Said::parse(line) else {
            world.log(line);
            return;
        };
        match said {
            Said::Ready { label, mode, note } => {
                self.kernel.state = State::Idle;
                self.kernel.label = label;
                self.kernel.note = if mode == "builtin" { note } else { String::new() };
                if !self.kernel.note.is_empty() {
                    world.log(&self.kernel.note);
                }
                self.touch();
                self.pump(world);
            }
            Said::Output { id, output } => {
                let Some(i) = self.index(id) else { return };
                let nb = match self.nb.as_mut() {
                    Ok(nb) => nb,
                    Err(_) => return,
                };
                if std::mem::take(&mut self.kernel.clear_next) {
                    let count = nb.count(i);
                    nb.clear_outputs(i);
                    nb.set_count(i, count);
                }
                nb.push_output(i, output);
                self.touch();
            }
            Said::Clear { id, wait } => {
                if wait {
                    self.kernel.clear_next = true;
                } else if let (Some(i), Ok(nb)) = (self.index(id), self.nb.as_mut()) {
                    nb.clear_outputs(i);
                    self.touch();
                }
            }
            Said::Input { prompt, password, .. } => {
                let ask = world.ask(if prompt.trim().is_empty() { "input()" } else { prompt.trim_end() }, password);
                self.kernel.asking = Some(ask);
            }
            Said::Done { id, count, status } => {
                if let (Some(i), Ok(nb)) = (self.index(id), self.nb.as_mut()) {
                    nb.set_count(i, count);
                }
                if self.kernel.running == Some(id) {
                    self.kernel.running = None;
                }
                if status == "error" || status == "aborted" {
                    // As Jupyter does: an error stops the cells after it.
                    self.kernel.queue.clear();
                }
                if self.kernel.state == State::Busy {
                    self.kernel.state = State::Idle;
                }
                self.touch();
                self.write(world);
                self.pump(world);
            }
            Said::Died { why } => self.ended(State::Dead(why), world),
            Said::Log { text } => world.log(&text),
        }
    }

    /// A line from the kernel's stderr: what the running cell's programs
    /// printed (its stdout is kept for Fenix), else for the log.
    pub fn said_stderr(&mut self, line: &str, world: &mut dyn World) {
        let running = self.kernel.running.and_then(|id| self.index(id));
        match (running, self.nb.as_mut()) {
            (Some(i), Ok(nb)) => {
                nb.push_output(i, serde_json::json!({"output_type": "stream", "name": "stderr", "text": format!("{line}\n")}));
                self.touch();
            }
            _ => world.log(line),
        }
    }

    /// The kernel's program ended.
    pub fn exited(&mut self, code: Option<i32>, world: &mut dyn World) {
        if self.kernel.process.is_none() {
            return;
        }
        let state = match (&self.kernel.state, code) {
            (State::Dead(why), _) => State::Dead(why.clone()),
            (_, Some(0)) => State::Dead("it ended".to_string()),
            (_, Some(c)) => State::Ended(c),
            (_, None) => State::Dead("it was stopped".to_string()),
        };
        self.ended(state, world);
    }

    /// The kernel's gone: `state` says why.
    fn ended(&mut self, state: State, world: &mut dyn World) {
        self.kernel.process = None;
        self.kernel.running = None;
        self.kernel.queue.clear();
        self.kernel.asking = None;
        self.kernel.state = state;
        self.touch();
        self.write(world);
    }

    /// What was typed for `input()`, or nothing.
    pub fn answered(&mut self, text: Option<String>, world: &mut dyn World) {
        self.kernel.asking = None;
        if let Some(p) = self.kernel.process {
            let _ = world.write(p, &kernel::input(&text.unwrap_or_default()));
        }
    }

    pub fn interrupt(&mut self, world: &mut dyn World) {
        let Some(p) = self.kernel.process else {
            self.say("no kernel running", Role::Muted);
            return;
        };
        self.kernel.queue.clear();
        let _ = world.write(p, &kernel::op("interrupt"));
        if self.kernel.asking.take().is_some() {
            let _ = world.write(p, &kernel::input(""));
        }
        self.say("interrupted", Role::Warn);
        self.touch();
    }

    pub fn restart(&mut self, world: &mut dyn World) {
        self.kernel.queue.clear();
        self.kernel.running = None;
        self.kernel.asking = None;
        match self.kernel.process {
            Some(p) => {
                let _ = world.write(p, &kernel::op("restart"));
                self.kernel.state = State::Starting;
                self.say("the kernel starts again: what it held is gone", Role::Warn);
            }
            None => {
                self.kernel.state = State::Off;
                self.say("no kernel yet: r runs a cell and starts it", Role::Muted);
            }
        }
        self.touch();
    }

    /// The page closed: its kernel shut down, its editors closed.
    pub fn close(&mut self, world: &mut dyn World) {
        if let Some(p) = self.kernel.process.take() {
            let _ = world.write(p, &kernel::op("shutdown"));
            world.stop(p);
        }
        for (s, _) in self.scratches.drain() {
            world.close_scratch(s);
        }
    }

    // ------------------------------------------------------------ keys

    pub fn key(&mut self, key: Key, world: &mut dyn World) {
        self.message = None;
        let pending = self.pending.take();
        if self.nb.is_err() {
            if key == Key::Char('t') {
                world.open_text(&self.full);
            }
            return;
        }
        let last = self.len().saturating_sub(1);
        let focus = |p: &mut NotebookPage, to: usize| {
            p.focus = to.min(last);
            p.follow = true;
        };
        match key {
            Key::Down | Key::Char('j') => focus(self, self.focus + 1),
            Key::Up | Key::Char('k') => focus(self, self.focus.saturating_sub(1)),
            Key::Home | Key::Char('g') => focus(self, 0),
            Key::End | Key::Char('G') => focus(self, last),
            Key::PageDown | Key::Char('J') => self.scroll_by(self.body_height as isize - 2),
            Key::PageUp | Key::Char('K') => self.scroll_by(-(self.body_height as isize - 2)),
            Key::Enter | Key::Char('e') => self.edit_focused(world),
            Key::Char('r') => self.run(vec![self.focus], world),
            Key::Char('R') => self.run_all(world),
            Key::Char('a') => self.insert(self.focus, Kind::Code, world),
            Key::Char('b') => self.insert(self.focus + 1, Kind::Code, world),
            Key::Char('d') if pending == Some('d') => self.remove(world, true),
            Key::Char('i') if pending == Some('i') => self.interrupt(world),
            Key::Char('0') if pending == Some('0') => self.restart(world),
            Key::Char(c @ ('d' | 'i' | '0')) => self.pending = Some(c),
            Key::Char('x') => self.remove(world, true),
            Key::Char('c') => {
                self.clipboard = self.nb.as_ref().ok().and_then(|nb| nb.cell(self.focus));
                self.say(format!("cell {} copied: v pastes it below, V above", self.focus + 1), Role::Muted);
            }
            Key::Char('v') => self.paste(true, world),
            Key::Char('V') => self.paste(false, world),
            Key::Char('m') => self.set_kind(Kind::Markdown, world),
            Key::Char('y') => self.set_kind(Kind::Code, world),
            Key::Char('[') => self.move_cell(false, world),
            Key::Char(']') => self.move_cell(true, world),
            Key::Char('o') => {
                let id = self.ids[self.focus];
                if !self.hidden.remove(&id) {
                    self.hidden.insert(id);
                }
                self.touch();
            }
            Key::Char('C') => self.clear(false, world),
            Key::Char('u') => self.undo(false, world),
            Key::Char('U') => self.undo(true, world),
            Key::Char('Y') => {
                let src = self.nb.as_ref().map(|nb| nb.source(self.focus)).unwrap_or_default();
                world.copy(&src);
                self.say("its source copied", Role::Muted);
            }
            Key::Char('w') => self.save(world),
            Key::Char('t') => world.open_text(&self.full),
            Key::Char('P') => world.install_kernel(&self.full),
            _ => {}
        }
    }

    /// Shift+Enter runs the cell and goes to the next (a new one after the
    /// last); Ctrl+Enter runs it; Alt+Enter runs it and makes one below.
    pub fn enter(&mut self, shift: bool, ctrl: bool, alt: bool, world: &mut dyn World) {
        self.message = None;
        if self.nb.is_err() {
            return;
        }
        let i = self.focus;
        self.run(vec![i], world);
        if alt {
            self.insert(i + 1, Kind::Code, world);
        } else if shift && !ctrl {
            if i + 1 >= self.len() {
                self.insert(i + 1, Kind::Code, world);
            } else {
                self.focus = i + 1;
                self.follow = true;
            }
        }
    }

    /// Ctrl+Z, Ctrl+Shift+Z: the buffer's undo and redo.
    pub fn ctrl(&mut self, c: char, shift: bool, world: &mut dyn World) {
        match c.to_ascii_lowercase() {
            'z' => self.undo(shift, world),
            'y' => self.undo(true, world),
            's' => self.save(world),
            _ => {}
        }
    }

    fn scroll_by(&mut self, n: isize) {
        let most = self.laid.rows.len().saturating_sub(self.body_height.max(1));
        self.scroll = (self.scroll as isize + n).clamp(0, most as isize) as usize;
        self.follow = false;
        // The focus goes to the cell at the top, as the view moved.
        if let Some(c) = self.laid.rows.get(self.scroll..).and_then(|r| r.iter().find_map(|r| r.cell)) {
            self.focus = c;
        }
    }

    pub fn mouse(&mut self, m: &Mouse, world: &mut dyn World) {
        match m.action {
            MouseAction::Wheel(n) => self.scroll_by(-(n as isize) * 3),
            MouseAction::Press { clicks, .. } => {
                if m.y < 2 {
                    return;
                }
                let row = self.scroll + m.y - 2;
                if let Some(cell) = self.laid.rows.get(row).and_then(|r| r.cell) {
                    self.focus = cell;
                    if clicks >= 2 {
                        self.edit_focused(world);
                    }
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ layout

    /// The gutter beside a code cell: `[3]:`, `[*]:` while it runs or
    /// waits, `[ ]:` before it's run.
    fn count_label(&self, i: usize, nb: &Notebook) -> String {
        let id = self.ids[i];
        if self.kernel.running == Some(id) || self.kernel.queue.contains(&id) {
            return "[*]:".into();
        }
        match nb.count(i) {
            Some(n) => format!("[{n}]:"),
            None => "[ ]:".into(),
        }
    }

    fn lay_out(&self, nb: &Notebook, w: usize) -> Laid {
        let cw = w.saturating_sub(GUTTER + 1).max(10);
        let mut laid = Laid { width: w, version: self.version, ..Laid::default() };
        let language = nb.language();
        for i in 0..nb.len() {
            let start = laid.rows.len();
            let row = |gutter: Option<(String, Role)>, body| Row { cell: Some(i), gutter, body };
            let src = nb.source(i);
            match nb.kind(i) {
                Kind::Code => {
                    let mut lines = highlight::lines(&src, &language);
                    if lines.is_empty() {
                        lines.push(Vec::new());
                    }
                    let mut first = Some((self.count_label(i, nb), Role::Muted));
                    for l in lines {
                        for piece in hard_wrap(&l, cw) {
                            laid.rows.push(row(first.take(), Body::Code(piece)));
                        }
                    }
                    if self.hidden.contains(&self.ids[i]) {
                        let n = nb.outputs(i).len();
                        if n > 0 {
                            laid.rows.push(row(None, Body::Text(vec![(format!("({n} output{} hidden: o shows them)", if n == 1 { "" } else { "s" }), Role::Muted)])));
                        }
                    } else {
                        for out in nb.outputs(i) {
                            let mut gutter = (out["output_type"] == "execute_result").then(|| (out["execution_count"].as_u64().map_or("Out:".to_string(), |n| format!("Out[{n}]:")), Role::Muted));
                            for shown in output::shown(out) {
                                self.lay_output(shown, cw, &mut gutter, i, &mut laid);
                            }
                        }
                    }
                }
                Kind::Markdown => {
                    let lines = markdown::render(&src, cw);
                    if lines.is_empty() {
                        laid.rows.push(row(None, Body::Text(vec![("empty markdown cell: Enter writes it".into(), Role::Muted)])));
                    }
                    for l in lines {
                        let body = if l.rule {
                            Body::Rule
                        } else if l.code {
                            Body::Code(l.spans)
                        } else {
                            Body::Text(l.spans)
                        };
                        laid.rows.push(row(None, body));
                    }
                }
                Kind::Raw => {
                    let raw = [(if src.is_empty() { "empty raw cell".to_string() } else { src.clone() }, Role::Muted)];
                    for l in raw[0].0.split('\n') {
                        for piece in hard_wrap(&[(l.to_string(), Role::Muted)], cw) {
                            laid.rows.push(row(None, Body::Text(piece)));
                        }
                    }
                }
            }
            laid.spans.push((start, laid.rows.len()));
            laid.rows.push(Row { cell: None, gutter: None, body: Body::Blank });
        }
        laid
    }

    fn lay_output(&self, shown: Shown, cw: usize, gutter: &mut Option<(String, Role)>, i: usize, laid: &mut Laid) {
        match shown {
            Shown::Text { text, tone } => {
                let role = match tone {
                    Tone::Text => Role::Text,
                    Tone::Warn => Role::Warn,
                    Tone::Bad => Role::Bad,
                    Tone::Muted => Role::Muted,
                };
                let text = text.strip_suffix('\n').unwrap_or(&text);
                let lines: Vec<&str> = text.split('\n').collect();
                for l in lines.iter().take(MAX_LINES) {
                    for piece in hard_wrap(&[(l.to_string(), role.clone())], cw) {
                        laid.rows.push(Row { cell: Some(i), gutter: gutter.take(), body: Body::Text(piece) });
                    }
                }
                if lines.len() > MAX_LINES {
                    let more = format!("… {} more lines: t has them all", lines.len() - MAX_LINES);
                    laid.rows.push(Row { cell: Some(i), gutter: None, body: Body::Text(vec![(more, Role::Muted)]) });
                }
            }
            Shown::Picture { bytes, width: pw, height: ph } => {
                let natural = (pw as f32 / CELL_PX.0).ceil() as usize;
                let cols = natural.clamp(4, cw);
                let rows = ((cols as f32 * CELL_PX.0) * ph as f32 / pw.max(1) as f32 / CELL_PX.1).ceil() as usize;
                let rows = rows.clamp(2, 40);
                laid.pictures.push(bytes);
                laid.rows.push(Row { cell: Some(i), gutter: gutter.take(), body: Body::Picture { picture: laid.pictures.len() - 1, rows, cols } });
                for _ in 1..rows {
                    laid.rows.push(Row { cell: Some(i), gutter: None, body: Body::PictureRest });
                }
            }
        }
    }

    /// The focused cell brought into view, if the view follows it.
    fn keep_in_view(&mut self) {
        let h = self.body_height.max(1);
        let most = self.laid.rows.len().saturating_sub(h);
        if self.follow {
            if let Some(&(start, end)) = self.laid.spans.get(self.focus) {
                if start < self.scroll {
                    self.scroll = start;
                } else if end > self.scroll + h {
                    self.scroll = if end - start <= h { end - h } else { start };
                }
            }
        }
        self.scroll = self.scroll.min(most);
    }

    pub fn render(&mut self, w: usize, h: usize, world: &mut dyn World) -> Canvas {
        let mut c = Canvas::new();
        c.set_more_keys(true);
        c.set_mouse(true);
        let w = w.max(30);
        self.body_height = h.saturating_sub(3).max(1);
        let name = self.name();
        let x = c.put(1, 0, &name, Role::Title);
        let nb = match &self.nb {
            Ok(nb) => nb,
            Err(e) => {
                c.rule(1, 0..w);
                c.put(1, 2, &fit(e, w.saturating_sub(2)), Role::Bad);
                c.put(1, 4, "t opens its text, to mend it", Role::Muted);
                return c;
            }
        };
        let kernel = if self.kernel.label.is_empty() { nb.kernel_display().unwrap_or_else(|| "Python".into()) } else { self.kernel.label.clone() };
        let state_role = match self.kernel.state {
            State::Busy | State::Starting => Role::Warn,
            State::Dead(_) | State::Ended(_) | State::NoPython => Role::Bad,
            State::Idle => Role::Good,
            State::Off => Role::Muted,
        };
        let mut info = vec![(format!("  {kernel}"), Role::Muted), (" · ".into(), Role::Muted), (self.kernel.state.label(), state_role)];
        if !self.kernel.python.is_empty() && self.kernel.state != State::Off {
            info.push((format!(" · {}", self.kernel.python), Role::Muted));
        }
        info.push((format!(" · {} cells", nb.len()), Role::Muted));
        if self.edited {
            info.push((" · not saved".into(), Role::Warn));
        }
        let mut at = x;
        for (t, r) in info {
            let room = w.saturating_sub(at + 1);
            if room == 0 {
                break;
            }
            at = c.put(at, 0, &fit(&t, room), r);
        }
        c.rule(1, 0..w);
        if self.laid.width != w || self.laid.version != self.version {
            self.laid = self.lay_out(nb, w);
        }
        self.keep_in_view();
        let cw = w.saturating_sub(GUTTER + 1).max(10);
        let top = 2;
        let bottom = top + self.body_height;
        let mut pictures = Vec::new();
        for (n, row) in self.laid.rows.iter().enumerate().skip(self.scroll).take(self.body_height) {
            let y = top + n - self.scroll;
            let focused = row.cell == Some(self.focus);
            if focused {
                c.put(0, y, "▌", Role::Accent);
            }
            if let Some((g, role)) = &row.gutter {
                let role = if focused { Role::Accent } else { role.clone() };
                c.put(GUTTER - 1 - width(g).min(GUTTER - 1), y, g, role);
            }
            match &row.body {
                Body::Text(spans) | Body::Code(spans) => {
                    let mut x = GUTTER;
                    for (t, r) in spans {
                        x = c.put(x, y, t, Style::new(r.clone()));
                    }
                    if matches!(row.body, Body::Code(_)) {
                        c.panel(y, GUTTER.saturating_sub(1)..GUTTER + cw);
                    }
                }
                Body::Rule => c.rule(y, GUTTER..GUTTER + cw),
                Body::Picture { picture, rows, cols } => pictures.push((y, *picture, (*rows).min(bottom - y), *cols)),
                Body::PictureRest if n == self.scroll => {
                    c.put(GUTTER, y, "(a picture, above: k shows it)", Role::Muted);
                }
                Body::PictureRest | Body::Blank => {}
            }
        }
        for (y, picture, rows, cols) in pictures {
            if let Some(id) = world.picture(&self.laid.pictures[picture]) {
                c.picture(Rect::new(GUTTER, y, cols, rows), id, None, Vec::new());
            }
        }
        if self.laid.rows.len() > self.body_height {
            fenix_ui_kit::scrollbar(&mut c, w - 1, top, self.body_height, self.laid.rows.len(), self.scroll);
        }
        let foot = h.saturating_sub(1).max(top + 1);
        let (text, role) = match (&self.message, &self.kernel.note) {
            (Some((m, r)), _) => (m.clone(), r.clone()),
            (None, note) if !note.is_empty() && self.kernel.running.is_none() => (format!("{note} -- P installs it"), Role::Muted),
            _ => ("Enter edit · Shift+Enter run, next · r run · R run all · a b new · dd delete · m y markdown, code · ii 00 interrupt, restart · w save · t text".into(), Role::Muted),
        };
        c.put(1, foot, &fit(&text, w.saturating_sub(2)), role);
        c
    }
}

#[cfg(test)]
mod tests;
