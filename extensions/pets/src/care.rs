//! Looking after them: what you do keeps them happy and teaches them
//! things. Saving, passing tests, petting and playing fetch earn a pet
//! experience; each level unlocks something to wear. Happiness drifts
//! down slowly while Fenix is open and nobody's playing, never below
//! content -- and never while you're away: a day off costs nothing.

/// What each level unlocks: the level, the part (`opt:<name>` in the
/// models), what it's called.
pub const UNLOCKS: [(u32, &str, &str); 5] = [(2, "bow", "a bow"), (3, "scarf", "a scarf"), (4, "glasses", "sunglasses"), (5, "party_hat", "a party hat"), (7, "crown", "a crown")];

/// The parts worn on top of the head: one at a time.
pub const HATS: [&str; 5] = ["bow", "party_hat", "crown", "santa_hat", "pumpkin"];

/// Happiness: never below this, never above 100.
pub const CONTENT: f32 = 40.0;
/// How far it drifts down a minute while nobody plays.
const DRIFT: f32 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub struct Care {
    pub xp: u32,
    pub happy: f32,
    /// The day it was adopted (days since 1970).
    pub adopted: u32,
}

impl Care {
    pub fn new(today: u32) -> Care {
        Care { xp: 0, happy: 80.0, adopted: today }
    }

    pub fn level(&self) -> u32 {
        level_for(self.xp)
    }

    /// Experience earned: the level it reached, when it went up.
    pub fn gain(&mut self, xp: u32) -> Option<u32> {
        let was = self.level();
        self.xp = self.xp.saturating_add(xp);
        (self.level() > was).then(|| self.level())
    }

    pub fn cheer(&mut self, by: f32) {
        self.happy = (self.happy + by).clamp(CONTENT, 100.0);
    }

    /// `minutes` of Fenix open with nobody playing.
    pub fn drift(&mut self, minutes: f32) {
        self.happy = (self.happy - DRIFT * minutes).max(CONTENT);
    }

    /// What it may wear by now.
    pub fn unlocked(&self) -> Vec<&'static str> {
        UNLOCKS.iter().filter(|(l, _, _)| self.level() >= *l).map(|(_, part, _)| *part).collect()
    }

    /// How far into this level, and how much the level spans.
    pub fn progress(&self) -> (u32, u32) {
        let (at, next) = (xp_for(self.level()), xp_for(self.level() + 1));
        (self.xp - at, next - at)
    }

    /// Out of five.
    pub fn hearts(&self) -> u32 {
        ((self.happy - CONTENT) / (100.0 - CONTENT) * 4.0).round() as u32 + 1
    }
}

/// The level `xp` makes: 2 at 25, 3 at 100, 4 at 225, ...
pub fn level_for(xp: u32) -> u32 {
    1 + ((xp as f32 / 25.0).sqrt() + 1e-4) as u32
}

pub fn xp_for(level: u32) -> u32 {
    25 * (level - 1) * (level - 1)
}

/// What a level-up unlocked, said.
pub fn unlocked_at(level: u32) -> Option<&'static str> {
    UNLOCKS.iter().find(|(l, _, _)| *l == level).map(|(_, _, what)| *what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_climb_and_unlock_things_to_wear() {
        let mut care = Care::new(100);
        assert_eq!((care.level(), care.unlocked().len()), (1, 0));
        assert_eq!(care.gain(24), None);
        assert_eq!(care.gain(1), Some(2));
        assert_eq!(care.unlocked(), ["bow"]);
        assert_eq!(care.progress(), (0, 75));
        care.gain(200);
        assert_eq!((care.level(), care.unlocked()), (4, vec!["bow", "scarf", "glasses"]));
        assert_eq!(unlocked_at(5), Some("a party hat"));
        assert_eq!((xp_for(1), xp_for(7), level_for(900)), (0, 900, 7));
    }

    #[test]
    fn happiness_drifts_but_never_below_content() {
        let mut care = Care::new(0);
        care.drift(600.0);
        assert_eq!(care.happy, CONTENT);
        assert_eq!(care.hearts(), 1);
        care.cheer(500.0);
        assert_eq!((care.happy, care.hearts()), (100.0, 5));
    }
}
