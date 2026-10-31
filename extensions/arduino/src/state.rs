//! What the extension does, as plain logic: the sketch you're in, what
//! `arduino-cli` said, what a key or a pick does next. It asks for work
//! as `Effect`s, which `lib.rs` carries out through the host, so it's all
//! tested here natively, with no `arduino-cli`.
//!
//! Every `arduino-cli` run works in the sketch's folder (`cwd`), so the
//! sketch is `.` to it: the extension's paths are its own (`/project/..`),
//! not the machine's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{self, Board, BoardOption, Library, Package, Port, BAUD_RATES};

/// Where it found the tools (`find-program`): the machine's paths.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tools {
    pub cli: Option<String>,
    pub clangd: Option<String>,
    pub language_server: Option<String>,
}

/// What a port is wanted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Upload,
    Monitor,
    Debug,
}

/// What an `arduino-cli` run is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// `config get directories.data`: where its configuration lives.
    Config,
    Ports { then: Option<Action> },
    Boards,
    Options { board: String },
    LibrarySearch,
    LibraryList { search: String },
    Packages,
    Debug { port: Option<String> },
    /// For the doctor: the installed cores, and what's plugged in.
    Cores,
    Plugged,
}

/// What to do when a task it ran ends.
#[derive(Debug, Clone, PartialEq)]
pub enum After {
    /// An upload: the monitor it stopped comes back either way.
    Upload { reopen: bool },
    /// A debug build was flashed: the debugger's console, with these.
    Debugger { args: Vec<String> },
    /// Something the language server should see (a library, a core).
    RestartLanguageServer,
}

/// What a picker was for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PickFor {
    pub then: Option<Action>,
    pub board: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Toolchain,
    Hardware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    Warn,
    Bad,
}

/// One doctor line: its fix runs one of its commands.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub section: Section,
    pub label: String,
    pub health: Health,
    pub detail: String,
    pub fix: Option<(String, String)>,
}

fn check(section: Section, label: &str, health: Health, detail: impl Into<String>) -> Check {
    Check { section, label: label.into(), health, detail: detail.into(), fix: None }
}

impl Check {
    fn fix(mut self, label: &str, command: &str) -> Check {
        self.fix = Some((label.into(), command.into()));
        self
    }
}

/// Work it asks the host for. Every program here is `arduino-cli`, run in
/// the sketch's folder, but the language server's.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Spawn { job: Job, args: Vec<String> },
    Pick { title: String, items: Vec<(String, String)>, pick: PickFor },
    Notify(String),
    Error(String),
    Task { name: String, args: Vec<String>, after: Option<After> },
    /// The serial monitor: started again in terminal `again`, else in a
    /// new pane under yours.
    Monitor { args: Vec<String>, title: String, again: Option<u64> },
    TerminalStop(u64),
    /// The debugger's console, in a pane under yours.
    Console { args: Vec<String>, title: String },
    Status(Option<String>),
    /// The language server for C and C++ in the sketch: the program, and
    /// its arguments; none gives them back to Fenix.
    LanguageServer(Option<(String, Vec<String>)>),
    RestartLanguageServer,
    Doctor(Vec<Check>),
    /// What it offers in the task picker: names, and arguments.
    Tasks(Vec<(String, Vec<String>)>),
    NewProject(String),
}

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[derive(Debug, Default)]
pub struct State {
    pub tools: Tools,
    /// `arduino-cli`'s configuration file, which the language server is
    /// told of.
    pub config: Option<String>,
    /// The project (`/project`), and the sketch you're in under it.
    pub top: PathBuf,
    pub sketch: Option<PathBuf>,
    /// Runs going, and what each wrote.
    pub jobs: HashMap<u64, (Job, Vec<String>, Vec<String>)>,
    pub picks: HashMap<u64, PickFor>,
    /// Tasks it ran, and what follows each.
    pub tasks: HashMap<u64, After>,
    /// The serial monitor's terminal, and its port.
    pub monitor: Option<(u64, String)>,
    /// The library index, read once: it's ~10,000 entries.
    pub libraries: Option<Vec<Library>>,
    /// The doctor's deeper findings: the installed cores, the ports.
    pub cores: Option<Vec<Package>>,
    pub plugged: Option<Vec<Port>>,
}

impl State {
    pub fn new(top: impl Into<PathBuf>, tools: Tools) -> State {
        State { top: top.into(), tools, ..State::default() }
    }

    /// At activation: where `arduino-cli` keeps its configuration.
    pub fn activate(&mut self) -> Vec<Effect> {
        self.cli().map(|_| vec![Effect::Spawn { job: Job::Config, args: args(&["config", "get", "directories.data"]) }]).unwrap_or_default()
    }

    fn cli(&self) -> Option<&str> {
        self.tools.cli.as_deref()
    }

    fn board(&self) -> (String, bool) {
        self.sketch.as_deref().map(model::board_of).unwrap_or((model::DEFAULT_BOARD.to_string(), false))
    }

    /// The sketch, or why there isn't one to act on.
    fn need(&self) -> Result<&Path, Effect> {
        if self.cli().is_none() {
            return Err(Effect::Error("arduino-cli not found -- install Arduino CLI (https://arduino.github.io/arduino-cli/), or set arduino.cli".into()));
        }
        self.sketch.as_deref().ok_or_else(|| Effect::Error("not in an Arduino sketch -- open a sketch's .ino file, or SPC f n to make one".into()))
    }

    /// You moved to `file` (under `/project`). None -- a page, the doctor,
    /// a terminal -- isn't leaving the sketch: what it said stays.
    pub fn focus(&mut self, file: Option<&Path>) -> Vec<Effect> {
        let Some(file) = file else { return Vec::new() };
        let sketch = model::sketch_of(file, &self.top);
        if sketch == self.sketch {
            return Vec::new();
        }
        self.sketch = sketch;
        self.cores = None;
        self.plugged = None;
        let mut out = self.refresh();
        if self.sketch.is_some() && self.cli().is_some() {
            out.push(Effect::Spawn { job: Job::Cores, args: args(&["core", "list", "--format", "json"]) });
            out.push(Effect::Spawn { job: Job::Plugged, args: args(&["board", "list", "--format", "json"]) });
        }
        out
    }

    /// What it says about the sketch: the modeline, its tasks, its
    /// language server, its doctor.
    fn refresh(&self) -> Vec<Effect> {
        let Some(root) = self.sketch.as_deref() else {
            return vec![Effect::Status(None), Effect::Tasks(Vec::new()), Effect::LanguageServer(None), Effect::Doctor(Vec::new())];
        };
        let (board, chosen) = self.board();
        let short = if chosen { model::short_board(&board).to_string() } else { format!("{} (default)", model::short_board(&board)) };
        let port = model::port_of(root);
        let mut tasks = vec![("Arduino: build".to_string(), args(&["compile", "--no-color", "-b", &board, "."]))];
        if let Some(port) = &port {
            tasks.push(("Arduino: upload".to_string(), args(&["compile", "--no-color", "--upload", "-p", port, "-b", &board, "."])));
        }
        vec![
            Effect::Status(Some(format!("{short} · {}", port.as_deref().unwrap_or("no port")))),
            Effect::Tasks(if self.cli().is_some() { tasks } else { Vec::new() }),
            Effect::LanguageServer(self.language_server()),
            Effect::Doctor(self.doctor()),
        ]
    }

    /// arduino-language-server, told where the CLI, its configuration and
    /// clangd are, and the board: it runs clangd against the sketch as the
    /// CLI would build it.
    pub fn language_server(&self) -> Option<(String, Vec<String>)> {
        let (cli, clangd, server, config) = (self.cli()?, self.tools.clangd.as_deref()?, self.tools.language_server.as_deref()?, self.config.as_deref()?);
        let (board, _) = self.board();
        Some((server.to_string(), args(&["-cli", cli, "-cli-config", config, "-clangd", clangd, "-fqbn", &board])))
    }

    /// The doctor's lines for the sketch.
    pub fn doctor(&self) -> Vec<Check> {
        let Some(root) = self.sketch.as_deref() else { return Vec::new() };
        let mut out = vec![match self.cli() {
            Some(path) => check(Section::Toolchain, "arduino-cli", Health::Ok, path),
            None => check(Section::Toolchain, "arduino-cli", Health::Bad, "not found -- install Arduino CLI from https://arduino.github.io/arduino-cli"),
        }];
        for (name, found) in [("arduino-language-server", &self.tools.language_server), ("clangd", &self.tools.clangd)] {
            out.push(match found {
                Some(path) => check(Section::Toolchain, name, Health::Ok, path.clone()),
                None => check(Section::Toolchain, name, Health::Warn, "not found -- no completion; use the one in an Arduino IDE 2 install, or set its path in the settings"),
            });
        }
        let (board, chosen) = self.board();
        out.push(if chosen { check(Section::Hardware, "board", Health::Ok, board.clone()) } else { check(Section::Hardware, "board", Health::Warn, "none chosen -- builds need one").fix("pick a board", "arduino.board") });
        if let Some(cores) = &self.cores {
            let core = model::package_of(&board);
            out.push(match cores.iter().find(|c| c.id == core).and_then(|c| c.installed.clone()) {
                Some(version) => check(Section::Toolchain, &format!("core {core}"), Health::Ok, version),
                None => check(Section::Toolchain, &format!("core {core}"), Health::Bad, "not installed -- builds will fail").fix(&format!("install {core}"), "arduino.install_core"),
            });
        }
        out.push(match (model::port_of(root), &self.plugged) {
            (None, _) => check(Section::Hardware, "port", Health::Warn, "none chosen -- uploads need one").fix("pick a port", "arduino.port"),
            (Some(port), Some(plugged)) if plugged.iter().any(|p| p.address == port) => check(Section::Hardware, &format!("board on {port}"), Health::Ok, "connected"),
            (Some(port), Some(_)) => check(Section::Hardware, &format!("board on {port}"), Health::Warn, "not connected").fix("pick a port", "arduino.port"),
            (Some(port), None) => check(Section::Hardware, "port", Health::Ok, port),
        });
        out
    }

    // ---------- commands ----------

    pub fn command(&mut self, id: &str) -> Vec<Effect> {
        if id == "arduino.new_sketch" {
            return vec![Effect::NewProject("arduino-sketch".into())];
        }
        let root = match self.need() {
            Ok(root) => root.to_path_buf(),
            Err(why) => return vec![why],
        };
        let (board, chosen) = self.board();
        match id {
            "arduino.build" => {
                let mut out = vec![Effect::Task { name: "Arduino: build".into(), args: args(&["compile", "--no-color", "-b", &board, "."]), after: None }];
                if !chosen {
                    out.push(Effect::Notify(format!("building for {board}, the default -- SPC m s picks your board")));
                }
                out
            }
            "arduino.upload" => self.with_port(Action::Upload),
            "arduino.monitor" => self.with_port(Action::Monitor),
            "arduino.debug" => self.debug(model::port_of(&root)),
            "arduino.port" => vec![Effect::Notify("looking for boards…".into()), Effect::Spawn { job: Job::Ports { then: None }, args: args(&["board", "list", "--format", "json"]) }],
            "arduino.board" => vec![Effect::Spawn { job: Job::Boards, args: args(&["board", "listall", "--format", "json"]) }],
            "arduino.board_options" => vec![Effect::Spawn { job: Job::Options { board: board.clone() }, args: args(&["board", "details", "-b", &board, "--format", "json"]) }],
            "arduino.baud" => {
                let current = model::baud_rate(&root);
                let items = BAUD_RATES.iter().map(|&b| (if b == current { format!("{b} baud  (current)") } else { format!("{b} baud") }, format!("baud:{b}"))).collect();
                vec![Effect::Pick { title: "Serial speed".into(), items, pick: PickFor::default() }]
            }
            "arduino.library" => match &self.libraries {
                Some(libraries) => vec![library_picker(libraries)],
                None => vec![Effect::Notify("loading the library index…".into()), Effect::Spawn { job: Job::LibrarySearch, args: args(&["lib", "search", "--omit-releases-details", "--format", "json"]) }],
            },
            "arduino.package" => vec![Effect::Notify("loading the board package index…".into()), Effect::Spawn { job: Job::Packages, args: args(&["core", "search", "--format", "json"]) }],
            "arduino.install_core" => {
                let core = model::package_of(&board);
                vec![Effect::Task { name: format!("install {core}"), args: args(&["core", "install", "--no-color", &core]), after: Some(After::RestartLanguageServer) }]
            }
            "arduino.info" => {
                let board = if chosen { board } else { format!("{board} (default -- SPC m s to choose)") };
                let port = model::port_of(&root).unwrap_or_else(|| "none yet -- SPC m p".into());
                let completion = if self.language_server().is_some() { "completion ready" } else { "no completion: arduino-language-server or clangd is missing" };
                vec![Effect::Notify(format!("Arduino sketch · board {board} · port {port} · {} baud · {completion}", model::baud_rate(&root)))]
            }
            other => vec![Effect::Error(format!("no command {other}"))],
        }
    }

    /// `action` on the sketch's port, finding one first if none is set: a
    /// single port with a board recognized on it is used, and saved;
    /// otherwise you pick.
    fn with_port(&mut self, action: Action) -> Vec<Effect> {
        let Some(root) = self.sketch.clone() else { return Vec::new() };
        match model::port_of(&root) {
            Some(port) => self.act(action, &port),
            None => vec![Effect::Notify("looking for your board…".into()), Effect::Spawn { job: Job::Ports { then: Some(action) }, args: args(&["board", "list", "--format", "json"]) }],
        }
    }

    fn act(&mut self, action: Action, port: &str) -> Vec<Effect> {
        let Some(root) = self.sketch.clone() else { return Vec::new() };
        let (board, _) = self.board();
        match action {
            Action::Upload => {
                // One program holds a serial port at a time: the monitor
                // steps aside for the upload, and comes back after.
                let mut out = Vec::new();
                let reopen = self.monitor.is_some();
                if let Some((terminal, _)) = &self.monitor {
                    out.push(Effect::TerminalStop(*terminal));
                }
                out.push(Effect::Task { name: format!("Arduino: upload to {port}"), args: args(&["compile", "--no-color", "--upload", "-p", port, "-b", &board, "."]), after: Some(After::Upload { reopen }) });
                out
            }
            Action::Monitor => {
                let baud = model::baud_rate(&root);
                let again = self.monitor.as_ref().map(|(t, _)| *t);
                self.monitor = self.monitor.take().map(|(t, _)| (t, port.to_string()));
                vec![Effect::Monitor { args: args(&["monitor", "-p", port, "-b", &board, "--config", &format!("baudrate={baud}")]), title: format!("Serial monitor  --  {port} at {baud} baud  --  Ctrl-\\ to leave, SPC m B for speed"), again }]
            }
            Action::Debug => self.debug(Some(port.to_string())),
        }
    }

    fn debug(&self, port: Option<String>) -> Vec<Effect> {
        let (board, _) = self.board();
        let mut words = args(&["debug", "--info", "-b", &board, "--format", "json"]);
        if let Some(port) = &port {
            words.extend(args(&["-p", port]));
        }
        words.push(".".into());
        vec![Effect::Notify("checking whether this board can be debugged…".into()), Effect::Spawn { job: Job::Debug { port }, args: words }]
    }

    /// The monitor's terminal opened, as `terminal`.
    pub fn monitor_opened(&mut self, terminal: u64, port: &str) {
        self.monitor = Some((terminal, port.to_string()));
    }

    pub fn terminal_closed(&mut self, terminal: u64) {
        if self.monitor.as_ref().is_some_and(|(t, _)| *t == terminal) {
            self.monitor = None;
        }
    }

    /// A task it ran started, as `task`.
    pub fn task_started(&mut self, task: u64, after: After) {
        self.tasks.insert(task, after);
    }

    pub fn task_finished(&mut self, task: u64, ok: bool) -> Vec<Effect> {
        match self.tasks.remove(&task) {
            Some(After::Upload { reopen }) => {
                let mut out = Vec::new();
                if reopen {
                    if let Some(port) = self.monitor.as_ref().map(|(_, p)| p.clone()) {
                        out.extend(self.act(Action::Monitor, &port));
                    }
                }
                out.push(if ok { Effect::Notify("uploaded".into()) } else { Effect::Error("upload failed -- is the board plugged in, and is it the right port? (SPC m p)".into()) });
                out
            }
            Some(After::Debugger { args }) if ok => vec![Effect::Console { args, title: "debugger  --  Ctrl-\\ to leave".into() }],
            Some(After::Debugger { .. }) => vec![Effect::Error("the debug build didn't flash, so there's nothing to attach to -- see the task output".into())],
            Some(After::RestartLanguageServer) => {
                self.libraries = None;
                if ok {
                    vec![Effect::RestartLanguageServer]
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        }
    }

    // ---------- runs ----------

    pub fn started(&mut self, process: u64, job: Job) {
        self.jobs.insert(process, (job, Vec::new(), Vec::new()));
    }

    pub fn output(&mut self, process: u64, stderr: bool, line: &str) {
        if let Some((_, out, err)) = self.jobs.get_mut(&process) {
            (if stderr { err } else { out }).push(line.to_string());
        }
    }

    pub fn exited(&mut self, process: u64, code: Option<i32>) -> Vec<Effect> {
        let Some((job, out, err)) = self.jobs.remove(&process) else { return Vec::new() };
        let text = out.join("\n");
        let why = || err.iter().chain(&out).rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("it said nothing").to_string();
        let ok = code == Some(0);
        match job {
            Job::Config if ok => {
                let dir = text.trim();
                if dir.is_empty() {
                    return Vec::new();
                }
                let sep = if dir.contains('\\') { '\\' } else { '/' };
                self.config = Some(format!("{}{sep}arduino-cli.yaml", dir.trim_end_matches(['/', '\\'])));
                self.refresh()
            }
            Job::Ports { then } if ok => match model::ports(&text) {
                Ok(ports) => self.ports(ports, then),
                Err(e) => vec![Effect::Error(format!("couldn't list ports: {e}"))],
            },
            Job::Boards if ok => match model::boards(&text) {
                Ok(boards) => vec![self.board_picker(&boards)],
                Err(e) => vec![Effect::Error(format!("couldn't list boards: {e}"))],
            },
            Job::Options { board } if ok => match model::board_options(&text) {
                Ok(options) if options.is_empty() => vec![Effect::Notify(format!("{board} has no options to choose"))],
                Ok(options) => vec![options_picker(&options, board)],
                Err(e) => vec![Effect::Error(format!("couldn't read the board's options: {e}"))],
            },
            Job::LibrarySearch if ok => vec![Effect::Spawn { job: Job::LibraryList { search: text }, args: args(&["lib", "list", "--format", "json"]) }],
            Job::LibraryList { search } if ok => match model::libraries(&search, &text) {
                Ok(libraries) => {
                    let pick = library_picker(&libraries);
                    self.libraries = Some(libraries);
                    vec![pick]
                }
                Err(e) => vec![Effect::Error(format!("couldn't load the library index: {e}"))],
            },
            Job::Packages if ok => match model::packages(&text) {
                Ok(packages) => vec![packages_picker(&packages)],
                Err(e) => vec![Effect::Error(format!("couldn't load the board package index: {e}"))],
            },
            Job::Debug { port: Some(port) } if ok => {
                let (board, _) = self.board();
                let prepare = args(&["compile", "--no-color", "--optimize-for-debug", "--upload", "-p", &port, "-b", &board, "."]);
                let session = args(&["debug", "-p", &port, "-b", &board, "."]);
                vec![Effect::Task { name: "debug build".into(), args: prepare, after: Some(After::Debugger { args: session }) }]
            }
            // It can be debugged, but there's no port yet.
            Job::Debug { port: None } if ok => self.with_port(Action::Debug),
            Job::Debug { .. } => {
                let reason = why();
                let (board, _) = self.board();
                if reason.contains("not supported") {
                    vec![Effect::Error(format!("{board} can't be debugged with breakpoints ({reason}). Print with Serial.println and watch the serial monitor instead."))]
                } else {
                    vec![Effect::Error(format!("couldn't start debugging: {reason}"))]
                }
            }
            Job::Cores if ok => {
                self.cores = model::packages(&text).ok();
                vec![Effect::Doctor(self.doctor())]
            }
            Job::Plugged if ok => {
                self.plugged = model::ports(&text).ok();
                vec![Effect::Doctor(self.doctor())]
            }
            Job::Cores | Job::Plugged | Job::Config => Vec::new(),
            Job::Ports { .. } | Job::Boards | Job::Options { .. } | Job::LibrarySearch | Job::LibraryList { .. } | Job::Packages => vec![Effect::Error(format!("arduino-cli: {}", why()))],
        }
    }

    fn ports(&mut self, mut ports: Vec<Port>, then: Option<Action>) -> Vec<Effect> {
        if ports.is_empty() {
            return vec![Effect::Error("no serial ports found -- plug the board in (and check its USB cable carries data)".into())];
        }
        // Ports with a board recognized on them first.
        ports.sort_by_key(|p| p.boards.is_empty());
        let recognized: Vec<&Port> = ports.iter().filter(|p| !p.boards.is_empty()).collect();
        if let (Some(action), [only]) = (then, recognized.as_slice()) {
            let (port, board) = (only.address.clone(), only.boards[0].name.clone());
            if let Some(Err(e)) = self.sketch.as_deref().map(|root| model::set_yaml_value(root, "default_port", &port)) {
                return vec![Effect::Error(e)];
            }
            let mut out = vec![Effect::Notify(format!("using {port} ({board})"))];
            out.extend(self.refresh());
            out.extend(self.act(action, &port));
            return out;
        }
        let current = self.sketch.as_deref().and_then(model::port_of);
        let items = ports
            .into_iter()
            .map(|p| {
                let board = p.boards.first().map(|b| format!("  {}", b.name)).unwrap_or_default();
                let mark = if current.as_deref() == Some(p.address.as_str()) { "  (current)" } else { "" };
                (format!("{}{board}  ({}){mark}", p.address, p.protocol), format!("port:{}", p.address))
            })
            .collect();
        vec![Effect::Pick { title: "Port".into(), items, pick: PickFor { then, board: None } }]
    }

    fn board_picker(&self, boards: &[Board]) -> Effect {
        let (current, _) = self.board();
        let mut items: Vec<(String, String)> = boards
            .iter()
            .map(|b| {
                let mark = if current.split(':').take(3).eq(b.id.split(':')) { "  (current)" } else { "" };
                (format!("{}  {}{mark}", b.name, b.id), format!("board:{}", b.id))
            })
            .collect();
        items.push(("Install more boards…".into(), "more".into()));
        Effect::Pick { title: "Board".into(), items, pick: PickFor::default() }
    }

    /// A picker it opened, as `id`.
    pub fn pick_opened(&mut self, id: u64, pick: PickFor) {
        self.picks.insert(id, pick);
    }

    /// What was picked in picker `id`.
    pub fn picked(&mut self, id: u64, value: Option<&str>) -> Vec<Effect> {
        let Some(pick) = self.picks.remove(&id) else { return Vec::new() };
        let (Some(value), Some(root)) = (value, self.sketch.clone()) else { return Vec::new() };
        let saved = |result: Result<(), String>, then: Vec<Effect>, state: &State| match result {
            Ok(()) => {
                let mut out = then;
                out.extend(state.refresh());
                out
            }
            Err(e) => vec![Effect::Error(e)],
        };
        let (kind, rest) = value.split_once(':').unwrap_or((value, ""));
        match kind {
            "board" => saved(model::set_yaml_value(&root, "default_fqbn", rest), vec![Effect::Notify(format!("board: {rest} -- SPC m o for its options")), Effect::RestartLanguageServer], self),
            "more" => self.command("arduino.package"),
            "opt" => {
                let Some((option, value)) = rest.split_once('=') else { return Vec::new() };
                let board = model::with_option(&pick.board.unwrap_or_else(|| self.board().0), option, value);
                saved(model::set_yaml_value(&root, "default_fqbn", &board), vec![Effect::Notify(format!("board: {board}")), Effect::RestartLanguageServer], self)
            }
            "port" => match model::set_yaml_value(&root, "default_port", rest) {
                Ok(()) => {
                    let mut out = self.refresh();
                    match pick.then {
                        Some(action) => out.extend(self.act(action, rest)),
                        None => out.push(Effect::Notify(format!("port: {rest}"))),
                    }
                    out
                }
                Err(e) => vec![Effect::Error(e)],
            },
            "baud" => {
                let Ok(baud) = rest.parse::<u32>() else { return Vec::new() };
                match model::set_baud_rate(&root, baud) {
                    Ok(()) => {
                        let mut out = vec![Effect::Notify(format!("serial monitor speed: {baud} baud -- match your Serial.begin({baud})"))];
                        if let Some(port) = self.monitor.as_ref().map(|(_, p)| p.clone()) {
                            out.extend(self.act(Action::Monitor, &port));
                        }
                        out
                    }
                    Err(e) => vec![Effect::Error(e)],
                }
            }
            "lib" => vec![Effect::Task { name: format!("install library {rest}"), args: args(&["lib", "install", "--no-color", rest]), after: Some(After::RestartLanguageServer) }],
            "pkg" => vec![Effect::Task { name: format!("install {rest}"), args: args(&["core", "install", "--no-color", rest]), after: Some(After::RestartLanguageServer) }],
            _ => Vec::new(),
        }
    }
}

fn library_picker(libraries: &[Library]) -> Effect {
    let items = libraries
        .iter()
        .map(|lib| {
            let installed = match &lib.installed {
                Some(v) if *v == lib.latest => "  [installed]".to_string(),
                Some(v) => format!("  [installed {v}, update available]"),
                None => String::new(),
            };
            (format!("{}  {}{installed}  --  {}", lib.name, lib.latest, lib.summary), format!("lib:{}", lib.name))
        })
        .collect();
    Effect::Pick { title: "Install library".into(), items, pick: PickFor::default() }
}

fn options_picker(options: &[BoardOption], board: String) -> Effect {
    let items = options
        .iter()
        .flat_map(|option| {
            option.values.iter().map(move |value| {
                let mark = if value.selected { "  (current)" } else { "" };
                (format!("{}: {}{mark}", option.label, value.label), format!("opt:{}={}", option.id, value.value))
            })
        })
        .collect();
    Effect::Pick { title: "Board options".into(), items, pick: PickFor { then: None, board: Some(board) } }
}

fn packages_picker(packages: &[Package]) -> Effect {
    let items = packages
        .iter()
        .map(|p| {
            let installed = match &p.installed {
                Some(v) if *v == p.latest => "  [installed]".to_string(),
                Some(v) => format!("  [installed {v}, update available]"),
                None => String::new(),
            };
            (format!("{}  {}  {}{installed}", p.name, p.id, p.latest), format!("pkg:{}", p.id))
        })
        .collect();
    Effect::Pick { title: "Install board package".into(), items, pick: PickFor::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Tools {
        Tools { cli: Some("C:\\cli\\arduino-cli.exe".into()), clangd: Some("C:\\ide\\clangd.exe".into()), language_server: Some("C:\\ide\\arduino-language-server.exe".into()) }
    }

    /// A project holding sketch Blink, and the state with you in it.
    fn in_sketch() -> (tempfile::TempDir, State, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let blink = dir.path().join("Blink");
        std::fs::create_dir_all(&blink).unwrap();
        std::fs::write(blink.join("Blink.ino"), "void setup() {}\nvoid loop() {}\n").unwrap();
        let mut st = State::new(dir.path(), tools());
        let started = st.activate();
        assert_eq!(started, [Effect::Spawn { job: Job::Config, args: args(&["config", "get", "directories.data"]) }]);
        st.started(1, Job::Config);
        st.output(1, false, "C:\\Users\\me\\AppData\\Local\\Arduino15");
        st.exited(1, Some(0));
        assert_eq!(st.config.as_deref(), Some("C:\\Users\\me\\AppData\\Local\\Arduino15\\arduino-cli.yaml"));
        (dir, st, blink)
    }

    #[test]
    fn going_into_a_sketch_says_its_board_tasks_language_server_and_doctor() {
        let (_dir, mut st, blink) = in_sketch();
        let out = st.focus(Some(&blink.join("Blink.ino")));
        assert!(out.contains(&Effect::Status(Some("uno (default) · no port".into()))), "{out:?}");
        assert!(out.iter().any(|e| matches!(e, Effect::Tasks(t) if t.len() == 1 && t[0].1 == args(&["compile", "--no-color", "-b", "arduino:avr:uno", "."]))), "{out:?}");
        let Some(Effect::LanguageServer(Some((program, server_args)))) = out.iter().find(|e| matches!(e, Effect::LanguageServer(_))) else { panic!("{out:?}") };
        assert_eq!(program, "C:\\ide\\arduino-language-server.exe");
        assert_eq!(server_args[..2], args(&["-cli", "C:\\cli\\arduino-cli.exe"]));
        assert_eq!(server_args[6..], args(&["-fqbn", "arduino:avr:uno"]));
        let Some(Effect::Doctor(checks)) = out.iter().find(|e| matches!(e, Effect::Doctor(_))) else { panic!() };
        assert!(checks.iter().any(|c| c.label == "board" && c.health == Health::Warn && c.fix.as_ref().is_some_and(|f| f.1 == "arduino.board")));
        assert!(out.iter().any(|e| matches!(e, Effect::Spawn { job: Job::Cores, .. })));
        // Moving within the sketch changes nothing, nor does a page; a file
        // elsewhere in the project takes it all back.
        assert!(st.focus(Some(&blink.join("Blink.ino"))).is_empty());
        assert!(st.focus(None).is_empty());
        let out = st.focus(Some(&blink.parent().unwrap().join("notes.txt")));
        assert!(out.contains(&Effect::Status(None)) && out.contains(&Effect::LanguageServer(None)));
    }

    #[test]
    fn the_doctor_learns_the_core_and_whether_the_board_is_plugged_in() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        model::set_yaml_value(&blink, "default_port", "COM4").unwrap();
        st.started(2, Job::Cores);
        st.output(2, false, r#"{"platforms": [{"id": "arduino:megaavr", "installed_version": "1.8.9"}]}"#);
        let out = st.exited(2, Some(0));
        let Some(Effect::Doctor(checks)) = out.first() else { panic!("{out:?}") };
        assert!(checks.iter().any(|c| c.label == "core arduino:avr" && c.health == Health::Bad && c.fix.as_ref().is_some_and(|f| f.1 == "arduino.install_core")));
        st.started(3, Job::Plugged);
        st.output(3, false, r#"{"detected_ports": [{"port": {"address": "COM4"}}]}"#);
        let Some(Effect::Doctor(checks)) = st.exited(3, Some(0)).first().cloned() else { panic!() };
        assert!(checks.iter().any(|c| c.label == "board on COM4" && c.health == Health::Ok));
        assert_eq!(st.command("arduino.install_core")[0], Effect::Task { name: "install arduino:avr".into(), args: args(&["core", "install", "--no-color", "arduino:avr"]), after: Some(After::RestartLanguageServer) });
    }

    #[test]
    fn commands_need_a_sketch_and_arduino_cli() {
        let (_dir, mut st, _blink) = in_sketch();
        assert!(matches!(&st.command("arduino.build")[0], Effect::Error(e) if e.starts_with("not in an Arduino sketch")));
        assert_eq!(st.command("arduino.new_sketch"), [Effect::NewProject("arduino-sketch".into())]);
        let mut none = State::new("/project", Tools::default());
        assert!(matches!(&none.command("arduino.build")[0], Effect::Error(e) if e.starts_with("arduino-cli not found")));
        assert!(none.activate().is_empty());
    }

    #[test]
    fn building_runs_a_task_and_says_when_its_the_default_board() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        let out = st.command("arduino.build");
        assert_eq!(out[0], Effect::Task { name: "Arduino: build".into(), args: args(&["compile", "--no-color", "-b", "arduino:avr:uno", "."]), after: None });
        assert!(matches!(&out[1], Effect::Notify(m) if m.contains("the default")));
    }

    /// No port yet: the one board found is used and saved; the monitor
    /// steps aside for an upload and comes back after.
    #[test]
    fn upload_finds_the_port_and_the_monitor_steps_aside() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        let out = st.command("arduino.monitor");
        assert!(matches!(&out[1], Effect::Spawn { job: Job::Ports { then: Some(Action::Monitor) }, .. }), "{out:?}");
        st.started(5, Job::Ports { then: Some(Action::Monitor) });
        st.output(5, false, r#"{"detected_ports": [{"port": {"address": "COM1"}}, {"matching_boards": [{"name": "Arduino UNO", "fqbn": "arduino:avr:uno"}], "port": {"address": "COM4", "protocol_label": "Serial Port (USB)"}}]}"#);
        let out = st.exited(5, Some(0));
        assert_eq!(model::port_of(&blink).as_deref(), Some("COM4"), "saved");
        let Some(Effect::Monitor { args: monitor_args, again: None, .. }) = out.iter().find(|e| matches!(e, Effect::Monitor { .. })) else { panic!("{out:?}") };
        assert_eq!(monitor_args, &args(&["monitor", "-p", "COM4", "-b", "arduino:avr:uno", "--config", "baudrate=9600"]));
        st.monitor_opened(40, "COM4");
        let out = st.command("arduino.upload");
        assert_eq!(out[0], Effect::TerminalStop(40));
        assert!(matches!(&out[1], Effect::Task { args, after: Some(After::Upload { reopen: true }), .. } if args[2] == "--upload"));
        st.task_started(7, After::Upload { reopen: true });
        let out = st.task_finished(7, true);
        assert!(matches!(&out[0], Effect::Monitor { again: Some(40), .. }), "{out:?}");
        assert_eq!(out[1], Effect::Notify("uploaded".into()));
        st.terminal_closed(40);
        assert!(st.monitor.is_none());
    }

    #[test]
    fn picks_save_to_the_sketch_and_follow_on() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        st.pick_opened(1, PickFor::default());
        let out = st.picked(1, Some("board:arduino:avr:nano"));
        assert_eq!(model::board_of(&blink), ("arduino:avr:nano".into(), true));
        assert!(out.contains(&Effect::RestartLanguageServer) && out.contains(&Effect::Status(Some("nano · no port".into()))), "{out:?}");
        st.pick_opened(2, PickFor { then: None, board: Some("arduino:avr:nano".into()) });
        st.picked(2, Some("opt:cpu=atmega328old"));
        assert_eq!(model::board_of(&blink).0, "arduino:avr:nano:cpu=atmega328old");
        st.pick_opened(3, PickFor::default());
        st.picked(3, Some("baud:115200"));
        assert_eq!(model::baud_rate(&blink), 115200);
        st.pick_opened(4, PickFor::default());
        let out = st.picked(4, Some("lib:Servo"));
        assert!(matches!(&out[0], Effect::Task { args: a, after: Some(After::RestartLanguageServer), .. } if a == &args(&["lib", "install", "--no-color", "Servo"])));
        assert!(st.picked(99, Some("board:x")).is_empty(), "a picker it didn't open");
    }

    #[test]
    fn debugging_checks_the_board_then_flashes_then_opens_the_console() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        model::set_yaml_value(&blink, "default_port", "COM4").unwrap();
        let out = st.command("arduino.debug");
        assert!(matches!(&out[1], Effect::Spawn { job: Job::Debug { port: Some(p) }, args } if p == "COM4" && args.last().map(String::as_str) == Some(".")));
        st.started(8, Job::Debug { port: Some("COM4".into()) });
        let out = st.exited(8, Some(0));
        let Some(Effect::Task { after: Some(After::Debugger { args: session }), .. }) = out.first() else { panic!("{out:?}") };
        assert_eq!(session[..2], args(&["debug", "-p"]));
        st.task_started(9, After::Debugger { args: session.clone() });
        assert!(matches!(&st.task_finished(9, true)[0], Effect::Console { .. }));
        st.started(10, Job::Debug { port: Some("COM4".into()) });
        st.output(10, true, "Error getting Debug info: debugging not supported for board arduino:avr:uno");
        assert!(matches!(&st.exited(10, Some(1))[0], Effect::Error(e) if e.contains("can't be debugged with breakpoints")));
    }

    #[test]
    fn the_library_index_is_read_once() {
        let (_dir, mut st, blink) = in_sketch();
        st.focus(Some(&blink));
        assert!(matches!(&st.command("arduino.library")[1], Effect::Spawn { job: Job::LibrarySearch, .. }));
        st.started(11, Job::LibrarySearch);
        st.output(11, false, r#"{"libraries": [{"name": "Servo", "latest": {"version": "1.3.0"}}]}"#);
        let out = st.exited(11, Some(0));
        let Some(Effect::Spawn { job: job @ Job::LibraryList { .. }, .. }) = out.first().cloned() else { panic!("{out:?}") };
        st.started(12, job);
        st.output(12, false, r#"{"installed_libraries": []}"#);
        assert!(matches!(&st.exited(12, Some(0))[0], Effect::Pick { items, .. } if items[0].1 == "lib:Servo"));
        assert!(matches!(&st.command("arduino.library")[0], Effect::Pick { .. }), "from the cache");
    }
}
