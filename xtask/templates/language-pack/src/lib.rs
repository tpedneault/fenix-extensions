//! {{name}}, on lang-kit: a language pack's smallest working shape. Its
//! [`doctor`] finds the tool the language needs and says what version it
//! is, its [`tasks`] are what `SPC p t` offers, and the kit does the rest
//! -- the doctor's page, running the tasks, answering `{{folder}}.doctor`.
//!
//! Every host call goes through lang-kit's `Host`: Fenix's in the
//! sandbox, a fake in the tests here, so all of this runs on your machine
//! under `cargo test`.
//!
//! TODO(you): this is a skeleton. Name the language's tool in
//! `doctor.rs`, its commands in `tasks.rs`, and grow from there: a
//! language server (`Kit::install`), a formatter (`Kit::formatter`), a
//! linter (`Kit::linter`), a package manager's page (`Kit::packages`).
//! `crates/lang-kit/README.md` has each, and `extensions/zig` and
//! `extensions/lua` are packs built from them.

pub mod doctor;
pub mod tasks;

#[cfg(test)]
mod data;

use fenix_extension_api::{export, Event, Extension};
use fenix_lang_kit::{Fenix, Host, Kit};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// What the pack's commands start with: `{{folder}}.doctor`. The same as
/// its name in `extension.toml`.
pub const PREFIX: &str = "{{folder}}";

pub struct Pack {
    pub kit: Kit,
}

impl Default for Pack {
    fn default() -> Pack {
        Pack::new(Box::new(Fenix))
    }
}

impl Pack {
    /// The pack, talking to `host`.
    pub fn new(host: Box<dyn Host>) -> Pack {
        let mut kit = Kit::with_host(PREFIX, host).tasks(tasks::tasks());
        for program in doctor::programs() {
            kit.doctor.program(program);
        }
        Pack { kit }
    }
}

impl Extension for Pack {
    /// When it loads: its tasks are set and its checks made.
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()
    }

    /// The kit's commands (`{{folder}}.doctor`); a command of the pack's
    /// own would be matched after it.
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id}")))
    }

    fn handle(&mut self, event: Event) {
        self.kit.handle(&event);
    }
}

export!(Pack);

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::{Health, ProcessOutput};
    use fenix_lang_kit::host::mock::Shared;

    fn pack() -> (Pack, Shared) {
        let host = Shared::default();
        (Pack::new(Box::new(host.clone())), host)
    }

    #[test]
    fn it_sets_its_tasks_and_says_when_the_tool_is_missing() {
        let (mut pack, host) = pack();
        pack.activate().unwrap();
        let names: Vec<String> = host.0.borrow().tasks.iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, ["build", "test"]);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Example tool".to_string(), Health::Warn)], "an optional tool, not found");
    }

    #[test]
    fn it_reads_the_tools_version_when_it_finds_it() {
        let (mut pack, host) = pack();
        host.0.borrow_mut().programs.insert(doctor::TOOL.into(), "/usr/bin/example-tool".into());
        pack.activate().unwrap();
        assert!(host.take().iter().any(|c| c.starts_with("spawn 1 /usr/bin/example-tool --version")));
        pack.handle(Event::ProcessOutput(ProcessOutput { process: 1, stderr: false, line: "example-tool 1.4.2".into() }));
        pack.handle(Event::ProcessExit((1, Some(0))));
        let check = host.0.borrow().checks[0].clone();
        assert_eq!(check.health, Health::Ok);
        assert!(check.detail.contains("1.4.2"), "{}", check.detail);
    }

    #[test]
    fn its_doctor_command_checks_again_and_others_are_refused() {
        let (mut pack, host) = pack();
        assert_eq!(pack.run_command("{{folder}}.doctor"), Ok(()));
        assert_eq!(host.0.borrow().checks.len(), 1);
        assert!(pack.run_command("{{folder}}.nothing").is_err());
    }
}
