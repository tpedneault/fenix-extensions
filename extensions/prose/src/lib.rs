//! Prose, on lang-kit: Harper (`harper-ls`), an offline grammar and
//! spelling server, added beside the language servers of the languages
//! `prose.languages` names, so its findings are problems and its fixes
//! are code actions. The pack finds Harper or installs the release for
//! this machine, tells it the severity, the dialect and where the
//! project's word list is, and adds a word to that list for you.
//!
//! What it knows of the project is only where it is ([`PROJECT`]); the
//! rest is the settings ([`settings`]), what's installed ([`tools`]) and
//! the list ([`words`]). A setting changed, Harper found or installed, or
//! a word added sets what Fenix has again, where it changed.

pub mod doctor;
pub mod settings;
pub mod tools;
pub mod words;

#[cfg(test)]
mod data;

// The API: in its tests, the test kit's, with a fake host.
#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{export, Command, Event, Extension};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Fenix, Host, Kit};

use settings::Settings;
use tools::Platform;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// What the pack's commands start with.
pub const PREFIX: &str = "prose";

/// The server added, as Fenix has it: its languages and what runs.
type Server = (Vec<String>, Command);

pub struct Prose {
    pub kit: Kit,
    root: String,
    settings: Settings,
    /// Where Harper is, as the doctor last found it.
    harper: Option<String>,
    /// How many times a word was added: Harper reads the list again when
    /// its settings change, so each makes them differ.
    revision: u32,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Server>,
    set_settings: Option<Value>,
}

impl Default for Prose {
    fn default() -> Prose {
        let platform = api::platform();
        Prose::new(Box::new(Fenix), PROJECT, Platform::new(&platform.os, &platform.arch))
    }
}

impl Prose {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`; its settings are read as it is made.
    pub fn new(mut host: Box<dyn Host>, root: &str, platform: Platform) -> Prose {
        let harper = tools::find(host.as_mut());
        let mut kit = Kit::with_host(PREFIX, host);
        if let Some(install) = platform.install() {
            kit.installer.add(install);
        }
        for program in doctor::programs(&platform) {
            kit.doctor.program(program);
        }
        let settings = Settings::read();
        kit.doctor.extra = vec![doctor::row(&settings, harper.as_deref())];
        Prose { kit, root: root.into(), settings, harper, revision: 0, set_server: None, set_settings: None }
    }

    /// What should be added in Fenix now: Harper, for the languages named,
    /// when it's on and found.
    fn wanted(&self) -> Option<Server> {
        let path = self.harper.as_deref()?;
        (self.settings.enabled && !self.settings.languages.is_empty()).then(|| (self.settings.languages.clone(), tools::server(path)))
    }

    /// The server and its settings set in Fenix, where they changed. The
    /// settings go after the server (Fenix takes them only for one it was
    /// given) and come away before it.
    fn apply(&mut self) {
        let want = self.wanted();
        let settings = want.as_ref().map(|_| self.settings.server_settings(self.revision));
        let host = self.kit.host.as_mut();
        if settings.is_none() && self.set_settings.is_some() {
            if let Err(why) = host.language_server_settings(tools::SERVER, None) {
                host.log(&format!("harper's settings: {why}"));
            }
            self.set_settings = None;
        }
        if want != self.set_server {
            let (languages, cmd) = want.clone().map_or((Vec::new(), None), |(l, c)| (l, Some(c)));
            if let Err(why) = host.add_language_server(tools::SERVER, &languages, cmd.as_ref()) {
                host.log(&format!("harper: {why}"));
            }
            self.set_server = want;
            self.set_settings = None;
        }
        if settings.is_some() && settings != self.set_settings {
            if let Err(why) = host.language_server_settings(tools::SERVER, settings.as_ref()) {
                host.log(&format!("harper's settings: {why}"));
            }
            self.set_settings = settings;
        }
    }

    /// What the doctor found since: Harper, where a fix put it, used.
    fn follow(&mut self) {
        let found = self.kit.doctor.found.get(tools::PROGRAM).cloned();
        if found != self.harper {
            self.harper = found;
            self.apply();
            self.kit.doctor.extra = vec![doctor::row(&self.settings, self.harper.as_deref())];
            let checks = self.kit.doctor.checks();
            self.kit.host.doctor(&checks);
        }
    }

    /// Harper looked for again, as `prose.doctor` and a setting do.
    fn check_again(&mut self) {
        self.kit.doctor.extra = vec![doctor::row(&self.settings, self.harper.as_deref())];
        let host = self.kit.host.as_mut();
        self.kit.doctor.run(host);
        self.follow();
    }

    /// The word you're on, or have selected, in the file you're in.
    fn word_here() -> Result<String, String> {
        let here = api::here().ok_or("there's no file here")?;
        let cursor = api::cursor().ok_or("there's no cursor here")?;
        if cursor.selected {
            return words::selected_word(&here.text);
        }
        words::word_at(&here.text, cursor.at.1 as usize).ok_or_else(|| "the cursor isn't on a word".to_string())
    }

    /// `prose.add-word`: the word at the cursor, in the project's list,
    /// and Harper told the list changed. Said in the message line.
    fn add_word(&mut self) -> Result<(), String> {
        let word = Prose::word_here()?;
        let path = words::file(&self.root, &self.settings.words)?;
        if words::add(&path, &word)? {
            self.revision += 1;
            self.apply();
            api::notify(&format!("Added \"{word}\" to {}", self.settings.words));
        } else {
            api::notify(&format!("\"{word}\" is in {} already", self.settings.words));
        }
        Ok(())
    }

    /// `prose.words`: the list, opened (made empty first when it's new,
    /// so there's a file to type in).
    fn open_words(&mut self) -> Result<(), String> {
        let path = words::file(&self.root, &self.settings.words)?;
        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&path, "").map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let full = format!("{}/{}", PROJECT, self.settings.words.replace('\\', "/"));
        api::host::open_file(&full, None);
        Ok(())
    }
}

impl Extension for Prose {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => match id {
                "prose.add-word" => self.add_word(),
                "prose.words" => self.open_words(),
                _ => Err(format!("no command {id} here")),
            },
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("prose."));
        self.kit.handle(&event);
        if setting {
            self.settings = Settings::read();
            self.apply();
            self.check_again();
            return;
        }
        self.follow();
    }
}

export!(Prose);

#[cfg(test)]
mod tests;
