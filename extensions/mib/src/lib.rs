//! SCOS-2000 MIBs in Fenix, as Fenix 1.x had them built in: the MIB page
//! (`SPC k k`), a definition's own page, the telecommand insert form,
//! pickers by kind, code generated from the MIB, and the MIB in your
//! scripts -- `K`, `gd`, completion and checks of telecommand calls,
//! through a language server.
//!
//! Which MIBs: those `mib.roots` names (the project's own first, as Fenix
//! gives it settings), each folder at `/folders/mib/<name>`; else the
//! project itself, when it's a MIB. They're read when first needed and
//! again when their files change (`mib.watch`). How they're read against
//! the mission comes from the CCSDS settings, `ccsds.*`
//! (`permissions.settings`).

mod mib_check;
mod mib_def;
mod mib_form;
mod mib_page;
mod page;
mod server;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fenix_extension_api::lsp::Servers;
use fenix_extension_api::{self as api, export, host, project, Check, Event, Extension, Health, Key, Page, PickerItem, Section};
use fenix_mib::mission::{list_items, map_entries, Mission};
use fenix_mib::{DefRef, Kind, MibRoot, MibSet};

use mib_def::DefPage;
use mib_form::InsertForm;
use mib_page::{MibKey, MibPage};

/// The kinds of page it opens.
const MIB_KIND: &str = "mib";
const DEF_KIND: &str = "mib-definition";
const FORM_KIND: &str = "mib-insert";

/// Where the folders `mib.roots` names are.
const FOLDERS: &str = "/folders/mib";
/// Where the project is.
const PROJECT: &str = "/project";

/// How often the MIBs' files are looked at for a change.
const WATCH_MILLIS: u32 = 3_000;

/// What it says when there's no MIB to work with.
const NO_MIB: &str = "no MIB for this project -- SPC k , lists one in its settings";

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
    /// Definitions `gd` asked for, opened once the language server's
    /// answer is sent.
    pub(crate) to_open: Vec<DefRef>,
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

/// The tables that say a folder is a MIB: any one is enough -- a TM-only
/// database has no `ccf.dat`.
const MIB_TABLES: &[&str] = &["vdf.dat", "ccf.dat", "pcf.dat", "pid.dat", "tpcf.dat", "caf.dat", "txf.dat"];

fn holds_mib_tables(dir: &Path) -> bool {
    MIB_TABLES.iter().any(|t| dir.join(t).is_file())
}

/// The project, when it's a MIB itself: its `mib/` folder, else its own.
fn project_mib() -> Option<PathBuf> {
    let root = Path::new(PROJECT);
    [root.join("mib"), root.to_path_buf()].into_iter().find(|d| holds_mib_tables(d))
}

/// The name of the project you're in.
fn project_name() -> String {
    api::here().and_then(|h| h.project).map(|p| p.name).unwrap_or_else(|| "the project".into())
}

/// Whether the project's own settings list MIBs (`[mib]` `roots`).
fn project_lists_roots() -> bool {
    let Ok(text) = std::fs::read_to_string(Path::new(PROJECT).join(".fenix").join("settings.toml")) else { return false };
    let Ok(table) = text.parse::<toml::Table>() else { return false };
    table.get("mib").and_then(|m| m.as_table()).and_then(|m| m.get("roots")).and_then(|r| r.as_table()).is_some_and(|r| !r.is_empty())
}

/// The MIBs here: those `mib.roots` names (the project's, else yours),
/// else the project itself when it's a MIB -- and where they come from,
/// for the MIB page to say.
pub(crate) fn key_here() -> (MibKey, String) {
    let mut roots: Vec<MibRoot> = map("mib.roots")
        .into_iter()
        .map(|(label, _)| {
            let path = PathBuf::from(format!("{FOLDERS}/{label}"));
            MibRoot { label, path }
        })
        .filter(|root| root.path.is_dir())
        .collect();
    let listed = !roots.is_empty();
    if roots.is_empty() {
        if let Some(dir) = project_mib() {
            roots.push(MibRoot { label: project_name(), path: dir });
        }
    }
    let default = text("mib.default");
    let source = match (listed, listed && project_lists_roots()) {
        (true, true) => format!("from {}'s settings", project_name()),
        (true, false) => "from your settings".to_string(),
        (false, _) if roots.is_empty() => String::new(),
        (false, _) => format!("{} is a MIB", project_name()),
    };
    let project = Path::new(PROJECT).is_dir().then(|| PathBuf::from(PROJECT));
    (MibKey { roots, default, project, options: read_options() }, source)
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
    host::run_command(if project && Path::new(PROJECT).is_dir() { "settings.this_project" } else { "settings.open" });
}

// -- The MIB ------------------------------------------------------------------

impl Mib {
    /// The MIBs here, read -- now, if they weren't, or if what they're
    /// read from changed.
    pub(crate) fn set(&mut self) -> Option<Arc<MibSet>> {
        let (key, _) = key_here();
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

    /// Reads `key`'s MIBs, checked against the mission; every page shows
    /// them.
    fn load(&mut self, key: MibKey) -> Arc<MibSet> {
        let mission = mission();
        let mut set = MibSet::load_with(key.roots.clone(), key.default.as_deref(), key.options.clone());
        let options = fenix_mib::checks::Options { disabled: &mission.checks_off, base: mission.base, profile: &mission.profile };
        let found = fenix_mib::checks::run(&set, &options);
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
            error(NO_MIB);
        }
        set
    }

    // -- Commands -------------------------------------------------------------

    fn command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "mib.page" => self.cmd_page(),
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
            other => return Err(format!("no command {other}")),
        }
        Ok(())
    }

    /// `SPC k k`: the MIB page, on the MIBs here.
    fn cmd_page(&mut self) {
        let _ = self.set();
        let (key, _) = key_here();
        let open = self.pages.iter_mut().find_map(|(id, p)| match p {
            Open::Mib(page) => Some((*id, page)),
            _ => None,
        });
        match open {
            Some((id, page)) => {
                page.key = key;
                api::page::refresh(id);
                api::page::focus(id);
            }
            None => {
                let id = api::page::open(MIB_KIND, "*mib*");
                self.pages.insert(id, Open::Mib(Box::new(MibPage::new(key))));
            }
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
        let (key, _) = key_here();
        if key.roots.is_empty() {
            return error(NO_MIB);
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
        let names: Vec<String> = fenix_mib::telecommand::tc_parameters(set.index(), &set.get(tc).row).into_iter().filter(|p| !p.fixed).map(|p| p.name).collect();
        let args = mib_form::read_arguments(&line, &set.get(tc).name, &names, &templates());
        Some(Call { set, tc, here, args })
    }

    /// `SPC k e`: the telecommand call on the cursor's line, back in the
    /// insert form; inserting puts it back in place of the line's call.
    fn cmd_edit_call(&mut self) {
        let Some(Call { set, tc, here, args }) = self.call_here() else { return };
        let line = here.text.lines().next().unwrap_or("").to_string();
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let mut form = InsertForm::new(&set, key_here().0, tc, templates(), None, apid_hex());
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

    /// `SPC k g`: something to generate from the MIBs.
    fn cmd_generate(&mut self) {
        if key_here().0.roots.is_empty() {
            return error(NO_MIB);
        }
        let items: Vec<PickerItem> = fenix_mib::generate::KINDS.iter().enumerate().map(|(i, (_, file, what))| PickerItem::new(format!("{file:<18} {what}"), i.to_string())).collect();
        let id = api::pick("Generate from the MIB", &items);
        self.asked.insert(id, Asked::Generate);
    }

    /// Writes generated file `kind` into the project's `generated/` and
    /// opens it.
    fn generate(&mut self, kind: usize) {
        let Some(set) = self.set_here() else { return };
        let Some((name, file, _)) = fenix_mib::generate::KINDS.get(kind) else { return };
        if !Path::new(PROJECT).is_dir() {
            return error("open a file of the project first -- what's generated goes in its generated/ folder");
        }
        let mission = mission();
        let xtce = text("mib.xtce_version").and_then(|v| fenix_mib::generate::XtceVersion::parse(&v).ok()).unwrap_or_default();
        let Some(generated) = fenix_mib::generate::generate_with(name, &set, &mission.profile, mission.base, xtce) else { return };
        let dir = Path::new(PROJECT).join("generated");
        let path = dir.join(file);
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, generated)) {
            Ok(()) => {
                host::open_file(&path.to_string_lossy(), None);
                api::notify(&format!("wrote generated/{file} -- regenerate it with SPC k g after the MIB changes"));
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
        let key = key_here().0;
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
            Some(Open::Mib(_) | Open::Def(_)) => self.set(),
            _ => self.set_ready(),
        };
        let hex = apid_hex();
        match self.pages.get(&id) {
            Some(Open::Mib(p)) => {
                let (here, source) = key_here();
                let source = if here == p.key { source } else { String::new() };
                let ctx = mib_page::Ctx { set: set.as_deref(), loading: false, apid_hex: hex, source };
                Page::canvas(mib_page::layout(p, &ctx, width)).typing(p.typing()).space(p.claims_space())
            }
            Some(Open::Def(p)) => Page::canvas(mib_def::layout(p, set.as_deref(), hex, width)).typing(p.typing()),
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
        let Some(open) = self.pages.get_mut(&id) else { return };
        match open {
            Open::Mib(p) => {
                let (_, source) = key_here();
                let ctx = mib_page::Ctx { set: set.as_deref(), loading: false, apid_hex: hex, source };
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
                    Ok(bytes) if action == mib_form::Action::DecodeBytes => {
                        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
                        let payload = serde_json::json!({ "hex": hex, "source": format!("{name} from the form") }).to_string();
                        let call = api::call_service("ccsds.decode", &payload);
                        self.calls.insert(call, "decoding a packet needs the CCSDS extension");
                    }
                    Ok(bytes) => copy_literal(&bytes, &ext, &name),
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
                if let Ok(kind) = value.parse() {
                    self.generate(kind);
                }
            }
        }
    }

    // -- The project doctor -------------------------------------------------

    /// The doctor's checks for a project that's a MIB: its tables, their
    /// problems, and the mission profile they're read against.
    fn doctor(&mut self) {
        let Some(dir) = project_mib() else {
            project::doctor(&[]);
            return;
        };
        let options = read_options();
        let set = MibSet::load_with(vec![MibRoot { label: project_name(), path: dir.clone() }], None, options.clone());
        let check = |label: &str, health: Health, detail: String| Check { section: Section::Database, label: label.to_string(), health, detail, fix: None };
        let mut checks = Vec::new();
        let info = set.root_info(0);
        let (tables, rows) = (info.tables, info.rows);
        let mut summary = format!("{tables} table{}, {rows} row{}", if tables == 1 { "" } else { "s" }, if rows == 1 { "" } else { "s" });
        if let Some(v) = &info.version {
            summary = format!("{v} -- {summary}");
        }
        checks.push(check("tables", if tables == 0 { Health::Warn } else { Health::Ok }, summary));
        if let Some(format) = &info.format {
            checks.push(check("layout", Health::Info, format!("{format} -- mib.icd in the project's settings (SPC k ,) says which")));
        }
        let problems = set.problems();
        let shown = problems.len().min(20);
        for p in &problems[..shown] {
            let file = p.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let name = p.line.map(|l| format!("{file}:{l}")).unwrap_or(file);
            checks.push(check(&name, if p.line.is_none() { Health::Bad } else { Health::Warn }, p.message.clone()));
        }
        if problems.len() > shown {
            checks.push(check("and more", Health::Warn, format!("{} more -- ! on the MIB page lists them all", problems.len() - shown)));
        }
        let mut unknown: Vec<String> = std::fs::read_dir(&dir)
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
            checks.push(check("not in the ICD", Health::Info, format!("{}.dat -- ignored; mib.columns reads a table of the mission's own", unknown.join(".dat, "))));
        }
        if tables > 0 {
            checks.push(check("browse", Health::Ok, "this project is its own MIB -- SPC k k lists what's in it".into()));
            checks.extend(mission_checks(&set).into_iter().map(|(label, health, detail)| check(&label, health, detail)));
        }
        project::doctor(&checks);
    }
}

/// The mission profile the project's MIB is read against, whether the
/// MIB agrees with it and the standards, and its recordings.
fn mission_checks(set: &MibSet) -> Vec<(String, Health, String)> {
    use fenix_ccsds::pus::PusEdition;
    let mission = mission();
    let mut out = Vec::new();
    let edition = match mission.profile.pus {
        PusEdition::C => "PUS-C",
        PusEdition::A => "PUS-A",
        PusEdition::None => "no PUS",
    };
    let summary = format!("{edition} · {} · SPC , sets the project's (CCSDS & PUS)", mission.profile.tm_time.label());
    if mission.problems.is_empty() {
        out.push(("mission profile".to_string(), Health::Ok, summary));
    } else {
        out.push(("mission profile".to_string(), Health::Bad, format!("{} -- {summary}", mission.problems.join("; "))));
    }
    let problems = fenix_mib::checks::run(set, &fenix_mib::checks::Options { disabled: &mission.checks_off, base: mission.base, profile: &mission.profile });
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
    let recordings = Path::new(PROJECT).join("recordings");
    if recordings.is_dir() {
        let n = std::fs::read_dir(&recordings).into_iter().flatten().flatten().filter(|e| e.path().is_file() && e.path().extension().is_none_or(|x| !x.eq_ignore_ascii_case("md"))).count();
        let what = if n == 0 { "none yet -- SPC k f opens one".to_string() } else { format!("{n} -- SPC k f opens one") };
        out.push(("recordings".to_string(), Health::Ok, what));
    } else {
        out.push(("recordings".to_string(), Health::Info, "no recordings/ folder for dumps and captures".to_string()));
    }
    out
}

/// The form's telecommand as a packet, from its arguments.
fn form_bytes(set: Option<&MibSet>, f: &InsertForm) -> Result<Vec<u8>, String> {
    let set = set.ok_or("the MIB isn't read yet")?;
    fenix_mib::packets::encode_tc(set, f.tc, &f.arguments(), &mission().profile, f.seq).map(|p| p.bytes)
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
    fn activate(&mut self) -> Result<(), String> {
        self.mib.doctor();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        self.mib.command(id)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Picked((id, value)) => self.mib.picked(id, value),
            Event::Timer(t) if Some(t) == self.mib.watch => self.mib.watched(),
            // Read before they're asked for, so K, gd and completion have
            // them.
            Event::FocusChanged(_) => {
                let _ = self.mib.set();
            }
            Event::ServiceReply((id, answer)) => {
                if let (Some(why), Err(e)) = (self.mib.calls.remove(&id), answer) {
                    error(&format!("{why} ({e})"));
                }
            }
            Event::SettingChanged(key) if key.starts_with("mib.") || key.starts_with("ccsds.") => {
                // What the pages show (APIDs) changes them; how the MIBs
                // are read is in their key, so they're read again when
                // next asked for.
                let (here, _) = key_here();
                for page in self.mib.pages.values_mut() {
                    if let Open::Mib(p) = page {
                        p.key = here.clone();
                    }
                }
                if key.starts_with("ccsds.") {
                    self.mib.set = None;
                }
                self.mib.watch();
                self.mib.refresh_pages();
                self.mib.doctor();
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
                self.mib.pages.insert(page, Open::Mib(Box::new(MibPage::new(key_here().0))));
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
    /// the MIBs here (the notebook's `mib:` links).
    fn serve(&mut self, _call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = match name {
            "mib.open" => self.open_by_name(payload),
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
        let set = self.mib.set().ok_or("no MIB here -- mib.roots names the folders")?;
        let def = (0..set.roots().len()).find_map(|root| Kind::ALL.iter().find_map(|&k| set.find(k, root, name))).ok_or_else(|| format!("{name} isn't in the MIB"))?;
        self.mib.open_def(def);
        Ok(String::new())
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
        let card = server::hover_text(&set, tc, true);
        assert!(card.starts_with("TC ZTC08101 · OPS"), "{card}");
        assert!(card.contains("PTH00101") && card.contains("Arguments"), "{card}");
        assert!(card.ends_with("gd opens it · SPC k e edits this call"), "{card}");
        let param = set.resolve("PTH00102").unwrap();
        assert!(server::hover_text(&set, param, true).ends_with("gd opens it"));
    }

    #[test]
    fn a_folder_with_mib_tables_is_a_mib() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!holds_mib_tables(dir.path()));
        std::fs::write(dir.path().join("pid.dat"), "").unwrap();
        assert!(holds_mib_tables(dir.path()), "a TM-only MIB has no ccf.dat");
    }
}
