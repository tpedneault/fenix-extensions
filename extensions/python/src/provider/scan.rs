//! The scanner: a Python file's text to the tests in it, a line at a time,
//! nothing run and nothing imported.
//!
//! What pytest collects, as far as the text says: functions the rules call
//! tests (`test*` by default) at the top of the module or in a class that
//! is one (`Test*`, or a `unittest.TestCase`), the classes nested in such
//! a class likewise; not a function or class inside a function. A
//! `@pytest.mark.parametrize` (or a fixture of the file's with `params`
//! that the test asks for) makes the test a template, its cases known only
//! once pytest lists them; `@pytest.mark.skip(reason=...)`, `@unittest.skip`
//! and `pytestmark = pytest.mark.skip(...)` skip it. A test is at the line
//! of its `def`, as pytest's own `item.location` has it.
//!
//! It never fails on half-typed text: strings, comments and brackets are
//! followed so a `def` inside a docstring isn't one and a decorator's
//! arguments over several lines are read whole; where a quote or a bracket
//! is never closed (a line being typed) that opener is read as plain text
//! and the file is read again, so the tests after it stay.

use super::layout::Rules;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The classes it's in and its own name, outermost first.
    pub path: Vec<String>,
    pub line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    /// The classes it's in, then its own name.
    pub path: Vec<String>,
    pub line: u32,
    pub end_line: u32,
    /// Skipped by a mark: why, or "".
    pub skip: Option<String>,
    /// Parametrized: its cases come from pytest.
    pub template: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scanned {
    /// The collectable classes, in file order.
    pub classes: Vec<Class>,
    pub tests: Vec<Test>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opener {
    Bracket,
    Quote,
}

/// Where a quote or a bracket opened: its line and byte column.
type At = (usize, usize);

enum Block {
    /// A class: whether pytest collects it, whether it's a `unittest`'s,
    /// its marks, and its entry in the classes' list when it's collected.
    Class { name: String, collected: bool, unittest: bool, skip: Option<String>, template: bool, entry: Option<usize> },
    /// A function: its entry in the tests' list when it's a test, and in
    /// the methods' when it's one of a class at the top of the module.
    Def { entry: Option<usize>, method: Option<usize> },
}

/// A test method of a class at the top of the module, collected or not: a
/// class that inherits it from one in the same file has it too.
struct Method {
    class: String,
    name: String,
    line: u32,
    end_line: u32,
    skip: Option<String>,
    template: bool,
    args: Vec<String>,
}

struct Open {
    indent: usize,
    block: Block,
}

#[derive(Default)]
struct Pass {
    out: Scanned,
    blocks: Vec<Open>,
    /// What each test's parameters are called, for the file's fixtures with
    /// params.
    args: Vec<Vec<String>>,
    param_fixtures: Vec<String>,
    /// `pytestmark = pytest.mark.skip(...)` at the top of the module.
    module_skip: Option<String>,
    methods: Vec<Method>,
    /// The classes at the top of the module: their bases, by name.
    bases: Vec<(String, Vec<String>)>,
    /// And what their own marks say, for the tests they inherit.
    class_marks: Vec<(String, Option<String>, bool)>,
    decorators: Vec<String>,
    /// The last line (from 1) with something on it.
    last_content: u32,
}

/// `text`'s first identifier (letters, digits, `_`; Python allows more than
/// ASCII).
fn ident(text: &str) -> &str {
    let end = text.char_indices().find(|(_, c)| !(c.is_alphanumeric() || *c == '_')).map(|(i, _)| i).unwrap_or(text.len());
    &text[..end]
}

/// The indentation of `line`: a space is 1, a tab to the next 8.
fn indent_of(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n = (n / 8 + 1) * 8,
            '\u{c}' => n = 0,
            _ => break,
        }
    }
    n
}

/// The first string literal in `text` at or after `from`: its content.
fn string_after(text: &str, from: usize) -> Option<String> {
    let rest = text.get(from..)?;
    let start = rest.find(['"', '\''])?;
    let quote = rest[start..].chars().next()?;
    let body = &rest[start + 1..];
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            c if c == quote => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// Whether a decorator (or `pytestmark` value) is a skip mark, and why:
/// `@pytest.mark.skip`, `@pytest.mark.skip(reason="slow")`,
/// `@unittest.skip("slow")`; not `skipif`, `skipIf`, `skipUnless`, whose
/// condition only the run knows, and not a skip inside another mark's
/// arguments (`parametrize(..., marks=pytest.mark.skip)` skips one case).
fn skip_of(text: &str) -> Option<String> {
    let text = text.trim().trim_start_matches(['@', '[', '(']).trim_start();
    let end = text.find(['(', ',', ']', ' ', '\n']).unwrap_or(text.len());
    let callee = &text[..end];
    if callee != "skip" && !callee.ends_with(".skip") {
        return None;
    }
    let args = text[end..].strip_prefix('(').unwrap_or("");
    let reason = match args.find("reason") {
        Some(r) => string_after(args, r + 6),
        None => string_after(args, 0).filter(|_| !args.trim_start().starts_with(')')),
    };
    Some(reason.unwrap_or_default())
}

/// The names of a `def`'s parameters, from its logical line.
fn params(text: &str) -> Vec<String> {
    let Some(open) = text.find('(') else { return Vec::new() };
    let mut depth = 0;
    let mut pieces: Vec<String> = vec![String::new()];
    for c in text[open + 1..].chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(p) = pieces.last_mut() {
            p.push(c);
        }
    }
    pieces.iter().map(|p| ident(p.trim().trim_start_matches('*')).to_string()).filter(|p| !p.is_empty()).collect()
}

impl Pass {
    /// A statement starts at `indent`: what it ends is closed.
    fn close(&mut self, indent: usize) {
        while self.blocks.last().is_some_and(|b| b.indent >= indent) {
            self.pop();
        }
    }

    fn pop(&mut self) {
        let Some(open) = self.blocks.pop() else { return };
        let end = self.last_content;
        match open.block {
            Block::Class { entry: Some(i), .. } => self.out.classes[i].end_line = end,
            Block::Def { entry, method } => {
                if let Some(i) = entry {
                    self.out.tests[i].end_line = end;
                }
                if let Some(i) = method {
                    self.methods[i].end_line = end;
                }
            }
            _ => {}
        }
    }

    /// What the decorators of the next `def` or `class` say.
    fn marks(&mut self) -> (Option<String>, bool, bool) {
        let decorators = std::mem::take(&mut self.decorators);
        let skip = decorators.iter().find_map(|d| skip_of(d));
        let template = decorators.iter().any(|d| d.contains("parametrize"));
        let fixture_with_params = decorators.iter().any(|d| d.contains("fixture") && d.contains("params"));
        (skip, template, fixture_with_params)
    }

    fn class(&mut self, indent: usize, line: u32, text: &str, rules: &Rules) {
        let rest = text.strip_prefix("class").unwrap_or(text).trim_start();
        let name = ident(rest).to_string();
        let bases = rest.get(name.len()..).unwrap_or("");
        let (skip, template, _) = self.marks();
        // Only within what's collected: a class in a function or in a class
        // pytest doesn't collect isn't.
        let in_scope = self.blocks.iter().all(|b| matches!(b.block, Block::Class { collected: true, .. }));
        let unittest = bases.split([',', '(', ')']).any(|b| b.trim().rsplit('.').next().is_some_and(|b| b.ends_with("TestCase")));
        let collected = in_scope && !name.is_empty() && (unittest || rules.class(&name));
        let entry = collected.then(|| {
            let mut path: Vec<String> = self.blocks.iter().filter_map(|b| if let Block::Class { name, .. } = &b.block { Some(name.clone()) } else { None }).collect();
            path.push(name.clone());
            self.out.classes.push(Class { path, line, end_line: line });
            self.out.classes.len() - 1
        });
        if self.blocks.is_empty() && !name.is_empty() {
            let bases = bases.trim_start().strip_prefix('(').and_then(|b| b.rsplit_once(')')).map(|(b, _)| b).unwrap_or("");
            let names = bases.split(',').map(|b| b.trim().rsplit('.').next().unwrap_or("").trim().to_string()).filter(|b| !b.is_empty() && b.chars().all(|c| c.is_alphanumeric() || c == '_')).collect();
            self.bases.push((name.clone(), names));
            self.class_marks.push((name.clone(), skip.clone(), template));
        }
        self.blocks.push(Open { indent, block: Block::Class { name, collected, unittest, skip, template, entry } });
    }

    fn def(&mut self, indent: usize, line: u32, text: &str, rules: &Rules) {
        let rest = text.strip_prefix("async").map(str::trim_start).unwrap_or(text);
        let rest = rest.strip_prefix("def").unwrap_or(rest).trim_start();
        let name = ident(rest).to_string();
        let (skip, template, fixture_with_params) = self.marks();
        if fixture_with_params && !name.is_empty() {
            self.param_fixtures.push(name.clone());
        }
        let in_scope = !name.is_empty() && self.blocks.iter().all(|b| matches!(b.block, Block::Class { collected: true, .. }));
        let unittest = matches!(self.blocks.last(), Some(Open { block: Block::Class { unittest: true, .. }, .. }));
        let looks_like_a_test = !name.is_empty() && if unittest { name.starts_with("test") } else { rules.function(&name) };
        let entry = (in_scope && looks_like_a_test).then(|| {
            let mut path: Vec<String> = self.blocks.iter().filter_map(|b| if let Block::Class { name, .. } = &b.block { Some(name.clone()) } else { None }).collect();
            path.push(name.clone());
            // The nearest class's marks are the test's own when it has none.
            let (class_skip, class_template) = self.blocks.iter().rev().find_map(|b| if let Block::Class { skip, template, .. } = &b.block { Some((skip.clone(), *template)) } else { None }).unwrap_or((None, false));
            self.out.tests.push(Test { path, line, end_line: line, skip: skip.clone().or(class_skip), template: template || class_template });
            self.args.push(params(rest));
            self.out.tests.len() - 1
        });
        // A method of a class at the top of the module, collected or not,
        // for the classes of the file that inherit it.
        let method = match self.blocks.as_slice() {
            [Open { block: Block::Class { name: class, .. }, .. }] if looks_like_a_test => {
                self.methods.push(Method { class: class.clone(), name: name.clone(), line, end_line: line, skip, template, args: params(rest) });
                Some(self.methods.len() - 1)
            }
            _ => None,
        };
        self.blocks.push(Open { indent, block: Block::Def { entry, method } });
    }

    /// The tests a collected class at the top of the module has from the
    /// classes it names that the file defines (not those it defines itself).
    fn inherit(&mut self) {
        let mut added: Vec<Test> = Vec::new();
        let mut args: Vec<Vec<String>> = Vec::new();
        for class in self.out.classes.iter().filter(|c| c.path.len() == 1) {
            let name = &class.path[0];
            let (class_skip, class_template) = self.class_marks.iter().find(|(n, _, _)| n == name).map(|(_, s, t)| (s.clone(), *t)).unwrap_or((None, false));
            let mut seen = vec![name.clone()];
            let mut queue: Vec<String> = self.bases.iter().find(|(n, _)| n == name).map(|(_, b)| b.clone()).unwrap_or_default();
            while let Some(base) = queue.pop() {
                if seen.contains(&base) || seen.len() > 16 {
                    continue;
                }
                seen.push(base.clone());
                queue.extend(self.bases.iter().find(|(n, _)| *n == base).map(|(_, b)| b.clone()).unwrap_or_default());
                for m in self.methods.iter().filter(|m| m.class == base) {
                    let defined = self.out.tests.iter().any(|t| t.path.len() == 2 && t.path[0] == *name && t.path[1] == m.name) || added.iter().any(|t| t.path[0] == *name && t.path[1] == m.name);
                    if !defined {
                        added.push(Test { path: vec![name.clone(), m.name.clone()], line: m.line, end_line: m.end_line, skip: m.skip.clone().or(class_skip.clone()), template: m.template || class_template });
                        args.push(m.args.clone());
                    }
                }
            }
        }
        self.out.tests.extend(added);
        self.args.extend(args);
    }

    /// A whole logical line, starting at `line` (from 1) at `indent`.
    fn statement(&mut self, indent: usize, line: u32, text: &str, rules: &Rules) {
        self.close(indent);
        let is_def = text.strip_prefix("async").map(str::trim_start).unwrap_or(text).strip_prefix("def").is_some_and(|r| r.starts_with([' ', '\t']));
        if text.starts_with('@') {
            self.decorators.push(text.to_string());
        } else if is_def {
            self.def(indent, line, text, rules);
        } else if text.strip_prefix("class").is_some_and(|r| r.starts_with([' ', '\t'])) {
            self.class(indent, line, text, rules);
        } else {
            self.decorators.clear();
            if indent == 0 && ident(text) == "pytestmark" {
                if let Some(skip) = text.split_once('=').and_then(|(_, v)| skip_of(v)) {
                    self.module_skip = Some(skip);
                }
            }
        }
    }
}

/// What's open at the end of `line`'s text, from `state`: updates the
/// strings and brackets, and says where the comment starts.
struct Walk {
    /// In a triple-quoted string: its quote and where it opened.
    triple: Option<(char, At)>,
    /// Open brackets and where each opened.
    brackets: Vec<At>,
    /// The line ended with a backslash.
    backslash: bool,
}

/// One pass over `lines`; `Err` is the quote or bracket still open at the
/// end, for the next pass to read as text.
fn pass(lines: &[&str], rules: &Rules, ignore: &[(Opener, At)]) -> Result<Scanned, (Opener, At)> {
    let mut p = Pass::default();
    let mut walk = Walk { triple: None, brackets: Vec::new(), backslash: false };
    let mut logical = String::new();
    let mut start = (0usize, 0u32);
    for (n, raw) in lines.iter().enumerate() {
        let number = n as u32 + 1;
        let line = if n == 0 { raw.strip_prefix('\u{feff}').unwrap_or(raw) } else { raw };
        let continuing = walk.triple.is_some() || !walk.brackets.is_empty() || walk.backslash;
        if !continuing {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            logical.clear();
            start = (indent_of(line), number);
        } else if walk.triple.is_none() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            // A line being typed: `def test_new(` with its bracket never
            // closed. A `def` or a `class` at or left of the statement's own
            // indent can't be inside a bracket, so the bracket is the typist's:
            // what was typed is a statement, and this line begins another.
            let starts = trimmed.starts_with("def ") || trimmed.starts_with("async def ") || trimmed.starts_with("class ");
            if !walk.brackets.is_empty() && starts && indent_of(line) <= start.0 {
                walk.brackets.clear();
                p.statement(start.0, start.1, &logical, rules);
                p.last_content = number - 1;
                logical.clear();
                start = (indent_of(line), number);
            }
        }
        walk.backslash = false;
        let bytes = line.as_bytes();
        let mut end = bytes.len();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if let Some((quote, _)) = walk.triple {
                if c == b'\\' {
                    i += 2;
                } else if c == quote as u8 && bytes.get(i + 1) == Some(&c) && bytes.get(i + 2) == Some(&c) {
                    walk.triple = None;
                    i += 3;
                } else {
                    i += 1;
                }
                continue;
            }
            match c {
                b'#' => {
                    end = i;
                    break;
                }
                b'"' | b'\'' => {
                    if bytes.get(i + 1) == Some(&c) && bytes.get(i + 2) == Some(&c) {
                        // Read as an empty string and a quote, if it's the typist's.
                        if !ignore.contains(&(Opener::Quote, (n, i))) {
                            walk.triple = Some((c as char, (n, i)));
                        }
                        i += 3;
                        continue;
                    }
                    // A string on one line: to its closing quote, or the line's end.
                    i += 1;
                    while i < bytes.len() && bytes[i] != c {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                b'(' | b'[' | b'{' => {
                    if !ignore.contains(&(Opener::Bracket, (n, i))) {
                        walk.brackets.push((n, i));
                    }
                }
                b')' | b']' | b'}' => {
                    walk.brackets.pop();
                }
                b'\\' if i + 1 >= bytes.len() => walk.backslash = true,
                _ => {}
            }
            i += 1;
        }
        let piece = line.get(..end).unwrap_or(line).trim();
        let whole = walk.triple.is_none() && walk.brackets.is_empty() && !walk.backslash;
        // A statement on one line, the common one, is read where it lies.
        if whole && logical.is_empty() {
            p.statement(start.0, start.1, piece, rules);
            p.last_content = number;
            continue;
        }
        if !logical.is_empty() {
            logical.push('\n');
        }
        logical.push_str(piece);
        if whole {
            p.statement(start.0, start.1, &logical, rules);
            p.last_content = number;
            logical.clear();
        }
    }
    if let Some((_, at)) = walk.triple {
        return Err((Opener::Quote, at));
    }
    if let Some(at) = walk.brackets.first() {
        return Err((Opener::Bracket, *at));
    }
    while !p.blocks.is_empty() {
        p.pop();
    }
    p.inherit();
    for (t, args) in p.out.tests.iter_mut().zip(&p.args) {
        if args.iter().any(|a| p.param_fixtures.contains(a)) {
            t.template = true;
        }
        if t.skip.is_none() {
            t.skip = p.module_skip.clone();
        }
    }
    Ok(p.out)
}

/// The tests in `text`. Never fails: a file being typed is read as well as
/// it can be.
pub fn scan(text: &str, rules: &Rules) -> Scanned {
    let lines: Vec<&str> = text.lines().collect();
    let mut ignore: Vec<(Opener, At)> = Vec::new();
    loop {
        match pass(&lines, rules, &ignore) {
            Ok(done) => return done,
            // One more opener read as text each time, so it ends: at most
            // as many passes as there are quotes and brackets, and a file of
            // thousands of lines with one typed quote takes two.
            Err(opener) if ignore.len() < 64 => ignore.push(opener),
            Err(_) => return Scanned::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = include_str!("../samples/tests/pytest/orbit/tests/test_basic.py");
    const EXTRA: &str = include_str!("../samples/tests/pytest/orbit/tests/test_extra.py");

    fn scanned(text: &str) -> Scanned {
        scan(text, &Rules::default())
    }

    fn names(s: &Scanned) -> Vec<String> {
        s.tests.iter().map(|t| t.path.join("::")).collect()
    }

    #[test]
    fn the_sample_is_found_at_the_defs_lines_with_its_marks() {
        let s = scanned(BASIC);
        assert_eq!(
            names(&s),
            ["test_adds", "test_fails_on_purpose", "test_skipped", "test_known_bug", "test_cases", "TestOuter::test_in_class", "TestOuter::TestInner::test_deep", "TestOuter::test_param_in_class", "test_uses_broken_fixture"]
        );
        let at = |n: &str| s.tests.iter().find(|t| t.path.join("::") == n).unwrap().clone();
        assert_eq!((at("test_adds").line, at("test_adds").end_line), (6, 7));
        assert_eq!(at("test_skipped").line, 16, "the def, not its decorator (pytest says the decorator's, 15)");
        assert_eq!(at("test_skipped").skip.as_deref(), Some("slow"));
        assert_eq!(at("test_known_bug").skip, None, "an xfail runs");
        assert!(at("test_cases").template && at("TestOuter::test_param_in_class").template && !at("test_adds").template);
        assert_eq!(at("test_cases").line, 26, "a decorator over one line, the def after it");
        let classes: Vec<(String, u32, u32)> = s.classes.iter().map(|c| (c.path.join("::"), c.line, c.end_line)).collect();
        assert_eq!(classes, [("TestOuter".to_string(), 30, 40), ("TestOuter::TestInner".to_string(), 34, 36)]);
        assert_eq!(at("TestOuter::TestInner::test_deep").end_line, 36);
    }

    #[test]
    fn the_extra_sample_has_marks_fixtures_with_params_and_a_name_in_japanese() {
        let s = scanned(EXTRA);
        let by = |n: &str| s.tests.iter().find(|t| t.path.join("::") == n).unwrap_or_else(|| panic!("{n} in {:?}", names(&s))).clone();
        assert_eq!(by("test_skipif").skip, None, "a condition only the run knows");
        assert_eq!(by("test_marked_case").skip, None, "one case skipped, not the test");
        assert!(by("test_名前").line > 0);
        assert!(by("test_stacked").template && by("test_marked_case").template && by("test_long_id").template);
        assert!(by("test_fixture_param").template, "a fixture of the file's with params");
        assert!(!by("test_teardown_fails").template, "a fixture without params");
        assert_eq!(by("test_stacked").line, 46, "two decorators, then the def");
    }

    #[test]
    fn a_class_decorator_marks_its_tests_and_a_unittest_class_is_one_by_its_base() {
        let s = scanned(
            "import unittest\nimport pytest\n\n@pytest.mark.skip(reason=\"later\")\nclass TestLater:\n    def test_a(self): pass\n\n@pytest.mark.parametrize(\"x\", [1])\nclass TestMany:\n    def test_b(self, x): pass\n\nclass AdderChecks(unittest.TestCase):\n    def test_c(self): pass\n    def check_d(self): pass\n\nclass Helper:\n    def test_not_collected(self): pass\n\nclass TestOuter:\n    class Helper:\n        def test_nested_not_collected(self): pass\n    class TestIn:\n        def test_nested(self): pass\n",
        );
        let by = |n: &str| s.tests.iter().find(|t| t.path.join("::") == n).unwrap_or_else(|| panic!("{n} in {:?}", names(&s))).clone();
        assert_eq!(by("TestLater::test_a").skip.as_deref(), Some("later"));
        assert!(by("TestMany::test_b").template);
        assert_eq!(names(&s), ["TestLater::test_a", "TestMany::test_b", "AdderChecks::test_c", "TestOuter::TestIn::test_nested"]);
    }

    #[test]
    fn a_class_has_the_tests_of_the_classes_of_the_file_it_inherits() {
        let s = scanned("class Base:\n    def test_shared(self): pass\n    def test_overridden(self): pass\n    def helper(self): pass\n\nclass Mixin(Base):\n    @pytest.mark.parametrize(\"x\", [1])\n    def test_mixed(self, x): pass\n\nclass TestA(Mixin):\n    def test_overridden(self): pass\n    def test_own(self): pass\n\nclass TestB(Base, object):\n    pass\n\nclass TestCycle(TestCycle2):\n    pass\nclass TestCycle2(TestCycle):\n    pass\n");
        assert_eq!(names(&s), ["TestA::test_overridden", "TestA::test_own", "TestA::test_mixed", "TestA::test_shared", "TestB::test_shared", "TestB::test_overridden"]);
        let by = |n: &str| s.tests.iter().find(|t| t.path.join("::") == n).unwrap().clone();
        assert_eq!((by("TestA::test_shared").line, by("TestA::test_shared").end_line), (2, 2), "where it's defined, as pytest says");
        assert!(by("TestA::test_mixed").template && !by("TestA::test_shared").template);
        assert_eq!(by("TestA::test_overridden").line, 11, "the class's own wins");
    }

    #[test]
    fn a_def_in_a_function_a_string_or_a_comment_is_not_a_test() {
        let s = scanned(
            "def test_outer():\n    def test_inner():\n        pass\n    class TestInner:\n        def test_x(self): pass\n\nDOC = \"\"\"\ndef test_in_a_string():\n    pass\n\"\"\"\n# def test_in_a_comment():\ns = 'def test_in_a_one_line_string(): pass'\ndef test_after():\n    pass\n",
        );
        assert_eq!(names(&s), ["test_outer", "test_after"]);
        assert_eq!(s.tests[0].end_line, 5, "the nested def and class are part of it");
    }

    #[test]
    fn a_module_mark_skips_every_test_and_a_conditional_def_is_collected() {
        let s = scanned("import pytest\nimport sys\npytestmark = pytest.mark.skip(reason=\"all later\")\n\nif sys.platform == \"linux\":\n    def test_linux():\n        pass\nelse:\n    def test_other():\n        pass\n");
        assert_eq!(names(&s), ["test_linux", "test_other"]);
        assert!(s.tests.iter().all(|t| t.skip.as_deref() == Some("all later")));
    }

    #[test]
    fn a_decorator_over_many_lines_is_read_whole() {
        let s = scanned("import pytest\n\n@pytest.mark.parametrize(\n    \"a,b\",\n    [(1, 2),\n  (3, 4)],  # a comment with a ) in it\n)\n@pytest.mark.skip(\n    reason=\"two\"\n    \" lines\",\n)\nasync def test_many(\n    a,\n    b,\n):\n    pass\n");
        assert_eq!(s.tests.len(), 1);
        let t = &s.tests[0];
        assert_eq!((t.line, t.end_line, t.template), (12, 16, true), "the def's line");
        assert_eq!(t.skip.as_deref(), Some("two"), "the first literal");
    }

    #[test]
    fn half_typed_text_keeps_the_tests_after_it() {
        let tail = "\n\ndef test_after():\n    pass\n\n\nclass TestAfter:\n    def test_in_class(self):\n        pass\n";
        for typing in [
            "def test_new(",
            "def test_new(a, b",
            "def test_new(a):\n    x = [1, 2",
            "@pytest.mark.parametrize(\"a\", [1, 2",
            "def test_new():\n    \"\"\"docs",
            "def test_new():\n    s = 'unclosed",
            "class TestNew:\n    def test_one(self):\n        assert foo(\n",
            "def test_new():\n    x = {",
        ] {
            let text = format!("def test_before():\n    pass\n\n{typing}{tail}");
            let s = scanned(&text);
            let got = names(&s);
            assert!(got.contains(&"test_before".to_string()) && got.contains(&"test_after".to_string()) && got.contains(&"TestAfter::test_in_class".to_string()), "{typing:?}: {got:?}");
        }
    }

    #[test]
    fn odd_text_never_panics() {
        for text in ["", "\n\n", "def", "def (", "class", "class :", "@", "@\ndef test_x(): pass", "\"\"\"", "'''\\", "(((((", ")))))", "def test_x(:\n", "\u{feff}def test_bom(): pass\n", "def test_\u{0}x(): pass\n", "\r\n\r\ndef test_crlf():\r\n    pass\r\n", "x = '\\", "\t\tdef test_tabs(): pass\n", "def test_é(): pass", "async", "async def", "pytestmark ="] {
            let _ = scanned(text);
        }
        assert_eq!(names(&scanned("\u{feff}def test_bom(): pass\n")), ["test_bom"]);
        assert_eq!(names(&scanned("\r\n\r\ndef test_crlf():\r\n    pass\r\n")), ["test_crlf"]);
        assert_eq!(names(&scanned("def test_\u{e9}(): pass")), ["test_\u{e9}"]);
    }

    #[test]
    fn a_test_defined_twice_is_listed_twice_for_the_provider_to_keep_the_later() {
        let s = scanned("def test_x():\n    pass\n\ndef test_x():\n    pass\n");
        assert_eq!(s.tests.iter().map(|t| t.line).collect::<Vec<_>>(), [1, 4]);
    }

    #[test]
    fn the_projects_rules_decide_what_a_test_is() {
        let rules = Rules { files: vec!["check_*.py".into()], classes: vec!["*Suite".into()], functions: vec!["check_".into()] };
        let s = scan("class AdderSuite:\n    def check_it(self): pass\n    def test_not(self): pass\nclass TestNot:\n    def check_no(self): pass\ndef check_top(): pass\n", &rules);
        assert_eq!(names(&s), ["AdderSuite::check_it", "check_top"]);
    }
}
