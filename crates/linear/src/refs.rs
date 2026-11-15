//! What a person (or the agenda) may call an issue by: its identifier
//! (`ENG-123`), a Linear address (`https://linear.app/acme/issue/ENG-123/fix-it`),
//! or Linear's own id. Everything else is no issue.

/// An issue, named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    /// `ENG-123`: the team's key and the number.
    Identifier { team: String, number: u64 },
    /// Linear's own id, a UUID.
    Id(String),
}

impl Named {
    /// As Linear's `issue(id:)` takes it.
    pub fn text(&self) -> String {
        match self {
            Named::Identifier { team, number } => format!("{team}-{number}"),
            Named::Id(id) => id.clone(),
        }
    }
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5 && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `ENG-123`, in any case: letters and digits starting with a letter, a
/// dash, and a number.
pub fn identifier(s: &str) -> Option<Named> {
    let (team, number) = s.trim().rsplit_once('-')?;
    let ok = !team.is_empty() && team.len() <= 12 && team.as_bytes()[0].is_ascii_alphabetic() && team.bytes().all(|b| b.is_ascii_alphanumeric());
    if !ok || number.is_empty() || number.len() > 9 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Named::Identifier { team: team.to_ascii_uppercase(), number: number.parse().ok()? })
}

/// An issue as it was asked for: its identifier, a Linear address, or an id.
pub fn parse(r: &str) -> Option<Named> {
    let r = r.trim();
    if let Some(rest) = r.strip_prefix("https://").or_else(|| r.strip_prefix("http://")) {
        let (host, path) = rest.split_once('/')?;
        if !host.eq_ignore_ascii_case("linear.app") {
            return None;
        }
        // `/acme/issue/ENG-123/fix-it`, or `/issue/ENG-123`, with a query or
        // a fragment after it perhaps.
        let path = path.split(['?', '#']).next().unwrap_or("");
        let mut parts = path.split('/');
        while let Some(p) = parts.next() {
            if p == "issue" {
                return parts.next().and_then(identifier);
            }
        }
        return None;
    }
    if is_uuid(r) {
        return Some(Named::Id(r.to_ascii_lowercase()));
    }
    identifier(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(team: &str, number: u64) -> Option<Named> {
        Some(Named::Identifier { team: team.into(), number })
    }

    #[test]
    fn an_issue_is_named_by_identifier_address_or_id() {
        assert_eq!(parse("ENG-123"), id("ENG", 123));
        assert_eq!(parse(" eng-7 "), id("ENG", 7), "typed in lower case");
        assert_eq!(parse("https://linear.app/acme/issue/ENG-123/fix-the-login"), id("ENG", 123));
        assert_eq!(parse("https://linear.app/acme/issue/eng-123"), id("ENG", 123));
        assert_eq!(parse("https://linear.app/issue/ENG-9?x=1#c"), id("ENG", 9));
        assert_eq!(parse("http://linear.app/acme/issue/ENG-1/x"), id("ENG", 1));
        assert_eq!(parse("3F2504E0-4F89-41D3-9A0C-0305E82C3301"), Some(Named::Id("3f2504e0-4f89-41d3-9a0c-0305e82c3301".into())));
        assert_eq!(parse("ENG-123").unwrap().text(), "ENG-123");
    }

    #[test]
    fn what_is_no_issue_is_none() {
        for bad in ["", "ENG", "ENG-", "-12", "1ENG-12", "ENG-12x", "ENG-1 2", "https://evil.example/acme/issue/ENG-1", "https://linear.app.evil.example/issue/ENG-1", "https://linear.app/acme/project/x", "https://linear.app/acme/issue/", "file:///etc/passwd", "ENG-99999999999", "ABCDEFGHIJKLM-1", "a b-1"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }
}
