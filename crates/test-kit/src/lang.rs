//! The fake as lang-kit's [`Host`]: a language pack gives it to its
//! [`Kit`](fenix_lang_kit::Kit), and what the kit asks and what the pack
//! asks through [`api`](crate::api) are written down in one log, in the
//! words lang-kit's own mock used.

use fenix_extension_api::{Check, Command, HttpRequest, LaunchConfig, Level, Notification, PickerItem, PlatformInfo, Problem, Task};
use fenix_lang_kit::Host;

use crate::Fake;

impl Host for Fake {
    fn spawn(&mut self, cmd: &Command) -> Result<u64, String> {
        self.state_mut().spawn(cmd, false)
    }
    fn close_stdin(&mut self, process: u64) {
        self.state_mut().close_stdin(process)
    }
    fn kill(&mut self, process: u64) {
        self.state_mut().kill(process)
    }
    fn run_task(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
        self.state_mut().run_task(name, cmd, false)
    }
    fn download(&mut self, req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String> {
        self.state_mut().download(req, path, sha256)
    }
    fn set_executable(&mut self, path: &str) -> Result<(), String> {
        self.state_mut().set_executable(path)
    }
    fn find_program(&mut self, name: &str, _hints: &[String]) -> Option<String> {
        self.state_mut().find_program(name)
    }
    fn set_timer(&mut self, millis: u32) -> u64 {
        self.state_mut().set_timer(millis, false)
    }
    fn prompt(&mut self, label: &str, initial: &str) -> u64 {
        self.state_mut().prompt(label, initial, false)
    }
    fn pick(&mut self, title: &str, items: &[PickerItem]) -> u64 {
        self.state_mut().pick(title, items, false)
    }
    fn notify(&mut self, message: &str) {
        self.state_mut().notify(Level::Info, message)
    }
    /// What Fenix logs as a warning.
    fn log(&mut self, message: &str) {
        self.state_mut().log(Level::Warn, message)
    }
    fn open_page(&mut self, kind: &str, title: &str) -> u64 {
        self.state_mut().open_page(kind, title, None)
    }
    fn refresh(&mut self, page: u64) {
        self.state_mut().refresh(page)
    }
    fn open_text_beside(&mut self, title: &str, text: &str, language: Option<&str>) {
        self.state_mut().open_text(title, text, language, Some(0.4))
    }
    fn doctor(&mut self, checks: &[Check]) {
        self.state_mut().doctor(checks)
    }
    fn tasks(&mut self, tasks: &[Task]) -> Result<(), String> {
        self.state_mut().set_tasks(tasks)
    }
    fn language_server(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.state_mut().language_server(languages, cmd)
    }
    fn debug_adapter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.state_mut().debug_adapter(languages, cmd, None)
    }
    fn formatter(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.state_mut().formatter(languages, cmd, false)
    }
    fn diagnostics(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        self.state_mut().diagnostics(path, problems)
    }
    fn here(&mut self) -> Option<(String, Option<String>)> {
        self.state().here_path()
    }
    fn now(&mut self) -> u64 {
        self.state().now
    }
    fn http(&mut self, req: &HttpRequest) -> Result<u64, String> {
        self.state_mut().http(req)
    }
    fn platform(&mut self) -> PlatformInfo {
        self.state().platform.clone()
    }
    fn open_url(&mut self, url: &str) -> Result<(), String> {
        self.state_mut().open_url(url)
    }
    fn add_language_server(&mut self, name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.state_mut().add_language_server(name, languages, cmd)
    }
    fn language_server_settings(&mut self, name: &str, settings: Option<&fenix_lang_kit::serde_json::Value>) -> Result<(), String> {
        self.state_mut().server_settings(name, settings.map(|s| s.to_string()).as_deref())
    }
    fn debug_adapter_connect(&mut self, languages: &[String], cmd: Option<&Command>, port: u16) -> Result<(), String> {
        self.state_mut().debug_adapter(languages, cmd, Some(port))
    }
    fn formatter_keeping_output(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.state_mut().formatter(languages, cmd, true)
    }
    fn diagnostics_with_fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        self.state_mut().diagnostics(path, problems)
    }
    fn run_task_background(&mut self, name: &str, cmd: &Command) -> Result<u64, String> {
        self.state_mut().run_task(name, cmd, true)
    }
    fn task_cancel(&mut self, task: u64) {
        self.state_mut().task_cancel(task)
    }
    fn task_show(&mut self, task: u64) -> Result<(), String> {
        self.state_mut().task_show(task)
    }
    fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
        self.state_mut().set_launches(launches)
    }
    fn launch_variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
        self.state_mut().launch_variable(name, choices)
    }
    fn notification(&mut self, n: &Notification) {
        self.state_mut().notify_with(n)
    }
    fn dismiss(&mut self, tag: &str) {
        self.state_mut().notify_dismiss(tag)
    }
    /// What a test provider runs (`test-provider-set`, API 1.6), written
    /// down as lang-kit's mock writes it: `test-provider rust: cargo
    /// nextest`. With the feature `tests` (lang-kit's `tests`).
    #[cfg(feature = "tests")]
    fn test_provider_set(&mut self, provider: &str, runner: &str, problem: Option<&str>) -> Result<(), String> {
        self.state_mut().test_provider_set(provider, runner, problem);
        Ok(())
    }
}

impl Fake {
    /// The fake, boxed as a kit's host: `Kit::with_host("zig", fake.host())`.
    pub fn host(&self) -> Box<dyn Host> {
        Box::new(self.clone())
    }

    /// What its test providers said they run, in order: provider, runner,
    /// problem (`test-provider-set`).
    pub fn test_providers(&self) -> Vec<(String, String, Option<String>)> {
        self.state().test_providers.clone()
    }

    /// A server's settings as it last set them (lang-kit's
    /// `language_server_settings`), read as JSON.
    pub fn server_settings(&self, name: &str) -> Option<fenix_lang_kit::serde_json::Value> {
        self.state().server_settings.get(name).and_then(|s| fenix_lang_kit::serde_json::from_str(s).ok())
    }
}
