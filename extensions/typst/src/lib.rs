//! Typst in Fenix, on lang-kit: the grammar (its language file and
//! queries), tinymist as the language server, typstyle as the formatter,
//! `typst compile` and `typst watch` with what they print read into
//! problems, the PDF opened in the PDF reader ([`preview`]), and the project
//! doctor's checks, with fix rows that install each tool for this machine.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]). A fix that installs a tool, a
//! setting changed, or a `.typ` file saved in a project that had none sets
//! everything again.

pub mod compile;
pub mod doctor;
pub mod preview;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use std::collections::BTreeSet;

use api::{export, Command, Event, Extension, Key, MouseEvent, Page, Severity};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit, Output};

use preview::Preview;
use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language: tinymist knows it by the same.
const SERVED: &[&str] = &["typst"];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The document, in the project, when it isn't `main.typ`.
    pub main: Option<String>,
    /// A compile that worked opens the PDF in the reader.
    pub preview: bool,
    /// Saving a `.typ` file compiles the document.
    pub compile_on_save: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { main: None, preview: true, compile_on_save: false }
    }
}

impl Settings {
    /// From Fenix. `preview` is on unless it says `false`; the other flag is
    /// off unless it says `true`.
    pub fn read() -> Settings {
        let flag = |key: &str| api::setting(key).map(|v| v.trim().to_ascii_lowercase());
        Settings {
            main: api::setting("typst.main").map(|m| m.trim().to_string()).filter(|m| !m.is_empty()),
            preview: flag("typst.preview").as_deref() != Some("false"),
            compile_on_save: flag("typst.compile_on_save").as_deref() == Some("true"),
        }
    }
}

/// A compile in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    /// The document and its PDF, in the project.
    main: String,
    pdf: String,
    /// What to do when it ends: open the PDF whatever the setting says
    /// (`typst.preview`), and start `typst watch` (`typst.watch`).
    open: bool,
    watch: bool,
}

pub struct Typst {
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
    /// Compiles running, the files they put problems on, the watch running
    /// in the background, and the PDF's preview.
    runs: Runs<Job>,
    spoke_of: BTreeSet<String>,
    watching: Option<u64>,
    pub preview: Preview,
    /// The project's folder as the machine has it, learned from where you are
    /// (the reader takes full paths).
    real_root: Option<String>,
}

impl Default for Typst {
    fn default() -> Typst {
        let platform = api::platform();
        Typst::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Typst {
    /// The pack for the project in `root`, talking to `host`, on `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Typst {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("typst", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let settings = read_settings();
        let project = Project::read(root, settings.main.as_deref());
        let mut t = Typst {
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
        };
        t.configure();
        t
    }

    /// The kit's tasks and checks, for the project and tools as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.platform) {
            self.kit.doctor.program(program);
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found leaves Typst without them.
    fn apply(&mut self) {
        let server = self.tools.tinymist.as_deref().map(|t| Command::new(t).arg("lsp"));
        let formatter = self.tools.typstyle.as_deref().map(Command::new);
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            let served: Vec<String> = SERVED.iter().map(|l| l.to_string()).collect();
            if let Err(why) = host.language_server(&served, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            let formatted: Vec<String> = SERVED.iter().map(|l| l.to_string()).collect();
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

    /// The document to compile: the project's, else the `.typ` file you're in.
    fn document(&mut self) -> Result<String, String> {
        if let Some(main) = &self.project.main {
            return Ok(main.clone());
        }
        let here = self.kit.host.here();
        let rel = here.and_then(|(path, root)| {
            let rel = fenix_lang_kit::lint::relative(&path, root.as_deref()?)?;
            rel.ends_with(".typ").then_some(rel)
        });
        rel.ok_or_else(|| "no document to compile: add a main.typ, name one in typst.main, or open a .typ file".to_string())
    }

    /// `typst compile` of the document, its problems set when it ends. With
    /// `open`, its PDF opened in the reader whatever the setting says; with
    /// `watch`, `typst watch` started after.
    pub fn compile(&mut self, open: bool, watch: bool) -> Result<(), String> {
        let main = self.document()?;
        if self.tools.typst.is_none() {
            return Err(format!("typst isn't installed: {} has a fix that installs it", keys_for("typst.doctor", "SPC p h")));
        }
        self.machine_root();
        let pdf = Project::pdf(&main);
        let mut cmd = compile::compile(self.tools.typst(), &main, &pdf);
        cmd.cwd = Some(PROJECT.into());
        self.runs.kill_all(self.kit.host.as_mut());
        self.runs.start(self.kit.host.as_mut(), &cmd, Job { main: main.clone(), pdf, open, watch })?;
        self.kit.host.notify(&format!("Compiling {main}…"));
        Ok(())
    }

    /// A compile ended: its problems set, and what came of it said.
    fn compiled(&mut self, job: Job, out: Output) {
        let files = compile::parse(&out.text(), &self.root, &job.main);
        let host = self.kit.host.as_mut();
        let mut now = BTreeSet::new();
        let (mut errors, mut warnings) = (0, 0);
        for (file, mut problems) in files {
            for p in &mut problems {
                p.source.get_or_insert_with(|| "typst".to_string());
                match p.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                    _ => {}
                }
            }
            let path = if file.starts_with('/') || file.as_bytes().get(1) == Some(&b':') { file } else { format!("{PROJECT}/{file}") };
            if let Err(why) = host.diagnostics(&path, &problems) {
                host.log(&format!("typst compile: {why}"));
            }
            now.insert(path);
        }
        // What was wrong last time and isn't now.
        for gone in self.spoke_of.difference(&now) {
            if let Err(why) = host.diagnostics(gone, &[]) {
                host.log(&format!("typst compile: {why}"));
            }
        }
        self.spoke_of = now;
        let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
        if out.ok() {
            let tail = if warnings > 0 { format!(", {}", plural(warnings, "warning")) } else { String::new() };
            host.notify(&format!("Compiled {} to {}{tail}", job.main, job.pdf));
        } else if errors > 0 {
            host.notify(&format!("{} didn't compile: {}", job.main, plural(errors, "error")));
        } else {
            host.notify(&format!("{} didn't compile: {}", job.main, out.why()));
        }
        if job.watch {
            self.start_watch(&job);
        }
        if out.ok() && (job.open || self.settings.preview) {
            if let Some(root) = self.machine_root() {
                let pdf = preview::full_path(&root, &job.pdf);
                if job.open {
                    self.preview.open(&pdf, None);
                } else {
                    self.preview.after_compile(&pdf);
                }
            } else {
                self.kit.host.notify(&format!("{} is made; open a file of the project and run {} to see it", job.pdf, keys_for("typst.preview", "SPC m p")));
            }
        }
    }

    /// `typst watch` as a background task, whose output `task-show` brings
    /// up: it compiles again whenever a file changes, and the reader reloads.
    fn start_watch(&mut self, job: &Job) {
        let mut cmd = compile::watch(self.tools.typst(), &job.main, &job.pdf);
        cmd.cwd = Some(PROJECT.into());
        match self.kit.host.run_task_background("typst watch", &cmd) {
            Ok(id) => {
                self.watching = Some(id);
                self.kit.host.notify("typst watch started: it compiles when a file changes (its output: the task's buffer)");
            }
            Err(why) => self.kit.host.log(&format!("typst watch: {why}")),
        }
    }

    /// Starts `typst watch`, or stops the one running.
    fn toggle_watch(&mut self) -> Result<(), String> {
        if let Some(id) = self.watching.take() {
            self.kit.host.task_cancel(id);
            self.kit.host.notify("typst watch stopped");
            return Ok(());
        }
        // Compiled once first: it has a PDF to show, and its problems.
        self.compile(true, true)
    }

    /// The PDF opened (compiled first when it isn't there).
    fn show_pdf(&mut self) -> Result<(), String> {
        let main = self.document()?;
        let pdf = Project::pdf(&main);
        if !std::path::Path::new(&format!("{}/{pdf}", self.root)).is_file() {
            return self.compile(true, false);
        }
        let Some(root) = self.machine_root() else { return Err("open a file of the project first: the reader needs the PDF's full path".into()) };
        self.preview.open(&preview::full_path(&root, &pdf), None);
        Ok(())
    }
}

/// The keys that run `command`, as Fenix has them, else `fallback`.
fn keys_for(command: &str, fallback: &str) -> String {
    api::key_for(command).unwrap_or_else(|| fallback.to_string())
}

impl Extension for Typst {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match id {
            "typst.compile" => self.compile(false, false),
            "typst.watch" => self.toggle_watch(),
            "typst.preview" => self.show_pdf(),
            _ => self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here"))),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.runs.owns(&event) {
            if let Some((job, out)) = self.runs.event(&event) {
                self.compiled(job, out);
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
        // A `.typ` file or `typst.toml` saved in a project that had none: it is one now.
        let first = matches!(&event, Event::Saved(name) if !self.project.typst && (name.ends_with(".typ") || name.ends_with("typst.toml")));
        let manifest = matches!(&event, Event::Saved(name) if name.ends_with("typst.toml"));
        let compile_on_save = matches!(&event, Event::Saved(name) if name.ends_with(".typ")) && self.settings.compile_on_save;
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("typst."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if first || manifest {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.reread();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            self.find_again();
        }
        self.follow();
        if compile_on_save && self.watching.is_none() {
            if let Err(why) = self.compile(false, false) {
                self.kit.host.log(&format!("compile on save: {why}"));
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

export!(Typst);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, touch};
    use api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    fn pack(dir: &std::path::Path, fake: &Fake) -> Harness<Typst> {
        let mut t = Typst::new(fake.host(), &dir.to_string_lossy(), Settings::read, Platform::new("linux", "x86_64"));
        t.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        Harness::wrap(fake.clone(), t)
    }

    /// Everything installed, saying what it did when recorded.
    fn installed() -> Fake {
        let fake = Fake::new();
        fake.program("typst", "/t/typst")
            .program("tinymist", "/t/tinymist")
            .program("typstyle", "/t/typstyle")
            .on_spawn("typst --version", Ran::ok(include_str!("samples/typst-version.txt")))
            .on_spawn("tinymist --version", Ran::ok(include_str!("samples/tinymist-version.txt")))
            .on_spawn("typstyle --version", Ran::ok(include_str!("samples/typstyle-version.txt")));
        fake
    }

    fn document(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        touch(&dir, "main.typ", "= Orbit\n#lorem(5)\n");
        dir
    }

    /// The compiles started so far (not the version checks).
    fn compiles(fake: &Fake) -> usize {
        fake.calls().iter().filter(|c| c.starts_with("spawn") && c.contains(" compile --diagnostic-format short")).count()
    }

    #[test]
    fn the_server_the_formatter_the_tasks_and_the_checks_are_set() {
        let dir = document("activate");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let calls = fake.take();
        for want in [
            "server typst: /t/tinymist lsp",
            "formatter typst: /t/typstyle",
            "tasks compile, compile (png), fonts, format -check",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[0], ("Typst".into(), Health::Ok, "0.15.1 (/t/typst)".into()));
        assert_eq!(checks[1], ("tinymist".into(), Health::Ok, "bundles Typst 0.15.1".into()));
        assert_eq!(checks[2], ("typstyle".into(), Health::Ok, "0.15.2 (/t/typstyle)".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_compile_that_fails_puts_its_problems_where_they_are_and_a_fixed_one_clears_them() {
        let dir = document("problems");
        let fake = installed();
        fake.on_spawn("/t/typst compile", Ran::exits(1).stderr(include_str!("samples/compile-import-error.txt")));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("typst.compile").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.contains("/t/typst compile --diagnostic-format short --root . main.typ main.pdf")), "{calls:?}");
        assert!(calls.contains(&"problems /project/chapters/intro.typ 1".to_string()), "{calls:?}");
        let ps = fake.problems("/project/chapters/intro.typ");
        assert_eq!((ps[0].start, ps[0].message.as_str(), ps[0].source.as_deref()), ((0, 21), "unknown variable: foo", Some("typst")));
        let process = fake.process("/t/typst compile").unwrap();
        assert!(process.stdin_closed);
        assert_eq!(process.cmd.cwd.as_deref(), Some("/project"));
        assert!(fake.notifications().last().unwrap().contains("main.typ didn't compile: 1 error"), "{:?}", fake.notifications());
        assert!(fake.services().is_empty(), "no PDF to show");

        fake.on_spawn("/t/typst compile", Ran::ok(""));
        t.command("typst.compile").unwrap();
        assert!(fake.take().contains(&"problems /project/chapters/intro.typ 0".to_string()), "what was wrong is cleared");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pdf_opens_in_the_reader_once_and_the_reader_reloads_it_after() {
        let dir = document("preview");
        let fake = installed();
        fake.on_spawn("/t/typst compile", Ran::ok("")).on_service("pdf.open", Ok(""));
        fake.in_file(&format!("{}/main.typ", dir.display()), "typst", "= Orbit\n").project("orbit", &dir.to_string_lossy());
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("typst.compile").unwrap();
        let services = fake.services();
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].name, "pdf.open");
        assert_eq!(services[0].payload, format!(r#"{{"page":null,"path":"{}/main.pdf"}}"#, dir.display()));
        assert!(fake.showing().is_empty(), "the reader was there: nothing to say");

        t.command("typst.compile").unwrap();
        assert_eq!(fake.services().len(), 1, "the reader reloads the file itself");
        assert!(fake.notifications().last().unwrap().starts_with("Compiled main.typ to main.pdf"));

        // SPC m p brings it forward again, once the PDF is there.
        touch(&dir, "main.pdf", "%PDF-1.7");
        t.command("typst.preview").unwrap();
        assert_eq!(fake.services().len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_the_pdf_reader_it_says_what_to_install() {
        let dir = document("no-reader");
        let fake = installed();
        fake.on_spawn("/t/typst compile", Ran::ok("")).on_service("pdf.open", Err("nothing offers pdf.open"));
        fake.in_file(&format!("{}/main.typ", dir.display()), "typst", "").project("orbit", &dir.to_string_lossy());
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("typst.compile").unwrap();
        let toasts = fake.showing();
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        assert_eq!(toasts[0].summary, "No PDF preview");
        assert!(toasts[0].body.contains(&format!("{}/main.pdf", dir.display())) && toasts[0].body.contains("fenix.pdf"), "{}", toasts[0].body);
        // The next compile asks again, in case it was installed meanwhile.
        t.command("typst.compile").unwrap();
        assert_eq!(fake.services().len(), 2);
        assert_eq!(fake.showing().len(), 1, "the toast is tagged: the second replaces the first");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_setting_turns_the_automatic_preview_off_but_the_command_still_opens_it() {
        let dir = document("preview-off");
        let fake = installed();
        fake.setting("typst.preview", "false").on_spawn("/t/typst compile", Ran::ok("")).on_service("pdf.open", Ok(""));
        fake.in_file(&format!("{}/main.typ", dir.display()), "typst", "").project("orbit", &dir.to_string_lossy());
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("typst.compile").unwrap();
        assert!(fake.services().is_empty());
        // No PDF yet: SPC m p compiles first, then opens it.
        t.command("typst.preview").unwrap();
        assert_eq!(compiles(&fake), 2, "a compile first");
        assert_eq!(fake.services().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn watch_compiles_once_then_runs_in_the_background_and_again_stops_it() {
        let dir = document("watch");
        let fake = installed();
        fake.on_spawn("/t/typst compile", Ran::ok("")).on_service("pdf.open", Ok(""));
        fake.in_file(&format!("{}/main.typ", dir.display()), "typst", "").project("orbit", &dir.to_string_lossy());
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("typst.watch").unwrap();
        let calls = fake.take();
        let background: Vec<&String> = calls.iter().filter(|c| c.starts_with("background")).collect();
        assert_eq!(background.len(), 1, "{calls:?}");
        assert!(background[0].ends_with("typst watch: /t/typst watch --diagnostic-format short --root . main.typ main.pdf"), "{}", background[0]);
        assert_eq!(fake.services().len(), 1, "its PDF is shown");
        // While it runs, saving doesn't compile a second way.
        fake.setting("typst.compile_on_save", "true");
        t.event(Event::SettingChanged("typst.compile_on_save".into()));
        let before = compiles(&fake);
        t.event(Event::Saved("main.typ".into()));
        assert_eq!(compiles(&fake), before, "{:?}", fake.calls());

        t.command("typst.watch").unwrap();
        assert!(fake.take().iter().any(|c| c.starts_with("cancel ")), "stopped");
        // Saved with compile_on_save, nothing watching: compiled.
        t.event(Event::Saved("main.typ".into()));
        assert_eq!(compiles(&fake), before + 1, "{:?}", fake.calls());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_no_document_the_file_you_are_in_is_compiled_and_without_one_it_says_so() {
        let dir = temp("no-main");
        touch(&dir, "a.typ", "x");
        touch(&dir, "b.typ", "y");
        let fake = installed();
        fake.on_spawn("/t/typst compile", Ran::ok("")).on_service("pdf.open", Ok(""));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let err = t.command("typst.compile").unwrap_err();
        assert!(err.starts_with("no document to compile"), "{err}");

        fake.in_file(&format!("{}/b.typ", dir.display()), "typst", "y").project("p", &dir.to_string_lossy());
        t.command("typst.compile").unwrap();
        assert!(fake.process("/t/typst compile --diagnostic-format short --root . b.typ b.pdf").is_some(), "{:?}", fake.spawned());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_typst_the_compile_says_where_its_fix_is_and_the_fix_installs_it_checked() {
        let dir = document("fix");
        let fake = Fake::new();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "nothing found, nothing set: {calls:?}");
        let err = t.command("typst.compile").unwrap_err();
        assert!(err.contains("typst isn't installed") && err.contains("SPC p h"), "{err}");
        let fixes: Vec<Option<String>> = t.ext.kit.doctor.programs.iter().map(|p| p.fix.clone().map(|f| f.1)).collect();
        assert_eq!(fixes, [Some("typst.install.typst".into()), Some("typst.install.tinymist".into()), Some("typst.install.typstyle".into())]);

        for (command, url, sha) in [
            ("typst.install.typst", "https://github.com/typst/typst/releases/download/v0.15.1/typst-x86_64-unknown-linux-musl.tar.xz", "a6d077d0a95eed5a2eba715b2dae06be954f624ccbf85758a03f389ded33118c"),
            ("typst.install.tinymist", "https://github.com/Myriad-Dreamin/tinymist/releases/download/v0.15.12/tinymist-linux-x64", "59cd0441dc6789a5715a4e7417ebdb492be07ca3d4ebdf40ade976bea1c1d1ff"),
            ("typst.install.typstyle", "https://github.com/Enter-tainer/typstyle/releases/download/v0.15.2/typstyle-x86_64-unknown-linux-gnu", "ea790059cd05016a6f4282386ab7b4fc92f83faa0f5126d8216df2562d7596fd"),
        ] {
            fake.take();
            t.command(command).unwrap();
            let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap_or_else(|| panic!("{command}"));
            assert!(download.contains(url) && download.ends_with(&format!(" {sha}")), "{download}");
        }
        assert!(t.command("typst.nothing").is_err());

        // Installed some other way, by a task: found and used.
        fake.program("tinymist", "/h/.cargo/bin/tinymist").program("typstyle", "/h/.local/bin/typstyle").program("typst", "/h/.cargo/bin/typst");
        t.event(Event::TaskFinished((99, true)));
        let calls = fake.take();
        for want in ["server typst: /h/.cargo/bin/tinymist lsp", "formatter typst: /h/.local/bin/typstyle"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_install_is_a_notification_that_offers_it_again() {
        let dir = temp("install-told");
        let fake = Fake::new();
        fake.on_http_error("https://*", "its SHA-256 isn't the release's");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.command("typst.install.tinymist").unwrap();
        assert_eq!(fake.notifications(), [format!("Downloading tinymist {}…", tools::TINYMIST)]);
        let n = &fake.showing()[0];
        assert_eq!((n.summary.clone(), n.tag.as_deref()), (format!("Couldn't install tinymist {}", tools::TINYMIST), Some("install.tinymist")));
        assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "typst.install.tinymist"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_typ_file_saved_in_a_folder_that_had_none_makes_it_a_project() {
        let dir = temp("first-typ");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("tasks")));
        touch(&dir, "notes.typ", "hi");
        t.event(Event::Saved("notes.typ".into()));
        assert!(fake.take().iter().any(|c| c == "tasks compile, compile (png), fonts, format -check"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
