//! Arduino in Fenix, as a WebAssembly extension.
//!
//! `state` decides; here what it asks for is carried out through the host,
//! and what the host says is given back to it.

pub mod model;
pub mod state;

use std::path::PathBuf;

use fenix_extension_api::{export, host, project, terminal, Check, Command, Event, Extension, Health, Level, PickerItem, Section, Task};

use state::{Effect, State, Tools};

/// Where an install puts each tool, besides `PATH` and Fenix's tools folder:
/// the Arduino CLI installer, and an Arduino IDE 2 (which brings its own
/// language server and clangd).
const IDE: &str = "%LOCALAPPDATA%/Programs/Arduino IDE/resources/app/lib/backend/resources";
const HINTS: [(&str, &str, &[&str]); 3] = [
    ("arduino-cli", "arduino.cli", &["%ProgramFiles%/Arduino CLI", "%ProgramFiles(x86)%/Arduino CLI", "%LOCALAPPDATA%/Programs/Arduino CLI", IDE, "/usr/local/bin", "/opt/homebrew/bin"]),
    ("clangd", "arduino.clangd", &[IDE]),
    ("arduino-language-server", "arduino.language_server", &[IDE]),
];

/// Each tool: the path its setting gives, else where it's found.
fn tools() -> Tools {
    let mut found = HINTS.iter().map(|(name, setting, hints)| {
        let configured = fenix_extension_api::setting(setting).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        configured.or_else(|| fenix_extension_api::find_program(name, hints))
    });
    Tools { cli: found.next().flatten(), clangd: found.next().flatten(), language_server: found.next().flatten() }
}

#[derive(Default)]
struct Arduino {
    state: Option<State>,
}

impl Arduino {
    fn state(&mut self) -> &mut State {
        self.state.get_or_insert_with(|| State::new("/project", tools()))
    }

    /// An `arduino-cli` command, in the sketch's folder.
    fn cli(&mut self, args: Vec<String>) -> Option<Command> {
        let st = self.state();
        let cli = st.tools.cli.clone()?;
        let dir = st.sketch.clone().unwrap_or_else(|| PathBuf::from("/project"));
        Some(Command::new(&cli).args(args).cwd(&dir.to_string_lossy().replace('\\', "/")))
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        let mut queue = std::collections::VecDeque::from(effects);
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Spawn { job, args } => match self.cli(args).map(|c| c.spawn()) {
                    Some(Ok(process)) => self.state().started(process, job),
                    Some(Err(why)) => fenix_extension_api::error!("arduino-cli: {why}"),
                    None => {}
                },
                Effect::Pick { title, items, pick } => {
                    let items: Vec<PickerItem> = items.into_iter().map(|(label, value)| PickerItem::new(label, value)).collect();
                    let id = fenix_extension_api::pick(&title, &items);
                    self.state().pick_opened(id, pick);
                }
                Effect::Notify(text) => fenix_extension_api::notify(&text),
                Effect::Error(text) => host::notify(Level::Error, &text),
                Effect::Task { name, args, after } => match self.cli(args).map(|c| c.run_as_task(&name)) {
                    Some(Ok(task)) => {
                        if let Some(after) = after {
                            self.state().task_started(task, after);
                        }
                    }
                    Some(Err(why)) => host::notify(Level::Error, &format!("{name}: {why}")),
                    None => {}
                },
                Effect::Monitor { args, title, again } => {
                    let port = args.iter().skip_while(|a| *a != "-p").nth(1).cloned().unwrap_or_default();
                    let Some(cmd) = self.cli(args) else { continue };
                    let result = match again {
                        Some(terminal) => terminal::restart(terminal, Some(&cmd), Some(&title)).map(|()| terminal),
                        None => cmd.run_below(&title),
                    };
                    match result {
                        Ok(terminal) => self.state().monitor_opened(terminal, &port),
                        Err(why) => host::notify(Level::Error, &format!("the serial monitor: {why}")),
                    }
                }
                Effect::TerminalStop(terminal) => terminal::stop(terminal),
                Effect::Console { args, title } => {
                    if let Some(Err(why)) = self.cli(args).map(|c| c.run_below(&title)) {
                        host::notify(Level::Error, &format!("the debugger: {why}"));
                    }
                }
                Effect::Status(text) => fenix_extension_api::status(text.as_deref()),
                Effect::LanguageServer(server) => {
                    let command = server.map(|(program, args)| Command::new(&program).args(args));
                    if let Err(why) = project::language_server(&["c", "cpp"], command.as_ref()) {
                        fenix_extension_api::warn!("the language server: {why}");
                    }
                }
                Effect::RestartLanguageServer => project::restart_language_server(),
                Effect::Doctor(checks) => {
                    let checks: Vec<Check> = checks.into_iter().map(doctor_check).collect();
                    project::doctor(&checks);
                }
                Effect::Tasks(tasks) => {
                    let tasks: Vec<Task> = tasks.into_iter().filter_map(|(name, args)| Some(Task { name, cmd: self.cli(args)? })).collect();
                    if let Err(why) = project::tasks(&tasks) {
                        fenix_extension_api::warn!("its tasks: {why}");
                    }
                }
                Effect::NewProject(args) => fenix_extension_api::new_project(&args),
            }
        }
    }
}

fn doctor_check(c: state::Check) -> Check {
    let section = match c.section {
        state::Section::Toolchain => Section::Toolchain,
        state::Section::Hardware => Section::Hardware,
    };
    let health = match c.health {
        state::Health::Ok => Health::Ok,
        state::Health::Warn => Health::Warn,
        state::Health::Bad => Health::Bad,
    };
    let check = Check::new(section, c.label, health, c.detail);
    match c.fix {
        Some((label, command)) => check.fix(&label, &command),
        None => check,
    }
}

impl Extension for Arduino {
    fn activate(&mut self) -> Result<(), String> {
        let effects = self.state().activate();
        self.apply(effects);
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let effects = self.state().command(id);
        self.apply(effects);
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        let effects = match event {
            Event::FocusChanged(file) => self.state().focus(file.map(PathBuf::from).as_deref()),
            Event::ProcessOutput(o) => {
                self.state().output(o.process, o.stderr, &o.line);
                Vec::new()
            }
            Event::ProcessExit((process, code)) => self.state().exited(process, code),
            Event::Picked((id, value)) => self.state().picked(id, value.as_deref()),
            Event::TaskFinished((task, ok)) => self.state().task_finished(task, ok),
            Event::TerminalClosed(terminal) => {
                self.state().terminal_closed(terminal);
                Vec::new()
            }
            // A tool's path changed: found again, and said again.
            Event::SettingChanged(_) => {
                let st = self.state();
                st.tools = tools();
                let sketch = st.sketch.take();
                let mut out = st.activate();
                out.extend(st.focus(sketch.as_deref()));
                out
            }
            _ => Vec::new(),
        };
        self.apply(effects);
    }
}

export!(Arduino);
