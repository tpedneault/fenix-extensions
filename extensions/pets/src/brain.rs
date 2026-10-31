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
/// A long while: until something happens.
const UNTIL_TOLD: u64 = u64::MAX / 4;

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

    /// What pet `i` does when it's nothing else to do.
    fn wander(&mut self, i: usize, now: u64) -> Vec<Step> {
        let pet = &self.pets[i];
        let (kind, here, posture, happy) = (pet.kind.clone(), pet.at(now), pet.posture, pet.care.happy);
        let home = self.home_floor();
        if posture == Posture::Gone {
            return vec![Step::lasting(Act::Ash, UNTIL_TOLD)];
        }
        if self.errors > 0 {
            return vec![Step::lasting(Act::Hide, UNTIL_TOLD)];
        }
        // Up on a ledge, or visiting: back home before long.
        if here.floor != home && !matches!(here.floor, Floor::Margin(_))
            && (self.random() < 0.5 || !self.screen.has(here.floor)) {
                let x = here.x + self.between(-6.0, 6.0);
                return vec![Step::go(if kind == "ember" { Act::Fly } else { Act::Hop }, Spot::new(home, x))];
            }
        let roll = self.random();
        let seated = posture == Posture::Sitting;
        let far = self.between(4.0, 22.0) * if self.random() < 0.5 { -1.0 } else { 1.0 };
        let wide = Spot::new(here.floor, here.x + far);
        if seated && roll < 0.55 {
            return vec![Step::lasting(Act::Sit, self.between(3000.0, 8000.0) as u64)];
        }
        match roll {
            r if r < 0.32 => vec![Step::go(Act::Walk, wide)],
            r if r < 0.47 => vec![Step::lasting(Act::Sit, self.between(4000.0, 10000.0) as u64)],
            r if r < 0.57 => vec![Step::act(Act::Look)],
            r if r < 0.61 => vec![Step::act(Act::Stretch)],
            r if r < 0.66 && happy > 70.0 => vec![Step::lasting(Act::Happy, 1600)],
            r if r < 0.71 => {
                let ledges = self.screen.ledges();
                if ledges.is_empty() || here.floor != home {
                    vec![Step::go(Act::Walk, wide)]
                } else {
                    let pick = ledges[(self.random() * ledges.len() as f32) as usize % ledges.len()];
                    let x = here.x + self.between(-4.0, 4.0);
                    vec![Step::go(if kind == "ember" { Act::Fly } else { Act::Hop }, Spot::new(pick, x)), Step::lasting(Act::Sit, self.between(5000.0, 12000.0) as u64)]
                }
            }
            r if r < 0.76 && kind == "duck" => vec![Step::act(Act::Quack)],
            r if r < 0.76 && kind == "ferris" => vec![Step::lasting(Act::Snap, 1800)],
            r if r < 0.76 && kind == "ember" => vec![Step::go(Act::Fly, wide)],
            _ => vec![Step::lasting(Act::Idle, self.between(2000.0, 5000.0) as u64)],
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
    pub fn diagnostics(&mut self, errors: u32, now: u64) {
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
        // Nobody's typed in a while: a nap, all together.
        if now.saturating_sub(self.last_typed) > SLEEP_AFTER && self.errors == 0 {
            self.nap(now);
        }
        for i in 0..self.pets.len() {
            if now >= self.pets[i].until {
                self.advance(i, now);
            }
        }
        self.puffs(now);
        if now >= self.next_social {
            self.next_social = now + self.between(40_000.0, 120_000.0) as u64;
            self.social(now);
        }
        if now >= self.next_visit {
            self.next_visit = now + self.between(120_000.0, 300_000.0) as u64;
            if self.visits {
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
        let awake: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].posture != Posture::Lying && !self.pets[i].gone() && !matches!(self.pets[i].act, Act::LieDown) && !self.pets[i].plan.iter().any(|s| s.act == Act::Sleep)).collect();
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
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].free() && self.pets[i].posture != Posture::Lying && self.pets[i].at(now).floor == self.home_floor()).collect();
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
        let a = free[(self.random() * free.len() as f32) as usize % free.len()];
        let b = free.iter().copied().find(|&b| b != a).unwrap_or(a);
        self.chase(a, b, now);
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
        let free: Vec<usize> = (0..self.pets.len()).filter(|&i| self.pets[i].free() && self.pets[i].posture != Posture::Lying && self.pets[i].at(now).floor == home).collect();
        if free.is_empty() {
            return;
        }
        let i = free[(self.random() * free.len() as f32) as usize % free.len()];
        let Some((l, r)) = self.screen.span(margin) else { return };
        let up = if self.pets[i].kind == "ember" { Act::Fly } else { Act::Hop };
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
            let up = if pet.kind == "ember" { Act::Fly } else { Act::Hop };
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
        st.diagnostics(3, 3000);
        assert_eq!(st.pets[0].act, Act::Ash);
        st.diagnostics(0, 3500);
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
        st.diagnostics(2, 100);
        assert!(st.pets.iter().all(|p| p.act == Act::Hide));
        for t in (200..20_000).step_by(250) {
            st.tick(t);
        }
        assert!(st.pets.iter().all(|p| p.act == Act::Hide), "still hiding");
        st.diagnostics(0, 20_000);
        assert!(st.pets.iter().all(|p| p.act == Act::Stretch));
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
