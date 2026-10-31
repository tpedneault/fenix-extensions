//! What lang-kit asks of Fenix, as a trait: [`Fenix`] makes the calls,
//! and a test gives its own that writes them down, so everything else
//! here runs and tests on the host.

use fenix_extension_api as api;
use fenix_extension_api::{Check, Command, HttpRequest, PickerItem, PlatformInfo, Problem, Task};
use fenix_extension_api::LaunchConfig;

/// The host calls lang-kit makes.
pub trait Host {
    /// Starts `cmd` with its stdout in lines; [`Host::close_stdin`] after.
    fn spawn(&mut self, cmd: &Command) -> Result<u64, String>;
    fn close_stdin(&mut self, process: u64);
    fn kill(&mut self, process: u64);
    /// Runs `cmd` as a task called `name`, in the task pane.
    fn run_task(&mut self, name: &str, cmd: &Command) -> Result<u64, String>;
    fn download(&mut self, req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String>;
    fn set_executable(&mut self, path: &str) -> Result<(), String>;
    fn find_program(&mut self, name: &str, hints: &[String]) -> Option<String>;
    fn set_timer(&mut self, millis: u32) -> u64;
    fn prompt(&mut self, label: &str, initial: &str) -> u64;
    fn pick(&mut self, title: &str, items: &[PickerItem]) -> u64;
    fn notify(&mut self, message: &str);
    fn log(&mut self, message: &str);
    fn open_page(&mut self, kind: &str, title: &str) -> u64;
    fn refresh(&mut self, page: u64);
    fn open_text_beside(&mut self, title: &str, text: &str, language: Option<&str>);
    fn doctor(&mut self, checks: &[Check]);
    fn tasks(&mut self, tasks: &[Task]) -> Result<(), String>;
    fn language_server(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
    fn debug_adapter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
    fn formatter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
    fn diagnostics(&mut self, path: &str, problems: &[Problem]) -> Result<(), String>;
    /// The file the command or event is about (`here`), as a full path,
    /// and its project's folder.
    fn here(&mut self) -> Option<(String, Option<String>)>;
    /// Seconds since 1970.
    fn now(&mut self) -> u64;

    // Later calls, each with what a host that hasn't it does: says so, or
    // nothing. `Fenix` makes those of 0.8.5 and after with the `later`
    // feature only, for a pack whose Cargo.lock reaches API 0.8.7; without
    // it they're these defaults, and a pack on 0.8.4 builds as it did.

    /// An HTTP request (`permissions.network`): its answer comes as
    /// `Event::HttpResponse`.
    fn http(&mut self, _req: &HttpRequest) -> Result<u64, String> {
        Err("this host can't make HTTP requests".into())
    }
    /// The machine Fenix runs on: what picks a release's file.
    fn platform(&mut self) -> PlatformInfo {
        PlatformInfo { os: std::env::consts::OS.into(), arch: std::env::consts::ARCH.into(), env: String::new() }
    }
    fn open_url(&mut self, _url: &str) -> Result<(), String> {
        Err("this host can't open a browser".into())
    }
    /// A language server called `name` beside the others for `languages`
    ///; none takes it away.
    fn add_language_server(&mut self, _name: &str, _languages: &[String], _cmd: Option<&Command>) -> Result<(), String> {
        Err("this host can't add a language server".into())
    }
    /// What server `name` is given when it asks (`workspace/configuration`,
    ///); none takes it away.
    fn language_server_settings(&mut self, _name: &str, _settings: Option<&serde_json::Value>) -> Result<(), String> {
        Err("this host can't give a server settings".into())
    }
    /// A debug adapter over a socket: `cmd` started, connected to at
    /// 127.0.0.1:`port` (0, a free one, `${port}` in its args).
    fn debug_adapter_connect(&mut self, _languages: &[String], _cmd: Option<&Command>, _port: u16) -> Result<(), String> {
        Err("this host can't connect to a debug adapter".into())
    }
    /// The formatter, its output kept when it fails having written some
    /// (0.8.7, ktlint).
    fn formatter_keeping_output(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.formatter(languages, cmd)
    }
    /// Problems with their fixes, offered by `SPC c a`.
    fn diagnostics_with_fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        let plain: Vec<Problem> = problems.iter().map(|p| p.clone()).collect();
        self.diagnostics(path, &plain)
    }
    /// A task beside the task pane: several run at once.
    fn run_task_background(&mut self, _name: &str, _cmd: &Command) -> Result<u64, String> {
        Err("this host can't run a task in the background".into())
    }
    fn task_cancel(&mut self, _task: u64) {}
    fn task_show(&mut self, _task: u64) -> Result<(), String> {
        Err("this host can't show a task".into())
    }
    /// The launches it works out, offered beside its launch files.
    fn launches(&mut self, _launches: &[LaunchConfig]) -> Result<(), String> {
        Err("this host can't take launches".into())
    }
    /// `${name}` in its launches filled in from `choices`.
    fn launch_variable(&mut self, _name: &str, _choices: &[PickerItem]) -> Result<(), String> {
        Err("this host can't take launch variables".into())
    }
}

/// The real thing: each call is Fenix's.
#[derive(Debug, Default, Clone, Copy)]
pub struct Fenix;

fn strings(languages: &[String]) -> Vec<&str> {
    languages.iter().map(String::as_str).collect()
}

impl Host for Fenix {
    fn spawn(&mut self, cmd: &Command) -> Result<u64, String> {
        cmd.spawn()
    }
    fn close_stdin(&mut self, process: u64) {
        api::process::close_stdin(process)
    }
    fn kill(&mut self, process: u64) {
        api::process::kill(process)
    }
    fn run_task(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
        cmd.run_as_task(name)
    }
    fn download(&mut self, req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String> {
        req.download(path, sha256)
    }
    fn set_executable(&mut self, path: &str) -> Result<(), String> {
        api::tools::set_executable(path)
    }
    fn find_program(&mut self, name: &str, hints: &[String]) -> Option<String> {
        api::find_program(name, &strings(hints))
    }
    fn set_timer(&mut self, millis: u32) -> u64 {
        api::host::set_timer(millis, false)
    }
    fn prompt(&mut self, label: &str, initial: &str) -> u64 {
        api::prompt(label, initial)
    }
    fn pick(&mut self, title: &str, items: &[PickerItem]) -> u64 {
        api::pick(title, items)
    }
    fn notify(&mut self, message: &str) {
        api::notify(message)
    }
    fn log(&mut self, message: &str) {
        api::host::log(api::Level::Warn, message)
    }
    fn open_page(&mut self, kind: &str, title: &str) -> u64 {
        api::page::open(kind, title)
    }
    fn refresh(&mut self, page: u64) {
        api::page::refresh(page)
    }
    fn open_text_beside(&mut self, title: &str, text: &str, language: Option<&str>) {
        api::open_text_beside(title, text, language, 0.4)
    }
    fn doctor(&mut self, checks: &[Check]) {
        api::project::doctor(checks)
    }
    fn tasks(&mut self, tasks: &[Task]) -> Result<(), String> {
        api::project::tasks(tasks)
    }
    fn language_server(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        api::project::language_server(&strings(languages), cmd)
    }
    fn debug_adapter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        api::project::debug_adapter(&strings(languages), cmd)
    }
    fn formatter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        api::project::formatter(&strings(languages), cmd)
    }
    fn diagnostics(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        api::diagnostics::set(path, problems)
    }
    fn here(&mut self) -> Option<(String, Option<String>)> {
        let here = api::here()?;
        Some((here.path?, here.project.map(|p| p.root)))
    }
    fn now(&mut self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    fn http(&mut self, req: &HttpRequest) -> Result<u64, String> {
        req.send()
    }
    fn platform(&mut self) -> PlatformInfo {
        api::platform()
    }
    fn open_url(&mut self, url: &str) -> Result<(), String> {
        api::open_url(url)
    }
    fn add_language_server(&mut self, name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        api::project::add_language_server(name, &strings(languages), cmd)
    }
    fn language_server_settings(&mut self, name: &str, settings: Option<&serde_json::Value>) -> Result<(), String> {
        api::project::language_server_settings(name, settings)
    }
    fn debug_adapter_connect(&mut self, languages: &[String], cmd: Option<&Command>, port: u16) -> Result<(), String> {
        api::project::debug_adapter_connect(&strings(languages), cmd, port)
    }
    fn formatter_keeping_output(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        api::project::formatter_keeping_output(&strings(languages), cmd)
    }
    fn diagnostics_with_fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        api::diagnostics::set(path, problems)
    }
    fn run_task_background(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
        cmd.run_in_background(name)
    }
    fn task_cancel(&mut self, task: u64) {
        api::task::cancel(task)
    }
    fn task_show(&mut self, task: u64) -> Result<(), String> {
        api::task::show(task)
    }
    fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
        api::debug::launches(launches)
    }
    fn launch_variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
        api::debug::variable(name, choices)
    }
}

/// A host for tests: ids counted up, each call written down as a line,
/// programs found from `programs`, `here` as set. A pack's own tests have
/// it with the `mock` feature (`[dev-dependencies]`).
#[cfg(any(test, feature = "mock"))]
pub mod mock {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Default)]
    pub struct Mock {
        pub next: u64,
        pub calls: Vec<String>,
        pub programs: HashMap<String, String>,
        pub here: Option<(String, Option<String>)>,
        pub now: u64,
        pub checks: Vec<Check>,
        pub tasks: Vec<Task>,
        pub problems: HashMap<String, Vec<Problem>>,
        /// What each download's file holds, by URL: written to its path.
        pub files: HashMap<String, Vec<u8>>,
        pub spawn_fails: bool,
        /// What `platform` says: Linux on x86_64 unless a test says.
        pub platform: Option<PlatformInfo>,
        /// Problems given with their fixes, by path.
        pub fixes: HashMap<String, Vec<Problem>>,
        /// Servers' settings, by name.
        pub settings: HashMap<String, serde_json::Value>,
        pub launches: Vec<LaunchConfig>,
        /// Launch variables' choices, by name.
        pub variables: HashMap<String, Vec<PickerItem>>,
    }

    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    impl Mock {
        fn id(&mut self) -> u64 {
            self.next += 1;
            self.next
        }

        /// The calls made since last asked.
        pub fn take(&mut self) -> Vec<String> {
            std::mem::take(&mut self.calls)
        }
    }

    impl Host for Mock {
        fn spawn(&mut self, cmd: &Command) -> Result<u64, String> {
            if self.spawn_fails {
                self.calls.push(format!("spawn failed {}", line(cmd)));
                return Err(format!("{} isn't there", cmd.program));
            }
            let id = self.id();
            self.calls.push(format!("spawn {id} {}", line(cmd)));
            Ok(id)
        }
        fn close_stdin(&mut self, process: u64) {
            self.calls.push(format!("close {process}"));
        }
        fn kill(&mut self, process: u64) {
            self.calls.push(format!("kill {process}"));
        }
        fn run_task(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
            let id = self.id();
            self.calls.push(format!("task {id} {name}: {}", line(cmd)));
            Ok(id)
        }
        fn download(&mut self, req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String> {
            let id = self.id();
            self.calls.push(format!("download {id} {} -> {path} {}", req.url, sha256.unwrap_or("-")));
            if let Some(bytes) = self.files.get(&req.url) {
                std::fs::write(path, bytes).map_err(|e| e.to_string())?;
            }
            Ok(id)
        }
        fn set_executable(&mut self, path: &str) -> Result<(), String> {
            self.calls.push(format!("executable {path}"));
            Ok(())
        }
        fn find_program(&mut self, name: &str, _hints: &[String]) -> Option<String> {
            self.programs.get(name).cloned()
        }
        fn set_timer(&mut self, millis: u32) -> u64 {
            let id = self.id();
            self.calls.push(format!("timer {id} {millis}"));
            id
        }
        fn prompt(&mut self, label: &str, initial: &str) -> u64 {
            let id = self.id();
            self.calls.push(format!("prompt {id} {label} [{initial}]"));
            id
        }
        fn pick(&mut self, title: &str, items: &[PickerItem]) -> u64 {
            let id = self.id();
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            self.calls.push(format!("pick {id} {title}: {}", labels.join(", ")));
            id
        }
        fn notify(&mut self, message: &str) {
            self.calls.push(format!("notify {message}"));
        }
        fn log(&mut self, message: &str) {
            self.calls.push(format!("log {message}"));
        }
        fn open_page(&mut self, kind: &str, title: &str) -> u64 {
            let id = self.id();
            self.calls.push(format!("page {id} {kind} {title}"));
            id
        }
        fn refresh(&mut self, page: u64) {
            self.calls.push(format!("refresh {page}"));
        }
        fn open_text_beside(&mut self, title: &str, _text: &str, _language: Option<&str>) {
            self.calls.push(format!("text {title}"));
        }
        fn doctor(&mut self, checks: &[Check]) {
            self.checks = checks.to_vec();
            self.calls.push(format!("doctor {}", checks.len()));
        }
        fn tasks(&mut self, tasks: &[Task]) -> Result<(), String> {
            self.tasks = tasks.to_vec();
            self.calls.push(format!("tasks {}", tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")));
            Ok(())
        }
        fn language_server(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
            self.calls.push(format!("server {}: {}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn debug_adapter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
            self.calls.push(format!("adapter {}: {}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn formatter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
            self.calls.push(format!("formatter {}: {}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn diagnostics(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
            self.calls.push(format!("problems {path} {}", problems.len()));
            self.problems.insert(path.into(), problems.to_vec());
            Ok(())
        }
        fn here(&mut self) -> Option<(String, Option<String>)> {
            self.here.clone()
        }
        fn now(&mut self) -> u64 {
            self.now
        }
        fn http(&mut self, req: &HttpRequest) -> Result<u64, String> {
            let id = self.id();
            self.calls.push(format!("http {id} {} {}", req.method, req.url));
            Ok(id)
        }
        fn platform(&mut self) -> PlatformInfo {
            self.platform.clone().unwrap_or_else(|| PlatformInfo { os: "linux".into(), arch: "x86_64".into(), env: "gnu".into() })
        }
        fn open_url(&mut self, url: &str) -> Result<(), String> {
            self.calls.push(format!("url {url}"));
            Ok(())
        }
        fn add_language_server(&mut self, name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
            self.calls.push(format!("add {name} {}: {}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn language_server_settings(&mut self, name: &str, settings: Option<&serde_json::Value>) -> Result<(), String> {
            self.calls.push(format!("settings {name}: {}", settings.map(|s| s.to_string()).unwrap_or_default()));
            match settings {
                Some(s) => self.settings.insert(name.into(), s.clone()),
                None => self.settings.remove(name),
            };
            Ok(())
        }
        fn debug_adapter_connect(&mut self, languages: &[String], cmd: Option<&Command>, port: u16) -> Result<(), String> {
            self.calls.push(format!("connect {}: {} :{port}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn formatter_keeping_output(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
            self.calls.push(format!("formatter {}: {} (kept)", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
        fn diagnostics_with_fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
            let fixes: usize = problems.iter().map(|p| p.fixes.len()).sum();
            self.calls.push(format!("problems {path} {} ({fixes} fixes)", problems.len()));
            self.problems.insert(path.into(), problems.iter().map(|p| p.clone()).collect());
            self.fixes.insert(path.into(), problems.to_vec());
            Ok(())
        }
        fn run_task_background(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
            let id = self.id();
            self.calls.push(format!("background {id} {name}: {}", line(cmd)));
            Ok(id)
        }
        fn task_cancel(&mut self, task: u64) {
            self.calls.push(format!("cancel {task}"));
        }
        fn task_show(&mut self, task: u64) -> Result<(), String> {
            self.calls.push(format!("show {task}"));
            Ok(())
        }
        fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
            self.calls.push(format!("launches {}", launches.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")));
            self.launches = launches.to_vec();
            Ok(())
        }
        fn launch_variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
            self.calls.push(format!("variable {name}: {}", choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")));
            if choices.is_empty() {
                self.variables.remove(name);
            } else {
                self.variables.insert(name.into(), choices.to_vec());
            }
            Ok(())
        }
    }
    /// The mock, shared: a test keeps one and gives the [`crate::Kit`]
    /// the other, and looks at the calls while the kit has it.
    #[derive(Clone, Default)]
    pub struct Shared(pub Rc<RefCell<Mock>>);

    impl Shared {
        /// The calls made since last asked.
        pub fn take(&self) -> Vec<String> {
            self.0.borrow_mut().take()
        }
    }

    macro_rules! pass {
        ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
            impl Host for Shared {
                $(fn $name(&mut self, $($arg: $ty),*) -> $ret { self.0.borrow_mut().$name($($arg),*) })*
                fn diagnostics_with_fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
                    self.0.borrow_mut().diagnostics_with_fixes(path, problems)
                }
                fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
                    self.0.borrow_mut().launches(launches)
                }
            }
        };
    }

    pass! {
        spawn(cmd: &Command) -> Result<u64, String>;
        close_stdin(process: u64) -> ();
        kill(process: u64) -> ();
        run_task(name: &str, cmd: &Command) -> Result<u64, String>;
        download(req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String>;
        set_executable(path: &str) -> Result<(), String>;
        find_program(name: &str, hints: &[String]) -> Option<String>;
        set_timer(millis: u32) -> u64;
        prompt(label: &str, initial: &str) -> u64;
        pick(title: &str, items: &[PickerItem]) -> u64;
        notify(message: &str) -> ();
        log(message: &str) -> ();
        open_page(kind: &str, title: &str) -> u64;
        refresh(page: u64) -> ();
        open_text_beside(title: &str, text: &str, language: Option<&str>) -> ();
        doctor(checks: &[Check]) -> ();
        tasks(tasks: &[Task]) -> Result<(), String>;
        language_server(languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
        debug_adapter(languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
        formatter(languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
        diagnostics(path: &str, problems: &[Problem]) -> Result<(), String>;
        here() -> Option<(String, Option<String>)>;
        now() -> u64;
        http(req: &HttpRequest) -> Result<u64, String>;
        platform() -> PlatformInfo;
        open_url(url: &str) -> Result<(), String>;
        add_language_server(name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
        language_server_settings(name: &str, settings: Option<&serde_json::Value>) -> Result<(), String>;
        debug_adapter_connect(languages: &[String], cmd: Option<&Command>, port: u16) -> Result<(), String>;
        formatter_keeping_output(languages: &[String], cmd: Option<&Command>) -> Result<(), String>;
        run_task_background(name: &str, cmd: &Command) -> Result<u64, String>;
        task_cancel(task: u64) -> ();
        task_show(task: u64) -> Result<(), String>;
        launch_variable(name: &str, choices: &[PickerItem]) -> Result<(), String>;
    }
}
