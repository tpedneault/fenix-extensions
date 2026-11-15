//! The gate on writes through the seam (`plans/tracker-seam.md` §3.6).
//!
//! Fenix 1.5 doesn't tell a tracker which extension called it, so any
//! installed extension could ask it to change issues as you. Until Fenix
//! passes the caller's id, a tracker asks you before the first write the
//! seam brings it, in these words, and keeps your answer while it runs;
//! its setting `<name>.seam_writes` (`ask`, `always`, `never`) can say for
//! good. It's a speed bump, not proof of who asks: once allowed, any
//! extension's writes go through. Every tracker uses this, so the
//! question reads the same everywhere.
//!
//! The gate holds the writes that wait on its question and says what to
//! do with each, as [`Step`]s, whenever something happens: a write comes
//! ([`Gate::check`]), you answer ([`Gate::answered`]), or the question's
//! time runs out ([`Gate::expire`], at [`Gate::deadline`]). It never
//! wedges: a question nobody answers -- Fenix replaced the picker, or
//! you walked away -- lapses after [`ASK_FOR`] and refuses what waited,
//! and the next write may ask again. At most [`MOST_HELD`] writes wait.
//! "Not now" (or Esc) refuses for [`COOL_OFF`] without asking, so it
//! can't be asked again and again until you give in; changing the
//! setting ([`Gate::reset`]) ends that sooner.
//!
//! When Fenix comes to pass the caller's id, give it as `caller`: the
//! question names it, and an "allow" is kept for that caller alone.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::{code, SeamError};

/// The setting's values, the first the default.
pub const SETTING_VALUES: [&str; 3] = ["ask", "always", "never"];

/// The picker's values (its rows' labels are [`Question::rows`]').
pub const ALLOW: &str = "allow";
pub const DENY: &str = "deny";

/// How long a question waits for an answer.
pub const ASK_FOR: Duration = Duration::from_secs(60);

/// How long "Not now" refuses without asking.
pub const COOL_OFF: Duration = Duration::from_secs(10 * 60);

/// The most writes that wait on one question; the rest are refused.
pub const MOST_HELD: usize = 50;

/// A write that's come through the seam: the service call to answer, and
/// what it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub call: u64,
    pub name: String,
    pub payload: String,
    /// Who sent it, when Fenix says (not in Fenix 1.5).
    pub caller: Option<String>,
}

/// What to show: a title, and rows as (label, value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub rows: Vec<(String, &'static str)>,
}

/// What to do now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Do this write.
    Go(Write),
    /// Ask this, as a picker; its answer goes to [`Gate::answered`], and
    /// [`Gate::expire`] is due at [`Gate::deadline`].
    Ask(Question),
    /// Answer this write with this error.
    Refuse(Write, SeamError),
}

#[derive(Debug, Clone)]
struct Asking {
    since: Instant,
    caller: Option<String>,
}

/// One tracker's gate, for as long as its extension runs.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    /// "Allow" said with no caller known: every extension.
    allowed_any: bool,
    /// "Allow" said for these callers.
    allowed: HashSet<String>,
    asking: Option<Asking>,
    held: Vec<Write>,
    cool_until: Option<Instant>,
}

/// The question, for tracker `tracker`, from `caller` when Fenix says.
pub fn question(tracker: &str, caller: Option<&str>) -> Question {
    let title = match caller {
        Some(c) => format!("{c} asks to change issues in {tracker} for you -- allow?"),
        None => format!("An extension asks to change issues in {tracker} for you (Fenix can't tell which; usually the Agenda) -- allow?"),
    };
    let allow = match caller {
        Some(c) => format!("Allow {c}'s changes through {tracker} until Fenix restarts"),
        None => format!("Allow every extension's changes through {tracker} until Fenix restarts"),
    };
    let deny = format!("Not now -- refuse them for {} minutes", COOL_OFF.as_secs() / 60);
    Question { title, rows: vec![(allow, ALLOW), (deny, DENY)] }
}

impl Gate {
    fn allows(&self, caller: Option<&str>) -> bool {
        self.allowed_any || caller.is_some_and(|c| self.allowed.contains(c))
    }

    /// When [`Gate::expire`] is next due: the open question's end.
    pub fn deadline(&self) -> Option<Instant> {
        self.asking.as_ref().map(|a| a.since + ASK_FOR)
    }

    /// Whether a question is open.
    pub fn asking(&self) -> bool {
        self.asking.is_some()
    }

    /// The setting changed: a "Not now" no longer holds.
    pub fn reset(&mut self) {
        self.cool_until = None;
    }

    /// A write has come, with the setting's value: what to do now.
    pub fn check(&mut self, setting: Option<&str>, tracker: &str, now: Instant, write: Write) -> Vec<Step> {
        let mut steps = self.expire(tracker, now);
        match setting.map(str::trim).unwrap_or("ask") {
            "always" => steps.push(Step::Go(write)),
            "never" => steps.push(Step::Refuse(write, SeamError::new(code::REFUSED, format!("changes from extensions are turned off in {tracker}'s settings")))),
            _ if self.allows(write.caller.as_deref()) => steps.push(Step::Go(write)),
            _ if self.cool_until.is_some_and(|until| until > now) => {
                let left = self.cool_until.map_or(0, |u| u.duration_since(now).as_secs().div_ceil(60));
                steps.push(Step::Refuse(write, SeamError::new(code::REFUSED, format!("you said not now to changes through {tracker} -- refused for {left} more minute(s), or change its seam_writes setting"))));
            }
            _ if self.asking.is_some() => {
                if self.held.len() >= MOST_HELD {
                    steps.push(Step::Refuse(write, SeamError::new(code::REFUSED, format!("too many changes waiting for your answer about {tracker} -- r on the task tries again"))));
                } else {
                    self.held.push(write);
                }
            }
            _ => {
                let q = question(tracker, write.caller.as_deref());
                self.asking = Some(Asking { since: now, caller: write.caller.clone() });
                self.held.push(write);
                steps.push(Step::Ask(q));
            }
        }
        steps
    }

    /// You answered the question: `Some(ALLOW)`, `Some(DENY)`, or nothing
    /// (Esc), which is "Not now".
    pub fn answered(&mut self, choice: Option<&str>, tracker: &str, now: Instant) -> Vec<Step> {
        let Some(asking) = self.asking.take() else { return Vec::new() };
        let held = std::mem::take(&mut self.held);
        if choice == Some(ALLOW) {
            match &asking.caller {
                Some(c) => {
                    self.allowed.insert(c.clone());
                }
                None => self.allowed_any = true,
            }
            return held
                .into_iter()
                .map(|w| {
                    if self.allows(w.caller.as_deref()) {
                        Step::Go(w)
                    } else {
                        Step::Refuse(w, SeamError::new(code::REFUSED, format!("you were asked about another extension's changes through {tracker} -- r on the task asks again")))
                    }
                })
                .collect();
        }
        self.cool_until = Some(now + COOL_OFF);
        let why = SeamError::new(code::REFUSED, format!("you didn't allow changes through {tracker} -- refused for {} minutes; r on the task tries again after that", COOL_OFF.as_secs() / 60));
        held.into_iter().map(|w| Step::Refuse(w, why.clone())).collect()
    }

    /// The question's time has run out at `now` (Fenix replaced it, or no
    /// one answered): what waited is refused, and the next write may ask.
    pub fn expire(&mut self, tracker: &str, now: Instant) -> Vec<Step> {
        if self.deadline().is_none_or(|d| d > now) {
            return Vec::new();
        }
        self.asking = None;
        let why = SeamError::new(code::REFUSED, format!("no answer came to whether to allow changes through {tracker} -- r on the task asks again"));
        std::mem::take(&mut self.held).into_iter().map(|w| Step::Refuse(w, why.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(call: u64) -> Write {
        Write { call, name: "comment".into(), payload: "{}".into(), caller: None }
    }

    fn calls(steps: &[Step]) -> Vec<(char, u64)> {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Go(w) => Some(('g', w.call)),
                Step::Refuse(w, _) => Some(('r', w.call)),
                Step::Ask(_) => None,
            })
            .collect()
    }

    #[test]
    fn it_asks_once_and_allow_lets_every_write_through() {
        let (mut g, t0) = (Gate::default(), Instant::now());
        let steps = g.check(None, "Jira", t0, w(1));
        assert!(matches!(&steps[..], [Step::Ask(q)] if q.title.contains("Fenix can't tell which; usually the Agenda")));
        assert!(g.check(None, "Jira", t0, w(2)).is_empty(), "held with the first, no second question");
        assert_eq!(calls(&g.answered(Some(ALLOW), "Jira", t0)), [('g', 1), ('g', 2)]);
        assert_eq!(calls(&g.check(None, "Jira", t0, w(3))), [('g', 3)]);
    }

    #[test]
    fn a_question_nobody_answers_lapses_and_never_wedges() {
        let (mut g, t0) = (Gate::default(), Instant::now());
        g.check(None, "Jira", t0, w(1));
        // Fenix replaced the picker: no answer ever comes.
        let later = t0 + ASK_FOR;
        assert_eq!(g.deadline(), Some(later));
        assert_eq!(calls(&g.expire("Jira", later)), [('r', 1)]);
        assert!(!g.asking());
        assert!(matches!(&g.check(None, "Jira", later, w(2))[..], [Step::Ask(_)]), "the next write asks again");
        // Lapsing found by the next write, too.
        let steps = g.check(None, "Jira", later + ASK_FOR, w(3));
        assert_eq!(calls(&steps), [('r', 2)]);
        assert!(steps.iter().any(|s| matches!(s, Step::Ask(_))));
    }

    #[test]
    fn at_most_so_many_writes_wait() {
        let (mut g, t0) = (Gate::default(), Instant::now());
        for i in 0..1000 {
            g.check(None, "Jira", t0, w(i));
        }
        let refused = calls(&g.answered(Some(DENY), "Jira", t0)).len();
        assert_eq!(refused, MOST_HELD, "only {MOST_HELD} waited; the rest were refused at once");
    }

    #[test]
    fn not_now_refuses_without_asking_for_a_while() {
        let (mut g, t0) = (Gate::default(), Instant::now());
        g.check(None, "Jira", t0, w(1));
        assert_eq!(calls(&g.answered(Some(DENY), "Jira", t0)), [('r', 1)]);
        for i in 2..7 {
            let steps = g.check(None, "Jira", t0 + Duration::from_secs(i), w(i));
            assert!(matches!(&steps[..], [Step::Refuse(_, e)] if e.message.contains("you said not now")), "no question: {steps:?}");
        }
        assert!(matches!(&g.check(None, "Jira", t0 + COOL_OFF, w(9))[..], [Step::Ask(_)]), "asked again after the cooling-off");
        // Esc is not now too; the setting's change ends it.
        let mut g = Gate::default();
        g.check(None, "Jira", t0, w(1));
        g.answered(None, "Jira", t0);
        assert!(matches!(&g.check(None, "Jira", t0, w(2))[..], [Step::Refuse(..)]));
        g.reset();
        assert!(matches!(&g.check(None, "Jira", t0, w(3))[..], [Step::Ask(_)]));
    }

    #[test]
    fn the_setting_overrides() {
        let t0 = Instant::now();
        assert_eq!(calls(&Gate::default().check(Some("always"), "Jira", t0, w(1))), [('g', 1)]);
        assert!(matches!(&Gate::default().check(Some("never"), "Jira", t0, w(1))[..], [Step::Refuse(_, e)] if e.code == "refused"));
    }

    #[test]
    fn a_known_caller_is_named_and_allowed_alone() {
        let (mut g, t0) = (Gate::default(), Instant::now());
        let from = |call: u64, c: &str| Write { caller: Some(c.into()), ..w(call) };
        let steps = g.check(None, "Linear", t0, from(1, "fenix.agenda"));
        assert!(matches!(&steps[..], [Step::Ask(q)] if q.title == "fenix.agenda asks to change issues in Linear for you -- allow?" && q.rows[0].0 == "Allow fenix.agenda's changes through Linear until Fenix restarts"));
        g.check(None, "Linear", t0, from(2, "acme.other"));
        assert_eq!(calls(&g.answered(Some(ALLOW), "Linear", t0)), [('g', 1), ('r', 2)]);
        assert_eq!(calls(&g.check(None, "Linear", t0, from(3, "fenix.agenda"))), [('g', 3)]);
        assert!(matches!(&g.check(None, "Linear", t0, from(4, "acme.other"))[..], [Step::Ask(q)] if q.title.starts_with("acme.other")));
    }

    #[test]
    fn the_words_say_what_is_true() {
        let q = question("Jira", None);
        assert_eq!(q.title, "An extension asks to change issues in Jira for you (Fenix can't tell which; usually the Agenda) -- allow?");
        assert_eq!(q.rows[0], ("Allow every extension's changes through Jira until Fenix restarts".to_string(), ALLOW));
        assert_eq!(q.rows[1], ("Not now -- refuse them for 10 minutes".to_string(), DENY));
    }
}
