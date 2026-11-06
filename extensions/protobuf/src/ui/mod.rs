//! Its page: the gRPC client.

pub mod client;

use fenix_ui_kit::Role;

use crate::grpc::describe::Streaming;

/// A status code's colour: OK, the client's doing, the server's.
pub fn code_role(code: &str) -> Role {
    match code {
        "OK" => Role::Good,
        "" => Role::Muted,
        "Canceled" | "InvalidArgument" | "NotFound" | "AlreadyExists" | "PermissionDenied" | "FailedPrecondition" | "OutOfRange" | "Unauthenticated" => Role::Warn,
        _ => Role::Bad,
    }
}

/// How a method streams, as its badge's colour.
pub fn streaming_role(s: Streaming) -> Role {
    match s {
        Streaming::Unary => Role::Good,
        Streaming::Server => Role::Accent,
        Streaming::Client => Role::Warn,
        Streaming::Both => Role::Bad,
    }
}

/// A line of pretty JSON, coloured: its key as a property, its value by
/// what it is.
pub fn json_line(line: &str) -> Vec<(String, Role)> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start();
    let mut out = vec![(line[..indent].to_string(), Role::Text)];
    let value = match rest.strip_prefix('"').and_then(|r| r.find("\": ").map(|i| i + 1)) {
        Some(end) => {
            out.push((rest[..=end].to_string(), Role::Syntax("property".into())));
            out.push((": ".into(), Role::Syntax("punctuation".into())));
            &rest[end + 3..]
        }
        None => rest,
    };
    let role = match value.chars().next() {
        Some('"') => Role::Syntax("string".into()),
        Some(c) if c.is_ascii_digit() || c == '-' => Role::Syntax("number".into()),
        Some('t' | 'f') if value.starts_with("true") || value.starts_with("false") => Role::Syntax("boolean".into()),
        Some('n') if value.starts_with("null") => Role::Syntax("constant".into()),
        _ => Role::Syntax("punctuation".into()),
    };
    out.push((value.to_string(), role));
    out.retain(|(t, _)| !t.is_empty());
    out
}

/// Milliseconds as the page says them: `85 ms`, `1.24 s`.
pub fn ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_coloured_by_what_each_part_is() {
        assert_eq!(
            json_line("  \"name\": \"ISS\","),
            [("  ".to_string(), Role::Text), ("\"name\"".into(), Role::Syntax("property".into())), (": ".into(), Role::Syntax("punctuation".into())), ("\"ISS\",".into(), Role::Syntax("string".into()))]
        );
        assert_eq!(json_line("    \"period\": 92.9").last().unwrap().1, Role::Syntax("number".into()));
        assert_eq!(json_line("\"ok\": true").last().unwrap().1, Role::Syntax("boolean".into()));
        assert_eq!(json_line("  \"a\": {").last().unwrap().1, Role::Syntax("punctuation".into()));
        assert_eq!(json_line("}"), [("}".to_string(), Role::Syntax("punctuation".into()))]);
        assert_eq!((ms(85), ms(1240)), ("85 ms".to_string(), "1.24 s".to_string()));
        assert_eq!((code_role("OK"), code_role("NotFound"), code_role("Unavailable")), (Role::Good, Role::Warn, Role::Bad));
    }
}
