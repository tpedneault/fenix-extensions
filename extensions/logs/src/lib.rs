//! Log files in a pane, as a WebAssembly extension: a `.log`, `.out` or
//! rotated `.log.1` (`contributes.opens`) opens in it however you open
//! it, and any file at all with `logs.open`.
//!
//! One in the project is read where it is, a few MiB at a time (`source`)
//! as it's indexed (`index`) and filtered (`filter`) in steps on a timer,
//! so a log of gigabytes opens at once and the page answers while it's
//! read; one elsewhere Fenix reads whole for it, up to its 64 MiB. Each
//! line's level and time are read from the formats logs are written in
//! (`detect`, `time`), and lines that go with the one before -- a stack
//! trace -- make one entry with it (`entry`).

pub mod detect;
pub mod entry;
pub mod filter;
pub mod hist;
pub mod index;
pub mod page;
pub mod source;
pub mod time;

use std::collections::HashMap;
use std::rc::Rc;

use fenix_extension_api::{export, host, page as pages, Event, Extension, Key, KeyPress, MouseEvent, Page};

use detect::Rules;
use page::{Action, Ask, LogPage, Saved};
use source::Source;

/// A log's page.
const PAGE: &str = "log";
/// How soon the next step of work runs: at once, after the frame.
const WORK_MS: u32 = 1;

/// Where a full path is under `/project`, if it is: read there, a part
/// at a time.
fn in_project(full: &str) -> Option<String> {
    let root = fenix_extension_api::here()?.project?.root;
    under(full, &root)
}

/// `full` under `root`, as `/project/...`: a drive's (`C:`) without case,
/// and `\\` as `/`.
fn under(full: &str, root: &str) -> Option<String> {
    let (f, r) = (full.replace('\\', "/"), root.replace('\\', "/"));
    let r = r.trim_end_matches('/');
    let same = |a: &str, b: &str| if a.as_bytes().get(1) == Some(&b':') { a.eq_ignore_ascii_case(b) } else { a == b };
    (f.len() > r.len() && f.is_char_boundary(r.len()) && same(&f[..r.len()], r) && f.as_bytes()[r.len()] == b'/').then(|| format!("/project{}", &f[r.len()..]))
}

/// What reads `path`: a file under `/project` where it is, any other
/// whole.
fn source(path: &str) -> Result<Source, String> {
    if path.starts_with("/project/") {
        return Source::path(path);
    }
    fenix_extension_api::read_file(path)
        .map(Source::bytes)
        .map_err(|e| format!("{e} (a log outside the project is read whole, up to 64 MiB: open it from its project to read it a part at a time)"))
}

/// `logs.levels`: the levels shown when a log opens.
pub fn levels(setting: Option<&str>) -> [bool; 6] {
    let Some(s) = setting.map(str::trim).filter(|s| !s.is_empty()) else { return [true; 6] };
    let mut on = [false; 6];
    for word in s.split([',', ' ']).map(str::trim).filter(|w| !w.is_empty()) {
        let level =
            detect::Level::word(word).or_else(|| (word.eq_ignore_ascii_case("other") || word.eq_ignore_ascii_case("none")).then_some(detect::Level::Other));
        if let Some(l) = level {
            on[filter::toggle_index(l)] = true;
        }
    }
    if on.iter().any(|&b| b) {
        on
    } else {
        [true; 6]
    }
}

fn setting_on(key: &str) -> bool {
    fenix_extension_api::setting(key).is_some_and(|v| matches!(v.trim(), "true" | "on" | "1" | "yes"))
}

/// The rules the settings give: `logs.patterns`, and this year for
/// syslog's dates without one.
fn rules() -> (Rules, Vec<String>) {
    let patterns: Vec<(String, String)> = fenix_extension_api::setting("logs.patterns").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let year = time::civil(fenix_extension_api::clock::now().timestamp().div_euclid(86_400)).0;
    Rules::new(&patterns, year)
}

/// `logs.poll`: how often a followed log is looked at, in milliseconds.
fn poll_ms() -> u32 {
    fenix_extension_api::setting("logs.poll").and_then(|s| s.trim().parse().ok()).unwrap_or(1000u32).clamp(200, 60_000)
}

#[derive(Default)]
struct Logs {
    pages: HashMap<u64, LogPage>,
    /// Prompts asked: for which page, and what.
    asks: HashMap<u64, (u64, Ask)>,
    rules: Option<Rc<Rules>>,
    /// The timer for the next step of work, and the one that polls.
    work: Option<u64>,
    poll: Option<(u64, u32)>,
}

impl Logs {
    fn rules(&mut self) -> Rc<Rules> {
        if let Some(r) = &self.rules {
            return r.clone();
        }
        let (r, bad) = rules();
        for b in bad {
            fenix_extension_api::warn!("logs.patterns: {b}");
        }
        let r = Rc::new(r);
        self.rules = Some(r.clone());
        r
    }

    fn open(&mut self, full: &str) {
        // One open already goes, its place coming here.
        let mut kept: Option<Saved> = None;
        let open: Vec<u64> = self.pages.iter().filter(|(_, p)| p.full == full).map(|(&id, _)| id).collect();
        for id in open {
            if let Some(p) = self.pages.remove(&id) {
                kept = Some(p.saved());
            }
            pages::close(id);
        }
        let path = in_project(full).unwrap_or_else(|| full.to_string());
        let mut p = LogPage::new(full, &path, source(&path), self.rules());
        p.set_levels(levels(fenix_extension_api::setting("logs.levels").as_deref()));
        p.follow = setting_on("logs.follow");
        if let Some(s) = &kept {
            p.restore(s);
        }
        let id = pages::open(PAGE, &format!("{} (log)", p.name()));
        self.pages.insert(id, p);
        self.schedule();
    }

    /// The timers as the pages need them: a step of work while any is
    /// busy, a poll while any follows its file.
    fn schedule(&mut self) {
        if self.work.is_none() && self.pages.values().any(LogPage::busy) {
            self.work = Some(host::set_timer(WORK_MS, false));
        }
        let following = self.pages.values().any(|p| p.follow);
        let ms = poll_ms();
        match self.poll {
            Some((t, every)) if !following || every != ms => {
                host::cancel_timer(t);
                self.poll = None;
            }
            _ => {}
        }
        if following && self.poll.is_none() {
            self.poll = Some((host::set_timer(ms, true), ms));
        }
    }

    /// A step for each busy page, drawn again.
    fn work(&mut self) {
        self.work = None;
        for (&id, p) in &mut self.pages {
            if p.busy() {
                p.work();
                pages::refresh(id);
            }
        }
        self.schedule();
    }

    /// The logs followed, looked at again.
    fn poll(&mut self) {
        for (&id, p) in &mut self.pages {
            if !p.follow {
                continue;
            }
            // One read whole is read whole again; one in the project, its
            // length looked at.
            let again = p.src.as_ref().is_ok_and(|s| !s.in_place()).then(|| fenix_extension_api::read_file(&p.full).ok()).flatten();
            if p.poll(again) {
                pages::refresh(id);
            }
        }
        self.schedule();
    }

    fn act(&mut self, page: u64, action: Action) {
        match action {
            Action::Nothing => {}
            Action::Redraw => pages::refresh(page),
            Action::Copy(text, said) => {
                fenix_extension_api::copy(&text);
                fenix_extension_api::notify(&said);
                pages::refresh(page);
            }
            Action::Say(said) => {
                fenix_extension_api::notify(&said);
                pages::refresh(page);
            }
            Action::Ask(ask, label, initial) => {
                let id = fenix_extension_api::prompt(&label, &initial);
                self.asks.insert(id, (page, ask));
            }
            Action::Open(line) => {
                if let Some(p) = self.pages.get(&page) {
                    // A file it opens itself: Fenix opens its text, at the
                    // line. `SPC f o` there comes back.
                    host::open_file(&p.full, Some(line));
                }
            }
            Action::Reload => {
                if let Some(p) = self.pages.get_mut(&page) {
                    let read = source(&p.path);
                    p.reload(read);
                    pages::refresh(page);
                }
            }
            Action::Follow(on) => {
                fenix_extension_api::notify(if on { "Following the log as it grows: F stops" } else { "Not following the log" });
                pages::refresh(page);
            }
        }
        self.schedule();
    }
}

impl Extension for Logs {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "logs.open" => {
                let full = fenix_extension_api::here().and_then(|h| h.path).ok_or("No file here to open as a log")?;
                self.open(&full);
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Opened(path) => self.open(&path),
            Event::Timer(id) if Some(id) == self.work => self.work(),
            Event::Timer(id) if self.poll.is_some_and(|(t, _)| t == id) => self.poll(),
            Event::Prompted((id, typed)) => {
                let Some((page, ask)) = self.asks.remove(&id) else { return };
                let (Some(typed), Some(p)) = (typed, self.pages.get_mut(&page)) else { return };
                let action = p.answer(ask, &typed);
                self.act(page, action);
            }
            Event::SettingChanged(key) => {
                if key == "logs.patterns" {
                    // Levels are read again with the new patterns.
                    self.rules = None;
                    let r = self.rules();
                    let ids: Vec<u64> = self.pages.keys().copied().collect();
                    for id in ids {
                        if let Some(p) = self.pages.get_mut(&id) {
                            let read = source(&p.path);
                            let saved = p.saved();
                            let mut fresh = LogPage::new(&p.full, &p.path, read, r.clone());
                            fresh.restore(&saved);
                            *p = fresh;
                            pages::refresh(id);
                        }
                    }
                }
                self.schedule();
            }
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let Some(p) = self.pages.get(&page) else {
            return Page::new().line([pages::muted("Nothing here: the log is closed")]);
        };
        p.render(width as usize, height as usize).to_page().more_keys(true).mouse(true)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.key(key.into());
        self.act(page, action);
    }

    /// PageUp and PageDown, Ctrl+D, U, F and B.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let (Some(p), Some(press)) = (self.pages.get_mut(&page), fenix_ui_kit::KeyPress::from_api(&key)) else { return };
        let action = p.press(press);
        self.act(page, action);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let Some(p) = self.pages.get_mut(&page) else { return };
        let action = p.mouse(&fenix_ui_kit::Mouse::from(&event));
        self.act(page, action);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        serde_json::to_string(&self.pages.get(&page)?.saved()).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != PAGE {
            return false;
        }
        let Ok(saved) = serde_json::from_str::<Saved>(state) else { return false };
        // One in the project it reads from there, if it's the same file;
        // one elsewhere Fenix gives it again with its page.
        let path = if saved.path.starts_with("/project/") {
            if in_project(&saved.full).as_deref() != Some(saved.path.as_str()) {
                return false;
            }
            saved.path.clone()
        } else {
            saved.full.clone()
        };
        let Ok(read) = source(&path) else { return false };
        let mut p = LogPage::new(&saved.full, &path, Ok(read), self.rules());
        p.restore(&saved);
        self.pages.insert(page, p);
        self.schedule();
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
        self.asks.retain(|_, (p, _)| *p != page);
        self.schedule();
    }
}

export!(Logs);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_in_the_project_is_read_there() {
        assert_eq!(under("/home/me/p/logs/a.log", "/home/me/p").as_deref(), Some("/project/logs/a.log"));
        assert_eq!(under("C:\\Work\\P\\a.log", "c:/work/p/").as_deref(), Some("/project/a.log"));
        assert_eq!(under("/home/me/pq/a.log", "/home/me/p"), None, "a folder named alike isn't it");
    }

    /// Each sample's entries' levels, and whether each has a time.
    fn sample(text: &str) -> Vec<(detect::Level, bool)> {
        let src = Source::bytes(text.as_bytes().to_vec());
        let mut i = index::Index::new(Rc::new(Rules { custom: Vec::new(), year: 2024 }), src.len());
        while i.step(&src, 1 << 20) {}
        let mut out = Vec::new();
        for n in 0..i.chunk_count() {
            let v = i.chunk(n).unwrap();
            entry::scan(&src, v.chunk.offset..v.chunk.end, v.eof, i.grouping(), &i.rules, false, |e, _| out.push((e.level, e.time.is_some())));
        }
        out
    }

    #[test]
    fn every_sample_s_levels_and_times() {
        use detect::Level::*;
        let levels = |text: &str| sample(text).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(levels(include_str!("samples/java.log")), [Info, Error, Warn, Debug]);
        assert_eq!(levels(include_str!("samples/python.log")), [Info, Error, Warn, Warn, Fatal, Debug]);
        assert_eq!(levels(include_str!("samples/rust.log")), [Info, Warn, Error, Info, Debug, Trace, Fatal]);
        assert_eq!(levels(include_str!("samples/app.jsonl")), [Info, Error, Warn, Debug, Fatal, Trace]);
        assert_eq!(levels(include_str!("samples/logfmt.log")), [Info, Warn, Error, Debug]);
        assert_eq!(levels(include_str!("samples/syslog.log")), [Other, Error, Error, Warn, Other]);
        assert_eq!(levels(include_str!("samples/journal.json")), [Info, Error, Warn]);
        assert_eq!(levels(include_str!("samples/access.log")), [Info, Warn, Error]);
        assert_eq!(levels(include_str!("samples/journal.export")), [Info, Error]);
        assert_eq!(levels(include_str!("samples/other.log")), [Info, Error, Info, Error, Info, Error, Error, Info]);
        for (name, text) in [
            ("java", include_str!("samples/java.log")),
            ("app.jsonl", include_str!("samples/app.jsonl")),
            ("logfmt", include_str!("samples/logfmt.log")),
            ("syslog", include_str!("samples/syslog.log")),
            ("journal", include_str!("samples/journal.json")),
            ("access", include_str!("samples/access.log")),
            ("journal export", include_str!("samples/journal.export")),
        ] {
            assert!(sample(text).iter().all(|(_, t)| *t), "{name}: every entry has its time");
        }
    }

    #[test]
    fn the_levels_setting_names_levels() {
        assert_eq!(levels(None), [true; 6]);
        assert_eq!(levels(Some("error, warn")), [true, true, false, false, false, false]);
        assert_eq!(levels(Some("fatal,info other")), [true, false, true, false, false, true]);
        assert_eq!(levels(Some("nonsense")), [true; 6], "nothing it knows: everything");
    }
}
