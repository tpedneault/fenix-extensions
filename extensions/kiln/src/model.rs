//! What kiln's JSON says, as types: `kiln --version`, `list`, `status`,
//! `next`, `check` and `show`, each read from the text kiln printed.
//!
//! Read tolerantly, strictly versioned. Unknown fields are ignored, and
//! only what names an item (`id`, `type`, `title`, `status`) is required;
//! everything else defaults when it's missing, so a kiln that adds a field
//! -- kiln's own promise for arrays, and for objects within a `kiln_json`
//! -- never breaks a read. What kiln can't add without saying so is a
//! change of shape: an object that carries `kiln_json` (`status`, `show`)
//! is refused unless it is [`KILN_JSON`], and a missing one is refused
//! too, since every kiln this reads (1.0 and later) writes it. Nothing
//! guesses at a shape it wasn't built for. `list`, `next` and `check` are
//! arrays, which kiln never versions (CLI.md, "JSON output").
//!
//! Every failure is one line saying what couldn't be read and why, for the
//! page to show as it is. Nothing here panics on kiln's output: truncated
//! JSON, a wrong type or a number out of range is an `Err`.
//!
//! What it deliberately doesn't do: run kiln (that's `kiln.rs`), decide
//! what to show, or type the answers only later pages read (`trace`,
//! `impact`, `diff`, `export --dry-run`): those come raw through
//! `Project::query`, and their pages type them. Recorded output for all of
//! them is in `tests/fixtures/out/`.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::Deserialize;

/// The shape version this reads (`kiln_json`): kiln raises it only when a
/// key is removed, renamed or changes type.
pub const KILN_JSON: u64 = 1;

/// The oldest kiln this reads: 1.0.0, the first with `kiln_json`, `next`,
/// `status` with `in_progress`, and `show`'s structured JSON.
pub const MIN_VERSION: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};

/// kiln's version, from `kiln --version` (`kiln 1.0.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// What `kiln --version` printed: `kiln 1.0.0`, maybe with a suffix
    /// (`1.1.0-dev`, `+abc`) that's ignored -- a prerelease of 1.1 reads as
    /// 1.1.0, which is what it's on its way to. Anything else is an error
    /// quoting the line, so another program called `kiln` says so.
    pub fn parse(text: &str) -> Result<Version, String> {
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        let fail = || {
            format!(
                "`kiln --version` said {:?}, not `kiln <version>`",
                clip(line, 80)
            )
        };
        let rest = line.strip_prefix("kiln ").ok_or_else(fail)?.trim();
        let core = rest.split(['-', '+', ' ']).next().unwrap_or("");
        let mut parts = core.split('.');
        let mut number = || -> Result<u32, String> {
            parts.next().and_then(|p| p.parse().ok()).ok_or_else(fail)
        };
        let v = Version {
            major: number()?,
            minor: number()?,
            patch: number()?,
        };
        Ok(v)
    }

    /// Whether this extension reads this kiln's output.
    pub fn supported(self) -> bool {
        self >= MIN_VERSION
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// -- list --------------------------------------------------------------------

/// One item, as `kiln list --format json` gives it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Item {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub jira_keys: Vec<String>,
    #[serde(default)]
    pub owner: Option<String>,
    /// `YYYY-MM-DD`, as written.
    #[serde(default)]
    pub due: Option<String>,
    /// Whether its status counts as done for its type.
    #[serde(default)]
    pub done: bool,
    /// Whether its status counts as closed (dropped, obsolete...).
    #[serde(default)]
    pub closed: bool,
    /// Outgoing links only: role -> the IDs as written. The inverse names
    /// (`implemented by`) are these read from the other end.
    #[serde(default)]
    pub links: BTreeMap<String, Vec<String>>,
    /// Relative to the project's top, `/` between folders.
    #[serde(default)]
    pub path: String,
    /// The line of its `id` (from 1).
    #[serde(default)]
    pub line: u32,
    /// Only with `list --body`.
    #[serde(default)]
    pub body: Option<String>,
}

/// `kiln list --format json` (with or without `--body`).
pub fn list(text: &str) -> Result<Vec<Item>, String> {
    array("kiln list", text)
}

// -- status ------------------------------------------------------------------

/// An item another answer names: `{id, title, status}`, or `{id, missing:
/// true}` for an ID nothing has.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ref {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    /// The ID is written somewhere but no item has it.
    #[serde(default)]
    pub missing: bool,
}

/// A milestone as `status` heads its group: with its due date.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Milestone {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub due: Option<String>,
}

/// A piece of open work in `status`: ready or in progress.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Work {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Who claimed it, if anyone.
    #[serde(default)]
    pub owner: Option<String>,
}

/// Work waiting: on open items it `depends-on`, or blocked by its status.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Waiting {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub owner: Option<String>,
    /// Its status is `blocked`.
    #[serde(default)]
    pub blocked: bool,
    /// What it depends on that's still open (empty when only blocked).
    #[serde(default)]
    pub waits_on: Vec<Ref>,
}

/// One group of `status`: a milestone's work, or the work in no milestone
/// (`milestone` none), which kiln lists last.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub milestone: Option<Milestone>,
    /// Its own work done, of `total`: the progress every Kiln view gives
    /// (ADR-020).
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub total: u32,
    /// Past its first status, waiting on nothing.
    #[serde(default)]
    pub in_progress: Vec<Work>,
    /// At its first status, waiting on nothing: what `kiln next` offers.
    #[serde(default)]
    pub ready: Vec<Work>,
    #[serde(default)]
    pub waiting: Vec<Waiting>,
}

/// How many findings `kiln check` has, as `status` counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Counts {
    #[serde(default)]
    pub errors: u32,
    #[serde(default)]
    pub warnings: u32,
}

impl Counts {
    /// `1 error, 2 warnings`; `None` when there are neither.
    pub fn said(&self) -> Option<String> {
        let (e, w) = (self.errors as usize, self.warnings as usize);
        match (e, w) {
            (0, 0) => None,
            (0, w) => Some(count(w, "warning")),
            (e, 0) => Some(count(e, "error")),
            (e, w) => Some(format!("{}, {}", count(e, "error"), count(w, "warning"))),
        }
    }
}

/// `kiln status --format json`: the plan by milestone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    /// The project's `name` in `kiln.toml`.
    pub project: String,
    /// Open milestones, and those with open work, in kiln's order; the
    /// work in no milestone last, when there's any.
    #[serde(default)]
    pub milestones: Vec<Group>,
    #[serde(default)]
    pub findings: Counts,
}

/// `kiln status --format json`, its `kiln_json` checked.
pub fn status(text: &str) -> Result<Status, String> {
    object("kiln status", text)
}

// -- next ----------------------------------------------------------------------

/// One piece of work ready to start, as `kiln next --format json` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Next {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: u32,
    /// The milestone it counts in (its parent's, for a sub-task).
    #[serde(default)]
    pub milestone: Option<Ref>,
}

/// `kiln next --format json`: `[]` when nothing is ready (kiln says why on
/// stderr).
pub fn next(text: &str) -> Result<Vec<Next>, String> {
    array("kiln next", text)
}

// -- check ---------------------------------------------------------------------

/// How serious a finding is. A severity this doesn't know (a later kiln's
/// `info`) reads as [`Severity::Other`], counted as neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    #[serde(other)]
    Other,
}

/// One finding of `kiln check --format json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Finding {
    /// `K102`.
    pub code: String,
    pub severity: Severity,
    pub message: String,
    /// Relative to the project's top; `kiln.toml` for the configuration.
    #[serde(default)]
    pub path: String,
    /// From 1; 0 when kiln gives none.
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
}

/// `kiln check --format json`. kiln exits 1 when a finding is an error,
/// with this same JSON on stdout: the exit code isn't the answer.
pub fn check(text: &str) -> Result<Vec<Finding>, String> {
    array("kiln check", text)
}

/// `1 item`, `16 items`: `n` and `what`, plural unless `n` is 1.
pub fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// Errors and warnings among `findings`.
pub fn counts(findings: &[Finding]) -> Counts {
    let n = |s| findings.iter().filter(|f| f.severity == s).count() as u32;
    Counts {
        errors: n(Severity::Error),
        warnings: n(Severity::Warning),
    }
}

// -- show ----------------------------------------------------------------------

/// Where a link comes from (`inbound`): an item, or a code reference.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Source {
    /// A code reference (`// verifies: REQ-001`): its file and line.
    Code {
        path: String,
        line: u32,
    },
    Item(Ref),
}

/// A milestone's progress: its own work done, of all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
}

/// `kiln show ID --format json`: one item with everything kiln computes
/// about it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Shown {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: u32,
    /// Other files with the same ID (a K101 duplicate), by path.
    #[serde(default)]
    pub also_in: Vec<String>,
    #[serde(default)]
    pub parent: Option<Ref>,
    #[serde(default)]
    pub children: Vec<Ref>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Its Jira keys (`list`'s `jira_keys`).
    #[serde(default)]
    pub jira: Vec<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    /// Its requirement level, from 1, when `kiln.toml` has levels.
    #[serde(default)]
    pub level: Option<u32>,
    #[serde(default)]
    pub derived: bool,
    /// Outgoing links: role -> the items, as written.
    #[serde(default)]
    pub links: BTreeMap<String, Vec<Ref>>,
    /// Incoming links, by the role's own name (`implements`, not
    /// `implemented by`): role -> items and code references.
    #[serde(default)]
    pub inbound: BTreeMap<String, Vec<Source>>,
    #[serde(default)]
    pub mentioned_by: Vec<Ref>,
    /// Coverage role -> whether it's covered, for the roles that can point
    /// at its type.
    #[serde(default)]
    pub coverage: BTreeMap<String, bool>,
    /// A milestone's; none for anything else, and for a milestone with no
    /// work at all.
    #[serde(default)]
    pub progress: Option<Progress>,
    /// Its Markdown after the front matter, as written.
    #[serde(default)]
    pub body: String,
}

/// `kiln show ID --format json`, its `kiln_json` checked.
pub fn show(text: &str) -> Result<Shown, String> {
    object("kiln show", text)
}

// -- Reading -------------------------------------------------------------------

/// `text` as `T`, an object that carries `kiln_json`: checked first, so a
/// shape this wasn't built for is said as that, not as the field it trips
/// on.
pub fn object<T: DeserializeOwned>(what: &str, text: &str) -> Result<T, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| unreadable(what, &e))?;
    let Some(map) = value.as_object() else {
        return Err(format!(
            "{what}: expected a JSON object, got {}",
            kind_of(&value)
        ));
    };
    if let Some(why) = shape_of(what, map) {
        return Err(why);
    }
    T::deserialize(value).map_err(|e| unreadable(what, &e))
}

/// Why `text`, an object that should carry `kiln_json`, is of a shape this
/// doesn't read -- none when it's [`KILN_JSON`], or when `text` isn't an
/// object at all (that's for the read itself to say). Apart from reading,
/// so a caller can tell "another kiln's shape", which turns the pages off,
/// from "an answer that didn't parse".
pub fn other_shape(what: &str, text: &str) -> Option<String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => shape_of(what, &map),
        _ => None,
    }
}

fn shape_of(what: &str, map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    match map.get("kiln_json") {
        Some(v) if v.as_u64() == Some(KILN_JSON) => None,
        Some(v) => Some(format!("{what}: its JSON is shape {v} (kiln_json), and this extension reads shape {KILN_JSON}: update the extension")),
        None => Some(format!("{what}: its JSON has no kiln_json, so its shape is unknown")),
    }
}

/// `text` as a JSON array of `T`. kiln never versions arrays; their
/// elements only gain fields.
pub fn array<T: DeserializeOwned>(what: &str, text: &str) -> Result<Vec<T>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| unreadable(what, &e))?;
    if !value.is_array() {
        return Err(format!(
            "{what}: expected a JSON array, got {}",
            kind_of(&value)
        ));
    }
    Vec::<T>::deserialize(value).map_err(|e| unreadable(what, &e))
}

fn unreadable(what: &str, e: &serde_json::Error) -> String {
    format!("{what}: kiln's JSON can't be read: {e}")
}

fn kind_of(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// `s` with every control character -- a newline, a tab, an escape
/// sequence's ESC and BEL -- and every bidirectional override (which could
/// make a line read as another) made a space, the ends trimmed, and cut to
/// `n` characters. Inner spacing stays: kiln aligns its plan's lines with
/// it. For text that is kiln's but holds other people's words (a ticket's
/// summary in a plan line).
pub fn printable(s: &str, n: usize) -> String {
    let spaced: String = s
        .chars()
        .map(|c| {
            let bidi = matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}')
                || ('\u{202A}'..='\u{202E}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c);
            if c.is_control() || bidi {
                ' '
            } else {
                c
            }
        })
        .collect();
    clip(spaced.trim(), n)
}

/// Other people's writing (a Jira status, a Confluence comment's author)
/// as one line of plain text for a cell: [`printable`], and runs of
/// whitespace made one. Nothing in it is interpreted: a cell shows text.
pub fn plain(s: &str, n: usize) -> String {
    let squeezed = printable(s, usize::MAX)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    clip(&squeezed, n)
}

/// `s` cut to `n` characters, `…` saying so.
pub fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A recorded file, by its path under `tests/fixtures/out/`.
    pub(crate) fn fixture(path: &str) -> String {
        let at = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/out/");
        std::fs::read_to_string(format!("{at}{path}")).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn the_version_is_read() {
        assert_eq!(
            Version::parse(&fixture("version.txt")),
            Ok(Version {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("kiln 1.2.3-dev+abc\n"),
            Ok(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert!(Version::parse("kiln 1.0.0").unwrap().supported());
        assert!(!Version::parse("kiln 0.9.12").unwrap().supported());
        assert!(Version::parse("kiln 2.0.0").unwrap().supported());
    }

    #[test]
    fn another_program_is_no_version() {
        assert_eq!(
            Version::parse("Kiln Pottery Tool v3"),
            Err("`kiln --version` said \"Kiln Pottery Tool v3\", not `kiln <version>`".into())
        );
        assert!(Version::parse("").is_err());
        assert!(Version::parse("kiln 1.x").is_err());
        assert!(Version::parse("kiln 99999999999.0.0").is_err());
    }

    #[test]
    fn the_demos_list_has_every_item() {
        let items = list(&fixture("demo/list.json")).unwrap();
        assert_eq!(items.len(), 16);
        let task2 = items.iter().find(|i| i.id == "TASK-002").unwrap();
        assert_eq!(
            (
                task2.kind.as_str(),
                task2.status.as_str(),
                task2.owner.as_deref()
            ),
            ("task", "doing", Some("ana"))
        );
        assert_eq!(task2.links["part-of"], ["MS-001"]);
        assert_eq!(
            task2.path,
            "design/tasks/TASK-002-write-readings-to-sqlite.md"
        );
        assert_eq!(task2.line, 2);
        assert_eq!(task2.body, None);
        let sub = items.iter().find(|i| i.id == "TASK-007").unwrap();
        assert_eq!(sub.parent.as_deref(), Some("TASK-002"));
        assert!(items.iter().find(|i| i.id == "TASK-001").unwrap().done);
    }

    #[test]
    fn a_list_with_bodies_has_them() {
        let items = list(&fixture("demo/list-body.json")).unwrap();
        let req = items.iter().find(|i| i.id == "REQ-001").unwrap();
        assert_eq!(
            req.body.as_deref(),
            Some("\nThe logger stores one reading a second from each sensor.\n")
        );
    }

    #[test]
    fn kilns_own_list_reads() {
        let items = list(&fixture("own/list.json")).unwrap();
        assert!(items.len() > 90, "{}", items.len());
        assert!(items.iter().all(|i| !i.id.is_empty() && !i.path.is_empty()));
        assert!(list(&fixture("own/list-body.json"))
            .unwrap()
            .iter()
            .all(|i| i.body.is_some()));
    }

    #[test]
    fn the_demos_status_has_each_state_of_work() {
        let s = status(&fixture("demo/status.json")).unwrap();
        assert_eq!(s.project, "Demo");
        assert_eq!(
            s.findings,
            Counts {
                errors: 1,
                warnings: 2
            }
        );
        assert_eq!(s.milestones.len(), 3);
        let first = &s.milestones[0];
        assert_eq!(
            first
                .milestone
                .as_ref()
                .map(|m| (m.id.as_str(), m.due.as_deref())),
            Some(("MS-001", Some("2030-06-30")))
        );
        assert_eq!((first.done, first.total), (1, 5));
        assert_eq!(
            first
                .in_progress
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            ["TASK-002"]
        );
        assert_eq!(
            first
                .ready
                .iter()
                .map(|w| (w.id.as_str(), w.owner.as_deref()))
                .collect::<Vec<_>>(),
            [("TASK-003", Some("bo")), ("TASK-007", None)]
        );
        assert_eq!(first.waiting[0].waits_on[0].id, "TASK-003");
        assert!(!first.waiting[0].blocked);
        let second = &s.milestones[1];
        assert!(second.waiting[0].blocked && second.waiting[0].waits_on.is_empty());
        // The work in no milestone comes last, with no milestone.
        let none = &s.milestones[2];
        assert_eq!(
            (none.milestone.as_ref(), none.ready[0].id.as_str()),
            (None, "TASK-008")
        );
    }

    #[test]
    fn kilns_own_status_has_no_open_milestone() {
        let s = status(&fixture("own/status.json")).unwrap();
        assert_eq!(
            (s.project.as_str(), s.milestones.len(), s.findings),
            ("Kiln", 0, Counts::default())
        );
    }

    #[test]
    fn next_reads_with_milestones() {
        let n = next(&fixture("demo/next.json")).unwrap();
        assert_eq!(
            n.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            ["TASK-003", "TASK-007", "TASK-006", "TASK-008"]
        );
        assert_eq!(
            n[0].milestone.as_ref().map(|m| m.id.as_str()),
            Some("MS-001")
        );
        assert_eq!(n[3].milestone, None);
        assert_eq!(next(&fixture("demo/next-unowned.json")).unwrap().len(), 3);
        assert!(next(&fixture("own/next.json")).unwrap().is_empty());
    }

    #[test]
    fn check_reads_its_findings() {
        let f = check(&fixture("demo/check.json")).unwrap();
        assert_eq!(
            counts(&f),
            Counts {
                errors: 1,
                warnings: 2
            }
        );
        let error = f.iter().find(|f| f.severity == Severity::Error).unwrap();
        assert_eq!(
            (error.code.as_str(), error.line, error.column),
            ("K102", 11, 23)
        );
        assert_eq!(error.message, "reference REQ-099 doesn't exist");
        assert!(check(&fixture("own/check.json")).unwrap().is_empty());
    }

    #[test]
    fn an_unknown_severity_counts_as_neither() {
        let f = check(r#"[{"code":"K999","severity":"info","message":"hi","path":"a.md","line":1,"column":1,"new":true}]"#).unwrap();
        assert_eq!(
            (f[0].severity, counts(&f)),
            (Severity::Other, Counts::default())
        );
    }

    #[test]
    fn show_reads_every_kind_recorded() {
        for name in [
            "REQ-001", "REQ-002", "ADR-001", "MS-001", "TASK-002", "TASK-004", "TEST-001",
            "TASK-006",
        ] {
            let s = show(&fixture(&format!("demo/show-{name}.json")))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(s.id, name);
        }
        for name in ["REQ-001", "ADR-001", "MS-002", "TASK-001"] {
            assert_eq!(
                show(&fixture(&format!("own/show-{name}.json"))).unwrap().id,
                name
            );
        }
        let ms = show(&fixture("demo/show-MS-001.json")).unwrap();
        assert_eq!(ms.progress, Some(Progress { done: 1, total: 5 }));
        assert_eq!(ms.inbound["part-of"].len(), 4);
        let req = show(&fixture("demo/show-REQ-001.json")).unwrap();
        assert_eq!(req.children[0].id, "REQ-002");
        assert_eq!(req.coverage.get("verifies"), Some(&true));
        let child = show(&fixture("demo/show-REQ-002.json")).unwrap();
        assert_eq!(child.parent.map(|p| p.id), Some("REQ-001".into()));
    }

    #[test]
    fn show_tells_code_references_from_items() {
        let own = show(&fixture("own/show-REQ-001.json")).unwrap();
        assert!(own.inbound.values().flatten().any(
            |s| matches!(s, Source::Code { path, line } if path.starts_with("crates/") && *line > 0)
        ));
        let s = show(r#"{"kiln_json":1,"id":"TASK-001","type":"task","title":"T","status":"todo","links":{"implements":[{"id":"REQ-009","missing":true}]},"inbound":{"verifies":[{"id":"TEST-001","title":"t","status":"ready"},{"path":"src/a.rs","line":4}]}}"#).unwrap();
        assert!(s.links["implements"][0].missing);
        assert_eq!(
            s.inbound["verifies"],
            [
                Source::Item(Ref {
                    id: "TEST-001".into(),
                    title: "t".into(),
                    status: "ready".into(),
                    missing: false
                }),
                Source::Code {
                    path: "src/a.rs".into(),
                    line: 4
                }
            ]
        );
    }

    #[test]
    fn another_shape_is_refused_not_guessed() {
        let e = status(r#"{"kiln_json":2,"project":"X","milestones":[]}"#).unwrap_err();
        assert_eq!(e, "kiln status: its JSON is shape 2 (kiln_json), and this extension reads shape 1: update the extension");
        assert_eq!(
            show(r#"{"id":"A-001","type":"t","title":"t","status":"s"}"#).unwrap_err(),
            "kiln show: its JSON has no kiln_json, so its shape is unknown"
        );
        assert_eq!(
            status("[]").unwrap_err(),
            "kiln status: expected a JSON object, got an array"
        );
        assert_eq!(
            list("{}").unwrap_err(),
            "kiln list: expected a JSON array, got an object"
        );
    }

    #[test]
    fn another_shape_is_told_apart_from_bad_json() {
        assert_eq!(
            other_shape("kiln status", &fixture("demo/status.json")),
            None
        );
        assert_eq!(other_shape("kiln status", "{\"kiln_json\": 2}").as_deref(), Some("kiln status: its JSON is shape 2 (kiln_json), and this extension reads shape 1: update the extension"));
        assert_eq!(other_shape("kiln status", "{\"kiln_j"), None);
        assert_eq!(other_shape("kiln status", "[]"), None);
    }

    #[test]
    fn fields_kiln_adds_later_are_ignored() {
        let items = list(r#"[{"id":"REQ-001","type":"requirement","title":"T","status":"draft","colour":"red","links":{},"later":{"x":[1]}}]"#).unwrap();
        assert_eq!(
            (
                items[0].id.as_str(),
                items[0].path.as_str(),
                items[0].owner.as_deref()
            ),
            ("REQ-001", "", None)
        );
    }

    #[test]
    fn bad_output_is_one_line_never_a_panic() {
        let whole = fixture("demo/status.json");
        for cut in [0, 1, 10, whole.len() / 2, whole.len() - 3] {
            let e = status(&whole[..cut]).unwrap_err();
            assert!(
                e.starts_with("kiln status: kiln's JSON can't be read: ") && !e.contains('\n'),
                "{e}"
            );
        }
        assert!(
            list(r#"[{"id":"REQ-001","type":"requirement","title":"T"}]"#)
                .unwrap_err()
                .contains("missing field `status`")
        );
        assert!(list(
            r#"[{"id":"REQ-001","type":"requirement","title":"T","status":"x","line":-4}]"#
        )
        .is_err());
        assert!(list("\u{FFFD}\u{FFFD}").is_err());
        assert!(check("null").is_err());
    }

    #[test]
    fn clip_cuts_on_characters() {
        assert_eq!(clip("héllo", 2), "hé…");
        assert_eq!(clip("hé", 2), "hé");
    }
}
