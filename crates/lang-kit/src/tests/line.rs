//! Fenix's limits a pack's answers are held to, and what keeps them
//! under: an id cut as Fenix cuts it, and a long selection split into
//! several commands under Windows' command line, counted as Fenix counts
//! it (Fenix's plan, §6.5 and §19).

use std::borrow::Cow;

use fenix_extension_api::Command;

/// The longest id Fenix keeps as it is, in bytes (Fenix §6.5).
pub const ID_MAX: usize = 1024;

/// How much of a longer id is kept before its hash: 1,024 less the `#`
/// and 16 hex digits (Fenix §19, finding 9).
pub const ID_PREFIX_MAX: usize = ID_MAX - 1 - 16;

/// The longest label, in characters (Fenix §6.5).
pub const LABEL_MAX: usize = 256;

/// The longest command line Fenix starts, in UTF-16 units:
/// `CreateProcessW`'s 32,767 with its null (Fenix §6.5). A longer step
/// doesn't start, on every platform.
pub const WINDOWS_MAX: usize = 32_766;

/// The longest when the program is a `.cmd` or `.bat` (an `npx`, a
/// `node_modules/.bin` shim), which Windows runs through `cmd.exe`
/// (Fenix §6.5).
pub const BATCH_FILE_MAX: usize = 8_191;

/// What [`Limit::len`] counts `${data}`, `${tools}` and the program's
/// full path as, at least: the classic `MAX_PATH`. Fenix fills the first
/// two in and resolves the program before it counts (Fenix §19, K2), and
/// a pack knows none of them, so it counts each generously: that's its
/// margin.
pub const PATH_ALLOWANCE: usize = 260;

/// The limit a step's command line is held to, and how it's counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// The most, in UTF-16 units.
    pub max: usize,
    /// The program is a `.cmd` or `.bat` on Windows: counted as the line
    /// Rust gives `cmd.exe` ([`batch_file_line_len`]).
    pub batch_file: bool,
}

/// A program started directly: [`WINDOWS_MAX`], counted as
/// [`command_line_len`] counts.
pub const DIRECT: Limit = Limit { max: WINDOWS_MAX, batch_file: false };

/// A program that's a `.cmd` or `.bat` on Windows: [`BATCH_FILE_MAX`],
/// counted as [`batch_file_line_len`] counts.
pub const BATCH_FILE: Limit = Limit { max: BATCH_FILE_MAX, batch_file: true };

impl Limit {
    /// `cmd`'s line as Fenix would count it, generously: each `${data}`
    /// and `${tools}` in it counted as [`PATH_ALLOWANCE`] characters
    /// holding a space (so an argument holding one is quoted, as one under
    /// `C:\Users\Ann Lee` would be), and its program as a full path of at
    /// least as many. `usize::MAX` when Rust wouldn't start it (a batch
    /// file's argument holding a line break).
    pub fn len(&self, cmd: &Command) -> usize {
        let (program, k) = generous(&cmd.program);
        let units = (utf16(&program) + k * (PATH_ALLOWANCE - 1)).max(PATH_ALLOWANCE);
        let args = cmd.args.iter().map(|a| generous(a));
        if self.batch_file {
            batch_line_len(&program, units, args).unwrap_or(usize::MAX)
        } else {
            line_len(units, args)
        }
    }

    /// Whether Fenix starts `cmd`, counted generously ([`Limit::len`]).
    pub fn fits(&self, cmd: &Command) -> bool {
        self.len(cmd) <= self.max
    }
}

/// `s` with each `${data}` and `${tools}` as one `x`, and how many there
/// were: each is then counted as [`PATH_ALLOWANCE`] characters, and the
/// argument as quoted (the stand-in holds a space). As it is when it has
/// neither.
fn generous(s: &str) -> (Cow<'_, str>, usize) {
    if !s.contains("${") {
        return (Cow::Borrowed(s), 0);
    }
    let k = s.matches("${data}").count() + s.matches("${tools}").count();
    if k == 0 {
        return (Cow::Borrowed(s), 0);
    }
    (Cow::Owned(s.replace("${data}", "x").replace("${tools}", "x")), k)
}

/// FNV-1a, 64 bits, of `bytes`: the hash a cut id ends with (Fenix §19,
/// finding 9; `fenix_tests::fnv1a_64`).
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `id` as Fenix keys it (Fenix §4, §19 finding 9, Decisions 33): one of
/// at most [`ID_MAX`] bytes as it is; a longer one as its first bytes up
/// to [`ID_PREFIX_MAX`], back to a character's start, then `#`, then the
/// FNV-1a 64-bit hash of the whole id's UTF-8 as 16 lowercase hex digits:
/// 1,021 to 1,024 bytes. Byte for byte `fenix_tests::cut_id`, so a key
/// cut here matches the one Fenix has, and two that share their first KiB
/// stay two. Cutting a cut id changes nothing.
pub fn cut_id(id: &str) -> Cow<'_, str> {
    if id.len() <= ID_MAX {
        return Cow::Borrowed(id);
    }
    Cow::Owned(format!("{}#{:016x}", cut_bytes(id, ID_PREFIX_MAX), fnv1a_64(id.as_bytes())))
}

/// Whether `id` has the form [`cut_id`] gives a long one: Fenix gives
/// such a node back only with its `data` (Fenix §19, finding 2).
pub fn is_cut_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() > ID_PREFIX_MAX + 13 && b.len() <= ID_MAX && b[b.len() - 17] == b'#' && b[b.len() - 16..].iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// `label` as Fenix shows it: its first [`LABEL_MAX`] characters.
pub fn cut_label(label: &str) -> Cow<'_, str> {
    match label.char_indices().nth(LABEL_MAX) {
        Some((at, _)) => Cow::Owned(label[..at].to_string()),
        None => Cow::Borrowed(label),
    }
}

pub(crate) fn cut_bytes(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut at = most;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

/// `cmd`'s command line as Rust's standard library builds it for
/// `CreateProcessW`, in UTF-16 units (Fenix §19, K2): the program always
/// in `"`, then each argument, in `"` when it's empty or holds a space or
/// a tab, a `"` in it as `\"` with the `\`s right before it doubled, and
/// the `\`s ending a quoted one doubled; joined by one space. Counted as
/// `cmd` is: Fenix counts it after filling in `${data}` and `${tools}`
/// and with the program's full path ([`Limit::len`] allows for those).
/// `env` and `cwd` aren't on it.
pub fn command_line_len(cmd: &Command) -> usize {
    line_len(utf16(&cmd.program), cmd.args.iter().map(|a| (Cow::Borrowed(a.as_str()), 0)))
}

/// The line of a program of `program` units and `args`, each with how
/// many placeholders it stands for (each [`PATH_ALLOWANCE`] units, one
/// already counted, and the argument quoted).
fn line_len<'a>(program: usize, args: impl Iterator<Item = (Cow<'a, str>, usize)>) -> usize {
    let mut n = 2 + program;
    for (arg, k) in args {
        n += 1 + quoted_len(&arg, k > 0) + k * (PATH_ALLOWANCE - 1);
    }
    n
}

/// UTF-16 units of `s`: a unit a character, two for one of four bytes.
/// Over bytes, so a debug build counts a long selection quickly too.
fn utf16(s: &str) -> usize {
    let mut n = 0;
    for &b in s.as_bytes() {
        n += usize::from(b & 0xC0 != 0x80) + usize::from(b >= 0xF0);
    }
    n
}

fn quoted_len(arg: &str, force: bool) -> usize {
    let bytes = arg.as_bytes();
    let quote = force || bytes.is_empty() || bytes.iter().any(|&b| b == b' ' || b == b'\t');
    let mut n = utf16(arg);
    let mut backslashes = 0;
    for &b in bytes {
        if b == b'\\' {
            backslashes += 1;
        } else {
            if b == b'"' {
                // Each backslash before it doubled, and the quote escaped.
                n += backslashes + 1;
            }
            backslashes = 0;
        }
    }
    if quote {
        // The closing quote's backslashes doubled, and the two quotes.
        n += backslashes + 2;
    }
    n
}

/// `cmd`'s command line as Rust's standard library gives a `.cmd` or
/// `.bat` to `cmd.exe` (Fenix §19, K2), in UTF-16 units: `cmd.exe /e:ON
/// /v:OFF /d /c "`, the script in `"`, each argument after a space --
/// in `"` when it's empty, ends with `\`, or holds an ASCII character
/// other than a letter, a digit or one of `#$*+-./:?@\_`, or a control
/// character; a `"` doubled with the `\`s before it doubled; each `%` as
/// `%%cd:~,%` -- and a closing `"`. `None` when Rust refuses it: an
/// argument holding `\r` or `\n`, a script holding `"` or ending in `\`.
pub fn batch_file_line_len(cmd: &Command) -> Option<usize> {
    batch_line_len(&cmd.program, utf16(&cmd.program), cmd.args.iter().map(|a| (Cow::Borrowed(a.as_str()), 0)))
}

fn batch_line_len<'a>(script: &str, script_units: usize, args: impl Iterator<Item = (Cow<'a, str>, usize)>) -> Option<usize> {
    if script.contains('"') || script.ends_with('\\') {
        return None;
    }
    let mut n = utf16("cmd.exe /e:ON /v:OFF /d /c \"") + 2 + script_units;
    for (arg, k) in args {
        if arg.contains(['\r', '\n']) {
            return None;
        }
        n += 1 + k * (PATH_ALLOWANCE - 1);
        const UNQUOTED: &str = r"#$*+-./:?@\_";
        let quote = k > 0 || arg.is_empty() || arg.ends_with('\\') || arg.chars().any(|c| (c.is_ascii() && !(c.is_ascii_alphanumeric() || UNQUOTED.contains(c))) || c.is_control());
        let mut backslashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
            } else {
                if c == '"' {
                    n += backslashes + 1;
                } else if c == '%' {
                    n += "%%cd:~,".len();
                }
                backslashes = 0;
            }
            n += c.len_utf16();
        }
        if quote {
            n += backslashes + 2;
        }
    }
    Some(n + 1)
}

/// A selection too long for one command line, as several: `make` gives
/// the command for some of `items`, and each command returned fits
/// `limit` ([`Limit::fits`]: [`DIRECT`] for a program started directly,
/// [`BATCH_FILE`] for a `.cmd` shim), counted with `${data}`, `${tools}`
/// and the program's path at [`PATH_ALLOWANCE`] each. The items keep
/// their order; an item too long on its own gets a command of its own,
/// which Fenix then refuses as too long. No items: one command,
/// `make(&[])`.
///
/// ```
/// use fenix_lang_kit::tests::{split, DIRECT};
/// use fenix_extension_api::Command;
///
/// let names: Vec<String> = (0..2000).map(|i| format!("geo::tests::case_number_{i:05}_of_many")).collect();
/// let steps = split(&names, DIRECT, |some| Command::new("cargo").args(["test", "--lib", "--", "--exact"]).args(some.iter().cloned()));
/// assert!(steps.len() > 1 && steps.iter().all(|c| DIRECT.fits(c)));
/// assert_eq!(steps.iter().map(|c| c.args.len() - 4).sum::<usize>(), 2000);
/// ```
pub fn split<T>(items: &[T], limit: Limit, make: impl Fn(&[T]) -> Command) -> Vec<Command> {
    if items.is_empty() {
        return vec![make(&[])];
    }
    let base = limit.len(&make(&[]));
    // What each item adds alone: measured once, so splitting 2,000 names
    // builds about 2,000 small commands, not 2,000 long ones.
    let mut out = Vec::new();
    let mut start = 0;
    let mut length = base;
    for i in 0..items.len() {
        let cost = limit.len(&make(&items[i..=i])).saturating_sub(base);
        if i > start && length.saturating_add(cost) > limit.max {
            settle(&items[start..i], limit, &make, &mut out);
            start = i;
            length = base;
        }
        length = length.saturating_add(cost);
    }
    settle(&items[start..], limit, &make, &mut out);
    out
}

/// `items`' command, halved until each fits (a `make` that doesn't just
/// append, a regex of names, can cost more than its parts).
fn settle<T>(items: &[T], limit: Limit, make: &impl Fn(&[T]) -> Command, out: &mut Vec<Command>) {
    let cmd = make(items);
    if items.len() <= 1 || limit.fits(&cmd) {
        out.push(cmd);
        return;
    }
    let half = items.len() / 2;
    settle(&items[..half], limit, make, out);
    settle(&items[half..], limit, make, out);
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn a_command_line_is_counted_as_rust_quotes_it_for_create_process() {
        assert_eq!(command_line_len(&Command::new("cargo")), 7, "\"cargo\"");
        let cmd = Command::new("go").args(["test", "", "a b", r#"say "hi""#, r"C:\dir\", r"C:\dir with space\", "ß😀"]);
        // "go" test "" "a b" "say \"hi\"" C:\dir\ "C:\dir with space\\" ß😀 (3 units)
        let expected = 4 + 5 + 3 + 6 + 13 + 8 + 22 + 4;
        assert_eq!(command_line_len(&cmd), expected);
    }

    #[test]
    fn a_batch_files_line_is_counted_as_rust_gives_it_to_cmd_exe() {
        // cmd.exe /e:ON /v:OFF /d /c ""C:\n\npx.cmd" vitest "a b" "" "x\\" %%cd:~,%P%%cd:~,%"
        let cmd = Command::new(r"C:\n\npx.cmd").args(["vitest", "a b", "", r"x\", "%P%"]);
        let line = r#"cmd.exe /e:ON /v:OFF /d /c ""C:\n\npx.cmd" vitest "a b" "" "x\\" "%%cd:~,%P%%cd:~,%"""#;
        assert_eq!(batch_file_line_len(&cmd), Some(line.len()));
        assert_eq!(batch_file_line_len(&Command::new("x.cmd").arg("a\nb")), None, "Rust refuses a line break");
        assert_eq!(batch_file_line_len(&Command::new(r"C:\x\")), None);
    }

    /// Fenix's test vectors for the id cut (`fenix-tests`' and
    /// `fenix-wasm`'s tests, `WRITING_EXTENSIONS.md`): every one, with
    /// its length.
    #[test]
    fn a_long_id_is_cut_to_a_prefix_and_a_hash_as_fenix_cuts_it() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        let shared = format!("tests/test_x.py::test_cases[{}", "a".repeat(1_100));
        let vectors: [(String, usize, &str); 7] = [
            ("a".repeat(1_023), 1_023, ""),
            ("a".repeat(1_024), 1_024, ""),
            ("a".repeat(1_025), 1_024, "#6a424025ce84288c"),
            ("é".repeat(600), 1_023, "#0e3ffe7c6421a045"),
            (format!("a{}", "😀".repeat(300)), 1_022, "#2056db1222d21dec"),
            (format!("{shared}-one]"), 1_024, "#589a673077e12368"),
            (format!("{shared}-two]"), 1_024, "#a9074c8a277a3a36"),
        ];
        for (id, len, end) in &vectors {
            let cut = cut_id(id);
            assert_eq!(cut.len(), *len, "{id:.40}");
            if end.is_empty() {
                assert_eq!(cut, id.as_str(), "kept as it is");
                assert!(!is_cut_id(&cut));
            } else {
                assert_eq!(&cut[cut.len() - 17..], *end, "{id:.40}");
                assert_eq!(&cut[..cut.len() - 17], &id[..cut.len() - 17], "its start");
                assert!(is_cut_id(&cut));
                assert_eq!(cut_id(&cut), cut, "cutting it again changes nothing");
            }
        }
        assert_ne!(cut_id(&vectors[5].0), cut_id(&vectors[6].0), "two that share their first 1,100 bytes stay two");
        assert_eq!(cut_label(&"x".repeat(300)).chars().count(), 256);
    }

    #[test]
    fn a_long_selection_is_split_under_the_limit_in_order() {
        let names: Vec<String> = (0..2000).map(|i| format!("tests::fails_number_{i:05}_on_purpose_xyz")).collect();
        let make = |some: &[String]| Command::new("cargo").args(["test", "--no-fail-fast", "--", "--exact"]).args(some.iter().cloned());
        let steps = split(&names, DIRECT, make);
        assert!(names.iter().map(|n| n.len() + 1).sum::<usize>() > 80_000);
        assert_eq!(steps.len(), 3);
        assert!(steps.iter().all(|c| DIRECT.fits(c) && command_line_len(c) + (PATH_ALLOWANCE - "cargo".len()) <= WINDOWS_MAX), "the program counted as a full path");
        let back: Vec<String> = steps.iter().flat_map(|c| c.args[4..].to_vec()).collect();
        assert_eq!(back, names);
        assert_eq!(split(&names, BATCH_FILE, make).len(), 11);

        // `${data}` counted at its most: a reporter's path in every step.
        let with_data = |some: &[String]| Command::new("pytest").args(["-p", "${data}/lang-kit/tests/fenix_tests.py"]).args(some.iter().cloned());
        let steps = split(&names, DIRECT, with_data);
        assert!(steps.iter().all(|c| DIRECT.len(c) <= WINDOWS_MAX && DIRECT.len(c) > command_line_len(c) + 2 * 200));

        // One regex of names: a make that doesn't append still fits.
        let regex = |some: &[String]| Command::new("go").args(["test", "-run", &format!("^({})$", some.join("|"))]);
        let steps = split(&names, DIRECT, regex);
        assert!(steps.iter().all(|c| DIRECT.fits(c)));
        assert_eq!(steps.iter().map(|c| c.args[2].matches('|').count() + 1).sum::<usize>(), 2000);

        // An item too long alone: a command of its own.
        let huge = vec!["a".repeat(40_000), "b".into()];
        let steps = split(&huge, DIRECT, |s: &[String]| Command::new("x").args(s.iter().cloned()));
        assert_eq!(steps.len(), 2);
        assert_eq!(split(&Vec::<String>::new(), DIRECT, |_| Command::new("x")).len(), 1);
    }
}
