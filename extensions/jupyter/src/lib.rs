//! Jupyter notebooks (`.ipynb`) in Fenix, as a WebAssembly extension: a
//! notebook opens in it however you open it (`contributes.opens`), its
//! cells and their outputs on a page, pictures and all; a cell edited in
//! a pane beside, its changes made to the file's buffer -- so saving, and
//! Fenix's undo, are the buffer's -- and run on a kernel: ipykernel when
//! the notebook's Python has it, else that Python itself
//! (`fenix_kernel.py`).

pub mod edit;
pub mod highlight;
pub mod kernel;
mod keys;
pub mod markdown;
pub mod nb;
pub mod output;
pub mod page;

use std::collections::HashMap;

use fenix_extension_api::{self as api, buffer, export, host, page as pages, process, scratch, Changed, Command, Event, Extension, Key, KeyPress, KeyScheme, MouseEvent, NamedKey, Page, SurfaceKey};

use kernel::{Found, Launch};
use page::{NotebookPage, Saved, World};

/// A notebook's page.
const NOTEBOOK: &str = "notebook";
/// Where the kernel's program is written, in its own folder.
const SCRIPT: &str = "/data/fenix_kernel.py";
const BACKEND: &str = "/data/fenix_mpl.py";

/// Fenix, as the pages ask things of it.
struct Fenix<'a> {
    pictures: &'a mut HashMap<u64, u32>,
    /// The task installing ipykernel, while it runs.
    installing: &'a mut Option<u64>,
}

/// A hash of a picture's bytes: which it is, to load it once.
fn hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    h ^ bytes.len() as u64
}

/// The folder a full path is in.
fn folder(full: &str) -> &str {
    match full.rfind(['/', '\\']) {
        Some(0) => "/",
        Some(i) => &full[..i],
        None => ".",
    }
}

/// The Pythons there are, found: the one you chose, the project's
/// `.venv`'s, uv, the one on `PATH`.
fn find() -> Found {
    // A path a setting holds runs as it is (`${setting:jupyter.python}`).
    let chosen = api::setting("jupyter.python").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    // The project's own, by a hint under it: not one found on `PATH`.
    let root = api::here().and_then(|h| h.project).map(|p| p.root.replace('\\', "/"));
    let venv = root.and_then(|root| ["python", "python3"].iter().find_map(|n| api::find_program(n, &["/project/.venv/bin", "/project/.venv/Scripts", "/project/venv/bin", "/project/venv/Scripts"]).filter(|p| p.replace('\\', "/").starts_with(&root))));
    let hints = ["$HOME/.local/bin", "$HOME/.cargo/bin", "%USERPROFILE%\\.local\\bin"];
    let uv = api::find_program("uv", &hints);
    let path = api::find_program("python3", &[]).or_else(|| api::find_program("python", &[]));
    Found { chosen, venv, uv, path, pyproject: std::path::Path::new("/project/pyproject.toml").exists() }
}

/// How to start the kernel's program.
fn launch() -> Result<Launch, String> {
    kernel::launch(&find())
}

impl World for Fenix<'_> {
    fn buffer_text(&mut self, path: &str) -> Option<String> {
        buffer::text(path)
    }

    fn edit(&mut self, path: &str, e: edit::TextEdit) -> Result<(), String> {
        buffer::edit(path, &[api::TextEdit::new(e.start, e.end, e.text)])
    }

    fn save(&mut self, path: &str) -> Result<(), String> {
        buffer::save(path)
    }

    fn undo(&mut self, path: &str, redo: bool) -> Result<(), String> {
        if redo {
            buffer::redo(path)
        } else {
            buffer::undo(path)
        }
    }

    fn copy(&mut self, text: &str) {
        api::copy(text)
    }

    fn say(&mut self, text: &str) {
        api::notify(text)
    }

    fn log(&mut self, text: &str) {
        api::info!("{text}");
    }

    fn open_text(&mut self, path: &str) {
        // A file it opens itself, asked for: Fenix opens its text, and
        // `SPC f o` there comes back.
        host::open_file(path, None);
    }

    fn picture(&mut self, bytes: &[u8]) -> Option<u32> {
        let key = hash(bytes);
        if let Some(&id) = self.pictures.get(&key) {
            return Some(id);
        }
        match api::image::load(bytes) {
            Ok(info) => {
                self.pictures.insert(key, info.id);
                Some(info.id)
            }
            Err(e) => {
                api::warn!("a picture didn't load: {e}");
                None
            }
        }
    }

    fn edit_cell(&mut self, title: &str, text: &str, language: &str) -> u64 {
        scratch::open_beside(title, text, Some(language), 0.45)
    }

    fn close_scratch(&mut self, id: u64) {
        scratch::close(id)
    }

    fn start_kernel(&mut self, full: &str, kernel: Option<&str>) -> Result<(u64, String), String> {
        write_script()?;
        let l = launch()?;
        let mut cmd = Command::new(&l.program).args(l.args.iter().cloned()).arg("${data}/fenix_kernel.py").args(["--cwd", folder(full)]);
        if let Some(k) = kernel {
            cmd = cmd.args(["--kernel", k]);
        }
        cmd = cmd.cwd(if l.in_project { "/project" } else { "/data" }).env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8");
        let id = cmd.spawn()?;
        api::info!("the kernel for {full}: {} {}", l.program, l.args.join(" "));
        Ok((id, l.label))
    }

    fn write(&mut self, process: u64, line: &str) -> Result<(), String> {
        process::write(process, line.as_bytes())
    }

    fn stop(&mut self, id: u64) {
        process::close_stdin(id)
    }

    fn ask(&mut self, prompt: &str, password: bool) -> u64 {
        if password {
            api::prompt_secret(prompt)
        } else {
            api::prompt(prompt, "")
        }
    }

    fn install_kernel(&mut self, _full: &str) {
        if let Some(task) = install_ipykernel() {
            *self.installing = Some(task);
        }
    }
}

/// ipykernel installed into the Python the notebooks run on, as a task:
/// the task, when it started. How it ended is [`installed_said`]'s.
fn install_ipykernel() -> Option<u64> {
    let found = find();
    let l = match kernel::launch(&found) {
        Ok(l) => l,
        Err(e) => {
            api::notify(&e);
            return None;
        }
    };
    let Some((program, args)) = kernel::install_ipykernel(&l, &found) else {
        api::notify("uv brings ipykernel for each run already");
        return None;
    };
    let mut cmd = Command::new(&program).args(args);
    if found.pyproject {
        cmd = cmd.cwd("/project");
    }
    match cmd.run_as_task("install ipykernel") {
        Ok(task) => {
            api::notify("installing ipykernel, in the task pane");
            Some(task)
        }
        Err(e) => {
            api::notify(&e);
            None
        }
    }
}

/// What's said in the message line when the task installing ipykernel
/// ends, whether it succeeded: what to do next. (Fenix tells of a task of
/// 10 seconds or more in a notification of its own; this says what it
/// means for the notebooks, however long it took.)
fn installed_said(ok: bool) -> &'static str {
    if ok {
        "ipykernel is installed: 00 on a notebook starts its kernel again with it"
    } else {
        "ipykernel wasn't installed: the task pane has why"
    }
}

/// The kernel's program, and its matplotlib backend, in its own folder,
/// where its Python runs them: written when they're not what this
/// version has.
fn write_script() -> Result<(), String> {
    for (path, text) in [(SCRIPT, kernel::SCRIPT), (BACKEND, kernel::BACKEND)] {
        if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
            std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
        }
    }
    Ok(())
}

/// Where a full path is under `/project`, if it is: for the session,
/// which reads it there.
fn in_project(full: &str) -> Option<String> {
    let root = api::here()?.project?.root;
    let (f, r) = (full.replace('\\', "/"), root.replace('\\', "/"));
    let r = r.trim_end_matches('/');
    let same = |a: &str, b: &str| if a.as_bytes().get(1) == Some(&b':') { a.eq_ignore_ascii_case(b) } else { a == b };
    (f.len() > r.len() && same(&f[..r.len()], r) && f.as_bytes()[r.len()] == b'/').then(|| format!("/project{}", &f[r.len()..]))
}

/// Whether full paths `a` and `b` are the same file: a drive's (`C:`)
/// without case, and `\\` as `/`.
fn same_file(a: &str, b: &str) -> bool {
    let (a, b) = (a.replace('\\', "/"), b.replace('\\', "/"));
    if a.as_bytes().get(1) == Some(&b':') {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// The file's text: its buffer's, if one's open, else what's on disk.
fn read(path: &str) -> Result<String, String> {
    match buffer::text(path) {
        Some(t) => Ok(t),
        None => api::read_file(path).map(|b| String::from_utf8_lossy(&b).into_owned()),
    }
}

#[derive(Default)]
struct Jupyter {
    pages: HashMap<u64, NotebookPage>,
    pictures: HashMap<u64, u32>,
    /// The page last shown or used: what the commands act on.
    last: Option<u64>,
    /// The task installing ipykernel, while it runs.
    installing: Option<u64>,
}

impl Jupyter {
    fn open(&mut self, full: &str) {
        if let Some((&id, _)) = self.pages.iter().find(|(_, p)| same_file(&p.full, full)) {
            pages::focus(id);
            self.last = Some(id);
            return;
        }
        let text = match read(full) {
            Ok(t) => t,
            Err(e) => return api::notify(&format!("{full}: {e}")),
        };
        let p = NotebookPage::new(full, full, text);
        let id = pages::open(NOTEBOOK, &p.name());
        self.pages.insert(id, p);
        self.last = Some(id);
    }

    /// `f` on a page, with Fenix; the page laid out again after.
    fn with(&mut self, page: u64, f: impl FnOnce(&mut NotebookPage, &mut dyn World)) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        f(p, &mut Fenix { pictures: &mut self.pictures, installing: &mut self.installing });
        self.last = Some(page);
        pages::refresh(page);
    }

    /// The page the commands act on: the last one used, else any.
    fn current(&self) -> Option<u64> {
        self.last.filter(|id| self.pages.contains_key(id)).or_else(|| self.pages.keys().next().copied())
    }

    fn by_process(&self, process: u64) -> Option<u64> {
        self.pages.iter().find(|(_, p)| p.process() == Some(process)).map(|(&id, _)| id)
    }
}

impl Extension for Jupyter {
    fn activate(&mut self) -> Result<(), String> {
        if let Err(e) = write_script() {
            api::warn!("{e}");
        }
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        if id == "jupyter.install-ipykernel" {
            if let Some(task) = install_ipykernel() {
                self.installing = Some(task);
            }
            return Ok(());
        }
        let Some(page) = self.current() else {
            return Err("no notebook open: open an .ipynb first".into());
        };
        match id {
            "jupyter.run-all" => self.with(page, |p, w| p.run_all(w)),
            "jupyter.interrupt" => self.with(page, |p, w| p.interrupt(w)),
            "jupyter.restart" => self.with(page, |p, w| p.restart(w)),
            "jupyter.clear-all" => self.with(page, |p, w| p.clear_all(w)),
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Opened(path) => self.open(&path),
            Event::ProcessOutput(out) => {
                if let Some(page) = self.by_process(out.process) {
                    let line = out.line;
                    let stderr = out.stderr;
                    self.with(page, |p, w| if stderr { p.said_stderr(&line, w) } else { p.said(&line, w) });
                }
            }
            Event::ProcessExit((process, code)) => {
                if let Some(page) = self.by_process(process) {
                    self.with(page, |p, w| p.exited(code, w));
                }
            }
            Event::Prompted((id, text)) => {
                if let Some((&page, _)) = self.pages.iter().find(|(_, p)| p.asking() == Some(id)) {
                    self.with(page, |p, w| p.answered(text, w));
                }
            }
            Event::PageShown((page, true)) if self.pages.contains_key(&page) => self.last = Some(page),
            Event::TaskFinished((task, ok)) if self.installing == Some(task) => {
                self.installing = None;
                api::notify(installed_said(ok));
            }
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let Some(p) = self.pages.get_mut(&page) else {
            return Page::new().line([pages::muted("Nothing here: the notebook is closed")]);
        };
        let c = p.render(width as usize, height as usize, &mut Fenix { pictures: &mut self.pictures, installing: &mut self.installing });
        Page::canvas(c.to_api()).more_keys(true).mouse(true)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.with(page, |p, w| p.key(key.into(), w));
    }

    /// Shift+Enter, Ctrl+Enter and Alt+Enter run; PageUp and PageDown a
    /// screen; Ctrl+Z and Ctrl+Shift+Z the buffer's undo and redo.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let m = key.mods;
        self.with(page, |p, w| match key.key {
            SurfaceKey::Named(NamedKey::Enter) => p.enter(m.shift, m.ctrl, m.alt, w),
            SurfaceKey::Named(NamedKey::PageUp) => p.key(fenix_ui_kit::Key::PageUp, w),
            SurfaceKey::Named(NamedKey::PageDown) => p.key(fenix_ui_kit::Key::PageDown, w),
            SurfaceKey::Char(c) if m.ctrl && !m.alt => p.ctrl(c, m.shift, w),
            _ => {}
        });
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let m = fenix_ui_kit::Mouse::from(&event);
        self.with(page, |p, w| p.mouse(&m, w));
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        let p = self.pages.get(&page)?;
        let mut s = p.saved();
        if let Some(path) = in_project(&p.full) {
            s.path = path;
        }
        serde_json::to_string(&s).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != NOTEBOOK {
            return false;
        }
        let Ok(saved) = serde_json::from_str::<Saved>(state) else { return false };
        // One in the project is read from there, if it's the same file;
        // one elsewhere Fenix gives it again with its page.
        let path = if saved.path.starts_with("/project/") {
            let root = api::here().and_then(|h| h.project).map(|p| p.root);
            if root.is_some() && in_project(&saved.full).as_deref() != Some(saved.path.as_str()) {
                return false;
            }
            saved.path.clone()
        } else {
            saved.full.clone()
        };
        let Ok(text) = read(&path) else { return false };
        let full = if saved.full.is_empty() { path.clone() } else { saved.full.clone() };
        let mut p = NotebookPage::new(&full, &path, text);
        p.restore(&saved);
        self.pages.insert(page, p);
        true
    }

    /// Vim keys or standard keys now: a page that names a key says it
    /// again.
    fn key_scheme_changed(&mut self, _scheme: KeyScheme) {
        for &page in self.pages.keys() {
            pages::refresh(page);
        }
    }

    fn page_closed(&mut self, page: u64) {
        if let Some(mut p) = self.pages.remove(&page) {
            p.close(&mut Fenix { pictures: &mut self.pictures, installing: &mut self.installing });
        }
    }

    /// A notebook's buffer changed -- typed in its text, undone, replaced
    /// across files -- or one of its cells' editors did.
    fn buffer_changed(&mut self, what: Changed) {
        match what {
            Changed::File(path) => {
                let ids: Vec<u64> = self.pages.iter().filter(|(_, p)| same_file(&p.full, &path)).map(|(&id, _)| id).collect();
                for id in ids {
                    self.with(id, |p, w| {
                        let text = buffer::text(&p.path);
                        p.changed(text, w);
                    });
                }
            }
            Changed::Scratch(scratch) => {
                let Some(text) = scratch::text(scratch) else { return };
                if let Some((&id, _)) = self.pages.iter().find(|(_, p)| p.scratches.contains_key(&scratch)) {
                    self.with(id, |p, w| p.scratch_changed(scratch, &text, w));
                }
            }
        }
    }

    fn scratch_closed(&mut self, scratch: u64) {
        for p in self.pages.values_mut() {
            p.scratches.remove(&scratch);
        }
    }
}

export!(Jupyter);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_the_same_however_fenix_spells_it() {
        assert!(same_file("/home/me/a.ipynb", "/home/me/a.ipynb"));
        assert!(!same_file("/home/me/a.ipynb", "/home/me/A.ipynb"));
        assert!(same_file("C:\\Work\\a.ipynb", "c:/work/a.ipynb"));
    }

    #[test]
    fn a_notebook_runs_in_its_folder() {
        assert_eq!(folder("/home/me/hw/a.ipynb"), "/home/me/hw");
        assert_eq!(folder("C:\\Users\\me\\a.ipynb"), "C:\\Users\\me");
        assert_eq!(folder("/a.ipynb"), "/");
    }

    #[test]
    fn the_kernels_program_is_python_that_compiles() {
        // It's written as it is: a syntax error would only show when run.
        assert!(kernel::SCRIPT.contains("def main():") && kernel::SCRIPT.ends_with("main()\n"));
        assert!(kernel::BACKEND.contains("FigureCanvas = FigureCanvasAgg"));
    }

    #[test]
    fn how_installing_ipykernel_ended_says_what_to_do_next() {
        assert!(installed_said(true).contains("00 on a notebook starts its kernel again"));
        assert!(installed_said(false).contains("wasn't installed") && installed_said(false).contains("task pane"));
    }
}
