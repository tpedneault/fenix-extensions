//! Tcl's language server and the project's tags: what `K`, completion and
//! `gd` answer in a Tcl file, the ctags scans behind them, `SPC m s` and
//! `SPC m T`. It decides; what it asks of Fenix goes out as [`Effect`]s,
//! which `lib.rs` carries out, so all of it runs in a plain test.

use std::collections::HashMap;

use fenix_extension_api::lsp::types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, CompletionTextEdit, CompletionTriggerKind, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, Location, MarkupContent, MarkupKind, OneOf, Position, Range, ServerCapabilities, SymbolInformation, SymbolKind, TextEdit, Uri,
};
use fenix_extension_api::lsp::{char_to_utf16, utf16_to_char, Documents, LanguageServer};

use crate::tags::{self, Tag};
use crate::tcl;

/// What it asks Fenix to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run ctags over `root`, as scan `scan`: `Tcl::started` with its
    /// process, or `Tcl::failed`.
    Scan { scan: u64, root: String },
    /// Said in the message line.
    Notify(String),
    /// Said as an error.
    Error(String),
    /// Kept on its output page.
    Log(String),
    /// The symbol picker: `(label, detail, value)` rows.
    Pick { title: String, items: Vec<(String, String, String)> },
    /// Open `file` (a full path) at `line`, from 1; `root` is the project
    /// it's in.
    Open { root: String, file: String, line: u32 },
}

/// What ctags is asked to scan, while it runs.
#[derive(Debug, Default)]
struct Scan {
    root: String,
    /// A manual `SPC m T`, which says what it found.
    report: bool,
    process: Option<u64>,
    out: String,
    err: String,
}

/// Said when there's no project to scan.
pub const NO_ROOT: &str = "no project root detected for the focused buffer -- open a file inside a recognized project (a `.git`/`Cargo.toml`/etc. marker somewhere above it) first";

/// Tcl: the tags of each project it's seen, and the scans running.
#[derive(Debug, Default)]
pub struct Tcl {
    /// Each project's definitions, by its root (`tags::normalize`d). A
    /// root is here once its first scan has landed.
    tags: HashMap<String, Vec<Tag>>,
    scans: HashMap<u64, Scan>,
    next_scan: u64,
    /// Each language server's project, by server.
    roots: HashMap<u64, String>,
    /// `SPC m s` waiting for this root's first scan.
    pick_after: Option<String>,
    /// The rows of the picker open now, and their project.
    picking: Option<(String, Vec<Tag>)>,
    /// What it asks of Fenix, for `lib.rs` to carry out.
    pub effects: Vec<Effect>,
}

impl Tcl {
    /// `root`'s definitions, as far as they're known: none until its
    /// first scan lands, and a scan started if none has.
    pub fn tags(&mut self, root: &str) -> &[Tag] {
        if !self.tags.contains_key(root) {
            self.scan(root, false);
        }
        self.tags.get(root).map(Vec::as_slice).unwrap_or_default()
    }

    /// Starts scanning `root`, unless a scan of it is running already --
    /// a manual refresh (`report`) starts over, so it sees files saved
    /// since, and the older scan's answer is dropped.
    fn scan(&mut self, root: &str, report: bool) {
        let running: Vec<u64> = self.scans.iter().filter(|(_, s)| s.root == root).map(|(id, _)| *id).collect();
        if !running.is_empty() && !report {
            return;
        }
        for id in running {
            self.scans.remove(&id);
        }
        self.next_scan += 1;
        let scan = self.next_scan;
        self.scans.insert(scan, Scan { root: root.to_string(), report, ..Scan::default() });
        if report {
            self.effects.push(Effect::Notify(format!("scanning {root} for Tcl definitions...")));
        }
        self.effects.push(Effect::Scan { scan, root: root.to_string() });
    }

    /// Scan `scan`'s ctags is running as `process`.
    pub fn started(&mut self, scan: u64, process: u64) {
        if let Some(s) = self.scans.get_mut(&scan) {
            s.process = Some(process);
        }
    }

    fn scan_of(&mut self, process: u64) -> Option<u64> {
        self.scans.iter().find(|(_, s)| s.process == Some(process)).map(|(id, _)| *id)
    }

    /// A line process `process` wrote.
    pub fn output(&mut self, process: u64, stderr: bool, line: &str) {
        let Some(scan) = self.scan_of(process) else { return };
        let s = self.scans.get_mut(&scan).expect("just found");
        let out = if stderr { &mut s.err } else { &mut s.out };
        out.push_str(line);
        out.push('\n');
    }

    /// Process `process` ended.
    pub fn exited(&mut self, process: u64, code: Option<i32>) {
        let Some(scan) = self.scan_of(process) else { return };
        if code == Some(0) {
            let out = std::mem::take(&mut self.scans.get_mut(&scan).expect("just found").out);
            let tags = tags::parse(&out);
            // Real tag lines, none of them read: another ctags than the
            // one this reads.
            // Said, when the scan was asked for: "0 definitions" alone
            // reads as a project with none.
            if tags.is_empty() && out.lines().any(|l| !l.trim().is_empty() && !l.starts_with("!_")) {
                let why = "ctags gave tag lines, but none read as a Tcl proc or namespace -- this reads Universal Ctags' plain `-f -` output".to_string();
                let s = &self.scans[&scan];
                let asked = s.report || self.pick_after.as_deref() == Some(s.root.as_str());
                self.effects.push(if asked { Effect::Error(why) } else { Effect::Log(why) });
            }
            self.scanned(scan, tags);
        } else {
            let s = &self.scans[&scan];
            let err = s.err.trim();
            let code = code.map_or("no exit code".to_string(), |c| format!("exit code {c}"));
            let why = format!("ctags ended with {code} scanning {} for Tcl definitions{}", s.root, if err.is_empty() { String::new() } else { format!(" -- {err}") });
            self.failed(scan, &why);
        }
    }

    /// Scan `scan` found `tags`: they're its project's now, and a picker
    /// waiting for them opens.
    pub fn scanned(&mut self, scan: u64, tags: Vec<Tag>) {
        let Some(s) = self.scans.remove(&scan) else { return };
        if s.report {
            self.effects.push(Effect::Notify(format!("refreshed Tcl completion tags for {} -- {} definition(s) found", s.root, tags.len())));
        }
        self.tags.insert(s.root.clone(), tags);
        if self.pick_after.as_deref() == Some(s.root.as_str()) {
            self.pick_after = None;
            self.symbols(Some(s.root));
        }
    }

    /// Scan `scan` couldn't run, or failed: said when it was asked for,
    /// logged when not; its project keeps what it had (nothing, the first
    /// time), so it isn't tried again on every key.
    pub fn failed(&mut self, scan: u64, why: &str) {
        let Some(s) = self.scans.remove(&scan) else { return };
        self.effects.push(if s.report || self.pick_after.as_deref() == Some(s.root.as_str()) { Effect::Error(why.to_string()) } else { Effect::Log(why.to_string()) });
        self.tags.entry(s.root.clone()).or_default();
        if self.pick_after.as_deref() == Some(s.root.as_str()) {
            self.pick_after = None;
        }
    }

    /// `SPC m T`: the project's tags scanned again now, and how many it
    /// found said once they're in. What it had keeps serving meanwhile.
    pub fn refresh(&mut self, root: Option<String>) {
        match root {
            Some(root) => self.scan(&root, true),
            None => self.effects.push(Effect::Error(NO_ROOT.into())),
        }
    }

    /// `SPC m s`: every definition in the project, by its fully-qualified
    /// name, to jump to -- once its first scan is in.
    pub fn symbols(&mut self, root: Option<String>) {
        let Some(root) = root else { return self.effects.push(Effect::Error(NO_ROOT.into())) };
        let Some(tags) = self.tags.get(&root).cloned() else {
            self.pick_after = Some(root.clone());
            self.effects.push(Effect::Notify(format!("scanning {root} for Tcl definitions...")));
            return self.scan(&root, false);
        };
        if tags.is_empty() {
            return self.effects.push(Effect::Notify(format!("no Tcl definitions found in {root} -- SPC m T scans it again")));
        }
        let items = tags
            .iter()
            .enumerate()
            .map(|(i, tag)| {
                let file = tags::relative_to(&tag.file, &root).unwrap_or(&tag.file);
                (tag.name.clone(), format!("{file}:{}", tag.line), i.to_string())
            })
            .collect();
        self.effects.push(Effect::Pick { title: "Tcl definitions".into(), items });
        self.picking = Some((root, tags));
    }

    /// The symbol picker closed: on what was picked, if anything.
    pub fn picked(&mut self, value: Option<&str>) {
        let Some((root, tags)) = self.picking.take() else { return };
        let Some(tag) = value.and_then(|v| v.parse::<usize>().ok()).and_then(|i| tags.get(i)) else { return };
        self.effects.push(Effect::Open { root, file: tag.file.clone(), line: tag.line.max(1) });
    }

    /// The project of language server `server`, when there's one.
    pub fn root_of(&self, server: u64) -> Option<&str> {
        self.roots.get(&server).map(String::as_str)
    }

    /// The one project its servers serve, when there's just one.
    pub fn only_root(&self) -> Option<String> {
        let mut roots: Vec<&String> = self.roots.values().collect();
        roots.dedup();
        (roots.len() == 1).then(|| roots[0].clone())
    }

    /// The `-flag`s, subcommands or words that complete at the end of
    /// `before` (a line up to the cursor) in project `root`: the char
    /// column the completion starts at, and what goes there. Nothing in a
    /// comment or a string, and nothing for a character typed that
    /// completes nothing (`puts ` has no subcommands, `expr {$a -` no
    /// options); `invoked` (Ctrl-Space) offers every word on an empty
    /// prefix.
    pub fn complete(&mut self, before: &str, invoked: bool, root: Option<&str>) -> Option<(usize, Vec<CompletionItem>)> {
        if in_comment_or_string(before) {
            return None;
        }
        let item = |label: &str, detail: &str, kind| CompletionItem { label: label.to_string(), detail: (!detail.is_empty()).then(|| detail.to_string()), kind: Some(kind), ..CompletionItem::default() };
        if let Some((dash, flag)) = tcl::option_prefix(before) {
            let words = tcl::command_words_before(&before[..before.len() - flag.len()])?;
            let options = tcl::options_at(&words);
            return (!options.is_empty()).then(|| (dash, options.into_iter().map(|(name, usage)| item(name, usage, CompletionItemKind::PROPERTY)).collect()));
        }
        let chars: Vec<char> = before.chars().collect();
        let start = chars.iter().rposition(|c| !(c.is_alphanumeric() || *c == '_' || *c == ':')).map_or(0, |i| i + 1);
        let leading: String = chars[..start].iter().collect();
        if let Some(words) = tcl::command_words_before(&leading).filter(|w| !w.is_empty()) {
            let subcommands = tcl::subcommands(&words.join(" "));
            if !subcommands.is_empty() {
                return Some((start, subcommands.into_iter().map(|(name, usage)| item(name, usage, CompletionItemKind::KEYWORD)).collect()));
            }
        }
        if start == chars.len() && !invoked {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let mut items: Vec<CompletionItem> = tcl::KEYWORDS.iter().filter(|k| seen.insert(k.to_string())).map(|k| item(k, tcl::signature(k).unwrap_or_default(), CompletionItemKind::KEYWORD)).collect();
        if let Some(root) = root {
            for tag in self.tags(root).to_vec() {
                if seen.insert(tag.name.clone()) {
                    items.push(item(&tag.name, "", CompletionItemKind::FUNCTION));
                }
            }
        }
        Some((start, items))
    }

    /// Where `word` (what `gd` is on) is defined: procs in `text` (the
    /// file it's in, at `uri`) first, then the project's tags.
    pub fn definitions(&mut self, word: &str, text: &str, uri: &str, root: Option<&str>) -> Vec<(String, u32)> {
        let name = word.strip_prefix("::").unwrap_or(word);
        let mut found: Vec<(String, u32)> = proc_lines(text).into_iter().filter(|(n, _)| n.strip_prefix("::").unwrap_or(n) == name).map(|(_, line)| (uri.to_string(), line)).collect();
        if found.is_empty() {
            if let Some(root) = root {
                let tags = self.tags(root);
                let exact: Vec<&Tag> = tags.iter().filter(|t| t.name == name).collect();
                // A call from inside its namespace names it unqualified.
                let near: Vec<&Tag> = if exact.is_empty() { tags.iter().filter(|t| t.name.ends_with(&format!("::{name}"))).collect() } else { exact };
                found = near.into_iter().map(|t| (tags::path_to_uri(&t.file), t.line.saturating_sub(1))).collect();
            }
        }
        found
    }
}

/// Whether the end of `before` (a line up to the cursor) is in a comment
/// or a string: a `#` starting the command, or an unclosed `"`.
pub fn in_comment_or_string(before: &str) -> bool {
    let command = before.rsplit([';', '[', '{']).next().unwrap_or(before);
    if before.trim_start().starts_with('#') || command.trim_start().starts_with('#') {
        return true;
    }
    let mut quotes = 0;
    let mut escaped = false;
    for c in before.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => {
                quotes += 1;
                escaped = false;
            }
            _ => escaped = false,
        }
    }
    quotes % 2 == 1
}

/// Every `proc` defined in `text`, with its line (from 0).
pub fn proc_lines(text: &str) -> Vec<(String, u32)> {
    text.lines().enumerate().flat_map(|(i, line)| tcl::procs_defined_in(line).into_iter().map(move |name| (name, i as u32))).collect()
}

/// The Tcl word at char `col` of `line` (a name: letters, digits, `_`,
/// `:`), as `(start, end)`.
fn name_at(line: &str, col: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let name = |c: char| c.is_alphanumeric() || c == '_' || c == ':';
    let col = if chars.get(col).copied().is_some_and(name) { col } else { col.checked_sub(1).filter(|&c| chars.get(c).copied().is_some_and(name))? };
    let start = (0..col).rev().take_while(|&i| name(chars[i])).last().unwrap_or(col);
    let end = (col..chars.len()).take_while(|&i| name(chars[i])).last().map_or(col, |i| i + 1);
    Some((start, end))
}

/// What `K` shows on char `col` of `line`: Tcl's own usage line for the
/// built-in the cursor is on, or is an argument of, and an ensemble's
/// subcommands. `None` on a user proc, a variable, whitespace.
pub fn hover_at(line: &str, col: usize) -> Option<(usize, usize, String)> {
    let (start, end) = tcl::word_at(line, col)?;
    let chars: Vec<char> = line.chars().collect();
    let before: String = chars[..start].iter().collect();
    let word: String = chars[start..end].iter().collect();
    let words = tcl::command_words_before(&before)?;
    tcl::hover_text(&words, &word).map(|text| (start, end, text))
}

fn range(line: u32, text: &str, start: usize, end: usize) -> Range {
    Range { start: Position { line, character: char_to_utf16(text, start) }, end: Position { line, character: char_to_utf16(text, end) } }
}

impl LanguageServer for Tcl {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            // `-` starts an option, a space may come before a subcommand.
            completion_provider: Some(CompletionOptions { trigger_characters: Some(vec!["-".into(), " ".into()]), ..CompletionOptions::default() }),
            definition_provider: Some(OneOf::Left(true)),
            // The project's procs and namespaces, which Fenix colors calls
            // by: a call to a proc ctags found reads as one.
            workspace_symbol_provider: Some(OneOf::Left(true)),
            ..ServerCapabilities::default()
        }
    }

    fn initialized(&mut self, server: u64, root: Option<&str>) {
        let Some(root) = root.and_then(tags::uri_to_path) else { return };
        self.roots.insert(server, root.clone());
        self.tags(&root);
    }

    fn hover(&mut self, _server: u64, docs: &Documents, params: HoverParams) -> Option<Hover> {
        let at = params.text_document_position_params;
        let line = docs.line(&at.text_document.uri, at.position.line)?;
        let (start, end, text) = hover_at(line, utf16_to_char(line, at.position.character))?;
        Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::PlainText, value: text }), range: Some(range(at.position.line, line, start, end)) })
    }

    fn completion(&mut self, server: u64, docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        let at = params.text_document_position;
        let line = docs.text(&at.text_document.uri).map(|t| t.lines().nth(at.position.line as usize).unwrap_or_default())?;
        let col = utf16_to_char(line, at.position.character);
        let before: String = line.chars().take(col).collect();
        let invoked = params.context.as_ref().is_none_or(|c| c.trigger_kind != CompletionTriggerKind::TRIGGER_CHARACTER);
        let root = self.root_of(server).map(str::to_string);
        let (start, items) = self.complete(&before, invoked, root.as_deref())?;
        let replace = range(at.position.line, line, start, col);
        let items = items
            .into_iter()
            .map(|item| CompletionItem { text_edit: Some(CompletionTextEdit::Edit(TextEdit { range: replace, new_text: item.label.clone() })), filter_text: Some(item.label.clone()), ..item })
            .collect();
        Some(CompletionResponse::Array(items))
    }

    fn definition(&mut self, server: u64, docs: &Documents, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let at = params.text_document_position_params;
        let uri = at.text_document.uri;
        let text = docs.text(&uri)?;
        let line = text.lines().nth(at.position.line as usize)?;
        let (start, end) = name_at(line, utf16_to_char(line, at.position.character))?;
        let word: String = line.chars().skip(start).take(end - start).collect();
        let root = self.root_of(server).map(str::to_string);
        let found = self.definitions(&word, text, uri.as_str(), root.as_deref());
        let locations: Vec<Location> = found
            .into_iter()
            .filter_map(|(uri, line)| Some(Location { uri: uri.parse::<Uri>().ok()?, range: Range { start: Position { line, character: 0 }, end: Position { line, character: 0 } } }))
            .collect();
        (!locations.is_empty()).then_some(GotoDefinitionResponse::Array(locations))
    }

    fn other(&mut self, server: u64, _docs: &Documents, method: &str, params: serde_json::Value) -> Option<Result<serde_json::Value, String>> {
        if method != "workspace/symbol" {
            return None;
        }
        let query = params.get("query").and_then(serde_json::Value::as_str).unwrap_or("").to_lowercase();
        let root = self.root_of(server).map(str::to_string)?;
        let symbols = self.workspace_symbols(&root, &query);
        Some(serde_json::to_value(symbols).map_err(|e| e.to_string()))
    }
}

impl Tcl {
    /// `workspace/symbol`: the project's procs and namespaces whose names
    /// hold `query` (lowercase; empty, every one), where ctags found them.
    pub fn workspace_symbols(&mut self, root: &str, query: &str) -> Vec<SymbolInformation> {
        self.tags(root)
            .iter()
            .filter(|t| query.is_empty() || t.name.to_lowercase().contains(query))
            .filter_map(|t| {
                let uri = tags::path_to_uri(&t.file).parse::<Uri>().ok()?;
                let line = t.line.saturating_sub(1);
                let at = Range { start: Position { line, character: 0 }, end: Position { line, character: 0 } };
                #[allow(deprecated)]
                Some(SymbolInformation {
                    name: t.name.clone(),
                    kind: if t.namespace { SymbolKind::NAMESPACE } else { SymbolKind::FUNCTION },
                    tags: None,
                    deprecated: None,
                    location: Location { uri, range: at },
                    container_name: None,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::lsp::types::{CompletionContext, TextDocumentIdentifier, TextDocumentPositionParams};
    use fenix_extension_api::lsp::Servers;

    fn tag(name: &str, file: &str, line: u32) -> Tag {
        Tag { name: name.into(), file: file.into(), line, namespace: false }
    }

    /// A Tcl whose project `/p` has been scanned and found `tags`.
    fn scanned(tags: Vec<Tag>) -> Tcl {
        let mut tcl = Tcl::default();
        tcl.refresh(Some("/p".into()));
        tcl.started(1, 7);
        tcl.scanned(1, tags);
        tcl.effects.clear();
        tcl
    }

    /// Fenix's documents, with `text` open at `file:///p/a.tcl`.
    fn opened(text: &str) -> Servers {
        let mut servers = Servers::default();
        let open = serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": "file:///p/a.tcl", "languageId": "tcl", "version": 1, "text": text } } });
        servers.handle(1, &open.to_string(), &mut Tcl::default());
        servers
    }

    fn at(line: u32, character: u32) -> TextDocumentPositionParams {
        TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri: "file:///p/a.tcl".parse().unwrap() }, position: Position { line, character } }
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn it_says_what_it_answers() {
        let caps = Tcl::default().capabilities();
        assert!(caps.hover_provider.is_some() && caps.definition_provider.is_some());
        assert_eq!(caps.completion_provider.unwrap().trigger_characters.unwrap(), ["-", " "]);
        assert!(caps.workspace_symbol_provider.is_some());
    }

    /// `workspace/symbol` gives the project's procs as functions and its
    /// namespaces as namespaces -- Fenix colors calls to the procs.
    #[test]
    fn workspace_symbols_are_the_projects_procs_and_namespaces() {
        let mut tcl = scanned(vec![tag("myns::greet", "/p/lib.tcl", 4), Tag { name: "myns".into(), file: "/p/lib.tcl".into(), line: 1, namespace: true }, tag("setup", "/p/main.tcl", 2)]);
        let all = tcl.workspace_symbols("/p", "");
        let named: Vec<(&str, SymbolKind, u32)> = all.iter().map(|s| (s.name.as_str(), s.kind, s.location.range.start.line)).collect();
        assert_eq!(named, [("myns::greet", SymbolKind::FUNCTION, 3), ("myns", SymbolKind::NAMESPACE, 0), ("setup", SymbolKind::FUNCTION, 1)]);
        assert_eq!(all[0].location.uri.as_str(), "file:///p/lib.tcl");
        let greet: Vec<String> = tcl.workspace_symbols("/p", "gre").into_iter().map(|s| s.name).collect();
        assert_eq!(greet, ["myns::greet"]);
        // As the language server's request, for the project it serves.
        tcl.initialized(1, Some("file:///p"));
        let servers = opened("setup\n");
        let docs = servers.documents(1).unwrap();
        let answer = tcl.other(1, docs, "workspace/symbol", serde_json::json!({ "query": "setup" })).unwrap().unwrap();
        assert_eq!(answer[0]["name"], "setup");
        assert_eq!(answer[0]["kind"], 12, "a function");
        assert!(tcl.other(1, docs, "textDocument/rename", serde_json::json!({})).is_none(), "nothing else is answered here");
    }

    #[test]
    fn k_shows_a_builtins_usage_line_even_on_one_of_its_arguments() {
        let (start, end, text) = hover_at("string compare $a $b", 2).unwrap();
        assert_eq!((start, end), (0, 6));
        assert!(text.starts_with("string subcommand ?arg ...?") && text.contains("subcommands: bytelength"), "{text}");
        assert_eq!(hover_at("string compare $a $b", 9).unwrap().2, "string compare ?-nocase? ?-length int? string1 string2");
        assert!(hover_at("my_proc 1 2", 2).is_none(), "a user proc");
        assert!(hover_at("string compare $a $b", 6).is_none(), "whitespace");
    }

    #[test]
    fn hover_comes_through_the_language_server() {
        let servers = opened("set x 1\nlappend x 2\n");
        let hover = Tcl::default().hover(1, servers.documents(1).unwrap(), HoverParams { text_document_position_params: at(1, 3), work_done_progress_params: Default::default() }).unwrap();
        let HoverContents::Markup(text) = hover.contents else { panic!("plain text") };
        assert_eq!(text.value, "lappend varName ?value ...?");
        assert_eq!(hover.range.unwrap().end.character, 7);
    }

    #[test]
    fn a_dash_completes_the_commands_options() {
        let mut tcl = Tcl::default();
        let (start, items) = tcl.complete("lsort -", false, None).expect("lsort takes options");
        assert_eq!(start, 6);
        assert!(labels(&items).contains(&"-unique"));
        assert!(items.iter().all(|i| i.label.starts_with('-')));
        let (start, items) = tcl.complete("string compare -nocase -le", false, None).unwrap();
        assert_eq!((start, labels(&items)), (23, vec!["-length", "-nocase"]));
        assert!(tcl.complete("expr {$a -", false, None).is_none(), "an arithmetic minus opens nothing");
        assert!(tcl.complete("set x -", false, None).is_none());
    }

    #[test]
    fn after_an_ensemble_only_its_subcommands_complete() {
        let mut tcl = Tcl::default();
        let (start, items) = tcl.complete("string ", false, None).unwrap();
        assert_eq!(start, 7);
        assert!(labels(&items).contains(&"compare") && !labels(&items).contains(&"puts"), "{:?}", labels(&items));
        assert_eq!(items.iter().find(|i| i.label == "compare").unwrap().detail.as_deref(), Some("string compare ?-nocase? ?-length int? string1 string2"));
        let (start, items) = tcl.complete("set x [string co", false, None).unwrap();
        assert_eq!(start, 14);
        assert!(labels(&items).contains(&"compare"));
        let (_, items) = tcl.complete("binary encode ", false, None).unwrap();
        assert_eq!(labels(&items), ["base64", "hex", "uuencode"]);
        assert!(tcl.complete("puts ", false, None).is_none(), "puts has no subcommands");
    }

    #[test]
    fn a_word_completes_from_the_keywords_and_the_projects_tags() {
        let mut tcl = scanned(vec![tag("myns::greet", "/p/lib.tcl", 2), tag("set", "/p/lib.tcl", 9)]);
        let (start, items) = tcl.complete("    se", false, Some("/p")).unwrap();
        assert_eq!(start, 4);
        let found = labels(&items);
        assert!(found.contains(&"puts") && found.contains(&"myns::greet"));
        assert_eq!(found.iter().filter(|l| **l == "set").count(), 1, "a tag named like a keyword isn't there twice");
        assert_eq!(items.iter().find(|i| i.label == "lappend").unwrap().detail.as_deref(), Some("lappend varName ?value ...?"), "a keyword's usage line");
        let (start, _) = tcl.complete("set x [myns::gr", false, Some("/p")).unwrap();
        assert_eq!(start, 7, "a qualified name is one word");
        assert!(tcl.complete("", false, Some("/p")).is_none(), "nothing typed");
        assert!(tcl.complete("", true, Some("/p")).is_some(), "Ctrl-Space");
    }

    #[test]
    fn nothing_completes_in_a_comment_or_a_string() {
        let mut tcl = Tcl::default();
        assert!(tcl.complete("# se", false, None).is_none());
        assert!(tcl.complete("set a 1 ;# se", false, None).is_none());
        assert!(tcl.complete("puts \"se", false, None).is_none());
        assert!(tcl.complete("puts \"a\" ; se", false, None).is_some(), "the string is closed");
        assert!(tcl.complete("puts \"a \\\" se", false, None).is_none(), "an escaped quote doesn't close it");
    }

    #[test]
    fn completion_through_the_language_server_replaces_from_the_dash() {
        let servers = opened("lsort -un\n");
        let params = CompletionParams { text_document_position: at(0, 9), work_done_progress_params: Default::default(), partial_result_params: Default::default(), context: None };
        let Some(CompletionResponse::Array(items)) = Tcl::default().completion(1, servers.documents(1).unwrap(), params) else { panic!("options") };
        let unique = items.iter().find(|i| i.label == "-unique").unwrap();
        let Some(CompletionTextEdit::Edit(edit)) = &unique.text_edit else { panic!("an edit") };
        assert_eq!((edit.range.start.character, edit.range.end.character, edit.new_text.as_str()), (6, 9, "-unique"));
        // A space typed after a command with nothing to offer.
        let servers = opened("puts \n");
        let trigger = Some(CompletionContext { trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER, trigger_character: Some(" ".into()) });
        let params = CompletionParams { text_document_position: at(0, 5), work_done_progress_params: Default::default(), partial_result_params: Default::default(), context: trigger };
        assert!(Tcl::default().completion(1, servers.documents(1).unwrap(), params).is_none());
    }

    #[test]
    fn a_scan_starts_with_the_server_and_its_tags_land() {
        let mut tcl = Tcl::default();
        tcl.initialized(3, Some("file:///C:/work/proj"));
        assert_eq!(tcl.effects, [Effect::Scan { scan: 1, root: "C:/work/proj".into() }]);
        assert_eq!(tcl.root_of(3), Some("C:/work/proj"));
        tcl.effects.clear();
        assert!(tcl.tags("C:/work/proj").is_empty());
        assert!(tcl.effects.is_empty(), "not again while it runs");
        tcl.started(1, 42);
        tcl.output(42, false, "landed_proc\tC:/work/proj/lib.tcl\t/^proc landed_proc {} {$/;\"\tp\tline:1");
        tcl.output(42, true, "ctags: Warning: something");
        tcl.exited(42, Some(0));
        assert_eq!(tcl.tags("C:/work/proj"), [tag("landed_proc", "C:/work/proj/lib.tcl", 1)]);
        assert!(tcl.effects.is_empty(), "a scan nobody asked for lands quietly");
        assert_eq!(tcl.only_root().as_deref(), Some("C:/work/proj"));
    }

    #[test]
    fn a_manual_refresh_starts_over_and_says_what_it_found() {
        let mut tcl = Tcl::default();
        tcl.tags("/p");
        tcl.started(1, 10);
        tcl.refresh(Some("/p".into()));
        assert_eq!(tcl.effects[1..], [Effect::Notify("scanning /p for Tcl definitions...".into()), Effect::Scan { scan: 2, root: "/p".into() }]);
        tcl.started(2, 11);
        tcl.effects.clear();
        tcl.output(10, false, "stale_proc\t/p/a.tcl\t/^proc stale_proc {} {$/;\"\tp\tline:1");
        tcl.exited(10, Some(0));
        assert!(!tcl.tags.contains_key("/p"), "the superseded scan is dropped");
        tcl.output(11, false, "fresh_proc\t/p/a.tcl\t/^proc fresh_proc {} {$/;\"\tp\tline:1");
        tcl.exited(11, Some(0));
        assert_eq!(tcl.effects, [Effect::Notify("refreshed Tcl completion tags for /p -- 1 definition(s) found".into())]);
        assert_eq!(tcl.tags("/p")[0].name, "fresh_proc");
        tcl.effects.clear();
        tcl.refresh(None);
        assert_eq!(tcl.effects, [Effect::Error(NO_ROOT.into())]);
    }

    #[test]
    fn tag_lines_none_of_which_read_are_an_error_when_the_scan_was_asked_for() {
        let mut tcl = Tcl::default();
        tcl.refresh(Some("/p".into()));
        tcl.started(1, 7);
        tcl.effects.clear();
        tcl.output(7, false, "greet\t/p/a.tcl\t/^proc greet {} {$/;\"\tfunction\tline:1");
        tcl.exited(7, Some(0));
        assert!(matches!(tcl.effects.first(), Some(Effect::Error(why)) if why.contains("none read")), "{:?}", tcl.effects);
    }

    #[test]
    fn a_scan_that_fails_is_said_when_asked_for_and_not_tried_on_every_key() {
        let mut tcl = Tcl::default();
        tcl.tags("/p");
        tcl.failed(1, "ctags not found");
        assert_eq!(tcl.effects[1..], [Effect::Log("ctags not found".into())]);
        tcl.effects.clear();
        assert!(tcl.tags("/p").is_empty());
        assert!(tcl.effects.is_empty(), "known to have nothing");
        tcl.refresh(Some("/p".into()));
        tcl.started(2, 5);
        tcl.output(5, true, "ctags: no Tcl parser");
        tcl.exited(5, Some(1));
        assert_eq!(tcl.effects.last(), Some(&Effect::Error("ctags ended with exit code 1 scanning /p for Tcl definitions -- ctags: no Tcl parser".into())));
    }

    #[test]
    fn spc_m_s_picks_a_definition_by_its_qualified_name_and_opens_it() {
        let mut tcl = scanned(vec![tag("myns::greet", "/p/lib/a.tcl", 3), tag("top", "/elsewhere/b.tcl", 0)]);
        tcl.symbols(Some("/p".into()));
        let Some(Effect::Pick { items, .. }) = tcl.effects.pop() else { panic!("a picker") };
        assert_eq!(items, [("myns::greet".into(), "lib/a.tcl:3".into(), "0".into()), ("top".into(), "/elsewhere/b.tcl:0".into(), "1".into())]);
        tcl.picked(Some("0"));
        assert_eq!(tcl.effects, [Effect::Open { root: "/p".into(), file: "/p/lib/a.tcl".into(), line: 3 }]);
        tcl.effects.clear();
        tcl.picked(Some("1"));
        assert!(tcl.effects.is_empty(), "the picker's gone");
    }

    #[test]
    fn spc_m_s_before_the_first_scan_waits_for_it() {
        let mut tcl = Tcl::default();
        tcl.symbols(Some("/p".into()));
        assert_eq!(tcl.effects, [Effect::Notify("scanning /p for Tcl definitions...".into()), Effect::Scan { scan: 1, root: "/p".into() }]);
        tcl.effects.clear();
        tcl.scanned(1, vec![tag("landed", "/p/a.tcl", 1)]);
        assert!(matches!(tcl.effects.as_slice(), [Effect::Pick { .. }]), "{:?}", tcl.effects);
        tcl.effects.clear();
        tcl.symbols(None);
        assert_eq!(tcl.effects, [Effect::Error(NO_ROOT.into())]);
    }

    #[test]
    fn gd_finds_a_proc_in_the_file_then_in_the_project() {
        let mut tcl = scanned(vec![tag("myns::greet", "/p/lib.tcl", 3), tag("other", "/p/lib.tcl", 9)]);
        let text = "proc local {} {}\nlocal\nmyns::greet\ngreet\n::other\n";
        assert_eq!(tcl.definitions("local", text, "file:///p/a.tcl", Some("/p")), [("file:///p/a.tcl".to_string(), 0)]);
        assert_eq!(tcl.definitions("myns::greet", text, "file:///p/a.tcl", Some("/p")), [("file:///p/lib.tcl".to_string(), 2)]);
        assert_eq!(tcl.definitions("greet", text, "file:///p/a.tcl", Some("/p")), [("file:///p/lib.tcl".to_string(), 2)], "called from inside its namespace");
        assert_eq!(tcl.definitions("::other", text, "file:///p/a.tcl", Some("/p")), [("file:///p/lib.tcl".to_string(), 8)]);
        assert!(tcl.definitions("nope", text, "file:///p/a.tcl", Some("/p")).is_empty());
        assert_eq!(name_at("set x [myns::greet 1]", 10), Some((7, 18)));
        assert_eq!(name_at("greet", 5), Some((0, 5)), "just after it");
    }

    #[test]
    fn procs_are_found_with_their_lines() {
        assert_eq!(proc_lines("set x 1\nproc a {} {}\nnamespace eval n { proc b {} {} }\n"), [("a".to_string(), 1), ("b".to_string(), 2)]);
    }
}
