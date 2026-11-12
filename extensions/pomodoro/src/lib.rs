//! Pomodoro in Fenix: work rounds and breaks, counted down on the
//! modeline, as a WebAssembly extension. `round` is the timer and
//! `history` what's kept, in plain Rust; here they're tied to Fenix's
//! timers, the modeline, a page, a chime, where you are, and the
//! agenda's clock.

pub mod focus;
pub mod history;
mod keys;
pub mod page;
pub mod round;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate};
use fenix_extension_api::{self as api, clock, export, host, Event, Extension, Focus, Key, Notification, Page, PickerItem};

use history::{Kept, Round, Task};
use round::{Ended, Lengths, Phase, Run};

/// The timer and the rounds done, kept between runs.
const KEPT: &str = "/data/pomodoro.json";
/// How often the countdown is drawn again, in ms.
const TICK: u32 = 1_000;
/// Played as a round runs out (`pomodoro.sound`).
const CHIME: &str = "sounds/chime.wav";

/// What a service call or a picker was for.
enum Asked {
    /// `agenda.timer`, as a work round starts: is its clock on already?
    Timer,
    /// `agenda.clock_in` on this task.
    ClockIn(Task),
    /// `agenda.tasks`, to pick from.
    Tasks,
    /// The picker of those.
    Pick(Vec<Task>),
}

#[derive(Default)]
struct Pomodoro {
    kept: Kept,
    lengths: Lengths,
    auto_breaks: bool,
    agenda: bool,
    sound: bool,
    unfocused: bool,
    /// The "unfocused" warning, once a work round.
    watch: focus::Watch,
    /// The countdown's timer, while a round runs.
    ticker: Option<u64>,
    /// The page, if it's open, and whether it's in view.
    page: Option<u64>,
    shown: bool,
    asked: HashMap<u64, Asked>,
}

/// Now, in ms since 1970.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Today, where you are.
fn today() -> NaiveDate {
    clock::now().date_naive()
}

impl Pomodoro {
    /// The settings, read again.
    fn settings(&mut self) {
        let minutes = |key: &str, default: u32| api::setting(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        let d = Lengths::default();
        self.lengths = Lengths {
            work: minutes("pomodoro.work", d.work),
            short_break: minutes("pomodoro.short_break", d.short_break),
            long_break: minutes("pomodoro.long_break", d.long_break),
            long_every: minutes("pomodoro.long_every", d.long_every),
        };
        let on = |key: &str| api::setting(key).as_deref() != Some("false");
        self.auto_breaks = on("pomodoro.auto_breaks");
        self.agenda = on("pomodoro.agenda");
        self.sound = on("pomodoro.sound");
        self.unfocused = api::setting("pomodoro.unfocused").as_deref() == Some("true");
    }

    fn save(&mut self) {
        self.kept.prune(today());
        if let Err(e) = std::fs::write(KEPT, self.kept.save()) {
            api::warn!("{KEPT}: {e}");
        }
    }

    /// The modeline, the page, and the countdown's timer, as the round is.
    fn refresh(&mut self) {
        let timer = &self.kept.timer;
        let icon = if timer.phase.is_break() { "\u{2615}" } else { "\u{1f345}" };
        let text = match (timer.run, timer.left(now())) {
            (Run::Running { .. }, Some(left)) => Some(format!("{icon} {}", round::clock(left))),
            (Run::Paused { .. }, Some(left)) => Some(format!("{icon} {} paused", round::clock(left))),
            // A break waiting for you to take it.
            (Run::Idle, _) if timer.phase.is_break() => Some(format!("{icon} break")),
            _ => None,
        };
        api::status(text.as_deref());
        match (timer.running(), self.ticker) {
            (true, None) => self.ticker = Some(host::set_timer(TICK, true)),
            (false, Some(ticker)) => {
                host::cancel_timer(ticker);
                self.ticker = None;
            }
            _ => {}
        }
        if let (Some(page), true) = (self.page, self.shown) {
            api::page::refresh(page);
        }
    }

    /// Rounds that ran out, seen to; `quietly` for those that did while
    /// Fenix was closed.
    fn catch_up(&mut self, quietly: bool) {
        let mut any = false;
        while let Some(ended) = self.kept.timer.tick(now(), &self.lengths, self.auto_breaks) {
            self.ended(ended, quietly);
            any = true;
        }
        if any {
            self.save();
        }
    }

    /// A round over: kept if it was work that ran out, and said.
    fn ended(&mut self, e: Ended, quietly: bool) {
        if e.phase == Phase::Work {
            if e.counted {
                let at = DateTime::from_timestamp_millis(e.at as i64).map(clock::local).unwrap_or_else(clock::now);
                self.kept.rounds.push(Round { ended: at, minutes: (e.length / 60_000) as u32, task: self.kept.on.clone() });
            }
            self.clock_out();
            self.kept.on = None;
            self.watch.next_round();
        }
        if quietly {
            return;
        }
        // A round that ran out, not one skipped.
        if e.counted && self.sound {
            if let Err(why) = api::play_sound(CHIME) {
                api::warn!("{CHIME}: {why}");
            }
        }
        let next = format!("{} ({} min)", e.next.name(), self.lengths.minutes(e.next));
        let (summary, body) = end_words(&e, &next, self.kept.timer.round(), &keys::start());
        if e.counted {
            // It ran out while you may be elsewhere: a notification, the
            // next replacing it.
            round_note(&e, summary, body, self.kept.timer.round()).show();
        } else {
            // Skipped: you're here, and the message line will do.
            api::notify(&format!("{summary} -- {body}"));
        }
    }

    fn start(&mut self) {
        if !self.kept.timer.start(now(), &self.lengths) {
            return api::notify(&format!("The round's already running -- {} pauses it", keys::keys_for("pomodoro.pause", "SPC o p p")));
        }
        // The round's end that asked for this is done with.
        api::notify_dismiss(ROUND_TAG);
        if self.kept.timer.phase == Phase::Work {
            self.clock_in();
        }
        self.save();
    }

    fn pause(&mut self) {
        if !self.kept.timer.pause(now()) {
            return api::notify(&format!("No round is running -- {} starts one", keys::start()));
        }
        self.clock_out();
        self.save();
    }

    fn working(&self) -> bool {
        self.kept.timer.running() && self.kept.timer.phase == Phase::Work
    }

    /// You went somewhere else: a page away from your work, in a work
    /// round, warned once (`pomodoro.unfocused`).
    fn moved(&mut self, to: &Focus) {
        if !self.unfocused || !self.watch.moved(to, self.working()) {
            return;
        }
        let left = self.kept.timer.left(now()).map(round::clock).unwrap_or_default();
        api::notify(&focus::warning(self.kept.timer.round(), &left));
    }

    /// Start, or pause: the page's `s`.
    fn toggle(&mut self) {
        if self.kept.timer.running() {
            self.pause();
        } else {
            self.start();
        }
    }

    fn skip(&mut self) {
        let ended = self.kept.timer.skip(now(), &self.lengths, self.auto_breaks);
        self.ended(ended, false);
        self.save();
    }

    fn reset(&mut self) {
        self.clock_out();
        self.kept.on = None;
        self.kept.timer.reset();
        self.watch.next_round();
        api::notify(&format!("Back to round 1 -- {} starts it", keys::start()));
        self.save();
    }

    // -- The agenda's clock -----------------------------------------------

    /// A work round under way: on the agenda's clock if it's running,
    /// else on the task picked for it (`SPC o p t`).
    fn clock_in(&mut self) {
        if self.agenda {
            self.asked.insert(api::call_service("agenda.timer", "null"), Asked::Timer);
        }
    }

    /// The agenda's clock stopped, if this started it.
    fn clock_out(&mut self) {
        if std::mem::take(&mut self.kept.clocked) {
            api::call_service("agenda.clock_out", "null");
        }
    }

    fn pick_task(&mut self) {
        let payload = serde_json::json!({ "status": ["inProgress", "todo"] }).to_string();
        self.asked.insert(api::call_service("agenda.tasks", &payload), Asked::Tasks);
    }

    fn replied(&mut self, asked: Asked, reply: Result<String, String>) {
        let working = self.working();
        match (asked, reply) {
            (Asked::Timer, Ok(text)) if working => {
                let theirs = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string));
                match (theirs, self.kept.task.clone()) {
                    // Its clock is on already: the round is for that.
                    (Some(title), _) => self.kept.on = Some(title),
                    (None, Some(task)) => {
                        let payload = serde_json::json!({ "task_id": task.id }).to_string();
                        self.asked.insert(api::call_service("agenda.clock_in", &payload), Asked::ClockIn(task));
                    }
                    (None, None) => {}
                }
            }
            (Asked::ClockIn(task), Ok(_)) => {
                self.kept.on = Some(task.title);
                self.kept.clocked = true;
                // Paused or ended while it was asked.
                if !working {
                    self.clock_out();
                    self.kept.on = None;
                }
            }
            (Asked::ClockIn(task), Err(why)) => {
                api::warn!("agenda.clock_in: {why}");
                self.kept.task = None;
                api::notify(&format!("Couldn't clock into \"{}\" -- {} picks another", task.title, keys::keys_for("pomodoro.task", "SPC o p t")));
            }
            (Asked::Tasks, Ok(text)) => {
                let tasks: Vec<Task> = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("tasks").cloned())
                    .and_then(|t| serde_json::from_value(t).ok())
                    .unwrap_or_default();
                if tasks.is_empty() {
                    return api::notify(&format!("No tasks to do in the agenda -- {} adds one", keys::keys_for("agenda.new_task", "SPC A n")));
                }
                let mut items = vec![PickerItem::new("None -- don't clock in", "none")];
                items.extend(tasks.iter().map(|t| PickerItem::new(t.title.clone(), t.id.to_string())));
                self.asked.insert(api::pick("Work rounds clock into", &items), Asked::Pick(tasks));
            }
            (Asked::Tasks, Err(why)) => {
                api::warn!("agenda.tasks: {why}");
                api::notify("The agenda isn't there to clock into -- it's the Agenda extension's");
            }
            // The agenda isn't there, or said no: the round goes on without it.
            (Asked::Timer, Err(why)) => api::info!("agenda.timer: {why}"),
            _ => {}
        }
        self.save();
    }

    fn picked(&mut self, tasks: Vec<Task>, value: &str) {
        let task = tasks.into_iter().find(|t| t.id.to_string() == value);
        let working = self.working();
        self.kept.task = task.clone();
        match task {
            None => {
                if self.kept.clocked {
                    self.clock_out();
                    self.kept.on = None;
                }
                api::notify("Work rounds won't clock into the agenda");
            }
            // On it now, if a round is under way: the agenda switches its clock.
            Some(task) if working && self.agenda => {
                let payload = serde_json::json!({ "task_id": task.id }).to_string();
                self.asked.insert(api::call_service("agenda.clock_in", &payload), Asked::ClockIn(task));
            }
            Some(task) if self.agenda => api::notify(&format!("Work rounds clock into \"{}\"", task.title)),
            Some(task) => api::notify(&format!("Work rounds are for \"{}\" -- pomodoro.agenda is off, so they won't clock in", task.title)),
        }
        self.save();
    }
}

/// The tag a round's end goes by: the next end replaces it.
const ROUND_TAG: &str = "pomodoro.round";

/// What a round's end says: a summary, and what comes next. `next` is
/// the next round's name and length, `next_round` its number, `start` the
/// keys that start a round as the key scheme in use has them.
fn end_words(e: &Ended, next: &str, next_round: u32, start: &str) -> (String, String) {
    match (e.phase, e.counted, e.started_next) {
        (Phase::Work, true, true) => (format!("Round {} done", e.round), format!("{next} has started")),
        (Phase::Work, true, false) => (format!("Round {} done", e.round), format!("{next} next: {start} starts it")),
        (Phase::Work, false, true) => (format!("Skipped round {}", e.round), format!("{next} has started")),
        (Phase::Work, false, false) => (format!("Skipped round {}", e.round), format!("{next} next: {start} starts it")),
        (_, true, _) => ("Break's over".to_string(), format!("{start} starts round {next_round}")),
        (_, false, _) => ("Skipped the break".to_string(), format!("{start} starts round {next_round}")),
    }
}

/// A round that ran out, as a notification: tagged, so the next end
/// replaces it; Enter on it starts what's next, unless that started by
/// itself.
fn round_note(e: &Ended, summary: String, body: String, next_round: u32) -> Notification {
    let note = Notification::new(summary).body(body).tag(ROUND_TAG);
    match (e.started_next, e.phase) {
        (true, _) => note,
        (false, Phase::Work) => note.action("Start it", "pomodoro.start"),
        (false, _) => note.action(format!("Start round {next_round}"), "pomodoro.start"),
    }
}

/// What `pomodoro.state` answers: `{"phase": "work"}`, `"break"` or
/// `"off"` when no round's running.
fn state_json(running: bool, phase: Phase) -> String {
    let phase = match (running, phase) {
        (false, _) => "off",
        (true, Phase::Work) => "work",
        (true, _) => "break",
    };
    format!(r#"{{"phase": "{phase}"}}"#)
}

impl Extension for Pomodoro {
    fn activate(&mut self) -> Result<(), String> {
        self.settings();
        self.kept = std::fs::read_to_string(KEPT).map(|text| Kept::load(&text)).unwrap_or_default();
        self.catch_up(true);
        self.refresh();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            // One open already is brought here.
            "pomodoro.open" => match self.page {
                Some(page) => api::page::focus(page),
                None => {
                    self.page = Some(api::page::open("main", "Pomodoro"));
                    self.shown = true;
                }
            },
            "pomodoro.start" => self.start(),
            "pomodoro.pause" => self.pause(),
            "pomodoro.skip" => self.skip(),
            "pomodoro.reset" => self.reset(),
            "pomodoro.task" => self.pick_task(),
            other => return Err(format!("no command {other}")),
        }
        self.refresh();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timer(id) if Some(id) == self.ticker => self.catch_up(false),
            Event::SettingChanged(_) => self.settings(),
            Event::PageShown((page, shown)) if Some(page) == self.page => self.shown = shown,
            Event::ServiceReply((id, reply)) => match self.asked.remove(&id) {
                Some(asked) => self.replied(asked, reply),
                None => return,
            },
            Event::Picked((id, Some(value))) => match self.asked.remove(&id) {
                Some(Asked::Pick(tasks)) => self.picked(tasks, &value),
                _ => return,
            },
            Event::Picked((id, None)) => {
                self.asked.remove(&id);
                return;
            }
            _ => return,
        }
        self.refresh();
    }

    fn page(&mut self, _page: u64, width: u32, _height: u32) -> Page {
        page::render(&self.kept, &self.lengths, now(), today(), width)
    }

    fn page_key(&mut self, _page: u64, key: Key) {
        match key {
            Key::Char('s') => self.toggle(),
            Key::Char('n') => self.skip(),
            Key::Char('r') => self.reset(),
            Key::Char('t') => self.pick_task(),
            _ => return,
        }
        self.refresh();
    }

    fn page_state(&mut self, _page: u64) -> Option<String> {
        Some(String::new())
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind == "main" {
            self.page = Some(page);
            self.shown = true;
        }
        kind == "main"
    }

    /// `pomodoro.state`: what the timer's doing, for other extensions --
    /// the pets keep still in a work round and play in the break.
    fn serve(&mut self, _call: u64, name: &str, _payload: &str) -> Option<Result<String, String>> {
        match name {
            "pomodoro.state" => Some(Ok(state_json(self.kept.timer.running(), self.kept.timer.phase))),
            _ => Some(Err(format!("no service {name}"))),
        }
    }

    fn page_closed(&mut self, page: u64) {
        if self.page == Some(page) {
            self.page = None;
            self.shown = false;
        }
    }

    fn focus_moved(&mut self, to: Focus) {
        self.moved(&to);
    }
}

export!(Pomodoro);

#[cfg(test)]
mod tests {
    use super::{end_words, round_note, state_json, Ended, Phase, ROUND_TAG};

    fn ended(phase: Phase, counted: bool, started_next: bool) -> Ended {
        Ended { phase, at: 0, length: 25 * 60_000, counted, round: 3, next: Phase::ShortBreak, started_next }
    }

    #[test]
    fn a_round_s_end_names_the_keys_of_the_scheme_in_use() {
        let next = "a short break (5 min)";
        let e = ended(Phase::Work, true, false);
        assert_eq!(end_words(&e, next, 4, "SPC o p s"), ("Round 3 done".to_string(), "a short break (5 min) next: SPC o p s starts it".to_string()));
        assert_eq!(end_words(&e, next, 4, "C-k o p s").1, "a short break (5 min) next: C-k o p s starts it");
        let e = ended(Phase::ShortBreak, true, false);
        assert_eq!(end_words(&e, next, 4, "C-k o p s"), ("Break's over".to_string(), "C-k o p s starts round 4".to_string()));
        let e = ended(Phase::Work, false, true);
        assert_eq!(end_words(&e, next, 4, "SPC o p s"), ("Skipped round 3".to_string(), "a short break (5 min) has started".to_string()));
    }

    #[test]
    fn a_round_that_ran_out_is_a_notification_that_starts_the_next() {
        let note = round_note(&ended(Phase::Work, true, false), "Round 3 done".into(), "a short break next".into(), 4);
        assert_eq!((note.summary.as_str(), note.body.as_str(), note.tag.as_deref()), ("Round 3 done", "a short break next", Some(ROUND_TAG)));
        assert_eq!((note.actions[0].label.as_str(), note.actions[0].command.as_str()), ("Start it", "pomodoro.start"));
        let note = round_note(&ended(Phase::LongBreak, true, false), "Break's over".into(), String::new(), 4);
        assert_eq!(note.actions[0].label, "Start round 4");
        // A break that started by itself has nothing to start.
        assert!(round_note(&ended(Phase::Work, true, true), "Round 3 done".into(), String::new(), 4).actions.is_empty());
    }

    #[test]
    fn its_state_is_served_for_other_extensions() {
        assert_eq!(state_json(false, Phase::Work), r#"{"phase": "off"}"#);
        assert_eq!(state_json(true, Phase::Work), r#"{"phase": "work"}"#);
        assert_eq!(state_json(true, Phase::LongBreak), r#"{"phase": "break"}"#);
    }

    /// What Fenix plays: a RIFF/WAVE file in its folder, well under 8 MiB.
    #[test]
    fn the_chime_is_a_small_wav() {
        let wav = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(super::CHIME)).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(wav.len() < 16 * 1024, "a chime, a few KB");
    }
}
