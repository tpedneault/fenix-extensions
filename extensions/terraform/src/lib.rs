//! Terraform in Fenix, on lang-kit: the grammar (its language files and
//! queries), terraform-ls as the language server, `terraform fmt` as the
//! formatter, `terraform validate` on save, init, validate, plan and
//! apply as tasks, and the project doctor's checks, with a fix row that
//! installs terraform-ls for this machine.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]). A fix that installs the server,
//! a setting changed, or a `.tf` file appearing in a project that had none
//! sets everything again.

pub mod doctor;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::{Fenix, Host, Kit};

use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages: `.tf`, `.tfvars` (the names
/// terraform-ls knows them by), and `.hcl`.
const SERVED: &[&str] = &["terraform", "terraform-vars"];
const FORMATTED: &[&str] = &["terraform", "terraform-vars", "hcl"];

/// What the pack's setting says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `terraform validate` on a saved `.tf` file.
    pub validate: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { validate: true }
    }
}

impl Settings {
    /// From Fenix: a setting that isn't `false` is on.
    pub fn read() -> Settings {
        Settings { validate: !api::setting("terraform.validate").is_some_and(|v| v.trim().eq_ignore_ascii_case("false")) }
    }
}

pub struct Terraform {
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
}

impl Default for Terraform {
    fn default() -> Terraform {
        let platform = api::platform();
        Terraform::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Terraform {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Terraform {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("terraform", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut t = Terraform { kit, root: root.into(), project: Project::read(root), tools, platform, settings: read_settings(), read_settings, set_server: None, set_formatter: None };
        t.configure();
        t
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.platform) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(terraform)) = (self.settings.validate, &self.tools.terraform) {
            self.kit.lint.add(lint::validate(&Command::new(terraform)));
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found leaves Terraform without them.
    fn apply(&mut self) {
        let server = self.tools.terraform_ls.as_deref().map(|ls| Command::new(ls).arg("serve"));
        let formatter = self.tools.terraform.as_deref().map(|terraform| Command::new(terraform).args(["fmt", "-no-color", "-"]));
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

    /// The tasks set again.
    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// The tools as they're found now; set again, and checked again,
    /// where that changed.
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

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.configure();
        self.set_tasks();
        self.kit.doctor.run(self.kit.host.as_mut());
        self.apply();
    }
}

impl Extension for Terraform {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here")));
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        // A `.tf` file saved in a project that had none: it is one now.
        let first = matches!(&event, Event::Saved(name) if !self.project.is_terraform() && name.ends_with(".tf"));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("terraform."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if first {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            self.set_tasks();
            self.apply();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix put here, or a task (a package manager's) did.
            self.find_again();
        }
        self.follow();
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

export!(Terraform);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use fenix_extension_api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        fenix_test_kit::line(cmd)
    }

    /// The pack for the project in `dir`, on `fake`, its tools folder in
    /// `dir` too.
    fn pack(dir: &std::path::Path, fake: &Fake, settings: fn() -> Settings) -> Harness<Terraform> {
        let mut t = Terraform::new(fake.host(), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        t.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        Harness::wrap(fake.clone(), t)
    }

    /// The processes started, as lines, in order.
    fn spawned(fake: &Fake) -> Vec<String> {
        fake.spawned().into_iter().map(|(_, l)| l).collect()
    }

    /// Terraform and its server installed, saying their versions.
    fn installed() -> Fake {
        let fake = Fake::new();
        fake.program("terraform", "/t/terraform")
            .program("terraform-ls", "/t/terraform-ls")
            .on_spawn("terraform version", Ran::ok(include_str!("samples/terraform-version.txt")))
            .on_spawn("terraform-ls version", Ran::ok(include_str!("samples/terraform-ls-version.txt")));
        fake
    }

    /// A project of one root module.
    fn module(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("main.tf"), "variable \"region\" {\n  type = string\n}\n").unwrap();
        dir
    }

    #[test]
    fn the_server_the_formatter_and_the_tasks_are_set() {
        let dir = module("activate");
        let fake = installed();
        let mut t = pack(&dir, &fake, Settings::default);
        t.activate().unwrap();
        let calls = fake.take();
        for want in [
            "tasks init, validate, fmt -check, plan, apply saved plan",
            "server terraform,terraform-vars: /t/terraform-ls serve",
            "formatter terraform,terraform-vars,hcl: /t/terraform fmt -no-color -",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(spawned(&fake), ["/t/terraform version", "/t/terraform-ls version"]);
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[0], ("Terraform".into(), Health::Ok, "1.17.2 (/t/terraform)".into()));
        assert_eq!(checks[1], ("terraform-ls".into(), Health::Ok, "0.39.1 (/t/terraform-ls)".into()));
        assert_eq!(t.ext.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["terraform validate"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_tf_file_is_validated_and_its_problems_shown() {
        let dir = module("lint");
        let fake = Fake::new();
        fake.program("terraform", "/t/terraform").on_spawn("/t/terraform validate -json -no-color", Ran::exits(1).stdout(include_str!("samples/validate-errors.json")));
        let mut t = pack(&dir, &fake, Settings::default);
        t.activate().unwrap();
        fake.take();
        let path = format!("{}/main.tf", dir.display());
        fake.in_file(&path, "terraform", "output \"name\" {\n}\n").project("lint", &dir.to_string_lossy());
        t.saved(&path);
        let calls = fake.take();
        assert!(calls.contains(&"problems /project/main.tf 2".to_string()), "{calls:?}");
        let problems = fake.problems("/project/main.tf");
        assert_eq!((problems.len(), problems[0].start, problems[0].source.as_deref()), (2, (17, 10), Some("terraform validate")));
        let process = fake.process("/t/terraform validate -json -no-color").expect("validate ran");
        assert!(process.stdin_closed);

        // Fixed: clean again.
        fake.on_spawn("/t/terraform validate -json -no-color", Ran::ok(include_str!("samples/validate-valid.json")));
        t.saved(&path);
        assert!(fake.take().contains(&"problems /project/main.tf 0".to_string()));

        // Off: no linter.
        t.ext.read_settings = || Settings { validate: false };
        t.event(Event::SettingChanged("terraform.validate".into()));
        assert!(t.ext.kit.lint.linters.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_terraform_ls_its_fix_row_installs_it_and_its_used() {
        let dir = module("fix");
        let fake = Fake::new();
        let mut t = pack(&dir, &fake, Settings::default);
        t.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "nothing found, nothing set: {calls:?}");
        assert!(t.ext.kit.lint.linters.is_empty(), "no Terraform to validate with");
        assert_eq!(t.ext.kit.doctor.programs.iter().map(|p| p.fix.clone().map(|f| f.1)).collect::<Vec<_>>(), [None, Some("terraform.install.terraform-ls".into())]);
        t.command("terraform.install.terraform-ls").unwrap();
        let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://releases.hashicorp.com/terraform-ls/0.39.1/terraform-ls_0.39.1_linux_amd64.zip") && download.ends_with(" fe2a158e4a1a8ec8047dae9e06c439b468412aa579929ba28b2e1258cedef9c0"), "{download}");
        assert!(t.command("terraform.nothing").is_err());

        // Installed some other way, by a task: found and used.
        fake.program("terraform", "/h/.tfenv/bin/terraform").program("terraform-ls", "/h/.local/bin/terraform-ls");
        t.event(Event::TaskFinished((99, true)));
        let calls = fake.take();
        for want in ["server terraform,terraform-vars: /h/.local/bin/terraform-ls serve", "formatter terraform,terraform-vars,hcl: /h/.tfenv/bin/terraform fmt -no-color -"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(t.ext.kit.lint.linters.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_isnt_terraform_has_no_tasks_until_a_tf_file_is_saved() {
        let dir = temp("empty");
        let fake = installed();
        let mut t = pack(&dir, &fake, Settings::default);
        t.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("tasks")), "{calls:?}");
        assert_eq!(fake.checks()[0].health, Health::Ok, "found, its version said");

        std::fs::write(dir.join("main.tf"), "").unwrap();
        t.event(Event::Saved("main.tf".into()));
        assert!(fake.take().iter().any(|c| c == "tasks init, validate, fmt -check, plan, apply saved plan"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An install takes minutes: how it ended is a notification, the
    /// newest for the tool in place of the last, saying what failed and
    /// offering it again.
    #[test]
    fn a_failed_install_of_the_server_is_a_notification_that_offers_it_again() {
        let dir = temp("install-told");
        let fake = Fake::new();
        fake.on_http_error("https://*", "its SHA-256 isn't the release's");
        let mut t = pack(&dir, &fake, Settings::default);
        t.activate().unwrap();
        t.command("terraform.install.terraform-ls").unwrap();
        assert_eq!(fake.notifications(), [format!("Downloading terraform-ls {}…", tools::LS)]);
        let shown = fake.showing();
        assert_eq!(shown.len(), 1, "{shown:?}");
        let n = &shown[0];
        assert_eq!((n.summary.clone(), n.urgency, n.tag.as_deref()), (format!("Couldn't install terraform-ls {}", tools::LS), api::Urgency::Normal, Some("install.terraform-ls")));
        assert!(n.body.starts_with("Its download from https://releases.hashicorp.com/") && n.body.ends_with("failed: its SHA-256 isn't the release's."), "{}", n.body);
        assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "terraform.install.terraform-ls"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
