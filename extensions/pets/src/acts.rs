//! What a pet can be doing: each act a clip of its model, played round
//! or once, from a posture and leaving it in one.

/// The kinds there are: model name, what the picker calls it, the name
/// it suggests.
pub const KINDS: [(&str, &str, &str); 4] = [("ember", "Ember, Fenix's phoenix", "Ember"), ("ferris", "Ferris, the Rust crab", "Ferris"), ("cat", "A cat", "Tom"), ("duck", "A rubber duck", "Quackers")];

/// How big a kind is beside the others: a crab is mostly width.
pub fn stature(kind: &str) -> f32 {
    match kind {
        "ferris" => 0.72,
        "duck" => 0.9,
        _ => 1.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    Idle,
    Look,
    Walk,
    Run,
    SitDown,
    Sit,
    StandUp,
    LieDown,
    Sleep,
    Wake,
    /// A hop where it stands.
    Jump,
    /// A hop from one floor to another.
    Hop,
    Happy,
    Hide,
    Stretch,
    Listen,
    Quack,
    Snap,
    Stalk,
    Pounce,
    Burn,
    Ash,
    Rise,
    Fly,
}

/// How a pet is standing: what an act starts from and leaves it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    Standing,
    Sitting,
    Lying,
    /// Ember, burnt to ash.
    Gone,
}

impl Act {
    /// The clip it plays on `kind`'s model.
    pub fn clip(self, kind: &str) -> &'static str {
        match self {
            Act::Idle => "idle",
            Act::Look => "look",
            Act::Walk => "walk",
            Act::Run => "run",
            Act::SitDown => "sit_down",
            Act::Sit => "sit",
            Act::StandUp => "stand_up",
            Act::LieDown => "lie_down",
            Act::Sleep => "sleep",
            Act::Wake => "wake",
            Act::Jump | Act::Hop => "jump",
            Act::Happy => match kind {
                "ferris" => "snap",
                _ => "happy",
            },
            Act::Hide => "hide",
            Act::Stretch => "stretch",
            Act::Listen => "listen",
            Act::Quack => "quack",
            Act::Snap => "snap",
            Act::Stalk => "stalk",
            Act::Pounce => "pounce",
            Act::Burn => "burn",
            Act::Ash => "ash",
            Act::Rise => "rise",
            Act::Fly => match kind {
                "ember" => "fly",
                _ => "jump",
            },
        }
    }

    /// Played once and held, rather than round.
    pub fn once(self) -> bool {
        matches!(self, Act::SitDown | Act::StandUp | Act::LieDown | Act::Wake | Act::Jump | Act::Hop | Act::Stretch | Act::Pounce | Act::Burn | Act::Rise)
    }

    /// The posture it starts from.
    pub fn from(self) -> Posture {
        match self {
            Act::Sit | Act::StandUp => Posture::Sitting,
            Act::Sleep | Act::Wake => Posture::Lying,
            Act::Ash | Act::Rise => Posture::Gone,
            _ => Posture::Standing,
        }
    }

    /// The posture it leaves the pet in.
    pub fn leaves(self) -> Posture {
        match self {
            Act::SitDown | Act::Sit => Posture::Sitting,
            Act::LieDown | Act::Sleep => Posture::Lying,
            Act::Burn | Act::Ash => Posture::Gone,
            _ => Posture::Standing,
        }
    }

    /// Whether it goes somewhere.
    pub fn moves(self) -> bool {
        matches!(self, Act::Walk | Act::Run | Act::Hop | Act::Stalk | Act::Pounce | Act::Fly)
    }

    /// Whether it's something only some kinds do, and `kind` isn't one.
    pub fn not_for(self, kind: &str) -> bool {
        match self {
            Act::Quack => kind != "duck",
            Act::Snap => kind != "ferris",
            Act::Stalk | Act::Pounce => kind != "cat",
            Act::Burn | Act::Ash | Act::Rise => kind != "ember",
            _ => false,
        }
    }

    /// What gets a pet from `posture` to where this act starts.
    pub fn way_from(self, posture: Posture) -> Vec<Act> {
        let to = self.from();
        if posture == to || to == Posture::Gone {
            return Vec::new();
        }
        let up = match posture {
            Posture::Sitting => vec![Act::StandUp],
            Posture::Lying => vec![Act::Wake],
            Posture::Gone => vec![Act::Rise],
            Posture::Standing => Vec::new(),
        };
        let down = match to {
            Posture::Sitting => vec![Act::SitDown],
            Posture::Lying => vec![Act::LieDown],
            _ => Vec::new(),
        };
        up.into_iter().chain(down).collect()
    }

    /// Cells a second, at a medium size.
    pub fn speed(self, kind: &str) -> f32 {
        match (self, kind) {
            (Act::Walk, "ferris") => 2.6,
            (Act::Walk, "duck") => 2.8,
            (Act::Walk, _) => 3.4,
            (Act::Run, _) => 11.0,
            (Act::Stalk, _) => 1.4,
            (Act::Fly, _) => 9.0,
            _ => 6.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_act_is_reached_from_any_posture() {
        assert_eq!(Act::Walk.way_from(Posture::Sitting), [Act::StandUp]);
        assert_eq!(Act::Sleep.way_from(Posture::Sitting), [Act::StandUp, Act::LieDown]);
        assert_eq!(Act::Sit.way_from(Posture::Standing), [Act::SitDown]);
        assert_eq!(Act::Sit.way_from(Posture::Sitting), []);
        assert_eq!(Act::Happy.way_from(Posture::Gone), [Act::Rise]);
        assert!(Act::Ash.way_from(Posture::Standing).is_empty(), "burning is how it gets there");
    }

    #[test]
    fn each_kind_plays_its_own_clips() {
        assert_eq!(Act::Happy.clip("ferris"), "snap");
        assert_eq!(Act::Fly.clip("ember"), "fly");
        assert_eq!(Act::Fly.clip("cat"), "jump");
        assert!(Act::Quack.not_for("cat") && !Act::Quack.not_for("duck"));
        assert!(Act::Jump.once() && !Act::Walk.once());
        assert_eq!((Act::SitDown.leaves(), Act::Burn.leaves(), Act::Rise.leaves()), (Posture::Sitting, Posture::Gone, Posture::Standing));
    }
}
