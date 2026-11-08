//! `SPC k i`, or `i` on a telecommand: the insert form. Every editable
//! argument at once (`CDF_ELTYPE` `E`), each with its default filled in
//! (`CDF_VALUE`, else `CPC_DEFVAL`, or the MIB's default value set), its
//! type, unit and what it may be; a status parameter cycles through its
//! texts with `h`/`l`; a counter (`CDF_GRPSIZE`) repeats the elements
//! after it -- fixed ones included, as the ICD counts them -- as many
//! times as its value says, `+`/`-` changing it from anywhere in its
//! group; `v` fills the form from a value set. The command, rendered
//! from the project's templates, updates under the form as values change,
//! with a warning beside any value outside what the MIB allows -- a
//! warning never stops the insert. `I` inserts it
//! where the form was opened.

use std::collections::HashMap;

use fenix_ccsds::pus::Profile;
use fenix_mib::commanding;
use fenix_mib::layout::{self, Field, Layout};
use fenix_mib::{DefRef, MibSet, Row};

use crate::mib_page::{kind_role, MibKey};
use crate::page::{fit, fit_tail, frame, wrap, Grid, Key, Page, Popup, Role};

/// How a command is written: the project's templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Templates {
    pub command: String,
    pub argument: String,
    pub separator: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    /// Open the telecommand's page.
    Open(DefRef),
    /// Insert this text.
    Insert(String),
    /// Copy the telecommand's packet, as a literal for the file.
    CopyBytes,
    /// Open the packet in the inspector.
    DecodeBytes,
}

/// A named set of values the telecommand may be sent with.
#[derive(Debug, Clone)]
struct ValueSet {
    name: String,
    description: String,
    values: Vec<(String, String)>,
}

pub struct InsertForm {
    pub key: MibKey,
    pub tc: DefRef,
    ccf: Row,
    root_label: String,
    name: String,
    description: String,
    pus: String,
    apid: String,
    layout: Layout,
    fixed: usize,
    values: HashMap<String, String>,
    value_sets: Vec<ValueSet>,
    /// The value set filling what hasn't been typed.
    value_set: Option<usize>,
    /// What the TM must show before it's sent (`PTV`).
    preconditions: Vec<String>,
    focus: usize,
    editing: bool,
    /// The texts menu, and its row.
    menu: Option<usize>,
    help: bool,
    templates: Templates,
    pub note: Option<(String, bool)>,
    /// A line being edited: the call is put back in its place.
    pub replacing: bool,
    /// The Bytes section is shown.
    pub show_bytes: bool,
    /// The sequence count built packets get.
    pub seq: u16,
}

/// What the Bytes section shows: the packet, where each value went in
/// it, and where it disagrees with the MIB's header for it.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub bytes: Vec<u8>,
    /// (field, bit from the packet's start, bits).
    pub fields: Vec<(String, usize, usize)>,
    pub header: Vec<String>,
}

/// The most fields a form lays out: counters nested in counters multiply,
/// and every key and frame goes through them all.
const MAX_SLOTS: usize = 1000;

/// The most a counter is set to from the form.
const MAX_COUNT: i64 = 999;

impl InsertForm {
    /// The form for `tc`, its values from `remembered` (a previous insert
    /// of the same telecommand) where there are some, else its default
    /// value set's, else the MIB's defaults.
    pub fn new(set: &MibSet, key: MibKey, tc: DefRef, templates: Templates, remembered: Option<&HashMap<String, String>>, apid_hex: bool) -> Self {
        let e = set.get(tc);
        let layout = Layout::of(set, tc);
        let fixed = layout.elements.iter().filter(|el| !el.asked() && !(el.kind == layout::ElementKind::Area && el.value.is_empty())).count();
        let sets = commanding::value_sets(set, tc);
        let value_set = sets.iter().position(|v| v.default);
        let value_sets = sets.iter().map(|v| ValueSet { name: v.name.clone(), description: v.description.clone(), values: v.engineering(set, tc) }).collect();
        let preconditions = commanding::preconditions(set, tc).iter().map(ToString::to_string).collect();
        let mut form = InsertForm {
            key,
            tc,
            ccf: e.row.clone(),
            root_label: set.root_label(e.root).to_string(),
            name: e.name.clone(),
            description: e.description.clone(),
            pus: format!("{},{}", e.row.clean("CCF_TYPE"), e.row.clean("CCF_STYPE")),
            apid: fenix_mib::types::apid(e.row.clean("CCF_APID"), apid_hex),
            layout,
            fixed,
            values: HashMap::new(),
            value_sets,
            value_set,
            preconditions,
            focus: 0,
            editing: false,
            menu: None,
            help: false,
            templates,
            note: None,
            replacing: false,
            show_bytes: false,
            seq: 0,
        };
        if let Some(r) = remembered {
            form.values = r.clone();
        }
        form
    }

    pub fn typing(&self) -> bool {
        self.editing
    }

    /// The telecommand's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn claims_space(&self) -> bool {
        self.editing
    }

    pub fn paste(&mut self, text: &str) {
        if self.editing {
            if let Some(f) = self.slots().get(self.focus).cloned() {
                let mut v = self.value(&f);
                v.extend(text.chars().filter(|c| !c.is_control()));
                self.set_value(&f, v);
            }
        }
    }

    /// Sets values by parameter name, in order -- a call being edited,
    /// read back from its line. A counter's value comes before the
    /// repetitions it opens, so each is placed as the form grows.
    pub fn fill(&mut self, args: &[(String, String)]) {
        self.values.clear();
        let mut used = vec![false; args.len()];
        loop {
            let slots = self.slots();
            let mut changed = false;
            for slot in &slots {
                let k = slot.key(&self.layout);
                if self.values.contains_key(&k) {
                    continue;
                }
                let name = &self.layout.elements[slot.element].name;
                if let Some(i) = (0..args.len()).find(|&i| !used[i] && &args[i].0 == name) {
                    used[i] = true;
                    self.values.insert(k, args[i].1.clone());
                    changed = true;
                    break;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Every field, as the counters' values expand the groups.
    fn slots(&self) -> Vec<Field> {
        let mut all = self.layout.fields_by(|f| layout::count_of(&self.layout.elements[f.element], &self.value(f)));
        all.truncate(MAX_SLOTS);
        all
    }

    /// What a field holds: what was typed, else its value set's, else its
    /// default.
    fn value(&self, f: &Field) -> String {
        if let Some(v) = self.values.get(&f.key(&self.layout)) {
            return v.clone();
        }
        let el = &self.layout.elements[f.element];
        if let Some(vs) = self.value_set.and_then(|i| self.value_sets.get(i)) {
            if let Some((_, v)) = vs.values.iter().find(|(p, _)| *p == el.name) {
                return v.clone();
            }
        }
        el.offered.clone()
    }

    fn set_value(&mut self, f: &Field, v: String) {
        self.values.insert(f.key(&self.layout), v);
    }

    /// Each field's `(name, value)`, in order: what the command takes.
    pub fn arguments(&self) -> Vec<(String, String)> {
        self.slots().iter().map(|s| (self.layout.elements[s.element].name.clone(), self.value(s))).collect()
    }

    /// The values as typed, to open with next time.
    pub fn values(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for s in self.slots() {
            out.insert(s.key(&self.layout), self.value(&s));
        }
        out
    }

    pub fn rendered(&self) -> String {
        let t = &self.templates;
        fenix_mib::telecommand::render_telecommand(&t.command, &t.argument, &t.separator, &self.ccf, &self.root_label, &self.arguments())
    }

    fn warnings_for(&self, slot: &Field) -> Vec<String> {
        let el = &self.layout.elements[slot.element];
        let v = self.value(slot);
        let label = slot.label(&self.layout);
        if v.trim().is_empty() {
            return vec![match self.layout.dynamic_default(slot.element) {
                Some(tm) => format!("{label}: no value -- SCOS takes it from {tm} when it's sent"),
                None => format!("{label}: no value"),
            }];
        }
        match &el.domain {
            Some(d) => d.check(&v, &Profile::default()),
            None => vec![format!("{label}: {} has no CPC definition", el.name)],
        }
    }

    pub fn warnings(&self) -> usize {
        self.slots().iter().map(|s| self.warnings_for(s).len()).sum()
    }

    fn set_focused(&mut self, value: String) {
        if let Some(f) = self.slots().get(self.focus).cloned() {
            self.set_value(&f, value);
        }
    }

    /// The counter of the group the focused field repeats in -- the field
    /// itself when it's a counter.
    fn counter_here(&self) -> Option<Field> {
        let f = self.slots().get(self.focus)?.clone();
        if self.layout.elements[f.element].is_repeater() && self.layout.elements[f.element].asked() {
            return Some(f);
        }
        let repeater = *self.layout.enclosing(f.element).first()?;
        if !self.layout.elements[repeater].asked() {
            return None;
        }
        let mut path = f.path.clone();
        path.pop();
        Some(Field { element: repeater, path })
    }

    /// One more or one fewer repetition of the group here.
    fn repeat(&mut self, by: i64) {
        let Some(c) = self.counter_here() else {
            self.note = Some(("+ - change a counter: this field isn't in a repeated group".into(), false));
            return;
        };
        let n = layout::count_of(&self.layout.elements[c.element], &self.value(&c)) as i64;
        let next = (n + by).clamp(0, MAX_COUNT);
        self.set_value(&c, next.to_string());
        let slots = self.slots();
        // Stay on the field; when it went away with its repetition, on the counter.
        if self.focus >= slots.len() {
            self.focus = slots.iter().position(|s| *s == c).unwrap_or(slots.len().saturating_sub(1));
        }
    }

    fn cycle(&mut self, by: isize) {
        let slots = self.slots();
        let Some(slot) = slots.get(self.focus) else { return };
        let el = &self.layout.elements[slot.element];
        let aliases = el.domain.as_ref().map(|d| d.aliases()).unwrap_or_default();
        if aliases.is_empty() {
            if el.is_repeater() {
                return self.repeat(by as i64);
            }
            self.note = Some(("h l change a status value or a count; Enter types this one".into(), false));
            return;
        }
        let at = aliases.iter().position(|a| *a == self.value(slot)).map(|i| i as isize).unwrap_or(-1);
        let next = (at + by).rem_euclid(aliases.len() as isize) as usize;
        self.set_focused(aliases[next].clone());
    }

    fn aliases_here(&self) -> Vec<String> {
        self.slots().get(self.focus).and_then(|s| self.layout.elements[s.element].domain.as_ref()).map(|d| d.aliases()).unwrap_or_default()
    }

    /// The next value set, or none.
    fn next_value_set(&mut self) {
        if self.value_sets.is_empty() {
            self.note = Some(("the MIB has no value sets for this telecommand".into(), false));
            return;
        }
        self.value_set = match self.value_set {
            None => Some(0),
            Some(i) if i + 1 < self.value_sets.len() => Some(i + 1),
            Some(_) => None,
        };
        // What it gives shows: what was typed over it goes.
        self.values.clear();
        let note = match self.value_set.and_then(|i| self.value_sets.get(i)) {
            Some(v) if v.description.is_empty() => format!("value set {}", v.name),
            Some(v) => format!("value set {} · {}", v.name, v.description),
            None => "no value set: the MIB's defaults".into(),
        };
        self.note = Some((note, false));
    }

    pub fn key(&mut self, key: Key) -> Action {
        self.note = None;
        if self.help {
            self.help = false;
            return Action::None;
        }
        let n = self.slots().len();
        if let Some(at) = self.menu {
            let aliases = self.aliases_here();
            match key {
                Key::Escape | Key::Char('q') => self.menu = None,
                Key::Down | Key::Char('j') => self.menu = Some((at + 1).min(aliases.len().saturating_sub(1))),
                Key::Up | Key::Char('k') => self.menu = Some(at.saturating_sub(1)),
                Key::Char(c @ '1'..='9') => {
                    if let Some(a) = aliases.get(c as usize - '1' as usize) {
                        self.set_focused(a.clone());
                        self.menu = None;
                    }
                }
                Key::Enter | Key::Space => {
                    if let Some(a) = aliases.get(at) {
                        self.set_focused(a.clone());
                    }
                    self.menu = None;
                }
                _ => {}
            }
            return Action::None;
        }
        if self.editing {
            match key {
                Key::Enter | Key::Escape => self.editing = false,
                Key::Tab | Key::Down => {
                    self.editing = false;
                    self.focus = (self.focus + 1).min(n.saturating_sub(1));
                }
                Key::BackTab | Key::Up => {
                    self.editing = false;
                    self.focus = self.focus.saturating_sub(1);
                }
                Key::Backspace => {
                    if let Some(s) = self.slots().get(self.focus).cloned() {
                        let mut v = self.value(&s);
                        v.pop();
                        self.set_value(&s, v);
                    }
                }
                Key::Char(c) => {
                    if let Some(s) = self.slots().get(self.focus).cloned() {
                        let mut v = self.value(&s);
                        v.push(c);
                        self.set_value(&s, v);
                    }
                }
                Key::Space => {
                    if let Some(s) = self.slots().get(self.focus).cloned() {
                        let v = self.value(&s) + " ";
                        self.set_value(&s, v);
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        match key {
            Key::Char('q') | Key::Escape => return Action::Close,
            Key::Char('?') => self.help = true,
            Key::Down | Key::Char('j') | Key::Tab => self.focus = (self.focus + 1).min(n.saturating_sub(1)),
            Key::Up | Key::Char('k') | Key::BackTab => self.focus = self.focus.saturating_sub(1),
            Key::Char('g') => self.focus = 0,
            Key::Char('G') => self.focus = n.saturating_sub(1),
            Key::Left | Key::Char('h') => self.cycle(-1),
            Key::Right | Key::Char('l') => self.cycle(1),
            Key::Char('-') => self.repeat(-1),
            Key::Char('+') | Key::Char('=') => self.repeat(1),
            Key::Char('v') => self.next_value_set(),
            Key::Enter => {
                if let Some(s) = self.slots().get(self.focus).cloned() {
                    let aliases = self.aliases_here();
                    if aliases.is_empty() {
                        self.editing = true;
                    } else {
                        self.menu = Some(aliases.iter().position(|a| *a == self.value(&s)).unwrap_or(0));
                    }
                }
            }
            Key::Char('i') | Key::Char('a') => self.editing = n > 0,
            Key::Char('c') => {
                if n > 0 {
                    self.set_focused(String::new());
                    self.editing = true;
                }
            }
            Key::Char('R') => {
                self.values.clear();
                self.value_set = None;
                self.note = Some(("back to the MIB's defaults".into(), false));
            }
            Key::Char('d') => return Action::Open(self.tc),
            Key::Char('b') => self.show_bytes = !self.show_bytes,
            Key::Char('s') => {
                self.seq = (self.seq + 1) & 0x3FFF;
                self.show_bytes = true;
            }
            Key::Char('S') => self.seq = 0,
            Key::Char('y') => return Action::CopyBytes,
            Key::Char('D') => return Action::DecodeBytes,
            Key::Char('I') => return Action::Insert(self.rendered()),
            _ => {}
        }
        Action::None
    }
}

pub fn title(form: &InsertForm) -> String {
    format!("*insert: {}*", form.name)
}

/// The form, with its packet's bytes when the Bytes section is shown
/// (the host builds them from the MIB).
pub fn layout(form: &InsertForm, bytes: Option<&Result<Built, String>>, cols: usize) -> Page {
    let (left, width) = frame(cols, 150);
    let mut g = Grid::new();
    g.put(1, left, if form.replacing { "Edit a telecommand call" } else { "Insert a telecommand" }, Role::Muted);
    let mut x = g.put(2, left, "TC", kind_role()) + 2;
    x = g.put(2, x, &form.name, Role::Title) + 2;
    g.put(2, x, &fit(&form.description, width.saturating_sub(x - left + 34)), Role::Text);
    let facts = format!("PUS {} · APID {} · {}", form.pus, form.apid, form.root_label);
    g.put(2, (left + width).saturating_sub(facts.chars().count()), &facts, Role::Muted);
    let mut y = 3;
    if let Some((text, bad)) = &form.note {
        g.put(y, left, &fit(text, width), if *bad { Role::Bad } else { Role::Good });
        y += 1;
    }
    for p in &form.preconditions {
        g.put(y, left, &fit(&format!("Sent only while {p} (pre-transmission validation)"), width), Role::Warn);
        y += 1;
    }
    if let Some(v) = form.value_set.and_then(|i| form.value_sets.get(i)) {
        g.put(y, left, &fit(&format!("Values from value set {} -- v another, R none", v.name), width), Role::Muted);
        y += 1;
    } else if !form.value_sets.is_empty() {
        g.put(y, left, &fit(&format!("{} value set{} -- v fills the form from one", form.value_sets.len(), if form.value_sets.len() == 1 { "" } else { "s" }), width), Role::Muted);
        y += 1;
    }
    y += 1;
    let slots = form.slots();
    let hidden = if form.fixed > 0 { format!("{} fixed, not asked", form.fixed) } else { String::new() };
    g.heading(y, left, width, &if hidden.is_empty() { format!("Arguments · {}", slots.len()) } else { format!("Arguments · {} · {hidden}", slots.len()) });
    y += 1;
    let (xn, xd, xv, xt, xc) = (left + 1, left + 24, left + 50, left + 74, left + 96);
    if slots.is_empty() {
        g.put(y, xn, "Nothing to fill: this telecommand takes no arguments.", Role::Muted);
        y += 1;
    }
    let mut anchor = y;
    let mut last_path: Vec<usize> = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let el = &form.layout.elements[slot.element];
        // A line where each repetition starts.
        let outer = form.layout.enclosing(slot.element);
        for k in 1..=slot.path.len() {
            if last_path.len() >= k && last_path[..k] == slot.path[..k] {
                continue;
            }
            let Some(&r) = outer.get(slot.path.len() - k) else { continue };
            let counter = Field { element: r, path: slot.path[..k - 1].to_vec() };
            let total = layout::count_of(&form.layout.elements[r], &form.value(&counter));
            g.put(y, xn + (k - 1) * 2, &fit(&format!("{} · {} of {total}", counter.label(&form.layout), slot.path[k - 1]), xv - xn), Role::Accent);
            y += 1;
        }
        last_path = slot.path.clone();
        let indent = slot.path.len() * 2;
        g.put(y, xn + indent, &fit(&slot.label(&form.layout), xd - xn - indent - 1), Role::Title);
        g.put(y, xd, &fit(&el.description, xv - xd - 2), Role::Muted);
        let value = form.value(slot);
        let aliases = el.domain.as_ref().map(|d| d.aliases()).unwrap_or_default();
        let on = i == form.focus;
        let shown = if on && form.editing {
            format!("{value}▏")
        } else if !aliases.is_empty() || el.is_repeater() {
            format!("‹ {value} ›")
        } else if value.is_empty() {
            "—".to_string()
        } else {
            value.clone()
        };
        let end = g.put(y, xv, &fit_tail(&shown, xt - xv - 2), if on { Role::Title } else { Role::Text });
        if on && form.editing {
            g.panels.push((y, xv..end.max(xv + 12)));
        }
        let mut t = el.ty.as_ref().map(|t| t.type_name()).unwrap_or_default();
        if let Some(u) = el.ty.as_ref().map(|t| t.unit.as_str()).filter(|u| !u.is_empty()) {
            t = format!("{t} {u}");
        }
        if el.is_repeater() {
            t = format!("repeats next {}", el.group);
        }
        g.put(y, xt, &fit(&t, xc - xt - 2), Role::Muted);
        let warnings = form.warnings_for(slot);
        let (mark, role) = match warnings.first() {
            None => (format!("✓ {}", el.domain.as_ref().filter(|d| d.aliases().is_empty()).map(|d| d.allowed_text()).unwrap_or_default()), Role::Good),
            Some(w) => (format!("⚠ {}", w.split_once(": ").map(|(_, r)| r).unwrap_or(w)), Role::Warn),
        };
        g.put(y, xc, &fit(&mark, (left + width).saturating_sub(xc)), role);
        if on {
            g.focus(y, left..left + width);
            anchor = y;
        }
        y += 1;
    }
    y += 1;
    let warnings = form.warnings();
    let head = if warnings == 0 { "Command".to_string() } else { format!("Command · {warnings} warning{}", if warnings == 1 { "" } else { "s" }) };
    g.heading(y, left, width, &head);
    y += 1;
    let rendered = form.rendered();
    let lines = wrap(&rendered, width - 2);
    for line in if lines.is_empty() { vec![String::new()] } else { lines } {
        let e = g.put(y, left + 2, &line, Role::Title);
        g.panels.push((y, left + 1..e.max(left + 2) + 1));
        y += 1;
    }
    y += 1;
    if form.show_bytes {
        y += 1;
        match bytes {
            Some(Ok(b)) => {
                g.heading(y, left, width, &format!("Bytes · {} · sequence count {} · s next, S back to 0", b.bytes.len(), form.seq));
                y += 1;
                for (i, chunk) in b.bytes.chunks(16).enumerate() {
                    g.put(y, left + 2, &format!("{:04X}", i * 16), Role::Muted);
                    g.put(y, left + 8, &fenix_ccsds::field::hex(chunk), Role::Title);
                    y += 1;
                }
                for w in &b.header {
                    g.put(y, left + 2, &fit(&format!("⚠ {w}"), width - 2), Role::Warn);
                    y += 1;
                }
                if !b.fields.is_empty() {
                    y += 1;
                    for (name, bit, bits) in &b.fields {
                        let at = if bit % 8 == 0 { format!("octet {}", bit / 8) } else { format!("octet {} bit {}", bit / 8, bit % 8) };
                        g.put(y, left + 2, &fit(name, 22), Role::Text);
                        g.put(y, left + 26, &format!("{at} · {bits} bits"), Role::Muted);
                        y += 1;
                    }
                }
            }
            Some(Err(e)) => {
                g.heading(y, left, width, "Bytes");
                y += 1;
                g.put(y, left + 2, &fit(&format!("can't build the packet: {e}"), width - 2), Role::Bad);
                y += 1;
            }
            None => {}
        }
        y += 1;
    }
    g.put(y, left, "Inserted as text where the form was opened -- nothing is sent anywhere.", Role::Muted);

    if let Some(at) = form.menu {
        if let Some(slot) = slots.get(form.focus) {
            let el = &form.layout.elements[slot.element];
            let mut rows = vec![vec![(format!("{} · {}", slot.label(&form.layout), el.description), Role::Title)], Vec::new()];
            for (i, a) in el.domain.as_ref().map(|d| d.aliases()).unwrap_or_default().iter().enumerate() {
                let n = if i < 9 { format!("{} ", i + 1) } else { "  ".into() };
                let on = i == at;
                rows.push(vec![(n, Role::Accent), (format!("{}{a}", if on { "› " } else { "  " }), if on { Role::Title } else { Role::Text })]);
            }
            rows.push(Vec::new());
            rows.push(vec![("Enter picks · Esc leaves it".into(), Role::Muted)]);
            g.popup = Some(Popup { line: anchor, col: xv, rows });
        }
    } else if form.help {
        let rows: Vec<Vec<(String, Role)>> = [
            ("j k Tab", "move between arguments"),
            ("Enter", "type a value, or pick a status text"),
            ("i c", "type, clear and type"),
            ("h l", "the previous, next status text (or count)"),
            ("+ -", "one more, one fewer repetition of the group here"),
            ("v", "fill the form from the next value set"),
            ("R", "the MIB's defaults again"),
            ("d", "the telecommand's page"),
            ("b s S", "the packet's bytes, next sequence count, back to 0"),
            ("y D", "copy the bytes for this file, decode them"),
            ("I", "insert"),
            ("q Esc", "leave without inserting"),
        ]
        .iter()
        .map(|(k, w)| vec![(format!("{k:<14}"), Role::Accent), (w.to_string(), Role::Text)])
        .collect();
        let mut all = vec![vec![("Keys".to_string(), Role::Title)], Vec::new()];
        all.extend(rows);
        g.popup = Some(Popup { line: anchor, col: left + 4, rows: all });
    }
    let keys: &[(&str, &str)] = if form.editing {
        &[("Enter", "keep"), ("Tab", "next"), ("Esc I", "insert")]
    } else if form.menu.is_some() {
        &[("Enter", "pick"), ("Esc", "leave it")]
    } else {
        &[("Enter", "edit"), ("h l", "change"), ("+ -", "repeat"), ("v", "value set"), ("I", "insert"), ("b", "bytes"), ("y", "copy bytes"), ("D", "decode them"), ("R", "defaults"), ("d", "definition"), ("?", "all keys"), ("q", "cancel")]
    };
    g.keys(left, width, keys);
    g.finish()
}

/// The form's packet, built: its bytes, where each value went, and where
/// it disagrees with the MIB's header for it.
pub fn build(set: &MibSet, form: &InsertForm, profile: &Profile) -> Result<Built, String> {
    let args = form.arguments();
    let pkt = fenix_mib::packets::encode_tc(set, form.tc, &args, profile, form.seq)?;
    let (_, placed) = form.layout.encode(set, &args, profile)?;
    let head = (6 + fenix_ccsds::pus::tc_header_len(profile)) * 8;
    let fields = placed
        .iter()
        .map(|p| (Field { element: p.element, path: p.path.clone() }.label(&form.layout), head + p.bit, p.bits))
        .collect();
    let header = commanding::header_mismatches(set, form.tc, &pkt.bytes)
        .into_iter()
        .map(|(f, want, have)| format!("the MIB's header for it fixes {f} at {want}; the mission profile's built {have}"))
        .collect();
    Ok(Built { bytes: pkt.bytes, fields, header })
}

/// A call `line` renders, read back into `(name, value)` arguments.
///
/// When the argument template names the argument before its value
/// (`{name}={value}`, `-{name} {value}`), each argument template with the
/// name filled in is looked for, and its value runs to the template's text
/// after `{value}`, else to the separator. Otherwise (`{value}`,
/// `{value} /* {name} */`) the arguments are read by position: the text
/// the command template puts them in, after `mnemo`, split at the
/// separator, in the order of `names`.
pub fn read_arguments(line: &str, mnemo: &str, names: &[String], t: &Templates) -> Vec<(String, String)> {
    let (before, after) = t.argument.split_once("{value}").unwrap_or((t.argument.as_str(), ""));
    if !before.contains("{name}") {
        return read_positional(line, mnemo, names, t, before, after);
    }
    let sep = t.separator.trim();
    let mut found: Vec<(usize, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !seen.insert(name.clone()) {
            continue;
        }
        let prefix = before.replace("{name}", name);
        let suffix = after.replace("{name}", name);
        let mut from = 0;
        while let Some(i) = line[from..].find(&prefix) {
            let start = from + i + prefix.len();
            // The name mustn't just be the end of a longer one.
            let ok_before = from + i == 0 || !line[..from + i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            let rest = &line[start..];
            let end = if !suffix.is_empty() {
                rest.find(&suffix)
            } else {
                let stop = [sep, "]", ")", "}"].iter().filter(|s| !s.is_empty()).filter_map(|s| rest.find(s)).min();
                Some(stop.unwrap_or(rest.len()))
            };
            if let (true, Some(end)) = (ok_before, end) {
                found.push((from + i, name.clone(), rest[..end].trim().to_string()));
            }
            // `prefix` holds the name, so it's never empty: this moves on.
            from = start.max(from + 1);
            if from > line.len() || !line.is_char_boundary(from) {
                break;
            }
        }
    }
    found.sort_by_key(|f| f.0);
    found.into_iter().map(|(_, n, v)| (n, v)).collect()
}

/// The fixed text right before `{placeholder}` in `template`, back to
/// the placeholder before it, and right after it, up to the next one.
fn around<'a>(template: &'a str, placeholder: &str) -> Option<(&'a str, &'a str)> {
    let (head, tail) = template.split_once(placeholder)?;
    let lead = head.rsplit_once('}').map(|(_, l)| l).unwrap_or(head);
    let trail = tail.split_once('{').map(|(t, _)| t).unwrap_or(tail);
    Some((lead, trail))
}

/// `read_arguments` for a template that doesn't name its arguments first.
fn read_positional(line: &str, mnemo: &str, names: &[String], t: &Templates, before: &str, after: &str) -> Vec<(String, String)> {
    let Some((lead, trail)) = around(&t.command, "{arguments}") else { return Vec::new() };
    // Past the mnemonic, as a word of its own.
    let mut start = 0;
    if !mnemo.is_empty() {
        let mut from = 0;
        while let Some(i) = line[from..].find(mnemo) {
            let at = from + i;
            let end = at + mnemo.len();
            let word = |c: char| c.is_alphanumeric() || c == '_';
            if !line[..at].ends_with(word) && !line[end..].starts_with(word) {
                start = end;
                break;
            }
            from = end;
        }
    }
    let lead = lead.trim();
    if !lead.is_empty() {
        match line[start..].find(lead) {
            Some(i) => start += i + lead.len(),
            None => return Vec::new(),
        }
    }
    let trail = trail.trim();
    let end = if trail.is_empty() { line.len() } else { line[start..].find(trail).map(|i| start + i).unwrap_or(line.len()) };
    let region = line[start..end].trim();
    if region.is_empty() {
        return Vec::new();
    }
    let sep = t.separator.trim();
    let pieces: Vec<&str> = if sep.is_empty() { region.split_whitespace().collect() } else { region.split(sep).map(str::trim).collect() };
    // What the argument template writes around the value, but not a
    // name, which a positional reading can't know.
    let lead_text = before.trim();
    let trail_text = after.split("{name}").next().unwrap_or("").trim();
    names
        .iter()
        .zip(pieces)
        .map(|(name, piece)| {
            let mut v = piece.strip_prefix(lead_text).unwrap_or(piece);
            if !trail_text.is_empty() {
                if let Some(i) = v.find(trail_text) {
                    v = &v[..i];
                }
            }
            (name.clone(), v.trim().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_mib::Kind;

    fn templates() -> Templates {
        Templates { command: "tc::send {mnemo} {arguments}".into(), argument: "-{name} {value}".into(), separator: ", ".into() }
    }

    fn form_with(ccf: &str, cdf: &str, cpc: &str) -> (InsertForm, MibSet, fenix_mib::MibRoot) {
        let root = crate::mib_page::tests::fixture();
        std::fs::write(root.path.join("ccf.dat"), ccf).unwrap();
        std::fs::write(root.path.join("cdf.dat"), cdf).unwrap();
        if !cpc.is_empty() {
            std::fs::write(root.path.join("cpc.dat"), cpc).unwrap();
        }
        let set = MibSet::load(vec![root.clone()], None);
        let tc = DefRef { kind: Kind::Telecommand, index: 0 };
        (InsertForm::new(&set, MibKey { roots: vec![root.clone()], ..Default::default() }, tc, templates(), None, true), set, root)
    }

    #[test]
    fn defaults_fill_the_form_and_the_command_follows_the_values() {
        let (mut f, _set, root) = form_with(
            "ZTC08101\tSet heater control mode\t\t\tN\t\t8\t1\t1010\n",
            "ZTC08101\tF\tFunction\t8\t0\t0\tPTC00001\t\t12\t\nZTC08101\tE\tLine\t8\t8\t0\tPTH00101\t\t\t\nZTC08101\tE\tMode\t8\t16\t0\tPTH00102\t\t\t\n",
            "",
        );
        assert_eq!(f.rendered(), "tc::send ZTC08101 -PTH00101 , -PTH00102 AUTO");
        assert_eq!(f.warnings(), 1, "the line has no value yet");
        f.key(Key::Enter);
        f.key(Key::Char('3'));
        f.key(Key::Enter);
        f.key(Key::Char('j'));
        f.key(Key::Char('l'));
        assert_eq!(f.rendered(), "tc::send ZTC08101 -PTH00101 3, -PTH00102 OFF");
        assert_eq!(f.warnings(), 0);
        f.key(Key::Char('k'));
        f.key(Key::Char('c'));
        f.key(Key::Char('9'));
        f.key(Key::Escape);
        assert_eq!(f.warnings(), 1, "9 is outside 1..8");
        let text = layout(&f, None, 160).text;
        assert!(text.contains("outside"), "{text}");
        assert!(text.contains("1 FIXED, NOT ASKED"), "{text}");
        assert_eq!(f.key(Key::Char('I')), Action::Insert("tc::send ZTC08101 -PTH00101 9, -PTH00102 OFF".into()));
        std::fs::remove_dir_all(&root.path).ok();
    }

    #[test]
    fn a_counter_repeats_the_group_after_it() {
        let (mut f, _set, root) = form_with(
            "ZTC11004\tInsert activities\t\t\tN\t\t11\t4\t1008\n",
            "ZTC11004\tE\tCount\t8\t0\t2\tPN\t\t\t\nZTC11004\tE\tTime\t32\t8\t0\tPT\t\t\t\nZTC11004\tE\tWhat\t16\t40\t0\tPW\t\t\t\nZTC11004\tE\tAfter\t8\t56\t0\tPA\t\t\t\n",
            "PN\tCount\t3\t4\t\t\t\t\t\t\t\t\t0\nPT\tTime\t3\t14\nPW\tWhat\t3\t12\nPA\tAfter\t3\t4\n",
        );
        assert_eq!(f.arguments().iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), vec!["PN", "PA"]);
        f.key(Key::Char('+'));
        f.key(Key::Char('+'));
        let names: Vec<String> = f.arguments().into_iter().map(|a| a.0).collect();
        assert_eq!(names, vec!["PN", "PT", "PW", "PT", "PW", "PA"]);
        let text = layout(&f, None, 160).text;
        assert!(text.contains("PW[2]") && text.contains("PN · 2 of 2"), "the repetitions are numbered: {text}");
        f.fill(&[("PN".into(), "1".into()), ("PT".into(), "100".into()), ("PW".into(), "7".into()), ("PA".into(), "5".into())]);
        assert_eq!(f.arguments(), vec![("PN".into(), "1".into()), ("PT".into(), "100".into()), ("PW".into(), "7".into()), ("PA".into(), "5".into())]);
        std::fs::remove_dir_all(&root.path).ok();
    }

    #[test]
    fn a_group_holds_its_fixed_elements_and_plus_works_from_inside_it() {
        // PN repeats PT, a spare and PF (fixed): three elements, not the
        // next three asked. PD's default is in CDF_VALUE.
        let (mut f, set, root) = form_with(
            "ZTC11005\tGroups\t\t\tN\t\t11\t5\t1008\n",
            "ZTC11005\tE\tCount\t8\t0\t3\tPN\t\t1\t\nZTC11005\tE\tTime\t8\t8\t0\tPT\t\t\t\nZTC11005\tA\tSpare\t8\t16\t0\t\t\t0\t\nZTC11005\tF\tFix\t8\t24\t0\tPF\tR\t7\t\nZTC11005\tE\tAfter\t8\t32\t0\tPD\tE\t5\t\n",
            "PN\tCount\t3\t4\nPT\tTime\t3\t4\nPF\tFix\t3\t4\nPD\tAfter\t3\t4\n",
        );
        assert_eq!(f.arguments(), vec![("PN".into(), "1".into()), ("PT".into(), "".into()), ("PD".into(), "5".into())]);
        f.key(Key::Char('j'));
        f.key(Key::Char('+'));
        let names: Vec<String> = f.arguments().into_iter().map(|a| a.0).collect();
        assert_eq!(names, vec!["PN", "PT", "PT", "PD"]);
        f.fill(&[("PN".into(), "2".into()), ("PT".into(), "1".into()), ("PT".into(), "2".into()), ("PD".into(), "9".into())]);
        let built = build(&set, &f, &Profile::default()).unwrap();
        let app = &built.bytes[11..built.bytes.len() - 2];
        assert_eq!(app, &[2, 1, 0, 7, 2, 0, 7, 9]);
        assert!(built.fields.iter().any(|(n, bit, _)| n == "PT[2]" && *bit == 11 * 8 + 32), "{:?}", built.fields);
        std::fs::remove_dir_all(&root.path).ok();
    }

    #[test]
    fn a_rendered_call_reads_back_into_its_arguments() {
        let t = templates();
        let names: Vec<String> = ["PTH00101", "PTH00102"].iter().map(|s| s.to_string()).collect();
        let args = read_arguments("  tc::send ZTC08101 -PTH00101 3, -PTH00102 AUTO", "ZTC08101", &names, &t);
        assert_eq!(args, vec![("PTH00101".into(), "3".into()), ("PTH00102".into(), "AUTO".into())]);
        let t = Templates { command: "x ARGUMENTS=[{arguments}]".into(), argument: "{name}={value}".into(), separator: ", ".into() };
        let args = read_arguments("x ARGUMENTS=[PTH00101=3, XPTH00102=1, PTH00102=ON]", "", &names, &t);
        assert_eq!(args, vec![("PTH00101".into(), "3".into()), ("PTH00102".into(), "ON".into())]);
    }

    #[test]
    fn a_template_that_doesnt_name_its_arguments_reads_them_by_position() {
        // `{value}` alone used to look for an empty prefix forever.
        let names: Vec<String> = ["PTH00101", "PTH00102"].iter().map(|s| s.to_string()).collect();
        let t = Templates { command: "tc::send {mnemo} {arguments}".into(), argument: "{value}".into(), separator: " ".into() };
        assert_eq!(read_arguments("    tc::send ZTC08101 3 AUTO", "ZTC08101", &names, &t), vec![("PTH00101".into(), "3".into()), ("PTH00102".into(), "AUTO".into())]);
        let t = Templates { command: "send(\"{mnemo}\", [{arguments}])".into(), argument: "{value} /* {name} */".into(), separator: ", ".into() };
        assert_eq!(read_arguments("send(\"ZTC08101\", [3 /* PTH00101 */, ON /* PTH00102 */])", "ZTC08101", &names, &t), vec![("PTH00101".into(), "3".into()), ("PTH00102".into(), "ON".into())]);
        let t = Templates { command: "tc::send {mnemo} {arguments}".into(), argument: "{value}={name}".into(), separator: ", ".into() };
        assert_eq!(read_arguments("tc::send ZTC08101", "ZTC08101", &names, &t), vec![]);
    }
}
