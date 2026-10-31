//! The project doctor's Ruby checks: Ruby, held to the version the
//! project asks for; Bundler and the gems Gemfile.lock pins (`bundle
//! check`), in a project with a Gemfile and its lock; the language server; and RuboCop
//! or Standard, the project's own when it pins it. Each that isn't right
//! has a fix row, where there's something to fix it with: RubyGems for a
//! gem, Bundler for the bundle. Ruby itself comes from your version
//! manager, which the pack doesn't pick for you.

use fenix_extension_api::{Command, Health, Section};
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::Project;
use crate::rubocop::Style;
use crate::tools::{hints, Server, Tools};
use crate::Settings;

/// The fix rows' commands, in the manifest.
pub const INSTALL_BUNDLER: &str = "ruby.install-bundler";
pub const BUNDLE_INSTALL: &str = "ruby.bundle-install";
pub const INSTALL_SERVER: &str = "ruby.install-server";
pub const INSTALL_RUBOCOP: &str = "ruby.install-rubocop";

/// The least Ruby the servers run on (Ruby LSP's).
pub const RUBY_SINCE: &str = "3.0";

/// How long `bundle check` and `bundle exec` have to answer: Bundler
/// reads the whole lock first.
pub const BUNDLE_MILLIS: u32 = 20_000;

/// `bundle check`: every gem the lock pins is installed, or the ones that
/// aren't.
pub fn installed(out: &Output) -> (Health, String) {
    if out.ok() {
        return (Health::Ok, "every gem in Gemfile.lock is installed".into());
    }
    let text = out.text();
    let missing: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_prefix("* ")).filter_map(|g| g.split(' ').next()).collect();
    if missing.is_empty() {
        return (Health::Warn, format!("bundle check failed: {}", out.why()));
    }
    let shown: Vec<&str> = missing.iter().take(3).copied().collect();
    let more = if missing.len() > 3 { format!(" and {} more", missing.len() - 3) } else { String::new() };
    let gems = if missing.len() == 1 { "gem isn't" } else { "gems aren't" };
    (Health::Warn, format!("{} {gems} installed: {}{more}", missing.len(), shown.join(", ")))
}

/// `name`, looked for where the version managers and Homebrew put it.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine.
pub fn programs(project: &Project, tools: &Tools, settings: &Settings) -> Vec<Program> {
    let mut out = Vec::new();

    // `RUBY_VERSION`, not `ruby -v`: 2.7's says `2.7.8p225`.
    let ruby = program("ruby", "Ruby").version_args(["-e", "print RUBY_VERSION"]);
    out.push(match &project.ruby {
        Some((wants, _)) => ruby.wants(wants),
        None => ruby.at_least(RUBY_SINCE),
    });

    if project.gemfile {
        out.push(fix(program("bundle", "Bundler"), "Install Bundler", INSTALL_BUNDLER, tools.install_gem("bundler")));
    }
    // Without a lock, `bundle check` resolves the Gemfile against
    // rubygems.org first.
    if project.gemfile && project.locked {
        let gems = program("bundle", "Gems").section(Section::Project).version_args(["check", "--dry-run"]).judge(installed).timeout(BUNDLE_MILLIS);
        out.push(fix(gems, "Install the gems (bundle install)", BUNDLE_INSTALL, Some(tools.bundle().arg("install"))));
    }

    // The language server: the one found, else the one asked for.
    let s = tools.server(project, settings.server).map(|(s, _)| s).unwrap_or(settings.server);
    let ls = if s == Server::Solargraph && project.has("solargraph") {
        program("bundle", "Language server").version_args(["exec", "solargraph", "--version"]).timeout(BUNDLE_MILLIS)
    } else {
        program(s.program(), "Language server")
    };
    out.push(fix(ls.optional(), &format!("Install {}", s.gem()), INSTALL_SERVER, tools.install_gem(s.gem())));

    // RuboCop, or Standard: the project's when it pins it, which `bundle
    // install` brings; else the one installed.
    if settings.format || settings.lint != crate::LintMode::Off {
        let style = Style::of(project);
        if project.has(style.gem()) {
            out.push(program("bundle", style.label()).version_args(["exec", style.program(), "--version"]).timeout(BUNDLE_MILLIS).optional());
        } else {
            let p = program(style.program(), style.label()).optional();
            out.push(fix(p, &format!("Install {}", style.gem()), INSTALL_RUBOCOP, tools.install_gem(style.gem())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn bundle_check_says_which_gems_are_missing() {
        assert_eq!(installed(&out(0, "The Gemfile's dependencies are satisfied\n", "")), (Health::Ok, "every gem in Gemfile.lock is installed".into()));
        let missing = "The following gems are missing\n * rspec (3.13.2)\n * rspec-core (3.13.6)\n * json (3.1.0)\n * racc (1.8.1)\nInstall missing gems with `bundle install`";
        assert_eq!(installed(&out(1, missing, "")), (Health::Warn, "4 gems aren't installed: rspec, rspec-core, json and 1 more".into()));
        assert_eq!(installed(&out(1, "The following gems are missing\n * rake (13.4.2)\n", "")).1, "1 gem isn't installed: rake");
        assert_eq!(installed(&out(10, "", "Could not locate Gemfile")).1, "bundle check failed: Could not locate Gemfile");
    }

    #[test]
    fn a_bundled_project_checks_its_ruby_its_gems_and_its_own_rubocop() {
        let dir = crate::project::tests::gem_project("doctor");
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { gem: Some("/r/gem".into()), bundle: Some("/r/bundle".into()), ..Tools::default() };
        let p = programs(&project, &tools, &Settings::default());
        assert_eq!(
            rows(&p),
            [
                ("Ruby".into(), "ruby".into(), true, None),
                ("Bundler".into(), "bundle".into(), true, Some(INSTALL_BUNDLER.into())),
                ("Gems".into(), "bundle".into(), true, Some(BUNDLE_INSTALL.into())),
                ("Language server".into(), "ruby-lsp".into(), false, Some(INSTALL_SERVER.into())),
                ("RuboCop".into(), "bundle".into(), false, None),
            ],
            "the lock's RuboCop comes with bundle install"
        );
        // The Gemfile's `ruby "~> 3.3"`.
        let c = p[0].evaluate(Some("/r/ruby"), Some(&out(0, "3.2.5", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "3.2.5, but the project needs >=3.3,<4 (/r/ruby)"));
        assert_eq!(p[0].evaluate(Some("/r/ruby"), Some(&out(0, "3.4.7", ""))).health, Health::Ok);
        assert_eq!(p[0].version_args, ["-e", "print RUBY_VERSION"]);
        assert_eq!(p[2].version_args, ["check", "--dry-run"], "never writing the lock");
        assert_eq!(p[4].version_args, ["exec", "rubocop", "--version"]);
        assert!(p[0].hints.contains(&"$HOME/.rbenv/shims".to_string()));
        let gems = p[2].evaluate(Some("/r/bundle"), Some(&out(1, "The following gems are missing\n * rake (13.4.2)\n", "")));
        assert_eq!((gems.health, gems.fix.map(|f| f.1)), (Health::Warn, Some(BUNDLE_INSTALL.into())));
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/r/bundle install"),
            other => panic!("{other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_of_scripts_checks_ruby_the_server_and_rubocop() {
        let dir = temp("scripts");
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { solargraph: Some("/g/solargraph".into()), ..Tools::default() };
        let p = programs(&project, &tools, &Settings::default());
        assert_eq!(
            rows(&p),
            [("Ruby".into(), "ruby".into(), true, None), ("Language server".into(), "solargraph".into(), false, None), ("RuboCop".into(), "rubocop".into(), false, None)],
            "Solargraph's found, so it's the one checked; no RubyGems, no fixes"
        );
        assert_eq!(p[0].evaluate(Some("/usr/bin/ruby"), Some(&out(0, "2.7.8", ""))).health, Health::Bad, "3.0 at least");
        let with_gem = Tools { gem: Some("/r/gem".into()), ..Tools::default() };
        let p = programs(&Project { standard_yml: true, ..project.clone() }, &with_gem, &Settings::default());
        assert_eq!(rows(&p)[2], ("Standard".into(), "standardrb".into(), false, Some(INSTALL_RUBOCOP.into())));
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/r/gem install standard"),
            other => panic!("{other:?}"),
        }
        let off = Settings { format: false, lint: crate::LintMode::Off, ..Settings::default() };
        assert_eq!(programs(&project, &with_gem, &off).len(), 2, "neither formats nor lints: not checked");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
