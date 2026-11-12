//! The MIB in your scripts, as a language server Fenix asks with every
//! other: `K` on a MIB name is its card, `gd` its page, completion
//! offers telecommands and parameters, and telecommand calls the MIB
//! disagrees with are underlined (`mib.check_scripts`). Only in the
//! files `mib.editor_files` lists -- every file when it lists none.

use fenix_extension_api::lsp::types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    MarkupContent, MarkupKind, OneOf, Range, ServerCapabilities, Uri, CompletionOptions,
};
use fenix_extension_api::lsp::{self, Documents, LanguageServer};
use fenix_mib::{DefRef, Kind, MibSet};

use crate::keys::Hints;
use crate::{mib_check, mib_page, Mib};

/// The words of `line` (letters, digits, `_`), each with the columns it
/// spans.
pub(crate) fn words(line: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (col, &(byte, c)) in chars.iter().enumerate() {
        let word = c.is_alphanumeric() || c == '_';
        match (word, start) {
            (true, None) => start = Some((col, byte)),
            (false, Some((sc, sb))) => {
                out.push((sc..col, &line[sb..byte]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some((sc, sb)) = start {
        out.push((sc..chars.len(), &line[sb..]));
    }
    out
}

/// What `K` shows on a MIB name.
pub(crate) fn hover_text(set: &MibSet, def: DefRef, apid_hex: bool, keys: &Hints) -> String {
    let e = set.get(def);
    let mut lines = vec![format!("{} {} · {}", def.kind.tag(), e.name, set.root_label(e.root)), e.description.clone()];
    let r = &e.row;
    match def.kind {
        Kind::Telecommand => {
            lines.push(format!("PUS {},{} · APID {} · {}", r.clean("CCF_TYPE"), r.clean("CCF_STYPE"), fenix_mib::types::apid(r.clean("CCF_APID"), apid_hex), r.clean("CCF_SUBSYS")));
            let els: Vec<_> = fenix_mib::detail::tc_elements(set, def).into_iter().filter(|el| el.kind == fenix_mib::layout::ElementKind::Editable && !el.name.is_empty()).collect();
            if !els.is_empty() {
                lines.push(String::new());
                lines.push("Arguments".to_string());
                for el in els {
                    let repeats = if el.group > 0 { format!("repeats the next {}", el.group) } else { String::new() };
                    let default = if el.default.is_empty() { String::new() } else { format!("default {}", el.default) };
                    let what = [el.description.as_str(), el.allowed.as_str(), default.as_str(), repeats.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
                    let indent = "  ".repeat(1 + el.within.len());
                    lines.push(format!("{indent}{:<10} {what}", el.name));
                }
            }
            let pre = fenix_mib::commanding::preconditions(set, def);
            if !pre.is_empty() {
                lines.push(String::new());
                lines.push(format!("Sent only while {}", pre.iter().map(ToString::to_string).collect::<Vec<_>>().join(" and ")));
            }
            lines.push(String::new());
            lines.push(format!("{} opens it · {} edits this call", keys.definition, keys.edit_call));
        }
        _ => {
            let cols: Vec<String> = (1..def.kind.headers().len())
                .map(|c| (def.kind.headers()[c], mib_page::cell(set, def, c, apid_hex)))
                .filter(|(_, v)| !v.is_empty())
                .map(|(h, v)| format!("{} {v}", h.to_lowercase()))
                .collect();
            if !cols.is_empty() {
                lines.push(cols.join(" · "));
            }
            lines.push(String::new());
            lines.push(format!("{} opens it", keys.definition));
        }
    }
    lines.retain(|l| !l.trim().is_empty() || l.is_empty());
    lines.join("\n")
}

/// Whether MIB names mean something in document `uri`.
fn names_doc(uri: &Uri) -> bool {
    crate::names_file(uri.as_str())
}

impl Mib {
    /// The MIB definition the word at the position names.
    fn def_at(&mut self, docs: &Documents, at: &fenix_extension_api::lsp::types::TextDocumentPositionParams) -> Option<(std::sync::Arc<MibSet>, DefRef)> {
        if !names_doc(&at.text_document.uri) {
            return None;
        }
        // A MIB name is letters, digits and `_`: `-PTH00101`, an argument
        // as a script writes it, names PTH00101.
        let line = docs.line(&at.text_document.uri, at.position.line)?;
        let col = lsp::utf16_to_char(line, at.position.character);
        let word = words(line).into_iter().find(|(cols, _)| cols.contains(&col) || cols.end == col).map(|(_, w)| w.to_string())?;
        let set = self.set()?;
        let def = set.resolve(&word)?;
        Some((set, def))
    }
}

impl LanguageServer for Mib {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            completion_provider: Some(CompletionOptions::default()),
            ..Default::default()
        }
    }

    /// Its telecommand calls checked against the MIB, when they're on.
    fn changed(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        let mut diagnostics = Vec::new();
        if crate::on("mib.check_scripts", true) && names_doc(uri) {
            if let (Some(text), Some(set)) = (docs.text(uri), self.set()) {
                for f in mib_check::check(text, &set, &crate::templates()) {
                    let line = text.lines().nth(f.line).unwrap_or("");
                    let start = lsp::position(f.line as u32, lsp::char_to_utf16(line, f.cols.start));
                    let end = lsp::position(f.line as u32, lsp::char_to_utf16(line, f.cols.end));
                    diagnostics.push(Diagnostic {
                        range: Range { start, end },
                        severity: Some(match f.severity {
                            mib_check::Severity::Error => DiagnosticSeverity::ERROR,
                            mib_check::Severity::Warning => DiagnosticSeverity::WARNING,
                            mib_check::Severity::Info => DiagnosticSeverity::INFORMATION,
                        }),
                        source: Some("MIB".into()),
                        message: f.message,
                        ..Default::default()
                    });
                }
            }
        }
        lsp::publish_diagnostics(server, uri.clone(), diagnostics);
    }

    /// `K` on a MIB name: its card.
    fn hover(&mut self, _server: u64, docs: &Documents, params: HoverParams) -> Option<Hover> {
        let (set, def) = self.def_at(docs, &params.text_document_position_params)?;
        Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::PlainText, value: hover_text(&set, def, crate::apid_hex(), &Hints::current()) }), range: None })
    }

    /// `gd` on a MIB name: its page, opened once the answer's sent -- it
    /// isn't somewhere in a file.
    fn definition(&mut self, _server: u64, docs: &Documents, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let (_, def) = self.def_at(docs, &params.text_document_position_params)?;
        self.to_open.push(def);
        None
    }

    /// The MIB's telecommands and parameters, with what they are.
    fn completion(&mut self, _server: u64, _docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        if !names_doc(&params.text_document_position.text_document.uri) {
            return None;
        }
        let set = self.set()?;
        let mut items = Vec::new();
        for (kind, item) in [(Kind::Telecommand, CompletionItemKind::FUNCTION), (Kind::TcParam, CompletionItemKind::VARIABLE), (Kind::TmParam, CompletionItemKind::VARIABLE)] {
            for e in set.entries(kind) {
                items.push(CompletionItem { label: e.name.clone(), kind: Some(item), detail: Some(format!("{} · {}", kind.tag(), e.description)), ..Default::default() });
            }
        }
        Some(CompletionResponse::Array(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_carry_their_columns() {
        let w = words("  tc::send ZTC08101 -PTH00101 3");
        let names: Vec<&str> = w.iter().map(|(_, w)| *w).collect();
        assert_eq!(names, vec!["tc", "send", "ZTC08101", "PTH00101", "3"]);
        assert_eq!(w[2].0, 11..19);
        assert_eq!(words("é_x"), vec![(0..3, "é_x")]);
    }
}
