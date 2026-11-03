//! The pets: who they are, what each is doing, what it means to do next,
//! and what they do when you do something. Time is milliseconds since the
//! extension started, given in, so it's all tested here natively.
//!
//! A pet does one act at a time (`acts`), from a plan of steps; when the
//! plan runs out it picks something itself. Getting from sitting to
//! walking, or from asleep to anything, goes by way of standing up or
//! waking: a plan gets those steps put in. Moving, it glides from one spot
//! to another and Fenix animates the glide, eased, arcing for a hop; the
//! extension decides a few times a second.

use std::collections::{HashMap, VecDeque};

use fenix_extension_api::{Ease, Item, Pose, Style};

use crate::acts::{Act, Posture};
use crate::care::{self, Care};
use crate::fx::{self, nudge, Ball, Fx};
use crate::season::{self, Season};
use crate::world::{Floor, Screen, Spot};

/// Nobody's typed for this long: they nap.
pub const SLEEP_AFTER: u64 = 3 * 60 * 1000;
/// Late at night, they nap sooner.
pub const SLEEP_AFTER_LATE: u64 = 60 * 1000;
/// Typing this long without a pause, and Ember catches fire.
const ON_FIRE_AFTER: u64 = 6000;
/// A pause this long ends a run of typing.
const STREAK_GAP: u64 = 2000;
/// Ash this long, and Ember rises again by itself.
const ASH_AT_MOST: u64 = 10 * 60 * 1000;
/// Saving teaches a pet something this often, at most.
const SAVE_XP_EVERY: u64 = 20_000;
/// The ball's item id.
pub const BALL_ID: u32 = 900;
/// The yarn's item id.
pub const YARN_ID: u32 = 901;
/// A long while: until something happens.
const UNTIL_TOLD: u64 = u64::MAX / 4;
/// Away this long, and they greet you when you're back.
const AWAY_AFTER: u64 = 90_000;
/// A build running this long, and its watcher dozes off.
const BUILD_DOZE_AFTER: u64 = 2 * 60 * 1000;
/// Tests passed in one run for a party, not just a cheer.
const PARTY_FROM: u32 = 100;
/// Moving between files makes one look round this often, at most.
const LOOK_ROUND_EVERY: u64 = 5000;

/// How a kind gets up to another floor: Ember flies, a snail creeps
/// there, the rest hop.
fn climb(kind: &str) -> Act {
    match kind {
        "ember" => Act::Fly,
        "snail" => Act::Walk,
        _ => Act::Hop,
    }
}

/// One step of a plan: an act, where it goes (if anywhere), how long it
/// lasts (else its clip's length, or a few seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub act: Act,
    pub to: Option<Spot>,
    pub ms: Option<u64>,
}

impl Step {
    pub fn act(act: Act) -> Step {
        Step { act, to: None, ms: None }
    }

    pub fn go(act: Act, to: Spot) -> Step {
        Step { act, to: Some(to), ms: None }
    }

    pub fn lasting(act: Act, ms: u64) -> Step {
        Step { act, to: None, ms: Some(ms) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pet {
    /// Its own, for its layer item: kept between runs.
    pub uid: u32,
    pub kind: String,
    pub name: String,
    /// Where it was when it set off, where it's going, and how.
    pub from: Spot,
    pub to: Spot,
    pub start: u64,
    pub millis: u32,
    pub ease: Ease,
    pub arc: f32,
    pub act: Act,
    pub since: u64,
    pub until: u64,
    pub posture: Posture,
    pub plan: VecDeque<Step>,
    /// Which way it faces, in degrees: 0 at you, 90 to the right.
    pub yaw: f32,
    pub care: Care,
    pub wearing: Vec<String>,
    /// When it next makes an effect of its own (z's, smoke, flames).
    pub puff_at: u64,
    /// When saving last taught it something.
    pub saved_at: u64,
}

impl Pet {
    pub fn new(uid: u32, kind: &str, name: &str, at: Spot, today: u32) -> Pet {
        Pet {
            uid,
            kind: kind.into(),
            name: name.into(),
            from: at,
            to: at,
            start: 0,
            millis: 0,
            ease: Ease::Linear,
            arc: 0.0,
            act: Act::Idle,
            since: 0,
            until: 0,
            posture: Posture::Standing,
            plan: VecDeque::new(),
            yaw: 0.0,
            care: Care::new(today),
            wearing: Vec::new(),
            puff_at: 0,
            saved_at: 0,
        }
    }

    pub fn moving(&self, now: u64) -> bool {
        self.millis > 0 && now < self.start + self.millis as u64
    }

    /// Where it is at `now`: on its way, by the clock (unless it's
    /// between floors, when it's where it's going).
    pub fn at(&self, now: u64) -> Spot {
        if !self.moving(now) || self.from.floor != self.to.floor {
            return self.to;
        }
        let t = (now - self.start) as f32 / self.millis as f32;
        Spot::new(self.to.floor, self.from.x + (self.to.x - self.from.x) * t)
    }

    /// Doing nothing in particular: free to be asked.
    fn free(&self) -> bool {
        self.plan.is_empty() && matches!(self.act, Act::Idle | Act::Look | Act::Sit | Act::Walk | Act::Stretch)
    }

    fn gone(&self) -> bool {
        self.posture == Posture::Gone
    }
}

/// Where they live: the modeline, or the foot of the pane you're in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    Modeline,
    Panes,
}

pub struct State {
    pub pets: Vec<Pet>,
    pub screen: Screen,
    pub home: Home,
    /// Lines tall.
    pub size: f32,
    pub most: usize,
    pub hidden: bool,
    pub style: Style,
    pub visits: bool,
    pub seasons: bool,
    pub errors: u32,
    pub last_typed: u64,
    /// When this run of typing began.
    pub streak_from: Option<u64>,
    /// When you last played with them.
    pub played_at: u64,
    pub fx: Fx,
    pub ball: Option<Ball>,
    /// Each kind's clips and how long they last, in seconds.
    pub clips: HashMap<String, HashMap<String, f32>>,
    /// Today, in days since 1970.
    pub today: u32,
    /// What to say, for whoever's showing them.
    pub said: Vec<String>,
    /// Something worth keeping changed.
    pub dirty: bool,
    next_uid: u32,
    rng: u64,
    next_social: u64,
    next_visit: u64,
    next_flake: u64,
    drifted_at: u64,
    /// The debug session the rubber duck is listening to.
    debugging: Option<u64>,
    /// When moving between files last made one look round.
    looked_round_at: u64,
    /// The focused file's warnings.
    pub warnings: u32,
    /// How long they linger, sitting and idling, and how far apart their
    /// games are: 1 normally, more when calm, less when lively.
    pub pace: f32,
    /// The hour, local time: late at night they're sleepy.
    pub hour: u8,
    next_follow: u64,
    /// The yarn's out: where it rolls from and to, and when it's put away.
    pub yarn: Option<Yarn>,
    /// There's a yarn model to play with.
    pub has_yarn: bool,
    /// A Pomodoro work round is on: they keep still and quiet.
    pub working: bool,
    /// The repository as last seen (`git-state`): what changed tells them
    /// of a commit, a push, a conflict.
    pub git: Option<Repo>,
    /// Files left in conflict: they hide till they're resolved.
    pub conflicts: u32,
    /// A task's running: since when, and which pet's watching it.
    pub building: Option<(u64, u32)>,
}

/// What they know of the repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repo {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub conflicts: u32,
}

/// What a change in the repository was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChange {
    Committed,
    Pushed,
    Pulled,
    Conflicted,
    Resolved,
    Switched,
}

impl Repo {
    /// What happened between `was` and `now`, most telling first.
    pub fn changes(was: &Repo, now: &Repo) -> Vec<GitChange> {
        let mut out = Vec::new();
        if was.conflicts == 0 && now.conflicts > 0 {
            out.push(GitChange::Conflicted);
        } else if was.conflicts > 0 && now.conflicts == 0 {
            out.push(GitChange::Resolved);
        }
        if was.branch != now.branch {
            out.push(GitChange::Switched);
        } else if was.head != now.head && now.head.is_some() {
            if now.behind < was.behind && now.ahead <= was.ahead {
                out.push(GitChange::Pulled);
            } else if now.conflicts == 0 {
                out.push(GitChange::Committed);
            }
        } else if was.ahead > 0 && now.ahead == 0 && now.behind == 0 {
            out.push(GitChange::Pushed);
        }
        out
    }
}

/// The cat's yarn, rolling from one spot to another.
#[derive(Debug, Clone, PartialEq)]
pub struct Yarn {
    pub from: Spot,
    pub to: Spot,
    pub start: u64,
    pub millis: u32,
    pub away_at: u64,
}

impl Yarn {
    pub fn rolling(&self, now: u64) -> bool {
        now < self.start + self.millis as u64
    }
}

impl State {
    pub fn new(pets: Vec<Pet>, seed: u64, today: u32) -> State {
        let next_uid = pets.iter().map(|p| p.uid).max().unwrap_or(0) + 1;
        State {
            pets,
            screen: Screen::default(),
            home: Home::Modeline,
            size: 2.2,
            most: 5,
            hidden: false,
            style: Style::Pixel,
            visits: true,
            seasons: true,
            errors: 0,
            last_typed: 0,
            streak_from: None,
            played_at: 0,
            fx: Fx::default(),
            ball: None,
            clips: HashMap::new(),
            today,
            said: Vec::new(),
            dirty: false,
            next_uid,
            rng: seed | 1,
            next_social: 60_000,
            next_visit: 120_000,
            next_flake: 0,
            drifted_at: 0,
            debugging: None,
            looked_round_at: 0,
            warnings: 0,
            pace: 1.0,
            hour: 12,
            next_follow: 20_000,
            yarn: None,
            has_yarn: false,
            working: false,
            git: None,
            conflicts: 0,
            building: None,
        }
    }

    fn random(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.random()
    }

    /// The floor they live on now.
    pub fn home_floor(&self) -> Floor {
        match (self.home, self.screen.focused()) {
            (Home::Panes, Some(p)) => Floor::Foot(p.id),
            _ => Floor::Modeline,
        }
    }

    /// Half a pet's width, in columns.
    fn half(&self) -> f32 {
        self.size * 0.9
    }

    fn season(&self) -> Season {
        if self.seasons {
            season::season(season::date(self.today))
        } else {
            Season::default()
        }
    }

    /// How long `act` lasts on `kind`'s model, played once, in ms.
    fn clip_ms(&self, kind: &str, act: Act) -> u64 {
        let secs = self.clips.get(kind).and_then(|c| c.get(act.clip(kind))).copied().unwrap_or(0.6);
        (secs * 1000.0) as u64
    }

    /// Which way to face to look at the caret from `at`: a little, not
    /// right round.
    fn towards_caret(&self, at: Spot) -> f32 {
        match self.screen.caret {
            Some((col, _)) => ((col as f32 - at.x) * 3.0).clamp(-50.0, 50.0),
            None => 0.0,
        }
    }

    /// Pet `i` starts `step` at `now`.
    fn begin(&mut self, i: usize, step: Step, now: u64) {
        let here = self.pets[i].at(now);
        let kind = self.pets[i].kind.clone();
        let act = step.act;
        let half = self.half();
        let scale = self.size / 2.2;
        let clip = self.clip_ms(&kind, act);
        let (to, millis, ease, arc, yaw) = match step.to {
            Some(to) => {
                let to = self.screen.clamp(to, half);
                let floors = if to.floor == here.floor { 0.0 } else { 4.0 };
                let distance = (to.x - here.x).abs() + floors;
                let millis = match act {
                    Act::Hop | Act::Pounce => 650,
                    _ => (distance / (act.speed(&kind) * scale) * 1000.0) as u32,
                }
                .max(1);
                let ease = match act {
                    Act::Hop | Act::Pounce => Ease::Linear,
                    Act::Fly => Ease::Smooth,
                    _ => Ease::Smooth,
                };
                let arc = match act {
                    Act::Hop => 1.6 + floors * 0.3,
                    Act::Pounce => 1.0,
                    Act::Fly => 2.5,
                    _ => 0.0,
                };
                let dx = to.x - here.x;
                let sideways = kind == "ferris" && matches!(act, Act::Walk | Act::Run | Act::Stalk);
                let yaw = if sideways || dx.abs() < 0.01 { self.pets[i].yaw } else if dx > 0.0 { 90.0 } else { -90.0 };
                let yaw = if sideways { 0.0 } else { yaw };
                (to, millis, ease, arc, yaw)
            }
            None => {
                let yaw = match act {
                    Act::Sleep | Act::LieDown => self.pets[i].yaw.clamp(-35.0, 35.0),
                    Act::Look | Act::Hide | Act::Burn | Act::Ash | Act::Rise => 0.0,
                    Act::Jump | Act::Stretch => self.pets[i].yaw,
                    _ => self.towards_caret(here),
                };
                (here, 0, Ease::Linear, 0.0, yaw)
            }
        };
        let lasts = match step.ms {
            Some(ms) => ms,
            None if act.moves() => (millis as u64).max(if act.once() { clip } else { 0 }),
            None if act.once() => clip,
            None => 3000,
        };
        let pet = &mut self.pets[i];
        pet.from = here;
        pet.to = to;
        pet.start = now;
        pet.millis = if act.moves() { millis } else { 0 };
        pet.ease = ease;
        pet.arc = arc;
        pet.act = act;
        pet.since = now;
        pet.until = now.saturating_add(lasts);
        pet.posture = act.leaves();
        pet.yaw = yaw;
        pet.puff_at = now;
    }

    /// Pet `i` means to do `steps`, by way of whatever its posture needs
    /// first: now (dropping what it meant to do) when `urgent`, else after
    /// what it already means to.
    pub fn plan(&mut self, i: usize, steps: Vec<Step>, now: u64, urgent: bool) {
        let kind = self.pets[i].kind.clone();
        let mut posture = if urgent { self.pets[i].posture } else { self.pets[i].plan.back().map_or(self.pets[i].posture, |s| s.act.leaves()) };
        let mut out = Vec::new();
        for step in steps.into_iter().filter(|s| !s.act.not_for(&kind)) {
            for way in step.act.way_from(posture) {
                out.push(Step::act(way));
            }
            posture = step.act.leaves();
            out.push(step);
        }
        let pet = &mut self.pets[i];
        if urgent {
            pet.plan = out.into();
            self.advance(i, now);
        } else {
            pet.plan.extend(out);
        }
    }

    /// Pet `i` goes on to its next step, or picks one.
    fn advance(&mut self, i: usize, now: u64) {
        match self.pets[i].plan.pop_front() {
            Some(step) => self.begin(i, step, now),
            None => {
                let steps = self.wander(i, now);
                self.plan(i, steps, now, false);
                if let Some(step) = self.pets[i].plan.pop_front() {
                    self.begin(i, step, now);
                }
            }
        }
    }

    /// How long they linger: their pace, and slower in a work round.
    fn lingering(&self) -> f32 {
        self.pace * if self.working { 1.8 } else { 1.0 }
    }

    /// What pet `i` does when it's nothing else to do.
    fn wander(&mut self, i: usize, now: u64) -> Vec<Step> {
        let pet = &self.pets[i];
        let (kind, here, posture, happy) = (pet.kind.clone(), pet.at(now), pet.posture, pet.care.happy);
        let home = self.home_floor();
        if posture == Posture::Gone {
            return vec![Step::lasting(Act::Ash, UNTIL_TOLD)];
        }
        if self.errors > 0 || self.conflicts > 0 {
            return vec![Step::lasting(Act::Hide, UNTIL_TOLD)];
        }
        // Watching a build: sitting, and dozing off if it goes on.
        if let Some((since, watcher)) = self.building {
            if self.pets[i].uid == watcher {
                return if now.saturating_sub(since) > BUILD_DOZE_AFTER { vec![Step::act(Act::LieDown), Step::lasting(Act::Sleep, UNTIL_TOLD)] } else { vec![Step::lasting(Act::Sit, 5000)] };
            }
        }
        // Debugging: the rubber duck stays by you, listening.
        if self.debugging.is_some() && kind == "duck" && posture != Posture::Lying {
            return vec![Step::lasting(Act::Listen, 8000)];
        }
        // Up on a ledge, or visiting: back home before long.
        if here.floor != home && !matches!(here.floor, Floor::Margin(_))
            && (self.random() < 0.5 || !self.screen.has(here.floor)) {
                let x = here.x + self.between(-6.0, 6.0);
                return vec![Step::go(climb(&kind), Spot::new(home, x))];
            }
        let roll = self.random();
        let seated = posture == Posture::Sitting;
        let far = self.between(4.0, 22.0) * if self.random() < 0.5 { -1.0 } else { 1.0 };
        let wide = Spot::new(here.floor, here.x + far);
        if seated && roll < 0.4 {
            return vec![Step::lasting(Act::Sit, (self.between(2500.0, 6000.0) * self.lingering()) as u64)];
        }
        match roll {
            r if r < 0.36 => vec![Step::go(Act::Walk, wide)],
            r if r < 0.46 => vec![Step::lasting(Act::Sit, (self.between(3000.0, 7000.0) * self.lingering()) as u64)],
            r if r < 0.56 => vec![Step::act(Act::Look)],
            r if r < 0.60 => vec![Step::act(Act::Stretch)],
            r if r < 0.65 && happy > 70.0 => vec![Step::lasting(Act::Happy, 1600)],
            r if r < 0.70 => {
                let ledges = self.screen.ledges();
                if ledges.is_empty() || here.floor != home || kind == "snail" {
                    vec![Step::go(Act::Walk, wide)]
                } else {
                    let pick = ledges[(self.random() * ledges.len() as f32) as usize % ledges.len()];
                    let x = here.x + self.between(-4.0, 4.0);
                    vec![Step::go(climb(&kind), Spot::new(pick, x)), Step::lasting(Act::Sit, self.between(5000.0, 12000.0) as u64)]
                }
            }
            // The zoomies: a dash one way and back again, when content
            // (not while you're in a work round).
            r if r < 0.74 && happy > 50.0 && !self.working && kind != "snail" => {
                let dash = self.between(10.0, 18.0) * far.signum();
                vec![Step::go(Act::Run, Spot::new(here.floor, here.x + dash)), Step::go(Act::Run, Spot::new(here.floor, here.x + dash * 0.3)), Step::act(Act::Look)]
            }
            r if r < 0.79 && kind == "duck" => vec![Step::act(Act::Quack)],
            r if r < 0.79 && kind == "ferris" => vec![Step::lasting(Act::Snap, 1800)],
            r if r < 0.79 && kind == "ember" => vec![Step::go(Act::Fly, wide)],
            // The owl hoots, the more so late at night.
            r if r < if self.late() { 0.86 } else { 0.79 } && kind == "owl" => vec![Step::act(Act::Hoot)],
            // The fox hunts something in the grass: creeps, and leaps.
            r if r < 0.79 && kind == "fox" => {
                let creep = Spot::new(here.floor, here.x + far * 0.5);
                vec![Step::go(Act::Stalk, creep), Step::go(Act::Pounce, Spot::new(here.floor, here.x + far * 0.7)), Step::act(Act::Look)]
            }
            r if r < 0.79 && kind == "snail" => vec![Step::act(Act::Look)],
            r if r < 0.79 && kind == "cat" && self.has_yarn && self.yarn.is_none() && !self.working => self.yarn_for(i, now),
            _ => vec![Step::lasting(Act::Idle, (self.between(1000.0, 2500.0) * self.lingering()) as u64)],
        }
    }

    // What you do, and what they do about it.

    /// You're typing: sleepers wake, and a long run of it sets Ember
    /// alight.
    pub fn typing(&mut self, now: u64) {
        if self.streak_from.is_none() || now.saturating_sub(self.last_typed) > STREAK_GAP {
            self.streak_from = Some(now);
        }
        self.last_typed = now;
        for i in 0..self.pets.len() {
            if self.pets[i].posture == Posture::Lying {
                let mut steps = vec![Step::act(Act::Wake)];
                if self.random() < 0.5 {
                    steps.push(Step::act(Act::Stretch));
                }
                self.plan(i, steps, now, true);
            }
        }
    }

    /// Whether a long run of typing is going on.
    pub fn on_fire(&self, now: u64) -> bool {
        self.streak_from.is_some_and(|from| self.last_typed.saturating_sub(from) >= ON_FIRE_AFTER) && now.saturating_sub(self.last_typed) < STREAK_GAP
    }

    /// A save: the pet nearest the caret hops, and learns a little.
    pub fn saved(&mut self, now: u64) {
        let caret = self.screen.caret.map(|(c, _)| c as f32);
        let nearest = (0..self.pets.len()).filter(|&i| !self.pets[i].gone() && self.pets[i].posture != Posture::Lying).min_by(|&a, &b| {
            let d = |i: usize| caret.map_or(i as f32, |c| (self.pets[i].at(now).x - c).abs());
            d(a).total_cmp(&d(b))
        });
        let Some(i) = nearest else { return };
        if !matches!(self.pets[i].act, Act::Run | Act::Hop | Act::Fly | Act::Hide) {
            self.plan(i, vec![Step::act(Act::Jump)], now, true);
        }
        if now.saturating_sub(self.pets[i].saved_at) >= SAVE_XP_EVERY || self.pets[i].saved_at == 0 {
            self.pets[i].saved_at = now;
            self.learn(i, 1, now);
        }
    }

    /// The focused file's errors: they hide while there are any, and
    /// stretch and cheer when they're gone.
    pub fn diagnostics(&mut self, errors: u32, warnings: u32, now: u64) {
        let had_warnings = self.warnings;
        self.warnings = warnings;
        // Warnings, and no errors: the one nearest the caret tilts its
        // head at them.
        if had_warnings == 0 && warnings > 0 && errors == 0 && self.errors == 0 {
            let caret = self.screen.caret.map(|(c, _)| c as f32);
            let near = (0..self.pets.len())
                .filter(|&i| self.pets[i].free() && !self.pets[i].gone() && self.pets[i].posture != Posture::Lying)
                .min_by(|&a, &b| {
                    let d = |i: usize| caret.map_or(i as f32, |c| (self.pets[i].at(now).x - c).abs());
                    d(a).total_cmp(&d(b))
                });
            if let Some(i) = near {
                self.plan(i, vec![Step::act(Act::Look), Step::act(Act::Look)], now, true);
            }
        }
        let was = self.errors;
        self.errors = errors;
        if (was > 0) == (errors > 0) {
            return;
        }
        for i in 0..self.pets.len() {
            if self.pets[i].gone() {
                continue;
            }
            if errors > 0 {
                let head = self.head(i, now);
                self.fx.add(now, &[fx::DROP], 0, head, nudge(&head, 0.6, 0.6), 900, Ease::In, 0.3, 0.6);
                self.plan(i, vec![Step::lasting(Act::Hide, UNTIL_TOLD)], now, true);
            } else {
                self.plan(i, vec![Step::act(Act::Stretch), Step::lasting(Act::Happy, 1400)], now, true);
            }
        }
    }

    /// A task ended. Passed: they all celebrate, and Ember rises from its
    /// ash. Failed: Ember burns up, the rubber duck comes to listen, the
    /// cat sits it out, Ferris ducks.
    pub fn task_ended(&mut self, ok: bool, now: u64) {
        self.task_ended_with(ok, None, now);
    }

    /// A task ended, with its tests' counts when Fenix read them: a big
    /// pass is a party, confetti from every one of them.
    pub fn task_ended_with(&mut self, ok: bool, tests: Option<(u32, u32)>, now: u64) {
        // The watcher wakes for the result.
        if let Some((_, watcher)) = self.building.take() {
            if let Some(i) = self.pets.iter().position(|p| p.uid == watcher) {
                if self.pets[i].posture == Posture::Lying {
                    self.plan(i, vec![Step::act(Act::Wake)], now, true);
                }
            }
        }
        if ok && tests.is_some_and(|(passed, failed)| failed == 0 && passed >= PARTY_FROM) {
            for i in 0..self.pets.len() {
                if !self.pets[i].gone() {
                    self.confetti(i, now);
                }
            }
        }
        let home = self.home_floor();
        let caret = self.screen.caret.map(|(c, _)| c as f32);
        for i in 0..self.pets.len() {
            let kind = self.pets[i].kind.clone();
            if ok {
                if self.pets[i].gone() {
                    self.rise(i, now);
                } else {
                    self.sparkle(i, now);
                    self.plan(i, vec![Step::lasting(Act::Happy, 2200)], now, true);
                }
                self.learn(i, 3, now);
                continue;
            }
            match kind.as_str() {
                "ember" if !self.pets[i].gone() => {
                    self.burst(i, now, &fx::FLAME, 7);
                    self.plan(i, vec![Step::act(Act::Burn), Step::lasting(Act::Ash, ASH_AT_MOST)], now, true);
                }
                "duck" => {
                    let x = caret.unwrap_or(self.pets[i].at(now).x + 3.0) + 3.0;
                    self.plan(i, vec![Step::go(Act::Walk, Spot::new(home, x)), Step::lasting(Act::Listen, 5000), Step::act(Act::Quack)], now, true);
                }
                "cat" => self.plan(i, vec![Step::lasting(Act::Sit, 6000), Step::act(Act::Look)], now, true),
                _ if !self.pets[i].gone() => self.plan(i, vec![Step::lasting(Act::Hide, 3000)], now, true),
                _ => {}
            }
        }
    }

    /// A debug session started: the rubber duck comes to sit by the line
    /// you're on and listens until it's over; the others look up. An
    /// adapter's child sessions are part of the one already going.
    pub fn debug_started(&mut self, id: u64, now: u64) {
        if self.debugging.is_some() {
            return;
        }
        self.debugging = Some(id);
        let home = self.home_floor();
        let caret = self.screen.caret.map(|(c, _)| c as f32);
        for i in 0..self.pets.len() {
            if self.pets[i].gone() || self.pets[i].posture == Posture::Lying {
                continue;
            }
            if self.pets[i].kind == "duck" {
                let x = caret.unwrap_or(self.pets[i].at(now).x) + 3.0;
                self.plan(i, vec![Step::go(Act::Walk, Spot::new(home, x)), Step::lasting(Act::Listen, UNTIL_TOLD)], now, true);
            } else if self.pets[i].free() {
                self.plan(i, vec![Step::act(Act::Look)], now, true);
            }
        }
    }

    /// Stopped at a breakpoint: a start from each who's watching, and the
    /// duck quacks; a step only gets a look.
    pub fn debug_paused(&mut self, breakpoint: bool, now: u64) {
        if self.debugging.is_none() {
            return;
        }
        for i in 0..self.pets.len() {
            if self.pets[i].gone() || self.pets[i].posture == Posture::Lying {
                continue;
            }
            if breakpoint {
                let head = self.head(i, now);
                self.fx.add(now, &[fx::BANG], 0, head, nudge(&head, 0.0, -0.8), 800, Ease::Out, 0.0, 0.6);
            }
            if self.pets[i].kind == "duck" {
                let steps = if breakpoint { vec![Step::act(Act::Quack), Step::lasting(Act::Listen, UNTIL_TOLD)] } else { vec![Step::lasting(Act::Listen, UNTIL_TOLD)] };
                self.plan(i, steps, now, true);
            } else if breakpoint && self.pets[i].free() {
                self.plan(i, vec![Step::act(Act::Jump), Step::act(Act::Look)], now, true);
            }
        }
    }

    /// The debug session's over: the duck's pleased it helped, and goes
    /// back to its day; the others stretch.
    pub fn debug_ended(&mut self, id: u64, now: u64) {
        if self.debugging != Some(id) {
            return;
        }
        self.debugging = None;
        for i in 0..self.pets.len() {
            if self.pets[i].gone() || self.pets[i].posture == Posture::Lying {
                continue;
            }
            if self.pets[i].kind == "duck" {
                self.sparkle(i, now);
                self.plan(i, vec![Step::lasting(Act::Happy, 1400)], now, true);
            } else if self.pets[i].free() {
                self.plan(i, vec![Step::act(Act::Stretch)], now, true);
            }
        }
    }

    /// A key, the first in a while: back after a good while away, those
    /// awake look up and greet you, one with a heart.
    pub fn came_back(&mut self, now: u64) {
        if now.saturating_sub(self.last_typed) < AWAY_AFTER || self.errors > 0 {
            return;
        }
        let mut hearted = false;
        for i in 0..self.pets.len() {
            if self.pets[i].gone() || self.pets[i].posture == Posture::Lying || !self.pets[i].free() {
                continue;
            }
            if !hearted {
                hearted = true;
                let head = self.head(i, now);
                self.fx.add(now, &[fx::HEART], 0, head, nudge(&head, 0.0, -1.6), 1300, Ease::Out, 0.0, 0.8);
            }
            self.plan(i, vec![Step::act(Act::Look), Step::lasting(Act::Happy, 1200)], now, true);
        }
    }

    /// You moved to another file: the pet nearest the caret looks round.
    pub fn focus_moved(&mut self, now: u64) {
        if now.saturating_sub(self.looked_round_at) < LOOK_ROUND_EVERY && self.looked_round_at > 0 {
            return;
        }
        let caret = self.screen.caret.map(|(c, _)| c as f32);
        let nearest = (0..self.pets.len()).filter(|&i| self.pets[i].free() && !self.pets[i].gone() && self.pets[i].posture != Posture::Lying && !self.pets[i].moving(now)).min_by(|&a, &b| {
            let d = |i: usize| caret.map_or(i as f32, |c| (self.pets[i].at(now).x - c).abs());
            d(a).total_cmp(&d(b))
        });
        if let Some(i) = nearest {
            self.looked_round_at = now;
            self.plan(i, vec![Step::act(Act::Look)], now, true);
        }
    }

    /// A task started: the one nearest the caret sits down to watch it.
    pub fn task_started(&mut self, now: u64) {
        let caret = self.screen.caret.map(|(c, _)| c as f32);
        let near = (0..self.pets.len()).filter(|&i| self.pets[i].free() && !self.pets[i].gone() && self.pets[i].posture != Posture::Lying).min_by(|&a, &b| {
            let d = |i: usize| caret.map_or(i as f32, |c| (self.pets[i].at(now).x - c).abs());
            d(a).total_cmp(&d(b))
        });
        let Some(i) = near else { return };
        self.building = Some((now, self.pets[i].uid));
        self.plan(i, vec![Step::act(Act::Look), Step::lasting(Act::Sit, 4000)], now, true);
    }

    /// A coding agent's turn ended (`agent-turn-ended`): done, and one
    /// cheers; an error, and Ember singes and the others look round; a
    /// refusal or a cancel gets a look.
    pub fn agent_done(&mut self, stop: &str, now: u64) {
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].free() && !self.pets[i].gone() && self.pets[i].posture != Posture::Lying).collect();
        match stop {
            "end_turn" => {
                if let Some(&i) = free.first() {
                    self.sparkle(i, now);
                    self.plan(i, vec![Step::lasting(Act::Happy, 1400)], now, true);
                }
            }
            "error" => {
                for i in free {
                    if self.pets[i].kind == "ember" {
                        self.burst(i, now, &fx::SMOKE, 4);
                        self.plan(i, vec![Step::act(Act::Jump), Step::act(Act::Look)], now, true);
                    } else {
                        self.plan(i, vec![Step::act(Act::Look)], now, true);
                    }
                }
            }
            _ => {
                if let Some(&i) = free.first() {
                    self.plan(i, vec![Step::act(Act::Look)], now, true);
                }
            }
        }
    }

    /// The repository as Fenix read it just now: compared with the last
    /// look, what changed. A commit: confetti from the one nearest the
    /// caret. A push: they all hop, seeing it off. A pull, or another
    /// branch: a look round. A conflict: they hide till it's resolved,
    /// then cheer.
    pub fn git(&mut self, repo: Option<Repo>, now: u64) {
        let Some(repo) = repo else {
            self.git = None;
            return;
        };
        let was = self.git.replace(repo.clone());
        self.conflicts = repo.conflicts;
        let Some(was) = was else { return };
        let awake: Vec<usize> = (0..self.pets.len()).filter(|&i| !self.pets[i].gone() && self.pets[i].posture != Posture::Lying).collect();
        for change in Repo::changes(&was, &repo) {
            match change {
                GitChange::Committed => {
                    let caret = self.screen.caret.map(|(c, _)| c as f32);
                    let near = awake.iter().copied().min_by(|&a, &b| {
                        let d = |i: usize| caret.map_or(i as f32, |c| (self.pets[i].at(now).x - c).abs());
                        d(a).total_cmp(&d(b))
                    });
                    if let Some(i) = near {
                        self.confetti(i, now);
                        self.plan(i, vec![Step::act(Act::Jump), Step::lasting(Act::Happy, 1500)], now, true);
                        self.learn(i, 1, now);
                    }
                }
                GitChange::Pushed => {
                    for &i in &awake {
                        self.plan(i, vec![Step::act(Act::Jump), Step::act(Act::Look)], now, true);
                    }
                }
                GitChange::Pulled | GitChange::Switched => {
                    for &i in &awake {
                        if self.pets[i].free() {
                            self.plan(i, vec![Step::act(Act::Look)], now, true);
                        }
                    }
                }
                GitChange::Conflicted => {
                    for &i in &awake {
                        let head = self.head(i, now);
                        self.fx.add(now, &[fx::BANG], 0, head, nudge(&head, 0.0, -0.8), 800, Ease::Out, 0.0, 0.6);
                        self.plan(i, vec![Step::lasting(Act::Hide, UNTIL_TOLD)], now, true);
                    }
                }
                GitChange::Resolved => {
                    for &i in &awake {
                        self.plan(i, vec![Step::act(Act::Stretch), Step::lasting(Act::Happy, 1400)], now, true);
                    }
                }
            }
        }
    }

    /// Ember, out of its ash.
    fn rise(&mut self, i: usize, now: u64) {
        self.burst(i, now, &fx::FLAME, 9);
        self.sparkle(i, now);
        self.pets[i].posture = Posture::Gone;
        self.plan(i, vec![Step::act(Act::Rise), Step::lasting(Act::Happy, 2000)], now, true);
    }

    /// Pet `uid` was clicked: a heart, a little dance.
    pub fn petted(&mut self, uid: u32, now: u64) {
        let Some(i) = self.pets.iter().position(|p| p.uid == uid) else { return };
        let head = self.head(i, now);
        self.fx.add(now, &[fx::HEART], 0, head, nudge(&head, 0.0, -1.6), 1300, Ease::Out, 0.0, 0.8);
        self.played_at = now;
        self.pets[i].care.cheer(8.0);
        self.learn(i, 1, now);
        if !self.pets[i].gone() && self.pets[i].posture != Posture::Lying {
            self.plan(i, vec![Step::lasting(Act::Happy, 1200)], now, true);
        }
    }

    /// `SPC P b`: thrown from the caret, it lands and bounces on; the
    /// nearest pet runs for it and brings it back.
    pub fn throw_ball(&mut self, now: u64) -> bool {
        let able: Vec<usize> = (0..self.pets.len()).filter(|&i| !self.pets[i].gone()).collect();
        if able.is_empty() {
            return false;
        }
        let home = self.home_floor();
        let Some((left, right)) = self.screen.span(home) else { return false };
        let land = left + (right - left) * self.between(0.25, 0.75);
        let dir = if self.random() < 0.5 { -1.0 } else { 1.0 };
        let from = if self.screen.caret.is_some() { Spot::new(Floor::Caret, 0.0) } else { Spot::new(home, land - dir * 10.0) };
        let mut ball = Ball::thrown(from, home, land, dir, now);
        let rest = self.screen.clamp(ball.rest(), 1.0);
        if let Some(last) = ball.legs.last_mut() {
            last.to = rest;
        }
        let fetcher = able.into_iter().min_by(|&a, &b| (self.pets[a].at(now).x - land).abs().total_cmp(&(self.pets[b].at(now).x - land).abs())).unwrap_or(0);
        ball.fetcher = Some(self.pets[fetcher].uid);
        let wait = ball.landed().saturating_sub(now);
        let side = if self.pets[fetcher].at(now).x < rest.x { -1.0 } else { 1.0 };
        let reach = Spot::new(home, rest.x + side * self.half() * 0.8);
        self.ball = Some(ball);
        self.played_at = now;
        self.plan(fetcher, vec![Step::lasting(Act::Look, wait), Step::go(Act::Run, reach)], now, true);
        true
    }

    /// The ball: picked up when the fetcher reaches it, carried back to
    /// the caret, dropped; gone a while after.
    fn ball_tick(&mut self, now: u64) {
        let Some(ball) = &self.ball else { return };
        if ball.dropped.is_some_and(|t| now > t + 5000) {
            self.ball = None;
            return;
        }
        let Some(i) = ball.fetcher.and_then(|uid| self.pets.iter().position(|p| p.uid == uid)) else {
            if let Some(b) = self.ball.as_mut().filter(|b| b.dropped.is_none()) {
                b.dropped = Some(now);
            }
            return;
        };
        let (rest, stopped, carried, dropped) = (ball.rest(), ball.stopped(), ball.carried, ball.dropped);
        let pet = &self.pets[i];
        let near = |x: f32, to: f32, half: f32| (x - to).abs() <= half * 1.2;
        if dropped.is_some() {
            return;
        }
        if !carried {
            if now >= stopped && !pet.moving(now) && near(pet.at(now).x, rest.x, self.half()) && pet.act != Act::Look {
                let home = self.home_floor();
                let back = self.screen.caret.map_or(rest.x + 10.0, |(c, _)| c as f32 + 4.0);
                if let Some(b) = self.ball.as_mut() {
                    b.carried = true;
                }
                self.plan(i, vec![Step::go(Act::Walk, Spot::new(home, back)), Step::lasting(Act::Idle, 300)], now, true);
            } else if now >= stopped && pet.plan.is_empty() && !pet.moving(now) && !matches!(pet.act, Act::Run | Act::Look) {
                // Distracted: go for it again.
                let reach = Spot::new(rest.floor, rest.x);
                self.plan(i, vec![Step::go(Act::Run, reach)], now, true);
            }
        } else if !pet.moving(now) && pet.act == Act::Idle {
            let at = pet.at(now);
            let dir = if pet.yaw < 0.0 { -1.0 } else { 1.0 };
            let x = at.x + dir * self.half();
            if let Some(b) = self.ball.as_mut() {
                b.carried = false;
                b.dropped = Some(now);
                b.legs = vec![fx::Leg { from: Spot::new(at.floor, x), to: Spot::new(at.floor, x + dir * 2.0), start: now, millis: 700, arc: 0.0, ease: Ease::Out }];
            }
            self.pets[i].care.cheer(10.0);
            self.learn(i, 4, now);
            self.plan(i, vec![Step::lasting(Act::Happy, 1500)], now, true);
        }
    }

    /// Pet `i` learns `xp`'s worth; a new level is told, with confetti.
    fn learn(&mut self, i: usize, xp: u32, now: u64) {
        self.dirty = true;
        if let Some(level) = self.pets[i].care.gain(xp) {
            let name = self.pets[i].name.clone();
            let what = care::unlocked_at(level).map(|w| format!(" -- and can wear {w} now (SPC P p)")).unwrap_or_default();
            self.said.push(format!("{name} reached level {level}{what}"));
            self.confetti(i, now);
        }
    }

    /// Where pet `i`'s head is, for an effect.
    fn head(&self, i: usize, now: u64) -> fenix_extension_api::Place {
        self.screen.place(self.pets[i].at(now), self.size * 0.85)
    }

    fn sparkle(&mut self, i: usize, now: u64) {
        let head = self.head(i, now);
        for k in 0..5 {
            let dx = (k as f32 - 2.0) * 1.2;
            self.fx.add(now, &fx::SPARK, 8, head, nudge(&head, dx, -0.6 - (k % 2) as f32 * 0.4), 600, Ease::Out, 0.6, 0.7);
        }
    }

    fn burst(&mut self, i: usize, now: u64, frames: &[u32], n: usize) {
        let foot = self.screen.place(self.pets[i].at(now), 0.3);
        for k in 0..n {
            let dx = self.between(-1.5, 1.5);
            let rise = self.between(0.8, 2.0);
            let millis = 500 + (k as u32 % 3) * 120;
            self.fx.add(now, frames, 10, foot, nudge(&foot, dx, -rise), millis, Ease::Out, 0.0, 0.8);
        }
    }

    fn confetti(&mut self, i: usize, now: u64) {
        let head = self.head(i, now);
        for k in 0..12 {
            let piece = fx::CONFETTI[k % 4];
            let from = nudge(&head, self.between(-2.0, 2.0), -2.5);
            let to = nudge(&head, self.between(-4.0, 4.0), self.size * 0.85);
            self.fx.add(now, &[piece], 0, from, to, 1400 + (k as u32 * 60), Ease::In, 0.5, 0.5);
        }
    }

    // Things they do on their own.

    /// A few times a second: what's due happens.
    pub fn tick(&mut self, now: u64) {
        self.fx.prune(now);
        self.ball_tick(now);
        // Nobody's typed in a while: a nap, all together -- sooner late
        // at night.
        if now.saturating_sub(self.last_typed) > self.sleep_after() && self.errors == 0 {
            self.nap(now);
        }
        if now >= self.next_follow {
            self.next_follow = now + (self.between(15_000.0, 35_000.0) * self.pace) as u64;
            if !self.working {
                self.follow(now);
            }
        }
        if self.yarn.as_ref().is_some_and(|y| now >= y.away_at) {
            self.yarn = None;
        }
        for i in 0..self.pets.len() {
            if now >= self.pets[i].until {
                self.advance(i, now);
            }
        }
        self.puffs(now);
        if now >= self.next_social {
            self.next_social = now + (self.between(25_000.0, 75_000.0) * self.pace) as u64;
            if !self.working {
                self.social(now);
            }
        }
        if now >= self.next_visit {
            self.next_visit = now + (self.between(120_000.0, 300_000.0) * self.pace) as u64;
            if self.visits && !self.working {
                self.visit(now);
            }
        }
        if self.on_fire(now) {
            self.ember_runs(now);
        }
        if self.season().snow && now >= self.next_flake {
            self.next_flake = now + 450;
            self.snow(now);
        }
        // Content, between bouts of play, a minute at a time.
        if now >= self.drifted_at + 60_000 {
            self.drifted_at = now;
            if now.saturating_sub(self.played_at) > 10 * 60_000 {
                for pet in &mut self.pets {
                    pet.care.drift(1.0);
                }
            }
        }
    }

    /// Whether pet `i` is busy watching a build: left to it.
    fn watching(&self, i: usize) -> bool {
        self.building.is_some_and(|(_, uid)| self.pets[i].uid == uid)
    }

    /// Whether it's late at night, local time.
    pub fn late(&self) -> bool {
        self.hour >= 22 || self.hour < 6
    }

    /// How long nobody types before they nap.
    pub fn sleep_after(&self) -> u64 {
        if self.late() {
            SLEEP_AFTER_LATE
        } else {
            SLEEP_AFTER
        }
    }

    /// Now and then the pet nearest the caret, if it's wandered off,
    /// trots back over to where you're working.
    fn follow(&mut self, now: u64) {
        let Some((col, _)) = self.screen.caret else { return };
        let (caret, home) = (col as f32, self.home_floor());
        let near = (0..self.pets.len())
            .filter(|&i| self.pets[i].free() && !self.watching(i) && !self.pets[i].gone() && self.pets[i].posture == Posture::Standing && !self.pets[i].moving(now) && self.pets[i].at(now).floor == home)
            .min_by(|&a, &b| (self.pets[a].at(now).x - caret).abs().total_cmp(&(self.pets[b].at(now).x - caret).abs()));
        let Some(i) = near else { return };
        let x = self.pets[i].at(now).x;
        if (x - caret).abs() < 8.0 {
            return;
        }
        let to = caret + if x < caret { -3.0 } else { 3.0 };
        let act = if (x - caret).abs() > 30.0 { Act::Run } else { Act::Walk };
        self.plan(i, vec![Step::go(act, Spot::new(home, to)), Step::act(Act::Look)], now, true);
    }

    /// Z's from sleepers, smoke from the ash, flames off Ember on fire.
    fn puffs(&mut self, now: u64) {
        let fire = self.on_fire(now);
        for i in 0..self.pets.len() {
            if now < self.pets[i].puff_at {
                continue;
            }
            let pet = &self.pets[i];
            let (act, kind, moving) = (pet.act, pet.kind.clone(), pet.moving(now));
            let head = self.head(i, now);
            let foot = self.screen.place(self.pets[i].at(now), 0.2);
            let every = match act {
                Act::Sleep => {
                    let big = (now / 1800).is_multiple_of(2);
                    self.fx.add(now, &[if big { fx::ZED_BIG } else { fx::ZED }], 0, head, nudge(&head, 1.2, -1.4), 2000, Ease::Out, 0.0, 0.6);
                    self.fx.fading();
                    1800
                }
                Act::Ash => {
                    self.fx.add(now, &fx::SMOKE, 3, foot, nudge(&foot, 0.4, -1.6), 1500, Ease::Out, 0.0, 0.8);
                    self.fx.fading();
                    1200
                }
                Act::Run | Act::Fly if kind == "ember" && fire && moving => {
                    let back = if self.pets[i].yaw > 0.0 { -1.0 } else { 1.0 };
                    let at = nudge(&foot, back * self.half(), 0.0);
                    self.fx.add(now, &fx::FLAME, 10, at, nudge(&at, back * 0.5, -0.9), 600, Ease::Out, 0.0, 0.7);
                    150
                }
                Act::Quack if now.saturating_sub(self.pets[i].since) < 400 => {
                    self.fx.add(now, &[fx::NOTE], 0, head, nudge(&head, 0.8, -1.2), 900, Ease::Out, 0.3, 0.6);
                    UNTIL_TOLD
                }
                _ => 500,
            };
            self.pets[i].puff_at = now.saturating_add(every);
        }
    }

    /// Asleep, all of them, curled up near the first to drop off.
    fn nap(&mut self, now: u64) {
        // Late at night the owl's wide awake: it naps only when everyone
        // would by day.
        let owl_up = self.late() && now.saturating_sub(self.last_typed) <= SLEEP_AFTER;
        let awake: Vec<usize> = (0..self.pets.len()).filter(|&i| !(owl_up && self.pets[i].kind == "owl") && self.pets[i].posture != Posture::Lying && !self.pets[i].gone() && !matches!(self.pets[i].act, Act::LieDown) && !self.pets[i].plan.iter().any(|s| s.act == Act::Sleep)).collect();
        if awake.is_empty() {
            return;
        }
        let home = self.home_floor();
        let den = self.pets.iter().find(|p| p.posture == Posture::Lying).map(|p| p.at(now).x).unwrap_or_else(|| self.pets[awake[0]].at(now).x);
        for (n, i) in awake.into_iter().enumerate() {
            let side = if n % 2 == 0 { 1.0 } else { -1.0 };
            let x = den + side * (n as f32 / 2.0 + 1.0).floor() * self.half() * 2.2;
            self.plan(i, vec![Step::go(Act::Walk, Spot::new(home, x)), Step::act(Act::LieDown), Step::lasting(Act::Sleep, UNTIL_TOLD)], now, true);
        }
    }

    /// Now and then, two of them play: the cat stalks the duck, or one
    /// chases another.
    fn social(&mut self, now: u64) {
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].free() && !self.watching(i) && self.pets[i].posture != Posture::Lying && self.pets[i].at(now).floor == self.home_floor()).collect();
        if free.len() < 2 {
            return;
        }
        let cat = free.iter().copied().find(|&i| self.pets[i].kind == "cat");
        let duck = free.iter().copied().find(|&i| self.pets[i].kind == "duck");
        if let (Some(cat), Some(duck)) = (cat, duck) {
            if self.random() < 0.6 {
                return self.stalk(cat, duck, now);
            }
        }
        // Snowing, and Ember's about: they gather round it to keep warm.
        if self.season().snow && self.random() < 0.5 {
            if let Some(ember) = free.iter().copied().find(|&i| self.pets[i].kind == "ember") {
                return self.warm_by(ember, &free, now);
            }
        }
        let a = free[(self.random() * free.len() as f32) as usize % free.len()];
        let b = free.iter().copied().find(|&b| b != a).unwrap_or(a);
        if self.random() < 0.35 {
            return self.race(a, b, now);
        }
        self.chase(a, b, now);
    }

    /// Two race to the same spot: the one that gets there first is pleased
    /// with itself; the other looks round for a rematch.
    fn race(&mut self, a: usize, b: usize, now: u64) {
        let home = self.home_floor();
        let (ax, bx) = (self.pets[a].at(now).x, self.pets[b].at(now).x);
        let dir = if self.random() < 0.5 { -1.0 } else { 1.0 };
        let line = (ax + bx) / 2.0 + dir * self.between(14.0, 24.0);
        let time = |i: usize, x: f32, st: &State| (line - x).abs() / Act::Run.speed(&st.pets[i].kind);
        let (first, second) = if time(a, ax, self) <= time(b, bx, self) { (a, b) } else { (b, a) };
        for (i, gap) in [(first, 0.0), (second, self.half() * 2.2)] {
            let at = Spot::new(home, line - dir * gap);
            let last = if i == first { Step::lasting(Act::Happy, 1600) } else { Step::act(Act::Look) };
            self.plan(i, vec![Step::lasting(Act::Look, 400), Step::go(Act::Run, at), last], now, true);
        }
        self.sparkle(first, now + 2500);
    }

    /// The others come and sit round Ember, out of the snow.
    fn warm_by(&mut self, ember: usize, free: &[usize], now: u64) {
        let home = self.home_floor();
        let x = self.pets[ember].at(now).x;
        self.plan(ember, vec![Step::lasting(Act::Sit, 15_000)], now, true);
        let others: Vec<usize> = free.iter().copied().filter(|&i| i != ember).collect();
        for (n, i) in others.into_iter().enumerate() {
            let side = if n % 2 == 0 { 1.0 } else { -1.0 };
            let at = x + side * (n as f32 / 2.0 + 1.0).floor() * self.half() * 2.2;
            self.plan(i, vec![Step::go(Act::Walk, Spot::new(home, at)), Step::lasting(Act::Sit, 15_000)], now, true);
        }
    }

    /// The cat bats its yarn: it rolls off, and the cat creeps after it
    /// and pounces.
    fn yarn_for(&mut self, i: usize, now: u64) -> Vec<Step> {
        let here = self.pets[i].at(now);
        let dir = if self.random() < 0.5 { -1.0 } else { 1.0 };
        let from = Spot::new(here.floor, here.x + dir * self.half() * 1.2);
        let to = Spot::new(here.floor, from.x + dir * self.between(6.0, 11.0));
        let yarn = Yarn { from, to, start: now, millis: 1100, away_at: now + 14_000 };
        let near = Spot::new(here.floor, to.x - dir * self.half() * 3.0);
        let pounce = Spot::new(here.floor, to.x - dir * self.half());
        self.yarn = Some(yarn);
        vec![Step::lasting(Act::Look, 600), Step::go(Act::Stalk, near), Step::go(Act::Pounce, pounce), Step::lasting(Act::Happy, 1500)]
    }

    /// The Pomodoro timer's phase (`pomodoro.state`): in a work round they
    /// keep still and quiet; when the break comes, they play.
    pub fn pomodoro(&mut self, phase: &str, now: u64) {
        let was = self.working;
        self.working = phase == "work";
        if was && !self.working && phase == "break" {
            for i in 0..self.pets.len() {
                if self.pets[i].free() && !self.pets[i].gone() && self.pets[i].posture != Posture::Lying {
                    self.plan(i, vec![Step::act(Act::Stretch), Step::lasting(Act::Happy, 1200)], now, true);
                }
            }
            self.next_social = now + 3000;
        }
    }

    /// The cat creeps up on the duck and pounces; the duck jumps and runs.
    fn stalk(&mut self, cat: usize, duck: usize, now: u64) {
        let home = self.home_floor();
        let (cx, dx) = (self.pets[cat].at(now).x, self.pets[duck].at(now).x);
        let side = if cx < dx { -1.0 } else { 1.0 };
        let creep = Spot::new(home, dx + side * self.half() * 3.0);
        let pounce = Spot::new(home, dx + side * self.half());
        let creep_ms = ((creep.x - cx).abs() / (Act::Stalk.speed("cat") * self.size / 2.2) * 1000.0) as u64;
        self.plan(cat, vec![Step::go(Act::Stalk, creep), Step::go(Act::Pounce, pounce), Step::lasting(Act::Happy, 1500)], now, true);
        // The duck notices just as the cat leaps.
        let away = Spot::new(home, dx - side * self.between(10.0, 18.0));
        let duck_head = self.head(duck, now);
        let wait = creep_ms + 250;
        self.plan(duck, vec![Step::lasting(Act::Idle, wait), Step::act(Act::Jump), Step::go(Act::Run, away), Step::act(Act::Quack)], now, true);
        self.fx.add(now + wait, &[fx::BANG], 0, duck_head, nudge(&duck_head, 0.0, -0.8), 800, Ease::Out, 0.0, 0.6);
    }

    /// One runs at the other, who runs off; then they're both pleased.
    fn chase(&mut self, a: usize, b: usize, now: u64) {
        let home = self.home_floor();
        let bx = self.pets[b].at(now).x;
        let away = bx + if self.pets[a].at(now).x < bx { 1.0 } else { -1.0 } * self.between(12.0, 20.0);
        self.plan(a, vec![Step::go(Act::Run, Spot::new(home, away)), Step::lasting(Act::Happy, 1200)], now, true);
        self.plan(b, vec![Step::lasting(Act::Look, 300), Step::go(Act::Run, Spot::new(home, away + (away - bx).signum() * self.half() * 2.5)), Step::lasting(Act::Happy, 1200)], now, true);
    }

    /// One comes up to sit beside the line you're on, in the margin, for a
    /// while.
    fn visit(&mut self, now: u64) {
        let Some(margin) = self.screen.margin() else { return };
        let home = self.home_floor();
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].free() && self.pets[i].posture != Posture::Lying && self.pets[i].at(now).floor == home && self.pets[i].kind != "snail" && !self.watching(i)).collect();
        if free.is_empty() {
            return;
        }
        let i = free[(self.random() * free.len() as f32) as usize % free.len()];
        let Some((l, r)) = self.screen.span(margin) else { return };
        let up = climb(&self.pets[i].kind);
        let back = self.pets[i].at(now).x;
        let stay = self.between(20_000.0, 40_000.0) as u64;
        self.plan(i, vec![Step::go(up, Spot::new(margin, (l + r) / 2.0)), Step::lasting(Act::Sit, stay), Step::go(up, Spot::new(home, back))], now, true);
    }

    /// Ember, alight with your typing, runs along the floor trailing
    /// flames.
    fn ember_runs(&mut self, now: u64) {
        let home = self.home_floor();
        let Some(i) = self.pets.iter().position(|p| p.kind == "ember" && !p.gone() && p.posture == Posture::Standing && p.plan.is_empty() && !p.moving(now)) else { return };
        if self.pets[i].at(now).floor != home {
            return;
        }
        let x = self.pets[i].at(now).x + self.between(8.0, 20.0) * if self.random() < 0.5 { -1.0 } else { 1.0 };
        self.plan(i, vec![Step::go(Act::Run, Spot::new(home, x))], now, true);
    }

    fn snow(&mut self, now: u64) {
        let Some((l, r)) = self.screen.span(Floor::Modeline) else { return };
        let x = self.between(l, r);
        let from = self.screen.place(Spot::new(Floor::Modeline, x), 6.0);
        let drift = self.between(-3.0, 3.0);
        let to = self.screen.place(Spot::new(Floor::Modeline, x + drift), 0.0);
        self.fx.add(now, &fx::FLAKE, 2, from, to, 5000, Ease::Linear, 0.0, 0.5);
        self.fx.fading();
    }

    /// The panes moved: a pet whose floor went goes home; one visiting
    /// follows the caret to its new line, or goes home when there's no
    /// room; and when home moved (another pane focused), they hop there.
    pub fn layout(&mut self, screen: Screen, now: u64) {
        let old_home = self.home_floor();
        self.screen = screen;
        let home = self.home_floor();
        let margin = self.screen.margin();
        for i in 0..self.pets.len() {
            let pet = &self.pets[i];
            let (at, gone) = (pet.at(now), pet.gone());
            let target = pet.to.floor;
            let up = climb(&pet.kind);
            match target {
                Floor::Margin(row) if margin != Some(Floor::Margin(row)) => {
                    let steps = match margin {
                        Some(m) if pet.act == Act::Sit => vec![Step::go(up, Spot::new(m, at.x)), Step::lasting(Act::Sit, 15_000), Step::go(up, Spot::new(home, at.x - 10.0))],
                        _ => vec![Step::go(up, Spot::new(home, at.x - 10.0))],
                    };
                    self.plan(i, steps, now, true);
                }
                f if !self.screen.has(f) => {
                    // Its floor's gone: it's home, at once.
                    let spot = self.screen.clamp(Spot::new(home, at.x), self.size);
                    let pet = &mut self.pets[i];
                    pet.from = spot;
                    pet.to = spot;
                    pet.millis = 0;
                }
                f if f == old_home && home != old_home && !gone => {
                    self.plan(i, vec![Step::go(up, Spot::new(home, at.x))], now, true);
                }
                _ => {}
            }
        }
    }

    // Who they are.

    /// A new pet, coming in from the side.
    pub fn adopt(&mut self, kind: &str, name: &str, now: u64) -> bool {
        if self.pets.len() >= self.most {
            return false;
        }
        let home = self.home_floor();
        let (l, r) = self.screen.span(home).unwrap_or((0.0, 80.0));
        let uid = self.next_uid;
        self.next_uid += 1;
        let mut pet = Pet::new(uid, kind, name, Spot::new(home, l - 2.0), self.today);
        pet.until = now;
        self.pets.push(pet);
        let i = self.pets.len() - 1;
        let x = self.between(l + (r - l) * 0.2, l + (r - l) * 0.8);
        let arrive = if kind == "ember" { Act::Fly } else { Act::Run };
        self.plan(i, vec![Step::go(arrive, Spot::new(home, x)), Step::lasting(Act::Happy, 1500)], now, true);
        self.confetti(i, now);
        self.dirty = true;
        true
    }

    pub fn rehome(&mut self, uid: u32) -> Option<Pet> {
        let i = self.pets.iter().position(|p| p.uid == uid)?;
        self.dirty = true;
        if self.ball.as_ref().is_some_and(|b| b.fetcher == Some(uid)) {
            self.ball = None;
        }
        Some(self.pets.remove(i))
    }

    /// The parts pet `i` wears now: what it chose (and has unlocked),
    /// the season's hat in place of its own, and Ember's ash.
    pub fn parts(&self, i: usize) -> Vec<String> {
        let pet = &self.pets[i];
        let unlocked = pet.care.unlocked();
        let mut parts: Vec<String> = pet.wearing.iter().filter(|w| unlocked.contains(&w.as_str())).cloned().collect();
        let season = self.season();
        let hat = season.hat.or_else(|| season::anniversary(self.today, pet.care.adopted).then_some("party_hat"));
        if let Some(hat) = hat {
            parts.retain(|p| !care::HATS.contains(&p.as_str()));
            parts.push(hat.into());
        }
        if pet.kind == "ember" && matches!(pet.act, Act::Burn | Act::Ash | Act::Rise) {
            parts.push("ash".into());
        }
        parts
    }

    /// What the layer draws at `now`: each pet, the ball, the effects.
    /// `models` gives a kind's model id; `ball` the ball's; `fx` the
    /// effects' sheet.
    pub fn items(&self, now: u64, models: &HashMap<String, u32>, ball: Option<u32>, fx: Option<u32>) -> Vec<Item> {
        if self.hidden {
            return Vec::new();
        }
        let redraws = if self.style == Style::Pixel { 12 } else { 30 };
        let fire = self.on_fire(now);
        let mut out = Vec::new();
        for (i, pet) in self.pets.iter().enumerate() {
            let Some(&model) = models.get(&pet.kind) else { continue };
            let mut pose = Pose::new(model, pet.act.clip(&pet.kind)).yaw(pet.yaw).style(self.style);
            if pet.act.once() {
                pose = pose.once();
            }
            for part in self.parts(i) {
                pose = pose.part(&part);
            }
            if pet.kind == "ember" {
                pose = pose.glow(match pet.act {
                    Act::Burn | Act::Rise => 0.85,
                    _ if fire => 0.35,
                    _ => 0.1,
                });
            }
            pose = match pet.act {
                Act::Hide => pose.tint((0.82, 0.82, 0.92)),
                Act::Sleep => pose.tint((0.9, 0.9, 1.0)),
                _ => pose,
            };
            let at = self.screen.place(pet.from, 0.0);
            let mut item = Item::model(pet.uid, pose, at).size(self.size * crate::acts::stature(&pet.kind)).redraws(redraws).clickable().keep_clear();
            if pet.millis > 0 {
                item = item.glide(self.screen.place(pet.to, 0.0), pet.millis).ease(pet.ease).arc(pet.arc);
            }
            out.push(item);
        }
        if let (Some(model), Some(b)) = (ball, &self.ball) {
            let dir = b.legs.last().map_or(1.0, |l| (l.to.x - l.from.x).signum());
            let pose = Pose::new(model, "roll").speed(if b.rolling(now) { 1.8 } else { 0.0 }).yaw(dir * 90.0).style(self.style);
            let carrier = b.carried.then(|| b.fetcher.and_then(|uid| self.pets.iter().find(|p| p.uid == uid))).flatten();
            let item = match carrier {
                Some(pet) => {
                    let ahead = if pet.yaw < 0.0 { -1.0 } else { 1.0 } * self.half();
                    let lift = self.size * 0.3;
                    let at = nudge(&self.screen.place(pet.from, lift), ahead, 0.0);
                    let item = Item::model(BALL_ID, pose, at).size(self.size * 0.3).redraws(redraws);
                    if pet.millis > 0 {
                        item.glide(nudge(&self.screen.place(pet.to, lift), ahead, 0.0), pet.millis).ease(pet.ease).arc(pet.arc)
                    } else {
                        item
                    }
                }
                None => {
                    let leg = b.leg(now);
                    let from = self.screen.place(leg.from, 0.0);
                    Item::model(BALL_ID, pose, from).size(self.size * 0.3).redraws(redraws).glide(self.screen.place(leg.to, 0.0), leg.millis).ease(leg.ease).arc(leg.arc)
                }
            };
            out.push(item);
        }
        if let (Some(&model), Some(y)) = (models.get("yarn"), &self.yarn) {
            let rolling = y.rolling(now);
            let dir = (y.to.x - y.from.x).signum();
            let pose = Pose::new(model, "roll").speed(if rolling { 1.6 } else { 0.0 }).yaw(dir * 90.0).style(self.style);
            let item = Item::model(YARN_ID, pose, self.screen.place(y.from, 0.0)).size(self.size * 0.25).redraws(redraws);
            out.push(item.glide(self.screen.place(y.to, 0.0), y.millis).ease(Ease::Out));
        }
        if let Some(sheet) = fx {
            out.extend(self.fx.items(sheet));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::tests::screen;

    fn state(kinds: &[&str]) -> State {
        let pets = kinds.iter().enumerate().map(|(i, k)| Pet::new(i as u32 + 1, k, k, Spot::new(Floor::Modeline, 10.0 + i as f32 * 20.0), 100)).collect();
        let mut st = State::new(pets, 7, 100);
        st.screen = screen();
        st.seasons = false;
        st
    }

    fn models() -> HashMap<String, u32> {
        ["ember", "ferris", "cat", "duck"].iter().enumerate().map(|(i, k)| (k.to_string(), i as u32 + 10)).collect()
    }

    #[test]
    fn a_plan_gets_there_by_way_of_standing_up() {
        let mut st = state(&["cat"]);
        st.plan(0, vec![Step::lasting(Act::Sit, 5000)], 0, true);
        assert_eq!(st.pets[0].act, Act::SitDown, "sits down first");
        assert_eq!(st.pets[0].plan.front().map(|s| s.act), Some(Act::Sit));
        st.plan(0, vec![Step::go(Act::Walk, Spot::new(Floor::Modeline, 40.0))], 100, true);
        assert_eq!(st.pets[0].act, Act::StandUp);
        st.tick(st.pets[0].until);
        let now = st.pets[0].since;
        assert_eq!(st.pets[0].act, Act::Walk);
        assert_eq!((st.pets[0].from.x, st.pets[0].to.x, st.pets[0].yaw, st.pets[0].ease), (10.0, 40.0, 90.0, Ease::Smooth));
        // 30 columns at 3.4 a second.
        assert_eq!(st.pets[0].millis, (30.0 / 3.4 * 1000.0) as u32);
        assert!(st.pets[0].at(now + 4412).x > 24.0 && st.pets[0].at(now + 4412).x < 26.0);
    }

    #[test]
    fn they_wander_nap_together_and_wake_when_you_type() {
        let mut st = state(&["cat", "duck", "ferris"]);
        st.last_typed = 0;
        for t in (0..60_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.pets.iter().all(|p| p.posture != Posture::Lying), "awake while you're about");
        let later = SLEEP_AFTER + 1000;
        for t in (later..later + 40_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.pets.iter().all(|p| p.act == Act::Sleep), "{:?}", st.pets.iter().map(|p| p.act).collect::<Vec<_>>());
        let xs: Vec<f32> = st.pets.iter().map(|p| p.at(later + 40_000).x).collect();
        let spread = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < 15.0, "curled up together: {xs:?}");
        assert!(st.fx.list.iter().any(|p| p.frames == [fx::ZED] || p.frames == [fx::ZED_BIG]), "z's");
        st.typing(later + 41_000);
        assert!(st.pets.iter().all(|p| p.act == Act::Wake));
    }

    #[test]
    fn one_trots_over_to_the_caret_and_late_at_night_they_nap_sooner() {
        let mut st = state(&["cat"]);
        st.tick(0);
        st.pets[0].plan.clear();
        st.pets[0].act = Act::Idle;
        st.pets[0].posture = Posture::Standing;
        st.pets[0].until = u64::MAX;
        let far = st.pets[0].at(0).x + 25.0;
        st.screen.caret = Some((far as u32, 3));
        st.follow(100);
        assert_eq!(st.pets[0].act, Act::Walk);
        assert!((st.pets[0].to.x - far).abs() <= 3.5, "to the caret: {:?}", st.pets[0].to);
        assert_eq!(st.sleep_after(), SLEEP_AFTER);
        st.hour = 23;
        assert_eq!(st.sleep_after(), SLEEP_AFTER_LATE);
        st.hour = 4;
        assert!(st.late());
    }

    #[test]
    fn warnings_get_a_look_and_calm_pets_linger() {
        let mut st = state(&["duck"]);
        st.tick(0);
        st.pets[0].plan.clear();
        st.pets[0].act = Act::Idle;
        st.pets[0].posture = Posture::Standing;
        st.diagnostics(0, 2, 100);
        assert_eq!(st.pets[0].act, Act::Look);
        st.pets[0].plan.clear();
        st.pets[0].act = Act::Idle;
        st.diagnostics(0, 3, 200);
        assert_eq!(st.pets[0].act, Act::Idle, "only when they appear");
        // How often they change what they're doing, over a few lives.
        let busy = |pace: f32| {
            let mut changes = 0;
            for seed in 1..9u64 {
                let mut st = state(&["cat"]);
                st.rng = (seed * 7919) | 1;
                st.pace = pace;
                let mut was = st.pets[0].act;
                for t in (0..300_000).step_by(200) {
                    st.last_typed = t;
                    st.tick(t);
                    changes += (st.pets[0].act != was) as u32;
                    was = st.pets[0].act;
                }
            }
            changes
        };
        let (calm, lively) = (busy(1.6), busy(0.6));
        assert!(calm < lively, "calm pets move less than lively ones: {calm} {lively}");
    }

    #[test]
    fn the_new_friends_do_their_own_things() {
        let mut st = state(&["owl", "fox", "snail", "cat"]);
        st.has_yarn = true;
        let mut seen = std::collections::HashSet::new();
        for t in (0..600_000).step_by(200) {
            st.last_typed = t;
            st.tick(t);
            for p in &st.pets {
                seen.insert((p.kind.clone(), p.act));
            }
            if st.yarn.is_some() {
                seen.insert(("yarn".into(), Act::Idle));
            }
        }
        for want in [("owl", Act::Hoot), ("fox", Act::Pounce), ("snail", Act::Walk), ("cat", Act::Stalk)] {
            assert!(seen.contains(&(want.0.to_string(), want.1)), "{want:?}");
        }
        assert!(seen.contains(&("yarn".to_string(), Act::Idle)), "the cat played with its yarn");
        assert!(!seen.iter().any(|(k, a)| k != "owl" && *a == Act::Hoot), "only the owl hoots");
        assert!(!seen.iter().any(|(k, a)| k == "snail" && *a == Act::Hop), "a snail doesn't hop");
    }

    #[test]
    fn late_at_night_the_owl_stays_up() {
        let mut st = state(&["owl", "cat"]);
        st.hour = 23;
        st.last_typed = 0;
        let t = SLEEP_AFTER_LATE + 30_000;
        for t in (0..t).step_by(250) {
            st.tick(t);
        }
        assert_eq!(st.pets[1].act, Act::Sleep, "the cat's asleep");
        assert_ne!(st.pets[0].posture, Posture::Lying, "the owl's up");
    }

    #[test]
    fn they_race_and_keep_warm_by_ember() {
        let mut st = state(&["duck", "ferris", "ember"]);
        st.tick(0);
        for i in 0..3 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.race(0, 1, 100);
        assert!(st.pets[..2].iter().all(|p| p.act == Act::Look && p.plan.front().is_some_and(|s| s.act == Act::Run)));
        let finish: Vec<f32> = st.pets[..2].iter().map(|p| p.plan[0].to.unwrap().x).collect();
        assert!((finish[0] - finish[1]).abs() > 1.0, "side by side at the line: {finish:?}");
        let free = [0, 1, 2];
        st.warm_by(2, &free, 200);
        let ember = st.pets[2].at(200).x;
        for i in 0..2 {
            let to = st.pets[i].to.x;
            assert!((to - ember).abs() < 12.0, "by Ember: {to} {ember}");
        }
    }

    #[test]
    fn in_a_work_round_they_keep_still_and_play_in_the_break() {
        let mut st = state(&["cat", "duck"]);
        st.pomodoro("work", 0);
        assert!(st.working);
        st.next_social = 0;
        for t in (0..60_000).step_by(200) {
            st.last_typed = t;
            st.tick(t);
            assert!(!matches!(st.pets[0].act, Act::Run | Act::Pounce), "no games in a work round");
        }
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.pomodoro("break", 60_000);
        assert!(!st.working);
        assert!(st.pets.iter().all(|p| p.act == Act::Stretch), "a stretch, and play");
    }

    #[test]
    fn what_changed_in_the_repository() {
        let repo = |branch: &str, head: &str, ahead: u32, behind: u32, conflicts: u32| Repo { branch: Some(branch.into()), head: Some(head.into()), ahead, behind, conflicts };
        use GitChange::*;
        assert_eq!(Repo::changes(&repo("main", "a", 0, 0, 0), &repo("main", "b", 1, 0, 0)), [Committed]);
        assert_eq!(Repo::changes(&repo("main", "b", 1, 0, 0), &repo("main", "b", 0, 0, 0)), [Pushed]);
        assert_eq!(Repo::changes(&repo("main", "a", 0, 3, 0), &repo("main", "d", 0, 0, 0)), [Pulled]);
        assert_eq!(Repo::changes(&repo("main", "a", 0, 0, 0), &repo("dev", "x", 0, 0, 0)), [Switched]);
        assert_eq!(Repo::changes(&repo("main", "a", 0, 0, 0), &repo("main", "a", 0, 0, 2)), [Conflicted]);
        assert_eq!(Repo::changes(&repo("main", "a", 0, 0, 2), &repo("main", "m", 1, 0, 0)), [Resolved, Committed]);
        assert!(Repo::changes(&repo("main", "a", 0, 0, 0), &repo("main", "a", 0, 0, 0)).is_empty());
    }

    #[test]
    fn a_commit_gets_confetti_and_a_conflict_sends_them_hiding() {
        let mut st = state(&["cat", "duck"]);
        st.tick(0);
        let repo = |head: &str, conflicts: u32| Some(Repo { branch: Some("main".into()), head: Some(head.into()), ahead: 1, behind: 0, conflicts });
        st.git(repo("a", 0), 100);
        assert!(st.fx.list.is_empty(), "the first look is only a look");
        st.git(repo("b", 0), 2100);
        assert!(st.fx.list.iter().any(|p| fx::CONFETTI.contains(&p.frames[0])), "confetti for a commit");
        st.git(repo("b", 2), 4100);
        for t in (4100..20_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.pets.iter().all(|p| p.act == Act::Hide), "hiding till it's resolved");
        st.git(repo("c", 0), 20_000);
        assert!(st.pets.iter().all(|p| p.act != Act::Hide), "out of hiding: {:?}", st.pets.iter().map(|p| p.act).collect::<Vec<_>>());
    }

    #[test]
    fn one_watches_a_build_dozes_if_it_drags_and_a_big_pass_is_a_party() {
        let mut st = state(&["cat", "duck"]);
        st.tick(0);
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.task_started(100);
        let watcher = st.building.unwrap().1;
        let w = st.pets.iter().position(|p| p.uid == watcher).unwrap();
        let t = 100 + BUILD_DOZE_AFTER + 20_000;
        let mut acts = Vec::new();
        for t in (100..t).step_by(250) {
            st.last_typed = t;
            st.tick(t);
            if acts.last() != Some(&st.pets[w].act) {
                acts.push(st.pets[w].act);
            }
        }
        assert_eq!(st.pets[w].act, Act::Sleep, "dozed off: {acts:?}");
        st.task_ended_with(true, Some((240, 0)), t);
        assert!(st.building.is_none());
        let confetti = st.fx.list.iter().filter(|p| fx::CONFETTI.contains(&p.frames[0])).count();
        assert!(confetti >= 24, "a party: {confetti}");
    }

    #[test]
    fn an_agents_turn_gets_a_cheer_or_a_singe() {
        let mut st = state(&["ember", "duck"]);
        st.tick(0);
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.agent_done("error", 100);
        assert!(st.fx.list.iter().any(|p| p.frames == fx::SMOKE), "Ember singes");
        assert_eq!(st.pets[1].act, Act::Look);
    }

    #[test]
    fn the_rubber_duck_listens_while_you_debug() {
        let mut st = state(&["duck", "cat"]);
        st.tick(0);
        st.debug_started(7, 1000);
        assert_eq!(st.pets[0].act, Act::Walk, "the duck comes over");
        st.debug_started(8, 1100);
        for t in (1100..20_000).step_by(250) {
            st.tick(t);
        }
        assert_eq!(st.pets[0].act, Act::Listen, "and listens");
        st.debug_paused(true, 20_000);
        assert_eq!(st.pets[0].act, Act::Quack, "a breakpoint: a quack");
        assert!(st.fx.list.iter().any(|p| p.frames == [fx::BANG]));
        for t in (20_000..40_000).step_by(250) {
            st.tick(t);
        }
        assert_eq!(st.pets[0].act, Act::Listen, "listening still");
        st.debug_ended(8, 40_000);
        assert_eq!(st.pets[0].act, Act::Listen, "a child session's end isn't the end");
        st.debug_ended(7, 40_000);
        assert_eq!(st.pets[0].act, Act::Happy, "pleased it helped");
        for t in (40_000..90_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.debugging.is_none());
    }

    #[test]
    fn back_after_a_while_they_greet_you_and_a_new_file_gets_a_look() {
        let mut st = state(&["cat", "ferris"]);
        st.last_typed = 0;
        st.tick(0);
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.came_back(30_000);
        assert!(st.pets.iter().all(|p| p.act == Act::Idle), "only after a good while");
        st.came_back(AWAY_AFTER + 1000);
        assert!(st.pets.iter().all(|p| p.act == Act::Look), "they look up");
        assert!(st.fx.list.iter().any(|p| p.frames == [fx::HEART]));
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.focus_moved(200_000);
        assert_eq!(st.pets.iter().filter(|p| p.act == Act::Look).count(), 1, "one looks round");
        for i in 0..2 {
            st.pets[i].plan.clear();
            st.pets[i].act = Act::Idle;
            st.pets[i].posture = Posture::Standing;
        }
        st.focus_moved(201_000);
        assert!(st.pets.iter().all(|p| p.act == Act::Idle), "not every time");
    }

    #[test]
    fn they_get_about() {
        let mut st = state(&["cat"]);
        st.last_typed = 0;
        let mut acts = std::collections::HashSet::new();
        let mut walking = 0;
        for t in (0..120_000).step_by(200) {
            st.last_typed = t;
            st.tick(t);
            acts.insert(st.pets[0].act);
            walking += st.pets[0].moving(t) as u32;
        }
        assert!(walking * 200 > 20_000, "on the move a good part of two minutes: {walking}");
        assert!(acts.len() >= 5, "{acts:?}");
    }

    #[test]
    fn ember_burns_on_a_failed_task_and_rises_on_the_next_pass() {
        let mut st = state(&["ember", "duck"]);
        st.clips.insert("ember".into(), HashMap::from([("burn".into(), 1.2), ("rise".into(), 1.4)]));
        st.task_ended(false, 1000);
        assert_eq!(st.pets[0].act, Act::Burn);
        assert_eq!(st.pets[0].until, 2200, "as long as its clip");
        assert_eq!(st.pets[1].act, Act::Walk, "the duck comes to listen");
        assert_eq!(st.pets[1].to.x, 23.0, "by the caret");
        st.tick(2200);
        assert_eq!((st.pets[0].act, st.pets[0].posture), (Act::Ash, Posture::Gone));
        assert!(st.parts(0).contains(&"ash".to_string()));
        // Errors don't move the ash.
        st.diagnostics(3, 0, 3000);
        assert_eq!(st.pets[0].act, Act::Ash);
        st.diagnostics(0, 0, 3500);
        st.tick(4000);
        assert!(st.fx.list.iter().any(|p| p.frames == fx::SMOKE), "smoke from the ash");
        st.task_ended(true, 5000);
        assert_eq!(st.pets[0].act, Act::Rise);
        st.tick(6400);
        assert_eq!((st.pets[0].act, st.pets[0].posture), (Act::Happy, Posture::Standing));
        assert!(!st.parts(0).contains(&"ash".to_string()));
    }

    #[test]
    fn errors_send_them_into_hiding_until_theyre_fixed() {
        let mut st = state(&["cat", "ferris"]);
        st.diagnostics(2, 0, 100);
        assert!(st.pets.iter().all(|p| p.act == Act::Hide));
        for t in (200..20_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.pets.iter().all(|p| p.act == Act::Hide), "still hiding");
        st.diagnostics(0, 0, 20_000);
        assert!(st.pets.iter().all(|p| p.act != Act::Hide), "out of hiding: {:?}", st.pets.iter().map(|p| p.act).collect::<Vec<_>>());
    }

    #[test]
    fn the_ball_is_fetched_and_brought_back() {
        let mut st = state(&["cat"]);
        st.screen.caret = Some((30, 22));
        st.last_typed = 0;
        assert!(st.throw_ball(1000));
        let ball = st.ball.clone().unwrap();
        assert_eq!(ball.fetcher, Some(1));
        assert_eq!(st.pets[0].act, Act::Look, "watching it fly");
        let mut t = 1000;
        while t < 30_000 && st.ball.as_ref().is_some_and(|b| b.dropped.is_none()) {
            t += 100;
            st.tick(t);
        }
        let ball = st.ball.as_ref().expect("still there, dropped");
        assert!(ball.dropped.is_some() && !ball.carried, "{ball:?}");
        assert!((st.pets[0].at(t).x - 34.0).abs() < 1.0, "back at the caret: {}", st.pets[0].at(t).x);
        assert_eq!(st.pets[0].act, Act::Happy);
        assert!(st.pets[0].care.xp >= 4);
        st.tick(t + 6000);
        assert!(st.ball.is_none(), "gone a while after");
    }

    #[test]
    fn the_cat_stalks_the_duck() {
        let mut st = state(&["cat", "duck"]);
        st.clips.insert("cat".into(), HashMap::from([("pounce".into(), 0.7)]));
        st.stalk(0, 1, 0);
        assert_eq!(st.pets[0].act, Act::Stalk);
        assert!(st.pets[0].to.x > 10.0 && st.pets[0].to.x < 30.0 - st.half() * 2.0);
        assert_eq!(st.pets[1].act, Act::Idle, "the duck hasn't seen yet");
        assert_eq!(st.pets[1].plan.iter().map(|s| s.act).collect::<Vec<_>>(), [Act::Jump, Act::Run, Act::Quack]);
    }

    #[test]
    fn a_visitor_sits_in_the_margin_and_follows_the_caret() {
        let mut st = state(&["cat"]);
        st.visit(0);
        assert_eq!(st.pets[0].act, Act::Hop);
        assert_eq!(st.pets[0].to.floor, Floor::Margin(22));
        assert_eq!(st.pets[0].arc, 1.6 + 4.0 * 0.3);
        st.tick(700);
        assert_eq!(st.pets[0].act, Act::SitDown);
        st.tick(1400);
        assert_eq!(st.pets[0].act, Act::Sit);
        let mut moved = screen();
        moved.caret = Some((20, 25));
        st.layout(moved, 2000);
        assert_eq!((st.pets[0].act, st.pets[0].to.floor), (Act::StandUp, Floor::Margin(22)));
        assert_eq!(st.pets[0].plan.front().and_then(|s| s.to).map(|s| s.floor), Some(Floor::Margin(25)), "on to the caret's new line");
        let mut away = screen();
        away.caret = Some((90, 25));
        st.layout(away, 3000);
        assert_eq!((st.pets[0].act, st.pets[0].to.floor), (Act::Hop, Floor::Modeline), "no room: home");
    }

    #[test]
    fn a_long_run_of_typing_sets_ember_alight() {
        let mut st = state(&["ember"]);
        for t in (1000..9000).step_by(500) {
            st.typing(t);
        }
        assert!(st.on_fire(9000));
        st.pets[0].until = 9000;
        st.pets[0].act = Act::Idle;
        st.pets[0].plan.clear();
        st.tick(9000);
        assert_eq!(st.pets[0].act, Act::Run);
        st.tick(9200);
        assert!(st.fx.list.iter().any(|p| p.frames == fx::FLAME), "flames behind");
        assert!(!st.on_fire(9000 + STREAK_GAP + 1), "a pause puts it out");
    }

    #[test]
    fn saving_teaches_them_and_a_level_unlocks_something() {
        let mut st = state(&["duck"]);
        st.pets[0].care.xp = 24;
        st.saved(1000);
        assert_eq!(st.pets[0].act, Act::Jump);
        assert_eq!(st.pets[0].care.level(), 2);
        assert_eq!(st.said, ["duck reached level 2 -- and can wear a bow now (SPC P p)"]);
        assert!(st.fx.list.iter().any(|p| fx::CONFETTI.contains(&p.frames[0])));
        st.saved(2000);
        assert_eq!(st.pets[0].care.xp, 25, "not again so soon");
    }

    #[test]
    fn they_dress_for_the_season_and_wear_what_they_unlocked() {
        let mut st = state(&["cat"]);
        st.pets[0].wearing = vec!["bow".into(), "glasses".into(), "crown".into()];
        st.pets[0].care.xp = 100;
        assert_eq!(st.parts(0), ["bow"], "glasses come at level 4, a crown at 7");
        st.seasons = true;
        st.today = 20_789; // 2 December 2026
        assert_eq!(st.parts(0), ["santa_hat"], "the season's hat in place of its own");
        st.today = 100 + 365;
        st.seasons = false;
        assert_eq!(st.parts(0), ["party_hat"], "a year since it was adopted");
    }

    #[test]
    fn the_layer_shows_each_pet_posed_and_gliding() {
        let mut st = state(&["ferris", "ember"]);
        st.plan(0, vec![Step::go(Act::Walk, Spot::new(Floor::Modeline, 30.0))], 0, true);
        let items = st.items(100, &models(), Some(99), Some(5));
        assert_eq!(items.len(), 2);
        let crab = &items[0];
        let pose = crab.pose.as_ref().unwrap();
        assert_eq!((pose.model, pose.clip.as_str(), pose.yaw, pose.style), (11, "walk", 0.0, Style::Pixel), "a crab walks sideways");
        assert_eq!((crab.id, crab.fps, crab.clickable, crab.keep_clear), (1, 12, true, true));
        assert!((crab.size - 2.2 * 0.72).abs() < 1e-6, "a crab's mostly width: not as tall");
        assert_eq!(crab.glide.as_ref().map(|g| (g.to.x, g.ease)), Some((30.0, Ease::Smooth)));
        let bird = items[1].pose.as_ref().unwrap();
        assert!((bird.glow - 0.1).abs() < 1e-6);
        st.hidden = true;
        assert!(st.items(100, &models(), None, None).is_empty());
    }

    #[test]
    fn a_new_pet_runs_in_from_the_side() {
        let mut st = state(&[]);
        st.most = 1;
        assert!(st.adopt("cat", "Tom", 0));
        assert_eq!((st.pets[0].uid, st.pets[0].act, st.pets[0].from.x), (1, Act::Run, -2.0));
        assert!(!st.adopt("duck", "Q", 0), "room for one");
        assert!(st.rehome(1).is_some());
        assert!(st.pets.is_empty());
    }
}
