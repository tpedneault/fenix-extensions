//! Code review in Fenix, as Fenix 1.x had it built in: the reviews
//! waiting (`SPC g M`), a request under review with its files, threads,
//! checks and your held comments, submitted with a verdict, and a new
//! pull request for the branch (`SPC g P`) -- on GitHub and GitLab.
//!
//! It's the forge for Fenix's Git pages (`[[contributes.forges]]`):
//! `forge.branch` is the status page's header line for the branch's
//! request, `forge.open` (`o`) a request or a new one, `forge.reviews`
//! the reviews. Everything that talks to the forge runs as a job
//! (`run::spawn`) and leaves what it found in `DONE`, applied to the
//! pages when it lands. Git runs as `git`, in the repository.

mod client;
mod git_bits;
mod git_request;
mod page;
mod proc;
mod review_inbox;
mod review_page;
mod request_drafts;
mod review_store;

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use fenix_extension_api::{self as api, export, forge, host, run, Event, Extension, Key, Page};
use fenix_forge::{Forge, MergeRequest, Verdict};

use client::Client;
use git_request::{Commit, Pushed, RequestAction, RequestPage, Standing};
use review_inbox::{Entry, Inbox, InboxAction};
use review_page::{FileView, ReviewAction, ReviewData, ReviewPage};
use review_store::Pending;

const INBOX_KIND: &str = "reviews";
const REVIEW_KIND: &str = "review";
const REQUEST_KIND: &str = "new-request";
/// The project, as the extension is given it.
const PROJECT: &str = "/project";

#[derive(Default)]
pub struct ReviewExtension {
    pages: HashMap<u64, Open>,
    /// What each compose pane is for.
    composing: HashMap<u64, Compose>,
}

enum Open {
    Inbox(Box<Inbox>),
    Review(Box<ReviewPage>),
    Request(Box<RequestPage>),
}

/// What a compose pane writes.
enum Compose {
    /// A comment held for the review on page `page`: a new one, or pending
    /// comment `index` edited.
    Pending { page: u64, pending: Box<Pending>, index: Option<usize> },
    /// A reply to a thread, sent straight away.
    Reply { page: u64, thread: String },
    Summary { page: u64 },
    Description { page: u64 },
}

/// What a job found, applied to its page once it's done.
enum Done {
    Inbox { page: u64, result: Result<Vec<Entry>, String> },
    Review { page: u64, result: Result<Box<ReviewData>, String> },
    Since { page: u64, result: Result<Vec<FileView>, String> },
    Wrote { page: u64, label: String, after: After, result: Result<(), String> },
    Log { page: u64, name: String, result: Result<String, String> },
    Existing { page: u64, result: Result<Option<MergeRequest>, String>, me: Option<String>, template: Option<String> },
    Opened { page: u64, result: Result<(MergeRequest, Option<String>), String> },
    /// Something to say, from a job.
    Say(Result<String, String>),
    OpenInbox { root: String, forge: String, project: String },
    OpenReview { root: String, forge: String, project: String, number: u64 },
    OpenRequest(Box<RequestPage>),
}

/// What to do once a write to the forge has come back.
enum After {
    Refresh,
    /// A review went out, at this head.
    Submitted(String),
    Merged,
}

thread_local! {
    static DONE: RefCell<Vec<Done>> = const { RefCell::new(Vec::new()) };
}

fn done(d: Done) {
    DONE.with(|q| q.borrow_mut().push(d));
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

/// A job that may fail with something to say.
fn job(f: impl Future<Output = Result<(), String>> + 'static) {
    run::spawn(async move {
        if let Err(e) = f.await {
            done(Done::Say(Err(e)));
        }
    });
}

/// The repository you're in: where `git` runs for it.
async fn repo_here() -> Result<String, String> {
    if !proc::git_ok(PROJECT, &["rev-parse", "--show-toplevel"]).await {
        return Err("not in a git repository -- open a file of the project first".into());
    }
    Ok(PROJECT.to_string())
}

/// A request's review, all at once.
async fn read_review(client: &Client, root: &str, number: u64) -> Result<ReviewData, String> {
    let request = client.merge_request(number).await?;
    // The head, fetched into the repository: what a worktree checks out,
    // and what the changes since your review are read from.
    if let Some(remote) = default_remote(root).await {
        let _ = proc::git(root, &["fetch", &remote, &client.checkout_refspec(number)]).await;
    }
    let approvals = client.approvals(number).await.ok();
    let (files, error) = match client.changed_files(number).await {
        Ok(files) => (files.iter().map(|f| FileView::new(f.display_path().to_string(), f.old_path.clone(), f.change.letter(), &f.unified_diff())).collect(), None),
        Err(err) => (Vec::new(), Some(format!("couldn't read the files: {err}"))),
    };
    let threads = client.discussions(number).await.unwrap_or_default();
    let checks = client.checks(number, &request.sha).await.unwrap_or_default();
    Ok(ReviewData { request, approvals, files, threads, checks, since: None, error })
}

/// The remote requests are fetched from: `origin`, else the first.
async fn default_remote(root: &str) -> Option<String> {
    let remotes = proc::git(root, &["remote"]).await.ok()?;
    let list: Vec<&str> = remotes.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    list.iter().find(|r| **r == "origin").or(list.first()).map(|r| r.to_string())
}

/// What changed between the head you reviewed and the head now -- just
/// the author's edits, even across a rebase: when the old head isn't in
/// the new one's history, only the files the request touches are
/// compared.
async fn read_since(root: &str, reviewed: &str, head: &str, paths: &[String]) -> Result<Vec<FileView>, String> {
    let rebased = !proc::git_ok(root, &["merge-base", "--is-ancestor", reviewed, head]).await;
    let mut args: Vec<&str> = vec!["diff", "--no-color", reviewed, head];
    if rebased {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    let text = proc::git(root, &args).await.map_err(|e| format!("couldn't diff against your last review: {e}"))?;
    Ok(split_diff(&text).into_iter().map(|(path, old_path, letter, raw)| FileView::new(path, old_path, letter, &raw)).collect())
}

/// A `git diff`, file by file: (path, old path, A/D/R/M, its diff).
fn split_diff(text: &str) -> Vec<(String, String, char, String)> {
    let mut files: Vec<(String, String, char, String)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (old, new) = rest.split_once(" b/").map(|(a, b)| (a.trim_start_matches("a/").to_string(), b.to_string())).unwrap_or_default();
            files.push((new, old, 'M', String::new()));
        }
        let Some(file) = files.last_mut() else { continue };
        if line.starts_with("new file mode") {
            file.2 = 'A';
        } else if line.starts_with("deleted file mode") {
            file.2 = 'D';
        } else if line.starts_with("rename from") {
            file.2 = 'R';
        }
        if !line.starts_with("diff --git ") {
            file.3.push_str(line);
            file.3.push('\n');
        }
    }
    files
}

/// A CI log line without the timestamp GitHub Actions puts in front of
/// every one (`2026-09-24T20:28:05.1234567Z `).
fn without_timestamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    let stamped = bytes.len() > 20 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-' && bytes[10] == b'T';
    match (stamped, line.find("Z ")) {
        (true, Some(end)) if end < 40 => &line[end + 2..],
        _ => line,
    }
}

/// The request page for the branch checked out at `root`: the branch's
/// commits against its base, whether it's pushed and merges cleanly.
async fn new_request(root: String) -> Result<(), String> {
    let branch = proc::git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await?.trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err("not on a branch -- switch to one (SPC g w) to open a pull request from it".into());
    }
    let client = client::for_repo(&root).await?;
    let base_ref = base_ref(&root).await.ok_or("no base branch to open it against -- set one in SPC , (Git)")?;
    let base = base_ref.strip_prefix("origin/").unwrap_or(&base_ref).to_string();
    if base == branch {
        return Err(format!("{branch} is the base branch -- make a branch for the change first (b c on the Git page)"));
    }
    let range = format!("{base_ref}..HEAD");
    let log = proc::git(&root, &["log", "--format=%h%x1f%s%x1f%b%x1e", "-n", "200", &range]).await.unwrap_or_default();
    let commits: Vec<Commit> = log
        .split('\u{1e}')
        .filter_map(|c| {
            let mut parts = c.trim_start_matches('\n').splitn(3, '\u{1f}');
            let short = parts.next()?.trim().to_string();
            (!short.is_empty()).then(|| Commit { short, subject: parts.next().unwrap_or("").trim().to_string(), body: parts.next().unwrap_or("").trim().to_string() })
        })
        .collect();
    let upstream = proc::git(&root, &["rev-parse", "--abbrev-ref", "@{u}"]).await.ok().map(|u| u.trim().to_string());
    let pushed = match &upstream {
        None => Pushed::NotYet,
        Some(u) => match proc::git(&root, &["rev-list", "--count", "@{u}..HEAD"]).await.ok().and_then(|n| n.trim().parse::<usize>().ok()) {
            Some(ahead) if ahead > 0 => Pushed::Ahead(ahead),
            _ => Pushed::UpToDate(u.split('/').next().unwrap_or("origin").to_string()),
        },
    };
    let fixups = commits.iter().filter(|c| c.subject.starts_with("fixup! ") || c.subject.starts_with("squash! ") || c.subject.starts_with("amend! ")).count();
    let merges_cleanly = match proc::output(api::Command::new("git").args(["merge-tree", "--write-tree", &base_ref, "HEAD"]).cwd(&root).env("GIT_TERMINAL_PROMPT", "0")).await {
        Ok(o) if o.ok => Some(true),
        Ok(o) if o.stdout.lines().count() > 1 => Some(false),
        _ => None,
    };
    let behind = format!("HEAD..{base_ref}");
    let base_moved = proc::git(&root, &["rev-list", "--count", &behind]).await.ok().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
    let standing = Standing { pushed, merges_cleanly, base_moved, fixups };
    let three = format!("{base_ref}...HEAD");
    let stat = shortstat(&proc::git(&root, &["diff", "--shortstat", &three]).await.unwrap_or_default());
    let meta_jira = api::here().and_then(|h| h.project).and_then(|p| p.meta.into_iter().find(|(k, _)| k == "jira").map(|(_, v)| v));
    let jira = git_request::jira_key(&branch, meta_jira.as_deref());
    let mut page = RequestPage::new(PathBuf::from(&root), client.name().to_string(), client.project().to_string(), branch, base, commits, stat, standing, jira);
    page.reviewers = api::setting("git.reviewers").unwrap_or_default().split(',').map(str::trim).filter(|r| !r.is_empty()).collect::<Vec<_>>().join(", ");
    done(Done::OpenRequest(Box::new(page)));
    Ok(())
}

/// The branch a request goes into: `git.base_branch`, else the remote's
/// default, else main or master.
async fn base_ref(root: &str) -> Option<String> {
    if let Some(base) = api::setting("git.base_branch").filter(|b| !b.trim().is_empty()) {
        let base = base.trim().to_string();
        let remote = format!("origin/{base}");
        if proc::git_ok(root, &["rev-parse", "--verify", "-q", &remote]).await {
            return Some(remote);
        }
        return proc::git_ok(root, &["rev-parse", "--verify", "-q", &base]).await.then_some(base);
    }
    if let Ok(head) = proc::git(root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).await {
        return Some(head.trim().to_string());
    }
    for name in ["origin/main", "origin/master", "main", "master"] {
        if proc::git_ok(root, &["rev-parse", "--verify", "-q", name]).await {
            return Some(name.to_string());
        }
    }
    None
}

/// `git diff --shortstat`'s line as (files, insertions, deletions).
fn shortstat(line: &str) -> (usize, usize, usize) {
    let mut out = (0, 0, 0);
    for part in line.split(',') {
        let n = part.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        if part.contains("file") {
            out.0 = n;
        } else if part.contains("insertion") {
            out.1 = n;
        } else if part.contains("deletion") {
            out.2 = n;
        }
    }
    out
}

/// The reviews for the repository at `root`.
async fn reviews_at(root: String) -> Result<(), String> {
    let client = client::for_repo(&root).await?;
    done(Done::OpenInbox { forge: client.name().into(), project: client.project().into(), root });
    Ok(())
}

/// `w`: fetches the request and checks it out beside the repository as a
/// worktree of its own, then opens a file of it -- your own branch and
/// buffers stay exactly as they were.
async fn review_in_worktree(root: String, number: u64) -> Result<(), String> {
    let client = client::for_repo(&root).await?;
    let top = PathBuf::from(proc::git(&root, &["rev-parse", "--show-toplevel"]).await?.trim());
    let name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".to_string());
    let path = top.parent().map(|p| p.join(format!("{name}.review-{number}"))).unwrap_or_else(|| PathBuf::from(format!("{name}.review-{number}")));
    let path_text = path.to_string_lossy().replace('\\', "/");
    let branch = format!("review/{number}");
    let list = proc::git(&root, &["worktree", "list", "--porcelain"]).await.unwrap_or_default();
    let have = list.lines().filter_map(|l| l.strip_prefix("worktree ")).any(|p| p.replace('\\', "/").eq_ignore_ascii_case(&path_text));
    if !have {
        let remote = default_remote(&root).await.ok_or("no remote to fetch the request from")?;
        let refspec = client.checkout_refspec(number);
        let head = refspec.split(':').next().unwrap_or(&refspec).to_string();
        let into = format!("+{}:refs/heads/{branch}", head.trim_start_matches('+'));
        proc::git(&root, &["fetch", &remote, &into]).await.map_err(|e| format!("couldn't fetch {number}: {e}"))?;
        proc::git(&root, &["worktree", "add", &path_text, &branch]).await.map_err(|e| format!("couldn't make the worktree: {e}"))?;
    }
    // A file of it, opened: Fenix moves to the worktree's project.
    let files = proc::git(&root, &["-C", &path_text, "ls-files", "README.md", "README", "readme.md", "Cargo.toml", "package.json"]).await.unwrap_or_default();
    match files.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(file) => host::open_file(&format!("{path_text}/{file}"), None),
        None => return Err(format!("{name}.review-{number} is beside the repository, on {branch} -- open a file in it to review there")),
    }
    done(Done::Say(Ok(format!("reviewing {} in {name}.review-{number}", client.project()))));
    Ok(())
}

/// `c`: checks the request out in this checkout, as `mr-N`.
async fn check_out(root: String, number: u64) -> Result<(), String> {
    let client = client::for_repo(&root).await?;
    let remote = default_remote(&root).await.ok_or("no remote to fetch the request from")?;
    proc::git(&root, &["fetch", &remote, &client.checkout_refspec(number)]).await.map_err(|e| format!("couldn't fetch it: {e}"))?;
    let branch = format!("mr-{number}");
    proc::git(&root, &["checkout", &branch]).await.map_err(|e| format!("couldn't check out {branch}: {e}"))?;
    forge::refresh();
    done(Done::Say(Ok(format!("checked out {branch}"))));
    Ok(())
}

fn browse(url: &str) {
    if let Err(e) = api::open_url(url) {
        error(&e);
    }
}

impl ReviewExtension {
    fn open_inbox(&mut self, root: String, forge: String, project: String) {
        let found = self.pages.iter().find_map(|(id, p)| match p {
            Open::Inbox(i) if i.root == Path::new(&root) => Some(*id),
            _ => None,
        });
        let id = match found {
            Some(id) => {
                api::page::focus(id);
                id
            }
            None => {
                let id = api::page::open(INBOX_KIND, &format!("*reviews: {project}*"));
                self.pages.insert(id, Open::Inbox(Box::new(Inbox::new(PathBuf::from(&root), forge, project))));
                id
            }
        };
        self.inbox_refresh(id);
    }

    fn inbox_refresh(&mut self, id: u64) {
        let Some(Open::Inbox(inbox)) = self.pages.get_mut(&id) else { return };
        inbox.loading = true;
        let (root, filter, project) = (inbox.root.to_string_lossy().into_owned(), inbox.filter, inbox.project.clone());
        api::page::refresh(id);
        run::spawn(async move {
            let result = async {
                let client = client::for_repo(&root).await?;
                let list = client.list_merge_requests(filter).await?;
                Ok(list
                    .into_iter()
                    .map(|request| {
                        let state = review_store::load(Path::new(review_store::REVIEWS), old_reviews(&root).as_deref(), &project, request.number);
                        let new = state.seen.as_deref() != Some(request.updated_at.as_str());
                        Entry { pending: state.pending.len(), new, request }
                    })
                    .collect())
            }
            .await;
            done(Done::Inbox { page: id, result });
        });
    }

    fn inbox_action(&mut self, id: u64, action: InboxAction) {
        let Some(Open::Inbox(inbox)) = self.pages.get(&id) else { return };
        let (root, forge, project) = (inbox.root.to_string_lossy().into_owned(), inbox.forge.clone(), inbox.project.clone());
        match action {
            InboxAction::None => {}
            InboxAction::Refresh => self.inbox_refresh(id),
            InboxAction::Close => self.close(id),
            InboxAction::Open(number) => self.open_review(root, forge, project, number),
            InboxAction::Worktree(number) => job(review_in_worktree(root, number)),
            InboxAction::CheckOut(number) => job(check_out(root, number)),
            InboxAction::Browser(url) => browse(&url),
        }
    }

    /// The review page for request `number` of the repository at `root`.
    fn open_review(&mut self, root: String, forge: String, project: String, number: u64) {
        let found = self.pages.iter().find_map(|(id, p)| match p {
            Open::Review(r) if r.root == Path::new(&root) && r.number == number => Some(*id),
            _ => None,
        });
        let id = match found {
            Some(id) => {
                api::page::focus(id);
                id
            }
            None => {
                let state = review_store::load(Path::new(review_store::REVIEWS), old_reviews(&root).as_deref(), &project, number);
                let mut page = ReviewPage::new(PathBuf::from(&root), forge, project, number, state);
                // A worktree made for reviewing it (`name.review-N`).
                page.in_worktree = proc_worktree_name(&root, number);
                let id = api::page::open(REVIEW_KIND, &format!("*review: {}*", page.reference()));
                self.pages.insert(id, Open::Review(Box::new(page)));
                id
            }
        };
        self.review_refresh(id);
    }

    fn review(&mut self, id: u64) -> Option<&mut ReviewPage> {
        match self.pages.get_mut(&id) {
            Some(Open::Review(r)) => Some(r),
            _ => None,
        }
    }

    fn review_refresh(&mut self, id: u64) {
        let Some(page) = self.review(id) else { return };
        page.loading = true;
        let (root, number) = (page.root.to_string_lossy().into_owned(), page.number);
        api::page::refresh(id);
        run::spawn(async move {
            let result = async {
                let client = client::for_repo(&root).await?;
                read_review(&client, &root, number).await.map(Box::new)
            }
            .await;
            done(Done::Review { page: id, result });
        });
    }

    /// A write to the forge for review page `id`, then `after`.
    fn review_write<F, Fut>(&mut self, id: u64, label: &str, after: After, write: F)
    where
        F: FnOnce(Client) -> Fut + 'static,
        Fut: Future<Output = Result<(), String>> + 'static,
    {
        let Some(page) = self.review(id) else { return };
        if let Some(busy) = &page.busy {
            page.message = Some((format!("still busy with {busy}"), true));
            return;
        }
        page.busy = Some(label.to_string());
        let root = page.root.to_string_lossy().into_owned();
        let label = label.to_string();
        api::page::refresh(id);
        run::spawn(async move {
            let result = match client::for_repo(&root).await {
                Ok(client) => write(client).await,
                Err(e) => Err(e),
            };
            done(Done::Wrote { page: id, label, after, result });
        });
    }

    fn save_review(&mut self, id: u64) {
        let Some(page) = self.review(id) else { return };
        if let Err(err) = review_store::save(Path::new(review_store::REVIEWS), &page.project, page.number, &page.state) {
            page.message = Some((format!("couldn't remember the review: {err}"), true));
        }
    }

    fn review_action(&mut self, id: u64, action: ReviewAction) {
        let Some(page) = self.review(id) else { return };
        let (root, number) = (page.root.to_string_lossy().into_owned(), page.number);
        let url = page.data.as_ref().map(|d| d.request.web_url.clone()).unwrap_or_default();
        match action {
            ReviewAction::None => {}
            ReviewAction::Refresh => self.review_refresh(id),
            ReviewAction::Close => self.close(id),
            ReviewAction::Save => self.save_review(id),
            ReviewAction::Comment { pending, index } => {
                let title = format!("Comment on {} (held for your review)", pending.path);
                let c = api::compose(&title, "keep", &pending.body, false);
                self.composing.insert(c, Compose::Pending { page: id, pending: Box::new(pending), index });
            }
            ReviewAction::Reply(thread) => {
                let c = api::compose("Reply", "send", "", false);
                self.composing.insert(c, Compose::Reply { page: id, thread });
            }
            ReviewAction::Summary => {
                let c = api::compose("Review summary", "keep", &page.summary, true);
                self.composing.insert(c, Compose::Summary { page: id });
            }
            ReviewAction::Resolve { thread, resolved } => {
                let label = if resolved { "resolve the thread" } else { "reopen the thread" };
                self.review_write(id, label, After::Refresh, move |c| async move { c.resolve(number, &thread, resolved).await });
            }
            ReviewAction::Submit { verdict, body } => {
                let Some(data) = &page.data else { return };
                let head = data.request.sha.clone();
                let drafts: Vec<_> = page.state.pending.iter().map(|p| p.draft(&data.request.diff_refs)).collect();
                if drafts.is_empty() && body.trim().is_empty() && verdict == Verdict::Comment {
                    page.message = Some(("nothing to send -- write a comment (C) or a summary (e), or pick approve".to_string(), true));
                    return;
                }
                let label = format!("submit your review ({})", verdict.label());
                let sent = head.clone();
                self.review_write(id, &label, After::Submitted(head), move |c| async move { c.submit_review(number, &sent, verdict, &body, &drafts).await });
            }
            ReviewAction::Merge(options) => self.review_write(id, "merge", After::Merged, move |c| async move { c.merge(number, &options).await }),
            ReviewAction::OpenFile { path, line } => {
                let full = format!("{}/{path}", root.trim_end_matches('/'));
                host::open_file(&full, line.map(|l| l as u32));
            }
            ReviewAction::OpenBrowser => browse(&url),
            ReviewAction::CopyUrl => {
                api::copy(&url);
                api::notify("copied the link");
            }
            ReviewAction::ReviewInWorktree => job(review_in_worktree(root, number)),
            ReviewAction::CheckOutHere => job(check_out(root, number)),
            ReviewAction::ShowLog(check) => run::spawn(async move {
                let result = match client::for_repo(&root).await {
                    Ok(c) => c.job_log(&check).await,
                    Err(e) => Err(e),
                };
                done(Done::Log { page: id, name: check.name.clone(), result });
            }),
            ReviewAction::Retry(check) => {
                let label = format!("rerun {}", check.name);
                self.review_write(id, &label, After::Refresh, move |c| async move { c.retry(&check).await });
            }
            ReviewAction::LoadSince => {
                let Some(data) = &page.data else { return };
                let Some(reviewed) = page.state.reviewed_head.clone() else { return };
                let head = data.request.sha.clone();
                let paths: Vec<String> = data.files.iter().flat_map(|f| [f.path.clone(), f.old_path.clone()]).collect();
                run::spawn(async move {
                    let result = read_since(&root, &reviewed, &head, &paths).await;
                    done(Done::Since { page: id, result });
                });
            }
        }
    }

    fn request(&mut self, id: u64) -> Option<&mut RequestPage> {
        match self.pages.get_mut(&id) {
            Some(Open::Request(r)) => Some(r),
            _ => None,
        }
    }

    fn open_request(&mut self, mut page: RequestPage) {
        if let Some(draft) = request_drafts::load(Path::new(request_drafts::REQUESTS), &page.project, &page.branch) {
            page.restore(draft);
        }
        let found = self.pages.iter().find_map(|(id, p)| match p {
            Open::Request(r) if r.root == page.root && r.branch == page.branch => Some(*id),
            _ => None,
        });
        if let Some(id) = found {
            return api::page::focus(id);
        }
        let (root, branch) = (page.root.to_string_lossy().into_owned(), page.branch.clone());
        let id = api::page::open(REQUEST_KIND, &format!("*new request: {}*", page.branch));
        self.pages.insert(id, Open::Request(Box::new(page)));
        run::spawn(async move {
            let found = match client::for_repo(&root).await {
                Ok(client) => {
                    let result = client.request_for_branch(&branch).await;
                    let me = client.current_user().await.ok();
                    let template = client.default_description().await.ok().flatten();
                    Done::Existing { page: id, result, me, template }
                }
                Err(e) => Done::Existing { page: id, result: Err(e), me: None, template: None },
            };
            done(found);
        });
    }

    fn request_action(&mut self, id: u64, action: RequestAction) {
        let Some(page) = self.request(id) else { return };
        let root = page.root.to_string_lossy().into_owned();
        match action {
            RequestAction::None => {}
            RequestAction::Close => self.close(id),
            RequestAction::EditDescription => {
                // Named for the branch: Fenix keeps a compose pane's unsent
                // text by its name, for the next one.
                let c = api::compose(&format!("Description of {}", page.branch), "keep", &page.description, true);
                self.composing.insert(c, Compose::Description { page: id });
            }
            RequestAction::Browser(url) => browse(&url),
            // The whole branch against its base: Fenix's Git compare page,
            // its base picked there.
            RequestAction::Compare(base) => {
                host::run_command("git.compare");
                api::notify(&format!("pick origin/{base} to see the whole branch against it"));
            }
            RequestAction::Review(number) => {
                let (forge, project) = (page.forge.clone(), page.project.clone());
                self.close(id);
                self.open_review(root, forge, project, number);
            }
            RequestAction::Open { request, reviewers, push } => {
                page.busy = true;
                page.message = None;
                api::page::refresh(id);
                run::spawn(async move {
                    let result = open_new(&root, request, reviewers, push).await;
                    done(Done::Opened { page: id, result });
                });
            }
        }
    }

    fn close(&mut self, id: u64) {
        self.pages.remove(&id);
        api::page::close(id);
    }

    /// The request form `page` as it stands, kept for its branch -- or,
    /// as it was filled in, nothing kept.
    fn keep_request_draft(&mut self, page: u64) {
        let Some(Open::Request(r)) = self.pages.get(&page) else { return };
        let base = Path::new(request_drafts::REQUESTS);
        match r.draft() {
            Some(d) => {
                let _ = request_drafts::save(base, &r.project, &r.branch, &d);
            }
            None => request_drafts::clear(base, &r.project, &r.branch),
        }
    }
    /// A compose pane's text, sent where it was written for.
    fn composed(&mut self, id: u64, text: Option<String>) {
        let (Some(purpose), Some(body)) = (self.composing.remove(&id), text) else { return };
        match purpose {
            Compose::Pending { page, pending, index } => {
                let Some(r) = self.review(page) else { return };
                let pending = Pending { body, ..*pending };
                match index {
                    Some(i) if i < r.state.pending.len() => r.state.pending[i] = pending,
                    _ => r.state.pending.push(pending),
                }
                r.message = Some(("held for your review -- s submits it with the rest".to_string(), false));
                self.save_review(page);
                api::page::focus(page);
                api::page::refresh(page);
            }
            Compose::Reply { page, thread } => {
                api::page::focus(page);
                let Some(number) = self.review(page).map(|r| r.number) else { return };
                self.review_write(page, "reply", After::Refresh, move |c| async move { c.reply(number, &thread, &body).await });
            }
            Compose::Summary { page } => {
                if let Some(r) = self.review(page) {
                    r.summary = body;
                }
                api::page::focus(page);
                api::page::refresh(page);
            }
            Compose::Description { page } => {
                if let Some(r) = self.request(page) {
                    r.set_description(body);
                }
                self.keep_request_draft(page);
                api::page::focus(page);
                api::page::refresh(page);
            }
        }
    }

    /// What the jobs found, onto their pages.
    fn drain(&mut self) {
        loop {
            let found = DONE.with(|q| std::mem::take(&mut *q.borrow_mut()));
            if found.is_empty() {
                break;
            }
            for d in found {
                self.apply(d);
            }
        }
    }

    fn apply(&mut self, d: Done) {
        match d {
            Done::Inbox { page, result } => {
                let Some(Open::Inbox(inbox)) = self.pages.get_mut(&page) else { return };
                match result {
                    Ok(entries) => inbox.set_entries(entries),
                    Err(err) => {
                        inbox.loading = false;
                        inbox.entries.get_or_insert_with(Vec::new);
                        inbox.message = Some((err, true));
                    }
                }
                api::page::refresh(page);
            }
            Done::Review { page, result } => {
                let Some(r) = self.review(page) else { return };
                match result {
                    Ok(data) => {
                        r.state.seen = Some(data.request.updated_at.clone());
                        r.set_data(*data);
                        host::page_retitle(page, &format!("*review: {}*", r.reference()));
                        self.save_review(page);
                    }
                    Err(err) => {
                        r.loading = false;
                        r.message = Some((err, true));
                    }
                }
                api::page::refresh(page);
            }
            Done::Since { page, result } => {
                let Some(r) = self.review(page) else { return };
                match result {
                    Ok(files) => {
                        if let Some(data) = &mut r.data {
                            data.since = Some(files);
                        }
                    }
                    Err(err) => {
                        r.since_review = false;
                        r.message = Some((err, true));
                    }
                }
                api::page::refresh(page);
            }
            Done::Wrote { page, label, after, result } => {
                let Some(r) = self.review(page) else { return };
                r.busy = None;
                match result {
                    Err(err) => r.message = Some((format!("{label} failed: {err}"), true)),
                    Ok(()) => {
                        r.message = Some((format!("{label} ✓"), false));
                        match after {
                            After::Submitted(head) => {
                                r.state.pending.clear();
                                r.state.reviewed_head = Some(head);
                                r.summary.clear();
                                self.save_review(page);
                            }
                            After::Merged => {
                                if r.in_worktree {
                                    r.message = Some(("merged ✓ -- q closes this; the review worktree can go (w d on the Git page)".to_string(), false));
                                }
                                forge::refresh();
                            }
                            After::Refresh => {}
                        }
                        self.review_refresh(page);
                    }
                }
                api::page::refresh(page);
            }
            Done::Log { page, name, result } => match result {
                Ok(log) => {
                    let lines: Vec<&str> = log.lines().collect();
                    let tail: Vec<&str> = lines[lines.len().saturating_sub(400)..].iter().map(|l| without_timestamp(l)).collect();
                    api::open_text(&format!("{name}: its log"), &format!("{name} -- the last {} lines of its log\n\n{}", tail.len(), tail.join("\n")), None);
                }
                Err(err) => {
                    if let Some(r) = self.review(page) {
                        r.message = Some((format!("couldn't read {name}'s log: {err}"), true));
                        api::page::refresh(page);
                    }
                }
            },
            Done::Existing { page, result, me, template } => {
                let Some(r) = self.request(page) else { return };
                if let Some(me) = me {
                    r.assign_to_me(&me);
                }
                if let Some(template) = template {
                    r.add_forge_template(template);
                }
                match result {
                    Ok(found) => r.existing = Some(found),
                    Err(err) => {
                        r.existing = Some(None);
                        r.message = Some((format!("couldn't ask {} whether one is open already: {err}", r.forge), true));
                    }
                }
                api::page::refresh(page);
            }
            Done::Opened { page, result } => {
                let Some(r) = self.request(page) else { return };
                r.busy = false;
                match result {
                    Ok((made, warning)) => {
                        request_drafts::clear(Path::new(request_drafts::REQUESTS), &r.project, &r.branch);
                        self.close(page);
                        let opened = format!("opened {} {}", made.reference(), made.title);
                        match warning {
                            Some(warning) => error(&format!("{opened} -- {warning}")),
                            None => api::notify(&format!("{opened} ✓ -- o on the Git page reviews it")),
                        }
                        forge::refresh();
                    }
                    Err(err) => {
                        r.message = Some((err, true));
                        api::page::refresh(page);
                    }
                }
            }
            Done::Say(Ok(text)) => api::notify(&text),
            Done::Say(Err(text)) => error(&text),
            Done::OpenInbox { root, forge, project } => self.open_inbox(root, forge, project),
            Done::OpenReview { root, forge, project, number } => self.open_review(root, forge, project, number),
            Done::OpenRequest(page) => self.open_request(*page),
        }
    }

    /// `forge.branch`, `forge.open`, `forge.reviews`: Fenix's Git pages
    /// asking. `forge.branch` is answered when the forge has.
    fn forge_call(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let repo = match forge::call(name, payload)? {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };
        match repo {
            forge::Call::Branch(repo) => {
                let Some(branch) = repo.branch else { return Some(Ok(forge::branch_answer(None))) };
                run::spawn(async move {
                    let answer = branch_line(&repo.root, &branch).await.map(|line| forge::branch_answer(line.as_ref()));
                    api::answer(call, answer);
                });
                None
            }
            forge::Call::Open(repo) => {
                let root = repo.root;
                match repo.number {
                    Some(number) => job(async move {
                        let client = client::for_repo(&root).await?;
                        done(Done::OpenReview { forge: client.name().into(), project: client.project().into(), root, number });
                        Ok(())
                    }),
                    None => job(new_request(root)),
                }
                Some(Ok(String::new()))
            }
            forge::Call::Reviews(repo) => {
                job(reviews_at(repo.root));
                Some(Ok(String::new()))
            }
        }
    }
}

/// The git dir 1.x kept reviews in (`.git/fenix/reviews`), when it can be
/// read: the project's.
fn old_reviews(root: &str) -> Option<PathBuf> {
    let git = Path::new(root).join(".git");
    (root == PROJECT && git.is_dir()).then_some(git)
}

/// Whether the repository at `root` is the worktree `w` made for request
/// `number`.
fn proc_worktree_name(root: &str, number: u64) -> bool {
    root.trim_end_matches(['/', '\\']).ends_with(&format!(".review-{number}"))
}

/// The branch's request, for the Git status page's header.
async fn branch_line(root: &str, branch: &str) -> Result<Option<forge::Request>, String> {
    let client = client::for_repo(root).await?;
    let Some(r) = client.request_for_branch(branch).await? else { return Ok(None) };
    let checks = client.checks(r.number, &r.sha).await.ok().and_then(|c| fenix_forge::github::parse::overall(&c)).or(r.pipeline.clone());
    let approved_by = client.approvals(r.number).await.map(|a| a.approved_by).unwrap_or_default();
    let unresolved = client.discussions(r.number).await.map(|d| d.iter().filter(|d| d.resolvable && !d.resolved).count()).unwrap_or(0) as u32;
    Ok(Some(forge::Request { number: r.number, reference: r.reference(), title: r.title.clone(), draft: r.draft, checks: checks.map(|c| c.label().to_string()), approved_by, unresolved }))
}

/// A new request opened, the branch pushed first if it's to be: what was
/// opened, and a warning if asking for reviewers didn't work.
async fn open_new(root: &str, request: fenix_forge::NewRequest, reviewers: Vec<String>, push: bool) -> Result<(MergeRequest, Option<String>), String> {
    let client = client::for_repo(root).await?;
    if push {
        let upstream = proc::git(root, &["rev-parse", "--abbrev-ref", "@{u}"]).await.ok();
        let remote = match upstream.as_deref().and_then(|u| u.trim().split('/').next()) {
            Some(r) => r.to_string(),
            None => default_remote(root).await.ok_or("no remote to push the branch to -- git remote add origin <url>")?,
        };
        let mut args = vec!["push"];
        if upstream.is_none() {
            args.push("-u");
        }
        args.extend([remote.as_str(), request.source_branch.as_str()]);
        proc::git(root, &args).await.map_err(|e| format!("the push failed, so nothing was opened: {e}"))?;
    }
    let made = client.create_request(&request).await?;
    // No forge lets you review your own.
    let me = client.current_user().await.unwrap_or_default();
    let reviewers: Vec<String> = reviewers.into_iter().filter(|r| !r.eq_ignore_ascii_case(&me)).collect();
    let mut warning = None;
    if !reviewers.is_empty() {
        if let Err(err) = client.request_review(made.number, &reviewers).await {
            warning = Some(format!("but asking {} to review didn't work: {err}", reviewers.join(", ")));
        }
    }
    Ok((made, warning))
}

impl Extension for ReviewExtension {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            // `SPC g M`: the reviews for the repository you're in.
            "git.merge_requests" => job(async { reviews_at(repo_here().await?).await }),
            // `SPC g P`: a pull request for the branch.
            "git.pull_request" => job(async { new_request(repo_here().await?).await }),
            "git.merge_requests_close" => {
                let inboxes: Vec<u64> = self.pages.iter().filter(|(_, p)| matches!(p, Open::Inbox(_))).map(|(id, _)| *id).collect();
                for id in inboxes {
                    self.close(id);
                }
            }
            other => return Err(format!("no command {other}")),
        }
        self.drain();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        if let Some(Event::Composed((id, text))) = proc::deliver(event).and_then(run::deliver) {
            self.composed(id, text);
        }
        self.drain();
    }

    fn page(&mut self, page: u64, width: u32, _height: u32) -> Page {
        let width = width as usize;
        match self.pages.get(&page) {
            Some(Open::Inbox(p)) => Page::canvas(review_inbox::layout(p, width)),
            Some(Open::Review(p)) => Page::canvas(review_page::layout(p, width)),
            Some(Open::Request(p)) => Page::canvas(git_request::layout(p, width)).typing(p.editing.is_some()),
            None => Page::new(),
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let key: page::Key = key.into();
        match self.pages.get_mut(&page) {
            Some(Open::Inbox(p)) => {
                let action = p.key(key);
                self.inbox_action(page, action);
            }
            Some(Open::Review(p)) => {
                let action = p.key(key);
                self.review_action(page, action);
            }
            Some(Open::Request(p)) => {
                let action = p.key(key);
                self.request_action(page, action);
                self.keep_request_draft(page);
            }
            None => {}
        }
        api::page::refresh(page);
        self.drain();
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(Open::Request(p)) = self.pages.get_mut(&page) {
            p.paste(text);
            api::page::refresh(page);
            self.keep_request_draft(page);
        }
    }



    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let answer = self.forge_call(call, name, payload);
        self.drain();
        answer
    }
}

export!(ReviewExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortstat_is_read() {
        assert_eq!(shortstat(" 3 files changed, 12 insertions(+), 4 deletions(-)"), (3, 12, 4));
        assert_eq!(shortstat(" 1 file changed, 1 deletion(-)"), (1, 0, 1));
        assert_eq!(shortstat(""), (0, 0, 0));
    }

    #[test]
    fn a_ci_timestamp_comes_off_but_an_ordinary_line_is_left_alone() {
        assert_eq!(without_timestamp("2026-09-24T20:28:05.1234567Z src/a.rs:3: error"), "src/a.rs:3: error");
        assert_eq!(without_timestamp("src/a.rs:3: error"), "src/a.rs:3: error");
    }

    #[test]
    fn a_diff_is_split_by_file() {
        let text = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+x\n";
        let files = split_diff(text);
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].0.as_str(), files[0].2), ("src/a.rs", 'M'));
        assert!(files[0].3.contains("+b"));
        assert_eq!((files[1].0.as_str(), files[1].2), ("new.txt", 'A'));
    }

    #[test]
    fn the_worktree_made_for_a_review_is_known() {
        assert!(proc_worktree_name("C:/src/fenix.review-42", 42));
        assert!(!proc_worktree_name("C:/src/fenix", 42));
    }
}
