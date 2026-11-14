//! Applying an edited YAML buffer back, safely or not at all:
//!
//! 1. The text is taken from the buffer once, and that text is what's
//!    checked and what's applied -- an edit after the check needs `r`.
//! 2. `kubectl apply --dry-run=server -o json` has the API server run the
//!    apply without keeping it: what it says it would make must be the one
//!    object the buffer was opened from (its kind, name and namespace), not
//!    a list, not another object.
//! 3. `kubectl diff` (the server's dry run again) shows what would change.
//! 4. Only then `a`, a question naming the context and the namespace (or,
//!    in a protected context, the name typed), and `y`.
//!
//! The YAML keeps the `resourceVersion` it was read with, so an object
//! changed by someone else since is refused by the API server, not
//! overwritten. A Secret's YAML is never offered for editing (its values
//! aren't in it).

use fenix_ui_kit::{Canvas, Key, Popup, Role, Style};
use serde_json::Value;

use crate::kubectl::Target;

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// The dry run and the diff are running.
    Checking,
    /// Both done: it can be applied.
    Ready,
    /// It can't be: why.
    Refused(String),
    Applying,
    Applied(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ApplyView {
    /// The scratch buffer it came from.
    pub scratch: u64,
    pub target: Target,
    pub context: Option<String>,
    /// The text checked, and the text applied.
    pub text: String,
    pub stage: Stage,
    pub dry_run: Option<Result<(), String>>,
    pub diff: Option<Result<Vec<String>, String>>,
    pub top: usize,
    pub confirm: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyAction {
    Nothing,
    Redraw,
    /// Asks to apply: `lib.rs` asks you (or refuses).
    Ask,
    Confirmed,
    Declined,
    /// The buffer read again and checked again.
    Recheck,
}

/// Whether the dry run's object is the one the buffer was opened from.
pub fn identity(json: &str, target: &Target) -> Result<(), String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "the dry run didn't answer with the object".to_string())?;
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or_default();
    if kind == "List" || kind.ends_with("List") {
        return Err("the buffer holds more than one object: only the one it was opened from is applied here".into());
    }
    let name = v.pointer("/metadata/name").and_then(Value::as_str).unwrap_or_default();
    let namespace = v.pointer("/metadata/namespace").and_then(Value::as_str).unwrap_or_default();
    if kind != target.kind.api_kind() || name != target.name || namespace != target.namespace {
        return Err(format!("the buffer is now {kind} {name} in {}, not the {} it was opened from: open that one's YAML to change it", if namespace.is_empty() { "no namespace" } else { namespace }, target));
    }
    Ok(())
}

impl ApplyView {
    pub fn new(scratch: u64, target: Target, context: Option<String>, text: String) -> ApplyView {
        ApplyView { scratch, target, context, text, stage: Stage::Checking, dry_run: None, diff: None, top: 0, confirm: None }
    }

    /// The dry run's end.
    pub fn dry_run_ended(&mut self, code: Option<i32>, out: &str, why: String) {
        self.dry_run = Some(match code {
            Some(0) => identity(out, &self.target),
            _ => Err(why),
        });
        self.settle();
    }

    /// The diff's end: 0 nothing differs, 1 something does, more an error.
    pub fn diff_ended(&mut self, code: Option<i32>, out: Vec<String>, why: String) {
        self.diff = Some(match code {
            Some(0) => Ok(Vec::new()),
            Some(1) => Ok(out.iter().map(|l| crate::agent::plain(l)).collect()),
            _ => Err(why),
        });
        self.settle();
    }

    fn settle(&mut self) {
        self.stage = match (&self.dry_run, &self.diff) {
            (Some(Err(why)), _) => Stage::Refused(why.clone()),
            (_, Some(Err(why))) => Stage::Refused(format!("kubectl diff failed: {why}")),
            (Some(Ok(())), Some(Ok(_))) => Stage::Ready,
            _ => Stage::Checking,
        };
    }

    pub fn key(&mut self, key: Key) -> ApplyAction {
        if self.confirm.take().is_some() {
            return if key == Key::Char('y') { ApplyAction::Confirmed } else { ApplyAction::Declined };
        }
        match key {
            Key::Char('a') if self.stage == Stage::Ready && self.diff.as_ref().is_some_and(|d| d.as_ref().is_ok_and(|d| !d.is_empty())) => ApplyAction::Ask,
            Key::Char('a') => ApplyAction::Nothing,
            Key::Char('r') if self.stage != Stage::Applying => ApplyAction::Recheck,
            Key::Char('j') | Key::Down => {
                self.top += 1;
                ApplyAction::Redraw
            }
            Key::Char('k') | Key::Up => {
                self.top = self.top.saturating_sub(1);
                ApplyAction::Redraw
            }
            Key::Char('g') => {
                self.top = 0;
                ApplyAction::Redraw
            }
            _ => ApplyAction::Nothing,
        }
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let w = width.max(30);
        let h = height.max(6);
        let x = c.put(0, 0, "Apply  ", Role::Title);
        c.put_fit(x, 0, w.saturating_sub(x), &format!("{} · context {} · namespace {}", self.target, self.context.as_deref().unwrap_or("?"), self.target.namespace), Role::Text);
        let (state, role) = match &self.stage {
            Stage::Checking => ("Checking: a dry run on the server, and the diff…".to_string(), Role::Muted),
            Stage::Ready if self.diff.as_ref().is_some_and(|d| d.as_ref().is_ok_and(Vec::is_empty)) => ("Nothing would change: the buffer is what the cluster has.".to_string(), Role::Muted),
            Stage::Ready => ("The server's dry run passed. a applies it, after a question.".to_string(), Role::Good),
            Stage::Refused(why) => (format!("Not applied: {why}"), Role::Bad),
            Stage::Applying => ("Applying…".to_string(), Role::Muted),
            Stage::Applied(what) => (format!("Applied: {what}"), Role::Good),
            Stage::Failed(why) => (format!("kubectl apply failed: {why}"), Role::Bad),
        };
        c.put_fit(0, 1, w, &state, role);
        c.rule(2, 0..w);
        let diff: &[String] = match &self.diff {
            Some(Ok(d)) => d,
            _ => &[],
        };
        let view = h.saturating_sub(4);
        let top = self.top.min(diff.len().saturating_sub(view));
        for (i, l) in diff.iter().skip(top).take(view).enumerate() {
            let role = if l.starts_with("+++") || l.starts_with("---") || l.starts_with("diff ") {
                Role::Title
            } else if l.starts_with('+') {
                Role::Good
            } else if l.starts_with('-') {
                Role::Bad
            } else if l.starts_with("@@") {
                Role::Accent
            } else {
                Role::Text
            };
            c.put_fit(0, 3 + i, w, l, role);
        }
        c.put_fit(0, h - 1, w, "a apply  r check again (the buffer read again)  j k scroll  q close", Role::Muted);
        if let Some(question) = &self.confirm {
            let rows = vec![vec![(question.clone(), Style::new(Role::Warn))], vec![("y ".to_string(), Style::new(Role::Accent)), ("yes, apply it".to_string(), Style::new(Role::Text)), ("    any other key: no".to_string(), Style::new(Role::Muted))]];
            c.popup(Popup { line: 1, col: 2, rows });
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kubectl::Kind;

    fn web() -> Target {
        Target::new(Kind::Deployment, "shop", "web").unwrap()
    }

    #[test]
    fn only_the_object_it_was_opened_from_is_applied() {
        assert_eq!(identity(r#"{"kind":"Deployment","metadata":{"name":"web","namespace":"shop"}}"#, &web()), Ok(()));
        assert!(identity(r#"{"kind":"List","items":[]}"#, &web()).unwrap_err().contains("more than one object"));
        assert!(identity(r#"{"kind":"Deployment","metadata":{"name":"api","namespace":"shop"}}"#, &web()).unwrap_err().contains("not the deployment web"));
        assert!(identity(r#"{"kind":"Deployment","metadata":{"name":"web","namespace":"prod"}}"#, &web()).is_err());
        assert!(identity(r#"{"kind":"Service","metadata":{"name":"web","namespace":"shop"}}"#, &web()).is_err());
        assert!(identity("not json", &web()).is_err());
    }

    #[test]
    fn it_is_ready_only_once_both_checks_pass_and_something_changes() {
        let mut v = ApplyView::new(7, web(), Some("kind-dev".into()), "kind: Deployment".into());
        assert_eq!(v.key(Key::Char('a')), ApplyAction::Nothing, "not checked yet");
        v.dry_run_ended(Some(0), r#"{"kind":"Deployment","metadata":{"name":"web","namespace":"shop"}}"#, String::new());
        assert_eq!(v.stage, Stage::Checking);
        v.diff_ended(Some(1), vec!["-  replicas: 3".into(), "+  replicas: 5".into()], String::new());
        assert_eq!(v.stage, Stage::Ready);
        assert_eq!(v.key(Key::Char('a')), ApplyAction::Ask);
        v.confirm = Some("Apply?".into());
        assert_eq!(v.key(Key::Char('n')), ApplyAction::Declined, "anything but y is no");
        v.confirm = Some("Apply?".into());
        assert_eq!(v.key(Key::Char('y')), ApplyAction::Confirmed);
        // Nothing differs: nothing to apply.
        let mut same = ApplyView::new(7, web(), None, String::new());
        same.dry_run_ended(Some(0), r#"{"kind":"Deployment","metadata":{"name":"web","namespace":"shop"}}"#, String::new());
        same.diff_ended(Some(0), Vec::new(), String::new());
        assert_eq!(same.key(Key::Char('a')), ApplyAction::Nothing);
        // A dry run the server refused.
        let mut bad = ApplyView::new(7, web(), None, String::new());
        bad.dry_run_ended(Some(1), "", "Deployment.apps \"web\" is invalid: spec.replicas: Invalid value: -1".into());
        bad.diff_ended(Some(2), Vec::new(), "the same".into());
        assert!(matches!(&bad.stage, Stage::Refused(w) if w.contains("Invalid value")));
        assert_eq!(bad.key(Key::Char('a')), ApplyAction::Nothing);
    }
}
