//! Where JavaScript, Python and PCRE read the pattern otherwise than the
//! flavour it's tried in: a line for each feature it uses that they
//! treat differently, and for each flag that differs.

use crate::engine::{Flags, Flavour};
use crate::explain::Features;

/// The notes for a pattern that uses `f`, with `flags`, in `flavour`.
pub fn notes(f: &Features, flags: Flags, flavour: Flavour) -> Vec<String> {
    let mut out = Vec::new();
    let mut say = |s: &str| out.push(s.to_string());
    if f.lookahead || f.lookbehind || f.backref || f.atomic {
        match flavour {
            Flavour::Rust => say("Lookaround and backreferences: not in Rust's regex (f tries fancy-regex). JavaScript, Python and PCRE have them."),
            Flavour::Fancy => say("fancy-regex backtracks for lookaround and backreferences: slower there, and it gives up past a limit. Rust's regex refuses them."),
        }
    }
    if f.lookbehind {
        say("Lookbehind: Python and PCRE want it a fixed length (PCRE: each alternative fixed); JavaScript takes any.");
    }
    if f.atomic {
        say("Atomic groups (?>...): PCRE and Python 3.11+ have them; JavaScript doesn't.");
    }
    if f.backref {
        say(r"Backreferences: \1 everywhere; by name \k<name> in JavaScript and PCRE, (?P=name) in Python.");
    }
    if f.named_python {
        say("(?P<name>...): Python and PCRE; JavaScript, Java and C# want (?<name>...), which the copies use.");
    } else if f.named_angle {
        say("(?<name>...): JavaScript, PCRE, Java, C#, Go 1.22+; Python wants (?P<name>...), which its copy uses.");
    }
    if f.perl_class || f.word_boundary {
        if flags.u {
            say(r"\d \w \s \b: Unicode here and in Python 3; ASCII in JavaScript (even with u), Go, Java and PCRE unless asked (PCRE2_UCP).");
        } else {
            say(r"\d \w \s \b: ASCII here (u off), as in JavaScript and PCRE; Python 3 is Unicode unless re.ASCII.");
        }
    }
    if f.word_edges {
        say(r"\b{start} \b{end} \< \>: Rust's only. Elsewhere: \b(?=\w) and \b(?<=\w).");
    }
    if f.unicode_class {
        say(r"\p{...}: JavaScript needs the u flag; Python's re has none (the regex module does); PCRE has it.");
    }
    if f.posix_class {
        say("[[:alpha:]]: PCRE and Rust; not JavaScript or Python.");
    }
    if f.nested_class || f.class_ops {
        say("Nested classes and && -- ~~: Rust's. JavaScript has && and -- with the v flag; Python and PCRE read them as plain characters.");
    }
    if f.text_anchors {
        say(r"\A \z: PCRE and Rust; Python spells \z as \Z; JavaScript has neither (^ and $ without m).");
    }
    if f.dollar && !flags.m {
        say("$: Python and PCRE also match just before a final newline; JavaScript and Rust only at the very end.");
    }
    if f.hex_brace {
        say(r"\x{...}: PCRE and Rust; JavaScript writes \u{...} (with u), Python \uXXXX and \UXXXXXXXX.");
    }
    if f.inline_flags {
        say("Inline flags: Python and PCRE have them (Python's global ones only at the start); JavaScript has only (?i: ...) groups, in recent engines.");
    }
    if f.rust_only_flags {
        say("The U (swap greed) and R (CRLF) flags are Rust's.");
    }
    if flags.x {
        say("x (extended): Python, PCRE, Java and C#; not JavaScript or Go.");
    }
    if flags.s {
        say("s: JavaScript's s, Python's re.DOTALL, PCRE's s; Go and Java (?s).");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_for_what_it_uses() {
        let f = Features { lookbehind: true, ..Features::default() };
        let n = notes(&f, Flags::default(), Flavour::Rust);
        assert!(n[0].contains("not in Rust's regex"), "{n:?}");
        assert!(n[1].contains("fixed length"));
        let n = notes(&f, Flags::default(), Flavour::Fancy);
        assert!(n[0].contains("backtracks"));
    }

    #[test]
    fn nothing_to_say_about_plain_text() {
        assert!(notes(&Features::default(), Flags::default(), Flavour::Rust).is_empty());
        let dollar = Features { dollar: true, ..Features::default() };
        assert_eq!(notes(&dollar, Flags::default(), Flavour::Rust).len(), 1);
        assert!(notes(&dollar, Flags { m: true, ..Flags::default() }, Flavour::Rust).is_empty(), "with m, $ is the same everywhere");
    }
}
