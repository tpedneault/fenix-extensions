//! Docker, Podman and wslc in Fenix, as a WebAssembly extension.
//!
//! `state` holds what the engines said and what each page shows; here it
//! meets the host: events become calls on it, and the `Effect`s it gives
//! back are carried out.

pub mod engine;
pub mod model;
pub mod state;

use std::collections::HashMap;

use fenix_extension_api::{export, host, page, Command, Event, Extension, Key, Page};

use engine::Engine;
use state::{Asking, Effect, State};

/// How often the containers are asked for again while a page shows them.
const TICK_MILLIS: u32 = 2000;

#[derive(Default)]
struct Docker {
    state: State,
    /// Prompts open, and what each is for.
    prompts: HashMap<u64, Asking>,
}

fn pinned() -> Option<Engine> {
    fenix_extension_api::setting("containers.engine").and_then(|e| Engine::by_name(&e))
}

impl Docker {
    fn apply(&mut self, effects: Vec<Effect>) {
        let mut queue = std::collections::VecDeque::from(effects);
        let mut refreshed = Vec::new();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Spawn { job, engine, args, cwd } => {
                    let mut cmd = Command::new(engine.program()).args(args);
                    if let Some(dir) = cwd {
                        cmd = cmd.cwd(&dir);
                    }
                    match cmd.spawn() {
                        Ok(process) => self.state.started(process, job),
                        Err(why) => {
                            fenix_extension_api::warn!("{} {:?}: {why}", engine.program(), cmd.args);
                            queue.extend(self.state.unspawnable(job));
                        }
                    }
                }
                Effect::Kill(process) => host::kill(process),
                Effect::OpenPage { kind, title } => {
                    let base = kind.split_once(':').map_or(kind.as_str(), |(b, _)| b);
                    let id = page::open(base, &title);
                    queue.extend(self.state.open(id, &kind, &title));
                }
                Effect::Refresh(p) => {
                    if !refreshed.contains(&p) {
                        refreshed.push(p);
                    }
                }
                Effect::Close(p) => {
                    page::close(p);
                    queue.extend(self.state.closed(p));
                }
                Effect::Prompt { what, label, initial } => {
                    let id = fenix_extension_api::prompt(&label, &initial);
                    self.prompts.insert(id, what);
                }
                Effect::Notify(text) => fenix_extension_api::notify(&text),
                Effect::OpenUrl(url) => {
                    if let Err(why) = fenix_extension_api::open_url(&url) {
                        fenix_extension_api::notify(&format!("Couldn't open {url}: {why}"));
                    }
                }
                Effect::Copy(text) => fenix_extension_api::copy(&text),
                Effect::Terminal { engine, args, title, cwd } => {
                    let mut cmd = Command::new(engine.program()).args(args);
                    if let Some(dir) = cwd {
                        cmd = cmd.cwd(&dir);
                    }
                    if let Err(why) = cmd.run_in_terminal(&title) {
                        fenix_extension_api::notify(&format!("Couldn't open a terminal: {why}"));
                    }
                }
                Effect::OpenText { title, text, language } => fenix_extension_api::open_text(&title, &text, Some(&language)),
                Effect::Status(text) => fenix_extension_api::status(text.as_deref()),
            }
        }
        for p in refreshed {
            page::refresh(p);
        }
    }
}

impl Extension for Docker {
    fn activate(&mut self) -> Result<(), String> {
        let effects = self.state.activate(pinned());
        self.apply(effects);
        host::set_timer(TICK_MILLIS, true);
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "containers.open" => {
                self.apply(vec![Effect::OpenPage { kind: "main".into(), title: "Containers".into() }]);
                Ok(())
            }
            "containers.performance" => {
                self.apply(vec![Effect::OpenPage { kind: "performance".into(), title: "Performance".into() }]);
                Ok(())
            }
            "containers.build" => {
                let effects = self.state.build();
                self.apply(effects);
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        let effects = match event {
            Event::Timer(_) => self.state.tick(),
            Event::ProcessOutput(o) => self.state.output(o.process, o.stderr, &o.line),
            Event::ProcessExit((process, code)) => self.state.exited(process, code),
            Event::Prompted((id, text)) => match self.prompts.remove(&id) {
                Some(what) => self.state.prompted(what, text),
                None => Vec::new(),
            },
            Event::PageShown((p, shown)) => self.state.shown(p, shown),
            Event::SettingChanged(key) if key == "containers.engine" => self.state.pin(pinned()),
            _ => Vec::new(),
        };
        self.apply(effects);
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        self.state.render(page, width)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let effects = self.state.key(page, key);
        self.apply(effects);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.state.save(page)
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        match self.state.restore(page, kind, state) {
            Some(effects) => {
                self.apply(effects);
                true
            }
            None => false,
        }
    }
}

export!(Docker);
