//! Comments: `kiln confluence comments --format json` -- the unresolved
//! footer comments on each Confluence target's pages, by the item they are
//! about (CONFLUENCE.md, "Comments"). A heading per target as kiln words it
//! (`spec → space FLINT: 2 unresolved comments`); under it the items with
//! comments (an item row: `Enter` opens its file, `v` and `i` work) and
//! under each its comments, `Alice Martin · 2026-10-12  Startup in 50 ms Is
//! that measured cold?`; a comment about the page as a whole, which kiln
//! can't place on an item, is under a line naming the page. `o` opens the
//! comment under the cursor in the browser ([`Reply::Url`]), `R` reads it
//! all again, `A` opens `kiln auth confluence` in a terminal when kiln has
//! no token.
//!
//! # Reading
//!
//! One command for every Confluence target (`kiln confluence comments
//! --format json`), asked when the tab is first shown and on `R`, not on
//! every reload ([`Query::per_round`]): it reads each target's pages from
//! the server, then each page's comments, so it is as costly as a dry run
//! and gets as long ([`COMMENTS_MS`]), "reading… (asks Confluence)"
//! meanwhile. Unlike Exports it can't be asked one target at a time to
//! keep one failure from hiding the others, and needn't be: kiln's command
//! fails *as a whole* on any target's error -- no token, a server that
//! doesn't answer, a missing page (`?` at crates/kiln/src/cmd/
//! confluence.rs:129 and 146, exit 3; no token, exit 2) -- and prints
//! nothing per target, so there is no per-target error to show under a
//! heading. A failure is kiln's one line for the whole tab, in the bad look,
//! with the `A` hint when it is a missing token (the helper Exports uses,
//! [`wants_token`]). A target that has no page yet isn't in kiln's JSON
//! (confluence.rs:35); it is shown from `kiln.toml` under its own heading
//! as "not published yet".
//!
//! With `kiln.toml` read and no Confluence target in it, the tab says so
//! and asks kiln nothing (kiln would say `kiln.toml declares no Confluence
//! export target` and exit 2). When the extension can't read `kiln.toml`
//! it asks kiln anyway, which is the authority, and shows what it lists.
//!
//! # Other people's writing
//!
//! Everything here but the target's name is typed by someone else in
//! Confluence: the author, the page's title, the comment. It is shown as
//! plain text on one line, a cell being nothing more ([`model::plain`]):
//! control characters (a newline, a tab, an escape sequence) and
//! bidirectional overrides become spaces, runs of them one, and the length
//! is cut; no Markdown, no link, nothing is interpreted. The one thing done
//! with it is `o` opening its link, which kiln builds from `[confluence]
//! url` and numbers and the frame opens only if it is `http(s)` with no
//! space or control character in it ([`super::view::web_url`]).
//!
//! What it deliberately doesn't do: write anything, answer or resolve a
//! comment (that is Confluence's), or open anything but the link kiln gave.

use fenix_ui_kit::{Key, KeyPress};
use serde::Deserialize;

use super::exports::{auth_args, wants_token, Failed};
use super::view::{Cell, Ctx, Look, Query, Reply, Row, View};
use crate::kiln::{Failure, Ran};
use crate::model::{self, count};

/// How long reading the comments may take: 10 minutes, as a dry run's. kiln
/// reads each target's pages and then each page's comments, a request each
/// at least, each bounded by its own timeouts (60 s, six tries on 429 and
/// 503), so a large tree has many. Stopping it is harmless (it reads only).
pub const COMMENTS_MS: u32 = 10 * 60 * 1000;

/// The query's tag.
const READ: &str = "comments";

/// Between a row's target and the rest of its key.
const SEP: char = '\u{1}';

/// What of a comment's author, a page's title or its text is shown: more is
/// cut. kiln cuts the text at 200 characters itself.
const AUTHOR_MAX: usize = 60;
const TITLE_MAX: usize = 100;
const TEXT_MAX: usize = 300;

/// `kiln confluence comments --format json`: every Confluence target.
pub fn args() -> Vec<String> {
    ["confluence", "comments", "--format", "json"]
        .into_iter()
        .map(String::from)
        .collect()
}

// -- What kiln says -------------------------------------------------------------

/// One target of `kiln confluence comments --format json`, as far as the
/// tab reads it. Every field optional so a newer kiln's extras don't stop
/// the tab.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Target {
    name: String,
    space: String,
    comments: Vec<Comment>,
}

/// One unresolved comment, kiln-confluence's `Placed`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Comment {
    /// The item it is about, when kiln could tell.
    item: Option<String>,
    page: u64,
    page_title: String,
    author: String,
    /// As Confluence gave it: `2026-10-12T09:15:00.000Z`.
    created: String,
    /// One line, kiln's own.
    text: String,
    /// What `o` opens.
    url: String,
}

/// kiln's answer as targets, or why there's none: any exit but 0 is its
/// line (the command fails whole), JSON that isn't the shape is said.
fn targets(result: &Result<Ran, Failure>) -> Result<Vec<Target>, Failed> {
    let ran = result.as_ref().map_err(Failed::of)?;
    match ran.code {
        Some(0) => {
            model::array::<Target>("kiln confluence comments", &ran.stdout).map_err(|line| Failed {
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

// -- The tab ----------------------------------------------------------------------

/// The Comments tab.
#[derive(Debug, Default)]
pub struct Comments {
    /// `R`'s count.
    round: u64,
    /// The last answer, and the round it answered in.
    heard: Option<(u64, Result<Vec<Target>, Failed>)>,
}

/// The Confluence targets' names `kiln.toml` has, in order, when it was
/// read.
fn configured<'a>(ctx: &Ctx<'a>) -> Option<Vec<&'a str>> {
    ctx.config.ok().map(|c| {
        c.exports
            .iter()
            .filter(|e| e.is_confluence())
            .map(|e| e.name.as_str())
            .collect()
    })
}

impl Comments {
    /// Whether the comments are being read (or not yet asked) this round.
    fn reading(&self) -> bool {
        self.heard.as_ref().is_none_or(|(r, _)| *r != self.round)
    }

    /// One target's rows: its heading, then its items and their comments.
    fn target_rows(&self, name: &str, found: Option<&Target>, ctx: &Ctx, rows: &mut Vec<Row>) {
        let Some(t) = found else {
            rows.push(Row::heading(vec![Cell::title(format!(
                "{name} → not published yet"
            ))]));
            rows.push(
                Row::note("kiln lists no comments for a target with no page: publish it first")
                    .depth(1),
            );
            return;
        };
        rows.push(Row::heading(vec![Cell::title(format!(
            "{} → space {}: {}",
            model::plain(&t.name, TITLE_MAX),
            model::plain(&t.space, TITLE_MAX),
            count(t.comments.len(), "unresolved comment")
        ))]));
        if t.comments.is_empty() {
            rows.push(Row::note("nothing to answer").depth(1));
            return;
        }
        // By item, in the order kiln gave them; what is about no item, by
        // page.
        let mut groups: Vec<(Option<&str>, u64, Vec<&Comment>)> = Vec::new();
        for c in &t.comments {
            let item = c.item.as_deref().filter(|i| !i.is_empty());
            let at = groups
                .iter()
                .position(|(i, page, _)| *i == item && (item.is_some() || *page == c.page));
            match at {
                Some(at) => groups[at].2.push(c),
                None => groups.push((item, c.page, vec![c])),
            }
        }
        for (item, page, comments) in groups {
            match item {
                Some(id) => {
                    let title = ctx
                        .loaded
                        .item(id)
                        .map_or("(not in the project)", |i| i.title.as_str());
                    rows.push(
                        ctx.row(
                            id,
                            vec![
                                Cell::id(model::plain(id, 40)),
                                Cell::text(model::plain(title, TITLE_MAX)),
                            ],
                        )
                        .keyed(format!("{name}{SEP}item {id}"))
                        .depth(1),
                    );
                }
                None => {
                    let title = comments
                        .first()
                        .map_or(String::new(), |c| model::plain(&c.page_title, TITLE_MAX));
                    rows.push(Row::note(format!("about the page “{title}”")).depth(1));
                }
            }
            for (n, c) in comments.iter().enumerate() {
                let date: String = c.created.chars().take(10).collect();
                let who = format!(
                    "{} · {}",
                    model::plain(&c.author, AUTHOR_MAX),
                    model::plain(&date, 10)
                );
                // The link is the key, so `o` finds it again and the
                // cursor stays on this comment across reads.
                let key = if c.url.is_empty() {
                    format!("{name}{SEP}#{page}:{n}")
                } else {
                    format!("{name}{SEP}{}", c.url)
                };
                rows.push(
                    Row::entry(
                        key,
                        vec![
                            Cell::muted(who),
                            Cell::text(model::plain(&c.text, TEXT_MAX)),
                        ],
                    )
                    .depth(2),
                );
            }
        }
    }
}

impl View for Comments {
    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        let names = configured(ctx);
        if names.as_ref().is_some_and(|n| n.is_empty()) {
            return vec![Row::note(
                "kiln.toml has no Confluence export target, so there are no comments to read",
            )];
        }
        let mut rows = Vec::new();
        if self.reading() {
            rows.push(Row::note("reading… (asks Confluence)"));
        }
        match self.heard.as_ref().map(|(_, got)| got) {
            None => {}
            Some(Err(f)) => {
                rows.push(Row::entry(
                    "error",
                    vec![Cell::new(model::plain(&f.line, 500), Look::Bad)],
                ));
                if wants_token(f) {
                    rows.push(Row::note(
                        "run kiln auth confluence in a terminal: A opens one below, then R reads this again",
                    ));
                }
            }
            Some(Ok(listed)) => {
                // `kiln.toml`'s targets in its order, each with what kiln
                // listed for it; else the targets as kiln listed them.
                let names: Vec<&str> = match &names {
                    Some(n) => n.clone(),
                    None => listed.iter().map(|t| t.name.as_str()).collect(),
                };
                for name in names {
                    self.target_rows(name, listed.iter().find(|t| t.name == name), ctx, &mut rows);
                }
            }
        }
        rows
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("R", "read again"), ("o", "open comment"), ("A", "sign in")]
    }

    fn key(&mut self, key: KeyPress, row: Option<&Row>, ctx: &Ctx) -> Reply {
        if key.mods.ctrl || key.mods.alt || key.mods.meta {
            return Reply::Ignored;
        }
        let Key::Char(c) = key.key else {
            return Reply::Ignored;
        };
        match c {
            'R' => {
                if ctx.reading {
                    return Reply::Say(
                        "the comments are still being read: R once they're shown".into(),
                    );
                }
                self.round += 1;
                Reply::Rebuild
            }
            'o' => {
                let Some(row) = row else {
                    return Reply::Say("no comment here".into());
                };
                if row.id.is_some() {
                    return Reply::Say(
                        "o opens a comment in the browser: this is an item (Enter opens its file)"
                            .into(),
                    );
                }
                // A comment's key is its target and its link.
                match row.key().split_once(SEP) {
                    Some((_, url)) if !url.starts_with('#') => Reply::Url(url.to_string()),
                    Some(_) => Reply::Say("kiln gave no link for this comment".into()),
                    None => Reply::Say("no comment here".into()),
                }
            }
            'A' => match &self.heard {
                Some((_, Err(f))) if wants_token(f) => Reply::Terminal {
                    title: "kiln auth confluence".into(),
                    args: auth_args(),
                },
                _ => Reply::Say("A signs in to Confluence when kiln says it has no token".into()),
            },
            _ => Reply::Ignored,
        }
    }

    fn empty(&self) -> String {
        "no comments".into()
    }

    fn queries(&self, ctx: &Ctx) -> Vec<Query> {
        // Nothing to ask when `kiln.toml` is read and has no Confluence
        // target: kiln would only say so.
        if configured(ctx).is_some_and(|n| n.is_empty()) {
            return Vec::new();
        }
        vec![Query::new(READ, args())
            .per_round(self.round)
            .limit(COMMENTS_MS)]
    }

    fn answer(&mut self, tag: &str, result: &Result<Ran, Failure>) {
        if tag == READ {
            self.heard = Some((self.round, targets(result)));
        }
    }
}

#[cfg(test)]
mod tests;
