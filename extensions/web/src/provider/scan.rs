//! The scanner: a JavaScript or TypeScript test file's text to the tests
//! in it, read token by token, nothing run. It finds `describe`, `suite`,
//! `it` and `test` (and `xit`, `xdescribe`, `fit`, `fdescribe`) called
//! with a string for a title, their `.skip`, `.todo`, `.only`, `.each`,
//! `.for`, `.skipIf(..)`, `.runIf(..)`, `.concurrent` and the like, at
//! their lines, inside the describes they're in.
//!
//! - A `.each` (or `.for`) test, and one whose title is a template literal
//!   with `${..}`, is a template: its cases come from the run, under it by
//!   their line or their title.
//! - A title that isn't a literal (`test(title, ..)`) isn't found: the
//!   listing or the run finds it. So isn't any test of a `describe` whose
//!   title isn't (or that's a `describe.each`): its names aren't known.
//! - Half-typed text is read as well as it can be: a string, a comment or
//!   a template literal left open ends with its line, a call left open is
//!   closed by the next `describe`/`it`/`test` at its indentation or
//!   left of it, so typing a test in the middle of a file keeps the tests
//!   after it (as Rust's scanner does). It never panics.

/// What the scan found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Scanned {
    pub tests: Vec<Test>,
    pub suites: Vec<Suite>,
}

/// A test, its names those of the describes it's in then its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Test {
    pub names: Vec<String>,
    /// Its call's first and last line, from 1.
    pub line: u32,
    pub end_line: Option<u32>,
    /// Why it's left out: "" for `.skip` (and its describe's), "todo".
    pub skip: Option<String>,
    pub template: bool,
}

/// A `describe`.
#[derive(Debug, Clone, PartialEq)]
pub struct Suite {
    pub names: Vec<String>,
    pub line: u32,
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok<'a> {
    Ident(&'a str),
    /// A string or template literal: its value, and whether it holds `${`.
    Str(String, bool),
    Punct(u8),
}

#[derive(Debug, Clone)]
struct Token<'a> {
    tok: Tok<'a>,
    line: u32,
    /// Whether it starts its line, and the line's indentation (in
    /// bytes, a tab one).
    first: bool,
    indent: u32,
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The text's tokens: identifiers, literals and punctuation; comments,
/// numbers and regex literals left out. Read as bytes: what isn't ASCII
/// is only ever inside a literal or an identifier.
fn lex(text: &str) -> Vec<Token<'_>> {
    let b = text.as_bytes();
    let mut out: Vec<Token> = Vec::with_capacity(b.len() / 6);
    let (mut i, mut line, mut indent, mut first) = (0usize, 1u32, 0u32, true);
    let mut at_start = true;
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            indent = 0;
            first = true;
            at_start = true;
            i += 1;
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' || c == 0x0b || c == 0x0c {
            if at_start {
                indent += 1;
            }
            i += 1;
            continue;
        }
        at_start = false;
        let next = b.get(i + 1).copied();
        let tok_line = line;
        let tok = if c == b'/' && next == Some(b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if c == b'/' && next == Some(b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] == b'\n' {
                    line += 1;
                    indent = 0;
                    first = true;
                }
                i += 1;
            }
            i += 2;
            continue;
        } else if c == b'\'' || c == b'"' {
            let (value, end) = string(text, i + 1, c);
            i = end;
            Tok::Str(value, false)
        } else if c == b'`' {
            let (value, expr, end, lines) = template(text, i + 1);
            i = end;
            line += lines;
            Tok::Str(value, expr)
        } else if c == b'/' && regex_allowed(out.last()) {
            i = regex_end(b, i + 1);
            continue;
        } else if c.is_ascii_digit() {
            while i < b.len() && (is_ident(b[i]) || b[i] == b'.') {
                i += 1;
            }
            Tok::Punct(b'0')
        } else if is_ident(c) {
            let s = i;
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            // Never inside a character: an identifier's bytes stop at ASCII.
            Tok::Ident(text.get(s..i).unwrap_or(""))
        } else {
            i += 1;
            Tok::Punct(c)
        };
        out.push(Token { tok, line: tok_line, first, indent });
        first = false;
    }
    out
}

/// A `'` or `"` string from byte `i`: its value, and where it ends. One
/// left open ends with its line.
fn string(text: &str, mut i: usize, quote: u8) -> (String, usize) {
    let b = text.as_bytes();
    let mut value = String::new();
    let mut from = i;
    while i < b.len() {
        match b[i] {
            c if c == quote => {
                value.push_str(text.get(from..i).unwrap_or(""));
                return (value, i + 1);
            }
            b'\n' => break,
            b'\\' => {
                value.push_str(text.get(from..i).unwrap_or(""));
                let (c, next) = escape(text, i + 1);
                value.extend(c);
                i = next;
                from = i;
            }
            _ => i += 1,
        }
    }
    value.push_str(text.get(from..i).unwrap_or(""));
    (value, i)
}

/// An escape after its `\` at byte `i`: the character it is, and what's
/// after.
fn escape(text: &str, i: usize) -> (Option<char>, usize) {
    let b = text.as_bytes();
    let Some(&c) = b.get(i) else { return (None, i) };
    let hex = |from: usize, to: usize| -> Option<char> { u32::from_str_radix(text.get(from..to)?, 16).ok().and_then(char::from_u32) };
    match c {
        b'n' => (Some('\n'), i + 1),
        b't' => (Some('\t'), i + 1),
        b'r' => (Some('\r'), i + 1),
        b'0' => (Some('\0'), i + 1),
        b'b' => (Some('\u{8}'), i + 1),
        b'f' => (Some('\u{c}'), i + 1),
        b'v' => (Some('\u{b}'), i + 1),
        // A line continued.
        b'\n' => (None, i + 1),
        b'x' => match hex(i + 1, i + 3) {
            Some(c) => (Some(c), i + 3),
            None => (Some('x'), i + 1),
        },
        b'u' if b.get(i + 1) == Some(&b'{') => {
            let end = b[i..].iter().position(|&c| c == b'}' || c == b'\n').map(|p| i + p);
            match end.filter(|&e| b[e] == b'}').and_then(|e| hex(i + 2, e).map(|c| (c, e))) {
                Some((c, e)) => (Some(c), e + 1),
                None => (Some('u'), i + 1),
            }
        }
        b'u' => match hex(i + 1, i + 5) {
            Some(c) => (Some(c), i + 5),
            None => (Some('u'), i + 1),
        },
        _ => {
            // The character escaped, whole.
            let ch = text.get(i..).and_then(|r| r.chars().next());
            (ch, i + ch.map_or(1, char::len_utf8))
        }
    }
}

/// The end of the line byte `from` is on.
fn line_end(b: &[u8], from: usize) -> usize {
    from + b[from.min(b.len())..].iter().position(|&c| c == b'\n').unwrap_or(b.len().saturating_sub(from))
}

/// A template literal from byte `start`: its text (`${..}` kept as
/// written), whether it holds one, where it ends, and how many lines it
/// spans. One that never closes, or whose `${` doesn't on its line, ends
/// with its first line.
fn template(text: &str, start: usize) -> (String, bool, usize, u32) {
    let b = text.as_bytes();
    let mut i = start;
    let mut value = String::new();
    let mut from = i;
    let mut expr = false;
    let mut lines = 0;
    let cut = |b: &[u8]| {
        let end = line_end(b, start);
        let raw = text.get(start..end).unwrap_or("").to_string();
        let expr = raw.contains("${");
        (raw, expr, end, 0)
    };
    while i < b.len() {
        match b[i] {
            b'`' => {
                value.push_str(text.get(from..i).unwrap_or(""));
                return (value, expr, i + 1, lines);
            }
            b'\\' => {
                value.push_str(text.get(from..i).unwrap_or(""));
                let (c, next) = escape(text, i + 1);
                value.extend(c);
                i = next;
                from = i;
            }
            b'$' if b.get(i + 1) == Some(&b'{') => {
                expr = true;
                let mut depth = 0;
                while i < b.len() {
                    match b[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        b'\n' | b'`' => return cut(b),
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\n' => {
                lines += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    cut(b)
}

/// Whether a `/` after `prev` starts a regex literal, not a division.
fn regex_allowed(prev: Option<&Token>) -> bool {
    match prev.map(|t| &t.tok) {
        None => true,
        Some(Tok::Ident(w)) => matches!(*w, "return" | "typeof" | "case" | "do" | "else" | "in" | "of" | "new" | "delete" | "void" | "throw" | "await" | "yield"),
        Some(Tok::Str(..)) => false,
        Some(Tok::Punct(c)) => !matches!(c, b')' | b']' | b'}' | b'0'),
    }
}

/// Past a regex literal's end, on its line.
fn regex_end(b: &[u8], mut i: usize) -> usize {
    let mut class = false;
    while i < b.len() {
        match b[i] {
            b'\n' => return i,
            b'\\' => i += 1,
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => {
                i += 1;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                return i;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Suite,
    Test,
}

/// A call being read: a describe's or a test's, open until its `)`.
#[derive(Debug)]
struct Open {
    kind: Kind,
    /// Its place in `tests` or `suites`; none for one not reported.
    at: Option<usize>,
    /// The names its tests are under (a describe's), or none when they
    /// can't be known.
    names: Option<Vec<String>>,
    skip: Option<String>,
    indent: u32,
    /// The stack's depth with its `(` on it.
    depth: usize,
}

/// The modifiers a call can have, and whether each takes arguments of its
/// own (`.each(table)`, `.skipIf(cond)`).
fn modifier(name: &str) -> Option<bool> {
    match name {
        "each" | "for" | "skipIf" | "runIf" => Some(true),
        "skip" | "todo" | "only" | "concurrent" | "sequential" | "fails" | "shuffle" | "failing" => Some(false),
        _ => None,
    }
}

/// The tests and describes in `text`.
pub fn scan(text: &str) -> Scanned {
    let toks = lex(text);
    let mut out = Scanned::default();
    // Brackets open: what closes each, and the call it opened if any.
    let mut stack: Vec<(u8, Option<usize>)> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        match &t.tok {
            Tok::Punct(c @ (b'(' | b'[' | b'{')) => {
                stack.push((closer(*c), None));
                i += 1;
            }
            Tok::Punct(c @ (b')' | b']' | b'}')) => {
                if let Some(at) = stack.iter().rposition(|(close, _)| close == c) {
                    for (_, call) in stack.drain(at..).rev() {
                        if call.is_some() {
                            close(&mut open, &mut out, Some(t.line));
                        }
                    }
                }
                i += 1;
            }
            Tok::Ident(word) if is_root(word) && !after_dot_or_function(&toks, i) => {
                match call(&toks, i) {
                    Some(c) => {
                        // Recovery: a call at a line's start closes those
                        // left open at its indentation or to its right.
                        if t.first {
                            while let Some(o) = open.last() {
                                if o.indent < t.indent {
                                    break;
                                }
                                let depth = o.depth;
                                stack.truncate(depth - 1);
                                close(&mut open, &mut out, toks.get(i.saturating_sub(1)).map(|p| p.line));
                            }
                        }
                        let parent = open.iter().rev().find(|o| o.kind == Kind::Suite);
                        let inside_test = open.iter().any(|o| o.kind == Kind::Test);
                        let names = match parent {
                            Some(p) => p.names.clone(),
                            None => Some(Vec::new()),
                        };
                        let inherited = parent.and_then(|p| p.skip.clone());
                        let skip = c.skip.clone().or(inherited);
                        let mut at = None;
                        let mut own_names = None;
                        if let (Some(mut names), Some((title, expr)), false) = (names, c.title.clone(), inside_test) {
                            match c.kind {
                                Kind::Test => {
                                    names.push(title);
                                    at = Some(out.tests.len());
                                    out.tests.push(Test { names, line: t.line, end_line: None, skip: skip.clone(), template: c.each || expr });
                                }
                                // A describe.each's or a `${..}` title's names
                                // are the run's: its tests aren't found.
                                Kind::Suite if !c.each && !expr => {
                                    names.push(title);
                                    at = Some(out.suites.len());
                                    out.suites.push(Suite { names: names.clone(), line: t.line, end_line: None });
                                    own_names = Some(names);
                                }
                                Kind::Suite => {}
                            }
                        }
                        stack.push((b')', Some(open.len())));
                        open.push(Open { kind: c.kind, at, names: own_names, skip, indent: t.indent, depth: stack.len() });
                        i = c.after;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut out, None);
    }
    out
}

fn closer(c: u8) -> u8 {
    match c {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

/// The last call open ends at `line`.
fn close(open: &mut Vec<Open>, out: &mut Scanned, line: Option<u32>) {
    let Some(o) = open.pop() else { return };
    match (o.kind, o.at) {
        (Kind::Test, Some(at)) => out.tests[at].end_line = line,
        (Kind::Suite, Some(at)) => out.suites[at].end_line = line,
        _ => {}
    }
}

fn is_root(word: &str) -> bool {
    matches!(word, "describe" | "suite" | "it" | "test" | "xit" | "xtest" | "xdescribe" | "fit" | "fdescribe")
}

/// `x.test(..)` (a regex's) and `function test(..)` aren't tests.
fn after_dot_or_function(toks: &[Token], i: usize) -> bool {
    match i.checked_sub(1).map(|p| &toks[p].tok) {
        Some(Tok::Punct(b'.')) => true,
        Some(Tok::Ident(w)) => matches!(*w, "function" | "const" | "let" | "var" | "class" | "async"),
        _ => false,
    }
}

/// A call read from its first word.
#[derive(Debug, Clone)]
struct Call {
    kind: Kind,
    each: bool,
    skip: Option<String>,
    /// Its title, when it's a literal: its text, and whether it holds `${`.
    title: Option<(String, bool)>,
    /// The token after its `(` and title: where reading goes on.
    after: usize,
}

/// The call that starts at `i` (`describe`, `test.skip.each(..)(`), up to
/// its `(` and its title: none if it isn't called.
fn call(toks: &[Token], i: usize) -> Option<Call> {
    let Tok::Ident(root) = &toks[i].tok else { return None };
    let (kind, mut skip) = match *root {
        "describe" | "suite" | "fdescribe" => (Kind::Suite, None),
        "xdescribe" => (Kind::Suite, Some(String::new())),
        "xit" | "xtest" => (Kind::Test, Some(String::new())),
        _ => (Kind::Test, None),
    };
    let mut each = false;
    let mut j = i + 1;
    loop {
        match toks.get(j).map(|t| &t.tok) {
            Some(Tok::Punct(b'.')) => {
                let Some(Tok::Ident(m)) = toks.get(j + 1).map(|t| &t.tok) else { return None };
                let takes = modifier(m)?;
                match *m {
                    "skip" => skip = Some(String::new()),
                    "todo" => skip = Some("todo".into()),
                    "each" | "for" => each = true,
                    _ => {}
                }
                j += 2;
                if takes {
                    match toks.get(j).map(|t| &t.tok) {
                        Some(Tok::Punct(b'(')) => j = past_group(toks, j)?,
                        // A tagged template table: test.each`a | b`.
                        Some(Tok::Str(..)) => j += 1,
                        _ => return None,
                    }
                }
            }
            Some(Tok::Punct(b'(')) => break,
            _ => return None,
        }
    }
    // j is the call's `(`.
    let title = match toks.get(j + 1).map(|t| &t.tok) {
        Some(Tok::Str(s, expr)) if matches!(toks.get(j + 2).map(|t| &t.tok), Some(Tok::Punct(b',' | b')')) | None) => Some((s.clone(), *expr)),
        _ => None,
    };
    let after = if title.is_some() { j + 2 } else { j + 1 };
    Some(Call { kind, each, skip, title, after })
}

/// Past the group whose `(` is at `j`; none when it doesn't close.
fn past_group(toks: &[Token], j: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (k, t) in toks.iter().enumerate().skip(j) {
        match t.tok {
            Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
            Tok::Punct(b')' | b']' | b'}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(k + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &Scanned) -> Vec<String> {
        s.tests.iter().map(|t| t.names.join(" > ")).collect()
    }

    #[test]
    fn tests_and_describes_at_their_lines() {
        let s = scan(include_str!("../samples/tests/vitest/orbit/src/orbit.test.js"));
        assert_eq!(
            names(&s),
            [
                "adds", "fails on purpose", "skipped", "to do", "add(%i, %i) is %i", "outer > in describe", "outer > inner > deep",
                "names > twice", "names > twice", "names > add(-1, -1) [regex] $x", "names > adds ${n}", "left out > with its describe",
            ]
        );
        let t = |n: &str| s.tests.iter().find(|t| t.names.join(" > ") == n).unwrap();
        assert_eq!((t("adds").line, t("adds").end_line), (4, Some(6)));
        assert_eq!((t("skipped").skip.as_deref(), t("to do").skip.as_deref()), (Some(""), Some("todo")));
        assert!(t("add(%i, %i) is %i").template && t("names > adds ${n}").template && !t("adds").template);
        assert_eq!((t("add(%i, %i) is %i").line, t("add(%i, %i) is %i").end_line), (16, Some(21)));
        assert_eq!(t("left out > with its describe").skip.as_deref(), Some(""), "its describe's skip");
        assert_eq!(s.suites.iter().map(|s| (s.names.join(" > "), s.line, s.end_line)).collect::<Vec<_>>(), [("outer".into(), 23, Some(30)), ("outer > inner".into(), 25, Some(29)), ("names".into(), 32, Some(45)), ("left out".into(), 47, Some(49))]);
    }

    #[test]
    fn what_isnt_a_test_isnt_and_odd_text_doesnt_panic() {
        let text = r#"
// test('in a comment', () => {})
/* it('in a block', () => {}) */
const s = "test('in a string')"
if (/test\(/.test(s)) {}
function test(x) { return x }
expect(it).toBe(1)
test(name, () => {})
describe(title, () => {
  test('under a describe whose name is the run's', () => {})
})
describe.each([1, 2])('case %i', (n) => {
  test('in each', () => {})
})
test('escaped \'quote\' and é', () => {})
test("double", function () {})
it.skipIf(process.env.CI)('skipped if', () => {})
test.each`
  a | b
  ${1} | ${2}
`('table $a', ({ a }) => {})
"#;
        let s = scan(text);
        assert_eq!(names(&s), ["escaped 'quote' and \u{e9}", "double", "skipped if", "table $a"]);
        assert!(s.tests[3].template);
        for odd in ["test('open", "describe('x', () => {\n  test(`a ${", "it(\u{0}\u{ffff}'\\", "`", "/", "test.each(", "describe('é', () => { it('ü'", "\\u{110000}'", "test('a', () => { /* never closed"] {
            let _ = scan(odd);
        }
        assert_eq!(names(&scan("test('open")), ["open"], "a string left open ends with its line");
    }

    #[test]
    fn a_test_half_typed_keeps_the_tests_after_it() {
        let text = "describe('outer', () => {\n  test('first', () => {\n    expect(1).toBe(\n  test('typing', () => {\n  test('after', () => {})\n})\n\ntest('top', () => {})\n";
        let s = scan(text);
        assert_eq!(names(&s), ["outer > first", "outer > typing", "outer > after", "top"]);
        assert_eq!(s.tests[0].end_line, Some(3), "closed by the next test at its indentation");
    }

    #[test]
    fn a_title_on_its_own_line_and_nested_braces() {
        let text = "test(\n  'long title',\n  async () => {\n    const o = { a: [1, (2)] }\n  },\n)\n\nit('after', () => {})\n";
        let s = scan(text);
        assert_eq!(names(&s), ["long title", "after"]);
        assert_eq!((s.tests[0].line, s.tests[0].end_line), (1, Some(6)));
    }
}
