//! Linear's page, drawn with `ui-kit` and kept as a state you can test
//! without Fenix: what a key does (an [`Action`] for the extension to
//! carry out), and how it's laid out ([`LinearPage::layout`]).
//!
//! On the left, the views: yours (assigned to you, created by you), each
//! team's active cycle and backlog, and the typed filter. In the middle, a
//! view's issues, a page at a time (moving past the last one asks for the
//! next). `Enter` opens an issue: its state, priority, people, labels,
//! cycle, project, parent and sub-issues, description and comments.
//!
//! An issue answers the same keys on the list and on its own page, and the
//! same keys the Jira page and the agenda's tasks have: `s` state (its
//! team's real workflow states), `p` priority, `A` assign, `u` due, `e`
//! title, `E` description, `C` comment, `a` and `t` add to the agenda
//! (and start its clock), `y` the address, `o` the browser. Linear's own:
//! `b` a branch for it, `Y` its identifier and `B` its branch name
//! copied. `n` is the new-issue form, `/` a typed filter, `R` refresh.
//!
//! Anything that changes Linear is a deliberate act: a form submitted
//! (`S`), a state or priority or person picked from a menu, a comment or
//! a description sent from its own pane.

use std::collections::{BTreeSet, HashMap, HashSet};

use fenix_linear_client::filter;
use fenix_linear_client::linear::{LabelInfo, Listed, NewIssue, StateInfo, TeamInfo, TeamMeta};
use fenix_linear_client::model::{Detail, PRIORITIES};
use fenix_tracker::Person;
use fenix_ui_kit::{text as ui, Canvas, Key, Outcome, Rect, Role, TextField};

// -- Views ---------------------------------------------------------------------

/// A list of issues.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ViewId {
    /// Assigned to you, not finished.
    Mine,
    /// Made by you, not finished.
    Created,
    /// A team's active cycle: its key.
    Cycle(String),
    /// A team's backlog: its key.
    Backlog(String),
    /// A filter typed, as it was typed.
    Typed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewDef {
    /// What it's listed under: "Me", a team's key, "Filter".
    pub group: String,
    pub name: String,
    pub id: ViewId,
}

fn view(group: &str, name: &str, id: ViewId) -> ViewDef {
    ViewDef { group: group.into(), name: name.into(), id }
}

/// The views for these teams, narrowed to `only` (their keys) when it has
/// any, and the typed filter after them.
pub fn views(teams: &[TeamInfo], only: &[String], typed: Option<&str>) -> Vec<ViewDef> {
    let mut out = vec![view("Me", "My issues", ViewId::Mine), view("Me", "Created by me", ViewId::Created)];
    for t in teams.iter().filter(|t| only.is_empty() || only.iter().any(|o| o.eq_ignore_ascii_case(&t.key))) {
        out.push(view(&t.key, "Active cycle", ViewId::Cycle(t.key.clone())));
        out.push(view(&t.key, "Backlog", ViewId::Backlog(t.key.clone())));
    }
    if let Some(text) = typed {
        out.push(view("Filter", &filter::describe(text), ViewId::Typed(text.to_string())));
    }
    out
}

/// What a view has so far.
#[derive(Debug, Clone)]
pub enum Loaded {
    Loading,
    Failed(String),
    Ready { issues: Vec<Detail>, next: Option<String>, more: bool },
}

// -- What a key asks for ---------------------------------------------------------

/// What the extension is to do after a key.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Close,
    /// The view showing again, from the start.
    Refresh,
    /// A view's issues, from `after` (the next page) or the start.
    Fetch { view: ViewId, after: Option<String> },
    /// An issue's own page.
    Detail(String),
    Teams,
    TeamMeta(String),
    /// The states an issue can move to, for a menu.
    States(String),
    /// The people it can be assigned to, for a menu.
    People(String),
    SetState { key: String, id: String, name: String },
    SetPriority { key: String, number: u8, name: String },
    /// `None` unassigns.
    Assign { key: String, person: Option<Person> },
    SetDue { key: String, due: Option<String> },
    SetTitle { key: String, title: String },
    /// Ask for a line (a title, a due date), starting from what's there.
    EditTitle { key: String, current: String },
    EditDue { key: String, current: String },
    /// Open Fenix's pane to write in.
    EditDescription(String),
    Comment(String),
    Create(NewIssue),
    /// Start the branch for an issue in the project: `confirmed` once the
    /// tree's changes were asked about.
    Branch { key: String, confirmed: bool },
    CopyUrl(String),
    CopyIdentifier(String),
    CopyBranch(String),
    Browser(String),
    AddToAgenda { key: String, clock: bool },
    OpenTask(String),
    /// The form was left: answer the agenda's `create` that opened it.
    FormClosed(Option<u64>),
}

/// What the page needs of the extension to draw and decide.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// Today, `YYYY-MM-DD`: a due date past is drawn so.
    pub today: String,
    /// The keys that open the settings page.
    pub settings_keys: String,
    /// There's an API key.
    pub ready: bool,
}

// -- Menus and the form --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Purpose {
    State(String),
    Priority(String),
    Assign(String),
    FormTeam,
    FormState,
    FormAssignee,
    FormLabels,
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub title: String,
    /// (what's shown, what it stands for).
    pub items: Vec<(String, String)>,
    pub at: usize,
    pub purpose: Purpose,
    /// Rows ticked, for a menu of several.
    pub checked: Option<BTreeSet<usize>>,
}

/// The new-issue form's fields, in the order the keys go through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Team,
    Title,
    Description,
    Priority,
    State,
    Assignee,
    Labels,
}

const FIELDS: [Field; 7] = [Field::Team, Field::Title, Field::Description, Field::Priority, Field::State, Field::Assignee, Field::Labels];

pub struct Form {
    pub team: Option<TeamInfo>,
    pub meta: Option<TeamMeta>,
    pub meta_failed: Option<String>,
    pub title: TextField,
    pub description: TextField,
    pub priority: u8,
    pub state: Option<StateInfo>,
    pub assignee: Option<Person>,
    pub labels: Vec<LabelInfo>,
    pub field: usize,
    /// A text field has the keyboard.
    pub editing: bool,
    /// The agenda's `create` call this form answers.
    pub waiting: Option<u64>,
}

impl Form {
    fn new(waiting: Option<u64>, title: &str, description: &str, team: Option<TeamInfo>) -> Form {
        Form {
            team,
            meta: None,
            meta_failed: None,
            title: TextField::new().with_text(title).with_placeholder("A title"),
            description: TextField::multiline().with_text(description).with_placeholder("A description, in Markdown"),
            priority: 0,
            state: None,
            assignee: None,
            labels: Vec::new(),
            field: 0,
            editing: false,
            waiting,
        }
    }

    fn at(&self) -> Field {
        FIELDS[self.field]
    }

    /// The issue it would make, or what's missing.
    pub fn issue(&self) -> Result<NewIssue, String> {
        let team = self.team.as_ref().ok_or("choose a team first")?;
        let title = self.title.text().trim();
        if title.is_empty() {
            return Err("an issue needs a title".into());
        }
        Ok(NewIssue {
            team_id: team.id.clone(),
            title: title.to_string(),
            description: self.description.text().trim_end().to_string(),
            priority: Some(self.priority).filter(|p| *p != 0),
            label_ids: self.labels.iter().map(|l| l.id.clone()).collect(),
            assignee_id: self.assignee.as_ref().map(|p| p.id.clone()),
            state_id: self.state.as_ref().map(|s| s.id.clone()),
        })
    }
}

// -- The page -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Views,
    Issues,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    List,
    /// An issue's own page, by identifier.
    Issue(String),
}

pub struct LinearPage {
    only_teams: Vec<String>,
    pub teams: Option<Vec<TeamInfo>>,
    pub views: Vec<ViewDef>,
    typed: Option<String>,
    pub at: usize,
    pub col: Col,
    pub lists: HashMap<ViewId, Loaded>,
    pub row: usize,
    pub screen: Screen,
    pub details: HashMap<String, Result<Detail, String>>,
    /// The first line of the issue page shown.
    pub scroll: usize,
    pub menu: Option<Menu>,
    pub form: Option<Form>,
    /// The typed filter being typed.
    pub typing_filter: Option<TextField>,
    pub help: bool,
    /// Something to say under the page, and whether it's bad.
    pub note: Option<(String, bool)>,
    /// Identifiers of the issues in the agenda.
    pub in_agenda: HashSet<String>,
}

impl LinearPage {
    pub fn new(only_teams: Vec<String>) -> LinearPage {
        LinearPage {
            views: views(&[], &only_teams, None),
            only_teams,
            teams: None,
            typed: None,
            at: 0,
            col: Col::Views,
            lists: HashMap::new(),
            row: 0,
            screen: Screen::List,
            details: HashMap::new(),
            scroll: 0,
            menu: None,
            form: None,
            typing_filter: None,
            help: false,
            note: None,
            in_agenda: HashSet::new(),
        }
    }

    pub fn current(&self) -> &ViewDef {
        &self.views[self.at.min(self.views.len() - 1)]
    }

    fn loaded(&self) -> Option<&Loaded> {
        self.lists.get(&self.current().id)
    }

    /// The issues of the view showing.
    pub fn issues(&self) -> &[Detail] {
        match self.loaded() {
            Some(Loaded::Ready { issues, .. }) => issues,
            _ => &[],
        }
    }

    pub fn selected(&self) -> Option<&Detail> {
        self.issues().get(self.row)
    }

    /// The issue the keys are about: the open one, or the selected row.
    pub fn target(&self) -> Option<String> {
        match &self.screen {
            Screen::Issue(key) => Some(key.clone()),
            Screen::List => self.selected().map(|d| d.issue.key.clone()),
        }
    }

    /// The loaded detail or row of `key`.
    pub fn known(&self, key: &str) -> Option<&Detail> {
        self.details.get(key).and_then(|d| d.as_ref().ok()).or_else(|| self.lists.values().find_map(|l| if let Loaded::Ready { issues, .. } = l { issues.iter().find(|d| d.issue.key == key) } else { None }))
    }

    /// The view showing, if it hasn't been asked for: marked loading, and
    /// to be fetched.
    pub fn wants_fetch(&mut self) -> Option<ViewId> {
        let id = self.current().id.clone();
        if self.lists.contains_key(&id) {
            return None;
        }
        self.lists.insert(id.clone(), Loaded::Loading);
        Some(id)
    }

    /// The teams came: the views for them, the selection kept.
    pub fn set_teams(&mut self, result: Result<Vec<TeamInfo>, String>) {
        match result {
            Ok(teams) => {
                let keep = self.current().id.clone();
                self.views = views(&teams, &self.only_teams, self.typed.as_deref());
                self.at = self.views.iter().position(|v| v.id == keep).unwrap_or(0);
                self.teams = Some(teams);
            }
            Err(why) => {
                self.teams = Some(Vec::new());
                self.note = Some((format!("couldn't list your teams: {why}"), true));
            }
        }
    }

    /// A view's issues came (`after`: the page after one it has).
    pub fn list_loaded(&mut self, view: &ViewId, after: Option<&str>, result: Result<Listed, String>) {
        let had = self.lists.remove(view);
        let entry = match (result, had, after) {
            (Ok(page), Some(Loaded::Ready { mut issues, .. }), Some(_)) => {
                issues.extend(page.issues);
                Loaded::Ready { issues, next: page.next, more: false }
            }
            (Ok(page), _, _) => Loaded::Ready { issues: page.issues, next: page.next, more: false },
            (Err(why), Some(Loaded::Ready { issues, next, .. }), Some(_)) => {
                self.note = Some((why, true));
                Loaded::Ready { issues, next, more: false }
            }
            (Err(why), _, _) => Loaded::Failed(why),
        };
        self.lists.insert(view.clone(), entry);
        let len = self.issues().len();
        if self.row >= len {
            self.row = len.saturating_sub(1);
        }
    }

    pub fn detail_loaded(&mut self, key: &str, result: Result<Detail, String>) {
        self.details.insert(key.to_string(), result);
    }

    /// A menu, opened by what the extension fetched for it.
    pub fn offer(&mut self, purpose: Purpose, title: &str, items: Vec<(String, String)>, current: Option<&str>) {
        let at = current.and_then(|c| items.iter().position(|(_, v)| v == c)).unwrap_or(0);
        let checked = matches!(purpose, Purpose::FormLabels).then(|| self.form.as_ref().map(|f| items.iter().enumerate().filter(|(_, (_, v))| f.labels.iter().any(|l| &l.id == v)).map(|(i, _)| i).collect()).unwrap_or_default());
        self.menu = Some(Menu { title: title.to_string(), items, at, purpose, checked });
    }

    /// A team's states, people and labels came, for the form.
    pub fn meta_loaded(&mut self, team_id: &str, result: Result<TeamMeta, String>) {
        let Some(form) = &mut self.form else { return };
        if form.team.as_ref().map(|t| t.id.as_str()) != Some(team_id) {
            return;
        }
        match result {
            Ok(meta) => {
                form.meta = Some(meta);
                form.meta_failed = None;
            }
            Err(why) => form.meta_failed = Some(why),
        }
    }

    /// The new-issue form, started from a title and description (the agenda's
    /// task), in `team` when it's known.
    pub fn start_form(&mut self, waiting: Option<u64>, title: &str, description: &str) -> Action {
        let team = self.current_team();
        let mut form = Form::new(waiting, title, description, team.clone());
        form.field = if form.team.is_some() { 1 } else { 0 };
        self.form = Some(form);
        self.menu = None;
        match team {
            Some(t) => Action::TeamMeta(t.id),
            None if self.teams.is_none() => Action::Teams,
            None => self.open_team_menu(),
        }
    }

    /// The team of the view showing, or the only one there is.
    fn current_team(&self) -> Option<TeamInfo> {
        let teams = self.teams.as_ref()?;
        let key = match &self.current().id {
            ViewId::Cycle(k) | ViewId::Backlog(k) => Some(k.clone()),
            _ => self.selected().map(|d| d.team.key.clone()),
        };
        key.and_then(|k| teams.iter().find(|t| t.key.eq_ignore_ascii_case(&k)).cloned()).or_else(|| (teams.len() == 1).then(|| teams[0].clone()))
    }

    fn open_team_menu(&mut self) -> Action {
        let teams = self.teams.clone().unwrap_or_default();
        if teams.is_empty() {
            self.note = Some(("no teams to make an issue in".into(), true));
            self.form = None;
            return Action::None;
        }
        let items = teams.iter().map(|t| (format!("{}  {}", t.key, t.name), t.id.clone())).collect();
        self.offer(Purpose::FormTeam, "Team", items, None);
        Action::None
    }

    /// The teams came while the form waited for them.
    pub fn teams_for_form(&mut self) -> Action {
        if self.form.as_ref().is_some_and(|f| f.team.is_none() && f.meta.is_none()) && self.menu.is_none() {
            return self.open_team_menu();
        }
        Action::None
    }

    pub fn typing(&self) -> bool {
        self.typing_filter.is_some() || self.form.as_ref().is_some_and(|f| f.editing)
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(f) = &mut self.typing_filter {
            f.paste(&text.replace('\n', " "));
        } else if let Some(form) = &mut self.form {
            match (form.editing, form.at()) {
                (true, Field::Title) => {
                    form.title.paste(&text.replace('\n', " "));
                }
                (true, Field::Description) => {
                    form.description.paste(text);
                }
                _ => {}
            }
        }
    }

    // -- Keys ---------------------------------------------------------------------

    pub fn key(&mut self, key: Key, ctx: &Ctx) -> Action {
        self.note = self.note.take().filter(|_| false);
        if self.help {
            self.help = false;
            return Action::None;
        }
        if self.menu.is_some() {
            return self.menu_key(key);
        }
        if self.form.is_some() {
            return self.form_key(key);
        }
        if self.typing_filter.is_some() {
            return self.filter_key(key);
        }
        match key {
            Key::Char('q') => return Action::Close,
            Key::Char('?') => {
                self.help = true;
                return Action::None;
            }
            Key::Char('R') => {
                return self.refresh();
            }
            Key::Char('n') => return self.start_form(None, "", ""),
            Key::Char('/') => {
                self.typing_filter = Some(TextField::new().with_text(self.typed.as_deref().unwrap_or("")).with_placeholder("team:ENG state:started assignee:me label:bug words"));
                return Action::None;
            }
            _ => {}
        }
        if let Screen::Issue(k) = self.screen.clone() {
            return self.issue_page_key(key, k, ctx);
        }
        match (key, self.col) {
            (Key::Tab | Key::BackTab | Key::Char('l') | Key::Right | Key::Enter, Col::Views) => {
                self.col = Col::Issues;
                Action::None
            }
            (Key::Tab | Key::BackTab | Key::Char('h') | Key::Left | Key::Escape, Col::Issues) => {
                self.col = Col::Views;
                Action::None
            }
            (Key::Down | Key::Char('j'), Col::Views) => self.pick_view(self.at + 1),
            (Key::Up | Key::Char('k'), Col::Views) => self.pick_view(self.at.saturating_sub(1)),
            (Key::Char('g'), Col::Views) => self.pick_view(0),
            (Key::Char('G'), Col::Views) => self.pick_view(self.views.len() - 1),
            (Key::Down | Key::Char('j'), Col::Issues) => {
                if self.row + 1 < self.issues().len() {
                    self.row += 1;
                    Action::None
                } else {
                    self.more()
                }
            }
            (Key::Up | Key::Char('k'), Col::Issues) => {
                self.row = self.row.saturating_sub(1);
                Action::None
            }
            (Key::Char('g'), Col::Issues) => {
                self.row = 0;
                Action::None
            }
            (Key::Char('G'), Col::Issues) => {
                self.row = self.issues().len().saturating_sub(1);
                Action::None
            }
            (Key::Enter, Col::Issues) => match self.selected().map(|d| d.issue.key.clone()) {
                Some(k) => {
                    if self.in_agenda.contains(&k) {
                        return Action::OpenTask(k);
                    }
                    self.open_issue(k)
                }
                None => Action::None,
            },
            (Key::Char('l') | Key::Right, Col::Issues) => match self.selected().map(|d| d.issue.key.clone()) {
                Some(k) => self.open_issue(k),
                None => Action::None,
            },
            (key, Col::Issues) => match self.selected().map(|d| d.issue.key.clone()) {
                Some(k) => self.issue_keys(key, k, ctx),
                None => Action::None,
            },
            _ => Action::None,
        }
    }

    fn refresh(&mut self) -> Action {
        let id = self.current().id.clone();
        self.lists.insert(id.clone(), Loaded::Loading);
        if let Screen::Issue(k) = &self.screen {
            self.details.remove(k);
            return Action::Detail(k.clone());
        }
        Action::Fetch { view: id, after: None }
    }

    fn pick_view(&mut self, at: usize) -> Action {
        self.at = at.min(self.views.len() - 1);
        self.row = 0;
        match self.wants_fetch() {
            Some(view) => Action::Fetch { view, after: None },
            None => Action::None,
        }
    }

    /// The next page, when the last row is passed and there is one.
    fn more(&mut self) -> Action {
        let id = self.current().id.clone();
        if let Some(Loaded::Ready { next: Some(cursor), more, .. }) = self.lists.get_mut(&id) {
            if !*more {
                *more = true;
                return Action::Fetch { view: id, after: Some(cursor.clone()) };
            }
        }
        Action::None
    }

    fn open_issue(&mut self, key: String) -> Action {
        self.screen = Screen::Issue(key.clone());
        self.scroll = 0;
        if self.details.get(&key).is_some_and(|d| d.is_ok()) {
            Action::None
        } else {
            Action::Detail(key)
        }
    }

    fn issue_page_key(&mut self, key: Key, k: String, ctx: &Ctx) -> Action {
        match key {
            Key::Escape | Key::Char('h') | Key::Left => {
                self.screen = Screen::List;
                Action::None
            }
            Key::Down | Key::Char('j') => {
                self.scroll += 1;
                Action::None
            }
            Key::Up | Key::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                Action::None
            }
            Key::Char('g') => {
                self.scroll = 0;
                Action::None
            }
            Key::Char('G') => {
                self.scroll = usize::MAX / 2;
                Action::None
            }
            Key::Enter if self.in_agenda.contains(&k) => Action::OpenTask(k),
            Key::Enter => Action::None,
            key => self.issue_keys(key, k, ctx),
        }
    }

    /// An issue's own keys, on the list and on its page.
    fn issue_keys(&mut self, key: Key, k: String, _ctx: &Ctx) -> Action {
        let current = self.known(&k);
        let title = current.map(|d| d.issue.title.clone()).unwrap_or_default();
        let due = current.and_then(|d| d.issue.due.clone()).unwrap_or_default();
        let priority = current.map(|d| d.issue.priority.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "No priority".into()));
        match key {
            Key::Char('s') => Action::States(k),
            Key::Char('p') => {
                let items = PRIORITIES.iter().map(|(n, name, _)| (name.to_string(), n.to_string())).collect();
                let now = current.map(|d| d.priority.to_string());
                let _ = priority;
                self.offer(Purpose::Priority(k.clone()), &format!("{k} priority"), items, now.as_deref());
                Action::None
            }
            Key::Char('A') => Action::People(k),
            Key::Char('u') => Action::EditDue { key: k, current: due },
            Key::Char('e') => Action::EditTitle { key: k, current: title },
            Key::Char('E') => Action::EditDescription(k),
            Key::Char('C') => Action::Comment(k),
            Key::Char('b') => Action::Branch { key: k, confirmed: false },
            Key::Char('a') => Action::AddToAgenda { key: k, clock: false },
            Key::Char('t') => Action::AddToAgenda { key: k, clock: true },
            Key::Char('y') => Action::CopyUrl(k),
            Key::Char('Y') => Action::CopyIdentifier(k),
            Key::Char('B') => Action::CopyBranch(k),
            Key::Char('o') => Action::Browser(k),
            _ => Action::None,
        }
    }

    // -- Menus -----------------------------------------------------------------------

    fn menu_key(&mut self, key: Key) -> Action {
        let Some(menu) = &mut self.menu else { return Action::None };
        let n = menu.items.len().max(1);
        match key {
            Key::Escape | Key::Char('q') => {
                let purpose = menu.purpose.clone();
                self.menu = None;
                // A form with no team to be in has nowhere to go.
                if purpose == Purpose::FormTeam && self.form.as_ref().is_some_and(|f| f.team.is_none()) {
                    return self.close_form();
                }
                Action::None
            }
            Key::Down | Key::Char('j') | Key::Tab => {
                menu.at = (menu.at + 1) % n;
                Action::None
            }
            Key::Up | Key::Char('k') | Key::BackTab => {
                menu.at = (menu.at + n - 1) % n;
                Action::None
            }
            Key::Char(' ') if menu.checked.is_some() => {
                let at = menu.at;
                if let Some(c) = &mut menu.checked {
                    if !c.remove(&at) {
                        c.insert(at);
                    }
                }
                Action::None
            }
            Key::Char(c @ '1'..='9') if (c as usize - '1' as usize) < menu.items.len() && menu.checked.is_none() => {
                menu.at = c as usize - '1' as usize;
                self.pick()
            }
            Key::Enter | Key::Char(' ') => self.pick(),
            _ => Action::None,
        }
    }

    fn pick(&mut self) -> Action {
        let Some(menu) = self.menu.take() else { return Action::None };
        let Some((label, value)) = menu.items.get(menu.at).cloned() else { return Action::None };
        let name = label.split("  (").next().unwrap_or(&label).trim().to_string();
        match menu.purpose {
            Purpose::State(key) => Action::SetState { key, id: value, name },
            Purpose::Priority(key) => {
                let number: u8 = value.parse().unwrap_or(0);
                Action::SetPriority { key, number, name }
            }
            Purpose::Assign(key) => Action::Assign { key, person: (!value.is_empty()).then(|| Person { id: value.clone(), name }) },
            Purpose::FormTeam => {
                let team = self.teams.iter().flatten().find(|t| t.id == value).cloned();
                if let (Some(form), Some(team)) = (&mut self.form, team) {
                    let changed = form.team.as_ref().map(|t| t.id != team.id).unwrap_or(true);
                    if changed {
                        form.meta = None;
                        form.meta_failed = None;
                        form.state = None;
                        form.assignee = None;
                        form.labels.clear();
                    }
                    form.team = Some(team.clone());
                    if form.field == 0 {
                        form.field = 1;
                    }
                    return Action::TeamMeta(team.id);
                }
                Action::None
            }
            Purpose::FormState => {
                if let Some(form) = &mut self.form {
                    form.state = form.meta.as_ref().and_then(|m| m.states.iter().find(|s| s.id == value).cloned());
                }
                Action::None
            }
            Purpose::FormAssignee => {
                if let Some(form) = &mut self.form {
                    form.assignee = form.meta.as_ref().and_then(|m| m.people.iter().find(|p| p.id == value).cloned());
                }
                Action::None
            }
            Purpose::FormLabels => {
                if let (Some(form), Some(checked)) = (&mut self.form, menu.checked) {
                    let all: Vec<LabelInfo> = form.meta.iter().flat_map(|m| m.labels.iter().cloned()).collect();
                    form.labels = checked.iter().filter_map(|i| menu.items.get(*i)).filter_map(|(_, id)| all.iter().find(|l| &l.id == id).cloned()).collect();
                }
                Action::None
            }
        }
    }

    // -- The typed filter --------------------------------------------------------------

    fn filter_key(&mut self, key: Key) -> Action {
        let Some(field) = &mut self.typing_filter else { return Action::None };
        match field.handle_key(key) {
            Outcome::Cancel => {
                self.typing_filter = None;
                Action::None
            }
            Outcome::Submit => {
                let text = field.text().trim().to_string();
                if text.is_empty() {
                    self.typing_filter = None;
                    return Action::None;
                }
                match filter::parse(&text) {
                    Err(why) => {
                        self.note = Some((why, true));
                        Action::None
                    }
                    Ok(_) => {
                        self.typing_filter = None;
                        self.typed = Some(text.clone());
                        self.views = views(self.teams.as_deref().unwrap_or(&[]), &self.only_teams, self.typed.as_deref());
                        self.lists.remove(&ViewId::Typed(text.clone()));
                        let at = self.views.len() - 1;
                        self.col = Col::Issues;
                        self.screen = Screen::List;
                        self.pick_view(at)
                    }
                }
            }
            _ => Action::None,
        }
    }

    // -- The form ----------------------------------------------------------------------

    fn close_form(&mut self) -> Action {
        let waiting = self.form.take().and_then(|f| f.waiting);
        Action::FormClosed(waiting)
    }

    fn form_key(&mut self, key: Key) -> Action {
        let Some(form) = &mut self.form else { return Action::None };
        if form.editing {
            let (field, end_on_enter) = match form.at() {
                Field::Title => (&mut form.title, true),
                _ => (&mut form.description, false),
            };
            match field.handle_key(key) {
                Outcome::Cancel => form.editing = false,
                Outcome::Submit if end_on_enter => form.editing = false,
                _ => {}
            }
            return Action::None;
        }
        let n = FIELDS.len();
        match key {
            Key::Escape | Key::Char('q') => return self.close_form(),
            Key::Down | Key::Char('j') | Key::Tab => form.field = (form.field + 1) % n,
            Key::Up | Key::Char('k') | Key::BackTab => form.field = (form.field + n - 1) % n,
            Key::Char('S') => {
                return match form.issue() {
                    Ok(issue) => Action::Create(issue),
                    Err(why) => {
                        self.note = Some((why, true));
                        Action::None
                    }
                };
            }
            Key::Left | Key::Char('h') | Key::Right | Key::Char('l') if form.at() == Field::Priority => {
                let order: Vec<u8> = PRIORITIES.iter().map(|(n, _, _)| *n).collect();
                let at = order.iter().position(|p| *p == form.priority).unwrap_or(0);
                let forward = matches!(key, Key::Right | Key::Char('l'));
                form.priority = order[if forward { (at + 1) % order.len() } else { (at + order.len() - 1) % order.len() }];
            }
            Key::Enter | Key::Char(' ') | Key::Char('e') => match form.at() {
                Field::Title | Field::Description => {
                    form.editing = true;
                    form.title.focused = form.at() == Field::Title;
                    form.description.focused = form.at() == Field::Description;
                }
                Field::Team => return self.open_team_menu(),
                Field::Priority => {}
                Field::State | Field::Assignee | Field::Labels => return self.form_menu(),
            },
            _ => {}
        }
        Action::None
    }

    fn form_menu(&mut self) -> Action {
        let Some(form) = &self.form else { return Action::None };
        let Some(meta) = form.meta.clone() else {
            let note = form.meta_failed.clone().map(|w| format!("couldn't read the team: {w}")).unwrap_or_else(|| "the team's states, people and labels are still coming".into());
            self.note = Some((note, form.meta_failed.is_some()));
            return Action::None;
        };
        match form.at() {
            Field::State => {
                let items = std::iter::once(("Team's default".to_string(), String::new())).chain(meta.states.iter().map(|s| (format!("{}  ({})", s.name, s.kind), s.id.clone()))).collect();
                let current = form.state.as_ref().map(|s| s.id.clone()).unwrap_or_default();
                self.offer(Purpose::FormState, "State", items, Some(&current));
            }
            Field::Assignee => {
                let items = std::iter::once(("Unassigned".to_string(), String::new())).chain(meta.people.iter().map(|p| (p.name.clone(), p.id.clone()))).collect();
                let current = form.assignee.as_ref().map(|p| p.id.clone()).unwrap_or_default();
                self.offer(Purpose::FormAssignee, "Assignee", items, Some(&current));
            }
            Field::Labels => {
                let items: Vec<(String, String)> = meta.labels.iter().map(|l| (l.name.clone(), l.id.clone())).collect();
                if items.is_empty() {
                    self.note = Some(("this team has no labels".into(), false));
                } else {
                    self.offer(Purpose::FormLabels, "Labels  (Space ticks, Enter keeps)", items, None);
                }
            }
            _ => {}
        }
        Action::None
    }
}

// -- Drawing --------------------------------------------------------------------------

fn put(c: &mut Canvas, x: usize, y: usize, w: usize, text: &str, role: Role) -> usize {
    c.put_fit(x, y, w, text, role)
}

fn priority_role(d: &Detail) -> Role {
    match d.priority {
        1 => Role::Bad,
        2 => Role::Warn,
        0 => Role::Muted,
        _ => Role::Text,
    }
}

fn state_role(d: &Detail) -> Role {
    match d.issue.state.stage.as_str() {
        "doing" => Role::Accent,
        "done" => Role::Good,
        _ => Role::Muted,
    }
}

fn due_role(due: &str, today: &str) -> Role {
    if !today.is_empty() && due < today {
        Role::Bad
    } else {
        Role::Text
    }
}

fn priority_name(d: &Detail) -> &str {
    d.issue.priority.as_ref().map(|p| p.name.as_str()).unwrap_or("")
}

impl LinearPage {
    /// The page, `w` cells by `h` lines.
    pub fn layout(&self, ctx: &Ctx, w: usize, h: usize) -> Canvas {
        let w = w.max(30);
        let h = h.max(8);
        let mut c = Canvas::new();
        let body_h = h - 1;
        if let Some(form) = &self.form {
            self.draw_form(form, &mut c, w, body_h);
        } else if !ctx.ready {
            let x = c.put(0, 0, "Linear", Role::Title);
            put(&mut c, x, 0, w - x, "  is not set up", Role::Muted);
            c.rule(1, 0..w);
            put(&mut c, 2, 3, w - 2, "Linear needs a personal API key.", Role::Text);
            put(&mut c, 2, 4, w - 2, "Make one in Linear: Settings, Security & access, Personal API keys.", Role::Muted);
            put(&mut c, 2, 5, w - 2, &format!("Then set it on the settings page ({}), under Linear: linear.token.", ctx.settings_keys), Role::Muted);
        } else {
            match &self.screen {
                Screen::List => self.draw_list(ctx, &mut c, w, body_h),
                Screen::Issue(key) => self.draw_issue(ctx, key, &mut c, w, body_h),
            }
        }
        if let Some(menu) = &self.menu {
            self.draw_menu(menu, &mut c, w, body_h);
        }
        if self.help {
            self.draw_help(&mut c, w, body_h);
        }
        // The last line: what's to say, or the keys.
        match &self.note {
            Some((text, bad)) => {
                put(&mut c, 0, h - 1, w, text, if *bad { Role::Bad } else { Role::Good });
            }
            None => {
                let mut x = 0;
                for (key, what) in self.key_strip(ctx) {
                    if x + key.chars().count() + what.chars().count() + 3 > w {
                        break;
                    }
                    x = c.put(x, h - 1, key, Role::Accent);
                    x = c.put(x, h - 1, &format!(" {what}   "), Role::Muted);
                }
            }
        }
        c.set_typing(self.typing());
        c
    }

    fn key_strip(&self, ctx: &Ctx) -> Vec<(&'static str, &'static str)> {
        if self.menu.is_some() || self.help {
            return vec![("Enter", "pick"), ("Esc", "leave it")];
        }
        if let Some(form) = &self.form {
            if form.editing {
                return vec![("Esc", "done")];
            }
            return vec![("j/k", "field"), ("Enter", "edit"), ("h/l", "priority"), ("S", "create"), ("Esc", "leave it")];
        }
        if self.typing_filter.is_some() {
            return vec![("Enter", "filter"), ("Esc", "cancel")];
        }
        if !ctx.ready {
            return vec![("q", "close")];
        }
        let mut keys = match &self.screen {
            Screen::Issue(_) => vec![("s", "state"), ("p", "priority"), ("A", "assign"), ("C", "comment"), ("b", "branch"), ("y", "link"), ("o", "browser"), ("Esc", "back")],
            Screen::List if self.col == Col::Views => vec![("j/k", "view"), ("l", "issues")],
            Screen::List => vec![("Enter", "open"), ("s", "state"), ("p", "priority"), ("A", "assign"), ("C", "comment"), ("b", "branch"), ("h", "views")],
        };
        keys.extend([("n", "new"), ("/", "filter"), ("R", "refresh"), ("?", "keys"), ("q", "close")]);
        keys
    }

    fn header(&self, c: &mut Canvas, w: usize, title: &str) {
        let x = c.put(0, 0, "Linear", Role::Title);
        put(c, x, 0, w.saturating_sub(x), &format!("  {title}"), Role::Muted);
        c.rule(1, 0..w);
    }

    fn draw_list(&self, ctx: &Ctx, c: &mut Canvas, w: usize, h: usize) {
        let v = self.current();
        let count = self.issues().len();
        let more = matches!(self.loaded(), Some(Loaded::Ready { next: Some(_), .. }));
        self.header(c, w, &format!("{}  {}{}", v.group, v.name, if count > 0 { format!("   {count}{}", if more { "+" } else { "" }) } else { String::new() }));
        let side = 26.min(w / 3);
        // The views.
        let mut y = 2;
        let mut group = "";
        for (i, def) in self.views.iter().enumerate() {
            if y + 2 > h {
                break;
            }
            if def.group != group {
                if !group.is_empty() {
                    y += 1;
                }
                group = &def.group;
                put(c, 1, y, side - 1, &def.group, Role::Muted);
                y += 1;
            }
            let here = i == self.at;
            put(c, 2, y, side - 3, &def.name, if here { Role::Title } else { Role::Text });
            if here {
                c.focus(y, 0..side);
            }
            y += 1;
        }
        if self.teams.is_none() {
            put(c, 1, y + 1, side - 1, "loading teams...", Role::Muted);
        }
        c.vrule(side, 2..h);
        // The issues.
        let x0 = side + 2;
        let iw = w - x0;
        match self.loaded() {
            None | Some(Loaded::Loading) => {
                put(c, x0, 3, iw, "loading...", Role::Muted);
            }
            Some(Loaded::Failed(why)) => {
                put(c, x0, 3, iw, why, Role::Bad);
                put(c, x0, 5, iw, "R tries again", Role::Muted);
            }
            Some(Loaded::Ready { issues, next, more }) => {
                if issues.is_empty() {
                    put(c, x0, 3, iw, "nothing here", Role::Muted);
                }
                let rows = h.saturating_sub(2);
                let top = self.row.saturating_sub(rows.saturating_sub(1));
                for (n, d) in issues.iter().enumerate().skip(top).take(rows) {
                    let y = 2 + n - top;
                    self.draw_row(ctx, c, x0, y, iw, d);
                    if n == self.row {
                        c.focus(y, side + 1..w);
                    }
                }
                if next.is_some() && *more {
                    put(c, x0, h - 1, iw, "loading more...", Role::Muted);
                }
            }
        }
    }

    fn draw_row(&self, ctx: &Ctx, c: &mut Canvas, x: usize, y: usize, w: usize, d: &Detail) {
        let wide = w >= 70;
        let (state_w, pri_w, due_w) = if wide { (14, 8, 11) } else { (0, 0, 0) };
        let id_w = 9;
        let title_w = w.saturating_sub(id_w + 1 + state_w + pri_w + due_w + 1);
        put(c, x, y, id_w, &d.issue.key, if self.in_agenda.contains(&d.issue.key) { Role::Good } else { Role::Accent });
        put(c, x + id_w + 1, y, title_w, &d.issue.title, Role::Text);
        if wide {
            let mut at = x + id_w + 1 + title_w + 1;
            put(c, at, y, state_w, &d.issue.state.name, state_role(d));
            at += state_w;
            put(c, at, y, pri_w, priority_name(d), priority_role(d));
            at += pri_w;
            if let Some(due) = &d.issue.due {
                put(c, at, y, due_w, due, due_role(due, &ctx.today));
            }
        }
    }

    // -- An issue ------------------------------------------------------------------------

    /// An issue's page as lines of coloured runs, `w` cells wide.
    pub fn issue_lines(&self, ctx: &Ctx, d: &Detail, w: usize) -> Vec<Vec<(String, Role)>> {
        let mut out: Vec<Vec<(String, Role)>> = Vec::new();
        let i = &d.issue;
        let pair = |label: &str, value: String, role: Role| vec![(format!("{label:<10}"), Role::Muted), (value, role)];
        for line in ui::wrap(&i.title, w) {
            out.push(vec![(line, Role::Title)]);
        }
        out.push(vec![(i.key.clone(), Role::Accent), (format!("  {}", d.team.name), Role::Muted), (if self.in_agenda.contains(&i.key) { "   in your agenda".to_string() } else { String::new() }, Role::Good)]);
        out.push(Vec::new());
        out.push(pair("State", format!("{}  ({})", i.state.name, d.state_type), state_role(d)));
        out.push(pair("Priority", if d.priority == 0 { "No priority".into() } else { priority_name(d).to_string() }, priority_role(d)));
        out.push(pair("Assignee", i.assignee.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "Unassigned".into()), if i.assignee.is_some() { Role::Text } else { Role::Muted }));
        if let Some(due) = &i.due {
            out.push(pair("Due", due.clone(), due_role(due, &ctx.today)));
        }
        if !i.labels.is_empty() {
            out.push(pair("Labels", i.labels.join(", "), Role::Text));
        }
        if let Some(cycle) = &d.cycle {
            out.push(pair("Cycle", cycle.clone(), Role::Text));
        }
        if let Some(project) = &d.project_name {
            out.push(pair("Project", project.clone(), Role::Text));
        }
        if let Some((key, title)) = &d.parent {
            out.push(pair("Parent", format!("{key}  {title}"), Role::Text));
        }
        if let Some(creator) = &d.creator {
            out.push(pair("Created", format!("{}  by {creator}", d.created), Role::Muted));
        }
        if !d.branch.is_empty() {
            out.push(pair("Branch", d.branch.clone(), Role::Muted));
        }
        if !d.children.is_empty() {
            out.push(Vec::new());
            out.push(vec![(format!("Sub-issues ({})", d.children.len()), Role::Muted)]);
            for ch in &d.children {
                let role = if ch.stage == "done" { Role::Good } else { Role::Text };
                out.push(vec![(format!("  {:<9} ", ch.key), Role::Accent), (ch.title.clone(), role), (format!("  {}", ch.state), Role::Muted)]);
            }
        }
        out.push(Vec::new());
        if i.body.trim().is_empty() {
            out.push(vec![("No description.".into(), Role::Muted)]);
        } else {
            for line in ui::wrap(&i.body, w) {
                out.push(vec![(line, Role::Text)]);
            }
            if i.body_cut {
                out.push(vec![("... the description is cut here: it's longer than this shows".into(), Role::Warn)]);
            }
        }
        out.push(Vec::new());
        out.push(vec![(format!("Comments ({}{})", i.comments.len(), if d.comments_more { "+" } else { "" }), Role::Muted)]);
        for cm in &i.comments {
            out.push(vec![(cm.author.clone(), Role::Accent), (format!("  {}", cm.created), Role::Muted)]);
            for line in ui::wrap(&cm.body, w.saturating_sub(2)) {
                out.push(vec![(format!("  {line}"), Role::Text)]);
            }
        }
        if i.comments.is_empty() {
            out.push(vec![("  none yet -- C writes one".into(), Role::Muted)]);
        }
        out
    }

    fn draw_issue(&self, ctx: &Ctx, key: &str, c: &mut Canvas, w: usize, h: usize) {
        self.header(c, w, key);
        let (iw, x0) = (w.saturating_sub(4), 2);
        match self.details.get(key) {
            None => {
                put(c, x0, 3, iw, "loading...", Role::Muted);
            }
            Some(Err(why)) => {
                put(c, x0, 3, iw, why, Role::Bad);
            }
            Some(Ok(d)) => {
                let lines = self.issue_lines(ctx, d, iw);
                let rows = h.saturating_sub(2);
                let top = self.scroll.min(lines.len().saturating_sub(rows));
                for (n, line) in lines.iter().enumerate().skip(top).take(rows) {
                    let mut x = x0;
                    for (text, role) in line {
                        x = put(c, x, 2 + n - top, (x0 + iw).saturating_sub(x), text, role.clone());
                    }
                }
                if lines.len() > rows {
                    fenix_ui_kit::scrollbar(c, w - 1, 2, rows, lines.len(), top);
                }
            }
        }
    }

    // -- A menu, help and the form -----------------------------------------------------------

    fn draw_menu(&self, menu: &Menu, c: &mut Canvas, w: usize, h: usize) {
        let width = menu.items.iter().map(|(l, _)| ui::width(l) + 6).chain([ui::width(&menu.title) + 4]).max().unwrap_or(20).clamp(24, w.saturating_sub(4));
        let rows = menu.items.len().min(h.saturating_sub(6)).max(1);
        let top = menu.at.saturating_sub(rows - 1);
        let (x, y) = (w.saturating_sub(width) / 2, 3);
        c.clear(Rect::new(x, y, width, rows + 3));
        for yy in y..y + rows + 3 {
            c.panel(yy, x..x + width);
        }
        put(c, x + 2, y + 1, width - 4, &menu.title, Role::Title);
        for (n, (label, _)) in menu.items.iter().enumerate().skip(top).take(rows) {
            let yy = y + 2 + n - top;
            let tick = match &menu.checked {
                Some(set) if set.contains(&n) => "[x] ",
                Some(_) => "[ ] ",
                None => "",
            };
            let key = if menu.checked.is_none() && n < 9 { format!("{} ", n + 1) } else { String::new() };
            let here = n == menu.at;
            let at = c.put(x + 2, yy, &key, Role::Accent);
            put(c, at, yy, (x + width).saturating_sub(at + 1), &format!("{tick}{label}"), if here { Role::Title } else { Role::Text });
            if here {
                c.focus(yy, x..x + width);
            }
        }
    }

    fn draw_help(&self, c: &mut Canvas, w: usize, h: usize) {
        let rows: [(&str, &str); 22] = [
            ("j k  g G", "move"),
            ("l  Enter", "open the issue (Enter: its task, when it's in your agenda)"),
            ("h  Esc", "back"),
            ("s", "move it to another state"),
            ("p", "priority"),
            ("A", "assign it"),
            ("u", "due date (empty clears it)"),
            ("e  E", "title, description"),
            ("C", "comment"),
            ("b", "a branch for it, in this project"),
            ("y  Y  B", "copy its address, its identifier, its branch name"),
            ("o", "open it in the browser"),
            ("a  t", "add it to the agenda, and start the clock"),
            ("n", "a new issue"),
            ("/", "a typed filter: team:ENG state:started assignee:me label:bug words"),
            ("", "  assignee: me or none or a name   state: a name or a type"),
            ("", "  priority: urgent high medium low none   is: open closed all"),
            ("R", "refresh"),
            ("?", "this"),
            ("q", "close"),
            ("", ""),
            ("", "Anything that changes Linear asks: a menu, a prompt, a form."),
        ];
        let width = w.saturating_sub(6).min(96);
        let (x, y) = (w.saturating_sub(width) / 2, 2);
        c.clear(Rect::new(x, y, width, rows.len() + 3));
        for yy in y..(y + rows.len() + 3).min(h) {
            c.panel(yy, x..x + width);
        }
        put(c, x + 2, y + 1, width - 4, "Linear's keys", Role::Title);
        for (n, (k, what)) in rows.iter().enumerate() {
            if y + 2 + n >= h {
                break;
            }
            put(c, x + 2, y + 2 + n, 12, k, Role::Accent);
            put(c, x + 14, y + 2 + n, width.saturating_sub(16), what, Role::Text);
        }
    }

    fn draw_form(&self, form: &Form, c: &mut Canvas, w: usize, h: usize) {
        self.header(c, w, "a new issue");
        let label_w = 12;
        let x0 = 2;
        let fw = w.saturating_sub(x0 + label_w + 2);
        let mut y = 3;
        for (n, f) in FIELDS.iter().enumerate() {
            let label = match f {
                Field::Team => "Team",
                Field::Title => "Title",
                Field::Description => "Description",
                Field::Priority => "Priority",
                Field::State => "State",
                Field::Assignee => "Assignee",
                Field::Labels => "Labels",
            };
            let here = n == form.field;
            put(c, x0, y, label_w, label, if here { Role::Title } else { Role::Muted });
            let vx = x0 + label_w;
            let text_value = |s: String, empty: &str| if s.is_empty() { (empty.to_string(), Role::Muted) } else { (s, Role::Text) };
            let (value, role) = match f {
                Field::Team => text_value(form.team.as_ref().map(|t| format!("{}  {}", t.key, t.name)).unwrap_or_default(), "Enter chooses one"),
                Field::Priority => {
                    let name = PRIORITIES.iter().find(|(p, _, _)| *p == form.priority).map(|(_, n, _)| *n).unwrap_or("No priority");
                    (format!("‹ {name} ›"), if form.priority == 0 { Role::Muted } else { Role::Text })
                }
                Field::State => text_value(form.state.as_ref().map(|s| s.name.clone()).unwrap_or_default(), "the team's default"),
                Field::Assignee => text_value(form.assignee.as_ref().map(|p| p.name.clone()).unwrap_or_default(), "unassigned"),
                Field::Labels => text_value(form.labels.iter().map(|l| l.name.clone()).collect::<Vec<_>>().join(", "), "none"),
                Field::Title | Field::Description => (String::new(), Role::Text),
            };
            match f {
                Field::Title => {
                    let mut t = form.title.clone();
                    t.focused = form.editing && here;
                    t.render(Rect::new(vx, y, fw, 1), c);
                    y += 1;
                }
                Field::Description => {
                    let mut t = form.description.clone();
                    t.focused = form.editing && here;
                    let rows = if form.editing && here { (h.saturating_sub(y + 10)).clamp(3, 12) } else { 3 };
                    t.render(Rect::new(vx, y, fw, rows), c);
                    y += rows;
                }
                _ => {
                    put(c, vx, y, fw, &value, role);
                    if matches!(f, Field::State | Field::Assignee | Field::Labels) && form.team.is_some() && form.meta.is_none() {
                        let why = form.meta_failed.as_deref().unwrap_or("  ...loading the team");
                        put(c, vx + ui::width(&value) + 1, y, fw.saturating_sub(ui::width(&value) + 1), why, if form.meta_failed.is_some() { Role::Bad } else { Role::Muted });
                    }
                    y += 1;
                }
            }
            if here && !form.editing {
                c.focus(y - 1, 0..w);
            }
            y += 1;
        }
    }
}

/// A due date typed: `YYYY-MM-DD` and a real day, or empty to clear it.
pub fn parse_due(text: &str) -> Result<Option<String>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("\"{}\" isn't a date: write it as 2026-12-31", t.chars().take(20).collect::<String>());
    let parts: Vec<&str> = t.split('-').collect();
    let [y, m, d] = parts.as_slice() else { return Err(bad()) };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 || !(y.bytes().chain(m.bytes()).chain(d.bytes())).all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let (year, month, day): (u32, u32, u32) = (y.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if day == 0 || day > days {
        return Err(bad());
    }
    Ok(Some(t.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_linear_client::model::{self, RawIssue};

    fn ctx() -> Ctx {
        Ctx { today: "2026-11-14".into(), settings_keys: "SPC ,".into(), ready: true }
    }

    fn issue(key: &str, title: &str) -> Detail {
        let raw: RawIssue = serde_json::from_value(serde_json::json!({
            "id": format!("id-{key}"), "identifier": key, "title": title, "priority": 2, "updatedAt": "2026-11-13T10:00:00.000Z", "dueDate": "2026-11-10", "branchName": format!("jo/{}", key.to_lowercase()),
            "state": {"id": "s1", "name": "In Progress", "type": "started"}, "team": {"id": "t1", "key": "ENG", "name": "Engineering"},
            "assignee": {"id": "u1", "name": "Jo", "isMe": true}, "labels": {"nodes": [{"id": "l1", "name": "bug"}]},
            "description": "Some *words*, in Markdown.", "comments": {"nodes": [{"id": "c1", "body": "First!", "createdAt": "2026-11-12T10:00:00.000Z", "user": {"name": "Amy"}}]}
        }))
        .unwrap();
        model::detail(raw).unwrap()
    }

    fn teams() -> Vec<TeamInfo> {
        vec![TeamInfo { id: "t1".into(), key: "ENG".into(), name: "Engineering".into() }, TeamInfo { id: "t2".into(), key: "OPS".into(), name: "Operations".into() }]
    }

    fn listed(issues: Vec<Detail>, next: Option<&str>) -> Listed {
        Listed { issues, next: next.map(Into::into) }
    }

    /// A page with teams and a list loaded, the cursor on the issues.
    fn page() -> LinearPage {
        let mut p = LinearPage::new(vec![]);
        p.set_teams(Ok(teams()));
        assert_eq!(p.wants_fetch(), Some(ViewId::Mine));
        p.list_loaded(&ViewId::Mine, None, Ok(listed(vec![issue("ENG-12", "Fix the login"), issue("ENG-15", "Write the docs")], Some("c2"))));
        p.key(Key::Char('l'), &ctx());
        p
    }

    fn key(p: &mut LinearPage, k: Key) -> Action {
        p.key(k, &ctx())
    }

    fn keys(p: &mut LinearPage, text: &str) -> Action {
        let mut last = Action::None;
        for c in text.chars() {
            last = key(p, Key::Char(c));
        }
        last
    }

    #[test]
    fn the_views_are_yours_each_teams_and_a_typed_filter() {
        let v = views(&teams(), &[], Some("team:ENG bug"));
        assert_eq!(v.iter().map(|d| format!("{}/{}", d.group, d.name)).collect::<Vec<_>>(), ["Me/My issues", "Me/Created by me", "ENG/Active cycle", "ENG/Backlog", "OPS/Active cycle", "OPS/Backlog", "Filter/team:ENG bug"]);
        let only = views(&teams(), &["ops".into()], None);
        assert_eq!(only.len(), 4, "narrowed to the teams in the setting: {only:?}");
    }

    #[test]
    fn moving_between_views_fetches_each_once_and_the_issues_page_on_demand() {
        let mut p = LinearPage::new(vec![]);
        p.set_teams(Ok(teams()));
        assert_eq!(p.wants_fetch(), Some(ViewId::Mine));
        assert_eq!(p.wants_fetch(), None, "asked once");
        assert_eq!(key(&mut p, Key::Char('j')), Action::Fetch { view: ViewId::Created, after: None });
        assert_eq!(key(&mut p, Key::Char('j')), Action::Fetch { view: ViewId::Cycle("ENG".into()), after: None });
        assert_eq!(key(&mut p, Key::Char('k')), Action::None, "Created is already asked for");
        p.at = 0;
        p.list_loaded(&ViewId::Mine, None, Ok(listed(vec![issue("ENG-1", "a"), issue("ENG-2", "b")], Some("cursor-9"))));
        key(&mut p, Key::Char('l'));
        assert_eq!(key(&mut p, Key::Char('j')), Action::None);
        // Past the last row: the next page, once.
        assert_eq!(key(&mut p, Key::Char('j')), Action::Fetch { view: ViewId::Mine, after: Some("cursor-9".into()) });
        assert_eq!(key(&mut p, Key::Char('j')), Action::None, "while it's coming");
        p.list_loaded(&ViewId::Mine, Some("cursor-9"), Ok(listed(vec![issue("ENG-3", "c")], None)));
        assert_eq!(p.issues().len(), 3, "added after the others");
        key(&mut p, Key::Char('j'));
        assert_eq!(p.selected().unwrap().issue.key, "ENG-3");
        assert_eq!(key(&mut p, Key::Char('j')), Action::None, "no more");
        // A page that fails keeps what's there and says so.
        p.lists.insert(ViewId::Mine, Loaded::Ready { issues: p.issues().to_vec(), next: Some("c".into()), more: true });
        p.list_loaded(&ViewId::Mine, Some("c"), Err("offline".into()));
        assert_eq!((p.issues().len(), p.note.clone()), (3, Some(("offline".into(), true))));
        // A first page that fails is the view's failure, and R asks again.
        p.at = 1;
        p.list_loaded(&ViewId::Created, None, Err("Linear didn't accept your API key".into()));
        assert!(matches!(p.lists.get(&ViewId::Created), Some(Loaded::Failed(_))));
        key(&mut p, Key::Char('h'));
        assert_eq!(key(&mut p, Key::Char('R')), Action::Fetch { view: ViewId::Created, after: None });
    }

    #[test]
    fn an_issue_opens_on_its_own_page_with_everything_it_has() {
        let mut p = page();
        assert_eq!(key(&mut p, Key::Enter), Action::Detail("ENG-12".into()));
        assert_eq!(p.screen, Screen::Issue("ENG-12".into()));
        let mut d = issue("ENG-12", "Fix the login");
        d.cycle = Some("Cycle 12: Hardening".into());
        d.project_name = Some("Auth".into());
        d.parent = Some(("ENG-3".into(), "Revamp login".into()));
        d.children = vec![model::Child { key: "ENG-14".into(), title: "Add a retry".into(), state: "Todo".into(), stage: "todo".into() }];
        d.creator = Some("Amy".into());
        p.detail_loaded("ENG-12", Ok(d));
        let screen = p.layout(&ctx(), 100, 40).text();
        for want in ["Fix the login", "ENG-12", "In Progress  (started)", "High", "Jo", "2026-11-10", "bug", "Cycle 12: Hardening", "Auth", "ENG-3  Revamp login", "Sub-issues (1)", "ENG-14", "Some *words*, in Markdown.", "Comments (1)", "Amy", "First!", "jo/eng-12"] {
            assert!(screen.contains(want), "{want:?} in\n{screen}");
        }
        // Esc goes back to the list; j and k scroll the page.
        key(&mut p, Key::Char('j'));
        assert_eq!(p.scroll, 1);
        key(&mut p, Key::Escape);
        assert_eq!(p.screen, Screen::List);
        // One already in the agenda opens its task.
        p.in_agenda.insert("ENG-12".into());
        assert_eq!(key(&mut p, Key::Enter), Action::OpenTask("ENG-12".into()));
        assert_eq!(key(&mut p, Key::Char('l')), Action::None, "l is its own page even then (its detail is here already)");
        assert_eq!(p.screen, Screen::Issue("ENG-12".into()));
    }

    #[test]
    fn an_issue_answers_the_same_keys_on_the_list_and_on_its_page() {
        let mut p = page();
        for on_page in [false, true] {
            if on_page {
                key(&mut p, Key::Char('l'));
            }
            assert_eq!(key(&mut p, Key::Char('s')), Action::States("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('A')), Action::People("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('C')), Action::Comment("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('E')), Action::EditDescription("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('b')), Action::Branch { key: "ENG-12".into(), confirmed: false });
            assert_eq!(key(&mut p, Key::Char('y')), Action::CopyUrl("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('Y')), Action::CopyIdentifier("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('B')), Action::CopyBranch("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('o')), Action::Browser("ENG-12".into()));
            assert_eq!(key(&mut p, Key::Char('a')), Action::AddToAgenda { key: "ENG-12".into(), clock: false });
            assert_eq!(key(&mut p, Key::Char('t')), Action::AddToAgenda { key: "ENG-12".into(), clock: true });
            assert_eq!(key(&mut p, Key::Char('e')), Action::EditTitle { key: "ENG-12".into(), current: "Fix the login".into() });
            assert_eq!(key(&mut p, Key::Char('u')), Action::EditDue { key: "ENG-12".into(), current: "2026-11-10".into() });
        }
    }

    #[test]
    fn a_state_is_picked_from_a_menu_and_nothing_is_changed_before() {
        let mut p = page();
        key(&mut p, Key::Char('s'));
        p.offer(Purpose::State("ENG-12".into()), "Move ENG-12", vec![("Todo  (unstarted)".into(), "s0".into()), ("In Progress  (started)".into(), "s1".into()), ("Done  (completed)".into(), "s2".into())], Some("s1"));
        assert_eq!(p.menu.as_ref().unwrap().at, 1, "the one it's in");
        assert!(p.layout(&ctx(), 100, 30).text().contains("Move ENG-12"));
        assert_eq!(key(&mut p, Key::Char('j')), Action::None);
        assert_eq!(key(&mut p, Key::Enter), Action::SetState { key: "ENG-12".into(), id: "s2".into(), name: "Done".into() });
        assert!(p.menu.is_none());
        // Esc puts it away and does nothing.
        p.offer(Purpose::State("ENG-12".into()), "Move", vec![("Todo".into(), "s0".into())], None);
        assert_eq!(key(&mut p, Key::Escape), Action::None);
        assert!(p.menu.is_none());
        // A digit picks the row.
        p.offer(Purpose::State("ENG-12".into()), "Move", vec![("Todo".into(), "s0".into()), ("Done".into(), "s2".into())], None);
        assert_eq!(key(&mut p, Key::Char('2')), Action::SetState { key: "ENG-12".into(), id: "s2".into(), name: "Done".into() });
    }

    #[test]
    fn priority_is_picked_from_linears_five_and_people_from_the_teams() {
        let mut p = page();
        assert_eq!(key(&mut p, Key::Char('p')), Action::None);
        let m = p.menu.as_ref().unwrap();
        assert_eq!(m.items.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["Urgent", "High", "Medium", "Low", "No priority"]);
        assert_eq!(m.at, 1, "High is where it is");
        assert_eq!(key(&mut p, Key::Char('1')), Action::SetPriority { key: "ENG-12".into(), number: 1, name: "Urgent".into() });
        p.offer(Purpose::Assign("ENG-12".into()), "Assign", vec![("Unassigned".into(), String::new()), ("Amy Park".into(), "u2".into())], Some("u1"));
        assert_eq!(key(&mut p, Key::Char('2')), Action::Assign { key: "ENG-12".into(), person: Some(Person { id: "u2".into(), name: "Amy Park".into() }) });
        p.offer(Purpose::Assign("ENG-12".into()), "Assign", vec![("Unassigned".into(), String::new()), ("Amy Park".into(), "u2".into())], None);
        assert_eq!(key(&mut p, Key::Enter), Action::Assign { key: "ENG-12".into(), person: None });
    }

    #[test]
    fn the_typed_filter_makes_a_view_and_says_what_it_cant_read() {
        let mut p = page();
        key(&mut p, Key::Char('/'));
        assert!(p.typing(), "the field has the keyboard");
        keys(&mut p, "colour:red");
        assert_eq!(key(&mut p, Key::Enter), Action::None);
        assert!(p.note.as_ref().unwrap().0.contains("colour"), "{:?}", p.note);
        assert!(p.typing_filter.is_some(), "still there to fix");
        for _ in 0..10 {
            key(&mut p, Key::Backspace);
        }
        keys(&mut p, "team:ENG state:started");
        assert_eq!(key(&mut p, Key::Enter), Action::Fetch { view: ViewId::Typed("team:ENG state:started".into()), after: None });
        assert_eq!(p.current().group, "Filter");
        assert!(!p.typing());
        // Esc leaves it.
        key(&mut p, Key::Char('/'));
        assert_eq!(p.typing_filter.as_ref().unwrap().text(), "team:ENG state:started", "what it was, to change");
        key(&mut p, Key::Escape);
        assert!(p.typing_filter.is_none());
    }

    #[test]
    fn a_new_issue_is_a_form_filled_and_then_deliberately_sent() {
        let mut p = page();
        // From a view with no team: the teams are asked for.
        assert_eq!(key(&mut p, Key::Char('n')), Action::TeamMeta("t1".into()), "the selected issue's team");
        assert_eq!(p.form.as_ref().unwrap().team.as_ref().unwrap().key, "ENG");
        let meta = TeamMeta {
            team: teams()[0].clone().into_meta(),
            states: vec![StateInfo { id: "s0".into(), name: "Todo".into(), kind: "unstarted".into() }],
            people: vec![Person { id: "u1".into(), name: "Jo".into() }],
            labels: vec![LabelInfo { id: "l1".into(), name: "bug".into() }, LabelInfo { id: "l2".into(), name: "docs".into() }],
        };
        p.meta_loaded("t1", Ok(meta));
        // On the title (the form starts there, the team known): type it.
        assert_eq!(p.form.as_ref().unwrap().at(), Field::Title);
        key(&mut p, Key::Enter);
        assert!(p.typing());
        keys(&mut p, "A \"quoted\" {title}");
        key(&mut p, Key::Enter);
        assert!(!p.typing());
        // S with what's missing says so; with a title, sends.
        key(&mut p, Key::Char('j'));
        key(&mut p, Key::Enter);
        keys(&mut p, "Words");
        key(&mut p, Key::Enter);
        key(&mut p, Key::Escape);
        key(&mut p, Key::Char('j'));
        assert_eq!(p.form.as_ref().unwrap().at(), Field::Priority);
        key(&mut p, Key::Char('l'));
        key(&mut p, Key::Char('l'));
        assert_eq!(p.form.as_ref().unwrap().priority, 2, "Urgent, then High");
        key(&mut p, Key::Char('j'));
        key(&mut p, Key::Enter);
        key(&mut p, Key::Char('2'));
        assert_eq!(p.form.as_ref().unwrap().state.as_ref().unwrap().name, "Todo");
        key(&mut p, Key::Char('j'));
        key(&mut p, Key::Enter);
        key(&mut p, Key::Char('2'));
        assert_eq!(p.form.as_ref().unwrap().assignee.as_ref().unwrap().name, "Jo");
        key(&mut p, Key::Char('j'));
        key(&mut p, Key::Enter);
        key(&mut p, Key::Char(' '));
        key(&mut p, Key::Enter);
        assert_eq!(p.form.as_ref().unwrap().labels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["bug"]);
        let screen = p.layout(&ctx(), 100, 30).text();
        for want in ["A \"quoted\" {title}", "Words", "High", "Todo", "Jo", "bug", "ENG  Engineering"] {
            assert!(screen.contains(want), "{want:?} in\n{screen}");
        }
        let Action::Create(new) = key(&mut p, Key::Char('S')) else { panic!("{:?}", p.note) };
        assert_eq!(
            new,
            NewIssue { team_id: "t1".into(), title: "A \"quoted\" {title}".into(), description: "Words".into(), priority: Some(2), label_ids: vec!["l1".into()], assignee_id: Some("u1".into()), state_id: Some("s0".into()) }
        );
    }

    #[test]
    fn a_form_without_a_title_or_team_isnt_sent_and_esc_leaves_it() {
        let mut p = LinearPage::new(vec![]);
        assert_eq!(key(&mut p, Key::Char('n')), Action::Teams, "the teams aren't known yet");
        p.set_teams(Ok(teams()));
        assert_eq!(p.teams_for_form(), Action::None);
        assert!(matches!(&p.menu, Some(m) if m.purpose == Purpose::FormTeam && m.items.len() == 2));
        assert_eq!(key(&mut p, Key::Char('2')), Action::TeamMeta("t2".into()));
        assert_eq!(key(&mut p, Key::Char('S')), Action::None);
        assert_eq!(p.note, Some(("an issue needs a title".into(), true)));
        assert_eq!(key(&mut p, Key::Escape), Action::FormClosed(None));
        assert!(p.form.is_none());
        // The agenda's own: opened for its call, closed answers it.
        let mut p = page();
        p.start_form(Some(7), "From a task", "Its notes");
        assert_eq!(p.form.as_ref().unwrap().title.text(), "From a task");
        assert_eq!(p.form.as_ref().unwrap().description.text(), "Its notes");
        assert_eq!(key(&mut p, Key::Escape), Action::FormClosed(Some(7)));
    }

    #[test]
    fn it_asks_for_a_key_before_anything_else() {
        let p = LinearPage::new(vec![]);
        let screen = p.layout(&Ctx { ready: false, settings_keys: "C-,".into(), ..Default::default() }, 100, 20).text();
        assert!(screen.contains("personal API key") && screen.contains("(C-,)") && screen.contains("linear.token"), "{screen}");
    }

    #[test]
    fn it_lays_out_at_any_size_and_the_cursors_row_is_in_view() {
        let mut p = page();
        let many: Vec<Detail> = (1..=60).map(|n| issue(&format!("ENG-{n}"), &format!("Issue number {n}"))).collect();
        p.list_loaded(&ViewId::Mine, None, Ok(listed(many, None)));
        p.row = 45;
        let c = p.layout(&ctx(), 100, 20);
        assert!(c.text().contains("ENG-46"), "{}", c.text());
        for (w, h) in [(0, 0), (20, 5), (40, 10), (80, 24), (300, 80)] {
            let _ = p.layout(&ctx(), w, h);
            p.screen = Screen::Issue("ENG-1".into());
            p.detail_loaded("ENG-1", Ok(issue("ENG-1", "x")));
            let _ = p.layout(&ctx(), w, h);
            p.screen = Screen::List;
            p.help = true;
            let _ = p.layout(&ctx(), w, h);
            p.help = false;
        }
        // A due date past is drawn as bad, one to come is not.
        assert_eq!(due_role("2026-11-10", "2026-11-14"), Role::Bad);
        assert_eq!(due_role("2026-11-20", "2026-11-14"), Role::Text);
    }

    #[test]
    fn a_due_date_is_a_real_day_or_nothing() {
        assert_eq!(parse_due("2026-12-31"), Ok(Some("2026-12-31".into())));
        assert_eq!(parse_due("  "), Ok(None));
        assert_eq!(parse_due("2028-02-29"), Ok(Some("2028-02-29".into())));
        for bad in ["2026-02-29", "2026-13-01", "2026-1-1", "tomorrow", "2026-12-32", "2026-00-10", "20261231"] {
            assert!(parse_due(bad).is_err(), "{bad}");
        }
    }

    /// A team as a form's meta names it.
    trait IntoMeta {
        fn into_meta(self) -> TeamInfo;
    }

    impl IntoMeta for TeamInfo {
        fn into_meta(self) -> TeamInfo {
            self
        }
    }
}
