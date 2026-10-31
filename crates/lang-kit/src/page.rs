//! The package manager page: the project's dependencies, the vulnerable
//! and outdated first, and keys to upgrade, add and remove them, each run
//! as a task.

use std::borrow::Cow;
use std::collections::VecDeque;

use fenix_extension_api as api;
use fenix_extension_api::{Command, Event, PickerItem};
use fenix_ui_kit::{Canvas, Column, Fill, Key, Mouse, Outcome, Popup, Rect, Role, Style, Tab, TableSource, TableState, TabsState, Width};

use crate::exec::{Output, Runs};
use crate::host::Host;
use crate::packages::{cache_key, merge, Cached, Dependency, Finding, PackageManager, Risk, Update};
use crate::version::Bump;

/// The page's kind, for `page::open` and `page_restore`.
pub const KIND: &str = "packages";

/// How long an audit is kept before it's run again on a refresh: they're
/// slow, and ask the network. An action runs it again at once.
pub const AUDIT_KEPT: u64 = 60 * 60;

/// What a program runs for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    List,
    Outdated,
    Audit,
    Search(String),
}

/// How one of `list`, `outdated` and `audit` is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Not run: the manager has none, or the audit's kept.
    Idle,
    Running,
    Done,
    Failed(String),
}

/// Which rows the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    All,
    Outdated,
    Vulnerable,
}

const SHOWS: [Show; 3] = [Show::All, Show::Outdated, Show::Vulnerable];

/// What's asked before it's done.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirm {
    Remove(Dependency),
    UpgradeAll,
}

/// The package manager page, for one manager.
pub struct PackagesPage {
    pub manager: Box<dyn PackageManager>,
    /// Its page, while it's open.
    pub page: Option<u64>,
    /// Where the cache is kept; none keeps none.
    pub cache_dir: Option<String>,
    show: Show,
    table: TableState,
    tabs: TabsState,
    runs: Runs<Job>,
    pub list: Step,
    pub outdated: Step,
    pub audit: Step,
    /// What each said last.
    listed: Vec<Dependency>,
    updates: Vec<Update>,
    findings: Vec<Finding>,
    listed_at: Option<u64>,
    audited_at: Option<u64>,
    key: String,
    /// The list shown: the three merged.
    deps: Vec<Dependency>,
    /// Actions waiting to run as tasks (one runs at a time), each its
    /// steps; and the one running, with the steps it has left.
    queue: VecDeque<(String, VecDeque<Command>)>,
    task: Option<(u64, String, VecDeque<Command>)>,
    confirm: Option<Confirm>,
    adding: Option<u64>,
    picking: Option<u64>,
    /// A line under the table: what happened last.
    message: Option<(String, Role)>,
}

impl PackagesPage {
    pub fn new(manager: Box<dyn PackageManager>) -> PackagesPage {
        let mut table = TableState::new();
        table.empty = "No dependencies".into();
        PackagesPage {
            manager,
            page: None,
            cache_dir: Some("/data/lang-kit".into()),
            show: Show::All,
            table,
            tabs: TabsState::new(),
            runs: Runs::new(),
            list: Step::Idle,
            outdated: Step::Idle,
            audit: Step::Idle,
            listed: Vec::new(),
            updates: Vec::new(),
            findings: Vec::new(),
            listed_at: None,
            audited_at: None,
            key: String::new(),
            deps: Vec::new(),
            queue: VecDeque::new(),
            task: None,
            confirm: None,
            adding: None,
            picking: None,
            message: None,
        }
    }

    /// The dependencies shown, in order.
    pub fn deps(&self) -> &[Dependency] {
        &self.deps
    }

    pub fn show(&self) -> Show {
        self.show
    }

    /// Opens the page, or refreshes it when it's open.
    pub fn open(&mut self, host: &mut dyn Host) {
        match self.page {
            Some(p) => {
                host.refresh(p);
            }
            None => {
                let title = format!("Packages · {}", self.manager.name());
                let p = host.open_page(KIND, &title);
                self.attach(host, p);
            }
        }
    }

    /// It's page `page` (opened, or restored), its list read again.
    pub fn attach(&mut self, host: &mut dyn Host, page: u64) {
        self.page = Some(page);
        self.load_cache();
        self.refresh(host, false);
    }

    /// Its page closed.
    pub fn closed(&mut self, host: &mut dyn Host, page: u64) {
        if self.page == Some(page) {
            self.page = None;
            self.runs.kill_all(host);
        }
    }

    /// What's needed to open it again: the rows it shows.
    pub fn state(&self) -> String {
        format!("{}", SHOWS.iter().position(|s| *s == self.show).unwrap_or(0))
    }

    pub fn restore(&mut self, host: &mut dyn Host, page: u64, state: &str) {
        let at = state.trim().parse::<usize>().unwrap_or(0).min(SHOWS.len() - 1);
        self.show = SHOWS[at];
        self.tabs.active = at;
        self.attach(host, page);
    }

    fn cache_file(&self) -> Option<String> {
        self.cache_dir.as_ref().map(|d| format!("{d}/packages-{}.json", self.key))
    }

    /// What was listed last for the lockfiles as they are, if anything.
    fn load_cache(&mut self) {
        self.key = cache_key(self.manager.as_ref());
        let Some(file) = self.cache_file() else { return };
        let Ok(bytes) = std::fs::read(&file) else { return };
        let Ok(c) = serde_json::from_slice::<Cached>(&bytes) else { return };
        if c.key != self.key {
            return;
        }
        self.listed = c.deps;
        self.updates = c.updates;
        self.findings = c.findings;
        self.listed_at = Some(c.listed);
        self.audited_at = c.audited;
        self.remerge();
    }

    fn save_cache(&self) {
        let (Some(file), Some(dir), Some(listed)) = (self.cache_file(), self.cache_dir.as_ref(), self.listed_at) else { return };
        let c = Cached { key: self.key.clone(), listed, audited: self.audited_at, deps: self.listed.clone(), updates: self.updates.clone(), findings: self.findings.clone() };
        if let Ok(json) = serde_json::to_vec(&c) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(file, json);
        }
    }

    /// Lists, finds what's outdated, and audits -- the audit only when
    /// it's older than [`AUDIT_KEPT`] or `audit` says.
    pub fn refresh(&mut self, host: &mut dyn Host, audit: bool) {
        self.runs.kill_where(host, |j| !matches!(j, Job::Search(_)));
        let key = cache_key(self.manager.as_ref());
        if key != self.key {
            // The lockfile changed: what was found is about another state.
            self.key = key;
            self.audited_at = None;
        }
        let now = host.now();
        let list = self.manager.list();
        self.list = self.start(host, &list, Job::List);
        self.outdated = match self.manager.outdated() {
            Some(c) => self.start(host, &c, Job::Outdated),
            None => Step::Idle,
        };
        let kept = !audit && self.audited_at.is_some_and(|t| now.saturating_sub(t) < AUDIT_KEPT);
        self.audit = match self.manager.audit() {
            Some(_) if kept => Step::Done,
            Some(c) => self.start(host, &c, Job::Audit),
            None => Step::Idle,
        };
        self.redraw(host);
    }

    fn start(&mut self, host: &mut dyn Host, cmd: &Command, job: Job) -> Step {
        match self.runs.start(host, cmd, job) {
            Ok(_) => Step::Running,
            Err(why) => Step::Failed(format!("{}: {why}", cmd.program)),
        }
    }

    fn redraw(&self, host: &mut dyn Host) {
        if let Some(p) = self.page {
            host.refresh(p);
        }
    }

    fn remerge(&mut self) {
        self.deps = merge(self.manager.as_ref(), self.listed.clone(), &self.updates, &self.findings);
        self.table.refit();
    }

    /// An event for it: a program's output or end, a task's end, what was
    /// typed or picked. Whether it was its.
    pub fn handle(&mut self, host: &mut dyn Host, event: &Event) -> bool {
        if self.runs.owns(event) {
            if let Some((job, out)) = self.runs.event(event) {
                self.finished(host, job, out);
            }
            return true;
        }
        match event {
            Event::TaskFinished((id, ok)) if self.task.as_ref().is_some_and(|(t, _, _)| t == id) => {
                let Some((_, name, rest)) = self.task.take() else { return true };
                // Its next step, if this one went well.
                if *ok && !rest.is_empty() {
                    self.queue.push_front((name, rest));
                } else {
                    self.message = Some(if *ok { (format!("{name}: done"), Role::Good) } else { (format!("{name}: failed (the task pane says why)"), Role::Bad) });
                }
                if !self.next_task(host) {
                    self.refresh(host, true);
                }
                true
            }
            Event::Prompted((id, text)) if self.adding == Some(*id) => {
                self.adding = None;
                if let Some(text) = text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    match self.manager.search(text) {
                        Some(cmd) => {
                            if let Err(why) = self.runs.start(host, &cmd, Job::Search(text.to_string())) {
                                self.say(host, format!("{}: {why}", cmd.program), Role::Bad);
                            }
                        }
                        None => self.add(host, text),
                    }
                }
                true
            }
            Event::Picked((id, value)) if self.picking == Some(*id) => {
                self.picking = None;
                if let Some(v) = value {
                    self.add(host, v);
                }
                true
            }
            _ => false,
        }
    }

    fn finished(&mut self, host: &mut dyn Host, job: Job, out: Output) {
        let fail = |why: String, out: &Output| if out.ok() || out.why() == why { Step::Failed(why) } else { Step::Failed(format!("{why} ({})", out.why())) };
        let now = host.now();
        match job {
            Job::List => match self.manager.parse_list(&out) {
                Ok(deps) => {
                    self.listed = deps;
                    self.listed_at = Some(now);
                    self.list = Step::Done;
                }
                Err(why) => self.list = fail(why, &out),
            },
            Job::Outdated => match self.manager.parse_outdated(&out) {
                Ok(u) => {
                    self.updates = u;
                    self.outdated = Step::Done;
                }
                Err(why) => self.outdated = fail(why, &out),
            },
            Job::Audit => match self.manager.parse_audit(&out) {
                Ok(f) => {
                    self.findings = f;
                    self.audited_at = Some(now);
                    self.audit = Step::Done;
                }
                Err(why) => self.audit = fail(why, &out),
            },
            Job::Search(query) => {
                match self.manager.parse_search(&out) {
                    Ok(found) => {
                        let mut items = vec![PickerItem::new(query.clone(), query.clone()).detail("as typed")];
                        items.extend(found.into_iter().filter(|f| f.name != query).map(|f| PickerItem::new(f.name.clone(), f.name).detail(format!("{}  {}", f.version, f.description).trim().to_string())));
                        self.picking = Some(host.pick(&format!("Add with {}", self.manager.name()), &items));
                    }
                    Err(why) => self.say(host, format!("Search: {why}"), Role::Bad),
                }
                return;
            }
        }
        self.remerge();
        if self.runs.is_empty() {
            self.save_cache();
        }
        self.redraw(host);
    }

    fn say(&mut self, host: &mut dyn Host, text: String, role: Role) {
        self.message = Some((text, role));
        self.redraw(host);
    }

    /// The rows shown, as indexes into `deps`.
    fn rows(&self) -> Vec<usize> {
        self.deps
            .iter()
            .enumerate()
            .filter(|(_, d)| match self.show {
                Show::All => true,
                Show::Outdated => d.is_outdated(),
                Show::Vulnerable => d.is_vulnerable(),
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The dependency the cursor's on.
    pub fn focused(&self) -> Option<&Dependency> {
        let rows = self.rows();
        rows.get(self.table.row.min(rows.len().saturating_sub(1))).map(|&i| &self.deps[i])
    }

    /// An action's steps queued, or why it can't be done said.
    fn queue_steps(&mut self, host: &mut dyn Host, what: String, steps: crate::packages::Steps) {
        match steps {
            Ok(steps) if steps.is_empty() => self.say(host, "Nothing to do".into(), Role::Muted),
            Ok(steps) => {
                self.queue.push_back((what, steps.into()));
                if self.task.is_none() {
                    self.next_task(host);
                }
                self.redraw(host);
            }
            Err(why) => self.say(host, why, Role::Warn),
        }
    }

    /// The next step started; false when there's none.
    fn next_task(&mut self, host: &mut dyn Host) -> bool {
        while let Some((what, mut steps)) = self.queue.pop_front() {
            let Some(cmd) = steps.pop_front() else { continue };
            let name = format!("{}: {what}", self.manager.name());
            match host.run_task(&name, &cmd) {
                Ok(id) => {
                    self.task = Some((id, what.clone(), steps));
                    self.message = Some((format!("{what}…"), Role::Accent));
                    return true;
                }
                Err(why) => self.message = Some((format!("{what}: {why}"), Role::Bad)),
            }
        }
        false
    }

    fn add(&mut self, host: &mut dyn Host, package: &str) {
        let steps = self.manager.add_steps(package);
        self.queue_steps(host, format!("add {package}"), steps);
    }

    /// A key on its page.
    pub fn key(&mut self, host: &mut dyn Host, key: Key) {
        if let Some(confirm) = self.confirm.take() {
            if key == Key::Char('y') || key == Key::Enter {
                match confirm {
                    Confirm::Remove(dep) => {
                        let steps = self.manager.remove_steps(&dep);
                        self.queue_steps(host, format!("remove {}", dep.name), steps);
                    }
                    Confirm::UpgradeAll => {
                        let outdated: Vec<&Dependency> = self.deps.iter().filter(|d| d.is_outdated()).collect();
                        let n = outdated.len();
                        match self.manager.upgrade_all_steps(&outdated) {
                            Ok(steps) if steps.is_empty() => self.say(host, "Nothing to upgrade".into(), Role::Muted),
                            steps => self.queue_steps(host, format!("upgrade {n} package{}", if n == 1 { "" } else { "s" }), steps),
                        }
                    }
                }
            }
            self.redraw(host);
            return;
        }
        let rows = self.rows();
        let moved = self.tabs.handle_key(key, SHOWS.len());
        if moved.handled() {
            self.show = SHOWS[self.tabs.active];
            self.table.row = 0;
            self.redraw(host);
            return;
        }
        match key {
            Key::Char('r') => {
                self.message = None;
                self.refresh(host, true);
            }
            Key::Char('u') => {
                if let Some(dep) = self.focused().cloned() {
                    let steps = self.manager.upgrade_steps(&dep);
                    self.queue_steps(host, format!("upgrade {}", dep.name), steps);
                }
            }
            Key::Char('U') => {
                if self.deps.iter().any(Dependency::is_outdated) {
                    self.confirm = Some(Confirm::UpgradeAll);
                } else {
                    self.message = Some(("Nothing is outdated".into(), Role::Muted));
                }
                self.redraw(host);
            }
            Key::Char('a') => {
                self.adding = Some(host.prompt(&format!("Add with {}", self.manager.name()), ""));
            }
            Key::Char('d') => {
                if let Some(dep) = self.focused().cloned() {
                    if dep.direct {
                        self.confirm = Some(Confirm::Remove(dep));
                    } else {
                        self.message = Some((format!("{} is there for another package: remove that one", dep.name), Role::Warn));
                    }
                    self.redraw(host);
                }
            }
            Key::Enter => {
                if let Some(dep) = self.focused() {
                    host.open_text_beside(&format!("{} {}", dep.name, dep.current), &details(dep), Some("md"));
                }
            }
            key => {
                if self.table.handle_key(key, rows.len(), COLUMNS).handled() {
                    self.redraw(host);
                }
            }
        }
    }

    /// The mouse on its page: a tab clicked shows it, a row clicked is
    /// focused and a double click shows its details, the wheel scrolls.
    /// Fenix draws the page again after.
    pub fn mouse(&mut self, host: &mut dyn Host, m: &Mouse) {
        if self.confirm.is_some() {
            return;
        }
        if self.tabs.mouse(m).handled() {
            self.show = SHOWS[self.tabs.active];
            self.table.row = 0;
            return;
        }
        let rows = self.rows().len();
        if self.table.mouse(m, rows, COLUMNS) == Outcome::Submit {
            self.key(host, Key::Enter);
        }
    }

    /// The page drawn `width` by `height` cells.
    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let all = Rect::new(0, 0, width, height.max(8));
        let (head, rest) = all.split_top(1);
        let x = c.put(head.x, head.y, "Packages", Role::Title);
        let x = c.put(x, head.y, &format!("  {}", self.manager.name()), Role::Muted);
        let steps = [("list", &self.list), ("outdated", &self.outdated), ("audit", &self.audit)];
        let mut sx = x + 3;
        for (name, step) in steps {
            let (mark, role) = match step {
                Step::Idle => continue,
                Step::Running => ("…", Role::Accent),
                Step::Done => ("✓", Role::Good),
                Step::Failed(_) => ("✗", Role::Bad),
            };
            sx = c.put(sx, head.y, &format!("{name} {mark}"), role) + 2;
        }
        let (bar, rest) = rest.split_top(2);
        let count = |s: Show| self.deps.iter().filter(|d| s == Show::All || (s == Show::Outdated && d.is_outdated()) || (s == Show::Vulnerable && d.is_vulnerable())).count();
        let tabs = [Tab::new("All").badge(count(Show::All).to_string()), Tab::new("Outdated").badge(count(Show::Outdated).to_string()), Tab::new("Vulnerable").badge(count(Show::Vulnerable).to_string())];
        self.tabs.render(&tabs, bar, &mut c);

        // At the foot: what went wrong or happened, and the keys.
        let failures: Vec<String> = steps.iter().filter_map(|(n, s)| if let Step::Failed(why) = s { Some(format!("{n}: {why}")) } else { None }).collect();
        let notes = failures.len() + usize::from(self.message.is_some());
        let (foot, body) = rest.split_bottom(notes + 2);
        let rows = self.rows();
        let source = Rows { deps: &self.deps, rows: &rows };
        self.table.render(&columns(), &source, body, &mut c);
        // The vulnerable on red, the outdated on amber: all of the row
        // but the focused cell and the scrollbar.
        let shown = self.table.visible_rows();
        let w = if shown.len() < rows.len() { body.w.saturating_sub(1) } else { body.w };
        for i in shown.clone() {
            let Some(d) = rows.get(i).map(|&r| &self.deps[r]) else { break };
            let fill = if d.is_vulnerable() {
                Fill::Bad
            } else if d.is_outdated() {
                Fill::Warn
            } else {
                continue;
            };
            let y = body.y + 2 + (i - shown.start);
            for x in body.x..body.x + w {
                if c.style_at(x, y).bg.is_none() {
                    c.fill(y, x..x + 1, fill);
                }
            }
        }
        let mut y = foot.y + 1;
        for f in &failures {
            c.put_fit(foot.x, y, foot.w, f, Role::Bad);
            y += 1;
        }
        if let Some((text, role)) = &self.message {
            c.put_fit(foot.x, y, foot.w, text, role.clone());
            y += 1;
        }
        c.set_mouse(true);
        let mut kx = foot.x;
        for (k, what) in [("u", "upgrade"), ("U", "all"), ("a", "add"), ("d", "remove"), ("r", "refresh"), ("⏎", "details")] {
            kx = c.put(kx, y, k, Role::Accent);
            kx = c.put(kx, y, &format!(" {what}   "), Role::Muted);
        }

        if let Some(confirm) = &self.confirm {
            let question = match confirm {
                Confirm::Remove(d) => format!("Remove {}?", d.name),
                Confirm::UpgradeAll => format!("Upgrade {} outdated?", count(Show::Outdated)),
            };
            let line = body.y + 2 + self.table.row.saturating_sub(self.table.visible_rows().start);
            c.popup(Popup {
                line,
                col: 2,
                rows: vec![vec![(question, Role::Title.into())], vec![("y".into(), Role::Accent.into()), (" yes   ".into(), Style::default()), ("n".into(), Role::Accent.into()), (" no".into(), Style::default())]],
            });
        }
        c
    }

    /// The page as Fenix takes it, with its backgrounds and the mouse.
    pub fn page(&self, width: u32, height: u32) -> api::Page {
        self.render(width as usize, height as usize).to_page()
    }
}

const COLUMNS: usize = 6;

fn columns() -> Vec<Column> {
    vec![
        Column::new("Name", Width::Fit { min: 12, max: 40 }),
        Column::fit("Current", 16),
        Column::fit("Wanted", 16),
        Column::fit("Latest", 16),
        Column::fit("Kind", 10),
        Column::new("Advisory", Width::Flex(1)),
    ]
}

fn kind(d: &Dependency) -> String {
    match (&d.group, d.direct) {
        (_, false) => "transitive".into(),
        (Some(g), true) => g.clone(),
        (None, true) => "direct".into(),
    }
}

fn advisory(d: &Dependency) -> String {
    match d.advisories.first() {
        None => String::new(),
        Some(a) if d.advisories.len() == 1 => format!("{} {}", a.id, a.risk.name()),
        Some(a) => format!("{} {} +{}", a.id, a.risk.name(), d.advisories.len() - 1),
    }
}

/// What the table shows: the rows picked, as cells.
struct Rows<'a> {
    deps: &'a [Dependency],
    rows: &'a [usize],
}

impl TableSource for Rows<'_> {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        let d = &self.deps[self.rows[index]];
        let newer = |v: &Option<String>| v.as_ref().filter(|v| **v != d.current).cloned().unwrap_or_default();
        vec![Cow::Borrowed(d.name.as_str()), Cow::Borrowed(d.current.as_str()), newer(&d.wanted).into(), newer(&d.latest).into(), kind(d).into(), advisory(d).into()]
    }

    fn style(&self, row: usize, col: usize) -> Style {
        let d = &self.deps[self.rows[row]];
        Style::new(match col {
            0 if !d.direct => Role::Muted,
            2 if d.wanted.as_ref().is_some_and(|w| crate::version::newer(w, &d.current)) => Role::Accent,
            3 if d.bump() == Bump::Major => Role::Warn,
            3 if d.is_outdated() => Role::Accent,
            4 => Role::Muted,
            5 => match d.risk() {
                Some(Risk::High | Risk::Critical) => Role::Bad,
                Some(_) => Role::Warn,
                None => Role::Text,
            },
            _ => Role::Text,
        })
    }
}

/// A dependency's details, as Markdown: its versions, its advisories.
pub fn details(d: &Dependency) -> String {
    let mut s = format!("# {} {}\n\n", d.name, d.current);
    s.push_str(&format!("- {}\n", kind(d)));
    if let Some(w) = &d.wanted {
        s.push_str(&format!("- wanted: {w}\n"));
    }
    if let Some(l) = &d.latest {
        s.push_str(&format!("- latest: {l}\n"));
    }
    for a in &d.advisories {
        s.push_str(&format!("\n## {} ({})\n\n", a.id, a.risk.name()));
        if !a.title.is_empty() {
            s.push_str(&format!("{}\n\n", a.title));
        }
        if let Some(f) = &a.fixed_in {
            s.push_str(&format!("Fixed in {f}.\n"));
        }
        if let Some(u) = &a.url {
            s.push_str(&format!("{u}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::mock::Mock as Host;
    use crate::packages::tests::{Mock, AUDIT, LIST, OUTDATED};

    fn exit(id: u64, code: i32) -> Event {
        Event::ProcessExit((id, Some(code)))
    }

    fn say(page: &mut PackagesPage, host: &mut Host, id: u64, text: &str) {
        for line in text.lines() {
            page.handle(host, &Event::ProcessOutput(api::ProcessOutput { process: id, stderr: false, line: line.into() }));
        }
    }

    fn opened(audit: bool) -> (PackagesPage, Host) {
        let mut host = Host { now: 1_000_000, ..Host::default() };
        let mut page = PackagesPage::new(Box::new(Mock { audit, search: true, lockfile: None }));
        page.cache_dir = None;
        page.open(&mut host);
        (page, host)
    }

    /// Opened, and each of its programs answered.
    fn answered() -> (PackagesPage, Host) {
        let (mut page, mut host) = opened(true);
        assert_eq!(host.take(), ["page 1 packages Packages · mock", "spawn 2 mock ls --json", "close 2", "spawn 3 mock outdated --json", "close 3", "spawn 4 mock audit --json", "close 4", "refresh 1"]);
        say(&mut page, &mut host, 2, LIST);
        say(&mut page, &mut host, 3, OUTDATED);
        say(&mut page, &mut host, 4, AUDIT);
        assert!(page.handle(&mut host, &exit(2, 0)));
        assert!(page.handle(&mut host, &exit(3, 1)));
        assert!(page.handle(&mut host, &exit(4, 1)));
        host.take();
        (page, host)
    }

    #[test]
    fn the_page_shows_the_vulnerable_and_outdated_first() {
        let (page, _) = answered();
        assert_eq!((&page.list, &page.outdated, &page.audit), (&Step::Done, &Step::Done, &Step::Done));
        // The coloured rows' blanks reach the edge, for their backgrounds.
        let text = page.render(96, 14).text();
        assert_eq!(
            text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n"),
            [
                "Packages  mock   list ✓  outdated ✓  audit ✓",
                "1 All 7   2 Outdated 3   3 Vulnerable 2",
                "",
                "Name          Current  Wanted  Latest  Kind        Advisory",
                "",
                "urllib3       1.26.5                   transitive  PYSEC-2023-212 high +1",
                "jinja2        3.1.2                    transitive  GHSA-h5c8-rqwp-cp95 low",
                "Flask         2.3.0    2.3.3   3.0.3   direct",
                "requests      2.31.0           2.32.3  direct",
                "rich                   13.7.1  13.7.1  direct",
                "pytest        8.0.0                    dev",
                "idna          3.6                      transitive",
                "",
                "u upgrade   U all   a add   d remove   r refresh   ⏎ details",
            ]
            .map(str::trim_end)
            .join("\n")
        );
    }

    #[test]
    fn its_cells_are_coloured_by_what_they_say() {
        let (page, _) = answered();
        let c = page.render(96, 14);
        let at = |line: usize, text: &str| {
            let l = c.line(line);
            let x = fenix_ui_kit::text::width(&l[..l.find(text).unwrap()]);
            c.style_at(x, line).fg
        };
        assert_eq!(at(5, "PYSEC"), Role::Bad);
        assert_eq!(at(6, "GHSA"), Role::Warn);
        assert_eq!(at(7, "3.0.3"), Role::Warn, "a major");
        assert_eq!(at(8, "2.32.3"), Role::Accent);
        assert_eq!(at(0, "list ✓"), Role::Good);
        assert_eq!(c.style_at(0, 5).bg, Some(Fill::Focus), "the focused cell");
        assert_eq!(c.style_at(20, 5).bg, Some(Fill::Bad), "a vulnerable row");
        assert_eq!(c.style_at(95, 7).bg, Some(Fill::Warn), "an outdated one, to the edge");
        assert_eq!(c.style_at(0, 10).bg, None, "a current one");
        assert!(c.mouse());
    }

    #[test]
    fn the_mouse_picks_a_tab_and_a_row() {
        let (mut page, mut host) = answered();
        page.render(96, 14);
        page.mouse(&mut host, &Mouse::click(2, 7));
        assert_eq!(page.focused().map(|d| d.name.as_str()), Some("Flask"));
        let twice = Mouse { action: fenix_ui_kit::MouseAction::Press { button: fenix_ui_kit::Button::Left, clicks: 2 }, ..Mouse::click(2, 8) };
        page.mouse(&mut host, &twice);
        assert_eq!(host.take(), ["text requests 2.31.0"], "a double click shows its details");
        page.mouse(&mut host, &Mouse { action: fenix_ui_kit::MouseAction::Wheel(-1), ..Mouse::click(2, 8) });
        assert_eq!(page.focused().map(|d| d.name.as_str()), Some("idna"), "three rows down");
        let x = page.render(96, 14).line(1).find("Vulnerable").unwrap();
        page.mouse(&mut host, &Mouse::click(x, 1));
        assert_eq!(page.show(), Show::Vulnerable);
    }

    #[test]
    fn tabs_show_the_outdated_or_the_vulnerable() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Char('3'));
        assert_eq!(page.show(), Show::Vulnerable);
        let text = page.render(96, 12).text();
        assert!(text.contains("urllib3") && text.contains("jinja2") && !text.contains("Flask"), "{text}");
        page.key(&mut host, Key::Tab);
        assert_eq!(page.show(), Show::All);
        page.key(&mut host, Key::BackTab);
        assert_eq!(page.show(), Show::Vulnerable);
        assert_eq!(page.state(), "2");
    }

    #[test]
    fn u_upgrades_one_as_a_task_then_it_all_refreshes() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Char('j'));
        page.key(&mut host, Key::Char('j'));
        assert_eq!(page.focused().map(|d| d.name.as_str()), Some("Flask"));
        host.take();
        page.key(&mut host, Key::Char('u'));
        assert_eq!(host.take(), ["task 5 mock: upgrade Flask: mock install Flask@latest", "refresh 1"]);
        assert!(page.render(96, 14).text().contains("upgrade Flask…"));
        // Another while it runs waits its turn.
        page.key(&mut host, Key::Char('j'));
        page.key(&mut host, Key::Char('u'));
        assert_eq!(host.take(), ["refresh 1", "refresh 1"]);
        assert!(page.handle(&mut host, &Event::TaskFinished((5, true))));
        assert_eq!(host.take(), ["task 6 mock: upgrade requests: mock install requests@latest"]);
        // One it can't upgrade on its own is said so.
        page.key(&mut host, Key::Char('g'));
        page.key(&mut host, Key::Char('u'));
        assert!(page.render(96, 14).text().contains("mock can't upgrade urllib3 on its own"));
        host.take();
        assert!(page.handle(&mut host, &Event::TaskFinished((6, false))));
        // Then it's listed and audited again, though the audit was new.
        assert_eq!(host.take(), ["spawn 7 mock ls --json", "close 7", "spawn 8 mock outdated --json", "close 8", "spawn 9 mock audit --json", "close 9", "refresh 1"]);
        assert!(page.render(96, 14).text().contains("upgrade requests: failed (the task pane says why)"));
        assert!(!page.handle(&mut host, &Event::TaskFinished((6, true))), "not its task any more");
    }

    #[test]
    fn a_transitive_one_cant_be_removed_and_a_direct_one_is_asked_first() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Char('d'));
        assert!(page.render(96, 14).text().contains("urllib3 is there for another package"));
        page.key(&mut host, Key::Char('G'));
        page.key(&mut host, Key::Char('k'));
        assert_eq!(page.focused().map(|d| d.name.as_str()), Some("pytest"));
        host.take();
        page.key(&mut host, Key::Char('d'));
        let c = page.render(96, 14);
        let popup = c.popup_ref().expect("asked");
        assert_eq!(popup.rows[0][0].0, "Remove pytest?");
        assert_eq!(popup.line, 10, "beside its row");
        page.key(&mut host, Key::Char('n'));
        assert!(page.render(96, 14).popup_ref().is_none());
        host.take();
        page.key(&mut host, Key::Char('d'));
        page.key(&mut host, Key::Char('y'));
        assert_eq!(host.take(), ["refresh 1", "task 5 mock: remove pytest: mock uninstall pytest", "refresh 1", "refresh 1"]);
    }

    #[test]
    fn shift_u_upgrades_every_outdated_one_once_asked() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Char('U'));
        assert_eq!(page.render(96, 14).popup_ref().map(|p| p.rows[0][0].0.clone()).as_deref(), Some("Upgrade 3 outdated?"));
        host.take();
        page.key(&mut host, Key::Enter);
        assert_eq!(host.take()[0], "task 5 mock: upgrade 3 packages: mock update Flask requests rich");
    }

    #[test]
    fn a_adds_what_a_search_found() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Char('a'));
        assert_eq!(host.take(), ["prompt 5 Add with mock []"]);
        assert!(page.handle(&mut host, &Event::Prompted((5, Some(" http ".into())))));
        assert_eq!(host.take(), ["spawn 6 mock search --json http", "close 6"]);
        say(&mut page, &mut host, 6, "httpx 0.27.0\nhttp 0.1");
        page.handle(&mut host, &exit(6, 0));
        assert_eq!(host.take(), ["pick 7 Add with mock: http, httpx"]);
        assert!(page.handle(&mut host, &Event::Picked((7, Some("httpx".into())))));
        assert_eq!(host.take()[0], "task 8 mock: add httpx: mock install httpx");
        // Nothing typed, nothing added.
        page.key(&mut host, Key::Char('a'));
        page.handle(&mut host, &Event::Prompted((9, None)));
        assert_eq!(host.take(), ["prompt 9 Add with mock []"]);
    }

    #[test]
    fn without_a_search_what_is_typed_is_added() {
        let (mut page, mut host) = answered();
        page.manager = Box::new(Mock { audit: true, search: false, lockfile: None });
        page.key(&mut host, Key::Char('a'));
        host.take();
        page.handle(&mut host, &Event::Prompted((5, Some("rich==13".into()))));
        assert_eq!(host.take()[0], "task 6 mock: add rich==13: mock install rich==13");
    }

    #[test]
    fn a_failure_is_shown_with_what_the_program_said() {
        let (mut page, mut host) = opened(true);
        host.take();
        page.handle(&mut host, &Event::ProcessOutput(api::ProcessOutput { process: 4, stderr: true, line: "error: no lockfile".into() }));
        page.handle(&mut host, &exit(4, 2));
        page.handle(&mut host, &exit(2, 0));
        assert!(matches!(&page.audit, Step::Failed(why) if why.ends_with("(error: no lockfile)")), "{:?}", page.audit);
        assert!(matches!(&page.list, Step::Failed(_)));
        let text = page.render(96, 12).text();
        assert!(text.contains("audit: its output isn't what was expected") && text.contains("list ✗"), "{text}");
    }

    #[test]
    fn a_program_that_isnt_there_is_said_so() {
        let mut host = Host { spawn_fails: true, ..Host::default() };
        let mut page = PackagesPage::new(Box::new(Mock::default()));
        page.cache_dir = None;
        page.open(&mut host);
        assert_eq!(page.list, Step::Failed("mock: mock isn't there".into()));
        assert_eq!(page.audit, Step::Idle, "it has no audit");
    }

    #[test]
    fn the_cache_shows_at_once_and_keeps_the_audit_for_an_hour() {
        let dir = std::env::temp_dir().join(format!("lang-kit-cache-{}", std::process::id()));
        let (mut page, mut host) = answered();
        page.cache_dir = Some(dir.to_string_lossy().into_owned());
        // Answered again, now with somewhere to keep it.
        page.refresh(&mut host, true);
        let ids: Vec<u64> = host.take().iter().filter_map(|c| c.strip_prefix("spawn ")).filter_map(|c| c.split(' ').next()?.parse().ok()).collect();
        for (id, text) in ids.iter().zip([LIST, OUTDATED, AUDIT]) {
            say(&mut page, &mut host, *id, text);
            page.handle(&mut host, &exit(*id, 0));
        }
        let mut again = PackagesPage::new(Box::new(Mock { audit: true, search: true, lockfile: None }));
        again.cache_dir = page.cache_dir.clone();
        host.now += 600;
        again.open(&mut host);
        assert_eq!(again.deps().len(), 7, "shown before anything answers");
        assert_eq!(again.deps()[0].name, "urllib3");
        assert_eq!(again.audit, Step::Done, "not audited again within the hour");
        assert!(!host.take().iter().any(|c| c.contains("audit")));
        let mut later = PackagesPage::new(Box::new(Mock { audit: true, search: true, lockfile: None }));
        later.cache_dir = page.cache_dir.clone();
        host.now += AUDIT_KEPT;
        later.open(&mut host);
        assert!(host.take().iter().any(|c| c.contains("audit")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn enter_shows_its_details_beside() {
        let (mut page, mut host) = answered();
        page.key(&mut host, Key::Enter);
        assert_eq!(host.take(), ["text urllib3 1.26.5"]);
        let d = details(&page.deps()[0]);
        assert!(d.contains("## GHSA-v845-jxx5-vc9f (moderate)") && d.contains("Fixed in 1.26.18."), "{d}");
    }

    #[test]
    fn closing_it_stops_what_runs_and_restoring_lists_again() {
        let (mut page, mut host) = opened(false);
        host.take();
        page.closed(&mut host, 1);
        assert_eq!(host.take(), ["kill 2", "kill 3"]);
        assert_eq!(page.page, None);
        page.restore(&mut host, 9, "1");
        assert_eq!((page.page, page.show()), (Some(9), Show::Outdated));
        assert_eq!(host.take().last().map(String::as_str), Some("refresh 9"));
        let p = page.page(80, 10);
        assert!(p.canvas.is_some_and(|c| c.text.starts_with("Packages")) && p.mouse);
    }

    /// Go's way: a step, then tidying; and one it can't add.
    struct Tidy(Mock);

    impl PackageManager for Tidy {
        fn name(&self) -> &str {
            "go"
        }
        fn list(&self) -> Command {
            self.0.list()
        }
        fn parse_list(&self, out: &Output) -> Result<Vec<Dependency>, String> {
            self.0.parse_list(out)
        }
        fn add(&self, package: &str) -> Command {
            Command::new("go").args(["get", package])
        }
        fn add_steps(&self, package: &str) -> crate::packages::Steps {
            if package.contains(' ') {
                return Err(format!("{package} isn't a module path"));
            }
            Ok(vec![self.add(package), Command::new("go").args(["mod", "tidy"])])
        }
        fn upgrade_all_steps(&self, outdated: &[&Dependency]) -> crate::packages::Steps {
            Ok(outdated.iter().map(|d| Command::new("go").args(["get".to_string(), format!("{}@latest", d.name)])).collect())
        }
    }

    #[test]
    fn an_actions_steps_run_one_after_another_and_stop_at_a_failure() {
        let mut host = Host { now: 1, ..Host::default() };
        let mut page = PackagesPage::new(Box::new(Tidy(Mock::default())));
        page.cache_dir = None;
        page.open(&mut host);
        say(&mut page, &mut host, 2, LIST);
        page.handle(&mut host, &exit(2, 0));
        host.take();
        page.add(&mut host, "rsc.io/quote");
        assert_eq!(host.take(), ["task 3 go: add rsc.io/quote: go get rsc.io/quote", "refresh 1"]);
        assert!(page.handle(&mut host, &Event::TaskFinished((3, true))));
        assert_eq!(host.take(), ["task 4 go: add rsc.io/quote: go mod tidy"], "the next step, not a refresh yet");
        assert!(page.handle(&mut host, &Event::TaskFinished((4, true))));
        assert_eq!(host.take()[0], "spawn 5 mock ls --json", "done: listed again");
        assert!(page.render(96, 14).text().contains("add rsc.io/quote: done"));
        // A step that fails ends its action.
        page.add(&mut host, "a/b");
        host.take();
        page.handle(&mut host, &exit(5, 0));
        page.handle(&mut host, &Event::TaskFinished((6, false)));
        assert!(!host.take().iter().any(|c| c.contains("go mod tidy")));
        assert!(page.render(96, 14).text().contains("add a/b: failed"));
        // Refused, and said why.
        page.add(&mut host, "not a path");
        assert!(page.render(96, 14).text().contains("not a path isn't a module path"));
        // A step for each outdated one.
        page.deps = vec![Dependency::new("x", "1.0").latest("2.0"), Dependency::new("y", "1.0").latest("1.1")];
        page.key(&mut host, Key::Char('U'));
        host.take();
        page.key(&mut host, Key::Char('y'));
        assert_eq!(host.take()[0], "task 8 go: upgrade 2 packages: go get x@latest");
        page.handle(&mut host, &Event::TaskFinished((8, true)));
        assert_eq!(host.take()[0], "task 9 go: upgrade 2 packages: go get y@latest");
        // Nothing it can do on its own: the defaults say so.
        page.key(&mut host, Key::Char('u'));
        assert!(page.render(96, 14).text().contains("go can't upgrade"));
    }
}
