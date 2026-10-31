//! Effects: little sprites of `fx.png` that drift, rise, burst and fall,
//! each a glide Fenix animates and a while it lasts; and the ball, thrown
//! from the caret, bouncing lower each time, then rolling to a stop.

use fenix_extension_api::{Ease, Item, Place};

use crate::world::{Floor, Spot};

/// Frames of `fx.png` (`src/fx.py`).
pub const HEART: u32 = 0;
pub const HEART_SMALL: u32 = 1;
pub const ZED: u32 = 2;
pub const ZED_BIG: u32 = 3;
pub const SPARK: [u32; 4] = [4, 5, 6, 7];
pub const FLAME: [u32; 4] = [8, 9, 10, 11];
pub const SMOKE: [u32; 4] = [12, 13, 14, 15];
pub const FLAKE: [u32; 2] = [16, 17];
pub const CONFETTI: [u32; 4] = [18, 19, 20, 21];
pub const STAR: u32 = 22;
pub const NOTE: u32 = 23;
pub const DROP: u32 = 24;
pub const BANG: u32 = 25;

/// The most effects at once.
const MOST: usize = 60;
/// Effects' item ids: from here.
const FIRST_ID: u32 = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    pub id: u32,
    pub frames: Vec<u32>,
    pub fps: u8,
    pub from: Place,
    pub to: Place,
    pub millis: u32,
    pub ease: Ease,
    pub arc: f32,
    /// Lines tall.
    pub size: f32,
    pub until: u64,
    pub fade: bool,
}

#[derive(Debug, Default)]
pub struct Fx {
    pub list: Vec<Particle>,
    next: u32,
}

impl Fx {
    /// Adds one, lasting `millis` from `now`, going from `from` to `to`.
    #[allow(clippy::too_many_arguments)]
    pub fn add(&mut self, now: u64, frames: &[u32], fps: u8, from: Place, to: Place, millis: u32, ease: Ease, arc: f32, size: f32) {
        if self.list.len() >= MOST {
            self.list.remove(0);
        }
        self.next = (self.next + 1) % 1000;
        self.list.push(Particle { id: FIRST_ID + self.next, frames: frames.to_vec(), fps, from, to, millis, ease, arc, size, until: now + millis as u64, fade: false });
    }

    /// Fading as it goes (smoke, snow).
    pub fn fading(&mut self) {
        if let Some(last) = self.list.last_mut() {
            last.fade = true;
        }
    }

    pub fn prune(&mut self, now: u64) {
        self.list.retain(|p| p.until > now);
    }

    pub fn items(&self, sheet: u32) -> Vec<Item> {
        self.list
            .iter()
            .map(|p| {
                let item = Item::new(p.id, sheet, p.frames[0], p.from).frames(&p.frames, p.fps).size(p.size).glide(p.to, p.millis).ease(p.ease).arc(p.arc);
                if p.fade {
                    item.opacity(0.75)
                } else {
                    item
                }
            })
            .collect()
    }
}

/// Moved by `dx` columns and `dy` lines.
pub fn nudge(place: &Place, dx: f32, dy: f32) -> Place {
    Place::new(place.anchor, place.x + dx, place.y + dy)
}

/// One leg of the ball's flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leg {
    pub from: Spot,
    pub to: Spot,
    pub start: u64,
    pub millis: u32,
    /// Lines high at its middle.
    pub arc: f32,
    pub ease: Ease,
}

/// The ball, in flight, at rest, or carried.
#[derive(Debug, Clone, PartialEq)]
pub struct Ball {
    pub legs: Vec<Leg>,
    /// Which pet is going for it, by uid; and whether it has it.
    pub fetcher: Option<u32>,
    pub carried: bool,
    /// When it was dropped, at the end: gone a while after.
    pub dropped: Option<u64>,
}

impl Ball {
    /// Thrown from the caret (or `from`) to land at `land` on `floor`,
    /// then bouncing on towards `dir` (-1 or 1), each bounce lower and
    /// shorter, and rolling to a stop.
    pub fn thrown(from: Spot, floor: Floor, land: f32, dir: f32, now: u64) -> Ball {
        let mut legs = vec![Leg { from, to: Spot::new(floor, land), start: now, millis: 900, arc: 3.0, ease: Ease::Linear }];
        let (mut x, mut height, mut length, mut millis) = (land, 1.4f32, 4.0f32, 520u32);
        while height > 0.2 {
            let to = x + dir * length;
            let start = legs.last().map_or(now, |l| l.start + l.millis as u64);
            legs.push(Leg { from: Spot::new(floor, x), to: Spot::new(floor, to), start, millis, arc: height, ease: Ease::Linear });
            x = to;
            height *= 0.45;
            length *= 0.6;
            millis = (millis as f32 * 0.72) as u32;
        }
        let start = legs.last().map_or(now, |l| l.start + l.millis as u64);
        legs.push(Leg { from: Spot::new(floor, x), to: Spot::new(floor, x + dir * 3.0), start, millis: 1100, arc: 0.0, ease: Ease::Out });
        Ball { legs, fetcher: None, carried: false, dropped: None }
    }

    /// The leg it's on at `now` (the last, once it's come to rest).
    pub fn leg(&self, now: u64) -> &Leg {
        self.legs.iter().rev().find(|l| l.start <= now).unwrap_or(&self.legs[0])
    }

    /// When it first lands.
    pub fn landed(&self) -> u64 {
        self.legs[0].start + self.legs[0].millis as u64
    }

    /// Where it comes to rest.
    pub fn rest(&self) -> Spot {
        self.legs.last().map(|l| l.to).unwrap_or(self.legs[0].to)
    }

    /// When it comes to rest.
    pub fn stopped(&self) -> u64 {
        self.legs.last().map_or(0, |l| l.start + l.millis as u64)
    }

    /// Whether it's rolling (spinning) at `now`.
    pub fn rolling(&self, now: u64) -> bool {
        !self.carried && now < self.stopped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::Anchor;

    #[test]
    fn effects_come_and_go() {
        let mut fx = Fx::default();
        let at = Place::new(Anchor::Modeline, 4.0, -1.0);
        fx.add(0, &[HEART], 0, at, nudge(&at, 0.0, -1.5), 1200, Ease::Out, 0.0, 0.8);
        fx.add(0, &SMOKE, 6, at, at, 600, Ease::Linear, 0.0, 0.8);
        fx.fading();
        let items = fx.items(9);
        assert_eq!((items.len(), items[0].sheet, items[0].id), (2, 9, 1001));
        assert_eq!(items[0].glide.as_ref().map(|g| (g.to.y, g.ease)), Some((-2.5, Ease::Out)));
        assert_eq!((items[1].fps, items[1].opacity), (6, 0.75));
        fx.prune(700);
        assert_eq!(fx.list.len(), 1);
        for _ in 0..100 {
            fx.add(0, &[STAR], 0, at, at, 10, Ease::Linear, 0.0, 1.0);
        }
        assert_eq!(fx.list.len(), MOST);
    }

    #[test]
    fn a_ball_bounces_lower_and_lower_then_rolls() {
        let ball = Ball::thrown(Spot::new(Floor::Caret, 0.0), Floor::Modeline, 40.0, 1.0, 1000);
        let arcs: Vec<f32> = ball.legs.iter().map(|l| l.arc).collect();
        assert_eq!(arcs[0], 3.0);
        assert!(arcs.windows(2).all(|w| w[1] < w[0]), "{arcs:?}");
        assert_eq!(*arcs.last().unwrap(), 0.0, "rolling");
        assert_eq!(ball.landed(), 1900);
        assert_eq!(ball.leg(1950).from.x, 40.0);
        assert!(ball.rest().x > 45.0 && ball.rest().x < 52.0, "{:?}", ball.rest());
        assert!(ball.rolling(ball.stopped() - 1) && !ball.rolling(ball.stopped()));
        // Each leg starts where the last ended.
        assert!(ball.legs.windows(2).all(|w| w[0].to == w[1].from && w[1].start == w[0].start + w[0].millis as u64));
    }
}
