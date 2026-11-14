//! SCOS-2000 MIBs in Fenix, as Fenix 1.x had them built in: the MIB page
//! (`SPC k k`), a definition's own page, the telecommand insert form,
//! pickers by kind, code generated from the MIB, and the MIB in your
//! scripts -- `K`, `gd`, completion and checks of telecommand calls,
//! through a language server.
//!
//! Which MIBs (`fenix_mib::which`, shared with the CCSDS extension, which
//! this one tells what's in use): those found in the project and those
//! `mib.roots` names (the project's own first, as Fenix gives it
//! settings, each folder at `/folders/mib/<name>`); in use, the one
//! picked here with `SPC k o`, else every one the settings name, else
//! one found when there's no doubt which. They're read when first needed
//! and again when their files change (`mib.watch`). How they're read
//! against the mission comes from the CCSDS settings, `ccsds.*`
//! (`permissions.settings`).
//!
//! Coding agents look definitions up with `mib_lookup` (`agent`).

pub mod keys;
mod agent;
mod mib_check;
mod mib_def;
mod mib_form;
mod mib_page;
mod page;
mod server;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::lsp::Servers;
use api::{export, host, project, Check, Event, Extension, Health, Key, Page, PickerItem, Section};
use fenix_mib::mission::{list_items, map_entries, Mission};
use fenix_mib::{DefRef, Kind, MibRoot, MibSet};

use keys::Hints;
use mib_def::DefPage;
use mib_form::InsertForm;
use mib_page::{MibKey, MibPage};
use fenix_mib::which::{self, Choice, Known};

/// The kinds of page it opens.
const MIB_KIND: &str = "mib";
const DEF_KIND: &str = "mib-definition";
const FORM_KIND: &str = "mib-insert";

/// Where the folders `mib.roots` names are, the project, and the
/// extension's own folder -- elsewhere in its tests.
fn folders_dir() -> PathBuf {
    place(1).unwrap_or_else(|| PathBuf::from("/folders/mib"))
}

fn project_dir() -> PathBuf {
    place(0).unwrap_or_else(|| PathBuf::from("/project"))
}

fn data_dir() -> PathBuf {
    place(2).unwrap_or_else(|| PathBuf::from("/data"))
}

#[cfg(not(test))]
fn place(_: usize) -> Option<PathBuf> {
    None
}

#[cfg(test)]
thread_local! {
    /// The project, the folders and the extension's own folder, in a test.
    static PLACES: std::cell::RefCell<Option<[PathBuf; 3]>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn place(which: usize) -> Option<PathBuf> {
    PLACES.with(|p| p.borrow().as_ref().map(|p| p[which].clone()))
}

/// How often the MIBs' files are looked at for a change.
const WATCH_MILLIS: u32 = 3_000;

/// What it says when there's no MIB to work with.
fn no_mib_text(open_keys: &str) -> String {
    format!("no MIB here -- none found in the project or named in the settings; {open_keys} opens one")
}

/// `no_mib_text`, with the keys of the scheme in use.
fn no_mib() -> String {
    no_mib_text(&keys::keys("mib.open", "SPC k o"))
}

/// The extension: its language servers' documents, and the MIB.
#[derive(Default)]
pub struct MibExtension {
    servers: Servers,
    mib: Mib,
}

/// One of its pages.
enum Open {
    Mib(Box<MibPage>),
    Def(DefPage),
    /// The insert form, and where its command goes.
    Form(Box<InsertForm>, Origin),
}

/// Where an insert form's command goes: the file and line it was opened
/// from, and the columns of that line it takes the place of (none, for
/// an insert at the cursor).
#[derive(Debug, Clone)]
struct Origin {
    path: Option<String>,
    /// From 1.
    line: u32,
    replace: Option<std::ops::Range<usize>>,
}

/// A telecommand call on the cursor's line: its MIBs, its definition,
/// where you are, and the arguments it's written with.
struct Call {
    set: Arc<MibSet>,
    tc: DefRef,
    here: api::Here,
    args: Vec<(String, String)>,
}

/// What a picker it opened is for.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// A definition, to open.
    Def,
    /// A telecommand, for its insert form.
    Insert,
    /// Something to generate.
    Generate,
    /// A command sequence, to insert as its telecommands.
    Sequence,
    /// Two MIBs, to compare.
    Diff,
    /// A MIB to open (`SPC k o`).
    Mib,
    /// A folder typed for "another folder…".
    Folder,
}

/// Everything it keeps between calls.
#[derive(Default)]
pub(crate) struct Mib {
    /// The MIBs read, and what they were read from.
    set: Option<(MibKey, Arc<MibSet>)>,
    pages: HashMap<u64, Open>,
    /// Its pickers, by id.
    asked: HashMap<u64, Asked>,
    /// Services it called, by id: what to say when there's none.
    calls: HashMap<u64, &'static str>,
    /// The last values used for each telecommand, by project.
    last: HashMap<(Option<PathBuf>, String), HashMap<String, String>>,
    /// The timer that looks for changed files.
    watch: Option<u64>,
    /// The timer that reads the MIBs a page asked for: a page is laid out
    /// in a second at most, and a large MIB takes longer to read, so it's
    /// read in a call of its own.
    reading: Option<u64>,
    /// Definitions `gd` asked for, opened once the language server's
    /// answer is sent.
    pub(crate) to_open: Vec<DefRef>,
    /// The MIB folders the project holds, as last looked through: at the
    /// start, and on `SPC k o`, `SPC k r` and `SPC k D` -- never while
    /// answering the language server or laying out a page.
    found: Option<which::Walk>,
    /// What's picked in this project with `SPC k o` (read once from the
    /// extension's own folder).
    choice: Option<Option<Choice>>,
    /// It said several MIBs were found and none is sure; it said the
    /// picks file was bad.
    said_undecided: bool,
    said_bad_picks: bool,
    /// Whether a MIB page waiting to be opened was looked for.
    waiting_seen: bool,
    /// The project's folder, as a full path, once Fenix has said it.
    project_path: Option<String>,
    /// Waits for the first look through the project and the doctor: not
    /// as Fenix starts.
    starting: Option<u64>,
    /// What the checks against the profile and the standards found in the
    /// MIBs read, besides what's wrong in their files.
    standards: Vec<fenix_mib::Problem>,
}

// -- Settings -----------------------------------------------------------------

/// A setting's text, when it says something.
pub(crate) fn text(key: &str) -> Option<String> {
    api::setting(key).filter(|t| !t.trim().is_empty())
}

/// A map setting's entries.
fn map(key: &str) -> Vec<(String, String)> {
    text(key).map(|t| map_entries(&t)).unwrap_or_default()
}

/// Whether a setting's text means on.
pub(crate) fn on(key: &str, default: bool) -> bool {
    match text(key).as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("true" | "on" | "yes" | "1") => true,
        Some("false" | "off" | "no" | "0") => false,
        _ => default,
    }
}

pub(crate) fn apid_hex() -> bool {
    text("mib.apid_format").as_deref().map(str::trim) != Some("decimal")
}

/// The mission profile the MIBs are read and packets built against: the
/// CCSDS settings, the project's over yours.
pub(crate) fn mission() -> Mission {
    Mission::read(api::setting, None)
}

/// How the MIBs are read: `mib.icd`, `mib.icd_per_mib`, `mib.columns`
/// and `mib.xtce_fields`.
fn read_options() -> fenix_mib::ReadOptions {
    use fenix_mib::{schema, Icd, Schema, XtceOptions};
    let issue = |t: &str| if t.trim() == "auto" { None } else { Some(Icd::parse(t).unwrap_or(Icd::LATEST)) };
    let schema = Schema {
        icd: text("mib.icd").map_or(Some(Icd::LATEST), |t| issue(&t)),
        per_root: map("mib.icd_per_mib").into_iter().map(|(label, t)| (label, issue(&t))).collect(),
        columns: map("mib.columns").into_iter().map(|(t, c)| (t.trim().to_ascii_lowercase(), schema::parse_columns(&c))).collect(),
    };
    let fields = map("mib.xtce_fields");
    let names = |field: &str| -> Vec<String> {
        fields.iter().filter(|(f, _)| f.trim().eq_ignore_ascii_case(field)).flat_map(|(_, v)| v.split([',', ' ']).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect::<Vec<_>>()).collect()
    };
    let xtce = XtceOptions { apid: names("apid"), service: names("service"), subtype: names("subtype"), pi1: names("pi1"), pi2: names("pi2") };
    fenix_mib::ReadOptions { schema, xtce }
}

/// How a command is written: the project's templates, else yours, else
/// Fenix's.
pub(crate) fn templates() -> mib_form::Templates {
    use fenix_mib::telecommand::{DEFAULT_ARGUMENT_SEPARATOR, DEFAULT_ARGUMENT_TEMPLATE, DEFAULT_TEMPLATE};
    mib_form::Templates {
        command: text("mib.telecommand_template").unwrap_or_else(|| DEFAULT_TEMPLATE.into()),
        argument: text("mib.telecommand_argument_template").unwrap_or_else(|| DEFAULT_ARGUMENT_TEMPLATE.into()),
        separator: api::setting("mib.telecommand_argument_separator").filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_ARGUMENT_SEPARATOR.into()),
    }
}

/// The name of the project you're in.
fn project_name() -> String {
    api::here().and_then(|h| h.project).map(|p| p.name).unwrap_or_else(|| "the project".into())
}

/// Whether the project's own settings list MIBs (`[mib]` `roots`).
fn project_lists_roots() -> bool {
    let Ok(text) = std::fs::read_to_string(project_dir().join(".fenix").join("settings.toml")) else { return false };
    let Ok(table) = text.parse::<toml::Table>() else { return false };
    table.get("mib").and_then(|m| m.as_table()).and_then(|m| m.get("roots")).and_then(|r| r.as_table()).is_some_and(|r| !r.is_empty())
}

/// What's picked with `SPC k o`, by project's full path: in the
/// extension's own folder, never in a project's settings.
const CHOICES: &str = "choices.json";

/// The picks file, and what to say of it: a file that isn't what it
/// should be is put aside (`choices.json.bad`) rather than written over,
/// so the other projects' picks can be had back from it.
fn read_choices() -> (serde_json::Map<String, serde_json::Value>, Option<String>) {
    let path = data_dir().join(CHOICES);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Default::default(), None),
        Err(e) => return (Default::default(), Some(format!("couldn't read the MIBs picked in each project ({e})"))),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(all)) => (all, None),
        _ => {
            let aside = data_dir().join(format!("{CHOICES}.bad"));
            let kept = std::fs::rename(&path, &aside).is_ok();
            let note = if kept { format!("the MIBs picked in each project couldn't be read -- the file is kept as {}, and they're picked afresh", aside.display()) } else { "the MIBs picked in each project couldn't be read".to_string() };
            (Default::default(), Some(note))
        }
    }
}

/// The picks file written whole, or not at all: to a file beside it, then
/// put in its place.
fn write_choices(all: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let new = dir.join(format!("{CHOICES}.new"));
    std::fs::write(&new, serde_json::Value::Object(all.clone()).to_string()).and_then(|()| std::fs::rename(&new, dir.join(CHOICES))).map_err(|e| format!("couldn't keep which MIB is open here: {e}"))
}

/// Whether the file at `path` is one where MIB names mean something
/// (`mib.editor_files`; none listed, every file).
pub(crate) fn names_file(path: &str) -> bool {
    let exts = text("mib.editor_files").map(|t| list_items(&t)).unwrap_or_default();
    if exts.is_empty() {
        return true;
    }
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    exts.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

// -- Pickers ------------------------------------------------------------------

/// A definition as a picker's value: its kind and index.
fn def_value(def: DefRef) -> String {
    let kind = Kind::ALL.iter().position(|k| *k == def.kind).unwrap_or(0);
    format!("{kind}:{}", def.index)
}

fn def_from(value: &str) -> Option<DefRef> {
    let (kind, index) = value.split_once(':')?;
    Some(DefRef { kind: *Kind::ALL.get(kind.parse::<usize>().ok()?)?, index: index.parse().ok()? })
}

/// The rows of a picker of `kinds`: name, columns, MIB and description.
fn picker_rows(set: &MibSet, kinds: &[Kind]) -> Vec<PickerItem> {
    let hex = apid_hex();
    let many = set.roots().len() > 1;
    let mut out = Vec::new();
    for &kind in kinds {
        for (index, e) in set.entries(kind).iter().enumerate() {
            let def = DefRef { kind, index };
            let cols: Vec<String> = (1..kind.headers().len()).map(|c| mib_page::cell(set, def, c, hex)).filter(|c| !c.is_empty()).collect();
            let mib = if many { format!("[{}]  ", set.root_label(e.root)) } else { String::new() };
            let tag = if kinds.len() > 1 { format!("{:<4}", kind.tag()) } else { String::new() };
            let label = format!("{tag}{:<10}  {:<24}  {mib}{}", e.name, cols.join("  "), e.description);
            out.push(PickerItem::new(label, def_value(def)));
        }
    }
    out
}

/// A MIB's row in the `SPC k o` picker: `●` on the one in use, its name
/// (a found one's path in the project), where it's from and what's in it.
fn mib_row(k: &Known, on: bool) -> PickerItem {
    let from = match &k.origin {
        which::Origin::Found(_) => "found in the project".to_string(),
        which::Origin::Project => "this project's settings".to_string(),
        which::Origin::Yours => "your settings".to_string(),
    };
    let detail = match &k.origin {
        which::Origin::Found(rel) if !rel.is_empty() => k.what.clone(),
        _ => format!("{} · {}", k.what, k.place),
    };
    PickerItem::new(format!("{} {:<28} {from}", if on { "●" } else { " " }, k.label), k.id.clone()).detail(detail)
}

/// What it says of a folder in your settings Fenix hasn't given the
/// extension yet: it gives it the folders `mib.roots` names as it starts
/// it.
fn not_reachable(label: &str) -> String {
    format!("{label}'s MIB page opens as soon as Fenix gives the extension its folder: when it starts the extension again -- at once on a Fenix that does so for a folder setting, else the next time Fenix starts")
}

/// `bytes`, written the way a file with extension `ext` writes bytes,
/// on the clipboard.
fn copy_literal(bytes: &[u8], ext: &str, what: &str) {
    api::copy(&fenix_ccsds::hex::literal(bytes, ext));
    api::notify(&format!("copied {what}'s packet, {} bytes", bytes.len()));
}

pub(crate) fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

fn copy(text: &str) {
    api::copy(text);
    api::notify(&format!("Copied {text}"));
}

fn open_file(path: &Path, line: usize) {
    host::open_file(&path.to_string_lossy(), Some(line as u32));
}

/// The settings page: this project's (`SPC k ,`), or yours.
fn open_settings(project: bool) {
    host::run_command(if project && project_dir().is_dir() { "settings.this_project" } else { "settings.open" });
}

// -- Which MIBs -----------------------------------------------------------------

impl Mib {
    /// The project's folder as a full path: what a setting naming a folder
    /// in it may say, and what's picked here is kept by.
    fn project_path(&mut self) -> Option<String> {
        if self.project_path.is_none() {
            self.project_path = api::here().and_then(|h| h.project).map(|p| p.root);
        }
        self.project_path.clone()
    }

    /// The picks file, said of once when it was bad.
    fn choices(&mut self) -> serde_json::Map<String, serde_json::Value> {
        let (all, note) = read_choices();
        if let Some(note) = note.filter(|_| !self.said_bad_picks) {
            self.said_bad_picks = true;
            error(&note);
        }
        all
    }

    /// What's picked in this project with `SPC k o`, if anything. Not
    /// known -- and not remembered as nothing -- before Fenix has said
    /// which project this is.
    fn choice(&mut self) -> Option<Choice> {
        if self.choice.is_none() {
            let project = self.project_path()?;
            let picked = self.choices().get(&project).and_then(|c| c.get("choice")).and_then(|c| c.as_str()).map(Choice::parse);
            self.choice = Some(picked);
        }
        self.choice.clone().flatten()
    }

    /// `choice` picked in this project, kept for the next time; `open`,
    /// its MIB page to open once it can be read, when it can't yet.
    /// Whether it could be: not without a project.
    fn pick(&mut self, choice: Choice, open: bool) -> bool {
        let Some(project) = self.project_path() else {
            error("open a file of the project first -- a MIB is picked for the project it's in");
            return false;
        };
        let mut all = self.choices();
        all.insert(project, serde_json::json!({ "choice": choice.text(), "open": open }));
        if let Err(e) = write_choices(&all) {
            error(&e);
        }
        self.choice = Some(Some(choice));
        self.tell_ccsds();
        true
    }

    /// Whether a MIB page waits to be opened here, and no longer.
    fn take_waiting_page(&mut self) -> bool {
        let Some(project) = self.project_path() else { return false };
        let mut all = self.choices();
        let waiting = all.get(&project).and_then(|c| c.get("open")).and_then(|o| o.as_bool()).unwrap_or(false);
        if waiting {
            if let Some(c) = all.get_mut(&project).and_then(|c| c.as_object_mut()) {
                c.insert("open".into(), false.into());
            }
            if let Err(e) = write_choices(&all) {
                error(&e);
            }
        }
        waiting
    }

    /// The project looked through for MIB folders, now.
    fn look(&mut self) -> which::Walk {
        let project = project_dir();
        let walk = if project.is_dir() { which::walk(&project, which::BOUNDS) } else { which::Walk::default() };
        self.found = Some(walk.clone());
        walk
    }

    /// What the settings name: `mib.roots` as Fenix gives it -- what the
    /// extension set itself included -- and whether it's the project's.
    fn named(&self) -> (Vec<(String, String)>, bool) {
        (map("mib.roots"), project_lists_roots())
    }

    /// The MIBs known here, from what was found (`walk`).
    fn known_from(&mut self, walk: &which::Walk) -> Vec<Known> {
        let (named, project) = self.named();
        let project_path = self.project_path();
        let picked = self.choice();
        let (project_dir, folders_dir, name) = (project_dir(), folders_dir(), project_name());
        let places = which::Places { project: &project_dir, folders: &folders_dir, project_path: project_path.as_deref(), project_name: &name };
        which::known(&walk.found, &named, project, picked.as_ref().and_then(Choice::found), &places)
    }

    /// The MIBs known here, as the project was last looked through.
    pub(crate) fn known(&mut self) -> Vec<Known> {
        let walk = self.found.clone().unwrap_or_default();
        self.known_from(&walk)
    }

    /// The MIBs in use, of `known`, as the set they're read into is looked
    /// up by -- and where they come from, for the MIB page to say.
    fn key_with(&mut self, known: &[Known]) -> (MibKey, String) {
        let choice = self.choice();
        let in_use = which::in_use(known, choice.as_ref());
        let roots: Vec<MibRoot> = in_use.used.iter().filter_map(|k| k.root()).collect();
        let source = which::source(&in_use, &project_name(), known.len());
        let project = project_dir().is_dir().then(project_dir);
        (MibKey { roots, default: text("mib.default"), project, options: read_options() }, source)
    }

    /// The MIBs in use here.
    pub(crate) fn key_here(&mut self) -> (MibKey, String) {
        let known = self.known();
        self.key_with(&known)
    }

    /// How many MIBs were found with none in use, none being sure.
    fn undecided(&mut self) -> usize {
        let known = self.known();
        let choice = self.choice();
        which::in_use(&known, choice.as_ref()).undecided
    }

    /// The MIBs in use as the CCSDS extension takes them (`mib.which`,
    /// and `ccsds.mib` when they change): each one's name and where it
    /// is -- the same in its sandbox, `/project/…`, `/folders/mib/…`.
    fn which_answer(&mut self) -> serde_json::Value {
        let (key, _) = self.key_here();
        let roots: Vec<(String, String)> = key.roots.iter().map(|r| (r.label.clone(), r.path.to_string_lossy().into_owned())).collect();
        serde_json::json!({ "roots": roots })
    }

    /// The CCSDS extension told what's in use now; nothing comes of it
    /// when it isn't there.
    fn tell_ccsds(&mut self) {
        let payload = self.which_answer().to_string();
        let _ = api::call_service("ccsds.mib", &payload);
    }
}

// -- The MIB ------------------------------------------------------------------

impl Mib {
    /// The MIBs here, read -- now, if they weren't, or if what they're
    /// read from changed.
    pub(crate) fn set(&mut self) -> Option<Arc<MibSet>> {
        let (key, _) = self.key_here();
        if key.roots.is_empty() {
            return None;
        }
        match &self.set {
            Some((k, set)) if *k == key => Some(set.clone()),
            _ => Some(self.load(key)),
        }
    }

    /// The MIBs, only if they're read: for what never waits.
    pub(crate) fn set_ready(&self) -> Option<Arc<MibSet>> {
        self.set.as_ref().map(|(_, s)| s.clone())
    }

    /// For a page: the MIBs here if they're read; else what was read, and
    /// they're read in a call of their own, the page shown again then.
    fn set_for_page(&mut self) -> Option<Arc<MibSet>> {
        let (key, _) = self.key_here();
        if key.roots.is_empty() {
            return None;
        }
        if !matches!(&self.set, Some((k, _)) if *k == key) && self.reading.is_none() {
            self.reading = Some(host::set_timer(1, false));
        }
        self.set_ready()
    }

    /// The reading timer: the MIBs here, read.
    fn read_now(&mut self) {
        self.reading = None;
        let _ = self.set();
        self.refresh_pages();
    }

    /// Reads `key`'s MIBs, checked against the mission; every page shows
    /// them.
    fn load(&mut self, key: MibKey) -> Arc<MibSet> {
        let mission = mission();
        let mut set = MibSet::load_with(key.roots.clone(), key.default.as_deref(), key.options.clone());
        let options = fenix_mib::checks::Options { disabled: &mission.checks_off, base: mission.base, profile: &mission.profile };
        let found = fenix_mib::checks::run(&set, &options);
        self.standards = found.clone();
        set.add_problems(found);
        let set = Arc::new(set);
        let again = self.set.is_some();
        self.set = Some((key, set.clone()));
        if again {
            let problems = set.problems().len();
            if problems > 0 {
                api::notify(&format!("MIB read again -- {problems} problem{} (! on the MIB page)", if problems == 1 { "" } else { "s" }));
            } else {
                api::notify("MIB read again");
            }
        }
        self.refresh_pages();
        self.watch();
        set
    }

    /// Looks for changed files every few seconds, while `mib.watch` is on.
    fn watch(&mut self) {
        let wanted = on("mib.watch", true) && self.set.is_some();
        match (wanted, self.watch) {
            (true, None) => self.watch = Some(host::set_timer(WATCH_MILLIS, true)),
            (false, Some(timer)) => {
                host::cancel_timer(timer);
                self.watch = None;
            }
            _ => {}
        }
    }

    /// The watch timer: a set whose files changed is read again. A MIB
    /// on a volume that isn't answering keeps what was read.
    fn watched(&mut self) {
        let Some((key, set)) = &self.set else { return };
        if !key.roots.iter().all(|r| r.path.is_dir()) {
            return;
        }
        if fenix_mib::Stamp::of(&key.roots) != *set.stamp() {
            let key = key.clone();
            self.load(key);
        }
    }

    fn refresh_pages(&self) {
        for (&id, open) in &self.pages {
            if let Open::Def(p) = open {
                host::page_retitle(id, &mib_def::title(self.set_ready().as_deref(), p));
            }
            api::page::refresh(id);
        }
    }

    /// The set here, or a word about why there's none.
    fn set_here(&mut self) -> Option<Arc<MibSet>> {
        let set = self.set();
        if set.is_none() {
            match self.undecided() {
                0 => error(&no_mib()),
                n => error(&which::undecided_text(n, &keys::keys("mib.open", "SPC k o"))),
            }
        }
        set
    }

    // -- Commands -------------------------------------------------------------

    fn command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "mib.page" => self.cmd_page(),
            "mib.open" => self.cmd_open(),
            "mib.search" => self.cmd_pick(&Kind::ALL, Asked::Def, "MIB: every kind"),
            "mib.telecommand" => self.cmd_pick(&[Kind::Telecommand], Asked::Def, "MIB: telecommands"),
            "mib.tc_parameter" => self.cmd_pick(&[Kind::TcParam], Asked::Def, "MIB: TC parameters"),
            "mib.tm_packet" => self.cmd_pick(&[Kind::TmPacket], Asked::Def, "MIB: TM packets"),
            "mib.tm_parameter" => self.cmd_pick(&[Kind::TmParam], Asked::Def, "MIB: TM parameters"),
            "mib.calibration" => self.cmd_pick(&[Kind::Calibration], Asked::Def, "MIB: calibrations"),
            "mib.insert" => self.cmd_pick(&[Kind::Telecommand], Asked::Insert, "Insert a telecommand"),
            "mib.edit_call" => self.cmd_edit_call(),
            "mib.reload" => self.cmd_reload(),
            "mib.settings" => open_settings(true),
            "mib.copy_bytes" => self.cmd_copy_bytes(),
            "mib.generate" => self.cmd_generate(),
            "mib.insert_sequence" => self.cmd_insert_sequence(),
            "mib.diff" => self.cmd_diff(),
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    /// `SPC k k`: the MIB page, on the MIBs here.
    fn cmd_page(&mut self) {
        let _ = self.set();
        let (key, _) = self.key_here();
        let open = self.pages.iter_mut().find_map(|(id, p)| match p {
            Open::Mib(page) => Some((*id, page)),
            _ => None,
        });
        match open {
            Some((id, page)) => {
                page.show(key);
                api::page::refresh(id);
                api::page::focus(id);
            }
            None => {
                let id = api::page::open(MIB_KIND, "*mib*");
                self.pages.insert(id, Open::Mib(Box::new(MibPage::new(key))));
            }
        }
    }

    /// `SPC k o`: every MIB known here -- found in the project, named by
    /// its settings or yours -- to open its page with; and another
    /// folder, added to your MIBs.
    fn cmd_open(&mut self) {
        let walk = self.look();
        let known = self.known_from(&walk);
        let choice = self.choice();
        let used: Vec<String> = which::in_use(&known, choice.as_ref()).used.iter().map(|k| k.id.clone()).collect();
        let mut items: Vec<PickerItem> = known.iter().map(|k| mib_row(k, used == [k.id.clone()])).collect();
        let named: Vec<&str> = known.iter().filter(|k| k.named() && k.path.is_some()).map(|k| k.label.as_str()).collect();
        if named.len() > 1 {
            let label = format!("{} {:<28} {}", if used.len() > 1 { "●" } else { " " }, "all of them together", if project_lists_roots() { "this project's settings" } else { "your settings" });
            items.push(PickerItem::new(label, "settings").detail(named.join(", ")));
        }
        let more = if walk.cut { format!(" -- the project was looked through as far as {} folders; one of its own further in, by its path", walk.folders) } else { String::new() };
        items.push(PickerItem::new(format!("  {:<28} {}", "another folder…", "by its path"), "folder").detail(format!("a MIB elsewhere on disk, added to your settings{more}")));
        let id = api::pick("Open a MIB", &items);
        self.asked.insert(id, Asked::Mib);
    }

    /// A MIB picked with `SPC k o`: in use here from now on, its page
    /// open.
    fn open_mib(&mut self, value: &str) {
        match value {
            "folder" => return self.ask_folder(),
            "settings" => {
                if !self.pick(Choice::Settings, false) {
                    return;
                }
            }
            id => {
                let known = self.known();
                let Some(k) = known.iter().find(|k| k.id == id) else { return };
                let reachable = k.path.is_some();
                let label = k.label.clone();
                if !self.pick(Choice::One(id.to_string()), !reachable) {
                    return;
                }
                if !reachable {
                    return api::notify(&not_reachable(&label));
                }
            }
        }
        self.cmd_page();
        self.doctor();
    }

    /// "another folder…": its full path, the explorer's selection to
    /// start from.
    fn ask_folder(&mut self) {
        if project_lists_roots() {
            api::notify(&format!("{} lists its own MIBs (mib.roots in its .fenix/settings.toml), which take the place of yours here -- a folder elsewhere goes in that list", project_name()));
            return open_settings(true);
        }
        let initial = host::explorer_selected()
            .map(|p| match host::folder_list(&p) {
                Ok(_) => p,
                // A file: its folder.
                Err(_) => Path::new(&p).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or(p),
            })
            .unwrap_or_default();
        let id = api::prompt("MIB folder (a full path)", &initial);
        self.asked.insert(id, Asked::Folder);
    }

    /// A folder typed for "another folder…". One of the project's is
    /// picked here by its path, found or not. One elsewhere is added to
    /// your `mib.roots` (once) and picked here, its page open -- or, when
    /// Fenix hasn't given it to the extension yet, open when it next
    /// starts the extension. Only ever from what you ran: Fenix may start
    /// the extension again once a folder setting of its changes.
    fn add_folder(&mut self, typed: &str) {
        let path = match which::typed_folder(typed) {
            Ok(path) => path,
            Err(why) => return error(&why),
        };
        let project_path = self.project_path();
        if let Some(rel) = project_path.as_deref().and_then(|p| which::in_project(&path, Some(p))) {
            let shown = if rel.is_empty() { "the project's top".to_string() } else { format!("{rel}/") };
            if which::probe(&project_dir().join(&rel), &rel).is_none() {
                return error(&format!("no MIB tables in {shown} -- a MIB folder holds its .dat tables (ccf.dat, pid.dat, ...)"));
            }
            // Known once picked, found or not.
            if self.pick(Choice::One(format!("found:{rel}")), false) {
                self.cmd_page();
                self.doctor();
            }
            return;
        }
        // A folder Fenix let it see (the explorer's): one with no tables
        // isn't a MIB. Others it can't look in before Fenix gives them.
        if let Ok(entries) = host::folder_list(&path) {
            if !entries.iter().any(|e| ["vdf.dat", "ccf.dat", "pcf.dat", "pid.dat", "tpcf.dat", "caf.dat", "txf.dat"].contains(&e.name.to_lowercase().as_str())) {
                return error(&format!("no MIB tables in {path} -- a MIB folder holds its .dat tables (ccf.dat, pid.dat, ...)"));
            }
        }
        if self.project_path().is_none() {
            return error("open a file of the project first -- a MIB is picked for the project it's in");
        }
        let known = self.known();
        let (yours, _) = self.named();
        let others: Vec<String> = known.iter().filter(|k| !k.named()).map(|k| k.label.clone()).collect();
        let (list, label) = which::with_folder(&yours, &path, &others);
        let id = format!("set:{label}");
        let reachable = folders_dir().join(&label).is_dir();
        // Picked before the setting: Fenix may start it again as soon as
        // the setting is set.
        self.pick(Choice::One(id), !reachable);
        if let Some(list) = list {
            let json = serde_json::to_string(&list).unwrap_or_default();
            if let Err(e) = api::set_setting("mib.roots", Some(&json)) {
                return error(&format!("couldn't add {path} to your MIBs: {e}"));
            }
        }
        if reachable {
            self.cmd_page();
            self.doctor();
        } else {
            api::notify(&format!("added {path} to your MIBs as {label}. {}", not_reachable(&label)));
        }
    }

    /// A picker of `kinds`, for `asked`.
    fn cmd_pick(&mut self, kinds: &[Kind], asked: Asked, title: &str) {
        let Some(set) = self.set_here() else { return };
        let items = picker_rows(&set, kinds);
        if items.is_empty() {
            let what = if kinds.len() == 1 { kinds[0].plural().to_lowercase() } else { "definitions".into() };
            api::notify(&format!("no {what} in this project's MIBs"));
            return;
        }
        let id = api::pick(title, &items);
        self.asked.insert(id, asked);
    }

    /// `SPC k r`: the MIBs read again now.
    fn cmd_reload(&mut self) {
        let walk = self.look();
        let known = self.known_from(&walk);
        let (key, _) = self.key_with(&known);
        if key.roots.is_empty() {
            return error(&no_mib());
        }
        api::notify("reading the MIB again…");
        self.load(key);
    }

    /// The telecommand from the MIB on the cursor's line, with where you
    /// are, and the arguments it's written with.
    fn call_here(&mut self) -> Option<Call> {
        let set = self.set_here()?;
        let Some(here) = api::here() else {
            error("no file here -- the telecommand call is on the cursor's line");
            return None;
        };
        let line = here.text.lines().next().unwrap_or("").to_string();
        let Some(tc) = server::words(&line).into_iter().find_map(|(_, w)| set.resolve(w).filter(|d| d.kind == Kind::Telecommand)) else {
            error("no telecommand from this project's MIBs on this line");
            return None;
        };
        let names: Vec<String> = fenix_mib::telecommand::tc_parameters(set.index(), &set.get(tc).row).into_iter().filter(|p| !p.fixed && !p.name.is_empty()).map(|p| p.name).collect();
        let args = mib_form::read_arguments(&line, &set.get(tc).name, &names, &templates());
        Some(Call { set, tc, here, args })
    }

    /// `SPC k e`: the telecommand call on the cursor's line, back in the
    /// insert form; inserting puts it back in place of the line's call.
    fn cmd_edit_call(&mut self) {
        let Some(Call { set, tc, here, args }) = self.call_here() else { return };
        let line = here.text.lines().next().unwrap_or("").to_string();
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let mut form = InsertForm::new(&set, self.key_here().0, tc, templates(), None, apid_hex());
        form.fill(&args);
        form.replacing = true;
        let origin = Origin { path: here.path, line: here.line, replace: Some(indent..line.chars().count()) };
        self.open_form_page(form, origin);
    }

    /// `SPC k b`: the telecommand call on this line, as its packet's
    /// bytes on the clipboard.
    fn cmd_copy_bytes(&mut self) {
        let Some(Call { set, tc, here, args }) = self.call_here() else { return };
        let name = set.get(tc).name.clone();
        match fenix_mib::packets::encode_tc(&set, tc, &args, &mission().profile, 0) {
            Ok(p) => copy_literal(&p.bytes, &here.language, &name),
            Err(e) => error(&format!("can't build {name}'s packet: {e}")),
        }
    }

    /// `SPC k g`: something to generate from the MIBs -- from one of
    /// them when there are several, since each defines its own names.
    fn cmd_generate(&mut self) {
        let (key, _) = self.key_here();
        if key.roots.is_empty() {
            return error(&no_mib());
        }
        let mut items = Vec::new();
        for (i, (_, file, what)) in fenix_mib::generate::KINDS.iter().enumerate() {
            if key.roots.len() == 1 {
                items.push(PickerItem::new(format!("{file:<18} {what}"), i.to_string()));
                continue;
            }
            for (r, root) in key.roots.iter().enumerate() {
                items.push(PickerItem::new(format!("{:<24} {what} · {}", generated_name(file, Some(&root.label)), root.label), format!("{i}:{r}")));
            }
        }
        let id = api::pick("Generate from the MIB", &items);
        self.asked.insert(id, Asked::Generate);
    }

    /// `SPC k q`: a command sequence, inserted as the telecommands it
    /// sends -- each through the project's templates, with its formal
    /// parameters' defaults, its timing and comments as comment lines.
    fn cmd_insert_sequence(&mut self) {
        let Some(set) = self.set_here() else { return };
        let all = fenix_mib::sequence::sequences(&set);
        if all.is_empty() {
            return api::notify("no command sequences in this project's MIBs");
        }
        let items: Vec<PickerItem> = all.iter().map(|(root, name, descr)| PickerItem::new(format!("{name:<12} {descr} · {}", set.root_label(*root)), format!("{root}:{name}"))).collect();
        let id = api::pick("Insert a command sequence", &items);
        self.asked.insert(id, Asked::Sequence);
    }

    fn insert_sequence(&mut self, value: &str) {
        let Some(set) = self.set_ready() else { return };
        let Some((root, name)) = value.split_once(':') else { return };
        let Some(seq) = root.parse().ok().and_then(|root| fenix_mib::sequence::sequence(&set, root, name)) else { return };
        let Some(here) = api::here().filter(|h| h.path.is_some()) else {
            return error("open the file the sequence goes in first -- it's inserted where the cursor is");
        };
        let comment = comment_prefix(&here.language);
        let t = templates();
        let expanded = fenix_mib::sequence::expand(&set, &seq, &std::collections::HashMap::new());
        let mut lines = vec![format!("{comment} {} · {}", seq.name, seq.description)];
        for f in &seq.formals {
            lines.push(format!("{comment}   formal {} = {}", f.name, f.default));
        }
        lines.extend(fenix_mib::sequence::render(&set, &expanded, &t.command, &t.argument, &t.separator, comment));
        let problems: usize = expanded.iter().map(|e| e.problems.len()).sum();
        let _ = api::buffer::insert(&(lines.join("\n") + "\n"));
        api::notify(&if problems == 0 { format!("inserted {}'s {} steps", seq.name, expanded.len()) } else { format!("inserted {} -- {problems} problem(s), marked ! in its comments", seq.name) });
    }

    /// `SPC k D`: two of the MIBs known here -- found in the project or
    /// named by the settings -- compared, definition by definition, into
    /// generated/.
    fn cmd_diff(&mut self) {
        let walk = self.look();
        let known: Vec<Known> = self.known_from(&walk).into_iter().filter(|k| k.path.is_some()).collect();
        if known.len() < 2 {
            return error("comparing needs two MIBs -- found in the project, or named in mib.roots: add the other version as a second MIB");
        }
        let mut items = Vec::new();
        for a in &known {
            for b in &known {
                if a.id != b.id {
                    items.push(PickerItem::new(format!("{} → {}", a.label, b.label), format!("{}\t{}", a.id, b.id)));
                }
            }
        }
        let id = api::pick("Compare MIBs: from the old one to the new one", &items);
        self.asked.insert(id, Asked::Diff);
    }

    fn diff(&mut self, value: &str) {
        let known = self.known();
        let options = read_options();
        let Some((a, b)) = value.split_once('\t') else { return };
        let root = |id: &str| known.iter().find(|k| k.id == id).and_then(Known::root);
        let (Some(old), Some(new)) = (root(a), root(b)) else { return };
        if !project_dir().is_dir() {
            return error("open a file of the project first -- the comparison goes in its generated/ folder");
        }
        let a = MibSet::load_with(vec![old.clone()], None, options.clone());
        let b = MibSet::load_with(vec![new.clone()], None, options);
        let changes = fenix_mib::diff::compare(&a, &b);
        let text = fenix_mib::diff::report(&changes, &old.label, &new.label);
        let safe = |s: &str| s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>();
        let file = format!("mib-diff-{}-{}.md", safe(&old.label), safe(&new.label));
        let dir = project_dir().join("generated");
        let path = dir.join(&file);
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
            Ok(()) => {
                host::open_file(&path.to_string_lossy(), None);
                api::notify(&format!("{} changes from {} to {} -- generated/{file}", changes.len(), old.label, new.label));
            }
            Err(e) => error(&format!("couldn't write generated/{file}: {e}")),
        }
    }

    /// Writes generated file `kind` into the project's `generated/` and
    /// opens it: from the MIB `root` when the project has several.
    fn generate(&mut self, kind: usize, root: Option<usize>) {
        let Some(all) = self.set_here() else { return };
        let Some((name, file, _)) = fenix_mib::generate::KINDS.get(kind) else { return };
        let (key, _) = self.key_here();
        let (set, file) = match root.and_then(|r| key.roots.get(r)).filter(|_| key.roots.len() > 1) {
            Some(r) => (Arc::new(MibSet::load_with(vec![r.clone()], None, key.options.clone())), generated_name(file, Some(&r.label))),
            None => (all, file.to_string()),
        };
        let file = file.as_str();
        if !project_dir().is_dir() {
            return error("open a file of the project first -- what's generated goes in its generated/ folder");
        }
        let mission = mission();
        let xtce = text("mib.xtce_version").and_then(|v| fenix_mib::generate::XtceVersion::parse(&v).ok()).unwrap_or_default();
        let Some(generated) = fenix_mib::generate::generate_with(name, &set, &mission.profile, mission.base, xtce) else { return };
        let dir = project_dir().join("generated");
        let path = dir.join(file);
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, generated)) {
            Ok(()) => {
                host::open_file(&path.to_string_lossy(), None);
                api::notify(&format!("wrote generated/{file} -- regenerate it with {} after the MIB changes", Hints::current().generate));
            }
            Err(e) => error(&format!("couldn't write generated/{file}: {e}")),
        }
    }

    // -- Pages ----------------------------------------------------------------

    /// `def`'s page: the open definition page goes to it (so `H` comes
    /// back), else a new one opens.
    pub(crate) fn open_def(&mut self, def: DefRef) {
        let set = self.set_ready();
        let open = self.pages.iter_mut().find_map(|(id, p)| match p {
            Open::Def(page) => Some((*id, page)),
            _ => None,
        });
        match open {
            Some((id, page)) => {
                page.go(def);
                host::page_retitle(id, &mib_def::title(set.as_deref(), page));
                api::page::refresh(id);
                api::page::focus(id);
            }
            None => {
                let page = DefPage::new(def);
                let id = api::page::open(DEF_KIND, &mib_def::title(set.as_deref(), &page));
                self.pages.insert(id, Open::Def(page));
            }
        }
    }

    /// The insert form for `tc`, inserting where the cursor is in the
    /// file you're in.
    fn open_form(&mut self, tc: DefRef) {
        let Some(set) = self.set_ready() else { return };
        let Some(here) = api::here().filter(|h| h.path.is_some()) else {
            return error("open the file the telecommand goes in first -- the form inserts where its cursor is");
        };
        let key = self.key_here().0;
        let remembered = self.last.get(&(key.project.clone(), set.get(tc).name.clone()));
        let form = InsertForm::new(&set, key, tc, templates(), remembered, apid_hex());
        let origin = Origin { path: here.path, line: here.line, replace: None };
        self.open_form_page(form, origin);
    }

    fn open_form_page(&mut self, form: InsertForm, origin: Origin) {
        let id = api::page::open(FORM_KIND, &mib_form::title(&form));
        self.pages.insert(id, Open::Form(Box::new(form), origin));
    }

    fn layout(&mut self, id: u64, width: usize) -> Page {
        let set = match self.pages.get(&id) {
            Some(Open::Mib(_) | Open::Def(_)) => self.set_for_page(),
            _ => self.set_ready(),
        };
        let loading = self.reading.is_some();
        let hex = apid_hex();
        let (here, source) = if matches!(self.pages.get(&id), Some(Open::Mib(_))) { self.key_here() } else { Default::default() };
        let undecided = if matches!(self.pages.get(&id), Some(Open::Mib(_))) { self.undecided() } else { 0 };
        match self.pages.get(&id) {
            Some(Open::Mib(p)) => {
                let source = if here == p.key { source } else { String::new() };
                let ctx = mib_page::Ctx { set: set.as_deref(), loading, apid_hex: hex, source, undecided };
                Page::canvas(mib_page::layout(p, &ctx, width)).typing(p.typing()).space(p.claims_space())
            }
            Some(Open::Def(p)) => Page::canvas(mib_def::layout(p, set.as_deref(), hex, width, &Hints::current())).typing(p.typing()),
            Some(Open::Form(f, _)) => {
                let bytes = f.show_bytes.then(|| form_bytes(set.as_deref(), f));
                Page::canvas(mib_form::layout(f, bytes.as_ref(), width)).typing(f.typing()).space(f.claims_space())
            }
            None => Page::new(),
        }
    }

    fn key(&mut self, id: u64, key: page::Key) {
        let set = self.set_ready();
        let hex = apid_hex();
        let source = if matches!(self.pages.get(&id), Some(Open::Mib(_))) { self.key_here().1 } else { String::new() };
        let Some(open) = self.pages.get_mut(&id) else { return };
        match open {
            Open::Mib(p) => {
                let ctx = mib_page::Ctx { set: set.as_deref(), loading: false, apid_hex: hex, source, undecided: 0 };
                let action = p.key(key, &ctx);
                api::page::refresh(id);
                self.page_action(id, action);
            }
            Open::Def(p) => {
                let action = match &set {
                    Some(set) => p.key(key, set),
                    None if matches!(key, page::Key::Char('q') | page::Key::Escape) => mib_def::Action::Close,
                    None => mib_def::Action::None,
                };
                host::page_retitle(id, &mib_def::title(set.as_deref(), p));
                api::page::refresh(id);
                self.def_action(id, action);
            }
            Open::Form(f, _) => {
                let action = f.key(key);
                api::page::refresh(id);
                self.form_action(id, action);
            }
        }
    }

    fn page_action(&mut self, id: u64, action: mib_page::Action) {
        match action {
            mib_page::Action::None => {}
            mib_page::Action::Close => self.close(id),
            mib_page::Action::Open(def) => self.open_def(def),
            mib_page::Action::Insert(def) => self.open_form(def),
            mib_page::Action::Copy(text) => copy(&text),
            mib_page::Action::Reload => self.cmd_reload(),
            mib_page::Action::Settings(project) => open_settings(project),
            mib_page::Action::OpenFile(path, line) => open_file(&path, line),
            mib_page::Action::Switch => self.cmd_open(),
        }
    }

    fn def_action(&mut self, id: u64, action: mib_def::Action) {
        match action {
            mib_def::Action::None => {}
            mib_def::Action::Close => self.close(id),
            mib_def::Action::Insert(def) => self.open_form(def),
            mib_def::Action::Copy(text) => copy(&text),
            mib_def::Action::OpenFile(path, line) => open_file(&path, line),
            mib_def::Action::MibPage => self.cmd_page(),
            // The CCSDS extension follows it on the recording or live
            // source open.
            mib_def::Action::Follow(def) => {
                let Some(set) = self.set_ready() else { return };
                let payload = serde_json::json!({ "name": set.get(def).name }).to_string();
                let call = api::call_service("ccsds.follow", &payload);
                self.calls.insert(call, "following a parameter needs the CCSDS extension, with a recording or live source open");
            }
        }
    }

    fn form_action(&mut self, id: u64, action: mib_form::Action) {
        match action {
            mib_form::Action::None => {}
            mib_form::Action::Close => self.close(id),
            mib_form::Action::Open(def) => self.open_def(def),
            mib_form::Action::Insert(text) => self.form_insert(id, text),
            mib_form::Action::CopyBytes | mib_form::Action::DecodeBytes => {
                let Some(Open::Form(f, origin)) = self.pages.get(&id) else { return };
                let name = f.name().to_string();
                let ext = origin.path.as_deref().and_then(|p| Path::new(p).extension()).map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                match form_bytes(self.set_ready().as_deref(), f) {
                    // The CCSDS extension's packet inspector.
                    Ok(mib_form::Built { bytes, .. }) if action == mib_form::Action::DecodeBytes => {
                        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
                        let payload = serde_json::json!({ "hex": hex, "source": format!("{name} from the form") }).to_string();
                        let call = api::call_service("ccsds.decode", &payload);
                        self.calls.insert(call, "decoding a packet needs the CCSDS extension");
                    }
                    Ok(built) => copy_literal(&built.bytes, &ext, &name),
                    Err(e) => error(&format!("can't build {name}'s packet: {e}")),
                }
            }
        }
    }

    /// The form's command goes where it was opened, the form closes, and
    /// the file it went into is back in front.
    fn form_insert(&mut self, id: u64, text: String) {
        let Some(Open::Form(form, origin)) = self.pages.remove(&id) else { return };
        let tc = form.name().to_string();
        self.last.insert((form.key.project.clone(), tc.clone()), form.values());
        api::page::close(id);
        let Some(path) = origin.path else { return error("the file the form was opened from is closed") };
        let done = match origin.replace {
            Some(cols) => {
                let line = origin.line.saturating_sub(1);
                let edit = api::TextEdit::new((line, cols.start as u32), (line, cols.end as u32), text);
                let done = api::buffer::edit(&path, &[edit]);
                host::open_file(&path, Some(origin.line));
                done
            }
            None => {
                host::open_file(&path, None);
                api::buffer::insert(&text)
            }
        };
        match done {
            Ok(()) => api::notify(&format!("inserted {tc}")),
            Err(e) => error(&format!("couldn't insert {tc}: {e}")),
        }
    }

    fn close(&mut self, id: u64) {
        self.pages.remove(&id);
        api::page::close(id);
    }

    fn picked(&mut self, id: u64, value: Option<String>) {
        let (Some(asked), Some(value)) = (self.asked.remove(&id), value) else { return };
        match asked {
            Asked::Def => {
                if let Some(def) = def_from(&value) {
                    self.open_def(def);
                }
            }
            Asked::Insert => {
                if let Some(def) = def_from(&value) {
                    self.open_form(def);
                }
            }
            Asked::Generate => {
                let (kind, root) = value.split_once(':').map_or((value.as_str(), None), |(k, r)| (k, r.parse().ok()));
                if let Ok(kind) = kind.parse() {
                    self.generate(kind, root);
                }
            }
            Asked::Sequence => self.insert_sequence(&value),
            Asked::Diff => self.diff(&value),
            Asked::Mib => self.open_mib(&value),
            Asked::Folder => self.add_folder(&value),
        }
    }

    /// Once started: the project looked through, the doctor told, and
    /// the page of a MIB picked before Fenix could give it to the
    /// extension opened, now it can be read.
    fn started(&mut self) {
        self.starting = None;
        // What the settings name is used unless something's picked: the
        // project isn't looked through for nothing.
        let (named, _) = self.named();
        if named.is_empty() || self.choice().is_some_and(|c| c.found().is_some()) {
            self.look();
        }
        self.doctor();
        self.tell_ccsds();
        let n = self.undecided();
        if n > 0 && !self.said_undecided {
            self.said_undecided = true;
            api::notify(&which::undecided_text(n, &keys::keys("mib.open", "SPC k o")));
        }
        self.open_waiting_page();
    }

    /// The page of a MIB picked before Fenix gave the extension its folder,
    /// opened once it can be read -- once Fenix has said which project
    /// this is (`waiting_seen`).
    fn open_waiting_page(&mut self) {
        if self.waiting_seen || self.project_path().is_none() {
            return;
        }
        self.waiting_seen = true;
        let known = self.known();
        let choice = self.choice();
        let in_use = which::in_use(&known, choice.as_ref());
        if choice.is_some() && in_use.choice == choice && !in_use.used.is_empty() && self.take_waiting_page() {
            self.cmd_page();
        }
    }

    // -- The project doctor -------------------------------------------------

    /// The doctor's checks for the MIBs in use when they're the project's
    /// -- found in it, wherever, or named by its own settings: each one's
    /// tables and their layout, the problems in their files, and the
    /// mission profile they're read against. The project's other MIBs are
    /// named, to switch to.
    fn doctor(&mut self) {
        let known = self.known();
        let choice = self.choice();
        let in_use = which::in_use(&known, choice.as_ref());
        let undecided = in_use.undecided;
        let used: Vec<Known> = in_use.used.into_iter().filter(|k| k.of_project()).cloned().collect();
        let keys = Hints::current();
        let check = |label: &str, health: Health, detail: String| Check { section: Section::Database, label: label.to_string(), health, detail, fix: None };
        let mut checks = Vec::new();
        let others: Vec<&str> = known.iter().filter(|k| k.of_project() && k.path.is_some() && !used.iter().any(|u| u.id == k.id)).map(|k| k.label.as_str()).collect();
        let others = (!others.is_empty()).then(|| check("other MIBs", Health::Info, format!("{} -- {} opens one", others.join(", "), keys.open)));
        if undecided > 0 {
            checks.push(check("MIBs found", Health::Info, which::undecided_text(undecided, &keys.open)));
        }
        let cut = self.found.as_ref().filter(|w| w.cut).map(|w| check("looked through", Health::Info, format!("the first {} folders of the project -- a MIB further in is named in its settings (mib.roots)", w.folders)));
        let set = if used.is_empty() { None } else { self.set() };
        let Some(set) = set else {
            checks.extend(others);
            project::doctor(&checks);
            return;
        };
        let options = read_options();
        let many = set.roots().len() > 1;
        for (i, root) in set.roots().iter().enumerate() {
            let named = |what: &str| if many { format!("{} {what}", root.label) } else { what.to_string() };
            let info = set.root_info(i);
            let (tables, rows) = (info.tables, info.rows);
            let mut summary = format!("{tables} table{}, {rows} row{}", if tables == 1 { "" } else { "s" }, if rows == 1 { "" } else { "s" });
            if let Some(v) = &info.version {
                summary = format!("{v} -- {summary}");
            }
            let place = match used.iter().find(|k| k.label == root.label).map(|k| &k.origin) {
                Some(which::Origin::Found(rel)) if rel.is_empty() => " · at the project's top".to_string(),
                Some(which::Origin::Found(rel)) => format!(" · in {rel}/"),
                _ => format!(" · {} in the project's settings", root.label),
            };
            checks.push(check(&named("tables"), if tables == 0 { Health::Warn } else { Health::Ok }, format!("{summary}{place}")));
            if let Some(format) = &info.format {
                checks.push(check(&named("layout"), Health::Info, format!("{format} -- mib.icd in the project's settings ({}) says which", keys.settings)));
            }
            let mut unknown: Vec<String> = std::fs::read_dir(&root.path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dat")))
                .map(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default())
                .filter(|t| !options.schema.knows(t))
                .collect();
            unknown.sort();
            if !unknown.is_empty() {
                checks.push(check(&named("not in the ICD"), Health::Info, format!("{}.dat -- ignored; mib.columns reads a table of the mission's own", unknown.join(".dat, "))));
            }
        }
        // What's wrong in their files; what's against the standards comes
        // with the profile.
        let standards: std::collections::HashSet<(usize, &Path, Option<usize>, &str)> = self.standards.iter().map(|p| (p.root, p.file.as_path(), p.line, p.message.as_str())).collect();
        let problems: Vec<&fenix_mib::Problem> = set.problems().iter().filter(|p| !standards.contains(&(p.root, p.file.as_path(), p.line, p.message.as_str()))).collect();
        let shown = problems.len().min(20);
        for p in &problems[..shown] {
            let file = p.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let name = p.line.map(|l| format!("{file}:{l}")).unwrap_or(file);
            let name = if many { format!("{} {name}", set.root_label(p.root)) } else { name };
            checks.push(check(&name, if p.line.is_none() { Health::Bad } else { Health::Warn }, p.message.clone()));
        }
        if problems.len() > shown {
            checks.push(check("and more", Health::Warn, format!("{} more -- ! on the MIB page lists them all", problems.len() - shown)));
        }
        if (0..set.roots().len()).any(|i| set.root_info(i).tables > 0) {
            let what = match used.as_slice() {
                [one] => match &one.origin {
                    which::Origin::Found(rel) if rel.is_empty() => "the MIB at the project's top".to_string(),
                    which::Origin::Found(rel) => format!("the MIB in {rel}/"),
                    _ => format!("{}, from the project's settings", one.label),
                },
                _ => "the MIBs the project's settings name".to_string(),
            };
            checks.push(check("browse", Health::Ok, format!("{what} -- {} lists what's in it", keys.page)));
            checks.extend(mission_checks(&self.standards, &keys).into_iter().map(|(label, health, detail)| check(&label, health, detail)));
        }
        checks.extend(others);
        checks.extend(cut);
        project::doctor(&checks);
    }
}

/// The doctor's line for the mission profile: its PUS edition, the
/// on-board time, and the keys of Fenix's settings, where it's set.
fn profile_summary(edition: &str, time: impl std::fmt::Display, settings_keys: &str) -> String {
    format!("{edition} · {time} · {settings_keys} sets the project's (CCSDS & PUS)")
}

/// The doctor's line for the project's `recordings/`: how many, and the
/// keys that open one.
fn recordings_summary(n: usize, recording_keys: &str) -> String {
    if n == 0 {
        format!("none yet -- {recording_keys} opens one")
    } else {
        format!("{n} -- {recording_keys} opens one")
    }
}

/// The mission profile the project's MIBs are read against, whether they
/// agree with it and the standards (`problems`, what the checks found),
/// and the project's recordings.
fn mission_checks(problems: &[fenix_mib::Problem], keys: &Hints) -> Vec<(String, Health, String)> {
    use fenix_ccsds::pus::PusEdition;
    let mission = mission();
    let mut out = Vec::new();
    let edition = match mission.profile.pus {
        PusEdition::C => "PUS-C",
        PusEdition::A => "PUS-A",
        PusEdition::None => "no PUS",
    };
    let summary = profile_summary(edition, mission.profile.tm_time.label(), &keys.fenix_settings);
    if mission.problems.is_empty() {
        out.push(("mission profile".to_string(), Health::Ok, summary));
    } else {
        out.push(("mission profile".to_string(), Health::Bad, format!("{} -- {summary}", mission.problems.join("; "))));
    }
    if problems.is_empty() {
        out.push(("agrees".to_string(), Health::Ok, "the MIB agrees with the profile and the standards".to_string()));
    }
    let shown = problems.len().min(20);
    for p in &problems[..shown] {
        let name = p.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let name = p.line.map(|l| format!("{name}:{l}")).unwrap_or(name);
        out.push((name, Health::Warn, p.message.clone()));
    }
    if problems.len() > shown {
        out.push(("and more".to_string(), Health::Warn, format!("{} more against the standards -- ! on the MIB page lists them all", problems.len() - shown)));
    }
    let recordings = project_dir().join("recordings");
    if recordings.is_dir() {
        let n = std::fs::read_dir(&recordings).into_iter().flatten().flatten().filter(|e| e.path().is_file() && e.path().extension().is_none_or(|x| !x.eq_ignore_ascii_case("md"))).count();
        let what = recordings_summary(n, &keys.recording);
        out.push(("recordings".to_string(), Health::Ok, what));
    } else {
        out.push(("recordings".to_string(), Health::Info, "no recordings/ folder for dumps and captures".to_string()));
    }
    out
}

/// A generated file's name: `file`, or with several MIBs the MIB's
/// label before its extension (`xtce-OPS.xml`, `mib-OPS.h`).
fn generated_name(file: &str, label: Option<&str>) -> String {
    let Some(label) = label else { return file.to_string() };
    let safe: String = label.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    match file.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{safe}.{ext}"),
        None => format!("{file}-{safe}"),
    }
}

/// What starts a comment in a file of `language` (its name or extension).
fn comment_prefix(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "tcl" | "python" | "py" | "sh" | "bash" | "shell" | "ruby" | "rb" | "perl" | "pl" | "yaml" | "toml" | "r" => "#",
        "lua" | "sql" | "ada" | "adb" | "ads" | "haskell" | "hs" => "--",
        "" | "text" | "plain" => "#",
        _ => "//",
    }
}

/// The form's telecommand as a packet, from its arguments.
fn form_bytes(set: Option<&MibSet>, f: &InsertForm) -> Result<mib_form::Built, String> {
    let set = set.ok_or("the MIB isn't read yet")?;
    mib_form::build(set, f, &mission().profile)
}

impl MibExtension {
    /// What the language server asked for, done once its answer is sent.
    fn apply(&mut self) {
        for def in std::mem::take(&mut self.mib.to_open) {
            self.mib.open_def(def);
        }
    }
}

impl Extension for MibExtension {
    /// The project is looked through, and the doctor told, in a call of
    /// their own: not while Fenix starts.
    fn activate(&mut self) -> Result<(), String> {
        self.mib.starting = Some(host::set_timer(1, false));
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        self.mib.command(id)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Picked((id, value)) => self.mib.picked(id, value),
            Event::Timer(t) if Some(t) == self.mib.watch => self.mib.watched(),
            Event::Timer(t) if Some(t) == self.mib.reading => self.mib.read_now(),
            Event::Timer(t) if Some(t) == self.mib.starting => self.mib.started(),
            Event::Prompted((id, text)) => self.mib.picked(id, text),
            // Read before they're asked for, so K, gd and completion have
            // them.
            Event::FocusChanged(_) => {
                // What's picked is known once Fenix has said which project
                // this is: the doctor and CCSDS go by it then.
                let unknown = self.mib.choice.is_none();
                let _ = self.mib.set();
                if unknown && self.mib.choice.is_some() {
                    self.mib.doctor();
                    self.mib.tell_ccsds();
                }
                self.mib.open_waiting_page();
            }
            Event::ServiceReply((id, answer)) => {
                if let (Some(why), Err(e)) = (self.mib.calls.remove(&id), answer) {
                    error(&format!("{why} ({e})"));
                }
            }
            Event::SettingChanged(key) if key.starts_with("mib.") || key.starts_with("ccsds.") => {
                // What the pages show (APIDs) changes them; how the MIBs
                // are read is in their key, so they're read again when
                // next asked for. Your MIBs as Fenix has them now.
                if key == "mib.roots" && self.mib.found.is_none() {
                    self.mib.look();
                }
                let (here, _) = self.mib.key_here();
                for page in self.mib.pages.values_mut() {
                    if let Open::Mib(p) = page {
                        p.show(here.clone());
                    }
                }
                if key.starts_with("ccsds.") {
                    self.mib.set = None;
                }
                self.mib.watch();
                self.mib.refresh_pages();
                self.mib.doctor();
                self.mib.tell_ccsds();
            }
            _ => {}
        }
        self.apply();
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        self.mib.layout(page, width as usize)
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.mib.key(page, key.into());
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        match self.mib.pages.get_mut(&page) {
            Some(Open::Mib(p)) => p.paste(text),
            Some(Open::Def(p)) => p.paste(text),
            Some(Open::Form(f, _)) => f.paste(text),
            None => return,
        }
        api::page::refresh(page);
    }

    /// The MIB page comes back as it was; a definition, by its kind and
    /// name. The insert form doesn't: where it inserts is gone.
    fn page_state(&mut self, page: u64) -> Option<String> {
        match self.mib.pages.get(&page)? {
            Open::Mib(_) => Some(String::new()),
            Open::Def(p) => {
                let set = self.mib.set_ready()?;
                let def = p.current();
                Some(format!("{}:{}", def.kind.tag(), set.get(def).name))
            }
            Open::Form(..) => None,
        }
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        match kind {
            MIB_KIND => {
                let key = self.mib.key_here().0;
                self.mib.pages.insert(page, Open::Mib(Box::new(MibPage::new(key))));
                true
            }
            DEF_KIND => {
                let Some(set) = self.mib.set() else { return false };
                let Some((tag, name)) = state.split_once(':') else { return false };
                let Some(&kind) = Kind::ALL.iter().find(|k| k.tag() == tag) else { return false };
                let Some(def) = (0..set.roots().len()).find_map(|root| set.find(kind, root, name)) else { return false };
                let def_page = DefPage::new(def);
                host::page_retitle(page, &mib_def::title(Some(&set), &def_page));
                self.mib.pages.insert(page, Open::Def(def_page));
                true
            }
            _ => false,
        }
    }

    fn page_closed(&mut self, page: u64) {
        self.mib.pages.remove(&page);
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.servers.handle(server, message, &mut self.mib);
        self.apply();
    }

    /// `mib.open {name}`: that definition's page, whatever its kind, from
    /// the MIBs in use here (the notebook's `mib:` links).
    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = match name {
            "mib.open" => self.open_by_name(payload),
            "mib.lookup" => self.lookup(payload),
            "mib.which" => Ok(self.mib.which_answer().to_string()),
            other => Err(format!("no service {other}")),
        };
        self.apply();
        Some(answer)
    }
}

impl MibExtension {
    fn open_by_name(&mut self, payload: &str) -> Result<String, String> {
        let call: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("a MIB call: {e}"))?;
        let name = call.get("name").and_then(|v| v.as_str()).ok_or("a MIB call names a definition: {name}")?;
        let set = self.mib.set().ok_or_else(no_mib)?;
        let def = (0..set.roots().len()).find_map(|root| Kind::ALL.iter().find_map(|&k| set.find(k, root, name))).ok_or_else(|| format!("{name} isn't in the MIB"))?;
        self.mib.open_def(def);
        Ok(String::new())
    }

    /// `mib.lookup {query, kind?}`: a coding agent's `mib_lookup`.
    fn lookup(&mut self, payload: &str) -> Result<String, String> {
        let call: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("the arguments: {e}"))?;
        let field = |k: &str| call.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
        let query = field("query").or_else(|| field("name")).unwrap_or("");
        let kinds = match field("kind") {
            Some(k) => vec![agent::kind(k).ok_or_else(|| format!("{k}: one of telecommand, tc_parameter, tm_packet, tm_parameter, calibration"))?],
            None => Kind::ALL.to_vec(),
        };
        let set = self.mib.set().ok_or_else(no_mib)?;
        agent::lookup(&set, query, &kinds, apid_hex()).map(|v| v.to_string())
    }
}

export!(MibExtension);

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample mission's MIB, in the shared crate's fixtures.
    fn ops() -> MibSet {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/mib/tests/fixtures/ops");
        MibSet::load(vec![MibRoot { label: "OPS".into(), path }], None)
    }

    #[test]
    fn each_mib_generates_into_its_own_file() {
        // Two MIBs define the same names: one file each, or XTCE gets
        // every MetaCommand twice in one SpaceSystem.
        assert_eq!(generated_name("xtce.xml", None), "xtce.xml");
        assert_eq!(generated_name("xtce.xml", Some("OPS")), "xtce-OPS.xml");
        assert_eq!(generated_name("mib_dissector.lua", Some("OPS 2.1")), "mib_dissector-OPS_2_1.lua");
    }

    #[test]
    fn a_definition_goes_through_a_picker_and_back() {
        for kind in Kind::ALL {
            let def = DefRef { kind, index: 7 };
            assert_eq!(def_from(&def_value(def)), Some(def));
        }
        assert_eq!(def_from("9:1"), None);
        assert_eq!(def_from("x"), None);
    }

    #[test]
    fn k_on_a_telecommand_shows_its_arguments_and_how_to_open_it() {
        let set = ops();
        let tc = set.find(Kind::Telecommand, 0, "ZTC08101").unwrap();
        let card = server::hover_text(&set, tc, true, &Hints::default());
        assert!(card.starts_with("TC ZTC08101 · OPS"), "{card}");
        assert!(card.contains("PTH00101") && card.contains("Arguments"), "{card}");
        assert!(card.ends_with("gd opens it · SPC k e edits this call"), "{card}");
        let param = set.resolve("PTH00102").unwrap();
        assert!(server::hover_text(&set, param, true, &Hints::default()).ends_with("gd opens it"));
    }

    #[test]
    fn the_card_names_the_keys_of_the_standard_scheme() {
        let set = ops();
        let tc = set.find(Kind::Telecommand, 0, "ZTC08101").unwrap();
        let card = server::hover_text(&set, tc, true, &Hints::standard());
        assert!(card.ends_with("F12 opens it · C-k k e edits this call"), "{card}");
        let param = set.resolve("PTH00102").unwrap();
        assert!(server::hover_text(&set, param, true, &Hints::standard()).ends_with("F12 opens it"));
    }

    #[test]
    fn the_doctor_names_the_keys_of_the_scheme() {
        let (vim, standard) = (Hints::default(), Hints::standard());
        assert_eq!(profile_summary("PUS-C", "CUC", &vim.fenix_settings), "PUS-C · CUC · SPC , sets the project's (CCSDS & PUS)");
        assert_eq!(profile_summary("PUS-C", "CUC", &standard.fenix_settings), "PUS-C · CUC · C-k , sets the project's (CCSDS & PUS)");
        assert_eq!(recordings_summary(0, &vim.recording), "none yet -- SPC k f opens one");
        assert_eq!(recordings_summary(3, &standard.recording), "3 -- C-k k f opens one");
    }

    #[test]
    fn what_it_says_of_a_missing_mib_names_the_keys_that_open_one() {
        assert_eq!(no_mib_text("SPC k o"), "no MIB here -- none found in the project or named in the settings; SPC k o opens one");
        assert_eq!(no_mib_text("C-k k o"), "no MIB here -- none found in the project or named in the settings; C-k k o opens one");
    }

    // -- Which MIBs, driven as Fenix drives it ----------------------------------

    use fenix_test_kit::{Fake, Harness};

    /// A project at `/home/me/mission` (as Fenix says it, at `/project` as
    /// the extension reaches it), Fenix's folders for `mib.roots`, and the
    /// extension's own folder: in a temporary folder, this thread's.
    struct World {
        dir: tempfile::TempDir,
    }

    impl World {
        fn new() -> World {
            let dir = tempfile::tempdir().unwrap();
            let paths = ["project", "folders", "data"].map(|p| dir.path().join(p));
            for p in &paths {
                std::fs::create_dir_all(p).unwrap();
            }
            PLACES.with(|p| *p.borrow_mut() = Some(paths));
            World { dir }
        }

        fn project(&self) -> PathBuf {
            self.dir.path().join("project")
        }

        fn folders(&self) -> PathBuf {
            self.dir.path().join("folders")
        }

        /// The sample mission's MIB copied to `to`.
        fn ops(&self, to: &Path) {
            let from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/mib/tests/fixtures/ops");
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }

        /// The extension, started in a script of the project, the first
        /// look through it done.
        fn start(&self, settings: &[(&str, &str)]) -> Harness<MibExtension> {
            let fake = Fake::new();
            for (k, v) in settings {
                fake.setting(k, v);
            }
            let project = api::ProjectInfo { name: "mission".into(), root: "/home/me/mission".into(), meta: Vec::new() };
            fake.state_mut().here = Some(api::Here { path: Some("/home/me/mission/scripts/pass.tcl".into()), line: 1, text: "ZTC08101".into(), language: "tcl".into(), project: Some(project) });
            let mut h = Harness::with_fake(fake);
            h.activate().unwrap();
            h.fire_timers();
            h
        }

        /// Nothing is written in the project's settings: it has none.
        fn project_settings_untouched(&self) {
            assert!(!self.project().join(".fenix").exists());
        }
    }

    fn pickers(h: &Harness<MibExtension>, title: &str) -> Vec<Vec<String>> {
        h.fake.state().pickers.iter().filter(|p| p.title.contains(title)).map(|p| p.items.iter().map(|i| i.label.split_whitespace().collect::<Vec<_>>().join(" ")).collect()).collect()
    }

    fn in_use(h: &mut Harness<MibExtension>) -> Vec<String> {
        h.ext.mib.key_here().0.roots.into_iter().map(|r| r.label).collect()
    }

    #[test]
    fn one_mib_in_a_subfolder_just_works_with_nothing_set() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        let mut h = w.start(&[]);
        // The doctor's checks are for it, wherever it is.
        let checks: Vec<(String, String)> = h.fake.state().checks.iter().map(|c| (c.label.clone(), c.detail.clone())).collect();
        assert!(checks.iter().any(|(l, d)| l == "tables" && d.ends_with("· in db/ops/")), "{checks:?}");
        assert!(checks.iter().any(|(l, d)| l == "browse" && d == "the MIB in db/ops/ -- SPC k k lists what's in it"), "{checks:?}");
        assert!(checks.iter().any(|(l, _)| l == "mission profile"), "{checks:?}");
        // The MIB page, K and gd's MIB, the pickers'.
        h.command("mib.page").unwrap();
        let page = h.page("mib");
        let screen = h.screen(page);
        assert!(screen.contains("db/ops") && screen.contains("found in db/ops/"), "{screen}");
        let set = h.ext.mib.set().unwrap();
        let tc = set.resolve("ZTC08101").unwrap();
        assert_eq!(set.root_label(set.get(tc).root), "db/ops");
        h.command("mib.search").unwrap();
        assert!(pickers(&h, "every kind")[0].iter().any(|l| l.contains("ZTC08101")));
        assert!(h.serve("mib.lookup", r#"{"query":"ZTC08101"}"#).unwrap().unwrap().contains("ZTC08101"));
        w.project_settings_untouched();
    }

    fn said(h: &Harness<MibExtension>) -> Vec<String> {
        h.fake.state().notifications.iter().map(|(_, m)| m.clone()).collect()
    }

    fn writes(h: &Harness<MibExtension>) -> Vec<String> {
        h.fake.calls().into_iter().filter(|c| c.starts_with("setting-set")).collect()
    }

    #[test]
    fn a_mib_at_the_top_or_in_mib_is_found_as_before_by_the_projects_name() {
        let w = World::new();
        w.ops(&w.project());
        let mut h = w.start(&[]);
        assert_eq!(in_use(&mut h), ["mission"], "the project's top, by its name");
        let w = World::new();
        w.ops(&w.project());
        w.ops(&w.project().join("mib"));
        w.ops(&w.project().join("db/old"));
        let mut h = w.start(&[]);
        assert_eq!(h.ext.mib.key_here().0.roots[0].path, w.project().join("mib"), "mib/ first, as before");
        assert_eq!(in_use(&mut h), ["mission"], "by the project's name, as before: mib.icd_per_mib by it still applies");
    }

    #[test]
    fn several_found_and_none_sure_is_none_until_one_is_picked() {
        let w = World::new();
        w.ops(&w.project().join("db/ops-2.0"));
        w.ops(&w.project().join("tools/sim"));
        let mut h = w.start(&[]);
        assert!(in_use(&mut h).is_empty(), "not guessed");
        // Said once, on the page and by the doctor.
        assert_eq!(said(&h).iter().filter(|m| m.as_str() == "2 MIBs found here -- SPC k o picks one").count(), 1, "{:?}", said(&h));
        assert!(h.fake.state().checks.iter().any(|c| c.label == "MIBs found" && c.detail == "2 MIBs found here -- SPC k o picks one"));
        h.command("mib.page").unwrap();
        let page = h.page("mib");
        assert!(h.screen(page).contains("2 MIBs found here"), "{}", h.screen(page));
        h.fake.on_pick("Open a MIB", "found:db/ops-2.0");
        h.command("mib.open").unwrap();
        let rows = &pickers(&h, "Open a MIB")[0];
        assert!(rows[0].starts_with("db/ops-2.0 found in the project"), "{rows:?}");
        assert!(rows[1].starts_with("tools/sim found in the project"), "{rows:?}");
        assert!(rows[2].starts_with("another folder…"), "{rows:?}");
        assert_eq!(rows.len(), 3);
        let detail = h.fake.state().pickers[0].items[0].detail.clone();
        assert!(detail.contains("tables · TC and TM"), "{detail:?}");
        // K, gd, completion and the pages use it now; its page is open.
        assert_eq!(in_use(&mut h), ["db/ops-2.0"]);
        assert!(h.screen(page).contains("found in db/ops-2.0/ · M: 2 MIBs"), "{}", h.screen(page));
        // M on the page switches.
        h.fake.on_pick("Open a MIB", "found:tools/sim");
        h.key(page, Key::Char('M'));
        assert_eq!(in_use(&mut h), ["tools/sim"]);
        // Kept for the project, not in its settings; nothing said then.
        let mut again = w.start(&[]);
        assert_eq!(in_use(&mut again), ["tools/sim"]);
        assert!(!said(&again).iter().any(|m| m.contains("found here")));
        w.project_settings_untouched();
    }

    #[test]
    fn mib_is_used_beside_others_and_one_alone_outside_tests_too() {
        let w = World::new();
        w.ops(&w.project().join("mib"));
        w.ops(&w.project().join("db/old"));
        let mut h = w.start(&[]);
        assert_eq!(h.ext.mib.key_here().0.roots[0].path, w.project().join("mib"));
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("tests/fixtures/ops"));
        let mut h = w.start(&[]);
        assert_eq!(in_use(&mut h), ["db/ops"]);
    }

    #[test]
    fn a_fixture_is_never_the_mib_unpicked() {
        // As this repository has one.
        let w = World::new();
        w.ops(&w.project().join("crates/mib/tests/fixtures/ops"));
        let mut h = w.start(&[]);
        assert!(in_use(&mut h).is_empty());
        assert!(h.fake.state().checks.iter().any(|c| c.label == "MIBs found"), "{:?}", h.fake.state().checks);
        assert!(!h.fake.state().checks.iter().any(|c| c.label == "tables"));
        h.command("mib.search").unwrap();
        assert!(said(&h).last().unwrap().starts_with("1 MIB found here, among tests or samples"), "{:?}", said(&h));
    }

    #[test]
    fn the_language_server_and_focus_never_look_through_the_project() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        let mut h = w.start(&[]);
        w.ops(&w.project().join("db/new"));
        // What K, gd and completion ask, and moving between files.
        let _ = h.ext.mib.set();
        h.event(Event::FocusChanged(None));
        assert_eq!(in_use(&mut h), ["db/ops"]);
        assert!(h.ext.mib.known().iter().all(|k| k.id != "found:db/new"));
        // Looked through on SPC k o.
        h.fake.on_pick_nothing("Open a MIB");
        h.command("mib.open").unwrap();
        assert!(h.ext.mib.known().iter().any(|k| k.id == "found:db/new"));
    }

    #[test]
    fn a_setting_naming_a_found_folder_is_offered_once() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("db/sim"));
        // Fenix gives the folder a project's setting names at
        // /folders/mib/<name> too.
        w.ops(&w.folders().join("OPS"));
        std::fs::create_dir_all(w.project().join(".fenix")).unwrap();
        std::fs::write(w.project().join(".fenix/settings.toml"), "[mib.roots]\nOPS = \"db/ops\"\n").unwrap();
        let settings = std::fs::read_to_string(w.project().join(".fenix/settings.toml")).unwrap();
        let mut h = w.start(&[("mib.roots", r#"[["OPS","db/ops"]]"#)]);
        assert_eq!(in_use(&mut h), ["OPS"], "the settings' first");
        h.fake.on_pick_nothing("Open a MIB");
        h.command("mib.open").unwrap();
        let rows = &pickers(&h, "Open a MIB")[0];
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows[0].starts_with("db/sim found in the project"), "{rows:?}");
        assert!(rows[1].starts_with("● OPS this project's settings"), "{rows:?}");
        assert_eq!(std::fs::read_to_string(w.project().join(".fenix/settings.toml")).unwrap(), settings);
    }

    #[test]
    fn two_from_the_settings_are_used_together_until_one_is_picked() {
        let w = World::new();
        w.ops(&w.folders().join("OPS"));
        w.ops(&w.folders().join("SIM"));
        w.ops(&w.project().join("mib"));
        let mut h = w.start(&[("mib.roots", r#"[["OPS","/srv/OPS"],["SIM","/srv/SIM"]]"#)]);
        assert_eq!(in_use(&mut h), ["OPS", "SIM"], "as before: the settings' MIBs, together");
        h.fake.on_pick("Open a MIB", "set:SIM");
        h.command("mib.open").unwrap();
        let rows = &pickers(&h, "Open a MIB")[0];
        assert!(rows[0].starts_with("mission found in the project") && rows[1].starts_with("OPS your settings") && rows[2].starts_with("SIM your settings"), "{rows:?}");
        assert!(rows[3].starts_with("● all of them together your settings"), "{rows:?}");
        assert_eq!(in_use(&mut h), ["SIM"]);
        h.fake.on_pick("Open a MIB", "settings");
        h.command("mib.open").unwrap();
        assert_eq!(in_use(&mut h), ["OPS", "SIM"]);
    }

    #[test]
    fn the_ccsds_extension_is_told_what_is_in_use() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("db/sim"));
        let mut h = w.start(&[]);
        let told = |h: &Harness<MibExtension>| h.fake.state().services.iter().filter(|c| c.name == "ccsds.mib").map(|c| c.payload.clone()).collect::<Vec<_>>();
        assert_eq!(told(&h), [r#"{"roots":[]}"#], "none is sure");
        h.fake.on_pick("Open a MIB", "found:db/sim");
        h.command("mib.open").unwrap();
        let sim = w.project().join("db/sim").to_string_lossy().into_owned();
        assert_eq!(told(&h).last().unwrap(), &serde_json::json!({ "roots": [["db/sim", sim]] }).to_string());
        assert_eq!(h.serve("mib.which", "{}").unwrap().unwrap(), serde_json::json!({ "roots": [["db/sim", sim]] }).to_string());
    }

    #[test]
    fn another_folder_is_added_to_your_settings_once() {
        let w = World::new();
        let mut h = w.start(&[]);
        assert!(in_use(&mut h).is_empty());
        // Selected in the explorer: the prompt starts from it.
        h.fake.state_mut().explorer = Some("/srv/mibs/OPS2/ccf.dat".into());
        h.fake.on_pick("Open a MIB", "folder");
        h.fake.on_prompt("MIB folder", "/srv/mibs/OPS2/");
        h.command("mib.open").unwrap();
        assert_eq!(h.fake.state().prompts[0].initial, "/srv/mibs/OPS2");
        assert_eq!(writes(&h), [r#"setting-set mib.roots [["OPS2","/srv/mibs/OPS2"]]"#]);
        let notice = said(&h).last().cloned().unwrap_or_default();
        assert!(notice.contains("added /srv/mibs/OPS2 to your MIBs as OPS2") && notice.contains("as soon as Fenix gives the extension its folder"), "{notice}");
        // Another goes beside it -- Fenix gives the extension back what it
        // set -- and the same one, however spelt, isn't added twice.
        h.fake.on_prompt("MIB folder", "/home/me/old/OPS2");
        h.command("mib.open").unwrap();
        h.fake.on_prompt("MIB folder", "/srv//mibs/./OPS2");
        h.command("mib.open").unwrap();
        assert_eq!(writes(&h), [r#"setting-set mib.roots [["OPS2","/srv/mibs/OPS2"]]"#, r#"setting-set mib.roots [["OPS2","/srv/mibs/OPS2"],["OPS2-2","/home/me/old/OPS2"]]"#]);
        // Started again, Fenix gives it the folder: its page opens, with
        // nothing said again.
        w.ops(&w.folders().join("OPS2"));
        let mut again = w.start(&[("mib.roots", r#"[["OPS2","/srv/mibs/OPS2"],["OPS2-2","/home/me/old/OPS2"]]"#)]);
        assert_eq!(in_use(&mut again), ["OPS2"]);
        let page = again.page("mib");
        assert!(again.screen(page).contains("OPS2"));
        assert!(said(&again).is_empty(), "{:?}", said(&again));
        // Once.
        let mut third = w.start(&[("mib.roots", r#"[["OPS2","/srv/mibs/OPS2"]]"#)]);
        assert!(third.fake.state().pages.is_empty());
        assert_eq!(in_use(&mut third), ["OPS2"]);
        assert!(!w.project().join(".fenix").exists(), "nothing in the project's settings");
    }

    #[test]
    fn what_cannot_be_a_mib_folder_is_refused() {
        let w = World::new();
        let mut h = w.start(&[]);
        h.fake.on_pick("Open a MIB", "folder");
        for (typed, why) in [("~", "home folder"), ("/", "top of a disk"), ("/home/me/", "home folder"), ("C:", "drive's current folder"), ("mibs", "isn't a full path")] {
            h.fake.on_prompt("MIB folder", typed);
            h.command("mib.open").unwrap();
            assert!(said(&h).last().unwrap().contains(why), "{typed}: {:?}", said(&h));
        }
        // One it can look in, with no tables.
        h.fake.state_mut().folders.insert("/srv/docs".into(), vec![api::FolderEntry { name: "notes.md".into(), size: 1 }]);
        h.fake.on_prompt("MIB folder", "/srv/docs");
        h.command("mib.open").unwrap();
        assert!(said(&h).last().unwrap().starts_with("no MIB tables in /srv/docs"), "{:?}", said(&h));
        // Of the project, with no tables.
        std::fs::create_dir_all(w.project().join("docs")).unwrap();
        h.fake.on_prompt("MIB folder", "/home/me/mission/docs");
        h.command("mib.open").unwrap();
        assert!(said(&h).last().unwrap().starts_with("no MIB tables in docs/"), "{:?}", said(&h));
        assert!(writes(&h).is_empty());
    }

    #[test]
    fn a_folder_of_the_project_the_walk_missed_is_picked_by_its_path() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("a/b/c/d/e/f/g/ops"));
        let mut h = w.start(&[]);
        assert_eq!(in_use(&mut h), ["db/ops"]);
        h.fake.on_pick("Open a MIB", "folder");
        h.fake.on_prompt("MIB folder", "/home/me/mission/a/b/c/d/e/f/g/ops/");
        h.command("mib.open").unwrap();
        assert!(writes(&h).is_empty(), "kept by the extension, not in settings");
        assert_eq!(in_use(&mut h), ["a/b/c/d/e/f/g/ops"]);
        let mut again = w.start(&[]);
        assert_eq!(in_use(&mut again), ["a/b/c/d/e/f/g/ops"]);
        w.project_settings_untouched();
    }

    #[test]
    fn nothing_is_picked_or_read_without_a_project() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("db/sim"));
        let mut h = w.start(&[]);
        h.fake.on_pick("Open a MIB", "found:db/sim");
        h.command("mib.open").unwrap();
        // Started again, before Fenix says which project it's in.
        let fake = Fake::new();
        let mut early = Harness::<MibExtension>::with_fake(fake);
        early.activate().unwrap();
        early.fire_timers();
        assert!(early.ext.mib.choice.is_none(), "not taken as nothing");
        early.fake.on_pick("Open a MIB", "found:db/ops");
        early.command("mib.open").unwrap();
        assert!(said(&early).last().unwrap().starts_with("open a file of the project first"));
        let picks = std::fs::read_to_string(w.dir.path().join("data").join(CHOICES)).unwrap();
        assert!(!picks.contains(r#""":"#), "{picks}");
        // Then it says: the pick is the project's.
        let project = api::ProjectInfo { name: "mission".into(), root: "/home/me/mission".into(), meta: Vec::new() };
        early.fake.state_mut().here = Some(api::Here { path: Some("/home/me/mission/a.tcl".into()), line: 1, text: String::new(), language: "tcl".into(), project: Some(project) });
        early.event(Event::FocusChanged(None));
        assert_eq!(in_use(&mut early), ["db/sim"]);
    }

    #[test]
    fn a_bad_picks_file_is_kept_aside_and_said_of_once() {
        let w = World::new();
        w.ops(&w.project().join("db/ops"));
        w.ops(&w.project().join("db/sim"));
        let data = w.dir.path().join("data");
        std::fs::write(data.join(CHOICES), "{\"/home/me/other\": {\"choice\": \"found:x\"").unwrap();
        let mut h = w.start(&[]);
        h.fake.on_pick("Open a MIB", "found:db/sim");
        h.command("mib.open").unwrap();
        h.fake.on_pick("Open a MIB", "found:db/ops");
        h.command("mib.open").unwrap();
        assert_eq!(said(&h).iter().filter(|m| m.contains("couldn't be read")).count(), 1, "{:?}", said(&h));
        assert!(std::fs::read_to_string(data.join("choices.json.bad")).unwrap().contains("/home/me/other"));
        let picks: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data.join(CHOICES)).unwrap()).unwrap();
        assert_eq!(picks["/home/me/mission"]["choice"], "found:db/ops");
        assert!(!data.join("choices.json.new").exists());
    }

    #[test]
    fn a_project_with_its_own_list_keeps_it() {
        let w = World::new();
        w.ops(&w.folders().join("OPS"));
        std::fs::create_dir_all(w.project().join(".fenix")).unwrap();
        std::fs::write(w.project().join(".fenix/settings.toml"), "[mib.roots]\nOPS = \"/srv/OPS\"\n").unwrap();
        let mut h = w.start(&[("mib.roots", r#"[["OPS","/srv/OPS"]]"#)]);
        h.fake.on_pick("Open a MIB", "folder");
        h.command("mib.open").unwrap();
        assert!(h.fake.state().prompts.is_empty());
        assert!(writes(&h).is_empty());
        assert!(h.fake.calls().iter().any(|c| c.contains("settings.this_project")), "{:?}", h.fake.calls());
    }

    #[test]
    fn no_mib_anywhere() {
        let w = World::new();
        let mut h = w.start(&[]);
        assert!(h.fake.state().checks.is_empty());
        h.command("mib.page").unwrap();
        let page = h.page("mib");
        h.fire_timers();
        assert!(h.screen(page).contains("No MIB here."));
        h.command("mib.search").unwrap();
        assert!(said(&h).last().unwrap().starts_with("no MIB here"), "{:?}", said(&h));
    }
}
