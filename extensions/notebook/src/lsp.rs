//! The notebook as a language server for Markdown and Mermaid: Fenix
//! sends it the files you edit as you type, so it writes a notebook file
//! a second after you stop (keeping the version before each editing
//! session -- there's no `:w` to forget for a file you never chose a
//! place for); `[[` completes a note's name; `gd` on a link follows it;
//! `K` on one says where it goes.

use std::time::{Duration, Instant};

use fenix_extension_api::host;
use fenix_extension_api::lsp::types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, CompletionTextEdit, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability, Location, MarkupContent,
    MarkupKind, OneOf, Position, Range, ServerCapabilities, TextEdit, Uri,
};
use fenix_extension_api::lsp::{char_to_utf16, utf16_to_char, Documents, LanguageServer, Servers};

use crate::book::{Kind, Target};
use crate::{exports, links, meta, paths, NotebookExt};

/// How long typing has to pause before a notebook file is written.
pub(crate) const AUTOSAVE_AFTER: Duration = Duration::from_millis(1000);

struct Lsp<'a> {
    ext: &'a mut NotebookExt,
    /// The message is a document's change (not its opening).
    changing: bool,
}

/// What's typed after the `[[` before the cursor, and where it starts
/// (chars), when the cursor is in a link being typed.
pub(crate) fn link_prefix(before: &str) -> Option<(usize, String)> {
    let open = before.rfind("[[")?;
    let typed = &before[open + 2..];
    if typed.contains(']') || typed.contains('|') {
        return None;
    }
    Some((before[..open + 2].chars().count(), typed.to_string()))
}

impl Lsp<'_> {
    /// The link under a position of a document, and the file it's in.
    fn link_at(&self, docs: &Documents, uri: &Uri, position: Position) -> Option<(meta::Link, String)> {
        let line = docs.line(uri, position.line)?;
        let col = utf16_to_char(line, position.character);
        let link = meta::links_in_line(line, position.line as usize).into_iter().find(|l| l.cols.contains(&col))?;
        Some((link, paths::uri_path(uri.as_str())))
    }
}

impl LanguageServer for Lsp<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            completion_provider: Some(CompletionOptions { trigger_characters: Some(vec!["[".into()]), ..Default::default() }),
            definition_provider: Some(OneOf::Left(true)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            ..Default::default()
        }
    }

    fn initialized(&mut self, _server: u64, root: Option<&str>) {
        if let Some(root) = root {
            self.ext.note_project(&paths::uri_path(root));
        }
    }

    fn changed(&mut self, _server: u64, _docs: &Documents, uri: &Uri) {
        if !self.changing {
            return;
        }
        let path = paths::uri_path(uri.as_str());
        if self.ext.places.guest(&path).is_some_and(|g| g.starts_with(paths::ROOT)) {
            self.ext.dirty.insert(uri.as_str().to_string(), Instant::now());
            if self.ext.autosave.is_none() {
                self.ext.autosave = Some(host::set_timer(AUTOSAVE_AFTER.as_millis() as u32, false));
            }
        }
    }

    fn completion(&mut self, _server: u64, docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        let at = &params.text_document_position;
        if !at.text_document.uri.as_str().to_lowercase().ends_with(".md") {
            return None;
        }
        let line = docs.line(&at.text_document.uri, at.position.line)?;
        let col = utf16_to_char(line, at.position.character);
        let before: String = line.chars().take(col).collect();
        let (start, _) = link_prefix(&before)?;
        let closed = line.chars().skip(col).collect::<String>().starts_with("]]");
        let nb = self.ext.notebook()?;
        let range = Range { start: Position { line: at.position.line, character: char_to_utf16(line, start) }, end: at.position };
        let items = nb
            .recent()
            .into_iter()
            .map(|e| {
                let insert = if closed { e.name.clone() } else { format!("{}]]", e.name) };
                CompletionItem {
                    label: e.display_name(),
                    kind: Some(if e.kind == Kind::Diagram { CompletionItemKind::FILE } else { CompletionItemKind::REFERENCE }),
                    detail: Some(e.kind.tag().to_string()),
                    filter_text: Some(e.name.clone()),
                    insert_text: Some(insert.clone()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text: insert })),
                    ..Default::default()
                }
            })
            .collect();
        Some(CompletionResponse::Array(items))
    }

    fn definition(&mut self, _server: u64, docs: &Documents, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let at = &params.text_document_position_params;
        let (link, from) = self.link_at(docs, &at.text_document.uri, at.position)?;
        self.ext.notebook()?;
        let from_guest = self.ext.places.guest(&from);
        let target = self.ext.book.as_ref()?.resolve(&link, from_guest.as_deref());
        // An entry is a place to go; anything else is followed.
        if let Target::Entry { id, heading } = &target {
            let e = self.ext.book.as_ref()?.get(id)?;
            let line = heading.as_deref().and_then(|h| links::heading_line(&e.text, h)).unwrap_or(0) as u32;
            let host = self.ext.places.host(&e.path)?;
            let uri = paths::path_uri(&host).parse::<Uri>().ok()?;
            let at = Position { line, character: 0 };
            return Some(GotoDefinitionResponse::Scalar(Location { uri, range: Range { start: at, end: at } }));
        }
        self.ext.follow(link, Some(&from));
        None
    }

    fn hover(&mut self, _server: u64, docs: &Documents, params: HoverParams) -> Option<Hover> {
        let at = &params.text_document_position_params;
        let uri = &at.text_document.uri;
        let text = |value: String| Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }), range: None });
        if let Some((link, from)) = self.link_at(docs, uri, at.position) {
            self.ext.notebook()?;
            let from_guest = self.ext.places.guest(&from);
            let nb = self.ext.book.as_ref()?;
            return text(match nb.resolve(&link, from_guest.as_deref()) {
                Target::Entry { id, .. } => {
                    let e = nb.get(&id)?;
                    format!("{} **{}** -- `gd` opens it", if e.kind == Kind::Diagram { "Diagram" } else { "Note" }, e.display_name())
                }
                Target::Missing(name) if !link.markdown && !name.is_empty() => format!("No note called **{name}** yet -- `gd` makes it"),
                Target::Missing(name) => format!("Nothing called {name}"),
                Target::File { path, .. } => format!("A file: {} -- `gd` opens it", self.ext.places.shown(&path)),
                Target::ProjectFile { project, path, .. } => format!("{path} in {project} -- `gd` opens it"),
                Target::Url(url) => format!("{url} -- `gd` opens it in the browser"),
                Target::Mib(name) => format!("The MIB's {name} -- `gd` opens it"),
                Target::Task(key) => format!("Task {key} -- `gd` opens it in the agenda"),
            });
        }
        // In a ```mermaid block: how to see it drawn.
        let all = docs.text(uri)?;
        exports::mermaid_block_at(all, at.position.line as usize)?;
        text("A Mermaid diagram -- `SPC m p` draws it beside; `SPC n s` keeps it in the notebook".into())
    }
}

impl NotebookExt {
    /// One message for its language server.
    pub(crate) fn language_server(&mut self, server: u64, message: &str) {
        let changing = message.contains("\"textDocument/didChange\"");
        let mut servers: Servers = std::mem::take(&mut self.servers);
        servers.handle(server, message, &mut Lsp { ext: self, changing });
        self.servers = servers;
    }

    /// Autosave: writes each notebook file a second after its last
    /// change; looks again for the ones still being typed.
    pub(crate) fn autosave_due(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self.dirty.iter().filter(|(_, at)| now.duration_since(**at) >= AUTOSAVE_AFTER).map(|(uri, _)| uri.clone()).collect();
        for uri in &due {
            self.dirty.remove(uri);
            self.save_document(uri);
        }
        if let Some(next) = self.dirty.values().map(|at| AUTOSAVE_AFTER.saturating_sub(now.duration_since(*at))).min() {
            self.autosave = Some(host::set_timer(next.as_millis().max(50) as u32, false));
        }
        if !due.is_empty() {
            self.changed();
        }
    }

    /// Writes notebook file `uri` as its buffer has it, keeping the
    /// version before this editing session in its history.
    pub(crate) fn save_document(&mut self, uri: &str) {
        let Some(guest) = self.places.guest(&paths::uri_path(uri)) else { return };
        let spelled = guest.to_string_lossy().into_owned();
        let Some(text) = fenix_extension_api::buffer::text(&spelled) else { return };
        let Some(nb) = self.notebook() else { return };
        if nb.by_path(&guest).is_none() {
            // A file made by hand: an entry from now on.
            let _ = nb.rescan();
        }
        let entry = nb.by_path(&guest).map(|e| e.id.clone());
        if let Some(id) = &entry {
            if let Err(e) = nb.snapshot(id) {
                fenix_extension_api::warn!("couldn't keep a version of {spelled}: {e}");
            }
        }
        match fenix_extension_api::buffer::save(&spelled) {
            Ok(()) => {
                if let (Some(id), Some(nb)) = (&entry, self.book.as_mut()) {
                    nb.refresh_text(id, &text);
                }
            }
            Err(e) => crate::error(&format!("couldn't save {}: {e}", self.host_of(&guest))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_being_typed_is_found_before_the_cursor() {
        assert_eq!(link_prefix("see [[upl"), Some((6, "upl".into())));
        assert_eq!(link_prefix("see [["), Some((6, String::new())));
        assert_eq!(link_prefix("see [[done]] and"), None);
        assert_eq!(link_prefix("[[a|b"), None);
        assert_eq!(link_prefix("plain"), None);
    }
}
