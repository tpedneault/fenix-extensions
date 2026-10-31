//! Where a variable's name is being typed: after `process.env.`,
//! `os.environ["`, `std::env::var("` and the like in code, after `${` in
//! an env file.

/// Followed by the name itself.
const DOTTED: &[&str] = &["process.env.", "import.meta.env.", "Bun.env."];

/// Followed by the name in quotes.
const CALLS: &[&str] = &[
    "process.env[",
    "os.environ[",
    "os.environ.get(",
    "environ.get(",
    "getenv(",
    "os.Getenv(",
    "os.LookupEnv(",
    "env::var(",
    "env::var_os(",
    "env!(",
    "option_env!(",
    "dotenvy::var(",
    "$_ENV[",
    "$_SERVER[",
    "ENV[",
    "ENV.fetch(",
    "Deno.env.get(",
];

/// A name being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The column (in characters) its first letter is at.
    pub start: usize,
    /// What's typed of it.
    pub prefix: String,
    /// No quote typed: the name goes in quotes.
    pub quote: bool,
    /// Only a name code can write after a dot will do.
    pub dotted: bool,
}

fn ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `before` ends with `what`, not as the end of a longer name.
fn ends_with(before: &[char], what: &str) -> bool {
    let w: Vec<char> = what.chars().collect();
    before.len() >= w.len() && before[before.len() - w.len()..] == w[..] && (before.len() == w.len() || !ident(before[before.len() - w.len() - 1]))
}

/// Where a name is being typed, given the line up to the cursor; in an
/// env file, after `$` or `${`.
pub fn site(before: &str, env_file: bool) -> Option<Site> {
    let chars: Vec<char> = before.chars().collect();
    let start = chars.iter().rposition(|c| !ident(*c)).map_or(0, |i| i + 1);
    let prefix: String = chars[start..].iter().collect();
    let rest = &chars[..start];
    let at = |quote, dotted| Some(Site { start, prefix: prefix.clone(), quote, dotted });
    if env_file {
        return if rest.last() == Some(&'$') || rest.ends_with(&['$', '{']) { at(false, true) } else { None };
    }
    if DOTTED.iter().any(|d| ends_with(rest, d)) {
        return at(false, true);
    }
    let trimmed = |r: &[char]| {
        let n = r.iter().rposition(|c| *c != ' ').map_or(0, |i| i + 1);
        r[..n].to_vec()
    };
    if let Some((&q, r)) = rest.split_last() {
        if q == '"' || q == '\'' {
            let r = trimmed(r);
            if CALLS.iter().any(|c| ends_with(&r, c)) {
                return at(false, false);
            }
        }
    }
    let r = trimmed(rest);
    if CALLS.iter().any(|c| ends_with(&r, c)) {
        return at(true, false);
    }
    None
}

/// Whether `key` can follow a dot in code.
pub fn dottable(key: &str) -> bool {
    !key.is_empty() && key.chars().all(ident) && !key.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(before: &str) -> Option<(usize, String, bool, bool)> {
        site(before, false).map(|s| (s.start, s.prefix, s.quote, s.dotted))
    }

    #[test]
    fn names_after_what_reads_the_environment() {
        assert_eq!(found("const url = process.env.DB_"), Some((24, "DB_".into(), false, true)));
        assert_eq!(found("x = import.meta.env."), Some((20, String::new(), false, true)));
        assert_eq!(found("key = os.environ[\"API"), Some((18, "API".into(), false, false)));
        assert_eq!(found("key = os.getenv('A"), Some((17, "A".into(), false, false)));
        assert_eq!(found("let k = std::env::var(\"K"), Some((23, "K".into(), false, false)));
        assert_eq!(found("$k = $_ENV['"), Some((12, String::new(), false, false)));
        assert_eq!(found("ENV.fetch(\"P"), Some((11, "P".into(), false, false)));
        assert_eq!(found("os.Getenv(\"HOME"), Some((11, "HOME".into(), false, false)));
        assert_eq!(found("k = os.getenv(AB"), Some((14, "AB".into(), true, false)), "no quote yet");
        assert_eq!(found("std::env::var( \"X"), Some((16, "X".into(), false, false)));
    }

    #[test]
    fn not_elsewhere() {
        assert_eq!(found("myprocess.env.X"), None);
        assert_eq!(found("process.env"), None);
        assert_eq!(found("print(\"X"), None);
        assert_eq!(found("MY_ENV['X"), None);
        assert_eq!(found("os.environ[x]."), None);
    }

    #[test]
    fn in_an_env_file_after_a_dollar() {
        assert_eq!(site("URL=http://${HO", true).map(|s| (s.start, s.prefix)), Some((13, "HO".into())));
        assert_eq!(site("A=$", true).map(|s| s.start), Some(3));
        assert_eq!(site("A=process.env.", true), None);
        assert!(dottable("API_KEY") && !dottable("my.key") && !dottable("1X"));
    }
}
