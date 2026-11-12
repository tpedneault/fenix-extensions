//! A list: a release's checklist, ticked off with Enter.

// gallery:begin
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Outcome, Rect, Role, Style, TreeNode, TreeState};

use crate::data::TASKS;
use crate::draw;
use crate::sections::Reply;

pub struct State {
    pub list: TreeState,
    /// Each task, whether it's done.
    pub done: Vec<bool>,
    /// `x` hides them all, to show the empty state.
    pub hidden: bool,
}

impl State {
    pub fn new() -> State {
        let mut list = TreeState::new();
        list.empty = "No tasks: x brings them back".into();
        State { list, done: TASKS.iter().map(|t| t.2).collect(), hidden: false }
    }

    /// A row a task: a tick when it's done, muted then; its day on the
    /// right.
    pub fn items(&self) -> Vec<TreeNode> {
        if self.hidden {
            return Vec::new();
        }
        TASKS
            .iter()
            .zip(&self.done)
            .enumerate()
            .map(|(i, ((what, due, _), &done))| {
                let (mark, style) = if done { ("✓ ", Style::new(Role::Muted)) } else { ("○ ", Style::default()) };
                TreeNode::leaf(i.to_string(), format!("{mark}{what}")).detail(*due).style(style)
            })
            .collect()
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("j k", "move"), ("J K", "a screen"), ("g G", "first, last"), ("Enter", "done, not done"), ("x", "empty, full")]
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let left = self.done.iter().filter(|d| !**d).count();
        draw::text(c, rect, 0, 0, &format!("Release 0.5.0 · {left} of {} to do", TASKS.len()), Role::Muted);
        let (_, body) = rect.split_top(2);
        let (foot, body) = body.split_bottom(2);
        let (list, _) = body.split_left(body.w.min(64));
        self.list.render(&self.items(), list, c);
        draw::text(c, foot, 0, 1, "j k J K g G move the same way in tables, trees, lists and scrolled regions", Role::Muted);
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        if press.key == Key::Char('x') {
            self.hidden = !self.hidden;
            return Reply::Done;
        }
        let items = self.items();
        let outcome = self.list.handle_key(press.key, &items);
        self.answer(outcome)
    }

    /// Enter (or a double click) ticks the task, or unticks it.
    fn answer(&mut self, outcome: Outcome) -> Reply {
        if outcome != Outcome::Submit {
            return Reply::from(outcome);
        }
        let i = self.list.selected.min(TASKS.len() - 1);
        self.done[i] = !self.done[i];
        Reply::Say(format!("{}: {}", TASKS[i].0, if self.done[i] { "done" } else { "to do again" }))
    }

    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        let items = self.items();
        let outcome = self.list.mouse(m, &items);
        self.answer(outcome)
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
