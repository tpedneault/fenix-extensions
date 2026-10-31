//! Its pages: the client (collections, the request, its response), the
//! response on its own, and the runner's report.

pub mod client;
pub mod report;
pub mod response;

use fenix_ui_kit::Role;

/// A method's colour, for its badge.
pub fn method_role(method: &str) -> Role {
    match method {
        "GET" => Role::Good,
        "POST" => Role::Accent,
        "PUT" | "PATCH" => Role::Warn,
        "DELETE" => Role::Bad,
        _ => Role::Muted,
    }
}

/// `text` wrapped to `width`, its own lines kept.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    text.lines().flat_map(|l| if l.is_empty() { vec![String::new()] } else { fenix_ui_kit::text::wrap(l, width.max(10)) }).collect()
}

/// A tab, its badge left off when there's nothing to say.
pub fn tab(title: &str, badge: String) -> fenix_ui_kit::Tab {
    let t = fenix_ui_kit::Tab::new(title);
    if badge.is_empty() {
        t
    } else {
        t.badge(badge)
    }
}

/// A value hidden but for its first characters: a token on the Auth tab.
pub fn mask(value: &str) -> String {
    let n = value.chars().count();
    if n <= 8 || value.starts_with("{{") {
        return value.to_string();
    }
    format!("{}…{}", value.chars().take(4).collect::<String>(), "•".repeat(6))
}
