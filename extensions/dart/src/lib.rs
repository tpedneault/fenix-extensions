//! Dart and Flutter in Fenix, on lang-kit: the analysis server
//! (`dart language-server`) as the language server, `dart format` as the
//! formatter, the SDK's debug adapters (`dart debug_adapter`, `flutter
//! debug_adapter`), tasks for Dart and for Flutter, the project doctor's
//! checks, and the package manager page for pub. Fenix has no Dart of
//! its own: the language file brings tree-sitter-dart's grammar and the
//! pack's queries for it.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]); the rest is lang-kit's [`Kit`].
//! The problems you see come from the analysis server, so there's no
//! linter beside it. Flutter's devices ([`devices`]) are listed with
//! `flutter devices --machine`: for a picker to run on, and a launch
//! configuration for each. Hot reload and hot restart are signals sent to
//! the `flutter run` this pack started, through the pid file it asked
//! for.

pub mod devices;
pub mod doctor;
pub mod launch;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit, PackagesPage};

use devices::Device;
use project::Project;
use tools::Tools;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language: the language file's.
const DART: &str = "dart";

/// The pack's own commands, in the manifest.
pub const DEVICES: &str = "dart.devices";
pub const RUN_ON_DEVICE: &str = "dart.run-on-device";
pub const RUN_IN_TERMINAL: &str = "dart.run-in-terminal";
pub const STOP: &str = "dart.stop";
pub const HOT_RELOAD: &str = "dart.hot-reload";
pub const HOT_RESTART: &str = "dart.hot-restart";

/// What a signal to `flutter run` does: SIGUSR1 reloads, SIGUSR2 restarts.
const RELOAD: (&str, &str) = ("USR1", "Hot reload");
const RESTART: (&str, &str) = ("USR2", "Hot restart");

/// Where `flutter run` is started from the device picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunIn {
    /// A task beside the task pane, which hot reload can reach.
    Task,
    /// A terminal, where `r` and `R` are typed.
    Terminal,
}

/// What a `flutter devices` is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// `dart.devices`: say what's there.
    Say,
    /// The start, or a change: launches for each, said to no one.
    Quiet,
    /// To run on one.
    Run(RunIn),
}

/// A program Fenix runs in a terminal, by its command and title (the
/// API's `Command::run_in_terminal`, which isn't a host call lang-kit's
/// `Host` has).
pub type Terminal = Box<dyn FnMut(&Command, &str) -> Result<u64, String>>;

pub struct Dart {
    pub kit: Kit,
    root: String,
    os: String,
    project: Project,
    tools: Tools,
    terminal: Terminal,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_adapter: Option<Command>,
    set_formatter: Option<Command>,
    set_launches: Vec<api::LaunchConfig>,
    devices: Vec<Device>,
    listing: Runs<Listing>,
    signals: Runs<&'static str>,
    /// The picker `flutter run`'s device is picked in.
    picking: Option<(u64, RunIn)>,
    /// The `flutter run` this pack started as a task.
    running: Option<u64>,
}

impl Default for Dart {
    fn default() -> Dart {
        let terminal: Terminal = Box::new(|cmd, title| cmd.run_in_terminal(title));
        Dart::new(Box::new(Fenix), PROJECT, terminal, &api::platform().os)
    }
}

impl Dart {
    /// The pack for the project in `root`, talking to `host`, on `os`.
    pub fn new(mut host: Box<dyn Host>, root: &str, terminal: Terminal, os: &str) -> Dart {
        let tools = Tools::find(host.as_mut());
        let project = Project::read(root);
        let kit = Kit::with_host("dart", host);
        let mut d = Dart { kit, root: root.into(), os: os.into(), project, tools, terminal, set_server: None, set_adapter: None, set_formatter: None, set_launches: Vec::new(), devices: Vec::new(), listing: Runs::new(), signals: Runs::new(), picking: None, running: None };
        d.configure();
        d
    }

    /// The kit's tasks, checks and package manager, for the project and
    /// tools as they are.
    fn configure(&mut self) {
        let (project, tools) = (&self.project, &self.tools);
        self.kit.tasks = tasks::tasks(project, tools, &self.os);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools) {
            self.kit.doctor.program(program);
        }
        let project = project.clone();
        self.kit.doctor.refresh = Some(Box::new(move || doctor::extra(&project)));
        let manager = Box::new(packages::Pub { tool: tools.pub_cmd(self.project.is_flutter()) });
        match self.kit.packages.as_mut() {
            Some(page) => page.manager = manager,
            None if self.project.is_dart() => self.kit.packages = Some(PackagesPage::new(manager)),
            None => {}
        }
    }

    /// The server, the adapter and the formatter set in Fenix, where they
    /// changed. Nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let flutter = self.project.is_flutter();
        let (server, adapter, formatter) = (self.tools.server(), self.tools.adapter(flutter), self.tools.formatter());
        let languages = [DART.to_string()];
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if adapter != self.set_adapter {
            if let Err(why) = host.debug_adapter(&languages, adapter.as_ref()) {
                host.log(&format!("debug adapter: {why}"));
            }
            self.set_adapter = adapter;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    /// The launches worked out from the project and the devices.
    fn set_launches(&mut self) {
        let launches = launch::launches(&self.project, &self.devices);
        if launches != self.set_launches {
            if let Err(e) = self.kit.host.launches(&launches) {
                self.kit.host.log(&format!("launches: {e}"));
            }
            self.set_launches = launches;
        }
    }

    fn apply_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// The programs looked for again: an install by a task or by hand, or
    /// a setting changed (`dart.sdk`, `dart.flutter`).
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            self.apply_tasks();
            self.apply();
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.configure();
        self.apply_tasks();
        self.apply();
        self.set_launches();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    /// `flutter devices --machine`, listed for `why`.
    fn list_devices(&mut self, why: Listing) -> Result<(), String> {
        if self.tools.flutter.is_none() {
            return Err(format!("Flutter isn't installed: {}", doctor::how_to_get_flutter()));
        }
        let cmd = self.tools.flutter_cmd().args(["devices", "--machine"]);
        self.listing.start(self.kit.host.as_mut(), &cmd, why).map(|_| ())
    }

    /// Flutter said what devices there are.
    fn listed(&mut self, why: Listing, out: fenix_lang_kit::Output) {
        let devices = match devices::parse(&out) {
            Ok(d) => d,
            Err(e) => {
                self.kit.host.notify(&format!("Flutter's devices: {e}"));
                return;
            }
        };
        if devices != self.devices {
            self.devices = devices;
            self.set_launches();
        }
        match why {
            Listing::Quiet => {}
            Listing::Say => {
                let names: Vec<String> = self.devices.iter().map(Device::label).collect();
                let said = if names.is_empty() { "Flutter finds no device: start an emulator or plug one in, then flutter doctor says what's missing".to_string() } else { format!("Flutter's devices: {}", names.join("; ")) };
                self.kit.host.notify(&said);
            }
            Listing::Run(how) => match self.devices.as_slice() {
                [] => {
                    self.kit.host.notify("Flutter finds no device: running flutter run without one");
                    self.run(how, None);
                }
                [one] => {
                    let id = one.id.clone();
                    self.run(how, Some(&id));
                }
                many => {
                    let id = self.kit.host.pick("Run on", &devices::items(many));
                    self.picking = Some((id, how));
                }
            },
        }
    }

    /// `flutter run` on `device`, as a task or in a terminal.
    fn run(&mut self, how: RunIn, device: Option<&str>) {
        let cmd = tasks::flutter_run(&self.tools, device);
        let started = match how {
            RunIn::Task => self.kit.host.run_task_background("flutter run", &cmd).map(|id| {
                self.running = Some(id);
                let _ = self.kit.host.task_show(id);
            }),
            RunIn::Terminal => (self.terminal)(&cmd, "flutter run").map(|_| ()),
        };
        if let Err(why) = started {
            self.kit.host.notify(&format!("flutter run: {why}"));
        }
    }

    /// A signal to the `flutter run` that wrote the pid file: SIGUSR1
    /// reloads, SIGUSR2 restarts. Windows has no signals.
    fn signal(&mut self, (signal, what): (&'static str, &'static str)) -> Result<(), String> {
        if self.os == "windows" {
            return Err(format!("{what} is a signal to flutter run, which Windows has none of: run Flutter in a terminal (Dart: run Flutter in a terminal) and press {} there", if signal == "USR1" { "r" } else { "R" }));
        }
        let file = format!("{}/{}", self.root, tasks::PID_FILE);
        let pid = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse::<u32>().ok());
        let Some(pid) = pid else {
            return Err(format!("{what}: no flutter run to send it to -- start one with the run task or Dart: run Flutter on a device ({} isn't there)", tasks::PID_FILE));
        };
        let cmd = Command::new("kill").args(["-s", signal, &pid.to_string()]);
        self.signals.start(self.kit.host.as_mut(), &cmd, signal).map(|_| ())
    }

    /// `kill` ended.
    fn signalled(&mut self, signal: &str, out: fenix_lang_kit::Output) {
        let what = if signal == "USR1" { RELOAD.1 } else { RESTART.1 };
        if out.ok() {
            self.kit.host.notify(&format!("{what} sent to flutter run"));
        } else {
            self.kit.host.notify(&format!("{what}: {} -- is flutter run still going? ({} may be left from an earlier one)", out.why(), tasks::PID_FILE));
        }
    }
}

impl Extension for Dart {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.set_launches();
        if self.project.is_flutter() && self.tools.flutter.is_some() {
            if let Err(why) = self.list_devices(Listing::Quiet) {
                self.kit.host.log(&format!("devices: {why}"));
            }
        }
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => match id {
                doctor::GET_DART => {
                    self.kit.host.notify(&doctor::how_to_get_dart(&self.os));
                    self.kit.host.open_url(doctor::DART_PAGE)
                }
                doctor::GET_FLUTTER => {
                    self.kit.host.notify(&doctor::how_to_get_flutter());
                    self.kit.host.open_url(doctor::FLUTTER_PAGE)
                }
                doctor::PUB_GET => {
                    let cmd = self.tools.pub_cmd(self.project.is_flutter()).args(["pub", "get"]);
                    self.kit.host.run_task("pub get", &cmd).map(|_| ())
                }
                DEVICES => self.list_devices(Listing::Say),
                RUN_ON_DEVICE | RUN_IN_TERMINAL => {
                    let how = if id == RUN_IN_TERMINAL { RunIn::Terminal } else { RunIn::Task };
                    if how == RunIn::Task && self.running.is_some() {
                        Err("flutter run is already going: stop it first (Dart: stop flutter run)".into())
                    } else {
                        self.list_devices(Listing::Run(how))
                    }
                }
                STOP => match self.running.take() {
                    Some(task) => {
                        self.kit.host.task_cancel(task);
                        Ok(())
                    }
                    None => Err("no flutter run of this pack's is going".into()),
                },
                HOT_RELOAD => self.signal(RELOAD),
                HOT_RESTART => self.signal(RESTART),
                _ => Err(format!("no command {id} here")),
            },
        };
        self.find_again();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.listing.owns(&event) {
            if let Some((why, out)) = self.listing.event(&event) {
                self.listed(why, out);
            }
            return;
        }
        if self.signals.owns(&event) {
            if let Some((signal, out)) = self.signals.event(&event) {
                self.signalled(signal, out);
            }
            return;
        }
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string()),
            _ => None,
        };
        let saved = name.as_deref() == Some("pubspec.yaml");
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("dart."));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        match &event {
            Event::Picked((id, value)) if self.picking.is_some_and(|(p, _)| p == *id) => {
                let (_, how) = self.picking.take().unwrap_or((0, RunIn::Task));
                if let Some(device) = value {
                    self.run(how, Some(device));
                }
            }
            Event::TaskFinished((id, ok)) if self.running == Some(*id) => {
                self.running = None;
                self.kit.host.notify(if *ok { "flutter run ended" } else { "flutter run ended with an error: see its output, in the buffer *flutter run*" });
            }
            _ => {}
        }
        if saved || (task_ended && Project::read(&self.root) != self.project) {
            self.reload();
        }
        if setting || task_ended {
            self.find_again();
        }
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

export!(Dart);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, APP, CONSOLE};
    use fenix_extension_api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The terminals started, as lines.
    type Terminals = Rc<RefCell<Vec<String>>>;

    fn pack(dir: &std::path::Path, fake: &Fake, os: &str) -> (Harness<Dart>, Terminals) {
        let started = Terminals::default();
        let log = started.clone();
        let terminal: Terminal = Box::new(move |cmd, title| {
            log.borrow_mut().push(format!("{title}: {}", fenix_test_kit::line(cmd)));
            Ok(7)
        });
        let d = Dart::new(fake.host(), &dir.to_string_lossy(), terminal, os);
        (Harness::wrap(fake.clone(), d), started)
    }

    /// Dart installed, saying its version.
    fn with_dart() -> Fake {
        let fake = Fake::new();
        fake.program("dart", "/t/dart").on_spawn("/t/dart --version", Ran::ok(include_str!("samples/dart-version.txt")));
        fake
    }

    /// Flutter and its Dart beside it, saying their versions and devices.
    fn with_flutter() -> Fake {
        let fake = Fake::new();
        fake.program("dart", "/f/bin/dart")
            .program("flutter", "/f/bin/flutter")
            .on_spawn("/f/bin/dart --version", Ran::ok(include_str!("samples/dart-version.txt")))
            .on_spawn("/f/bin/flutter --version --machine", Ran::ok(include_str!("samples/flutter-version.json")))
            .on_spawn("/f/bin/flutter devices --machine", Ran::ok(include_str!("samples/flutter-devices.json")));
        fake
    }

    fn calls_with(fake: &Fake, start: &str) -> Vec<String> {
        fake.take().into_iter().filter(|c| c.starts_with(start)).collect()
    }

    #[test]
    fn a_dart_package_gets_its_server_formatter_adapter_tasks_and_checks() {
        let dir = temp("lib-dart");
        write(&dir, &[("pubspec.yaml", CONSOLE), ("bin/orbit.dart", "void main() {}\n")]);
        let fake = with_dart();
        let (mut d, _) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        let calls = fake.take();
        for want in [
            "server dart: /t/dart language-server --client-id fenix",
            "formatter dart: /t/dart format --stdin-name ${file}",
            "adapter dart: /t/dart debug_adapter",
            "launches Dart: run bin/orbit.dart",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert!(calls[0].starts_with("tasks pub get, run, run (attachable), test") || calls.iter().any(|c| c.starts_with("tasks pub get, run, run (attachable), analyze")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains("flutter")), "a Dart package doesn't touch Flutter: {calls:?}");

        // The doctor: which Dart, its version, held to ^3.14.0; the packages not resolved.
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[0], ("Dart SDK".into(), Health::Ok, "3.14.0 stable (standalone, /t/dart): the analysis server and debugger come from it".into()));
        assert_eq!((checks[1].0.as_str(), checks[1].1), ("Packages", Health::Warn));
        assert_eq!(fake.checks()[1].fix.as_ref().map(|f| f.1.as_str()), Some("dart.pub-get"));
        assert_eq!(fake.spawned().into_iter().map(|(_, l)| l).collect::<Vec<_>>(), ["/t/dart --version"]);

        // Its fix: pub get, as a task; and the check is made again once `.dart_tool` is there.
        d.command("dart.pub-get").unwrap();
        let calls = fake.take();
        let task = calls.iter().find(|c| c.ends_with("pub get: /t/dart pub get")).unwrap_or_else(|| panic!("{calls:?}"));
        let id: u64 = task.split(' ').nth(1).unwrap().parse().unwrap();
        write(&dir, &[(".dart_tool/package_config.json", "{}")]);
        d.event(Event::TaskFinished((id, true)));
        assert_eq!(fake.checks().iter().map(|c| (c.label.as_str(), c.health)).collect::<Vec<_>>()[1], ("Packages", Health::Ok));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flutter_app_is_debugged_with_flutters_adapter_and_has_a_launch_for_each_device() {
        let dir = temp("lib-flutter");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", "void main() {}\n"), ("android/a", ""), ("web/a", ""), (".dart_tool/package_config.json", "{}")]);
        let fake = with_flutter();
        let (mut d, _) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        let calls = fake.take();
        for want in ["server dart: /f/bin/dart language-server --client-id fenix", "adapter dart: /f/bin/flutter debug_adapter", "formatter dart: /f/bin/dart format --stdin-name ${file}"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert!(calls.iter().any(|c| c.starts_with("tasks pub get, run, test, analyze, format, build apk, build appbundle, build web, devices, doctor, clean")), "{calls:?}");
        // The devices, listed once as it starts: a launch on each.
        assert_eq!(fake.spawned().into_iter().map(|(_, l)| l).collect::<Vec<_>>(), ["/f/bin/dart --version", "/f/bin/flutter --version --machine", "/f/bin/flutter devices --machine"]);
        let launches = calls_with(&fake, "launches");
        assert!(launches.is_empty(), "taken already: {launches:?}");
        let names: Vec<String> = fake.state().launches.iter().map(|l| l.name.clone()).collect();
        assert_eq!(names, ["Flutter: run", "Flutter: run this file", "Flutter: run on sdk gphone64 x86 64", "Flutter: run on Pixel 8", "Flutter: run on Linux (desktop)", "Flutter: run on Chrome (web)"]);
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[0].0, "Dart SDK");
        assert!(checks[0].2.contains("Flutter's, /f/bin/dart"), "{}", checks[0].2);
        assert_eq!(checks[1], ("Flutter".into(), Health::Ok, "3.27.1 stable, with Dart 3.6.0 (/home/dev/flutter)".into()));
        assert_eq!(checks[2].1, Health::Ok, "packages resolved");

        // Listing says what's there.
        d.command("dart.devices").unwrap();
        assert_eq!(fake.notifications().last().unwrap(), "Flutter's devices: sdk gphone64 x86 64 · android-x64 (emulator); Pixel 8 · android-arm64; Linux (desktop) · linux-x64; Chrome (web) · web-javascript");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_on_a_device_picks_one_and_runs_flutter_as_a_task_which_can_be_stopped() {
        let dir = temp("lib-run");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", "")]);
        let fake = with_flutter();
        fake.on_pick("Run on", "Linux (desktop)");
        let (mut d, _) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        fake.take();
        d.command(RUN_ON_DEVICE).unwrap();
        let picker = fake.pickers().pop().unwrap();
        assert_eq!(picker.items.iter().map(|i| i.value.as_str()).collect::<Vec<_>>(), ["emulator-5554", "37271FDJG0023A", "linux", "chrome"]);
        let calls = fake.take();
        let run = calls.iter().find(|c| c.starts_with("background ")).unwrap_or_else(|| panic!("{calls:?}"));
        assert!(run.ends_with("flutter run: /f/bin/flutter run --pid-file .dart_tool/fenix-flutter.pid -d linux"), "{run}");
        let id: u64 = run.split(' ').nth(1).unwrap().parse().unwrap();
        assert!(calls.contains(&format!("show {id}")), "its output brought up: {calls:?}");

        // Not twice; and stopping it.
        assert!(d.command(RUN_ON_DEVICE).unwrap_err().contains("already going"));
        d.command(STOP).unwrap();
        assert!(fake.take().contains(&format!("cancel {id}")));
        assert!(d.command(STOP).is_err(), "nothing to stop now");
        d.command(RUN_ON_DEVICE).unwrap();
        let again = calls_with(&fake, "background");
        assert_eq!(again.len(), 1);

        // How it ended.
        let id: u64 = again[0].split(' ').nth(1).unwrap().parse().unwrap();
        d.event(Event::TaskFinished((id, false)));
        assert!(fake.notifications().last().unwrap().starts_with("flutter run ended with an error"));
        d.command(RUN_ON_DEVICE).unwrap();
        assert_eq!(calls_with(&fake, "background").len(), 1, "it can be run again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn one_device_is_run_on_without_asking_none_runs_flutters_own_choice_and_a_terminal_takes_r() {
        let dir = temp("lib-run-one");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", "")]);
        let fake = with_flutter();
        fake.on_spawn("/f/bin/flutter devices --machine", Ran::ok("[{\"name\": \"Linux (desktop)\", \"id\": \"linux\", \"isSupported\": true, \"targetPlatform\": \"linux-x64\", \"emulator\": false, \"sdk\": \"Ubuntu\"}]"));
        let (mut d, terminals) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        fake.take();
        d.command(RUN_IN_TERMINAL).unwrap();
        assert!(fake.pickers().is_empty(), "one device: no question");
        assert_eq!(*terminals.borrow(), ["flutter run: /f/bin/flutter run --pid-file .dart_tool/fenix-flutter.pid -d linux"]);
        assert!(calls_with(&fake, "background").is_empty(), "a terminal, not a task");

        fake.on_spawn("/f/bin/flutter devices --machine", Ran::ok("[]"));
        d.command(RUN_IN_TERMINAL).unwrap();
        assert_eq!(terminals.borrow()[1], "flutter run: /f/bin/flutter run --pid-file .dart_tool/fenix-flutter.pid");
        assert_eq!(fake.notifications().last().unwrap(), "Flutter finds no device: running flutter run without one");

        // Flutter failing says why, and doesn't run anything.
        fake.on_spawn("/f/bin/flutter devices --machine", Ran::fails(1, "Unable to locate Android SDK."));
        d.command(RUN_IN_TERMINAL).unwrap();
        assert_eq!(terminals.borrow().len(), 2);
        assert_eq!(fake.notifications().last().unwrap(), "Flutter's devices: Unable to locate Android SDK.");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hot_reload_and_restart_are_signals_to_the_pid_flutter_run_wrote() {
        let dir = temp("lib-hot");
        write(&dir, &[("pubspec.yaml", APP), ("lib/main.dart", "")]);
        let fake = with_flutter();
        let (mut d, _) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        fake.take();
        assert!(d.command(HOT_RELOAD).unwrap_err().contains("no flutter run to send it to"), "no pid file yet");

        write(&dir, &[(".dart_tool/fenix-flutter.pid", "48213\n")]);
        fake.on_spawn("kill -s USR1 48213", Ran::ok("")).on_spawn("kill -s USR2 48213", Ran::ok(""));
        d.command(HOT_RELOAD).unwrap();
        assert_eq!(fake.notifications().last().unwrap(), "Hot reload sent to flutter run");
        d.command(HOT_RESTART).unwrap();
        assert_eq!(fake.notifications().last().unwrap(), "Hot restart sent to flutter run");
        let sent: Vec<String> = fake.spawned().into_iter().map(|(_, l)| l).filter(|l| l.starts_with("kill")).collect();
        assert_eq!(sent, ["kill -s USR1 48213", "kill -s USR2 48213"]);

        // A pid left by a run that's over.
        fake.on_spawn("kill -s USR1 48213", Ran::fails(1, "kill: (48213): No such process"));
        d.command(HOT_RELOAD).unwrap();
        let said = fake.notifications().last().unwrap().clone();
        assert!(said.starts_with("Hot reload: kill: (48213): No such process -- is flutter run still going?"), "{said}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows_has_no_signals_and_says_to_use_the_terminal() {
        let dir = temp("lib-hot-windows");
        write(&dir, &[("pubspec.yaml", APP), (".dart_tool/fenix-flutter.pid", "1\n")]);
        let fake = with_flutter();
        let (mut d, _) = pack(&dir, &fake, "windows");
        d.activate().unwrap();
        let why = d.command(HOT_RELOAD).unwrap_err();
        assert!(why.contains("Windows has none of") && why.contains("press r there"), "{why}");
        assert!(d.command(HOT_RESTART).unwrap_err().contains("press R there"));
        assert!(!fake.spawned().iter().any(|(_, l)| l.starts_with("kill")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_dart_nothing_is_set_and_the_fix_row_says_how_to_get_it() {
        let dir = temp("lib-none");
        write(&dir, &[("pubspec.yaml", CONSOLE)]);
        let fake = Fake::new();
        let (mut d, _) = pack(&dir, &fake, "macos");
        d.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter") || c.starts_with("adapter")), "nothing found, nothing set: {calls:?}");
        let check = fake.checks()[0].clone();
        assert_eq!((check.label.as_str(), check.health, check.detail.as_str()), ("Dart SDK", Health::Bad, "dart isn't installed (or isn't on PATH)"));
        assert_eq!(check.fix.as_ref().map(|f| f.1.as_str()), Some("dart.get-dart"));
        d.command("dart.get-dart").unwrap();
        assert_eq!(fake.state().urls, ["https://dart.dev/get-dart"]);
        assert_eq!(fake.notifications().last().unwrap(), "Dart (brew tap dart-lang/dart && brew install dart): https://dart.dev/get-dart");
        d.command("dart.get-flutter").unwrap();
        assert_eq!(fake.state().urls[1], "https://docs.flutter.dev/get-started/install");
        assert!(d.command(DEVICES).unwrap_err().starts_with("Flutter isn't installed"));
        assert!(d.command("dart.nothing").is_err());

        // Installed some other way, by a task: found and used.
        fake.program("dart", "/h/dart-sdk/bin/dart").on_spawn("/h/dart-sdk/bin/dart --version", Ran::ok(include_str!("samples/dart-version.txt")));
        d.event(Event::TaskFinished((99, true)));
        let calls = fake.take();
        for want in ["server dart: /h/dart-sdk/bin/dart language-server --client-id fenix", "formatter dart: /h/dart-sdk/bin/dart format --stdin-name ${file}"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(fake.checks()[0].health, Health::Ok);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pubspec_saved_changes_what_the_project_is() {
        let dir = temp("lib-saved");
        write(&dir, &[("pubspec.yaml", CONSOLE), ("lib/main.dart", "")]);
        let fake = with_flutter();
        let (mut d, _) = pack(&dir, &fake, "linux");
        d.activate().unwrap();
        assert!(fake.tasks().iter().all(|t| t.name != "doctor"));
        // It starts depending on Flutter: Flutter's adapter, tasks and check.
        write(&dir, &[("pubspec.yaml", APP)]);
        fake.take();
        d.event(Event::Saved("/project/pubspec.yaml".into()));
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "adapter dart: /f/bin/flutter debug_adapter"), "{calls:?}");
        assert!(fake.tasks().iter().any(|t| t.name == "doctor"));
        assert!(fake.checks().iter().any(|c| c.label == "Flutter"));
        // Another file saved is not that.
        d.event(Event::Saved("/project/lib/main.dart".into()));
        assert!(fake.take().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pub_page_lists_the_packages_outdated_first() {
        let dir = temp("lib-packages");
        write(&dir, &[("pubspec.yaml", CONSOLE), (".dart_tool/package_config.json", "{}")]);
        let fake = with_dart();
        fake.on_spawn("/t/dart pub deps --json", Ran::ok(include_str!("samples/pub-deps.json"))).on_spawn("/t/dart pub outdated --json", Ran::ok(include_str!("samples/pub-outdated.json")));
        let (mut h, _) = pack(&dir, &fake, "linux");
        h.activate().unwrap();
        h.command("dart.packages").unwrap();
        let page = h.page("packages");
        let screen = h.screen(page);
        let http = screen.lines().position(|l| l.starts_with("http ") && l.contains("0.13.6")).unwrap_or_else(|| panic!("{screen}"));
        let path = screen.lines().position(|l| l.starts_with("path ") && l.contains("1.9.1")).unwrap_or_else(|| panic!("{screen}"));
        assert!(http < path, "the outdated first:\n{screen}");
        assert!(screen.lines().nth(http).unwrap().contains("1.6.0"), "{screen}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
