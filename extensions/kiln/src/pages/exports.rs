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
//! # Jira
//!
//! With `[jira] project` in `kiln.toml` ([`crate::config::Jira`]) the tab
//! ends with one more target, `jira`: `kiln jira sync --dry-run`, shown
//! as kiln prints it -- the heading (`jira (DEMO) → DEMO-100 "Kiln:
//! Demo"`), one line per operation (`create`, `update`, `move`, `orphan`,
//! `left alone`), the totals, and `refused` lines in the bad look. `jira
//! sync` has no `--format json` (kiln 1.0.0, `JiraCommand::Sync`), so these
//! are kiln's text lines as they are: nothing is parsed but telling the
//! heading (the first line, unindented), the totals (`N create, N update,
//! N move, N unchanged`, matched whole or not at all), a refusal (a line
//! that starts `refused`) and which operations write (`create`, `update`,
//! `move`) apart. JSON would be a Kiln-side addition; until then a change
//! of kiln's wording shows as plain lines, never as a wrong count (`P`
//! counts only lines that start with those three words).
//!
//! It is asked like the other targets (when the tab is first shown, on
//! `R`, held back while the project is read) and may take [`JIRA_DRY_MS`]:
//! kiln reads the project issue's sub-tasks and the workflow from Jira.
//! Unlike a Confluence plan, a refused operation doesn't stop the sync:
//! kiln leaves that ticket alone and syncs the rest (CLI.md, "kiln jira
//! sync"), so `P` is allowed and says how many are refused.
//!
//! `P` on it syncs under the same rules as publishing: no publish runs (it
//! shares the one-at-a-time guard, [`JIRA_SYNC`]); the dry run is current
//! (read in this round, at the project's change count as it is now); it has
//! something to write. The prompt shows the heading and the totals and asks
//! for the Jira project key typed exactly (`DEMO`, from `[jira] project`;
//! the first thing `kiln jira sync` prints is the project). Then exactly
//! `kiln jira sync --yes` ([`sync_args`]) runs, for as long as
//! [`PUBLISH_MS`]: it creates a ticket per request and writes the keys into
//! the items only when it has applied everything, so a watchdog that
//! stopped it half way would leave tickets no item names, and the next sync
//! would create them again. It isn't stopped when `kiln.path` changes.
//!
//! What it says after is kiln's: the summary (`jira: 1 created, 1 updated,
//! 1 moved; 1 file gained ticket keys (commit them)`, shown prominently,
//! as a warning, since the files it names are left uncommitted), the
//! refusals after it, or its line. Exit 3 (Jira failed part way) says what
//! CLI.md says: the keys of what was created are written into the items
//! even so, so commit them, and run it again to finish. Then the project
//! is read again (sync writes `jira =` into items and `kiln.toml`;
//! [`Reply::Reload`]) and the dry run read again once that read has ended
//! ([`Query::settled`]), so the plan shown is of the project as it now is.
//!
//! `A` on it opens `kiln auth jira` in a terminal. Never `--force` and
//! the rest, which Jira has none of; never a bare `kiln jira sync`, which
//! would ask a question nobody sees; never `--yes` anywhere but in the
//! confirmed [`sync_args`].
//!
//! What it deliberately doesn't do: write a file, run anything but these
//! kiln commands, or show comments (that's the Comments tab).

use std::collections::HashMap;

use fenix_ui_kit::{Key, KeyPress};
use serde::Deserialize;

use super::view::{publish_running, Cell, Ctx, Look, Query, Reply, Row, View, JIRA_SYNC};
use crate::config::Export;
use crate::jira;
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

/// How long the Jira plan may take: 5 minutes. kiln reads the project
/// issue's sub-tasks and the project's workflows from Jira, a few requests;
/// each is bounded by `fenix-jira`'s own timeouts.
pub const JIRA_DRY_MS: u32 = 5 * 60 * 1000;

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
/// The Jira plan's dry run, the prompt for the project key, and the sync.
const JIRA_DRY: &str = "jira-dry-run";
const JIRA_CONFIRM: &str = "sync-jira";
const JIRA_RUN: &str = "sync-jira-run";

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

/// `kiln jira sync --dry-run`: the plan, nothing changed. No `--format`:
/// `jira sync` has none.
pub fn jira_dry_run_args() -> Vec<String> {
    ["jira", "sync", "--dry-run"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// `kiln jira sync --yes`: syncs Jira, after the project key is typed. The
/// only list of arguments with `--yes` that isn't `kiln export`'s.
pub fn sync_args() -> Vec<String> {
    ["jira", "sync", "--yes"]
        .into_iter()
        .map(String::from)
        .collect()
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

/// Why a target has no answer: kiln's line and exit code. The Comments tab
/// and the Jira mirror say a failure the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub line: String,
    pub code: Option<i32>,
}

impl Failed {
    pub fn of(f: &Failure) -> Failed {
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
pub fn wants_token(f: &Failed) -> bool {
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

/// One line of `kiln jira sync --dry-run`, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct JiraLine {
    /// kiln's line, indentation off, on one line of plain text.
    text: String,
    kind: JiraKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JiraKind {
    /// The first, unindented line: `jira (DEMO) → DEMO-100 "Kiln: Demo"`.
    Heading,
    /// `create …`, `update …`, `move …`: what a sync would write.
    Write,
    /// `orphan …`, `left alone …`: reported, never written.
    Left,
    /// `refused …` or `refused: …`.
    Refused,
    /// `1 create, 1 update, 1 move, 2 unchanged[, 1 refused]`.
    Totals,
    /// Any other line, as kiln wrote it.
    Other,
}

/// The Jira plan's dry run: the round it answered in, the project's
/// change count when asked, and kiln's lines or why there are none.
#[derive(Debug, Clone)]
struct JiraHeard {
    round: u64,
    at: u64,
    got: Result<Vec<JiraLine>, Failed>,
}

/// What `P` on the Jira target showed and asked to be confirmed.
#[derive(Debug, Clone)]
struct JiraConfirm {
    project: String,
    round: u64,
    at: u64,
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
    /// The Jira target: its last dry run, its reads again since (after a
    /// sync), `P`'s prompt, open, and the sync's outcome (in `outcome`,
    /// under [`JIRA_SYNC`]).
    jira: Option<JiraHeard>,
    jira_again: u64,
    jira_confirm: Option<JiraConfirm>,
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

    /// The round the Jira plan is read in.
    fn jira_round(&self) -> u64 {
        self.round + self.jira_again
    }

    /// Whether the Jira plan's dry run is under way (or not yet asked).
    fn jira_reading(&self) -> bool {
        self.jira
            .as_ref()
            .is_none_or(|h| h.round != self.jira_round())
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
        if name == JIRA_SYNC {
            return self.ask_sync(ctx);
        }
        if let Some(running) = ctx.publishing {
            return Reply::Say(publish_running(running));
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
                "{}; nothing else was published",
                publish_running(running)
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

    // -- Jira ----------------------------------------------------------------

    /// `P` on the Jira target: why it can't sync, or the prompt for the
    /// project key.
    fn ask_sync(&mut self, ctx: &Ctx) -> Reply {
        if let Some(running) = ctx.publishing {
            return Reply::Say(publish_running(running));
        }
        let Some(project) = jira_project(ctx) else {
            return Reply::Say("kiln.toml's [jira] has no project: there's nothing to sync".into());
        };
        let Some(heard) = &self.jira else {
            return Reply::Say("the Jira plan is still being read: P once it's shown".into());
        };
        if heard.round != self.jira_round() {
            return Reply::Say("the Jira plan is being read again: P once it's shown".into());
        }
        let lines = match &heard.got {
            Ok(lines) => lines,
            Err(f) if jira::wants_token(&f.line) => {
                return Reply::Bad(format!(
                    "jira has no plan: {} -- A runs kiln auth jira in a terminal, then R",
                    f.line
                ))
            }
            Err(f) => return Reply::Bad(format!("jira has no plan: {}", f.line)),
        };
        if heard.at != ctx.changes {
            return Reply::Say(
                "the project changed since jira's plan was read: R reads it again, then P".into(),
            );
        }
        let writes = lines.iter().filter(|l| l.kind == JiraKind::Write).count();
        let refused = lines.iter().filter(|l| l.kind == JiraKind::Refused).count();
        if writes == 0 {
            let totals = lines
                .iter()
                .find(|l| l.kind == JiraKind::Totals)
                .map_or("empty", |l| l.text.as_str());
            return Reply::Say(format!("nothing to sync: jira's plan is {totals}"));
        }
        let heading = lines
            .iter()
            .find(|l| l.kind == JiraKind::Heading)
            .map_or("jira", |l| l.text.as_str());
        let totals = lines
            .iter()
            .find(|l| l.kind == JiraKind::Totals)
            .map_or(String::new(), |l| format!(" ({})", l.text));
        let left = if refused == 0 {
            String::new()
        } else {
            format!(
                "; kiln leaves the {} it refuses alone and syncs the rest",
                count(refused, "ticket")
            )
        };
        let label = format!(
            "Sync Jira: creates, updates and moves tickets, {heading}: {}{totals}{left}. Type {project} to sync",
            count(writes, "operation")
        );
        self.jira_confirm = Some(JiraConfirm {
            project: project.to_string(),
            round: heard.round,
            at: heard.at,
        });
        Reply::ask(JIRA_CONFIRM, label, "")
    }

    /// The project key typed after `P` on Jira: exactly `[jira] project`,
    /// and the plan still the one shown, or nothing is synced.
    fn confirmed_sync(&mut self, text: &str, ctx: &Ctx) -> Reply {
        let Some(c) = self.jira_confirm.take() else {
            return Reply::Ignored;
        };
        if text != c.project {
            return Reply::Say(format!("“{text}” isn't {}: nothing was synced", c.project));
        }
        if let Some(running) = ctx.publishing {
            return Reply::Say(format!("{}; nothing was synced", publish_running(running)));
        }
        let fresh = self
            .jira
            .as_ref()
            .is_some_and(|h| h.round == c.round && h.at == c.at && self.jira_round() == c.round);
        if !fresh || c.at != ctx.changes {
            return Reply::Say(
                "jira's plan isn't the one shown any more (the project or the plan was read again): nothing was synced; R, then P"
                    .into(),
            );
        }
        Reply::Run {
            tag: JIRA_RUN.into(),
            args: sync_args(),
            limit_ms: Some(PUBLISH_MS),
            publish: Some(JIRA_SYNC.into()),
        }
    }

    /// The sync ended: kiln's own word on it kept under the target and
    /// said, then the project read again (it wrote keys into items and
    /// `kiln.toml`) and the dry run read again after.
    fn synced(&mut self, result: &Result<Ran, Failure>) -> Reply {
        // Refused at once (another publish runs): nothing ran.
        if let Err(Failure::Unusable(why)) = result {
            return Reply::Bad(why.clone());
        }
        self.jira_again += 1;
        let (line, look) = sync_outcome(result);
        self.outcome
            .insert(JIRA_SYNC.to_string(), (line.clone(), look.clone()));
        Reply::Reload {
            say: line,
            bad: look == Look::Bad,
        }
    }

    /// The Jira target's rows: its heading, then what kiln said.
    fn jira_rows(&self, ctx: &Ctx, project: &str, rows: &mut Vec<Row>) {
        let got = self.jira.as_ref().map(|h| &h.got);
        let lines = got.and_then(|g| g.as_ref().ok());
        let heading = lines
            .and_then(|l| l.iter().find(|l| l.kind == JiraKind::Heading))
            .map_or_else(|| format!("jira ({project})"), |l| l.text.clone());
        rows.push(Row::heading(vec![Cell::title(heading)]));
        let key = |what: &str| format!("{JIRA_SYNC}{SEP}{what}");
        let entry = |what: &str, text: String, look: Look| {
            Row::entry(key(what), vec![Cell::new(text, look)]).depth(1)
        };
        if ctx.publishing == Some(JIRA_SYNC) {
            rows.push(entry(
                "publishing",
                "syncing… kiln plans again and applies it; the keys of new tickets are written into the items".into(),
                Look::Warn,
            ));
        }
        if let Some((line, look)) = self.outcome.get(JIRA_SYNC) {
            rows.push(entry("outcome", line.clone(), look.clone()));
        }
        if self.jira_reading() {
            rows.push(Row::note("reading… (asks Jira)").depth(1));
        } else if self.jira.as_ref().is_some_and(|h| h.at != ctx.changes) && lines.is_some() {
            rows.push(
                Row::note("the project changed since this was read: R reads it again").depth(1),
            );
        }
        match got {
            None => {
                if !self.jira_reading() {
                    rows.push(Row::note("not read yet: R reads it").depth(1));
                }
            }
            Some(Err(f)) => {
                rows.push(entry("error", f.line.clone(), Look::Bad));
                if jira::wants_token(&f.line) {
                    rows.push(
                        Row::note(
                            "run kiln auth jira in a terminal: A opens one below, then R reads this again",
                        )
                        .depth(1),
                    );
                }
            }
            Some(Ok(lines)) => {
                for (i, l) in lines.iter().enumerate() {
                    let look = match l.kind {
                        // The heading is the row above.
                        JiraKind::Heading => continue,
                        JiraKind::Refused => Look::Bad,
                        JiraKind::Left => Look::Warn,
                        JiraKind::Totals | JiraKind::Other => Look::Text,
                        JiraKind::Write => match l.text.split_whitespace().next() {
                            Some("create") => Look::Good,
                            _ => Look::Accent,
                        },
                    };
                    rows.push(entry(&format!("op{i}"), l.text.clone(), look));
                }
                if lines.iter().any(|l| l.kind == JiraKind::Refused) {
                    rows.push(
                        Row::note(
                            "kiln leaves a refused ticket alone and syncs the rest: P does the same",
                        )
                        .depth(1),
                    );
                }
            }
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

/// `[jira] project`, when `kiln.toml` was read and has one: whether the
/// tab has a Jira target.
fn jira_project<'a>(ctx: &Ctx<'a>) -> Option<&'a str> {
    ctx.config.ok().and_then(|c| c.jira_project())
}

/// kiln's `jira sync --dry-run` as lines: its answer when it exited 0 (a
/// plan) or 1 (a plan with a refusal), else its one line. See the module
/// header, "Jira", for how little of it is read.
fn jira_plan(result: &Result<Ran, Failure>) -> Result<Vec<JiraLine>, Failed> {
    let ran = result.as_ref().map_err(Failed::of)?;
    match ran.code {
        Some(0 | 1) if !ran.stdout.trim().is_empty() => Ok(jira_lines(&ran.stdout)),
        _ => Err(Failed {
            line: ran.error_line(),
            code: ran.code,
        }),
    }
}

/// Each non-empty line of `text`, told apart as far as is robust: the
/// first line when it isn't indented is the heading; a refusal starts
/// `refused`; the totals are `N create, N update, N move, N unchanged`
/// (and `, N refused`), matched whole; the operations that write start
/// `create`, `update` or `move`; `orphan` and `left alone` are reported
/// only. Anything else is [`JiraKind::Other`], shown as kiln wrote it.
fn jira_lines(text: &str) -> Vec<JiraLine> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, raw)| {
            let t = raw.trim_start();
            let first = t.split(|c: char| c.is_whitespace() || c == ':').next();
            let kind = if i == 0 && raw == t {
                JiraKind::Heading
            } else if first == Some("refused") {
                JiraKind::Refused
            } else if is_jira_totals(t) {
                JiraKind::Totals
            } else {
                match first {
                    Some("create" | "update" | "move") => JiraKind::Write,
                    Some("orphan" | "left") => JiraKind::Left,
                    _ => JiraKind::Other,
                }
            };
            JiraLine {
                // Kiln's own spacing kept: it aligns the plan with it.
                text: model::printable(t, 400),
                kind,
            }
        })
        .collect()
}

/// `1 create, 1 update, 1 move, 2 unchanged` and the same with `, 1
/// refused`: four counts (five), in that order, nothing else.
fn is_jira_totals(t: &str) -> bool {
    let parts: Vec<&str> = t.split(", ").collect();
    let words = ["create", "update", "move", "unchanged", "refused"];
    (parts.len() == 4 || parts.len() == 5)
        && parts.iter().zip(words).all(|(p, word)| {
            p.split_once(' ').is_some_and(|(n, w)| {
                w == word && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

/// A sync's outcome, from kiln's answer: its summary line, the refusals
/// after it, or its line, with what to do next.
fn sync_outcome(result: &Result<Ran, Failure>) -> (String, Look) {
    let ran = match result {
        Err(Failure::TimedOut(_)) => {
            let line = result.as_ref().err().map(Failure::line).unwrap_or_default();
            return (
                format!(
                    "{line}: the sync was stopped before it wrote the keys of the tickets it created; look in Jira for sub-tasks of the project issue no item links (the next plan reports them as orphans) before syncing again"
                ),
                Look::Bad,
            );
        }
        Err(f) => return (f.line(), Look::Bad),
        Ok(ran) => ran,
    };
    let lines: Vec<&str> = ran
        .stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    let at = lines.iter().rposition(|l| l.starts_with("jira: "));
    let summary = at.map(|i| model::printable(lines[i], 400));
    let refused: Vec<String> = at
        .map(|i| {
            lines[i + 1..]
                .iter()
                .map(|l| model::printable(l, 400))
                .collect()
        })
        .unwrap_or_default();
    let gained = summary
        .as_deref()
        .is_some_and(|s| s.contains("gained ticket keys"));
    let with_refused = |base: String| {
        if refused.is_empty() {
            base
        } else {
            format!("{base} -- {}", refused.join("; "))
        }
    };
    match ran.code {
        Some(0) => match summary {
            Some(s) => (
                with_refused(s),
                // The files it names are changed and uncommitted.
                if gained { Look::Warn } else { Look::Good },
            ),
            None => (
                "nothing synced: kiln's plan, read as it synced, had nothing to do".into(),
                Look::Text,
            ),
        },
        // Some of it refused, or keys that couldn't be written: after
        // what it did, kiln's line when it said one, else that it refused.
        Some(1) => {
            let line = ran
                .stderr
                .iter()
                .rev()
                .find(|l| l.starts_with("kiln: "))
                .map(|l| model::printable(l, 400))
                .unwrap_or_else(|| "kiln refused some of it (exit 1): the plan shows what".into());
            let base = match summary {
                Some(s) => format!("{s} -- {line}"),
                None => line,
            };
            (with_refused(base), Look::Bad)
        }
        // Jira failed part way: the keys of what was created are written
        // first (CLI.md, "kiln jira sync").
        Some(3) => {
            let line = ran.error_line();
            let keys = if gained {
                " -- the keys of the tickets it created are in your files even so: commit them, so none is created twice, then sync again to finish"
            } else {
                ""
            };
            let base = match summary {
                Some(s) => format!("{s} -- {line}{keys}"),
                None => line,
            };
            (with_refused(base), Look::Bad)
        }
        Some(2) if jira::wants_token(&ran.error_line()) => (
            format!(
                "{} -- A runs kiln auth jira in a terminal",
                ran.error_line()
            ),
            Look::Bad,
        ),
        _ => (ran.error_line(), Look::Bad),
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
                    // Jira is its own command: its target stands.
                    if let Some(project) = jira_project(ctx) {
                        self.jira_rows(ctx, project, &mut rows);
                    }
                    return rows;
                }
                Some((round, _)) if *round == self.round => {}
                _ => rows.push(Row::note("reading…")),
            }
        }
        for n in self.names(ctx) {
            self.target_rows(&n, ctx, &mut rows);
        }
        if let Some(project) = jira_project(ctx) {
            self.jira_rows(ctx, project, &mut rows);
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
                if name == JIRA_SYNC {
                    return Reply::Say("jira isn't a file target: P syncs it".into());
                }
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
                Some(name) if name == JIRA_SYNC => Reply::Terminal {
                    title: "kiln auth jira".into(),
                    args: jira::auth_args(),
                },
                Some(name) if self.confluence(&name, ctx) == Some(true) => Reply::Terminal {
                    title: "kiln auth confluence".into(),
                    args: auth_args(),
                },
                _ => Reply::Say(
                    "A signs in: on a Confluence target (kiln auth confluence) or on jira (kiln auth jira)"
                        .into(),
                ),
            },
            _ => Reply::Ignored,
        }
    }

    fn empty(&self) -> String {
        "no export targets".into()
    }

    fn queries(&self, ctx: &Ctx) -> Vec<Query> {
        let mut qs = if !Self::one_by_one(ctx) {
            vec![Query::new(DRY_ALL, dry_run_args(None))
                .per_round(self.round)
                .limit(DRY_RUN_MS)]
        } else {
            self.names(ctx)
                .iter()
                .map(|n| {
                    Query::new(&format!("{DRY}{}", n.name), dry_run_args(Some(n.name)))
                        .per_round(self.round_of(n.name))
                        .limit(DRY_RUN_MS)
                })
                .collect()
        };
        if jira_project(ctx).is_some() {
            // Held back while the project is read: after a sync that
            // writes keys into items, the plan is of the project as that
            // read leaves it.
            qs.push(
                Query::new(JIRA_DRY, jira_dry_run_args())
                    .per_round(self.jira_round())
                    .limit(JIRA_DRY_MS)
                    .settled(),
            );
        }
        qs
    }

    fn answer_at(&mut self, tag: &str, result: &Result<Ran, Failure>, changes: u64) {
        if tag == JIRA_DRY {
            self.jira = Some(JiraHeard {
                round: self.jira_round(),
                at: changes,
                got: jira_plan(result),
            });
            return;
        }
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
        match tag {
            CONFIRM => self.confirmed(text, ctx),
            JIRA_CONFIRM => self.confirmed_sync(text, ctx),
            _ => Reply::Ignored,
        }
    }

    fn ran(&mut self, tag: &str, result: &Result<Ran, Failure>, ctx: &Ctx) -> Reply {
        if tag == WRITE {
            return self.written(result, ctx);
        }
        if tag == JIRA_RUN {
            return self.synced(result);
        }
        match tag.strip_prefix(PUBLISH) {
            Some(name) => self.published(name, result, ctx),
            None => Reply::Ignored,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
