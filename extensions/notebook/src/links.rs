//! Links between notes: following one (making the note when it doesn't
//! exist yet), `SPC n b`'s sidebar of what links here beside the note,
//! `SPC n l` inserting a link through a picker, `SPC n y` copying a link
//! to the line of code you're on, and `SPC n v` pasting a picture from
//! the clipboard next to the note. `[[` completing a name and `gd`
//! following a link are its language server's (`lsp`).

use std::path::Path;

use fenix_extension_api::{self as api, host, run, PickerItem};

use crate::book::{self, Kind, Notebook, Target};
use crate::links_page::{Action as LinkAction, Item, LinksPage};
use crate::meta::{self, Link};
use crate::{error, later, paths, templates, Asked, NotebookExt, LINKS_KIND};

/// A link as Fenix's reading view sends it: `{target, heading, markdown,
/// embed}`.
pub(crate) fn link_of(v: &serde_json::Value) -> Link {
    let text = |key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let flag = |key: &str| v.get(key).and_then(|x| x.as_bool()).unwrap_or(false);
    Link { target: text("target").unwrap_or_default(), heading: text("heading").filter(|h| !h.is_empty()), alias: None, embed: flag("embed"), markdown: flag("markdown"), line: 0, cols: 0..0 }
}

/// What a link goes to, for the reading view: whether it's there, and an
/// entry's kind, name and text (an embed shows those).
pub(crate) fn describe(nb: &Notebook, link: &Link, from: Option<&Path>) -> serde_json::Value {
    match nb.resolve(link, from) {
        Target::Entry { id, .. } => match nb.get(&id) {
            Some(e) => serde_json::json!({ "exists": true, "kind": if e.kind == Kind::Diagram { "diagram" } else { "note" }, "name": if e.kind == Kind::Diagram { e.name.clone() } else { e.display_name() }, "text": e.text }),
            None => serde_json::json!({ "exists": false, "kind": "missing" }),
        },
        Target::Missing(_) => serde_json::json!({ "exists": false, "kind": "missing" }),
        _ => serde_json::json!({ "exists": true, "kind": "other" }),
    }
}

/// The line of heading `title` in `text`, by its words or its slug.
pub(crate) fn heading_line(text: &str, title: &str) -> Option<usize> {
    let want = title.trim().to_lowercase();
    let want_slug = book::slug(title);
    meta::headings(text).into_iter().find(|h| h.title.to_lowercase() == want || book::slug(&h.title) == want_slug).map(|h| h.line)
}

impl NotebookExt {
    /// Goes where `link`, written in the file `from` (as Fenix names it),
    /// points.
    pub(crate) fn follow(&mut self, link: Link, from: Option<&str>) {
        if self.notebook().is_none() {
            return;
        }
        let from_guest = from.and_then(|f| self.places.guest(f));
        let target = self.book.as_ref().map(|nb| nb.resolve(&link, from_guest.as_deref())).unwrap_or(Target::Missing(link.target.clone()));
        match target {
            Target::Entry { id, heading } => {
                let line = heading.and_then(|h| self.book.as_ref().and_then(|nb| nb.get(&id)).and_then(|e| heading_line(&e.text, &h)));
                self.open_entry(&id, line);
            }
            Target::Missing(name) if !link.markdown && !name.is_empty() => self.create_note(&name, templates::BUILT_IN[0].body),
            Target::Missing(name) => error(&format!("nothing called {name}")),
            Target::File { path, fragment } => self.open_file_at(&path, fragment.as_deref()),
            Target::ProjectFile { project, path, line } => {
                self.here();
                let root = self.projects.iter().find(|r| paths::slashed(r).rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(&project))).cloned();
                let Some(root) = root else {
                    return error(&format!("no project called {project} that Fenix has shown the notebook -- open a file in it first"));
                };
                let sep = if root.contains('\\') { "\\" } else { "/" };
                let file = format!("{}{sep}{}", root.trim_end_matches(['/', '\\']), path.replace('/', sep));
                host::open_file(&file, line.map(|l| l as u32));
            }
            Target::Url(url) => {
                if let Err(e) = api::open_url(&url) {
                    error(&format!("couldn't open {url}: {e}"));
                }
            }
            Target::Mib(name) => {
                let payload = serde_json::json!({ "name": name }).to_string();
                run::spawn(async move {
                    if let Err(e) = run::call("mib.open", &payload).await {
                        later(move |_| error(&e));
                    }
                });
            }
            Target::Task(key) => {
                let payload = serde_json::json!({ "key": key }).to_string();
                run::spawn(async move {
                    match run::call("agenda.open_task", &payload).await {
                        Ok(_) => {}
                        Err(e) if e.contains("no service") => later(move |_| api::notify(&format!("{key} is a task -- the agenda is an extension (SPC h x installs it)"))),
                        Err(_) => later(move |_| api::notify(&format!("{key} isn't in the agenda -- SPC A i imports it"))),
                    }
                });
            }
        }
    }

    /// Opens a file a link names: a PDF at its `#page=`, a file at its
    /// `#L` line or heading.
    fn open_file_at(&mut self, path: &Path, fragment: Option<&str>) {
        let spelled = path.to_string_lossy().replace('\\', "/");
        let inside = !paths::is_host_absolute(&spelled);
        if inside && !path.exists() {
            return error(&format!("no file {}", self.host_of(path)));
        }
        let open = if inside { spelled.clone() } else { path.to_string_lossy().into_owned() };
        let pdf = spelled.to_lowercase().ends_with(".pdf");
        let line = match fragment {
            // A PDF opens at the page given as its line.
            Some(f) if pdf => f.strip_prefix("page=").and_then(|p| p.parse::<u32>().ok()),
            Some(f) => f.strip_prefix('L').and_then(|l| l.parse::<u32>().ok()).or_else(|| {
                let text = if inside { std::fs::read_to_string(path).ok() } else { api::buffer::text(&open) };
                text.and_then(|t| heading_line(&t, f)).map(|l| l as u32 + 1)
            }),
            None => None,
        };
        host::open_file(&open, line);
    }

    /// A link to `path` (a file as Fenix names it; at `line`, from 1): a
    /// notebook entry by name, a project file as `project:path:line`,
    /// anything else by full path.
    pub(crate) fn link_to(&mut self, path: &str, line: Option<usize>) -> String {
        self.notebook();
        if let Some(e) = self.entry_at(path) {
            return format!("[[{}]]", e.name);
        }
        let project = self.here().and_then(|h| h.project);
        let slashed = paths::slashed(path);
        let rel = project.as_ref().and_then(|p| {
            let root = paths::slashed(&p.root);
            let lower = |s: &str| if cfg!(windows) || s.as_bytes().get(1) == Some(&b':') { s.to_lowercase() } else { s.to_string() };
            lower(&slashed).starts_with(&format!("{}/", lower(&root))).then(|| (p.name.clone(), slashed[root.len() + 1..].to_string()))
        });
        match rel {
            Some((name, rel)) => match line {
                Some(l) => format!("[[{name}:{rel}:{l}]]"),
                None => format!("[[{name}:{rel}]]"),
            },
            None => {
                let file = slashed.rsplit('/').next().unwrap_or("").to_string();
                let p = slashed.replace(' ', "%20");
                match line {
                    Some(l) => format!("[{file}:{l}]({p}#L{l})"),
                    None => format!("[{file}]({p})"),
                }
            }
        }
    }

    /// `SPC n l`: a link to a note, inserted at the cursor.
    pub(crate) fn insert_link(&mut self) {
        let Some(nb) = self.notebook() else { return };
        let items: Vec<PickerItem> = nb.recent().into_iter().map(|e| PickerItem::new(format!("{}  {}", e.kind.tag(), e.display_name()), e.name.clone())).collect();
        let id = api::pick("Link", &items);
        self.asked.insert(id, Asked::InsertLink);
    }

    /// `SPC n y`: a link to the line you're on -- `[[project:path:line]]`
    /// in a project, the file's full path otherwise -- to the clipboard.
    pub(crate) fn copy_link(&mut self) {
        let Some((path, line)) = self.here().and_then(|h| Some((h.path?, h.line))) else { return error("this buffer isn't a file") };
        let link = self.link_to(&path, Some(line as usize));
        api::copy(&link);
        api::notify(&format!("copied {link}"));
    }

    /// `SPC n v`: the clipboard's picture, saved beside the note (in its
    /// attachments folder) and linked at the cursor.
    pub(crate) fn paste_image(&mut self) {
        let Some(path) = self.here().and_then(|h| h.path) else { return error("pictures go into a saved note") };
        let Some(png) = api::pasted_image() else { return error("there's no picture on the clipboard") };
        self.notebook();
        let Some(guest) = self.places.guest(&path) else { return error("pictures go into a note in the notebook or the project") };
        let entry = self.book.as_ref().and_then(|nb| nb.by_path(&guest)).map(|e| e.id.clone());
        let dir = match entry.and_then(|id| self.book.as_ref().and_then(|nb| nb.attachments_dir(&id))) {
            Some(dir) => dir,
            None => guest.parent().map(|p| p.join(format!("{}_files", guest.file_stem().unwrap_or_default().to_string_lossy()))).unwrap_or_default(),
        };
        let name = format!("paste-{}.png", crate::clock::now().format("%m%d-%H%M%S"));
        let file = dir.join(&name);
        if let Err(e) = book::write_file(&file, &png) {
            return error(&format!("couldn't save the picture: {e}"));
        }
        let rel = paths::relative_path(guest.parent().unwrap_or(Path::new(".")), &file);
        self.insert_text(&format!("![]({})", rel.replace(' ', "%20")));
        api::notify(&format!("pasted {}", self.host_of(&file)));
    }

    // ------------------------------------------------------------------
    // The links sidebar
    // ------------------------------------------------------------------

    /// `SPC n b`: what links to the note you're in, beside it.
    pub(crate) fn backlinks(&mut self) {
        let Some(path) = self.here().and_then(|h| h.path) else { return error("SPC n b shows a notebook note's links") };
        if self.notebook().is_none() {
            return;
        }
        let Some(entry) = self.entry_at(&path).cloned() else { return error("this file isn't in the notebook -- SPC n i imports it") };
        // One sidebar: it moves to this note.
        match &mut self.links_page {
            Some((id, p)) => {
                *p = LinksPage::new(entry.id.clone(), entry.display_name());
                api::page::refresh(*id);
            }
            None => {
                let id = api::page::open_beside(LINKS_KIND, &format!("*links: {}*", entry.display_name()), 0.3);
                self.links_page = Some((id, LinksPage::new(entry.id.clone(), entry.display_name())));
            }
        }
        self.refresh_links_page();
    }

    /// Fills the sidebar's lists for its note, as the notebook now is.
    pub(crate) fn refresh_links_page(&mut self) {
        let Some((page_id, p)) = &self.links_page else { return };
        let (page_id, id) = (*page_id, p.id.clone());
        let Some(nb) = self.book.as_ref() else { return };
        let Some(entry) = nb.get(&id).cloned() else { return };
        let row = |nb: &Notebook, b: book::Backlink| {
            let e = nb.get(&b.from);
            Item { id: b.from.clone(), name: e.map(|e| e.display_name()).unwrap_or_default(), kind: e.map(|e| e.kind.tag()).unwrap_or(""), line: b.line, context: b.context, exists: true }
        };
        let here: Vec<Item> = nb.backlinks(&id).into_iter().map(|b| row(nb, b)).collect();
        let mentions: Vec<Item> = nb.unlinked_mentions(&id).into_iter().map(|b| row(nb, b)).collect();
        let outline: Vec<Item> = meta::headings(&entry.text)
            .into_iter()
            .map(|h| Item { id: String::new(), name: h.title, kind: ["", "", "  ", "    ", "      ", "        ", "          "][h.level.min(6)], line: h.line, context: String::new(), exists: true })
            .collect();
        let out: Vec<Item> = entry
            .links
            .iter()
            .map(|l| {
                let (kind, exists) = match nb.resolve(l, Some(&entry.path)) {
                    Target::Entry { id, .. } => (nb.get(&id).map(|e| e.kind.tag()).unwrap_or("NOTE"), true),
                    Target::Missing(_) => ("NEW", false),
                    Target::File { .. } | Target::ProjectFile { .. } => ("FILE", true),
                    Target::Url(_) => ("URL", true),
                    Target::Mib(_) => ("MIB", true),
                    Target::Task(_) => ("TASK", true),
                };
                Item { id: String::new(), name: l.label(), kind, line: l.line, context: String::new(), exists }
            })
            .collect();
        if let Some((_, p)) = &mut self.links_page {
            p.here = here;
            p.mentions = mentions;
            p.outline = outline;
            p.out = out;
            p.clamp();
        }
        api::page::refresh(page_id);
    }

    pub(crate) fn links_action(&mut self, page_id: u64, action: LinkAction) {
        let Some(note_id) = self.links_page.as_ref().map(|(_, p)| p.id.clone()) else { return };
        match action {
            LinkAction::None => {}
            LinkAction::Close => {
                self.links_page = None;
                api::page::close(page_id);
            }
            LinkAction::Open { id, line } => self.open_entry(&id, Some(line)),
            LinkAction::Line(line) => self.open_entry(&note_id, Some(line)),
            LinkAction::Follow { name: _, line } => {
                let Some(e) = self.book.as_ref().and_then(|nb| nb.get(&note_id)).cloned() else { return };
                let from = self.host_of(&e.path);
                if let Some(link) = e.links.iter().find(|l| l.line == line).cloned() {
                    self.follow(link, Some(&from));
                }
            }
            LinkAction::Link { id, line } => {
                let Some(nb) = self.book.as_ref() else { return };
                let (Some(target), Some(entry)) = (nb.get(&note_id).cloned(), nb.get(&id).cloned()) else { return };
                let text = link_mention(&entry.text, line, &target.name);
                self.write_entry(&id, &text);
                self.refresh_links_page();
                api::notify(&format!("linked {} in {}", target.name, entry.display_name()));
            }
        }
    }

    /// `SPC i D`: a notebook diagram into this buffer, as a block or as an
    /// exported SVG and a link to it.
    pub(crate) fn insert_diagram(&mut self) {
        let Some(nb) = self.notebook() else { return };
        let mut items = Vec::new();
        for e in nb.recent().into_iter().filter(|e| e.kind == Kind::Diagram) {
            items.push(PickerItem::new(format!("block  {}", e.name), format!("block:{}", e.id)));
            items.push(PickerItem::new(format!("SVG    {}  (exported beside, linked)", e.name), format!("svg:{}", e.id)));
        }
        if items.is_empty() {
            return api::notify("no diagrams in the notebook yet -- SPC n d makes one");
        }
        let id = api::pick("Diagram", &items);
        self.asked.insert(id, Asked::InsertDiagram);
    }
}

/// `text` with the mention of `name` on `line` made a `[[link]]`.
pub(crate) fn link_mention(text: &str, line: usize, name: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if let Some(l) = lines.get_mut(line) {
        let lower = l.to_lowercase();
        if let Some(at) = lower.find(&name.to_lowercase()) {
            let end = at + name.len();
            let shown = l[at..end].to_string();
            let link = if shown == name { format!("[[{name}]]") } else { format!("[[{name}|{shown}]]") };
            l.replace_range(at..end, &link);
        }
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Where a picture pasted beside `note` would go, for a test.
#[cfg(test)]
fn beside(note: &Path) -> std::path::PathBuf {
    note.parent().unwrap().join(format!("{}_files", note.file_stem().unwrap().to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_becomes_a_link_keeping_how_it_was_written() {
        assert_eq!(link_mention("the uplink is slow\n", 0, "Uplink"), "the [[Uplink|uplink]] is slow\n");
        assert_eq!(link_mention("see Uplink\nx", 0, "Uplink"), "see [[Uplink]]\nx");
    }

    #[test]
    fn links_are_described_for_the_reading_view() {
        let dir = tempfile::tempdir().unwrap();
        let mut nb = Notebook::open(dir.path()).unwrap();
        nb.create(Kind::Diagram, "TC flow", "flowchart TD\n  a --> b\n").unwrap();
        nb.create(Kind::Note, "Ground", "Antenna notes\n").unwrap();
        let link = |v: serde_json::Value| link_of(&v);
        let d = describe(&nb, &link(serde_json::json!({ "target": "TC flow", "embed": true })), None);
        assert_eq!((d["exists"].as_bool(), d["kind"].as_str(), d["name"].as_str()), (Some(true), Some("diagram"), Some("TC flow")));
        let n = describe(&nb, &link(serde_json::json!({ "target": "ground" })), None);
        assert_eq!((n["kind"].as_str(), n["text"].as_str()), (Some("note"), Some("Antenna notes\n")));
        assert_eq!(describe(&nb, &link(serde_json::json!({ "target": "Nowhere" })), None)["exists"], false);
        assert_eq!(describe(&nb, &link(serde_json::json!({ "target": "https://x.org" })), None)["kind"], "other");
    }

    #[test]
    fn headings_are_found_by_their_words_or_slug() {
        let text = "# Target\n\n## Decision made\nhere\n";
        assert_eq!(heading_line(text, "decision made"), Some(2));
        assert_eq!(heading_line(text, "decision-made"), Some(2));
        assert_eq!(heading_line(text, "nope"), None);
        assert!(beside(Path::new("/project/docs/a.md")).ends_with("a_files"));
    }
}
