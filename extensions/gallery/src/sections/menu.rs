//! A popup menu: what to do with a file, beside its row.

// gallery:begin
use fenix_ui_kit::text::width;
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Outcome, Popup, Rect, Role, Style, TreeNode, TreeState};

use crate::data::{size, FILES};
use crate::draw;
use crate::sections::Reply;

/// The menu's rows: what, and the key that picks it straight away.
const ACTIONS: [(&str, char); 5] = [("Open", 'o'), ("Open beside", 'b'), ("Copy the path", 'y'), ("Rename…", 'r'), ("Delete", 'd')];

pub struct State {
    pub list: TreeState,
    /// The menu, open, and its row picked.
    pub menu: Option<usize>,
}

fn items() -> Vec<TreeNode> {
    FILES.iter().map(|(path, bytes)| TreeNode::leaf(*path, *path).detail(size(*bytes))).collect()
}

impl State {
    pub fn new() -> State {
        State { list: TreeState::new(), menu: None }
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        match self.menu {
            Some(_) => vec![("j k", "move"), ("Enter", "pick"), ("o b y r d", "pick that"), ("m", "close it")],
            None => vec![("j k", "move"), ("m Enter", "the menu")],
        }
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let (foot, body) = rect.split_bottom(2);
        let (list, _) = body.split_left(body.w.min(44));
        let items = items();
        self.list.render(&items, list, c);
        let Some(picked) = self.menu else {
            draw::text(c, foot, 0, 1, "m opens a menu beside the row: Fenix draws it, over the page", Role::Muted);
            return;
        };
        // Beside the focused row -- the line the list drew it on -- after
        // the file's name. Fenix draws the box; keys still come here.
        let Some(&(line, _)) = c.focused() else { return };
        let name = self.list.selected_node(&items).map_or(0, |n| width(&n.label));
        let rows = ACTIONS
            .iter()
            .enumerate()
            .map(|(i, (what, key))| {
                let style = if i == picked { Style::new(Role::Accent) } else if *what == "Delete" { Style::new(Role::Bad) } else { Style::new(Role::Text) };
                vec![(format!("{} {what:<14}", if i == picked { "›" } else { " " }), style), (key.to_string(), Style::new(Role::Muted))]
            })
            .collect();
        c.popup(Popup { line, col: list.x + (name + 2).min(list.w.saturating_sub(1)), rows });
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        let items = items();
        let Some(at) = self.menu else {
            return match press.key {
                Key::Char('m') | Key::Enter => {
                    self.menu = Some(0);
                    Reply::Done
                }
                key => Reply::from(self.list.handle_key(key, &items)),
            };
        };
        let picked = match press.key {
            Key::Char('m') => {
                self.menu = None;
                return Reply::Done;
            }
            Key::Down | Key::Char('j') => {
                self.menu = Some((at + 1) % ACTIONS.len());
                return Reply::Done;
            }
            Key::Up | Key::Char('k') => {
                self.menu = Some((at + ACTIONS.len() - 1) % ACTIONS.len());
                return Reply::Done;
            }
            Key::Enter => at,
            Key::Char(c) => match ACTIONS.iter().position(|a| a.1 == c) {
                Some(i) => i,
                None => return Reply::Done,
            },
            _ => return Reply::Done,
        };
        self.menu = None;
        let path = self.list.selected_node(&items).map(|n| n.id.clone()).unwrap_or_default();
        match ACTIONS[picked].1 {
            'y' => Reply::Copy { text: path.clone(), say: format!("Copied {path}") },
            _ => Reply::Say(format!("{}: {path} -- a real extension would do it here", ACTIONS[picked].0)),
        }
    }

    /// A click picks a row (and closes the menu); a double click opens
    /// the menu on it.
    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        let outcome = self.list.mouse(m, &items());
        if outcome == Outcome::Submit {
            self.menu = Some(0);
            return Reply::Done;
        }
        if m.left_clicks() > 0 && self.menu.take().is_some() {
            return Reply::Done;
        }
        Reply::from(outcome)
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
