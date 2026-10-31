//! Tcl in Fenix, as a WebAssembly extension: a language server for Tcl
//! files -- `K` on a built-in, completion of options, subcommands,
//! built-ins and the project's own procs, `gd` -- `SPC m s` and `SPC m T`,
//! and the project doctor's checks.
//!
//! `server` decides; here what it asks for is carried out through the
//! host, and what the host says is given back to it.

pub mod doctor;
pub mod server;
pub mod signatures;
pub mod tags;
pub mod tcl;

use fenix_extension_api::lsp::Servers;
use fenix_extension_api::{export, host, project, Check, Command, Event, Extension, Health, Level, PickerItem, Section};

use server::{Effect, Tcl};

/// Where an install puts Universal Ctags and Tcl, besides `PATH` and
/// Fenix's tools folder.
const CTAGS_HINTS: &[&str] = &["%LOCALAPPDATA%/Microsoft/WinGet/Links", "%ProgramData%/chocolatey/bin", "%USERPROFILE%/scoop/shims", "/usr/local/bin", "/opt/homebrew/bin"];
const TCLSH_HINTS: &[&str] = &["%ProgramFiles%/Tcl86/bin", "%ProgramFiles%/Tcl/bin", "C:/ActiveTcl/bin", "C:/Tcl/bin", "%USERPROFILE%/scoop/shims", "/usr/local/bin", "/opt/homebrew/bin"];

#[derive(Default)]
struct TclExtension {
    servers: Servers,
    tcl: Tcl,
    /// The symbol picker open now.
    pick: Option<u64>,
}

impl TclExtension {
    /// The project you're in when a command runs: the file's, else the
    /// one its language server serves.
    fn root(&self) -> Option<String> {
        fenix_extension_api::here().and_then(|h| h.project).map(|p| tags::normalize(&p.root)).or_else(|| self.tcl.only_root())
    }

    /// Carries out what `Tcl` asked for, and what that leads to.
    fn apply(&mut self) {
        loop {
            let effects = std::mem::take(&mut self.tcl.effects);
            if effects.is_empty() {
                return;
            }
            for effect in effects {
                match effect {
                    Effect::Scan { scan, root } => match fenix_extension_api::find_program("ctags", CTAGS_HINTS) {
                        Some(ctags) => match Command::new(&ctags).args(tags::args(&root)).spawn() {
                            Ok(process) => self.tcl.started(scan, process),
                            Err(why) => self.tcl.failed(scan, &format!("couldn't run ctags: {why}")),
                        },
                        None => self.tcl.failed(scan, "ctags not found -- completion of the project's procs, gd and SPC m s need Universal Ctags on PATH"),
                    },
                    Effect::Notify(text) => fenix_extension_api::notify(&text),
                    Effect::Error(text) => host::notify(Level::Error, &text),
                    Effect::Log(text) => fenix_extension_api::warn!("{text}"),
                    Effect::Pick { title, items } => {
                        let items: Vec<PickerItem> = items.into_iter().map(|(label, detail, value)| PickerItem::new(label, value).detail(detail)).collect();
                        self.pick = Some(fenix_extension_api::pick(&title, &items));
                    }
                    Effect::Open { root, file, line } => {
                        // Under `/project` when it's in it; a full path
                        // otherwise (which `editor = "read"` lets it open).
                        let path = tags::relative_to(&file, &root).map(|rel| format!("/project/{rel}")).unwrap_or(file);
                        host::open_file(&path, Some(line));
                    }
                }
            }
        }
    }

    /// The doctor's checks, for a Tcl project.
    fn doctor(&self) {
        if !doctor::is_tcl_project(std::path::Path::new("/project")) {
            return project::doctor(&[]);
        }
        let tclsh = fenix_extension_api::find_program("tclsh", TCLSH_HINTS);
        let ctags = fenix_extension_api::find_program("ctags", CTAGS_HINTS);
        let checks: Vec<Check> = doctor::checks(tclsh.as_deref(), ctags.as_deref())
            .into_iter()
            .map(|c| {
                let health = match c.health {
                    doctor::Health::Ok => Health::Ok,
                    doctor::Health::Info => Health::Info,
                    doctor::Health::Warn => Health::Warn,
                };
                Check::new(Section::Toolchain, c.label, health, c.detail)
            })
            .collect();
        project::doctor(&checks);
    }
}

impl Extension for TclExtension {
    fn activate(&mut self) -> Result<(), String> {
        self.doctor();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "code.symbols" => {
                let root = self.root();
                self.tcl.symbols(root);
            }
            "completion.refresh_tags" => {
                let root = self.root();
                self.tcl.refresh(root);
            }
            _ => return Err(format!("no command {id}")),
        }
        self.apply();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::ProcessOutput(o) => self.tcl.output(o.process, o.stderr, &o.line),
            Event::ProcessExit((process, code)) => self.tcl.exited(process, code),
            Event::Picked((id, value)) if Some(id) == self.pick => {
                self.pick = None;
                self.tcl.picked(value.as_deref());
            }
            _ => {}
        }
        self.apply();
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.servers.handle(server, message, &mut self.tcl);
        self.apply();
    }
}

export!(TclExtension);
