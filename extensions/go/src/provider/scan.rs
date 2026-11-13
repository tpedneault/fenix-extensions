//! The scanner: what a Go test file's text says it tests, nothing else.
//! A line at a time with a small lexer behind it (comments, strings, raw
//! strings and runes, so a brace in one doesn't count), so a function's
//! end is where its braces close and a `func TestX` in a raw string isn't
//! a test.
//!
//! What `go test` runs of a `_test.go` file, at the top level of it:
//!
//! - `func TestXxx(t *testing.T)`: a name `Test` ends, or whose next
//!   letter isn't lower case (`TestAdds`, `Test_x`, `Test1`; `Testadds`
//!   isn't one). A body that calls `.Run(` with an argument is a
//!   *template*: its subtests exist only once it has run.
//! - `func FuzzXxx(f *testing.F)`: run as a test on its seeds, each a
//!   subtest `FuzzXxx/seed#0` (and a file of `testdata/fuzz`): a template.
//! - `func ExampleXxx()`: run only with an `// Output:` (or `//
//!   Unordered output:`) comment in its body; without, `go test` only
//!   builds it, so it isn't listed here either.
//!
//! A benchmark isn't run without `-bench`: left out. Methods, generic
//! functions and a signature that isn't the one `go test` wants are left
//! out; a file the build tags or its name's `_linux` exclude is scanned
//! all the same (Go says which files it builds; the scanner doesn't ask).

/// What a test is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Test,
    Fuzz,
    Example,
}

/// One test of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub kind: Kind,
    /// Its `func` line, from 1.
    pub line: u32,
    /// Where its braces close; none for half-typed text that never does.
    pub end_line: Option<u32>,
    /// Its subtests exist only once it has run.
    pub template: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Block,
    Raw,
}

/// `Test`, `Fuzz` or `Example` after `name`'s start `prefix`: the rest
/// can't start with a lower case letter.
fn named(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_lowercase()))
}

/// A top-level `func` line as a test: its name, what it is, whether
/// `go test` would take its signature.
fn header(line: &str) -> Option<(&str, Kind)> {
    let rest = line.strip_prefix("func ")?.trim_start();
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let (name, after) = rest.split_at(end);
    let kind = if named(name, "Test") {
        Kind::Test
    } else if named(name, "Fuzz") {
        Kind::Fuzz
    } else if named(name, "Example") {
        Kind::Example
    } else {
        return None;
    };
    // The parameters, up to their `)`; what follows may only open the body.
    let params = after.strip_prefix('(')?;
    let close = params.find(')')?;
    let (params, tail) = (params[..close].trim(), params[close + 1..].trim_start());
    if !(tail.is_empty() || tail.starts_with('{') || tail.starts_with("//")) {
        return None;
    }
    let wants = match kind {
        Kind::Example => params.is_empty(),
        Kind::Test | Kind::Fuzz => {
            let ty = params.split_whitespace().last().unwrap_or("");
            let ends = if kind == Kind::Test { ".T" } else { ".F" };
            params.split_whitespace().count() <= 2 && ty.starts_with('*') && ty.ends_with(ends)
        }
    };
    wants.then_some((name, kind))
}

/// `// Output: 4` and `// Unordered output:`, in any case.
fn is_output_comment(comment: &str) -> bool {
    let c = comment.trim_start().trim_start_matches('/').trim_start().to_ascii_lowercase();
    c.starts_with("output:") || c.starts_with("unordered output:")
}

/// The tests of `text`, in file order.
pub fn scan(text: &str) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut outputs: Vec<bool> = Vec::new();
    let mut mode = Mode::Code;
    let mut depth = 0u32;
    // The test whose body the lexer is in.
    let mut open: Option<usize> = None;
    for (i, line) in text.lines().enumerate() {
        let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
        if mode == Mode::Code && line.starts_with("func ") {
            // gofmt'd code never has a `func` in column 0 inside another: a
            // body half typed and never closed ends here.
            depth = 0;
            open = None;
            if let Some((name, kind)) = header(line) {
                items.push(Item { name: name.into(), kind, line: n, end_line: None, template: kind == Kind::Fuzz });
                outputs.push(false);
                open = Some(items.len() - 1);
            }
        }
        let b = line.as_bytes();
        let mut j = 0;
        while j < b.len() {
            let c = b[j];
            let next = b.get(j + 1).copied();
            match mode {
                Mode::Block => {
                    if c == b'*' && next == Some(b'/') {
                        mode = Mode::Code;
                        j += 1;
                    }
                }
                Mode::Raw => {
                    if c == b'`' {
                        mode = Mode::Code;
                    }
                }
                Mode::Code => match c {
                    b'/' if next == Some(b'/') => {
                        if let (Some(o), Some(comment)) = (open, line.get(j + 2..)) {
                            if is_output_comment(comment) {
                                outputs[o] = true;
                            }
                        }
                        break;
                    }
                    b'/' if next == Some(b'*') => {
                        mode = Mode::Block;
                        j += 1;
                    }
                    b'"' => {
                        j += 1;
                        while j < b.len() && b[j] != b'"' {
                            if b[j] == b'\\' {
                                j += 1;
                            }
                            j += 1;
                        }
                    }
                    b'`' => mode = Mode::Raw,
                    b'\'' => {
                        // A rune: `'{'`, `'\''`, `'é'`.
                        j += 1;
                        if b.get(j) == Some(&b'\\') {
                            j += 2;
                        }
                        let mut width = 0;
                        while j < b.len() && b[j] != b'\'' && width < 12 {
                            j += 1;
                            width += 1;
                        }
                    }
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            if let Some(o) = open.take() {
                                items[o].end_line = Some(n);
                            }
                        }
                    }
                    b'.' => {
                        if let (Some(o), Some(rest)) = (open, line.get(j + 1..)) {
                            if let Some(args) = rest.strip_prefix("Run(") {
                                if !args.starts_with(')') {
                                    items[o].template = true;
                                }
                            }
                        }
                    }
                    _ => {}
                },
            }
            j += 1;
        }
    }
    // An example with no output comment isn't run.
    let mut keep = outputs.into_iter();
    items.retain(|it| {
        let has_output = keep.next().unwrap_or(false);
        it.kind != Kind::Example || has_output
    });
    items
}

/// The expression of the file's `//go:build` line: the first of its header,
/// before the package clause.
pub fn constraint(text: &str) -> Option<&str> {
    let mut block = false;
    for line in text.lines() {
        let l = line.trim();
        if block {
            block = !l.contains("*/");
            continue;
        }
        if let Some(expr) = l.strip_prefix("//go:build ") {
            return Some(expr.trim());
        }
        if l.is_empty() || l.starts_with("//") {
            continue;
        }
        if l.starts_with("/*") {
            block = !l.contains("*/");
            continue;
        }
        return None;
    }
    None
}

/// The operating systems and architectures Go names, for a file's name and
/// its `//go:build`.
const GOOS: &[&str] = &["aix", "android", "darwin", "dragonfly", "freebsd", "hurd", "illumos", "ios", "js", "linux", "nacl", "netbsd", "openbsd", "plan9", "solaris", "wasip1", "windows", "zos"];
const GOARCH: &[&str] = &["386", "amd64", "amd64p32", "arm", "armbe", "arm64", "arm64be", "loong64", "mips", "mipsle", "mips64", "mips64le", "mips64p32", "mips64p32le", "ppc", "ppc64", "ppc64le", "riscv", "riscv64", "s390", "s390x", "sparc", "sparc64", "wasm"];

/// `darwin` for what `platform()` calls `macos`.
fn goos(os: &str) -> &str {
    if os == "macos" {
        "darwin"
    } else {
        os
    }
}

/// Whether build tag `tag` is set on `os`. What depends on the machine's
/// architecture or Go's own build (`amd64`, `cgo`, `go1.21`) is taken as
/// set: a file isn't called excluded unless it surely is.
fn tag_set(tag: &str, os: &str) -> bool {
    if GOOS.contains(&tag) {
        return tag == goos(os);
    }
    match tag {
        "unix" => !matches!(goos(os), "windows" | "plan9" | "js" | "wasip1"),
        "gc" | "cgo" => true,
        t => GOARCH.contains(&t) || t.strip_prefix("go1.").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())),
    }
}

/// `//go:build` expression `expr` on `os`: `linux && !cgo`, `(a || b) && c`.
/// An expression it can't read is true.
fn build_ok(expr: &str, os: &str) -> bool {
    struct P<'a> {
        t: Vec<&'a str>,
        i: usize,
        os: &'a str,
    }
    impl P<'_> {
        fn or(&mut self) -> bool {
            let mut v = self.and();
            while self.t.get(self.i) == Some(&"||") {
                self.i += 1;
                v |= self.and();
            }
            v
        }
        fn and(&mut self) -> bool {
            let mut v = self.not();
            while self.t.get(self.i) == Some(&"&&") {
                self.i += 1;
                v &= self.not();
            }
            v
        }
        fn not(&mut self) -> bool {
            match self.t.get(self.i).copied() {
                Some("!") => {
                    self.i += 1;
                    !self.not()
                }
                Some("(") => {
                    self.i += 1;
                    let v = self.or();
                    if self.t.get(self.i) == Some(&")") {
                        self.i += 1;
                    }
                    v
                }
                Some(tag) if tag != ")" && tag != "&&" && tag != "||" => {
                    self.i += 1;
                    tag_set(tag, self.os)
                }
                _ => true,
            }
        }
    }
    let b = expr.as_bytes();
    let mut tokens: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if matches!(c, b'(' | b')' | b'!') {
            tokens.push(&expr[i..i + 1]);
            i += 1;
        } else if matches!(c, b'&' | b'|') {
            if b.get(i + 1) == Some(&c) {
                tokens.push(&expr[i..i + 2]);
                i += 2;
            } else {
                i += 1;
            }
        } else {
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'(' | b')' | b'!' | b'&' | b'|') {
                i += 1;
            }
            tokens.push(&expr[start..i]);
        }
    }
    let mut p = P { t: tokens, i: 0, os };
    p.or()
}

/// Why `go test` doesn't build a test file on `os`, when it surely
/// doesn't: its `//go:build` line is false (`integration`, `windows`), or
/// its name ends in another system's (`x_windows_test.go`). `os` is
/// `platform()`'s (`linux`, `macos`, `windows`); empty, nothing is said.
pub fn excluded(base: &str, text: &str, os: &str) -> Option<String> {
    if os.is_empty() {
        return None;
    }
    let stem = base.strip_suffix("_test.go").unwrap_or(base);
    let parts: Vec<&str> = stem.split('_').collect();
    let n = parts.len();
    let named = if n >= 3 && GOARCH.contains(&parts[n - 1]) && GOOS.contains(&parts[n - 2]) {
        Some(parts[n - 2])
    } else if n >= 2 && GOOS.contains(&parts[n - 1]) {
        Some(parts[n - 1])
    } else {
        None
    };
    if let Some(named) = named.filter(|g| *g != goos(os)) {
        return Some(format!("not built here: its name ends in _{named}"));
    }
    let expr = constraint(text)?;
    (!build_ok(expr, os)).then(|| format!("not built here: //go:build {expr}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORBIT: &str = include_str!("../samples/tests/go/orbit/orbit_test.go");
    const EXTRA: &str = include_str!("../samples/tests/go/orbit/extra_test.go");

    fn names(items: &[Item]) -> Vec<(&str, u32, Option<u32>, bool)> {
        items.iter().map(|i| (i.name.as_str(), i.line, i.end_line, i.template)).collect()
    }

    #[test]
    fn tests_examples_and_fuzz_targets_are_found_at_their_lines() {
        assert_eq!(
            names(&scan(ORBIT)),
            [
                ("TestAdds", 8, Some(12), false),
                ("TestFailsOnPurpose", 14, Some(19), false),
                ("TestSkipped", 21, Some(23), false),
                ("TestTable", 25, Some(41), true),
                ("TestNested", 43, Some(47), true),
                ("ExampleAdd", 49, Some(52), false),
            ],
            "BenchmarkAdd is left out"
        );
        assert_eq!(
            names(&scan(EXTRA)),
            [
                ("TestExternal", 11, Some(15), false),
                ("TestParallel", 17, Some(28), true),
                ("TestFromHelper", 30, Some(32), false),
                ("TestPrints", 34, Some(38), false),
                ("TestSpecial", 40, Some(44), true),
                ("ExampleAdd_wrong", 46, Some(49), false),
                ("FuzzAdd", 55, Some(63), true),
            ],
            "ExampleAdd_silent has no Output comment: go test doesn't run it"
        );
    }

    #[test]
    fn go_names_a_test_by_what_follows_test() {
        let src = "func Test(t *testing.T) {}\nfunc Test_x(t *testing.T) {}\nfunc Test1(t *testing.T) {}\nfunc Testadds(t *testing.T) {}\nfunc TestÄ(t *testing.T) {}\nfunc Testé(t *testing.T) {}\nfunc Tester(t *testing.T) {}\n";
        assert_eq!(scan(src).iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Test", "Test_x", "Test1", "TestÄ"]);
    }

    #[test]
    fn the_signature_is_the_one_go_test_wants() {
        let src = "\
func TestOk(t *testing.T) {}
func TestUnnamed(*testing.T) {}
func TestBlank(_ *testing.T) {}
func TestAliased(t *tt.T) {}
func TestTwo(t *testing.T, x int) {}
func TestResult(t *testing.T) error { return nil }
func TestBench(b *testing.B) {}
func TestGeneric[T any](t *testing.T) {}
func (s *Suite) TestMethod(t *testing.T) {}
func TestNone() {}
func FuzzOk(f *testing.F) {}
func FuzzWrong(t *testing.T) {}
func ExampleOk() {
	// Output: yes
}
func ExampleParam(x int) {
	// Output: yes
}
func ExampleResult() int {
	// Output: yes
}
func BenchmarkNo(b *testing.B) {}
func TestMain(m *testing.M) {}
";
        let items = scan(src);
        let got: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(got, ["TestOk", "TestUnnamed", "TestBlank", "TestAliased", "FuzzOk", "ExampleOk"]);
    }

    #[test]
    fn an_example_runs_only_with_an_output_comment() {
        let src = "func ExampleA() {\n\tfmt.Println(1)\n\t// Output: 1\n}\nfunc ExampleB() {\n\t// Unordered output:\n\t// 1\n}\nfunc ExampleC() {\n\t// OUTPUT: x\n}\nfunc ExampleD() {\n\t// not an output: comment\n\tfmt.Println(\"// Output: in a string\")\n}\n";
        assert_eq!(scan(src).iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["ExampleA", "ExampleB", "ExampleC"]);
    }

    #[test]
    fn a_template_is_a_test_that_runs_subtests() {
        let src = "\
func TestA(t *testing.T) {
	t.Run(\"x\", func(t *testing.T) {})
}
func TestB(t *testing.T) {
	cmd.Run()
	// t.Run(\"in a comment\", f)
	s := \"t.Run(\\\"in a string\\\", f)\"
}
func TestC(t *testing.T) {
	suite.Run(t, new(S))
}
func TestD(t *testing.T) {
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Run(\"deep\", nil)
		})
	}
}
";
        let found = scan(src);
        assert_eq!(names(&found), [("TestA", 1, Some(3), true), ("TestB", 4, Some(8), false), ("TestC", 9, Some(11), true), ("TestD", 12, Some(18), true)]);
    }

    #[test]
    fn braces_in_strings_runes_and_comments_dont_count() {
        let src = "\
func TestA(t *testing.T) {
	s := \"}\"
	r := '}'
	q := '\\''
	x := `
}
func TestNotThis(t *testing.T) {}
`
	/* } */ // }
	_ = s + string(r) + string(q) + x
}
func TestB(t *testing.T) {}
";
        assert_eq!(names(&scan(src)), [("TestA", 1, Some(11), false), ("TestB", 12, Some(12), false)], "a func in a raw string isn't a test");
    }

    #[test]
    fn half_typed_text_loses_only_the_test_it_is_in() {
        let src = "func TestA(t *testing.T) {\n\tif x {\n\nfunc TestB(t *testing.T) {\n}\nfunc TestC(t *testing.T) {\n\tfoo(\n}\n";
        assert_eq!(names(&scan(src)), [("TestA", 1, None, false), ("TestB", 4, Some(5), false), ("TestC", 6, Some(8), false)], "a func in column 0 closes what's open");
        // A raw string never closed, a block comment, odd bytes: no panic.
        for src in ["func TestA(t *testing.T) {\n\tx := `\n", "/* func TestA(t *testing.T) {}\n", "func TestA(t *testing.T) {\n\tr := '\n}\n", "func TestÄ(t *testing.T) { s := \"é\\\n", "\u{feff}func TestA(t *testing.T) {}", "func ", "func T", "func Test(", "func Test(t *testing.T", "{{{{", "}}}}\nfunc TestA(t *testing.T) {}"] {
            let _ = scan(src);
        }
        assert_eq!(scan("}}}}\nfunc TestA(t *testing.T) {}").len(), 1);
        assert!(scan("").is_empty());
        assert_eq!(scan("func TestA(t *testing.T) {}\r\nfunc TestB(t *testing.T) {\r\n}\r\n").len(), 2, "CRLF");
    }

    #[test]
    fn nothing_but_the_top_level_counts() {
        let src = "func TestA(t *testing.T) {\n\tfunc TestInner(t *testing.T) {}\n}\n\tfunc TestIndented(t *testing.T) {}\nvar x = func TestNo(t *testing.T) {}\n";
        assert_eq!(scan(src).iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["TestA"]);
    }

    #[test]
    fn a_build_constraint_that_is_surely_false_says_so() {
        let on = |expr: &str, os: &str| build_ok(expr, os);
        assert!(!on("integration", "linux") && on("linux", "linux") && !on("windows", "linux") && on("windows", "windows"));
        assert!(on("linux || darwin", "macos") && !on("linux && darwin", "linux") && on("!windows", "linux") && !on("!linux", "linux"));
        assert!(on("(linux || windows) && !integration", "windows") && !on("(linux || windows) && integration", "windows"));
        assert!(on("unix", "macos") && !on("unix", "windows") && on("amd64", "linux") && on("cgo && go1.21", "linux") && !on("ignore", "linux") && !on("race", "linux"));
        assert!(on("", "linux") && on("(", "linux"), "what it can't read is true");
        let tagged = "//go:build integration\n\npackage tagged\n";
        assert_eq!(excluded("tagged_test.go", tagged, "linux").as_deref(), Some("not built here: //go:build integration"));
        assert_eq!(excluded("tagged_test.go", tagged, ""), None, "no system, no claim");
        assert_eq!(excluded("x_test.go", "// Copyright\n\n//go:build linux\n\npackage x\n", "linux"), None);
        assert_eq!(excluded("x_test.go", "package x\n//go:build windows\n", "linux"), None, "after the package clause it's no constraint");
        assert_eq!(excluded("x_test.go", "/* a\n//go:build windows\n*/\npackage x\n", "linux"), None, "in a comment");
        assert_eq!(excluded("x_windows_test.go", "package x\n", "linux").as_deref(), Some("not built here: its name ends in _windows"));
        assert_eq!(excluded("x_windows_amd64_test.go", "package x\n", "linux").as_deref(), Some("not built here: its name ends in _windows"));
        assert_eq!(excluded("x_linux_test.go", "package x\n", "linux"), None);
        assert_eq!(excluded("windows_test.go", "package x\n", "linux"), None, "a name of its own, no suffix");
        assert_eq!(excluded("x_amd64_test.go", "package x\n", "linux"), None);
    }
}
