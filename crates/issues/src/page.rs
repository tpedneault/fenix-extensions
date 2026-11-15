//! The page: searches across the top, the selected search's issues, an
//! issue with its comments, and the new-issue form -- drawn with
//! `ui-kit`, keys answered with an [`Action`] the extension carries out.
//! Its keys follow Review's where the action is the same (`Enter`, `Tab`,
//! `o`, `u`, `q`, `C`, `S S`) and Jira's for an issue's fields (`e`, `E`,
//! `s`, `A`, `y`, `a`).
//!
//! Nothing it shows is trusted: every title, label, name and body is
//! someone's text, cleaned before it's drawn and never followed. Nothing
//! on the forge changes from a single key: a comment and an edit are
//! written in a pane or a prompt and sent from there, a move is picked
//! from the list `s` offers, and a new issue is sent with `S S`.

use fenix_forge::issues::{Host, Issue, IssueRef, Linked, NewIssue, Note, Search, Template};
use fenix_tracker::clean;
use fenix_ui_kit::{text, Canvas, Key, Role};

use crate::markdown;

/// A search on the page's top line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub label: String,
    pub search: Search,
}

/// The searches every page starts with, then a repository's or
/// project's open issues for each of `projects` (the one you're in
/// first), then your saved queries.
pub fn searches(host: Host, projects: &[String], queries: &[(String, String)]) -> Vec<Saved> {
    let mut out = vec![
        Saved { label: "Assigned to me".into(), search: Search::Assigned },
        Saved { label: "Created by me".into(), search: Search::Created },
        Saved { label: "Mentioning me".into(), search: Search::Mentioned },
    ];
    for p in projects {
        if !out.iter().any(|s| matches!(&s.search, Search::Project(q) if q.eq_ignore_ascii_case(p))) {
            out.push(Saved { label: p.clone(), search: Search::Project(p.clone()) });
        }
    }
    for (name, q) in queries.iter().filter(|(n, q)| !n.trim().is_empty() && !q.trim().is_empty()) {
        out.push(Saved { label: name.clone(), search: Search::Query(q.clone()) });
    }
    let _ = host;
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Issue,
    Form,
}

/// An issue open on the page, with what came with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail {
    pub issue: Issue,
    pub notes: Vec<Note>,
    pub linked: Vec<Linked>,
    pub scroll: usize,
}

/// A field of the new-issue form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Project,
    Title,
    Body,
    Labels,
    Assignees,
    Template,
}

const FIELDS: [Field; 6] = [Field::Project, Field::Title, Field::Body, Field::Labels, Field::Assignees, Field::Template];

/// The new-issue form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    pub new: NewIssue,
    pub field: usize,
    pub templates: Vec<Template>,
    pub template: Option<usize>,
    /// `S` pressed once: the next `S` sends it.
    pub armed: bool,
    pub sending: bool,
}

/// What a key asks of the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Close,
    /// A search's issues: its first page, or the page `more` names.
    Load { search: Search, more: Option<String> },
    /// An issue, with its comments and linked requests.
    Open(IssueRef),
    Browser(String),
    /// Text to the clipboard, and what it is ("ref", "address").
    Copy(String, &'static str),
    Comment(Issue),
    EditTitle(Issue),
    EditBody(Issue),
    EditLabels(Issue),
    EditAssignees(Issue),
    /// Its moves, to pick one.
    Moves(Issue),
    Branch(Issue),
    Agenda(Issue),
    /// A query typed in the forge's own syntax.
    Query,
    NewIssue,
    /// A form field to type: its current value.
    FormEdit(Field, String),
    FormTemplate,
    Submit(NewIssue),
    CancelForm,
}

/// The page.
#[derive(Debug, Clone)]
pub struct Page {
    pub host: Host,
    pub searches: Vec<Saved>,
    pub current: usize,
    pub issues: Vec<Issue>,
    pub cursor: usize,
    pub next: Option<String>,
    pub loading: bool,
    pub detail: Option<Detail>,
    pub form: Option<Form>,
    pub view: View,
    /// A line said at the foot: what happened, or why not.
    pub status: Option<(String, Role)>,
    /// What isn't set up yet, instead of the issues.
    pub not_ready: Option<String>,
    /// The rows the list showed last, for paging the cursor.
    shown: usize,
}

impl Page {
    pub fn new(host: Host, searches: Vec<Saved>) -> Page {
        Page { host, searches, current: 0, issues: Vec::new(), cursor: 0, next: None, loading: false, detail: None, form: None, view: View::List, status: None, not_ready: None, shown: 10 }
    }

    pub fn title(&self) -> String {
        format!("{} Issues", self.host.name())
    }

    /// The search shown.
    pub fn search(&self) -> Option<&Search> {
        self.searches.get(self.current).map(|s| &s.search)
    }

    /// The first page of the search shown, asked for.
    pub fn load(&mut self) -> Action {
        self.loading = true;
        match self.search().cloned() {
            Some(search) => Action::Load { search, more: None },
            None => Action::Redraw,
        }
    }

    /// A typed query, added as a search and shown.
    pub fn query(&mut self, q: &str) -> Action {
        let q = q.trim();
        if q.is_empty() {
            return Action::None;
        }
        let label = format!("\"{}\"", text::fit(q, 24));
        self.searches.retain(|s| !s.label.starts_with('"'));
        self.searches.push(Saved { label, search: Search::Query(q.to_string()) });
        self.current = self.searches.len() - 1;
        self.view = View::List;
        self.load()
    }

    /// A search's issues came: the first page, or more of them.
    pub fn listed(&mut self, search: &Search, more: bool, result: Result<(Vec<Issue>, Option<String>), String>) {
        if self.search() != Some(search) {
            return;
        }
        self.loading = false;
        match result {
            Ok((issues, next)) => {
                if more {
                    self.issues.extend(issues);
                } else {
                    self.issues = issues;
                    self.cursor = 0;
                }
                self.next = next;
                self.status = None;
            }
            Err(e) => self.status = Some((clean::message(&e), Role::Bad)),
        }
    }

    /// An issue came, to show.
    pub fn opened(&mut self, result: Result<Detail, String>) {
        match result {
            Ok(detail) => {
                let scroll = self.detail.as_ref().filter(|d| d.issue.key == detail.issue.key).map_or(0, |d| d.scroll);
                if let Some(row) = self.issues.iter_mut().find(|i| i.key == detail.issue.key) {
                    *row = detail.issue.clone();
                }
                self.detail = Some(Detail { scroll, ..detail });
                self.view = View::Issue;
            }
            Err(e) => self.status = Some((clean::message(&e), Role::Bad)),
        }
    }

    pub fn say(&mut self, text: &str, role: Role) {
        self.status = Some((clean::message(text), role));
    }

    /// The new-issue form, filled in.
    pub fn open_form(&mut self, new: NewIssue) {
        self.form = Some(Form { new, ..Default::default() });
        self.view = View::Form;
    }

    pub fn set_templates(&mut self, templates: Vec<Template>) {
        if let Some(f) = &mut self.form {
            f.templates = templates;
        }
    }

    /// A template's text and fields put in the form; the title and body
    /// already typed are kept when the template has none.
    pub fn use_template(&mut self, index: usize) {
        let Some(f) = &mut self.form else { return };
        let Some(t) = f.templates.get(index).cloned() else { return };
        if !t.title.is_empty() && f.new.title.trim().is_empty() {
            f.new.title = t.title.clone();
        }
        f.new.body = if f.new.body.trim().is_empty() { t.body.clone() } else { format!("{}\n\n{}", f.new.body.trim_end(), t.body) };
        for l in t.labels {
            if !f.new.labels.contains(&l) {
                f.new.labels.push(l);
            }
        }
        for a in t.assignees {
            if !f.new.assignees.contains(&a) {
                f.new.assignees.push(a);
            }
        }
        f.template = Some(index);
    }

    /// A field typed in.
    pub fn set_field(&mut self, field: Field, value: &str) {
        let Some(f) = &mut self.form else { return };
        let list = |v: &str| v.split(',').map(|s| s.trim().trim_start_matches('@').to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        match field {
            Field::Project => f.new.project = value.trim().to_string(),
            Field::Title => f.new.title = value.trim().to_string(),
            Field::Body => f.new.body = value.to_string(),
            Field::Labels => f.new.labels = list(value),
            Field::Assignees => f.new.assignees = list(value),
            Field::Template => {}
        }
        f.armed = false;
    }

    /// The form was sent: the issue made, shown; or why not, and the form
    /// kept.
    pub fn submitted(&mut self, result: Result<Issue, String>) {
        let Some(f) = &mut self.form else { return };
        f.sending = false;
        match result {
            Ok(issue) => {
                self.form = None;
                self.say(&format!("made {}", issue.key), Role::Good);
                self.detail = Some(Detail { issue, ..Default::default() });
                self.view = View::Issue;
            }
            Err(e) => self.say(&e, Role::Bad),
        }
    }

    fn selected(&self) -> Option<&Issue> {
        match self.view {
            View::Issue => self.detail.as_ref().map(|d| &d.issue),
            _ => self.issues.get(self.cursor),
        }
    }

    /// A key on the page.
    pub fn key(&mut self, key: Key) -> Action {
        match self.view {
            View::Form => self.form_key(key),
            View::List | View::Issue => {
                if let Some(a) = self.issue_keys(key) {
                    return a;
                }
                if self.view == View::Issue {
                    self.detail_key(key)
                } else {
                    self.list_key(key)
                }
            }
        }
    }

    /// The keys an issue answers to, in the list or open.
    fn issue_keys(&mut self, key: Key) -> Option<Action> {
        let Key::Char(c) = key else { return None };
        let issue = self.selected()?.clone();
        Some(match c {
            'o' if !issue.url.is_empty() => Action::Browser(issue.url),
            'y' => Action::Copy(issue.key.key(), "ref"),
            'Y' if !issue.url.is_empty() => Action::Copy(issue.url, "address"),
            'b' => Action::Branch(issue),
            'a' => Action::Agenda(issue),
            's' => Action::Moves(issue),
            'C' => Action::Comment(issue),
            'e' => Action::EditTitle(issue),
            'E' => Action::EditBody(issue),
            'l' => Action::EditLabels(issue),
            'A' => Action::EditAssignees(issue),
            _ => return None,
        })
    }

    fn list_key(&mut self, key: Key) -> Action {
        let last = self.issues.len().saturating_sub(1);
        match key {
            Key::Char('j') | Key::Down => {
                if self.cursor < last {
                    self.cursor += 1;
                    return Action::Redraw;
                }
                match (&self.next, self.loading, self.search().cloned()) {
                    (Some(next), false, Some(search)) => {
                        self.loading = true;
                        Action::Load { search, more: Some(next.clone()) }
                    }
                    _ => Action::None,
                }
            }
            Key::Char('k') | Key::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                Action::Redraw
            }
            Key::PageDown | Key::Char('J') => {
                self.cursor = (self.cursor + self.shown.max(1)).min(last);
                Action::Redraw
            }
            Key::PageUp | Key::Char('K') => {
                self.cursor = self.cursor.saturating_sub(self.shown.max(1));
                Action::Redraw
            }
            Key::Char('g') | Key::Home => {
                self.cursor = 0;
                Action::Redraw
            }
            Key::Char('G') | Key::End => {
                self.cursor = last;
                Action::Redraw
            }
            Key::Enter => self.issues.get(self.cursor).map_or(Action::None, |i| Action::Open(i.key.clone())),
            Key::Tab | Key::BackTab if !self.searches.is_empty() => {
                let n = self.searches.len();
                self.current = if key == Key::Tab { (self.current + 1) % n } else { (self.current + n - 1) % n };
                self.issues.clear();
                self.next = None;
                self.load()
            }
            Key::Char(c @ '1'..='9') if (c as usize - '1' as usize) < self.searches.len() => {
                self.current = c as usize - '1' as usize;
                self.issues.clear();
                self.load()
            }
            Key::Char('/') => Action::Query,
            Key::Char('n') => Action::NewIssue,
            Key::Char('u') => self.load(),
            Key::Char('q') | Key::Escape => Action::Close,
            _ => Action::None,
        }
    }

    fn detail_key(&mut self, key: Key) -> Action {
        let Some(d) = &mut self.detail else { return Action::None };
        match key {
            Key::Char('j') | Key::Down => d.scroll += 1,
            Key::Char('k') | Key::Up => d.scroll = d.scroll.saturating_sub(1),
            Key::PageDown | Key::Char('J') | Key::Char(' ') => d.scroll += 20,
            Key::PageUp | Key::Char('K') => d.scroll = d.scroll.saturating_sub(20),
            Key::Char('g') | Key::Home => d.scroll = 0,
            Key::Char('u') => return Action::Open(d.issue.key.clone()),
            Key::Char('n') => return Action::NewIssue,
            Key::Char('q') | Key::Escape | Key::Backspace => {
                self.view = View::List;
            }
            _ => return Action::None,
        }
        Action::Redraw
    }

    fn form_key(&mut self, key: Key) -> Action {
        let Some(f) = &mut self.form else { return Action::None };
        if f.sending {
            return Action::None;
        }
        let armed = std::mem::take(&mut f.armed);
        let value = |f: &Form, field: Field| match field {
            Field::Project => f.new.project.clone(),
            Field::Title => f.new.title.clone(),
            Field::Body => f.new.body.clone(),
            Field::Labels => f.new.labels.join(", "),
            Field::Assignees => f.new.assignees.join(", "),
            Field::Template => String::new(),
        };
        match key {
            Key::Char('j') | Key::Down | Key::Tab => f.field = (f.field + 1) % FIELDS.len(),
            Key::Char('k') | Key::Up | Key::BackTab => f.field = (f.field + FIELDS.len() - 1) % FIELDS.len(),
            Key::Enter => {
                let field = FIELDS[f.field];
                return if field == Field::Template { Action::FormTemplate } else { Action::FormEdit(field, value(f, field)) };
            }
            Key::Char('e') => return Action::FormEdit(Field::Body, f.new.body.clone()),
            Key::Char('t') => return Action::FormTemplate,
            Key::Char('S') if armed => {
                if f.new.title.trim().is_empty() {
                    self.say("a new issue needs a title -- Enter on Title", Role::Warn);
                    return Action::Redraw;
                }
                if IssueRef::parse(&format!("{}#1", f.new.project), self.host, "").is_none() {
                    let what = self.host.place();
                    self.say(&format!("which {what}? -- Enter on {}", if what == "project" { "Project" } else { "Repository" }), Role::Warn);
                    return Action::Redraw;
                }
                f.sending = true;
                let new = f.new.clone();
                self.say("sending...", Role::Muted);
                return Action::Submit(new);
            }
            Key::Char('S') => {
                f.armed = true;
                self.say("S again sends it", Role::Accent);
            }
            Key::Char('q') | Key::Escape => {
                self.form = None;
                self.view = if self.detail.is_some() { View::Issue } else { View::List };
                self.status = None;
                return Action::CancelForm;
            }
            _ => return Action::None,
        }
        Action::Redraw
    }

    // -- Drawing ------------------------------------------------------

    /// The page, `width` cells by `height` lines.
    pub fn render(&mut self, width: usize, height: usize) -> Canvas {
        let (w, h) = (width.max(30), height.max(8));
        let mut c = Canvas::new();
        let mut x = c.put(0, 0, &self.title(), Role::Title);
        if self.view != View::Form {
            for (i, s) in self.searches.iter().enumerate() {
                if x + 4 >= w {
                    break;
                }
                let role = if i == self.current { Role::Accent } else { Role::Muted };
                x += 2;
                x = c.put_fit(x, 0, w - x, &format!("{} {}", i + 1, clean::name(&s.label)), role);
            }
        } else {
            c.put_fit(x, 0, w - x, "  · a new issue", Role::Muted);
        }
        c.rule(1, 0..w);
        let body = 2..h - 2;
        match (self.view, &self.not_ready) {
            (View::List, Some(why)) => {
                c.put_fit(0, 2, w, &clean::message(why), Role::Warn);
            }
            (View::List, None) => self.render_list(&mut c, w, body),
            (View::Issue, _) => self.render_issue(&mut c, w, body),
            (View::Form, _) => self.render_form(&mut c, w, body),
        }
        if let Some((s, role)) = &self.status {
            c.put_fit(0, h - 2, w, s, role.clone());
        } else if self.loading {
            c.put_fit(0, h - 2, w, "asking...", Role::Muted);
        }
        let keys = match self.view {
            View::List => "Enter open  Tab search  / query  n new  s move  b branch  o browser  y copy  u again  q close",
            View::Issue => "C comment  e title  E body  l labels  A assignees  s move  b branch  a agenda  o browser  y Y copy  q back",
            View::Form => "Enter edit  e body  t template  S S send  q cancel",
        };
        c.put_fit(0, h - 1, w, keys, Role::Muted);
        c
    }

    fn render_list(&mut self, c: &mut Canvas, w: usize, rows: std::ops::Range<usize>) {
        let n = rows.len();
        self.shown = n;
        if self.issues.is_empty() {
            let what = if self.loading { "" } else { "nothing here" };
            c.put_fit(0, rows.start, w, what, Role::Muted);
            return;
        }
        let first = self.cursor.saturating_sub(n.saturating_sub(1));
        let key_w = self.issues.iter().map(|i| text::width(&i.key.key())).max().unwrap_or(0).min(w / 3);
        for (row, issue) in self.issues.iter().enumerate().skip(first).take(n) {
            let y = rows.start + row - first;
            let mut x = c.put_fit(0, y, key_w, &issue.key.key(), Role::Accent);
            x = x.max(key_w) + 2;
            let state = if issue.open { String::new() } else { format!("  {}", issue.state_label()) };
            let labels = if issue.labels.is_empty() { String::new() } else { format!("  {}", issue.labels.iter().map(|l| clean::name(l)).collect::<Vec<_>>().join(" · ")) };
            if x < w {
                x = c.put_fit(x, y, w - x, &clean::line(&issue.title, clean::TITLE), Role::Text);
            }
            if x < w {
                x = c.put_fit(x, y, w - x, &state, Role::Warn);
            }
            if x < w {
                c.put_fit(x, y, w - x, &labels, Role::Muted);
            }
            if row == self.cursor {
                c.focus(y, 0..w);
            }
        }
        if self.next.is_some() && first + n >= self.issues.len() {
            c.put_fit(0, rows.end.saturating_sub(1), w, "j at the end for more", Role::Muted);
        }
    }

    /// An issue's lines, all of them, to scroll through.
    fn issue_lines(&self, d: &Detail, w: usize) -> Vec<(String, Role)> {
        let i = &d.issue;
        let mut out: Vec<(String, Role)> = Vec::new();
        for l in text::wrap(&clean::line(&i.title, clean::TITLE), w) {
            out.push((l, Role::Title));
        }
        let mut meta = format!("{}  ·  {}", i.key, i.state_label());
        if !i.author.is_empty() {
            meta.push_str(&format!("  ·  opened by {}", clean::name(&i.author)));
        }
        meta.push_str(&format!("  ·  updated {}", clean::stamp(&i.updated)));
        out.push((meta, Role::Muted));
        let list = |v: Vec<String>| v.iter().map(|s| clean::name(s)).collect::<Vec<_>>().join(", ");
        let assignees = list(i.assignees.iter().map(|a| if a.name != a.username && !a.name.is_empty() { format!("{} ({})", a.name, a.username) } else { a.username.clone() }).collect());
        out.push((format!("Assignees  {}", if assignees.is_empty() { "nobody".into() } else { assignees }), Role::Text));
        out.push((format!("Labels     {}", if i.labels.is_empty() { "none".into() } else { list(i.labels.clone()) }), Role::Text));
        if let Some(m) = &i.milestone {
            out.push((format!("Milestone  {}", clean::name(m)), Role::Text));
        }
        if let Some(d) = &i.due {
            out.push((format!("Due        {}", clean::name(d)), Role::Text));
        }
        if let Some(wt) = i.weight {
            out.push((format!("Weight     {wt}"), Role::Text));
        }
        if i.spent.is_some() || i.estimate.is_some() {
            let h = |s: Option<u64>| s.map_or("none".to_string(), |s| fenix_forge::issues::gitlab::duration(s as i64));
            out.push((format!("Time       {} spent of {} estimated", h(i.spent), h(i.estimate)), Role::Text));
        }
        for l in &d.linked {
            out.push((format!("{}  {}  {} -- {}", if self.host == Host::GitHub { "Pull" } else { "MR  " }, clean::name(&l.reference), clean::name(&l.state), clean::line(&l.title, clean::TITLE)), Role::Accent));
        }
        out.push((String::new(), Role::Text));
        if i.body.trim().is_empty() {
            out.push(("No description.".into(), Role::Muted));
        } else {
            out.extend(markdown::lines(&i.body, w));
        }
        let people: Vec<&Note> = d.notes.iter().filter(|n| !n.system).collect();
        if !d.notes.is_empty() {
            out.push((String::new(), Role::Text));
            out.push((format!("{} comment(s)", people.len()), Role::Title));
        }
        for n in &d.notes {
            out.push((String::new(), Role::Text));
            if n.system {
                out.push((format!("{} {}  ·  {}", clean::name(&n.author), clean::line(&n.body, clean::MESSAGE), clean::stamp(&n.created)), Role::Muted));
                continue;
            }
            out.push((format!("{}  ·  {}", clean::name(&n.author), clean::stamp(&n.created)), Role::Accent));
            out.extend(markdown::lines(&n.body, w.saturating_sub(2)).into_iter().map(|(l, r)| (format!("  {l}"), r)));
        }
        out
    }

    fn render_issue(&mut self, c: &mut Canvas, w: usize, rows: std::ops::Range<usize>) {
        let Some(d) = &self.detail else { return };
        let lines = self.issue_lines(d, w);
        let most = lines.len().saturating_sub(rows.len());
        let scroll = d.scroll.min(most);
        if let Some(d) = &mut self.detail {
            d.scroll = scroll;
        }
        for (k, (line, role)) in lines.iter().skip(scroll).take(rows.len()).enumerate() {
            c.put_fit(0, rows.start + k, w, line, role.clone());
        }
    }

    fn render_form(&self, c: &mut Canvas, w: usize, rows: std::ops::Range<usize>) {
        let Some(f) = &self.form else { return };
        let place = if self.host == Host::GitHub { "Repository" } else { "Project" };
        let template = f.template.and_then(|t| f.templates.get(t)).map(|t| clean::name(&t.name));
        let template = match (template, f.templates.len()) {
            (Some(t), _) => t,
            (None, 0) => "none here".into(),
            (None, n) => format!("{n} to choose from"),
        };
        let body_first = f.new.body.lines().find(|l| !l.trim().is_empty()).map(|l| clean::line(l, 200)).unwrap_or_default();
        let lines_n = f.new.body.lines().count();
        let fields: [(&str, String); 6] = [
            (place, f.new.project.clone()),
            ("Title", f.new.title.clone()),
            ("Description", if lines_n > 1 { format!("{body_first}  (+{} lines)", lines_n - 1) } else { body_first }),
            ("Labels", f.new.labels.join(", ")),
            ("Assignees", f.new.assignees.join(", ")),
            ("Template", template),
        ];
        for (k, (label, value)) in fields.iter().enumerate() {
            let y = rows.start + k * 2;
            if y >= rows.end {
                break;
            }
            let x = c.put(0, y, &format!("{label:<12}"), Role::Muted);
            c.put_fit(x + 1, y, w.saturating_sub(x + 1), &clean::line(value, clean::TITLE), if value.is_empty() { Role::Muted } else { Role::Text });
            if k == f.field {
                c.focus(y, 0..w);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_forge::issues::User;

    fn issue(project: &str, n: u64, title: &str) -> Issue {
        Issue { key: IssueRef::new(project, n), title: title.into(), open: true, url: format!("https://github.com/{project}/issues/{n}"), updated: "2026-11-13T16:05:09Z".into(), ..Default::default() }
    }

    fn page() -> Page {
        let mut p = Page::new(Host::GitHub, searches(Host::GitHub, &["acme/rocket".into()], &[("Bugs".into(), "label:bug".into())]));
        let a = p.load();
        assert_eq!(a, Action::Load { search: Search::Assigned, more: None });
        p.listed(&Search::Assigned, false, Ok((vec![issue("acme/rocket", 12, "Login\u{1b}[2J times out"), issue("acme/web", 3, "Header")], Some("2".into()))));
        p
    }

    #[test]
    fn the_searches_are_yours_then_the_repositorys_then_your_queries() {
        let s = searches(Host::GitHub, &["acme/rocket".into(), "Acme/Rocket".into(), "acme/web".into()], &[("Bugs".into(), "label:bug".into()), ("".into(), "x".into())]);
        assert_eq!(s.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Assigned to me", "Created by me", "Mentioning me", "acme/rocket", "acme/web", "Bugs"]);
    }

    #[test]
    fn the_list_moves_opens_and_loads_more_at_its_end() {
        let mut p = page();
        let shown = p.render(100, 12).text();
        assert!(shown.contains("acme/rocket#12  Login[2J times out"), "{shown}");
        assert!(shown.contains("1 Assigned to me  2 Created by me"));
        assert_eq!(p.key(Key::Enter), Action::Open(IssueRef::new("acme/rocket", 12)));
        assert_eq!(p.key(Key::Char('j')), Action::Redraw);
        assert_eq!(p.key(Key::Char('j')), Action::Load { search: Search::Assigned, more: Some("2".into()) });
        assert_eq!(p.key(Key::Char('j')), Action::None, "already asking");
        p.listed(&Search::Assigned, true, Ok((vec![issue("acme/rocket", 14, "More")], None)));
        assert_eq!(p.issues.len(), 3);
        assert_eq!(p.key(Key::Char('y')), Action::Copy("acme/web#3".into(), "ref"));
        assert_eq!(p.key(Key::Char('o')), Action::Browser("https://github.com/acme/web/issues/3".into()));
        assert!(matches!(p.key(Key::Char('b')), Action::Branch(i) if i.key.number == 3));
        assert_eq!(p.key(Key::Tab), Action::Load { search: Search::Created, more: None });
        assert!(p.issues.is_empty());
        // An answer for a search no longer shown is dropped.
        p.listed(&Search::Assigned, false, Ok((vec![issue("a/b", 1, "late")], None)));
        assert!(p.issues.is_empty());
        assert_eq!(p.key(Key::Char('/')), Action::Query);
        assert_eq!(p.query("label:bug is:open"), Action::Load { search: Search::Query("label:bug is:open".into()), more: None });
        assert_eq!(p.searches.last().unwrap().label, "\"label:bug is:open\"");
        assert_eq!(p.key(Key::Char('q')), Action::Close);
    }

    #[test]
    fn an_issue_shows_its_fields_body_and_comments_and_its_keys_ask_first() {
        let mut p = page();
        let mut i = issue("acme/rocket", 12, "Login times out");
        i.body = "# Steps\nLog in.\n```\nlet x = 1;\n```".into();
        i.assignees = vec![User { username: "octocat".into(), name: "Mona".into(), id: String::new() }];
        i.labels = vec!["bug".into()];
        i.milestone = Some("v2.0".into());
        i.author = "hubot".into();
        let notes = vec![Note { id: "1".into(), author: "kim".into(), body: "Seen it\u{202e} too".into(), created: "2026-11-02".into(), system: false }, Note { id: "2".into(), author: "jo".into(), body: "added ~bug".into(), created: "2026-11-03".into(), system: true }];
        let linked = vec![Linked { reference: "acme/rocket#20".into(), title: "Shorter timer".into(), url: String::new(), state: "merged".into() }];
        p.opened(Ok(Detail { issue: i, notes, linked, scroll: 0 }));
        assert_eq!(p.view, View::Issue);
        let shown = p.render(100, 40).text();
        for want in ["Login times out", "acme/rocket#12  ·  open  ·  opened by hubot", "Assignees  Mona (octocat)", "Labels     bug", "Milestone  v2.0", "Pull  acme/rocket#20  merged -- Shorter timer", "# Steps", "let x = 1;", "1 comment(s)", "kim  ·  2026-11-02", "  Seen it too", "jo added ~bug"] {
            assert!(shown.contains(want), "{want}:\n{shown}");
        }
        for (k, want) in [('C', "Comment"), ('e', "EditTitle"), ('E', "EditBody"), ('l', "EditLabels"), ('A', "EditAssignees"), ('s', "Moves"), ('a', "Agenda")] {
            assert!(format!("{:?}", p.key(Key::Char(k))).starts_with(want), "{k}");
        }
        assert_eq!(p.key(Key::Char('Y')), Action::Copy("https://github.com/acme/rocket/issues/12".into(), "address"));
        assert_eq!(p.key(Key::Char('u')), Action::Open(IssueRef::new("acme/rocket", 12)));
        assert_eq!(p.key(Key::Char('q')), Action::Redraw);
        assert_eq!(p.view, View::List);
    }

    #[test]
    fn the_form_takes_fields_a_template_and_sends_only_on_s_s() {
        let mut p = page();
        p.open_form(NewIssue { project: "acme/rocket".into(), title: "From the agenda".into(), ..Default::default() });
        p.set_templates(vec![Template { name: "Bug report".into(), title: "[Bug] ".into(), body: "## What happened\n".into(), labels: vec!["bug".into()], assignees: vec![] }]);
        assert_eq!(p.key(Key::Enter), Action::FormEdit(Field::Project, "acme/rocket".into()));
        p.key(Key::Char('j'));
        assert_eq!(p.key(Key::Enter), Action::FormEdit(Field::Title, "From the agenda".into()));
        p.set_field(Field::Labels, "triage, @x ,");
        p.set_field(Field::Assignees, "@octocat");
        p.use_template(0);
        let f = p.form.clone().unwrap();
        assert_eq!((f.new.title.as_str(), f.new.body.as_str(), f.new.labels.clone(), f.new.assignees.clone()), ("From the agenda", "## What happened\n", vec!["triage".to_string(), "x".into(), "bug".into()], vec!["octocat".to_string()]));
        let shown = p.render(100, 20).text();
        assert!(shown.contains("Template     Bug report") && shown.contains("Labels       triage, x, bug"), "{shown}");
        assert_eq!(p.key(Key::Char('S')), Action::Redraw, "armed, not sent");
        assert_eq!(p.key(Key::Char('j')), Action::Redraw);
        assert_eq!(p.key(Key::Char('S')), Action::Redraw, "another key in between disarms it");
        assert!(matches!(p.key(Key::Char('S')), Action::Submit(n) if n.title == "From the agenda"));
        assert_eq!(p.key(Key::Char('S')), Action::None, "sending: nothing more");
        p.submitted(Err("HTTP 422 (Unprocessable Entity): Validation Failed".into()));
        assert_eq!(p.view, View::Form, "kept to try again");
        p.set_field(Field::Project, "not a repo");
        p.key(Key::Char('S'));
        assert_eq!(p.key(Key::Char('S')), Action::Redraw, "which repository?");
        assert_eq!(p.status.as_ref().unwrap().0, "which repository? -- Enter on Repository");
        assert_eq!(p.key(Key::Escape), Action::CancelForm);
        assert_eq!(p.view, View::List);
    }

    #[test]
    fn it_lays_out_at_any_size() {
        let mut p = page();
        for (w, h) in [(0, 0), (10, 3), (80, 24), (300, 80)] {
            assert!(p.render(w, h).height() >= 3, "{w}x{h}");
        }
        p.view = View::Form;
        p.open_form(NewIssue::default());
        p.render(10, 3);
    }
}
