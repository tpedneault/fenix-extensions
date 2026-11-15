//! A branch started from an issue: its name, from a pattern, and whether
//! the project you're in is where the issue lives and in a state to make
//! one -- worked out from what `git` printed, so the rules are tested
//! without a repository.

use fenix_forge::issues::{Host, Issue, IssueRef};

/// The pattern used when the setting is empty.
pub const DEFAULT_PATTERN: &str = "{number}-{title}";

/// The most a branch name's title part is, in characters.
const TITLE: usize = 40;

/// `Fix the login: 5 minutes!` as `fix-the-login-5-minutes`: lower-case
/// ASCII letters and digits, a dash between words, at most `max`, cut at
/// a word.
pub fn slug(text: &str, max: usize) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        let c = fold(c);
        if c.is_ascii_alphanumeric() {
            word.push(c.to_ascii_lowercase());
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let mut out = String::new();
    for w in words {
        let extra = if out.is_empty() { w.len() } else { w.len() + 1 };
        if out.len() + extra > max {
            if out.is_empty() {
                out = w[..max.min(w.len())].to_string();
            }
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&w);
    }
    out
}

/// A letter with an accent as the plain one, for names: `é` as `e`.
fn fold(c: char) -> char {
    match c {
        'à'..='å' | 'À'..='Å' => 'a',
        'è'..='ë' | 'È'..='Ë' => 'e',
        'ì'..='ï' | 'Ì'..='Ï' => 'i',
        'ò'..='ö' | 'Ò'..='Ö' | 'ø' | 'Ø' => 'o',
        'ù'..='ü' | 'Ù'..='Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ý' | 'ÿ' | 'Ý' => 'y',
        other => other,
    }
}

/// The branch name `pattern` makes for `issue`: `{number}`, `{title}`
/// (a slug), `{project}` (the repository's or project's last part), and
/// `{type}`-free text as written. An empty pattern is the default.
pub fn name(pattern: &str, issue: &Issue) -> String {
    let pattern = if pattern.trim().is_empty() { DEFAULT_PATTERN } else { pattern.trim() };
    let project = issue.key.project.rsplit('/').next().unwrap_or_default();
    let raw = pattern.replace("{number}", &issue.key.number.to_string()).replace("{title}", &slug(&issue.title, TITLE)).replace("{project}", &slug(project, TITLE));
    clean_name(&raw)
}

/// What git would refuse taken out of a typed or made name: spaces,
/// `~^:?*[\`, `..`, `@{`, a leading `-` or `/`, a trailing `.` `/` or
/// `.lock`. Git's own check (`check-ref-format`) still has the last word.
pub fn clean_name(raw: &str) -> String {
    let mut s: String = raw.trim().chars().map(|c| if c.is_whitespace() || "~^:?*[\\".contains(c) || c.is_control() { '-' } else { c }).collect();
    while s.contains("..") {
        s = s.replace("..", ".");
    }
    while s.contains("//") {
        s = s.replace("//", "/");
    }
    s = s.replace("@{", "-");
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let mut s = s.trim_start_matches(['-', '/', '.']).trim_end_matches(['/', '.', '-']).to_string();
    while let Some(stripped) = s.strip_suffix(".lock") {
        s = stripped.to_string();
    }
    s
}

/// A remote's host and path: `git@github.com:acme/rocket.git`,
/// `ssh://git@host:22/group/sub/project.git`, `https://host/a/b`.
pub fn remote_parts(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        (host.split(':').next().unwrap_or(host), path)
    } else {
        let (authority, path) = url.split_once(':')?;
        (authority.rsplit_once('@').map_or(authority, |(_, h)| h), path)
    };
    let path = path.trim_matches('/').trim_end_matches(".git").trim_matches('/');
    (!host.is_empty() && path.contains('/')).then(|| (host.to_ascii_lowercase(), path.to_string()))
}

/// The host of a web address: `https://github.com` is `github.com`.
fn web_host(web: &str) -> String {
    let rest = web.split_once("://").map_or(web, |(_, r)| r);
    rest.split(['/', ':']).next().unwrap_or_default().to_ascii_lowercase()
}

/// Whether `remote_url` is the repository or project `project` on the
/// forge at `web`. GitHub's host has to be the forge's (`github.com`, or
/// your Enterprise Server); GitLab's needn't, as Review takes it: one
/// instance is routinely cloned by another name (an SSH alias, a proxy).
pub fn same_place(host: Host, web: &str, remote_url: &str, project: &str) -> bool {
    let Some((remote_host, path)) = remote_parts(remote_url) else { return false };
    if !path.eq_ignore_ascii_case(project) {
        return false;
    }
    match host {
        Host::GitHub => remote_host == web_host(web) || (web_host(web) == "github.com" && remote_host == "ssh.github.com"),
        Host::GitLab => true,
    }
}

/// `git remote -v`'s lines as (name, URL), each remote once.
pub fn remotes(output: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else { continue };
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), url.to_string()));
        }
    }
    out
}

/// What `git status --porcelain` says of the tree: files changed that git
/// tracks, and files it doesn't.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tree {
    pub changed: usize,
    pub untracked: usize,
}

pub fn tree(porcelain: &str) -> Tree {
    let mut t = Tree::default();
    for line in porcelain.lines().filter(|l| l.len() > 2) {
        if line.starts_with("??") {
            t.untracked += 1;
        } else if !line.starts_with("!!") {
            t.changed += 1;
        }
    }
    t
}

/// Where a branch for an issue would be made, and what you're told
/// before it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The remote that is the issue's repository or project.
    pub remote: String,
    /// The branch it starts from, `origin/main`, when the remote says
    /// which is its default; else what's checked out now.
    pub base: Option<String>,
    pub name: String,
    /// Said in the prompt, so Enter is a choice made knowing it: the
    /// changes that come along, or the branch that's already there.
    pub warning: Option<String>,
    /// The branch is there already: Enter switches to it.
    pub exists: bool,
}

/// What `git` printed in the project, for [`plan`].
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// `git remote -v`.
    pub remotes: String,
    /// `git status --porcelain`.
    pub status: String,
    /// `git symbolic-ref refs/remotes/<remote>/HEAD`, for each remote that
    /// has one: `refs/remotes/origin/main`.
    pub heads: Vec<(String, String)>,
    /// Local branches, `git branch --format=%(refname:short)`.
    pub branches: String,
}

/// Whether, where and how a branch for `issue` is made, from what git
/// printed, or why not.
pub fn plan(host: Host, web: &str, issue: &Issue, pattern: &str, seen: &Seen) -> Result<Plan, String> {
    let remotes = remotes(&seen.remotes);
    if remotes.is_empty() {
        return Err(format!("this project has no remote -- a branch for {} is made in a clone of {}", issue.key, issue.key.project));
    }
    let Some((remote, _)) = remotes.iter().find(|(_, url)| same_place(host, web, url, &issue.key.project)) else {
        let names: Vec<String> = remotes.iter().filter_map(|(_, url)| remote_parts(url).map(|(h, p)| format!("{h}/{p}"))).collect();
        return Err(format!("this project is {}, not {} -- open a clone of {} to start a branch for {}", names.join(", "), issue.key.project, issue.key.project, issue.key));
    };
    let name = name(pattern, issue);
    if name.is_empty() {
        return Err("the branch pattern made an empty name -- set another in the settings".into());
    }
    let exists = seen.branches.lines().any(|b| b.trim() == name);
    let base = seen.heads.iter().find(|(r, _)| r == remote).and_then(|(_, head)| head.trim().strip_prefix("refs/remotes/").map(str::to_string));
    let t = tree(&seen.status);
    let warning = match (exists, t.changed) {
        (true, 0) => Some(format!("{name} is there already: Enter switches to it")),
        (true, n) => Some(format!("{name} is there already: Enter switches to it, carrying your {n} uncommitted change(s) if git can")),
        (false, 0) => None,
        (false, n) => Some(format!("your {n} uncommitted change(s) come along to the new branch")),
    };
    Ok(Plan { remote: remote.clone(), base, name, warning, exists })
}

/// The git commands that make the plan so, in order: a fetch of the
/// remote's default branch, then the branch made from it (or switched
/// to). `name` is what you confirmed.
pub fn commands(plan: &Plan, name: &str) -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<String>>();
    if plan.exists && name == plan.name {
        return vec![s(&["switch", name])];
    }
    match &plan.base {
        Some(base) => {
            let branch = base.split_once('/').map_or(base.as_str(), |(_, b)| b);
            vec![s(&["fetch", &plan.remote, branch]), s(&["switch", "--no-track", "-c", name, base])]
        }
        None => vec![s(&["switch", "-c", name])],
    }
}

/// An issue's key, for the words: `acme/rocket#12`.
pub fn label(key: &IssueRef) -> String {
    key.key()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(project: &str, number: u64, title: &str) -> Issue {
        Issue { key: IssueRef::new(project, number), title: title.into(), open: true, ..Default::default() }
    }

    #[test]
    fn a_name_is_the_number_and_the_title_as_a_slug() {
        let i = issue("acme/rocket", 123, "Login times out: after 5 minutes! (Safari)");
        assert_eq!(name("", &i), "123-login-times-out-after-5-minutes-safari");
        assert_eq!(name("feature/{number}-{title}", &issue("a/b", 7, "Café crème")), "feature/7-cafe-creme");
        assert_eq!(name("{project}/{number}", &i), "rocket/123");
        assert_eq!(slug("A very long title that goes on and on and on past the end", 20), "a-very-long-title");
        assert_eq!(slug("Supercalifragilisticexpialidocious", 10), "supercalif");
        assert_eq!(name("", &issue("a/b", 9, "日本語")), "9", "nothing of the title left");
    }

    #[test]
    fn a_name_has_nothing_git_refuses() {
        assert_eq!(clean_name("  -feat ure..x~y^z:w?*[q\\@{u}.lock "), "feat-ure.x-y-z-w-q-u}");
        assert_eq!(clean_name("a//b/"), "a/b");
        assert_eq!(clean_name("x.lock.lock"), "x");
    }

    #[test]
    fn a_remote_is_read_in_each_form() {
        assert_eq!(remote_parts("git@github.com:acme/rocket.git"), Some(("github.com".into(), "acme/rocket".into())));
        assert_eq!(remote_parts("ssh://git@gitlab.example.com:2222/acme/platform/api.git"), Some(("gitlab.example.com".into(), "acme/platform/api".into())));
        assert_eq!(remote_parts("https://user:pw@GitHub.com/acme/rocket/"), Some(("github.com".into(), "acme/rocket".into())));
        assert_eq!(remote_parts("/srv/git/rocket"), None);
    }

    #[test]
    fn the_remote_has_to_be_the_issues_repository() {
        assert!(same_place(Host::GitHub, "https://github.com", "git@github.com:Acme/Rocket.git", "acme/rocket"));
        assert!(!same_place(Host::GitHub, "https://github.com", "git@github.com:acme/rocket-fork.git", "acme/rocket"));
        assert!(!same_place(Host::GitHub, "https://github.com", "git@ghe.example.com:acme/rocket.git", "acme/rocket"), "another host's repository of that name");
        assert!(same_place(Host::GitHub, "https://ghe.example.com", "https://ghe.example.com/acme/rocket.git", "acme/rocket"));
        assert!(same_place(Host::GitLab, "https://gitlab.example.com", "git@gitlab-ssh.internal:acme/platform/api.git", "acme/platform/api"), "GitLab by another name");
        assert!(!same_place(Host::GitLab, "https://gitlab.example.com", "git@gitlab.example.com:acme/platform.git", "acme/platform/api"));
    }

    #[test]
    fn a_plan_says_where_and_what_comes_along() {
        let i = issue("acme/rocket", 12, "Login times out");
        let mut seen = Seen { remotes: "fork\tgit@github.com:me/rocket.git (fetch)\norigin\tgit@github.com:acme/rocket.git (fetch)\norigin\tgit@github.com:acme/rocket.git (push)\n".into(), heads: vec![("origin".into(), "refs/remotes/origin/main\n".into())], ..Default::default() };
        let p = plan(Host::GitHub, "https://github.com", &i, "", &seen).unwrap();
        assert_eq!((p.remote.as_str(), p.base.as_deref(), p.name.as_str(), p.warning.clone(), p.exists), ("origin", Some("origin/main"), "12-login-times-out", None, false));
        assert_eq!(commands(&p, "12-login-times-out"), [vec!["fetch", "origin", "main"], vec!["switch", "--no-track", "-c", "12-login-times-out", "origin/main"]]);

        seen.status = " M src/a.rs\n?? notes.txt\nA  src/b.rs\n".into();
        let p = plan(Host::GitHub, "https://github.com", &i, "", &seen).unwrap();
        assert_eq!(p.warning.as_deref(), Some("your 2 uncommitted change(s) come along to the new branch"), "an untracked file isn't a change to carry");

        seen.branches = "main\n12-login-times-out\n".into();
        let p = plan(Host::GitHub, "https://github.com", &i, "", &seen).unwrap();
        assert!(p.exists && p.warning.as_deref().unwrap().starts_with("12-login-times-out is there already"));
        assert_eq!(commands(&p, "12-login-times-out"), [vec!["switch", "12-login-times-out"]]);
        assert_eq!(commands(&p, "12-other"), [vec!["fetch", "origin", "main"], vec!["switch", "--no-track", "-c", "12-other", "origin/main"]], "a name you typed instead is a new branch");

        let elsewhere = plan(Host::GitHub, "https://github.com", &issue("acme/web", 3, "x"), "", &seen).unwrap_err();
        assert_eq!(elsewhere, "this project is github.com/me/rocket, github.com/acme/rocket, not acme/web -- open a clone of acme/web to start a branch for acme/web#3");
        assert!(plan(Host::GitHub, "https://github.com", &i, "", &Seen::default()).unwrap_err().starts_with("this project has no remote"));
        let no_head = Seen { remotes: "origin\thttps://github.com/acme/rocket (fetch)\n".into(), ..Default::default() };
        let p = plan(Host::GitHub, "https://github.com", &i, "", &no_head).unwrap();
        assert_eq!(commands(&p, &p.name), [vec!["switch", "-c", "12-login-times-out"]]);
    }
}
