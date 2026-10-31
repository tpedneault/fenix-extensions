//! An env file against its example: the keys it lacks and those the
//! example doesn't have; and lines added at a file's end, as one edit.

use crate::parse::EnvFile;

/// The keys `example` has and `env` hasn't, in the example's order.
pub fn missing<'a>(env: &EnvFile, example: &'a EnvFile) -> Vec<&'a str> {
    let have = env.keys();
    example.keys().into_iter().filter(|k| !have.contains(k)).collect()
}

/// The keys `env` has and `example` hasn't, in `env`'s order.
pub fn extra<'a>(env: &'a EnvFile, example: &EnvFile) -> Vec<&'a str> {
    let known = example.keys();
    env.keys().into_iter().filter(|k| !known.contains(k)).collect()
}

/// Where `text` ends: its last line and the characters on it.
pub fn end_of(text: &str) -> (u32, u32) {
    let last = text.rsplit('\n').next().unwrap_or("");
    (text.matches('\n').count() as u32, last.chars().count() as u32)
}

/// `lines` added at the end of `text`, each on a line of its own, in the
/// line ending it uses: where, and what's inserted there.
pub fn append(text: &str, lines: &[String]) -> ((u32, u32), String) {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut add = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        add.push_str(nl);
    }
    for line in lines {
        add.push_str(line);
        add.push_str(nl);
    }
    (end_of(text), add)
}

/// The lines that add `keys` with empty values.
pub fn empty_lines(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| format!("{k}=")).collect()
}

/// The text's whole range, for replacing it.
pub fn whole(text: &str) -> ((u32, u32), (u32, u32)) {
    ((0, 0), end_of(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_extra_keys() {
        let example = EnvFile::parse("A=\nB=\n# note\nC=default\nA=again\n");
        let env = EnvFile::parse("B=1\nD=2\n");
        assert_eq!(missing(&env, &example), ["A", "C"]);
        assert_eq!(extra(&env, &example), ["D"]);
        assert!(missing(&example, &example).is_empty());
    }

    #[test]
    fn lines_go_at_the_end() {
        assert_eq!(append("A=1\n", &empty_lines(&["B", "C"])), ((1, 0), "B=\nC=\n".into()));
        assert_eq!(append("A=1", &empty_lines(&["B"])), ((0, 3), "\nB=\n".into()));
        assert_eq!(append("", &empty_lines(&["B"])), ((0, 0), "B=\n".into()));
        assert_eq!(append("A=1\r\nX=é\r\n", &empty_lines(&["B"])), ((2, 0), "B=\r\n".into()));
        assert_eq!(whole("ab\ncdé"), ((0, 0), (1, 3)));
    }
}
