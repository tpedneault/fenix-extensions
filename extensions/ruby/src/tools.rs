//! The programs the pack runs, found on this machine: Ruby, RubyGems,
//! Bundler, Rake, the language servers (Ruby LSP, Solargraph), RuboCop
//! and Standard. rbenv's, asdf's and mise's shims, Homebrew's Ruby and
//! its gems, and RubyInstaller's folders may not be on the `PATH` of an
//! editor started from the desktop, so they're looked in too. A gem the
//! project's Gemfile.lock pins runs as the project's, through `bundle
//! exec`.

use fenix_extension_api::Command;
use fenix_lang_kit::Host;

use crate::project::Project;

/// Where else to look: the version managers' shims, Homebrew's Ruby and
/// the gems it installs, RubyInstaller's.
pub const HINTS: &[&str] = &[
    "$HOME/.rbenv/shims", "$HOME/.asdf/shims", "$HOME/.local/share/mise/shims", "$HOME/.local/bin", "$HOME/.gem/bin",
    "/opt/homebrew/opt/ruby/bin", "/opt/homebrew/lib/ruby/gems/4.0.0/bin", "/opt/homebrew/lib/ruby/gems/3.4.0/bin",
    "/usr/local/opt/ruby/bin", "/usr/local/lib/ruby/gems/4.0.0/bin", "/usr/local/lib/ruby/gems/3.4.0/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "C:\\Ruby40-x64\\bin", "C:\\Ruby34-x64\\bin", "C:\\Ruby33-x64\\bin", "%LOCALAPPDATA%\\mise\\shims",
];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// The language server to prefer (`ruby.server`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    RubyLsp,
    Solargraph,
}

impl Server {
    pub fn parse(text: &str) -> Server {
        if text.trim().eq_ignore_ascii_case("solargraph") {
            Server::Solargraph
        } else {
            Server::RubyLsp
        }
    }

    /// Its gem, and its program.
    pub fn gem(self) -> &'static str {
        match self {
            Server::RubyLsp => "ruby-lsp",
            Server::Solargraph => "solargraph",
        }
    }

    pub fn program(self) -> &'static str {
        self.gem()
    }

    pub fn other(self) -> Server {
        match self {
            Server::RubyLsp => Server::Solargraph,
            Server::Solargraph => Server::RubyLsp,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub ruby: Option<String>,
    pub gem: Option<String>,
    pub bundle: Option<String>,
    pub rake: Option<String>,
    pub ruby_lsp: Option<String>,
    pub solargraph: Option<String>,
    pub rubocop: Option<String>,
    pub standardrb: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut find = |name: &str| host.find_program(name, &hints);
        Tools {
            ruby: find("ruby"),
            gem: find("gem"),
            bundle: find("bundle"),
            rake: find("rake"),
            ruby_lsp: find("ruby-lsp"),
            solargraph: find("solargraph"),
            rubocop: find("rubocop"),
            standardrb: find("standardrb"),
        }
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "ruby" => &mut self.ruby,
            "gem" => &mut self.gem,
            "bundle" => &mut self.bundle,
            "rake" => &mut self.rake,
            "ruby-lsp" => &mut self.ruby_lsp,
            "solargraph" => &mut self.solargraph,
            "rubocop" => &mut self.rubocop,
            "standardrb" => &mut self.standardrb,
            _ => return None,
        })
    }

    /// Bundler, by its path when found, else by its name.
    pub fn bundle(&self) -> Command {
        Command::new(self.bundle.as_deref().unwrap_or("bundle"))
    }

    pub fn ruby(&self) -> Command {
        Command::new(self.ruby.as_deref().unwrap_or("ruby"))
    }

    /// `program`, of gem `gem`: the project's (`bundle exec`) when its
    /// Gemfile.lock pins it, else the one installed; none when neither.
    pub fn gem_program(&self, project: &Project, gem: &str, program: &str) -> Option<Command> {
        if project.has(gem) && self.bundle.is_some() {
            return Some(self.bundle().args(["exec", program]));
        }
        self.found(program).map(Command::new)
    }

    /// Where `name` was found.
    pub fn found(&self, name: &str) -> Option<&str> {
        match name {
            "ruby" => self.ruby.as_deref(),
            "gem" => self.gem.as_deref(),
            "bundle" => self.bundle.as_deref(),
            "rake" => self.rake.as_deref(),
            "ruby-lsp" => self.ruby_lsp.as_deref(),
            "solargraph" => self.solargraph.as_deref(),
            "rubocop" => self.rubocop.as_deref(),
            "standardrb" => self.standardrb.as_deref(),
            _ => None,
        }
    }

    /// The server to use, and its command: the one asked for, else the
    /// other, when there's one. Ruby LSP runs as it's installed, not by
    /// `bundle exec`: it makes a bundle of its own with the project's
    /// gems in it. Solargraph runs as the project's when it pins it.
    pub fn server(&self, project: &Project, wanted: Server) -> Option<(Server, Command)> {
        [wanted, wanted.other()].into_iter().find_map(|s| match s {
            Server::RubyLsp => self.ruby_lsp.as_deref().map(|p| (s, Command::new(p))),
            Server::Solargraph => self.gem_program(project, "solargraph", "solargraph").map(|c| (s, c.arg("stdio"))),
        })
    }

    /// Installing `gem` for you (`gem install`); none without RubyGems.
    pub fn install_gem(&self, gem: &str) -> Option<Command> {
        self.gem.as_ref().map(|g| Command::new(g).args(["install", gem]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn locked(gems: &[&str]) -> Project {
        Project { gemfile: true, locked: true, gems: gems.iter().map(|g| g.to_string()).collect(), ..Project::default() }
    }

    #[test]
    fn a_gem_the_lock_pins_runs_through_bundle_exec() {
        let t = Tools { bundle: Some("/r/bundle".into()), rubocop: Some("/g/rubocop".into()), ..Tools::default() };
        assert_eq!(t.gem_program(&locked(&["rubocop"]), "rubocop", "rubocop").map(|c| line(&c)).as_deref(), Some("/r/bundle exec rubocop"));
        assert_eq!(t.gem_program(&locked(&["rspec-core"]), "rubocop", "rubocop").map(|c| line(&c)).as_deref(), Some("/g/rubocop"), "not the project's: the one installed");
        assert_eq!(t.gem_program(&Project::default(), "standard", "standardrb"), None);
        let no_bundler = Tools { bundle: None, ..t };
        assert_eq!(no_bundler.gem_program(&locked(&["rubocop"]), "rubocop", "rubocop").map(|c| line(&c)).as_deref(), Some("/g/rubocop"), "no Bundler to run the project's");
    }

    #[test]
    fn the_server_asked_for_is_used_or_else_the_other() {
        let t = Tools { bundle: Some("/r/bundle".into()), solargraph: Some("/g/solargraph".into()), ..Tools::default() };
        let (s, cmd) = t.server(&Project::default(), Server::RubyLsp).unwrap();
        assert_eq!((s, line(&cmd).as_str()), (Server::Solargraph, "/g/solargraph stdio"));
        let (_, cmd) = t.server(&locked(&["solargraph"]), Server::Solargraph).unwrap();
        assert_eq!(line(&cmd), "/r/bundle exec solargraph stdio");
        let lsp = Tools { ruby_lsp: Some("/g/ruby-lsp".into()), ..t };
        assert_eq!(lsp.server(&locked(&["ruby-lsp"]), Server::RubyLsp).map(|(_, c)| line(&c)).as_deref(), Some("/g/ruby-lsp"), "never through bundle exec");
        assert_eq!(Tools::default().server(&Project::default(), Server::RubyLsp), None);
        assert_eq!(Server::parse(" Solargraph"), Server::Solargraph);
        assert_eq!(Server::parse(""), Server::RubyLsp);
    }

    #[test]
    fn gems_are_installed_with_rubygems() {
        let t = Tools { gem: Some("/r/gem".into()), ..Tools::default() };
        assert_eq!(t.install_gem("ruby-lsp").map(|c| line(&c)).as_deref(), Some("/r/gem install ruby-lsp"));
        assert_eq!(Tools::default().install_gem("rubocop"), None, "no RubyGems");
        assert!(hints().contains(&"$HOME/.rbenv/shims".to_string()));
    }
}
