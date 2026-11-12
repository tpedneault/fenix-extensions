//! `cargo xtask validate`'s part for tutorial lessons: every extension's
//! `tutorial/*.toml`, read as Fenix reads them (`crates/fenix-gui/src/
//! tutorial.rs`, its `Lesson` and `Step`, `validate` and `load`) and held
//! to what Fenix would refuse at load -- an unknown or mistyped field, a
//! step that checks nothing, a mode, picker or terminal it doesn't know,
//! a lesson id it already has -- and to what would load but never work: a
//! page no Fenix has, an extension's page that isn't there, a field or a
//! placeholder older Fenix releases the extension's `fenix` floor admits
//! can't read, a step that passes before anything is done.
//!
//! What it can't know without a Fenix: whether a step's check is what the
//! step's words ask for. That takes playing the lesson through.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Fenix's releases, oldest first: its tags `v1.0.0` to `v1.5.2`. An
/// extension's floor is the first of these its `engine.fenix` admits.
const FENIX_RELEASES: &[&str] = &["1.0.0", "1.0.1", "1.1.0", "1.2.0", "1.3.0", "1.4.0", "1.5.0", "1.5.1", "1.5.2"];

/// What a lesson may use that not every Fenix reads, and the first
/// release that does; `None` is on Fenix's main branch, in no release yet.
/// The rest of the format is Fenix 1.0.0's, unchanged since. From Fenix's
/// history (`git log -- crates/fenix-gui/src/tutorial.rs
/// crates/fenix-gui/src/app/tutorial_host.rs`, `git tag --contains`):
///
/// - `scheme`, and the `{leader}` and `{key:command.id}` its words are put
///   through (`for_scheme`, `lesson_words`): 7c46152, "The tutorial in
///   either key scheme; release 1.5.0". Before, `scheme` is an unknown
///   field -- the lesson is refused -- and a placeholder is shown as it's
///   written.
/// - the `notifications` and `agent-usage` pages: in `tutorial_page_kind`
///   from v1.2.0 (not in v1.1.0's).
/// - the `import-settings` page: after v1.5.2, on main only.
const SINCE: &[(&str, Option<&str>)] = &[
    ("scheme", Some("1.5.0")),
    ("{leader}", Some("1.5.0")),
    ("{key:...}", Some("1.5.0")),
    ("page notifications", Some("1.2.0")),
    ("page agent-usage", Some("1.2.0")),
    ("page import-settings", None),
];

/// Fenix's own pages, as a step's `page` and `closed` name them: the kinds
/// `tutorial_page_kind` in `crates/fenix-gui/src/app/tutorial_host.rs`
/// gives (`home` is the dashboard). Those newer than 1.0.0 are in `SINCE`.
const PAGES: &[&str] = &[
    "home", "new-project", "projects", "doctor", "git", "git-log", "git-compare", "git-rebase", "settings", "snippets", "reading", "welcome", "extensions",
    "extensions-output", "notifications", "tutorial", "explorer", "explorer-preview", "search", "agent", "agent-changes", "agent-usage", "import-settings",
];
/// The Extensions page's tabs, `extensions/<tab>` (its `Tab::label`,
/// lower-cased, in `extensions_page.rs`): a `page`, never a `closed`.
const EXTENSION_TABS: &[&str] = &["installed", "updates", "browse", "bundles"];
/// What's true of the Git page, for a `page` (`GitPage::tutorial_facts`
/// in `git_status.rs`): these, and `git/menu/<menu>`, a `MenuKind`
/// lower-cased.
const GIT_FACTS: &[&str] = &["git/staged", "git/changed", "git/clean"];
const GIT_MENUS: &[&str] = &["commit", "push", "pull", "branch", "stash", "log", "rebase", "commitrow", "reset", "stashrow", "worktree", "help"];

/// What `mode`, `picker` and `terminal` take (`MODES`, `PICKERS`,
/// `TERMINALS` in `tutorial.rs`).
const MODES: &[&str] = &["normal", "insert", "visual", "replace", "command", "search"];
const PICKERS: &[&str] = &["files", "commands", "bindings", "lines", "buffers", "tasks", "any", "none"];
const TERMINALS: &[&str] = &["panel", "pane", "focused", "unfocused", "closed"];
/// What `scheme` takes. Fenix doesn't check it: a lesson for any other is
/// never listed.
const SCHEMES: &[&str] = &["vim", "standard", "any"];

/// Fenix's own lessons (`crates/fenix-gui/tutorial/*.toml`, 1.5.2): their
/// ids, which Fenix refuses an extension's again, and their tracks, which
/// an extension's lessons are listed after.
const BUILT_IN_IDS: &[&str] = &[
    "editing.modes", "editing.words", "editing.objects", "editing.visual", "editing.search", "editing.macros", "fenix.leader", "fenix.find", "fenix.windows",
    "fenix.pages", "fenix.terminals", "fenix.settings", "git.basics", "standard.basics",
];
const BUILT_IN_TRACKS: &[&str] = &["Editing", "Fenix", "Git"];

/// A lesson file, as Fenix's `tutorial::Lesson` reads it: the same fields,
/// types and defaults, and nothing else.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lesson {
    pub id: String,
    pub track: String,
    #[serde(default)]
    pub order: u32,
    pub title: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub scheme: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(rename = "step")]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub repo: Option<PracticeRepo>,
}

/// `[repo]`, Fenix's `tutorial::PracticeRepo`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PracticeRepo {
    pub file: String,
    /// Read only so that a lesson without it is refused, as Fenix refuses it.
    #[allow(dead_code)]
    pub committed: String,
}

/// `[[step]]`, Fenix's `tutorial::Step`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub say: String,
    #[serde(default)]
    pub hint: String,
    pub text: Option<String>,
    pub cursor: Option<[usize; 2]>,
    pub line: Option<usize>,
    pub mode: Option<String>,
    pub windows: Option<String>,
    pub moved: Option<bool>,
    pub page: Option<String>,
    pub closed: Option<String>,
    pub picker: Option<String>,
    pub leader: Option<bool>,
    pub terminal: Option<String>,
}

impl Step {
    /// `Step::checks` in Fenix: how many checks it gives.
    fn checks(&self) -> usize {
        [
            self.text.is_some(),
            self.cursor.is_some(),
            self.line.is_some(),
            self.mode.is_some(),
            self.windows.is_some(),
            self.moved.is_some(),
            self.page.is_some(),
            self.closed.is_some(),
            self.picker.is_some(),
            self.leader.is_some(),
            self.terminal.is_some(),
        ]
        .iter()
        .filter(|c| **c)
        .count()
    }
}

/// An extension as its lessons are checked against it.
#[derive(Debug, Clone, Default)]
pub struct Ext {
    pub id: String,
    pub dir: PathBuf,
    /// `engine.fenix`, as written.
    pub fenix: String,
    /// The first Fenix release that range admits; `None` when it admits
    /// none yet.
    pub floor: Option<semver::Version>,
    pub dependencies: BTreeSet<String>,
    /// `[[contributes.commands]]`' ids.
    pub commands: BTreeSet<String>,
    /// Whether it runs code, and so can have pages.
    pub code: bool,
}

impl Ext {
    /// The id's name part, which its lessons' ids start with.
    fn name(&self) -> &str {
        self.id.rsplit('.').next().unwrap_or(&self.id)
    }
}

/// The first of Fenix's releases `range` admits.
fn floor(range: &str) -> Option<semver::Version> {
    let req = semver::VersionReq::parse(range).ok()?;
    FENIX_RELEASES.iter().filter_map(|v| semver::Version::parse(v).ok()).find(|v| req.matches(v))
}

/// Whether `ext`'s floor reads `what` (a key of `SINCE`); else why not.
fn needs(what: &str, ext: &Ext) -> Result<(), String> {
    let Some((_, since)) = SINCE.iter().find(|(k, _)| *k == what) else { return Ok(()) };
    let Some(since) = since else {
        return Err(format!("{what}: no released Fenix has it yet (it's after {})", FENIX_RELEASES[FENIX_RELEASES.len() - 1]));
    };
    let since = semver::Version::parse(since).expect("SINCE's versions are versions");
    match &ext.floor {
        Some(floor) if *floor < since => Err(format!(
            "{what} needs Fenix {since}, and engine.fenix = {:?} admits {floor}, which can't read it -- fenix = \">={}.{}\" would, or do without it",
            ext.fenix, since.major, since.minor
        )),
        _ => Ok(()),
    }
}

/// `plain`, `line_counts` and `has_change`, as `tutorial.rs` has them: a
/// step's `text` passes when the lines it changes from `before` are there.
fn plain(text: &str) -> String {
    text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end_matches('\n').to_string()
}

fn line_counts(text: &str) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for line in text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn has_change(before: &str, want: &str, text: &str) -> bool {
    let (before, want_counts, now) = (line_counts(before), line_counts(want), line_counts(text));
    let touched: Vec<&str> = before.keys().chain(want_counts.keys()).copied().filter(|l| before.get(l) != want_counts.get(l)).collect();
    if touched.is_empty() {
        return plain(want) == plain(text);
    }
    touched.iter().all(|l| now.get(l) == want_counts.get(l))
}

/// The placeholders in `words` (`{leader}`, `{key:id}`): each held to the
/// floor, and a command of an extension here held to its commands.
fn placeholders(words: &str, ext: &Ext, all: &BTreeMap<String, Ext>, at: &str) -> Result<(), String> {
    if words.contains("{leader}") {
        needs("{leader}", ext).map_err(|e| format!("{at}: {e}"))?;
    }
    let mut rest = words;
    while let Some(start) = rest.find("{key:") {
        needs("{key:...}", ext).map_err(|e| format!("{at}: {e}"))?;
        let after = &rest[start + 5..];
        // Fenix stops at an unclosed one and shows the rest as written.
        let Some(len) = after.find('}') else { return Err(format!("{at}: a {{key: with no }} -- {{key:command.id}}")) };
        let id = after[..len].trim();
        if id.is_empty() || id.contains(char::is_whitespace) {
            return Err(format!("{at}: {{key:{}}} -- a command's id goes there", &after[..len]));
        }
        // A command in an extension's namespace: one it has. Fenix's own
        // aren't listed here, and are taken as written.
        if let Some(owner) = all.values().find(|e| e.commands.iter().any(|c| c.split('.').next() == id.split('.').next())) {
            if !all.values().any(|e| e.commands.contains(id)) {
                return Err(format!("{at}: {{key:{id}}} -- {} has no command {id}, and Fenix would say the id in place of a key", owner.id));
            }
        }
        rest = &after[len + 1..];
    }
    Ok(())
}

/// Every string literal `"<kind>"` in an extension's Rust source, and its
/// path dependencies' under `crates/` (lang-kit opens the packages page).
fn source_has(dir: &Path, kind: &str) -> bool {
    let literal = format!("\"{kind}\"");
    let mut roots = vec![dir.join("src")];
    if let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) {
        if let Ok(table) = toml::from_str::<toml::Table>(&text) {
            for deps in ["dependencies", "dev-dependencies"].iter().filter_map(|k| table.get(*k).and_then(|d| d.as_table())) {
                for path in deps.values().filter_map(|d| d.get("path").and_then(|p| p.as_str())) {
                    roots.push(dir.join(path).join("src"));
                }
            }
        }
    }
    roots.iter().any(|root| {
        crate::files(root).iter().filter(|f| f.extension().is_some_and(|x| x == "rs")).any(|f| std::fs::read_to_string(root.join(f)).is_ok_and(|t| t.contains(&literal)))
    })
}

/// A step's `page` (`closed` false) or `closed` (true): one of Fenix's
/// pages, a tab or fact of one (`page` only), or an extension's,
/// `<id>/<kind>`, that's here and opens it.
fn page_name(name: &str, closed: bool, ext: &Ext, all: &BTreeMap<String, Ext>, at: &str) -> Result<(), String> {
    let what = if closed { "closed" } else { "page" };
    if PAGES.contains(&name) {
        return needs(&format!("page {name}"), ext).map_err(|e| format!("{at}: {e}"));
    }
    let fact = name.strip_prefix("extensions/").is_some_and(|t| EXTENSION_TABS.contains(&t)) || GIT_FACTS.contains(&name) || name.strip_prefix("git/menu/").is_some_and(|m| GIT_MENUS.contains(&m));
    if fact {
        if closed {
            // `closed` looks at the kinds of the pages open: a tab or a
            // fact is never one, so it's never there to see go.
            return Err(format!("{at}: closed = {name:?} is a tab or a fact of a page, not a kind of page: it never passes -- closed = {:?}", name.split('/').next().unwrap_or(name)));
        }
        return Ok(());
    }
    let Some((id, kind)) = name.rsplit_once('/') else {
        return Err(format!("{at}: {what} = {name:?} isn't one of Fenix's pages ({}) nor an extension's, <id>/<kind>", PAGES.join(", ")));
    };
    let Some(owner) = all.get(id) else {
        return Err(format!("{at}: {what} = {name:?} -- {id:?} isn't an extension here, nor is {name:?} one of Fenix's pages"));
    };
    if owner.id != ext.id && !ext.dependencies.contains(id) {
        return Err(format!("{at}: {what} = {name:?} is {id}'s, which {} doesn't depend on: it may not be installed", ext.id));
    }
    if !owner.code {
        return Err(format!("{at}: {what} = {name:?} -- {id} runs no code, so it has no pages"));
    }
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err(format!("{at}: {what} = {name:?} -- a page's kind is lower-case letters, digits, - and _"));
    }
    if !source_has(&owner.dir, kind) {
        return Err(format!("{at}: {what} = {name:?} -- {id}'s source never says \"{kind}\": no page of that kind opens"));
    }
    Ok(())
}

/// Reads and checks one lesson of `ext`'s: what Fenix's `parse` refuses,
/// then what would load and not work. `at` names it in what's said.
pub fn check(text: &str, ext: &Ext, all: &BTreeMap<String, Ext>, at: &str) -> Result<Lesson, String> {
    // As Fenix says it: where in the file, and what.
    let lesson: Lesson = toml::from_str(text).map_err(|e| format!("{at}: {}", e.to_string().trim()))?;
    // Fenix's `tutorial::validate`.
    if lesson.id.trim().is_empty() || lesson.track.trim().is_empty() || lesson.title.trim().is_empty() {
        return Err(format!("{at}: id, track and title are needed"));
    }
    if lesson.steps.is_empty() {
        return Err(format!("{at}: no [[step]]"));
    }
    for (i, step) in lesson.steps.iter().enumerate() {
        let n = i + 1;
        if step.checks() == 0 {
            return Err(format!("{at}: step {n} checks nothing -- Fenix refuses a step that doesn't (give it a page, closed, picker, text, ...)"));
        }
        for (what, value, of) in [("mode", &step.mode, MODES), ("picker", &step.picker, PICKERS), ("terminal", &step.terminal, TERMINALS)] {
            if let Some(v) = value.as_deref().filter(|v| !of.contains(v)) {
                return Err(format!("{at}: step {n}: {what} = \"{v}\" isn't one of {}", of.join(", ")));
            }
        }
        if let Some(w) = &step.windows {
            let w = w.trim();
            let ok = if w.starts_with('+') || w.starts_with('-') { w.parse::<i64>().is_ok() } else { w.parse::<usize>().is_ok() };
            if !ok {
                return Err(format!("{at}: step {n}: windows = \"{w}\" isn't a count (\"2\") or a change (\"+1\")"));
            }
        }
        if step.cursor.is_some_and(|[l, c]| l == 0 || c == 0) || step.line == Some(0) {
            return Err(format!("{at}: step {n}: lines and columns count from 1"));
        }
    }
    // What loads, and wouldn't work. Fenix's `[usize; 2]` takes the first
    // two of a longer list and drops the rest without a word.
    let raw: toml::Table = toml::from_str(text).map_err(|e| format!("{at}: {}", e.message()))?;
    let raw_steps = raw.get("step").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    if let Some(n) = raw_steps.iter().position(|s| s.get("cursor").and_then(|c| c.as_array()).is_some_and(|c| c.len() != 2)) {
        return Err(format!("{at}: step {}: cursor is [line, column], two numbers", n + 1));
    }
    let name = ext.name();
    let topic = lesson.id.strip_prefix(name).and_then(|t| t.strip_prefix('.'));
    if !topic.is_some_and(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')) {
        return Err(format!("{at}: id {:?} -- an extension's lesson is {name}.<topic>, the topic lower-case letters, digits, - and _ (Fenix keeps what's done by it)", lesson.id));
    }
    if BUILT_IN_IDS.contains(&lesson.id.as_str()) {
        return Err(format!("{at}: id {:?} is one of Fenix's own lessons: Fenix refuses it", lesson.id));
    }
    if BUILT_IN_TRACKS.contains(&lesson.track.as_str()) {
        return Err(format!("{at}: track {:?} is Fenix's own: an extension's lessons are a section of their own, after Fenix's", lesson.track));
    }
    if lesson.order == 0 {
        return Err(format!("{at}: order -- its place in the track, from 1 (without one it's 0, and goes before the rest by its title)"));
    }
    if let Some(scheme) = &lesson.scheme {
        needs("scheme", ext).map_err(|e| format!("{at}: {e}"))?;
        if !SCHEMES.contains(&scheme.as_str()) {
            return Err(format!("{at}: scheme = {scheme:?} isn't one of {}: the lesson would never be listed", SCHEMES.join(", ")));
        }
    }
    if let Some(repo) = &lesson.repo {
        // Written as `<dir>/<file>`, with no folder made for it.
        let file = repo.file.trim();
        if file.is_empty() || file.contains(['/', '\\']) || file == "." || file == ".." {
            return Err(format!("{at}: [repo] file = {:?} -- a file's name, in the practice repository's top folder", repo.file));
        }
    }
    placeholders(&lesson.title, ext, all, &format!("{at}: title"))?;
    placeholders(&lesson.about, ext, all, &format!("{at}: about"))?;
    placeholders(&lesson.text, ext, all, &format!("{at}: text"))?;
    let mut before = lesson.text.as_str();
    for (i, step) in lesson.steps.iter().enumerate() {
        let here = format!("{at}: step {}", i + 1);
        if step.say.trim().is_empty() {
            return Err(format!("{here}: say is what to do, and it's empty"));
        }
        placeholders(&step.say, ext, all, &here)?;
        placeholders(&step.hint, ext, all, &here)?;
        if let Some(page) = &step.page {
            page_name(page, false, ext, all, &here)?;
        }
        if let Some(closed) = &step.closed {
            page_name(closed, true, ext, all, &here)?;
        }
        if let Some(want) = &step.text {
            placeholders(want, ext, all, &here)?;
            if has_change(before, want, before) {
                return Err(format!("{here}: its text is the practice text it starts from: it passes before anything is done"));
            }
            before = want;
        }
    }
    Ok(lesson)
}

/// Every extension here, as its lessons are checked against it.
pub fn extensions(root: &Path) -> Result<BTreeMap<String, Ext>, String> {
    let mut out = BTreeMap::new();
    let dirs = std::fs::read_dir(root.join("extensions")).map_err(|e| format!("extensions/: {e}"))?;
    for dir in dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let m = crate::manifest(&dir)?;
        let path = dir.join("extension.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
        let commands = table
            .get("contributes")
            .and_then(|c| c.get("commands"))
            .and_then(|c| c.as_array())
            .map(|a| a.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str()).map(str::to_string)).collect())
            .unwrap_or_default();
        let ext = Ext {
            floor: floor(&m.engine.fenix),
            fenix: m.engine.fenix.clone(),
            dependencies: m.dependencies.keys().cloned().collect(),
            commands,
            code: m.extension.runtime == "wasm" || m.extension.runtime == "lua",
            dir,
            id: m.extension.id.clone(),
        };
        out.insert(m.extension.id, ext);
    }
    Ok(out)
}

/// `cargo xtask validate`'s part: every lesson there is, each on its own
/// and together -- ids each their own, and one lesson to a place in a
/// track.
pub fn validate_all(root: &Path) -> Result<(), String> {
    let all = extensions(root)?;
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut places: BTreeMap<(String, u32), String> = BTreeMap::new();
    let mut count = 0;
    for ext in all.values() {
        let folder = ext.dir.join("tutorial");
        if !folder.is_dir() {
            continue;
        }
        for rel in crate::files(&folder) {
            let path = folder.join(&rel);
            let at = path.display().to_string();
            // Fenix reads the folder's own *.toml, and nothing else.
            if rel.components().count() > 1 || rel.extension().is_none_or(|x| x != "toml") {
                return Err(format!("{at}: Fenix reads only tutorial/*.toml -- this would be packed and never read"));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{at}: {e}"))?;
            let lesson = check(&text, ext, &all, &at)?;
            if let Some(other) = ids.insert(lesson.id.clone(), at.clone()) {
                return Err(format!("{at}: {other} is lesson {} already -- Fenix loads the first and refuses the other", lesson.id));
            }
            if let Some(other) = places.insert((lesson.track.clone(), lesson.order), at.clone()) {
                return Err(format!("{at}: {other} is order {} of track {:?} already -- give each its own place", lesson.order, lesson.track));
            }
            println!("ok  {}/tutorial/{}: {}, {} step(s)", ext.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), rel.display(), lesson.id, lesson.steps.len());
            count += 1;
        }
    }
    println!("{count} lesson(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temp folder, gone when
    /// dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            // Tests run side by side: each its own, whatever it's called.
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("xtask-lessons-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `acme.dusk` on `fenix`, with a page of kind `contrast` in its
    /// source and a command, and `acme.other`, a theme.
    fn world(fenix: &str) -> (Temp, BTreeMap<String, Ext>) {
        let t = Temp::new(&fenix.replace(|c: char| !c.is_ascii_alphanumeric(), ""));
        std::fs::create_dir_all(t.0.join("dusk/src")).unwrap();
        std::fs::write(t.0.join("dusk/src/lib.rs"), "const KIND: &str = \"contrast\";\n").unwrap();
        let dusk = Ext {
            id: "acme.dusk".into(),
            dir: t.0.join("dusk"),
            fenix: fenix.into(),
            floor: floor(fenix),
            commands: BTreeSet::from(["dusk.contrast".to_string()]),
            code: true,
            ..Ext::default()
        };
        let other = Ext { id: "acme.other".into(), dir: t.0.join("other"), fenix: fenix.into(), floor: floor(fenix), ..Ext::default() };
        let all = BTreeMap::from([("acme.dusk".to_string(), dusk), ("acme.other".to_string(), other)]);
        (t, all)
    }

    const GOOD: &str = r#"
id = "dusk.contrast"
track = "Dusk"
order = 1
title = "Checking contrast"
about = "What it teaches."
text = """
SPC d c   the contrast page
note: here
"""

[[step]]
say = "SPC d c: the contrast page."
hint = "SPC d, then c."
page = "acme.dusk/contrast"

[[step]]
say = "q closes it."
closed = "acme.dusk/contrast"

[[step]]
say = "Change here to there."
text = """
SPC d c   the contrast page
note: there
"""
"#;

    fn try_lesson(fenix: &str, text: &str) -> Result<Lesson, String> {
        let (_t, all) = world(fenix);
        check(text, &all["acme.dusk"], &all, "dusk.toml")
    }

    fn refused(text: &str) -> String {
        try_lesson(">=1.0, <2", text).unwrap_err()
    }

    #[test]
    fn a_good_lesson_reads() {
        let lesson = try_lesson(">=1.0, <2", GOOD).unwrap();
        assert_eq!((lesson.id.as_str(), lesson.steps.len()), ("dusk.contrast", 3));
    }

    #[test]
    fn what_fenix_refuses_at_load_is_refused() {
        assert!(refused(&GOOD.replace("say = \"q closes it.\"", "say = \"q closes it.\"\nclosd = \"x\"")).contains("closd"), "an unknown field");
        assert!(refused(&GOOD.replace("order = 1", "order = 1\nfile = \"x.toml\"")).contains("file"), "Fenix's own, never read from the file");
        assert!(refused(&GOOD.replace("order = 1", "order = \"1\"")).contains("order"), "mistyped");
        assert!(refused(&GOOD.replace("order = 1", "order = -1")).contains("order"));
        assert!(refused(&GOOD.replace("title = \"Checking contrast\"\n", "")).contains("title"), "a field it needs");
        assert!(refused(&GOOD.replace("title = \"Checking contrast\"", "title = \" \"")).contains("id, track and title"));
        let no_steps = GOOD.split("[[step]]").next().unwrap();
        assert!(refused(no_steps).contains("step"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\n")).contains("checks nothing"));
        assert!(refused(&format!("{no_steps}[[step]]\nhint = \"do\"\npicker = \"any\"\n")).contains("say"), "say is needed");
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\nmode = \"sideways\"\n")).contains("isn't one of"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\npicker = \"other\"\n")).contains("isn't one of"), "a picker Fenix names only to itself");
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\nterminal = \"open\"\n")).contains("isn't one of"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\nwindows = \"two\"\n")).contains("isn't a count"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\ncursor = [0, 1]\n")).contains("from 1"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\ncursor = [1]\n")).contains("cursor"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\ncursor = [1, 2, 3]\n")).contains("two numbers"), "Fenix reads it, and drops the 3");
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\nline = 0\n")).contains("from 1"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\nmoved = \"yes\"\n")).contains("moved"));
        assert!(refused(&format!("{no_steps}[[step]]\nsay = \"do\"\npicker = \"any\"\n[repo]\nfile = \"a.txt\"\n")).contains("committed"));
    }

    #[test]
    fn ids_tracks_and_orders_are_its_own() {
        assert!(refused(&GOOD.replace("id = \"dusk.contrast\"", "id = \"contrast\"")).contains("dusk.<topic>"));
        assert!(refused(&GOOD.replace("id = \"dusk.contrast\"", "id = \"acme.dusk.contrast\"")).contains("dusk.<topic>"));
        assert!(refused(&GOOD.replace("id = \"dusk.contrast\"", "id = \"dusk.Contrast\"")).contains("lower-case"));
        assert!(refused(&GOOD.replace("track = \"Dusk\"", "track = \"Fenix\"")).contains("Fenix's own"));
        assert!(refused(&GOOD.replace("order = 1\n", "")).contains("order"));
        let (_t, mut all) = world(">=1.0");
        all.get_mut("acme.dusk").unwrap().id = "acme.git".into();
        let git = all["acme.dusk"].clone();
        assert!(check(&GOOD.replace("id = \"dusk.contrast\"", "id = \"git.basics\""), &git, &all, "x").unwrap_err().contains("Fenix's own lessons"));
    }

    #[test]
    fn pages_are_fenixs_or_an_extensions_here_that_opens_them() {
        let page = |p: &str| GOOD.replace("page = \"acme.dusk/contrast\"", &format!("page = \"{p}\""));
        for ok in ["doctor", "settings", "extensions/browse", "git/staged", "git/menu/commit"] {
            try_lesson(">=1.0", &page(ok)).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        assert!(refused(&page("dashboard")).contains("isn't one of Fenix's pages"));
        assert!(refused(&page("extensions/shop")).contains("isn't an extension here"));
        assert!(refused(&page("acme.nowhere/contrast")).contains("isn't an extension here"));
        assert!(refused(&page("acme.dusk/contrasts")).contains("never says \"contrasts\""), "a kind it never opens");
        assert!(refused(&page("acme.dusk/Contrast")).contains("lower-case"));
        assert!(refused(&page("acme.other/x")).contains("doesn't depend on"));
        let (_t, mut all) = world(">=1.0");
        all.get_mut("acme.dusk").unwrap().dependencies.insert("acme.other".into());
        assert!(check(&page("acme.other/x"), &all["acme.dusk"], &all, "x").unwrap_err().contains("runs no code"));
        let closed = |p: &str| GOOD.replace("closed = \"acme.dusk/contrast\"", &format!("closed = \"{p}\""));
        assert!(refused(&closed("extensions/browse")).contains("never passes"), "closed looks at kinds only");
        assert!(refused(&closed("git/staged")).contains("never passes"));
        try_lesson(">=1.0", &closed("git")).unwrap();
    }

    #[test]
    fn what_needs_a_newer_fenix_than_the_floor_is_refused() {
        let scheme = GOOD.replace("order = 1", "order = 1\nscheme = \"standard\"");
        let why = try_lesson(">=1.0, <2", &scheme).unwrap_err();
        assert!(why.contains("scheme needs Fenix 1.5.0") && why.contains("fenix = \">=1.5\""), "{why}");
        try_lesson(">=1.5, <2", &scheme).unwrap();
        try_lesson("^1.5.1", &scheme).unwrap();
        assert!(try_lesson(">=1.5", &scheme.replace("\"standard\"", "\"emacs\"")).unwrap_err().contains("never be listed"));
        let leader = GOOD.replace("hint = \"SPC d, then c.\"", "hint = \"{leader}, d, c.\"");
        assert!(try_lesson(">=1.3", &leader).unwrap_err().contains("{leader} needs Fenix 1.5.0"));
        try_lesson(">=1.5", &leader).unwrap();
        let key = GOOD.replace("say = \"SPC d c: the contrast page.\"", "say = \"{key:dusk.contrast}: the contrast page.\"");
        assert!(try_lesson(">=1.0", &key).unwrap_err().contains("{key:...} needs Fenix 1.5.0"));
        try_lesson(">=1.5", &key).unwrap();
        try_lesson(">=1.5", &key.replace("dusk.contrast}", "git.open}")).unwrap();
        assert!(try_lesson(">=1.5", &key.replace("dusk.contrast}", "dusk.contrasts}")).unwrap_err().contains("no command dusk.contrasts"));
        assert!(try_lesson(">=1.5", &key.replace("dusk.contrast}", "dusk.contrast")).unwrap_err().contains("no }"));
        assert!(try_lesson(">=1.5", &key.replace("{key:dusk.contrast}", "{key:}")).unwrap_err().contains("a command's id"));
        let text = GOOD.replace("note: there", "note: {leader}");
        assert!(try_lesson(">=1.0", &text).unwrap_err().contains("{leader} needs"), "a step's text is said too");
        let notifications = GOOD.replace("page = \"acme.dusk/contrast\"", "page = \"notifications\"");
        assert!(try_lesson(">=1.0", &notifications).unwrap_err().contains("page notifications needs Fenix 1.2.0"));
        try_lesson(">=1.2", &notifications).unwrap();
        let import = GOOD.replace("page = \"acme.dusk/contrast\"", "page = \"import-settings\"");
        assert!(try_lesson(">=1.5", &import).unwrap_err().contains("no released Fenix"));
    }

    #[test]
    fn a_step_that_would_pass_at_once_or_a_repo_without_a_place_is_refused() {
        let same = GOOD.replace("note: there", "note: here");
        assert!(refused(&same).contains("passes before anything is done"));
        let blank = GOOD.replace("note: there\n", "note: here\n\n\n");
        assert!(refused(&blank).contains("passes before anything is done"), "blank lines aside");
        let repo = format!("{GOOD}\n[repo]\nfile = \"notes/a.txt\"\ncommitted = \"x\"\n");
        assert!(refused(&repo).contains("[repo] file"));
        try_lesson(">=1.0", &repo.replace("notes/a.txt", "a.txt")).unwrap();
    }

    /// The folder: only its own *.toml, an id once, a place in a track
    /// once -- read from extensions/ as `validate` reads it.
    #[test]
    fn the_repository_s_lessons_are_each_their_own() {
        let t = Temp::new("repo");
        let ext = |name: &str| {
            let dir = t.0.join("extensions").join(name);
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::create_dir_all(dir.join("tutorial")).unwrap();
            std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
            std::fs::write(dir.join("src/lib.rs"), "\"contrast\"").unwrap();
            std::fs::write(
                dir.join("extension.toml"),
                format!("[extension]\nid = \"acme.{name}\"\nname = \"N\"\nversion = \"1.0.0\"\nruntime = \"wasm\"\n\n[engine]\nfenix = \">=1.0\"\napi = \"1.0\"\n"),
            )
            .unwrap();
            dir
        };
        let dusk = ext("dusk");
        std::fs::write(dusk.join("tutorial/a.toml"), GOOD).unwrap();
        validate_all(&t.0).unwrap();
        std::fs::write(dusk.join("tutorial/notes.md"), "x").unwrap();
        assert!(validate_all(&t.0).unwrap_err().contains("only tutorial/*.toml"));
        std::fs::remove_file(dusk.join("tutorial/notes.md")).unwrap();
        std::fs::write(dusk.join("tutorial/b.toml"), GOOD.replace("order = 1", "order = 2")).unwrap();
        assert!(validate_all(&t.0).unwrap_err().contains("is lesson dusk.contrast already"));
        std::fs::write(dusk.join("tutorial/b.toml"), GOOD.replace("dusk.contrast\"", "dusk.again\"")).unwrap();
        assert!(validate_all(&t.0).unwrap_err().contains("order 1 of track \"Dusk\" already"));
        std::fs::write(dusk.join("tutorial/b.toml"), GOOD.replace("dusk.contrast\"", "dusk.again\"").replace("order = 1", "order = 2")).unwrap();
        validate_all(&t.0).unwrap();
        // Another extension's page, without depending on it.
        let dawn = ext("dawn");
        std::fs::write(dawn.join("tutorial/a.toml"), GOOD.replace("dusk.contrast\"", "dawn.contrast\"").replace("track = \"Dusk\"", "track = \"Dawn\"")).unwrap();
        assert!(validate_all(&t.0).unwrap_err().contains("doesn't depend on"));
    }

    #[test]
    fn the_floor_is_the_first_release_the_range_admits() {
        assert_eq!(floor(">=1.0, <2"), Some(semver::Version::new(1, 0, 0)));
        assert_eq!(floor(">=1.3"), Some(semver::Version::new(1, 3, 0)));
        assert_eq!(floor("^1.5"), Some(semver::Version::new(1, 5, 0)));
        assert_eq!(floor(">=1.4.1"), Some(semver::Version::new(1, 5, 0)), "no 1.4.1: the next there is");
        assert_eq!(floor(">=2"), None);
    }

    /// The store's lessons, as they are: Review's, and each since.
    #[test]
    fn every_lesson_here_passes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        validate_all(&root).unwrap();
    }
}
