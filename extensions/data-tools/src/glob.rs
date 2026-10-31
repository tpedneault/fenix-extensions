//! File patterns, as `.editorconfig` and SchemaStore's catalog write
//! them: `*`, `**`, `?`, `[abc]`, `[!abc]`, `{a,b}`.

use regex::Regex;

fn translate(pattern: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut braces = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                // `**/` is any folders, none too.
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    out.push_str("(?:.*/)?");
                } else {
                    out.push_str(".*");
                }
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => match chars[i + 1..].iter().position(|&c| c == ']') {
                Some(len) => {
                    let inner: String = chars[i + 1..i + 1 + len].iter().collect();
                    let (neg, inner) = inner.strip_prefix('!').map_or((false, inner.as_str()), |r| (true, r));
                    out.push('[');
                    if neg {
                        out.push('^');
                    }
                    out.push_str(&inner.replace('\\', "\\\\").replace('[', "\\["));
                    out.push(']');
                    i += len + 1;
                }
                None => out.push_str("\\["),
            },
            '{' => {
                braces += 1;
                out.push_str("(?:");
            }
            '}' if braces > 0 => {
                braces -= 1;
                out.push(')');
            }
            ',' if braces > 0 => out.push('|'),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                out.push_str(&regex::escape(&chars[i].to_string()));
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    for _ in 0..braces {
        out.push(')');
    }
    out
}

/// A pattern, matched against a whole path (with `/`).
pub struct Glob(Option<Regex>);

impl Glob {
    pub fn new(pattern: &str) -> Glob {
        Glob(Regex::new(&format!("^{}$", translate(pattern))).ok())
    }

    pub fn matches(&self, path: &str) -> bool {
        self.0.as_ref().is_some_and(|r| r.is_match(path))
    }
}

/// Whether `pattern` takes the file at `path` (a full path, `/` between
/// folders): one without a `/` by its name, one with by the end of its
/// path, `!` before it not.
pub fn file_matches(pattern: &str, path: &str) -> bool {
    let path = path.replace('\\', "/");
    let (negated, pattern) = pattern.strip_prefix('!').map_or((false, pattern), |p| (true, p));
    let pattern = pattern.trim_start_matches("./");
    let hit = if pattern.contains('/') { Glob::new(&format!("**/{}", pattern.trim_start_matches('/'))).matches(&path) } else { Glob::new(pattern).matches(path.rsplit('/').next().unwrap_or(&path)) };
    hit != negated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(Glob::new("*.json").matches("a.json"));
        assert!(!Glob::new("*.json").matches("x/a.json"));
        assert!(Glob::new("**/*.json").matches("x/y/a.json"));
        assert!(Glob::new("**/*.json").matches("a.json"));
        assert!(Glob::new("*.{yml,yaml}").matches("ci.yaml"));
        assert!(!Glob::new("*.{yml,yaml}").matches("ci.json"));
        assert!(Glob::new("file?.[ch]").matches("file1.c"));
        assert!(Glob::new("[!a]*").matches("b.txt"));
        assert!(!Glob::new("[!a]*").matches("a.txt"));
        assert!(Glob::new("a+b(1).json").matches("a+b(1).json"), "regex characters are themselves");
    }

    #[test]
    fn files_by_name_or_by_path() {
        assert!(file_matches("package.json", "/home/me/p/package.json"));
        assert!(!file_matches("package.json", "/home/me/p/package.json5"));
        assert!(file_matches(".github/workflows/*.yml", "/r/.github/workflows/ci.yml"));
        assert!(file_matches("**/.github/workflows/*.yaml", "/r/.github/workflows/ci.yaml"));
        assert!(!file_matches(".github/workflows/*.yml", "/r/workflows/ci.yml"));
        assert!(file_matches("tsconfig.*.json", "C:\\work\\tsconfig.base.json"));
        assert!(!file_matches("!*.json", "a.json"));
    }
}
