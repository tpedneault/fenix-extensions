//! Env files (`.env`, `.env.<profile>`, `<name>.env`): their values
//! hidden in the editor (`••••••`) but on the cursor's line, all shown
//! for a file with `SPC p e r`; a page with every env file in the project
//! side by side; switching profiles by copying one over `.env`, a backup
//! kept; a warning when Git would commit one, with the fix; the keys an
//! example has that a file lacks, and a command that adds them; tasks
//! run with a profile's variables.
//!
//! It's a language server too: in env files, what doesn't parse, keys set
//! twice, what the example says against them and what Git would commit;
//! in code, the project's variable names after `process.env.`,
//! `os.environ["`, `std::env::var("` and the like.

mod compare;
mod complete;
mod files;
mod git;
mod page;
mod parse;
mod paths;
mod rules;
mod tasks;

use std::collections::{HashMap, HashSet};

use fenix_extension_api::lsp::types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity, Position, Range, ServerCapabilities, TextEdit as LspEdit,
    Uri,
};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{self as api, decorations, export, host, page as pages, project, Changed, Check, Command, Decoration, DecorationLook, DecorationRule, Event, Extension, Focus, Health, Key, Page, PickerItem, Role, Section, Task, TextEdit};

use git::{GitState, Risk};
use page::{Action, EnvPage};
use parse::{EnvFile, Quote, Severity, Span, MASK};

/// Where the backups a profile switch keeps go, a folder a project.
const BACKUPS: &str = "/data/backups";

/// Where the env file a project's tasks run with is kept, a file a
/// project.
const PROFILES: &str = "/data/profiles";

/// The page's kind.
const PAGE: &str = "env";

/// What a picker it opened is for.
enum Ask {
    /// A profile to copy over `target`.
    Profile(String),
    /// Yes or no, to copying `from` over `target`.
    CopyProfile { target: String, from: String },
    /// A backup to put back.
    Restore,
    /// Yes or no, to putting `backup` back over `target`.
    PutBack { target: String, backup: String },
    /// The file to add missing keys to.
    AddMissing,
    /// The env file tasks run with.
    TaskProfile,
    /// Yes or no, to `git rm --cached` these.
    Untrack(Vec<String>),
}

/// Git, asked about the env files.
enum Run {
    Tracked(Vec<String>),
    Ignored,
    Untrack(Vec<String>),
}

#[derive(Default)]
pub struct Dotenv {
    servers: Servers,
    core: Core,
}

#[derive(Default)]
struct Core {
    /// Its language servers, one a project.
    server_ids: Vec<u64>,
    /// The project's folder, as a full path.
    root: Option<String>,
    /// The project's env files, below it, once looked for.
    found: Option<Vec<String>>,
    git: GitState,
    /// Git's answers coming: the two asks, and what each wrote.
    runs: HashMap<u64, (Run, Vec<String>)>,
    /// Asked again while it was asking.
    git_again: bool,
    asks: HashMap<u64, Ask>,
    pages: HashMap<u64, EnvPage>,
    /// Files whose values are shown, by full path.
    revealed: HashSet<String>,
    /// The env file you're in, as a full path.
    focused: Option<String>,
    /// The files its masking rules are for, as last given Fenix.
    ruled: Option<Vec<String>>,
    /// The env file tasks run with, below the project, once read.
    task_profile: Option<String>,
    /// The tasks it offers, as last given Fenix.
    offered: Option<Vec<Task>>,
}

fn setting_on(key: &str, default: bool) -> bool {
    match api::setting(key).as_deref().map(str::trim) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

fn is_env_path(path: &str) -> bool {
    files::is_env(files::name(&path.replace('\\', "/")))
}

/// A file's text below the project: its buffer's, else what's on disk.
fn read(rel: &str) -> Option<String> {
    let at = paths::project(rel);
    api::buffer::text(&at).or_else(|| std::fs::read_to_string(&at).ok())
}

/// `text` in place of file `rel`'s: in its buffer as one change you can
/// undo, saved, when it's open; else on disk.
fn write(rel: &str, text: &str) -> Result<(), String> {
    let at = paths::project(rel);
    match api::buffer::text(&at) {
        Some(old) => {
            let (start, end) = compare::whole(&old);
            api::buffer::edit(&at, &[TextEdit::new(start, end, text)])?;
            api::buffer::save(&at)
        }
        None => std::fs::write(&at, text).map_err(|e| format!("{rel}: {e}")),
    }
}

fn severity(s: Severity) -> DiagnosticSeverity {
    match s {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
    }
}

/// What a problem in an env file is, where.
struct Mark {
    span: Span,
    severity: Severity,
    message: String,
}

/// The folder a project's backups go in: its name and a hash of where it
/// is, so two projects of one name don't share it.
fn backup_dir(root: &str) -> String {
    project_path(BACKUPS, root)
}

/// What's kept for project `root` under `base`: by its name and a hash of
/// where it is.
fn project_path(base: &str, root: &str) -> String {
    let name = root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("project");
    let hash = root.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    format!("{base}/{name}-{:08x}", hash as u32)
}

/// A backup's file name: when, and the file's path below the project with
/// its `/` as `%`.
fn backup_name(stamp: &str, rel: &str) -> String {
    format!("{stamp}__{}", rel.replace('/', "%"))
}

/// A backup's file name read back: when, and the file it was.
fn backup_of(name: &str) -> Option<(String, String)> {
    let (stamp, rel) = name.split_once("__")?;
    Some((stamp.to_string(), rel.replace('%', "/")))
}

impl Core {
    fn root(&mut self) -> Option<String> {
        if self.root.is_none() {
            self.root = api::here().and_then(|h| h.project).map(|p| p.root);
        }
        self.root.clone()
    }

    fn rel(&mut self, path: &str) -> Option<String> {
        let root = self.root();
        paths::below(path, root.as_deref())
    }

    /// The project's env files, looked for the first time they're asked.
    fn files(&mut self) -> Vec<String> {
        self.found.get_or_insert_with(|| files::find(std::path::Path::new("/project"))).clone()
    }

    /// Looked for again when `rel` is one they don't have.
    fn know(&mut self, rel: &str) {
        if self.found.as_ref().is_some_and(|f| !f.iter().any(|x| x == rel)) {
            self.found = None;
        }
    }

    /// Every env file's text: as a server has it, else its buffer's or
    /// what's on disk.
    fn texts(&mut self, open_docs: &[&Documents]) -> Vec<(String, String)> {
        let mut open: HashMap<String, String> = HashMap::new();
        for docs in open_docs {
            let root = docs.root.as_deref().and_then(paths::uri_path);
            for uri in docs.uris() {
                let (Some(path), Ok(parsed)) = (paths::uri_path(uri), uri.parse::<Uri>()) else { continue };
                if let (Some(rel), Some(text)) = (paths::below(&path, root.as_deref()), docs.text(&parsed)) {
                    open.insert(rel, text.to_string());
                }
            }
        }
        self.files().into_iter().filter_map(|rel| open.remove(&rel).or_else(|| read(&rel)).map(|t| (rel, t))).collect()
    }

    /// The example file `rel` is held to, and its text.
    fn example(&mut self, rel: &str, texts: &[(String, String)]) -> Option<(String, EnvFile)> {
        let all = self.files();
        let example = files::example_for(rel, &all)?.to_string();
        let text = texts.iter().find(|(r, _)| *r == example).map(|(_, t)| t.clone()).or_else(|| read(&example))?;
        Some((example, EnvFile::parse(&text)))
    }

    /// What's wrong in env file `rel` (none when it isn't below the
    /// project), whose text is `text`.
    fn marks(&mut self, rel: Option<&str>, text: &str, texts: &[(String, String)]) -> Vec<Mark> {
        let file = EnvFile::parse(text);
        let mut marks: Vec<Mark> = file.problems.iter().map(|p| Mark { span: p.span, severity: p.severity, message: p.message.clone() }).collect();
        for (e, (_, unset)) in file.entries.iter().zip(file.values()) {
            if let (false, Some(span)) = (unset.is_empty(), e.value.first()) {
                let names: Vec<String> = unset.iter().map(|n| format!("${{{n}}}")).collect();
                marks.push(Mark { span: *span, severity: Severity::Info, message: format!("{} isn't set above: the environment's, if it has it", names.join(", ")) });
            }
        }
        let Some(rel) = rel else { return marks };
        let top = Span::new(0, 0, text.lines().next().map_or(0, |l| l.chars().count() as u32));
        if let Some((example, ex)) = self.example(rel, texts) {
            let missing = compare::missing(&file, &ex);
            if !missing.is_empty() {
                marks.push(Mark { span: top, severity: Severity::Warning, message: format!("{example} has what this lacks: {} (SPC p e m adds them)", missing.join(", ")) });
            }
            for key in compare::extra(&file, &ex) {
                if let Some(e) = file.get(key) {
                    marks.push(Mark { span: e.key_span, severity: Severity::Info, message: format!("`{key}` isn't in {example}") });
                }
            }
        }
        if let Some(risk) = self.git.risk(rel) {
            let fix = match risk {
                Risk::Tracked => " (SPC p e u stops tracking it, the file kept)",
                Risk::NotIgnored => " (SPC p e i adds it to .gitignore)",
            };
            marks.push(Mark { span: top, severity: Severity::Warning, message: risk.said(rel) + fix });
        }
        marks
    }

    /// The project doctor's checks: what Git would commit, what lacks what
    /// its example has.
    fn doctor(&mut self, texts: &[(String, String)]) {
        let mut checks = Vec::new();
        for (rel, risk) in self.git.risks() {
            let (health, fix) = match risk {
                Risk::Tracked => (Health::Bad, ("stop tracking it", "dotenv.untrack")),
                Risk::NotIgnored => (Health::Warn, ("add it to .gitignore", "dotenv.gitignore")),
            };
            checks.push(Check::new(Section::Project, rel.clone(), health, risk.said(&rel)).fix(fix.0, fix.1));
        }
        for (rel, text) in texts {
            if let Some((example, ex)) = self.example(rel, texts) {
                let n = compare::missing(&EnvFile::parse(text), &ex).len();
                if n > 0 {
                    let detail = format!("lacks {n} key{} {example} has", if n == 1 { "" } else { "s" });
                    checks.push(Check::new(Section::Project, rel.clone(), Health::Info, detail).fix("add them, empty", "dotenv.add_missing"));
                }
            }
        }
        if checks.is_empty() && self.git.repo && !self.git.asked.is_empty() {
            checks.push(Check::new(Section::Project, "env files", Health::Ok, "ignored by Git"));
        }
        project::doctor(&checks);
    }

    /// Git asked again which env files it tracks and ignores.
    fn check_git(&mut self) {
        if !self.runs.is_empty() {
            self.git_again = true;
            return;
        }
        let asked = git::worth_asking(&self.files());
        if asked.is_empty() || !setting_on("dotenv.git", true) {
            self.git = GitState { asked, ..GitState::default() };
            return;
        }
        let spawn = |args: Vec<String>| Command::new("git").args(args).cwd("/project").spawn();
        match (spawn(git::ls_files(&asked)), spawn(git::check_ignore(&asked))) {
            (Ok(a), Ok(b)) => {
                self.runs.insert(a, (Run::Tracked(asked.clone()), Vec::new()));
                self.runs.insert(b, (Run::Ignored, Vec::new()));
            }
            (a, b) => {
                let why = a.as_ref().err().or(b.as_ref().err()).cloned().unwrap_or_default();
                for id in [a.ok(), b.ok()].into_iter().flatten() {
                    host::kill(id);
                }
                api::warn!("git: {why}");
            }
        }
    }

    /// The values of env file `path` (a full path) hidden, unless they're
    /// shown; with them shown, what `${VAR}` comes to after each.
    fn decorate(&mut self, path: &str) {
        if !is_env_path(path) {
            return;
        }
        let Some(text) = api::buffer::text(path) else { return };
        let file = EnvFile::parse(&text);
        let example = files::is_example(files::name(&path.replace('\\', "/")));
        let hidden = setting_on("dotenv.mask", true) && (!example || setting_on("dotenv.mask_examples", false)) && !self.revealed.contains(path);
        let mut marks = Vec::new();
        for (e, (value, _)) in file.entries.iter().zip(file.values()) {
            if hidden {
                marks.extend(e.value.iter().map(|s| Decoration::hide((s.line, s.start), (s.line, s.end), MASK)));
            } else if matches!(e.quote, Quote::None | Quote::Double) && e.raw.contains('$') {
                if let Some(last) = e.value.last() {
                    marks.push(Decoration::after((last.line, last.end - 1), format!("  = {}", value.replace('\n', "⏎")), Role::Muted));
                }
            }
        }
        if let Err(e) = decorations::set(&decorations::file(path), &marks) {
            api::warn!("{path}: {e}");
        }
    }

    /// Its rules for masking env files from their first frame, given Fenix
    /// again when what they're for has changed: the setting, the files
    /// found, those shown.
    fn set_rules(&mut self) {
        let names = if setting_on("dotenv.mask", true) {
            let known = self.files();
            let revealed: Vec<String> = self.revealed.iter().cloned().collect();
            rules::files(setting_on("dotenv.mask_examples", false), &known, &revealed)
        } else {
            Vec::new()
        };
        if self.ruled.as_ref() == Some(&names) {
            return;
        }
        let files: Vec<&str> = names.iter().map(String::as_str).collect();
        let set: Vec<DecorationRule> = if files.is_empty() { Vec::new() } else { rules::patterns().iter().map(|p| DecorationRule::new(&files, p, DecorationLook::Hide(MASK.into())).group(1)).collect() };
        match decorations::rules(&set) {
            Ok(()) => self.ruled = Some(names),
            Err(e) => api::warn!("masking rules: {e}"),
        }
    }

    /// The env file tasks run with: as kept for the project, else `.env`.
    fn task_profile(&mut self) -> String {
        if let Some(p) = &self.task_profile {
            return p.clone();
        }
        let Some(root) = self.root() else { return ".env".into() };
        let kept = std::fs::read_to_string(project_path(PROFILES, &root)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        self.task_profile.insert(kept.unwrap_or_else(|| ".env".into())).clone()
    }

    /// The commands `dotenv.tasks` lists, offered in the task picker with
    /// the variables of the env file tasks run with.
    fn offer_tasks(&mut self, texts: &[(String, String)]) {
        let lines: Vec<String> = api::setting("dotenv.tasks").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let profile = self.task_profile();
        let env = texts.iter().find(|(r, _)| *r == profile).map(|(_, t)| tasks::env(t)).unwrap_or_default();
        let mut offered = Vec::new();
        let mut bad = Vec::new();
        for line in &lines {
            match tasks::command(line) {
                Ok((program, args)) => {
                    let cmd = env.iter().fold(Command::new(&program).args(args).cwd("/project"), |cmd, (k, v)| cmd.env(k, v));
                    offered.push(Task { name: tasks::name(line, &profile), cmd });
                }
                Err(e) => bad.push(e),
            }
        }
        if self.offered.as_ref() == Some(&offered) {
            return;
        }
        for e in bad {
            api::warn!("dotenv.tasks: {e}");
        }
        match project::tasks(&offered) {
            Ok(()) => self.offered = Some(offered),
            Err(e) => api::warn!("tasks: {e}"),
        }
    }

    /// The page's notes: what Git would commit, and the profile `.env` is.
    fn notes(&mut self, texts: &[(String, String)]) -> Vec<(String, fenix_ui_kit::Role)> {
        let mut notes: Vec<(String, fenix_ui_kit::Role)> = self.git.risks().into_iter().map(|(rel, r)| (format!("⚠ {}", r.said(&rel)), fenix_ui_kit::Role::Warn)).collect();
        if let Some(profile) = profile_now(texts) {
            notes.push((format!(".env is .env.{profile}'s copy (p switches)"), fenix_ui_kit::Role::Muted));
        }
        notes
    }
}

/// The profile the project's `.env` is a copy of, if it's one's.
fn profile_now(texts: &[(String, String)]) -> Option<String> {
    let env = texts.iter().find(|(r, _)| r == ".env")?;
    texts.iter().filter(|(r, _)| files::folder(r).is_empty() && !files::is_example(r) && r != ".env").find(|(_, t)| t.trim_end() == env.1.trim_end()).and_then(|(r, _)| files::profile(r).map(String::from))
}

/// The language server, over the `Core`.
struct Server<'a>(&'a mut Core);

impl Server<'_> {
    fn publish(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        let Some(path) = paths::uri_path(uri.as_str()) else { return };
        if !is_env_path(&path) {
            return;
        }
        let Some(text) = docs.text(uri) else { return };
        let rel = paths::below(&path, docs.root.as_deref().and_then(paths::uri_path).as_deref());
        let texts = self.0.texts(&[docs]);
        let lines: Vec<&str> = text.split('\n').collect();
        let at = |line: u32, col: u32| Position { line, character: lsp::char_to_utf16(lines.get(line as usize).copied().unwrap_or(""), col as usize) };
        let diagnostics = self
            .0
            .marks(rel.as_deref(), text, &texts)
            .into_iter()
            .map(|m| Diagnostic {
                range: Range { start: at(m.span.line, m.span.start), end: at(m.span.line, m.span.end) },
                severity: Some(severity(m.severity)),
                source: Some("dotenv".into()),
                message: m.message,
                ..Default::default()
            })
            .collect();
        lsp::publish_diagnostics(server, uri.clone(), diagnostics);
    }
}

impl LanguageServer for Server<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            completion_provider: Some(CompletionOptions { trigger_characters: Some([".", "\"", "'", "(", "[", "{", "$"].map(String::from).to_vec()), ..Default::default() }),
            ..Default::default()
        }
    }

    fn initialized(&mut self, server: u64, root: Option<&str>) {
        if !self.0.server_ids.contains(&server) {
            self.0.server_ids.push(server);
        }
        if self.0.root.is_none() {
            self.0.root = root.and_then(paths::uri_path);
        }
    }

    fn changed(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        self.publish(server, docs, uri);
    }

    fn completion(&mut self, _server: u64, docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        let at = &params.text_document_position;
        let line = docs.line(&at.text_document.uri, at.position.line)?;
        let cursor = lsp::utf16_to_char(line, at.position.character);
        let before: String = line.chars().take(cursor).collect();
        let path = paths::uri_path(at.text_document.uri.as_str()).unwrap_or_default();
        let env_file = is_env_path(&path);
        let site = complete::site(&before, env_file)?;
        // Each key, and the files that set it.
        let mut keys: Vec<(String, Vec<String>)> = Vec::new();
        let mut texts = self.0.texts(&[docs]);
        // An env file outside the project: its own keys.
        if env_file && paths::below(&path, docs.root.as_deref().and_then(paths::uri_path).as_deref()).is_none() {
            texts.push((files::name(&path).to_string(), docs.text(&at.text_document.uri).unwrap_or_default().to_string()));
        }
        for (rel, text) in &texts {
            for key in EnvFile::parse(text).keys() {
                match keys.iter_mut().find(|(k, _)| k == key) {
                    Some((_, in_files)) => {
                        if !in_files.contains(rel) {
                            in_files.push(rel.clone());
                        }
                    }
                    None => keys.push((key.to_string(), vec![rel.clone()])),
                }
            }
        }
        let range = Range { start: Position { line: at.position.line, character: lsp::char_to_utf16(line, site.start) }, end: at.position };
        let items = keys
            .into_iter()
            .filter(|(k, _)| !site.dotted || complete::dottable(k))
            .map(|(key, in_files)| CompletionItem {
                label: key.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("env · {}", in_files.join(", "))),
                filter_text: Some(key.clone()),
                text_edit: Some(CompletionTextEdit::Edit(LspEdit { range, new_text: if site.quote { format!("\"{key}\"") } else { key } })),
                ..Default::default()
            })
            .collect();
        Some(CompletionResponse::Array(items))
    }
}

impl Dotenv {
    /// Every env file's text, as the servers have them or on disk.
    fn texts(&mut self) -> Vec<(String, String)> {
        let ids = self.core.server_ids.clone();
        let docs: Vec<&Documents> = ids.iter().filter_map(|&s| self.servers.documents(s)).collect();
        self.core.texts(&docs)
    }

    /// Every env file a server has open, its diagnostics said again.
    fn republish(&mut self) {
        for server in self.core.server_ids.clone() {
            let Some(docs) = self.servers.documents(server) else { continue };
            let uris: Vec<Uri> = docs.uris().filter(|u| paths::uri_path(u).is_some_and(|p| is_env_path(&p))).filter_map(|u| u.parse().ok()).collect();
            for uri in uris {
                Server(&mut self.core).publish(server, docs, &uri);
            }
        }
    }

    /// Everything that shows what the env files hold, brought up to date.
    fn refresh(&mut self) {
        self.republish();
        let texts = self.texts();
        self.core.doctor(&texts);
        let notes = self.core.notes(&texts);
        self.core.set_rules();
        self.core.offer_tasks(&texts);
        let mask_examples = setting_on("dotenv.mask_examples", false);
        for (&id, page) in self.core.pages.iter_mut() {
            page.load(texts.clone());
            page.notes = notes.clone();
            page.mask_examples = mask_examples;
            pages::refresh(id);
        }
        match profile_now(&texts) {
            Some(p) => api::status(Some(&format!("env: {p}"))),
            None => api::status(None),
        }
    }

    /// The env file a command is about: the one you're in, else none.
    fn here_rel(&mut self) -> Option<String> {
        let here = api::here()?;
        let path = here.path?;
        if !is_env_path(&path) {
            return None;
        }
        if self.core.root.is_none() {
            self.core.root = here.project.map(|p| p.root);
        }
        self.core.rel(&path)
    }

    fn open_page(&mut self) {
        if let Some(&id) = self.core.pages.keys().next() {
            pages::focus(id);
            return;
        }
        let id = pages::open(PAGE, "Env files");
        self.core.pages.insert(id, EnvPage::new(Vec::new()));
        self.refresh();
    }

    fn reveal(&mut self) -> Result<(), String> {
        let path = api::here().and_then(|h| h.path).filter(|p| is_env_path(p)).ok_or("not in an env file")?;
        let shown = !self.core.revealed.remove(&path);
        if shown {
            self.core.revealed.insert(path.clone());
        }
        self.core.decorate(&path);
        self.core.set_rules();
        api::notify(if shown { "Values shown in this file" } else { "Values hidden in this file" });
        Ok(())
    }

    /// The profiles beside `.env` (in the folder of the env file you're
    /// in, else the project's), to pick one to copy over it.
    fn pick_profile(&mut self) -> Result<(), String> {
        let dir = self.here_rel().map(|r| files::folder(&r).to_string()).unwrap_or_default();
        let target = if dir.is_empty() { ".env".to_string() } else { format!("{dir}/.env") };
        let texts = self.texts();
        let now = texts.iter().find(|(r, _)| *r == target).map(|(_, t)| t.trim_end().to_string());
        let items: Vec<PickerItem> = texts
            .iter()
            .filter(|(r, _)| files::folder(r) == dir && *r != target && !files::is_example(files::name(r)))
            .map(|(r, t)| {
                let n = EnvFile::parse(t).keys().len();
                let same = if now.as_deref() == Some(t.trim_end()) { " · what .env is now" } else { "" };
                PickerItem::new(files::name(r), r.clone()).detail(format!("{n} keys{same}"))
            })
            .collect();
        if items.is_empty() {
            return Err(format!("no profiles beside {target}: .env.<name> files"));
        }
        let id = api::pick(&format!("Copy which over {target}?"), &items);
        self.core.asks.insert(id, Ask::Profile(target));
        Ok(())
    }

    /// The env files at the project's top, to pick the one tasks run with.
    fn pick_task_profile(&mut self) -> Result<(), String> {
        let texts = self.texts();
        let now = self.core.task_profile();
        let items: Vec<PickerItem> = texts
            .iter()
            .filter(|(r, _)| files::folder(r).is_empty() && !files::is_example(r))
            .map(|(r, t)| {
                let n = EnvFile::parse(t).keys().len();
                let mark = if *r == now { " · tasks run with it now" } else { "" };
                PickerItem::new(r.clone(), r.clone()).detail(format!("{n} keys{mark}"))
            })
            .collect();
        if items.is_empty() {
            return Err("no env files at the project's top".into());
        }
        let id = api::pick("Run tasks with which env file's variables?", &items);
        self.core.asks.insert(id, Ask::TaskProfile);
        Ok(())
    }

    /// Tasks run with `rel`'s variables from now on, in this project.
    fn set_task_profile(&mut self, rel: String) -> Result<(), String> {
        let root = self.core.root().ok_or("no project")?;
        std::fs::create_dir_all(PROFILES).map_err(|e| format!("{PROFILES}: {e}"))?;
        std::fs::write(project_path(PROFILES, &root), &rel).map_err(|e| format!("{PROFILES}: {e}"))?;
        let none = api::setting("dotenv.tasks").is_none_or(|v| v.trim().is_empty());
        api::notify(&if none { format!("Tasks run with {rel}'s variables: dotenv.tasks says which (npm run dev, cargo run)") } else { format!("Tasks run with {rel}'s variables (SPC p t)") });
        self.core.task_profile = Some(rel);
        self.refresh();
        Ok(())
    }

    fn confirm(&mut self, title: &str, yes: &str, ask: Ask) {
        let id = api::pick(title, &[PickerItem::new(yes, "yes"), PickerItem::new("Cancel", "no")]);
        self.core.asks.insert(id, ask);
    }

    /// `target`'s text kept in its project's backups.
    fn backup(&mut self, target: &str, text: &str) -> Result<String, String> {
        let root = self.core.root().unwrap_or_else(|| "project".into());
        let dir = backup_dir(&root);
        std::fs::create_dir_all(&dir).map_err(|e| format!("backups: {e}"))?;
        let stamp = api::clock::now().format("%Y-%m-%d_%H%M%S").to_string();
        let file = format!("{dir}/{}", backup_name(&stamp, target));
        std::fs::write(&file, text).map_err(|e| format!("backup: {e}"))?;
        Ok(stamp)
    }

    /// `text` copied over `target`, which is kept first.
    fn replace(&mut self, target: &str, text: &str, what: &str) -> Result<(), String> {
        let kept = match read(target) {
            Some(old) if old != text => Some(self.backup(target, &old)?),
            _ => None,
        };
        write(target, text)?;
        self.core.know(target);
        let said = match kept {
            Some(stamp) => format!("{what} copied over {target}; the old one is kept ({stamp}: Env: put back a backup)"),
            None => format!("{what} copied over {target}"),
        };
        api::notify(&said);
        self.refresh();
        Ok(())
    }

    fn pick_backup(&mut self) -> Result<(), String> {
        let root = self.core.root().ok_or("no project")?;
        let dir = backup_dir(&root);
        let mut names: Vec<String> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        names.sort_by(|a, b| b.cmp(a));
        let items: Vec<PickerItem> = names.iter().filter_map(|n| backup_of(n).map(|(stamp, rel)| PickerItem::new(format!("{rel} · {}", stamp.replace('_', " ")), n.clone()))).collect();
        if items.is_empty() {
            return Err("no backups kept for this project".into());
        }
        let id = api::pick("Put back which backup?", &items);
        self.core.asks.insert(id, Ask::Restore);
        Ok(())
    }

    /// The keys its example has that `rel` lacks, added at its end with
    /// empty values, as one change you can undo.
    fn add_missing(&mut self, rel: &str) -> Result<(), String> {
        let texts = self.texts();
        let (example, ex) = self.core.example(rel, &texts).ok_or_else(|| format!("{rel} has no example beside it (.env.example)"))?;
        let text = read(rel).unwrap_or_default();
        let missing = compare::missing(&EnvFile::parse(&text), &ex);
        if missing.is_empty() {
            api::notify(&format!("{rel} has every key {example} has"));
            return Ok(());
        }
        let (at, add) = compare::append(&text, &compare::empty_lines(&missing));
        api::buffer::edit(&paths::project(rel), &[TextEdit::insert(at, add)])?;
        api::notify(&format!("Added to {rel}: {}", missing.join(", ")));
        self.refresh();
        Ok(())
    }

    /// Which file to add missing keys to: the one you're in, else one to
    /// pick of those that lack some.
    fn add_missing_here(&mut self) -> Result<(), String> {
        if let Some(rel) = self.here_rel() {
            return self.add_missing(&rel);
        }
        let texts = self.texts();
        let items: Vec<PickerItem> = texts
            .iter()
            .filter_map(|(rel, text)| {
                let (example, ex) = self.core.example(rel, &texts)?;
                let n = compare::missing(&EnvFile::parse(text), &ex).len();
                (n > 0).then(|| PickerItem::new(rel.clone(), rel.clone()).detail(format!("lacks {n} of {example}'s keys")))
            })
            .collect();
        match items.len() {
            0 => Err("no env file lacks a key its example has".into()),
            1 => self.add_missing(&items[0].value),
            _ => {
                let id = api::pick("Add missing keys to which?", &items);
                self.core.asks.insert(id, Ask::AddMissing);
                Ok(())
            }
        }
    }

    /// The env files Git would commit added to the project's `.gitignore`.
    fn gitignore(&mut self) -> Result<(), String> {
        if self.core.git.asked.is_empty() && self.core.runs.is_empty() {
            self.core.check_git();
        }
        if !self.core.runs.is_empty() {
            return Err("Git hasn't answered yet: try again in a moment".into());
        }
        if self.core.git.asked.is_empty() {
            return Err("no env files here, but examples".into());
        }
        if !self.core.git.repo {
            return Err("the project isn't in a Git repository".into());
        }
        let rels: Vec<String> = self.core.git.asked.iter().filter(|r| !self.core.git.ignored.contains(*r)).cloned().collect();
        let old = read(".gitignore").unwrap_or_default();
        let lines = git::ignore_lines(&old, &rels);
        if lines.is_empty() {
            api::notify(if self.core.git.tracked.is_empty() { "Every env file is ignored" } else { "Every env file is in .gitignore; some are tracked still (Env: stop tracking)" });
            return Ok(());
        }
        let at = paths::project(".gitignore");
        if api::buffer::text(&at).is_some() {
            let (pos, add) = compare::append(&old, &lines);
            api::buffer::edit(&at, &[TextEdit::insert(pos, add)])?;
            api::buffer::save(&at)?;
        } else {
            let (_, add) = compare::append(&old, &lines);
            std::fs::write(&at, old + &add).map_err(|e| format!(".gitignore: {e}"))?;
        }
        api::notify(&format!("Added to .gitignore: {}", lines.join(", ")));
        self.core.check_git();
        Ok(())
    }

    fn untrack(&mut self) -> Result<(), String> {
        let rels: Vec<String> = self.core.git.risks().into_iter().filter(|(_, r)| *r == Risk::Tracked).map(|(f, _)| f).collect();
        if rels.is_empty() {
            return Err("Git tracks no env file here".into());
        }
        let title = format!("Stop tracking {} in Git? The files stay; their history doesn't go", rels.join(", "));
        self.confirm(&title, "Stop tracking them (git rm --cached)", Ask::Untrack(rels));
        Ok(())
    }

    fn picked(&mut self, ask: Ask, value: String) -> Result<(), String> {
        match ask {
            Ask::Profile(target) => {
                let title = format!("Copy {} over {target}? The old one is kept", files::name(&value));
                self.confirm(&title, &format!("Copy {} over {target}", files::name(&value)), Ask::CopyProfile { target, from: value });
                Ok(())
            }
            Ask::CopyProfile { target, from } if value == "yes" => {
                let text = read(&from).ok_or_else(|| format!("{from} can't be read"))?;
                self.replace(&target, &text, &from)
            }
            Ask::Restore => {
                let (stamp, rel) = backup_of(&value).ok_or("not a backup")?;
                self.confirm(&format!("Put back {rel} as it was at {}?", stamp.replace('_', " ")), &format!("Put it back over {rel}"), Ask::PutBack { target: rel, backup: value });
                Ok(())
            }
            Ask::PutBack { target, backup } if value == "yes" => {
                let root = self.core.root().ok_or("no project")?;
                let text = std::fs::read_to_string(format!("{}/{backup}", backup_dir(&root))).map_err(|e| e.to_string())?;
                self.replace(&target, &text, "The backup")
            }
            Ask::AddMissing => self.add_missing(&value),
            Ask::TaskProfile => self.set_task_profile(value),
            Ask::Untrack(rels) if value == "yes" => {
                let args: Vec<String> = ["rm", "--cached", "--quiet", "--"].into_iter().map(String::from).chain(rels.iter().cloned()).collect();
                let id = Command::new("git").args(args).cwd("/project").spawn()?;
                self.core.runs.insert(id, (Run::Untrack(rels), Vec::new()));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn exited(&mut self, id: u64, code: Option<i32>) {
        let Some((run, out)) = self.core.runs.remove(&id) else { return };
        match run {
            Run::Tracked(asked) => {
                self.core.git.repo = code == Some(0);
                self.core.git.tracked = git::paths(&out);
                self.core.git.asked = asked;
            }
            Run::Ignored => self.core.git.ignored = if matches!(code, Some(0 | 1)) { git::paths(&out) } else { HashSet::new() },
            Run::Untrack(rels) => {
                if code == Some(0) {
                    api::notify(&format!("Git no longer tracks {}: commit that, and add them to .gitignore", rels.join(", ")));
                } else {
                    error(&format!("git rm --cached: {}", out.join(" ")));
                }
                api::forge::refresh();
                self.core.check_git();
                return;
            }
        }
        if self.core.runs.is_empty() {
            if std::mem::take(&mut self.core.git_again) {
                self.core.check_git();
            }
            self.refresh();
        }
    }

    fn page_action(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::Nothing | Action::Redraw => Ok(()),
            Action::Open(rel, line) => {
                host::open_file(&paths::project(&rel), line.map(|l| l + 1));
                Ok(())
            }
            Action::Copy(text) => {
                api::copy(&text);
                api::notify("Copied");
                Ok(())
            }
            Action::AddMissing(rel) => self.add_missing(&rel),
            Action::Profile => self.pick_profile(),
            Action::Ignore => self.gitignore(),
            Action::Reload => {
                self.core.found = None;
                self.core.check_git();
                self.refresh();
                Ok(())
            }
        }
    }
}

impl Extension for Dotenv {
    /// Its rules given Fenix, so an env file is masked from its first
    /// frame, and its tasks offered.
    fn activate(&mut self) -> Result<(), String> {
        self.core.set_rules();
        let texts = self.texts();
        self.core.offer_tasks(&texts);
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "dotenv.page" => {
                self.open_page();
                Ok(())
            }
            "dotenv.reveal" => self.reveal(),
            "dotenv.profile" => self.pick_profile(),
            "dotenv.restore" => self.pick_backup(),
            "dotenv.task_profile" => self.pick_task_profile(),
            "dotenv.add_missing" => self.add_missing_here(),
            "dotenv.gitignore" => self.gitignore(),
            "dotenv.untrack" => self.untrack(),
            "dotenv.check" => {
                self.core.found = None;
                self.core.check_git();
                self.refresh();
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Picked((id, value)) => {
                if let (Some(ask), Some(value)) = (self.core.asks.remove(&id), value) {
                    if let Err(e) = self.picked(ask, value) {
                        error(&e);
                    }
                }
            }
            Event::ProcessOutput(o) => {
                if let Some((_, out)) = self.core.runs.get_mut(&o.process) {
                    out.push(o.line);
                }
            }
            Event::ProcessExit((id, code)) => self.exited(id, code),
            // Its name only: a new env file is looked for again.
            Event::Saved(name) => {
                if name == ".gitignore" {
                    self.core.check_git();
                } else if files::is_env(&name) {
                    if self.core.found.as_ref().is_some_and(|f| !f.iter().any(|r| files::name(r) == name)) {
                        self.core.found = None;
                        self.core.check_git();
                    }
                    self.refresh();
                }
            }
            Event::SettingChanged(_) => {
                if let Some(path) = self.core.focused.clone() {
                    self.core.decorate(&path);
                }
                self.core.check_git();
                self.refresh();
            }
            _ => {}
        }
    }

    fn buffer_changed(&mut self, what: Changed) {
        let Changed::File(path) = what else { return };
        if !is_env_path(&path) {
            return;
        }
        self.core.decorate(&path);
        self.refresh();
    }

    fn focus_moved(&mut self, focus: Focus) {
        let Focus::File(Some(path)) = focus else { return };
        if self.core.root.is_none() {
            self.core.root();
        }
        if !is_env_path(&path) {
            self.core.focused = None;
            return;
        }
        self.core.focused = Some(path.clone());
        self.core.decorate(&path);
        let rel = self.core.rel(&path);
        let first = self.core.found.is_none() || self.core.git.asked.is_empty();
        if let Some(rel) = rel {
            self.core.know(&rel);
        }
        if first {
            self.core.check_git();
        }
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.servers.handle(server, message, &mut Server(&mut self.core));
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let canvas = self.core.pages.get(&page).map(|p| p.render(width as usize, height as usize)).unwrap_or_default();
        Page::canvas(canvas.to_api())
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(p) = self.core.pages.get_mut(&page) else { return };
        let action = p.key(key.into());
        if let Err(e) = self.page_action(action) {
            error(&e);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.core.pages.contains_key(&page).then(String::new)
    }

    fn page_restore(&mut self, page: u64, kind: &str, _state: &str) -> bool {
        if kind != PAGE {
            return false;
        }
        self.core.pages.insert(page, EnvPage::new(Vec::new()));
        self.refresh();
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.core.pages.remove(&page);
    }
}

export!(Dotenv);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_are_named_for_their_file_and_project() {
        let name = backup_name("2026-10-29_101500", "api/.env");
        assert_eq!(name, "2026-10-29_101500__api%.env");
        assert_eq!(backup_of(&name), Some(("2026-10-29_101500".into(), "api/.env".into())));
        assert!(backup_dir("/home/me/app").starts_with("/data/backups/app-"));
        assert_ne!(backup_dir("/home/me/app"), backup_dir("/work/app"));
        assert_eq!(project_path(PROFILES, "/home/me/app").rsplit('/').next(), backup_dir("/home/me/app").rsplit('/').next());
    }

    /// The language files as Fenix reads them: in order of their names, a
    /// later pattern winning, a file's extension after them.
    #[test]
    fn env_files_are_shell_and_code_named_like_them_is_not() {
        fn glob(p: &[char], n: &[char]) -> bool {
            match p.first() {
                None => n.is_empty(),
                Some('*') => (0..=n.len()).any(|i| glob(&p[1..], &n[i..])),
                Some(c) => n.first() == Some(c) && glob(&p[1..], &n[1..]),
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("languages");
        let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        let mut patterns: Vec<(String, String)> = Vec::new();
        let mut extensions: Vec<(String, String)> = Vec::new();
        for path in paths {
            let text = std::fs::read_to_string(path).unwrap();
            let field = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim_matches(['[', ']', '"', ' ']).split("\", \"").map(String::from).collect::<Vec<_>>()).unwrap_or_default();
            let language = field("language = ").remove(0);
            patterns.extend(field("filenames = ").into_iter().map(|f| (f, language.clone())));
            extensions.extend(field("extensions = ").into_iter().map(|e| (e, language.clone())));
        }
        let language = |name: &str| {
            let n: Vec<char> = name.chars().collect();
            patterns.iter().rev().find(|(p, _)| glob(&p.chars().collect::<Vec<_>>(), &n)).or_else(|| extensions.iter().find(|(e, _)| name.ends_with(&format!(".{e}")))).map(|(_, l)| l.as_str())
        };
        for env in [".env", ".env.staging", ".env.anything-at-all", ".env.example", "prod.env"] {
            assert_eq!(language(env), Some("bash"), "{env}");
        }
        for (code, lang) in [(".env.d.ts", "typescript"), (".env.mjs", "javascript"), (".env.json", "json"), (".env.local.yml", "yaml"), (".env.py", "python")] {
            assert_eq!(language(code), Some(lang), "{code}");
        }
    }

    #[test]
    fn the_profile_env_is_a_copy_of() {
        let texts = |env: &str| vec![(".env".to_string(), env.to_string()), (".env.example".into(), "A=\n".into()), (".env.staging".into(), "A=1\n".into()), ("api/.env.prod".into(), "A=2\n".into())];
        assert_eq!(profile_now(&texts("A=1")).as_deref(), Some("staging"));
        assert_eq!(profile_now(&texts("A=2\n")), None, "only beside it");
        assert_eq!(profile_now(&texts("A=\n")), None, "not an example");
    }
}
