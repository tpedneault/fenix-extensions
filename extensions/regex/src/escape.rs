//! The pattern as a string literal of another language, its flags with
//! it: inline (`(?i)`) where the language reads them, a JavaScript
//! literal's own after its `/`. Named groups are written the way the
//! language wants them.

use crate::engine::Flags;
use crate::syntax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    Go,
    Java,
    CSharp,
}

pub const LANGS: [Lang; 6] = [Lang::Rust, Lang::Python, Lang::JavaScript, Lang::Go, Lang::Java, Lang::CSharp];

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::JavaScript => "JavaScript",
            Lang::Go => "Go",
            Lang::Java => "Java",
            Lang::CSharp => "C#",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::CSharp => "csharp",
        }
    }

    pub fn from_id(id: &str) -> Option<Lang> {
        LANGS.into_iter().find(|l| l.id() == id)
    }

    /// The inline flags it reads.
    fn inline(self) -> &'static str {
        match self {
            Lang::Rust | Lang::Python | Lang::Java | Lang::CSharp => "imsx",
            Lang::Go => "ims",
            Lang::JavaScript => "",
        }
    }
}

/// The pattern copied for `lang`: the literal, and the flags it had to
/// leave out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub code: String,
    pub lost: Vec<char>,
}

pub fn copy(lang: Lang, pattern: &str, flags: Flags) -> Copied {
    let on: Vec<char> = flags.letters().chars().filter(|&c| c != 'u').collect();
    let mut lost: Vec<char> = Vec::new();
    let mut inline = String::new();
    for &c in &on {
        if lang.inline().contains(c) {
            inline.push(c);
        } else if !(lang == Lang::JavaScript && "ims".contains(c)) {
            lost.push(c);
        }
    }
    // Unicode off: Rust says so, Python has its ASCII flag.
    match (lang, flags.u) {
        (Lang::Rust, false) => inline.push_str("-u"),
        (Lang::Python, false) => inline.push('a'),
        _ => {}
    }
    let named = match lang {
        Lang::Rust | Lang::Go => pattern.to_string(),
        Lang::Python => syntax::named_groups_as(pattern, true, flags.x),
        Lang::JavaScript | Lang::Java | Lang::CSharp => syntax::named_groups_as(pattern, false, flags.x),
    };
    let body = if inline.is_empty() { named } else { format!("(?{inline}){named}") };
    let code = match lang {
        Lang::Rust => rust_raw(&body),
        Lang::Python => python(&body),
        Lang::JavaScript => {
            let mut suffix: String = on.iter().filter(|c| "ims".contains(**c)).collect();
            if flags.u {
                suffix.push('u');
            }
            format!("/{}/{suffix}", if body.is_empty() { "(?:)".to_string() } else { js_slashes(&body) })
        }
        Lang::Go if !body.contains('`') => format!("`{body}`"),
        Lang::Go | Lang::Java => quoted(&body),
        Lang::CSharp => format!("@\"{}\"", body.replace('"', "\"\"")),
    };
    Copied { code, lost }
}

/// `r"..."`, with as many `#` as it takes.
fn rust_raw(s: &str) -> String {
    let mut hashes = 0;
    while s.contains(&format!("\"{}", "#".repeat(hashes))) {
        hashes += 1;
    }
    let h = "#".repeat(hashes);
    format!("r{h}\"{s}\"{h}")
}

/// A raw string when one can hold it, else an escaped one.
fn python(s: &str) -> String {
    let odd_end = s.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1;
    if !odd_end && !s.contains('"') {
        format!("r\"{s}\"")
    } else if !odd_end && !s.contains('\'') {
        format!("r'{s}'")
    } else {
        quoted(s)
    }
}

/// `"..."` with `\` and `"` escaped.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Each `/` not already escaped, escaped.
fn js_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut escaped = false;
    for c in s.chars() {
        if c == '/' && !escaped {
            out.push('\\');
        }
        escaped = c == '\\' && !escaped;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(lang: Lang, p: &str, flags: Flags) -> String {
        copy(lang, p, flags).code
    }

    #[test]
    fn each_language_its_literal() {
        let p = r#"(?P<key>\w+)="([^"]*)"/\d"#;
        let f = Flags::default();
        assert_eq!(code(Lang::Rust, p, f), r###"r#"(?P<key>\w+)="([^"]*)"/\d"#"###);
        assert_eq!(code(Lang::Python, p, f), r#"r'(?P<key>\w+)="([^"]*)"/\d'"#);
        assert_eq!(code(Lang::JavaScript, p, f), r#"/(?<key>\w+)="([^"]*)"\/\d/u"#);
        assert_eq!(code(Lang::Go, p, f), r#"`(?P<key>\w+)="([^"]*)"/\d`"#);
        assert_eq!(code(Lang::Java, p, f), r#""(?<key>\\w+)=\"([^\"]*)\"/\\d""#);
        assert_eq!(code(Lang::CSharp, p, f), r#"@"(?<key>\w+)=""([^""]*)""/\d""#);
    }

    #[test]
    fn flags_go_inline_or_after_the_slash() {
        let f = Flags { i: true, x: true, ..Flags::default() };
        assert_eq!(code(Lang::Rust, "a", f), r#"r"(?ix)a""#);
        let js = copy(Lang::JavaScript, "a", f);
        assert_eq!(js.code, "/a/iu");
        assert_eq!(js.lost, ['x']);
        let go = copy(Lang::Go, "a", f);
        assert_eq!((go.code.as_str(), go.lost.as_slice()), ("`(?i)a`", &['x'][..]));
        let ascii = Flags { u: false, ..Flags::default() };
        assert_eq!(code(Lang::Rust, r"\w", ascii), r#"r"(?-u)\w""#);
        assert_eq!(code(Lang::Python, r"\w", ascii), r#"r"(?a)\w""#);
        assert_eq!(code(Lang::JavaScript, r"\w", ascii), r"/\w/");
    }

    #[test]
    fn awkward_endings_and_quotes() {
        assert_eq!(code(Lang::Python, r"a\\", Flags { u: true, ..Flags::default() }), r#"r"a\\""#, "an even run of backslashes is fine raw");
        assert_eq!(code(Lang::Python, r#"'"\"#, Flags::default()), r#""'\"\\""#);
        assert_eq!(code(Lang::Go, "`", Flags::default()), "\"`\"");
        assert_eq!(code(Lang::JavaScript, r"a\/b", Flags { u: false, ..Flags::default() }), r"/a\/b/", "an escaped slash stays as it is");
        assert_eq!(code(Lang::JavaScript, "", Flags { u: false, ..Flags::default() }), "/(?:)/");
        assert_eq!(Lang::from_id("csharp"), Some(Lang::CSharp));
    }
}
