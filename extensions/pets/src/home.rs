//! The pets' home (`SPC P p`): each pet, its level and how far to the
//! next, how happy it is, how long you've been together and what it
//! wears; and keys to pet it, dress it, rename it, rehome it, or adopt
//! another.

use fenix_extension_api::{page, Key, Page};

use crate::brain::State;
use crate::care;

/// What a key on the page asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Moved: draw the page again.
    Moved,
    Pet(u32),
    Dress(u32),
    Rename(u32),
    Rehome(u32),
    Adopt,
}

/// Lines a pet takes on the page, and the lines above the first.
const EACH: u32 = 4;
const TOP: u32 = 2;

pub fn render(st: &State, focus: usize, width: u32) -> Page {
    let mut p = Page::new().line([page::title("At home"), page::muted(format!("  {} of {} (pets.most)", st.pets.len(), st.most))]).rule();
    if st.pets.is_empty() {
        return p.line([page::muted("Nobody's home. "), page::accent("a"), page::muted(" adopts a pet.")]).key("a", "adopt");
    }
    let bar = (width.saturating_sub(30) as usize).clamp(10, 30);
    for pet in &st.pets {
        let care = &pet.care;
        let (into, span) = care.progress();
        let filled = (into as usize * bar / span.max(1) as usize).min(bar);
        let days = st.today.saturating_sub(care.adopted);
        let together = match days {
            0 => "since today".to_string(),
            1 => "1 day together".to_string(),
            n => format!("{n} days together"),
        };
        let kind = crate::acts::KINDS.iter().find(|(k, _, _)| *k == pet.kind).map_or(pet.kind.as_str(), |(_, about, _)| about);
        p = p.line([page::accent(pet.name.clone()), page::muted(format!("  {kind}  ·  ")), page::text(format!("level {}", care.level())), page::muted(format!("  ·  {together}"))]);
        let hearts = care.hearts() as usize;
        p = p.line([
            page::muted("  "),
            page::good("█".repeat(filled)),
            page::muted("░".repeat(bar - filled)),
            page::muted(format!(" {into}/{span}   ")),
            page::bad("♥".repeat(hearts)),
            page::muted("♡".repeat(5 - hearts)),
        ]);
        let unlocked = care.unlocked();
        let wearing: Vec<&str> = care::UNLOCKS.iter().filter(|(_, part, _)| unlocked.contains(part) && pet.wearing.iter().any(|w| w == part)).map(|(_, _, what)| *what).collect();
        let next = care::UNLOCKS.iter().find(|(level, _, _)| *level > care.level());
        let mut line = vec![page::muted("  wearing "), page::text(if wearing.is_empty() { "nothing".to_string() } else { wearing.join(", ") })];
        if let Some((level, _, what)) = next {
            line.push(page::muted(format!("  ·  {what} at level {level}")));
        }
        p = p.line(line).blank();
    }
    p.focus(TOP + focus.min(st.pets.len() - 1) as u32 * EACH)
        .key("j/k", "move")
        .key("Enter", "pet")
        .key("w", "dress")
        .key("n", "rename")
        .key("r", "rehome")
        .key("a", "adopt")
}

pub fn key(st: &State, focus: &mut usize, key: Key) -> Action {
    let uid = st.pets.get(*focus).map(|p| p.uid);
    match (key, uid) {
        (Key::Down | Key::Char('j'), _) if *focus + 1 < st.pets.len() => {
            *focus += 1;
            Action::Moved
        }
        (Key::Up | Key::Char('k'), _) if *focus > 0 => {
            *focus -= 1;
            Action::Moved
        }
        (Key::Enter, Some(uid)) => Action::Pet(uid),
        (Key::Char('w'), Some(uid)) => Action::Dress(uid),
        (Key::Char('n'), Some(uid)) => Action::Rename(uid),
        (Key::Char('r'), Some(uid)) => Action::Rehome(uid),
        (Key::Char('a'), _) => Action::Adopt,
        _ => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::Pet;
    use crate::world::{Floor, Spot};

    #[test]
    fn the_home_shows_each_pet_and_its_keys_work() {
        let mut ember = Pet::new(4, "ember", "Ember", Spot::new(Floor::Modeline, 3.0), 90);
        ember.care.xp = 110;
        ember.wearing = vec!["scarf".into(), "crown".into()];
        let duck = Pet::new(9, "duck", "Q", Spot::new(Floor::Modeline, 9.0), 100);
        let st = State::new(vec![ember, duck], 1, 100);
        let p = render(&st, 1, 80);
        let text = |n: usize| p.lines[n].spans.iter().map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(text(2), "Ember  Ember, Fenix's phoenix  ·  level 3  ·  10 days together");
        assert!(text(3).contains(" 10/125   "), "{}", text(3));
        assert_eq!(text(4), "  wearing a scarf  ·  sunglasses at level 4", "a crown isn't unlocked yet");
        assert_eq!(text(6), "Q  A rubber duck  ·  level 1  ·  since today");
        assert_eq!(p.focus, Some(6));
        let mut focus = 1;
        assert_eq!(key(&st, &mut focus, Key::Char('j')), Action::Nothing, "already the last");
        assert_eq!(key(&st, &mut focus, Key::Char('k')), Action::Moved);
        assert_eq!(key(&st, &mut focus, Key::Char('w')), Action::Dress(4));
        assert_eq!(key(&st, &mut focus, Key::Enter), Action::Pet(4));
        let empty = State::new(Vec::new(), 1, 0);
        assert_eq!(render(&empty, 0, 80).keys.len(), 1);
    }
}
