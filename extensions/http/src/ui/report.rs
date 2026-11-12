//! The collection runner's report: each request of a file or a folder,
//! sent in turn, and whether it passed -- its tests, or, with none, a
//! status under 400.

use fenix_ui_kit::{Canvas, Key, Rect, Role, Style, TreeNode, TreeState};

use crate::assert::Outcome as Test;
use crate::model::duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub file: String,
    pub label: String,
    pub method: String,
    /// The request line, from 0.
    pub line: u32,
    /// Its status and how long it took (microseconds), or why it failed.
    pub answer: Result<(u16, u64), String>,
    pub tests: Vec<Test>,
}

impl RunResult {
    pub fn passed(&self) -> bool {
        match &self.answer {
            Ok((status, _)) => {
                if self.tests.is_empty() {
                    *status < 400
                } else {
                    self.tests.iter().all(|t| t.passed)
                }
            }
            Err(_) => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// What ran: a file, or a folder.
    pub title: String,
    pub results: Vec<RunResult>,
    /// How many it's sending in all.
    pub planned: usize,
    pub done: bool,
    pub list: TreeState,
}

/// A run this long, or longer, is worth telling you about when it ends
/// while you're elsewhere: a notification's worth.
pub const LONG_RUN_MS: i64 = 10_000;

/// What to say when a run ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// Nothing: its report is in front of you, and says it.
    Nothing,
    /// A line on the modeline: a short run, ended while you were
    /// elsewhere.
    Status(String),
    /// A long one, ended while you were elsewhere: a notification (API
    /// 1.3's) when the extension has one; the modeline's line until then.
    Notice(String),
}

pub enum Action {
    Nothing,
    /// Open file at a line.
    Open(String, u32),
    Again,
}

impl Report {
    pub fn new(title: &str, planned: usize) -> Report {
        let mut list = TreeState::new();
        list.empty = "Sending…".into();
        Report { title: title.to_string(), planned, list, ..Report::default() }
    }

    /// What to say now it's done, `elapsed_ms` after it began, its
    /// report `in_front` of you or not.
    pub fn said(&self, elapsed_ms: i64, in_front: bool) -> Said {
        if in_front {
            return Said::Nothing;
        }
        let passed = self.results.iter().filter(|r| r.passed()).count();
        let failed = self.results.len() - passed;
        let mut s = format!("Run: {}: {passed}/{} passed", self.title, self.results.len());
        if failed > 0 {
            s.push_str(&format!(", {failed} failed"));
        }
        if elapsed_ms < LONG_RUN_MS {
            return Said::Status(s);
        }
        s.push_str(&format!(" in {}", duration(elapsed_ms.max(0) as u64 * 1000)));
        Said::Notice(s)
    }

    fn nodes(&self) -> Vec<TreeNode> {
        self.results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (mark, role) = if r.passed() { ("✓", Role::Good) } else { ("✗", Role::Bad) };
                let status = match &r.answer {
                    Ok((s, t)) => format!("{s}  {:>7}", duration(*t)),
                    Err(e) => format!("failed: {e}"),
                };
                let tests = if r.tests.is_empty() { String::new() } else { format!("  tests {}/{}", r.tests.iter().filter(|t| t.passed).count(), r.tests.len()) };
                let failing: Vec<TreeNode> = r.tests.iter().enumerate().filter(|(_, t)| !t.passed).map(|(j, t)| TreeNode::leaf(format!("{i}/{j}"), format!("✗ {}  (found {})", t.test, t.detail)).style(Role::Bad)).collect();
                let label = format!("{mark} {:<6} {}  {status}{tests}", r.method, r.label);
                let node = if failing.is_empty() { TreeNode::leaf(i.to_string(), label) } else { TreeNode::branch(i.to_string(), label, failing) };
                node.style(Style::new(role)).detail(r.file.clone())
            })
            .collect()
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let passed = self.results.iter().filter(|r| r.passed()).count();
        let failed = self.results.len() - passed;
        let mut x = c.put(0, 0, &format!("Run · {}", self.title), Role::Title);
        x = c.put(x, 0, &format!("   {passed} passed"), Role::Good);
        if failed > 0 {
            x = c.put(x, 0, &format!(" · {failed} failed"), Role::Bad);
        }
        if !self.done {
            c.put(x, 0, &format!(" · sending {}/{}", (self.results.len() + 1).min(self.planned), self.planned), Role::Muted);
        }
        c.rule(1, 0..width);
        let nodes = self.nodes();
        self.list.render(&nodes, Rect::new(0, 2, width, height.saturating_sub(4)), &mut c);
        c.put(0, height.saturating_sub(1), "Enter open the request  l show what failed  r run again  q close", Role::Muted);
        c
    }

    pub fn key(&mut self, key: Key) -> Action {
        let nodes = self.nodes();
        match key {
            Key::Char('r') => Action::Again,
            Key::Enter => {
                let Some(node) = self.list.selected_node(&nodes) else { return Action::Nothing };
                let i: usize = node.id.split('/').next().and_then(|i| i.parse().ok()).unwrap_or(0);
                match self.results.get(i) {
                    Some(r) => Action::Open(r.file.clone(), r.line),
                    None => Action::Nothing,
                }
            }
            k => {
                self.list.handle_key(k, &nodes);
                Action::Nothing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(status: u16, tests: &[bool]) -> RunResult {
        RunResult {
            file: "api.http".into(),
            label: "users".into(),
            method: "GET".into(),
            line: 3,
            answer: Ok((status, 12_000)),
            tests: tests.iter().map(|&p| Test { test: "status == 200".into(), passed: p, detail: status.to_string() }).collect(),
        }
    }

    #[test]
    fn what_passed_and_what_failed() {
        assert!(result(200, &[]).passed());
        assert!(!result(404, &[]).passed(), "no tests: a status under 400");
        assert!(result(404, &[true]).passed(), "its tests say");
        assert!(!result(200, &[true, false]).passed());
        let mut r = Report::new("api.http", 3);
        r.results = vec![result(200, &[true]), result(500, &[false])];
        let c = r.render(80, 10);
        assert_eq!(c.line(0), "Run · api.http   1 passed · 1 failed · sending 3/3");
        assert!(c.line(2).starts_with("  ✓ GET    users  200    12 ms  tests 1/1"), "{}", c.line(2));
        assert!(c.line(3).starts_with("▸ ✗ GET    users  500"), "{}", c.line(3));
        r.list.selected = 1;
        assert!(matches!(r.key(Key::Enter), Action::Open(f, 3) if f == "api.http"));
        r.key(Key::Char('l'));
        r.done = true;
        let c = r.render(80, 10);
        assert!(c.line(4).contains("✗ status == 200  (found 500)"), "{}", c.text());
        assert_eq!(c.line(0), "Run · api.http   1 passed · 1 failed");
    }

    #[test]
    fn a_long_run_ended_elsewhere_is_a_notice_a_quick_one_a_line() {
        let mut r = Report::new("the project", 2);
        r.results = vec![result(200, &[]), result(500, &[])];
        r.done = true;
        assert_eq!(r.said(60_000, true), Said::Nothing, "its report is in front of you");
        assert_eq!(r.said(800, false), Said::Status("Run: the project: 1/2 passed, 1 failed".into()));
        match r.said(LONG_RUN_MS + 2_500, false) {
            Said::Notice(s) => assert!(s.starts_with("Run: the project: 1/2 passed, 1 failed in ") && s.ends_with('s'), "{s}"),
            other => panic!("{other:?}"),
        }
        r.results.pop();
        assert_eq!(r.said(10, false), Said::Status("Run: the project: 1/1 passed".into()), "nothing failed: not said");
    }
}
