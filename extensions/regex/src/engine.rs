//! Matching: the pattern compiled with its flags by the flavour chosen --
//! Rust's `regex`, or `fancy-regex` for lookaround and backreferences --
//! and run over the test text. Its matches and their groups, the
//! replacement's result, or the error, at the place in the pattern it's
//! about. Positions are in characters, as the page draws them.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::syntax;

/// The most matches listed; past it the list says there are more.
pub const MOST: usize = 1000;
/// How far fancy-regex backtracks before it gives up on a match.
const BACKTRACK: usize = 1_000_000;

/// The flags, as toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Flags {
    /// `i`: case-insensitive.
    pub i: bool,
    /// `m`: `^` and `$` at each line.
    pub m: bool,
    /// `s`: `.` matches a newline too.
    pub s: bool,
    /// `x`: extended -- whitespace ignored, `#` comments.
    pub x: bool,
    /// `u`: Unicode (`\d`, `\w`, `.` on characters, not bytes).
    pub u: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Flags { i: false, m: false, s: false, x: false, u: true }
    }
}

impl Flags {
    /// The flags on, as letters: `ims`.
    pub fn letters(&self) -> String {
        [(self.i, 'i'), (self.m, 'm'), (self.s, 's'), (self.x, 'x'), (self.u, 'u')].iter().filter(|(on, _)| *on).map(|(_, c)| *c).collect()
    }

    /// From letters (`ims`): only the letters given are on.
    pub fn from_letters(letters: &str) -> Flags {
        let has = |c| letters.contains(c);
        Flags { i: has('i'), m: has('m'), s: has('s'), x: has('x'), u: has('u') }
    }

    /// Flips the flag `c` is (`i`, `m`, `s`, `x`, `u`): whether it's one.
    pub fn toggle(&mut self, c: char) -> bool {
        let flag = match c {
            'i' => &mut self.i,
            'm' => &mut self.m,
            's' => &mut self.s,
            'x' => &mut self.x,
            'u' => &mut self.u,
            _ => return false,
        };
        *flag = !*flag;
        true
    }
}

/// Which engine matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flavour {
    /// Rust's `regex`: linear time, no lookaround or backreferences.
    #[default]
    Rust,
    /// `fancy-regex`: those too, by backtracking where they're used.
    Fancy,
}

impl Flavour {
    pub fn name(self) -> &'static str {
        match self {
            Flavour::Rust => "Rust regex",
            Flavour::Fancy => "fancy-regex",
        }
    }

    pub fn other(self) -> Flavour {
        match self {
            Flavour::Rust => Flavour::Fancy,
            Flavour::Fancy => Flavour::Rust,
        }
    }
}

/// What's wrong with the pattern, and where (characters), when that's
/// known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    pub at: Option<Range<usize>>,
    pub message: String,
}

/// One match: the whole of it (group 0) and each group, where it matched
/// or `None` when it took no part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub groups: Vec<Option<Range<usize>>>,
}

impl Match {
    pub fn at(&self) -> Range<usize> {
        self.groups[0].clone().unwrap_or(0..0)
    }
}

/// The pattern run over the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Each group's name, group 0's first (always none).
    pub names: Vec<Option<String>>,
    pub matches: Vec<Match>,
    /// There were more than [`MOST`].
    pub more: bool,
    /// The text with every match replaced, when there's a replacement.
    pub replaced: Option<String>,
    pub error: Option<PatternError>,
    /// Matching stopped: fancy-regex backtracked too far.
    pub stopped: Option<String>,
}

enum Compiled {
    Rust(regex::Regex),
    /// With `u` off: on bytes, so `.` and `\xFF` may match inside a
    /// character.
    Bytes(regex::bytes::Regex),
    Fancy(fancy_regex::Regex),
}

/// Char offsets for byte offsets of one string.
struct Chars(Vec<usize>);

impl Chars {
    fn of(text: &str) -> Chars {
        Chars(text.char_indices().map(|(b, _)| b).chain(std::iter::once(text.len())).collect())
    }

    fn at(&self, byte: usize) -> usize {
        self.0.partition_point(|&b| b < byte)
    }

    fn range(&self, r: Range<usize>) -> Range<usize> {
        self.at(r.start)..self.at(r.end)
    }
}

/// `regex-syntax` set up as the flags say.
fn syntax_parser(flags: Flags) -> regex_syntax::Parser {
    regex_syntax::ParserBuilder::new()
        .case_insensitive(flags.i)
        .multi_line(flags.m)
        .dot_matches_new_line(flags.s)
        .ignore_whitespace(flags.x)
        .unicode(flags.u)
        .utf8(flags.u)
        .build()
}

/// Where `regex-syntax` finds the pattern wrong, if it does.
pub fn syntax_error(pattern: &str, flags: Flags) -> Option<PatternError> {
    let err = syntax_parser(flags).parse(pattern).err()?;
    let (span, message) = match &err {
        regex_syntax::Error::Parse(e) => (*e.span(), e.kind().to_string()),
        regex_syntax::Error::Translate(e) => (*e.span(), e.kind().to_string()),
        other => return Some(PatternError { at: None, message: other.to_string() }),
    };
    let chars = Chars::of(pattern);
    let mut at = chars.range(span.start.offset..span.end.offset);
    if at.is_empty() {
        at.end = at.start + 1;
    }
    Some(PatternError { at: Some(at), message })
}

fn compile(pattern: &str, flags: Flags, flavour: Flavour) -> Result<Compiled, PatternError> {
    match flavour {
        Flavour::Rust => {
            if let Some(e) = syntax_error(pattern, flags) {
                return Err(e);
            }
            let error = |e: regex::Error| PatternError { at: None, message: first_line(&e.to_string()) };
            if flags.u {
                regex::RegexBuilder::new(pattern)
                    .case_insensitive(flags.i)
                    .multi_line(flags.m)
                    .dot_matches_new_line(flags.s)
                    .ignore_whitespace(flags.x)
                    .build()
                    .map(Compiled::Rust)
                    .map_err(error)
            } else {
                regex::bytes::RegexBuilder::new(pattern)
                    .case_insensitive(flags.i)
                    .multi_line(flags.m)
                    .dot_matches_new_line(flags.s)
                    .ignore_whitespace(flags.x)
                    .unicode(false)
                    .build()
                    .map(Compiled::Bytes)
                    .map_err(error)
            }
        }
        Flavour::Fancy => {
            let built = fancy_regex::RegexBuilder::new(pattern)
                .case_insensitive(flags.i)
                .multi_line(flags.m)
                .dot_matches_new_line(flags.s)
                .ignore_whitespace(flags.x)
                .unicode_mode(flags.u)
                .backtrack_limit(BACKTRACK)
                .build();
            built.map(Compiled::Fancy).map_err(|e| fancy_error(pattern, flags, e))
        }
    }
}

/// fancy-regex's error, at the place it says -- or, when it doesn't say,
/// where `regex-syntax` finds it reading the pattern with its lookaround
/// and backreferences put aside.
fn fancy_error(pattern: &str, flags: Flags, e: fancy_regex::Error) -> PatternError {
    let chars = Chars::of(pattern);
    match e {
        fancy_regex::Error::ParseError(at, kind) => {
            let at = chars.at(at);
            PatternError { at: Some(at..at + 1), message: kind.to_string() }
        }
        other => {
            let readable = syntax::readable(pattern, &syntax::scan(pattern, flags.x));
            let at = syntax_error(&readable, flags).and_then(|e| e.at);
            PatternError { at, message: first_line(&other.to_string()) }
        }
    }
}

fn first_line(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // regex's own errors draw the pattern with a caret; its last line
    // says what's wrong.
    lines.iter().rev().find(|l| l.starts_with("error:")).map(|l| l.trim_start_matches("error:").trim().to_string()).unwrap_or_else(|| lines.first().map(|l| l.to_string()).unwrap_or_default())
}

/// `pattern` with `flags` and `flavour` run over `text`, and `replacement`
/// put for each match when there is one. An empty pattern matches
/// nothing worth listing.
pub fn run(pattern: &str, flags: Flags, flavour: Flavour, text: &str, replacement: Option<&str>) -> Outcome {
    let mut out = Outcome::default();
    if pattern.is_empty() {
        return out;
    }
    let re = match compile(pattern, flags, flavour) {
        Ok(re) => re,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let chars = Chars::of(text);
    match &re {
        Compiled::Rust(re) => {
            out.names = re.capture_names().map(|n| n.map(str::to_string)).collect();
            for (n, caps) in re.captures_iter(text).enumerate() {
                if n == MOST {
                    out.more = true;
                    break;
                }
                out.matches.push(Match { groups: (0..caps.len()).map(|i| caps.get(i).map(|m| chars.range(m.range()))).collect() });
            }
            out.replaced = replacement.map(|r| re.replace_all(text, r).into_owned());
        }
        Compiled::Bytes(re) => {
            out.names = re.capture_names().map(|n| n.map(str::to_string)).collect();
            for (n, caps) in re.captures_iter(text.as_bytes()).enumerate() {
                if n == MOST {
                    out.more = true;
                    break;
                }
                out.matches.push(Match { groups: (0..caps.len()).map(|i| caps.get(i).map(|m| chars.range(m.range()))).collect() });
            }
            out.replaced = replacement.map(|r| String::from_utf8_lossy(&re.replace_all(text.as_bytes(), r.as_bytes())).into_owned());
        }
        Compiled::Fancy(re) => {
            out.names = re.capture_names().map(|n| n.map(str::to_string)).collect();
            for (n, caps) in re.captures_iter(text).enumerate() {
                if n == MOST {
                    out.more = true;
                    break;
                }
                match caps {
                    Ok(caps) => out.matches.push(Match { groups: (0..caps.len()).map(|i| caps.get(i).map(|m| chars.range(m.start()..m.end()))).collect() }),
                    Err(e) => {
                        out.stopped = Some(e.to_string());
                        break;
                    }
                }
            }
            out.replaced = replacement.and_then(|r| match re.try_replacen(text, 0, r) {
                Ok(t) => Some(t.into_owned()),
                Err(e) => {
                    out.stopped = Some(e.to_string());
                    None
                }
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str, r: Range<usize>) -> String {
        t.chars().skip(r.start).take(r.len()).collect()
    }

    #[test]
    fn matches_and_their_groups_numbered_and_named() {
        let t = "on 2026-10-29 and 2027-01-02";
        let o = run(r"(?P<year>\d{4})-(\d\d)-(\d\d)", Flags::default(), Flavour::Rust, t, None);
        assert_eq!(o.names, [None, Some("year".into()), None, None]);
        assert_eq!(o.matches.len(), 2);
        assert_eq!(text(t, o.matches[1].at()), "2027-01-02");
        assert_eq!(o.matches[0].groups[1], Some(3..7));
        assert_eq!(text(t, o.matches[0].groups[3].clone().unwrap()), "29");
    }

    #[test]
    fn positions_are_characters_not_bytes() {
        let o = run("é+", Flags::default(), Flavour::Rust, "ça éé", None);
        assert_eq!(o.matches[0].at(), 3..5);
    }

    #[test]
    fn a_group_that_took_no_part_is_none() {
        let o = run("(a)|(b)", Flags::default(), Flavour::Rust, "b", None);
        assert_eq!(o.matches[0].groups, [Some(0..1), None, Some(0..1)]);
    }

    #[test]
    fn flags_change_what_matches() {
        let t = "One\none";
        assert_eq!(run("^one$", Flags::default(), Flavour::Rust, t, None).matches.len(), 0);
        assert_eq!(run("^one$", Flags { m: true, ..Flags::default() }, Flavour::Rust, t, None).matches.len(), 1);
        assert_eq!(run("^one$", Flags { m: true, i: true, ..Flags::default() }, Flavour::Rust, t, None).matches.len(), 2);
        assert_eq!(run("e.o", Flags { s: true, ..Flags::default() }, Flavour::Rust, t, None).matches.len(), 1);
        assert_eq!(run("O n e # a comment", Flags { x: true, ..Flags::default() }, Flavour::Rust, t, None).matches.len(), 1);
        assert_eq!(run(r"\w+", Flags::default(), Flavour::Rust, "été", None).matches[0].at(), 0..3);
        let ascii = run(r"\w+", Flags { u: false, ..Flags::default() }, Flavour::Rust, "aé", None);
        assert_eq!(ascii.matches[0].at(), 0..1, "{ascii:?}");
        let bytes = run("a.b", Flags { u: false, ..Flags::default() }, Flavour::Rust, "a-b", Some("x"));
        assert_eq!((bytes.matches.len(), bytes.replaced.as_deref()), (1, Some("x")), "{bytes:?}");
    }

    #[test]
    fn the_replacement_live() {
        let o = run(r"(?P<y>\d{4})-(\d\d)", Flags::default(), Flavour::Rust, "2026-10, 2027-01", Some("$2/${y}"));
        assert_eq!(o.replaced.as_deref(), Some("10/2026, 01/2027"));
        let o = run(r"(\w)\1", Flags::default(), Flavour::Fancy, "aabcc", Some("<$1>"));
        assert_eq!(o.replaced.as_deref(), Some("<a>b<c>"));
    }

    #[test]
    fn an_error_is_at_its_place_in_the_pattern() {
        let e = run(r"ab(c", Flags::default(), Flavour::Rust, "", None).error.unwrap();
        assert_eq!(e.at, Some(2..3), "{e:?}");
        assert!(e.message.contains("unclosed group"), "{}", e.message);
        let e = run(r"a{2,1}", Flags::default(), Flavour::Rust, "", None).error.unwrap();
        assert_eq!(e.at, Some(1..6), "{e:?}");
        let e = run(r"x\p{Nope}", Flags::default(), Flavour::Rust, "", None).error.unwrap();
        assert_eq!(e.at.clone().map(|a| a.start), Some(1), "{e:?}");
    }

    #[test]
    fn lookaround_is_an_error_in_rust_and_matches_in_fancy() {
        let e = run(r"a(?=b)", Flags::default(), Flavour::Rust, "ab", None).error.unwrap();
        assert_eq!(e.at.clone().map(|a| a.start), Some(1));
        assert!(e.message.contains("look-around"), "{}", e.message);
        let o = run(r"a(?=b)", Flags::default(), Flavour::Fancy, "ab ac", None);
        assert_eq!(o.matches.len(), 1);
        assert_eq!(o.matches[0].at(), 0..1);
        let o = run(r"(?<=\$)\d+", Flags::default(), Flavour::Fancy, "cost $42", None);
        assert_eq!(o.matches[0].at(), 6..8);
    }

    #[test]
    fn a_fancy_error_has_its_place_too() {
        let e = run(r"(?=a)(b", Flags::default(), Flavour::Fancy, "", None).error.unwrap();
        assert!(e.at.is_some(), "{e:?}");
        let e = run(r"(a)\2", Flags::default(), Flavour::Fancy, "", None).error.unwrap();
        assert!(!e.message.is_empty());
    }

    #[test]
    fn the_list_stops_at_the_most() {
        let o = run("a", Flags::default(), Flavour::Rust, &"a".repeat(MOST + 5), None);
        assert_eq!(o.matches.len(), MOST);
        assert!(o.more);
    }

    #[test]
    fn flags_as_letters() {
        assert_eq!(Flags::default().letters(), "u");
        let mut f = Flags::from_letters("is");
        assert!(f.toggle('m') && !f.toggle('q'));
        assert_eq!(f.letters(), "ims");
    }
}
