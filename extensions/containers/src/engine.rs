//! The engines: Docker, Podman and wslc, each a CLI with its own words
//! for the same things and its own gaps. An `Engine` says what it can do
//! (`Caps`) and gives the arguments for each thing the page asks of it;
//! the extension runs them through the host (`permissions.process`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Engine {
    Docker,
    Podman,
    Wslc,
}

/// What an engine can do: a tab or action it can't says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub compose: bool,
    pub volumes: bool,
    pub networks: bool,
    /// `stats` streams; otherwise it's asked again on a timer.
    pub stats_stream: bool,
    pub history: bool,
    /// Answers in JSON; otherwise in columns.
    pub json: bool,
}

/// Where an engine is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// Being asked.
    Probing,
    /// Not on this machine.
    Missing,
    /// Installed, not running, and why.
    Down(String),
    Running { version: String },
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::Docker, Engine::Podman, Engine::Wslc];

    pub fn program(self) -> &'static str {
        match self {
            Engine::Docker => "docker",
            Engine::Podman => "podman",
            Engine::Wslc => "wslc",
        }
    }

    pub fn by_name(name: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.program().eq_ignore_ascii_case(name.trim()))
    }

    pub fn caps(self) -> Caps {
        match self {
            Engine::Docker | Engine::Podman => Caps { compose: true, volumes: true, networks: true, stats_stream: true, history: true, json: true },
            // wslc 2.9 (preview): containers, images, build, logs, exec, stats.
            Engine::Wslc => Caps { compose: false, volumes: false, networks: false, stats_stream: false, history: false, json: false },
        }
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// Asks whether it's up and which version.
    pub fn probe(self) -> Vec<String> {
        match self {
            Engine::Docker | Engine::Podman => Self::args(&["version", "--format", "{{.Server.Version}}"]),
            Engine::Wslc => Self::args(&["version"]),
        }
    }

    /// What its probe said: `code` its exit, `out` and `err` its lines.
    pub fn read_probe(self, code: Option<i32>, out: &str, err: &str) -> Up {
        let version = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
        match (self, code) {
            (_, Some(0)) if !version.is_empty() => Up::Running { version: version.trim_start_matches("wslc ").trim_start_matches("version ").to_string() },
            (Engine::Docker, _) => Up::Down(if err.contains("dockerDesktop") || err.contains("docker_engine") || err.contains("npipe") { "Docker Desktop isn't running".into() } else { first_line(err, "Docker's engine isn't running") }),
            (Engine::Podman, _) => Up::Down(if err.contains("machine") || err.contains("Cannot connect") { "Podman's machine isn't running".into() } else { first_line(err, "Podman isn't answering") }),
            (Engine::Wslc, _) => Up::Down(first_line(err, "wslc needs WSL 2.9.3 or later (wsl --update --pre-release)")),
        }
    }

    /// How to start it when it's down: (what it says, the arguments).
    pub fn starter(self) -> Option<(&'static str, Vec<String>)> {
        match self {
            Engine::Docker => Some(("start Docker Desktop", Self::args(&["desktop", "start"]))),
            Engine::Podman => Some(("start its machine (podman machine start)", Self::args(&["machine", "start"]))),
            Engine::Wslc => None,
        }
    }

    pub fn containers(self) -> Vec<String> {
        match self {
            Engine::Wslc => Self::args(&["container", "list", "--all"]),
            _ => Self::args(&["ps", "-a", "--no-trunc", "--format", "json"]),
        }
    }

    pub fn images(self) -> Vec<String> {
        match self {
            Engine::Wslc => Self::args(&["image", "list"]),
            _ => Self::args(&["images", "--format", "json"]),
        }
    }

    pub fn volumes(self) -> Vec<String> {
        Self::args(&["volume", "ls", "--format", "json"])
    }

    pub fn networks(self) -> Vec<String> {
        Self::args(&["network", "ls", "--format", "json"])
    }

    pub fn stacks(self) -> Vec<String> {
        Self::args(&["compose", "ls", "--all", "--format", "json"])
    }

    /// Load, streamed (or once, for an engine that doesn't stream).
    pub fn stats(self) -> Vec<String> {
        match self {
            Engine::Wslc => Self::args(&["stats"]),
            _ => Self::args(&["stats", "--format", "json"]),
        }
    }

    pub fn disk_use(self) -> Vec<String> {
        Self::args(&["system", "df", "--format", "json"])
    }

    /// A container's log, followed: its last `tail` lines, then what comes.
    pub fn logs(self, id: &str, tail: u32) -> Vec<String> {
        let tail = tail.to_string();
        match self {
            Engine::Wslc => Self::args(&["container", "logs", "-f", id]),
            _ => Self::args(&["logs", "-f", "--timestamps", "--tail", &tail, id]),
        }
    }

    pub fn inspect(self, id: &str) -> Vec<String> {
        Self::args(&["container", "inspect", id])
    }

    pub fn inspect_image(self, id: &str) -> Vec<String> {
        Self::args(&["image", "inspect", id])
    }

    pub fn history(self, id: &str) -> Vec<String> {
        Self::args(&["history", "--format", "json", id])
    }

    /// A container action by verb: start, stop, restart, pause, unpause,
    /// kill, rm.
    pub fn act(self, verb: &str, id: &str) -> Vec<String> {
        match (self, verb) {
            (Engine::Wslc, _) => Self::args(&["container", verb, id]),
            (_, "rm") => Self::args(&["rm", id]),
            _ => Self::args(&[verb, id]),
        }
    }

    /// A shell in a container: bash when it has it, else sh.
    pub fn shell(self, id: &str) -> Vec<String> {
        Self::args(&["exec", "-it", id, "sh", "-c", "command -v bash >/dev/null 2>&1 && exec bash || exec sh"])
    }

    /// An action on an image: rm, pull.
    pub fn image_act(self, verb: &str, reference: &str) -> Vec<String> {
        match self {
            Engine::Wslc => Self::args(&["image", verb, reference]),
            _ => Self::args(&["image", verb, reference]),
        }
    }

    /// Removes what nothing uses, of `kind` (image, volume, network,
    /// container).
    pub fn prune(self, kind: &str) -> Vec<String> {
        Self::args(&[kind, "prune", "-f"])
    }

    pub fn remove(self, kind: &str, id: &str) -> Vec<String> {
        Self::args(&[kind, "rm", id])
    }

    /// A Compose stack's `verb` (up, down, restart, pull, build, logs) by
    /// its file, or by its name when that's all there is.
    pub fn compose(self, verb: &str, name: &str, file: Option<&str>) -> Vec<String> {
        let mut args = Self::args(&["compose"]);
        match file {
            Some(f) => args.extend(Self::args(&["-f", f])),
            None => args.extend(Self::args(&["-p", name])),
        }
        args.push(verb.to_string());
        match verb {
            "up" => args.push("-d".into()),
            "logs" => args.extend(Self::args(&["-f", "--timestamps", "--tail", "200"])),
            _ => {}
        }
        args
    }
}

fn first_line(err: &str, otherwise: &str) -> String {
    err.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.trim_start_matches("Error: ").to_string()).unwrap_or_else(|| otherwise.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_engine_says_its_words_and_gaps() {
        assert_eq!(Engine::Docker.containers(), ["ps", "-a", "--no-trunc", "--format", "json"]);
        assert_eq!(Engine::Wslc.containers(), ["container", "list", "--all"]);
        assert_eq!(Engine::Wslc.act("stop", "web"), ["container", "stop", "web"]);
        assert_eq!(Engine::Podman.act("rm", "db"), ["rm", "db"]);
        assert!(!Engine::Wslc.caps().compose && Engine::Podman.caps().compose);
        assert_eq!(Engine::Docker.compose("up", "shop", Some("compose.yaml")), ["compose", "-f", "compose.yaml", "up", "-d"]);
        assert_eq!(Engine::Docker.compose("down", "monitoring", None), ["compose", "-p", "monitoring", "down"]);
        assert_eq!(Engine::by_name(" Podman "), Some(Engine::Podman));
    }

    /// The probe says up, down and why, in each engine's own words.
    #[test]
    fn a_probe_is_read() {
        assert_eq!(Engine::Docker.read_probe(Some(0), "29.7.2\n", ""), Up::Running { version: "29.7.2".into() });
        let desktop = "failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine; check if the path is correct and if the daemon is running";
        assert_eq!(Engine::Docker.read_probe(Some(1), "", desktop), Up::Down("Docker Desktop isn't running".into()));
        assert_eq!(Engine::Podman.read_probe(Some(125), "", "Cannot connect to Podman. Please verify your connection to the Linux system"), Up::Down("Podman's machine isn't running".into()));
        assert_eq!(Engine::Wslc.read_probe(Some(0), "wslc 2.9.4\n", ""), Up::Running { version: "2.9.4".into() });
        assert!(matches!(Engine::Wslc.read_probe(Some(1), "", ""), Up::Down(w) if w.contains("2.9.3")));
    }
}
