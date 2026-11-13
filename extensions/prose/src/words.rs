//! The project's word list: one word a line, as Harper reads and writes
//! its workspace dictionary (sorted, no comments), and finding the word
//! under the cursor.

use std::path::{Path, PathBuf};

/// A character a word may hold: letters, digits and the apostrophes of
/// "don't" and "Fenix's".
fn in_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}'
}

/// The word at column `col` (characters, from 0) of `line`, or the one
/// ending there when the cursor is just past it; none on a gap.
pub fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let on = |i: usize| chars.get(i).copied().is_some_and(in_word);
    let at = if on(col) {
        col
    } else if col > 0 && on(col - 1) {
        col - 1
    } else {
        return None;
    };
    let mut start = at;
    while start > 0 && on(start - 1) {
        start -= 1;
    }
    let mut end = at;
    while on(end + 1) {
        end += 1;
    }
    let word: String = chars[start..=end].iter().collect();
    let word = word.trim_matches(|c| c == '\'' || c == '\u{2019}');
    (!word.is_empty()).then(|| word.to_string())
}

/// A word taken from a selection: one word, as typed.
pub fn selected_word(text: &str) -> Result<String, String> {
    let word = text.trim();
    if word.is_empty() {
        return Err("nothing selected".into());
    }
    if word.chars().any(|c| !in_word(c)) {
        return Err(format!("\"{word}\" isn't one word: select a word, or put the cursor in it"));
    }
    Ok(word.to_string())
}

/// The list's words, one a line; blank lines left out.
pub fn words(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// The list with `word` in it, sorted as Harper sorts it; none when it's
/// there already.
pub fn with_word(text: &str, word: &str) -> Option<String> {
    let mut list = words(text);
    if list.iter().any(|w| w == word) {
        return None;
    }
    list.push(word.to_string());
    list.sort();
    list.dedup();
    Some(list.iter().map(|w| format!("{w}\n")).collect())
}

/// The list's file: `words` under `root`; refused outside the project,
/// where the pack may write nothing.
pub fn file(root: &str, words: &str) -> Result<PathBuf, String> {
    let rel = Path::new(words);
    if rel.is_absolute() || words.starts_with(['/', '\\']) || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("prose.words is {words}: a path under the project, such as .fenix/words.txt, is the one the pack writes"));
    }
    Ok(Path::new(root).join(rel))
}

/// `word` added to the list at `path`, which is made, and its folder, if
/// it isn't there. Whether it was new.
pub fn add(path: &Path, word: &str) -> Result<bool, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let Some(text) = with_word(&text, word) else { return Ok(false) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_under_the_cursor_is_found_whole() {
        let line = "Fenix's `wasm32-wasip2` extensions, don't.";
        assert_eq!(word_at(line, 0).as_deref(), Some("Fenix's"));
        assert_eq!(word_at(line, 6).as_deref(), Some("Fenix's"));
        assert_eq!(word_at(line, 7).as_deref(), Some("Fenix's"), "just past the word is still it");
        assert_eq!(word_at(line, 9).as_deref(), Some("wasm32"));
        assert_eq!(word_at(line, 17).as_deref(), Some("wasip2"));
        assert_eq!(word_at(line, 38).as_deref(), Some("don't"));
        assert_eq!(word_at("a  b", 2), None, "a gap");
        assert_eq!(word_at("", 0), None);
        assert_eq!(word_at("'' x", 1), None, "apostrophes alone are no word");
        assert_eq!(word_at("na\u{ef}ve caf\u{e9}", 8).as_deref(), Some("caf\u{e9}"));
    }

    #[test]
    fn a_selection_is_one_word() {
        assert_eq!(selected_word("  Harper\n").as_deref(), Ok("Harper"));
        assert!(selected_word("two words").is_err());
        assert!(selected_word("").is_err());
    }

    #[test]
    fn the_list_stays_sorted_one_word_a_line_and_has_each_once() {
        assert_eq!(with_word("", "wasm").as_deref(), Some("wasm\n"));
        assert_eq!(with_word("Zig\nbeta\n\nalpha\n", "Fenix").as_deref(), Some("Fenix\nZig\nalpha\nbeta\n"), "by character, as Harper sorts");
        assert_eq!(with_word("alpha\nbeta\n", "alpha"), None);
        assert_eq!(with_word("alpha", "beta").as_deref(), Some("alpha\nbeta\n"), "no newline at the end before");
        assert_eq!(words("a\r\n b \n\n"), ["a", "b"]);
    }

    #[test]
    fn the_list_is_a_file_under_the_project_only() {
        assert_eq!(file("/p", ".fenix/words.txt").unwrap(), Path::new("/p/.fenix/words.txt"));
        for bad in ["/etc/words", "../words.txt", "a/../../w", "\\words"] {
            assert!(file("/p", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_word_is_added_to_a_file_made_when_it_isnt_there() {
        let dir = std::env::temp_dir().join(format!("fenix-prose-words-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(".fenix/words.txt");
        assert_eq!(add(&path, "Fenix"), Ok(true));
        assert_eq!(add(&path, "wasm"), Ok(true));
        assert_eq!(add(&path, "Fenix"), Ok(false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Fenix\nwasm\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
