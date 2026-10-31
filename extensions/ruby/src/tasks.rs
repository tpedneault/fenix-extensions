//! The tasks (`SPC p t`): `bundle install`, the tests (RSpec's, Rails',
//! or Rake's `test`), RuboCop or Standard over the project and with its
//! corrections, Rails' server, and -- once `rake -T` has said what there
//! is -- each Rake task with a description. The project's gems run
//! through `bundle exec`. The project's `.env` goes to each; a task of
//! the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::rubocop::{self, Style};
use crate::tools::Tools;

/// How many of Rake's tasks get a row: a Rails app describes a hundred.
pub const RAKE_TASKS: usize = 60;

/// Rake, the project's when it pins it.
pub fn rake(project: &Project, tools: &Tools) -> Option<Command> {
    tools.gem_program(project, "rake", "rake")
}

/// `rake -T`'s tasks, by name: `rake build:checksum   # Generate ...`,
/// its arguments (`release[remote]`) left off.
pub fn parse_rake(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        // Each with its description after a `#`; not `rake aborted!`.
        let Some(rest) = line.strip_prefix("rake ").filter(|r| r.contains('#')) else { continue };
        let name = rest.split_whitespace().next().unwrap_or_default();
        let name = name.split('[').next().unwrap_or_default();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// The tasks for `project` in folder `root`, with what's on this machine
/// and the Rake tasks `rake -T` listed.
pub fn tasks(project: &Project, tools: &Tools, rake_tasks: &[String], root: &str) -> Tasks {
    let mut t = Tasks::default();
    if project.gemfile {
        t.add("bundle install", tools.bundle().arg("install"));
    }
    let rake = rake(project, tools);
    // Rails' own test runner, else RSpec's, else Rake's `test`.
    if project.rails && project.test {
        t.add("test", tools.ruby().args(["bin/rails", "test"]));
    } else if project.spec || project.has("rspec-core") {
        t.add("test", tools.gem_program(project, "rspec-core", "rspec").unwrap_or_else(|| Command::new("rspec")));
    } else if let (true, Some(r)) = (project.rakefile && project.test, &rake) {
        t.add("test", r.clone().arg("test"));
    }
    if let Some((style, cmd)) = rubocop::command(project, tools) {
        let fix = if style == Style::Standard { "--fix" } else { "--autocorrect" };
        t.add(style.program(), cmd.clone());
        t.add(&format!("{}: correct", style.program()), cmd.arg(fix));
    }
    if project.rails {
        t.add("rails server", tools.ruby().args(["bin/rails", "server"]));
    }
    if let Some(r) = &rake {
        for name in rake_tasks.iter().take(RAKE_TASKS) {
            t.add(&format!("rake {name}"), r.clone().arg(name));
        }
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn rakes_list_is_read() {
        let names = parse_rake(include_str!("samples/rake-T.txt"));
        assert_eq!(names, ["build", "build:checksum", "clean", "clobber", "hello", "install", "install:local", "release", "test"]);
        assert!(parse_rake("(in /project)\nrake aborted!\n").is_empty());
    }

    #[test]
    fn a_bundled_gem_runs_its_own_rspec_rubocop_and_rake() {
        let dir = crate::project::tests::gem_project("tasks");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=test\n").unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { bundle: Some("bundle".into()), ruby: Some("ruby".into()), ..Tools::default() };
        let t = tasks(&project, &tools, &["build".into(), "release".into()], &dir.to_string_lossy());
        assert_eq!(
            table(&t),
            rows(&[
                ("bundle install", "bundle install"),
                ("test", "bundle exec rspec"),
                ("rubocop", "bundle exec rubocop"),
                ("rubocop: correct", "bundle exec rubocop --autocorrect"),
                ("rake build", "bundle exec rake build"),
                ("rake release", "bundle exec rake release"),
            ])
        );
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "test".to_string())]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rails_runs_its_tests_and_server_and_standard_corrects() {
        let project = Project { gemfile: true, locked: true, rails: true, test: true, rakefile: true, gems: vec!["rails".into(), "standard".into()], ..Project::default() };
        let tools = Tools { bundle: Some("/r/bundle".into()), ruby: Some("/r/ruby".into()), rake: Some("/r/rake".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, &[], "/nowhere")),
            rows(&[
                ("bundle install", "/r/bundle install"),
                ("test", "/r/ruby bin/rails test"),
                ("standardrb", "/r/bundle exec standardrb"),
                ("standardrb: correct", "/r/bundle exec standardrb --fix"),
                ("rails server", "/r/ruby bin/rails server"),
            ]),
            "the lock doesn't pin rake, so it isn't the project's: no rake tasks until they're listed"
        );
    }

    #[test]
    fn a_folder_of_scripts_uses_what_is_installed() {
        let project = Project { rakefile: true, test: true, ..Project::default() };
        let tools = Tools { rake: Some("/g/rake".into()), rubocop: Some("/g/rubocop".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, &["test".into()], "/nowhere")),
            rows(&[("test", "/g/rake test"), ("rubocop", "/g/rubocop"), ("rubocop: correct", "/g/rubocop --autocorrect"), ("rake test", "/g/rake test")])
        );
        assert!(table(&tasks(&Project::default(), &Tools::default(), &[], "/nowhere")).is_empty());
    }
}
