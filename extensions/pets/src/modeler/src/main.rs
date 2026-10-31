//! The pets' models: low-poly toys built from simple shapes, each part
//! bound whole to a joint, with the clips the extension plays. Run it
//! here (`cargo run --release`) and it writes `../../models/<name>.glb`.
//!
//! Every pet faces +Z with its feet at y = 0, and has the same clips --
//! idle, walk, run, sit_down, sit, stand_up, lie_down, sleep, jump, happy,
//! hide, look, stretch, listen -- plus its own (Ember's burn, ash, rise and
//! fly; the cat's stalk and pounce; Ferris's snap; the duck's quack). Its
//! accessories are optional parts (`opt:party_hat`, ...), worn when the
//! extension asks.

use std::f32::consts::TAU;

use fenix_model::build::{Builder, Mesh, Track};
use glam::{Quat, Vec3};

type Rgba = [f32; 4];

const SEGMENTS: u32 = 14;
const RINGS: u32 = 9;

const fn rgb(r: f32, g: f32, b: f32) -> Rgba {
    [r, g, b, 1.0]
}

const BLACK: Rgba = rgb(0.09, 0.08, 0.1);
const WHITE: Rgba = rgb(0.98, 0.98, 0.96);

fn rx(deg: f32) -> Quat {
    Quat::from_rotation_x(deg.to_radians())
}
fn ry(deg: f32) -> Quat {
    Quat::from_rotation_y(deg.to_radians())
}
fn rz(deg: f32) -> Quat {
    Quat::from_rotation_z(deg.to_radians())
}

/// 0 before `a`, 1 after `b`, easing between.
fn ramp(p: f32, a: f32, b: f32) -> f32 {
    let t = ((p - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Up at the middle of `a`..`b`, 0 outside it.
fn bump(p: f32, a: f32, b: f32) -> f32 {
    if p <= a || p >= b {
        0.0
    } else {
        (std::f32::consts::PI * (p - a) / (b - a)).sin()
    }
}

fn wave(p: f32, times: f32, phase: f32) -> f32 {
    (TAU * (p * times + phase)).sin()
}

/// A model being built: its joints (placed where they are, from the
/// origin) and the shapes bound to them.
struct Rig {
    b: Builder,
    local: Vec<Vec3>,
    world: Vec<Vec3>,
    mesh: Mesh,
}

impl Rig {
    fn new() -> Rig {
        Rig { b: Builder::new(), local: Vec::new(), world: Vec::new(), mesh: Mesh::default() }
    }

    fn joint(&mut self, name: &str, parent: Option<usize>, at: Vec3) -> usize {
        let local = at - parent.map_or(Vec3::ZERO, |p| self.world[p]);
        let j = self.b.joint(name, parent, local.to_array());
        self.local.push(local);
        self.world.push(at);
        j
    }

    fn ball(&mut self, j: usize, c: Vec3, r: Vec3, color: Rgba) {
        self.mesh.ellipsoid(c.to_array(), r.to_array(), Quat::IDENTITY, color, j, RINGS, SEGMENTS);
    }

    fn ball_turned(&mut self, j: usize, c: Vec3, r: Vec3, turn: Quat, color: Rgba) {
        self.mesh.ellipsoid(c.to_array(), r.to_array(), turn, color, j, RINGS, SEGMENTS);
    }

    fn cone(&mut self, j: usize, base: Vec3, tip: Vec3, radius: f32, color: Rgba) {
        self.mesh.cone(base.to_array(), tip.to_array(), radius, color, j, SEGMENTS);
    }

    fn tube(&mut self, j: usize, a: Vec3, b: Vec3, ra: f32, rb: f32, color: Rgba) {
        self.mesh.frustum(a.to_array(), b.to_array(), ra, rb, color, j, SEGMENTS);
    }

    fn cube(&mut self, j: usize, c: Vec3, half: Vec3, turn: Quat, color: Rgba) {
        self.mesh.cuboid_turned(c.to_array(), half.to_array(), turn, color, j);
    }

    /// Two eyes, each its own joint so they can blink: dark, with a
    /// glint up and to the side.
    fn eyes(&mut self, head: usize, at: [Vec3; 2], r: Vec3) -> [usize; 2] {
        let mut out = [0; 2];
        for (i, c) in at.into_iter().enumerate() {
            let j = self.joint(if i == 0 { "eye_l" } else { "eye_r" }, Some(head), c);
            self.ball(j, c, r, BLACK);
            self.ball(j, c + Vec3::new(r.x * 0.35 * if c.x > 0.0 { -1.0 } else { 1.0 }, r.y * 0.35, r.z * 0.7), Vec3::splat(r.x * 0.32), WHITE);
            out[i] = j;
        }
        out
    }

    fn finish(mut self, clips: Vec<(String, Vec<Track>)>) -> Vec<u8> {
        self.b.mesh(std::mem::take(&mut self.mesh));
        for (name, tracks) in clips {
            self.b.clip(&name, tracks);
        }
        self.b.glb()
    }
}

/// One clip: `n` steps over `seconds`, a function of how far through
/// (0 to 1) for each joint it moves.
struct Anim<'a> {
    rig: &'a Rig,
    seconds: f32,
    n: usize,
    tracks: Vec<Track>,
}

impl<'a> Anim<'a> {
    fn new(rig: &'a Rig, seconds: f32, n: usize) -> Anim<'a> {
        Anim { rig, seconds, n, tracks: Vec::new() }
    }

    fn times(&self) -> Vec<f32> {
        (0..=self.n).map(|i| self.seconds * i as f32 / self.n as f32).collect()
    }

    fn steps(&self) -> impl Iterator<Item = f32> + '_ {
        (0..=self.n).map(|i| i as f32 / self.n as f32)
    }

    fn turn(&mut self, j: usize, f: impl Fn(f32) -> Quat) -> &mut Self {
        let values = self.steps().map(|p| f(p).normalize().to_array()).collect();
        self.tracks.push(Track::rotation(j, self.times(), values));
        self
    }

    /// Moved by `f`'s offset from where it rests.
    fn shift(&mut self, j: usize, f: impl Fn(f32) -> Vec3) -> &mut Self {
        let rest = self.rig.local[j];
        let values = self.steps().map(|p| (rest + f(p)).to_array()).collect();
        self.tracks.push(Track::translation(j, self.times(), values));
        self
    }

    fn scale(&mut self, j: usize, f: impl Fn(f32) -> Vec3) -> &mut Self {
        let values = self.steps().map(|p| f(p).to_array()).collect();
        self.tracks.push(Track::scale(j, self.times(), values));
        self
    }

    fn done(&mut self, name: &str) -> (String, Vec<Track>) {
        (name.to_string(), std::mem::take(&mut self.tracks))
    }
}

/// The joints the shared clips move.
struct Parts {
    root: usize,
    body: usize,
    head: usize,
    eyes: [usize; 2],
    /// Each leg, and when in a stride it swings forward (0 to 1).
    legs: Vec<(usize, f32)>,
    tail: Vec<usize>,
    /// Each wing (or arm), and its side: 1 or -1.
    wings: Vec<(usize, f32)>,
    /// Legs walk sideways, swinging out rather than forward (a crab).
    sideways: bool,
}

/// Where an accessory goes on a pet.
struct Fit {
    head: usize,
    /// What its eyes are on: what glasses follow.
    face: usize,
    top: Vec3,
    /// How big the head is: what a hat's size follows.
    size: f32,
    eyes: [Vec3; 2],
    neck: usize,
    neck_at: Vec3,
    neck_r: f32,
}

/// Hats, a crown, a bow, a scarf, glasses and a pumpkin: each an
/// optional part (`opt:<name>`), worn when asked.
fn accessories(rig: &mut Rig, fit: &Fit) {
    let s = fit.size;
    let up = Vec3::Y;
    let top = fit.top;

    let j = rig.joint("opt:party_hat", Some(fit.head), top);
    rig.cone(j, top - up * 0.02 * s, top + Vec3::new(0.0, 0.26, -0.03) * s, 0.09 * s, rgb(0.95, 0.35, 0.65));
    rig.tube(j, top - up * 0.01 * s, top + up * 0.03 * s, 0.092 * s, 0.075 * s, rgb(0.35, 0.75, 1.0));
    rig.ball(j, top + Vec3::new(0.0, 0.27, -0.03) * s, Vec3::splat(0.035 * s), rgb(1.0, 0.85, 0.2));

    let j = rig.joint("opt:santa_hat", Some(fit.head), top);
    rig.cone(j, top, top + Vec3::new(0.0, 0.2, -0.12) * s, 0.11 * s, rgb(0.85, 0.1, 0.12));
    rig.tube(j, top - up * 0.025 * s, top + up * 0.03 * s, 0.125 * s, 0.12 * s, WHITE);
    rig.ball(j, top + Vec3::new(0.0, 0.19, -0.15) * s, Vec3::splat(0.045 * s), WHITE);

    let j = rig.joint("opt:crown", Some(fit.head), top);
    let gold = rgb(1.0, 0.78, 0.15);
    rig.tube(j, top - up * 0.02 * s, top + up * 0.06 * s, 0.085 * s, 0.1 * s, gold);
    for k in 0..5 {
        let a = TAU * k as f32 / 5.0;
        let rim = top + Vec3::new(a.sin() * 0.09, 0.06, a.cos() * 0.09) * s;
        rig.cone(j, rim, rim + up * 0.07 * s, 0.025 * s, gold);
    }
    rig.ball(j, top + Vec3::new(0.0, 0.025, 0.1) * s, Vec3::splat(0.022 * s), rgb(0.9, 0.1, 0.3));

    let j = rig.joint("opt:bow", Some(fit.head), top);
    let pink = rgb(1.0, 0.45, 0.65);
    let side = Vec3::new(0.09, 0.02, 0.0) * s;
    rig.ball_turned(j, top + side, Vec3::new(0.07, 0.045, 0.03) * s, rz(-20.0), pink);
    rig.ball_turned(j, top - Vec3::new(side.x, -side.y, 0.0), Vec3::new(0.07, 0.045, 0.03) * s, rz(20.0), pink);
    rig.ball(j, top + up * 0.02 * s, Vec3::splat(0.035 * s), rgb(0.9, 0.3, 0.5));

    let j = rig.joint("opt:scarf", Some(fit.neck), fit.neck_at);
    let red = rgb(0.85, 0.15, 0.2);
    rig.tube(j, fit.neck_at - up * 0.035, fit.neck_at + up * 0.035, fit.neck_r, fit.neck_r * 0.95, red);
    rig.cube(j, fit.neck_at + Vec3::new(fit.neck_r * 0.5, -0.07, fit.neck_r * 0.8), Vec3::new(0.035, 0.07, 0.015), rz(-10.0), red);

    let j = rig.joint("opt:glasses", Some(fit.face), (fit.eyes[0] + fit.eyes[1]) / 2.0);
    for eye in fit.eyes {
        rig.ball(j, eye + Vec3::new(0.0, 0.0, 0.03), Vec3::new(0.055, 0.04, 0.012) * s.sqrt(), rgb(0.08, 0.08, 0.12));
    }
    let mid = (fit.eyes[0] + fit.eyes[1]) / 2.0 + Vec3::new(0.0, 0.01, 0.03);
    rig.cube(j, mid, Vec3::new((fit.eyes[0].x - fit.eyes[1].x).abs() / 2.0 - 0.04, 0.008, 0.008), Quat::IDENTITY, rgb(0.08, 0.08, 0.12));

    let j = rig.joint("opt:pumpkin", Some(fit.head), top);
    rig.ball(j, top + up * 0.06 * s, Vec3::new(0.12, 0.085, 0.12) * s, rgb(1.0, 0.5, 0.08));
    rig.tube(j, top + up * 0.13 * s, top + up * 0.19 * s, 0.018 * s, 0.014 * s, rgb(0.25, 0.55, 0.15));
}

/// The clips every pet has, made from its parts.
fn shared(rig: &Rig, p: &Parts) -> Vec<(String, Vec<Track>)> {
    let mut out = Vec::new();
    let one = Vec3::ONE;
    // A crab's legs swing out to the side; everyone else's, forward.
    let legs = |a: &mut Anim, swing: f32, p: &Parts| {
        for &(leg, phase) in &p.legs {
            let side = if rig.world[leg].x > 0.0 { 1.0 } else { -1.0 };
            a.turn(leg, move |q| if p.sideways { rz(swing * wave(q, 1.0, phase) * side) } else { rx(swing * wave(q, 1.0, phase)) });
        }
    };
    let blink = |a: &mut Anim, closed: &dyn Fn(f32) -> f32| {
        for &eye in &p.eyes {
            a.scale(eye, |q| Vec3::new(1.0, 1.0 - 0.85 * closed(q), 1.0));
        }
    };
    let tail = |a: &mut Anim, amp: f32, times: f32| {
        for (k, &t) in p.tail.iter().enumerate() {
            let k = k as f32;
            a.turn(t, |q| ry(amp * wave(q, times, -0.12 * k)) * rx(-4.0 * wave(q, times, 0.25 - 0.1 * k)));
        }
    };

    // Breathing, a look about, a blink.
    let mut a = Anim::new(rig, 3.2, 32);
    a.scale(p.body, |q| Vec3::new(1.0 + 0.015 * wave(q, 2.0, 0.0), 1.0 + 0.03 * wave(q, 2.0, 0.0), 1.0));
    a.turn(p.head, |q| ry(10.0 * wave(q, 1.0, 0.0) * ramp(q, 0.2, 0.5)) * rx(3.0 * wave(q, 2.0, 0.25)));
    blink(&mut a, &|q| bump(q, 0.8, 0.88));
    tail(&mut a, 14.0, 1.0);
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 4.0 * wave(q, 2.0, 0.0)));
    }
    out.push(a.done("idle"));

    // A stride.
    let mut a = Anim::new(rig, 0.8, 16);
    legs(&mut a, 28.0, p);
    a.shift(p.root, |q| Vec3::new(0.0, 0.018 * wave(q, 2.0, 0.0).abs(), 0.0));
    a.turn(p.body, |q| rz(if p.sideways { 0.0 } else { 3.0 * wave(q, 1.0, 0.0) }));
    a.turn(p.head, |q| rx(4.0 * wave(q, 2.0, 0.1)));
    tail(&mut a, 18.0, 1.0);
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 10.0 * wave(q, 2.0, 0.0)));
    }
    out.push(a.done("walk"));

    // A dash: longer strides, leaning in.
    let mut a = Anim::new(rig, 0.42, 12);
    legs(&mut a, 45.0, p);
    a.shift(p.root, |q| Vec3::new(0.0, 0.04 * wave(q, 2.0, 0.0).abs(), 0.0));
    a.turn(p.body, |q| rx(if p.sideways { 0.0 } else { 8.0 }) * rx(3.0 * wave(q, 2.0, 0.0)));
    a.turn(p.head, |_| rx(-6.0));
    tail(&mut a, 8.0, 2.0);
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * (25.0 + 25.0 * wave(q, 2.0, 0.0))));
    }
    out.push(a.done("run"));

    // Sitting: rear down, nose up, the back legs folded.
    let sit = |a: &mut Anim, k: &dyn Fn(f32) -> f32| {
        let k = |q: f32| k(q);
        a.shift(p.root, |q| Vec3::new(0.0, -0.06 * k(q), 0.0));
        a.turn(p.body, |q| rx(-16.0 * k(q)));
        a.turn(p.head, |q| rx(14.0 * k(q)));
        for &(leg, _) in &p.legs {
            let back = rig.world[leg].z < 0.0;
            a.turn(leg, move |q| if back { rx(-50.0 * k(q)) } else { rx(16.0 * k(q)) });
        }
    };
    let mut a = Anim::new(rig, 0.45, 9);
    sit(&mut a, &|q| ramp(q, 0.0, 1.0));
    out.push(a.done("sit_down"));
    let mut a = Anim::new(rig, 0.4, 8);
    sit(&mut a, &|q| 1.0 - ramp(q, 0.0, 1.0));
    out.push(a.done("stand_up"));
    let mut a = Anim::new(rig, 3.0, 30);
    sit(&mut a, &|_| 1.0);
    a.scale(p.body, |q| Vec3::new(1.0, 1.0 + 0.025 * wave(q, 2.0, 0.0), 1.0));
    blink(&mut a, &|q| bump(q, 0.55, 0.62));
    tail(&mut a, 22.0, 1.0);
    out.push(a.done("sit"));

    // Lying down, and asleep: low, legs tucked, head down, eyes shut.
    let lie = |a: &mut Anim, k: &dyn Fn(f32) -> f32| {
        let k = |q: f32| k(q);
        a.shift(p.root, |q| Vec3::new(0.0, -0.13 * k(q), 0.0));
        a.turn(p.head, |q| rx(22.0 * k(q)) * rz(10.0 * k(q)));
        for &(leg, _) in &p.legs {
            let side = if rig.world[leg].x > 0.0 { 1.0 } else { -1.0 };
            a.turn(leg, move |q| if p.sideways { rz(side * 40.0 * k(q)) } else { rx(-75.0 * k(q)) });
        }
        for &(w, side) in &p.wings {
            a.turn(w, move |q| rz(-side * 6.0 * k(q)));
        }
    };
    let mut a = Anim::new(rig, 0.7, 14);
    lie(&mut a, &|q| ramp(q, 0.0, 1.0));
    blink(&mut a, &|q| ramp(q, 0.5, 1.0));
    out.push(a.done("lie_down"));
    let mut a = Anim::new(rig, 0.6, 12);
    lie(&mut a, &|q| 1.0 - ramp(q, 0.2, 1.0));
    blink(&mut a, &|q| 1.0 - ramp(q, 0.0, 0.3));
    out.push(a.done("wake"));
    let mut a = Anim::new(rig, 3.6, 36);
    lie(&mut a, &|_| 1.0);
    a.scale(p.body, |q| Vec3::new(1.0 + 0.03 * wave(q, 1.0, 0.0), 1.0 + 0.05 * wave(q, 1.0, 0.0), 1.0));
    blink(&mut a, &|_| 1.0);
    tail(&mut a, 4.0, 1.0);
    out.push(a.done("sleep"));

    // A hop: crouch, stretch up, tuck, land and squash.
    let mut a = Anim::new(rig, 0.6, 18);
    a.scale(p.root, |q| {
        let crouch = bump(q, 0.0, 0.28) + bump(q, 0.8, 1.0) * 0.8;
        let stretch = bump(q, 0.22, 0.6);
        Vec3::new(1.0 + 0.12 * crouch - 0.06 * stretch, 1.0 - 0.2 * crouch + 0.14 * stretch, 1.0 + 0.12 * crouch - 0.06 * stretch)
    });
    for &(leg, _) in &p.legs {
        let back = rig.world[leg].z < 0.0;
        a.turn(leg, move |q| rx(bump(q, 0.3, 0.8) * if back { 35.0 } else { -35.0 }));
    }
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 60.0 * bump(q, 0.25, 0.75)));
    }
    a.turn(p.head, |q| rx(-12.0 * bump(q, 0.2, 0.7)));
    out.push(a.done("jump"));

    // Glad: bouncing, wiggling, wagging.
    let mut a = Anim::new(rig, 0.8, 16);
    a.shift(p.root, |q| Vec3::new(0.0, 0.07 * wave(q, 2.0, 0.0).abs(), 0.0));
    a.turn(p.body, |q| ry(14.0 * wave(q, 1.0, 0.0)));
    a.turn(p.head, |q| rz(12.0 * wave(q, 1.0, 0.25)) * rx(-8.0));
    tail(&mut a, 35.0, 4.0);
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * (40.0 + 30.0 * wave(q, 4.0, 0.0))));
    }
    blink(&mut a, &|q| 0.5 * bump(q, 0.0, 0.5) + 0.5 * bump(q, 0.5, 1.0));
    out.push(a.done("happy"));

    // Hiding: low and small, a shiver.
    let mut a = Anim::new(rig, 1.0, 24);
    a.scale(p.root, |_| Vec3::new(1.08, 0.78, 1.0));
    a.turn(p.body, |q| rz(2.5 * wave(q, 8.0, 0.0)));
    a.turn(p.head, |_| rx(26.0));
    for &(w, side) in &p.wings {
        a.turn(w, move |_| rz(-side * 8.0));
    }
    out.push(a.done("hide"));

    // A look round: left, back, right.
    let mut a = Anim::new(rig, 2.4, 24);
    a.turn(p.head, |q| ry(42.0 * (ramp(q, 0.05, 0.25) - ramp(q, 0.4, 0.6)) - 42.0 * (ramp(q, 0.6, 0.75) - ramp(q, 0.85, 1.0))) * rx(-6.0 * bump(q, 0.0, 1.0)));
    blink(&mut a, &|q| bump(q, 0.48, 0.54));
    tail(&mut a, 10.0, 1.0);
    out.push(a.done("look"));

    // A stretch: front down and forward, rear up, then back.
    let mut a = Anim::new(rig, 1.3, 26);
    let k = |q: f32| ramp(q, 0.05, 0.4) - ramp(q, 0.7, 1.0);
    a.turn(p.body, move |q| rx(14.0 * k(q)));
    a.shift(p.root, move |q| Vec3::new(0.0, -0.03 * k(q), 0.0));
    a.turn(p.head, move |q| rx(-20.0 * k(q)));
    for &(leg, _) in &p.legs {
        let front = rig.world[leg].z > 0.0;
        a.turn(leg, move |q| rx(if front { -45.0 * k(q) } else { 8.0 * k(q) }));
    }
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 70.0 * k(q)));
    }
    blink(&mut a, &move |q| k(q));
    a.scale(p.root, |_| one);
    out.push(a.done("stretch"));

    // Listening: head tilted one way then the other, nodding.
    let mut a = Anim::new(rig, 1.8, 24);
    a.turn(p.head, |q| rz(14.0 * wave(q, 1.0, 0.0)) * rx(6.0 * wave(q, 3.0, 0.0)));
    blink(&mut a, &|q| bump(q, 0.3, 0.36));
    tail(&mut a, 10.0, 1.0);
    out.push(a.done("listen"));
    out
}

fn cat() -> Vec<u8> {
    let orange = rgb(0.95, 0.56, 0.24);
    let dark = rgb(0.78, 0.38, 0.12);
    let cream = rgb(1.0, 0.89, 0.72);
    let pink = rgb(0.98, 0.55, 0.62);
    let mut r = Rig::new();
    let root = r.joint("root", None, Vec3::ZERO);
    let body = r.joint("body", Some(root), Vec3::new(0.0, 0.4, 0.0));
    r.ball(body, Vec3::new(0.0, 0.4, 0.0), Vec3::new(0.19, 0.17, 0.33), orange);
    r.ball(body, Vec3::new(0.0, 0.35, 0.03), Vec3::new(0.15, 0.13, 0.27), cream);
    for z in [-0.16f32, 0.0, 0.15] {
        r.ball(body, Vec3::new(0.0, 0.545, z), Vec3::new(0.15, 0.03, 0.045), dark);
    }
    let head = r.joint("head", Some(body), Vec3::new(0.0, 0.56, 0.27));
    r.ball(head, Vec3::new(0.0, 0.67, 0.34), Vec3::new(0.2, 0.175, 0.18), orange);
    r.ball(head, Vec3::new(0.0, 0.6, 0.47), Vec3::new(0.105, 0.07, 0.065), cream);
    r.ball(head, Vec3::new(0.0, 0.64, 0.528), Vec3::new(0.026, 0.018, 0.015), pink);
    r.ball(head, Vec3::new(0.0, 0.79, 0.4), Vec3::new(0.06, 0.02, 0.05), dark);
    for x in [0.11f32, -0.11] {
        let s = x.signum();
        r.cone(head, Vec3::new(x, 0.78, 0.33), Vec3::new(x + 0.04 * s, 0.94, 0.31), 0.068, orange);
        r.cone(head, Vec3::new(x, 0.79, 0.36), Vec3::new(x + 0.035 * s, 0.9, 0.345), 0.04, pink);
        // Whiskers, a few thin boxes each side.
        for (k, tilt) in [8.0f32, -6.0].into_iter().enumerate() {
            r.cube(head, Vec3::new(x * 1.6, 0.6 + 0.022 * k as f32, 0.47), Vec3::new(0.065, 0.0035, 0.0035), rz(tilt * s), WHITE);
        }
    }
    let eyes = r.eyes(head, [Vec3::new(0.078, 0.69, 0.49), Vec3::new(-0.078, 0.69, 0.49)], Vec3::new(0.034, 0.045, 0.022));
    let mut legs = Vec::new();
    for (name, x, z, phase) in [("leg_fl", 0.1f32, 0.2f32, 0.0f32), ("leg_fr", -0.1, 0.2, 0.5), ("leg_bl", 0.1, -0.2, 0.5), ("leg_br", -0.1, -0.2, 0.0)] {
        let j = r.joint(name, Some(body), Vec3::new(x, 0.33, z));
        r.tube(j, Vec3::new(x, 0.35, z), Vec3::new(x, 0.05, z), 0.056, 0.05, orange);
        r.ball(j, Vec3::new(x, 0.035, z + 0.025), Vec3::new(0.062, 0.036, 0.078), cream);
        legs.push((j, phase));
    }
    let t1 = r.joint("tail1", Some(body), Vec3::new(0.0, 0.48, -0.3));
    let t2 = r.joint("tail2", Some(t1), Vec3::new(0.0, 0.6, -0.39));
    let t3 = r.joint("tail3", Some(t2), Vec3::new(0.0, 0.74, -0.43));
    r.tube(t1, Vec3::new(0.0, 0.48, -0.3), Vec3::new(0.0, 0.6, -0.39), 0.046, 0.042, orange);
    r.ball(t2, Vec3::new(0.0, 0.6, -0.39), Vec3::splat(0.043), orange);
    r.tube(t2, Vec3::new(0.0, 0.6, -0.39), Vec3::new(0.0, 0.74, -0.43), 0.042, 0.038, dark);
    r.ball(t3, Vec3::new(0.0, 0.74, -0.43), Vec3::splat(0.039), dark);
    r.tube(t3, Vec3::new(0.0, 0.74, -0.43), Vec3::new(0.0, 0.85, -0.4), 0.038, 0.03, orange);
    r.ball(t3, Vec3::new(0.0, 0.855, -0.4), Vec3::splat(0.032), cream);

    accessories(&mut r, &Fit { head, face: head, top: Vec3::new(0.0, 0.83, 0.32), size: 1.0, eyes: [Vec3::new(0.078, 0.69, 0.49), Vec3::new(-0.078, 0.69, 0.49)], neck: body, neck_at: Vec3::new(0.0, 0.55, 0.27), neck_r: 0.13 });
    let p = Parts { root, body, head, eyes, legs, tail: vec![t1, t2, t3], wings: Vec::new(), sideways: false };
    let mut clips = shared(&r, &p);

    // Stalking: low, slow, the tail's tip twitching; then the pounce.
    let mut a = Anim::new(&r, 1.2, 24);
    a.shift(root, |_| Vec3::new(0.0, -0.08, 0.0));
    a.turn(head, |q| rx(10.0) * ry(4.0 * wave(q, 1.0, 0.0)));
    for &(leg, phase) in &p.legs {
        a.turn(leg, move |q| rx(14.0 * wave(q, 1.0, phase)));
    }
    a.turn(t3, |q| ry(30.0 * wave(q, 3.0, 0.0)));
    a.turn(t1, |_| rx(40.0));
    clips.push(a.done("stalk"));
    let mut a = Anim::new(&r, 0.7, 21);
    a.shift(root, |q| Vec3::new(0.0, -0.08 * (1.0 - ramp(q, 0.2, 0.35)) + 0.05 * bump(q, 0.35, 0.8), 0.0));
    a.turn(body, |q| rx(-12.0 * bump(q, 0.3, 0.7)));
    for &(leg, _) in &p.legs {
        let front = r.world[leg].z > 0.0;
        a.turn(leg, move |q| rx(if front { -60.0 } else { 45.0 } * bump(q, 0.3, 0.85)));
    }
    a.turn(t1, |q| rx(30.0 * (1.0 - ramp(q, 0.2, 0.5))));
    clips.push(a.done("pounce"));
    r.finish(clips)
}

fn duck() -> Vec<u8> {
    let yellow = rgb(1.0, 0.84, 0.22);
    let shade = rgb(0.98, 0.74, 0.14);
    let beak = rgb(1.0, 0.5, 0.1);
    let mut r = Rig::new();
    let root = r.joint("root", None, Vec3::ZERO);
    let body = r.joint("body", Some(root), Vec3::new(0.0, 0.3, 0.0));
    r.ball(body, Vec3::new(0.0, 0.31, -0.03), Vec3::new(0.27, 0.22, 0.32), yellow);
    r.ball(body, Vec3::new(0.0, 0.33, 0.13), Vec3::new(0.22, 0.2, 0.2), yellow);
    r.cone(body, Vec3::new(0.0, 0.38, -0.27), Vec3::new(0.0, 0.54, -0.42), 0.11, yellow);
    let head = r.joint("head", Some(body), Vec3::new(0.0, 0.48, 0.13));
    r.ball(head, Vec3::new(0.0, 0.67, 0.16), Vec3::splat(0.175), yellow);
    r.ball(head, Vec3::new(0.0, 0.625, 0.33), Vec3::new(0.105, 0.04, 0.095), beak);
    r.ball(head, Vec3::new(0.0, 0.598, 0.31), Vec3::new(0.088, 0.03, 0.08), rgb(0.92, 0.4, 0.06));
    r.cone(head, Vec3::new(0.0, 0.82, 0.15), Vec3::new(0.0, 0.9, 0.09), 0.04, yellow);
    let eyes = r.eyes(head, [Vec3::new(0.085, 0.71, 0.295), Vec3::new(-0.085, 0.71, 0.295)], Vec3::new(0.03, 0.04, 0.02));
    let mut wings = Vec::new();
    for (name, x) in [("wing_l", 0.24f32), ("wing_r", -0.24)] {
        let j = r.joint(name, Some(body), Vec3::new(x, 0.4, 0.03));
        r.ball_turned(j, Vec3::new(x * 1.06, 0.33, -0.05), Vec3::new(0.055, 0.13, 0.2), rx(-15.0) * rz(-8.0 * x.signum()), shade);
        wings.push((j, x.signum()));
    }
    let mut legs = Vec::new();
    for (name, x, phase) in [("foot_l", 0.1f32, 0.0f32), ("foot_r", -0.1, 0.5)] {
        let j = r.joint(name, Some(body), Vec3::new(x, 0.12, 0.04));
        r.tube(j, Vec3::new(x, 0.12, 0.04), Vec3::new(x, 0.03, 0.07), 0.025, 0.022, beak);
        r.ball(j, Vec3::new(x, 0.02, 0.12), Vec3::new(0.075, 0.02, 0.095), beak);
        legs.push((j, phase));
    }
    accessories(&mut r, &Fit { head, face: head, top: Vec3::new(0.0, 0.83, 0.15), size: 0.95, eyes: [Vec3::new(0.085, 0.71, 0.295), Vec3::new(-0.085, 0.71, 0.295)], neck: body, neck_at: Vec3::new(0.0, 0.5, 0.14), neck_r: 0.14 });
    let p = Parts { root, body, head, eyes, legs, tail: Vec::new(), wings, sideways: false };
    let mut clips = shared(&r, &p);
    // A waddle in place of the walk: rocking side to side.
    if let Some(walk) = clips.iter().position(|(n, _)| n == "walk") {
        clips.remove(walk);
    }
    let mut a = Anim::new(&r, 0.7, 14);
    a.turn(body, |q| rz(9.0 * wave(q, 1.0, 0.0)));
    a.shift(root, |q| Vec3::new(0.0, 0.015 * wave(q, 2.0, 0.0).abs(), 0.0));
    for &(leg, phase) in &p.legs {
        a.turn(leg, move |q| rx(30.0 * wave(q, 1.0, phase)));
    }
    a.turn(head, |q| rz(-6.0 * wave(q, 1.0, 0.0)));
    clips.push(a.done("walk"));
    // A quack: head bobbing, wings out.
    let mut a = Anim::new(&r, 0.5, 12);
    a.turn(head, |q| rx(-12.0 * bump(q, 0.0, 0.5) + 6.0 * bump(q, 0.5, 1.0)));
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 35.0 * bump(q, 0.0, 0.6)));
    }
    a.scale(root, |q| Vec3::new(1.0, 1.0 + 0.06 * bump(q, 0.0, 0.4), 1.0));
    clips.push(a.done("quack"));
    r.finish(clips)
}

fn ferris() -> Vec<u8> {
    let shell = rgb(0.97, 0.3, 0.02);
    let deep = rgb(0.82, 0.2, 0.0);
    let mut r = Rig::new();
    let root = r.joint("root", None, Vec3::ZERO);
    let body = r.joint("body", Some(root), Vec3::new(0.0, 0.28, 0.0));
    r.ball(body, Vec3::new(0.0, 0.28, 0.0), Vec3::new(0.37, 0.16, 0.25), shell);
    r.ball(body, Vec3::new(0.0, 0.34, -0.02), Vec3::new(0.3, 0.1, 0.19), rgb(1.0, 0.42, 0.1));
    r.ball(body, Vec3::new(0.0, 0.23, 0.235), Vec3::new(0.07, 0.014, 0.012), BLACK);
    // Its face: the eyes on their stalks, which look about and nod
    // without tipping the shell.
    let head = r.joint("face", Some(body), Vec3::new(0.0, 0.38, 0.12));
    let mut eye_at = [Vec3::ZERO; 2];
    let mut eye_joints = Vec::new();
    for (name, x) in [("stalk_l", 0.1f32), ("stalk_r", -0.1)] {
        let j = r.joint(name, Some(head), Vec3::new(x, 0.38, 0.12));
        r.tube(j, Vec3::new(x, 0.38, 0.12), Vec3::new(x * 1.15, 0.54, 0.14), 0.026, 0.022, shell);
        r.ball(j, Vec3::new(x * 1.15, 0.585, 0.15), Vec3::splat(0.065), WHITE);
        eye_joints.push(j);
        eye_at[usize::from(x < 0.0)] = Vec3::new(x * 1.15, 0.59, 0.2);
    }
    let pupil_l = r.joint("eye_l", Some(eye_joints[0]), eye_at[0]);
    r.ball(pupil_l, eye_at[0], Vec3::new(0.032, 0.038, 0.02), BLACK);
    let pupil_r = r.joint("eye_r", Some(eye_joints[1]), eye_at[1]);
    r.ball(pupil_r, eye_at[1], Vec3::new(0.032, 0.038, 0.02), BLACK);
    let mut wings = Vec::new();
    let mut pincers = Vec::new();
    for (name, x) in [("arm_l", 0.3f32), ("arm_r", -0.3)] {
        let s = x.signum();
        let j = r.joint(name, Some(body), Vec3::new(x, 0.3, 0.12));
        r.tube(j, Vec3::new(x, 0.3, 0.12), Vec3::new(x + 0.12 * s, 0.4, 0.25), 0.042, 0.036, shell);
        r.ball(j, Vec3::new(x + 0.15 * s, 0.43, 0.3), Vec3::new(0.085, 0.065, 0.1), deep);
        let pincer = r.joint(if s > 0.0 { "pincer_l" } else { "pincer_r" }, Some(j), Vec3::new(x + 0.15 * s, 0.47, 0.32));
        r.ball_turned(pincer, Vec3::new(x + 0.16 * s, 0.5, 0.38), Vec3::new(0.05, 0.03, 0.08), rx(-15.0), deep);
        wings.push((j, s));
        pincers.push(pincer);
    }
    let mut legs = Vec::new();
    for (k, z) in [0.08f32, -0.04, -0.16].into_iter().enumerate() {
        for (x, side) in [(0.3f32, "l"), (-0.3, "r")] {
            let s = x.signum();
            let j = r.joint(&format!("leg_{side}{k}"), Some(body), Vec3::new(x, 0.24, z));
            r.tube(j, Vec3::new(x, 0.24, z), Vec3::new(x + 0.15 * s, 0.12, z), 0.03, 0.026, shell);
            r.tube(j, Vec3::new(x + 0.15 * s, 0.12, z), Vec3::new(x + 0.19 * s, 0.0, z), 0.026, 0.016, shell);
            legs.push((j, (k as f32 * 0.33 + if s > 0.0 { 0.0 } else { 0.5 }).fract()));
        }
    }
    accessories(&mut r, &Fit { head: body, face: head, top: Vec3::new(0.0, 0.43, -0.02), size: 1.15, eyes: eye_at, neck: body, neck_at: Vec3::new(0.0, 0.28, 0.0), neck_r: 0.0 });
    let p = Parts { root, body, head, eyes: [pupil_l, pupil_r], legs, tail: Vec::new(), wings, sideways: true };
    let mut clips = shared(&r, &p);
    // Snapping the claws, arms up.
    let mut a = Anim::new(&r, 0.6, 18);
    for &(arm, side) in &p.wings {
        a.turn(arm, move |q| rz(side * (30.0 + 10.0 * wave(q, 1.0, 0.0))));
    }
    for &pincer in &pincers {
        a.turn(pincer, |q| rx(-30.0 * wave(q, 3.0, 0.0).max(0.0)));
    }
    for &stalk in &eye_joints {
        a.turn(stalk, |q| rz(8.0 * wave(q, 1.0, 0.0)));
    }
    a.shift(root, |q| Vec3::new(0.0, 0.03 * wave(q, 2.0, 0.0).abs(), 0.0));
    clips.push(a.done("snap"));
    r.finish(clips)
}

fn ember() -> Vec<u8> {
    let red = rgb(0.93, 0.24, 0.08);
    let belly = rgb(1.0, 0.64, 0.2);
    let orange = rgb(1.0, 0.46, 0.08);
    let yellow = rgb(1.0, 0.83, 0.28);
    let mut r = Rig::new();
    let root = r.joint("root", None, Vec3::ZERO);
    let body = r.joint("body", Some(root), Vec3::new(0.0, 0.38, 0.0));
    r.ball(body, Vec3::new(0.0, 0.41, 0.0), Vec3::new(0.21, 0.24, 0.22), red);
    r.ball(body, Vec3::new(0.0, 0.37, 0.085), Vec3::new(0.155, 0.18, 0.15), belly);
    let head = r.joint("head", Some(body), Vec3::new(0.0, 0.6, 0.06));
    r.ball(head, Vec3::new(0.0, 0.75, 0.08), Vec3::splat(0.17), red);
    r.cone(head, Vec3::new(0.0, 0.72, 0.21), Vec3::new(0.0, 0.69, 0.35), 0.052, yellow);
    let crest = r.joint("crest", Some(head), Vec3::new(0.0, 0.88, 0.06));
    for (x, color, h) in [(0.0f32, yellow, 1.1f32), (0.06, orange, 1.03), (-0.06, orange, 1.03)] {
        r.cone(crest, Vec3::new(x, 0.87, 0.07), Vec3::new(x * 1.6, h, -0.07), 0.048, color);
    }
    let eyes = r.eyes(head, [Vec3::new(0.072, 0.79, 0.21), Vec3::new(-0.072, 0.79, 0.21)], Vec3::new(0.03, 0.04, 0.02));
    let mut wings = Vec::new();
    for (name, x) in [("wing_l", 0.19f32), ("wing_r", -0.19)] {
        let s = x.signum();
        let j = r.joint(name, Some(body), Vec3::new(x, 0.53, 0.0));
        r.ball_turned(j, Vec3::new(x * 1.3, 0.43, -0.05), Vec3::new(0.05, 0.17, 0.13), rz(-15.0 * s) * rx(-20.0), orange);
        r.cone(j, Vec3::new(x * 1.38, 0.32, -0.1), Vec3::new(x * 1.5, 0.19, -0.22), 0.05, yellow);
        wings.push((j, s));
    }
    let tail = r.joint("tail", Some(body), Vec3::new(0.0, 0.32, -0.18));
    for (x, color, y) in [(0.0f32, orange, 0.2f32), (0.07, red, 0.25), (-0.07, red, 0.25), (0.0, yellow, 0.32)] {
        r.cone(tail, Vec3::new(x, 0.32, -0.18), Vec3::new(x * 2.2, y, -0.52), 0.055, color);
    }
    let mut legs = Vec::new();
    for (name, x, phase) in [("leg_l", 0.08f32, 0.0f32), ("leg_r", -0.08, 0.5)] {
        let j = r.joint(name, Some(body), Vec3::new(x, 0.2, 0.02));
        r.tube(j, Vec3::new(x, 0.2, 0.02), Vec3::new(x, 0.04, 0.04), 0.024, 0.02, yellow);
        for turn in [-25.0f32, 0.0, 25.0] {
            let toe = ry(turn) * Vec3::new(0.0, 0.0, 0.08);
            r.cone(j, Vec3::new(x, 0.025, 0.04), Vec3::new(x, 0.015, 0.04) + toe, 0.018, yellow);
        }
        legs.push((j, phase));
    }
    // What's left when it burns: a little mound of ash, with embers.
    let ash = r.joint("opt:ash", None, Vec3::ZERO);
    r.ball(ash, Vec3::new(0.0, 0.03, 0.0), Vec3::new(0.24, 0.07, 0.2), rgb(0.42, 0.4, 0.4));
    r.ball(ash, Vec3::new(0.05, 0.06, 0.04), Vec3::new(0.12, 0.05, 0.1), rgb(0.55, 0.52, 0.5));
    for (x, z) in [(0.08f32, 0.1f32), (-0.1, 0.02), (0.02, -0.08)] {
        r.ball(ash, Vec3::new(x, 0.07, z), Vec3::splat(0.022), rgb(1.0, 0.45, 0.1));
    }
    accessories(&mut r, &Fit { head, face: head, top: Vec3::new(0.0, 0.9, 0.08), size: 0.9, eyes: [Vec3::new(0.072, 0.79, 0.21), Vec3::new(-0.072, 0.79, 0.21)], neck: body, neck_at: Vec3::new(0.0, 0.6, 0.05), neck_r: 0.13 });
    let p = Parts { root, body, head, eyes, legs, tail: vec![tail], wings, sideways: false };
    let mut clips = shared(&r, &p);
    // Its crest flickers in every clip it's in: a clip of its own would
    // be blended away, so it's folded into the idle.
    for (name, tracks) in clips.iter_mut() {
        if name == "idle" || name == "sit" || name == "happy" {
            let n = 24;
            let seconds = tracks.first().map(|_| match name.as_str() {
                "idle" => 3.2,
                "sit" => 3.0,
                _ => 0.8,
            });
            let seconds = seconds.unwrap_or(1.0);
            let times = (0..=n).map(|i| seconds * i as f32 / n as f32).collect();
            let values = (0..=n).map(|i| (rx(6.0 * wave(i as f32 / n as f32, 4.0, 0.0)) * rz(4.0 * wave(i as f32 / n as f32, 3.0, 0.3))).to_array()).collect();
            tracks.push(Track::rotation(crest, times, values));
        }
    }
    // Burning up: a spin, flaring, shrinking to nothing.
    let mut a = Anim::new(&r, 1.2, 30);
    a.turn(root, |q| ry(720.0 * ramp(q, 0.1, 1.0) * ramp(q, 0.1, 1.0)));
    a.scale(root, |q| Vec3::splat((1.0 + 0.15 * bump(q, 0.0, 0.3)) * (1.0 - ramp(q, 0.3, 1.0)).max(0.001)));
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * 80.0 * ramp(q, 0.0, 0.3)));
    }
    clips.push(a.done("burn"));
    // Ash: the bird gone, the mound breathing a little.
    let mut a = Anim::new(&r, 2.0, 20);
    a.scale(root, |_| Vec3::splat(0.001));
    a.scale(ash, |q| Vec3::new(1.0, 1.0 + 0.08 * wave(q, 1.0, 0.0), 1.0));
    clips.push(a.done("ash"));
    // Rising again: out of the ash, growing past its size, wings wide.
    let mut a = Anim::new(&r, 1.4, 28);
    a.scale(root, |q| Vec3::splat((ramp(q, 0.0, 0.6) * (1.0 + 0.2 * bump(q, 0.45, 0.9))).max(0.001)));
    a.scale(ash, |q| Vec3::splat((1.0 - ramp(q, 0.0, 0.4)).max(0.001)));
    a.turn(root, |q| ry(-360.0 * ramp(q, 0.0, 0.7)));
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * (70.0 + 30.0 * wave(q, 3.0, 0.0)) * (1.0 - ramp(q, 0.8, 1.0))));
    }
    a.turn(head, |q| rx(-20.0 * bump(q, 0.5, 1.0)));
    clips.push(a.done("rise"));
    // Flying: big wingbeats, legs tucked.
    let mut a = Anim::new(&r, 0.45, 12);
    for &(w, side) in &p.wings {
        a.turn(w, move |q| rz(side * (45.0 + 50.0 * wave(q, 1.0, 0.0))));
    }
    for &(leg, _) in &p.legs {
        a.turn(leg, |_| rx(-50.0));
    }
    a.turn(body, |q| rx(10.0 + 4.0 * wave(q, 1.0, 0.25)));
    a.turn(tail, |q| rx(-10.0 * wave(q, 1.0, 0.5)));
    clips.push(a.done("fly"));
    r.finish(clips)
}

/// The ball: red and white, rolling.
fn ball() -> Vec<u8> {
    let mut r = Rig::new();
    let root = r.joint("root", None, Vec3::new(0.0, 0.5, 0.0));
    r.ball(root, Vec3::new(0.0, 0.5, 0.0), Vec3::splat(0.5), rgb(0.92, 0.18, 0.2));
    r.tube(root, Vec3::new(-0.06, 0.5, 0.0), Vec3::new(0.06, 0.5, 0.0), 0.505, 0.505, WHITE);
    r.ball(root, Vec3::new(0.0, 0.5, 0.45), Vec3::new(0.1, 0.1, 0.07), rgb(1.0, 0.85, 0.2));
    let mut a = Anim::new(&r, 1.0, 4);
    a.turn(root, |q| rx(360.0 * q));
    let roll = a.done("roll");
    r.finish(vec![roll])
}

fn main() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    std::fs::create_dir_all(&out).expect("the models folder");
    for (name, glb) in [("cat", cat()), ("duck", duck()), ("ferris", ferris()), ("ember", ember()), ("ball", ball())] {
        let model = fenix_model::Model::from_glb(&glb).unwrap_or_else(|e| panic!("{name}: {e}"));
        let clips: Vec<String> = model.clips.iter().map(|c| format!("{} {:.2}", c.name, c.seconds)).collect();
        println!("{name}: {} bytes, {} vertices; {}", glb.len(), model.parts.iter().map(|p| p.vertices()).sum::<usize>(), clips.join(", "));
        std::fs::write(out.join(format!("{name}.glb")), glb).expect("written");
    }
}
