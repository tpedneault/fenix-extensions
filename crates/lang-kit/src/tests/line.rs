//! Fenix's limits a pack's answers are held to, and what keeps them
//! under: an id cut as Fenix cuts it, and a long selection split into
//! several commands under Windows' command line.

use std::borrow::Cow;

use fenix_extension_api::Command;

/// The longest id Fenix keeps, in bytes (Fenix §6.5).
pub const ID_MAX: usize = 1024;

/// The longest label, in characters (Fenix §6.5).
pub const LABEL_MAX: usize = 256;

/// The longest command line Fenix starts, as Windows quotes it, in
/// characters: `CreateProcessW`'s 32,767 with its null (Fenix §6.5). A
/// longer step doesn't start, on every platform.
pub const WINDOWS_MAX: usize = 32_766;

/// The longest when the program is a `.cmd` or `.bat` (an `npx`, a
/// `node_modules/.bin` shim), which Windows runs through `cmd.exe`
/// (Fenix §6.5).
pub const BATCH_FILE_MAX: usize = 8_191;

/// What [`DIRECT`] and [`BATCH_FILE`] leave below Fenix's limits: Fenix
/// fills in `${data}` and `${tools}` and finds the program's full path
/// before it starts a step, and its plan doesn't say whether its limit is
/// counted before or after.
pub const HEADROOM: usize = 1_024;

/// What [`split`] keeps a command line under for a program started
/// directly: [`WINDOWS_MAX`] less [`HEADROOM`].
pub const DIRECT: usize = WINDOWS_MAX - HEADROOM;

/// The same, for a program that's a `.cmd` or `.bat` on Windows.
pub const BATCH_FILE: usize = BATCH_FILE_MAX - HEADROOM;

/// `id` as Fenix keeps it: its first [`ID_MAX`] bytes, at a character's
/// boundary. Fenix cuts an id over 1 KiB the same way wherever it comes
/// (Fenix §4), so a key cut here matches the one Fenix has.
pub fn cut_id(id: &str) -> Cow<'_, str> {
    cut_bytes(id, ID_MAX)
}

/// `label` as Fenix shows it: its first [`LABEL_MAX`] characters.
pub fn cut_label(label: &str) -> Cow<'_, str> {
    match label.char_indices().nth(LABEL_MAX) {
        Some((at, _)) => Cow::Owned(label[..at].to_string()),
        None => Cow::Borrowed(label),
    }
}

pub(crate) fn cut_bytes(text: &str, most: usize) -> Cow<'_, str> {
    if text.len() <= most {
        return Cow::Borrowed(text);
    }
    let mut at = most;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    Cow::Owned(text[..at].to_string())
}

/// `cmd`'s command line as Windows quotes it, in UTF-16 units: the
/// program in quotes, then each argument quoted as `CommandLineToArgvW`
/// reads it (in quotes when it's empty or holds a space or a tab, a quote
/// as `\"`, backslashes before a quote doubled), joined by spaces -- as
/// Rust's `Command` writes it, which Fenix starts steps with. `env` and
/// `cwd` aren't on it.
pub fn command_line_len(cmd: &Command) -> usize {
    let mut n = 2 + utf16(&cmd.program);
    for arg in &cmd.args {
        n += 1 + quoted_len(arg);
    }
    n
}

fn utf16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn quoted_len(arg: &str) -> usize {
    let quote = arg.is_empty() || arg.contains([' ', '\t']);
    let mut n = 0;
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // Each backslash before it doubled, and the quote escaped.
                n += backslashes + 1;
            }
            backslashes = 0;
        }
        n += c.len_utf16();
    }
    if quote {
        // The closing quote's backslashes doubled, and the two quotes.
        n += backslashes + 2;
    }
    n
}

/// A selection too long for one command line, as several: `make` gives
/// the command for some of `items`, and each command returned has a
/// line ([`command_line_len`]) of at most `limit` -- [`DIRECT`] for a
/// program started directly, [`BATCH_FILE`] for a `.cmd` shim. The items
/// keep their order; an item too long on its own gets a command of its
/// own, which Fenix then refuses as too long. No items: one command,
/// `make(&[])`.
///
/// ```
/// use fenix_lang_kit::tests::{command_line_len, split, DIRECT};
/// use fenix_extension_api::Command;
///
/// let names: Vec<String> = (0..2000).map(|i| format!("geo::tests::case_number_{i:05}_of_many")).collect();
/// let steps = split(&names, DIRECT, |some| Command::new("cargo").args(["test", "--lib", "--", "--exact"]).args(some.iter().cloned()));
/// assert!(steps.len() > 1 && steps.iter().all(|c| command_line_len(c) <= DIRECT));
/// assert_eq!(steps.iter().map(|c| c.args.len() - 4).sum::<usize>(), 2000);
/// ```
pub fn split<T>(items: &[T], limit: usize, make: impl Fn(&[T]) -> Command) -> Vec<Command> {
    if items.is_empty() {
        return vec![make(&[])];
    }
    let base = command_line_len(&make(&[]));
    // What each item adds alone: measured once, so splitting 2,000 names
    // builds about 2,000 small commands, not 2,000 long ones.
    let mut out = Vec::new();
    let mut start = 0;
    let mut length = base;
    for i in 0..items.len() {
        let cost = command_line_len(&make(&items[i..=i])).saturating_sub(base);
        if i > start && length + cost > limit {
            settle(&items[start..i], limit, &make, &mut out);
            start = i;
            length = base;
        }
        length += cost;
    }
    settle(&items[start..], limit, &make, &mut out);
    out
}

/// `items`' command, halved until each is under `limit` (a `make` that
/// doesn't just append, a regex of names, can cost more than its parts).
fn settle<T>(items: &[T], limit: usize, make: &impl Fn(&[T]) -> Command, out: &mut Vec<Command>) {
    let cmd = make(items);
    if items.len() <= 1 || command_line_len(&cmd) <= limit {
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
    fn a_command_line_is_counted_as_windows_quotes_it() {
        assert_eq!(command_line_len(&Command::new("cargo")), 7, "\"cargo\"");
        let cmd = Command::new("go").args(["test", "", "a b", r#"say "hi""#, r"C:\dir\", r"C:\dir with space\", "ß😀"]);
        // "go" test "" "a b" "say \"hi\"" C:\dir\ "C:\dir with space\\" ß😀 (3 units)
        let expected = 4 + 5 + 3 + 6 + 13 + 8 + 22 + 4;
        assert_eq!(command_line_len(&cmd), expected);
    }

    #[test]
    fn an_id_is_cut_at_a_characters_boundary_as_fenix_cuts_it() {
        assert_eq!(cut_id("short"), "short");
        let long = format!("{}é{}", "a".repeat(1023), "b".repeat(10));
        let cut = cut_id(&long);
        assert_eq!(cut.len(), 1023, "é would end past 1,024 bytes");
        assert_eq!(cut_id(&cut), cut, "cut again, the same");
        assert_eq!(cut_label(&"x".repeat(300)).chars().count(), 256);
    }

    #[test]
    fn a_long_selection_is_split_under_the_limit_in_order() {
        let names: Vec<String> = (0..2000).map(|i| format!("tests::fails_number_{i:05}_on_purpose_xyz")).collect();
        let make = |some: &[String]| Command::new("cargo").args(["test", "--no-fail-fast", "--", "--exact"]).args(some.iter().cloned());
        let steps = split(&names, DIRECT, make);
        assert!(names.iter().map(|n| n.len() + 1).sum::<usize>() > 80_000);
        assert_eq!(steps.len(), 3);
        assert!(steps.iter().all(|c| command_line_len(c) <= DIRECT));
        let back: Vec<String> = steps.iter().flat_map(|c| c.args[4..].to_vec()).collect();
        assert_eq!(back, names);
        assert_eq!(split(&names, BATCH_FILE, make).len(), 12);

        // One regex of names: a make that doesn't append still fits.
        let regex = |some: &[String]| Command::new("go").args(["test", "-run", &format!("^({})$", some.join("|"))]);
        let steps = split(&names, DIRECT, regex);
        assert!(steps.iter().all(|c| command_line_len(c) <= DIRECT));
        assert_eq!(steps.iter().map(|c| c.args[2].matches('|').count() + 1).sum::<usize>(), 2000);

        // An item too long alone: a command of its own.
        let huge = vec!["a".repeat(40_000), "b".into()];
        let steps = split(&huge, DIRECT, |s: &[String]| Command::new("x").args(s.iter().cloned()));
        assert_eq!(steps.len(), 2);
        assert_eq!(split(&Vec::<String>::new(), DIRECT, |_| Command::new("x")).len(), 1);
    }
}
