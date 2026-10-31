//! What the pack knows of the project, read from its files: a Gemfile and
//! the gems Gemfile.lock pins, the Ruby it asks for (`.ruby-version`,
//! `.tool-versions`, the Gemfile's `ruby`), a Rakefile, its tests
//! (RSpec's `spec/`, minitest's `test/`), Rails, and Standard. Nothing is
//! run to know it.

use std::path::Path;

/// The Rakefile's names, as Rake looks for them.
const RAKEFILES: &[&str] = &["Rakefile", "rakefile", "Rakefile.rb", "rakefile.rb"];

/// What its files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub gemfile: bool,
    /// A Gemfile.lock: what `bundle check` holds the gems to.
    pub locked: bool,
    /// The gems Gemfile.lock pins, by name: the project's own and what
    /// they need.
    pub gems: Vec<String>,
    /// The Ruby it asks for, as lang-kit reads a requirement (`3.3.6`,
    /// `>=3.3,<4`), and the file that says so.
    pub ruby: Option<(String, &'static str)>,
    pub rakefile: bool,
    /// RSpec's `spec/`, minitest's `test/`.
    pub spec: bool,
    pub test: bool,
    /// `bin/rails`: Rails runs its tests and its server.
    pub rails: bool,
    /// A `.standard.yml`: Standard's, though its gem isn't in the bundle.
    pub standard_yml: bool,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let read = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let gemfile = read("Gemfile").or_else(|| read("gems.rb"));
        let lock = read("Gemfile.lock").or_else(|| read("gems.locked"));
        let ruby = read(".ruby-version").and_then(|t| ruby_version(&t)).map(|r| (r, ".ruby-version"))
            .or_else(|| read(".tool-versions").and_then(|t| tool_versions(&t)).map(|r| (r, ".tool-versions")))
            .or_else(|| gemfile.as_deref().and_then(gemfile_ruby).map(|r| (r, "Gemfile")));
        Project {
            gemfile: gemfile.is_some(),
            locked: lock.is_some(),
            gems: lock.as_deref().map(locked_gems).unwrap_or_default(),
            ruby,
            rakefile: RAKEFILES.iter().any(|f| root.join(f).is_file()),
            spec: root.join("spec").is_dir(),
            test: root.join("test").is_dir(),
            rails: root.join("bin/rails").is_file(),
            standard_yml: root.join(".standard.yml").is_file(),
        }
    }

    /// Whether Gemfile.lock pins `gem`.
    pub fn has(&self, gem: &str) -> bool {
        self.gems.iter().any(|g| g == gem)
    }

    /// Whether it's styled by Standard rather than RuboCop's own config.
    pub fn standard(&self) -> bool {
        self.has("standard") || self.standard_yml
    }
}

/// `.ruby-version`'s Ruby: `3.3.6`, `ruby-3.3.6`, `3.3`. None for
/// another Ruby (`jruby-9.4.8.0`), whose version isn't MRI's.
pub fn ruby_version(text: &str) -> Option<String> {
    let v = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let v = v.strip_prefix("ruby-").unwrap_or(v);
    v.starts_with(|c: char| c.is_ascii_digit()).then(|| v.to_string())
}

/// asdf's and mise's `.tool-versions`: its `ruby` line's first version.
pub fn tool_versions(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.split('#').next()?.trim().strip_prefix("ruby ")).and_then(|v| ruby_version(v.split_whitespace().next()?))
}

/// The Gemfile's `ruby "~> 3.3"` (or `ruby ">= 3.1", "< 4"`) as a
/// requirement. `ruby file: ".ruby-version"` says nothing of its own.
pub fn gemfile_ruby(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| l.starts_with("ruby ") || l.starts_with("ruby("))?;
    let quoted: Vec<&str> = line.split(['"', '\'']).skip(1).step_by(2).collect();
    let parts: Vec<String> = quoted.iter().filter(|q| q.contains(|c: char| c.is_ascii_digit())).map(|q| pessimistic(&q.replace(' ', ""))).collect();
    (!parts.is_empty() && !line.contains("file:")).then(|| parts.join(","))
}

/// RubyGems' `~> 3.3` as the range it is, `>=3.3,<4`: the last number
/// given may move, the one before it may not. Anything else as it is.
pub fn pessimistic(requirement: &str) -> String {
    let Some(v) = requirement.strip_prefix("~>") else { return requirement.to_string() };
    let numbers: Vec<u64> = v.split('.').map_while(|n| n.parse().ok()).collect();
    if numbers.len() < 2 {
        return format!(">={v}");
    }
    let mut upper = numbers[..numbers.len() - 1].to_vec();
    *upper.last_mut().unwrap_or(&mut 0) += 1;
    format!(">={v},<{}", upper.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
}

/// Gemfile.lock's gems: each `specs:` entry (`    rubocop (1.92.2)`, four
/// spaces in), in the GEM, PATH and GIT sections alike.
pub fn locked_gems(text: &str) -> Vec<String> {
    let mut gems: Vec<String> = text
        .lines()
        .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
        .filter_map(|l| l.trim().split(' ').next())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    gems.sort();
    gems.dedup();
    gems
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-ruby-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A gem's project: Gemfile, its lock (RSpec, RuboCop, Rake), a
    /// Rakefile and `spec/`.
    pub fn gem_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("Gemfile"), "source \"https://rubygems.org\"\n\nruby \"~> 3.3\"\n\ngemspec\n\ngem \"rake\", \"~> 13.0\"\n").unwrap();
        std::fs::write(dir.join("Gemfile.lock"), include_str!("samples/Gemfile.lock")).unwrap();
        std::fs::write(dir.join("Rakefile"), "require \"bundler/gem_tasks\"\n").unwrap();
        std::fs::create_dir_all(dir.join("spec")).unwrap();
        dir
    }

    #[test]
    fn the_lock_says_the_gems() {
        let gems = locked_gems(include_str!("samples/Gemfile.lock"));
        for gem in ["orbit", "rake", "rspec-core", "rubocop", "rubocop-ast", "unicode-emoji"] {
            assert!(gems.contains(&gem.to_string()), "{gem}: {gems:?}");
        }
        assert_eq!(gems.iter().filter(|g| *g == "rspec-support").count(), 1, "once, though three need it");
        assert!(!gems.iter().any(|g| g.contains('(') || g.contains('~')), "not their requirements: {gems:?}");
    }

    #[test]
    fn the_ruby_asked_for_is_read_from_where_it_is_said() {
        assert_eq!(ruby_version("ruby-3.3.6\n").as_deref(), Some("3.3.6"));
        assert_eq!(ruby_version("3.4\n").as_deref(), Some("3.4"));
        assert_eq!(ruby_version("jruby-9.4.8.0"), None, "not MRI");
        assert_eq!(tool_versions("nodejs 22.11.0\nruby 3.3.6 3.2.5 # both\n").as_deref(), Some("3.3.6"));
        assert_eq!(gemfile_ruby("source \"https://rubygems.org\"\nruby \"~> 3.3\"\n").as_deref(), Some(">=3.3,<4"));
        assert_eq!(pessimistic("~>3.3.1"), ">=3.3.1,<3.4");
        assert_eq!(gemfile_ruby("ruby '>= 3.1', '< 4'\n").as_deref(), Some(">=3.1,<4"));
        assert_eq!(gemfile_ruby("ruby file: \".ruby-version\"\n"), None);
        assert_eq!(gemfile_ruby("gem \"rails\"\n"), None);
    }

    #[test]
    fn a_projects_files_are_read() {
        let dir = gem_project("read");
        std::fs::write(dir.join(".ruby-version"), "3.3.6\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.gemfile && p.locked && p.rakefile && p.spec && !p.test && !p.rails);
        assert_eq!(p.ruby, Some(("3.3.6".to_string(), ".ruby-version")), "over the Gemfile's");
        assert!(p.has("rubocop") && !p.has("standard") && !p.standard());
        std::fs::remove_file(dir.join(".ruby-version")).unwrap();
        std::fs::write(dir.join(".standard.yml"), "ruby_version: 3.3\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.ruby, Some((">=3.3,<4".to_string(), "Gemfile")));
        assert!(p.standard());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()), Project::default(), "nothing there");
    }
}
