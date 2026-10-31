//! Keys as RFB wants them: X11 keysyms, pressed and let go, with the
//! modifiers held around them.

use fenix_extension_api::{Modifiers, NamedKey, SurfaceKey};

pub const CONTROL_L: u32 = 0xFFE3;
pub const ALT_L: u32 = 0xFFE9;
pub const SUPER_L: u32 = 0xFFEB;
pub const CAPS_LOCK: u32 = 0xFFE5;

/// The keysym of a key on the surface.
pub fn keysym_for(key: &SurfaceKey) -> u32 {
    match *key {
        // Printable ASCII: its keysym is its own code point.
        SurfaceKey::Char(c) if (' '..='~').contains(&c) => c as u32,
        // Anything else typed, from accented letters to CJK: X11's
        // Unicode keysyms, `0x01000000 | codepoint`.
        SurfaceKey::Char(c) => 0x0100_0000 | c as u32,
        SurfaceKey::Named(named) => match named {
            NamedKey::Escape => 0xFF1B,
            NamedKey::Enter => 0xFF0D,
            NamedKey::Tab => 0xFF09,
            NamedKey::Backspace => 0xFF08,
            NamedKey::Delete => 0xFFFF,
            NamedKey::Left => 0xFF51,
            NamedKey::Right => 0xFF53,
            NamedKey::Up => 0xFF52,
            NamedKey::Down => 0xFF54,
            NamedKey::Home => 0xFF50,
            NamedKey::End => 0xFF57,
            NamedKey::PageUp => 0xFF55,
            NamedKey::PageDown => 0xFF56,
            NamedKey::CapsLock => CAPS_LOCK,
        },
    }
}

/// The `(keysym, down)` events one key sends: bracketed with Control,
/// Alt and Super for whichever of `mods` are held -- down before it on a
/// press, up after it on a release -- since a held modifier comes with
/// the key it modifies, not as a key of its own. Without it Ctrl-C,
/// Ctrl-Alt-F2 or Alt-Tab in the guest would arrive as the bare key.
///
/// Two keys held under one modifier can let go of it early, when the
/// first of them is released: there's no modifier event of its own to
/// bracket around instead.
pub fn key_sequence(keysym: u32, mods: Modifiers, down: bool) -> Vec<(u32, bool)> {
    let held: Vec<u32> = [(mods.ctrl, CONTROL_L), (mods.alt, ALT_L), (mods.meta, SUPER_L)].into_iter().filter(|(h, _)| *h).map(|(_, sym)| sym).collect();
    let mut sequence = Vec::new();
    if down {
        sequence.extend(held.iter().map(|&sym| (sym, true)));
        sequence.push((keysym, true));
    } else {
        sequence.push((keysym, false));
        sequence.extend(held.iter().map(|&sym| (sym, false)));
    }
    sequence
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, alt: bool, meta: bool) -> Modifiers {
        Modifiers { ctrl, alt, meta }
    }

    #[test]
    fn every_printable_ascii_char_maps_to_its_own_codepoint() {
        for c in ' '..='~' {
            assert_eq!(keysym_for(&SurfaceKey::Char(c)), c as u32, "char {c:?}");
        }
    }

    #[test]
    fn a_non_latin1_char_uses_the_unicode_keysym_convention() {
        assert_eq!(keysym_for(&SurfaceKey::Char('é')), 0x0100_0000 | 'é' as u32);
        assert_eq!(keysym_for(&SurfaceKey::Char('★')), 0x0100_0000 | '★' as u32);
    }

    #[test]
    fn named_keys_map_to_their_documented_x11_keysym() {
        let cases = [
            (NamedKey::Escape, 0xFF1B),
            (NamedKey::Enter, 0xFF0D),
            (NamedKey::Tab, 0xFF09),
            (NamedKey::Backspace, 0xFF08),
            (NamedKey::Delete, 0xFFFF),
            (NamedKey::Left, 0xFF51),
            (NamedKey::Right, 0xFF53),
            (NamedKey::Up, 0xFF52),
            (NamedKey::Down, 0xFF54),
            (NamedKey::Home, 0xFF50),
            (NamedKey::End, 0xFF57),
            (NamedKey::PageUp, 0xFF55),
            (NamedKey::PageDown, 0xFF56),
            (NamedKey::CapsLock, 0xFFE5),
        ];
        for (named, keysym) in cases {
            assert_eq!(keysym_for(&SurfaceKey::Named(named)), keysym, "{named:?}");
        }
    }

    #[test]
    fn modifier_keysyms_match_the_documented_x11_values() {
        assert_eq!((CONTROL_L, ALT_L, SUPER_L, CAPS_LOCK), (0xFFE3, 0xFFE9, 0xFFEB, 0xFFE5));
    }

    #[test]
    fn the_bare_key_goes_alone_with_no_modifiers_held() {
        assert_eq!(key_sequence(0x61, mods(false, false, false), true), [(0x61, true)]);
        assert_eq!(key_sequence(0x61, mods(false, false, false), false), [(0x61, false)]);
    }

    #[test]
    fn ctrl_goes_down_before_on_a_press_and_up_after_on_a_release() {
        assert_eq!(key_sequence(0x63, mods(true, false, false), true), [(CONTROL_L, true), (0x63, true)]);
        assert_eq!(key_sequence(0x63, mods(true, false, false), false), [(0x63, false), (CONTROL_L, false)]);
    }

    #[test]
    fn every_held_modifier_is_bracketed() {
        assert_eq!(key_sequence(0xFFBF, mods(true, true, true), true), [(CONTROL_L, true), (ALT_L, true), (SUPER_L, true), (0xFFBF, true)]);
        assert_eq!(key_sequence(0xFFBF, mods(true, true, true), false), [(0xFFBF, false), (CONTROL_L, false), (ALT_L, false), (SUPER_L, false)]);
    }
}
