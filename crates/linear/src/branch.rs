//! A branch started from an issue: its name (Linear's suggestion,
//! `branchName`, or the person's own pattern), whether it's a name `git`
//! would take and nothing could mistake for an option, and what to do in
//! the repository you're in, which depends on what's there.
//!
//! The rules (they're tested against a real repository):
//!
//! - Not in a git repository: nothing is run, and it says so.
//! - Already on that branch: nothing is done.
//! - The branch exists: it's switched to -- never recreated or reset --
//!   and only when the working tree is clean; with changes, it says
//!   to commit or stash first.
//! - It doesn't exist: it's created from where you are (`HEAD`; no
//!   fetch, no network) and switched to. Creating a branch changes no
//!   file, so a dirty tree comes along; but never silently: it asks
//!   first, saying how many files changed, and only an answer of yes
//!   (`confirmed`) goes on.
//!
//! **Every `git` this runs, exactly** (`name` is checked by [`check_name`]
//! first, and nothing else from the network or a project's settings is ever
//! an argument; the working folder is the project's, `/project`):
//!
//! ```text
//! git rev-parse --show-toplevel
//! git check-ref-format refs/heads/<name>
//! git status --porcelain
//! git symbolic-ref --short -q HEAD
//! git show-ref --verify --quiet -- refs/heads/<name>
//! git switch --end-of-options <name>        (the branch exists)
//! git switch -c <name> --                   (it doesn't)
//! ```
//!
//! A name is only ever after `refs/heads/`, after `--end-of-options`, or
//! the value of `-c`, which git takes whole, so none can be read as an
//! option, as well as never starting with `-` (`check_name`).
//!
//! The host runs the programs; this is the logic around them, over any
//! way of running `git` ([`Run`]).

use std::future::Future;

/// What a `git` run came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// How a branch is made of an issue: Linear's suggestion, or the
/// person's pattern with `{id}` (`eng-123`), `{ID}` (`ENG-123`), `{team}`
/// (`eng`), `{number}` (`123`) and `{slug}` (the title as a slug, at most
/// 50 characters) in it.
pub fn name_for(suggested: &str, pattern: &str, identifier: &str, title: &str) -> Result<String, String> {
    let pattern = pattern.trim();
    let name = if pattern.is_empty() {
        if suggested.trim().is_empty() {
            return Err("Linear gave no branch name for this issue -- set one in linear.branch_pattern".into());
        }
        suggested.trim().to_string()
    } else {
        let (team, number) = identifier.rsplit_once('-').unwrap_or((identifier, ""));
        pattern.replace("{ID}", identifier).replace("{id}", &identifier.to_lowercase()).replace("{team}", &team.to_lowercase()).replace("{number}", number).replace("{slug}", &slug(title, 50))
    };
    check_name(&name)?;
    Ok(name)
}

/// A title as a slug: lower case letters and digits, one `-` between words.
pub fn slug(title: &str, max: usize) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.chars().take(max).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// A name this extension will hand to `git`: what Git itself allows, and
/// less -- no leading `-` (so it can't be an option), no control
/// characters or spaces, no `..`, `@{`, `//`, `\`, `~^:?*[`, no `.lock`
/// ending, not `@`, at most 200 bytes. Git is asked too
/// (`check-ref-format`), but this is the part that doesn't depend on it.
pub fn check_name(name: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("\"{}\" isn't a branch name I'll use: {why}", name.chars().take(60).collect::<String>()));
    if name.is_empty() {
        return bad("it's empty");
    }
    if name.len() > 200 {
        return bad("it's too long");
    }
    if name.starts_with('-') || name.starts_with('/') || name.starts_with('.') || name.ends_with('/') || name.ends_with('.') || name.ends_with(".lock") || name == "@" {
        return bad("it starts or ends oddly");
    }
    if name.contains("..") || name.contains("@{") || name.contains("//") || name.contains("/.") {
        return bad("it has a sequence git refuses");
    }
    if name.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c)) {
        return bad("it has a character git refuses");
    }
    Ok(())
}

/// What's true of the repository, asked of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub is_repo: bool,
    /// Files changed, staged or not, and untracked ones.
    pub changed: usize,
    /// The branch you're on; none on a detached `HEAD`.
    pub current: Option<String>,
    pub exists: bool,
}

/// What came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Said, and nothing done: not a repository, already there, a dirty tree
    /// and an existing branch, or a name git refuses.
    Said(String),
    /// Ask first: the tree has `changed` files changed, and creating the
    /// branch would carry them. `start` again with `confirmed`.
    Confirm { changed: usize },
    /// Done: switched to an existing branch, or made it and switched.
    Switched { name: String, created: bool },
}

fn files(n: usize) -> String {
    if n == 1 {
        "1 changed file".into()
    } else {
        format!("{n} changed files")
    }
}

/// What to do for branch `name` given `facts`: the `git` arguments to run,
/// or what to say instead.
pub fn decide(name: &str, facts: &Facts, confirmed: bool) -> Result<Vec<String>, Outcome> {
    if !facts.is_repo {
        return Err(Outcome::Said("this project isn't a git repository: nothing to make a branch in (the name is copied instead)".into()));
    }
    if facts.current.as_deref() == Some(name) {
        return Err(Outcome::Said(format!("already on {name}")));
    }
    if facts.exists {
        if facts.changed > 0 {
            return Err(Outcome::Said(format!("{name} exists, and the tree has {} -- commit or stash them, then try again", files(facts.changed))));
        }
        return Ok(vec!["switch".into(), "--end-of-options".into(), name.into()]);
    }
    if facts.changed > 0 && !confirmed {
        return Err(Outcome::Confirm { changed: facts.changed });
    }
    Ok(vec!["switch".into(), "-c".into(), name.into(), "--".into()])
}

/// Asks the repository what's true of it, for `name`. `run` runs `git`
/// with these arguments in the project.
pub async fn look<R, F>(run: &R, name: &str) -> Result<Facts, String>
where
    R: Fn(Vec<String>) -> F,
    F: Future<Output = Result<Ran, String>>,
{
    let git = |args: &[&str]| run(args.iter().map(|a| a.to_string()).collect());
    if !git(&["rev-parse", "--show-toplevel"]).await.is_ok_and(|r| r.ok) {
        return Ok(Facts::default());
    }
    // Git has the last word on a name.
    // The full ref, never the bare name: `refs/heads/...` can't be read as an option.
    if !git(&["check-ref-format", &format!("refs/heads/{name}")]).await.is_ok_and(|r| r.ok) {
        return Err(format!("git won't take \"{}\" as a branch name", name.chars().take(60).collect::<String>()));
    }
    let status = git(&["status", "--porcelain"]).await?;
    if !status.ok {
        return Err(first_line(&status.stderr, "git status failed"));
    }
    let current = git(&["symbolic-ref", "--short", "-q", "HEAD"]).await?;
    let exists = git(&["show-ref", "--verify", "--quiet", "--", &format!("refs/heads/{name}")]).await?;
    Ok(Facts { is_repo: true, changed: status.stdout.lines().filter(|l| !l.trim().is_empty()).count(), current: Some(current.stdout.trim().to_string()).filter(|c| current.ok && !c.is_empty()), exists: exists.ok })
}

fn first_line(text: &str, otherwise: &str) -> String {
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or(otherwise).trim().to_string()
}

/// Starts the branch for an issue in the repository `run` reaches.
pub async fn start<R, F>(run: R, suggested: &str, pattern: &str, identifier: &str, title: &str, confirmed: bool) -> Result<Outcome, String>
where
    R: Fn(Vec<String>) -> F,
    F: Future<Output = Result<Ran, String>>,
{
    let name = name_for(suggested, pattern, identifier, title)?;
    let facts = look(&run, &name).await?;
    let args = match decide(&name, &facts, confirmed) {
        Ok(args) => args,
        Err(outcome) => return Ok(outcome),
    };
    let ran = run(args).await?;
    if !ran.ok {
        return Err(first_line(&ran.stderr, "git couldn't switch"));
    }
    Ok(Outcome::Switched { name, created: !facts.exists })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A repository of the test's own, in a folder of its own, gone with it.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Repo {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("fenix-linear-{name}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            let r = Repo(path);
            r.git(&["init", "-q", "-b", "main"]);
            r.git(&["config", "user.email", "t@example.com"]);
            r.git(&["config", "user.name", "T"]);
            r.git(&["config", "commit.gpgsign", "false"]);
            std::fs::write(r.0.join("a.txt"), "a\n").unwrap();
            r.git(&["add", "."]);
            r.git(&["commit", "-q", "-m", "first"]);
            r
        }

        fn git(&self, args: &[&str]) -> Ran {
            run_in(&self.0, args.iter().map(|a| a.to_string()).collect())
        }

        fn branch(&self) -> String {
            self.git(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout.trim().to_string()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn run_in(dir: &Path, args: Vec<String>) -> Ran {
        let out = Command::new("git").args(&args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").output().expect("git");
        Ran { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).into(), stderr: String::from_utf8_lossy(&out.stderr).into() }
    }

    fn go(dir: &Path, name: &str, confirmed: bool) -> Result<Outcome, String> {
        let dir = dir.to_path_buf();
        futures_executor::block_on(start(move |args| std::future::ready(Ok(run_in(&dir, args))), name, "", "ENG-12", "Fix the login", confirmed))
    }

    /// Runs `git` in the repository and writes every argv down.
    fn recording(dir: &Path, seen: &std::cell::RefCell<Vec<Vec<String>>>, args: Vec<String>) -> std::future::Ready<Result<Ran, String>> {
        seen.borrow_mut().push(args.clone());
        std::future::ready(Ok(run_in(dir, args)))
    }

    #[test]
    fn every_git_call_has_its_exact_argv_and_a_name_is_never_an_option() {
        let repo = Repo::new("argv");
        let seen = std::cell::RefCell::new(Vec::new());
        let dir = repo.0.clone();
        let out = futures_executor::block_on(start(|a| recording(&dir, &seen, a), "jo/eng-12-x", "", "ENG-12", "t", false));
        assert_eq!(out, Ok(Outcome::Switched { name: "jo/eng-12-x".into(), created: true }));
        let argv: Vec<String> = seen.borrow().iter().map(|a| a.join(" ")).collect();
        assert_eq!(
            argv,
            [
                "rev-parse --show-toplevel",
                "check-ref-format refs/heads/jo/eng-12-x",
                "status --porcelain",
                "symbolic-ref --short -q HEAD",
                "show-ref --verify --quiet -- refs/heads/jo/eng-12-x",
                "switch -c jo/eng-12-x --"
            ]
        );
        repo.git(&["switch", "-q", "main"]);
        seen.borrow_mut().clear();
        futures_executor::block_on(start(|a| recording(&dir, &seen, a), "jo/eng-12-x", "", "ENG-12", "t", false)).unwrap();
        assert_eq!(seen.borrow().last().unwrap().join(" "), "switch --end-of-options jo/eng-12-x");
        // A name that could be an option never reaches git at all, from Linear or from a pattern.
        for (suggested, pattern) in [("-D", ""), ("--force", ""), ("", "--{id}"), ("", "-c{slug}")] {
            seen.borrow_mut().clear();
            let out = futures_executor::block_on(start(|a| recording(&dir, &seen, a), suggested, pattern, "ENG-12", "-x", false));
            assert!(out.is_err() && seen.borrow().is_empty(), "{suggested:?} {pattern:?}: {out:?} {:?}", seen.borrow());
        }
        // Whatever a name is, every argv that holds it puts it where git can't read an option.
        for argv in [decide("n", &Facts { is_repo: true, ..Default::default() }, false).unwrap(), decide("n", &Facts { is_repo: true, exists: true, ..Default::default() }, false).unwrap()] {
            let at = argv.iter().position(|a| a == "n").unwrap();
            assert!(argv[..at].iter().any(|a| a == "--end-of-options") || argv[at - 1] == "-c", "{argv:?}");
        }
    }

    #[test]
    fn names_come_from_linear_or_a_pattern() {
        assert_eq!(name_for("jo/eng-12-fix-the-login", "", "ENG-12", "Fix the login").unwrap(), "jo/eng-12-fix-the-login");
        assert_eq!(name_for("ignored", "feature/{id}-{slug}", "ENG-12", "Fix the login!  (now)").unwrap(), "feature/eng-12-fix-the-login-now");
        assert_eq!(name_for("", "{team}/{number}-{ID}", "ENG-12", "x").unwrap(), "eng/12-ENG-12");
        assert!(name_for("", "", "ENG-12", "x").unwrap_err().contains("branch_pattern"));
        assert_eq!(slug("  Ünïcode, and   \"quotes\" {braces}  ", 50), "ünïcode-and-quotes-braces");
        assert_eq!(slug("a very long title indeed, long enough to be cut", 12), "a-very-long");
    }

    #[test]
    fn a_name_that_could_be_an_option_or_trouble_is_refused() {
        for bad in ["", "-D", "--force", "a b", "a..b", "a@{1}", "x/", "/x", "x.lock", ".x", "a\nb", "a~1", "a:b", "a\\b", "a//b", "a/.b", "@", "x.", "a?b", &"x".repeat(201)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        // A title that makes a bad name through a pattern is refused too.
        assert!(name_for("", "{slug}", "ENG-1", "--force").is_ok(), "slugs have no leading dash...");
        assert_eq!(name_for("", "{slug}", "ENG-1", "--force").unwrap(), "force");
        assert!(name_for("-D", "", "ENG-1", "x").is_err());
        assert!(check_name("jo/eng-12-fix-it_2").is_ok());
        assert!(check_name("feature/ünï").is_ok());
    }

    #[test]
    fn what_to_do_follows_what_is_there() {
        let repo = |changed, current: Option<&str>, exists| Facts { is_repo: true, changed, current: current.map(Into::into), exists };
        assert!(matches!(decide("b", &Facts::default(), true), Err(Outcome::Said(s)) if s.contains("isn't a git repository")));
        assert_eq!(decide("b", &repo(0, Some("b"), true), false), Err(Outcome::Said("already on b".into())));
        assert_eq!(decide("b", &repo(0, Some("main"), true), false), Ok(vec!["switch".into(), "--end-of-options".into(), "b".into()]));
        assert!(matches!(decide("b", &repo(2, Some("main"), true), true), Err(Outcome::Said(s)) if s.contains("2 changed files") && s.contains("stash")), "even confirmed: an existing branch isn't switched to over changes");
        assert_eq!(decide("b", &repo(0, Some("main"), false), false), Ok(vec!["switch".into(), "-c".into(), "b".into(), "--".into()]));
        assert_eq!(decide("b", &repo(3, Some("main"), false), false), Err(Outcome::Confirm { changed: 3 }));
        assert_eq!(decide("b", &repo(3, None, false), true), Ok(vec!["switch".into(), "-c".into(), "b".into(), "--".into()]));
    }

    #[test]
    fn a_clean_repository_gets_the_branch_and_then_just_switches() {
        let repo = Repo::new("clean");
        assert_eq!(go(&repo.0, "jo/eng-12-fix", false), Ok(Outcome::Switched { name: "jo/eng-12-fix".into(), created: true }));
        assert_eq!(repo.branch(), "jo/eng-12-fix");
        assert_eq!(go(&repo.0, "jo/eng-12-fix", false), Ok(Outcome::Said("already on jo/eng-12-fix".into())));
        repo.git(&["switch", "-q", "main"]);
        assert_eq!(go(&repo.0, "jo/eng-12-fix", false), Ok(Outcome::Switched { name: "jo/eng-12-fix".into(), created: false }), "it exists: switched to, not made again");
        assert_eq!(repo.branch(), "jo/eng-12-fix");
    }

    #[test]
    fn a_dirty_tree_is_asked_about_and_never_touched_silently() {
        let repo = Repo::new("dirty");
        std::fs::write(repo.0.join("a.txt"), "changed\n").unwrap();
        std::fs::write(repo.0.join("new.txt"), "new\n").unwrap();
        assert_eq!(go(&repo.0, "eng-12", false), Ok(Outcome::Confirm { changed: 2 }));
        assert_eq!(repo.branch(), "main", "nothing was done");
        assert_eq!(go(&repo.0, "eng-12", true), Ok(Outcome::Switched { name: "eng-12".into(), created: true }));
        assert_eq!(repo.branch(), "eng-12");
        assert_eq!(std::fs::read_to_string(repo.0.join("a.txt")).unwrap(), "changed\n", "the changes came along, whole");
        // An existing branch is not switched to over changes, confirmed or not.
        repo.git(&["stash", "-q", "-u"]);
        repo.git(&["switch", "-q", "main"]);
        std::fs::write(repo.0.join("a.txt"), "dirty again\n").unwrap();
        assert!(matches!(go(&repo.0, "eng-12", true), Ok(Outcome::Said(s)) if s.contains("commit or stash")));
        assert_eq!(repo.branch(), "main");
    }

    #[test]
    fn not_a_repository_runs_nothing_but_the_check() {
        let dir = std::env::temp_dir().join(format!("fenix-linear-norepo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ran = Cell::new(0);
        let d = dir.clone();
        let outcome = futures_executor::block_on(start(
            |args| {
                ran.set(ran.get() + 1);
                let mut ran = run_in(&d, args);
                // The temp folder may itself sit in a repository on someone's machine.
                ran.ok = false;
                std::future::ready(Ok(ran))
            },
            "eng-12",
            "",
            "ENG-12",
            "x",
            true,
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(outcome, Ok(Outcome::Said(s)) if s.contains("isn't a git repository")));
        assert_eq!(ran.get(), 1, "only rev-parse was run");
    }

    #[test]
    fn git_has_the_last_word_on_a_name() {
        let repo = Repo::new("refname");
        // This crate's check passes it; git refuses a component ending in .lock.
        assert!(check_name("feature/x.lock/y").is_ok());
        let outcome = go(&repo.0, "feature/x.lock/y", false);
        assert!(matches!(&outcome, Err(e) if e.contains("git won't take")), "{outcome:?}");
        assert_eq!(repo.branch(), "main");
    }
}
