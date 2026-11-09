//! A code cell's lines coloured as Fenix colours code -- keywords,
//! strings, comments, numbers, definitions' names -- by the theme's
//! syntax captures. Python's keywords for a Python notebook; another
//! language's strings, comments and numbers.

use fenix_ui_kit::Role;

const PYTHON: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return",
    "try", "while", "with", "yield",
];

const BUILTINS: &[&str] = &[
    "abs", "all", "any", "bool", "dict", "dir", "enumerate", "filter", "float", "format", "getattr", "hasattr", "input", "int", "isinstance", "len", "list", "map", "max", "min", "next", "open", "print", "range", "repr", "reversed", "round", "set", "setattr", "sorted", "str", "sum",
    "super", "tuple", "type", "zip", "self",
];

fn syntax(name: &str) -> Role {
    Role::Syntax(name.into())
}

/// Where a line starts: inside a string three quotes long, or not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    open: Option<char>,
}

/// `source`'s lines, each as coloured runs.
pub fn lines(source: &str, language: &str) -> Vec<Vec<(String, Role)>> {
    let python = language.starts_with("python") || language.is_empty();
    let mut state = State::default();
    let mut out: Vec<Vec<(String, Role)>> = source.split('\n').map(|l| line(l, python, &mut state)).collect();
    if source.ends_with('\n') {
        out.pop();
    }
    out
}

fn line(text: &str, python: bool, state: &mut State) -> Vec<(String, Role)> {
    let chars: Vec<char> = text.replace('\t', "    ").chars().collect();
    let mut out: Vec<(String, Role)> = Vec::new();
    let push = |out: &mut Vec<(String, Role)>, s: String, role: Role| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.1 == role => last.0.push_str(&s),
            _ => out.push((s, role)),
        }
    };
    let mut i = 0;
    let mut last_word = String::new();
    // Inside a triple-quoted string from the line before.
    if let Some(q) = state.open {
        let end = find_triple(&chars, 0, q);
        let stop = end.map_or(chars.len(), |e| e + 3);
        push(&mut out, chars[..stop].iter().collect(), syntax("string"));
        if end.is_some() {
            state.open = None;
        }
        i = stop;
    }
    while i < chars.len() {
        let c = chars[i];
        if c == '#' || (!python && c == '/' && chars.get(i + 1) == Some(&'/')) {
            push(&mut out, chars[i..].iter().collect(), syntax("comment"));
            break;
        }
        if c == '"' || c == '\'' {
            // A prefix (f, r, b) is part of the word before it: coloured as one.
            if chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c) {
                let end = find_triple(&chars, i + 3, c);
                let stop = end.map_or(chars.len(), |e| e + 3);
                push(&mut out, chars[i..stop].iter().collect(), syntax("string"));
                if end.is_none() {
                    state.open = Some(c);
                }
                i = stop;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            let stop = (j + 1).min(chars.len());
            push(&mut out, chars[i..stop].iter().collect(), syntax("string"));
            i = stop;
            continue;
        }
        if c.is_ascii_digit() && !(i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_') {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), syntax("number"));
            i = j;
            last_word.clear();
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            // A string's prefix: f"...", rb'...'.
            if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') && word.len() <= 2 && word.chars().all(|c| "rRbBfFuU".contains(c)) {
                push(&mut out, word, syntax("string"));
                i = j;
                continue;
            }
            let role = if python && matches!(last_word.as_str(), "def") {
                syntax("function")
            } else if python && last_word == "class" {
                syntax("type")
            } else if python && matches!(word.as_str(), "True" | "False" | "None") {
                syntax("constant.builtin")
            } else if python && PYTHON.contains(&word.as_str()) {
                syntax("keyword")
            } else if python && BUILTINS.contains(&word.as_str()) && chars.get(j) == Some(&'(') {
                syntax("function.builtin")
            } else if chars.get(j) == Some(&'(') {
                syntax("function")
            } else {
                Role::Text
            };
            push(&mut out, word.clone(), role);
            last_word = word;
            i = j;
            continue;
        }
        if !c.is_whitespace() {
            last_word.clear();
        }
        push(&mut out, c.to_string(), Role::Text);
        i += 1;
    }
    out
}

fn find_triple(chars: &[char], from: usize, q: char) -> Option<usize> {
    (from..chars.len().saturating_sub(2)).find(|&j| chars[j] == q && chars[j + 1] == q && chars[j + 2] == q && (j == 0 || chars[j - 1] != '\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_of(runs: &[(String, Role)], text: &str) -> Option<Role> {
        runs.iter().find(|(t, _)| t == text).map(|(_, r)| r.clone())
    }

    #[test]
    fn python_is_coloured() {
        let l = lines("def area(r):  # circle\n    return 3.14 * r ** 2", "python");
        assert_eq!(role_of(&l[0], "def"), Some(syntax("keyword")));
        assert_eq!(role_of(&l[0], "area"), Some(syntax("function")));
        assert_eq!(role_of(&l[0], "# circle"), Some(syntax("comment")));
        assert_eq!(role_of(&l[1], "3.14"), Some(syntax("number")));
        assert_eq!(l[1].iter().map(|(t, _)| t.as_str()).collect::<String>(), "    return 3.14 * r ** 2");
    }

    #[test]
    fn strings_with_prefixes_and_on_several_lines() {
        let l = lines("print(f\"{x}\")\ns = \"\"\"one\ntwo\"\"\" + 'a#b'", "python");
        assert_eq!(role_of(&l[0], "print"), Some(syntax("function.builtin")));
        assert_eq!(role_of(&l[0], "f\"{x}\""), Some(syntax("string")));
        assert_eq!(role_of(&l[1], "\"\"\"one"), Some(syntax("string")));
        assert_eq!(role_of(&l[2], "two\"\"\""), Some(syntax("string")));
        assert_eq!(role_of(&l[2], "'a#b'"), Some(syntax("string")));
    }

    #[test]
    fn a_name_with_digits_isnt_a_number() {
        let l = lines("x2 = df1", "python");
        assert!(l[0].iter().all(|(_, r)| *r == Role::Text), "{:?}", l[0]);
        assert_eq!(l.len(), 1);
    }
}
