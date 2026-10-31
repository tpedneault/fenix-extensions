//! A language server or a debug adapter installed into the tools folder,
//! `/tools`: downloaded and checked against its SHA-256 by Fenix
//! (`http-download`), unpacked here a step at a time, made to run
//! (`set-executable`), and set for its languages (`project::
//! language_server`, `debug_adapter`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use fenix_extension_api::{Command, Event, HttpRequest};

use crate::archive::{Kind, Unpack};
use crate::host::Host;

/// How much is unpacked a step: well within a call's deadline.
pub const STEP_BYTES: u64 = 16 << 20;

/// The file in an install's folder that says what it was installed from.
pub const MARKER: &str = ".lang-kit-install";

/// What an installed program is used as.
#[derive(Debug, Clone, PartialEq)]
pub enum Use {
    LanguageServer { languages: Vec<String>, cmd: Command },
    DebugAdapter { languages: Vec<String>, cmd: Command },
    /// A debug adapter over a socket (`debug-adapter-connect`):
    /// Fenix starts `cmd` and connects at 127.0.0.1:`port`, 0 a free one,
    /// which `${port}` in its args is (js-debug, CodeLLDB's `--port`).
    DebugAdapterConnect { languages: Vec<String>, cmd: Command, port: u16 },
    /// A language server beside the others, by name
    /// (`language-server-add`).
    AddedLanguageServer { name: String, languages: Vec<String>, cmd: Command },
}

/// A release to install: where it's from, and what's in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Install {
    /// Its folder under the tools folder, and its name in commands.
    pub name: String,
    /// What it's called in messages.
    pub label: String,
    pub url: String,
    /// The release's SHA-256, hex: Fenix refuses one that isn't it.
    pub sha256: Option<String>,
    pub headers: Vec<(String, String)>,
    pub kind: Kind,
    /// Leading folders taken off the archive's paths.
    pub strip: usize,
    /// The program in it, relative to its folder (`bin/server`): made to
    /// run, and what its uses run unless they say otherwise. A lone file
    /// (`.gz`, a plain download) is written as this.
    pub program: String,
    /// More to make runnable than the archive marks.
    pub executables: Vec<String>,
    pub uses: Vec<Use>,
}

impl Install {
    /// `name`, from `url`, its kind from the URL's file name.
    pub fn new(name: &str, url: &str) -> Install {
        let file = url.rsplit('/').next().unwrap_or(name).split(['?', '#']).next().unwrap_or(name);
        let kind = Kind::from_name(file);
        let program = match kind {
            Kind::Gz => file[..file.len() - 3].to_string(),
            Kind::Plain => file.to_string(),
            _ => name.to_string(),
        };
        Install { name: name.into(), label: name.into(), url: url.into(), sha256: None, headers: Vec::new(), kind, strip: 0, program, executables: Vec::new(), uses: Vec::new() }
    }

    pub fn label(mut self, label: &str) -> Install {
        self.label = label.into();
        self
    }

    pub fn sha256(mut self, hex: &str) -> Install {
        self.sha256 = Some(hex.to_ascii_lowercase());
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Install {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn kind(mut self, kind: Kind) -> Install {
        self.kind = kind;
        self
    }

    pub fn strip(mut self, n: usize) -> Install {
        self.strip = n;
        self
    }

    pub fn program(mut self, path: &str) -> Install {
        self.program = path.trim_start_matches('/').into();
        self
    }

    pub fn executable(mut self, path: &str) -> Install {
        self.executables.push(path.trim_start_matches('/').into());
        self
    }

    /// Its program, with `args`, as the language server for `languages`.
    pub fn language_server<S: Into<String>>(mut self, languages: &[&str], args: impl IntoIterator<Item = S>) -> Install {
        let cmd = self.run_in("/tools").args(args);
        self.uses.push(Use::LanguageServer { languages: languages.iter().map(|l| l.to_string()).collect(), cmd });
        self
    }

    /// Its program, with `args`, as the debug adapter for `languages`.
    pub fn debug_adapter<S: Into<String>>(mut self, languages: &[&str], args: impl IntoIterator<Item = S>) -> Install {
        let cmd = self.run_in("/tools").args(args);
        self.uses.push(Use::DebugAdapter { languages: languages.iter().map(|l| l.to_string()).collect(), cmd });
        self
    }

    /// Its program, with `args`, as the debug adapter for `languages`
    /// over a socket: `["--port", "${port}"]` with `port` 0.
    pub fn debug_adapter_connect<S: Into<String>>(mut self, languages: &[&str], args: impl IntoIterator<Item = S>, port: u16) -> Install {
        let cmd = self.run_in("/tools").args(args);
        self.uses.push(Use::DebugAdapterConnect { languages: languages.iter().map(|l| l.to_string()).collect(), cmd, port });
        self
    }

    /// Its program, with `args`, as a language server called `name` beside
    /// the others for `languages`.
    pub fn added_language_server<S: Into<String>>(mut self, name: &str, languages: &[&str], args: impl IntoIterator<Item = S>) -> Install {
        let cmd = self.run_in("/tools").args(args);
        self.uses.push(Use::AddedLanguageServer { name: name.into(), languages: languages.iter().map(|l| l.to_string()).collect(), cmd });
        self
    }

    /// A use of its own: `java -jar ${tools}/jdtls/...`, `node
    /// ${tools}/js-debug/...`.
    pub fn use_as(mut self, u: Use) -> Install {
        self.uses.push(u);
        self
    }

    /// Its folder, under `tools`.
    pub fn dir(&self, tools: &str) -> String {
        format!("{tools}/{}", self.name)
    }

    /// Its program, run by its path under `tools`.
    pub fn run_in(&self, tools: &str) -> Command {
        Command::new(&format!("{}/{}", self.dir(tools), self.program))
    }
}

enum Job {
    Downloading { http: u64, archive: PathBuf },
    Unpacking { timer: u64, archive: PathBuf, unpack: Box<Unpack> },
}

/// What [`Installer::handle`] made of an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// Not its.
    No,
    /// Its, and it's going on.
    Yes,
    /// An install ended: its name, and how.
    Ended(String, Result<(), String>),
}

/// The installs a pack offers, and those going on.
pub struct Installer {
    /// The tools folder: `/tools`.
    pub tools: String,
    pub installs: Vec<Install>,
    jobs: HashMap<String, Job>,
}

impl Default for Installer {
    fn default() -> Installer {
        Installer { tools: "/tools".into(), installs: Vec::new(), jobs: HashMap::new() }
    }
}

impl Installer {
    pub fn new() -> Installer {
        Installer::default()
    }

    pub fn add(&mut self, install: Install) {
        self.installs.retain(|i| i.name != install.name);
        self.installs.push(install);
    }

    pub fn get(&self, name: &str) -> Option<&Install> {
        self.installs.iter().find(|i| i.name == name)
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.jobs.contains_key(name)
    }

    /// Whether `name` is installed, from the URL it names now: a new
    /// release is installed again.
    pub fn installed(&self, name: &str) -> bool {
        let Some(i) = self.get(name) else { return false };
        fs::read_to_string(Path::new(&i.dir(&self.tools)).join(MARKER)).is_ok_and(|u| u.trim() == i.url)
    }

    /// Each installed one's uses set: when it starts.
    pub fn apply_installed(&self, host: &mut dyn Host) {
        for i in self.installs.iter().filter(|i| self.installed(&i.name)) {
            apply_uses(host, i);
        }
    }

    /// Starts installing `name`.
    pub fn start(&mut self, host: &mut dyn Host, name: &str) -> Result<(), String> {
        let i = self.get(name).ok_or_else(|| format!("nothing to install called {name}"))?.clone();
        if self.is_running(name) {
            return Err(format!("{} is being installed", i.label));
        }
        let downloads = Path::new(&self.tools).join(".downloads");
        fs::create_dir_all(&downloads).map_err(|e| format!("{}: {e}", downloads.display()))?;
        let file = i.url.rsplit('/').next().unwrap_or("download").split(['?', '#']).next().unwrap_or("download");
        let archive = downloads.join(format!("{}-{file}", i.name));
        let _ = fs::remove_file(&archive);
        let mut req = HttpRequest::get(&i.url);
        for (k, v) in &i.headers {
            req = req.header(k, v);
        }
        let http = host.download(&req, &archive.to_string_lossy(), i.sha256.as_deref())?;
        host.notify(&format!("Downloading {}…", i.label));
        self.jobs.insert(i.name.clone(), Job::Downloading { http, archive });
        Ok(())
    }

    /// An event for one going on: its download done, or the time for
    /// its next step.
    pub fn handle(&mut self, host: &mut dyn Host, event: &Event) -> Handled {
        let name = match event {
            Event::HttpResponse((id, _)) => self.jobs.iter().find(|(_, j)| matches!(j, Job::Downloading { http, .. } if http == id)),
            Event::Timer(t) => self.jobs.iter().find(|(_, j)| matches!(j, Job::Unpacking { timer, .. } if timer == t)),
            _ => None,
        }
        .map(|(n, _)| n.clone());
        let Some(name) = name else { return Handled::No };
        let Some(job) = self.jobs.remove(&name) else { return Handled::No };
        let Some(install) = self.get(&name).cloned() else { return Handled::No };
        let result = match (job, event) {
            (Job::Downloading { archive, .. }, Event::HttpResponse((_, answer))) => match answer {
                Err(why) => Err(why.clone()),
                Ok(r) if !(200..300).contains(&r.status) => Err(format!("the server said {}", r.status)),
                Ok(_) => {
                    let partial = self.partial(&install);
                    let _ = fs::remove_dir_all(&partial);
                    let lone = install.program.rsplit('/').next().unwrap_or(&install.program).to_string();
                    match Unpack::new(&archive, install.kind, &partial, install.strip, &lone) {
                        Ok(unpack) => {
                            let timer = host.set_timer(1);
                            self.jobs.insert(name, Job::Unpacking { timer, archive, unpack: Box::new(unpack) });
                            return Handled::Yes;
                        }
                        Err(why) => Err(why),
                    }
                }
            },
            (Job::Unpacking { archive, mut unpack, .. }, _) => match unpack.step(STEP_BYTES) {
                Ok(false) => {
                    let timer = host.set_timer(1);
                    self.jobs.insert(name, Job::Unpacking { timer, archive, unpack });
                    return Handled::Yes;
                }
                Ok(true) => {
                    let _ = fs::remove_file(&archive);
                    self.finish(host, &install, &unpack)
                }
                Err(why) => {
                    let _ = fs::remove_file(&archive);
                    Err(why)
                }
            },
            (job, _) => {
                self.jobs.insert(name, job);
                return Handled::Yes;
            }
        };
        match &result {
            Ok(()) => host.notify(&format!("Installed {}", install.label)),
            Err(why) => {
                let _ = fs::remove_dir_all(self.partial(&install));
                host.notify(&format!("Couldn't install {}: {why}", install.label));
            }
        }
        Handled::Ended(name, result)
    }

    fn partial(&self, i: &Install) -> PathBuf {
        PathBuf::from(format!("{}.partial", i.dir(&self.tools)))
    }

    /// Unpacked: put in place of what was there, made to run, marked, and
    /// used.
    fn finish(&self, host: &mut dyn Host, i: &Install, unpack: &Unpack) -> Result<(), String> {
        let dir = PathBuf::from(i.dir(&self.tools));
        let partial = self.partial(i);
        if !partial.join(&i.program).is_file() {
            return Err(format!("{} isn't in it", i.program));
        }
        let _ = fs::remove_dir_all(&dir);
        fs::rename(&partial, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut runnable: Vec<&String> = unpack.executables.iter().chain(&i.executables).collect();
        runnable.push(&i.program);
        runnable.sort();
        runnable.dedup();
        for rel in runnable {
            let path = format!("{}/{rel}", i.dir(&self.tools));
            if let Err(why) = host.set_executable(&path) {
                host.log(&format!("{path}: {why}"));
            }
        }
        fs::write(dir.join(MARKER), &i.url).map_err(|e| format!("{}: {e}", dir.display()))?;
        apply_uses(host, i);
        Ok(())
    }
}

fn apply_uses(host: &mut dyn Host, i: &Install) {
    for u in &i.uses {
        let r = match u {
            Use::LanguageServer { languages, cmd } => host.language_server(languages, Some(cmd)),
            Use::DebugAdapter { languages, cmd } => host.debug_adapter(languages, Some(cmd)),
            Use::DebugAdapterConnect { languages, cmd, port } => host.debug_adapter_connect(languages, Some(cmd), *port),
            Use::AddedLanguageServer { name, languages, cmd } => host.add_language_server(name, languages, Some(cmd)),
        };
        if let Err(why) = r {
            host.log(&format!("{}: {why}", i.label));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::tests::{tar_gz_bytes, temp, zip_bytes};
    use crate::host::mock::Mock;
    use fenix_extension_api::HttpResponse;

    const URL: &str = "https://github.com/x/server/releases/download/v1.0/server-1.0.tar.gz";

    fn ok() -> Result<HttpResponse, String> {
        Ok(HttpResponse::new(200, Vec::new()))
    }

    fn installer(name: &str) -> (Installer, Mock, PathBuf) {
        let tools = temp(name);
        let mut ins = Installer::new();
        ins.tools = tools.to_string_lossy().into_owned();
        ins.add(Install::new("server", URL).label("Server").sha256("ABC123").strip(1).program("bin/server").language_server(&["python"], ["--stdio"]).debug_adapter(&["python"], Vec::<String>::new()));
        let mut host = Mock::default();
        host.files.insert(URL.into(), tar_gz_bytes());
        (ins, host, tools)
    }

    fn run(ins: &mut Installer, host: &mut Mock) -> Handled {
        let mut timer = None;
        for c in host.take() {
            if let Some(t) = c.strip_prefix("timer ") {
                timer = t.split(' ').next().and_then(|t| t.parse().ok());
            }
        }
        let mut steps = 0;
        while let Some(t) = timer.take() {
            steps += 1;
            assert!(steps < 100);
            match ins.handle(host, &Event::Timer(t)) {
                Handled::Yes => timer = host.calls.iter().rev().find_map(|c| c.strip_prefix("timer ")).and_then(|t| t.split(' ').next()?.parse().ok()),
                h => return h,
            }
        }
        panic!("it didn't end")
    }

    #[test]
    fn a_release_is_downloaded_checked_unpacked_made_to_run_and_used() {
        let (mut ins, mut host, tools) = installer("install");
        let t = tools.to_string_lossy().into_owned();
        assert_eq!(ins.get("server").map(|i| i.kind), Some(Kind::TarGz));
        assert!(!ins.installed("server"));
        ins.start(&mut host, "server").unwrap();
        assert_eq!(host.take(), [format!("download 1 {URL} -> {t}/.downloads/server-server-1.0.tar.gz abc123"), "notify Downloading Server…".into()]);
        assert!(ins.start(&mut host, "server").unwrap_err().contains("being installed"));
        assert_eq!(ins.handle(&mut host, &Event::HttpResponse((1, ok()))), Handled::Yes);
        assert_eq!(run(&mut ins, &mut host), Handled::Ended("server".into(), Ok(())));
        let calls: Vec<String> = host.take().into_iter().filter(|c| !c.starts_with("timer")).collect();
        assert_eq!(
            calls,
            [
                format!("executable {t}/server/bin/server"),
                "server python: /tools/server/bin/server --stdio".into(),
                "adapter python: /tools/server/bin/server".into(),
                "notify Installed Server".into(),
            ]
        );
        assert_eq!(fs::read(tools.join("server/lib/core.js")).unwrap(), b"module.exports = 1\n");
        assert!(!tools.join("server.partial").exists() && !tools.join(".downloads/server-server-1.0.tar.gz").exists());
        assert!(ins.installed("server"));
        ins.apply_installed(&mut host);
        assert_eq!(host.take().len(), 2, "used again when it starts");
        // A new release isn't the one installed.
        let mut newer = ins.get("server").unwrap().clone();
        newer.url = URL.replace("1.0", "1.1");
        ins.add(newer);
        assert!(!ins.installed("server"));
        fs::remove_dir_all(&tools).unwrap();
    }

    #[test]
    fn a_failed_download_or_a_missing_program_is_said() {
        let (mut ins, mut host, tools) = installer("install-fail");
        ins.start(&mut host, "server").unwrap();
        host.take();
        let refused = Event::HttpResponse((1, Err("its SHA-256 isn't abc123".into())));
        assert_eq!(ins.handle(&mut host, &refused), Handled::Ended("server".into(), Err("its SHA-256 isn't abc123".into())));
        assert_eq!(host.take(), ["notify Couldn't install Server: its SHA-256 isn't abc123"]);
        ins.start(&mut host, "server").unwrap();
        host.take();
        let missing = Event::HttpResponse((2, Ok(HttpResponse::new(404, Vec::new()))));
        assert_eq!(ins.handle(&mut host, &missing), Handled::Ended("server".into(), Err("the server said 404".into())));
        // An archive without the program it names.
        let mut i = ins.get("server").unwrap().clone();
        i.program = "bin/nope".into();
        ins.add(i);
        ins.start(&mut host, "server").unwrap();
        host.take();
        ins.handle(&mut host, &Event::HttpResponse((3, ok())));
        assert_eq!(run(&mut ins, &mut host), Handled::Ended("server".into(), Err("bin/nope isn't in it".into())));
        assert!(!tools.join("server").exists() && !tools.join("server.partial").exists());
        assert!(ins.start(&mut host, "other").is_err());
        assert_eq!(ins.handle(&mut host, &Event::Timer(99)), Handled::No);
        fs::remove_dir_all(&tools).unwrap();
    }

    #[test]
    fn a_zip_and_a_lone_gz_install_too() {
        let tools = temp("install-zip");
        let mut ins = Installer::new();
        ins.tools = tools.to_string_lossy().into_owned();
        let zip = "https://x/adapter-linux.vsix";
        let gz = "https://x/rust-analyzer-x86_64-unknown-linux-gnu.gz";
        ins.add(Install::new("adapter", zip).strip(1).program("bin/server").use_as(Use::DebugAdapter { languages: vec!["rust".into()], cmd: Command::new("node").arg("${tools}/adapter/lib/core.js") }));
        ins.add(Install::new("ra", gz).language_server(&["rust"], Vec::<String>::new()).added_language_server("ra-extra", &["toml"], ["--toml"]).debug_adapter_connect(&["rust"], ["--port", "${port}"], 0));
        assert_eq!((ins.get("ra").unwrap().kind, ins.get("ra").unwrap().program.as_str()), (Kind::Gz, "rust-analyzer-x86_64-unknown-linux-gnu"));
        let mut host = Mock::default();
        host.files.insert(zip.into(), zip_bytes());
        let mut gzipped = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gzipped, b"ELF").unwrap();
        host.files.insert(gz.into(), gzipped.finish().unwrap());
        ins.start(&mut host, "adapter").unwrap();
        ins.start(&mut host, "ra").unwrap();
        host.take();
        ins.handle(&mut host, &Event::HttpResponse((1, ok())));
        assert_eq!(run(&mut ins, &mut host), Handled::Ended("adapter".into(), Ok(())));
        assert!(host.take().contains(&"adapter rust: node ${tools}/adapter/lib/core.js".to_string()));
        ins.handle(&mut host, &Event::HttpResponse((2, ok())));
        assert_eq!(run(&mut ins, &mut host), Handled::Ended("ra".into(), Ok(())));
        assert_eq!(fs::read(tools.join("ra/rust-analyzer-x86_64-unknown-linux-gnu")).unwrap(), b"ELF");
        let calls = host.take();
        for c in ["server rust: /tools/ra/rust-analyzer-x86_64-unknown-linux-gnu", "add ra-extra toml: /tools/ra/rust-analyzer-x86_64-unknown-linux-gnu --toml", "connect rust: /tools/ra/rust-analyzer-x86_64-unknown-linux-gnu --port ${port} :0"] {
            assert!(calls.contains(&c.to_string()), "{c}: {calls:?}");
        }
        fs::remove_dir_all(&tools).unwrap();
    }
}
