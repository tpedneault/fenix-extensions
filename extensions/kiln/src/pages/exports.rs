//! Exports: `kiln export --dry-run`, target by target, as the command line
//! prints it -- the target's line as a heading (`design-md (markdown,
//! document) → docs/generated/DESIGN.md`), then what would change (`would
//! update …`, `would remove …`, `unchanged (1 file)`), `leaves out: …`,
//! and for Confluence the plan's operations with their reasons, the totals
//! line, warnings and refusals. `w` writes the file target under the
//! cursor, `W` every file target, `P` publishes the Confluence target under
//! the cursor after its name is typed, `A` opens `kiln auth confluence` in
//! a terminal, `R` reads it all again.
//!
//! This is the one tab whose keys reach outside the machine: `P` writes
//! pages in a team's Confluence. What it does and never does is therefore
//! spelled out here, and tested.
//!
//! # Reading
//!
//! One dry run per target (`kiln export --dry-run --format json --
//! NAME`), the names from `kiln.toml` ([`crate::config::Export`]), because
//! in kiln 1.0.0 one target kiln can't plan fails the whole command
//! (`confluence_plan(..)?`, crates/kiln/src/cmd/export.rs:305): with no
//! token, or a server that doesn't answer, every target would show nothing
//! (recorded: tests/fixtures/out/exports/all-no-token.*). Asked one by one,
//! a failing target shows kiln's line under its heading and the others
//! show their plans. When the extension can't read `kiln.toml` (or it names
//! no target), every target is asked at once and kiln's answer, or its one
//! line, is all there is.
//!
//! A dry run renders the whole document and, for Confluence, reads the
//! space: too costly for every reload (the refresh timer reads the project
//! every few seconds while a page is in view). So it is asked when the tab
//! is first shown and on `R` ([`Query::per_round`]), and again for a target
//! after `w`, `W` or `P` writes it. It may take [`DRY_RUN_MS`] (kiln's own
//! timeouts bound each Confluence request: 60 s, six tries on 429 and
//! 503); the tab says "reading… (asks Confluence)" under the target
//! meanwhile. kiln runs one command at a time, so while a dry run (or a
//! publish) runs, the project's reloads and edits wait behind it: the
//! other tabs keep showing the last read, with "reading…" on the top line.
//!
//! # Writing file targets
//!
//! `w` runs `kiln export --format json -- NAME`, `W` the same with every
//! file target named. Never a bare `kiln export`: with no name kiln takes
//! every target, Confluence ones included, and plans them against the live
//! space first (export.rs:276-308, which needs a token and reads the
//! server); not on a terminal and without `--yes` it then publishes
//! nothing and exits 1 (export.rs:689-708, "pass --yes to publish without
//! asking"), and with no token it fails before writing any file (recorded:
//! out/exports/bare-no-token.*). Naming the file targets writes exactly
//! them. A write is local and git undoes it, so it asks nothing; its
//! outcome is said, and the target's dry run read again. It writes
//! generated files, not items, so the project isn't read again
//! ([`Reply::Run`], not [`Reply::Edit`]).
//!
//! # Publishing
//!
//! `P` on a Confluence target publishes it, only when:
//!
//! 1. no publish runs (one at a time for the whole extension);
//! 2. its last dry run is fresh: read in this round, and asked at the
//!    project's change count as it is now (nothing saved, edited, checked
//!    out or changed on disk since: [`Ctx::changes`]);
//! 3. its plan isn't refused, and has something to do.
//!
//! It then shows what will happen -- the pages written and the totals, the
//! space, the parent page by ID and title (`parentTitle`) -- in Fenix's
//! prompt, and asks for the target's name typed exactly (not `y`): a slip
//! of the finger on a page that publishes must not be enough. Anything
//! else cancels, and says so. Then it runs exactly
//! `kiln export --yes --format json -- NAME` ([`publish_args`]): one named
//! target, `--` before it so no name is read as an option.
//!
//! kiln plans again when it publishes: it reads the space afresh and
//! publishes its own plan, not the dry run's (export.rs:276-328; JSON
//! output publishes right after planning). The page state may have moved
//! since the dry run -- someone edited a page, another publish ran -- so
//! what kiln did is shown as kiln says it (`applied`: created, updated,
//! retired), and when kiln's plan differs from the one confirmed, that is
//! said too. A refusal at publish, or kiln's error line, is shown as it
//! is; exit 3 (network or server) says the next publish finishes what this
//! one started (CONFLUENCE.md: an interrupted publish leaves what it
//! applied applied, and the next run's plan picks up from there). The
//! dry run is read again after.
//!
//! A publish may take minutes against a large space, so it may run for
//! [`PUBLISH_MS`], two hours: the watchdog is there for a kiln that never
//! ends, and stopping one mid-publish leaves the space half updated until
//! the next publish, which is worse than waiting. It isn't stopped when
//! `kiln.path` changes either. While it runs, every tab's bottom line
//! says so, and everything else kiln does waits behind it.
//!
//! # Never
//!
//! - **`--force`, `--adopt`, `--prune-removed`, `--allow-shallow`.** Each
//!   overrides a refusal or destroys something: `--force` overwrites pages
//!   people edited in Confluence, `--adopt` replaces an existing page's
//!   content, `--prune-removed` sends pages to the trash, `--allow-shallow`
//!   publishes links Kiln can't check. Those are decisions to type in a
//!   terminal, where they are written down, not a key on a page. A plan
//!   that needs one is shown with kiln's reason and that sentence. No
//!   function here builds them ([`FORBIDDEN`], and the test over every
//!   argument list this module can build).
//! - **Publishing anywhere but `P` here.** It is not an agent tool, not a
//!   Fenix task (the tasks never pass `--yes`: [`crate::tasks`]) and not a
//!   `kiln.*` command.
//! - **A token.** The extension never asks for, reads or sees one: no
//!   secret, no prompt for one, no environment. `A` runs `kiln auth
//!   confluence` in a terminal pane, where the token is typed into kiln.
//!
//! What it deliberately doesn't do: write a file, run anything but these
//! kiln commands, publish Jira (E3.4) or show comments (Comments, E3.3).

use std::collections::HashMap;

use fenix_ui_kit::{Key, KeyPress};
use serde::Deserialize;

use super::view::{Cell, Ctx, Look, Query, Reply, Row, View};
use crate::config::Export;
use crate::kiln::{Failure, Ran};
use crate::model::{self, count};

/// How long a dry run may take: 10 minutes. A file target renders in well
/// under a second; a Confluence one reads the target's pages, each
/// request bounded by kiln (60 s, six tries on 429 and 503), and a large
/// tree has many. Stopping one is harmless (it writes nothing but kiln's
/// page cache, atomically).
pub const DRY_RUN_MS: u32 = 10 * 60 * 1000;

/// How long `w` and `W` may take: as long as a dry run, which renders the
/// same.
pub const WRITE_MS: u32 = DRY_RUN_MS;

/// How long a publish may take: 2 hours. A document is a handful of
/// requests; a tree of hundreds of pages is several requests a page, each
/// bounded by kiln's own timeouts (60 s, 120 s for attachments; six tries
/// on 429 and 503). No bound both frees the queue soon and never stops a
/// real publish, and a publish stopped half done is worse than a queue
/// that waits (CONFLUENCE.md: the next run finishes it): so this is far
/// past any real one, and only ends a kiln that never would.
pub const PUBLISH_MS: u32 = 2 * 60 * 60 * 1000;

/// What no argument list of this module may hold (module header,
/// "Never").
pub const FORBIDDEN: [&str; 4] = ["--force", "--adopt", "--prune-removed", "--allow-shallow"];

/// Tags: a dry run of one target is `DRY` and its name; of all at once,
/// `DRY_ALL`.
const DRY: &str = "dry-run:";
const DRY_ALL: &str = "dry-run";
/// `w` and `W`.
const WRITE: &str = "write";
/// The prompt for the name, and the publish (`PUBLISH` and the name).
const CONFIRM: &str = "publish";
const PUBLISH: &str = "publish:";

/// Between a row's target and the rest of its key.
const SEP: char = '\u{1}';

/// `kiln export --dry-run --format json`, of one target or of all.
pub fn dry_run_args(name: Option<&str>) -> Vec<String> {
    let mut args = vec!["export", "--dry-run", "--format", "json"];
    if let Some(name) = name {
        args.extend(["--", name]);
    }
    args.into_iter().map(String::from).collect()
}

/// `kiln export --format json -- NAME…`: writes these file targets.
pub fn write_args(names: &[&str]) -> Vec<String> {
    let mut args = vec!["export", "--format", "json", "--"];
    args.extend(names);
    args.into_iter().map(String::from).collect()
}

/// `kiln export --yes --format json -- NAME`: publishes this one target.
pub fn publish_args(name: &str) -> Vec<String> {
    ["export", "--yes", "--format", "json", "--", name]
        .into_iter()
        .map(String::from)
        .collect()
}

/// `kiln auth confluence`, in a terminal.
pub fn auth_args() -> Vec<String> {
    vec!["auth".into(), "confluence".into()]
}

// -- What kiln says -------------------------------------------------------------

/// One target of `kiln export --format json`, as far as the tab reads it.
/// kiln 1.0.0 gives no `kiln_json` here (it's an array) and no file
/// target's path (only its files); every field is optional so a newer
/// kiln's extras don't stop the tab.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Target {
    name: String,
    format: String,
    layout: String,
    /// File targets: what it writes (or would), new or not.
    write: Vec<Written>,
    unchanged: Vec<String>,
    remove: Vec<String>,
    #[serde(rename = "leftOut")]
    left_out: Vec<String>,
    /// Confluence targets.
    space: Option<String>,
    /// The page it goes under (or adopts).
    parent: Option<u64>,
    #[serde(rename = "parentTitle")]
    parent_title: Option<String>,
    plan: Option<Plan>,
    /// What publishing did; null in a dry run.
    applied: Option<Applied>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Written {
    path: String,
    new: bool,
}

/// kiln-confluence's `Plan`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Plan {
    ops: Vec<Op>,
    foreign: Vec<String>,
    warnings: Vec<String>,
    refusals: Vec<String>,
}

/// One page's operation. kiln 1.0.0 writes the page's ID as `page_id`,
/// unlike its other camelCase keys; `pageId` is read too.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Op {
    /// create, update, unchanged, drift, retire, prune.
    kind: String,
    title: String,
    reasons: Vec<String>,
    size: u64,
    #[serde(alias = "pageId")]
    page_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Applied {
    created: u64,
    updated: u64,
    retired: u64,
    pruned: u64,
    skipped: Vec<String>,
}

/// A plan's counts, as kiln's totals line prints them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Totals {
    create: usize,
    update: usize,
    unchanged: usize,
    drift: usize,
    retire: usize,
    prune: usize,
}

impl Totals {
    fn of(plan: &Plan) -> Totals {
        let n = |k: &str| plan.ops.iter().filter(|o| o.kind == k).count();
        Totals {
            create: n("create"),
            update: n("update"),
            unchanged: n("unchanged"),
            drift: n("drift"),
            retire: n("retire"),
            prune: n("prune"),
        }
    }

    /// The pages a publish writes: created, updated, retired, pruned.
    fn writes(&self) -> usize {
        self.create + self.update + self.retire + self.prune
    }

    /// `1 create, 0 update, 0 unchanged, 0 drift, 0 retire`, as kiln
    /// prints it (`, 1 prune` when there's one).
    fn line(&self) -> String {
        let prune = match self.prune {
            0 => String::new(),
            n => format!(", {n} prune"),
        };
        format!(
            "{} create, {} update, {} unchanged, {} drift, {} retire{prune}",
            self.create, self.update, self.unchanged, self.drift, self.retire
        )
    }
}

impl Target {
    fn is_confluence(&self) -> bool {
        self.format == "confluence"
    }
}

/// Why a target has no answer: kiln's line and exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Failed {
    line: String,
    code: Option<i32>,
}

impl Failed {
    fn of(f: &Failure) -> Failed {
        Failed {
            line: f.line(),
            code: f.code(),
        }
    }
}

/// kiln's answer as the targets it names, or why there's none. A dry run
/// exits 1 when a Confluence plan is refused, its JSON printed all the
/// same; a publish exits 1 so when it refuses. Any other exit, or no JSON,
/// is kiln's line.
fn targets(result: &Result<Ran, Failure>) -> Result<Vec<Target>, Failed> {
    let ran = result.as_ref().map_err(Failed::of)?;
    match ran.code {
        Some(0 | 1) if !ran.stdout.trim().is_empty() => {
            model::array::<Target>("kiln export", &ran.stdout).map_err(|line| Failed {
                line,
                code: ran.code,
            })
        }
        _ => Err(Failed {
            line: ran.error_line(),
            code: ran.code,
        }),
    }
}

/// Whether kiln's line is about a missing token: what `kiln auth
/// confluence` gives it. kiln 1.0.0 names `KILN_CONFLUENCE_TOKEN` in both
/// ways of saying so -- "no token for …: run `kiln auth confluence`, or
/// set KILN_CONFLUENCE_TOKEN" when the credential store has none, and the
/// store's own error "… (or set KILN_CONFLUENCE_TOKEN)" when there is no
/// store to ask (export.rs:570-583, crates/kiln-secrets/src/lib.rs:81-88).
fn wants_token(f: &Failed) -> bool {
    f.code == Some(2)
        && (f.line.contains("KILN_CONFLUENCE_TOKEN") || f.line.contains("kiln auth confluence"))
}

// -- The tab ---------------------------------------------------------------------

/// A dry run's answer for one target: the round it answered in, the
/// project's change count when it was asked, and what kiln said.
#[derive(Debug, Clone)]
struct Heard {
    round: u64,
    at: u64,
    got: Result<Target, Failed>,
}

/// What `P` showed and asked to be confirmed.
#[derive(Debug, Clone)]
struct Confirm {
    target: String,
    round: u64,
    at: u64,
    totals: Totals,
}

/// The Exports tab.
#[derive(Debug, Default)]
pub struct Exports {
    /// `R`'s count: every target's round moves with it.
    round: u64,
    /// A target's own reads again since (after `w`, `W`, `P`).
    again: HashMap<String, u64>,
    /// Each target's last dry run, by name.
    heard: HashMap<String, Heard>,
    /// Asked all at once: the names in kiln's order, and kiln's line when
    /// it failed as a whole.
    order: Vec<String>,
    all: Option<(u64, Result<(), Failed>)>,
    /// `P`'s prompt, open.
    confirm: Option<Confirm>,
    /// What each publish under way was confirmed with.
    sent: HashMap<String, Totals>,
    /// The last write's or publish's outcome under each target.
    outcome: HashMap<String, (String, Look)>,
    /// The targets the write under way names (`w`, `W`).
    writing: Vec<String>,
}

/// A target to show: its name, and what `kiln.toml` says of it when the
/// extension read it.
struct Named<'a> {
    name: &'a str,
    config: Option<&'a Export>,
}

impl Exports {
    /// The round target `name` is read in.
    fn round_of(&self, name: &str) -> u64 {
        self.round + self.again.get(name).copied().unwrap_or(0)
    }

    /// Target `name` read again (after it was written or published): on
    /// its own when targets are asked one by one, else all of them.
    fn read_again(&mut self, name: &str, ctx: &Ctx) {
        if Self::one_by_one(ctx) {
            *self.again.entry(name.to_string()).or_insert(0) += 1;
        } else {
            self.round += 1;
        }
    }

    /// The targets, from `kiln.toml` when it was read and names any; else
    /// as kiln last named them, asked all at once.
    fn names<'a>(&'a self, ctx: &Ctx<'a>) -> Vec<Named<'a>> {
        match ctx.config {
            Ok(c) if !c.exports.is_empty() => c
                .exports
                .iter()
                .map(|e| Named {
                    name: &e.name,
                    config: Some(e),
                })
                .collect(),
            _ => self
                .order
                .iter()
                .map(|n| Named {
                    name: n,
                    config: None,
                })
                .collect(),
        }
    }

    /// Whether the targets are asked one by one (`kiln.toml` names them).
    fn one_by_one(ctx: &Ctx) -> bool {
        matches!(ctx.config, Ok(c) if !c.exports.is_empty())
    }

    /// Whether target `name` publishes to Confluence, by kiln's answer,
    /// else by `kiln.toml`.
    fn confluence(&self, name: &str, ctx: &Ctx) -> Option<bool> {
        if let Some(Heard { got: Ok(t), .. }) = self.heard.get(name) {
            return Some(t.is_confluence());
        }
        match ctx.config {
            Ok(c) => c
                .exports
                .iter()
                .find(|e| e.name == name)
                .map(Export::is_confluence),
            Err(_) => None,
        }
    }

    /// Whether a dry run of `name` is under way (or not yet asked) in its
    /// round.
    fn reading(&self, name: &str) -> bool {
        self.heard
            .get(name)
            .is_none_or(|h| h.round != self.round_of(name))
    }

    /// `P` on target `name`: why it can't publish, or the prompt for its
    /// name.
    fn ask_publish(&mut self, name: &str, ctx: &Ctx) -> Reply {
        if let Some(running) = ctx.publishing {
            return Reply::Say(format!(
                "{running} is being published: one publish at a time"
            ));
        }
        if self.confluence(name, ctx) == Some(false) {
            return Reply::Say(format!("{name} is a file target: w writes it"));
        }
        let Some(heard) = self.heard.get(name) else {
            return Reply::Say(format!(
                "{name}'s plan is still being read: P once it's shown"
            ));
        };
        if heard.round != self.round_of(name) {
            return Reply::Say(format!(
                "{name}'s plan is being read again: P once it's shown"
            ));
        }
        let t = match &heard.got {
            Ok(t) => t,
            Err(f) if wants_token(f) => {
                return Reply::Bad(format!(
                    "{name} has no plan: {} -- A runs kiln auth confluence in a terminal, then R",
                    f.line
                ))
            }
            Err(f) => return Reply::Bad(format!("{name} has no plan: {}", f.line)),
        };
        if heard.at != ctx.changes {
            return Reply::Say(format!(
                "the project changed since {name}'s plan was read: R reads it again, then P"
            ));
        }
        let Some(plan) = &t.plan else {
            return Reply::Say(format!("{name} has no Confluence plan: w writes it"));
        };
        if let Some(why) = plan.refusals.first() {
            return Reply::Bad(format!("kiln refuses {name}'s plan: {why} -- {TERMINAL}"));
        }
        let totals = Totals::of(plan);
        if totals.writes() == 0 {
            return Reply::Say(format!(
                "nothing to publish: {name}'s plan is {}",
                totals.line()
            ));
        }
        let label = confirm_label(name, t, &totals, ctx);
        self.confirm = Some(Confirm {
            target: name.to_string(),
            round: heard.round,
            at: heard.at,
            totals,
        });
        Reply::ask(CONFIRM, label, "")
    }

    /// The name typed after `P`: exactly the target's, and the plan still
    /// fresh, or nothing is published.
    fn confirmed(&mut self, text: &str, ctx: &Ctx) -> Reply {
        let Some(c) = self.confirm.take() else {
            return Reply::Ignored;
        };
        let name = c.target;
        if text != name {
            return Reply::Say(format!("“{text}” isn't {name}: nothing was published"));
        }
        if let Some(running) = ctx.publishing {
            return Reply::Say(format!(
                "{running} is being published: one publish at a time; nothing else was published"
            ));
        }
        let fresh = self
            .heard
            .get(&name)
            .is_some_and(|h| h.round == c.round && h.at == c.at && self.round_of(&name) == c.round);
        if !fresh || c.at != ctx.changes {
            return Reply::Say(format!(
                "{name}'s plan isn't the one shown any more (the project or the plan was read again): nothing was published; R, then P"
            ));
        }
        self.sent.insert(name.clone(), c.totals);
        Reply::Run {
            tag: format!("{PUBLISH}{name}"),
            args: publish_args(&name),
            limit_ms: Some(PUBLISH_MS),
            publish: Some(name),
        }
    }

    /// `w` or `W` ended: each target's outcome kept under it and said, and
    /// what was written read again.
    fn written(&mut self, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply {
        let names = std::mem::take(&mut self.writing);
        for n in &names {
            self.read_again(n, ctx);
        }
        match targets(result) {
            Ok(list) => {
                let mut said = Vec::new();
                for t in &list {
                    let line = wrote(t);
                    self.outcome
                        .insert(t.name.clone(), (line.clone(), Look::Good));
                    said.push(format!("{}: {line}", t.name));
                }
                if said.is_empty() {
                    return Reply::Say("kiln wrote nothing, and said so".into());
                }
                Reply::Say(said.join("; "))
            }
            Err(f) => {
                for n in names {
                    self.outcome.insert(n, (f.line.clone(), Look::Bad));
                }
                Reply::Bad(f.line)
            }
        }
    }

    /// The file targets, in order: by `kiln.toml`, else by kiln's answers.
    fn file_names(&self, ctx: &Ctx) -> Vec<String> {
        self.names(ctx)
            .iter()
            .filter(|n| self.confluence(n.name, ctx) == Some(false))
            .map(|n| n.name.to_string())
            .collect()
    }

    /// A publish ended: kiln's own word on what it did, kept under the
    /// target and said; the target's dry run read again.
    fn published(&mut self, name: &str, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply {
        let shown = self.sent.remove(name);
        // Refused at once (another publish runs): nothing ran, nothing to
        // read again.
        if let Err(Failure::Unusable(why)) = result {
            return Reply::Bad(why.clone());
        }
        self.read_again(name, ctx);
        let (line, look) = publish_outcome(name, result, shown);
        self.outcome
            .insert(name.to_string(), (line.clone(), look.clone()));
        if look == Look::Good {
            Reply::Say(line)
        } else {
            Reply::Bad(line)
        }
    }

    /// One target's rows: its heading, then what kiln said.
    fn target_rows(&self, n: &Named, ctx: &Ctx, rows: &mut Vec<Row>) {
        let name = n.name;
        let heard = self.heard.get(name);
        let got = heard.map(|h| &h.got);
        let target = got.and_then(|g| g.as_ref().ok());
        let confluence = self.confluence(name, ctx).unwrap_or(false);
        rows.push(Row::heading(vec![Cell::title(heading(
            name, target, n.config,
        ))]));
        let key = |what: &str| format!("{name}{SEP}{what}");
        let entry = |what: &str, text: String, look: Look| {
            Row::entry(key(what), vec![Cell::new(text, look)]).depth(1)
        };
        if ctx.publishing == Some(name) {
            rows.push(entry(
                "publishing",
                "publishing… kiln reads the space again and publishes its own plan".into(),
                Look::Warn,
            ));
        }
        if let Some((line, look)) = self.outcome.get(name) {
            rows.push(entry("outcome", line.clone(), look.clone()));
        }
        if self.reading(name) {
            rows.push(
                Row::note(if confluence {
                    "reading… (asks Confluence)"
                } else {
                    "reading…"
                })
                .depth(1),
            );
        } else if heard.is_some_and(|h| h.at != ctx.changes) && target.is_some() {
            rows.push(
                Row::note("the project changed since this was read: R reads it again").depth(1),
            );
        }
        match got {
            None => {
                // Asked all at once, and kiln failed: its line is above.
                if !self.reading(name) {
                    rows.push(Row::note("not read yet: R reads it").depth(1));
                }
            }
            Some(Err(f)) => {
                rows.push(entry("error", f.line.clone(), Look::Bad));
                if confluence && wants_token(f) {
                    rows.push(
                        Row::note(
                            "run kiln auth confluence in a terminal: A opens one below, then R reads this again",
                        )
                        .depth(1),
                    );
                }
            }
            Some(Ok(t)) if t.is_confluence() => plan_rows(t, &key, rows),
            Some(Ok(t)) => file_rows(t, &key, rows),
        }
        if let Some(t) = target {
            if !t.left_out.is_empty() {
                rows.push(Row::note(format!("leaves out: {}", t.left_out.join(", "))).depth(1));
            }
        }
    }
}

/// What tells the person a refusal is theirs to override, in a terminal.
const TERMINAL: &str = "nothing is published from here; overriding a refusal (--force, --adopt) is done in a terminal, where you type it";

/// The target's line, as `kiln export` prints it: `design-md (markdown,
/// document) → docs/generated/DESIGN.md`, `spec (confluence, document) →
/// space FLINT, page 123 "Flint"`. From kiln's answer when there is one
/// (the path from `kiln.toml`: kiln's JSON has only the files), else from
/// `kiln.toml`.
fn heading(name: &str, t: Option<&Target>, e: Option<&Export>) -> String {
    let format = t
        .map(|t| t.format.as_str())
        .or(e.map(|e| e.format.as_str()))
        .unwrap_or("");
    let layout = t
        .map(|t| t.layout.as_str())
        .or(e.map(|e| e.layout.as_str()))
        .unwrap_or("document");
    let what = if format.is_empty() {
        String::new()
    } else {
        format!(" ({format}, {layout})")
    };
    if format == "confluence" {
        let space = t
            .and_then(|t| t.space.as_deref())
            .or(e.and_then(|e| e.space.as_deref()));
        let page = t.and_then(|t| t.parent).or(e.and_then(|e| e.page));
        let title = t
            .and_then(|t| t.parent_title.as_deref())
            .filter(|s| !s.is_empty())
            .map(|s| format!(" \"{s}\""))
            .unwrap_or_default();
        return match (space, page) {
            (Some(s), Some(p)) => format!("{name}{what} → space {s}, page {p}{title}"),
            (Some(s), None) => format!("{name}{what} → space {s}"),
            _ => format!("{name}{what}"),
        };
    }
    let path = e.and_then(|e| e.path.clone()).or_else(|| {
        // Asked all at once, a document's one file is its path.
        let t = t?;
        (t.layout == "document")
            .then(|| {
                t.write
                    .first()
                    .map(|w| w.path.clone())
                    .or(t.unchanged.first().cloned())
            })
            .flatten()
    });
    match path {
        Some(p) => format!("{name}{what} → {p}"),
        None => format!("{name}{what}"),
    }
}

/// A file target's rows: `would create`/`would update` each file (`Enter`
/// opens one that exists), `would remove` the ones no longer produced, or
/// `unchanged (N files)`.
fn file_rows(t: &Target, key: &dyn Fn(&str) -> String, rows: &mut Vec<Row>) {
    for w in &t.write {
        let verb = if w.new {
            "would create"
        } else {
            "would update"
        };
        let cells = vec![Cell::new(format!("{verb} {}", w.path), Look::Accent)];
        let row = if w.new {
            Row::entry(key(&w.path), cells)
        } else {
            Row::code(&w.path, 0, cells).keyed(key(&w.path))
        };
        rows.push(row.depth(1));
    }
    for path in &t.remove {
        rows.push(
            Row::entry(
                key(path),
                vec![Cell::new(
                    format!("would remove {path} (no longer produced)"),
                    Look::Warn,
                )],
            )
            .depth(1),
        );
    }
    if t.write.is_empty() && t.remove.is_empty() {
        let n = t.unchanged.len();
        let cells = vec![Cell::muted(format!(
            "unchanged ({n} file{})",
            if n == 1 { "" } else { "s" }
        ))];
        // One file: `Enter` opens it.
        let row = match t.unchanged.as_slice() {
            [one] => Row::code(one, 0, cells).keyed(key("unchanged")),
            _ => Row::entry(key("unchanged"), cells),
        };
        rows.push(row.depth(1));
    }
}

/// A Confluence plan's rows, as kiln prints them: each operation with its
/// reasons and size, the foreign pages, the totals, warnings, refusals.
fn plan_rows(t: &Target, key: &dyn Fn(&str) -> String, rows: &mut Vec<Row>) {
    let Some(plan) = &t.plan else {
        rows.push(Row::note("kiln gave no plan").depth(1));
        return;
    };
    for (i, op) in plan.ops.iter().enumerate() {
        let reasons = if op.reasons.is_empty() {
            String::new()
        } else {
            format!(": {}", op.reasons.join("; "))
        };
        let size = if op.size > 0 {
            format!(" ({} KiB)", op.size.div_ceil(1024))
        } else {
            String::new()
        };
        let look = match op.kind.as_str() {
            "create" => Look::Good,
            "update" => Look::Accent,
            "unchanged" => Look::Muted,
            "drift" => Look::Bad,
            _ => Look::Warn,
        };
        let text = format!("{:9} \"{}\"{reasons}{size}", op.kind, op.title);
        let id = op.page_id.map(|p| p.to_string()).unwrap_or_default();
        rows.push(Row::entry(key(&format!("op{i}:{id}")), vec![Cell::new(text, look)]).depth(1));
    }
    for title in &plan.foreign {
        rows.push(Row::note(format!("foreign   \"{title}\" (not Kiln's; left alone)")).depth(1));
    }
    rows.push(Row::entry(key("totals"), vec![Cell::text(Totals::of(plan).line())]).depth(1));
    for (i, w) in plan.warnings.iter().enumerate() {
        rows.push(
            Row::entry(
                key(&format!("warning{i}")),
                vec![Cell::new(format!("warning: {w}"), Look::Warn)],
            )
            .depth(1),
        );
    }
    for (i, r) in plan.refusals.iter().enumerate() {
        rows.push(
            Row::entry(
                key(&format!("refused{i}")),
                vec![Cell::new(format!("refused: {r}"), Look::Bad)],
            )
            .depth(1),
        );
    }
    if !plan.refusals.is_empty() {
        rows.push(
            Row::note(
                "kiln refuses this plan, so P publishes nothing of it; overriding it (--force, --adopt) is done in a terminal",
            )
            .depth(1),
        );
    }
}

/// What a write did to one target, in kiln's words without "would":
/// `created docs/a.md, removed docs/b.md`, or `unchanged`.
fn wrote(t: &Target) -> String {
    let mut done: Vec<String> = t
        .write
        .iter()
        .map(|w| format!("{} {}", if w.new { "created" } else { "updated" }, w.path))
        .collect();
    done.extend(t.remove.iter().map(|p| format!("removed {p}")));
    if done.is_empty() {
        "unchanged: nothing written".into()
    } else {
        done.join(", ")
    }
}

/// What the prompt says before the name is typed: that it writes to
/// Confluence, where, how many pages, and how to confirm.
fn confirm_label(name: &str, t: &Target, totals: &Totals, ctx: &Ctx) -> String {
    let space = t.space.as_deref().unwrap_or("?");
    let adopts =
        matches!(ctx.config, Ok(c) if c.exports.iter().any(|e| e.name == name && e.adopts));
    let place = match (
        t.parent,
        t.parent_title.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(p), Some(title)) => format!("page {p} \"{title}\""),
        (Some(p), None) => format!("page {p}"),
        _ => "its page".into(),
    };
    let where_ = if adopts { "into" } else { "under" };
    format!(
        "Publish to Confluence: writes {} in space {space}, {where_} {place} ({}). Type {name} to publish",
        count(totals.writes(), "page"),
        totals.line()
    )
}

/// A publish's outcome, from kiln's answer: what it applied, its refusal,
/// or its line, with what to do next.
fn publish_outcome(
    name: &str,
    result: &Result<Ran, Failure>,
    shown: Option<Totals>,
) -> (String, Look) {
    if let Err(Failure::TimedOut(_)) = result {
        let line = result.as_ref().err().map(Failure::line).unwrap_or_default();
        return (
            format!(
                "{line}: the publish may have stopped half done; the next publish finishes what it started"
            ),
            Look::Bad,
        );
    }
    match targets(result) {
        Ok(list) => {
            let Some(t) = list.iter().find(|t| t.name == name) else {
                return (format!("kiln's answer names no target {name}"), Look::Bad);
            };
            let plan = t.plan.clone().unwrap_or_default();
            if let Some(why) = plan.refusals.first() {
                return (
                    format!(
                        "nothing published: kiln refused {name}'s plan as it published: {why} -- {TERMINAL}"
                    ),
                    Look::Bad,
                );
            }
            let totals = Totals::of(&plan);
            let differs = match shown {
                Some(s) if s != totals => format!(
                    " -- kiln's plan as it published was {} (the one shown: {})",
                    totals.line(),
                    s.line()
                ),
                _ => String::new(),
            };
            match &t.applied {
                Some(a) => {
                    let skipped = if a.skipped.is_empty() {
                        String::new()
                    } else {
                        format!("; skipped: {}", a.skipped.join("; "))
                    };
                    (
                        format!(
                            "{name}: published {} created, {} updated, {} retired, {} pruned{skipped}{differs}",
                            a.created, a.updated, a.retired, a.pruned
                        ),
                        if differs.is_empty() && a.skipped.is_empty() {
                            Look::Good
                        } else {
                            Look::Warn
                        },
                    )
                }
                None => (
                    format!(
                        "{name}: nothing published: kiln's plan, read as it published, had nothing to do{differs}"
                    ),
                    Look::Warn,
                ),
            }
        }
        Err(f) if f.code == Some(3) => (
            format!(
                "{} -- the next publish finishes what this one started",
                f.line
            ),
            Look::Bad,
        ),
        Err(f) if wants_token(&f) => (
            format!("{} -- A runs kiln auth confluence in a terminal", f.line),
            Look::Bad,
        ),
        Err(f) => (f.line, Look::Bad),
    }
}

impl View for Exports {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let mut rows = Vec::new();
        if !Self::one_by_one(ctx) {
            let why = match ctx.config {
                Err(why) => format!("{why}, so kiln was asked about every target at once"),
                Ok(_) => "kiln.toml names no [[export]] here, so kiln was asked about every target at once".into(),
            };
            rows.push(Row::note(why));
            match &self.all {
                Some((round, Err(f))) if *round == self.round => {
                    rows.push(Row::entry(
                        SEP.to_string(),
                        vec![Cell::new(f.line.clone(), Look::Bad)],
                    ));
                    return rows;
                }
                Some((round, _)) if *round == self.round => {}
                _ => rows.push(Row::note("reading…")),
            }
        }
        for n in self.names(ctx) {
            self.target_rows(&n, ctx, &mut rows);
        }
        rows
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("R", "read again"),
            ("w", "write"),
            ("W", "write all"),
            ("P", "publish"),
            ("A", "sign in"),
        ]
    }

    fn key(&mut self, key: KeyPress, row: Option<&Row>, ctx: &Ctx) -> Reply {
        if key.mods.ctrl || key.mods.alt || key.mods.meta {
            return Reply::Ignored;
        }
        let Key::Char(c) = key.key else {
            return Reply::Ignored;
        };
        let target = row
            .and_then(|r| r.key().split(SEP).next())
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        match c {
            'R' => {
                if ctx.reading {
                    return Reply::Say("the dry run is still being read: R once it's shown".into());
                }
                self.round += 1;
                Reply::Rebuild
            }
            'w' => {
                let Some(name) = target else {
                    return Reply::Say("no export target here".into());
                };
                if self.confluence(&name, ctx) == Some(true) {
                    return Reply::Say(format!("{name} publishes to Confluence: P publishes it"));
                }
                self.writing = vec![name.clone()];
                Reply::Run {
                    tag: WRITE.into(),
                    args: write_args(&[name.as_str()]),
                    limit_ms: Some(WRITE_MS),
                    publish: None,
                }
            }
            'W' => {
                let names = self.file_names(ctx);
                if names.is_empty() {
                    return Reply::Say("no file targets to write".into());
                }
                let args = write_args(&names.iter().map(String::as_str).collect::<Vec<_>>());
                self.writing = names;
                Reply::Run {
                    tag: WRITE.into(),
                    args,
                    limit_ms: Some(WRITE_MS),
                    publish: None,
                }
            }
            'P' => match target {
                Some(name) => self.ask_publish(&name, ctx),
                None => Reply::Say("no export target here".into()),
            },
            'A' => match target {
                Some(name) if self.confluence(&name, ctx) == Some(true) => Reply::Terminal {
                    title: "kiln auth confluence".into(),
                    args: auth_args(),
                },
                _ => Reply::Say("A signs in to Confluence: on a Confluence target".into()),
            },
            _ => Reply::Ignored,
        }
    }

    fn empty(&self) -> String {
        "no export targets".into()
    }

    fn queries(&self, ctx: &Ctx) -> Vec<Query> {
        if !Self::one_by_one(ctx) {
            return vec![Query::new(DRY_ALL, dry_run_args(None))
                .per_round(self.round)
                .limit(DRY_RUN_MS)];
        }
        self.names(ctx)
            .iter()
            .map(|n| {
                Query::new(&format!("{DRY}{}", n.name), dry_run_args(Some(n.name)))
                    .per_round(self.round_of(n.name))
                    .limit(DRY_RUN_MS)
            })
            .collect()
    }

    fn answer_at(&mut self, tag: &str, result: &Result<Ran, Failure>, changes: u64) {
        if tag == DRY_ALL {
            match targets(result) {
                Ok(list) => {
                    self.order = list.iter().map(|t| t.name.clone()).collect();
                    for t in list {
                        let round = self.round_of(&t.name);
                        self.heard.insert(
                            t.name.clone(),
                            Heard {
                                round,
                                at: changes,
                                got: Ok(t),
                            },
                        );
                    }
                    self.all = Some((self.round, Ok(())));
                }
                Err(f) => self.all = Some((self.round, Err(f))),
            }
            return;
        }
        let Some(name) = tag.strip_prefix(DRY) else {
            return;
        };
        let got = targets(result).and_then(|list| {
            list.into_iter()
                .find(|t| t.name == name)
                .ok_or_else(|| Failed {
                    line: format!("kiln's answer names no target {name}"),
                    code: None,
                })
        });
        let round = self.round_of(name);
        self.heard.insert(
            name.to_string(),
            Heard {
                round,
                at: changes,
                got,
            },
        );
    }

    fn typed(&mut self, tag: &str, text: &str, ctx: &Ctx) -> Reply {
        if tag != CONFIRM {
            return Reply::Ignored;
        }
        self.confirmed(text, ctx)
    }

    fn ran(&mut self, tag: &str, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply {
        if tag == WRITE {
            return self.written(result, ctx);
        }
        match tag.strip_prefix(PUBLISH) {
            Some(name) => self.published(name, result, ctx),
            None => Reply::Ignored,
        }
    }
}

#[cfg(test)]
mod tests;
