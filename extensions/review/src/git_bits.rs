//! What the review pages share with Fenix's own Git pages: counted
//! nouns, short hashes, and a menu shown as a popup.

use crate::page::{fit, Popup, Role};

pub(crate) fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

pub(crate) fn count(n: usize, what: &str) -> String {
    let plural = if what.ends_with("ch") || what.ends_with('s') { format!("{what}es") } else { format!("{what}s") };
    format!("{n} {}", if n == 1 { what.to_string() } else { plural })
}

/// One entry of a menu. `key` is what a pick matches on.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub detail: String,
    /// A flag's state; `None` for a verb.
    pub flag: Option<bool>,
    pub danger: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub title: String,
    pub groups: Vec<(Option<String>, Vec<MenuItem>)>,
}

/// A menu (with the keys typed so far) as a popup beside page line
/// `line`, its text lined up with column `col`.
pub(crate) fn popup(line: usize, col: usize, menu: Option<(&Menu, &str)>) -> Option<Popup> {
    let (menu, typed) = menu?;
    let mut rows: Vec<Vec<(String, Role)>> = Vec::new();
    // Wide enough for the widest row: key, label, and its detail pushed
    // to the right edge.
    const KEY: usize = 7;
    let widest = menu
        .groups
        .iter()
        .flat_map(|(_, items)| items)
        .map(|item| {
            let detail = match item.flag {
                Some(_) => 3,
                None => item.detail.chars().count(),
            };
            KEY + item.label.chars().count() + if detail > 0 { detail + 4 } else { 0 }
        })
        .chain([menu.title.chars().count()])
        .chain(menu.groups.iter().filter_map(|(t, _)| t.as_ref().map(|t| t.chars().count())))
        .max()
        .unwrap_or(0);
    let w = widest.clamp(28, 72);
    rows.push(vec![(fit(&menu.title, w), Role::Title)]);
    for (title, items) in &menu.groups {
        rows.push(Vec::new());
        if let Some(title) = title {
            rows.push(vec![(title.to_uppercase(), Role::Muted)]);
        }
        for item in items {
            let typed = !typed.is_empty() && item.key.starts_with(typed);
            let key_role = if typed { Role::Title } else if item.danger { Role::Bad } else { Role::Accent };
            let mut row = vec![(format!("{:<KEY$}", item.key), key_role)];
            let label = fit(&item.label, w.saturating_sub(KEY));
            let used = KEY + label.chars().count();
            row.push((label, Role::Text));
            let (detail, role) = match item.flag {
                Some(true) => ("on".to_string(), Role::Good),
                Some(false) => ("off".to_string(), Role::Muted),
                None => (item.detail.clone(), Role::Muted),
            };
            let room = w.saturating_sub(used + 2);
            if !detail.is_empty() && room > 3 {
                let d = fit(&detail, room);
                row.push((" ".repeat(w - used - d.chars().count()), Role::Text));
                row.push((d, role));
            }
            rows.push(row);
        }
    }
    Some(Popup { line, col, rows })
}
