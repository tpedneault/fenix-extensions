//! LaTeX and BibTeX in Fenix, on lang-kit: the grammars (their language
//! files and queries), texlab as the language server, tex-fmt (or
//! latexindent) as the formatter, chktex on save, a build with whatever the
//! user has -- latexmk, Tectonic, or an engine -- whose log is read into
//! problems, the PDF opened in the PDF reader ([`preview`]), a forward search
//! to the PDF's page, and the project doctor's checks, with fix rows that
//! install texlab, tex-fmt and Tectonic for this machine.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]). A fix that installs a tool, a
//! setting changed, or a `.tex` file saved in a project that had none sets
//! everything again.

pub mod build;
pub mod chktex;
pub mod doctor;
pub mod log;
pub mod preview;
pub mod project;
pub mod synctex;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use api::{export, Check, Command, Event, Extension, Key, MouseEvent, Page, Severity};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit, Output};

use build::Builder;
use preview::Preview;
use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages: texlab knows them by the same.
const SERVED: &[&str] = &["latex", "bibtex"];
const FORMATTED: &[&str] = &["latex"];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The document, in the project, when it isn't `main.tex`.
    pub main: Option<String>,
    /// A build that worked opens the PDF in the reader.
    pub preview: bool,
    /// Saving a `.tex` file builds the document.
    pub build_on_save: bool,
    /// chktex on a saved file.
    pub chktex: bool,
    /// `auto`, `tex-fmt` or `latexindent`.
    pub formatter: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { main: None, preview: true, build_on_save: false, chktex: true, formatter: "auto".into() }
    }
}

impl Settings {
    /// From Fenix: `preview` and `chktex` are on unless they say `false`, the
    /// other flag off unless it says `true`.
    pub fn read() -> Settings {
        let flag = |key: &str| api::setting(key).map(|v| v.trim().to_ascii_lowercase());
        let formatter = flag("latex.formatter").filter(|f| ["tex-fmt", "latexindent"].contains(&f.as_str())).unwrap_or_else(|| "auto".into());
        Settings {
            main: api::setting("latex.main").map(|m| m.trim().to_string()).filter(|m| !m.is_empty()),
            preview: flag("latex.preview").as_deref() != Some("false"),
            build_on_save: flag("latex.build_on_save").as_deref() == Some("true"),
            chktex: flag("latex.chktex").as_deref() != Some("false"),
            formatter,
        }
    }
}

/// A run in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    /// A build of the document `main`: its PDF and log, and the folder the
    /// log's names are relative to. With `open`, the PDF opened whatever the
    /// setting says; with `watch`, `latexmk -pvc` started after.
    Build { main: String, pdf: String, log: String, base: String, open: bool, watch: bool, builder: Builder },
    /// `synctex view`: the PDF the page is in.
    Forward { pdf: String },
}

pub struct Latex {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
    /// Runs going, the files a build put problems on, the continuous build
    /// in the background, and the PDF's preview.
    runs: Runs<Job>,
    spoke_of: BTreeSet<String>,
    watching: Option<u64>,
    pub preview: Preview,
    /// The project's folder as the machine has it, learned from where you are.
    real_root: Option<String>,
    /// The doctor's check of what builds, which the kit asks for each time
    /// the doctor runs.
    build_check: Rc<RefCell<Option<Check>>>,
}

impl Default for Latex {
    fn default() -> Latex {
        let platform = api::platform();
        Latex::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Latex {
    /// The pack for the project in `root`, talking to `host`, on `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Latex {
        let tools = Tools::find(host.as_mut());
        let build_check = Rc::new(RefCell::new(None));
        let shared = build_check.clone();
        let mut kit = Kit::with_host("latex", host).extra_checks(move || shared.borrow().iter().cloned().collect());
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let settings = read_settings();
        let project = Project::read(root, settings.main.as_deref());
        let mut t = Latex {
            kit,
            root: root.into(),
            project,
            tools,
            platform,
            settings,
            read_settings,
            set_server: None,
            set_formatter: None,
            runs: Runs::new(),
            spoke_of: BTreeSet::new(),
            watching: None,
            preview: Preview::default(),
            real_root: None,
            build_check,
        };
        t.configure();
        t
    }

    /// The document's magic `% !TEX program` comment, from its file.
    fn program_of(&self, main: &str) -> Option<String> {
        let text = std::fs::read_to_string(format!("{}/{main}", self.root)).ok()?;
        project::magic(&text).program
    }

    /// What builds `main`.
    fn builder(&self, main: &str) -> Option<Builder> {
        build::choose(&self.tools, &self.project, self.program_of(main).as_deref())
    }

    /// The kit's tasks, checks and linter, for the project, tools and settings
    /// as they are.
    fn configure(&mut self) {
        let main = self.project.main.clone();
        let builder = main.as_deref().and_then(|m| self.builder(m));
        self.kit.tasks = tasks::tasks(main.as_deref(), builder.as_ref(), &self.tools);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.platform) {
            self.kit.doctor.program(program);
        }
        let program = main.as_deref().and_then(|m| self.program_of(m));
        *self.build_check.borrow_mut() = Some(doctor::build_check(&self.tools, &self.project, &self.platform, program.as_deref()));
        self.kit.lint.linters.clear();
        if let (true, Some(chktex)) = (self.settings.chktex, &self.tools.chktex) {
            self.kit.lint.add(chktex::linter(chktex));
        }
    }

    /// The command that formats a file, by the setting: tex-fmt when it's
    /// found (a single program, no Perl), else latexindent.
    fn formatter_command(&self) -> Option<Command> {
        let tex_fmt = self.tools.tex_fmt.as_deref().map(|p| Command::new(p).arg("--stdin"));
        // latexindent writes an `indent.log` in the folder it runs in unless told not to.
        let null = if self.platform.os == "windows" { "nul" } else { "/dev/null" };
        let indent = self.tools.latexindent.as_deref().map(|p| Command::new(p).args(["-g", null, "-"]));
        match self.settings.formatter.as_str() {
            "tex-fmt" => tex_fmt,
            "latexindent" => indent,
            _ => tex_fmt.or(indent),
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found leaves LaTeX without them.
    fn apply(&mut self) {
        let server = self.tools.texlab.as_deref().map(Command::new);
        let formatter = self.formatter_command();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            let served: Vec<String> = SERVED.iter().map(|l| l.to_string()).collect();
            if let Err(why) = host.language_server(&served, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            let formatted: Vec<String> = FORMATTED.iter().map(|l| l.to_string()).collect();
            if let Err(why) = host.formatter(&formatted, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// The tools as they're found now; set again, and checked again, where
    /// that changed.
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        self.take(tools);
    }

    fn take(&mut self, tools: Tools) {
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            self.set_tasks();
            self.apply();
        }
    }

    /// The project read again, and its tasks set again.
    fn reread(&mut self) {
        self.project = Project::read(&self.root, self.settings.main.as_deref());
        self.configure();
        self.set_tasks();
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.reread();
        self.kit.doctor.run(self.kit.host.as_mut());
        self.apply();
    }

    /// The project's folder as the machine has it, from where you are.
    fn machine_root(&mut self) -> Option<String> {
        if let Some((_, Some(root))) = self.kit.host.here() {
            self.real_root = Some(root);
        }
        self.real_root.clone()
    }

    /// The file you're in, in the project, with its text.
    fn file_here(&mut self) -> Option<(String, String)> {
        let (path, root) = self.kit.host.here()?;
        let rel = fenix_lang_kit::lint::relative(&path, &root?)?;
        let text = std::fs::read_to_string(format!("{}/{rel}", self.root)).ok()?;
        Some((rel, text))
    }

    /// The document to build: the root the file you're in names, or that
    /// file if it's a document, or the project's.
    fn document(&mut self) -> Result<String, String> {
        let here = self.file_here();
        let file = here.as_ref().map(|(p, t)| (p.as_str(), t.as_str()));
        self.project.document(file).ok_or_else(|| "no document to build: add a main.tex, name one in latex.main, or open one (a file with \\documentclass, or % !TEX root = main.tex)".to_string())
    }

    /// Builds the document, its log read into problems when it ends. With
    /// `open`, its PDF opened in the reader whatever the setting says; with
    /// `watch`, `latexmk -pvc` started after.
    pub fn build(&mut self, open: bool, watch: bool) -> Result<(), String> {
        let main = self.document()?;
        let builder = self.builder(&main).ok_or_else(|| format!("nothing here builds a document: {} has a fix that installs Tectonic, or install a TeX distribution", keys_for("latex.doctor", "SPC p h")))?;
        let mut cmd = builder.command(&main, false).ok_or("nothing to run")?;
        cmd.cwd = Some(PROJECT.into());
        self.machine_root();
        let job = Job::Build { pdf: builder.pdf(&main), log: builder.log(&main), base: builder.runs_in(&main), main: main.clone(), open, watch, builder: builder.clone() };
        self.runs.kill_all(self.kit.host.as_mut());
        self.runs.start(self.kit.host.as_mut(), &cmd, job)?;
        self.kit.host.notify(&format!("Building {main} ({})…", builder.describe()));
        Ok(())
    }

    /// A build ended: its problems set, and what came of it said.
    fn built(&mut self, job: Job, out: Output) {
        let Job::Build { main, pdf, log, base, open, watch, builder } = job else { return };
        // The log the engine wrote says more than what it printed; without it, what it printed.
        let text = std::fs::read(format!("{}/{log}", self.root)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|_| out.text());
        let root = self.root.clone();
        let exists = |p: &str| std::path::Path::new(&format!("{root}/{p}")).is_file();
        let files = log::parse(&text, &log::Context { root: &self.root, base: &base, main: &main, exists: &exists });
        let host = self.kit.host.as_mut();
        let mut now = BTreeSet::new();
        let (mut errors, mut warnings) = (0, 0);
        for (file, mut problems) in files {
            for p in &mut problems {
                p.source.get_or_insert_with(|| "latex".to_string());
                match p.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                    _ => {}
                }
            }
            let path = format!("{PROJECT}/{file}");
            if let Err(why) = host.diagnostics(&path, &problems) {
                host.log(&format!("latex build: {why}"));
            }
            now.insert(path);
        }
        // What was wrong last time and isn't now.
        for gone in self.spoke_of.difference(&now) {
            if let Err(why) = host.diagnostics(gone, &[]) {
                host.log(&format!("latex build: {why}"));
            }
        }
        self.spoke_of = now;
        let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
        let counts = match (errors, warnings) {
            (0, 0) => String::new(),
            (e, 0) => format!(": {}", plural(e, "error")),
            (0, w) => format!(": {}", plural(w, "warning")),
            (e, w) => format!(": {}, {}", plural(e, "error"), plural(w, "warning")),
        };
        if out.ok() {
            host.notify(&format!("Built {main} to {pdf}{counts}"));
        } else if errors > 0 {
            host.notify(&format!("{main} didn't build{counts}"));
        } else {
            host.notify(&format!("{main} didn't build: {}", out.why()));
        }
        if watch {
            self.start_watch(&builder, &main);
        }
        if out.ok() && (open || self.settings.preview) {
            if let Some(root) = self.machine_root() {
                let full = preview::full_path(&root, &pdf);
                if open {
                    self.preview.open(&full, None);
                } else {
                    self.preview.after_build(&full);
                }
            } else {
                self.kit.host.notify(&format!("{pdf} is made; open a file of the project and run {} to see it", keys_for("latex.preview", "SPC m p")));
            }
        }
    }

    /// `latexmk -pvc` as a background task, whose output `task-show` brings
    /// up: it builds again whenever a file changes, and the reader reloads.
    fn start_watch(&mut self, builder: &Builder, main: &str) {
        let Some(mut cmd) = builder.command(main, true) else { return };
        cmd.cwd = Some(PROJECT.into());
        match self.kit.host.run_task_background("latexmk -pvc", &cmd) {
            Ok(id) => {
                self.watching = Some(id);
                self.kit.host.notify("latexmk -pvc started: it builds when a file changes (its output: the task's buffer)");
            }
            Err(why) => self.kit.host.log(&format!("latexmk -pvc: {why}")),
        }
    }

    /// Starts the continuous build, or stops the one running.
    fn toggle_watch(&mut self) -> Result<(), String> {
        if let Some(id) = self.watching.take() {
            self.kit.host.task_cancel(id);
            self.kit.host.notify("latexmk -pvc stopped");
            return Ok(());
        }
        let main = self.document()?;
        match self.builder(&main) {
            Some(b) if b.command(&main, true).is_some() => {}
            Some(b) => return Err(format!("building continuously is latexmk's (-pvc): {} doesn't watch. Build again after a change, or install latexmk with a TeX distribution", b.describe())),
            None => return Err("nothing here builds a document: SPC p h has a fix that installs Tectonic".into()),
        }
        // Built once first: it has a PDF to show, and its problems.
        self.build(true, true)
    }

    /// The PDF opened (built first when it isn't there).
    fn show_pdf(&mut self) -> Result<(), String> {
        let main = self.document()?;
        let Some(builder) = self.builder(&main) else { return Err("nothing here builds a document: SPC p h has a fix that installs Tectonic".into()) };
        let pdf = builder.pdf(&main);
        if !std::path::Path::new(&format!("{}/{pdf}", self.root)).is_file() {
            return self.build(true, false);
        }
        let Some(root) = self.machine_root() else { return Err("open a file of the project first: the reader needs the PDF's full path".into()) };
        self.preview.open(&preview::full_path(&root, &pdf), None);
        Ok(())
    }

    /// Forward search: the page the cursor's line is on, in the PDF reader.
    /// The reader takes a page, not a place on it.
    fn forward_search(&mut self) -> Result<(), String> {
        let Some(synctex) = self.tools.synctex.clone() else {
            return Err("forward search needs the synctex program, which comes with a TeX distribution (TeX Live, MiKTeX, MacTeX); Tectonic doesn't ship it".into());
        };
        let main = self.document()?;
        let Some(builder) = self.builder(&main) else { return Err("nothing here builds a document".into()) };
        let pdf = builder.pdf(&main);
        if !std::path::Path::new(&format!("{}/{pdf}", self.root)).is_file() {
            return Err(format!("{pdf} isn't there: build the document first ({})", keys_for("latex.build", "SPC m b")));
        }
        let Some(here) = api::here() else { return Err("no file to search from".into()) };
        let (Some(path), Some(project)) = (here.path.clone(), here.project.clone()) else { return Err("open a file of the project first".into()) };
        let Some(file) = fenix_lang_kit::lint::relative(&path, &project.root) else { return Err("that file isn't in the project".into()) };
        self.real_root = Some(project.root);
        let mut cmd = synctex::view(&synctex, here.line, &file, &pdf);
        cmd.cwd = Some(PROJECT.into());
        self.runs.start(self.kit.host.as_mut(), &cmd, Job::Forward { pdf })?;
        Ok(())
    }

    /// `synctex view` ended: the reader is asked for its page.
    fn forwarded(&mut self, pdf: &str, out: &Output) {
        match synctex::parse(&out.text()) {
            Some(spot) => {
                let Some(root) = self.real_root.clone() else { return };
                self.preview.open(&preview::full_path(&root, pdf), Some(spot.page));
            }
            None => self.kit.host.notify("synctex found nothing for that line: the PDF may be older than the file, or built without SyncTeX data (build again)"),
        }
    }

    /// A task of its own in the task pane: `texcount`, `latexmk -c`.
    fn run_task(&mut self, name: &str, cmd: Option<Command>, missing: &str) -> Result<(), String> {
        let cmd = cmd.ok_or_else(|| missing.to_string())?;
        self.kit.host.run_task(name, &cmd).map(|_| ())
    }
}

/// The keys that run `command`, as Fenix has them, else `fallback`.
fn keys_for(command: &str, fallback: &str) -> String {
    api::key_for(command).unwrap_or_else(|| fallback.to_string())
}

impl Extension for Latex {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match id {
            "latex.build" => self.build(false, false),
            "latex.watch" => self.toggle_watch(),
            "latex.preview" => self.show_pdf(),
            "latex.forward_search" => self.forward_search(),
            "latex.clean" | "latex.clean_all" => {
                let all = id == "latex.clean_all";
                let main = self.document();
                let cmd = main.ok().and_then(|m| self.builder(&m).and_then(|b| b.clean(&m, all)));
                self.run_task(if all { "clean (all)" } else { "clean" }, cmd, "cleaning is latexmk's (latexmk -c): it's the one that knows what it left")
            }
            "latex.word_count" => {
                let main = self.document();
                let cmd = main.ok().and_then(|m| self.tools.texcount.as_deref().map(|t| tasks::word_count(t, &m)));
                self.run_task("word count", cmd, "the word count needs texcount, which comes with a TeX distribution")
            }
            _ => self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here"))),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.runs.owns(&event) {
            match self.runs.event(&event) {
                Some((job @ Job::Build { .. }, out)) => self.built(job, out),
                Some((Job::Forward { pdf }, out)) => self.forwarded(&pdf, &out),
                None => {}
            }
            return;
        }
        if self.preview.handle(self.kit.host.as_mut(), &event) {
            return;
        }
        if let Event::TaskFinished((id, _)) = &event {
            if self.watching == Some(*id) {
                self.watching = None;
            }
        }
        // A `.tex` file saved in a project that had none: it is one now.
        let saved = |ends: &[&str]| matches!(&event, Event::Saved(name) if ends.iter().any(|e| name.ends_with(e)));
        let first = !self.project.latex && saved(&[".tex"]);
        let magic_or_rc = saved(&[".tex", "latexmkrc", "Tectonic.toml"]) && self.project.latex;
        let build_on_save = saved(&[".tex", ".sty", ".cls", ".bib"]) && self.settings.build_on_save;
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("latex."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if first {
            self.reload();
        } else if magic_or_rc {
            // A magic comment or a .latexmkrc may have changed what builds.
            self.reread();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.reread();
            self.apply();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            self.find_again();
        }
        self.follow();
        if build_on_save && self.watching.is_none() {
            if let Err(why) = self.build(false, false) {
                self.kit.host.log(&format!("build on save: {why}"));
            }
        }
    }

    fn file_saved(&mut self, path: &str) {
        self.kit.file_saved(path);
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        self.kit.page(page, width, height).unwrap_or_default()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.kit.page_key(page, key);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        self.kit.page_mouse(page, &event);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.kit.page_state(page)
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        self.kit.page_restore(page, kind, state)
    }

    fn page_closed(&mut self, page: u64) {
        self.kit.page_closed(page);
    }
}

export!(Latex);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, touch};
    use api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    fn pack(dir: &std::path::Path, fake: &Fake) -> Harness<Latex> {
        let mut t = Latex::new(fake.host(), &dir.to_string_lossy(), Settings::read, Platform::new("linux", "x86_64"));
        t.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        Harness::wrap(fake.clone(), t)
    }

    /// The server, the usual builder and the extras installed, saying what they did.
    fn installed() -> Fake {
        let fake = Fake::new();
        for (name, path) in [("texlab", "/t/texlab"), ("latexmk", "/t/latexmk"), ("pdflatex", "/t/pdflatex"), ("xelatex", "/t/xelatex"), ("tex-fmt", "/t/tex-fmt"), ("chktex", "/t/chktex"), ("texcount", "/t/texcount"), ("synctex", "/t/synctex")] {
            fake.program(name, path);
        }
        fake.on_spawn("texlab --version", Ran::ok(include_str!("samples/texlab-version.txt")))
            .on_spawn("tex-fmt --version", Ran::ok(include_str!("samples/tex-fmt-version.txt")))
            .on_spawn("chktex --version", Ran::ok("ChkTeX v1.7.8 - Copyright 1995-96 Jens T. Berger Thielemann.\n"))
            .on_spawn("texcount -version", Ran::ok("TeXcount version 3.2.0.41, 2020-11-22\n"))
            .on_spawn("synctex help", Ran::ok("This is SyncTeX command line utility, version 1.5\n"));
        fake
    }

    fn document(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        touch(&dir, "main.tex", "\\documentclass{article}\n\\begin{document}\nHi\n\\end{document}\n");
        touch(&dir, "chapters/intro.tex", "% !TEX root = ../main.tex\n\\section{A}\n");
        dir
    }

    /// The builds started so far (not the version checks).
    fn builds(fake: &Fake) -> usize {
        fake.calls().iter().filter(|c| c.starts_with("spawn") && c.contains(" -interaction=nonstopmode")).count()
    }

    fn open_here(fake: &Fake, dir: &std::path::Path, file: &str, text: &str) {
        fake.in_file(&format!("{}/{file}", dir.display()), "latex", text).project("orbit", &dir.to_string_lossy());
    }

    #[test]
    fn the_server_the_formatter_the_tasks_and_the_checks_are_set() {
        let dir = document("activate");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let calls = fake.take();
        for want in [
            "server latex,bibtex: /t/texlab",
            "formatter latex: /t/tex-fmt --stdin",
            "tasks build, clean, clean (all), word count",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        let by = |label: &str| checks.iter().find(|c| c.0 == label).unwrap_or_else(|| panic!("{label}: {checks:?}")).clone();
        assert_eq!(by("LaTeX build"), ("LaTeX build".into(), Health::Ok, "latexmk with pdflatex (/t/latexmk)".into()));
        assert_eq!(by("texlab").2, "5.26.0 (/t/texlab)");
        assert_eq!(by("tex-fmt").1, Health::Ok);
        assert_eq!(by("chktex").2, "1.7.8 (/t/chktex)");
        assert_eq!(t.ext.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["chktex"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_formatter_is_tex_fmt_unless_the_setting_or_the_machine_says_latexindent() {
        let dir = document("formatter");
        let fake = Fake::new();
        fake.program("latexindent", "/t/latexindent");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(fake.take().iter().any(|c| c == "formatter latex: /t/latexindent -g /dev/null -"), "only latexindent is here: it's used");

        fake.program("tex-fmt", "/t/tex-fmt");
        t.event(Event::TaskFinished((1, true)));
        assert!(fake.take().iter().any(|c| c == "formatter latex: /t/tex-fmt --stdin"), "tex-fmt once it's found");

        fake.setting("latex.formatter", "latexindent");
        t.event(Event::SettingChanged("latex.formatter".into()));
        assert!(fake.take().iter().any(|c| c == "formatter latex: /t/latexindent -g /dev/null -"));
        fake.setting("latex.formatter", "nonsense");
        t.event(Event::SettingChanged("latex.formatter".into()));
        assert_eq!(t.ext.settings.formatter, "auto");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_build_reads_the_log_the_engine_wrote_into_problems_in_the_files_they_are_in() {
        let dir = document("problems");
        let fake = installed();
        fake.on_spawn("/t/latexmk -pdf", Ran::exits(12));
        std::fs::write(dir.join("main.log"), include_str!("samples/pdflatex-errors.log")).unwrap();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("latex.build").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.contains("/t/latexmk -pdf -interaction=nonstopmode -file-line-error -synctex=1 main.tex")), "{calls:?}");
        assert!(calls.contains(&"problems /project/main.tex 3".to_string()), "{calls:?}");
        assert!(calls.contains(&"problems /project/chapters/intro.tex 2".to_string()), "{calls:?}");
        let ps = fake.problems("/project/chapters/intro.tex");
        assert_eq!((ps[0].start, ps[0].message.as_str(), ps[0].source.as_deref()), ((2, 0), "Missing $ inserted", Some("latex")));
        let process = fake.process("/t/latexmk -pdf").unwrap();
        assert!(process.stdin_closed);
        assert_eq!(process.cmd.cwd.as_deref(), Some("/project"));
        assert!(fake.notifications().iter().any(|n| n.contains("main.tex didn't build: 5 errors")), "{:?}", fake.notifications());
        assert!(fake.services().is_empty(), "no PDF to show");

        // Built: what was wrong is cleared, and the log's warnings are the only problems.
        std::fs::write(dir.join("main.log"), "(./main.tex\n\nLaTeX Warning: Citation `k' on page 1 undefined on input line 3.\n\n)\n").unwrap();
        fake.on_spawn("/t/latexmk -pdf", Ran::ok(""));
        t.command("latex.build").unwrap();
        let calls = fake.take();
        assert!(calls.contains(&"problems /project/chapters/intro.tex 0".to_string()), "{calls:?}");
        assert!(calls.contains(&"problems /project/main.tex 1".to_string()), "{calls:?}");
        assert!(fake.notifications().iter().any(|n| n.starts_with("Built main.tex to main.pdf: 1 warning")), "{:?}", fake.notifications());
        assert!(fake.notifications().iter().any(|n| n == "Building main.tex (latexmk with pdflatex)…"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_log_what_the_builder_printed_is_read_instead() {
        let dir = document("no-log");
        let fake = installed();
        fake.on_spawn("/t/latexmk -pdf", Ran::exits(12).stdout("(./main.tex\n./main.tex:3: Undefined control sequence.\nl.3 \\hi\n"));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("latex.build").unwrap();
        assert_eq!(fake.problems("/project/main.tex")[0].message, "Undefined control sequence: \\hi");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pdf_opens_in_the_reader_once_and_without_the_reader_the_pack_says_what_to_install() {
        let dir = document("preview");
        let fake = installed();
        fake.on_spawn("/t/latexmk -pdf", Ran::ok("")).on_service("pdf.open", Ok(""));
        open_here(&fake, &dir, "main.tex", "");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("latex.build").unwrap();
        let services = fake.services();
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].payload, format!(r#"{{"page":null,"path":"{}/main.pdf"}}"#, dir.display()));
        t.command("latex.build").unwrap();
        assert_eq!(fake.services().len(), 1, "the reader reloads the file itself");
        touch(&dir, "main.pdf", "%PDF-1.7");
        t.command("latex.preview").unwrap();
        assert_eq!(fake.services().len(), 2, "SPC m p brings it forward again");
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = document("no-reader");
        let fake = installed();
        fake.on_spawn("/t/latexmk -pdf", Ran::ok("")).on_service("pdf.open", Err("nothing offers pdf.open"));
        open_here(&fake, &dir, "main.tex", "");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("latex.build").unwrap();
        let toasts = fake.showing();
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        assert_eq!((toasts[0].summary.as_str(), toasts[0].tag.as_deref()), ("No PDF preview", Some("latex.preview")));
        assert!(toasts[0].body.contains(&format!("{}/main.pdf", dir.display())) && toasts[0].body.contains("fenix.pdf"), "{}", toasts[0].body);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_chapter_builds_the_root_it_names_and_a_magic_program_picks_the_engine() {
        let dir = document("root");
        touch(&dir, "paper.tex", "% !TEX program = xelatex\n\\documentclass{article}\n");
        touch(&dir, "chapters/two.tex", "% !TEX root = ../paper.tex\nText\n");
        let fake = installed();
        fake.on_spawn("/t/latexmk", Ran::ok(""));
        open_here(&fake, &dir, "chapters/two.tex", "% !TEX root = ../paper.tex\nText\n");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("latex.build").unwrap();
        assert!(fake.process("/t/latexmk -xelatex -interaction=nonstopmode -file-line-error -synctex=1 paper.tex").is_some(), "{:?}", fake.spawned());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_no_document_it_says_so_and_with_nothing_to_build_with_it_points_to_the_fix() {
        let dir = temp("no-doc");
        touch(&dir, "a.tex", "\\section{A}");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(t.command("latex.build").unwrap_err().starts_with("no document to build"));

        let dir2 = document("nothing");
        let fake2 = Fake::new();
        let mut t2 = pack(&dir2, &fake2);
        t2.activate().unwrap();
        let err = t2.command("latex.build").unwrap_err();
        assert!(err.contains("nothing here builds") && err.contains("SPC p h") && err.contains("Tectonic"), "{err}");
        let check = fake2.checks().into_iter().find(|c| c.label == "LaTeX build").unwrap();
        assert_eq!(check.health, Health::Bad);
        assert_eq!(check.fix.as_ref().map(|f| f.1.as_str()), Some("latex.install.tectonic"));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir2).unwrap();
    }

    #[test]
    fn the_fix_rows_install_each_tool_checked_and_what_was_installed_is_used() {
        let dir = document("fix");
        let fake = Fake::new();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "nothing found, nothing set");
        for (command, url, sha) in [
            ("latex.install.texlab", "https://github.com/latex-lsp/texlab/releases/download/v5.26.0/texlab-x86_64-linux.tar.gz", "8697bd5e479d4584b14b7eed5c320c80ec4e1d91ebefbb6801e6bf38e9971300"),
            ("latex.install.tectonic", "https://github.com/tectonic-typesetting/tectonic/releases/download/tectonic%400.17.0/tectonic-0.17.0-x86_64-unknown-linux-musl.tar.gz", "8533d07f9ccbd7a65824b9e0459041bca34af1eb33daba48f59215593753a3b7"),
            ("latex.install.tex-fmt", "https://github.com/WGUNDERWOOD/tex-fmt/releases/download/v0.5.7/tex-fmt-x86_64-linux.tar.gz", "d0e30b9b811fdc1e66a9328d5c95e9d8e585019288ca39c07f643e0796bcc4a3"),
        ] {
            fake.take();
            t.command(command).unwrap();
            let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap_or_else(|| panic!("{command}"));
            assert!(download.contains(url) && download.ends_with(&format!(" {sha}")), "{download}");
        }
        assert!(t.command("latex.nothing").is_err());

        // Tectonic installed: it builds, with its own flags.
        fake.program("tectonic", "/h/.cargo/bin/tectonic").program("texlab", "/h/.local/bin/texlab");
        t.event(Event::TaskFinished((99, true)));
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "server latex,bibtex: /h/.local/bin/texlab"), "{calls:?}");
        assert!(calls.iter().any(|c| c == "tasks build"), "tasks for what builds now: {calls:?}");
        fake.on_spawn("/h/.cargo/bin/tectonic", Ran::ok(""));
        t.command("latex.build").unwrap();
        assert!(fake.process("/h/.cargo/bin/tectonic -X compile --keep-logs --synctex main.tex").is_some(), "{:?}", fake.spawned());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_install_is_a_notification_that_says_tectonic_downloads_and_offers_again() {
        let dir = temp("install-told");
        let fake = Fake::new();
        fake.on_http_error("https://*", "its SHA-256 isn't the release's");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("latex.install.tectonic").unwrap();
        assert_eq!(fake.notifications(), [format!("Downloading Tectonic {}…", tools::TECTONIC)]);
        let n = &fake.showing()[0];
        assert_eq!((n.summary.clone(), n.tag.as_deref()), (format!("Couldn't install Tectonic {}", tools::TECTONIC), Some("install.tectonic")));
        assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "latex.install.tectonic"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn continuous_builds_are_latexmks_and_again_stops_them() {
        let dir = document("watch");
        let fake = installed();
        fake.on_spawn("/t/latexmk -pdf", Ran::ok("")).on_service("pdf.open", Ok(""));
        open_here(&fake, &dir, "main.tex", "");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("latex.watch").unwrap();
        let calls = fake.take();
        let background: Vec<&String> = calls.iter().filter(|c| c.starts_with("background")).collect();
        assert_eq!(background.len(), 1, "{calls:?}");
        assert!(background[0].ends_with("latexmk -pvc: /t/latexmk -pdf -interaction=nonstopmode -file-line-error -synctex=1 -pvc -view=none main.tex"), "{}", background[0]);
        assert_eq!(fake.services().len(), 1, "its PDF is shown");
        // While it runs, saving doesn't build a second way.
        fake.setting("latex.build_on_save", "true");
        t.event(Event::SettingChanged("latex.build_on_save".into()));
        let before = builds(&fake);
        t.event(Event::Saved("main.tex".into()));
        assert_eq!(builds(&fake), before);
        t.command("latex.watch").unwrap();
        assert!(fake.take().iter().any(|c| c.starts_with("cancel ")), "stopped");
        t.event(Event::Saved("main.tex".into()));
        assert_eq!(builds(&fake), before + 1, "build on save, nothing watching");
        std::fs::remove_dir_all(&dir).unwrap();

        // Tectonic doesn't watch.
        let dir = document("no-watch");
        let fake = Fake::new();
        fake.program("tectonic", "/t/tectonic");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let err = t.command("latex.watch").unwrap_err();
        assert!(err.contains("latexmk's") && err.contains("Tectonic doesn't watch"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn forward_search_asks_synctex_for_the_page_and_the_reader_opens_it() {
        let dir = document("forward");
        let fake = installed();
        fake.on_service("pdf.open", Ok(""));
        fake.on_spawn("/t/synctex view", Ran::ok(include_str!("samples/synctex-view.txt")));
        open_here(&fake, &dir, "chapters/intro.tex", "% !TEX root = ../main.tex\n\\section{A}\n");
        fake.cursor(12, 0);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let err = t.command("latex.forward_search").unwrap_err();
        assert!(err.contains("main.pdf isn't there"), "{err}");

        touch(&dir, "main.pdf", "%PDF-1.7");
        t.command("latex.forward_search").unwrap();
        let asked = fake.process("/t/synctex view").expect("synctex ran");
        assert_eq!(asked.cmd.args.len(), 5);
        assert!(asked.cmd.args[2].ends_with(":0:chapters/intro.tex") && asked.cmd.args[4] == "main.pdf", "{:?}", asked.cmd.args);
        let services = fake.services();
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].payload, format!(r#"{{"page":3,"path":"{}/main.pdf"}}"#, dir.display()), "the page synctex said");

        // Nothing found: said, not opened.
        fake.on_spawn("/t/synctex view", Ran::ok("SyncTeX result begin\nSyncTeX result end\n"));
        t.command("latex.forward_search").unwrap();
        assert_eq!(fake.services().len(), 1);
        assert!(fake.notifications().last().unwrap().starts_with("synctex found nothing"));
        std::fs::remove_dir_all(&dir).unwrap();

        // Without synctex it says what's needed.
        let dir = document("no-synctex");
        let fake = Fake::new();
        fake.program("latexmk", "/t/latexmk");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let err = t.command("latex.forward_search").unwrap_err();
        assert!(err.contains("synctex program") && err.contains("TeX Live"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clean_and_the_word_count_are_tasks_and_chktex_runs_on_a_saved_file() {
        let dir = document("lint");
        let fake = installed();
        fake.on_spawn("/t/chktex", Ran::exits(2).stdout("main.tex:3:1:2:Warning:2:Non-breaking space (~) should have been used.\n"));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("latex.clean").unwrap();
        t.command("latex.clean_all").unwrap();
        t.command("latex.word_count").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.ends_with("clean: /t/latexmk -c main.tex")), "{calls:?}");
        assert!(calls.iter().any(|c| c.ends_with("clean (all): /t/latexmk -C main.tex")), "{calls:?}");
        assert!(calls.iter().any(|c| c.ends_with("word count: /t/texcount -brief -inc -utf8 main.tex")), "{calls:?}");

        let path = format!("{}/main.tex", dir.display());
        fake.in_file(&path, "latex", "").project("orbit", &dir.to_string_lossy());
        t.saved(&path);
        let ps = fake.problems("/project/main.tex");
        assert_eq!((ps.len(), ps[0].source.as_deref(), ps[0].code.as_deref()), (1, Some("chktex"), Some("2")));
        // Off: no linter.
        fake.setting("latex.chktex", "false");
        t.event(Event::SettingChanged("latex.chktex".into()));
        assert!(t.ext.kit.lint.linters.is_empty());

        // Nothing but a builder without latexmk: nothing to clean with, said.
        let dir2 = document("no-clean");
        let fake2 = Fake::new();
        fake2.program("pdflatex", "/t/pdflatex");
        let mut t2 = pack(&dir2, &fake2);
        t2.activate().unwrap();
        assert!(t2.command("latex.clean").unwrap_err().contains("latexmk"));
        assert!(t2.command("latex.word_count").unwrap_err().contains("texcount"));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir2).unwrap();
    }

    #[test]
    fn a_tex_file_saved_in_a_folder_that_had_none_makes_it_a_project() {
        let dir = temp("first-tex");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("tasks")));
        touch(&dir, "main.tex", "\\documentclass{article}\n");
        t.event(Event::Saved("main.tex".into()));
        assert!(fake.take().iter().any(|c| c == "tasks build, clean, clean (all), word count"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
