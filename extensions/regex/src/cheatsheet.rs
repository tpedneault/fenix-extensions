//! The cheatsheet: the syntax, a section at a time, and recipes. Each
//! entry has an example that loads into the playground -- its pattern,
//! a text to try it on, and the flavour and flags it needs.

use crate::engine::{Flags, Flavour};

pub struct Entry {
    pub syntax: &'static str,
    pub meaning: &'static str,
    pub pattern: &'static str,
    pub text: &'static str,
    /// Its flags, as letters (`u` is on unless it says otherwise).
    pub flags: &'static str,
    pub fancy: bool,
}

impl Entry {
    pub fn flags(&self) -> Flags {
        let mut f = Flags::from_letters(self.flags);
        f.u = !self.flags.contains('A');
        f
    }

    pub fn flavour(&self) -> Flavour {
        if self.fancy {
            Flavour::Fancy
        } else {
            Flavour::Rust
        }
    }
}

pub struct Section {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

const fn e(syntax: &'static str, meaning: &'static str, pattern: &'static str, text: &'static str) -> Entry {
    Entry { syntax, meaning, pattern, text, flags: "", fancy: false }
}

const fn flagged(syntax: &'static str, meaning: &'static str, pattern: &'static str, text: &'static str, flags: &'static str) -> Entry {
    Entry { syntax, meaning, pattern, text, flags, fancy: false }
}

const fn fancy(syntax: &'static str, meaning: &'static str, pattern: &'static str, text: &'static str) -> Entry {
    Entry { syntax, meaning, pattern, text, flags: "", fancy: true }
}

pub const SECTIONS: &[Section] = &[
    Section {
        title: "Character classes",
        entries: &[
            e(".", "any character but a newline", "c.t", "cat cot c\nt"),
            e(r"\d \D", "a digit, a non-digit", r"\d+", "order 66, room 101"),
            e(r"\w \W", "a word character (letter, digit, _), and not", r"\w+", "snake_case and été"),
            e(r"\s \S", "whitespace, and not", r"\S+", "two  words\tand a tab"),
            e("[abc]", "one of a, b or c", "gr[ae]y", "grey gray groy"),
            e("[^abc]", "any character but a, b or c", "[^aeiou ]+", "regular expression"),
            e("[a-z]", "a range", "[A-Z][a-z]+", "Fenix and Rust and rust"),
            e(r"\p{L} \pN", "a Unicode class: a letter, a number", r"\p{Greek}+", "alpha is α, omega is Ω"),
            e("[[:alpha:]]", "a POSIX class, inside brackets", "[[:upper:]]+", "NASA and ESA"),
            e("[a-z&&[^aeiou]]", "intersection: consonants (Rust)", "[a-z&&[^aeiou]]+", "strengths"),
            e(r"\. \* \\", "a character that means something, as itself", r"\d+\.\d+", "pi is 3.14, not 3x14"),
        ],
    },
    Section {
        title: "Anchors",
        entries: &[
            e("^ $", "the start and end of the text (of each line with m)", r"^\d+$", "12345"),
            flagged("(?m) ^ $", "with m: each line", "^- .*$", "- one\n- two\nthree", "m"),
            e(r"\A \z", "the very start and end, whatever the flags", r"\Aa|b\z", "ab\nab"),
            e(r"\b", "a word boundary", r"\bcat\b", "cat concat cats cat."),
            e(r"\B", "anywhere but a word boundary", r"\Bcat", "cat concat"),
            e(r"\b{start} \b{end}", "the start, the end of a word (Rust)", r"\b{start}\w", "first letters only"),
        ],
    },
    Section {
        title: "Quantifiers",
        entries: &[
            e("*", "0 or more, as many as can be", "ab*", "a ab abbb"),
            e("+", "1 or more", "ab+", "a ab abbb"),
            e("?", "0 or 1: optional", "colou?r", "color colour"),
            e("{n} {n,} {n,m}", "exactly n, n or more, n to m", r"\d{2,4}", "1 12 123 12345"),
            e("*? +? ??", "lazy: as few as can be", "<.+?>", "<b>bold</b>"),
            e(".*", "greedy: as many as can be", "<.+>", "<b>bold</b>"),
            flagged("(?U)", "swap greed: lazy by default (Rust)", "(?U)<.+>", "<b>bold</b>", ""),
        ],
    },
    Section {
        title: "Groups",
        entries: &[
            e("(...)", "a group, captured and numbered", r"(\w+)@(\w+)", "me@home you@work"),
            e("(?<name>...)", "a named group", r"(?<key>\w+)=(?<value>\w+)", "a=1 b=2"),
            e("(?P<name>...)", "a named group, Python's way", r"(?P<year>\d{4})-(?P<month>\d\d)", "2026-10"),
            e("(?:...)", "a group that doesn't capture", r"(?:ab)+", "ababab"),
            e("a|b", "either", "cat|dog", "hot dog, cool cat"),
            e("(?i:...)", "flags for the group only", "(?i:hello) world", "HELLO world HELLO WORLD"),
            fancy("(?>...)", "atomic: never backtracked into (fancy-regex)", "(?>a+)b", "aaab"),
        ],
    },
    Section {
        title: "Lookaround (fancy-regex)",
        entries: &[
            fancy("(?=...)", "lookahead: followed by", r"\d+(?= dollars)", "5 euros, 10 dollars"),
            fancy("(?!...)", "negative lookahead: not followed by", r"\b\d+\b(?!%)", "50% of 80"),
            fancy("(?<=...)", "lookbehind: preceded by", r"(?<=\$)\d+", "costs $42 or 42"),
            fancy("(?<!...)", "negative lookbehind: not preceded by", r"(?<!-)\b\d+", "-5 and 7"),
        ],
    },
    Section {
        title: "Backreferences (fancy-regex)",
        entries: &[
            fancy(r"\1", "the text group 1 matched, again", r"\b(\w+) \1\b", "this is is a test test"),
            fancy(r"\k<name>", "a named group's text, again", r#"(?<q>['"]).*?\k<q>"#, r#"say "hi" or 'yo'"#),
            fancy("$1 ${name}", "in the replacement: a group's text", r"(?<first>\w+) (\w+)", "Ada Lovelace"),
        ],
    },
    Section {
        title: "Flags",
        entries: &[
            flagged("i", "case-insensitive", "fenix", "Fenix FENIX fenix", "i"),
            flagged("m", "multi-line: ^ and $ at each line", r"^\w+", "one\ntwo", "m"),
            flagged("s", "dot matches newline", "a.b", "a\nb", "s"),
            flagged("x", "extended: spaces ignored, # comments", r"\d{3} - \d{4}  # a phone number", "555-1234", "x"),
            flagged("u off", r"ASCII: \w and \d on ASCII only", r"\w+", "café", "A"),
            e("(?i) (?-i)", "flags inline, from there on", "(?i)a(?-i)b", "ab AB Ab aB"),
        ],
    },
    Section {
        title: "Recipes",
        entries: &[
            flagged("email", "an address, close enough", r"[\w.+-]+@[\w-]+(\.[\w-]+)+", "write to ada@example.org or a.b+c@mail.co.uk", ""),
            e("URL", "http and https", r#"https?://[^\s/$.?#][^\s"<>]*"#, "see https://example.org/path?q=1 and http://x.io"),
            e("IPv4", "four numbers 0-255", r"\b((25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(25[0-5]|2[0-4]\d|1?\d?\d)\b", "10.0.0.1 256.1.1.1 192.168.1.254"),
            e("ISO date", "yyyy-mm-dd", r"(?<y>\d{4})-(?<m>0[1-9]|1[0-2])-(?<d>0[1-9]|[12]\d|3[01])", "2026-10-29 and 2026-13-01"),
            e("time", "hh:mm[:ss]", r"\b([01]\d|2[0-3]):[0-5]\d(:[0-5]\d)?\b", "at 09:30, 23:59:59, not 24:00"),
            e("semver", "a semantic version", r"\bv?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?\b", "1.2.3 v2.0.0-rc.1+build.5 1.2"),
            flagged("UUID", "8-4-4-4-12 hex", r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", "id 123e4567-E89B-12d3-a456-426614174000", "i"),
            flagged("log line", "time, level, message", r"^(?<time>\S+ \S+) (?<level>[A-Z]+) +(?<msg>.*)$", "2026-10-29 10:00:01 INFO started\n2026-10-29 10:00:02 ERROR no disk", "m"),
            e("hex colour", "#rgb or #rrggbb", r"#([0-9a-fA-F]{3}){1,2}\b", "#fff #1e1e2e #12"),
            e("quoted string", "with escaped quotes inside", r#""(\\.|[^"\\])*""#, r#"say "a \"quote\"" now"#),
        ],
    },
];

/// Every entry, with its section's title.
pub fn all() -> impl Iterator<Item = (&'static str, &'static Entry)> {
    SECTIONS.iter().flat_map(|s| s.entries.iter().map(move |e| (s.title, e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::run;

    #[test]
    fn every_example_compiles_and_matches_its_text() {
        for (section, e) in all() {
            let o = run(e.pattern, e.flags(), e.flavour(), e.text, None);
            assert!(o.error.is_none(), "{section} {}: {:?}", e.syntax, o.error);
            assert!(!o.matches.is_empty(), "{section} {}: no match in {:?}", e.syntax, e.text);
        }
    }

    #[test]
    fn recipes_match_what_they_should() {
        let find = |name: &str| all().find(|(_, e)| e.syntax == name).unwrap().1;
        let ipv4 = find("IPv4");
        let o = run(ipv4.pattern, ipv4.flags(), ipv4.flavour(), ipv4.text, None);
        let found: Vec<String> = o.matches.iter().map(|m| ipv4.text.chars().skip(m.at().start).take(m.at().len()).collect()).collect();
        assert!(found.contains(&"10.0.0.1".to_string()) && found.contains(&"192.168.1.254".to_string()) && !found.iter().any(|f| f.starts_with("256")), "{found:?}");
        let date = find("ISO date");
        assert_eq!(run(date.pattern, date.flags(), date.flavour(), date.text, None).matches.len(), 1, "month 13 isn't one");
    }
}
