//! Which forge a checkout's CI is on, and where: its `origin` remote,
//! read from `.git/config`, and the settings (a provider forced, GitHub
//! Enterprise's address, a GitLab's).

use crate::model::Provider;

/// Where the CI is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub provider: Provider,
    /// GitHub's API root (`https://api.github.com`, `https://ghe/api/v3`),
    /// or the GitLab instance's root (`https://gitlab.example.com`).
    pub api: String,
    /// `owner/repo`, or GitLab's `group/sub/project`.
    pub project: String,
    /// The forge's host, for its token (`gh auth token --hostname`).
    pub host: String,
}

/// What the settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `github`, `gitlab`, or none to tell by the remote.
    pub provider: Option<Provider>,
    /// GitHub Enterprise Server's address (`https://github.acme.dev`).
    pub github_url: Option<String>,
    /// Your GitLab's address: this extension's setting, else Fenix's own
    /// (`gitlab.base_url`).
    pub gitlab_url: Option<String>,
    /// Your Jenkins's address (`https://ci.example.com`).
    pub jenkins_url: Option<String>,
    /// The project's job there: `space/rocket`, or its address.
    pub jenkins_job: Option<String>,
    /// The keys that open Fenix's settings, as the key scheme in use has
    /// them (`SPC ,`, `C-,`): where what's missing is set.
    pub settings_keys: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { provider: None, github_url: None, gitlab_url: None, jenkins_url: None, jenkins_job: None, settings_keys: "SPC ,".to_string() }
    }
}

/// The URL of a remote in a `.git/config`: `origin`'s, else the first.
pub fn remote_url(config: &str) -> Option<String> {
    let mut section: Option<String> = None;
    let mut urls: Vec<(String, String)> = Vec::new();
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if let Some(head) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            // [remote "origin"]
            section = head.trim().strip_prefix("remote").map(|n| n.trim().trim_matches('"').to_string()).filter(|_| head.trim().starts_with("remote"));
            continue;
        }
        if let (Some(name), Some((key, value))) = (&section, line.split_once('=')) {
            if key.trim().eq_ignore_ascii_case("url") {
                let value = value.trim().trim_matches('"').to_string();
                if !urls.iter().any(|(n, _)| n == name) {
                    urls.push((name.clone(), value));
                }
            }
        }
    }
    urls.iter().find(|(n, _)| n == "origin").or(urls.first()).map(|(_, u)| u.clone())
}

/// The host a remote URL is on: `git@host:path`, `ssh://git@host:22/path`,
/// `https://user@host/path`.
pub fn remote_host(url: &str) -> Option<String> {
    let url = url.trim();
    let authority = match url.split_once("://") {
        Some((_, rest)) => rest.split('/').next()?,
        None => url.split_once(':')?.0,
    };
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// A URL's host.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map(|(_, r)| r).unwrap_or(url.trim());
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Whether the settings say the CI is a Jenkins: asked for, or -- with
/// no provider chosen -- a job named.
pub fn is_jenkins(config: &Config) -> bool {
    config.provider.map(|p| p == Provider::Jenkins).unwrap_or(nonempty(&config.jenkins_job).is_some())
}

/// The Jenkins job the settings name. A remote has no part in it.
pub fn jenkins(config: &Config) -> Result<Repo, String> {
    let settings = &config.settings_keys;
    let base = nonempty(&config.jenkins_url).ok_or_else(|| format!("Jenkins: set its address (ci.jenkins_url, like https://ci.example.com) in {settings} (CI status)."))?;
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err(format!("Jenkins: its address (ci.jenkins_url) is {base}: it starts with https:// or http://"));
    }
    let job = nonempty(&config.jenkins_job).ok_or_else(|| format!("Jenkins: name this project's job (ci.jenkins_job, like space/rocket) in {settings} (CI status)."))?;
    let path = crate::jenkins::job_path(job);
    if path.is_empty() {
        return Err(format!("Jenkins: couldn't read a job out of ci.jenkins_job ({job}): its folders and its name, like space/rocket"));
    }
    let host = url_host(base).ok_or_else(|| format!("Jenkins: couldn't read a host out of ci.jenkins_url ({base})"))?;
    Ok(Repo { provider: Provider::Jenkins, api: crate::jenkins::job_url(base, &path), project: path.join("/"), host })
}

/// Where the CI is: the Jenkins job the settings name, else the forge
/// `remote` is on.
pub fn locate(remote: Option<&str>, config: &Config) -> Result<Repo, String> {
    if is_jenkins(config) {
        return jenkins(config);
    }
    resolve(remote.ok_or("This repository has no remote to find its CI by.")?, config)
}

/// Where `remote`'s CI is, as the settings say.
pub fn resolve(remote: &str, config: &Config) -> Result<Repo, String> {
    if config.provider == Some(Provider::Jenkins) {
        return jenkins(config);
    }
    let host = remote_host(remote).ok_or_else(|| format!("couldn't read a host out of the remote's URL ({remote})"))?;
    let project = fenix_forge::gitlab::remote::project_path(remote).ok_or_else(|| format!("couldn't read a project out of the remote's URL ({remote})"))?;
    let github_url = nonempty(&config.github_url);
    let gitlab_url = nonempty(&config.gitlab_url);
    let is_host = |url: Option<&str>| url.and_then(url_host).is_some_and(|h| h == host);
    let provider = config.provider.unwrap_or(if host == "github.com" || is_host(github_url) {
        Provider::GitHub
    } else if host == "gitlab.com" || is_host(gitlab_url) {
        Provider::GitLab
    } else if host.contains("github") {
        Provider::GitHub
    } else {
        // GitLab is routinely self-hosted under any name, and reached
        // over an SSH alias: as the Review extension does, anything else.
        Provider::GitLab
    });
    let repo = match provider {
        Provider::GitHub => {
            if project.matches('/').count() != 1 {
                return Err(format!("{project} isn't a GitHub owner/repo"));
            }
            let web = if host == "github.com" { "https://github.com".to_string() } else { github_url.map(str::to_string).unwrap_or_else(|| format!("https://{host}")) };
            Repo { provider, api: crate::github::api_root(&web), project, host: url_host(&web).unwrap_or(host) }
        }
        Provider::GitLab => {
            let base = if host == "gitlab.com" && !is_host(gitlab_url) { "https://gitlab.com".to_string() } else { gitlab_url.map(str::to_string).unwrap_or_else(|| format!("https://{host}")) };
            let base = base.trim_end_matches('/').to_string();
            Repo { provider, host: url_host(&base).unwrap_or(host), api: base, project }
        }
        Provider::Jenkins => return jenkins(config),
    };
    Ok(repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[core]
	repositoryformatversion = 0
	bare = false
[remote "upstream"]
	url = git@github.com:acme/rocket.git
	fetch = +refs/heads/*:refs/remotes/upstream/*
[remote "origin"]
	url = https://alex@gitlab.example.com/space/ground/rocket.git
	fetch = +refs/heads/*:refs/remotes/origin/*
[branch "main"]
	remote = origin
	merge = refs/heads/main
"#;

    #[test]
    fn origin_is_read_out_of_git_config() {
        assert_eq!(remote_url(CONFIG).as_deref(), Some("https://alex@gitlab.example.com/space/ground/rocket.git"));
        assert_eq!(remote_url("[remote \"fork\"]\n url = git@github.com:me/r.git\n").as_deref(), Some("git@github.com:me/r.git"), "else the first remote");
        assert_eq!(remote_url("[core]\n bare = false\n"), None);
    }

    #[test]
    fn hosts_are_read_out_of_every_kind_of_remote() {
        assert_eq!(remote_host("git@github.com:acme/rocket.git").as_deref(), Some("github.com"));
        assert_eq!(remote_host("ssh://git@gitlab.example.com:2222/g/p.git").as_deref(), Some("gitlab.example.com"));
        assert_eq!(remote_host("https://alex@GitLab.Example.com/g/p.git").as_deref(), Some("gitlab.example.com"));
        assert_eq!(url_host("https://github.acme.dev:8443/x").as_deref(), Some("github.acme.dev"));
    }

    #[test]
    fn github_com_and_gitlab_com_are_known_by_their_hosts() {
        let none = Config::default();
        let r = resolve("git@github.com:acme/rocket.git", &none).unwrap();
        assert_eq!((r.provider, r.api.as_str(), r.project.as_str()), (Provider::GitHub, "https://api.github.com", "acme/rocket"));
        let r = resolve("https://gitlab.com/space/ground/rocket.git", &none).unwrap();
        assert_eq!((r.provider, r.api.as_str(), r.project.as_str()), (Provider::GitLab, "https://gitlab.com", "space/ground/rocket"));
    }

    #[test]
    fn enterprise_and_self_hosted_forges_come_from_the_settings() {
        let config = Config { github_url: Some("https://github.acme.dev/".into()), gitlab_url: Some("https://gitlab.example.com".into()), ..Config::default() };
        let r = resolve("git@github.acme.dev:acme/rocket.git", &config).unwrap();
        assert_eq!((r.provider, r.api.as_str(), r.host.as_str()), (Provider::GitHub, "https://github.acme.dev/api/v3", "github.acme.dev"));
        // An SSH alias for the GitLab: the setting's address is used.
        let r = resolve("git@work:space/ground/rocket.git", &config).unwrap();
        assert_eq!((r.provider, r.api.as_str()), (Provider::GitLab, "https://gitlab.example.com"));
    }

    #[test]
    fn a_jenkins_job_comes_from_the_settings_whatever_the_remote() {
        let config = Config { jenkins_url: Some("https://ci.example.com/jenkins/".into()), jenkins_job: Some("space/rocket".into()), ..Config::default() };
        let r = locate(Some("git@github.com:acme/rocket.git"), &config).unwrap();
        assert_eq!((r.provider, r.api.as_str(), r.project.as_str(), r.host.as_str()), (Provider::Jenkins, "https://ci.example.com/jenkins/job/space/job/rocket", "space/rocket", "ci.example.com"));
        assert_eq!(locate(None, &config).unwrap(), r, "no remote needed");
        // Another provider chosen: the job named isn't asked.
        let github = Config { provider: Some(Provider::GitHub), ..config.clone() };
        assert_eq!(locate(Some("git@github.com:acme/rocket.git"), &github).unwrap().provider, Provider::GitHub);
        // Jenkins chosen, and what's missing is said.
        let forced = Config { provider: Some(Provider::Jenkins), ..Config::default() };
        assert_eq!(locate(None, &forced).unwrap_err(), "Jenkins: set its address (ci.jenkins_url, like https://ci.example.com) in SPC , (CI status).");
        let standard = Config { settings_keys: "C-,".into(), ..forced.clone() };
        assert_eq!(locate(None, &standard).unwrap_err(), "Jenkins: set its address (ci.jenkins_url, like https://ci.example.com) in C-, (CI status).", "standard keys' settings key");
        let no_job = Config { jenkins_url: Some("https://ci.example.com".into()), ..forced.clone() };
        assert!(locate(None, &no_job).unwrap_err().contains("ci.jenkins_job"));
        let no_job = Config { settings_keys: "C-k ,".into(), ..no_job };
        assert!(locate(None, &no_job).unwrap_err().ends_with("in C-k , (CI status)."));
        let bare = Config { jenkins_url: Some("ci.example.com".into()), jenkins_job: Some("rocket".into()), ..forced };
        assert!(bare.jenkins_url.is_some() && locate(None, &bare).unwrap_err().contains("https://"));
        assert_eq!(locate(None, &Config::default()).unwrap_err(), "This repository has no remote to find its CI by.");
    }

    #[test]
    fn the_provider_can_be_forced() {
        let config = Config { provider: Some(Provider::GitHub), ..Config::default() };
        let r = resolve("git@code.internal:acme/rocket.git", &config).unwrap();
        assert_eq!((r.provider, r.api.as_str()), (Provider::GitHub, "https://code.internal/api/v3"));
        assert!(resolve("git@code.internal:a/b/c.git", &config).is_err(), "GitHub has no subgroups");
        assert!(resolve("not a remote", &Config::default()).is_err());
    }
}
