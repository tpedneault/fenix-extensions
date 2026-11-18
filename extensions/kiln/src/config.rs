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
//! What it deliberately doesn't mirror: titles, plurals and folders (the
//! edits show type names, and kiln picks the folder), `template`, `paths`,
//! `[code]`, `[coverage]`, `[requirements]`, `[jira]`, `[signoff]`,
//! `[check]` and the exports -- nothing an edit offers depends on them.
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

/// What an edit needs of `kiln.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    /// In `kiln.toml`'s order.
    pub types: Vec<Type>,
    /// In `kiln.toml`'s order.
    pub roles: Vec<Role>,
}

impl Config {
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
    Ok(Config { types, roles })
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
        let c = parse("format = 1\n[types.a]\nprefix = \"A\"\nstatuses = [\"x\", 3]\nparents = 7\n[links]\nr = 1").unwrap();
        assert_eq!(c.types[0].statuses, ["x"]);
        assert!(c.roles.is_empty());
    }
}
