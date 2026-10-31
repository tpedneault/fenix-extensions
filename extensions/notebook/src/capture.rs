//! Getting things into the notebook and finding them again: `SPC n c`
//! captures a line (to today's journal, or the Inbox) from anywhere,
//! with a link back to where you were; `SPC n /` searches every note and
//! diagram with the notebook's query syntax; `SPC n t` sends the
//! checkbox under the cursor to the agenda; and in a journal day,
//! `SPC m a` refreshes its agenda list and `SPC m g` adds today's
//! commits.

use std::path::{Path, PathBuf};

use fenix_extension_api::{self as api, run, Command, PickerItem};

use crate::actions::whole;
use crate::book::Kind;
use crate::query::{self, Query};
use crate::{error, later, meta, on, paths, templates, Asked, NotebookExt};

/// Where a capture goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum To {
    Journal,
    Inbox,
}

/// Where `git` is in finding a journal day's commits.
pub(crate) enum GitStep {
    /// Who you are (`git config user.email`).
    Email { date: chrono::NaiveDate, path: String },
    /// What you committed.
    Log { date: chrono::NaiveDate, path: String, project: String },
}

/// What a capture's text says beyond itself: `[ ]` first makes it a
/// task, `>` first sends it to the other place.
pub(crate) fn parse_capture(text: &str, to: To) -> (To, bool, String) {
    let mut text = text.trim();
    let mut to = to;
    let mut task = false;
    loop {
        if let Some(rest) = text.strip_prefix('>') {
            to = if to == To::Journal { To::Inbox } else { To::Journal };
            text = rest.trim_start();
        } else if let Some(rest) = text.strip_prefix("[ ]").or_else(|| text.strip_prefix("[]")) {
            task = true;
            text = rest.trim_start();
        } else {
            break;
        }
    }
    (to, task, text.to_string())
}

/// The journal day's agenda section of `text`, refreshed to `fresh`'s.
pub(crate) fn refresh_agenda_section(text: &str, fresh: &str) -> Option<String> {
    let section = |text: &str| -> Option<String> {
        let start = text.find("## Agenda")?;
        let rest = &text[start..];
        let end = rest[3..].find("\n## ").map(|e| e + 4).unwrap_or(rest.len());
        Some(rest[..end].trim_end().to_string())
    };
    Some(match (section(text), section(fresh)) {
        (Some(old), Some(new)) => text.replacen(&old, &new, 1),
        (Some(old), None) => text.replacen(&old, "## Agenda\n\nNothing due.", 1),
        (None, Some(new)) => match text.find("\n## ") {
            Some(at) => format!("{}\n\n{new}\n{}", text[..at].trim_end(), &text[at..]),
            None => format!("{}\n\n{new}\n", text.trim_end()),
        },
        (None, None) => return None,
    })
}

/// `text` with its `## Commits` section made of `lines`.
pub(crate) fn with_commits(text: &str, lines: &[String]) -> String {
    let block = format!("## Commits\n\n{}\n", lines.join("\n"));
    match text.find("## Commits") {
        Some(at) => {
            let rest = &text[at..];
            let end = rest[3..].find("\n## ").map(|e| e + 4).unwrap_or(rest.len());
            format!("{}{}{}", &text[..at], block, &rest[end..])
        }
        None => format!("{}\n\n{block}", text.trim_end()),
    }
}

impl NotebookExt {
    /// Where captures go unless you say: today's journal, or the Inbox
    /// with `notebook.journal` off.
    fn capture_default(&self) -> To {
        if on(api::setting("notebook.journal"), true) {
            To::Journal
        } else {
            To::Inbox
        }
    }

    /// `SPC n c`: a line into the notebook from wherever you are.
    pub(crate) fn capture(&mut self) {
        if self.notebook().is_none() {
            return;
        }
        let to = self.capture_default();
        // A link back to the line, when there's a file here (that isn't a
        // notebook entry itself).
        let here = self.here();
        let link = here.and_then(|h| Some((h.path?, h.line))).filter(|(p, _)| self.entry_at(p).is_none()).map(|(p, line)| self.link_to(&p, Some(line as usize)));
        let other = if to == To::Journal { "the Inbox" } else { "today's journal" };
        let back = link.as_deref().map(|l| format!(" · links {l}")).unwrap_or_default();
        let id = api::prompt(&format!("Capture → {} ([ ] first: a task, > first: {other}){back}", to.label()), "");
        self.asked.insert(id, Asked::Capture { link });
    }

    /// Files a capture.
    pub(crate) fn capture_done(&mut self, link: Option<String>, typed: &str) {
        let (to, task, text) = parse_capture(typed, self.capture_default());
        if text.is_empty() {
            return;
        }
        match self.capture_into(to, task, link.as_deref(), &text) {
            Ok(_) => api::notify(&format!("captured to {}", to.label())),
            Err(why) => error(&why),
        }
    }

    /// Appends `text` to the journal day's Log or the Inbox, as `SPC n c`
    /// does, through its buffer when it's open. The file it went into;
    /// why not, when it couldn't. `fenix/notebook.capture` comes here too.
    pub(crate) fn capture_into(&mut self, to: To, task: bool, link: Option<&str>, text: &str) -> Result<PathBuf, String> {
        let text = text.trim();
        let now = crate::clock::now();
        let line = templates::capture_line(text, task, link, now.time());
        let template = self.journal_template(now.date_naive());
        let id = match to {
            To::Journal => self.notebook().and_then(|nb| nb.ensure_day(now.date_naive(), || template).ok()).map(|(id, _)| id),
            To::Inbox => self.notebook().and_then(|nb| match nb.by_name("Inbox") {
                Some(e) => Some(e.id.clone()),
                None => nb.create(Kind::Note, "Inbox", "# Inbox\n\nThings captured to sort out later.\n").ok(),
            }),
        };
        let Some(id) = id else {
            return Err(self.failed.clone().unwrap_or_else(|| "couldn't open the notebook to capture into".to_string()));
        };
        self.flush(&id);
        let path = self.book.as_ref().and_then(|nb| nb.get(&id)).map(|e| e.path.clone()).ok_or("the notebook lost the entry it was capturing into")?;
        // As it is now: the open buffer's, else the file's.
        let before = api::buffer::text(&path.to_string_lossy()).or_else(|| self.book.as_ref().and_then(|nb| nb.get(&id)).map(|e| e.text.clone())).unwrap_or_default();
        let after = templates::append_to_log(&before, &line);
        self.write_entry(&id, &after);
        match self.book.as_ref().and_then(|nb| nb.get(&id)) {
            Some(e) if e.text == after => Ok(e.path.clone()),
            Some(e) => Err(format!("couldn't write {}", self.host_of(&e.path))),
            None => Err("the notebook lost the entry it was capturing into".to_string()),
        }
    }

    /// Every line `query` matches, as a picker.
    pub(crate) fn search_results(&mut self, text: &str) {
        let Some(nb) = self.notebook() else { return };
        let _ = nb.rescan();
        let q = Query::parse(text);
        let results = query::search(nb, &q, 20);
        let mut items = Vec::new();
        for (id, hits) in &results {
            let Some(e) = nb.get(id) else { continue };
            for hit in hits {
                let (label, value) = match hit.line {
                    Some(l) => (format!("{} {}  {:>4}  {}", e.kind.tag(), e.display_name(), l + 1, hit.text), format!("{id}:{l}")),
                    None => (format!("{} {}", e.kind.tag(), e.display_name()), id.clone()),
                };
                items.push(PickerItem::new(label, value));
            }
        }
        if items.is_empty() {
            return api::notify(&format!("nothing in the notebook matches {text}"));
        }
        let found = results.len();
        let id = api::pick("Found", &items);
        self.asked.insert(id, Asked::Open);
        api::notify(&format!("{found} match{} -- typing narrows them", if found == 1 { "" } else { "es" }));
    }

    // ------------------------------------------------------------------
    // Tasks
    // ------------------------------------------------------------------

    /// `SPC n t`: the checkbox under the cursor, as an agenda task (once;
    /// the line gets a mark that it went).
    pub(crate) fn send_to_agenda(&mut self) {
        let Some(here) = self.here() else { return error("SPC n t sends a checkbox line (- [ ] ...) to the agenda") };
        let Some(path) = here.path.clone() else { return error("SPC n t sends a checkbox line (- [ ] ...) to the agenda") };
        let line = here.line.saturating_sub(1) as usize;
        let text = api::buffer::text(&path).and_then(|t| t.lines().nth(line).map(str::to_string)).unwrap_or_else(|| here.text.lines().next().unwrap_or("").to_string());
        let Some(task) = meta::task_on(text.trim_end_matches(['\n', '\r']), line) else {
            return error("SPC n t sends a checkbox line (- [ ] ...) to the agenda");
        };
        if text.contains("→ agenda") {
            return api::notify("that one's in the agenda already");
        }
        let title: String = task.text.split(" · ").next().unwrap_or(&task.text).trim().to_string();
        let payload = serde_json::json!({ "title": title, "path": path, "line": line + 1 }).to_string();
        run::spawn(async move {
            let said = run::call("agenda.add_task", &payload).await;
            later(move |_| match said {
                Ok(_) => mark_sent_to_agenda(&path, line, &title),
                Err(e) if e.contains("no service") => error("SPC n t sends it to the agenda -- that's an extension (SPC h x installs it)"),
                Err(e) => error(&format!("couldn't add it to the agenda: {e}")),
            });
        });
    }

    // ------------------------------------------------------------------
    // Journal days
    // ------------------------------------------------------------------

    /// The journal day you're in: its date and its file, as Fenix names it.
    fn focused_day(&mut self) -> Option<(chrono::NaiveDate, String)> {
        let path = self.here()?.path?;
        self.notebook()?;
        let date = self.entry_at(&path)?.date()?;
        Some((date, path))
    }

    /// `SPC m a` in a journal day: its agenda list, as the agenda is now.
    pub(crate) fn journal_refresh_agenda(&mut self) {
        let Some((date, path)) = self.focused_day() else { return error("SPC m a refreshes a journal day's agenda list") };
        self.fetch_due(move |ext| {
            let fresh = ext.journal_template(date);
            let Some(text) = api::buffer::text(&path) else { return };
            match refresh_agenda_section(&text, &fresh) {
                Some(new) => {
                    if let Err(e) = api::buffer::edit(&path, &[whole(&text, &new)]) {
                        return error(&e);
                    }
                    api::notify("agenda list refreshed");
                }
                None => api::notify("nothing due in the agenda"),
            }
        });
    }

    /// `SPC m g` in a journal day: what you committed that day in the
    /// project you're in.
    pub(crate) fn journal_commits(&mut self) {
        let Some((date, path)) = self.focused_day() else { return error("SPC m g adds a journal day's commits") };
        if !Path::new(paths::PROJECT).is_dir() {
            return error("SPC m g adds the commits of the project you're in -- open a file in one first");
        }
        self.git(&["config", "user.email"], GitStep::Email { date, path });
    }

    fn git(&mut self, args: &[&str], step: GitStep) {
        match Command::new("git").args(args.iter().copied()).cwd(paths::PROJECT).spawn() {
            Ok(id) => {
                self.asked.insert(id, Asked::Git { step, out: Vec::new() });
            }
            Err(e) => error(&format!("couldn't run git: {e}")),
        }
    }

    /// `git` finished a step.
    pub(crate) fn git_done(&mut self, step: GitStep, ok: bool, out: Vec<String>) {
        match step {
            GitStep::Email { date, path } => {
                let Some(email) = out.first().map(|e| e.trim().to_string()).filter(|e| ok && !e.is_empty()) else {
                    return error("git doesn't know who you are here (git config user.email)");
                };
                let since = format!("--since={} 00:00", date.format("%Y-%m-%d"));
                let until = format!("--until={} 00:00", (date + chrono::Duration::days(1)).format("%Y-%m-%d"));
                let author = format!("--author={email}");
                let project = self.here().and_then(|h| h.project).map(|p| p.name).unwrap_or_else(|| "project".into());
                self.git(&["log", "--all", "--no-merges", &author, &since, &until, "--format=%h %s"], GitStep::Log { date, path, project });
            }
            GitStep::Log { date, path, project } => {
                let lines: Vec<String> = out
                    .iter()
                    .filter(|l| ok && !l.trim().is_empty())
                    .map(|l| {
                        let (hash, subject) = l.split_once(' ').unwrap_or((l, ""));
                        format!("- {project} `{hash}` {subject}")
                    })
                    .collect();
                if lines.is_empty() {
                    return api::notify(&format!("no commits of yours on {}", date.format("%b %-d")));
                }
                let Some(text) = api::buffer::text(&path) else { return };
                let new = with_commits(&text, &lines);
                match api::buffer::edit(&path, &[whole(&text, &new)]) {
                    Ok(()) => api::notify(&format!("{} commit{} added", lines.len(), if lines.len() == 1 { "" } else { "s" })),
                    Err(e) => error(&e),
                }
            }
        }
    }
}

/// Checkbox `line` of `path` went to the agenda: marked so.
fn mark_sent_to_agenda(path: &str, line: usize, title: &str) {
    let Some(text) = api::buffer::text(path) else { return };
    let Some(len) = text.lines().nth(line).map(|l| l.trim_end_matches('\r').chars().count()) else { return };
    match api::buffer::edit(path, &[api::TextEdit::insert((line as u32, len as u32), " → agenda")]) {
        Ok(()) => api::notify(&format!("added to the agenda: {title}")),
        Err(e) => error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_says_where_it_goes_and_whether_its_a_task() {
        assert_eq!(parse_capture("retry is 3", To::Journal), (To::Journal, false, "retry is 3".into()));
        assert_eq!(parse_capture("[ ] call OBSW", To::Journal), (To::Journal, true, "call OBSW".into()));
        assert_eq!(parse_capture("> [ ] call OBSW", To::Journal), (To::Inbox, true, "call OBSW".into()));
        assert_eq!(parse_capture(">sort this", To::Inbox), (To::Journal, false, "sort this".into()));
        assert_eq!(parse_capture("   ", To::Inbox).2, "");
    }

    #[test]
    fn a_journal_days_agenda_and_commits_are_put_in_place() {
        let day = "# Tuesday\n\n## Agenda\n\n- [ ] old · due today\n\n## Log\n\n- a\n";
        let fresh = "# Tuesday\n\n## Agenda\n\n- [ ] new · due today\n\n## Log\n\n";
        assert_eq!(refresh_agenda_section(day, fresh).unwrap(), "# Tuesday\n\n## Agenda\n\n- [ ] new · due today\n\n## Log\n\n- a\n");
        assert_eq!(refresh_agenda_section(day, "# T\n\n## Log\n\n").unwrap(), "# Tuesday\n\n## Agenda\n\nNothing due.\n\n## Log\n\n- a\n");
        assert_eq!(refresh_agenda_section("# T\n\n## Log\n", "# T\n\n## Log\n"), None);
        let with = with_commits("# T\n\n## Log\n", &["- fenix `abc1234` Fix it".into()]);
        assert_eq!(with, "# T\n\n## Log\n\n## Commits\n\n- fenix `abc1234` Fix it\n");
        assert_eq!(with_commits(&with, &["- fenix `def` Again".into()]), "# T\n\n## Log\n\n## Commits\n\n- fenix `def` Again\n");
    }
}
