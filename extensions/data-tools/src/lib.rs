//! JSON, YAML and TOML tools (`SPC x`): format with the project's indent,
//! minify, sort keys, convert between the three, query with JSONPath or
//! jq, escape and unescape strings, copy the path to the key under the
//! cursor, and make types from a sample -- each on the selection, else the
//! whole buffer, as one change you can undo.
//!
//! It's a language server too, for JSON, YAML and TOML files: what
//! doesn't parse, and what the file's JSON Schema says against it, are
//! its diagnostics. The schema is one picked for the file, the one it
//! names (`$schema`), or SchemaStore's for its name; fetched once and kept
//! under `/data/schemas`.

mod diagnose;
mod doc;
mod edit;
mod editorconfig;
mod glob;
mod json;
mod net;
mod node;
mod path;
mod schema;
mod toml_doc;
mod types;
mod yaml;

use std::collections::{BTreeMap, HashMap};

use fenix_extension_api::lsp::types::{Diagnostic, DiagnosticSeverity, Position, Range, ServerCapabilities, Uri};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{self as api, export, host, Command, Event, Extension, PickerItem, TextEdit};

use doc::{Format, Indent};
use edit::Target;
use net::Again;
use node::{Lines, Pos};

/// Schemas you picked for files, by path.
const PICKED: &str = "/data/picked.json";

/// What a picker or prompt it opened is for.
enum Ask {
    Query(Source),
    ConvertInPlace(Source),
    Types(Source),
    Schema(String),
    SchemaUrl(String),
}

/// What a command works on, and where it goes back.
struct Source {
    format: Format,
    target: Target,
    /// The file it's from; none in a buffer that isn't one.
    path: Option<String>,
    /// Its name without its extension, for what's made from it.
    stem: String,
    indent: Indent,
}

/// jq running: what it's written so far.
struct Jq {
    out: Vec<String>,
    err: Vec<String>,
}

#[derive(Default)]
pub struct DataTools {
    servers: Servers,
    core: Core,
}

#[derive(Default)]
struct Core {
    /// Its language servers, one a project.
    server_ids: Vec<u64>,
    asks: HashMap<u64, Ask>,
    jq: HashMap<u64, Jq>,
    query_scratch: Option<u64>,
    last_query: String,
    /// Compiled schemas, by where they are, with how many schemas were
    /// fetched when they were compiled.
    validators: HashMap<String, (usize, Box<jsonschema::Validator>)>,
    /// Files to check again once this call's done, and whether to say how
    /// it went.
    pending: Vec<(String, bool)>,
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

fn setting(key: &str) -> String {
    api::setting(key).unwrap_or_default().trim().to_string()
}

fn on(key: &str) -> bool {
    setting(key) != "false"
}

/// The file's name without its extension.
fn stem(path: Option<&str>) -> String {
    path.and_then(|p| p.replace('\\', "/").rsplit('/').next().map(str::to_string)).map(|name| name.split('.').find(|s| !s.is_empty()).unwrap_or("data").to_string()).unwrap_or_else(|| "data".into())
}

/// The indent formatting uses: its setting, the project's `.editorconfig`,
/// then Fenix's indent width.
fn indent_for(path: Option<&str>, root: Option<&str>) -> Indent {
    match setting("data-tools.indent").as_str() {
        "tab" | "tabs" => return Indent::Tab,
        n => {
            if let Ok(n) = n.parse::<usize>() {
                return Indent::Spaces(n);
            }
        }
    }
    if let (Some(path), Some(root)) = (path, root) {
        let (path, root) = (path.replace('\\', "/"), root.replace('\\', "/").trim_end_matches('/').to_string());
        if let Some(rel) = path.strip_prefix(&format!("{root}/")) {
            // From the file's folder up to the project's, nearest first.
            let parts: Vec<&str> = rel.split('/').collect();
            let mut configs = Vec::new();
            for depth in (0..parts.len()).rev() {
                let dir = parts[..depth].join("/");
                let file = if dir.is_empty() { "/project/.editorconfig".to_string() } else { format!("/project/{dir}/.editorconfig") };
                if let Ok(text) = std::fs::read_to_string(file) {
                    configs.push((parts[depth..].join("/"), text));
                }
            }
            if let Some(i) = editorconfig::indent(&configs) {
                return i;
            }
        }
    }
    api::setting("editor.indent_width").and_then(|w| w.trim().parse().ok()).map_or(Indent::Spaces(2), Indent::Spaces)
}

/// The selection, else the buffer, of where you are.
fn source() -> Result<Source, String> {
    let here = api::here().ok_or("not in a buffer of text")?;
    let cursor = api::cursor();
    let selection = cursor.as_ref().filter(|c| c.selected).and_then(|c| c.selection).map(|(a, b)| (Pos::new(a.0, a.1), Pos::new(b.0, b.1)));
    let buffer = here.path.as_deref().and_then(api::buffer::text);
    let target = match (&buffer, selection) {
        (Some(text), _) => edit::target(text, selection),
        // Not a file: what `here` has, the selection or the line.
        (None, _) => edit::target(&here.text, None),
    };
    if target.text.trim().is_empty() {
        return Err("there's nothing here to work on".into());
    }
    let format = doc::detect(if selection.is_some() && Format::from_language(&here.language).is_none() { "" } else { &here.language }, &target.text);
    let root = here.project.as_ref().map(|p| p.root.clone());
    Ok(Source { format, indent: indent_for(here.path.as_deref(), root.as_deref()), stem: stem(here.path.as_deref()), path: buffer.and(here.path), target })
}

/// A result in a scratch buffer of its own: beside the one you're in,
/// unless `data-tools.results` says here.
fn open_result(title: &str, text: &str, language: &str) -> u64 {
    if setting("data-tools.results") != "here" {
        host::run_command("window.split_vertical");
    }
    api::scratch::open(title, text, Some(language))
}

impl Source {
    /// `out` in place of what it worked on; a buffer that's no file gets
    /// it beside instead.
    fn replace(&self, out: &str, what: &str, note: Option<String>) {
        let said = match &self.path {
            Some(path) => {
                let t = &self.target;
                match api::buffer::edit(path, &[TextEdit::new((t.start.line, t.start.col), (t.end.line, t.end.col), t.finish(out))]) {
                    Ok(()) => what.to_string(),
                    Err(e) => return error(&e),
                }
            }
            None => {
                open_result(&format!("{}.{}", self.stem, self.format.ext()), out, self.format.ext());
                format!("{what} (not a file: beside it)")
            }
        };
        api::notify(&match note {
            Some(n) => format!("{said}; {n}"),
            None => said,
        });
    }
}

impl Core {
    fn run(&mut self, id: &str) -> Result<(), String> {
        match id {
            "data-tools.format" => {
                let s = source()?;
                let done = doc::format(s.format, &s.target.text, s.indent)?;
                s.replace(&done.text, &format!("formatted {}", s.format.name()), done.note);
            }
            "data-tools.minify" => {
                let s = source()?;
                let done = doc::minify(s.format, &s.target.text)?;
                s.replace(&done.text, &format!("minified {}", s.format.name()), done.note);
            }
            "data-tools.sort" | "data-tools.sort_top" => {
                let s = source()?;
                let recursive = id == "data-tools.sort";
                let done = doc::sort(s.format, &s.target.text, recursive, s.indent)?;
                s.replace(&done.text, if recursive { "sorted every object's keys" } else { "sorted the top level's keys" }, done.note);
            }
            "data-tools.to_json" | "data-tools.to_yaml" | "data-tools.to_toml" => {
                let s = source()?;
                let to = Format::from_name(id.trim_start_matches("data-tools.to_")).unwrap_or(Format::Json);
                let done = doc::convert(s.format, &s.target.text, to, s.indent)?;
                open_result(&format!("{}.{}", s.stem, to.ext()), &format!("{}\n", done.text), to.ext());
                if let Some(n) = done.note {
                    api::notify(&n);
                }
            }
            "data-tools.convert" => {
                let s = source()?;
                let items: Vec<PickerItem> = [Format::Json, Format::Yaml, Format::Toml]
                    .into_iter()
                    .filter(|f| *f != s.format)
                    .map(|f| PickerItem::new(f.name(), f.ext()).detail(format!("{} to {}, in its place", s.format.name(), f.name())))
                    .collect();
                let id = api::pick("Convert, in place, to", &items);
                self.asks.insert(id, Ask::ConvertInPlace(s));
            }
            "data-tools.query" => {
                let s = source()?;
                let jq = if api::find_program("jq", &[]).is_some() { ", or a jq filter" } else { "" };
                let id = api::prompt(&format!("Query: JSONPath ($.a[0].b){jq}"), &self.last_query);
                self.asks.insert(id, Ask::Query(s));
            }
            "data-tools.types" => {
                let s = source()?;
                let items: Vec<PickerItem> = types::Lang::ALL.iter().map(|l| PickerItem::new(l.name(), l.id())).collect();
                let id = api::pick("Types from this sample, in", &items);
                self.asks.insert(id, Ask::Types(s));
            }
            "data-tools.escape" | "data-tools.unescape" | "data-tools.stringify" | "data-tools.parse_string" => {
                let s = source()?;
                let (out, what, note) = match id {
                    "data-tools.escape" => (json::quote(&s.target.text), "as a JSON string", None),
                    "data-tools.unescape" => (json::unquote(&s.target.text).map_err(|e| format!("the string: {e}"))?, "unescaped", None),
                    "data-tools.stringify" => {
                        let d = doc::to_string_literal(s.format, &s.target.text)?;
                        (d.text, "as a string of JSON", d.note)
                    }
                    _ => {
                        let d = doc::from_string_literal(&s.target.text, s.indent)?;
                        (d.text, "the string's JSON, formatted", d.note)
                    }
                };
                s.replace(&out, what, note);
            }
            "data-tools.copy_path" => self.copy_path()?,
            "data-tools.validate" => {
                let here = api::here().ok_or("not in a buffer of text")?;
                let path = here.path.ok_or("not a file: only a file is checked against a schema")?;
                self.check_path(&path, true);
            }
            "data-tools.schema" => {
                let here = api::here().ok_or("not in a buffer of text")?;
                let path = here.path.ok_or("not a file: a schema is picked for a file")?;
                self.pick_schema(&path);
            }
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn copy_path(&self) -> Result<(), String> {
        let here = api::here().ok_or("not in a buffer of text")?;
        let cursor = api::cursor().ok_or("not in a buffer of text")?;
        let text = here.path.as_deref().and_then(api::buffer::text).ok_or("not a file: the path is to a key in a file")?;
        let format = doc::detect(&here.language, &text);
        let parsed = doc::parse(format, &text)?;
        let located = path::locate(&parsed.node);
        let at = Pos::new(cursor.at.0, cursor.at.1);
        let found = path::at(&located, at).ok_or("the cursor isn't on a value")?;
        let style = path::Style::from_setting(&setting("data-tools.path_style"));
        let rendered = path::render(&found.path, style);
        api::copy(&rendered);
        api::notify(&format!("copied {rendered}"));
        Ok(())
    }

    fn picked(&mut self, id: u64, value: Option<String>) {
        let (Some(ask), Some(value)) = (self.asks.remove(&id), value) else { return };
        let result = match ask {
            Ask::ConvertInPlace(s) => (|| {
                let to = Format::from_name(&value).ok_or("no such format")?;
                let done = doc::convert(s.format, &s.target.text, to, s.indent)?;
                s.replace(&done.text, &format!("{} to {}", s.format.name(), to.name()), done.note);
                Ok::<(), String>(())
            })(),
            Ask::Types(s) => (|| {
                let lang = types::Lang::from_id(&value).ok_or("no such language")?;
                let parsed = doc::parse(s.format, &s.target.text)?;
                let out = types::generate(&parsed.node.to_json(), &s.stem, lang);
                open_result(&format!("{}.{}", s.stem, lang.ext()), &out, lang.ext());
                Ok(())
            })(),
            Ask::Schema(path) => {
                match value.as_str() {
                    "\u{0}auto" => self.set_picked(&path, None),
                    "\u{0}enter" => {
                        let id = api::prompt("Schema: a URL, or a path from the file's folder", "");
                        self.asks.insert(id, Ask::SchemaUrl(path));
                    }
                    url => self.set_picked(&path, Some(url)),
                }
                Ok(())
            }
            Ask::Query(_) | Ask::SchemaUrl(_) => Ok(()),
        };
        if let Err(e) = result {
            error(&e);
        }
    }

    fn prompted(&mut self, id: u64, text: Option<String>) {
        let Some(text) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
            self.asks.remove(&id);
            return;
        };
        match self.asks.remove(&id) {
            Some(Ask::Query(s)) => {
                self.last_query = text.clone();
                if let Err(e) = self.query(&s, &text) {
                    error(&e);
                }
            }
            Some(Ask::SchemaUrl(path)) => self.set_picked(&path, Some(&text)),
            _ => {}
        }
    }

    fn show_query(&mut self, title: &str, text: &str) {
        if let Some(id) = self.query_scratch {
            if api::scratch::set(id, text).is_ok() {
                return api::notify(title);
            }
        }
        self.query_scratch = Some(open_result("query.json", text, "json"));
        api::notify(title);
    }

    fn query(&mut self, s: &Source, q: &str) -> Result<(), String> {
        if q.starts_with('$') {
            let (out, n) = doc::query(s.format, &s.target.text, q, s.indent)?;
            self.show_query(&format!("{q}: {n} match{}", if n == 1 { "" } else { "es" }), &format!("{out}\n"));
            return Ok(());
        }
        if api::find_program("jq", &[]).is_none() {
            return Err("jq isn't installed: a JSONPath query starts with $".into());
        }
        let parsed = doc::parse(s.format, &s.target.text)?;
        let input = json::write(&parsed.node, &json::Style::Minified)?;
        std::fs::write("/data/query.json", input).map_err(|e| format!("couldn't write jq's input: {e}"))?;
        let id = Command::new("jq")
            .args([
                "--indent",
                &match s.indent {
                    Indent::Spaces(n) => n.min(7).to_string(),
                    Indent::Tab => "2".into(),
                },
            ])
            .args([q, "query.json"])
            .cwd("/data")
            .spawn()?;
        self.jq.insert(id, Jq { out: Vec::new(), err: Vec::new() });
        Ok(())
    }

    fn process_output(&mut self, id: u64, stderr: bool, line: String) {
        if let Some(jq) = self.jq.get_mut(&id) {
            if stderr {
                jq.err.push(line)
            } else {
                jq.out.push(line)
            }
        }
    }

    fn process_exit(&mut self, id: u64, code: Option<i32>) {
        let Some(jq) = self.jq.remove(&id) else { return };
        if code == Some(0) {
            let mut text = jq.out.join("\n");
            text.push('\n');
            self.show_query(&format!("jq {}", self.last_query), &text);
        } else {
            error(&format!("jq: {}", jq.err.first().map(String::as_str).unwrap_or("it failed")));
        }
    }

    // Schemas.

    fn picked_schemas() -> BTreeMap<String, String> {
        std::fs::read_to_string(PICKED).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn set_picked(&mut self, path: &str, schema: Option<&str>) {
        let mut all = Self::picked_schemas();
        match schema {
            Some(s) => all.insert(path.to_string(), s.to_string()),
            None => all.remove(path),
        };
        let written = serde_json::to_string_pretty(&all).map_err(|e| e.to_string()).and_then(|t| std::fs::write(PICKED, t).map_err(|e| e.to_string()));
        if let Err(e) = written {
            return error(&format!("couldn't keep the schema picked: {e}"));
        }
        self.check_path(path, true);
    }

    fn pick_schema(&mut self, path: &str) {
        let catalog = net::catalog(Again::Pick(path.to_string()));
        if catalog.is_none() && net::failed(net::CATALOG).is_none() {
            return api::notify("fetching SchemaStore's catalog...");
        }
        let mut items =
            vec![PickerItem::new("As the file says", "\u{0}auto").detail("its $schema, else SchemaStore's for its name"), PickerItem::new("A URL or a file...", "\u{0}enter").detail("typed in")];
        if let Some(c) = catalog {
            items.extend(c.entries.iter().map(|e| PickerItem::new(e.name.clone(), e.url.clone()).detail(if e.description.is_empty() { e.url.clone() } else { e.description.clone() })));
        }
        let id = api::pick("Check this file against", &items);
        self.asks.insert(id, Ask::Schema(path.to_string()));
    }

    /// The schema for the file at `path`, and whether it was asked for (so
    /// it not being there is worth saying).
    fn schema_for(&self, path: &str, text: &str, node: &node::Node) -> Option<(String, bool)> {
        if let Some(s) = Self::picked_schemas().into_iter().find(|(p, _)| diagnose::same_path(p, path)).map(|(_, s)| s) {
            return Some((s, true));
        }
        let patterns: Vec<(String, String)> = api::setting("data-tools.schemas").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if let Some((_, s)) = patterns.into_iter().find(|(pattern, _)| glob::file_matches(pattern, path)) {
            return Some((s, true));
        }
        if let Some(s) = schema::declared(text, node) {
            return Some((s, true));
        }
        if !on("data-tools.schemastore") {
            return None;
        }
        let catalog = net::catalog(Again::Check(path.to_string(), false))?;
        catalog.find(path).map(|e| (e.url.clone(), false))
    }

    /// A schema file on disk: in the project, or one Fenix gave it.
    fn read_schema_file(file: &str, root: Option<&str>) -> Result<serde_json::Value, String> {
        let rel = root.and_then(|r| file.replace('\\', "/").strip_prefix(&format!("{}/", r.replace('\\', "/").trim_end_matches('/'))).map(str::to_string));
        let text = match rel {
            Some(rel) => std::fs::read_to_string(format!("/project/{rel}")).map_err(|e| e.to_string())?,
            None => String::from_utf8_lossy(&api::read_file(file)?).into_owned(),
        };
        serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}"))
    }

    /// The file at `path` to check again (`DataTools::recheck`): its
    /// problems published if a server of its has it open, said if `tell`.
    fn check_path(&mut self, path: &str, tell: bool) {
        self.pending.push((path.to_string(), tell));
    }
}

/// A document one of its servers has open.
struct Open {
    server: u64,
    uri: String,
    root: Option<String>,
}

impl Core {
    /// `text`, the file at `path`, checked: the marks to show, and what to
    /// say of it.
    fn check(&mut self, path: &str, text: &str, root: Option<&str>) -> (Vec<diagnose::Mark>, Vec<diagnose::Mark>, Option<String>) {
        let ext = path.rsplit('.').next().unwrap_or("");
        let format = Format::from_language(ext).unwrap_or_else(|| doc::detect("", text));
        let parsed = match format {
            Format::Json => json::parse(text).map(|r| r.node),
            Format::Yaml => yaml::parse(text),
            Format::Toml => toml_doc::parse(text),
        };
        let node = match parsed {
            Ok(n) => n,
            Err(e) => return (vec![diagnose::syntax(e.at, &format!("{}: {}", format.name(), e.message))], Vec::new(), Some(format!("{} doesn't parse: {e}", format.name()))),
        };
        let Some((reference, asked)) = self.schema_for(path, text, &node) else {
            return (Vec::new(), Vec::new(), Some("no schema for this file: SPC x V picks one".into()));
        };
        let src = schema::source(&reference, path);
        let key = src.key();
        let (value, base) = match &src {
            schema::Source::Url(url) => match net::schema(url, Again::Check(path.to_string(), false)) {
                Some(v) => (v, Some(url.clone())),
                None => {
                    let said = match net::failed(url) {
                        Some(why) => format!("couldn't fetch the schema {url}: {why}"),
                        None => format!("fetching the schema {url}..."),
                    };
                    let warn = if asked && net::failed(url).is_some() { vec![diagnose::syntax(Pos::default(), &said)] } else { Vec::new() };
                    return (Vec::new(), warn, Some(said));
                }
            },
            schema::Source::File(file) => match Self::read_schema_file(file, root) {
                Ok(v) => (v, None),
                Err(e) => {
                    let said = format!("couldn't read the schema {file}: {e}");
                    return (Vec::new(), vec![diagnose::syntax(Pos::default(), &said)], Some(said));
                }
            },
        };
        let store = net::store();
        let fetched = store.0.lock().map(|s| s.len()).unwrap_or(0);
        if self.validators.get(&key).is_none_or(|(n, _)| *n != fetched) {
            match schema::compile(&value, base.as_deref(), &store) {
                schema::Compiled::Ready(v) => {
                    self.validators.insert(key.clone(), (fetched, v));
                }
                schema::Compiled::Needs(urls) => {
                    let mut waiting = false;
                    for url in &urls {
                        if net::schema(url, Again::Check(path.to_string(), false)).is_none() && net::failed(url).is_none() {
                            waiting = true;
                        }
                    }
                    if waiting {
                        return (Vec::new(), Vec::new(), Some(format!("fetching what {reference} refers to...")));
                    }
                    let said = format!("the schema {reference} refers to what couldn't be fetched: {}", urls.join(", "));
                    return (Vec::new(), vec![diagnose::syntax(Pos::default(), &said)], Some(said));
                }
                schema::Compiled::Broken(e) => {
                    let said = format!("the schema {reference} is broken: {e}");
                    return (Vec::new(), vec![diagnose::syntax(Pos::default(), &said)], Some(said));
                }
            }
        }
        let Some((_, validator)) = self.validators.get(&key) else { return (Vec::new(), Vec::new(), None) };
        let problems = schema::check(validator, &node.to_json());
        let located = path::locate(&node);
        let marks = diagnose::problems(&problems, &located, &Lines::new(text));
        let said = match marks.len() {
            0 => format!("valid against {reference}"),
            1 => format!("1 problem against {reference}"),
            n => format!("{n} problems against {reference}"),
        };
        (Vec::new(), marks, Some(said))
    }

    fn publish(&self, open: &Open, text: &str, errors: &[diagnose::Mark], warnings: &[diagnose::Mark]) {
        let Ok(uri) = open.uri.parse::<Uri>() else { return };
        let lines = Lines::new(text);
        let at = |p: Pos| Position { line: p.line, character: lsp::char_to_utf16(lines.line(p.line), p.col as usize) };
        let diagnostic = |m: &diagnose::Mark, severity| Diagnostic {
            range: Range { start: at(m.start), end: at(m.end) },
            severity: Some(severity),
            source: Some("data-tools".into()),
            message: m.message.clone(),
            ..Default::default()
        };
        let all = errors.iter().map(|m| diagnostic(m, DiagnosticSeverity::ERROR)).chain(warnings.iter().map(|m| diagnostic(m, DiagnosticSeverity::WARNING))).collect();
        lsp::publish_diagnostics(open.server, uri, all);
    }

    /// Checks a document a server has open, publishing what it finds.
    fn check_open(&mut self, open: &Open, text: &str, tell: bool) {
        let path = diagnose::uri_path(&open.uri).unwrap_or_default();
        if !on("data-tools.check") {
            self.publish(open, text, &[], &[]);
            return;
        }
        let (errors, warnings, said) = self.check(&path, text, open.root.as_deref());
        self.publish(open, text, &errors, &warnings);
        if let (true, Some(said)) = (tell, said) {
            api::notify(&said);
        }
    }
}

/// The language server, over the `Core` that checks.
struct Checker<'a>(&'a mut Core);

impl LanguageServer for Checker<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities::default()
    }

    fn changed(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        let Some(text) = docs.text(uri) else { return };
        let open = Open { server, uri: uri.as_str().to_string(), root: docs.root.as_deref().and_then(diagnose::uri_path) };
        self.0.check_open(&open, text, false);
    }
}

impl DataTools {
    /// The files to check again: each where a server has it open, else
    /// (when it's to be said) from its buffer.
    fn recheck(&mut self) {
        let mut todo: Vec<(String, bool)> = std::mem::take(&mut self.core.pending);
        for again in net::again() {
            match again {
                Again::Check(path, tell) => todo.push((path, tell)),
                Again::Pick(path) => self.core.pick_schema(&path),
                Again::Nothing => {}
            }
        }
        todo.sort();
        todo.dedup_by(|a, b| {
            a.0 == b.0 && {
                b.1 |= a.1;
                true
            }
        });
        for (path, tell) in todo {
            let open = self.core.server_ids.iter().find_map(|&server| {
                let docs = self.servers.documents(server)?;
                let uri = docs.uris().find(|u| diagnose::uri_path(u).is_some_and(|p| diagnose::same_path(&p, &path)))?.to_string();
                let text = docs.text(&uri.parse().ok()?)?.to_string();
                Some((Open { server, uri, root: docs.root.as_deref().and_then(diagnose::uri_path) }, text))
            });
            match open {
                Some((open, text)) => self.core.check_open(&open, &text, tell),
                None if tell => {
                    let Some(text) = api::buffer::text(&path) else { continue };
                    let root = api::here().and_then(|h| h.project).map(|p| p.root);
                    let (errors, warnings, said) = self.core.check(&path, &text, root.as_deref());
                    let n = errors.len() + warnings.len();
                    if let Some(said) = said {
                        api::notify(&if n > 0 && errors.is_empty() { format!("{said} (not shown: Fenix hasn't opened the file to it)") } else { said });
                    }
                }
                None => {}
            }
        }
    }
}

impl Extension for DataTools {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let result = self.core.run(id);
        self.recheck();
        result
    }

    fn handle(&mut self, event: Event) {
        let Some(event) = api::run::deliver(event) else {
            self.recheck();
            return;
        };
        match event {
            Event::Picked((id, value)) => self.core.picked(id, value),
            Event::Prompted((id, text)) => self.core.prompted(id, text),
            Event::ProcessOutput(o) => self.core.process_output(o.process, o.stderr, o.line),
            Event::ProcessExit((id, code)) => self.core.process_exit(id, code),
            Event::SettingChanged(_) => {
                let paths: Vec<String> = self.core.server_ids.iter().filter_map(|&s| self.servers.documents(s)).flat_map(|d| d.uris().filter_map(diagnose::uri_path).collect::<Vec<_>>()).collect();
                self.core.validators.clear();
                self.core.pending.extend(paths.into_iter().map(|p| (p, false)));
            }
            _ => {}
        }
        self.recheck();
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        if !self.core.server_ids.contains(&server) {
            self.core.server_ids.push(server);
        }
        self.servers.handle(server, message, &mut Checker(&mut self.core));
        self.recheck();
    }

    fn scratch_closed(&mut self, scratch: u64) {
        if self.core.query_scratch == Some(scratch) {
            self.core.query_scratch = None;
        }
    }
}

export!(DataTools);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(stem(Some("/p/config.prod.json")), "config");
        assert_eq!(stem(Some("C:\\p\\.eslintrc.json")), "eslintrc");
        assert_eq!(stem(None), "data");
    }
}
