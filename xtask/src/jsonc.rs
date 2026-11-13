//! JSON with comments, as VS Code writes its themes and snippets: `//`
//! and `/* */` comments, and a comma after the last item of an object or
//! a list. Both are taken out -- comments become spaces, so a mistake
//! still says its line and column -- and what's left is read as JSON.

/// `text` read as JSON with comments.
pub fn parse(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&strip(text)).map_err(|e| format!("line {}, column {}: {e}", e.line(), e.column()))
}

/// `text` without its comments and trailing commas; strings as they are.
pub fn strip(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    // Where the last comma written outside a string is in `out`, while
    // nothing but blanks and comments has followed it.
    let mut comma: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                comma = None;
                out.push('"');
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if chars[i - 1] == '"' {
                        break;
                    }
                }
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                out.push_str("  ");
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < chars.len() {
                    out.push_str("  ");
                    i += 2;
                }
            }
            ',' => {
                comma = Some(out.len());
                out.push(',');
                i += 1;
            }
            '}' | ']' => {
                if let Some(at) = comma.take() {
                    out.replace_range(at..at + 1, " ");
                }
                out.push(c);
                i += 1;
            }
            c if c.is_whitespace() => {
                out.push(c);
                i += 1;
            }
            c => {
                comma = None;
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas_are_taken_out_and_strings_kept() {
        let text = "\u{feff}{\n  // a comment\n  \"a\": \"http://x/*y*/\", /* inline */\n  \"b\": [1, 2,],\n  \"c\": \"say \\\"//\\\"\",\n}\n";
        let v = parse(text).unwrap();
        assert_eq!(v["a"], "http://x/*y*/");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
        assert_eq!(v["c"], "say \"//\"");
        let err = parse("{\n\n  \"a\": nope\n}").unwrap_err();
        assert!(err.starts_with("line 3"), "{err}");
    }
}
