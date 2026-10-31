//! The rules Fenix masks env files' values by on its own (API 0.8.4):
//! applied to a file's buffer before it's first drawn and as it's
//! edited, so a value never shows as a file opens. They're regular
//! expressions on a line, so they take what's on one line -- `KEY=value`,
//! `KEY="value"` -- and the extension's own decorations the rest: a
//! quoted value over several lines, once you're in the file.

use crate::files;

/// What comes before a value: `export `, a key, `=`.
const KEY: &str = r"^[ \t]*(?:export[ \t]+)?[^=\s#][^=\s]*[ \t]*=[ \t]*";

/// The values, group 1 of each: unquoted, up to a `#` after a space,
/// trailing spaces not part of it; inside double quotes (escapes as they
/// are), single quotes, back quotes, closed on the line.
const VALUES: [&str; 4] = [r#"([^\s"'`#](?:[^ \t]|[ \t]+[^ \t#])*)"#, r#""((?:[^"\\]|\\.)*)""#, "'([^']*)'", "`([^`]*)`"];

/// The usual env files' names, for those it hasn't found in the project.
const USUAL: &[&str] = &[
    ".env",
    ".env.local",
    ".env.example",
    ".env.sample",
    ".env.template",
    ".env.dist",
    ".env.defaults",
    ".env.development",
    ".env.development.local",
    ".env.dev",
    ".env.test",
    ".env.test.local",
    ".env.testing",
    ".env.ci",
    ".env.staging",
    ".env.stage",
    ".env.preview",
    ".env.qa",
    ".env.uat",
    ".env.production",
    ".env.production.local",
    ".env.prod",
    ".env.docker",
    ".env.backup",
    ".env.bak",
];

/// The regular expressions, each a value in group 1.
pub fn patterns() -> Vec<String> {
    VALUES.iter().map(|v| format!("{KEY}{v}")).collect()
}

/// The names of the files the rules are for. Every env file by pattern
/// when examples are masked too and none is shown; else by name -- the
/// usual ones and those `known` (paths below the project) -- with the
/// examples left out unless they're masked, and the files shown
/// (`revealed`, full paths) left out, a pattern having no "but".
pub fn files(mask_examples: bool, known: &[String], revealed: &[String]) -> Vec<String> {
    if mask_examples && revealed.is_empty() {
        return [".env", ".env.*", "*.env"].map(String::from).to_vec();
    }
    let shown: Vec<String> = revealed.iter().map(|p| files::name(&p.replace('\\', "/")).to_ascii_lowercase()).collect();
    let mut names: Vec<String> = USUAL
        .iter()
        .map(|n| n.to_string())
        .chain(known.iter().map(|k| files::name(k).to_ascii_lowercase()))
        .filter(|n| (mask_examples || !files::is_example(n)) && !shown.contains(n))
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{EnvFile, Span};

    /// What the rules hide on each line of `text`, as the parser's spans.
    fn ruled(text: &str) -> Vec<Span> {
        let rules: Vec<regex::Regex> = patterns().iter().map(|p| regex::Regex::new(p).unwrap()).collect();
        let mut out = Vec::new();
        for (n, line) in text.split('\n').enumerate() {
            for rule in &rules {
                for c in rule.captures_iter(line) {
                    let Some(m) = c.get(1).filter(|m| !m.is_empty()) else { continue };
                    let col = |byte: usize| line[..byte].chars().count() as u32;
                    out.push(Span::new(n as u32, col(m.start()), col(m.end())));
                }
            }
        }
        out.sort_by_key(|s| (s.line, s.start));
        out
    }

    #[test]
    fn the_rules_hide_what_the_parser_reads_as_a_value() {
        let text = "\
# A=commented
A=plain
export B = spaced out  # a comment
C=a#b
D=\"double \\\" quoted\" # after
E='single'
F=`back`
G=
H=\"\"
  I=x
J=ünïcode
K=\"over
two lines\"
L=x #
";
        let parsed: Vec<Span> = EnvFile::parse(text).entries.iter().flat_map(|e| e.value.clone()).filter(|s| s.line != 11 && s.line != 12).collect();
        assert_eq!(ruled(text), parsed);
        // The value over two lines is the extension's own to hide, once
        // you're in the file: neither line is the rules'.
        assert!(ruled(text).iter().all(|s| s.line != 11 && s.line != 12));
    }

    #[test]
    fn every_pattern_has_its_group() {
        for p in patterns() {
            assert_eq!(regex::Regex::new(&p).unwrap().captures_len(), 2, "{p}");
        }
    }

    #[test]
    fn by_pattern_only_when_nothing_is_left_out() {
        assert_eq!(files(true, &[], &[]), [".env", ".env.*", "*.env"]);
        let known = vec!["api/.env.custom".to_string(), "prod.env".into(), "docs/example.env".into()];
        let names = files(false, &known, &[]);
        assert!(names.contains(&".env.custom".to_string()) && names.contains(&"prod.env".to_string()) && names.contains(&".env.staging".to_string()));
        assert!(!names.iter().any(|n| files::is_example(n)), "examples are shown: {names:?}");
        let names = files(true, &known, &["/home/me/app/.env.staging".to_string()]);
        assert!(names.contains(&".env.example".to_string()) && names.contains(&"example.env".to_string()));
        assert!(!names.contains(&".env.staging".to_string()), "shown");
        assert!(!names.iter().any(|n| n.contains('*')));
    }
}
