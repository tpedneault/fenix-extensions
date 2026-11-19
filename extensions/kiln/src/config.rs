//! What the project's `kiln.toml` says about its types and link roles,
//! read here so the edits can *offer* choices: a type's statuses in order
//! (which is first, which are done or closed), which types may be a
//! type's parent, which roles a type may start and what each may point
//! at, and the types there are.
//!
//! # Why it's read here
//!
//! kiln 1.0.0 prints none of this as data: `list`, `show` and `status`
//! give an item's status and whether it is done or closed, not its type's
//! statuses or roles; `kiln agents` writes them as prose into
//! `design/AGENTS.md`, a guide, not an interface; `kiln lsp` and `kiln
//! provider` are servers. So the file is read with a TOML parser, from the
//! project's top (`filesystem = "project-read"`), the way `lsp.rs` reads
//! `.fenix/tools.json`.
//!
//! # Only to offer
//!
//! What this says is never a check. kiln validates every edit itself, and
//! a choice kiln then refuses is shown as kiln's line; what this gets
//! wrong costs a refused edit, never a wrong file. A `kiln.toml` that
//! can't be read or parsed (or isn't at the project's top: kiln walks up
//! to the nearest one, which may be above what Fenix lets this read) is
//! [`Err`] with why, and the edits fall back to asking for the value as
//! text (the status typed, the target ID typed).
//!
//! # The defaults, as Kiln applies them
//!
//! Mirrored from Kiln's own parser, `crates/kiln-parse/src/config.rs`
//! (`types`, `word_list`, `subset`, `links`), as of Kiln 1.0.0:
//!
//! - `initial`: the first status when it's left out, *or* when it names a
//!   status the type doesn't have (Kiln reports K002 and uses the first);
//! - `done`, `closed`, `parents`: none when left out; a status in `done`
//!   or `closed` that isn't one of the type's is dropped, as is a parent
//!   that isn't a key of `[types]` (or is `code`);
//! - a type is left out when its name isn't a lowercase word or is
//!   `code`, it isn't a table, it has no `prefix`, its prefix isn't
//!   uppercase letters and digits starting with a letter, another type has
//!   that prefix, or it has no statuses; a status that isn't a word, or
//!   is listed twice, is dropped;
//! - a role is left out when its name isn't a word, or it lacks `inverse`
//!   or a `from` or `to` naming a kept type (`from` may name `code`; `to`
//!   never does). Kiln has no default roles: no `[links]`, no roles.
//! - `format` other than 1 is refused (Kiln reads nothing else then).
//!
//! Types and roles keep `kiln.toml`'s order, as Kiln's do (`kiln new`'s
//! list of types, the order `kiln list` sorts types by).
//!
//! # The export targets
//!
//! The Exports tab asks kiln about each `[[export]]` on its own (`kiln
//! export --dry-run -- NAME`), because one target kiln can't plan (a
//! Confluence target with no token, a server that doesn't answer) fails
//! the whole command in kiln 1.0.0, and the other targets would then show
//! nothing. So their names are read here, in `kiln.toml`'s order, with
//! what a target's heading says before kiln has answered (its format,
//! layout, path, and for Confluence the space and the page). A table
//! without a string `name` is left out (Kiln reports it as K005); a name
//! listed twice is kept once (kiln exports the first). kiln's answer is
//! what the tab shows once it comes; this only names what to ask about.
//!
//! # The Jira section
//!
//! `[jira] url` and `[jira] project`, as strings, so the plan's tabs know
//! whether there is a Jira to read tickets from (item rows end with them
//! once the mirror arrives) and whether the Exports tab has a `jira`
//! target (`kiln jira sync` needs the project key; it is also the word
//! typed to confirm a sync). Nothing else of `[jira]` is read (`parent`,
//! `types`, `[jira.status]` are kiln's to apply), and a value that isn't a
//! string is as good as absent.
//!
//! # Sign-off
//!
//! `[signoff]` says which statuses are a person's call (Kiln's K19, `kiln
//! signoff`): the agent tools refuse to set one ([`crate::agent`]), since
//! Kiln's own rule for agents is to leave sign-off to people. Read as
//! Kiln's `signoff` and `Config::signoff_statuses` read it
//! (`crates/kiln-parse/src/config.rs`, `crates/kiln-model/src/config.rs`):
//! written as a table, it replaces the defaults -- each key a type, its
//! value the statuses, those the type doesn't have (or that aren't strings)
//! dropped, a type it doesn't name having none; not written, or not a table
//! (Kiln reports that and goes on as if it weren't there), the defaults
//! apply where the type has them: a requirement's `approved` and
//! `verified`, a decision's `accepted` and `rejected`, a risk's `accepted`
//! and `closed`, a milestone's `done` ([`DEFAULT_SIGNOFF`]). Here this is a
//! check, unlike the rest: what it gets wrong refuses an agent's edit (a
//! person can still make it), which is the side to err on.
//!
//! What it deliberately doesn't mirror: titles, plurals and folders (the
//! edits show type names, and kiln picks the folder), `template`, `paths`,
//! `[code]`, `[coverage]`, `[requirements]`, the rest of `[jira]`,
//! `[check]` and the rest of the exports and `[confluence]` -- nothing an
//! edit, the agent tools or the Exports tab's headings depend on.
//! Unknown keys are ignored (Kiln reports them as K003; kiln's own check
//! says so, not this). It never writes the file.

use std::collections::HashSet;

use toml::{Table, Value};

/// The largest `kiln.toml` read: 1 MiB. Kiln's own is a few KiB; past
/// this it isn't read at all, and the edits ask for text instead.
pub const MAX_KILN_TOML: u64 = 1024 * 1024;

/// The only `format` Kiln 1.0 reads.
const FORMAT: i64 = 1;

/// The pseudo-type of code references: only ever a role's `from`.
const CODE: &str = "code";

/// One `[types.NAME]`, as Kiln applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    /// `task`.
    pub name: String,
    /// `TASK`.
    pub prefix: String,
    /// In `kiln.toml`'s order: the order `H` and `L` move through.
    pub statuses: Vec<String>,
    /// What `kiln new` writes.
    pub initial: String,
    pub done: Vec<String>,
    pub closed: Vec<String>,
    /// Types allowed as its `parent`; none: it can't have one.
    pub parents: Vec<String>,
}

impl Type {
    /// Where `status` comes in the type's order.
    pub fn position(&self, status: &str) -> Option<usize> {
        self.statuses.iter().position(|s| s == status)
    }
}

/// One `[links.ROLE]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// `verifies`.
    pub name: String,
    /// The types it's written on (maybe `code`).
    pub from: Vec<String>,
    /// The types it may point at.
    pub to: Vec<String>,
    /// `verified by`.
    pub inverse: String,
}

/// One `[[export]]`, as far as the Exports tab names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// `design-md`: what `kiln export NAME` takes.
    pub name: String,
    /// `markdown`, `html`, `docx`, `confluence`… (`""` when not given:
    /// kiln calls that K005).
    pub format: String,
    /// `document` unless given.
    pub layout: String,
    /// A file target's file or folder.
    pub path: Option<String>,
    /// A Confluence target's space: its own, else `[confluence] space`.
    pub space: Option<String>,
    /// A Confluence target's `page`, else its `parent`.
    pub page: Option<u64>,
    /// It writes into an existing page (`page =`), rather than under a
    /// parent.
    pub adopts: bool,
}

impl Export {
    /// Whether it publishes to Confluence rather than writing files.
    pub fn is_confluence(&self) -> bool {
        self.format == "confluence"
    }
}

/// `[jira]`, as far as the tabs read it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Jira {
    /// `[jira] url`: where tickets are read from.
    pub url: Option<String>,
    /// `[jira] project`: the Jira project key `kiln jira sync` puts
    /// tickets in. Without it there is nothing to sync.
    pub project: Option<String>,
}

/// What an edit needs of `kiln.toml`, and the export targets' names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    /// In `kiln.toml`'s order.
    pub types: Vec<Type>,
    /// In `kiln.toml`'s order.
    pub roles: Vec<Role>,
    /// `[[export]]`, in `kiln.toml`'s order, each name once.
    pub exports: Vec<Export>,
    /// `[jira]`, when the table is there.
    pub jira: Option<Jira>,
    /// `[signoff]` as written, type by type, in `kiln.toml`'s order; `None`
    /// when it isn't written as a table, so the defaults apply
    /// ([`Config::signoff_statuses`]).
    pub signoff: Option<Vec<(String, Vec<String>)>>,
}

/// The sign-off statuses when `[signoff]` isn't written, as Kiln's
/// `DEFAULT_SIGNOFF` has them: its agent guide's rule (approving a
/// requirement, accepting or rejecting a decision, accepting or closing a
/// risk, finishing a milestone), and a requirement's `verified`.
pub const DEFAULT_SIGNOFF: &[(&str, &[&str])] = &[
    ("requirement", &["approved", "verified"]),
    ("decision", &["accepted", "rejected"]),
    ("risk", &["accepted", "closed"]),
    ("milestone", &["done"]),
];

impl Config {
    /// The names of the file targets (Markdown, HTML, Word), in order:
    /// every target that doesn't publish to Confluence. What `W` on the
    /// Exports tab and the export tasks name, so that no run of kiln
    /// they cause can reach Confluence.
    pub fn file_targets(&self) -> Vec<&str> {
        self.exports
            .iter()
            .filter(|e| !e.is_confluence())
            .map(|e| e.name.as_str())
            .collect()
    }

    /// Whether any `[[export]]` publishes to Confluence.
    pub fn has_confluence_target(&self) -> bool {
        self.exports.iter().any(Export::is_confluence)
    }

    /// `[jira] url`, when it is a non-empty string.
    pub fn jira_url(&self) -> Option<&str> {
        self.jira
            .as_ref()
            .and_then(|j| j.url.as_deref())
            .filter(|u| !u.trim().is_empty())
    }

    /// `[jira] project`, when it is a non-empty string.
    pub fn jira_project(&self) -> Option<&str> {
        self.jira
            .as_ref()
            .and_then(|j| j.project.as_deref())
            .filter(|p| !p.trim().is_empty())
    }

    /// The statuses of type `kind` that are a person's call: `[signoff]`'s
    /// list when it's written (and then only what it says), else the
    /// defaults the type has. None for a type `kiln.toml` doesn't have.
    pub fn signoff_statuses(&self, kind: &str) -> Vec<&str> {
        let Some(t) = self.kind(kind) else {
            return Vec::new();
        };
        let has = |s: &&str| t.statuses.iter().any(|x| x == s);
        match &self.signoff {
            Some(written) => written
                .iter()
                .filter(|(name, _)| name == kind)
                .flat_map(|(_, statuses)| statuses.iter().map(String::as_str))
                .filter(has)
                .collect(),
            None => DEFAULT_SIGNOFF
                .iter()
                .filter(|(name, _)| *name == kind)
                .flat_map(|(_, statuses)| statuses.iter().copied())
                .filter(has)
                .collect(),
        }
    }

    pub fn kind(&self, name: &str) -> Option<&Type> {
        self.types.iter().find(|t| t.name == name)
    }

    /// The roles an item of type `kind` may start.
    pub fn roles_from(&self, kind: &str) -> Vec<&Role> {
        self.roles
            .iter()
            .filter(|r| r.from.iter().any(|f| f == kind))
            .collect()
    }

    /// The types an item of type `kind` may be the parent of.
    pub fn children_of(&self, kind: &str) -> Vec<&Type> {
        self.types
            .iter()
            .filter(|t| t.parents.iter().any(|p| p == kind))
            .collect()
    }
}

/// `kiln.toml`'s text, read as Kiln reads it, or why it can't be: not
/// TOML, another `format`, no type kept. Never panics.
pub fn parse(text: &str) -> Result<Config, String> {
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("kiln.toml isn't valid TOML: {}", first_line(&e)))?;
    match root.get("format").and_then(Value::as_integer) {
        Some(FORMAT) => {}
        Some(n) => {
            return Err(format!(
                "kiln.toml is format {n}; this reads format {FORMAT}"
            ))
        }
        None => return Err("kiln.toml has no `format = 1`".into()),
    }
    let types = match root.get("types").and_then(Value::as_table) {
        Some(table) => types(table),
        None => Vec::new(),
    };
    if types.is_empty() {
        return Err("kiln.toml has no type Kiln can use".into());
    }
    let roles = match root.get("links").and_then(Value::as_table) {
        Some(table) => roles(table, &types),
        None => Vec::new(),
    };
    let exports = exports(&root);
    let jira = root.get("jira").and_then(Value::as_table).map(|t| {
        let text = |key: &str| t.get(key).and_then(Value::as_str).map(str::to_string);
        Jira {
            url: text("url"),
            project: text("project"),
        }
    });
    let signoff = root.get("signoff").and_then(Value::as_table).map(|t| {
        t.iter()
            .map(|(name, statuses)| (name.clone(), strings(Some(statuses))))
            .collect()
    });
    Ok(Config {
        types,
        roles,
        exports,
        jira,
        signoff,
    })
}

/// `[[export]]`'s targets with a name, the first of each name.
fn exports(root: &Table) -> Vec<Export> {
    let space = root
        .get("confluence")
        .and_then(|c| c.get("space"))
        .and_then(Value::as_str);
    let mut out: Vec<Export> = Vec::new();
    let Some(list) = root.get("export").and_then(Value::as_array) else {
        return out;
    };
    for t in list.iter().filter_map(Value::as_table) {
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.is_empty() || out.iter().any(|e| e.name == name) {
            continue;
        }
        let text = |key: &str| t.get(key).and_then(Value::as_str).map(str::to_string);
        let confluence = t.get("confluence").and_then(Value::as_table);
        let number = |key: &str| {
            confluence
                .and_then(|c| c.get(key))
                .and_then(Value::as_integer)
                .and_then(|n| u64::try_from(n).ok())
        };
        out.push(Export {
            name: name.to_string(),
            format: text("format").unwrap_or_default(),
            layout: text("layout").unwrap_or_else(|| "document".into()),
            path: text("path"),
            space: confluence
                .and_then(|c| c.get("space"))
                .and_then(Value::as_str)
                .or(space)
                .map(str::to_string),
            page: number("page").or_else(|| number("parent")),
            adopts: number("page").is_some(),
        });
    }
    out
}

fn first_line(e: &toml::de::Error) -> String {
    e.message()
        .lines()
        .next()
        .unwrap_or("not valid TOML")
        .trim()
        .to_string()
}

/// Kiln's `is_word`: lowercase ASCII letters, digits and single `-`
/// between them.
fn is_word(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Kiln's `is_prefix`: uppercase letters and digits, a letter first.
fn is_prefix(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// A list of strings; what isn't a string is skipped.
fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn types(table: &Table) -> Vec<Type> {
    let names: Vec<&str> = table.keys().map(String::as_str).collect();
    let mut prefixes: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (name, value) in table {
        if name == CODE || !is_word(name) {
            continue;
        }
        let Some(t) = value.as_table() else { continue };
        let Some(prefix) = t.get("prefix").and_then(Value::as_str) else {
            continue;
        };
        if !is_prefix(prefix) || !prefixes.insert(prefix.to_string()) {
            continue;
        }
        let mut statuses: Vec<String> = Vec::new();
        for s in strings(t.get("statuses")) {
            if is_word(&s) && !statuses.contains(&s) {
                statuses.push(s);
            }
        }
        let Some(first) = statuses.first().cloned() else {
            continue;
        };
        let initial = t
            .get("initial")
            .and_then(Value::as_str)
            .filter(|s| statuses.iter().any(|x| x == s))
            .map(str::to_string)
            .unwrap_or(first);
        let subset = |key: &str| -> Vec<String> {
            strings(t.get(key))
                .into_iter()
                .filter(|s| statuses.contains(s))
                .collect()
        };
        let parents = strings(t.get("parents"))
            .into_iter()
            .filter(|p| p != CODE && names.contains(&p.as_str()))
            .collect();
        out.push(Type {
            name: name.clone(),
            prefix: prefix.to_string(),
            initial,
            done: subset("done"),
            closed: subset("closed"),
            statuses,
            parents,
        });
    }
    out
}

fn roles(table: &Table, types: &[Type]) -> Vec<Role> {
    let known = |t: &str| types.iter().any(|d| d.name == t);
    let mut out = Vec::new();
    for (name, value) in table {
        if !is_word(name) {
            continue;
        }
        let Some(t) = value.as_table() else { continue };
        let from: Vec<String> = strings(t.get("from"))
            .into_iter()
            .filter(|f| f == CODE || known(f))
            .collect();
        let to: Vec<String> = strings(t.get("to"))
            .into_iter()
            .filter(|f| f != CODE && known(f))
            .collect();
        let Some(inverse) = t.get("inverse").and_then(Value::as_str) else {
            continue;
        };
        if from.is_empty() || to.is_empty() {
            continue;
        }
        out.push(Role {
            name: name.clone(),
            from,
            to,
            inverse: inverse.to_string(),
        });
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture(path: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// The demo's `kiln.toml`, as `kiln init` wrote it.
    pub(crate) fn demo() -> Config {
        parse(&fixture("demo/kiln.toml")).unwrap()
    }

    #[test]
    fn the_demos_types_and_roles_in_order() {
        let c = demo();
        let names: Vec<&str> = c.types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "requirement",
                "design",
                "decision",
                "milestone",
                "task",
                "test",
                "risk"
            ]
        );
        let task = c.kind("task").unwrap();
        assert_eq!(task.prefix, "TASK");
        assert_eq!(
            task.statuses,
            ["todo", "doing", "blocked", "done", "dropped"]
        );
        assert_eq!(task.initial, "todo");
        assert_eq!(
            (task.done.clone(), task.closed.clone()),
            (vec!["done".to_string()], vec!["dropped".to_string()])
        );
        assert_eq!(task.parents, ["task"]);
        let adr = c.kind("decision").unwrap();
        assert!(
            adr.done.is_empty() && adr.parents.is_empty(),
            "left out: none"
        );
        let roles: Vec<&str> = c
            .roles_from("task")
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(roles, ["implements", "part-of", "mitigates", "depends-on"]);
        assert_eq!(c.roles[1].inverse, "verified by");
        let kids: Vec<&str> = c
            .children_of("requirement")
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(kids, ["requirement"]);
        assert!(c.children_of("risk").is_empty());
        assert_eq!(task.position("done"), Some(3));
    }

    #[test]
    fn kilns_own_kiln_toml_reads() {
        let c = parse(&fixture("out/own/kiln.toml")).unwrap();
        assert!(c.kind("requirement").is_some() && c.kind("task").is_some());
        let task = c.kind("task").unwrap();
        assert_eq!(task.initial, task.statuses[0]);
        assert!(!c.roles.is_empty());
        // Every role it keeps names kept types only.
        for r in &c.roles {
            assert!(r.to.iter().all(|t| c.kind(t).is_some()), "{r:?}");
        }
    }

    #[test]
    fn defaults_are_kilns() {
        let c = parse(
            r#"
format = 1
name = "x"
[types.story]
prefix = "S"
statuses = ["new", "Bad", "built", "new", "shipped"]
initial = "nope"
done = ["shipped", "gone"]
parents = ["story", "code", "ghost", "epic"]
[types.epic]
prefix = "E"
statuses = ["open"]
[types.code]
prefix = "C"
statuses = ["a"]
[types.dup]
prefix = "S"
statuses = ["a"]
[types.noprefix]
statuses = ["a"]
[types.lower]
prefix = "low"
statuses = ["a"]
[types.empty]
prefix = "EM"
statuses = []
[types.Upper]
prefix = "U"
statuses = ["a"]
[links.grows]
from = ["story", "code", "ghost"]
to = ["epic", "code"]
inverse = "grown by"
[links.nowhere]
from = ["story"]
to = ["ghost"]
inverse = "x"
[links.noinverse]
from = ["story"]
to = ["epic"]
[unknown]
key = 1
"#,
        )
        .unwrap();
        let names: Vec<&str> = c.types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["story", "epic"]);
        let s = &c.types[0];
        assert_eq!(s.statuses, ["new", "built", "shipped"], "not a word, twice");
        assert_eq!(s.initial, "new", "an initial it hasn't is the first");
        assert_eq!(s.done, ["shipped"]);
        assert!(s.closed.is_empty());
        assert_eq!(
            s.parents,
            ["story", "epic"],
            "code and unknown names dropped"
        );
        assert_eq!(c.roles.len(), 1);
        assert_eq!(c.roles[0].from, ["story", "code"]);
        assert_eq!(c.roles[0].to, ["epic"]);
    }

    #[test]
    fn what_kiln_wouldnt_read_is_an_error_not_a_panic() {
        for (text, said) in [
            ("format = ", "kiln.toml isn't valid TOML: "),
            (
                "format = 2\n[types.a]\nprefix=\"A\"\nstatuses=[\"x\"]",
                "kiln.toml is format 2",
            ),
            ("name = \"x\"", "kiln.toml has no `format = 1`"),
            (
                "format = 1\n[types.a]\nstatuses=[\"x\"]",
                "kiln.toml has no type",
            ),
            ("format = 1\ntypes = 3", "kiln.toml has no type"),
            ("", "kiln.toml has no `format = 1`"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.starts_with(said), "{text:?}: {e}");
        }
        // Odd shapes are skipped, not fatal.
        let c = parse("format = 1\n[types.a]\nprefix = \"A\"\nstatuses = [\"x\"]\n[[export]]\nformat = \"html\"\n[[export]]\nname = \"x\"\n[[export]]\nname = \"x\"\nformat = \"html\"\n[[export]]\nname = \"c\"\nformat = \"confluence\"\n[export.confluence]\nparent = -4").unwrap();
        assert_eq!(
            c.exports.len(),
            2,
            "no name, and a name twice: {:?}",
            c.exports
        );
        assert_eq!(
            (c.exports[0].format.as_str(), c.exports[1].page),
            ("", None)
        );
        let c = parse("format = 1\n[types.a]\nprefix = \"A\"\nstatuses = [\"x\", 3]\nparents = 7\n[links]\nr = 1").unwrap();
        assert_eq!(c.types[0].statuses, ["x"]);
        assert!(c.roles.is_empty());
    }

    #[test]
    fn the_export_targets_in_order_with_what_their_headings_say() {
        let c = parse(&fixture("out/exports/kiln.toml")).unwrap();
        let names: Vec<(&str, &str, &str)> = c
            .exports
            .iter()
            .map(|e| (e.name.as_str(), e.format.as_str(), e.layout.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("design-doc", "markdown", "document"),
                ("requirements-html", "html", "document"),
                ("sections-md", "markdown", "sections"),
                ("design-doc-confluence", "confluence", "document"),
                ("adopted", "confluence", "document"),
            ]
        );
        assert_eq!(
            c.exports[0].path.as_deref(),
            Some("docs/DESIGN-DOCUMENT.md")
        );
        let spec = &c.exports[3];
        assert!(spec.is_confluence() && !c.exports[0].is_confluence());
        assert_eq!(
            (spec.space.as_deref(), spec.page),
            (Some("DEMO"), Some(123456789))
        );
        assert_eq!(c.exports[4].page, Some(555), "page = before parent");
        assert!(c.exports[4].adopts && !spec.adopts);
        assert_eq!(demo().exports.len(), 1, "the demo's own design-doc");
        assert_eq!(
            c.file_targets(),
            ["design-doc", "requirements-html", "sections-md"],
            "no Confluence target among them"
        );
        assert!(c.has_confluence_target() && !demo().has_confluence_target());
    }

    #[test]
    fn the_jira_section_is_its_url_and_project() {
        let c = parse(&fixture("out/jira/kiln.toml")).unwrap();
        assert_eq!(
            c.jira,
            Some(Jira {
                url: Some("http://127.0.0.1:9".into()),
                project: Some("DEMO".into())
            })
        );
        assert_eq!(
            (c.jira_url(), c.jira_project()),
            (Some("http://127.0.0.1:9"), Some("DEMO"))
        );
        assert_eq!(demo().jira, None, "no [jira]: no tickets, no target");
        // A url alone mirrors tickets and has no sync; empty or not a
        // string is as good as absent.
        let base = "format = 1\n[types.t]\nprefix = \"T\"\nstatuses = [\"a\"]\n";
        let only_url = parse(&format!("{base}[jira]\nurl = \"http://j\"\n")).unwrap();
        assert_eq!(
            (only_url.jira_url(), only_url.jira_project()),
            (Some("http://j"), None)
        );
        let odd = parse(&format!("{base}[jira]\nurl = \"\"\nproject = 4\n")).unwrap();
        assert_eq!((odd.jira_url(), odd.jira_project()), (None, None));
    }

    #[test]
    fn sign_off_is_kilns_defaults_unless_signoff_is_written() {
        // The demo writes none: the defaults its types have.
        let c = demo();
        assert_eq!(c.signoff, None);
        assert_eq!(c.signoff_statuses("requirement"), ["approved", "verified"]);
        assert_eq!(c.signoff_statuses("decision"), ["accepted", "rejected"]);
        assert_eq!(c.signoff_statuses("risk"), ["accepted", "closed"]);
        assert_eq!(c.signoff_statuses("milestone"), ["done"]);
        assert!(c.signoff_statuses("task").is_empty());
        assert!(c.signoff_statuses("nosuch").is_empty());
        // A default the type doesn't have isn't one.
        let base = "format = 1\n[types.requirement]\nprefix = \"REQ\"\nstatuses = [\"draft\", \"approved\"]\n[types.task]\nprefix = \"T\"\nstatuses = [\"todo\", \"done\"]\n";
        assert_eq!(
            parse(base).unwrap().signoff_statuses("requirement"),
            ["approved"]
        );
        // Written, it replaces them: only what it says, of the type's own.
        let written = parse(&format!(
            "{base}[signoff]\nrequirement = [\"draft\", \"signed\"]\ntask = [\"done\", 3]\nwidget = [\"x\"]\n"
        ))
        .unwrap();
        assert_eq!(written.signoff_statuses("requirement"), ["draft"]);
        assert_eq!(written.signoff_statuses("task"), ["done"]);
        let only = parse(&format!("{base}[signoff]\ntask = [\"done\"]\n")).unwrap();
        assert!(
            only.signoff_statuses("requirement").is_empty(),
            "written, no defaults"
        );
        // An empty table is written: nothing is sign-off.
        let none = parse(&format!("{base}[signoff]\n")).unwrap();
        assert!(none.signoff_statuses("requirement").is_empty());
        // Not a table: as if it weren't there, as Kiln reads it.
        let odd = parse(&format!("signoff = 4\n{base}")).unwrap();
        assert_eq!(odd.signoff_statuses("requirement"), ["approved"]);
    }
}
