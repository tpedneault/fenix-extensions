//! The tests in a Rust file's text, nothing run: a line at a time, with a
//! lexer's state carried from line to line (block comments, strings, raw
//! strings), braces counted outside them, and the item each brace opens
//! kept on a stack.
//!
//! A test is an `fn` after `#[test]`, any `#[<path>::test]`
//! (`#[tokio::test]`, `#[sqlx::test]`), `#[quickcheck]` or `#[proptest]`;
//! `#[rstest]` with `#[case]`s or `#[values]`, `#[test_case(..)]` and
//! `#[test_matrix(..)]` are templates, their cases found when run.
//! `#[ignore]` and `#[ignore = "why"]` give its `skip`. Inline `mod`s are
//! its suites and its path; an `fn` inside an `impl`, another `fn` or a
//! `macro_rules!` isn't a test, nor one whose name is a macro's `$name`.
//! A macro-made test (`cases! { .. }`) has no `fn` to find: a listing or a
//! run finds it. `#[path]` and `include!` aren't followed.
//!
//! A scan of half-typed text still finds what it can: an item that starts
//! a line at the indentation of an item still open (an `fn` whose brace
//! isn't closed yet) closes it, so the tests after it stay found.

/// A test as the scan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// Its path below the file's module: `tests::adds`.
    pub path: Vec<String>,
    /// The line of its `fn`, from 1, and of its closing brace.
    pub line: u32,
    pub end_line: Option<u32>,
    pub skip: Option<String>,
    pub template: bool,
}

/// An inline module: its path, and its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub path: Vec<String>,
    pub line: u32,
    pub end_line: Option<u32>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub tests: Vec<Scanned>,
    pub modules: Vec<Module>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// `mod name {`: its index in `modules`.
    Mod(usize),
    /// A test's body: its index in `tests`.
    Test(usize),
    /// `name! {` -- a macro call, whose items can be tests (`proptest!`).
    Call,
    /// `macro_rules! {`: nothing in it is a test.
    Rules,
    /// Any other brace: an `impl`, an `fn`'s body, a block.
    Block,
}

/// What the next `{` opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Opening {
    #[default]
    Nothing,
    Mod(Module),
    /// A test, its parameters being read (`#[values]` makes it a template).
    Test(Scanned),
    Call,
    Rules,
}

/// A test attribute seen, waiting for its `fn`.
#[derive(Debug, Default, Clone)]
struct Pending {
    test: bool,
    template: bool,
    rstest: bool,
    cases: bool,
    skip: Option<String>,
}

impl Pending {
    fn any(&self) -> bool {
        self.test || self.template || self.rstest || self.cases || self.skip.is_some()
    }
}

/// The last word, as far as items go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Last {
    #[default]
    Nothing,
    Fn,
    Mod,
    MacroRules,
    Other,
}

/// Words that may come between an item's attributes and its `fn`.
const QUALIFIERS: &[&str] = &["pub", "crate", "super", "self", "in", "async", "unsafe", "const", "extern", "default"];

/// Words that start an item at a line's start, for half-typed text.
const STARTS: &[&str] = &["fn", "mod", "pub", "async", "unsafe"];

#[derive(Default)]
struct Scanner {
    out: Scan,
    /// What each open brace opened, and its line's indentation.
    stack: Vec<(Frame, usize)>,
    opening: Opening,
    pending: Pending,
    /// An attribute's text so far while its brackets are open, how deep,
    /// and whether it's an inner one (`#![..]`), which is dropped.
    attribute: Option<(String, i32, bool)>,
    /// The last word since the last `{`, `}` or `;`.
    last: Last,
    /// The lexer, across lines.
    block_comment: u32,
    string: Option<Option<usize>>,
}

/// A byte of an identifier: ASCII's letters, digits and `_`, and any
/// byte of a character past ASCII.
fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl Scanner {
    /// The path of the innermost module open: the modules' own paths
    /// say it, so it's the last open one's.
    fn module_path(&self) -> Vec<String> {
        self.stack.iter().rev().find_map(|(f, _)| if let Frame::Mod(i) = f { Some(self.out.modules[*i].path.clone()) } else { None }).unwrap_or_default()
    }

    /// Items here can be tests: only modules and macro calls are open.
    fn testable(&self) -> bool {
        self.stack.iter().all(|(f, _)| matches!(f, Frame::Mod(_) | Frame::Call))
    }

    /// Half-typed text: an item starting a line at the indentation of an
    /// `fn` or block still open closes it, as rustfmt's layout says it
    /// ended. Modules and macro calls are only closed by their braces.
    fn recover(&mut self, indent: usize) {
        while let Some(&(frame, at)) = self.stack.last() {
            if at < indent || matches!(frame, Frame::Mod(_) | Frame::Call | Frame::Rules) {
                break;
            }
            self.stack.pop();
        }
    }

    fn attribute(&mut self, text: &str) {
        let inner = text.trim();
        let (path, rest) = match inner.find(|c: char| c == '(' || c == '=' || c.is_whitespace()) {
            Some(at) => (inner[..at].to_string(), inner[at..].trim()),
            None => (inner.to_string(), ""),
        };
        let path = path.replace(' ', "");
        if let Opening::Test(t) = &mut self.opening {
            // A parameter's: `#[values(..)]`, `#[files(..)]`.
            if matches!(path.as_str(), "values" | "files") {
                t.template = true;
            }
            return;
        }
        let last = path.rsplit("::").next().unwrap_or(&path);
        match path.as_str() {
            "test" | "quickcheck" | "proptest" => self.pending.test = true,
            "rstest" | "rstest::rstest" => {
                self.pending.test = true;
                self.pending.rstest = true;
            }
            "test_case" | "test_case::test_case" | "test_matrix" | "test_case::test_matrix" => {
                self.pending.test = true;
                self.pending.template = true;
            }
            "case" | "values" => self.pending.cases = true,
            "ignore" => self.pending.skip = Some(rest.strip_prefix('=').map(|r| r.trim().trim_matches('"').to_string()).unwrap_or_default()),
            _ if path.starts_with("case::") => self.pending.cases = true,
            _ if last == "test" && path.contains("::") => self.pending.test = true,
            _ => {}
        }
    }

    /// A word of code, outside strings and comments.
    fn word(&mut self, word: &str, line: u32) {
        match self.last {
            Last::Fn => {
                let p = std::mem::take(&mut self.pending);
                if p.test && self.testable() && !word.starts_with('$') {
                    let mut path = self.module_path();
                    path.push(word.to_string());
                    self.opening = Opening::Test(Scanned { path, line, end_line: None, skip: p.skip, template: p.template || (p.rstest && p.cases) });
                } else {
                    self.opening = Opening::Nothing;
                }
            }
            Last::Mod if !word.starts_with('$') => {
                let mut path = self.module_path();
                path.push(word.to_string());
                self.opening = Opening::Mod(Module { path, line, end_line: None });
                self.pending = Pending::default();
            }
            // Another item, or code: the attributes weren't a test's.
            _ if self.pending.any() && word != "fn" && word != "mod" && !QUALIFIERS.contains(&word) && !matches!(self.opening, Opening::Test(_)) => self.pending = Pending::default(),
            _ => {}
        }
        self.last = match word {
            "fn" => Last::Fn,
            "mod" => Last::Mod,
            "macro_rules" => Last::MacroRules,
            _ => Last::Other,
        };
    }

    fn open(&mut self, indent: usize) {
        let frame = match std::mem::take(&mut self.opening) {
            Opening::Mod(m) => {
                self.out.modules.push(m);
                Frame::Mod(self.out.modules.len() - 1)
            }
            Opening::Test(t) => {
                self.out.tests.push(t);
                Frame::Test(self.out.tests.len() - 1)
            }
            Opening::Call => Frame::Call,
            Opening::Rules => Frame::Rules,
            Opening::Nothing => Frame::Block,
        };
        self.stack.push((frame, indent));
        self.last = Last::Nothing;
        self.pending = Pending::default();
    }

    fn close(&mut self, line: u32) {
        match self.stack.pop() {
            Some((Frame::Mod(i), _)) => self.out.modules[i].end_line = Some(line),
            Some((Frame::Test(i), _)) => self.out.tests[i].end_line = Some(line),
            _ => {}
        }
        self.last = Last::Nothing;
        self.opening = Opening::Nothing;
        self.pending = Pending::default();
    }

    fn line(&mut self, text: &str, line: u32) {
        let indent = text.len() - text.trim_start().len();
        // Bytes: every byte this reads is ASCII, and a character past it
        // is only ever part of a word, a string or a comment.
        let b = text.as_bytes();
        let at = |i: usize| b.get(i).copied();
        // The indentation is read: past it, unless a comment or a string
        // goes on from the line before.
        let mut i = if self.block_comment > 0 || self.string.is_some() || self.attribute.is_some() { 0 } else { indent };
        // Still at the line's start: nothing but space read yet.
        let mut first = true;
        while i < b.len() {
            let c = b[i];
            let next = at(i + 1);
            if c == b' ' && self.block_comment == 0 && self.string.is_none() && self.attribute.is_none() {
                i += 1;
                continue;
            }
            if self.block_comment > 0 {
                if c == b'*' && next == Some(b'/') {
                    self.block_comment -= 1;
                    i += 2;
                } else if c == b'/' && next == Some(b'*') {
                    self.block_comment += 1;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(raw) = self.string {
                match raw {
                    None if c == b'\\' => i += 2,
                    None if c == b'"' => {
                        self.string = None;
                        i += 1;
                    }
                    Some(hashes) if c == b'"' && b[i + 1..].iter().take(hashes).filter(|h| **h == b'#').count() == hashes => {
                        self.string = None;
                        i += 1 + hashes;
                    }
                    _ => i += 1,
                }
                continue;
            }
            if c == b'/' && next == Some(b'/') {
                break;
            }
            if c == b'/' && next == Some(b'*') {
                self.block_comment = 1;
                i += 2;
                continue;
            }
            if c == b'"' {
                // In an attribute a string's text is kept (`#[ignore = "slow"]`).
                if let Some((t, _, _)) = &mut self.attribute {
                    if let Some(end) = b[i + 1..].iter().position(|q| *q == b'"').filter(|e| !b[i + 1..i + 1 + e].contains(&b'\\')) {
                        t.push_str(text.get(i..i + 2 + end).unwrap_or(""));
                        i += 2 + end;
                        continue;
                    }
                }
                self.string = Some(None);
                i += 1;
                continue;
            }
            if (c == b'r' || c == b'b') && !(i > 0 && ident(b[i - 1])) {
                // r"..", r#".."#, br"..", b"..".
                let j = if c == b'b' && next == Some(b'r') { i + 2 } else { i + 1 };
                let raw = c == b'r' || j == i + 2;
                let hashes = b[j.min(b.len())..].iter().take_while(|h| **h == b'#').count();
                if at(j + hashes) == Some(b'"') && (raw || hashes == 0) {
                    self.string = Some(raw.then_some(hashes));
                    i = j + hashes + 1;
                    continue;
                }
            }
            if c == b'\'' {
                // A char literal ('{', '\'', '\u{7b}', 'é') or a lifetime ('a).
                let width = text.get(i + 1..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
                if next == Some(b'\\') {
                    i = b.get(i + 3..).and_then(|r| r.iter().position(|q| *q == b'\'')).map_or(b.len(), |e| i + 4 + e);
                } else if at(i + 1 + width) == Some(b'\'') {
                    i += 2 + width;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some((t, depth, _)) = &mut self.attribute {
                match c {
                    b'[' | b'(' | b'{' => *depth += 1,
                    b']' | b')' | b'}' => *depth -= 1,
                    _ => {}
                }
                if *depth == 0 {
                    let (t, _, inner) = self.attribute.take().expect("open");
                    if !inner {
                        self.attribute(&t);
                    }
                    i += 1;
                } else {
                    // A whole character: one past ASCII is several bytes.
                    let width = text.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
                    t.push_str(text.get(i..i + width).unwrap_or(""));
                    i += width;
                }
                continue;
            }
            if c == b'#' && (next == Some(b'[') || (next == Some(b'!') && at(i + 2) == Some(b'['))) {
                if first {
                    self.recover(indent);
                    first = false;
                }
                let inner = next == Some(b'!');
                self.attribute = Some((String::new(), 1, inner));
                i += if inner { 3 } else { 2 };
                continue;
            }
            if ident(c) || (c == b'$' && next.is_some_and(ident)) {
                let start = i;
                i += 1;
                while i < b.len() && ident(b[i]) {
                    i += 1;
                }
                // Never a panic on odd text: a word that isn't whole
                // characters (after a string's `\` before a wide one) is "".
                let word = text.get(start..i).unwrap_or("");
                if first && STARTS.contains(&word) {
                    self.recover(indent);
                }
                first = false;
                self.word(word, line);
                continue;
            }
            match c {
                // `name!` -- right after its name.
                b'!' if i > 0 && ident(b[i - 1]) => {
                    self.opening = if self.last == Last::MacroRules { Opening::Rules } else { Opening::Call };
                    self.pending = Pending::default();
                }
                b'(' | b'[' if matches!(self.opening, Opening::Call | Opening::Rules) => {
                    // `x!(..)`, `x![..]`: a call, holding no items to find.
                    self.opening = Opening::Nothing;
                }
                b'{' => self.open(indent),
                b'}' => self.close(line),
                b';' => {
                    self.last = Last::Nothing;
                    self.opening = Opening::Nothing;
                    self.pending = Pending::default();
                }
                _ => {}
            }
            first = first && c.is_ascii_whitespace();
            i += 1;
        }
    }
}

/// The tests and inline modules in `text`.
pub fn scan(text: &str) -> Scan {
    let mut s = Scanner::default();
    for (n, line) in text.lines().enumerate() {
        s.line(line, n as u32 + 1);
    }
    s.out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str = include_str!("../samples/tests/cargo/orbit/src/lib.rs");

    type Row<'a> = (String, u32, Option<u32>, Option<&'a str>, bool);

    fn rows(s: &Scan) -> Vec<Row<'_>> {
        s.tests.iter().map(|t| (t.path.join("::"), t.line, t.end_line, t.skip.as_deref(), t.template)).collect()
    }

    #[test]
    fn the_samples_tests_at_their_lines_in_their_modules() {
        let s = scan(LIB);
        assert_eq!(
            rows(&s),
            [
                ("tests::adds".into(), 18, Some(20), None, false),
                ("tests::fails_on_purpose".into(), 23, Some(26), None, false),
                ("tests::ignored_one".into(), 30, Some(30), Some("slow"), false),
                ("tests::panics_as_it_should".into(), 34, Some(36), None, false),
                ("tests::nested::deep".into(), 50, Some(50), None, false),
            ],
            "the macro's `#[test] fn $name` and the cases it makes aren't scanned"
        );
        let mods: Vec<(String, u32, Option<u32>)> = s.modules.iter().map(|m| (m.path.join("::"), m.line, m.end_line)).collect();
        assert_eq!(mods, [("tests".to_string(), 14, Some(52)), ("tests::nested".into(), 48, Some(51))]);
    }

    #[test]
    fn attributes_of_every_kind_and_what_isnt_a_test() {
        let text = r#####"
#[cfg(test)]
mod tests {
    #[tokio::test(flavor = "multi_thread")]
    async fn talks() {}

    #[test]
    #[ignore]
    #[should_panic(expected = "}")]
    pub(crate) fn plain() { let s = "{ not a brace"; let c = '{'; let r = r#"}"#; }

    #[rstest]
    #[case(1, 2)]
    #[case::negative(-1, -2)]
    fn sums(#[case] a: i32, #[case] b: i32) {}

    #[rstest]
    fn with_a_fixture(answer: u32) {}

    #[rstest]
    fn matrix(#[values(1, 2)] x: u8) {}

    #[test_case(1 ; "one")]
    #[test_case(
        2 ; "two"
    )]
    fn cased(x: u8) {}

    /* #[test] fn commented() {} */
    // #[test] fn also_commented() {}

    struct S;
    impl S {
        #[test]
        fn in_an_impl() {}
    }

    fn helper() {
        #[test]
        fn inside_a_fn() {}
    }

    proptest! {
        #[test]
        fn proptested(x in 0..10u8) {}
    }

    #[ignore = "needs a network"] #[test] fn one_line() {}

    #[sqlx::test]
    async fn queries(pool: PgPool) {}
}
"#####;
        let s = scan(text);
        assert_eq!(
            rows(&s),
            [
                ("tests::talks".into(), 5, Some(5), None, false),
                ("tests::plain".into(), 10, Some(10), Some(""), false),
                ("tests::sums".into(), 15, Some(15), None, true),
                ("tests::with_a_fixture".into(), 18, Some(18), None, false),
                ("tests::matrix".into(), 21, Some(21), None, true),
                ("tests::cased".into(), 27, Some(27), None, true),
                ("tests::proptested".into(), 45, Some(45), None, false),
                ("tests::one_line".into(), 48, Some(48), Some("needs a network"), false),
                ("tests::queries".into(), 51, Some(51), None, false),
            ]
        );
    }

    #[test]
    fn half_typed_text_keeps_the_tests_after_it() {
        let text = "mod tests {\n    #[test]\n    fn being_typed() {\n        let x = 1;\n\n    #[test]\n    fn after() {}\n}\n";
        let s = scan(text);
        let names: Vec<String> = s.tests.iter().map(|t| t.path.join("::")).collect();
        assert_eq!(names, ["tests::being_typed", "tests::after"]);
        assert_eq!(s.tests[0].end_line, None, "not closed: no end");
        assert_eq!(s.modules[0].end_line, Some(8));
    }

    #[test]
    fn odd_text_is_read_without_a_panic() {
        let text = "mod tests {\n    #[test]\n    fn größe() { let c = 'é'; let s = \"\\é {\"; }\n    #[test]\n    fn after() {}\n    #[test] fn unclosed() { let s = \"\n}\n";
        let s = scan(text);
        let names: Vec<String> = s.tests.iter().map(|t| t.path.join("::")).collect();
        assert_eq!(names, ["tests::größe", "tests::after", "tests::unclosed"]);
        assert!(scan("/* never closed\n#[test] fn x() {}").tests.is_empty());
        assert!(scan("#[test").tests.is_empty());
    }

    #[test]
    fn two_thousand_tests_scan_in_a_few_milliseconds() {
        let text: String = std::iter::once("#[cfg(test)]\nmod tests {\n".to_string()).chain((0..2000).map(|i| format!("    #[test]\n    fn case_{i}() {{\n        assert_eq!(1 + 1, 2, \"{{}}\");\n    }}\n\n"))).chain(std::iter::once("}\n".to_string())).collect();
        let start = std::time::Instant::now();
        let s = scan(&text);
        let took = start.elapsed();
        assert_eq!(s.tests.len(), 2000);
        assert!(took < std::time::Duration::from_millis(if cfg!(debug_assertions) { 50 } else { 5 }), "{took:?}");
    }
}
