//! Which item a place in a buffer is about, for the commands that start
//! from one (`kiln.show`, `kiln.impact`): the ID under the cursor, else
//! the item whose file it is.
//!
//! An ID is recognised by the loaded project, not by a pattern: a word
//! under the cursor is an ID when `kiln list` has an item by it. That
//! takes any prefix a project configures, and doesn't mistake `UTF-8` or
//! `SHA-256` for one. A word that runs on past the ID (a file name,
//! `TASK-002-write-readings-to-sqlite.md`) is split on its `-`s and the
//! parts tried, the ones under the cursor first.
//!
//! Paths. Fenix gives `here()`'s path in full (`editor = "read"`), and
//! its project's folder in full too; kiln's paths are from the project's
//! top. The file is the project's when its path starts with that folder
//! (or with `/project`, the folder as the extension's own reads name it);
//! then the rest is compared with each item's path. When neither prefix
//! matches -- a Kiln project below the folder Fenix opened -- a path that
//! ends with `/` and an item's whole path is taken as that item's file,
//! which an item's path, `design/tasks/TASK-002-….md`, makes unlikely to
//! be wrong. Backslashes are read as `/`, for Windows. Case is compared
//! as it is: a Windows path typed in another case is not matched (said
//! as "isn't an item's file", not guessed at).
//!
//! What it deliberately doesn't do: call the host (`lib.rs` hands it what
//! `here()` and `cursor()` said, so it's tested natively), read a file,
//! or guess at an ID kiln doesn't list.

use std::collections::HashMap;

use crate::project::Loaded;

/// Where the cursor was when the command ran, as `here()` and `cursor()`
/// said it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spot {
    /// The file, as a full path; none in a buffer that isn't one.
    pub path: Option<String>,
    /// The project's folder, as a full path.
    pub root: Option<String>,
    /// `text`'s first line, from 1.
    pub line: u32,
    /// The cursor's line, or what's selected.
    pub text: String,
    /// The cursor's column in `text`, in characters from 0; none when it
    /// isn't known.
    pub column: Option<u32>,
    /// `text` is a selection, not the cursor's line.
    pub selected: bool,
}

/// The project folder as the extension's own reads see it.
const PROJECT: &str = "/project";

/// `path` with `\` read as `/`.
fn slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// `path` from the project's top, when it's in `root` (or `/project`).
pub fn relative(path: &str, root: Option<&str>) -> Option<String> {
    let path = slashes(path);
    let roots = root.map(slashes).into_iter().chain([PROJECT.to_string()]);
    for root in roots {
        let root = root.trim_end_matches('/');
        if root.is_empty() {
            continue;
        }
        if let Some(rest) = path.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
            return Some(rest.to_string());
        }
    }
    None
}

/// The item whose file `path` is (see the module header), by ID.
fn item_of_file<'a>(loaded: &'a Loaded, path: &str, root: Option<&str>) -> Option<&'a str> {
    let by_path: HashMap<String, &str> = loaded
        .items
        .iter()
        .filter(|i| !i.path.is_empty())
        .map(|i| (slashes(&i.path), i.id.as_str()))
        .collect();
    if let Some(id) = relative(path, root).and_then(|rel| by_path.get(&rel).copied()) {
        return Some(id);
    }
    let path = slashes(path);
    // Not from the top: by the end of the path, the longest item path
    // first, so `a/design/x.md` isn't taken for `design/x.md`'s twin.
    let mut found: Option<(&str, usize)> = None;
    for (p, id) in &by_path {
        let ends = path.len() > p.len()
            && path.ends_with(p.as_str())
            && path.as_bytes()[path.len() - p.len() - 1] == b'/';
        if ends && found.is_none_or(|(_, n)| p.len() > n) {
            found = Some((id, p.len()));
        }
    }
    found.map(|(id, _)| id)
}

/// The words of `text` an ID may be in: runs of letters, digits, `-` and
/// `_`, each with where it starts and ends, in characters.
fn words(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut start = 0;
    for (n, c) in text.chars().enumerate() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            if word.is_empty() {
                start = n;
            }
            word.push(c);
        } else if !word.is_empty() {
            out.push((start, n, std::mem::take(&mut word)));
        }
    }
    if !word.is_empty() {
        let end = start + word.chars().count();
        out.push((start, end, word));
    }
    out
}

/// The IDs `kiln list` has in `word`, its `-`-separated parts joined
/// again: those with the part at character `at` (of the word) first, then
/// the others, each left to right.
fn ids_in<'a>(loaded: &'a Loaded, word: &str, at: Option<usize>) -> Vec<&'a str> {
    let parts: Vec<&str> = word.split('-').collect();
    // Which part `at` is in.
    let under = at.map(|at| {
        let mut end = 0;
        parts
            .iter()
            .position(|p| {
                end += p.chars().count() + 1;
                at < end
            })
            .unwrap_or(parts.len().saturating_sub(1))
    });
    let mut over = Vec::new();
    let mut rest = Vec::new();
    for i in 0..parts.len() {
        for j in i..parts.len() {
            let joined = parts[i..=j].join("-");
            if let Some(item) = loaded.item(&joined) {
                let id = item.id.as_str();
                match under {
                    Some(u) if i <= u && u <= j => over.push(id),
                    _ => rest.push(id),
                }
            }
        }
    }
    over.extend(rest);
    over
}

/// Whether `word` looks like an ID (`REQ-099`): letters, a `-`, digits.
/// Only for saying why nothing was found; an ID is found by the project.
fn id_shaped(word: &str) -> bool {
    let mut parts = word.split('-');
    let (Some(head), Some(tail)) = (parts.next(), parts.next()) else {
        return false;
    };
    head.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
}

/// The ID of the item `spot` is about: the one under the cursor, else (a
/// selection: the first in it; no column: the first on the line) the one
/// whose file it is. Or why there's none, in a line that names what it
/// looked at.
pub fn item_at(loaded: &Loaded, spot: &Spot) -> Result<String, String> {
    let words = words(&spot.text);
    let mut near: Option<&str> = None;
    match (spot.selected, spot.column) {
        (false, Some(col)) => {
            let col = col as usize;
            if let Some((start, _, word)) = words.iter().find(|(s, e, _)| *s <= col && col <= *e) {
                if let Some(id) = ids_in(loaded, word, Some(col - start)).first() {
                    return Ok(id.to_string());
                }
                near = Some(word);
            }
        }
        _ => {
            if let Some(id) = words
                .iter()
                .find_map(|(_, _, w)| ids_in(loaded, w, None).first().copied())
            {
                return Ok(id.to_string());
            }
        }
    }
    let Some(path) = &spot.path else {
        return Err(match near {
            Some(word) if id_shaped(word) => {
                format!("{word} isn't an item of this project: kiln list has no such ID")
            }
            _ => "no Kiln ID under the cursor, and no file in front: put the cursor on an item's ID, or open an item's file".to_string(),
        });
    };
    if let Some(id) = item_of_file(loaded, path, spot.root.as_deref()) {
        return Ok(id.to_string());
    }
    let file = relative(path, spot.root.as_deref()).unwrap_or_else(|| path.clone());
    Err(match near {
        Some(word) if id_shaped(word) => format!(
            "{word} (line {} of {file}) isn't an item of this project: kiln list has no such ID",
            spot.line
        ),
        _ => format!(
            "no Kiln ID under the cursor on line {} of {file}, and {file} isn't an item's file",
            spot.line
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::frame::tests::demo;

    fn at(text: &str, column: u32) -> Spot {
        Spot {
            path: Some("/home/me/demo/src/log.rs".into()),
            root: Some("/home/me/demo".into()),
            line: 4,
            text: text.into(),
            column: Some(column),
            selected: false,
        }
    }

    #[test]
    fn the_id_under_the_cursor_is_found_by_the_project() {
        let d = demo();
        let line = "// implements: REQ-001, and see TASK-002";
        assert_eq!(item_at(&d, &at(line, 16)).as_deref(), Ok("REQ-001"));
        assert_eq!(
            item_at(&d, &at(line, 15)).as_deref(),
            Ok("REQ-001"),
            "its first character"
        );
        assert_eq!(
            item_at(&d, &at(line, 22)).as_deref(),
            Ok("REQ-001"),
            "just after it"
        );
        assert_eq!(item_at(&d, &at(line, 39)).as_deref(), Ok("TASK-002"));
        // Off any ID, in a file that isn't an item's.
        assert_eq!(
            item_at(&d, &at(line, 5)),
            Err("no Kiln ID under the cursor on line 4 of src/log.rs, and src/log.rs isn't an item's file".into())
        );
    }

    #[test]
    fn an_id_inside_a_file_name_is_found_and_the_part_under_the_cursor_wins() {
        let d = demo();
        let line = "see design/tasks/TASK-002-write-readings-to-sqlite.md and TASK-007";
        assert_eq!(item_at(&d, &at(line, 23)).as_deref(), Ok("TASK-002"));
        assert_eq!(
            item_at(&d, &at(line, 30)).as_deref(),
            Ok("TASK-002"),
            "on `write`"
        );
    }

    #[test]
    fn an_unknown_id_says_kiln_has_none_and_a_lookalike_is_not_one() {
        let d = demo();
        assert_eq!(
            item_at(&d, &at("reference REQ-099 here", 12)),
            Err("REQ-099 (line 4 of src/log.rs) isn't an item of this project: kiln list has no such ID".into())
        );
        // A word shaped like one is no ID unless kiln lists it.
        assert!(item_at(&d, &at("utf-8 UTF-8", 8))
            .unwrap_err()
            .starts_with("UTF-8 (line 4"));
        assert!(item_at(&d, &at("utf-8 UTF-8", 2))
            .unwrap_err()
            .starts_with("no Kiln ID"));
    }

    #[test]
    fn a_selection_or_no_column_takes_the_first_id_in_it() {
        let d = demo();
        let mut s = at("x REQ-003 y TASK-001", 0);
        s.selected = true;
        assert_eq!(item_at(&d, &s).as_deref(), Ok("REQ-003"));
        s.selected = false;
        s.column = None;
        assert_eq!(item_at(&d, &s).as_deref(), Ok("REQ-003"));
    }

    #[test]
    fn off_an_id_in_an_items_file_it_is_that_item() {
        let d = demo();
        let mut s = at("Some prose with no ID in it.", 3);
        s.path = Some("/home/me/demo/design/tasks/TASK-004-add-the-export-button.md".into());
        assert_eq!(item_at(&d, &s).as_deref(), Ok("TASK-004"));
        // Under /project, as the extension's own reads name it.
        s.root = None;
        s.path = Some("/project/design/tasks/TASK-004-add-the-export-button.md".into());
        assert_eq!(item_at(&d, &s).as_deref(), Ok("TASK-004"));
        // Windows, and a Kiln project below the folder Fenix opened.
        s.path = Some(r"C:\work\repo\plan\design\tasks\TASK-004-add-the-export-button.md".into());
        s.root = Some(r"C:\work\repo".into());
        assert_eq!(item_at(&d, &s).as_deref(), Ok("TASK-004"));
        // The ID under the cursor wins over the file's.
        s.text = "depends-on: TASK-003".into();
        s.column = Some(14);
        assert_eq!(item_at(&d, &s).as_deref(), Ok("TASK-003"));
    }

    #[test]
    fn not_in_a_file_says_so() {
        let d = demo();
        let s = Spot {
            text: String::new(),
            ..Spot::default()
        };
        assert!(item_at(&d, &s).unwrap_err().contains("no file in front"));
        assert_eq!(relative("/project/a/b.md", None).as_deref(), Some("a/b.md"));
        assert_eq!(relative("/projects/a.md", None), None);
        assert_eq!(
            relative("/home/me/x/a.md", Some("/home/me/x/")).as_deref(),
            Some("a.md")
        );
    }
}
